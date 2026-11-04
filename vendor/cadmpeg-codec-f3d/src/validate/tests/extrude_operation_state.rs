// SPDX-License-Identifier: Apache-2.0
//! Extrude result operations and body operands in committed and null states.

use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

const CONFLICT: &str = "Fusion Design Extrude operation conflicts with its body operands";

/// An Extrude with `operation`, no body operand group, and the given state.
fn extrude(
    operation: DesignExtrudeOperation,
    history_state_id: Option<i64>,
) -> crate::native::F3dNative {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Extrude,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = history_state_id;
            draft.previous_history_state_id = history_state_id.map(|state| state - 1);
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot) =
        scope.payload_mut()
    {
        let extrude = slot.get_or_insert_with(Default::default);
        extrude.extrude_prologue = Some(DesignExtrudePrologue::ReferenceAware {
            reference: None,
            operation,
            operation_offset: 28,
            direction_face_extend_values: [1, 2],
            side_extent_discriminators: [2, 0],
            side_extent_discriminator_offsets: [77, 90],
            first_side_target_ordinal: None,
            extent: DesignExtrudeExtent::OneSidedToFace,
            direction_face_extend_offsets: [32, 36],
            direction_reversed: false,
            direction_reversed_offset: 40,
            solid_operation: true,
            solid_operation_offset: 41,
            start: DesignExtrudeStart::ProfilePlane,
            start_offset: 42,
        });
    }
    crate::native::F3dNative {
        design_parameter_scopes: vec![scope],
        ..Default::default()
    }
}

fn conflicts(native: &crate::native::F3dNative) -> bool {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let ctx = super::super::Ctx::new(&ir, native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_extrude_parameter_operands(&ctx, &mut findings);
        findings.iter().any(|finding| finding.message == CONFLICT)
    })
}

#[test]
fn boolean_extrude_without_body_operands_needs_a_null_state() {
    for operation in [
        DesignExtrudeOperation::Join,
        DesignExtrudeOperation::Cut,
        DesignExtrudeOperation::Intersect,
    ] {
        assert!(conflicts(&extrude(operation, Some(7))), "{operation:?}");
        assert!(!conflicts(&extrude(operation, None)), "{operation:?}");
    }
}

#[test]
fn new_body_extrude_has_no_body_operands_in_either_state() {
    for state in [None, Some(7)] {
        assert!(!conflicts(&extrude(DesignExtrudeOperation::NewBody, state)));
    }
}
