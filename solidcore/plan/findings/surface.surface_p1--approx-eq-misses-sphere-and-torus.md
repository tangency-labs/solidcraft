# surface.surface_p1: `Surface::approx_eq` never matches a sphere or a torus (2026-10-26)

- **Unit:** `surface.surface_p1`. This is a port bug on main and not an upstream defect, so it is not recorded under `plan/upstream-issues/`.
- **What is missing:**
  - `Surface::approx_eq` (`crates/solidcore/src/surface/surface/p1.rs:1318`) ports the virtual `SmSurface::operator==`. It dispatches on the pair of kinds, with arms for CurveBounded, Step, OffsetSurface, BSpline, Plane, Cone, SurfOfRevolution, SurfOfExtrusion and SrfInVolume.
  - It has no arm for `SurfaceKind::Sphere` or `SurfaceKind::Torus`, so two equal spheres or two equal tori fall to `_ => Ok(false)`. Only a pointer-identical pair returns true.
  - Upstream overrides `operator==` for both kinds (`SmSphere.cpp:116`, `SmTorus.cpp:124`), and the port already has them as `Sphere::approx_eq` (`surface/sphere.rs:512`) and `Torus::approx_eq` (`surface/torus.rs:537`). They just aren't reached from the dispatcher.
- **Effect:**
  - Any comparison of two distinct sphere or torus objects through `Surface::approx_eq` answers false where upstream answers true.
  - At v1.0.1, `SmBrep.cpp:12270` (`sm_GetNaturalSeamInterval`; port `topology/brep/p1.rs:2180`) compares an edge's `SmCrvOnSurf` base surface with the face surface. The Rust `CrvOnSurf` holds its own copy of the surface, so the comparison is never pointer-identical.
  - On a sphere or torus face the curve is therefore never recognised as a natural seam. The 1.0.1 nonseparating-seam branch of `MakeEdgeInFaceWithVU` (insert a seam edge as a new loop without a new face) never fires there, and the edge splits the face instead. A torus is the main surface that is periodic in both directions without poles, so it is the case that branch was written for.
- **How found:** the pin101 faithfulness review of fixer-5's `topology.brep_p1` / `topology.brep_p9` port (2026-10-26). I confirmed it by reading the dispatcher on `origin/main` and on pin101. It has not been reproduced with a difftest case yet.
- **Fix:** add two arms that defer to the existing kind comparisons, as the other analytic kinds do:
  `(SurfaceKind::Sphere(a), SurfaceKind::Sphere(b)) => Ok(a.approx_eq(b))` and
  `(SurfaceKind::Torus(a), SurfaceKind::Torus(b)) => Ok(a.approx_eq(b))`.
  A sphere/torus face test through `MakeEdgeInFaceWithVU` (or a `topology.brep_p1` seam-interval case) should confirm the seam branch is taken.
  Applied in the v1.0.1 pin work (2026-10-26) with the unit test `approx_eq_compares_spheres_and_tori_by_their_own_operator` (`surface/surface/p1.rs`): equal spheres and equal tori compare true; a sphere against a torus, and pairs with different radii, compare false. The seam-branch case is still to be added.
