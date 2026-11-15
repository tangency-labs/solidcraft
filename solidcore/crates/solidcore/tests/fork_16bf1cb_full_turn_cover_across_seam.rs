//! WR-599 (the SolidCraft fork's tangency-labs/smlib@16bf1cb): the test-peg part, a bar
//! 40 x 16 x 10 with a half-round end of radius 8 and a cylinder r 8 x 10 standing on its
//! top about the half-round's axis, joined. The cylinder's wall is one full-turn face; the
//! bar's half-round wall lies on the same cylinder across that face's seam (the seam at
//! angle 0 runs down the middle of the half-round). Upstream `CoverCoincidentSurface`
//! refuses the cover as soon as its boundary walk steps across the seam, so the two walls
//! stay apart with their edge at z 10 between them (8 faces). With
//! `Kernel::fixes.cover_wraps_across_closed_seam` the crossing of a seam the surface
//! already wraps is no failure, the walls merge and `BooleanPostProcess` adds the missing
//! piece of the seam through the bar's part (`SplitAtSeam`): 7 faces.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::{edge, vertex};
use solidcore::{Brep, Kernel};

const R: f64 = 8.0;
/// The half-round's centre in x.
const CX: f64 = 32.0;

fn p(x: f64, y: f64) -> Point3 {
    Point3::new(x, y, 0.0)
}

fn line(a: Point3, b: Point3) -> Curve {
    Curve::from(Line::create_line_segment(3, a, b, None).unwrap())
}

/// The half-round end: the r 8 circle about (32, 0) in z = 0 from -90 to 90 degrees.
fn half_round(k: &Kernel) -> Curve {
    let mut frame = Axis2Placement::default();
    frame
        .set_canonical(
            p(CX, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        )
        .unwrap();
    let interval = Extent1d::from_min_max(-90.0, 90.0);
    Curve::from(Ellipse::create_canonical_circle(k, &frame, R, Some(&interval), None).unwrap())
}

/// The peg joined, its face count and volume.
fn peg(fix: bool) -> (Brep, usize, f64) {
    let mut k = Kernel::default();
    k.fixes.cover_wraps_across_closed_seam = fix;
    let a = line(p(0.0, -R), p(CX, -R));
    let b = half_round(&k);
    let c = line(p(CX, R), p(0.0, R));
    let d = line(p(0.0, R), p(0.0, -R));
    let bar = create_linear_sweep(
        &mut k,
        &[Some(&a), Some(&b), Some(&c), Some(&d)],
        Vec3::new(0.0, 0.0, 1.0),
        10.0,
        true,
    )
    .unwrap();
    let post = create_cylinder(&mut k, Point3::new(CX, 0.0, 10.0), R, 10.0).unwrap();
    let mut joined = boolean_union(&mut k, bar, post).unwrap();
    assert!(is_manifold_solid(&joined).unwrap());
    let faces = p1::faces(&joined, None).unwrap().len();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let volume = brep_volume(&k, &mut joined, 1e-6, &attrs).unwrap();
    (joined, faces, volume)
}

/// The bar (32 x 16 plus the half disc) times 10, and the post.
fn want_volume() -> f64 {
    let pi = core::f64::consts::PI;
    (CX * 2.0 * R + 0.5 * pi * R * R) * 10.0 + pi * R * R * 10.0
}

#[test]
fn wr599_default_keeps_the_walls_apart() {
    let (_, faces, volume) = peg(false);
    assert_eq!(
        faces, 8,
        "upstream leaves the half-round and post walls apart"
    );
    let want = want_volume();
    assert!((volume - want).abs() <= 1e-6 * want, "{volume} vs {want}");
}

/// Whether the body has a vertex at `at` (within 1e-9).
fn has_vertex_at(b: &Brep, at: Point3) -> bool {
    p1::vertices(b, None)
        .unwrap()
        .into_iter()
        .any(|v| vertex::point(b, v).unwrap().distance(at) <= 1e-9)
}

#[test]
fn wr599_full_turn_wall_takes_in_the_half_round() {
    let (joined, faces, volume) = peg(true);
    assert_eq!(faces, 7, "the post's wall takes in the bar's half-round");
    let want = want_volume();
    assert!((volume - want).abs() <= 1e-6 * want, "{volume} vs {want}");
    // The merged wall's seam (angle 0, x 40) runs on down through the bar's part
    // (`SplitAtSeam`): the bottom arc, which crossed it, is split there.
    assert!(
        has_vertex_at(&joined, Point3::new(CX + R, 0.0, 0.0)),
        "the seam splits the bottom arc at (40, 0, 0)"
    );
    let seams = p1::edges(&joined, None)
        .unwrap()
        .into_iter()
        .filter(|&e| edge::faces(&joined, e, None).unwrap().len() == 1)
        .count();
    assert!(seams >= 1, "the merged wall has a seam");
}

#[test]
fn wr599_default_has_no_seam_vertex_on_the_bar() {
    let (joined, _, _) = peg(false);
    assert!(!has_vertex_at(&joined, Point3::new(CX + R, 0.0, 0.0)));
}
