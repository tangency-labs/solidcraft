# Simulation requirements

Part of the [simulation plan](README.md) and [RFC 0010](../rfcs/0010-simulation.md).

Requirements are numbered for traceability into tests and PRs. "Shall" means testable. Where a
number is a target rather than a guarantee, it is marked *(budget)* and is tuned after M2 with
measurements, not guessed twice.

## 1. Users

| Persona | What they need | What would lose them |
|---|---|---|
| Hobbyist / maker | "Will this bracket bend? Is 3 mm enough? Where do I put ribs?" | Jargon, a mesher that fails on their part, a UI that assumes FEA knowledge |
| Student | Learn the physics, see assumptions, compare with a hand calculation | A black box with no units, no mesh view, no convergence information |
| Small shop / startup engineer | Quick screening before paying for a specialist; a defensible first pass | Wrong numbers presented confidently; no export to hand to a reviewer |
| Experienced engineer at home | A fast sanity check on their own machine, scriptable, reproducible | Results that cannot be traced, versioned or compared |

## 2. Functional requirements

### 2.1 Studies and lifecycle

- **SIM-R-001** The user shall create, rename, duplicate and delete simulation studies in a design.
- **SIM-R-002** A study shall have a type: Static, Modal, Buckling, Thermal, Thermal-stress,
  Fatigue, Topology, Flow. The type determines available inputs, solver and outputs.
- **SIM-R-003** A study shall reference bodies by persistent body/face/edge names, not by index.
- **SIM-R-004** When a referenced face or edge is renamed by a model edit, the study shall follow
  the name; when it is split, the study shall use all pieces and warn; when it is gone, the study
  shall mark that item broken, name it, and refuse to run until re-picked.
- **SIM-R-005** A run result shall be valid only for the exact inputs that produced it. Changing
  geometry, material, mesh settings, loads or solver version shall mark it out of date.
- **SIM-R-006** Undo/redo shall cover study creation, input edits and deletion. A run is not undoable
  (it is derived), but the result it replaced shall be available again after redo.
- **SIM-R-007** Studies shall be copied with their bodies across design copy/paste and preserved
  through configuration switching where their body set is unchanged.
- **SIM-R-008** Study and input edits shall be scriptable through the same commands as the UI.

### 2.2 Geometry and meshing

- **SIM-R-010** The analysis mesh shall be generated from exact B-rep at a study-specified
  tolerance; the display mesh shall never be reused for analysis.
- **SIM-R-011** Mesh settings shall include target element size, minimum size, growth rate, element
  order (linear/quadratic) and, for flow, near-wall refinement.
- **SIM-R-012** The user shall preview the mesh, its element count and a quality report (aspect
  ratio, minimum Jacobian, volume error, worst elements highlighted) before running.
- **SIM-R-013** When geometry cannot be meshed, the run shall refuse with the offending bodies,
  faces or regions named and a suggested fix (repair, simplify, change size, exclude body).
- **SIM-R-014** Meshing shall be deterministic for the same inputs on the same build.
- **SIM-R-015** Meshing shall not hang or exhaust memory on hostile or degenerate input; it shall
  stop at documented caps with a named error.
- **SIM-R-016** Assemblies shall be supported with bonded (tied) interfaces in v1; frictional
  contact is out of v1.
- **SIM-R-017** Sheet-metal bodies shall support shell idealisation (mid-surface) as a separate
  milestone, reusing the flat pattern and thickness rules.

### 2.3 Materials

- **SIM-R-020** Material properties shall come from the design's physical material cards, including
  isotropic and orthotropic elasticity, density, thermal conductivity and thermal expansion.
- **SIM-R-021** A study shall allow a per-body material override without changing the design's
  assigned material.
- **SIM-R-022** Missing required properties shall refuse the run and name exactly what is missing.
- **SIM-R-023** Every result shall record the material source and conditions (grade, temperature,
  direction) that were used.
- **SIM-R-024** Fatigue shall require an optional S-N curve in the material card; when absent, the
  fatigue study shall refuse rather than assume one.
- **SIM-R-025** Material changes shall invalidate results that used them.

### 2.4 Boundary conditions and loads

- **SIM-R-030** Fixtures shall be definable on faces, edges and vertices: fixed, frictionless
  (roller/symmetry), and prescribed displacement (with components).
- **SIM-R-031** Loads shall include force (total or per-area), pressure, moment, bearing load,
  gravity/inertial, and imposed temperature or heat flux for thermal studies.
- **SIM-R-032** Loads shall support constant and linearly varying distributions; time/frequency
  dependence is out of v1.
- **SIM-R-033** Each boundary condition shall show its magnitude, units and a glyph in the viewport
  that can be shown/hidden and filtered.
