//! Regression test for WR-571 (the fork's tangency-labs/smlib@183ea65, capi sweeps cases):
//! the curvature-keeping and tangent corner rules of `create_taper_extrude` refused a
//! draft in which a wall's section shrinks to nothing below the far end
//! (`TaperCornerRefusal::WallCloses`), though the solid exists. With
//! `Kernel::fixes.taper_wall_closes_at_apex` the wall ends at its apex and its
//! neighbours meet from there up.
//!
//! - A wedge (0,-2) (20,-0.01) (20,0.01) (0,2) drafted 5 degrees in over 2: the tip's
//!   wall closes at z* = 0.126237833 and the long sides meet along a ridge above. The
//!   section is a quadrilateral below z* and a triangle above, each area quadratic in z,
//!   so the volume is exact: 72.883284979193, 6 faces.
//! - A slot 0.2 wide, 8 deep in a 10 x 10 square, drafted 10 degrees out over 2: the
//!   slot's sides close together at z* = 0.1 / tan 10, face to face:
//!   V = ((10 + 4t)^3 - 1000) / (6t) - 0.08 / t = 213.984095421783 (t = tan 10),
//!   10 faces.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::sweep::api_brep::{
    create_taper_extrude_with_corner, OffsetCornerType, TaperCornerRefusal, TaperExtrude,
};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::Vec3;

fn p(x: f64, y: f64) -> Vec3 {
    Vec3::new(x, y, 0.0)
}

fn polygon(pts: &[Vec3]) -> Vec<Curve> {
    (0..pts.len())
        .map(|i| {
            Curve::from(
                Line::create_line_segment(3, pts[i], pts[(i + 1) % pts.len()], None).unwrap(),
            )
        })
        .collect()
}

fn wedge() -> Vec<Curve> {
    polygon(&[p(0.0, -2.0), p(20.0, -0.01), p(20.0, 0.01), p(0.0, 2.0)])
}

/// A 10 x 10 square with a slot 0.2 wide and 8 deep from its top side.
fn slotted_square() -> Vec<Curve> {
    polygon(&[
        p(0.0, 0.0),
        p(10.0, 0.0),
        p(10.0, 10.0),
        p(5.1, 10.0),
        p(5.1, 2.0),
        p(4.9, 2.0),
        p(4.9, 10.0),
        p(0.0, 10.0),
    ])
}

/// The draft of `curves` over a height of 2 (positive degrees inward).
fn draft(
    fixed: bool,
    tangent: bool,
    curves: &[Curve],
    draft_deg: f64,
    corner: OffsetCornerType,
) -> (Kernel, TaperExtrude) {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes.taper_wall_closes_at_apex = fixed;
    k.fixes.taper_tangent_corner_planes = tangent;
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let result =
        create_taper_extrude_with_corner(&mut k, &refs, 2.0, draft_deg, 3, corner).unwrap();
    (k, result)
}

/// A closed manifold solid: its face count and volume.
fn solid(k: &Kernel, result: TaperExtrude) -> (usize, f64) {
    let TaperExtrude::Solid(mut b) = result else {
        panic!("refused: {result:?}");
    };
    assert!(is_manifold_solid(&b).unwrap());
    let store = AttributeStore::default();
    let key_of = |_| ObjectKey(0);
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    (
        b.face_list().len(),
        brep_volume(k, &mut b, 1e-9, &attributes).unwrap(),
    )
}

fn assert_rel(a: f64, b: f64, rel: f64) {
    assert!((a - b).abs() <= rel * a.abs().max(b.abs()), "{a} vs {b}");
}

const WEDGE_VOLUME: f64 = 72.883_284_979_193;

fn slot_volume() -> f64 {
    let t = 10.0_f64.to_radians().tan();
    ((10.0 + 4.0 * t).powi(3) - 1000.0) / (6.0 * t) - 0.08 / t
}

#[test]
fn wr_571_without_the_opt_in_a_wall_closing_partway_is_refused() {
    for (curves, deg) in [(wedge(), 5.0), (slotted_square(), -10.0)] {
        let (_, result) = draft(false, false, &curves, deg, OffsetCornerType::CurveExtension);
        let TaperExtrude::Refused(refusal) = result else {
            panic!("built");
        };
        assert_eq!(refusal.reason, TaperCornerRefusal::WallCloses);
    }
}

#[test]
fn wr_571_the_wedge_tip_closes_into_a_ridge() {
    let (k, result) = draft(true, false, &wedge(), 5.0, OffsetCornerType::CurveExtension);
    let (faces, v) = solid(&k, result);
    assert_eq!(faces, 6);
    assert_rel(v, WEDGE_VOLUME, 1e-8);
    // the tangent rule (lines: the same walls)
    let (k, result) = draft(true, true, &wedge(), 5.0, OffsetCornerType::LinearExtension);
    let (faces, v) = solid(&k, result);
    assert_eq!(faces, 6);
    assert_rel(v, WEDGE_VOLUME, 1e-8);
}

#[test]
fn wr_571_the_slot_closes_face_to_face() {
    let (k, result) = draft(
        true,
        false,
        &slotted_square(),
        -10.0,
        OffsetCornerType::CurveExtension,
    );
    let (faces, v) = solid(&k, result);
    assert_eq!(faces, 10);
    assert_rel(v, slot_volume(), 1e-8);
}
