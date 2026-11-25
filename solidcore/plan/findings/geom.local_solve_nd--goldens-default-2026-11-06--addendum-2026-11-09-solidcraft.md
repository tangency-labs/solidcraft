# geom.local_solve_nd: SolidCraft integration and fresh controls (2026-11-09)

Updates: `geom.local_solve_nd--goldens-default-2026-11-06.md`.

Also follows the November 7 rebase and geometric-review addenda.
Unqualified issue numbers in those historical records refer to `tangency-labs/solidcore`.

Issue: tangency-labs/solidcraft#13764, formerly tangency-labs/solidcore#2782.
This change migrates tangency-labs/solidcore#2791 into SolidCraft's `solidcore/` tree.
The ten original commits retain their order and authors.
The integration base is SolidCraft main `f9ed9a90b453517d83e787169887c180e17656fd`.
Its kernel tree equals kernel-branch commit `8ffb97d541bcc971a9b4a373f3b6a64d1cc75daa`.
Current contribution policy sends this self-contained kernel change to `main`.

## Integration boundary

The solver correction and geometric proofs are unchanged from the original PR.
The app's adapter already enabled `oscillating_skips_failed_guesses`.
Its assignment is removed because the corrected kernel no longer exposes that flag.
The existing adapter regression still passes.

Conflicts in `blend.fillet_executive_p3.jsonl` and `known_failing.json` were resolved per case.
All 135 known failures on the new base retain their classifications and reasons.
The five additional known failures remain executed, bringing the list to 140.
Their references now point to SolidCraft #13733 and #13684.
No failing case is blessed, no tolerance changes, and no C++ is built or run.

## Fresh measurements

The original November 7 record remains a historical measurement.
Fresh controls use the same Rust 1.98.0 toolchain and fixture pin on both sides.
The before side is unmodified main `f9ed9a90b453517d83e787169887c180e17656fd`.
The measured after side is imported change `03c8ff2d4afbd9af213b60952eaba4b1bd2a9e4f`.
Message-only import cleanup publishes the identical tree as `c0cb7f90003463b1451479b6d87eba71f8f2c52a`.
The adapter assignment and issue-reference integration edits do not change those observations.

All 18 reviewed caller inputs match the historical inputs exactly.
Every before observation matches its historical before observation exactly.
Every after observation matches its historical corrected observation exactly.
The comparison includes floating-point signs and every output-array element.
All 11,903 before values and 11,973 after values are finite.
Per-case results are in [comparison.json](geom.local_solve_nd--solidcraft-integration-2026-11-09/comparison.json).
The eleven imported geometric review regressions pass against the integrated kernel.
Therefore the November 7 geometric arguments and same-view figures still apply.
These comparisons establish continuity of the evidence; the geometric tests establish correctness.

The exact golden comparator still measures 1,394 serialized updates across 31 units.
Of those, 1,328 changes across 30 units exceed the unchanged comparator's thresholds.
Five lines leave the expected files because their cases are executed as known failures.

The open-shell control uses `Fillet/open_shell.smb`, owning-list edge 5 and radius 0.5.
Both sides return status 0 with 18 vertices, 24 edges and seven faces.
Both fail Level-2 validity with the same nine reports.
The maximum reported gap is `3.019034068882236e-5` on both sides.
This also matches the historical control; #13733 still tracks the defect.
The four independently checked nonmaxima remain tracked by #13684.
Their counterexamples and domain checks still pass in the geometric review tests.

## Fixture provenance

The required upstream fixture pin is `3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`.
Only the permitted `TestFiles/pt_TestFiles/` data was checked out for these controls.
No upstream implementation was compiled or executed.

The first complete golden run exposed two local setup mistakes.
An older fixture checkout had `Platform.smb` and `SmallBox.smb` instead of the pinned `.smp` files.
Also, `curve.crv_on_surf` resolves fixture paths from the kernel workspace directory.
That first run used the monorepo root as its working directory.
After correcting the pin and working directory, all four affected units pass unchanged.
All common fixture files have identical bytes between the two fixture checkouts.
The 279 unaffected passing units plus those four reruns cover all 283 units.

## Validation

- Kernel workspace nextest: 6,930 passed, zero failed, 108 skipped.
- All 283 golden units pass: 592,395 cases, including 140 known failures.
  This includes the four setup-correction reruns above.
- Kernel workspace doctests, fmt, strict Clippy and wasm build pass.
- Record identifiers, frozen archives and the panic-site ratchet pass.
- The panic check uses a synthetic base containing main's kernel subtree at repository root.
  This reproduces CI's prefix handling without changing any branch or index.
- The existing app adapter regression passes.

## Application baseline failures

The full application gate reports the failures listed below.
Each minimized reproduction was built separately against clean main and this migration.
No application baseline, assertion or expected result was changed.

