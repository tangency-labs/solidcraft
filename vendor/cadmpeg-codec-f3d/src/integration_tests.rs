// SPDX-License-Identifier: Apache-2.0
//! End-to-end contracts over synthesized F3D and F3Z archives.

mod act_links;
mod align;
mod archive_session;
mod attribute_colors;
mod body_reference_tails;
mod configuration_parameters;
mod design_containment;
mod emboss_operands;
mod external_placement;
mod history_deltas;
mod history_scale;
mod inconsistent_records;
mod measured_geometry;
mod optional_payloads;
mod pcurve_uv_offset;
mod placement_history;
mod procedural_support;
mod product_owners;
mod profile_region_selection;
mod reference_rows;
mod sketch_regions;
mod work_planes;

use cadmpeg_test_support::{wire, EditableDecodeResult};

use cadmpeg_core::container::ContainerRole;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};

use crate::test_support::assembly_test::{f3d_without_brep, f3z_archive, XREF_ROLE};
use crate::test_support::native_test::{f3d_native, f3d_native_mut};
use crate::test_support::smbh_curves_test::synthetic_geometry_with_helix_curve_smbh;
use crate::test_support::smbh_geometry_test::{
    synthetic_geometry_smbh, synthetic_geometry_with_history_smbh,
};
use crate::test_support::smbh_pcurves_test::{
    synthetic_geometry_with_rational_pcurve_smbh, with_plane_pcurve_offset_vertices,
};
use crate::test_support::smbh_surfaces_test::{
    synthetic_compound_loft_smbh, synthetic_cyl_spl_sur_smbh, synthetic_profile_first_sweep_smbh,
};
use crate::test_support::zip_test::{
    f3d_with_configuration, f3d_with_smbh, f3d_with_smbh_and_manifest_version,
    f3d_with_smbh_and_protein,
};
use crate::F3dCodec;
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};

fn decode(bytes: Vec<u8>) -> EditableDecodeResult {
    EditableDecodeResult::from(
        crate::test_support::archive_session::decode(
            &mut Cursor::new(bytes),
            &DecodeOptions::default(),
        )
        .expect("synthesized Fusion archive should decode"),
    )
}

fn assert_valid(result: &EditableDecodeResult) {
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("validate the neutral model");
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn f3d_pipeline_aligns_detection_inspection_container_roles_and_decode() {
    let bytes = f3d_with_smbh(&synthetic_geometry_smbh());
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&F3dCodec, &bytes),
        Confidence::High
    );
    let summary = F3dCodec
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .expect("F3D inspection");
    assert_eq!(summary.format(), "f3d");
    assert_eq!(summary.container_kind, "zip");
    assert!(summary
        .entries
        .iter()
        .any(|entry| entry.role == ContainerRole::BrepSmbh));

    let result = decode(bytes);
    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(!result.source_fidelity().retained_records().is_empty());
    assert_valid(&result);
}

/// A single-document archive names its own row, not the F3Z one.
#[test]
fn a_document_archive_reports_the_manifest_row_at_inspect_and_decode() {
    let document = f3d_with_smbh(&synthetic_geometry_smbh());

    let summary = F3dCodec
        .inspect(
            &mut Cursor::new(document.clone()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    let inspected = summary
        .dialects()
        .expect("inspect must report exactly one primary F3D layer")
        .primary()
        .clone();
    let inspected_dialects = summary.dialects();
    assert_eq!(inspected.format(), "f3d");
    assert_eq!(inspected.dialect().as_str(), "f3d:manifest-3-2-0-0");
    assert_eq!(
        inspected.declared()["top_level_manifest_version"],
        "3-2-0-0"
    );
    assert_eq!(
        inspected.admission(),
        &cadmpeg_core::dialect::Admission::Admitted
    );

    let decoded = EditableDecodeResult::from(
        F3dCodec
            .decode(&mut Cursor::new(document), &DecodeOptions::default())
            .unwrap(),
    );
    assert_eq!(decoded.report().dialects(), inspected_dialects);
    let source = decoded.ir().source.as_ref().unwrap();
    assert_eq!(source.dialect(), Some(&inspected));
}

#[test]
fn geometry_pipeline_composes_topology_pcurves_freeform_and_procedural_families() {
    let fixtures = [
        (synthetic_geometry_smbh(), true),
        (
            with_plane_pcurve_offset_vertices(
                synthetic_geometry_with_rational_pcurve_smbh(),
                [-1.0, 2.0],
            ),
            true,
        ),
        (synthetic_geometry_with_helix_curve_smbh(), false),
        (synthetic_cyl_spl_sur_smbh(), true),
        (synthetic_profile_first_sweep_smbh(), true),
        (synthetic_compound_loft_smbh(), true),
    ];
    let mut saw_pcurve = false;
    let mut saw_nurbs = false;
    let mut saw_procedural_curve = false;
    let mut saw_procedural_surface = false;
    for (smbh, geometrically_consistent) in fixtures {
        let result = decode(f3d_with_smbh(&smbh));
        saw_pcurve |= !result.ir().model.pcurves.is_empty();
        saw_nurbs |= result.ir().model.curves.iter().any(|curve| {
            matches!(
                curve.geometry,
                cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_))
            )
        }) || result.ir().model.surfaces.iter().any(|surface| {
            matches!(
                surface.geometry,
                cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))
            )
        });
        saw_procedural_curve |= !result.ir().model.procedural_curves.is_empty();
        saw_procedural_surface |= !result.ir().model.procedural_surfaces.is_empty();
        if geometrically_consistent {
            assert_valid(&result);
        } else {
            assert!(result.ir().native.namespace("f3d").is_some());
        }
    }
    assert!(saw_pcurve && saw_nurbs && saw_procedural_curve && saw_procedural_surface);
}

