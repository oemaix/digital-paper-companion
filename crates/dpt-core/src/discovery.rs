//! Device discovery via mDNS/DNS-SD and manual address probing
//! (protocol §3; FR-CONN-1, FR-CONN-2).
//!
//! Service types:
//! - `_digitalpaper._tcp.local.` (Sony DPT-RP1 / DPT-CP1)
//! - `_dp_fujitsu._tcp.local.` (Fujitsu Quaderno)
//!
//! Implementation plan: browse with `mdns-sd`, then identify each hit via
//! `GET http://{addr}:{port}/register/information` (unauthenticated).

/// mDNS service types announced by compatible devices.
pub const SERVICE_TYPES: [&str; 2] = ["_digitalpaper._tcp.local.", "_dp_fujitsu._tcp.local."];

// TODO(FR-CONN-1): start_discovery() -> impl Stream<DiscoveredDevice>
// TODO(FR-CONN-2): probe(addr) -> Result<DeviceInfo>
