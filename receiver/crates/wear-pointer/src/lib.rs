//! Desktop runtime shared by the `wear-pointer` CLI and `wear-pointer-app` GUI tray binary.
//!
//! Bluetooth runs on a Tokio runtime. The cursor loop runs on a dedicated thread
//! at 240 Hz so Wayland dispatch stays on one thread, while the main thread is
//! free to host the native Menu Bar / System Tray event loop when enabled.

pub mod autostart;
pub mod tray;

use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tray::{TrayBridge, TrayCommand, TraySnapshot};
use wear_pointer_bluetooth::{CentralLink, LinkEvent, PeripheralLink, PointerLink};
use wear_pointer_core::{
    dpi_to_speed, reach_degrees, ControlMode, CursorAction, PointerSession, Screen, SessionConfig,
    CURSOR_HZ, DEFAULT_DPI,
};
use wear_pointer_platform::{LogBackend, PointerBackend};

#[cfg(target_os = "linux")]
use wear_pointer_platform::HyprlandBackend;
#[cfg(target_os = "macos")]
use wear_pointer_platform::MacosBackend;
#[cfg(all(feature = "uinput", target_os = "linux"))]
use wear_pointer_platform::UinputBackend;
#[cfg(target_os = "windows")]
use wear_pointer_platform::WindowsBackend;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Central,
    Peripheral,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackendKind {
    Hyprland,
    Log,
    Macos,
    Uinput,
    Windows,
}

#[derive(Clone)]
struct Logger {
    file: Option<Arc<Mutex<BufWriter<File>>>>,
}

impl Logger {
    fn new(log_to_default_file: bool) -> Self {
        let file = if log_to_default_file {
            let path = autostart::default_log_path();
            // Cap log growth across restarts at ~2 MB.
            let truncate = std::fs::metadata(&path)
                .map(|m| m.len() > 2 * 1024 * 1024)
                .unwrap_or(false);
            OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(truncate)
                .append(!truncate)
                .open(&path)
                .ok()
                .map(|f| Arc::new(Mutex::new(BufWriter::new(f))))
        } else {
            None
        };
        Self { file }
    }

    fn line(&self, msg: impl AsRef<str>) {
        let text = msg.as_ref();
        eprintln!("{text}");
        if let Some(f) = &self.file {
            if let Ok(mut guard) = f.lock() {
                let secs = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let _ = writeln!(guard, "[{secs}] {text}");
                let _ = guard.flush();
            }
        }
    }
}

struct Args {
    debug: bool,
    tray: bool,
    file_log: bool,
    adapter: Option<String>,
    device: Option<String>,
    trace: Option<String>,
    replay: Option<String>,
    role: Role,
    backend: BackendKind,
    screen_explicit: bool,
    config: SessionConfig,
}

/// Entry point for the `wear-pointer` developer/terminal binary.
///
/// Preserves 100% of traditional CLI behavior in terminals, while automatically
/// enabling the macOS Menu Bar UI when `WearPointer.app` is double-clicked in Finder.
pub fn run_cli_entry() -> Result<()> {
    let args = parse_args(false)?;
    run_with_args(args)
}

/// Entry point for the `wear-pointer-app` / `WearPointer.exe` consumer GUI binary.
///
/// Defaults to System Tray / Menu Bar mode and logs to the OS user log folder.
pub fn run_app_entry() -> Result<()> {
    let args = parse_args(true)?;
    run_with_args(args)
}

