//! `hc-isy` — HomeCore plugin for Universal Devices ISY/IoX controllers.
//!
//! Connects to an ISY994i, eisy, or Polisy controller via HTTP REST and
//! WebSocket, registers all nodes as HomeCore devices, and bridges real-time
//! state changes bidirectionally between ISY and the HomeCore MQTT bus.
//!
//! ## Supported device types
//!
//! | ISY category / UOM | HomeCore type |
//! |---|---|
//! | Insteon dimmer (UOM 51, cat 1) | `light` |
//! | Insteon relay / Z-Wave switch (UOM 78) | `switch` |
//! | Door/window sensors | `contact_sensor` |
//! | Motion sensors | `motion_sensor` |
//! | Moisture / leak sensors | `water_sensor` |
//! | Unknown binary sensors | `binary_sensor` |
//! | Temperature, humidity, power, … | `sensor` |
//! | Deadbolt / Z-Wave lock (UOM 11) | `lock` |
//! | Garage door / shade / motor (UOM 97, cat 14) | `cover` |
//! | Insteon FanLinc (cat 1.46) | `fan` |
//! | Insteon thermostat (cat 5) | `thermostat` |
//! | ISY scenes / node groups | `scene` |
//!
//! ## Usage
//!
//! ```sh
//! hc-isy [config/config.toml]
//! ```

mod bridge;
mod config;
mod device;
mod isy;
mod logging;

use bridge::Bridge;
use config::Config;
use std::path::{Path, PathBuf};
use tracing::error;

#[tokio::main]
async fn main() {
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/config.toml".to_string());

    let _log_guard = init_logging(&config_path);

    let cfg = match Config::load(&config_path) {
        Ok(c)  => c,
        Err(e) => {
            error!(error = %e, path = %config_path, "Failed to load config");
            std::process::exit(1);
        }
    };

    tracing::info!(
        config      = %config_path,
        plugin_id   = %cfg.homecore.plugin_id,
        broker_host = %cfg.homecore.broker_host,
        broker_port = cfg.homecore.broker_port,
        isy_host    = %cfg.isy.host,
        isy_port    = cfg.isy.port,
        isy_tls     = cfg.isy.tls,
        "hc-isy starting",
    );

    if let Err(e) = (Bridge {
        config: cfg,
        published_ids_cache_path: published_ids_cache_path(&config_path),
    })
    .run()
    .await
    {
        error!(error = %e, "Bridge exited with error");
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Logging: stderr (RUST_LOG filtered) + rotating compressed file in logs/
// ---------------------------------------------------------------------------

fn init_logging(config_path: &str) -> tracing_appender::non_blocking::WorkerGuard {
    #[derive(serde::Deserialize, Default)]
    struct Bootstrap {
        #[serde(default)]
        logging: logging::LoggingConfig,
    }
    let bootstrap: Bootstrap = std::fs::read_to_string(config_path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default();
    logging::init_logging(config_path, "hc-isy", "hc_isy=info", &bootstrap.logging)
}

fn published_ids_cache_path(config_path: &str) -> PathBuf {
    Path::new(config_path)
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".published-device-ids.json")
}
