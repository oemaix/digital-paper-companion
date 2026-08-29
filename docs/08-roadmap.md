# 08 — Roadmap

Phases build on each other; each ends with a testable, releasable increment.
Requirement IDs refer to [02-functional-requirements.md](02-functional-requirements.md).

## Phase 0 — Foundation (`dpt-core` bring-up)

**Goal:** talk to a real device from Rust; no UI.

- Workspace scaffolding, CI skeleton (lint + test on 3 OSes).
- Registration handshake with test vectors (FR-REG-1, NFR-QLT-2).
- Session auth, pinned-TLS client, `ping` (FR-REG-4, NFR-SEC-2).
- Entry listing incl. 1300-entry fallback; download/upload streaming
  (FR-BRW-1 core, FR-TRF-10 core).
- Fake device server for integration tests.

**Exit:** a Rust example binary can pair, list and round-trip a PDF against
real hardware and against the fake device in CI.

## Phase 1 — MVP release (v0.1)

**Goal:** replace the Sony app for daily manual use.

- Tauri shell, design system foundations, sidebar/status-bar layout.
- Pairing wizard with mDNS discovery + manual address (FR-CONN-1/2,
  FR-REG-2/3), credential storage in keychain (NFR-SEC-1).
- Library browser: tree, list view, search, sort, context actions,
  preview via OS viewer (FR-BRW-1/2/3/5).
- Transfers with queue UI, drag & drop upload, folder download/upload,
  overwrite dialog (FR-TRF-1…5, 7, 9, 10).
- Device page: status cards, set clock (FR-SET-1/3).
- Auto-reconnect + session refresh (FR-CONN-5/6/8, FR-APP-1).

**Exit:** success criterion 1 of the product overview (first PDF in
< 5 minutes) verified with test users on all three OSes.

## Phase 2 — Sync release (v0.2)

**Goal:** the headline feature; trust-critical.

- Sync engine per docs/06 with full scenario test suite (FR-SYN-1/2/6/9,
  NFR-QLT-3).
- Sync UI: pair editor, preview/dry-run, run view, history log
  (FR-SYN-3/5/7).
- Scheduler: on-connect and interval triggers (FR-SYN-4); tray with
  "Sync now" (FR-APP-2).
- Filters (FR-SYN-8).

**Exit:** 1 000-document two-way sync soak test (repeated runs with random
mutations on both sides) shows zero data loss and correct conflict copies.

## Phase 3 — Completeness (v0.3)

- Notes view and Templates management (FR-BRW-4/7, FR-TRF-6).
- Device settings editor incl. advanced key/value table (FR-SET-2),
  Wi-Fi management (FR-SET-4), screenshots (FR-SET-5),
  "Open on device" (FR-BRW-6).
- USB connection with mode switch (FR-CONN-4); Bluetooth PAN documentation
  and address preset (FR-CONN-3 polish).
- Multi-device registry and switching (FR-CONN-7), credential import from
  Sony app / dptrp1 (FR-REG-6), "Forget device" (FR-REG-5).
- German localization (NFR-I18N-1); accessibility pass (NFR-UX-4).

## Phase 4 — Polish and 1.0

- Code signing + notarization pipeline (NFR-PLT-4); update check
  (FR-APP-5).
- Performance hardening against NFR-PRF targets with large libraries.
- Onboarding refinements from user feedback; empty-state and error-copy
  review.
- Documentation for end users (in-app help + website/README).

**Exit:** v1.0 — all P1 and P2 requirements shipped.

## Post-1.0 candidates (unscheduled)

- Template thumbnails; in-app PDF preview pane.
- CLI companion built on `dpt-core`; headless sync daemon mode.
- Fujitsu Quaderno-specific quirks/testing as hardware becomes available.
- Additional locales.

## Open questions

Tracked here until resolved; each resolution updates the affected document.

1. **Note detection heuristic** — confirm on hardware how notes are best
   distinguished (path under `Document/Note/` vs. `document_source` field).
   Affects FR-BRW-4. → verify in Phase 0.
2. **`file_revision` stability** — confirm the revision string changes on
   annotation-only edits and survives reboots. Affects 06 §4. → verify in
   Phase 0 with hardware.
3. **Windows RNDIS driver situation** on Windows 11 (RNDIS deprecation) —
   may require CDC/ECM path or documentation. Affects FR-CONN-4.
4. **App name and icon** — must avoid Sony trademarks (NFR-QLT-6); working
   title "Digital Paper Companion" to be legally sanity-checked before 1.0.
