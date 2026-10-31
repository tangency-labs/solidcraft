//! The SolidCraft preset against the upstream-issue records: every record that cites a
//! commit of the SMLib fork (tangency-labs/smlib) names a field in
//! [`UpstreamFixes::SOLIDCRAFT_FORK`], or is listed with the reason it has none.
//!
//! A new record citing a fork commit fails `fork_records_are_classified` until it is added
//! to [`FORK_RECORDS`] or [`FORK_RECORDS_WITHOUT_FIX`].

use std::collections::BTreeSet;
use std::path::Path;

use solidcore::kernel::UpstreamFixes;

/// Records citing a fork commit whose fix a field ports, with the field(s).
const FORK_RECORDS: &[(&str, &[&str])] = &[
    ("WR-74", &["ssi_tangent_contact_points"]),
    ("WR-75", &["ssi_tangent_curve_tracer"]),
    ("WR-76", &["ssi_trace_tangent_curves_once"]),
    ("WR-79", &["sphere_small_circle_not_touching"]),
    ("WR-80", &["ssi_no_reverse_of_empty_trace"]),
    ("WR-211", &["ellipse_adjust_step_interval_keeps_turns"]),
    ("WR-255", &["uv_trim_corner_end_points_only"]),
    ("WR-260", &["edge_reversal_keeps_failed_edge"]),
    ("WR-265", &["boolean_outer_shell_first"]),
    ("WR-300", &["fillet_retry_radius_as_first"]),
    ("WR-301", &["fillet_rail_rim_on_face"]),
    (
        "WR-345",
        &[
            "offset_walls_touch_refused",
            "local_operation_offset_status",
        ],
    ),
    ("WR-346", &["offset_collapsed_rounds_mated"]),
    ("WR-361", &["keep_operand_uv_trims"]),
    ("WR-385", &["cylinder_rebuild_mean_radius"]),
    ("WR-404", &["curve_sweep_caps_at_sweep_ends"]),
    (
        "WR-407",
        &[
            "sweep_frame_on_planar_path_normal",
            "cover_refuses_folded_continuation",
        ],
    ),
    ("WR-408", &["torus_signed_gen_curve_interval"]),
    ("WR-420", &["fillet_corner_ball_patch"]),
    ("WR-421", &["fillet_chamfer_cone_on_arc"]),
    ("WR-422", &["fillet_torus_center_on_axis"]),
    ("WR-448", &["revolve_analytic_profile_faces"]),
    ("WR-450", &["analytic_copy_keeps_parameterization"]),
    ("WR-452", &["imprint_trims_edge_to_proto_vertex"]),
    ("WR-453", &["imprint_extends_edge_to_proto_vertex"]),
    ("WR-454", &["shell_empty_cavity_fails"]),
    (
        "WR-455",
        &[
            "offset_stitch_at_body_tolerance",
            "curve_contacts_at_curve_ends",
        ],
    ),
    ("WR-457", &["fillet_nx1_tangent_side_edge"]),
    ("WR-458", &["offset_extension_from_base"]),
    ("WR-459", &["fillet_cleanup_keeps_target_faces"]),
    ("WR-463", &["offset_closed_curve_trim_arc"]),
    ("WR-469", &["new_edge_vertex_tolerance_covers_gap"]),
    ("WR-471", &["revolve_arc_centre_near_axis_is_sphere"]),
    ("WR-472", &["precise_properties_body_length_scale"]),
    ("WR-490", &["ellipse_trim_seam_lines_span_cone_height"]),
    ("WR-493", &["found_torus_sphere_keep_tested_nurb"]),
    ("WR-494", &["precise_properties_split_at_surface_knots"]),
    ("WR-495", &["offset_analytic_mate_edges"]),
    ("WR-497", &["shell_inner_shell_of_statuses"]),
    (
        "WR-498",
        &[
            "imprint_trims_edge_to_proto_vertex",
            "imprint_extends_edge_to_proto_vertex",
        ],
    ),
    ("WR-499", &["fillet_nx1_opposite_continuation_side_edge"]),
    ("WR-501", &["torus_major_radius_from_origin"]),
    ("WR-504", &["torus_iso_circle_axis_from_midpoint"]),
    ("WR-510", &["cylinder_plane_tangent_within_tolerance"]),
    ("WR-513", &["fillet_vertex_error_diagnostics"]),
    ("WR-521", &["taper_tangent_corner_planes"]),
    ("WR-524", &["fillet_3x2_mixed_concave_side_edge"]),
    ("WR-527", &["boolean_skips_faces_apart"]),
    ("WR-529", &["revolve_holed_profile_void"]),
    (
        "WR-530",
        &[
            "local_operation_refuses_loose_tangent_edges",
            "local_operation_step_walls",
        ],
    ),
    ("WR-531", &["offset_point_vertex_caps"]),
    ("WR-532", &["offset_extends_cones_as_themselves"]),
    ("WR-533", &["boolean_splits_definition_across_split"]),
    ("WR-534", &["local_operation_box_holds_new_surfaces"]),
    ("WR-535", &["brep_area_between_solid_and_void"]),
    ("WR-536", &["boolean2d_plane_covers_far_faces"]),
    ("WR-537", &["seam_point_solutions_are_one"]),
    ("WR-538", &["torus_check_reads_v_interval"]),
    ("WR-539", &["torus_check_spindle_apex"]),
    ("WR-540", &["edge_check_orders_tangent_faces"]),
    ("WR-541", &["fillet_corner_ball_center_from_axes"]),
    ("WR-542", &["fillet_refuse_overlapping_bands"]),
    ("WR-543", &["uv_trim_joins_drop_pieces"]),
    ("WR-545", &["shell_edges_on_analytics_exact"]),
    ("WR-546", &["fillet_patch_keeps_fillet_edges"]),
    ("WR-547", &["corner_blend_coons_patch_tangent_to_sides"]),
    ("WR-550", &["ssi_pole_point_keeps_its_tangent"]),
    ("WR-552", &["revolve_arc_touching_axis_is_horn_torus"]),
    ("WR-560", &["uv_trim_checks_on_surface"]),
    ("WR-561", &["loop_orientation_floor_from_loop_box"]),
    ("WR-562", &["iso_curve_sliver_domain_is_degenerate"]),
    ("WR-565", &["linear_sweep_arc_is_cylinder"]),
    ("WR-566", &["local_operation_closes_walls_to_nothing"]),
    ("WR-567", &["revolution_round_offset_collapses"]),
    ("WR-568", &["fillet_torus_span_from_start_section"]),
    ("WR-569", &["stitch_solid_nests_cavities"]),
    ("WR-570", &["fillet_knife_edge_rounds"]),
    ("WR-571", &["taper_wall_closes_at_apex"]),
    ("WR-572", &["fillet_chamfer_cone_stored_as_cone"]),
    ("WR-573", &["approximate_surface_bezier_starter"]),
    ("WR-576", &["linear_sweep_arc_to_cylinder_zero"]),
    ("WR-578", &["offset_closes_loop_gaps"]),
    ("WR-579", &["proto_edge_splits_at_captured_vertices"]),
    ("WR-580", &["imprint_reuses_coincident_face_edge"]),
    ("WR-584", &["boolean_new_face_closure_both_sides"]),
    ("WR-585", &["boolean_wire_edge_keeps_faceuse_shell"]),
    ("WR-588", &["face_tight_box_searches_trim"]),
    ("WR-591", &["loops_go_on_round_the_vertex"]),
    ("WR-592", &["fillet_torus_rail_on_axis"]),
    ("WR-593", &["fillet_nx1_tangent_corner_edge"]),
    ("WR-596", &["box_face_failures_propagate"]),
];

