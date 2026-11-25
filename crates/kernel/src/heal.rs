//! Clean-up after operations: faces on one surface that meet along an edge become one face, and
//! two edges on one curve meeting at a vertex nothing else uses become one edge. SMLib does it
//! (`Brep::merge_faces`, its `SmBrep::RemoveTopologicalEdgesAndVertices`; its booleans do the
//! same at their end), keeping apart the faces split on purpose along the body's split lines.

use solidcore_record::Vec3;
use solidcore_record::brep::Brep;
use solidcraft_geom::brep::eval::CurveEval;
use solidcraft_geom::prelude::*;

use crate::body::Body;
use crate::{KernelError, Result};

pub(crate) fn find(p: &mut [usize], mut i: usize) -> usize {
    while let Some(&q) = p.get(i) {
        if q == i {
            break;
        }
        i = q;
    }
    i
}

fn line_tol(line: &[Vec3]) -> f64 {
    let seg = line.windows(2).filter_map(|w| Some(w.first()?.dist(*w.get(1)?))).fold(0.0, f64::max);
    1e-6 * (1.0 + seg) + if line.len() > 2 { seg * 0.1 } else { 0.0 }
}

fn point_on_line(q: Vec3, line: &[Vec3], tol: f64) -> bool {
    line.windows(2).any(|w| w.first().zip(w.get(1)).is_some_and(|(a, b)| q.dist_to_segment(*a, *b) <= tol))
}

/// Do the points lie along the lines faces were split along on purpose (polylines;
/// chords of a curved line sag by under a tenth of their length)?
///
/// Healing can remove a degree-2 vertex between two consecutive split edges on one curve,
/// merging them into one edge that spans multiple recorded lines in `keep` (#21864). Every
/// sample point must lie on some line in `keep`, not necessarily all on the same single entry.
pub(crate) fn on_split_line(pts: &[Vec3], keep: &[Vec<Vec3>]) -> bool {
    let Some(&first) = pts.first() else { return false };
    let mut hint: Option<(&[Vec3], f64)> = None;
    for line in keep {
        let tol = line_tol(line);
        if point_on_line(first, line, tol) {
            if pts.iter().skip(1).all(|&q| point_on_line(q, line, tol)) {
                return true;
            }
            hint.get_or_insert((line, tol));
        }
    }
    let Some((first_line, first_tol)) = hint else { return false };
    pts.iter().skip(1).all(|&q| point_on_line(q, first_line, first_tol) || keep.iter().any(|line| point_on_line(q, line, line_tol(line))))
}

/// Points along edge `e` of the record `b`: its ends and three inside, evenly in its parameter
/// (the middle one at the middle of its interval).
pub(crate) fn edge_points(b: &Brep, e: u32) -> Vec<Vec3> {
    let Some(edge) = b.edges.get(e as usize) else { return Vec::new() };
    let Ok(c) = CurveEval::new(edge.parameters()) else { return Vec::new() };
    let [t0, t1] = edge.interval;
    (0..=4).filter_map(|k| c.point(t0 + (t1 - t0) * k as f64 / 4.0)).collect()
}

/// `body` with its faces on one surface merged and its edges on one curve joined, except along
/// the lines in `keep` (its split lines, as Split Face made them): each face and edge of the
/// result comes from those of `body` as SMLib's history says (a merged face from every face
/// merged into it), over `[body]`.
pub(crate) fn heal(body: &Body, keep: &[Vec<Vec3>]) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "heal");
    let record = crate::native::brep_record(body)?;
    // Each edge along a split line, by the point halfway along it.
    let mut kept: Vec<[f64; 3]> = Vec::new();
    if !keep.is_empty() {
        for e in 0..record.edges.len() {
            let pts = edge_points(&record, e as u32);
            if on_split_line(&pts, keep) {
                let mid = pts.get(2).ok_or_else(|| KernelError::Failed(format!("healing: edge {e} has no middle point")))?;
                kept.push([mid.x, mid.y, mid.z]);
            }
        }
    }
    let tolerance = record.tolerance * 10.0;
    let (healed, history) = crate::offset_backend::run("healing", body, None, |b| b.merge_faces(&kept, tolerance))?;
    Ok(healed.with_checked_history(history, &crate::record::counts(&[body])).with_split_lines(body.splits.clone()))
}
