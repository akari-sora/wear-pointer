//! Multi-monitor desktop geometry and target selection.
//!
//! Coordinates are in the platform's global desktop space (`y` grows downward).
//! Secondary monitors to the left of or above the primary monitor legitimately
//! have negative `x` / `y` origins.

use crate::project::Screen;

/// Axis-aligned rectangle in global desktop coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width: width.max(1.0),
            height: height.max(1.0),
        }
    }

    pub fn from_screen(screen: Screen) -> Self {
        Self::new(0.0, 0.0, screen.width, screen.height)
    }

    pub fn to_screen(self) -> Screen {
        Screen::new(self.width, self.height)
    }

    pub fn center(self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// Inclusive maximum addressable pixel coordinate along X.
    pub fn max_x_px(self) -> f64 {
        (self.x + self.width - 1.0).max(self.x)
    }

    /// Inclusive maximum addressable pixel coordinate along Y.
    pub fn max_y_px(self) -> f64 {
        (self.y + self.height - 1.0).max(self.y)
    }

    /// Half-open `[x, x + width) × [y, y + height)` membership check so
    /// side-by-side monitors (e.g. `[-1512, 0)` and `[0, 1920)`) touch with
    /// zero seam gap in either direction.
    pub fn contains_half_open(self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }

    /// Closed `[x, x + width] × [y, y + height]` membership check.
    pub fn contains_closed(self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }

    /// Clamps `(px, py)` into continuous `[x, x + width] × [y, y + height]`.
    pub fn clamp_continuous(self, px: f64, py: f64) -> (f64, f64) {
        (
            px.clamp(self.x, self.x + self.width),
            py.clamp(self.y, self.y + self.height),
        )
    }

    /// Clamps `(px, py)` into the closed pixel-safe interior `[x, x + width - 1]`.
    pub fn clamp_px(self, px: f64, py: f64) -> (f64, f64) {
        (
            px.clamp(self.x, self.max_x_px()),
            py.clamp(self.y, self.max_y_px()),
        )
    }

    /// Squared Euclidean distance from `(px, py)` to this rectangle (`0.0` inside).
    pub fn distance_sq_to(self, px: f64, py: f64) -> f64 {
        let dx = if px < self.x {
            self.x - px
        } else if px > self.x + self.width {
            px - (self.x + self.width)
        } else {
            0.0
        };
        let dy = if py < self.y {
            self.y - py
        } else if py > self.y + self.height {
            py - (self.y + self.height)
        } else {
            0.0
        };
        dx * dx + dy * dy
    }
}

/// One physical or logical display in the desktop layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    pub id: u32,
    pub name: String,
    pub bounds: Rect,
    pub primary: bool,
}

impl Display {
    pub fn new(id: u32, name: impl Into<String>, bounds: Rect, primary: bool) -> Self {
        Self {
            id,
            name: name.into(),
            bounds,
            primary,
        }
    }

    /// Human-readable label for Menu Bar / System Tray items.
    pub fn menu_label(
        &self,
        index_1based: usize,
        primary_center: (f64, f64),
        total: usize,
    ) -> String {
        let res = format!("{:.0}×{:.0}", self.bounds.width, self.bounds.height);
        let trimmed = self.name.trim();
        if total <= 1 {
            if trimmed.is_empty()
                || trimmed.eq_ignore_ascii_case("main")
                || trimmed.eq_ignore_ascii_case("primary")
            {
                return format!("Display {index_1based} ({res})");
            }
            return format!("Display {index_1based}: {trimmed} ({res})");
        }

        if self.primary {
            if trimmed.is_empty()
                || trimmed.eq_ignore_ascii_case("main")
                || trimmed.eq_ignore_ascii_case("primary")
            {
                format!("Display {index_1based}: Main ({res})")
            } else {
                format!("Display {index_1based}: {trimmed} (Main · {res})")
            }
        } else {
            let (cx, cy) = self.bounds.center();
            let dx = cx - primary_center.0;
            let dy = cy - primary_center.1;
            let pos = if dx.abs() >= dy.abs() {
                if dx < -1.0 {
                    "Left"
                } else if dx > 1.0 {
                    "Right"
                } else {
                    ""
                }
            } else if dy < -1.0 {
                "Above"
            } else if dy > 1.0 {
                "Below"
            } else {
                ""
            };
            let suffix = if pos.is_empty() {
                res
            } else {
                format!("{res} · {pos}")
            };
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("external") {
                format!("Display {index_1based}: External ({suffix})")
            } else {
                format!("Display {index_1based}: {trimmed} ({suffix})")
            }
        }
    }
}

