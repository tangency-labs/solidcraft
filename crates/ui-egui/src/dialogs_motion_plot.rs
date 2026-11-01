//! Motion Study analysis rows: a chart over the study's timeline of a joint's value, or of a
//! picked point's travel, as value, velocity or acceleration (`motion.plot`), with a cursor at
//! the step shown; and the parts that collide as the study plays (`motion.interference`). Both
//! are worked out on a scratch copy, again only when what they are for changes.

use crate::dialogs_assembly::stop_text;
use crate::ui_kit::choice_row;
use std::hash::{Hash, Hasher};

use egui::{Align2, FontId, Rect, RichText, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::doc::joints::JointKind;

use crate::SolveApp;
use crate::dialogs::{combo, form_row};
use crate::selection::SelInput;
use crate::theme::Tokens;

/// Most samples the chart takes (each is a solve).
const SAMPLES: u64 = 201;
pub const QUANTITIES: [&str; 3] = ["Value", "Velocity", "Acceleration"];

/// What the chart shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Plotted {
    Nothing,
    /// A joint's value: (joint id, value index).
    Joint(u64, usize),
    /// The picked point: its distance from where it starts.
    Point,
}

#[derive(Clone, Debug, Default)]
pub struct Analysis {
    /// Index into `choices`.
    pub what: usize,
    /// Index into `QUANTITIES`.
    pub quantity: usize,
    /// Seconds the whole study takes (empty: rates per step).
    pub duration: String,
    /// Collisions among all components (0) or in the design's contact sets (1).
    pub among: usize,
    /// The joint last picked in the browser, which the chart then shows.
    followed: Option<u64>,
    /// The chart's `motion.plot` answer (or why none) and what it was for.
    plot: Option<(u64, Result<Value, String>)>,
    /// The last collision check's answer and what it was for.
    collisions: Option<(u64, Result<Value, String>)>,
}

/// What the chart can show: (label, what).
pub fn choices(s: &Session, point: bool) -> Vec<(String, Plotted)> {
    let mut out = vec![("None".to_string(), Plotted::Nothing)];
    for j in s.doc.assembly.joints.iter().filter(|j| j.kind != JointKind::Rigid && !j.suppressed) {
        let dofs = j.kind.dofs();
        for (i, d) in dofs.iter().enumerate() {
            out.push((if dofs.len() > 1 { format!("{} · {d}", j.name) } else { j.name.clone() }, Plotted::Joint(j.id, i)));
        }
    }
    if point {
        out.push(("Picked point".into(), Plotted::Point));
    }
    out
}

/// `motion.plot`'s parameters for `study` (the study command's parameters).
pub fn plot_params(s: &Session, what: Plotted, study: &Value, duration: &str, inputs: &[SelInput]) -> Result<Value, String> {
    let name = study.get("study").or_else(|| study.get("name")).cloned().unwrap_or(Value::Null);
    let steps = study["steps"].as_u64().unwrap_or(100);
    let mut p = json!({"study": name, "samples": steps.saturating_add(1).clamp(3, SAMPLES)});
    match what {
        Plotted::Nothing => return Err(String::new()),
        Plotted::Joint(id, _) => p["joints"] = json!([id]),
        Plotted::Point => {
            let picked = crate::dialogs_assembly::picked_point(s, inputs)?;
            p["joints"] = json!([]);
            p["occurrence"] = picked["occurrence"].clone();
            p["point"] = picked["point"].clone();
        }
    }
    let d = duration.trim();
    if !d.is_empty() {
        let seconds = d.parse::<f64>().ok().filter(|x| (1e-3..=1e9).contains(x)).ok_or("the duration must be a number of seconds, 0.001 or more")?;
        p["duration"] = json!(seconds);
    }
    Ok(p)
}

fn nums(v: &Value) -> Vec<f64> {
    v.as_array().into_iter().flatten().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()
}

