# The AssertValid coincident-topology sweep was a no-op, and it was the per-feature check's cost

Upstream bug: [WR-777](../upstream-issues/WR-777--assertvalid-coincidence-check-never-fires-boolean-always-true-and-consumed-inverted.md). Fixed 2026-11-12.

## What was found

`SmBrep::AssertValid` runs `ContainsCoincidentTopology` unconditionally, but two
coupled upstream defects (WR-777) made the result a no-op while the search still
cost time:

1. the accumulator is initialised `FALSE` instead of `TRUE`, so the boolean
   always reports "coincidence found", whatever the search saw;
2. the boolean is consumed inverted (`bRtn &= <coincidence-flag>` instead of
   `bRtn &= !<…>`), and the correct level-2 report is commented out.

Defect 1 masks defect 2: with the boolean always `TRUE`, `rtn &= TRUE` never
fires. The Rust port copied both faithfully (`found` set once to `true`; `rtn &=
…found`).

## The fix and its golden evidence

The fix makes `found` report whether a pair was found, and consumes it as
`rtn &= !found` gated at level 2 with the test-15 report. The two halves cannot
be fixed separately: defect 1 alone turns every clean body into a false failure
(`found == false`, `rtn &= false`). Together they move 50 `topology.assert_valid`
golden cases (coincident bodies now record the test-15 failure, `assert_valid
1→0`) and 36 `topology.brep_validation` cases (clean bodies report `found`
false, `status 1→0`).

## Performance implication for SolidCraft

This is why a SolidCraft design rebuilds slowly. A knurled grip
(`Grip.sc`, a cylinder with two counter-wound helical groove coils patterned 4×
and cut in one Combine, then filleted/chamfered) rebuilt in ~267 s on the `ci`
profile; the profiler put ~236 s (88 %) in `solidcore check`, the per-feature
`AssertValid` run once per feature, and sampling showed that time is the
`contains_coincident_topology` edge/face search — `simple_coincidence_checker`
dropping each edge's midpoint onto every candidate face with a 7×7→5×5→3×3 grid
search plus Newton. Seven features each paid ~33 s of it.

Because the sweep's results were thrown away (WR-777), that ~230 s was pure
wasted work. Two distinct follow-ups, now both landed separately:

- **Correctness** — fix WR-777 so coincident topology actually fails the check
  (this change). It does not speed anything up; it only makes the search's
  result matter.
- **Performance** — the search is O(faces × edges) surface point solves whose
  result was discarded at `AssertValid`'s default report level. It is now
  pruned by certified span boxes (the `simple_coincidence_checker` span-box
  prune), a correctness-preserving speed-up.

The earlier "run the per-feature check at `Geometry`" idea was not the answer:
`Geometry` also runs `contains_coincident_topology` (it is gated only by the
report level, not the search), so it stays slow, and it additionally drops the
real level-2 tests (Face 32/33/38, Edge 13).
