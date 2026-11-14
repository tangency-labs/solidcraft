//! #2406 (from tangency-labs/smlib#98): a round ending at a smooth vertex while its rail
//! has already rolled over its side face's tangent edge onto the next face.
//!
//! The part is SolidCraft #851's plate with its concave R21 corner wall (the fork's capi
//! `arc_wall_plate`): the plate x 0..41 1/6, y -21..0, z 0..10 and the corner
//! x 41 1/6..62 1/6 less the cylinder r 21 about the axis A = (62 1/6, -21). Its top front
//! edge (y = -21, z = 10) is rounded R2, then the middle edge of the arc top edge's tangent
//! chain, between that round and the arc wall, is rounded alone at r (the fork's capi
//! `scenario_middle_edge_of_an_arc_tangent_chain_r0_25`). The middle edge ends at two smooth
//! vertices: at the bottom (41 1/6, -21, 8) it runs on into the wall's upright edge, at the
//! top V = (62 1/6 - sqrt(437), -19, 10) into the arc's top edge. Each End cuts the round on
//! the plane through its vertex square to its edge: z = 8 at the bottom, the plane P
//! through V and the wall's axis at the top.
//!
//! The rail on the R2 round reaches the round's tangent edge T with the top face
//! (y = -19, z = 10) before P: a ball touching the round on T at x has its centre at
//! (x, -19, 10 - r), 21 + r from A, so (x - 62 1/6)^2 + 4 = (21 + r)^2, at the angle
//! asin(2 / (21 + r)) about A, short of V's asin(2 / 21) for every r > 0. Past T the ball
//! rolls on the top face: from there to P the round is the one between the top face and
//! the wall (centres on the circle of radius 21 + r about A at z = 10 - r), its rail on the
//! top face the circle of radius 21 + r at z = 10, and the cap's edge on the top face is
//! the top face's line in P from that rail to V. The fork (18e96f1) refuses it,
//! `SMLIB_ERR_NO_ROOM` (`CalcEndCapEdges`: "a rail rolling over into an end is not built").
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_executive::{FitKind, NoRoomReason};
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point2, Point3, Vec3, BIG_DOUBLE};
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
#[path = "support/profile.rs"]
mod profile;
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::surface::procedural_blend::{ProceduralBlend, PROCEDURAL_BLEND_TYPE};
use solidcore::surface::surface::p4::NoObject;
use solidcore::surface::surface::p5::DropPointOptions;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::database_io::{DatabaseReader, DatabaseWriter, FileType};
use solidcore::topology::face::p2 as face2;
use solidcore::topology::geometry_write::write_surface;
use solidcore::topology::{edge, vertex, EdgeId, FaceId};
use solidcore::{Brep, Kernel, SolidError};

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

