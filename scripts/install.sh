#!/bin/sh
# Rusty Inbox installer / updater for Linux and macOS.
#   curl -fsSL https://raw.githubusercontent.com/karem505/rusty-inbox/main/scripts/install.sh | sh
#   sh -s -- --method appimage|flatpak|source|tarball|dmg   [--version X] [--update] [--uninstall]
set -eu

REPO="karem505/rusty-inbox"
APP_ID="io.github.karem505.rusty-inbox"
FLATPAK_REF="https://karem505.github.io/rusty-inbox/rusty-inbox.flatpakref"
PREFIX="${PREFIX:-$HOME/.local}"
LIB_DIR="$PREFIX/lib/rusty-inbox"
BIN_DIR="$PREFIX/bin"
APPS_DIR="$PREFIX/share/applications"
ICONS_DIR="$PREFIX/share/icons/hicolor"
SRC_DIR="${RUSTY_INBOX_SRC:-$PREFIX/src/rusty-inbox}"

METHOD=""
VERSION=""
ACTION="install"

say() { printf '%s\n' "$*"; }
err() { printf 'error: %s\n' "$*" >&2; }
die() { err "$*"; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

usage() {
  cat <<USAGE
Rusty Inbox installer

Usage: install.sh [--method appimage|flatpak|source|tarball|dmg] [--version X.Y.Z] [--update] [--uninstall]

  --method     how to install (default: dmg on macOS, appimage on Linux)
  --version    install this release instead of the latest
  --update     install the latest release if it is newer than the installed one
  --uninstall  remove what this script installed
USAGE
}

while [ $# -gt 0 ]; do
  case "$1" in
    --method) [ $# -ge 2 ] || die "--method needs a value"; METHOD="$2"; shift 2 ;;
    --method=*) METHOD="${1#--method=}"; shift ;;
    --version) [ $# -ge 2 ] || die "--version needs a value"; VERSION="$2"; shift 2 ;;
    --version=*) VERSION="${1#--version=}"; shift ;;
    --update) ACTION="update"; shift ;;
    --uninstall) ACTION="uninstall"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; die "unknown option: $1" ;;
  esac
done
VERSION="${VERSION#v}"

OS="$(uname -s)"
MACHINE="$(uname -m)"
case "$OS" in
  Linux) PLATFORM=linux ;;
  Darwin) PLATFORM=macos ;;
  *) die "unsupported OS: $OS (on Windows use scripts/install.ps1)" ;;
esac
case "$MACHINE" in
  x86_64|amd64) ARCH=x86_64 ;;
  arm64|aarch64) ARCH=arm64 ;;
  *) die "unsupported architecture: $MACHINE" ;;
esac

