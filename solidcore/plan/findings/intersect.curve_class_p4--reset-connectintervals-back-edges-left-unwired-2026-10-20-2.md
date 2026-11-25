# intersect.curve_class_p4 — `ReSet` / `ConnectIntervals` back-edges left unwired (2026-10-20, small-fixes PR)

Both `intersect.curve_class_p2` functions are ported (`CurveClassification::reset`,
`CurveClassification::connect_intervals`), but wiring the p4 stubs to them is not
mechanical:

- **`ReSet`** (the full constructor `with_curve`): wired, `topology.brep_p2`'s fixture
  `merge_plane10_line` (`fixtures/topology.brep_p2/ops.json#0`, MergeCurves on a 10x10
  plane with a line at y = 5.5) stops being deferred and fails with the harness error
  `intersect::point_class::ClassGeometry: invalid input: classification object handle
  does not resolve`: the brep_p2 MergeCurves path builds a classification over handles
  its `ClassGeometry` adapter cannot resolve. Fix that adapter before wiring.
- **`ConnectIntervals`** (`sm_SqueezeOutSmallestInterval`): wired together with `ReSet`,
  4 generated `intersect.curve_class_p4` cases disagree on the record length
  (gen0164 C++ 827 / Rust 741, gen1926 1289 / 1203, gen2544 7063 / 7059, gen2695
  4099 / 4095; default seeds). Not diagnosed: either `connect_intervals` with
  `allow_removal_of_ends = false` or the runner's record after the squeeze op.

`SmCurveInterval::GetMinUV` / `GetMaxUV` are wired (no case reaches them yet).
```text
PARTIAL sweep.topology_sweep cases=568 passed=554 failed=0 deferred=14 rejected=0
PARTIAL topology.brep_p5 cases=797 passed=737 failed=0 deferred=60
PARTIAL topology.brep_p7 cases=316 passed=197 failed=0 deferred=119
PARTIAL blend.hcr cases=434 passed=394 failed=0 deferred=40
```
Sweep deferrals remain `SmBrep::MakeShellVertexTopology` (`topology.brep_p2`).
Construction/HCR/merge/stitch continuations remain uncertified native coverage;
this integration does not promote the unit to green. Full-gate and exact-head
CI results are recorded in the PR discussion.