/// The chart's line from `motion.plot`'s answer: (study steps, values, unit).
pub fn line(v: &Value, what: Plotted, quantity: usize) -> Option<(Vec<f64>, Vec<f64>, String)> {
    let series = match what {
        Plotted::Nothing => return None,
        Plotted::Joint(_, i) => v["joints"].as_array()?.iter().find(|j| j["index"] == i)?,
        Plotted::Point => &v["point"],
    };
    let unit = series[["unit", "velocity_unit", "acceleration_unit"].get(quantity)?].as_str()?.to_string();
    let ys = match (what, quantity) {
        (Plotted::Point, 0) => {
            let at = |p: &Value| [0, 1, 2].map(|c| p[c].as_f64().unwrap_or(f64::NAN));
            let pts: Vec<[f64; 3]> = series["position"].as_array()?.iter().map(at).collect();
            let start = *pts.first()?;
            pts.iter().map(|p| p.iter().zip(&start).map(|(a, b)| (a - b).powi(2)).sum::<f64>().sqrt()).collect()
        }
        (Plotted::Point, 1) => nums(&series["speed"]),
        (Plotted::Point, _) => nums(&series["acceleration_norm"]),
        (_, q) => nums(&series[*["value", "velocity", "acceleration"].get(q)?]),
    };
    Some((nums(&v["step"]), ys, unit))
}

/// A short number for an axis or a readout.
fn short(v: f64) -> String {
    let a = v.abs();
    if a >= 1000.0 {
        format!("{v:.0}")
    } else if a >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.3}")
    }
}

/// The value of a line at step `at` (linear between samples; `None` outside it).
pub fn value_at(xs: &[f64], ys: &[f64], at: f64) -> Option<f64> {
    let k = xs.iter().position(|x| *x >= at)?;
    let (x1, y1) = (*xs.get(k)?, *ys.get(k)?);
    if k == 0 || x1 <= at {
        return Some(y1);
    }
    let (x0, y0) = (*xs.get(k - 1)?, *ys.get(k - 1)?);
    Some(y0 + (y1 - y0) * (at - x0) / (x1 - x0).max(1e-12))
}

