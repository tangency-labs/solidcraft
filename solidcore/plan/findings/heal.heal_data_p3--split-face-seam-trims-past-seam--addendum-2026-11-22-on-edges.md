# heal.heal_data_p3: the seam cut's trims lie on their edges (2026-11-22)

Updates: plan/findings/heal.heal_data_p3--split-face-seam-trims-past-seam.md

**Not a kernel defect.** The trims `split_face_seam` leaves on edge uses 1 and 11 lie on
their edges in every case the finding lists. The rounded-interval certificate of
`crates/goldens/src/trims_on_edges.rs` (`certify_edgeuse`, target an eighth of the edge
tolerance) holds for each of them on main (4c275ffb7d), as it does for every other trim
of those bodies.

**What the trims are.** In `split_face_seam_cone_r1_1_h1_s90_q0`, after the split, edge
use 1's trim is a cubic B-spline (51 control points) running in `u` from 0.875 to 1.0 at
`v = 0`. Edge use 11's is the same at `v = 1`. Both stay inside the cone's `[0, 1]` domain.
Nine evenly spaced points of each trim evaluate onto their edge's points to four
decimals. Nothing in them runs past the seam (`u > 1`), so the finding's suspected cause
does not apply.

**When.** At the merge of the PR that added the finding (a739b6ef7f), the certificate
already holds for the `q0` cases, `r1_1_h1_s90_q0` included (the finding lists it 1.851
off). The first `q1` case failed there inside the certificate itself ("a face's weights
are not positive over a box"), not on a measured distance. A later change to the certificate
or the kernel (not bisected) removed that failure, and on main the `q1` and `wide` cases
certify too. The finding's sampled distances (0.037 to 1.85) cannot
be reproduced from these trims.

**Change.** `operand_trim_rows::trim_blocks::off_on_main` is removed:
`heal_data_p3_drop_rows_are_certified` now certifies edge uses 1 and 11 of all 26 seam
cases on their edges, and asserts that both trims are present.
