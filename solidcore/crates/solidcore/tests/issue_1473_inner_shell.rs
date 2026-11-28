//! Regression test for #1473: `SmShell::IsInnerShellOf` on a box with a sealed internal
//! void (the issue's repro: a 20 mm cube minus a 10 mm cube at (5, 5, 5)).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep as sm;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::kernel::Kernel;
use solidcore::math::det;
use solidcore::math::Vec3;
use solidcore::primitive::api_primitives::{create_box, create_sphere};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2::points_in_face;
use solidcore::topology::shell::{is_inner_shell_of, ShellMembers, SM_FALSE, SM_TRUE, SM_UNSURE};
use solidcore::topology::{brep, brep::p1, Brep, ShellId};

/// The largest |coordinate| of the shell's face vertices: 20 for the outer wall, 15 for
/// the cavity's.
fn shell_extent(b: &Brep, s: ShellId) -> f64 {
    let mut m: f64 = 0.0;
    for &f in &b.shell(s).unwrap().faces {
        for &l in &b.face(f).unwrap().loops {
            for le in &b.loop_(l).unwrap().edges {
                for v in b.edge(le.edge).unwrap().vertices {
                    let p = b.vertex(v).unwrap().point;
                    m = m.max(p.x.abs()).max(p.y.abs()).max(p.z.abs());
                }
            }
        }
    }
    m
}

