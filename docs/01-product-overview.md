# 01 — Product Overview

## 1. Background

Sony's Digital Paper devices (DPT-RP1, 13.3″ and DPT-CP1, 10.3″) are e-ink
PDF readers and notepads. They are managed from a desktop computer through a
companion application: documents are uploaded and downloaded, folders are
organized, note templates are installed, and device settings are changed from
the desktop.

Sony discontinued the product line and removed the official **Digital Paper
App** from its websites. Existing devices still work perfectly, but new users
(and users setting up a new computer) have no supported way to manage them.

The device's network protocol has been thoroughly reverse-engineered by the
community and is documented in
[sony-digital-paper-protocol.md](sony-digital-paper-protocol.md). It is a
conventional HTTPS/JSON API with a one-time cryptographic pairing step. This
makes a full-featured, unofficial replacement client feasible.

**Digital Paper Companion** is that replacement: a modern, open,
cross-platform desktop application that covers the day-to-day management of a
Digital Paper device.

## 2. Product vision

> The tool a Digital Paper owner opens every day: plug in or connect the
> device, see your documents, drag files in and out, and let a background
> sync keep a folder on your computer and the device identical — with a UI
> that feels native, clean and current on Windows, macOS and Linux.

Design values, in priority order:

1. **Trustworthy** — never lose or silently overwrite a user's documents or
   notes. Sync behavior is predictable and inspectable.
2. **Simple** — first-run pairing in under two minutes; core actions
   (upload, download, sync) reachable in one or two clicks.
3. **Modern** — clean visual design, fast, responsive, keyboard-friendly,
   light and dark themes.
4. **Self-contained** — a single installable app with no external runtime
   dependencies.

## 3. Target users

| Persona | Description | Primary needs |
|---|---|---|
| **The academic** | Reads and annotates papers on the DPT-RP1 daily | Fast upload of new PDFs, reliable download of annotated versions, folder sync with a "Papers" directory |
| **The note-taker** | Uses the device mainly as a notepad | Downloading notes to the computer, managing note templates, browsing notes by date |
| **The new owner** | Bought a second-hand device; never had the Sony app | Guided first-time setup: Wi-Fi, pairing, discovering what the device can do |
| **The archivist** | Wants a complete, current backup of the device | Scheduled one-way or two-way sync, confidence that the local copy is complete |

All personas are desktop users on Windows, macOS or Linux. No mobile client
is planned.

## 4. Product scope

### 4.1 In scope (v1)

1. **Device connection** — discover devices via mDNS, connect via Wi-Fi,
   Bluetooth PAN or USB (Ethernet-over-USB), and perform the one-time
   PIN-based registration (pairing).
2. **Device settings** — view device information (model, serial, firmware,
   storage, battery) and read/write device configuration (owner name, date &
   time, timezone, standby timeout, Wi-Fi networks, …).
3. **Content browsing** — list and navigate the device's folder tree;
   distinguish documents, notes, folders and note templates; view metadata
   and open a local preview of any document.
4. **Transfer** — upload and download documents, notes, whole folders and
   note templates, including drag & drop in both directions; create, rename,
   move, copy and delete entries on the device.
5. **Sync** — associate a local folder with the device (whole device or a
   subtree), configure direction and schedule, and run sync manually or
   automatically. Conflict handling with a safe default.
6. **Quality of life** — device screenshot capture, "open this document on
   the device" action, session persistence across app restarts.

### 4.2 Out of scope (v1)

- Editing or annotating PDFs inside the app (the OS default PDF viewer is
  used for previews).
- Firmware updates. Sony has discontinued the device and ended support;
  this companion will not upload or trigger firmware packages.
- Cloud storage integrations (Dropbox, Google Drive, …).
- Managing more than one device *simultaneously*; multiple devices may be
  registered and switched between, but only one active connection at a time.
- Mobile or web versions.

### 4.3 Non-goals

- Reimplementing Sony's UI pixel-by-pixel. We build a better, contemporary
  UX on the same protocol.
- Supporting non-PDF file conversion. The device stores PDFs only; the app
  MAY warn and refuse non-PDF uploads but does not convert.

## 5. Technology decision

The application is built with **Rust** and **Tauri 2**:

- The entire protocol layer (crypto handshake, HTTPS client, sync engine)
  is native Rust — fast, memory-safe, and independently testable as a
  library crate.
- Tauri provides small, native-feeling installers for Windows (`.msi`),
  macOS (`.dmg`, universal binary) and Linux (`.deb`, `.rpm`, AppImage)
  from one codebase.
- The UI is a web frontend rendered in the system webview (see
  [04-architecture.md](04-architecture.md) for the frontend stack choice).

## 6. Success criteria

- A user with a factory-reset device and no prior Sony software can pair and
  upload their first PDF in **< 5 minutes** using only in-app guidance.
- Two-way sync of a 1 000-document library completes incrementally in
  seconds when nothing changed, and never produces silent data loss
  (verified by the sync test suite, see 06).
- The app runs on current Windows 10/11, macOS 12+, and mainstream Linux
  distributions without additional runtimes.

## 7. Glossary

| Term | Meaning |
|---|---|
| **DPT / device** | Sony DPT-RP1 or DPT-CP1 (or Fujitsu Quaderno) |
| **Entry** | Any item in the device storage tree: a document or a folder |
| **Document** | A PDF stored on the device (includes notes; see below) |
| **Note** | A document created on the device from a note template; lives under the device's `Document/Note/` folder by convention |
| **Note template** | A background PDF used when creating new notes; managed separately from documents |
| **Registration / pairing** | The one-time PIN-confirmed key exchange that authorizes this app on a device (protocol §4) |
| **Client credentials** | The client ID (UUID) and RSA-2048 private key produced by pairing; stored locally, secret |
| **Session** | An authenticated period on the device API, established by the nonce-signing exchange (protocol §5) |
| **Sync pair** | A configured (local folder ⇄ device subtree) relationship |
| **Checkpoint** | The saved snapshot of the last successful sync state, used to detect changes on both sides |
