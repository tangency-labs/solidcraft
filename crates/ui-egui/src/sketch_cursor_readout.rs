//! The cursor's coordinates in an open sketch (#11070). A sketch tool shows the point it would
//! place beside the pointer (`tools::preview`); with no tool running, the same readout shows
//! where the pointer meets the sketch plane, in the sketch's own X and Y from its origin, so the
//! position can be read while just looking around. Over a sketch point it reads that point's
//! exact place. Preferences › Sketch ("Show the cursor's coordinates in a sketch",
//! `sketch_cursor_coordinates`) turns it off, leaving the readout to the tools as before.
//! It is UI only: drawing it runs no command and changes nothing in the design.

use egui::Pos2;
use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::viewport::Proj;

/// How near (screen points) the pointer reads a sketch point's exact place: the reach the
/// viewport's picking gives sketch points.
const POINT_REACH: f32 = 8.0;

/// The sketch coordinates where the view ray `origin + t·direction` meets `plane`: None when
/// the ray runs along the plane or the numbers are not finite.
pub(crate) fn plane_point(plane: &Plane, origin: Vec3, direction: Vec3) -> Option<Vec2> {
    if !origin.is_finite() || !direction.is_finite() {
        return None;
    }
    let local = plane.to_local(plane.intersect_ray(origin, direction)?);
    local.is_finite().then_some(local)
}

/// The readout's text for the sketch point `p`: X and Y in the design's units.
pub(crate) fn text(app: &SolveApp, p: Vec2) -> String {
    format!("{}, {}", crate::prefs::show_mm(app, p.x, 1), crate::prefs::show_mm(app, p.y, 1))
}

/// The sketch coordinates to show for a pointer at `pointer` in the open sketch with no tool
/// running: the hovered point of the sketch, else where the pointer's ray meets the sketch plane.
/// None when the preference is off, a tool runs (it shows its own), or no 2D sketch is open
/// (none, or a 3D sketch).
pub(crate) fn idle_point(app: &SolveApp, proj: &Proj, pointer: Pos2) -> Option<Vec2> {
    if !app.preferences.sketch_cursor_coordinates || app.tool.is_some() {
        return None;
    }
    let sid = app.session.active_sketch?;
    let world = crate::open_meshing::shown_world(app);
    // A 3D sketch's points leave its plane: X and Y on it would not say where the pointer is.
    let ss = world.sketch(sid).filter(|ss| !ss.sketch.view.three_d)?;
    // The nearest shown point under the pointer, from the sketch as drawn this frame: not the
    // selection hover, which a selection filter can turn off and which can hold a dragged
    // point's place from before the drag.
    let visible = solidcraft_engine::view::SketchPointVisibility::new(&ss.sketch);
    let near = ss
        .sketch
        .points
        .iter()
        .filter(|p| visible.takes(p) && p.pos.is_finite())
        .filter_map(|p| Some((proj.to_screen(ss.plane.to_world(p.pos))?.distance(pointer), p.pos)))
        .filter(|(d, _)| d.is_finite() && *d < POINT_REACH)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, at)) = near {
        return Some(at);
    }
    let (origin, direction) = proj.ray(pointer);
    plane_point(&ss.plane, origin, direction)
}