#[test]
fn design_pipeline_correlates_protein_properties_history_sketches_and_configuration() {
    use crate::test_support::configuration_table::{
        property_target, root_record, string_cell, table_document, with_root_segment, ROOT_RECORD,
    };
    let protein = decode(f3d_with_smbh_and_protein(
        &synthetic_geometry_with_history_smbh(),
    ));
    let native = f3d_native(protein.ir());
    assert!(!native.design_record_headers.is_empty());
    assert!(!native.asm_histories.is_empty());
    assert!(!protein.ir().model.appearances.is_empty());
    assert_valid(&protein);

    let name = "FusionAssetName[Active]/DesignConfigurationTable.integration.dsgcfg";
    let payload = serde_json::to_vec(&table_document(
        "1",
        &[("P", property_target("Part Number"))],
        &[("wide", "Wide", vec![("P", string_cell("W-1"))])],
        &["wide"],
    ))
    .unwrap();
    let configured = decode(with_root_segment(
        f3d_with_configuration(&synthetic_geometry_smbh(), name, &payload),
        root_record(*b"256", ROOT_RECORD, "1", "wide"),
    ));
    assert_eq!(configured.ir().model.configurations.len(), 1);
    assert!(configured.ir().model.configurations[0].active);
    assert_eq!(
        configured.ir().model.configurations[0].properties["property:Part Number"],
        "W-1"
    );
    assert_valid(&configured);
}

#[test]
fn preserved_source_pipeline_applies_semantic_geometry_edits_without_losing_archive_entries() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = decode(source);
    let mut edited = decoded.ir().clone();
    let moved = edited.model.points[0].position().get();
    edited.model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            2.5, moved.y, moved.z,
        ))
        .expect("a finite position is a point"),
    );
    edited.model.faces[0].sense = cadmpeg_ir::topology::Sense::Reversed;
    let mut bytes = Vec::new();
    crate::test_support::plan_inherited_write(&edited, decoded.source_fidelity(), &mut bytes)
        .expect("preserved F3D write");
    let round_trip = decode(bytes);
    assert_eq!(round_trip.ir().model.points[0].position().get().x, 2.5);
    assert_eq!(
        round_trip.ir().model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_valid(&round_trip);
}

#[test]
fn source_less_writer_pipeline_emits_a_fresh_valid_archive() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.source = None;
    drop(f3d_native_mut(&mut ir));
    let mut bytes = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("source-less F3D encode");
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&F3dCodec, &bytes),
        Confidence::High
    );
    let round_trip = decode(bytes);
    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(round_trip.ir().model.faces.len(), 6);
    assert_valid(&round_trip);
}