/// Records citing a fork commit with no field porting it, and why.
const FORK_RECORDS_WITHOUT_FIX: &[(&str, &str)] = &[
    (
        "WR-259",
        "the fork's fix differs; the port's own `cone_parallel_line_*` are `reported`",
    ),
    (
        "WR-344",
        "cites the fork's build that confirmed it; `offset_extended_edges_mated` is `reported`",
    ),
    (
        "WR-496",
        "the fork has upstream's code; `nurb_cylinder_inside_out_frame` is `reported`",
    ),
    (
        "WR-500",
        "the fork has upstream's code; `torus_keeps_inside_out` is `reported`",
    ),
    (
        "WR-520",
        "a fork feature (draft corner rule), not a `Kernel::fixes` field",
    ),
    (
        "WR-522",
        "a fork option (`bProfileAtPathStart`), not a `Kernel::fixes` field",
    ),
    (
        "WR-523",
        "a fork option (`pOptPlacements`), not a `Kernel::fixes` field",
    ),
    (
        "WR-525",
        "a fork option (`SetBlendCurveAtThirds`), not a `Kernel::fixes` field",
    ),
    (
        "WR-526",
        "a fork option (`SetMarkCornerPatches`), not a `Kernel::fixes` field",
    ),
    (
        "WR-582",
        "a change to the fork's C API (blends take exactly the given edges), not to SMLib",
    ),
    (
        "WR-590",
        "the fork's fix (smlib e4f1377) is not on the fork's main yet; its field `revolve_splits_curves_on_axis` is in `off` until it lands",
    ),
    (
        "WR-594",
        "the fork's fix (smlib 5fdefb9) is not on the fork's main yet; its field `fillet_rails_meet_at_one_end_point` is in `off` until it lands",
    ),
    (
        "WR-598",
        "the fork's fix (smlib 829b982) is not on the fork's main yet; its field `delete_vertex_joins_seam_iso_curve` is in `off` until it lands",
    ),
];

