// SPDX-License-Identifier: Apache-2.0
//! Whole archives with one inconsistent record, converted to STEP along the
//! CLI's path. Each must still export geometry and report the inconsistency
//! as a loss rather than refusing the container.

use cadmpeg_ir::CadIr;
use serde_json::Value;

use crate::loss::F3dLossCode;
use crate::test_support::body_map_test::{f3d_with_model_breps, ModelBlob};
use crate::test_support::lost_edge_test::{GROUP, OPERAND_COUNT};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::smbh_surfaces_test::synthetic_off_spl_sur_smbh;
use crate::test_support::step_conversion::convert_to_step;
use crate::test_support::zip_test::f3d_with_smbh_and_remove_body_lost_edge_run;

/// Lost-edge records the generated Design stream holds before the group's run.
const BASE_LOST_EDGES: usize = 1;

/// The lost-edge references bound by the construction group at record index
/// `GROUP`.
fn group_lost_edge_references(ir: &CadIr) -> Vec<Value> {
    let native = ir.native.namespace("f3d").expect("F3D native namespace");
    let groups = &native.arenas()["design_construction_operand_groups"];
    let group = groups
        .iter()
        .find(|group| {
            group.field("record_index").and_then(|index| index.as_u64()) == Some(u64::from(GROUP))
        })
        .expect("the RemoveBody construction group");
    let members = group.field("members").expect("group members");
    assert_eq!(members.as_array().map(Vec::len), Some(OPERAND_COUNT));
    group
        .field("lost_edge_references")
        .and_then(|references| references.as_array().cloned())
        .unwrap_or_default()
}

fn lost_edge_record_count(ir: &CadIr) -> usize {
    let native = ir.native.namespace("f3d").expect("F3D native namespace");
    native.arenas()["lost_edge_references"].len()
}

fn edge_reference_losses(conversion_losses: &[cadmpeg_ir::report::loss::LossNote]) -> usize {
    conversion_losses
        .iter()
        .filter(|loss| loss.code == F3dLossCode::EdgeReferenceLostUnrepaired.kind())
        .count()
}

#[test]
fn lost_edge_run_shorter_than_its_operands_exports_with_unbound_edge_loss() {
    let bytes = f3d_with_smbh_and_remove_body_lost_edge_run(&synthetic_geometry_smbh(), 1);
    let conversion = convert_to_step(&bytes);
    assert!(!conversion.step.is_empty());
    assert_eq!(lost_edge_record_count(&conversion.ir), BASE_LOST_EDGES + 1);
    assert!(group_lost_edge_references(&conversion.ir).is_empty());
    assert_eq!(edge_reference_losses(&conversion.decode.losses), 1);
}

#[test]
fn lost_edge_run_matching_its_operands_binds_to_the_group() {
    let run = u32::try_from(OPERAND_COUNT).unwrap();
    let bytes = f3d_with_smbh_and_remove_body_lost_edge_run(&synthetic_geometry_smbh(), run);
    let conversion = convert_to_step(&bytes);
    assert_eq!(
        lost_edge_record_count(&conversion.ir),
        BASE_LOST_EDGES + OPERAND_COUNT
    );
    assert_eq!(
        group_lost_edge_references(&conversion.ir).len(),
        OPERAND_COUNT
    );
}

/// The procedural surface's owner is in the second of two model blobs, so
/// both its identity and its owning surface's identity carry that blob.
#[test]
fn procedural_surface_in_a_second_model_blob_exports_with_its_owner() {
    let offset = synthetic_off_spl_sur_smbh("off_spl_sur");
    let plane = synthetic_geometry_smbh();
    let bytes = f3d_with_model_breps(&[
        ModelBlob {
            basename: "BREP.plane.smbh",
            smbh: &plane,
            selector: 42,
        },
        ModelBlob {
            basename: "BREP.offset.smbh",
            smbh: &offset,
            selector: 0,
        },
    ]);
    let conversion = convert_to_step(&bytes);
    let model = &conversion.ir.model;
    assert_eq!(model.bodies.len(), 2);
    assert!(!model.procedural_surfaces.is_empty());
    for procedural in &model.procedural_surfaces {
        assert!(
            procedural.id.as_str().contains("/offset.smbh/"),
            "{} names its source blob",
            procedural.id
        );
        let owners = model
            .surfaces
            .iter()
            .filter(|surface| {
                surface
                    .geometry
                    .procedural_construction()
                    .is_some_and(|construction| construction.as_str() == procedural.id.as_str())
            })
            .count();
        assert_eq!(owners, 1, "{} has one owning surface", procedural.id);
    }
    assert!(!conversion.step.is_empty());
}
