//! tangency-labs/solidcore#2360 (from smlib#82): the Mixed corner, a convex and a concave
//! blend meeting G1 across a tangent seam (WR-697).
//!
//! The body is #2256's tangent peg (`data/issue_2256_peg_step_reimported.smb`: a bar
//! 40 x 16 x 10 whose far end is a half disc R8 about (40, 8), joined with a disc R8 about
//! the same centre from z = 10 to 20, written to STEP and read back). Its front top edge
//! (convex) and the concave arc where the bar's top meets the cylinder leave the vertex
//! V = (40, 0, 10) along one tangent; the third edge there, x = 40 on the front face, is a
//! straight tangent seam. Rounded together at r, their rails on the top cross at
//! X = (40 - sqrt((8 + r)^2 - (8 - r)^2), r, 10) = (40 - sqrt(32 r), r, 10).
//!
//! Graded on geometry (the measures of smlib#82's capi test `mixed_rounds_together`,
//! re-implemented on solidcore calls):
//! - the body stays inside its unblended box (the planes y = 0, y = 16, z = 0, x = 0, the
//!   cylinder's top z = 20, x = 48), to the blends' tolerance 4e-5;
//! - `check_brep` passes; 11 faces (the peg's 7, the two blends, the corner's two);
//! - X is a vertex where three or more faces meet;
//! - G1 at every edge's stations within the corner's box B, to 1e-9 rad, on the faces'
//!   surfaces' normals; and inside every face across each interior knot line;
//! - the volume of the body less the boxes B and B' (meeting on y = 8, the plane through
//!   the cylinder's axis, where every cut of the concave blend is exact), against its
//!   closed form, to 1e-11 relative.
//!
//! Pinned SMLib's corner here stands 6.544e-3 out of y = 0; a 3x2 mixed-convexity corner
//! that is not the Mixed one is refused by WR-710.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::nurbs::b_spline_surface::p1::SurfParamType;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{brep_bounding_box, edge_closest_point};
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::surface::surface::p4::NoObject;
use solidcore::surface::surface::Surface;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, Brep, EdgeId};
use solidcore::EntityRef;
use std::f64::consts::PI;

fn key_of(e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
        EntityRef::Brep(x) => (16011, 0u64, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(((kind + 1) << 32) | u64::from(i)),
    }
}

/// Every opt-in fix on (SolidCraft's set, WR-710's refusal among them).
fn kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = UpstreamFixes::all();
    k
}

fn read(k: &mut Kernel) -> Brep {
    let data = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/issue_2256_peg_step_reimported.smb"
    ))
    .unwrap();
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key_of,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(data), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

fn volume(k: &Kernel, b: &mut Brep, acc: f64) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, acc, &attrs).unwrap()
}

fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

/// The front top edge and the concave arc rounded (`xsect` 1) or chamfered (0) at `r`.
fn blended(r: f64, xsect: u32) -> (Kernel, Brep, solidcore::Result<()>) {
    blended_kept(true, r, xsect)
}

/// [`blended`], with `definition` (`fillet_round_as_definition`, #2488) set.
fn blended_kept(definition: bool, r: f64, xsect: u32) -> (Kernel, Brep, solidcore::Result<()>) {
    let mut k = kernel();
    k.fixes.fillet_round_as_definition = definition;
    let mut b = read(&mut k);
    let es: Vec<EdgeId> = [[20.0, 0.0, 10.0], [32.0, 8.0, 10.0]]
        .iter()
        .map(|p| pick(&k, &b, *p))
        .collect();
    let res = fillet_edges(&mut k, &mut b, &es, r, xsect, 1, 1.0);
    b.refresh_cached_topology();
    (k, b, res)
}
fn bspline_net(s: &Surface) -> Option<&BSplineSurface> {
    match s.kind() {
        SurfaceKind::BSpline(b) => Some(b),
        SurfaceKind::Plane(p) => Some(p.base()),
        SurfaceKind::SurfOfRevolution(r) => Some(r.base()),
        SurfaceKind::SurfOfExtrusion(r) => Some(r.base()),
        SurfaceKind::Cone(c) => Some(c.rev().base()),
        SurfaceKind::Sphere(s) => Some(s.base().base()),
        SurfaceKind::Torus(t) => Some(t.base().base()),
        _ => None,
    }
}

