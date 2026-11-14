#!/usr/bin/env bash
# Desktop harness: Wear OS emulator -> netsim HCI -> Bumble /dev/vhci bridge ->
# BlueZ -> real Rust wear-pointer --backend hyprland -> real Hyprland (QEMU/KVM
# + seatd + virtio-vga) -> fullscreen wev + Hyprland IPC cursor sampler.
#
# Runs on an x86_64 Linux host with KVM, QEMU, and the Android SDK/emulator.
# See docs/desktop-harness.md. Usage:
#   tools/harness/run.sh [RUN_NAME]
# Exit code 0 when tools/harness/check.py passes.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
RUN_NAME=${1:-$(date +%Y%m%d-%H%M%S)}
WORK=${HARNESS_DIR:-$HOME/wp-harness}
export VM_DIR=${VM_DIR:-$WORK/vm}
SHARE="$VM_DIR/share"
OUT="$SHARE/out/$RUN_NAME"
APK=${APK:-$WORK/app-debug.apk}
RECEIVER=${RECEIVER:-$REPO/receiver/target/release/wear-pointer}
export ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
ADB="$ANDROID_HOME/platform-tools/adb"
W=emulator-5560

[[ -f "$HOME/.cargo/env" ]] && . "$HOME/.cargo/env"

mkdir -p "$SHARE" "$OUT"
echo "harness run: $RUN_NAME -> $OUT"

if [[ ! -x "$RECEIVER" || "${BUILD:-0}" == "1" ]]; then
  echo "building receiver..."
  (cd "$REPO/receiver" && cargo build --release -p wear-pointer)
fi

if [[ ! -f "$APK" ]]; then
  CANDIDATE="$REPO/wearos/app/build/outputs/apk/debug/app-debug.apk"
  if [[ ! -f "$CANDIDATE" ]]; then
    echo "building wearos debug APK..."
    (cd "$REPO/wearos" && ./gradlew :app:assembleDebug)
  fi
  APK="$CANDIDATE"
fi

# 1. Watch emulator (Wear OS API 35 x86_64).
"$HERE/make-avd.sh" > /dev/null
"$HERE/start-emulator.sh"
"$ADB" -s $W install -r -g "$APK" > /dev/null
"$ADB" -s $W shell am force-stop dev.wearpointer
"$ADB" -s $W shell cmd sensorservice reset-uid-state dev.wearpointer > /dev/null 2>&1 || true
"$ADB" -s $W shell svc power stayon true > /dev/null 2>&1 || true
"$ADB" -s $W shell settings put system screen_off_timeout 600000 > /dev/null 2>&1 || true
"$ADB" -s $W shell input keyevent KEYCODE_WAKEUP > /dev/null 2>&1 || true
"$ADB" -s $W emu sensor set gyroscope 0:0:0 > /dev/null

NETSIM_INI=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/netsim.ini
HCI_PORT=$(sed -n 's/^hci.port=//p' "$NETSIM_INI")
[[ -n "$HCI_PORT" ]] || { echo "no hci.port in $NETSIM_INI" >&2; exit 1; }
echo "netsim hci.port: $HCI_PORT"

# 2. Desktop VM (Arch + Hyprland + seatd + BlueZ + /dev/vhci).
[[ -f "$VM_DIR/desktop.qcow2" ]] || "$HERE/vm/make-vm.sh"
"$HERE/vm/start-vm.sh"

cp "$RECEIVER" "$SHARE/wear-pointer"
chmod +x "$SHARE/wear-pointer"
cp "$HERE/hyprland.lua" "$HERE/bridge.py" \
   "$HERE/vm/hypr-start.sh" "$HERE/vm/desktop-up.sh" \
   "$HERE/vm/desktop-down.sh" "$HERE/vm/cursor-sampler.py" "$SHARE/"
chmod +x "$SHARE/hypr-start.sh" "$SHARE/desktop-up.sh" "$SHARE/desktop-down.sh" "$SHARE/cursor-sampler.py"

# Align VM wall clock with the host so marks.log, trace.log, and cursor.log match.
"$HERE/vm/vm-ssh.sh" "sudo date -u -s '@$(date -u +%s.%N)' >/dev/null"

cleanup() {
  "$HERE/vm/vm-ssh.sh" "bash /mnt/share/desktop-down.sh" >/dev/null 2>&1 || true
}
trap cleanup EXIT

"$HERE/vm/vm-ssh.sh" "bash /mnt/share/desktop-up.sh '$RUN_NAME' '$HCI_PORT'"

# 3. Watch app + scenario.
"$ADB" -s $W logcat -c
"$ADB" -s $W shell am start -n dev.wearpointer/.MainActivity > /dev/null
"$HERE/scenario.sh" "$OUT"

"$ADB" -s $W logcat -d -s wear-pointer > "$OUT/watch.log"
"$HERE/vm/vm-ssh.sh" "bash /mnt/share/desktop-down.sh"
trap - EXIT

python3 "$HERE/check.py" "$OUT"
