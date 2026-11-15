//! Feed a `wp-probe` logcat capture through the desktop session.
//!
//! The probe (wearos/probe, emulator only) logs one line per notification:
//! `rx <elapsedRealtimeNanos> <hex>`, plus `connected` and `disconnected`.
//! This replays those lines into [`PointerSession`] with 240 Hz ticks in
//! between, exactly as the receiver's loop would, and checks:
//!
//! - sequence: per link, every packet is previous + 1 (no gap, no step back);
//! - Center: the first aim after each Center is near the new zero;
//! - buttons: what the cursor backend would have pressed and released, and
//!   whether a button is still held at the end.
//!
//! Usage (from receiver/):
//!     adb -s emulator-5562 logcat -d -s wp-probe | cargo run -p wear-pointer-core --example replay
//!
//! Exit code 1 when a sequence or Center check fails.

use std::io::{self, BufRead};

use wear_pointer_core::{decode, Button, CursorAction, Packet, PointerSession, SessionConfig};

/// An aim this close to zero (degrees, each axis) counts as "on the new base".
const NEAR_ZERO_DEG: f64 = 3.0;

#[derive(Default)]
struct Link {
    index: usize,
    packets: u64,
    gaps: u64,
    steps_back: u64,
    last_seq: Option<u16>,
    first_seq: Option<u16>,
    kinds: [u64; 9],
}

