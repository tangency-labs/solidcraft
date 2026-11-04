// SPDX-License-Identifier: Apache-2.0
//! Golden-snapshot harness for decode, inspect, and encode branches.
//!
//! `tests/golden/fixtures/*.f3d` are frozen inputs, and every snapshot here is
//! produced from the committed bytes. Regenerate the artifacts with
//! `UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-f3d golden` and review the
//! diff. A regeneration writes only the artifacts their own comparison
//! rejects, so one over a clean tree leaves the tree clean. Fixture
//! regeneration is separate: `UPDATE_GOLDEN_FIXTURES=1`.

use cadmpeg_test_support::EditableDecodeResult;

use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions, DecodeResult};
use cadmpeg_ir::examples;
use cadmpeg_ir::{report::export::WritePath, CadIr};
use cadmpeg_measure::test_support::round_trip::{measures_agree, measures_decide};
use cadmpeg_test_support::golden::{elide_local_digests, snapshot_text, snapshots_agree};
use cadmpeg_test_support::roundtrip::{
    mutation_roundtrip, semantic_roundtrip, verbatim_replay_holds, ExpectedWritePath,
    MutationOutcome, SemanticOutcome,
};

use super::F3dCodec;
use crate::test_support::native_test::TestEncode;
use crate::test_support::smbh_bf4_test::{
    synthetic_geometry_bf4_nurbs_smbh, synthetic_geometry_bf4_smbh,
};
use crate::test_support::smbh_blends_test::{
    synthetic_full_rolling_ball_smbh, synthetic_g2_blend_spl_sur_smbh,
    synthetic_variable_blend_smbh, synthetic_vertex_blend_smbh,
};
use crate::test_support::smbh_curves_test::{
    synthetic_geometry_with_compound_curve_smbh, synthetic_geometry_with_exact_curve_smbh,
    synthetic_geometry_with_helix_curve_smbh, synthetic_geometry_with_law_curve_smbh,
    synthetic_geometry_with_projection_smbh, synthetic_geometry_with_silhouette_smbh,
    synthetic_geometry_with_spring_smbh, synthetic_geometry_with_subset_curve_smbh,
    synthetic_geometry_with_surface_intersection_smbh, synthetic_geometry_with_surface_offset_smbh,
};
use crate::test_support::smbh_geometry_test::{
    synthetic_free_vertex_body_smbh, synthetic_geometry_smbh,
    synthetic_geometry_with_attribute_smbh, synthetic_geometry_with_body_color_smbh,
    synthetic_geometry_with_degenerate_curve_smbh, synthetic_geometry_with_face_color_smbh,
    synthetic_geometry_with_history_smbh, synthetic_geometry_with_mesh_surface_smbh,
    synthetic_geometry_with_sketch_link_smbh, synthetic_geometry_with_transform_smbh,
    synthetic_mixed_face_wire_body_smbh, synthetic_wire_body_smbh, SketchLinkForm,
};
use crate::test_support::smbh_header_test::synthetic_smbh;
use crate::test_support::smbh_pcurves_test::{
    synthetic_geometry_with_pcurve_smbh, synthetic_geometry_with_rational_pcurve_smbh,
    synthetic_geometry_with_ref_pcurve_smbh, with_pcurve_uv_offset,
};
use crate::test_support::smbh_surfaces_test::{
    synthetic_comp_spl_sur_smbh, synthetic_cyl_spl_sur_smbh, synthetic_exact_spl_sur_smbh,
    synthetic_helix_surface_smbh, synthetic_law_spl_sur_smbh, synthetic_loft_spl_sur_smbh,
    synthetic_minimal_deformable_surface_smbh, synthetic_net_spl_sur_smbh,
    synthetic_off_spl_sur_smbh, synthetic_profile_first_sweep_smbh, synthetic_rot_spl_sur_smbh,
    synthetic_ruled_spl_sur_smbh, synthetic_scaled_compound_loft_smbh, synthetic_skin_spl_sur_smbh,
    synthetic_sub_spl_sur_smbh, synthetic_sum_spl_sur_smbh, synthetic_t_spl_sur_smbh,
    synthetic_taper_spl_sur_smbh,
};
use crate::test_support::zip_test::{
    f3d_with_configuration, f3d_with_smbh, f3d_with_smbh_and_protein,
    f3d_with_smbh_and_protein_with_generated_base_feature,
    f3d_with_smbh_and_protein_with_generated_base_flange,
    f3d_with_smbh_and_protein_with_generated_copy_paste,
    f3d_with_smbh_and_protein_with_generated_copy_paste_bodies,
    f3d_with_smbh_and_protein_with_generated_form,
    f3d_with_smbh_and_protein_with_generated_remove_body,
    f3d_with_smbh_and_protein_with_generated_sketch_dimension,
    f3d_with_smbh_and_protein_with_generated_surface_stitch,
};
use cadmpeg_core::CodecError;

