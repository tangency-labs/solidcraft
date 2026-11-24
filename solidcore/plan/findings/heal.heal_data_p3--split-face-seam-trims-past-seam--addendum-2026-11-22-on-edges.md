# heal.heal_data_p3: the seam cut's trims lie on their edges (2026-11-22)

Updates: plan/findings/heal.heal_data_p3--split-face-seam-trims-past-seam.md

**On main.** On main (4c275ffb7d), in all 26 `split_face_seam_cone_*` rows, the healer's
own body keeps trims on edge uses 1 and 11. The rounded-interval certificate of
`crates/goldens/src/trims_on_edges.rs` (`certify_edgeuse`, target an eighth of the edge
tolerance) holds for each of them. The edge tolerance is 1e-5, unraised, and the certified
bound is at most 1.25e-6. Every case the finding lists is among these, the `q1` and `wide`
rows included.

**The fixing change.** `heal_data_p3_drop_rows_are_certified` was run with
`off_on_main` emptied over main's first-parent history from #17896's merge (a739b6ef7f) to
4c275ffb7d. The `q1` rows fail at every commit up to and including f94d244c56. The
certificate itself fails there, on edge use 1 of `r1_1_h1_s90_q1`, with "a face's weights
are not positive over a box". The rows certify from e6c78eb059 (#19561) on.

#19561 changed the point-drop interpolation that fits a UV trim to an edge projected onto
a closed surface (`topology::edgeuse::p3::brute_force`). It now lifts consecutive samples
across the seam before fitting, and a loop wider than the range is placed in the base
period by its midpoint. Before #19561, the wrapped samples on either side of the cut gave
the trim a whole-period excursion. That matches the finding: trims running to
`u` = 1.086..1.096, and distances near the cone's diameter (1.85 for radius 1).

**The `r0.02` and `q0` numbers.** The same run, with `off_on_main` emptied, certifies
every `q0` row, the `r0.02` ones included. It does so at the commit that added the finding
(3bf00493b5, on #17896's branch), along that branch to its head, and at a739b6ef7f. So the
certified 0.046 and 0.097 for `r0.02`, and the sampled `q0` distances, are not reproduced
by the committed test at any of those commits.

On those commits the test checked that the trims were present only after `remade` had
given every edge use a trim. A certified `q0` trim there may therefore have been one made
again from its edge rather than the healer's own. Which one it was, at those commits, was
not established. The finding's numbers came from a probe that was not committed and that
evaluated past the domain. They stay unreconciled.

**Change.** `operand_trim_rows::trim_blocks::off_on_main` is removed.
`heal_data_p3_drop_rows_are_certified` checks that the healer's body keeps trims on edge
uses 1 and 11 of every seam row, before any trim is made again. Removing either trim on a
scratch copy fails that check. It then certifies those trims on their edges like every
other trim.
