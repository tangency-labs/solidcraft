//! The two opt-in arc tests of `CreateLinearSweep` together: WR-576
//! (`Kernel::fixes.linear_sweep_arc_to_cylinder_zero`, #1507, tangency-labs/smlib@fd59111)
//! tests a generating curve's arc to the cylinder check's zero, `1000 SM_EFF_ZERO * (1 +
//! size)`, not to the B-rep tolerance; WR-565 (`Kernel::fixes.linear_sweep_arc_is_cylinder`,
//! #1477, tangency-labs/smlib 12918e9) makes an arc short of a whole turn swept square to
//! its plane a cylinder too. The arc test gates both, as `ArcZero` gates `ArcSweepCylinder`
//! in the fork: with both on only an exact arc becomes a cylinder, whole or partial.
//!
//! Each profile (an ellipse of radii 10 and 10 + 1e-7, an arc to the B-rep tolerance and
//! not to the cylinder check's zero, or an exact circle, each over a whole turn or half a
//! turn closed by its chord) is swept 2 along +z, capped, with the two fixes off-off,
//! off-on, on-off and on-on (WR-576, WR-565).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::Line;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::math::{Point3, Vec3};
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1;
use solidcore::{Brep, Kernel};

const R: f64 = 10.0;

/// The frame of the z = 0 plane at the origin.
fn frame() -> Axis2Placement {
    let mut frame = Axis2Placement::default();
    frame
        .set_canonical(
            Point3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        )
        .unwrap();
    frame
}

/// The ellipse of radii `R` and `R + 1e-7` in z = 0 over `[0, end]` degrees: an arc to
/// the B-rep tolerance, not to the cylinder check's zero.
fn near_arc(k: &Kernel, end: f64) -> Curve {
    let interval = Extent1d::from_min_max(0.0, end);
    Curve::from(
        Ellipse::create_canonical(k, &frame(), R, R + 1.0e-7, Some(&interval), None).unwrap(),
    )
}

/// The circle of radius `R` in z = 0 over `[0, end]` degrees.
fn arc(k: &Kernel, end: f64) -> Curve {
    let interval = Extent1d::from_min_max(0.0, end);
    Curve::from(Ellipse::create_canonical_circle(k, &frame(), R, Some(&interval), None).unwrap())
}

/// The side faces' surface kinds: `"cylinder"` (an `SmCylinder`, the native cone
/// class) or `"extrusion"` (an `SmSurfOfExtrusion`); the caps and the half disc's
/// planar side are left out.
fn side_kinds(b: &Brep) -> Vec<&'static str> {
    p1::faces(b, None)
        .unwrap()
        .into_iter()
        .filter_map(
            |f| match b.face(f).unwrap().surface.as_ref().unwrap().kind() {
                SurfaceKind::Cone(_) => Some("cylinder"),
                SurfaceKind::SurfOfExtrusion(_) => Some("extrusion"),
                _ => None,
            },
        )
        .collect()
}

/// The profile of `curve`: the curve alone for a whole turn, else closed by its chord
/// from (-R, 0, 0) back to (R, 0, 0) (a half disc).
fn profile(k: &Kernel, curve: &dyn Fn(&Kernel) -> Curve) -> Vec<Curve> {
    let c = curve(k);
    let ends = c.ends(k).unwrap();
    if (ends.start - ends.end).length() < 1.0e-6 {
        return vec![c];
    }
    let chord = Line::create_line_segment(3, ends.end, ends.start, None).unwrap();
    vec![c, Curve::from(chord)]
}

/// The profile of `curve` extruded 2 along +z and capped, with WR-576's fix `arc_zero`
/// and WR-565's `partial_arcs`.
fn sweep(curve: &dyn Fn(&Kernel) -> Curve, arc_zero: bool, partial_arcs: bool) -> &'static str {
    let mut k = Kernel::default();
    k.fixes.linear_sweep_arc_to_cylinder_zero = arc_zero;
    k.fixes.linear_sweep_arc_is_cylinder = partial_arcs;
    let curves = profile(&k, curve);
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let b = create_linear_sweep(&mut k, &refs, Vec3::new(0.0, 0.0, 1.0), 2.0, true).unwrap();
    let kinds = side_kinds(&b);
    let [kind] = kinds.as_slice() else {
        panic!("one side face: {kinds:?}");
    };
    kind
}

/// The side face of `curve` for WR-576 / WR-565 off/off, off/on, on/off and on/on.
fn matrix(curve: &dyn Fn(&Kernel) -> Curve) -> [&'static str; 4] {
    [
        sweep(curve, false, false),
        sweep(curve, false, true),
        sweep(curve, true, false),
        sweep(curve, true, true),
    ]
}

#[test]
fn arc_flags_near_circle_is_an_arc_only_to_the_brep_tolerance() {
    let k = Kernel::default();
    for end in [360.0, 180.0] {
        let c = near_arc(&k, end);
        // the B-rep tolerance (the context's zone tolerance) takes it for an arc
        assert!(c.is_arc(&k, 8, 1.0e-6).unwrap().is_some(), "{end}");
        // the cylinder check's zero, 1000 SM_EFF_ZERO * (1 + R), does not
        assert!(
            c.is_arc(&k, 8, 1.0e-9 * (1.0 + R)).unwrap().is_none(),
            "{end}"
        );
    }
}

#[test]
fn arc_flags_near_circle_is_a_cylinder_only_without_wr576() {
    assert_eq!(
        matrix(&|k| near_arc(k, 360.0)),
        ["cylinder", "cylinder", "extrusion", "extrusion"]
    );
}

#[test]
fn arc_flags_near_half_circle_is_a_cylinder_only_with_wr565_alone() {
    assert_eq!(
        matrix(&|k| near_arc(k, 180.0)),
        ["extrusion", "cylinder", "extrusion", "extrusion"]
    );
}

#[test]
fn arc_flags_exact_arcs_keep_their_cylinders_with_wr576() {
    assert_eq!(
        matrix(&|k| arc(k, 360.0)),
        ["cylinder", "cylinder", "cylinder", "cylinder"]
    );
    assert_eq!(
        matrix(&|k| arc(k, 180.0)),
        ["extrusion", "cylinder", "extrusion", "cylinder"]
    );
}
