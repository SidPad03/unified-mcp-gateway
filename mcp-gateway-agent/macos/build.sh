#!/usr/bin/env bash
#
# Build MCP Gateway Agent.app (and optionally a .dmg).
#
# There is no .xcodeproj on purpose. Everything here is Command Line Tools —
# swiftc, cargo, iconutil, codesign, hdiutil — so a checkout builds without
# Xcode, CI does exactly what you can run locally, and the whole packaging step
# is a script you can read rather than a project file you cannot diff.
#
#   ./build.sh                      # host architecture, ad-hoc signed
#   ./build.sh --universal          # arm64 + x86_64 (needs both rust std libs)
#   ./build.sh --version 1.2.3 --dmg
#
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"
AGENT_ROOT="$(cd .. && pwd)"
REPO_ROOT="$(cd ../.. && pwd)"

VERSION=""
UNIVERSAL=0
MAKE_DMG=0
CONFIGURATION="release"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --universal) UNIVERSAL=1; shift ;;
    --dmg) MAKE_DMG=1; shift ;;
    --debug) CONFIGURATION="debug"; shift ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
done

if [[ -z "$VERSION" ]]; then
  # The version of record lives in the FFI crate; CI patches it there.
  VERSION="$(grep '^version' "$AGENT_ROOT/ffi/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')"
fi

APP_NAME="MCP Gateway Agent"
BUILD_DIR="$AGENT_ROOT/build"
APP="$BUILD_DIR/$APP_NAME.app"

say() { printf '\033[1;35m▸\033[0m %s\n' "$*"; }

# ── 1. The Rust core ────────────────────────────────────────────────────

RUST_TARGETS=()
if [[ $UNIVERSAL -eq 1 ]]; then
  RUST_TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
else
  RUST_TARGETS=("$(rustc -vV | awk '/^host:/ {print $2}')")
fi

# cargo reserves the profile name `debug`: the built-in development profile is
# called `dev` and *outputs* to target/debug. swift build wants `debug` for the
# same thing. Translate here rather than making the caller know.
CARGO_PROFILE="$CONFIGURATION"
[[ "$CONFIGURATION" == "debug" ]] && CARGO_PROFILE="dev"

say "Building the agent core for: ${RUST_TARGETS[*]}"
for target in "${RUST_TARGETS[@]}"; do
  if ! rustc --print target-libdir --target "$target" >/dev/null 2>&1; then
    cat >&2 <<EOF
error: the Rust standard library for $target is not installed.

  rustup target add $target

A Homebrew rust only ships the host target, so --universal needs rustup.
EOF
    exit 1
  fi
  cargo build --manifest-path "$AGENT_ROOT/Cargo.toml" \
    -p mcp-gateway-agent-ffi --profile "$CARGO_PROFILE" --target "$target"
done

LIB_DIR="$BUILD_DIR/lib"
mkdir -p "$LIB_DIR"
LIB_NAME="libmcp_gateway_agent_ffi.a"
ARCHIVES=()
for target in "${RUST_TARGETS[@]}"; do
  ARCHIVES+=("$AGENT_ROOT/target/$target/$CONFIGURATION/$LIB_NAME")
done