fetch() { # fetch URL [DEST]
  have curl || die "curl is required"
  if [ $# -ge 2 ]; then curl -fL --retry 3 --progress-bar -o "$2" "$1"; else curl -fsSL --retry 3 "$1"; fi
}

latest_version() {
  json="$(fetch "https://api.github.com/repos/$REPO/releases/latest")" || die "could not query the latest release"
  tag="$(printf '%s\n' "$json" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  [ -n "$tag" ] || die "no published release found for $REPO"
  printf '%s\n' "${tag#v}"
}

asset_url() { # asset name
  printf 'https://github.com/%s/releases/download/v%s/%s\n' "$REPO" "$VERSION" "$1"
}

installed_version() {
  if [ -f "$LIB_DIR/version" ]; then cat "$LIB_DIR/version"; return; fi
  for b in "$BIN_DIR/rusty-inbox" "$HOME/Applications/Rusty Inbox.app/Contents/MacOS/rusty-inbox" \
    "/Applications/Rusty Inbox.app/Contents/MacOS/rusty-inbox"; do
    if [ -x "$b" ]; then "$b" --version 2>/dev/null | sed 's/^rusty-inbox //' | head -n 1; return; fi
  done
  have rusty-inbox && rusty-inbox --version 2>/dev/null | sed 's/^rusty-inbox //' | head -n 1
  return 0
}

default_method() {
  if [ "$PLATFORM" = macos ]; then METHOD=dmg; return; fi
  if [ -r /etc/os-release ] && grep -qi 'arch' /etc/os-release 2>/dev/null; then
    say "Arch Linux detected: a native package is also available (makepkg -si in packaging/aur/rusty-inbox-bin)."
  fi
  METHOD=appimage
}

refresh_desktop() {
  if have update-desktop-database; then update-desktop-database "$APPS_DIR" >/dev/null 2>&1 || true; fi
  if have gtk-update-icon-cache; then gtk-update-icon-cache -q -f -t "$ICONS_DIR" >/dev/null 2>&1 || true; fi
}

set_mailto_default() {
  if have xdg-mime; then
    xdg-mime default rusty-inbox.desktop x-scheme-handler/mailto >/dev/null 2>&1 &&
      say "mailto: links now open in Rusty Inbox" || true
  fi
}

install_icons_from() { # dir containing rusty-inbox-<size>.png
  for s in 16 22 24 32 48 64 96 128 256 512; do
    [ -f "$1/rusty-inbox-$s.png" ] || continue
    mkdir -p "$ICONS_DIR/${s}x${s}/apps"
    cp "$1/rusty-inbox-$s.png" "$ICONS_DIR/${s}x${s}/apps/rusty-inbox.png"
  done
}

write_desktop_entry() { # exec path
  mkdir -p "$APPS_DIR"
  cat >"$APPS_DIR/rusty-inbox.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Rusty Inbox
GenericName=Email Client
Comment=Fast native email client built with Rust and Qt/QML
Exec=$1 %u
Icon=rusty-inbox
Terminal=false
Categories=Network;Email;Qt;
Keywords=email;mail;inbox;imap;smtp;rust;qt;
MimeType=x-scheme-handler/mailto;
StartupNotify=true
StartupWMClass=rusty-inbox
Actions=compose;

[Desktop Action compose]
Name=New Message
Exec=$1 --compose
DESKTOP
}

icons_from_repo() { # fetch the icon set from the tagged source archive
  tmp="$(mktemp -d)"
  if fetch "https://github.com/$REPO/archive/refs/tags/v$VERSION.tar.gz" "$tmp/src.tar.gz" 2>/dev/null &&
    tar -xzf "$tmp/src.tar.gz" -C "$tmp" 2>/dev/null; then
    d="$(find "$tmp" -type d -path '*app/packaging/icons' | head -n 1)"
    [ -n "$d" ] && install_icons_from "$d"
  fi
  rm -rf "$tmp"
}

install_appimage() {
  [ "$ARCH" = x86_64 ] || die "the AppImage is only built for x86_64; try --method source or flatpak"
  name="rusty-inbox-$VERSION-x86_64.AppImage"
  mkdir -p "$LIB_DIR" "$BIN_DIR"
  tmp="$LIB_DIR/$name.part"
  say "Downloading $name"
  fetch "$(asset_url "$name")" "$tmp" || { rm -f "$tmp"; die "download failed (does release v$VERSION exist?)"; }
  chmod +x "$tmp"
  mv "$tmp" "$LIB_DIR/$name"
  for old in "$LIB_DIR"/rusty-inbox-*.AppImage; do
    [ "$old" = "$LIB_DIR/$name" ] || rm -f "$old"
  done
  ln -sf "$LIB_DIR/$name" "$LIB_DIR/rusty-inbox.AppImage"
  ln -sf "$LIB_DIR/rusty-inbox.AppImage" "$BIN_DIR/rusty-inbox"
  printf '%s\n' "$VERSION" >"$LIB_DIR/version"
  icons_from_repo
  write_desktop_entry "$LIB_DIR/rusty-inbox.AppImage"
  refresh_desktop
  set_mailto_default
  say "Installed Rusty Inbox $VERSION to $LIB_DIR (command: $BIN_DIR/rusty-inbox)"
  case ":$PATH:" in *":$BIN_DIR:"*) ;; *) say "Note: add $BIN_DIR to your PATH." ;; esac
}

install_tarball() {
  [ "$ARCH" = x86_64 ] || die "the tarball is only built for x86_64"
  name="rusty-inbox-$VERSION-linux-x86_64"
  tmp="$(mktemp -d)"
  fetch "$(asset_url "$name.tar.gz")" "$tmp/t.tar.gz" || { rm -rf "$tmp"; die "download failed"; }
  tar -xzf "$tmp/t.tar.gz" -C "$tmp"
  mkdir -p "$LIB_DIR" "$BIN_DIR"
  install -m 755 "$tmp/$name/rusty-inbox" "$LIB_DIR/rusty-inbox-bin"
  ln -sf "$LIB_DIR/rusty-inbox-bin" "$BIN_DIR/rusty-inbox"
  printf '%s\n' "$VERSION" >"$LIB_DIR/version"
  install_icons_from "$tmp/$name/icons"
  write_desktop_entry "$BIN_DIR/rusty-inbox"
  rm -rf "$tmp"
  refresh_desktop
  set_mailto_default
  say "Installed Rusty Inbox $VERSION (needs Qt 6 with QtWebEngine from your distribution)."
}

install_flatpak() {
  have flatpak || die "flatpak is not installed"
  if flatpak remote-info --user flathub "$APP_ID" >/dev/null 2>&1 ||
    flatpak remote-info flathub "$APP_ID" >/dev/null 2>&1; then
    flatpak install --user -y flathub "$APP_ID"
  else
    # the project's own signed repository; `flatpak update` keeps it current afterwards
    flatpak install --user -y --noninteractive "$FLATPAK_REF" ||
      die "could not install from $FLATPAK_REF"
  fi
  say "Installed with Flatpak (updates come with 'flatpak update'). Run: flatpak run $APP_ID"
}