#[test]
fn f3z_pipeline_merges_three_levels_of_occurrences() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let inner = f3d_without_brep(
        "assembly-design",
        "inner.f3d",
        &[("component.f3d", XREF_ROLE)],
    );
    let middle = f3d_without_brep("assembly-design", "middle.f3d", &[("inner.f3d", XREF_ROLE)]);
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("middle.f3d", XREF_ROLE)]);
    let merged = decode(f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("middle.f3d", middle.as_slice()),
            ("inner.f3d", inner.as_slice()),
            ("component.f3d", component.as_slice()),
        ],
    ));
    let scope = format!("xref/role-{XREF_ROLE}/reference-0/occurrence-0/");
    let expected = format!("f3d:{scope}{scope}{scope}");
    assert_eq!(merged.ir().model.bodies.len(), 1);
    assert!(merged.ir().model.bodies[0]
        .id
        .as_str()
        .starts_with(&expected));
    assert_valid(&merged);
}

#[test]
fn f3z_pipeline_recursively_merges_occurrences_and_reports_reference_cycles() {
    const CHILD_ROLE: &str = "11112222-3333-4444-5555-666677778888";
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let middle = f3d_without_brep(
        "assembly-design",
        "middle.f3d",
        &[("component.f3d", CHILD_ROLE)],
    );
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("middle.f3d", XREF_ROLE)]);
    let merged = decode(f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("middle.f3d", middle.as_slice()),
            ("component.f3d", component.as_slice()),
        ],
    ));
    assert!(merged.ir().model.bodies[0].id.as_str().contains(&format!(
        "xref/role-{XREF_ROLE}/reference-0/occurrence-0/xref/role-{CHILD_ROLE}/reference-0/occurrence-0/"
    )));
    assert_valid(&merged);

    let cyclic_middle =
        f3d_without_brep("assembly-design", "middle.f3d", &[("root.f3d", CHILD_ROLE)]);
    let cyclic = decode(f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("middle.f3d", cyclic_middle.as_slice()),
        ],
    ));
    assert!(cyclic
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("reference cycle")));
}

/// A geometry request stops a standalone geometry document before design
/// intent. A document with no body source classifies itself by what design
/// intent projects, and an F3Z archive merges its members into one document,
/// so both decode in full. Each agrees with its full decode.
#[test]
fn geometry_depth_cuts_only_documents_that_agree_without_design_intent() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let root = f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("component.f3d", XREF_ROLE)],
    );
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("component.f3d", component.as_slice()),
        ],
    );
    let inputs = [
        ("standalone", component),
        ("no body source", root),
        ("archive", archive),
    ];
    let delivered = inputs
        .iter()
        .map(|(name, bytes)| {
            let result = crate::test_support::archive_session::decode(
                &mut Cursor::new(bytes),
                &DecodeOptions {
                    depth: cadmpeg_core::decode::DecodeDepth::Geometry,
                    ..DecodeOptions::default()
                },
            )
            .expect("geometry-depth decode");
            (*name, result.ir().decode_depth())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        delivered,
        [
            ("standalone", cadmpeg_core::decode::DecodeDepth::Geometry),
            ("no body source", cadmpeg_core::decode::DecodeDepth::Full),
            ("archive", cadmpeg_core::decode::DecodeDepth::Full),
        ]
    );
    let summary = cadmpeg_test_support::depth::assert_geometry_agrees_with_full(&F3dCodec, &inputs);
    assert_eq!(summary.cut, 1, "{summary:?}");
}

#[test]
fn container_only_pipeline_retains_native_sections_without_semantic_projection() {
    let bytes = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let result = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(bytes),
                &DecodeOptions {
                    depth: cadmpeg_core::decode::DecodeDepth::Container,
                    ..DecodeOptions::default()
                },
            )
            .expect("container-only F3D decode"),
    );
    assert!(result.report().container_only());
    assert!(!result.report().geometry_transferred());
    assert!(result.ir().model.bodies.is_empty());
    assert!(!result.source_fidelity().retained_records().is_empty());
    assert!(result.ir().native.namespace("f3d").is_some());
}

