//! Plugin configuration — loaded from `config/config.toml`.

use anyhow::Result;
use serde::Deserialize;

/// Operator-config JSON Schema, published on the capability manifest so the
/// hc-web editor renders a typed form. `None` without the `schema` feature.
#[cfg(feature = "schema")]
pub fn config_schema() -> Option<serde_json::Value> {
    serde_json::to_value(schemars::schema_for!(Config)).ok()
}

#[cfg(not(feature = "schema"))]
pub fn config_schema() -> Option<serde_json::Value> {
    None
}

// ---------------------------------------------------------------------------
// Top-level config
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Config {
    pub homecore: HomecoreConfig,
    #[serde(default)]
    pub isy: IsyConfig,
    #[serde(default)]
    pub logging: crate::logging::LoggingConfig,
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Cannot read config {path}: {e}"))?;
        toml::from_str(&text).map_err(|e| anyhow::anyhow!("Config parse error in {path}: {e}"))
    }
}

// ---------------------------------------------------------------------------
// [homecore] — MQTT broker connection and plugin identity
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

fn default_broker_host() -> String {
    "127.0.0.1".into()
}
fn default_broker_port() -> u16 {
    1883
}
fn default_plugin_id() -> String {
    "plugin.isy".into()
}

// ---------------------------------------------------------------------------
// [isy] — ISY/IoX controller connection
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct IsyConfig {
    /// ISY/IoX controller hostname or IP address.
    #[serde(default)]
    pub host: String,

    /// HTTP port.  Default 80 (HTTP) or 443 (TLS).
    #[serde(default = "default_isy_port")]
    pub port: u16,

    /// ISY administrator username (usually "admin").
    #[serde(default)]
    pub username: String,

    /// ISY administrator password.
    #[serde(default)]
    pub password: String,

    /// Use HTTPS/WSS instead of HTTP/WS.
    /// The ISY typically uses a self-signed certificate; certificate
    /// verification is skipped when tls = true.
    #[serde(default)]
    pub tls: bool,
}

impl Default for IsyConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: default_isy_port(),
            username: String::new(),
            password: String::new(),
            tls: false,
        }
    }
}

fn default_isy_port() -> u16 {
    80
}