/// The edge at `at` rounded `r` alone (a circular round), then the body stitched.
fn round(k: &mut Kernel, b: &mut Brep, at: [f64; 3], r: f64) -> Result<(), String> {
    let e = pick(k, b, at);
    {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius(
            k,
            &[e],
            &[r],
            &[FilletSurfaceGeneratorType::Circular],
            1,
            1.0,
        )
        .unwrap();
        if let Err(err) = s.do_fillet(k) {
            let info = s.error_info();
            return Err(format!(
                "{err:?}: {:?} {} (fit {:?})",
                info.code,
                info.comment,
                s.fit_size()
            ));
        }
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    Ok(())
}

/// `SmApiBrepComputeVolume` at 1e-9: the volume the r 0.25 round removes, a difference of
/// two of these, lies 4e-3 inside the bound it is checked to, and at 1e-9 it lands within
/// 4e-9 of its value at 1e-12 (measured).
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

fn clean(k: &mut Kernel, b: &mut Brep) {
    let lines = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
    assert!(lines.is_empty(), "{lines:?}");
}

/// The plate's width, x of the arc wall's axis less 21.
const W: f64 = 41.0 + 1.0 / 6.0;
/// The arc wall's axis (x, y), vertical.
const AXIS: [f64; 2] = [W + 21.0, -21.0];
/// The arc wall's radius and the front round's.
const WALL: f64 = 21.0;
const FRONT_R: f64 = 2.0;
/// A point of the plate's top front edge, and of the middle edge (the fork's capi picks).
const FRONT: [f64; 3] = [20.583333333333332, -21.0, 10.0];
const MIDDLE: [f64; 3] = [41.17486688363082, -20.413218067968561, 9.415188112149071];

/// SolidCraft #851's plate with its concave R21 corner wall.
fn arc_wall_plate(k: &mut Kernel) -> Brep {
    let corner0 = create_box(k, Point3::new(W, -21.0, 0.0), 21.0, 21.0, 10.0).unwrap();
    let disc = create_cylinder(k, Point3::new(W + 21.0, -21.0, 0.0), WALL, 10.0).unwrap();
    let corner = boolean_difference(k, corner0, disc).unwrap();
    let plate = create_box(k, Point3::new(0.0, -21.0, 0.0), W, 21.0, 10.0).unwrap();
    boolean_union(k, plate, corner).unwrap()
}

/// The plate with its top front edge rounded R2.
fn front_rounded(k: &mut Kernel) -> Brep {
    let mut b = arc_wall_plate(k);
    round(k, &mut b, FRONT, FRONT_R).unwrap();
    b
}

/// The distance of `p` from the arc wall's axis.
fn rho(p: Point3) -> f64 {
    let (dx, dy) = (p.x - AXIS[0], p.y - AXIS[1]);
    (dx * dx + dy * dy).sqrt()
}

/// `p` less the front round's axis (y = -19, z = 8, along x), in (y, z).
fn off_front_axis(p: Point3) -> (f64, f64) {
    (p.y + 21.0 - FRONT_R, p.z - 10.0 + FRONT_R)
}

/// The point at angle `phi` about the arc wall's axis (from -x towards +y), `rho` from it,
/// at height `z`.
fn about_axis(phi: f64, rho: f64, z: f64) -> Point3 {
    Point3::new(AXIS[0] - rho * phi.cos(), AXIS[1] + rho * phi.sin(), z)
}

/// The closed forms of the middle edge rounded `r`, at its top End.
struct TopEnd {
    /// V's angle about the axis, asin(2 / 21): P is the half-plane at this angle.
    phi_v: f64,
    /// Where the rail on the R2 round meets its tangent edge T with the top face.
    q: Point3,
    /// The rolled rail's end on P (on the top face), and the wall rail's end on P.
    rail_end: Point3,
    wall_end: Point3,
    /// The ball centres at Q and on P.
    centre_q: Point3,
    centre_p: Point3,
    /// The vertex V.
    v: Point3,
}

impl TopEnd {
    fn new(r: f64) -> Self {
        let phi_v = (2.0 / WALL).asin();
        let phi_q = (2.0 / (WALL + r)).asin();
        Self {
            phi_v,
            q: about_axis(phi_q, WALL + r, 10.0),
            rail_end: about_axis(phi_v, WALL + r, 10.0),
            wall_end: about_axis(phi_v, WALL, 10.0 - r),
            centre_q: about_axis(phi_q, WALL + r, 10.0 - r),
            centre_p: about_axis(phi_v, WALL + r, 10.0 - r),
            v: about_axis(phi_v, WALL, 10.0),
        }
    }

    /// The signed distance of `p` from P (positive past it, away from the bottom).
    fn past_p(&self, p: Point3) -> f64 {
        let n = Vec3::new(self.phi_v.sin(), self.phi_v.cos(), 0.0);
        (p - self.v).dot(n)
    }
}

/// `n + 1` points along edge `e`'s curve.
fn samples(k: &Kernel, b: &Brep, e: EdgeId, n: usize) -> Vec<Point3> {
    let c = edge::curve(b, e).unwrap().unwrap();
    let ivl = edge::interval(b, e).unwrap();
    (0..=n)
        .map(|i| {
            let t = ivl.min() + (ivl.max() - ivl.min()) * (i as f64) / (n as f64);
            c.evaluate(k, t, 0, true, false).unwrap()[0]
        })
        .collect()
}

/// Edge `e`'s end points.
fn ends(b: &Brep, e: EdgeId) -> [Point3; 2] {
    let v0 = edge::vertex(b, e).unwrap().unwrap();
    let v1 = edge::other_vertex(b, e, Some(v0)).unwrap().unwrap();
    [vertex::point(b, v0).unwrap(), vertex::point(b, v1).unwrap()]
}

/// The edges every sample of which `on` takes within `tol`, with their worst value.
fn edges_where(k: &Kernel, b: &Brep, tol: f64, on: impl Fn(Point3) -> f64) -> Vec<(EdgeId, f64)> {
    let mut out = Vec::new();
    for e in get_edges(b).unwrap() {
        if edge::curve(b, e).unwrap().is_none() {
            continue;
        }
        let worst = samples(k, b, e, 16)
            .into_iter()
            .map(|p| on(p).abs())
            .fold(0.0, f64::max);
        if worst <= tol {
            out.push((e, worst));
        }
    }
    out
}

/// Face `f`'s unit normal (the surface's, unsigned) at its point nearest `p`.
fn normal_at(k: &Kernel, b: &Brep, f: FaceId, p: Point3) -> Vec3 {
    let s = face2::surface(b, f).unwrap().unwrap();
    let dom = s.natural_uv_domain_with_nurb(k).unwrap();
    let d = s
        .drop_point::<NoObject>(k, p, &dom, &DropPointOptions::default())
        .unwrap();
    assert!(d.success, "no foot of {p:?}");
    s.evaluate_normal(k, d.result_uv, Side::Right, Side::Right)
        .unwrap()
}

/// The largest angle (radians) between the normals of edge `e`'s two faces along it.
fn crease(k: &Kernel, b: &Brep, e: EdgeId) -> f64 {
    let fs = edge::faces(b, e, None).unwrap();
    assert_eq!(fs.len(), 2, "edge {e:?} is not between two faces");
    let mut worst = 0.0f64;
    for p in samples(k, b, e, 16) {
        let n0 = normal_at(k, b, fs[0], p);
        let n1 = normal_at(k, b, fs[1], p);
        worst = worst.max(n0.dot(n1).abs().min(1.0).acos());
    }
    worst
}

/// The one edge whose samples all satisfy `on` within `tol`, and its worst value.
fn the_edge(
    k: &Kernel,
    b: &Brep,
    what: &str,
    tol: f64,
    on: impl Fn(Point3) -> f64,
) -> (EdgeId, f64) {
    let found = edges_where(k, b, tol, on);
    assert_eq!(found.len(), 1, "{what}: {found:?}");
    found[0]
}

/// Some end of edge `e` at `want`.
fn assert_ends_at(b: &Brep, e: EdgeId, what: &str, want: Point3, tol: f64) {
    let [a, z] = ends(b, e);
    let d = a.distance(want).min(z.distance(want));
    assert!(
        d <= tol,
        "{what}: ends {a:?}, {z:?}, want {want:?} ({d:e} off)"
    );
}

/// The closed forms: the rolled rail on the top face, the cap's edges, the end rib and the
/// rollover seam (the ball's circles in P and at Q), the End's vertices, and the rounds by
/// their definitions (exact, a ball touching its two faces at every section) on both sides
/// of the seam, G1 there and to the faces they roll on. Off them by rounding only.
const EXACT_TOL: f64 = 1e-9;

/// Checks the middle edge rounded `r` at its top End: the rail rolls over T onto the top
/// face, the round from there to P is the top face's and the wall's, and the cap's edges
/// lie on the faces' sections by P. The rolled rail's edge is the rail's fitted curve: off
/// its circle within `rail_tol`.
fn check_top_end(k: &Kernel, b: &Brep, r: f64, rail_tol: f64) {
    let t = TopEnd::new(r);

    // the rolled rail: on the top face (z = 10) on the circle of radius 21 + r about the
    // axis, from T (at Q) to P, the top face G1 to the round on it
    let (rail, off) = the_edge(k, b, "the rolled rail", rail_tol, |p| {
        if t.past_p(p) > EXACT_TOL || p.distance(t.q) > 4.0 * r {
            1.0
        } else {
            (p.z - 10.0).abs().max((rho(p) - (WALL + r)).abs())
        }
    });
    eprintln!("r {r}: the rolled rail off its closed form by {off:e}");
    assert_ends_at(b, rail, "the rolled rail at T", t.q, EXACT_TOL);
    assert_ends_at(b, rail, "the rolled rail on P", t.rail_end, EXACT_TOL);
    let c = crease(k, b, rail);
    eprintln!("r {r}: the rolled rail's crease {c:e}");
    assert!(c <= EXACT_TOL, "the rolled rail is not G1: {c:e}");

    // the cap's edge on the top face: the top face's line in P from the rail's end to V
    let seg = t.v - t.rail_end;
    let (cap_top, off) = the_edge(k, b, "the cap's edge on the top face", EXACT_TOL, |p| {
        let s = ((p - t.rail_end).dot(seg) / seg.dot(seg)).clamp(0.0, 1.0);
        p.distance(t.rail_end + seg * s)
    });
    eprintln!("r {r}: the cap's edge on the top face off by {off:e}");
    assert_ends_at(
        b,
        cap_top,
        "the top cap edge at the rail",
        t.rail_end,
        EXACT_TOL,
    );
    assert_ends_at(b, cap_top, "the top cap edge at V", t.v, EXACT_TOL);

    // the cap's edge on the wall: the wall's line in P (vertical) from the wall rail to V
    let (cap_wall, _) = the_edge(k, b, "the cap's edge on the wall", EXACT_TOL, |p| {
        p.distance(Point3::new(t.v.x, t.v.y, p.z.clamp(10.0 - r, 10.0)))
    });
    assert_ends_at(
        b,
        cap_wall,
        "the wall cap edge at its rail",
        t.wall_end,
        EXACT_TOL,
    );

    // the end rib: the ball's circle on P from the wall's rail to the top face's
    let (rib, off) = the_edge(k, b, "the end rib", EXACT_TOL, |p| {
        (p.distance(t.centre_p) - r).abs().max(t.past_p(p).abs())
    });
    eprintln!("r {r}: the end rib off its circle by {off:e}");
    assert_ends_at(b, rib, "the end rib on the top face", t.rail_end, EXACT_TOL);
    assert_ends_at(b, rib, "the end rib on the wall", t.wall_end, EXACT_TOL);

    // the seam where the rail rolls over: the ball's circle in the section plane at Q (the
    // half-plane through the axis and Q), between the round on the R2 round and the round
    // on the top face, G1
    let nq = {
        let (dx, dy) = (t.centre_q.x - AXIS[0], t.centre_q.y - AXIS[1]);
        Vec3::new(dy, -dx, 0.0) * (1.0 / (dx * dx + dy * dy).sqrt())
    };
    let (seam, off) = the_edge(k, b, "the rollover seam", EXACT_TOL, |p| {
        (p.distance(t.centre_q) - r)
            .abs()
            .max((p - t.centre_q).dot(nq).abs())
    });
    eprintln!("r {r}: the rollover seam off its circle by {off:e}");
    assert_ends_at(b, seam, "the seam at T", t.q, EXACT_TOL);

    // the rounds on both sides of the seam are the ball's by their definitions: the round on
    // the R2 round (and the wall) up to Q, the round on the top face (and the wall) from Q
    // to P. Each one's section in the seam's plane is the ball's circle at Q, and they meet
    // there G1 (their normals the ball's radii); the rolled one's section in P is the
    // ball's circle on P
    let rolled_face = other_face(b, rail, |s| s == "SmPlane_TYPE");
    let rolled = definition(b, rolled_face);
    assert_supports(rolled, ["SmCylinder_TYPE", "SmPlane_TYPE"], r);
    let (round_rail, round_face) = round_rail(k, b, &t, seam, r);
    let before = definition(b, round_face);
    assert_supports(before, ["SmCylinder_TYPE", "SmCone_TYPE"], r);
    let at_q = |pb: &ProceduralBlend| section_in(k, pb, t.centre_q, nq);
    let (s_before, s_rolled) = (at_q(before), at_q(rolled));
    let mut worst = (0.0f64, 0.0f64);
    for i in 0..=8 {
        let f = f64::from(i) / 8.0;
        let (p0, n0) = point_normal(k, before, s_before, from_wall(before, f));
        let (p1, n1) = point_normal(k, rolled, s_rolled, from_wall(rolled, f));
        let off = (p0.distance(t.centre_q) - r)
            .abs()
            .max((p0 - t.centre_q).dot(nq).abs())
            .max(p0.distance(p1));
        worst = (worst.0.max(off), worst.1.max(angle(n0, n1)));
    }
    eprintln!(
        "r {r}: the definitions at Q off the ball's circle by {:e}, their normals by {:e}",
        worst.0, worst.1
    );
    assert!(worst.0 <= EXACT_TOL, "the sections at Q: {:e}", worst.0);
    assert!(worst.1 <= EXACT_TOL, "the seam is not G1: {:e}", worst.1);
    let np = Vec3::new(t.phi_v.sin(), t.phi_v.cos(), 0.0);
    let s_p = section_in(k, rolled, t.centre_p, np);
    let mut off_p = 0.0f64;
    for i in 0..=8 {
        let (p, _) = point_normal(k, rolled, s_p, f64::from(i) / 8.0);
        off_p = off_p.max((p.distance(t.centre_p) - r).abs().max(t.past_p(p).abs()));
    }
    eprintln!("r {r}: the rolled round's section in P off the ball's circle by {off_p:e}");
    assert!(off_p <= EXACT_TOL, "the section in P: {off_p:e}");

    // the rail on the R2 round runs up to T at Q, the ball on it touching the wall: at each
    // point the round's foot on the R2 round is on it, and the ball's centre there (r in
    // from it) is 21 + r from the wall's axis; the round's normal there is the R2 round's;
    // the rail itself, the round's net's edge, lies within the net's certified bound of it
    let on_round = |p: Point3| {
        let (dy, dz) = off_front_axis(p);
        ((dy * dy + dz * dz).sqrt() - FRONT_R).abs()
    };
    let surface = face2::surface(b, round_face).unwrap().unwrap();
    let dom = surface.natural_uv_domain_with_nurb(k).unwrap();
    let round_side = support_index(before, "SmCone_TYPE");
    let mut worst = [0.0f64; 4];
    for p in samples(k, b, round_rail, 32) {
        let d = surface
            .drop_point::<NoObject>(k, p, &dom, &DropPointOptions::default())
            .unwrap();
        assert!(d.success, "no foot of {p:?}");
        let st = before.definition_parameters(d.result_uv);
        let (foot, n) = point_normal(k, before, st.x, round_side as f64);
        let (dy, dz) = off_front_axis(foot);
        let l = (dy * dy + dz * dz).sqrt();
        let centre = Point3::new(foot.x, foot.y - r * dy / l, foot.z - r * dz / l);
        let radial = Vec3::new(0.0, dy / l, dz / l);
        worst = [
            worst[0].max(on_round(foot)),
            worst[1].max((rho(centre) - (WALL + r)).abs()),
            worst[2].max(angle(n, radial)),
            worst[3].max(p.distance(foot)),
        ];
    }
    eprintln!(
        "r {r}: the round's feet on the R2 round off it by {:e}, their balls off the wall by \
         {:e}, its normal off the R2 round's by {:e}; the rail {:e} off them (bound {:e})",
        worst[0],
        worst[1],
        worst[2],
        worst[3],
        before.bound()
    );
    assert!(
        worst[0] <= EXACT_TOL,
        "the feet off the R2 round: {:e}",
        worst[0]
    );
    assert!(
        worst[1] <= EXACT_TOL,
        "the balls off the wall: {:e}",
        worst[1]
    );
    assert!(
        worst[2] <= EXACT_TOL,
        "the round is not G1 to the R2 round: {:e}",
        worst[2]
    );
    assert!(
        worst[3] <= before.bound(),
        "the rail {:e} off the round's feet, bound {:e}",
        worst[3],
        before.bound()
    );
}

/// The angle between two lines along `a` and `b` (radians), to rounding near 0.
fn angle(a: Vec3, b: Vec3) -> f64 {
    a.cross(b).length().atan2(a.dot(b).abs())
}

/// The face of edge `e` other than the one whose surface's type `is` takes.
fn other_face(b: &Brep, e: EdgeId, is: impl Fn(&str) -> bool) -> FaceId {
    let fs = edge::faces(b, e, None).unwrap();
    assert_eq!(fs.len(), 2, "edge {e:?} is not between two faces");
    let ty = |f: FaceId| face2::surface(b, f).unwrap().unwrap().type_string();
    let found: Vec<FaceId> = fs.iter().copied().filter(|&f| !is(ty(f))).collect();
    assert_eq!(
        found.len(),
        1,
        "edge {e:?}: {:?}",
        fs.iter().map(|&f| ty(f)).collect::<Vec<_>>()
    );
    found[0]
}

/// Face `f`'s round by its definition.
fn definition(b: &Brep, f: FaceId) -> &ProceduralBlend {
    let s = face2::surface(b, f).unwrap().unwrap();
    match s.kind() {
        SurfaceKind::ProceduralBlend(pb) => pb,
        _ => panic!(
            "face {f:?} is a {}, not a round by its definition",
            s.type_string()
        ),
    }
}

/// The definition rolls a ball of radius `r` on supports of these types.
fn assert_supports(pb: &ProceduralBlend, mut want: [&str; 2], r: f64) {
    let mut got = [0, 1].map(|i| pb.support(i).unwrap().type_string());
    got.sort_unstable();
    want.sort_unstable();
    assert_eq!(got, want);
    assert_eq!([pb.offset(0), pb.offset(1)], [Some(r), Some(r)]);
}

/// The definition's t of its foot on the support of type `ty` (0 or 1).
fn support_index(pb: &ProceduralBlend, ty: &str) -> usize {
    (0..2)
        .find(|&i| pb.support(i).unwrap().type_string() == ty)
        .unwrap()
}

/// The definition's t a fraction `f` of the way from its foot on the wall.
fn from_wall(pb: &ProceduralBlend, f: f64) -> f64 {
    if support_index(pb, "SmCylinder_TYPE") == 0 {
        f
    } else {
        1.0 - f
    }
}

/// The definition's point and unit normal at (s, t).
fn point_normal(k: &Kernel, pb: &ProceduralBlend, s: f64, t: f64) -> (Point3, Vec3) {
    let d = pb.evaluate_definition(k, s, t, 1, 1).unwrap();
    let n = d[1][0].cross(d[0][1]);
    (d[0][0], n * (1.0 / n.length()))
}

/// The s of the definition's section whose middle lies in the plane through `o` square to
/// `n`, nearest `o` (the plane crosses the leader more than once where it turns about the
/// wall's axis): each crossing bracketed among its leader's parameters (a hundredth past
/// each end, for a section at an end), then bisected to rounding.
fn section_in(k: &Kernel, pb: &ProceduralBlend, o: Point3, n: Vec3) -> f64 {
    let ivl = pb.leader().natural_interval();
    let mid = |s: f64| point_normal(k, pb, s, 0.5).0;
    let f = |s: f64| (mid(s) - o).dot(n);
    let (lo, len) = (ivl.min() - 0.01 * ivl.length(), 1.02 * ivl.length());
    let at = |i: usize| lo + len * (i as f64) / 256.0;
    let mut best: Option<(f64, f64)> = None;
    for i in 0..256 {
        let (mut a, mut z) = (at(i), at(i + 1));
        if f(a) * f(z) > 0.0 {
            continue;
        }
        for _ in 0..200 {
            let m = 0.5 * (a + z);
            if m <= a || m >= z {
                break;
            }
            if f(a) * f(m) <= 0.0 {
                z = m;
            } else {
                a = m;
            }
        }
        let s = 0.5 * (a + z);
        let d = mid(s).distance(o);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((s, d));
        }
    }
    best.expect("the definition's sections cross the plane").0
}

