Updates: HANG-44

#### HANG-44 addendum (2026-10-29). The port's sample cap is input-scaled (issue #1476)
- **Severity:** Hang
- **Area:**
- **What changed:** the fixed `MAX_OFFSET_SAMPLES = 16 384` refused a legitimate
  offset: shelling a C++-swept bar (issue #1476, SolidCraft rig case shell/566) offsets
  bend walls whose curves have 125 and 127 cubic Bezier segments, and at
  `approximation_tol_3d = 1e-5` `N_CrvOffsetPtSampling` asks for 37 625 and 38 227
  samples. That is the least upstream samples on those segments: each takes
  `ceil((1/tol)^0.34 * max(2 deg, sqrt(Muu / 8)))` and here `2 deg` dominates
  (`ceil(50.12 * 6) = 301` per segment). The count grows with the segment count and
  the tolerance, not only with a pole.
- **New bound** (`nurbs::crv_offset::offset_sample_bound`, unconditional, decision 78):
  the total count `mu` may be at most

  `min(MAX_OFFSET_SAMPLES_ANY, max(MAX_OFFSET_SAMPLES, OFFSET_SAMPLE_SLACK * segments * ceil((1/tol)^exp * 2 deg)))`

  with `MAX_OFFSET_SAMPLES = 16 384` (the former cap: every input it accepted is
  accepted with an identical result), `OFFSET_SAMPLE_SLACK = 4` and
  `MAX_OFFSET_SAMPLES_ANY = 2^17 = 131 072`. `ceil((1/tol)^exp * 2 deg)` is the
  per-segment count without the curvature term, the floor upstream samples any segment
  at; four times it on average admits an offset second derivative `Muu` up to
  `8 (8 deg)^2` per segment (cubic: 4 608), far above what smooth walls reach, while a
  pole or an offset cusp drives `Muu`, and the count, orders of magnitude past it. The
  ceiling bounds the quadratic knot removal for any input.
- **Timings** (opt-level 3, `--profile difftest`, shared machine):
  - the swept bar's shell (`crates/solidcore/tests/issue_1476_swept_bar_shell.rs`)
    with SolidCraft's opt-in fixes, which offsets both 38 000-sample curves: 6.4 s for
    the whole shell (255 s in a debug build); with the default fixes it takes a path
    that never samples that many (1.2 s);
  - one curve of 127 cubic segments at `1e-5` (38 227 samples): 2.1 s to 3.6 s;
  - one curve of 435 segments (130 935 samples, just under the ceiling): 29.7 s; 436
    segments (131 236): refused in 16 ms;
  - fuzz00366 (727 541 samples, bound 16 384 there): refused before any sampling,
    in 7.7 ms in a debug build (unit test `sample_count_and_degree_are_bounded` asserts under 2 s in a debug
    build); a gentle cubic at `tol = 1e-12` (about 72 000 samples per segment) is
    refused by the ceiling the same way (`tiny_tolerance_is_refused_quickly`).
- **Still open:** upstream has no bound at all; the C++ was not run on the refused
  inputs (the shim keeps its own 4 000-sample guard).
