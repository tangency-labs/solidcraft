//! The case of tangency-labs/solidcore#2540: a triangular profile whose short span is a
//! cubic B-spline 7.1e-7 mm off its chord, extruded 44 mm along the sketch normal.
//!
//! The issue reports the extrude refused, with `SmSurfOfRevolution` test 10 "Dist U" at
//! 4.15e-3 against a scaled zero of 2.6e-9, owner `SmCylinder`, from a cylinder of radius
//! 216985 mm recognised on the swept face. Through this kernel's own sweep the body builds
//! and the swept face is a surface of extrusion, so these tests pin both halves: the solid
//! the profile describes, against a closed form, and the recognition that would have to
//! change for the refusal to appear here.
//!
//! The entry `kernel_cases/sweeps/2540-extrude-near-straight-spline-recognised-as-a-huge-cylinder.json`
//! carries the same case as data, and
//! `plan/findings/sweep.api_brep--2540-near-straight-spline-is-not-a-cylinder.md`
//! records what was measured.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test

use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::assert_array::{AssertArray, ObjectKey};
use solidcore::geom::attribute::{AttributeStore, ObjectKey as AttrKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::{Kernel, Vec3};

/// The profile's five points, the issue's sketch read into model space: the plane x = -22,
/// sketch x along +Z and sketch y along -Y. Each is the nearest double to the exact decimal.
const P0: [f64; 3] = [-22.0, -2.808157, -25.446366];
const P1: [f64; 3] = [-22.0, -2.828967043, -25.743002617];
const P2: [f64; 3] = [-22.0, -2.849776091, -26.039639304];
const P3: [f64; 3] = [-22.0, -2.870585, -26.336276];
const P4: [f64; 3] = [-22.0, 2.148371345929137, -26.24121568504867];

/// How far the extrude runs along the profile plane's normal, as the issue sets it.
const DISTANCE: f64 = 44.0;

/// The profile's planar area by Green's theorem, exactly
/// `446048552350889275593 / 200000000000000000000` mm², times [`DISTANCE`].
///
/// The five points lie in one plane and the extrude runs along its normal with no taper, so
/// the body is a prism and `V = 44 * A`. A four-point clamped cubic B-spline (knots
/// 0,0,0,0,1,1,1,1) is the cubic Bézier on its control points, so `A` is the sum of one
/// term per boundary piece of `1/2 ∮ (x dy - y dx)` in the plane's own coordinates:
///
/// ```text
/// A      = B + T(p3,p4) + T(p4,p0)
/// T(a,b) = (ax*by - ay*bx)/2                          for a straight segment
/// B      = (1/20) * [6(x0y1 - x1y0) + 3(x0y2 - x2y0) + (x0y3 - x3y0)
///                    + 3(x1y2 - x2y1) + 3(x1y3 - x3y1) + 6(x2y3 - x3y2)]
///                                                     for the Bézier p0..p3
/// ```
///
/// The derivation is the corpus entry's `expected.method`; `A` is 2.2302427617544462 mm².
const DERIVED_VOLUME_MM3: f64 = 98.130_681_517_195_64;

/// `brep_volume` integrates to its own accuracy, so the prism is checked a little above it;
/// the measured gap is 8.7e-16 relative.
const VOLUME_TOLERANCE: f64 = 1e-12;

fn point(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// The profile's short span: the cubic B-spline on `P0..P3`, 7.1e-7 mm off its chord.
fn spline_span() -> Curve {
    let points = [P0, P1, P2, P3]
        .into_iter()
        .map(|p| CPoint::from_point(point(p)))
        .collect();
    let nurb = NurbCurve::new(points, 3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]).unwrap();
    Curve::from(BSplineCurve::from_nurb(nurb, 3, false))
}

fn segment(a: [f64; 3], b: [f64; 3]) -> Curve {
    Curve::from(Line::from_points(point(a), point(b), 3))
}

/// Which analytic surface a face carries, as a name to assert on.
fn kind_name(surface: &Surface) -> &'static str {
    match surface.kind() {
        SurfaceKind::Cone(c) if c.is_cylinder_class() => "Cone(cylinder class)",
        SurfaceKind::Cone(_) => "Cone",
        SurfaceKind::Plane(_) => "Plane",
        SurfaceKind::BSpline(_) => "BSpline",
        SurfaceKind::SurfOfExtrusion(_) => "SurfOfExtrusion",
        SurfaceKind::SurfOfRevolution(_) => "SurfOfRevolution",
        SurfaceKind::Sphere(_) => "Sphere",
        SurfaceKind::Torus(_) => "Torus",
        _ => "other",
    }
}

/// Every surface test the kernel runs on `surface` at level 0 that did not pass, as
/// `<index> <name>`, so a failure says which test went off.
fn failed_surface_tests(k: &Kernel, surface: &Surface) -> Vec<String> {
    let mut list = AssertArray::new();
    let ok = surface
        .assert_valid(k, Some(&mut list), 0, ObjectKey(1))
        .unwrap();
    let failed: Vec<String> = list
        .reports()
        .iter()
        .filter(|r| !r.ok)
        .map(|r| format!("{} {}", r.test_index, r.name))
        .collect();
    assert_eq!(
        ok,
        failed.is_empty(),
        "assert_valid agrees with its reports"
    );
    failed
}

