//! tangency-labs/solidcore#2907: a tangent chain of seven edges chamfered on a curved face,
//! the rim of SolidCraft's public `examples/regressions/front_triangle_hull_triangle.json`
//! (the body before its chamfer, `data/issue_2907c_hull_rim.smb`). The band check took a rail
//! point far across the face for one inside another blend's band, whose chord direction it
//! only projected on: refused as overlapping at any size past the smallest. The edges are 15
//! to 24 long and the bands 1 to 3 wide, and they do not meet. Unequal chamfers on a block's
//! U groove are the same false overlap, with the volume in closed form.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::query::api_queries::{edge_closest_point, surface_closest_point};
use solidcore::surface::surface::Surface;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::loopuse;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, Brep, EdgeId, FaceId, VertexId};

const BODY: &[u8] = include_bytes!("data/issue_2907c_hull_rim.smb");
/// The chain's edges in the body's edge list.
const CHAIN: [usize; 7] = [9, 10, 8, 0, 1, 2, 3];
/// The smallest of the offsets a 1.528 chamfer has at its edges' middles.
const OFFSET: f64 = 2.8673496400813243;

fn kernel() -> Kernel {
    let mut fixes = UpstreamFixes::all();
    for held in [
        "fillet_refuse_rounds_breaking_through",
        "fillet_round_ends_on_outline",
    ] {
        *fixes.flag_mut(held).unwrap() = false;
    }
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = fixes;
    k
}

fn body(k: &mut Kernel) -> Brep {
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &loopuse::notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(BODY.to_vec()), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// One offset on the chain builds: a clean closed solid with the seven chamfers' faces. Its faces
/// curve, so its gain has no closed form. The gain is the flux over the faces that differ, taken
/// about a point of the chain (`direct_gain`): 80.12092 at accuracy 1e-11, 80.12081 at 1e-9.
/// The body is about 5.1e6 and some 500 from the world origin, and a whole-body volume about the
/// box's middle misses its asked 1e-11 by 1.5e-9 relative there (tangency-labs/solidcraft#14170),
/// so the gain is not a difference of two volumes. It is checked against an independent estimate,
/// not a closed form: the chamfer's sections integrated along the chain from the faces and the
/// rails alone, the chain's ends sectioned with their end faces (`integrated_gain`, 79.82 +-
/// 0.53 here, the stations near the ends with no section the main term of that error). The
/// groove below has a closed form.
#[test]
fn a_chamfered_rim_on_a_curved_face_builds() {
    let mut k = kernel();
    let mut b = body(&mut k);
    let faces = b.face_list().len();
    let edges: Vec<_> = CHAIN.iter().map(|&i| b.edge_list()[i]).collect();
    let chain = chain_before(&b, &edges);
    let old_faces = b.face_list().to_vec();
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_per_edge_constant_radius_with_second(
        &mut k,
        &edges,
        &[OFFSET; 7],
        &[OFFSET; 7],
        &[FilletSurfaceGeneratorType::Linear],
        1,
        1.0,
    )
    .unwrap();
    s.create_corners(&k, None).unwrap();
    for id in s.fillet_corners().unwrap() {
        s.set_corner_bevel(id, true).unwrap();
    }
    s.do_filleting(&mut k, None).unwrap();
    drop(s);
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    assert!(is_manifold_solid(&b).unwrap());
    assert_eq!(b.face_list().len(), faces + 7);
    // the gain, by the flux over the faces that differ about a point of the chain, at two
    // accuracies (the step control's estimate is no bound: the two must agree)
    let mut before = body(&mut kernel());
    let (gained, _) = direct_gain(&k, &mut before, &mut b, &chain, &old_faces, 1e-11);
    let (coarser, _) = direct_gain(&k, &mut before, &mut b, &chain, &old_faces, 1e-9);
    let (estimate, error) = integrated_gain(&k, &b, &chain, &old_faces);
    assert!(
        (gained - coarser).abs() <= 1e-3,
        "{gained} vs {coarser} at 1e-9"
    );
    assert!(
        (gained - estimate).abs() <= error,
        "{gained} vs {estimate} +- {error}"
    );
    let report = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(report.is_valid(), "{report:?}");
}

/// The groove's half width.
const R: f64 = 5.0;
/// The length of the groove's straight legs, from the rims down: the first span of a clamped
/// uniform cubic ends at `P1 / 4 + 7 P2 / 12 + P3 / 6`, y = -23 / 6.
const LEG: f64 = 23.0 / 6.0;
/// The block's half width.
const W: f64 = 10.0;
/// The block's height.
const H: f64 = 10.0;
/// The block's length, along z.
const L: f64 = 20.0;

/// The line in z = 0 from `a` to `b`.
fn line(a: [f64; 2], b: [f64; 2]) -> Curve {
    let at = |p: [f64; 2]| Point3::new(p[0], p[1], 0.0);
    Curve::from(create_line_segment(at(a), at(b)).unwrap())
}

/// A 20 x 10 block, 20 long in z, with a U groove in its top face y = 0: one B-spline face
/// whose legs at x = -5 and x = 5 run straight down from the rims before its bottom turns.
fn grooved(k: &mut Kernel) -> Brep {
    // degree 3, uniform (C2 at its knots, one face): the first and last spans' four control
    // points are collinear, so the legs are straight from the rims down to y = -23 / 6
    let pts = [
        Point3::new(-R, 0.0, 0.0),
        Point3::new(-R, -2.0, 0.0),
        Point3::new(-R, -4.0, 0.0),
        Point3::new(-R, -6.0, 0.0),
        Point3::new(R, -6.0, 0.0),
        Point3::new(R, -4.0, 0.0),
        Point3::new(R, -2.0, 0.0),
        Point3::new(R, 0.0, 0.0),
    ];
    let u = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 3,
        control_points: &pts,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[4, 1, 1, 1, 1, 4],
        knots: &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
    .unwrap();
    let profile = [
        line([-W, 0.0], [-R, 0.0]),
        Curve::from(u),
        line([R, 0.0], [W, 0.0]),
        line([W, 0.0], [W, -H]),
        line([W, -H], [-W, -H]),
        line([-W, -H], [-W, 0.0]),
    ];
    let refs: Vec<Option<&Curve>> = profile.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), L, true).unwrap()
}

