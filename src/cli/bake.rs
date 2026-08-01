//! `mcp2cli bake` subcommand handlers.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::bake::{
    create_baked, default_install_dir, install_wrapper, load_baked_all, parse_auth_header_raw,
    parse_env_raw, remove_baked, require_baked, split_csv_list, split_methods, update_baked,
    BakedTool,
};
use crate::error::{Error, Result};
use crate::paths::DEFAULT_CACHE_TTL;

#[derive(Debug, Parser)]
#[command(
    name = "mcp2cli bake",
    about = "Manage saved connection settings",
    disable_help_subcommand = true
)]
struct BakeCli {
    #[command(subcommand)]
    command: Option<BakeCommand>,
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
enum BakeCommand {
    /// Save connection settings as a named baked tool
    Create(CreateArgs),
    /// List all baked tools
    List,
    /// Show config for a baked tool (secrets masked)
    Show { name: String },
    /// Delete a baked tool
    Remove { name: String },
    /// Update settings on an existing baked tool
    Update(UpdateArgs),
    /// Create a ~/.local/bin wrapper script
    Install(InstallArgs),
}

#[derive(Debug, Parser)]
struct CreateArgs {
    /// Name for the baked tool
    name: String,
    #[arg(long)]
    spec: Option<String>,
    #[arg(long)]
    mcp: Option<String>,
    #[arg(long)]
    mcp_stdio: Option<String>,
    #[arg(long)]
    graphql: Option<String>,
    #[arg(long)]
    base_url: Option<String>,
    #[arg(long = "auth-header", value_name = "Name:Value")]
    auth_header: Vec<String>,
    #[arg(long = "env", value_name = "KEY=VALUE")]
    env: Vec<String>,
    #[arg(long, default_value_t = DEFAULT_CACHE_TTL)]
    cache_ttl: u64,
    #[arg(long, default_value = "auto", value_parser = ["auto", "sse", "streamable"])]
    transport: String,
    #[arg(long)]
    oauth: bool,
    #[arg(long)]
    oauth_client_id: Option<String>,
    #[arg(long)]
    oauth_client_secret: Option<String>,
    #[arg(long, default_value = "mcp2cli")]
    oauth_client_name: String,
    #[arg(long)]
    oauth_scope: Option<String>,
    #[arg(long)]
    oauth_redirect_uri: Option<String>,
    #[arg(long, default_value = "auto", value_parser = ["auto", "authorization_code", "client_credentials"])]
    oauth_flow: String,
    /// Prefer this named session when using @name
    #[arg(long)]
    session: Option<String>,
    #[arg(long, default_value = "")]
    include: String,
    #[arg(long, default_value = "")]
    exclude: String,
    #[arg(long, default_value = "")]
    methods: String,
    #[arg(long, default_value = "")]
    description: String,
    /// Overwrite existing
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Parser)]
struct UpdateArgs {
    name: String,
    #[arg(long)]
    cache_ttl: Option<u64>,
    #[arg(long)]
    include: Option<String>,
    #[arg(long)]
    exclude: Option<String>,
    #[arg(long)]
    methods: Option<String>,
    #[arg(long)]
    description: Option<String>,
    #[arg(long)]
    base_url: Option<String>,
    #[arg(long, value_parser = ["auto", "sse", "streamable"])]
    transport: Option<String>,
}

#[derive(Debug, Parser)]
struct InstallArgs {
    name: String,
    /// Directory to install wrapper into (default: ~/.local/bin)
    #[arg(long)]
    dir: Option<PathBuf>,
}

pub fn handle_bake(argv: &[OsString]) -> Result<()> {
    let mut clap_argv = vec![OsString::from("mcp2cli bake")];
    clap_argv.extend_from_slice(argv);

    let cli = BakeCli::try_parse_from(&clap_argv).map_err(|e| {
        let _ = e.print();
        if e.kind() == clap::error::ErrorKind::DisplayHelp
            || e.kind() == clap::error::ErrorKind::DisplayVersion
        {
            Error::usage("__printed__")
        } else {
            Error::usage(e.to_string())
        }
    })?;

    match cli.command {
        None => {
            print_bake_help();
            Err(Error::usage("__printed__"))
        }
        Some(BakeCommand::Create(args)) => bake_create(args),
        Some(BakeCommand::List) => {
            bake_list();
            Ok(())
        }
        Some(BakeCommand::Show { name }) => bake_show(&name),
        Some(BakeCommand::Remove { name }) => {
            remove_baked(&name)?;
            println!("Baked tool '{name}' removed.");
            Ok(())
        }
        Some(BakeCommand::Update(args)) => bake_update(args),
        Some(BakeCommand::Install(args)) => bake_install(args),
    }
}