/// Covering fixture set as `(golden name, full .f3d bytes)`.
#[allow(clippy::vec_init_then_push)]
fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("topology_base", f3d_with_smbh(&synthetic_geometry_smbh())),
        ("wire_body", f3d_with_smbh(&synthetic_wire_body_smbh())),
        (
            "free_vertex_body",
            f3d_with_smbh(&synthetic_free_vertex_body_smbh()),
        ),
        (
            "mixed_face_wire_body",
            f3d_with_smbh(&synthetic_mixed_face_wire_body_smbh()),
        ),
        (
            "attributes",
            f3d_with_smbh(&synthetic_geometry_with_attribute_smbh()),
        ),
        (
            "body_color",
            f3d_with_smbh(&synthetic_geometry_with_body_color_smbh()),
        ),
        (
            "face_color",
            f3d_with_smbh(&synthetic_geometry_with_face_color_smbh()),
        ),
        (
            "body_transform",
            f3d_with_smbh(&synthetic_geometry_with_transform_smbh()),
        ),
        (
            "history",
            f3d_with_smbh(&synthetic_geometry_with_history_smbh()),
        ),
        (
            "design_appearances_protein",
            f3d_with_smbh_and_protein(&synthetic_geometry_smbh()),
        ),
        (
            "design_sketch_constraints",
            f3d_with_smbh_and_protein_with_generated_sketch_dimension(&synthetic_geometry_smbh()),
        ),
        (
            "design_base_feature",
            f3d_with_smbh_and_protein_with_generated_base_feature(&synthetic_geometry_smbh()),
        ),
        (
            "design_base_flange",
            f3d_with_smbh_and_protein_with_generated_base_flange(&synthetic_geometry_smbh()),
        ),
        (
            "design_remove_body",
            f3d_with_smbh_and_protein_with_generated_remove_body(&synthetic_geometry_smbh()),
        ),
        (
            "design_surface_stitch",
            f3d_with_smbh_and_protein_with_generated_surface_stitch(&synthetic_geometry_smbh()),
        ),
        (
            "design_copy_paste",
            f3d_with_smbh_and_protein_with_generated_copy_paste(&synthetic_geometry_smbh()),
        ),
        (
            "design_copy_paste_bodies",
            f3d_with_smbh_and_protein_with_generated_copy_paste_bodies(&synthetic_geometry_smbh()),
        ),
        (
            "design_form",
            f3d_with_smbh_and_protein_with_generated_form(&synthetic_geometry_smbh()),
        ),
        ("container_metadata_only", f3d_with_smbh(&synthetic_smbh())),
        (
            "mesh_surface",
            f3d_with_smbh(&synthetic_geometry_with_mesh_surface_smbh()),
        ),
        (
            "sketch_link",
            f3d_with_smbh(&synthetic_geometry_with_sketch_link_smbh(
                SketchLinkForm::Tagged("113 0 1 0 2 3"),
            )),
        ),
        (
            "pcurve_inline",
            f3d_with_smbh(&with_pcurve_uv_offset(
                synthetic_geometry_with_pcurve_smbh(),
                [0.0, 0.0],
            )),
        ),
        (
            "pcurve_ref",
            f3d_with_smbh(&with_pcurve_uv_offset(
                synthetic_geometry_with_ref_pcurve_smbh(),
                [0.0, 0.0],
            )),
        ),
        (
            "pcurve_rational",
            f3d_with_smbh(&with_pcurve_uv_offset(
                synthetic_geometry_with_rational_pcurve_smbh(),
                [0.0, 0.0],
            )),
        ),
        (
            "exact_curve",
            f3d_with_smbh(&synthetic_geometry_with_exact_curve_smbh()),
        ),
        (
            "subset_curve",
            f3d_with_smbh(&synthetic_geometry_with_subset_curve_smbh()),
        ),
        (
            "compound_curve",
            f3d_with_smbh(&synthetic_geometry_with_compound_curve_smbh()),
        ),
        (
            "helix_curve",
            f3d_with_smbh(&synthetic_geometry_with_helix_curve_smbh()),
        ),
        (
            "law_curve",
            f3d_with_smbh(&synthetic_geometry_with_law_curve_smbh()),
        ),
        (
            "silhouette_curve",
            f3d_with_smbh(&synthetic_geometry_with_silhouette_smbh(
                "para_silh_int_cur",
                None,
            )),
        ),
        (
            "surface_offset_curve",
            f3d_with_smbh(&synthetic_geometry_with_surface_offset_smbh()),
        ),
        (
            "spring_curve",
            f3d_with_smbh(&synthetic_geometry_with_spring_smbh()),
        ),
        (
            "projection_curve",
            f3d_with_smbh(&synthetic_geometry_with_projection_smbh()),
        ),
        (
            "surface_intersection_curve",
            f3d_with_smbh(&synthetic_geometry_with_surface_intersection_smbh()),
        ),
        (
            "degenerate_curve",
            f3d_with_smbh(&synthetic_geometry_with_degenerate_curve_smbh()),
        ),
        (
            "cyl_spline_surface",
            f3d_with_smbh(&synthetic_cyl_spl_sur_smbh()),
        ),
        (
            "exact_surface",
            f3d_with_smbh(&synthetic_exact_spl_sur_smbh("exact_spl_sur")),
        ),
        (
            "ruled_surface",
            f3d_with_smbh(&synthetic_ruled_spl_sur_smbh("rule_sur", true)),
        ),
        (
            "revolution_surface",
            f3d_with_smbh(&synthetic_rot_spl_sur_smbh("rot_spl_sur")),
        ),
        (
            "offset_surface",
            f3d_with_smbh(&synthetic_off_spl_sur_smbh("off_spl_sur")),
        ),
        (
            "taper_surface",
            f3d_with_smbh(&synthetic_taper_spl_sur_smbh("taper_spl_sur")),
        ),
        (
            "sum_surface",
            f3d_with_smbh(&synthetic_sum_spl_sur_smbh("sum_spl_sur", true)),
        ),
        (
            "sub_surface",
            f3d_with_smbh(&synthetic_sub_spl_sur_smbh("sub_spl_sur")),
        ),
        (
            "loft_surface",
            f3d_with_smbh(&synthetic_loft_spl_sur_smbh("loft_spl_sur")),
        ),
        (
            "compound_loft_surface",
            f3d_with_smbh(&synthetic_comp_spl_sur_smbh()),
        ),
        (
            "scaled_compound_loft_surface",
            f3d_with_smbh(&synthetic_scaled_compound_loft_smbh(true)),
        ),
        (
            "law_surface",
            f3d_with_smbh(&synthetic_law_spl_sur_smbh("law_spl_sur", false, 0)),
        ),
        (
            "skin_surface",
            f3d_with_smbh(&synthetic_skin_spl_sur_smbh(0, false)),
        ),
        ("net_surface", f3d_with_smbh(&synthetic_net_spl_sur_smbh())),
        (
            "sweep_surface",
            f3d_with_smbh(&synthetic_profile_first_sweep_smbh()),
        ),
        (
            "g2_blend_surface",
            f3d_with_smbh(&synthetic_g2_blend_spl_sur_smbh("g2_blend_spl_sur", true)),
        ),
        (
            "variable_blend_surface",
            f3d_with_smbh(&synthetic_variable_blend_smbh("var_blend_spl_sur")),
        ),
        (
            "rolling_ball_blend_surface",
            f3d_with_smbh(&synthetic_full_rolling_ball_smbh("rb_blend_spl_sur")),
        ),
        (
            "vertex_blend_surface",
            f3d_with_smbh(&synthetic_vertex_blend_smbh("VBL_SURF")),
        ),
        (
            "helix_surface",
            f3d_with_smbh(&synthetic_helix_surface_smbh(true)),
        ),
        (
            "tspline_surface",
            f3d_with_smbh(&synthetic_t_spl_sur_smbh()),
        ),
        (
            "deformable_surface",
            f3d_with_smbh(&synthetic_minimal_deformable_surface_smbh()),
        ),
        ("binaryfile4", f3d_with_smbh(&synthetic_geometry_bf4_smbh())),
        (
            "binaryfile4_nurbs",
            f3d_with_smbh(&synthetic_geometry_bf4_nurbs_smbh()),
        ),
        (
            "design_configuration",
            f3d_with_configuration(
                &synthetic_geometry_smbh(),
                "FusionAssetName[Active]/DesignConfigurationTable.123.dsgcfg",
                &golden_configuration_document(),
            ),
        ),
        ("generated_unit_cube", encoder_generated_unit_cube()),
    ]
}

