//! Brisby Index Provider - Federated search server

use anyhow::Result;
use brisby_core::Transport;
use clap::Parser;
use std::path::PathBuf;
use std::time::Duration;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod handler;
mod search;

use handler::MessageHandler;
use search::SearchIndex;

/// Cleanup interval for expired entries (1 hour)
const CLEANUP_INTERVAL: Duration = Duration::from_secs(3600);

#[derive(Parser)]
#[command(name = "brisby-index")]
#[command(about = "Brisby index provider server", long_about = None)]
struct Cli {
    /// Path to data directory
    #[arg(short, long, default_value = ".brisby-index")]
    data_dir: PathBuf,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Use mock transport instead of real Nym (for testing)
    #[arg(long)]
    mock: bool,
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

    // Create data directory if it doesn't exist
    std::fs::create_dir_all(&cli.data_dir)?;

    // Initialize search index
    let index_path = cli.data_dir.join("index.db");
    let index = SearchIndex::open(&index_path)?;
    tracing::info!("Opened search index at {:?}", index_path);

    // Show index stats
    if let Ok(stats) = index.stats() {
        tracing::info!(
            "Index contains {} entries ({} bytes total)",
            stats.entry_count,
            stats.total_size_bytes
        );
    }

    // Create message handler
    let handler = MessageHandler::new(index);

    // Spawn cleanup task
    let cleanup_index_path = index_path.clone();
    let cleanup_handle = tokio::spawn(async move {
        run_cleanup_task(&cleanup_index_path).await;
    });

    if cli.mock {
        // Use mock transport for testing
        tracing::info!("Using mock transport (test mode)");
        let mut transport = brisby_core::transport::mock::MockTransport::new();
        transport.connect().await?;
        tracing::info!("Mock transport connected");
        tracing::info!("Address: {}", transport.our_address().unwrap());

        // Run message loop with ctrl-c handler
        tokio::select! {
            result = handler::run_message_loop(&transport, &handler) => {
                if let Err(e) = result {
                    tracing::error!("Message loop error: {}", e);
                }
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Received shutdown signal");
            }
        }
    } else {
        // Real Nym transport requires the "nym" feature
        #[cfg(feature = "nym")]
        {
            use brisby_core::NymTransport;
            let storage_path = cli.data_dir.join("nym");
            let mut transport = NymTransport::with_storage(storage_path);

            tracing::info!("Connecting to Nym network...");
            transport.connect().await?;
            tracing::info!("Connected to Nym network");
            tracing::info!("Address: {}", transport.our_address().unwrap());

            // Run message loop with ctrl-c handler
            tokio::select! {
                result = handler::run_message_loop(&transport, &handler) => {
                    if let Err(e) = result {
                        tracing::error!("Message loop error: {}", e);
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("Received shutdown signal");
                }
            }

            transport.disconnect().await?;
        }

        #[cfg(not(feature = "nym"))]
        {
            tracing::error!("Nym transport not available. Compile with --features nym or use --mock");
            return Err(anyhow::anyhow!(
                "Nym transport not available. Compile with --features nym or use --mock"
            ));
        }
    }

    // Cancel cleanup task
    cleanup_handle.abort();

    tracing::info!("Shutting down");
    Ok(())
}

/// Run periodic cleanup of expired index entries
async fn run_cleanup_task(index_path: &PathBuf) {
    tracing::info!("Starting cleanup task (interval: {:?})", CLEANUP_INTERVAL);

    loop {
        tokio::time::sleep(CLEANUP_INTERVAL).await;

        // Open a separate connection for cleanup
        match SearchIndex::open(index_path) {
            Ok(index) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                match index.cleanup_expired(now) {
                    Ok(removed) => {
                        if removed > 0 {
                            tracing::info!("Cleanup: removed {} expired entries", removed);
                        } else {
                            tracing::debug!("Cleanup: no expired entries");
                        }
                    }
                    Err(e) => {
                        tracing::error!("Cleanup failed: {}", e);
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to open index for cleanup: {}", e);
            }
        }
    }
}
