# Wiring FindVertexSector brings 9 more cases to the loop-bisector-on-a-copy mismatch (2026-10-24)

`SmFace::FindVertexSector` is now wired on the mutable curve-classification path
(`intersect::curve_class::p7::Mutable::find_vertex_sector`, unit `topology.face_p4`). Nine
heal-path cases that used to defer there now run on into `SetProps_Stage3` →
`ClassifyLoops` → `sms_FindAndClassifyLoopBisector`, and they disagree in the same way as
the cases in "ClassifyLoops' loop bisector builds UV trim curves upstream, on a copy in
the port" above. They are listed in `crates/difftest/known_mismatches.toml` under that
entry's reason:

| unit | cases | first mismatch |
|---|---|---|
| `heal.heal_data_p1` | `file_plane_circ`, `file_plane_circ_rebuild_reverse` | `values.out[215]`: cpp `-1.23456e-61` (cache reset), rust `0` |
| `topology.brep_p1` | `plane_circ_heal` | `values.out[134]`: cpp 1, rust 0 |
| `topology.brep_p10` | `read_Fillet_bottle`, `read_Fillet_cyl_cyl_shell`, `read_Fillet_fillet0227`, `read_Solids_extrusion2` | `ints.dbl_bits`, max relative difference 7e-7 |
| `topology.brep_p10` | `gen0040` | `values.dbls.len`: cpp 1921, rust 1785 |
| `topology.brep_p10` | `gen0165` | `ints.ints[170]`: cpp 1, rust 0 |

- **Checked (port instrumented, not committed).**
  - In all nine cases `find_and_classify_loop_bisector_into` runs.
  - In eight of them the wired `FindVertexSector` is reached from the bisector's
    `CurveOnClassify` (backtraces).
  - The two `plane_circ` cases show the same signature as the `plane_square` entries.
- **`read_Fillet_cyl_cyl_shell`, `read_Fillet_bottle`, `read_Fillet_fillet0227`.** Here
  `HasSeamProblem`'s seam classification also reaches `FindVertexSector`. It returns
  `SM_ERR_INVALID_INPUT` from `EvaluateNormalAtSingularity` (the normal sum at a
  singularity does not unitize). `CurveOnClassify`'s status is ignored there, as upstream
  does. I have not confirmed that the C++ errors at the same point, so for
  `read_Fillet_cyl_cyl_shell`, where the bisector reaches no `FindVertexSector`, the seam
  path is a second possible source.
- **Fix.** The same as the entry above: thread `&mut Brep` through `ClassifyLoops` to the
  bisector. When that lands, re-run these nine cases and remove their known-mismatch
  entries.
