//! Domes on planar faces (#19532) against their closed forms: a face raised into a smooth cap
//! of height `h` whose outline is the face's own edges and whose apex is `h` above the face's
//! centroid along the outward normal. The volume of a dome is `(2/3) A h` over the face's area
//! `A`, for every outline (each section at height `h sin t` is the outline scaled by `cos t`
//! about the centroid); a circle's elliptical dome is the half-ellipsoid, a circle's spherical
//! dome the spherical cap.
#![allow(
    clippy::disallowed_methods, // closed-form oracles, not ops geometry
)]

use std::f64::consts::{FRAC_PI_2, PI};

use solidcore::geom::extent3d::Extent3d;
use solidcore::kernel::Kernel;
use solidcore::query::api_queries::face_bounding_box;
use solidcore::topology::brep::dome::{DomeRefusal, DomeShape};

use crate::boolean::boolean;
use crate::dome::dome;
use crate::session;
use crate::sweep::{extrude, Curve, DraftCorner};
use crate::{make_box, make_cylinder, Body, BooleanOp, Error, Order};
use solidcore::math::det;

fn all_fixes() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

/// The record order of `b`: record face `i` is the kernel face `order.faces[i]`.
fn order(b: &Body) -> Order {
    b.to_record().unwrap().1
}

/// The record index (through `order`) of the face of `b` whose tight box `pick` accepts.
fn face(b: &Body, order: &Order, pick: impl Fn(&Extent3d) -> bool) -> usize {
    let k = Kernel::default();
    let brep = b.lock();
    order
        .faces
        .iter()
        .position(|&f| pick(&face_bounding_box(&k, &brep, f, true).unwrap()))
        .unwrap()
}

/// The flat face at height `z` (both box ends equal to `z`), by its record index.
fn flat_at(b: &Body, order: &Order, z: f64) -> usize {
    face(b, order, |bx| {
        (bx.min().z - z).abs() < 1e-9 && (bx.max().z - z).abs() < 1e-9
    })
}

fn volume(b: &Body) -> f64 {
    b.volume(1e-10).unwrap()
}

/// The highest point of `b` along z.
fn top_z(b: &Body) -> f64 {
    b.bounds().unwrap().1[2]
}

/// The straight edge from `a` to `b` at height `z`.
fn line(a: [f64; 2], b: [f64; 2], z: f64) -> Curve {
    Curve::Line {
        start: [a[0], a[1], z],
        end: [b[0], b[1], z],
    }
}

/// The closed polygon through `points` at height `z`, one line per side.
fn polygon(points: &[[f64; 2]], z: f64) -> Vec<Curve> {
    (0..points.len())
        .map(|i| line(points[i], points[(i + 1) % points.len()], z))
        .collect()
}

/// The circular arc of radius `r` about `centre` from angle `from` counter-clockwise by a right
/// angle, at height `z`.
fn quarter_arc(centre: [f64; 2], r: f64, from: f64, z: f64) -> Curve {
    Curve::Arc {
        center: [centre[0], centre[1], z],
        axis: [0.0, 0.0, 1.0],
        reference: [1.0, 0.0, 0.0],
        radius: r,
        start: from,
        end: from + FRAC_PI_2,
    }
}

/// The slot of two straight sides and two semicircular ends (each end two quarter arcs):
/// the rectangle between the end centres (±10, 0) plus two half discs of radius 5.
fn slot(z: f64) -> Vec<Curve> {
    let r = 5.0;
    vec![
        line([-10.0, -r], [10.0, -r], z),
        quarter_arc([10.0, 0.0], r, -FRAC_PI_2, z),
        quarter_arc([10.0, 0.0], r, 0.0, z),
        line([10.0, r], [-10.0, r], z),
        quarter_arc([-10.0, 0.0], r, FRAC_PI_2, z),
        quarter_arc([-10.0, 0.0], r, PI, z),
    ]
}

