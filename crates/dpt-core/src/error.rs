//! Error types for the crate (docs/04 §6, "Cross-cutting concerns").

use thiserror::Error;

/// Top-level error type of `dpt-core`.
///
/// Will be split into per-domain enums (`RegistrationError`, `ApiError`,
/// `SyncError`) as the corresponding modules are implemented.
#[derive(Debug, Error)]
pub enum Error {
    /// The device answered with a non-2xx status and (usually) a JSON body
    /// containing a human-readable `message` field (protocol §9).
    #[error("device API error (HTTP {status}): {message}")]
    Api { status: u16, message: String },

    /// Network-level failure (unreachable, timeout, TLS).
    #[error("network error: {0}")]
    Network(String),

    /// Local I/O failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Malformed data from the device or from a local state file.
    #[error("protocol/data error: {0}")]
    Protocol(String),
}
