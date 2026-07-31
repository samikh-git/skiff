//! --list output formatting.

use crate::model::{command_to_json, CommandDef};
use crate::output::{output_result, OutputOptions};
use crate::usage::{resolve_sort_mode, sort_commands};
use serde_json::Value;

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
        .filter(|c| {
            c.name.to_lowercase().contains(&p) || c.description.to_lowercase().contains(&p)
        })
        .collect()
}

pub fn list_commands(
    commands: &[CommandDef],
    opts: &ListOptions,
) -> crate::error::Result<()> {
    if opts.json_output {
        let payload = if opts.compact {
            Value::Array(
                commands
                    .iter()
                    .map(|c| Value::String(c.name.clone()))
                    .collect(),
            )
        } else {
            Value::Array(commands.iter().map(command_to_json).collect())
        };
        output_result(
            payload,
            &OutputOptions {
                pretty: opts.pretty,
                json_output: true,
                ..Default::default()
            },
        )?;
        return Ok(());
    }

    if opts.compact {
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
            // Group by first kebab segment
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
                    let method = cmd
                        .method
                        .as_deref()
                        .unwrap_or("")
                        .to_uppercase();
                    let desc = truncate_desc(&cmd.description, if opts.verbose { 10_000 } else { 60 });
                    if desc.is_empty() {
                        println!("  {:<45} {:<6}", cmd.name, method);
                    } else {
                        println!("  {:<45} {:<6} {desc}", cmd.name, method);
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
}

#[derive(Debug, Clone)]
pub struct ListOptions {
    pub verbose: bool,
    pub compact: bool,
    pub json_output: bool,
    pub pretty: bool,
    pub style: ListStyle,
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
