//! tangency-labs/solidcore#2343 (smlib#85 part C, built in solidcore; the fork's work in
//! progress at tangency-labs/smlib 37c9e1d is the reference, not the target): three unequal
//! rounds at a vertex where a face is a cylinder, cone or sphere, convex or concave, end on a
//! Setback (`Kernel::fixes.fillet_setback_beside_curved_faces`, WR-697), its fill certified no more concave than what each patch is G1 to (the owner's
//! ruling (a) on #85, `II >= k G`).
//!
//! The cases are the fork's capi `setback` cases `curved_d_prism_corner`,
//! `curved_knife_edge_corner`, `curved_d_pocket_corner` and `curved_sphere_corner`
//! (`capi/tests/ops_kernel.cpp` at 37c9e1d), built from the same curves and solids. Each is
//! graded against the faces' closed forms, not against a net: the result sound
//! (`check_brep` level 2) with the face count of a Setback (the part's faces, its rounds and
//! 2 n patches); the 2 n patches one Bezier span each; every seam G1 to 1e-9 rad (against a
//! curved face's exact normal); 2 n patch edges (the spring curves' halves) on the faces to
//! 1e-12 of the part's size; every patch's least principal curvature at least k (0 beside
//! convex faces and rounds, -1/21 beside the knife part's concave wall) less 1e-9; and every
//! patch on the material's side of every face, by the face's closed-form distance, to 1e-12
//! of the part's size; and every patch's control net never turning back (`check_brep`'s test
//! #5, part of the certificate since #2243). Before this change each corner was the old N-sided corner's patch
//! (`NByNConvex`), not a Setback, and built: without its own flag it still is, so a design
//! made before rebuilds the same.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::blend_corner::BlendCornerEvent;
use solidcore::blend::blend_refusal::BlendRefusal;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::error::SolidError;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::kernel::UpstreamFixes;
use solidcore::math::{Point2, Point3, Vec3, BIG_DOUBLE};
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_sphere};
use solidcore::query::api_queries::{brep_bounding_box, get_edges, get_faces};
use solidcore::surface::procedural_blend::convert::{corner_rebuild, corner_to_record};
use solidcore::surface::procedural_blend::corner::CornerKind;
use solidcore::surface::procedural_blend::record::{CornerRecord, QuadricKindRecord};
use solidcore::surface::procedural_blend::BlendRef;
use solidcore::surface::surface::assert_valid::grid_has_non_monotonic_points;
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::surface::surface::p4::NoObject;
use solidcore::surface::surface::Surface;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p4 as face4;
use solidcore::topology::{edge, EdgeId, FaceId, VertexId};
use solidcore::{Brep, Kernel};
use std::f64::consts::PI;

/// Every fix but `geometric_g2_continuity`.
fn all_fixes() -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
}

fn pick(k: &Kernel, b: &Brep, at: [f64; 3]) -> EdgeId {
    let p = Point3::new(at[0], at[1], at[2]);
    let mut best: Option<(EdgeId, f64)> = None;
    for e in get_edges(b).unwrap() {
        let Some(c) = edge::curve(b, e).unwrap() else {
            continue;
        };
        let ivl = edge::interval(b, e).unwrap();
        let d = c
            .drop_point(
                k,
                &ivl,
                p,
                None,
                BIG_DOUBLE,
                None,
                SolverOperation::Minimize,
            )
            .unwrap();
        if d.success && d.distance < 0.1 && best.is_none_or(|(_, bd)| d.distance < bd) {
            best = Some((e, d.distance));
        }
    }
    best.unwrap().0
}

/// The edges of `b` every station of which (9 inside, both ends) satisfies `on`.
fn edges_on(k: &Kernel, b: &Brep, on: impl Fn(Point3) -> bool) -> Vec<EdgeId> {
    let mut out = Vec::new();
    for e in get_edges(b).unwrap() {
        let Some(c) = edge::curve(b, e).unwrap() else {
            continue;
        };
        let ivl = edge::interval(b, e).unwrap();
        if (0..=10).all(|i| {
            on(c.evaluate_point(k, ivl.evaluate(f64::from(i) / 10.0))
                .unwrap())
        }) {
            out.push(e);
        }
    }
    out
}

fn line(a: Point3, b: Point3) -> Curve {
    Curve::from(create_line_segment(a, b).unwrap())
}

fn arc(k: &Kernel, c: Point3, r: f64, a0: f64, a1: f64) -> Curve {
    let axis = Vec3::Z;
    let reference = Vec3::X;
    let y = axis.cross(reference);
    let x0 = reference * a0.cos() + y * a0.sin();
    let y0 = axis.cross(x0);
    let frame = Axis2Placement::new(c, x0, y0);
    let interval = Extent1d::from_min_max(0.0, (a1 - a0) * 180.0 / PI);
    Curve::from(Ellipse::create_canonical_circle(k, &frame, r, Some(&interval), None).unwrap())
}

/// The disc of radius `rr` about (`cx`, `cy`) less x > cx + a, from z0 up `height`.
fn d_prism(k: &mut Kernel, cx: f64, cy: f64, rr: f64, a: f64, z0: f64, height: f64) -> Brep {
    let al = (a / rr).acos();
    let h = (rr * rr - a * a).sqrt();
    let l = line(
        Point3::new(cx + a, cy - h, z0),
        Point3::new(cx + a, cy + h, z0),
    );
    let c = arc(k, Point3::new(cx, cy, z0), rr, al, 2.0 * PI - al);
    create_linear_sweep(k, &[Some(&l), Some(&c)], Vec3::Z, height, true).unwrap()
}

/// SolidCraft issue 851's part (the fork's `knife_edge_arc_part`): a 41 1/6 x 21 plate and
/// the corner a circle of radius 21 about (62 1/6, -21) leaves, 10 high.
fn knife_edge_arc_part(k: &mut Kernel) -> Brep {
    let tip = 62.0 + 1.0 / 6.0;
    let wall = 21.0;
    let x0 = tip - wall;
    let disc = create_cylinder(k, Point3::new(tip, -wall, 0.0), wall, 10.0).unwrap();
    let plate = create_box(k, Point3::new(0.0, -wall, 0.0), x0, wall, 10.0).unwrap();
    let corner = create_box(k, Point3::new(x0, -wall, 0.0), wall, wall, 10.0).unwrap();
    let corner = boolean_difference(k, corner, disc).unwrap();
    boolean_union(k, plate, corner).unwrap()
}

/// What blending the corner did: the event the corner at `v` took, and the session's status
/// and noted reason.
struct Rounded {
    event: Option<BlendCornerEvent>,
    ok: bool,
    reason: String,
    /// The setback the corner names as building, where it refused one too short.
    fit: f64,
    /// The corner's setbacks (empty where it found none).
    setbacks: Vec<f64>,
    /// The room on each of the corner's edges.
    room: Vec<f64>,
    /// The refusal as data, where the session returned one ([`SolidError::BlendRefused`]).
    refusal: Option<BlendRefusal>,
}

/// The vertex of `b` at `v`.
fn vertex_at(b: &Brep, v: Point3) -> VertexId {
    for e in get_edges(b).unwrap() {
        for x in [
            edge::start_vertex(b, e).unwrap(),
            edge::end_vertex(b, e).unwrap(),
        ]
        .into_iter()
        .flatten()
        {
            if b.vertex(x).unwrap().point.distance(v) < 1e-9 {
                return x;
            }
        }
    }
    panic!("no vertex at {v:?}");
}

/// Rounds `edges` at `radii` together; `v` the corner vertex to report.
fn round_each(k: &mut Kernel, b: &mut Brep, edges: &[EdgeId], radii: &[f64], v: Point3) -> Rounded {
    round_asked(k, b, edges, radii, v, None)
}

