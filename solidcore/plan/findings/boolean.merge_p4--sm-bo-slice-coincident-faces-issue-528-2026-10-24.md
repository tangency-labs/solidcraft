# boolean.merge_p4: `SM_BO_SLICE` coincident faces (issue #528, 2026-10-24)

- **Change.** `ManifoldBoolean` sends `SM_BO_SLICE` through the difference rule for
  coincident face pairs (steps 7.3e and 7.B). Against an oriented sheet, which is void on
  both sides, that rule never deletes a pair, so a face lying in the slicing sheet is kept
  in both parts (WR-254). When the primary face bounds material (solid on exactly one
  side), `coincident_keep` now keeps a slice pair only when that material lies on the side
  the sheet's `SM_OT_SAME` faceuse bounds, the rule step 7.4d uses for radial sectors.
  This deliberately differs from the pinned C++. Primary faces without a material side
  (a sheet primary) or with material on both sides keep the difference rule; other
  operations are unchanged and do no extra lookups.
- **Coverage.** `boolean.merge_p4` and `boolean.merge_p3` are unchanged (0 failed). Of the
  merge_p4 slice cases, the two with coincident faces (`op10-overlap-sheets`,
  `op10-equal-sheets`) still defer before classification, and their primary is a sheet,
  so they keep upstream's rule when they unblock. The reported box/slab slice has no
  difftest case. The unit test `slice_keeps_a_coincident_pair_on_the_sheet_same_side_only`
  pins the rule. A slice of a solid with a face in the sheet will differ from the pinned
  C++ here by design, until the pin takes the fork's fix (ee2a67b).
