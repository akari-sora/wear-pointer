//! Desktop half of wear-pointer.
//!
//! Bluetooth runs on a Tokio runtime. The cursor loop is a plain thread at
//! 240 Hz so Wayland dispatch stays on one thread. `--debug` prints packet
//! rate, loss, and relative latency once a second for Stage 0.

use anyhow::{Context, Result};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use wear_pointer_bluetooth::{CentralLink, LinkEvent, PeripheralLink, PointerLink};
use wear_pointer_core::{CursorAction, PointerSession, Screen, SessionConfig, CURSOR_HZ};
use wear_pointer_platform::{LogBackend, PointerBackend};

#[cfg(target_os = "linux")]
use wear_pointer_platform::HyprlandBackend;
#[cfg(all(feature = "uinput", target_os = "linux"))]
use wear_pointer_platform::UinputBackend;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Central,
    Peripheral,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackendKind {
    Hyprland,
    Log,
    Uinput,
}

struct Args {
    debug: bool,
    role: Role,
    backend: BackendKind,
    config: SessionConfig,
}

fn main() -> Result<()> {
    let args = parse_args()?;
    if args.role == Role::Peripheral {
        let mut link = PeripheralLink;
        let (tx, _rx) = mpsc::channel();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        return rt.block_on(link.run(tx, Instant::now()));
    }

    let start = Instant::now();
    let (tx, rx) = mpsc::channel::<LinkEvent>();
    let clock = start;
    thread::Builder::new()
        .name("wear-pointer-bt".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(err) => {
                    eprintln!("wear-pointer: tokio runtime failed: {err}");
                    return;
                }
            };
            rt.block_on(async move {
                let mut link = match CentralLink::new() {
                    Ok(link) => link,
                    Err(err) => {
                        eprintln!("wear-pointer: {err:#}");
                        return;
                    }
                };
                if let Err(err) = link.run(tx, clock).await {
                    eprintln!("wear-pointer: {err:#}");
                }
            });
        })
        .context("spawn bluetooth thread")?;

    let mut backend = open_backend(args.backend, args.config.screen)?;
    let mut session = PointerSession::new(args.config);
    session.set_screen(backend.screen());
    let frame = Duration::from_secs_f64(1.0 / f64::from(CURSOR_HZ));
    let mut next = Instant::now();
    let mut link_epoch = 0u64;
    eprintln!(
        "wear-pointer: cursor loop at {CURSOR_HZ} Hz. Ctrl-C to quit. Pairing whitelist is not implemented."
    );
    loop {
        let screen = backend.screen();
        if screen != session.screen() {
            session.set_screen(screen);
        }
        while let Ok(event) = rx.try_recv() {
            let actions = match event {
                LinkEvent::Connected { epoch, device } => {
                    link_epoch = epoch;
                    eprintln!("wear-pointer: link up epoch={epoch} {device}");
                    session.on_connected()
                }
                LinkEvent::Disconnected { epoch, reason } => {
                    if epoch == link_epoch {
                        link_epoch = 0;
                    }
                    eprintln!("wear-pointer: link down epoch={epoch} ({reason})");
                    session.on_disconnected()
                }
                LinkEvent::Packet {
                    epoch,
                    received_at_s,
                    bytes,
                } if epoch == link_epoch => session.push_packet(&bytes, received_at_s),
                LinkEvent::Packet { epoch, .. } => {
                    eprintln!(
                        "wear-pointer: dropped packet from epoch {epoch}, live is {link_epoch}"
                    );
                    Vec::new()
                }
            };
            for action in actions {
                apply(&mut *backend, action)?;
            }
        }
        let now = start.elapsed().as_secs_f64();
        let (actions, debug) = session.tick(now);
        for action in actions {
            apply(&mut *backend, action)?;
        }
        if args.debug {
            if let Some(debug) = debug {
                let cursor = match (debug.cursor_x, debug.cursor_y) {
                    (Some(x), Some(y)) => format!("{x:.0},{y:.0}"),
                    _ => "-".to_string(),
                };
                let remote = match (
                    debug.remote_aim_hz,
                    debug.remote_samples,
                    debug.remote_notify_ok,
                    debug.remote_notify_drop,
                ) {
                    (Some(aim_hz), Some(samples), Some(ok), Some(drop)) => {
                        format!(
                            " aim_hz={aim_hz:.1} watch_samples={samples} notify_ok={ok} notify_drop={drop}"
                        )
                    }
                    _ => String::new(),
                };
                eprintln!(
                    "wear-pointer debug: rx={:.1}/s loss_window={} ({:.1}%) latency_ms avg={:.1} min={:.1} max={:.1} cursor={cursor}{remote}",
                    debug.rx_hz,
                    debug.lost_window,
                    debug.loss_pct_window,
                    debug.latency_avg_ms,
                    debug.latency_min_ms,
                    debug.latency_max_ms,
                );
            }
        }
        next += frame;
        let now_instant = Instant::now();
        if next > now_instant {
            thread::sleep(next - now_instant);
        } else {
            next = now_instant;
        }
    }
}

