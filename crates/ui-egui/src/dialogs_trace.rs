//! Motion Trace: the path a picked point of a component takes while a joint is driven between
//! two values or a motion study plays. The path is computed on a scratch copy (`motion.trace`)
//! and drawn in the viewport; OK keeps it as a 3D sketch spline (`motion.trace.keep`).

use crate::ui_kit::{choice_row, expr_row, text_row};
use egui::{Pos2, RichText, Stroke};
use serde_json::{Value, json};
use solidcraft_engine::doc::{IDENTITY, Mat, apply_point, mat_inverse, mat_mul};
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::{Sel, Session};

use super::{Hover, body_of, capital, movable, num, pt, sel_items, snap_of};
use crate::dialogs::{combo, form_row};
use crate::selection::SelInput;
use crate::theme::Tokens;
use crate::viewport::Proj;

#[derive(Clone, Debug)]
pub struct TraceForm {
    /// Traced as a motion study plays (else as a joint is driven from `from` to `to`).
    pub by_study: bool,
    /// Indices into the design's joints and motion studies.
    pub joint: usize,
    pub index: usize,
    pub from: String,
    pub to: String,
    pub study: usize,
    /// Empty: the command's default.
    pub samples: String,
    pub hover: Hover,
    /// The trace shown: what it was computed for, and `motion.trace`'s answer (or why none).
    pub shown: Option<(String, Result<Value, String>)>,
    /// The picked part's frame as shown and the inverse of its placement: the traced path (where
    /// the parts are placed) is drawn moved with the part, through the picked point.
    pub view: Option<(Mat, Mat)>,
}

/// The dialog on a joint (an index into the design's joints), from its value now over a turn.
pub fn form(s: &Session, joint: usize) -> TraceForm {
    let (from, to) = range(s, joint, 0);
    TraceForm { by_study: false, joint, index: 0, from, to, study: 0, samples: String::new(), hover: None, shown: None, view: None }
}

/// From a joint's value now over a turn (an angle) or 20 mm (a distance).
fn range(s: &Session, joint: usize, index: usize) -> (String, String) {
    let Some(j) = s.doc.assembly.joints.get(joint) else { return Default::default() };
    let v = j.values.get(index).copied().unwrap_or(0.0);
    if j.kind.is_angle(index) { (num(v.to_degrees(), "deg"), num(v.to_degrees() + 360.0, "deg")) } else { (num(v, "mm"), num(v + 20.0, "mm")) }
}

/// The occurrence path placing the picked body: the instance clicked, also when its component
/// (or a sub-assembly holding it) is placed more than once (empty: the root design).
fn path(s: &Session, sel: &Sel) -> Option<Vec<u64>> {
    let body = body_of(sel)?;
    s.doc.body_frames(&s.model.state()).into_iter().find(|(name, ..)| name == body).map(|(_, _, path, _)| path)
}

/// The pick: its occurrence path, the point where the part shows, and the part's frame as shown
/// and as placed (they differ under an exploded view or a move not captured yet).
pub(super) fn placement(s: &Session, inputs: &[SelInput]) -> Option<(Vec<u64>, Vec3, Mat, Mat)> {
    let pick = sel_items(inputs, 0).first()?;
    let (snap, path) = (snap_of(s, pick)?, path(s, pick)?);
    let world = |shown: bool| {
        path.iter().try_fold(IDENTITY, |m, id| {
            let o = s.doc.occurrences.iter().find(|o| o.id == *id)?;
            Some(mat_mul(&m, s.pending_moves.get(id).filter(|_| shown).unwrap_or(&o.transform)))
        })
    };
    let (shown, placed) = (world(true)?, world(false)?);
    Some((path, snap.at, shown, placed))
}

