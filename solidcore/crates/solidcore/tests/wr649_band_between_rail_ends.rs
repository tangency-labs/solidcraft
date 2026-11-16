//! Regression test for WR-649 (the fork's tangency-labs/smlib@d548305): with
//! `fillet_refuse_overlapping_bands` (WR-542) and `fillet_mitre_concave_face_corners`
//! (WR-642), a blend's band on a face is its rail between the rail's end vertices. The
//! rails are not trimmed yet when the bands are checked, and run on past the corners that end
//! them; sampled over the whole curve, the rail of SolidCraft's #871 part's plate-top round
//! on the end face, run on past the shoulder's mitre (y 13.244), crossed the band of the
//! window's y 14 round (rail y 13.5) at (23.5, 13.8999, 9.5), and r 0.5 was refused as no
//! room.
//!
//! The part: a 14 wide stepped bar with a window through it, all 24 edges of both end
//! faces rounded. The expected volume is the fork's closed form
//! `V0 - 2 L r^2 (1 - pi/4)` (L = 107.6, each end face's 12 edges; the six mitres' and
//! six convex corners' terms cancel at each end). At r 1 the rounds overlap across the
//! 1.26 wall and the fillet is still refused.
//!
//! The mirrored shell-rim refusals (solidcore#2903) live here too, at the end: their
//! captured bodies rerun SolidCraft's fused application case on each of the four mirror
//! seeds of the same shelled-box rim, both as captured and after the application edited
//! the box, and every newly built rim strip must carry its `ProceduralBlend` definition.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::definition_check::bare_blend_faces;
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{
    brep_volume, brep_volume_default, PropertyAttributes,
};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel, Vec3};

#[path = "support/mirrored_shell_rim.rs"]
mod mirrored_shell_rim;
use mirrored_shell_rim::{build_rim, edited_input, input, Kind};

const A: f64 = 12.744_373_655_385_26;
const B: f64 = 22.692_759_633_669_36;

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

fn boxed(k: &mut Kernel, lo: Point3, hi: Point3) -> Brep {
    create_box(k, lo, hi.x - lo.x, hi.y - lo.y, hi.z - lo.z).unwrap()
}

/// SolidCraft's #871 part: a base, a top on it, a window through both.
fn part(k: &mut Kernel) -> Brep {
    let base = boxed(k, p(9.5, 8.0, 0.0), p(23.5, 29.5, 10.0));
    let top = boxed(k, p(9.5, A, 10.0), p(23.5, B, 20.0));
    let joined = boolean_union(k, base, top).unwrap();
    let window = boxed(k, p(9.5, 14.0, 7.0), p(23.5, 19.2, 14.1));
    boolean_difference(k, joined, window).unwrap()
}

fn v0() -> f64 {
    14.0 * (21.5 * 10.0 + (B - A) * 10.0 - 5.2 * 7.1)
}

/// The (y, z) of a point on each of an end face's 12 edges.
const END_EDGE_POINTS: [(f64, f64); 12] = [
    (A, 15.0),
    (10.372_186_827_692_63, 10.0),
    (8.0, 5.0),
    (18.75, 0.0),
    (29.5, 5.0),
    (26.096_379_816_834_68, 10.0),
    (B, 15.0),
    (17.718_566_644_527_31, 20.0),
    (16.6, 7.0),
    (14.0, 10.55),
    (16.6, 14.1),
    (19.2, 10.55),
];

/// The edge of `b` through `q` (every edge of the part is a line segment).
fn edge_through(k: &Kernel, b: &Brep, q: Point3) -> EdgeId {
    let found: Vec<EdgeId> = b
        .edges()
        .iter()
        .filter(|(_, e)| {
            let c = e.curve.as_ref().unwrap();
            let s = c.evaluate_point(k, e.interval.min()).unwrap();
            let t = c.evaluate_point(k, e.interval.max()).unwrap();
            let d = t - s;
            let len2 = d.dot(d);
            let u = ((q - s).dot(d) / len2).clamp(0.0, 1.0);
            (s + d * u).distance(q) < 1e-6
        })
        .map(|(id, _)| id)
        .collect();
    assert_eq!(found.len(), 1, "one edge through {q:?}");
    found[0]
}

