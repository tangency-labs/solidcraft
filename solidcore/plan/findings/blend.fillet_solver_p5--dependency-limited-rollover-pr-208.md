# blend.fillet_solver_p5 — dependency-limited rollover (PR #208)

- `ProcessRollOver` compares the blend early return, NER for missing rail
  classifications/corners, two classified single-interval rails, and the rejection
  of unsupported full-rollover corner types. Every compared case observes the
  post-call geometry type, interval endpoints/classes and the ordered adjusted
  corner list (including existing duplicates). Interval scale cases change stored
  classification parameters; they do not establish large/small fillet geometry
  coverage. Statuses/enums/counts/identities compare as integers.
- `two_real_rails_classified_on_rectangle` constructs real `SmLine` rails inside a
  real planar face. C++ finishes with `SM_SUCCESS` and one `SM_PC_FACE` interval on
  each rail. Rust installs the first classification on its rail, then the actual
  `query.face_p2::curve_on_classify` stops at `intersect.curve_class_p2::ReSet`.
  This is deferred, not a comparison of the completed classifications. The native
  target is exported/imported using the existing edgeuse graph format.
- `multi_or_empty_intervals_2_1` (and the other non-[1,1] interval counts) reaches
  `IsConvexRadialSector(10, TRUE, 0.0)`. Its wire edgeuse returns an upstream soft
  binormal error (the Boolean SER returns true), then C++ returns
  `SM_ERR_NULL_POINTER` for the absent fillet surface. Rust stops at the existing
  `topology.edgeuse_p3::EvaluateBinormal` dependency. No analytic detection, splitting,
  tangent/cliff repair, corner adjustment or surface rebuild is compared.
- Supported full rollover needs real rail/face geometry that the process fixture
  does not construct. Exactly that reachable incomplete-fixture branch is rejected
  by both guards; blend/known-class/missing-class/unsupported-corner paths stay
  accepted. These are fixture limitations, not claims that valid upstream full
  rollover inputs are undefined. A Rust unit test checks lazy corner resolution,
  resolver-error propagation, retained caller output and the p3 recovery deferral.
- The API-level `TestSmCircularFillet`/`TestSmFilletEdges` inputs need the full fillet
  executive and topology construction; this unit does not claim those end-to-end
  fillet outputs. Solver and pseudo-B-rep records reuse PRs #192/#147. The latter's
  shared-solver integration uses the base GetSurface path (including its existing
  geometry-redirection boundary), not a second copy of solver data.
- A guard-fuzz probe with rail 1 interval `[0, -1e300]` found a fixture-builder
  mismatch: C++ `SetMinMax` rejects the assignment and preserves the default
  inside-out extent, while the Rust fixture's two-end constructor reordered it.
  The fixture now calls the matching setter and preserves its ignored-error state;
  `reversed_interval_preserves_initial_extent` is the regression. No kernel
  tolerance or upstream interval implementation was changed.
- Native fixture parsing is inside the exception boundary, including its small
  vector allocations. The standalone regression in
  `harness/cpp/tests/blend_fillet_solver_p5_alloc.cpp` forces the first allocation
  to throw: before this fix the exception escaped; afterwards the shim returns
  `SC_ERR_EXCEPTION` (-1000). This test is linked separately so its allocator
  override is never part of the FFI library.


### PR #208 follow-up: complete caller continuation and native containment

The original prefix-only port was incomplete, despite passing its limited gate.
The caller bodies now continue through their final return. The following mapping
was checked against the active pinned upstream source (debug drawing excluded):

| Upstream `SmFilletSolver.cpp` | Rust continuation |
|---|---|
| 5714–5752 | geometry-list snapshot; new classification installed before fallible `CurveOnClassify` |
| 5765–5810 | original reverse interval walk; start/end `ConnectIntervals` direction; descending interior-short list |
| 5811–5822 | full refinement arguments; warning-only status with retained mutations; process each geometry |
| 7161–7254 | blend skip, ordered rail NERs, corner restriction, rail 0 then rail 1 full recovery |
| 7256–7327 | convexity, analytic test, lifetime of original rail/UV/surface copies |
| 7329–7450 | segment state, saved offsets, side search, adjacent face, tangent offset orientation/type |
| 7452–7504 | parameter comparison at exactly 1e-3, index increments, no-side early returns |
| 7506–7622 | last segment; cliff rail vs tangent geometry splits; mates; base faces; saved offset copy/output even on ignored error |
| 7624–7744 | rail intersections; recomputation after a previous split; cliff support construction and side geometry |
| 7747–7802 | ordered AddUnique of adjusted corners; mate classifications; solver non-convergence |
| 7805–7867 | delete surface; global solve; derivative direction; rebuild; support edges; trim; restore tangent offsets |
| 7869–7899 | update current geometry/rail handles and split flags; final return |