/// The rail on the R2 round ending at Q, and the round's face beside it.
fn round_rail(k: &Kernel, b: &Brep, t: &TopEnd, seam: EdgeId, r: f64) -> (EdgeId, FaceId) {
    let mut found = Vec::new();
    for e in get_edges(b).unwrap() {
        if edge::curve(b, e).unwrap().is_none() || e == seam {
            continue;
        }
        let [a, z] = ends(b, e);
        if a.distance(t.q).min(z.distance(t.q)) > EXACT_TOL {
            continue;
        }
        let fs = edge::faces(b, e, None).unwrap();
        let ty = |f: FaceId| face2::surface(b, f).unwrap().unwrap().type_string();
        if fs.len() == 2
            && fs.iter().any(|&f| ty(f) == "SmCone_TYPE")
            && samples(k, b, e, 16).iter().any(|p| p.z < 10.0 - r)
        {
            found.push((e, other_face(b, e, |s| s == "SmCone_TYPE")));
        }
    }
    assert_eq!(found.len(), 1, "the rail on the R2 round: {found:?}");
    found[0]
}

/// The middle edge rounded 0.25 builds: its rail rolls over T onto the top face before the
/// top End, and the cap's edge on the top face is the top face's line in P. The fork
/// (18e96f1) refuses it, `SMLIB_ERR_NO_ROOM` with no size.
#[test]
fn the_middle_edge_r0_25_rolls_over_onto_the_top_face_before_its_end() {
    let mut k = all_fixes();
    // (built twice, not cloned: a cloned body's surfaces have no owning faces)
    let mut b = front_rounded(&mut k);
    let mut before = front_rounded(&mut k);
    round(&mut k, &mut b, MIDDLE, 0.25).unwrap();
    clean(&mut k, &mut b);
    let removed = volume(&k, &mut before) - volume(&k, &mut b);
    eprintln!(
        "r 0.25: removed {removed}, faces {}",
        get_faces(&b).unwrap().len()
    );
    // the round takes less than its ball's corner, r^2 (1 - pi/4), along the edge (a
    // quarter of the R2 round's circle and the rolled stretch)
    let corner = 0.0625 * (1.0 - std::f64::consts::FRAC_PI_4);
    assert!(
        removed > 0.0 && removed < corner * (std::f64::consts::PI + 0.1),
        "{removed}"
    );
    check_top_end(&k, &b, 0.25, EXACT_TOL);
}

