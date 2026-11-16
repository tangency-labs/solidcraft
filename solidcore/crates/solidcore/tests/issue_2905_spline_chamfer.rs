//! tangency-labs/solidcore#2905: a tangent chain chamfered through a round's end arc, the
//! chain of a box's top edge, the arc and its vertical edge. On one offset it builds, its
//! volume in closed form. On unequal offsets the chamfers, each by its definition on its own
//! offsets, have no section that ends both at the smooth vertices: refused, naming why and
//! the offset that builds, rather than a bare face failure.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_executive::NoRoomReason;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::{Brep, EdgeId};
use solidcore::Kernel;

const R: f64 = 3.0;
const D: f64 = 0.473;

/// A refusal: its reason, and the size that builds and on which edge.
type Refused = (Option<NoRoomReason>, Option<f64>, Option<EdgeId>);

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
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

/// A kernel with the chamfer's definition on.
fn kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_round_as_definition = true;
    k
}

/// A 40 x 30 x 20 box with its top front edge rounded `R`, and its chain round the side face
/// x = 40: the top edge, the round's end arc, the vertical edge.
fn rounded(k: &mut Kernel) -> (Brep, [EdgeId; 3]) {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let top = nearest(k, &b, Point3::new(20.0, 0.0, 20.0));
    fillet_edges(k, &mut b, &[top], R, 1, 1, 1.0).unwrap();
    let c = 0.5 * std::f64::consts::SQRT_2 * R;
    let chain = [
        nearest(k, &b, Point3::new(40.0, 15.0, 20.0)),
        nearest(k, &b, Point3::new(40.0, R - c, 20.0 - R + c)),
        nearest(k, &b, Point3::new(40.0, 0.0, 8.0)),
    ];
    (b, chain)
}

fn chamfer(
    k: &mut Kernel,
    b: &mut Brep,
    chain: &[EdgeId; 3],
    offsets: [f64; 3],
) -> Result<(), Refused> {
    blend(k, b, chain, offsets, FilletSurfaceGeneratorType::Linear)
}

fn blend(
    k: &mut Kernel,
    b: &mut Brep,
    chain: &[EdgeId; 3],
    offsets: [f64; 3],
    section: FilletSurfaceGeneratorType,
) -> Result<(), Refused> {
    let mut s = FilletSession::new(k, b).unwrap();
    s.prepare_per_edge_constant_radius_with_second(
        k,
        chain,
        &offsets,
        &offsets,
        &[section],
        1,
        1.0,
    )
    .unwrap();
    s.create_corners(k, None).unwrap();
    if section == FilletSurfaceGeneratorType::Linear {
        for id in s.fillet_corners().unwrap() {
            s.set_corner_bevel(id, true).unwrap();
        }
    }
    if s.do_filleting(k, None).is_ok() {
        return Ok(());
    }
    let fit = s.fit_size();
    Err((
        s.error_info().no_room_reason(),
        fit.as_ref().map(|f| f.size),
        fit.and_then(|f| f.edge),
    ))
}

/// One offset on the chain: two prisms of the section `D^2 / 2`, along 27 and 17, and the
/// section revolved a quarter turn, its centroid at `R - D / 3` from the arc's axis.
#[test]
fn a_chain_on_one_offset_builds_its_closed_form() {
    let mut k = kernel();
    let (mut b, chain) = rounded(&mut k);
    let before = volume(&k, &mut b);
    chamfer(&mut k, &mut b, &chain, [D; 3]).unwrap();
    let section = 0.5 * D * D;
    let removed = section * (27.0 + 17.0) + 0.5 * PI * (R - D / 3.0) * section;
    let got = before - volume(&k, &mut b);
    assert!((got - removed).abs() < 1e-5, "{got} vs {removed}");
}

/// The middle edge on a wider offset: refused at the Join, the other offset naming the size
/// that builds on that edge. The refused chamfer's reason survives to the caller.
#[test]
fn a_chain_on_unequal_offsets_is_refused_by_name() {
    let mut k = kernel();
    let (mut b, chain) = rounded(&mut k);
    let wider = 0.5430636408071486;
    let (why, fit, edge) = chamfer(&mut k, &mut b, &chain, [D, wider, D]).unwrap_err();
    assert_eq!(why, Some(NoRoomReason::JoinOffsetsDiffer));
    assert_eq!(fit, Some(D));
    assert_eq!(edge, Some(chain[1]));
}

/// Rounds on unequal radii are the same refusal.
#[test]
fn a_round_chain_on_unequal_radii_is_refused_by_name() {
    let mut k = kernel();
    let (mut b, chain) = rounded(&mut k);
    let (why, fit, edge) = blend(
        &mut k,
        &mut b,
        &chain,
        [0.4, 0.6, 0.4],
        FilletSurfaceGeneratorType::Circular,
    )
    .unwrap_err();
    assert_eq!(why, Some(NoRoomReason::JoinOffsetsDiffer));
    assert_eq!(fit, Some(0.4));
    assert_eq!(edge, Some(chain[1]));
}

/// Offsets that differ by less than a loop closes are not this refusal's.
#[test]
fn offsets_within_the_loop_gap_are_not_refused_by_name() {
    let mut k = kernel();
    let (mut b, chain) = rounded(&mut k);
    let near = D + 0.5 * b.tolerance();
    if let Err((why, ..)) = chamfer(&mut k, &mut b, &chain, [D, near, D]) {
        assert_ne!(why, Some(NoRoomReason::JoinOffsetsDiffer));
    }
}
