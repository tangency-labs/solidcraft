# Simulation architecture

Part of the [simulation plan](README.md) and [RFC 0010](../rfcs/0010-simulation.md).

## 1. Where the code goes

The layer rules in `docs/layers.md` decide this. wgpu may appear only in L5, the apps and shared
crates; a crate below L5 cannot reach it, directly or through a shared crate.

| Layer | Crate | Holds |
|---|---|---|
| L1 | `crates/meshing` (new) | Analysis tessellation request, 2D/3D meshing, quality report, caps |
| L1 | `crates/fea` (new) | Elements, materials, assembly, linear/eigen/thermal solvers, fields, the `Backend` trait |
| L1 | `crates/flow` (new) | Voxelisation and the CPU LBM stepper |
| L2 | `crates/doc` | `Study` records, validation, name resolution, format, invalidation hashes |
| L4 | `crates/engine` | `sim.*` commands, run queue, backend injection, result cache |
| L5 | `crates/sim-gpu` (new) | wgpu compute backend implementing `fea::Backend` and `flow::Backend` |
| L5 | `crates/ui-egui` | Simulation workspace, dialogs, progress, result controls |
| shared | `crates/viewport`, `viewport-gpu` | Field shading, deformation, probe rendering; adapter survey already lives in `viewport-gpu::select` |

Why the GPU backend is L5 and not L1: the existing render stack already works this way — L1
`render` describes a job and has no wgpu dependency, while the device work lives above it. The
trait lives in L1; the implementation is injected by the host. `crates/sim-gpu` is a new L5 crate
with a row in `xtask/src/layers.rs` and `docs/architecture.md`.

Dependency direction:

```
ui-egui ──► engine ──► doc ──► fea ──► geom
   │           │         │       ▲
   │           │         └──► meshing
   │           └── Arc<dyn fea::Backend>   (injected)
   └──► sim-gpu ──► fea (trait), flow (trait), viewport-gpu::select, wgpu
```

`meshing` and `fea` depend on `solidcore-tess` only through its public re-exports; exact surface
evaluation and tessellation are already there. No new solidcore seam.

## 2. Data model

### 2.1 Study (saved in the design)

```rust
struct Study {
    id: StudyId,
    name: String,
    kind: StudyKind,              // Static | Modal | Buckling | Thermal | ThermalStress | Fatigue | Topology | Flow
    bodies: Vec<BodyRef>,         // persistent body names
    overrides: Vec<MaterialOverride>,
    mesh: MeshSettings,
    fixtures: Vec<Fixture>,
    loads: Vec<Load>,
    options: StudyOptions,        // tolerance, mode count, volume fraction, flow conditions...
}
```

Every reference is a persistent name (`F2:top`, body id, edge name), never an index. Broken
references use the same outcomes as linked designs: exact, merged, split, by geometry, missing —
with `missing` refusing the run and naming the item.

### 2.2 Run result (derived, never saved)

```rust
struct RunResult {
    inputs_hash: Hash,            // geometry + study + material cards + solver version
    meta: RunMeta,                // backend, precision, versions, timings, peak memory, counts
    mesh: Arc<Mesh>,              // the analysis mesh actually used
    fields: Vec<Field>,           // displacement, stress, temperature, velocity...
    scalars: Scalars,             // max von Mises + location, min FoS, frequencies, drag...
    diagnostics: Diagnostics,     // residual history, convergence, mesh quality, warnings
}
```

`Field` is the one result type every solver produces and every view consumes:

```rust
struct Field {
    name: String,                 // "von_mises", "temperature", "velocity"
    domain: Domain,               // Node | Element | Face | Voxel
    components: u8,               // 1, 2, 3, 6 (tensor)
    unit: Unit,                   // MPa, mm, K, m/s
    values: Vec<f64>,             // canonical f64; GPU results are read back and checked
    range: (f64, f64),
}
```

### 2.3 Invalidation

```text
inputs_hash = H(
    for each body:  exact geometry fingerprint + face/edge name set,
    study JSON,
    material cards used (values + conditions),
    solver version + precision mode
)
```

A result whose hash differs from the current study is shown as **out of date**, never silently
reused. Per-body fingerprints come from the record hash when available, else exact bounds, volume,
area and the sorted face/edge name set.

### 2.4 Result cache

Engine-owned `HashMap<StudyId, RunResult>` with an LRU bound by estimated memory. Results survive
undo/redo of unrelated edits; they do not survive a save/reopen (recomputed on demand). This keeps
`.sc` files small and avoids stale-file class bugs.

## 3. Meshing

- Request an **analysis tessellation** from `solidcore-tess` at the study's tolerance. Never reuse
  the display mesh, whose tolerance is chosen for pixels.
