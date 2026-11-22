# query.poly (`SmPolyBrep` and its poly objects, 2026-10-14)

- **Harness builds the poly B-rep directly.** Upstream's poly B-rep constructors and
  `StitchEdgeTopological` are outside the in-scope call graph. The shim builds the B-rep
  with the real constructors (`SmPolyBrep`, `SmPolyRegion`, `SmPolyShell`, `SmPolyFace`,
  `SmPolyVertex`, `StartPolyEdgeLoop`, `AddPolyEdge` without an end vertex, so no
  stitching, `FinishPolyEdgeLoop`, `SmCPolyFace`, `SmPolyVertAuxData`) and sets the radial
  rings, the vertex list order and `m_bOKBackPtrs` directly. To do that it includes
  `SmPoly.h` with `protected`/`private` defined as `public`, because the classes are
  `final`. The Rust side uses the construction mirrors in `query::poly`. Every case dumps
  the whole structure, so a construction difference would show up as a failure.
- **Default vectors are `SM_UNDEF_DOUBLE`.** `SmVector3d` has default member initialisers,
  so a face built without a normal starts with an undefined `m_vNormal` and
  `m_vSphereCenter`, and a degenerate face then caches `(1,1,1)`.
- **`SmPolyVertex::SetTolerance` always stores.** `#define cbi` sits just above the
  function, so a tolerance more than 1000 times the old one is signalled and still
  stored. The first port refused it, and the generated `Transform` cases with scale 2000
  caught that.
- **Deleted objects.** Upstream poly objects live in a calloc'ed arena that is not freed
  on `delete`, so code reading a deleted face still sees its cleared members. The port
  uses empty slots instead: `SetPoint` skips deleted faces, and `GetOKBackPtrs` /
  `IsDegenerate` treat a deleted face as being in no shell and having no original face.
  The FFI wrapper rejects mutations that touch an already deleted object.
- **Deferred.** `IsDegenerate` of an edge whose face has an original face while the
  back pointers are valid needs `SmFace::GetSurface` (`topology.face_p2`). The shim
  skips that call because the labels are fake pointers, and the case defers.
- **Nested composite faces rejected (review fix).** `SmCPolyFace::AddPolyFace` accepts any
  face, including a composite or the composite itself, and the virtual `GetPolyEdges` /
  `GetNormal` / `CalculateBoundingBox` / `GetOKBackPtrs` of a composite would then recurse
  without bound (the Rust port overflowed the stack on `add_cpoly_face(f) -> c;
  cpoly_add_poly_face(c, c)`; on wasm that corrupts memory). Upstream only ever adds shell
  faces (`SmLineSegClass.cpp:1879-1883`), and adding a composite would also overwrite that
  composite's own `m_pCPolyFace`. `add_cpoly_face` and `cpoly_add_poly_face` return
  `InvalidInput` for a composite `face`; the harness never builds nested composites, so
  difftest is unaffected.
- **Adversarial fixtures.** `fixtures/query.poly/adversarial.json` (59 cases: bow-tie,
  collinear, 1-/2-edge and open loops, given/wrong normals, holes, faces at 1e150/1e8/1e-150
  scale, struts, non-planar faces at the tolerance limits, degenerate/undefined/skew
  transforms, degenerate edges with zero/negative tolerances, empty faces and B-reps,
  composites) all pass bit-identically.
- **Not compared through FFI.** `sm_IsUniformPositiveScale` and
  `sm_ReportInvalidTransformInput` are in an anonymous namespace of `SmApiGeneral.cpp`;
  they have unit tests only. The `GetTypeString` / `GetClassString` strings are also
  covered by unit tests only.