fn nearest(k: &Kernel, b: &Brep, p: Point3) -> EdgeId {
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        let d = edge_closest_point(k, b, e, p).unwrap().closest.distance;
        if d < best.0 {
            best = (d, Some(e));
        }
    }
    best.1.unwrap()
}

/// The groove's rims chamfered 0.5 and 1. On the U face the 0.5 chamfer's rail lies across the
/// groove, 0.5 below the rim, level with the middle of the 1 chamfer's band along its chord:
/// the check took it for a rail inside that band, though it is 10 from the edge. Each rim is a
/// square edge between the top and a straight leg, so each chamfer is the prism of the right
/// triangle of its offset `r` (`r^2 / 2`) along the block, its ends square:
/// `L (0.5^2 + 1^2) / 2` removed.
#[test]
fn a_groove_chamfered_unequally_at_its_rims_removes_its_closed_form() {
    let (small, large) = (0.5, 1.0);
    assert!(large < LEG, "the rails are on the straight legs");
    let mut k = kernel();
    let mut b = grooved(&mut k);
    let before = volume(&k, &mut b);
    let rims = [
        nearest(&k, &b, Point3::new(-R, 0.0, 0.5 * L)),
        nearest(&k, &b, Point3::new(R, 0.0, 0.5 * L)),
    ];
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_per_edge_constant_radius_with_second(
        &mut k,
        &rims,
        &[small, large],
        &[small, large],
        &[FilletSurfaceGeneratorType::Linear],
        1,
        1.0,
    )
    .unwrap();
    s.create_corners(&k, None).unwrap();
    s.do_filleting(&mut k, None).unwrap();
    drop(s);
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    assert!(is_manifold_solid(&b).unwrap());
    let removed = before - volume(&k, &mut b);
    let want = 0.5 * L * (small * small + large * large);
    assert!((removed - want).abs() <= 1e-9 * want, "{removed} vs {want}");
    let report = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(report.is_valid(), "{report:?}");
}

