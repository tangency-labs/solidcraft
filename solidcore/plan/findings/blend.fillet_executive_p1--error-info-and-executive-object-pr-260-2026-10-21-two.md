# blend.fillet_executive_p1, error info and executive object (PR #260, 2026-10-21; two harness bugs, no port disagreement)

- **Corpus.** 61 cases (45 fixture, 16 generated): the 51 law cases plus scripts of
  `NoteError` calls on a standalone `SmFilletErrorInfo`, and on a real
  `SmFilletExecutive(ctx, target)` scripts of the `NoteFilletError` overloads (one on a
  real `SmFilletEdge` in the pseudo B-rep), the setters (`SetDoGlobalMerge`,
  `SetDoTrimming`, `SetSelfIntersectionHandler`, `AddFilletSurfaceGenerator`,
  `AddTopoEdges`, `RemoveFromTopoEdges`) and the corner/solver maps and lookups. Other
  handles become pointers the shim never dereferences; the executive's generator list is
  emptied before its destructor would delete them. Short texts include non-ASCII
  characters; long ones stay ASCII (a cut inside a UTF-8 sequence is a deviation). 61 pass
  at max_rel_err 0; 500 seeds 545 pass; `--perturb` SENSITIVE 61/61; `--fuzz 2000`
  FUZZ-PASS.
- **Text.** `smos_sprintf` is `snprintf(buf, SM_TBLOCK_SIZE, ...)`: comments and names keep
  1023 bytes. Every overload but the generic one prints a null comment through `"%s"`,
  undefined in C; glibc prints `(null)`, and so does the port (fixture
  `error_info_null_texts_print_null`). `SmFilletEdge::GetName` writes nothing for the fillet
  edge types it has no name for, so `NoteFilletError(eErrorType, pFilletEdge, ...)` would
  print an uninitialised buffer: the port returns `InvalidInput` instead, and the scripts
  pass names directly.
- **Uninitialised members.** `set()` leaves `m_pFilletVertex1/2` and `m_vLocation` unset; the
  scripts compare them only after a call sets them.
- **Harness bug 1 (co-port of #244).** `ErrorInfo::default` had edge type 0
  (`SM_FE_BLENDING_RAIL`); `set()` writes `SM_FE_UNKNOWN` (14). Every script caught it; no
  p3 output reads the field.
- **Harness bug 2 (`blend.fillet_executive_p2` runner, #267).** Its `SmFilletSolverType`
  table mapped codes 1, 2, 4 to `ConstRadius`, `ConstDist`, `SurfSurf`; upstream
  (`SmFilletSolver.h:47`) has `SURF_SURF`, `CONST_RADIUS`, `CONST_DIST`. The shim passes the
  raw code, so the two sides built different solvers. While `NoteError` was unported, every
  case where that mattered deferred; once wired, `set_params_per_edge_r4` failed (C++
  `SM_SUCCESS` with constant-distance solvers, Rust "invalid radius type"). Fixed: p2 is
  274 cases, 258 pass, 0 failed, 16 deferred.
- **Not ported yet.** The stubs in `blend.fillet_solver_p1`, `blend.fillet_corner_p3` and
  `blend.fillet_geom_p4` reach the executive through their own typed ids, not a
  `FilletExecutive<O>`; wiring them waits for the shared object model (PR #192).
  `blend::fillet_corner::p3::FilletExecutiveData` holds the same edge-use-to-solver map as
  `FilletExecutive::edgeuse_solver_map`.
