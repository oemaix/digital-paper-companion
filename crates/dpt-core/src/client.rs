//! Authenticated HTTPS client for the main API on port 8443
//! (docs/04 §4.1; NFR-SEC-2, NFR-REL-4).
//!
//! - TLS with **certificate pinning** against the certificate obtained at
//!   registration (custom `rustls` verifier, byte-equal DER comparison);
//!   never plain "verification off".
//! - Session cookie management with transparent one-shot re-authentication
//!   on an auth failure.
//! - Timeouts and a probe/registration entry point.

use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use tokio::sync::RwLock;

use crate::auth;
use crate::model::{Credentials, DeviceAddr, DeviceInfo, Registration};
use crate::register::RegistrationClient;
use crate::Error;

/// An authenticated connection to one device.
pub struct DeviceClient {
    http: Client,
    api_base: String,
    credentials: Credentials,
    cookie: RwLock<String>,
}

impl DeviceClient {
    /// Probes an address without authentication (protocol §7.1): returns the
    /// device information from the registration server on port 8080.
    pub async fn probe(addr: &DeviceAddr) -> Result<DeviceInfo, Error> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build()?;
        let url = format!("{}/register/information", addr.registration_base());
        let resp = http.get(url).send().await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(Error::Api {
                status: status.as_u16(),
                message: "probe failed".into(),
            });
        }
        serde_json::from_str(&text).map_err(|e| Error::Protocol(format!("bad device info: {e}")))
    }

    /// Begins the one-time pairing handshake (protocol §4). See
    /// [`RegistrationClient`] for the two-phase (PIN) flow.
    pub fn register(addr: &DeviceAddr) -> Result<RegistrationClient, Error> {
        RegistrationClient::new(addr)
    }

    /// Opens an authenticated session against the API server (port 8443),
    /// pinning the given device certificate (PEM from registration, M5).
    pub async fn connect(
        addr: &DeviceAddr,
        credentials: Credentials,
        device_cert_pem: &str,
    ) -> Result<Self, Error> {
        let tls = pinned_tls_config(device_cert_pem)?;
        let http = Client::builder()
            .use_preconfigured_tls(tls)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()?;
        let client = Self {
            http,
            api_base: addr.api_base(),
            credentials,
            cookie: RwLock::new(String::new()),
        };
        client.authenticate().await?;
        Ok(client)
    }

    /// Performs the nonce-signature exchange and stores the session cookie
    /// (protocol §5).
    pub async fn authenticate(&self) -> Result<(), Error> {
        let nonce_url = format!(
            "{}/auth/nonce/{}",
            self.api_base, self.credentials.client_id
        );
        let resp = self.http.get(nonce_url).send().await?;
        if !resp.status().is_success() {
            return Err(Error::Auth(format!(
                "nonce request failed (HTTP {})",
                resp.status().as_u16()
            )));
        }
        #[derive(serde::Deserialize)]
        struct NonceResp {
            nonce: String,
        }
        let nonce: NonceResp = resp
            .json()
            .await
            .map_err(|e| Error::Auth(format!("bad nonce response: {e}")))?;

        let signature = auth::sign_nonce(&self.credentials.private_key_pem, &nonce.nonce)?;

        let resp = self
            .http
            .put(format!("{}/auth", self.api_base))
            .json(&serde_json::json!({
                "client_id": self.credentials.client_id,
                "nonce_signed": signature,
            }))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(Error::Auth(format!(
                "auth exchange failed (HTTP {})",
                resp.status().as_u16()
            )));
        }
        let cookie = resp
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find_map(auth::extract_credentials_cookie)
            .ok_or_else(|| Error::Auth("no Credentials cookie in response".into()))?;

        *self.cookie.write().await = cookie;
        Ok(())
    }

    /// `GET /ping` — succeeds when the session is valid (protocol §5).
    pub async fn ping(&self) -> Result<(), Error> {
        let resp = self
            .send(|http, base| http.get(format!("{base}/ping")))
            .await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(Error::Auth("ping failed".into()))
        }
    }

    // ---- request plumbing ------------------------------------------------

    /// Sends a request built by `make`, attaching the session cookie and
    /// re-authenticating once on an auth failure. `make` must be replayable
    /// (it is called again on retry), so use it only for requests whose body
    /// can be rebuilt (GET/JSON/DELETE).
    pub(crate) async fn send(
        &self,
        make: impl Fn(&Client, &str) -> RequestBuilder,
    ) -> Result<Response, Error> {
        if self.cookie.read().await.is_empty() {
            self.authenticate().await?;
        }
        let cookie = self.cookie.read().await.clone();
        let resp = make(&self.http, &self.api_base)
            .header(reqwest::header::COOKIE, format!("Credentials={cookie}"))
            .send()
            .await?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            self.authenticate().await?;
            let cookie = self.cookie.read().await.clone();
            let resp = make(&self.http, &self.api_base)
                .header(reqwest::header::COOKIE, format!("Credentials={cookie}"))
                .send()
                .await?;
            return Ok(resp);
        }
        Ok(resp)
    }

    /// Like [`Self::send`] but returns the session cookie for a one-shot,
    /// non-replayable request (streaming upload). Ensures authentication
    /// first; the caller sends the request itself.
    pub(crate) async fn cookie_header(&self) -> Result<String, Error> {
        if self.cookie.read().await.is_empty() {
            self.authenticate().await?;
        }
        Ok(format!("Credentials={}", self.cookie.read().await))
    }

    pub(crate) fn http(&self) -> &Client {
        &self.http
    }

    pub(crate) fn api_base(&self) -> &str {
        &self.api_base
    }

    /// GET a JSON body from `path`.
    pub(crate) async fn get_json<R: DeserializeOwned>(&self, path: &str) -> Result<R, Error> {
        let p = path.to_string();
        let resp = self
            .send(move |http, base| http.get(format!("{base}{p}")))
            .await?;
        json_or_error(resp).await
    }

    /// PUT a JSON body, expecting only a success status (no body).
    pub(crate) async fn put_ok<B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<(), Error> {
        let p = path.to_string();
        let body = serde_json::to_value(body).map_err(|e| Error::Protocol(e.to_string()))?;
        let resp = self
            .send(move |http, base| http.put(format!("{base}{p}")).json(&body))
            .await?;
        ok_or_error(resp).await
    }

    /// POST a JSON body to `path`, expecting a JSON response.
    pub(crate) async fn post_json<B, R>(&self, path: &str, body: &B) -> Result<R, Error>
    where
        B: serde::Serialize,
        R: DeserializeOwned,
    {
        let p = path.to_string();
        let body = serde_json::to_value(body).map_err(|e| Error::Protocol(e.to_string()))?;
        let resp = self
            .send(move |http, base| http.post(format!("{base}{p}")).json(&body))
            .await?;
        json_or_error(resp).await
    }

    /// DELETE `path`, expecting a success status.
    pub(crate) async fn delete_ok(&self, path: &str) -> Result<(), Error> {
        let p = path.to_string();
        let resp = self
            .send(move |http, base| http.delete(format!("{base}{p}")))
            .await?;
        ok_or_error(resp).await
    }

    /// Returns the registration/credentials in use (client id only is safe
    /// to surface; used by callers that need the id).
    pub fn client_id(&self) -> &str {
        &self.credentials.client_id
    }
}