/// A chain edge before the chamfer: its curve over its interval, its two faces' surfaces, its
/// end vertices, and at an end of the chain (a vertex of no other chain edge) the end face
/// there with its two edges from the vertex, one along each of the edge's faces.
struct ChainEdge {
    curve: Curve,
    ivl: [f64; 2],
    faces: [FaceId; 2],
    surfaces: [Surface; 2],
    ends: [VertexId; 2],
    chain_end: [Option<ChainEnd>; 2],
}

/// An end of the chain: the end face's surface, and its edges from the vertex along the chain
/// edge's two faces (`[0]` beside `faces[0]`, `[1]` beside `faces[1]`), each a curve over its
/// interval.
struct ChainEnd {
    surface: Surface,
    sides: [(Curve, [f64; 2]); 2],
}

fn chain_before(b: &Brep, edges: &[EdgeId]) -> Vec<ChainEdge> {
    let ends_of = |e: EdgeId| {
        [
            edge::start_vertex(b, e).unwrap().unwrap(),
            edge::end_vertex(b, e).unwrap().unwrap(),
        ]
    };
    let surface = |f: FaceId| b.face(f).unwrap().surface.clone().unwrap();
    edges
        .iter()
        .map(|&e| {
            let curve = edge::curve(b, e).unwrap().unwrap().clone();
            let ivl = edge::interval(b, e).unwrap();
            let faces = edge::faces(b, e, None).unwrap();
            let faces = [faces[0], faces[1]];
            let ends = ends_of(e);
            let chain_end = ends.map(|v| {
                if edges.iter().any(|&o| o != e && ends_of(o).contains(&v)) {
                    return None;
                }
                // the vertex's other edges: one along each of the edge's faces, both on the
                // end face
                let mut sides: [Option<(Curve, [f64; 2])>; 2] = [None, None];
                let mut end_face = None;
                for &o in b.edge_list() {
                    if o == e || !ends_of(o).contains(&v) {
                        continue;
                    }
                    let of = edge::faces(b, o, None).unwrap();
                    let Some(side) = (0..2).find(|&s| of.contains(&faces[s])) else {
                        continue;
                    };
                    end_face = of.iter().copied().find(|f| !faces.contains(f));
                    let ivl = edge::interval(b, o).unwrap();
                    sides[side] = Some((
                        edge::curve(b, o).unwrap().unwrap().clone(),
                        [ivl.min(), ivl.max()],
                    ));
                }
                let [Some(a), Some(c)] = sides else {
                    panic!("an end of the chain without its two end edges");
                };
                Some(ChainEnd {
                    surface: surface(end_face.unwrap()),
                    sides: [a, c],
                })
            });
            ChainEdge {
                curve,
                ivl: [ivl.min(), ivl.max()],
                faces,
                surfaces: [surface(faces[0]), surface(faces[1])],
                ends,
                chain_end,
            }
        })
        .collect()
}

/// The rails' curves: the edges of the faces the chamfer made (those not in `old`), with their
/// intervals and tolerances.
fn new_edge_curves(b: &Brep, old: &[FaceId]) -> Vec<(Curve, [f64; 2], f64)> {
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for &f in b.face_list() {
        if old.contains(&f) {
            continue;
        }
        for e in solidcore::topology::face::p4::edges(b, f, None, false, None).unwrap() {
            if seen.contains(&e) {
                continue;
            }
            seen.push(e);
            let ivl = edge::interval(b, e).unwrap();
            out.push((
                edge::curve(b, e).unwrap().unwrap().clone(),
                [ivl.min(), ivl.max()],
                edge::tolerance(b, e).unwrap(),
            ));
        }
    }
    out
}

