use serde::{Deserialize, Serialize};
use std::fs;
use tracing::info;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub burst_size: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SourceConfig {
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub source: SourceConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "0.0.0.0".to_string(),
                port: 8000,
                burst_size: 131_072,
            },
            source: SourceConfig {
                username: Some("source".to_string()),
                password: Some("hackme".to_string()),
            },
        }
    }
}

impl Config {
    pub fn initialize_from_path<P: AsRef<std::path::Path>>(path: P) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            anyhow::bail!("Configuration file not found on disk");
        }
        let content = fs::read_to_string(path)?;
        let config = toml::from_str(&content)?;
        info!("Loaded configuration from {:?}", path);
        Ok(config)
    }
}