/// The regular pentagon of circumradius `R` about the origin, at height `z`.
fn pentagon(radius: f64, z: f64) -> Vec<Curve> {
    let corners: Vec<[f64; 2]> = (0..5)
        .map(|k| {
            let a = FRAC_PI_2 + 2.0 * PI * f64::from(k) / 5.0;
            [radius * det::cos(a), radius * det::sin(a)]
        })
        .collect();
    polygon(&corners, z)
}

/// The U-shaped outline of area 52 (a base 10 by 2 and two arms 2 wide), whose area centroid
/// lies in the notch between the arms, outside the material.
fn u_shape(z: f64) -> Vec<Curve> {
    polygon(
        &[
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [8.0, 10.0],
            [8.0, 2.0],
            [2.0, 2.0],
            [2.0, 10.0],
            [0.0, 10.0],
        ],
        z,
    )
}

/// The extruded prism of `profile` from height 0 to `height`.
fn prism(profile: &[Curve], height: f64) -> Body {
    extrude(
        profile,
        [0.0, 0.0, 1.0],
        height,
        0.0,
        DraftCorner::default(),
        true,
    )
    .unwrap()
}

#[test]
fn a_box_top_domed_ten_millimetres_is_exact_and_valid() {
    all_fixes();
    let b = make_box([0.0; 3], [30.0, 20.0, 10.0]).unwrap();
    let o = order(&b);
    let top = flat_at(&b, &o, 10.0);
    let domed = dome(&b, &o, top, 10.0, DomeShape::Elliptical, false).unwrap();
    // the block 6000 plus (2/3) * (30 * 20) * 10
    let exact = 6000.0 + 4000.0;
    assert!(
        (volume(&domed) - exact).abs() <= exact * 1e-9,
        "volume {:.12} vs {exact}",
        volume(&domed)
    );
    // the apex is exactly 10 above the top
    assert!(
        (top_z(&domed) - 20.0).abs() <= 1e-9,
        "top {}",
        top_z(&domed)
    );
    assert!(domed.is_manifold_solid().unwrap());
    domed.check_solid().unwrap();
}

#[test]
fn a_pentagon_top_domed_matches_two_thirds_a_h() {
    all_fixes();
    let b = prism(&pentagon(10.0, 0.0), 10.0);
    let o = order(&b);
    let top = flat_at(&b, &o, 10.0);
    let domed = dome(&b, &o, top, 10.0, DomeShape::Elliptical, false).unwrap();
    // a regular pentagon of circumradius R has area (5/2) R^2 sin 72 degrees
    let area = 2.5 * 100.0 * det::sin(2.0 * PI / 5.0);
    let exact = area * 10.0 + (2.0 / 3.0) * area * 10.0;
    assert!(
        (volume(&domed) - exact).abs() <= exact * 1e-9,
        "volume {:.12} vs {exact}",
        volume(&domed)
    );
    assert!((top_z(&domed) - 20.0).abs() <= 1e-9);
    assert!(domed.is_manifold_solid().unwrap());
    domed.check_solid().unwrap();
}

#[test]
fn a_slot_top_domed_matches_two_thirds_a_h() {
    all_fixes();
    let b = prism(&slot(0.0), 10.0);
    let o = order(&b);
    let top = flat_at(&b, &o, 10.0);
    let domed = dome(&b, &o, top, 4.0, DomeShape::Elliptical, false).unwrap();
    let area = 200.0 + 25.0 * PI;
    let exact = area * 10.0 + (2.0 / 3.0) * area * 4.0;
    assert!(
        (volume(&domed) - exact).abs() <= exact * 1e-9,
        "volume {:.12} vs {exact}",
        volume(&domed)
    );
    assert!((top_z(&domed) - 14.0).abs() <= 1e-9);
    assert!(domed.is_manifold_solid().unwrap());
    domed.check_solid().unwrap();
}

#[test]
fn a_circular_top_elliptical_is_the_half_ellipsoid() {
    all_fixes();
    let b = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
    let o = order(&b);
    let top = flat_at(&b, &o, 10.0);
    let domed = dome(&b, &o, top, 6.0, DomeShape::Elliptical, false).unwrap();
    // the cylinder 25 pi 10 plus the half ellipsoid (2/3) pi r^2 h
    let exact = 250.0 * PI + (2.0 / 3.0) * PI * 25.0 * 6.0;
    assert!(
        (volume(&domed) - exact).abs() <= exact * 1e-9,
        "volume {:.12} vs {exact}",
        volume(&domed)
    );
    assert!((top_z(&domed) - 16.0).abs() <= 1e-9);
    assert!(domed.is_manifold_solid().unwrap());
    domed.check_solid().unwrap();
}

