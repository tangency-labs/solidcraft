//! WR-697 (smlib#79 step 4b, the fork's tangency-labs/smlib@e647dac, 1fcbdb7, af7a4d9):
//! with the session's swept turns (`FilletSession::set_swept_turns`, the fork's
//! `SetSweptTurns`) the blend corner's Turn is Swept: each round ends on its own section
//! and the corner's face is the end section revolved about the side edge, an exact torus.
//!
//! The fork's capi case `turn swept_boss_base`: a 40 x 30 x 10 plate with a 10 x 10 x 5
//! boss on it (x 15..25, y 10..20), the boss's four base edges rounded r 1 together. At
//! each vertical boss edge the two rounds' end sections at (cx, cy, 11) turn about the
//! edge: a horn torus of radii 1, 1 centred on the edge at (cx, cy, 11). The fork's notes
//! give the closed form `12500 + 40 A + 2 pi M = 12508.885259020` and its own volume
//! 12508.885258803. The fork 5e8c9c1, built locally and run as capi's `round_edges` calls
//! the executive (`SetSweptTurns(TRUE)` before `CreateFilletCorners`), prints volume
//! 12508.885258803208, 19 faces and four tori of radii 1, 1. Refused there: the same
//! rounds along a pocket's rim (a sharp concave side edge, `SM_FILERR_NO_ROOM`, 8) and
//! unequal radii 1 and 1.5 (`SM_FILERR_CORNER_UNSUPPORTED`, 10 there, 9 here), both at
//! `CreateFilletCorners` with `SM_ERR`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::blend_corner::TurnChoice;
use solidcore::blend::blend_refusal::BlendRefusal;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::FilletErrorType;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::error::SM_ERR;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::det;
use solidcore::math::{Point3, BIG_DOUBLE};
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, EntityRef, Kernel, SolidError};

/// Every fix.
fn all_fixes() -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        *f.flag_mut(name).unwrap() = true;
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

fn boss_base(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 10.0).unwrap();
    let boss = create_box(k, Point3::new(15.0, 10.0, 10.0), 10.0, 10.0, 5.0).unwrap();
    boolean_union(k, plate, boss).unwrap()
}

fn pocket(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 10.0).unwrap();
    let cut = create_box(k, Point3::new(10.0, 10.0, 5.0), 20.0, 10.0, 6.0).unwrap();
    boolean_difference(k, plate, cut).unwrap()
}

const BASE: [[f64; 3]; 4] = [
    [20.0, 10.0, 10.0],
    [25.0, 15.0, 10.0],
    [20.0, 20.0, 10.0],
    [15.0, 15.0, 10.0],
];

const RIM: [[f64; 3]; 4] = [
    [20.0, 10.0, 5.0],
    [30.0, 15.0, 5.0],
    [20.0, 20.0, 5.0],
    [10.0, 15.0, 5.0],
];

/// Rounds the edges at `at`, swept turns on: the error code and reason on failure.
fn round(
    k: &mut Kernel,
    b: &mut Brep,
    at: &[[f64; 3]],
    radii: &[f64],
) -> Result<(), (i32, String)> {
    let edges: Vec<EdgeId> = at.iter().map(|&p| pick(k, b, p)).collect();
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius(k, &edges, radii, &xs, 1, 1.0)
            .unwrap();
        s.set_swept_turns(true);
        if s.do_fillet(k).is_err() {
            let info = s.error_info();
            return Err((info.code, info.comment.clone()));
        }
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    Ok(())
}

/// Rounds the edges at `at` as [`round`] does, their turns made as `turns` says
/// ([`FilletSession::set_turns`]): the error code and reason on failure.
fn round_turning(
    k: &mut Kernel,
    b: &mut Brep,
    at: &[[f64; 3]],
    radii: &[f64],
    turns: TurnChoice,
) -> Result<(), (i32, String)> {
    let edges: Vec<EdgeId> = at.iter().map(|&p| pick(k, b, p)).collect();
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius(k, &edges, radii, &xs, 1, 1.0)
            .unwrap();
        s.set_turns(turns);
        if s.do_fillet(k).is_err() {
            let info = s.error_info();
            return Err((info.code, info.comment.clone()));
        }
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    Ok(())
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// The tori among `b`'s faces: (major, minor) radii.
fn tori(b: &Brep) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for f in get_faces(b).unwrap() {
        if let Some(s) = face2::surface(b, f).unwrap() {
            if let SurfaceKind::Torus(t) = s.kind() {
                out.push((t.major_radius(), t.minor_radius()));
            }
        }
    }
    out
}