/// How absolute (`LASER`) mode maps onto a multi-monitor desktop.
///
/// Relative modes (`AIR` and `TOUCH`) always traverse all displays seamlessly.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DisplayTarget {
    /// Default: anchors `Center` and 1× angular sensitivity to the active
    /// display, while allowing wrist turns past the edge to continue seamlessly
    /// onto adjacent monitors.
    #[default]
    Auto,
    /// Treats the union bounding box of all displays as a single wide canvas.
    SpanAll,
    /// Locks `LASER` strictly to the display at 0-based index `idx` (ideal for
    /// slide presentations on a projector / secondary screen).
    Monitor(usize),
    /// Locks `LASER` to a display matched by CLI name (`DP-1`, `Built-in`, `primary`, …).
    Named(String),
}

impl DisplayTarget {
    pub fn parse(spec: &str) -> Result<Self, String> {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return Err("empty --display value".to_string());
        }
        let lower = trimmed.to_ascii_lowercase();
        match lower.as_str() {
            "auto" | "active" | "follow" => Ok(Self::Auto),
            "all" | "span" | "virtual" => Ok(Self::SpanAll),
            "primary" | "main" => Ok(Self::Named("primary".to_string())),
            _ => {
                if let Ok(index_1based) = trimmed.parse::<usize>() {
                    if index_1based == 0 {
                        return Err(
                            "--display monitor numbers are 1-based (use 1 for Display 1, or 'auto')"
                                .to_string(),
                        );
                    }
                    Ok(Self::Monitor(index_1based - 1))
                } else {
                    Ok(Self::Named(trimmed.to_string()))
                }
            }
        }
    }

    pub fn short_label(&self, layout: &DesktopLayout) -> String {
        match self {
            Self::Auto => "Auto".to_string(),
            Self::SpanAll => "All Displays".to_string(),
            Self::Monitor(idx) => format!("Display {}", idx + 1),
            Self::Named(spec) => match layout.resolve_named(spec) {
                Some(idx) => format!("Display {}", idx + 1),
                None => spec.clone(),
            },
        }
    }
}

/// Non-empty set of monitors forming the desktop coordinate space.
#[derive(Clone, Debug, PartialEq)]
pub struct DesktopLayout {
    displays: Vec<Display>,
    virtual_bounds: Rect,
    primary_idx: usize,
}

impl Default for DesktopLayout {
    fn default() -> Self {
        Self::single(Screen::new(1920.0, 1080.0))
    }
}

impl DesktopLayout {
    pub fn single(screen: Screen) -> Self {
        let bounds = Rect::from_screen(screen);
        Self {
            displays: vec![Display::new(0, "Main", bounds, true)],
            virtual_bounds: bounds,
            primary_idx: 0,
        }
    }

    pub fn new(mut displays: Vec<Display>) -> Self {
        displays.retain(|d| d.bounds.width.is_finite() && d.bounds.height.is_finite());
        if displays.is_empty() {
            return Self::default();
        }
        let primary_idx = displays.iter().position(|d| d.primary).unwrap_or(0);
        if !displays.iter().any(|d| d.primary) {
            displays[0].primary = true;
        }
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for d in &displays {
            min_x = min_x.min(d.bounds.x);
            min_y = min_y.min(d.bounds.y);
            max_x = max_x.max(d.bounds.x + d.bounds.width);
            max_y = max_y.max(d.bounds.y + d.bounds.height);
        }
        let virtual_bounds = Rect::new(min_x, min_y, max_x - min_x, max_y - min_y);
        Self {
            displays,
            virtual_bounds,
            primary_idx,
        }
    }

