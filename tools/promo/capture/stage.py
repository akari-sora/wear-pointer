#!/usr/bin/env python3
"""
Interactive 1920x1080 Wayland Pointer Stage for wear-pointer live demo capture.
Runs natively as a tiled Hyprland Wayland window and responds to REAL wl_pointer
motion, left-click, hold-drag-drop, and right-click events emitted by
`wear-pointer --backend hyprland`, while decoding real 12-byte BLE GATT packets
from trace.log in real time on the right pane.
"""
import math
import os
import struct
import sys
import time
from collections import deque

import cairo
import gi

gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk, GLib, Gtk  # noqa: E402

TRACE_PATH = sys.argv[1] if len(sys.argv) > 1 else "/mnt/share/out/promo/trace.log"
STAGE_STATE_PATH = sys.argv[2] if len(sys.argv) > 2 else "/mnt/share/out/promo/stage_events.log"

W, H = 1920, 1080

KIND_AIM = 0
KIND_LEFT_CLICK = 1
KIND_RIGHT_CLICK = 2
KIND_SCROLL = 3
KIND_CENTER = 4
KIND_PAUSE = 5
KIND_INFO = 6
KIND_DRAG = 7
KIND_STATS = 8


def rounded_rect(cr: cairo.Context, x: float, y: float, w: float, h: float, r: float) -> None:
    r = min(r, w / 2.0, h / 2.0)
    cr.new_sub_path()
    cr.arc(x + w - r, y + r, r, -math.pi / 2, 0)
    cr.arc(x + w - r, y + h - r, r, 0, math.pi / 2)
    cr.arc(x + r, y + h - r, r, math.pi / 2, math.pi)
    cr.arc(x + r, y + r, r, math.pi, 3 * math.pi / 2)
    cr.close_path()


