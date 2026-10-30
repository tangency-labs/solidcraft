//! The general fill on bodies (#759): G1 and G2 to the faces beside the loop, loops that are not
//! height fields, N-sided loops and points the patch passes through. Every number is checked
//! against the analytic surface the loop was cut from, or against a quadrature of the fitted
//! surface itself, never against the mesher alone.

use solidcraft_geom::brep::{self as b, eval::CurveEval, eval::SurfaceEval};
use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};

use super::{PATCH_ANGLE_TOL_DEG, PatchReport, PatchSpec, face_tolerance, patch_edges_report};
use crate::body::Body;
use crate::bridge::Continuity;
use crate::{BoolOp, boolean, box_solid, copy_faces, cylinder, extrude, measure, patch_edges, stitch};

/// A body's faces as surfaces over their parameter boxes, with the (u, v) polygon of each face's
/// loops (its exact pcurves, sampled).
struct Faces {
    faces: Vec<(SurfaceEval, [[f64; 2]; 2], Vec<Vec<[f64; 2]>>)>,
}

impl Faces {
    fn of(body: &Body) -> Faces {
        let r = body.record();
        let faces = r
            .faces
            .iter()
            .map(|f| {
                let polys = f
                    .loops
                    .iter()
                    .filter_map(|l| match l {
                        b::Loop::Edges { edges } => Some(
                            edges
                                .iter()
                                .flat_map(|u| {
                                    let [t0, t1] = r.edges[u.edge as usize].interval;
                                    (0..64).map(move |k| {
                                        let x = k as f64 / 64.0;
                                        u.pcurve.eval(if u.forward { t0 + (t1 - t0) * x } else { t1 - (t1 - t0) * x }).unwrap()
                                    })
                                })
                                .collect(),
                        ),
                        b::Loop::Vertex { .. } => None,
                    })
                    .collect();
                (SurfaceEval::new(&f.surface).unwrap(), f.domain, polys)
            })
            .collect();
        Faces { faces }
    }

    /// The face, parameters and point of the fill nearest `p`.
    fn nearest(&self, p: Vec3) -> (usize, [f64; 2], Vec3) {
        let mut best: Option<(f64, usize, [f64; 2], Vec3)> = None;
        for (k, (s, dom, _)) in self.faces.iter().enumerate() {
            let [[u0, u1], [v0, v1]] = *dom;
            let Some(uv) = s.project(p, None, f64::INFINITY) else { continue };
            let uv = [uv[0].clamp(u0, u1), uv[1].clamp(v0, v1)];
            let q = s.point(uv).unwrap();
            if best.is_none_or(|(d, ..)| q.dist(p) < d) {
                best = Some((q.dist(p), k, uv, q));
            }
        }
        let (_, k, uv, q) = best.unwrap();
        (k, uv, q)
    }

    /// `∫∫ f(S, S_u × S_v) du dv` over every face's own region (inside its loops' (u, v)
    /// polygons), by the midpoint rule on an `n × n` grid over each region's box.
    fn quadrature(&self, n: usize, f: &dyn Fn(Vec3, Vec3) -> f64) -> f64 {
        let mut sum = 0.0;
        for (s, _, polys) in &self.faces {
            let all: Vec<[f64; 2]> = polys.iter().flatten().copied().collect();
            let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
            for q in &all {
                lo = [lo[0].min(q[0]), lo[1].min(q[1])];
                hi = [hi[0].max(q[0]), hi[1].max(q[1])];
            }
            let inside = |u: f64, v: f64| {
                let mut w = false;
                for poly in polys {
                    for k in 0..poly.len() {
                        let (a, c) = (poly[k], poly[(k + 1) % poly.len()]);
                        if (a[1] > v) != (c[1] > v) && u < a[0] + (v - a[1]) / (c[1] - a[1]) * (c[0] - a[0]) {
                            w = !w;
                        }
                    }
                }
                w
            };
            let (du, dv) = ((hi[0] - lo[0]) / n as f64, (hi[1] - lo[1]) / n as f64);
            for i in 0..n {
                for j in 0..n {
                    let (u, v) = (lo[0] + (i as f64 + 0.5) * du, lo[1] + (j as f64 + 0.5) * dv);
                    if inside(u, v) {
                        let (p, su, sv) = s.derivs(u, v).unwrap();
                        sum += f(p, su.cross(sv)) * du * dv;
                    }
                }
            }
        }
        sum
    }
}

