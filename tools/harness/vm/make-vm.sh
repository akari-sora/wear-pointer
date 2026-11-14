#!/usr/bin/env bash
# Build the desktop VM once: Arch cloud image + cloud-init. It runs Hyprland
# on virtio-gpu (seatd, no logind seat needed), BlueZ, and the bumble bridge.
# Hyprland 0.57 cannot run nested or headless-only here (see
# docs/desktop-harness.md), so it gets a real KMS device inside a VM.
set -euo pipefail
VM=${VM_DIR:-$HOME/wp-harness/vm}
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$VM"
cd "$VM"
[[ -f id_ed25519 ]] || ssh-keygen -q -t ed25519 -N "" -f id_ed25519
if [[ ! -f arch-base.qcow2 ]]; then
  wget -q -O arch-base.qcow2.part https://archlinux.cs.nycu.edu.tw/images/latest/Arch-Linux-x86_64-cloudimg.qcow2
  mv arch-base.qcow2.part arch-base.qcow2
fi
rm -f desktop.qcow2
qemu-img create -q -f qcow2 -b arch-base.qcow2 -F qcow2 desktop.qcow2 20G
PUB=$(cat id_ed25519.pub)
mkdir -p seed
sed "s|@SSH_KEY@|$PUB|" "$HERE/user-data" > seed/user-data
printf 'instance-id: wp-desktop\nlocal-hostname: wp-desktop\n' > seed/meta-data
docker run --rm -v "$VM/seed:/seed" -w /seed alpine:3 sh -c \
  'apk add -q xorriso >/dev/null && xorriso -as mkisofs -quiet -o seed.iso -V cidata -J -r user-data meta-data'
"$HERE/start-vm.sh"
# cloud-init installs packages on first boot.
for _ in $(seq 1 120); do
  if "$HERE/vm-ssh.sh" 'test -f /var/lib/wp-provisioned' 2>/dev/null; then
    echo "VM provisioned"
    exit 0
  fi
  sleep 5
done
echo "VM provisioning timed out" >&2
exit 1
