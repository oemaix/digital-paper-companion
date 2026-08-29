//! User-presentable error type for IPC commands (FR-APP-3).
//!
//! Raw protocol/HTTP detail stays in the logs; the frontend receives a
//! stable `code` and a short human-readable `message`.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

impl From<dpt_core::Error> for AppError {
    fn from(e: dpt_core::Error) -> Self {
        use dpt_core::Error as E;
        let code = match &e {
            E::Api { .. } => "device_api",
            E::Network(_) => "network",
            E::Registration(_) => "registration",
            E::Auth(_) => "auth",
            E::Crypto(_) => "crypto",
            E::CertPinMismatch => "cert_pin_mismatch",
            E::Io(_) => "io",
            E::Protocol(_) => "protocol",
            E::Sync(_) => "sync",
        };
        tracing::debug!(error = %e, "dpt-core error surfaced to UI");
        AppError::new(code, e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::new("io", e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::new("serde", e.to_string())
    }
}

pub type CmdResult<T> = std::result::Result<T, AppError>;