/// Points along every free edge of the fill (its loop): `per` on each.
fn rim_points(body: &Body, per: usize) -> Vec<Vec3> {
    let r = body.record();
    let mut out = Vec::new();
    for e in r.edges.iter().filter(|e| e.uses.len() == 1) {
        let c = CurveEval::new(e.parameters()).unwrap();
        let [t0, t1] = e.interval;
        for k in 0..per {
            out.push(c.point(t0 + (t1 - t0) * (k as f64 + 0.5) / per as f64).unwrap());
        }
    }
    out
}

/// Along the loop, densely: the farthest a rim point is from the patch, the largest angle
/// (degrees) between the patch's normal and the neighbour's exact one (`normal(p)`, one sign for
/// the whole loop: a fold would show as 180°), and the largest difference between the patch's
/// normal curvature across the loop and `curvature(p)`, the neighbour's.
fn along_the_loop(fill: &Body, normal: &dyn Fn(Vec3) -> Vec3, curvature: &dyn Fn(Vec3) -> f64) -> (f64, f64, f64) {
    let faces = Faces::of(fill);
    let pts = rim_points(fill, 200);
    let found: Vec<(usize, [f64; 2], Vec3)> = pts.iter().map(|p| faces.nearest(*p)).collect();
    let patch_normal = |k: usize, uv: [f64; 2]| {
        let (_, su, sv) = faces.faces[k].0.derivs(uv[0], uv[1]).unwrap();
        su.cross(sv).normalized().unwrap()
    };
    // One sign for the whole loop: the one most rim points agree with.
    let agree: f64 = pts.iter().zip(&found).map(|(p, (k, uv, _))| patch_normal(*k, *uv).dot(normal(*p)).signum()).sum();
    let sign = agree.signum();
    let (mut gap, mut angle, mut kappa) = (0.0f64, 0.0f64, 0.0f64);
    for (i, (p, &(k, uv, q))) in pts.iter().zip(&found).enumerate() {
        gap = gap.max(q.dist(*p));
        let n = patch_normal(k, uv);
        let want = normal(*p) * sign;
        angle = angle.max(n.dot(want).clamp(-1.0, 1.0).acos().to_degrees());
        // Across the loop: the tangent-plane direction square to the rim.
        let next = pts.get(i + 1).or(pts.get(i.wrapping_sub(1))).copied().unwrap();
        let t = (next - *p).normalized().unwrap();
        let Some(d) = n.cross(t).normalized() else { continue };
        let s = &faces.faces[k].0;
        let (_, su, sv) = s.derivs(uv[0], uv[1]).unwrap();
        // d = a·su + b·sv.
        let (aa, ab, bb) = (su.dot(su), su.dot(sv), sv.dot(sv));
        let det = aa * bb - ab * ab;
        let (a, c) = ((bb * d.dot(su) - ab * d.dot(sv)) / det, (aa * d.dot(sv) - ab * d.dot(su)) / det);
        let (suu, suv, svv) = s.second(uv[0], uv[1]).unwrap();
        let second = suu * (a * a) + suv * (2.0 * a * c) + svv * (c * c);
        kappa = kappa.max((second.dot(want) - curvature(*p)).abs());
    }
    (gap, angle, kappa)
}

