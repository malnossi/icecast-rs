use icecast_rs::{config, server, state};
use std::sync::Arc;
use tracing::info;
use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

/// High-Performance Standalone Audio Streaming Server
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct CliArgs {
    /// Path to the custom configuration TOML file
    #[arg(short, long, value_name = "FILE", default_value = "config.toml")]
    pub config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    
    let args = CliArgs::parse();

    let config = match config::Config::initialize_from_path(&args.config) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("[-] Initialization Error: Failed to parse configuration file at {:?}. Details: {}", args.config, e);
            std::process::exit(1);
        }
    };

    let state = state::State::new();
    let addr = format!("{}:{}", config.server.host, config.server.port);
    
    info!("Starting server...");
    server::run_server(&addr, state, config).await.context("Server execution failed")?;
    
    Ok(())
}
