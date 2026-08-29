# Development environment for Digital Paper Companion (NixOS / Nix).
#
# Usage:  nix-shell          (from the repository root)
# Then:   npm install && npm run dev
#
# Provides the Rust toolchain, Node.js/npm and all system libraries
# required by Tauri 2 on Linux (WebKitGTK 4.1, GTK 3, libsoup 3, ...).
{ pkgs ? import <nixpkgs> { } }:

pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    # Rust toolchain
    rustc
    cargo
    rustfmt
    clippy
    rust-analyzer

    # Frontend toolchain
    nodejs_22

    # Build helpers
    pkg-config
    gobject-introspection
  ];

  buildInputs = with pkgs; [
    # Tauri 2 Linux runtime dependencies
    at-spi2-atk
    atkmm
    cairo
    gdk-pixbuf
    glib
    gtk3
    harfbuzz
    librsvg
    libsoup_3
    pango
    webkitgtk_4_1
    openssl

    # For the future USB connection feature (serialport crate)
    systemdLibs # provides libudev
  ];

  shellHook = ''
    # Works around a WebKitGTK DMA-BUF rendering issue on some
    # GPU/driver combinations (blank Tauri window).
    export WEBKIT_DISABLE_DMABUF_RENDERER=1
    echo "Digital Paper Companion dev shell — rustc $(rustc --version | cut -d' ' -f2), node $(node --version)"
  '';
}