fn angle(a: Vec3, b: Vec3) -> f64 {
    a.cross(b).length().atan2(a.dot(b))
}

/// The largest angle between the faces' surface normals (turned as their face uses)
/// either side of any edge, at 31 stations inside each edge within the corner's box B of
/// blends of size `s`; and the number of edges with a station there.
fn worst_seam(k: &Kernel, b: &mut Brep, s: f64) -> (f64, usize) {
    let (xlo, y1, z0, z1) = (31.0 - s, (s + 1.0).min(8.0), 9.0 - s, 11.0 + s);
    let mut worst: f64 = 0.0;
    let mut seams = 0;
    for e in b.edge_list().to_vec() {
        let Some(curve) = edge::curve(b, e).unwrap().cloned() else {
            continue;
        };
        if edge::start_vertex(b, e).unwrap() == edge::end_vertex(b, e).unwrap() {
            continue;
        }
        let ivl = edge::interval(b, e).unwrap();
        let prim = solidcore::topology::edgeuse::p1::primary_edgeuse(b, e)
            .unwrap()
            .unwrap();
        let rad = solidcore::topology::edgeuse::p1::radial(b, prim)
            .unwrap()
            .unwrap();
        assert_ne!(prim, rad, "edge with one face");
        let eus = [prim, rad];
        let mut counted = false;
        for st in 1..32 {
            let t = ivl.evaluate(f64::from(st) / 32.0);
            let m = curve.evaluate_point(k, t).unwrap();
            if m.x < xlo || m.y > y1 || m.z < z0 || m.z > z1 {
                continue;
            }
            if !counted {
                counted = true;
                seams += 1;
            }
            let mut ns = Vec::new();
            for &eu in &eus {
                let bi =
                    evaluate_binormal(k, b, eu, t, false, &BinormalOptions::default()).unwrap();
                let f = solidcore::topology::edgeuse::p1::face(b, eu)
                    .unwrap()
                    .unwrap();
                let srf = b.face(f).unwrap().surface.clone().unwrap();
                let dom = srf.natural_uv_domain_with_nurb(k).unwrap();
                let sols = srf
                    .global_point_solve_point_test_off::<NoObject>(
                        k,
                        None,
                        &dom,
                        SolverOperation::Minimize,
                        m,
                        1e-9,
                        None,
                        SolutionRequested::Single,
                    )
                    .unwrap();
                let sol = sols.get(0).unwrap();
                let uv = Point2::new(sol.start.param(0).unwrap(), sol.start.param(1).unwrap());
                let mut n = srf.evaluate_normal(k, uv, Side::Left, Side::Left).unwrap();
                if n.dot(bi.face_normal) < 0.0 {
                    n = -n;
                }
                ns.push(n);
            }
            worst = worst.max(angle(ns[0], ns[1]));
        }
    }
    (worst, seams)
}

/// The largest crease inside any face: across each interior knot line of a B-spline face
/// (within its uv box), the normals 1e-11 of the span either side, at 12 p + 1 stations
/// per span along the line; and the number of lines.
fn worst_crease(k: &Kernel, b: &Brep) -> (f64, usize) {
    let mut worst: f64 = 0.0;
    let mut lines = 0;
    for f in solidcore::topology::brep::p1::faces(b, None).unwrap() {
        let srf = b.face(f).unwrap().surface.clone().unwrap();
        let Some(net) = bspline_net(&srf) else {
            continue;
        };
        let bx = solidcore::topology::face::p2::uv_domain(b, f).unwrap();
        let dom = srf.natural_uv_domain_with_nurb(k).unwrap();
        for dir in 0..2 {
            let (across, along) = if dir == 0 {
                (SurfParamType::U, SurfParamType::V)
            } else {
                (SurfParamType::V, SurfParamType::U)
            };
            let knots = net.knots(across, None).unwrap().0;
            let along_knots = net.knots(along, None).unwrap().0;
            let (lo, hi, alo, ahi, span) = if dir == 0 {
                (
                    bx.min().x,
                    bx.max().x,
                    bx.min().y,
                    bx.max().y,
                    dom.max().x - dom.min().x,
                )
            } else {
                (
                    bx.min().y,
                    bx.max().y,
                    bx.min().x,
                    bx.max().x,
                    dom.max().y - dom.min().y,
                )
            };
            let d = 1e-11 * span;
            let per = 12 * net.degree(along) + 1;
            for &t in knots.iter().skip(1).take(knots.len().saturating_sub(2)) {
                if t <= lo + d || t >= hi - d {
                    continue;
                }
                lines += 1;
                for w2 in along_knots.windows(2) {
                    let (a0, a1) = (w2[0].max(alo), w2[1].min(ahi));
                    for j in 0..per {
                        if a1 <= a0 {
                            break;
                        }
                        let w = a0 + (a1 - a0) * (j as f64 + 0.5) / per as f64;
                        let (m0, m1) = if dir == 0 {
                            (Point2::new(t - d, w), Point2::new(t + d, w))
                        } else {
                            (Point2::new(w, t - d), Point2::new(w, t + d))
                        };
                        let (Ok(n0), Ok(n1)) = (
                            srf.evaluate_normal(k, m0, Side::Left, Side::Left),
                            srf.evaluate_normal(k, m1, Side::Left, Side::Left),
                        ) else {
                            continue;
                        };
                        worst = worst.max(angle(n0, n1));
                    }
                }
            }
        }
    }
    (worst, lines)
}

