//! SolidCraft #11360's `a_round_ending_in_an_earlier_round_matches_both_together` (#2364's
//! review): the arc-wall plate (the plate x 0..41 1/6, y -21..0 and the corner x 41 1/6..62 1/6
//! less the cylinder r 21 about (62 1/6, -21), 10 high), a first edge rounded r 2, then the
//! upright at the knife-edge tip (62 1/6, 0) rounded r 2 on that result. The first edge is
//! the arc's top edge or the back top edge.
//!
//! The second round ends at the top of the upright, on the first round (an End on its end
//! face). Its rail on the side face runs past the far end P of the side edge, the first
//! round's boundary there. At P the first round runs on tangentially into the top face, so
//! the rail ends on the top face, and the global merge cuts the round where it meets the
//! first round and the top face. The fork's 41a39ec
//! (`RefuseEndPastSideEdge`, `FitOfEndPastSideEdges`) refused it with `SM_FILERR_NO_ROOM`,
//! naming 1.04762 (the arc) and 1.05 (the back edge). The fork at v1.1.0-alpha.1 refuses it
//! too, also with `AddEarlierPassFace`. Before #2364 it built through WR-303's global merge
//! to the volume of rounding both edges together, as it does again
//! (`end_face_runs_on_past_side_edge`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::EdgeId;
use solidcore::Brep;

const W: f64 = 41.0 + 1.0 / 6.0;
/// The capi's point on the arc wall's top edge.
const ARC_TOP: Point3 = Point3::new(47.31742426174917, -6.150757595082503, 10.0);
/// A point on the back top edge (y = 0, z = 10).
const BACK_TOP: Point3 = Point3::new(30.0, 0.0, 10.0);
/// A point on the upright at the knife-edge tip.
const TIP: Point3 = Point3::new(W + 21.0, 0.0, 5.0);

/// SolidCraft's rig set: every fix but the four it leaves off.
fn kernel() -> Kernel {
    let mut f = UpstreamFixes::all();
    f.cone_unbounded_parameterized_as_nurb = false;
    f.ellipse_open_arc_keeps_end_angle = false;
    f.fillet_overflow_crease_along_edge = false;
    f.fillet_refuse_rounds_breaking_through = false;
    Kernel {
        fixes: f,
        ..Kernel::default()
    }
}

fn arc_wall_plate(k: &mut Kernel) -> Brep {
    let corner_box = create_box(k, Point3::new(W, -21.0, 0.0), 21.0, 21.0, 10.0).unwrap();
    let cyl = create_cylinder(k, Point3::new(W + 21.0, -21.0, 0.0), 21.0, 10.0).unwrap();
    let corner = boolean_difference(k, corner_box, cyl).unwrap();
    let plate = create_box(k, Point3::new(0.0, -21.0, 0.0), W, 21.0, 10.0).unwrap();
    boolean_union(k, plate, corner).unwrap()
}

fn pick(k: &Kernel, b: &Brep, q: Point3) -> EdgeId {
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    assert!(best.0 <= 0.1, "no edge at {q:?}");
    best.1.unwrap()
}

fn round(k: &mut Kernel, b: &mut Brep, at: &[Point3], r: f64) {
    let edges: Vec<EdgeId> = at.iter().map(|&q| pick(k, b, q)).collect();
    fillet_edges(k, b, &edges, r, 1, 1, 0.0).unwrap_or_else(|e| panic!("{at:?} r {r}: {e}"));
}

/// The body's face count and volume, checked one manifold solid, `check_brep` clean.
fn measured(k: &mut Kernel, b: &mut Brep) -> (usize, f64) {
    assert!(is_manifold_solid(b).unwrap());
    let check = check_brep(k, b, CheckLevel::Level2).unwrap();
    assert!(check.lines().is_empty(), "check_brep: {:?}", check.lines());
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    (
        b.face_list().len(),
        brep_volume(k, b, 1e-8, &attrs).unwrap(),
    )
}

/// `first` rounded r 2, then the tip's upright r 2, against both rounded together.
fn rounds_in_turn_match_both_together(first: Point3, together_volume: f64, in_turn_volume: f64) {
    let mut k = kernel();
    let mut b = arc_wall_plate(&mut k);
    round(&mut k, &mut b, &[first, TIP], 2.0);
    let (faces, together) = measured(&mut k, &mut b);
    assert_eq!(faces, 8);
    assert!(
        (together - together_volume).abs() <= 1e-9 * together_volume,
        "{together:.12}"
    );

    let mut k = kernel();
    let mut b = arc_wall_plate(&mut k);
    round(&mut k, &mut b, &[first], 2.0);
    round(&mut k, &mut b, &[TIP], 2.0);
    let (faces, in_turn) = measured(&mut k, &mut b);
    assert_eq!(faces, 8);
    assert!(
        (in_turn - in_turn_volume).abs() <= 1e-9 * in_turn_volume,
        "{in_turn:.12}"
    );
    // two constructions of one body: the corner of both rounds, and the global merge's cut
    let rel = (in_turn - together).abs() / together;
    eprintln!("in turn {in_turn:.12}, together {together:.12}, rel {rel:.2e}");
    assert!(
        rel <= 1e-8,
        "in turn {in_turn:.12}, together {together:.12}"
    );
}

/// The arc's top edge first: 9452.927851913 in turn against 9452.927826170 together
/// (2.7e-9 relative).
#[test]
fn the_tip_rounded_after_the_arc_top_matches_both_together() {
    rounds_in_turn_match_both_together(ARC_TOP, 9452.927826170264, 9452.927851913100);
}

/// The back top edge first: 9428.557343079310 in turn against 9428.557343079308 together.
#[test]
fn the_tip_rounded_after_the_back_top_matches_both_together() {
    rounds_in_turn_match_both_together(BACK_TOP, 9428.557343079308, 9428.557343079310);
}
