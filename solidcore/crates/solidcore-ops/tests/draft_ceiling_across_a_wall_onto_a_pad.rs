//! A draft that tilts a ceiling across a corner where it met a wall only at a vertex.
//!
//! A section in x-z extruded 3 along +y: below the top z = 16 the solid's underside is a ceiling,
//! flat at z = 10 for x in [-4.2, 0] and rising at 9 deg toward -x for x in [-14, -4.2]. A pad
//! 4.2 x 2 x 6 is joined on the y = 0 side under the flat ceiling, so the flat ceiling is one face
//! running across the wall line y = 0 onto the pad, and it meets the wall y = 0 (beside the
//! sloped ceiling) only at a vertex.
//!
//! The flat ceiling drafted -2.5 deg about x = 0 (pull +x) rises toward -x. Inside the wall it
//! grows until it meets the 9 deg ceiling; outside it only lifts the pad's end; so it now needs an
//! edge with the wall. The extended offset's corner split made that edge only at a convex corner
//! of four planes; this corner has a concave edge, the split was skipped, and the draft was refused
//! ("not a closed solid"). Without the pad there is no such corner, and that case always built.

#![allow(clippy::disallowed_methods)] // the invented body's profile and closed-form volumes
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test

use solidcore_ops::boolean::boolean;
use solidcore_ops::offset::draft;
use solidcore_ops::sweep::{extrude, Curve, DraftCorner};
use solidcore_ops::{make_box, session, Body, BooleanOp, CheckLevel, Order};
use solidcore_record::brep::Surface;

/// SolidCraft's fix set (all but `HELD_BACK` in `crates/smlib/src/core.rs`), with or without
/// `offset_splits_mixed_corner_vertices`.
fn fixes(fix: bool) {
    const HELD_BACK: [&str; 2] = [
        "fillet_refuse_rounds_breaking_through",
        "fillet_round_ends_on_outline",
    ];
    let on = solidcore::kernel::UpstreamFixes::NAMES
        .iter()
        .copied()
        .filter(|n| !HELD_BACK.contains(n))
        .filter(|n| fix || *n != "offset_splits_mixed_corner_vertices");
    session::set_thread_fixes(on).unwrap();
}

const SLOPE: f64 = 9.0;
const DRAFT: f64 = 2.5;

/// The solid above the ceiling, with the pad (`pad`) or without it.
fn body(pad: bool) -> Body {
    let ts = SLOPE.to_radians().tan();
    let pts = [
        [0.0, 0.0, 10.0],
        [0.0, 0.0, 16.0],
        [-14.0, 0.0, 16.0],
        [-14.0, 0.0, 10.0 + 9.8 * ts],
        [-4.2, 0.0, 10.0],
    ];
    let profile: Vec<Curve> = pts
        .iter()
        .zip(pts.iter().cycle().skip(1))
        .map(|(&start, &end)| Curve::Line { start, end })
        .collect();
    let b = extrude(
        &profile,
        [0.0, 1.0, 0.0],
        3.0,
        0.0,
        DraftCorner::default(),
        true,
    )
    .unwrap();
    if !pad {
        return b;
    }
    let pad = make_box([-4.2, -2.0, 10.0], [4.2, 2.0, 6.0]).unwrap();
    boolean(b, pad, BooleanOp::Union).unwrap().unwrap()
}

/// The record index of the one plane face of `b` with normal along z at height `z`.
fn ceiling(b: &Body, z: f64) -> (usize, Order) {
    let (record, order) = b.to_record().unwrap();
    let hits: Vec<usize> = record
        .faces
        .iter()
        .enumerate()
        .filter(|(_, f)| match f.surface {
            Surface::Plane {
                origin,
                u_axis,
                v_axis,
            } => {
                let n = [
                    u_axis[1] * v_axis[2] - u_axis[2] * v_axis[1],
                    u_axis[2] * v_axis[0] - u_axis[0] * v_axis[2],
                    u_axis[0] * v_axis[1] - u_axis[1] * v_axis[0],
                ];
                n[0].abs() < 1e-12 && n[1].abs() < 1e-12 && (origin[2] - z).abs() < 1e-9
            }
            _ => false,
        })
        .map(|(i, _)| i)
        .collect();
    assert_eq!(hits.len(), 1, "one face at z = {z}, {hits:?}");
    (*hits.first().unwrap(), order)
}

/// The flat ceiling drafted -2.5 deg about x = 0: the volume, or the refusal.
fn drafted(pad: bool) -> solidcore_ops::Result<f64> {
    let b = body(pad);
    let (face, order) = ceiling(&b, 10.0);
    draft(
        &b,
        &order,
        &[face],
        [0.0, 0.0, 10.0],
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        -DRAFT.to_radians(),
    )?;
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "pad {pad}: {report}");
    Ok(b.volume(1e-9).unwrap())
}

/// The closed form. With ts = tan 9 deg and td = tan 2.5 deg the section's area is
/// A = 6 x 14 - ts 9.8^2 / 2. The drafted ceiling z = 10 - x td meets the 9 deg one at
/// xc = -4.2 ts / (ts - td); inside (width 3) the draft takes off
/// 3 [td xc^2 / 2 - ts (-4.2 - xc)^2 / 2], and on the pad (width 2) 2 td 4.2^2 / 2.
fn closed_form(pad: bool) -> f64 {
    let ts = SLOPE.to_radians().tan();
    let td = DRAFT.to_radians().tan();
    let area = 6.0 * 14.0 - ts * 9.8 * 9.8 / 2.0;
    let xc = -4.2 * ts / (ts - td);
    let inside = 3.0 * (td * xc * xc / 2.0 - ts * (-4.2 - xc).powi(2) / 2.0);
    let v = 3.0 * area - inside;
    if pad {
        v + 2.0 * 4.2 * 6.0 - 2.0 * td * 4.2 * 4.2 / 2.0
    } else {
        v
    }
}

#[test]
fn a_ceiling_drafted_across_a_wall_onto_a_pad_closes() {
    fixes(true);
    let v = drafted(true).unwrap();
    let want = closed_form(true);
    assert!((v - want).abs() < 1e-6 * want, "{v} vs {want}");
}

/// Without the fix the corner is not split and the draft is refused.
#[test]
fn without_the_fix_the_draft_onto_the_pad_is_refused() {
    fixes(false);
    assert!(drafted(true).is_err(), "built without the fix");
}

/// Without the pad there is no corner to split: built with the fix and without it.
#[test]
fn the_no_pad_control_builds_either_way() {
    for fix in [true, false] {
        fixes(fix);
        let v = drafted(false).unwrap();
        let want = closed_form(false);
        assert!((v - want).abs() < 1e-6 * want, "fix {fix}: {v} vs {want}");
    }
}
