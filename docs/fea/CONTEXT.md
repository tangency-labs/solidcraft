# SolidCraft FEA — Context

> Status: proposal / planning only. No solver code in this PR beyond a
> compiling scaffold under `fea/`. This is a clean-room, from-scratch
> project inspired by the *category* of tools such as ANSYS Mechanical /
> Fluent. It is **not** a copy, port, or decompilation of ANSYS or any
> other commercial solver. ANSYS installed locally may be used only as
> a reference/validation oracle on the same meshes.

## Vision

A GPU-first finite-element / CFD solver in Rust, differentiated from
primarily CPU-solved incumbents by a matrix-free, iterative,
GPU-resident architecture: one PCIe transfer in, one out, and the hot
loop never leaves the GPU.

MVP physics, in order:

1. Linear heat conduction (steady, then transient).
2. Linear elasticity (implicit, preconditioned conjugate gradients).
3. Explicit elastodynamics (Phase 3, not MVP).

## Non-goals (MVP)

- No copying, decompilation, reverse engineering, or reuse of ANSYS
  (or other commercial) code, file formats, or proprietary algorithms.
- No implicit contact, plasticity, or large-deformation formulations
  in the MVP — those kill schedules.
- No CAD kernel and no mesher of our own (see Stack).
- No GUI and no built-in viewer in the MVP. Visualisation is by
  writing `.vtu` and viewing in ParaView.
- No CPU direct-solver port to GPU (no GPU Cholesky). The path is
  matrix-free matvec + iterative PCG.

## Architecture (CPU/GPU split)

```text
+------------------------------- CPU (host) ------------------------------+
|  Gmsh (FFI) / meshio  ->  mesh import, BCs, setup, orchestration        |
|  Validation drivers (CalculiX / Code_Aster as external oracles)         |
|  PCG control loop (host): convergence checks, reductions orchestration  |
+-------------------------- one upload (H2D) ------------------------------+
|                           GPU (device, wgpu/WGSL)                       |
|  Element assembly (matrix-free) -> matvec -> PCG kernels (dot, axpy)    |
|  Solution field stays GPU-resident across all iterations                |
+-------------------------- one download (D2H) -----------------------------+
|  Write .vtu -> ParaView                                                  |
+-------------------------------------------------------------------------+
```

Fallback: if no GPU is available, the same algorithms run on CPU
(`faer` / `nalgebra-sparse` / `rayon`) so development and CI never
require a GPU. See `ARCHITECTURE.md` for data layout and kernels.

## Precision

FP64 is required for FEA accuracy. Consumer GPUs deliver roughly
1/32–1/64 of FP32 throughput in FP64, so:

- Development: FP32 with FP64 iterative refinement.
- Production target: FP64 on FP64-capable parts (A100/H100/RTX PRO
  class). WGSL/`wgpu` FP64 feature support is an open question
  (see below) — the scaffold is precision-agnostic by design.

## Stack (reuse, do not reinvent)

| Concern | Choice | Licence note |
|---|---|---|
| Meshing / IO | Gmsh via FFI, Rust `meshio` / `sprs` | Permissive |
| GPU sparse algebra / preconditioner | Ginkgo via FFI (PCG + Jacobi/AMG); CPU fallback `faer`, `nalgebra-sparse`, `rayon` | Permissive |
| GPU compute | `wgpu` + WGSL (Vulkan/DX12/Metal-first); CUDA/`cudarc` only if we later lock to NVIDIA | Permissive |
| CPU kernels / math types | `wgpu`, `bytemuck`, `glam` | Permissive |
| Validation oracle | CalculiX (CCX), Code_Aster, NAFEMS benchmarks in CI | GPL — external validators only, do not copy into closed-source code |
| Prototyping math (optional) | FEniCSx in Python | GPL — prototyping only |
| Visualisation | Write `.vtu`, view in ParaView | — |

Architectural references to *study* (do not copy code; GPL warning):
PyFR (GPU CFD), SPECFEM++/SeisSol (GPU explicit), MFEM (GPU FEM
assembly), deal.II (matrix-free). Ginkgo/MFEM/Gmsh are permissive;
CalculiX/Code_Aster/OpenFOAM/Elmer are GPL — use as external
validators, never copy into this codebase.

## Clean-room / licence guardrails

- All algorithms documented from public literature and textbooks
  (e.g. Hughes, Bathe, Zienkiewicz & Taylor) before implementation.
- No commercial solver binaries in the build or test path; oracles run
  as separate external processes on identical meshes.
- New dependencies must be checked against `deny.toml` / existing
  licence policy in this repository before being added.

## Open questions

- WGSL FP64: which wgpu backends/devices expose `f64`, and what is the
  fallback strategy on consumer GPUs? (Target for Phase 1 spike.)
- Gmsh FFI vs. `meshio`-only ingestion for Phase 0: which gives us
  structured hex meshes fastest?
- Ginkgo FFI: link strategy (system vs. vendored build) and whether a
  pure-WGSL Jacobi preconditioner is sufficient for Phase 1 before
  pulling in Ginkgo.
- Primary GPU vendor for first benchmark hardware (not yet decided;
  portability via wgpu is the default assumption).
