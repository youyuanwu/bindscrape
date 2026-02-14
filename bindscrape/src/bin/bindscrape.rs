//! CLI entry point for bindscrape.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

/// bindscrape — generate WinMD metadata from C headers.
#[derive(Parser, Debug)]
#[command(name = "bindscrape", version, about)]
struct Cli {
    /// Path to the bindscrape.toml configuration file.
    #[arg(default_value = "bindscrape.toml")]
    config: PathBuf,

    /// Output file path (overrides config).
    #[arg(short, long)]
    output: Option<PathBuf>,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("bindscrape=info")),
        )
        .init();

    let cli = Cli::parse();
    bindscrape::run(&cli.config, cli.output.as_deref())?;
    Ok(())
}