class PointerStage(Gtk.Window):
    def __init__(self) -> None:
        super().__init__(title="wear-pointer-stage")
        self.set_default_size(W, H)
        self.set_decorated(False)
        # Do NOT call self.fullscreen() so Hyprland never attempts DRM direct scanout;
        # Hyprland tiles a single client cleanly to 1920x1080 with gaps=0, border=0.

        self.area = Gtk.DrawingArea()
        self.area.set_size_request(W, H)
        self.area.add_events(
            Gdk.EventMask.POINTER_MOTION_MASK
            | Gdk.EventMask.BUTTON_PRESS_MASK
            | Gdk.EventMask.BUTTON_RELEASE_MASK
            | Gdk.EventMask.SCROLL_MASK
        )
        self.area.connect("draw", self.on_draw)
        self.area.connect("motion-notify-event", self.on_motion)
        self.area.connect("button-press-event", self.on_button_press)
        self.area.connect("button-release-event", self.on_button_release)
        self.add(self.area)

        self.cx = 960.0
        self.cy = 540.0
        self.left_down = False
        self.right_down = False
        self.draw_count = 0
        self.trail: deque[tuple[float, float, float]] = deque(maxlen=42)
        self.ripples: list[tuple[float, float, float, str]] = []

        # Interactive target cards in Left Tiled Pane (x=24..1268, y=70..1056)
        self.cards = [
            {"id": "term", "label": "01 · Launch Terminal", "sub": "hyprctl dispatch exec foot", "x": 88, "y": 176, "w": 340, "h": 164, "clicks": 0, "active_ts": 0.0},
            {"id": "ws", "label": "02 · Focus Workspace 2", "sub": "zwlr_virtual_pointer_v1", "x": 468, "y": 176, "w": 340, "h": 164, "clicks": 0, "active_ts": 0.0},
            {"id": "media", "label": "03 · Play / Media Deck", "sub": "BLE 100 Hz Low-Latency", "x": 848, "y": 176, "w": 340, "h": 164, "clicks": 0, "active_ts": 0.0},
        ]

        # Draggable tile + Drop target slot (positioned below the (960, 540) origin crosshair)
        self.drag_box = {"x": 116.0, "y": 680.0, "w": 320.0, "h": 184.0}
        self.dragging = False
        self.drag_off = (0.0, 0.0)
        self.drop_zone = {"x": 750.0, "y": 640.0, "w": 416.0, "h": 252.0}
        self.dropped = False
        self.drop_ts = 0.0

        # Context menu opened on real right-click
        self.ctx_menu: dict | None = None

        # Center bullseye at (960, 540) & Clutch pause state
        self.last_center_ts = 0.0
        self.last_pause_state = False

        # Live trace reader state
        self.trace_fp = None
        self.trace_lines: deque[tuple[str, str]] = deque(maxlen=20)
        self.rx_timestamps: deque[float] = deque(maxlen=240)
        self.pkt_total = 0
        self.connected = False
        self.last_kind = "0x00 AIM"

        self.evt_fp = open(STAGE_STATE_PATH, "a", buffering=1)
        GLib.timeout_add(30, self.tick)

    def log_evt(self, msg: str) -> None:
        self.evt_fp.write(f"{time.time():.4f} {msg}\n")

    def poll_trace(self) -> None:
        if self.trace_fp is None:
            if os.path.exists(TRACE_PATH):
                self.trace_fp = open(TRACE_PATH, "r", encoding="utf-8", errors="replace")
            else:
                return
        now = time.time()
        while True:
            try:
                line = self.trace_fp.readline()
            except OSError:
                break
            if not line:
                break
            parts = line.strip().split()
            if len(parts) < 2 or parts[0].startswith("#"):
                continue
            tag = parts[1]
            if tag == "connected":
                self.connected = True
                self.trace_lines.append(("link", "LINK  GATT notify subscribed"))
            elif tag == "disconnected":
                self.connected = False
                self.trace_lines.append(("warn", "LINK  disconnected"))
            elif tag == "rx" and len(parts) >= 4:
                self.pkt_total += 1
                self.rx_timestamps.append(now)
                try:
                    raw = bytes.fromhex(parts[3])
                    if len(raw) >= 8:
                        _ver, kind, seq, _ts = struct.unpack_from("<BBHI", raw, 0)
                        if kind == KIND_AIM and len(raw) >= 12:
                            lc, uc = struct.unpack_from("<hh", raw, 8)
                            self.last_kind = "0x00 AIM"
                            if self.pkt_total % 14 == 0:
                                self.trace_lines.append(
                                    ("aim", f"#{seq:04d} AIM     L={lc/100:+5.1f}° U={uc/100:+5.1f}°")
                                )
                        elif kind == KIND_LEFT_CLICK:
                            self.last_kind = "0x01 CLICK"
                            self.trace_lines.append(("click", f"#{seq:04d} L_CLICK button=LEFT tap"))
                        elif kind == KIND_RIGHT_CLICK:
                            self.last_kind = "0x02 RCLICK"
                            self.trace_lines.append(("rclick", f"#{seq:04d} R_CLICK button=RIGHT"))
                        elif kind == KIND_DRAG and len(raw) >= 9:
                            down = raw[8] != 0
                            self.last_kind = "0x07 DRAG"
                            state_s = "LOCKED (DOWN)" if down else "RELEASED (UP)"
                            self.trace_lines.append(("drag", f"#{seq:04d} DRAG    {state_s}"))
                        elif kind == KIND_CENTER:
                            self.last_center_ts = now
                            self.last_kind = "0x04 CENTER"
                            self.trace_lines.append(("center", f"#{seq:04d} CENTER  origin=(960, 540)"))
                        elif kind == KIND_PAUSE and len(raw) >= 9:
                            self.last_pause_state = raw[8] != 0
                            self.last_kind = "0x05 PAUSE"
                            p_s = "CLUTCH ON (FROZEN)" if self.last_pause_state else "RESUMED (LIVE)"
                            self.trace_lines.append(("pause", f"#{seq:04d} PAUSE   {p_s}"))
                        elif kind == KIND_STATS and len(raw) >= 16:
                            smp, nok, ndrop, win_ms = struct.unpack_from("<HHHH", raw, 8)
                            imu_hz = (smp * 1000.0 / win_ms) if win_ms > 0 else 0.0
                            tx_hz = (nok * 1000.0 / win_ms) if win_ms > 0 else 0.0
                            self.trace_lines.append(
                                ("stats", f"#{seq:04d} STATS   imu={imu_hz:.0f}Hz tx={tx_hz:.0f}/s drop={ndrop}")
                            )
                except Exception:
                    pass

        while self.rx_timestamps and now - self.rx_timestamps[0] > 1.0:
            self.rx_timestamps.popleft()

    def tick(self) -> bool:
        try:
            self.poll_trace()
            gdk_win = self.area.get_window()
            if gdk_win is not None:
                gdk_win.invalidate_rect(None, False)
            self.area.queue_draw()
        except Exception as exc:
            self.log_evt(f"TICK_ERR {exc!r}")
        return True

    def on_motion(self, _w: Gtk.Widget, ev: Gdk.EventMotion) -> bool:
        self.cx, self.cy = ev.x, ev.y
        now = time.time()
        self.trail.append((self.cx, self.cy, now))
        if self.dragging:
            self.drag_box["x"] = max(56.0, min(1220.0 - self.drag_box["w"], self.cx - self.drag_off[0]))
            self.drag_box["y"] = max(360.0, min(1010.0 - self.drag_box["h"], self.cy - self.drag_off[1]))
            bx = self.drag_box["x"] + self.drag_box["w"] / 2
            by = self.drag_box["y"] + self.drag_box["h"] / 2
            dz = self.drop_zone
            self.dropped = (dz["x"] <= bx <= dz["x"] + dz["w"]) and (dz["y"] <= by <= dz["y"] + dz["h"])
        return True

    def on_button_press(self, _w: Gtk.Widget, ev: Gdk.EventButton) -> bool:
        now = time.time()
        self.cx, self.cy = ev.x, ev.y
        if ev.button == 1:
            self.left_down = True
            self.ctx_menu = None
            self.ripples.append((self.cx, self.cy, now, "left"))
            self.log_evt(f"BTN_LEFT_DOWN {self.cx:.1f} {self.cy:.1f} draws={self.draw_count}")
            for c in self.cards:
                if c["x"] <= self.cx <= c["x"] + c["w"] and c["y"] <= self.cy <= c["y"] + c["h"]:
                    c["clicks"] += 1
                    c["active_ts"] = now
                    self.log_evt(f"CARD_CLICK {c['id']}")
            db = self.drag_box
            if db["x"] <= self.cx <= db["x"] + db["w"] and db["y"] <= self.cy <= db["y"] + db["h"]:
                self.dragging = True
                self.drag_off = (self.cx - db["x"], self.cy - db["y"])
                self.log_evt("DRAG_GRAB")
        elif ev.button == 3:
            self.right_down = True
            self.ripples.append((self.cx, self.cy, now, "right"))
            self.ctx_menu = {"x": min(self.cx, 960.0), "y": min(self.cy, 800.0), "ts": now}
            self.log_evt(f"BTN_RIGHT_DOWN {self.cx:.1f} {self.cy:.1f} draws={self.draw_count}")
        return True

    def on_button_release(self, _w: Gtk.Widget, ev: Gdk.EventButton) -> bool:
        now = time.time()
        if ev.button == 1:
            self.left_down = False
            if self.dragging:
                self.dragging = False
                bx = self.drag_box["x"] + self.drag_box["w"] / 2
                by = self.drag_box["y"] + self.drag_box["h"] / 2
                dz = self.drop_zone
                if dz["x"] <= bx <= dz["x"] + dz["w"] and dz["y"] <= by <= dz["y"] + dz["h"]:
                    self.dropped = True
                    self.drop_ts = now
                    self.drag_box["x"] = dz["x"] + (dz["w"] - self.drag_box["w"]) / 2
                    self.drag_box["y"] = dz["y"] + 50.0
                    self.log_evt("DRAG_DROPPED_IN_ZONE")
            self.log_evt(f"BTN_LEFT_UP {self.cx:.1f} {self.cy:.1f} draws={self.draw_count}")
        elif ev.button == 3:
            self.right_down = False
        return True

    def on_draw(self, _w: Gtk.Widget, cr: cairo.Context) -> bool:
        self.draw_count += 1
        now = time.time()

        # Wallpaper background
        pat = cairo.LinearGradient(0, 0, W, H)
        pat.add_color_stop_rgb(0.0, 0.031, 0.043, 0.078)
        pat.add_color_stop_rgb(1.0, 0.047, 0.067, 0.118)
        cr.set_source(pat)
        cr.rectangle(0, 0, W, H)
        cr.fill()

        # ── Waybar Top Bar ──────────────────────────────────────────────────
        rounded_rect(cr, 24, 14, W - 48, 42, 12)
        cr.set_source_rgba(0.067, 0.094, 0.153, 0.94)
        cr.fill_preserve()
        cr.set_source_rgba(0.16, 0.23, 0.34, 0.8)
        cr.set_line_width(1.5)
        cr.stroke()

        cr.select_font_face("JetBrains Mono", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD)
        cr.set_font_size(16)
        cr.set_source_rgb(0.063, 0.725, 0.506)
        cr.move_to(44, 41)
        cr.show_text("◆ Hyprland 0.57 · Wayland")

        for idx, ws in enumerate(["1: stage", "2: code", "3: term"]):
            wx = 330 + idx * 116
            rounded_rect(cr, wx, 21, 104, 28, 8)
            if idx == 0:
                cr.set_source_rgba(0.063, 0.725, 0.506, 0.22)
            else:
                cr.set_source_rgba(1, 1, 1, 0.05)
            cr.fill()
            cr.set_font_size(14)
            cr.set_source_rgb(0.85, 0.95, 0.90) if idx == 0 else cr.set_source_rgb(0.55, 0.62, 0.72)
            cr.move_to(wx + 16, 40)
            cr.show_text(ws)

        hz = len(self.rx_timestamps)
        status_str = f"BLE GATT {'LINKED' if self.connected else 'SCANNING'} · {hz:3d} pkt/s · ({int(self.cx):4d}, {int(self.cy):4d})"
        cr.set_font_size(15)
        cr.set_source_rgb(0.38, 0.85, 0.98)
        cr.move_to(1365, 40)
        cr.show_text(status_str)

        # ── Left Tiled Window: Interactive Wayland Pointer Stage ────────────
        lx, ly, lw, lh = 24, 70, 1244, 986
        rounded_rect(cr, lx, ly, lw, lh, 18)
        cr.set_source_rgba(0.043, 0.059, 0.098, 0.96)
        cr.fill_preserve()
        bpat = cairo.LinearGradient(lx, ly, lx + lw, ly + lh)
        bpat.add_color_stop_rgb(0.0, 0.063, 0.725, 0.506)
        bpat.add_color_stop_rgb(1.0, 0.220, 0.741, 0.973)
        cr.set_source(bpat)
        cr.set_line_width(2.4)
        cr.stroke()

        cr.set_source_rgba(1, 1, 1, 0.032)
        cr.set_line_width(1.0)
        for gx in range(lx + 48, lx + lw, 48):
            cr.move_to(gx, ly + 56)
            cr.line_to(gx, ly + lh - 16)
        for gy in range(ly + 64, ly + lh, 48):
            cr.move_to(lx + 16, gy)
            cr.line_to(lx + lw - 16, gy)
        cr.stroke()

        cr.set_font_size(18)
        cr.set_source_rgb(0.92, 0.96, 1.0)
        cr.move_to(lx + 28, ly + 38)
        cr.show_text("Wayland Native Virtual Pointer Stage  —  zwlr_virtual_pointer_v1")

        # Center bullseye marker at (960, 540)
        center_glow = max(0.0, 1.0 - (now - self.last_center_ts) / 1.6)
        cr.set_source_rgba(0.22, 0.74, 0.97, 0.26 + 0.72 * center_glow)
        cr.set_line_width(2.0 + 3.0 * center_glow)
        cr.new_sub_path()
        cr.arc(960, 540, 28 + 18 * center_glow, 0, 2 * math.pi)
        cr.stroke()
        cr.move_to(960 - 44, 540)
        cr.line_to(960 + 44, 540)
        cr.move_to(960, 540 - 44)
        cr.line_to(960, 540 + 44)
        cr.stroke()
        cr.set_font_size(14)
        cr.set_source_rgba(0.56, 0.88, 1.0, 0.80 + 0.20 * center_glow)
        cr.move_to(892, 598)
        cr.show_text("✓ RECENTERED (960, 540)" if center_glow > 0.1 else "ORIGIN (960, 540)")

        # Interactive target cards
        for c in self.cards:
            hovered = c["x"] <= self.cx <= c["x"] + c["w"] and c["y"] <= self.cy <= c["y"] + c["h"]
            flash = max(0.0, 1.0 - (now - c["active_ts"]) / 1.4)
            rounded_rect(cr, c["x"], c["y"], c["w"], c["h"], 16)
            if c["clicks"] > 0:
                cr.set_source_rgba(0.055, 0.22, 0.16, 0.85 + 0.12 * flash)
            elif hovered:
                cr.set_source_rgba(0.14, 0.22, 0.34, 0.90)
            else:
                cr.set_source_rgba(0.082, 0.114, 0.180, 0.88)
            cr.fill_preserve()

            if c["clicks"] > 0 or hovered:
                cr.set_source_rgba(0.063, 0.725, 0.506, 1.0 if c["clicks"] > 0 else 0.75)
                cr.set_line_width(2.8 if c["clicks"] > 0 else 2.2)
            else:
                cr.set_source_rgba(1, 1, 1, 0.14)
                cr.set_line_width(1.5)
            cr.stroke()

            cr.set_font_size(19)
            cr.set_source_rgb(0.97, 0.99, 1.0)
            cr.move_to(c["x"] + 22, c["y"] + 46)
            cr.show_text(c["label"])

            cr.set_font_size(14)
            cr.set_source_rgb(0.62, 0.72, 0.84)
            cr.move_to(c["x"] + 22, c["y"] + 78)
            cr.show_text(c["sub"])

            rounded_rect(cr, c["x"] + 22, c["y"] + 106, 178, 36, 99)
            if c["clicks"] > 0:
                cr.set_source_rgba(0.063, 0.725, 0.506, 0.34)
                cr.fill()
                cr.set_source_rgb(0.28, 0.98, 0.72)
                cr.set_font_size(15)
                cr.move_to(c["x"] + 38, c["y"] + 130)
                cr.show_text(f"✓ ACTIVATED ×{c['clicks']}")
            else:
                cr.set_source_rgba(1, 1, 1, 0.07)
                cr.fill()
                cr.set_source_rgb(0.65, 0.72, 0.82)
                cr.set_font_size(14)
                cr.move_to(c["x"] + 38, c["y"] + 129)
                cr.show_text("TAP TO ACTIVATE")

        # Drop Zone
        dz = self.drop_zone
        rounded_rect(cr, dz["x"], dz["y"], dz["w"], dz["h"], 18)
        if self.dropped:
            cr.set_source_rgba(0.063, 0.725, 0.506, 0.18)
            cr.fill_preserve()
            cr.set_source_rgba(0.063, 0.725, 0.506, 0.96)
            cr.set_line_width(3.0)
            cr.stroke()
        else:
            cr.set_source_rgba(0.07, 0.12, 0.20, 0.55)
            cr.fill_preserve()
            cr.set_dash([10.0, 8.0])
            cr.set_source_rgba(0.22, 0.74, 0.97, 0.70)
            cr.set_line_width(2.2)
            cr.stroke()
            cr.set_dash([])

        cr.set_font_size(17)
        cr.set_source_rgb(0.24, 0.96, 0.70) if self.dropped else cr.set_source_rgb(0.45, 0.78, 0.96)
        cr.move_to(dz["x"] + 26, dz["y"] + 36)
        cr.show_text("✓ DROP SLOT LOCKED" if self.dropped else "⇢ TARGET DROP ZONE")

        # Draggable Card
        db = self.drag_box
        rounded_rect(cr, db["x"], db["y"], db["w"], db["h"], 16)
        if self.dragging:
            cr.set_source_rgba(0.04, 0.28, 0.20, 0.97)
        elif self.dropped:
            cr.set_source_rgba(0.05, 0.23, 0.17, 0.96)
        else:
            cr.set_source_rgba(0.11, 0.15, 0.24, 0.95)
        cr.fill_preserve()
        cr.set_source_rgba(0.063, 0.725, 0.506, 1.0) if (self.dragging or self.dropped) else cr.set_source_rgba(0.38, 0.85, 0.98, 0.85)
        cr.set_line_width(3.2 if self.dragging else 2.2)
        cr.stroke()

        cr.set_font_size(20)
        cr.set_source_rgb(0.98, 0.99, 1.0)
        cr.move_to(db["x"] + 22, db["y"] + 56)
        cr.show_text("Window Tile #4")
        cr.set_font_size(15)
        cr.set_source_rgb(0.28, 0.98, 0.72) if (self.dragging or self.dropped) else cr.set_source_rgb(0.68, 0.76, 0.88)
        cr.move_to(db["x"] + 22, db["y"] + 94)
        cr.show_text("● DRAG LOCKED — MOVING" if self.dragging else ("✓ DROPPED IN TARGET" if self.dropped else "Hold 350 ms on Watch"))
        cr.set_font_size(14)
        cr.set_source_rgba(0.78, 0.86, 0.96, 0.8)
        cr.move_to(db["x"] + 22, db["y"] + 134)
        cr.show_text(f"pos = ({int(db['x'])}, {int(db['y'])})")

        # Context Menu on Right Click (hide during pause so scene 3 banner shines cleanly)
        if self.ctx_menu is not None and not self.last_pause_state and (now - self.ctx_menu["ts"] < 4.0):
            mx, my = self.ctx_menu["x"], self.ctx_menu["y"]
            rounded_rect(cr, mx, my, 276, 180, 14)
            cr.set_source_rgba(0.06, 0.09, 0.16, 0.98)
            cr.fill_preserve()
            cr.set_source_rgba(0.66, 0.55, 0.98, 0.96)
            cr.set_line_width(2.4)
            cr.stroke()
            items = ["✓ Right-Click Context Menu", "  Move to Workspace 2", "  Toggle Floating Tile", "  Inspect GATT Stream"]
            for idx, it in enumerate(items):
                if idx == 0:
                    rounded_rect(cr, mx + 10, my + 12, 256, 36, 8)
                    cr.set_source_rgba(0.66, 0.55, 0.98, 0.28)
                    cr.fill()
                cr.set_font_size(15)
                cr.set_source_rgb(0.96, 0.97, 1.0) if idx == 0 else cr.set_source_rgb(0.74, 0.80, 0.90)
                cr.move_to(mx + 18, my + 36 + idx * 38)
                cr.show_text(it)

        # Clutch / Pause banner when watch enters PAUSED state
        if self.last_pause_state:
            rounded_rect(cr, 320, 405, 620, 82, 18)
            cr.set_source_rgba(0.24, 0.15, 0.02, 0.96)
            cr.fill_preserve()
            cr.set_source_rgba(0.96, 0.62, 0.04, 0.98)
            cr.set_line_width(2.8)
            cr.stroke()
            cr.set_source_rgb(1.0, 0.80, 0.24)
            rounded_rect(cr, 354, 433, 7, 26, 2)
            cr.fill()
            rounded_rect(cr, 367, 433, 7, 26, 2)
            cr.fill()
            cr.set_font_size(23)
            cr.move_to(388, 454)
            cr.show_text("CLUTCH PAUSED · CURSOR FROZEN")

        # ── Right Tiled Window: Live Rust Receiver & BLE Packet Stream ──────
        rx, ry, rw, rh = 1286, 70, 610, 986
        rounded_rect(cr, rx, ry, rw, rh, 18)
        cr.set_source_rgba(0.031, 0.043, 0.071, 0.97)
        cr.fill_preserve()
        cr.set_source_rgba(0.22, 0.34, 0.48, 0.92)
        cr.set_line_width(2.2)
        cr.stroke()

        cr.set_font_size(17)
        cr.set_source_rgb(0.42, 0.92, 0.72)
        cr.move_to(rx + 24, ry + 40)
        cr.show_text("$ wear-pointer --backend hyprland")

        kpis = [
            ("RX RATE", f"{hz} pkt/s", (0.063, 0.725, 0.506)),
            ("PACKETS", f"{self.pkt_total}", (0.22, 0.74, 0.97)),
            ("LAST KIND", self.last_kind, (0.98, 0.78, 0.26)),
        ]
        for i, (k, v, col) in enumerate(kpis):
            kx = rx + 22 + i * 192
            rounded_rect(cr, kx, ry + 60, 180, 76, 12)
            cr.set_source_rgba(1, 1, 1, 0.05)
            cr.fill()
            cr.set_font_size(12)
            cr.set_source_rgb(0.58, 0.66, 0.78)
            cr.move_to(kx + 14, ry + 84)
            cr.show_text(k)
            cr.set_font_size(19)
            cr.set_source_rgb(*col)
            cr.move_to(kx + 14, ry + 116)
            cr.show_text(v)

        cr.set_font_size(14.5)
        y_line = ry + 174
        palette = {
            "click": (0.26, 0.96, 0.70),
            "rclick": (0.74, 0.62, 1.0),
            "drag": (0.28, 0.82, 1.0),
            "center": (1.0, 0.84, 0.30),
            "pause": (1.0, 0.62, 0.24),
            "link": (0.38, 0.95, 0.75),
            "warn": (0.98, 0.45, 0.45),
            "stats": (0.52, 0.86, 0.92),
            "aim": (0.72, 0.80, 0.90),
        }
        for kind_key, ln in list(self.trace_lines):
            cr.set_source_rgb(*palette.get(kind_key, (0.72, 0.80, 0.90)))
            cr.move_to(rx + 24, y_line)
            cr.show_text(ln)
            y_line += 39

        # ── Cursor Comet Trail + Click Ripples + High-Visibility Halo ───────
        for tx, ty, ts in list(self.trail):
            age = now - ts
            if age < 0.42:
                alpha = (1.0 - age / 0.42) * 0.62
                rad = 4.0 + (1.0 - age / 0.42) * 10.0
                cr.set_source_rgba(0.063, 0.725, 0.506, alpha)
                cr.new_sub_path()
                cr.arc(tx, ty, rad, 0, 2 * math.pi)
                cr.fill()

        alive_ripples = []
        for rxp, ryp, rts, kind in self.ripples:
            age = now - rts
            if age < 0.80:
                alive_ripples.append((rxp, ryp, rts, kind))
                p = age / 0.80
                if kind == "left":
                    cr.set_source_rgba(0.063, 0.725, 0.506, (1.0 - p) * 0.95)
                else:
                    cr.set_source_rgba(0.72, 0.56, 1.0, (1.0 - p) * 0.95)
                cr.set_line_width(4.0 * (1.0 - p) + 1.2)
                cr.new_sub_path()
                cr.arc(rxp, ryp, 16.0 + p * 68.0, 0, 2 * math.pi)
                cr.stroke()
        self.ripples = alive_ripples

        # High-contrast pointer halo around the real Wayland cursor
        cr.set_source_rgba(0.063, 0.725, 0.506, 0.96 if self.left_down else 0.84)
        cr.set_line_width(3.4 if self.left_down else 2.4)
        cr.new_sub_path()
        cr.arc(self.cx, self.cy, 22.0 if self.left_down else 16.0, 0, 2 * math.pi)
        cr.stroke()
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.96)
        cr.new_sub_path()
        cr.arc(self.cx, self.cy, 4.5, 0, 2 * math.pi)
        cr.fill()

        return False


if __name__ == "__main__":
    os.environ.setdefault("GDK_BACKEND", "wayland")
    win = PointerStage()
    win.connect("destroy", Gtk.main_quit)
    win.show_all()
    Gtk.main()
