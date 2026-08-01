//! stdout is data only; diagnostics go to stderr.
//!
//! `--toon` is accepted for CLI parity but currently warns and emits JSON.

use std::io::{self, IsTerminal, Write};

use serde_json::Value;

use crate::coerce::apply_head;

#[derive(Debug, Clone, Default)]
pub struct OutputOptions {
    pub pretty: bool,
    pub raw: bool,
    pub toon: bool,
    pub head: Option<usize>,
    pub json_output: bool,
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

/// Print result respecting `--json` / `--raw` / `--pretty` / `--head` / `--toon`.
///
/// `--toon` warns and falls back to JSON until a native encoder ships.
pub fn output_result(data: Value, opts: &OutputOptions) -> io::Result<()> {
    if opts.json_output {
        let mut data = data;
        if let Value::String(s) = &data {
            if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                data = parsed;
            }
        }
        if let Some(n) = opts.head {
            data = apply_head(data, n);
        }
        return emit_json(&data, opts.pretty);
    }

    if opts.raw {
        match data {
            Value::String(s) => {
                println!("{s}");
                return Ok(());
            }
            other => {
                println!("{}", serde_json::to_string(&other)?);
                return Ok(());
            }
        }
    }

    let mut data = data;
    if let Value::String(s) = &data {
        match serde_json::from_str::<Value>(s) {
            Ok(parsed) => data = parsed,
            Err(_) => {
                println!("{s}");
                return Ok(());
            }
        }
    }

    if let Some(n) = opts.head {
        data = apply_head(data, n);
    }

    if opts.toon {
        eprintln!(
            "Warning: --toon requires the TOON CLI (@toon-format/cli). \
             Install with: npm install -g @toon-format/cli"
        );
        // Fall through to JSON for M1.
    }

    emit_json(&data, opts.pretty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn apply_head_via_options_path() {
        // smoke: ensure options construct
        let opts = OutputOptions {
            pretty: true,
            head: Some(2),
            ..Default::default()
        };
        assert_eq!(opts.head, Some(2));
        let _ = json!([1, 2, 3]);
    }
}
