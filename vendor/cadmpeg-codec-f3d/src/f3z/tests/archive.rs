// SPDX-License-Identifier: Apache-2.0

use cadmpeg_test_support::{assembly, wire, EditableDecodeResult};

use crate::test_support::assembly_test::{
    f3d_without_brep, f3d_without_brep_with_xref_placement, f3z_archive,
    f3z_archive_with_design_description, XREF_ROLE,
};
use crate::test_support::smbh_geometry_test::{synthetic_geometry_smbh, synthetic_mixed_smbh};
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;
use crate::F3dLossCode;
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::Confidence;
use cadmpeg_ir::codec::DecodeOptions;
use std::io::Cursor;

fn drawing_archive_for_root_limit_tests() -> Vec<u8> {
    let description = br#"{"designDescription":{"designGraphs":[{"rootIds":[10],"designObjects":[{"id":10,"relativePath":"drawing.f2d","contentType":"f2d","references":[{"type":"DERIVED","ids":[11]}]},{"id":11,"relativePath":"model.f3d","contentType":"f3d","references":[]}] }]}}"#;
    f3z_archive_with_design_description(
        "drawing.f2d",
        &[("drawing.f2d", b"drawing"), ("model.f3d", b"model")],
        description,
    )
}

#[test]
fn f3z_description_json_refuses_recursion_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let bytes = drawing_archive_for_root_limit_tests();
    let arena = DecodeArena::new();
    let (scan_context, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
    let scan = crate::container::scan(&scan_context, root).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = crate::f3z::archive::model_root(&limited, &scan).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "match F3Z derived model reference")
    );
}

