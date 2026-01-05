//! Client configuration

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Data directory path
    pub data_dir: String,

    /// Index provider configuration
    pub index_providers: Vec<IndexProviderConfig>,

    /// DHT configuration
    pub dht: DhtConfig,

    /// Transfer configuration
    pub transfer: TransferConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexProviderConfig {
    /// Name of the provider
    pub name: String,
    /// Nym address of the provider
    pub nym_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhtConfig {
    /// Bootstrap nodes
    pub bootstrap_nodes: Vec<String>,
    /// K parameter (nodes per bucket)
    pub k: usize,
    /// Alpha parameter (lookup parallelism)
    pub alpha: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferConfig {
    /// Maximum concurrent chunk requests
    pub max_concurrent_requests: usize,
    /// Chunk request timeout in seconds
    pub request_timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            data_dir: "~/.brisby".to_string(),
            index_providers: vec![IndexProviderConfig {
                name: "default".to_string(),
                nym_address: "".to_string(), // TODO: Set default provider
            }],
            dht: DhtConfig {
                bootstrap_nodes: vec![],
                k: 20,
                alpha: 3,
            },
            transfer: TransferConfig {
                max_concurrent_requests: 50,
                request_timeout_secs: 30,
            },
        }
    }
}

impl Config {
    /// Load configuration from a file
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Expand ~ in data_dir path
    pub fn data_dir(&self) -> std::path::PathBuf {
        if self.data_dir.starts_with("~/") {
            if let Some(home) = dirs::home_dir() {
                return home.join(&self.data_dir[2..]);
            }
        }
        std::path::PathBuf::from(&self.data_dir)
    }
}
