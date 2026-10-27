# Modelled: the lazy NURB that upstream const queries keep (2026-10-27)

Updates: plan/findings/surface.plane_p1--smplane-part-1.md

The two entries "Lazy NURB through `const_cast`" and "Port deviation:
`natural_uv_domain` does not cache the NURB" are resolved for planes and surfaces of
extrusion.

- **Kernel.** `BSplineSurface` keeps its NURB in a `NurbSlot`: a `OnceLock` behind an
  `Arc`. A read-only query fills it once from `&self`, as upstream's
  `GetOrCreateGwNurbPointer` keeps a NURB that a `const` method makes through a
  `const_cast`. `&mut` access detaches a shared cell first (copy on write).
  - `Plane::base_with_nurb` and `SurfOfExtrusion::base_with_nurb` (so
    `natural_uv_domain`, the boxes, `Surface::kind_with_nurb`) make a missing NURB and
    keep it.
  - `Plane::copy` (the copy constructor) makes the source's NURB first and the source
    keeps it.
  - Evaluation copies (`Surface::clone_for_evaluation`, `clone_exact_bounded`) stand
    for upstream's pointer to the one object, so they share the cell: a NURB made
    through a copy stays on the surface.
- **Observable effects, now as upstream.** `IntersectWithPlane`'s "has a NURB" test
  (`SmPlane.cpp:2183`), `ToggleSwapUVBit` (leaves the NURB alone), `operator==`
  (compares NURBs), and `SmSurfPtGlobalSolver`'s `m_pBSplSrf` test at construction.
- **Port bookkeeping must not make it early.** Where the port reads a natural domain
  that upstream does not read, a lazy plane's domain now comes from a NURB made on a
  copy (`Surface::stand_in_key_domain`, `Plane::natural_uv_domain_without_keeping`).
  This covers the stand-in object key for `this` in point-solve solutions. Without it,
  `surface.surface_p4`, `surface.surface_point_solve_p2` and `_p3` disagreed: the solver
  saw a NURB that the C++ one does not.
- **Harness.** The `surface.plane_p1` shim asks `const` queries of plane A itself, with
  no side-free copy. Its state records whether the plane holds its NURB.
  `ToggleSwapUVBit` and `operator==` are generated anywhere in a program. The unit passes
  2705/2705.
- **Remaining.**
  - Cone, cylinder, sphere, torus and surface of revolution still keep the made kind in
    the `Surface` side memo (`NurbMemo`). Their `MakeNurb` also rebuilds the polar
    converter and may trim the generator, so `kind()` readers and copies of those kinds
    do not see the NURB.
  - An extrusion's in-place `MakeNurb` also adjusts its generator's STEP interval (and
    makes a lazy line generator's NURB). The filled slot keeps only the NURB.
  - Copies of extrusions cannot make a missing NURB first, because `MakeNurb` needs the
    `Kernel`.
  - Lazy curves (`Line::base_with_nurb` and the conic kinds) still make theirs on a
    copy.
