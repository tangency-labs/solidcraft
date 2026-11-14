# ClassifyLoops' loop bisector keeps its B-rep edits: 37 of 49 listed cases match, 12 topology.brep_p10 cases still differ (2026-10-24)

Follow-up to "ClassifyLoops' loop bisector builds UV trim curves upstream, on a copy in the
port" and "Wiring FindVertexSector brings 9 more cases to the loop-bisector-on-a-copy
mismatch". The bisector's `IsSeam` and `CurveOnClassify` calls now share one
`CurveOnClassifyCache` snapshot, threaded through `classify_loops` and `SetProps_Stage3`. The
healer's in-place `SetProps` (`set_props_with_query_gaps`, now also used by
`CacheFaceProps_Stage3`) runs the seam classifications and `ClassifyLoops` on that snapshot
and copies its UV trims, tolerance growth and gap caches back (`apply_uv_trim_updates`, as
`IsInnerShell` does).

- **Cleared (entries removed):** heal.heal_data_p1 28 and topology.brep_p1 9 now match C++
  bit for bit, including `file_plane_circ`, `file_plane_circ_rebuild_reverse` and
  `plane_circ_heal`.
- **Still differ (entries kept, reason updated):** topology.brep_p10 `healer10_always_Defeature_inset_cube`,
  `read_Boolean_extrudeHoles1`, `read_Fillet_bottle`, `read_Fillet_cyl_cyl_shell`,
  `read_Fillet_fillet0227`, `read_Solids_InsetTaperedExtrude`, `read_Solids_extrusion2`,
  `read_Solids_fillet_ts`, `gen0040`, `gen0115`, `gen0117`, `gen0165`. Their first differences
  are unchanged, except gen0040's (now `ints.ints[1681]`: cpp 1, rust 0).
- **Checked (port instrumented, not committed):** `gen0165` and `read_Fillet_bottle` take the
  in-place path (`set_props_with_query_gaps` Stage2 and Stage3 → `classify_loops_cached`; 1 and
  16 faces), so their bisector edits are kept. Reading the bisector's UV guess
  (`eu1::uv_trim_curve`) on the snapshot instead of the B-rep, where `IsSeam` may just have
  made the trim, changed none of the 12.
- **Not traced.** Within one `SetProps` call the bisector's handles still resolve against
  the caller's B-rep while its edits sit on the snapshot until the copy-back. Upstream sees
  them at once. The remaining copy sites on these paths (`intersect::curve_class` p4/p7,
  `topology::face::p4`) are other candidates. For `read_Fillet_cyl_cyl_shell`, the seam-path
  caveat recorded in the entry above still applies.
