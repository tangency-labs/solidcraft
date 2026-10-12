# Findings

Rust-vs-C++ disagreements, upstream bugs, nondeterminism and other facts later agents
need. Add entries under a heading per unit (or "General"); keep the newest at the end
of each section.

## General

- **Nondeterministic upstream paths.** `SmFace::GetPointsInFace` (asked for more than one
  point) and `SmShell.cpp` call `srand(time(NULL))`. Those paths can't be reproduced bit for
  bit. Difftests must avoid them, or compare only results that don't depend on the random
  draw. The Rust port draws from `kernel.rng` instead. (Reported by Wave 0 agents B and C;
  see `plan/closure.md` for the other hazards: pointer-keyed maps, global tolerances, and
  statics set by the first call.)
- **NLib global error record.** NLib routines return a flag and store the error number in
  the process-wide `NL_ERROR.eno` (`N_ErrSet`). Shims must call `N_ErrClear()` before the
  call and read `NL_ERROR.eno` after it. On the Rust side this is
  `SolidError::Nlib { code: NlibCode, .. }` (added to `crates/solidcore/src/error.rs` with
  `nlib.knots`). Normalise both sides to `nlib_<number>` in runners.
- **JSON can't hold NaN or inf.** difftest inputs are JSON, so generated cases can't contain
  non-finite parameters (serde turns NaN into `null`). Test robustness to non-finite input in
  Rust unit tests. Outputs may still be non-finite.
- **difftest report `seeds` field.** It records the `--seeds` CLI value, not the number of
  generated cases a runner actually produced. `nlib.knots` overrides `cases()` to always
  generate at least 1200.

## nlib.knots (green; no disagreements)

2953 default cases and a 51,753-case run (`--seeds 50000`) all agree bit for bit with
upstream (`max_rel_err = 0`). Upstream behaviour worth knowing:

- **`N_BasisEval` with a flag other than `NL_LEFT`/`NL_RIGHT`.** It ignores the
  `NL_CAL_ERR` from `N_BasisFindSpan` and reads an uninitialised span index (UB). Rust
  takes a `Side` enum, so this can't happen. The ffi wrappers reject other flags.
- **Out-of-bounds reads on short knot vectors.** Upstream span and basis routines read
  outside `U` when `m < 2p+1` (and `N_BasisEval` overflows its stack arrays when
  `p > NL_MAXDEG + 1`). Rust returns `SolidError::InvalidInput` for these. The ffi wrappers
  only pass inputs with `n_knots >= 2p+2`.
- **`N_BasisEval` on `NL_NUM_ERR`.** Upstream leaves `N` partly computed. Rust returns only
  the error. A later port that relies on the partial values would need a different API.
- **NL_LEFT/NL_RIGHT docs contradict the code.** The upstream comments say `NL_LEFT` means
  `U[spn] < u <= U[spn+1]`, but the code does the opposite: `NL_LEFT` gives
  `U[spn] <= u < U[spn+1]`. Rust `Side` documents what the code does.
- **`N_KnotVectorIsValid` accepts degenerate vectors.** It accepts a vector of `p+1` equal
  knots (for example `[2,2,2,2]` with p=3), and a single knot with p=0. Rust does the same,
  faithfully.
- **Zero-length end spans are skipped.** `N_BasisFindSpan` moves the end spans inward past
  zero-length spans (`U[i+1] < U[i] + 1e-12`, upstream note [B405]). `find_span_and_mult`
  therefore reports multiplicity `p+1` at the domain ends, whatever the real end
  multiplicity is.

## geom.vector2d (green; no disagreements)

4060 default cases (60 fixture + 4000 generated) and a 200,060-case run (`--seeds 200000`)
agree bit for bit with upstream (`max_rel_err = 0`). Notes for later agents:

- **`Vec2` moved.** `math::Vec2`/`Point2` are now defined in `geom::vector2d` and
  re-exported from `math` (old paths still work).