#[test]
fn a_version_only_manifest_drift_decodes_as_unverified_and_charges_the_recovery() {
    // The archive differs from the known-version archive in the manifest
    // version field alone, so the whole decode runs the same code the known
    // version runs. What changes is the identity claim: the row is the
    // recovery row, the admission names the strategy applied, and the report
    // charges the dialect-unverified loss.
    let known = decode(f3d_with_smbh(&synthetic_geometry_smbh()));
    let drifted = decode(f3d_with_smbh_and_manifest_version(
        &synthetic_geometry_smbh(),
        "3-3-0-0",
    ));

    assert!(drifted.report().geometry_transferred());
    assert_eq!(
        drifted.ir().model.bodies.len(),
        known.ir().model.bodies.len()
    );

    let matched = drifted
        .report()
        .dialects()
        .as_ref()
        .expect("the primary layer is classified")
        .primary();
    assert_eq!(matched.dialect().as_str(), "f3d:unknown");
    assert!(matches!(
        matched.admission(),
        cadmpeg_core::dialect::Admission::Unverified { .. }
    ));
    assert_eq!(
        matched.using(),
        Some(cadmpeg_core::dialect_id!("f3d:manifest-3-2-0-0"))
    );
    assert_eq!(matched.declared()["top_level_manifest_version"], "3-3-0-0");

    let expected = crate::loss::F3dLossCode::SourceDialectUnverified.kind();
    assert!(
        drifted
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == expected),
        "the recovery must be charged"
    );
    assert!(
        !known
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == expected),
        "a known version charges no recovery"
    );
}

// --------------------------------------------------------------------------
// Resolution of a write request against the source and target-report honesty
// on every write path.
// --------------------------------------------------------------------------

fn plan(
    result: &EditableDecodeResult,
    fidelity: bool,
    request: TargetRequest<'_>,
) -> Result<cadmpeg_ir::codec::write::ExportPlan, cadmpeg_core::CodecError> {
    F3dCodec.plan(
        EncodeInput::new(result.ir(), fidelity.then(|| result.source_fidelity())),
        request,
    )
}

fn named_target(plan: &cadmpeg_ir::codec::write::ExportPlan) -> String {
    wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
        plan.report(),
        "identity/target",
    )
    .as_ref()
    .expect("an F3D write always names its dialect")
    .to_string()
}

/// The flagship case: `convert in.f3d -o out.f3d` on an archive that is not the
/// catalog row keeps the archive it was handed, and says so.
///
/// The command line builds `Inherit` for a same-format conversion, and the
/// resolved dialect is then the source's by construction, so the replay law
/// admits preservation. Before resolution the report named no dialect at all,
/// so the bytes went out with the claim unstated.
#[test]
fn inherit_replays_an_off_catalog_dialect_and_names_it() {
    let source = f3d_with_smbh_and_manifest_version(&synthetic_geometry_smbh(), "3-3-0-0");
    let result = decode(source.clone());
    let plan = plan(&result, true, TargetRequest::Inherit).expect("preservation is available");

    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::VerbatimReplay { .. }
    ));
    assert_eq!(named_target(&plan), "f3d:unknown");
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    assert_eq!(written, source);
}

/// A dialect this codec reads but cannot generate refuses under `Inherit` once
/// its retained image is gone. There is no fall-through to the catalog row: a
/// same-format conversion never silently changes what the archive is, and the
/// refusal names both the source's dialect and the escape.
#[test]
fn inherit_refuses_an_off_catalog_source_dialect_with_no_retained_image() {
    let result = decode(f3d_with_smbh_and_manifest_version(
        &synthetic_geometry_smbh(),
        "3-3-0-0",
    ));
    let error = plan(&result, false, TargetRequest::Inherit)
        .expect_err("f3d:unknown is not a synthesis target");
    let cadmpeg_core::CodecError::UnsupportedTarget(refusal) = &error else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(refusal.format(), "f3d");
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
        Some("f3d:unknown")
    );
    assert!(
        refusal
            .available()
            .iter()
            .any(|target| target.id.as_str() == "f3d:manifest-3-2-0-0"),
        "{:?}",
        refusal.available()
    );
}

/// The replay law's compare, not the presence of an image: an explicit catalog
/// row over a retained archive of a different dialect regenerates rather than
/// replaying bytes it would then have to misname.
#[test]
fn an_explicit_catalog_row_does_not_replay_a_different_dialect() {
    let source = f3d_with_smbh_and_manifest_version(&synthetic_geometry_smbh(), "3-3-0-0");
    let result = decode(source.clone());
    let plan = plan(
        &result,
        true,
        TargetRequest::Explicit("f3d:manifest-3-2-0-0"),
    )
    .expect("the catalog row is synthesizable");

    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    assert_eq!(named_target(&plan), "f3d:manifest-3-2-0-0");
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    assert_ne!(written, source, "the source archive must not be replayed");
}

