#!/usr/bin/env bash
# Runs inside the harness VM: stop what desktop-up.sh started for one run.
# Hyprland and the radio bridge stay up for the next run.
sleep 0.3
for name in receiver sampler wev; do
  f=/tmp/wp-$name.pid
  if [[ -f $f ]]; then
    kill "$(cat "$f")" 2>/dev/null || true
    rm -f "$f"
  fi
done
sleep 0.5
