//! Device discovery via mDNS/DNS-SD and manual address probing
//! (protocol §3; FR-CONN-1, FR-CONN-2).
//!
//! Service types:
//! - `_digitalpaper._tcp.local.` (Sony DPT-RP1 / DPT-CP1)
//! - `_dp_fujitsu._tcp.local.` (Fujitsu Quaderno)
//!
//! Discovery only works for a few minutes after the device's Wi-Fi setting
//! is switched on. Manual address entry (protocol §7.1 probe via
//! [`crate::client::DeviceClient::probe`]) is always available as a fallback.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent};

use crate::Error;

/// mDNS service types announced by compatible devices.
pub const SERVICE_TYPES: [&str; 2] = ["_digitalpaper._tcp.local.", "_dp_fujitsu._tcp.local."];

/// A device found on the local network via mDNS.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Discovered {
    /// IP address (usable directly as a [`crate::model::DeviceAddr`]).
    pub address: String,
    /// mDNS instance name (human-ish label until probed for the serial).
    pub name: String,
    /// Advertised port (typically 8080, the registration server).
    pub port: u16,
}

/// Browses for compatible devices for up to `timeout`, returning the unique
/// set discovered (deduplicated by address).
pub async fn discover(timeout: Duration) -> Result<Vec<Discovered>, Error> {
    tokio::task::spawn_blocking(move || discover_blocking(timeout))
        .await
        .map_err(|e| Error::Network(format!("discovery task failed: {e}")))?
}

fn discover_blocking(timeout: Duration) -> Result<Vec<Discovered>, Error> {
    let mdns = ServiceDaemon::new().map_err(|e| Error::Network(e.to_string()))?;
    let mut receivers = Vec::new();
    for st in SERVICE_TYPES {
        receivers.push(mdns.browse(st).map_err(|e| Error::Network(e.to_string()))?);
    }

    let mut found: HashMap<String, Discovered> = HashMap::new();
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let slice = Duration::from_millis(200).min(remaining);
        for rx in &receivers {
            if let Ok(ServiceEvent::ServiceResolved(info)) = rx.recv_timeout(slice) {
                if let Some(ip) = info.get_addresses().iter().next() {
                    let address = ip.to_string();
                    found.entry(address.clone()).or_insert_with(|| Discovered {
                        address,
                        name: info.get_fullname().to_string(),
                        port: info.get_port(),
                    });
                }
            }
        }
    }

    for st in SERVICE_TYPES {
        let _ = mdns.stop_browse(st);
    }
    let _ = mdns.shutdown();
    Ok(found.into_values().collect())
}
