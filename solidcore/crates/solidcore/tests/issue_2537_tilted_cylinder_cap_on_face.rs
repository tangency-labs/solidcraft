//! A box minus a tilted cylinder whose top end cap is centred on the box's top face
//! plane (#2537): the cut must give one closed solid of the closed-form volume.
//!
//! SolidCraft reports the result check refusing this cut with `SmFace::Test #36`
//! ("Face Surface Seam has suspicious Edge near seam") twice, and the C++ fork
//! refusing it with eight `Bad Gap` failures besides. The cap plane, the cylinder and
//! the box's face all meet at the point where the seam ends, so the seam's end sits on
//! three surfaces at once; 1 um of clearance makes the cut pass.
//!
//! Built from solidcore's own primitives the cut passes, so this pins the geometry
//! rather than reproducing that refusal (`plan/findings/`
//! `boolean.api_brep--2537-tilted-cylinder-cap-on-a-box-face.md`). What it holds is the
//! volume: the whole family of cap offsets has a closed form, and three offsets of it
//! are checked here, including the degenerate one, with the tool built both as a
//! primitive cylinder and the way the issue builds it, a full-turn revolve.
//!
//! The case: the box 198 x 105.5 x 71.5 with its lower left corner at (-99, -6.5, -55),
//! minus the cylinder of radius 8 whose axis is the unit of the issue's sketch `y_dir`
//! [0.069857, 0, 0.997557] (4.005773878 degrees from z), whose seam runs along world
//! +y and whose top cap centre sits `offset` mm along that axis above the box's top
//! face plane z = 16.5. Its bottom cap centre stays 1.995 mm below the box's bottom
//! face, so the box's face does the cutting there and that cap never takes part.
//!
//! The same case is in the kernel-rig corpus as
//! `tests/kernel_cases/booleans/2537-box-minus-tilted-cylinder-cap-on-top-face`, whose
//! `expected.method` carries the derivation of the `offset` = 0 volume.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::field_reassign_with_default
)] // test

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives as api;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::Brep;
use solidcore::Kernel;
use std::f64::consts::PI;

/// Tool radius.
const R: f64 = 8.0;
/// Axial length of the tool at `offset` 0 (the issue's sketch rectangle).
const H0: f64 = 73.675097;
/// The sketch parameter at which the issue's profile reaches the top cap.
const S_HI: f64 = 15.692826;
/// The box, as the issue builds it.
const BOX_LO: Point3 = Point3::new(-99.0, -6.5, -55.0);
const BOX_SIZE: (f64, f64, f64) = (198.0, 105.5, 71.5);
/// The box's top face plane, on which the degenerate cap centre sits.
const TOP_Z: f64 = 16.5;
/// The integrator runs at accuracy 1e-7; `boolean_api.rs` grades volumes at 1e-6.
const VOLUME_REL_TOL: f64 = 1e-6;

/// How the tool body is built.
#[derive(Clone, Copy, Debug)]
enum Tool {
    /// `create_cylinder` placed by a rigid transform.
    Primitive,
    /// The issue's own path: the profile rectangle revolved a full turn.
    Revolve,
}

/// Sine and cosine of the tilt: the unit of the issue's sketch `y_dir`.
fn axis_sin_cos() -> (f64, f64) {
    let (dx, dz) = (0.069857_f64, 0.997557_f64);
    let n = (dx * dx + dz * dz).sqrt();
    (dx / n, dz / n)
}

