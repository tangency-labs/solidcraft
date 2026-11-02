//! WR-490 (tangency-labs/solidcore#868; #1954's flag audit, class B):
//! `Kernel::fixes.ellipse_trim_seam_lines_span_cone_height` on the record's boolean
//! triggers. A cylinder C r 44.45 along x (-200..200) and two rods r 24.15 at +-45 deg in
//! the XZ plane whose axes meet at (0, 0, 30), each 240 long from 40 before that point,
//! every one built as `SmApiCreateCylinder` turned `seam` about its own axis (Z), about Y
//! onto its axis and moved to its start. At the old pin `(C u A) u B` dropped the half
//! ellipse whose seam crossing lay in A's trimmed-off part and gave `C u A`; the same joint
//! with tubes (steel-026) was refused or came out 4592 mm^3 too large.
//!
//! Since v1.0.1 `sm_GetEllipseTrimIntervals` takes the trim circles at the domain's v
//! limits (the record's 2026-10-27 addendum), and both orders give the right body at every
//! seam with the flag off and on, as the pinned C++ does
//! (`plan/findings/general--flag-pin-wr490-ellipse-trim-same-since-v1-0-1--addendum-2026-11-02-boolean-triggers.md`).
//! The expected volumes are closed forms by slices y = const (each section a union of
//! rectangles in the XZ plane, integrated in y; #868 gives 3137849.373 for the rods).
//!
//! The tubes coped to the chord's solid before joining fail at seams 35 and 90 deg on
//! both engines and with either setting (a body of volume -4592.38 that is not a manifold
//! solid): a separate defect, kept in [`coped_tubes_join_known_failing`].
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::{Brep, Kernel};

/// The seam angles of #868 (0, 35 and 60 deg lost body B at the old pin) and 90 deg.
const SEAMS: [f64; 4] = [0.0, 35.0, 60.0, 90.0];

/// `C u A u B` of the rods, by slices.
const RODS_VOLUME: f64 = 3_137_849.373_374;

/// The tubes (chord CHS 88.9x5, braces CHS 48.3x4, open-ended) joined as they are,
/// #868's `(tube + A) + B`, by slices.
const TUBES_VOLUME: f64 = 779_093.246_074;

/// The chord joined with the braces coped to the chord's solid, by slices.
const COPED_VOLUME: f64 = 730_345.211_757;

/// `brep_volume`'s finest relative accuracy (upstream clamps to `[1e-8, 1e-1]`).
const VOLUME_ACCURACY: f64 = 1e-8;

/// How far a right body's volume may be from its closed form, relative. The intersection
/// edges are fitted curves; at the four seams the rods land within 3.2e-9 and the tubes
/// within 6.4e-8. A body missing a brace, or #868's tubes 4592 mm^3 too large, is off by
/// 5.9e-3 or more.
const VOLUME_REL: f64 = 1e-7;

/// A cylinder of radius `r` and length `len` along Z at the origin, turned `seam` about Z,
/// `y_deg` about Y and moved to `at`.
fn cylinder(k: &mut Kernel, r: f64, len: f64, seam: f64, y_deg: f64, at: Vec3) -> Brep {
    let mut b = create_cylinder(k, Point3::ZERO, r, len).unwrap();
    let o = Point3::ZERO;
    api_rotate(k, Some(TransformObject::Brep(&mut b)), o, Vec3::Z, seam).unwrap();
    api_rotate(k, Some(TransformObject::Brep(&mut b)), o, Vec3::Y, y_deg).unwrap();
    api_translate(k, Some(TransformObject::Brep(&mut b)), at).unwrap();
    b
}

/// An open-ended tube: the outer cylinder minus the inner one of the same length.
fn tube(k: &mut Kernel, ro: f64, ri: f64, len: f64, seam: f64, y_deg: f64, at: Vec3) -> Brep {
    let outer = cylinder(k, ro, len, seam, y_deg, at);
    let inner = cylinder(k, ri, len, seam, y_deg, at);
    boolean_difference(k, outer, inner).unwrap()
}

/// The start of the brace at `y_deg` (+45 or -45): 40 before (0, 0, 30) along its axis
/// (`sin 45` for both components, as the difftest fixture of the measurement has them).
fn brace_start(y_deg: f64) -> Vec3 {
    let s = 45f64.to_radians().sin();
    Vec3::new(0.0, 0.0, 30.0) - 40.0 * Vec3::new(s.copysign(y_deg), 0.0, s)
}

const CHORD_START: Vec3 = Vec3::new(-200.0, 0.0, 0.0);

fn kernel(fixed: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.ellipse_trim_seam_lines_span_cone_height = fixed;
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, VOLUME_ACCURACY, &attrs).unwrap()
}

/// `None` when `b` is a manifold solid of volume `expected` (within [`VOLUME_REL`]), else
/// what is wrong.
fn wrong(k: &Kernel, b: &mut Brep, expected: f64) -> Option<String> {
    let manifold = is_manifold_solid(b).unwrap();
    let v = volume(k, b);
    println!("  volume {v:.6} manifold {manifold}");
    if !manifold {
        return Some(format!("not a manifold solid, volume {v}"));
    }
    let off = (v - expected).abs() / expected;
    (off > VOLUME_REL).then(|| format!("volume {v}, expected {expected} (off {off:.1e})"))
}

