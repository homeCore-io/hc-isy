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
//! | Door/window, motion, moisture sensors | `binary_sensor` |
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

use bridge::Bridge;
use config::Config;
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

    if let Err(e) = (Bridge { config: cfg }).run().await {
        error!(error = %e, "Bridge exited with error");
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Logging: stderr (RUST_LOG filtered) + rolling daily file in logs/
// ---------------------------------------------------------------------------

fn init_logging(config_path: &str) -> tracing_appender::non_blocking::WorkerGuard {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

    let log_dir = std::path::Path::new(config_path)
        .parent()                      // config/
        .and_then(|p| p.parent())      // plugin root
        .map(|p| p.join("logs"))
        .unwrap_or_else(|| std::path::PathBuf::from("logs"));
    std::fs::create_dir_all(&log_dir).ok();

    let file_appender = tracing_appender::rolling::daily(&log_dir, "hc-isy.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let stderr_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "hc_isy=info".parse().unwrap());

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(stderr_filter);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_filter(EnvFilter::new("debug"));

    tracing_subscriber::registry()
        .with(stderr_layer)
        .with(file_layer)
        .init();

    guard
}