/// The fill is a sheet on the open body's own edges (its free edges lie on the body's edge curves;
/// a loop of five or more edges is filled by patches about a centre, its edges in halves), within
/// the fill's tolerance of them; its mesh has no folded triangle (each faces the way its corners'
/// normals do); and the mesher's area agrees with a quadrature of the surfaces over their own
/// regions. Returns the area.
fn check_face(name: &str, open: &Body, fill: &Body) -> f64 {
    assert!(fill.is_surface());
    assert!(fill.face_count() >= 1, "{name}: a face");
    let (own, made) = (open.record(), fill.record());
    // The same kind of curve on the same curve it is parameterised on, to rounding (SMLib makes a
    // conic's frame and NURBS form again on a round trip, the last bit of a coordinate apart).
    let close = |p: &[f64], q: &[f64]| p.len() == q.len() && p.iter().zip(q).all(|(x, y)| (x - y).abs() <= 1e-12 * (1.0 + x.abs().max(y.abs())));
    let same = |c: &b::Curve, d: &b::Curve| match (c, d) {
        (b::Curve::Spline { knots: k, points: p }, b::Curve::Spline { knots: l, points: q }) => {
            k == l && p.len() == q.len() && p.iter().zip(q).all(|(x, y)| close(x, y))
        }
        (b::Curve::RationalSpline { knots: k, homogeneous: p }, b::Curve::RationalSpline { knots: l, homogeneous: q }) => {
            k == l && p.len() == q.len() && p.iter().zip(q).all(|(x, y)| close(x, y))
        }
        _ => c == d,
    };
    let kind = |e: &b::Edge| std::mem::discriminant(&e.curve);
    for e in made.edges.iter().filter(|e| e.uses.len() == 1) {
        assert!(own.edges.iter().any(|o| kind(o) == kind(e) && same(o.parameters(), e.parameters())), "{name}: an edge that is not the body's");
    }
    let faces = Faces::of(fill);
    let tol = face_tolerance(open);
    let off = rim_points(fill, 64).iter().map(|p| faces.nearest(*p).2.dist(*p)).fold(0.0f64, f64::max);
    assert!(off <= tol * 1.01, "{name}: an edge is {off} off the face (tolerance {tol})");
    let m = fill.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
    assert!(!m.triangles.is_empty(), "{name}: the face meshes");
    for t in &m.triangles {
        let [a, b, c] = m.tri(t).unwrap();
        let v = (b - a).cross(c - a);
        let n: Vec3 = t.iter().map(|&i| m.normals[i as usize]).fold(Vec3::ZERO, |s, x| s + x);
        if v.len() > 1e-10 {
            assert!(v.dot(n) > 0.0, "{name}: a folded triangle");
        }
    }
    let area = measure(fill).unwrap().area;
    let quad = faces.quadrature(500, &|_, n| n.len());
    eprintln!("{name}: {} faces, edges within {off:e} (tolerance {tol:e}), mesh area {area:.4}, quadrature {quad:.4}", fill.face_count());
    assert!((area - quad).abs() < 3e-3 * quad, "{name}: mesh area {area} against the surface's {quad}");
    area
}

fn spec(g: Continuity) -> PatchSpec {
    PatchSpec { continuity: g, ..PatchSpec::default() }
}

fn report(name: &str, r: &PatchReport) {
    eprintln!(
        "{name}: fill reports gap {:e}, normals within {:.4}° ({:.4}° near clashing corners), curvature mismatch {:e}, points within {:e}",
        r.gap, r.normal_deviation_deg, r.normal_deviation_all_deg, r.jumps[2], r.point_gap
    );
}

/// `body` without its planar faces but those at height `z0` or `z1` (the ends of a cylinder):
/// a cut solid's walls (and ends) as a surface, the faces the cut made taken away.
fn curved_and_ends(body: &Body, ends: &[f64]) -> Body {
    let r = body.record();
    let drop: Vec<usize> = r
        .faces
        .iter()
        .enumerate()
        .filter(|(_, f)| match &f.surface {
            b::Surface::Plane { origin, u_axis, v_axis } => {
                let n = Vec3::from(*u_axis).cross(Vec3::from(*v_axis)).normalized().unwrap();
                !ends.iter().any(|z| (origin[2] - z).abs() < 1e-9 && n.z.abs() > 1.0 - 1e-12)
            }
            _ => false,
        })
        .map(|(i, _)| i)
        .collect();
    let open = crate::remove_faces(body, &drop).unwrap();
    assert!(open.is_surface());
    open
}

/// A point on each edge that bounds only one face of `body` (its free edges): the middle of
/// each, where the edge picker samples it.
fn free_edge_points(body: &Body) -> Vec<Vec3> {
    let r = body.record();
    let mut points = r
        .edges
        .iter()
        .filter(|e| e.uses.len() == 1)
        .map(|e| CurveEval::new(e.parameters()).unwrap().point(0.5 * (e.interval[0] + e.interval[1])).unwrap())
        .collect::<Vec<_>>();
    // In a fixed order.
    points.sort_by(|a, b| (a.z, a.y, a.x).partial_cmp(&(b.z, b.y, b.x)).unwrap());
    points
}

fn radial(p: Vec3) -> Vec3 {
    Vec3::new(p.x, p.y, 0.0).normalized().unwrap()
}