/// `(C u A) u B` when `a_first`, else `(C u B) u A`, of the rods at `seam`: `None` when it
/// is right (with an `AssertValid` that passes), else what is wrong.
fn rods(fixed: bool, seam: f64, a_first: bool) -> Option<String> {
    println!("rods fix {fixed} seam {seam} a_first {a_first}");
    let mut k = kernel(fixed);
    let c = cylinder(&mut k, 44.45, 400.0, seam, 90.0, CHORD_START);
    let a = cylinder(&mut k, 24.15, 240.0, seam, 45.0, brace_start(45.0));
    let b = cylinder(&mut k, 24.15, 240.0, seam, -45.0, brace_start(-45.0));
    let (first, second) = if a_first { (a, b) } else { (b, a) };
    let joined = boolean_union(&mut k, c, first).unwrap();
    let mut r = boolean_union(&mut k, joined, second).unwrap();
    if let Some(w) = wrong(&k, &mut r, RODS_VOLUME) {
        return Some(w);
    }
    // some orders report an edge/vertex gap of 5.0e-6 or 5.1e-6 against the tolerance
    // 5e-6 (`SmBrep` tests 0 and 2), as the C++ does; `AssertValid` passes them
    let check = check_brep(&mut k, &mut r, CheckLevel::Level2).unwrap();
    (check.assert_valid != Some(true)).then(|| format!("check: {:?}", check.lines()))
}

/// Both orders at every seam, flag off (SMLib v1.0.1) and on: the whole body, the volume
/// of #868's closed form.
#[test]
fn equal_rods_join_in_either_order() {
    let mut failures = Vec::new();
    for fixed in [false, true] {
        for seam in SEAMS {
            for a_first in [true, false] {
                if let Some(w) = rods(fixed, seam, a_first) {
                    let order = if a_first {
                        "(C u A) u B"
                    } else {
                        "(C u B) u A"
                    };
                    failures.push(format!("fix {fixed} seam {seam} {order}: {w}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The tubes at `seam`: the chord and both braces when `coped` is false (the braces
/// joined as they are, `(chord + A) + B`), else the braces each minus the chord's solid,
/// joined, then the chord joined with them. `None` when the result is right.
fn tubes(fixed: bool, seam: f64, coped: bool) -> Option<String> {
    println!("tubes fix {fixed} seam {seam} coped {coped}");
    let mut k = kernel(fixed);
    let chord = tube(&mut k, 44.45, 39.45, 400.0, seam, 90.0, CHORD_START);
    let brace = |k: &mut Kernel, y_deg: f64| {
        let b = tube(k, 24.15, 20.15, 240.0, seam, y_deg, brace_start(y_deg));
        if !coped {
            return b;
        }
        let solid = cylinder(k, 44.45, 400.0, seam, 90.0, CHORD_START);
        boolean_difference(k, b, solid).unwrap()
    };
    let a = brace(&mut k, 45.0);
    let b = brace(&mut k, -45.0);
    let (mut r, expected) = if coped {
        let braces = boolean_union(&mut k, a, b).unwrap();
        (boolean_union(&mut k, chord, braces).unwrap(), COPED_VOLUME)
    } else {
        let joined = boolean_union(&mut k, chord, a).unwrap();
        (boolean_union(&mut k, joined, b).unwrap(), TUBES_VOLUME)
    };
    wrong(&k, &mut r, expected)
}

/// The steel-026 joint as #868 builds it, at every seam, flag off and on: a manifold
/// solid of the closed form's volume. (Its check reports `SmBrep` test 12, an inner shell
/// not inside its outer shell, at every seam, flag off and on, as the C++ does.)
#[test]
fn steel_026_tubes_join() {
    let mut failures = Vec::new();
    for fixed in [false, true] {
        for seam in SEAMS {
            if let Some(w) = tubes(fixed, seam, false) {
                failures.push(format!("fix {fixed} seam {seam}: {w}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The seams at which the coped joint fails, flag off and on, on both engines.
const COPED_FAILING: [f64; 2] = [35.0, 90.0];

/// The coped joint at the other seams, flag off and on (check as in
/// [`steel_026_tubes_join`]).
#[test]
fn coped_tubes_join() {
    let mut failures = Vec::new();
    for fixed in [false, true] {
        for seam in SEAMS.into_iter().filter(|s| !COPED_FAILING.contains(s)) {
            if let Some(w) = tubes(fixed, seam, true) {
                failures.push(format!("fix {fixed} seam {seam}: {w}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// At seams 35 and 90 deg the last union of the coped joint gives a body of volume
/// -4592.38 that is not a manifold solid (check: `SmTrimmingTools` tests 3 and 4 at 35,
/// `SmBrep` tests 8 and 12 at 90), with the flag off and on, and the pinned C++ gives the
/// same: not WR-490's defect. Fails when one of them comes out right.
#[test]
fn coped_tubes_join_known_failing() {
    let mut passing = Vec::new();
    for fixed in [false, true] {
        for seam in COPED_FAILING {
            if tubes(fixed, seam, true).is_none() {
                passing.push(format!("fix {fixed} seam {seam}"));
            }
        }
    }
    assert!(
        passing.is_empty(),
        "passes, though listed as failing: {passing:#?}"
    );
}
