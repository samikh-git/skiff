//! Top-level CLI dispatch: bake / `@name` / sessions / OpenAPI / MCP / GraphQL.
//!
//! Session lifecycle and `--session` IPC run before source-mode mutual exclusion
//! so agents can stop/list without repeating `--mcp*`.

use std::ffi::OsString;

use clap::Parser;
use serde_json::{Map, Value};

use crate::bake::require_baked;
use crate::cache::cache_key_for;
use crate::cli::args::GlobalArgs;
use crate::cli::bake::handle_bake;
use crate::cli::dynamic::{parse_tool_args, read_stdin_json};
use crate::cli::list::{
    apply_list_options, describe_tool, filter_by_search, list_commands, maybe_tool_help,
    ListOptions, ListStyle,
};
use crate::cli::{global_option_sets, split_at_subcommand};
use crate::error::{Error, Result};
use crate::filter::filter_commands;
use crate::graphql::{execute_graphql, extract_graphql_commands, load_graphql_schema};
use crate::mcp::{
    call_tool_http, call_tool_stdio, fetch_mcp_tools_http, fetch_mcp_tools_stdio,
    tools_to_commands, TransportMode,
};
use crate::model::{BakeConfig, ListDetail};
use crate::oauth::{
    clear_oauth_credentials, discovery_url_from_args, oauth_wanted, options_from_args,
    setup_from_args, OAuthReady,
};
use crate::openapi::{
    execute_openapi, extract_openapi_commands, load_openapi_spec, resolve_base_url,
};
use crate::output::output_result;
use crate::tools_index::{tools_to_light_commands, try_commands_from_index};
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
        if msg.contains("help")
            || e.kind() == clap::error::ErrorKind::DisplayHelp
            || e.kind() == clap::error::ErrorKind::DisplayVersion
        {
            e.print().ok();
            Error::usage("__printed__")
        } else {
            Error::usage(msg)
        }
    })?;

    pre_args.apply_agent_defaults();

    if pre_args.spool_clean {
        let n = crate::spool::clean_spool(crate::spool::DEFAULT_SPOOL_TTL_SECS)?;
        println!("Removed {n} expired spool file(s)");
        return Ok(());
    }

    if pre_args.search_pattern.is_some() {
        pre_args.list_commands = true;
    }

    if pre_args.describe.is_some() {
        pre_args.list_commands = false;
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

    // Session management that does not require a source mode.
    #[cfg(unix)]
    {
        if pre_args.session_list {
            return dispatch_session_list(&pre_args);
        }
        if let Some(name) = &pre_args.session_stop {
            return crate::session::session_stop(name);
        }
        if let Some(name) = &pre_args.session_start {
            return dispatch_session_start(&pre_args, name);
        }
        if let Some(name) = &pre_args.session {
            return dispatch_session(&pre_args, name, &tool_argv, bake_config.as_ref());
        }
    }
    #[cfg(not(unix))]
    {
        if pre_args.session_list
            || pre_args.session_stop.is_some()
            || pre_args.session_start.is_some()
            || pre_args.session.is_some()
        {
            return Err(crate::session::sessions_unsupported());
        }
    }

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

/// Resolve commands for MCP `--list`/`--search` with warm index + light extract when possible.
fn mcp_discovery_commands(
    tools: Option<&[Value]>,
    cache_key: &str,
    pre: &GlobalArgs,
    bake: Option<&BakeConfig>,
) -> Result<Vec<crate::CommandDef>> {
    let detail = pre.list_detail();
    let light = matches!(detail, ListDetail::Names | ListDetail::Brief);
    let tools_key = format!("{cache_key}_tools");
    let require_descs = matches!(detail, ListDetail::Brief);

    if light && !pre.refresh {
        if let Some(cmds) = try_commands_from_index(
            &tools_key,
            pre.cache_ttl,
            pre.search_pattern.as_deref(),
            require_descs,
        )? {
            return Ok(apply_bake_filter(cmds, bake));
        }
    }

    let tools = tools.ok_or_else(|| Error::runtime("internal: tools required when index miss"))?;
    let mut commands = if light {
        apply_bake_filter(tools_to_light_commands(tools), bake)
    } else {
        apply_bake_filter(tools_to_commands(tools), bake)
    };
    if let Some(pat) = &pre.search_pattern {
        commands = filter_by_search(commands, pat);
    }
    Ok(commands)
}

fn emit_mcp_list(
    commands: Vec<crate::CommandDef>,
    pre: &GlobalArgs,
    src_hash: &str,
    list_opts: &ListOptions,
) -> Result<()> {
    if let Some(pat) = &pre.search_pattern {
        if commands.is_empty() {
            if pre.json_output || pre.agent {
                list_commands(&commands, list_opts)?;
            } else {
                println!("\nNo tools matching '{pat}'.");
            }
            return Ok(());
        }
        if !pre.quiet_list() {
            println!("\nTools matching '{pat}':");
        }
    } else if !pre.quiet_list() {
        println!("\nAvailable tools:");
    }
    // Search already applied in mcp_discovery_commands when from index;
    // when from tools with search in discovery, also applied. Avoid double-filter.
    let commands = apply_list_options(commands, src_hash, pre.sort.as_deref(), pre.top);
    list_commands(&commands, list_opts)
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

    let list_opts = ListOptions::from_global(pre, ListStyle::OpenApi);

    if let Some(name) = &pre.describe {
        let commands = apply_list_options(commands, &src_hash, pre.sort.as_deref(), pre.top);
        return describe_tool(&commands, name, pre);
    }

    if pre.list_commands {
        if let Some(pat) = &pre.search_pattern {
            commands = filter_by_search(commands, pat);
            if commands.is_empty() {
                if pre.json_output || pre.agent {
                    list_commands(&commands, &list_opts)?;
                } else {
                    println!("\nNo tools matching '{pat}'.");
                }
                return Ok(());
            }
            if !pre.quiet_list() {
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

    if maybe_tool_help(&commands, remaining, pre)? {
        return Ok(());
    }

    let base_url = resolve_base_url(pre.base_url.as_deref(), &spec, spec_source)?;
    let parsed = match parse_tool_args(&commands, remaining) {
        Err(Error::Usage(msg)) if msg == "__help__" => return Ok(()),
        other => other?,
    };

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

    let list_opts = ListOptions::from_global(pre, ListStyle::Graphql);

    if let Some(name) = &pre.describe {
        let commands = apply_list_options(commands, &src_hash, pre.sort.as_deref(), pre.top);
        return describe_tool(&commands, name, pre);
    }

    if pre.list_commands {
        if let Some(pat) = &pre.search_pattern {
            commands = filter_by_search(commands, pat);
            if commands.is_empty() {
                if pre.json_output || pre.agent {
                    list_commands(&commands, &list_opts)?;
                } else {
                    println!("\nNo tools matching '{pat}'.");
                }
                return Ok(());
            }
            if !pre.quiet_list() {
                println!("\nTools matching '{pat}':");
            }
        }
        let commands = apply_list_options(commands, &src_hash, pre.sort.as_deref(), pre.top);
        return list_commands(&commands, &list_opts);
    }

    if remaining.is_empty() {
        if !pre.quiet_list() {
            println!("Available operations:");
        }
        let commands = apply_list_options(commands, &src_hash, pre.sort.as_deref(), pre.top);
        list_commands(&commands, &list_opts)?;
        if !pre.quiet_list() {
            println!("\nUse --list for the same output, or provide a subcommand.");
        }
        return Ok(());
    }

    if maybe_tool_help(&commands, remaining, pre)? {
        return Ok(());
    }

    let parsed = match parse_tool_args(&commands, remaining) {
        Err(Error::Usage(msg)) if msg == "__help__" => return Ok(()),
        other => other?,
    };

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

    let list_opts = ListOptions::from_global(pre, ListStyle::Mcp);
    let tools_key = format!("{cache_key}_tools");
    let detail = pre.list_detail();
    let can_light =
        matches!(detail, ListDetail::Names | ListDetail::Brief) && pre.describe.is_none();
    let require_descs = matches!(detail, ListDetail::Brief);
    let discovery = pre.list_commands || remaining.is_empty();

    if discovery && can_light && !pre.refresh {
        if let Some(cmds) = try_commands_from_index(
            &tools_key,
            pre.cache_ttl,
            pre.search_pattern.as_deref(),
            require_descs,
        )? {
            emit_mcp_list(apply_bake_filter(cmds, bake), pre, &src_hash, &list_opts)?;
            if remaining.is_empty() && !pre.list_commands && !pre.quiet_list() {
                println!("\nUse --list for the same output, or provide a subcommand.");
            }
            return Ok(());
        }
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

    if let Some(name) = &pre.describe {
        let commands = apply_bake_filter(tools_to_commands(&tools), bake);
        let commands = apply_list_options(commands, &src_hash, pre.sort.as_deref(), pre.top);
        return describe_tool(&commands, name, pre);
    }

    if discovery {
        let commands = mcp_discovery_commands(Some(&tools), &cache_key, pre, bake)?;
        emit_mcp_list(commands, pre, &src_hash, &list_opts)?;
        if remaining.is_empty() && !pre.list_commands && !pre.quiet_list() {
            println!("\nUse --list for the same output, or provide a subcommand.");
        }
        return Ok(());
    }

    let commands = apply_bake_filter(tools_to_commands(&tools), bake);

    if maybe_tool_help(&commands, remaining, pre)? {
        return Ok(());
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
        pre.full_envelope(),
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

    let list_opts = ListOptions::from_global(pre, ListStyle::Mcp);
    let tools_key = format!("{cache_key}_tools");
    let detail = pre.list_detail();
    let can_light =
        matches!(detail, ListDetail::Names | ListDetail::Brief) && pre.describe.is_none();
    let require_descs = matches!(detail, ListDetail::Brief);
    let discovery = pre.list_commands || remaining.is_empty();

    if discovery && can_light && !pre.refresh {
        if let Some(cmds) = try_commands_from_index(
            &tools_key,
            pre.cache_ttl,
            pre.search_pattern.as_deref(),
            require_descs,
        )? {
            emit_mcp_list(apply_bake_filter(cmds, bake), pre, &src_hash, &list_opts)?;
            if remaining.is_empty() && !pre.list_commands && !pre.quiet_list() {
                println!("\nUse --list for the same output, or provide a subcommand.");
            }
            return Ok(());
        }
    }

    let tools = rt.block_on(fetch_mcp_tools_stdio(
        command_str,
        &env_vars,
        &cache_key,
        pre.cache_ttl,
        pre.refresh,
    ))?;

    if let Some(name) = &pre.describe {
        let commands = apply_bake_filter(tools_to_commands(&tools), bake);
        let commands = apply_list_options(commands, &src_hash, pre.sort.as_deref(), pre.top);
        return describe_tool(&commands, name, pre);
    }

    if discovery {
        let commands = mcp_discovery_commands(Some(&tools), &cache_key, pre, bake)?;
        emit_mcp_list(commands, pre, &src_hash, &list_opts)?;
        if remaining.is_empty() && !pre.list_commands && !pre.quiet_list() {
            println!("\nUse --list for the same output, or provide a subcommand.");
        }
        return Ok(());
    }

    let commands = apply_bake_filter(tools_to_commands(&tools), bake);

    if maybe_tool_help(&commands, remaining, pre)? {
        return Ok(());
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
        pre.full_envelope(),
    ))?;
    output_result(data, &pre.output_options())?;
    let _ = record_usage(&src_hash, &tool_name);
    Ok(())
}

#[cfg(unix)]
fn dispatch_session_list(pre: &GlobalArgs) -> Result<()> {
    let entries = crate::session::session_list()?;
    if pre.json_output {
        let v = serde_json::to_value(&entries)?;
        output_result(v, &pre.output_options())?;
        return Ok(());
    }
    if entries.is_empty() {
        println!("No sessions.");
        return Ok(());
    }
    for e in entries {
        let status = if e.alive { "alive" } else { "dead" };
        println!(
            "  {:<20} {status:<6} pid={}  {} ({})",
            e.name, e.pid, e.transport, e.source
        );
    }
    Ok(())
}

#[cfg(unix)]
fn dispatch_session_start(pre: &GlobalArgs, name: &str) -> Result<()> {
    let (source, is_stdio) = if let Some(cmd) = &pre.mcp_stdio {
        if oauth_wanted(pre) {
            return Err(Error::usage(
                "OAuth is not supported with --mcp-stdio (HTTP discovery required)",
            ));
        }
        (cmd.clone(), true)
    } else if let Some(url) = &pre.mcp {
        (url.clone(), false)
    } else {
        return Err(Error::usage(
            "--session-start requires --mcp or --mcp-stdio",
        ));
    };

    let mut auth = pre.parse_auth_headers()?;
    if !is_stdio && oauth_wanted(pre) {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| Error::runtime(e.to_string()))?;
        if let Some(o) = rt.block_on(setup_from_args(pre))? {
            let token = rt.block_on(o.access_token())?;
            auth.retain(|(k, _)| !k.eq_ignore_ascii_case("authorization"));
            auth.push(("Authorization".into(), format!("Bearer {token}")));
        }
    }

    let idle_secs = crate::session::resolve_idle_secs(pre.session_idle_secs);
    let config = crate::session::DaemonConfig {
        name: name.to_string(),
        source,
        is_stdio,
        auth_headers: auth,
        env_vars: pre.parse_env_vars()?,
        transport: pre.transport.clone(),
        clean_env: pre.session_clean_env,
        idle_secs,
    };
    crate::session::session_start(config)
}

#[cfg(unix)]
fn dispatch_session(
    pre: &GlobalArgs,
    name: &str,
    remaining: &[String],
    bake: Option<&BakeConfig>,
) -> Result<()> {
    let _ = crate::session::clear_stale_session(name)?;
    if !crate::session::session_sock_path(name).exists() {
        return Err(Error::runtime(format!(
            "session {name:?} is not running. Start with --session-start {name}"
        )));
    }

    if pre.list_resources {
        let v = crate::session::session_request(name, "list_resources", Value::Null)?;
        output_result(v, &pre.output_options())?;
        return Ok(());
    }
    if pre.list_resource_templates {
        let v = crate::session::session_request(name, "list_resource_templates", Value::Null)?;
        output_result(v, &pre.output_options())?;
        return Ok(());
    }
    if let Some(uri) = &pre.read_resource {
        let v = crate::session::session_request(
            name,
            "read_resource",
            serde_json::json!({ "uri": uri }),
        )?;
        output_result(v, &pre.output_options())?;
        return Ok(());
    }
    if pre.list_prompts {
        let v = crate::session::session_request(name, "list_prompts", Value::Null)?;
        output_result(v, &pre.output_options())?;
        return Ok(());
    }
    if let Some(pname) = &pre.get_prompt {
        let mut args = Map::new();
        for item in &pre.prompt_arg {
            let Some((k, v)) = item.split_once('=') else {
                return Err(Error::usage(format!(
                    "invalid --prompt-arg {item:?}; expected KEY=VALUE"
                )));
            };
            args.insert(k.to_string(), Value::String(v.to_string()));
        }
        let v = crate::session::session_request(
            name,
            "get_prompt",
            serde_json::json!({ "name": pname, "arguments": args }),
        )?;
        output_result(v, &pre.output_options())?;
        return Ok(());
    }

    let src_hash = source_hash_for(&format!("session:{name}"));
    let list_opts = ListOptions::from_global(pre, ListStyle::Mcp);
    let detail = pre.list_detail();
    let use_light_index =
        matches!(detail, ListDetail::Names) && pre.describe.is_none();
    let needs_catalog = pre.list_commands
        || pre.describe.is_some()
        || remaining.is_empty()
        || remaining.get(1).map(String::as_str) == Some("--help")
        || remaining.get(1).map(String::as_str) == Some("-h");

    let load_catalog = || -> Result<Vec<crate::CommandDef>> {
        let tools_val = crate::session::session_request(
            name,
            "list_tools",
            serde_json::json!({ "refresh": pre.refresh }),
        )?;
        let tools = tools_val
            .as_array()
            .cloned()
            .ok_or_else(|| Error::runtime("session list_tools did not return an array"))?;
        Ok(apply_bake_filter(tools_to_commands(&tools), bake))
    };

    let load_light_catalog = || -> Result<Vec<crate::CommandDef>> {
        let mut params = serde_json::json!({ "refresh": pre.refresh });
        if let Some(pat) = &pre.search_pattern {
            params["search"] = Value::String(pat.clone());
        }
        let tools_val =
            crate::session::session_request(name, "list_tools_light", params)?;
        let tools = tools_val
            .as_array()
            .cloned()
            .ok_or_else(|| Error::runtime("session list_tools_light did not return an array"))?;
        Ok(apply_bake_filter(tools_to_light_commands(&tools), bake))
    };

    if needs_catalog {
        let mut commands = if use_light_index {
            load_light_catalog()?
        } else {
            load_catalog()?
        };

        if let Some(dname) = &pre.describe {
            let commands = apply_list_options(commands, &src_hash, pre.sort.as_deref(), pre.top);
            return describe_tool(&commands, dname, pre);
        }

        if pre.list_commands {
            if use_light_index {
                // Search already applied in the daemon via list_tools_light.
                if commands.is_empty() {
                    if let Some(pat) = &pre.search_pattern {
                        if pre.json_output || pre.agent {
                            list_commands(&commands, &list_opts)?;
                        } else {
                            println!("\nNo tools matching '{pat}'.");
                        }
                        return Ok(());
                    }
                } else if let Some(pat) = &pre.search_pattern {
                    if !pre.quiet_list() {
                        println!("\nTools matching '{pat}':");
                    }
                } else if !pre.quiet_list() {
                    println!("\nAvailable tools:");
                }
            } else if let Some(pat) = &pre.search_pattern {
                commands = filter_by_search(commands, pat);
                if commands.is_empty() {
                    if pre.json_output || pre.agent {
                        list_commands(&commands, &list_opts)?;
                    } else {
                        println!("\nNo tools matching '{pat}'.");
                    }
                    return Ok(());
                }
                if !pre.quiet_list() {
                    println!("\nTools matching '{pat}':");
                }
            } else if !pre.quiet_list() {
                println!("\nAvailable tools:");
            }
            let commands = apply_list_options(commands, &src_hash, pre.sort.as_deref(), pre.top);
            return list_commands(&commands, &list_opts);
        }

        if remaining.is_empty() {
            if !pre.quiet_list() {
                println!("Available tools:");
            }
            let commands = apply_list_options(commands, &src_hash, pre.sort.as_deref(), pre.top);
            list_commands(&commands, &list_opts)?;
            if !pre.quiet_list() {
                println!("\nUse --list for the same output, or provide a subcommand.");
            }
            return Ok(());
        }

        if maybe_tool_help(&commands, remaining, pre)? {
            return Ok(());
        }
    }

    // Call path: fetch one tool schema over IPC (daemon cache), not the full catalog.
    let tname = remaining
        .first()
        .ok_or_else(|| Error::usage("no subcommand specified"))?;
    let tool_val = crate::session::session_request(
        name,
        "get_tool",
        serde_json::json!({ "name": tname, "refresh": pre.refresh }),
    )?;
    let commands = if tool_val.is_null() {
        load_catalog()?
    } else {
        let one = tools_to_commands(std::slice::from_ref(&tool_val));
        apply_bake_filter(one, bake)
    };

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

    let data = crate::session::session_request(
        name,
        "call_tool",
        serde_json::json!({
            "name": tool_name,
            "arguments": arguments,
            "full_envelope": pre.full_envelope(),
        }),
    )?;
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
  mcp2cli --mcp-stdio <CMD> --session-start <NAME>
  mcp2cli --session <NAME> [--list] [command]
  mcp2cli bake <create|list|show|remove|update|install> ...
  mcp2cli @<name> ...
",
        version = env!("CARGO_PKG_VERSION")
    );
}
