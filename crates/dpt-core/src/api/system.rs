//! System configuration, status and screenshots
//! (protocol §7.7–§7.9; FR-SET-1/2/3/5).

use crate::client::DeviceClient;
use crate::model::{BatteryStatus, DeviceInfo, StorageStatus};
use crate::Error;

#[derive(serde::Deserialize)]
struct Value {
    value: String,
}

impl DeviceClient {
    /// Device information incl. serial number (protocol §7.1/§7.8).
    pub async fn device_info(&self) -> Result<DeviceInfo, Error> {
        self.get_json("/register/information").await
    }

    /// Storage capacity/availability in bytes (protocol §7.8).
    pub async fn storage(&self) -> Result<StorageStatus, Error> {
        self.get_json("/system/status/storage").await
    }

    /// Battery level and charging state (protocol §7.8).
    pub async fn battery(&self) -> Result<BatteryStatus, Error> {
        self.get_json("/system/status/battery").await
    }

    /// Firmware version string (protocol §7.8).
    pub async fn firmware_version(&self) -> Result<String, Error> {
        let v: Value = self.get_json("/system/status/firmware_version").await?;
        Ok(v.value)
    }

    /// Device MAC address (protocol §7.8).
    pub async fn mac_address(&self) -> Result<String, Error> {
        let v: Value = self.get_json("/system/status/mac_address").await?;
        Ok(v.value)
    }

    /// All configuration values as a raw JSON object (protocol §7.7). A
    /// generic client can round-trip these; known keys are surfaced by the
    /// UI and the rest shown in an advanced editor (FR-SET-2).
    pub async fn configs(&self) -> Result<serde_json::Value, Error> {
        self.get_json("/system/configs/").await
    }

    /// Reads a single configuration value (protocol §7.7).
    pub async fn config(&self, key: &str) -> Result<String, Error> {
        let v: Value = self.get_json(&format!("/system/configs/{key}")).await?;
        Ok(v.value)
    }

    /// Writes a single configuration value (protocol §7.7).
    pub async fn set_config(&self, key: &str, value: &str) -> Result<(), Error> {
        self.put_ok(
            &format!("/system/configs/{key}"),
            &serde_json::json!({ "value": value }),
        )
        .await
    }

    /// Sets the device clock to the current UTC time (FR-SET-3). Recommended
    /// before comparing `modified_date` values during sync (protocol §10.12).
    pub async fn set_clock_now(&self) -> Result<(), Error> {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.set_config("datetime", &now).await
    }

    /// Captures the current screen as PNG bytes (protocol §7.9; FR-SET-5).
    pub async fn screenshot_png(&self) -> Result<Vec<u8>, Error> {
        let resp = self
            .send(|http, base| http.get(format!("{base}/system/controls/screen_shot")))
            .await?;
        if !resp.status().is_success() {
            return Err(Error::Api {
                status: resp.status().as_u16(),
                message: "screenshot failed".into(),
            });
        }
        Ok(resp.bytes().await?.to_vec())
    }
}
