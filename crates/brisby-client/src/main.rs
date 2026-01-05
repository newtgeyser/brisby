//! Brisby - Privacy-preserving P2P file sharing client

use anyhow::Result;
use brisby_core::Transport;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod config;
mod downloader;
mod local_index;
mod network;
mod seeder;

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

    /// Index provider Nym address (overrides config)
    #[arg(long)]
    index_provider: Option<String>,

    /// Use mock transport (for testing without Nym)
    #[arg(long)]
    mock: bool,

    /// Data directory
    #[arg(short, long, default_value = "~/.brisby")]
    data_dir: String,

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

        /// Seeder Nym address(es) to download from
        #[arg(short, long, required = true)]
        seeder: Vec<String>,

        /// Expected number of chunks (from search results)
        #[arg(short, long, default_value = "1")]
        chunks: u32,

        /// Expected filename
        #[arg(short, long)]
        filename: Option<String>,

        /// Expected file size
        #[arg(long)]
        size: Option<u64>,
    },

    /// List locally shared files
    List,

    /// Show status and statistics
    Status,

    /// Initialize configuration
    Init,

    /// Start seeding (serve files to other peers)
    Seed {
        /// Files to share (optional, loads all from storage if not specified)
        #[arg(short, long)]
        file: Vec<String>,

        /// Also publish to index provider
        #[arg(short, long)]
        publish: bool,
    },
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
            share_file(&file, &cli.data_dir).await?;
        }
        Commands::Search { query, max_results } => {
            search_files(
                &query,
                max_results,
                cli.index_provider.as_deref(),
                cli.mock,
                &cli.data_dir,
            )
            .await?;
        }
        Commands::Download { hash, output, seeder, chunks, filename, size } => {
            download_file(
                &hash,
                output.as_deref(),
                &seeder,
                chunks,
                filename.as_deref(),
                size,
                cli.mock,
                &cli.data_dir,
            )
            .await?;
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
        Commands::Seed { file, publish } => {
            start_seeding(
                &file,
                publish,
                cli.index_provider.as_deref(),
                cli.mock,
                &cli.data_dir,
            )
            .await?;
        }
    }

    Ok(())
}

async fn share_file(path: &str, data_dir: &str) -> Result<()> {
    use std::path::Path;

    let path = Path::new(path);
    if !path.exists() {
        anyhow::bail!("File not found: {}", path.display());
    }

    // Set up chunk storage
    let data_path = expand_path(data_dir);
    std::fs::create_dir_all(&data_path)?;
    let chunks_dir = data_path.join("chunks");

    let mut store = seeder::ChunkStore::new(chunks_dir);

    // Add file to chunk store (this chunks and stores locally)
    tracing::info!("Processing file: {}", path.display());
    let metadata = store.add_file(path)?;

    tracing::info!(
        "File stored: {} bytes, {} chunks",
        metadata.size,
        metadata.chunks.len()
    );
    tracing::info!("Content hash: {}", brisby_core::hash_to_hex(&metadata.content_hash));

    println!("Shared: {}", metadata.filename);
    println!("Hash: {}", brisby_core::hash_to_hex(&metadata.content_hash));
    println!("Size: {} bytes ({} chunks)", metadata.size, metadata.chunks.len());
    println!();
    println!("File is stored locally. To make it available on the network:");
    println!("  brisby seed --publish --index-provider <ADDRESS>");

    Ok(())
}

