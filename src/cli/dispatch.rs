//! Top-level CLI dispatch.

use std::ffi::OsString;

use clap::Parser;
use serde_json::{Map, Value};

use crate::bake::require_baked;
use crate::cache::cache_key_for;
use crate::cli::args::GlobalArgs;
use crate::cli::bake::handle_bake;
use crate::cli::dynamic::{parse_tool_args, print_command_help, read_stdin_json};
use crate::cli::list::{
    apply_list_options, filter_by_search, list_commands, ListOptions, ListStyle,
};
use crate::cli::{global_option_sets, split_at_subcommand};
use crate::error::{Error, Result};
use crate::filter::filter_commands;
use crate::mcp::{
    call_tool_http, call_tool_stdio, fetch_mcp_tools_http, fetch_mcp_tools_stdio, tools_to_commands,
    TransportMode,
};
use crate::model::BakeConfig;
use crate::oauth::{
    clear_oauth_credentials, discovery_url_from_args, oauth_wanted, options_from_args,
    setup_from_args, OAuthReady,
};
use crate::openapi::{
    execute_openapi, extract_openapi_commands, load_openapi_spec, resolve_base_url,
};
use crate::graphql::{execute_graphql, extract_graphql_commands, load_graphql_schema};
use crate::output::output_result;
use crate::usage::{record_usage, source_hash_for};

pub fn dispatch(argv: Vec<OsString>) -> Result<()> {
    if argv.first().and_then(|a| a.to_str()) == Some("bake") {
        return handle_bake(&argv[1..]);
    }
    if let Some(first) = argv.first().and_then(|a| a.to_str()) {
        if let Some(name) = first.strip_prefix('@') {
            return run_baked(name, &argv[1..]);
        }
    }

    dispatch_impl(argv, None)
}

fn run_baked(name: &str, rest: &[OsString]) -> Result<()> {
    let cfg = require_baked(name)?;
    let bake_config = cfg.bake_config();
    let mut synthetic: Vec<OsString> = cfg.to_argv().into_iter().map(OsString::from).collect();
    synthetic.extend_from_slice(rest);
    dispatch_impl(synthetic, Some(bake_config))
}

fn dispatch_impl(argv: Vec<OsString>, bake_config: Option<BakeConfig>) -> Result<()> {
    let (value_opts, bool_opts) = global_option_sets();
    let (global_argv, tool_argv) = split_at_subcommand(&argv, &value_opts, &bool_opts);

    let mut clap_argv = vec![OsString::from("mcp2cli")];
    clap_argv.extend(global_argv);

    let mut pre_args = GlobalArgs::try_parse_from(&clap_argv).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("help") || e.kind() == clap::error::ErrorKind::DisplayHelp {
            e.print().ok();
            Error::usage("__printed__")
        } else if e.kind() == clap::error::ErrorKind::DisplayVersion {
            e.print().ok();
            Error::usage("__printed__")
        } else {
            Error::usage(msg)
        }
    })?;

    if pre_args.search_pattern.is_some() {
        pre_args.list_commands = true;
    }

    // --oauth-clear can run with just a discovery URL source flag.
    if pre_args.oauth_clear {
        let url = discovery_url_from_args(&pre_args)?;
        clear_oauth_credentials(&url)?;
        println!("Cleared OAuth credentials for {url}");
        return Ok(());
    }

    // Validate oauth flags early (e.g. secret without id, stdio+oauth).
    let _ = options_from_args(&pre_args)?;

    let tool_argv: Vec<String> = tool_argv
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();

    let modes = [
        pre_args.spec.is_some(),
        pre_args.mcp.is_some(),
        pre_args.mcp_stdio.is_some(),
        pre_args.graphql.is_some(),
    ];
    let active = modes.iter().filter(|x| **x).count();
    if active == 0 {
        print_help();
        return Err(Error::usage(
            "one of --spec, --mcp, --mcp-stdio, or --graphql is required.",
        ));
    }
    if active > 1 {
        return Err(Error::usage(
            "--spec, --mcp, --mcp-stdio, and --graphql are mutually exclusive.",
        ));
    }

    if let Some(cmd) = &pre_args.mcp_stdio {
        if oauth_wanted(&pre_args) {
            return Err(Error::usage(
                "OAuth is not supported with --mcp-stdio (HTTP discovery required)",
            ));
        }
        return dispatch_mcp_stdio(&pre_args, cmd, &tool_argv, bake_config.as_ref());
    }

    if let Some(url) = &pre_args.mcp {
        return dispatch_mcp_http(&pre_args, url, &tool_argv, bake_config.as_ref());
    }

    if let Some(spec) = &pre_args.spec {
        return dispatch_openapi(&pre_args, spec, &tool_argv, bake_config.as_ref());
    }

    if let Some(url) = &pre_args.graphql {
        return dispatch_graphql(&pre_args, url, &tool_argv, bake_config.as_ref());
    }

    Err(Error::usage("no source mode selected"))
}