/// The fork 5e8c9c1's volume of the swept boss base (run locally).
const FORK_SWEPT_VOLUME: f64 = 12_508.885_258_803_208;

#[test]
fn boss_base_swept_turns_are_horn_tori() {
    let mut k = all_fixes();
    let mut b = boss_base(&mut k);
    round(&mut k, &mut b, &BASE, &[1.0; 4]).unwrap();
    let v = volume(&k, &mut b);
    let faces = get_faces(&b).unwrap().len();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    let tori = tori(&b);
    eprintln!("volume {v:.12}, {faces} faces, tori {tori:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(faces, 19);
    assert_eq!(tori.len(), 4);
    for (major, minor) in tori {
        assert!((major - 1.0).abs() <= 1e-12 && (minor - 1.0).abs() <= 1e-12);
    }
    assert!(
        (v - FORK_SWEPT_VOLUME).abs() <= 1e-9 * FORK_SWEPT_VOLUME,
        "{v}"
    );
}

#[test]
fn a_pocket_rim_is_refused_naming_the_radius() {
    let mut k = all_fixes();
    let mut b = pocket(&mut k);
    let (code, why) = round(&mut k, &mut b, &RIM, &[1.0; 4]).unwrap_err();
    assert_eq!(code, FilletErrorType::NoRoom as i32);
    assert!(
        why.contains("round that edge at a radius of at least 1 first"),
        "{why}"
    );
}

/// The fork's `NoteFitRadius(0.0, edge)` (5e8c9c1) at the pocket rim's sharp concave side
/// edge: no radius builds the swept turn, so the refusal's fit size is 0, for the first
/// round's edge.
#[test]
fn a_pocket_rim_notes_that_no_radius_fits() {
    let mut k = all_fixes();
    let mut b = pocket(&mut k);
    let edges: Vec<EdgeId> = RIM.iter().map(|&p| pick(&k, &b, p)).collect();
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_per_edge_constant_radius(&mut k, &edges, &[1.0; 4], &xs, 1, 1.0)
        .unwrap();
    s.set_swept_turns(true);
    assert!(s.do_fillet(&mut k).is_err());
    let fit = s.fit_size().expect("a fit size is noted");
    eprintln!("{fit:?}");
    assert_eq!(fit.size, 0.0);
    assert!(fit.edge.is_some_and(|e| edges.contains(&e)), "{fit:?}");
}

/// The pocket rim's refusal as data, beside its note: the error is the typed refusal at the
/// noted corner, one of the rim's four floor corners, with the status `SM_ERR` the corners
/// step fails with, the sharp concave side edge there (the pocket's upright edge) and the
/// least radius it is rounded at for the turn to pass, the rounds' radius
/// (`docs/generic-blend.md`, Turn: R >= r). The kernel case
/// `fillets/cov-fillet-turn-swept-pocket-sharp-corner-refused-r1` is this pocket.
#[test]
fn a_pocket_rim_is_refused_with_its_radius_as_data() {
    let mut k = all_fixes();
    let mut b = pocket(&mut k);
    let edges: Vec<EdgeId> = RIM.iter().map(|&p| pick(&k, &b, p)).collect();
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_per_edge_constant_radius(&mut k, &edges, &[1.0; 4], &xs, 1, 1.0)
        .unwrap();
    s.set_swept_turns(true);
    let e = s.do_fillet(&mut k).unwrap_err();
    let info = s.error_info();
    assert_eq!(info.code, FilletErrorType::NoRoom as i32);
    let at = info.location.expect("the corner's point is noted");
    let Some(EntityRef::Vertex(v)) = e.entity() else {
        panic!("{e:?}");
    };
    let SolidError::BlendRefused {
        code,
        refusal:
            BlendRefusal::SweptTurnPastSharpEdge {
                vertex: refused_at,
                side_edge,
                least_radius,
            },
        ..
    } = e
    else {
        panic!("{e:?}");
    };
    assert_eq!(code, SM_ERR);
    assert_eq!(refused_at, v);
    // the rounds' one radius, asked 1: a side edge rounded at R >= 1 lets the ball pass
    assert_eq!(least_radius, 1.0);
    let t = s.target();
    let p = vertex::point(t, v).unwrap();
    assert_eq!(p, at);
    assert!(
        [[10.0, 10.0], [30.0, 10.0], [30.0, 20.0], [10.0, 20.0]].contains(&[p.x, p.y])
            && p.z == 5.0,
        "{p:?}"
    );
    // the side edge is the pocket's upright edge at that corner, from the floor (z 5) to the
    // plate's top (z 10)
    let ends = [
        edge::start_vertex(t, side_edge).unwrap(),
        edge::end_vertex(t, side_edge).unwrap(),
    ]
    .map(|end| vertex::point(t, end.unwrap()).unwrap());
    let top = Point3::new(p.x, p.y, 10.0);
    assert!(ends == [p, top] || ends == [top, p], "{ends:?}");
    // refused at the corners step, before any blend is made: the pocketed plate as it was,
    // the box's 6 faces, 12 edges and 8 vertices and the pocket's 5, 12 and 8
    let counts = (
        get_faces(t).unwrap().len(),
        get_edges(t).unwrap().len(),
        t.vertex_list().len(),
    );
    assert_eq!(counts, (11, 24, 16));
}

/// An L-shaped prism, two 20 x 10 x 5 boxes crossed at the origin, so the L's inner corner is a
/// sharp concave vertical edge at (10, 10). Its two inner walls' top edges are rounded r 1
/// together: convex rounds, whose ball lies in the material, turning round a concave side edge,
/// whose air side is a 90 degree wedge. The ball keeps its distance r from that wedge by
/// circling the edge, so the swept turn passes, a horn torus of radii 1, 1, where a ball in the
/// air (the pocket rim's) could not.
///
/// Closed form: the L is 300 x 5 = 1500. Each straight round takes the spandrel between the
/// square corner and its arc, r^2 (1 - pi/4), off each unit length of its 10 mm wall: it ends
/// square at the wall's far end (the end face there is square to it) and at the corner's
/// section. The corner revolves the same spandrel about the side edge through the turn, pi/2.
/// By Pappus that is its area times pi/2 times its centroid's distance from the axis, the
/// distance from the wall, (10 - 3 pi) r / (3 (4 - pi)): pi r^3 (10 - 3 pi) / 24.
///
/// The torus: the end sections' arcs are centred at (10, 9, 4) and (9, 10, 4), one r in from
/// the top face and from their walls, and revolve about the side edge x = 10, y = 10, so it is
/// centred on that edge at the height of the arcs' centres, (10, 10, 4), with its axis along
/// z. Its face is the triangle S = (10, 10, 4), where both sections meet the side edge, A =
/// (10, 9, 5) and B = (9, 10, 5) on the top face.
///
/// Topology: the L has 8 faces, 18 edges and 12 vertices. The rounds and the torus add 3
/// faces. The rounded top corner (10, 10, 5) becomes S, A and B, and each round's far end
/// corner, (20, 10, 5) and (10, 20, 5), becomes two (the end face's notch, an arc from the top
/// face down to the wall): 12 + 2 + 1 + 1 = 16 vertices. Edges: the 6 bottom ones, 6 upright
/// ones (three ending at S and at the far ends' wall points), 7 round the top face (the outer
/// four, the two rounds' tangent lines and the torus's arc A-B), the two rounds' tangent lines
/// on their walls, the two end notches' arcs and the torus's arcs S-A and S-B: 25. Euler:
/// 16 - 25 + 11 = 2.
#[test]
fn convex_rounds_turn_round_a_concave_side_edge_as_a_horn_torus() {
    let mut k = all_fixes();
    let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 10.0, 5.0).unwrap();
    let c = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 5.0).unwrap();
    let mut b = boolean_union(&mut k, a, c).unwrap();
    round(
        &mut k,
        &mut b,
        &[[15.0, 10.0, 5.0], [10.0, 15.0, 5.0]],
        &[1.0; 2],
    )
    .unwrap();
    let v = volume(&k, &mut b);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    let tori = tori(&b);
    let pi = std::f64::consts::PI;
    let expect = 1500.0 - 2.0 * 10.0 * (1.0 - pi / 4.0) - pi * (10.0 - 3.0 * pi) / 24.0;
    eprintln!("volume {v:.12} (closed form {expect:.12}), tori {tori:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(tori.len(), 1, "{tori:?}");
    assert!((tori[0].0 - 1.0).abs() <= 1e-12 && (tori[0].1 - 1.0).abs() <= 1e-12);
    assert!((v - expect).abs() <= 1e-9 * expect, "{v} against {expect}");
    let faces = get_faces(&b).unwrap().len();
    let edges = get_edges(&b).unwrap().len();
    let vertices = solidcore::topology::brep::p1::vertices(&b, None)
        .unwrap()
        .len();
    eprintln!("{faces} faces, {edges} edges, {vertices} vertices");
    assert_eq!((faces, edges, vertices), (11, 25, 16));
    let frames: Vec<_> = get_faces(&b)
        .unwrap()
        .into_iter()
        .filter_map(|f| match face2::surface(&b, f).unwrap()?.kind() {
            SurfaceKind::Torus(t) => Some(*t.base().position()),
            _ => None,
        })
        .collect();
    let place = frames[0];
    let o = place.origin();
    let z = place.z_axis();
    eprintln!("torus origin {o:?}, axis {z:?}");
    assert!(
        (o.x - 10.0).abs() <= 1e-12 && (o.y - 10.0).abs() <= 1e-12 && (o.z - 4.0).abs() <= 1e-12,
        "{o:?}"
    );
    assert!(z.x.abs() <= 1e-12 && z.y.abs() <= 1e-12, "{z:?}");
}