/// The volume the cut must leave, for a cap centre `offset` mm up the axis from the
/// box's top face plane.
///
/// With (u, a, b) on the axis (u along it from the cap centre, a along the seam, which
/// is world +y, b = axis x a), the box's faces bound u to
/// `[-L - k b - offset, -k b - offset]` and the tool's cap plane bounds it to `u <= 0`,
/// where `L = 71.5/cos(t)` is the box's height along the axis and `k = tan(t)`. The
/// axis lies in the plane y = -6.5, the box's own `y_min` face, so the region in y is the
/// half disc `a` in `[0, sqrt(R^2 - b^2)]` and (u, a, b) is orthonormal. That leaves
/// the cut as half a slanted cylinder through the slab, `32 pi L`, less the ungula the
/// tilted cap cuts off it where it dips below the face, `b < -offset/k`:
///
/// ```text
/// ungula(offset) = integral((-offset - k b) sqrt(R^2 - b^2) db, -R .. -offset/k)
///                = -offset [(c/2) sqrt(R^2-c^2) + (R^2/2) asin(c/R) + (pi/4) R^2]
///                  + (k/3) (R^2 - c^2)^(3/2),            c = -offset/k
/// ```
///
/// Past `offset = R k` the whole cap is clear of the face and the ungula is zero.
fn expected_volume(offset: f64) -> f64 {
    let (sin_t, cos_t) = axis_sin_cos();
    let k = sin_t / cos_t;
    let slab = 32.0 * PI * (71.5 / cos_t);
    let ungula = if offset >= R * k {
        0.0
    } else {
        let c = -offset / k;
        let a = c / 2.0 * (R * R - c * c).sqrt() + R * R / 2.0 * (c / R).asin() + PI / 4.0 * R * R;
        -offset * a + k / 3.0 * (R * R - c * c).powf(1.5)
    };
    BOX_SIZE.0 * BOX_SIZE.1 * BOX_SIZE.2 - slab + ungula
}

fn the_box(k: &mut Kernel) -> Brep {
    api::create_box(k, BOX_LO, BOX_SIZE.0, BOX_SIZE.1, BOX_SIZE.2).unwrap()
}

/// The top cap centre for a cap `offset` mm up the axis from the box's top face.
fn cap_centre(offset: f64) -> Point3 {
    let (s, c) = axis_sin_cos();
    Point3::new(-12.07395 + s * S_HI, -6.5, TOP_Z + offset * c)
}

/// The tool: radius 8, tilted, seam along world +y, its top cap centre `offset` mm up
/// the axis from z = 16.5 and its bottom cap centre always at the same place.
fn the_tool(k: &mut Kernel, tool: Tool, offset: f64) -> Brep {
    let (s, c) = axis_sin_cos();
    let axis = Vec3::new(s, 0.0, c);
    let h = H0 + offset;
    let top = cap_centre(offset);
    match tool {
        Tool::Primitive => {
            // Local: the top cap centre at the origin, the body from z = -h to 0, seam
            // along local +x. The placement maps that origin onto the cap centre
            // exactly, so the degeneracy is exact and not a rounded rotation.
            let mut b = api::create_cylinder(k, Point3::new(0.0, 0.0, -h), R, h).unwrap();
            // x -> world +y (the seam), and x cross y = the axis, pointing up.
            let pl = Axis2Placement::new(top, Vec3::new(0.0, 1.0, 0.0), Vec3::new(-c, 0.0, s));
            solidcore::topology::brep::p5::transform(k, &mut b, &pl, None, false).unwrap();
            b
        }
        Tool::Revolve => {
            // The issue's sketch: x along world +y, y along the axis, the profile the
            // rectangle R wide from the axis, revolved a full turn about the axis.
            let origin = Point3::new(top.x - s * S_HI, top.y, top.z - c * S_HI);
            let x_dir = Vec3::new(0.0, 1.0, 0.0);
            let at = |sx: f64, sy: f64| origin + x_dir * sx + axis * sy;
            let (lo, hi) = (S_HI - h, S_HI);
            let cs = [
                Curve::from(create_line_segment(at(0.0, lo), at(R, lo)).unwrap()),
                Curve::from(create_line_segment(at(R, lo), at(R, hi)).unwrap()),
                Curve::from(create_line_segment(at(R, hi), at(0.0, hi)).unwrap()),
                Curve::from(create_line_segment(at(0.0, hi), at(0.0, lo)).unwrap()),
            ];
            let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
            create_rotational_sweep(k, &refs, origin, axis, 360.0, false).unwrap()
        }
    }
}

