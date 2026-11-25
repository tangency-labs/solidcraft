# Quadric reach: cancellation, branch choice and operation checks

Addendum to `offset.offset_executive--quadric-pair-reach.md`.

The edge-piece callback now returns `Result<PieceBound>`. Cancellation from either
quadric solve propagates through subdivision and leaves `EdgeBounds` untouched.
An ordinary unproved piece remains `Unknown`; an ordinary unproved edge can still
be cached. The regression cancels on Newton's second poll, clears cancellation,
and requests the same edge and surface bits with the same cache. It must solve
again, then reuse the successfully certified result.

A certified normal-section meeting bounds a root's distance but does not identify
which intersection component lies on a curved face's retained side. In particular,
a radius-3 upper hemisphere tangent to a cylinder at z = 0, with that cylinder
replaced by r(z) = 2.99 - 0.05z, has sphere/cone intersections at
z = (0.299 ± sqrt(0.3296))/2.005. The negative circle is nearer the old edge but
belongs to the hemisphere's continuation. A nearest-midpoint choice is unsound.
The existing plane/curved side construction is retained. A tangent curved/curved
pair marked as crossing without a unique-component proof now returns `TangentBranchUncertified` before making
intersection curves. General support needs a certificate of the retained branch
and that branch's identity carried into edge mating; a distance alone cannot
supply it. The synthetic sphere/cylinder operation checks this typed refusal.
`edgeuse_offset` passes it on rather than swallowing it as a soft mating failure:
allowing merge after an uncertified branch discards the refusal's guarantee.
The cone is obtained by drafting the original cylinder about z = -0.2 by
atan(0.05), preserving the original seam frame and UV period. An independently
constructed cone has a different period and tests seam refusal instead.

The shared cone polynomial remains v.v - z² - r(z)², where z = axis.v. It is
evaluated as |v-axis*z|² + (1-axis.axis)z² - r(z)². Exact expansion products and
subtraction enclose the small constant norm defect; products outside expansion
range retain their outward interval enclosure. The gradient is 2(v-axis*z) minus
2*axis*r(z)*slope. Cardinal unit axes have exactly zero defect, so the value and
gradient enclosures use the previous projected net-corner operations bit for bit.
Tests compare both endpoints' bits for boxes at heights through 100 million,
multiple slopes and cardinal axes. A separate plane/quadric/net corner solve at
heights 0, 1000 and ±1 million still certifies the closed-form meeting. This avoids
the width from subtracting two large axial squares. Rounded non-cardinal axes
retain the norm correction required by the canonical polynomial; the rotated,
translated corner regression checks that path.

Four operation regressions draft the full and half spherical-bored slab at 1 and
5 degrees, then require a closed manifold solid and its independently expected
volume. With sphere radius R, bore radius a and slope t, let B = 1+t². The two
cap intersections are z = (-at ± sqrt(R²B-a²))/B. Integrating
pi*(R²-z²-(a+tz)²) between them gives

    V(t) = (4*pi/3)*(R²B-a²)^(3/2)/B².

Both bores lie strictly inside the slab's square sides, since their radii are at
most 1+3*tan(5 degrees) < 2. Exterior clipping therefore cancels from V(t)-V(0).
Reflection halves the half slab's volume change. The test compares measured
post-operation volume with measured baseline plus that exact difference, to
1e-5 absolute volume, below the smallest tested change. No attachment data is
used in any regression.

The split-cylinder draft is a distinct, valid construction, despite the old
operation test expecting the former missing-bound refusal. Each half is a
frustum; the new surfaces are exactly coaxial cones (including the unchanged
cylinder on the first call). For a common exact unit axis and exact common axis
line, positive-nappe radii are affine in a shared axial coordinate. Unequal
signed slopes therefore have at most one physical intersection circle. Reach
already certifies a regular meeting, so no competing continuation circle exists.
`unique_cone_meeting` verifies these hypotheses using analytic-class checks,
exact unit norm, exact equal/opposite axes and expansion cross products for the
axis lines (no rounded subtraction of their origins). It permits that proved
single-component case and retains the typed refusal otherwise. A transverse
axis displacement below body tolerance is deliberately not accepted as exact.

The former both-ways cylinder refusal test is replaced with success, closed
manifold, outward-solid and full validity checks after each draft. With
r = 10 - 10*tan(3 degrees), each half-frustum volume is
pi*10*(100+10*r+r²)/3. The first operation retains a radius-10 cylinder in the
other half; the second gives two such frusta. This does not introduce a general
parting-line operation or infer a new unsupported-input rule from missing bounds.

