# SolidCraft Simulation — Architecture

> At `docs/fea/ARCHITECTURE.md` deliberately: `docs/architecture.md`
> describes the CAD layers and would collide on case-insensitive
> filesystems.

## Integration points (existing crates)

| From | To `crates/sim` | Notes |
|---|---|---|
| `crates/kernel` | B-rep solid(s) selected in the document | Source geometry; no CAD code duplicated |
| `crates/geom` | surface mesh / measures (`mesh.rs`, `tess`) | Starting point for volume meshing |
| Gmsh (FFI) | volume hex mesh from the B-rep/surface mesh | **Main gap**: SolidCraft tessellates surfaces; hex volume mesh needs Gmsh |
| `crates/doc` | materials ($E,\nu,k,\rho$), bodies, parameters | Units mm/kg/s; stress N/mm² = MPa |
| `crates/engine` | commands: create study, set load/constraint, solve | Undo/redo, CLI and MCP for free |
| `crates/gpu` | wgpu device + WGSL kernels | Reuses the viewport's wgpu stack (v30) |
| `crates/render` | result fields → per-vertex colours + legend | No second viewer; ParaView only for `.vtu` |

## Data layout

SoA GPU buffers (coordinates, connectivity, solution/residual per
DOF), CSR only for CPU reference/preconditioner, element-major
ordering for WGSL workgroups, `bytemuck` packing. Matrix-free on
GPU: assembly → matvec → PCG (dot/axpy/Jacobi). PCG loop is host
code; one H2D, one D2H per solve.

## WGSL kernels

`assemble`, `matvec`, `dot` (+ host reduction), `axpy`, `jacobi`.
AMG via Ginkgo FFI only if Jacobi proves insufficient.

## Fallback

No GPU → same algorithms on CPU (`faer`/`nalgebra-sparse`/`rayon`),
same mesh, same viewport output. Backend is a runtime choice behind
a small trait; physics never branches on it.

## Crate layout

```text
crates/sim/     NEW workspace member: study types, mesh/BC assembly, solvers
  src/lib.rs    #![forbid(unsafe_code)], validated inputs, capped sizes
```

Dependencies (scaffold): `solidcraft-geom`, `serde`, `thiserror`,
`log` — kernel/doc/engine wiring is Phase 0 work, added with the
real APIs then, not stubbed with invented ones.

## Open questions

Hex volume meshing from SolidCraft B-reps · WGSL FP64 · nodal
result → render tessellation mapping · Ginkgo link strategy.
