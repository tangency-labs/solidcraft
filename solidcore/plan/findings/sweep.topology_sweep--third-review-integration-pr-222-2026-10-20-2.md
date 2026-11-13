# sweep.topology_sweep — third review integration (PR #222, 2026-10-20)

Merged canonical main `bb137d7d` after review 5589823426. The manifest keeps
main's complete `topology.brep_p7` notes and appends the sweep-specific
`make_face_with_curve_owners` API extension. Both appended findings sections
survive the merge. No code conflict required a behavioral resolution.

The `piecewise_merge` / `union` defaults remain tagged with their p3/p4 owners:
although those units are partial, neither path can acquire a `SweepMergeId`
before `new_merge` reaches the still-unported `SmMerge::SmMerge` in
`boolean.merge_p2`. Canonical engine/storage integration belongs with that
constructor, consistent with `PiecewiseAccess` and the review's permitted split.
This is a documented integration boundary, not native merge-path coverage.

The HCR five-degree angle and wire-union twenty-degree angle now use
`angle * (PI / 180.0)`, exactly the grouping of `SM_DEG2RAD` at
`SmTopologySweep.cpp:844,1424` / `SmMath.h:78`. The other three twenty-degree
expressions keep upstream's literal `20.0*SM_PI/180.0` order
(`SmTopologySweep.cpp:926,1123,1339`). The two corrected constants have unchanged
floating-point bits. No differential-coverage boundary or tolerance is relaxed.

Local validation after this merge: workspace fmt, explicit included-file rustfmt,
strict workspace Clippy with and without C++, workspace tests (1529 kernel
tests), wasm32, and all four native sweep FFI tests pass. The four affected
difftests pass every comparable case with maximum relative error zero:

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