install_source() {
  have git || die "git is required"
  have cargo || die "Rust is required: https://rustup.rs"
  QMAKE_BIN=""
  for q in qmake6 qmake-qt6 qmake; do
    if have "$q"; then QMAKE_BIN="$q"; break; fi
  done
  [ -n "$QMAKE_BIN" ] || die "Qt 6 (qmake6) not found; install Qt 6 with QtDeclarative, QtSvg and QtWebEngine"
  mkdir -p "$(dirname "$SRC_DIR")"
  if [ -d "$SRC_DIR/.git" ]; then
    git -C "$SRC_DIR" fetch --tags --quiet
  else
    git clone "https://github.com/$REPO" "$SRC_DIR"
  fi
  if [ -n "$VERSION" ]; then git -C "$SRC_DIR" checkout --quiet "v$VERSION"; else git -C "$SRC_DIR" checkout --quiet main && git -C "$SRC_DIR" pull --ff-only --quiet; fi
  (cd "$SRC_DIR" && QMAKE="$(command -v "$QMAKE_BIN")" cargo build --release -p rusty-inbox)
  (cd "$SRC_DIR" && PREFIX="$PREFIX" app/packaging/install-user.sh)
  mkdir -p "$LIB_DIR"
  "$BIN_DIR/rusty-inbox" --version | sed 's/^rusty-inbox //' >"$LIB_DIR/version"
}

install_dmg() {
  [ "$PLATFORM" = macos ] || die "dmg is for macOS"
  name="rusty-inbox-$VERSION-macos-$ARCH.dmg"
  tmp="$(mktemp -d)"
  fetch "$(asset_url "$name")" "$tmp/$name" || { rm -rf "$tmp"; die "download failed (does release v$VERSION exist?)"; }
  mnt="$tmp/mnt"; mkdir -p "$mnt"
  hdiutil attach "$tmp/$name" -mountpoint "$mnt" -nobrowse -quiet
  if [ -w /Applications ]; then dest=/Applications; else dest="$HOME/Applications"; mkdir -p "$dest"; fi
  rm -rf "$dest/Rusty Inbox.app"
  cp -R "$mnt/Rusty Inbox.app" "$dest/"
  hdiutil detach "$mnt" -quiet || true
  xattr -dr com.apple.quarantine "$dest/Rusty Inbox.app" 2>/dev/null || true
  rm -rf "$tmp"
  say "Installed Rusty Inbox $VERSION to $dest/Rusty Inbox.app"
}

uninstall() {
  if [ "$PLATFORM" = macos ]; then
    rm -rf "/Applications/Rusty Inbox.app" "$HOME/Applications/Rusty Inbox.app" 2>/dev/null || true
  else
    rm -rf "$LIB_DIR"
    if [ -L "$BIN_DIR/rusty-inbox" ] || [ -f "$BIN_DIR/rusty-inbox" ]; then rm -f "$BIN_DIR/rusty-inbox"; fi
    rm -f "$APPS_DIR/rusty-inbox.desktop"
    for s in 16 22 24 32 48 64 96 128 256 512; do rm -f "$ICONS_DIR/${s}x${s}/apps/rusty-inbox.png"; done
    refresh_desktop
    if have flatpak && flatpak info --user "$APP_ID" >/dev/null 2>&1; then flatpak uninstall --user -y "$APP_ID" || true; fi
  fi
  say "Rusty Inbox removed. Your mail data is kept (~/.local/share/rusty-inbox or the platform equivalent)."
}

case "$ACTION" in
  uninstall) uninstall; exit 0 ;;
esac

[ -n "$METHOD" ] || default_method
if [ "$METHOD" != source ] || [ "$ACTION" = update ]; then
  [ -n "$VERSION" ] || VERSION="$(latest_version)"
fi

if [ "$ACTION" = update ]; then
  cur="$(installed_version || true)"
  if [ -n "$cur" ] && [ "$cur" = "$VERSION" ]; then say "Rusty Inbox $cur is up to date."; exit 0; fi
  say "Updating ${cur:-(not installed)} -> $VERSION"
fi

case "$METHOD" in
  appimage) [ "$PLATFORM" = linux ] || die "AppImage is Linux only"; install_appimage ;;
  tarball) [ "$PLATFORM" = linux ] || die "tarball is Linux only"; install_tarball ;;
  flatpak) [ "$PLATFORM" = linux ] || die "Flatpak is Linux only"; install_flatpak ;;
  source) install_source ;;
  dmg) install_dmg ;;
  *) die "unknown method: $METHOD" ;;
esac
