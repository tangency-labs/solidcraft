# Simulation roadmap

Part of the [simulation plan](README.md) and [RFC 0010](../rfcs/0010-simulation.md).

No dates: SolidCraft releases daily and features land when they are finished. Milestones are
ordered by dependency and by how much each one unlocks. Every milestone is a set of small PRs
(one topic each), not a branch.

## Dependency graph

```
M0 foundation (RFC, studies, commands, results viewer, dynamics UI, device survey)
 ├── M1 mesher ──► M2 static ──► M3 real parts ──┬──► M4 shells (sheet metal)
 │                                               ├──► M5 modal + buckling
 │                                               ├──► M6 thermal + thermal stress
 │                                               └──► M7 fatigue
 ├── M8 topology (needs M2 + M3)
 ├── M9 flow (needs M0; independent of FEA)
 └── M10 parameter optimisation + nonlinear research (needs M2, M4)

G  GPU track: runs beside M2, M5, M8, M9 (one kernel set at a time)
P  platform track: macOS is the baseline; Windows/Linux/wasm validation beside each milestone
```

## M0 — Foundation

**Goal:** a real Simulation workspace and the contracts every later milestone depends on.

Slices:

1. RFC 0010 (scope, data model, commands, benchmark gates, non-claims) + ROADMAP milestone +
   CONTRIBUTING note.
2. `sim.study.*` commands, `Study` model, save/reopen with one `format/pending` file.
3. Broken-reference behaviour: follow renames, split warns, missing refuses by name.
4. Results viewer: field data, palette, legend, probe, deformation scale (fed by dynamics first).
5. Simulation workspace tabs and ribbon sections.
6. Dynamics front end: mass/inertia, joint reactions, contacts, playback over `crates/dynamics`.
7. `fea::Backend` trait, `BackendInfo`, `Control`, caps, `sim.devices`, `sim.selftest` on CPU.
8. Preflight: counts, memory estimate, missing material data, unbalanced load, unconstrained bodies.

**Exit:** a study can be created, edited, saved, reopened, marked out of date and deleted; dynamics
results play in the workspace; `sim.devices` and `sim.selftest` work on a machine with no GPU;
`cargo xtask ci` green. **Evidence:** screenshots and a short recording.

## M1 — Mesher

**Goal:** a trustworthy mesh for well-behaved solids, with honest refusals.

Slices:

1. Analysis tessellation request at study tolerance (never the display mesh).
2. 2D constrained Delaunay core (planar faces, shell mid-surfaces later).
3. 3D tetrahedral meshing with boundary recovery and Delaunay refinement.
4. Quadratic (tet10) subdivision with mid-side projection to the exact surface.
5. Quality report and worst-element highlighting.
6. Caps, determinism and refusal tests.
7. Fuzz corpus: degenerate, self-intersecting, open shells, zero-thickness, imported STEP junk.

**Exit:** cube, cylinder, plate with hole, L-bracket and a simple imported STEP bracket mesh with
documented quality; hostile cases refuse; same input gives the same mesh. **Gate:** if 3D quality on
real parts is not there, ship planar meshing only first and say so in the UI and roadmap; shell
analysis waits for the M4 shell elements and loads.

## M2 — Linear static

**Goal:** the first real answer: stress, displacement and factor of safety.

Slices:

1. Tet4/tet10 elements, material mapping, assembly.
2. Fixtures (fixed, frictionless, prescribed) and loads (force, pressure, moment, bearing, gravity).
3. Sparse CG with Jacobi/Chebyshev preconditioner; direct factorisation for small/ill-conditioned.
4. Residual checks, convergence reporting, condition warnings.
5. Stress recovery: von Mises, principal, tresca, strain, FoS.
6. `sim.preflight`, `sim.run`, `sim.status`, `sim.cancel`, `sim.results`.
7. Benchmarks: patch test, cantilever, plate with hole, L-bracket, pressurised cylinder.
8. "Refine and compare" convergence button.

**Exit:** benchmarks within tolerances; 150k DOF within budget; cancel works; results invalidate on
edit; screenshots. **This is the first user-usable release.**

## M3 — Real parts

**Goal:** make it survive the geometry people actually have.

Slices:

1. Holes, fillets, pockets, ribs: meshing and stress concentration.
2. Thin walls and local refinement.
3. Assemblies with bonded interfaces.
4. Imported STEP bodies; repair suggestions.
5. Load transfer across bonded interfaces and reaction checks.
6. Fuzz/robustness pass driven by real files.
7. Documentation: "why your part failed to mesh" guide.

**Exit:** an imported bracket assembly runs; reaction sums equal applied loads; failure modes are
named; no hangs on the fuzz corpus.

## M4 — Shells for sheet metal

**Goal:** the differentiator for this app.

Slices:

