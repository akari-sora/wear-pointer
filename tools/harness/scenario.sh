#!/usr/bin/env bash
# Drives the watch emulator for the desktop harness and writes step marks.
#   scenario.sh OUT_DIR
# Marks: "<unix seconds> <step>" in OUT_DIR/marks.log. check.py reads them.
set -uo pipefail
OUT=$1
export ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
ADB="$ANDROID_HOME/platform-tools/adb"
W=emulator-5560
MARKS="$OUT/marks.log"
: > "$MARKS"
mark() { echo "$(date -u +%s.%N) $1" >> "$MARKS"; echo "step: $1"; }
gyro() { "$ADB" -s $W emu sensor set gyroscope "$1" > /dev/null; }
tap() { "$ADB" -s $W shell input tap "$1" "$2"; }
count_trace() { grep -c "$1" "$OUT/trace.log" 2>/dev/null || true; }
wait_more() {  # wait_more PATTERN PREV_COUNT TIMEOUT_S
  for _ in $(seq 1 "$3"); do
    [[ $(count_trace "$1") -gt "$2" ]] && return 0
    sleep 1
  done
  return 1
}

# Watch UI (wearos_large_round, 454x454 circular instrument layout).
CENTER="227 324"; PAD="227 177"; RIGHT="104 324"; PAUSE="350 324"; STOP="227 404"
YAW=${YAW_AXIS:-y}   # the emulator's default pose is upright: up ~ device +Y
turn() {  # turn RATE(rad/s) SECONDS
  case $YAW in
    y) gyro "0:$1:0" ;;
    z) gyro "0:0:$1" ;;
  esac
  sleep "$2"
  gyro 0:0:0
}

gyro 0:0:0
mark start
wait_more " rx " 0 45 || { mark "no-link"; exit 1; }
sleep 2

mark center1; tap $CENTER; sleep 1.2
mark turn-left; turn 0.5 1; sleep 1.0
mark center-mid-turn
gyro "0:0.5:0"; sleep 0.5; tap $CENTER; sleep 0.5; gyro 0:0:0; sleep 1.0
mark click; tap $PAD; sleep 1.0
mark drag
"$ADB" -s $W shell input swipe $PAD $PAD 2500 &
sleep 1.2; turn -0.5 0.6
wait; sleep 1.0
mark rclick; tap $RIGHT; sleep 1.0
mark pause; tap $PAUSE; sleep 0.6
turn 0.5 1; sleep 0.4
mark resume; tap $PAUSE; sleep 1.5

# Sensor hole while paused: SensorService uid-idle drops gyro events while the
# hardware clock advances, creating a real >500 ms sensor-timestamp gap.
mark gap-pause; tap $PAUSE; sleep 0.6
if "$ADB" -s $W shell "cmd sensorservice set-uid-state dev.wearpointer idle" 2>/dev/null; then
  sleep 2
  "$ADB" -s $W shell "cmd sensorservice reset-uid-state dev.wearpointer"
  echo "injected 2 s sensor timestamp hole via sensorservice uid-idle"
else
  echo "could not set sensorservice uid-state; gap step skipped"
  mark gap-skipped
fi
sleep 0.6
mark gap-resume; tap $PAUSE; sleep 1.5

n0=$(count_trace " connected ")
d0=$(count_trace " disconnected ")
mark drop-drag
"$ADB" -s $W shell input swipe $PAD $PAD 4000 &
sleep 1.5
mark drop
"$ADB" -s $W shell am broadcast -a dev.wearpointer.DEBUG_DROP_LINK > /dev/null
wait
wait_more " disconnected " "$d0" 15 || { mark "drop-no-disconnect"; exit 1; }
wait_more " connected " "$n0" 40 || { mark "drop-no-reconnect"; exit 1; }
rx0=$(count_trace " rx ")
wait_more " rx " "$rx0" 15 || { mark "drop-no-rx"; exit 1; }
sleep 1.5
mark after-reconnect; tap $CENTER; sleep 0.6; turn 0.3 0.7; sleep 1.2

d0=$(count_trace " disconnected ")
mark stop; tap $STOP
wait_more " disconnected " "$d0" 15 || { mark "stop-no-disconnect"; exit 1; }
sleep 1.5

n0=$(count_trace " connected ")
rx0=$(count_trace " rx ")
mark start-again; tap $STOP
wait_more " connected " "$n0" 40 || { mark "restart-no-connect"; exit 1; }
wait_more " rx " "$rx0" 15 || { mark "restart-no-rx"; exit 1; }
sleep 2.0
mark end
