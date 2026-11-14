//! Fixes of the fork's fillet defects. WR-300 (#1023, #421), WR-303 (#841) and WR-320
//! (#879) are opt-in (`Kernel::fixes`): their cases run with the field off (SMLib's
//! behaviour) and on. WR-301 (#485) and WR-302 (#841, #1024, with the cliff overflow,
//! WR-631) are the default: their cases run on the default kernel.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::p3::NoRoomReason;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::FilletErrorType;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

fn kernel(set: impl FnOnce(&mut UpstreamFixes)) -> Kernel {
    let mut k = Kernel::default();
    set(&mut k.fixes);
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The edge of `b` whose ends are `p` and `q` (either way round).
fn edge_between(k: &Kernel, b: &Brep, p: [f64; 3], q: [f64; 3]) -> EdgeId {
    let near = |a: Point3, c: [f64; 3]| {
        (a.x - c[0]).abs() < 1e-6 && (a.y - c[1]).abs() < 1e-6 && (a.z - c[2]).abs() < 1e-6
    };
    b.edge_list()
        .iter()
        .copied()
        .find(|&e| {
            let curve = edge::curve(b, e).unwrap().unwrap();
            let ivl = edge::interval(b, e).unwrap();
            let s = curve.evaluate_point(k, ivl.min()).unwrap();
            let t = curve.evaluate_point(k, ivl.max()).unwrap();
            (near(s, p) && near(t, q)) || (near(s, q) && near(t, p))
        })
        .unwrap_or_else(|| panic!("no edge {p:?} - {q:?}"))
}

/// What a fillet came to: its volume and whether it is a manifold solid, or the error
/// code the executive recorded (`SmFilletErrorType`).
#[derive(Debug, PartialEq)]
enum Outcome {
    Built { volume: f64, manifold: bool },
    Refused { code: i32 },
}

/// Stitch what a session built, or report the executive's error code.
fn outcome(k: &mut Kernel, b: &mut Brep, result: Result<(), i32>) -> Outcome {
    if let Err(code) = result {
        return Outcome::Refused { code };
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    Outcome::Built {
        volume: volume(k, b),
        manifold: is_manifold_solid(b).unwrap(),
    }
}

/// `SmApiFilletEdges` with a constant radius, reporting the executive's error code.
fn fillet(k: &mut Kernel, b: &mut Brep, edges: &[EdgeId], radius: f64, xsect: u32) -> Outcome {
    let result = {
        let mut session = FilletSession::new(k, b).unwrap();
        session
            .prepare_constant_radius_edges(k, edges, radius, xsect, 1, 1.0)
            .unwrap();
        session.do_fillet(k).map_err(|_| session.error_info().code)
    };
    outcome(k, b, result)
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

// ---- WR-301 / WR-302: the cube of 20 with three openings (SolidCraft rig fillets/1080) ----

/// A cube of 20 with three orthogonal 10 x 10 openings through it, 5 in from its sides.
fn holed_cube(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
    for (o, d) in [
        ([-1.0, 5.0, 5.0], [22.0, 10.0, 10.0]),
        ([5.0, -1.0, 5.0], [10.0, 22.0, 10.0]),
        ([5.0, 5.0, -1.0], [10.0, 10.0, 22.0]),
    ] {
        let tool = create_box(k, Point3::new(o[0], o[1], o[2]), d[0], d[1], d[2]).unwrap();
        b = boolean_difference(k, b, tool).unwrap();
    }
    b
}

/// The top front edge (y = 0, z = 20) chamfered (`xsect` 0) or rounded (1) by `size`.
fn holed_cube_blend(k: &mut Kernel, size: f64, xsect: u32) -> Outcome {
    let mut b = holed_cube(k);
    let e = edge_between(k, &b, [0.0, 0.0, 20.0], [20.0, 0.0, 20.0]);
    fillet(k, &mut b, &[e], size, xsect)
}

/// A blend whose setback reaches exactly the openings' rims (5): SMLib's rollover fails
/// on the rail touching the rim's corners and running along its top edge. Joining the
/// rail's intervals on its face first, it builds, to the closed forms 4000 - 12.5 * 20
/// (chamfer) and 4000 - (1 - pi/4) * 25 * 20 (round).
#[test]
fn rail_along_its_face_rim_rolls_over_only_where_it_leaves_the_face() {
    for (xsect, expected) in [
        (0, 3750.0),
        (1, 4000.0 - (1.0 - std::f64::consts::FRAC_PI_4) * 500.0),
    ] {
        let mut k = Kernel::default();
        let Outcome::Built { volume, manifold } = holed_cube_blend(&mut k, 5.0, xsect) else {
            panic!("xsect {xsect}: refused");
        };
        assert!(
            manifold && near(volume, expected, 1e-4),
            "xsect {xsect}: {volume}"
        );
    }
}

/// A size that leaves the rims alone (4) takes the closed forms 4000 - 8 * 20 (chamfer)
/// and 4000 - (1 - pi/4) * 16 * 20 (round).
#[test]
fn rail_clear_of_the_rim_builds_to_the_closed_form() {
    for (xsect, expected) in [
        (0, 3840.0),
        (1, 4000.0 - (1.0 - std::f64::consts::FRAC_PI_4) * 320.0),
    ] {
        let mut k = Kernel::default();
        let Outcome::Built { volume, manifold } = holed_cube_blend(&mut k, 4.0, xsect) else {
            panic!("xsect {xsect}: refused");
        };
        assert!(
            manifold && near(volume, expected, 1e-4),
            "xsect {xsect}: {volume}"
        );
    }
}

// ---- WR-303: the end of a round that crosses more than one face at a three-edge vertex ----

// ---- WR-300: a variable round whose rail ends at a rounded corner's arc ----

/// `FilletErrorType::NoRoom`, the fork's error type for a blend larger than its faces.
const NO_ROOM: i32 = FilletErrorType::NoRoom as i32;

/// The section a blend of `size` (`xsect` 0 chamfer, 1 round) takes off the holed cube's
/// top front edge, clipped to the body, in closed form. In the section plane, `y` from the
/// front face and `w` down from the top, the blend's section is the triangle `y + w <= d`
/// or the round's spandrel outside the circle of radius `r` about `(r, r)`. Outside the
/// openings' 10 wide span along the edge, the opening along x takes `y >= 5, w >= 5`,
/// which no section of size under 10 reaches; across it, the openings along y and z take
/// `w >= 5` and `y >= 5`, leaving the section's part in the square `[0, 5)^2`.
fn holed_cube_removed(size: f64, xsect: u32) -> f64 {
    let (whole, past) = if xsect == 0 {
        // the triangle's part at `y >= 5`: the triangle of legs `size - 5`
        let p = (size - 5.0).max(0.0);
        (size * size / 2.0, p * p / 2.0)
    } else {
        // the spandrel's part at `y >= 5`: the integral over `t = r - y` in `0..r - 5` of
        // `r - sqrt(r^2 - t^2)`
        let r = size;
        let a = (r - 5.0).max(0.0);
        let f = (a * (r * r - a * a).sqrt() + r * r * (a / r).asin()) / 2.0;
        (r * r * (1.0 - std::f64::consts::FRAC_PI_4), r * a - f)
    };
    10.0 * whole + 10.0 * (whole - 2.0 * past)
}

/// Blends of 6 and 9 run across the openings: SMLib builds a body that is no manifold
/// solid, and the fork's first check refused them (`NoRoom`, WR-302). Their rails leave
/// the top and front faces across the openings' rims (cliffs), so each is built as a cliff
/// overflow (WR-631): the holed cube (4000) less the blend's section clipped to the body
/// ([`holed_cube_removed`]).
#[test]
fn blends_across_the_openings_are_cliff_overflows() {
    for xsect in [0, 1] {
        for size in [6.0, 9.0] {
            let mut k = Kernel::default();
            let expected = 4000.0 - holed_cube_removed(size, xsect);
            let mut b = holed_cube(&mut k);
            let e = edge_between(&k, &b, [0.0, 0.0, 20.0], [20.0, 0.0, 20.0]);
            let Outcome::Built { volume, manifold } = fillet(&mut k, &mut b, &[e], size, xsect)
            else {
                panic!("xsect {xsect} size {size}: refused");
            };
            let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
                .unwrap()
                .lines();
            assert!(lines.is_empty(), "xsect {xsect} size {size}: {lines:?}");
            assert!(
                manifold && near(volume, expected, 1e-9 * expected),
                "xsect {xsect} size {size}: {volume} against {expected} ({:e})",
                (volume - expected).abs() / expected
            );
        }
    }
}

// ---- WR-320: blends as wide as the face they roll on ----

/// A cylinder of radius 10 and height 20, its top rim blended by `size`.
fn cylinder_rim(k: &mut Kernel, size: f64, xsect: u32) -> Outcome {
    let mut b = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0).unwrap();
    let rim = *b
        .edge_list()
        .iter()
        .find(|&&e| {
            let c = edge::curve(&b, e).unwrap().unwrap();
            let ivl = edge::interval(&b, e).unwrap();
            let p = c.evaluate_point(k, ivl.min()).unwrap();
            (p.z - 20.0).abs() < 1e-6 && (p.x.hypot(p.y) - 10.0).abs() < 1e-6
        })
        .unwrap();
    fillet(k, &mut b, &[rim], size, xsect)
}

/// Blends as wide as the face they roll on: the chamfer's cone would end in a point at the
/// top's centre, where SMLib goes on (the fork's `FacesOutlastTheirBlends`, WR-320). With
/// `fillet_refuse_face_consuming_blends` a chamfer of 10 on the top rim of a cylinder of
/// radius 10 is refused (`NoRoom`). A round of 10 closes the top as a hemisphere
/// tangent to it, as the fork's amendment (tangency-labs/smlib `8bffb23`) lets it, and
/// builds either way, as does everything below the full width.
#[test]
fn chamfer_as_wide_as_its_face_is_refused() {
    let mut k = kernel(|f| f.fillet_refuse_face_consuming_blends = true);
    assert_eq!(
        cylinder_rim(&mut k, 10.0, 0),
        Outcome::Refused { code: NO_ROOM }
    );
    let mut k = Kernel::default();
    assert!(!matches!(
        cylinder_rim(&mut k, 10.0, 0),
        Outcome::Refused { code: NO_ROOM }
    ));

    // the round of 10: the cylinder (pi 100 10) and a hemisphere (2/3 pi 1000)
    let hemisphere = std::f64::consts::PI * (1000.0 + 2000.0 / 3.0);
    for on in [false, true] {
        let mut k = kernel(|f| f.fillet_refuse_face_consuming_blends = on);
        let Outcome::Built { volume, manifold } = cylinder_rim(&mut k, 10.0, 1) else {
            panic!("on {on}: refused");
        };
        assert!(
            manifold && near(volume, hemisphere, 5e-3),
            "on {on}: {volume}"
        );
        for xsect in [0, 1] {
            assert!(
                matches!(
                    cylinder_rim(&mut k, 9.99, xsect),
                    Outcome::Built { manifold: true, .. }
                ),
                "on {on} xsect {xsect}"
            );
        }
    }
}

// ---- #960: the refusals named, not only commented ----

/// What a refused blend's error info names (`ErrorInfo::no_room_reason`).
fn no_room_reason(
    k: &mut Kernel,
    b: &mut Brep,
    edges: &[EdgeId],
    size: f64,
) -> Option<NoRoomReason> {
    let mut session = FilletSession::new(k, b).unwrap();
    session
        .prepare_constant_radius_edges(k, edges, size, 0, 1, 1.0)
        .unwrap();
    assert!(session.do_fillet(k).is_err(), "size {size}: built");
    session.error_info().no_room_reason()
}

/// The `NoRoom` refusals name themselves (#960): a blend taking a whole face (the
/// cylinder's chamfer of 10), where the comment was the only difference; a refusal under
/// another code names none. The holed cube's chamfer of 5, once refused under another code
/// (WR-301's rollover), builds. (The holed cube's blend of 6, once named as a rail leaving
/// its face, and its chamfer of 25, once refused under another code, are now cliff
/// overflows: `blends_across_the_openings_are_cliff_overflows` and below.)
#[test]
fn no_room_refusals_are_named() {
    let mut k = kernel(|f| f.fillet_refuse_face_consuming_blends = true);
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0).unwrap();
    let rim = *b
        .edge_list()
        .iter()
        .find(|&&e| {
            let c = edge::curve(&b, e).unwrap().unwrap();
            let ivl = edge::interval(&b, e).unwrap();
            let p = c.evaluate_point(&k, ivl.min()).unwrap();
            (p.z - 20.0).abs() < 1e-6 && (p.x.hypot(p.y) - 10.0).abs() < 1e-6
        })
        .unwrap();
    assert_eq!(
        no_room_reason(&mut k, &mut b, &[rim], 10.0),
        Some(NoRoomReason::FaceConsumed)
    );

    // the holed cube's chamfer of 5, whose rollover failed (SM_FILERR_EDGE_PROBLEM) before
    // WR-301 became the default: 4000 - 12.5 * 20
    let mut k = Kernel::default();
    let Outcome::Built { volume, manifold } = holed_cube_blend(&mut k, 5.0, 0) else {
        panic!("size 5: refused");
    };
    assert!(manifold && near(volume, 3750.0, 1e-4), "size 5: {volume}");

    // a chamfer of 25 on the cube of 20, once refused under another code
    // (SM_FILERR_BAD_INT), is a cliff overflow: the plane y + w = 25 (y from the front, w
    // down from the top) cuts the body. Outside the openings' span (10 along x) the section
    // is the 20 square less the opening along x, [5, 15]^2: the triangle takes
    // 312.5 - 2 * 12.5 of the square and 100 - 12.5 of the opening, 200. Across the span the
    // section is the square's four 5 x 5 corners: the triangle takes the three nearest the
    // edge, 75. 4000 - 10 * 200 - 10 * 75 = 1250.
    let mut k = Kernel::default();
    let mut b = holed_cube(&mut k);
    let e = edge_between(&k, &b, [0.0, 0.0, 20.0], [20.0, 0.0, 20.0]);
    let Outcome::Built { volume, manifold } = fillet(&mut k, &mut b, &[e], 25.0, 0) else {
        panic!("size 25: refused");
    };
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "size 25: {lines:?}");
    assert!(
        manifold && near(volume, 1250.0, 1e-9 * 1250.0),
        "size 25: {volume}"
    );
}