/// Where `curve` crosses the plane through `o` square to `t`, within `reach` of `o`.
fn crossings(
    k: &Kernel,
    curve: &Curve,
    ivl: [f64; 2],
    o: Point3,
    t: Vec3,
    reach: f64,
) -> Vec<Point3> {
    let g = |u: f64| (curve.evaluate_point(k, u).unwrap() - o).dot(t);
    let n = 64;
    let mut out = Vec::new();
    for i in 0..n {
        let (mut a, mut c) = (
            ivl[0] + (ivl[1] - ivl[0]) * f64::from(i) / f64::from(n),
            ivl[0] + (ivl[1] - ivl[0]) * f64::from(i + 1) / f64::from(n),
        );
        let (mut ga, gc) = (g(a), g(c));
        if ga == 0.0 || ga.signum() == gc.signum() {
            continue;
        }
        for _ in 0..60 {
            let m = 0.5 * (a + c);
            let gm = g(m);
            if gm.signum() == ga.signum() {
                (a, ga) = (m, gm);
            } else {
                c = m;
            }
        }
        let p = curve.evaluate_point(k, 0.5 * (a + c)).unwrap();
        if p.distance(o) < reach {
            out.push(p);
        }
    }
    out
}

/// The point of `s` nearest `p`.
fn onto(k: &Kernel, s: &Surface, p: Point3) -> (Point3, f64) {
    let c = surface_closest_point(k, s, p).unwrap().closest;
    (c.point, c.distance)
}

/// `x` unless it is at `o` itself (a crossing at the edge's own point is its vertex, no rail).
fn off_point(x: Option<(Point3, &Surface)>, o: Point3) -> Option<(Point3, &Surface)> {
    x.filter(|(p, _)| p.distance(o) > 1e-9 * OFFSET)
}

/// The area and centroid of a section in the plane through `o` square to `t`, bounded by the
/// closed path `path`: from each point to the next, along the surface given (its curve in the
/// plane, as `m` points between them dropped onto it and back into the plane), or straight.
fn polygon(
    k: &Kernel,
    o: Point3,
    t: Vec3,
    path: &[(Point3, Option<&Surface>)],
    m: u32,
) -> (f64, Point3) {
    let flat = |p: Point3| p - t * (p - o).dot(t);
    let mut poly = Vec::new();
    for (i, &(p, s)) in path.iter().enumerate() {
        let q = path[(i + 1) % path.len()].0;
        poly.push(p);
        if let Some(s) = s {
            for j in 1..m {
                let x = p + (q - p) * (f64::from(j) / f64::from(m));
                poly.push(flat(onto(k, s, x).0));
            }
        }
    }
    let mut area = Vec3::new(0.0, 0.0, 0.0);
    let mut moment = Vec3::new(0.0, 0.0, 0.0);
    for i in 0..poly.len() {
        let (a, c) = (poly[i] - o, poly[(i + 1) % poly.len()] - o);
        let tri = a.cross(c) * 0.5;
        area += tri;
        moment += (a + c) * (tri.dot(t) / 3.0);
    }
    let signed = area.dot(t);
    if signed == 0.0 {
        return (0.0, o);
    }
    (signed.abs(), o + moment * (1.0 / signed))
}