fn run_with_args(args: Args) -> Result<()> {
    if args.role == Role::Peripheral {
        let mut link = PeripheralLink;
        let (tx, _rx) = mpsc::channel();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        return rt.block_on(link.run(tx, Instant::now()));
    }

    let logger = Logger::new(args.file_log);
    let stop = Arc::new(AtomicBool::new(false));
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    if args.tray {
        let initial_snap = TraySnapshot {
            effective_mode: args.config.mode_override.unwrap_or(ControlMode::Absolute),
            mode_override: args.config.mode_override,
            effective_dpi: wear_pointer_core::speed_to_dpi(args.config.speed),
            dpi_override: args
                .config
                .cli_speed_override
                .map(wear_pointer_core::speed_to_dpi),
            ..TraySnapshot::default()
        };
        let (bridge, cmd_rx) = TrayBridge::new(initial_snap, Arc::clone(&stop));
        let engine_bridge = Arc::clone(&bridge);
        let engine_stop = Arc::clone(&stop);
        let engine_logger = logger.clone();

        let engine_handle = thread::Builder::new()
            .name("wear-pointer-engine".into())
            .spawn(move || -> Result<()> {
                let res = run_engine(
                    args,
                    engine_logger.clone(),
                    Arc::clone(&engine_stop),
                    shutdown_tx,
                    shutdown_rx,
                    Some((engine_bridge, cmd_rx)),
                );
                if let Err(ref err) = res {
                    engine_logger.line(format!("wear-pointer: fatal error: {err:#}"));
                }
                engine_stop.store(true, Ordering::Release);
                res
            })
            .context("spawn engine thread")?;

        let tray_res = tray::run_tray_loop(bridge);
        stop.store(true, Ordering::Release);
        let engine_res = match engine_handle.join() {
            Ok(r) => r,
            Err(_) => anyhow::bail!("pointer engine thread panicked"),
        };
        tray_res?;
        engine_res
    } else {
        run_engine(args, logger, stop, shutdown_tx, shutdown_rx, None)
    }
}