#[test]
fn f3z_archive_merges_identity_occurrences() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let component_alone = EditableDecodeResult::from(
        crate::test_support::archive_session::decode(
            &mut Cursor::new(component.clone()),
            &DecodeOptions::default(),
        )
        .unwrap(),
    );
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("comp.f3d", component.as_slice()),
        ],
    );
    let decoded = EditableDecodeResult::from(
        crate::test_support::archive_session::decode(
            &mut Cursor::new(archive),
            &DecodeOptions::default(),
        )
        .unwrap(),
    );
    assert!(decoded.report().geometry_transferred());
    assert!(
        decoded
            .report()
            .losses
            .iter()
            .all(|loss| loss.severity < cadmpeg_ir::report::Severity::Error),
        "{:?}",
        decoded.report().losses
    );
    assert!(decoded
        .report()
        .notes
        .iter()
        .any(|note| note.contains("merged 1 external occurrence")));
    assert_eq!(
        decoded.ir().model.bodies.len(),
        component_alone.ir().model.bodies.len()
    );
    assert_eq!(
        decoded.ir().model.faces.len(),
        component_alone.ir().model.faces.len()
    );
    assert_eq!(
        decoded.ir().model.points.len(),
        component_alone.ir().model.points.len()
    );
    let prefix = format!("f3d:xref/role-{XREF_ROLE}/reference-0/");
    let body = &decoded.ir().model.bodies[0];
    assert!(
        body.id.as_str().starts_with(&prefix),
        "{}",
        body.id.as_str()
    );
    for shell_owner in &decoded.ir().model.shells {
        assert!(
            shell_owner.id.as_str().starts_with(&prefix),
            "occurrence graph must stay internally consistent: {}",
            shell_owner.id.as_str()
        );
    }
    assert!(decoded
        .source_fidelity()
        .retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
        .is_none());
    assert_eq!(
        decoded.source_fidelity().annotations.provenance.len(),
        component_alone
            .source_fidelity()
            .annotations
            .provenance
            .len()
    );
    assert!(decoded
        .source_fidelity()
        .annotations
        .provenance
        .keys()
        .all(|id| id.starts_with(&prefix)));
    // A merged F3Z has no retained image of itself, so there is nothing to
    // preserve, and `f3d:f3z-multi-document` is not a row the generator can
    // synthesize. `Inherit` therefore refuses by name rather than quietly
    // handing back a single-document archive under the F3Z document's identity.
    let error = F3dCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .expect_err("the F3Z row is not a synthesis target");
    let cadmpeg_core::CodecError::UnsupportedTarget(refusal) = &error else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(
        ({
            let wire = serde_json::to_value(refusal).expect("serialize refusal");
            wire["refusal"]
                .get("requested")
                .or_else(|| wire["refusal"].get("source"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .as_deref(),
        Some("f3d:f3z-multi-document")
    );
    assert!(
        refusal
            .available()
            .iter()
            .any(|target| target.id.as_str() == "f3d:manifest-3-2-0-0"),
        "{:?}",
        refusal.available()
    );

    // Naming the row is the escape, and it still regenerates the merged model
    // as a single-document archive — now with the report saying so.
    let mut regenerated = Vec::new();
    let report = F3dCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Explicit("f3d:manifest-3-2-0-0"),
        )
        .and_then(|plan| plan.write_to(&mut regenerated))
        .expect("merged F3Z regenerates at the named row");
    assert!(!regenerated.is_empty());
    assert_eq!(
        wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            &(report),
            "identity/target"
        )
        .as_ref()
        .map(cadmpeg_core::dialect::DialectId::as_str),
        Some("f3d:manifest-3-2-0-0")
    );
    assert!(report
        .notes
        .iter()
        .any(|note| note == "source container regenerated from IR"));
}

#[test]
fn duplicate_role_references_keep_archive_occurrences_disjoint() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let root = f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("first.f3d", XREF_ROLE), ("second.f3d", XREF_ROLE)],
    );
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("first.f3d", component.as_slice()),
            ("second.f3d", component.as_slice()),
        ],
    );

    let decoded = EditableDecodeResult::from(
        crate::test_support::archive_session::decode(
            &mut Cursor::new(archive),
            &DecodeOptions::default(),
        )
        .expect("admitted duplicate-role references remain independently mergeable"),
    );
    assert_eq!(decoded.ir().model.bodies.len(), 2);
    let first = decoded.ir().model.bodies[0].id.as_str();
    let second = decoded.ir().model.bodies[1].id.as_str();
    assert!(first.contains(&format!("role-{XREF_ROLE}/reference-0/occurrence-0/")));
    assert!(second.contains(&format!("role-{XREF_ROLE}/reference-1/occurrence-0/")));
    assert_ne!(first, second);
    for (ordinal, expected) in [(0, first), (1, second)] {
        let source_image = format!(
            "f3d:xref/role-{XREF_ROLE}/reference-{ordinal}/occurrence-0/file:source-image#0"
        );
        assert_eq!(
            decoded
                .source_fidelity()
                .retained_record(&source_image)
                .and_then(|record| record.data()),
            Some(component.as_slice()),
            "each duplicate-role member retains its own source image ({expected})"
        );
    }
}

#[test]
fn f3z_drawing_root_decodes_its_unambiguous_derived_model() {
    let model = f3d_with_smbh(&synthetic_geometry_smbh());
    let drawing = b"synthetic drawing payload";
    let description = br#"{"designDescription":{"designGraphs":[{"rootIds":[10],"designObjects":[{"id":10,"relativePath":"drawing.f2d","contentType":"f2d","references":[{"type":"DERIVED","ids":[11]}]},{"id":11,"relativePath":"model.f3d","contentType":"f3d","references":[]}]}]}}"#;
    let archive = f3z_archive_with_design_description(
        "drawing.f2d",
        &[("drawing.f2d", drawing), ("model.f3d", model.as_slice())],
        description,
    );

    let decoded = crate::test_support::archive_session::decode(
        &mut Cursor::new(archive),
        &DecodeOptions::default(),
    )
    .unwrap();

    assert!(decoded.report().geometry_transferred());
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::DrawingDocumentOmitted.kind()));
}

#[test]
fn f3z_drawing_root_rejects_ambiguous_derived_models() {
    let model = f3d_with_smbh(&synthetic_geometry_smbh());
    let description = br#"{"designDescription":{"designGraphs":[{"rootIds":[10],"designObjects":[{"id":10,"relativePath":"drawing.f2d","contentType":"f2d","references":[{"type":"DERIVED","ids":[11,12]}]},{"id":11,"relativePath":"first.f3d","contentType":"f3d","references":[]},{"id":12,"relativePath":"second.f3d","contentType":"f3d","references":[]}]}]}}"#;
    let archive = f3z_archive_with_design_description(
        "drawing.f2d",
        &[
            ("drawing.f2d", b"synthetic drawing payload"),
            ("first.f3d", model.as_slice()),
            ("second.f3d", model.as_slice()),
        ],
        description,
    );

    let result = crate::test_support::archive_session::decode(
        &mut Cursor::new(archive),
        &DecodeOptions::default(),
    );

    assert!(matches!(
        result,
        Err(cadmpeg_ir::DecodeFailure::Codec(
            cadmpeg_core::CodecError::Malformed(_)
        ))
    ));
}