fn apply_bake_filter(
    commands: Vec<crate::CommandDef>,
    bake: Option<&BakeConfig>,
) -> Vec<crate::CommandDef> {
    match bake {
        Some(b) => filter_commands(commands, &b.include, &b.exclude, &b.methods),
        None => commands,
    }
}

fn dispatch_openapi(
    pre: &GlobalArgs,
    spec_source: &str,
    remaining: &[String],
    bake: Option<&BakeConfig>,
) -> Result<()> {
    let mut auth = pre.parse_auth_headers()?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::runtime(e.to_string()))?;
    let oauth = rt.block_on(setup_from_args(pre))?;
    if let Some(ref o) = oauth {
        let token = rt.block_on(o.access_token())?;
        auth.retain(|(k, _)| !k.eq_ignore_ascii_case("authorization"));
        auth.push(("Authorization".into(), format!("Bearer {token}")));
    }

    let src_hash = source_hash_for(spec_source);
    let spec = load_openapi_spec(
        spec_source,
        &auth,
        pre.cache_key.as_deref(),
        Some(pre.cache_ttl),
        pre.refresh,
    )?;
    let mut commands = extract_openapi_commands(&spec);
    commands = apply_bake_filter(commands, bake);

    let list_opts = ListOptions {
        verbose: pre.verbose,
        compact: pre.compact,
        json_output: pre.json_output,
        pretty: pre.pretty,
        style: ListStyle::OpenApi,
    };

    if pre.list_commands {
        if let Some(pat) = &pre.search_pattern {
            commands = filter_by_search(commands, pat);
            if commands.is_empty() {
                if pre.json_output {
                    list_commands(&commands, &list_opts)?;
                } else {
                    println!("\nNo tools matching '{pat}'.");
                }
                return Ok(());
            }
            if !pre.compact && !pre.json_output {
                println!("\nTools matching '{pat}':");
            }
        }
        let commands = apply_list_options(commands, &src_hash, pre.sort.as_deref(), pre.top);
        return list_commands(&commands, &list_opts);
    }

    if remaining.is_empty() {
        print_help();
        eprintln!("\nUse --list to see all available commands.");
        return Err(Error::usage("__printed__"));
    }

    let base_url = resolve_base_url(pre.base_url.as_deref(), &spec, spec_source)?;
    let parsed = match parse_tool_args(&commands, remaining) {
        Err(Error::Usage(msg)) if msg == "__help__" => return Ok(()),
        other => other?,
    };
    if remaining.get(1).map(String::as_str) == Some("--help")
        || remaining.get(1).map(String::as_str) == Some("-h")
    {
        print_command_help(&parsed.command);
        return Ok(());
    }

    execute_openapi(&parsed, &base_url, &auth, &pre.output_options())?;
    let _ = record_usage(&src_hash, &parsed.command.name);
    Ok(())
}

fn dispatch_graphql(
    pre: &GlobalArgs,
    url: &str,
    remaining: &[String],
    bake: Option<&BakeConfig>,
) -> Result<()> {
    let mut auth = pre.parse_auth_headers()?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::runtime(e.to_string()))?;
    let oauth = rt.block_on(setup_from_args(pre))?;
    if let Some(ref o) = oauth {
        let token = rt.block_on(o.access_token())?;
        auth.retain(|(k, _)| !k.eq_ignore_ascii_case("authorization"));
        auth.push(("Authorization".into(), format!("Bearer {token}")));
    }

    let src_hash = source_hash_for(url);
    let schema = load_graphql_schema(
        url,
        &auth,
        pre.cache_key.as_deref(),
        Some(pre.cache_ttl),
        pre.refresh,
    )?;
    let mut commands = extract_graphql_commands(&schema);
    commands = apply_bake_filter(commands, bake);

    let list_opts = ListOptions {
        verbose: pre.verbose,
        compact: pre.compact,
        json_output: pre.json_output,
        pretty: pre.pretty,
        style: ListStyle::Graphql,
    };

    if pre.list_commands {
        if let Some(pat) = &pre.search_pattern {
            commands = filter_by_search(commands, pat);
            if commands.is_empty() {
                if pre.json_output {
                    list_commands(&commands, &list_opts)?;
                } else {
                    println!("\nNo tools matching '{pat}'.");
                }
                return Ok(());
            }
            if !pre.compact && !pre.json_output {
                println!("\nTools matching '{pat}':");
            }
        }
        let commands = apply_list_options(commands, &src_hash, pre.sort.as_deref(), pre.top);
        return list_commands(&commands, &list_opts);
    }

    if remaining.is_empty() {
        if !pre.compact && !pre.json_output {
            println!("Available operations:");
        }
        let commands = apply_list_options(commands, &src_hash, pre.sort.as_deref(), pre.top);
        list_commands(&commands, &list_opts)?;
        if !pre.compact && !pre.json_output {
            println!("\nUse --list for the same output, or provide a subcommand.");
        }
        return Ok(());
    }

    let parsed = match parse_tool_args(&commands, remaining) {
        Err(Error::Usage(msg)) if msg == "__help__" => return Ok(()),
        other => other?,
    };
    if remaining.get(1).map(String::as_str) == Some("--help")
        || remaining.get(1).map(String::as_str) == Some("-h")
    {
        print_command_help(&parsed.command);
        return Ok(());
    }

    execute_graphql(
        &parsed,
        url,
        &schema,
        &auth,
        pre.fields.as_deref(),
        &pre.output_options(),
    )?;
    let usage_key = parsed
        .command
        .graphql_field_name
        .as_deref()
        .unwrap_or(&parsed.command.name);
    let _ = record_usage(&src_hash, usage_key);
    Ok(())
}

