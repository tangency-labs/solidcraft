# SolidCraft FEA — Roadmap

Planning document only. No benchmarks or test results are claimed here;
pass criteria below are targets to be measured, not results.

## Phase 0 — CPU reference heat solver (4–6 weeks)

- **Inputs:** structured hexahedral mesh generator (in-house for tests
  only, not a production mesher), Gmsh/`meshio` ingestion for external
  meshes later; material properties (scalar conductivity $k$), Dirichlet
  + Neumann boundary conditions; analytic manufactured solution.
- **Outputs:** CPU reference steady-state linear heat solver
  (Galerkin FEM, assembled sparse system, PCG), `.vtu` export readable
  in ParaView, mesh/convergence test harness.
- **Benchmark test:** 1D/2D/3D manufactured solution; mesh-refinement
  study measuring $L^2$ error vs. $h$.
- **Pass criteria:** observed convergence order matches theory for
  linear hex elements (2 in $L^2$ for temperature); solver residual
  reduced to the requested tolerance on all test cases; results
  reproducible in CI without a GPU.

## Phase 1 — wgpu matrix-free PCG for heat (8–10 weeks)

- **Inputs:** Phase 0 mesh, element data, and BCs; GPU device via
  `wgpu` (Vulkan/DX12/Metal-first); FP32 + FP64 iterative refinement
  for development.
- **Outputs:** matrix-free WGSL kernels (assembly, matvec, dot, axpy),
  host-side PCG loop, GPU-resident solution, CPU fallback path.
- **Benchmark test:** 500k DOF heat problem, GPU PCG vs. CPU PCG
  (Phase 0) on identical mesh/BCs; count PCIe transfers per solve.
- **Pass criteria:** GPU solution matches CPU reference within the
  precision policy tolerance (FP32+refinement in dev); exactly one
  H2D and one D2H transfer per solve (no per-iteration copies);
  speedup vs. CPU PCG reported with hardware named — no invented
  numbers if hardware is unavailable.

## Phase 2 — Vector linear elasticity (implicit PCG)

- **Inputs:** Phase 1 operators; 3 DOF/node; elasticity tensor
  $D(E, \nu)$; body forces and traction BCs in addition to Dirichlet.
- **Outputs:** element stiffness $k_e = \int B^T D B \, d\Omega$ via
  the same matrix-free assembly path; displacement/stress/strain
  recovery; `.vtu` output.
- **Benchmark test:** cantilever beam and a NAFEMS elasticity
  benchmark (e.g. NAFEMS II.1-series) compared against CalculiX /
  Code_Aster run externally on the same mesh.
- **Pass criteria:** displacement/stress agree with the external
  oracle within the benchmark's published tolerance band; patch test
  and rigid-body-mode tests pass; no contact or plasticity exercised.

## Phase 3 — Explicit elastodynamics

- **Inputs:** Phase 2 spatial discretisation; mass lumping; stable
  time-step estimate ($\Delta t \le 2/\omega_{\max}$).
- **Outputs:** explicit central-difference integrator, GPU-resident
  throughout; energy-balance diagnostics.
- **Benchmark test:** 1D elastic wave / vibration benchmark against
  analytic solution and an external oracle.
- **Pass criteria:** total energy drift within tolerance over the
  benchmark duration; stable at the computed CFL-limited step;
  results match analytic frequency within tolerance.

## Cross-cutting

- NAFEMS benchmarks added to CI as external-oracle comparisons
  (CalculiX / Code_Aster as separate processes, never linked in).
- Every phase: `.vtu` output + ParaView check; convergence/residual
  reported, not just final fields.
- NO implicit contact/plasticity in any MVP phase.

## Open questions

See `CONTEXT.md`: WGSL FP64 availability, Gmsh FFI vs. `meshio`-only,
Ginkgo FFI timing, first benchmark GPU vendor.