/// At r 1 the rolled stretch is four times as long (0.096): it builds the same way.
#[test]
fn the_middle_edge_r1_rolls_over_onto_the_top_face_before_its_end() {
    let mut k = all_fixes();
    let mut b = front_rounded(&mut k);
    round(&mut k, &mut b, MIDDLE, 1.0).unwrap();
    clean(&mut k, &mut b);
    check_top_end(&k, &b, 1.0, EXACT_TOL);
}

/// The round on each side of the seam is
/// a `ProceduralBlend`, a ball of radius r by its definition: the rolled one rolls it on the
/// wall and the top face (the face beyond T), the one before on the wall and the R2 round.
/// Each goes to the fork's record and back unchanged: the same definition, the same points
/// bit for bit.
#[test]
fn the_rounds_by_their_definitions_round_trip() {
    let r = 0.25;
    let mut k = all_fixes();
    let mut b = front_rounded(&mut k);
    round(&mut k, &mut b, MIDDLE, r).unwrap();
    let mut kinds = Vec::new();
    for f in get_faces(&b).unwrap() {
        let s = face2::surface(&b, f).unwrap().unwrap();
        let SurfaceKind::ProceduralBlend(pb) = s.kind() else {
            continue;
        };
        let mut supports = [0, 1].map(|i| pb.support(i).unwrap().type_string());
        supports.sort_unstable();
        kinds.push(supports);
        assert_eq!([pb.offset(0), pb.offset(1)], [Some(r), Some(r)]);
        let mut db = DatabaseWriter::open_for_write(FileType::Binary);
        write_surface(s, &mut db, 30).unwrap();
        let mut rd = DatabaseReader::new();
        rd.open_for_read(db.into_bytes(), FileType::Binary);
        let back = Surface::read_from_db(&k, PROCEDURAL_BLEND_TYPE, &mut rd, 30).unwrap();
        let SurfaceKind::ProceduralBlend(back_pb) = back.kind() else {
            panic!("face {f:?} read back as a {}", back.type_string());
        };
        assert!(pb.same_definition(&k, back_pb).unwrap(), "face {f:?}");
        let dom = s.natural_uv_domain_with_nurb(&k).unwrap();
        for i in 0..=4 {
            for j in 0..=4 {
                let uv = Point2::new(
                    dom.min().x + (dom.max().x - dom.min().x) * f64::from(i) / 4.0,
                    dom.min().y + (dom.max().y - dom.min().y) * f64::from(j) / 4.0,
                );
                let (p, q) = (
                    s.evaluate_point(&k, uv).unwrap(),
                    back.evaluate_point(&k, uv).unwrap(),
                );
                assert_eq!(
                    [p.x, p.y, p.z].map(f64::to_bits),
                    [q.x, q.y, q.z].map(f64::to_bits),
                    "face {f:?} at {uv:?}"
                );
            }
        }
    }
    kinds.sort_unstable();
    assert_eq!(
        kinds,
        [
            ["SmCone_TYPE", "SmCylinder_TYPE"],
            ["SmCylinder_TYPE", "SmPlane_TYPE"]
        ]
    );
}

