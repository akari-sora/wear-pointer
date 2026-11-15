//! Motion math and the pointer session.
//!
//! The aim integrator, laser projection, 1€ filter, and 240 Hz follow are a
//! reimplementation of WatchPointer (Copyright (c) 2026 Umar Afzal, MIT).
//! See the repository NOTICE.

mod aim;
mod filter;
mod layout;
mod packet;
mod project;
mod relative;
mod session;
mod stats;

pub use aim::{AimSample, AimTracker};
pub use filter::OneEuroFilter;
pub use layout::{DesktopLayout, Display, DisplayTarget, Rect};
pub use packet::{
    control, decode, encode, Packet, PacketError, BASE_DPI, CONTROL_REQUEST_STATS, CONTROL_UUID,
    CURSOR_HZ, DEFAULT_DPI, DPI_STEP, HEADER_LEN, HOLD_TO_DRAG_MS, KIND_AIM, KIND_CENTER,
    KIND_DRAG, KIND_INFO, KIND_LEFT_CLICK, KIND_MODE, KIND_PAUSE, KIND_RIGHT_CLICK, KIND_SCROLL,
    KIND_SENSITIVITY, KIND_STATS, KIND_TOUCH, MAX_DPI, MAX_PACKET_LEN, MIN_DPI, MODE_ABSOLUTE,
    MODE_AIR, MODE_TOUCH, POINTER_UUID, SENSOR_HZ, SERVICE_UUID, VERSION,
};
pub use project::{
    dpi_to_speed, project_aim, project_aim_rect, reach_degrees, speed_to_dpi, Screen,
};
pub use relative::{AirMouse, ControlMode, TouchPad};
pub use session::{Button, CursorAction, DebugSnapshot, PointerSession, SessionConfig, WatchInfo};
pub use stats::{LinkStats, Timing};

/// Cursor follow time constant, in seconds. About a 5 ms catch-up.
pub const FOLLOW_TAU_S: f64 = 0.005;
/// Ignore a cursor step smaller than this (pixels, L1). Once the cursor is
/// within a fraction of a pixel, further ticks stay quiet.
pub const MIN_MOVE_PX: f64 = 0.05;
/// Arrival gap after which the filters snap instead of gliding.
pub const FRESH_GAP_S: f64 = 0.3;
/// Watch dt above this is not fed to the 1€ filter (a dropped burst, or a pause).
pub const MAX_FILTER_DT_S: f64 = 0.2;
