//! A merged solid inset past a face's radius, or to within the body's tolerance of it, that is
//! left no manifold solid is refused naming the largest inset that builds (#15409). Insetting a
//! cylinder or cone frustum by the distance its side's offset collapses at leaves that face no
//! offset surface; upstream and the fork then leave it out and return the caps as a success,
//! loose. Short of that, a side left a radius under the body's tolerance has a surface, but the
//! merge's intersections, at that tolerance, cut no circle on it, and the caps come back loose
//! too. Closed form: a cylinder's side keeps its radius less the inset, a cone's its larger
//! radius less the inset times the cosine of its half-angle; it builds while that is at least
//! the body's tolerance. Where every face collapses, a sphere or torus inset past its (minor)
//! radius, nothing is left, and the offset is the empty body, as #2092 ruled. Below the radius,
//! the inset builds the smaller solid with its exact volume. Only a merged result is held to the
//! input's manifold, as `offset_brep` holds every merged offset: without the merge the pieces
//! stay as they are. The boundary at the tolerance itself is `issue_15409_inset_at_tolerance`.

#![allow(clippy::unwrap_used, clippy::panic)] // test helpers

use solidcore::error::offset::OffsetFailure;
use solidcore::error::Reason;
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::brep_volume_default;
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p2::{offset_brep_simple, OffsetOptions};
use solidcore::primitive::api_primitives::{
    create_cone, create_cylinder, create_sphere, create_torus,
};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::{Brep, SolidError};

/// The test solids: a cylinder of radius 5 and height 20, a sphere of radius 5, a torus of
/// radii 10 and 3, a cone frustum from radius 5 to 2 over 6 (half-angle atan(1/2)).
fn shape(k: &mut Kernel, name: &str) -> Brep {
    let o = Point3::new(0.0, 0.0, 0.0);
    match name {
        "cylinder" => create_cylinder(k, o, 5.0, 20.0).unwrap(),
        "sphere" => create_sphere(k, o, 5.0).unwrap(),
        "torus" => create_torus(k, o, 10.0, 3.0).unwrap(),
        "cone" => create_cone(k, o, 5.0, 2.0, 6.0).unwrap(),
        _ => panic!("{name}"),
    }
}

/// `name` inset by `d` with rounded edges.
fn inset(name: &str, d: f64, merge: bool) -> Result<Brep, SolidError> {
    let mut k = Kernel::default();
    let mut b = shape(&mut k, name);
    let options = OffsetOptions {
        distance: -d,
        extend_convex_edges: false,
        merge_results: merge,
    };
    offset_brep_simple(&mut k, &mut b, options)
}

/// The collapse refusal behind `e`: its distance asked and the radius it names.
fn collapse(e: &SolidError) -> Option<(f64, f64)> {
    std::iter::successors(Some(e), |e| e.cause()).find_map(|e| {
        match e.failure().map(|f| &f.reason) {
            Some(&Reason::Offset(OffsetFailure::Collapses {
                distance, radius, ..
            })) => Some((distance, radius)),
            _ => None,
        }
    })
}

/// The largest inset each shape's side still builds at, closed form: the side's offset
/// collapses once `r + d cos(a) < t` (`Cone::offset_collapses`, strict), `r` the larger radius,
/// `a` the half-angle and `t` the body's tolerance, so it builds up to `(r - t) / cos(a)`.
fn limit(name: &str) -> f64 {
    let mut k = Kernel::default();
    let t = shape(&mut k, name).tolerance();
    match name {
        "cylinder" => 5.0 - t,
        // cos(atan(1/2)) = 2 / sqrt(5)
        _ => (5.0 - t) * 5f64.sqrt() / 2.0,
    }
}

fn assert_refused(name: &str, d: f64, merge: bool) {
    let e = match inset(name, d, merge) {
        Ok(b) => panic!(
            "{name} inset by {d} (merge {merge}) built {} faces",
            solidcore::topology::brep::p1::faces(&b, None)
                .unwrap()
                .len()
        ),
        Err(e) => e,
    };
    let Some((asked, radius)) = collapse(&e) else {
        panic!("{name} inset by {d} (merge {merge}): {e}");
    };
    assert_eq!(asked, -d);
    // the named distance is the closed form, to rounding: the bisection halves 64 times, and
    // `r + d cos(a)` against `t` is evaluated to a few units in the last place
    let exact = limit(name);
    assert!(
        (radius - exact).abs() <= 1e-12 * exact,
        "{name} inset by {d}: names {radius}, the limit is {exact}"
    );
}

#[test]
fn a_cylinder_inset_past_its_radius_is_refused_naming_it() {
    for d in [5.0, 5.5, 10.0] {
        assert_refused("cylinder", d, true);
    }
}

/// #2092: every face collapses, so the offset is the empty body, not a refusal.
#[test]
fn a_sphere_or_torus_inset_past_its_radius_is_empty() {
    for merge in [true, false] {
        for (name, d) in [
            ("sphere", 5.0),
            ("sphere", 6.0),
            ("torus", 3.0),
            ("torus", 3.3),
        ] {
            let b = inset(name, d, merge).unwrap_or_else(|e| panic!("{name} by {d}: {e}"));
            let faces = solidcore::topology::brep::p1::faces(&b, None).unwrap();
            assert!(
                faces.is_empty(),
                "{name} inset by {d}: {} faces",
                faces.len()
            );
        }
    }
}

#[test]
fn a_cone_inset_past_its_collapse_is_refused_naming_it() {
    assert_refused("cone", 5.6, true);
}

#[test]
fn insets_below_the_radius_build_the_smaller_solid() {
    let pi = std::f64::consts::PI;
    // the cylinder: radius 1, height 20 - 2 * 4
    // the sphere: radius 1
    // the torus: minor radius 0.6 about the same major radius 10
    for (name, d, exact) in [
        ("cylinder", 4.0, pi * 1.0 * 12.0),
        ("sphere", 4.0, 4.0 / 3.0 * pi),
        ("torus", 2.4, 2.0 * pi * pi * 10.0 * 0.36),
    ] {
        let k = Kernel::default();
        let mut b = inset(name, d, true).unwrap();
        assert!(is_manifold_solid(&b).unwrap(), "{name} inset by {d}");
        let v = brep_volume_default(&k, &mut b, 1e-9).unwrap();
        assert!(
            (v - exact).abs() <= 1e-6 * exact,
            "{name} inset by {d}: {v} against {exact}"
        );
    }
    // past its top radius 2, the cone's offset is truncated at its apex and still a solid
    let b = inset("cone", 2.2, true).unwrap();
    assert!(is_manifold_solid(&b).unwrap());
}