/// At r 2, the R2 round's own radius, the ball's offset of the R2 round collapses onto its
/// axis: the ball's centre is fixed where that axis crosses the wall's offset, and at the
/// top End its zone has no section (#2401). Refused as such (`OffsetCollapses`), naming 2
/// less the fit margin (a thousand of the edge's tolerances), not a bare 1001.
#[test]
fn the_middle_edge_at_the_front_round_s_radius_is_refused_as_a_collapsed_offset() {
    let mut k = all_fixes();
    let mut b = front_rounded(&mut k);
    let e = pick(&k, &b, MIDDLE);
    let fit = 2.0 - 1000.0 * edge::tolerance(&b, e).unwrap();
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_per_edge_constant_radius(
        &mut k,
        &[e],
        &[2.0],
        &[FilletSurfaceGeneratorType::Circular],
        1,
        1.0,
    )
    .unwrap();
    match s.do_fillet(&mut k) {
        Err(SolidError::FilletRefused {
            reason: Some(NoRoomReason::OffsetCollapses),
            fit: got,
            ..
        }) => {
            assert_eq!(got.kind, FitKind::Radius);
            assert!(
                (got.size - fit).abs() <= 1e-12,
                "fit {}, want {fit}",
                got.size
            );
        }
        other => panic!("{other:?}"),
    }
}