/// A line chart of `ys` over study steps `xs` (0…`total`), with the step shown as a cursor and
/// `bands` (step ranges where parts collide) shaded. Returns the step clicked or dragged to.
fn chart(ui: &mut egui::Ui, xs: &[f64], ys: &[f64], unit: &str, total: u32, now: f64, bands: &[[f64; 2]], t: &Tokens) -> Option<f64> {
    let (r, resp) = ui.allocate_exact_size(vec2(300.0, 120.0), Sense::click_and_drag());
    crate::scenario::publish_rect("motion:chart", r);
    let p = ui.painter_at(r);
    p.rect(r, t.radius_sm.saturating_sub(1), t.field, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let plot = Rect::from_min_max(r.min + vec2(6.0, 16.0), r.max - vec2(6.0, 16.0));
    let total = f64::from(total.max(1));
    let x = |s: f64| plot.left() + plot.width() * (s / total).clamp(0.0, 1.0) as f32;
    for [a, b] in bands {
        let (a, b) = (x(*a), x(*b));
        p.rect_filled(Rect::from_x_y_ranges((a - 1.0)..=(b + 1.0), r.y_range()), 0.0, t.interference.gamma_multiply(0.35));
    }
    let (lo, hi) = ys.iter().filter(|v| v.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
    if lo.is_finite() {
        // A flat line sits in the middle.
        let pad = if hi - lo < 1e-9 * hi.abs().max(1.0) { 1.0f64.max(hi.abs() * 0.1) } else { 0.0 };
        let (lo, hi) = (lo - pad, hi + pad);
        let y = |v: f64| plot.bottom() - plot.height() * ((v - lo) / (hi - lo)) as f32;
        if lo < 0.0 && hi > 0.0 {
            p.line_segment([pos2(plot.left(), y(0.0)), pos2(plot.right(), y(0.0))], Stroke::new(1.0, t.border));
        }
        let pts: Vec<egui::Pos2> = xs.iter().zip(ys).filter(|(_, v)| v.is_finite()).map(|(s, v)| pos2(x(*s), y(*v))).collect();
        p.add(egui::Shape::line(pts, Stroke::new(1.5, t.accent)));
        let font = FontId::proportional(crate::theme::Tokens::get().font_micro);
        p.text(r.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, format!("{} {unit}", short(hi)), font.clone(), t.text_dim);
        p.text(r.left_bottom() + vec2(4.0, -2.0), Align2::LEFT_BOTTOM, format!("{} {unit}", short(lo)), font.clone(), t.text_dim);
        if let Some(v) = value_at(xs, ys, now) {
            p.text(r.right_top() + vec2(-4.0, 2.0), Align2::RIGHT_TOP, format!("{} {unit}", short(v)), font, t.text);
        }
    }
    // Where steps 0 and `total` fall, for a scenario or an agent to click between.
    crate::scenario::publish_handle("motion:chart_start", pos2(x(0.0), plot.center().y));
    crate::scenario::publish_handle("motion:chart_end", pos2(x(total), plot.center().y));
    let cx = x(now);
    p.line_segment([pos2(cx, r.top()), pos2(cx, r.bottom())], Stroke::new(1.5, t.warning));
    let pos = resp.interact_pointer_pos().filter(|_| resp.clicked() || resp.dragged())?;
    Some((f64::from((pos.x - plot.left()) / plot.width().max(1.0)).clamp(0.0, 1.0) * total).round())
}

fn key_of(x: impl std::fmt::Debug) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{x:?}").hash(&mut h);
    h.finish()
}

/// The occurrences of the bodies picked in input `i` (the instance each pick is on).
fn picked_occurrences(s: &Session, inputs: &[SelInput], i: usize) -> Vec<u64> {
    let names: Vec<&str> = inputs.get(i).map(|inp| inp.items.iter().filter_map(crate::dialogs_assembly::body_of).collect()).unwrap_or_default();
    if names.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (name, _, path, _) in s.doc.body_frames(&s.model.state()) {
        if let Some(o) = path.last().filter(|_| names.contains(&name.as_str()))
            && !out.contains(o)
        {
            out.push(*o);
        }
    }
    out
}

/// The step ranges where any pair collides, from `motion.interference`'s answer.
pub fn collision_bands(v: &Value) -> Vec<[f64; 2]> {
    let ranges = v["collisions"].as_array().into_iter().flatten().flat_map(|c| c["steps"].as_array().cloned().unwrap_or_default());
    ranges.filter_map(|r| Some([r.get(0)?.as_f64()?, r.get(1)?.as_f64()?])).collect()
}

/// The analysis rows by a study's timeline: `study` is the study command's parameters (or why
/// there are none yet), `step` the step shown (a click on the chart or a collision moves it).
#[allow(clippy::too_many_arguments)]
pub fn rows(
    app: &SolveApp,
    ui: &mut egui::Ui,
    a: &mut Analysis,
    study: &Result<Value, String>,
    step: &mut f64,
    playing: &mut bool,
    total: u32,
    inputs: &[SelInput],
) {
    let t = Tokens::get();
    // The collision check first: the chart below it shades what it finds.
    collision_rows(app, ui, a, study, step, playing, inputs, &t);
    plot_rows(app, ui, a, study, step, playing, total, inputs, &t);
}

/// The Collisions rows: Check collisions and what it found (a click on a pair shows its first step).
#[allow(clippy::too_many_arguments)]
fn collision_rows(
    app: &SolveApp,
    ui: &mut egui::Ui,
    a: &mut Analysis,
    study: &Result<Value, String>,
    step: &mut f64,
    playing: &mut bool,
    inputs: &[SelInput],
    t: &Tokens,
) {
    // Collisions over the whole study: of the picked components (against the other picks, if
    // any), else among all components or in the contact sets.
    // Offered when there is a set to check (suppressed ones don't count).
    let assembly = &app.session.doc.assembly;
    let sets = assembly.contact_all || assembly.contact_sets.iter().any(|c| !c.suppressed);
    a.among = if sets { a.among.min(1) } else { 0 };
    let (parts, against) = (picked_occurrences(&app.session, inputs, 1), picked_occurrences(&app.session, inputs, 2));
    let among = if !parts.is_empty() {
        let mut p = json!({"occurrences": parts});
        if !against.is_empty() {
            p["against"] = json!(against);
        }
        p
    } else {
        json!({"contact_sets": a.among == 1})
    };
    let check = study.as_ref().ok().map(|st| (st.clone(), among));
    let key = key_of(&check);
    if a.collisions.as_ref().is_some_and(|(k, _)| *k != key) {
        a.collisions = None;
    }
    crate::ui_kit::section(ui, "motion_collisions", "Collisions", true, |ui| {
        ui.horizontal(|ui| {
            if sets && parts.is_empty() {
                ui.allocate_ui(vec2(t.form_label_width * 1.6, t.control_height), |ui| {
                    combo(ui, "ms_among", &["All components", "Contact sets"], &mut a.among)
                });
            }
            let button = crate::ui_kit::button(ui, "Check collisions", crate::ui_kit::Button::Enabled(check.is_some()));
            crate::scenario::publish_handle("motion:check", button.rect.center());
            if button.clicked()
                && let Some((st, mut p)) = check
            {
                let mut sc = app.session.scratch();
                let r = super::scratch_study(&mut sc, &st).and_then(|id| {
                    p["study"] = id;
                    sc.execute("motion.interference", &p).map_err(|e| e.to_string())
                });
                a.collisions = Some((key, r));
            }
        });
    });
    match a.collisions.as_ref().map(|(_, r)| r) {
        Some(Ok(v)) => {
            let hits = v["collisions"].as_array().cloned().unwrap_or_default();
            ui.vertical(|ui| {
                ui.vertical(|ui| {
                    if hits.is_empty() {
                        ui.label(RichText::new(crate::i18n::tr("motion.no_collisions")).color(t.text_dim));
                    }
                    for (i, c) in hits.iter().enumerate() {
                        let steps: Vec<String> = c["steps"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|r| {
                                let (from, to) = (r[0].as_f64().unwrap_or(0.0).round(), r[1].as_f64().unwrap_or(0.0).round());
                                if from == to { format!("{from}") } else { format!("{from}-{to}") }
                            })
                            .collect();
                        let name = |k: &str| c[k].as_str().unwrap_or_default().to_string();
                        let text = crate::i18n::format("motion.collision", &[("a", &name("a")), ("b", &name("b")), ("steps", &steps.join(", "))]);
                        // A click shows where they first collide.
                        let link = ui.link(RichText::new(text).color(t.error));
                        crate::scenario::publish_handle(&format!("motion:collision:{i}"), link.rect.center());
                        if link.clicked()
                            && let Some(first) = c["first_step"].as_f64()
                        {
                            (*step, *playing) = (first, false);
                        }
                    }
                    if v["truncated"] == true {
                        let to = v["checked_to"].as_f64().map(|x| x.round().to_string()).unwrap_or_default();
                        ui.label(RichText::new(crate::i18n::format("motion.checked_to", &[("step", &to)])).color(t.warning));
                    }
                });
            });
        }
        Some(Err(e)) => {
            ui.vertical(|ui| {
                ui.add(egui::Label::new(RichText::new(crate::i18n::tr_label(e)).color(t.error)).wrap());
            });
        }
        None => {}
    }
}

/// The Plot rows: what to chart, and the chart (a click on it moves the step).
#[allow(clippy::too_many_arguments)]
fn plot_rows(
    app: &SolveApp,
    ui: &mut egui::Ui,
    a: &mut Analysis,
    study: &Result<Value, String>,
    step: &mut f64,
    playing: &mut bool,
    total: u32,
    inputs: &[SelInput],
    t: &Tokens,
) {
    let s = &app.session;
    let picked = inputs.first().is_some_and(|i| !i.items.is_empty());
    let list = choices(s, picked);
    a.what = a.what.min(list.len().saturating_sub(1));
    // A joint picked in the browser is charted (until the box or another pick says otherwise).
    let browsed = app.tree.picked_joints.first().copied();
    if browsed != a.followed {
        a.followed = browsed;
        if let Some(i) = browsed.and_then(|id| list.iter().position(|(_, w)| *w == Plotted::Joint(id, 0))) {
            a.what = i;
        }
    }
    form_row(ui, "Plot", |ui| {
        let labels: Vec<&str> = list.iter().map(|(l, _)| l.as_str()).collect();
        combo(ui, "ms_plot", &labels, &mut a.what);
    });
    let what = list.get(a.what).map_or(Plotted::Nothing, |(_, w)| *w);
    let bands = a.collisions.as_ref().and_then(|(_, r)| r.as_ref().ok()).map(collision_bands).unwrap_or_default();
    if what != Plotted::Nothing {
        choice_row(ui, "motion.plot_quantity", "ms_quantity", &mut a.quantity, &QUANTITIES);
        form_row(ui, "Duration", |ui| {
            let r = crate::ui_kit::cell_editor(ui, ui.id().with("duration"), &mut a.duration, "motion.duration_hint", false);
            crate::scenario::publish_handle("dialog:Duration", r.rect.center());
        });
        let params = study.clone().and_then(|st| plot_params(s, what, &st, &a.duration, inputs).map(|p| (st, p)));
        let key = key_of(&params);
        if a.plot.as_ref().is_none_or(|(k, _)| *k != key) {
            let r = params.and_then(|(st, mut p)| {
                let mut sc = s.scratch();
                p["study"] = super::scratch_study(&mut sc, &st)?;
                sc.execute("motion.plot", &p).map_err(|e| e.to_string())
            });
            a.plot = Some((key, r));
        }
        ui.vertical(|ui| match a.plot.as_ref().map(|(_, r)| r) {
            Some(Ok(v)) => {
                ui.vertical(|ui| {
                    if let Some((xs, ys, unit)) = line(v, what, a.quantity)
                        && let Some(to) = chart(ui, &xs, &ys, &unit, total, *step, &bands, t)
                    {
                        (*step, *playing) = (to, false);
                    }
                    if let Some(why) = stop_text(v) {
                        ui.add(egui::Label::new(RichText::new(why).color(t.warning)).wrap());
                    }
                });
            }
            Some(Err(e)) => {
                ui.add(egui::Label::new(RichText::new(crate::i18n::tr_label(e)).color(t.text_dim)).wrap());
            }
            None => {}
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The chart's line: a joint's quantity with its unit, or a point's distance from its start.
    #[test]
    fn lines_come_from_the_plot_answer() {
        let v = json!({"step": [0.0, 1.0, 2.0],
            "joints": [{"index": 0, "unit": "deg", "velocity_unit": "deg/s", "acceleration_unit": "deg/s²",
                "value": [0, 10, 30], "velocity": [5, 15, 25], "acceleration": [10, 10, 10]}],
            "point": {"unit": "mm", "velocity_unit": "mm/step", "acceleration_unit": "mm/step²",
                "position": [[0, 0, 0], [3, 4, 0], [6, 8, 0]], "speed": [5, 5, 5], "acceleration_norm": [0, 0, 0]}});
        assert_eq!(line(&v, Plotted::Joint(7, 0), 1), Some((vec![0.0, 1.0, 2.0], vec![5.0, 15.0, 25.0], "deg/s".into())));
        assert_eq!(line(&v, Plotted::Point, 0).map(|l| l.1), Some(vec![0.0, 5.0, 10.0]));
        assert_eq!(line(&v, Plotted::Point, 2).map(|l| l.2), Some("mm/step²".into()));
        assert_eq!(line(&v, Plotted::Joint(7, 1), 0), None);
        assert_eq!(line(&v, Plotted::Nothing, 0), None);
        assert_eq!(value_at(&[0.0, 2.0, 4.0], &[0.0, 10.0, 30.0], 3.0), Some(20.0));
        assert_eq!(value_at(&[0.0, 2.0], &[1.0, 3.0], 9.0), None);
        let hits = json!({"collisions": [{"steps": [[11.0, 15.0], [20.0, 20.0]]}]});
        assert_eq!(collision_bands(&hits), vec![[11.0, 15.0], [20.0, 20.0]]);
        // A duration is a number of seconds, more than 0; empty is per step.
        let s = Session::default();
        let study = json!({"name": "S", "steps": 10});
        let p = |d: &str| plot_params(&s, Plotted::Joint(7, 0), &study, d, &[]);
        assert_eq!(p("2").unwrap()["duration"], json!(2.0));
        assert!(p("").unwrap().get("duration").is_none());
        for bad in ["1e400", "nan", "-1", "0", "soon"] {
            assert!(p(bad).is_err(), "{bad}");
        }
    }

    /// The analysis of the open Motion Study dialog, and its step.
    fn shown(h: &crate::scenario::Harness) -> (Analysis, f64) {
        match h.app.dialog.as_ref().map(|d| &d.kind) {
            Some(crate::dialogs::Kind::Motion(crate::dialogs_motion::Mo::MotionStudy { analysis, step, .. })) => ((**analysis).clone(), *step),
            _ => panic!("no Motion Study dialog"),
        }
    }

    /// In the dialog: a slider pushing B 1 mm a step from 10 mm off A to 5 mm into it. Plotting
    /// the slider's velocity draws a chart of -1 mm/step; a click on the chart moves the step
    /// there, and Check collisions names A and B from step 11.
    #[test]
    fn the_study_dialog_charts_a_joint_and_finds_collisions() {
        let mut h = crate::scenario::Harness::new();
        for (name, x) in [("A", 0), ("B", 20)] {
            h.app.run("component.activate", json!({"component": "root"})).unwrap();
            h.app.run("component.create", json!({ "name": name })).unwrap();
            h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]})).unwrap();
        }
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        h.app.run("occurrence.ground", json!({"occurrence": "A:1", "grounded": true})).unwrap();
        h.app.run("joint.create", json!({"type": "slider", "a": {"face": [10, 5, 5]}, "b": {"face": [20, 5, 5]}, "name": "Rail"})).unwrap();
        h.app.run("joint.drive", json!({"joint": "Rail", "value": 10})).unwrap();
        let push = h
            .app
            .run("motion.study", json!({"name": "Push", "steps": 15, "keys": [{"joint": "Rail", "points": [[0, 10], [15, -5]]}]}))
            .unwrap()["study"]
            .to_string();
        // A study keyed between whole steps or off its timeline is not opened (the dialog would move its keys).
        h.app.run("motion.study", json!({"name": "Fine", "steps": 15, "keys": [{"joint": "Rail", "points": [[0, 10], [7.5, -5]]}]})).unwrap();
        h.app.run("motion.study", json!({"name": "Off", "steps": 15, "keys": [{"joint": "Rail", "points": [[-1, 10], [15, -5]]}]})).unwrap();
        // A tiny key keeps its value in the form (OK writes the study back from it).
        h.app.run("motion.study", json!({"name": "Tiny", "steps": 15, "keys": [{"joint": "Rail", "points": [[0, 0.00004], [15, 3]]}]})).unwrap();
        let (_, _, _, tiny) = crate::dialogs_motion::study_tracks(&h.app.session, "Tiny", false).unwrap();
        assert_eq!(tiny[0].points, vec![(0, "0.00004 mm".to_string()), (15, "3 mm".to_string())]);
        for bad in
            [json!({"study": "Nope"}), json!({"study": "Push", "step": 3}), json!({"study": [1]}), json!({"study": "Fine"}), json!({"study": "Off"})]
        {
            let r = h.call("ui.start", json!({"command": "motion.study", "params": bad}));
            assert!(r["ok"] == false && h.app.dialog.is_none(), "{bad}: {r}");
        }
        // The control channel opens the saved study to edit (and chart).
        let r = h.call("ui.start", json!({"command": "motion.study", "params": {"study": "Push"}}));
        assert_eq!(r["ok"], true, "{r}");
        if let Some(crate::dialogs::Kind::Motion(crate::dialogs_motion::Mo::MotionStudy { study, steps, tracks, analysis, .. })) =
            h.app.dialog.as_mut().map(|d| &mut d.kind)
        {
            assert_eq!((study.as_deref(), steps.as_str()), (Some(push.as_str()), "15"));
            assert_eq!(*tracks, vec![crate::dialogs_motion::Track { joint: 0, index: 0, points: vec![(0, "10 mm".into()), (15, "-5 mm".into())] }]);
            (analysis.what, analysis.quantity) = (1, 1);
        }
        h.frames(4);
        let (a, _) = shown(&h);
        let plot = a.plot.as_ref().and_then(|(_, r)| r.as_ref().ok()).cloned().unwrap_or_default();
        let (_, ys, unit) = line(&plot, Plotted::Joint(0, 0), 1).unwrap();
        assert!(unit == "mm/step" && ys.len() == 16 && ys.iter().all(|v| (v + 1.0).abs() < 1e-9), "{plot}");
        let chart = crate::scenario::rect_of("motion:chart").unwrap();
        // The plot area starts 6 points in: two thirds of the way along is step 10.
        let x = chart.left() + 6.0 + (chart.width() - 12.0) * 2.0 / 3.0;
        h.step(&json!({"click": [x, chart.center().y]})).unwrap();
        h.frames(2);
        assert_eq!(shown(&h).1, 10.0);
        let [bx, by] = h.widget("Check collisions").unwrap();
        h.step(&json!({"click": [bx, by]})).unwrap();
        h.frames(2);
        let (a, _) = shown(&h);
        let found = a.collisions.as_ref().and_then(|(_, r)| r.as_ref().ok()).cloned().unwrap_or_default();
        assert_eq!((&found["collisions"][0]["a"], &found["collisions"][0]["first_step"]), (&json!("A:1"), &json!(11.0)), "{:?}", a.collisions);
        assert_eq!(collision_bands(&found), vec![[11.0, 15.0]]);
    }
}
