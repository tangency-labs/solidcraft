//! tangency-labs/solidcore#2350, S1 (opt-in `UpstreamFixes::shell_one_construction`,
//! WR-718): each face's offset sheet exact or certified.
//!
//! The issue's §3.1 parabolic sheet, the quadratic Bezier (-10, 0), (0, 10), (10, 0)
//! extruded 20 (one span, and two spans with a knot at s = 1/2), thickened by `t`:
//! towards the centre of curvature the solid is the tube formula's
//! `20 (t L - t^2 pi / 4)` (L = 22.955871, the arc length; the tangent turns by pi/2),
//! away from it `20 (t L + t^2 pi / 4)`. At t11 towards the centre the offset folds
//! (the apex's radius of curvature is 10): with the flag the thicken is refused naming
//! 10 (D5); without it SMLib's port fails on its broken topology (the pinned C++ builds
//! a folded body of one span and crashes on two).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{volume, Notifications};
use solidcore::error::offset::OffsetFailure;
use solidcore::error::{ErrorKind, Reason};
use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::nurbs::b_spline_surface::p1::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{Brep, FaceId};
use solidcore::{EntityRef, Kernel};

/// The quadratic Bezier (-10, 0), (0, 10), (10, 0) in the xy-plane extruded 20 along z;
/// `two_spans` inserts a knot at s = 1/2.
fn parabola_sheet(k: &mut Kernel, two_spans: bool) -> Brep {
    let (pts, uk): (Vec<(f64, f64)>, Vec<f64>) = if two_spans {
        (
            vec![(-10.0, 0.0), (-5.0, 5.0), (5.0, 5.0), (10.0, 0.0)],
            vec![0., 0., 0., 0.5, 1., 1., 1.],
        )
    } else {
        (
            vec![(-10.0, 0.0), (0.0, 10.0), (10.0, 0.0)],
            vec![0., 0., 0., 1., 1., 1.],
        )
    };
    let pw: Vec<Vec<CPoint>> = pts
        .iter()
        .map(|&(x, y)| vec![CPoint::new(x, y, 0.0, 1.0), CPoint::new(x, y, 20.0, 1.0)])
        .collect();
    let n = pts.len() - 1;
    let s = Surface::from(
        BSplineSurface::from_data(n, 1, 2, 1, uk.len() - 1, 3, &pw, &uk, &[0., 0., 1., 1.])
            .unwrap(),
    );
    let d = s.natural_uv_domain_with_nurb(k).unwrap();
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT).unwrap();
    {
        let this_face = |f: FaceId| -> ObjectRef { notify_ref(EntityRef::Face(f)) };
        let mut state = ArenaNotifyState::new(&b);
        let mut env = state.env(None);
        let mut marks = Marks::default();
        let mut fenv = FaceBuildEnv {
            env: &mut env,
            marks: &mut marks,
            this_face: &this_face,
        };
        create_face_from_surface(k, &mut b, Some(s), &d, &mut fenv).unwrap();
    }
    b.refresh_cached_topology();
    b
}

fn thicken(k: &mut Kernel, mut sheet: Brep, distance: f64) -> solidcore::Result<Brep> {
    let mut notifications = Notifications::new(&mut sheet);
    OffsetExecutive::shell_brep(
        k,
        sheet,
        ShellOptions {
            offset: OffsetOptions {
                distance,
                extend_convex_edges: true,
                merge_results: true,
            },
            create_offset_solid: true,
            merge_sheet_side_walls: false,
        },
        &[],
        &mut notifications.context(),
    )
    .result
    .map(|mut b| {
        b.refresh_cached_topology();
        b
    })
}

/// The arc length of the parabola y = (100 - x^2) / 20, x in [-10, 10].
fn arc_length() -> f64 {
    // integral of sqrt(1 + x^2 / 100): [x/2 sqrt(1 + x^2/100) + 5 asinh(x / 10)]
    let half = 5.0 * 2f64.sqrt() + 5.0 * 1f64.asinh();
    2.0 * half
}

fn tube(t: f64, towards: bool) -> f64 {
    let turn = std::f64::consts::FRAC_PI_4 * t * t;
    20.0 * (t * arc_length() + if towards { -turn } else { turn })
}