/// Closer to r 2 the ball's foot on the R2 round sits further past T at P: asin(2 r /
/// (21 (2 - r))), 42 degrees at r 1.75. It still builds the same way; its rolled stretch,
/// 0.168 long, is longer, and the rail's fitted curve lies 1.8e-9 off its circle (pinned at
/// 1e-8; 1.9e-9 just short of 42/23 and 4.2e-9 at 1.96).
#[test]
fn the_middle_edge_r1_75_rolls_over_onto_the_top_face_before_its_end() {
    let mut k = all_fixes();
    let mut b = front_rounded(&mut k);
    round(&mut k, &mut b, MIDDLE, 1.75).unwrap();
    clean(&mut k, &mut b);
    check_top_end(&k, &b, 1.75, 1e-8);
}

/// Just short of r 42/23 the ball's foot on the R2 round at P would be 90 degrees past T
/// (its centre on the R2 round's offset as far from the wall's axis's plane through T as
/// it goes), past where the R2 round runs on (45 degrees): the rails traced on it end short
/// of P, and the End's rail ends take their traced ends until the rollover. The ball rolls
/// over T onto the top face long before P, as at r 0.25, and on it reaches P.
#[test]
fn the_middle_edge_just_short_of_r_42_23_rolls_over_onto_the_top_face_before_its_end() {
    let r = 42.0 / 23.0 - 1e-3;
    let mut k = all_fixes();
    let mut b = front_rounded(&mut k);
    round(&mut k, &mut b, MIDDLE, r).unwrap();
    clean(&mut k, &mut b);
    check_top_end(&k, &b, r, 1e-8);
}