- **Co-ported back-edges.** `sm_sqrt`, `sm_acos`, `sm_asin` (unit `geom.vector3d`) and
  `SM_IS_ZERO_SQUARED` (`SmTol::InTol` + `SmTol::GetScaledZeroSq`, unit `geom.tol`) are
  private helpers in `geom/vector2d.rs`, marked `// CO-PORT(<unit>): <symbol>`, rather
  than `Unported` stubs (they are pure one-liners; stubbing them would make `Length`
  fallible). The owning units may replace them with shared versions.
- **Upstream `operator==`/`operator!=` are tolerant and asymmetric.** The tolerance is
  `SM_EFF_ZERO * (1 + lhs.GetMaxDimension())`, so `a == b` and `b == a` can differ, and
  with a NaN distance both `==` and `!=` are false. Rust keeps exact `PartialEq` and
  exposes the upstream tests as `approx_eq`/`approx_ne`.
- **`operator/` silently skips the division** when `|d| < |x| * 1e-12` or
  `|d| < |y| * 1e-12` (returns the vector unchanged); `0/0` still gives NaN.
- **`CCWAngleBetween` snapping is one-sided.** Only cross products in
  `[-2*scaled_zero, 0)` snap to 0 or PI (upstream comment asks why it is not symmetric);
  ported as is.
- **`operator[]` out of range** returns 0.0 (const) or dereferences NULL (non-const) upstream;
  Rust returns `SolidError::Upstream { code: 1001 }`. The ffi wrapper only passes 0 or 1.

## geom.extent3d (partial; no disagreements)

3086 default cases (86 fixture + 3000 generated) and a 100,086-case run (`--seeds 100000`):
every non-deferred case agrees bit for bit with upstream (`max_rel_err = 0`). 6 fixture
cases are deferred. Notes for later agents:

- **Deferred functions (no Rust code yet).** `GetUInterval`/`GetVInterval`/`GetWInterval` and
  `SetUInterval`/`SetVInterval` need `SmExtent1d` (unit `geom.extent1d`); `Circumscribe` and
  `SmExtent3d(const SmPseudoBox&)` need `SmPseudoBox` (unit `geom.pseudo_box`). Both are
  marked `UNPORTED(<unit>)` at the top of `geom/extent3d.rs` and in the runner's
  `circumscribe`/`intervals` ops. Those porters should add the methods to `Extent3d` (its
  fields are `pub(crate)`) and switch the runner ops from `Output::deferred` to real calls.
- **Co-ports.** `sm_sqrt`, `sm_asin`, `SmVector3d::GetMaxDimension` and
  `SmVector3d::operator[] const` (unit `geom.vector3d`) are private helpers marked
  `// CO-PORT(geom.vector3d)`. `SM_BIG_DOUBLE`, `SM_INFINITE_PARAMETER(_SQRT)`,
  `SM_BOUNDED_INFINITE_PARAM` and `SM_IS_INFINITE` (`math::is_infinite_param`) are now in
  `math`.
- **`ContainsLineSeg3d` ORs the end-point tests.** Its comment says the box contains the
  segment when it contains both ends, but the code uses `|=`, so one end inside is enough.
  Ported as is.
- **`IntersectLine` leaves `rdTEnter`/`rdTExit` untouched when nothing is found**, and
  `SmPseudoBox::SetBasis` returns early (leaving members uninitialised) for a near-singular
  basis. The shim pre-fills the outputs with NaN, and the ffi wrapper only passes clearly
  independent bases for `circumscribe`.
- **`ApproximateUnbounded` computes each axis's max from the already replaced min**
  (`SM_BT_UNBOUNDED_MAX` branch), and its optional expanded extent is left untouched when
  `SetMinMax` rejects it (inverted interval, e.g. a negative half size). Rust returns `None`
  for that second extent.
- **`GetMin(ULONG)`/`GetMax(ULONG)` return 0.0 for an index above 2** (via
  `SmVector3d::operator[] const`); `Extent3d::min_at`/`max_at` do the same.

## topology.database_io (green; no disagreements)