- Build the volume mesh from the exact surface: boundary nodes projected onto the exact surface
  through `EvalSurface`, inside/outside classified against the exact solid.
- v1 targets solids that are closed, non-self-intersecting and have no zero-thickness features.
  Thin walls, slivers and bad imports refuse with a named reason (see SIM-R-013).
- Output: nodes, elements, boundary face → exact face mapping, quality metrics per element, and
  the material region of each element.
- Determinism: stable ordering, no hash-map iteration order in the algorithm, fixed refinement
  queue tie-breaks.
- Caps: max nodes, elements, boundary faces; stop before allocating; report estimate in preflight.

Meshing strategy, staged:

1. **2D constrained Delaunay** for planar faces and shell mid-surfaces.
2. **3D tetrahedral** from the surface triangulation with boundary recovery and Delaunay refinement.
3. **Boundary layers** for flow (prisms), later.
4. **Quadratic** (tet10) by edge subdivision for stress accuracy, with mid-side projection to the
   exact surface.

If stage 2 cannot reach quality on real parts by the end of M2, the plan narrows the shipped UI to
what passes (planar/shell + simple solids) and says so. It does not ship a mesher that silently
produces bad elements.

## 4. Solvers

| Study | Core | Notes |
|---|---|---|
| Static | `K u = f`, sparse SPD | CG + preconditioner; direct factorisation for small/ill-conditioned; shells and solids |
| Modal | `K φ = λ M φ` | Lanczos/shift-invert; rigid-body modes; participation factors |
| Buckling | `(K + λ K_g) φ = 0` | Pre-stress from a static run |
| Thermal | `K_t T = q` | Conduction, convection, flux, generation |
| Thermal-stress | Thermal field → static | Reference temperature recorded |
| Fatigue | Post-process | S-N curve, mean-stress correction named |
| Topology | SIMP loop | Many static solves; density filter + projection; iso-surface output |
| Flow | LBM D3Q19 | Voxel grid, bounce-back walls, inlet/outlet; indicative only |

Element set for v1: tet4/tet10 (solid), tri3/tri6 shell (sheet metal), with reduced integration
and hourglass control for shells; beam/truss later. Assembly supports shared-node bonded
interfaces; no general contact in v1.

Linear algebra:

- Evaluate `faer` (MIT), `sprs` (MIT), `nalgebra` (Apache-2.0) in M0. Prefer `faer` for sparse
  Cholesky/LU and dense eigen; implement CG/PCG and Lanczos ourselves so the GPU backend can share
  the iteration.
- Matrix storage: CSR f64 on CPU; CSR f32 or matrix-free on GPU.
- Deterministic reductions on CPU: fixed tree order per row block, no atomic accumulation.
- Condition estimate and a warning when the problem is ill-conditioned; suggest quadratic
  elements, a finer mesh, or the f64 CPU path.

## 5. Backend abstraction and precision

Trait in L1 (`fea::Backend`), implemented by CPU f64 in `fea` and by `sim-gpu` for wgpu:

```rust
trait Backend: Send + Sync {
    fn describe(&self) -> BackendInfo;         // name, device, precision, limits
    fn capabilities(&self) -> Capabilities;    // which solvers this backend accelerates
    fn static_solve(&self, problem: &Problem, ctl: &Control) -> Result<Solution>;
    fn modal_solve(&self, problem: &Problem, ctl: &Control) -> Result<ModalSolution>;
    fn thermal_solve(&self, problem: &Problem, ctl: &Control) -> Result<Solution>;
    fn flow_solve(&self, problem: &FlowProblem, ctl: &Control) -> Result<FlowSolution>;
}
```

`Control` carries progress, cancellation, memory budget and a preferred precision. Unsupported
combinations return `Unsupported` and the engine falls back to CPU rather than failing the study.

Precision policy:

- **CPU f64 is authoritative.** It is the oracle for tests and the fallback for everything.
- **GPU f32 is the accelerator.** Shader f64 is not portable (Apple GPUs have none), so kernels
  are f32; f64 residuals, convergence decisions and stress recovery happen on the CPU.
- A run records `precision: "f64"`, `"mixed-f32-f64"` or `"f32"` and only claims **verified** when
  the f64 residual check passed.
- If mixed precision fails to converge or the condition estimate is poor, the run falls back to
  CPU f64 and says so.

## 6. Result rendering

- Fields are plain data: positions, per-vertex or per-element values, range, palette, unit.
- `crates/viewport`'s analysis shading already has `AnalysisMode` (zebra, draft, curvature,
  environment, access) and `AnalysisColors`; add a field mode that takes a value buffer and a
  palette. Keep `viewport` free of `fea` types — pass plain data.
