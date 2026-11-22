Updates: WR-717

#### WR-717 (update, 2026-11-16). `fillet_overflow_crease_along_edge` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246). In the trim at creases (`session/trim_at_creases.rs`), a crease that runs along an edge of the face it crosses, within the trim's tolerance, is always that edge: the sheet is split along it once (`pieces_in_face_to_ends` returns the along-edge runs, marked where a piece was run on over them, #17673), and the body's edge stands as the imprint. A face that holds a rail only to the looser `on_tol` is always looked at for creases. SolidCraft's preset already had the flag on; its rig set in the `issue_2364` tests and `blend_overflow_first.rs` left it off, and those tests pass with it on.

Removed: the field `UpstreamFixes::fillet_overflow_crease_along_edge`, SolidCraft's preset line, its entry in `fixes.json`, the fixture opt-in (`190-round-as-tall-as-its-wall.json`'s `disc_top_rounded_just_past_its_height`), the flag-off branches in `trim_at_creases.rs`, and every test setting of it.

- **Results:** no goldens row moves because of this flag. The two `blend.fillet_executive_p3` rows that move with this change (`big-radius-box_diff_cyl_big_rad-r28-edge12` and `issue_2020_mitred_put_back_d9_full`) reached the removed subtraction and are explained in the WR-695 addendum. The `issue_2364` tests and `blend_overflow_first.rs`, which ran with this flag off, pass with it on, and `issue_2385_crease_along_edge.rs` keeps its closed-form checks.
- **Tests:** `issue_2385_crease_along_edge.rs` keeps `a_round_just_past_its_wall_builds` as it was; `without_the_flag_the_band_is_refused` goes with the flag-off path; `a_crease_inside_the_face_is_unchanged` keeps its closed-form check (V(r) to 1e-6 at r 2, 2.02, 2.5) without the on/off bit comparison. `wr695_overflow_trim_at_creases.rs`'s #17673 variable round was run with this flag off and on; the two tests are one, with the flag's fixed behaviour.