2059 default cases (59 fixtures over `Surface.sms`, `MakeFace/ApproximateSphere.smp`,
`Tess/Extrusion.pbp`, `Tess/BoxFillet.stl` plus edge cases, and 2000 generated scripts)
and a 200,059-case run (`--seeds 200000`) agree exactly with upstream, including the
stream state and read position after every step. Things later ASCII/binary reader ports
must know:

- **ASCII readers depend on libstdc++ details.** `*GetInStreamPtr() >> x` is reproduced
  by `InputStream::extract_*` (sentry, `num_get`, `strtod`): no digits gives 0 with
  `failbit`; overflow clamps (`long` to `LONG_MIN/MAX`, `unsigned long` to `ULONG_MAX`,
  `double` to `±DBL_MAX`) with `failbit`; `>> unsigned long` accepts `-n` and negates
  modulo 2^64; `>> short`/`>> int` read a `long` and clamp; a failed sentry stores
  nothing. Once `failbit` or `eofbit` is set, every later extraction and `GoToNextLine`
  does nothing; upstream readers rarely check, so later values keep their old contents.
- **`GoToNextLine` = `ignore(256, '\n')` stops after 256 bytes without consuming the
  newline.** A line of 256+ bytes from the current position is therefore not finished:
  the next read starts on the same line.
- **`ReadType` word buffers are capped at 255 bytes, but only in optimised builds.** GCC
  11+ libstdc++ bounds `operator>>(istream&, char*)` with `__builtin_object_size` under
  `__OPTIMIZE__`. Upstream is built `-O2`, so the 256-byte `sBuff` takes 255 bytes and
  leaves the rest in the stream; at `-O0` the same code overflows the buffer. The port
  caps at 255 (`extract_word_limited`). Shim code built at `-O0` doesn't have the cap.
- **Binary short reads are undefined upstream.** `ReadLong`/`ReadLongs` (`version >= 40`)
  read into uninitialised locals and return `SM_SUCCESS` with garbage when the data ends.
  The port returns `SolidError::InvalidInput`; difftest compares only the status (`-2`)
  and the stream state in those cases.
- **LP64.** `long` is 8 bytes on the reference platform, so the `version < 40` and
  `>= 40` integer paths are identical and their range checks are dead. A file written on
  LLP64 Windows with `version < 40` (4-byte longs) would not read correctly upstream
  either.
- **`ReadBoolean` never byte-swaps, `ReadBooleans` does** (upstream asymmetry, kept).
  `ByteSwap32(unsigned short)` always returns 0, but nothing reaches it when
  `sizeof(short) == 2`.

## nlib.math (green; no disagreements)

3076 default cases (76 fixture + 3000 generated) and a 100,076-case run (`--seeds
100000`) agree bit for bit with upstream (`max_rel_err = 0`), across all 79 functions
(point/control-point arithmetic, `N_SolveQuadraticEq`, `N_FloatOpIsBad`, the real-matrix
routines, power-basis surfaces, reallocation). Upstream behaviour worth knowing:

- **No back-edge left.** `N_SrfPowerBasisEvalPt` calls `N_SrfGetCPts` and
  `N_SrfGetKnotVectors` (unit `nlib.srf_basic`) only to read the control net; the knot
  vectors are unused. `N_SrfPowerBasisEvalDerivs` reads `spl->p`, `spl->q` and the net
  directly. The Rust functions take the net (and degrees) instead of an `NL_SURFACE`, so
  nothing is stubbed; the difftest drives the C++ through a real `NL_SURFACE`. When
  `nlib.srf_basic` lands, a `Surface` method can forward to these.
- **Banded matrices with an even bandwidth** index one column past the row
  (`j - i + bw/2` reaches `bw`), and an in-place banded transpose writes past its new
  `(n+1)`-column rows when `bw > n + 1`. Triangular operands that are not square read
  outside their rows in `N_RealMatrixMultiply`/`Transpose`/`MultiplyPtArray`. Rust
  returns `InvalidInput` for all of these; the ffi wrappers only pass odd bandwidths and
  square triangular/banded matrices.