fn dispatch_mcp_http(
    pre: &GlobalArgs,
    url: &str,
    remaining: &[String],
    bake: Option<&BakeConfig>,
) -> Result<()> {
    let auth = pre.parse_auth_headers()?;
    let transport = TransportMode::parse(&pre.transport)?;
    let src_hash = source_hash_for(url);
    let cache_key = pre.cache_key.clone().unwrap_or_else(|| {
        cache_key_for(&serde_json::json!({
            "source": url,
            "is_stdio": false,
            "auth_headers": auth,
            "transport": pre.transport,
        }))
    });

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::runtime(e.to_string()))?;

    let oauth: Option<OAuthReady> = rt.block_on(setup_from_args(pre))?;
    let oauth_ref = oauth.as_ref();

    let list_opts = ListOptions {
        verbose: pre.verbose,
        compact: pre.compact,
        json_output: pre.json_output,
        pretty: pre.pretty,
        style: ListStyle::Mcp,
    };

    if pre.list_commands {
        let tools = rt.block_on(fetch_mcp_tools_http(
            url,
            &auth,
            &cache_key,
            pre.cache_ttl,
            pre.refresh,
            transport,
            oauth_ref,
        ))?;
        let mut commands = apply_bake_filter(tools_to_commands(&tools), bake);
        if let Some(pat) = &pre.search_pattern {
            commands = filter_by_search(commands, pat);
            if commands.is_empty() {
                if pre.json_output {
                    list_commands(&commands, &list_opts)?;
                } else {
                    println!("\nNo tools matching '{pat}'.");
                }
                return Ok(());
            }
            if !pre.compact && !pre.json_output {
                println!("\nTools matching '{pat}':");
            }
        } else if !pre.compact && !pre.json_output {
            println!("\nAvailable tools:");
        }
        let commands = apply_list_options(commands, &src_hash, pre.sort.as_deref(), pre.top);
        return list_commands(&commands, &list_opts);
    }

    let tools = rt.block_on(fetch_mcp_tools_http(
        url,
        &auth,
        &cache_key,
        pre.cache_ttl,
        pre.refresh,
        transport,
        oauth_ref,
    ))?;
    let commands = apply_bake_filter(tools_to_commands(&tools), bake);

    if remaining.is_empty() {
        if !pre.compact && !pre.json_output {
            println!("Available tools:");
        }
        let commands = apply_list_options(commands, &src_hash, pre.sort.as_deref(), pre.top);
        list_commands(&commands, &list_opts)?;
        if !pre.compact && !pre.json_output {
            println!("\nUse --list for the same output, or provide a subcommand.");
        }
        return Ok(());
    }

    if remaining.get(1).map(String::as_str) == Some("--help")
        || remaining.get(1).map(String::as_str) == Some("-h")
    {
        if let Some(cmd) = commands.iter().find(|c| c.name == remaining[0]) {
            print_command_help(cmd);
            return Ok(());
        }
    }

    let parsed = match parse_tool_args(&commands, remaining) {
        Err(Error::Usage(msg)) if msg == "__help__" => return Ok(()),
        other => other?,
    };

    let arguments: Map<String, Value> = if parsed.stdin {
        match read_stdin_json("MCP tool arguments")? {
            Value::Object(map) => map,
            other => {
                return Err(Error::runtime(format!(
                    "MCP --stdin expects a JSON object, got {}",
                    other
                )));
            }
        }
    } else {
        parsed.values.into_iter().collect()
    };

    let tool_name = parsed
        .command
        .tool_name
        .clone()
        .unwrap_or_else(|| parsed.command.name.clone());

    let data = rt.block_on(call_tool_http(
        url,
        &auth,
        &tool_name,
        arguments,
        pre.json_output,
        transport,
        oauth_ref,
    ))?;
    output_result(data, &pre.output_options())?;
    let _ = record_usage(&src_hash, &tool_name);
    Ok(())
}

