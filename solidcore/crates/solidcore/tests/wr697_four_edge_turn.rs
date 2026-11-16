//! WR-697 (the fork's smlib#113, e4083c8 and 34288e5): the blend corner's Turn at a vertex
//! of four edges is the two blends mitred, with no tangent adjustment.
//!
//! Two blends of one convexity on opposite edges of a four-edge vertex, not G1 there, are
//! a Turn (`SmBlendCorner::Classify`). The fork builds it with the two-blend topology,
//! which smlib#113 cut down to the mitre: one `FILLET_X_FILLET` edge between the rails'
//! crossings with the side edges. Its G1 test, its blending branch and its tangent
//! adjustment went, since `Classify` makes a pair that passes them a Join. The cases are
//! the apex of a square pyramid (convex) and the bottom of a pyramid-shaped pocket
//! (concave), two opposite slant edges rounded at r 0.5, 1 and 2. Each builds a valid body
//! with no corner face.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3, BIG_DOUBLE};
use solidcore::primitive::api_primitives::{create_box, create_pyramid};
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::topology::brep::mirror::mirror;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

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

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// A square pyramid on z = 0, 20 wide and 10 high, apex (0, 0, 10).
fn pyramid(k: &mut Kernel) -> Brep {
    create_pyramid(k, Point3::new(0.0, 0.0, 0.0), 20.0, 10.0).unwrap()
}

/// A 30 x 30 x 20 block (z -10..10) less that pyramid mirrored in z = 5: a pocket whose
/// square mouth is on the top face and whose four concave slant edges meet at (0, 0, 0).
fn pocket(k: &mut Kernel) -> Brep {
    let block = create_box(k, Point3::new(-15.0, -15.0, -10.0), 30.0, 30.0, 20.0).unwrap();
    let mut tool = pyramid(k);
    let plane = Axis2Placement::new(
        Point3::new(0.0, 0.0, 5.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    mirror(k, &mut tool, &plane).unwrap();
    boolean_difference(k, block, tool).unwrap()
}

/// The two opposite slant edges through (-5, -5, 5) and (5, 5, 5) rounded `r` together
/// (a Turn at the four-edge apex): the volume, face count and check lines.
fn rounded(body: fn(&mut Kernel) -> Brep, r: f64) -> (f64, usize, Vec<String>) {
    let mut k = all_fixes();
    let mut b = body(&mut k);
    let edges: Vec<EdgeId> = [[-5.0, -5.0, 5.0], [5.0, 5.0, 5.0]]
        .iter()
        .map(|&p| pick(&k, &b, p))
        .collect();
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(
            &mut k,
            &edges,
            &[r, r],
            &[FilletSurfaceGeneratorType::Circular; 2],
            1,
            1.0,
        )
        .unwrap();
        s.do_fillet(&mut k).unwrap();
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let v = volume(&k, &mut b);
    let faces = get_faces(&b).unwrap().len();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    (v, faces, lines)
}

fn four_edge_turns(name: &str, body: fn(&mut Kernel) -> Brep, faces: usize) {
    for r in [0.5, 1.0, 2.0] {
        let (turn, turn_faces, turn_lines) = rounded(body, r);
        eprintln!("{name} r {r}: volume {turn:?}, {turn_faces} faces");
        assert!(turn_lines.is_empty(), "{name} r {r}: {turn_lines:?}");
        // the body's faces and the two rounds, mitred: no corner face
        assert_eq!(turn_faces, faces, "{name} r {r}");
    }
}

#[test]
fn a_pyramid_apex_turn_of_four_edges_is_mitred() {
    four_edge_turns("pyramid", pyramid, 7);
}

#[test]
fn a_pyramid_pocket_floor_turn_of_four_edges_is_mitred() {
    four_edge_turns("pocket", pocket, 12);
}