The synthetic slab now revolves a closed meridian (the radius-3 spherical arc
closed by its radius-1 bore line), then intersects the same square slab. The two
profile endpoints are reused for the bore, so its paired seam trims share their
parameters by construction. This keeps the original synthetic geometry. The
previous sphere/slab intersection followed by bore subtraction produced a high
seam trim whose u coordinate drifted from 2*pi to 2*pi - 8.506124e-9. That trim
fails the existing seam certificate even on the original surface, before drafting;
it did not exercise reach. The seam certificate and its tolerances are unchanged.

The exact-axis proof exposed a second construction defect in drafting itself.
Tracing the split-cylinder case gives an unchanged cylinder with origin (0,0,0)
and a drafted cone with fitted origin
(-4.163336342344337e-15, -2.220446049250313e-16, 0). Both axes have exactly unit
squared length and both analytic labels pass their existing checks, but their
axis lines differ exactly. The fitted angular frame also turns by about 2.1e-17.
Those differences cannot be treated as exact coaxiality by a tolerance test.

`Cone::drafted_about_plane` now keeps its source frame at the positive neutral
circle, with radius `d.r0` and signed source-frame slope `d.sense*d.turned`.
`AnalyticSupport::build` places that support on the radially edited form, checking
both `lies_on` and `evaluates_as`. Its existing origin conversion shifts the
analytic height range and generator parameters together, preserving the NURBS
parameters. SSI and the uniqueness proof therefore use the same intended
support; no fitted label is silently accepted as coaxial. Failed certification
propagates. The draft definition is attached and re-certified as before.

The existing draft test module gains exact source-axis and angular-frame checks
for 3 and 5 degrees with both pull directions. The existing cone test also
checks a draft that cancels its slope: it remains a correctly certified
zero-angle cone, without retaining the old semi-angle from reconstruction.

## Changed files

- `solidcore/crates/solidcore/src/offset/offset_executive/reach.rs`: fallible
  piece evaluation, cancellation/cache regression, tangent-branch operation and
  full/half slab operation-volume regressions.
- `solidcore/crates/solidcore/src/offset/offset_executive/reach/quadric.rs`:
  stable canonical cone value and gradient with an enclosed norm defect.
- `solidcore/crates/solidcore/src/offset/offset_executive/reach/net.rs`: unchanged
  cardinal enclosure endpoint bits and large-height corner certification tests.
- `solidcore/crates/solidcore/src/offset/offset_geometry_creation.rs`: typed
  curved-crossing refusal, exact coaxial uniqueness proof and error propagation.
- `solidcore/crates/solidcore/src/error/offset.rs`: the branch-refusal variant,
  classification and explanatory text.
- `solidcore/crates/solidcore/src/offset/draft.rs`: source-axis preservation on
  the certified edited net and native surface regressions.
- `solidcore/crates/solidcore-ops/src/offset/tests.rs`: split-cylinder success,
  full validity and closed-form volume regression.
- `COMMIT_MSG.txt` and `PR-BODY.md`: follow-up description and measured evidence.

## Validation run record

All compilation, targeted tests, operation measurements and clippy checks ran
through the build queue. No workspace or full-crate test suite was run.

- The first reach/net attempt did not compile: the new cache test supplied face
  IDs where its `Meeting` key takes optional face IDs. Corrected before execution.
- The first executed reach/net run passed 49 tests and failed 3: the two full
  operations stopped at the fixture's seam certificate, and the tangent example
  used a cone with a different seam period. Geometry creation passed 9 tests and
  offset error classification passed 3.
- The split-cylinder test passed alone and all 11 draft operation tests passed
  before the typed mating refusal was propagated. Both frustum stages were
  closed and passed full validity and volume checks; this also exposed why a
  swallowed refusal could not count as a successful branch certificate.
- Initial clippy runs for both affected crates failed on style warnings in the
  new code. These were corrected; both crates subsequently passed all-target
  clippy with warnings denied.
- Seam diagnostics reproduced refusal of the full fixture even on its original
  surface. The corrected tangent fixture reached final validity rather than the
  typed refusal, exposing its soft-error swallowing. The revolved fixture trial
  passed 51 reach/net tests, including all four operation volumes, and failed
  only the not-yet-propagated tangent refusal.
- With the refusal propagated, all 52 reach/net tests, 9 geometry creation tests
  and 3 offset error tests passed. The complete offset operation file passed 25
  tests and failed only the split-cylinder success case. Both crate clippy
  checks and formatting passed.