/// Two convex rounds at a box's corner, the third edge there (the convex side edge) left sharp:
/// the ball lies in the material, which is only a 90 degree wedge round that edge, so it cannot
/// circle it. The swept turn is refused, as the pocket rim's is (a ball in the air at a concave
/// side edge), naming the radius to round the side edge first, and builds no body
/// (tangency-labs/solidcraft#13805).
#[test]
fn convex_rounds_at_a_convex_side_edge_are_refused_naming_the_radius() {
    let mut k = all_fixes();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let (code, why) = round(
        &mut k,
        &mut b,
        &[[5.0, 0.0, 10.0], [0.0, 5.0, 10.0]],
        &[1.0; 2],
    )
    .unwrap_err();
    assert_eq!(code, FilletErrorType::NoRoom as i32, "{why}");
    assert!(
        why.contains("round that edge at a radius of at least 1 first"),
        "{why}"
    );
}

/// The spandrel between a round's square corner and its arc, r^2 (1 - pi/4), and its first
/// moment about either of its straight sides, r^3 (10 - 3 pi) / 12 (its centroid lies
/// (10 - 3 pi) r / (3 (4 - pi)) from each), for r 1.
fn spandrel() -> (f64, f64) {
    let pi = std::f64::consts::PI;
    (1.0 - pi / 4.0, (10.0 - 3.0 * pi) / 12.0)
}

