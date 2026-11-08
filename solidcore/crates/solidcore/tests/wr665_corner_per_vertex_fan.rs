//! WR-665 (`fillet_corner_per_vertex_fan`; the fork's tangency-labs/smlib `4ecfbab` and
//! `e418750`): a vertex where the body touches itself is split into one vertex per fan of
//! faces before the blends, so that each fan is a corner of its own.
//!
//! The part is the fork's up-down joined squares: four 10 x 10 squares drawn edge to
//! edge round the origin, the up halves (z 0..10) missing the quadrant x, y > 0 and the
//! down halves (z -10..0) the quadrant x, y < 0. Built here as the cube [-10, 10]^3 less
//! the octants [0, 10]^3 and [-10, 0]^3, the two missing octants touching at the origin
//! only. The origin is a vertex of six edges in two fans of three.
//!
//! - `4ecfbab` (`SmBrep::SplitVertexByFans` / `JoinVertexFans`): the origin splits into
//!   two vertices of three edges each, a closed solid of the same volume (6000), and
//!   joins back as it was; a box's corner stays one vertex.
//! - `e418750`: chamfering the two concave edges on the line x = y = 0 (one in each fan)
//!   with leg 1 fails without the flag (`SmFilletCorner::Create` walks one fan's faces,
//!   finds the blended edges of both and builds an Nx2 corner of edges that share no
//!   face: status 1001); with it each chamfer ends on its own fan's end face and the
//!   volume is the fork's `6000 + 10` (each chamfer of a 10 long right-angled concave
//!   edge adds a triangle of legs 1, area 1/2, along 10).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_corner::p3::FilletCornerKind;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p5::is_knife_edge;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Vec3;
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::primitive::api_primitives::create_box;
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::vertex_fans::{join_vertex_fans, split_vertex_by_fans};
use solidcore::topology::{edge, vertex, EdgeId, VertexId};
use solidcore::{Brep, Kernel, Point3};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    b.refresh_cached_topology();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The cube [-10, 10]^3 less the octants [0, 10]^3 and [-10, 0]^3.
fn up_down_squares(k: &mut Kernel) -> Brep {
    let p = Point3::new;
    let cube = create_box(k, p(-10.0, -10.0, -10.0), 20.0, 20.0, 20.0).unwrap();
    let up = create_box(k, p(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let down = create_box(k, p(-10.0, -10.0, -10.0), 10.0, 10.0, 10.0).unwrap();
    let once = boolean_difference(k, cube, up).unwrap();
    boolean_difference(k, once, down).unwrap()
}

fn vertices_at(b: &Brep, at: Point3) -> Vec<VertexId> {
    b.vertices()
        .ids()
        .filter(|&v| vertex::point(b, v).unwrap().distance(at) < 1e-9)
        .collect()
}

fn ends(b: &Brep, e: EdgeId) -> (Point3, Point3) {
    let (s, t) = edge::start_end_vertices(b, e).unwrap();
    (
        vertex::point(b, s.unwrap()).unwrap(),
        vertex::point(b, t.unwrap()).unwrap(),
    )
}

/// The two edges on the line x = y = 0 (z 0..10 and z -10..0).
fn axis_edges(b: &Brep) -> Vec<EdgeId> {
    let on_axis = |q: Point3| q.x.abs() < 1e-9 && q.y.abs() < 1e-9;
    let mut out: Vec<EdgeId> = b
        .edges()
        .ids()
        .filter(|&e| {
            let (s, t) = ends(b, e);
            on_axis(s) && on_axis(t) && s.distance(t) > 1e-9
        })
        .collect();
    out.sort();
    out
}

/// `4ecfbab`: the origin of the up-down squares splits into two vertices of three edges,
/// the same closed solid, and joins back; a box's corner is one fan and stays one vertex.
#[test]
fn fork_4ecfbab_split_vertex_by_fans_and_join() {
    let mut k = Kernel::default();
    let mut b = up_down_squares(&mut k);
    let before = volume(&k, &mut b);
    assert!((before - 6000.0).abs() < 1e-9 * 6000.0, "{before}");
    let origin = vertices_at(&b, Point3::ZERO);
    assert_eq!(origin.len(), 1, "one vertex at the origin before the split");
    let v = origin[0];
    assert_eq!(vertex::edges(&b, v, None).unwrap().len(), 6);
    let count = b.vertices().ids().count();

    let made = split_vertex_by_fans(&mut b, v).unwrap();
    assert_eq!(made.len(), 1, "two fans: one new vertex");
    let w = made[0];
    assert_eq!(vertex::point(&b, w).unwrap(), vertex::point(&b, v).unwrap());
    assert_eq!(vertex::edges(&b, v, None).unwrap().len(), 3);
    assert_eq!(vertex::edges(&b, w, None).unwrap().len(), 3);
    assert_eq!(b.vertices().ids().count(), count + 1);
    let split = volume(&k, &mut b);
    assert!(
        (split - before).abs() < 1e-9 * before,
        "{split} vs {before}"
    );
    // one fan each: neither splits again
    assert!(split_vertex_by_fans(&mut b, v).unwrap().is_empty());
    assert!(split_vertex_by_fans(&mut b, w).unwrap().is_empty());

    join_vertex_fans(&mut b, v, w).unwrap();
    assert_eq!(b.vertices().ids().count(), count);
    assert_eq!(vertex::edges(&b, v, None).unwrap().len(), 6);
    let joined = volume(&k, &mut b);
    assert!(
        (joined - before).abs() < 1e-9 * before,
        "{joined} vs {before}"
    );

    // a box's corner: one fan
    let mut cube = create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0).unwrap();
    let corner = vertices_at(&cube, Point3::ZERO)[0];
    assert!(split_vertex_by_fans(&mut cube, corner).unwrap().is_empty());
    assert_eq!(vertex::edges(&cube, corner, None).unwrap().len(), 3);
}

/// `JoinVertexFans` refuses vertices that share an edge or lie apart.
#[test]
fn fork_4ecfbab_join_refuses_neighbours() {
    let mut k = Kernel::default();
    let mut cube = create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0).unwrap();
    let a = vertices_at(&cube, Point3::ZERO)[0];
    let c = vertices_at(&cube, Point3::new(1.0, 0.0, 0.0))[0];
    assert!(join_vertex_fans(&mut cube, a, c).is_err());
    assert!(join_vertex_fans(&mut cube, a, a).is_err());
}

