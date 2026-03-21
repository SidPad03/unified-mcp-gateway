#!/bin/bash
set -euo pipefail

# MCP Gateway Agent Installer (macOS & Linux)
# Usage: curl -fsSL https://raw.githubusercontent.com/SidPad03/unified-mcp-gateway/main/install.sh | bash

INSTALL_DIR="$HOME/.mcp-gateway-agent/bin"
BINARY_NAME="mcp-gateway-agent"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

info() { echo -e "${CYAN}[INFO]${NC} $1"; }
success() { echo -e "${GREEN}[OK]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Darwin)
        case "$ARCH" in
            arm64|aarch64) TARGET="aarch64-apple-darwin" ;;
            x86_64)        TARGET="x86_64-apple-darwin" ;;
            *)             error "Unsupported architecture: $ARCH" ;;
        esac
        info "Detected: macOS $ARCH ($TARGET)"
        ;;
    Linux)
        case "$ARCH" in
            aarch64)       TARGET="aarch64-unknown-linux-gnu" ;;
            x86_64)        TARGET="x86_64-unknown-linux-gnu" ;;
            *)             error "Unsupported architecture: $ARCH" ;;
        esac
        info "Detected: Linux $ARCH ($TARGET)"
        ;;
    *)
        error "Unsupported OS: $OS. Use install.ps1 for Windows."
        ;;
esac

# Get gateway URL
if [ -z "${GATEWAY_URL:-}" ]; then
    echo -e "${CYAN}Enter your MCP Gateway URL${NC} (e.g., https://mcp-gateway.example.com):"
    read -r GATEWAY_URL </dev/tty
fi

if [ -z "$GATEWAY_URL" ]; then
    error "Gateway URL is required. Set GATEWAY_URL env var or provide it when prompted."
fi

# Normalize URL: strip trailing slash, convert wss/ws to https/http, strip /agent/ws path
GATEWAY_URL="${GATEWAY_URL%/}"
GATEWAY_URL="${GATEWAY_URL%/agent/ws}"
GATEWAY_URL="${GATEWAY_URL%/}"
GATEWAY_URL="$(echo "$GATEWAY_URL" | sed 's|^wss://|https://|; s|^ws://|http://|')"

# Add https:// if no protocol given
if ! echo "$GATEWAY_URL" | grep -qE '^https?://'; then
    GATEWAY_URL="https://$GATEWAY_URL"
fi

info "Using gateway: $GATEWAY_URL"

# Get API key for authenticated download
if [ -z "${API_KEY:-}" ]; then
    echo -e "${CYAN}Enter your API key${NC} (starts with mcpgw_):"
    read -rs API_KEY </dev/tty
    echo
fi

if [ -z "$API_KEY" ]; then
    error "API key is required for downloading the agent binary."
fi

# Fetch latest release info
info "Fetching latest release info..."
RELEASE_BODY=$(mktemp)
RELEASE_HTTP=$(curl -s -w '%{http_code}' \
    -H "Authorization: Bearer $API_KEY" \
    -o "$RELEASE_BODY" \
    "$GATEWAY_URL/api/v1/agent/releases/latest" 2>/dev/null) || true

if [ "$RELEASE_HTTP" != "200" ]; then
    RELEASE_ERR=$(cat "$RELEASE_BODY" 2>/dev/null | tr -d '\n' | head -c 200)
    rm -f "$RELEASE_BODY"
    case "$RELEASE_HTTP" in
        000) error "Could not connect to $GATEWAY_URL — check the URL and network." ;;
        401|403) error "Authentication failed (HTTP $RELEASE_HTTP) — check your API key." ;;
        404) error "No agent releases found (HTTP 404). Push a tag like 'agent-v0.1.0' to trigger the CI release build." ;;
        *) error "Failed to fetch release info (HTTP $RELEASE_HTTP): $RELEASE_ERR" ;;
    esac
fi

RELEASE_INFO=$(cat "$RELEASE_BODY")
rm -f "$RELEASE_BODY"

VERSION=$(echo "$RELEASE_INFO" | grep -o '"version":"[^"]*"' | head -1 | cut -d'"' -f4)
if [ -z "$VERSION" ]; then
    error "Could not determine latest version from release info."
fi

info "Latest version: v$VERSION"

# Create install directory
mkdir -p "$INSTALL_DIR"

# Download binary
DOWNLOAD_URL="$GATEWAY_URL/api/v1/agent/releases/v$VERSION/download?arch=$TARGET"
BINARY_PATH="$INSTALL_DIR/$BINARY_NAME"
TEMP_PATH="$BINARY_PATH.download"

info "Downloading $BINARY_NAME for $TARGET..."
DOWNLOAD_HTTP=$(curl -s -w '%{http_code}' \
    -H "Authorization: Bearer $API_KEY" \
    -o "$TEMP_PATH" \
    "$DOWNLOAD_URL" 2>/dev/null) || true

if [ "$DOWNLOAD_HTTP" != "200" ]; then
    rm -f "$TEMP_PATH"
    error "Download failed (HTTP $DOWNLOAD_HTTP) — no binary for $TARGET in release v$VERSION."
fi

# Make executable and move into place
chmod +x "$TEMP_PATH"
mv "$TEMP_PATH" "$BINARY_PATH"

success "Downloaded $BINARY_NAME v$VERSION to $BINARY_PATH"

# Detect shell config file(s) and add to PATH
add_to_path() {
    local rc_file="$1"
    local path_line="export PATH=\"$INSTALL_DIR:\$PATH\""

    if [ -f "$rc_file" ]; then
        if ! grep -q "$INSTALL_DIR" "$rc_file" 2>/dev/null; then
            echo "" >> "$rc_file"
            echo "# MCP Gateway Agent" >> "$rc_file"
            echo "$path_line" >> "$rc_file"
            info "Added $INSTALL_DIR to PATH in $rc_file"
        fi
    fi
}

SHELL_NAME="$(basename "${SHELL:-/bin/sh}")"
case "$SHELL_NAME" in
    zsh)
        SHELL_RC="$HOME/.zshrc"
        [ ! -f "$SHELL_RC" ] && touch "$SHELL_RC"
        add_to_path "$SHELL_RC"
        ;;
    bash)
        # On macOS, bash uses .bash_profile; on Linux, .bashrc
        if [ "$OS" = "Darwin" ]; then
            SHELL_RC="$HOME/.bash_profile"
        else
            SHELL_RC="$HOME/.bashrc"
        fi
        [ ! -f "$SHELL_RC" ] && touch "$SHELL_RC"
        add_to_path "$SHELL_RC"
        ;;
    *)
        # Fallback: try .profile
        SHELL_RC="$HOME/.profile"
        [ ! -f "$SHELL_RC" ] && touch "$SHELL_RC"
        add_to_path "$SHELL_RC"
        ;;
esac

echo ""
success "MCP Gateway Agent v$VERSION installed successfully!"
echo ""
echo "Next steps:"
echo "  1. Open a new terminal or run: source $SHELL_RC"
echo "  2. Run the setup wizard:  mcp-gateway-agent setup"
echo ""