fn run_engine(
    args: Args,
    logger: Logger,
    cursor_stop: Arc<AtomicBool>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
    tray_io: Option<(Arc<TrayBridge>, mpsc::Receiver<TrayCommand>)>,
) -> Result<()> {
    let start = Instant::now();
    let (tx, rx) = mpsc::channel::<LinkEvent>();
    let bt_stop = Arc::clone(&cursor_stop);
    let bt_logger = logger.clone();
    let bt_shutdown_tx = shutdown_tx.clone();
    let clock = start;
    let adapter = args.adapter.clone();
    let device = args.device.clone();
    let replay = args.replay.clone();
    let mut trace = match &args.trace {
        Some(path) => Some(Trace::create(path)?),
        None => None,
    };

    let bt_thread = thread::Builder::new()
        .name("wear-pointer-bt".into())
        .spawn(move || -> Result<()> {
            if let Some(path) = replay {
                let res = replay_trace_file(&path, tx, clock, &bt_logger);
                bt_stop.store(true, Ordering::Release);
                return res;
            }
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(err) => {
                    bt_stop.store(true, Ordering::Release);
                    return Err(err).context("tokio runtime failed");
                }
            };
            rt.block_on(async move {
                let fallback_stop = Arc::clone(&bt_stop);
                let sig_logger = bt_logger.clone();
                tokio::spawn(async move {
                    wait_for_termination_signal().await;
                    sig_logger.line("wear-pointer: shutting down...");
                    let _ = bt_shutdown_tx.send(true);
                    tokio::time::sleep(Duration::from_millis(2500)).await;
                    fallback_stop.store(true, Ordering::Release);
                });
                let mut link = match CentralLink::new() {
                    Ok(link) => link
                        .with_adapter(adapter)
                        .with_device(device)
                        .with_shutdown(shutdown_rx),
                    Err(err) => {
                        bt_stop.store(true, Ordering::Release);
                        return Err(err);
                    }
                };
                let res = link.run(tx, clock).await;
                bt_stop.store(true, Ordering::Release);
                res
            })
        })
        .context("spawn bluetooth thread")?;

    let mut backend = open_backend(args.backend, args.config.screen, args.screen_explicit)?;
    let mut session = PointerSession::new(args.config);
    session.set_screen(backend.screen());
    let mut last_logged_dpi = session.effective_dpi();
    let mut last_logged_mode = session.effective_mode();
    let mut last_cursor_sync_s = f64::NEG_INFINITY;
    let mut last_remote_note: Option<ControlMode> = None;
    let frame = Duration::from_secs_f64(1.0 / f64::from(CURSOR_HZ));
    let mut next = Instant::now();
    let mut link_epoch = 0u64;
    let mut connected_peer_label: Option<String> = None;
    let mut latest_rx_hz = 0.0f64;
    let mut last_tray_push_s = f64::NEG_INFINITY;

    let pairing_note = if args.device.is_some() {
        "Device filter active."
    } else {
        "No --device filter set (accepts first matching watch)."
    };
    logger.line(format!(
        "wear-pointer: cursor loop at {CURSOR_HZ} Hz ({} DPI, reach ±{:.1}°, {} mode{}). {} {pairing_note}",
        last_logged_dpi,
        reach_degrees(session.effective_speed()),
        last_logged_mode.label(),
        if session.mode_override().is_some() {
            ", forced by desktop"
        } else {
            "; the watch can switch it"
        },
        if tray_io.is_some() {
            "Tray/Menu Bar active."
        } else {
            "Ctrl-C to quit."
        },
    ));

    loop {
        if let Some((_, ref cmd_rx)) = tray_io {
            while let Ok(cmd) = cmd_rx.try_recv() {
                let now = start.elapsed().as_secs_f64();
                match cmd {
                    TrayCommand::SetPaused(p) => {
                        for action in session.set_local_paused(p) {
                            if let Some(trace) = trace.as_mut() {
                                trace.action(now, &action);
                            }
                            apply(&mut *backend, action)?;
                        }
                        logger.line(format!(
                            "wear-pointer: pointer {}",
                            if p {
                                "paused from tray"
                            } else {
                                "resumed from tray"
                            }
                        ));
                        last_tray_push_s = f64::NEG_INFINITY;
                    }
                    TrayCommand::SetModeOverride(mode) => {
                        session.set_mode_override(mode);
                        last_remote_note = None;
                        logger.line(match mode {
                            Some(m) => format!(
                                "wear-pointer: mode override -> {} (desktop tray)",
                                mode_blurb(m)
                            ),
                            None => format!(
                                "wear-pointer: mode override cleared -> following watch ({})",
                                session.effective_mode().label()
                            ),
                        });
                        last_tray_push_s = f64::NEG_INFINITY;
                    }
                    TrayCommand::SetDpiOverride(dpi) => {
                        session.set_dpi_override(dpi);
                        logger.line(match dpi {
                            Some(d) => {
                                format!("wear-pointer: DPI override -> {d} DPI (desktop tray)")
                            }
                            None => format!(
                                "wear-pointer: DPI override cleared -> following watch ({} DPI)",
                                session.effective_dpi()
                            ),
                        });
                        last_tray_push_s = f64::NEG_INFINITY;
                    }
                    TrayCommand::Recenter => {
                        for action in session.center_now() {
                            if let Some(trace) = trace.as_mut() {
                                trace.action(now, &action);
                            }
                            apply(&mut *backend, action)?;
                        }
                        logger.line("wear-pointer: laser re-centered from desktop tray");
                    }
                    TrayCommand::Quit => {
                        logger.line("wear-pointer: quit requested from tray");
                        let _ = shutdown_tx.send(true);
                        cursor_stop.store(true, Ordering::Release);
                    }
                }
            }
        }

        let stopping = cursor_stop.load(Ordering::Acquire);
        let screen = backend.screen();
        if screen != session.screen() {
            session.set_screen(screen);
        }
        while let Ok(event) = rx.try_recv() {
            if let Some(trace) = trace.as_mut() {
                trace.event(start.elapsed().as_secs_f64(), &event);
            }
            let actions = match event {
                LinkEvent::Connected { epoch, device } => {
                    link_epoch = epoch;
                    connected_peer_label = Some(device.clone());
                    last_tray_push_s = f64::NEG_INFINITY;
                    logger.line(format!("wear-pointer: link up epoch={epoch} {device}"));
                    session.on_connected()
                }
                LinkEvent::Disconnected { epoch, reason } => {
                    if epoch == link_epoch {
                        link_epoch = 0;
                        connected_peer_label = None;
                        latest_rx_hz = 0.0;
                        last_tray_push_s = f64::NEG_INFINITY;
                    }
                    logger.line(format!("wear-pointer: link down epoch={epoch} ({reason})"));
                    session.on_disconnected()
                }
                LinkEvent::Packet {
                    epoch,
                    received_at_s,
                    bytes,
                } if epoch == link_epoch => session.push_packet(&bytes, received_at_s),
                LinkEvent::Packet { epoch, .. } => {
                    logger.line(format!(
                        "wear-pointer: dropped packet from epoch {epoch}, live is {link_epoch}"
                    ));
                    Vec::new()
                }
            };
            for action in actions {
                if let Some(trace) = trace.as_mut() {
                    trace.action(start.elapsed().as_secs_f64(), &action);
                }
                apply(&mut *backend, action)?;
            }
            let cur_dpi = session.effective_dpi();
            if cur_dpi != last_logged_dpi {
                last_logged_dpi = cur_dpi;
                last_tray_push_s = f64::NEG_INFINITY;
                if let Some(trace) = trace.as_mut() {
                    trace.dpi(start.elapsed().as_secs_f64(), cur_dpi);
                }
                logger.line(format!(
                    "wear-pointer: sensitivity -> {cur_dpi} DPI (speed {:.2}x, reach ±{:.1}°)",
                    session.effective_speed(),
                    reach_degrees(session.effective_speed()),
                ));
            }
            let cur_mode = session.effective_mode();
            if cur_mode != last_logged_mode {
                last_logged_mode = cur_mode;
                last_tray_push_s = f64::NEG_INFINITY;
                if let Some(trace) = trace.as_mut() {
                    trace.mode(start.elapsed().as_secs_f64(), cur_mode.label());
                }
                logger.line(format!(
                    "wear-pointer: pointing mode -> {}",
                    mode_blurb(cur_mode)
                ));
            } else if let (Some(forced), Some(remote)) =
                (session.mode_override(), session.remote_mode())
            {
                if remote != forced && remote != last_remote_note.unwrap_or(forced) {
                    logger.line(format!(
                        "wear-pointer: watch asked for {} mode; staying in {} (desktop override)",
                        remote.label(),
                        forced.label()
                    ));
                }
                last_remote_note = Some(remote);
            }
        }
        if stopping {
            let _ = shutdown_tx.send(true);
            if link_epoch != 0 {
                let now = start.elapsed().as_secs_f64();
                for action in session.on_disconnected() {
                    if let Some(trace) = trace.as_mut() {
                        trace.action(now, &action);
                    }
                    apply(&mut *backend, action)?;
                }
            }
            if let Some(trace) = trace.as_mut() {
                trace.flush_now();
            }
            break;
        }
        let now = start.elapsed().as_secs_f64();
        if session.wants_external_cursor() && now - last_cursor_sync_s >= CURSOR_SYNC_INTERVAL_S {
            last_cursor_sync_s = now;
            if let Some(point) = backend.cursor_position() {
                session.sync_external_cursor(point, now);
            }
        }
        let (actions, debug) = session.tick(now);
        for action in actions {
            if let Some(trace) = trace.as_mut() {
                trace.action(now, &action);
            }
            apply(&mut *backend, action)?;
        }
        if let Some(trace) = trace.as_mut() {
            trace.flush_if_due(now);
        }
        if let Some(ref dbg) = debug {
            latest_rx_hz = dbg.rx_hz;
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
                logger.line(format!(
                    "wear-pointer debug: rx={:.1}/s loss_window={} ({:.1}%) latency_ms avg={:.1} min={:.1} max={:.1} dpi={} mode={} cursor={cursor}{remote}",
                    debug.rx_hz,
                    debug.lost_window,
                    debug.loss_pct_window,
                    debug.latency_avg_ms,
                    debug.latency_min_ms,
                    debug.latency_max_ms,
                    debug.effective_dpi,
                    debug.mode.label(),
                ));
            }
        }

        if let Some((ref bridge, _)) = tray_io {
            if now - last_tray_push_s >= 0.25 {
                last_tray_push_s = now;
                let info = session.remote_info();
                let label = info
                    .map(|i| i.name.clone())
                    .filter(|s| !s.is_empty())
                    .or_else(|| connected_peer_label.clone());
                bridge.update(TraySnapshot {
                    connected: link_epoch != 0,
                    device_label: label,
                    battery_pct: info.and_then(|i| i.battery_pct),
                    charging: info.map(|i| i.charging).unwrap_or(false),
                    paused: session.is_paused(),
                    effective_mode: session.effective_mode(),
                    mode_override: session.mode_override(),
                    effective_dpi: session.effective_dpi(),
                    dpi_override: session.dpi_override(),
                    rx_hz: latest_rx_hz,
                    accessibility_ok: true,
                });
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
    match bt_thread.join() {
        Ok(res) => res?,
        Err(_) => anyhow::bail!("bluetooth worker thread panicked"),
    }
    Ok(())
}

async fn wait_for_termination_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut sigterm) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = sigterm.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// `--trace FILE`: one line per link event, packet, and cursor action, with
/// seconds since start. The desktop harness (tools/harness) reads it.
struct Trace {
    out: BufWriter<File>,
    last_flush_s: f64,
}