- **`N_SrfPowerBasisEvalDerivs` overflows `up[NL_MAXDEG+1]`.** It stores the v-power rows
  after the `p+1` u-powers in the same 18-entry stack array, so e.g. bicubic with `vdr = 3`
  (`p + 1 + min(vdr,q)*(q+1) + q > 17`) writes past it. Rust returns `InvalidInput`.
- **`N_SrfPowerBasisEvalDerivs` rational upper-half quirk.** The final loop tests
  `k1 == vdr` (a leftover loop variable, `(k2+1)*(q+1)`) instead of `udr == vdr`. The port
  keeps it. With `udr > vdr` it can read past `SD`/`CwDers` (Rust: `InvalidInput`; the
  ffi wrapper rejects rational `mfl` cases with `udr > vdr`).
- **Uninitialised storage.** NLib allocates with `malloc`; triangular results only fill
  their triangle and `N_Realloc*` leaves new entries unset. Rust zero-fills. difftest
  compares the *defined region* of each result (the entries upstream writes).
- **`N_RealMatrixIsNULL` treats `bw <= 0` as null**, so a full result of a 1x1 product
  (`bw = na = 0`) is reallocated when reused as an output. Ported as is.
- **`N_RealMatrixTranspose` in place, full type**, leaks the old storage (the free is after
  `NL_OUT`). No effect on results.
- **Globals.** `NLIBCHKBYPASS` (only `M_NoErrChk` sets it; nothing calls that) is treated
  as 0. `NL_MTOL` is a parameter of `CPoint::approx_eq` because `N_SrfIsClosedSmooth`
  temporarily overrides it around `N_SrfIsClosed`.

## geom.tol (partial; no disagreements)

3430 default cases (430 fixture + 3000 generated) and a 60,430-case run (`--seeds 60000`):
every non-deferred case agrees bit for bit with upstream (`max_rel_err = 0`). The 221
deferred cases are `AreParallel` / `ArePerpendicular`, which call
`SmVector3d::IsParallelTo` / `IsPerpendicularTo` (unit `geom.vector3d`). Notes for later
agents:

- **Build mode is `SM_USE_OLDTOL`** (`SmConfig.h`); `SM_NEWTOL_LINE` / `SM_OLDTOL_LINE` are
  empty markers, so lines tagged with either one are compiled. Only `#ifdef SM_USE_NEWTOL`
  blocks are dead.
- **Tolerance classes are `f64`.** `SmZoneTol3d(double)` etc. substitute the system value
  for `SM_USE_DEFAULT` (9876543.21) and take `fabs`; the Rust port applies
  `tol::zone_tol3d` / `xsect_tol3d` / `approx_tol3d` / `tol3d` exactly where upstream
  converts a `double` (returns, casts, initialisations). `operator=(double)` only takes
  `fabs` (no `SM_USE_DEFAULT` check); `operator*=` takes neither.
- **Small topology scales tolerances *up*.** `GetZoneTol3d(SM_ST_SMALL_SIZE)` returns
  `s_LargeSmallSizeRatio * s_ZoneTol3d` (default 1000x), despite the "tighter tolerances"
  comments. The `const SmContext*` overloads ignore the small flag entirely.
- **System statics live in the `Kernel`.** `s_ZoneTol3d`, `s_TolGapGain`,
  `s_ModelSizeEstimate`, `s_LargeSmallSizeRatio` are `kernel.tolerances.{zone_tol_3d,
  tol_gap_gain, model_size_estimate, large_small_size_ratio}`. The kernel also stands in
  for `SmContext`; since `geom.context` the `context_*` / `calc_*_zone_tol_3d`
  functions take `Option<&Context>` (`None` = null pointer, `Some(&k.context)` usual). The difftest shim sets the
  statics with `SmTol::SetModelSizeEstimate` / `SetLargeSmallSizeRatio` per case and
  restores the defaults afterwards (the C++ statics are thread-local globals).
