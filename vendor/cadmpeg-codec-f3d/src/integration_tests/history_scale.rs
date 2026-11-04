// SPDX-License-Identifier: Apache-2.0
//! Session budgets for designs that carry long construction histories.
//!
//! Each history state stores its transition as a delta, so the retained bytes
//! a decode charges for history grow with the total bulletin-board change, not
//! with the number of states times the model size. A design of a few
//! megabytes decodes under both default limit profiles.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeResult};

use crate::loss::F3dLossCode;
use crate::test_support::history_stream::DesignScale;
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;

/// Fifty states over 901 entities.
const SMALL: DesignScale = DesignScale {
    states: 50,
    bodies: 50,
    faces_per_body: 1,
    updates_per_state: 2,
};
/// Twice the states and twice the entities of [`SMALL`]: twice the change,
/// four times the states-by-entities product.
const MEDIUM: DesignScale = DesignScale {
    states: 100,
    bodies: 100,
    faces_per_body: 1,
    updates_per_state: 2,
};
/// Two hundred states over 5,401 entities.
const LARGE: DesignScale = DesignScale {
    states: 200,
    bodies: 300,
    faces_per_body: 1,
    updates_per_state: 2,
};
/// Twice [`LARGE`] in both directions.
const LARGER: DesignScale = DesignScale {
    states: 400,
    bodies: 600,
    faces_per_body: 1,
    updates_per_state: 2,
};
/// About 30,000 entities, a few megabytes of SMBH content, in a hundred
/// twenty-face sheet bodies built over a short history.
const MODERATE: DesignScale = DesignScale {
    states: 10,
    bodies: 100,
    faces_per_body: 20,
    updates_per_state: 3,
};
/// Smallest SMBH payload that counts as a few megabytes.
const MODERATE_MIN_BYTES: usize = 2 << 20;
/// Allowed relative difference between two sizes' retained history bytes per
/// bulletin-board change.
const PER_CHANGE_SPREAD: f64 = 0.25;

struct Charged {
    result: DecodeResult,
    retained_bytes: u64,
}

/// Decode in a caller-owned session, as the CLI loader does, and return the
/// result with the retained bytes it charged.
fn decode_charged(bytes: &[u8], policy: &DecodePolicy) -> Charged {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, policy).expect("acquire the archive");
    let available = ctx.retained_bytes_available();
    let result = F3dCodec.decode_with_context(&ctx, root);
    let retained_bytes = available - ctx.retained_bytes_available();
    assert_eq!(
        ctx.resource_refusal(),
        None,
        "no session ceiling is reached"
    );
    ctx.finish_session().expect("the session closes cleanly");
    Charged {
        result: result.expect("the design decodes"),
        retained_bytes,
    }
}

fn assert_history_bound(result: &DecodeResult, scale: DesignScale) {
    assert!(result.report().geometry_transferred());
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::HistoryBindingBudgetExceeded.kind()));
    assert_eq!(result.ir().model.bodies.len(), scale.bodies);
    let native = result
        .ir()
        .native
        .namespace("f3d")
        .expect("F3D native namespace");
    let states = &native.arenas()["asm_delta_states"];
    assert_eq!(states.len(), scale.states);
    assert!(states.iter().all(|state| state
        .field("transition")
        .is_some_and(|transition| !transition.is_null())));
}

/// Retained bytes the history partition adds to the same active model.
fn history_retained_bytes(scale: DesignScale, policy: &DecodePolicy) -> u64 {
    let with_history = decode_charged(&f3d_with_smbh(&scale.history_smbh()), policy);
    assert_history_bound(&with_history.result, scale);
    let without_history = decode_charged(&f3d_with_smbh(&scale.snapshot_smbh()), policy);
    with_history.retained_bytes - without_history.retained_bytes
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    f64::from(u32::try_from(numerator).unwrap()) / f64::from(u32::try_from(denominator).unwrap())
}

fn assert_retention_tracks_change(smaller: DesignScale, larger: DesignScale, policy: DecodePolicy) {
    let change_growth = ratio(larger.total_changes(), smaller.total_changes());
    let product_growth = ratio(
        larger.states * larger.active_records(),
        smaller.states * smaller.active_records(),
    );
    assert!(
        product_growth > change_growth * (1.0 + 2.0 * PER_CHANGE_SPREAD),
        "the two sizes separate change growth from states-by-entities growth"
    );
    let per_change = |scale: DesignScale| {
        let bytes = usize::try_from(history_retained_bytes(scale, &policy)).unwrap();
        ratio(bytes, scale.total_changes())
    };
    let (smaller_rate, larger_rate) = (per_change(smaller), per_change(larger));
    assert!(
        (larger_rate / smaller_rate - 1.0).abs() <= PER_CHANGE_SPREAD,
        "retained history bytes per change: {smaller_rate:.1} at {smaller:?}, \
         {larger_rate:.1} at {larger:?}"
    );
}

#[test]
fn retained_history_bytes_grow_with_total_change_under_desktop_limits() {
    assert_retention_tracks_change(SMALL, MEDIUM, DecodePolicy::desktop());
}

#[test]
fn retained_history_bytes_grow_with_total_change_under_service_limits() {
    assert_retention_tracks_change(SMALL, MEDIUM, DecodePolicy::service());
}

#[test]
#[ignore = "decodes a 400-state history over 10,801 entities; run with --ignored"]
fn retained_history_bytes_grow_with_total_change_over_hundreds_of_states() {
    for policy in [DecodePolicy::desktop(), DecodePolicy::service()] {
        assert_retention_tracks_change(LARGE, LARGER, policy);
    }
}

fn assert_moderate_design_decodes(policy: &DecodePolicy) {
    let smbh = MODERATE.history_smbh();
    assert!(
        smbh.len() >= MODERATE_MIN_BYTES,
        "{} SMBH bytes",
        smbh.len()
    );
    let decoded = decode_charged(&f3d_with_smbh(&smbh), policy);
    assert_history_bound(&decoded.result, MODERATE);
}

#[test]
fn moderate_design_reaches_no_session_ceiling_under_desktop_limits() {
    assert_moderate_design_decodes(&DecodePolicy::desktop());
}

#[test]
fn moderate_design_reaches_no_session_ceiling_under_service_limits() {
    assert_moderate_design_decodes(&DecodePolicy::service());
}
