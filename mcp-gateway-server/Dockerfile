FROM rust:latest AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release

# Must match the glibc version from rust:latest (Debian Trixie)
FROM debian:trixie-slim AS runtime

# Detect arch for Go download
ARG TARGETARCH

# Install system deps + Node.js + Go (needed to spawn stdio MCP backends)
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
COPY --from=builder /app/target/release/mcp-gateway-server /app/mcp-gateway-server

ENV RUST_LOG=mcp_gateway_server=info,tower_http=debug
ENV LISTEN_ADDR=0.0.0.0:3200

EXPOSE 3200

CMD ["sh", "-c", "update-ca-certificates 2>/dev/null; exec /app/mcp-gateway-server"]
