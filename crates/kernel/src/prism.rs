//! Booleans of planar regions (lines, circular arcs and free-form curves) and the extrusion of
//! planar material from them, both on SMLib's 2D boolean: each operand a sheet of planar faces
//! built from its loops' own curves, combined as regions of the plane, its faces read back. The
//! result's boundaries are the operands' curves, cut at their crossings and never refitted.
//! Also the test of whether a record surface is a wall swept along a direction.

use solidcraft_geom::{Plane, Region2, Seg2, Vec2};

use crate::body::Body;
use crate::{BoolOp, KernelError, Result};

const MAX_PLANAR_SEGMENTS: usize = 8192;

/// How far a free-form boundary may be replaced by lines and arcs where a caller needs lines and
/// arcs only (sheet metal's flat patterns, mm). The booleans here keep free-form curves exact.
pub const PLANAR_FIT_TOL: f64 = 1e-3;

/// Boolean of planar regions bounded by straight lines, circular arcs and free-form segments,
/// exact: every boundary of the result is a piece of a boundary of `a` or `b`, cut where it
/// crosses the other (SMLib's 2D boolean). Disjoint results and retained islands are returned as
/// separate regions. `tolerance` (mm, 1e-9 to 1) is how far apart a loop's segments may end and
/// still close it.
pub fn planar_regions_boolean(a: &[Region2], b: &[Region2], op: BoolOp, tolerance: f64) -> Result<Vec<Region2>> {
    crate::guard("planar region boolean", || {
        if !tolerance.is_finite() || !(1e-9..=1.0).contains(&tolerance) {
            return Err(KernelError::Invalid("planar boolean tolerance must be finite and between 1e-9 and 1 mm".into()));
        }
        check_regions(a.iter().chain(b), tolerance)?;
        match (a.is_empty(), b.is_empty(), op) {
            (true, _, BoolOp::Union) => return Ok(b.to_vec()),
            (true, _, _) | (false, true, BoolOp::Intersect) => return Ok(Vec::new()),
            (false, true, _) => return Ok(a.to_vec()),
            _ => {}
        }
        let r = sheets::combine(&sheets::sheet(a, tolerance)?, &sheets::sheet(b, tolerance)?, op)?;
        Ok(match r {
            Some(sheet) => sheets::regions(&sheet, tolerance)?.regions,
            None => Vec::new(),
        })
    })
}

/// Each loop closed within ten times `tol` and within the segment budget, each segment finite
/// and of some length (a line's ends apart, an arc of a positive radius turning at most once).
fn check_regions<'a>(regions: impl Iterator<Item = &'a Region2>, tol: f64) -> Result<()> {
    let mut edges = 0usize;
    for lp in regions.flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)) {
        edges = edges.saturating_add(lp.segs.len());
        if edges > MAX_PLANAR_SEGMENTS || lp.segs.is_empty() || !lp.is_closed(tol * 10.0) {
            return Err(KernelError::Invalid("planar boolean needs closed loops within its geometry budget".into()));
        }
        let finite = |p: Vec2| p.x.is_finite() && p.y.is_finite() && p.x.abs() < 1e9 && p.y.abs() < 1e9;
        for s in &lp.segs {
            let valid = match *s {
                Seg2::Line { a, b } => finite(a) && finite(b) && a.dist(b) > 1e-12,
                Seg2::Arc { center, radius, start, sweep } => {
                    finite(center)
                        && radius.is_finite()
                        && radius > 0.0
                        && radius < 1e9
                        && start.is_finite()
                        && sweep.is_finite()
                        && sweep.abs() > 1e-12
                        && sweep.abs() <= std::f64::consts::TAU + 1e-9
                }
                Seg2::Cubic { p0, p1, p2, p3 } => [p0, p1, p2, p3].into_iter().all(finite) && p0.dist(p3) > 1e-12,
                Seg2::Conic { a, apex, b, w } => [a, apex, b].into_iter().all(finite) && w.is_finite() && w > 1e-9 && a.dist(b) > 1e-12,
                Seg2::Bezier { n, ref p } => {
                    p.get(..=usize::from(n)).is_some_and(|p| p.iter().copied().all(finite)) && s.start().dist(s.end()) > 1e-12
                }
            };
            if !valid {
                let what = if s.is_line_or_arc() { "lines and circular arcs" } else { "free-form segments" };
                return Err(KernelError::Invalid(format!("planar boolean supports finite nondegenerate {what}")));
            }
        }
    }
    Ok(())
}

