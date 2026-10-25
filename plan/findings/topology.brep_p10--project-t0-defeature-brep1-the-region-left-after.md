# topology.brep_p10 `project_t0_Defeature_brep1`: the region left after ProjectAndTrim (2026-10-24)

Once `CreateParallelProjectionCurves` reaches the face's cache through the B-rep (it
deferred at "SmFace handle of a surface's face" before), this ProjectAndTrim case
compares. Every face is removed on both sides, and the topology agrees except for the
one region left: C++ keeps the B-rep's former solid region and makes it the infinite
void region. Its `SmTopology` fields are the file's 99999999 flags and user indices. The
port keeps the original infinite region, whose fields are 0
(`ints[6366..6368]`: cpp 99999999, rust 0).

- **Where.** `SmBrep::DeleteFace` with region nesting (`RemoveFaces`) chooses the
  surviving region from the deleted face's upward face use: the mate's region survives
  when it is infinite, or void while this side is not. `topology::brep::p3::delete_face`
  has the same branch, so the difference is in the input. Either the order of the faces
  `ProjectAndTrim` deletes, or the upward face use of a face split by the imprint,
  differs before the deletions. Neither is visible in the final export, since everything
  is gone.
- **Status.** Recorded as a known mismatch. It needs an intermediate export (a
  `TrimType::Split` run of the same input, or the face order of the deletion list) to
  pin down. The four sibling cases stop at `topology.brep_p2`.