/// Whether `text` cites a fork commit: `tangency-labs/smlib` (or `fork`, `fork commit`)
/// followed by an abbreviated commit hash (seven or more hex digits, at least one a
/// digit), allowing `@`, `` ` ``, spaces and `commit/` between.
fn cites_fork_commit(text: &str) -> bool {
    ["tangency-labs/smlib", "fork commit ", "fork "]
        .iter()
        .any(|prefix| {
            text.match_indices(prefix).any(|(i, p)| {
                let rest = text.get(i + p.len()..).unwrap_or_default();
                let rest = rest.trim_start_matches(['@', '`', ' ']);
                let rest = rest.strip_prefix("commit/").unwrap_or(rest);
                let hex: String = rest.chars().take_while(char::is_ascii_hexdigit).collect();
                let next = rest.get(hex.len()..).and_then(|r| r.chars().next());
                hex.len() >= 7
                    && hex.chars().any(|c| c.is_ascii_digit())
                    && hex.chars().all(|c| !c.is_ascii_uppercase())
                    && !next.is_some_and(char::is_alphanumeric)
            })
        })
}

#[test]
fn fork_records_are_classified() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plan/upstream-issues");
    let mut citing = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        let Some((id, _)) = name.split_once("--") else {
            continue;
        };
        if id.starts_with("WR-") && cites_fork_commit(&std::fs::read_to_string(&path).unwrap()) {
            citing.insert(id.to_string());
        }
    }
    assert!(citing.len() > 50, "{citing:?}");
    for id in &citing {
        let listed = FORK_RECORDS.iter().any(|(r, _)| r == id)
            || FORK_RECORDS_WITHOUT_FIX.iter().any(|(r, _)| r == id);
        assert!(
            listed,
            "plan/upstream-issues/{id}--*.md cites a tangency-labs/smlib commit: add \
             `(\"{id}\", &[\"<field>\"])` to FORK_RECORDS in \
             crates/solidcore/tests/solidcraft_preset_records.rs, naming the \
             `Kernel::fixes` field that ports it (and put that field in `fork: [...]` in \
             crates/solidcore/src/kernel/solidcraft.rs), or, when no field ports it, \
             `(\"{id}\", \"<why>\")` to FORK_RECORDS_WITHOUT_FIX"
        );
    }
}

#[test]
fn fork_record_fields_are_in_the_fork_list() {
    for (id, fields) in FORK_RECORDS {
        for field in *fields {
            assert!(
                UpstreamFixes::SOLIDCRAFT_FORK.contains(field),
                "{id}: `{field}` ports a fork fix: move it to `fork: [...]` in \
                 crates/solidcore/src/kernel/solidcraft.rs"
            );
        }
    }
}