#[test]
fn f3z_archive_merges_occurrence_scoped_unknown_carriers() {
    let component = f3d_with_smbh(&synthetic_mixed_smbh());
    let component_alone = crate::test_support::archive_session::decode(
        &mut Cursor::new(component.clone()),
        &DecodeOptions::default(),
    )
    .unwrap();
    let component_unknowns = component_alone.ir().native_unknowns("f3d").unwrap();
    assert!(!component_unknowns.is_empty());

    let root = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("comp.f3d", component.as_slice()),
        ],
    );
    let decoded = crate::test_support::archive_session::decode(
        &mut Cursor::new(archive),
        &DecodeOptions::default(),
    )
    .unwrap();

    let prefix = format!("f3d:xref/role-{XREF_ROLE}/reference-0/occurrence-0/");
    let merged_unknowns = decoded.ir().native_unknowns("f3d").unwrap();
    assert_eq!(merged_unknowns.len(), component_unknowns.len());
    assert!(merged_unknowns
        .iter()
        .all(|record| record.id.as_str().starts_with(&prefix)));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone())
        .expect("validate the neutral model");
    assert!(
        !validation.findings.iter().any(|finding| {
            finding.check == cadmpeg_ir::report::check::Check::ReferentialIntegrity
        }),
        "{validation:#?}"
    );
}

#[test]
fn f3z_archive_without_merged_components_preserves_root_replay() {
    let root = f3d_with_smbh(&synthetic_geometry_smbh());
    let archive = f3z_archive("root.f3d", &[("root.f3d", root.as_slice())]);
    let decoded = EditableDecodeResult::from(
        crate::test_support::archive_session::decode(
            &mut Cursor::new(archive.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap(),
    );

    assert!(decoded
        .source_fidelity()
        .retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
        .is_some());
    let plan = F3dCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .expect("unmerged F3Z archive remains replayable");
    let reported = wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
        plan.report(),
        "identity/target",
    )
    .as_ref()
    .expect("an F3D export names its target")
    .clone();
    let mut replayed = Vec::new();
    plan.write_to(&mut replayed).unwrap();
    assert_eq!(replayed, archive);

    let redecode = crate::test_support::archive_session::decode(
        &mut Cursor::new(replayed),
        &DecodeOptions::default(),
    )
    .unwrap();
    let primary = redecode
        .report()
        .dialects()
        .expect("re-decoded F3Z archive has a primary layer")
        .primary();
    assert_eq!(primary.dialect(), &reported);
}

#[test]
fn f3z_container_only_stamps_the_outer_document_digest() {
    let root = f3d_with_smbh(&synthetic_geometry_smbh());
    let archive = f3z_archive("root.f3d", &[("root.f3d", root.as_slice())]);
    let decoded = crate::test_support::archive_session::decode(
        &mut Cursor::new(archive),
        &DecodeOptions {
            depth: cadmpeg_core::decode::DecodeDepth::Container,
            ..DecodeOptions::default()
        },
    )
    .unwrap();

    let source = decoded.ir().source.as_ref().unwrap();
    assert_eq!(
        source
            .attributes
            .get(cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE),
        Some(&crate::decode::document_local_sha256(decoded.ir()).unwrap())
    );
}

#[test]
fn f3z_archive_recursively_merges_nested_occurrences() {
    const CHILD_ROLE: &str = "11112222-3333-4444-5555-666677778888";
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let component_alone = crate::test_support::archive_session::decode(
        &mut Cursor::new(component.clone()),
        &DecodeOptions::default(),
    )
    .unwrap();
    let middle = f3d_without_brep(
        "assembly-design",
        "middle.f3d",
        &[("component.f3d", CHILD_ROLE)],
    );
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("middle.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("middle.f3d", middle.as_slice()),
            ("component.f3d", component.as_slice()),
        ],
    );

    let decoded = crate::test_support::archive_session::decode(
        &mut Cursor::new(archive),
        &DecodeOptions::default(),
    )
    .unwrap();

    assert_eq!(
        decoded.ir().model.bodies.len(),
        component_alone.ir().model.bodies.len()
    );
    assert!(decoded
        .report()
        .notes
        .iter()
        .any(|note| note.contains("merged 2 external occurrence")));
    let body_id = &decoded.ir().model.bodies[0].id.as_str();
    assert!(body_id.contains(&format!(
        "xref/role-{XREF_ROLE}/reference-0/occurrence-0/xref/role-{CHILD_ROLE}/reference-0/occurrence-0/"
    )));
}

