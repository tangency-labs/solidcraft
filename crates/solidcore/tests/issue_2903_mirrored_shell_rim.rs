//! SolidCraft's mirrored shell-rim refusals (solidcore#2903).
//!
//! The captured input is the application's exact pre-operation body: a 60x40x30
//! extrusion, open at its top with an inward wall of 3, outer vertical rounds R6,
//! inner vertical rounds R3, and an inner floor round R1. Synthetic reconstruction
//! through ops does not reproduce the refusal, so retain this valid representation.
//!
//! The inner planar contour has length L = 152 + 6*pi and total turning 2*pi.
//! Integrating a section against its parallel-curve Jacobian (1 +/- curvature*u)
//! gives volume L*area +/- 2*pi*`first_moment`. A round's removed section has area
//! (1-pi/4)*r^2 and first moment (5/6-pi/4)*r^3. A chamfer's are d^2/2 and d^3/6.
//! The floor adds material on the inward side; the rim removes it outward.
//!
//! A second capture uses the application Kernel APIs to construct
//! `typical/filleted-shelled-box` after its height changes from 30 to 33: a centered
//! 70x50 extrusion, outer vertical R6 rounds, then a top-open inward shell of 2
//! (inner vertical R4). Its inner contour
//! has L = 192 + 8*pi. The matrix's unchanged blend size is 0.627, derived from
//! the original display polylines. This tests the resulting geometry, not timeline
//! reference resolution. No floor round is present in this second capture.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::f64::consts::PI;

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::AttributeStore;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::brep_volume_default;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::procedural_blend::chamfer::ChamferSize;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::loopuse;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel, Vec3};

fn input() -> (Kernel, Brep) {
    input_from_bytes(include_bytes!("data/issue_2903_shelled_box_rim.smb"))
}

fn edited_input() -> (Kernel, Brep) {
    input_from_bytes(include_bytes!("data/issue_2903_edited_shelled_box_rim.smb"))
}

fn input_from_bytes(bytes: &[u8]) -> (Kernel, Brep) {
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    k.fixes.fillet_refuse_rounds_breaking_through = false;
    k.fixes.fillet_round_ends_on_outline = false;
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &loopuse::notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let bytes = bytes.to_vec();
    let b = read_brep_from_file(&mut k, Some(bytes), true, false, &mut env).unwrap();
    (k, b)
}

fn perimeter() -> f64 {
    152.0 + 6.0 * PI
}

fn base_volume() -> f64 {
    let area = 1.0 - PI / 4.0;
    let moment = 5.0 / 6.0 - PI / 4.0;
    60.0 * 40.0 * 30.0 - 54.0 * 34.0 * 27.0 - 4.0 * area * 6.0 * 6.0 * 30.0
        + 4.0 * area * 3.0 * 3.0 * 27.0
        + perimeter() * area
        - 2.0 * PI * moment
}

fn volume_is(k: &Kernel, b: &mut Brep, expected: f64) {
    let measured = brep_volume_default(k, b, 1e-10).unwrap();
    assert!(
        (measured - expected).abs() <= 1e-9 * expected.abs(),
        "volume {measured} against closed form {expected}"
    );
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Round,
    Chamfer,
}

fn rim(k: &mut Kernel, b: &mut Brep, point: Vec3, kind: Kind, size: f64) -> solidcore::Result<()> {
    let picked = b
        .edge_list()
        .iter()
        .copied()
        .map(|e| {
            (
                e,
                edge_closest_point(k, b, e, point).unwrap().closest.distance,
            )
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    assert!(picked.1 < 1e-8, "rim selection missed {point:?}");
    let edges =
        solidcore::blend::blend_refusal::tangent_chain(k, b, &[picked.0], 0.5 * PI / 180.0)?;
    assert_eq!(edges.len(), 8, "the entire inner rim");
    {
        let mut s = FilletSession::new(k, b)?;
        s.set_swept_turns(false);
        match kind {
            Kind::Round => s.prepare_per_edge_constant_radius(
                k,
                &edges,
                &[size],
                &[FilletSurfaceGeneratorType::Circular],
                1,
                1.0,
            )?,
            Kind::Chamfer => {
                s.prepare_chamfers_along(k, &edges, &[ChamferSize::Distances([size, size])], &[])?;
            }
        }
        s.create_corners(k, None)?;
        if kind == Kind::Chamfer {
            for corner in s.fillet_corners()? {
                s.set_corner_bevel(corner, true)?;
            }
        }
        s.set_overflow_first(kind == Kind::Round);
        s.set_classify_only(false);
        s.do_filleting(k, None)?;
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default())?;
    b.refresh_cached_topology();
    Ok(())
}

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

#[test]
fn a_round_builds_the_same_rim_from_all_four_mirror_seeds() {
    mirrors(Kind::Round);
}

#[test]
fn a_chamfer_builds_the_same_rim_from_all_four_mirror_seeds() {
    mirrors(Kind::Chamfer);
}

fn edited_base_volume() -> f64 {
    let area = 1.0 - PI / 4.0;
    // Rounded outer area times full height, less rounded cavity area times depth.
    (70.0 * 50.0 - 4.0 * area * 6.0 * 6.0) * 33.0 - (66.0 * 46.0 - 4.0 * area * 4.0 * 4.0) * 31.0
}

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

#[test]
fn a_round_builds_the_edited_rim_from_all_four_mirror_seeds() {
    edited_mirrors(Kind::Round);
}

#[test]
fn a_chamfer_builds_the_edited_rim_from_all_four_mirror_seeds() {
    edited_mirrors(Kind::Chamfer);
}