/// The gain integrated section by section along the chain, independently of the volume
/// integration: each section in the plane square to its edge (weighted by Pappus's factor for
/// the edge's bend), bounded by the faces before (points dropped onto them) and the rails the
/// chamfer made, and at an end of the chain by the end face too. There the end face cuts the
/// chamfer at a slant: a rail runs onto it before the edge ends, the section then closes along
/// the end face to the chamfer's edge on it, and past the edge's end (in planes parallel to the
/// one there) the chamfer runs on between the end face and the other face. The trapezoid rule
/// in the stations and the chords on each face are extrapolated once each (Richardson, both
/// second order); the error is a third of each extrapolation's step. Returns the estimate and
/// its error.
fn integrated_gain(k: &Kernel, b: &Brep, chain: &[ChainEdge], old: &[FaceId]) -> (f64, f64) {
    let rails = new_edge_curves(b, old);
    let reach = 4.0 * OFFSET;
    // the curves that come within reach of each chain edge (the rest never cross its planes
    // near it)
    let near: Vec<Vec<usize>> = chain
        .iter()
        .map(|c| {
            let pts: Vec<Point3> = (0..=16)
                .map(|i| {
                    let u = c.ivl[0] + (c.ivl[1] - c.ivl[0]) * f64::from(i) / 16.0;
                    c.curve.evaluate_point(k, u).unwrap()
                })
                .collect();
            let span = pts[0].distance(pts[16]) / 16.0 + 2.0 * reach;
            (0..rails.len())
                .filter(|&r| {
                    let (curve, ivl, _) = &rails[r];
                    (0..=32).any(|i| {
                        let u = ivl[0] + (ivl[1] - ivl[0]) * f64::from(i) / 32.0;
                        let q = curve.evaluate_point(k, u).unwrap();
                        pts.iter().any(|p| p.distance(q) < span)
                    })
                })
                .collect()
        })
        .collect();
    // the faces a rail of edge `i` may lie on, per side: its own, and on the side whose face
    // changes at a vertex of the chain, the neighbour's there (a section near the vertex runs
    // past the side edge between them onto it)
    let sides = |i: usize| -> [Vec<&Surface>; 2] {
        let c = &chain[i];
        let mut out = [vec![&c.surfaces[0]], vec![&c.surfaces[1]]];
        for (j, o) in chain.iter().enumerate() {
            if j == i || !o.ends.iter().any(|v| c.ends.contains(v)) {
                continue;
            }
            for (side, allowed) in out.iter_mut().enumerate() {
                let shared = c.faces[1 - side];
                if let Some(k2) = (0..2).find(|&q| o.faces[q] == shared) {
                    allowed.push(&o.surfaces[1 - k2]);
                }
            }
        }
        out
    };
    // the rails' and the chamfer's edges' crossings with the plane through `o` square to `t`,
    // nearest `o`: on each side's faces, and (`end`) on the end face alone
    let hits = |o: Point3, t: Vec3, i: usize, end: Option<&ChainEnd>| {
        let allowed = sides(i);
        let mut f: [Option<(Point3, &Surface)>; 2] = [None, None];
        let mut on_end: Option<Point3> = None;
        for &r in &near[i] {
            let (curve, ivl, tol) = &rails[r];
            for p in crossings(k, curve, *ivl, o, t, reach) {
                // on the end face: the chamfer's edge there, or (on a side face too) the side
                // face's edge with the end face, which is no rail
                let on_end_face = end.is_some_and(|end| onto(k, &end.surface, p).1 <= 2.0 * tol);
                let mut on_side = false;
                for (slot, candidates) in f.iter_mut().zip(&allowed) {
                    for &s in candidates {
                        if onto(k, s, p).1 <= 2.0 * tol {
                            on_side = true;
                            if !on_end_face
                                && slot.is_none_or(|(q, _)| p.distance(o) < q.distance(o))
                            {
                                *slot = Some((p, s));
                            }
                        }
                    }
                }
                if on_end_face && !on_side && on_end.is_none_or(|q| p.distance(o) < q.distance(o)) {
                    on_end = Some(p);
                }
            }
        }
        (f, on_end)
    };
    // where the end edge along side `side` crosses the plane, nearest `o`
    let end_side = |end: &ChainEnd, side: usize, o: Point3, t: Vec3| -> Option<Point3> {
        let (curve, ivl) = &end.sides[side];
        crossings(k, curve, *ivl, o, t, reach)
            .into_iter()
            .min_by(|p, q| p.distance(o).total_cmp(&q.distance(o)))
    };
    // the section of edge `i` in the plane square to it through its point `e` (or, past an
    // end of the chain, through `o` with no point of the edge): its area and centroid
    let section_at = |i: usize,
                      o: Point3,
                      t: Vec3,
                      on_edge: bool,
                      end: Option<&ChainEnd>,
                      m: u32|
     -> Option<(f64, Point3)> {
        let ([fa, fb], c_end) = hits(o, t, i, end);
        // a rail crossing at the edge itself is the vertex, not a rail
        let (fa, fb) = if on_edge {
            (off_point(fa, o), off_point(fb, o))
        } else {
            (fa, fb)
        };
        let c = &chain[i];
        let (sa, sb) = (&c.surfaces[0], &c.surfaces[1]);
        let mut path: Vec<(Point3, Option<&Surface>)> = Vec::new();
        if on_edge {
            path.push((o, Some(fa.map_or(sa, |x| x.1))));
        }
        match (fa, fb, end) {
            (Some((pa, _)), Some((pb, s)), _) => {
                path.push((pa, None));
                path.push((pb, Some(s)));
            }
            (None, Some((pb, s)), Some(end)) => {
                if on_edge {
                    path.push((end_side(end, 0, o, t)?, Some(&end.surface)));
                } else {
                    path.push((end_side(end, 1, o, t)?, Some(&end.surface)));
                }
                path.push((c_end?, None));
                path.push((pb, Some(s)));
            }
            (Some((pa, _)), None, Some(end)) => {
                if on_edge {
                    path.push((pa, None));
                    path.push((c_end?, Some(&end.surface)));
                    path.push((end_side(end, 1, o, t)?, Some(sb)));
                } else {
                    path.push((end_side(end, 0, o, t)?, Some(sa)));
                    path.push((pa, None));
                    path.push((c_end?, Some(&end.surface)));
                }
            }
            _ => return None,
        }
        Some(polygon(k, o, t, &path, m))
    };
    let run = |n: u32, m: u32| -> (f64, f64) {
        let (mut v, mut gap) = (0.0, 0.0);
        for (ci, c) in chain.iter().enumerate() {
            let h = (c.ivl[1] - c.ivl[0]) / f64::from(n);
            let mut g: Vec<Option<f64>> = Vec::new();
            for i in 0..=n {
                let u = c.ivl[0] + h * f64::from(i);
                let d = c.curve.evaluate(k, u, 2, true, false).unwrap();
                let (e, d1, d2) = (d[0], d[1], d[2]);
                let speed = d1.length();
                let t = d1 * (1.0 / speed);
                let kn = (d2 - t * d2.dot(t)) * (1.0 / (speed * speed));
                // the chain's end nearer this station, if any
                let end = if 2 * i < n {
                    &c.chain_end[0]
                } else {
                    &c.chain_end[1]
                };
                g.push(
                    section_at(ci, e, t, true, end.as_ref(), m)
                        .map(|(a, centroid)| a * (1.0 - kn.dot(centroid - e)) * speed),
                );
            }
            for i in 0..usize::try_from(n).unwrap() {
                let (Some(x), Some(y)) = (g[i], g[i + 1]) else {
                    // a station with no section: the larger neighbour's over its width, all of
                    // it error
                    gap += h * g[i].or(g[i + 1]).unwrap_or(0.0);
                    continue;
                };
                v += 0.5 * h * (x + y);
            }
            // past an end of the chain, in planes parallel to the one at the edge's end, as
            // far as the rails run
            for (endi, end) in c.chain_end.iter().enumerate() {
                let Some(end) = end else { continue };
                let u = c.ivl[endi];
                let d = c.curve.evaluate(k, u, 1, true, false).unwrap();
                let outward = if endi == 0 { -1.0 } else { 1.0 };
                let t = d[1] * (outward / d[1].length());
                let mut depth: f64 = 0.0;
                for &r in &near[ci] {
                    let (curve, ivl, _) = &rails[r];
                    for x in ivl {
                        let p = curve.evaluate_point(k, *x).unwrap();
                        if p.distance(d[0]) < reach {
                            depth = depth.max((p - d[0]).dot(t));
                        }
                    }
                }
                let np = (n / 4).max(4);
                let hp = depth / f64::from(np);
                let mut prev: Option<f64> = None;
                for j in 0..=np {
                    let o = d[0] + t * (hp * f64::from(j));
                    let a = if j == np {
                        Some(0.0)
                    } else {
                        section_at(ci, o, t, false, Some(end), m).map(|(a, _)| a)
                    };
                    if let (Some(x), Some(y)) = (prev, a) {
                        v += 0.5 * hp * (x + y);
                    } else if j > 0 {
                        gap += hp * prev.or(a).unwrap_or(0.0);
                    }
                    prev = a;
                }
            }
        }
        (v, gap)
    };
    let (s_n, gap_n) = run(64, 16);
    let (s_2n, gap_2n) = run(128, 16);
    let (s_2n_2m, gap_2n_2m) = run(128, 32);
    let (dn, dm) = (s_2n - s_n, s_2n_2m - s_2n);
    let estimate = s_2n_2m + dn / 3.0 + dm / 3.0;
    let error = dn.abs() / 3.0 + dm.abs() / 3.0 + gap_n.max(gap_2n).max(gap_2n_2m);
    (estimate, error)
}