/// The distance from `(x, y)` to the parabola arc (its closest point by bisection on
/// the derivative of the squared distance, over a fine bracket).
fn distance_to_arc(x: f64, y: f64) -> f64 {
    let f = |s: f64| {
        let (px, py) = (-10.0 + 20.0 * s, 20.0 * s * (1.0 - s));
        ((px - x).powi(2) + (py - y).powi(2)).sqrt()
    };
    let n = 2000;
    let mut best = (f64::INFINITY, 0.0);
    for i in 0..=n {
        let s = f64::from(i) / f64::from(n);
        let d = f(s);
        if d < best.0 {
            best = (d, s);
        }
    }
    let (mut lo, mut hi) = ((best.1 - 1e-3_f64).max(0.0), (best.1 + 1e-3_f64).min(1.0));
    for _ in 0..200 {
        let (a, b) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
        if f(a) < f(b) {
            hi = b;
        } else {
            lo = a;
        }
    }
    f(0.5 * (lo + hi))
}

/// The largest deviation of the result's B-spline faces off the sheet from the
/// distance `t` to the sheet (sampled on each face's UV domain).
fn offset_face_error(k: &Kernel, b: &Brep, t: f64) -> f64 {
    let mut worst: f64 = 0.0;
    for &f in b.face_list() {
        let face = b.face(f).unwrap();
        let s = face.surface.as_ref().unwrap();
        let d = face.uv_domain;
        let mid = s.evaluate_point(k, d.mid()).unwrap();
        let flat = (mid.z - 0.0).abs() < 1e-9 || (mid.z - 20.0).abs() < 1e-9;
        if flat || (distance_to_arc(mid.x, mid.y) - t).abs() > 1e-3 {
            continue;
        }
        for i in 0..=12 {
            for j in 0..=12 {
                let uv = solidcore::math::Point2::new(
                    d.min().x + (d.max().x - d.min().x) * f64::from(i) / 12.0,
                    d.min().y + (d.max().y - d.min().y) * f64::from(j) / 12.0,
                );
                let p = s.evaluate_point(k, uv).unwrap();
                worst = worst.max((distance_to_arc(p.x, p.y) - t).abs());
            }
        }
    }
    worst
}

fn kernel(flag: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.shell_one_construction = flag;
    k
}

/// S1's gate: the offset face is the certified offset, within the shell's tolerance
/// (1e-5) of distance `t` from the sheet at every parameter (sampled: 2.7e-6 at t2), so
/// the solid is the tube formula's to the tolerance times its boundary area. Measured at
/// t2: the offset face's area 6.7e-9 off the closed form (`N_SrfOffset`'s fit: 1.4e-8),
/// the volume 2.4e-8, from the two end caps, ruled between the sheet's edge and the
/// offset edge the executive fits; their exact step walls are S2's (§1.2).
#[test]
fn parabola_thickens_to_its_certified_offset() {
    for two in [false, true] {
        for (d, t, towards) in [(2.0, 2.0, true), (-2.0, 2.0, false), (5.0, 5.0, true)] {
            let mut k = kernel(true);
            let sheet = parabola_sheet(&mut k, two);
            let mut b = thicken(&mut k, sheet, d).unwrap();
            assert_eq!(b.face_list().len(), 6);
            assert!(matches!(brep_is_manifold_solid(&b), Ok(true)));
            let c = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
            assert_eq!(c.failures().count(), 0, "{:?}", c.reports);
            let err = offset_face_error(&k, &b, t);
            // the certificate's guarantee: within the shell's tolerance everywhere
            assert!(err <= b.tolerance(), "offset face off the offset by {err}");
            // so the solid is the tube formula's to the tolerance over its boundary
            // (every face within the tolerance of its exact place)
            let want = tube(t, towards);
            // the sheet, its offset (no longer than the sheet plus 20 t pi / 2), two caps, two walls
            let boundary =
                40.0 * arc_length() + 10.0 * t * std::f64::consts::PI + want / 10.0 + 40.0 * t;
            let got = volume(&k, &mut b).unwrap();
            assert!(
                (got - want).abs() <= b.tolerance() * boundary,
                "{got} vs {want}"
            );
        }
    }
}

