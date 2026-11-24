//! The 3D view's scale bar (#375): a line in the view's bottom left corner and the length it stands
//! for, a round number in the design's units, so a distance on screen can be read without
//! measuring. Off by default; the navigation bar's View menu turns it on (`view.scale_bar`), and
//! the app remembers the choice (`scale_bar`).
//!
//! The scale is exact at the camera's target, the point the view turns about, which is the centre
//! of the view. An orthographic view has that scale everywhere; in a perspective view nearer
//! things look larger, so the label says where the scale holds.
use egui::{Rect, Stroke, pos2, vec2};
use solidcraft_theme::Tokens;
use solidcraft_viewport::Camera;

use crate::SolveApp;
use crate::prefs::unit_scale;
use crate::viewport::Proj;

/// The bar to draw: its length on screen and the length it stands for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Bar {
    /// The bar's length in screen points.
    pub points: f32,
    /// The length it stands for, as written: a round number without its unit (`20`, `0.5`).
    pub value: String,
    /// The unit that number is in (`mm`, `in`).
    pub unit: &'static str,
}

/// Millimetres one screen point covers at the camera's target, for a view `view_height` points
/// high. `None` for a view with no height or a camera with no usable scale.
pub(crate) fn mm_per_point(cam: &Camera, view_height: f32) -> Option<f64> {
    let height = f64::from(view_height);
    let mm = 2.0 * cam.half_height() / height;
    (height > 0.0 && mm.is_finite() && mm > 0.0).then_some(mm)
}

/// The longest round length (1, 2 or 5 times a power of ten, in `units`) that fits in `longest`
/// points at `mm_per_point`. `None` when the scale is unusable or so extreme that no sensible
/// number describes it.
pub(crate) fn bar(mm_per_point: f64, units: &str, longest: f32) -> Option<Bar> {
    let (per_mm, unit) = unit_scale(units);
    let per_point = mm_per_point * per_mm;
    let room = per_point * f64::from(longest);
    if !(room.is_finite() && room > 0.0 && per_point > 0.0) {
        return None;
    }
    // `room` may come out a hair below an exact round length (999.99... for 1000), which must
    // still fit whole: a relative nudge covers that rounding.
    let room = room * (1.0 + 1e-9);
    let exponent = room.log10().floor();
    // Beyond a micrometre or a thousand kilometres the bar says nothing useful.
    if !(-6.0..=9.0).contains(&exponent) {
        return None;
    }
    let base = 10f64.powf(exponent);
    let step = [5.0, 2.0, 1.0].into_iter().map(|m| m * base).find(|v| *v <= room)?;
    let decimals = if exponent < 0.0 { (-exponent) as usize } else { 0 };
    Some(Bar { points: (step / per_point) as f32, value: format!("{step:.decimals$}"), unit })
}

/// The bar's label: the length, and in a perspective view where that length holds.
pub(crate) fn label(bar: &Bar, perspective: bool) -> String {
    let length = format!("{} {}", bar.value, bar.unit);
    if perspective { solidcraft_ui_text::format("view.scale_bar_at_centre", &[("length", &length)]) } else { length }
}

