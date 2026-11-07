# SolidCraft Simulation — Architecture

> At `docs/fea/ARCHITECTURE.md` deliberately: `docs/architecture.md`
> describes the CAD layers and would collide on case-insensitive
> filesystems.

## Integration points (existing crates)

| From | To `crates/sim` | Notes |
|---|---|---|
| `crates/kernel` | B-rep solid(s) selected in the document | Source geometry; no CAD code duplicated |
| `crates/geom` | surface mesh / measures (`mesh.rs`, `tess`) | Starting point for volume meshing |
| Gmsh | volume hex mesh from the B-rep/surface mesh | **Main gap**: SolidCraft tessellates surfaces; hex volume mesh needs Gmsh. Licence: GPLv2-or-later w/ linking exception + commercial option — external process preferred; FFI needs licence review |
| `crates/doc` | materials ($E,\nu,k,\rho$), bodies, parameters | Units mm/kg/s; coherent stress mN/mm², 1 mN/mm² = 0.001 MPa |
| `crates/engine` | commands: create study, set load/constraint, solve | Undo/redo, CLI and MCP for free |
| `crates/gpu` | wgpu device + WGSL kernels | Reuses the viewport's wgpu stack (v30) |
| `crates/render` | result fields → per-vertex colours + legend | No second viewer; ParaView only for `.vtu` |

## Data layout

SoA GPU buffers (coordinates, connectivity, solution/residual per
DOF), CSR only for CPU reference/preconditioner, element-major
ordering for WGSL workgroups, `bytemuck` packing. Matrix-free on
GPU: assembly → matvec → PCG (dot/axpy/Jacobi). PCG loop is host
code: bulk field data is uploaded once and downloaded once, plus a
small scalar readback per iteration for the `dot` reductions.
Transfers are always reported as bulk vs. per-iteration scalars.

## WGSL kernels

`assemble`, `matvec`, `dot` (+ host reduction), `axpy`, `jacobi`.
AMG via Ginkgo FFI only if Jacobi proves insufficient.

## Precision and fallback

WGSL kernels are FP32-only (WGSL has no `f64`). Native FP64 needs a
separate backend (CUDA/CPU), not yet chosen. Fallback to CPU
(`faer`/`nalgebra-sparse`/`rayon`) — same mesh, same viewport
output — when: no GPU, CI/headless, or native FP64 is required but
no compatible GPU backend/adapter is available (e.g. no
`SHADER_F64`). Backend is a runtime choice behind a small trait;
physics never branches on it.

## Crate layout

```text
crates/sim/     NEW workspace member: study types, mesh/BC assembly, solvers
  src/lib.rs    #![forbid(unsafe_code)], validated inputs, capped sizes
```

Dependencies (scaffold): `solidcraft-geom`, `serde`, `thiserror`,
`log` — kernel/doc/engine wiring is Phase 0 work, added with the
real APIs then, not stubbed with invented ones.

## Open questions

Hex volume meshing from SolidCraft B-reps · native-FP64 backend
choice · nodal result → render tessellation mapping · Gmsh licence
route · Ginkgo link strategy.
