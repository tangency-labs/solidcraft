//! A Single hole placed by distances from reference edges (#37). After the face pick the Hole
//! dialog takes up to two straight edges of that face (Reference 1 and 2), each with the hole
//! centre's distance from it (Distance 1 and 2). Picking an edge measures its distance from where
//! the hole is; typing a distance (in the dialog or in its box on the canvas) moves the hole
//! there; moving the hole (another pick, or dragging its handle) measures the distances again.
//! The canvas shows each distance as a dimension from its edge to the hole's centre. OK sends the
//! edges and distances as `solid.hole`'s `references`, so the hole follows its edges when the
//! part changes; Edit Feature picks the edges again where they are now.

use egui::{Color32, Pos2, Stroke, vec2};
use serde_json::{Value, json};
use solidcraft_engine::doc::HoleReference;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::hole_reference::{ReferenceFrame, on_reference_face, reference_frame};
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::{Sel, Session};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::selection::{EDGES, SelInput};
use crate::theme::Tokens;
use crate::viewport::Proj;

/// The reference edge inputs, after the face input.
pub(crate) const REFERENCES: [&str; 2] = ["Reference 1", "Reference 2"];
/// Each reference edge's distance row (and its box on the canvas).
pub(crate) const DISTANCES: [&str; 2] = ["Distance 1", "Distance 2"];

/// The distances typed for the reference edges, what they were last worked out for, the face's
/// frame for the picked edges, and the distance whose canvas box takes the keyboard next (a
/// newly picked edge's, so a number typed straight after the pick sets it).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Refs {
    pub distance: [String; 2],
    seen: Option<Seen>,
    frame: Option<Framed>,
    focus: Option<usize>,
}

/// The place, edges and distances the dialog last had in step.
#[derive(Clone, Debug, PartialEq)]
struct Seen {
    place: Vec3,
    edges: [Option<Vec3>; 2],
    distance: [String; 2],
}

/// The face's frame (with the reference inputs its lines are for) and what it was found for: the
/// design's revision, the place and the edges.
#[derive(Clone, Debug, PartialEq)]
struct Framed {
    revision: u64,
    place: Vec3,
    edges: [Option<Vec3>; 2],
    frame: Option<(ReferenceFrame, Vec<usize>)>,
}

/// The reference edge inputs a Single hole's dialog has after its face input.
pub(crate) fn inputs() -> [SelInput; 2] {
    REFERENCES.map(|label| SelInput::new(label, EDGES, false))
}

/// Is `d` a Single hole's dialog?
fn single(d: &Dialog) -> bool {
    matches!(&d.kind, Kind::Hole { opts, .. } if !opts.multiple)
}

/// The hole's place: the first face pick (its index in the face input, and the point).
fn place(d: &Dialog) -> Option<(usize, Vec3)> {
    d.inputs.first()?.items.iter().enumerate().find_map(|(i, x)| match x {
        Sel::Face { point, .. } => Some((i, *point)),
        _ => None,
    })
}

/// A point on each reference edge picked, by input.
fn edges(d: &Dialog) -> [Option<Vec3>; 2] {
    REFERENCES.map(|label| {
        d.inputs.iter().find(|i| i.label == label).and_then(|i| i.items.first()).and_then(|x| match x {
            Sel::Edge { point, .. } => Some(*point),
            _ => None,
        })
    })
}

/// A point `[x, y, z]` of a dialog's stored parameters.
fn vec3(v: &Value) -> Option<Vec3> {
    let a = v.as_array()?;
    let c = |i: usize| a.get(i).and_then(Value::as_f64).filter(|x| x.is_finite());
    (a.len() == 3).then(|| Some(Vec3::new(c(0)?, c(1)?, c(2)?))).flatten()
}

/// The way the hole drills at `at`: as it was made (editing), else into the face there.
fn direction(s: &Session, d: &Dialog, at: Vec3) -> Option<Vec3> {
    d.extra.get("direction").and_then(vec3).or_else(|| solidcraft_engine::doc::hole_direction_at(&s.world_state(), at))
}

