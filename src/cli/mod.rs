//! CLI parsing helpers (two-pass argv split) and dispatch.

pub mod args;
pub mod bake;
pub mod dispatch;
pub mod dynamic;
pub mod list;

pub use dispatch::dispatch;

use std::collections::HashSet;
use std::ffi::OsString;

/// Split argv into `(global_args, tool_args)` at the first positional subcommand.
///
/// Mirrors Python `_split_at_subcommand` so tool flags like `--env` are not
/// swallowed by the global parser (upstream GH #15).
pub fn split_at_subcommand(
    argv: &[OsString],
    value_options: &HashSet<String>,
    bool_options: &HashSet<String>,
) -> (Vec<OsString>, Vec<OsString>) {
    let _ = bool_options; // retained for API parity / future stricter splitting
    let mut i = 0;
    while i < argv.len() {
        let arg = argv[i].to_string_lossy();
        if arg == "--" {
            return (argv[..i].to_vec(), argv[i + 1..].to_vec());
        }
        if arg.starts_with('-') {
            if arg.starts_with("--") && arg.contains('=') {
                i += 1;
            } else if value_options.contains(arg.as_ref()) {
                i += 2;
            } else {
                // Bool flags and unknown dashed tokens stay in the global slice.
                i += 1;
            }
        } else {
            return (argv[..i].to_vec(), argv[i..].to_vec());
        }
    }
    (argv.to_vec(), Vec::new())
}

/// Global flag sets for the pre-parser (subset used for splitting).
pub fn global_option_sets() -> (HashSet<String>, HashSet<String>) {
    let value = [
        "--spec",
        "--mcp",
        "--mcp-stdio",
        "--graphql",
        "--auth-header",
        "--base-url",
        "--cache-key",
        "--cache-ttl",
        "--search",
        "--sort",
        "--top",
        "--head",
        "--fields",
        "--transport",
        "--env",
        "--oauth-client-id",
        "--oauth-client-secret",
        "--oauth-client-name",
        "--oauth-scope",
        "--oauth-redirect-uri",
        "--oauth-flow",
        "--read-resource",
        "--get-prompt",
        "--prompt-arg",
        "--session-start",
        "--session-stop",
        "--session",
        "--session-idle-secs",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();

    let bools = [
        "--refresh",
        "--list",
        "--verbose",
        "--compact",
        "--pretty",
        "--raw",
        "--json",
        "--toon",
        "--oauth",
        "--oauth-clear",
        "--list-resources",
        "--list-resource-templates",
        "--list-prompts",
        "--session-list",
        "--session-clean-env",
        "--version",
        "-V",
        "-h",
        "--help",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();

    (value, bools)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(s: &str) -> OsString {
        OsString::from(s)
    }

    #[test]
    fn splits_before_subcommand() {
        let (value, bools) = global_option_sets();
        let argv = vec![
            os("--mcp"),
            os("http://example.com"),
            os("--list"),
            os("search"),
            os("--query"),
            os("hi"),
        ];
        let (global, tool) = split_at_subcommand(&argv, &value, &bools);
        assert_eq!(
            global,
            vec![os("--mcp"), os("http://example.com"), os("--list")]
        );
        assert_eq!(tool, vec![os("search"), os("--query"), os("hi")]);
    }

    #[test]
    fn double_dash_separator() {
        let (value, bools) = global_option_sets();
        let argv = vec![os("--spec"), os("a.json"), os("--"), os("--env"), os("x=1")];
        let (global, tool) = split_at_subcommand(&argv, &value, &bools);
        assert_eq!(global, vec![os("--spec"), os("a.json")]);
        assert_eq!(tool, vec![os("--env"), os("x=1")]);
    }
}
