#!/usr/bin/env bash
# Runs inside the VM as user harness: start Hyprland on the virtio-vga DRM device via seatd.
set -u
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export LIBSEAT_BACKEND=seatd
OUT=/mnt/share/out
mkdir -p "$OUT"
if pgrep -x Hyprland >/dev/null; then
  exit 0
fi
setsid Hyprland --config /mnt/share/hyprland.lua >"$OUT/hyprland.log" 2>&1 </dev/null &
for _ in $(seq 1 40); do
  sig=$(ls "$XDG_RUNTIME_DIR/hypr" 2>/dev/null | head -1)
  if [[ -n "$sig" ]] && HYPRLAND_INSTANCE_SIGNATURE=$sig hyprctl monitors >/dev/null 2>&1; then
    HYPRLAND_INSTANCE_SIGNATURE=$sig hyprctl monitors | head -2
    exit 0
  fi
  sleep 0.5
done
echo "Hyprland did not come up" >&2
tail -30 "$OUT/hyprland.log" >&2
exit 1