/// The face's frame at the place for the picked edges (and which reference inputs they are), found
/// again only when the design, the place or the edges change.
fn frame(s: &Session, d: &mut Dialog) -> Option<(ReferenceFrame, Vec<usize>)> {
    let (_, at) = place(d)?;
    let edges = edges(d);
    if edges.iter().all(Option::is_none) {
        return None;
    }
    let Kind::Hole { opts, .. } = &d.kind else { return None };
    if let Some(f) = &opts.refs.frame
        && f.revision == s.revision
        && f.place == at
        && f.edges == edges
    {
        return f.frame.clone();
    }
    let picked: Vec<(usize, Vec3)> = edges.iter().enumerate().filter_map(|(k, e)| e.map(|p| (k, p))).collect();
    let found = direction(s, d, at).and_then(|dir| {
        let pts: Vec<(Vec3, Option<&str>)> = picked.iter().map(|(_, p)| (*p, None)).collect();
        reference_frame(&s.world_state(), at, dir, &pts, true).ok().map(|f| (f, picked.iter().map(|(k, _)| *k).collect()))
    });
    if let Kind::Hole { opts, .. } = &mut d.kind {
        opts.refs.frame = Some(Framed { revision: s.revision, place: at, edges, frame: found.clone() });
    }
    found
}

/// Keep the distances and the hole's place in step: a new place or a new edge is measured, a
/// typed distance moves the place there (when it stays on the face). Run before the dialog's
/// commands are made.
pub(crate) fn sync(s: &Session, d: &mut Dialog) {
    if !single(d) {
        return;
    }
    let edges = edges(d);
    let Some((i, at)) = place(d) else {
        if let Kind::Hole { opts, .. } = &mut d.kind {
            opts.refs.seen = None;
        }
        return;
    };
    let framed = frame(s, d);
    let Kind::Hole { opts, .. } = &mut d.kind else { return };
    let refs = &mut opts.refs;
    let mut to = at;
    if let Some((f, picked)) = &framed {
        let same = refs.seen.as_ref().filter(|sn| sn.place == at && sn.edges == edges);
        match same {
            // Typed: the hole goes to the distances.
            Some(sn) if sn.distance != refs.distance => {
                let values: Option<Vec<f64>> =
                    picked.iter().map(|k| refs.distance.get(*k).and_then(|e| s.doc.eval(e, ValueKind::Length).ok())).collect();
                if let Some(c) = values.and_then(|v| f.place(at, &v).ok()).filter(|c| on_reference_face(&s.world_state(), f, *c)) {
                    to = c;
                }
            }
            Some(_) => {}
            // A new place, or a new edge: measured from where the hole is.
            None => {
                let moved = refs.seen.as_ref().is_none_or(|sn| sn.place != at);
                for (k, v) in picked.iter().zip(f.distances(at)) {
                    let new_edge = refs.seen.as_ref().is_none_or(|sn| sn.edges.get(*k) != edges.get(*k));
                    if let Some(slot) = refs.distance.get_mut(*k)
                        && (moved || new_edge || slot.trim().is_empty())
                    {
                        *slot = crate::canvas::format_len(v.max(0.0));
                    }
                    if new_edge && edges.get(*k).is_some_and(Option::is_some) {
                        refs.focus = Some(*k);
                    }
                }
            }
        }
    }
    refs.seen = Some(Seen { place: to, edges, distance: refs.distance.clone() });
    if to != at
        && let Some(Sel::Face { point, .. }) = d.inputs.first_mut().and_then(|inp| inp.items.get_mut(i))
    {
        *point = to;
    }
}