- **Overloads needing later types are not in Rust yet** (markers at the top of
  `geom/tol.rs`): every `SmObject` / `SmCurve` / `SmSurface` / topology /
  classification / `SmExtent1d` / `SmExtent2d` / `SmPseudoBox` / `SmVolume` overload,
  `MapTo1d`, `MapTo2d`, `IsDegenerate(SmObject*)`, `IsClosed`, `UpdateObjectTolerance`.
  Their porters should add them to `geom::tol` and extend the runner.
- **`SmRelation` iteration order is unspecified upstream.** `SmMapPtrToPtrs` is a
  `std::unordered_map<T*, std::unordered_set<U*>>`, so `GetSeconds`, `GetAllFirsts`,
  `GetAllSeconds` come out in pointer-hash order. `geom::tol::Relation` uses
  `BTreeMap`/`BTreeSet` and returns keys sorted; the shim sorts the upstream results
  before comparing. `GetSecond` returns `NULL` (the `SE(SM_ERR)` is a no-op in release)
  for 0 or >1 partners; Rust returns `None`. `RelatePair` returns `SM_SUCCESS` (1000) or
  `SM_ERR` (1001) on a cardinality violation; `RelateToMany` stops at the first error and
  keeps the pairs related before it.
- **Co-ports.** `sm_sqrt` and `SmVector3d::GetMaxDimension` (unit `geom.vector3d`) are
  private helpers marked `// CO-PORT(geom.vector3d)`. The `SM_IS_ZERO_SQUARED` co-port in
  `geom/vector2d.rs` now calls `geom::tol` (vector2d difftest re-run: green, 4060 cases).

## geom.context

- **Two-context `SmNewMarkAndLock` constructor bug (ported as is).** In
  `SmNewMarkAndLock(SmContext* p1, SmContext* p2)` with `p1 != p2`, upstream picks
  `m_eMarkType2 = GetUnlockedMark(*p2)` but then calls `m_pContext1->NewMark(m_eMarkType1)`
  (advancing context 1's mark a second time, not context 2's) and
  `LockMark(m_pContext2, m_eMarkType1)` (locking mark type 1 in context 2, not type 2).
  The destructor then unlocks `m_eMarkType2` in context 2, so when the types differ a
  stray lock of type 1 stays on context 2. `geom::context::MarkLock::new_pair` reproduces
  this exactly (difftest case `boolean_pair_two_contexts`). `SetContext(p1, p2)` is
  correct (locks type 2 in context 2) and does not advance marks.
- **Release build ignores locks.** `NewMark`'s lock checks and `LockMark` /
  `UnlockMark`'s state checks are `SM_ASSERT*` (messages only), so locked marks can still
  be advanced and double locks are silent; the port drops the assertions.
- **`SmNewMarkAndLock` binds contexts by index.** Rust `MarkLock` stores indices into a
  caller-owned `&mut [Context]` instead of pointers and its destructor is the explicit
  `release`; `with_mark_lock(&mut Kernel, ..)` is the scoped form for the kernel's own
  context (`Kernel::context`). Topology porters: use `k.context` for
  `GetContext()->GetCurrentMark(..)` and `with_mark_lock` for `SmNewMarkAndLock`.
- **Build flags.** The reference build has neither `SM_USE_GLOBAL_CACHE` nor
  `SM_USE_NEWTOL`, so the constructor is the two-double one and
  `SetThisModelSizeEstimate` is the inline header version
  (`m_sThisZoneTol3d = SizeEstimateToZoneTol3d(estimate)`, which maps `SM_USE_DEFAULT` to
  `SM_MODEL_SIZE_ESTIMATE`, not to the system statics).
- **`SmApiGetOrCreateContext` global.** The SM_API keeps one process-global context;
  the port takes the `Option<Context>` slot from the caller.

## geom.tree (partial; no disagreements)