/// The trace's parameters (OK's `motion.trace.keep` and the preview's `motion.trace`).
pub fn params(s: &Session, f: &TraceForm, inputs: &[SelInput]) -> Result<Value, String> {
    let mut p = picked_point(s, inputs)?;
    // Retain a geometric snap, rather than just its current world coordinates. Map a pick
    // shown in an exploded view back to the actual placement, as picked_point does.
    if let Some(pick) = sel_items(inputs, 0).first()
        && let Some(snap) = snap_of(s, pick)
        && let Some((path, _, shown, placed)) = placement(s, inputs)
        && let Some(inverse) = mat_inverse(&shown)
    {
        let mut reference = snap.param;
        reference["occurrence"] = json!(path.last().copied().unwrap_or(0));
        for key in ["point", "face", "circle", "edge"] {
            if let Some(at) = reference.get(key).and_then(solidcraft_engine::params::vec3) {
                reference[key] = pt(apply_point(&placed, apply_point(&inverse, at)));
            }
        }
        p["point_ref"] = reference;
    }
    if f.by_study {
        p["study"] = json!(s.doc.assembly.studies.get(f.study).ok_or("no motion study yet")?.id);
    } else {
        let j = s.doc.assembly.joints.get(f.joint).ok_or("no joint that moves yet")?;
        p["joint"] = json!(j.id);
        p["index"] = json!(f.index);
        p["from"] = json!(f.from.trim());
        p["to"] = json!(f.to.trim());
    }
    if !f.samples.trim().is_empty() {
        p["samples"] = json!(f.samples.trim().parse::<f64>().map_err(|_| "samples must be a number")?);
    }
    Ok(p)
}

/// The picked point as `motion.trace` and `motion.plot` take it: `{occurrence, point}`.
pub fn picked_point(s: &Session, inputs: &[SelInput]) -> Result<Value, String> {
    let (path, at, shown, placed) = placement(s, inputs).ok_or("select a point on a component")?;
    // The point starts where the part is placed, not where it may be shown.
    let at = mat_inverse(&shown).map(|m| apply_point(&placed, apply_point(&m, at))).ok_or("select a point on a component")?;
    let occurrence = if path.is_empty() { json!(0) } else { json!(path) };
    Ok(json!({"occurrence": occurrence, "point": pt(at)}))
}

/// Where and why the motion stopped short, ending the trace (`None`: it went all the way).
pub(crate) fn stop_text(v: &Value) -> Option<String> {
    let at = v["stopped_at"].as_u64()?.to_string();
    // The values come from the engine: a missing key is an empty name, never a panic.
    let name = |m: &serde_json::Map<String, Value>, key: &str| m.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    if let Some(l) = v["stopped_by_loop"].as_object() {
        return Some(crate::i18n::format("trace.stopped_by_loop", &[("sample", &at), ("joint", &name(l, "joint"))]));
    }
    let c = v["stopped_by_contact"].as_object()?;
    Some(crate::i18n::format("trace.stopped_by_contact", &[("sample", &at), ("a", &name(c, "a")), ("b", &name(c, "b"))]))
}

/// The dialog's rows; true when Enter was pressed in a value.
pub fn rows(app: &mut crate::SolveApp, ui: &mut egui::Ui, f: &mut TraceForm, inputs: &[SelInput]) -> bool {
    let t = Tokens::get();
    let s = &app.session;
    let mut enter = false;
    let studies: Vec<&str> = s.doc.assembly.studies.iter().map(|m| m.name.as_str()).collect();
    f.by_study &= !studies.is_empty();
    if !studies.is_empty() {
        form_row(ui, "Motion", |ui| {
            let mut by = usize::from(f.by_study);
            combo(ui, "tr_by", &["Joint", "Motion Study"], &mut by);
            f.by_study = by == 1;
        });
    }
    if f.by_study {
        choice_row(ui, "Study", "tr_study", &mut f.study, &studies);
    } else {
        let list = movable(s);
        if list.is_empty() {
            form_row(ui, "", |ui| {
                ui.label(RichText::new(crate::i18n::tr_label("no joint that moves yet")).color(t.text_dim));
            });
            return false;
        }
        let labels: Vec<&str> = list.iter().filter_map(|i| s.doc.assembly.joints.get(*i)).map(|j| j.name.as_str()).collect();
        let mut pos = list.iter().position(|i| *i == f.joint).unwrap_or(0);
        choice_row(ui, "Joint", "tr_joint", &mut pos, &labels);
        let joint = list.get(pos).copied().unwrap_or(0);
        let dofs = s.doc.assembly.joints.get(joint).map(|j| j.kind.dofs()).unwrap_or(&[]);
        let mut index = if joint == f.joint { f.index.min(dofs.len().saturating_sub(1)) } else { 0 };
        if dofs.len() > 1 {
            form_row(ui, "  Of", |ui| {
                let caps: Vec<String> = dofs.iter().map(|d| capital(d)).collect();
                let refs: Vec<&str> = caps.iter().map(String::as_str).collect();
                combo(ui, "tr_index", &refs, &mut index);
            });
        }
        if joint != f.joint || index != f.index {
            (f.joint, f.index) = (joint, index);
            (f.from, f.to) = range(s, joint, index);
        }
        enter |= expr_row(ui, "From", &mut f.from);
        enter |= expr_row(ui, "To", &mut f.to);
    }
    let default = match s.doc.assembly.studies.get(f.study).filter(|_| f.by_study) {
        Some(m) => (u64::from(m.steps) + 1).clamp(2, 500),
        None => 100,
    };
    enter |= text_row(ui, "Samples", &mut f.samples, &default.to_string()).0;
    // The preview, traced again when what it traces (or the design) changes.
    let p = params(s, f, inputs);
    let key = format!("{}:{p:?}", s.revision);
    if f.shown.as_ref().is_none_or(|(k, _)| *k != key) {
        let r = p.and_then(|p| s.scratch().execute("motion.trace", &p).map_err(|e| e.to_string()));
        f.shown = Some((key, r));
        f.view = placement(s, inputs).and_then(|(_, _, shown, placed)| Some((shown, mat_inverse(&placed)?)));
    }
    if let Some((_, r)) = &f.shown {
        let (text, colour) = match r {
            Ok(v) => match stop_text(v) {
                Some(why) => (why, t.warning),
                None => (crate::i18n::format("trace.points", &[("count", &v["samples"].to_string())]), t.text_dim),
            },
            Err(e) => (e.clone(), t.text_dim),
        };
        form_row(ui, "", |ui| {
            ui.add(egui::Label::new(RichText::new(text).color(colour)).wrap());
        });
    }
    enter
}

