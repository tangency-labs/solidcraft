# Local vertex uncertainty and imported cuts (2026-11-13)

The kernel's tolerant-model validity checks must use the local uncertainty already
stored on a vertex. A finite local radius larger than 1,000 body tolerances does
not, by itself, show that the boundary is wrong. Geometric gaps still need bounds.
The contract is described in [Validity and tolerances](../../docs/validity-and-tolerances.md).

## Tolerance writers and the diagnostic

The STEP reader measures endpoint gaps, propagates edge tolerances to vertices,
and covers vertex-loop surface gaps (`solidcore-step/src/topology.rs`). Trusted
flat import converts the record tolerance to a kernel radius and skips healing.
The exported reader record for the reported socket has four relevant vertex
tolerances around 0.008220648 mm, converted to radii around 0.004110324 mm.

Two later writers can produce the reported radii 0.0056613767 and 0.0055817271 mm:

- A vertex/face projection can take the global fallback in
  `topology/vertexuse.rs::compute_uv_point`. Its face-cache lookup reaches
  `query/face/p2.rs::BrepCacheFace::create_uv_trim_curves`, which requests tolerance
  adjustment. `topology/face/p3.rs::create_uv_trim_curves_body` then raises a vertex
  radius to twice its curve-end gap. Merely measuring a cold face can therefore
  change its stored uncertainty.
- Boolean result processing in SolidCraft's `crates/smlib/src/core/boolean.rs`
  calls `validate_and_update_tolerances`, which also grows vertex tolerances to
  twice their edge or face gaps.

The measured endpoint gaps are 0.0028306883266788 and 0.0027908635563428 mm;
doubling them explains the reported values. The first invocation on the reported
body has not been isolated. The application's failure message prints the result's
report. Its first-operand annotation independently checks whether that operand
fails validation; it does not compare the two failure lists.