- The first worker-only regression checks all failed: cancellation and the
  expanded cone demonstrated their regressions, while the curved-branch case
  still stopped at the incorrectly constructed fixture seam and did not yet
  prove branch selection.
- After correcting that fixture, temporary worker-only regressions each failed
  as expected with its fix
  disabled: cancellation returned cached `None`; removing the tangent guard
  returned success on the wrong branch; the expanded cone polynomial lost the
  large-height corner's sharp enclosure. Sources were restored after each.
- Exact-axis diagnostics reproduced the split-cylinder refusal and recorded
  the fitted origin above. The new native axis regression failed against the
  old draft construction, on its changed angular frame.

All kernel test commands use `cargo test --jobs 2 --manifest-path
solidcore/Cargo.toml -p <crate> --lib <filter> -- --nocapture`, inside a queued
job. Filters exercised are `offset::draft::tests`,
`offset::offset_executive::reach` (including net),
`offset::offset_geometry_creation::tests`, `error::offset::tests`, and in
`solidcore-ops`, the split-cylinder test, `offset::tests::drafts` and the complete
changed `offset::tests` file. Both clippy commands use `cargo clippy --jobs 2
--manifest-path solidcore/Cargo.toml -p <crate> --all-targets -- -D warnings`.
Golden commands use `cargo run --jobs 2 --manifest-path solidcore/Cargo.toml
--profile goldens -p goldens -- --unit <unit> --goldens`. Formatting checks
cover both affected crates. The application measurement builds only
`solidcraft-cli` and evaluates the original design separately for each angle.

The final source-axis construction batch passes all 16 native draft-surface
tests, the split-cylinder test alone, all 52 reach/net tests, 9 geometry creation
tests, 3 offset error tests and all 26 offset operation tests (including all 11
drafts). The two frustum stages measure 6121.417619712363 and
5959.649932288919 against 6121.417619752132 and 5959.649932324678 expected.
Both stages pass closed-manifold, outward-solid and full validity checks.

Both affected crates pass all-target clippy with warnings denied after the final
source-axis changes. All four affected golden units were attempted in each of
three complete check batches, with the same result each time:

- `offset.offset_executive_p1`: PASS, 1060 cases, changed/added/removed/broken all
  zero, 95 existing known failures.
- `offset.offset_geometry_creation`: BLOCKED, 926 expected cases, 90 available
  unchanged, 836 missing/broken due absent reference files including
  `Solids/box.smb`, `Solids/cylinder.smb`, `Fillet/nurb_box.smb` and
  `ellips_ts2.smb`. The command exits 1; it is not counted as a passing unit.
- `offset.offset_executive_p2`: BLOCKED on missing `Defeature/extrusion.smb`,
  case `extended_inset_round_drop`; command exits 1.
- `topology.brep_local_operation`: BLOCKED because `gen0014` is registered in
  `known_failing.json` but missing from fixture-dependent generated cases;
  command exits 1.

These last three units need CI with the complete reference fixtures. The pinned
fixtures cannot be recovered from the available checkout or local objects, and
network access is unavailable. No generated data, golden expectations or
geometric tolerances were changed. The full application suite and remaining CI
gates were deliberately not run locally, following the lane's test limits.

The reporter design was measured in three separate completed batches. The
original Body1 volume is 310436.73286083934 mm³. With the reported three faces
and neutral plane, the 1-degree draft measures 311637.96578734944 mm³
(+1201.23292651010 mm³), and the 5-degree draft measures
316448.44046827446 mm³ (+6011.70760743512 mm³). The final source-axis correction
leaves those measurements unchanged. Both results are closed, have an active
unsuppressed Draft feature, and have no timeline feature errors. The original
0.41.0 reproduction refused both angles with an uncertified reach; its failed
preview is not a successful result. The measured increases agree with the
independent first-order estimates of roughly 1201 and 6021 mm³.

The four synthetic operation volumes in the final run are:

| Case | Expected | Measured |
| --- | --- | --- |
| Full, 1 degree | 61.386829656652 | 61.386829652559 |
| Full, 5 degrees | 61.169693881995 | 61.169693872795 |
| Half, 1 degree | 30.693414828326 | 30.693414826282 |
| Half, 5 degrees | 30.584846940998 | 30.584846936407 |

The final formatting check passes. The final check batch exits 1 solely because
of the three fixture-dependent golden units above; every targeted test,
clippy, formatting and reporter measurement command passes. Hashes of all seven
changed Rust files match the sources checked in that batch. Temporary
reproduction inputs and diagnostic scripts are excluded from the change.
