//! A sheet of planar faces read back as regions of its plane, and a region's loops made simple
//! for the faces a sheet is built from. The planar booleans of `solidcore-ops` are a recipe over
//! these and the kernel's 2D boolean.
//!
//! Not upstream: SolidCraft's planar regions (`prism.rs`, `sheets`), the geometric steps moved
//! into the kernel (docs/layers.md §1).
//!
//! Deviations: none (no upstream counterpart). No recursion.

use std::collections::HashMap;

use super::profile::{simple_loops, Loop2, Region2, Seg2};
use super::read::{planar_pieces, Run};
use crate::error::{Result, SolidError};
use crate::eval::Frame;
use crate::kernel::Kernel;
use crate::math::Point2;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::api_queries::{face_get_boundary_loops, get_faces};
use crate::surface::surface::SurfaceKind;
use crate::topology::{edge, Brep, VertexId};

const OP: &str = "prism::sheet::sheet_regions";
const OP_LOOPS: &str = "prism::sheet::region_loops";

/// The most work one reading of a sheet may do (curve spans times their knots, and so on).
pub const READ_WORK: usize = 2_000_000;

/// A sheet's faces as regions, and whether two of its faces touch (share a vertex).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SheetRegions {
    /// The regions, outer loops counter-clockwise in the frame, holes clockwise.
    pub regions: Vec<Region2>,
    /// Whether two faces share a vertex.
    pub touching: bool,
}

/// A region's loops as simple loops, each the boundary of one planar face to build: the
/// material ones (counter-clockwise) and the holes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RegionLoops {
    /// The loops bounding material.
    pub material: Vec<Loop2>,
    /// The loops bounding holes (counter-clockwise too: each is the face taken away).
    pub holes: Vec<Loop2>,
}

fn refused(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason,
    }
}

/// `region`'s loops as simple loops for faces of their own: the outer loop counter-clockwise and
/// each hole clockwise, split where a loop passes a point twice (two of its segments starting
/// within ten times `tol` of each other: pieces of material touching at a corner, an outline
/// passing a point twice), as the kernel's face builder takes no point where more than two
/// curves meet. Pieces running counter-clockwise are material, the others holes (turned to run
/// counter-clockwise).
///
/// # Errors
///
/// `Unsupported` where a loop's turning is not proved by its signed area's bound; those of the
/// area.
pub fn region_loops(region: &Region2, tol: f64) -> Result<RegionLoops> {
    let mut out = RegionLoops::default();
    let near = tol * 10.0;
    let oriented = |lp: &Loop2, ccw: bool| -> Result<Loop2> {
        let positive = lp.signed_area()?.sign().ok_or(SolidError::Unsupported {
            operation: OP_LOOPS,
            reason: "a loop whose turning its area does not decide",
        })?;
        Ok(if positive == ccw {
            lp.clone()
        } else {
            lp.reversed()
        })
    };
    let loops = std::iter::once(oriented(&region.outer, true))
        .chain(region.holes.iter().map(|h| oriented(h, false)))
        .collect::<Result<Vec<_>>>()?;
    for lp in loops {
        let keyed = lp.segs.into_iter().map(|s| (s.start(), s)).collect();
        for piece in simple_loops(keyed, |a, b| a.distance(*b) < near) {
            match piece.signed_area()?.sign() {
                Some(true) => out.material.push(piece),
                Some(false) => out.holes.push(piece.reversed()),
                None => {
                    return Err(SolidError::Unsupported {
                        operation: OP_LOOPS,
                        reason: "a loop whose turning its area does not decide",
                    })
                }
            }
        }
    }
    Ok(out)
}

