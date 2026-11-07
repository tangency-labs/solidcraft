# SolidCraft Simulation — Roadmap

Planning only; no benchmarks or results claimed. Each phase ends with
a result viewable in the SolidCraft viewport (and `.vtu` export).

## Phase 0 — CPU reference heat solver in `crates/sim` (4–6 weeks)

- **Inputs:** a SolidCraft solid (kernel B-rep) mesh — structured
  hex for tests, Gmsh hex for real parts; material $k$ from the
  document; Dirichlet/Neumann BCs as engine commands.
- **Outputs:** steady linear heat solve on CPU, results written back
  to the document/viewport, `.vtu` export.
- **Benchmark:** manufactured solution, refinement study ($L^2$ vs $h$).
- **Pass:** convergence order matches theory (2 for linear hex);
  residual to tolerance; runs in CI without GPU.

## Phase 1 — GPU matrix-free PCG for heat (8–10 weeks)

- **Inputs:** Phase 0 mesh/BCs; `wgpu` device via `crates/gpu`.
- **Outputs:** WGSL assembly/matvec/dot/axpy kernels, host PCG loop,
  GPU-resident solution, CPU fallback.
- **Benchmark:** 500k DOF vs CPU PCG, identical mesh; PCIe transfers
  counted separately as (a) bulk field transfers and (b)
  per-iteration scalar `dot` readbacks required by the host PCG loop.
- **Pass:** matches CPU reference within precision policy (WGSL is
  FP32-only; see CONTEXT precision section); bulk transfers are
  1 H2D + 1 D2H per solve, with per-iteration scalar readbacks
  reported (not hidden in the bulk count); speedup reported with
  hardware named.

## Phase 2 — Linear elasticity (static stress)

- **Inputs:** 3 DOF/node, $D(E,\nu)$ from document materials,
  $k_e=\int B^TDB$; body forces + tractions.
- **Outputs:** displacement, von Mises stress, strain — shown in
  viewport with legend.
- **Benchmark:** cantilever + NAFEMS benchmark vs CalculiX /
  Code_Aster / ANSYS run externally on the same mesh.
- **Pass:** within oracle tolerance band; patch/rigid-body tests pass.

## Phase 3 — Explicit dynamics + modal/thermal studies

- Explicit dynamics: lumped mass, CFL step, energy diagnostics.
- Modal and transient thermal as study types reusing Phase 1–2.
- **Pass:** energy drift and frequency within tolerance vs analytic
  and external oracle.

## Cross-cutting

NAFEMS in CI · every phase: viewport check + `.vtu` · no contact /
plasticity in MVP · ANSYS/Fusion code never enters this repo.

## Open questions

See `CONTEXT.md` (volume meshing, native-FP64 backend, Gmsh licence
route, result-to-viewport mapping).
