//! The Arrange dialog's sheet in the view: its edge dashed, short striped marks at its corners
//! like a taped-off area, the margin as a fainter dotted line inside it, and its size. Display
//! only: the sheets come from the previewed command's reply.

use egui::{Align2, FontId, Pos2, Shape, Stroke, pos2};
use serde_json::Value;
use solidcore_record::Vec3;

use crate::viewport::Proj;
use solidcraft_theme::Tokens;

/// Most points a drawn ring may have; a finer one is cut down to this many.
const MAX_POINTS: usize = 4_096;
/// Rings with at most this many corners get the taped corner marks.
const TAPED_CORNERS: usize = 64;

/// Which picked parts have no place on the sheet, while the Arrange dialog is open.
pub(crate) enum Unplaced<'a> {
    /// The dialog is not open.
    None,
    /// The preview laid nothing out (no sheet picked yet, say): every picked part.
    All,
    /// The parts the preview found no place for, by name.
    Named(std::collections::HashSet<&'a str>),
}

impl Unplaced<'_> {
    pub(crate) fn contains(&self, name: &str) -> bool {
        match self {
            Unplaced::None => false,
            Unplaced::All => true,
            Unplaced::Named(names) => names.contains(name),
        }
    }
}

/// The picked parts with no place on the sheet ([`Unplaced`]).
pub(crate) fn unplaced(app: &crate::SolveApp) -> Unplaced<'_> {
    if !app.dialog.as_ref().is_some_and(|d| matches!(d.kind, crate::dialogs::Kind::Arrange { .. })) {
        return Unplaced::None;
    }
    let Some(reply) = app.preview.reply.as_ref().filter(|r| r.get("placed").is_some()) else { return Unplaced::All };
    Unplaced::Named(reply.get("unplaced").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect())
}

/// The colour of a picked part with no place on the sheet: the warning colour, apart from the
/// light red of the parts that are laid out ([`picked`]).
pub(crate) fn no_place(t: &Tokens) -> egui::Color32 {
    t.warning
}

/// The colour a part that is laid out is shown in where it stands now: the error colour, a
/// little see-through.
pub(crate) fn picked(t: &Tokens) -> egui::Color32 {
    t.error.gamma_multiply_u8(215)
}

/// Draws the sheets of the Arrange preview, when the Arrange dialog is open and its preview
/// has a reply.
pub(crate) fn paint(app: &crate::SolveApp, painter: &egui::Painter, proj: &Proj) {
    if !app.dialog.as_ref().is_some_and(|d| matches!(d.kind, crate::dialogs::Kind::Arrange { .. })) {
        return;
    }
    let Some(reply) = app.preview.reply.as_ref() else { return };
    let z = reply.get("sheet_z").and_then(Value::as_f64).unwrap_or(0.0);
    let frame = reply.get("sheet_frame").and_then(frame).unwrap_or(XY);
    let at = |x: f64, y: f64| frame[0] * x + frame[1] * y + frame[2] * z;
    let Some(sheets) = reply.get("sheet_outlines").and_then(Value::as_array) else { return };
    let t = Tokens::get();
    let edge = Stroke::new(1.5, t.warning.gamma_multiply(0.85));
    let inner = Stroke::new(1.0, t.warning.gamma_multiply(0.45));
    let tape = Stroke::new(5.0, t.warning.gamma_multiply(0.8));
    // Under the edge, the sketch halo: the dashes read on a light face as on the background.
    let halo = Stroke::new(edge.width + 3.0, t.sketch_halo);
    let clip = painter.clip_rect();
    for (k, sheet) in sheets.iter().enumerate() {
        let Some(outer) = sheet.get("outer").and_then(ring) else { continue };
        // Each ring with the margin's line beside it, worked out by the command with the sheet.
        let raw = |key: &str| sheet.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        let hole_margins = raw("hole_margins");
        let holes: Vec<(Vec<[f64; 2]>, Option<Vec<[f64; 2]>>)> =
            raw("holes").iter().enumerate().filter_map(|(i, h)| Some((ring(h)?, hole_margins.get(i).and_then(ring)))).collect();
        let rings = std::iter::once((&outer, false, sheet.get("outer_margin").and_then(ring))).chain(holes.iter().map(|(h, m)| (h, true, m.clone())));
        for (r, is_hole, margin) in rings {
            let Some(pts) = screen(proj, r, &at) else { continue };
            painter.add(Shape::line(pts.clone(), halo));
            painter.extend(crate::ui_kit::dashed_line(&pts, clip, edge, 8.0, 5.0));
            if let Some(line) = margin.and_then(|m| screen(proj, &m, &at)) {
                painter.extend(crate::ui_kit::dashed_line(&line, clip, inner, 2.0, 5.0));
            }
            if !is_hole && r.len() <= TAPED_CORNERS {
                taped_corners(painter, &pts, tape);
            }
        }
        if k == 0 {
            label(painter, proj, &outer, &at, t.warning.gamma_multiply(0.85));
        }
    }
}

