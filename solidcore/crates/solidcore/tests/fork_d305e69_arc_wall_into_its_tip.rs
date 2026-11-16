//! The SolidCraft fork's tangency-labs/smlib@d305e69 (#2273, smlib#79 step 5): the top edge
//! of #851's arc wall rounded and chamfered into the wall's tip, the knife edge where the
//! wall runs out tangent into the back face, builds (`BandCutsThePart`, aac4288, WR-696). Near the tip the part is thinner than the
//! blend's band, so the blend takes all of the part between its rails there and ends: the
//! part is not cut in two, the tip is blended away.
//!
//! The capi's `round_of_an_arc_wall_into_its_tip`, `chamfer_of_an_arc_wall_into_its_tip` and
//! `chamfers_of_an_arc_wall_and_the_top_front` (`ops_kernel.cpp` at d305e69, SolidCraft rig
//! fillets/851-fillet-arc-top-edge-9-5, 851-chamfer-arc-top-edge-9-5 and
//! 851-chamfer-arc-and-top-front-2), graded as the capi's `scenario_blend` grades them: built,
//! one solid, the volume to 1e-6 relative (the rig's analytic volumes), and here also
//! `check_brep` clean at level 2. The commit's fourth case, `chamfer_two_distances_past_the_top_refused`,
//! is the refusal of a two-distance chamfer that runs past a cylinder's top.
//!
//! How this test drives the kernel differs from the capi: the edges are picked as the
//! nearest within 0.1 of the capi's points, and the blend is `fillet_edges` with the
//! neighbouring step 5 tests' fixes (`wr696_band_cuts`, `wr696_fit_sizes`: every fix) and
//! their continuity 2.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_executive::{FitKind, NoRoomReason};
use solidcore::blend::fillet_solver::FilletSurfaceGenerator;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::brep::p1::regions;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edgeuse::p1 as eu;
use solidcore::topology::face::p2 as face;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel, SolidError};

/// Every fix.
fn kernel() -> Kernel {
    Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    }
}

fn pick(k: &Kernel, b: &Brep, q: Point3) -> EdgeId {
    b.edges()
        .ids()
        .filter_map(|e| {
            edge_closest_point(k, b, e, q)
                .ok()
                .map(|c| (e, c.closest.distance))
        })
        .filter(|&(_, d)| d <= 0.1)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("edge")
        .0
}