/// The L of [`convex_rounds_turn_round_a_concave_side_edge_as_a_horn_torus`].
fn l_prism(k: &mut Kernel) -> Brep {
    let a = create_box(k, Point3::new(0.0, 0.0, 0.0), 20.0, 10.0, 5.0).unwrap();
    let c = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 5.0).unwrap();
    boolean_union(k, a, c).unwrap()
}

/// `b`'s face, edge and vertex counts.
fn counts(b: &Brep) -> (usize, usize, usize) {
    (
        get_faces(b).unwrap().len(),
        get_edges(b).unwrap().len(),
        solidcore::topology::brep::p1::vertices(b, None)
            .unwrap()
            .len(),
    )
}

/// The L's swept inner corner, as [`convex_rounds_turn_round_a_concave_side_edge_as_a_horn_torus`]
/// derives it: one horn torus (1, 1) centred on the side edge x = y = 10 at the arcs' height,
/// (10, 10, 4), its axis along z, and its surface its own definition (the swept corner's
/// recertified distance to it 0).
fn assert_l_inner_torus(k: &Kernel, b: &Brep) {
    assert_eq!(tori(b), [(1.0, 1.0)]);
    let place = get_faces(b)
        .unwrap()
        .into_iter()
        .find_map(|f| match face2::surface(b, f).unwrap()?.kind() {
            SurfaceKind::Torus(t) => Some(*t.base().position()),
            _ => None,
        })
        .unwrap();
    let (o, z) = (place.origin(), place.z_axis());
    assert!(
        (o.x - 10.0).abs() <= 1e-12 && (o.y - 10.0).abs() <= 1e-12 && (o.z - 4.0).abs() <= 1e-12,
        "{o:?}"
    );
    assert!(z.x.abs() <= 1e-12 && z.y.abs() <= 1e-12, "{z:?}");
    let swept = corner_faces(b, CornerKind::Swept);
    assert_eq!(swept.len(), 1);
    assert_eq!(
        swept[0].procedural_blend().unwrap().recertify(k).unwrap(),
        0.0
    );
}

