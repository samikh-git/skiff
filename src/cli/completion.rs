//! Shell completion scripts and dynamic candidate helpers.
//!
//! ```text
//! skiff completion bash|zsh|fish   # print installable script
//! skiff __complete KIND [prefix]   # one candidate per line (used by scripts)
//! ```

use std::ffi::OsString;
use std::io::{self, Write};

use crate::bake::load_baked_all;
use crate::error::{Error, Result};

/// Top-level: `skiff completion <shell>`
pub fn handle_completion(argv: &[OsString]) -> Result<()> {
    let shell = argv
        .first()
        .and_then(|a| a.to_str())
        .ok_or_else(|| Error::usage("usage: skiff completion <bash|zsh|fish>"))?;
    match shell {
        "bash" => print!("{}", BASH_SCRIPT),
        "zsh" => print!("{}", ZSH_SCRIPT),
        "fish" => print!("{}", FISH_SCRIPT),
        "-h" | "--help" => {
            println!("Usage: skiff completion <bash|zsh|fish>");
            println!();
            println!("Print a shell completion script to stdout. Examples:");
            println!("  skiff completion bash >> ~/.bashrc");
            println!("  skiff completion zsh  > ~/.zsh/completions/_skiff");
            println!("  skiff completion fish > ~/.config/fish/completions/skiff.fish");
            return Err(Error::usage("__printed__"));
        }
        other => {
            return Err(Error::usage(format!(
                "unknown shell {other:?}; expected bash|zsh|fish"
            )));
        }
    }
    Ok(())
}