618 default cases (18 fixture + 600 generated): every non-deferred case agrees exactly
(`max_rel_err = 0`); 7 are deferred (`GetFirstBezierPatch` paths). Notes for later agents:

- **Arenas mirror `SmMemBlockMgr`.** Nodes and object-list items live in `Tree::nodes` /
  `Tree::lists` in allocation order, which is the order `GetAllTreeNodes` and
  `SmTree::GetObjectList` report (and therefore the order a forced rebuild in
  `AddToSpatialTree` re-inserts objects). Fresh nodes are zero-filled like `smos_Calloc`
  memory (all-zero box, `SM_NG_NONE`); a failed child `SetMinMax` leaves that zero box.
- **Objects are a generic handle `O`.** `SmObject*` becomes `Option<O>`; the shim uses fake,
  never dereferenced pointers into a char array.
- **Subdivision data.** `SurfaceAux` holds only the `SmBezierAux2d` fields this unit reads
  (`m_eSplitDir`, `m_bIsMarked`, `m_eNodeClass`, `m_pFace`, `mBA_pSurface`); the surface
  cache units own the rest and should extend it. Upstream casts parent/sibling `m_pData`
  without checking the aux type; Rust returns `InvalidInput` where that cast would be invalid.
- **Back-edges.** `GetOwnerObject` (curve/surface cache owners), `GetFirstBezierPatch`
  (`SmBezierPatch::GetBezierPtr`, `SmBSplineSurface::GetGwNurbPointer`) are `UNPORTED` stubs;
  the shim skips exactly those paths (owner is always NULL there).
- **Upstream quirks kept.** `SmObjectList::FindObject` returns the last item (not NULL) as
  "previous" when the object is not found. A point tree (`SmTree(point, size, ...)`) never
  initialises its object-list memory manager, so `AddToSpatialTree` on it divides by zero
  upstream; the ffi wrapper refuses that op. `1 << lSplitCount` in the neighbour searches is
  a C `int` shift (sign-extended into `ULONG`); Rust mirrors it.

## geom.periodic_extent1d (green; no disagreements)

Green, 4054 difftest cases, all bit-identical. Upstream quirks ported as is (not bugs in
the port):
- **`AddInterval` on an empty extent gives the full period.** The empty branch computes
  `m_bFullPeriod = (m_dPeriod - |m_dMax - m_dMin|) < ScaledZero` on the empty sentinels
  `[1e20, -1e20]`, which is true for any period below `2e20`, so the extent becomes
  `[0, period]` full instead of `[from, to]`. Case `add_empty_quirk`.
- **`SetMinMax` updates the extent even when it returns `SM_ERR_INVALID_INPUT`** (min or
  max outside `[0, period]`). `PeriodicExtent1d::set_min_max` does the same and documents it.
  When the bounds differ but span the period, `m_bFullPeriod` is set without snapping
  `min`/`max` to `[0, period]`.
- **`IsInit` compares the `int` `m_bFullPeriod` with `-SM_BIG_DOUBLE`**, so it is always
  true; it only feeds a compiled-out assert (and is unused), not ported.
- Only the 17 functions owned by the unit are ported; `Invert`, `AddValue`, `Union`,
  `Intersect`, `Subtract`, `Evaluate`, `GetLength`, `IsPoint`, the 3-argument constructor
  etc. are unreachable from the upstream entry points (`harness/callgraph/unused_functions.txt`).

## geom.matrix (green; no disagreements)

4044 default cases (44 fixture + 4000 generated) and a 100,044-case run (`--seeds 100000`)
agree bit for bit with upstream (`max_rel_err = 0`). Upstream behaviour worth knowing:

- **`SM_SUCCESS` is 1000, not 0.** Shims that pass an `SmStatus` back must map it to 0
  (`geom_matrix.cpp` does) or the runner sees `status_1000` as a failure.
