# SolidCraft FEA (scaffold)

Clean-room, GPU-first FEA/CFD solver in Rust — planning scaffold only.
See `../docs/fea/CONTEXT.md`, `../docs/fea/ROADMAP.md`, and
`../docs/fea/ARCHITECTURE.md`.

This is a standalone Cargo workspace, deliberately **not** part of the
root `crates/*` workspace, so the SolidCraft CAD build is unaffected.

Crates:

- `crates/mesh` — structured hex mesh types (+ future meshio/Gmsh ingestion)
- `crates/solver` — physics (Phase 0: CPU reference heat solver)
- `crates/gpu` — wgpu/WGSL backend + CPU fallback dispatch (Phase 1+)

```sh
cargo check
cargo test
```
