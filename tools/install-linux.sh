#!/usr/bin/env bash
# Installs (or uninstalls) Wear Pointer for Linux desktop users:
# - ~/.local/bin/wear-pointer       (developer / terminal CLI)
# - ~/.local/bin/wear-pointer-app   (system-tray / background receiver)
# - ~/.local/share/applications/wear-pointer.desktop
# - ~/.config/systemd/user/wear-pointer.service
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="${HOME}/.local/bin"
APP_DIR="${HOME}/.local/share/applications"
ICON_DIR="${HOME}/.local/share/icons/hicolor/64x64/apps"
UNIT_DIR="${HOME}/.config/systemd/user"

if [[ "${1:-}" == "--uninstall" ]]; then
    if command -v systemctl >/dev/null 2>&1; then
        systemctl --user disable --now wear-pointer.service 2>/dev/null || true
        systemctl --user daemon-reload 2>/dev/null || true
    fi
    rm -f \
        "${BIN_DIR}/wear-pointer" \
        "${BIN_DIR}/wear-pointer-app" \
        "${APP_DIR}/wear-pointer.desktop" \
        "${ICON_DIR}/wear-pointer.png" \
        "${UNIT_DIR}/wear-pointer.service" \
        "${HOME}/.config/autostart/wear-pointer.desktop"
    echo "Wear Pointer uninstalled from ~/.local."
    exit 0
fi

find_file() {
    local name="$1"
    for candidate in \
        "${SCRIPT_DIR}/${name}" \
        "${SCRIPT_DIR}/../receiver/target/release/${name}" \
        "${SCRIPT_DIR}/../receiver/crates/wear-pointer/assets/${name}"; do
        if [[ -f "$candidate" ]]; then
            echo "$candidate"
            return 0
        fi
    done
    return 1
}

CLI_SRC="$(find_file wear-pointer)" || {
    echo "Error: wear-pointer binary not found next to install-linux.sh" >&2
    exit 1
}
APP_SRC="$(find_file wear-pointer-app || echo "$CLI_SRC")"

mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_DIR" "$UNIT_DIR"
install -m 0755 "$CLI_SRC" "${BIN_DIR}/wear-pointer"
install -m 0755 "$APP_SRC" "${BIN_DIR}/wear-pointer-app"

if ICON_SRC="$(find_file icon-64.png 2>/dev/null || find_file wear-pointer.png 2>/dev/null)"; then
    install -m 0644 "$ICON_SRC" "${ICON_DIR}/wear-pointer.png"
fi

cat > "${APP_DIR}/wear-pointer.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Wear Pointer
Comment=Turn your Wear OS watch into a spatial laser, air mouse, or touchpad
Exec=${BIN_DIR}/wear-pointer-app
Icon=wear-pointer
Terminal=false
Categories=Utility;HardwareSettings;
StartupNotify=false
EOF

cat > "${UNIT_DIR}/wear-pointer.service" <<EOF
[Unit]
Description=Wear Pointer Bluetooth LE Spatial Receiver
After=graphical-session.target bluetooth.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=${BIN_DIR}/wear-pointer-app
Restart=on-failure
RestartSec=3

[Install]
WantedBy=graphical-session.target
EOF

if command -v systemctl >/dev/null 2>&1 && systemctl --user status >/dev/null 2>&1; then
    systemctl --user daemon-reload
    systemctl --user enable --now wear-pointer.service
    echo "Wear Pointer installed and started via systemd --user (wear-pointer.service)."
else
    echo "Wear Pointer installed to ${BIN_DIR}/wear-pointer-app (launch from your app launcher or run wear-pointer-app)."
fi
echo "Developer CLI remains available at: ${BIN_DIR}/wear-pointer --debug"