/// Hidden: `skiff __complete <kind> [prefix]`
pub fn handle_complete_helper(argv: &[OsString]) -> Result<()> {
    let kind = argv
        .first()
        .and_then(|a| a.to_str())
        .ok_or_else(|| Error::usage("usage: skiff __complete <kind> [prefix]"))?;
    let prefix = argv
        .get(1)
        .and_then(|a| a.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let mut out = io::stdout().lock();
    match kind {
        "bake-names" => {
            for name in bake_names() {
                if prefix.is_empty() || name.starts_with(&prefix) {
                    writeln!(out, "{name}")?;
                }
            }
        }
        "bake-names-at" => {
            // Candidates include leading `@` for top-level completion.
            for name in bake_names() {
                let cand = format!("@{name}");
                if prefix.is_empty()
                    || cand.starts_with(&prefix)
                    || name.starts_with(prefix.trim_start_matches('@'))
                {
                    writeln!(out, "{cand}")?;
                }
            }
        }
        "session-names" => {
            for name in session_names() {
                if prefix.is_empty() || name.to_ascii_lowercase().starts_with(&prefix) {
                    writeln!(out, "{name}")?;
                }
            }
        }
        "bake-cmds" => {
            for cmd in BAKE_CMDS {
                if prefix.is_empty() || cmd.starts_with(prefix.as_str()) {
                    writeln!(out, "{cmd}")?;
                }
            }
        }
        "top-cmds" => {
            for cmd in TOP_CMDS {
                if prefix.is_empty() || cmd.starts_with(prefix.as_str()) {
                    writeln!(out, "{cmd}")?;
                }
            }
        }
        "flags" => {
            for flag in GLOBAL_FLAGS {
                if prefix.is_empty() || flag.starts_with(prefix.as_str()) {
                    writeln!(out, "{flag}")?;
                }
            }
        }
        "detail" => write_enum(&mut out, &["names", "brief", "full"], &prefix)?,
        "transport" => write_enum(&mut out, &["auto", "sse", "streamable"], &prefix)?,
        "sort" => write_enum(&mut out, &["usage", "recent", "alpha", "default"], &prefix)?,
        "oauth-flow" => write_enum(
            &mut out,
            &["auto", "authorization_code", "client_credentials"],
            &prefix,
        )?,
        "import-from" => write_enum(&mut out, &["auto", "cursor", "claude", "codex"], &prefix)?,
        other => {
            return Err(Error::usage(format!(
                "unknown __complete kind {other:?}; expected bake-names|bake-names-at|session-names|bake-cmds|top-cmds|flags|detail|transport|sort|oauth-flow|import-from"
            )));
        }
    }
    Ok(())
}

fn write_enum(out: &mut impl Write, values: &[&str], prefix: &str) -> Result<()> {
    for v in values {
        if prefix.is_empty() || v.starts_with(prefix) {
            writeln!(out, "{v}")?;
        }
    }
    Ok(())
}

fn bake_names() -> Vec<String> {
    load_baked_all().unwrap_or_default().into_keys().collect()
}

fn session_names() -> Vec<String> {
    #[cfg(unix)]
    {
        crate::session::session_list()
            .unwrap_or_default()
            .into_iter()
            .map(|e| e.name)
            .collect()
    }
    #[cfg(not(unix))]
    {
        Vec::new()
    }
}

const TOP_CMDS: &[&str] = &["bake", "doctor", "completion"];
const BAKE_CMDS: &[&str] = &[
    "create", "list", "show", "remove", "update", "install", "import",
];

const GLOBAL_FLAGS: &[&str] = &[
    "--spec",
    "--mcp",
    "--mcp-stdio",
    "--graphql",
    "--auth-header",
    "--base-url",
    "--list",
    "--search",
    "--detail",
    "--describe",
    "--agent",
    "--json",
    "--envelope",
    "--toon",
    "--pretty",
    "--raw",
    "--head",
    "--compact",
    "--verbose",
    "--top",
    "--sort",
    "--max-bytes",
    "--inline",
    "--spool-clean",
    "--fields",
    "--transport",
    "--env",
    "--refresh",
    "--cache-ttl",
    "--cache-key",
    "--oauth",
    "--oauth-client-id",
    "--oauth-client-secret",
    "--oauth-flow",
    "--oauth-clear",
    "--session",
    "--session-start",
    "--session-stop",
    "--session-list",
    "--session-idle-secs",
    "--session-clean-env",
    "--list-resources",
    "--list-prompts",
    "--read-resource",
    "--get-prompt",
    "--prompt-arg",
    "--help",
    "--version",
];

const BASH_SCRIPT: &str = r#"# skiff bash completion — eval "$(skiff completion bash)"
_skiff_complete() {
  local cur="${COMP_WORDS[COMP_CWORD]}"
  local prev="${COMP_WORDS[COMP_CWORD-1]}"
  local cmd="skiff"

  case "${prev}" in
    --detail) COMPREPLY=( $(compgen -W "$(${cmd} __complete detail 2>/dev/null)" -- "${cur}") ); return ;;
    --transport) COMPREPLY=( $(compgen -W "$(${cmd} __complete transport 2>/dev/null)" -- "${cur}") ); return ;;
    --sort) COMPREPLY=( $(compgen -W "$(${cmd} __complete sort 2>/dev/null)" -- "${cur}") ); return ;;
    --oauth-flow) COMPREPLY=( $(compgen -W "$(${cmd} __complete oauth-flow 2>/dev/null)" -- "${cur}") ); return ;;
    --session|--session-start|--session-stop)
      COMPREPLY=( $(compgen -W "$(${cmd} __complete session-names 2>/dev/null)" -- "${cur}") ); return ;;
    --from) COMPREPLY=( $(compgen -W "$(${cmd} __complete import-from 2>/dev/null)" -- "${cur}") ); return ;;
    bake)
      COMPREPLY=( $(compgen -W "$(${cmd} __complete bake-cmds 2>/dev/null)" -- "${cur}") ); return ;;
    show|remove|update|install)
      if [[ "${COMP_WORDS[1]}" == "bake" ]]; then
        COMPREPLY=( $(compgen -W "$(${cmd} __complete bake-names 2>/dev/null)" -- "${cur}") ); return
      fi
      ;;
    completion)
      COMPREPLY=( $(compgen -W "bash zsh fish" -- "${cur}") ); return ;;
  esac

  if [[ "${COMP_WORDS[1]}" == "bake" && "${COMP_WORDS[2]}" == "import" ]]; then
    case "${prev}" in
      --from) COMPREPLY=( $(compgen -W "$(${cmd} __complete import-from 2>/dev/null)" -- "${cur}") ); return ;;
      --name) COMPREPLY=( $(compgen -W "$(${cmd} __complete bake-names 2>/dev/null)" -- "${cur}") ); return ;;
    esac
    COMPREPLY=( $(compgen -W "--from --path --name --force --dry-run --help" -- "${cur}") ); return
  fi

  if [[ ${cur} == @* ]]; then
    COMPREPLY=( $(compgen -W "$(${cmd} __complete bake-names-at 2>/dev/null)" -- "${cur}") ); return
  fi

  if [[ ${cur} == -* ]]; then
    COMPREPLY=( $(compgen -W "$(${cmd} __complete flags 2>/dev/null)" -- "${cur}") ); return
  fi

  if [[ ${COMP_CWORD} -eq 1 ]]; then
    local tops
    tops="$(${cmd} __complete top-cmds 2>/dev/null) $(${cmd} __complete bake-names-at 2>/dev/null)"
    COMPREPLY=( $(compgen -W "${tops}" -- "${cur}") )
  fi
}
complete -F _skiff_complete skiff
"#;

const ZSH_SCRIPT: &str = r#"#compdef skiff
# skiff zsh completion — place as _skiff on fpath, or: eval "$(skiff completion zsh)"