/// A cylinder about z (radius 10, 40 high) with a pentagonal pocket cut into its wall from the
/// +x side (the pentagon in the yz plane about z = 20, circumradius 6): the wall and both ends
/// copied out, and a point on each of the pocket rim's five edges.
fn pentagon_window() -> (Body, Vec<Vec3>) {
    let solid = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 40.0).unwrap();
    let corners: Vec<Vec2> = (0..5).map(|k| Vec2::from_angle(0.3 + k as f64 * std::f64::consts::TAU / 5.0) * 6.0 + Vec2::new(0.0, 20.0)).collect();
    let prism = extrude(&Plane::YZ, &[Region2 { outer: Loop2::polygon(&corners), holes: vec![] }], 0.0, 15.0).unwrap().pop().unwrap();
    let cut = boolean(&solid, &prism, BoolOp::Cut).unwrap().unwrap();
    let open = curved_and_ends(&cut, &[0.0, 40.0]);
    let rim = free_edge_points(&open);
    (open, rim)
}

#[test]
fn a_five_sided_window_fills_g1_and_through_a_point() {
    let (open, rim) = pentagon_window();
    let (fill, r) = patch_edges_report(&open, &rim, &spec(Continuity::G1)).unwrap();
    report("pentagon g1", &r);
    check_face("pentagon g1", &open, &fill);
    let (gap, angle, _) = along_the_loop(&fill, &radial, &|_| 0.0);
    eprintln!("pentagon g1: along the rim: within {gap:e}, normal within {angle:.5}° of the wall's");
    assert!(angle < 0.1, "{angle}°");
    // Through a point half a millimetre out from the wall in the middle (G0 and G1).
    let point = Vec3::new(10.5, 0.0, 20.0);
    for g in [Continuity::G0, Continuity::G1] {
        let name = format!("pentagon {} through a point", g.as_str());
        let spec = PatchSpec { continuity: g, points: vec![point], ..PatchSpec::default() };
        let (fill, r) = patch_edges_report(&open, &rim, &spec).unwrap();
        report(&name, &r);
        check_face(&name, &open, &fill);
        let off = Faces::of(&fill).nearest(point).2.dist(point);
        eprintln!("{name}: the point is {off:e} off the surface");
        assert!(off <= face_tolerance(&open), "{name}: {off}");
    }
}

#[test]
fn a_hexagonal_rim_fills_through_a_point_and_g1_to_walls_meeting_at_angles_holds_away_from_the_corners() {
    // A hexagonal prism's top cut by a sideways cylinder: six walls meeting at 120°.
    let corners: Vec<Vec2> = (0..6).map(|k| Vec2::from_angle(0.3 + k as f64 * std::f64::consts::TAU / 6.0) * 20.0).collect();
    let prism = extrude(&Plane::XY, &[Region2 { outer: Loop2::polygon(&corners), holes: vec![] }], 0.0, 20.0).unwrap().pop().unwrap();
    let tool = cylinder(Vec3::new(0.0, -60.0, 50.0), Vec3::Y, 40.0, 120.0).unwrap();
    let solid = boolean(&prism, &tool, BoolOp::Cut).unwrap().unwrap();
    let mut keep = vec![Vec3::ZERO];
    let mut rim = Vec::new();
    for k in 0..6 {
        let m = corners[k].lerp(corners[(k + 1) % 6], 0.5);
        keep.push(m.extend(5.0));
        rim.push(m.extend(50.0 - (1600.0 - m.x * m.x).sqrt()));
    }
    let open = copy_faces(&solid, &keep).unwrap();
    let plain = patch_edges(&open, &rim).unwrap();
    let sealed = |f: &Body| measure(&stitch(&[&open, f], 0.0).unwrap()).unwrap().volume;
    // Six sides, G0, through a point 3 mm above the middle of the plain fill: it passes through
    // the point and seals the cup into a closed solid holding more than the plain fill's.
    let point = Faces::of(&plain).nearest(Vec3::new(0.0, 0.0, 12.0)).2 + Vec3::Z * 3.0;
    let (lifted, r) = patch_edges_report(&open, &rim, &PatchSpec { points: vec![point], ..PatchSpec::default() }).unwrap();
    report("hexagon through a point", &r);
    check_face("hexagon through a point", &open, &lifted);
    let off = Faces::of(&lifted).nearest(point).2.dist(point);
    eprintln!("hexagon through a point: the point is {off:e} off the surface; sealed {:.2} against {:.2} plain", sealed(&lifted), sealed(&plain));
    assert!(off <= face_tolerance(&open) && sealed(&lifted) > sealed(&plain));
    // G1 to the walls: at a corner no surface is tangent to both walls (its tangent plane there
    // holds both rim edges), so G1 is asked only away from the corners. On walls standing square
    // to the patch the corner is pinched too sharply for the finest grid to hold the rim within
    // the fill's tolerance, and the fill says so.
    match patch_edges_report(&open, &rim, &spec(Continuity::G1)) {
        Ok((fill, r)) => {
            report("hexagon g1", &r);
            check_face("hexagon g1", &open, &fill);
            assert!(r.normal_deviation_deg <= PATCH_ANGLE_TOL_DEG);
            assert!(sealed(&fill) > sealed(&plain));
        }
        Err(e) => {
            eprintln!("hexagon g1: {e}");
            assert!(e.to_string().contains("not supported yet"), "{e}");
        }
    }
}