/// The mitre at a convex corner of two r 1 rounds on the top face z = `top`, whose side edge is
/// the upright line x = `side[0]`, y = `side[1]`, the material on the `inward` side of it (each
/// component +1 or -1). The round along x runs on the cylinder of axis y = side[1] +
/// inward[1], z = top - 1, the round along y on that of axis x = side[0] + inward[0],
/// z = top - 1. The mitre is their crossing: it runs from the side edge at (side, top - 1),
/// where both rounds meet their walls, to (side + inward, top), where both meet the top face,
/// and every point of it is 1 from both axes and on the bisector plane |x - side[0]| =
/// |y - side[1]|. Returns the largest of those distances' errors over 33 points.
fn mitre_off(k: &Kernel, b: &Brep, side: [f64; 2], inward: [f64; 2], top: f64) -> f64 {
    use solidcore::topology::vertex;
    let foot = Point3::new(side[0], side[1], top - 1.0);
    let head = Point3::new(side[0] + inward[0], side[1] + inward[1], top);
    let at = |v: Option<solidcore::topology::VertexId>, p: Point3| {
        v.is_some_and(|v| vertex::point(b, v).unwrap().distance(p) <= 1e-9)
    };
    let mitre = get_edges(b)
        .unwrap()
        .into_iter()
        .find(|&e| {
            let (s, t) = (
                edge::start_vertex(b, e).unwrap(),
                edge::end_vertex(b, e).unwrap(),
            );
            (at(s, foot) && at(t, head)) || (at(s, head) && at(t, foot))
        })
        .expect("the mitre runs from the side edge to the top face");
    let c = edge::curve(b, mitre).unwrap().unwrap();
    let ivl = edge::interval(b, mitre).unwrap();
    let (ya, xa, za) = (side[1] + inward[1], side[0] + inward[0], top - 1.0);
    let mut worst: f64 = 0.0;
    for i in 0..=32 {
        let p = c
            .evaluate_point(k, ivl.evaluate(f64::from(i) / 32.0))
            .unwrap();
        let along_x = (det::hypot(p.y - ya, p.z - za) - 1.0).abs();
        let along_y = (det::hypot(p.x - xa, p.z - za) - 1.0).abs();
        let bisector = ((p.x - side[0]).abs() - (p.y - side[1]).abs()).abs();
        worst = worst.max(along_x).max(along_y).max(bisector);
    }
    worst
}

/// Auto turns are swept where the swept turn builds: the L's inner corner is the same horn
/// torus, at the same closed-form volume, as with every turn swept.
#[test]
fn auto_turns_are_swept_where_the_swept_turn_builds() {
    let mut k = all_fixes();
    let mut b = l_prism(&mut k);
    round_turning(
        &mut k,
        &mut b,
        &[[15.0, 10.0, 5.0], [10.0, 15.0, 5.0]],
        &[1.0; 2],
        TurnChoice::Auto,
    )
    .unwrap();
    let (area, moment) = spandrel();
    let pi = std::f64::consts::PI;
    let expect = 1500.0 - 20.0 * area - moment * pi / 2.0;
    let v = volume(&k, &mut b);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert!((v - expect).abs() <= 1e-9 * expect, "{v} against {expect}");
    // the body every turn swept builds: its topology and torus as derived there
    assert_eq!(counts(&b), (11, 25, 16));
    assert_l_inner_torus(&k, &b);
}

/// Auto turns are mitred where the swept turn does not build, never refused: two convex rounds
/// at a box's convex corner, which the swept turn refuses, meet in the mitre. Closed form: each
/// round runs on to the bisector plane through the side edge, so each straight 10 mm round
/// gives back the part of its spandrel prism past that plane, the spandrel's first moment about
/// the other wall, r^3 (10 - 3 pi) / 12; the box is 1000.
///
/// Topology: the box's 6 faces, 12 edges and 8 vertices. The two rounds add 2 faces. The
/// rounded corner (0, 0, 10) becomes the mitre's two ends, (0, 0, 9) on the side edge and
/// (1, 1, 10) on the top face, and each round's far end, (10, 0, 10) and (0, 10, 10), becomes
/// two (the end face's notch, an arc from the top face down to the wall): 8 - 3 + 2 + 2 + 2 =
/// 11 vertices. Edges: the 12 less the 2 rounded, plus each round's tangent lines on the top
/// face and on its wall (4), the two end notches' arcs and the mitre: 17. Euler: 11 - 17 + 8 = 2.
#[test]
fn auto_turns_are_mitred_where_the_swept_turn_is_refused() {
    let at = [[5.0, 0.0, 10.0], [0.0, 5.0, 10.0]];
    let mut k = all_fixes();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    round_turning(&mut k, &mut b, &at, &[1.0; 2], TurnChoice::Auto).unwrap();
    let (area, moment) = spandrel();
    let expect = 1000.0 - 20.0 * area + 2.0 * moment;
    let v = volume(&k, &mut b);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    eprintln!("auto box corner: volume {v:.12} (closed form {expect:.12})");
    assert!(lines.is_empty(), "{lines:?}");
    assert!(tori(&b).is_empty());
    assert!((v - expect).abs() <= 1e-9 * expect, "{v} against {expect}");
    assert_eq!(counts(&b), (8, 17, 11));
    let off = mitre_off(&k, &b, [0.0, 0.0], [1.0, 1.0], 10.0);
    eprintln!("auto box corner: mitre off its closed form by {off:e}");
    assert!(off <= 1e-9, "{off:e}");
    // the mitre is the one every turn mitred builds
    let mut k = all_fixes();
    let mut m = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    round_turning(&mut k, &mut m, &at, &[1.0; 2], TurnChoice::Mitre).unwrap();
    assert!((volume(&k, &mut m) - v).abs() <= 1e-9 * expect);
}

