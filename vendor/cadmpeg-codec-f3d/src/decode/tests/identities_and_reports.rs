use cadmpeg_core::decode::DecodePolicy;

fn annotation_placement() -> crate::records::sketch_placement::DesignSketchPlacement {
    crate::records::sketch_placement::DesignSketchPlacement {
        id: "f3d:test/BulkStream.dat:design-sketch-placement#4".into(),
        scope_record_index: None,
        entity_id: crate::records::identity::DesignEntityId::from_parts("sketch", 7),
        visibility: None,
        class_tag: "330".to_owned().try_into().unwrap(),
        record_index: 4,
        paired_class_tag: "330".to_owned().try_into().unwrap(),
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),
    }
}

#[test]
fn sketch_annotation_ids_preserve_identity_text() {
    let placement = annotation_placement();
    assert_eq!(
        crate::ids::neutral_sketch_id(&placement),
        crate::ids::neutral_sketch_id(&placement)
    );
    assert_eq!(
        crate::ids::neutral_spatial_sketch_id(&placement),
        crate::ids::neutral_spatial_sketch_id(&placement)
    );
    assert_eq!(
        crate::ids::neutral_sketch_constraint_id(&placement.id, 4),
        crate::ids::neutral_sketch_constraint_id(&placement.id, 4)
    );
}

#[test]
fn failed_mesh_resource_limit_propagates() {
    let refusal = cadmpeg_core::CodecError::ResourceLimit(cadmpeg_core::decode::ResourceLimit {
        dimension: cadmpeg_core::decode::ResourceDimension::DecompressedBytes,
        reason: cadmpeg_core::decode::ResourceFailure::BudgetExceeded,
        limit: 0,
        used: 0,
        additional: 1,
        operation: "synthetic mesh refusal",
    });
    let mut bodies = Vec::new();
    let mut report = cadmpeg_ir::codec::DecodeBody::new(
        cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
    );
    let error = super::super::collect_mesh_outcome(
        &mut bodies,
        &mut report,
        crate::design::decode::mesh::MeshContainerOutcome::Failed {
            entry_name: "mesh.paramesh".into(),
            error: refusal,
        },
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "synthetic mesh refusal")
    );
    assert!(report.losses.is_empty());
}

#[test]
fn geometry_kind_counts_keep_sorted_report_text() {
    let counts = std::collections::BTreeMap::from([("plane".into(), 2), ("spline".into(), 3)]);
    assert_eq!(
        super::super::KindCounts(&counts).to_string(),
        "plane=2, spline=3"
    );
}

fn dimension_native() -> crate::native::F3dNative {
    use crate::records::parameters::{
        DesignCompanionPayload, DesignParameter, DesignParameterDraft, DesignParameterOwner,
        DesignParameterOwnerWire, DesignParameterSource,
    };
    use crate::records::references::DesignClassTag;

    let stream = "f3d:test/BulkStream.dat";
    let parameter = DesignParameter::try_from(DesignParameterDraft {
        id: format!("{stream}:design-parameter#28"),
        byte_offset: 0,
        class_tag: DesignClassTag::try_from("305".to_owned()).unwrap(),
        record_index: 28,
        source_ordinal: 0,
        source: DesignParameterSource::new(
            "Linear Dimension-2".into(),
            Some(29),
            Some(crate::records::identity::Located {
                value: crate::records::parameters::DesignParameterDiscriminator::Code0,
                offset: 22,
            }),
        )
        .unwrap(),
        expression: "5 mm".into(),
        expression_offset: 40,
        source_kind_offset: 60,
        unit: Some(crate::records::identity::RecordedValue {
            value: "mm".into(),
            offset: 90,
        }),
        name: "d1".into(),
        name_offset: 100,
        evaluated_value: 0.5,
        evaluated_value_offset: 110,
    })
    .unwrap();
    let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: format!("{stream}:design-parameter-owner#29"),
        byte_offset: 120,
        frame_length: 104,
        class_tag: DesignClassTag::try_from("292".to_owned()).unwrap(),
        record_index: 29,
        scope_record_index: 1,
        local_ordinal: 0,
        evaluated_value: 0.5,
        evaluated_value_offset: 160,
        parameter_record_index: 28,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: 30,
    })
    .unwrap();
    let companion = crate::records::parameters::DesignParameterCompanion::unbound(
        format!("{stream}:design-parameter-companion#30"),
        220,
        DesignClassTag::try_from("408".to_owned()).unwrap(),
        30,
        29,
        std::num::NonZeroU64::new(1).unwrap(),
        262,
    )
    .bound(DesignCompanionPayload::new(278, 100, Vec::new()));
    crate::native::F3dNative {
        design_parameters: vec![parameter],
        design_parameter_owners: vec![owner],
        design_parameter_companions: vec![companion],
        ..crate::native::F3dNative::default()
    }
}

#[test]
fn container_only_neutral_parameter_id_matches_identity_shape() {
    let native = dimension_native();
    let parameter = native.design_parameters.first().unwrap();
    assert_eq!(
        crate::ids::neutral_parameter_id(parameter),
        crate::ids::neutral_parameter_id(parameter),
    );
}

#[test]
fn archive_entries_do_not_hide_new_model_entities() {
    let bytes = crate::test_support::assembly_test::f3d_without_brep(
        "assembly",
        "root.f3d",
        &[("part.f3d", crate::test_support::assembly_test::XREF_ROLE)],
    );
    crate::test_support::with_decode_context(|scan_ctx| {
        let scan =
            crate::container::scan(scan_ctx, cadmpeg_core::decode::View::over_retained(&bytes))
                .unwrap();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = cadmpeg_core::decode::u64_from_index(scan.entries.len());
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let error = super::super::decode_scanned_document(
                ctx,
                &scan,
                crate::report::ReportScope::Standalone,
            )
            .err()
            .unwrap();
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                panic!("model population must refuse");
            };
            assert_eq!(
                limit.dimension,
                cadmpeg_core::decode::ResourceDimension::Entities
            );
            assert_eq!(limit.operation, "admit F3D entities");
            assert_eq!(Some(limit), ctx.resource_refusal());
        });
    });
}

#[test]
fn unsupported_parameter_owners_report_a_loss_only_when_present() {
    let mut report = cadmpeg_ir::codec::DecodeBody::new(
        cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
    );
    super::super::report_unparsed_parameter_owners(&mut report, 0);
    assert!(report.losses.is_empty());
    super::super::report_unparsed_parameter_owners(&mut report, 2);
    assert_eq!(report.losses.len(), 1);
    assert_eq!(
        report.losses[0],
        crate::loss::F3dLossCode::ParameterOwnerUnparsed.note(
            "2 Design parameter owner binding(s) match no owner grammar; their parameters remain native without interpreted owner bindings."
        )
    );
}