#[test]
fn a_circular_top_spherical_is_the_spherical_cap() {
    all_fixes();
    let b = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
    let o = order(&b);
    let top = flat_at(&b, &o, 10.0);
    let domed = dome(&b, &o, top, 3.0, DomeShape::Spherical, false).unwrap();
    // the sphere radius R = (r^2 + h^2) / (2h); its cap has volume pi h^2 (3R - h) / 3
    let h = 3.0_f64;
    let big_r = (25.0 + h * h) / (2.0 * h);
    let cap = PI * h * h * (3.0 * big_r - h) / 3.0;
    let exact = 250.0 * PI + cap;
    assert!(
        (volume(&domed) - exact).abs() <= exact * 1e-9,
        "volume {:.12} vs {exact}",
        volume(&domed)
    );
    assert!((top_z(&domed) - 13.0).abs() <= 1e-9);
    assert!(domed.is_manifold_solid().unwrap());
    domed.check_solid().unwrap();
}

#[test]
fn a_dome_on_a_circle_keeps_the_face_count_and_adds_one_seam_edge() {
    all_fixes();
    let b = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
    let o = order(&b);
    let top = flat_at(&b, &o, 10.0);
    let before = b.counts().unwrap();
    let domed = dome(&b, &o, top, 6.0, DomeShape::Elliptical, false).unwrap();
    let after = domed.counts().unwrap();
    // an outline without corners is one smooth run: one cap face for the top face, its seam the
    // one edge added, the apex the one vertex
    assert_eq!(after[0], before[0], "faces {after:?}");
    assert_eq!(after[1], before[1] + 1, "edges {after:?}");
    assert_eq!(after[2], before[2] + 1, "vertices {after:?}");
}

#[test]
fn a_dome_on_a_box_top_has_a_face_per_side_and_an_edge_per_corner() {
    all_fixes();
    let b = make_box([0.0; 3], [30.0, 20.0, 10.0]).unwrap();
    let o = order(&b);
    let top = flat_at(&b, &o, 10.0);
    let before = b.counts().unwrap();
    let domed = dome(&b, &o, top, 10.0, DomeShape::Elliptical, false).unwrap();
    let after = domed.counts().unwrap();
    // a face's surface must be smooth across its knots, so the cap is one face per smooth run of
    // the outline: four for the top face, a meridian edge from each corner to the apex
    assert_eq!(after[0], before[0] + 3, "faces {after:?}");
    assert_eq!(after[1], before[1] + 4, "edges {after:?}");
    assert_eq!(after[2], before[2] + 1, "vertices {after:?}");
}

#[test]
fn a_dome_on_a_non_planar_face_is_refused() {
    all_fixes();
    let b = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
    let o = order(&b);
    let side = face(&b, &o, |bx| {
        (bx.max().z - bx.min().z - 10.0).abs() < 1e-9 && (bx.max().x - bx.min().x) > 9.0
    });
    let err = dome(&b, &o, side, 2.0, DomeShape::Elliptical, false).err();
    assert!(
        matches!(err, Some(Error::Dome(DomeRefusal::NotPlanar))),
        "{err:?}"
    );
}

#[test]
fn a_dome_on_a_face_with_a_hole_is_refused() {
    all_fixes();
    let block = make_box([0.0; 3], [30.0, 20.0, 10.0]).unwrap();
    let drill = make_cylinder([15.0, 10.0, -5.0], 3.0, 20.0).unwrap();
    let holed = boolean(block, drill, BooleanOp::Subtract).unwrap().unwrap();
    let o = order(&holed);
    let top = flat_at(&holed, &o, 10.0);
    let err = dome(&holed, &o, top, 4.0, DomeShape::Elliptical, false).err();
    assert!(
        matches!(err, Some(Error::Dome(DomeRefusal::Holes))),
        "{err:?}"
    );
}