- **SIM-R-034** A study with no fixture, or with fixtures insufficient to remove rigid-body motion,
  shall refuse to run and explain which motion is unconstrained.
- **SIM-R-035** A study whose net load is unbalanced (for a static case) shall warn with the
  imbalance magnitude and direction.
- **SIM-R-036** Units shall be explicit at input and output: mm, N, MPa, N·mm, K, W, W/(m·K).

### 2.5 Solvers

- **SIM-R-040 Static** shall solve linear elasticity for solids and shells and report displacement,
  von Mises/principal/tresca stress, strain and factor of safety against yield and ultimate.
- **SIM-R-041 Modal** shall report natural frequencies, mode shapes, participation factors and
  effective mass for a requested mode count, with rigid-body modes identified as zero.
- **SIM-R-042 Buckling** shall report load factors and buckling modes for a pre-stressed state.
- **SIM-R-043 Thermal** shall solve steady conduction with prescribed temperature, heat flux,
  convection (film coefficient + ambient) and internal generation.
- **SIM-R-044 Thermal-stress** shall map the temperature field into a static solve and report the
  resulting stress, with the reference temperature recorded.
- **SIM-R-045 Fatigue** shall report stress-life damage/life for constant-amplitude loading using
  the material's S-N curve, with the chosen mean-stress correction named.
- **SIM-R-046 Topology** shall minimise compliance subject to a volume fraction, with optional
  minimum member size, symmetry planes and draw direction, and shall output a watertight mesh body
  that can be used downstream.
- **SIM-R-047 Flow** shall solve steady incompressible flow on a voxel grid derived from exact
  geometry and report velocity, pressure, streamlines, and drag/lift for selected faces, labelled
  indicative.
- **SIM-R-048** Every solver shall report convergence: residual history, iterations, and whether
  the requested tolerance was met.
- **SIM-R-049** Every solver shall estimate discretisation error where feasible: a one-click
  "refine and compare" that reports the change in the headline result.
- **SIM-R-050** Every solver shall refuse rather than return a non-converged result as if valid.

### 2.6 Results and reporting

- **SIM-R-060** Results shall be viewable as colour-mapped fields on the deformed or undeformed
  model, with a legend showing quantity, unit, min/max and, where meaningful, a symmetric scale.
- **SIM-R-061** The user shall probe a point, face or node and see interpolated values.
- **SIM-R-062** The user shall control deformation scale, animation of modes, clipping/section,
  and iso-surfaces.
- **SIM-R-063** Results shall be comparable between runs and configurations (side-by-side or delta).
- **SIM-R-064** Results shall export to VTK (fields), CSV (scalars/probes) and PNG (pictures).
- **SIM-R-065** Every result view shall include a "what this does not prove" summary and the
  assumptions used.
- **SIM-R-066** A run summary shall record backend, precision mode, solver version, mesh hash,
  element/DOF counts, wall time, peak memory and warnings.

### 2.7 Hardware use

- **SIM-R-070** The CPU f64 path shall work on every supported platform with no GPU present,
  including CI machines and the browser.
- **SIM-R-071** A GPU path shall be used only when the device passes a capability check (compute
  shaders, required limits) and the problem is large enough to benefit.
- **SIM-R-072** The user shall see which backend, device and precision produced a result.
- **SIM-R-073** The user shall be able to force CPU or a specific adapter through Preferences and
  through the existing `SOLIDCRAFT_GPU` environment policy.
- **SIM-R-074** Long runs shall be cancellable and shall not block the window; progress and elapsed
  time shall be visible.
- **SIM-R-075** Running out of device memory shall be predicted where possible and otherwise
  recovered from, with a CPU retry offered.
- **SIM-R-076** Device loss or driver reset shall lose at most the current run; the study and design
  shall remain intact.
- **SIM-R-077** Independent studies and optimisation iterations may run concurrently across
  adapters; concurrent use shall never corrupt results or exceed a user-set memory budget.
- **SIM-R-078** The browser build shall solve small studies when WebGPU compute is available and
  fall back to a cooperative CPU solver otherwise, without freezing the tab.

### 2.8 Automation and agents

- **SIM-R-080** Every action shall be a command with an input and output schema.
- **SIM-R-081** Commands shall never open a dialog; the UI invokes them like an agent does.
- **SIM-R-082** Commands shall return machine-readable diagnostics, not only human strings.
- **SIM-R-083** A `sim.devices` command shall report adapters, tiers, limits and chosen backend.
- **SIM-R-084** A `sim.selftest` command shall run a tiny known problem on a chosen backend and
  report pass/fail, precision and determinism, for contributors and CI.
