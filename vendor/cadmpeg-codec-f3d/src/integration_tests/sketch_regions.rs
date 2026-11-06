// SPDX-License-Identifier: Apache-2.0
//! Sketch-region bodies of a keyless BREP blob stay out of the model and out of
//! the body map's selector ordinals.

use super::EditableDecodeResult;
use crate::loss::F3dLossCode;
use crate::test_support::body_map_test::f3d_with_breps_and_design;
use crate::test_support::native_test::f3d_native;
use crate::test_support::sketch_region_test::{
    snapshot_map_design, triangle_bodies_smbh, TriangleBody, MODEL_BODY, SKETCH_REGION,
    SKETCH_SURFACE,
};

const BLOB: &str = "BREP.snapshot.smb";
const EPS_PLANE_HEIGHT_MM: f64 = 1.0e-9;

fn decoded(bodies: &[TriangleBody], pairs: &[(u64, u64)]) -> EditableDecodeResult {
    let (bulk, meta) = snapshot_map_design(BLOB, pairs);
    let smbh = triangle_bodies_smbh(bodies);
    super::decode(f3d_with_breps_and_design(&[(BLOB, &smbh)], &bulk, &meta))
}

/// The source ordinal of each model face's plane, in ascending order. Body `i`
/// of a stream lies at height `i` centimetres.
fn kept_source_ordinals(result: &EditableDecodeResult) -> Vec<u32> {
    let mut ordinals = result
        .ir()
        .model
        .surfaces
        .iter()
        .map(|surface| {
            let height = serde_json::to_value(surface).unwrap()["geometry"]["origin"]["z"]
                .as_f64()
                .unwrap()
                / 10.0;
            (0_u32..16)
                .find(|ordinal| (height - f64::from(*ordinal)).abs() < EPS_PLANE_HEIGHT_MM)
                .unwrap()
        })
        .collect::<Vec<_>>();
    ordinals.sort_unstable();
    ordinals
}

fn reports(result: &EditableDecodeResult, code: F3dLossCode) -> bool {
    result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == code.kind())
}

#[test]
fn selectors_of_a_keyless_blob_count_only_the_bodies_after_its_sketch_regions() {
    for (bodies, pairs, kept) in [
        (&[SKETCH_REGION, MODEL_BODY][..], &[(0, 350)][..], &[1][..]),
        (
            &[SKETCH_REGION, SKETCH_REGION, MODEL_BODY],
            &[(0, 350)],
            &[2],
        ),
        (
            &[SKETCH_REGION, MODEL_BODY, SKETCH_REGION, MODEL_BODY],
            &[(0, 350), (1, 351)],
            &[1, 3],
        ),
        (&[SKETCH_REGION, SKETCH_SURFACE], &[(0, 350)], &[1]),
    ] {
        let result = decoded(bodies, pairs);
        assert_eq!(kept_source_ordinals(&result), kept);
        assert_eq!(result.ir().model.bodies.len(), kept.len());
        assert!(reports(&result, F3dLossCode::SketchRegionBodyOmitted));
        assert!(!reports(&result, F3dLossCode::DesignBodyBindingUnresolved));
        let native = f3d_native(result.ir());
        assert!(native
            .design_body_bindings
            .iter()
            .all(|binding| binding.body.is_some()));
    }
}

#[test]
fn a_blob_outside_the_sketch_region_form_keeps_its_ordinals() {
    let partly_linked = TriangleBody {
        linked_edges: 2,
        ..SKETCH_REGION
    };
    for (bodies, pairs, kept) in [
        // A surface made from a Sketch has a face attribute.
        (&[SKETCH_SURFACE, MODEL_BODY][..], &[(0, 350)][..], &[0][..]),
        // One edge stores no link.
        (&[partly_linked, MODEL_BODY], &[(0, 350)], &[0]),
        // No model body has a selector of its own.
        (&[SKETCH_REGION, MODEL_BODY], &[(0, 350), (1, 351)], &[0, 1]),
    ] {
        let result = decoded(bodies, pairs);
        assert_eq!(kept_source_ordinals(&result), kept);
        assert!(!reports(&result, F3dLossCode::SketchRegionBodyOmitted));
    }
}

#[test]
fn a_stored_body_key_selects_without_the_sketch_region_form() {
    let keyed_region = TriangleBody {
        key: 5,
        ..SKETCH_REGION
    };
    let keyed_body = TriangleBody {
        key: 6,
        ..MODEL_BODY
    };
    for (selector, kept) in [(6, 1), (5, 0)] {
        let result = decoded(&[keyed_region, keyed_body], &[(selector, 350)]);
        assert_eq!(kept_source_ordinals(&result), [kept]);
        assert!(!reports(&result, F3dLossCode::SketchRegionBodyOmitted));
    }
}

#[test]
fn a_blob_of_only_sketch_regions_selects_no_physical_model() {
    let result = decoded(&[SKETCH_REGION, SKETCH_REGION], &[]);
    assert!(result.ir().model.bodies.is_empty());
    let has = |text: &str| {
        result
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains(text))
    };
    assert!(has("No physical model BREP was selected"));
    assert!(!has("the selected model has no surfaces"));
    // One model body beside the regions and no selector stays out of the form.
    let result = decoded(&[SKETCH_REGION, MODEL_BODY], &[]);
    assert_eq!(result.ir().model.bodies.len(), 2);
    assert!(!reports(&result, F3dLossCode::SketchRegionBodyOmitted));
}
