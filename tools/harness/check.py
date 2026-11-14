#!/usr/bin/env python3
"""Verify an end-to-end desktop harness capture directory.

Reads:
  trace.log    receiver link events, raw BLE notifications, and CursorActions
  marks.log    host wall-clock step timestamps from scenario.sh
  cursor.log   Hyprland IPC cursorpos samples (10 ms)
  wev.log      Wayland client wl_pointer events
  monitors.txt hyprctl monitors output
  watch.log    Wear OS logcat (-s wear-pointer)

Exits 0 iff all protocol, queue, calibration, sensor-gap, link-drop, Hyprland
compositor, and Wayland client checks pass.
"""

from __future__ import annotations

import re
import struct
import sys
from dataclasses import dataclass, field
from pathlib import Path

VERSION = 1
HEADER_LEN = 8
NEAR_ZERO_DEG = 3.0

KIND_AIM = 0
KIND_LEFT_CLICK = 1
KIND_RIGHT_CLICK = 2
KIND_SCROLL = 3
KIND_CENTER = 4
KIND_PAUSE = 5
KIND_INFO = 6
KIND_DRAG = 7
KIND_STATS = 8

KIND_NAMES = {
    KIND_AIM: "Aim",
    KIND_LEFT_CLICK: "LeftClick",
    KIND_RIGHT_CLICK: "RightClick",
    KIND_SCROLL: "Scroll",
    KIND_CENTER: "Center",
    KIND_PAUSE: "Pause",
    KIND_INFO: "Info",
    KIND_DRAG: "Drag",
    KIND_STATS: "Stats",
}


@dataclass
class DecodedPacket:
    unix: float
    rel_s: float
    epoch: int
    kind: int
    seq: int
    ts_us: int
    left_deg: float = 0.0
    up_deg: float = 0.0
    paused: bool = False
    down: bool = False
    delta_px: int = 0
    samples: int = 0
    notifies_ok: int = 0
    notifies_drop: int = 0


@dataclass
class EpochSummary:
    epoch: int
    connected_at: float | None = None
    disconnected_at: float | None = None
    disconnect_reason: str = ""
    packets: list[DecodedPacket] = field(default_factory=list)
    gaps: int = 0
    regressions: int = 0


def decode_packet(unix: float, rel_s: float, epoch: int, raw: bytes) -> DecodedPacket:
    if len(raw) < HEADER_LEN:
        raise ValueError(f"packet shorter than {HEADER_LEN}B: {raw.hex()}")
    ver, kind, seq, ts_us = struct.unpack_from("<BBHI", raw, 0)
    if ver != VERSION:
        raise ValueError(f"bad protocol version {ver}")
    pkt = DecodedPacket(unix=unix, rel_s=rel_s, epoch=epoch, kind=kind, seq=seq, ts_us=ts_us)
    if kind == KIND_AIM:
        if len(raw) < 12:
            raise ValueError("truncated Aim")
        left_cdeg, up_cdeg = struct.unpack_from("<hh", raw, 8)
        pkt.left_deg = left_cdeg / 100.0
        pkt.up_deg = up_cdeg / 100.0
    elif kind == KIND_SCROLL:
        if len(raw) < 10:
            raise ValueError("truncated Scroll")
        (pkt.delta_px,) = struct.unpack_from("<h", raw, 8)
    elif kind == KIND_PAUSE:
        if len(raw) < 9:
            raise ValueError("truncated Pause")
        pkt.paused = raw[8] != 0
    elif kind == KIND_DRAG:
        if len(raw) < 9:
            raise ValueError("truncated Drag")
        pkt.down = raw[8] != 0
    elif kind == KIND_STATS:
        if len(raw) < 16:
            raise ValueError("truncated Stats")
        pkt.samples, pkt.notifies_ok, pkt.notifies_drop, _ = struct.unpack_from("<HHHH", raw, 8)
    elif kind == KIND_INFO:
        if len(raw) < 12:
            raise ValueError("truncated Info")
    return pkt


def parse_marks(path: Path) -> dict[str, float]:
    marks: dict[str, float] = {}
    for line in path.read_text().splitlines():
        parts = line.strip().split(None, 1)
        if len(parts) == 2:
            marks[parts[1]] = float(parts[0])
    return marks


def parse_cursor(path: Path) -> list[tuple[float, float, float]]:
    samples: list[tuple[float, float, float]] = []
    if not path.exists():
        return samples
    for line in path.read_text().splitlines():
        parts = line.strip().split()
        if len(parts) == 3:
            samples.append((float(parts[0]), float(parts[1]), float(parts[2])))
    return samples


