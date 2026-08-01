//! stdout is data only; diagnostics go to stderr.
//!
//! Supports JSON, raw, pretty, head, native TOON, max-bytes spill-to-spool,
//! and light TTY sanitization for human (non-structured) paths.

use std::io::{self, IsTerminal, Write};

use serde_json::Value;

use crate::coerce::apply_head;
use crate::spool::{self, pointer_json, write_spool};

#[derive(Debug, Clone, Default)]
pub struct OutputOptions {
    pub pretty: bool,
    pub raw: bool,
    pub toon: bool,
    pub head: Option<usize>,
    pub json_output: bool,
    /// When set, spill rendered output larger than this to spool (None / Some(0) = never).
    pub max_bytes: Option<usize>,
    /// Force full inline output even if over max_bytes.
    pub inline: bool,
}

/// Strip common ANSI CSI / OSC sequences for human TTY display.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for c2 in chars.by_ref() {
                        if c2.is_ascii_uppercase() || c2.is_ascii_lowercase() {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    for c2 in chars.by_ref() {
                        if c2 == '\u{7}' || c2 == '\u{1b}' {
                            break;
                        }
                    }
                }
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
        } else if c == '\0' {
            // drop NULs
        } else {
            out.push(c);
        }
    }
    out
}

fn emit_json(data: &Value, pretty: bool) -> io::Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if pretty || io::stdout().is_terminal() {
        serde_json::to_writer_pretty(&mut out, data)?;
        writeln!(out)?;
    } else {
        serde_json::to_writer(&mut out, data)?;
        writeln!(out)?;
    }
    Ok(())
}

fn render_json_bytes(data: &Value, pretty: bool) -> io::Result<Vec<u8>> {
    if pretty {
        let mut v = serde_json::to_vec_pretty(data)?;
        v.push(b'\n');
        Ok(v)
    } else {
        let mut v = serde_json::to_vec(data)?;
        v.push(b'\n');
        Ok(v)
    }
}

fn encode_toon(data: &Value) -> Result<String, String> {
    toon_format::encode(data, &toon_format::EncodeOptions::default()).map_err(|e| e.to_string())
}

fn should_spill(opts: &OutputOptions, len: usize) -> bool {
    if opts.inline {
        return false;
    }
    match opts.max_bytes {
        Some(0) | None => false,
        Some(n) => len > n,
    }
}

fn spill_and_print_pointer(bytes: &[u8], kind: &str, preview: &str) -> io::Result<()> {
    spool::maybe_clean_expired();
    let path = write_spool(bytes, kind).map_err(io::Error::other)?;
    eprintln!(
        "mcp2cli: output {} bytes exceeded limit; spooled to {}",
        bytes.len(),
        path.display()
    );
    let ptr = pointer_json(&path, bytes, preview);
    // Pointer is always compact JSON for reliable agent parsing.
    let stdout = io::stdout();
    let mut out = stdout.lock();
    serde_json::to_writer(&mut out, &ptr)?;
    writeln!(out)?;
    Ok(())
}

/// Print result respecting output flags (JSON / TOON / raw / head / spool).
pub fn output_result(data: Value, opts: &OutputOptions) -> io::Result<()> {
    let mut data = data;

    if let Value::String(s) = &data {
        if opts.json_output || opts.toon {
            if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                data = parsed;
            }
        } else if !opts.raw {
            match serde_json::from_str::<Value>(s) {
                Ok(parsed) => data = parsed,
                Err(_) => {
                    let text = if io::stdout().is_terminal() {
                        strip_ansi(s)
                    } else {
                        s.clone()
                    };
                    if should_spill(opts, text.len()) {
                        return spill_and_print_pointer(text.as_bytes(), "txt", &text);
                    }
                    println!("{text}");
                    return Ok(());
                }
            }
        }
    }

    if opts.raw {
        match data {
            Value::String(s) => {
                if should_spill(opts, s.len()) {
                    return spill_and_print_pointer(s.as_bytes(), "txt", &s);
                }
                println!("{s}");
                return Ok(());
            }
            other => {
                let s = serde_json::to_string(&other)?;
                if should_spill(opts, s.len()) {
                    return spill_and_print_pointer(s.as_bytes(), "json", &s);
                }
                println!("{s}");
                return Ok(());
            }
        }
    }

    if let Some(n) = opts.head {
        data = apply_head(data, n);
    }

    if opts.toon {
        match encode_toon(&data) {
            Ok(toon) => {
                let mut bytes = toon.into_bytes();
                bytes.push(b'\n');
                if should_spill(opts, bytes.len()) {
                    let preview = String::from_utf8_lossy(&bytes).into_owned();
                    return spill_and_print_pointer(&bytes, "toon", &preview);
                }
                let stdout = io::stdout();
                let mut out = stdout.lock();
                out.write_all(&bytes)?;
                return Ok(());
            }
            Err(e) => {
                eprintln!("Warning: --toon encode failed ({e}); falling back to JSON");
                // fall through to JSON
            }
        }
    }

    // Structured JSON path (explicit --json, or default non-raw after parse)
    if opts.json_output || opts.toon || !matches!(&data, Value::String(_)) {
        let pretty = opts.pretty;
        // Agents / pipes: compact unless --pretty or (TTY and not forced json for machines)
        let use_pretty = pretty || (io::stdout().is_terminal() && !opts.json_output && !opts.toon);
        let bytes = render_json_bytes(&data, use_pretty)?;
        if should_spill(opts, bytes.len()) {
            let preview = String::from_utf8_lossy(&bytes).into_owned();
            return spill_and_print_pointer(&bytes, "json", &preview);
        }
        let stdout = io::stdout();
        let mut out = stdout.lock();
        out.write_all(&bytes)?;
        return Ok(());
    }

    emit_json(&data, opts.pretty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strip_ansi_removes_csi() {
        let s = "\u{1b}[31mred\u{1b}[0m plain";
        assert_eq!(strip_ansi(s), "red plain");
    }

    #[test]
    fn encode_toon_uniform_array() {
        let data = json!([{"id": 1, "name": "a"}, {"id": 2, "name": "b"}]);
        let t = encode_toon(&data).unwrap();
        assert!(t.contains("id") && t.contains("name"));
    }

    #[test]
    fn options_construct() {
        let opts = OutputOptions {
            pretty: true,
            head: Some(2),
            max_bytes: Some(100),
            ..Default::default()
        };
        assert_eq!(opts.head, Some(2));
        assert_eq!(opts.max_bytes, Some(100));
    }
}
