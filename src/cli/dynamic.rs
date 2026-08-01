//! Dynamic tool/subcommand argument parsing from CommandDef.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::coerce::coerce_value;
use crate::error::{Error, Result};
use crate::model::{CommandDef, ParamType};

#[derive(Debug, Clone)]
pub struct ParsedToolArgs {
    pub command: CommandDef,
    pub values: BTreeMap<String, Value>,
    pub stdin: bool,
}

/// Parse `remaining` as `<subcommand> [--flag value ...]`.
pub fn parse_tool_args(commands: &[CommandDef], remaining: &[String]) -> Result<ParsedToolArgs> {
    if remaining.is_empty() {
        return Err(Error::usage(
            "no subcommand specified. Use --list to see available tools.",
        ));
    }
    if remaining[0] == "-h" || remaining[0] == "--help" {
        return Err(Error::usage("use --list to see available commands"));
    }

    let name = &remaining[0];
    let cmd = commands
        .iter()
        .find(|c| &c.name == name)
        .cloned()
        .ok_or_else(|| Error::usage(format!("unknown command: {name}")))?;

    if remaining.get(1).map(String::as_str) == Some("-h")
        || remaining.get(1).map(String::as_str) == Some("--help")
    {
        print_command_help(&cmd);
        return Err(Error::usage("__help__"));
    }

    let mut values = BTreeMap::new();
    let mut stdin = false;
    let mut i = 1;
    while i < remaining.len() {
        let arg = &remaining[i];
        if arg == "--stdin" {
            stdin = true;
            i += 1;
            continue;
        }
        if !arg.starts_with("--") {
            return Err(Error::usage(format!("unexpected argument: {arg}")));
        }
        let flag = arg.trim_start_matches("--");
        let param = cmd
            .params
            .iter()
            .find(|p| p.name == flag)
            .ok_or_else(|| Error::usage(format!("unknown option: {arg}")))?;

        match param.python_type {
            ParamType::Boolean if param.location == crate::model::ParamLocation::GraphqlArg => {
                // GraphQL booleans: bare `--flag` ⇒ true; `--flag true|false` also accepted.
                i += 1;
                if let Some(raw) = remaining.get(i) {
                    let lower = raw.to_lowercase();
                    if matches!(lower.as_str(), "true" | "false" | "1" | "0" | "yes" | "no") {
                        let coerced = coerce_value(Some(Value::String(raw.clone())), &param.schema)
                            .unwrap_or(Value::Bool(matches!(lower.as_str(), "true" | "1" | "yes")));
                        values.insert(param.original_name.clone(), coerced);
                        i += 1;
                        continue;
                    }
                }
                values.insert(param.original_name.clone(), Value::Bool(true));
            }
            ParamType::Boolean => {
                values.insert(param.original_name.clone(), Value::Bool(true));
                i += 1;
            }
            other => {
                i += 1;
                let Some(raw) = remaining.get(i) else {
                    return Err(Error::usage(format!("option {arg} requires a value")));
                };
                if let Some(choices) = &param.choices {
                    if !choices.iter().any(|c| c == raw) {
                        return Err(Error::usage(format!(
                            "invalid value {raw:?} for {arg}; choices: {choices:?}"
                        )));
                    }
                }
                let as_value = match other {
                    ParamType::Integer => json!(raw.parse::<i64>().map_err(|_| {
                        Error::usage(format!("invalid integer for {arg}: {raw}"))
                    })?),
                    ParamType::Float => json!(raw.parse::<f64>().map_err(|_| {
                        Error::usage(format!("invalid number for {arg}: {raw}"))
                    })?),
                    ParamType::String | ParamType::Boolean => Value::String(raw.clone()),
                };
                let coerced = coerce_value(Some(as_value), &param.schema)
                    .unwrap_or(Value::String(raw.clone()));
                values.insert(param.original_name.clone(), coerced);
                i += 1;
            }
        }
    }

    // Required path/query/header params (body/tool_input can use --stdin)
    for p in &cmd.params {
        if !p.required {
            continue;
        }
        if matches!(
            p.location,
            crate::model::ParamLocation::Body
                | crate::model::ParamLocation::ToolInput
                | crate::model::ParamLocation::GraphqlArg
        ) {
            continue;
        }
        if p.python_type == ParamType::Boolean {
            continue;
        }
        if !values.contains_key(&p.original_name) && !stdin {
            return Err(Error::usage(format!(
                "missing required argument: --{}",
                p.name
            )));
        }
    }

    Ok(ParsedToolArgs {
        command: cmd,
        values,
        stdin,
    })
}

pub fn print_command_help(cmd: &CommandDef) {
    println!("{}", cmd.name);
    if !cmd.description.is_empty() {
        println!("  {}", cmd.description);
    }
    println!();
    if cmd.has_body {
        println!("  --stdin    Read JSON body/arguments from stdin");
    }
    for p in &cmd.params {
        let req = if p.required { " (required)" } else { "" };
        let ty = p.python_type.type_name();
        println!("  --{:<20} [{}]{} {}", p.name, ty, req, p.description);
    }
}

pub fn read_stdin_json(context: &str) -> Result<Value> {
    use std::io::Read;
    let mut raw = String::new();
    std::io::stdin()
        .read_to_string(&mut raw)
        .map_err(Error::from)?;
    if raw.trim().is_empty() {
        return Err(Error::runtime(format!(
            "--stdin expects JSON for {context}, but stdin was empty."
        )));
    }
    serde_json::from_str(&raw).map_err(|exc| {
        Error::runtime(format!(
            "invalid JSON on stdin for {context} (line {}, column {}).",
            exc.line(),
            exc.column()
        ))
    })
}
