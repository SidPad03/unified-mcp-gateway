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

# Runtime - includes tools needed for local MCP backends
FROM debian:trixie-slim AS runtime

ARG TARGETARCH

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

# Install uv + uvx (for Python MCP backends like Serena)
RUN curl -LsSf https://astral.sh/uv/install.sh | sh
ENV PATH="/root/.local/bin:${PATH}"

WORKDIR /app
COPY --from=builder /app/target/release/mcp-gateway-agent /app/mcp-gateway-agent

ENV RUST_LOG=mcp_gateway_agent=info

# The CLI requires a subcommand, and the config path is a flag rather than a
# positional argument — passing the path alone made clap exit 2 ("unrecognized
# subcommand") before any agent code ran. `--foreground` is required too: a bare
# `run` daemonizes by spawning a detached child and returning, which as PID 1
# would stop the container immediately. Mount your config at the path below.
CMD ["/app/mcp-gateway-agent", "run", "--foreground", "--config", "/app/mcp-gateway-agent.toml"]
