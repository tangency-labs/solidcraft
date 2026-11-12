//! tangency-labs/solidcore#919 (WR-581): a curvature-continuous round on an R10 cylinder's
//! rim at the full width 10. SMLib's blend curve fails there (`SmApiFilletEdges` returns
//! `SM_ERR`, 1001, and leaves the cylinder as it was): the fillet's centre line is the single
//! point at the dome's centre on the axis, and `CalcTorusFilletGeom`, which builds the
//! circular round's hemisphere there, takes only circular sections, so the blend is marched
//! along a centre line of no length and `TrimRails` fails (the goldens' cases
//! `issue919-cylinder-rim-edge{1,2}-blend-curve-g{1,2}-r10.0`,
//! `fixtures/blend.fillet_executive_p3/issue_919_920_full_width.json`).
//!
//! The executive's curvature-continuous round is the G^n section on every kernel (WR-685;
//! its flag, `fillet_gn_section_generator`, is retired: WR-685 addendum 2026-11-06), and
//! WR-581's revolved blend curve went with the blend curve it revolved
//! (`fillet_blend_curve_revolved_on_arc`, WR-581 addendum 2026-11-07). The G^n section is
//! marched, so the full-width round is refused as the blend curve was, the cylinder as it
//! was; and it takes orders 2 to 4 only, so a G1 request is invalid input.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_section::section::GN_DEFAULT_THUMBWEIGHT;
use solidcore::error::{SM_ERR, SM_ERR_INVALID_INPUT};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::edge;
use solidcore::{Brep, Kernel, SolidError};

const R: f64 = 10.0;
const H: f64 = 20.0;

/// `SM_FSG_BLEND_CURVE`.
const BLEND_CURVE: u32 = 2;

/// The cylinder of radius 10 and height 20 with its top rim blended by `radius` at
/// `continuity` (1 G1, 2 G2) and the G^n section's default thumbweight: the API's result and
/// the body.
fn blended(radius: f64, continuity: u32) -> (Result<(), SolidError>, Brep) {
    let mut k = Kernel::default();
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), R, H).unwrap();
    let rim = *b
        .edge_list()
        .iter()
        .find(|&&e| {
            let c = edge::curve(&b, e).unwrap().unwrap();
            let ivl = edge::interval(&b, e).unwrap();
            let p = c.evaluate_point(&k, ivl.min()).unwrap();
            (p.z - H).abs() < 1e-6 && (p.x.hypot(p.y) - R).abs() < 1e-6
        })
        .unwrap();
    let result = fillet_edges(
        &mut k,
        &mut b,
        &[rim],
        radius,
        BLEND_CURVE,
        continuity,
        GN_DEFAULT_THUMBWEIGHT,
    );
    (result, b)
}

/// The kinds of `b`'s face surfaces, as names.
fn kinds(b: &Brep) -> Vec<&'static str> {
    let mut got: Vec<_> = b
        .face_list()
        .iter()
        .map(
            |&f| match b.face(f).unwrap().surface.as_ref().unwrap().kind() {
                SurfaceKind::Plane(_) => "plane",
                SurfaceKind::Cone(_) => "cylinder",
                _ => "other",
            },
        )
        .collect();
    got.sort_unstable();
    got
}

#[test]
fn issue_919_full_width_g2_round_is_refused() {
    let (result, b) = blended(R, 2);
    assert!(
        result
            .as_ref()
            .err()
            .and_then(solidcore::error::upstream_status)
            == Some(SM_ERR),
        "{result:?}"
    );
    // the cylinder as it was: its wall and two caps
    assert_eq!(kinds(&b), ["cylinder", "plane", "plane"]);
}

#[test]
fn issue_919_a_g1_round_is_not_a_gn_section() {
    for radius in [R, 9.99] {
        let (result, b) = blended(radius, 1);
        assert!(
            matches!(&result, Err(e) if solidcore::error::upstream_status(e) == Some(SM_ERR_INVALID_INPUT)),
            "r {radius}: {result:?}"
        );
        assert_eq!(kinds(&b), ["cylinder", "plane", "plane"], "r {radius}");
    }
}