/// One fillet, two kinds of turn: the L's two inner top edges and the outer wall x = 20's,
/// r 1. Its inner corner is a swept turn (convex rounds at a concave side edge) and the corner
/// at (20, 10) is not (convex rounds at a convex side edge): with every turn swept the fillet is
/// refused there; with Auto each corner is decided alone, the inner one swept and the outer
/// one mitred. Closed form: 1500, less three straight rounds' spandrels, less the swept
/// corner's revolved spandrel, plus the two moments the mitre gives back.
///
/// Topology: the swept L's 11 faces, 25 edges and 16 vertices, as
/// [`convex_rounds_turn_round_a_concave_side_edge_as_a_horn_torus`] derives them, and the round
/// along x = 20 adds a face. At (20, 10, 5) the swept L's two vertices (its notch arc's ends on
/// the end face x = 20, (20, 9, 5) and (20, 10, 4)) become the mitre's two, (19, 9, 5) on the
/// top face and (20, 10, 4) on the side edge, and the notch arc the mitre: no change. The new
/// round's far
/// end (20, 0, 5) becomes two (+1 vertex), and the round replaces its edge with its tangent
/// lines on the top face and on the wall and the end notch's arc on y = 0 (+2 edges): 12 faces,
/// 27 edges and 17 vertices. Euler: 17 - 27 + 12 = 2.
#[test]
fn auto_decides_each_turn_of_one_fillet_on_its_own() {
    let at = [[15.0, 10.0, 5.0], [10.0, 15.0, 5.0], [20.0, 5.0, 5.0]];
    let mut k = all_fixes();
    let mut b = l_prism(&mut k);
    let (code, _) = round(&mut k, &mut b, &at, &[1.0; 3]).unwrap_err();
    assert_eq!(code, FilletErrorType::NoRoom as i32);
    let mut k = all_fixes();
    let mut b = l_prism(&mut k);
    round_turning(&mut k, &mut b, &at, &[1.0; 3], TurnChoice::Auto).unwrap();
    let (area, moment) = spandrel();
    let pi = std::f64::consts::PI;
    let expect = 1500.0 - 30.0 * area - moment * pi / 2.0 + 2.0 * moment;
    let v = volume(&k, &mut b);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    eprintln!("auto L: volume {v:.12} (closed form {expect:.12})");
    assert!(lines.is_empty(), "{lines:?}");
    assert!((v - expect).abs() <= 1e-9 * expect, "{v} against {expect}");
    assert_eq!(counts(&b), (12, 27, 17));
    assert_l_inner_torus(&k, &b);
    let off = mitre_off(&k, &b, [20.0, 10.0], [-1.0, -1.0], 5.0);
    eprintln!("auto L: mitre off its closed form by {off:e}");
    assert!(off <= 1e-9, "{off:e}");
}