/// Convenience: register interactively then connect. Not used by the app
/// (which drives the two phases across IPC), kept for library ergonomics.
pub async fn register_and_connect(
    addr: &DeviceAddr,
    pin: &str,
) -> Result<(Registration, DeviceClient), Error> {
    let pending = RegistrationClient::new(addr)?.begin().await?;
    let reg = pending.submit_pin(pin).await?;
    let client = DeviceClient::connect(addr, reg.credentials.clone(), &reg.device_cert_pem).await?;
    Ok((reg, client))
}

async fn json_or_error<R: DeserializeOwned>(resp: Response) -> Result<R, Error> {
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(Error::Api {
            status: status.as_u16(),
            message: device_message(&text),
        });
    }
    serde_json::from_str(&text).map_err(|e| Error::Protocol(format!("malformed response: {e}")))
}

async fn ok_or_error(resp: Response) -> Result<(), Error> {
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let text = resp.text().await.unwrap_or_default();
    Err(Error::Api {
        status: status.as_u16(),
        message: device_message(&text),
    })
}

fn device_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
        .unwrap_or_else(|| {
            let b = body.trim();
            if b.is_empty() {
                "no message".into()
            } else {
                b.to_string()
            }
        })
}

/// Builds a rustls client config that trusts *only* the given device
/// certificate (byte-equal DER pin) and skips hostname verification, since
/// the device's cert is not issued for its rotating IP (docs/07 §3).
fn pinned_tls_config(device_cert_pem: &str) -> Result<rustls::ClientConfig, Error> {
    let mut reader = std::io::BufReader::new(device_cert_pem.as_bytes());
    let cert = rustls_pemfile::certs(&mut reader)
        .next()
        .ok_or_else(|| Error::Crypto("no certificate in pinned PEM".into()))?
        .map_err(|e| Error::Crypto(format!("cannot parse pinned certificate: {e}")))?;
    let pinned_der = cert.as_ref().to_vec();

    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let supported = provider.signature_verification_algorithms;
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Crypto(format!("rustls setup failed: {e}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(pin::PinnedVerifier {
            pinned_der,
            supported,
        }))
        .with_no_client_auth();
    Ok(config)
}

mod pin {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::crypto::{
        verify_tls12_signature, verify_tls13_signature, WebPkiSupportedAlgorithms,
    };
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{DigitallySignedStruct, Error as TlsError, SignatureScheme};

    /// A verifier that accepts exactly one pinned leaf certificate.
    #[derive(Debug)]
    pub struct PinnedVerifier {
        pub pinned_der: Vec<u8>,
        pub supported: WebPkiSupportedAlgorithms,
    }

    impl ServerCertVerifier for PinnedVerifier {
        fn verify_server_cert(
            &self,
            end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, TlsError> {
            if end_entity.as_ref() == self.pinned_der.as_slice() {
                Ok(ServerCertVerified::assertion())
            } else {
                Err(TlsError::General(
                    "device certificate does not match pin".into(),
                ))
            }
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, TlsError> {
            verify_tls12_signature(message, cert, dss, &self.supported)
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, TlsError> {
            verify_tls13_signature(message, cert, dss, &self.supported)
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.supported.supported_schemes()
        }
    }
}