/// Draw the scale bar in the bottom left corner of `view`, moved clear of `taken` (panels, the
/// navigation bar, readouts and notices). Returns where it was drawn, or `None` when it is off
/// or has no room.
pub(crate) fn show(app: &SolveApp, ui: &egui::Ui, proj: &Proj, view: Rect, taken: &[Rect]) -> Option<Rect> {
    if !app.preferences.scale_bar || !view.is_finite() || !view.is_positive() {
        return None;
    }
    let t = Tokens::get();
    let mm = mm_per_point(&proj.cam, proj.rect.height())?;
    let bar = bar(mm, &app.session.doc.units, t.space_24 * 5.0)?;
    let painter = ui.painter().clone();
    let galley = painter.layout_no_wrap(label(&bar, proj.cam.fov > 0.0), egui::FontId::proportional(t.font_label), t.text);
    let pad = vec2(t.space_8, t.space_4);
    let tick = t.space_6;
    let size = vec2(pad.x * 2.0 + bar.points.max(galley.size().x), pad.y * 2.0 + galley.size().y + t.space_4 + tick);
    let mut r = Rect::from_min_size(pos2(view.left() + t.space_8, view.bottom() - t.space_8 - size.y), size);
    // As the quick measurement does from the other corner: each step clears one rectangle, a
    // tall one by moving right of it and a short one by moving above it.
    for _ in 0..=2 * taken.len() {
        let Some(o) = taken.iter().find(|o| o.expand(t.space_4).intersects(r)) else { break };
        r = if o.height() > view.height() * 0.5 {
            r.translate(vec2(o.right() + t.space_8 - r.left(), 0.0))
        } else {
            r.translate(vec2(0.0, o.top() - t.space_8 - r.bottom()))
        };
    }
    if !view.contains_rect(r) {
        return None;
    }
    crate::scenario::publish_rect("view:scale_bar", r);
    painter.rect_filled(r, t.radius_sm, t.overlay);
    painter.rect_stroke(r, t.radius_sm, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    painter.galley(r.min + pad, galley, t.text);
    let (left, y) = (r.left() + pad.x, r.bottom() - pad.y - tick * 0.5);
    let right = left + bar.points;
    let stroke = Stroke::new(1.5, t.text);
    painter.line_segment([pos2(left, y), pos2(right, y)], stroke);
    for x in [left, right] {
        painter.line_segment([pos2(x, y - tick * 0.5), pos2(x, y + tick * 0.5)], stroke);
    }
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar_of(mm_per_point: f64, units: &str) -> (String, &'static str, f32) {
        let b = bar(mm_per_point, units, 120.0).unwrap();
        (b.value, b.unit, b.points)
    }

    #[test]
    fn the_bar_is_the_longest_round_length_that_fits() {
        // 0.5 mm a point: 120 points hold 60 mm, so the bar is 50 mm, 100 points long.
        assert_eq!(bar_of(0.5, "mm"), ("50".into(), "mm", 100.0));
        // 120 points hold 24 mm: 20 mm.
        assert_eq!(bar_of(0.2, "mm"), ("20".into(), "mm", 100.0));
        // 120 points hold 14.4 mm: 10 mm.
        assert_eq!(bar_of(0.12, "mm"), ("10".into(), "mm", 1000.0 / 12.0));
        // Exactly a round length fits whole, despite log10 rounding.
        assert_eq!(bar_of(1000.0 / 120.0, "mm"), ("1000".into(), "mm", 120.0));
        for mm in [1e-4, 0.003, 0.07, 1.3, 42.0, 900.0, 12_345.0] {
            let b = bar(mm, "mm", 120.0).unwrap();
            assert!(b.points <= 120.0 + 1e-3 && b.points >= 120.0 / 2.5 - 1e-3, "{mm} mm a point gives {} points", b.points);
            let lead = b.value.trim_start_matches(['0', '.']).chars().next();
            assert!(matches!(lead, Some('1' | '2' | '5')), "{mm}: {} is a round length", b.value);
        }
    }

    #[test]
    fn the_length_is_in_the_designs_units() {
        // 1 inch is 25.4 mm: at 0.254 mm a point, 120 points hold 1.2 in, so the bar is 1 in.
        assert_eq!(bar_of(0.254, "in"), ("1".into(), "in", 100.0));
        // Small lengths keep the decimals they need, and no more.
        assert_eq!(bar_of(0.01, "in").0, "0.02");
        assert_eq!(bar_of(0.01, "cm").0, "0.1");
        assert_eq!(bar_of(5.0, "m").0, "0.5");
        assert_eq!(bar_of(100.0, "ft").0, "20");
        assert_eq!(bar_of(0.5, "unknown").1, "mm", "an unknown unit falls back to millimetres");
    }

    #[test]
    fn an_unusable_scale_draws_no_bar() {
        for mm in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e-12, 1e12] {
            assert_eq!(bar(mm, "mm", 120.0), None, "{mm}");
        }
        assert_eq!(bar(0.5, "mm", 0.0), None);
        assert_eq!(bar(0.5, "mm", f32::NAN), None);
    }

    #[test]
    fn the_scale_is_the_views_height_at_the_target() {
        let cam = Camera { distance: 250.0, fov: 0.0, ..Camera::default() };
        // An orthographic view shows 0.8 times the distance across its height.
        let mm = mm_per_point(&cam, 400.0).unwrap();
        assert!((mm - 0.5).abs() < 1e-12, "{mm}");
        let cam = Camera { distance: 100.0, fov: std::f64::consts::FRAC_PI_2, ..Camera::default() };
        // A 90 degree perspective shows twice the distance across its height at the target.
        assert!((mm_per_point(&cam, 200.0).unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(mm_per_point(&cam, 0.0), None);
        assert_eq!(mm_per_point(&Camera { distance: f64::NAN, ..cam }, 200.0), None);
    }

    /// Off by default; the View menu turns it on, and then the bar sits in the view's bottom left
    /// corner, above History and clear of the Browser. The choice survives a settings reload, and
    /// the `view.scale_bar` command turns it off again.
    #[test]
    fn the_view_menu_shows_the_bar_in_the_bottom_left_corner() {
        assert!(!crate::prefs::Prefs::default().scale_bar);
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", serde_json::json!({"length": 10, "width": 20, "height": 30})).unwrap();
        h.frames(3);
        assert!(crate::scenario::rect_of("view:scale_bar").is_none(), "off by default");
        h.step(&serde_json::json!({"click":{"handle":"nav:view"}})).unwrap();
        h.step(&serde_json::json!({"click":{"handle":"view:scale_bar_toggle"}})).unwrap();
        assert!(h.app.preferences.scale_bar, "the View menu turns it on");
        h.step(&serde_json::json!({"click":{"handle":"nav:view"}})).unwrap();
        let mut restored = crate::SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(restored.preferences.scale_bar, "turning it on survives a settings reload");
        h.frames(3);
        let r = crate::scenario::rect_of("view:scale_bar").expect("the bar is drawn");
        let screen = h.ctx.content_rect();
        assert!(r.center().x < screen.center().x && r.center().y > screen.center().y, "{r:?} in the bottom left of {screen:?}");
        if let Some(history) = h.app.shell.history {
            assert!(r.bottom() <= history.top(), "{r:?} above History {history:?}");
        }
        for covered in crate::shell::covered_rects(&h.app) {
            assert!(!covered.intersects(r), "{r:?} clear of {covered:?}");
        }
        let reply = h.call("engine.execute", serde_json::json!({"command": "view.scale_bar", "params": {"enabled": false}}));
        assert_eq!(reply["ok"], true, "{reply}");
        assert!(!h.app.preferences.scale_bar);
        h.frames(2);
        assert!(crate::scenario::rect_of("view:scale_bar").is_none(), "turned off again");
    }

    /// In a narrow view, the bar keeps clear of the compatibility analysis legend and of the
    /// placement hint, which share its corner and the bottom of the view.
    #[test]
    fn the_bar_keeps_clear_of_the_analysis_legend_and_the_placement_hint() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", serde_json::json!({"width": 700, "height": 520}));
        h.call("ui.prefs", serde_json::json!({"viewport_analysis_status": false}));
        h.app.run("view.scale_bar", serde_json::json!({"enabled": true})).unwrap();
        h.app.run("solid.box", serde_json::json!({"length": 10, "width": 20, "height": 30})).unwrap();
        h.app.run("inspect.draft", serde_json::json!({"angle": 3.5})).unwrap();
        h.frames(3);
        let legend = crate::scenario::rect_of("analysis:legend").expect("the legacy legend is drawn");
        let bar = crate::scenario::rect_of("view:scale_bar").expect("the bar is drawn");
        assert!(!bar.intersects(legend), "{bar:?} clear of the legend {legend:?}");
        h.app.tool = crate::tools::Tool::for_command("canvas.insert");
        h.frames(3);
        let hint = crate::scenario::rect_of("toast:placement").expect("the placement hint is shown");
        if let Some(bar) = crate::scenario::rect_of("view:scale_bar") {
            assert!(!bar.intersects(hint), "{bar:?} clear of the hint {hint:?}");
        }
    }

    #[test]
    fn a_perspective_label_says_where_the_scale_holds() {
        crate::i18n::install();
        let b = Bar { points: 100.0, value: "20".into(), unit: "mm" };
        assert_eq!(label(&b, false), "20 mm");
        let perspective = label(&b, true);
        assert!(perspective.contains("20 mm") && perspective.len() > "20 mm".len(), "{perspective}");
    }
}