/// A table whose parameter and feature columns name records that the
/// geometry-only fixture lacks, beside a property column.
fn golden_configuration_document() -> Vec<u8> {
    use crate::test_support::configuration_table::{
        bool_cell, expression_cell, object_target, property_target, string_cell, table_document,
    };
    serde_json::to_vec(&table_document(
        "1",
        &[
            ("P", property_target("Part Number")),
            ("W", object_target("71", "UserParameterValue")),
            ("S", object_target("40", "DcFeature")),
        ],
        &[
            (
                "wide",
                "Wide",
                vec![
                    ("P", string_cell("W-1")),
                    ("W", expression_cell("25 mm", 2.5)),
                    ("S", bool_cell(false)),
                ],
            ),
            (
                "narrow",
                "Narrow",
                vec![
                    ("P", string_cell("N-1")),
                    ("W", expression_cell("10 mm", 1.0)),
                    ("S", bool_cell(true)),
                ],
            ),
        ],
        &["narrow", "wide"],
    ))
    .unwrap()
}

fn encoder_generated_unit_cube() -> Vec<u8> {
    let ir = examples::unit_cube().expect("unit cube fixture is admitted");
    let mut bytes = Vec::new();
    F3dCodec
        .encode(&ir, &mut bytes)
        .expect("encode neutral unit-cube IR");
    bytes
}

