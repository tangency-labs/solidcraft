# General: addendum (2026-10-31): `fillet_run_on_past_far_end_only` (WR-643) not shown by a knife-edge sweep under SolidCraft's preset

Updates: plan/findings/general--1954-unit-only-flags-not-shown-under-the-preset.md

That record left WR-643 as an open follow-up: showing it needs a rail that runs past only the far end of its side edge. A sweep of WR-570's knife-edge plate does not show it either.

- **Input:** the plate, generalised, with its knife edge rounded `r`:
  - tip radius R in {5, 10, 21};
  - back face length L = R + {0.5, 2, 5};
  - r in {0.5, 1, 2, 5};
  - as built and mirrored.
  That is 72 cases, run under SolidCraft's shipped preset plus the flags of `crates/solidcore/tests/wr643_run_on_past_far_end_only.rs`, with the flag off and then on (probe batch 54g6).
- **Result:** off and on are identical in all 72 cases. Every case gives a manifold solid. That includes the 10 cases whose round runs off the back face (√(4Rr) > L), which is the flag's own territory: R 5 with L 5.5 and r 2 or 5, R 5 with L 7 and r 5, and R 10 with L 10.5 or 12 and r 5, each as built and mirrored.
- **Volumes:** in the other 62 cases the removed volume matches WR-570's closed form within 1.6e-10 relative. The probe's starting plate volume had the wrong formula; it is (L R − π R²/4) × 10, and the removed volumes quoted here were corrected for that offline. The correction is a constant per case and does not touch the off/on comparison.

**Status:** no test was added. WR-643's branch stays pinned by its unit test.
