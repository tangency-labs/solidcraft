# blend.fillet_geom_p1: FilletGeom Trim status propagation (#483, PR #674, 2026-10-24)

Historical evidence from the initial unconditional implementation. The current
policy and validation are in
`blend.fillet_geom_p1--fillet-trim-status-483--addendum-2026-10-30-opt-in.md`.

`blend.fillet_geom_p1` copied the pinned C++ `SmFilletGeom::Trim`'s dropped
statuses. `FilletBrep::trim_geom` now returns the actual error from each primary
rail, both other-rail lists, the center line, either interval intersection and
the surface trim. A failed intersection has no usable result: it must not become
`Extent1d::new()` assigned to an edge or a surface domain. Successful edits and
parameter snapping before an error remain, in the original visit order. This
is error propagation, not transaction rollback. The wider boolean/offset/sweep
sites in #483 remain separate work; this PR does not close that broad issue.

A small native reproduction uses a degree-one rail over `[0,0.5]` and asks to
trim it to `[0.25,0.75]`. Its trim returns `SM_ERR` (1001), but the pinned C++
outer operation returns success, sets the edge interval to `[0.25,0.75]` although
the curve still ends at 0.5, and edits subsequent rails. Rust now returns 1001
and stops at that rail. With a valid curve but edge interval `[2,3]`, the same
request makes C++ install its unset intersection `[1e20,-1e20]`; Rust preserves
`[2,3]` and returns the intersection error. Earlier successful curve edits remain.
No CAD application exists in this kernel repository to capture this status-only
API defect; the fixtures contain the complete small pseudo-B-reps and export
both statuses and geometry/interval state.

Six Rust regressions exercise all four rail positions (one parameterized test),
a disjoint edge interval, center-line failure before surface edits, disjoint
surface U domains, a collapsed surface trim and successful snapping/trimming of
all geometry. Five tests fail against the original implementation; all six pass
with error propagation. The successful case snaps -1e-14 to 0 and trims every
rail, center line and surface to `[0,0.75]`.

The native corpus retains all original cases and adds nine small fixtures. Its
minimum generated corpus increases from 300 to 2000 to keep the additional error
paths in the canonical check. The unclassified native run has 2199 cases: 2162
bit-identical passes, 13 existing explicit reference guard rejections and 24
C++ correctness mismatches. All 24 are recorded individually in
the former shared `crates/difftest/known_mismatches.toml`; they are not passes and this unit is
**partial**. No tolerance, reference implementation or refusal guard changes.
The mismatches are:

- `fb_trim_outside`, `fb_trim_point`: out-of-domain/collapsed rail trim failures
  were ignored; C++ changes rail intervals anyway (1001/1007 become success).
- `trim_failed_rail0`, `trim_failed_rail1`, `trim_failed_other1`,
  `trim_failed_other2`: the short rail reproduction above at each visit position.
- `trim_disjoint_rail_interval`: failed intersection installs the unset sentinel.
- `trim_failed_center_line`: C++ continues to trim the surface after the center
  curve trim failed; Rust propagates 1001.
- `trim_disjoint_surface_domain`, `trim_failed_surface_point`: C++ returns success
  after the surface intersection/trim failed; Rust propagates 1001/1007.
- `gen0139`, `gen0185`, `gen0476`, `gen1351`, `gen1477`: requested first-rail trim
  exceeds its natural domain; C++ drops the error and continues editing.
- `gen0438`: curve-less rails are skipped, then the center trim's lower bound is
  outside its natural domain; C++ drops the error.
- `gen0245`, `gen1452`, `gen1626`: first visited rail has an unset edge interval;
  C++ ignores the failed intersection and edits later geometry.
- `gen0829`: the reversed request is normalized to `[0.2866718515818742,
  0.4458908358559929]`; the second rail has an unset edge interval. The reversal
  itself is not an error. C++ ignores the failed intersection.
- `gen1609`: an other rail has an unset edge interval; C++ proceeds into the center.
- `gen1440`, `gen1647`, `gen1709`: a visited rail's edge interval is disjoint from
  the request; C++ installs an unset result and returns success.

Local Rust validation on c44a2e1e3 plus this fix: 2077 kernel tests pass (two
pre-existing ignored tests), default workspace Clippy with -D warnings and
wasm32-unknown-unknown check pass. The full macOS --green suite remains subject
to the already reported C++ reference defects #448/#449/#450; it has not been
re-certified by this change. Canonical CI still needs maintainer approval.

Default and native-feature workspace Clippy with -D warnings pass, as do the
46 default difftest unit tests and one floating-point roundtrip integration test.
The classified native run is PARTIAL: 2199 / 2162 pass / 24 known mismatches /
13 existing guard rejections / 0 unexpected failures. Whole-unit perturbation
reports INSENSITIVE because its 24 known-mismatch outcomes are not counted as
FAIL; all 2162 originally passing comparisons are detected, and the new
successful-snapping fixture independently reports SENSITIVE (1/1). This is not
claimed as a clean whole-unit perturbation gate.

Final rebase onto main 73e557331 preserves all implementation/fixture/mismatch
bytes. Repeated fmt, all 2077 kernel tests, default/native workspace Clippy,
wasm and the canonical native run pass their stated gates on that base: the
native unit remains partial, with 2162 exact passes, 24 explicit reference bugs,
13 existing guard rejections and zero unexpected failures. Successful-snapping
perturbation remains SENSITIVE (1/1). No conflicts required resolution.
