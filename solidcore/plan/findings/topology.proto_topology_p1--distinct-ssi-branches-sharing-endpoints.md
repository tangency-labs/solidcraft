# topology.proto_topology_p1: a midpoint match discards a distinct surface-intersection branch (2026-11-18)

**Units:** topology.proto_topology_p1 (`is_same_edge_definition`, `is_same_proto_edge`), topology.proto_topology_p2 (`find_proto_edge`), topology.proto_topology_p3 (`find_or_create_proto_edge`), curve.proximity (`proved_together`). **Status:** blocked: the midpoint discard is confirmed, but correcting it does not complete the Boolean. The whole-curve candidate preserves both arcs and still fails both offsets. No experimental production code or unverified unit test is retained. A complete exact correction needs a classification certificate; the remaining runtime classification mechanism has not been established. Reference: [Booleans](../../docs/booleans.md), “One point or two” and “One edge or two”.

**Case** (tangency-labs/solidcraft#18475). Two equal holes of radius 0.9 cross at right angles in a 25 × 20.9 × 4.5 block. The block starts at `(0, -10.45, 0)`. The first hole is along Z, based at `(12.5, 0, -1)` and 6.5 long; the second is along Y, based at `(12.5 + d, -11.45, 2.25)` and 22.9 long, with its seam along +X. The existing regression file is `crates/solidcore-ops/tests/boolean.rs`, test `a_hole_whose_axis_misses_an_equal_crossing_holes_by_a_hair_cuts`. The nine offsets retained for the regression are:

```text
0, -1e-7, -1e-6, -5e-6, -1e-5, -2e-5, +1e-5, -1e-4, +1e-3
```

The report builds at the other offsets and refuses the second cut at `-1e-5` and `-2e-5` as `NotSolid`. The baseline regression was reproduced failing at `-1e-5`; this finding does not claim a completed validation of all nine offsets with the candidate correction.

**The two branches.** Translate the Z axis to `(x, y) = (0, 0)` and the Y axis to `(x, z) = (-δ, 0)`, where `δ = -d > 0`. The two walls are

```text
x² + y² = r²,
(x + δ)² + z² = r².
```

The Y cylinder's +X seam has `x = r - δ`, `z = 0`. Its crossings of the Z wall therefore have `y = ±q`, where `q = sqrt(2rδ - δ²)`: their separation is `2q`. For `r = 0.9`, `δ = 1e-5`, `q ≈ 0.0042426289`. The wall intersection is one loop split into two curves at these seam crossings. Both curves have the same two endpoints, but one takes the positive-Z side and the other the negative-Z side. For example, at `x = 0`, `y = r`, their Z coordinates differ by `2 sqrt(r² - δ²)`, nearly 1.8. Matching their endpoints cannot identify them as one curve.

**Confirmed discard.** The instrumented baseline shows both crossing curves survive `FaceIntersect`'s extraction: each `extract_geometry_mut` produces one curve. Each has one full classification interval, approximately `0..6.861435837662`, with midpoint classes Face/Face; the Y classifications at both ends are Edge. The two endpoints remain distinct proto vertices `PV2` and `PV3`, with point tolerance `1e-5`; the lookup's capture distance is `0.01`.

The first branch creates `PE2`. The second reaches `find_proto_edge` with those same endpoint proto vertices and finds `PE2` with `is_identical = true`. Its evaluated midpoint is close to the first branch's evaluated midpoint:

```text
upper: (11.600000001674385, approximately 1.56e-5, 2.25424314654)
lower: (11.600000001674385, approximately 1.56e-5, 2.24575685346)
```

The midpoint separation is about `0.00848629`, below the capture distance. `is_same_edge_definition` calls `are_same_curve`, whose geometric comparison is only the drop of the stored curve's midpoint onto the incoming curve, using that capture distance. The identical source face pair then makes the result “identical”. `find_or_create_proto_edge` creates neither another proto edge nor another definition for an identical match, so the entire lower branch is discarded. Only three definitions remain: the two stock exits and one wall-intersection branch. No creation error or later split removes a second branch in this trace. The surviving common wall-intersection edge has degree-one endpoints, and a piece of the tool's interior cylindrical wall is kept; the operation returns `NotSolid`.

**Search distance and identity.** The stored capture distance is a lookup radius, not a statement that curves as much as 0.01 apart represent the same exact edge. The Boolean rules already distinguish this from local geometry tolerances: “One point or two” says it only determines where to look, and “One edge or two” requires the two whole curves within their summed tolerances, or within the endpoint vertex balls. Discarding a same-source definition does not increase the surviving definition's tolerance to the capture distance. Retaining that radius for a stronger whole-curve test would reject these widely separated branches, but would not establish identity at their declared local precision in general.

The proposed correction is to retain the same-handle/same-interval shortcut and otherwise use the existing `Curve::proved_together` construction over both complete intervals, with the stored definition's tolerance plus the manager's tolerance and the two endpoint proto-vertex balls. Its proof encloses each curve piece by a chord and its Bézier hull radius, including rounding, and bounds the piece against a facing piece of the other curve. It checks both directions. The drops select facing intervals; they do not certify distance. The experiment below did not fix the complete Boolean and has been removed. It is recorded as evidence, not as a retained correction.

**Candidate result, not a complete fix.** In buildq job `1795027714.346360-939b6f74`, the whole-curve comparison rejects identifying the lower arc with the upper arc at both `d = -1e-5` and `d = -2e-5`. All four proto edges and definitions survive resolution, and all four imprints return success. The diagnostic found no definitions sharing a non-null final edge. Both final cuts still return `NotSolid` (exit 101, 0 passed, 1 failed, 56 filtered out). Thus preserving the two arcs is necessary for the first offset but is not sufficient to fix the issue. It does not establish that the old midpoint discard explains the second offset.

No unclassified-face pass ran for either failed cut. The observed rays only set up the infinite regions, with tied hits at an edge. There is no evidence that ray-hit ordering explains this failure, and it is outside this change. A further trace was prepared to record each definition's final edge pair, the common-edge incident faces, radial classes and the face sets each class floods. Job `1795038581.952593-eac3dcd4` was canceled before starting after more than two and a half hours in the queue; it produced no runtime evidence. A successful imprint alone does not establish that the flood's topology is right.

The expected pre-classification partition is eight primary faces (six stock walls and the two pieces of the Z wall), and seven tool faces (two caps, two wall bands outside the stock, and three middle wall patches). The two middle tool patches inside the Z hole meet across the unmarked seam and should flood together for deletion. The middle tool patch outside the Z hole should flood separately for keeping. A flood joining an inside patch with the outside patch would locate an imprint/barrier defect; distinct components with the wrong class would locate a radial-classification defect. These partitions are geometric expectations, not yet measured counts.

The radial classifier's finite step-offs and angular coincidence thresholds are explicitly uncertified in `docs/booleans.md` §4.3. At the two arcs' midpoints the analytic cylinder normals are nonparallel, with decisive component `±sqrt(2rδ - δ²)/r`. That alone is not a certificate on normals evaluated at an approximate SSI point: across-intersection uncertainty grows as `tol / sin(theta)`. A classifier change needs a derived enclosure or a certified local implicit predicate, not a guessed step, a tightened angular threshold or a retry at another point. The trace has not yet shown that this is the remaining failure.

**Uncertified step-off construction.** `topology/edgeuse/p2.rs::evaluate_binormal_step_off_into` checks only `face.uv_domain`, then solves the plane/surface/sphere equations on the underlying surface. `FindStepOff` receives no trim loops. A split face initially inherits the old rectangular domain (`brep/p9.rs`), so a step can pass the opposite SSI trim while remaining in that rectangle and returning success. The nonconverged local solve also accepts a clamped UV guess. This is a structural gap in the routine's “stepped off the face” contract; its runtime role in this case remains unconfirmed.

On the left generator of the Z wall (`x = -r, y = 0`), the Y wall's left side is `x_Y(z) = -δ - sqrt(r² - z²)`. Between `z = ±q`, its distance from the Z wall never exceeds `δ`. A downward step from the upper root reaches the opposite SSI trim after `2q`: approximately `0.008485257804` for `δ = 1e-5`, and `0.011999933333` for `δ = 2e-5`. Past the lower root the relative side reverses, so accepting such a step can invert the class. The canceled trace would have recorded each step's point, face and returned sector; no such runtime result is claimed. Endpoint membership alone would not establish first-boundary behavior: a path can leave and re-enter a trimmed face.

There is a second, independent geometric limit to adding a trim check alone. At a finite step, `evaluate_binormal_step_off_into` returns `normal_at_step × edge_tangent`, the tangent direction of the normal section at the step point. Its order can reverse before the section crosses a trim: in the left-generator model the Y normal changes sign at its leftmost point, `z = 0`, reached at chord distance `sqrt(δ² + q²) = sqrt(2rδ)`. For `δ = 1e-5` this is approximately `0.004242640687`, before the opposite trim at `0.008485257804`. The radial chord from the common point remains on its original side until that opposite trim. A whole UV-path membership check would therefore not by itself prove the order of the endpoint tangents.

A sound finite neighborhood orders chords from one certified common point, on one transverse plane and sphere, on certified inward section branches, with the radius before their first trim, meeting or singular event. Merely substituting chords is insufficient while the current local solve can choose another sphere root, accepts a nonconverged clamped guess, and has no first-boundary constraint. The existing normal cones and trim-intersection APIs are building blocks, but this review found no certificate for that full neighborhood. A smaller local differential certificate is also possible in principle: at `x = 0`, `y = ±r`, `z = ±sqrt(r² - δ²)`, the two cylinder normal lines are perpendicular. Existing `Curve::hull_breaks`, `piece_ball`, `tangent_cone`, `Surface::normal_cone_in_ball` and `intersection_reach` can bound positions, surface normals and the stored curve's tangents. They do not establish the true SSI branch's signed incidence and orientation against the pcurves. `docs/ssi.md` §6.1 explicitly leaves topology/uniqueness in the port and the B-spline between traced points unproved; `docs/validity-and-tolerances.md` §10 records missing edge and pcurve error certificates. An enclosed determinant would need these uncertainties before its sign could certify a class. Retrying a well-separated parameter alone would therefore still be uncertified. Either construction needs a new certificate; this does not establish that the larger sphere-neighborhood construction is the only possible fix. This review has not established which limits the reported cuts reach at runtime.

**Idealized step sequence, not a runtime trace.** The following calculation evaluates the classifier's adaptive rules on exact plane/circle sections at the left generator, with `r = 0.9`, `δ = 1e-5` or `2e-5`, and summed edge tolerance `τ = 2e-5`. It assumes the two sector faces follow the Z generator down and up, the classified Y face follows the inward circle branch down from the upper crossing, the section plane is exactly `y = 0`, and every requested step succeeds on the underlying untrimmed surfaces. These assumptions are not yet mapped to the actual edge uses, approximate SSI tangent, points or tolerances in a runtime diagnostic. Python numerical arithmetic was used only to evaluate the expressions below; no kernel code, build or test was run for this calculation.

Work in `(x, z)`. Put `q = sqrt(2rδ - δ²)`, `P = (-r, q)`, and `φ = atan2(q, r - δ)`, so `cos φ = (r - δ)/r` and `sin φ = q/r`. The Y section is the circle about `C = (-δ, 0)` of radius `r`; at `P` its polar angle is `π - φ`. At sphere radius `s`, the inward circle branch turns through

```text
h(s) = 2 asin(s / (2r)),
α(s) = h(s)/2 - φ,
Q(s) - P = s (sin α, -cos α).
```

The Z section points are `A(s) = P + (0, -s)` and `B(s) = P + (0, s)`. In the range used here the classifier's minimum step-point distance is therefore

```text
m(s) = min(|Q - A|, |Q - B|) = 2s |sin(α/2)|.
```

The two Z binormals have angle `π`. The Y endpoint differential binormal, oriented down along this branch, is `(sin(h - φ), -cos(h - φ))`; its angle to the downward Z binormal is `a(s) = |h - φ|`, and its angle to the upward one is `π - a(s)`. Its X sign flips at `h = φ`, or `s = sqrt(2rδ)`. The radial chord's X sign instead flips at `α = 0`, or `s = 2q`, where the branch meets the opposite SSI trim. For the sector swept from downward Z through positive X to upward Z, the initial differential class is Outside and the class after `h > φ` is Inside. The actual sector/region assignment remains unconfirmed; reversing the sector changes the labels but does not remove this inversion.

Apply the existing adaptive rules with `count` read at the start of each iteration. The first positive steps are `τ` and `2τ`. Subsequently the proposed next step is `τ (s - s_previous)/(m - m_previous)`, limited to between `s/3` and `3s`; a negative proposal becomes `2s`. A denominator within `EFF_ZERO` also gives `2s`. Neither a zero sector nor a held coincidence occurs in this model. No iteration accumulates a coincidence run: neither the distance-steady test nor the 0.01-degree angle test passes. At an iteration with `count > 2` and `m > τ`, the two sector step points are already more than `τ` apart, so the main separation exit returns the current differential inside/outside class.

For `δ = 1e-5`, `q ≈ 0.004242628901989897`, `φ ≈ 0.270095144935623°`, the differential flip is at `0.004242640687119285`, and the opposite trim is at `2q ≈ 0.008485257803979794`:

| count | step `s` | minimum distance `m` | differential angle `a` (degrees) | circle point `Q.z` | chord X sign | differential class |
| --- | ---: | ---: | ---: | ---: | :---: | --- |
| 0 | 0 | 0 | 0.270095145 | 0.00424262890 | zero | Outside |
| 1 | 0.00002 | 9.40586826e-8 | 0.268821905 | 0.00422262912 | negative | Outside |
| 2 | 0.00004 | 1.87672922e-7 | 0.267548666 | 0.00420262934 | negative | Outside |
| 3 | 0.00012 | 5.57685447e-7 | 0.262455708 | 0.00412263020 | negative | Outside |
| 4 | 0.00036 | 1.62505647e-6 | 0.247176833 | 0.00388263257 | negative | Outside |
| 5 | 0.00108 | 4.44317037e-6 | 0.201340205 | 0.00316263804 | negative | Outside |
| 6 | 0.00324 | 9.44151413e-6 | 0.0638302273 | 0.00100264266 | negative | Outside |
| 7 | 0.008642862931690191 | 7.56763852e-7 | 0.280128713 | -0.00440023400 | positive | Inside |
| 8 | 0.01728572586338038 | 8.45146535e-5 | 0.830365256 | -0.0130428904 | positive | Inside; separation exit |

Count 7 has already passed the opposite trim. Its minimum distance decreases, so the negative adaptive proposal doubles the next step. Count 8 first passes the separation test and returns the inverted class. The positive-X chord components at counts 7 and 8 are approximately `7.56763851e-7` and `8.45144010e-5`. A chord substitution without a first-boundary constraint would also have crossed to the wrong side by this exit.

For `δ = 2e-5`, `q ≈ 0.005999966666574074`, `φ ≈ 0.381972570779388°`, the differential flip is at `0.006`, and the opposite trim is at `2q ≈ 0.01199993333314815`:

| count | step `s` | minimum distance `m` | differential angle `a` (degrees) | circle point `Q.z` | chord X sign | differential class |
| --- | ---: | ---: | ---: | ---: | :---: | --- |
| 0 | 0 | 0 | 0.381972571 | 0.00599996667 | zero | Outside |
| 1 | 0.00002 | 1.33111112e-7 | 0.380699331 | 0.00597996711 | negative | Outside |
| 2 | 0.00004 | 2.65777783e-7 | 0.379426092 | 0.00595996755 | negative | Outside |
| 3 | 0.00012 | 7.92000044e-7 | 0.374333134 | 0.00587996928 | negative | Outside |
| 4 | 0.00036 | 2.32800039e-6 | 0.359054259 | 0.00563997419 | negative | Outside |
| 5 | 0.00108 | 6.55200325e-6 | 0.313217631 | 0.00491998654 | negative | Outside |
| 6 | 0.00324 | 1.57680213e-5 | 0.175707653 | 0.00276000504 | negative | Outside |
| 7 | 0.004687490822730780 | 1.90429863e-5 | 0.0835572994 | 0.00131251453 | negative | Outside |
| 8 | 0.008839732935978843 | 1.55198864e-5 | 0.180784568 | -0.00283975265 | negative | Inside |
| 9 | 0.01767946587195769 | 5.57858412e-5 | 0.743555280 | -0.0116794112 | positive | Inside; separation exit |

Here count 8 is still before the opposite trim: its chord X component is approximately `-1.55198805e-5`, yet its endpoint differential binormal has already inverted. Its minimum distance is below `τ`, so the loop continues; the negative adaptive proposal again doubles the step, and count 9 crosses the opposite trim and takes the separation exit. This idealized sequence needs both a correct neighborhood direction and a first-event constraint. It does not depend on discarding an SSI branch, so it is a possible independent explanation for the second failing offset, which already retains both arcs. The actual diagnostic must still establish its face and point correspondence before this is called the runtime cause.

**Support review.** The existing proximity proof has hulls for lines and NURBS-backed B-splines and conics. It proves nothing for independent copies of Hermite, offset, composite, projected, iso or curve-on-surface curves. Coincident-face boundary extraction can copy an original edge of one of these concrete kinds under a new handle (`topology/face/p3.rs`, `FaceIntersect`), so the same-handle shortcut does not cover every production boundary. Rejecting an unproved identity is conservative, but retaining duplicate proto edges may expose a later refusal. No such new refusal has yet been reproduced by this review; the experiment is removed, and no new production refusal is retained. The inherited exact-handle shortcut also bypasses missing-handle and interval validation in the low-level API; the production caller resolves its incoming curve before lookup.

**A finite proof budget.** `curve/proximity.rs` currently stops a proof after 4096 attempted pieces, counting unsuccessful parent pieces as well as accepted leaves. A hand calculation shows why that can refuse an exact large-circle comparison even at an ordinary local tolerance. It does not establish the cause of any earlier candidate-test failure.

Take two independent copies of a radius-16 circle, no endpoint balls, and proof radius `ε = 1e-5`, the default zone tolerance. The circle constructor makes four rational quadratic quarter arcs. On a quarter arc's normalized parameter, angular speed lies between `sqrt(2)` and `4(sqrt(2) - 1)`. After `n` parameter halvings, each piece's angular width `α` is between those values divided by `2^n`. A circular quadratic's middle control point is `R(sec(α/2) - cos(α/2))` from its chord. For identical facing pieces, the two tube radii sum to

```text
2R sin²(α/2) / cos(α/2).
```

At depth 10 this is at least `1.5258790e-5`, above `ε`; at depth 11 it is at most `5.2359888e-6`, below `ε` with room for rounding. Thus the expected cost is 8192 accepted leaves, approximately `4 × (2^12 - 1) = 16380` attempted nodes per direction. Actual counts still need measurement because the seam's dropped parameter can introduce another initial cut.

A resource cap of `1 << 16` (65536), an existing scale of finite work budget in `curve/length_bound.rs`, was considered during the experiment. It changes neither the proof radius nor the bound: an additional accepted result must satisfy the same enclosure inequality. Depth and cancellation checks remain. Proximity counts attempted nodes, whereas the length bound counts resulting pieces; equal constants do not imply equal work. An experimental inline proximity regression compared an independent radius-16 copy, a 90° shifted seam, and reversed orientation at the default zone tolerance. The diagnostic prelude of job `1795027714.346360-939b6f74` confirms the independent-copy comparison stops at attempted piece 4097 and returns false at radius `1e-5`. The larger cap and that unverified unit test were removed. The three-case inline regression was queued, then canceled before execution while the complete-fix construction remained unresolved. Production retains its original 4096-piece budget.

**Volume oracle for all nine offsets.** The unchanged oracle is the zero-offset closed form

```text
want = 25 × 20.9 × 4.5 - πr²(4.5 + 20.9) + 16r³/3.
```

The individual through-hole volumes do not vary with these offsets. Only their common volume changes. Put `f(x) = 2 sqrt(max(r² - x², 0))`; the common volume is the autocorrelation `Vδ = integral f(x) f(x - δ) dx`. Translation invariance gives

```text
V0 - Vδ = (1/2) integral (f(x) - f(x - δ))² dx ≥ 0.
```

The offset sign does not matter. For the test range `0 < δ ≤ 0.001`, split the union of the supports, `[-r, r + δ]`, into the interior `[-r + 2δ, r - δ]` and its boundary pieces, whose total length is `4δ`. Cauchy–Schwarz and integration over the interior give

```text
(1/2) integral_interior (f(x) - f(x - δ))² dx
  ≤ (δ²/2) integral_[-r+δ,r-δ] f'(x)² dx
  = δ²[-4(r - δ) + 2r ln((2r - δ)/δ)].
```

For the boundary, `F = f² = 4 max(r² - x², 0)` is globally Lipschitz with constant `8r`. Since `(sqrt(F) - sqrt(G))² ≤ |F - G|`, the boundary contribution is at most `(1/2)(4δ)(8rδ) = 16rδ²`. Consequently

```text
0 ≤ V0 - Vδ
  ≤ δ²[12r + 4δ + 2r ln((2r - δ)/δ)]
  < δ²[16r + 2r ln(2r/δ)].
```

The last expression increases over this range. At `r = 0.9`, `δ = 0.001`, the tighter bound is `2.429497522111096e-5` and the last bound is `2.789197549899166e-5`, below `3e-5`. The kept volume is `want - (V0 - Vδ)`. Thus the test comment must say “under 3e-5” when retaining `+1e-3`, rather than “under 2e-5”. The existing `near` predicate, `1e-7 × max(|want|, 1)`, allows approximately `2.29e-4`; neither its tolerance nor the expected value needs changing. No golden or kernel-case value is derived or blessed here.

**Validation and retained changes.** Only the existing Boolean regression is changed: its mathematical comments are corrected and offsets `-1e-7` and `-5e-6` are added. Its expected value and `near` predicate are unchanged. The rest of the changes are this finding and `COMMIT_MSG.txt`. No production change, experimental logging, unrun unit test, golden or kernel-case expectation is retained. The issue remains open.

All cargo commands below were run from the worktree root through `buildq run solidcraft . --`. No main files were merged or copied into the worktree.

- Baseline reproduction: `cargo test --manifest-path solidcore/Cargo.toml -p solidcore-ops --test boolean a_hole_whose_axis_misses_an_equal_crossing_holes_by_a_hair_cuts -- --exact --nocapture`. Job `1795009377.079597-30b5131a`, exit 101: the named test FAILED at `d = -0.00001` with `NotSolid`; 0 passed, 1 failed, 56 filtered out. The baseline included the nine-offset regression. Later instrumented baseline jobs reproduced that failure and located the midpoint discard.
- Whole-curve experiment, same exact test command: job `1795027714.346360-939b6f74`, exit 101: both `-1e-5` and `-2e-5` returned `NotSolid`, 0 passed, 1 failed, 56 filtered out. All four proto definitions and successful imprints were observed. This experiment is removed.
- Wider experimental run: `cargo test --manifest-path solidcore/Cargo.toml -p solidcore-ops --test boolean -- --nocapture`, job `1795018303.263512-73c24531`, intentionally stopped with exit 130 after the issue and six other tests failed. The other names were `a_traced_cut_flush_with_an_earlier_stadium_cut_is_its_closed_form`, `a_cut_whose_bottom_is_a_hair_above_a_stadium_holes_flat_side_removes_the_rest`, `a_join_whose_hole_lines_up_with_a_bore_run_the_other_way_is_one_bore`, `a_cylinder_cut_flush_with_a_rounded_bore_ceiling_is_one_valid_solid`, `a_drum_cut_on_its_void_ceiling_with_a_hole_through_both_is_two_valid_solids`, and `a_rod_cut_by_two_counter_wound_channel_sweeps_is_valid`. The causes of those six failures were not established; the candidate was removed rather than shipping that result. No affected-file pass is claimed.
- Diagnostic renderer: `cargo build --manifest-path solidcore/Cargo.toml -p diagnostics`, job `1795019315.465542-068987ee`, exit 0. Native wireframe rendering of the failing body also exited 0. The baseline body has one retained SSI arc; the zero-offset rendering is a reference, not a fixed result. Artifacts are `/tmp/lane-18475-evidence/lane-18475-before.png` and `/tmp/lane-18475-zero-reference.png`.
- Clippy: `cargo clippy --manifest-path solidcore/Cargo.toml -p solidcore -p solidcore-ops --all-targets -- -D warnings`, job `1795029122.765189-3ff1f843`, canceled before starting (exit 130) after waiting over 30 minutes. No clippy pass is claimed.
- Core identity/proximity unit tests and the final optional radial trace were canceled before starting (exit 130). Their experimental source is removed, and they supply no test evidence.
- Final `cargo fmt` and `cargo fmt --manifest-path solidcore/Cargo.toml --all` both exited 0 after cleanup; final `git diff --check` also exited 0. The wider Boolean file, clippy, full suite, wasm and golden checks are left to CI under the owner's updated rule to stop waiting for wider checks after 30 minutes. No commit or push was made; all changes remain unstaged.

**Blocker.** The demonstrated midpoint identity defect is at `crates/solidcore/src/topology/proto_topology/p1/proto.rs:3286`: it uses the capture radius for a one-point match and discards a distinct arc. Preserving that arc is insufficient, as the two-offset experiment demonstrates. The available radial classification in `query/edge.rs` and finite step-off in `topology/edgeuse/p2.rs` do not supply the required oriented-intersection certificate; `docs/booleans.md` §4.3 explicitly records certified classification as open work. A complete exact correction needs that new certificate, either for a local differential predicate or for a bounded neighborhood. Neither a guessed threshold nor an unverified retry is retained. The remaining runtime failure is not falsely attributed to the idealized step sequence above.
