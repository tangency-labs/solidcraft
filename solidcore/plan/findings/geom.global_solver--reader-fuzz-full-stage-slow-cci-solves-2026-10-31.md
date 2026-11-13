# geom.global_solver: reader-fuzz `full` stage, slow curve/curve solves on a spiked trim circle (2026-10-31)

Unit: `geom.global_solver` (the `CciGlobalSolver` instantiation that
`intersect.curve_class_p9` drives). Base `origin/main` df1b3b79f. Rust only: the
reader fuzzer has no C++ side, so C++ timing is not measured here.

**Sweep.** `examples/reader_fuzz` over the 186 `.smb` seeds in
`upstream/TestFiles/pt_TestFiles`, 400 cases each, stages `read`, `topo` and `full`
(223 200 cases; `--jobs 2 --mem-mb 2048`, default 20 s case timeout, Rust 1.97.0):

| stage | ok | err | unported | bad |
|---|---|---|---|---|
| read | 25 564 | 48 836 | 0 | 0 |
| topo | 21 742 | 52 658 | 0 | 0 |
| full | 21 343 | 53 044 | 1 | 12 `timeout` |

No `panic`, `oom`, `stack-overflow` or `crash` verdict in any stage.

**The 12 timeouts are slow, not hung.** Every mutation sets one float to an extreme or
out-of-scale value (`1e300`, `±1e308`, `4.9e-324`, `3.0e5`, a weight `1 -> -1`). Six
were replayed in-process (`reader_fuzz run --stage full`); all six finished `full-ok`:

| seed, case | mutation | time |
|---|---|---|
| Defeature/defillet.smb #142 | float `1.0 -> 1e300` | 22 s |
| Defeature/flathead_screw_top.smb #354 | float `0.7071… -> 1e300`, bitflip | 31 s |
| Defeature/inset_cube_3x1fillet.smb #20 | float `2.0 -> 4.9e-324`, `0.0 -> -0.0` | 36 s |
| Defeature/LShape_II.smb #193 | float `0.25 -> 2` | 40 s |
| Defeature/defillet.smb #244 | weight `1.0 -> -1.0`, byte | 41 s |
| Defeature/flathead_screw_top.smb #270 | float `-6e-16 -> 3.0e5` | 190 s |

Not replayed: `Fillet/box_extrusion2.smb` #69 and #118, `Fillet/extrusion0206.smb` #59,
`Fillet/fillet0114.smb` #157, `Fillet/sphere_diff_3cylinders.smb` #303,
`surf_extrude.smb` #349 (all `±1e300`/`±1e308` mutations).

**flathead_screw_top.smb #270 (190 s; the unmutated file loads in 5 ms).** The mutation
moves one control point of a 9-point rational full circle of radius 3 (byte 46763, the
seventh point, `x: -6e-16 -> 3.0e5`), so the trim curve becomes a 300 000-long spike
on a cone face. Time profile (gdb stack samples, 37 of 37 in
`solve_branch`, innermost frames `curve_solve_newton` -> `Curve::evaluate_point` ->
`CrvOnSurf` -> cone `convert_uv_from_nurbs_to_step` -> `Ellipse::convert_to_polar_parameter`).
A temporary counter on `GlobalSolve::solve_it` (not committed) gave:

- 56 solves over 20 000 branches or 200 ms, 9 000 802 branches and 184 s in total.
- 16 solves took 7.6-17.8 s each (178 s together), each visiting 262 463-263 807
  branches (about 2^18: a near-complete walk of the curve trees) at 29-67 µs per branch;
  the others visited up to 263 807 branches at about 0.1 µs per branch.
- Every solve stays far under the caps: the most branches in one solve is 3.1 % of
  `MAX_SOLVE_BRANCHES` (2^23), and no solve returned an error.

So the bounds work as documented; the cost is how many branches stay alive (the spiked
curve's boxes keep most branches from being culled) times a Newton local solve per
branch through the cone's polar-parameter inversion.

**Not changed.** These are bounded, accepted inputs, like the open library-speed
timeouts in `general--fuzz-sweep-library-seeds-5-24-40-2026-10-31.md` and the
`approx = 1e300` cases in `boolean.topology_intersector_p1--fuzz-30000-timeouts-2026-10-21.md`.
Open questions for whoever picks this up: whether the C++ reference spends the same
time on #270 (needs a C++ `.smb` reader driver, which is not in the repo; see
`crates/difftest/README.md`, Fuzzing the `.smb` readers), and whether culling in
`branch_may_contain_answers` or the per-branch local solve could be cheaper at parity.

**Reproduce.**

```sh
X="nice -n 10 cargo run --profile difftest -p difftest --example reader_fuzz --"
$X sweep --seeds-dir upstream/TestFiles/pt_TestFiles --cases 400 --jobs 2 \
    --mem-mb 2048 --out reader-fuzz-out --tag full --stage full
$X repro --seed upstream/TestFiles/pt_TestFiles/Defeature/flathead_screw_top.smb \
    --case 270 --write fst270.smb
$X run --input fst270.smb --stage full     # about 190 s, full-ok
```
