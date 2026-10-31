//! The SolidCraft preset of [`UpstreamFixes`]: the fixes the SMLib fork SolidCraft runs
//! (tangency-labs/smlib) has, so a kernel can reproduce the fork's results.
//!
//! Upstream: none (a selection of the opt-in fixes, each still described on its
//! [`UpstreamFixes`] field and in its `plan/upstream-issues/` record).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`UpstreamFixes::solidcraft`] | (none: the fork's build of SMLib) |
//! | [`UpstreamFixes::set_by_name`] | (none) |
//! | [`UpstreamFixes::SOLIDCRAFT_FORK`] and the other lists | (none) |
//!
//! Every field of [`UpstreamFixes`] is in exactly one of three lists, which the tests
//! check (a new field fails them until it is listed here):
//! - `fork`: the field ports a fix of the fork (its record or doc names the fork commit,
//!   branch or pull request): on;
//! - `reported`: a fix of the port's own design for a case SolidCraft reported (the
//!   issue's reporter, or a SolidCraft oracle case): on;
//! - `off`: neither: off.
//!
//! Deviations: none (no behaviour; [`Kernel`](crate::kernel::Kernel)'s default stays
//! upstream's).

use super::UpstreamFixes;

/// Declares the three lists, [`UpstreamFixes::solidcraft`] and the name constants from
/// one list of field names, so a name that is not a field does not compile.
macro_rules! solidcraft_preset {
    (
        fork: [$($a:ident),* $(,)?],
        reported: [$($b:ident),* $(,)?],
        off: [$($c:ident),* $(,)?] $(,)?
    ) => {
        impl UpstreamFixes {
            /// The SolidCraft preset: every fix the SMLib fork SolidCraft runs has
            /// ([`Self::SOLIDCRAFT_FORK`]) and every fix of the port's own for a case
            /// SolidCraft reported ([`Self::SOLIDCRAFT_REPORTED`]) on, the rest
            /// ([`Self::NOT_SOLIDCRAFT`]) off. The kernel's default stays
            /// [`UpstreamFixes::default`] (upstream SMLib 1.0.1).
            #[must_use]
            pub fn solidcraft() -> Self {
                let mut f = Self::default();
                $(f.$a = true;)*
                $(f.$b = true;)*
                $(f.$c = false;)*
                f
            }

            /// Sets the field `name` to `on`; `false` when no field has that name (for
            /// runners that name fixes in their data, such as difftest's SolidCraft cases).
            pub fn set_by_name(&mut self, name: &str, on: bool) -> bool {
                match name {
                    $(stringify!($a) => self.$a = on,)*
                    $(stringify!($b) => self.$b = on,)*
                    $(stringify!($c) => self.$c = on,)*
                    _ => return false,
                }
                true
            }

            /// The fields that port a fix of the SolidCraft fork (tangency-labs/smlib).
            pub const SOLIDCRAFT_FORK: &'static [&'static str] = &[$(stringify!($a)),*];

            /// The fields of the port's own design for cases SolidCraft reported.
            pub const SOLIDCRAFT_REPORTED: &'static [&'static str] = &[$(stringify!($b)),*];

            /// The fields the SolidCraft preset leaves off.
            pub const NOT_SOLIDCRAFT: &'static [&'static str] = &[$(stringify!($c)),*];
        }
    };
}

