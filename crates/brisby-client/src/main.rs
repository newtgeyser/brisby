//! Brisby - Privacy-preserving P2P file sharing client

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod config;
mod downloader;
mod local_index;

#[derive(Parser)]
#[command(name = "brisby")]
#[command(about = "Privacy-preserving P2P file sharing", long_about = None)]
struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "~/.brisby/config.toml")]
    config: String,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Share a file on the network
    Share {
        /// Path to the file to share
        #[arg(required = true)]
        file: String,
    },

    /// Search for files
    Search {
        /// Search query
        #[arg(required = true)]
        query: String,

        /// Maximum number of results
        #[arg(short, long, default_value = "20")]
        max_results: u32,
    },

    /// Download a file by its content hash
    Download {
        /// Content hash (hex-encoded)
        #[arg(required = true)]
        hash: String,

        /// Output path
        #[arg(short, long)]
        output: Option<String>,
    },

    /// List locally shared files
    List,

    /// Show status and statistics
    Status,

    /// Initialize configuration
    Init,
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

    match cli.command {
        Commands::Share { file } => {
            share_file(&file).await?;
        }
        Commands::Search { query, max_results } => {
            search_files(&query, max_results).await?;
        }
        Commands::Download { hash, output } => {
            download_file(&hash, output.as_deref()).await?;
        }
        Commands::List => {
            list_files().await?;
        }
        Commands::Status => {
            show_status().await?;
        }
        Commands::Init => {
            init_config().await?;
        }
    }

    Ok(())
}

async fn share_file(path: &str) -> Result<()> {
    use brisby_core::chunk::chunk_file;
    use std::path::Path;

    let path = Path::new(path);
    if !path.exists() {
        anyhow::bail!("File not found: {}", path.display());
    }

    tracing::info!("Chunking file: {}", path.display());
    let (metadata, chunks) = chunk_file(path)?;

    tracing::info!(
        "File chunked: {} bytes, {} chunks",
        metadata.size,
        chunks.len()
    );
    tracing::info!("Content hash: {}", brisby_core::hash_to_hex(&metadata.content_hash));

    // TODO: Store chunks locally
    // TODO: Publish to index provider
    // TODO: Announce to DHT

    println!("Shared: {}", metadata.filename);
    println!("Hash: {}", brisby_core::hash_to_hex(&metadata.content_hash));
    println!("Size: {} bytes ({} chunks)", metadata.size, chunks.len());

    Ok(())
}

async fn search_files(query: &str, max_results: u32) -> Result<()> {
    tracing::info!("Searching for: {} (max {} results)", query, max_results);

    // TODO: Connect to Nym
    // TODO: Query index providers
    // TODO: Merge and display results

    println!("Search functionality not yet implemented");
    println!("Query: {}", query);

    Ok(())
}

async fn download_file(hash: &str, output: Option<&str>) -> Result<()> {
    let content_hash = brisby_core::hex_to_hash(hash)
        .map_err(|e| anyhow::anyhow!("Invalid hash: {}", e))?;

    tracing::info!("Downloading: {}", hash);

    // TODO: Query DHT for seeders
    // TODO: Request chunks from seeders
    // TODO: Reassemble file

    let output_path = output.unwrap_or("download");
    println!("Download functionality not yet implemented");
    println!("Hash: {}", hash);
    println!("Output: {}", output_path);

    Ok(())
}

async fn list_files() -> Result<()> {
    // TODO: Query local index
    println!("List functionality not yet implemented");
    Ok(())
}

async fn show_status() -> Result<()> {
    println!("Brisby v{}", env!("CARGO_PKG_VERSION"));
    println!("Protocol version: {}", brisby_core::PROTOCOL_VERSION);

    // TODO: Show Nym connection status
    // TODO: Show DHT status
    // TODO: Show shared files count

    Ok(())
}

async fn init_config() -> Result<()> {
    use config::Config;

    let config_dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
        .join(".brisby");

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir)?;
        tracing::info!("Created config directory: {}", config_dir.display());
    }

    let config_path = config_dir.join("config.toml");
    if config_path.exists() {
        println!("Config already exists at: {}", config_path.display());
        return Ok(());
    }

    let config = Config::default();
    let toml = toml::to_string_pretty(&config)?;
    std::fs::write(&config_path, toml)?;

    // Create other directories
    std::fs::create_dir_all(config_dir.join("chunks"))?;
    std::fs::create_dir_all(config_dir.join("downloads"))?;
    std::fs::create_dir_all(config_dir.join("nym"))?;

    println!("Initialized Brisby at: {}", config_dir.display());

    Ok(())
}