- **SIM-R-085** Batch/parameter sweeps and topology iterations shall be drivable from MCP and the
  control channel with progress and cancellation.

## 3. Non-functional requirements

### 3.1 Reliability and safety

- **SIM-N-001** No `unwrap`, `expect`, `panic`, `unsafe` in non-test code; all failures are `Result`.
- **SIM-N-002** Every array, string, mesh size, DOF count, iteration count and file size has a cap.
- **SIM-N-003** A killed/cancelled run leaves no partial state applied to the design.
- **SIM-N-004** Results are attached atomically: either a complete, converged result or none.
- **SIM-N-005** The app shall remain responsive (window thread free) during mesh and solve.
- **SIM-N-006** Crash reports shall include study type, sizes and backend, never design geometry.

### 3.2 Performance budgets

Measured on a baseline 8 GB Apple Silicon laptop, release build, default settings. Targets, not
promises; revised with data in M2.

| Workload | Integrated GPU | CPU only | Discrete GPU |
|---|---|---|---|
| Mesh 100k tets | ≤ 20 s | ≤ 20 s | ≤ 20 s |
| Static, 150k DOF | ≤ 60 s | ≤ 180 s | ≤ 40 s |
| Static, 1M DOF | refuse with advice | refuse with advice | ≤ 180 s |
| Modal, 20 modes, 150k DOF | ≤ 120 s | ≤ 300 s | ≤ 90 s |
| Topology, 30 iterations, 100k elements | ≤ 15 min | ≤ 45 min | ≤ 10 min |
| Flow, 2M voxels, 2000 steps | ≤ 3 min | ≤ 20 min | ≤ 2 min |
| Result field upload/display | ≤ 200 ms | ≤ 200 ms | ≤ 100 ms |

- **SIM-N-010** Every run shall show a size/memory estimate before it starts and refuse before
  allocating beyond the budget.
- **SIM-N-011** Peak memory shall be reported; the default budget shall be a fraction of physical
  RAM/VRAM, user-adjustable.
- **SIM-N-012** Progress shall advance at least every 500 ms, or the UI shall say the current stage
  is long (meshing, factorisation).
- **SIM-N-013** Cancel shall take effect within 500 ms for iterative stages and at the next safe
  point for factorisation.

### 3.3 Portability

- **SIM-N-020** All library crates shall build for `wasm32-unknown-unknown`.
- **SIM-N-021** The numerical core shall not depend on OS, filesystem, threads or GPU APIs.
- **SIM-N-022** Platform-specific code shall be confined to the job runner, adapter selection and
  file dialogs.
- **SIM-N-023** The app shall run offline; no network call is required for any simulation feature.
- **SIM-N-024** Endianness, locale and decimal separators shall not affect results or file content.
- **SIM-N-025** GPU shaders shall avoid optional features unless the capability check passed.

### 3.4 Compatibility and contribution

- **SIM-N-030** A design with studies shall open in an older build with the documented "newer
  format" behaviour; it shall not be silently rewritten.
- **SIM-N-031** Adding a study type shall not require a new format change once the study envelope
  exists; unknown study types shall be preserved on save by older builds where the format allows.
- **SIM-N-032** New crates shall have a layer table row, architecture row, licence, README (shared
  crates) and pass `cargo xtask layers`, `shared`, `wasm`.
- **SIM-N-033** New UI strings shall live in `assets/locales/pending/<slug>.json`; new assets shall
  have attribution rows.
- **SIM-N-034** Every command shall have a schema or an entry in `schema/unconverted.txt` that only
  shrinks.
- **SIM-N-035** Tests shall go in the area's existing test file, not a per-bug file.
- **SIM-N-036** A changed default shall keep the old behaviour behind a preference and fill the PR's
  "Changes a default:" line.

## 4. Definition of done per area

A solver is "done" only when all of these exist:

1. Functional requirement IDs implemented and covered by command tests.
2. A benchmark table with reference values, tolerances and a convergence check.
3. CPU f64 and GPU f32 agreement test where a GPU exists, skipped cleanly where none does.
4. Cap, fuzz and refusal tests for hostile input.
5. Result metadata (backend, precision, versions, mesh hash, residual).
6. UI with live preview where applicable, progress, cancel and a "what this does not prove" panel.
7. Screenshots or a recording for the observable change.
8. `cargo xtask ci` green.

## 5. Explicit non-claims

The product shall not state or imply:

- that results are certified, validated for a regulatory regime, or a substitute for a licensed
  engineer where one is required;
- that flow results are wind-tunnel accurate outside the documented validity range;
- that a mesh-quality or convergence warning can be ignored;
- that a topology result is manufacturable without the user checking the manufacturing constraints;
- that material cards authenticate the source of their data.
