# Kernel-aware Surface dispatch revived on main (2026-10-20, issue #76)

Branch `refactor/kernel-surface-dispatch` merges #133 (Karl-VonRichter, head `88596f8e`)
into main `862746f4` and keeps its design; the two sections below are #133's own
records.

- **Where the Kernel is threaded.** `Surface::evaluate`, `evaluate_point`,
  `evaluate_simple`, `evaluate_ignoring_status`, `copy`, `copy_and_add_analytics`,
  `max_analytic_domain`, `is_periodic`, the degeneracy and normal queries, the
  `SurfaceEvaluate` / `CurveEvaluate` traits, `IsoCurve`/`Curve` evaluation and the
  surface solvers (`SpGlobalSolver`, `LsiGlobalSolver`, `CsiGlobalSolver`,
  `SurfPtSolveStepper`) take or hold the caller's `&Kernel`. Everything main added
  after #133's base (`OffsetSurface`, blend, boolean, offset, heal, query.face,
  sphere, surface p8/p10/p11) was threaded the same way: no production path builds a
  `Kernel::default()`. The B-rep boolean classification uses the source operand's own
  kernel (`BooleanKernels`), not an extra parameter.
- **Shape for the next kinds.** One `SurfaceKind` variant per concrete class
  (`Cone(Box<Cone>)` holds `SmCone` and `SmCylinder`, distinguished by `type_code`);
  every `Surface` method is one `match` with one arm per kind; inherited
  `SmBSplineSurface`/`SmSurfOfRevolution` bodies are reached through the kind's
  `rev().base()`. Adding `Sphere`, `Torus` or `SurfOfExtrusion` is: a variant, a
  `From` impl, one arm per method (the compiler lists every match), and replacing the
  `DISPATCH GAP` in `Surface::read_from_db` and the recogniser chain.
- **`Result<Option<Surface>>`** for `create_offset_surface`: `None` is upstream's
  success with a `NULL` surface (a cone whose offset radii both collapse).
- **Wired here** (cone_p2/cone_p3 are green/partial on main): `CopyAndAddAnalytics`
  returns the recognised cone; `Reverse`, `SwapUV`, `Reparameterize` of a cone;
  `IntersectWithPlane` / `GlobalSurfaceIntersect` of a cone and the `SmCone_TYPE`
  cases of the revolution, sphere and cone intersections; `sm_TestAnalyticSurface`'s
  `IsNurbSurfaceCone`.
- **Still gaps (named `surface.surface_p2` / `surface.surface_p9`)**: cone
  `DropPointFast` and `TrimWithDomain` (inherited revolution bodies need the cone's
  virtual callbacks), the part 9 iso/tessellation helpers that store B-splines,
  `cone_p3`'s coincident-boundary adapter (it reaches the part 9 gap), sphere / torus /
  extrusion variants, and a base-only `SmSurface` variant (WR-196, #245).
- **Runner note.** `intersect.surface_intersector_p4` records an unset curve type as
  the shim does, `(double)(SmTsectCurveType)-7` = `2^32 - 7` (same fix as #270).