/// [`round_each`], the corner at `v` asked to be set back `asked` along each of its edges.
fn round_asked(
    k: &mut Kernel,
    b: &mut Brep,
    edges: &[EdgeId],
    radii: &[f64],
    v: Point3,
    asked: Option<f64>,
) -> Rounded {
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    let vid = vertex_at(b, v);
    let rounded = {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius(k, edges, radii, &xs, 1, 1.0)
            .unwrap();
        if let Some(sb) = asked {
            s.set_set_back_fill(vid, sb, 0.0, false);
        }
        s.create_corners(k, None).unwrap();
        let mut event = None;
        let mut this = None;
        for c in s.fillet_corners().unwrap() {
            let fc = s.corner(c).unwrap();
            let at = fc.vertex.map(|vv| s.target().vertex(vv).unwrap().point);
            if at.is_some_and(|p| p.distance(v) < 1e-9) {
                this = Some(c);
                event = Some(
                    fc.blend
                        .as_ref()
                        .map_or(BlendCornerEvent::None, |bc| bc.event),
                );
                eprintln!(
                    "corner at {v:?}: {:?}, {} edges",
                    fc.corner_type(),
                    fc.all_eus.as_ref().map_or(0, Vec::len)
                );
            }
        }
        let result = s.do_filleting(k, None);
        let (mut setbacks, mut room) = (Vec::new(), Vec::new());
        for c in s.fillet_corners().unwrap() {
            let fc = s.corner(c).unwrap();
            if let Some(sb) = fc.setback.as_ref() {
                eprintln!("setbacks {:?}", sb.setbacks);
                if this == Some(c) {
                    setbacks.clone_from(&sb.setbacks);
                    room.clone_from(&sb.room);
                }
            }
        }
        let info = s.error_info();
        let refusal = match &result {
            Err(SolidError::BlendRefused { refusal, .. }) => Some(*refusal),
            _ => None,
        };
        Rounded {
            event,
            ok: result.is_ok(),
            reason: format!("{} {} ({result:?})", info.code, info.comment),
            fit: s.fit_setback(),
            setbacks,
            room,
            refusal,
        }
    };
    if rounded.ok {
        stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    }
    rounded
}

/// A face at the corner by its closed form, signed so that `g < 0` in the material: a plane
/// (`axis` its unit normal out of the material), a cylinder, sphere or cone (`side` +1: the
/// material inside).
#[derive(Clone, Copy)]
enum Kind {
    Plane,
    Cylinder,
    Sphere,
    /// About `axis` through `at`, of radius `radius + slope h` at `h` along the axis.
    Cone,
}

#[derive(Clone, Copy)]
struct CurvedFace {
    kind: Kind,
    at: Point3,
    axis: Vec3,
    radius: f64,
    slope: f64,
    side: f64,
}

impl CurvedFace {
    fn plane(at: [f64; 3], normal: [f64; 3]) -> Self {
        Self {
            kind: Kind::Plane,
            at: Point3::new(at[0], at[1], at[2]),
            axis: Vec3::new(normal[0], normal[1], normal[2]),
            radius: 0.0,
            slope: 0.0,
            side: 1.0,
        }
    }
    /// A cylinder, sphere or cone (`kind`) of `side`.
    fn quadric(kind: Kind, at: [f64; 3], radius: f64, slope: f64, side: f64) -> Self {
        Self {
            kind,
            at: Point3::new(at[0], at[1], at[2]),
            axis: Vec3::Z,
            radius,
            slope,
            side,
        }
    }
    fn g(&self, x: Point3) -> f64 {
        let d = x - self.at;
        match self.kind {
            Kind::Plane => d.dot(self.axis),
            Kind::Cylinder => {
                self.side * ((d - self.axis * d.dot(self.axis)).length() - self.radius)
            }
            Kind::Sphere => self.side * (d.length() - self.radius),
            Kind::Cone => {
                let h = d.dot(self.axis);
                let rho = (d - self.axis * h).length();
                self.side * (rho - (self.radius + self.slope * h))
                    / (1.0 + self.slope * self.slope).sqrt()
            }
        }
    }
    /// The face's normal direction at `x` (its distance's gradient, either way round).
    fn grad(&self, x: Point3) -> Vec3 {
        let d = x - self.at;
        match self.kind {
            Kind::Plane => self.axis,
            Kind::Cylinder => d - self.axis * d.dot(self.axis),
            Kind::Sphere => d,
            Kind::Cone => {
                let h = d.dot(self.axis);
                (d - self.axis * h).unitize().unwrap() - self.axis * self.slope
            }
        }
    }
}

/// A case's corner and what its fill is graded against.
struct Case {
    name: &'static str,
    v: Point3,
    /// The patches' vertices lie within this of `v`.
    reach: f64,
    convex: bool,
    /// A point on the side the fill bounds (the material at a convex corner, the void at a
    /// concave one).
    inside: Point3,
    faces: Vec<CurvedFace>,
    /// A point on each round away from the corner.
    on_rounds: Vec<Point3>,
    /// The least normal curvature the patches may have.
    k: f64,
    faces_want: usize,
}

fn angle(a: Vec3, b: Vec3) -> f64 {
    a.cross(b).length().atan2(a.dot(b))
}

/// The parameters of `s` nearest `x`, and how far `x` is from it there.
fn nearest(k: &Kernel, s: &Surface, x: Point3) -> (Point2, f64) {
    let dom = s.natural_uv_domain_with_nurb(k).unwrap();
    let sols = s
        .global_point_solve_point_test_off::<NoObject>(
            k,
            None,
            &dom,
            SolverOperation::Minimize,
            x,
            1e-12,
            None,
            SolutionRequested::Single,
        )
        .unwrap();
    let sol = sols.get(0).unwrap();
    let uv = Point2::new(sol.start.param(0).unwrap(), sol.start.param(1).unwrap());
    (uv, s.evaluate_point(k, uv).unwrap().distance(x))
}

/// The principal curvatures of `s` at `uv` about its normal away from `inside` (positive
/// where it bends away from that normal, as a convex body's boundary does).
fn principal_curvatures(k: &Kernel, s: &Surface, uv: Point2, inside: Point3) -> Option<(f64, f64)> {
    let flags = EvaluateFlags {
        non_zero_tangents: false,
        ..EvaluateFlags::default()
    };
    let d = s.evaluate(k, uv, 2, 2, flags).ok()?;
    let (p, pu, pv, puu, puv, pvv) = (d[0][0], d[1][0], d[0][1], d[2][0], d[1][1], d[0][2]);
    let mut n = pu.cross(pv).unitize().ok()?;
    if n.dot(p - inside) < 0.0 {
        n = -n;
    }
    let (e, f, g) = (pu.dot(pu), pu.dot(pv), pv.dot(pv));
    let (l, m, nn) = (-puu.dot(n), -puv.dot(n), -pvv.dot(n));
    let a = e * g - f * f;
    let b = -(e * nn - 2.0 * f * m + g * l);
    let c = l * nn - m * m;
    let disc = (b * b - 4.0 * a * c).max(0.0);
    Some((
        (-b - disc.sqrt()) / (2.0 * a),
        (-b + disc.sqrt()) / (2.0 * a),
    ))
}

