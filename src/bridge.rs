//! Bridge between the ISY/IoX controller and HomeCore MQTT.
//!
//! # Flow
//!
//! 1. Connect MQTT → subscribe `homecore/devices/isy_+/cmd`
//! 2. Fetch all ISY nodes + full status via REST
//! 3. Register every enabled node with HomeCore, publish initial state
//! 4. Connect WebSocket to ISY `/rest/subscribe`
//! 5. Event loop:
//!    - ISY WS event → translate → `homecore/devices/isy_{addr}/state/partial`
//!    - MQTT cmd     → translate → ISY REST command (via reqwest)
//! 6. Reconnect on any error with exponential back-off

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, http::HeaderValue},
};
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::device::{classify_node, cmd_to_isy, event_to_patch, node_to_state, DeviceKind};
use crate::isy::{addr_to_device_id, parse_event_xml, IsyClient};

// ---------------------------------------------------------------------------
// Device registry (shared read-only after startup)
// ---------------------------------------------------------------------------

/// Everything needed to translate between HomeCore device IDs and ISY.
struct Registry {
    /// device_id → device kind
    kinds: HashMap<String, DeviceKind>,
    /// device_id → original ISY address (for REST commands)
    addrs: HashMap<String, String>,
}

impl Registry {
    fn kind(&self, device_id: &str) -> Option<&DeviceKind> {
        self.kinds.get(device_id)
    }

    fn addr(&self, device_id: &str) -> Option<&str> {
        self.addrs.get(device_id).map(String::as_str)
    }
}

// ---------------------------------------------------------------------------
// Public struct
// ---------------------------------------------------------------------------

pub struct Bridge {
    pub config: Config,
    pub published_ids_cache_path: PathBuf,
}

impl Bridge {
    pub async fn run(self) -> Result<()> {
        let mut backoff = 2u64;
        loop {
            match self.run_once().await {
                Ok(()) => {
                    info!("Bridge exited cleanly");
                    break;
                }
                Err(e) => {
                    error!(error = %e, backoff_secs = backoff, "Bridge error; reconnecting");
                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(60);
                }
            }
        }
        Ok(())
    }