/// Unequal rounds at a sharp side edge the ball cannot pass are refused as not swept yet, not
/// as wanting the side edge rounded first: no one radius would let both balls pass, and the
/// refusal is the same whichever round comes first. The box's convex rounds at its convex
/// corner edge, r 1 and 2 in both orders, and the pocket rim's concave rounds at its concave
/// corner edges, r 1 and 1.5.
#[test]
fn unequal_rounds_at_a_side_edge_the_ball_cannot_pass_are_not_swept_yet() {
    for radii in [[1.0, 2.0], [2.0, 1.0]] {
        let mut k = all_fixes();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
        let (code, why) = round(
            &mut k,
            &mut b,
            &[[5.0, 0.0, 10.0], [0.0, 5.0, 10.0]],
            &radii,
        )
        .unwrap_err();
        assert_eq!(
            code,
            FilletErrorType::CornerUnsupported as i32,
            "{radii:?}: {why}"
        );
        assert!(
            why.contains("constant rounds of one radius"),
            "{radii:?}: {why}"
        );
    }
    let mut k = all_fixes();
    let mut b = pocket(&mut k);
    let (code, why) = round(&mut k, &mut b, &RIM, &[1.0, 1.5, 1.0, 1.5]).unwrap_err();
    assert_eq!(code, FilletErrorType::CornerUnsupported as i32, "{why}");
    assert!(why.contains("constant rounds of one radius"), "{why}");
}

#[test]
fn unequal_rounds_are_not_swept_yet() {
    let mut k = all_fixes();
    let mut b = boss_base(&mut k);
    let (code, why) = round(&mut k, &mut b, &BASE, &[1.0, 1.5, 1.0, 1.5]).unwrap_err();
    // the fork's SM_FILERR_CORNER_UNSUPPORTED (10 there: this port has no 9 before it)
    assert_eq!(code, FilletErrorType::CornerUnsupported as i32);
    assert!(why.contains("constant rounds of one radius"), "{why}");
}

/// The swept boss base's torus faces turned round (`SmFace::SwapUV`, which keeps each
/// torus's definition, WR-701) `times` times: the volume, check lines and tori after, and
/// each torus face's surface then rebuilt from its NURB (`Surface::reverse` along u runs
/// the torus's `RebuildSTEPFromNURBParameters` on the swapped net): its radii.
type Turned = (f64, Vec<String>, Vec<(f64, f64)>, Vec<(f64, f64)>);

fn turned_round(times: usize) -> Turned {
    let mut k = all_fixes();
    let mut b = boss_base(&mut k);
    round(&mut k, &mut b, &BASE, &[1.0; 4]).unwrap();
    let torus_faces: Vec<_> = get_faces(&b)
        .unwrap()
        .into_iter()
        .filter(|&f| {
            face2::surface(&b, f)
                .unwrap()
                .is_some_and(|s| matches!(s.kind(), SurfaceKind::Torus(_)))
        })
        .collect();
    assert_eq!(torus_faces.len(), 4);
    for _ in 0..times {
        for &f in &torus_faces {
            solidcore::topology::face::p1::swap_uv(&k, &mut b, f).unwrap();
        }
    }
    let rebuilt = torus_faces
        .iter()
        .map(|&f| {
            let mut s = face2::surface(&b, f).unwrap().unwrap().clone();
            s.reverse(&k, SurfParamType::U).unwrap();
            let SurfaceKind::Torus(t) = s.kind() else {
                panic!("rebuilt as {:?}", s.kind());
            };
            (t.major_radius(), t.minor_radius())
        })
        .collect();
    let v = volume(&k, &mut b);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    (v, lines, tori(&b), rebuilt)
}

/// The fork's af7a4d9 (`turn swept_tori_turned_round`): a horn torus turned round stays
/// the horn torus it was, its faces' edges on it, and rebuilt from its swapped NURB it is
/// the horn torus again.
#[test]
fn horn_tori_turned_round_stay_horn_tori() {
    for times in [1, 2] {
        let (v, lines, tori, rebuilt) = turned_round(times);
        eprintln!("turned {times}: volume {v:.12}, tori {tori:?}, rebuilt {rebuilt:?}, {lines:?}");
        assert!(lines.is_empty(), "{lines:?}");
        for (major, minor) in tori.into_iter().chain(rebuilt) {
            assert!((major - minor).abs() <= 0.0, "{major} {minor}");
        }
        assert!(
            (v - FORK_SWEPT_VOLUME).abs() <= 1e-9 * FORK_SWEPT_VOLUME,
            "{v}"
        );
    }
}

// ---- #2488: a Swept turn's torus keeps its definition ----

use solidcore::surface::procedural_blend::convert::{corner_rebuild, corner_to_record};
use solidcore::surface::procedural_blend::corner::CornerKind;
use solidcore::surface::procedural_blend::record::{CornerRecord, SupportRecord};
use solidcore::surface::procedural_blend::BlendNet;
use solidcore::surface::surface::Surface;

