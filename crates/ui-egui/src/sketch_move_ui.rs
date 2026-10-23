//! Sketch Move/Copy: a 2D version of the body Move window (Free, Translate, Rotate, Point to
//! Point) on the sketch plane, with the same icon row and a planar triad on the canvas.

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::geom::{Vec2, Vec3};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::gizmo::{self, Change, Drag};
use crate::sketch_modify_ui::{self, Mod};
use crate::ui_kit;

pub const TYPES: [&str; 4] = ["Free Move", "Translate", "Rotate", "Point to Point"];
const ICONS: [&str; 4] = ["move_free", "move_translate", "move_rotate", "move_ptp"];
pub const FREE: usize = 0;
pub const TRANSLATE: usize = 1;
pub const ROTATE: usize = 2;
pub const POINT_TO_POINT: usize = 3;

pub fn kind(mode: usize) -> Mod {
    Mod::Move { mode, x: "0 mm".into(), y: "0 mm".into(), angle: "0 deg".into(), copy: false }
}

pub fn type_row(d: &mut Dialog, ui: &mut egui::Ui) {
    let Kind::SketchModify(Mod::Move { mode, .. }) = d.kind else { return };
    let mut m = mode;
    ui_kit::row(ui, "Move Type", |ui| {
        let choices: Vec<_> =
            TYPES.iter().zip(&ICONS).map(|(key, icon)| ui_kit::IconSegment { key, icon, tip: crate::i18n::tr_label(key).to_string() }).collect();
        ui_kit::segmented_icons(ui, &mut m, &choices, &[]);
    });
    if m != mode {
        let entities = d.inputs.first().map(|i| i.items.clone()).unwrap_or_default();
        d.kind = Kind::SketchModify(kind(m));
        d.inputs = sketch_modify_ui::move_inputs(m);
        d.active = if entities.is_empty() { 0 } else { 1.min(d.inputs.len() - 1) };
        if let Some(i) = d.inputs.first_mut() {
            i.items = entities;
        }
        d.extra.remove("drag0");
    }
}

fn plane_axes(app: &SolveApp) -> Option<(Vec3, Vec3, Vec3)> {
    let world = app.session.world_state();
    let ss = world.sketch(app.session.active_sketch?)?;
    let o = ss.plane.origin;
    let (u, v) = (ss.plane.x.normalized()?, ss.plane.y.normalized()?);
    Some((o, u, v))
}

fn entity_centroid_local(app: &SolveApp, d: &Dialog) -> Option<Vec2> {
    let sid = app.session.active_sketch?;
    let world = app.session.world_state();
    let ss = world.sketch(sid)?;
    let mut bb: Option<(Vec2, Vec2)> = None;
    for sel in d.inputs.first()?.items.iter().flat_map(|s| sketch_modify_ui::expand_sketch_entity(&app.session, s)) {
        let p = match &sel {
            Sel::SketchCurve { id } => {
                let idx = ss.sketch.curve_index(id)?;
                let pts = ss.sketch.polyline(idx);
                if pts.is_empty() {
                    continue;
                }
                pts.iter().fold(Vec2::ZERO, |s, q| s + *q) * (1.0 / pts.len() as f64)
            }
            Sel::SketchPoint { id } => ss.sketch.point(ss.sketch.resolve_point(id)?)?,
            _ => continue,
        };
        bb = Some(match bb {
            None => (p, p),
            Some((lo, hi)) => (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y))),
        });
    }
    Some(match bb {
        Some((lo, hi)) => (lo + hi) * 0.5,
        None => Vec2::ZERO,
    })
}