fn golden_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn update_requested() -> bool {
    std::env::var_os("UPDATE_GOLDEN").is_some()
}

/// Whether the caller asked to rewrite the frozen `tests/golden/fixtures/*.f3d`
/// inputs from the builders (`UPDATE_GOLDEN_FIXTURES=1`).
fn fixture_update_requested() -> bool {
    std::env::var_os("UPDATE_GOLDEN_FIXTURES").is_some()
}

fn read_fixture(name: &str) -> Result<Vec<u8>, String> {
    let path = golden_root().join("fixtures").join(format!("{name}.f3d"));
    std::fs::read(&path).map_err(|error| {
        format!(
            "fixture `{name}`: cannot read {} ({error}); restore the committed input, or rebuild it from its builder with `UPDATE_GOLDEN_FIXTURES=1 cargo test -p cadmpeg-codec-f3d golden`",
            path.display()
        )
    })
}

fn decode_result(bytes: &[u8]) -> Result<DecodeResult, DecodeFailure> {
    F3dCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
}

fn indent_block(block: &str) -> String {
    let mut lines = block.lines();
    let mut out = String::new();
    if let Some(first) = lines.next() {
        out.push_str(first);
    }
    for line in lines {
        out.push('\n');
        out.push_str("  ");
        out.push_str(line);
    }
    out
}

fn decode_snapshot(bytes: &[u8]) -> String {
    match decode_result(bytes) {
        Ok(result) => {
            let mut result = EditableDecodeResult::from(result);
            if let Some(source) = result.ir_mut().source.as_mut() {
                elide_local_digests(&mut source.attributes);
            }
            let ir = result
                .ir()
                .to_canonical_json()
                .expect("serialize canonical ir");
            let report = serde_json::to_string_pretty(result.report()).expect("serialize report");
            let fidelity = serde_json::to_string_pretty(result.source_fidelity())
                .expect("serialize source_fidelity");
            let mut out = String::from("{\n");
            out.push_str("  \"ir\": ");
            out.push_str(&indent_block(&ir));
            out.push_str(",\n  \"report\": ");
            out.push_str(&indent_block(&report));
            out.push_str(",\n  \"source_fidelity\": ");
            out.push_str(&indent_block(&fidelity));
            out.push_str("\n}\n");
            out
        }
        Err(error) => {
            let value = serde_json::json!({ "decode_error": error.to_string() });
            let mut text = serde_json::to_string_pretty(&value).expect("serialize decode error");
            text.push('\n');
            text
        }
    }
}

fn inspect_snapshot(bytes: &[u8]) -> String {
    let value = match F3dCodec.inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default())
    {
        Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect"),
        Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
    };
    let mut text = serde_json::to_string_pretty(&value).expect("serialize inspect snapshot");
    text.push('\n');
    text
}

/// Encodes an unedited decode result via the verbatim-replay branch.
fn replay_outcome(bytes: &[u8]) -> Option<Result<Vec<u8>, String>> {
    let result = EditableDecodeResult::from(decode_result(bytes).ok()?);
    let mut out = Vec::new();
    let outcome = match F3dCodec.plan(
        EncodeInput::new(result.ir(), Some(result.source_fidelity())),
        TargetRequest::Inherit,
    ) {
        Ok(plan) => {
            let path = plan.report().write_path().clone();
            match plan.write_to(&mut out) {
                Ok(_) => {
                    assert!(
                        matches!(path, WritePath::VerbatimReplay { .. }),
                        "the replay lane must take the verbatim-replay write path"
                    );
                    Ok(out)
                }
                Err(error) => Err(error.to_string()),
            }
        }
        Err(error) => Err(error.to_string()),
    };
    Some(outcome)
}

fn generate_outcome(bytes: &[u8]) -> Option<Result<Vec<u8>, String>> {
    let result = decode_result(bytes).ok()?;
    let mut out = Vec::new();
    Some(match F3dCodec.encode(result.ir(), &mut out) {
        Ok(report) => {
            assert!(
                matches!(report.write_path(), WritePath::Synthesized { .. }),
                "the generate lane withholds the sidecar, so the writer must author every byte"
            );
            Ok(out)
        }
        Err(error) => Err(error.to_string()),
    })
}

fn patch_outcome(bytes: &[u8]) -> Option<Result<Vec<u8>, String>> {
    let result = EditableDecodeResult::from(decode_result(bytes).ok()?);
    if result.ir().model.points.is_empty() {
        return None;
    }
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
    let mut out = Vec::new();
    Some(
        match crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut out)
        {
            Ok(path) => {
                assert!(
                    matches!(path, WritePath::Patched { .. }),
                    "the patch lane edits the IR, so the writer must run"
                );
                Ok(out)
            }
            Err(error) => Err(error.to_string()),
        },
    )
}

