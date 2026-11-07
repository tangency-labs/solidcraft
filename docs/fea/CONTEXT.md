# SolidCraft Simulation (Analysis) — Context

> Status: proposal / planning only, plus a compiling `crates/sim`
> scaffold in this PR. No solver is implemented yet.

## What this is

SolidCraft is an open-source parametric 3D CAD application in Rust —
an alternative to Fusion 360. This work adds its **analysis /
simulation tool**, in the spirit of Fusion 360's Simulation workspace
and comparable in scope to ANSYS (static stress, thermal, modal),
built clean-room from public literature. It is **not** a copy, port,
or decompilation of ANSYS or Fusion 360, and contains no commercial
solver code. ANSYS installed locally may be used only as an external
validation oracle on the same mesh.

## User workflow (inside SolidCraft, not a separate app)

1. Model a part in SolidCraft (sketch → features on the timeline).
2. Open **Simulation**, select solid body/bodies from the document.
3. Assign materials (from the document's material definitions:
   $E$, $\nu$, $k$, density), loads, and constraints — as engine
   commands, so undo/redo, CLI, and the MCP server all see them.
4. Mesh the solid (see Architecture — volume hex meshing is the key
   integration gap) and solve on GPU.
5. View results in the SolidCraft viewport (von Mises stress,
   displacement, temperature), with a legend — the same viewport
   used for CAD, via `crates/render` / `crates/gpu`.
6. Optionally export `.vtu` for ParaView, or the same mesh to
   ANSYS/CalculiX for external comparison.

MVP physics, in order: linear heat → linear elasticity (implicit
PCG) → explicit dynamics. No implicit contact/plasticity in MVP.

## Non-goals (MVP)

- No ANSYS/Fusion code copying, reverse engineering, or solver
  binary embedding. ANSYS is a validation oracle only.
- No CAD kernel, mesher, or viewer of our own: geometry comes from
  `crates/kernel`, meshing from Gmsh (FFI) / `meshio`, viewing from
  the existing viewport + ParaView for `.vtu`.
- No GPU direct solver (no GPU Cholesky): matrix-free matvec + PCG.

## Architecture (where `crates/sim` sits)

```text
crates/kernel  B-rep solid (solidcore) ─┐
crates/geom    meshes, measures         ├─> crates/sim (NEW)   mesh → solve → results
crates/doc     materials, parameters,   ┘         │
               timeline, bodies                   ├─> crates/gpu (wgpu/WGSL)  GPU PCG
                                                  └─> crates/render           viewport results
crates/engine  commands (study, load, constraint, solve) — undo, CLI, MCP
```

`crates/sim` is a normal workspace member (`crates/*`), following the
same conventions as `crates/dynamics`: `#[forbid(unsafe_code)]`,
validated inputs, capped sizes, errors as values. Units follow
SolidCraft: mm, kg, s — so stress is N/mm² (MPa), forces mN.

## Precision / GPU

GPU-first via existing `wgpu` (Vulkan/DX12/Metal): one upload, one
download, hot loop GPU-resident. FP64 required for FEA accuracy;
FP32 + FP64 iterative refinement in dev, FP64 target on A100/H100/
RTX PRO class. CPU fallback (`faer`/`rayon`) for CI/headless.

## Stack

Gmsh FFI + `meshio`/`sprs` (mesh/IO) · Ginkgo FFI (PCG/AMG) with CPU
fallback · `wgpu`/WGSL, `bytemuck`, `glam` · CalculiX/Code_Aster +
ANSYS as external validation oracles (GPL/commercial — never linked
or copied) · NAFEMS benchmarks in CI · ParaView for `.vtu`.

## Open questions

- Volume meshing: kernel surface tessellation → Gmsh hex mesh is
  the main integration gap. Which path gives reliable hex meshes
  from SolidCraft B-reps fastest?
- WGSL FP64 availability per backend; Ginkgo FFI vs. WGSL Jacobi
  preconditioner for Phase 1.
- Result mapping: nodal results → existing render tessellation
  (per-vertex colour/legend) without a second viewer.
