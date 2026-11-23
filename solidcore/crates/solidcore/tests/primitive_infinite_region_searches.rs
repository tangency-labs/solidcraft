//! The searches a solid primitive runs for its infinite region, as a count, and the side
//! of the solid the region it ends with is on.
//!
//! A box, a cylinder or a cone is made face by face and stitched. The stitch's glue that
//! closes the shell splits the B-rep's one region in two, and has to say which of the two
//! is the infinite one. Upstream's fires a ray at the new shell to find out
//! (`FindInfiniteRegion`), and the stitch ends with the same search over the whole B-rep.
//! A box or a cylinder made alone in its B-rep is convex and knows its own outside, so its
//! closing glue takes the half its outside face use is in and fires no ray, where that ray
//! would leave nothing else in the body: every length beyond the stitch's reach (80 times
//! the tolerance), and a size the tolerance serves.
//! [`Kernel::work_counters`] counts each search (`infinite_region_searches`), the same on
//! every machine.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::brep_volume_default;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives::{create_box, create_cone, create_cylinder};
use solidcore::primitive::primitive_creation::p1::PrimitiveCreation;
use solidcore::topology::brep::{p1, p3};
use solidcore::topology::Brep;
use solidcore::{Kernel, Result};

fn kernel(solidcraft: bool) -> Kernel {
    let fixes = if solidcraft {
        UpstreamFixes::all()
    } else {
        UpstreamFixes::default()
    };
    Kernel {
        fixes,
        ..Kernel::default()
    }
}

/// Checks `b` is one solid with its outside as the infinite region: two regions, the
/// infinite one void and first on the list, the other not void, and `faces` faces whose
/// first has its normal pointing out of the solid. The solid is convex, so "out" is away
/// from any other point of it: the middle of the second face of a box, the point half a
/// turn on of a cylinder's side (`fractions` of the two UV boxes).
fn assert_outside_is_infinite(k: &Kernel, b: &Brep, faces: usize, cylinder: bool) {
    let regions = p1::regions(b, None).unwrap();
    assert_eq!(regions.len(), 2);
    let infinite = b.infinite_region().unwrap();
    assert_eq!(regions[0], infinite);
    assert_eq!(b.region(infinite).unwrap().is_void, 1);
    assert_eq!(b.region(regions[1]).unwrap().is_void, 0);
    let list = p1::faces(b, None).unwrap();
    assert_eq!(list.len(), faces);
    let first = b.face(list[0]).unwrap();
    let (at, other, inside) = if cylinder {
        ([0.25, 0.5], first, [0.75, 0.5])
    } else {
        ([0.5, 0.5], b.face(list[1]).unwrap(), [0.5, 0.5])
    };
    let surface = first.surface.as_ref().unwrap();
    let uv = first.uv_domain.evaluate(at[0], at[1]);
    let point = surface.evaluate_point(k, uv).unwrap();
    let normal = surface
        .evaluate_normal(k, uv, Side::Right, Side::Right)
        .unwrap();
    let within = other
        .surface
        .as_ref()
        .unwrap()
        .evaluate_point(k, other.uv_domain.evaluate(inside[0], inside[1]))
        .unwrap();
    assert!(
        normal.dot(point - within) > 0.0,
        "the first face's normal points into the solid"
    );
}

/// Makes one primitive with the entry SolidCraft uses, checks it
/// ([`assert_outside_is_infinite`], and the exact `volume`) and returns the searches for
/// the infinite region its making ran.
fn searches(
    solidcraft: bool,
    make: impl Fn(&mut Kernel) -> Result<Brep>,
    faces: usize,
    cylinder: bool,
    volume: f64,
) -> u64 {
    let mut k = kernel(solidcraft);
    k.work_counters.reset();
    let mut b = make(&mut k).unwrap();
    let n = k.work_counters.infinite_region_searches();
    assert_outside_is_infinite(&k, &b, faces, cylinder);
    let measured = brep_volume_default(&k, &mut b, 1e-6).unwrap();
    assert!(
        (measured - volume).abs() <= 1e-6 * volume,
        "volume {measured}, exact {volume}"
    );
    n
}