/// A ring of [x, y] points, at most [`MAX_POINTS`] of them (every n-th of a finer one).
fn ring(v: &Value) -> Option<Vec<[f64; 2]>> {
    let all: Vec<[f64; 2]> =
        v.as_array()?.iter().filter_map(|p| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?])).filter(|p| p.iter().all(|c| c.is_finite())).collect();
    if all.len() < 3 {
        return None;
    }
    let every = all.len().div_ceil(MAX_POINTS).max(1);
    Some(all.into_iter().step_by(every).collect())
}

/// The XY plane's frame: along, across and out of it.
const XY: [Vec3; 3] = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)];

/// A tilted sheet's frame from the reply: three finite vectors.
fn frame(v: &Value) -> Option<[Vec3; 3]> {
    let a = v.as_array()?;
    let one = |k: usize| {
        let c = a.get(k)?.as_array()?;
        let n = |i: usize| c.get(i)?.as_f64().filter(|x| x.is_finite());
        Some(Vec3::new(n(0)?, n(1)?, n(2)?))
    };
    Some([one(0)?, one(1)?, one(2)?])
}

/// The ring on screen, closed; none when part of it is behind the camera.
fn screen(proj: &Proj, r: &[[f64; 2]], at: &dyn Fn(f64, f64) -> Vec3) -> Option<Vec<Pos2>> {
    let mut pts: Vec<Pos2> = r.iter().map(|p| proj.to_screen(at(p[0], p[1]))).collect::<Option<_>>()?;
    pts.push(*pts.first()?);
    Some(pts)
}

/// A short striped mark along both edges at each corner of a closed ring on screen.
fn taped_corners(painter: &egui::Painter, pts: &[Pos2], tape: Stroke) {
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        // A strip of tape along each side from each corner, 12% of the side long.
        for (from, to) in [(a, a.lerp(b, 0.12)), (b, b.lerp(a, 0.12))] {
            painter.extend(Shape::dashed_line(&[from, to], tape, 4.0, 4.0));
        }
    }
}

/// The sheet's size by its first corner, above the sheet.
fn label(painter: &egui::Painter, proj: &Proj, r: &[[f64; 2]], at: &dyn Fn(f64, f64) -> Vec3, color: egui::Color32) {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in r {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    let Some(at) = proj.to_screen(at(lo[0], hi[1])) else { return };
    let (w, l) = (format!("{:.0}", hi[0] - lo[0]), format!("{:.0}", hi[1] - lo[1]));
    let text = solidcraft_ui_text::format("arrange.sheet_size", &[("w", &w), ("l", &l)]);
    let t = Tokens::get();
    let galley = painter.layout_no_wrap(text, FontId::proportional(t.font_small), color);
    let rect = Align2::LEFT_BOTTOM.anchor_size(pos2(at.x + 2.0, at.y - 6.0), galley.size());
    // A backing in the sketch halo colour, so the size reads on a light face too.
    painter.rect_filled(rect.expand(2.0), 2.0, t.sketch_halo);
    painter.galley(rect.min, galley, color);
}