/// Grades the Setback at `case.v` in `b` (module docs).
fn graded(k: &mut Kernel, b: &mut Brep, case: &Case, n: usize) {
    let name = case.name;
    let lines = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
    assert!(lines.is_empty(), "{name}: {lines:?}");
    let all = get_faces(b).unwrap();
    assert_eq!(all.len(), case.faces_want, "{name}: faces");
    let bbox = brep_bounding_box(k, b, false).unwrap();
    let size = bbox.max().distance(bbox.min());
    let on_tol = 1e-12 * size;
    let surface_of = |f: FaceId| b.face(f).unwrap().surface.clone().unwrap();
    // the rounds' faces: through each round's point
    let round_faces: Vec<FaceId> = case
        .on_rounds
        .iter()
        .map(|&p| {
            *all.iter()
                .find(|&&f| nearest(k, &surface_of(f), p).1 <= 1e-9)
                .unwrap_or_else(|| panic!("{name}: no round through {p:?}"))
        })
        .collect();
    let mut patches = Vec::new();
    for &f in &all {
        let vs = face4::vertices(b, f, None, false, None).unwrap();
        let near = !vs.is_empty()
            && vs
                .iter()
                .all(|&vv| b.vertex(vv).unwrap().point.distance(case.v) <= case.reach);
        let s = surface_of(f);
        let mid = s
            .evaluate_point(k, b.face(f).unwrap().uv_domain.mid())
            .unwrap();
        let on_face = case.faces.iter().any(|cf| cf.g(mid).abs() <= 1e-9);
        let on_round = round_faces
            .iter()
            .any(|&rf| rf == f || nearest(k, &surface_of(rf), mid).1 <= 1e-9);
        if near && !on_face && !on_round {
            patches.push(f);
        }
    }
    assert_eq!(patches.len(), 2 * n, "{name}: patches");
    let (mut worst_seam, mut worst_on, mut worst_k, mut worst_out) =
        (0.0f64, 0.0f64, f64::INFINITY, f64::NEG_INFINITY);
    let mut on_edges = 0;
    for &p in &patches {
        let s = surface_of(p);
        let single = match s.kind() {
            SurfaceKind::BSpline(net) => {
                let c = net.canonical().unwrap();
                c.u_knots.len() == 2 && c.v_knots.len() == 2
            }
            _ => false,
        };
        assert!(single, "{name}: a patch not one Bezier span");
        // its net never turns back (check_brep's test #5, the owner's ruling on #2243)
        if let SurfaceKind::BSpline(bs) = s.kind() {
            let net = bs.control_point_net().unwrap();
            assert!(
                !grid_has_non_monotonic_points(&net.points, net.v_count, false, false).unwrap(),
                "{name}: a patch's control net turns back"
            );
        }
        for e in face4::edges(b, p, None, false, None).unwrap() {
            let curve = edge::curve(b, e).unwrap().unwrap().clone();
            let ivl = edge::interval(b, e).unwrap();
            let mid = curve.evaluate_point(k, ivl.evaluate(0.5)).unwrap();
            let on = case.faces.iter().find(|cf| cf.g(mid).abs() <= 1e-6);
            let prim = solidcore::topology::edgeuse::p1::primary_edgeuse(b, e)
                .unwrap()
                .unwrap();
            let rad = solidcore::topology::edgeuse::p1::radial(b, prim)
                .unwrap()
                .unwrap();
            assert_ne!(prim, rad, "{name}: a patch edge with one face");
            let other = [prim, rad]
                .into_iter()
                .map(|eu| {
                    solidcore::topology::edgeuse::p1::face(b, eu)
                        .unwrap()
                        .unwrap()
                })
                .find(|&f| f != p)
                .unwrap();
            let os = surface_of(other);
            let mut off: f64 = 0.0;
            for q in 0..=32 {
                let x = curve
                    .evaluate_point(k, ivl.evaluate(f64::from(q) / 32.0))
                    .unwrap();
                if let Some(cf) = on {
                    off = off.max(cf.g(x).abs());
                }
                if q == 0 || q == 32 {
                    continue;
                }
                let (uv, _) = nearest(k, &s, x);
                let np = s.evaluate_normal(k, uv, Side::Left, Side::Left).unwrap();
                // against the face's exact normal (a curved face's trim curve in its own
                // parameters is not exact, smlib#103)
                let nf = if let Some(cf) = on {
                    cf.grad(x).unitize().unwrap()
                } else {
                    let (ouv, _) = nearest(k, &os, x);
                    os.evaluate_normal(k, ouv, Side::Left, Side::Left).unwrap()
                };
                let a = angle(np, nf);
                worst_seam = worst_seam.max(a.min(PI - a));
            }
            if on.is_some() {
                on_edges += 1;
                worst_on = worst_on.max(off);
            }
        }
        let dom = b.face(p).unwrap().uv_domain;
        for i in 0..=16 {
            for j in 0..=16 {
                let uv = Point2::new(
                    dom.u_interval().evaluate(f64::from(i) / 16.0),
                    dom.v_interval().evaluate(f64::from(j) / 16.0),
                );
                let x = s.evaluate_point(k, uv).unwrap();
                for cf in &case.faces {
                    worst_out = worst_out.max(if case.convex { cf.g(x) } else { -cf.g(x) });
                }
                if i == 0 || i == 16 || j == 0 || j == 16 {
                    continue;
                }
                let (k1, _) = principal_curvatures(k, &s, uv, case.inside)
                    .unwrap_or_else(|| panic!("{name}: a patch not regular"));
                worst_k = worst_k.min(k1);
            }
        }
    }
    eprintln!(
        "{name}: seams within {worst_seam:.3e} rad; {on_edges} patch edges on the faces, off by \
         {worst_on:.3e}; least principal curvature {worst_k:.6e} (k {:.6e}); past the faces by \
         {worst_out:.3e}",
        case.k
    );
    assert!(worst_seam <= 1e-9, "{name}: seams {worst_seam}");
    assert_eq!(on_edges, 2 * n, "{name}: patch edges on the faces");
    assert!(worst_on <= on_tol, "{name}: off the faces by {worst_on}");
    assert!(
        worst_k >= case.k - 1e-9,
        "{name}: curvature {worst_k} below {}",
        case.k
    );
    assert!(worst_out <= on_tol, "{name}: past the faces by {worst_out}");
}

/// The D prism (R 10, flat a 4, H 10) and its top corner V = (4, sqrt 84, 10), where the
/// top, the flat and the convex cylinder meet; the chord, the upright and the arc to round
/// at 2, 2.5 and 3 there.
fn d_prism_case() -> (Kernel, Brep, Vec<EdgeId>, Case) {
    let mut k = all_fixes();
    let (rr, a, hh) = (10.0f64, 4.0f64, 10.0f64);
    let h = (rr * rr - a * a).sqrt();
    let b = d_prism(&mut k, 0.0, 0.0, rr, a, 0.0, hh);
    let edges = [[a, 0.0, hh], [a, h, 5.0], [-rr, 0.0, hh]]
        .iter()
        .map(|&p| pick(&k, &b, p))
        .collect();
    let s2 = 2.0f64.sqrt();
    let mid = |c: Point3, e: Point3, r: f64| c + (e - c).unitize().unwrap() * r;
    let case = Case {
        name: "d prism",
        v: Point3::new(a, h, hh),
        reach: 9.8,
        convex: true,
        inside: Point3::new(0.0, 0.0, 5.0),
        faces: vec![
            CurvedFace::plane([0.0, 0.0, hh], [0.0, 0.0, 1.0]),
            CurvedFace::plane([a, 0.0, 0.0], [1.0, 0.0, 0.0]),
            CurvedFace {
                kind: Kind::Cylinder,
                at: Point3::new(0.0, 0.0, 0.0),
                axis: Vec3::Z,
                radius: rr,
                slope: 0.0,
                side: 1.0,
            },
        ],
        on_rounds: vec![
            Point3::new(2.0 + s2, 0.0, 8.0 + s2),
            mid(
                Point3::new(1.5, 54.0f64.sqrt(), 5.0),
                Point3::new(a, h, 5.0),
                2.5,
            ),
            Point3::new(-7.0, 0.0, 7.0) + Vec3::new(-3.0, 0.0, 3.0) * (3.0 / 18.0f64.sqrt()),
        ],
        k: 0.0,
        faces_want: 13,
    };
    (k, b, edges, case)
}

#[test]
fn d_prism_unequal_corner_is_a_setback_beside_the_cylinder() {
    let (mut k, mut b, edges, case) = d_prism_case();
    let r = round_each(&mut k, &mut b, &edges, &[2.0, 2.5, 3.0], case.v);
    assert_eq!(r.event, Some(BlendCornerEvent::Setback), "{}", r.reason);
    assert!(r.ok, "{}", r.reason);
    graded(&mut k, &mut b, &case, 3);
}