fn round_the_ends(r: f64, fixed: bool) -> Result<(usize, f64), String> {
    let mut k = Kernel::default();
    k.fixes.fillet_mitre_concave_face_corners = true;
    k.fixes.fillet_refuse_overlapping_bands = fixed;
    let mut b = part(&mut k);
    let mut ids = Vec::new();
    for x in [23.5, 9.5] {
        for &(y, z) in &END_EDGE_POINTS {
            ids.push(edge_through(&k, &b, p(x, y, z)));
        }
    }
    fillet_edges(&mut k, &mut b, &ids, r, 1, 1, 1.0).map_err(|e| format!("{e}"))?;
    assert!(
        is_manifold_solid(&b).unwrap(),
        "r {r}: not a manifold solid"
    );
    b.refresh_cached_topology();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "r {r}: check_brep: {lines:?}");
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(&k, &mut b, 1e-6, &attrs).unwrap();
    Ok((b.faces().ids().count(), v))
}

#[test]
fn wr649_end_face_rounds_mitred_at_concave_corners() {
    let r = 0.5;
    let (faces, v) = round_the_ends(r, true).expect("r 0.5 builds");
    let expected = v0() - 2.0 * 107.6 * r * r * (1.0 - PI / 4.0);
    println!("r {r}: {faces} faces, volume {v:.6} against {expected:.6}");
    assert_eq!(faces, 38);
    // the fork: 3874.348250 against 3874.348458 (the corners' curves are fitted
    // intersections)
    assert!(
        (v - expected).abs() <= 1e-7 * expected,
        "volume {v} against {expected} (diff {:e})",
        v - expected
    );
    // r 1: the rounds overlap across the 1.26 wall, still no room
    assert!(round_the_ends(1.0, true).is_err());
}

#[test]
fn wr649_without_the_band_check() {
    // Off, the band check does not run (upstream SMLib has none). r 0.5 builds the same
    // body, and r 1 is not refused (the body it builds is not a manifold solid).
    let (faces, v) = round_the_ends(0.5, false).expect("r 0.5 builds");
    assert_eq!(faces, 38);
    let (on_faces, on_v) = round_the_ends(0.5, true).expect("r 0.5 builds");
    assert_eq!((faces, v.to_bits()), (on_faces, on_v.to_bits()));
}

// === Mirrored shell-rim refusals (solidcore#2903) ===========================================
//
// The captured input is the application's exact pre-operation body: a 60x40x30
// extrusion, open at its top with an inward wall of 3, outer vertical rounds R6,
// inner vertical rounds R3, and an inner floor round R1. Synthetic reconstruction
// through ops does not reproduce the refusal, so retain this valid representation.
//
// The kernel is configured from the rig's (`kernel_cases/fixes.json`), so the review
// sees what every kernel case sees: no operation-specific fix set of its own.
//
// The inner planar contour has length L = 152 + 6*pi and total turning 2*pi.
// Integrating a section against its parallel-curve Jacobian (1 +/- curvature*u)
// gives volume L*area +/- 2*pi*`first_moment`. A round's removed section has area
// (1-pi/4)*r^2 and first moment (5/6-pi/4)*r^3. A chamfer's are d^2/2 and d^3/6.
// The floor adds material on the inward side; the rim removes it outward.

/// The original cavity's inner planar contour length.
fn perimeter() -> f64 {
    152.0 + 6.0 * PI
}

/// The original rounded shell's volume, including its inner floor round.
fn base_volume() -> f64 {
    let area = 1.0 - PI / 4.0;
    let moment = 5.0 / 6.0 - PI / 4.0;
    60.0 * 40.0 * 30.0 - 54.0 * 34.0 * 27.0 - 4.0 * area * 6.0 * 6.0 * 30.0
        + 4.0 * area * 3.0 * 3.0 * 27.0
        + perimeter() * area
        - 2.0 * PI * moment
}

/// Compare the measured volume with its independent closed form.
fn volume_is(k: &Kernel, b: &mut Brep, expected: f64) {
    let measured = brep_volume_default(k, b, 1e-10).unwrap();
    assert!(
        (measured - expected).abs() <= 1e-9 * expected.abs(),
        "volume {measured} against closed form {expected}"
    );
}

