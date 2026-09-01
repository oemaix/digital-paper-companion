//! Wi-Fi management (protocol §7.6; FR-SET-4).
//!
//! SSIDs are Base64-encoded (UTF-8 bytes) in list/register payloads.
//! Passphrases are forwarded to the device and never persisted (NFR-SEC-5).

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::api::entries::form_encode;
use crate::client::DeviceClient;
use crate::Error;

/// A stored or scanned access point (protocol §7.6). The SSID is decoded
/// from the wire's Base64 form; unknown fields are preserved in `extra`.
#[derive(Debug, Clone, Serialize)]
pub struct AccessPoint {
    pub ssid: String,
    pub security: String,
    #[serde(default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Configuration for registering a network
/// (`PUT /system/controls/wifi_accesspoints/register`, protocol §7.6).
/// The device wants every value as a string; booleans become
/// `"true"`/`"false"` and the SSID is Base64-wrapped on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WifiNetworkConfig {
    pub ssid: String,
    /// `"psk"` (WPA/WPA2 personal) or `"nonsec"` (open network).
    pub security: String,
    /// Passphrase; empty for open networks. Sent to the device, never stored.
    #[serde(default)]
    pub passwd: String,
    #[serde(default = "default_true")]
    pub dhcp: bool,
    #[serde(default)]
    pub static_address: String,
    #[serde(default)]
    pub gateway: String,
    /// Prefix length as a string, e.g. `"24"`.
    #[serde(default)]
    pub network_mask: String,
    #[serde(default)]
    pub dns1: String,
    #[serde(default)]
    pub dns2: String,
    #[serde(default)]
    pub proxy: bool,
}

fn default_true() -> bool {
    true
}

/// Encodes an SSID the way the device expects it (Base64 of the UTF-8 bytes).
pub fn encode_ssid(ssid: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(ssid.as_bytes())
}

/// Decodes a wire SSID; falls back to the raw string when it is not valid
/// Base64 (defensive — some firmwares might send it plain).
pub fn decode_ssid(wire: &str) -> String {
    base64::engine::general_purpose::STANDARD
        .decode(wire.as_bytes())
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_else(|| wire.to_string())
}

#[derive(Deserialize)]
struct RawAccessPoint {
    #[serde(default)]
    ssid: String,
    #[serde(default)]
    security: String,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct ApList {
    #[serde(default)]
    aplist: Vec<RawAccessPoint>,
}

impl From<RawAccessPoint> for AccessPoint {
    fn from(raw: RawAccessPoint) -> Self {
        AccessPoint {
            ssid: decode_ssid(&raw.ssid),
            security: raw.security,
            extra: raw.extra,
        }
    }
}

impl DeviceClient {
    /// Whether the Wi-Fi radio is on (`GET /system/configs/wifi`).
    pub async fn wifi_enabled(&self) -> Result<bool, Error> {
        Ok(self.config("wifi").await? == "on")
    }

    /// Switches the Wi-Fi radio on or off (`PUT /system/configs/wifi`).
    /// Note: turning it off while connected over Wi-Fi drops the connection.
    pub async fn set_wifi_enabled(&self, on: bool) -> Result<(), Error> {
        self.set_config("wifi", if on { "on" } else { "off" }).await
    }

    /// Networks stored on the device (`GET /system/configs/wifi_accesspoints`).
    pub async fn stored_access_points(&self) -> Result<Vec<AccessPoint>, Error> {
        let resp: ApList = self.get_json("/system/configs/wifi_accesspoints").await?;
        Ok(resp.aplist.into_iter().map(Into::into).collect())
    }

    /// Scans for visible networks
    /// (`POST /system/controls/wifi_accesspoints/scan`). Takes a few seconds
    /// on the device.
    pub async fn scan_access_points(&self) -> Result<Vec<AccessPoint>, Error> {
        let resp = self
            .send(|http, base| http.post(format!("{base}/system/controls/wifi_accesspoints/scan")))
            .await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(Error::Api {
                status: status.as_u16(),
                message: "wifi scan failed".into(),
            });
        }
        let list: ApList = serde_json::from_str(&text)
            .map_err(|e| Error::Protocol(format!("malformed scan response: {e}")))?;
        Ok(list.aplist.into_iter().map(Into::into).collect())
    }

    /// Adds or reconfigures a network
    /// (`PUT /system/controls/wifi_accesspoints/register`). The passphrase
    /// goes to the device and is never persisted by this library.
    pub async fn register_access_point(&self, cfg: &WifiNetworkConfig) -> Result<(), Error> {
        let body = serde_json::json!({
            "ssid": encode_ssid(&cfg.ssid),
            "security": cfg.security,
            "passwd": cfg.passwd,
            "dhcp": if cfg.dhcp { "true" } else { "false" },
            "static_address": cfg.static_address,
            "gateway": cfg.gateway,
            "network_mask": cfg.network_mask,
            "dns1": cfg.dns1,
            "dns2": cfg.dns2,
            "proxy": if cfg.proxy { "true" } else { "false" },
        });
        self.put_ok("/system/controls/wifi_accesspoints/register", &body)
            .await
    }

    /// Removes a stored network
    /// (`DELETE /system/configs/wifi_accesspoints/{ssid}/{security}`).
    /// Here the SSID travels as the plain, URL-encoded network name — not
    /// Base64 (protocol §7.6).
    pub async fn delete_access_point(&self, ssid: &str, security: &str) -> Result<(), Error> {
        self.delete_ok(&format!(
            "/system/configs/wifi_accesspoints/{}/{}",
            form_encode(ssid),
            form_encode(security)
        ))
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssid_roundtrip() {
        assert_eq!(encode_ssid("MyNet"), "TXlOZXQ=");
        assert_eq!(decode_ssid("TXlOZXQ="), "MyNet");
        // Non-ASCII SSIDs survive the roundtrip.
        assert_eq!(decode_ssid(&encode_ssid("Café-Netz")), "Café-Netz");
        // Defensive: not Base64 → passed through unchanged.
        assert_eq!(decode_ssid("not/valid base64!"), "not/valid base64!");
    }

    #[test]
    fn access_point_list_decodes_wire_ssid() {
        let json = r#"{ "aplist": [
            { "ssid": "TXlOZXQ=", "security": "psk", "dhcp": "true" }
        ]}"#;
        let list: ApList = serde_json::from_str(json).unwrap();
        let ap: AccessPoint = list.aplist.into_iter().next().unwrap().into();
        assert_eq!(ap.ssid, "MyNet");
        assert_eq!(ap.security, "psk");
        assert_eq!(ap.extra.get("dhcp").unwrap(), "true");
    }

    #[test]
    fn register_body_is_all_strings() {
        let cfg = WifiNetworkConfig {
            ssid: "MyNet".into(),
            security: "psk".into(),
            passwd: "secret".into(),
            dhcp: true,
            static_address: String::new(),
            gateway: String::new(),
            network_mask: String::new(),
            dns1: String::new(),
            dns2: String::new(),
            proxy: false,
        };
        let body = serde_json::json!({
            "ssid": encode_ssid(&cfg.ssid),
            "dhcp": if cfg.dhcp { "true" } else { "false" },
            "proxy": if cfg.proxy { "true" } else { "false" },
        });
        assert_eq!(body["ssid"], "TXlOZXQ=");
        assert_eq!(body["dhcp"], "true");
        assert_eq!(body["proxy"], "false");
    }
}