    pub fn displays(&self) -> &[Display] {
        &self.displays
    }

    pub fn len(&self) -> usize {
        self.displays.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn is_multi(&self) -> bool {
        self.displays.len() > 1
    }

    pub fn virtual_bounds(&self) -> Rect {
        self.virtual_bounds
    }

    pub fn primary_index(&self) -> usize {
        self.primary_idx
    }

    pub fn primary_display(&self) -> &Display {
        &self.displays[self.primary_idx]
    }

    /// Returns the index of the display containing `(x, y)`, or the nearest
    /// display if `(x, y)` lies outside all displays (e.g. in an L-shape notch).
    pub fn display_index_at(&self, x: f64, y: f64) -> usize {
        for (i, d) in self.displays.iter().enumerate() {
            if d.bounds.contains_half_open(x, y) {
                return i;
            }
        }
        let mut best_idx = self.primary_idx;
        let mut best_dist = f64::INFINITY;
        for (i, d) in self.displays.iter().enumerate() {
            let dist = d.bounds.distance_sq_to(x, y);
            if dist < best_dist {
                best_dist = dist;
                best_idx = i;
            }
        }
        best_idx
    }

    pub fn display_at(&self, x: f64, y: f64) -> &Display {
        &self.displays[self.display_index_at(x, y)]
    }

    /// Clamps `(x, y)` to the continuous union of active displays.
    ///
    /// Points inside any display's `[x, x + width] × [y, y + height]` pass
    /// through untouched so sub-pixel motion crosses monitor seams smoothly;
    /// points outside all displays snap to the nearest display's rectangle.
    pub fn clamp_point(&self, x: f64, y: f64) -> (f64, f64) {
        for d in &self.displays {
            if d.bounds.contains_closed(x, y) {
                return (x, y);
            }
        }
        let idx = self.display_index_at(x, y);
        self.displays[idx].bounds.clamp_continuous(x, y)
    }

    /// Clamps `(x, y)` for OS cursor injection: preserves sub-pixel coordinates
    /// inside any `[x, x + width) × [y, y + height)` display interior (including
    /// shared seams) while pulling outer right/bottom walls in to `width - 1`.
    pub fn clamp_pixel(&self, x: f64, y: f64) -> (f64, f64) {
        for d in &self.displays {
            if d.bounds.contains_half_open(x, y) {
                return (x, y);
            }
        }
        let idx = self.display_index_at(x, y);
        self.displays[idx].bounds.clamp_px(x, y)
    }

    /// Resolves a CLI display name (`primary`, exact name, or case-insensitive substring).
    pub fn resolve_named(&self, spec: &str) -> Option<usize> {
        let needle = spec.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return None;
        }
        if needle == "primary" || needle == "main" {
            return Some(self.primary_idx);
        }
        if let Some(i) = self
            .displays
            .iter()
            .position(|d| d.name.eq_ignore_ascii_case(spec.trim()))
        {
            return Some(i);
        }
        self.displays
            .iter()
            .position(|d| d.name.to_ascii_lowercase().contains(&needle))
    }

