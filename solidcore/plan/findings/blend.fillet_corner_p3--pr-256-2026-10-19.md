# blend.fillet_corner_p3 (PR #256, 2026-10-19)

- **Stack.** Built on PR #252 (`blend.fillet_geom_p4`, which merges #147 `blend.fillet_geom_p1`
  and #192 `blend.fillet_solver_p1`): the corner operations run on p1's `FilletBrep` and
  its runner/shim `Model`. The corner-operation shim is `blend_fillet_corner_p3_static.inc`,
  included at the end of `blend_fillet_geom_p1.cpp` (as p4's), because the `Model` lives in
  that translation unit's anonymous namespace.
- **Intersection-point user pointers.** `SmFilletNx2Corner::AdjustTangentCorner` reads
  `SmTsectPnt::m_apUserPointer[0..2]` of the rail vertex uses as `SmSurface*`; upstream
  sets them only in `SmFilletGeom::MakeRailEdgeuse` (`SmFilletGeom.cpp:10083-10091`). The
  Rust `TsectPnt` has no such member, so `FilletCorner::adjust_tangent_corner` takes them per
  vertex use in `CornerEnv::tsect_surfaces`. Whoever ports `MakeRailEdgeuse` should store
  the surfaces (a field on `FilletVertexuse`, or a map in the executive) and feed that map.
- **Coverage.** `Create` runs on real B-reps (upstream `.smb` files incl. 14 of
  `TestFiles/pt_TestFiles/Fillet`) and compares the corner type, the edge uses around the
  vertex, tolerances and flags for the 3x2 mixed-convexity, Nx2, 4x3, closed Nx1, no-corner
  and (all-linear) convex NxN outcomes; the 1x1, 2x2, Nx1, open, degenerate and concave NxN
  outcomes stop at `sm_TestG1Continuity` / `CheckNx1Degeneracy` (`blend.fillet_corner_p1`) or
  `sm_FindConcaveEdgeuses` (`blend.fillet_corner_p2`). Every corner patch builder reaches
  `MakeAnalyticCornerSurface` (p2), `CreateEndPatch` (p1) or `SmSurface::DropCurve` after its
  state checks, so only those checks (and the 3x2 curve join before them) are compared. The
  shim does not call upstream on deferred paths (it repeats upstream's code up to the
  unported call) and refuses inputs where upstream would dereference null (`-2`).
- **Review fixes (2026-10-19).** (1) `AdjustTangentCorner`'s `SmTemporaryChangeValue`
  enables out-of-bounds evaluation through `SM_CAST_PTR(SmBSplineSurface, pSrf)`, which also
  succeeds for `SmPlane` and `SmSurfOfRevolution`; the port first did it only for B-spline
  surfaces, so a plane user surface evaluated outside its domain was clamped in Rust but not
  in C++, flipping `bCloseEnough` (Rust returned early, C++ went on to split the fillet
  geometry). The shim's corner `Model` only built `SmBSplineSurface` faces, so the analytic
  branch was never compared; the shim now builds real `SmPlane` user surfaces (selectors
  1000/1001) and 60 adversarial Nx2 tangent cases (`review_plane_oob.json`, 6 of which failed
  before the fix) cover it. (2) The rail-curve check now returns at the first missing curve,
  as upstream's `||` chain short-circuits (before, a later NULL rail raised null-pointer).
  (3) The shim no longer refuses a failed `SmEdgeuse::NormalizedEvaluate`: `SmVector3d`'s
  members default to `SM_UNDEF_DOUBLE`, so upstream is well defined there and Rust mirrors it
  with `Vec3::UNDEF`. (4) The `SplitFilletGeomAtSideEdge` stub is tagged
  `UNPORTED(blend.fillet_geom_p6)`: the function is ported in `blend.fillet_solver_p1`, but on
  the solver's `FilletGeomRef` stand-in, and its first call `GetRailIndexOfVertexuse` is
  `blend.fillet_geom_p6`; wire the real call when that unit lands (deliberate exception to
  AGENTS.md 4a: the owning unit per `units.json` is `blend.fillet_solver_p1`, partial; wire it
  once the solver's port moves from `FilletGeomRef` to `FilletGeomId`). (5) superseded by (6).
- **Review fixes, round 2 (2026-10-19).** (6) `CreateCornerPatch` (3x2) calls
  `pCurve1->JoinWith(1, pCurve2, 0)`, which is virtual: `SmLine` and `SmEllipse` override it
  (`SmLine::JoinWith` rejects a bent or kinked other curve and uses a length-based gap
  tolerance; the B-spline join accepts both). The port joined the `SmBSplineCurve` parts, so a
  line on-extended edge ran the wrong join and left the line's analytic domain stale. It now
  dispatches on the curve kind (as `topology::brep::p5::join_copied_curves` does) and stores
  the whole joined `Curve` on the edge. The shim's pre-check had the same bug: it copied the
  curves with the non-virtual `Copy(ctx, SmBSplineCurve*&)`, slicing an `SmLine` into an
  `SmBSplineCurve`; it now uses the virtual `Copy(ctx, SmCurve*&)`. The corner model only
  built B-spline pseudo-edge curves, so `CornerIn::line_edges` now replaces listed pseudo-edge
  curves with real `SmLine`s on both sides, and `review_line_join.json` adds 48 adversarial
  cases (line on-extended edge against bent, kinked and straight B-spline or line on-edges,
  gaps 5e-4..1e-2 across both join tolerances, no original face, calc_geom, scales 1e-3/1/1e3).
  Before the fix 3 of them failed (C++ `SM_ERR` with 4 corner edges, Rust joined and stopped at
  the null original face with 3) and 18 were misclassified as deferred. `dump_curve`
  (`blend_fillet_geom_p1` runner) now exports the `SmBSplineCurve` part of the analytic kinds,
  matching the shim's `IsKindOf(SmBSplineCurve_TYPE)`.
- **Review fixes, round 3 (2026-10-20).** (7) The `SmEllipse::JoinWith` arm of the 3x2
  dispatch was never compared: the corner model built only B-spline and `SmLine` pseudo-edge
  curves. The reviewer disabled that arm (ellipses fell through to the B-spline join) and 15 of
  81 circle cases then failed against the C++ while every existing case still passed.
  `CornerIn::arc_edges` (`[edge, cx, cy, cz, r, d0, d1]`) now replaces a pseudo-edge curve with
  a real circle on both sides (`Ellipse::new_circle` / `SmCircle(center, X, Y,
  SmExtent1d(d0, d1), r, 3, &ctx)` + `GetOrCreateGwNurbPointer` + `SetCurve`). The FFI guard
  refuses exactly what the constructors assert on: `r <= SM_EFF_ZERO`, an end outside
  `[-360, 360]`, or a domain length not in `(SM_EFF_ZERO, 360 + SM_EFF_ZERO]`.
  `review_arc_join.json` (81 cases: circle on-extended edge against B-spline, bent, line,
  same-circle, radius/origin off by 5e-4/2e-3/1e-2 and cusp on-edges; line or B-spline
  on-extended edge against a circle on-edge; a negative domain; calc_geom; with and without an
  original face; scales 1e-3/1/1e3) and `review_line_join_tol.json` (108 cases: straight
  degree-1/3 B-spline on-extended edge joined to an `SmLine` on-edge, i.e. the B-spline
  `JoinWith` `IsLine` path; gaps straddling the length-based tolerance along and across the
  line, kinks 1e-4/3e-3/5e-3, L = 0.01 / 100, a folded collinear line; scales 1e-3/1/1e3) all
  agree with the C++. The 40 of them refused are successful joins without an original face
  (upstream then deletes harness objects; see below).
- **Join notes for later porters.** (a) When `this` is a B-spline and `other` an
  `SmLine`/`SmEllipse`, Rust runs the B-spline join on `other`'s `SmBSplineCurve` base, while
  C++ calls `other`'s virtual `EvaluatePoint` / `ApproximateLength` / `IsLine` overrides. The
  results differ at most at rounding level, and the review fixtures agree at 1e-9;
  `topology::brep::p5::join_copied_curves` does the same. Revisit if a later unit compares
  joined geometry tightly. (b) No successful 3x2 join result is compared: with an original
  face the case defers at `SmSurface::DropCurve`, without one the shim refuses. The shim could
  export the joined copy from its pre-check to compare the joined curve; not done yet. (c) On a
  failed join the Rust side discards its cloned curves, while C++ `CreateByJoining` may
  degree-elevate or convert the real edge curves in place before failing. Not observable with
  the current fixtures (the corner fails right after), but the porter of whatever continues
  after a failed join must mirror those in-place edits.
- **Known gaps.** No `CreateCornerPatch` implementation compares surface output (default
  patch stops at `MakeAnalyticCornerSurface`, Nx1 at `CreateEndPatch`, 3x2 at `DropCurve`).
  `create_rust` defers a whole `Create` case when a face around the filleted edges is neither
  plane nor B-spline (the import holds those faces only as their B-spline part), even when the
  classification alone would compare. The `bspline_surface()` stub tags
  `nurbs.b_spline_surface_n_lib_p2` (partial), following the `topology::brep::p1` precedent.
