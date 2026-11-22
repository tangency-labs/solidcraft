# blend.fillet_geom_p1: opt-in trim status propagation (2026-10-30)

Updates: plan/findings/blend.fillet_geom_p1--fillet-trim-status-483.md

PR #674 follows the default-off `UpstreamFixes` policy. The same failing inputs
now preserve the pinned C++ result by default. `Kernel::fixes.fillet_trim_status`
enables the first-error behavior described in the original finding (WR-604).
Both modes preserve visitation order and successful prefix edits; neither rolls
back. The broader boolean, sweep and offset paths in #483 remain separate work.

The nine native fixtures and 2000-case generated floor remain. Of the 24 old exemptions, 23 are removed because the default statuses now
compare against C++. `gen1440` retains a separate successful curve-trim geometry
mismatch in `crates/difftest/known_mismatches/blend.fillet_geom_p1.toml`. No tolerance, FFI guard, C++ shim or
reference behavior changes. The retired shared records are absent; the historical
finding is preserved as its own file and this addendum states the current policy.

Ten Rust regression tests cover default continued edits and enabled first-error
behavior, including each primary/other rail, interval intersections, center and
surface failures, and successful knot snapping. They use main's current curve
and surface-owner APIs.

Validation on main `559672505599ae077f81fe80dd953e335637557b` plus this follow-up:

- Rust 1.98.0: fmt, plain workspace/all-target Clippy with warnings denied, wasm,
  record-ID and retired-archive checks pass. Kernel unit tests: 2610 pass,
  3 existing ignores, zero failures (including the ten trim regressions).
- Full native-feature Clippy and difftest cannot compile: main's `geom_tol`
  imports the deleted `query_face_p1::{everything, inexact}`; `heal_gap` constructs
  `NotifyEnv::records` with a `BTreeMap` instead of `NotifyRecords`. These three
  runner files are byte-identical to main. They are not changed in this PR.
- A diagnostic native build omits only the unrelated `geom_tol` and `heal_gap`
  modules/constructors from the local generated runner registry. No tracked build
  script, runner, FFI guard, C++ shim or case is altered. This is not a clean
  full-workspace native build or a `--green` gate claim.
- With pinned v1.0.1 (`3fd5b95`) built by Apple Clang, the affected unit compares
  all 2199 cases: 2185 bit-identical value passes, 13 existing guard rejections,
  and one known geometry mismatch (`gen1440`), no deferrals. Unit remains partial.

`gen1440` uses trim `[0.4916704865259837,0.619432454122106]`. Its first rail
(edge 2) is a rational degree-2 curve with an interior knot at
`0.6125828265124486`. Its curve trim succeeds, but C++ keeps four control points
(16 homogeneous entries) and seven knots; Rust keeps three control points
(12 entries) and six knots. The second rail's edge interval is disjoint and its
status is dropped by default as requested. Removing that second rail's curve and
both other-rail lists in an uncommitted diagnostic fixture leaves the same first
rail geometry mismatch, even though the outer operation and interval intersection
succeed. Thus the remaining mismatch precedes the failed-status path; it is not
an opt-in policy difference. Its origin in curve trimming is still unresolved;
no C++ correctness claim or `cpp_defect` parking marker is added. The diagnostic
fixture is removed; the complete generated input remains in the canonical run.

The successful-snapping fixture perturbation is SENSITIVE 1/1. The generated
registry is restored after the diagnostic run; this is not a whole-unit
perturbation claim.

The historical finding's claims of 24 known status differences apply to the prior
unconditional implementation only. Current PR validation and blockers are explicit
in the PR description.