_skiff() {
  local -a cmds bake_cmds shells
  local cur="${words[CURRENT]}"
  local prev="${words[CURRENT-1]}"

  case "${prev}" in
    --detail)
      _values 'detail' names brief full
      return
      ;;
    --transport)
      _values 'transport' auto sse streamable
      return
      ;;
    --sort)
      _values 'sort' usage recent alpha default
      return
      ;;
    --oauth-flow)
      _values 'oauth-flow' auto authorization_code client_credentials
      return
      ;;
    --session|--session-start|--session-stop)
      _values 'session' ${(f)"$(skiff __complete session-names 2>/dev/null)"}
      return
      ;;
    --from)
      _values 'from' auto cursor claude codex
      return
      ;;
    bake)
      bake_cmds=( ${(f)"$(skiff __complete bake-cmds 2>/dev/null)"} )
      _describe -t bake-cmds 'bake command' bake_cmds
      return
      ;;
    show|remove|update|install)
      if [[ "${words[2]}" == "bake" ]]; then
        _values 'bake name' ${(f)"$(skiff __complete bake-names 2>/dev/null)"}
        return
      fi
      ;;
    completion)
      _values 'shell' bash zsh fish
      return
      ;;
  esac

  if [[ "${words[2]}" == "bake" && "${words[3]}" == "import" ]]; then
    if [[ "${prev}" == "--name" ]]; then
      _values 'server' ${(f)"$(skiff __complete bake-names 2>/dev/null)"}
      return
    fi
    _arguments \
      '--from[source]:from:(auto cursor claude codex)' \
      '--path[config file]:file:_files' \
      '--name[server name]:name:' \
      '--force[overwrite]' \
      '--dry-run[preview]' \
      '(-h --help)'{-h,--help}'[help]'
    return
  fi

  if [[ "${cur}" == @* ]]; then
    _values 'baked tool' ${(f)"$(skiff __complete bake-names-at 2>/dev/null)"}
    return
  fi

  if [[ "${cur}" == -* ]]; then
    _values 'flag' ${(f)"$(skiff __complete flags 2>/dev/null)"}
    return
  fi

  if (( CURRENT == 2 )); then
    cmds=( ${(f)"$(skiff __complete top-cmds 2>/dev/null)"} )
    cmds+=( ${(f)"$(skiff __complete bake-names-at 2>/dev/null)"} )
    _describe -t commands 'skiff' cmds
  fi
}

_skiff "$@"
"#;

const FISH_SCRIPT: &str = r#"# skiff fish completion — skiff completion fish > ~/.config/fish/completions/skiff.fish

function __skiff_bake_names
  skiff __complete bake-names 2>/dev/null
end

function __skiff_bake_names_at
  skiff __complete bake-names-at 2>/dev/null
end

function __skiff_session_names
  skiff __complete session-names 2>/dev/null
end

complete -c skiff -f

complete -c skiff -n '__fish_use_subcommand' -a 'bake' -d 'Manage baked configs'
complete -c skiff -n '__fish_use_subcommand' -a 'doctor' -d 'Install / cache diagnostics'
complete -c skiff -n '__fish_use_subcommand' -a 'completion' -d 'Print shell completion script'
complete -c skiff -n '__fish_use_subcommand' -a '(__skiff_bake_names_at)' -d 'Baked tool'

complete -c skiff -n '__fish_seen_subcommand_from bake' -a 'create list show remove update install import'

complete -c skiff -n '__fish_seen_subcommand_from show remove update install' -a '(__skiff_bake_names)'

complete -c skiff -n '__fish_seen_subcommand_from completion' -a 'bash zsh fish'

complete -c skiff -l detail -xa 'names brief full'
complete -c skiff -l transport -xa 'auto sse streamable'
complete -c skiff -l sort -xa 'usage recent alpha default'
complete -c skiff -l oauth-flow -xa 'auto authorization_code client_credentials'
complete -c skiff -l session -xa '(__skiff_session_names)'
complete -c skiff -l session-start -xa '(__skiff_session_names)'
complete -c skiff -l session-stop -xa '(__skiff_session_names)'
complete -c skiff -l from -xa 'auto cursor claude codex'

complete -c skiff -l spec -r
complete -c skiff -l mcp -r
complete -c skiff -l mcp-stdio -r
complete -c skiff -l graphql -r
complete -c skiff -l list
complete -c skiff -l search -r
complete -c skiff -l describe -r
complete -c skiff -l agent
complete -c skiff -l json
complete -c skiff -l envelope
complete -c skiff -l toon
complete -c skiff -l pretty
complete -c skiff -l help
complete -c skiff -l version
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bake::{create_baked, BakedTool};
    use crate::paths::{set_config_dir_override, TEST_PATHS_LOCK};
    use tempfile::TempDir;

    #[test]
    fn complete_bake_names_respects_prefix() {
        let _g = TEST_PATHS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().unwrap();
        set_config_dir_override(Some(dir.path().to_path_buf()));
        create_baked(
            "alpha-tool",
            BakedTool {
                source_type: "mcp".into(),
                source: "https://example.com".into(),
                ..Default::default()
            },
            true,
        )
        .unwrap();
        create_baked(
            "beta-tool",
            BakedTool {
                source_type: "mcp".into(),
                source: "https://example.com".into(),
                ..Default::default()
            },
            true,
        )
        .unwrap();

        let names = bake_names();
        assert!(names.contains(&"alpha-tool".into()));
        assert!(names.iter().filter(|n| n.starts_with("alpha")).count() == 1);
        set_config_dir_override(None);
    }

    #[test]
    fn completion_help_shells() {
        let err = handle_completion(&[OsString::from("--help")]).unwrap_err();
        assert!(matches!(err, Error::Usage(m) if m == "__printed__"));
    }
}