`p5/recovery.rs` also supplies typed adapters for p1's `sm_SplitRail` and
`sm_ComputeSupportEdge`. Their full bodies retain the continuation after
`MakeFilletEdge` and `sm_SrfSrfIntersection`. Curve installation reuses the real
pseudo-rail notification reset, updates `Curve::owner` with caller-resolved object
keys, and routes owner-change notifications through `RolloverOwnership`. Curve
and surface solution handles retain their real natural domains, reusing p1's
`CliffHandle` rather than an opaque `NoObject`. The offset copy takes the caller's
Kernel; the support intersection preserves explicit null optional arguments and
`bOptSkipTwoPntsIntersection=FALSE`. New
solver-local geometry handles are distinct from arena ids. The segment loop has
a checked progress budget of `8 + 4 * total_intervals * (target_edgeuses + 1)`;
a non-progressing corner callee returns `InvalidInput` instead of looping forever.

Fourteen new kernel regressions cover full-rollover call order and early failure,
tiny end intervals with explicit `bAllowRemovalOfEnds=TRUE`, interior-short
ordering, refinement argument transfer and
warning-only mutations, two-segment tangent rebuild/offset restoration, retained
partial output before failed rebuild, ignored Copy status with usable output,
cliff support ordering, stalled corner traversal, empty classes, typed split
backlinks/endpoints/curve ownership notifications, support-intersection failure, and the complete successful support-curve
intersection/trim/installation path.
These tests script unavailable callees to exercise the real caller; they do not
establish Rust-versus-C++ repair parity. The native gate remains 86 cases:
78 passed / 0 failed / 6 deferred / 2 rejected, with the same dependency boundaries.

The pre-fix native audit found 5184 bytes leaked for a simple process fixture and
21872 bytes for the nonempty TestRollOver fixture. Fixture vertices/rails now have
explicit RAII owners; rail curves receive their actual native owner; TestRollOver
is not seeded with classifications it would overwrite and leak. The standalone
ASan/LSan regression calls the in-process implementation directly, so process exit
cannot hide leaked allocations. It checks process, empty/nonempty TestRollOver and
the native rectangular target, with real destruction and zero reported leaks.
The upstream libraries are the normal reference build; the shim and standalone
regression are instrumented, not all SMLib source files.

The pre-fix calloc sweep produced 9 SIGSEGV and 15 SIGABRT in 200 probes, including
`CreateRectangle` cleanup and the allocating `SmBrep` destructor. A catch around the
fixture cannot intercept either. Both exported rollover functions now run all
native construction, evaluation and destruction in a Unix child with bounded IPC
and a 30-second stalled-read timeout. Parent bookkeeping uses bounded stack arrays;
no SMLib objects are made in the caller. Child signals, aborts, exceptions and failed
protocols become `SC_ERR_EXCEPTION`, not parity statuses or rejected fixtures. The
parent reaps its own worker and kills only that worker on failed/stalled IPC. On a
non-Unix platform the fixture fails closed. This changes harness isolation only;
no upstream algorithm or kernel tolerance is modified.

Reproduce the committed native regressions (reference libraries must exist):

```sh
SOLIDCORE_CPP_DIR=/path/to/bin/cpp python3 harness/cpp/tests/run_blend_fillet_solver_p5_safety.py
SOLIDCORE_CPP_DIR=/path/to/bin/cpp python3 harness/cpp/tests/run_blend_fillet_solver_p5_safety.py --asan
```

The sweep runs 1200 combinations of `new`/`calloc` allocation failure across both
exports: zero host crashes; 843 successful calls and 357 contained native errors.
Explicit native SIGSEGV and SIGABRT are contained as well. Rust FFI regressions
cover raw-parser truncation, empty/short output buffers and repeated concurrent
workers. Native operation failures remain distinct from fixture rejection.