/// The Single hole's selection rows, each reference edge with its distance on the row below it
/// once it is picked. Whether Enter was pressed in a distance.
pub(crate) fn input_rows(d: &mut Dialog, ui: &mut egui::Ui, s: &Session) -> bool {
    let mut enter = false;
    for i in 0..d.inputs.len() {
        crate::dialogs::input_rows_range(d, ui, s, i..i + 1);
        let Some(k) = d.inputs.get(i).filter(|inp| !inp.items.is_empty()).and_then(|inp| REFERENCES.iter().position(|l| *l == inp.label)) else {
            continue;
        };
        if let Kind::Hole { opts, .. } = &mut d.kind
            && let (Some(label), Some(value)) = (DISTANCES.get(k), opts.refs.distance.get_mut(k))
        {
            enter |= crate::ui_kit::expr_row(ui, label, value);
        }
    }
    sync(s, d);
    enter
}

/// The `references` OK sends: each picked edge (a point on it) with its distance.
pub(crate) fn params(d: &Dialog) -> Vec<Value> {
    let Kind::Hole { opts, .. } = &d.kind else { return Vec::new() };
    if opts.multiple {
        return Vec::new();
    }
    edges(d)
        .iter()
        .zip(&opts.refs.distance)
        .filter_map(|(e, distance)| e.map(|p| json!({"edge": [p.x, p.y, p.z], "distance": distance.trim()})))
        .collect()
}

/// Editing a hole with reference edges: the edges picked again where they are now, their
/// distances, and the place where those distances put the hole now.
pub(crate) fn restore(s: &Session, d: &mut Dialog, references: &[HoleReference], names: &[String]) {
    if references.is_empty() {
        return;
    }
    let Some((i, at)) = place(d) else { return };
    let st = s.world_state();
    let edges: Vec<(Vec3, Option<&str>)> = references.iter().enumerate().map(|(k, r)| (r.edge.0, names.get(k).map(String::as_str))).collect();
    let found = direction(s, d, at).and_then(|dir| reference_frame(&st, at, dir, &edges, false).ok());
    let mesh = found.as_ref().and_then(|f| st.body(&f.body)).map(|b| b.mesh());
    for (k, r) in references.iter().enumerate().take(REFERENCES.len()) {
        // Where the edge is now, else where it was picked.
        let sel = found.as_ref().zip(mesh.as_ref()).and_then(|(f, m)| {
            let index = *f.edges.get(k)?;
            let point = crate::selection::polyline_mid(m.edges.get(index)?);
            Some(Sel::Edge { body: f.body.clone(), index, point })
        });
        let sel = sel.or_else(|| nearest_edge(s, at, r.edge.0));
        if let (Some(label), Some(sel)) = (REFERENCES.get(k), sel)
            && let Some(inp) = d.inputs.iter_mut().find(|inp| inp.label == *label)
        {
            inp.items = vec![sel];
        }
        if let Kind::Hole { opts, .. } = &mut d.kind
            && let Some(slot) = opts.refs.distance.get_mut(k)
        {
            *slot = r.distance.to_string();
        }
    }
    // The hole where its distances put it now (the part may have changed since it was placed).
    let mut to = at;
    if let Some(f) = &found
        && let Kind::Hole { opts, .. } = &d.kind
    {
        let values: Option<Vec<f64>> = opts.refs.distance.iter().take(f.lines.len()).map(|e| s.doc.eval(e, ValueKind::Length).ok()).collect();
        if let Some(c) = values.and_then(|v| f.place(at, &v).ok()).filter(|c| on_reference_face(&st, f, *c)) {
            to = c;
        }
    }
    if let Some(Sel::Face { point, .. }) = d.inputs.first_mut().and_then(|inp| inp.items.get_mut(i)) {
        *point = to;
    }
    let edges = self::edges(d);
    if let Kind::Hole { opts, .. } = &mut d.kind {
        opts.refs.seen = Some(Seen { place: to, edges, distance: opts.refs.distance.clone() });
    }
}

/// The edge of the body under `at` nearest `p`, picked at `p` (a reference edge that is not
/// found as it was made: OK then reports why).
fn nearest_edge(s: &Session, at: Vec3, p: Vec3) -> Option<Sel> {
    let Some(Sel::Face { body, .. }) = crate::dialogs::face_sel(s, at) else { return None };
    let st = s.world_state();
    let m = st.body(&body)?.mesh();
    let to = |e: &Vec<Vec3>| e.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min);
    let index = m.edges.iter().enumerate().map(|(i, e)| (to(e), i)).filter(|(d, _)| d.is_finite()).min_by(|a, b| a.0.total_cmp(&b.0))?.1;
    Some(Sel::Edge { body, index, point: p })
}

