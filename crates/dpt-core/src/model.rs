//! Shared data types (protocol §6.2, docs/04 §4).
//!
//! Note: the device serializes *every* JSON scalar as a string
//! (`"file_size": "12345"`, `"is_new": "false"`; protocol §6.1). Dedicated
//! serde helpers (`string_bool`, `string_u64`, `string_date`) will live here
//! and convert at the deserialization boundary.

use serde::{Deserialize, Serialize};

/// Network address of a device: an IPv4/IPv6 address (IPv6 possibly with a
/// zone identifier, e.g. `fe80::1%usb0`) or a hostname such as
/// `digitalpaper.local`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceAddr(pub String);

/// Long-term client credentials produced by registration (protocol §4).
///
/// SECURITY: never log, never serialize into IPC payloads or state files;
/// storage goes through the OS keychain (docs/07 §2).
#[derive(Clone)]
pub struct Credentials {
    /// Client ID: a lowercase UUIDv4 string chosen at registration.
    pub client_id: String,
    /// RSA-2048 private key, PEM-encoded.
    pub private_key_pem: String,
}

/// Result of a successful registration (protocol §4.7, message M5/M6).
pub struct Registration {
    pub credentials: Credentials,
    /// Device server certificate (PEM) for TLS pinning (docs/07 §3).
    pub device_cert_pem: String,
}

/// Device information from `GET /register/information` (protocol §7.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub serial_number: String,
    // TODO: model/firmware fields once captured from real hardware.
}

/// A document or folder in the device storage tree (protocol §6.2).
///
/// TODO: full field set with string-typed conversions; kept minimal until
/// the entries API (FR-BRW-1) is implemented.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub entry_id: String,
    pub entry_name: String,
    pub entry_path: String,
    pub entry_type: EntryType,
    pub parent_folder_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryType {
    Document,
    Folder,
}