/// A same-dialect `Inherit` still replays, and an explicit id naming the very
/// dialect the source is still replays: the resolution gates preservation on
/// the dialect compare and on nothing else.
#[test]
fn a_same_dialect_request_replays_under_both_spellings() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let result = decode(source.clone());
    for request in [
        TargetRequest::Inherit,
        TargetRequest::Explicit("f3d:manifest-3-2-0-0"),
        TargetRequest::Explicit("3-2-0-0"),
    ] {
        let plan = plan(&result, true, request).expect("the source's own dialect is writable");
        assert!(matches!(
            plan.report().write_path(),
            cadmpeg_ir::report::export::WritePath::VerbatimReplay { .. }
        ));
        assert_eq!(named_target(&plan), "f3d:manifest-3-2-0-0");
        let mut written = Vec::new();
        plan.write_to(&mut written).unwrap();
        assert_eq!(written, source, "{request:?}");
    }
}

/// The patch path names a dialect too, and it is the source's: patching
/// rewrites members inside the retained archive and never its `Manifest.dat`,
/// so the output is still whatever the source was.
#[test]
fn the_patch_path_names_the_preserved_dialect() {
    let result = decode(f3d_with_smbh_and_manifest_version(
        &synthetic_geometry_smbh(),
        "3-3-0-0",
    ));
    assert!(
        !result.ir().model.points.is_empty(),
        "the patch lane needs an editable point"
    );
    let mut edited = result.ir().clone();
    let moved = edited.model.points[0].position().get();
    edited.model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x + 1.0,
            moved.y,
            moved.z,
        ))
        .expect("a finite position is a point"),
    );

    let plan = F3dCodec
        .plan(
            EncodeInput::new(&edited, Some(result.source_fidelity())),
            TargetRequest::Inherit,
        )
        .expect("an edited archive still preserves its dialect");
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Patched { .. }
    ));
    assert_eq!(named_target(&plan), "f3d:unknown");

    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    let redecoded = EditableDecodeResult::from(
        F3dCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .expect("the patched archive decodes"),
    );
    assert_eq!(
        redecoded
            .report()
            .dialects()
            .map(cadmpeg_core::dialect::DialectLayers::primary)
            .map(cadmpeg_core::dialect::DialectMatch::dialect)
            .map(cadmpeg_core::dialect::DialectId::as_str),
        Some("f3d:unknown")
    );
}

/// The §8.3 honesty invariant, on the preservation and generation paths:
/// re-decoding the output classifies the host layer into exactly the dialect
/// the report named.
///
/// The assertion is against the bytes, not against the report twice. `target`
/// is a claim about what was written, and the only thing that can check it is
/// reading the bytes back through the classifier the codec uses on any other
/// input. Replay carries the source's own `Manifest.dat` version through, and
/// generation pins the catalog row.
#[test]
fn every_write_path_re_decodes_as_the_dialect_the_report_named() {
    let replayed = decode(f3d_with_smbh_and_manifest_version(
        &synthetic_geometry_smbh(),
        "3-3-0-0",
    ));
    let synthesized = decode(f3d_with_smbh(&synthetic_geometry_smbh()));

    let replay_matches: fn(&cadmpeg_ir::report::export::WritePath) -> bool = |path| {
        matches!(
            path,
            cadmpeg_ir::report::export::WritePath::VerbatimReplay { .. }
        )
    };
    let synthesize_matches: fn(&cadmpeg_ir::report::export::WritePath) -> bool = |path| {
        matches!(
            path,
            cadmpeg_ir::report::export::WritePath::Synthesized { .. }
        )
    };
    for (label, result, fidelity, path_matches) in [
        ("replay", &replayed, true, replay_matches),
        ("synthesize", &synthesized, false, synthesize_matches),
    ] {
        let plan = plan(result, fidelity, TargetRequest::Inherit)
            .unwrap_or_else(|error| panic!("{label} must plan, got {error}"));
        let path = plan.report().write_path();
        assert!(path_matches(path), "{label}: took the {path} path");
        let claimed = named_target(&plan);
        let mut written = Vec::new();
        plan.write_to(&mut written).unwrap();

        let redecoded = EditableDecodeResult::from(
            F3dCodec
                .decode(&mut Cursor::new(written), &DecodeOptions::default())
                .unwrap_or_else(|error| panic!("{label} output must decode, got {error}")),
        );
        let classified = redecoded
            .report()
            .dialects()
            .unwrap_or_else(|| panic!("{label} output must classify a host dialect"))
            .primary()
            .dialect()
            .clone();
        assert_eq!(
            classified.as_str(),
            claimed,
            "{label}: the report claims {claimed} but the bytes are {classified}"
        );
    }
}

mod dimension_annotations;
