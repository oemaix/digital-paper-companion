//! One-time registration (pairing) handshake (protocol §4; FR-REG-1/2/3).
//!
//! Six-message authenticated key exchange over plain HTTP on port 8080:
//! Diffie-Hellman (RFC 3526 group 14) → PBKDF2 key derivation → PIN-based
//! mutual authentication → HMAC-chained messages → AES-128-CBC key wrap.
//!
//! The message/crypto logic in [`flow`] is a pure state machine so it can be
//! unit-tested against fixtures without network I/O (NFR-QLT-2). This module
//! adds the HTTP transport, split into two phases to match the interactive
//! PIN entry across the IPC boundary (FR-REG-2):
//!
//! 1. [`RegistrationClient::begin`] performs cleanup → M1 → M2 → M3 and
//!    returns a [`PendingPin`]; at this point the device shows a PIN.
//! 2. [`PendingPin::submit_pin`] performs M4 → M5 → M6 → commit → cleanup
//!    and returns the [`Registration`] to persist.

pub mod dh;
pub mod flow;
pub mod kdf;
pub mod wrap;

use std::time::Duration;

use reqwest::Client;

use crate::model::{DeviceAddr, Registration};
use crate::Error;

use flow::{AwaitingPin, M1, M3, M5};

/// Drives the registration handshake over HTTP against one device.
pub struct RegistrationClient {
    http: Client,
    base: String,
}

impl RegistrationClient {
    pub fn new(addr: &DeviceAddr) -> Result<Self, Error> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            base: addr.registration_base(),
        })
    }

    /// Phase 1: aborts any half-finished registration, requests a PIN (shown
    /// on the device), and completes the M1–M3 exchange. On success the
    /// device is displaying a PIN and a [`PendingPin`] is returned.
    pub async fn begin(self) -> Result<PendingPin, Error> {
        // Clean up any previous attempt (ignore failures).
        let _ = self
            .http
            .put(format!("{}/register/cleanup", self.base))
            .send()
            .await;

        // POST /register/pin -> M1 (device displays a PIN).
        let m1: M1 = self.post_json("/register/pin", &serde_json::json!({})).await?;
        let (started, m2) = flow::start(&m1)?;

        // POST /register/hash (M2) -> M3.
        let m3: M3 = self.post_json("/register/hash", &m2).await?;
        let awaiting = started.on_m3(&m3)?;

        Ok(PendingPin {
            http: self.http,
            base: self.base,
            awaiting,
        })
    }

    async fn post_json<B: serde::Serialize, R: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<R, Error> {
        let resp = self
            .http
            .post(format!("{}{}", self.base, path))
            .json(body)
            .send()
            .await?;
        parse_json(resp).await
    }
}

/// A registration in progress: the device is showing a PIN, awaiting the
/// user to type it into the app.
pub struct PendingPin {
    http: Client,
    base: String,
    awaiting: AwaitingPin,
}

impl PendingPin {
    /// Phase 2: completes the handshake with the PIN and commits the
    /// registration on the device. Returns the credentials + device
    /// certificate to persist (docs/07 §2).
    pub async fn submit_pin(self, pin: &str) -> Result<Registration, Error> {
        let (awaiting_m5, m4) = self.awaiting.build_m4(pin)?;

        // POST /register/ca (M4) -> M5.
        let m5: M5 = post_json(&self.http, &self.base, "/register/ca", &m4).await?;
        let (registration, m6) = awaiting_m5.on_m5(&m5)?;

        // POST /register (M6) commits the registration.
        let resp = self
            .http
            .post(format!("{}/register", self.base))
            .json(&m6)
            .send()
            .await?;
        ensure_success(resp).await?;

        // Best-effort cleanup.
        let _ = self
            .http
            .put(format!("{}/register/cleanup", self.base))
            .send()
            .await;

        Ok(registration)
    }
}

async fn post_json<B: serde::Serialize, R: serde::de::DeserializeOwned>(
    http: &Client,
    base: &str,
    path: &str,
    body: &B,
) -> Result<R, Error> {
    let resp = http.post(format!("{base}{path}")).json(body).send().await?;
    parse_json(resp).await
}

async fn parse_json<R: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<R, Error> {
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(Error::Registration(format!(
            "device rejected registration (HTTP {}): {}",
            status.as_u16(),
            device_message(&text)
        )));
    }
    serde_json::from_str(&text)
        .map_err(|e| Error::Registration(format!("malformed device message: {e}")))
}

async fn ensure_success(resp: reqwest::Response) -> Result<(), Error> {
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let text = resp.text().await.unwrap_or_default();
    Err(Error::Registration(format!(
        "device rejected registration (HTTP {}): {}",
        status.as_u16(),
        device_message(&text)
    )))
}

/// Extracts the device's `message` field from an error body, if present.
fn device_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
        .unwrap_or_else(|| body.trim().to_string())
}