fn void_body(k: &mut Kernel) -> Brep {
    let outer = create_box(k, Vec3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
    let cavity = create_box(k, Vec3::new(5.0, 5.0, 5.0), 10.0, 10.0, 10.0).unwrap();
    sm::boolean(k, outer, cavity, BooleanOperation::Difference)
        .unwrap()
        .result
}

fn check(k: &mut Kernel) {
    let mut b = void_body(k);
    let regions = p1::regions(&b, None).unwrap();
    let infinite = b.infinite_region();
    let mut solid_checked = false;
    for r in regions {
        let shells = b.region(r).unwrap().shells.clone();
        if Some(r) == infinite {
            // the infinite region's one shell (the 20 mm cube's outside) is an inner
            // shell of the infinite region
            assert_eq!(shells.len(), 1);
            assert_eq!(
                is_inner_shell_of(k, &mut b, shells[0], None).unwrap(),
                SM_TRUE
            );
            continue;
        }
        if shells.len() == 1 {
            // the void: another shell contains it
            assert_eq!(shell_extent(&b, shells[0]), 15.0);
            assert_eq!(
                is_inner_shell_of(k, &mut b, shells[0], None).unwrap(),
                SM_FALSE
            );
            continue;
        }
        // the solid region: the outer wall encloses the cavity's wall
        assert_eq!(shells.len(), 2);
        let (outer, cavity) = if shell_extent(&b, shells[0]) > shell_extent(&b, shells[1]) {
            (shells[0], shells[1])
        } else {
            (shells[1], shells[0])
        };
        assert_eq!(shell_extent(&b, outer), 20.0);
        assert_eq!(
            is_inner_shell_of(k, &mut b, cavity, Some(outer)).unwrap(),
            SM_TRUE
        );
        assert_eq!(
            is_inner_shell_of(k, &mut b, outer, Some(cavity)).unwrap(),
            SM_FALSE
        );
        solid_checked = true;
    }
    assert!(solid_checked, "no region with two shells");
}

#[test]
fn issue_1473_box_with_sealed_void() {
    let mut k = Kernel::default();
    check(&mut k);
}

/// WR-497: an outer face-use shell whose closure traversal fails (its face-use list is
/// empty, so there is no first face use). Upstream ignores the status and reads the
/// shell as not closed (FALSE); here the failure is UNSURE.
fn failed_closure(k: &mut Kernel) -> i32 {
    let mut b = void_body(k);
    let solid = p1::regions(&b, None)
        .unwrap()
        .into_iter()
        .find(|&r| b.region(r).unwrap().shells.len() == 2)
        .unwrap();
    let shells = b.region(solid).unwrap().shells.clone();
    let (outer, cavity) = if shell_extent(&b, shells[0]) > shell_extent(&b, shells[1]) {
        (shells[0], shells[1])
    } else {
        (shells[1], shells[0])
    };
    b.shell_mut(outer).unwrap().members = ShellMembers::Faceuses(Vec::new());
    is_inner_shell_of(k, &mut b, cavity, Some(outer)).unwrap()
}

#[test]
fn issue_1473_failed_closure_traversal_is_unsure() {
    let mut k = Kernel::default();
    assert_eq!(failed_closure(&mut k), SM_UNSURE);
}

// ---- the answer does not depend on what the kernel sampled before -------------------------

/// The distance from the cavity's axis at which the notches' corner edges stand.
const NOTCH_RADIUS: f64 = 6.0;
/// Half the height of the lowest notch; notch `i` is `1 + 0.03 * i` times as high, so no two
/// notches share a top or bottom plane.
const NOTCH_HALF_HEIGHT: f64 = 2.27;

/// A 20 mm cube about the origin less a ball of radius 2 at its centre (the cavity: one
/// spherical face, a shell of its own inside the cube's wall), with one notch per `(dx, dy)`
/// direction: a box cut out of the cube from outside whose vertical corner edge stands at
/// `NOTCH_RADIUS * (dx, dy)` for `|z|` below the notch's half height. A ray from the
/// cavity's face along its normal runs through the cube's centre line; at a notch's azimuth
/// it meets that corner EDGE when its slope `|z| / horizontal` is below half height /
/// `NOTCH_RADIUS`, and a face of the body otherwise. The inner-shell test refuses a ray that
/// meets an edge, and is unsure when it has refused all three rays of the cavity's one face.
fn notched_cavity_body(k: &mut Kernel, directions: &[(f64, f64)]) -> Brep {
    let mut body = create_box(k, Vec3::new(-10.0, -10.0, -10.0), 20.0, 20.0, 20.0).unwrap();
    for (i, &(dx, dy)) in directions.iter().enumerate() {
        let h = NOTCH_HALF_HEIGHT * (1.0 + 0.03 * i as f64);
        let (x0, y0) = (NOTCH_RADIUS * dx, NOTCH_RADIUS * dy);
        let xmin = if dx >= 0.0 { x0 } else { x0 - 20.0 };
        let ymin = if dy >= 0.0 { y0 } else { y0 - 20.0 };
        let notch = create_box(k, Vec3::new(xmin, ymin, -h), 20.0, 20.0, 2.0 * h).unwrap();
        body = sm::boolean_difference(k, body, notch).unwrap();
    }
    let ball = create_sphere(k, Vec3::new(0.0, 0.0, 0.0), 2.0).unwrap();
    sm::boolean_difference(k, body, ball).unwrap()
}

/// The cavity's shell (one face) and the outer wall's shell, both of the material region.
fn cavity_and_outer(b: &Brep) -> (ShellId, ShellId) {
    let infinite = b.infinite_region();
    for r in p1::regions(b, None).unwrap() {
        let shells = b.region(r).unwrap().shells.clone();
        if Some(r) != infinite && shells.len() == 2 {
            let faces = |s: ShellId| b.shell(s).unwrap().faces.len();
            return if faces(shells[0]) < faces(shells[1]) {
                (shells[0], shells[1])
            } else {
                (shells[1], shells[0])
            };
        }
    }
    panic!("no region with two shells");
}

/// The horizontal directions of the cavity face's three sample rays, each with its
/// opposite (the ray runs along the face use's normal, either way). The sampler places its
/// three points on three fixed constant-u lines of the sphere, which are its meridians;
/// only the height along each is dithered, so the azimuths are the same on every call.
/// Taken on a kernel and an un-notched body of their own.
fn sample_ray_azimuths() -> Vec<(f64, f64)> {
    let mut k = Kernel::default();
    let mut b = notched_cavity_body(&mut k, &[]);
    let (cavity, _) = cavity_and_outer(&b);
    let face = b.shell(cavity).unwrap().faces[0];
    let points = points_in_face(&mut k, &mut b, face, 3).unwrap();
    let mut azimuths = Vec::new();
    for normal in points.pv.iter().skip(1).step_by(2) {
        let horizontal = det::hypot(normal.x, normal.y);
        assert!(horizontal > 0.5, "a sample at a pole: {normal:?}");
        azimuths.push((normal.x / horizontal, normal.y / horizontal));
        azimuths.push((-normal.x / horizontal, -normal.y / horizontal));
    }
    assert_eq!(azimuths.len(), 6);
    azimuths
}

/// The cavity lies inside the cube's wall, so the answer is TRUE; it is the same on every
/// one of 40 calls on one kernel and one untouched body. Before each call drew its own
/// samples, the rays of a call depended on the kernel's earlier draws, some calls had all
/// three rays refused at the notches' edges, and those answered UNSURE.
#[test]
fn inner_shell_answer_is_the_same_on_every_call() {
    let azimuths = sample_ray_azimuths();
    let mut k = Kernel::default();
    let mut b = notched_cavity_body(&mut k, &azimuths);
    let (cavity, outer) = cavity_and_outer(&b);
    let answers: Vec<i32> = (0..40)
        .map(|_| is_inner_shell_of(&mut k, &mut b, cavity, Some(outer)).unwrap())
        .collect();
    assert_eq!(answers, vec![SM_TRUE; 40]);
    // and the other way round, which no ray decides differently
    assert_eq!(
        is_inner_shell_of(&mut k, &mut b, outer, Some(cavity)).unwrap(),
        SM_FALSE
    );
}

/// The same body through the validity check, whose inner-shell test fails unless the
/// answer is TRUE: valid on every one of 16 checks of one untouched body.
#[test]
fn check_brep_of_a_body_with_a_cavity_is_the_same_on_every_call() {
    let azimuths = sample_ray_azimuths();
    let mut k = Kernel::default();
    let mut b = notched_cavity_body(&mut k, &azimuths);
    for n in 0..16 {
        let report = check_brep(&mut k, &mut b, CheckLevel::Level0).unwrap();
        assert!(report.is_valid(), "check {n}: {:?}", report.lines());
    }
}

/// Sampling calls on another body between the calls change nothing: the untouched body
/// answers TRUE after any number of them.
#[test]
fn inner_shell_answer_does_not_depend_on_unrelated_sampling() {
    let azimuths = sample_ray_azimuths();
    let mut k = Kernel::default();
    let mut b = notched_cavity_body(&mut k, &azimuths);
    let (cavity, outer) = cavity_and_outer(&b);
    let mut other = create_box(&mut k, Vec3::new(50.0, 50.0, 50.0), 1.0, 2.0, 3.0).unwrap();
    let other_face = brep::faces(&other, None).unwrap()[0];
    for n in 0..24 {
        assert_eq!(
            is_inner_shell_of(&mut k, &mut b, cavity, Some(outer)).unwrap(),
            SM_TRUE,
            "after {n} unrelated sampling calls"
        );
        points_in_face(&mut k, &mut other, other_face, 3).unwrap();
    }
}