fn first_line_diff(expected: &str, actual: &str) -> (usize, String, String) {
    let mut exp = expected.lines();
    let mut act = actual.lines();
    let mut line = 0usize;
    loop {
        line += 1;
        match (exp.next(), act.next()) {
            (Some(expected_line), Some(actual_line)) if expected_line == actual_line => {}
            (expected_line, actual_line) => {
                let trunc = |value: Option<&str>| match value {
                    Some(text) if text.len() > 200 => format!("{}…", &text[..200]),
                    Some(text) => text.to_string(),
                    None => "<end of file>".to_string(),
                };
                return (line, trunc(expected_line), trunc(actual_line));
            }
        }
    }
}

fn first_byte_diff(expected: &[u8], actual: &[u8]) -> String {
    let offset = expected
        .iter()
        .zip(actual)
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| expected.len().min(actual.len()));
    let describe = |bytes: &[u8]| match bytes.get(offset) {
        Some(byte) => format!("0x{byte:02x}"),
        None => "<end of file>".to_string(),
    };
    format!(
        "first byte difference at offset {offset}: golden {}, actual {} (lengths: {} and {})",
        describe(expected),
        describe(actual),
        expected.len(),
        actual.len(),
    )
}

/// Writes one golden, creating its branch directory.
///
/// Reached only where a comparison rejected the committed golden, so a
/// regeneration over a clean tree writes nothing.
fn write_golden(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().expect("golden path has a parent"))
        .expect("create golden dir");
    std::fs::write(path, bytes).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

fn compare_text(update: bool, path: &Path, actual: &str, failures: &mut Vec<String>) {
    let failure = match std::fs::read_to_string(path) {
        Ok(expected) => {
            let expected = expected.replace("\r\n", "\n");
            let actual = actual.replace("\r\n", "\n");
            snapshots_agree(&expected, &actual)
                .err()
                .map(|mismatch| format!("{}: diverged {mismatch}", path.display()))
        }
        Err(error) => Some(format!(
            "{}: cannot read golden ({error}); regenerate with `UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-f3d golden`",
            path.display()
        )),
    };
    let Some(failure) = failure else { return };
    if update {
        write_golden(path, actual.as_bytes());
    } else {
        failures.push(failure);
    }
}

fn compare_bytes(update: bool, path: &Path, actual: &[u8], failures: &mut Vec<String>) {
    let failure = match std::fs::read(path) {
        Ok(expected) if expected == actual => None,
        Ok(expected) => Some(format!(
            "{}: {}",
            path.display(),
            first_byte_diff(&expected, actual)
        )),
        Err(error) => Some(format!(
            "{}: cannot read golden ({error}); regenerate with `UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-f3d golden`",
            path.display()
        )),
    };
    let Some(failure) = failure else { return };
    if update {
        write_golden(path, actual);
    } else {
        failures.push(failure);
    }
}

/// Serializes the document a generated container decodes to, with every
/// `*_sha256` attribute elided. Those digests cover written bytes that move
/// under platform libm while the geometry still agrees within tolerance.
fn generated_container_snapshot(bytes: &[u8]) -> String {
    let mut ir = match decode_result(bytes) {
        Ok(result) => result.into_parts().0,
        Err(error) => {
            let value = serde_json::json!({ "decode_error": error.to_string() });
            return serde_json::to_string_pretty(&value).expect("serialize decode error");
        }
    };
    if let Some(source) = ir.source.as_mut() {
        for (key, value) in &mut source.attributes {
            if key.as_str().ends_with("_sha256") {
                cadmpeg_test_support::golden::ELIDED_DIGEST.clone_into(value);
            }
        }
    }
    ir.to_canonical_json().expect("serialize canonical ir")
}

/// Compares produced bytes against a golden container by the document each
/// decodes to, tolerating last-place disagreement in decoded numbers.
fn compare_decoded_bytes(update: bool, path: &Path, actual: &[u8], failures: &mut Vec<String>) {
    let failure = match std::fs::read(path) {
        Ok(expected) if expected == actual => None,
        Ok(expected) => snapshots_agree(
            &generated_container_snapshot(&expected),
            &generated_container_snapshot(actual),
        )
        .err()
        .map(|mismatch| {
            format!(
                "{}: the produced container decodes differently: {mismatch}\n    {}",
                path.display(),
                first_byte_diff(&expected, actual)
            )
        }),
        Err(error) => Some(format!(
            "{}: cannot read golden ({error}); regenerate with `UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-f3d golden`",
            path.display()
        )),
    };
    let Some(failure) = failure else { return };
    if update {
        write_golden(path, actual);
    } else {
        failures.push(failure);
    }
}

fn remove_if_exists(path: &Path) {
    if path.exists() {
        std::fs::remove_file(path)
            .unwrap_or_else(|error| panic!("remove {}: {error}", path.display()));
    }
}

