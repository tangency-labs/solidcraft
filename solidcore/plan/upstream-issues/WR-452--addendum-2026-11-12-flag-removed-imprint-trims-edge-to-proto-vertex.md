Updates: WR-452

#### WR-452 (update, 2026-11-12). `imprint_trims_edge_to_proto_vertex` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch B21a). `imprint_edge_definition`
(`topology/proto_topology/p2.rs`) always fits each end of a definition's interval to its proto vertex before the edge is
made (`fit_edge_def_ends_to_proto_vertices`, `p2/imprint_ends.rs`), with the fork's correction ffaf4e8 (WR-498): an end
the opt-in extension (WR-453, `imprint_extends_edge_to_proto_vertex`) does not continue is moved to the curve's point
nearest its proto vertex, when that point lies inside the interval, within the definition's tolerance of the vertex and
nearer than the end (the proto edge open, the trimmed interval longer than `EFF_ZERO`). SolidCraft's preset and the kernel rig's `fixes.json` already had the flag on, so their results
don't change.

- **Tests:** `tests/issue_410_435_thread_groove.rs`, `tests/issue_410_screw_thread_vertex_gaps.rs` and
  `tests/issue_435_screw_thread_groove.rs` expect the fitted ends with the extension off and on; the tests that pinned
  upstream's gaps and test #34 reports with the trim off are deleted. `tests/issue_1480_pleated_loft_union_edge_ends.rs`
  keeps both its cases, the one without the extension now with the trim.
- **Results and why WR-453 stays opt-in:**
  `plan/findings/general--b21a-imprint-trim-retired-extension-kept-2026-11-12.md`.
