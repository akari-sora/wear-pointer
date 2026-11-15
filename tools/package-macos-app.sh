#!/usr/bin/env bash
# Packages the macOS receiver binary into a proper WearPointer.app bundle so
# LaunchServices/TCC binds NSBluetoothAlwaysUsageDescription and Accessibility
# permissions to dev.wearpointer.receiver instead of aborting bare terminal runs.
set -euo pipefail

if [[ $# -lt 2 ]]; then
    echo "Usage: $0 <wear-pointer-binary> <output-dir> [version] [codesign-identity]" >&2
    exit 1
fi

BIN_SRC="$1"
OUT_DIR="$2"
VERSION="${3:-0.2.2}"
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
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleVersion</key>
    <string>4</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSBluetoothAlwaysUsageDescription</key>
    <string>WearPointer connects to your Wear OS spatial pointer watch over Bluetooth LE.</string>
</dict>
</plist>
EOF

codesign --force --deep --sign "$SIGN_ID" "$APP_DIR"

cat > "$OUT_DIR/wear-pointer" <<'EOF'
#!/usr/bin/env bash
# Launches WearPointer.app via LaunchServices so macOS TCC resolves Bluetooth
# and Accessibility permissions against dev.wearpointer.receiver.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_BIN="$SCRIPT_DIR/WearPointer.app/Contents/MacOS/wear-pointer"
for arg in "$@"; do
    case "$arg" in
        -h|--help|--version|-V|--replay)
            exec "$APP_BIN" "$@"
            ;;
    esac
done
LOG_FILE="${WEAR_POINTER_LOG:-/tmp/wear-pointer-macos.log}"
echo "wear-pointer: launching WearPointer.app (logs -> $LOG_FILE; Ctrl-C to stop)" >&2
: > "$LOG_FILE"
open -W -n -g --stdout "$LOG_FILE" --stderr "$LOG_FILE" "$SCRIPT_DIR/WearPointer.app" --args "$@" &
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