impl Trace {
    fn create(path: &str) -> Result<Self> {
        let file = File::create(path).with_context(|| format!("--trace {path}"))?;
        let mut out = BufWriter::new(file);
        let unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        writeln!(out, "# start_unix {unix:.6}")?;
        Ok(Self {
            out,
            last_flush_s: 0.0,
        })
    }

    fn event(&mut self, t: f64, event: &LinkEvent) {
        let line = match event {
            LinkEvent::Connected { epoch, device } => format!("{t:.6} connected {epoch} {device}"),
            LinkEvent::Disconnected { epoch, reason } => {
                format!("{t:.6} disconnected {epoch} {reason}")
            }
            LinkEvent::Packet {
                epoch,
                received_at_s,
                bytes,
            } => {
                let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                format!("{received_at_s:.6} rx {epoch} {hex}")
            }
        };
        let _ = writeln!(self.out, "{line}");
    }

    fn action(&mut self, t: f64, action: &CursorAction) {
        let line = match action {
            CursorAction::Move { x, y } => format!("{t:.6} move {x:.1} {y:.1}"),
            CursorAction::Button { button, down } => {
                format!(
                    "{t:.6} button {button:?} {}",
                    if *down { "down" } else { "up" }
                )
            }
            CursorAction::Scroll { delta_px } => format!("{t:.6} scroll {delta_px}"),
        };
        let _ = writeln!(self.out, "{line}");
    }