- **`LUDecompose` never fails on a singular pivot.** A pivot below `SM_EFF_ZERO` is
  replaced by `+SM_EFF_ZERO` (or `-SM_EFF_ZERO` when it is `<= 0`, including exactly 0)
  and the status is `SM_ERR_WARNING`; `SolveLinearSystem` passes the warning through
  with a (meaningless) solution, which `my_test_matrix` relies on. Only a row whose
  largest magnitude is below `SM_EFF_ZERO` gives `SM_ERR`. The port reports the warning
  as `near_singular` on success. With NaN entries the pivot search keeps `lMax = 0`
  (NaN never compares `>=`), so row 0 can be swapped back in; ported as is.
- **`Compute3x3EigenVectors` is `const` but mutates `this`** (it casts away const and runs
  `EigenSystemJacobiSolver`, which restores the upper triangle from the lower one on
  success and leaves it rotated on non-convergence). It ignores the solver status: for a
  non-symmetric matrix it returns `SM_SUCCESS` with eigenvectors read from an
  uninitialized `SmMatrix`. The shim and the port report `SM_ERR` there instead; when
  the solver runs out of sweeps both return the unconverged, unsorted state with success.
- **Fixed-size buffers.** `LUDecompose` scales with a stack `double[MAX_MATRIX_SIZE]`
  (1000) and only `SolveLinearSystem` checks the size; the shim limits matrices to 64x64.
- `sm_sqrt` (geom.vector3d) and `SmVector3d::Set` are co-ported in `geom/matrix.rs`
  (`// CO-PORT(geom.vector3d)`), so no case is deferred.

## nlib.geom (green; no disagreements)

3173 default cases (173 fixture + 3000 generated) agree bit for bit with upstream
(`max_rel_err = 0`). Upstream behaviour worth knowing:

- **Unwritten outputs.** `N_IsectLineLine` leaves `P`, `t1`, `t2` unset when the lines
  are parallel or miss a bounded segment; `N_IsectLinePlane` leaves `P` (and, when
  parallel, `t`) unset. The port returns `None` / `LinePlaneIsect::{Parallel,
  OutOfBounds { t }}` instead; the shim pre-sets NaN.
- **Uninitialised reads.** `N_DistPtLineSeg` writes no distance when its position tests
  are all false (NaN coordinates); the port returns NaN. `N_CPtsAreColinear` (3D)
  ignores `N_DistPtLineSeg`'s error and compares that unwritten distance (e.g. a zero
  weight makes the Euclidean image NaN); the port answers `false`. Generated
  collinearity cases therefore use positive weights. `N_ProjectPtPlane` and
  `N_DistPtInfLine` read an unset point when their unbounded line misses the plane,
  which cannot happen for finite input; the port returns `InvalidInput` there.
- **`NL_MTOL` is a global** that `N_SrfIsClosedSmooth` overrides temporarily; the
  `*_tol` variants of `dist_pt_line_seg`, `dist_perp_pt_line_seg` and
  `cpts_are_colinear` take it explicitly.
- `N_IsectLineLine` tests parallelism with `NL_ZCTL` (1e-12) on `1 - cos^2`, while
  `N_IsectLinePlane` uses `NL_LTOL` (1e-9) on the cosine; both ported as is.
## geom.attribute (partial; no disagreements)

2029 default cases (6 upstream-derived fixtures, 23 edge-case fixtures, 2000 generated
scripts) and a 200,029-case run (`--seeds 200000`): every non-deferred case agrees
exactly with upstream (ints/topology identical, doubles bit for bit). Cases whose script
runs `Merge` on an `SmMergeRegionAttribute` are deferred on `topology.shell`
(`SmRegion::GetBrep` / `IsInfiniteRegion` / `IsVoid`; 38 of the default cases). Notes for
later agents:

- **Data model.** `geom::attribute::AttributeStore` owns every attribute (append-only
  slots, `AttributeHandle`) *and* every object's attribute list, keyed by an opaque
  `ObjectKey(u64)`. Whoever ports `SmAObject` (unit `geom.a_object`) and the topology
  owners should map their ids to `ObjectKey`s (or replace the key type) and hold one
  store per B-rep or kernel. The `SmAObject` attribute-list methods
  (`AddAttribute`, `RemoveAttribute`, `ClearAttributes`, `GetAttributes`,
  `FindAttribute`) are co-ported as store methods (`// CO-PORT(geom.a_object)`);
  `SmObject::IsKindOf` (`geom.object`) closes `Attribute::is_kind_of`.
