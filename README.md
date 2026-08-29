# Digital Paper Companion

A free, cross-platform desktop companion for **Sony Digital Paper**
(DPT-RP1 / DPT-CP1) and compatible devices such as the Fujitsu Quaderno —
a replacement for Sony's discontinued official Digital Paper App.

Built with **Rust** and **Tauri 2**; runs on Windows, macOS and Linux.

> Status: early development. See [docs/08-roadmap.md](docs/08-roadmap.md)
> for the phase plan; the full product definition lives in [docs/](docs/).

## Features (planned)

- One-time PIN pairing and automatic reconnection (Wi-Fi, Bluetooth, USB)
- Browse, upload, download and organize documents, notes and templates
- Device settings: clock, owner, standby, Wi-Fi networks, screenshots
- Two-way folder sync with scheduling, conflict safety and dry-run preview

## Development

### Prerequisites

- **NixOS / Nix users:** everything is provided by the dev shell —
  `nix-shell` in the repository root.
- **Other Linux:** Rust (stable), Node.js ≥ 22, plus the
  [Tauri 2 Linux prerequisites](https://tauri.app/start/prerequisites/)
  (WebKitGTK 4.1, GTK 3, libsoup 3, OpenSSL, pkg-config).
- **Windows / macOS:** Rust (stable), Node.js ≥ 22 and the platform
  prerequisites from the Tauri docs.

### Getting started

```bash
nix-shell              # NixOS/Nix only; provides rustc, cargo, node, npm
npm install            # frontend dependencies
npm run dev            # full app: vite dev server + tauri window
```

Useful scripts (see `package.json`):

| Script | Purpose |
|---|---|
| `npm run dev` | Run the desktop app in development mode (hot reload) |
| `npm run build` | Build release bundles (`.deb`/`.rpm`/AppImage etc.) |
| `npm run dev:web` | Frontend only, in a browser (no Tauri APIs) |
| `npm run typecheck` / `lint` / `format` | Frontend checks |
| `npm run check:rust` | `cargo fmt --check`, `clippy`, `cargo test` |
| `npm run check` | Everything above |

### Repository layout

- `crates/dpt-core` — protocol client + sync engine (pure Rust, no Tauri);
  see [docs/04-architecture.md](docs/04-architecture.md)
- `src-tauri` — Tauri application layer (crate `dpt-app`)
- `src` — React/TypeScript frontend
- `docs` — product definition and the
  [device protocol specification](docs/sony-digital-paper-protocol.md)

### Notes

- The app icon is a placeholder. Replace it by running
  `npm run tauri icon <path-to-1024px-png>`.
- Editor: Cursor/VS Code users get extension recommendations from
  `.vscode/extensions.json` (rust-analyzer, Tauri, Tailwind, ESLint, …).

## License

MIT OR Apache-2.0 (final choice pending, see NFR-QLT-6). This project is
not affiliated with or endorsed by Sony.
