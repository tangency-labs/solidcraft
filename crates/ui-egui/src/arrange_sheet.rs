//! The Arrange dialog's sheet in the view: its edge dashed, short striped marks at its corners
//! like a taped-off area, the margin as a fainter dotted line inside it, and its size. Display
//! only: the sheets come from the previewed command's reply.

use egui::{Align2, FontId, Pos2, Shape, Stroke, pos2};
use serde_json::Value;
use solidcore_record::Vec3;

use crate::theme::Tokens;
use crate::viewport::Proj;

/// Most points a drawn ring may have; a finer one is cut down to this many.
const MAX_POINTS: usize = 4_096;
/// Rings with at most this many corners get the taped corner marks.
const TAPED_CORNERS: usize = 64;

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
    let margin = reply.get("margin_mm").and_then(Value::as_f64).unwrap_or(0.0);
    let Some(sheets) = reply.get("sheet_outlines").and_then(Value::as_array) else { return };
    let t = Tokens::get();
    let edge = Stroke::new(1.5, t.warning.gamma_multiply(0.85));
    let inner = Stroke::new(1.0, t.warning.gamma_multiply(0.45));
    let tape = Stroke::new(5.0, t.warning.gamma_multiply(0.5));
    let clip = painter.clip_rect();
    for (k, sheet) in sheets.iter().enumerate() {
        let Some(outer) = sheet.get("outer").and_then(ring) else { continue };
        let holes: Vec<Vec<[f64; 2]>> = sheet.get("holes").and_then(Value::as_array).into_iter().flatten().filter_map(ring).collect();
        for (r, is_hole) in std::iter::once((&outer, false)).chain(holes.iter().map(|h| (h, true))) {
            let Some(pts) = screen(proj, r, &at) else { continue };
            painter.extend(crate::viewport::dashed_line(&pts, clip, edge, 8.0, 5.0));
            if margin > 0.0
                && let Some(inset) = inset(r, margin, is_hole).and_then(|i| screen(proj, &i, &at))
            {
                painter.extend(crate::viewport::dashed_line(&inset, clip, inner, 2.0, 5.0));
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

/// The ring moved `by` into the sheet: an outer ring inward, a hole's outward. Each edge is
/// moved along its normal and the corners are where the moved edges meet, a sharp one held to
/// four times `by` from the corner it came from.
fn inset(r: &[[f64; 2]], by: f64, hole: bool) -> Option<Vec<[f64; 2]>> {
    let n = r.len();
    let area: f64 = (0..n).map(|i| r[i][0] * r[(i + 1) % n][1] - r[(i + 1) % n][0] * r[i][1]).sum::<f64>() / 2.0;
    if area.abs() < 1e-12 {
        return None;
    }
    // Into a counter-clockwise ring is to the left of its edges.
    let side = if (area > 0.0) != hole { 1.0 } else { -1.0 };
    let normal = |a: [f64; 2], b: [f64; 2]| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        (len > 1e-12).then(|| [-dy / len * side, dx / len * side])
    };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (prev, here, next) = (r[(i + n - 1) % n], r[i], r[(i + 1) % n]);
        let (Some(a), Some(b)) = (normal(prev, here), normal(here, next)) else { continue };
        let (sx, sy) = (a[0] + b[0], a[1] + b[1]);
        let dot = sx * a[0] + sy * a[1];
        let scale = if dot.abs() < 1e-9 { 0.0 } else { (by / dot).min(4.0 * by / (sx * sx + sy * sy).sqrt().max(1e-12)) };
        out.push([here[0] + sx * scale, here[1] + sy * scale]);
    }
    (out.len() >= 3).then_some(out)
}

/// A short striped mark along both edges at each corner of a closed ring on screen.
fn taped_corners(painter: &egui::Painter, pts: &[Pos2], tape: Stroke) {
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = a.distance(b);
        if len < 8.0 {
            continue;
        }
        let reach = (len * 0.12).min(36.0);
        let d = (b - a) / len;
        for (from, dir) in [(a, d), (b, -d)] {
            painter.extend(Shape::dashed_line(&[from, from + dir * reach], tape, 4.0, 4.0));
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
    let text = crate::i18n::format("arrange.sheet_size", &[("w", &w), ("l", &l)]);
    painter.text(pos2(at.x + 2.0, at.y - 6.0), Align2::LEFT_BOTTOM, text, FontId::proportional(Tokens::get().font_small), color);
}

#[cfg(test)]
mod tests {
    use super::inset;

    #[test]
    fn a_square_moves_in_by_the_margin_and_a_hole_out() {
        let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9;
        let sq = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let i = inset(&sq, 1.0, false).unwrap();
        assert!(i.iter().zip([[1.0, 1.0], [9.0, 1.0], [9.0, 9.0], [1.0, 9.0]]).all(|(a, b)| near(*a, b)), "{i:?}");
        let h = inset(&sq, 1.0, true).unwrap();
        assert!(near(h[0], [-1.0, -1.0]), "{h:?}");
        // Clockwise is the same ring.
        let cw: Vec<[f64; 2]> = sq.iter().rev().copied().collect();
        assert!(inset(&cw, 1.0, false).unwrap().iter().all(|p| (1.0..=9.0).contains(&p[0]) && (1.0..=9.0).contains(&p[1])));
        assert!(inset(&[[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]], 1.0, false).is_none());
    }
}