/// The faces of the planar sheet `b` as regions of `frame`: each face's loops read exactly
/// ([`planar_pieces`], each span of a free-form curve a segment), split where a loop passes one
/// vertex twice (a cut-out touching the outline, material meeting at a corner), seen from the
/// face's normal turned to run counter-clockwise in the frame; counter-clockwise pieces are
/// material, the others holes of the smallest material piece of the face holding them
/// ([`Loop2::holds`]). The regions come in a canonical order: each loop starts at its smallest
/// start point (x, then y, each rounded to the grid of `tol` so rounding noise does not decide),
/// and holes and regions are sorted by that point, the larger first where two share it. `tol` (mm, at least the sheet's edges') is how far an edge's curve may end
/// from its vertices.
///
/// # Errors
///
/// `Unsupported` for a face not on a plane parallel to the frame, an edge not read exactly in
/// it, a hole held by no piece or not decided; those of the topology walk.
pub fn sheet_regions(k: &Kernel, b: &Brep, frame: &Frame, tol: f64) -> Result<SheetRegions> {
    let (_, _, _, n) = super::profile::axes(frame);
    let mut work = READ_WORK;
    let mut owner: HashMap<VertexId, usize> = HashMap::new();
    let mut out = SheetRegions::default();
    for (f, face) in get_faces(b)?.into_iter().enumerate() {
        let surface = b
            .face(face)?
            .surface
            .as_ref()
            .ok_or_else(|| refused("a face without a surface"))?;
        let SurfaceKind::Plane(plane) = surface.kind() else {
            return Err(refused("a face not on a plane"));
        };
        let up = plane.evaluate_normal().dot(n);
        if up.abs() < 1.0 - 1e-9 {
            return Err(refused("a face not in the plane of the regions"));
        }
        let loops = face_get_boundary_loops(b, face)?;
        let mut pieces = Vec::new();
        let mut uses = loops.edges.iter().zip(&loops.orientations);
        for &count in &loops.edge_counts {
            let mut segs: Vec<(Option<VertexId>, Seg2)> = Vec::new();
            for (&e, o) in uses.by_ref().take(count) {
                let forward = matches!(o, OrientType::Same);
                let ed = b.edge(e)?;
                let curve = ed
                    .curve
                    .as_ref()
                    .ok_or_else(|| refused("an edge without a curve"))?;
                let (v0, v1) = (edge::start_vertex(b, e)?, edge::end_vertex(b, e)?);
                let (Some(v0), Some(v1)) = (v0, v1) else {
                    return Err(refused("an edge without its vertices"));
                };
                let (p0, p1) = (b.vertex(v0)?.point, b.vertex(v1)?.point);
                let run = Run {
                    curve,
                    interval: ed.interval,
                    forward,
                    ends: if forward { [p0, p1] } else { [p1, p0] },
                };
                // no closer than the kernel holds the edge
                let tol = tol.max(edge::tolerance(b, e)?);
                let read = planar_pieces(k, &run, frame, tol, &mut work, true)
                    .ok_or_else(|| refused("an edge not read exactly in the plane"))?;
                // the first piece of the edge starts on the use's first vertex
                let start = if forward { v0 } else { v1 };
                segs.extend(
                    read.into_iter()
                        .enumerate()
                        .map(|(i, s)| ((i == 0).then_some(start), s)),
                );
                for v in [v0, v1] {
                    out.touching |= *owner.entry(v).or_insert(f) != f;
                }
            }
            for piece in simple_loops(segs, |a, b| a.is_some() && a == b) {
                pieces.push(if up > 0.0 { piece } else { piece.reversed() });
            }
        }
        let first = out.regions.len();
        let mut holes = Vec::new();
        for piece in pieces {
            match piece.signed_area()?.sign() {
                Some(true) => out.regions.push(Region2 {
                    outer: piece,
                    holes: Vec::new(),
                }),
                Some(false) => holes.push(piece),
                None => return Err(refused("a loop whose turning its area does not decide")),
            }
        }
        for hole in holes {
            let mid = hole
                .segs
                .first()
                .map(|s| s.point_at(0.5))
                .ok_or_else(|| refused("an empty loop"))?;
            let mut best: Option<(f64, usize)> = None;
            for (i, r) in out.regions.iter().enumerate().skip(first) {
                if r.outer
                    .holds(mid)
                    .ok_or_else(|| refused("a hole whose place is not decided"))?
                {
                    let area = r.outer.signed_area()?.value;
                    if best.is_none_or(|(a, _)| area < a) {
                        best = Some((area, i));
                    }
                }
            }
            let (_, i) = best.ok_or_else(|| refused("a hole outside its face's material"))?;
            out.regions
                .get_mut(i)
                .ok_or_else(|| refused("a hole outside its face's material"))?
                .holes
                .push(hole);
        }
    }
    canonical(&mut out.regions, tol)?;
    Ok(out)
}