    async fn run_once(&self) -> Result<()> {
        let cfg = &self.config;

        // ── MQTT ─────────────────────────────────────────────────────────
        let (mqtt, mut eventloop) = {
            let mut opts = MqttOptions::new(
                &cfg.homecore.plugin_id,
                &cfg.homecore.broker_host,
                cfg.homecore.broker_port,
            );
            opts.set_keep_alive(Duration::from_secs(30));
            opts.set_clean_session(true);
            if !cfg.homecore.password.is_empty() {
                opts.set_credentials(&cfg.homecore.plugin_id, &cfg.homecore.password);
            }
            AsyncClient::new(opts, 128)
        };

        mqtt.subscribe("homecore/devices/+/cmd", QoS::AtLeastOnce).await?;
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => break,
                Ok(_) => {}
                Err(e) => bail!("MQTT connect error: {e}"),
            }
        }
        info!(broker = %cfg.homecore.broker_host, "MQTT connected");

        // ── ISY REST: load nodes + full status ───────────────────────────
        let isy = Arc::new(
            IsyClient::new(
                &cfg.isy.host,
                cfg.isy.port,
                &cfg.isy.username,
                &cfg.isy.password,
                cfg.isy.tls,
            )
            .context("create ISY client")?,
        );

        let mut nodes = isy.get_nodes().await.context("GET /rest/nodes")?;
        let status    = isy.get_status().await.context("GET /rest/status")?;

        // Merge full property status into each node
        for node in &mut nodes {
            if let Some(props) = status.get(&node.address) {
                for (id, prop) in props {
                    node.properties.insert(id.clone(), prop.clone());
                }
            }
        }
        info!(total = nodes.len(), "Loaded ISY nodes");

        // ── Register devices and build registry ───────────────────────────
        let plugin_id = &cfg.homecore.plugin_id;
        let current_ids: Vec<String> = nodes
            .iter()
            .filter(|node| node.enabled)
            .map(|node| node.device_id())
            .collect();

        for stale_id in load_published_ids(&self.published_ids_cache_path)
            .into_iter()
            .filter(|device_id| !current_ids.iter().any(|current| current == device_id))
        {
            if let Err(e) = unregister_device(&mqtt, plugin_id, &stale_id).await {
                warn!(device_id = %stale_id, error = %e, "Failed to unregister stale ISY device");
            } else {
                info!(device_id = %stale_id, "Unregistered stale ISY device");
            }
        }

        let mut kinds: HashMap<String, DeviceKind> = HashMap::new();
        let mut addrs: HashMap<String, String>     = HashMap::new();

        for node in &nodes {
            if !node.enabled {
                debug!(addr = %node.address, "Skipping disabled node");
                continue;
            }
            let kind      = classify_node(node);
            let device_id = node.device_id();

            register_device(&mqtt, plugin_id, &device_id, &node.name, &kind).await?;
            let state = node_to_state(node, &kind);
            publish_state(&mqtt, &device_id, &state).await?;
            publish_avail(&mqtt, &device_id, true).await?;

            debug!(device_id, kind = kind.as_str(), "Registered");
            addrs.insert(device_id.clone(), node.address.clone());
            kinds.insert(device_id, kind);
        }
        info!(registered = kinds.len(), "All ISY devices registered with HomeCore");
        save_published_ids(&self.published_ids_cache_path, &current_ids)?;

        let registry = Arc::new(Registry { kinds, addrs });

        // ── ISY WebSocket subscription ────────────────────────────────────
        let ws_url = isy.ws_url();
        let mut ws_req = ws_url
            .parse::<tokio_tungstenite::tungstenite::http::Uri>()
            .with_context(|| format!("parse WS URL {ws_url}"))?
            .into_client_request()
            .context("build WS request")?;
        {
            let h = ws_req.headers_mut();
            h.insert("Authorization",
                     HeaderValue::from_str(&isy.basic_auth_header())
                         .context("auth header")?);
            h.insert("Sec-WebSocket-Protocol",
                     HeaderValue::from_static("ISYSUB"));
            h.insert("Origin",
                     HeaderValue::from_static("com.universal-devices.websockets.isy"));
        }
        let (ws_stream, _) = connect_async(ws_req)
            .await
            .with_context(|| format!("WebSocket connect to {ws_url}"))?;
        let (_ws_tx, mut ws_rx) = ws_stream.split();
        info!(url = %ws_url, "ISY WebSocket connected");

        // ── Task: WS events → MQTT ────────────────────────────────────────
        let mqtt_ws   = mqtt.clone();
        let reg_ws    = Arc::clone(&registry);
        let ws_task   = tokio::spawn(async move {
            use tokio_tungstenite::tungstenite::Message;
            loop {
                match ws_rx.next().await {
                    Some(Ok(Message::Text(text))) => {
                        handle_ws_event(&text, &mqtt_ws, &reg_ws).await;
                    }
                    Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Close(_))) => {
                        warn!("ISY WebSocket closed by server");
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => { error!(error = %e, "ISY WebSocket read error"); break; }
                    None => { warn!("ISY WebSocket stream ended"); break; }
                }
            }
        });

        // ── Task: ISY command sender ─────────────────────────────────────
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<MqttCmd>(64);
        let isy_cmd  = Arc::clone(&isy);
        let reg_cmd  = Arc::clone(&registry);
        let cmd_task = tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                let Some(kind) = reg_cmd.kind(&cmd.device_id) else {
                    debug!(device_id = %cmd.device_id, "Unknown device — ignoring cmd");
                    continue;
                };
                let Some(addr) = reg_cmd.addr(&cmd.device_id) else { continue };

                let cmds = cmd_to_isy(&cmd.payload, kind);
                if cmds.is_empty() {
                    debug!(device_id = %cmd.device_id, "No ISY commands for payload");
                    continue;
                }

                for ic in &cmds {
                    if let Err(e) = isy_cmd.send_cmd(addr, ic.cmd, ic.value).await {
                        warn!(device_id = %cmd.device_id, cmd = ic.cmd, error = %e,
                              "ISY command failed");
                    } else {
                        info!(device_id = %cmd.device_id, cmd = ic.cmd, value = ?ic.value,
                              "ISY command sent");
                    }
                }
            }
        });

        // ── Main: MQTT event loop ─────────────────────────────────────────
        let result = mqtt_event_loop(&mut eventloop, &cmd_tx, &registry).await;
        ws_task.abort();
        cmd_task.abort();
        result
    }
}

// ---------------------------------------------------------------------------
// MQTT helpers
// ---------------------------------------------------------------------------

async fn publish_state(client: &AsyncClient, device_id: &str, state: &Value) -> Result<()> {
    client
        .publish(
            format!("homecore/devices/{device_id}/state"),
            QoS::AtLeastOnce, true,
            serde_json::to_vec(state)?,
        )
        .await.context("publish state")
}

async fn publish_partial(client: &AsyncClient, device_id: &str, patch: &Value) -> Result<()> {
    client
        .publish(
            format!("homecore/devices/{device_id}/state/partial"),
            QoS::AtLeastOnce, false,
            serde_json::to_vec(patch)?,
        )
        .await.context("publish partial")
}