/// The volume outside B and B' (blends of size `s`, B' B mirrored about y = 8, both
/// reaching y = 8) and its closed form (the round's section `(1 - pi/4) s^2`, the
/// chamfer's `s^2 / 2`).
fn outside_volume(k: &mut Kernel, b: &Brep, s: f64, chamfer: bool) -> (f64, f64) {
    let (x0, y1, z0, z1) = (31.0 - s, 8.0, 9.0 - s, 11.0 + s);
    let copy = solidcore::topology::brep::p3::brep_copy(k, b).unwrap();
    let cut = create_box(k, Point3::new(x0, -1.0, z0), 60.0, y1 + 1.0, z1 - z0).unwrap();
    let cut2 = create_box(k, Point3::new(x0, 16.0 - y1, z0), 60.0, y1 + 1.0, z1 - z0).unwrap();
    let mid = boolean_difference(k, copy, cut).unwrap();
    let mut out = boolean_difference(k, mid, cut2).unwrap();
    let got = volume(k, &mut out, 1e-12);
    let half = 32.0 * PI;
    let in_box = (40.0 - x0) * 8.0 * (10.0 - z0) + 0.5 * half * (10.0 - z0) + half * (z1 - 10.0);
    let section = if chamfer {
        0.5 * s * s
    } else {
        (1.0 - PI / 4.0) * s * s
    };
    let want = 160.0 * 40.0 + 960.0 * PI - 2.0 * in_box - section * (40.0 - (40.0 - x0));
    (got, want)
}

/// The grades of the blended peg (`order` G1 for rounds, G0 for chamfers).
fn graded(r: f64, chamfer: bool) {
    let (mut k, mut b, res) = blended(r, u32::from(!chamfer));
    res.unwrap();
    let bb = brep_bounding_box(&k, &b, true).unwrap();
    let (lo, hi) = ([0.0, 0.0, 0.0], [48.0, 16.0, 20.0]);
    for i in 0..3 {
        assert!(
            bb.min_at(i) >= lo[i] - 4e-5,
            "box min {i}: {}",
            bb.min_at(i)
        );
        assert!(
            bb.max_at(i) <= hi[i] + 4e-5,
            "box max {i}: {}",
            bb.max_at(i)
        );
    }
    eprintln!("r {r} chamfer {chamfer}: box min y {:e}", bb.min_at(1));
    let faces = solidcore::topology::brep::p1::faces(&b, None)
        .unwrap()
        .len();
    assert_eq!(faces, 11, "faces");
    // X, where the rails cross on the top
    let x = Point3::new(
        40.0 - ((8.0 + r) * (8.0 + r) - (8.0 - r) * (8.0 - r)).sqrt(),
        r,
        10.0,
    );
    let mut best = f64::INFINITY;
    for &v in b.vertex_list() {
        best = best.min(b.vertex(v).unwrap().point.distance(x));
    }
    eprintln!("  nearest vertex {best:e} from X");
    assert!(best <= 5e-11, "X {best:e}");
    if !chamfer {
        let (seam, seams) = worst_seam(&k, &mut b, r);
        let (crease, lines) = worst_crease(&k, &b);
        eprintln!(
            "  {seams} seams, worst {seam:e} rad; {lines} knot lines, worst crease {crease:e}"
        );
        assert!(seams > 0 && seam <= 1e-9, "seam {seam:e}");
        assert!(crease <= 1e-9, "crease {crease:e}");
    }
    let (got, want) = outside_volume(&mut k, &b, r, chamfer);
    eprintln!(
        "  outside volume {got:.13} want {want:.13} rel {:e}",
        (got - want) / want
    );
    assert!(
        ((got - want) / want).abs() <= 1e-11,
        "outside volume {got} want {want}"
    );
    assert!(check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .is_valid());
}