fn volume_of(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

fn lumps(b: &Brep) -> usize {
    let infinite = b.infinite_region();
    regions(b, None)
        .unwrap()
        .into_iter()
        .filter(|r| Some(*r) != infinite)
        .count()
}

/// The fork's `arc_wall_plate`: the plate x 0..41 1/6, y -21..0 and the corner
/// x 41 1/6..62 1/6 less the cylinder r21 about (62 1/6, -21), 10 high,
/// V0 = 10 (62 1/6 x 21 - pi 21^2 / 4). The wall runs into the back face y = 0 tangent at
/// the tip (62 1/6, 0, 10).
fn arc_wall_plate(k: &mut Kernel) -> Brep {
    let w = 41.0 + 1.0 / 6.0;
    let corner_box = create_box(k, Point3::new(w, -21.0, 0.0), 21.0, 21.0, 10.0).unwrap();
    let cyl = create_cylinder(k, Point3::new(w + 21.0, -21.0, 0.0), 21.0, 10.0).unwrap();
    let corner = boolean_difference(k, corner_box, cyl).unwrap();
    let plate = create_box(k, Point3::new(0.0, -21.0, 0.0), w, 21.0, 10.0).unwrap();
    boolean_union(k, plate, corner).unwrap()
}

/// The capi's point on the wall's top edge.
const ARC_TOP: Point3 = Point3::new(47.31742426174917, -6.150757595082503, 10.0);
/// The capi's point on the plate's top front edge (y = -21, z = 10).
const TOP_FRONT: Point3 = Point3::new(20.0, -21.0, 10.0);

/// The edges at `at` of the arc-wall plate blended by `r` (`xsect` 1 a round, 0 a
/// chamfer), graded: built, one solid, `check_brep` clean, of volume `want` to 1e-6.
fn assert_blend_builds(at: &[Point3], r: f64, xsect: u32, want: f64) {
    let mut k = kernel();
    let mut b = arc_wall_plate(&mut k);
    let edges: Vec<EdgeId> = at.iter().map(|&q| pick(&k, &b, q)).collect();
    fillet_edges(&mut k, &mut b, &edges, r, xsect, 2, 0.0).unwrap();
    assert_eq!(lumps(&b), 1, "one solid");
    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(check.lines().is_empty(), "check_brep: {:?}", check.lines());
    let v = volume_of(&k, &mut b);
    eprintln!(
        "volume {v:.12}, want {want:.12}, rel {:.3e}",
        (v - want).abs() / want
    );
    assert!(
        (v - want).abs() <= 1e-6 * want,
        "volume {v:.12}, want {want:.12}"
    );
}

/// Rounded R9.5, just under the wall's height. In (rho, z) about the wall's axis the round
/// takes the corner outside the circle about (21 + s, 10 - s), s = 9.5, swept from the front
/// face to the back face: at radius rho the part spans the angle asin(21 / rho), so the round
/// takes the integral over rho from 21 to 30.5 of rho asin(21 / rho) h(rho), where
/// h(rho) = s - sqrt(s^2 - (rho - 21 - s)^2): 525.584488, so V = 9065.80961148609.
#[test]
fn the_arc_walls_top_edge_rounded_into_its_tip_builds() {
    assert_blend_builds(&[ARC_TOP], 9.5, 1, 9065.80961148609);
}

/// Chamfered 9.5: the triangle (rho - 21) + (10 - z) <= 9.5 swept the same way,
/// V = 8410.449939154401.
#[test]
fn the_arc_walls_top_edge_chamfered_into_its_tip_builds() {
    assert_blend_builds(&[ARC_TOP], 9.5, 0, 8410.449939154401);
}

/// The wall's top edge and the plate's top front edge chamfered 2 together:
/// V = 9453.6359553261.
#[test]
fn the_arc_walls_top_edge_and_the_top_front_chamfered_together_build() {
    assert_blend_builds(&[ARC_TOP, TOP_FRONT], 2.0, 0, 9453.6359553261);
}

/// The capi's `chamfer_two_distances_past_the_top_refused` (SolidCraft rig
/// fillets/2893-chamfer-two-distances-past-the-top-refused): the top rim of a cylinder r10 x 20
/// chamfered 12 across the top and 3 down the wall. 12 across a top of radius 10 runs past
/// the axis, so no chamfer between the top and the wall has those sizes: refused for no room
/// with a reason, nothing built. The capi asserts `SMLIB_ERR_NO_ROOM` and a reason.
///
/// Here the chamfer is two offsets in the blend edge use's surface order, each the distance
/// of the chamfer's side on the other face from the edge: the wall's offset 12 (across the
/// top), the top's 3 (down the wall). The wall's offset of 12 inward folds past the axis
/// (`NoRoomReason::OffsetFolds`, the fork's `BallIsRegular`, 410dfe1): an offset below the
/// wall's radius 10 fits, a factor on both offsets of 10 / 12 less the fit margin (a thousand
/// edge tolerances of 1e-5, per unit of the larger offset).
#[test]
fn a_two_distance_chamfer_past_the_top_of_a_cylinder_is_refused() {
    let mut k = kernel();
    let mut b = create_cylinder(&mut k, Point3::ZERO, 10.0, 20.0).unwrap();
    let e = pick(&k, &b, Point3::new(0.0, 10.0, 20.0));
    let got = {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        let first = session.blend_edgeuse(&k, e).unwrap().unwrap();
        let f = eu::face(session.target(), first).unwrap().unwrap();
        let first_is_top = face::surface(session.target(), f)
            .unwrap()
            .is_some_and(|s| matches!(s.kind(), SurfaceKind::Plane(_)));
        let (d0, d1) = if first_is_top {
            (3.0, 12.0)
        } else {
            (12.0, 3.0)
        };
        session
            .prepare_per_edge_two_offsets(&k, &[e], &[d0], &[d1], &[FilletSurfaceGenerator::Linear])
            .unwrap();
        let got = session.do_fillet(&mut k);
        eprintln!("{got:?}: {}", session.error_info().comment);
        assert!(!session.error_info().comment.is_empty());
        got
    };
    let Err(SolidError::FilletRefused { reason, fit, .. }) = got else {
        panic!("not refused: {got:?}");
    };
    assert_eq!(reason, Some(NoRoomReason::OffsetFolds));
    assert_eq!(fit.kind, FitKind::Scale);
    assert_eq!(fit.edge, Some(e));
    let closed = (10.0 - 0.01) / 12.0;
    assert!(
        (fit.size - closed).abs() <= 1e-9 * closed,
        "fit {} vs closed form {closed}",
        fit.size
    );
    let v = volume_of(&k, &mut b);
    assert!(
        (v - 2000.0 * PI).abs() <= 1e-6 * 2000.0 * PI,
        "nothing built: volume {v:.12}"
    );
}