#[test]
fn per_edge_continuity_and_hostile_specs() {
    let (open, rim) = pentagon_window();
    // G1 on two edges only.
    let spec = PatchSpec { edges: vec![(rim[0], Continuity::G1), (rim[2], Continuity::G2)], ..PatchSpec::default() };
    let (fill, r) = patch_edges_report(&open, &rim, &spec).unwrap();
    report("pentagon, two edges g1/g2", &r);
    check_face("pentagon, two edges g1/g2", &open, &fill);
    let bad = |spec: PatchSpec| patch_edges_report(&open, &rim, &spec).is_err();
    let g1 = PatchSpec { continuity: Continuity::G1, ..PatchSpec::default() };
    assert!(bad(PatchSpec { edges: vec![(Vec3::new(0.0, 0.0, 40.0), Continuity::G1)], ..g1.clone() }), "an edge off the loop");
    assert!(bad(PatchSpec { edges: vec![(Vec3::new(f64::NAN, 0.0, 0.0), Continuity::G1)], ..g1.clone() }));
    assert!(bad(PatchSpec { points: vec![Vec3::new(f64::INFINITY, 0.0, 0.0)], ..g1.clone() }));
    assert!(bad(PatchSpec { points: vec![Vec3::new(10.0, 0.0, 20.0); 65], ..g1.clone() }));
    assert!(bad(PatchSpec { points: vec![Vec3::new(-10.0, 0.0, 20.0)], ..g1.clone() }), "a point off the far side");
    for crown in [f64::NAN, 3.5, -1.0] {
        assert!(bad(PatchSpec { crown, ..g1.clone() }), "crown {crown}");
    }
    assert!(bad(PatchSpec { crown: 0.1, points: vec![Vec3::new(10.2, 0.0, 20.0)], ..PatchSpec::default() }), "a crown with no g1 edge");
    // A crown fills the middle out from the wall (the side the wall's normals face) and leaves
    // the loop, and G1 across it, as they were.
    let (plain, _) = patch_edges_report(&open, &rim, &g1).unwrap();
    let (full, r) = patch_edges_report(&open, &rim, &PatchSpec { crown: 0.1, ..g1.clone() }).unwrap();
    report("pentagon g1 crown 0.1", &r);
    check_face("pentagon g1 crown 0.1", &open, &full);
    let (_, angle, _) = along_the_loop(&full, &radial, &|_| 0.0);
    let middle = |b: &Body| {
        let p = Faces::of(b).nearest(Vec3::new(10.0, 0.0, 20.0)).2;
        Vec2::new(p.x, p.y).len()
    };
    let (a, b) = (middle(&plain), middle(&full));
    eprintln!("pentagon g1: the middle {a:.4} from the axis plain, {b:.4} with crown 0.1; normal within {angle:.5}° of the wall's");
    assert!(b > a + 0.5 && angle < 0.1, "{a} {b} {angle}");
    // The edges of a closed solid lie between two faces: g1 has no one face to meet.
    let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
    let top = [Vec3::new(5.0, 0.0, 10.0), Vec3::new(10.0, 5.0, 10.0), Vec3::new(5.0, 10.0, 10.0), Vec3::new(0.0, 5.0, 10.0)];
    assert!(patch_edges_report(&b, &top, &PatchSpec { continuity: Continuity::G1, ..PatchSpec::default() }).is_err());
}

