# blend.fillet_solver_p4 (harness)

- **ND-3 (`sdStaticStep`) in the difftest.** The function static of `ComputePointValues` lives
  in libSMLib and is not linkable (local symbol), and earlier units of a `--green` run
  (`blend.fillet_intersector`) may already have latched it. The shim's probe
  (`sc_blend_fillet_solver_p4_static_step`) calls the library's `ComputePointValues` once on a
  fixed finite-difference crossing point (cubic sheet `(u, v, u^3)` against the plane
  `y = 0.25`, zero offsets; this latches the static with `1e-5` if nothing has) and replays the
  finite-difference block with `1e-3` and with `1e-5` through the public `StepSolve`; exactly one
  replay reproduces the library's curve tangent bit for bit. The runner probes before every
  case that can step (finite-difference solver, incoming `SM_IP_TANGENT_POINT`, or a previous
  point) and seeds `Kernel::fillet_point_step`. Run order (C++ or Rust first) does not matter:
  either the case or the probe latches the static before the probe reads it.
- **Surfaces of revolution, not the SmCylinder/SmCone/SmSphere/SmTorus classes.** The Rust
  port has only `SurfaceKind::SurfOfRevolution`; `SmSphere` overrides `EvaluateNormal` (used by
  `ComputeSurfaceValues`' degenerate-normal fallback) and has no Rust counterpart yet. The point
  cases therefore build cylinder, cone (also with its apex on the axis), sphere and torus shapes
  as real `SmSurfOfRevolution`s from line / circle generators, as unit
  `surface.surf_of_revolution_p1` does; poles and apexes are covered through them.
- **CalcFilletGeom without the executive pipeline.** The only safe standalone paths are the
  ones before the first rail: a non-B-spline edge curve (`SM_ERR_NULL_POINTER` at :1504) and a
  bare `SmFilletGeom` without rails (`NER(pThisRail)`, or `SM_ERR` from a failed
  `sm_FindFilletTsectPnt` at a G1/split knot first). `SetExtensionFlags` is overridden on both
  sides (the base dereferences the unset executive). An empty `m_vFilletGeoms` with a B-spline
  curve and a `NULL` `m_pEdgeuses[0]` are undefined behaviour upstream and rejected by the FFI.
  Full pipeline cases (executive, corners, pseudo-B-rep import) are left until p2/p3 land:
  every such case would defer (`topology.edgeuse_p3` is wired since the rebuild on main).