/// With `fillet_round_as_definition` on (as here), a Setback's fill patches keep the fill's
/// definition (#2488). Beside a curved face its faces are the quadrics they are: each patch is
/// its definition, with its bound, re-certified from it; its record keeps the cylinder as a
/// cylinder and rebuilds the same patch; and moved, the definition's cylinder moves with it.
#[test]
fn beside_a_curved_face_the_fill_is_its_definition() {
    let (mut k, mut b, edges, case) = d_prism_case();
    assert!(k.fixes.fillet_round_as_definition);
    let r = round_each(&mut k, &mut b, &edges, &[2.0, 2.5, 3.0], case.v);
    assert_eq!(r.event, Some(BlendCornerEvent::Setback), "{}", r.reason);
    assert!(r.ok, "{}", r.reason);
    let patches: Vec<Surface> = get_faces(&b)
        .unwrap()
        .into_iter()
        .filter_map(|f| b.face(f).unwrap().surface.clone())
        .filter(|s| {
            s.procedural_blend()
                .and_then(|d| d.corner())
                .is_some_and(|c| c.kind() == CornerKind::Setback)
        })
        .collect();
    assert_eq!(patches.len(), 6, "patches keeping a Setback definition");
    for s in &patches {
        let d = s.procedural_blend().unwrap();
        assert!(d.bound() > 0.0 && d.bound() < 1e-3, "{}", d.bound());
        assert_eq!(d.recertify(&k).unwrap(), 0.0);
        // its record: the D prism's cylinder (R 10 about the z axis, the material inside)
        let rec = corner_to_record(&k, s).unwrap();
        let CornerRecord::Setback(sr) = &rec else {
            panic!("not a setback's record");
        };
        let cylinders: Vec<_> = sr.faces.iter().filter_map(|f| f.quadric).collect();
        assert_eq!(cylinders.len(), 1);
        let q = cylinders[0];
        assert_eq!(q.kind, QuadricKindRecord::Cylinder);
        assert!((q.radius - 10.0).abs() <= 1e-9 && q.slope == 0.0 && q.side == 1.0);
        assert!((q.axis[2].abs() - 1.0).abs() <= 1e-12);
        let json = serde_json::to_string(&rec).unwrap();
        let back = corner_rebuild(&k, &serde_json::from_str(&json).unwrap()).unwrap();
        assert_eq!(corner_to_record(&k, &back).unwrap(), rec);
        assert_eq!(back.procedural_blend().unwrap().recertify(&k).unwrap(), 0.0);
        // moved: the cylinder's axis turns with the patch
        let mut moved = s.copy(&k).unwrap();
        let mut place = Axis2Placement::default();
        place.rotate_about_axis(0.3, Vec3::new(1.0, 2.0, 3.0).unitize().unwrap());
        place.translate(Vec3::new(1e2, -2e2, 5e1));
        moved.transform(&k, &place, None).unwrap();
        let dm = moved.procedural_blend().unwrap();
        let proved = dm.recertify(&k).unwrap();
        assert!(
            proved <= dm.bound(),
            "moved: {proved:e} against {:e}",
            dm.bound()
        );
    }
}

/// The D prism's corner holds its setbacks within each edge's room measured along the edge,
/// as the setbacks are: the top chord 2 sqrt 84 and the upright 10 (straight), and the top
/// arc its arc length `R (2 pi - 2 acos(a / R))`, 39.63, not its chord 18.33.
#[test]
fn d_prism_corner_room_is_measured_along_its_edges() {
    let (mut k, mut b, edges, case) = d_prism_case();
    let r = round_each(&mut k, &mut b, &edges, &[2.0, 2.5, 3.0], case.v);
    assert_eq!(r.event, Some(BlendCornerEvent::Setback), "{}", r.reason);
    assert!(r.ok, "{}", r.reason);
    let (rr, a) = (10.0f64, 4.0f64);
    let mut want = [
        2.0 * (rr * rr - a * a).sqrt(),
        10.0,
        rr * (2.0 * PI - 2.0 * (a / rr).acos()),
    ];
    want.sort_by(f64::total_cmp);
    let mut room = r.room.clone();
    room.sort_by(f64::total_cmp);
    assert_eq!(room.len(), 3, "{room:?}");
    for (&got, &w) in room.iter().zip(&want) {
        assert!((got - w).abs() <= 1e-6, "{room:?} against {want:?}");
    }
}

/// The D prism built far from the origin (its axis at (o, o), its foot at z = o): its corner
/// is the same Setback, its setbacks the origin's (3, 4.5 and 3.75). Where the rails cross on
/// its faces (`curved_least`) is solved to the rounding of the corner's coordinates, not of
/// its radii alone: before, from about 4e3 out, it was refused as "where the rails ... cross
/// on its curved faces cannot be found". At 1e3 and 2e3 it builds. Farther out the fillet
/// fails later, as it does without the curved Setback
/// (`plan/findings/blend.fillet_executive--d-prism-rounds-fail-far-from-the-origin.md`).
#[test]
fn d_prism_corner_far_from_the_origin_is_a_setback() {
    for o in [1.0e3, 2.0e3, 4.0e3, -4.0e3, 1.0e4] {
        let mut k = all_fixes();
        let (rr, a, hh) = (10.0f64, 4.0f64, 10.0f64);
        let h = (rr * rr - a * a).sqrt();
        let mut b = d_prism(&mut k, o, o, rr, a, o, hh);
        let at = |x: f64, y: f64, z: f64| [o + x, o + y, o + z];
        let edges: Vec<EdgeId> = [at(a, 0.0, hh), at(a, h, 5.0), at(-rr, 0.0, hh)]
            .iter()
            .map(|&p| pick(&k, &b, p))
            .collect();
        let v = Point3::new(o + a, o + h, o + hh);
        let r = round_each(&mut k, &mut b, &edges, &[2.0, 2.5, 3.0], v);
        assert_eq!(
            r.event,
            Some(BlendCornerEvent::Setback),
            "at {o}: {}",
            r.reason
        );
        assert!(
            !r.reason.contains("cannot be found"),
            "at {o}: {}",
            r.reason
        );
        assert_eq!(r.setbacks.len(), 3, "at {o}: {}", r.reason);
        for (&s, want) in r.setbacks.iter().zip([3.0, 4.5, 3.75]) {
            assert!((s - want).abs() <= 1e-9 * want, "at {o}: {:?}", r.setbacks);
        }
        if o.abs() <= 2.0e3 {
            assert!(r.ok, "at {o}: {}", r.reason);
        }
    }
}

/// Without `fillet_setback_beside_curved_faces` (as a design made before it), a corner beside a curved face is the corner it was: not a Setback
/// (the N-by-N convex corner), and built, asked for a setback or not. The D prism's corner
/// (beside a cylinder) and the countersink's (beside a cone).
#[test]
fn without_its_flag_a_corner_beside_a_curved_face_builds_as_before() {
    for asked in [None, Some(3.05)] {
        let (mut k, mut b, edges, case) = d_prism_case();
        k.fixes.fillet_setback_beside_curved_faces = false;
        let r = round_asked(&mut k, &mut b, &edges, &[2.0, 2.5, 3.0], case.v, asked);
        assert_eq!(
            r.event,
            Some(BlendCornerEvent::None),
            "{asked:?}: {}",
            r.reason
        );
        assert!(r.ok, "{asked:?}: {}", r.reason);
    }
    let mut k = all_fixes();
    k.fixes.fillet_setback_beside_curved_faces = false;
    let (mut b, edges, radii, case) = countersink_case(&mut k);
    let r = round_each(&mut k, &mut b, &edges, &radii, case.v);
    assert_eq!(r.event, Some(BlendCornerEvent::None), "{}", r.reason);
    assert!(r.ok, "{}", r.reason);
}

