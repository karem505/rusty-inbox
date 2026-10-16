#!/bin/bash
# Install Rusty Inbox for the current user: binary, launcher entry and the full icon set.
# Usage: app/packaging/install-user.sh   (after `cargo build --release`)
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
PKG="$ROOT/app/packaging"
BIN="$ROOT/target/release/rusty-inbox"
[ -x "$BIN" ] || { echo "build first: cargo build --release -p rusty-inbox" >&2; exit 1; }
PREFIX=${PREFIX:-$HOME/.local}
ICONS="$PREFIX/share/icons/hicolor"
install -Dm755 "$BIN" "$PREFIX/bin/rusty-inbox"
install -Dm644 "$PKG/rusty-inbox.desktop" "$PREFIX/share/applications/rusty-inbox.desktop"
# raster app icon at every hicolor size; drop any old scalable placeholder (it would win)
rm -f "$ICONS/scalable/apps/rusty-inbox.svg"
for s in 16 22 24 32 48 64 96 128 256 512; do
  install -Dm644 "$PKG/icons/rusty-inbox-$s.png" "$ICONS/${s}x${s}/apps/rusty-inbox.png"
done
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -f -t "$ICONS" || true
echo "installed $PREFIX/bin/rusty-inbox (+ desktop entry, ${ICONS})"