solidcraft_preset! {
    fork: [
        ellipse_adjust_step_interval_keeps_turns, // WR-211; smlib 5c0b5ed
        fillet_torus_rail_on_axis, // WR-592; smlib 994f129
        fillet_retry_radius_as_first, // WR-300; smlib 1d06159
        fillet_rail_rim_on_face, // WR-301; smlib f6ed05a
        fillet_refuse_overlapping_bands, // WR-542; smlib c0d74ee
        fillet_vertex_error_diagnostics, // WR-513; smlib a73cf5e
        fillet_corner_ball_patch, // WR-420; smlib 04bd9a5
        fillet_corner_ball_center_from_axes, // WR-541; smlib bc6ddf9
        fillet_torus_span_from_start_section, // WR-568; smlib 985faae
        fillet_chamfer_cone_on_arc, // WR-421; smlib 8a7deae
        fillet_chamfer_cone_stored_as_cone, // WR-572; smlib b5743ec
        corner_blend_coons_patch_tangent_to_sides, // WR-547; smlib cae8618
        fillet_torus_center_on_axis, // WR-422; smlib 110b131
        fillet_patch_keeps_fillet_edges, // WR-546; smlib 1072d1b
        fillet_knife_edge_rounds, // WR-570; smlib b7ec55b
        offset_point_vertex_caps, // WR-531; smlib d5dadaa, fbe43bb
        offset_extends_cones_as_themselves, // WR-532; smlib 3626d4b, b283ea2
        thicken_planar_side_walls, // WR-564; smlib #62
        keep_operand_uv_trims, // WR-361; smlib ad24a8e
        edge_check_orders_tangent_faces, // WR-540; smlib 5533bbb
        uv_trim_checks_on_surface, // WR-560; smlib b4f8c57
        loop_orientation_floor_from_loop_box, // WR-561; smlib 54ca3e0
        torus_check_reads_v_interval, // WR-538; smlib 0cd9d08
        torus_check_spindle_apex, // WR-539; smlib daba678
        cylinder_rebuild_mean_radius, // WR-385; smlib f06b48a
        revolve_analytic_profile_faces, // WR-448; smlib 0b143f7, 9853e54
        revolve_arc_centre_near_axis_is_sphere, // WR-471; smlib 0b143f7
        revolve_arc_touching_axis_is_horn_torus, // WR-552; smlib 0b143f7, c4e378d
        taper_tangent_corner_planes, // WR-521; smlib e8f23e0
        taper_wall_closes_at_apex, // WR-571; smlib 183ea65, 54148db
        revolve_holed_profile_void, // WR-529; smlib 38b4212
        curve_sweep_caps_at_sweep_ends, // WR-404; smlib a83108d
        local_operation_box_holds_new_surfaces, // WR-534; smlib a1ee273
        offset_walls_touch_refused, // WR-345; smlib a759622
        local_operation_offset_status, // WR-345; smlib a759622
        local_operation_refuses_loose_tangent_edges, // WR-530; smlib 23c5b0a
        local_operation_step_walls, // WR-530; smlib 5d428ae
        local_operation_closes_walls_to_nothing, // WR-566; smlib ccbf77a
        offset_collapsed_rounds_mated, // WR-346, #397; smlib a252570
        approximate_surface_bezier_starter, // WR-573; smlib eae96cb
        revolution_round_offset_collapses, // WR-567; smlib 8b2090f
        uv_trim_corner_end_points_only, // WR-255; smlib 5af7070
        uv_trim_joins_drop_pieces, // WR-543; smlib e36feb1
        boolean_outer_shell_first, // WR-265; smlib branch abastran/void-shell-check
        shell_inner_shell_of_statuses, // WR-497; smlib 4038552
        edge_reversal_keeps_failed_edge, // WR-260; smlib 47507b6
        iso_curve_sliver_domain_is_degenerate, // WR-562; smlib 8088f5e
        sweep_frame_on_planar_path_normal, // WR-407; smlib 1b6f5fa
        linear_sweep_arc_to_cylinder_zero, // WR-576; smlib fd59111
        cover_refuses_folded_continuation, // WR-407; smlib 1b6f5fa
        boolean2d_plane_covers_far_faces, // WR-536; smlib 67b8a1e
        boolean_splits_definition_across_split, // WR-533; smlib f11d0ac
        proto_edge_splits_at_captured_vertices, // WR-579; smlib 9eec159
        classify_edge_interval, // WR-305; smlib ClassifyEdgeInterval
        seam_point_solutions_are_one, // WR-537; smlib 577f7be
        ellipse_trim_seam_lines_span_cone_height, // WR-490; smlib 5af7070
        new_edge_vertex_tolerance_covers_gap, // WR-469; smlib 5af7070
        transform_scales_each_tolerance, // WR-507; smlib #49
        transform_model_size_from_body_box, // WR-508; smlib #50
        torus_signed_gen_curve_interval, // WR-408; smlib 40344bf, d9f20b0
        torus_major_radius_from_origin, // WR-501; smlib 6136a04
        torus_iso_circle_axis_from_midpoint, // WR-504; smlib 672a437, #40
        found_torus_sphere_keep_tested_nurb, // WR-493; smlib 7ea6a96
        internal_pole_row_through_knot, // WR-505; smlib #41
        extrusion_origin_scaled_zero, // WR-511; smlib #31
        imprint_trims_edge_to_proto_vertex, // WR-452, WR-498; smlib b43876f, ffaf4e8
        imprint_extends_edge_to_proto_vertex, // WR-453, WR-498; smlib a8caccd, b43876f, ffaf4e8
        imprint_reuses_coincident_face_edge, // WR-580; smlib 81cd61a
        fillet_point_step_per_point, // ND-3, #1173; smlib 0a05860
        analytic_copy_keeps_parameterization, // WR-450; smlib 276b509
        precise_properties_body_length_scale, // WR-472; smlib 073d474
        precise_properties_split_at_surface_knots, // WR-494; smlib 08d9a14
        brep_area_between_solid_and_void, // WR-535; smlib d2d9586
        offset_extension_from_base, // WR-458; smlib 2d7c7ed
        fillet_cleanup_keeps_target_faces, // WR-459; smlib 9ddf4a8
        fillet_nx1_tangent_side_edge, // WR-457; smlib d5ebcb3
        fillet_nx1_opposite_continuation_side_edge, // WR-499; smlib 420e47d
        fillet_3x2_mixed_concave_side_edge, // WR-524; smlib 1255993
        offset_closed_curve_trim_arc, // WR-463; smlib fa7ff4a
        offset_analytic_mate_edges, // WR-495; smlib fddc366
        shell_edges_on_analytics_exact, // WR-545; smlib cd85d34
        offset_stitch_at_body_tolerance, // WR-455; smlib 601d804
        curve_contacts_at_curve_ends, // WR-455; smlib 601d804
        shell_empty_cavity_fails, // WR-454; smlib fbe43bb
        fillet_refuse_ball_not_fitting, // WR-462; smlib 2c5baae..e5696ca
        sphere_small_circle_not_touching, // WR-79; smlib 30d25bf
        ssi_tangent_contact_points, // WR-74; smlib c26edbc
        ssi_trace_tangent_curves_once, // WR-76; smlib 0ce09f2, 1499681
        ssi_no_reverse_of_empty_trace, // WR-80; smlib 0dbe681
        ssi_singular_start_inside_direction, // WR-512; smlib #52
        ssi_pole_point_keeps_its_tangent, // WR-550; smlib 8d95600
        ssi_tangent_curve_tracer, // WR-75; smlib ef4ab25
        curve_tight_box_interval_knots, // WR-474; smlib "a curve's tight box holds the knots inside ..."
        boolean_skips_faces_apart, // WR-527; smlib 60b1c6a
        boolean_new_face_closure_both_sides, // WR-584; smlib 69c5198
        stitch_solid_nests_cavities, // WR-569; smlib 351c57c
        box_face_failures_propagate, // WR-596; smlib 0df62c8, 35008b1, 449c81c
        offset_closes_loop_gaps, // WR-578; smlib 7ff0a94, 34318f4
        linear_sweep_arc_is_cylinder, // WR-565; smlib 12918e9
        face_tight_box_searches_trim, // WR-588; smlib 9d44c1b
        cylinder_plane_tangent_within_tolerance, // WR-510; smlib bd20f6e (smlib#55)
        fillet_nx1_tangent_corner_edge, // WR-593; smlib 1ff7353, d5ebcb3
        loops_go_on_round_the_vertex, // WR-591; smlib 9209e76
        boolean_wire_edge_keeps_faceuse_shell, // WR-585; smlib be8ac73
    ],
    reported: [
        fillet_refuse_rails_leaving_faces, // WR-302, #1024
        fillet_refuse_face_consuming_blends, // WR-320, #879
        fillet_refuse_non_manifold_result, // WR-212, #422
        fillet_refuse_crossing_rails, // WR-460, #1073
        fillet_blend_curve_revolved_on_arc, // WR-581, #919
        offset_excess_faces_after_merge, // WR-341, #977
        offset_clip_right_side_out, // WR-342, #974
        revolution_offset_axis_both_sides, // WR-343, #973
        offset_extended_edges_mated, // WR-344, #469
        remove_slit_loops_where_solids_touch, // WR-360, #746
        geometric_g2_continuity, // WR-380, #933
        proto_vertex_miss_is_false, // WR-73, #848
        check_loop_vertex_tolerance, // WR-381, #476
        check_loop_projected_seam_ends, // WR-440, #530
        trim_boundary_seam_wrap, // WR-491, #1437
        bspline_check_min_radius, // WR-382, #475
        oscillating_skips_failed_guesses, // WR-253, #481
        refine_cc_range_ends, // WR-383, #527
        flat_corners_test_bits, // WR-384, #525
        nurb_cylinder_inside_out_frame, // WR-496, #1463
        planar_path_uncapped_open_profiles, // WR-400, #837
        taper_pivot_nearest_corner, // WR-402, #479
        revolve_nested_shells_as_cavities, // WR-403, #478
        join_touching_loops, // WR-405, #892
        uv_trim_reversal_on_surface, // WR-406, #891
        offset_resolve_crossing_loops, // WR-345, #854
        offset_missing_surface_fails, // WR-464, #1072
        fillet_nx1_end_rib_global_merge, // WR-303, #841
        bspline_trim_preserves_uv_spans, // WR-441, #1114
        bspline_trim_keeps_rational_end_spans, // WR-575, #1529
        cone_parallel_line_midpoint, // WR-259, #711
        cone_parallel_line_domains, // WR-259, #711
        proto_edge_self_loop_on_vertex_degenerate, // WR-214, #480
        slice_coincident_faces_sheet_side, // WR-254, #528
        topology_failed_evaluations_propagate, // WR-262, WR-263, WR-264, #521
        heal_stage3_failures_propagate, // WR-266, WR-268, #521
        boolean_vertex_by_best_crossing, // WR-410, #1059
        boolean_cleanup_removes_slits, // WR-304, #1120
        degenerate_proto_edge_in_one_vertex, // WR-306, #1058
        torus_keeps_inside_out, // WR-500, #1490
        sheet_sweep_joins_profile_wires, // WR-409, #1113
        point_solve_keeps_triangle_guess, // WR-444, #1164
        fillet_setback_from_start_vertex, // WR-445, #1169
        fillet_corner_sphere_from_four_arcs, // WR-502, SolidCraft oracle
        fillet_point_bigger_step_retry, // WR-456, #1181
        fillet_through_extended_rail_end, // WR-468, #1022
        precise_properties_trim_base_v, // WR-563, #1556
        corner_blend_stays_inside_neighbours, // WR-461, #1170
        fillet_trace_rejects_short_step, // WR-503, SolidCraft oracle
    ],
    off: [
        fillet_rails_meet_at_one_end_point, // WR-594: smlib 5fdefb9 is not on the fork's main yet
        delete_vertex_joins_seam_iso_curve, // WR-598: smlib 829b982 is not on the fork's main yet
        revolve_splits_curves_on_axis, // WR-590: smlib e4f1377 is not on the fork's main yet
        fillet_trim_status, // WR-604: own design (#674), not a SolidCraft report; the fork has upstream's
        boolean_lists_virtual_surface_copies, // WR-196: a source audit's, not SolidCraft's; the fork has upstream's
    ],
}