def cursor_near(samples: list[tuple[float, float, float]], t: float) -> tuple[float, float] | None:
    if not samples:
        return None
    best = min(samples, key=lambda s: abs(s[0] - t))
    if abs(best[0] - t) > 0.5:
        return None
    return best[1], best[2]


def last_move_before(moves: list[tuple[float, float, float]], t: float) -> tuple[float, float] | None:
    prior = [m for m in moves if m[0] <= t]
    if not prior:
        return None
    return prior[-1][1], prior[-1][2]


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: check.py OUT_DIR", file=sys.stderr)
        return 2
    out = Path(sys.argv[1])
    checks: list[tuple[str, bool, str]] = []

    def record(name: str, ok: bool, detail: str) -> None:
        checks.append((name, ok, detail))

    marks = parse_marks(out / "marks.log")
    cursor_samples = parse_cursor(out / "cursor.log")
    monitors_txt = (out / "monitors.txt").read_text() if (out / "monitors.txt").exists() else ""
    wev_txt = (out / "wev.log").read_text(errors="replace") if (out / "wev.log").exists() else ""
    watch_txt = (out / "watch.log").read_text(errors="replace") if (out / "watch.log").exists() else ""

    start_unix = 0.0
    epochs: dict[int, EpochSummary] = {}
    all_packets: list[DecodedPacket] = []
    moves: list[tuple[float, float, float]] = []
    buttons: list[tuple[float, str, bool]] = []
    decode_errors: list[str] = []
    live_epoch = 0

    for line in (out / "trace.log").read_text().splitlines():
        line = line.strip()
        if not line:
            continue
        if line.startswith("# start_unix "):
            start_unix = float(line.split()[2])
            continue
        if line.startswith("#"):
            continue
        parts = line.split()
        rel_s = float(parts[0])
        unix = start_unix + rel_s
        tag = parts[1]
        if tag == "connected":
            ep = int(parts[2])
            live_epoch = ep
            es = epochs.setdefault(ep, EpochSummary(epoch=ep))
            es.connected_at = unix
        elif tag == "disconnected":
            ep = int(parts[2])
            if live_epoch == ep:
                live_epoch = 0
            es = epochs.setdefault(ep, EpochSummary(epoch=ep))
            es.disconnected_at = unix
            es.disconnect_reason = " ".join(parts[3:])
        elif tag == "rx":
            ep = int(parts[2])
            if ep != live_epoch:
                continue
            try:
                pkt = decode_packet(unix, rel_s, ep, bytes.fromhex(parts[3]))
            except ValueError as exc:
                decode_errors.append(str(exc))
                continue
            es = epochs.setdefault(ep, EpochSummary(epoch=ep))
            if es.packets:
                prev = es.packets[-1].seq
                step = (pkt.seq - prev) & 0xFFFF
                if step != 1:
                    if step == 0 or step > 0x8000:
                        es.regressions += 1
                    else:
                        es.gaps += step - 1
            es.packets.append(pkt)
            all_packets.append(pkt)
        elif tag == "move":
            moves.append((unix, float(parts[2]), float(parts[3])))
        elif tag == "button":
            buttons.append((unix, parts[2], parts[3] == "down"))

    # 1. Multi-epoch BLE link lifecycle.
    active_epochs = [es for _, es in sorted(epochs.items()) if es.packets]
    aim_counts = [sum(1 for p in es.packets if p.kind == KIND_AIM) for es in active_epochs]
    record(
        "ble_epochs_and_decode",
        len(active_epochs) >= 3 and all(c > 0 for c in aim_counts) and aim_counts[0] >= 30 and not decode_errors,
        f"epochs={len(active_epochs)} aims_per_epoch={aim_counts} decode_errors={len(decode_errors)}",
    )

    # 2. Sequence continuity (no gaps, no regressions, starts at 0 on every link).
    bad_starts = [(es.epoch, es.packets[0].seq) for es in active_epochs if es.packets[0].seq != 0]
    total_gaps = sum(es.gaps for es in active_epochs)
    total_regs = sum(es.regressions for es in active_epochs)
    record(
        "sequence_contiguous_from_zero",
        not bad_starts and total_gaps == 0 and total_regs == 0,
        f"bad_starts={bad_starts} gaps={total_gaps} regressions={total_regs} total_rx={len(all_packets)}",
    )

    # 3. Center calibration boundary across all Center packets + mid-turn check.
    center_checks: list[str] = []
    center_failures: list[str] = []
    mid_turn_proved = False
    t_mid_start = marks.get("center-mid-turn", 0.0)
    t_click = marks.get("click", 0.0)
    for es in active_epochs:
        last_aim: DecodedPacket | None = None
        pending_center: tuple[DecodedPacket, DecodedPacket | None] | None = None
        for pkt in es.packets:
            if pkt.kind == KIND_CENTER:
                pending_center = (pkt, last_aim)
            elif pkt.kind == KIND_AIM:
                if pending_center is not None:
                    c_pkt, prev_aim = pending_center
                    pending_center = None
                    near = abs(pkt.left_deg) <= NEAR_ZERO_DEG and abs(pkt.up_deg) <= NEAR_ZERO_DEG
                    prev_desc = f"{prev_aim.left_deg:.2f}" if prev_aim else "none"
                    center_checks.append(
                        f"ep{es.epoch}#seq{c_pkt.seq}: before={prev_desc}deg -> after=({pkt.left_deg:.2f},{pkt.up_deg:.2f})deg"
                    )
                    if not near:
                        center_failures.append(center_checks[-1])
                    if t_mid_start - 0.2 <= c_pkt.unix <= t_click and prev_aim and abs(prev_aim.left_deg) >= 15.0 and near:
                        mid_turn_proved = True
                last_aim = pkt
    record(
        "center_state_boundary",
        len(center_checks) >= 4 and not center_failures and mid_turn_proved,
        f"centers={len(center_checks)} mid_turn_proved={mid_turn_proved} failures={center_failures} ({'; '.join(center_checks)})",
    )

    # 4. Left click, hold-to-drag, and right click.
    t_drag = marks.get("drag", 0.0)
    t_rclick = marks.get("rclick", 0.0)
    t_pause = marks.get("pause", 0.0)
    lclicks = [p for p in all_packets if p.kind == KIND_LEFT_CLICK and t_click - 0.1 <= p.unix <= t_drag]
    click_btns = [(b, d) for (t, b, d) in buttons if t_click - 0.1 <= t <= t_drag]
    drag_pkts = [p for p in all_packets if p.kind == KIND_DRAG and t_drag - 0.1 <= p.unix <= t_rclick]
    drag_btns = [(t, b, d) for (t, b, d) in buttons if t_drag - 0.1 <= t <= t_rclick]
    drag_moves_ok = False
    if len(drag_btns) >= 2 and drag_btns[0][1:] == ("Left", True) and drag_btns[-1][1:] == ("Left", False):
        span_moves = [m for m in moves if drag_btns[0][0] <= m[0] <= drag_btns[-1][0]]
        if len(span_moves) >= 5 and (span_moves[-1][1] - span_moves[0][1]) >= 150.0:
            drag_moves_ok = True
    rclicks = [p for p in all_packets if p.kind == KIND_RIGHT_CLICK and t_rclick - 0.1 <= p.unix <= t_pause]
    rclick_btns = [(b, d) for (t, b, d) in buttons if t_rclick - 0.1 <= t <= t_pause]
    record(
        "click_drag_rclick",
        len(lclicks) == 1
        and click_btns == [("Left", True), ("Left", False)]
        and [p.down for p in drag_pkts] == [True, False]
        and drag_moves_ok
        and len(rclicks) == 1
        and rclick_btns == [("Right", True), ("Right", False)],
        f"lclick={len(lclicks)} click_btns={click_btns} drag_pkts={[p.down for p in drag_pkts]} drag_moves_ok={drag_moves_ok} rclick={len(rclicks)} rclick_btns={rclick_btns}",
    )

    # 5. Pause keeps tracking while awake, sends no Aim while paused, carries turn into Resume.
    t_gap_pause = marks.get("gap-pause", 0.0)
    pause_window = [p for p in all_packets if t_pause - 0.1 <= p.unix <= t_gap_pause - 0.05]
    p_on = next((p for p in pause_window if p.kind == KIND_PAUSE and p.paused), None)
    p_off = next((p for p in pause_window if p_on and p.unix > p_on.unix and p.kind == KIND_PAUSE and not p.paused), None)
    pause_ok = False
    pause_detail = "missing Pause/Resume"
    if p_on and p_off:
        aims_during_pause = [p for p in pause_window if p.kind == KIND_AIM and p_on.unix < p.unix < p_off.unix]
        centers_on_resume = [p for p in pause_window if p.kind == KIND_CENTER and p_on.unix <= p.unix <= p_off.unix + 0.8]
        moves_during_pause = [m for m in moves if p_on.unix + 0.05 < m[0] < p_off.unix]
        aim_before = next((p for p in reversed(all_packets) if p.kind == KIND_AIM and p.unix < p_on.unix), None)
        aim_after = next((p for p in all_packets if p.kind == KIND_AIM and p.unix > p_off.unix), None)
        delta_left = (aim_after.left_deg - aim_before.left_deg) if (aim_before and aim_after) else 0.0
        pause_ok = (
            not aims_during_pause
            and not centers_on_resume
            and not moves_during_pause
            and 20.0 <= delta_left <= 37.0
        )
        pause_detail = (
            f"aims_during={len(aims_during_pause)} moves_during={len(moves_during_pause)} "
            f"centers={len(centers_on_resume)} delta_left={delta_left:.2f}deg"
        )
    record("pause_awake_carryover", pause_ok, pause_detail)

    # 6. Sensor gap while paused triggers automatic Center on Resume.
    t_drop_drag = marks.get("drop-drag", 0.0)
    gap_skipped = "gap-skipped" in marks
    gap_window = [p for p in all_packets if t_gap_pause - 0.1 <= p.unix <= t_drop_drag - 0.05]
    gp_on = next((p for p in gap_window if p.kind == KIND_PAUSE and p.paused), None)
    gp_off = next((p for p in gap_window if gp_on and p.unix > gp_on.unix and p.kind == KIND_PAUSE and not p.paused), None)
    auto_center = next(
        (p for p in gap_window if gp_off and gp_off.unix <= p.unix <= gp_off.unix + 1.0 and p.kind == KIND_CENTER),
        None,
    )
    aim_after_gap = next(
        (p for p in gap_window if auto_center and p.unix > auto_center.unix and p.kind == KIND_AIM),
        None,
    )
    gap_status_logged = "Sensor gap" in watch_txt or auto_center is not None
    record(
        "sensor_gap_auto_recenter",
        (not gap_skipped)
        and gp_on is not None
        and gp_off is not None
        and auto_center is not None
        and aim_after_gap is not None
        and abs(aim_after_gap.left_deg) <= NEAR_ZERO_DEG
        and gap_status_logged,
        f"skipped={gap_skipped} auto_center={auto_center.seq if auto_center else None} "
        f"post_aim={f'{aim_after_gap.left_deg:.2f}' if aim_after_gap else None}deg",
    )

    # 7. Forced link drop mid-drag releases Left button, fires silence/disconnect, and reconnects at seq 0.
    t_drop = marks.get("drop", 0.0)
    t_after_reconn = marks.get("after-reconnect", 0.0)
    drop_btns = [(t, b, d) for (t, b, d) in buttons if t_drop_drag - 0.1 <= t <= t_after_reconn]
    drop_disc = next(
        (es for es in active_epochs if es.disconnected_at and t_drop - 0.2 <= es.disconnected_at <= t_drop + 6.5),
        None,
    )
    drop_reconn = next(
        (es for es in active_epochs if es.connected_at and t_drop < es.connected_at <= t_after_reconn),
        None,
    )
    held_state = {"Left": False, "Right": False}
    for _, b, d in buttons:
        held_state[b] = d
    drop_pressed = any(b == "Left" and d and t <= t_drop + 0.2 for (t, b, d) in drop_btns)
    drop_released = any(b == "Left" and (not d) and t >= t_drop - 0.05 for (t, b, d) in drop_btns)
    record(
        "link_drop_releases_drag_and_reconnects",
        drop_pressed
        and drop_released
        and drop_disc is not None
        and drop_reconn is not None
        and drop_reconn.packets[0].seq == 0
        and not any(held_state.values()),
        f"btns={[(b, d) for (_, b, d) in drop_btns]} "
        f"disc_in={f'{drop_disc.disconnected_at - t_drop:.2f}s ({drop_disc.disconnect_reason})' if drop_disc and drop_disc.disconnected_at else None} "
        f"reconn_seq0={drop_reconn.packets[0].seq if drop_reconn else None} held_end={held_state}",
    )

    # 8. Stop disconnects cleanly; Start reconnects cleanly at seq 0.
    t_stop = marks.get("stop", 0.0)
    t_start_again = marks.get("start-again", 0.0)
    stop_disc = next(
        (es for es in active_epochs if es.disconnected_at and t_stop - 0.1 <= es.disconnected_at <= t_stop + 6.5),
        None,
    )
    rx_while_stopped = [
        p
        for p in all_packets
        if stop_disc and stop_disc.disconnected_at and stop_disc.disconnected_at + 0.2 < p.unix < t_start_again - 0.05
    ]
    restart_ep = next(
        (es for es in active_epochs if es.connected_at and es.connected_at >= t_start_again - 0.1),
        None,
    )
    record(
        "stop_and_start_lifecycle",
        stop_disc is not None
        and not rx_while_stopped
        and restart_ep is not None
        and restart_ep.packets[0].seq == 0,
        f"stop_disc={f'{stop_disc.disconnected_at - t_stop:.2f}s' if stop_disc and stop_disc.disconnected_at else None} "
        f"rx_while_stopped={len(rx_while_stopped)} "
        f"restart_packets={len(restart_ep.packets) if restart_ep else 0}",
    )

    # 9. Real Hyprland compositor state (monitors.txt + cursor.log IPC samples).
    mon_ok = "1920x1080" in monitors_txt
    c_center1 = cursor_near(cursor_samples, marks.get("turn-left", 0.0) - 0.15)
    c_clamped = cursor_near(cursor_samples, marks.get("center-mid-turn", 0.0) - 0.15)
    m_clamped = last_move_before(moves, marks.get("center-mid-turn", 0.0) - 0.15)
    c_gap = cursor_near(cursor_samples, marks.get("drop-drag", 0.0) - 0.15)
    c_interior = cursor_near(cursor_samples, marks.get("stop", 0.0) - 0.15)
    m_interior = last_move_before(moves, marks.get("stop", 0.0) - 0.15)

    def close(pt: tuple[float, float] | None, target: tuple[float, float], tol: float = 4.0) -> bool:
        return pt is not None and abs(pt[0] - target[0]) <= tol and abs(pt[1] - target[1]) <= tol

    interior_match = False
    if c_interior is not None and m_interior is not None:
        clamped_move = (min(max(round(m_interior[0]), 0), 1920), min(max(round(m_interior[1]), 0), 1080))
        interior_match = close(c_interior, clamped_move, tol=4.0) and 200.0 <= c_interior[0] <= 800.0

    record(
        "hyprland_compositor_cursor",
        mon_ok
        and len(cursor_samples) >= 500
        and close(c_center1, (960.0, 540.0))
        and m_clamped is not None
        and m_clamped[0] < 0.0
        and close(c_clamped, (0.0, 540.0), tol=5.0)
        and close(c_gap, (960.0, 540.0))
        and interior_match,
        f"samples={len(cursor_samples)} center1={c_center1} edge=({m_clamped}->{c_clamped}) "
        f"gap_center={c_gap} interior=({m_interior}->{c_interior})",
    )

    # 10. Real Wayland client (wev) receives motion and balanced button events.
    wev_motions = len(re.findall(r"wl_pointer\]\s+motion:", wev_txt))
    left_pressed = len(re.findall(r"button:\s+272\b.*state:\s+1\b", wev_txt))
    left_released = len(re.findall(r"button:\s+272\b.*state:\s+0\b", wev_txt))
    right_pressed = len(re.findall(r"button:\s+273\b.*state:\s+1\b", wev_txt))
    right_released = len(re.findall(r"button:\s+273\b.*state:\s+0\b", wev_txt))
    expected_left = sum(1 for (_, b, d) in buttons if b == "Left" and d)
    expected_right = sum(1 for (_, b, d) in buttons if b == "Right" and d)
    record(
        "wayland_client_wev_delivery",
        wev_motions >= 50
        and left_pressed >= 3
        and left_pressed == left_released == expected_left
        and right_pressed >= 1
        and right_pressed == right_released == expected_right,
        f"motions={wev_motions} left={left_pressed}/{left_released}(expected {expected_left}) "
        f"right={right_pressed}/{right_released}(expected {expected_right})",
    )

    failed = [c for c in checks if not c[1]]
    print(f"== desktop harness check: {out} ==")
    for name, ok, detail in checks:
        mark = "PASS" if ok else "FAIL"
        print(f"  [{mark}] {name}: {detail}")
    if failed:
        print(f"RESULT: FAILED ({len(failed)}/{len(checks)} checks failed)")
        return 1
    print(f"RESULT: OK ({len(checks)}/{len(checks)} checks passed)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