#[test]
fn parabola_fold_is_refused_naming_its_apex_radius() {
    for two in [false, true] {
        let mut k = kernel(true);
        let sheet = parabola_sheet(&mut k, two);
        let err = thicken(&mut k, sheet, 11.0).unwrap_err();
        let Some(failure) = err.failure() else {
            panic!("t11 towards the centre: {err:?}");
        };
        let Reason::Offset(OffsetFailure::Folds {
            face,
            distance,
            largest,
        }) = failure.reason
        else {
            panic!("t11 towards the centre: {err:?}");
        };
        assert!((largest - 10.0).abs() < 1e-9, "{largest}");
        assert_eq!(distance.abs(), 11.0);
        assert_eq!(err.sm_status(), Some(1001));
        // #2615: the refusal in solidcore's terms, naming the face and the sizes
        assert_eq!(err.kind(), ErrorKind::OffsetFolds);
        assert_eq!(err.entity(), Some(EntityRef::Face(face)));
        assert_eq!(
            err.to_string(),
            format!(
                "{}: the offset of face {} by {distance} folds: the wall is thicker than a \
                 radius of curvature of the face on that side (the largest wall that works is \
                 {largest})",
                err.operation(),
                face.index()
            )
        );
        assert!(!err.to_string().contains("1001"), "{err}");
    }
}

/// With the flag off the shell is SMLib's: t11 fails as the port does (not refused), and
/// t2 builds on `N_SrfOffset`'s sampled fit.
#[test]
fn flag_off_keeps_smlibs_results() {
    for two in [false, true] {
        let mut k = kernel(false);
        let sheet = parabola_sheet(&mut k, two);
        let err = thicken(&mut k, sheet, 11.0).unwrap_err();
        assert_ne!(err.kind(), ErrorKind::OffsetFolds, "{err:?}");
        let mut k = kernel(false);
        let sheet = parabola_sheet(&mut k, two);
        let mut b = thicken(&mut k, sheet, 2.0).unwrap();
        assert!(matches!(brep_is_manifold_solid(&b), Ok(true)));
        let v = volume(&k, &mut b).unwrap();
        assert!((v - tube(2.0, true)).abs() <= 1e-6 * v, "{v}");
    }
}

/// §3.1's closed-form shells on analytic faces, which S1 leaves to their exact offsets:
/// with every opt-in fix on (this flag included) they keep their closed forms.
#[test]
fn analytic_shells_keep_their_closed_forms() {
    use offset_fixture::{planar_faces_at, shell};
    use solidcore::kernel::UpstreamFixes;
    use solidcore::math::Point3;
    use solidcore::primitive::api_primitives::{create_cylinder, create_sphere, create_torus};
    use std::f64::consts::PI;
    let fixed = || {
        let mut k = Kernel::default();
        for name in UpstreamFixes::NAMES {
            *k.fixes.flag_mut(name).unwrap() = true;
        }
        k
    };
    // the cup r10 h20 open at its top, t2
    let mut k = fixed();
    let cup = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0).unwrap();
    let top = planar_faces_at(&k, &cup, 2, 20.0);
    let mut b = shell(&mut k, cup, -2.0, &top).unwrap();
    let want = PI * (100.0 * 20.0 - 64.0 * 18.0);
    let got = volume(&k, &mut b).unwrap();
    assert!((got - want).abs() <= 1e-8 * want, "cup {got} vs {want}");
    // the closed sphere R10, t2: a sealed void
    let mut k = fixed();
    let ball = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0).unwrap();
    let mut b = shell(&mut k, ball, -2.0, &[]).unwrap();
    let want = 4.0 / 3.0 * PI * (1000.0 - 512.0);
    let got = volume(&k, &mut b).unwrap();
    assert!((got - want).abs() <= 1e-8 * want, "sphere {got} vs {want}");
    // the closed torus (20, 5), t1
    let mut k = fixed();
    let ring = create_torus(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 5.0).unwrap();
    let mut b = shell(&mut k, ring, -1.0, &[]).unwrap();
    let want = 2.0 * PI * PI * 20.0 * (25.0 - 16.0);
    let got = volume(&k, &mut b).unwrap();
    assert!((got - want).abs() <= 1e-8 * want, "torus {got} vs {want}");
}