#[test]
fn near_straight_spline_extrudes_to_the_prism_its_profile_describes() {
    let span = spline_span();
    let back = segment(P3, P4);
    let front = segment(P4, P0);
    let profile = [Some(&span), Some(&back), Some(&front)];

    let mut k = Kernel::default();
    let mut body = create_linear_sweep(&mut k, &profile, Vec3::X, DISTANCE, true)
        .expect("the profile the issue sketches extrudes");

    // One side face per profile curve plus two caps; the spline span has no interior knot,
    // so it stays one face.
    assert_eq!(body.face_list().len(), 5, "faces");
    assert_eq!(body.edge_list().len(), 9, "edges");
    assert_eq!(body.vertex_list().len(), 6, "vertices");

    let kinds: Vec<&str> = body
        .face_list()
        .iter()
        .map(|fid| {
            let face = body.face(*fid).unwrap();
            let surface = face.surface.as_ref().unwrap();
            let failed = failed_surface_tests(&k, surface);
            assert!(
                failed.is_empty(),
                "a face failed its surface tests: {failed:?}"
            );
            kind_name(surface)
        })
        .collect();
    assert_eq!(
        kinds,
        vec!["SurfOfExtrusion", "Plane", "Plane", "Plane", "Plane"],
        "the spline span sweeps a surface of extrusion, the two segments and the caps planes"
    );

    let store = AttributeStore::default();
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &|_| AttrKey(0),
    };
    let volume = brep_volume(&k, &mut body, 1e-9, &attributes).expect("the prism has a volume");
    let relative = (volume - DERIVED_VOLUME_MM3).abs() / DERIVED_VOLUME_MM3;
    assert!(
        relative < VOLUME_TOLERANCE,
        "volume {volume:.15} against the closed form {DERIVED_VOLUME_MM3:.15}, \
         {relative:.3e} relative"
    );
}

/// The swept face of the spline span as `add_analytics` sees it: the extrusion net of the
/// span's four control points along the extrude, degree 3 in u and 1 in v.
fn swept_net() -> BSplineSurface {
    let net = [P0, P1, P2, P3]
        .into_iter()
        .map(|p| {
            vec![
                CPoint::new(p[0], p[1], p[2], NOW),
                CPoint::new(p[0] + DISTANCE, p[1], p[2], NOW),
            ]
        })
        .collect();
    let nurb = NurbSurface::new(
        net,
        3,
        1,
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
    )
    .unwrap();
    BSplineSurface::from_nurb(nurb, false)
}

/// SolidCraft's own fix profile at its `ws/pin-16`: every flag on but the two it holds
/// back, `fillet_keep_features` and `fillet_refuse_rounds_breaking_through`, as
/// `issue_2370_keep_features_band_clear_of_boss` names that profile. The issue measured
/// through SolidCraft, so this is the set its refusal came from.
fn solidcraft_fixes() -> UpstreamFixes {
    let mut fixes = UpstreamFixes::all();
    fixes.fillet_keep_features = false;
    fixes.fillet_refuse_rounds_breaking_through = false;
    fixes
}

/// The fix sets this test runs, each with the name it is reported under: the pinned C++
/// behaviour, SolidCraft's profile, every flag on, and each flag alone on top of the
/// default.
///
/// That is `3 + UpstreamFixes::NAMES.len()` of the `2 ^ NAMES.len()` sets the type can
/// hold. The flags are independent fields, so a set that is not run here can still differ
/// from all of these; the test pins the sets it names and makes no claim about the rest.
fn measured_fix_sets() -> Vec<(&'static str, UpstreamFixes)> {
    let mut sets = vec![
        ("default", UpstreamFixes::default()),
        ("solidcraft", solidcraft_fixes()),
        ("all", UpstreamFixes::all()),
    ];
    for &name in UpstreamFixes::NAMES {
        let mut fixes = UpstreamFixes::default();
        *fixes
            .flag_mut(name)
            .expect("NAMES lists field names flag_mut finds") = true;
        sets.push((name, fixes));
    }
    sets
}

#[test]
fn the_swept_span_is_recognised_as_a_surface_of_extrusion_not_a_cylinder() {
    let net = swept_net();
    // Every set is run before anything is asserted, so a failure names all the sets that
    // differ rather than only the first.
    let mut other_kind: Vec<String> = Vec::new();
    let mut failed_tests: Vec<String> = Vec::new();
    for (tag, fixes) in measured_fix_sets() {
        let k = Kernel {
            fixes,
            ..Kernel::default()
        };
        let recognised = net
            .copy_and_add_analytics(&k)
            .expect("the extrusion net goes through the recogniser");
        let kind = kind_name(&recognised);
        if kind != "SurfOfExtrusion" {
            other_kind.push(format!("{tag}: {kind}"));
        }
        let failed = failed_surface_tests(&k, &recognised);
        if !failed.is_empty() {
            failed_tests.push(format!("{tag}: {failed:?}"));
        }
    }
    assert!(
        other_kind.is_empty(),
        "the near-straight span is no cylinder under the sets measured here, but          {} of them recognised another surface: {other_kind:?}",
        other_kind.len()
    );
    assert!(
        failed_tests.is_empty(),
        "the recognised surface failed its own tests: {failed_tests:?}"
    );
}