fn center_world(app: &SolveApp, d: &Dialog) -> Option<Vec3> {
    let Kind::SketchModify(Mod::Move { mode, .. }) = &d.kind else { return None };
    let sid = app.session.active_sketch?;
    let world = app.session.world_state();
    let ss = world.sketch(sid)?;
    if *mode == POINT_TO_POINT {
        let from = d.inputs.get(1).and_then(|i| i.items.first());
        let to = d.inputs.get(2).and_then(|i| i.items.first());
        if let (Some(f), Some(t)) = (from, to)
            && let (Some(a), Some(b)) = (sketch_modify_ui::sketch_point_pos(&app.session, f), sketch_modify_ui::sketch_point_pos(&app.session, t))
        {
            let c = entity_centroid_local(app, d)? + (b - a);
            return Some(ss.plane.to_world(c));
        }
        if let Some(f) = from
            && let Some(a) = sketch_modify_ui::sketch_point_pos(&app.session, f)
        {
            return Some(ss.plane.to_world(a));
        }
        return Some(ss.plane.to_world(entity_centroid_local(app, d)?));
    }
    if let Some(Sel::SketchPoint { id }) = d.inputs.get(1).and_then(|i| i.items.first()) {
        let p = ss.sketch.point(ss.sketch.resolve_point(id)?)?;
        return Some(ss.plane.to_world(p));
    }
    Some(ss.plane.to_world(entity_centroid_local(app, d)?))
}

fn len(app: &SolveApp, s: &str) -> Result<f64, String> {
    app.session.doc.eval(s, ValueKind::Length).map_err(|e| e.to_string()).and_then(|v| if v.is_finite() { Ok(v) } else { Err("not a number".into()) })
}

fn ang(app: &SolveApp, s: &str) -> Result<f64, String> {
    app.session.doc.eval(s, ValueKind::Angle).map_err(|e| e.to_string()).and_then(|v| if v.is_finite() { Ok(v) } else { Err("not a number".into()) })
}

fn mm(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{} mm", if s == "-0" || s.is_empty() { "0" } else { s })
}

fn deg(v: f64) -> String {
    let s = format!("{:.4}", v.to_degrees());
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{} deg", if s == "-0" || s.is_empty() { "0" } else { s })
}

/// Keep typed X/Y in sync with From→To for point-to-point (preview and value box).
pub fn sync_p2p(app: &SolveApp, d: &mut Dialog) {
    let Kind::SketchModify(Mod::Move { mode, x, y, .. }) = &mut d.kind else { return };
    if *mode != POINT_TO_POINT {
        return;
    }
    let from = d.inputs.get(1).and_then(|i| i.items.first());
    let to = d.inputs.get(2).and_then(|i| i.items.first());
    if let (Some(f), Some(t)) = (from, to)
        && let (Some(a), Some(b)) = (sketch_modify_ui::sketch_point_pos(&app.session, f), sketch_modify_ui::sketch_point_pos(&app.session, t))
    {
        *x = mm(b.x - a.x);
        *y = mm(b.y - a.y);
    }
}

pub fn triad(app: &SolveApp, d: &Dialog) -> Option<gizmo::Triad> {
    let Kind::SketchModify(Mod::Move { mode, x, y, .. }) = &d.kind else { return None };
    let (_, u, v) = plane_axes(app)?;
    if *mode == POINT_TO_POINT {
        let from = d.inputs.get(1).and_then(|i| i.items.first());
        let to = d.inputs.get(2).and_then(|i| i.items.first());
        if from.is_some() && to.is_none() {
            let c = center_world(app, d)?;
            return Some(gizmo::Triad { center: c, translate: true, rotate: false, ring_axis: None, translate_u: Some(u), translate_v: Some(v) });
        }
        return None;
    }
    let mut c = center_world(app, d)?;
    if matches!(*mode, FREE | TRANSLATE) {
        let t = Vec2::new(len(app, x).unwrap_or(0.0), len(app, y).unwrap_or(0.0));
        let world = app.session.world_state();
        let ss = world.sketch(app.session.active_sketch?)?;
        c = ss.plane.to_world(ss.plane.to_local(c) + t);
    }
    let n = u.cross(v).normalized().unwrap_or(u.cross(Vec3::Z).normalized().unwrap_or(Vec3::Z));
    Some(gizmo::Triad {
        center: c,
        translate: matches!(*mode, FREE | TRANSLATE),
        rotate: matches!(*mode, FREE | ROTATE),
        ring_axis: if matches!(*mode, FREE | ROTATE) { Some(n) } else { None },
        translate_u: if matches!(*mode, FREE | TRANSLATE) { Some(u) } else { None },
        translate_v: if matches!(*mode, FREE | TRANSLATE) { Some(v) } else { None },
    })
}