/// The seams between `fill` and the faces of `open` beside its edges, in the middle of each
/// edge (30 % to 70 %): per seam, the largest normal angle (degrees) and jumps of `H₂`, `H₃` and
/// `H₄` along the direction across the seam (`solidcraft_geom::linkage`), read off the two
/// faces' own surfaces: the normal curvature, its rate of change and that rate's.
fn seam_jumps(open: &Body, fill: &Body) -> Vec<[f64; 4]> {
    use solidcraft_geom::linkage::surface_jet;
    let (ro, rf) = (open.record(), fill.record());
    let normal = |s: &SurfaceEval, uv: [f64; 2]| {
        let (_, su, sv) = s.derivs(uv[0], uv[1]).unwrap();
        su.cross(sv).normalized().unwrap()
    };
    let mut out = Vec::new();
    for e in &rf.edges {
        let curve = CurveEval::new(e.parameters()).unwrap();
        let [t0, t1] = e.interval;
        let on_fill = SurfaceEval::new(rf.faces[e.uses[0].face as usize].parameters()).unwrap();
        // The face of `open` whose own free edge this is.
        let mid = curve.point(0.5 * (t0 + t1)).unwrap();
        let near = |x: &b::Edge| {
            let c = CurveEval::new(x.parameters()).unwrap();
            (0..=32).map(|k| c.point(x.interval[0] + (x.interval[1] - x.interval[0]) * k as f64 / 32.0).unwrap().dist(mid)).fold(f64::MAX, f64::min)
        };
        let beside = ro.edges.iter().filter(|x| x.uses.len() == 1).min_by(|a, b| near(a).total_cmp(&near(b))).unwrap();
        let on_open = SurfaceEval::new(ro.faces[beside.uses[0].face as usize].parameters()).unwrap();
        let mut worst = [0.0f64; 4];
        for i in 0..=20 {
            let (p, d) = curve.derivs(t0 + (t1 - t0) * (0.3 + 0.4 * i as f64 / 20.0)).unwrap();
            let (ua, ub) = (on_fill.project(p, None, f64::INFINITY).unwrap(), on_open.project(p, None, f64::INFINITY).unwrap());
            let na = normal(&on_fill, ua);
            let nb = normal(&on_open, ub) * normal(&on_open, ub).dot(na).signum();
            let w = na.cross(d.normalized().unwrap()).normalized().unwrap();
            let (ja, jb) = (surface_jet(&on_fill, ua, na, w, 4).unwrap(), surface_jet(&on_open, ub, nb, w, 4).unwrap());
            worst[0] = worst[0].max(na.dot(nb).clamp(-1.0, 1.0).acos().to_degrees());
            for k in 2..=4 {
                worst[k - 1] = worst[k - 1].max((ja.form(k, [0.0, 1.0]).unwrap() - jb.form(k, [0.0, 1.0]).unwrap()).abs());
            }
        }
        out.push(worst);
    }
    out
}

/// The Continuity Analysis (`inspect.continuity`) of `fill` sewn into `open`, on the seams at
/// `hole`: per seam, its order over the whole edge, its order over the stations from 30 % to 70 %
/// (the corners left out), and the worst G3 jump there (1/mm²).
fn continuity_analysis(open: &Body, fill: &Body, hole: &[Vec3]) -> Vec<(Option<Continuity>, Option<Continuity>, f64)> {
    use crate::{ContinuityTolerances, EdgePick, pick_edges, seam_diagnostics};
    let sewn = stitch(&[open, fill], 1e-3).unwrap();
    let picks: Vec<EdgePick> = hole.iter().map(|p| EdgePick::Point(*p)).collect();
    let edges = pick_edges(&sewn, &picks).unwrap();
    let t = ContinuityTolerances::default();
    let limits = [t.gap_mm, t.angle_deg, t.curvature_per_mm, t.curvature_change_per_mm2, t.curvature_change_rate_per_mm3];
    let report = seam_diagnostics(&sewn, &edges, 33, t).unwrap();
    report
        .seams
        .iter()
        .map(|seam| {
            let inside: Vec<_> = seam.stations.iter().filter(|s| (0.3..=0.7).contains(&s.edge_fraction)).collect();
            let worst = |f: fn(&crate::SeamStation) -> f64| inside.iter().map(|s| f(s)).fold(0.0f64, f64::max);
            let metrics = [
                worst(|s| s.gap_mm),
                worst(|s| s.normal_angle_deg),
                worst(|s| s.curvature_jump_per_mm),
                worst(|s| s.curvature_change_jump_per_mm2),
                worst(|s| s.curvature_change_rate_jump_per_mm3),
            ];
            let met = metrics.iter().zip(limits).take_while(|(w, l)| **w <= *l).count();
            (seam.order, met.checked_sub(1).and_then(Continuity::from_order), metrics[3])
        })
        .collect()
}