/// How a lane's produced bytes are held to their golden.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ByteComparison {
    /// The bytes must match the golden exactly.
    Exact,
    /// The bytes must decode to the same document as the golden does.
    Decoded,
}

fn compare_encode(
    update: bool,
    root: &Path,
    dir: &str,
    name: &str,
    comparison: ByteComparison,
    outcome: Option<Result<Vec<u8>, String>>,
    failures: &mut Vec<String>,
) {
    let bin_path = root.join(dir).join(format!("{name}.bin"));
    let err_path = root.join(dir).join(format!("{name}.err.txt"));
    match outcome {
        Some(Ok(encoded)) => {
            if update {
                remove_if_exists(&err_path);
            }
            match comparison {
                ByteComparison::Exact => compare_bytes(update, &bin_path, &encoded, failures),
                ByteComparison::Decoded => {
                    compare_decoded_bytes(update, &bin_path, &encoded, failures);
                }
            }
        }
        Some(Err(message)) => {
            if update {
                remove_if_exists(&bin_path);
            }
            compare_text(update, &err_path, &format!("{message}\n"), failures);
        }
        None if update => {
            remove_if_exists(&bin_path);
            remove_if_exists(&err_path);
        }
        None => {}
    }
}

#[test]
fn golden_snapshots_are_byte_identical() {
    let update = update_requested();
    let root = golden_root();
    let mut failures = Vec::new();
    for (name, builder_bytes) in fixtures() {
        if fixture_update_requested() {
            let fixture_path = root.join("fixtures").join(format!("{name}.f3d"));
            std::fs::create_dir_all(fixture_path.parent().expect("fixtures dir"))
                .expect("create fixtures dir");
            std::fs::write(&fixture_path, &builder_bytes)
                .unwrap_or_else(|error| panic!("write fixture {name}: {error}"));
        }
        let input = match read_fixture(name) {
            Ok(value) => value,
            Err(message) => {
                failures.push(message);
                continue;
            }
        };

        compare_text(
            update,
            &root.join("decode").join(format!("{name}.json")),
            &decode_snapshot(&input),
            &mut failures,
        );
        compare_text(
            update,
            &root.join("inspect").join(format!("{name}.json")),
            &inspect_snapshot(&input),
            &mut failures,
        );
        compare_encode(
            update,
            &root,
            "replay",
            name,
            ByteComparison::Exact,
            replay_outcome(&input),
            &mut failures,
        );
        compare_encode(
            update,
            &root,
            "generate",
            name,
            ByteComparison::Decoded,
            generate_outcome(&input),
            &mut failures,
        );
        compare_encode(
            update,
            &root,
            "patch",
            name,
            ByteComparison::Exact,
            patch_outcome(&input),
            &mut failures,
        );
    }
    assert!(
        failures.is_empty(),
        "{} golden artifact(s) drifted; if the change is intended regenerate with `UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-f3d golden` and review the diff:\n\n{}",
        failures.len(),
        failures.join("\n\n"),
    );
}

#[test]
fn golden_output_is_deterministic() {
    for (name, bytes) in fixtures() {
        let decode_first = decode_snapshot(&bytes);
        let decode_second = decode_snapshot(&bytes);
        if decode_first != decode_second {
            let (line, first, second) = first_line_diff(&decode_first, &decode_second);
            panic!(
                "fixture `{name}`: nondeterministic decode at line {line}\n    run 1: {first}\n    run 2: {second}"
            );
        }
        let inspect_first = inspect_snapshot(&bytes);
        let inspect_second = inspect_snapshot(&bytes);
        if inspect_first != inspect_second {
            let (line, first, second) = first_line_diff(&inspect_first, &inspect_second);
            panic!(
                "fixture `{name}`: nondeterministic inspect at line {line}\n    run 1: {first}\n    run 2: {second}"
            );
        }
        assert_eq!(
            replay_outcome(&bytes),
            replay_outcome(&bytes),
            "fixture `{name}`: nondeterministic replay"
        );
        assert_eq!(
            generate_outcome(&bytes),
            generate_outcome(&bytes),
            "fixture `{name}`: nondeterministic generate"
        );
        assert_eq!(
            patch_outcome(&bytes),
            patch_outcome(&bytes),
            "fixture `{name}`: nondeterministic patch"
        );
    }
}

/// Every committed fixture replays its retained bytes back to itself.
#[test]
fn fixtures_replay_verbatim() {
    for (name, _) in fixtures() {
        let input = read_fixture(name).expect("committed fixture");
        verbatim_replay_holds(&F3dCodec, name, &input);
    }
}

/// Write path requires a decode baseline.
#[test]
fn fixtures_refuse_to_write_without_a_baseline() {
    for (name, _) in fixtures() {
        let input = read_fixture(name).expect("committed fixture");
        semantic_roundtrip(&F3dCodec, name, &input, |outcome| {
            match outcome {
            SemanticOutcome::Refused { error } => assert!(
                matches!(error, CodecError::NotImplemented(_)),
                "fixture `{name}`: a missing baseline is an unbuilt capability, not a malformed input: {error}"
            ),
            SemanticOutcome::Written { report, .. } => panic!(
                "fixture `{name}`: the baseline was removed, yet the encoder wrote by the {} path",
                report.write_path()
            ),
        }
        });
    }
}