/// The size r 2's refusal names builds: past r 42/23 the ball on the R2 round never reaches
/// P (the trace on it goes on without the edge's end's section), but it leaves the R2 round
/// at T long before and rolls on the top face to P.
#[test]
fn the_size_the_collapsed_offset_names_builds() {
    let mut k = all_fixes();
    let mut b = front_rounded(&mut k);
    let e = pick(&k, &b, MIDDLE);
    let fit = 2.0 - 1000.0 * edge::tolerance(&b, e).unwrap();
    round(&mut k, &mut b, MIDDLE, fit).unwrap();
    clean(&mut k, &mut b);
    check_top_end(&k, &b, fit, 1e-8);
    profile::write_profile(&k);
}

/// The round's ball touching the R2 round on T lies short of P (at the angle
/// asin(2 / (21 + r)) about A, short of V's asin(2 / 21)) for every r: part of the round lies
/// between the edge's ends, so it is not refused as rolling past an end (#14846's refusal of
/// a ball every position of which lies past an end): it builds, and the body is clean.
#[test]
fn the_middle_edge_r1_9_is_not_refused_as_rolling_past_an_end() {
    let r: f64 = 1.9;
    assert!((2.0 / (21.0 + r)).asin() < (2.0_f64 / 21.0).asin());
    let mut k = all_fixes();
    let mut b = front_rounded(&mut k);
    round(&mut k, &mut b, MIDDLE, r).unwrap();
    clean(&mut k, &mut b);
}
