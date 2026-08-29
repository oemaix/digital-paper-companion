//! Client-side sync engine (authoritative spec: docs/06-sync-specification.md).
//!
//! Pipeline: [`snapshot`] (three views: checkpoint / local / remote)
//! → [`plan`] (pure decision function → `Vec<Action>`)
//! → [`apply`] (execution with incremental [`checkpoint`] advancement).
//!
//! `plan` being pure is the testing linchpin (NFR-QLT-3): scenario tests
//! feed synthetic trees and assert on the produced actions.

pub mod apply;
pub mod checkpoint;
pub mod plan;
pub mod snapshot;
