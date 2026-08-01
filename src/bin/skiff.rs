fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    if let Err(err) = skiff_cli::run(std::env::args_os().skip(1)) {
        eprintln!("Error: {err}");
        std::process::exit(err.exit_code());
    }
}
