//! Prints cursor actions. Used for Stage 0 before pointing a real cursor,
//! and whenever Hyprland is not available.

use crate::PointerBackend;
use anyhow::Result;
use std::time::Instant;
use wear_pointer_core::{Button, DesktopLayout, Screen};

pub struct LogBackend {
    layout: DesktopLayout,
    last_move: Instant,
    moves: u64,
}

impl LogBackend {
    pub fn new(screen: Screen) -> Self {
        Self::with_layout(DesktopLayout::single(screen))
    }

    pub fn with_layout(layout: DesktopLayout) -> Self {
        Self {
            layout,
            last_move: Instant::now(),
            moves: 0,
        }
    }
}

impl PointerBackend for LogBackend {
    fn motion_absolute(&mut self, x: f64, y: f64) -> Result<()> {
        self.moves += 1;
        if self.last_move.elapsed().as_millis() >= 100 {
            self.last_move = Instant::now();
            eprintln!(
                "wear-pointer log: move {x:.0},{y:.0}  ({} moves)",
                self.moves
            );
        }
        Ok(())
    }

    fn button(&mut self, button: Button, down: bool) -> Result<()> {
        let name = match button {
            Button::Left => "left",
            Button::Right => "right",
        };
        let state = if down { "down" } else { "up" };
        eprintln!("wear-pointer log: {name} {state}");
        Ok(())
    }

    fn scroll(&mut self, delta_px: i32) -> Result<()> {
        eprintln!("wear-pointer log: scroll {delta_px}");
        Ok(())
    }

    fn screen(&self) -> Screen {
        self.layout.primary_display().bounds.to_screen()
    }

    fn layout(&mut self) -> DesktopLayout {
        self.layout.clone()
    }
}