/// The swept boss base.
fn swept_boss_base() -> (Kernel, Brep) {
    let mut k = all_fixes();
    let mut b = boss_base(&mut k);
    round(&mut k, &mut b, &BASE, &[1.0; 4]).unwrap();
    (k, b)
}

/// The faces' surfaces whose definition is a corner's of `kind`.
fn corner_faces(b: &Brep, kind: CornerKind) -> Vec<Surface> {
    get_faces(b)
        .unwrap()
        .into_iter()
        .filter_map(|f| face2::surface(b, f).unwrap().cloned())
        .filter(|s| {
            s.procedural_blend()
                .and_then(|d| d.corner())
                .is_some_and(|c| c.kind() == kind)
        })
        .collect()
}

/// `rec` with its first NURBS support's form moved by `by` along x, y and z.
fn moved(rec: &SupportRecord, by: f64) -> SupportRecord {
    let mut rec = rec.clone();
    if let SupportRecord::Nurbs { form, .. } = &mut rec {
        let w = form.weights.clone();
        for (i, p) in form.points.iter_mut().enumerate() {
            let wi = w.as_ref().map_or(1.0, |w| w[i]);
            for x in p.iter_mut() {
                *x += by * wi;
            }
        }
    }
    rec
}

#[test]
fn the_swept_tori_are_their_definitions() {
    let (k, b) = swept_boss_base();
    let tori = corner_faces(&b, CornerKind::Swept);
    assert_eq!(tori.len(), 4);
    for s in &tori {
        assert!(matches!(s.kind(), SurfaceKind::Torus(_)));
        let d = s.procedural_blend().unwrap();
        assert!(matches!(d.net(), BlendNet::Exact(_)));
        assert_eq!(d.bound(), 0.0);
        assert_eq!(d.order(), 1);
        assert!(d.support(0).is_some());
        assert_eq!(d.recertify(&k).unwrap(), 0.0);
    }
}

#[test]
fn a_swept_torus_round_trips_through_its_record() {
    let (k, b) = swept_boss_base();
    for s in corner_faces(&b, CornerKind::Swept) {
        let rec = corner_to_record(&k, &s).unwrap();
        let json = serde_json::to_string(&rec).unwrap();
        let back = corner_rebuild(&k, &serde_json::from_str(&json).unwrap()).unwrap();
        assert!(matches!(back.kind(), SurfaceKind::Torus(_)));
        assert_eq!(corner_to_record(&k, &back).unwrap(), rec);
        assert_eq!(back.procedural_blend().unwrap().recertify(&k).unwrap(), 0.0);
    }
}

#[test]
fn a_swept_torus_off_its_common_face_or_its_axis_is_refused() {
    let (k, b) = swept_boss_base();
    let s = &corner_faces(&b, CornerKind::Swept)[0];
    let CornerRecord::Swept(rec) = corner_to_record(&k, s).unwrap() else {
        panic!("not a swept record");
    };
    // the common face moved apart from the torus
    let mut off = rec.clone();
    off.common = moved(&rec.common, 1e-3);
    let why = corner_rebuild(&k, &CornerRecord::Swept(off))
        .unwrap_err()
        .to_string();
    assert!(why.contains("do not touch its common face"), "{why}");
    // the side edge moved: the torus is not its end section's about it
    let mut off = rec.clone();
    off.axis_point[0] += 1e-3;
    let why = corner_rebuild(&k, &CornerRecord::Swept(off))
        .unwrap_err()
        .to_string();
    assert!(why.contains("side edge"), "{why}");
    // the net moved off its definition
    let mut off = rec;
    off.net = moved(&off.net, 1e-3);
    let why = corner_rebuild(&k, &CornerRecord::Swept(off))
        .unwrap_err()
        .to_string();
    assert!(why.contains("torus"), "{why}");
}

#[test]
fn a_moved_swept_torus_is_certified_again() {
    let (k, b) = swept_boss_base();
    for s in corner_faces(&b, CornerKind::Swept) {
        let mut s = s.copy(&k).unwrap();
        let mut place = solidcore::geom::axis2_placement::Axis2Placement::default();
        place.rotate_about_axis(
            0.3,
            solidcore::math::Vec3::new(1.0, 2.0, 3.0).unitize().unwrap(),
        );
        place.translate(solidcore::math::Vec3::new(1e2, -2e2, 5e1));
        s.transform(&k, &place, None).unwrap();
        assert_eq!(s.procedural_blend().unwrap().recertify(&k).unwrap(), 0.0);
    }
}
