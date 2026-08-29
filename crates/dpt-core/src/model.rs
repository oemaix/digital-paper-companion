//! Shared data types (protocol §6.2, docs/04 §4).
//!
//! The device serializes *every* JSON scalar as a string
//! (`"file_size": "12345"`, `"is_new": "false"`; protocol §6.1). The
//! [`de`] helpers convert those at the serde boundary.

use serde::{Deserialize, Serialize};

/// Network address of a device: an IPv4/IPv6 address (IPv6 possibly with a
/// zone identifier, e.g. `fe80::1%usb0`) or a hostname such as
/// `digitalpaper.local`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceAddr(pub String);

impl DeviceAddr {
    pub fn new(addr: impl Into<String>) -> Self {
        Self(addr.into())
    }

    /// Base URL of the unauthenticated registration server (HTTP, port 8080).
    pub fn registration_base(&self) -> String {
        format!("http://{}:8080", self.host_for_url())
    }

    /// Base URL of the authenticated API server (HTTPS, port 8443).
    pub fn api_base(&self) -> String {
        format!("https://{}:8443", self.host_for_url())
    }

    /// Wraps bare IPv6 literals in brackets for use in a URL authority,
    /// preserving any `%zone` identifier. IPv4 and hostnames pass through.
    fn host_for_url(&self) -> String {
        let a = self.0.trim();
        if a.starts_with('[') {
            return a.to_string();
        }
        // Treat as IPv6 literal if it contains ':' (hostnames/IPv4 do not).
        if a.contains(':') {
            format!("[{a}]")
        } else {
            a.to_string()
        }
    }
}

/// Long-term client credentials produced by registration (protocol §4).
///
/// SECURITY: never log, never serialize into IPC payloads or state files;
/// storage goes through the OS keychain (docs/07 §2).
#[derive(Clone)]
pub struct Credentials {
    /// Client ID: a lowercase UUIDv4 string chosen at registration.
    pub client_id: String,
    /// RSA-2048 private key, PKCS#8 PEM.
    pub private_key_pem: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never leak the private key.
        f.debug_struct("Credentials")
            .field("client_id", &self.client_id)
            .field("private_key_pem", &"<redacted>")
            .finish()
    }
}

/// Result of a successful registration (protocol §4.7, messages M5/M6).
#[derive(Debug, Clone)]
pub struct Registration {
    pub credentials: Credentials,
    /// Device server certificate (PEM) for TLS pinning (docs/07 §3).
    pub device_cert_pem: String,
}

/// Device information from `GET /register/information` (protocol §7.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub serial_number: String,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub firmware_version: Option<String>,
    #[serde(default)]
    pub pkcs12: Option<String>,
}

/// Kind of a storage entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryType {
    Document,
    Folder,
}

/// A document or folder in the device storage tree (protocol §6.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub entry_id: String,
    pub entry_name: String,
    pub entry_path: String,
    pub entry_type: EntryType,
    #[serde(default)]
    pub parent_folder_id: Option<String>,
    #[serde(default)]
    pub created_date: Option<String>,
    #[serde(default)]
    pub modified_date: Option<String>,
    #[serde(default)]
    pub reading_date: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string_u64")]
    pub file_size: Option<u64>,
    #[serde(default)]
    pub file_revision: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "de::opt_string_u64")]
    pub total_page: Option<u64>,
    #[serde(default, deserialize_with = "de::opt_string_bool")]
    pub is_new: Option<bool>,
}

impl Entry {
    pub fn is_folder(&self) -> bool {
        self.entry_type == EntryType::Folder
    }
}

/// Response shape of `GET /documents2` (protocol §7.3.2).
#[derive(Debug, Clone, Deserialize)]
pub struct EntryListResponse {
    #[serde(default, deserialize_with = "de::opt_string_u64")]
    pub count: Option<u64>,
    #[serde(default)]
    pub entry_list: Vec<Entry>,
}

/// Storage figures from `GET /system/status/storage` (protocol §7.8).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStatus {
    #[serde(default, deserialize_with = "de::opt_string_u64")]
    pub capacity: Option<u64>,
    #[serde(default, deserialize_with = "de::opt_string_u64")]
    pub available: Option<u64>,
}

/// Battery figures from `GET /system/status/battery` (protocol §7.8).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryStatus {
    #[serde(default, deserialize_with = "de::opt_string_u64")]
    pub level: Option<u64>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub plugged: Option<String>,
    #[serde(default)]
    pub health: Option<String>,
}

/// A note template (protocol §7.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteTemplate {
    pub template_name: String,
    pub note_template_id: String,
}

/// Serde helpers for the device's string-typed JSON scalars (protocol §6.1).
pub mod de {
    use serde::{Deserialize, Deserializer};

    /// Deserialize an optional numeric string (`"12345"`) into `Option<u64>`.
    /// Accepts a real JSON number too, and treats empty strings as `None`.
    pub fn opt_string_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum StringOrNum {
            Str(String),
            Num(u64),
        }
        match Option::<StringOrNum>::deserialize(deserializer)? {
            None => Ok(None),
            Some(StringOrNum::Num(n)) => Ok(Some(n)),
            Some(StringOrNum::Str(s)) => {
                let s = s.trim();
                if s.is_empty() {
                    Ok(None)
                } else {
                    s.parse::<u64>().map(Some).map_err(serde::de::Error::custom)
                }
            }
        }
    }

    /// Deserialize an optional boolean string (`"true"`/`"false"`) into
    /// `Option<bool>`. Accepts a real JSON bool too.
    pub fn opt_string_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum StringOrBool {
            Str(String),
            Bool(bool),
        }
        match Option::<StringOrBool>::deserialize(deserializer)? {
            None => Ok(None),
            Some(StringOrBool::Bool(b)) => Ok(Some(b)),
            Some(StringOrBool::Str(s)) => match s.trim() {
                "" => Ok(None),
                "true" | "1" => Ok(Some(true)),
                "false" | "0" => Ok(Some(false)),
                other => Err(serde::de::Error::custom(format!(
                    "invalid boolean string: {other:?}"
                ))),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_parses_string_typed_scalars() {
        let json = r#"{
            "entry_id": "id-1",
            "entry_name": "a.pdf",
            "entry_path": "Document/a.pdf",
            "entry_type": "document",
            "file_size": "12345",
            "total_page": "10",
            "is_new": "true"
        }"#;
        let e: Entry = serde_json::from_str(json).unwrap();
        assert_eq!(e.file_size, Some(12345));
        assert_eq!(e.total_page, Some(10));
        assert_eq!(e.is_new, Some(true));
        assert!(!e.is_folder());
    }

    #[test]
    fn folder_without_file_fields_parses() {
        let json = r#"{
            "entry_id": "id-2",
            "entry_name": "Papers",
            "entry_path": "Document/Papers",
            "entry_type": "folder"
        }"#;
        let e: Entry = serde_json::from_str(json).unwrap();
        assert!(e.is_folder());
        assert_eq!(e.file_size, None);
        assert_eq!(e.is_new, None);
    }

    #[test]
    fn ipv6_address_is_bracketed_with_zone() {
        let a = DeviceAddr::new("fe80::1%usb0");
        assert_eq!(a.api_base(), "https://[fe80::1%usb0]:8443");
        let v4 = DeviceAddr::new("10.0.1.12");
        assert_eq!(v4.registration_base(), "http://10.0.1.12:8080");
    }
}