/// A 40 x 40 x 20 block with a D-shaped pocket 10 deep (R 8 about (20, 20), flat x = 24,
/// floor z = 10), its floor corner V = (24, 20 + sqrt 48, 10), where the floor, the flat and
/// the concave cylinder meet: the floor's chord, the upright and the floor's arc rounded 2,
/// 2.5 and 3: a concave Setback beside a curved face, the fill convex about the void.
#[test]
fn d_pocket_unequal_corner_is_a_concave_setback_beside_the_cylinder() {
    let mut k = all_fixes();
    let (rr, h) = (8.0f64, 48.0f64.sqrt());
    let block = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 40.0, 20.0).unwrap();
    let cut = d_prism(&mut k, 20.0, 20.0, rr, 4.0, 10.0, 11.0);
    let mut b = boolean_difference(&mut k, block, cut).unwrap();
    let edges: Vec<EdgeId> = [
        [24.0, 20.0, 10.0],
        [24.0, 20.0 + h, 15.0],
        [12.0, 20.0, 10.0],
    ]
    .iter()
    .map(|&p| pick(&k, &b, p))
    .collect();
    let mid = |c: Point3, e: Point3, r: f64| c + (e - c).unitize().unwrap() * r;
    let case = Case {
        name: "d pocket",
        v: Point3::new(24.0, 20.0 + h, 10.0),
        reach: 9.8,
        convex: false,
        inside: Point3::new(20.0, 20.0, 15.0),
        faces: vec![
            CurvedFace::plane([0.0, 0.0, 10.0], [0.0, 0.0, 1.0]),
            CurvedFace::plane([24.0, 0.0, 0.0], [-1.0, 0.0, 0.0]),
            CurvedFace {
                kind: Kind::Cylinder,
                at: Point3::new(20.0, 20.0, 0.0),
                axis: Vec3::Z,
                radius: rr,
                slope: 0.0,
                side: -1.0,
            },
        ],
        on_rounds: vec![
            mid(
                Point3::new(22.0, 20.0, 12.0),
                Point3::new(24.0, 20.0, 10.0),
                2.0,
            ),
            mid(
                Point3::new(21.5, 20.0 + 28.0f64.sqrt(), 15.0),
                Point3::new(24.0, 20.0 + h, 15.0),
                2.5,
            ),
            mid(
                Point3::new(15.0, 20.0, 13.0),
                Point3::new(12.0, 20.0, 10.0),
                3.0,
            ),
        ],
        k: 0.0,
        faces_want: 18,
    };
    let r = round_each(&mut k, &mut b, &edges, &[2.0, 2.5, 3.0], case.v);
    assert_eq!(r.event, Some(BlendCornerEvent::Setback), "{}", r.reason);
    assert!(r.ok, "{}", r.reason);
    graded(&mut k, &mut b, &case, 3);
}

/// The 851 part's corner (41 1/6, -21, 10): the top, the front and the concave arc wall (the
/// material at rho 21 or more about (62 1/6, -21)); the top front edge, the arc's top edge and the upright at the
/// arc rounded 2, 2.5 and 3: a convex Setback beside a concave face, no more concave than the
/// wall and the arc's round (k = -1/21).
#[test]
fn knife_edge_corner_is_a_setback_no_more_concave_than_its_wall() {
    let mut k = all_fixes();
    let tip = 62.0 + 1.0 / 6.0;
    let x0 = tip - 21.0;
    let mut b = knife_edge_arc_part(&mut k);
    let edges: Vec<EdgeId> = [
        [20.0, -21.0, 10.0],
        [47.3174, -6.1508, 10.0],
        [x0, -21.0, 5.0],
    ]
    .iter()
    .map(|&p| pick(&k, &b, p))
    .collect();
    let mid = |c: Point3, e: Point3, r: f64| c + (e - c).unitize().unwrap() * r;
    let q = 0.75 * PI;
    let cu = (24.0f64 * 24.0 - 9.0).sqrt();
    let case = Case {
        name: "knife edge",
        v: Point3::new(x0, -21.0, 10.0),
        reach: 9.8,
        convex: true,
        inside: Point3::new(x0 - 10.0, -11.0, 5.0),
        faces: vec![
            CurvedFace::plane([0.0, 0.0, 10.0], [0.0, 0.0, 1.0]),
            CurvedFace::plane([0.0, -21.0, 0.0], [0.0, -1.0, 0.0]),
            CurvedFace {
                kind: Kind::Cylinder,
                at: Point3::new(tip, -21.0, 0.0),
                axis: Vec3::Z,
                radius: 21.0,
                slope: 0.0,
                side: -1.0,
            },
        ],
        on_rounds: vec![
            mid(
                Point3::new(20.0, -19.0, 8.0),
                Point3::new(20.0, -21.0, 10.0),
                2.0,
            ),
            mid(
                Point3::new(tip + 23.5 * q.cos(), -21.0 + 23.5 * q.sin(), 7.5),
                Point3::new(tip + 21.0 * q.cos(), -21.0 + 21.0 * q.sin(), 10.0),
                2.5,
            ),
            mid(
                Point3::new(tip - cu, -18.0, 5.0),
                Point3::new(x0, -21.0, 5.0),
                3.0,
            ),
        ],
        k: -1.0 / 21.0,
        faces_want: 15,
    };
    let r = round_each(&mut k, &mut b, &edges, &[2.0, 2.5, 3.0], case.v);
    assert_eq!(r.event, Some(BlendCornerEvent::Setback), "{}", r.reason);
    assert!(r.ok, "{}", r.reason);
    graded(&mut k, &mut b, &case, 3);
}

