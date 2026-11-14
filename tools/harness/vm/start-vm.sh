#!/usr/bin/env bash
# Start the desktop VM (idempotent). ssh on 127.0.0.1:2222.
set -euo pipefail
VM=${VM_DIR:-$HOME/wp-harness/vm}
HERE=$(cd "$(dirname "$0")" && pwd)
if [[ -f "$VM/qemu.pid" ]] && kill -0 "$(cat "$VM/qemu.pid")" 2>/dev/null; then
  exit 0
fi
mkdir -p "$VM/share"
qemu-system-x86_64 -enable-kvm -cpu host -smp 4 -m 4096 \
  -drive file="$VM/desktop.qcow2",if=virtio \
  -drive file="$VM/seed/seed.iso",media=cdrom \
  -device virtio-vga -display none \
  -nic user,model=virtio-net-pci,hostfwd=tcp:127.0.0.1:2222-:22 \
  -virtfs local,path="$VM/share",mount_tag=wpshare,security_model=none,id=wpshare \
  -daemonize -pidfile "$VM/qemu.pid" -serial file:"$VM/serial.log"
for _ in $(seq 1 60); do
  if "$HERE/vm-ssh.sh" true 2>/dev/null; then
    # Host directory shared at /mnt/share: binaries in, traces out.
    "$HERE/vm-ssh.sh" 'sudo loginctl enable-linger harness; sudo modprobe hci_vhci && sudo chmod 666 /dev/vhci; mountpoint -q /mnt/share || { sudo mkdir -p /mnt/share && sudo mount -t 9p -o trans=virtio,version=9p2000.L wpshare /mnt/share; }'
    exit 0
  fi
  sleep 3
done
echo "VM ssh did not come up" >&2
exit 1
