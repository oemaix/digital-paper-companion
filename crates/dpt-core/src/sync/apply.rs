//! Apply phase (docs/06 §6): execute planned actions.
//!
//! Order: create dirs → transfers (concurrency 2) → file deletions →
//! folder deletions (deepest first). Downloads via `*.part` + atomic
//! rename (NFR-REL-2); checkpoint advanced per completed action so an
//! interrupted run resumes cleanly (FR-SYN-9).

// TODO: apply(actions, client, fs, progress_sink, cancel_token) -> RunReport.
