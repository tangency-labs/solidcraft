# Periodic boundary reconstruction for an offset seam adjoining a closed rim

Addendum to `offset.offset_executive_p2--stress-corpus-half-torus-topology-change-18803.md`.
Refs #18803. This is the first local boundary construction for that issue,
covering S1983 and a retained-seam control. The complete sheet arrangement and
winding-number cavity construction remain outside its scope.

## Construction and obligations

A same-face seam is an identification of two sides of a parameter chart. It
supplies no additional material support. An SSI branch's active interval is
bounded by genuine displaced supports: in S1983 these are the opening plane
and the pocket cap. A chart crossing beyond the cap cannot retain a seam in
that interval.

The geometry creator keeps the raw paired SSI curves until its candidate is
selected. It traverses a same-face seam to the genuine boundary at its far
endpoint, solves the displaced boundary support against the SSI curve, and
checks the chart crossing on the interval between that root and the opposite
endpoint's genuine support. A branch belongs to the source edge only if that
opposite root belongs to an already mated genuine boundary edge. Event branches
are selected by that incidence, never by the old edge's nearest midpoint.
Successful mating is recorded separately from traversal marks. Multiple solver
reports are accepted only at the identical parameter of the same SSI curve.
Spatial proximity does not merge root reports, including those the native solver
keeps across chart seams or closed-curve ends. Roots supplied by multiple support
incidences must also agree at the identical SSI parameter. Distinct roots
without established ownership are refused; no nearest old vertex substitutes
for a missing intersection. A
component that does not meet the rim support retains its chart constraint.

For the local graph implemented here, both incidences of the obsolete seam
adjoin one closed rim. Its sole vertex is the seam's far endpoint, which is
also the cut of the rim edge's parameter interval. Remove both seam incidences
and retain the rim interval between the two new roots. This excludes the arc
through the old cut. The prescribed orientations and endpoint agreement are
checked in 3D and UV before replacing any face. Other seam graphs require
further work. Multiple events on one face and events without an already mated
opposite boundary remain outside this first construction. The closed-shell,
grooved and threaded stress recipes are not established by this case.

The two new SSI endpoints are separate vertices. Faces are rebuilt from paired
3D/UV curves, with the exact source edge recorded for every input curve slot.
The face builder returns each slot's edge owner, so correspondence uses those
owners directly rather than a nearest-midpoint provenance guess. Source
attributes, tolerances and face/edge-use maps pass through the notified
replacement, then the superseded faces are deleted. The existing inverted-edge
face marker follows the replacement, preserving the later loop resolver's
scheduling; rebuilding the boundary does not discard that obligation.

The builder leaves a coincident slot empty when it reuses an existing edge.
For a seam's second incidence, the same source edge copied over exactly the
same parameter interval identifies the identical 3D curve. Its unique
completed owner supplies that incidence's edge identity. A missing slot with
no such identity proof is refused. This does not search for a nearby curve.
An unmapped faceuse side obtains its radial image through the mapped mates;
the relation's orientation is carried explicitly. Face images are obtained from
the unique union of both source faceuse incidences: Shell stores the opening
face relation on the side selected by its region and inset, which can be the
mate of the upward faceuse.

Where one surviving seam use lacks its UV curve, translate the other use's
curve by the surface's parameter period. The periodicity query identifies the
candidate axis and translation; it is not the acceptance proof. Certify the
translated curve's lift against the retained 3D edge over its full stored
interval, using `exact_pieces::certified_lift` at the inherited edge bound.
Failure to certify is a refusal, never a drop or tolerance-changing retry.

The opening face can have two closed rim curves joined at the original shared
vertex. Once displaced, that vertex need not lie on both curves. Separate the
closed cycles, use certified signed UV area to order the outer cycle and holes,
and let the existing curve arrangement cut their actual crossings before gap
closure. Both 3D and UV curves retain their stored edge intervals. Crossing
stretches use one common trim interval for both curves. Face construction is
asked to attach the supplied UV curves along with the supplied 3D curves.

No tolerance is introduced or increased by this construction. Root parameters
must agree exactly. Membership residuals and endpoint checks use the inherited
edge/body bounds. No mesh decides
material, no analytic-kind/recipe branch selects the construction, and no
failed construction triggers a different construction.

## Review corrections

Observations are grouped by source face and seam edge. Only the identical
source edge use, vertex, support incidence and root point are deduplicated;
the two uses of the seam may report that same observation. A different
support or root remains in the group, so the planner's exactly-two-ends and
one-closed-rim obligations see every constraint, independently of report order.

The disappearing-seam construction applies only with genuine supports on
both sides of its active interval. If either side lacks those supports,
even an SSI component missing the source chart line retains ordinary
endpoint trimming. This applicability decision precedes the chart query;
it is not a retry after a failed reconstruction. A mixture of an ordinary
constraint and a replacement event at one endpoint remains unresolved.
The native regression splits a cylinder rim between its existing two
supports, then checks the same exact half-rim trim with both the natural
UV curve and its translation by two whole periods.

