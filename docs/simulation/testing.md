# Simulation testing and verification

Part of the [simulation plan](README.md) and [RFC 0010](../rfcs/0010-simulation.md).

The rule that keeps this honest: **a solver is not done when it produces a picture. It is done when
it reproduces a known answer within a stated tolerance, converges at the expected rate, and refuses
correctly when it cannot.**

Terminology used here:

- **Code verification:** does the implementation solve the equations it claims? (patch test,
  convergence order, manufactured solutions)
- **Solution verification:** is this particular mesh good enough? (refinement, error estimate,
  quality report)
- **Validation:** does the model match reality? Only where public, cited reference data exists; we
  do not invent it.

## 1. Test pyramid

| Level | What | Where |
|---|---|---|
| 1. Unit | Element stiffness symmetry, rigid-body null space, material mapping, units, field math | `crates/fea/src/*_tests.rs` |
| 2. Solver | CG residual, preconditioner SPD, Lanczos orthogonality, condition warnings | `crates/fea/src/solver_tests.rs` |
| 3. Physics | Benchmark catalogue (below), convergence studies | `crates/fea/tests/benchmarks/` |
| 4. Command | `sim.*` input/output schemas, refusals, undo, save/reopen | `crates/engine/src/sim_tests.rs` |
| 5. Cross-backend | CPU f64 vs GPU f32 on the same problem | `crates/fea/src/backend_tests.rs` + `crates/sim-gpu/src/tests.rs` |
| 6. UI | Workspace, result fields, probe, progress, cancel | `crates/ui-egui/src/sim_*_tests.rs`, scenarios |
| 7. Robustness | Caps, fuzz, hostile JSON, OOM, device loss, sleep/wake | `crates/fea/src/fuzz_tests.rs`, engine |
| 8. Platform | Build and run matrix | `cargo xtask wasm`, CI, manual checklist |
| 9. Performance | Budgets, regressions, memory | `docs/perf.md` entries + bot report |

Repo conventions apply: tests live in the area's existing file, never a per-bug file (#3226); no
wall-clock assertions; deterministic seeds; `cargo xtask test` locally with the `ci` profile.

## 2. Benchmark catalogue

Every entry stores: problem definition, reference value, source, tolerance, mesh used, expected
convergence order, and the measured result in the PR. Values are compared, never auto-accepted into
a golden file.

### Structural

| Case | Reference | Tolerance | Checks |
|---|---|---|---|
| Patch test | Constant strain reproduced exactly | 1e-10 relative (f64) | Element completeness |
| Rigid-body motion | Zero strain energy and stress | 1e-12 relative | No spurious stiffness |
| Cantilever, end load | Euler–Bernoulli / Timoshenko tip deflection | 2% with documented mesh | Bending, shear |
| Plate with hole | Kirsch stress concentration (with finite-width note) | 3% at the hole | Stress recovery |
| Pressurised thick cylinder | Lamé solution | 2% through thickness | Curved geometry, radial stress |
| Curved beam | Curved-beam theory | 3% | Non-prismatic geometry |
| L-bracket | No closed form: refinement only | Change < 5% between two refinements | Convergence, singularity warning |
| Reaction equilibrium | Sum of reactions = applied load | 1e-8 relative | Assembly, load transfer |
| Superposition | Two loads = sum of separate solves | 1e-8 relative | Linearity |
| Unit scaling | Same model in mm and m (scaled) | 1e-8 relative | SI boundary |
| Mesh permutation | Same mesh, reordered nodes/elements | 1e-10 relative | Determinism, ordering |

### Modal and buckling

| Case | Reference | Tolerance |
|---|---|---|
| Cantilever frequencies | `1.875²`, `4.694²`, … | 2% |
| Free–free | Six rigid-body modes at zero | 1e-8 relative |
| Simply supported plate | Plate theory | 3% |
| Euler column | `π²EI/(KL)²` | 2% |
| Mode orthogonality | `φᵢᵀMφⱼ = δᵢⱼ` | 1e-8 |
| Participation mass | Sum approaches total in the solved directions | 1% |

### Thermal and fatigue

| Case | Reference | Tolerance |
|---|---|---|
| 1D wall | Linear profile | 1e-8 |
| Fin | Analytical fin efficiency | 2% |
| Thick cylinder | Logarithmic profile | 2% |
| Constrained bar | `σ = EαΔT` | 1% |
| Bimetallic strip | Curvature by Timoshenko | 3% |
| Fatigue life | Hand calculation from the S-N curve | 1% |
| Missing S-N | Refusal naming the missing data | exact |

### Topology

- Compliance decreases monotonically per iteration (allow a documented tolerance for projection).
- Volume fraction met within 1%.
- Output body is watertight and has no disconnected islands below the minimum member size.
- Same seed and inputs produce the same density field (deterministic).
- Symmetry and draw direction are honoured (measurable projection checks).

### Flow

