#!/usr/bin/env bash
# Packages the macOS receiver binary into a proper WearPointer.app bundle.
# - Double-clicking WearPointer.app in Finder launches the Menu Bar app (top-right icon).
# - Running ./wear-pointer in a terminal preserves 100% of CLI flags & live terminal logs
#   while sharing the exact same TCC Bluetooth & Accessibility grant (dev.wearpointer.receiver).
set -euo pipefail

if [[ $# -lt 2 ]]; then
    echo "Usage: $0 <wear-pointer-binary> <output-dir> [version] [codesign-identity]" >&2
    exit 1
fi

BIN_SRC="$1"
OUT_DIR="$2"
VERSION="${3:-0.3.0}"
SIGN_ID="${4:--}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ICON_SRC="$REPO_ROOT/receiver/crates/wear-pointer/assets/AppIcon.icns"

APP_DIR="$OUT_DIR/WearPointer.app"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"
cp "$BIN_SRC" "$APP_DIR/Contents/MacOS/wear-pointer"
chmod +x "$APP_DIR/Contents/MacOS/wear-pointer"
if [[ -f "$ICON_SRC" ]]; then
    cp "$ICON_SRC" "$APP_DIR/Contents/Resources/AppIcon.icns"
fi

cat > "$APP_DIR/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>wear-pointer</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>dev.wearpointer.receiver</string>
    <key>CFBundleName</key>
    <string>WearPointer</string>
    <key>CFBundleDisplayName</key>
    <string>Wear Pointer</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleVersion</key>
    <string>7</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSBluetoothAlwaysUsageDescription</key>
    <string>WearPointer connects to your Wear OS spatial pointer watch over Bluetooth LE.</string>
</dict>
</plist>
EOF

codesign --force --deep --sign "$SIGN_ID" "$APP_DIR"

cat > "$OUT_DIR/wear-pointer" <<'EOF'
#!/usr/bin/env bash
# Developer / terminal CLI wrapper for WearPointer.app.
# Preserves full CLI output in the terminal while resolving macOS TCC Bluetooth
# and Accessibility permissions against dev.wearpointer.receiver.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_BIN="$SCRIPT_DIR/WearPointer.app/Contents/MacOS/wear-pointer"
WANT_TRAY=0
for arg in "$@"; do
    case "$arg" in
        -h|--help|--version|-V|--replay)
            exec "$APP_BIN" "$@"
            ;;
        --tray)
            WANT_TRAY=1
            ;;
    esac
done
EXTRA_ARGS=()
if [[ "$WANT_TRAY" -eq 0 ]]; then
    EXTRA_ARGS+=(--no-tray)
fi
LOG_FILE="${WEAR_POINTER_LOG:-/tmp/wear-pointer-macos.log}"
echo "wear-pointer: launching WearPointer.app (logs -> $LOG_FILE; Ctrl-C to stop)" >&2
: > "$LOG_FILE"
open -W -n -g --stdout "$LOG_FILE" --stderr "$LOG_FILE" "$SCRIPT_DIR/WearPointer.app" --args "${EXTRA_ARGS[@]}" "$@" &
OPEN_PID=$!
tail -n +1 -f "$LOG_FILE" >&2 &
TAIL_PID=$!
cleanup() {
    kill "$TAIL_PID" 2>/dev/null || true
    pkill -TERM -f "$APP_BIN" 2>/dev/null || true
    wait "$OPEN_PID" 2>/dev/null || true
}
trap cleanup INT TERM EXIT
wait "$OPEN_PID" || true
EOF
chmod +x "$OUT_DIR/wear-pointer"