Trim-cache tolerance growth is an established mechanism, also documented in
[the cache-history finding](general--trimmed-surface-cache-history-tolerance-growth-deferral.md).
Pinned C++ was not executed for this investigation. This finding records the
Rust validity contract and evidence, without claiming a new native defect
confirmation. Related upstream records are WR-381 (shared junction uncertainty),
WR-93 (open-loop reporting), and WR-679 (the tolerance updater's half-radius test).

## Local geometric bounds

The vertex ratio limit is replaced by a finite-value check. The existing lower
bound remains. Measured face gaps use the larger of the vertex radius and the
body's existing intersection tolerance. Strict corner queries used by placement
and repair retain their existing semantics. A placement solver's displacement
to an ideal surface intersection is not a vertex/face gap: the solver can slide
along an intersection curve, and ideal surfaces omit the faces' tolerance bands.
Making that displacement a new mandatory bound regressed fillet case 1488 and
the R 1/2 and 1/2-14 NPT tapered external thread ladder cases. Vertex Test 1
continues to report actual face gaps, as before this branch.

Diagnostic replays of those three cases measured face gaps of 0.000000413910,
0.000000891323 and 0.000000682559 mm, respectively. The displaced ideal corners
were 0.000036418552, 0.000037565744 and 0.000024489688 mm away. The near-parallel
supports amplify small face deviations into larger ideal-intersection motion;
the stored points themselves remain inside the incident face bands. No vertex,
edge or face tolerance is enlarged by the correction.

After the correction, the CLI's normal each-feature rig replay passes
`1488-fillet-slab-end-after-nudged-loft-join`, `022-external-r-1-2-taper` and
`023-external-1-2-npt`. The ladder golden is unchanged. The existing validation
tests also cover a ball reaching a non-incident box face or a separate wire edge,
and loop-use tests cover both directions and individual endpoint-zone failures.
These are newly added tests, not newly executed tests: this review lane's buildq
policy refuses targeted Rust tests, clippy and golden comparison/regeneration.
The unit-test results recorded below describe the earlier branch revision;
the review additions still need CI validation.

Removing the ratio does not allow a vertex ball to swallow unrelated topology.
The pointer validator drops vertices onto non-incident bounded edges and trimmed
faces, refusing contact within or at the vertex's radius. Solver precision stays
independent of that radius; bounding boxes only prune the distance queries.

For a loop junction, the previous curve end and next curve start must name one
topological vertex. Each endpoint must lie within that vertex's radius plus its
own edge zone. The triangle inequality then bounds their mutual gap by
`2 * vertex_radius + previous_edge_zone + next_edge_zone`. Distinct vertices and
endpoints outside those individual bounds do not acquire this allowance. The
check adds no stored tolerance and moves no point.

The existing tests in `tests/issue_1419_brep_check.rs` reproduce the four ratio
failures with a 10 mm cube and body tolerance 0.000005 mm, then exercise a real
0.002 mm corner gap and a radius-1.5 mm through cut. Its analytic volume is
`1000 - pi * 1.5^2 * 10 = 929.314165294230 mm^3`, with allowed error 0.000001 mm^3.
The final measured volume is 929.314165296871 mm³ (absolute error about
0.000000002642 mm³). All six whole-body tests and all thirteen loop-use tests
pass, as does all-target clippy with warnings denied.
The out-of-bound fixture prepares query caches and restores its recorded radius
before checking, so lazy query healing cannot repair the deliberately invalid
input. `topology::loopuse::tests` covers both loop directions, distinct vertices,
and individual endpoint bounds.

## Application measurements

The public socket import is one closed body with 46 faces and volume
1785.049600532154 mm³. An XY circle centered at `[5, 5]`, radius 1.5 mm, cut through
symmetrically, now evaluates with normal validation: one closed body, 48 faces,
126 edges, 82 vertices, volume 1756.560081917680 mm³. It removes
28.489518614474 mm³ and reports no cut error or warning.

The old executable with validation disabled produces exactly the same volume and
topology. The correction therefore admits that existing geometric result.
YZ through also succeeds, measuring 1770.807836546366 mm³. XZ through is
effectively unchanged. The symmetric 10 mm evaluation measures
1785.055655493262 mm³, a 0.006054961109 mm³ increase; the old unchecked executable
produces exactly the same value and topology. That existing measurement drift is
not resolved by this validity change.

The exact-box mutation with a finite 0.1 mm vertex radius now has no validity
failure. The old golden `box_mutation1_capi` encodes two lines: ratio failure 82
and skipped `AssertValid`. Those expectations need deliberate baseline review;
the boundary itself is unchanged.


## Follow-up: local adjacency and conservative clearance

The first exclusion implementation treated every different topology ID as separate
geometry. That was too strict for the contract in §4/§8: unmerged sweeps, split
boundaries and touching solids can represent the same local boundary with different
IDs. It also returned `InvalidInput` instead of a failed check, preempting the
existing local-operation refusal and losing the other validity diagnostics.

The revised neighbourhood starts at the incident **curve endpoints**, with their
edge zones. An edge is adjacent only if its bounded curve meets one of those fixed
endpoints within the sum of their edge zones. A face is adjacent only through one
of those edges, or its trimmed surface meets an endpoint within the edge and face
zones (including interior point contact and vertex loops). Newly adjacent edges do not extend the search. Neither the vertex radius
nor a displaced stored vertex point grows this neighbourhood. This is deliberately
local: the opposite x=1 face of the 1×2×3 origin-corner box remains separate even
when the corner's radius becomes 2. The exact curve-drop and surface-drop/trimmed
face classification queries still measure contact; bounding boxes only prune.

A contact now produces a numbered pointer finding (82), with both entity keys,
separation and radius. Unsuccessful or non-finite distance probes, geometric query
errors and inconclusive face classifications produce an inconclusive-clearance
finding instead of passing or aborting the walk. Cancellation and unavailable
implementations propagate. Failed bounding boxes do not establish clearance; they
leave the exact query unpruned. Existing tests add a genuinely unevaluable curve
and require findings for multiple independent vertices while the walk completes.

### Audit of the eleven SolidCore failures

| Test | Cause and disposition / Test change reason |
|---|---|
| trimming_tools::issue_476_loop_junction_uses_previous_end_not_previous_start | Correct the fixture's world point by evaluating its face at the fixture UV. `Plane::from_normal(Z)` maps UV (0.5,0) to world (0,0.5,0), not (0.5,0,0). Keep endpoint-zone assertions. |
| issue_1937_bottle_sweep::regularization_preserves_ordinary_box_sweeps | Distinct boundary IDs in the same geometric star. Keep its sweep expectations. |
| issue_1937_bottle_sweep::regularization_handles_near_parallel_box_sweep | Same adjacency cause; keep its sweep expectations. |
| surface::surface::p2::issue_892_tests::issue_892_only_join_touching_loops_changes_the_sweep | Touching loop boundaries are local neighbours. Keep expected join/no-join behavior. |
| solidcore-convert::brep::lumps_and_solids_touching_along_an_edge_go_round | Different lump boundary IDs at a shared geometric edge. Keep round-trip expectations. |
| local_operation::a_coplanar_neighbour_raised_off_its_twin_is_refused_as_not_sound | The old exclusion error preempted the intended Unsound refusal. Normal findings restore that path; keep refusal/rollback assertions. |
| local_operation::a_far_top_on_a_unit_square_plane_is_refused_as_not_sound | Same report-versus-error problem; retain Unsound/rollback assertions. |
| solidcore-ops::a_failed_check_reports_its_lines | Replace a stale upper-ratio violation with a vertex radius below the body lower bound. Preserve the failed-check/report-lines contract. |
| issue_2972_volume_sign_enclosure::the_solid_test_names_the_test_a_body_fails | Likewise use the still-invalid lower bound instead of a removed upper ratio. Preserve PointersInvalid and named-test assertions. |
| wr_450_box_minus_rational_cylinder_is_valid_only_with_the_fix | Remove the two obsolete ratio-82 expectations; retain the actual edge-59 failure and its fix requirement. |
| wide_vertices::the_check_refuses_a_vertex_tolerance_at_the_limit | Main-only test still assumes an upper ratio. This file is absent from this worktree/head, so it cannot safely be changed here without importing a separate production feature. The exact test-only replacement is below; applying it to the merged main tree remains blocked. |

The box exclusion regression now expects an invalid **report**, not an exception;
radii 0.1 and 0.999 remain accepted, and radii 1 and 2 remain refused. New private
pointer tests cover distinct touching/nearby boundary IDs and show that increasing
the corner radius leaves its three-edge neighbourhood unchanged.

### Golden derivations and remaining regeneration blocker

No golden was edited by hand or re-recorded in this follow-up. The documented
runner is `buildq run solidcraft <worktree> -- cargo run --profile goldens -p goldens -- --unit <unit> --goldens`
(and `--bless` only after checking the resulting per-case derivation). Buildq
refuses the runner in this fix lane with exit 2. A final, regenerated per-case diff
is therefore still required; the old CI deltas below are an audit, not a claim
about a run of this revision.

For `topology.assert_valid`, all eleven previous errors must become numbered
geometric findings rather than exceptions with empty reports:

- `l2_box_level2_m6_v-1_i0`, `i1`, `i2`, `i3`, `i4`, `i5`, `i6`, `i7`:
  each displaces one unit-box vertex by -1 in x and grows its ball to cover that
  gap. The enlarged ball reaches the opposite, separate boundary. Unlike a local
  split boundary, that face has no edge near an original incident curve endpoint.
  The first two used to pass; others already had findings which must not be lost.
- `negative_tolerance_fuzz00133`: the setter normalizes -1 to a radius of 1.
  That ball reaches the opposite face at x=1; its sign normalization does not make
  the geometry locally adjacent.
- `update_box_m6_i0_v1_l0`, `update_box_m6_i0_v1_l2`: moving the vertex by +1
  and updating to radius 2 covers the measured gap but reaches separate topology.
  Keep tolerance updating separate from validity; report the geometric breach.

For `corpus.parity`, CI records only lower failure counts (`check` and
`rt_check`), with topology, area and volume unchanged in each visible case:

| Case | Old → previous CI failures, before/after round trip | Observed delta; individual report derivation remains pending |
|---|---|---|
| Endcap.smb | 1 → 0 | One fewer finding; no shape change. |
| Fillet__bolt.smb | 10 → 8 | Two fewer findings; eight remain. |
| Fillet__bottle.smb | 9 → 8 | One fewer finding; eight remain. |
| Fillet__fillet1017.smb | 11 → 4 | Seven fewer findings; four remain. |
| Fillet__fillet1019.smb | 14 → 13 | One fewer finding; thirteen remain. |
| Fillet__sphere_diff_3cylinders.smb | 4 → 2 | Two fewer findings; two remain. |
| Fillet__vase.smb | 12 → 4 | Eight fewer findings; four remain. |
| Fillet__vase1.smb | 4 → 2 | Two fewer findings; two remain. |
| Fillet__vase2.smb | 7 → 1 | Six fewer findings; one remains. |
| Fillet__vase3.smb | 5 → 0 | Five fewer findings; no shape change. |

The runner stores `failure_count`, not the identity of each finding. These counts
are consistent with the removed ratio and corrected local face-gap checks, but
each new runner result must still be inspected; counts alone do not prove which
individual reports disappeared. The ten visible count deltas are not yet sufficient
derivations for blessing them. The other
three changed corpus case IDs are hidden by CI's ten-item output limit. Their
source models are absent locally, the runner is refused, and GitHub access failed;
their identities and individual derivations remain blocked, not silently blessed.

For `boolean.api_brep::issue2177_crescent_on_top_of_rim_checked`, CI explicitly
removes `(class=16011,list=1,test=82,count=2)` while retaining test 59 and the
12225 reports. Status and record blocks are unchanged. This is the obsolete pair
of vertex-ratio findings, not a geometric change; inspect the final regenerated
line before blessing it.

### Runtime evidence and checks

An authorized debug CLI build succeeded. Its each-feature kernel-rig replays pass
`sweeps/3191-revolve-cut-leaves-a-cone`, fillet 1488, ladder external R 1/2 (022)
and external 1/2 NPT (023), including normal record and SolidCore validation.
3191 measures volume 12566.37061448028 versus analytic 12566.370614359172.
No ladder or rig golden changes are needed. Replay output is
`/tmp/fix16183-round4-replay.json`. These initial replay results precede the last
conservative-query error handling edits; the final CLI rebuild/replay is tracked
below.

Both native repair setups replay successfully: an unstitched 10 mm box and its
placed copy repair to six faces, closed/modelling=true, volume 1000. The legacy
naming design retains its three intended reselect errors. Two fresh naming builds
have identical saved names and cached geometry. These focused CLI checks do not
claim to execute the whole naming corpus; the native failure log was not supplied.

Attempted targeted tests for issue_1419, pointer tests, issue_476, issue_2972,
wr_450, solidcore-ops report-lines, engine repair_part_ and CLI naming_corpus all
receive buildq exit 2 (fix-lane CI-only policy). Crate-targeted clippy with warnings
denied is likewise refused. All three targeted golden comparisons are refused.
No full suite, direct local cargo test/clippy, BUILDQ_FULL override, commit, push,
or other git metadata write was performed. The owner must push for CI; this lane
cannot do so under its explicit unstaged-only rule.

### Test-only correction for the main-only centering test

Apply this replacement in the existing `wide_vertices.rs` test after main is
integrated. It preserves the centering heuristic without treating its threshold
as a validity upper ratio:

```rust
    /// The centering threshold is a heuristic, not a validity limit. A finite local
    /// ball remains valid while it stays clear of separate topology.
    #[test]
    fn the_check_accepts_local_tolerances_at_the_centering_limit() {
        use crate::topology::brep::validation::{check_brep, CheckLevel};
        for factor in [0.999_999, 1.0, 2.0] {
            with_box(|k, b| {
                let (v, _, limit) = corner_and_limit(b);
                b.vertex_mut(v)?.tolerance = factor * limit;
                let c = check_brep(k, b, CheckLevel::Level2)?;
                assert!(c.is_valid(), "at {factor} of the centering limit: {:?}", c.lines());
                assert_eq!(b.vertex(v)?.tolerance, factor * limit);
                Ok(())
            })
            .unwrap();
        }
    }
```


### Cone pole contact in the SolidCore API rig

The CI failure for 3191 is specifically the **SolidCore API rig** boolean call,
not the CLI command rig. Both initially available CLI binaries pass the command
rig, which alone does not prove the API rig regression fixed. Exporting that same
case reveals the missing neighbourhood rule: cutter `b3` has pole vertex 2 at
(0,0,30), radius 0.00002, incident only to cone seam edge 3. Its cap (face 2)
has a circular boundary of radius 20 and no vertex loop at the pole. The pole
therefore touches the cap's **interior** without sharing any cap boundary ID.
The initial strict exclusion rejects that local singular contact when the API
rig reuses the exported cutter as an operand.

The neighbourhood now also drops the fixed incident curve endpoints onto each
candidate trimmed face, at the sum of endpoint-edge and face zones. A successful
interior/edge/vertex contact makes the face adjacent. The wide vertex radius is
still absent from this decision; the opposite box face remains at distance 1
from the fixed corner, well outside the edge/face zones. Failed adjacency probes
remain conservative findings if no other endpoint establishes adjacency.

The new existing-module regression puts a separate wire endpoint in a box face's
interior, accepts the local contact, then widens that vertex and requires refusal
when its ball reaches the opposite face. This covers the pole/cap representation
without importing another rig fixture or changing a golden. The API rig test
itself is CI-only under this lane's buildq policy; its exact test command is
`buildq run solidcraft <worktree> -- cargo test -p solidcore-ops --test kernel_cases`.


### Final geometry CLI replay results

Buildq CLI job `1794622588.809665-cc98c38c` completed with exit 0 (dev profile,
5m52s compilation). It includes conservative query findings and trimmed-face
interior adjacency. Two redundant `Surface` qualifications were warned about and
subsequently removed; the follow-up build is only a warning cleanup, with no
geometry behavior change.

The rebuilt CLI's each-feature replay again exits 0, with **all 28 checks passing**:
3191 (9), fillet 1488 (5), ladder 022 (7), ladder 023 (7). Output is
`/tmp/fix16183-round4-final-replay.json`. The two repair setups also exit 0 and
produce closed six-face boxes, volume 1000 each; their results are
`/tmp/fix-16183-repair-{missing,placed}.final.result.json`.

The rebuilt CLI's focused naming replay also passes. The legacy design has only
the three expected Round/Bevel/Raise reselect errors, four earlier features build,
and its closed body's volume is 10500. Both fresh authored builds complete all
11 commands with no warnings or errors. Their saved reference names match,
`cache/bodies.bin` is byte-identical, and `cache/model.json` matches after excluding
timings and filename-derived metadata. Summary:
`/tmp/fix-16183-final-naming-verification.json`.

These are CLI replay results, not a claim that the SolidCore API rig, native Rust
tests or full naming corpus ran. Their buildq commands were refused (exit 2).
Golden regeneration and final corpus per-report derivations remain blocked for
the reasons above. No expected golden or ladder result was edited.


### Final cleanup build and deliverables

The follow-up CLI build, job `1794622715.719581-ece3a016`, also completed with
exit 0 (6m56s compilation), **without warnings**. It changes only redundant type
qualifications from the functionally replayed build. Selected-file rustfmt and
`git diff --check` pass. All changes remain unstaged; `COMMIT_MSG.txt` contains
the owner's proposed commit message, without trailers.

The remaining blockers are explicit:

1. Buildq refuses the targeted Rust tests, crate clippy, API kernel rig and all
   three golden-unit runners with exit 2. No override or direct local test/clippy
   invocation was used. These checks still need CI, alongside its full suite.
2. All 25 previous golden deltas still need a run of this revision. In particular,
   corpus counts alone do not derive report identities, and three changed names
   are truncated. Models and the native failure log are absent locally; GitHub
   access failed. No golden was blindly blessed.
3. The stale centering test exists only on main, in feature #16187 (8d122477d),
   not this branch or its merge base. Integrating that feature is a prerequisite
   to applying the prepared test-only correction; doing so requires git metadata
   writes, prohibited in this lane. Adding its whole production module here is
   not a test-only change and risks an add/add merge conflict. The exact unified
   test patch is also available locally at
   `/tmp/fix-16183-wide-vertices-main.patch`.

Changed source/test files are `validation/pointers.rs`, `topology/trimming_tools.rs`,
`tests/issue_1419_brep_check.rs`, `tests/issue_2972_volume_sign_enclosure.rs`,
`tests/wr_450_box_minus_rational_cylinder.rs`, and `solidcore-ops/src/tests.rs`.
The validity document and this finding carry the contract and review evidence;
`COMMIT_MSG.txt` is the additional handoff artifact. CI logs were not modified.
