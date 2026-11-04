// SPDX-License-Identifier: Apache-2.0

use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::parameters::{
    DesignParameter, DesignParameterCompanion, DesignParameterDraft, DesignParameterOwner,
    DesignParameterOwnerWire, DesignParameterSource,
};
use crate::records::references::DesignClassTag;
use crate::test_support::native_test::f3d_native_mut;

const DESIGN_STREAM: &str = "Design/BulkStream.dat";
const SCOPE: u32 = 10;

fn header(record_index: u32, byte_offset: u64) -> DesignRecordHeader {
    DesignRecordHeader {
        id: crate::ids::native_scoped_id(DESIGN_STREAM, "record-header", byte_offset),
        record_index,
        class_tag: DesignClassTag::try_from("300".to_owned()).unwrap(),
        byte_offset,
    }
}

/// One sketch-dimension owner, parameter, and companion at consecutive record
/// indices from `owner_index`, stored with `scope_record_index` and
/// `local_ordinal`.
fn add_dimension(
    ir: &mut cadmpeg_ir::document::CadIr,
    owner_index: u32,
    scope_record_index: u32,
    local_ordinal: u32,
) {
    let base = u64::from(owner_index) * 1_000;
    let parameter = DesignParameter::try_from(DesignParameterDraft {
        id: crate::ids::native_design_parameter_id(DESIGN_STREAM, owner_index + 1),
        byte_offset: base + 200,
        class_tag: DesignClassTag::try_from("305".to_owned()).unwrap(),
        record_index: owner_index + 1,
        source_ordinal: 0,
        source: DesignParameterSource::new("Linear Dimension-1".into(), Some(owner_index), None)
            .unwrap(),
        expression: "3 mm".into(),
        expression_offset: base + 210,
        source_kind_offset: base + 230,
        unit: Some(crate::records::identity::RecordedValue {
            value: "cm".into(),
            offset: base + 250,
        }),
        name: format!("d{owner_index}"),
        name_offset: base + 260,
        evaluated_value: 0.3,
        evaluated_value_offset: base + 270,
    })
    .unwrap();
    let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: crate::ids::native_design_parameter_owner_id(DESIGN_STREAM, base),
        byte_offset: base,
        frame_length: 104,
        class_tag: DesignClassTag::try_from("447".to_owned()).unwrap(),
        record_index: owner_index,
        scope_record_index,
        local_ordinal,
        evaluated_value: 0.3,
        evaluated_value_offset: base + 40,
        parameter_record_index: owner_index + 1,
        owned_ordinal: owner_index,
        variant: Some(0),
        companion_record_index: owner_index + 2,
    })
    .unwrap();
    let companion = DesignParameterCompanion::unbound(
        format!(
            "{}:design-parameter-companion#{}",
            crate::ids::native_scope(DESIGN_STREAM),
            base + 400
        ),
        base + 400,
        DesignClassTag::try_from("258".to_owned()).unwrap(),
        owner_index + 2,
        owner_index,
        std::num::NonZeroU64::new(1).unwrap(),
        base + 442,
    )
    .bound(crate::records::parameters::DesignCompanionPayload::new(
        base + 458,
        0,
        Vec::new(),
    ));
    let mut native = f3d_native_mut(ir);
    native.design_parameters.push(parameter);
    native.design_parameter_owners.push(owner);
    native.design_parameter_companions.push(companion);
    native.design_record_headers.extend([
        header(owner_index, base),
        header(owner_index + 1, base + 200),
        header(owner_index + 2, base + 400),
    ]);
}

fn owner_findings(ir: &cadmpeg_ir::document::CadIr) -> Vec<String> {
    crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, ir).expect("validate the native namespace")
    })
    .into_iter()
    .filter(|finding| {
        finding
            .message
            .contains("Fusion Design parameter owner has an invalid frame or indexed link")
    })
    .filter_map(|finding| finding.entity)
    .collect()
}

fn sketch_naming(record_indices: &[u32]) -> cadmpeg_ir::document::CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut scope = DesignParameterScope::empty(
        &format!("f3d:{DESIGN_STREAM}:design-parameter-scope#100"),
        DesignFeatureKind::Sketch,
        SCOPE,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(record_indices.to_vec());
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut native = f3d_native_mut(&mut ir);
    native.design_parameter_scopes.push(scope);
    native.design_record_headers.push(header(SCOPE, 100));
    drop(native);
    ir
}

#[test]
fn detached_owner_may_repeat_the_local_ordinal_of_a_named_owner() {
    let mut ir = sketch_naming(&[20]);
    add_dimension(&mut ir, 20, SCOPE, 4);
    add_dimension(&mut ir, 30, SCOPE, 4);
    assert_eq!(owner_findings(&ir), Vec::<String>::new());
}

#[test]
fn detached_owner_may_name_an_absent_scope_record() {
    let mut ir = sketch_naming(&[20]);
    add_dimension(&mut ir, 20, SCOPE, 0);
    add_dimension(&mut ir, 30, 798, 0);
    assert_eq!(owner_findings(&ir), Vec::<String>::new());
}

#[test]
fn named_owners_keep_unique_local_ordinals() {
    let mut ir = sketch_naming(&[20, 30]);
    add_dimension(&mut ir, 20, SCOPE, 4);
    add_dimension(&mut ir, 30, SCOPE, 4);
    assert_eq!(owner_findings(&ir).len(), 1);
}