/// The ball of radius 10 about the origin less x > 4, y > 4 and z < 0, its corner V = (4, 4,
/// sqrt 68), where the planes x = 4, y = 4 and the convex sphere meet: the upright, the circle
/// x = 4 and the circle y = 4 on the sphere rounded 1.5, 2 and 2.5 (each circle with every
/// piece the sphere's seam splits it into).
#[test]
fn sphere_corner_is_a_setback_beside_the_sphere() {
    let mut k = all_fixes();
    let ball = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0).unwrap();
    let cut_x = create_box(&mut k, Point3::new(4.0, -20.0, -20.0), 30.0, 40.0, 40.0).unwrap();
    let cut_y = create_box(&mut k, Point3::new(-20.0, 4.0, -20.0), 40.0, 30.0, 40.0).unwrap();
    let cut_z = create_box(&mut k, Point3::new(-20.0, -20.0, -30.0), 40.0, 40.0, 30.0).unwrap();
    let part = boolean_difference(&mut k, ball, cut_x).unwrap();
    let part = boolean_difference(&mut k, part, cut_y).unwrap();
    let mut b = boolean_difference(&mut k, part, cut_z).unwrap();
    let on_sphere = |p: Point3| (p.length() - 10.0).abs() <= 1e-7;
    let mut edges = vec![pick(&k, &b, [4.0, 4.0, 4.0])];
    let mut radii = vec![1.5];
    for (e, r) in edges_on(&k, &b, |p| (p.x - 4.0).abs() <= 1e-7 && on_sphere(p))
        .into_iter()
        .map(|e| (e, 2.0))
        .chain(
            edges_on(&k, &b, |p| (p.y - 4.0).abs() <= 1e-7 && on_sphere(p))
                .into_iter()
                .map(|e| (e, 2.5)),
        )
    {
        edges.push(e);
        radii.push(r);
    }
    let s68 = 68.0f64.sqrt();
    let s84 = 84.0f64.sqrt();
    let mid = |c: Point3, e: Point3, r: f64| c + (e - c).unitize().unwrap() * r;
    let case = Case {
        name: "sphere",
        v: Point3::new(4.0, 4.0, s68),
        reach: 7.9,
        convex: true,
        inside: Point3::new(0.0, 0.0, 3.0),
        faces: vec![
            CurvedFace::plane([4.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            CurvedFace::plane([0.0, 4.0, 0.0], [0.0, 1.0, 0.0]),
            CurvedFace {
                kind: Kind::Sphere,
                at: Point3::new(0.0, 0.0, 0.0),
                axis: Vec3::Z,
                radius: 10.0,
                slope: 0.0,
                side: 1.0,
            },
        ],
        on_rounds: vec![
            mid(Point3::new(2.5, 2.5, 4.0), Point3::new(4.0, 4.0, 4.0), 1.5),
            mid(
                Point3::new(
                    2.0,
                    0.5 * (60.0f64 / 84.0).sqrt(),
                    (83.75f64 * 60.0 / 84.0).sqrt(),
                ),
                Point3::new(4.0, 0.5, 83.75f64.sqrt()),
                2.0,
            ),
            mid(
                Point3::new(0.0, 1.5, 54.0f64.sqrt()),
                Point3::new(0.0, 4.0, s84),
                2.5,
            ),
        ],
        k: 0.0,
        faces_want: 14,
    };
    let r = round_each(&mut k, &mut b, &edges, &radii, case.v);
    assert_eq!(r.event, Some(BlendCornerEvent::Setback), "{}", r.reason);
    assert!(r.ok, "{}", r.reason);
    graded(&mut k, &mut b, &case, 3);
}

/// The point of a round of radius `r` whose ball's centre is `c`, along the bisector of its
/// contact normals `n1` and `n2` out of the material.
fn middle(c: [f64; 3], n1: [f64; 3], n2: [f64; 3], r: f64) -> Point3 {
    let d = (Vec3::new(n1[0], n1[1], n1[2]) + Vec3::new(n2[0], n2[1], n2[2]))
        .unitize()
        .unwrap();
    Point3::new(c[0], c[1], c[2]) + d * r
}

/// The fork's `curved_cone_corner`: a cone's frustum (radius 10 at z = 0 narrowing at 30
/// degrees to z = 6) less x < 0, its top corner V = (0, `rho_t`, 6), `rho_t` = 10 - 6 tan 30, where
/// the top, the plane x = 0 and the convex cone meet: the top's diameter, the generator and the
/// top's arc rounded 1.25, 1.5 and 2 (k = 0). Faces: the part's 5, 4 rounds, 6 patches (15).
#[test]
fn cone_frustum_corner_is_a_setback_beside_the_cone() {
    let mut k = all_fixes();
    let tb = (PI / 6.0).tan();
    let (cb, sb) = ((PI / 6.0).cos(), (PI / 6.0).sin());
    let rt = 10.0 - 6.0 * tb;
    let rm = 10.0 - 3.0 * tb;
    let cone = solidcore::primitive::api_primitives::create_cone(
        &mut k,
        Point3::new(0.0, 0.0, 0.0),
        10.0,
        rt,
        6.0,
    )
    .unwrap();
    let cut = create_box(&mut k, Point3::new(-20.0, -20.0, -1.0), 20.0, 40.0, 10.0).unwrap();
    let mut b = boolean_difference(&mut k, cone, cut).unwrap();
    let mut edges = vec![pick(&k, &b, [0.0, 0.0, 6.0]), pick(&k, &b, [0.0, rm, 3.0])];
    let mut radii = vec![1.25, 1.5];
    for e in edges_on(&k, &b, |p| {
        (p.z - 6.0).abs() <= 1e-7 && (p.x.hypot(p.y) - rt).abs() <= 1e-7
    }) {
        edges.push(e);
        radii.push(2.0);
    }
    let rg = rm - 1.5 / cb;
    let yg = (rg * rg - 2.25).sqrt();
    let ra = 10.0 - 4.0 * tb - 2.0 / cb;
    let case = Case {
        name: "cone frustum",
        v: Point3::new(0.0, rt, 6.0),
        reach: 6.5,
        convex: true,
        inside: Point3::new(3.0, 0.0, 3.0),
        faces: vec![
            CurvedFace::plane([0.0, 0.0, 6.0], [0.0, 0.0, 1.0]),
            CurvedFace::plane([0.0, 0.0, 0.0], [-1.0, 0.0, 0.0]),
            CurvedFace::quadric(Kind::Cone, [0.0, 0.0, 0.0], 10.0, -tb, 1.0),
        ],
        on_rounds: vec![
            middle([1.25, 3.0, 4.75], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.25),
            middle(
                [1.5, yg, 3.0],
                [-1.0, 0.0, 0.0],
                [cb * 1.5 / rg, cb * yg / rg, sb],
                1.5,
            ),
            middle(
                [ra * 0.5, ra * 0.75f64.sqrt(), 4.0],
                [0.0, 0.0, 1.0],
                [cb * 0.5, cb * 0.75f64.sqrt(), sb],
                2.0,
            ),
        ],
        k: 0.0,
        faces_want: 15,
    };
    let r = round_each(&mut k, &mut b, &edges, &radii, case.v);
    assert_eq!(r.event, Some(BlendCornerEvent::Setback), "{}", r.reason);
    assert!(r.ok, "{}", r.reason);
    graded(&mut k, &mut b, &case, 3);
}

/// The fork's `curved_countersink_corner` (below): the part, its three edges at the corner,
/// their radii and the case.
fn countersink_case(k: &mut Kernel) -> (Brep, Vec<EdgeId>, Vec<f64>, Case) {
    let c45 = 0.5f64.sqrt();
    let sink = solidcore::primitive::api_primitives::create_cone(
        k,
        Point3::new(4.0, 0.0, 14.0),
        2.0,
        9.0,
        7.0,
    )
    .unwrap();
    let block = create_box(k, Point3::new(-20.0, -20.0, 0.0), 24.0, 40.0, 20.0).unwrap();
    let b = boolean_difference(k, block, sink).unwrap();
    let mut edges = vec![
        pick(k, &b, [4.0, 14.0, 20.0]),
        pick(k, &b, [4.0, 5.0, 17.0]),
    ];
    let mut radii = vec![1.25, 1.0];
    for e in edges_on(k, &b, |p| {
        (p.z - 20.0).abs() <= 1e-7 && ((p.x - 4.0).hypot(p.y) - 8.0).abs() <= 1e-7
    }) {
        edges.push(e);
        radii.push(1.5);
    }
    let zr = 18.5;
    let rr = (zr - 14.0) + 2.0 + 1.5 / c45;
    let a3 = 0.75 * PI;
    let rg = 5.0 + 1.0 / c45;
    let yg = (rg * rg - 1.0).sqrt();
    let case = Case {
        name: "countersink",
        v: Point3::new(4.0, 8.0, 20.0),
        reach: 6.0,
        convex: true,
        inside: Point3::new(-5.0, 10.0, 10.0),
        faces: vec![
            CurvedFace::plane([0.0, 0.0, 20.0], [0.0, 0.0, 1.0]),
            CurvedFace::plane([4.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            CurvedFace::quadric(Kind::Cone, [4.0, 0.0, 14.0], 2.0, 1.0, -1.0),
        ],
        on_rounds: vec![
            middle(
                [4.0 - 1.25, 16.0, 20.0 - 1.25],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                1.25,
            ),
            middle(
                [4.0 + rr * a3.cos(), rr * a3.sin(), zr],
                [0.0, 0.0, 1.0],
                [-c45 * a3.cos(), -c45 * a3.sin(), c45],
                1.5,
            ),
            middle(
                [3.0, yg, 17.0],
                [1.0, 0.0, 0.0],
                [c45 / rg, -c45 * yg / rg, c45],
                1.0,
            ),
        ],
        k: -c45 / 5.5,
        faces_want: 17,
    };
    (b, edges, radii, case)
}

/// The fork's `curved_countersink_corner`: the block x in [-20, 4], y in [-20, 20], z in [0, 20]
/// less a cone's frustum about (4, 0) widening at 45 degrees from radius 2 at z = 14, its
/// corner V = (4, 8, 20) where the top, the side and the concave cone meet: the top-side edge,
/// the rim and the generator rounded 1.25, 1.5 and 1: a convex corner beside a concave cone, no
/// more concave than the cone (k = -cos 45 / 5.5). Faces: 8, 3 rounds, 6 patches (17). Its
/// rule's setbacks grow to 1.416 x (2.655, 2.124, 3.186); the fork's open item (b), a net
/// that doubled back there, is gone with the net's fairness and certificate (#2243).
#[test]
#[ignore = "slow: its setbacks grow to 1.416 x the rule's (twelve curved fills), about 8 minutes \
            in release; countersink_asked_corner_is_certified_with_its_nets builds the corner \
            without the growth"]
fn countersink_corner_is_a_setback_no_more_concave_than_its_cone() {
    let mut k = all_fixes();
    let (mut b, edges, radii, case) = countersink_case(&mut k);
    let r = round_each(&mut k, &mut b, &edges, &radii, case.v);
    assert_eq!(r.event, Some(BlendCornerEvent::Setback), "{}", r.reason);
    assert!(r.ok, "{}", r.reason);
    graded(&mut k, &mut b, &case, 3);
}

/// The countersink's corner asked to be set back 3.2 along each edge (past its grown rule's
/// setbacks, 2.655, 2.124 and 3.186): certified there as it is, with every patch's net
/// never turning back (net test #5) and contained.
#[test]
fn countersink_asked_corner_is_certified_with_its_nets() {
    let mut k = all_fixes();
    let (mut b, edges, radii, case) = countersink_case(&mut k);
    let r = round_asked(&mut k, &mut b, &edges, &radii, case.v, Some(3.2));
    assert_eq!(r.event, Some(BlendCornerEvent::Setback), "{}", r.reason);
    assert!(r.ok, "{}", r.reason);
    graded(&mut k, &mut b, &case, 3);
}

/// The D prism's corner asked to be set back 3.05 along each edge: at or past every least
/// setback (3 the most), but its fill is not certified there. The setbacks grow with the
/// real rounds (twice first, then the planar search's bisection on the grid of thousandths),
/// and the corner is refused naming the setback that builds, 3.05 x 1.06 = 3.233 (each trial
/// certified by more than its rounding bound, #2243). Slow (the growth's twelve fills, each
/// from degree 3 up to 9 where none certifies).
#[test]
#[ignore = "slow: about 8 minutes in release, more than 20 in the test profile (twelve curved fills); \
            the_named_setback_builds builds the setback it names"]
fn d_prism_asked_too_short_names_the_setback_that_builds() {
    let (mut k, mut b, edges, case) = d_prism_case();
    let r = round_asked(&mut k, &mut b, &edges, &[2.0, 2.5, 3.0], case.v, Some(3.05));
    assert_eq!(r.event, Some(BlendCornerEvent::Setback), "{}", r.reason);
    assert!(!r.ok);
    assert!(
        r.reason.contains("a setback of 3.05 is too short") && r.reason.contains("3.233 builds it"),
        "{}",
        r.reason
    );
    assert!((r.fit - 3.05 * 1.06).abs() <= 1e-12, "{}", r.fit);
}

/// The setback the growth names for 3.05 (3.05 x 1.06, above), asked for, builds
/// (`fit_setback`'s contract, as the planar `asked_setback_too_short_names_what_builds`
/// checks): certified at it as it is, its nets never turning back.
#[test]
fn the_named_setback_builds() {
    let (mut k, mut b, edges, case) = d_prism_case();
    let named = 3.05 * 1.06;
    let r = round_asked(
        &mut k,
        &mut b,
        &edges,
        &[2.0, 2.5, 3.0],
        case.v,
        Some(named),
    );
    assert_eq!(r.event, Some(BlendCornerEvent::Setback), "{}", r.reason);
    assert!(r.ok, "asked {named}: {}", r.reason);
    graded(&mut k, &mut b, &case, 3);
}

/// The D prism with flat a 8 (R 10, H 20), bored about its axis with radius `bore` where
/// given (the bore R 6.9 leaves the top a strip 1.1 wide beside the chord), and its top corner
/// V = (8, 6, 20), where the top, the flat and the convex cylinder meet: the chord, the upright
/// and the arc to round at 1, 1.5 and 2 there. At the chord's far vertex, (8, -6, 20), the
/// chord's and the arc's rounds turn at the unrounded upright (a Mitre); the upright's round
/// ends on the bottom. `faces`: the faces the rounded part has.
fn d_prism_eight_case(bore: Option<f64>, faces: usize) -> (Kernel, Brep, Vec<EdgeId>, Case) {
    let mut k = all_fixes();
    let (rr, a, hh) = (10.0f64, 8.0f64, 20.0f64);
    let h = (rr * rr - a * a).sqrt();
    let mut b = d_prism(&mut k, 0.0, 0.0, rr, a, 0.0, hh);
    if let Some(r) = bore {
        let cyl = create_cylinder(&mut k, Point3::new(0.0, 0.0, -1.0), r, hh + 2.0).unwrap();
        b = boolean_difference(&mut k, b, cyl).unwrap();
    }
    let edges = [[a, 0.0, hh], [a, h, 5.0], [-rr, 0.0, hh]]
        .iter()
        .map(|&p| pick(&k, &b, p))
        .collect();
    let s2 = 0.5f64.sqrt();
    let mid = |c: Point3, e: Point3, r: f64| c + (e - c).unitize().unwrap() * r;
    let case = Case {
        name: if bore.is_some() {
            "bored d prism, flat 8"
        } else {
            "d prism, flat 8"
        },
        v: Point3::new(a, h, hh),
        reach: 11.0,
        convex: true,
        inside: Point3::new(0.0, 0.0, 10.0),
        faces: vec![
            CurvedFace::plane([0.0, 0.0, hh], [0.0, 0.0, 1.0]),
            CurvedFace::plane([a, 0.0, 0.0], [1.0, 0.0, 0.0]),
            CurvedFace {
                kind: Kind::Cylinder,
                at: Point3::new(0.0, 0.0, 0.0),
                axis: Vec3::Z,
                radius: rr,
                slope: 0.0,
                side: 1.0,
            },
        ],
        // on the rounds past the setbacks asked below: the chord's at y = -4.5, the upright's
        // at z = 5, the arc's opposite the flat
        on_rounds: vec![
            Point3::new(7.0 + s2, -4.5, 19.0 + s2),
            mid(
                Point3::new(6.5, 30.0f64.sqrt(), 5.0),
                Point3::new(a, h, 5.0),
                1.5,
            ),
            Point3::new(-8.0 - 2.0f64.sqrt(), 0.0, 18.0 + 2.0f64.sqrt()),
        ],
        k: 0.0,
        faces_want: faces,
    };
    (k, b, edges, case)
}

/// #2835: a setback asked so long that its end section on the arc's round lies farther from
/// the vertex than that round's far end. The round is extended past the Mitre at the chord's
/// far vertex by a quarter of the turn its circle leaves open, and so comes back round toward
/// V. The corner's side of each end section is where the round's edge runs on to the vertex
/// there, not which end of its rails lies nearer the vertex: main took the latter, built
/// asked 6.88 and refused 6.9 to 10.5 (bored R 6.9 or not; by hand the side flips at 6.893)
/// as "a round's end section lies outside its rails' interval", naming no size. The issue's
/// bored prism at 6, as before, and
/// at 7.5, and the plain one at 9 build, graded as the corners above; each is short of where
/// the far Mitre cuts the chord's round (below).
#[test]
fn d_prism_asked_long_setback_builds_on_its_rails() {
    for (bore, asked, faces) in [(Some(6.9), 6.0, 14), (Some(6.9), 7.5, 14), (None, 9.0, 13)] {
        let (mut k, mut b, edges, case) = d_prism_eight_case(bore, faces);
        let r = round_asked(
            &mut k,
            &mut b,
            &edges,
            &[1.0, 1.5, 2.0],
            case.v,
            Some(asked),
        );
        assert_eq!(r.event, Some(BlendCornerEvent::Setback), "{}", r.reason);
        assert!(r.ok, "{}, asked {asked}: {}", case.name, r.reason);
        graded(&mut k, &mut b, &case, 3);
        kept_definitions(&k, &b, &case, 3);
    }
}

/// The rounds through `case.on_rounds` keep their exact rounds' definitions, and the fill's
/// 2n patches the Setback's, each re-certified from it: the corner's side at each end section
/// decides where the rounds are trimmed and which jets the fill takes.
fn kept_definitions(k: &Kernel, b: &Brep, case: &Case, n: usize) {
    let name = case.name;
    let surfaces: Vec<Surface> = get_faces(b)
        .unwrap()
        .into_iter()
        .filter_map(|f| b.face(f).unwrap().surface.clone())
        .collect();
    for &p in &case.on_rounds {
        let s = surfaces
            .iter()
            .find(|s| nearest(k, s, p).1 <= 1e-9)
            .unwrap_or_else(|| panic!("{name}: no round through {p:?}"));
        assert!(
            matches!(s.procedural_blend(), Some(BlendRef::Exact { .. })),
            "{name}: the round through {p:?} keeps no exact round's definition"
        );
    }
    let patches: Vec<&Surface> = surfaces
        .iter()
        .filter(|s| {
            s.procedural_blend()
                .and_then(|d| d.corner())
                .is_some_and(|c| c.kind() == CornerKind::Setback)
        })
        .collect();
    assert_eq!(
        patches.len(),
        2 * n,
        "{name}: patches keeping a Setback definition"
    );
    for s in patches {
        assert_eq!(
            s.procedural_blend().unwrap().recertify(k).unwrap(),
            0.0,
            "{name}"
        );
    }
}

/// #2835: a setback asked past where a far corner cuts a round is refused, naming the largest
/// that fits on the rounds' rails. The Mitre at the chord's far vertex cuts the chord's round
/// where its rail on the top meets the arc's round's rail there, (7, -sqrt 15, 20): 6 + sqrt 15
/// (9.873) along the chord from V, short of the chord's 12. With the side above, the plain
/// prism builds asked 9.85, and 9.9 to 11.9 failed later, trimming the rails, naming nothing.
/// Asked 10.5 on the bored prism and on the plain one: `SetbackTooLong` at V naming that
/// reach less the fit margin (the bound is open: a setback at it would leave the chord's round
/// no length past its end section), a thousand of the chord's tolerances. Asked that size,
/// the plain prism builds, graded as the corners above. The bored one is refused there by the
/// bore ("a round kept round a feature does not close on the body"; the bore's own bound is
/// #2343's (d)), not by the rails.
#[test]
fn d_prism_asked_past_the_rails_names_the_setback_that_fits() {
    for (bore, faces) in [(Some(6.9), 14), (None, 13)] {
        let (mut k, mut b, edges, case) = d_prism_eight_case(bore, faces);
        let v = vertex_at(&b, case.v);
        let want = 6.0 + 15.0f64.sqrt() - 1000.0 * edge::tolerance(&b, edges[0]).unwrap();
        let r = round_asked(&mut k, &mut b, &edges, &[1.0, 1.5, 2.0], case.v, Some(10.5));
        assert_eq!(r.event, Some(BlendCornerEvent::Setback), "{}", r.reason);
        assert!(!r.ok, "{}: built", case.name);
        let named = match r.refusal {
            Some(BlendRefusal::SetbackTooLong { vertex, largest }) => {
                assert_eq!(vertex, v, "{}", case.name);
                assert!(
                    (largest - want).abs() <= 1e-9,
                    "{}: {largest} against {want}",
                    case.name
                );
                largest
            }
            other => panic!("{}: {other:?}, {}", case.name, r.reason),
        };
        if bore.is_none() {
            let (mut k, mut b, edges, case) = d_prism_eight_case(bore, faces);
            let r = round_asked(
                &mut k,
                &mut b,
                &edges,
                &[1.0, 1.5, 2.0],
                case.v,
                Some(named),
            );
            assert!(r.ok, "{}, asked {named}: {}", case.name, r.reason);
            graded(&mut k, &mut b, &case, 3);
            kept_definitions(&k, &b, &case, 3);
        }
    }
}

/// The least distance from the prism's axis of the top face's spring curve at the corner of
/// [`d_prism_eight_case`] when every setback is `s`, by its closed form: the top is a plane,
/// so the spring curve is the parabola (P, X, Q) itself, P on the chord's rail x = 7, `s`
/// along the chord from V, Q on the arc's rail rho = 8, `s` along the arc from V, and X where
/// the rails' tangents there cross. The least over 20000 even steps of the parabola's
/// parameter (each a few millionths long).
fn top_spring_least_rho(s: f64) -> f64 {
    let p = (7.0, 6.0 - s);
    let psi = 6.0f64.atan2(8.0) + s / 10.0;
    let q = (8.0 * psi.cos(), 8.0 * psi.sin());
    let along = (7.0 - q.0) / psi.sin();
    let x = (7.0, q.1 - along * psi.cos());
    (0..=20000)
        .map(|i| {
            let t = f64::from(i) / 20000.0;
            let (a, b, c) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
            (a * p.0 + b * x.0 + c * q.0).hypot(a * p.1 + b * x.1 + c * q.1)
        })
        .fold(f64::INFINITY, f64::min)
}

/// #2343 (d), "a thin flat strip beside a cylinder": the bored D prism (bore R 6.9) asked 8.5
/// at V. By the closed form above its top face's spring curve dips into the bore from asked
/// 7.8068; main built asked 7.8 (the spring curve 6e-4 clear of the bore) and refused 8 to
/// 9.5 late, "a round kept round a feature does not close on the body", naming nothing. The
/// corner checks what it takes out of the top: refused as `SetbackTooLong` at V, naming a
/// setback whose spring curve stays on the face and that lies within 1e-3 of that limit: the
/// root, not the last point of a grid of thousandths below it (0.918 x 8.5 = 7.803, 3.8e-3
/// short). The spring curve clears the bore there by the check's gap, the tolerances, which
/// costs about 6e-4 of setback. Asked that, the corner builds, graded as the corners above.
#[test]
fn bored_d_prism_spring_curve_leaving_the_top_names_the_setback_that_keeps_it() {
    let (bore, asked) = (6.9, 8.5);
    // the limit by the closed form: where the spring curve's least rho reaches the bore
    let (mut clear, mut into) = (7.0, 8.0);
    for _ in 0..50 {
        let mid = 0.5 * (clear + into);
        if top_spring_least_rho(mid) > bore {
            clear = mid;
        } else {
            into = mid;
        }
    }
    let limit = clear;
    assert!((limit - 7.8068).abs() < 1e-4, "{limit}");
    let (mut k, mut b, edges, case) = d_prism_eight_case(Some(bore), 14);
    let v = vertex_at(&b, case.v);
    let r = round_asked(
        &mut k,
        &mut b,
        &edges,
        &[1.0, 1.5, 2.0],
        case.v,
        Some(asked),
    );
    assert_eq!(r.event, Some(BlendCornerEvent::Setback), "{}", r.reason);
    assert!(!r.ok, "asked {asked}: built");
    let named = match r.refusal {
        Some(BlendRefusal::SetbackTooLong { vertex, largest }) => {
            assert_eq!(vertex, v);
            largest
        }
        other => panic!("{other:?}, {}", r.reason),
    };
    // its spring curve clear of the bore, and within 1e-3 of the limit
    assert!(
        top_spring_least_rho(named) > bore,
        "{named}: {}",
        top_spring_least_rho(named)
    );
    assert!(
        named < limit && named > limit - 1e-3,
        "{named} against the limit {limit}"
    );
    let (mut k, mut b, edges, case) = d_prism_eight_case(Some(bore), 14);
    let r = round_asked(
        &mut k,
        &mut b,
        &edges,
        &[1.0, 1.5, 2.0],
        case.v,
        Some(named),
    );
    assert!(r.ok, "asked {named}: {}", r.reason);
    graded(&mut k, &mut b, &case, 3);
    kept_definitions(&k, &b, &case, 3);
}