Multiple SSI branches claiming the same event now return the typed
`OffsetFailure::PeriodicBranchAmbiguous`. The edge-use mating policy
propagates it without marking the edge as an ordinary unmated edge.
Ordinary trim misses keep their existing unmated-edge handling.

## Independent values

The S1983 shell volume remains the integral derived in the original finding:
`3607.14296142514 mm³`. Its expected value is unchanged.

A second test preserves the seam, using the same half-torus and a drill at
`a = 37`, radius `0.95`, depth `4`, with inward wall `t = 0.01`. The displaced
outer rim is `30 + (8 - t) = 37.99`; the displaced drill's outer meridian is
`37 + (0.95 + t) = 37.96`. The seam still meets the SSI before the cap.

The retained short seam has `y = 0`, with endpoints at `x = 37.99` on
the opening and `x = 37.96` on the drill. Its latter height satisfies
`z² = 7.99² - 7.96² = 0.4785`, between zero and the displaced cap at
`z = 4.01`. Both ends therefore lie strictly beyond the drill centre
`x = 37`. The other meridian reaches the cap at
`x = 30 + sqrt(7.99² - 4.01²) = 30 + sqrt(47.76) < 37`.
The control's topology assertion selects the unique nonclosed cavity edge
with both ends beyond `x = 37`, then requires exactly two incidences on
that same cavity face. No numerical nearness test selects the seam.

Using the original finding's polar overlap integral H, its wall volume is

```text
pi² * 30 * (8² - 7.99²) - H(8, 0.95, 4) + H(7.99, 0.96, 4.01).
```

Split each radial integral at `30 + sqrt(q²-h²)`, substitute
`rho = midpoint - halfwidth*cos(u)` on each interval, and integrate over
`0 <= u <= pi`. Composite Simpson counts 4096, 8192, 16384 and 32768 give:

| count | original overlap | displaced overlap | wall volume |
|---|---|---|---|
| 4096 | 9.822987588758211 | 9.976516275138310 | 47.49802099840375 |
| 8192 | 9.822987588758233 | 9.976516275138234 | 47.49802099840366 |
| 16384 | 9.822987588758298 | 9.976516275138286 | 47.49802099840364 |
| 32768 | 9.822987588758245 | 9.976516275138318 | 47.49802099840372 |

The installed CLI 0.44.1 also builds this retained-seam case and measures
`47.49802099838924 mm³`. This is a cross-check, not this checkout's validation
or the source of the oracle.

These values follow the set defined by the analytic supports, independently of
the shell result. The convergence check is not an interval certificate.

## Research and scope

The repository's offset, SSI, boolean, definition and tolerance references were
read. The local reconstruction follows their rule that material boundaries
come from support intersections, while a periodic chart cut is an identified
boundary. Certified signed UV area is the existing kernel primitive used to
order the separated cycles. The general winding-number cell selection remains
outside this local graph construction. External retrieval of Chen and McMains'
2005 paper was attempted, but DNS resolution of mcmains.me.berkeley.edu failed;
no claim is made to have read that paper in this session.

## Validation

All Cargo commands run from the worktree root through `buildq run solidcraft . --`.
After the baseline reproduction, the commands also use
`RUST_BACKTRACE=1` (local execution) and Cargo's two split environment overrides
`--config 'env.TMPDIR.value="/home/ray/builds/lane-18803/tmp"'`
`--config 'env.TMPDIR.force=true'`.

- Baseline: `cargo test --manifest-path solidcore/Cargo.toml -p solidcore-ops
  --lib a_half_torus_with_a_blind_hole_shells_open_at_its_bottom -- --ignored
  --nocapture` failed as expected with `StageFailed`: 0 passed, 1 failed.
- Initial implementation: `cargo test --manifest-path solidcore/Cargo.toml
  -p solidcore-ops --lib a_half_torus_ -- --include-ignored --nocapture`
  compiled successfully, then finished with 1 passed and 1 failed.
  `a_half_torus_keeps_a_periodic_seam_when_its_offsets_still_meet` passed its
  full validity and volume assertions. S1983 reached reconstruction, then
  refused a missing periodic-boundary topology relation. Static API review
  identified discarded coincident owner slots and missing mate-side UV
  correspondence. The corrected construction carries those identities explicitly
  and asks the builder to attach the supplied paired curves.
- `cargo fmt --manifest-path solidcore/Cargo.toml --all` and root
  `cargo fmt --all` passed after the initial implementation. One earlier
  invocation using an inline TOML environment table failed before formatting;
  the split environment keys above corrected it.
- `git diff --check` passed. A source scan found no added explicit panic-prone
  calls in non-test code.

- The corrected rerun compiled in 2m 36s and again finished with 1 passed and
  1 failed (46.97s test time). The retained-seam control passed; S1983 reached
  reconstruction and hit a generic unsupported-graph guard. Static review found
  that the opening planner queried only the upward source faceuse although
  Shell can store its relation on the mate. Both planners now require one unique
  image across both incidences. Refusals now identify their failed invariant.

- The next focused rerun (job `1795012216.537936-09bc84c1`) passed:
  `a_half_torus_with_a_blind_hole_shells_open_at_its_bottom` and
  `a_half_torus_keeps_a_periodic_seam_when_its_offsets_still_meet`, 2 passed,
  0 failed, 296 filtered out; 46.41s test time. Both full validity and the
  independent volume assertions passed. Compilation passed in 2m 36s.
