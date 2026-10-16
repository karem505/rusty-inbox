#!/bin/bash
# demo-env.sh x|app|start|stop: a private 1920x1080 X display running Rusty Inbox on the demo
# mailbox. Its own D-Bus session keeps tray icons and notifications off your desktop; offline mode
# keeps it off the network.
#   DEMO_DIR (default target/demo)  XVFB (default Xvfb)  DISP (default :78)  THEME (default gruvbox)
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
D=${DEMO_DIR:-$ROOT/target/demo}
DISP=${DISP:-:78}
XV=${XVFB:-$(command -v Xvfb)}
THEMES=${THEMES:-/usr/share/omarchy/themes}
BIN="$ROOT/target/release/rusty-inbox"
x_up() {
  mkdir -p "$D/theme" "$D/logs"
  [ -f "$D/theme/colors.toml" ] || cp "$THEMES/${THEME:-gruvbox}/colors.toml" "$D/theme/colors.toml"
  setsid "$XV" $DISP -screen 0 1920x1080x24 -nolisten tcp > "$D/logs/xvfb.log" 2>&1 < /dev/null &
  sleep 1.5
  pgrep -f "^$XV $DISP" > /dev/null && echo "xvfb up" || echo "xvfb DOWN"
}
app_up() {
  # QtWebEngine paints nothing inside Xvfb unless the GPU path is off
  cd ~ && setsid dbus-run-session -- env -u WAYLAND_DISPLAY DISPLAY=$DISP QT_QPA_PLATFORM=xcb QT_SCALE_FACTOR=1.25 \
    QT_FORCE_STDERR_LOGGING=1 RUSTY_INBOX_DATA_DIR="$D/core-data" RUSTY_INBOX_OFFLINE=1 \
    QTWEBENGINE_CHROMIUM_FLAGS="--disable-gpu" RUSTY_INBOX_THEME_FILE="$D/theme/colors.toml" \
    "$BIN" -qwindowgeometry 1536x864+0+0 > "$D/logs/app.log" 2>&1 < /dev/null &
}
case "$1" in
x) x_up ;;
app) app_up ;;
start) x_up; app_up; sleep 3; pgrep -f "^$BIN" > /dev/null && echo "app up" || echo "app DOWN" ;;
stop)
  for pat in "^$BIN" "^$XV $DISP"; do
    P=$(pgrep -f "$pat"); [ -n "$P" ] && kill $P
  done
  sleep 3
  for pat in "^$BIN" "^$XV $DISP"; do pgrep -f "$pat" | xargs -r kill -9; done
  echo "left app=$(pgrep -fc "^$BIN") xvfb=$(pgrep -fc "^$XV $DISP")"
  ;;
*) echo "usage: $0 x|app|start|stop" >&2; exit 2 ;;
esac