#[cfg(test)]
mod tests {
    use super::UpstreamFixes;

    /// Each field of `f` with its value, from the derived `Debug`.
    fn fields(f: UpstreamFixes) -> Vec<(String, bool)> {
        let text = format!("{f:?}");
        let inner = text
            .split_once('{')
            .and_then(|(_, rest)| rest.rsplit_once('}'))
            .map(|(inner, _)| inner)
            .unwrap();
        inner
            .split(',')
            .filter_map(|kv| kv.split_once(':'))
            .map(|(k, v)| (k.trim().to_string(), v.trim() == "true"))
            .collect()
    }

    #[test]
    fn every_field_is_in_one_list() {
        let lists = [
            UpstreamFixes::SOLIDCRAFT_FORK,
            UpstreamFixes::SOLIDCRAFT_REPORTED,
            UpstreamFixes::NOT_SOLIDCRAFT,
        ];
        let all = fields(UpstreamFixes::default());
        assert!(all.len() > 100, "{all:?}");
        for (name, _) in &all {
            let n = lists.iter().filter(|l| l.contains(&name.as_str())).count();
            assert_eq!(
                n, 1,
                "`UpstreamFixes::{name}` is in {n} of the SolidCraft preset's lists: add it, \
                 once, to one list in crates/solidcore/src/kernel/solidcraft.rs \
                 (`solidcraft_preset!`): `fork: [...]` if it ports a tangency-labs/smlib \
                 commit, `reported: [...]` if it is the port's own fix for a case SolidCraft \
                 reported, else `off: [...]`; one line `{name}, // WR-<n>; smlib <sha>`. A \
                 record of yours that cites a fork commit also goes into FORK_RECORDS in \
                 crates/solidcore/tests/solidcraft_preset_records.rs"
            );
        }
        let listed: usize = lists.iter().map(|l| l.len()).sum();
        assert_eq!(listed, all.len());
    }

    #[test]
    fn set_by_name_reaches_every_field() {
        for (name, _) in fields(UpstreamFixes::default()) {
            let mut f = UpstreamFixes::default();
            assert!(f.set_by_name(&name, true), "`{name}`");
            assert_eq!(
                fields(f).iter().filter(|(_, on)| *on).count(),
                1,
                "`{name}`"
            );
        }
        assert!(!UpstreamFixes::default().set_by_name("no_such_fix", true));
    }

    #[test]
    fn preset_turns_on_fork_and_reported() {
        for (name, on) in fields(UpstreamFixes::solidcraft()) {
            assert_eq!(
                on,
                !UpstreamFixes::NOT_SOLIDCRAFT.contains(&name.as_str()),
                "`{name}`"
            );
        }
    }
}