/// How far the mutation lane moves a point, in millimetres.
const MUTATION_MM: f64 = 1.0;

/// Fixtures whose measures the mutation lane compares across the patch
/// writer: every fixture with a point except `design_copy_paste`, whose
/// occurrences place its bodies.
const MEASURED_FIXTURE_FLOOR: usize = 59;

/// Fixtures with no point for the mutation lane to move.
const FIXTURES_WITHOUT_POINTS: [&str; 1] = ["container_metadata_only"];

/// Serializes the neutral model plus unit and tolerance declarations.
fn neutral_document(ir: &CadIr) -> String {
    snapshot_text(&serde_json::json!({
        "model": serde_json::to_value(&ir.model).expect("serialize model"),
        "units": { "length": "millimeter" },
        "tolerances": serde_json::to_value(ir.tolerances).expect("serialize tolerances"),
    }))
}

/// An edit to a decoded document survives the patch writer.
#[test]
fn an_edit_survives_the_patch_writer() {
    let mut edited_count = 0usize;
    let mut measured_count = 0usize;
    let mut skipped: Vec<&str> = Vec::new();
    for (name, _) in fixtures() {
        let input = read_fixture(name).expect("committed fixture");
        let ran = mutation_roundtrip(
            &F3dCodec,
            name,
            &input,
            ExpectedWritePath::Patched,
            |ir| {
                let Some(point) = ir.model.points.first_mut() else {
                    return false;
                };
                let moved = point.position().get();
                point.set_position(
                    cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                        moved.x + MUTATION_MM,
                        moved.y,
                        moved.z,
                    ))
                    .expect("a finite position is a point"),
                );
                true
            },
            |outcome| match outcome {
                MutationOutcome::Written { edited, bytes, .. } => {
                    let round_trip = decode_result(bytes).unwrap_or_else(|error| {
                        panic!("fixture `{name}`: patched output does not decode: {error}")
                    });
                    let moved = edited.model.points[0].position().get().x;
                    let returned = round_trip.ir().model.points[0].position().get().x;
                    assert!(
                        (returned - moved).abs() <= 1.0e-9,
                        "fixture `{name}`: the patch writer produced a container that round-trips, but the \
                         edited coordinate came back as {returned} rather than {moved}; the edit was dropped"
                    );
                    if let Err(mismatch) = snapshots_agree(
                        &neutral_document(edited),
                        &neutral_document(round_trip.ir()),
                    ) {
                        panic!(
                            "fixture `{name}`: the patched container decodes to a different document: {mismatch}"
                        );
                    }
                    if measures_decide(name, edited).expect("the fixture is measured") {
                        measures_agree(name, edited, round_trip.ir())
                            .expect("the round trip keeps its measured geometry");
                        measured_count += 1;
                    }
                }
                MutationOutcome::Refused { error } => {
                    panic!("fixture `{name}`: the patch writer declined to move a point: {error}")
                }
            },
        );
        if ran {
            edited_count += 1;
        } else {
            skipped.push(name);
        }
    }
    assert_eq!(
        skipped, FIXTURES_WITHOUT_POINTS,
        "the set of fixtures with no point to move changed; this lane covers every other fixture, so \
         a new name here narrows it and a missing one means a fixture lost its geometry"
    );
    assert_eq!(
        edited_count,
        fixtures().len() - FIXTURES_WITHOUT_POINTS.len(),
        "every fixture that carries a point must reach the patch writer"
    );
    assert!(
        measured_count >= MEASURED_FIXTURE_FLOOR,
        "fewer fixtures than the floor measure a comparable body through the patch writer"
    );
}

/// Every committed fixture must still match its builder bytes.
#[test]
fn golden_fixtures_match_builders() {
    if fixture_update_requested() {
        return;
    }
    let mut failures: Vec<String> = Vec::new();
    for (name, builder_bytes) in fixtures() {
        match read_fixture(name) {
            Ok(bytes) if bytes == builder_bytes => {}
            Ok(bytes) => failures.push(format!(
                "fixture `{name}`: {}",
                first_byte_diff(&bytes, &builder_bytes)
            )),
            Err(message) => failures.push(message),
        }
    }
    assert!(
        failures.is_empty(),
        "committed fixtures diverged from their builders; either restore the inputs or, if the builder change is intended, rebuild them with `UPDATE_GOLDEN_FIXTURES=1 cargo test -p cadmpeg-codec-f3d golden` and regenerate every artifact in the same commit:\n\n{}",
        failures.join("\n")
    );
}

