//! The 3D sketch's constraints and dimensions in the view (#10617), drawn as the planar
//! sketch's are: a glyph beside what each constraint holds (Along Axis as its axis letter in the
//! axis colour), and each dimension with extension lines, arrowheads and its value on it. A
//! distance from a plane or face is a dashed leader to the foot on the plane; an angle is an
//! arc. Show Constraints and Show Dimensions hide them as they hide the planar sketch's.
//!
//! Hovering a glyph names its constraint and lights what it holds; a click selects it and Delete
//! removes it (`sketch.delete`). A double-click on a dimension edits its value in place, in the
//! planar sketch's editor (`crate::dim_view`), which changes its parameter
//! (`parameters.change`).

use std::cell::RefCell;

use egui::{FontId, Pos2, Rect, Shape, Stroke, vec2};
use solidcore_record::{Vec2, Vec3};
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::sketch::{Axis, Sketch, WireConstraint, WireConstraintKind};

use crate::SolveApp;
use crate::sketch_tools::GLYPH;
use crate::theme::Tokens;
use crate::viewport::Proj;

/// A dimension as drawn this frame (screen space), for hit testing.
#[derive(Clone, Debug)]
struct Drawn {
    id: String,
    param: Option<String>,
    text: Rect,
    segs: Vec<[Pos2; 2]>,
}

thread_local! {
    /// Glyphs drawn last frame: constraint id, screen centre.
    static GLYPHS: RefCell<Vec<(String, Pos2)>> = const { RefCell::new(Vec::new()) };
    static DIMS: RefCell<Vec<Drawn>> = const { RefCell::new(Vec::new()) };
    /// The glyph a click selected (its constraint id), for Delete.
    static PICKED: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// How far a linear dimension's line stands off what it measures, and an angle's arc from its
/// corner (screen px).
const OFFSET: f32 = 22.0;
/// Pixels from a dimension line that still count as on it.
const HIT_PX: f32 = 4.0;

fn clear() {
    GLYPHS.with(|g| g.borrow_mut().clear());
    DIMS.with(|d| d.borrow_mut().clear());
}

/// Millimetres one screen pixel spans at `p`, square to the view.
pub(crate) fn mm_per_px(proj: &Proj, p: Vec3) -> Option<f64> {
    let s = proj.to_screen(p)?;
    let (o, d) = proj.ray(s + vec2(1.0, 0.0));
    let mm = (p - o).cross(d).len() / d.len().max(1e-12);
    (mm.is_finite() && mm > 0.0).then_some(mm)
}

fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + ab * t)
}

/// The glyph under a screen position (its constraint id).
pub fn glyph_at(pos: Pos2) -> Option<String> {
    GLYPHS.with(|g| g.borrow().iter().find(|(_, c)| c.distance(pos) <= GLYPH * 0.6).map(|(id, _)| id.clone()))
}

/// Glyphs drawn last frame: (constraint id, screen centre), for tests and the control channel.
pub fn glyph_positions() -> Vec<(String, Pos2)> {
    GLYPHS.with(|g| g.borrow().clone())
}

/// Dimensions drawn last frame: (constraint id, value text centre).
pub fn drawn() -> Vec<(String, Pos2)> {
    DIMS.with(|d| d.borrow().iter().map(|x| (x.id.clone(), x.text.center())).collect())
}

/// The dimension under a screen position: its text first, then its lines.
fn hit(pos: Pos2) -> Option<Drawn> {
    DIMS.with(|d| {
        let d = d.borrow();
        d.iter()
            .find(|x| x.text.expand(2.0).contains(pos))
            .or_else(|| d.iter().find(|x| x.segs.iter().any(|s| seg_dist(pos, s[0], s[1]) <= HIT_PX)))
            .cloned()
    })
}

/// A double-click on a 3D dimension opens its value in the inline editor (true: it was on one).
pub fn double_click(app: &mut SolveApp, pos: Pos2) -> bool {
    if !crate::sketch3d::active(app) {
        return false;
    }
    let Some(d) = hit(pos) else { return false };
    crate::dim_view::open_param(app, d.param, d.text.center());
    true
}