#[test]
fn f3z_nested_member_reference_obeys_session_depth_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    const CHILD_ROLE: &str = "11112222-3333-4444-5555-666677778888";
    let component = f3d_without_brep("component-design", "component.f3d", &[]);
    let middle = f3d_without_brep(
        "assembly-design",
        "middle.f3d",
        &[("component.f3d", CHILD_ROLE)],
    );
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("middle.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("middle.f3d", middle.as_slice()),
            ("component.f3d", component.as_slice()),
        ],
    );
    let mut options = DecodeOptions::default();
    // The session ceiling includes member frames and decoded entity reconstruction.
    options.policy.limits.max_recursion_depth = 6;
    let error =
        crate::test_support::archive_session::decode(&mut Cursor::new(archive.clone()), &options)
            .expect_err("nested member redirections exceed the selected depth");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RecursionDepth
                && limit.limit == 6 && limit.used == 6 && limit.additional == 1
                && limit.reason == cadmpeg_core::decode::ResourceFailure::BudgetExceeded
    ));
    crate::test_support::archive_session::decode(
        &mut Cursor::new(archive),
        &DecodeOptions::default(),
    )
    .expect("the nested member graph fits the complete session policy");
}

#[test]
fn f3z_archive_composes_nonidentity_nested_occurrence_placements() {
    const CHILD_ROLE: &str = "11112222-3333-4444-5555-666677778888";
    let inner = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 2.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let outer = [
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let middle = f3d_without_brep_with_xref_placement(
        "assembly-design",
        "middle.f3d",
        "component.f3d",
        CHILD_ROLE,
        inner,
    );
    let root = f3d_without_brep_with_xref_placement(
        "assembly-design",
        "root.f3d",
        "middle.f3d",
        XREF_ROLE,
        outer,
    );
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("middle.f3d", middle.as_slice()),
            ("component.f3d", component.as_slice()),
        ],
    );

    let decoded = crate::test_support::archive_session::decode(
        &mut Cursor::new(archive),
        &DecodeOptions::default(),
    )
    .expect("nonidentity nested placements decode");
    let outer_id = cadmpeg_ir::ids::OccurrenceId::mint("f3d:model:occurrence#xref-0-0")
        .expect("outer occurrence identity");
    let child = decoded
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| {
            matches!(
                &occurrence.parent,
                cadmpeg_ir::products::OccurrenceParent::Occurrence { occurrence: parent }
                    if parent == &outer_id
            )
        })
        .expect("merged component occurrence retains its outer parent");
    let outer_occurrence = decoded
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.id == outer_id)
        .expect("outer occurrence");
    assert_eq!(outer_occurrence.transform.rows()[0][3], 10.0);
    assert_eq!(child.transform.rows()[1][3], 20.0);

    let graph = cadmpeg_ir::products::AssemblyGraph::new(&decoded.ir().model.occurrences)
        .expect("merged occurrence graph");
    let resolved = assembly::resolved_transform(&graph, &child.id)
        .expect("resolved nested occurrence transform");
    assert_eq!(resolved.rows()[0][3], 10.0);
    assert_eq!(resolved.rows()[1][3], 20.0);
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert!(decoded.ir().model.bodies[0].transform.is_none());
    let cadmpeg_ir::products::PrototypeReference::Local { definition } = &child.prototype else {
        panic!("merged member must have a local prototype");
    };
    assert_eq!(
        decoded
            .ir()
            .model
            .product_definitions
            .iter()
            .find(|item| &item.id == definition)
            .unwrap()
            .bodies,
        [decoded.ir().model.bodies[0].id.clone()]
    );
}

