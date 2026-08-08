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

say() { printf '\033[1;35m▸\033[0m %s\n' "$*"; }

# ── A stable signing identity ───────────────────────────────────────────
#
# Creates a self-signed code-signing certificate in the login keychain, once.
# See the note above the signing step for why this matters: it is what stops
# macOS re-asking for Keychain access after every single build.
#
# The certificate is not trusted by Gatekeeper — nothing self-signed is — so
# this changes nothing about distribution. What it changes is that every build
# from this machine is the *same application* as far as the Keychain is
# concerned.
make_dev_identity() {
  local name="MCP Gateway Agent (local dev)"
  if security find-certificate -c "$name" >/dev/null 2>&1; then
    say "'$name' already exists; nothing to do."
    return 0
  fi

  local work
  work="$(mktemp -d)"
  trap 'rm -rf "$work"' RETURN

  say "Creating '$name'"
  openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
    -keyout "$work/key.pem" -out "$work/cert.pem" \
    -subj "/CN=$name" \
    -addext "extendedKeyUsage=critical,codeSigning" \
    -addext "basicConstraints=critical,CA:false" \
    -addext "keyUsage=critical,digitalSignature" >/dev/null 2>&1

  # `-legacy` and the SHA-1 PBE algorithms are not optional: OpenSSL 3 defaults
  # to AES-256-CBC with a SHA-256 MAC, and the Security framework's PKCS#12
  # reader rejects that with "MAC verification failed (wrong password?)" — which
  # is the wrong diagnosis and sends you looking for a typo that is not there.
  openssl pkcs12 -export -out "$work/id.p12" \
    -inkey "$work/key.pem" -in "$work/cert.pem" \
    -name "$name" -passout pass:mcpga \
    -legacy -macalg sha1 -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES >/dev/null 2>&1

  security import "$work/id.p12" -k "$HOME/Library/Keychains/login.keychain-db" \
    -P mcpga -T /usr/bin/codesign -A

  say "Done. Rebuild, then click 'Always Allow' on the Keychain prompt once."
  say "It will not ask again."
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --universal) UNIVERSAL=1; shift ;;
    --dmg) MAKE_DMG=1; shift ;;
    --debug) CONFIGURATION="debug"; shift ;;
    --make-dev-identity) make_dev_identity; exit 0 ;;
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
  # `sips -s format png`, not `qlmanage -t`. qlmanage renders a Quick Look
  # *thumbnail*: it composites the artwork onto an opaque background and frames
  # it, so the result is 100% opaque with a #DDD edge. That put a white border
  # around the Dock icon, and made the menu-bar icon a solid white square, since
  # a template image is drawn from its alpha channel alone. sips rasterises the
  # vector at the size asked for and keeps the transparency.
  local png="$work/icon.png"
  sips -s format png -Z 1024 "$source_svg" --out "$png" >/dev/null 2>&1 || true
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
    # Rasterised at each size from the vector rather than scaled from one
    # bitmap, and again via sips so the alpha survives — this one is a template
    # image, so its alpha channel is the whole icon.
    sips -s format png -Z 22 "$tray_svg" --out "$out/agent-tray-Template.png" >/dev/null 2>&1 || true
    sips -s format png -Z 44 "$tray_svg" --out "$out/agent-tray-Template@2x.png" >/dev/null 2>&1 || true
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

# The signing identity is what the Keychain remembers, and that is why this is
# not just a Gatekeeper question.
#
# An ad-hoc signature (`-`) has no identity: the app is identified by the hash of
# its own binary, which changes on **every build**. The Keychain grants access to
# an application by its designated requirement, so with ad-hoc signing every
# build is a different application as far as the Keychain is concerned, and every
# build re-asks "MCP Gateway Agent wants to use your confidential information".
# That is not a one-off after an update — it is once per rebuild, for ever, and
# denying it looks exactly like being signed out.
#
# Signing with any *stable* identity fixes it, because then the designated
# requirement is "this bundle id, signed by this certificate" and it holds across
# rebuilds. A Developer ID certificate is the right one (it also clears
# Gatekeeper), but a self-signed certificate costs nothing and fixes the Keychain
# half on its own. Create one with `./build.sh --make-dev-identity`.
#
# Order: an explicit APPLE_SIGNING_IDENTITY wins, then the local dev identity if
# it exists, then ad-hoc.
DEV_IDENTITY="MCP Gateway Agent (local dev)"
if [[ -n "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  IDENTITY="$APPLE_SIGNING_IDENTITY"
elif security find-certificate -c "$DEV_IDENTITY" >/dev/null 2>&1; then
  IDENTITY="$DEV_IDENTITY"
else
  IDENTITY="-"
  say "No stable signing identity; using ad-hoc."
  say "  macOS will re-ask for Keychain access after every build."
  say "  Run './build.sh --make-dev-identity' once to stop that."
fi
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