- **Wiring `topology.shell`.** `AttributeStore::merge` on a merge-region attribute calls
  the private stubs `region_brep` / `region_is_infinite` / `region_is_void`
  (`// UNPORTED(topology.shell)`). They need region data the store does not have, so the
  porter will have to pass the B-rep (or a region lookup) into `merge`. In the difftest
  the regions are bare `SmRegion`s outside any B-rep: `GetBrep()` is NULL for both,
  `IsInfiniteRegion()` FALSE, `IsVoid()` FALSE, so the added region type is always 0.
- **Upstream behaviour kept (all compared):**
  - `SmLongAttribute`, `SmVector3dAttribute` and `SmColorAttribute::MakeCopy` drop the
    behaviour (copies are `SM_AB_COPY` even from `SM_AB_STANDALONE_COPY`); the other
    classes keep it.
  - `AddAttribute` only skips the attribute when `FindAttribute(id)` returns *that*
    attribute, so a second attribute with the same id is added, and adding it again adds
    a duplicate entry and a duplicate user (`AddUser` only signals `SM_ERR`).
  - `fromClearAttributes` with exactly one user empties the user list and deletes a
    dependent attribute whoever that user is.
  - `SmMergeRegionAttribute::GetType()` is `SmPointerAttribute_TYPE`, but `IsKindOf`
    answers for `SmMergeRegionAttribute_TYPE` and not the pointer type.
    `SmColorAttribute::GetType()` is `SmVector3dAttribute_TYPE`.
  - Only `SmColorAttribute` is propagated through Booleans; an `SM_AI_COLOR`
    `SmVector3dAttribute` (what persistence and the upstream tests create) is not.
    `SmColorAttribute::PropagateRegionsThroughBooleans` copies *itself* (not the colour
    it found) onto the result when only one input has a colour; any other class's
    `PropagateRegionsThroughBooleans` runs the base `Merge`, even for merge-region
    attributes.
  - `GOTO_NEXT_ATTLINE` treats a `0xFF` byte as `EOF` (`(signed char)get()`), and fails
    with `SM_ERR` after more than 1000 bytes before the newline.
- **Upstream undefined behaviour (Rust returns `SolidError::InvalidInput`; the difftest
  skips these with status -2 on both sides):** `~SmAttribute` on a dependent attribute
  with users deletes it twice (its `RemoveAttribute` -> `RemoveUser` deletes when the last
  user goes); `~SmAttribute` with a user that does not list it leaves other lists
  dangling; `RemoveUser`/`fromClearAttributes` called directly on a listed attribute can
  leave a dangling list entry; `SmMergeRegionAttribute::Merge` dereferences NULL when
  both inputs have a merge-region attribute and the result does not, and casts any
  `SM_AI_OTHER_REGION_ID` attribute to the merge-region class; the colour average casts
  whatever `SM_AI_COLOR` attribute it finds and dereferences a NULL result colour;
  `SetRegions` writes past the "other" lists when they are shorter than the "this" list;
  `ReadIndexedAttributesFromDB` reads an uninitialised count when the ASCII stream is
  already failed or at its end, and a short binary read gives garbage; counts/indices
  near `ULONG_MAX` wrap `SetSize`. Rust caps counts and indices at 2^20.
- **Shim liveness is predicted.** Upstream gives no way to see that `RemoveUser` /
  `fromClearAttributes` / `ClearAttributes` deleted an attribute, so the shim predicts
  which script handles die from the state before the op (rule documented in
  `harness/cpp/shim/geom_attribute.cpp`); the Rust side reports its store's real
  liveness. Everything else in the dump (lists, users, values) is read back from the
  C++ objects.
