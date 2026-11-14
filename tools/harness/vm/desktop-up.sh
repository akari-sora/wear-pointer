#!/usr/bin/env bash
# Runs inside the harness VM as user harness. Brings up the desktop side of
# one harness run and leaves it running:
#   Hyprland (DRM on virtio-vga, seatd) -> fullscreen wev (client-side pointer log)
#   netsim HCI port -> bridge.py -> /dev/vhci -> hciN -> BlueZ
#   wear-pointer --backend hyprland on that adapter, with --trace
#   cursor-sampler.py: Hyprland's own cursor position every 10 ms
# Usage: desktop-up.sh RUN_NAME NETSIM_HCI_PORT
set -euo pipefail
NAME=$1
PORT=$2
S=/mnt/share
OUT=$S/out/$NAME
mkdir -p "$OUT"
export XDG_RUNTIME_DIR=/run/user/$(id -u)

bash "$S/desktop-down.sh" >/dev/null 2>&1 || true

bash "$S/hypr-start.sh" >/dev/null
export HYPRLAND_INSTANCE_SIGNATURE=$(ls "$XDG_RUNTIME_DIR/hypr" | head -1)
export WAYLAND_DISPLAY=$(cd "$XDG_RUNTIME_DIR" && ls -d wayland-[0-9]* | grep -v lock | sort | tail -1)
hyprctl monitors > "$OUT/monitors.txt"

# Radio: one bridge per netsim instance. Restart it when the adapter is gone,
# the bridge PID died, or netsim's HCI port changed.
adapter() { ls /sys/class/bluetooth 2>/dev/null | grep -E '^hci[0-9]+$' | head -1 || true; }
prev_port=$(cat /tmp/wp-bridge.port 2>/dev/null || true)
bridge_alive=0
if [[ -f /tmp/wp-bridge.pid ]] && kill -0 "$(cat /tmp/wp-bridge.pid)" 2>/dev/null; then
  bridge_alive=1
fi
if [[ -z "$(adapter)" || "$bridge_alive" -ne 1 || "$prev_port" != "$PORT" ]]; then
  [[ -f /tmp/wp-bridge.pid ]] && kill "$(cat /tmp/wp-bridge.pid)" 2>/dev/null || true
  for p in $(pgrep -f '/mnt/share/bridge\.py' || true); do
    kill "$p" 2>/dev/null || true
  done
  for _ in $(seq 1 15); do
    [[ -z "$(adapter)" ]] && break
    sleep 0.2
  done
  setsid "$HOME/bumble/bin/python" "$S/bridge.py" "tcp-client:10.0.2.2:$PORT" \
    > "$OUT/bridge.log" 2>&1 < /dev/null &
  echo $! > /tmp/wp-bridge.pid
  echo "$PORT" > /tmp/wp-bridge.port
fi
HCI=""
for _ in $(seq 1 50); do
  HCI=$(adapter)
  [[ -n "$HCI" ]] && break
  sleep 0.2
done
[[ -n "$HCI" ]] || { echo "no virtual adapter in the VM" >&2; cat "$OUT/bridge.log" >&2 || true; exit 1; }
# bluetooth.service has ConditionPathIsDirectory=/sys/class/bluetooth, which is
# false on boot until /dev/vhci creates hci0. Ensure bluetoothd is running now.
sudo systemctl start bluetooth
sleep 1
bluetoothctl power on > /dev/null 2>&1 || true
echo "$HCI" > "$OUT/adapter.txt"

# Client: wev tiles to the whole monitor (gaps and borders are 0), so every
# pointer event Hyprland routes lands in its log.
setsid stdbuf -oL wev > "$OUT/wev.log" 2>&1 < /dev/null &
echo $! > /tmp/wp-wev.pid
for _ in $(seq 1 30); do
  hyprctl clients | grep -q 'class: wev' && break
  sleep 0.2
done

setsid python3 "$S/cursor-sampler.py" "$OUT/cursor.log" > /dev/null 2>&1 < /dev/null &
echo $! > /tmp/wp-sampler.pid

setsid "$S/wear-pointer" --backend hyprland --adapter "$HCI" --debug \
  --trace "$OUT/trace.log" > "$OUT/receiver.log" 2>&1 < /dev/null &
echo $! > /tmp/wp-receiver.pid
sleep 1
kill -0 "$(cat /tmp/wp-receiver.pid)" || { echo "receiver exited" >&2; cat "$OUT/receiver.log" >&2; exit 1; }
echo "desktop up: adapter $HCI, $WAYLAND_DISPLAY, out $OUT"
