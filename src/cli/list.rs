//! --list output formatting.

use crate::model::{CommandDef, ListDetail};
use crate::output::{output_result, OutputOptions};
use crate::usage::{resolve_sort_mode, sort_commands};

pub fn apply_list_options(
    commands: Vec<CommandDef>,
    source_hash: &str,
    sort_mode: Option<&str>,
    top: Option<usize>,
) -> Vec<CommandDef> {
    let mode = resolve_sort_mode(sort_mode, source_hash);
    let mut commands = sort_commands(commands, &mode, source_hash);
    if let Some(n) = top {
        commands.truncate(n);
    }
    commands
}

pub fn filter_by_search(commands: Vec<CommandDef>, pattern: &str) -> Vec<CommandDef> {
    let p = pattern.to_lowercase();
    commands
        .into_iter()
        .filter(|c| c.name.to_lowercase().contains(&p) || c.description.to_lowercase().contains(&p))
        .collect()
}

pub fn list_commands(commands: &[CommandDef], opts: &ListOptions) -> crate::error::Result<()> {
    if opts.json_output || opts.toon {
        let detail = if opts.compact {
            ListDetail::Names
        } else {
            opts.detail
        };
        let payload = crate::model::commands_to_json(commands, detail);
        output_result(
            payload,
            &OutputOptions {
                pretty: opts.pretty,
                json_output: opts.json_output || !opts.toon,
                toon: opts.toon,
                max_bytes: opts.max_bytes,
                inline: opts.inline,
                ..Default::default()
            },
        )?;
        return Ok(());
    }

    if opts.compact || opts.detail == ListDetail::Names {
        let names: Vec<_> = commands.iter().map(|c| c.name.as_str()).collect();
        println!("{}", names.join(" "));
        return Ok(());
    }

    match opts.style {
        ListStyle::Mcp => {
            for cmd in commands {
                let desc = truncate_desc(&cmd.description, if opts.verbose { 10_000 } else { 70 });
                if desc.is_empty() {
                    println!("  {:<40}", cmd.name);
                } else {
                    println!("  {:<40}  {desc}", cmd.name);
                }
            }
        }
        ListStyle::OpenApi => {
            let mut groups: std::collections::BTreeMap<String, Vec<&CommandDef>> =
                std::collections::BTreeMap::new();
            for cmd in commands {
                let prefix = cmd
                    .name
                    .split_once('-')
                    .map(|(a, _)| a.to_string())
                    .unwrap_or_else(|| "other".into());
                groups.entry(prefix).or_default().push(cmd);
            }
            for (group, cmds) in groups {
                println!("\n{group}:");
                for cmd in cmds {
                    let method = cmd.method.as_deref().unwrap_or("").to_uppercase();
                    let desc =
                        truncate_desc(&cmd.description, if opts.verbose { 10_000 } else { 60 });
                    if desc.is_empty() {
                        println!("  {:<45} {:<6}", cmd.name, method);
                    } else {
                        println!("  {:<45} {:<6} {desc}", cmd.name, method);
                    }
                }
            }
        }
        ListStyle::Graphql => {
            let mut groups: std::collections::BTreeMap<&str, Vec<&CommandDef>> =
                std::collections::BTreeMap::new();
            for cmd in commands {
                let key = cmd.graphql_operation_type.as_deref().unwrap_or("other");
                groups.entry(key).or_default().push(cmd);
            }
            for group in ["query", "mutation"] {
                let Some(cmds) = groups.get(group) else {
                    continue;
                };
                if cmds.is_empty() {
                    continue;
                }
                let label = if group == "query" {
                    "queries"
                } else {
                    "mutations"
                };
                println!("\n{label}:");
                for cmd in cmds {
                    let desc =
                        truncate_desc(&cmd.description, if opts.verbose { 10_000 } else { 60 });
                    if desc.is_empty() {
                        println!("  {:<40}", cmd.name);
                    } else {
                        println!("  {:<40}  {desc}", cmd.name);
                    }
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub enum ListStyle {
    Mcp,
    OpenApi,
    Graphql,
}

#[derive(Debug, Clone)]
pub struct ListOptions {
    pub verbose: bool,
    pub compact: bool,
    pub json_output: bool,
    pub pretty: bool,
    pub toon: bool,
    pub detail: ListDetail,
    pub max_bytes: Option<usize>,
    pub inline: bool,
    pub style: ListStyle,
}

impl ListOptions {
    pub fn from_global(pre: &crate::cli::args::GlobalArgs, style: ListStyle) -> Self {
        Self {
            verbose: pre.verbose,
            compact: pre.compact,
            json_output: pre.json_output || pre.agent,
            pretty: pre.pretty,
            toon: pre.toon,
            detail: pre.list_detail(),
            max_bytes: pre.max_bytes,
            inline: pre.inline,
            style,
        }
    }
}

fn truncate_desc(description: &str, max_len: usize) -> String {
    if description.len() <= max_len {
        return description.to_string();
    }
    let truncated = &description[..max_len];
    match truncated.rsplit_once(' ') {
        Some((head, _)) => format!("{head}..."),
        None => format!("{truncated}..."),
    }
}

/// Emit one-tool schema (JSON/TOON) or human help.
pub fn emit_command_help(
    cmd: &CommandDef,
    pre: &crate::cli::args::GlobalArgs,
) -> crate::error::Result<()> {
    if pre.json_output || pre.toon || pre.agent {
        output_result(crate::model::command_to_json(cmd), &pre.output_options())?;
    } else {
        crate::cli::dynamic::print_command_help(cmd);
    }
    Ok(())
}

/// If remaining is `TOOL --help`/`-h`, emit help and return true.
pub fn maybe_tool_help(
    commands: &[CommandDef],
    remaining: &[String],
    pre: &crate::cli::args::GlobalArgs,
) -> crate::error::Result<bool> {
    if remaining.len() >= 2 && (remaining[1] == "--help" || remaining[1] == "-h") {
        if let Some(cmd) = commands.iter().find(|c| c.name == remaining[0]) {
            emit_command_help(cmd, pre)?;
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn describe_tool(
    commands: &[CommandDef],
    name: &str,
    pre: &crate::cli::args::GlobalArgs,
) -> crate::error::Result<()> {
    let cmd = commands
        .iter()
        .find(|c| c.name == name || c.tool_name.as_deref() == Some(name))
        .ok_or_else(|| crate::error::Error::usage(format!("unknown tool: {name}")))?;
    // describe always structured
    let mut opts = pre.output_options();
    opts.json_output = true;
    output_result(crate::model::command_to_json(cmd), &opts)?;
    Ok(())
}