    fn dpi(&mut self, t: f64, dpi: u16) {
        let _ = writeln!(self.out, "{t:.6} dpi {dpi}");
    }

    fn mode(&mut self, t: f64, mode: &str) {
        let _ = writeln!(self.out, "{t:.6} mode {mode}");
    }

    fn flush_if_due(&mut self, t: f64) {
        if t - self.last_flush_s >= 0.2 {
            let _ = self.out.flush();
            self.last_flush_s = t;
        }
    }

    fn flush_now(&mut self) {
        let _ = self.out.flush();
    }
}

/// How often the relative modes ask the OS where the cursor is.
const CURSOR_SYNC_INTERVAL_S: f64 = 1.0 / 30.0;

fn mode_blurb(mode: ControlMode) -> &'static str {
    match mode {
        ControlMode::Absolute => {
            "absolute (laser: the cursor goes where the watch points; Center re-aims)"
        }
        ControlMode::Air => {
            "air (air mouse: wrist motion moves the cursor, faster turns go farther; Pause is the clutch)"
        }
        ControlMode::Touch => {
            "touch (touchpad: slide a finger on the watch to move; tap clicks, hold drags)"
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

fn open_backend(
    kind: BackendKind,
    screen: Screen,
    screen_explicit: bool,
) -> Result<Box<dyn PointerBackend>> {
    match kind {
        BackendKind::Log => Ok(Box::new(LogBackend::new(screen))),
        BackendKind::Hyprland => {
            #[cfg(target_os = "linux")]
            {
                let _ = screen_explicit;
                Ok(Box::new(HyprlandBackend::connect()?))
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = screen_explicit;
                Err(anyhow::anyhow!("the hyprland backend is Linux-only"))
            }
        }
        BackendKind::Macos => {
            #[cfg(target_os = "macos")]
            {
                let override_screen = screen_explicit.then_some(screen);
                Ok(Box::new(MacosBackend::open(override_screen)))
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (screen, screen_explicit);
                Err(anyhow::anyhow!("the macos backend is macOS-only"))
            }
        }
        BackendKind::Uinput => {
            #[cfg(all(feature = "uinput", target_os = "linux"))]
            {
                let _ = screen_explicit;
                Ok(Box::new(UinputBackend::open(screen)?))
            }
            #[cfg(not(all(feature = "uinput", target_os = "linux")))]
            {
                let _ = (screen, screen_explicit);
                Err(anyhow::anyhow!(
                    "uinput is not in this build. From receiver/: cargo build -p wear-pointer --features uinput"
                ))
            }
        }
        BackendKind::Windows => {
            #[cfg(target_os = "windows")]
            {
                let override_screen = screen_explicit.then_some(screen);
                Ok(Box::new(WindowsBackend::open(override_screen)))
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = (screen, screen_explicit);
                Err(anyhow::anyhow!("the windows backend is Windows-only"))
            }
        }
    }
}

fn default_backend() -> BackendKind {
    if cfg!(target_os = "macos") {
        BackendKind::Macos
    } else if cfg!(target_os = "windows") {
        BackendKind::Windows
    } else if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        BackendKind::Hyprland
    } else {
        BackendKind::Log
    }
}

fn is_macos_finder_launch(had_user_args: bool) -> bool {
    #[cfg(target_os = "macos")]
    {
        if had_user_args || std::env::var_os("WEAR_POINTER_CLI").is_some() {
            return false;
        }
        // When launched via Finder / Spotlight / Dock, stderr is not a TTY and
        // the executable lives inside `WearPointer.app/Contents/MacOS/`.
        let in_app_bundle = std::env::current_exe()
            .map(|p| p.to_string_lossy().contains(".app/Contents/MacOS/"))
            .unwrap_or(false);
        let stderr_not_tty = unsafe { libc::isatty(2) == 0 };
        in_app_bundle && stderr_not_tty
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = had_user_args;
        false
    }
}

fn parse_args(default_app_mode: bool) -> Result<Args> {
    let raw: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with("-psn_"))
        .collect();
    let had_user_args = !raw.is_empty();
    let mut args = raw.into_iter();

    let mut debug = false;
    let mut tray_flag: Option<bool> = None;
    let mut adapter = None;
    let mut device = None;
    let mut trace = None;
    let mut replay = None;
    let mut role = Role::Central;
    let mut backend = default_backend();
    let mut screen_explicit = false;
    let mut config = SessionConfig::default();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("wear-pointer {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--debug" => debug = true,
            "--tray" => tray_flag = Some(true),
            "--no-tray" => tray_flag = Some(false),
            "--adapter" => adapter = Some(args.next().context("--adapter NAME, for example hci1")?),
            "--device" => {
                device = Some(
                    args.next()
                        .context("--device MATCH (address, UUID, or local name substring)")?,
                )
            }
            "--trace" => trace = Some(args.next().context("--trace FILE")?),
            "--replay" => replay = Some(args.next().context("--replay FILE")?),
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
                    Some("macos") => BackendKind::Macos,
                    Some("uinput") => BackendKind::Uinput,
                    Some("windows") => BackendKind::Windows,
                    other => {
                        anyhow::bail!(
                            "--backend expects hyprland, macos, windows, log, or uinput, got {other:?}"
                        )
                    }
                };
            }
            "--dpi" => {
                let dpi_val = number(&mut args, "--dpi")?;
                anyhow::ensure!(
                    (100.0..=6400.0).contains(&dpi_val),
                    "--dpi must be between 100 and 6400, got {dpi_val}"
                );
                let speed = dpi_to_speed(dpi_val.round() as u16);
                config.speed = speed;
                config.cli_speed_override = Some(speed);
            }
            "--speed" => {
                let speed = number(&mut args, "--speed")?;
                anyhow::ensure!(speed > 0.0 && speed <= 6.4, "--speed must be in (0, 6.4]");
                config.speed = speed;
                config.cli_speed_override = Some(speed);
            }
            "--steady" => config.steady_hz = number(&mut args, "--steady")?,
            "--quick" => config.quickness = number(&mut args, "--quick")?,
            "--dead-zone" => config.dead_zone_deg = number(&mut args, "--dead-zone")?,
            "--mode" => {
                let value = args.next().context("--mode absolute|air|touch")?;
                config.mode_override = Some(ControlMode::parse(&value).with_context(|| {
                    format!("--mode expects absolute, air, or touch, got {value:?}")
                })?);
            }
            "--screen" => {
                let value = args.next().context("--screen WIDTH x HEIGHT")?;
                config.screen = parse_screen(&value)?;
                screen_explicit = true;
            }
            other if other.starts_with("--") => anyhow::bail!("unknown argument {other}"),
            other => {
                if let Ok(speed) = other.parse::<f64>() {
                    config.speed = speed;
                    config.cli_speed_override = Some(speed);
                } else if let Some(value) = other.strip_prefix("dpi=") {
                    let dpi: u16 = value.parse().context("dpi=")?;
                    let speed = dpi_to_speed(dpi);
                    config.speed = speed;
                    config.cli_speed_override = Some(speed);
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

    let auto_finder_tray = is_macos_finder_launch(had_user_args);
    let tray = tray_flag.unwrap_or(default_app_mode || auto_finder_tray);
    let file_log = default_app_mode || auto_finder_tray || tray_flag == Some(true);

    Ok(Args {
        debug,
        tray,
        file_log,
        adapter,
        device,
        trace,
        replay,
        role,
        backend,
        screen_explicit,
        config,
    })
}

fn decode_hex(hex: &str) -> Result<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        anyhow::bail!("odd hex length {}", hex.len());
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    let bytes = hex.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let s = std::str::from_utf8(&bytes[i..i + 2]).context("invalid utf8 hex")?;
        out.push(u8::from_str_radix(s, 16).with_context(|| format!("invalid hex byte {s:?}"))?);
    }
    Ok(out)
}