#[test]
fn f3z_archive_preserves_noncommuting_parent_and_child_placements() {
    const CHILD_ROLE: &str = "11112222-3333-4444-5555-666677778888";
    // The outer quarter-turn maps the inner +X translation to +Y. Reversing
    // composition instead leaves that translation on +X.
    let inner = [
        [1.0, 0.0, 0.0, 2.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let outer = [
        [0.0, -1.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let expected = [
        [0.0, -1.0, 0.0, 10.0],
        [1.0, 0.0, 0.0, 20.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let middle = f3d_without_brep_with_xref_placement(
        "assembly-design",
        "middle.f3d",
        "component.f3d",
        CHILD_ROLE,
        inner,
    );
    let root = f3d_without_brep_with_xref_placement(
        "assembly-design",
        "root.f3d",
        "middle.f3d",
        XREF_ROLE,
        outer,
    );
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("middle.f3d", middle.as_slice()),
            ("component.f3d", component.as_slice()),
        ],
    );
    let decoded = crate::test_support::archive_session::decode(
        &mut Cursor::new(&archive),
        &DecodeOptions::default(),
    )
    .expect("noncommuting nested placements");
    assert!(
        decoded.report().losses.iter().all(|loss| loss.code
            != F3dLossCode::XrefPlacementUndecoded.kind()
            && loss.code != F3dLossCode::XrefTableUndecoded.kind()),
        "{:?}",
        decoded.report().losses
    );
    let admitted: cadmpeg_ir::CadIr =
        serde_json::from_slice(&serde_json::to_vec(decoded.ir()).unwrap())
            .expect("complete merged CADIR admission");
    for ir in [decoded.ir(), &admitted] {
        assert_eq!(ir.model.occurrences.len(), 2);
        let outer_id =
            cadmpeg_ir::ids::OccurrenceId::mint("f3d:model:occurrence#xref-0-0").unwrap();
        let child = ir
            .model
            .occurrences
            .iter()
            .find(|occurrence| {
                matches!(&occurrence.parent,
                cadmpeg_ir::products::OccurrenceParent::Occurrence { occurrence: parent }
                if parent == &outer_id)
            })
            .expect("child remains relative to its containing occurrence");
        assert_eq!(
            child.transform.rows(),
            [
                [1.0, 0.0, 0.0, 20.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
        );
        let graph = cadmpeg_ir::products::AssemblyGraph::new(&ir.model.occurrences)
            .expect("nested occurrence graph");
        assert_eq!(
            assembly::resolved_transform(&graph, &child.id)
                .unwrap()
                .rows(),
            expected
        );
        assert_eq!(ir.model.bodies.len(), 1);
        assert!(ir.model.bodies[0].transform.is_none());
        let cadmpeg_ir::products::PrototypeReference::Local { definition } = &child.prototype
        else {
            panic!("merged member must have a local prototype");
        };
        assert_eq!(
            ir.model
                .product_definitions
                .iter()
                .find(|item| &item.id == definition)
                .unwrap()
                .bodies,
            [ir.model.bodies[0].id.clone()]
        );
    }
}

#[test]
fn f3z_archive_reports_reference_cycles_without_recursing() {
    const CHILD_ROLE: &str = "11112222-3333-4444-5555-666677778888";
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("middle.f3d", XREF_ROLE)]);
    let middle = f3d_without_brep("assembly-design", "middle.f3d", &[("root.f3d", CHILD_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("middle.f3d", middle.as_slice()),
        ],
    );

    let decoded = crate::test_support::archive_session::decode(
        &mut Cursor::new(archive),
        &DecodeOptions::default(),
    )
    .unwrap();

    assert!(decoded.report().losses.iter().any(|loss| {
        loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("reference cycle through root.f3d")
    }));
}

#[test]
fn f3z_prefix_detects_as_f3d() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("comp.f3d", component.as_slice()),
        ],
    );
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&F3dCodec, &archive[..512.min(archive.len())]),
        Confidence::High
    );
}
