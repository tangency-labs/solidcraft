//! Clean-up after operations: faces on one surface that meet along an edge become one face, and
//! two edges on one curve meeting at a vertex nothing else uses become one edge. SMLib does it
//! (`Brep::merge_faces`, its `SmBrep::RemoveTopologicalEdgesAndVertices`; its booleans do the
//! same at their end), keeping apart the faces split on purpose along the body's split lines.

use solidcore_record::Vec3;
use solidcore_record::brep::Brep;
use solidcraft_geom::brep::eval::CurveEval;
use solidcraft_geom::prelude::*;

use crate::Result;
use crate::body::Body;

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

/// Maps kept record edge indices into SMLib's edge order using `order`.
/// Identity (`order.edges` and `order.pieces` empty) returns the record indices directly;
/// otherwise every SMLib edge whose mapped canonical edge or split piece is kept is returned,
/// sorted and deduplicated.
pub(crate) fn smlib_edges_for_record(keep_record: &[usize], record_edge_count: usize, order: &solidcraft_smlib::history::Order) -> Vec<usize> {
    if order.edges.is_empty() && order.pieces.is_empty() {
        return keep_record.to_vec();
    }
    let mut wanted = vec![false; record_edge_count];
    for &e in keep_record {
        if let Some(slot) = wanted.get_mut(e) {
            *slot = true;
        }
    }
    let mut out = Vec::new();
    for (smlib_edge, &rec_edge) in order.edges.iter().enumerate() {
        if wanted.get(rec_edge).copied().unwrap_or(false) {
            out.push(smlib_edge);
        }
    }
    for piece in &order.pieces {
        if wanted.get(piece.canonical).copied().unwrap_or(false) {
            out.push(piece.edge);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// `body` with its faces on one surface merged and its edges on one curve joined, except along
/// the lines in `keep` (its split lines, as Split Face made them): each face and edge of the
/// result comes from those of `body` as SMLib's history says (a merged face from every face
/// merged into it), over `[body]`.
pub(crate) fn heal(body: &Body, keep: &[Vec<Vec3>]) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "heal");
    let record = crate::native::brep_record(body)?;
    // Each edge along a split line, by its record edge index.
    let mut kept_edges: Vec<usize> = Vec::new();
    if !keep.is_empty() {
        for e in 0..record.edges.len() {
            let pts = edge_points(&record, e as u32);
            if on_split_line(&pts, keep) {
                kept_edges.push(e);
            }
        }
    }
    let tolerance = record.tolerance * 10.0;
    let (healed, history) = crate::offset_backend::run_ordered("healing", body, None, |b, order| {
        if let Some(order) = order {
            let smlib_kept = smlib_edges_for_record(&kept_edges, record.edges.len(), order);
            b.merge_faces_keeping_edges(&smlib_kept)
        } else {
            let mut kept_pts = Vec::with_capacity(kept_edges.len());
            for &e in &kept_edges {
                let pts = edge_points(&record, e as u32);
                let Some(mid) = pts.get(2) else {
                    return Err(solidcraft_smlib::Error::Argument);
                };
                kept_pts.push([mid.x, mid.y, mid.z]);
            }
            b.merge_faces(&kept_pts, tolerance)
        }
    })?;
    Ok(healed.with_checked_history(history, &crate::record::counts(&[body])).with_split_lines(body.splits.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_smlib::history::{Order, Piece};

    #[test]
    fn smlib_edges_for_record_handles_identity_permuted_and_split_pieces() {
        // Identity order keeps record indices unchanged.
        assert_eq!(smlib_edges_for_record(&[1, 4, 9], 12, &Order::default()), vec![1, 4, 9]);

        // Permuted `order.edges` maps SMLib edge -> canonical record edge.
        let permuted = Order { faces: Vec::new(), edges: vec![3, 0, 2, 1], pieces: Vec::new() };
        assert_eq!(smlib_edges_for_record(&[0, 2], 4, &permuted), vec![1, 2]);

        // Non-manifold split pieces map back to their shared SMLib edge once.
        let with_pieces = Order {
            faces: Vec::new(),
            edges: vec![0, 1, 2],
            pieces: vec![Piece { edge: 2, canonical: 2, faces: vec![0, 1] }, Piece { edge: 2, canonical: 3, faces: vec![2, 3] }],
        };
        assert_eq!(smlib_edges_for_record(&[3], 4, &with_pieces), vec![2]);
        assert_eq!(smlib_edges_for_record(&[2, 3], 4, &with_pieces), vec![2]);
    }

    /// Regression test for #21834: `heal` and `offset_faces` succeed on a body with more than
    /// `solidcraft_smlib::MAX_POINTS` (4096) edges lying along split lines instead of failing
    /// with `SMLib healing: operation: invalid argument for SMLib`.
    #[test]
    fn heal_and_offset_face_handle_more_than_max_points_split_edges() {
        use solidcore_record::{Loop2, Plane, Region2, Vec2};
        use std::sync::Arc;

        // Extrude a rectangle whose y=0 and y=10 sides are each broken into 1,025 collinear
        // segments: 4 * 1,025 = 4,100 horizontal edges (> MAX_POINTS = 4,096) lie on just four
        // 2-point straight lines, so `on_split_line` and `offset_faces` stay fast while exercising
        // a > 4,096 kept-edge list through both `heal` and `offset_faces`.
        let segs = 1_025usize;
        let len = segs as f64;
        let w = 10.0_f64;
        let h = 10.0_f64;
        let mut ring2 = Vec::with_capacity((segs + 1) * 2);
        for i in 0..=segs {
            ring2.push(Vec2::new(i as f64, 0.0));
        }
        for i in 0..=segs {
            ring2.push(Vec2::new(len - i as f64, w));
        }
        let bar = crate::extrude(&Plane::XY, &[Region2 { outer: Loop2::polygon(&ring2), holes: vec![] }], 0.0, h).unwrap().remove(0);
        assert_eq!(bar.edge_count(), (segs * 2 + 2) * 3);

        // Four straight split lines covering all 4,100 horizontal edges along y=0 and y=w, plus
        // one vertical seam at x=500 on y=0 (4,101 kept edges > MAX_POINTS = 4,096) so the test
        // verifies both that > 4,096 kept edges succeed and that a kept coplanar seam stays split.
        let keep = vec![
            vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(len, 0.0, 0.0)],
            vec![Vec3::new(0.0, 0.0, h), Vec3::new(len, 0.0, h)],
            vec![Vec3::new(0.0, w, 0.0), Vec3::new(len, w, 0.0)],
            vec![Vec3::new(0.0, w, h), Vec3::new(len, w, h)],
            vec![Vec3::new(500.0, 0.0, 0.0), Vec3::new(500.0, 0.0, h)],
        ];
        assert!(segs * 4 + 1 > solidcraft_smlib::MAX_POINTS);

        // Direct heal with 4,101 kept split-line edges succeeds: unkept vertical coplanar seams
        // merge (2054 -> 7 faces), while the kept vertical split at x=500 on y=0 stays apart.
        let healed = heal(&bar, &keep).unwrap();
        assert_eq!(healed.face_count(), 7);

        // Offset Face on the x=0 end face triggers `boolean_record::healed` with `body.split_keep()`:
        // moving x=0 outward by +2 mm succeeds and increases volume from len*w*h to (len+2)*w*h.
        let with_splits = bar.with_split_lines(Some(Arc::new(keep)));
        let raised = crate::offset_faces(&with_splits, &[Vec3::new(0.0, w * 0.5, h * 0.5)], 2.0).unwrap();
        assert_eq!(raised.face_count(), 7);
        let vol_after = raised.volume_at(1e-6).unwrap();
        let expected_vol = (len + 2.0) * w * h;
        assert!((vol_after - expected_vol).abs() < 1e-3, "expected volume ~{expected_vol}, got {vol_after}");
    }
}