/// A box or a cylinder made alone in its B-rep runs one search, the one at the stitch's
/// end: the glue that closes its shell takes the infinite half of the region it splits
/// from the solid's own outside. With a ray fired there too, there were two.
#[test]
fn a_box_or_a_cylinder_made_alone_searches_for_its_infinite_region_once() {
    for solidcraft in [true, false] {
        let p = Point3::new(10.0, 20.0, -1.0);
        let counts = (
            searches(
                solidcraft,
                |k| create_box(k, p, 8.0, 14.0, 3.0),
                6,
                false,
                336.0,
            ),
            searches(
                solidcraft,
                |k| create_cylinder(k, p, 4.0, 14.0),
                3,
                true,
                PI * 16.0 * 14.0,
            ),
        );
        assert_eq!(counts, (1, 1), "SolidCraft's fixes: {solidcraft}");
    }
}

/// A cylinder 5e-4 high is within the stitch's reach at the default tolerance of 1e-5
/// (8e-4), and one at 600 from the origin is past the size that tolerance serves (500):
/// each is stitched as every stitch is, with a search at its closing glue and one at the
/// stitch's end, and its outside is still the infinite region.
#[test]
fn a_thin_or_far_cylinder_searches_at_each_closure() {
    for solidcraft in [true, false] {
        let cylinder = |at: Point3, height: f64| {
            searches(
                solidcraft,
                |k| create_cylinder(k, at, 4.0, height),
                3,
                true,
                PI * 16.0 * height,
            )
        };
        let counts = (
            cylinder(Point3::new(10.0, 20.0, -1.0), 5e-4),
            cylinder(Point3::new(600.0, 0.0, 0.0), 14.0),
        );
        assert_eq!(counts, (2, 2), "SolidCraft's fixes: {solidcraft}");
    }
}

/// A cone's solid is stitched as every stitch is, with a search at the shell its glue
/// closes and one at the stitch's end.
#[test]
fn a_cone_searches_at_each_closure() {
    let p = Point3::new(10.0, 20.0, -1.0);
    let cone = |top: f64, faces: usize, volume: f64| {
        let mut k = kernel(true);
        let mut b = create_cone(&mut k, p, 5.0, top, 7.0).unwrap();
        assert_eq!(p1::faces(&b, None).unwrap().len(), faces);
        let measured = brep_volume_default(&k, &mut b, 1e-6).unwrap();
        assert!((measured - volume).abs() <= 1e-6 * volume);
        k.work_counters.infinite_region_searches()
    };
    let pointed = cone(0.0, 2, PI * 7.0 * 25.0 / 3.0);
    let frustum = cone(2.0, 3, PI * 7.0 * (25.0 + 10.0 + 4.0) / 3.0);
    assert_eq!((pointed, frustum), (2, 2));
}

/// A box added to a B-rep that already holds one is not alone: the region its closing
/// glue splits need not be the one outside it, and the glue searches as upstream's does.
#[test]
fn a_box_added_to_a_b_rep_that_holds_another_searches_at_its_closure() {
    let mut k = kernel(true);
    let mut b = p3::new_brep(&mut k, -1.0, -1.0).unwrap();
    let mut counts = Vec::new();
    for x in [0.0, 100.0] {
        let frame = Axis2Placement::new(Point3::new(x, 0.0, 0.0), Vec3::X, Vec3::Y);
        let mut maker = PrimitiveCreation::new(b.infinite_region(), 0, true);
        k.work_counters.reset();
        maker
            .create_box(&mut k, &mut b, 8.0, 14.0, 3.0, &frame)
            .unwrap();
        counts.push(k.work_counters.infinite_region_searches());
    }
    assert_eq!(counts, [1, 2]);
    assert_eq!(p1::regions(&b, None).unwrap().len(), 3);
    assert_eq!(p1::faces(&b, None).unwrap().len(), 12);
}

/// A box far larger than its tolerance serves, 1000 x 1e7 x 4e7 in a B-rep of tolerance
/// 1e-9, is stitched as every stitch is: two searches, with SolidCraft's fixes and
/// without, and its infinite region is the one outside it.
#[test]
fn a_box_far_larger_than_its_tolerance_serves_searches_at_each_closure() {
    for solidcraft in [true, false] {
        let mut k = kernel(solidcraft);
        let mut b = p3::new_brep(&mut k, 1e-8, -1.0).unwrap();
        assert!((b.tolerance() - 1e-9).abs() < 1e-24);
        let frame = Axis2Placement::from_stored(Point3::ZERO, Vec3::Z, Vec3::Y);
        let mut maker = PrimitiveCreation::new(b.infinite_region(), 0, true);
        k.work_counters.reset();
        maker
            .create_box(&mut k, &mut b, 1000.0, 1e7, 4e7, &frame)
            .unwrap();
        b.refresh_cached_topology();
        assert_eq!(k.work_counters.infinite_region_searches(), 2);
        assert_outside_is_infinite(&k, &b, 6, false);
    }
}