/// Extrude normalized planar material as one logical compound, retaining every disjoint
/// region and its holes. The regions are united in the plane first (overlapping ones merge),
/// and pieces of the union that touch each other are refused: apart,
/// each is extruded on its own and the solids joined as one body.
pub fn extrude_planar_regions(plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Body> {
    crate::guard("planar material extrusion", || {
        check_regions(regions.iter(), 1e-7)?;
        if regions.is_empty() {
            return Err(KernelError::Invalid("empty planar material extrusion".into()));
        }
        // One sheet of all the regions: their material united, so overlapping ones merge.
        let union = sheets::sheet(regions, 1e-7)?;
        let united = sheets::regions(&union, 1e-7)?;
        if united.touching {
            return Err(KernelError::Invalid("planar material pieces touch at a boundary".into()));
        }
        if united.regions.is_empty() {
            return Err(KernelError::Invalid("empty planar material extrusion".into()));
        }
        let pieces = crate::extrude(plane, &united.regions, start, end)?;
        crate::ops::join_pieces(pieces)?.ok_or_else(|| KernelError::Failed("empty planar material extrusion".into()))
    })
}

/// The regions of a planar sheet, and whether two of its faces touch (share a vertex).
struct Regions {
    regions: Vec<Region2>,
    touching: bool,
}

#[cfg(feature = "smlib")]
mod sheets {
    use solidcraft_geom::Loop2;
    use solidcraft_geom::Vec3;
    use solidcraft_geom::brep::Loop;
    use solidcraft_geom::brep::Surface;
    use solidcraft_smlib::{BooleanOp, Brep};

    use super::*;
    use crate::prism_curve::{Run, projected};

    fn failed(what: &str, e: impl std::fmt::Display) -> KernelError {
        KernelError::Failed(format!("SMLib planar regions: {what}: {e}"))
    }

    /// `regions` (in the plane's own coordinates) as one sheet on z = 0. SMLib's face builder
    /// refuses curves of which more than two meet at one end point (`CreatePlanarFacesWith3DCurves`,
    /// SmBrep.cpp: "spine points"), as where pieces of material touch at a corner or an outline
    /// passes a point twice. So each loop, oriented (outer counter-clockwise, holes clockwise), is
    /// split where it passes a point twice into simple loops, and each simple loop is a face of its
    /// own; those running counter-clockwise are material, the others holes. Each region is its
    /// material united less its holes united (SMLib's 2D boolean, which takes touching faces), and
    /// the sheet is the regions united: a region may lie in another's hole (the island inside a
    /// ring), where the other's hole must not take it. `tol` (mm): how close a loop's passes
    /// through one point are.
    pub(super) fn sheet(regions: &[Region2], tol: f64) -> Result<Brep> {
        let mut sheets = Vec::with_capacity(regions.len());
        for r in regions {
            let (mut material, mut holes) = (Vec::new(), Vec::new());
            let loops = std::iter::once(r.outer.ccw()).chain(r.holes.iter().map(|h| h.ccw().reversed()));
            for lp in loops {
                let near = tol * 10.0;
                for piece in simple_loops(lp.segs.into_iter().map(|s| (s.start(), s)).collect(), |a, b| a.dist(*b) < near) {
                    let face = face(&piece)?;
                    if piece.signed_area() > 0.0 { material.push(face) } else { holes.push(face) }
                }
            }
            let Some(material) = united(material)? else { continue };
            sheets.push(match united(holes)? {
                Some(holes) => combine(&material, &holes, BoolOp::Cut)?.ok_or_else(|| failed("sheet", "the holes take all the material"))?,
                None => material,
            });
        }
        united(sheets)?.ok_or_else(|| failed("sheet", "the regions hold no material"))
    }

    /// A simple closed loop as one planar face.
    fn face(lp: &Loop2) -> Result<Brep> {
        let curves = lp.segs.iter().map(|s| crate::smlib_sweeps::curve(&Plane::XY, s)).collect::<Result<Vec<_>>>()?;
        Brep::planar_faces(&curves).map_err(|e| failed("sheet", e))
    }

    /// `sheets` united in balanced rounds (each union of two sheets of about the same size);
    /// `None` for none.
    fn united(mut sheets: Vec<Brep>) -> Result<Option<Brep>> {
        while sheets.len() > 1 {
            let mut next = Vec::with_capacity(sheets.len().div_ceil(2));
            let mut iter = sheets.into_iter();
            while let Some(a) = iter.next() {
                next.push(match iter.next() {
                    Some(b) => combine(&a, &b, BoolOp::Union)?.ok_or_else(|| failed("sheet", "a union of faces is empty"))?,
                    None => a,
                });
            }
            sheets = next;
        }
        Ok(sheets.pop())
    }

    /// A closed loop, each segment with where it starts, split where it passes a point twice
    /// (`same` start for two of its segments) into loops that pass every point once.
    fn simple_loops<K>(segs: Vec<(K, Seg2)>, same: impl Fn(&K, &K) -> bool) -> Vec<Loop2> {
        let mut todo = vec![segs];
        let mut out = Vec::new();
        // Each split leaves two shorter loops, so the work is bounded by the segment count.
        while let Some(segs) = todo.pop() {
            let repeat = (0..segs.len()).find_map(|i| (i + 1..segs.len()).find(|&j| same(&segs[i].0, &segs[j].0)).map(|j| (i, j)));
            match repeat {
                Some((i, j)) => {
                    let mut rest = segs;
                    let inner: Vec<(K, Seg2)> = rest.drain(i..j).collect();
                    rest.rotate_left(i);
                    todo.push(inner);
                    todo.push(rest);
                }
                None => out.push(Loop2 { segs: segs.into_iter().map(|(_, s)| s).collect() }),
            }
        }
        out
    }

    /// `a` and `b` combined by `op`; `None` when nothing is left.
    pub(super) fn combine(a: &Brep, b: &Brep, op: BoolOp) -> Result<Option<Brep>> {
        let op = match op {
            BoolOp::Union => BooleanOp::Union,
            BoolOp::Cut => BooleanOp::Subtract,
            BoolOp::Intersect => BooleanOp::Intersect,
        };
        a.boolean2d(b, op).map_err(|e| failed("2D boolean", e))
    }

    /// The faces of `sheet` as regions: each face's loops read off its edges exactly
    /// ([`projected`]), the one running counter-clockwise about the face's normal its outer loop,
    /// turned to run counter-clockwise in the plane and its holes clockwise. `tol` (mm, at least
    /// the record's) is how far an edge's curve may end from its vertices.
    pub(super) fn regions(sheet: &Brep, tol: f64) -> Result<Regions> {
        let record = sheet.to_native().map_err(|e| failed("export", e))?;
        let tol = tol.max(record.tolerance);
        let mut work = 2_000_000usize;
        let mut owner: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        let mut touching = false;
        let mut out = Vec::with_capacity(record.faces.len());
        for (f, face) in record.faces.iter().enumerate() {
            let up = natural_normal(&face.surface).ok_or_else(|| failed("face", format!("face {f} is not on a plane")))?.z;
            if up.abs() < 1.0 - 1e-9 {
                return Err(failed("face", format!("face {f} is not in the plane of the regions")));
            }
            // Each loop as it runs in the plane, then split where it passes a point twice (a
            // cut-out touching the outline, material meeting at a corner): counter-clockwise
            // pieces are material, the others holes in the material that holds them.
            // A loop passes a point twice where it passes one vertex of the record twice: the
            // record's vertices, not their places, which may lie closer together than its
            // tolerance at the ends of a short edge.
            let mut pieces = Vec::new();
            for lp in &face.loops {
                let Loop::Edges { edges } = lp else { return Err(failed("face", format!("face {f} has a vertex loop"))) };
                let mut segs = Vec::new();
                for e in edges {
                    let edge = record.edges.get(e.edge as usize).ok_or_else(|| failed("edge", format!("edge {} is missing", e.edge)))?;
                    let run = Run::of(&record, e.edge as usize, e.forward).ok_or_else(|| failed("edge", format!("edge {} is missing", e.edge)))?;
                    let read = projected(&run, &Plane::XY, tol, &mut work)
                        .ok_or_else(|| failed("edge", format!("edge {} of face {f} is not read exactly in the plane", e.edge)))?;
                    // The first piece of the edge starts on the use's first vertex.
                    let start = if e.forward { edge.vertices[0] } else { edge.vertices[1] };
                    segs.extend(read.into_iter().enumerate().map(|(k, s)| ((k == 0).then_some(start), s)));
                    for v in edge.vertices {
                        touching |= *owner.entry(v).or_insert(f) != f;
                    }
                }
                // Seen from the face's normal, the outer loop runs counter-clockwise and holes clockwise.
                for piece in simple_loops(segs, |a, b| a.is_some() && a == b) {
                    pieces.push(if up > 0.0 { piece } else { piece.reversed() });
                }
            }
            let (outers, holes): (Vec<Loop2>, Vec<Loop2>) = pieces.into_iter().partition(|lp| lp.signed_area() > 0.0);
            let first = out.len();
            out.extend(outers.into_iter().map(|outer| Region2 { outer, holes: Vec::new() }));
            for hole in holes {
                // The smallest material piece of this face holding the hole's middle.
                let mid = hole.segs.first().map(|s| s.point_at(0.5)).ok_or_else(|| failed("face", format!("face {f} has an empty loop")))?;
                let owner = out[first..]
                    .iter_mut()
                    .filter(|r| holds(&r.outer, mid))
                    .min_by(|a, b| a.outer.signed_area().total_cmp(&b.outer.signed_area()))
                    .ok_or_else(|| failed("face", format!("face {f} has a hole outside its material")))?;
                owner.holes.push(hole);
            }
        }
        Ok(Regions { regions: out, touching })
    }

    /// Whether the loop `outer` holds `p`: even-odd against its polyline within a quarter of
    /// `p`'s distance from it, so the polyline's chords cannot pass `p` by (a loop's own
    /// [`Loop2::contains`] reads it within 1e-3 mm, and a hole closer than that to an arc of the
    /// outline was taken for one outside it, a chord cutting it off).
    fn holds(outer: &Loop2, p: Vec2) -> bool {
        let d = outer.segs.iter().map(|s| s.point_at(s.closest_param(p)).dist(p)).fold(f64::INFINITY, f64::min);
        outer.polyline((d * 0.25).clamp(1e-9, 1e-3)).contains(p)
    }

    /// The natural normal of a planar surface record.
    fn natural_normal(s: &Surface) -> Option<Vec3> {
        let v = |p: [f64; 3]| Vec3::new(p[0], p[1], p[2]);
        match s {
            Surface::Plane { u_axis, v_axis, .. } => v(*u_axis).cross(v(*v_axis)).normalized(),
            Surface::PlaneThrough { origin, u_point, v_point } => (v(*u_point) - v(*origin)).cross(v(*v_point) - v(*origin)).normalized(),
            _ => None,
        }
    }
}

#[cfg(not(feature = "smlib"))]
mod sheets {
    use super::*;

    fn refused<T>() -> Result<T> {
        Err(KernelError::Failed("not supported yet: planar region booleans run on SMLib (build with the feature `smlib`)".into()))
    }

    /// A sheet stands for nothing without SMLib.
    pub(super) enum Sheet {}

    pub(super) fn sheet(_: &[Region2], _: f64) -> Result<Sheet> {
        refused()
    }

    pub(super) fn combine(a: &Sheet, _: &Sheet, _: BoolOp) -> Result<Option<Sheet>> {
        match *a {}
    }

    pub(super) fn regions(sheet: &Sheet, _: f64) -> Result<Regions> {
        match *sheet {}
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod planar_tests {
    use super::*;
    use solidcraft_geom::Loop2;
    #[test]
    fn single_prism_intersection_uses_common_height_in_both_orders() {
        for k in [1.0, 0.05] {
            for (lo, height, expected) in [
                (7.5, 10.0, Some((7.5, 17.5))),
                (15.0, 10.0, Some((15.0, 22.5))),
                (-5.0, 15.0, Some((5.0, 10.0))),
                (22.5, 10.0, None),
                (25.0, 10.0, None),
            ] {
                let a = crate::cylinder(solidcraft_geom::Vec3::new(12.5, 5.0, 5.0) * k, solidcraft_geom::Vec3::Z, 2.5 * k, 17.5 * k).unwrap();
                let b =
                    crate::box_solid(solidcraft_geom::Vec3::new(10.0, 5.0, lo) * k, solidcraft_geom::Vec3::new(22.5, 10.0, lo + height) * k).unwrap();
                for (lhs, rhs) in [(&a, &b), (&b, &a)] {
                    let result = crate::boolean(lhs, rhs, BoolOp::Intersect).unwrap();
                    if let Some((bottom, top)) = expected {
                        let body = result.expect("a positive-height half disc is a solid");
                        assert!(body.validity().is_empty());
                        assert!(body.history().is_some(), "clipped caps and walls retain their operand history");
                        let expected = std::f64::consts::PI * (2.5 * k).powi(2) * (top - bottom) * k / 2.0;
                        let actual = crate::measure(&body).unwrap().volume;
                        assert!((actual - expected).abs() < expected * 2e-3, "{lo} {height} {k}: {actual} != {expected}");
                        let bounds = body.mesh(&crate::MeshTolerance::chord(k * 1e-3)).unwrap().bounds();
                        assert!((bounds.min.z - bottom * k).abs() < k * 1e-7);
                        assert!((bounds.max.z - top * k).abs() < k * 1e-7);
                    } else {
                        assert!(result.is_none(), "touching or separated caps have no intersection volume");
                    }
                }
            }
        }
    }
    #[test]
    fn shared_arc_line_endpoint_roundoff_does_not_open_a_union() {
        let a = Region2 { outer: Loop2::polygon(&[Vec2::new(-2., 0.), Vec2::new(2., 0.), Vec2::new(2., 2.), Vec2::new(-2., 2.)]), holes: vec![] };
        let line = |a: (f64, f64), b: (f64, f64)| Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) };
        for epsilon in [-4e-14, 4e-14] {
            let b = Region2 {
                outer: Loop2 {
                    segs: vec![
                        line((1.15, epsilon), (-1.15, -epsilon)),
                        Seg2::Arc { center: Vec2::new(-1.35, epsilon), radius: 0.2, start: 0., sweep: -std::f64::consts::FRAC_PI_2 },
                        line((-1.35, -0.2), (-2.4, -0.2)),
                        line((-2.4, -0.2), (-2.4, -1.)),
                        line((-2.4, -1.), (2.4, -1.)),
                        line((2.4, -1.), (2.4, -0.2)),
                        line((2.4, -0.2), (1.35, -0.2)),
                        Seg2::Arc {
                            center: Vec2::new(1.35, -epsilon),
                            radius: 0.2,
                            start: -std::f64::consts::FRAC_PI_2,
                            sweep: -std::f64::consts::FRAC_PI_2,
                        },
                    ],
                },
                holes: vec![],
            };
            let result = planar_regions_boolean(std::slice::from_ref(&a), std::slice::from_ref(&b), BoolOp::Union, 1e-5).unwrap();
            assert_eq!(result.len(), 1);
            assert!(result[0].outer.is_closed(1e-4));
            assert!((result[0].area() - a.area() - b.area()).abs() < 1e-6);
        }
    }
    #[test]
    fn a_ledge_shorter_than_the_record_tolerance_reads_back_as_one_region() {
        // A plate with a ledge 0.0001 tall along part of its lower edge: SMLib's record holds a
        // tolerance of 2e-5 mm, so the ledge's short side has both ends within ten times that.
        // Read back, its outline is one loop of six lines, not split into a loop of one line.
        let ledged = Region2 {
            outer: Loop2::polygon(
                &[(100.0, 3.0), (100.0, 3.0001), (97.0, 3.0001), (97.0, 60.0), (0.0, 60.0), (0.0, 3.0)].map(|(x, y)| Vec2::new(x, y)),
            ),
            holes: vec![],
        };
        let below = rectangle(Vec2::new(0.0, -12.0), Vec2::new(100.0, 0.0));
        let out = planar_regions_boolean(std::slice::from_ref(&ledged), std::slice::from_ref(&below), BoolOp::Union, 1e-7).unwrap();
        assert_eq!(out.len(), 2, "{out:?}");
        let top = out.iter().find(|r| r.outer.contains(Vec2::new(50.0, 30.0))).unwrap();
        assert!(top.outer.segs.len() == 6 && top.outer.is_closed(1e-9), "{top:?}");
        assert!((top.area() - ledged.area()).abs() < 1e-9, "{}", top.area());
    }
    fn rectangle(a: Vec2, b: Vec2) -> Region2 {
        Region2 { outer: Loop2::polygon(&[a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)]), holes: vec![] }
    }
    #[test]
    fn compound_planar_extrusion_normalizes_overlap_and_retains_islands() {
        let plane = Plane::named("XY").unwrap();
        let a = rectangle(Vec2::ZERO, Vec2::new(10.0, 10.0));
        let b = rectangle(Vec2::new(5.0, 0.0), Vec2::new(15.0, 10.0));
        let body = extrude_planar_regions(&plane, &[a.clone(), b], 0.0, 2.0).unwrap();
        assert!((crate::measure(&body).unwrap().volume - 300.0).abs() < 1e-6);
        let inner = Region2 { outer: Loop2::circle(Vec2::new(5.0, 5.0), 2.0), holes: vec![] };
        let body = extrude_planar_regions(&plane, &[a.clone(), inner.clone()], 0.0, 2.0).unwrap();
        assert!((crate::measure(&body).unwrap().volume - 200.0).abs() < 1e-6);
        let annulus = Region2 { outer: a.outer, holes: vec![Loop2::circle(Vec2::new(5.0, 5.0), 3.0)] };
        let body = extrude_planar_regions(&plane, &[annulus, inner], 0.0, 2.0).unwrap();
        let expected = (100.0 - std::f64::consts::PI * 5.0) * 2.0;
        assert!((crate::measure(&body).unwrap().volume - expected).abs() < 0.1);
        assert_eq!(body.shell_count(), 2);
        let a = rectangle(Vec2::ZERO, Vec2::new(10.0, 10.0));
        let b = rectangle(Vec2::new(10.0, 10.0), Vec2::new(20.0, 20.0));
        assert!(extrude_planar_regions(&plane, &[a, b], 0.0, 2.0).is_err());
        let circles = [Vec2::ZERO, Vec2::new(10.0, 0.0)].map(|center| Region2 { outer: Loop2::circle(center, 5.0), holes: vec![] });
        assert!(extrude_planar_regions(&plane, &circles, 0.0, 2.0).is_err());
    }
    /// An ellipse about `c` with semi-axes `a` (along x) and `b`, as four exact conics.
    fn ellipse(c: Vec2, a: f64, b: f64) -> Loop2 {
        let corners = [c + Vec2::new(a, 0.0), c + Vec2::new(0.0, b), c - Vec2::new(a, 0.0), c - Vec2::new(0.0, b)];
        let w = std::f64::consts::FRAC_1_SQRT_2;
        Loop2 {
            segs: (0..4).map(|i| Seg2::Conic { a: corners[i], apex: corners[i] + corners[(i + 1) % 4] - c, b: corners[(i + 1) % 4], w }).collect(),
        }
    }
    #[test]
    fn free_form_cut_outs_stay_exact_whole_or_cut() {
        let plate = rectangle(Vec2::ZERO, Vec2::new(100.0, 60.0));
        let hole = Region2 { outer: ellipse(Vec2::new(40.0, 30.0), 15.0, 8.0), holes: vec![] };
        let out = planar_regions_boolean(std::slice::from_ref(&plate), std::slice::from_ref(&hole), BoolOp::Cut, 1e-7).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].holes.len(), 1);
        // The whole ellipse comes back as its four exact conics, running as a hole (clockwise):
        // the plate less π·15·8.
        let h = &out[0].holes[0];
        assert_eq!(h.segs.len(), 4);
        assert!(h.segs.iter().all(|s| hole.outer.segs.contains(&s.reversed())), "{h:?}");
        assert!(h.signed_area() < 0.0);
        let exact = 6000.0 - std::f64::consts::PI * 15.0 * 8.0;
        assert!((out[0].area() - exact).abs() < 1e-9, "{} vs {exact}", out[0].area());
        // What the cut takes is the ellipse, whole and exact, counter-clockwise.
        let taken = planar_regions_boolean(std::slice::from_ref(&plate), std::slice::from_ref(&hole), BoolOp::Intersect, 1e-7).unwrap();
        assert!(taken.len() == 1 && taken[0].outer.segs.iter().all(|s| hole.outer.segs.contains(s)), "{taken:?}");
        // An ellipse centred on the plate's edge makes a notch of half the ellipse: its part
        // inside is the ellipse's own conics, cut where they cross the edge, so the notch takes
        // exactly half the ellipse.
        let over = Region2 { outer: ellipse(Vec2::new(100.0, 30.0), 15.0, 8.0), holes: vec![] };
        let out = planar_regions_boolean(std::slice::from_ref(&plate), std::slice::from_ref(&over), BoolOp::Cut, 1e-7).unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].holes.is_empty() && !out[0].outer.is_line_arc());
        let notched = 6000.0 - std::f64::consts::PI * 15.0 * 8.0 / 2.0;
        assert!((out[0].area() - notched).abs() < 1e-6, "{} vs {notched}", out[0].area());
        // Two ellipses that overlap join into one outline of their conics; apart, both stay whole.
        let beside = |dx: f64| Region2 { outer: ellipse(Vec2::new(40.0 + dx, 30.0), 15.0, 8.0), holes: vec![] };
        let joined = planar_regions_boolean(std::slice::from_ref(&hole), &[beside(20.0)], BoolOp::Union, 1e-7).unwrap();
        assert!(joined.len() == 1 && joined[0].outer.segs.iter().all(|s| !s.is_line_or_arc()), "{joined:?}");
        let apart = planar_regions_boolean(std::slice::from_ref(&hole), &[beside(40.0)], BoolOp::Union, 1e-7).unwrap();
        assert!(apart.len() == 2 && apart.iter().all(|r| r.outer.segs.len() == 4 && !r.outer.is_line_arc()), "{apart:?}");
        // The flat material extrudes with the exact elliptic wall: four conic faces, two caps.
        let body = extrude_planar_regions(&Plane::XY, &planar_regions_boolean(&[plate], &[hole], BoolOp::Cut, 1e-7).unwrap(), 0.0, 2.0).unwrap();
        assert_eq!(body.face_count(), 4 + 4 + 2);
        let v = crate::measure(&body).unwrap().volume;
        assert!((v - exact * 2.0).abs() < 1e-3 * exact, "{v}");
    }
    #[test]
    fn free_form_loops_that_are_not_finite_are_refused() {
        let plate = rectangle(Vec2::ZERO, Vec2::new(100.0, 60.0));
        let mut bad = ellipse(Vec2::new(40.0, 30.0), 15.0, 8.0);
        bad.segs[1] = Seg2::Conic { a: bad.segs[1].start(), apex: Vec2::new(f64::NAN, 0.0), b: bad.segs[1].end(), w: 1.0 };
        let e = planar_regions_boolean(std::slice::from_ref(&plate), &[Region2 { outer: bad, holes: vec![] }], BoolOp::Cut, 1e-7).unwrap_err();
        assert!(e.to_string().contains("free-form"), "{e}");
    }
    #[test]
    fn many_small_cut_outs_far_apart_cut_as_before() {
        // 400 round holes in a strip: almost every pair of segments is far apart.
        let plate = rectangle(Vec2::ZERO, Vec2::new(2000.0, 20.0));
        let holes: Vec<Region2> =
            (0..400).map(|i| Region2 { outer: Loop2::circle(Vec2::new(2.5 + 5.0 * i as f64, 10.0), 1.8), holes: vec![] }).collect();
        let out = planar_regions_boolean(&[plate], &holes, BoolOp::Cut, 1e-7).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].holes.len(), 400);
        let exact = 40_000.0 - 400.0 * std::f64::consts::PI * 1.8 * 1.8;
        assert!((out[0].area() - exact).abs() < 1e-6, "{}", out[0].area());
    }
    /// Loops are closed only within the chaining tolerance. A ray level with one end of such a
    /// gap slipped through it without a crossing, so a point beside the loop counted as inside:
    /// a sheet shrunk by a parameter change (sketch solver residuals of 1e-13 mm) lost a bend
    /// zone that way and its unfold failed.
    #[test]
    fn ray_parity_closes_the_gaps_chaining_leaves() {
        let gap = 1e-13;
        let square = Loop2 {
            segs: vec![
                Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) },
                Seg2::Line { a: Vec2::new(10.0, 0.0), b: Vec2::new(10.0, 10.0) },
                Seg2::Line { a: Vec2::new(10.0, 10.0), b: Vec2::new(0.0, 10.0) },
                Seg2::Line { a: Vec2::new(0.0, 10.0), b: Vec2::new(0.0, 5.0 + gap) },
                Seg2::Line { a: Vec2::new(0.0, 5.0), b: Vec2::new(0.0, 0.0) },
            ],
        };
        // A neighbour with an edge level with the gap joins the square instead of being
        // dropped as if it lay inside it (which left the outline open: no result at all).
        let a = Region2 { outer: square, holes: vec![] };
        let b = rectangle(Vec2::new(-4.0, 2.0), Vec2::new(0.0, 5.0));
        let union = planar_regions_boolean(&[a], &[b], BoolOp::Union, 1e-7).unwrap();
        assert_eq!(union.len(), 1);
        assert!((union[0].area().abs() - 112.0).abs() < 1e-9, "{}", union[0].area());
    }
    /// Two edges lying on one another within the tolerance, a hair from parallel, meet at a
    /// point somewhere along their overlap. Taken for a crossing, that point split the square's
    /// edge once more: 5e-8 mm before its end, which left an edge shorter than the tolerance in
    /// the result, or in the middle of a straight edge. Edges on one line within the tolerance
    /// are one edge of the result: where they meet is no corner.
    #[test]
    fn overlapping_lines_a_hair_from_parallel_do_not_cross() {
        let square = rectangle(Vec2::ZERO, Vec2::new(10.0, 10.0));
        // A quadrilateral from x = 4 to x = 14 with one edge along the square's bottom edge,
        // tilted by 1e-9 rad about (meet, 0), and the opposite edge at y = far.
        let quad = |meet: f64, far: f64| {
            let on = |x: f64| Vec2::new(x, (meet - x) * 1e-9);
            let mut corners = [on(4.0), on(14.0), Vec2::new(14.0, far), Vec2::new(4.0, far)];
            if far < 0.0 {
                corners.reverse();
            }
            Region2 { outer: Loop2::polygon(&corners), holes: vec![] }
        };
        let lengths = |r: &Region2| {
            let mut all: Vec<f64> = r.outer.segs.iter().map(Seg2::length).collect();
            all.sort_by(f64::total_cmp);
            all
        };
        let same = |got: &[f64], want: &[f64]| got.len() == want.len() && got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-6);
        for meet in [10.0 - 5e-8, 7.0] {
            // Below the square, against its bottom edge: cut away, it leaves the square, its four
            // edges whole (the quadrilateral's corner at x = 4 lies within 1e-8 of the bottom
            // edge, on its line, and its edge from there is no more than 1e-8 off it).
            let below = quad(meet, -5.0);
            let cut = planar_regions_boolean(std::slice::from_ref(&square), std::slice::from_ref(&below), BoolOp::Cut, 1e-7).unwrap();
            assert_eq!(cut.len(), 1, "{meet}");
            assert!(same(&lengths(&cut[0]), &[10.0, 10.0, 10.0, 10.0]), "{meet}: cut edges {:?}", lengths(&cut[0]));
            assert!((cut[0].area() - 100.0).abs() < 1e-6, "{meet}: {}", cut[0].area());
            // Over the square, flush with that edge and reaching past it to x = 14: the
            // union's outline runs along y = 0 once, one edge from 0 to 14 (the quadrilateral's
            // bottom edge lies within 1e-8 of the line y = 0 all along).
            let flush = quad(meet, 5.0);
            let union = planar_regions_boolean(std::slice::from_ref(&square), std::slice::from_ref(&flush), BoolOp::Union, 1e-7)
                .unwrap_or_else(|e| panic!("{meet}: union {e}"));
            assert_eq!(union.len(), 1, "{meet}");
            assert!(same(&lengths(&union[0]), &[4.0, 5.0, 5.0, 10.0, 10.0, 14.0]), "{meet}: union edges {:?}", lengths(&union[0]));
            assert!((union[0].area() - 120.0).abs() < 1e-6, "{meet}: {}", union[0].area());
        }
    }
    #[test]
    fn tiny_tool_between_circle_and_display_chord_is_not_lost() {
        let circle = Region2 { outer: Loop2::circle(Vec2::ZERO, 10.0), holes: vec![] };
        let sampled = circle.outer.polyline(1e-3);
        let direction = (sampled[0] + sampled[1]).normalized().unwrap();
        let at = direction * 9.9998;
        let half = Vec2::new(0.000025, 0.000025);
        let square = rectangle(at - half, at + half);
        assert!(square.outer.segs.iter().all(|s| s.start().len() < 10.0));
        assert!(!circle.contains(at), "fixture must lie outside the display chord");
        let cut = planar_regions_boolean(std::slice::from_ref(&circle), std::slice::from_ref(&square), BoolOp::Cut, 1e-7).unwrap();
        assert_eq!(cut.len(), 1);
        assert_eq!(cut[0].holes.len(), 1);
        assert!((cut[0].holes[0].signed_area().abs() - 2.5e-9).abs() < 1e-13);
        let intersect = planar_regions_boolean(&[circle], &[square], BoolOp::Intersect, 1e-7).unwrap();
        assert_eq!(intersect.len(), 1);
        assert!((intersect[0].area() - 2.5e-9).abs() < 1e-13);
    }
    /// A 200 x 37 plate turned by `theta` about the origin, and the fillet region of one of its
    /// corners: an arc of radius `r` tangent to both edges there, closed by two lines that meet
    /// `reach` out past the corner.
    fn plate_and_corner_fillet(theta: f64, r: f64, reach: f64) -> (Region2, Region2) {
        let (sn, cs) = theta.sin_cos();
        let rot = |x: f64, y: f64| Vec2::new(x * cs - y * sn, x * sn + y * cs);
        let (w, lo, hi) = (200.0, -40.25, -3.25);
        let plate = Region2 { outer: Loop2::polygon(&[rot(0.0, hi), rot(0.0, lo), rot(w, lo), rot(w, hi)]), holes: vec![] };
        let arc = Seg2::Arc { center: rot(w - r, lo + r), radius: r, start: theta, sweep: -std::f64::consts::FRAC_PI_2 };
        let out = rot(w + reach, lo - reach);
        let cut =
            Region2 { outer: Loop2 { segs: vec![arc, Seg2::Line { a: arc.end(), b: out }, Seg2::Line { a: out, b: arc.start() }] }, holes: vec![] };
        (plate, cut)
    }
    /// A contact between a line and an arc that touch is one point. The quadratic's two roots
    /// there are rounding noise: a 1e-12 mm error in the centre's distance from the line moves
    /// them 2e-6 mm either way along it, and the cut then ran that far along the plate's edge
    /// past the arc's end before turning onto the arc: an edge a few millionths of a millimetre
    /// long in the result. Which turns of the plate do that is a matter of rounding, so every
    /// one is tried.
    #[test]
    fn an_arc_tangent_to_the_edges_it_ends_on_leaves_no_hairline_edges() {
        for k in 0..=360 {
            let theta = (k as f64 * 0.25).to_radians();
            for r in [2.0, 0.5, 25.0] {
                let (plate, cut) = plate_and_corner_fillet(theta, r, 0.4);
                let out = planar_regions_boolean(std::slice::from_ref(&plate), std::slice::from_ref(&cut), BoolOp::Cut, 1e-7)
                    .unwrap_or_else(|e| panic!("{k} r {r}: {e}"));
                assert_eq!(out.len(), 1, "{k} r {r}");
                let lp = &out[0].outer;
                let shortest = lp.segs.iter().map(Seg2::length).fold(f64::MAX, f64::min);
                let arcs = lp.segs.iter().filter(|s| matches!(s, Seg2::Arc { .. })).count();
                assert!(
                    out[0].holes.is_empty() && lp.segs.len() == 5 && arcs == 1 && shortest > 0.5,
                    "{k} r {r}: {} edges, shortest {shortest:e}",
                    lp.segs.len()
                );
                let want = 200.0 * 37.0 - r * r * (1.0 - std::f64::consts::FRAC_PI_4);
                assert!((out[0].area() - want).abs() < 1e-9, "{k} r {r}: {} vs {want}", out[0].area());
                // What the cut takes is the corner's fillet region, whole.
                let taken = planar_regions_boolean(std::slice::from_ref(&plate), std::slice::from_ref(&cut), BoolOp::Intersect, 1e-7).unwrap();
                assert!(taken.len() == 1 && taken[0].outer.segs.len() == 3, "{k} r {r}: {taken:?}");
                assert!((taken[0].area() - r * r * (1.0 - std::f64::consts::FRAC_PI_4)).abs() < 1e-9, "{k} r {r}");
            }
        }
    }
    /// A 55 x 30 plate whose bottom edge bends up by `slope` over its last 5 mm before the
    /// corner at (100, 50), and a strip 3 wide below the straight 50 mm, as a sheet's bend zone
    /// lies against its base: the strip's corners on that edge rounded to the nanometre, as a
    /// flange keeps them, and moved by `off`. Both turned by `theta` about the origin.
    fn plate_and_strip_by_a_bent_edge(theta: f64, slope: f64, off: Vec2) -> (Region2, Region2) {
        let (sn, cs) = theta.sin_cos();
        let rot = |x: f64, y: f64| Vec2::new(x * cs - y * sn, x * sn + y * cs);
        let nm = |p: Vec2| Vec2::new((p.x * 1e9).round() / 1e9, (p.y * 1e9).round() / 1e9) + off;
        let plate = [rot(95.0, 50.0 + 5.0 * slope), rot(100.0, 50.0), rot(150.0, 50.0), rot(150.0, 80.0), rot(95.0, 80.0)];
        let strip = [nm(rot(100.0, 50.0)), rot(100.0, 47.0), rot(150.0, 47.0), nm(rot(150.0, 50.0))];
        (Region2 { outer: Loop2::polygon(&plate), holes: vec![] }, Region2 { outer: Loop2::polygon(&strip), holes: vec![] })
    }
    /// The bent piece of the plate's edge, a hair from the strip's edge, crosses it short of
    /// the corner they share: the corners a nanometre apart at most, the crossing up to a
    /// hundred times that before it at a slope of 1 in 100, a thousand at 1 in 1000. A split
    /// there left a sliver of edge shorter than the distance within which the loops join their
    /// pieces; the chaining took it for a second way out of the corner, and a loop of that one
    /// sliver is open, its area not zero away from the origin: a hole no outline held (no
    /// result at all) or a region of its own. The union is the plate and the strip, whole: six
    /// edges, the strip's end and the plate's on the line x = 150 one edge from y = 47 to 80.
    #[test]
    fn a_bent_edge_crossing_its_neighbour_by_their_corner_leaves_no_sliver() {
        let union_is_whole = |(plate, strip): (Region2, Region2), what: &str| {
            let want = plate.area() + strip.area();
            for (a, b) in [(&plate, &strip), (&strip, &plate)] {
                let out = planar_regions_boolean(std::slice::from_ref(a), std::slice::from_ref(b), BoolOp::Union, 1e-7)
                    .unwrap_or_else(|e| panic!("{what}: {e}"));
                assert_eq!(out.len(), 1, "{what}: {out:?}");
                let lp = &out[0].outer;
                let shortest = lp.segs.iter().map(Seg2::length).fold(f64::MAX, f64::min);
                assert!(out[0].holes.is_empty() && lp.segs.len() == 6 && shortest > 2.9, "{what}: {} edges, shortest {shortest:e}", lp.segs.len());
                assert!((out[0].area() - want).abs() < 1e-6, "{what}: {} vs {want}", out[0].area());
            }
        };
        for k in 0..360 {
            for slope in [0.01, 0.001] {
                union_is_whole(plate_and_strip_by_a_bent_edge((k as f64 * 0.25).to_radians(), slope, Vec2::ZERO), &format!("{k} slope {slope}"));
            }
        }
        // Corners a few nanometres apart, any way round: the sliver is then longer than the
        // tolerance (1e-7 with the strip 1e-9 into the plate) but shorter than the joining
        // distance (1e-6).
        for k in 0..16 {
            for d in [1e-9, 4e-9] {
                let off = Vec2::from_angle(k as f64 * std::f64::consts::TAU / 16.0) * d;
                union_is_whole(plate_and_strip_by_a_bent_edge(0.0, 0.01, off), &format!("off {off:?}"));
            }
        }
    }
    #[test]
    fn planar_region_api_rejects_hostile_values_and_budgets() {
        let plate = rectangle(Vec2::ZERO, Vec2::new(10.0, 10.0));
        for tolerance in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            assert!(planar_regions_boolean(std::slice::from_ref(&plate), &[], BoolOp::Union, tolerance).is_err());
        }
        let mut bad = plate.clone();
        bad.outer.segs[0] = Seg2::Line { a: Vec2::new(f64::NAN, 0.0), b: Vec2::X };
        assert!(planar_regions_boolean(&[bad], &[], BoolOp::Union, 1e-7).is_err());
        let many = vec![plate; MAX_PLANAR_SEGMENTS / 4 + 1];
        assert!(planar_regions_boolean(&many, &[], BoolOp::Union, 1e-7).is_err());
    }
}
