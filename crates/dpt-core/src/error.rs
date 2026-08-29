//! Error types for the crate (docs/04 §6, "Cross-cutting concerns").

use thiserror::Error;

/// Top-level error type of `dpt-core`.
#[derive(Debug, Error)]
pub enum Error {
    /// The device answered with a non-2xx status and (usually) a JSON body
    /// containing a human-readable `message` field (protocol §9).
    #[error("device API error (HTTP {status}): {message}")]
    Api { status: u16, message: String },

    /// Network-level failure (unreachable, timeout, TLS).
    #[error("network error: {0}")]
    Network(String),

    /// The registration (pairing) handshake failed.
    #[error("registration failed: {0}")]
    Registration(String),

    /// Session authentication failed.
    #[error("authentication failed: {0}")]
    Auth(String),

    /// A cryptographic operation failed or a value failed verification.
    #[error("crypto error: {0}")]
    Crypto(String),

    /// TLS certificate pin mismatch — the device identity changed.
    #[error("device certificate does not match the pinned certificate")]
    CertPinMismatch,

    /// Local I/O failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Malformed data from the device or from a local state file.
    #[error("protocol/data error: {0}")]
    Protocol(String),
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Network(e.to_string())
    }
}