/// Chamfers the axis edges `which` (of the two, sorted) with leg 1; `Err` is the fillet
/// error code. Also gives the number of vertices left at the origin.
fn chamfer_axis_edges(per_fan: bool, which: &[usize]) -> Result<(f64, usize), i32> {
    let mut k = Kernel::default();
    k.fixes.fillet_corner_per_vertex_fan = per_fan;
    let mut b = up_down_squares(&mut k);
    let all = axis_edges(&b);
    assert_eq!(all.len(), 2);
    let ids: Vec<EdgeId> = which.iter().map(|&i| all[i]).collect();
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_constant_radius_edges(&mut k, &ids, 1.0, 0, 1, 1.0)
        .unwrap();
    if s.do_fillet(&mut k).is_err() {
        return Err(s.error_info().code);
    }
    drop(s);
    let at_origin = vertices_at(&b, Point3::ZERO).len();
    Ok((volume(&k, &mut b), at_origin))
}

/// `e418750`: without the flag the chamfers fail at the origin's corner; with it each
/// ends on its own fan's end face, `V = 6000 + 10`.
#[test]
fn fork_e418750_each_fan_is_its_own_corner() {
    let off = chamfer_axis_edges(false, &[0, 1]);
    eprintln!("without the flag: {off:?}");
    assert_eq!(off, Err(3), "fails at the corner without the flag");
    let (v, at_origin) = chamfer_axis_edges(true, &[0, 1]).unwrap_or_else(|code| panic!("{code}"));
    eprintln!(
        "up-down squares, axis edges chamfered 1: {v:.12} ({at_origin} vertices at the origin)"
    );
    assert!((v - 6010.0).abs() < 1e-9 * 6010.0, "{v}");
}

/// One axis edge alone: one fan's corner is blended, the other fan's vertex stays at the
/// origin. Both with and without the flag (the flag joins what is left at the point).
#[test]
fn fork_e418750_one_fan_blended() {
    for which in [0, 1] {
        let off = chamfer_axis_edges(false, &[which]);
        let on = chamfer_axis_edges(true, &[which]);
        eprintln!("axis edge {which} alone: without the flag {off:?}, with it {on:?}");
        let (v, at_origin) = on.unwrap_or_else(|code| panic!("{code}"));
        assert!((v - 6005.0).abs() < 1e-9 * 6005.0, "{v}");
        assert!(at_origin <= 1, "{at_origin} vertices at the origin");
    }
}

/// A blend that fails after the split (leg 30 on 10 long edges): `DoFilleting` joins the
/// split vertices again whether the blends built or not, so the origin is one vertex of
/// six edges again.
#[test]
fn fork_e418750_failed_blend_joins_the_fans_again() {
    let mut k = Kernel::default();
    k.fixes.fillet_corner_per_vertex_fan = true;
    let mut b = up_down_squares(&mut k);
    let ids = axis_edges(&b);
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_constant_radius_edges(&mut k, &ids, 30.0, 0, 1, 1.0)
        .unwrap();
    let result = s.do_fillet(&mut k);
    eprintln!("leg 30: {result:?}, code {}", s.error_info().code);
    assert!(result.is_err());
    drop(s);
    let origin = vertices_at(&b, Point3::ZERO);
    assert_eq!(origin.len(), 1, "{origin:?}");
    assert_eq!(vertex::edges(&b, origin[0], None).unwrap().len(), 6);
}