| Test | Main and migrated result |
|---|---|
| `boolean_props::commutative` | Both intersection orders refuse the result with `SmFace::Test #29 Face crosses Seam`; both union orders build valid bodies. |
| `action_matrix_sample`, `corpus/issue-616-lid-ribs`, `Lid/F8:-x`, `face.shell` | The same regenerated command fails with the same `SmLoop::Test #2 Bad Loop Orient` message. |
| `naming_corpus`, `oracle/83-boundary-loop-thicken` | Identical generated scripts fail at step 8, `surface.thicken`, with the same offset-regularity refusal. |
| App kernel library, eleven failed regressions | All eleven fail against clean main with identical diagnostics and numerical observations. |
| `pleat_regressions`, two failed regressions | Both fixture replays stop at the same six feature errors, starting with `CaseShell`'s uncertified extension bound. |
| SMLib adapter, `a_variable_round_with_a_corner_in_its_law_is_its_balls_union` | The unchanged test fails on both sides with `Err(Unsupported)`. |
| Engine library, twelve failed regressions | Every failure reproduces on main with identical diagnostics and numerical observations, including the complete mirrored-mesh triangle arrays. |

The boolean input is a cylinder at `(7.5, 15, 5)`, axis +X, radius 15, height 20,
and a sphere at `(5, 0, 2.5)`, radius 3.75.
Its proptest seed is `79bd104b4d427c2df50620c3f0c8049e7994fb515719c1162b3dec7be907687f`.
`boolean_props` is already quarantined from PR CI in `.config/nextest.toml`.
The temporary seed appended by the local property run is not part of this change.

The action worker independently rebuilds the corpus on each side.
It regenerates `solid.shell` with face pick `(102.79999999999998, 16, 8.299999999999999)`
and thickness `0.5`; only execution time differs between the two result rows.
The naming control converts `tests/oracle/83-boundary-loop-thicken/recipe.json`
with the CLI's `recipe` command, then executes the generated script with `run --quiet`.
Both latter failures are already named in [the open construction hub #13827](https://github.com/tangency-labs/solidcraft/issues/13827).

The eleven app-kernel checks cover consecutive rounds, a variable-radius corner,
five G2 corner tests, the least-setback refusal, the wire-record fixture and smooth-edge drawing.
Their unchanged test bodies, helpers and assertions were compiled against each side's library.
Only paths to public reexports change in that temporary harness.
All eleven fail on both sides with identical observations after removing OS thread identifiers.
The long-thread regression itself passes in the full application run.

The pleat controls replay `battery_case_pleats.json`, first unchanged, then with `wall = 3 mm`.
Both timelines have identical errors on both sides: the shell's missing extension bound,
an ambiguous selection point and four missing downstream fillet references.
The original assertions fail before constructing the far-end pleat.
This establishes those assertion failures, not a claim about the unreached construction.

The engine controls use the unchanged tests and their helpers against each side's library.
They cover variable-fillet edits, torus offset, curved-wall cut extent, face naming,
hole body count, boss radius measurement, chord and variable fillets, taper fitting,
overlapping-cylinder fillets, mirrored-mesh caching, the saved pleat and a rim-cut face count.
The saved pleat fails at its initial 2.5 mm assertion; later 2.4/2.6 mm assertions remain unreached.
The curved-wall cut removes zero volume on both sides; its reference cut measures `48.20229278227089`.
No assertion, reference value or application baseline is changed to obtain these controls.

App wasm checks, doctests and browser offline launch/update tests pass.
All 249 graphics tests and all three command-atomicity tests pass as well.

## Extended rig deadline

The stock ignored-test run finishes with two tests passing and `kernel_cases` failing.
The corpus grades 370 cases: 290 pass, 69 are unsupported, ten are known failures,
and `fillets/cov-fillet-section-curvature-boss-root-r2` reaches the unchanged 180-second deadline.
The harness then leaves 711 cases unrun; no listed case changes classification.
The two other ignored tests, turn replay and the captured unions, both pass.
The whole ignored-test process exits 101 after 743.10 seconds.
This is not a passing full-rig result.

Separate, serial probes run the original boss-case harness against clean main and this migration.
Both reach the same 180-second deadline with identical one-case failure tables.
Measured wall times are 180.015 seconds before and 180.018 seconds after.
The UI and engine suites remain active during both probes.
Load changes from 6.59 to 5.25 before, and from 4.81 to 5.15 after.
These are observations under concurrent load, not quiet-machine timings.
Neither probe produces a volume or a physical-validity result.
The fixture JSON and compressed body records are byte-identical on both sides.
The only difference in the fixes list is retirement of the N-D switch.

The full application gate completes all 97 native groups in 5,353.0 seconds, after the initial build.
It reports 14,385 passing tests, 29 failing tests and 67 ignored tests, and exits 1.
All 29 failures across seven groups have the clean-main controls described above.
The application gate is not green.
This migration is prepared for a draft PR, not reported as ready to merge.

## Final rig run without concurrent application work

A final stock run starts after all application suites and baseline probes have exited.
The initial load is 0.42; no other build or geometry probe runs alongside it.
It preserves the original 180-second per-case deadline and executes all three ignored tests.
All three pass, with process exit 0 after 1,629.52 seconds.

The corpus grades all 1,081 cases: 803 pass, 263 are unsupported and 15 are known failures.
There are zero unexpected failures, zero changed known-failure classifications and zero unrun cases.
The earlier boss-case deadline does not recur in this run.
The turn replay and all captured unions also pass.
This completes the extended rig check without changing a bound, fixture, expected result or classification.