fn parse_trace_link_event(line: &str) -> Result<Option<(f64, LinkEvent)>> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Ok(None);
    }
    let mut parts = trimmed.splitn(4, ' ');
    let Some(ts_str) = parts.next() else {
        return Ok(None);
    };
    let Some(kind) = parts.next() else {
        return Ok(None);
    };
    let ts: f64 = ts_str
        .parse()
        .with_context(|| format!("invalid trace timestamp {ts_str:?}"))?;
    match kind {
        "connected" => {
            let epoch: u64 = parts
                .next()
                .context("connected missing epoch")?
                .parse()
                .context("invalid epoch")?;
            let device = parts.next().unwrap_or("watch").to_string();
            Ok(Some((ts, LinkEvent::Connected { epoch, device })))
        }
        "disconnected" => {
            let epoch: u64 = parts
                .next()
                .context("disconnected missing epoch")?
                .parse()
                .context("invalid epoch")?;
            let reason = parts.next().unwrap_or("disconnected").to_string();
            Ok(Some((ts, LinkEvent::Disconnected { epoch, reason })))
        }
        "rx" => {
            let epoch: u64 = parts
                .next()
                .context("rx missing epoch")?
                .parse()
                .context("invalid epoch")?;
            let hex = parts.next().context("rx missing hex payload")?;
            let bytes = decode_hex(hex.trim())?;
            Ok(Some((
                ts,
                LinkEvent::Packet {
                    epoch,
                    received_at_s: ts,
                    bytes,
                },
            )))
        }
        _ => Ok(None),
    }
}

