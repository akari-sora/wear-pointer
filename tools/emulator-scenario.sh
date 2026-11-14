#!/bin/zsh
# Emulator scenario: watch (emulator-5560) -> probe central (emulator-5562).
# Drives gyro + watch UI. Replay the probe capture through the desktop session afterwards.
set -u
W=emulator-5560
P=emulator-5562
OUT=${1:-/tmp/wp-scenario}
mkdir -p $OUT
gyro() { adb -s $W emu sensor set gyroscope "$1" >/dev/null; }
tap() { adb -s $W shell input tap $1 $2; }
waitsub() {  # wait until the probe logs a new "subscribed"
  local n0=$(adb -s $P logcat -d -s wp-probe | grep -c subscribed)
  for i in {1..30}; do
    sleep 1
    [[ $(adb -s $P logcat -d -s wp-probe | grep -c subscribed) -gt $n0 ]] && return 0
  done
  echo "probe did not resubscribe"; return 1
}

gyro 0:0:0
adb -s $W logcat -c; adb -s $P logcat -c
adb -s $W shell am force-stop dev.wearpointer
adb -s $W shell am start -n dev.wearpointer/.MainActivity >/dev/null
waitsub; sleep 2

echo "1. turn left, Center mid-turn, keep turning"
gyro 0:0.5:0; sleep 1
tap 228 286          # Center
sleep 0.5; gyro 0:0:0; sleep 0.5

echo "2. tap = left click"
tap 227 164; sleep 1

echo "3. hold = drag, turning right while held"
adb -s $W shell input swipe 227 164 227 164 2500 &
sleep 1.5; gyro 0:-0.5:0; sleep 0.6; gyro 0:0:0
wait; sleep 1

echo "4. right click"
tap 84 286; sleep 1

echo "5. pause, turn while paused, resume"
tap 370 286; sleep 0.5
gyro 0:0.5:0; sleep 1; gyro 0:0:0; sleep 0.3
tap 370 286; sleep 1.5

echo "6. hold drag, then force the watch's link-drop path mid-drag"
adb -s $W shell input swipe 227 164 227 164 4000 &
sleep 2.0
adb -s $W shell am broadcast -a dev.wearpointer.DEBUG_DROP_LINK >/dev/null
wait
waitsub; sleep 1

echo "7. after reconnect: short turn"
gyro 0:0.3:0; sleep 0.7; gyro 0:0:0; sleep 0.7

adb -s $P logcat -d -s wp-probe > $OUT/probe.log
adb -s $W logcat -d -s wear-pointer > $OUT/watch.log
echo "captured $(grep -c ' rx ' $OUT/probe.log) notifications"