/// The traced path in the viewport, with a dot where it starts.
pub fn overlay(painter: &egui::Painter, proj: &Proj, f: &TraceForm) {
    let Some((_, Ok(v))) = &f.shown else { return };
    let at = |p: &Value| p.as_array().and_then(|a| Some(Vec3::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?)));
    let shown = |p: Vec3| f.view.map_or(p, |(shown, inverse)| apply_point(&shown, apply_point(&inverse, p)));
    let pts: Vec<Pos2> = v["points"].as_array().into_iter().flatten().filter_map(at).map(shown).filter_map(|p| proj.to_screen(p)).collect();
    let t = Tokens::get();
    if let Some(first) = pts.first() {
        painter.circle_filled(*first, 3.5, t.preview_sketch);
    }
    painter.add(egui::Shape::line(pts, Stroke::new(2.0, t.preview_sketch)));
}

#[cfg(test)]
mod shared_status_baseline_tests {
    use super::*;

    #[test]
    fn stop_text_tolerates_missing_keys() {
        for v in [
            json!({"stopped_at":1,"stopped_by_contact":{}}),
            json!({"stopped_at":1,"stopped_by_contact":{"a":"A"}}),
            json!({"stopped_at":1,"stopped_by_loop":{}}),
        ] {
            assert!(stop_text(&v).is_some());
        }
    }

    #[test]
    fn stop_text_preserves_missing_values_and_loop_precedence() {
        for v in [json!({}), json!({"stopped_at":-1}), json!({"stopped_at":2}), json!({"stopped_at":2.5,"stopped_by_loop":{}})] {
            assert_eq!(stop_text(&v), None);
        }
        assert_eq!(
            stop_text(&json!({"stopped_at":3,"stopped_by_loop":{"joint":"Hinge"},"stopped_by_contact":{"a":"A","b":"B"}})),
            Some(crate::i18n::format("trace.stopped_by_loop", &[("sample", "3"), ("joint", "Hinge")]))
        );
        assert_eq!(
            stop_text(&json!({"stopped_at":4,"stopped_by_contact":{"a":"A","b":"B"}})),
            Some(crate::i18n::format("trace.stopped_by_contact", &[("sample", "4"), ("a", "A"), ("b", "B")]))
        );
        assert_eq!(
            stop_text(&json!({"stopped_at":0,"stopped_by_contact":{"a":1,"b":null}})),
            Some(crate::i18n::format("trace.stopped_by_contact", &[("sample", "0"), ("a", ""), ("b", "")]))
        );
    }
}
