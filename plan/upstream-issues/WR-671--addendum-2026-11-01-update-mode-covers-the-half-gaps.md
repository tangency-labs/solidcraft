Updates: WR-671

The d = 0.5 gaps that `offset_edges_keep_source_tolerance` leaves (6.0e-5 to 6.8e-5, over half the sheet's vertex tolerance) are what SMLib's `SmBrep::ValidateAndUpdateTolerances(FALSE)` update mode would grow vertices for. That mode is now ported as `topology::brep::validation::validate_and_update_tolerances`. Upstream's update only grows a vertex for a gap over its whole tolerance, so these gaps stay reported. With the opt-in `Kernel::fixes.validate_tolerances_fixes_reported_gaps` (WR-679, the fork's tangency-labs/smlib@8c4bda5) it grows them, and the offset checks clean after the update (`issue_2111_offset_patch_sheet_gaps.rs`, `updated_tolerances_at_a_half_cover_the_gaps`).

Neither upstream nor the fork's SMLib calls the update mode from the offset, so `offset_brep_simple` does not call it. The d = 0.5 pin of the offset itself is unchanged. A caller runs the update after the offset.