#[test]
fn a_teardrop_top_domed_matches_two_thirds_a_h() {
    all_fixes();
    // a circle of radius 5 about the origin and the two tangents to it from (0, -10): the
    // tangent points are at -30 and 210 degrees, the tangents sqrt(75) long
    let r = 5.0_f64;
    let (from, to) = (-PI / 6.0, 7.0 * PI / 6.0);
    let start = [r * det::cos(from), r * det::sin(from)];
    let end = [r * det::cos(to), r * det::sin(to)];
    let profile = vec![
        line([0.0, -10.0], start, 0.0),
        Curve::Arc {
            center: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius: r,
            start: from,
            end: to,
        },
        line(end, [0.0, -10.0], 0.0),
    ];
    let b = prism(&profile, 10.0);
    let o = order(&b);
    let top = flat_at(&b, &o, 10.0);
    let domed = dome(&b, &o, top, 4.0, DomeShape::Elliptical, false).unwrap();
    // the kite of the two tangents and two radii, r sqrt(75), plus the 240 degree sector
    let area = r * 75.0_f64.sqrt() + (2.0 / 3.0) * PI * r * r;
    let exact = area * 10.0 + (2.0 / 3.0) * area * 4.0;
    assert!(
        (volume(&domed) - exact).abs() <= exact * 1e-9,
        "volume {:.12} vs {exact}",
        volume(&domed)
    );
    assert!((top_z(&domed) - 14.0).abs() <= 1e-9);
    assert!(domed.is_manifold_solid().unwrap());
    domed.check_solid().unwrap();
}

#[test]
fn a_dome_on_an_outline_the_centroid_does_not_see_round_is_refused() {
    all_fixes();
    let b = prism(&u_shape(0.0), 10.0);
    let o = order(&b);
    let top = flat_at(&b, &o, 10.0);
    let err = dome(&b, &o, top, 4.0, DomeShape::Elliptical, false).err();
    assert!(
        matches!(err, Some(Error::Dome(DomeRefusal::NotStar))),
        "{err:?}"
    );
}

#[test]
fn a_dome_height_that_is_not_positive_or_finite_is_refused() {
    all_fixes();
    let b = make_box([0.0; 3], [30.0, 20.0, 10.0]).unwrap();
    let o = order(&b);
    let top = flat_at(&b, &o, 10.0);
    for h in [0.0, -1.0] {
        let err = dome(&b, &o, top, h, DomeShape::Elliptical, false).err();
        assert!(
            matches!(err, Some(Error::Dome(DomeRefusal::NonPositiveHeight))),
            "h {h}: {err:?}"
        );
    }
    for h in [f64::NAN, f64::INFINITY] {
        let err = dome(&b, &o, top, h, DomeShape::Elliptical, false).err();
        assert!(
            matches!(err, Some(Error::Dome(DomeRefusal::NonFiniteHeight))),
            "h {h}: {err:?}"
        );
    }
}

#[test]
fn a_spherical_dome_needs_a_circular_face() {
    all_fixes();
    let b = make_box([0.0; 3], [30.0, 20.0, 10.0]).unwrap();
    let o = order(&b);
    let top = flat_at(&b, &o, 10.0);
    let err = dome(&b, &o, top, 4.0, DomeShape::Spherical, false).err();
    assert!(
        matches!(err, Some(Error::Dome(DomeRefusal::SphericalNeedsCircle))),
        "{err:?}"
    );
}

#[test]
fn a_reversed_dome_is_not_supported_yet() {
    all_fixes();
    let b = make_box([0.0; 3], [30.0, 20.0, 10.0]).unwrap();
    let o = order(&b);
    let top = flat_at(&b, &o, 10.0);
    let err = dome(&b, &o, top, 4.0, DomeShape::Elliptical, true).err();
    assert!(
        matches!(err, Some(Error::Dome(DomeRefusal::ReverseNotSupported))),
        "{err:?}"
    );
}