- Deformation: positions are displaced by `scale × displacement`; the scale is a view control, not
  a result.
- Modes: animate a selected mode with a phase, not a physics run.
- Probe: nearest node/face, barycentric interpolation, show all components and the element id.
- Section/clipping reuses the existing section view.

## 7. Commands

Every command has an input and output schema, never opens a dialog, and is callable from the UI,
CLI, control channel and MCP.

| Command | Does |
|---|---|
| `sim.study.create` | New study of a kind for named bodies |
| `sim.study.edit` | Rename, change options, change body set |
| `sim.study.duplicate` / `sim.study.delete` | With undo |
| `sim.study.list` | Studies and their validity state |
| `sim.material.assign` | Per-body material or override |
| `sim.fixture.add` / `.edit` / `.remove` | Named face/edge/vertex supports |
| `sim.load.add` / `.edit` / `.remove` | Force, pressure, moment, bearing, gravity, thermal |
| `sim.mesh.settings` | Size, order, growth, refinement |
| `sim.mesh.preview` | Mesh + quality report, no solve |
| `sim.preflight` | Counts, memory estimate, missing properties, unbalanced load, bad picks |
| `sim.run` | Mesh + solve; returns run id, or refuses with reasons |
| `sim.cancel` | Stop a run by id |
| `sim.status` | Progress, stage, elapsed, peak memory |
| `sim.results` | Fields, scalars, diagnostics, metadata |
| `sim.probe` | Values at a point, face or node |
| `sim.export` | VTK, CSV, PNG, study report |
| `sim.compare` | Runs side by side or delta |
| `sim.devices` | Adapters, tier, limits, chosen backend |
| `sim.selftest` | Known problem on a backend: pass/fail, precision, determinism |

`sim.preflight` mirrors `render.preflight`: it is what the UI shows before spending minutes, and
what an agent calls before running a batch.

## 8. Jobs, threads and the window

- Meshing and solving run in the engine's job queue, off the window thread. The UI applies a
  completed result atomically and drops it if the study changed or the run was cancelled.
- Progress events (`sim.progress`) carry stage, fraction, elapsed and a message id for translation.
- Native: worker threads (rayon inside CPU kernels is allowed, with deterministic reductions).
- Browser: cooperative chunked solver; no rayon. A run yields between chunks so the tab stays
  responsive. A Web Worker is a later change and needs JS glue, which the repo keeps minimal.
- Device loss: the backend returns `DeviceLost`; the engine marks the run failed, keeps the study
  and offers a CPU retry. It never aborts the app.

## 9. Format

- One `crates/doc/src/format/pending/studies.rs` file: `//!` note plus, only if needed, a `step`.
- Saves under `simulation: { studies: [...] }` at the document root.
- Older builds: the standard "newer format" warning, open what they can, keep the rest.
- Results are not saved. A study's `last_run` metadata is not saved either; on reopen the UI says
  "not run in this session".
- Unknown study kinds and unknown option keys survive save by a build that does not understand
  them, so adding a solver later does not need another format change.

## 10. UI structure

Simulation workspace tabs, registered in `crates/ui-egui/src/workspace.rs`:

| Tab | Panels |
|---|---|
| SETUP | Bodies, study type, units, duplicate/delete |
| MATERIAL | Assignment, overrides, missing-property warnings |
| LOADS | Fixtures, loads, thermal conditions, glyph visibility |
| MESH | Size/order/growth, preview, quality histogram, worst elements |
| RUN | Preflight, memory estimate, progress, cancel, backend/precision |
| RESULTS | Field, range, deformation, probe, section, animation, export, diagnostics |
| OPTIMIZE | Volume fraction, constraints, iterations, output body |
| FLOW | Inlet/outlet, fluid properties, grid, streamlines, drag/lift |

All strings via `assets/locales/pending/<slug>.json`; all colours, sizes and spacing from
`solidcraft_theme::Tokens`; one outer container; icons original with attribution rows.

## 11. Interaction with existing areas

- **Dynamics:** S0 puts a UI on `crates/dynamics` (mass, inertia, reactions, contacts, playback).
  No new solver.
- **CAM:** both need derived results, invalidation, preflight, jobs and cancellation. Topology
  output is a mesh body CAM can machine; draw direction and minimum member size are shared inputs.
- **Sheet metal:** flat patterns and thickness rules feed shell mid-surfaces; this is the strongest
  reason to do shells early.
- **Configurations:** a configuration switch that changes geometry or material invalidates runs;
  configuration comparison reads `sim.compare`.
- **Parameters:** S11 drives the parameter graph to run sweeps and optimisation without a second
  document model.
