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
SolidCraft: mm, kg, s — so the coherent stress unit is **mN/mm²**,
forces are mN, and **1 mN/mm² = 0.001 MPa** (state this conversion
wherever material properties or results are shown in MPa).

## Precision / GPU

GPU-first via existing `wgpu` (Vulkan/DX12/Metal): the hot loop is
GPU-resident; bulk field data is uploaded once and the solution
downloaded once. Host-side PCG additionally reads back small scalar
`dot` results per iteration — those per-iteration scalar readbacks
are counted separately from bulk transfers (see ROADMAP Phase 1).
WGSL defines only `f32`/`f16`, **not** `f64`, so FP64 cannot be a
WGSL production path: WGSL kernels stay FP32 (with FP64 iterative
refinement where the refinement step runs). Native FP64 requires a
separate, named compute backend (e.g. CUDA or CPU) — until one is
chosen, production FP64 uses the CPU fallback (`faer`/`rayon`), which
is also the fallback for CI/headless and for any adapter without
the required FP64 support (`SHADER_F64` on wgpu, or equivalent).

## Stack

Gmsh + `meshio`/`sprs` (mesh/IO) · Ginkgo FFI (PCG/AMG) with CPU
fallback · `wgpu`/WGSL, `bytemuck`, `glam` · CalculiX/Code_Aster +
ANSYS as external validation oracles (GPL/commercial — never linked
or copied) · NAFEMS benchmarks in CI · ParaView for `.vtu`.

**Gmsh licence (review before integration):** Gmsh is distributed
under GPLv2-or-later, with a linking exception and a separate
commercial-licence option — it is **not** permissive. FFI-linking it
into this MIT/Apache-2.0 codebase needs a licence review first;
running Gmsh as an external process (like the CalculiX oracle)
avoids linking our code under GPL. `meshio` (permissive) is the
default ingestion path until that review concludes.

## Open questions

- Volume meshing: kernel surface tessellation → Gmsh hex mesh is
  the main integration gap. Which path gives reliable hex meshes
  from SolidCraft B-reps fastest?
- Native-FP64 backend choice (CUDA vs CPU-only); Ginkgo FFI vs.
  WGSL Jacobi preconditioner for Phase 1.
- Gmsh licence review: external process vs. FFI with commercial
  licence (see Stack).
- Result mapping: nodal results → existing render tessellation
  (per-vertex colour/legend) without a second viewer.
