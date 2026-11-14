#!/usr/bin/env bash
# Run a command in the desktop VM.
VM=${VM_DIR:-$HOME/wp-harness/vm}
exec ssh -q -i "$VM/id_ed25519" -p 2222 -o StrictHostKeyChecking=no \
  -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 -o BatchMode=yes harness@127.0.0.1 "$@"