/// r 2: the Mixed corner builds the peg inside its box, G1 within the corner, to the
/// closed form's volume.
#[test]
fn rounds_together_r2() {
    graded(2.0, false);
}

/// r 5: the trace runs on to the seam's line before it is extended (the through point at
/// the edge's end, from smlib#82's trace rule).
#[test]
fn rounds_together_r5() {
    graded(5.0, false);
}

/// Chamfers d 2: the corner's faces are the bilinear Coons patches (G0).
#[test]
fn chamfers_together_d2() {
    graded(2.0, true);
}

// ---- #2488: a Mixed corner's faces keep their rule ----

use solidcore::surface::procedural_blend::convert::{corner_rebuild, corner_to_record};
use solidcore::surface::procedural_blend::corner::CornerKind;
use solidcore::surface::procedural_blend::record::{CornerRecord, SupportRecord};
use solidcore::surface::procedural_blend::BlendNet;

/// The faces' surfaces whose definition is a corner's of `kind`.
fn corner_faces(b: &Brep, kind: CornerKind) -> Vec<Surface> {
    b.faces()
        .ids()
        .filter_map(|f| b.face(f).unwrap().surface.clone())
        .filter(|s| {
            s.procedural_blend()
                .and_then(|d| d.corner())
                .is_some_and(|c| c.kind() == kind)
        })
        .collect()
}

/// Every face's surface over a 9 x 9 grid of its natural domain, as bits; a round that is
/// a procedural blend by its net (its own definition is #2480's, and evaluates by solving
/// it).
fn face_bits(k: &Kernel, b: &Brep) -> Vec<Vec<[u64; 3]>> {
    b.faces()
        .ids()
        .filter_map(|f| b.face(f).unwrap().surface.clone())
        .map(|s| {
            let s = match s.kind() {
                SurfaceKind::ProceduralBlend(pb) => Surface::from(pb.net().copy()),
                _ => s.clone(),
            };
            let d = s.natural_uv_domain_with_nurb(k).unwrap();
            let mut pts = Vec::new();
            for i in 0..=8 {
                for j in 0..=8 {
                    let u = d.min().x + (d.max().x - d.min().x) * f64::from(i) / 8.0;
                    let v = d.min().y + (d.max().y - d.min().y) * f64::from(j) / 8.0;
                    let p = s.evaluate_point(k, Point2::new(u, v)).unwrap();
                    pts.push([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]);
                }
            }
            pts
        })
        .collect()
}

/// Stored points `points` (weighted by `weights` when rational) moved by `by` along x, y
/// and z.
fn shift(points: &mut [[f64; 3]], weights: Option<&Vec<f64>>, by: f64) {
    for (i, p) in points.iter_mut().enumerate() {
        let w = weights.map_or(1.0, |w| w[i]);
        for x in p.iter_mut() {
            *x += by * w;
        }
    }
}

/// `rec` moved by `by` along x, y and z: a NURBS form, or a blend whole (its supports, its
/// curves and its net).
fn moved(rec: &SupportRecord, by: f64) -> SupportRecord {
    let mut rec = rec.clone();
    match &mut rec {
        SupportRecord::Nurbs { form, .. } => {
            let w = form.weights.clone();
            shift(&mut form.points, w.as_ref(), by);
        }
        SupportRecord::Blend(b) => {
            let w = b.net.weights.clone();
            shift(&mut b.net.points, w.as_ref(), by);
            let d = &mut b.definition;
            for c in std::iter::once(&mut d.leader).chain(d.seeds.iter_mut()) {
                let w = c.weights.clone();
                shift(&mut c.points, w.as_ref(), by);
            }
            for support in &mut d.supports {
                *support = moved(support, by);
            }
        }
    }
    rec
}