| Case | Reference | Tolerance |
|---|---|---|
| Poiseuille channel | Parabolic profile | 2% |
| Lid-driven cavity Re 100 | Ghia et al. centreline velocities (cited) | 5% |
| Lid-driven cavity Re 1000 | Ghia et al. (cited) | 8%, with resolution note |
| Cylinder Re 100 | Drag coefficient from literature (cited) | 10% |
| Mass conservation | Inflow = outflow | 1e-3 relative |
| Symmetry | Symmetric case stays symmetric | 1e-6 relative |

Flow tolerances are deliberately loose and always reported with the resolution and Reynolds number.
A result outside the documented validity range is a warning, not a hidden pass.

## 3. Cross-backend testing

- The CPU f64 result is the reference.
- GPU tests run only when a compute-capable adapter exists; otherwise they skip cleanly. Reuse the
  existing pattern: `SOLIDCRAFT_GPU` policy, `offscreen_adapter`, and a shared device where
  possible.
- Record: backend, adapter name, driver, precision mode, residual, iterations, peak memory.
- Tolerances: displacement 1e-4 relative; stress 1e-3 relative typical, 1e-2 at a peak with a
  documented reason; modal frequency 1e-4; thermal 1e-5; flow 1e-4 between CPU and GPU.
- A result that misses tolerance is a bug or a documented precision limitation, never a silently
  relaxed threshold.
- Same-device determinism is required (bitwise). Cross-vendor bitwise equality is not required and
  must not be asserted.

## 4. Robustness and hostile input

- NaN/Inf in any input refuses with the field name.
- Zero or negative thickness, stiffness, density, conductivity refuses.
- Ridiculous magnitudes (1e300) refuse before they propagate.
- Study JSON size caps, array caps, string caps.
- Degenerate geometry (zero-volume body, open shell, self-intersection, duplicate faces) refuses
  with the body/face named.
- Mesher fuzz corpus: every case either meshes or refuses within a time and memory cap. No hang, no
  OOM-kill, no panic.
- Solver: non-convergence returns an error with the residual history, never a result.
- Device loss, OOM, cancel at every stage, sleep/wake, and closing the window mid-run.
- A cancelled run leaves no partial result applied; a failed run leaves the previous valid result
  marked out of date, not deleted.

## 5. Performance and memory

- Budgets are in [requirements.md](requirements.md) §3.2. Measure with release builds, on a quiet machine, and
  report medians with range and machine, following `docs/perf.md`.
- CI does not fail on wall-clock time (flaky, thermal throttling, shared runners). It records and
  warns on a >2× regression for the same machine class.
- Memory: preflight estimate vs measured peak within 25%; tests assert the refusal path above the
  cap.
- No per-iteration heap allocation in hot loops (allocation counter in tests where practical).
- GPU: dispatch batching and progress cadence tested with a forced slow kernel.

## 6. CI matrix

| Job | Runs | Gates |
|---|---|---|
| Linux CPU | All unit, solver, physics, command tests in f64 | Required |
| Linux wasm | `cargo xtask wasm`, `web-offline-test` | Required |
| macOS CPU | Full suite, Apple Silicon | Required |
| macOS Metal | GPU kernels where the runner exposes an adapter | Optional, recorded |
| Windows CPU | Full suite | Required |
| Windows Vulkan/DX12 | GPU kernels where available | Optional, recorded |
| Linux software adapter | Shader compilation and a smoke solve on lavapipe | Optional |
| Self-test | `sim.selftest --json` artefacts uploaded | Required where a GPU exists |

GPU jobs that cannot run must report **skipped with the reason**, never a green pass. A skipped GPU
job is visible in the PR evidence.

## 7. Manual acceptance checklist (per platform)

Run before claiming platform support in a PR:

1. Create a static study on a bracket, mesh, run, probe, change a load, re-run.
2. Force CPU, then Auto, compare timings and agreement.
3. Cancel mid-run and mid-mesh.
4. Sleep the machine during a run; wake it; the app survives and the run fails cleanly.
5. Fill memory near the budget; the preflight refuses or the run fails with a named error.
6. Save, close, reopen; the study is intact and marked "not run in this session".
7. Undo a load edit; redo; the result state is consistent.
8. Switch configurations; studies invalidate only where their inputs changed.
9. Export VTK/CSV; open the files in an independent viewer.
10. Screenshot or record the observable change.

## 8. Evidence required in every solver PR

- The benchmark table with measured values and tolerances.
- The convergence study (two or three refinements) for at least one case.
- The CPU/GPU comparison, or "no GPU available; skipped" plus a self-test from a machine that has one.
- The refusal tests for the new failure modes.
- A note on what was not verified.
- `cargo xtask ci` green.

## 9. What we will not do to make tests pass

- Loosen a tolerance without a stated numerical reason and a linked issue.
- Compare against a previous run of the same code instead of an external reference.
- Mark a GPU test as passed when no adapter was used.
- Hide a non-converged result behind a warning while still showing it as a valid result.
- Auto-update golden benchmark values.
