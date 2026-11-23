Updates: WR-727

#### WR-727 (update, 2026-11-12). `topological_vertex_has_only_edge_uses` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch B55, which retires the two flags
`is_topological_vertex_rejects_cusps_and_tangent_errors` and `topological_vertex_has_only_edge_uses`). `is_topological_vertex` (`topology/vertex.rs`) always returns false for a vertex with a use that is not an edge use. The kernel rig's `fixes.json` never listed this flag, so the rig now runs with the fix too.

- **Tests:** `tests/boolean_api.rs` `point_contact_unions_check_clean` runs every point contact, on a seam edge or not, on `Kernel::default()`, and adds the box less its inscribed sphere (`seam-box-minus-sphere`, the goldens case `boolean.api_brep/with_options_difference`, which failed the Level2 check with a vertex gap of 5 before); `point_contacts_off_seam_edges_check_clean_by_default`, which ran the controls without the fix, is folded into it.
- **Results:** the goldens rows the fix moves, with their old and new values:
  `plan/findings/topology.vertex--b55-two-flags-retired-2026-11-12.md`.
