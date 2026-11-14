#!/usr/bin/env python3
"""
60 Hz rigid-body wrist choreographer for wear-pointer synchronized dual capture.
Drives the Wear OS emulator's IMU (accelerometer + gyroscope) over the persistent
QEMU console TCP socket at 60 Hz using minimum-jerk human wrist trajectories
(s(u) = 10u^3 - 15u^4 + 6u^5) while triggering touch gestures via adb, and logs
exact timestamps of every beat to timeline.json.
"""
import json
import math
import os
import socket
import subprocess
import sys
import time

OUT_DIR = sys.argv[1]
ADB = os.path.expanduser("~/Android/Sdk/platform-tools/adb")
SERIAL = "emulator-5560"
K_PROJ = 960.0 / math.tan(math.radians(25.0))  # 2058.7266 px

# Watch UI coordinates (454x454)
PT_STATUS = (227, 52)
PT_PAD = (227, 177)
PT_RIGHT = (104, 324)
PT_CENTER = (227, 324)
PT_PAUSE = (350, 324)


def screen_to_angles_rad(tx: float, ty: float) -> tuple[float, float]:
    """Inverse of project_aim(left_deg, up_deg, Screen(1920, 1080), 25.0).
    Returns (heading_rad, elevation_rad) where heading_rad > 0 is left turn."""
    h = math.atan2(tx - 960.0, K_PROJ)
    heading = -h
    elevation = math.atan2((540.0 - ty) * math.cos(h), K_PROJ)
    return heading, elevation


def min_jerk(u: float) -> tuple[float, float]:
    """Returns (position_factor s(u), velocity_factor ds/du) for u in [0, 1]."""
    if u <= 0.0:
        return 0.0, 0.0
    if u >= 1.0:
        return 1.0, 0.0
    u2 = u * u
    u3 = u2 * u
    u4 = u3 * u
    u5 = u4 * u
    s = 10.0 * u3 - 15.0 * u4 + 6.0 * u5
    ds = 30.0 * u2 - 60.0 * u3 + 30.0 * u4
    return s, ds


class EmulatorConsole:
    def __init__(self, port: int = 5560) -> None:
        tok_path = os.path.expanduser("~/.emulator_console_auth_token")
        tok = open(tok_path).read().strip()
        self.sock = socket.create_connection(("127.0.0.1", port), timeout=5)
        self.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        time.sleep(0.08)
        self._drain()
        self.send_sync(f"auth {tok}")

    def _drain(self) -> None:
        self.sock.setblocking(False)
        try:
            while self.sock.recv(16384):
                pass
        except BlockingIOError:
            pass
        finally:
            self.sock.setblocking(True)

    def send_sync(self, cmd: str) -> None:
        self.sock.sendall((cmd + "\n").encode())
        time.sleep(0.05)
        self._drain()

    def set_imu(self, ax: float, ay: float, az: float, gx: float, gy: float, gz: float) -> None:
        msg = (
            f"sensor set acceleration {ax:.5f}:{ay:.5f}:{az:.5f}\n"
            f"sensor set gyroscope {gx:.5f}:{gy:.5f}:{gz:.5f}\n"
        )
        self.sock.sendall(msg.encode())
        self._drain()