/// The reference distances on the canvas: a dimension from each edge to the hole's centre, and a
/// box beside it to type the distance in. Whether a box took the keyboard (a newly picked edge's),
/// which the dialog's main value then leaves to it.
pub(crate) fn canvas(app: &SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj, d: &mut Dialog) -> bool {
    if !single(d) {
        return false;
    }
    let Some((_, at)) = place(d) else { return false };
    let Some((f, picked)) = frame(&app.session, d) else { return false };
    let mut took = false;
    let t = Tokens::get();
    let px = 2.0 * app.cam.half_height() / f64::from(proj.rect.height().max(1.0));
    let centre = f.onto_plane(at);
    for (line, k) in f.lines.iter().zip(&picked) {
        let foot = line.foot(centre);
        let (Some(a), Some(b)) = (proj.to_screen(foot), proj.to_screen(centre)) else { continue };
        // The reference edge's line on either side of where the dimension leaves it.
        let reach = line.along * (14.0 * px);
        if let (Some(e0), Some(e1)) = (proj.to_screen(foot - reach), proj.to_screen(foot + reach)) {
            painter.line_segment([e0, e1], Stroke::new(1.2, t.dimension));
        }
        dimension(painter, a, b, t.dimension);
        let (Some(label), Kind::Hole { opts, .. }) = (DISTANCES.get(*k), &mut d.kind) else { continue };
        let focus = opts.refs.focus == Some(*k);
        if focus {
            opts.refs.focus = None;
            took = true;
        }
        let Some(value) = opts.refs.distance.get_mut(*k) else { continue };
        let at = box_beyond(a, b);
        let r = crate::canvas::value_box_in(
            ui,
            (egui::Id::new(("sc_hole_ref_area", *k)), egui::Id::new(("sc_hole_ref", *k))),
            label,
            value,
            at,
            focus,
            false,
        );
        if let Some(r) = r {
            crate::scenario::publish_handle(&format!("canvas:{label}"), r.center());
        }
    }
    took
}

/// Where a distance's box goes: just past its edge (at `a`, the hole's centre at `b`), on the side
/// away from the hole, so it stays clear of the hole's own value box at its centre. The box's side
/// toward the edge sits there.
fn box_beyond(a: Pos2, b: Pos2) -> (Pos2, egui::Align2) {
    const GAP: f32 = 10.0;
    let v = a - b;
    let out = if v.length() < 1.0 { vec2(1.0, 0.0) } else { v / v.length() };
    if out.x.abs() >= out.y.abs() {
        if out.x > 0.0 { (a + vec2(GAP, 0.0), egui::Align2::LEFT_CENTER) } else { (a - vec2(GAP, 0.0), egui::Align2::RIGHT_CENTER) }
    } else if out.y > 0.0 {
        (a + vec2(0.0, GAP), egui::Align2::CENTER_TOP)
    } else {
        (a - vec2(0.0, GAP), egui::Align2::CENTER_BOTTOM)
    }
}

/// A dimension line from `a` to `b`, an arrowhead at each end.
fn dimension(painter: &egui::Painter, a: Pos2, b: Pos2, col: Color32) {
    let v = b - a;
    let l = v.length();
    if l < 1.0 {
        return;
    }
    let u = v / l;
    let n = vec2(-u.y, u.x);
    painter.line_segment([a, b], Stroke::new(1.2, col));
    let head = 8.0f32.min(l * 0.4);
    for (tip, out) in [(a, -u), (b, u)] {
        painter.add(egui::Shape::convex_polygon(
            vec![tip, tip - out * head + n * head * 0.35, tip - out * head - n * head * 0.35],
            col,
            Stroke::NONE,
        ));
    }
}