/// `regions` in their canonical order, whatever order the sheet's faces, loops and edges came in:
/// each loop starts at its smallest start point and the holes of a region and the regions are
/// sorted by that point, the larger loop first where two share it. Points compare by
/// [`point_key`] on the grid of `tol`, so coordinates that differ by rounding only (a vertical
/// edge whose ends' x differ in the last bits) compare equal and the next coordinate decides.
fn canonical(regions: &mut Vec<Region2>, tol: f64) -> Result<()> {
    let mut keyed = Vec::new();
    for mut r in std::mem::take(regions) {
        let key = start_at_smallest(&mut r.outer, tol)?;
        let mut holes = Vec::new();
        for mut h in std::mem::take(&mut r.holes) {
            let k = start_at_smallest(&mut h, tol)?;
            holes.push((k, h));
        }
        holes.sort_by(|a, b| by_key(&a.0, &b.0));
        r.holes = holes.into_iter().map(|(_, h)| h).collect();
        keyed.push((key, r));
    }
    keyed.sort_by(|a, b| by_key(&a.0, &b.0));
    regions.extend(keyed.into_iter().map(|(_, r)| r));
    Ok(())
}

/// A point's sort key on the grid of step `tol`: its x and y rounded to the nearest multiple of
/// `tol` (as `x / tol` rounded half away from zero), then its exact x and y. Comparing keys
/// lexicographically with `total_cmp` is a total order. Two points whose coordinates differ by
/// much less than `tol` get the same rounded coordinates, unless one lies within that difference
/// of a cell boundary, a half-odd multiple of `tol` (round coordinates sit at cell centres, not
/// on boundaries); the exact coordinates then break the tie between points of one cell, which
/// are within `tol` of each other in both coordinates. A `tol` that is not positive and finite
/// leaves the exact coordinates alone to decide.
fn point_key(p: Point2, tol: f64) -> [f64; 4] {
    let cell = |c: f64| {
        if tol > 0.0 && tol.is_finite() {
            (c / tol).round()
        } else {
            0.0
        }
    };
    [cell(p.x), cell(p.y), p.x, p.y]
}

fn by_point(a: &[f64; 4], b: &[f64; 4]) -> std::cmp::Ordering {
    a.iter()
        .zip(b)
        .map(|(x, y)| x.total_cmp(y))
        .find(|o| o.is_ne())
        .unwrap_or(std::cmp::Ordering::Equal)
}

/// A loop's sort key: the [`point_key`] of its first start point and the size of its area.
type Key = ([f64; 4], f64);

fn by_key(a: &Key, b: &Key) -> std::cmp::Ordering {
    by_point(&a.0, &b.0).then(b.1.total_cmp(&a.1))
}

/// Turns `lp` to start at the segment whose start point has the smallest [`point_key`] on the
/// grid of `tol` (the first such), and returns that key with the size of the loop's area.
fn start_at_smallest(lp: &mut Loop2, tol: f64) -> Result<Key> {
    let mut best: Option<(usize, [f64; 4])> = None;
    for (i, s) in lp.segs.iter().enumerate() {
        let k = point_key(s.start(), tol);
        if best.is_none_or(|(_, q)| by_point(&k, &q).is_lt()) {
            best = Some((i, k));
        }
    }
    let (i, k) = best.ok_or_else(|| refused("an empty loop"))?;
    let segs = std::mem::take(&mut lp.segs);
    let (before, after): (Vec<_>, Vec<_>) = segs.into_iter().enumerate().partition(|(j, _)| *j < i);
    lp.segs = after.into_iter().chain(before).map(|(_, s)| s).collect();
    Ok((k, lp.signed_area()?.value.abs()))
}