def adb_bg(*args: str) -> subprocess.Popen:
    return subprocess.Popen(
        [ADB, "-s", SERIAL, "shell", *args],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def tap(pt: tuple[int, int]) -> subprocess.Popen:
    return adb_bg("input", "tap", str(pt[0]), str(pt[1]))


def hold_pad(duration_ms: int) -> subprocess.Popen:
    x, y = PT_PAD
    return adb_bg("input", "swipe", str(x), str(y), str(x), str(y), str(duration_ms))


def main() -> None:
    emu = EmulatorConsole(5560)
    # Settle flat wrist pose: gravity = (0, 0, +9.81), gyro = (0, 0, 0)
    for _ in range(90):
        emu.set_imu(0.0, 0.0, 9.81, 0.0, 0.0, 0.0)
        time.sleep(1.0 / 60.0)

    # Initial Center calibration before recording starts
    tap(PT_CENTER).wait()
    time.sleep(1.0)

    cur_h = 0.0
    cur_v = 0.0
    marks: dict[str, float] = {}
    t0 = time.time()

    def mark(name: str) -> None:
        dt = time.time() - t0
        marks[name] = round(dt, 4)
        print(f"[{dt:6.3f}s] {name}", flush=True)

    def hold_still(seconds: float) -> None:
        nonlocal cur_h, cur_v
        end = time.time() + seconds
        ay = 9.81 * math.sin(cur_v)
        az = 9.81 * math.cos(cur_v)
        while time.time() < end:
            emu.set_imu(0.0, ay, az, 0.0, 0.0, 0.0)
            time.sleep(1.0 / 60.0)

    def glide_to(tx: float, ty: float, duration: float) -> None:
        nonlocal cur_h, cur_v
        target_h, target_v = screen_to_angles_rad(tx, ty)
        start_h, start_v = cur_h, cur_v
        dh = target_h - start_h
        dv = target_v - start_v
        t_start = time.time()
        while True:
            now = time.time()
            u = (now - t_start) / duration
            if u >= 1.0:
                break
            s, ds = min_jerk(u)
            h = start_h + dh * s
            v = start_v + dv * s
            omega_h = dh * ds / duration
            omega_v = dv * ds / duration
            ay = 9.81 * math.sin(v)
            az = 9.81 * math.cos(v)
            # Rigid-body consistent angular velocity:
            # gx = dv/dt, (gy, gz) = omega_h * (sin v, cos v) so dot(gyro, up) == omega_h
            gx = omega_v
            gy = omega_h * math.sin(v)
            gz = omega_h * math.cos(v)
            emu.set_imu(0.0, ay, az, gx, gy, gz)
            cur_h, cur_v = h, v
            time.sleep(1.0 / 60.0)
        cur_h, cur_v = target_h, target_v
        emu.set_imu(0.0, 9.81 * math.sin(cur_v), 9.81 * math.cos(cur_v), 0.0, 0.0, 0.0)

    mark("take_start")
    glide_to(380, 540, 0.55)
    hold_still(0.35)

    # Clapperboard sync event: Center tap snaps BOTH Watch puck & Desktop cursor to (0,0)/(960,540) in 1 frame!
    mark("clapper_center")
    tap(PT_CENTER)
    cur_h, cur_v = 0.0, 0.0
    hold_still(1.2)

    # ── SCENE 1: 100 Hz 2D Laser Aim & Click across 3 Hyprland cards ────────
    mark("scene1_start")
    glide_to(258, 250, 1.05)
    hold_still(0.28)
    mark("click_card1")
    tap(PT_PAD)
    hold_still(0.42)

    glide_to(638, 250, 0.95)
    hold_still(0.28)
    mark("click_card2")
    tap(PT_PAD)
    hold_still(0.42)

    glide_to(1018, 250, 0.95)
    hold_still(0.28)
    mark("click_card3")
    tap(PT_PAD)
    hold_still(0.45)

    glide_to(276, 680, 1.15)
    hold_still(0.75)
    mark("scene1_end")

    # ── SCENE 2: Hold 350 ms Drag Lock -> Drag Tile to Drop Slot + Right Click
    mark("scene2_start")
    hold_proc = hold_pad(3100)
    # Wait 0.70s so the 350ms radial progress fills and DRAG LOCKED triggers cleanly
    hold_still(0.70)
    mark("drag_locked_move")
    glide_to(958, 680, 1.75)
    hold_still(0.75)
    hold_proc.wait()
    mark("drag_released")
    hold_still(0.60)

    # Glide slightly up-left and Right-Click to pop context menu
    glide_to(640, 480, 0.85)
    hold_still(0.28)
    mark("right_click")
    tap(PT_RIGHT)
    hold_still(0.95)
    mark("scene2_end")

    # ── SCENE 3: Clutch Pause (wrist moves, cursor stays frozen!) & Recenter
    mark("scene3_start")
    tap(PT_PAUSE)
    hold_still(0.55)
    mark("paused_wrist_wiggle")
    # Move wrist noticeably left/right/up/down while paused — cursor on Hyprland MUST stay frozen!
    glide_to(260, 300, 0.85)
    glide_to(1080, 760, 0.95)
    hold_still(0.35)
    mark("resume_tap")
    tap(PT_PAUSE)
    hold_still(0.55)
    mark("recenter_tap")
    # Reset physical elevation back to level and tap Center -> snaps dead bullseye (960, 540)!
    glide_to(960, 540, 0.45)
    tap(PT_CENTER)
    cur_h, cur_v = 0.0, 0.0
    hold_still(1.15)
    mark("scene3_end")

    # ── SCENE 4: Live 100 Hz Telemetry Overlay + Smooth 2D Orbit ─────────────
    mark("scene4_start")
    tap(PT_STATUS)
    hold_still(0.65)
    # Orbit smoothly so AIM L/U degrees update live inside TelemetryOverlay while desktop cursor orbits!
    glide_to(560, 340, 1.15)
    glide_to(1120, 380, 1.25)
    glide_to(820, 680, 1.15)
    glide_to(960, 540, 0.95)
    hold_still(0.65)
    mark("scene4_end")

    with open(os.path.join(OUT_DIR, "timeline.json"), "w") as fp:
        json.dump({"t0_unix": t0, "marks": marks}, fp, indent=2)
    print("CHOREOGRAPHY COMPLETE!")


if __name__ == "__main__":
    main()