/// WR-570's knife-edge part: a 62 1/6 x 21 plate whose corner is cut by a circle of
/// radius 21 about (TIP, -21), extruded 10; the arc wall meets the back face y = 0
/// tangentially along the vertical edge at x = TIP.
const TIP: f64 = 62.0 + 1.0 / 6.0;
const R: f64 = 21.0;
const H: f64 = 10.0;

fn knife_part(k: &mut Kernel) -> Brep {
    let line = |a: (f64, f64), b: (f64, f64)| {
        Curve::from(
            create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap(),
        )
    };
    let curves = [
        line((0.0, 0.0), (TIP, 0.0)),
        Curve::from(create_arc(Vec3::new(TIP, -R, 0.0), R, 90.0, 180.0).unwrap()),
        line((TIP - R, -R), (0.0, -R)),
        line((0.0, -R), (0.0, 0.0)),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_taper_extrude(k, &refs, H, 0.0, 3, false).unwrap()
}

/// The edges of `b` at one end vertex of the knife edge (x = TIP, y = 0): the knife edge
/// and the two cap edges, (knife, others).
fn tip_edges(b: &Brep) -> (EdgeId, Vec<EdgeId>) {
    let v: Vec<VertexId> = b
        .vertices()
        .ids()
        .filter(|&v| {
            let q = vertex::point(b, v).unwrap();
            (q.x - TIP).abs() < 1e-6 && q.y.abs() < 1e-6
        })
        .collect();
    assert_eq!(v.len(), 2);
    let mut knife = None;
    let mut others = Vec::new();
    for e in vertex::edges(b, v[0], None).unwrap() {
        let (s, t) = ends(b, e);
        if (s.z - t.z).abs() > H / 2.0 {
            knife = Some(e);
        } else {
            others.push(e);
        }
    }
    others.sort();
    (knife.unwrap(), others)
}

/// `e418750`'s `SmFilletSolver::IsKnifeEdge`: the tangent, folded-back edge is a knife
/// edge; the part's square edges and a box's edges are not.
#[test]
fn fork_e418750_is_knife_edge() {
    let mut k = Kernel::default();
    let mut b = knife_part(&mut k);
    let (knife, others) = tip_edges(&b);
    let tol = 10.0_f64.to_radians();
    assert!(is_knife_edge(&k, &mut b, knife, tol).unwrap());
    for e in others {
        assert!(!is_knife_edge(&k, &mut b, e, tol).unwrap());
    }
    let mut cube = create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0).unwrap();
    let edges: Vec<EdgeId> = cube.edges().ids().collect();
    for e in edges {
        assert!(!is_knife_edge(&k, &mut cube, e, tol).unwrap());
    }
}

/// `e418750`'s `SmFilletCorner::Create`: the two cap edges at the knife edge's end
/// vertex blended together make a two-edge corner there without the flag; with it no
/// corner is built at that vertex ("Corner Case Not Implemented").
#[test]
fn fork_e418750_no_two_edge_corner_at_a_knife_edge() {
    let kinds = |per_fan: bool| {
        let mut k = Kernel::default();
        k.fixes.fillet_corner_per_vertex_fan = per_fan;
        let mut b = knife_part(&mut k);
        let (knife, others) = tip_edges(&b);
        let tip = edge::start_end_vertices(&b, others[0]).unwrap();
        let (kv, kw) = edge::start_end_vertices(&b, knife).unwrap();
        let tip = [tip.0, tip.1]
            .into_iter()
            .flatten()
            .find(|&v| Some(v) == kv || Some(v) == kw)
            .unwrap();
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_constant_radius_edges(&mut k, &others, 1.0, 1, 1, 1.0)
            .unwrap();
        let _ = s.create_corners(&k, None);
        s.fillet_corners()
            .unwrap()
            .into_iter()
            .map(|id| s.corner(id).unwrap())
            .filter(|c| c.vertex == Some(tip))
            .map(|c| c.kind)
            .collect::<Vec<_>>()
    };
    let off = kinds(false);
    let on = kinds(true);
    eprintln!("corners at the knife edge's end: without the flag {off:?}, with it {on:?}");
    assert_eq!(off, vec![FilletCornerKind::NByTwo]);
    assert!(on.is_empty(), "{on:?}");
}
