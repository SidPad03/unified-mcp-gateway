# Multi-arch image, built on one architecture.
#
# CI runs on x86_64 self-hosted runners, so the arm64 image has to come off an
# amd64 machine. The expensive part — the Rust release build — is
# **cross-compiled** rather than emulated: the builder stage is pinned to
# `$BUILDPLATFORM` and targets `$TARGETARCH`, which is minutes instead of the
# best part of an hour under QEMU.
#
# The runtime stage is *not* pinned, so it resolves to the target platform and
# its `RUN` steps do go through emulation. That is deliberate and cheap: they are
# apt-get and two tarballs, not a compiler.
#
# The only C in the dependency graph is `ring` (via rustls, via sqlx), which
# needs a cross gcc and nothing else — no OpenSSL, no cmake.

FROM --platform=$BUILDPLATFORM rust:latest AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
ARG TARGETARCH

# Resolve the Rust target triple once and leave it where later steps can read
# it — a RUN cannot export an ENV.
#
# The cross toolchain is installed only when the build and target architectures
# actually differ, and the direction is not assumed: CI crosses amd64 → arm64,
# but building on an Apple Silicon machine crosses the other way, and a
# Dockerfile that only handles one of those is a Dockerfile nobody can reproduce
# locally.
RUN set -eux; \
    case "$TARGETARCH" in \
      amd64) triple=x86_64-unknown-linux-gnu;  cross="gcc-x86-64-linux-gnu libc6-dev-amd64-cross" ;; \
      arm64) triple=aarch64-unknown-linux-gnu; cross="gcc-aarch64-linux-gnu libc6-dev-arm64-cross" ;; \
      *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    echo "$triple" > /triple; \
    rustup target add "$triple"; \
    if [ "$(dpkg --print-architecture)" != "$TARGETARCH" ]; then \
      apt-get update; \
      apt-get install -y --no-install-recommends $cross; \
      rm -rf /var/lib/apt/lists/*; \
    fi

# Only ever consulted for the target being built, so naming both is harmless.
# `cc` reads the CC_/AR_ pair when it assembles ring.
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
    CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \
    AR_x86_64_unknown_linux_gnu=x86_64-linux-gnu-ar

# Dependencies first, so a source-only change does not rebuild them.
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --target "$(cat /triple)" --recipe-path recipe.json

COPY . .
RUN cargo build --release --target "$(cat /triple)" \
 && cp "target/$(cat /triple)/release/mcp-gateway-server" /app/mcp-gateway-server

# Must match the glibc version from rust:latest (Debian Trixie)
FROM debian:trixie-slim AS runtime

# Detect arch for Go download
ARG TARGETARCH

# Install system deps + Node.js + Go (needed to spawn stdio MCP backends).
#
# `libssl3` is no longer needed by the gateway itself — it uses rustls — but the
# stdio MCP backends it spawns are third-party Python and Node programs that
# often do link OpenSSL, so it stays.
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libssl3 \
    gnupg \
    git \
    python3 \
    nodejs \
    npm \
    && rm -rf /var/lib/apt/lists/*

# Install Go (arch-aware)
RUN GOARCH=$(case "${TARGETARCH}" in arm64) echo "arm64";; *) echo "amd64";; esac) \
    && curl -fsSL "https://go.dev/dl/go1.23.6.linux-${GOARCH}.tar.gz" | tar -C /usr/local -xz
ENV PATH="/usr/local/go/bin:${PATH}"

# Install uv + uvx (needed for Python-based MCP backends like Serena)
RUN curl -LsSf https://astral.sh/uv/install.sh | sh
ENV PATH="/root/.local/bin:${PATH}"

WORKDIR /app
COPY --from=builder /app/mcp-gateway-server /app/mcp-gateway-server

ENV RUST_LOG=mcp_gateway_server=info,tower_http=debug
ENV LISTEN_ADDR=0.0.0.0:3200

EXPOSE 3200

CMD ["sh", "-c", "update-ca-certificates 2>/dev/null; exec /app/mcp-gateway-server"]
