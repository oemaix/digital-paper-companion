//! Typed wrappers for the device REST API (protocol §7; docs/04 §4.2).
//!
//! One module per endpoint domain. All wrappers convert the device's
//! string-typed JSON scalars into proper Rust types at the serde boundary.

pub mod entries;
pub mod system;
pub mod templates;
pub mod viewer;
pub mod wifi;
