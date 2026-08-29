//! # dpt-core
//!
//! Protocol client and sync engine for Sony Digital Paper devices
//! (DPT-RP1 / DPT-CP1 and compatible devices such as the Fujitsu Quaderno).
//!
//! This crate is UI-agnostic (no Tauri dependency) and implements the
//! reverse-engineered device protocol specified in
//! `docs/sony-digital-paper-protocol.md`, plus the client-side sync engine
//! specified in `docs/06-sync-specification.md`.
//!
//! Module map (see `docs/04-architecture.md` §4):
//!
//! | Module | Responsibility |
//! |---|---|
//! | [`discovery`] | mDNS browsing and manual address probing (protocol §3) |
//! | [`usb`] | USB CDC ACM detection and Ethernet-over-USB mode switch (protocol §2) |
//! | [`register`] | One-time pairing handshake (protocol §4) |
//! | [`auth`] | Session authentication via nonce signing (protocol §5) |
//! | [`client`] | Authenticated HTTPS client with certificate pinning |
//! | [`api`] | Typed wrappers for all REST endpoints (protocol §7) |
//! | [`model`] | Shared data types (entries, device info, credentials) |
//! | [`sync`] | Snapshot → plan → apply sync engine (docs/06) |

pub mod api;
pub mod auth;
pub mod client;
pub mod discovery;
pub mod error;
pub mod model;
pub mod register;
pub mod sync;
pub mod usb;

pub use error::Error;

/// Re-exported so downstream crates (dpt-app) use the exact same reqwest
/// version for streaming response bodies.
pub use reqwest;

/// Crate-wide result type.
pub type Result<T> = std::result::Result<T, Error>;
