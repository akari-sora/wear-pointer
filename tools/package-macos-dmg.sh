#!/usr/bin/env bash
# Builds a drag-and-drop macOS .dmg installer containing WearPointer.app and /Applications link.
set -euo pipefail

if [[ $# -lt 2 ]]; then
    echo "Usage: $0 <WearPointer.app-path> <output-dmg-path> [volume-name]" >&2
    exit 1
fi

APP_SRC="$1"
OUT_DMG="$2"
VOL_NAME="${3:-Wear Pointer}"

if [[ ! -d "$APP_SRC" ]]; then
    echo "Error: $APP_SRC does not exist" >&2
    exit 1
fi

STAGE_DIR="$(mktemp -d /tmp/wear-pointer-dmg.XXXXXX)"
cleanup() {
    rm -rf "$STAGE_DIR"
}
trap cleanup EXIT

ditto "$APP_SRC" "$STAGE_DIR/WearPointer.app"
ln -s /Applications "$STAGE_DIR/Applications"

cat > "$STAGE_DIR/Quick Start.txt" <<'EOF'
Wear Pointer for macOS
======================

1. Drag "WearPointer.app" into "Applications".
2. Open "WearPointer" from Applications or Spotlight.
   - Look for the crosshair icon in the top-right macOS Menu Bar.
3. Grant Bluetooth and Accessibility permissions when macOS prompts you
   (or click "Grant Accessibility Permission…" in the Menu Bar menu).
4. Open Wear Pointer on your Wear OS watch (BLE APP mode) — it connects automatically!

Tip: Want zero desktop software? Open Settings on the watch and switch Connection
to "BT HID" to pair the watch directly in macOS System Settings -> Bluetooth.
EOF

mkdir -p "$(dirname "$OUT_DMG")"
rm -f "$OUT_DMG"
hdiutil create -volname "$VOL_NAME" -srcfolder "$STAGE_DIR" -ov -format UDZO "$OUT_DMG" >/dev/null
echo "Created $OUT_DMG"