/// Every key the wire-shape cut deleted is now refused where it used to sit.
///
/// The skin construction is exercised on the document route through the
/// synthetic `skin_surface` fixture; the other three sites are refused
/// by the type that owns them.
#[test]
fn the_deleted_wire_keys_are_refused_at_the_level_they_were_deleted_from() {
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::geometry::VectorOffsetRoles;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(0, false))),
            &DecodeOptions::default(),
        )
        .expect("skin construction fixture");
    let document = serde_json::to_value(decoded.ir()).expect("skin document JSON");
    serde_json::from_value::<CadIr>(document.clone()).expect("the fixture is a document");

    let mut with_inner_count = document;
    let mut touched = 0usize;
    for surface in with_inner_count["model"]["procedural_surfaces"]
        .as_array_mut()
        .expect("procedural surfaces")
    {
        let construction = &mut surface["definition"]["construction"];
        if construction.get("layout").is_some() {
            assert!(construction.get("inner_count").is_none());
            construction["inner_count"] = serde_json::json!(0);
            touched += 1;
        }
    }
    assert!(touched > 0, "the skin golden carries a layout");
    let error = serde_json::from_value::<CadIr>(with_inner_count)
        .expect_err("inner_count lives only on the compact layout that owns it")
        .to_string();
    assert!(error.contains("inner_count"), "{error}");

    let face_maker_object = serde_json::json!({
        "definition": "extrude",
        "direction": {"kind": "explicit", "source": {"kind": "custom"},
                      "vector": {"x": 0.0, "y": 0.0, "z": 1.0}},
        "extent": {"kind": "symmetric",
                   "side": {"termination": {"kind": "blind", "length": 12.0}}},
        "face_maker": {"class": "Part::FaceMakerBullseye", "mode": "bullseye"},
        "inner_wire_taper": "inverted",
        "op": "new_body",
        "profile": {"kind": "native", "value": "fcstd:native:property#X:Base"},
        "solid": true,
        "start": {"kind": "profile_plane"},
    });
    assert!(serde_json::from_value::<FeatureDefinition>(face_maker_object.clone()).is_err());
    let error = serde_json::from_value::<cadmpeg_ir::features::FeatureOperation>(face_maker_object)
        .expect_err("the face maker is its class")
        .to_string();
    assert!(error.contains("invalid type: map"), "{error}");

    let solver_scalar_object = serde_json::json!({
        "kind": "angle_difference",
        "first": {"variable_type": 4, "key": 1},
        "second": 2,
    });
    let error = serde_json::from_value::<cadmpeg_ir::sketches::SketchConstraintDefinitionInput>(
        solver_scalar_object,
    )
    .expect_err("a solver scalar slot is its key")
    .to_string();
    assert!(error.contains("invalid type: map"), "{error}");

    let labelled = serde_json::json!({
        "source": 7,
        "offset": 9,
        "labels": ["source", "offset"],
    });
    let error = serde_json::from_value::<VectorOffsetRoles>(labelled)
        .expect_err("the two roles are the two named keys")
        .to_string();
    assert!(error.contains("labels"), "{error}");
}

/// A construction that owns a revision-gated cache form states its
/// solved-cache fit tolerance there, so the procedural record cannot state it
/// a second time.
#[test]
fn a_revision_cached_surface_refuses_a_record_level_cache_fit_tolerance() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_full_rolling_ball_smbh(
                "rb_blend_spl_sur",
            ))),
            &DecodeOptions::default(),
        )
        .expect("rolling-ball construction fixture");
    let document = serde_json::to_value(decoded.ir()).expect("rolling-ball document JSON");
    serde_json::from_value::<CadIr>(document.clone()).expect("the fixture is a document");

    let mut restated = document;
    let mut touched = 0usize;
    for surface in restated["model"]["procedural_surfaces"]
        .as_array_mut()
        .expect("procedural surfaces")
    {
        let stored = surface["definition"]["cache"]["form"]["cache"]["fit_tolerance"].clone();
        if stored.is_f64() {
            assert!(surface.get("cache_fit_tolerance").is_none());
            surface["cache_fit_tolerance"] = stored;
            touched += 1;
        }
    }
    assert!(touched > 0, "the golden carries a solved revision cache");
    let error = serde_json::from_value::<CadIr>(restated)
        .expect_err("the cache form owns the tolerance")
        .to_string();
    assert!(error.contains("cache_fit_tolerance"), "{error}");
}

/// A geometry-depth decode agrees with the full decode outside the design
/// arenas. Every committed input the codec admits is cut to geometry depth.
#[test]
fn geometry_depth_agrees_with_full_depth() {
    let inputs = fixtures()
        .into_iter()
        .map(|(name, _)| (name, read_fixture(name).expect("committed fixture")))
        .collect::<Vec<_>>();
    let summary = cadmpeg_test_support::depth::assert_geometry_agrees_with_full(&F3dCodec, &inputs);
    assert_eq!(summary.cut, summary.inputs - summary.refused, "{summary:?}");
}