#[test]
fn the_mixed_faces_are_their_rules() {
    for (xsect, order) in [(1, 1), (0, 0)] {
        let (k, mut b, res) = blended_kept(true, 2.0, xsect);
        res.unwrap();
        let faces = corner_faces(&b, CornerKind::Mixed);
        assert_eq!(faces.len(), 2);
        for s in &faces {
            let d = s.procedural_blend().unwrap();
            assert!(matches!(d.net(), BlendNet::Approx(_)));
            assert!(d.bound() > 0.0 && d.bound() < 1e-3, "{}", d.bound());
            assert_eq!(d.order(), order);
            assert!((0..3).all(|i| d.support(i).is_some()));
            // where built, the net is its rule's patch
            assert_eq!(d.recertify(&k).unwrap(), 0.0);
        }
        let (k0, mut b0, res0) = blended_kept(false, 2.0, xsect);
        res0.unwrap();
        assert!(corner_faces(&b0, CornerKind::Mixed).is_empty());
        let (v, v0) = (volume(&k, &mut b, 1e-9), volume(&k0, &mut b0, 1e-9));
        if xsect == 1 {
            // the rounds are the same either way: the body bit for bit
            assert_eq!(v.to_bits(), v0.to_bits());
            assert_eq!(face_bits(&k, &b), face_bits(&k0, &b0));
        } else {
            // a chamfer is its definition with the flag on (#2486), its net certified
            // within its bound of the chord, so the corner is built on other nets: the body
            // moves within that bound (3.2e-8 of it here)
            assert!((v - v0).abs() <= 1e-7 * v0, "{v} vs {v0}");
        }
    }
}

#[test]
fn a_mixed_face_round_trips_through_its_record() {
    let (k, b, res) = blended_kept(true, 2.0, 1);
    res.unwrap();
    for s in corner_faces(&b, CornerKind::Mixed) {
        let rec = corner_to_record(&k, &s).unwrap();
        let json = serde_json::to_string(&rec).unwrap();
        let back = corner_rebuild(&k, &serde_json::from_str(&json).unwrap()).unwrap();
        assert_eq!(corner_to_record(&k, &back).unwrap(), rec);
        assert_eq!(back.procedural_blend().unwrap().recertify(&k).unwrap(), 0.0);
    }
}

#[test]
fn a_moved_mixed_face_is_certified_again() {
    let (k, b, res) = blended_kept(true, 2.0, 1);
    res.unwrap();
    for s in corner_faces(&b, CornerKind::Mixed) {
        let mut s = s.copy(&k).unwrap();
        let mut place = solidcore::geom::axis2_placement::Axis2Placement::default();
        place.rotate_about_axis(0.3, Vec3::new(1.0, 2.0, 3.0).unitize().unwrap());
        place.translate(Vec3::new(1e2, -2e2, 5e1));
        s.transform(&k, &place, None).unwrap();
        let d = s.procedural_blend().unwrap();
        let proved = d.recertify(&k).unwrap();
        eprintln!("moved: {proved:e}");
        assert!(proved <= d.bound());
    }
}

#[test]
fn a_mixed_face_off_its_blend_or_t_is_refused() {
    let (k, b, res) = blended_kept(true, 2.0, 1);
    res.unwrap();
    for s in corner_faces(&b, CornerKind::Mixed) {
        let CornerRecord::Mixed(rec) = corner_to_record(&k, &s).unwrap() else {
            panic!("not a mixed record");
        };
        // its trimline's blend moved
        let mut off = rec.clone();
        let i = rec.blend as usize;
        off.blends[i] = moved(&rec.blends[i], 1e-3);
        let why = corner_rebuild(&k, &CornerRecord::Mixed(off))
            .unwrap_err()
            .to_string();
        assert!(why.contains("off its blend"), "{why}");
        // T moved
        let mut off = rec.clone();
        off.common = moved(&rec.common, 1e-3);
        let why = corner_rebuild(&k, &CornerRecord::Mixed(off))
            .unwrap_err()
            .to_string();
        assert!(why.contains("off T"), "{why}");
        // the net moved off its rule
        let mut off = rec;
        for p in &mut off.net.points {
            p[2] += 1e-3;
        }
        let why = corner_rebuild(&k, &CornerRecord::Mixed(off))
            .unwrap_err()
            .to_string();
        assert!(why.contains("rule's patch"), "{why}");
    }
}
