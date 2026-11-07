# SolidCraft FEA — Architecture

> Note: this file lives at `docs/fea/ARCHITECTURE.md` deliberately.
> `docs/architecture.md` in this repository describes the SolidCraft
> CAD application; on case-insensitive filesystems (Windows/macOS)
> the two names would collide.

## Data layout

- **Structure-of-arrays (SoA)** for GPU-bound fields: coordinates,
  element connectivity, and per-DOF solution/residual vectors stored
  as separate contiguous buffers, not array-of-structs, so WGSL
  workgroups read coalesced memory.
- **CSR** for any explicitly assembled sparse operator (CPU reference
  and preconditioner construction). GPU path is matrix-free and does
  not assemble a global CSR.
- **Element-major ordering** for assembly inputs, matching WGSL
  workgroup dispatch (one workgroup per element batch).
- Index type and buffer alignment follow `wgpu` requirements
  (`bytemuck` for host-side packing); padded strides where needed for
  `vec4`-friendly access.

## WGSL kernels (Phase 1, heat)

| Kernel | Role |
|---|---|
| `assemble` | Matrix-free element contribution from $k$, geometry, quadrature |
| `matvec` | $y = A x$ without forming $A$ (element loop + scatter/gather) |
| `dot` | Partial dot products + host reduction for PCG scalars |
| `axpy` | $y = \alpha x + y$ vector updates |
| `jacobi` | Diagonal (Jacobi) preconditioner apply; AMG via Ginkgo later |

The **PCG loop itself is host code**: it orchestrates kernel launches
and reads only the scalar reductions (`dot` results) per iteration.
All vectors stay GPU-resident; the PCIe bus is touched once to upload
mesh/BCs and once to download the solution.

## Precision strategy

- Compile-time precision selection (FP32 dev path + FP64 iterative
  refinement; FP64 production path where hardware supports it).
- WGSL `f64` availability is backend-dependent and is an open
  question — kernels must not hard-code assumptions about it.

## Fallback (no GPU)

If `wgpu` device creation fails (CI, headless, unsupported driver),
the identical algorithms run on CPU:

- Sparse assembly + PCG via `faer` / `nalgebra-sparse`, parallelised
  with `rayon`.
- Same mesh format, same `.vtu` output, same convergence tests, so
  Phase 0 (pure CPU) remains the correctness reference for Phase 1+.

Dispatch is a runtime backend choice behind a small trait
(`GpuBackend` vs `CpuBackend`); physics code never branches on backend.

## Crate layout (`fea/`)

```text
fea/
  Cargo.toml            # standalone workspace (not part of root crates/*)
  crates/
    mesh/               # structured hex mesh + meshio/Gmsh ingestion types
    solver/             # physics: heat (Phase 0), elasticity (Phase 2)
    gpu/                # wgpu/WGSL backend + CPU fallback dispatch
```

`fea/` is intentionally **excluded from the root workspace**
(`members = ["crates/*", "apps/*", "xtask"]`), so the CAD build and
its `cargo check` are unaffected until FEA is promoted.

## Open questions

- CSR vs. element-matrix caching inside `matvec` for elasticity
  (Phase 2): memory vs. recompute trade-off on target GPUs.
- Reduction strategy for `dot` at 500k+ DOF (two-pass vs. atomics).
- Whether a WGSL Jacobi preconditioner suffices, or Ginkgo AMG (FFI)
  is needed from Phase 1 onwards.
