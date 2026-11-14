#!/usr/bin/env bash
# Create the Wear OS AVD used by the desktop harness (idempotent).
# Writes the AVD files directly: avdmanager hung on this host.
set -euo pipefail
export ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
TOOLS=${CMDLINE_TOOLS:-$HOME/wp-harness/cmdline-tools/bin}
IMAGE="system-images;android-35-ext15;android-wear;x86_64"
SYSDIR="system-images/android-35-ext15/android-wear/x86_64/"
if [[ ! -d "$ANDROID_HOME/$SYSDIR" ]]; then
  (set +o pipefail; yes | "$TOOLS/sdkmanager" --sdk_root="$ANDROID_HOME" "$IMAGE" emulator platform-tools)
fi
AVD="$HOME/.android/avd/wp_wear35.avd"
mkdir -p "$AVD"
cat > "$HOME/.android/avd/wp_wear35.ini" <<EOF
avd.ini.encoding=UTF-8
path=$AVD
path.rel=avd/wp_wear35.avd
target=android-35
EOF
cat > "$AVD/config.ini" <<EOF
AvdId=wp_wear35
PlayStore.enabled=false
abi.type=x86_64
avd.ini.displayname=wp_wear35
avd.ini.encoding=UTF-8
disk.dataPartition.size=2G
hw.accelerometer=yes
hw.gyroscope=yes
hw.sensors.orientation=yes
hw.audioInput=no
hw.battery=yes
hw.cpu.arch=x86_64
hw.cpu.ncore=4
hw.device.manufacturer=Google
hw.device.name=wearos_large_round
hw.gpu.enabled=yes
hw.gpu.mode=swiftshader_indirect
hw.keyboard=yes
hw.lcd.density=320
hw.lcd.height=454
hw.lcd.width=454
hw.lcd.circular=true
hw.mainKeys=yes
hw.ramSize=2048
hw.sdCard=no
image.sysdir.1=$SYSDIR
tag.display=Wear OS
tag.id=android-wear
EOF
ls "$HOME/.android/avd"