fn apply(backend: &mut dyn PointerBackend, action: CursorAction) -> Result<()> {
    match action {
        CursorAction::Move { x, y } => backend.motion_absolute(x, y),
        CursorAction::Button { button, down } => backend.button(button, down),
        CursorAction::Scroll { delta_px } => backend.scroll(delta_px),
    }
}

fn open_backend(kind: BackendKind, screen: Screen) -> Result<Box<dyn PointerBackend>> {
    match kind {
        BackendKind::Log => Ok(Box::new(LogBackend::new(screen))),
        BackendKind::Hyprland => {
            #[cfg(target_os = "linux")]
            {
                Ok(Box::new(HyprlandBackend::connect()?))
            }
            #[cfg(not(target_os = "linux"))]
            {
                Err(anyhow::anyhow!("the hyprland backend is Linux-only"))
            }
        }
        BackendKind::Uinput => {
            #[cfg(all(feature = "uinput", target_os = "linux"))]
            {
                Ok(Box::new(UinputBackend::open(screen)?))
            }
            #[cfg(not(all(feature = "uinput", target_os = "linux")))]
            {
                let _ = screen;
                Err(anyhow::anyhow!(
                    "uinput is not in this build. From receiver/: cargo build -p wear-pointer --features uinput"
                ))
            }
        }
    }
}

fn parse_args() -> Result<Args> {
    let mut args = std::env::args().skip(1);
    let mut debug = false;
    let mut role = Role::Central;
    let mut backend = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        BackendKind::Hyprland
    } else {
        BackendKind::Log
    };
    let mut config = SessionConfig::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            "--debug" => debug = true,
            "--role" => {
                role = match args.next().as_deref() {
                    Some("central") => Role::Central,
                    Some("peripheral") => Role::Peripheral,
                    other => anyhow::bail!("--role expects central or peripheral, got {other:?}"),
                };
            }
            "--backend" => {
                backend = match args.next().as_deref() {
                    Some("hyprland") => BackendKind::Hyprland,
                    Some("log") => BackendKind::Log,
                    Some("uinput") => BackendKind::Uinput,
                    other => {
                        anyhow::bail!("--backend expects hyprland, log, or uinput, got {other:?}")
                    }
                };
            }
            "--speed" => config.speed = number(&mut args, "--speed")?,
            "--steady" => config.steady_hz = number(&mut args, "--steady")?,
            "--quick" => config.quickness = number(&mut args, "--quick")?,
            "--dead-zone" => config.dead_zone_deg = number(&mut args, "--dead-zone")?,
            "--screen" => {
                let value = args.next().context("--screen WIDTH x HEIGHT")?;
                config.screen = parse_screen(&value)?;
            }
            other if other.starts_with("--") => anyhow::bail!("unknown argument {other}"),
            other => {
                if let Ok(speed) = other.parse::<f64>() {
                    config.speed = speed;
                } else if let Some(value) = other.strip_prefix("steady=") {
                    config.steady_hz = value.parse().context("steady=")?;
                } else if let Some(value) = other.strip_prefix("quick=") {
                    config.quickness = value.parse().context("quick=")?;
                } else {
                    anyhow::bail!("unknown argument {other}");
                }
            }
        }
    }
    Ok(Args {
        debug,
        role,
        backend,
        config,
    })
}

fn number(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<f64> {
    args.next()
        .with_context(|| format!("{flag} needs a number"))?
        .parse()
        .with_context(|| format!("{flag} needs a number"))
}

fn parse_screen(value: &str) -> Result<Screen> {
    let (w, h) = value
        .split_once(['x', 'X'])
        .context("--screen example: 1920x1080")?;
    Ok(Screen::new(w.parse()?, h.parse()?))
}

fn print_help() {
    eprintln!(
        "\
wear-pointer — desktop receiver

  wear-pointer [--debug] [--role central|peripheral] [--backend hyprland|log|uinput]
               [--speed 1.0] [--steady 0.8] [--quick 0.01] [--dead-zone 0.0]
               [--screen 1920x1080]

Stage 0 (no cursor movement):
  cargo run -p wear-pointer -- --debug --backend log

Stage 1 on Hyprland (default when WAYLAND_DISPLAY is set):
  cargo run -p wear-pointer --release -- --debug

--speed is WatchPointer's sensitivity: higher reaches the screen edge with less turn.
--dead-zone is degrees of aim to ignore. Leave it at 0 unless a log shows jitter.
--role peripheral is the documented fallback and is not implemented on the desktop yet.
Pairing whitelist is a TODO. Quit the receiver when you are not using it.
"
    );
}
