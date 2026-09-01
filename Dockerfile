# Development / build image for Digital Paper Companion.
#
# Provides the Rust toolchain, Node.js 22 and all Tauri 2 Linux system
# dependencies on a Debian base, so builds behave like a mainstream distro
# (and like CI) regardless of the host OS.
#
# Usage: see README.md ("Development with Docker") or use the dev container
# configuration in .devcontainer/.

FROM rust:1-bookworm

# Tauri 2 Linux prerequisites (https://tauri.app/start/prerequisites/)
# plus general build tooling.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        file \
        git \
        libayatana-appindicator3-dev \
        librsvg2-dev \
        libssl-dev \
        libudev-dev \
        libwebkit2gtk-4.1-dev \
        libxdo-dev \
        pkg-config \
        wget \
        xdg-utils \
        patchelf \
        file \
        dpkg \
        fakeroot \
        rpm \
    && rm -rf /var/lib/apt/lists/*

# Node.js 22 (NodeSource)
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# GitHub CLI (release/tag management from inside the container)
RUN curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
        -o /usr/share/keyrings/githubcli-archive-keyring.gpg \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
        > /etc/apt/sources.list.d/github-cli.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends gh \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt clippy

# Non-root user; UID/GID default to 1000 to match typical Linux hosts so
# bind-mounted files keep sane ownership. Override via build args if needed.
ARG UID=1000
ARG GID=1000
RUN groupadd -g "${GID}" dev \
    && useradd -m -u "${UID}" -g "${GID}" -s /bin/bash dev \
    && chown -R dev:dev /usr/local/cargo \
    # Pre-create the named-volume mount points with correct ownership so
    # Docker initializes the volumes as user-writable (compose mounts
    # target/ and node_modules/ as volumes over the workspace bind mount).
    && mkdir -p /workspace/target /workspace/node_modules \
    && chown -R dev:dev /workspace

USER dev
WORKDIR /workspace

# Same WebKitGTK workaround as shell.nix (harmless where not needed).
ENV WEBKIT_DISABLE_DMABUF_RENDERER=1

CMD ["bash"]