async fn search_files(
    query: &str,
    max_results: u32,
    index_provider: Option<&str>,
    use_mock: bool,
    data_dir: &str,
) -> Result<()> {
    let index_provider = index_provider
        .ok_or_else(|| anyhow::anyhow!("Index provider address required. Use --index-provider"))?;

    tracing::info!("Searching for: {} (max {} results)", query, max_results);
    tracing::info!("Index provider: {}", index_provider);

    let index_addr = brisby_core::NymAddress::new(index_provider);

    if use_mock {
        // Use mock transport
        let mut transport = brisby_core::transport::mock::MockTransport::new();
        transport.connect().await?;
        tracing::info!("Connected (mock mode)");

        println!("Mock mode: would search for '{}' on {}", query, index_provider);
        println!("(No real network connection in mock mode)");
    } else {
        // Real Nym transport
        #[cfg(feature = "nym")]
        {
            use brisby_core::NymTransport;

            let data_path = expand_path(data_dir);
            std::fs::create_dir_all(&data_path)?;
            let nym_path = data_path.join("nym");

            tracing::info!("Connecting to Nym network...");
            let mut transport = NymTransport::with_storage(nym_path);
            transport.connect().await?;

            tracing::info!("Connected to Nym network");
            if let Some(addr) = transport.our_address() {
                tracing::info!("Our address: {}", addr);
            }

            // Perform search
            tracing::info!("Sending search query...");
            let results = network::search_index_provider(&transport, &index_addr, query, max_results).await?;

            if results.is_empty() {
                println!("No results found for '{}'", query);
            } else {
                println!("Found {} results for '{}':", results.len(), query);
                println!();
                for (i, result) in results.iter().enumerate() {
                    println!(
                        "{}. {} ({} bytes, {} chunks)",
                        i + 1,
                        result.filename,
                        result.size,
                        result.chunk_count
                    );
                    println!("   Hash: {}", brisby_core::hash_to_hex(&result.content_hash));
                    println!("   Relevance: {:.2}", result.relevance);
                    if !result.seeders.is_empty() {
                        println!("   Seeders: {}", result.seeders.len());
                    }
                    println!();
                }
            }

            transport.disconnect().await?;
        }

        #[cfg(not(feature = "nym"))]
        {
            // Suppress unused variable warnings in non-nym build
            let _ = (&index_addr, &data_dir);
            anyhow::bail!("Nym transport not available. Compile with --features nym or use --mock");
        }
    }

    Ok(())
}

fn expand_path(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(&path[2..]);
        }
    }
    PathBuf::from(path)
}

async fn download_file(
    hash: &str,
    output: Option<&str>,
    seeders: &[String],
    chunk_count: u32,
    filename: Option<&str>,
    size: Option<u64>,
    use_mock: bool,
    data_dir: &str,
) -> Result<()> {
    use std::path::Path;

    if seeders.is_empty() {
        anyhow::bail!("At least one seeder address required. Use -s <address>");
    }

    let default_filename = format!("{}.download", &hash[..8]);
    let output_filename = filename.unwrap_or(&default_filename);
    let output_path = Path::new(output.unwrap_or(output_filename));

    tracing::info!("Downloading: {}", hash);
    tracing::info!("From {} seeder(s)", seeders.len());
    tracing::info!("Output: {}", output_path.display());

    if use_mock {
        println!("Mock mode: would download '{}' from {} seeder(s)", hash, seeders.len());
        println!("(No real network connection in mock mode)");
        return Ok(());
    }

    #[cfg(feature = "nym")]
    {
        use brisby_core::{ChunkInfo, FileMetadata, NymTransport};

        let content_hash = brisby_core::hex_to_hash(hash)
            .map_err(|e| anyhow::anyhow!("Invalid hash: {}", e))?;

        // Create a minimal FileMetadata for the downloader
        // In a real scenario, we'd get full metadata from the index provider
        let size_hint = size.unwrap_or(0);
        let chunk_entries: Vec<ChunkInfo> = (0..chunk_count)
            .map(|i| {
                // If we know the total size, derive per-chunk sizes; otherwise mark as unknown (0)
                let chunk_size = if size_hint > 0 {
                    let offset = i as u64 * brisby_core::CHUNK_SIZE as u64;
                    let remaining = size_hint.saturating_sub(offset);
                    remaining.min(brisby_core::CHUNK_SIZE as u64) as u32
                } else {
                    0
                };

                ChunkInfo {
                    index: i,
                    hash: [0u8; 32], // We verify chunks by their own hash in the response
                    size: chunk_size,
                }
            })
            .collect();

        let metadata = FileMetadata {
            content_hash,
            filename: output_filename.to_string(),
            size: size_hint,
            mime_type: None,
            chunks: chunk_entries,
            keywords: vec![],
            created_at: 0,
        };

        let data_path = expand_path(data_dir);
        std::fs::create_dir_all(&data_path)?;
        let nym_path = data_path.join("nym");

        tracing::info!("Connecting to Nym network...");
        let mut transport = NymTransport::with_storage(nym_path);
        transport.connect().await?;

        tracing::info!("Connected to Nym network");

        let seeder_addresses: Vec<brisby_core::NymAddress> = seeders
            .iter()
            .map(|s| brisby_core::NymAddress::new(s))
            .collect();

        let dl = downloader::Downloader::new(&transport);

        println!("Downloading {} chunks from {} seeder(s)...", chunk_count, seeders.len());

        let chunks = dl
            .download_sequential(&metadata, &seeder_addresses, |current, total| {
                if current % 10 == 0 || current == total {
                    println!("Progress: {}/{} chunks", current, total);
                }
            })
            .await?;

        dl.reassemble_to_file(chunks, &metadata, output_path)?;

        println!("Downloaded successfully: {}", output_path.display());

        transport.disconnect().await?;

        Ok(())
    }

    #[cfg(not(feature = "nym"))]
    {
        // Suppress unused variable warnings in non-nym build
        let _ = (&seeders, &chunk_count, &filename, &size, &data_dir);
        anyhow::bail!("Nym transport not available. Compile with --features nym or use --mock");
    }
}