1. Mid-surface extraction from sheet-metal bodies and flat patterns.
2. Tri3/tri6 shells with reduced integration and hourglass control.
3. Thickness from sheet-metal rules; bend regions with reduced stiffness or explicit treatment.
4. Shell loads, fixtures and stress output.
5. Benchmarks: cantilever plate, simply supported plate, curved panel, bent bracket.

**Exit:** a folded sheet-metal bracket matches plate/beam references and a solid model within a
documented tolerance; flat pattern and analysis share thickness and bend data.

## M5 — Modal and buckling

Slices: mass matrix, Lanczos/shift-invert, rigid-body modes, participation factors, effective
mass; pre-stressed buckling; benchmarks (cantilever frequencies, free-free modes, Euler column).

**Exit:** frequencies within tolerance, six zero modes for free-free, mode shapes normalised and
animatable.

## M6 — Thermal and thermal stress

Slices: steady conduction, convection/film, flux, generation; material temperature dependence;
thermal field mapped to static with reference temperature; benchmarks (1D wall, fin, thick
cylinder, bimetallic strip).

**Exit:** temperatures match closed form within tolerance; thermal stress on a constrained bar
matches the analytic value; units and sources reported.

## M7 — Fatigue

Slices: S-N curve in the material card, constant-amplitude life, mean-stress correction (Goodman,
Gerber, none), damage and safety factor, hot-spot probe; benchmarks against hand calculations.

**Exit:** life and damage match the hand calculation; missing S-N refuses; the chosen correction is
visible in the report.

## M8 — Topology optimisation

Slices: SIMP, density filter, projection, volume constraint, symmetry, draw direction, minimum
member size; convergence history; iso-surface extraction to a watertight mesh body; manufacturability
warnings; benchmark (compliance decreases monotonically, volume met, output closed).

**Exit:** a bracket optimises to the requested volume, the output is a valid mesh body, the result
is reproducible from the same seed, and the manufacturing constraints are visible and testable.

## M9 — Flow (indicative)

Slices: voxelisation from exact solids; D3Q19 LBM with bounce-back; inlet/outlet/wall/moving-wall;
Smagorinsky LES option with a validity warning; streamlines, cut planes, pressure/velocity fields;
drag/lift integration; benchmarks (Poiseuille, lid-driven cavity Re 100/1000, cylinder drag).

**Exit:** benchmarks within documented tolerances; mass conservation reported; the UI states the
Reynolds/resolution validity range and that results are indicative. **Not claimed:** wind-tunnel
accuracy, high-Reynolds engineering CFD.

## M10 — Optimisation driver and nonlinear research

Slices:

1. Parameter sweeps and DOE over the existing parameter graph, evaluating static/modal/thermal;
   parallel across devices; results as a table and a plot.
2. Size optimisation of named parameters (wall thickness, rib width) against stress/frequency/
   mass targets.
3. Research spikes, each behind a feature flag and with a go/no-go gate: frictional contact,
   plasticity, transient thermal, explicit drop test, composite shells.

## GPU track (runs beside the milestones)

| Gate | Work | Landed with |
|---|---|---|
| G0 | Capability check, device survey, calibration, `sim.selftest` | M0 |
| G1 | SpMV/dot/axpy, CG, mixed precision, f64 residual | M2 |
| G2 | Batched dispatch, watchdog safety, cancel, device loss | M2 |
| G3 | Lanczos kernels | M5 |
| G4 | Density filter/sensitivity kernels | M8 |
| G5 | Voxelise + LBM kernels | M9 |
| G6 | Multi-device job queue; domain decomposition behind a preference | M10 |

## Platform track

| Gate | Work |
|---|---|
| P0 | macOS Apple Silicon and Intel: baseline, Metal, sandbox-free desktop |
| P1 | macOS: spelling, accessibility, Retina/HiDPI, sleep/wake during a run |
| P2 | Windows: Vulkan preferred, DX12 opt-in, TDR recovery |
| P3 | Linux: Vulkan, software adapter tests, headless CI |
| P4 | wasm: WebGPU, cooperative CPU, caps, offline test |

Each platform gate is a checklist in the PR that claims platform support, with the machines used
and what was *not* tested.

## What ships when

- **After M2:** a usable linear static stress tool for simple parts on any machine.
- **After M3:** credible on real, imported parts.
- **After M4:** sheet-metal analysis, which no hobbyist CAD does well.
- **After M5/M6:** vibration and temperature, reusing the same mesh and viewer.
- **After M8:** topology optimisation feeding CAM.
- **After M9:** indicative airflow, clearly labelled.
- **M10:** the automation and optimisation layer that makes the rest compound.

## Effort shape

Measured in slices, not months: M0 ~8 PRs, M1 ~8, M2 ~10, M3 ~6, M4 ~6, M5 ~5, M6 ~5, M7 ~3,
M8 ~6, M9 ~8, M10 ~6, plus the GPU and platform tracks. The binding constraint is validation and
review, not typing; keep each PR independently testable and mergeable.
