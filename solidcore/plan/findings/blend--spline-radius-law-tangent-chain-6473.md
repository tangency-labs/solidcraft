# Spline radius laws do not span tangent chains

Issue: SolidCraft #6473.

The synthetic reproduction is a 100 x 60 x 30 box starting at the origin. Round
the vertical edge at (100, 0, 15) at R10, then ask for a spline fillet at
(50, 0, 30), starting at (0, 0, 30), with five control radii of 2 and knots
`[0, 0, 0, 0, 0.5, 1, 1, 1, 1]`. The top tangent chain contains a straight
edge, the R10 quarter arc, and another straight edge.

The regression is `solidcraft-engine::tests::spline_fillet_follows_a_rounded_corner_chain`
in `crates/engine/src/tests.rs`, tracked in `tests/KNOWN_FAILING_TESTS`.
Its reference is the same selection rounded with an ordinary R2 fillet.
This equivalence is exact: the B-spline basis sums to one, so five control
radii of 2 give radius 2 everywhere, regardless of the interior knot.
No golden or kernel-case expectation is changed.

## Existing contract and missing construction

- `crates/doc/src/document.rs`, `FilletStyle::Spline`, defines the law over
  each individual edge, from its end nearest `start` to its other end.
- `crates/doc/src/eval.rs`, `blend_raw`, calls `one_by_one` for spline and
  variable fillets. Even explicitly picking all three edges runs independent
  operations against successively changed bodies.
- The fillet evaluation's `chain` match explicitly excludes both styles,
  even when `tangent_chain` is true.
- `solidcore/crates/solidcore-ops/src/blend.rs`, `fillet_variable_impl`, selects one edge and
  supplies one law to a pass with `edges: &[e]`.
- `solidcore/crates/solidcore/src/blend/fillet_executive/session.rs`,
  `prepare_variable_radius_edges`, can prepare multiple edges, but assigns
  the same complete law to every edge's solver. It does not partition a law
  over an oriented chain.
- `FilletExecutive::law_sequence_edges` mentions `CreateSequenceOfLaws`, but there
  is no production construction that populates a chain law sequence.

A complete fix needs a chain radius-law construction and an explicit contract
for the chain's parameter and orientation. Merely enabling
propagation still runs the independent operations. Repeating the full law on
each edge can make the radius discontinuous at their joins when its endpoint
radii differ. Replacing
only constant spline laws with ordinary rounds would leave the reported spline
chain capability absent.

No geometry workaround is implemented. The requested whole-chain behavior is
a feature gap requiring that construction and contract. The single-edge corner
failure may also need a solver or end-construction fix, but making that edge
build would still leave the rest of the chain sharp.

## Worktree regression

`buildq run solidcraft . -- cargo test -p solidcraft-engine --lib tests::spline_fillet_follows_a_rounded_corner_chain -- --exact --nocapture`
compiled successfully and ran one test. The constant-radius control succeeded;
the spline operation failed with status 1001 and
`Fillet Corner Error: Unable to Compute Geometry`, naming the unblended tangent
continuation at (97.071, 2.929, 30.000). Result: 0 passed, 1 failed, 6157 filtered
out; cargo exit 101. Thus the failure remains in the current source, not only
in the installed 0.44.1 CLI.
The error recommends `tangent_chain: true` even though the test already gives
that value: the evaluation's style filter prevents that advice from helping.

Additional checks:

- `buildq run solidcraft . -- cargo test -p solidcraft-engine --lib tests::spline_fillet -- --nocapture`:
  `spline_fillet_takes_over_a_taper` passed;
  `spline_fillet_follows_a_rounded_corner_chain` failed with the same status 1001
  corner error. Result: 1 passed, 1 failed, 6156 filtered out; cargo exit 101.
- `buildq run solidcraft . -- cargo clippy -p solidcraft-engine --all-targets -- -D warnings`: passed, exit 0.
- `buildq run solidcraft . -- cargo fmt --all --check`: passed, exit 0.
- `buildq run solidcraft . -- cargo fmt --manifest-path solidcore/Cargo.toml --all --check`: passed, exit 0.
- `git diff --check`: passed. The known-failure list is sorted; the commit message
  has no attribution trailers or tool, model, or session lines.

## Installed CLI reproduction

The installed `solidcraft-cli` is version 0.44.1, not the worktree build.
Running the three-step spline reproduction fails at step 3 with status 1001,
`Fillet Corner Error: Unable to Compute Geometry`, and the unblended tangent
continuation is reported at (97.071, 2.929, 30.000).
An ordinary R2 fillet of the same selection succeeds. A variable R2-to-R1
fillet reports success, but its snapshot shows the arc and the second straight
edge still sharp: it rounds only the selected straight edge. Thus the triage's
successful variable fillet does not demonstrate support for a chain law.
Snapshots captured under `/tmp/solidcraft-6473/` show the input and the ordinary
R2 reference from the same view; they are not evidence of a fixed spline fillet.