if [[ ${#ARCHIVES[@]} -gt 1 ]]; then
  say "Merging the static libraries into one universal archive"
  lipo -create -output "$LIB_DIR/$LIB_NAME" "${ARCHIVES[@]}"
else
  cp "${ARCHIVES[0]}" "$LIB_DIR/$LIB_NAME"
fi

# ── 2. The Swift app ────────────────────────────────────────────────────

# macOS ships bash 3.2, where "${empty[@]}" trips `set -u`. Build the flag list
# as a string and let word splitting do the rest.
SWIFT_ARCH_FLAGS=""
if [[ $UNIVERSAL -eq 1 ]]; then
  SWIFT_ARCH_FLAGS="--arch arm64 --arch x86_64"
fi

say "Building the app"
# shellcheck disable=SC2086
swift build \
  --package-path . \
  --configuration "$CONFIGURATION" \
  $SWIFT_ARCH_FLAGS \
  -Xlinker -L"$LIB_DIR"

# shellcheck disable=SC2086
BINARY="$(swift build --package-path . --configuration "$CONFIGURATION" \
  $SWIFT_ARCH_FLAGS -Xlinker -L"$LIB_DIR" --show-bin-path)/MCPGatewayAgent"

# ── 3. Icons ────────────────────────────────────────────────────────────

make_icons() {
  local out="$1"
  local source_svg="$REPO_ROOT/brand/agent-app-icon.svg"
  [[ -f "$source_svg" ]] || { say "No app icon artwork; skipping"; return; }

  local work
  work="$(mktemp -d)"
  # qlmanage ships with macOS and rasterises SVG; no extra dependency.
  qlmanage -t -s 1024 -o "$work" "$source_svg" >/dev/null 2>&1 || true
  local png="$work/$(basename "$source_svg").png"
  [[ -f "$png" ]] || { say "Could not rasterise the icon; skipping"; rm -rf "$work"; return; }

  local iconset="$work/AppIcon.iconset"
  mkdir -p "$iconset"
  for size in 16 32 64 128 256 512; do
    sips -z $size $size "$png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
    sips -z $((size * 2)) $((size * 2)) "$png" \
      --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
  done
  iconutil -c icns "$iconset" -o "$out/AppIcon.icns"

  # The menu-bar icon must keep its `Template` suffix: AppKit keys off the
  # filename to decide whether to invert it on a light menu bar and highlight it
  # when the menu is open.
  local tray_svg="$REPO_ROOT/brand/agent-tray-Template.svg"
  if [[ -f "$tray_svg" ]]; then
    qlmanage -t -s 44 -o "$work" "$tray_svg" >/dev/null 2>&1 || true
    local tray_png="$work/$(basename "$tray_svg").png"
    if [[ -f "$tray_png" ]]; then
      sips -z 22 22 "$tray_png" --out "$out/agent-tray-Template.png" >/dev/null
      sips -z 44 44 "$tray_png" --out "$out/agent-tray-Template@2x.png" >/dev/null
    fi
  fi
  rm -rf "$work"
}

# ── 4. The bundle ───────────────────────────────────────────────────────

say "Assembling $APP_NAME.app ($VERSION)"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp "$BINARY" "$APP/Contents/MacOS/MCPGatewayAgent"
cp Resources/Info.plist "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"
make_icons "$APP/Contents/Resources"

plist="$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $VERSION" "$plist"
if [[ -n "${MCPGA_UPDATE_PUBLIC_KEY:-}" ]]; then
  /usr/libexec/PlistBuddy -c "Set :MCPGAUpdatePublicKey $MCPGA_UPDATE_PUBLIC_KEY" "$plist"
  say "Update signing key embedded"
else
  say "No MCPGA_UPDATE_PUBLIC_KEY set — this build will not self-install updates"
fi
if [[ -n "${MCPGA_APPCAST_URL:-}" ]]; then
  /usr/libexec/PlistBuddy -c "Set :MCPGAAppcastURL $MCPGA_APPCAST_URL" "$plist"
fi

if [[ "$CONFIGURATION" == "release" ]]; then
  strip -x "$APP/Contents/MacOS/MCPGatewayAgent" 2>/dev/null || true
fi

# ── 5. Signing ──────────────────────────────────────────────────────────

# Ad-hoc unless a Developer ID identity is configured (decision D1). An ad-hoc
# signature is enough for the app to run and for `SMAppService` to register it;
# what it does not do is clear Gatekeeper's quarantine on a fresh download, which
# is why the README says right-click → Open the first time.
IDENTITY="${APPLE_SIGNING_IDENTITY:--}"
say "Signing with identity: $IDENTITY"
codesign --force --deep --options runtime \
  --sign "$IDENTITY" \
  --identifier com.mcpgateway.agent \
  "$APP"
codesign --verify --strict "$APP" && say "Signature verified"

# ── 6. Disk image ───────────────────────────────────────────────────────

if [[ $MAKE_DMG -eq 1 ]]; then
  DMG="$BUILD_DIR/MCP-Gateway-Agent-$VERSION.dmg"
  say "Creating $(basename "$DMG")"
  rm -f "$DMG"
  staging="$(mktemp -d)"
  cp -R "$APP" "$staging/"
  ln -s /Applications "$staging/Applications"
  hdiutil create -volname "$APP_NAME" -srcfolder "$staging" \
    -ov -format ULFO "$DMG" >/dev/null
  rm -rf "$staging"
  say "$DMG"
fi

say "Done: $APP"