fn replay_trace_file(
    path: &str,
    tx: mpsc::Sender<LinkEvent>,
    start: Instant,
    logger: &Logger,
) -> Result<()> {
    let file = File::open(path).with_context(|| format!("--replay {path}"))?;
    let reader = BufReader::new(file);
    let mut base_ts: Option<f64> = None;
    logger.line(format!("wear-pointer: replaying trace {path}"));
    for line in reader.lines() {
        let line = line?;
        let Some((ts, mut event)) = parse_trace_link_event(&line)? else {
            continue;
        };
        let origin = *base_ts.get_or_insert(ts);
        let rel = (ts - origin).max(0.0);
        let target = Duration::from_secs_f64(rel);
        let elapsed = start.elapsed();
        if target > elapsed {
            thread::sleep(target - elapsed);
        }
        if let LinkEvent::Packet { received_at_s, .. } = &mut event {
            *received_at_s = start.elapsed().as_secs_f64();
        }
        if tx.send(event).is_err() {
            break;
        }
    }
    thread::sleep(Duration::from_millis(80));
    Ok(())
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
    println!(
        "\
wear-pointer {version} — desktop receiver & menu-bar / system-tray app

  wear-pointer [--tray | --no-tray] [--debug] [--role central|peripheral]
               [--backend hyprland|macos|windows|log|uinput]
               [--dpi {DEFAULT_DPI}] [--speed 1.6] [--steady 0.8] [--quick 0.01] [--dead-zone 0.0]
               [--screen 1920x1080] [--adapter hci1] [--device MATCH] [--trace FILE] [--replay FILE]
               [--mode absolute|air|touch] [--version]

Consumer GUI mode (Menu Bar on macOS, System Tray on Windows/Linux):
  - macOS:   Double-click WearPointer.app (or run `wear-pointer --tray`)
  - Windows: Double-click WearPointer.exe (no console window; lives in System Tray)
  - Linux:   Run `wear-pointer-app` (or `install-linux.sh` to enable the user service)

Developer / Terminal CLI mode:
  wear-pointer --debug --backend log    # verify BLE link & packet stats without moving cursor
  wear-pointer --debug                  # live cursor loop with 1 Hz telemetry in terminal

--tray / --no-tray explicitly enables or disables the Menu Bar / System Tray icon.
--dpi sets pointer sensitivity in DPI (default: {DEFAULT_DPI} DPI = ±15.6° reach; 1000 DPI = ±25°).
      Also adjustable live from the watch UI or desktop tray menu (400..3200 DPI).
--speed is WatchPointer's multiplier alias (speed = dpi / 1000).
--mode forces a pointing model; without it the watch chooses (Settings sheet → Mode):
      absolute  laser pointer: the cursor goes where the watch points (default)
      air       air mouse: wrist motion moves the cursor with acceleration, no Center needed
      touch     touchpad: slide a finger on the watch screen
--dead-zone is degrees of aim to ignore. Leave it at 0 unless a log shows jitter.
--adapter picks a Bluetooth adapter by name (default: the first one).
--device restricts connections to a peripheral whose ID, BD_ADDR, or local name matches MATCH.
--trace writes every link event, packet, and cursor action to FILE (tools/harness reads it).
--replay replays link events and raw BLE packets from a --trace FILE through the selected backend.
",
        version = env!("CARGO_PKG_VERSION"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_trace_connected_rx_and_ignores_actions() {
        assert!(parse_trace_link_event("# start_unix 1794705977.698")
            .unwrap()
            .is_none());
        assert!(parse_trace_link_event("4.2 move 960.0 540.0")
            .unwrap()
            .is_none());

        let (t, ev) = parse_trace_link_event("1.25 connected 1 SM-L310 (hci0)")
            .unwrap()
            .unwrap();
        assert!((t - 1.25).abs() < 1e-9);
        assert!(matches!(ev, LinkEvent::Connected { epoch: 1, .. }));

        let (t2, ev2) = parse_trace_link_event("2.50 rx 1 01030500aabbccdd")
            .unwrap()
            .unwrap();
        assert!((t2 - 2.50).abs() < 1e-9);
        match ev2 {
            LinkEvent::Packet { epoch, bytes, .. } => {
                assert_eq!(epoch, 1);
                assert_eq!(bytes, vec![0x01, 0x03, 0x05, 0x00, 0xaa, 0xbb, 0xcc, 0xdd]);
            }
            _ => panic!("expected packet"),
        }
    }
}
