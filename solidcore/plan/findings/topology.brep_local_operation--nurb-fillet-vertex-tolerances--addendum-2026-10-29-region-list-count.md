# topology.brep_local_operation: nurb_fillet tolerances, cause and fix (addendum, 2026-10-29)

Updates: plan/findings/topology.brep_local_operation--nurb-fillet-vertex-tolerances.md

**Cause.** `sm_FindBrepRegionOfOtherFace` (`SmMerge.cpp:186`) returns the B-rep's only region
when `GetPrimaryBrep()->GetSize() == 1`, the length of its region list. The port tested
`brep.regions().len() == 1`, the number of live region records. `delete_face` takes a region it
merges away off the list but keeps its record, so after `RemoveFaces` emptied the B-rep, the copy
target had one listed region and two records. For every face the merge copy made, the port
skipped the early return and classified the face against a radial sector. That asked for a
binormal on an already-copied edge use, built a UV trim curve (`GetOrCreateUVTrimCurve`), and
`SetUVTrimCurve` raised the edge tolerance to the curve's distance to the surface, which cascaded
into its vertices. C++ never classifies there: inside its `CopyFaces` it makes no
`GetOrCreateUVTrimCurve` call.

**Trace** (gdb, C++ v1.0.1 `-O2 -g` against the Rust debug build of main 709138774; probe
scripts not committed): the effective tolerance writes of the copy agree for the first 81, then
Rust re-writes edges 75, 74, 72, 71 and 64 (up to 5.759574129151012e-4) through
`set_uv_trim_curve` under `find_brep_region_of_other_face`; at every call the region arena has 2
live records and the region list 1.

**Fix.** `find_brep_region_of_other_face` counts the region list (`topology::brep::p1::regions`)
and returns its head, as upstream. The 8 `nurb_fillet_face*_same` known mismatches are removed.