async fn start_seeding(
    files: &[String],
    publish: bool,
    index_provider: Option<&str>,
    use_mock: bool,
    data_dir: &str,
) -> Result<()> {
    use std::path::Path;

    let data_path = expand_path(data_dir);
    std::fs::create_dir_all(&data_path)?;
    let chunks_dir = data_path.join("chunks");

    // Create chunk store and load existing files
    let mut store = seeder::ChunkStore::new(chunks_dir);
    let loaded = store.load_all()?;
    tracing::info!("Loaded {} existing files from storage", loaded);

    // Add any new files
    for file_path in files {
        let path = Path::new(file_path);
        if !path.exists() {
            tracing::warn!("File not found: {}", file_path);
            continue;
        }
        match store.add_file(path) {
            Ok(metadata) => {
                println!("Added: {} ({})", metadata.filename, brisby_core::hash_to_hex(&metadata.content_hash));
            }
            Err(e) => {
                tracing::error!("Failed to add {}: {}", file_path, e);
            }
        }
    }

    let file_count = store.list_files().len();
    if file_count == 0 {
        println!("No files to seed. Use -f <file> to add files.");
        return Ok(());
    }

    println!("Seeding {} file(s)", file_count);
    for metadata in store.list_files() {
        println!("  - {} ({} bytes, {} chunks)",
            metadata.filename,
            metadata.size,
            metadata.chunks.len()
        );
    }

    if use_mock {
        println!("Mock mode: seeder would start here");
        println!("(No real network connection in mock mode)");
        return Ok(());
    }

    #[cfg(feature = "nym")]
    {
        use brisby_core::NymTransport;

        let nym_path = data_path.join("nym");
        std::fs::create_dir_all(&nym_path)?;

        tracing::info!("Connecting to Nym network...");
        let mut transport = NymTransport::with_storage(nym_path);
        transport.connect().await?;

        let our_address = transport.our_address()
            .ok_or_else(|| anyhow::anyhow!("Failed to get our Nym address"))?;

        println!("Connected to Nym network");
        println!("Address: {}", our_address);
        println!();
        println!("Seeder is running. Press Ctrl+C to stop.");

        // Publish to index provider if requested
        if publish {
            if let Some(index_addr) = index_provider {
                let index_nym = brisby_core::NymAddress::new(index_addr);
                let our_nym = our_address.clone();

                for metadata in store.list_files() {
                    tracing::info!("Publishing {} to index provider", metadata.filename);
                    if let Err(e) = network::publish_to_index_provider(&transport, &index_nym, metadata, &our_nym).await {
                        tracing::error!("Failed to publish {}: {}", metadata.filename, e);
                    } else {
                        println!("Published: {}", metadata.filename);
                    }
                }
            } else {
                tracing::warn!("--publish specified but no --index-provider given");
            }
        }

        // Create seeder and run message loop
        let seeder_service = seeder::Seeder::new(store);
        seeder::run_seeder_loop(&transport, &seeder_service).await?;

        transport.disconnect().await?;
        Ok(())
    }

    #[cfg(not(feature = "nym"))]
    {
        let _ = (&index_provider, &publish, &data_dir);
        anyhow::bail!("Nym transport not available. Compile with --features nym or use --mock");
    }
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