fn print_bake_help() {
    println!("Usage: mcp2cli bake [options]\n");
    println!("Commands:");
    println!("  create    Save connection settings as a named baked tool");
    println!("  list      List all baked tools");
    println!("  show      Show config for a baked tool (secrets masked)");
    println!("  remove    Delete a baked tool");
    println!("  update    Update settings on an existing baked tool");
    println!("  install   Create a ~/.local/bin wrapper script");
    println!("\nRun 'mcp2cli bake <command> --help' for command-specific help.");
}

fn bake_create(args: CreateArgs) -> Result<()> {
    let modes = [
        args.spec.is_some(),
        args.mcp.is_some(),
        args.mcp_stdio.is_some(),
        args.graphql.is_some(),
    ];
    let active = modes.iter().filter(|x| **x).count();
    if active == 0 {
        return Err(Error::usage(
            "one of --spec, --mcp, --mcp-stdio, or --graphql is required.",
        ));
    }
    if active > 1 {
        return Err(Error::usage(
            "--spec, --mcp, --mcp-stdio, and --graphql are mutually exclusive.",
        ));
    }

    let (source_type, source) = if let Some(s) = args.spec {
        ("spec", s)
    } else if let Some(s) = args.mcp {
        ("mcp", s)
    } else if let Some(s) = args.graphql {
        ("graphql", s)
    } else {
        ("mcp_stdio", args.mcp_stdio.expect("checked above"))
    };

    let tool = BakedTool {
        source_type: source_type.into(),
        source,
        base_url: args.base_url,
        auth_headers: parse_auth_header_raw(&args.auth_header)?,
        env_vars: parse_env_raw(&args.env)?,
        cache_ttl: args.cache_ttl,
        transport: args.transport,
        oauth: args.oauth,
        oauth_client_id: args.oauth_client_id,
        oauth_client_secret: args.oauth_client_secret,
        oauth_client_name: args.oauth_client_name,
        oauth_scope: args.oauth_scope,
        oauth_redirect_uri: args.oauth_redirect_uri,
        oauth_flow: args.oauth_flow,
        session: args.session,
        include: split_csv_list(&args.include),
        exclude: split_csv_list(&args.exclude),
        methods: split_methods(&args.methods),
        description: args.description,
        ..Default::default()
    };

    if let Some(sec) = &tool.oauth_client_secret {
        if !sec.starts_with("env:") && !sec.starts_with("file:") {
            use std::io::IsTerminal;
            if std::io::stdout().is_terminal() {
                eprintln!(
                    "warning: --oauth-client-secret is a literal value; prefer env:VAR or file:PATH so secrets stay off the process list and bake config"
                );
            }
        }
    }

    create_baked(&args.name, tool, args.force)?;
    println!("Baked tool '{}' created.", args.name);
    Ok(())
}

fn bake_list() {
    let configs = match load_baked_all() {
        Ok(c) => c,
        Err(_) => {
            println!("No baked tools.");
            return;
        }
    };
    if configs.is_empty() {
        println!("No baked tools.");
        return;
    }
    println!("{:<20} {:<10} {:<50}", "Name", "Type", "Source");
    println!("{}", "-".repeat(80));
    for (name, cfg) in &configs {
        let st = cfg.source_type.as_str();
        let mut src = cfg.source.clone();
        if src.len() > 48 {
            src = format!("{}...", &src[..45]);
        }
        println!("{name:<20} {st:<10} {src:<50}");
    }
}

fn bake_show(name: &str) -> Result<()> {
    let cfg = require_baked(name)?;
    let display = cfg.masked_for_display();
    println!("{}", serde_json::to_string_pretty(&display)?);
    Ok(())
}

fn bake_update(args: UpdateArgs) -> Result<()> {
    update_baked(&args.name, |cfg| {
        if let Some(ttl) = args.cache_ttl {
            cfg.cache_ttl = ttl;
        }
        if let Some(include) = &args.include {
            cfg.include = split_csv_list(include);
        }
        if let Some(exclude) = &args.exclude {
            cfg.exclude = split_csv_list(exclude);
        }
        if let Some(methods) = &args.methods {
            cfg.methods = split_methods(methods);
        }
        if let Some(desc) = &args.description {
            cfg.description = desc.clone();
        }
        if let Some(base) = &args.base_url {
            cfg.base_url = Some(base.clone());
        }
        if let Some(transport) = &args.transport {
            cfg.transport = transport.clone();
        }
    })?;
    println!("Baked tool '{}' updated.", args.name);
    Ok(())
}

fn bake_install(args: InstallArgs) -> Result<()> {
    let wrapper = install_wrapper(&args.name, args.dir.as_deref())?;
    println!("Installed wrapper: {}", wrapper.display());
    if args.dir.is_none() {
        if let Some(default_dir) = default_install_dir() {
            let path_env = std::env::var("PATH").unwrap_or_default();
            let in_path = std::env::split_paths(&path_env).any(|p| p == default_dir);
            if !in_path {
                println!("  Note: {} may not be in your PATH", default_dir.display());
            }
        }
    }
    Ok(())
}
