//! Keeping the restored window reachable.
//!
//! The window comes back where it was left. After a monitor was unplugged, the scale changed or
//! the settings came from a larger screen, that can put its title bar above the top edge or
//! under the taskbar, out of reach of the mouse. [`fit`] pulls a window back into the monitor's
//! work area (the screen without the taskbar), in whatever unit the caller measures both, as
//! long as it is the same one.

/// A rectangle: left, top, width, height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Area {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Area { x, y, w, h }
    }
    fn right(self) -> f32 {
        self.x + self.w
    }
    fn bottom(self) -> f32 {
        self.y + self.h
    }
    fn sane(self) -> bool {
        [self.x, self.y, self.w, self.h].iter().all(|v| v.is_finite()) && self.w > 0.0 && self.h > 0.0
    }
}

/// How much of the title bar must stay over the work area at least, to grab it.
pub const REACHABLE: f32 = 100.0;
/// The size a window gets when its saved geometry is unusable, before it is fitted to the screen.
pub const DEFAULT_SIZE: (f32, f32) = (1600.0, 1000.0);

/// `window` moved and shrunk so that it can be reached on `work`: no larger than the work area
/// (but not below `min`), the title bar not above the work area's top, the window not below its
/// bottom, and at least [`REACHABLE`] of it over the work area horizontally. A window that is
/// fine is returned unchanged. Geometry that makes no sense (not finite, empty), or a work
/// area that does not, gives the default size centred on the work area.
pub fn fit(window: Area, work: Area, min: (f32, f32)) -> Area {
    if !work.sane() {
        return window;
    }
    let min = (min.0.min(work.w), min.1.min(work.h));
    if !window.sane() {
        let w = DEFAULT_SIZE.0.min(work.w * 0.9).max(min.0);
        let h = DEFAULT_SIZE.1.min(work.h * 0.9).max(min.1);
        return Area::new(work.x + (work.w - w) / 2.0, work.y + (work.h - h) / 2.0, w, h);
    }
    let w = window.w.clamp(min.0, work.w);
    let h = window.h.clamp(min.1, work.h);
    let reach = REACHABLE.min(w);
    // A window that had to shrink goes fully onto the screen; one that fits may keep straddling two.
    let x = if w < window.w { window.x.max(work.x).min(work.right() - w) } else { window.x.max(work.x + reach - w).min(work.right() - reach) };
    let y = window.y.min(work.bottom() - h).max(work.y);
    Area::new(x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Area = Area::new(0.0, 0.0, 2560.0, 1400.0);
    const MIN: (f32, f32) = (960.0, 600.0);

    #[test]
    fn a_window_on_screen_is_left_alone() {
        let w = Area::new(300.0, 120.0, 1600.0, 1000.0);
        assert_eq!(fit(w, WORK, MIN), w);
    }

    #[test]
    fn a_title_bar_above_the_top_or_under_the_taskbar_comes_back() {
        let fitted = fit(Area::new(300.0, -400.0, 1600.0, 1000.0), WORK, MIN);
        assert_eq!((fitted.y, fitted.h), (0.0, 1000.0));
        let fitted = fit(Area::new(300.0, 1300.0, 1600.0, 1000.0), WORK, MIN);
        assert_eq!(fitted.y + fitted.h, 1400.0);
        // A taskbar at the top moves the work area down.
        let below_bar = Area::new(0.0, 48.0, 2560.0, 1352.0);
        assert_eq!(fit(Area::new(300.0, 0.0, 1600.0, 1000.0), below_bar, MIN).y, 48.0);
    }

    #[test]
    fn a_window_from_a_monitor_that_is_gone_keeps_a_grabbable_part_on_screen() {
        let right = fit(Area::new(5000.0, 100.0, 1600.0, 1000.0), WORK, MIN);
        assert_eq!(right.x, 2560.0 - REACHABLE);
        let left = fit(Area::new(-4000.0, 100.0, 1600.0, 1000.0), WORK, MIN);
        assert_eq!(left.x + left.w, REACHABLE);
    }

    #[test]
    fn a_window_larger_than_the_screen_shrinks_to_it() {
        let small = Area::new(0.0, 0.0, 1280.0, 720.0);
        let fitted = fit(Area::new(-20.0, -30.0, 3200.0, 2000.0), small, MIN);
        assert_eq!(fitted, small);
        // Never below the minimum, unless the screen itself is smaller.
        assert_eq!(fit(Area::new(0.0, 0.0, 100.0, 100.0), WORK, MIN).w, 960.0);
        assert_eq!(fit(Area::new(0.0, 0.0, 100.0, 100.0), Area::new(0.0, 0.0, 800.0, 500.0), MIN).h, 500.0);
    }

    #[test]
    fn unusable_geometry_is_centred_at_a_sensible_size() {
        for bad in [Area::new(f32::NAN, 0.0, 1600.0, 1000.0), Area::new(0.0, 0.0, 0.0, 0.0), Area::new(0.0, 0.0, f32::INFINITY, 10.0)] {
            let fitted = fit(bad, WORK, MIN);
            assert_eq!(fitted, Area::new(480.0, 200.0, 1600.0, 1000.0));
        }
        let small = Area::new(100.0, 50.0, 1366.0, 728.0);
        let fitted = fit(Area::new(f32::NAN, 0.0, 1.0, 1.0), small, MIN);
        assert!(fitted.w <= small.w && fitted.h <= small.h && fitted.x >= small.x && fitted.y >= small.y);
        // No work area to go by: nothing to change.
        let w = Area::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(fit(w, Area::new(0.0, 0.0, 0.0, 0.0), MIN), w);
    }
}