    /// Compact one-line summary for startup and hotplug logs.
    pub fn summary(&self) -> String {
        if self.displays.len() == 1 {
            let d = &self.displays[0];
            return format!(
                "1 display ({} {:.0}x{:.0}@({:.0},{:.0}))",
                if d.name.is_empty() { "Main" } else { &d.name },
                d.bounds.width,
                d.bounds.height,
                d.bounds.x,
                d.bounds.y
            );
        }
        let parts: Vec<String> = self
            .displays
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let tag = if d.primary { "*" } else { "" };
                let name = if d.name.is_empty() {
                    "Display"
                } else {
                    &d.name
                };
                format!(
                    "#{}{tag} {name} {:.0}x{:.0}@({:.0},{:.0})",
                    i + 1,
                    d.bounds.width,
                    d.bounds.height,
                    d.bounds.x,
                    d.bounds.y
                )
            })
            .collect();
        format!("{} displays [{}]", self.displays.len(), parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_three_monitors() -> DesktopLayout {
        DesktopLayout::new(vec![
            Display::new(1, "Main", Rect::new(0.0, 0.0, 3840.0, 2160.0), true),
            Display::new(
                2,
                "Built-in",
                Rect::new(-1512.0, 420.0, 1512.0, 982.0),
                false,
            ),
            Display::new(
                3,
                "External",
                Rect::new(3840.0, 200.0, 1366.0, 768.0),
                false,
            ),
        ])
    }

    #[test]
    fn virtual_bounds_and_negative_origin_lookup() {
        let layout = sample_three_monitors();
        let vb = layout.virtual_bounds();
        assert_eq!(vb, Rect::new(-1512.0, 0.0, 6718.0, 2160.0));
        assert_eq!(layout.primary_index(), 0);
        assert_eq!(layout.display_index_at(-800.0, 600.0), 1);
        assert_eq!(layout.display_index_at(100.0, 100.0), 0);
        assert_eq!(layout.display_index_at(4000.0, 400.0), 2);
    }

    #[test]
    fn subpixel_crossing_between_negative_and_zero_displays_has_no_gap() {
        let layout = sample_three_monitors();
        // Walk from (-1.2, 600.0) across x = 0.0 in 0.3 px steps: never snapped back!
        let mut x = -1.2;
        for _ in 0..10 {
            let (cx, cy) = layout.clamp_point(x, 600.0);
            assert!((cx - x).abs() < 1e-9, "got {cx} for {x}");
            assert!((cy - 600.0).abs() < 1e-9);
            x += 0.3;
        }
    }

    #[test]
    fn dead_corner_clamps_to_nearest_monitor_interior() {
        let layout = sample_three_monitors();
        // Above the shorter left Built-in monitor: (-1000, 100) is 320 px above Built-in (y=420..1402)
        // and 1000 px left of Main (x=0..3840). Nearest monitor is Built-in at (-1000, 420).
        assert_eq!(layout.clamp_point(-1000.0, 100.0), (-1000.0, 420.0));
        // Just 5 px left of Main near y = 50 (where Built-in does not reach): nearest is Main (0, 50).
        assert_eq!(layout.clamp_point(-5.0, 50.0), (0.0, 50.0));
        // Far right past External (3840..5206): continuous clamp is 5206.0, OS pixel clamp is 5205.0.
        assert_eq!(layout.clamp_point(6000.0, 400.0), (5206.0, 400.0));
        assert_eq!(layout.clamp_pixel(6000.0, 400.0), (5205.0, 400.0));
    }

    #[test]
    fn menu_labels_and_target_parsing() {
        let layout = sample_three_monitors();
        let pc = layout.primary_display().bounds.center();
        let d0 = layout.displays()[0].menu_label(1, pc, 3);
        let d1 = layout.displays()[1].menu_label(2, pc, 3);
        let d2 = layout.displays()[2].menu_label(3, pc, 3);
        assert_eq!(d0, "Display 1: Main (3840×2160)");
        assert_eq!(d1, "Display 2: Built-in (1512×982 · Left)");
        assert_eq!(d2, "Display 3: External (1366×768 · Right)");

        assert_eq!(DisplayTarget::parse("auto").unwrap(), DisplayTarget::Auto);
        assert_eq!(DisplayTarget::parse("ALL").unwrap(), DisplayTarget::SpanAll);
        assert_eq!(
            DisplayTarget::parse("2").unwrap(),
            DisplayTarget::Monitor(1)
        );
        assert!(DisplayTarget::parse("0").is_err());
        assert_eq!(layout.resolve_named("built-in"), Some(1));
        assert_eq!(layout.resolve_named("primary"), Some(0));
    }
}