- The regression's ignore marker and temporary SMB capture hooks were then
  removed. Root `cargo fmt --all` and
  `cargo fmt --manifest-path solidcore/Cargo.toml --all` both passed again.
- Offset-area tests, job `1795019980.149930-1326b484`:
  `cargo test --manifest-path solidcore/Cargo.toml -p solidcore
  -p solidcore-ops --lib offset:: -- --test-threads=2`.
  Compilation passed in 2m 06s. The kernel portion passed: 299 passed,
  0 failed, 3 existing ignored tests, 4982 filtered out (209.99s).
  The public operation portion passed: 44 passed, 0 failed, 1 existing
  ignored test, 253 filtered out (729.72s). Both half-torus regressions ran
  normally without an ignore marker and passed. The job exited 0 in 1068s.
- The first executed clippy run, job `1795019979.106005-0631a5b7`:
  `cargo clippy --manifest-path solidcore/Cargo.toml -p solidcore
  -p solidcore-ops --all-targets -- -D warnings`, failed (101) on
  `clippy::single_match_else` in the new owner lookup. Its two-arm `match`
  was changed to `if let`, without changing the construction. Solidcore fmt
  passed again. The same clippy command was requeued as
  `1795029324.377456-92115969`, but remained waiting for over four hours.
  It was canceled without execution (130) under the subsequent user rule
  to end after compilation and the regression pass, and not wait over
  30 minutes for wider checks. The corrected clippy result remains for CI;
  no clean clippy result is claimed.
  An earlier waiting clippy request was canceled without execution (130)
  to prioritize the corrected regression; it was not a compiler result.
- Final source scan and `git diff --check` pass. No golden or kernel-case
  expectation has changed.

The successful test captured `before.smb` and `after.smb` under
`/home/ray/builds/lane-18803/tmp/evidence/`. Both were rendered by the same
local `solidcore-diag` executable, from view `0,90` at `1000x800`:

- `native-bottom/before.png` and `before.json`: 11 edges, 3090 triangles,
  0 faces refused.
- `native-bottom/after.png` and `after.json`: 20 edges, 5754 triangles,
  0 faces refused. The open cavity and the rim beside the blind hole are visible.
- Both renders exited 0. Body tolerance is `1e-5` and the largest reported edge
  tolerance is `2e-5` before and after.

The renderer was copied from the available local build
`lane-18475-7fee1f05/debug/solidcore-diag` into the evidence directory before
rendering both files. The geometry in `after.smb` comes from this checkout's
passing shell test. This renderer reads and draws that result; it does not
perform the construction. The earlier renderer path was removed by build-target
cleanup (invocation exited 127); the replacement rendered both bodies successfully.
A queued `cargo run -p diagnostics` request, job
`1795040512.427445-70ec1217`, was canceled without execution (130) once the
local renderer became available. No additional build was needed. The earlier
CLI pictures of an independent constructive reference are not presented as
this fix's output.

## Review validation

The combined exact-test run, buildq job `1795057753.582548-e45a1c89`, compiled
successfully and exited 0. It selected only these six tests across `solidcore`
and `solidcore-ops`:

- `periodic_events_keep_distinct_supports_and_roots`: passed.
- `periodic_chart_miss_without_a_third_support_keeps_ordinary_trim`: passed.
- `periodic_branch_ambiguity_is_not_an_unmated_edge`: passed.
- `every_offset_failure_keeps_its_class`: passed.
- `a_half_torus_with_a_blind_hole_shells_open_at_its_bottom`: passed.
- `a_half_torus_keeps_a_periodic_seam_when_its_offsets_still_meet`: passed,
  including its new same-face incidence assertion.

The kernel reported 4 passed, 0 failed, 0 ignored; operations reported 2
passed, 0 failed, 0 ignored. Compilation took 2m35s; the operation tests took
53.69s. No expected volume or golden changed. The run forced the lane's
TMPDIR through Cargo's environment configuration and used RUST_BACKTRACE=1
to execute locally. Root and solidcore formatting passed through buildq;
`git diff --check` passed.

The initial kernel compile exited 101 on test-only traversal names and a
missing point type name, corrected before the passing run. Intermediate
queued requests were canceled without execution (130) while consolidating
the exact tests; formatting retries initially hit the two-job queue guard
(2), then passed (0). Current clippy was canceled before execution (130)
under the owner's focused-regression stop rule. No current clippy pass is
claimed. The proposed scoped PR description is prepared, but GitHub's API
was unreachable and its wrapper's guard logs were read-only here, so the
actual PR description and any closing keyword could not be verified or
updated.

The remaining stress recipes and general offset-cell arrangement are outside
this first self-contained construction. Keep issue #18803 open to track the
other reported recipes; this PR must reference it with `Refs #18803`, without
a closing keyword. Validation of two half-torus cases does not establish a fix
for all 17 reported items. The full suite, wasm, goldens and repository-wide
gates remain for CI after the owner pushes.