fn main() {
    let mut session = PointerSession::new(SessionConfig::default());
    let mut link = Link::default();
    let mut links = Vec::new();
    let mut failures = Vec::<String>::new();
    let mut held = false;
    let mut last_tick_s: Option<f64> = None;
    let mut last_aim: Option<(f64, f64)> = None;
    let mut awaiting_first_aim_after_center: Option<(u16, Option<(f64, f64)>)> = None;
    let mut cursor: Option<(f64, f64)> = None;

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Some(pos) = line.find("wp-probe") else { continue };
        let body = line[pos..].splitn(2, ": ").nth(1).unwrap_or("").trim();

        // The receiver emits Connected before it subscribes, and the watch can
        // send before the probe logs its CCCD write callback, so "connected"
        // is the link boundary.
        if body.starts_with("connected") {
            if link.packets > 0 {
                links.push(std::mem::take(&mut link));
            }
            link.index = links.len() + 1;
            apply(&session.on_connected(), &mut held, &mut cursor, "connect");
            println!("== link {} connected", link.index);
            last_aim = None;
            continue;
        }
        if body.starts_with("disconnected") {
            println!("== link {} disconnected", link.index);
            apply(&session.on_disconnected(), &mut held, &mut cursor, "disconnect");
            continue;
        }
        let mut parts = body.split_whitespace();
        if parts.next() != Some("rx") {
            continue;
        }
        let (Some(ns), Some(hex)) = (parts.next(), parts.next()) else { continue };
        let Ok(ns) = ns.parse::<u64>() else { continue };
        let Some(bytes) = unhex(hex) else { continue };
        let local_s = ns as f64 / 1e9;

        // 240 Hz ticks up to this arrival, as the receiver loop would run them.
        if let Some(mut t) = last_tick_s {
            while t + 1.0 / 240.0 <= local_s {
                t += 1.0 / 240.0;
                let (actions, _) = session.tick(t);
                apply(&actions, &mut held, &mut cursor, "tick");
            }
            last_tick_s = Some(t);
        } else {
            last_tick_s = Some(local_s);
        }

        let Ok(packet) = decode(&bytes) else {
            failures.push(format!("undecodable packet {hex}"));
            continue;
        };
        let header = packet.header().clone();
        let seq = header.sequence;
        link.packets += 1;
        if let Some(k) = link.kinds.get_mut(usize::from(header.kind)) {
            *k += 1;
        }
        match link.last_seq {
            None => link.first_seq = Some(seq),
            Some(prev) => {
                let step = seq.wrapping_sub(prev);
                if step != 1 {
                    if step > 0x8000 || step == 0 {
                        link.steps_back += 1;
                        failures.push(format!("link {} seq stepped back {prev} -> {seq}", link.index));
                    } else {
                        link.gaps += u64::from(step - 1);
                        failures.push(format!("link {} seq gap {prev} -> {seq}", link.index));
                    }
                }
            }
        }
        link.last_seq = Some(seq);

        match &packet {
            Packet::Aim {
                left_cdeg, up_cdeg, ..
            } => {
                let aim = (f64::from(*left_cdeg) / 100.0, f64::from(*up_cdeg) / 100.0);
                if let Some((center_seq, before)) = awaiting_first_aim_after_center.take() {
                    let near = aim.0.abs() <= NEAR_ZERO_DEG && aim.1.abs() <= NEAR_ZERO_DEG;
                    println!(
                        "   first aim after Center#{center_seq}: seq {seq} left {:.2} up {:.2} (aim before Center: {}) {}",
                        aim.0,
                        aim.1,
                        before.map(|(l, u)| format!("left {l:.2} up {u:.2}")).unwrap_or_else(|| "none".into()),
                        if near { "OK" } else { "NOT NEAR ZERO" },
                    );
                    if !near {
                        failures.push(format!("aim after Center#{center_seq} is not on the new base"));
                    }
                }
                last_aim = Some(aim);
            }
            Packet::Center { .. } => {
                println!("   seq {seq} Center");
                awaiting_first_aim_after_center = Some((seq, last_aim));
            }
            Packet::LeftClick { .. } => println!("   seq {seq} LeftClick"),
            Packet::RightClick { .. } => println!("   seq {seq} RightClick"),
            Packet::Drag { down, .. } => println!("   seq {seq} Drag {}", if *down { "down" } else { "up" }),
            Packet::Pause { paused, .. } => println!("   seq {seq} {}", if *paused { "Pause" } else { "Resume" }),
            Packet::Scroll { delta_px, .. } => println!("   seq {seq} Scroll {delta_px}"),
            Packet::Stats {
                samples,
                notifies_ok,
                notifies_drop,
                ..
            } => println!("   seq {seq} Stats samples={samples} ok={notifies_ok} drop={notifies_drop}"),
            Packet::Sensitivity { dpi, .. } => println!("   seq {seq} Sensitivity {dpi} DPI"),
            Packet::Info { .. } | Packet::Unknown { .. } => {}
        }
        let actions = session.push_packet(&bytes, local_s);
        apply(&actions, &mut held, &mut cursor, "packet");
    }
    // One more second of 240 Hz ticks, so the 300 ms no-aim release can fire.
    if let Some(mut t) = last_tick_s {
        let end = t + 1.0;
        while t < end {
            t += 1.0 / 240.0;
            let (actions, _) = session.tick(t);
            apply(&actions, &mut held, &mut cursor, "tick");
        }
    }
    if link.packets > 0 {
        links.push(link);
    }

    println!();
    for l in &links {
        println!(
            "link {}: {} packets, seq {:?}..{:?}, gaps {}, steps back {}, aim {} click {} center {} pause {} drag {} stats {}",
            l.index,
            l.packets,
            l.first_seq,
            l.last_seq,
            l.gaps,
            l.steps_back,
            l.kinds[0],
            l.kinds[1] + l.kinds[2],
            l.kinds[4],
            l.kinds[5],
            l.kinds[7],
            l.kinds[8],
        );
        if l.first_seq != Some(0) && l.first_seq.is_some() {
            println!("   note: link {} did not start at sequence 0", l.index);
        }
    }
    if links.iter().all(|l| l.packets == 0) {
        failures.push("no packets in the capture".into());
    }
    println!("button held at end: {held}");
    if held {
        failures.push("a mouse button is still held at the end".into());
    }
    if let Some((x, y)) = cursor {
        println!("last cursor move: {x:.1}, {y:.1}");
    }
    if failures.is_empty() {
        println!("RESULT: OK");
    } else {
        println!("RESULT: {} problem(s)", failures.len());
        for f in &failures {
            println!("  - {f}");
        }
        std::process::exit(1);
    }
}

fn apply(actions: &[CursorAction], held: &mut bool, cursor: &mut Option<(f64, f64)>, why: &str) {
    for action in actions {
        match action {
            CursorAction::Move { x, y } => *cursor = Some((*x, *y)),
            CursorAction::Button { button, down } => {
                if *button == Button::Left {
                    // A click is down+up in one batch; a drag holds across packets.
                    *held = *down;
                }
                println!(
                    "   -> {:?} {} ({why}){}",
                    button,
                    if *down { "down" } else { "up" },
                    cursor.map(|(x, y)| format!(" at {x:.0},{y:.0}")).unwrap_or_default(),
                );
            }
            CursorAction::Scroll { delta_px } => println!("   -> scroll {delta_px} ({why})"),
        }
    }
}

fn unhex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}