/// Blend one rim chain and check every new strip's definition or recorded exception.
fn rim(k: &mut Kernel, b: &mut Brep, point: Vec3, kind: Kind, size: f64) -> solidcore::Result<()> {
    let original_faces = b.face_list().to_vec();
    build_rim(k, b, point, kind, size)?;
    let strips: Vec<_> = b
        .face_list()
        .iter()
        .copied()
        .filter(|face| !original_faces.contains(face))
        .collect();
    assert_eq!(strips.len(), 8, "the eight newly built rim strips");
    let made = k
        .blend_face_log
        .as_ref()
        .map_or_else(Vec::new, |log| log.last().to_vec());
    assert!(
        !made.is_empty(),
        "{kind:?} at {point:?}: the blend-face log recorded no strip"
    );
    match kind {
        Kind::Round => {
            // Every strip is a definition: the round strips are exact-round
            // sections over revolute/cone surfaces, all ProceduralBlend.
            for face in strips {
                let surface = b.face(face)?.surface.as_ref().unwrap();
                assert!(
                    surface.procedural_blend().is_some(),
                    "{kind:?} at {point:?}: new strip {face:?} is not a ProceduralBlend"
                );
            }
            assert!(
                bare_blend_faces(b, &made)?.is_empty(),
                "no round strip is bare"
            );
        }
        Kind::Chamfer => {
            // Measured (the probe the review asked for, issue_2991): at the
            // once-refused corner the chamfer's four CONE strips are not a
            // definition yet (each a `Chamfer` whose surface kind is `cone`), and
            // the four plane strips are. The four are listed in the
            // procedural-blend ledger (`data/blend_definition_ledger/chamfer.md`).
            let bare = bare_blend_faces(b, &made)?;
            assert_eq!(bare.len(), 4, "{kind:?} at {point:?}: bare strips {bare:?}");
            for bare_face in &bare {
                assert_eq!(bare_face.kind.name(), "chamfer", "{bare_face:?}");
                assert_eq!(bare_face.surface, "cone", "{bare_face:?}");
            }
            for face in strips {
                if bare.iter().any(|bare_face| bare_face.face == face) {
                    continue;
                }
                let surface = b.face(face)?.surface.as_ref().unwrap();
                assert!(
                    surface.procedural_blend().is_some(),
                    "{kind:?} at {point:?}: new strip {face:?} is not a ProceduralBlend"
                );
            }
        }
    }
    Ok(())
}

/// The original capture is valid before any operation prepares its caches.
#[test]
fn captured_operand_is_valid_and_has_its_closed_form_volume() {
    let (mut k, mut b) = input();
    assert_eq!(
        (
            b.face_list().len(),
            b.edge_list().len(),
            b.vertex_list().len()
        ),
        (27, 64, 40)
    );
    assert!(is_manifold_solid(&b).unwrap());
    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "{:?}", check.lines());
    volume_is(&k, &mut b, base_volume());
}

/// Check all four original rim seeds against the same topology and closed form.
fn mirrors(kind: Kind) {
    let size: f64 = 0.47;
    let removed = match kind {
        Kind::Round => {
            perimeter() * (1.0 - PI / 4.0) * size * size
                + 2.0 * PI * (5.0 / 6.0 - PI / 4.0) * size * size * size
        }
        Kind::Chamfer => perimeter() * size * size / 2.0 + PI * size * size * size / 3.0,
    };
    let mut refused = Vec::new();
    for point in [
        Vec3::new(3.8786796564403563, 36.121320343559645, 30.0),
        Vec3::new(56.121320343559645, 36.121320343559645, 30.0),
        Vec3::new(3.8786796564403563, 3.8786796564403563, 30.0),
        Vec3::new(56.121320343559645, 3.8786796564403563, 30.0),
    ] {
        // Each seed starts from a fresh input. Measurements/checks on a different instance
        // above must not prepare or change this operation's operand before it runs.
        let (mut k, mut b) = input();
        if let Err(e) = rim(&mut k, &mut b, point, kind, size) {
            refused.push(format!("{kind:?} at {point:?}: {e}"));
            continue;
        }
        assert_eq!(
            (
                b.face_list().len(),
                b.edge_list().len(),
                b.vertex_list().len()
            ),
            (35, 80, 48)
        );
        assert!(is_manifold_solid(&b).unwrap());
        let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert!(check.is_valid(), "{point:?}: {:?}", check.lines());
        volume_is(&k, &mut b, base_volume() - removed);
    }
    assert!(refused.is_empty(), "{}", refused.join("\n"));
}

