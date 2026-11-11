# Skin annular caps: CreateFacesFromSurface resets its output list (2026-10-24)

Root cause of the two cases in "IsInnerShell on the B-rep with the kept trimmed caches"
above (primitive.primitive_creation_p2 `skin_concentric_loops`, record[7] cpp 1 rust 2;
sweep.api_surfaces `skin_primitive_annular`, faces[1] cpp 1 rust 2). It was not region
nesting. `SmPrimitiveCreation::CreateSkinFromFaces` reuses one `sNewFaces` list for every
loop layer and appends it to `rNewFaces` after each `CreateFacesFromSurface`, which
starts with `rNewFaces.ReSet()`. The port's `PrimitiveAccess::surface_faces` appended to
the caller's list instead, so the second layer (the inner tube) also re-reported the
outer tube's faces: 6 side faces instead of 4, with the outer tube's 2 listed twice.
`sm_CreateSkinPrimitiveInternal` takes one iso curve per side face at the skin's start
and end, so each cap got 3 loops (the outer circle twice). The duplicate circle lies on
the edge of the first copy (`Point3DClassify` gives `SM_PC_VERTEX` in both faces, and
the big face is used), and the start cap came out as 2 faces.

- **Fix.** `surface_faces` clears its output first, as `ReSet()`. The side face lists of
  every other caller were already fresh or unused (the swung-surface loop drops its list).
- **Result.** Both known-mismatch entries are removed and both cases match C++ bit for
  bit. sweep.api_surfaces is green (882/882).


### blend.fillet_executive_p1 — review alignment with single executive context (#665)

Removed the executive registry, identity tokens, foreign-store checks and the
registration-only load failure. `FilletExecutive` remains the one mutable state;
solver `FilletExecutiveRef` records only native back-pointer presence. The loader
sets it immediately after appending the solver. `VertexEntry` unifies pending
solver edgeuses and completed corners in one map with nullable vertex keys;
requesting a corner from a pending entry reports the native type-pun precondition
instead of returning a fabricated corner. Full production-session and corner
factory migration remain separate work coordinated with #641's author.

All 28 direct native loader cases now use one executive. Former cross-executive
cases construct distinct owned solvers; explicit repeat-load cases keep their
controlled fixture ownership. Snapshots compare native solver/vertex/pending
order, edgeuse identity, geometry, G1 knots and the actual back-pointer getter.
The p1 runner enumerates these cases; p2 only houses their shared shim helper.
The mutation review's p2 replay therefore did not run these 28 p1 records.
The unrelated legacy geometry executive-access stubs are unchanged from main;
`SurfaceSurfaceFillet` still lacks a direct successful-result harness operation,
so no coverage claim is made for a returned nonempty B-rep from that API.