/// Arrow from From to To when both sketch points are picked.
pub fn line_drag(app: &SolveApp, d: &Dialog) -> Option<gizmo::LineDrag> {
    let Kind::SketchModify(Mod::Move { mode, .. }) = &d.kind else { return None };
    if *mode != POINT_TO_POINT || d.inputs.first().is_none_or(|i| i.items.is_empty()) {
        return None;
    }
    let from = d.inputs.get(1).and_then(|i| i.items.first())?;
    let to = d.inputs.get(2).and_then(|i| i.items.first())?;
    let sid = app.session.active_sketch?;
    let world = app.session.world_state();
    let ss = world.sketch(sid)?;
    let a = sketch_modify_ui::sketch_point_pos(&app.session, from)?;
    let b = sketch_modify_ui::sketch_point_pos(&app.session, to)?;
    let from_w = ss.plane.to_world(a);
    let to_w = ss.plane.to_world(b);
    let delta = to_w - from_w;
    let dir = delta.normalized()?;
    Some(gizmo::LineDrag { start: from_w, dir, length: delta.len() })
}

pub fn drag(app: &SolveApp, d: &mut Dialog, g: Drag) {
    let Kind::SketchModify(Mod::Move { mode, x, y, angle, .. }) = &d.kind else { return };
    if *mode == POINT_TO_POINT {
        return;
    }
    if g.started || !d.extra.contains_key("drag0") {
        let v: Vec<f64> = match *mode {
            ROTATE => vec![ang(app, angle).unwrap_or(0.0)],
            _ => vec![len(app, x).unwrap_or(0.0), len(app, y).unwrap_or(0.0), ang(app, angle).unwrap_or(0.0)],
        };
        d.extra.insert("drag0".into(), json!(v));
    }
    let v0: Vec<f64> = d.extra.get("drag0").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let Kind::SketchModify(Mod::Move { mode, x, y, angle, .. }) = &mut d.kind else { return };
    match (*mode, g.change) {
        (ROTATE, Change::RotateAbout { angle: th, .. }) | (ROTATE, Change::Rotate { angle: th, .. }) => {
            *angle = deg(v0.first().copied().unwrap_or(0.0) + th);
        }
        (FREE, Change::RotateAbout { angle: th, .. }) | (FREE, Change::Rotate { angle: th, .. }) => {
            *angle = deg(v0.get(2).copied().unwrap_or(0.0) + th);
        }
        (FREE | TRANSLATE, Change::Translate(t)) => {
            if let Some((_, u, v)) = plane_axes(app) {
                *x = mm(v0.first().copied().unwrap_or(0.0) + t.dot(u));
                *y = mm(v0.get(1).copied().unwrap_or(0.0) + t.dot(v));
            }
        }
        _ => {}
    }
    if g.done {
        d.extra.remove("drag0");
    }
}

pub fn value_rows(ui: &mut egui::Ui, mode: usize, x: &mut String, y: &mut String, angle: &mut String, copy: &mut bool) -> bool {
    let mut enter = false;
    match mode {
        FREE | TRANSLATE => {
            enter |= ui_kit::value_pair(ui, "Position", [x, y], ["X", "Y"], Some(["X Distance", "Y Distance"]));
            if mode == FREE {
                enter |= ui_kit::row(ui, "Angle", |ui| ui_kit::field(ui, angle));
            }
        }
        ROTATE => enter |= ui_kit::row(ui, "Angle", |ui| ui_kit::field(ui, angle)),
        POINT_TO_POINT => {
            ui_kit::row(ui, "X Distance", |ui| ui.label(x.as_str()));
            ui_kit::row(ui, "Y Distance", |ui| ui.label(y.as_str()));
        }
        _ => {}
    }
    ui_kit::options(ui, "move_options", &mut [("Create Copy", copy)]);
    enter
}
