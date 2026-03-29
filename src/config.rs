//! Plugin configuration — loaded from `config/config.toml`.

use anyhow::Result;
use serde::Deserialize;

use crate::logging::LoggingConfig;

// ---------------------------------------------------------------------------
// Top-level config
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct Config {
    pub homecore: HomecoreConfig,
    pub isy: IsyConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Cannot read config {path}: {e}"))?;
        toml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("Config parse error in {path}: {e}"))
    }
}

// ---------------------------------------------------------------------------
// [homecore] — MQTT broker connection and plugin identity
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct HomecoreConfig {
    #[serde(default = "default_broker_host")]
    pub broker_host: String,
    #[serde(default = "default_broker_port")]
    pub broker_port: u16,
    #[serde(default = "default_plugin_id")]
    pub plugin_id: String,
    #[serde(default)]
    pub password: String,
}

fn default_broker_host() -> String { "127.0.0.1".into() }
fn default_broker_port() -> u16    { 1883 }
fn default_plugin_id()   -> String { "plugin.isy".into() }

// ---------------------------------------------------------------------------
// [isy] — ISY/IoX controller connection
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
pub struct IsyConfig {
    /// ISY/IoX controller hostname or IP address.
    pub host: String,

    /// HTTP port.  Default 80 (HTTP) or 443 (TLS).
    #[serde(default = "default_isy_port")]
    pub port: u16,

    /// ISY administrator username (usually "admin").
    pub username: String,

    /// ISY administrator password.
    pub password: String,

    /// Use HTTPS/WSS instead of HTTP/WS.
    /// The ISY typically uses a self-signed certificate; certificate
    /// verification is skipped when tls = true.
    #[serde(default)]
    pub tls: bool,
}

fn default_isy_port() -> u16 { 80 }