/// Cuts the tool out of the box and returns the result's volume and face count.
fn cut(tool: Tool, offset: f64, fixes: &UpstreamFixes) -> (f64, usize) {
    let mut k = Kernel::default();
    k.fixes = *fixes;
    let a = the_box(&mut k);
    let b = the_tool(&mut k, tool, offset);
    let mut brep = boolean_difference(&mut k, a, b)
        .unwrap_or_else(|e| panic!("{tool:?} offset {offset}: the cut was refused: {e}"));
    solidcore::topology::integrity::check(&brep).unwrap();
    assert!(
        solidcore::topology::brep::p2::is_manifold_solid(&brep).unwrap(),
        "{tool:?} offset {offset}: the result is not a closed manifold solid"
    );
    // Level2 runs face test 36, the one SolidCraft reports refusing this cut.
    let check = check_brep(&mut k, &mut brep, CheckLevel::Level2).unwrap();
    assert!(
        check.is_valid(),
        "{tool:?} offset {offset}: Level2 check: {:?}",
        check.reports
    );
    let faces = solidcore::topology::brep::p1::faces(&brep, None)
        .unwrap()
        .len();
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let v = brep_volume(&k, &mut brep, 1e-7, &attrs).unwrap();
    let want = expected_volume(offset);
    let rel = (v - want).abs() / want.abs();
    // Printed so a run reports what it measured, not only that it passed.
    println!(
        "offset {offset:<6} {tool:<12} faces {faces}  volume {v:.6}  \
         closed form {want:.6}  rel {rel:.1e}",
        tool = format!("{tool:?}")
    );
    assert!(
        rel <= VOLUME_REL_TOL,
        "{tool:?} offset {offset}: volume {v} vs the closed form {want} (rel {rel:.2e})"
    );
    (v, faces)
}

/// The case of the issue: the cap centre exactly on the box's top face plane, so the
/// cap is bisected by that face and the seam's end lies on cylinder, cap plane and face
/// at once. One closed solid of 1486369.884 mm3, not a refusal, however the tool is
/// built and whichever upstream fixes are on (SolidCraft runs all but its held-back
/// ones).
#[test]
fn cap_centre_on_the_top_face_leaves_one_closed_solid() {
    let want = expected_volume(0.0);
    assert!(
        (want - 1486369.8843794577).abs() < 1e-7,
        "the closed form at offset 0 is the corpus entry's value, got {want}"
    );
    assert_eq!(cap_centre(0.0).z, TOP_Z, "the degeneracy must be exact");
    for tool in [Tool::Primitive, Tool::Revolve] {
        for fixes in [UpstreamFixes::default(), UpstreamFixes::all()] {
            let (_, faces) = cut(tool, 0.0, &fixes);
            assert_eq!(
                faces, 10,
                "{tool:?}: the box's 6 faces, the cut wall, its cap and the rims"
            );
        }
    }
}

/// The neighbours of that degeneracy, as the issue measures them: 1 um of clearance
/// (where the cap still dips below the face, so the ungula is all but unchanged) and
/// 2 mm (where the whole cap is clear and the box's own faces do all the cutting).
#[test]
fn neighbouring_cap_offsets_follow_the_same_closed_form() {
    for offset in [0.001, 2.0] {
        for tool in [Tool::Primitive, Tool::Revolve] {
            cut(tool, offset, &UpstreamFixes::all());
        }
    }
    // The cap plane spares material only where it dips below the face, so the higher
    // the cap sits the more the cut takes: the volume left falls with the offset, and
    // settles for good once the whole cap is clear of the face.
    let (sin_t, cos_t) = axis_sin_cos();
    let clear = R * sin_t / cos_t;
    assert!(expected_volume(0.0) > expected_volume(0.001));
    assert!(expected_volume(0.001) > expected_volume(clear));
    assert_eq!(expected_volume(clear), expected_volume(2.0));
}
