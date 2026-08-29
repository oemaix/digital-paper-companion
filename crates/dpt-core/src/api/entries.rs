//! Documents and folders (protocol §7.3; FR-BRW-1, FR-TRF-*).
//!
//! Key behaviors to implement:
//! - `list_all_entries()` with the ~1300-entry truncation fallback to
//!   recursive per-folder listing (protocol §7.3.2/§7.3.3, §10.8).
//! - Streaming download/upload (NFR-PRF-3); upload = create entry + put
//!   content, deleting the created entry if the content upload fails
//!   (FR-TRF-10).
//! - Path resolution with form-encoded paths (protocol §6.3).

// TODO: list_all_entries, resolve_path, download_document, upload_document,
// create_folder, delete_entry, move_entry, copy_entry.