/// A click without a tool on a glyph selects it (true: it was on one); elsewhere, it clears it.
pub fn click(app: &mut SolveApp, pos: Pos2) -> bool {
    let id = glyph_at(pos);
    PICKED.with(|p| *p.borrow_mut() = id.clone());
    let Some(id) = id else { return false };
    let st = crate::open_meshing::session_world(&app.session);
    let name = app
        .session
        .active_sketch
        .and_then(|s| st.sketch(s))
        .and_then(|ss| ss.sketch.wire_constraints.iter().find(|c| c.id == id).map(|c| tooltip(&c.kind, &id)));
    if let Some(name) = name {
        app.set_status(name, false);
    }
    true
}

/// Delete the selected glyph's constraint (true when one was selected).
pub fn delete_picked(app: &mut SolveApp) -> bool {
    let Some(id) = PICKED.with(|p| p.borrow_mut().take()) else { return false };
    if let Err(e) = app.run("sketch.delete", serde_json::json!({ "entities": [id] })) {
        app.set_status(e, true);
    }
    true
}

/// The command that makes a constraint: its label names the glyph.
fn command(k: &WireConstraintKind) -> &'static str {
    match k {
        WireConstraintKind::Coincident { .. } | WireConstraintKind::PointOnLine { .. } | WireConstraintKind::PointOnCurve { .. } => {
            "sketch.constraint.coincident"
        }
        WireConstraintKind::Fix { .. } => "sketch.constraint.fix",
        WireConstraintKind::AlongAxis { .. } => "sketch.constraint.along_axis",
        WireConstraintKind::Parallel { .. } | WireConstraintKind::ParallelToPlane { .. } => "sketch.constraint.parallel",
        WireConstraintKind::Perpendicular { .. } => "sketch.constraint.perpendicular",
        WireConstraintKind::Collinear { .. } => "sketch.constraint.collinear",
        WireConstraintKind::Equal { .. } => "sketch.constraint.equal",
        WireConstraintKind::Tangent { .. } => "sketch.constraint.tangent",
        WireConstraintKind::OnPlane { .. } => "sketch.constraint.on_plane",
        _ => "sketch.dimension",
    }
}

fn tooltip(k: &WireConstraintKind, id: &str) -> String {
    format!("{} ({id})", crate::i18n::tr_label(crate::command_label(command(k))))
}

/// A glyph's icon (the planar sketch's for the same relation), or its letter.
fn look(k: &WireConstraintKind) -> (Option<&'static str>, &'static str) {
    use crate::icons::markers::glyph_icon;
    match k {
        WireConstraintKind::Coincident { .. }
        | WireConstraintKind::PointOnLine { .. }
        | WireConstraintKind::PointOnCurve { .. }
        | WireConstraintKind::OnPlane { .. } => (glyph_icon("Coincident"), "?"),
        WireConstraintKind::Fix { .. } => (glyph_icon("Fix"), "?"),
        WireConstraintKind::AlongAxis { axis, .. } => (None, axis_letter(*axis)),
        WireConstraintKind::Parallel { .. } | WireConstraintKind::ParallelToPlane { .. } => (glyph_icon("Parallel"), "?"),
        WireConstraintKind::Perpendicular { .. } => (glyph_icon("Perpendicular"), "?"),
        WireConstraintKind::Collinear { .. } => (glyph_icon("Collinear"), "?"),
        WireConstraintKind::Equal { .. } => (glyph_icon("Equal"), "?"),
        WireConstraintKind::Tangent { .. } => (glyph_icon("Tangent"), "?"),
        _ => (None, "?"),
    }
}

pub(crate) fn axis_letter(a: Axis) -> &'static str {
    match a {
        Axis::X => "X",
        Axis::Y => "Y",
        Axis::Z => "Z",
    }
}

pub(crate) fn axis_colour(tk: &Tokens, a: Axis) -> egui::Color32 {
    match a {
        Axis::X => tk.axis_x,
        Axis::Y => tk.axis_y,
        Axis::Z => tk.axis_z,
    }
}

