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

Two interchangeable dev environments are provided; pick one.

### Option A: Docker (recommended)

Requires Docker with the Compose plugin. The image contains Rust, Node 22
and all Tauri Linux dependencies on a Debian base — identical for every
contributor and to CI. `target/` and `node_modules/` live in named volumes,
so container builds never collide with host builds.

```bash
docker compose build                       # once (and after Dockerfile changes)
docker compose run --rm dev npm install    # once
docker compose run --rm dev                # interactive shell
docker compose run --rm dev npm run check  # lint + typecheck + tests
docker compose run --rm dev npm run build  # release bundles
```

Running the GUI (`npm run dev`) from inside the container works on an X11
host after allowing local connections once (`xhost +local:`); on Wayland,
prefer running the GUI via Option B and use the container for
building/testing. Cursor/VS Code users can instead open the repo as a
**Dev Container** (configuration in `.devcontainer/`), which uses the same
compose service.

Note on permissions: the compose service runs as container root by default,
which is correct for **rootless Docker** (container root = your host user).
On classic rootful Docker, run as the unprivileged user instead:
`DPC_CONTAINER_USER=dev docker compose run --rm dev`.

### Option B: Nix shell (NixOS hosts)

```bash
nix-shell              # provides rustc, cargo, node, npm + Tauri libs
npm install            # frontend dependencies
npm run dev            # full app: vite dev server + tauri window
```

### Manual setup

Rust (stable), Node.js ≥ 22, plus the
[Tauri 2 prerequisites](https://tauri.app/start/prerequisites/) for your
platform (on Linux: WebKitGTK 4.1, GTK 3, libsoup 3, OpenSSL, pkg-config).

Useful scripts (see `package.json`):

| Script | Purpose |
|---|---|
| `npm run dev` | Run the desktop app in development mode (hot reload) |
| `npm run build` | Build release bundles for **this** OS (`.deb`/`.rpm`/AppImage on Linux) |

Installers for Windows (`.msi`/`.exe`) and macOS (`.dmg`) are produced by
[`.github/workflows/release.yml`](.github/workflows/release.yml) — Tauri
cannot be cross-compiled from one OS. After the repo is on GitHub, run
**Actions → Release → Run workflow**, or push a `v*` tag.
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

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under either of

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT license](LICENSE-MIT)

at your option. This project is not affiliated with or endorsed by Sony.