/// Every mirrored round seed builds all eight defined rim strips.
#[test]
fn a_round_builds_the_same_rim_from_all_four_mirror_seeds() {
    mirrors(Kind::Round);
}

/// Every mirrored chamfer seed preserves the four measured cone exceptions.
#[test]
fn a_chamfer_builds_the_same_rim_from_all_four_mirror_seeds() {
    mirrors(Kind::Chamfer);
}

// A second capture: the application Kernel APIs built `typical/filleted-shelled-box`
// after its height changes from 30 to 33: a centered 70x50 extrusion, outer vertical
// R6 rounds, then a top-open inward shell of 2 (inner vertical R4). Its inner contour
// has L = 192 + 8*pi. The matrix's unchanged blend size is 0.627, derived from the
// original display polylines. This tests the resulting geometry, not timeline
// reference resolution. No floor round is present in this second capture.

/// The edited shell's closed-form volume, with no floor round.
fn edited_base_volume() -> f64 {
    let area = 1.0 - PI / 4.0;
    // Rounded outer area times full height, less rounded cavity area times depth.
    (70.0 * 50.0 - 4.0 * area * 6.0 * 6.0) * 33.0 - (66.0 * 46.0 - 4.0 * area * 4.0 * 4.0) * 31.0
}

/// The edited capture independently satisfies validity and its closed-form volume.
#[test]
fn captured_edited_operand_is_valid_and_has_its_closed_form_volume() {
    let (mut k, mut b) = edited_input();
    assert_eq!(
        (
            b.face_list().len(),
            b.edge_list().len(),
            b.vertex_list().len()
        ),
        (19, 48, 32)
    );
    assert!(is_manifold_solid(&b).unwrap());
    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "{:?}", check.lines());
    volume_is(&k, &mut b, edited_base_volume());
}

/// Check all four edited rim seeds on fresh operands.
fn edited_mirrors(kind: Kind) {
    let size: f64 = 0.627;
    let length = 192.0 + 8.0 * PI;
    let removed = match kind {
        Kind::Round => {
            length * (1.0 - PI / 4.0) * size * size
                + 2.0 * PI * (5.0 / 6.0 - PI / 4.0) * size * size * size
        }
        Kind::Chamfer => length * size * size / 2.0 + PI * size * size * size / 3.0,
    };
    // Inner R4 corners are centered at (+/-29, +/-19).
    let diagonal = 4.0 / 2.0_f64.sqrt();
    let mut refused = Vec::new();
    for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
        let point = Vec3::new(sx * (29.0 + diagonal), sy * (19.0 + diagonal), 33.0);
        // Load a fresh operand before the operation; do not measure or check it first.
        let (mut k, mut b) = edited_input();
        if let Err(e) = rim(&mut k, &mut b, point, kind, size) {
            refused.push(format!("{kind:?} at {point:?}: {e}"));
            continue;
        }
        assert_eq!(
            (
                b.face_list().len(),
                b.edge_list().len(),
                b.vertex_list().len()
            ),
            (27, 64, 40)
        );
        assert!(is_manifold_solid(&b).unwrap());
        let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert!(check.is_valid(), "{point:?}: {:?}", check.lines());
        volume_is(&k, &mut b, edited_base_volume() - removed);
    }
    assert!(refused.is_empty(), "{}", refused.join("\n"));
}

/// The edited operand's round strips retain their definitions at every mirror seed.
#[test]
fn a_round_builds_the_edited_rim_from_all_four_mirror_seeds() {
    edited_mirrors(Kind::Round);
}

/// The edited operand's chamfers retain the measured definition disposition.
#[test]
fn a_chamfer_builds_the_edited_rim_from_all_four_mirror_seeds() {
    edited_mirrors(Kind::Chamfer);
}