/// Where a constraint's glyphs go, from its operand positions (`Sketch::wire_constraint_points`):
/// one beside each thing it holds, a point as it is, a line at its middle with its direction.
fn anchors(k: &WireConstraintKind, at: &[Vec3]) -> Vec<(Vec3, Option<Vec3>)> {
    let mid = |i: usize| Some(((*at.get(i)? + *at.get(i + 1)?) * 0.5, Some(*at.get(i + 1)? - *at.get(i)?)));
    let pt = |i: usize| at.get(i).map(|p| (*p, None));
    match k {
        WireConstraintKind::Coincident { .. }
        | WireConstraintKind::PointOnLine { .. }
        | WireConstraintKind::PointOnCurve { .. }
        | WireConstraintKind::Fix { .. }
        | WireConstraintKind::OnPlane { .. } => pt(0).into_iter().collect(),
        WireConstraintKind::AlongAxis { .. } | WireConstraintKind::ParallelToPlane { .. } => mid(0).into_iter().collect(),
        WireConstraintKind::Parallel { .. } | WireConstraintKind::Perpendicular { .. } | WireConstraintKind::Collinear { .. } => {
            [mid(0), mid(2)].into_iter().flatten().collect()
        }
        // Arcs by their middle points.
        WireConstraintKind::Equal { arcs: true, .. } => [pt(1), pt(4)].into_iter().flatten().collect(),
        WireConstraintKind::Equal { .. } => [mid(0), mid(2)].into_iter().flatten().collect(),
        WireConstraintKind::Tangent { line_end, .. } => pt(*line_end).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// The 3D sketch being edited, with its 3D solve's failing constraints.
fn with_sketch<R>(app: &SolveApp, f: impl FnOnce(&Sketch, &[String], &solidcore_record::Plane) -> R) -> Option<R> {
    let st = crate::open_meshing::session_world(&app.session);
    let ss = app.session.active_sketch.and_then(|s| st.sketch(s))?;
    let failing = ss.report.wires.as_ref().map(|w| w.failing.as_slice()).unwrap_or(&[]);
    Some(f(&ss.sketch, failing, &ss.plane))
}

/// Per frame: the 3D sketch's dimensions, then its glyphs (which keep off the dimensions'
/// values), and what is remembered for picking.
pub fn show(app: &SolveApp, ui: &egui::Ui, painter: &egui::Painter, proj: &Proj) {
    if !crate::sketch3d::active(app) {
        clear();
        PICKED.with(|p| *p.borrow_mut() = None);
        return;
    }
    let hover = ui.input(|i| i.pointer.hover_pos()).filter(|_| ui.rect_contains_pointer(proj.rect));
    let shown = with_sketch(app, |sk, failing, plane| {
        let dims = if sk.view.hide_dimensions { Vec::new() } else { dimensions(app, painter, proj, sk, failing, hover) };
        // Zoomed out far enough, fixed-size glyphs would cover the sketch (#8980).
        let px_per_mm = proj.to_screen(plane.to_world(Vec2::ZERO)).zip(proj.to_screen(plane.to_world(Vec2::X)));
        let zoomed_out = px_per_mm.is_some_and(|(a, b)| {
            !crate::sketch_tools::constraint_glyphs_visible_at_scale(a.distance(b), app.preferences.hide_constraint_glyphs_zoomed_out)
        });
        let texts: Vec<Rect> = dims.iter().map(|d| d.text.expand(2.0)).collect();
        let glyphs = if sk.view.hide_constraints || zoomed_out { Vec::new() } else { glyphs(app, painter, proj, sk, failing, hover, &texts) };
        (dims, glyphs)
    });
    let (dims, glyphs) = shown.unwrap_or_default();
    DIMS.with(|d| *d.borrow_mut() = dims);
    GLYPHS.with(|g| *g.borrow_mut() = glyphs);
}

fn glyphs(
    app: &SolveApp,
    painter: &egui::Painter,
    proj: &Proj,
    sk: &Sketch,
    failing: &[String],
    hover: Option<Pos2>,
    texts: &[Rect],
) -> Vec<(String, Pos2)> {
    let tk = Tokens::get();
    let on_text = |p: Pos2| texts.iter().any(|t| t.intersects(Rect::from_center_size(p, vec2(GLYPH, GLYPH))));
    // Beside the entity, away from its line; glyphs that would overlap step along it.
    let mut placed: Vec<(&WireConstraint, Pos2, Pos2)> = Vec::new();
    for c in sk.wire_constraints.iter().filter(|c| !c.kind.is_dimension()) {
        let Some(at) = sk.wire_constraint_points(&c.kind) else { continue };
        for (p, dir) in anchors(&c.kind, &at) {
            let Some(base) = proj.to_screen(p) else { continue };
            let side = dir.and_then(|d| proj.to_screen(p + d)).map(|q| q - base).filter(|t| t.length() > 0.5).map_or(vec2(0.7, -0.7), |t| {
                let t = t.normalized();
                vec2(-t.y, t.x)
            });
            let step = if dir.is_some() { vec2(side.y, -side.x) } else { vec2(1.0, 0.0) };
            let mut sp = base + side * (GLYPH * 0.9);
            for _ in 0..12 {
                if !placed.iter().any(|(_, q, _)| q.distance(sp) < GLYPH + 1.0) && !on_text(sp) {
                    break;
                }
                sp += step * (GLYPH + 2.0);
            }
            placed.push((c, sp, base));
        }
    }
    let picked = PICKED.with(|p| p.borrow().clone());
    let hovered = hover.and_then(|h| placed.iter().find(|(_, q, _)| q.distance(h) <= GLYPH * 0.6).map(|(c, _, _)| c.id.clone()));
    // What the hovered (or selected) constraint holds, lit up.
    if let Some(c) = sk.wire_constraints.iter().find(|c| Some(&c.id) == hovered.as_ref().or(picked.as_ref())) {
        for q in sk.wire_constraint_points(&c.kind).unwrap_or_default().into_iter().filter_map(|p| proj.to_screen(p)) {
            painter.circle_filled(q, 4.5, tk.accent);
        }
    }
    let mut drawn = Vec::new();
    for (c, sp, base) in &placed {
        let lit = picked.as_ref() == Some(&c.id) || hovered.as_ref() == Some(&c.id);
        let fails = failing.contains(&c.id);
        let col = match (&c.kind, fails, lit) {
            (_, true, _) => crate::viewport::Conflict::glyph_colour(app, &tk),
            (_, _, true) => tk.accent,
            (WireConstraintKind::AlongAxis { axis, .. }, _, _) => axis_colour(&tk, *axis),
            _ => tk.glyph_text,
        };
        if sp.distance(*base) > GLYPH * 1.6 {
            painter.line_segment([*base, *sp], Stroke::new(0.8, tk.text_dim.gamma_multiply(0.6)));
        }
        let (icon, letter) = look(&c.kind);
        crate::sketch_tools::paint_glyph(painter, *sp, icon, letter, col, lit, fails);
        drawn.push((c.id.clone(), *sp));
    }
    if let (Some(id), Some(h)) = (hovered, hover)
        && let Some(c) = sk.wire_constraints.iter().find(|c| c.id == id)
    {
        let at = h + vec2(12.0, 12.0);
        let g = painter.layout_no_wrap(tooltip(&c.kind, &id), FontId::proportional(tk.font_small), tk.text);
        painter.rect_filled(Rect::from_min_size(at, g.size()).expand(3.0), tk.radius_sm.saturating_sub(1), tk.panel);
        painter.galley(at, g, tk.text);
    }
    drawn
}

/// One dimension's drawing on screen: solid and dashed lines, arrowheads (tip, a point behind
/// it), and where its value goes.
#[derive(Default)]
struct Figure {
    lines: Vec<Vec<Pos2>>,
    dashed: Vec<[Pos2; 2]>,
    arrows: Vec<(Pos2, Pos2)>,
    text: Pos2,
}

/// Points along the arc about `c` from the unit direction `u` towards `v`, radius `r` (mm).
fn arc_between(c: Vec3, u: Vec3, v: Vec3, r: f64) -> Option<Vec<Vec3>> {
    let w = v - u * u.dot(v);
    let w = w * (1.0 / (w.len() > 1e-9).then_some(w.len())?);
    let angle = u.cross(v).len().atan2(u.dot(v));
    Some((0..=16).map(|i| angle * f64::from(i) / 16.0).map(|t| c + (u * t.cos() + w * t.sin()) * r).collect())
}

fn unit(v: Vec3) -> Option<Vec3> {
    let l = v.len();
    (l > 1e-12 && l.is_finite()).then(|| v * (1.0 / l))
}

/// A linear dimension between `a` and `b`, its line `off` px to one side on screen.
fn linear(proj: &Proj, a: Vec3, b: Vec3, off: f32) -> Option<Figure> {
    let (sa, sb) = (proj.to_screen(a)?, proj.to_screen(b)?);
    let t = sb - sa;
    if t.length() < 2.0 {
        return Some(Figure { text: sa + vec2(OFFSET, -OFFSET), ..Default::default() });
    }
    let t = t.normalized();
    // Above the line on screen, so a dimension reads the same whichever way it was picked.
    let n = vec2(-t.y, t.x);
    let n = if n.y > 0.0 { -n } else { n };
    let (ea, eb) = (sa + n * off, sb + n * off);
    Some(Figure {
        lines: vec![vec![sa + n * 3.0, ea + n * 4.0], vec![sb + n * 3.0, eb + n * 4.0], vec![ea, eb]],
        arrows: vec![(ea, ea + t * 9.0), (eb, eb - t * 9.0)],
        text: ea + (eb - ea) * 0.5,
        ..Default::default()
    })
}

/// How a 3D dimension is drawn, `off` px out (more for one stepped clear of another's value).
fn figure(proj: &Proj, k: &WireConstraintKind, at: &[Vec3], off: f32) -> Option<Figure> {
    let p = |i: usize| at.get(i).copied();
    match k {
        WireConstraintKind::Distance { .. } => linear(proj, p(0)?, p(1)?, off),
        WireConstraintKind::AxisDistance { axis, .. } => {
            // The run along the axis from `a`, and a dashed line on to `b`.
            let (a, b) = (p(0)?, p(1)?);
            let corner = a + axis.unit() * (b - a).dot(axis.unit());
            let mut f = linear(proj, a, corner, off)?;
            if let (Some(sb), Some(sc)) = (proj.to_screen(b), proj.to_screen(corner)) {
                f.dashed.push([sb, sc]);
            }
            Some(f)
        }
        WireConstraintKind::Angle { a_reversed, b_reversed, .. } => {
            let dir = |s: usize, e: usize, rev: bool| unit(if rev { p(s)? - p(e)? } else { p(e)? - p(s)? });
            let (u, v) = (dir(0, 1, *a_reversed)?, dir(2, 3, *b_reversed)?);
            // At the end the lines share, else at the start of the first.
            let ends = [(0, 2), (0, 3), (1, 2), (1, 3)];
            let corner = ends
                .iter()
                .filter_map(|(i, j)| Some((p(*i)?, p(*i)?.dist(p(*j)?))))
                .min_by(|x, y| x.1.total_cmp(&y.1))
                .filter(|(_, d)| *d < 1e-6)
                .map_or(p(0)?, |(c, _)| c);
            let r = f64::from(off + 8.0) * mm_per_px(proj, corner)?;
            arc_figure(proj, corner, u, v, r)
        }
        WireConstraintKind::Radius { .. } => {
            let arc = solidcraft_engine::sketch::arc_from_3_points(p(0)?, p(1)?, p(2)?)?;
            let (sc, st) = (proj.to_screen(arc.center)?, proj.to_screen(p(1)?)?);
            let d = st - sc;
            let d = if d.length() > 1.0 { d.normalized() } else { vec2(1.0, 0.0) };
            Some(Figure { lines: vec![vec![sc, st]], arrows: vec![(st, st - d * 9.0)], text: st + d * (off * 0.8), ..Default::default() })
        }
        WireConstraintKind::PlaneDistance { plane, .. } => {
            // A dashed leader from the point to its foot on the plane, and a mark there.
            let (o, n) = plane.frame()?;
            let q = p(0)?;
            let foot = q - n * (q - o).dot(n);
            let (sq, sf) = (proj.to_screen(q)?, proj.to_screen(foot)?);
            let along = sf - sq;
            // The value on the leader, stepped out to its side when it would cover another.
            let side = if along.length() > 1.0 { vec2(-along.y, along.x).normalized() } else { vec2(0.0, -1.0) };
            let mut f = Figure { dashed: vec![[sq, sf]], text: sq + along * 0.5 + side * (off - OFFSET), ..Default::default() };
            if along.length() > 12.0 {
                let t = along.normalized();
                f.arrows = vec![(sq, sq + t * 9.0), (sf, sf - t * 9.0)];
            }
            let mm = mm_per_px(proj, foot)?;
            let x = unit(n.cross(if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y }))? * (6.0 * mm);
            let y = unit(n.cross(x))? * (6.0 * mm);
            let mark: Option<Vec<Pos2>> = [x + y, x - y, -x - y, -x + y, x + y].iter().map(|c| proj.to_screen(foot + *c)).collect();
            f.lines.extend(mark);
            Some(f)
        }
        WireConstraintKind::PlaneAngle { plane, .. } => {
            // From the line's start: its shadow on the plane (dashed) and the arc up to it.
            let (_, n) = plane.frame()?;
            let (a, b) = (p(0)?, p(1)?);
            let u = unit(b - a)?;
            let flat = unit(u - n * u.dot(n))?;
            let r = f64::from(off + 8.0) * mm_per_px(proj, a)?;
            let mut f = arc_figure(proj, a, flat, u, r)?;
            if let (Some(sa), Some(sf)) = (proj.to_screen(a), proj.to_screen(a + flat * (r * 1.4))) {
                f.dashed.push([sa, sf]);
            }
            Some(f)
        }
        _ => None,
    }
}

/// An angle drawn as an arc about `c` from `u` to `v` (unit directions), radius `r` mm.
fn arc_figure(proj: &Proj, c: Vec3, u: Vec3, v: Vec3, r: f64) -> Option<Figure> {
    let sc = proj.to_screen(c)?;
    let Some(arc) = arc_between(c, u, v, r) else {
        return Some(Figure { text: sc + vec2(OFFSET, -OFFSET), ..Default::default() });
    };
    let pts: Vec<Pos2> = arc.iter().filter_map(|q| proj.to_screen(*q)).collect();
    let (first, last) = (*pts.first()?, *pts.last()?);
    let mid = *pts.get(pts.len() / 2)?;
    let out = if (mid - sc).length() > 1.0 { (mid - sc).normalized() } else { vec2(0.0, -1.0) };
    Some(Figure {
        arrows: vec![(first, *pts.get(1)?), (last, *pts.get(pts.len().saturating_sub(2))?)],
        lines: vec![pts],
        text: mid + out * 12.0,
        ..Default::default()
    })
}

fn dimensions(app: &SolveApp, painter: &egui::Painter, proj: &Proj, sk: &Sketch, failing: &[String], hover: Option<Pos2>) -> Vec<Drawn> {
    let tk = Tokens::get();
    let font = FontId::monospace(tk.font_body);
    let hovered = hover.and_then(hit).map(|d| d.id);
    let mut placed: Vec<Rect> = Vec::new();
    let mut drawn = Vec::new();
    for c in sk.wire_constraints.iter().filter(|c| c.kind.is_dimension()) {
        let Some(at) = sk.wire_constraint_points(&c.kind) else { continue };
        let col = if failing.contains(&c.id) {
            crate::viewport::Conflict::glyph_colour(app, &tk)
        } else if hovered.as_ref() == Some(&c.id) {
            tk.accent
        } else if c.driven {
            tk.dimension_driven
        } else {
            tk.dimension
        };
        let expr = c.param.as_ref().and_then(|p| app.session.doc.param(p)).map(|p| p.expr.as_str());
        let prefix = match &c.kind {
            WireConstraintKind::Radius { .. } => "R".to_string(),
            WireConstraintKind::AxisDistance { axis, .. } => format!("{} ", axis_letter(*axis)),
            _ => String::new(),
        };
        let text = crate::dim_view::value_label(
            c.kind.value().unwrap_or(0.0),
            c.kind.is_angle(),
            &prefix,
            expr,
            c.driven,
            &app.session.doc.units,
            app.preferences.length_decimals,
        );
        let galley = painter.layout_no_wrap(text, font.clone(), col);
        let size = galley.size() + vec2(6.0, 2.0);
        // A value that would land on one already drawn steps out a row (#44).
        let mut off = OFFSET;
        let mut fig = figure(proj, &c.kind, &at, off);
        for _ in 0..6 {
            let Some(f) = fig.as_ref() else { break };
            let r = Rect::from_center_size(f.text, size);
            if !placed.iter().any(|q| q.expand(-0.5).intersects(r.expand(-0.5))) {
                break;
            }
            off += size.y + 2.0;
            fig = figure(proj, &c.kind, &at, off);
        }
        let Some(fig) = fig else { continue };
        let stroke = Stroke::new(1.0, col);
        let mut segs = Vec::new();
        for line in &fig.lines {
            segs.extend(line.windows(2).map(|w| [w[0], w[1]]));
            painter.add(Shape::line(line.clone(), stroke));
        }
        for [a, b] in &fig.dashed {
            segs.push([*a, *b]);
            painter.add(Shape::dashed_line(&[*a, *b], stroke, 5.0, 4.0));
        }
        for (tip, back) in &fig.arrows {
            crate::icons::markers::dimension_arrow(painter, *tip, *back, col);
        }
        let r = Rect::from_center_size(fig.text, size);
        placed.push(r);
        painter.rect_filled(r, tk.radius_sm / 2, tk.viewport_top.gamma_multiply(0.85));
        painter.galley(r.min + vec2(3.0, 1.0), galley, col);
        drawn.push(Drawn { id: c.id.clone(), param: c.param.clone(), text: r, segs });
    }
    drawn
}
