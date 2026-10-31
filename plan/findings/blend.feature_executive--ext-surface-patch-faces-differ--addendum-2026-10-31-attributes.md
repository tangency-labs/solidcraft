# blend.feature_executive: attributes on the patch path follow the merge mismatches (2026-10-31)

Updates: plan/findings/blend.feature_executive--ext-surface-patch-faces-differ.md

The runner now attaches `SmLongAttribute(id, 7, behaviour)` to fixture faces and edges.
For each id it records how many faces and edges carry it in the base B-rep and in each
feature B-rep (cases `attributes_*` of `fixtures/blend.feature_executive/generated.json`).
The port carries attributes on `PartitionFeature`'s and `RestoreFeature`'s `CopyFaces`
(`topology::brep::p2::CopyNotifyState`). The patch copies do not carry them yet.

- `attributes_rebuild_fillets_defillet`, `attributes_restore_defillet`,
  `attributes_inner_loop_extrusion`: pass. Copy and reference attributes reach the feature
  B-rep, temporary ones do not, and base attributes stay.
- `attributes_add_by_faces_extrusion`: fails on the copy attribute of base face 0 (record
  156-160). C++ gives it to 3 base faces and to one face each of feature B-reps 0 and 1. The
  port leaves it on face 0 only. Upstream carries it through the patch path: the inner-loop
  copy of face 0 in `ExtSurfaceRebuild`, then the temporary copy, `PiecewiseMerge` and
  `ApplyPatches`' copies.
- Carrying on every patch copy as well makes this case pass on the base. It then
  over-carries in `attributes_rebuild_fillets_defillet` (2 faces against C++'s 1 for the
  feature face and for base face 0) and in two other cases. Those patches go through the
  `ManifoldBoolean` / `PiecewiseMerge` steps that already differ (addendum
  `--addendum-2026-10-31-boolean-merge.md`), so the patch-path carry waits for that fix.
