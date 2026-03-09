#!/usr/bin/env bash
#
# Build a macOS .pkg installer that installs:
#   - parc CLI      -> /usr/local/bin/parc
#   - parc-gui.app  -> /Applications/parc-gui.app
#
# Usage: ./pkg/macos/build-pkg.sh [--sign "Developer ID Installer: ..."]
#
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION="0.1.0"
PKG_ID="com.parc"
STAGING="$ROOT/target/pkg-staging"
OUT="$ROOT/target/parc-${VERSION}.pkg"
SIGN_IDENTITY=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --sign) SIGN_IDENTITY="$2"; shift 2 ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

rm -rf "$STAGING"
mkdir -p "$STAGING/cli/usr/local/bin"
mkdir -p "$STAGING/gui/Applications"
mkdir -p "$STAGING/scripts"
mkdir -p "$STAGING/out"

# --- Build CLI ---
echo "==> Building parc CLI (release)"
cargo build --release --manifest-path "$ROOT/parc-cli/Cargo.toml"
cp "$ROOT/target/release/parc" "$STAGING/cli/usr/local/bin/parc"

# --- Build GUI ---
echo "==> Building parc GUI (tauri)"
cd "$ROOT/parc-gui/ui"
npm install --silent
npx tauri build 2>&1

APP_BUNDLE="$ROOT/target/release/bundle/macos/parc-gui.app"
if [ ! -d "$APP_BUNDLE" ]; then
  echo "ERROR: .app bundle not found at $APP_BUNDLE"
  echo "Check target/release/bundle/macos/ for the actual name"
  exit 1
fi
cp -R "$APP_BUNDLE" "$STAGING/gui/Applications/"

# --- Component packages ---
echo "==> Creating component packages"

pkgbuild \
  --root "$STAGING/cli" \
  --identifier "${PKG_ID}.cli" \
  --version "$VERSION" \
  --install-location "/" \
  "$STAGING/out/parc-cli.pkg"

pkgbuild \
  --root "$STAGING/gui" \
  --identifier "${PKG_ID}.gui" \
  --version "$VERSION" \
  --install-location "/" \
  "$STAGING/out/parc-gui.pkg"

# --- Distribution XML ---
cat > "$STAGING/distribution.xml" <<'DISTXML'
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
    <title>parc</title>
    <welcome mime-type="text/plain"><![CDATA[Install parc — Personal Archive

This package installs:
  • parc CLI (/usr/local/bin/parc)
  • parc GUI (/Applications/parc-gui.app)
]]></welcome>
    <options customize="allow" require-scripts="false" hostArchitectures="x86_64,arm64"/>
    <choices-outline>
        <line choice="cli"/>
        <line choice="gui"/>
    </choices-outline>
    <choice id="cli" title="parc CLI" description="Command-line tool installed to /usr/local/bin">
        <pkg-ref id="com.parc.cli"/>
    </choice>
    <choice id="gui" title="parc GUI" description="Desktop app installed to /Applications">
        <pkg-ref id="com.parc.gui"/>
    </choice>
    <pkg-ref id="com.parc.cli" version="VERSION" installKBytes="CLISIZE">#parc-cli.pkg</pkg-ref>
    <pkg-ref id="com.parc.gui" version="VERSION" installKBytes="GUISIZE">#parc-gui.pkg</pkg-ref>
</installer-gui-script>
DISTXML

# Fill in sizes
CLI_KB=$(du -sk "$STAGING/cli" | cut -f1)
GUI_KB=$(du -sk "$STAGING/gui" | cut -f1)
sed -i '' "s/VERSION/$VERSION/g" "$STAGING/distribution.xml"
sed -i '' "s/CLISIZE/$CLI_KB/g" "$STAGING/distribution.xml"
sed -i '' "s/GUISIZE/$GUI_KB/g" "$STAGING/distribution.xml"

# --- Product package ---
echo "==> Creating installer package"

SIGN_ARGS=()
if [ -n "$SIGN_IDENTITY" ]; then
  SIGN_ARGS=(--sign "$SIGN_IDENTITY")
fi

productbuild \
  --distribution "$STAGING/distribution.xml" \
  --package-path "$STAGING/out" \
  "${SIGN_ARGS[@]}" \
  "$OUT"

rm -rf "$STAGING"

echo ""
echo "Built: $OUT"
echo ""
echo "Size: $(du -h "$OUT" | cut -f1)"
if [ -z "$SIGN_IDENTITY" ]; then
  echo "Note: Package is unsigned. Pass --sign \"Developer ID Installer: ...\" to sign."
fi
