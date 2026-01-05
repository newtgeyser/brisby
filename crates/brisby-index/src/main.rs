//! Brisby Index Provider - Federated search server

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod search;

#[derive(Parser)]
#[command(name = "brisby-index")]
#[command(about = "Brisby index provider server", long_about = None)]
struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Port to listen on (for admin interface)
    #[arg(short, long, default_value = "8080")]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set up logging
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();

    tracing::info!("Starting Brisby Index Provider");
    tracing::info!("Protocol version: {}", brisby_core::PROTOCOL_VERSION);

    // TODO: Initialize Nym node
    // TODO: Initialize search index
    // TODO: Start handling requests

    println!("Index provider starting...");
    println!("Config: {}", cli.config);
    println!("Admin port: {}", cli.port);

    // Keep running
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down");

    Ok(())
}
