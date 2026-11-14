#!/usr/bin/env bash
# Start (or reuse) the headless Wear OS emulator for the desktop harness.
set -euo pipefail
export ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
ADB="$ANDROID_HOME/platform-tools/adb"
SERIAL=emulator-5560
LOG=${HARNESS_DIR:-$HOME/wp-harness}/emu.log
if ! "$ADB" devices | grep -q "^$SERIAL"; then
  setsid nohup "$ANDROID_HOME/emulator/emulator" -avd wp_wear35 -port 5560 \
    -no-window -no-audio -no-snapshot -no-boot-anim -gpu swiftshader_indirect \
    > "$LOG" 2>&1 < /dev/null &
  disown || true
fi
for _ in $(seq 1 120); do
  if [[ "$("$ADB" -s $SERIAL shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == "1" ]]; then
    echo "booted"
    exit 0
  fi
  sleep 2
done
echo "emulator did not boot; see $LOG" >&2
exit 1
