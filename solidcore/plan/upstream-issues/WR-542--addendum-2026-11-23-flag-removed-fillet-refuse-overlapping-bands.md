Updates: WR-542

#### WR-542 (update, 2026-11-23). `fillet_refuse_overlapping_bands` is removed; the bands' check always runs

`FilletExecutive::refuse_overlapping_bands` (`blend/fillet_executive/p3.rs`) always asks the session's
`geometry_bands_clear_other_rails` (the fork's `BandsClearOtherRails`, with the WR-649 band between a rail's end
vertices), right after `refuse_face_consuming_blends`. Two blends whose bands overlap on a face they share, away from
any corner, are refused as `NoRoom` (`NoRoomReason::BandsOverlap`) instead of building a body that is not closed.
Removed: the field and its doc file, the executive's `rolled_beside_band` latch and the rollover's
`Apart::BesideBand` (#19242: with the check always on, a round that rolls onto a wall beside another round's band is
simply apart, and the bands' check compares the two), the flag's entries in the `issue_18166`, `issue_1945`,
`issue_2020`, `issue_2260`, `wr650` and `fillet_round_failure_notes_status` flag lists and in the `fixes` lists of
the SolidCraft cases 199, 871 and 1080, and the flag-off halves of the `issue_2050`, `wr460`, `wr649` and
`solidcore-ops` rollover (#19242) tests.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes:
  a fillet whose bands overlap is refused.
- **Goldens:** no row moves (283 units pass unchanged). The `blend.fillet_executive_p3` record that reached the hook
  only with the flag on reaches it with every flag off.
- **SolidCraft cases:** the same verdicts as on main (340 pass, the same two 9522 failures). One refusal changes its
  reason: 199's `round_past_the_end_face_is_refused` (r 3.001, no fixes) was refused with status 1001 from
  `find_fillet_brep_edge` and is now refused by the bands' check, naming the radius that builds, 3.
- **Tests:** two pins of the default kernel move, each from a refusal of an open body after it was built to a
  refusal before the body is edited. Both use #1073's 14 x 6 x 10 post with its four vertical edges rounded r 3.5.
  On each 6 mm end face, each round's band runs from its edge to its rail, r in from it, so past r = 3 (half the
  face) the two bands overlap and the round does not fit (the rails cross; main built the open body that leaves).
  - `issue_1073_the_default_kernels_overlapping_bands_are_refused` (was `..._open_body_is_refused`): a defect
    (status 1001) becomes `BandsOverlap` naming 3 for a constant radius. The radius law has no one size that fits,
    so it keeps status 1001, and `issue_1073_the_default_kernels_radius_law_notes_the_overlap` checks that the
    session noted `BandsOverlap`.
  - `offset_faces::tests::a_put_back_whose_bands_overlap_is_refused` (was
    `a_put_back_that_leaves_the_solid_open_is_refused`): `NotPutBack` becomes `NoRoom`.

  In each case the post comes back closed, with its six faces. No test input now reaches `reblend`'s check for a
  stitched result that is not closed (WR-212). The check stays.
