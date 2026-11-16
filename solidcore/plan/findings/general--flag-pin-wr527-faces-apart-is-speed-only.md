# General: WR-527 `boolean_skips_faces_apart` changes speed, not results (2026-10-31)

- **Flag:** `Kernel::fixes.boolean_skips_faces_apart` (WR-527, #1526/#1527; fork tangency-labs/smlib@60b1c6a).
- **Audit:** class B. `tests/issue_1526_boolean_2d_faces_apart.rs` reaches the guard and asserts the same faces and exact area with and without the fix, so nothing fails when it is forced off.
- **Why no test can fail:** the fork's fix only skips face pairs whose boxes are apart and inner loops outside a new face's outer-loop box, work whose results upstream discards. The record states the results are right but take quadratic time (a 256-hole sheet takes about 14 times the 64-hole one). The measurable is time, which a deterministic regression test cannot pin, and the all-on SolidCraft cases (34 files) give identical output with the flag forced off.
- **Possible pin:** a count of `FaceIntersect` calls or classified loops exposed for tests (not available without kernel changes, out of scope here), or a time bound in a benchmark rather than a test.