/// A box's corner where a round of 8 up the vertical edge meets the rounds of 4 along the top
/// (one blend face rolls the top round over the vertical one); that face taken off, and the hole
/// patched G2, G3 and G4 to the rounds and the top around it. Read off the faces themselves in
/// the middle of each seam: all meet the tangent planes and the curvature; the G3 patch also meets
/// the curvature's rate of change, which the G2 patch leaves many times larger. The Continuity
/// Analysis reads each seam's middle at the order asked, and the whole seam G1: at the corners the
/// rounds' curvatures differ, so no patch can meet both. G3 to an edge between two faces is
/// refused.
#[test]
fn a_corner_of_three_rounds_patched_g3_holds_the_curvature_change_a_g2_patch_does_not() {
    let solid = box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 30.0)).unwrap();
    let up = crate::fillet(&solid, &[Vec3::new(40.0, 40.0, 15.0)], 8.0).unwrap();
    // The top edges and the arc between them over the vertical round: one tangent chain.
    let arc = Vec3::new(32.0, 32.0, 30.0) + Vec3::new(1.0, 1.0, 0.0) * (8.0 / 2f64.sqrt());
    let rounded = crate::fillet(&up, &[Vec3::new(40.0, 20.0, 30.0), arc, Vec3::new(20.0, 40.0, 30.0)], 4.0).unwrap();
    // The blend face at the corner: the one face neither plane nor cylinder there.
    let corner = Vec3::new(37.79, 36.155, 28.43);
    let faces = Faces::of(&rounded);
    let (k, _, _) = faces.nearest(corner);
    let open = crate::remove_faces(&rounded, &[k]).unwrap();
    assert!(open.is_surface());
    let hole = [Vec3::new(34.828, 34.828, 30.0), Vec3::new(37.657, 37.657, 26.0), Vec3::new(38.828, 32.0, 28.828), Vec3::new(32.0, 38.828, 28.828)];
    let mut read = Vec::new();
    for g in [Continuity::G2, Continuity::G3, Continuity::G4] {
        let name = format!("three-round corner {}", g.as_str());
        let (fill, r) = patch_edges_report(&open, &hole, &spec(g)).unwrap();
        report(&name, &r);
        let seams = seam_jumps(&open, &fill);
        eprintln!("{name}: jumps of the fill's report {:?}; read off the faces, per seam [angle°, H2, H3, H4] {seams:?}", r.jumps);
        let analysed = continuity_analysis(&open, &fill, &hole);
        eprintln!("{name}: Continuity Analysis, per seam [order over the edge, order 30-70 %, worst G3 jump 30-70 % /mm²] {analysed:?}");
        assert_eq!(analysed.len(), 4, "{name}: four seams analysed");
        for (whole, middle, _) in &analysed {
            assert_eq!(*middle, Some(g), "{name}: the Continuity Analysis reads the order asked in the middle of each seam");
            assert_eq!(*whole, Some(Continuity::G1), "{name}: the corners, where the rounds' curvatures differ, hold the tangent plane only");
        }
        assert_eq!(seams.len(), 4, "{name}: four seams");
        for s in &seams {
            assert!(s[0] < PATCH_ANGLE_TOL_DEG, "{name}: the tangent planes meet: {s:?}");
            assert!(s[1] < 1e-3, "{name}: the curvature meets: {s:?}");
        }
        read.push(seams);
    }
    for (g2, g3) in read[0].iter().zip(&read[1]) {
        assert!(g3[2] < 1e-2, "G3 meets the curvature change: {g3:?}");
        assert!(g2[2] > 20.0 * g3[2].max(1e-6), "G2 leaves the curvature change: {g2:?} vs G3 {g3:?}");
    }
    // A closed box's top edges lie between two faces: there is no one face to meet G3.
    let top = [Vec3::new(5.0, 0.0, 10.0), Vec3::new(10.0, 5.0, 10.0), Vec3::new(5.0, 10.0, 10.0), Vec3::new(0.0, 5.0, 10.0)];
    let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
    assert!(patch_edges_report(&b, &top, &spec(Continuity::G3)).is_err());
}