/// Draw the readout for a pointer at `at` (None: the pointer is not over the model), where a
/// tool's readout starts: down and right of the pointer, clear of the arrow and the tool badge.
pub(crate) fn show(app: &SolveApp, painter: &egui::Painter, proj: &Proj, at: Option<Pos2>) {
    let Some(at) = at else { return };
    let Some(p) = idle_point(app, proj, at) else { return };
    let rect = painter.text(
        at + crate::tool_cursor::READOUT,
        egui::Align2::LEFT_TOP,
        text(app, p),
        egui::FontId::proportional(crate::theme::Tokens::get().font_small),
        crate::theme::Tokens::view().text_dim,
    );
    crate::scenario::publish_rect("viewport:cursor_readout", rect);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{Harness, rect_of};
    use serde_json::json;

    fn plane(origin: [f64; 3], x: [f64; 3], y: [f64; 3]) -> Plane {
        let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
        Plane::new(v(origin), v(x), v(y)).unwrap()
    }

    #[test]
    fn plane_point_reads_the_sketch_axes_from_the_sketch_origin() {
        // The XY plane: the ray straight down through (12, -7) reads (12, -7).
        let xy = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let p = plane_point(&xy, Vec3::new(12.0, -7.0, 50.0), Vec3::new(0.0, 0.0, -1.0)).unwrap();
        assert!((p.x - 12.0).abs() < 1e-12 && (p.y + 7.0).abs() < 1e-12, "{p:?}");
        // A plane moved and turned: X along world Y, Y along world Z, origin at (5, 10, 20).
        let side = plane([5.0, 10.0, 20.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        let p = plane_point(&side, Vec3::new(100.0, 13.0, 16.0), Vec3::new(-2.0, 0.0, 0.0)).unwrap();
        assert!((p.x - 3.0).abs() < 1e-12 && (p.y + 4.0).abs() < 1e-12, "{p:?}");
        // A slanted ray lands where it crosses, not under its start.
        let p = plane_point(&xy, Vec3::new(0.0, 0.0, 10.0), Vec3::new(1.0, 2.0, -1.0)).unwrap();
        assert!((p.x - 10.0).abs() < 1e-12 && (p.y - 20.0).abs() < 1e-12, "{p:?}");
    }

    #[test]
    fn plane_point_is_none_along_the_plane_or_for_hostile_numbers() {
        let xy = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert_eq!(plane_point(&xy, Vec3::new(0.0, 0.0, 5.0), Vec3::new(1.0, 0.0, 0.0)), None);
        assert_eq!(plane_point(&xy, Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 0.0)), None);
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
            assert!(plane_point(&xy, Vec3::new(bad, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0)).is_none_or(|p| p.is_finite()));
            assert!(plane_point(&xy, Vec3::new(0.0, 0.0, 5.0), Vec3::new(bad, 1.0, -1.0)).is_none_or(|p| p.is_finite()));
            assert!(plane_point(&xy, Vec3::new(0.0, 0.0, bad), Vec3::new(0.0, 0.0, -1.0)).is_none_or(|p| p.is_finite()));
        }
    }

    /// Open a sketch on XY facing it, rest the pointer at the sketch point (10, 5) and draw.
    fn idle_sketch() -> (Harness, Pos2) {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.call("ui.view", json!({"view": "top"}));
        h.frames(2);
        let at = h.at(&json!({"world": [10, 5, 0]})).unwrap();
        let at = egui::pos2(at[0] as f32, at[1] as f32);
        h.step(&json!({"move": [at.x, at.y]})).unwrap();
        h.frames(2);
        (h, at)
    }

    #[test]
    fn an_idle_sketch_shows_the_cursor_coordinates_and_the_preference_restores_tools_only() {
        let (mut h, at) = idle_sketch();
        assert!(h.app.tool.is_none());
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let p = idle_point(&h.app, &proj, at).expect("the pointer meets the sketch plane");
        assert!((p.x - 10.0).abs() < 0.05 && (p.y - 5.0).abs() < 0.05, "{p:?}");
        let readout = rect_of("viewport:cursor_readout").expect("the readout shows with no tool running");
        assert!(readout.left() > at.x && readout.top() > at.y, "down and right of the pointer: {readout:?} at {at:?}");
        assert!(text(&h.app, p).contains(','), "{}", text(&h.app, p));

        // Off: no readout until a tool draws, as before.
        h.call("ui.prefs", json!({"sketch_cursor_coordinates": false}));
        h.frames(2);
        assert!(idle_point(&h.app, &proj, at).is_none());
        assert!(rect_of("viewport:cursor_readout").is_none(), "the preference turns the idle readout off");

        // A running tool keeps its own readout, and this one stays away.
        h.call("ui.prefs", json!({"sketch_cursor_coordinates": true}));
        h.app.start("sketch.line");
        h.step(&json!({"move": [at.x + 3.0, at.y]})).unwrap();
        h.frames(2);
        assert!(rect_of("viewport:cursor_readout").is_none(), "one readout at a time");
        assert!(rect_of("viewport:readout").is_some(), "the tool's readout is drawn");
    }

    /// Over a sketch point the readout gives the point's exact place, also with sketch points
    /// filtered out of selection (the selection hover is then empty).
    #[test]
    fn over_a_point_the_readout_is_the_points_place_whatever_the_selection_filter() {
        let (mut h, at) = idle_sketch();
        h.app.run("sketch.point", json!({"point": [10.04, 4.98]})).unwrap();
        for off in [false, true] {
            h.app.ui.pick_off.retain(|kind| kind != "sketch_points");
            if off {
                h.app.ui.pick_off.push("sketch_points".into());
            }
            h.frames(2);
            let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
            let p = idle_point(&h.app, &proj, at).expect("a point under the pointer");
            assert!((p.x - 10.04).abs() < 1e-9 && (p.y - 4.98).abs() < 1e-9, "filter off: {off}: {p:?}");
        }
    }

    #[test]
    fn no_readout_outside_a_sketch() {
        let mut h = Harness::new();
        h.app.home.open = false;
        let c = h.app.viewport.rect.map(|r| r.center()).unwrap();
        h.step(&json!({"move": [c.x, c.y]})).unwrap();
        h.frames(2);
        assert!(rect_of("viewport:cursor_readout").is_none());
    }
}