async fn publish_avail(client: &AsyncClient, device_id: &str, online: bool) -> Result<()> {
    client
        .publish(
            format!("homecore/devices/{device_id}/availability"),
            QoS::AtLeastOnce, true,
            if online { "online" } else { "offline" },
        )
        .await.context("publish availability")
}

async fn clear_retained_topic(client: &AsyncClient, topic: String) -> Result<()> {
    client
        .publish(topic, QoS::AtLeastOnce, true, Vec::<u8>::new())
        .await
        .context("clear retained topic")
}

async fn unregister_device(client: &AsyncClient, plugin_id: &str, device_id: &str) -> Result<()> {
    clear_retained_topic(client, format!("homecore/devices/{device_id}/state")).await?;
    clear_retained_topic(client, format!("homecore/devices/{device_id}/availability")).await?;
    clear_retained_topic(client, format!("homecore/devices/{device_id}/schema")).await?;

    let payload = serde_json::json!({
        "device_id": device_id,
        "plugin_id": plugin_id,
    });
    client
        .publish(
            format!("homecore/plugins/{plugin_id}/unregister"),
            QoS::AtLeastOnce,
            false,
            serde_json::to_vec(&payload)?,
        )
        .await
        .context("unregister device")
}

async fn register_device(
    client: &AsyncClient, plugin_id: &str,
    device_id: &str, name: &str, kind: &DeviceKind,
) -> Result<()> {
    let payload = serde_json::json!({
        "device_id":   device_id,
        "plugin_id":   plugin_id,
        "name":        name,
        "device_type": kind.as_str(),
    });
    client
        .publish(
            format!("homecore/plugins/{plugin_id}/register"),
            QoS::AtLeastOnce, false,
            serde_json::to_vec(&payload)?,
        )
        .await.context("register device")
}

fn load_published_ids(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
        .unwrap_or_default()
}

fn save_published_ids(path: &Path, device_ids: &[String]) -> Result<()> {
    let payload = serde_json::to_vec_pretty(device_ids)?;
    std::fs::write(path, payload)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// WebSocket event handler
// ---------------------------------------------------------------------------

async fn handle_ws_event(text: &str, mqtt: &AsyncClient, reg: &Registry) {
    // The ISY may batch multiple XML events in one frame; split on </Event>
    for part in text.split("</Event>") {
        if part.trim().is_empty() { continue; }
        let xml = format!("{part}</Event>");

        let Some(event) = parse_event_xml(&xml) else { continue };

        if event.is_system() {
            debug!(control = %event.control, "ISY system/heartbeat event");
            continue;
        }

        let device_id = addr_to_device_id(&event.node_addr);
        let Some(kind) = reg.kind(&device_id) else {
            debug!(device_id, control = %event.control, "Event for unknown device — ignored");
            continue;
        };

        if let Some(patch) = event_to_patch(&event, kind) {
            if let Err(e) = publish_partial(mqtt, &device_id, &patch).await {
                warn!(device_id, error = %e, "Failed to publish state patch");
            } else {
                debug!(device_id, control = %event.control, value = event.value,
                       "Published state patch");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// MQTT event loop
// ---------------------------------------------------------------------------

struct MqttCmd {
    device_id: String,
    payload:   Value,
}

async fn mqtt_event_loop(
    eventloop: &mut EventLoop,
    cmd_tx:    &mpsc::Sender<MqttCmd>,
    reg:       &Registry,
) -> Result<()> {
    loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::Publish(p))) => {
                handle_mqtt_cmd(&p.topic, &p.payload, cmd_tx, reg).await;
            }
            Ok(_) => {}
            Err(e) => bail!("MQTT error: {e}"),
        }
    }
}

async fn handle_mqtt_cmd(
    topic:   &str,
    payload: &[u8],
    cmd_tx:  &mpsc::Sender<MqttCmd>,
    reg:     &Registry,
) {
    // Topic: homecore/devices/isy_{addr}/cmd
    let parts: Vec<&str> = topic.split('/').collect();
    if parts.len() != 4 || parts[3] != "cmd" { return; }
    let device_id = parts[2];
    if !device_id.starts_with("isy_") { return; }
    if reg.kind(device_id).is_none() { return; }

    let payload: Value = match serde_json::from_slice(payload) {
        Ok(v) => v,
        Err(e) => { warn!(topic, error = %e, "Non-JSON command payload"); return; }
    };
    info!(device_id, "Received HomeCore command");

    if cmd_tx.send(MqttCmd { device_id: device_id.to_string(), payload }).await.is_err() {
        warn!("Command task gone — dropping cmd");
    }
}