fn dispatch_mcp_stdio(
    pre: &GlobalArgs,
    command_str: &str,
    remaining: &[String],
    bake: Option<&BakeConfig>,
) -> Result<()> {
    let env_vars = pre.parse_env_vars()?;
    let src_hash = source_hash_for(command_str);
    let cache_key = pre.cache_key.clone().unwrap_or_else(|| {
        cache_key_for(&serde_json::json!({
            "source": command_str,
            "is_stdio": true,
            "env_vars": env_vars,
        }))
    });

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::runtime(e.to_string()))?;

    let list_opts = ListOptions {
        verbose: pre.verbose,
        compact: pre.compact,
        json_output: pre.json_output,
        pretty: pre.pretty,
        style: ListStyle::Mcp,
    };

    if pre.list_commands {
        let tools = rt.block_on(fetch_mcp_tools_stdio(
            command_str,
            &env_vars,
            &cache_key,
            pre.cache_ttl,
            pre.refresh,
        ))?;
        let mut commands = apply_bake_filter(tools_to_commands(&tools), bake);
        if let Some(pat) = &pre.search_pattern {
            commands = filter_by_search(commands, pat);
            if commands.is_empty() {
                if pre.json_output {
                    list_commands(&commands, &list_opts)?;
                } else {
                    println!("\nNo tools matching '{pat}'.");
                }
                return Ok(());
            }
            if !pre.compact && !pre.json_output {
                println!("\nTools matching '{pat}':");
            }
        } else if !pre.compact && !pre.json_output {
            println!("\nAvailable tools:");
        }
        let commands = apply_list_options(commands, &src_hash, pre.sort.as_deref(), pre.top);
        return list_commands(&commands, &list_opts);
    }

    let tools = rt.block_on(fetch_mcp_tools_stdio(
        command_str,
        &env_vars,
        &cache_key,
        pre.cache_ttl,
        pre.refresh,
    ))?;
    let commands = apply_bake_filter(tools_to_commands(&tools), bake);

    if remaining.is_empty() {
        if !pre.compact && !pre.json_output {
            println!("Available tools:");
        }
        let commands = apply_list_options(commands, &src_hash, pre.sort.as_deref(), pre.top);
        list_commands(&commands, &list_opts)?;
        if !pre.compact && !pre.json_output {
            println!("\nUse --list for the same output, or provide a subcommand.");
        }
        return Ok(());
    }

    if remaining.get(1).map(String::as_str) == Some("--help")
        || remaining.get(1).map(String::as_str) == Some("-h")
    {
        if let Some(cmd) = commands.iter().find(|c| c.name == remaining[0]) {
            print_command_help(cmd);
            return Ok(());
        }
    }

    let parsed = match parse_tool_args(&commands, remaining) {
        Err(Error::Usage(msg)) if msg == "__help__" => return Ok(()),
        other => other?,
    };

    let arguments: Map<String, Value> = if parsed.stdin {
        match read_stdin_json("MCP tool arguments")? {
            Value::Object(map) => map,
            other => {
                return Err(Error::runtime(format!(
                    "MCP --stdin expects a JSON object, got {}",
                    other
                )));
            }
        }
    } else {
        parsed.values.into_iter().collect()
    };

    let tool_name = parsed
        .command
        .tool_name
        .clone()
        .unwrap_or_else(|| parsed.command.name.clone());

    let data = rt.block_on(call_tool_stdio(
        command_str,
        &env_vars,
        &tool_name,
        arguments,
        pre.json_output,
    ))?;
    output_result(data, &pre.output_options())?;
    let _ = record_usage(&src_hash, &tool_name);
    Ok(())
}

fn print_help() {
    eprintln!(
        "\
mcp2cli {version} — Turn any MCP server, OpenAPI spec, or GraphQL endpoint into a CLI

Usage:
  mcp2cli --spec <URL|FILE> [--list] [command]
  mcp2cli --mcp <URL> [--list] [command]
  mcp2cli --mcp-stdio <CMD> [--list] [command]
  mcp2cli --graphql <URL> [--list] [command]
  mcp2cli bake <create|list|show|remove|update|install> ...
  mcp2cli @<name> ...
",
        version = env!("CARGO_PKG_VERSION")
    );
}