/// The volume flux `(1/3) int (x - origin).n dA` of face `f` of the closed solid `b`, over its
/// faceuse on the material side (outward), as the kernel's precise face properties integrate it.
fn flux(k: &Kernel, b: &mut Brep, f: FaceId, origin: Point3, accuracy: f64, estimated: f64) -> f64 {
    use solidcore::mass::integrator::properties::{face_properties, region_material};
    use solidcore::topology::edgeuse::p1::faceuse_region;
    let up = b.face(f).unwrap().faceuse.unwrap();
    let mate = b.faceuse(up).unwrap().mate.unwrap();
    let fu = [up, mate]
        .into_iter()
        .find(|&fu| region_material(b, faceuse_region(b, fu).unwrap().unwrap()).unwrap())
        .unwrap();
    let orientation = b.faceuse(fu).unwrap().orientation;
    face_properties(k, b, f, orientation, accuracy, origin, estimated, 0.0, 0x3f)
        .unwrap()
        .volume
}

/// The chamfer's gain by the divergence theorem over the faces that differ only: the flux of
/// the faces the chamfer made, plus each old face beside them after less before, about one
/// origin on the chain (every other face is the same surface over the same trims, and cancels).
/// Returns the gain and its parts (new faces, old faces' change).
fn direct_gain(
    k: &Kernel,
    before: &mut Brep,
    after: &mut Brep,
    chain: &[ChainEdge],
    old: &[FaceId],
    accuracy: f64,
) -> (f64, [f64; 2]) {
    let mut origin = Vec3::new(0.0, 0.0, 0.0);
    for c in chain {
        let m = c
            .curve
            .evaluate_point(k, 0.5 * (c.ivl[0] + c.ivl[1]))
            .unwrap();
        origin += Vec3::new(m.x, m.y, m.z) * (1.0 / f64::from(u32::try_from(chain.len()).unwrap()));
    }
    let origin = Point3::new(origin.x, origin.y, origin.z);
    // the error scale the kernel's brep volume gives every face: its box's largest side
    // squared over 100, the before body's for both, so a face the same in both integrates the
    // same
    let size = solidcore::query::brep::calculate_bounding_box(
        k,
        &mut *before,
        solidcore::query::brep::BoundingBoxOptions::default(),
    )
    .unwrap()
    .size();
    let largest = size.x.max(size.y).max(size.z);
    let estimated = largest * largest / 100.0;
    let new: Vec<FaceId> = after
        .face_list()
        .iter()
        .copied()
        .filter(|f| !old.contains(f))
        .collect();
    // the old faces sharing an edge with a new one
    let mut beside: Vec<FaceId> = Vec::new();
    for &f in &new {
        for e in solidcore::topology::face::p4::edges(after, f, None, false, None).unwrap() {
            for g in edge::faces(after, e, None).unwrap() {
                if old.contains(&g) && !beside.contains(&g) {
                    beside.push(g);
                }
            }
        }
    }
    // the old faces the chamfer replaced (no longer in the body after)
    let gone: Vec<FaceId> = old
        .iter()
        .copied()
        .filter(|f| !after.face_list().contains(f))
        .collect();
    let new_part: f64 = new
        .iter()
        .map(|&f| flux(k, after, f, origin, accuracy, estimated))
        .sum::<f64>()
        - gone
            .iter()
            .map(|&f| flux(k, before, f, origin, accuracy, estimated))
            .sum::<f64>();
    let old_part: f64 = beside
        .iter()
        .map(|&f| {
            flux(k, after, f, origin, accuracy, estimated)
                - flux(k, before, f, origin, accuracy, estimated)
        })
        .sum();
    (new_part + old_part, [new_part, old_part])
}
