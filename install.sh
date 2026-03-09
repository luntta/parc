#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
INSTALL_DIR="${CARGO_HOME:-$HOME/.cargo}/bin"

EXE=""
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) EXE=".exe" ;;
esac

echo "==> Installing parc CLI"
cargo install --path "$ROOT/parc-cli"

echo "==> Building parc GUI"
cd "$ROOT/parc-gui/ui"
npm install --silent
npx tauri build 2>&1

GUI_BIN="$ROOT/target/release/parc-gui${EXE}"
if [ ! -f "$GUI_BIN" ]; then
  echo "ERROR: GUI binary not found at $GUI_BIN"
  exit 1
fi

echo "==> Installing parc-gui to $INSTALL_DIR"
cp "$GUI_BIN" "$INSTALL_DIR/parc-gui${EXE}"
chmod +x "$INSTALL_DIR/parc-gui${EXE}" 2>/dev/null || true

echo ""
echo "Installed:"
echo "  parc     -> $INSTALL_DIR/parc${EXE}"
echo "  parc-gui -> $INSTALL_DIR/parc-gui${EXE}"
