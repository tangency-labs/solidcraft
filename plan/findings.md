# Findings

Rust-vs-C++ disagreements, upstream bugs, nondeterminism and other facts later agents
need. Add entries under a heading per unit (or "General"); keep the newest at the end
of each section.

Upstream C++ defects only (UB, crashes, hangs, nondeterminism, wrong results, wrong docs)
are also logged, one entry each, in `plan/upstream-issues.md` for the upstream maintainers.

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

## geom.extent2d (green; no disagreements)

3069 default cases (69 fixture from `my_test_extent2d` + edge cases, 3000 generated)
agree bit for bit with upstream (`max_rel_err = 0`); a 30000-seed run is clean.
Upstream behaviour worth knowing:

- **`ContainsPeriodicPoint2d` can hang.** It calls `SmExtent1d::ContainsPeriodicValue`,
  whose `PeriodicWrap` steps one period per iteration; a period that is tiny relative
  to the point's distance from the interval (e.g. period `1e-9`, value `1e15`) runs
  for ~1e24 iterations in both C++ and the port. The generator only uses periods with
  `|p - min| / period < 1e4`.
- `Inversion` may write a partial normalised point (`-1` components) before returning
  `SM_ERR`; the port returns only the error. `IntersectWithInfiniteLine` always
  returns `SM_SUCCESS` and leaves the interval untouched when nothing is found (port:
  `None`).
- `GetU/VLeftEval` return TRUE unless `value > mid` (TRUE for NaN), kept as is.
- `SmTol::GetScaledZero(const SmExtent2d&)` is implemented in `geom/extent2d.rs`
  (wired from the `UNPORTED(geom.extent2d)` marker in `geom/tol.rs`) and covered by
  the `geom.tol` fixtures `fixtures/geom.tol/extent2d.json` (object kind 5).

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

## geom.polynomial (green; no disagreements)

6038 default cases (38 fixture from `my_test_polynomial` + edge cases, 6000 generated)
agree bit for bit with upstream (`max_rel_err = 0`). Upstream behaviour worth knowing:

- **Uninitialised read in `SolveQuarticEqn`.** In the `r != 0` branch it takes
  `z = adCSol[0]` from the resolvent cubic (leading coefficient `1.0`) without checking
  the root count. With `dZeroTolerance > 1` that cubic degrades to a quadratic that may
  return no roots, leaving `adCSol[0]` uninitialised (UB). The port uses `z = 0.0`; the
  FFI wrapper rejects quartics with `tol > 1`, so generated quartic tolerances stay <= 1.
- **Unused output slots.** `SolveQuadraticEqn` zeroes both slots; `SolveCubicEqn` /
  `SolveQuarticEqn` leave slots past `rlNumSolutions` untouched. Callers must read only
  the first `rlNumSolutions`; the port's `Roots` exposes exactly those.
- `SolveCubicEqn`'s three-root branch uses the plain `sqrt` for `sqrt(-cb_p)` but
  `smos_Sqrt` (clamping) for `sqrt(-p)`, and `smos_ArcCosine` clamps to `[0, PI]`; ported
  as is. Every solver always returns `SM_SUCCESS`, so the port is infallible.

## nlib.srf_basic (green; no disagreements)

3220 default cases (220 fixture + 3000 generated) and a 100,220-case run (`--seeds
100000`) agree bit for bit with upstream (`max_rel_err = 0`). Fixtures include the
upstream surfaces `Surface.sms` (thinly closed in u), `offset_nurb.sms` and
`DropCurveSurface.sms`, read by the runner. Notes for later agents:

- **`nurbs::srf_basic::Surface` is the `NL_SURFACE`.** It owns its net and knots but, as
  upstream, keeps the indices in use (`n`, `m`, `r`, `s`) apart from the storage, which may
  be larger (`Surface::with_indices`, `set_size_indices`). The NULL surface of
  `N_SrfInitArrays` is `None` in an `Option<Surface>`; `size_arrays` / `copy_into` take
  that `Option` (`N_SrfSizeArrays` / `N_SrfCopy`). Storage upstream `malloc`s is
  zero-filled. `KnotVector`s are borrowed from the surface with `knot_vectors()`.
- **`N_SrfIsSized` compares the indices in use, not the capacity.** So `N_SrfSizeArrays`
  reallocates whenever a request exceeds the current `n`/`m`/`r`/`s`, even if the storage
  is big enough; spare capacity is only reachable through `N_SrfSetSizeIndices`. When it
  reallocates, `N_SrfSizeArrays` returns 0 but leaves `NL_STO_ERR` in `NL_ERROR` (set by
  `N_SrfIsSized`); the Rust port records nothing.
- **Weight checks.** `N_SrfWeightsAreValid` lets NaN weights through (`<`/`>` tests); a
  non-rational point (`w = NL_NOW`) in a rational surface fails it as a weight error.
  Rationality is decided by `Pw[0][0]` alone (`N_IsSrfRat`).
- **Undefined upstream paths, made errors in Rust.** `N_SrfReparamToInterval` writes
  `U[r-p..]` outside the array when `r < p` (`InvalidInput`, nothing changed);
  `N_SrfSetSizeIndices` / `N_SrfSetCPtsAndKnots` accept indices past the storage
  (`InvalidInput`). The ffi wrapper never passes those inputs.
- **Unwritten outputs.** `N_SrfIsFlatCheap` leaves `flt`/`dir` unset when
  `N_PlanePtNormalToImplicit` fails (the normal's `1/|N|` overflows); the port returns only
  the error. Its closed and degenerate exits use the global `NL_MTOL`
  (`Surface::is_closed_tol` takes it explicitly for `N_SrfIsClosedSmooth`).
- **`N_SrfIsClosed` with `NL_UVDIR`** (or any flag but `NL_UDIR`/`NL_VDIR`) is `false`.
- No `UNPORTED(nlib.srf_basic)` stubs existed to wire. `nlib.math`'s power-basis
  routines still take the net directly; a `Surface` method may forward to them.

## nlib.bezier (green; no disagreements)

2223 difftest cases (223 fixture: SM_API_test rectangle/polygon segments, `FitCrvPoints.pts`,
`Points.pts`, `Surface.sms`, edge cases; 2000 generated), all bit-identical
(`max_rel_err = 0`), 0 deferred. Notes for later agents:

- **Matrices built on a freed stack.** `N_BezElevateDegree`, `N_BezSrfElevateDegree` and
  the `N_BezFuncMultiplyBez*` routines compute a NULL elevation / product matrix into the
  caller's `NL_RMATRIX` on a local `NL_STACKS` that `N_EndNurbs` frees before returning, so
  the caller is left with dangling storage. The Rust routines take `&RealMatrix` and build
  a local matrix when it `is_null()`; callers that want reuse should call
  `elevation_matrix` / `product_matrix` first. With degree `p = 0` the matrix has
  bandwidth 0 and is "NULL" again (recomputed every call), harmless.
- **`NL_PascalTri` is `[18][18]`.** Every routine using it (elevation/product matrices,
  `N_BezCrvExtend`, hence `N_BezSrfExtend4D`) reads past it for degree sums above 17;
  Rust returns `InvalidInput`. `bezier::PASCAL_TRI` / `binomial` are the table.
- **`ST_BezCalcArcLength` gives up past `NL_RECURSELIM` (20)** by returning 1 without
  setting `NL_ERROR.eno` (shim status 1000; Rust `NotConverged`). With `tol = 0` any curved
  segment hits it; collinear control points finish (`del == 0`). NaN coordinates also end
  there.
- **`N_BezReduceDegree` needs `p >= 2`**: for `p = 1` it reads the never-written
  `alf[0]` and `Qw[-1]` (Rust `InvalidInput`). `N_BezDegreeReduceCoefs` writes `alf/oma`
  only at `1..=r` and `bet/omb` at `r..=p-2` (`r = (p-1)/2`).
- **Flags.** `N_BezCrvExtend`'s `whr`/`rev` became `ExtendAt` / `bool` (its `NL_CAL_ERR` for
  other values is unreachable); `N_BezSrfExtend4D` with a direction other than U/V returns
  a surface whose net was never written (Rust `InvalidInput`); `N_BezSrfElevateDegree`
  with `NL_UVDIR` fails with `NL_CAL_ERR` after building the matrix (kept).
- **NaN parameters** pass the `[0, 1]` checks of `N_BezEvalOneBasis` / `N_BezSplit`
  (`<`/`>` tests) and propagate, as in Rust.
- No `UNPORTED(nlib.bezier)` stubs existed to wire.

## nlib.basis_basic (green; no disagreements)

2596 default cases (1096 fixture + 1500 generated) and a 201,096-case run
(`--seeds 200000`) agree bit for bit with upstream (`max_rel_err = 0`). Fixtures use the
knot vectors of every `.smc`/`.sms` file in `pt_TestFiles`, the `smTopoTest.cpp` and
SM_API_test rectangle/polygon layouts, and the examples in the upstream doc comments.
Upstream behaviour worth knowing:

- **Endless loops upstream.** `N_BasisFindGlobalMax` loops forever when its bracketing step
  `(U[i+p+1]-U[i])/(10p)` is below half an ulp of the knots (huge knot values, tiny span).
  `N_GetCompatibleKnotVectorToTol` loops forever when an iteration matches no knot
  (`tol <= 0`, NaN knots). `N_MakeKnotsCompatible` loops forever on NaN knots. Rust returns
  `SolidError::InvalidInput` in exactly these states (the loop state is unchanged, so
  upstream would repeat it forever). The ffi wrappers reject these inputs.
- **`N_GetCompatibleKnotVectorToTol` reads `U[-1]`** when a vector's first knot is above
  the current smallest knot (the "move to average" test runs for vectors with no match).
  The read never changes the result. Rust skips the test for those vectors. The shim
  gives each vector a leading pad slot. It also writes into a buffer of `sum(m)+1` slots
  per vector, which a vector with fewer than `k-1` knots can overflow. Rust returns
  `InvalidInput` there, and the ffi wrapper requires at least `n_vec - 1` knots per vector.
- **`N_GetCompatibleKnotVectorToTol` needs a common parameter range.** Vectors with
  different end knots run out at different times. Upstream then fails with `NL_INP_ERR`
  (14), and Rust does the same. Multiplicities *are* carried over between vectors (each
  distinct knot gets the highest multiplicity it has anywhere), unlike the
  `N_GetCompatibleKnotVector` doc comment says.
- **`N_MakeKnotsCompatible` reads past `U[m]`** on degree-0 vectors whose ends differ by
  less than `NL_PTOL` (so no rescale happens). It can also read past `U[m]` with inner
  knots within `NL_PTOL` of an end, or with unclamped vectors. Its output buffers are
  caller-allocated and unbounded. Rust returns `InvalidInput` for out-of-range reads. The
  shim pads both vectors with `+inf` entries so upstream stays in bounds and terminates.
  The generator keeps to degree >= 1 for the "ends within `NL_PTOL`" mode. It also
  pushes the end knot `U[m]` of the shorter-ended vector into the other's insert list
  when the ends differ by less than `NL_PTOL`. Rust does the same, faithfully.
- **`N_BasisDerivs` int factor.** The factor `r *= (p-k)` is an `int`, and it is computed
  one step past the last row it scales. It overflows (UB) for `p >= 13` once
  `der >= 10`, for example. Rust returns `InvalidInput`, and the ffi wrapper rejects these inputs.
- **`N_BasisFindAllSpanMaxima` with degree 1 or a `p`-fold inner knot** returns fixed
  values (`max = 1` at `U[i+1]`) without searching. Degree 0 goes through the Newton
  path with `du = span/0 = inf`. Rust does the same, faithfully.
## geom.hash_table (green; no disagreements)

619 default cases (19 fixture + 600 generated) and a 10,019-case run (`--seeds 10000`)
agree bit for bit with upstream (`max_rel_err = 0`). Fixtures replay the
`SmHealData::Cache_CoinVertices` pattern (12x12x12 voxels over the vertex box, `7 * n`
estimate, all-pairs search with histogram, then an incremental batch with tracked buckets
and the old vertex count as limit) on `Points.pts`, `FitCrvPoints.pts` and
`PointSet/Section*.pts`, plus edge cases. Upstream behaviour later agents must know:

- **Use-after-free in `SmObjsInVoxels::IncMaxItemCount`.** It re-points every bucket item's
  `m_sItem` at `&m_sPointItems.GetAt(id)` *before* `SetArray` swaps in the new array and
  frees the old one, so once a model that still has items in buckets grows (`Add` with
  `size + 7 > GetDataSize()`), every bucket item dangles (`FindCoincidentPairs` then reads
  freed memory). `SmHealData` only avoids it while incremental adds stay within the
  `max(1.5 * 7n, 7n + 64)` slots reserved by the first `SetSize`. The Rust port stores
  indices (the intended contents); the shim refuses such programs (`SC_ERR_ARG`) and the
  generator skips them, so that path is not difftested.
- **`FindCoincidentPairs` only compares against the bucket's first item.** `pBegPointItem`
  / `rBegPoint` are bound once per bucket and never advanced, so two coincident items that
  are both behind the bucket head are never paired (only head-vs-other pairs are found).
  Also `rEndPoint` is a `SmPoint3d&`: `rEndPoint = pEndPointItem->m_sTgtPoint3d` copies each
  end item's point into the item after the current begin item, so the stored points of the
  model change during the search. Both kept faithfully (`geom::hash_table` module docs).
- **Points below the voxel box snap to the top voxel.** `(ULONG)((x - min) / len)` is UB for
  negative values; x86-64 g++ truncates signed (`-3.2` -> `2^64 - 3`), which the clamp then
  maps to `cnt - 1` (the comment says the low end). NaN maps high, `+inf` / `>= 2^64` map to
  `0`. The port emulates this conversion (`ulong_from_f64`).
- **Growth after every item was removed zeroes the bucket items.** `SmHashTable::
  IncMaxItemCount` copies the records only while `GetUsedBucketItemCount() > 0`; otherwise
  they come back calloc-zeroed (bucket `0`, NULL item, id `0`), and a later `Remove` of such
  an item "removes" it from bucket 0 (no-op walk, counters still incremented). Reproduced.
- `SetSize` does not reset `m_lRemovedPointItemCount`; `SmHealData` passes the old *vertex*
  count as `pOptLimitIndx`, which `FindCoincidentPairs` compares with point-item indices
  (one vertex can own up to 7 point items). An empty `pOptBucketIndices` reads `GetAt(0)` of
  an empty array (UB; Rust checks no bucket, the shim refuses it).
- No `UNPORTED(geom.hash_table)` stubs existed to wire.

## geom.object (partial; no disagreements)

624 default cases (24 fixture, 600 generated) — 593 pass bit for bit, 31 deferred (every
non-empty `SmShape`: topology.brep_p1, topology.face_p2, topology.edge, topology.vertex).
Upstream behaviour worth knowing:

- **`SmApiReadBrepFromFile` crashes on `pt_TestFiles/SmallBox.smb` read as ASCII**
  (null `SmShellData` in `SmTopologyData::ReadFromDB`, `SmBrepData.cpp:4307`; depends on
  heap layout, so it crashes in some runs only). The shape fixtures therefore build their
  B-reps with `SmApiCreateBox`; the shim still supports file sources.
- **Empty `SmShape`.** With no topology objects the constructor reports `SE_MSG` and runs
  `new (*cpContext) SmTree(...)` through a NULL context reference (works in practice);
  all three trees cover `(0,0,0)-(1,1,1)`. The port builds the same shape without the
  message.
- **"Been through new" handshake.** `SmObject()` leaves `m_cpContext` uninitialised when
  the thread-local `s_sBeenThroughNew` flag is set by an `operator new(SmContext&)` whose
  constructor never ran; the port has no per-object context, so nothing to reproduce.
- `SmTemporaryChangeValue::UseOrigValue` / `UseTempValue` on a default-constructed,
  never-`Set` guard dereference NULL; the port does nothing.

## geom.local_solve_nd (green; no disagreements)

3043 default cases (43 fixture, 3000 generated), bit-identical, 0 deferred; a 30,043-case
run was also clean. Both sides solve the same test equation set (shim `ScTestENFO`, runner
`TestEquations`) and the whole `Evaluate` trace (points, Jacobian requests, failures) is
compared. Upstream behaviour worth knowing:

- **A failed first `Evaluate` is reported as found**: `SolveIt` returns `SM_SUCCESS` with
  `rbFoundSolution = TRUE` and `SM_TR_UNABLE_TO_CONVERGE`, the solution being the guess.
  Kept as is.
- **Uninitialised state.** `bFoundAnswer` is read after `Evaluate` even when an evaluator
  never writes it (the header notes many do not), and `m_lCurrentIteration` is not set by
  the constructor (early returns leave it untouched). The port returns "found" from
  `evaluate` (`Ok(bool)`; a failed call leaves the flag unchanged) and starts the counter
  at 0; the shim sets `m_lCurrentIteration = 0` and its evaluator writes `rbFoundAnswer`
  only on success.
- **Line-search break on a failed unclamped evaluation** keeps the previous `dNextSumF`
  and makes the *unclamped* next guess the current point; when the scaled clamped step
  wins, `sJacobian2` is discarded (the next iteration uses the unscaled point's Jacobian).
  Ported faithfully.
- **`SolveLinearSystem` statuses other than `SM_ERR`** (e.g. `SM_ERR_INVALID_INPUT` if an
  evaluator resized the Jacobian) do not stop the iteration upstream and leave the step
  stale; the port returns them as errors (unreachable with an `N x N` Jacobian).
- **Periodic clamping cost.** `SmExtent1d::PeriodicWrap` steps one period at a time, so a
  diverging Newton step (e.g. 1e8 with a period of 0.01) loops ~1e10 times in both
  languages. The generator only uses periodic intervals of width >= 0.5 on well-posed
  problems.
- `m_dFoundAccuracy` is never written by `SolveIt` and `m_eLocalSolverType` is never read;
  neither is ported. Wired `SmTol::GetScaledZero(const SmEvalNFunctionsObject&)` in
  `geom::tol` (`scaled_zero_of_equations`).

## geom.local_solve1d (green; no disagreements)

3316 default cases (316 fixture + 3000 generated) agree bit for bit with upstream
(`max_rel_err = 0`), comparing the full evaluation trace, outputs and solver state.
Upstream behaviour worth knowing:

- **Interval-less constructor is broken.** `SmLocalSolve1d(rFunctionEvaluator)` leaves
  `m_vInterval` as `SmExtent1d()` (min `1e20`, max `-1e20`), yet `SolveIt` still limits
  each Newton step to `GetLength() * 0.08 + SM_EFF_ZERO`, which is negative: every step
  becomes `+-1.584e19` *against* the Newton direction. Ported as is (fixtures
  `no_interval_*`); callers should always pass an interval.
- **Uninitialised `bFoundAnswer`.** When the first evaluation in `SolveIt` fails and
  there is no interval (no retry), the serendipity check reads `bFoundAnswer` as the
  evaluator left it (uninitialised if it did not write it). The shim's test evaluators
  always write it on failure; the port starts it `false`.
- **Partial evaluator writes are observable.** `SolveOscillating` keeps scanning after a
  failed evaluation and then reads `sTempIter.m_dFOfT`, i.e. whatever the failed
  evaluation wrote (or the previous value). The Rust `EvalFunction::evaluate` therefore
  keeps upstream's out-parameter shape and the solver keeps partial writes.
- `SolveIt` ignores the `SolveByBisection` status after the Newton loop and leaves
  `m_dLowerBracketT`/`m_dUpperBracketT` (and `m_dFoundAccuracy` on the Brent paths)
  from the previous solve; the port keeps that state.
- Not reached by any case (no difftest coverage, ported by inspection): the
  bracket choices in `SolveOscillating` that pick the *current* iterate (the cycle phase
  always puts the guess on the previous iterate's side in the constructed cycles), a
  failing final evaluation in `SolveOscillating`, a failing `SolveByBrent` called
  directly from the Newton loop, and `SolveByBrent`'s `u == x` early return.

## geom.polar_box (green; no disagreements)

All 3030 default cases (30 fixture programs plus 3000 generated programs of 4 to ~30
commands on four box registers) agree bit for bit with upstream (`max_rel_err = 0`).
Upstream behaviour worth knowing:

- **`Union` reads an unset count.** When neither box is unbounded and the axis tests pass
  (`|b0 + b0'|^2 >= SM_EFF_ZERO_SQ`, `b0 . b0' >= SM_EFF_ZERO`) but one box is
  `SM_PS_UNINITIALIZED` (possible after `ReSet`, which keeps the stale basis),
  `GetBoundaryVectors` returns `SM_ERR_INVALID_INPUT` without setting its count and
  `Union` loops over that unset count (undefined behaviour). The Rust `union` /
  `union_assign` return `InvalidInput` there; the shim detects the case and skips the call.
- **`Union` aliasing.** `SmCompositeCurve.cpp:446` calls `p->Union(t, *p)`. The result
  is `ReSet` and seeded with the mid vector before `this`'s corners are read, so the
  original extent of `p` is dropped (only `t`'s corners and the mid vector remain).
  Ported as `PolarBox::union_assign`.
- **Region-to-arc relabel does not transpose.** In `AddVector3d`'s `SM_PS_REGION` case,
  when the x range collapses below `SM_EFF_ZERO_SQRT` the basis is rotated
  (`b1 <- b2`, `b2 <- -b1`) and the state becomes `SM_PS_ARC`, but the domain is re-set
  to its own bounds (`SetMinMax(UMin, VMin, UMax, VMax)`), so the arc's span stays in y
  while arc code reads x. Kept as is.
- **`Cross`/`Union` keep stale fields.** On their early `SM_PS_UNBOUNDED` exits (and when
  `Cross` adds nothing) the result box keeps its previous basis and domain. The port
  returns a fresh box; the difftest compares upstream through a fresh temporary plus
  `operator=`, which is what `PolarBox::assign` reproduces.

## nlib.funcs_basic (green; no disagreements)

Difftest: 3130 cases (130 fixture + 3000 generated), bit-identical (max_rel_err 0); a
50,130-case run is also clean. Fixtures evaluate the control-point coordinates of the
upstream `.smc` / `.sms` test files as scalar curve / surface functions, plus edge cases.

- **Unchecked indices.** `N_CFuncSetSizeIndices` / `N_SFuncSetSizeIndices` and
  `N_CrvFuncSetPtrs` trust the caller; indices past the storage make later reads go out of
  bounds. The evaluators (`N_CFuncEval`, `N_CFuncDerivs`, `N_SrfFuncEvalPt`) read the value
  *storage* at `spn-p..=spn` with `spn <= m-p-1` and never compare with `n` (so a value array
  shorter than `m - p` entries is read past its end, while a larger storage is read beyond
  the indices in use). The Rust port reads the storage as well (same results) and returns
  `InvalidInput` instead of reading past it; the FFI wrapper refuses such inputs.
- **`N_CFuncSizeArrays` / `N_SFuncSizeArrays` compare with the indices in use**, not the
  storage: an existing function whose indices were shrunk cannot be regrown within its own
  storage (`NL_STO_ERR`). Kept (note `nlib.srf_basic`'s `size_arrays` reallocates instead,
  following `N_SrfSizeArrays`).
- A function is "NULL" (`N_CrvAreFuncArraysNULL`) when any pointer is NULL **or** the degree
  is `-1`; Rust uses `Option<CurveFunction>` / `Option<SurfaceFunction>`, so partly NULL
  functions cannot be expressed. Storage upstream allocates with `malloc` is zero-filled.
- No `UNPORTED(nlib.funcs_basic)` stubs existed to wire.

## nlib.basis_adv (green; no disagreements)

1744 default cases (244 fixture + 1500 generated) and a 100,244-case run (`--seeds 100000`)
agree bit for bit with upstream (`max_rel_err = 0`). Fixtures use the 33 knot vectors of
the `nlib.basis_basic` fixtures (every `.smc`/`.sms` in `pt_TestFiles`, `smTopoTest.cpp`,
SM_API_test rectangle/polygon layouts, doc examples) plus edge cases. Upstream behaviour
worth knowing:

- **`N_BasisDerivsArray` span walk.** Only `u[0]` and `u[n]` go through `N_BasisFindSpan`;
  the spans of `u[1..n]` are found by walking forward from the span of `u[0]` (the
  `NL_LEFT` case falls through into the `NL_RIGHT` loop, which is then a no-op). The walk
  has no bound: with `NL_LEFT` an inner parameter equal to the domain end `U[m-p]` of a
  clamped vector walks past `U[m]` (out-of-bounds read), and any inner parameter past the
  end does the same with either flag. Unsorted parameters are not rejected (spans only
  move forward). Rust returns `InvalidInput` for out-of-range reads; the ffi wrapper
  requires inner parameters `< U[m-p]` (left) / `<= U[m-p]` (right). Unlike
  `N_BasisDerivs`, there is no `1e-24` zero-denominator guard: zero knot differences give
  inf/NaN, as upstream. The `int` factor `p!/(p-k)!` overflows as in `N_BasisDerivs`
  (Rust `InvalidInput`, ffi rejects).
- **`N_BasisInsertKnots` reads `X[mx+1]`** after inserting the last new knot (the second
  comparison in the merge step). The value never changes the output, so Rust skips the
  comparison; the shim pads `X` by one slot. An empty `X` reads `X[-1]` (Rust
  `InvalidInput`). NaN knots make the merge loop forever (Rust `InvalidInput`).
- **`N_BasisIncreaseKnotMult`** fails with `NL_KNT_ERR` (4) when any inner knot would
  exceed multiplicity `p`; new knots of earlier spans are dropped (index not set).
- **`N_GetCompatibleKnotArrayMult`** has the same structure as `N_MakeKnotsCompatible`
  (same degree for both vectors, multiplicities merged to the maximum) and the same
  out-of-bounds reads past `U[m]` on unusual inputs and endless loop on NaN knots. The
  shim pads with `+inf`; the generator keeps to clamped vectors of degree >= 1.
- No `UNPORTED(nlib.basis_adv)` stubs existed to wire.

## geom.grid (partial; no disagreements)

615 default cases (15 fixture, 600 generated) — 613 pass bit for bit, 2 deferred
(`SmSurfaceGridElement::Init` -> surface.surface_cache_p1, `SmSurfaceHierarchyElement::
ExpandHierarchy` -> surface.surface_p2); 30k-seed run clean. Upstream behaviour worth knowing:

- **`Step3DDDA` returns the last voxel's hits together with `rbExitedGrid = TRUE`.** The
  exit test runs before the "found something" break, so callers must use the element lists
  even when the ray left the grid. Stepping again after an exit indexes voxel `m_lOut*`
  (out of bounds); the port returns `InvalidInput`, the shim never does it.
- **`FindVoxelsInBox` builds a tolerance-expanded copy of the box and never uses it**: the
  voxel range comes from the unexpanded box (so `AddElement` / `AddHierarchyElement` do
  not pad by `m_dVoxelTolerance`). Kept.
- `GetVoxelAddress` on a grid with a zero-size axis converts NaN to `ULONG` (UB); the port's
  saturating cast gives 0 and the shim only accepts grids with positive sizes. A zero voxel
  count makes `~SmGrid` read `m_aVoxels[0][0]` out of bounds; `Grid::new` rejects it.
- `Setup3DDDA` writes `m_lAddress` before its "test point outside the grid" early return,
  and leaves `m_lStep*`/`m_lOut*` unset for axes the ray does not move along (never read).
- `AddElement(..., SM_GE_HIERARCHY, ...)` dereferences a NULL element; not expressible in
  the port (`ElementSource` has no such variant).
- No `UNPORTED(geom.grid)` stubs existed to wire.

## geom.geom_utility_p1 (green; no disagreements)

4185 default cases (185 fixture + 4000 generated) agree bit for bit with upstream
(`max_rel_err = 0`); `--perturb` catches all of them. Wiring the back-edges made
`geom.axis2_placement` (`AreCoaxial`; 4048 cases) and `geom.geom_utility_p2` (its private
stubs became calls into `p1`; 3270 cases) green with 0 deferred. `p2::PointSetType` is now a
re-export of `p1::PointSetType`. Upstream behaviour worth knowing:

- **Testing file-static helpers.** `sm_AddPoint` and `sm_AddClosestPoint` are `static` in
  `SmGeomUtility.cpp`. `harness/cpp/shim/geom_geom_utility_p1_static.cpp` compiles the
  upstream `.cpp` inside a namespace, so the shim can call them. It includes every header
  the file uses at global scope first, but not `SmGeomUtility.h`, which then lands in the
  namespace together with the definitions. That avoids clashes with the library's global
  `smgu_*` (ADL would make a global declaration ambiguous). A `using ::SmMatrix;` makes
  the header's `class SmMatrix;` forward declaration refer to the real class. Other units
  with static helpers can reuse the trick.
- **`smgu_BestFitLineToPoints` takes `aEigenVectors[2]` of `C = del*I - M`.** The code
  comment calls this the eigenvector of the largest eigenvalue, but the least-squares line
  direction minimises `v^T C v`. In practice the vector it returns depends on the Jacobi
  solver and the sort: for axis-aligned collinear points it is perpendicular to the points
  (so they classify as `SM_PST_SCATTERED`), while for points along `(1,1,1)` it is along
  the line (`SM_PST_LINEAR`). Ported as is. `smgu_BestFitPlaneToPoints` (p2) uses the
  same vector as a plane normal.
- **`smgu_CircleCenterFrom3Points` returns `FALSE` (0)** for (nearly) coincident points.
  0 is not a valid `SmStatus` and is not `SM_SUCCESS`, and the centre is left unset. The
  port returns `SolidError::Upstream { code: 0 }`.
- **`smgu_CircleFindSilhouettePoints`, perspective case**, returns the rotated unit
  directions times the radius, without adding the circle centre (the parallel case does
  add it). This looks like an upstream bug; ported as is.
- **`smgu_CircleFrom3Points`** returns `SM_ERR_WARNING` (1023) from
  `SmMatrix::SolveLinearSystem` when a pivot is near zero (`SER_MSG` passes any status that
  is not `SM_SUCCESS`), leaving every output unset.
- **`smgu_PlanePointDistance`** rejects a normal only when `|n|^2 - 1 > SM_EFF_ZERO`, so
  too-short normals are accepted.
- **`smgu_PolygonArea` with no points** reads `sPoints[-1]`. The port returns
  `InvalidInput`, and the ffi wrapper only passes at least one point.
- **`SmAxis2Placement::AreCoaxial`** takes `SmApproxTol3d`. The port uses `|tol3d|` but
  does not apply the `SM_USE_DEFAULT` substitution, because `are_coaxial` has no kernel.

## geom.tools (partial; no disagreements)

- **913 cases, 721 pass, 192 deferred** (`topology.vertex` 60, `topology.brep_p1` 64,
  `curve.curve_p2` 68). `FindFirstPointWithinTolToPnt` is fully compared; the vertex,
  coincident-edge, edge and face searches are deferred until vertex/edge tolerances, edge
  geometry, curve evaluation/drops, `SmBrep::GetEdges`/`GetTolerance` and
  `SmTopologySolver::BrepPointSolve` land. Their C++ side already runs on real B-reps
  (`cone`, `sphere`, `disk`, `plane_square`, `torus`, `box_with_large_fillet`, `Mouse`,
  `plane_circ` `.smb`); the Rust runner searches placeholder edges because it cannot read
  `.smb` yet. Whoever ports `.smb` reading should switch the `coincident`/`edge`/`face`
  cases of `crates/difftest/src/units/geom_tools.rs` to the real B-rep.
- **`FindFirstPointWithinTolToPnt` never reports a double hit.** Both branches of the
  `closest < 0.5 * second` test set `rbIsFoundArg = TRUE` (comment: two loops may touch at
  a vertex), so `found` is just `closest < tol`. Ported as is.
- **`FindCoincidentEdge` leaks a rejected candidate's orientation**: `rbSameOrient` is
  written before the max-distance check, and with no edge found `rbSameOrient` /
  `rlFoundIndex` keep whatever they held. The port returns `None`; the shim reports them
  only on a hit.
- **`SmBrep::ReadFromFile` crashes on old (`DB version 30`) ASCII files**: `SmallBox.smb`
  and `Platform.smb` (pt_TestFiles) dereference a NULL `SmShellData` in
  `SmTopologyData::ReadFromDB` (`SmBrepData.cpp:4307`), or fail with a non-success status,
  depending on heap state. They are not used as fixtures.

## nlib.crv_basic (partial; no disagreements)

Difftest: 3417 cases (417 fixture + 3000 generated), 3139 bit-identical (max_rel_err 0),
278 deferred; a 30,000-generated-case run is also clean. Fixtures use the six upstream
`.smc` curves, the `.sms` surfaces (cross-boundary derivatives), the rectangle / polygon
segments of `TestSmCreateRectangle` / `TestSmCreateRegularPolygon` (including the far-centre
degenerate ones), the 16 line pairs of `my_test_reversed_line_join`, conic data and edge
cases.

- **Deferred: `N_CrvsAreCoincident`** calls `N_CrvsMakeCompatible` (unit
  `nlib.crv_tool_p2`, back-edge). The Rust side returns `Unported` (stub
  `UNPORTED(nlib.crv_tool_p2)` in `nurbs/crv_basic.rs`); every `coincident` case is
  deferred until that unit lands. Upstream turns that call's failure into `false`; the port
  propagates the error. The FFI only passes well-formed clamped B-splines to it.
- **Uninitialised output in `N_CrvGetNumAndDenom`.** With a rational curve, a NULL numerator
  and a too-small denominator, upstream allocates the numerator (`malloc`), then fails
  (`NL_STO_ERR`) before writing it: the numerator holds garbage (Rust: zeros). The generator
  always passes an existing numerator in that situation.
- **`N_CrvGetConicData` loops forever** for non-finite angles or angles where `ae + 360 == ae`;
  the port returns `InvalidInput` for non-finite angles and after 2^20 turns. An unknown
  conic flag leaves `n`, `p` unwritten upstream (not expressible with `ConicType`).
- **`N_CrvIsPlanar`** reads `ang` from `N_VectorsAngle` even when that call failed (stale or
  uninitialised value) if the cross product still normalises; the port keeps the previous
  vertex's angle (NaN at the first). It also writes `PP`/`NN` (zeroed `NN`) on its error
  path; the port returns only the error and the difftest does not compare them then.
- **`N_CrvReparamToInterval`** writes `U[m-p..]` for `m < p` (negative index): `InvalidInput`.
- **`N_CrvSizeArrays` compares with the indices in use**, not the storage (as
  `N_CFuncSizeArrays`); the port is faithful (unlike `nlib.srf_basic`'s `size_arrays`).
- **`N_CrossBoundDerivCrvNonRatSrf`** reads `Pw[1]` / `Pw[n-1]` and `U[p+1]` / `U[n+p]` (v
  likewise) without checks; too-small surfaces are `InvalidInput` and refused by the FFI.
- No `UNPORTED(nlib.crv_basic)` stubs existed to wire.

## geom.transform (partial; no disagreements)

- **`SmTransform::EvaluateBoundingBoxSimple` dereferences a null `pOptParamPseudoBox`.**
  When an output is requested, it calls
  `sThisParamPseudoBox.AreAligned(*pOptParamPseudoBox, alAxisMap)` before checking
  `pOptParamPseudoBox == NULL`, and `AreAligned` reads the other box's basis. So the
  default call (`pOptParamPseudoBox = NULL` plus an output box) crashes. The difftest
  passes a default `SmPseudoBox` for every case that requests an output. Those cases
  are deferred until `geom.pseudo_box` lands. Whoever wires the
  `UNPORTED(geom.pseudo_box)` stub there has to choose what the null case does.
- **The aligned branch of `EvaluateBoundingBoxSimple` scales the parameter intervals
  by `1/|axis|` and shifts them by `origin·axis`.** That is not the image of the box
  under the transform, for example under a pure translation. It is still behind the
  `geom.pseudo_box` stub; whoever ports it should keep it as is.
- **`SmTransform::ReadFromDB` ignores the status of the base-class read**
  (`SmVolume::ReadFromDB`) and always returns `SM_SUCCESS` once the 16 matrix values
  are read.

## geom.a_object (partial; no disagreements)

The difftest has 2040 default cases: 8 fixtures derived from upstream tests, 32 edge-case
fixtures and 2000 generated cases (types, NLI helpers, notify/attribute/cache scripts).
A 50,040-case run (`--seeds 50000`) was also clean. Every case that is not deferred
matches upstream exactly. The deferred cases are on `topology.brep_cache` (a new
`SmBrepCache`), `topology.shell` (`SmRegion::IsVoid` in region propagation) and
`topology.topology` (`SmSAGObject::Notify` ends in `SmTopology::Notify`).

Notes for later agents:

- **Data model.** `geom::a_object::notify` drives the `AttributeStore` handlers of
  `geom.attribute`. Pass it the notified object as an `ObjectRef`; region-ness is
  `type_code == SmRegion_TYPE`. Caches live in `CacheStore<T>` and `AObject` holds
  the slot. A class's `CacheMakeOrValidate` override is chosen by passing a
  `CacheMaker`. The curve and surface makers are `UNPORTED(curve.curve_p5)` and
  `UNPORTED(surface.surface_p1)`.
- **Region propagation (`SM_NO_REG_PROPAGATION`) is quirky upstream. It is ported
  as is but stays behind `UNPORTED(topology.shell)`:**
  - The source loop runs twice. The second pass resets `sAttributes1` and keeps
    `sAttributes2`.
  - `lAttrib2Size` is taken *before* `GetAttributes(sAttributes2)` refills the
    list. On the first pass the other region's attributes are therefore never
    visited. With two or more other regions, upstream indexes the refilled list
    with the previous region's count, which reads stale or out-of-range entries.
    Rust returns `InvalidInput` where the index is out of range.
  - `SET_FROM_OTHER` removes the target attribute while the snapshot still holds
    it. With a duplicate list entry, that becomes a read after delete.
  - The difftest skips (status -2) every region-propagation case with more than
    one other region, the caller as its own other region, a duplicate list entry,
    or a non-vector `SM_AI_COLOR` attribute. For merges, it skips sources that
    carry `SM_AI_OTHER_REGION_ID`, because the merge-region handler casts and
    dereferences without checks.
- **`SmSAGObject::IsKindOf` chains to `SmAObject::IsKindOf`**, which skips
  `SmOwningTopology` and `SmTopology`. `SmBrep::IsKindOf(SmTopology_TYPE)` is
  therefore FALSE upstream. The port keeps this.
- **`SmSAGObject::CacheMakeOrValidate` stores a newly built cache in `this`, not
  in the target object.** The two are the same object when it is called from
  `GetOrCreateObjectCache`.
- No `UNPORTED(geom.a_object)` stubs existed to wire. The attribute-list methods
  remain co-ported in `geom::attribute::AttributeStore`.

## geom.pseudo_box (green; no disagreements)

3011 difftest cases (11 fixtures + 3000 generated) are bit-identical. Upstream
behaviour worth knowing:

- **`SmExtent3d::Circumscribe` of an axis-aligned pseudo box mixes up the axis map.**
  `IsAxisAligned` fills `alAxisMap[i]` with the axis that *basis* `i` is parallel to,
  but `Circumscribe` reads `alAxisMap[i]` as the basis parallel to *axis* `i`. Swaps and
  sign flips come out right, but a 3-cycle does not: basis `(y, z, x)` with intervals
  `[1,2], [3,5], [-7,-6]` gives `min (3, -7, 1)` instead of `(-7, 1, 3)`. Ported as is
  (fixture `circumscribe_axis_cycle_quirk`). `geom.extent3d` is green now that this is
  wired.
- **`DistanceToPoint`** returns `SM_ERR` (1001.0) as a distance when the point is outside
  more than three planes. That only happens for inside-out boxes (`Init()` state). When
  `smgu_IntersectTwoPlanes`/`ThreePlanes` fail it ignores the status and uses the
  uninitialised (`SM_UNDEF_DOUBLE`) line or vertex.
- **`ProjectToPlane`** projects the corners *parallel* onto the plane for every projection
  type (the perspective type only changes the basis vectors). It writes into a
  caller-provided box and keeps that box's basis on the rare `SetBasis` failure. The
  `MakeUnitOrthoVectors` branch writes `m_aBasis` directly without updating
  `m_bOrthogonalBasis`. When that branch fails (seen only for an uninitialised basis with
  huge intervals), basis vector 0 is left as `(0,0,1)`. All three are kept in
  `project_to_plane_into`.
- The `SM_IS_BETWEEN_TO_TOL(s, 0, 1, tol)` test in the rotation branch has an
  always-false reversed-interval half; the port keeps only the live half.
- **Wired `SmTransform::EvaluateBoundingBoxSimple`** (unit `geom.transform`). A requested
  output without `pOptParamPseudoBox` (an upstream null dereference) is `InvalidInput` in
  the port. The unaligned-and-bounded branch still needs `SmVolume::EvaluatePoint` and
  returns `Unported(surface.volume_p2)`. The 113 `geom.transform` cases deferred on
  `geom.pseudo_box` now pass. 256 cases are still deferred, on `surface.volume_p1`/`_p2`.

## curve.hermite_curve (partial; no disagreements)

2131 difftest cases (131 fixtures + 2000 generated): 2038 bit-identical, 93 deferred
(`IsKindOf` of a type other than `SmHermiteCurve_TYPE` reaches `SmCurve::IsKindOf`,
unit `curve.curve_p2`). Upstream behaviour worth knowing:

- **`Evaluate` leaves its last entry unset for `lNumDerivatives >= 4`.** The zeroing
  loop is `for (i = 4; i < lNumDerivatives; i++)`, so `aPointAndDerivatives[n]` keeps
  whatever the caller had (an `SmVector3d` default is `SM_UNDEF_DOUBLE`). The port
  returns zero there; the shim pre-fills the caller array with zeros so the two agree.
- `ReadFromDB` reads into `new SmHermiteCurve()` whose vectors are `SM_UNDEF_DOUBLE`
  (the `SmVector3d` member initialisers), so a skipped ASCII extraction (stream
  already failed) leaves `1.23456e-61`, and a value that fails to parse becomes `0`.
  Fixtures `db_ascii_truncated`, `db_ascii_garbage`, `db_ascii_empty` cover this. A
  short *binary* read is undefined upstream (see `topology.database_io`) and not tested.
- `CalculateBoundingBox` sets the basis of the caller's pseudo box without resetting
  its intervals (unless the chord is zero, which resets the whole box), so a pseudo
  box that already holds points keeps them; fixtures `*_preset` cover it.

## nlib.crv_tool_p1 (partial; no disagreements)

2143 default cases (143 fixtures on the `.smc` test curves, 2000 generated): 2063 agree bit
for bit (`max_rel_err = 0`), 80 deferred. C++ runs both the in-place and out-of-place forms;
the Rust methods return a new curve. Upstream behaviour worth knowing:
- **Deferred:** `N_CrvRemoveKnot` on a rational curve calls `N_CrvGetMinMaxWeightsAndPts`
  (`nlib.crv_nurb`). That routine always returns `pmax = 0`, so the scaled tolerance is
  `tol * wmin`. Wire `UNPORTED(nlib.crv_nurb)` in `nurbs/crv_tool/p1.rs`; the rational
  weight check after it is already ported.
- **`N_CrvRemoveKnot` error flag:** a failing `N_BasisFindGlobalMax` gives an assumed maximum
  of 1, but the flag stays in `error`. If the last such call failed, the routine finishes the
  removal and still returns 1. The port returns only the error then; the shim dumps nothing
  on a nonzero status.
- **No input checks:** `N_CrvInsertKnot`, `N_CrvSplit`, `N_CrvDecomposeBez`, `N_CrvRefine`,
  `N_CrvExtractCrvSeg` and `N_CrvRemoveKnotMaxErr` don't validate the curve. For unclamped
  knots, multiplicity above `p`, out-of-range `ul`/`ur` or wrong `(r, s)` they index outside
  their arrays. The port returns `InvalidInput`, and the FFI refuses such inputs.
  `N_CrvDecomposeBez` past `nsp` segments does `NL_QUIT` with no error number.
- **Fixture parameters:** serde_json (without `float_roundtrip`) can parse decimal text
  1 ulp off, so knot-valued parameters in fixtures use `{"knot": i}`. Without it, the
  `curve0.smc` end knots fell outside the range.
- No `UNPORTED(nlib.crv_tool_p1)` stubs existed to wire.

## nlib.crv_fit_p1 (green; no disagreements)

2084 difftest cases (84 fixtures from `FitCrvPoints.pts`, `Points.pts`, rectangle / hexagon
corners of `TestSmCreateRectangle` / `TestSmCreateRegularPolygon`, and edge cases; 2000
generated), all bit-identical (`max_rel_err = 0`).

- **`N_FitCrvFirstDerivAndKnots` writes `M[2i+1][-1]`** whenever the span of `u[i]` gives
  `l == 0` (not with the knots of its driver `N_FitCalcKnotsDerivs`, where `l == 1`). The
  banded matrix is one row-major block, so that is the last entry of row `2i` (band column
  `bw-1`, i.e. a wrong matrix entry). The port writes exactly there; fixture
  `first_derivs_row_underflow_p2` matches C++ bit for bit through it.
- **Unwritten outputs.** `N_FitCalcCrvParamValues` leaves the interior parameters unwritten
  when the total chord length is `<= NL_PTOL`, and `N_FitCalcKnotVectorDeriv` leaves the
  interior knots unwritten for `p == 0`; the port returns zeros there and the shim passes
  zero-filled buffers.
- **Out-of-bounds reads/writes refused by the FFI and `InvalidInput` in Rust:**
  `N_FitCalcKnotVectorEndDerivs` / `N_FitCalcKnotVectorDeriv` with `p == 1` and one
  parameter read `u[1]` / `u[-1]`; `N_FitCrvApproxKnots` copies `n+p+2` knots before checking
  the knot count, writes `Pw[1]` for `n == 0`, and its band bookkeeping (`start` / `end` /
  `index`) indexes `B` correctly only when the evaluated parameters `u[1..m-1]` are
  non-decreasing; `N_FitCrvCPtsFromSrfData` with a matrix larger than the extracted row /
  column reads uninitialised entries; `N_FitCrvInterpGivenParams` with `2 <= n < p` gives
  `N_BasisEval` too few knots.
- `N_FitCrvKnotsAndDeriv` uses band offset `l = 0` for every row when `p == 2` and
  `l = j - i - 1` otherwise (not `sp - i + j` as its two-derivative sibling); ported as is.
- No `UNPORTED(nlib.crv_fit_p1)` stubs existed to wire.

## geom.point_set (green; no disagreements)

2029 difftest cases (29 fixtures + 2000 generated) are bit-identical. Upstream
behaviour worth knowing:

- **`SetUpTightPseudoBox` never projects its points.** The "extra planar work" means to
  project the boundary and neighbour points onto the box mid-plane before measuring
  angles, but `SmPoint3d::ProjectPointToPlane` is `const` and returns the projection;
  the results are discarded (`sBegPt0.ProjectPointToPlane(..) ;`). The angles are
  measured between raw points. A port that projects differs in the last bits of the
  rotation angle, which shows up as a ~1e-15 vs ~1e-16 basis component on slightly
  non-planar sets. Ported as is.
- **`SetUpCentroid` block sums.** The first block holds `size % 50` points and the loop
  runs `size / 50 + (extras ? 1 : 0)` times. For a size that is a positive multiple of 50,
  the first block is empty and the last 50 points are never added (50 points give a
  centroid of `(0,0,0)`). `SetUpCentroid` also never sets `m_bCentroidSet`, so
  `GetCentroid` recomputes every time. After `GetNormal` (or any box query), however,
  the centroid is the best-fit plane's centroid (`smgu_BestFitPlaneToPoints`), which
  is computed correctly. So `GetCentroid` depends on the call order. Kept (fixtures
  `centroid_blocks_*`).
- **Reads outside the array (shim rejects).** `GetArea` on an empty sequence calls
  `smgu_PolygonArea`, which reads `m_pData[-1]`. `GetTightPBox` on an empty set built
  with a normal (classified planar) reads `m_pData[0]`, which segfaulted in difftest.
  The Rust port returns area `-1` and skips the extra work, using `(0,0,0)` for the
  missing points.
- `SetUpAxes` returns the point-set type as its `SmStatus`; every caller ignores it.
  `SmPointSet3d::SetNormal` assigns to its own argument (`sNormal = m_sNormal`) and so
  only clears the cache. It is not reached in scope and is not ported.

## topology.topology (green; no disagreements)

657 difftest cases (57 fixtures + 600 generated) agree exactly. Upstream behaviour
worth knowing:

- **`SmTopologyChangeCallback::Execute` belongs to no unit.** The call graph never
  reaches it (it is in `harness/callgraph/unused_functions.txt` with `AddUnique`,
  `Remove` and both `Dump`s), so the tracker's lists are only ever reset. The Rust
  callback that `TrackTopologyChanges` installs returns `SolidError::Unsupported`
  from `execute`, which `SmObject::Notify` ignores. Upstream `Execute` only records
  faces, edges and vertices, so with the bare `SmTopology` objects in the difftest
  the C++ lists stay empty too. A future unit that needs real change tracking has to
  port `Execute` (it walks surface/curve owners and edgeuse edges).
- **Starting a tracker twice leaves its callback installed after it dies.** The
  second `StartTracking` sees the tracker's own callback in the slot and stores it in
  `m_pLastNotifyCallback`; `StopTracking` (and so the destructor) then does nothing,
  and the context keeps a dangling `SmNotifyCallback*`. The next `Notify` calls
  `Execute` on a destroyed object; a later tracker allocated at the same address is
  even reported as tracking. The shim skips (`ST_SKIP`) destroying a tracker in that
  state, and the Rust runner applies the same rule. Rust's tracker recognises its
  callback by a caller-chosen key, so address reuse cannot fake `IsTracking`.
- **`StopTracking` clears whatever callback is installed** when this tracker found
  the slot empty at start, including a foreign one installed since (fixture
  `track_stop_clears_foreign`). Ported as is.
- **Notify asymmetries** (ported as is): a merge ORs the two targets' flags into both
  but copies no flags from the source; a copy (`SM_NO_COPY`, or `SM_NO_CONSTRUCTION`
  with a source) zeroes the target's three general marks while a split copies them;
  `m_lMarkIO` and `m_lMarkAssert` are never propagated; `SM_NO_MERGE_IN_BREP` takes the
  *deleted* object (`pData2`) as the source (the upstream "GWC:TODO try typo bug fix").
- `SmTol::GetZoneTol3d(const SmObject*)` and friends dispatch to the *derived*
  `GetTolerance` overrides; their `UNPORTED` markers now name `topology.vertex`,
  `topology.edge`, `topology.face_p2`, `topology.edgeuse_p1` and `topology.brep_p1`
  instead of this unit.

## nlib.crv_nurb (`NL_CrvNurb.cpp`)

No Rust-vs-C++ disagreement: 2852 of 2871 difftest cases are bit-identical, 19 deferred
on `nlib.crv_geom` (`N_CrvArcLength`). Upstream behaviour worth knowing:

- `N_CrvGetMinMaxWeightsAndPts` always returns `pmax = 0` (deliberate, see its header);
  the removal tolerance of rational `N_CrvRemoveKnot` is therefore `tol * wmin`.
- `N_CrvReverse` has two codings: in place (swap loops over `n/2` and `m/2 - p`) and
  into a new curve (copies `p + 1` knots at each end from `UP[i]` and `UP[n+i+1]`).
  They agree only when `m = n + p + 1`; the port keeps both (`reverse` / `reversed`).
- `N_CrvGetDegenSegs` ignores the error flags of its `N_CrvEval` calls (a failed call
  leaves the point unset); the port propagates them (unreachable for clamped curves).
  For single-span curves it stops the whole scan at the first short-chord span whose
  midpoint detour exceeds `tol` (`break`, not `continue`).
- `N_CrvGetType` clears the errors of `N_CrvEval` / `N_CrvEvalCurvature` and reports
  `NL_NFREEFORM`; degree-1 polylines that are not lines are therefore always free-form
  (`N_CrvEvalFrenetFrame` fails with `NL_DEG_ERR`). In the line test `kh` is decremented
  for the last span and stays decremented. The circle normal `BB` is the unnormalised
  average osculating normal; `XX`/`YY` and the angle are written after `AA`/`BB`, so a
  failing normalisation returns an error with partial outputs.
- `N_CrvDerivs` aliases `left`, `right` and the denominator derivatives `dd` with the rows
  of its scratch array `a`; the port uses the same aliasing.

## nurbs.nurbs_crv (`SmNurbsCrv.cpp`)

No Rust-vs-C++ disagreement: all 4089 difftest cases (1089 fixture + 3000 generated)
are bit-identical, 0 deferred; a 51,089-case run was also clean. Upstream behaviour
worth knowing:

- `sm_BasisEval` starts its flag at `NL_YES` for early exits, so at `u == U[p]` and
  `u == U[m-p]` it fills `N` but returns the error flag; `sm_CrvEval` therefore fails
  at both domain ends. Its only caller (`SmBSplineCurve::EvaluatePoint`) evaluates the
  end control points itself there. The port returns `Unsupported` at the ends.
- `sm_CrvTrim` returns the flag value `NL_INP_ERR` (14), not `NL_YES`, for `ur <= ul`,
  without `N_ErrSet`; callers comparing with `NL_YES` treat it as success.
- `sm_CrvDerivs` rejects `der > GW_MAX_DERIV` (5) but `der == 5` writes past its
  `[GW_MAX_DERIV]` row arrays; the port rejects `der >= 5`.
- `sm_CrvRemoveKnots` can return a stale `NL_YES` from a failed (and recovered)
  `sm_BasisFindAllSpanMaxima` when no knot is removed; the port returns that flag as
  `Ok(bool)`. Its rational weight check `continue`s after already updating the error
  vector `er`.
- `sm_CreateBezSegments` keeps the "next segment" pointer from the previous iteration
  for the last segment (and NULL for a single span); writes through it happen only
  for knot vectors with trailing over-multiplicity.
- The `sm_Basis*` index/convergence errors go through `smos_ErrorMessage`, which does
  not set `NL_ERROR.eno`; the difftest compares returned flags only.

## nlib.crv_geom (`NL_CrvGeom.cpp`)

No Rust-vs-C++ disagreement: 1723 of 1723 difftest cases bit-identical (223 fixtures from
the six upstream `.smc` curves plus edge cases, 1500 generated). Wiring
`UNPORTED(nlib.crv_geom)` (`N_CrvArcLength` in `N_CrvGetDegenSegs`) made `nlib.crv_nurb`
green (2871/2871, 0 deferred). Upstream behaviour worth knowing:

- `N_CrvClosestPt` writes its best `u`/`Q` even when it returns `NL_CON_ERR`, and
  callers use them (`NL_CrvApprox.cpp` reads `Q` after a failure; in-place calls such as
  `&u[k]` keep the best value). The port exposes this as `Curve::closest_pt_search`
  (best values plus error, explicit `NL_ITLIM`); `closest_pt` is the strict form. When
  `N_CrvDerivs` fails on the first step (`u0` outside the knots: `NL_PAR_ERR`) the
  outputs stay unwritten (`best = None`).
- `N_ApproxCrvWithPolyline` subdivides forever when the tolerance is below the
  floating-point resolution of the curve (e.g. `tol = 0`, or a relative tolerance on a
  zero-length curve): a closed/degenerate segment test `dd < tolsq` never succeeds and
  coincident control points keep splitting. The port stops after
  `POLYLINE_MAX_SPLITS` splits with `NotConverged`; the ffi rejects such tolerances.
  With `tol <= 0` the last vertex is never appended (`dd > 0.01*tol` fails).
  For `p = 1` the tolerance (and its flag) is ignored and the knots `U[1..=n+1]` are
  returned as parameters.
- `N_CrvArcLength` returns its bare error flag (no `NL_ERROR` number) when the Bezier
  length recursion exceeds `NL_RECURSELIM` (e.g. `tol = 0`); the difftest maps it to
  status 1000 on both sides. A tolerance flag other than `NL_ABSOLUTE`/`NL_RELATIVE`
  fails with `NL_CAL_ERR`, but only after the line/circle shortcut.

## nlib.crv_adv_p1 (`NL_CrvAdv.cpp`, part 1)

No Rust-vs-C++ disagreement: 2670 of 2670 default difftest cases bit-identical (670
fixtures: the six upstream `.smc` curves, the `TestSmCreateRectangle` /
`TestSmCreateRegularPolygon` edges as degree-1 curves, and edge cases; 2000 generated),
and a 100,670-case run (`--seeds 100000`) also clean. The file-private `ST_curxcdr` /
`ST_curxcrr` are ported as `Curve::extend_by_dist_at_depth` /
`Curve::extend_by_param_dist_at_depth`; their one-line public wrappers
`N_CrvExtendByDist` / `N_CrvExtendByParamDist` belong to `nlib.crv_adv_p2` (call them
with `depth = 0`). The shim reaches the `ST_` routines through those wrappers. No
`UNPORTED(nlib.crv_adv_p1)` stubs existed to wire. Upstream behaviour worth knowing:

- **`ST_curxcrr` in place frees an uninitialised curve.** Its `EXIT` label runs
  `if( curP EQ curQ ) N_FreeCrv( &curA, SP )`, but `curA = *curP` is assigned only after
  the `NL_G2` two-step recursion (`par` more than `0.9213` parameter ranges outside) and
  the `N_BasisFindSpanAndMult` calls. An in-place `NL_G2` call that recurses (or fails
  before that point) frees stack garbage: the difftest crashed with SIGSEGV on such a
  case. The ffi rejects in-place `NL_G2` extensions that would recurse; out of place
  (and every nested recursive call: the second step is in place but never recurses) is
  fine. `ST_curxcdr` frees `curA` before its `EXIT` label and has no such problem.
- **Bare error flags.** The recursion limits (`ST_curxcdr` depth > 50, `ST_curxcrr`
  depth > 10) `return (1)` without an `NL_ERROR` number, as does `ST_curxcrr`'s
  `NL_QUIT` when the knot-inserted curve has an unexpected size (unreachable for clamped
  curves). The port returns `NotConverged` / `InvalidInput` (`BARE_FLAG_REASON`); the
  difftest maps both to status 1000. A relative `NL_G2` distance above ~24.8 curve
  lengths hits the `ST_curxcdr` limit, a `NL_G2` parameter more than ~6 ranges out hits
  the `ST_curxcrr` limit.
- **`ST_curxcdr` `NL_G2` parameter search never ends** when the step
  `du = range * ldst / d1` no longer changes the parameter (or is zero: `dist = 0`), and
  reads outside the knot range for a negative distance. The port fails with
  `NotConverged` when a step changes nothing; the ffi only passes positive distances of
  at least `1e-3` curve sizes for `NL_G2`.
- **Tangent fallbacks read past short curves.** When the end pair of control points
  cannot be normalised, `NL_G1`/`NL_G2` (of `ST_curxcdr`, `NL_G1` of `ST_curxcrr`) retry
  with the third point from the end, which does not exist for `n < 2`; the ffi rejects
  that. `ST_curxcrr`'s `NL_G2` has no fallback (`NL_GEO_ERR` at once).
- **`N_CptReflect`'s flag is ignored** by both extension routines; it cannot fail for
  their (normalised) plane normals. The port propagates it.
- **Extensions that do not extend.** `ST_curxcrr` with `par` on or inside the extended
  end, and `ST_curxcdr` with a non-positive distance, build knot vectors with too many
  equal or decreasing knots that later calls index with; the ffi requires a strict
  extension. `NL_CMAX` secant steps of `ST_curxcdr` can in principle leave the span
  length non-positive; never seen in the runs.
- `N_CrvEvalEvenSpacedPts` reads the global `NL_ITLIM` (default 50, lowered temporarily
  by some approximation routines); the port uses `crv_geom::ITER_LIMIT`.

## nlib.crv_adv_p2 (`NL_CrvAdv.cpp`, part 2)

No Rust-vs-C++ disagreement: 2815 of 2815 default difftest cases bit-identical (815
fixtures: the six upstream `.smc` curves, the `TestSmCreateRectangle` /
`TestSmCreateRegularPolygon` edges as degree-1 curves, and edge cases; 2000 generated),
and a 300,815-case run (`--seeds 300000`) also clean. `N_CrvRemoveKnotsDerivConstraints`
is `N_CrvRemoveKnots` plus a constraint check, so `crv_tool::p2::Curve::remove_knots`
now shares its body (`remove_knots_constrained`; `nlib.crv_tool_p2` re-run: 2124/2124,
report unchanged). No `UNPORTED(nlib.crv_adv_p2)` stubs existed to wire. Upstream
behaviour worth knowing:

- **`N_CrvSumDiffCrv` corrupts the heap for some rational sums.** The rational branch
  computes `num = D_q * N_p` and `tmp = D_p * N_q` with `N_CrvFuncMultiply*`, which
  rescale the knot vector of the operand with the shorter parameter range **in place**
  (so `D_p`/`D_q` change between the calls), then adds `tmp`'s control points into
  `num` for `i <= n` of `tmp`. When the two products come out with different knot
  vectors (curves over different parameter ranges, or interior knots closer than
  `NL_PTOL` but not equal) `n` of `tmp` exceeds `num`'s and upstream writes past `num`'s
  control points: the difftest died with `double free or corruption` /
  `free(): invalid next size` on such cases (or returned `NL_INP_ERR` from
  `N_CreateCrvFromNumAndDenom` when it survived). The port returns `InvalidInput` there;
  the ffi only passes rational sums over identical parameter ranges whose knots are
  equal or more than `1e-9` ranges apart. Non-rational sums over different ranges are
  fine (rescaled once by `N_GetCompatibleKnotArrayMult`) and tested.
- `N_CrvSumDiffCrv` and `N_CrvReparamRat` call `N_CrvNonRatToRat` on their **input**
  curve; `N_CrvSumDiffCrv` converts it back at the end (not on its error paths),
  `N_CrvReparamRat` leaves even an out-of-place input rational. The port leaves inputs
  alone.
- `N_CrvRatGetFirstDeriv` squares the denominator with `N_CrvFuncMultiplyCrvFunc(&d, &d)`
  (the same object twice); the port multiplies a copy (identical: equal ranges, nothing
  rescaled). Interior knots of multiplicity `p` make the squared denominator's
  multiplicity `2p`, and `N_BasisIncreaseKnotMult` then fails with `NL_KNT_ERR`.
- `N_CrvEvalUnboundedPtsAndDerivs`: the periodic shift of a G1-closed curve
  (`u = ue - us + u`) loops forever once the range is below the resolution of `u`; the
  port fails with `NotConverged`, the ffi bounds `u` and the range. Extensions run in
  place (`ST_curxcrr` with `curP == curQ`) in steps of at most `0.9` ranges, below the
  `0.9213` two-step split, so the in-place `NL_G2` free of `nlib.crv_adv_p1` cannot
  trigger. `ecur` keeps a partially extended curve after a failed step; the port keeps
  the last complete one (the shim stops at the first error).
- `N_LinearMultiplyCrv` ignores the error of `N_CrvGetGrevilleAbscissa` (degree 0) and
  then uses an uninitialised parameter; the port propagates the error.
- `N_CrvOffsetGetMax2ndDeriv` and several others allocate `N_AllocReal1dArray(mu)` and
  write index `mu`: NLib's allocators hold `hi + 1` entries, so this is in bounds.

## nlib.crv_common (green; no disagreements)

- Rust and C++ agree bit for bit on all 2137 cases (no disagreements).
- The thin wrappers `N_CreateQuadraticArc`, `N_CreateQuarticArc`, `N_CreateQuinticArc`,
  `N_CreateEllipticalArc`, `N_CreateEllipticalArcAngType` (all `at = 0.01`, `atp = 0`
  for the first ellipse form) are not owned by any unit in `harness/callgraph/units.json`.
  Callers (e.g. `NL_CrvApprox.cpp`, `NL_SrfCommon.cpp`, `NL_Iges.cpp`,
  `SmBSplineSurfaceNLib.cpp`) should call the `*_tol` functions of
  `nurbs::crv_common` with `DEFAULT_ANGLE_TOL` (and `AngleType::Circle`).
- `N_CreateQuadraticArcTol` reads `N_IsectLineLine`'s uninitialised `ints` flag when
  that call fails (zero tangent / bad division); the port fails with `NL_INP_ERR`.
- `N_CreateEllipticalArcAngTypeTol` loops forever on a non-finite end angle (and
  `N_CrvGetConicData` on huge angles); the port returns `InvalidInput` and the ffi
  rejects non-finite inputs, angles above 1e6 degrees and zero ellipse radii (0/0 in
  the polar angle).

## nlib.crv_fit_p3 (`NL_CrvFit.cpp`, part 3)

No Rust-vs-C++ disagreement: 2242 of 2242 difftest cases bit-identical (242 fixtures on
`FitCrvPoints.pts` / `Points.pts` and the call shapes of the upstream drivers, plus 2000
generated). No `UNPORTED(nlib.crv_fit_p3)` stubs existed. Upstream behaviour worth knowing:

- `N_FitCrvDerivs` / `N_FitCubicSplineInterp` estimate a zero `Ds` / `De` from a cubic
  Bezier through `P[0..=3]` / `P[k-3..=k]` whatever `k` is (out of bounds below four
  points, e.g. the `kk = 1` / `kk = 2` calls in `NL_CrvApprox.cpp` only work because they
  pass nonzero derivatives) and ignore the errors of that fit, of `N_CrvDerivs` and of the
  normalisation (a failed normalisation leaves the vector unnormalised and scales it by
  the polygon length). The port returns the fit / derivative errors (`InvalidInput`
  below four points); the ffi rejects those inputs.
- `N_FitCrvDerivs` and `N_FitCrvFirstDeriv` scale the end derivatives by `U[p+1]/p` and
  `(1 - U[n])/p`, i.e. assume the knots start at 0 and end at 1 (the comment in the
  source says "this may be where the routine gives poor results"). Ported as is.
- `N_FitCrvDerivs` with `par = NL_INHERITED` reads `u[i] = U[i+p]` from the knots
  already in the caller's curve and keeps them; with a NULL curve it reads freshly
  allocated, unset memory. The port takes the curve slot (`&mut Option<Curve>`, sized
  with `size_arrays`); a NULL slot gives zero knots.
- `N_FitCrvFirstDeriv` with `p = 2` copies only `ND[0..1][0..=1]` into each row (`hp =
  p - 1`), dropping the third basis function; ported as is.
- `N_FitCrvKnotsAndTangents` ignores the error of `N_FitCalcKnotVectorDeriv` (`p > k + 1`
  with one tangent); `N_FitCrvKnotsAndDeriv` then fails with `NL_INP_ERR` before reading
  the unset knots, which the port returns directly. Callers pass the same vector as `Ts`
  and `Te` (`NL_SrfAdv.cpp:22688`); it is then normalised and scaled twice
  (`EndTangents::Same`). The merged knot vector would have unset tail entries if a new
  knot were not below `UT[mt]` (cannot happen for increasing parameters); the port
  returns `InvalidInput` there.

## nlib.spiral (`NL_Spiral.cpp`, `SmNurbsVol.cpp`)

No Rust-vs-C++ disagreement: 2032 of 2032 difftest cases bit-identical (32 fixtures from
`TestSmCreateHelixInvalidInputs`, `smSurfaceTest.cpp:1975` and edge cases, plus 2000
generated; a 20k-seed run is clean). No `UNPORTED(nlib.spiral)` stubs existed.
Upstream behaviour worth knowing:

- `N_CrvApproxSpiral` crashes on its own input errors: the checks `height < NL_MTOL`,
  a radius `< -NL_MTOL`, `rad0 + rad1 < NL_MTOL` (after zeroing radii below `NL_MTOL`)
  and `NumTurns < NL_MTOL` run `NL_ERROR` (goto `EXIT`) *before*
  `N_InitNurbs(&SL)`, and `EXIT` calls `N_EndNurbs(&SL)` on the uninitialised local
  stacks (`free(): invalid pointer`, e.g. height 0 with r0 = r1 = 2, turns 5). The
  public callers (`SmApiCreateHelix`, `SmBSplineCurve::CreateHelixSegment`) validate
  first, so they never reach it. The port returns `NlibCode::Input`; the ffi rejects
  these inputs (`hits_early_input_error`) and the Rust unit tests cover them.
- Fewer than 1/8 turn when fitting (`NumPoints <= 1`): the initial span count
  `(int)(8 * NumTurns + 0.99)` is 0, `dTheta` divides by zero, and the cubic spline
  fit with one point fails with `NL_INP_ERR` (after `N_InitNurbs`, so no crash).
- When the span count reaches `NL_CCPLIM - 6` (994) without meeting `tol`, the loop
  adds the next increment and stops, so `*NumPoints` comes back as
  `994 + delta + 1` while the curve was fitted with 995 points. Ported as is.
- The span count conversions are C `(int)` casts of doubles (undefined beyond the `int`
  range; Rust saturates); the ffi caps turns at 1000 and `NumPoints` at 100000.

## nlib.srf_common (`NL_SrfCommon.cpp`)

No Rust-vs-C++ disagreement: 1550 of 1550 difftest cases bit-identical (50 fixtures from
`TestSmCreateSurfaceFromCornerPoints`, `TestSmCreateExtrude`, `TestSmCreateRuledSurface`,
`TestSmCreateSurfaceRevolution`, `my_test_surf_methods` and the `.smc` curves in
`pt_TestFiles`, plus 1500 generated). No `UNPORTED(nlib.srf_common)` stubs existed.
Upstream behaviour worth knowing:

- `N_CreateRevolvedSrf` stores the circle weights from `N_CalcCircWeights` in a local
  `NL_REAL wi[9]`. A quartic (`NL_QUARTIC`) revolution of 241 degrees or more has 13
  weights and overflows the stack buffer (undefined behaviour). Upstream callers only pass
  `NL_QUADRATIC` or a caller-chosen `rtp` (`NL_SrfFit.cpp:13242`, `:13828`). The port keeps
  all the weights; the ffi wrapper rejects those inputs so difftest never runs the overflow.
- The routines change their input curves in place: `N_CreateSrfExtrudeCrv` makes a 2D
  curve 3D (after the `dir` check, before the direction is normalised, so a zero direction
  still converts the curve), `N_CreateRuledSrf` makes both curves compatible (and keeps the
  changes made before a failure), `N_CreateRevolvedSrf` makes the profile rational and 3D.
  The port takes `&mut Curve`; difftest compares the curves after the call.
- `N_CreateCylCone` reads the indices and knots of the top circle when there is one, else
  of the bottom circle; a collapsed circle (radius at most `NL_MTOL`) becomes its centre
  weighted by the other circle's weights. `N_CreateRuledSrf` takes `n` from curve B and
  `p`, `m`, `U` from curve A (equal after `N_CrvsMakeCompatible`).

## nurbs.b_spline_curve_p1 (SmBSplineCurve part 1)

No Rust-vs-C++ disagreement: 2156 of 2156 non-deferred difftest cases bit-identical (216
fixtures on the six `pt_TestFiles` `.smc` curves, the `smSweepTest.cpp` lines and circles
and edge cases, plus 2000 generated; a 30,216-case run was also clean). 60 cases are
deferred on `curve.curve_p2` (`SmCurve::EvaluateContinuity` for curves with interior knots,
`SmCurve::IsKindOf` for other types, `ApproximateLength`) and `curve.curve_p1`
(`TessellateByBisection`). Upstream behaviour worth knowing:

- `SmBSplineCurve.h` declares `Evaluate(..., bNonZeroTangents = FALSE)` while `SmCurve.h`
  says `TRUE`. Default arguments bind by static type, so the recursion inside
  `SmBSplineCurve::Evaluate` for a zero first derivative with one derivative asked
  (`Evaluate(dParameter, 2, bFromLeft, sVals)`) runs with `FALSE` and hands back the exact
  zero derivative: the "non-zero tangent" fix only acts when 2+ derivatives are asked.
  The port passes `false` there (caught by the `zero_tangent_start_nd1` fixture).
- Upstream dereferences the `NULL` NURB of a curve built with `SmBSplineCurve(lDim)` in
  nearly every query (`GetOrCreateGwNurbPointer` asserts only in debug builds); the port
  returns `SM_ERR` and the ffi wrapper only runs the NULL-safe ops on such curves.
- `RemoveExtraKnots` on a borrowed NURB swaps the shrunken copy in but keeps
  `m_bNurbIsBorrowed = TRUE`, and its temporary frees the borrowed curve; the port keeps the
  flag as upstream (ownership is moot in Rust).

## nurbs.b_spline_surface_p1 (SmBSplineSurface part 1)

No Rust-vs-C++ disagreement: 1985 of 1985 non-deferred difftest cases bit-identical (120
fixtures on the three `pt_TestFiles` `.sms` surfaces (also read back through `ReadFromDB`,
whose ASCII layout the `.sms` files share), the `smAdvancedTest.cpp` / `smTopoTest.cpp`
cone patches and bilinear patches, canonical, sweep and edge cases, plus 2000 generated).
135 cases are deferred: `surface.surface_p2` (`SmSurface::IsKindOf` for other types, which
`CalculateBoundingBox` and `IsParallelToVector` reach before anything else) and
`surface.surface_p1` (`CreateOffsetSurface`, `IsDegenerateCurve`, `STEPInversion`'s point
solves). Upstream behaviour worth knowing:

- The data constructor `SmBSplineSurface(n, m, p, q, r, s, Pw, U, V)` copies `V` with
  `smos_MemCpy(knv->U, V, (s+1) doubles, (r+1) doubles)`: the destination size is the *u*
  knot count, so when `s > r` the copy is refused (status only `SE`'d) and the v knots stay
  0 from `sm_AllocateNurbSurface`'s calloc, then `sm_FixupKnotVector` runs on zeros. The
  port reproduces this (caught by every generated `data` case with more v than u knots).
- `IsParallelToVector` returns `SER(sTestVec.Unitize())`'s `SM_ERR_INVALID_INPUT` as its
  `SmBoolean`, so a zero vector reads as TRUE; its line test calls
  `smgu_LinePointDistance(LinePnt, LineEnd, Pnt, d)`, which binds the (point, *direction*)
  overload, so the second net point is used as the line direction. Both kept.
- `CreateLinearSweep` passes the curve's own NURB to `N_CreateSrfExtrudeCrv`, which makes
  a 2D curve 3D in place (the `const` is cast away); difftest compares the curve after.
- `CalculateBoundingBox` only `SE`s (does not return) when the domain is outside the
  natural domain, and ignores `N_SrfIsFlatCheap`'s and `MakeUnitOrthoVectors`' status.

## Header-declared sizes (all file readers)

Upstream readers (`SmBSplineSurface::ReadFromDB` and friends) `malloc` whatever counts
a file header declares and fail later at end of data; ASCII readers even keep going
with stale values after a missing field. In Rust a huge allocation aborts the process
(and `n + p + 1` can overflow), so readers bound counts against the bytes left via
`DatabaseReader::ensure_available` (binary: 8 bytes per double; ASCII: 1 byte per
item) after the same validation checks upstream makes first (e.g. the rational flag).
Deviation: an ASCII file whose header declares more items than it has bytes left is
an error in Rust, while upstream reads it with garbage values. No fixture exercises
this; found by the commit security review of `nurbs.b_spline_surface_p1`.

## nurbs.b_spline_curve_n_lib (`SmBSplineCurveNLib.cpp`)

No Rust-vs-C++ disagreement: 2132 of 2132 non-deferred difftest cases bit-identical (282
fixtures on the six `pt_TestFiles` `.smc` curves, `FitCrvPoints.pts` / `Points.pts`,
`my_test_curve_interface`, the `smSurfaceTest.cpp:1975` helix, `TestSmMakeCurvesCompatible`,
the `TestSmCreateRectangle` / `TestSmCreateRegularPolygon` edges and edge cases, plus 2000
generated); a 100,282-case run was also clean. 150 cases are deferred on
`nurbs.b_spline_curve_p2`: every `SetIntersectionCurveThroughPoint` (it starts with
`IsIntersectorApprox` = `GetDegree() == 3 && IsPiecewiseC0()`) and `DeformToEndPointTargets`
of curves of degree below 3 or of even degree (`DegreeElevate`). No
`UNPORTED(nurbs.b_spline_curve_n_lib)` stubs existed to wire. Upstream behaviour worth knowing:

- **`DampenKnotSpacing` can loop forever.** Each pass halves every span more than
  `dMaximumDeltaFactor` times longer than a neighbour; for factors below about 2 the halving
  cascades without end on many knot vectors (e.g. unique knots `0 .3 .6 1` with factor 1.2),
  and a factor at most 1 never finishes. The port stops after `DAMPEN_SPLIT_LIMIT` splits with
  `NotConverged`; the ffi wrapper simulates the splitting and rejects inputs that do not
  finish within 4000 splits.
- **`CreateHelixSegment` passes inputs that crash `N_CrvApproxSpiral`.** Its checks allow a
  height in `[-SM_EFF_ZERO, NL_MTOL)`, radii (both) below `NL_MTOL` and turns in
  `[SM_EFF_ZERO, NL_MTOL)`, which hit the spiral's early input errors (the uninitialised
  `N_EndNurbs(&SL)` crash of `nlib.spiral`); the ffi rejects them, the port returns `SM_ERR`.
- **`Trim` and `sm_CrvTrim`.** `sm_CrvTrim` returns `NL_INP_ERR` (not `NL_YES`) for
  `ur <= ul`, so `Trim` would go on with a `NULL` curve; the snapping and degenerate-interval
  checks before it make that unreachable for a proper interval. The port returns
  `InvalidInput`. `SmTol::GetZoneTol3d(this)` / `SmTol::MapTo1d(tol, t, *this)` (owned by
  `geom.tol`, stubbed there for `SmCurve`) are computed locally: a curve without edge has the
  system zone tolerance, and `MapTo1d` evaluates through `SmCurve`'s static type, i.e.
  `Evaluate(t, 1, TRUE, .., bNonZeroTangents = TRUE)`.
- `CreateInterpolatingCurve` reinterprets its `const` vector array as the in/out `D` of
  `N_FitCrvFirstDeriv`, so with `bIgnoreVectorMagnitude` the caller's vectors come back
  normalised and rescaled. The port works on a copy.
- `InterpolatePoints` copies `cpOptParams->GetSize()` parameters into an array of `m + 1`
  (reading unset memory or writing past it when the sizes differ); the port requires one
  parameter per point.
- `SetIntersectionCurveThroughPoint` appends the UV control point spacings once per
  through point (2 or 3 per UV curve) but reads them by UV curve index, so UV curve `i > 0`
  uses curve 0's spacing; ported as is (deferred until `IsPiecewiseC0` lands).
- `DeformToEndPointTargets` ignores the status of its in-place knot insertions and of the
  Reg2 refinement (an empty Reg2 knot vector fails `N_CrvRefine`, which is harmless: the
  displacement curve is then already as large as the curve); the port keeps the unchanged
  curve on such a failure.

## nlib.srf_adv_p2 (`NL_SrfAdv.cpp`, part 2)

No Rust-vs-C++ disagreement: 1695 of 1695 default difftest cases bit-identical (195
fixtures: extensions of the three `pt_TestFiles` `.sms` surfaces and the
`TestSmCreateRectangle` patch in every direction / end / continuity, small and large
distances, in place and not, plus normal derivatives; 1500 generated), and a 30,195-case
run (`--seeds 30000`) also clean. The file-private `ST_surxsdr` is ported as
`srf_adv::p2::extend_by_dist_at_depth`; its one-line public wrapper `N_SrfExtendByDist`
belongs to `nlib.srf_adv_p1` (call it with `depth = 0`). The shim calls `ST_surxsdr`
directly (it has external linkage). No `UNPORTED(nlib.srf_adv_p2)` stubs existed to wire.
Upstream behaviour worth knowing:

- **Stale error number past the recursion limit.** `ST_surxsdr` returns `1` without
  `N_ErrSet` at `depth > 10`. `NL_ERROR` then holds whatever was recorded last: errors of
  the sampled iso-curve extensions (which it ignores), `N_VectorNormalize`'s `NL_GEO_ERR`
  for a degenerate reflection plane, or `0` after a callee's `N_ErrClear` (e.g. in
  `NL_CrvNurb.cpp`). `DropCurveSurface.sms` extended 900 units in v hits the limit with
  `eno = 17` at one end and `0` at the other. The port returns `NotConverged`; the runner
  compares an upstream failure as the bare flag (status 1000) only when the port reports
  the recursion limit (a C++ success there still fails the case).
- **Zero distance hangs for G1/G2.** `NL_G1`/`NL_G2` extend the sampled iso-curves with
  `ST_curxcdr` `NL_G2`, whose parameter search never ends for `dist = 0` (see
  `nlib.crv_adv_p1`). The difftest uses `dist = 0` only with `NL_CMAX` / `NL_G1R`.
- **Degree-1 reflection fallback reads past `Rw`.** When the first reflection-plane
  normal is degenerate, `ST_surxsdr` retries with `Rw[ii][kby + 2]` (start) /
  `Rw[ii][kby - 2]` (end); for a span of a degree-1 direction that is outside the
  `2 * kby + 1` row. The port returns `InvalidInput` there.
- `NL_G1` maps to the same reflection algorithm as `NL_G2` (only the number of boundary
  knots removed afterwards differs: 1 vs 2); `NL_G1R` extends the iso-curves with `NL_G1`
  but builds a ruled extension. A direction flag other than `NL_UDIR` extends in v; an
  `end` other than `NL_START` extends the end. The `static int cbi = 0` branch
  (`N_SrfExtendByParamDist`) is dead.
- `N_SrfEvalPtNormalDeriv` fails with `NL_NUM_ERR` where `|S_u x S_v|` is zero (a pole).

## nlib.srf_adv_p4 (NL_SrfAdv.cpp part 4: Coons surfaces, cross-boundary derivatives)

No Rust-vs-C++ disagreement: 3155 of 3155 difftest cases bit-identical (155 fixtures on the
six `pt_TestFiles` `.smc` curves plus Coons, knot-merge, weight-scale and derivative-strip
edge cases, and 3000 generated; a 60,155-case run was also clean). Upstream behaviour worth
knowing (all kept out of the generated inputs, documented in the module header):

- `N_ApproxCrossBoundaryDerivs` ignores the status of `N_VectorsAngle`: a zero second or
  next-to-last control point leaves the end angle uninitialised. When one end angle equals
  `eps` exactly while the other exceeds it (e.g. `eps = 0` with an exactly aligned end),
  neither knot branch runs and `N_CrvRefine` inserts the uninitialised `UX[0]` (malloc'd,
  not zeroed); upstream then "succeeds" with garbage. The port returns the angle error /
  `InvalidInput` in these cases.
- `N_SweepCrossTangentCrv` accumulates sample parameters (`params[i] = params[i-1] + du`),
  so the last one can round past the last knot; it ignores the `N_CrvEval` failure and
  reuses the previous sample. The port does the same (only a failure on the very first
  sample, where upstream reads an uninitialised point, is returned).
- `N_MergeKnotVectors` loops forever on knots that compare unordered (NaN); the port
  returns `InvalidInput`, the ffi wrapper rejects non-finite input.
- `N_CreateCoonsSrf`, `N_CreateCoonsBoundaryCrvs`, `N_CreateSkinSrfBoundaryDerivs` and
  `N_ApproxCrossBoundaryDerivs` modify their input curves (rationalised, reparametrised,
  made compatible, made 3D, mid knot inserted); difftest compares the curves after the call.

## nurbs.b_spline_curve_p2 (SmBSplineCurve part 2)

No Rust-vs-C++ disagreement: 2011 of 2011 non-deferred difftest cases bit-identical (328
fixtures: the six `pt_TestFiles` `.smc` curves through every op and read back with
`ReadFromDB`, the `my_test_reversed_line_join` line pairs, `TestSmCreateRectangle` /
`TestSmCreateRegularPolygon` style joins, arc/line joins, Bezier box special cases and edge
cases, plus 2000 generated); a 20,328-case run was also clean. 317 cases are deferred:
`curve.curve_p2` (`SmCurve::IsKindOf` in `IsDegenerate` / `HasRepeatedEndControlPoints`,
`EvaluateContinuity` in `CreateSimpleOffset`), `curve.curve_p4` (`SmCurve::IsClosed` in
`GetMaxAnalyticDomain`, `SmCurve::IsDegenerate`), `curve.curve_p5` (`SmCurve::IsLine` for
rational / non-evenly-spaced curves, `SmCurve::IsArc`) and `curve.line` (`SmLine` built by
`CreateAnalyticCurve`). Wiring its two `UNPORTED` stubs dropped `nurbs.b_spline_curve_n_lib`
from 150 to 57 deferred cases (all `curve.curve_p2` now). Upstream behaviour worth knowing:

- **`USE_ANALYTICS` is on.** `SmConfig.h` defines it, so `CreateAnalyticCurve` really builds
  `SmLine` / `SmCircle` objects (it crashes on a curve without NURB at its first
  `EvaluatePoint`). The port returns `Unported` where they would be built; `AnalyticCurve`
  is an empty enum until `curve.line` / `curve.ellipse_p1` land.
- `CalculateBoundingBox` picks its pseudo-box basis from control points `ii*lInc` counted
  from the start of the whole polygon, not from the first point of the spans covering the
  interval (`lStartPoint`); kept. Its Bezier special case calls the virtual `IsLine` on the
  whole curve and evaluates the line at the *unclamped* interval ends.
- `SetCanonical` swaps in the new NURB and dimension but keeps the old knot type and form.
- `GetControlPointsExpert` writes two values per point even for a curve of dimension 1
  (past a stride of 1); the ffi rejects that, the port returns `InvalidInput`.
- The `.smc` files in `pt_TestFiles` are `ReadFromDB` ASCII streams of version <= 32
  (no knot type / form lines); a non-zero rational flag (not only 1) reads weights, and
  the curve takes the file's dimension (`ConvertTo2D` only for a 3D file read as 2D).
- Harness note: `serde_json` (without `float_roundtrip`) can parse a fixture float one ulp
  off the correctly rounded value `read_smc` gets from the same text, so fixture intervals
  meant to equal a curve's last knot may land an ulp outside; the ffi's interval checks for
  `BBox` allow a few ulps (upstream clamps them).

## nlib.srf_adv_p1 (NL_SrfAdv.cpp part 1: extension by distance, offset bounds, unbounded evaluation, spine skinning, vector-field strips)

No Rust-vs-C++ disagreement: 2886 of 2886 default difftest cases bit-identical (386
fixtures on the three `pt_TestFiles` `.sms` surfaces, the `TestSmCreateRectangle` patch, a
closed rational cylinder, the `.smc` curves and the `TestSmCreateRegularPolygon` hexagon;
2500 generated), and a 20,386-case run (`--seeds 20000`) also clean; `--perturb` detects
every case. No `UNPORTED(nlib.srf_adv_p1)` stubs existed to wire. Upstream behaviour worth
knowing:

- **Degree-1 extensions read past `Rw` (inherited from `ST_surxsdr`).** `N_SrfExtendByDist`
  is a wrapper of `ST_surxsdr` (`nlib.srf_adv_p2`): with G1/G2 in a degree-1 direction the
  reflection-plane fallback can read outside the scratch row, where upstream "succeeds" on
  whatever memory it reads and the port returns `InvalidInput`. The generator keeps
  degree-1 extension directions to `NL_CMAX`. Past the recursion limit the stale error
  number is compared as the bare flag, as in the p2 runner.
- **`N_SrfEvalPtDerivsUnbounded`** wraps parameters of a G1-closed direction with `while`
  loops that never end when `ue - us` no longer changes the parameter (or take ~1e12 steps
  for far parameters); the port stops with `NotConverged` (after `MAX_PERIOD_SHIFTS`) and
  the FFI only passes parameters within 16 domain lengths (or beyond `1e12`, the immediate
  bare-flag exit). It copies the surface into `esur` before `N_SrfIsClosedSmooth`; if that
  test fails `esur` stays set with the caller's `gflg`, while the port leaves its
  extension `None` (the difftest stops a call sequence at the first failure).
- **`N_SrfOffsetGetMaxDeriv`** allocates `u`/`v` with `N_AllocReal1dArray(p)` (p + 1 entries,
  enough) and clears every `N_SrfEvalPtNormalDeriv` error (skips the sample).
- **`N_CreateSkinSpine`** modifies the profiles in place (compatibility, alignment, and the
  `N_CrvsAreCoincident` closedness test on profiles 0 and k), reads
  `pSpineParams[iNumProfiles - 1]` (index -1 for a single profile with end derivatives;
  the port returns `InvalidInput`), and with `bMakeCompatible == NL_NO` copies profile 0's
  control point count from every profile (the FFI requires equal shapes then). Rational
  profiles often produce weights outside `[NL_WMIN, NL_WMAX]` (`NL_WEI_ERR`, surface built
  but not returned by the port).
- **`N_CrossBoundaryDerivsVectorField`** compares knots exactly; two valid clamped curves
  with equal knot vectors have equal degree and size, so its unchecked `Qw[ii]` reads stay
  in bounds.

## nlib.srf_adv_p5 (NL_SrfAdv.cpp part 5: N_CreateSkinSrfBoundaryContinuity)

- **Shared knot array.** In the rational case with rails, the derivative curves are built by
  `N_CrvFromCPolygonAndKnotVector` over the derivative surface's own knot vector (v for
  `NL_UDIR`, u for `NL_VDIR`), so the in-place knot writes of `N_CrvsMakeCompatibleKnotTol`
  (`N_CrvReparamToInterval`, the knot averaging of `N_GetCompatibleKnotVectorToTol`) and the
  rescaling in `N_CrvFuncMultiplyCrv4d` (`N_MakeKnotsCompatible`) also change `der[i]`'s
  knots, until a degree elevation or refinement gives the curve new storage. Only the
  in-place output `der` is affected (the surface is not). The port reproduces it with
  `crv_tool::p2::make_compatible_knot_tol_traced` (contents of each curve's entry knot
  storage); difftest compares `der` after the call bit for bit.
- With derivative surfaces and `NL_UVDIR` upstream uses an uninitialised derivative curve
  (the port returns `InvalidInput`; the FFI rejects it). With `cmp == NL_YES` upstream reads
  `Dws[j]`/`Rw` assuming the cross sections and the derivative data are compatible, and copies
  rail control points into the net assuming both rails share a knot vector; the FFI enforces
  both.

## nurbs.b_spline_curve_p3 (SmBSplineCurve part 3)

No Rust-vs-C++ disagreement: 1229 of 1229 non-deferred difftest cases bit-identical (195
fixtures: the six `pt_TestFiles` `.smc` curves through every op, the `my_test_curve_interface`
`JoinWith` / `GetControlPoint` curves, `my_test_reversed_line_join` as B-spline lines,
`TestSmCreateRectangle` / `TestSmCreateRegularPolygon` style line joins and gaps, edge cases,
plus 2000 generated). 966 cases are deferred: `curve.curve_p2` (the `IsKindOf(SmLine_TYPE)` test
of `IsSegmentLine` / `IsOnPlane` / `JoinWith`, `IsDegenerate` in `HasInternalPole`,
`SmTol::IsDegenerate` in `AssertValid`, `ApproximateLength` for joins without a gap tolerance,
`EvaluateContinuity` in `SubdivideAtDiscontinuity`), `curve.curve_p4` (`SmCurve::AssertValid`),
`curve.curve_p5` (`IsLine` in `JoinWith`) and `curve.curve_p8` (`GlobalCurveIntersect` in
`TrimGap`). No `UNPORTED(nurbs.b_spline_curve_p3)` stubs existed. Upstream behaviour worth
knowing:

- `IsSegmentLine` returns an `SmBoolean` but uses `SER`, so a failed evaluation returns the
  status (e.g. 1001) as a "true" boolean; the shim reports it as a status.
- `AssertValid` test 4 (last two control points coincident) tests and reports the *start*
  pair's distance and scaled zero; kept. Its radius-of-curvature check (tests 13/14) compares
  `dMinRadius` before setting it (it is still `SM_BIG_DOUBLE`), so it never fires.
- Without `SM_DEBUG_CODE` the `SM_ASSERT_*_REPORT` macros ignore `pTestRequests`; the test
  level only matters as `SM_LEVEL_GIVEN` (skips `SmCurve::AssertValid`).
- `GetControlPoint` of a 2D curve takes the weight from the third slot; `CreateByScaleTransRot`
  then writes a `z` value into a 2D curve's `NOZ` slots through `SetControlPoint`.
- `JoinWith` hands `this` and the other curve to `CreateByJoining`, which may degree-elevate
  the other curve in place.

## nurbs.b_spline_surface_n_lib_p1 (SmBSplineSurfaceNLib part 1)

No Rust-vs-C++ disagreement: 638 of 638 non-deferred difftest cases bit-identical (65 fixtures:
`my_test_bsplinesurface_nlib` offset / sweep / swung, `my_test_surface_interface` operations on
the surf1 net, Coons patches from the surf1 boundaries, the three `pt_TestFiles` `.sms` surfaces
and the 3D `.smc` curves through every op, edge cases; plus 600 generated). 27 cases are
deferred: every `CreateExtendedSurface` (`SmSurface::IsSingularity`, `surface.surface_p5`, then
`IsClosed`, `surface.surface_p7`). The file statics `sm_CreateNonRationalCurve`, `sm_MakeKnots`,
`sm_CheckTrimBoundaries` and `sm_check_zero_surface_derivs` are diffed through verbatim copies
in the shim (the file defines class members, so it cannot be compiled inside a namespace). No
`UNPORTED(nurbs.b_spline_surface_n_lib_p1)` stubs existed. Upstream behaviour worth knowing:

- `sm_CreateNonRationalCurve` of a rational curve with a base surface crashes in the C++ shim
  (segfault) even for a line lying on a bilinear plane (`DropCurveCurve.smc` with unit weights on
  `DropCurveSurface.sms`); presumably `SmSurface::DropCurve` on a surface without face/brep.
  Root cause not verified; the drop path (Rust: `UNPORTED(intersect.surface_drop_lib_p3)`) is not
  diffed.
- `CreateCoonsPatch`, `CreateNSidedPatch` and `JoinSurface` hand the NURBs of their inputs to NLib,
  which changes them in place (compatibility, rescaled cross-boundary derivatives); arrays NLib
  reallocates come from the method's local stack, so the caller's objects dangle after the call.
  The port stores the changed NURBs back; the shim never reads the inputs afterwards.
- `Reverse` reverses in v only for `SM_SP_V` (every other value reverses in u), while
  `CreateRuledSurface` / `ExpandBoundarySpan` treat only `SM_SP_U` as u.
- `CreateExtendedSurface` halves the distance after each failed extension until it is below
  `dDist / 100`, which never happens for `dDist == 0` (or NaN): upstream loops forever; the port
  stops after 1100 halvings (`NotConverged`).
- `N_SrfPowerBasisEvalDerivs` (power-basis `EvaluateSimple`) stores `min(hv, q)` rows of v-power
  derivatives after the `p + 1` u powers in one `NL_MAXDEG + 1` array and overruns it when
  `p + 1 + min(hv, q)(q + 1) + q > 17`; the FFI rejects such input.
## curve.curve_p2 (SmCurve part 2: the curve base class)

No Rust-vs-C++ disagreement: 3012 of 3012 non-deferred difftest cases bit-identical (18
fixtures re-expressing the `prog_test` / `SM_API_test` curves — the `CreateLineSegment`
line and `CreateCircleSegment` circle of `smCurveTest.cpp`, `ApproximateLength(ivl, 20)` of
`TestSmPrimitives`, the circle seam continuity — plus 3000 generated over B-spline,
Hermite and arc curves, the static helpers and the solver box tests); `--perturb` is
SENSITIVE. 6 cases are deferred: `CreateAnalyticCurve` of B-splines reaching
`SmCurve::IsArc` (`curve.curve_p5`) or building an `SmLine` (`curve.line`).

The port introduces `curve::curve::Curve` (the `SmCurve` object: a `CurveKind` enum over
the ported curve classes plus the owner) and `CurveEvaluate` (the evaluation virtuals the
base numerics call, implemented by every kind and by `Curve`). New curve units add a
`CurveKind` variant and fill its arm in each `match` of `curve/curve/p2.rs`; base
numerics written over `CurveEvaluate` work for them unchanged.

Wiring its stubs made `curve.hermite_curve` (93 -> 0 deferred) and
`nurbs.b_spline_curve_n_lib` (57 -> 0) green and dropped `nurbs.b_spline_curve_p1` from 60
to 4 (`curve.curve_p1`), `nurbs.b_spline_curve_p2` from 317 to 150 (`curve.curve_p4`,
`curve.curve_p5`, `curve.line`) and `nurbs.b_spline_curve_p3` from 391 to 359 (no
`curve.curve_p2` cases left; the rest now reach `curve.curve_p4`/`_p5`/`_p7`/`_p8`). For
p3's `AssertValid`, `SmTol::IsDegenerate(this)` (the object dispatch `geom.tol` does not
port) is computed in place for an edge-less curve — `IsDegenerate` at
`GetXSectTol3d(zone, zone)` with the system zone tolerance — so
`BSplineCurve::assert_valid` now takes the `Kernel`. Stubs **not** wired: the `UNPORTED(curve.curve_p2)` sites
in `geom/tools.rs`, `geom/solution_array.rs` and the `geom/tol.rs` comment call `SmCurve`
methods on opaque handles (`C: ?Sized`, `O: SolutionObject`) that cannot yet be turned into
a `Curve` (in the difftests those objects are `SmLine`s / edges of B-rep files). They keep
their markers and still report `curve.curve_p2` as the blocker
(`geom.tools` 68, `geom.solution_array` 16, `geom.global_solver` 9 cases); whoever gives
solution objects and edges real curves (`topology.edge`, the object model) should replace
them with the `Curve` methods (`owner`, `evaluate`, `evaluate_point`, `natural_interval`,
`approximate_length`).

Harness: the file-static helpers and local classes of `SmCurve.cpp` (`sm_FlushCrv`,
`sm_AddToCurve`, the stack helpers, `sm_SnapSolToSpecifiedEndpoint`, `sm_UnitizeTangent`,
`sm_GetProjSignedAngle`, `sm_FindTestParams`, `SmLengthIFEO`, `SmCCGlobalSolver`,
`SmCPGlobalSolver`, `SmCurvePropertyGS`) cannot be linked and the file cannot be compiled
inside a namespace (it defines `SmCurve` members), so `harness/cpp/shim/curve_curve_p2.cpp`
holds **verbatim** copies of those line ranges (noted per block) inside a namespace; their
`LocalSolve` members are `SM_ERR` stubs. `sm_TestStep` is global and linked directly. The
solver tests hand-build trees whose `m_pData` is a dummy pointer; the ffi rejects every
branch that would read it (span turning angle, ends, pseudo box).

Upstream behaviour worth knowing:

- **`NER` in `BranchMayContainAnswers`.** With a null `m_pData` the three solvers
  `return SM_ERR_NULL_POINTER` (1006) from a `SmBoolean` function, which every caller reads
  as `TRUE`; the port answers `true`. `SmCPGlobalSolver` checks it *before* the aux-type
  test, `SmCCGlobalSolver` after.
- `SmCurvePropertyGS::BranchMayContainAnswers` evaluates its "corner" points with
  `dZV = 1.0 * j` (not `k`), so only 4 distinct box points are tried, each twice; and the
  `SM_CP_PERPENDICULAR_TO_VECTOR` / `SM_CP_MAXIMA_TO_PLANE` test adds a turning angle in
  degrees to an angle in radians before comparing with `PI/2`. Both kept.
- `SmCurvePropertyGS` declares its own `m_cpOptVectors`, shadowing the base member: the
  property vectors never reach `SmGlobalSolver`'s checks.
- `EuclidianStepOff` returns `SM_ERR` *with* valid outputs (clamped / best parameter) when
  it leaves the interval more than 3 times or does not converge in 30 steps; the port
  returns `Ok` with `StepOff::converged == false`.
- `sm_FlushCrv` returns an exact quadratic arc (when the cubic Bezier polygon lies on a
  circle and the arc matches the span ends within `tol`) *without* clearing the polygon and
  knot arrays; the cubic path clears them.
- `ApproximateLength` and `GetEnds` ignore evaluation statuses (stale or `SM_UNDEF_DOUBLE`
  values); `CreateBySplitCurve` ignores both `Trim` statuses. `SmHermiteCurve` has no
  `Copy`, so splitting one fails with `SM_ERR`; it also inherits the base `GetDegree` (3),
  `GetNumberControlPoints` (0), `GetNumberNaturalKnots` (2) and `IsRational` (`FALSE`).

## curve.crv_in_volume (`SmCrvInVolume.cpp`)

No Rust-vs-C++ disagreement: 1426 of 1426 non-deferred difftest cases bit-identical (30
fixture cases built around the `SmSrfInVolume.cpp:1219` usage `SmCrvInVolume(*pBSC,
bInParamSpace, vol, 0, 1)` and the `smCurveTest.cpp` line, plus 2000 generated);
`--perturb` is SENSITIVE. 604 cases are deferred: everything that reaches the volume
(`SmVolume`, `surface.volume_p1` 603, `surface.volume_p2` 1) — evaluation, uncached
breaks (`GetKnots` starts with `GetNaturalParamDomain`), continuities at interior breaks,
equality of equal curves, non-identity transforms, the volume part of `ReadFromDB`.

The port adds `CurveKind::CrvInVolume` and a stand-in `curve::crv_in_volume::Volume`
(type code only; every volume call is `Unported`). **Whoever ports `surface.volume_p1`
should replace that struct with the real volume type** and wire the `UNPORTED(surface.volume_p1)`
/ `(surface.volume_p2)` stubs in `curve/crv_in_volume.rs` (volume calls and the
`smvol_Lift*Derivative` lifts); the `SmCurveClassification` stub there points at
`intersect.curve_class_p4` / `_p8` / `_p1`. `Curve::approx_eq` now returns `Result<bool>`
(the volume comparison can be unported). The C++ side uses an identity `SmTransform` as
the volume and loads the protected break cache through a derived test class.

Upstream behaviour worth knowing:

- `GenerateMults` indexes `m_sConts` by the *knot* index `ii`, not the matched break index
  `bi`; with an interval, `GetKnots`' cached path therefore reports multiplicities of the
  wrong breaks. Its state check returns before sizing the output, so `GetKnots` (which
  ignores the status) then returns no multiplicities; `GetNumberNaturalKnots` then reads
  past the empty array (UB; the ffi rejects that shape).
- `GetKnots`' cached path and computed path give different multiplicities for the same
  break (the computed path keeps the curve's end multiplicity, e.g. 4; the cached path
  derives 3 from `SM_CT_DISCONTINUOUS`). The `&self` dispatch through `Curve` does not
  store the cache (no `const_cast`), so it always answers like upstream's first call.
- `EvaluatePoint` maps the point with `MapPoint` unconditionally before choosing
  `EvaluatePoint` / `MapPoint`; `EvaluateVolume` (not this unit) swaps the two branches
  relative to `Evaluate`.
- The copy constructor copies the volume when it owns the curve *or* the volume; with owner
  flag 1 the copied volume is not owned and leaks. Copying an owned `SmHermiteCurve` base
  fails silently, leaving both objects owning one curve (double delete).
- `CalculateContinuities` with no breaks reads `SetAt(0)` of an empty array and loops on
  `size - 1` underflow (UB); `ReadFromDB` of `SmVolume_TYPE` dereferences a null volume.

## curve.curve_cache

Partial (deferred: `GetAnalyticCurve` of lines and arcs needs `SmLine` / `SmCircle` /
`SmCurve::IsArc`), no disagreement. Notes for later units:

- **Co-ports.** Every tessellation reaches `SmFindCurveDistanceExtremaEFO::Evaluate`
  (unit `curve.curve_p4`) and, through `SmBSplineCurve::CalculateBoundingBox` /
  `IsLine`, `SmCurve::IsLine` (unit `curve.curve_p5`). Both are ported here:
  `impl EvalFunction for FindCurveDistanceExtrema` in `curve::curve_cache`, and
  `curve::curve::p5::is_line_base` (B-spline only; the `Curve` dispatch is left to
  `curve.curve_p5`). The `UNPORTED(curve.curve_p5): SmCurve::IsLine` stub of
  `nurbs::b_spline_curve::p2` now calls it; `nurbs.b_spline_curve_p2` (56 cases) and
  `_p3` (188 cases, some now reaching `curve.curve_p7`) un-deferred and pass. Porters of
  `curve.curve_p4` / `_p5`: do not port these twice.
- **Tree data.** `geom::tree::NodeData` gained `Curve(CurveSpanRef)`; a curve tree owns
  its cache's span/parent arrays (`Tree::set_curve_data`) and the end points `GetEnds`
  evaluates (stored at build time, since the tree cannot reach the curve). The curve
  solvers of `curve::curve::p2` read them through `Tree::curve_aux` / `curve_span`.
- **Upstream quirks kept.** `sm_ComputeConstants` stops at the first non-zero polygon
  angle when the angle tolerance is `0` (`dTotalAngle > 0`), so with only a chord
  tolerance the chord height is often not computed; after a successful local solve it
  measures the distance at the *step* parameter, not the solution. `BuildTree`'s parent
  test omits the off-axis and approximation tests the leaf tests use. Tessellation with
  a tiny chord tolerance and `dMinimumParametricRatio = 0` subdivides down to
  `SM_EFF_ZERO_SQRT` intervals (up to ~10^6 spans); the ffi wrapper requires a ratio of
  at least `1e-4`.

## curve.tangent_field

- **Base class not ported.** `SmTangentField` derives from `SmCrvOnSurf`
  (`curve.crv_on_surf`). The port keeps the UV curve and surface as borrowed references
  (upstream's owner flag `0`) and co-ports `SetAll` (given UV domain only),
  `GetNaturalInterval`, `IsKindOf` and `operator==` (`// CO-PORT(curve.crv_on_surf)`).
  Deferred: every constructor without a UV domain (`SmSurface::GetNaturalUVDomain`,
  `surface.surface_p2`) - which includes every successful `Create`, since upstream always
  passes `NULL`; every evaluation inside the end bands (`SmSurface::EvaluateNormal`,
  `surface.surface_p7`); equality of fields on different surface objects
  (`SmSurface::operator==`, `surface.surface_p1`); and every `ReadFromDB` that reads the
  members (`SmCrvOnSurf::ReadFromDB`).
- `EvaluatePoint` returns the stored start (end) tangent for `t < min + 1e-8 + EFF_ZERO`
  (`t > max - 1e-8 - EFF_ZERO`), so `Evaluate(n = 1)` there is a zero derivative, and
  `Evaluate` with `n >= 2` is `SM_ERR` before any evaluation.
- `ReadFromDB` ignores the status of the `SmCrvOnSurf` base read and returns `SM_SUCCESS`;
  binary short reads of the members are undefined upstream (see `database_io` above), so
  difftest never truncates inside them.

## curve.conic

Green, no disagreement. Upstream behaviour worth knowing:

- **`SmConic::Evaluate` zero-tangent fix reads an unwritten slot.** With
  `bNonZeroTangents` and two derivatives, the fix of a tiny first derivative reads
  `aPointAndDerivatives[2]` *before* the second derivative is written there (it is written
  afterwards), i.e. whatever the caller's array held. For a fresh `SmVector3d` array
  (`SM_UNDEF_DOUBLE` members, length ~2e-61) the fix never fires; the port reads
  `Vec3::UNDEF` there, so it matches every caller that passes a fresh array. The
  `lNumDerivatives < 2` recursion branch is unreachable (the function returns after one
  derivative before the fix), and `lNumDerivatives > 2` fills three vectors and returns
  `SM_ERR`.
- `SmConic::IntersectImplicitConic` (and so `IntersectConic`) of an `SM_CT_UNKNOWN` conic
  solves a quartic of uninitialised coefficients (UB); the port returns `InvalidInput`,
  the shim rejects such inputs. `EvaluatePoint` of an unknown conic leaves `x`, `y` of the
  caller's point untouched (port: a fresh point, `SM_UNDEF_DOUBLE`).
- `IntersectConic` ignores both intervals. The upstream test asserts on solution counts
  in `smCurveTest.cpp` (lines 6053-6499) all hold in both implementations.
- `SmConic::ReadFromDB`: an ASCII type code whose extraction is skipped leaves
  `lConicType` uninitialised (port: `InvalidInput`); type codes outside `0..=3` behave as
  `SM_CT_UNKNOWN`.

## Nested reads (all polymorphic readers)

Upstream `SmCurve::ReadFromDB` dispatches to `SmProjectedCurve::ReadFromDB` (and offset,
iso, ... curves), which reads its base curve through `SmCurve::ReadFromDB` again, with no
depth limit: a crafted file of a few bytes per level overflows the stack. The Rust
dispatcher bounds nesting with `DatabaseReader::enter_nested` (limit
`MAX_READ_NESTING` = 64); deeper input is `InvalidInput`. Deviation from upstream only
for inputs that would crash it. Found by the commit security review of
`curve.projected_curve`.

## curve.hyperbola

- `SmHyperbola::MakeNurb` fails (`SM_ERR`, no NURB) for a degenerate analytic domain
  (parallel end tangents in `smgu_LineLineClosestPoint`) or when the middle tangent
  line cuts the chord outside `(SM_EFF_ZERO, 1 - SM_EFF_ZERO)`; the constructor ignores
  the status. `GlobalPointSolveSTEP` on such a curve reaches `SmCurve::GlobalPointSolve`,
  whose curve cache dereferences the missing NURB (`SmBSplineCurve::EvaluatePoint`):
  upstream segfaults. The difftest shim refuses those inputs and the generator avoids
  them; the Rust side returns `Unported` there anyway until `curve.curve_p1` lands,
  whose porter must decide what a NURB-less curve does (`SM_ERR` is the natural port).
- Upstream frees the NURB in `ReverseParameterization` and rebuilds it lazily through the
  virtual `MakeNurb` in `GetOrCreateGwNurbPointer`. The Rust B-spline base cannot reach
  the override, so `Hyperbola::reverse_parameterization` rebuilds it at once (same NURB);
  the difftest shim calls `GetOrCreateGwNurbPointer` before reading the state.
- `MakeNurb` of a generator curve of `SmSurfOfExtrusion` / `SmSurfOfRevolution` (owner
  type) returns `Unported` (`surface.surf_of_extrusion_p1`, `surface.surf_of_revolution_p1`);
  the hyperbola methods that make the NURB take the owner as an argument.
## curve.projected_curve (`SmProjectedCurve`)

Difftest: 2727 cases (227 fixtures re-expressing the two upstream constructions in
`SmTransSweepGeometry.cpp:794` / `SmRotationalSweepGeometry.cpp:586`, perspective views,
`MakeOrthoNormal` edge cases and `WriteToDB`-format streams, plus 2500 generated), all
bit-identical. The shim reads the private members through the explicit-instantiation access
rule (no upstream change). Upstream behaviour kept as is:

- **`MakeOrthoNormal` is inverted.** It unitizes `m_sProjVec` only when it `IsZero()`
  (giving `(0, 0, 1)`) and returns `SM_ERR` for every non-zero vector, which is left
  un-normalised; for `SM_PT_ROTATION` the same holds for `m_sAuxData`, and the
  `NormalInPlane` result is discarded. Callers (the sweep geometry above) pass vectors that
  are already unit, so the projections still work; a non-unit view normal gives a skewed
  "projection" (both sides agree). The constructor ignores the status; `SetCanonical`
  returns it.
- **`Transform` needs `m_bOwnsCurve == UNSURE`.** It tests `m_bOwnsCurve < 2`, so a curve
  that owns its base (`TRUE`, e.g. every curve from `ReadFromDB`) or borrows it (`FALSE`)
  fails with `SM_ERR` (after a pre-edit notify); only the member-initialiser value
  `UNSURE` passes. `EditParameterization`, `ReverseParameterization` and `Trim` refuse only
  `FALSE`.
- **`Copy` of a curve owning a Hermite base** (`SmCurve::Copy` = `SM_ERR`, ignored by the
  copy constructor) returns `SM_SUCCESS` and a projected curve with a null base curve that
  crashes on use. The port returns the `SM_ERR`; the shim maps upstream's null-base result
  to `SM_ERR`.
- `EvaluatePoint` computes the parallel projection before the switch and again inside it;
  `Evaluate` projects perspective derivatives about the already projected position.
- `ReadFromDB` casts the stored ULONG to `SmProjectionType` (undefined beyond `0..=3`; the
  port maps it to `Unknown`) and does not call `MakeOrthoNormal`.

## curve.iso_curve

- `SmIsoCurve::CreateUVIsoLine` tests `m_eSurfParam == SM_SP_U` in both branches, so a
  constant-v iso curve (and every other `SmSurfParamType`) gets `SM_ERR_INVALID_INPUT`;
  the constant-u branch builds the UV line over the domain's *u* interval although the
  line runs along v. The port keeps both (difftest `*_uvline` cases).
- The empty I/O constructor `SmIsoCurve()` leaves `m_cpSurface` uninitialised; only
  `ReadFromDB` uses it, and the port always builds a new curve there.
- The base surface is held by value as a `BSplineSurface` (the only ported `SmSurface`
  class); `operator==`'s pointer-identity shortcut becomes a value comparison. When the
  `SmSurface` enum lands (units `surface.surface_*`), `IsoCurve::surface` should become
  that type.

## curve.offset_curve

Green, no disagreement (2808 cases bit-identical). Upstream behaviour worth knowing:
- `SmOffsetCurve::Evaluate` with a zero offset distance evaluates the base curve with
  `SER(...)` but does not return: the result is overwritten by the general path (same
  values with `D = 0`); only the status of that first base evaluation matters. The port
  keeps the extra call.
- With `bNonZeroTangents` and `lNumDerivatives == 1`, the recursive `Evaluate(dParameter,
  2, ...)` passes the already mirrored (and, for a zero base tangent, shifted) parameter,
  so a reversed curve is mirrored twice there; its status is ignored. Kept.
- `Trim` checks the *current* curve interval against the base curve domain, not the new
  trim interval, and never snaps it. Kept.
- `Copy` (copy constructor) of an offset that owns a base without `Copy` (a Hermite
  curve: `SmCurve::Copy` = `SM_ERR`, ignored) shares the base pointer and both curves
  delete it (double free). The port owns its base by value (a clone); the ffi wrapper
  rejects such cases.
- A zero-distance offset of a rational B-spline evaluated with more than `NL_MAXDER` (4)
  derivatives overflows a fixed NLib stack array (`sm_CrvRationalBasisDerivs`, stack
  smashing). The ffi wrapper limits `Evaluate` to 4 derivatives.
- The base curve is held by value (`Box<Curve>`), so the pointer test of `operator==`
  becomes the base curve `operator==`.
- `SmBSplineCurve::CreateSimpleOffset` now builds the `OffsetCurve`; it defers on
  `SmCurve::ApproximateCurve` (`curve.curve_p1`) instead of on this unit.


## nurbs.b_spline_surface_n_lib_p2 (`Evaluate`, `TrimWithDomain`, `CreateSkinnedSurface`, `CreateCornerBlend`)

Partial, no disagreement (654 cases, 26 deferred; 20k-seed run clean). Upstream behaviour
worth knowing:

- **`TrimWithDomain` v snapping uses `idxMin` for both boundaries.** In the v loop the upper
  boundary branch writes `idxMin` (not `idxMax`), and `idxMax` stays `-1`, so
  `sm_CheckTrimBoundaries` never widens `vr`; the lower-boundary "previous knot" branch also
  leaves `idxMin` at `i`. The u loop's upper "slightly above the knot" branch does not set
  `idxMax`. Ported as is. Only the u boundaries are checked for being too close
  (`SM_ERR_INVALID_INPUT`); the v check is commented out upstream.
- **`CreateSkinnedSurface`** treats a skinning call as failed only when NLib returns exactly
  `NL_YES`; it retries without the knot vector, then (with derivative surfaces) without
  them, on the cross sections / rails / derivative surfaces the failed calls already changed
  in place. The knot vector is only used with derivative surfaces (it is reset to `NULL`
  otherwise). It casts away the rails' constness: two rails are reparametrised by
  `SyncronizeKnotsOfCurves` (the port takes them `&mut`). The spine branch evaluates
  `pRail1` (not the degree-elevated NLib copy) through the `SmBSplineCurve.h` overload, so
  `bNonZeroTangents = FALSE`.
- **`CreateCornerBlend`**: the `NER` check of missing UV curves / surfaces fires only at the
  entry where the null count first exceeds the limit (a four-sided G1 blend tolerates one
  missing pair, used as a degenerate triangle side). A surface without UV curve, a rational
  3D curve with a surface (`SmSurface::DropCurve` crashes on a standalone surface) and
  n-sided bilinear blends with missing UV curves / surfaces dereference `NULL` upstream; the
  shim rejects them, the port returns `InvalidInput`. `SM_OT_UNKNOWN` behaves as "not
  `SM_OT_SAME`" for four curves and as "not `SM_OT_OPPOSITE`" for more. G1 blends run
  `SmTangentField::Create` (its end-point checks are compared) and are then deferred on
  `SmSurface::GetNaturalUVDomain` (`surface.surface_p2`), `SmCrvOnSurf::ApproximateCurve`
  (`curve.crv_on_surf`) or, for n-sided blends, `SmSurface::EvaluateNormal`
  (`surface.surface_p7`).
- **`Evaluate`**'s `SmSurface::GetDerivativeLimit` (`10 * SM_EFF_ZERO * (1 + max|P|)`) is
  co-ported (`CO-PORT(surface.surface_p2)`). With fewer than two derivatives in either
  direction a zero tangent is fixed by recursing with `2, 2` upper-half (from the
  unclamped parameter); otherwise from `Suv` / `Suu` / `Svv`, then by sampling up to five
  nearby points for steps `1e-5 .. 0.1`; a surface collapsed to a point returns `SM_ERR`.

## curve.line (`SmLine`)

Difftest: one program per case (a line construction, then up to ~6 ops on it, then the
line's full state: analytic data, B-spline part, natural interval), so state changes made
by one op (lazy NURBs, trims, joins, reparameterisations) are checked by the next.
Upstream behaviour worth knowing:

- **Lazy NURB.** The two-point constructor makes no NURB; `GetNaturalInterval` then
  returns the analytic domain and `Length` uses `interval * scale` instead of the end
  point distance. Inherited `const` queries (`GetEnds`, `GetNumberControlPoints` inside
  `CreateAnalyticCurve`, ...) make the NURB through `GetOrCreateGwNurbPointer` (a
  `const_cast` + virtual `SmLine::MakeNurb`), so a later `Length` changes branch. The port's
  `Line::ends` / `Line::create_analytic_curve` take `&mut self` and do the same; the
  read-only variants make it on a copy.
- **`SmBSplineCurve::DeformToEndPointTargets` ignores `DegreeElevate`'s status**
  (`SmBSplineCurveNLib.cpp:3038`). The port of `nurbs.b_spline_curve_n_lib` propagated it;
  fixed in this unit (its difftest stays green, 2282 cases). For a NURB over a zero-length
  knot interval (a degenerate line: `[a, a, a, a]`) `N_CrvElevateDegree` fails
  (`N_KnotVectorIsValid`), upstream then runs the degree-3 indexing scheme on the degree-1
  curve and reads past its arrays (undefined behaviour; it happened to "succeed" in our
  runs). The port returns `InvalidInput` ("knot index out of range"). The curve.line
  generator only deforms regular lines into a new B-spline (first op only).
- **Short database streams.** `SmLine::ReadFromDB` / `SmBSplineCurve::ReadFromDB` on a
  truncated binary stream throw (`SmDatabaseIOStream`) or read uninitialised values, and a
  short ASCII header leaves `n`, `p` uninitialised before the allocation: not difftested;
  the ffi only builds complete streams.
- `IsLine` always answers `TRUE`; `IsClosed` / `IsArc` `FALSE`; `IntersectWithLine`
  sorts a coincident-range solution by this line's parameter, re-evaluating the start with
  the second point's parameters. `DropPointFast` compares the clamped and raw NURB
  parameters with `!=` (a NaN parameter recomputes the distance).

## surface.volume_p1 (`SmVolume` base class, `smvol_Lift*Derivative`, `smvol_ConcatEvals`)

All difftest cases agree bit for bit with upstream (`max_rel_err = 0`, 2886 default cases,
a 20,886-case run clean); the deferred cases need the simple maps of the twist and unbend
volumes. Upstream behaviour worth knowing:
- **Lift typos (ported as written).** The surface and volume forms of
  `smvol_LiftThirdDerivative` use `crDxxz` where the `Dxzz` term belongs in `rDuuv`,
  `rDuvv`, `rDuuw`, `rDuww`, `rDvvw` and `rDvww` (`SmVolume.cpp:9851`, `:10126`),
  `rDuww` is built from `Fuv`/`Fv` instead of `Fuw`/`Fw`, and `rDvww` from `Fvv`/`Fv`
  instead of `Fvw`/`Fw`. Mixed third derivatives of compound volumes (and of `SmSrfInVolume`)
  are therefore wrong upstream; the port keeps the formulas so results match.
- **Unfilled evaluation slots.** `smvol_ConcatEvals` writes only the partials of total order
  `<= n`; the other slots of the `(n+1)^3` array keep the caller's values (a fresh
  `SmVector3d` array: `SM_UNDEF_DOUBLE`). `Evaluate` / `Map` with an orient or next map
  therefore return `SM_UNDEF_DOUBLE` there, while a plain simple map returns zeros. The port
  returns `Vec3::UNDEF` in those slots.
- **Copies drop maps they cannot copy.** The copy constructor copies an owned next map with
  its virtual `Copy` and ignores the status: a plain `SmVolume` (whose `Copy` fails with
  `SM_ERR`) in the chain truncates the copy there. A shared next map (owner flag 0) is kept.
  `AddNextMap(p, 1)` of a plain `SmVolume` fails and leaves no next map. Without a context
  upstream also silently drops the next map (not modelled: the port always copies).
- **Undefined behaviour.** `RefreshInvOrientMap` without an orient map calls
  `MakeMapSimple` through a null pointer; `IsCurveInParamDomain` dereferences a null copy
  when `Copy` fails (unreachable for the base class, whose curve check is `FALSE`);
  `Evaluate` / `Map` only `SM_ASSERT` the inverse orient map. The port returns
  `InvalidInput`; the ffi never sends `Refresh` without an orient map.
- **`Orient` ignores the orient map's status** (`n > GW_MAX_DERIV` leaves the caller's array
  untouched); without an orient map `n > 5` is `SM_ERR_INVALID_INPUT`.
- **Default-constructed `SmUnbendVolume`.** `IsCurveInInSpaceDomain` of an oriented default
  unbend volume crashes upstream (segfault in the unbend volume's curve check); the ffi
  rejects line and curve domain checks on chains with an unbend map.
- **`SmFindPVIntersectENFO`** is declared only in `SmVolume.cpp`; the shim redeclares it
  identically and links the library's `Evaluate` and vtable.

## curve.curve_p4 (`SmCurve`, part 4)

Difftest: 3259 cases (259 fixture + 3000 generated), all non-deferred cases bit-identical
(max_rel_err 0); 30,259-case run clean; `--perturb` SENSITIVE. Upstream behaviour worth
knowing:

- **`SmCurve::FixChainCurvesList` with `bDeleteDuplicates`** deletes a duplicate link
  (`delete pBSp1; pBSp1 = NULL;`, `SmCurve.cpp:946`, 968, 989, 1010) but
  leaves the dangling pointer in `r3DCurves[j]`: the next scan (`goto REDOLOOP`) and the
  outer chain-start loop dereference the freed curve (use after free), a later branch of
  the same scan can `delete` it again (double free: a reversed duplicate matches both the
  "end at end" and "end at start" tests), and a later branch can pick the `NULL` local as
  the next link (`pNextCrv = pBSp1`), which cases 2 / 4 dereference. Not difftestable: the
  generator drops every case whose Rust run deletes a duplicate. The port removes the
  duplicate from the list once, and a link choice that is the deleted curve is
  `InvalidInput`. Without duplicates (or with `bDeleteDuplicates = FALSE`) both sides
  agree.
- **`sm_FilterSolution`** passes `SmVector3d sPV1[2]` / `sPV2[2]` to `sm_UnitizeTangent`,
  which reads `rPVV[2]` when the (non-zero-replaced) first derivative is not longer than
  `SM_EFF_ZERO`: an out-of-bounds stack read. The port reads `Vec3::UNDEF` there; only a
  curve whose first and second derivatives both vanish reaches it.
- **`sm_GetProjDist`** (a `double` function) returns the failing `SmStatus` of
  `EvaluatePoint` through `SER` as the distance (e.g. `1001.0`). The port returns the
  error; the difftest maps it back to the status value.
- **`SmCurvePropertyEFO::Evaluate`**: `SM_CP_PROJECTED_POINT_*` read `m_cpOptVectors`
  without a `NULL` check, `SM_CP_DISTANCE_TO_POINT` / `SM_CP_CYLINDER_INTERSECTION`
  read `m_cpdPropertyValue` without one (port: `InvalidInput`; the ffi never sends
  these); `SM_CP_MAXIMIZE_RADIUS_OF_CURVATURE` with a zero curvature sets
  `rbFoundAnswer` and falls through into the division by the zero numerator (NaN `F'`,
  kept); `SM_CP_PLANE_CLASSIFY` has no case (`SM_ERR`). Values written before a failing
  `return SM_ERR` (e.g. `SM_CP_X_NORMAL`) are part of the comparison.
- `SmFindCurveDistanceExtremaEFO::Evaluate` was co-ported by `curve.curve_cache`; this
  unit only difftests it. `HasPlaneCrossings` with planes is deferred on
  `surface.surface_p8` (`SmSurface::GlobalCurveIntersect`); ellipse operands of
  `AnalyticCurveIntersect` defer on `curve.curve_p1` (`GlobalPointSolve`).

## curve.curve_p3 (`SmCurve`, part 3)

- **`SmCurve::FindParameterAtArcLength`** stepping backwards (`dArcLength < 0`) from a
  start parameter within `dParamTol` *below* the first knot leaves `ik = -1` and reads
  `sKnots[(ULONG)-1]` (out of bounds). The port returns `InvalidInput`; the ffi only
  sends backwards steps from the first knot on. `lOffEndCount` is a `ULONG`: the first
  Newton step that leaves the curve below its start wraps it to `ULONG_MAX`, so the
  iteration stops at once with `*pbResultPastEnd = TRUE` (above the end it needs three
  steps); the port keeps the unsigned wrap. A failed `SetMinMax` (start parameter
  slightly past the last knot) leaves the span interval in its `Init()` state, as in
  the port. On curves other than lines the base `SmCurve::Length` (unit
  `curve.curve_p5`) defers every case.
- **`SmCurvePropertyGS::LocalSolve`** returns `TRUE` (status `1`) from an `SmStatus`
  function when a projected pseudo box corner of the angle properties lies on the
  rotation origin; the port returns `SolidError::Upstream { code: 1 }`. Every guess
  goes through `SmCurve::LocalPropertyAnalysis` (unit `curve.curve_p5`), so only the
  early exits (angle box rejection, disjoint / zero-length intervals, a plane
  intersection span lying in the plane) are difftested now.
- **`SmCurve::CrvsFixGaps`** casts every curve to `SmBSplineCurve` and calls the
  non-virtual `SetControlPoint`: for another kind that writes through a bad cast. The
  port moves end points of `CurveKind::BSpline` curves only (`InvalidInput` otherwise);
  the ffi sends B-splines only. Its `FixChainCurvesList` call deletes duplicates (see
  `curve.curve_p4` above): such cases are not generated.
- **`SmCurve::LocalCurveSolve` / `LocalCurveIntersect`** reach `SmFindCCNormalENFO::
  Evaluate` / `SmCCIntersectENFO::Evaluate` (unit `curve.curve_p5`) on every path, so
  all their cases are deferred; the C++ side runs them (no crash on the generated
  inputs). `SmBestLineFitENFO` (marked "UNDER CONSTRUCTION" upstream, no caller) is
  ported and difftested on its own.
## curve.curve_p1 (`SmCurve`, part 1)

Difftest: 3083 cases (83 fixture: smCurveTest.cpp `my_test_point_extrema` / `my_test_drop_point`
on the unit line segment and the radius-1 circle, the circle centre (every point a normal
foot), SM_API_test rectangle and hexagon edges through `Tessellate`, `TessellateByBisection`,
`GlobalPointSolve` and `ApproximateCurve`, degenerate / closed / short-span edge cases + 3000
generated), all non-deferred cases bit-identical (max_rel_err 0); a 12,083-case run was also
clean; `--perturb` SENSITIVE. 590 deferred on `curve.curve_p5`: `LocalPropertyAnalysis` (the
ray, directed and projected `GlobalPointSolve` operations, 310), `SmFindCCNormalENFO::Evaluate`
(`ClassifyIntersectingRangeSolution` past its point-solution exit, 163),
`SmCCIntersectENFO::Evaluate` (`sm_RefineCCSolutions` on range solutions, 73) and `IsArc`
(minimise / intersect on curved B-splines reach the cache's analytic conversion, 44).

Wiring its stubs made `nurbs.b_spline_curve_p1` green (4 -> 0: `TessellateByBisection`) and
removed every `curve.curve_p1` deferral of `curve.hyperbola` (70 -> 0, all pass),
`curve.curve_p4` (53 -> 0: 28 pass, 25 now wait on `curve.ellipse_p1`) and
`curve.ellipse_p2` (233 -> 0: 68 pass, 164 now wait on `curve.ellipse_p1`, 1 on
`curve.curve_p5`). Notes for later units:

- **Solution objects must report their curve's interval.** `SmGlobalSolver::IdenticalSolutions`
  treats parameters within `1e-4` of the object's parameter interval as equal
  (`SmSolution::GetParameterInterval` reads the object, a curve). With opaque handles the Rust
  port had no interval, kept near-duplicates apart and disagreed. `SolutionObject` gained a
  default method `natural_interval()` (default `None`, which keeps the old
  `UNPORTED(curve.curve_p2)` path); `GetParameterInterval` of a curve object uses it. Handles
  passed to `Curve::global_point_solve` (and any solver that sorts curve solutions) should
  answer it. `GetObjectTolerance` (Rayfire) still needs `SmTol::GetZoneTol3d(object)`
  (`topology.brep_p1`); ray solves defer on `curve.curve_p5` before reaching it today.
- **`IdenticalSolutions` prefers the smaller value even when maximising.** A
  `SM_SO_MAXIMIZE` solve can replace an interior maximum by a span-boundary point `5e-5`
  away with a smaller distance (seen on a closed Hermite curve: `0.42572...` replaced by
  `0.42578125`). Kept.
- **`SmBSplineCurve::CreateSimpleOffset`** calls `sOff.ApproximateCurve(ctx, breaks, tol, gap,
  approx, bOptMatchParameterization, FALSE)`: the caller's match flag lands in the
  `bOptCreateAnalytics` slot and matching is always off. Kept. `ApproximateCurve` of a
  degenerate curve does not write `rdAchievedTol`, so `CreateSimpleOffset` would copy an
  uninitialised `dMaxGap3d`; the port returns `InvalidInput` there.
- **Co-ports from `curve.curve_p5`** (in `curve/curve/p5.rs`): the `Curve` dispatch of `IsLine`
  (with the base body over `Curve`) and the new-cache branch of `CacheMakeOrValidate`
  (`make_curve_cache`). Its porter should route `a_object::get_or_create_object_cache`'s
  `CacheMaker::Curve` through `make_curve_cache`, not port it again.
- `GlobalPointSolve` on an `SmEllipse` needs the ellipse as a `CurveKind` (its `DropPointFast`
  override and cache); the ellipse stub now reads `UNPORTED(curve.ellipse_p1)`. The line's
  `GlobalPointSolve(SM_SO_INTERSECT)` in `SmEllipse::IntersectWithLine` is the line's
  `DropPointFast` and is wired.
- `SmCPGlobalSolver::LocalSolve` reuses one `SmSolution` across guesses; `LocalPointSolve`
  writes only type, value and first parameter on success, so stale fields of earlier guesses
  feed the duplicate test. Ported as written. `sm_GetProjDist` returns a failing status as the
  distance (see `curve.curve_p4`), kept.
- Harness: `sm_TestSpanAccuracy` and `sm_RefineCCSolutions` are file-static, so
  `harness/cpp/shim/curve_curve_p1.cpp` holds verbatim copies (line ranges noted);
  `SmCPGlobalSolver::LocalSolve` is exercised through the public `GlobalPointSolve`.

## surface.twist_volume (`SmTwistVolume`)

All difftest cases agree bit for bit with upstream (`max_rel_err = 0`, 1572 default cases,
a 20,072-case run clean); the 3 deferred cases are `ReadFromDB`, whose base-class read
`SmVolume::ReadFromDB` belongs to `surface.volume_p2`. No upstream test builds a twist
volume. Upstream behaviour worth knowing (all ported as written):
- **Constructor repair.** The component constructor compares `Unitize`'s
  `SM_ERR_INVALID_INPUT` with `SM_ERR`, so with a zero axis or mid direction only its last
  branch runs (a frame about the repaired axis `(0, 0, 1)`). A too-small period becomes
  `|dPeriodMinDist|` of the argument, not the clamped minimum (so it can stay below
  `SM_ZONE_TOL_3D`). The component constructor is in no unit's function list; it is ported
  as `TwistVolume::create` because nothing else builds a usable twist.
- **Default construction.** `SmTwistVolume()` has period `0` and minimum `SM_UNDEF_DOUBLE`
  (member initializer); every map then divides by zero (NaN / inf on both sides).
- **`operator==`** overwrites the period comparison with the minimum comparison: twists
  differing only in period compare equal.
- **`EvaluateSimple`'s `Duw`** is `(-per sin a, -per cos a, 0)`; the header documents
  `(-per sin a, per cos a, 0)` and `Evaluate` uses that. Mixed `uw` derivatives of a simple
  twist map are therefore wrong upstream.
- **`EvaluateBoundingBoxSimple`** writes `*pProjBox` unconditionally in the unbounded
  branch (null dereference when only the pseudo box is requested: the port returns
  `InvalidInput`, the ffi rejects it). Its `(long)(theta / 2 PI)` of a NaN quotient is UB;
  the port emulates x86-64 (`LONG_MIN`).


## surface.b_spline_volume (SmBSplineVolume)

- **`USE_VOLUMES` is not defined in the SMLib build.** `SmBSplineVolume.cpp` compiles only
  the `#else` branches of `AreEqual_gw_VOLUME` (`FALSE`), `sm_LoadNLibTransform`,
  `EvaluateSimple`, `EvaluateIsoParametricCurveSimple`, `EvaluateIsoParametricSurfaceSimple`,
  `CreateCanonical`, `WriteToDB`, `ReadFromDB` and `Dump` (all `SM_ERR` / no-ops). So a
  B-spline volume cannot be evaluated (`Evaluate`, `Map`, `MapPoint` return `SM_ERR`), two
  distinct B-spline volumes never compare equal (a volume equals only itself, not its own
  copy), and `TransformSimple` builds its transform then returns `SM_ERR` with the control
  points untouched (`Transform` of a volume with next maps still succeeds: it goes to the
  last map). The port follows the build (difftest: identical statuses).
- **Copies re-run the knot fixup.** Every constructor (including the copy constructor) runs
  `sm_FixupKnotVector(.., SM_EFF_ZERO_PARAM)` on all three knot vectors, so knots closer
  than `1.2e-8` are spread / merged on construction and again on every `Copy`.
- **`InvEvaluateGuessPointSimple`** indexes knots by the closest control point's index
  without a check (only `SM_ASSERT`); a record whose knot vectors are shorter than control
  points plus degree reads out of bounds. The port returns `InvalidInput`; the ffi only
  builds records with `cp + degree + 1` knots.


## curve.curve_p6 (SmCurve part 6: boxes, planarity, drops, gaps, coincidence, deform)

No Rust/C++ disagreements (3136 cases, 3048 bit-identical, 88 deferred on `curve.curve_p5`;
30k-seed run clean; `--perturb` SENSITIVE). Notes for later porters:

- **`CurveMaxDistanceBetween` with `lNumSamples == 1`** divides `0.0 / 0.0` (`ii /
  (lNumSamples - 1)`) and evaluates both curves at NaN parameters. The port computes the
  same NaN; the ffi rejects one sample.
- **`CurveMaxDistanceBetween` early returns** (the gap limit `pdOptMaxDistanceNeeded` hit,
  the same curve over the same interval) leave `pbOptIncorrectDir` unwritten; the port
  returns `incorrect_dir: None` there.
- **`DeformToEndPointTargets`** ignores the status of `ApproximateCurve` and then calls the
  B-spline deform through `*ppOptNewCurve` (a null dereference when the approximation
  fails); the port returns the error. The `SmCrvOnSurf` branch is taken only with the
  `pOptSurfCrvOnSurf` argument the port drops (see `curve.crv_on_surf` below).
- **`GlobalCoincidenceChecker`** recurses on closed and half closed curves without a
  depth bound; the port stops at `MAX_COINCIDENCE_DEPTH` (200) nested splits with
  `InvalidInput` (AGENTS.md recursion rule). Its recursive calls use the default
  `bSnapToEnds = TRUE`, whatever the caller passed; the port keeps that.
- **`IsPlanar`** ignores the status of `CalculateBoundingBox`; a failed box leaves the
  default-constructed pseudo box, which the port reproduces.
- **Stubs left.** `UNPORTED(curve.curve_p6)` in `geom/tools.rs` (`DropPoint`,
  `CurveMaxDistanceBetween` on the opaque curve handle `C` and on edge curves) stay, as
  the `curve.curve_p2` stubs there did: the handle cannot become a `Curve` until edges
  carry real curves (`topology.edge`). They are behind `topology.*` stubs, so no
  `geom.tools` case is deferred on `curve.curve_p6`. The ellipse's
  `GlobalCoincidenceChecker` stub (`curve.ellipse_p2`) is retagged
  `UNPORTED(curve.ellipse_p1)`: the checker is ported but needs `SmEllipse` as a curve
  kind (its `IsDegenerate` / `GetDegree` overrides), as `curve.curve_p1` did for the
  ellipse's `GlobalPointSolve` (`curve.ellipse_p2`: 1 case moved from `curve.curve_p6` to
  `curve.ellipse_p1`).
- **Wired at merge.** `curve.curve_p1` landed while this unit was in flight; its
  `GlobalPointSolve`, `Tessellate` and `ApproximateCurve` are called directly (288
  deferred cases became passes).

## curve.curve_p7 (`SmCCIGlobalSolver::LocalSolve`, `SmCurve::GlobalPropertyAnalysis`)

- **Pruned curve trees.** `LocalSolve` casts every leaf's `m_pData` to `SmBezierSpan`
  (the `m_eAuxDataType == SM_AD_BEZIER_CURVE` check is an `SM_ASSERT`). A cache built with
  aux data but without a full tree (`bComputeAuxTreeData && !bMakeFullTree`) turns parents
  into leaves holding `SmBezierAux1d` data, so the cast reads past the object (the pseudo
  box is garbage). The curves' object caches always build full trees, so upstream never
  reaches this; the port returns `InvalidInput` and the ffi rejects such caches.
- **Degenerate curves.** When either (projected) curve is degenerate, upstream re-adds the
  last end point solution built (`sSolution2`) once per accumulated end point solution
  instead of each accumulated one; kept.
- **Ignored statuses.** The span end `EvaluatePoint`s, the closed-curve
  `EvaluateContinuity`, `SmProjectedCurve::SetCanonical`, the in-place
  `SmPseudoBox::ProjectToPlane`s and the end point `LocalPointSolve`s ignore their status;
  the port does too (a failed point solve is no answer).
- **`GlobalPropertyAnalysis` comb.** `EvaluateGeometric`'s status is ignored, so for a curve
  whose second derivative vanishes (degree 1) the comb uses what
  `smgu_FrenetFromDerivatives` wrote before failing (position, tangent, zero curvature);
  the port reproduces those partial outputs (`evaluate_geometric_partial`).
- **`SmBSplineCurve::AssertValid`** ignores `GlobalPropertyAnalysis`'s status; on a failed
  solve upstream keeps the solutions found before the failure, the port none (the solve
  only fails for errors upstream asserts on).
- **Deferred.** Every leaf pair that reaches a Newton intersection
  (`LocalCurveIntersect` -> `SmCCIntersectENFO::Evaluate`) and every property solve that
  reaches `SmCurve::LocalPropertyAnalysis` is deferred on `curve.curve_p5`; the co-ported
  `sm_FindAdditionalCcTangentGuess` is only reached after such an intersection.

## curve.parabola (SmParabola)

Upstream behaviour worth knowing (all kept by the port unless noted):

- **Lazy NURB.** `ReverseParameterization` frees the NURB; the next
  `GetOrCreateGwNurbPointer` rebuilds it with `MakeNurb`. The port rebuilds it at once (same
  data, same NURB). A `Transform` in between differs: upstream skips the control points
  (`if (m_pNurb)`) and later rebuilds from the transformed placement, the port transforms
  the rebuilt control points; the results agree to rounding only. The difftest shim calls
  `GetOrCreateGwNurbPointer` after the reversal.
- **`ReadFromDB` ignores the `SmBSplineCurve::ReadFromDB` status** (and does not clip the
  domain to `±SM_INFINITE_PARAMETER_SQRT`, unlike the constructor and
  `AdjustSTEPInterval`). A short or missing base stream makes upstream read undefined
  values (huge `n`, `bad_alloc`); the runner only feeds complete base streams.
- `GlobalPointSolveSTEP` first runs `AdjustSTEPInterval` (it changes the curve), projects
  the point onto the plane and discards the projection. When the interval equals the
  current one and the curve has no NURB (its `MakeNurb` failed: zero focal distance,
  degenerate interval), `GlobalPointSolve` would run on a NULL NURB; the runner avoids
  that input. At merge `SmCurve::GlobalPointSolve` (`curve.curve_p1`) was ported and is
  called directly (the 206 formerly deferred `gps_step` cases now pass bit-identical).
- `MakeNurb` fails (`SM_ERR` from `smgu_LineLineClosestPoint`) when the end tangents are
  parallel or zero: a degenerate interval or a zero focal distance. The constructor ignores
  that status (the curve has no NURB); `AdjustSTEPInterval` returns it.
- `STEPInversion` reads the parameter off the `Y` coordinate only (`dY / 2 / F`); with
  classification the point must also be within `SM_EFF_ZERO * (1 + size)` of the curve.
- `Trim`, `EditParameterization` and `ConvertTo2D` are the `SmBSplineCurve` ones: they edit
  the NURB and leave the analytic domain stale.
## curve.crv_on_surf (SmCrvOnSurf)

- **Partial, not blocked.** Every non-deferred difftest case is bit-identical
  (max_rel_err 0, `--perturb` SENSITIVE). The deferred cases reach: the
  `SmCurveClassification` of uncached breaks (`intersect.curve_class_p4/p8/p1`, behind
  `GetKnots`, `GetNumberNaturalKnots`, `CalculateContinuities` and the approximation
  without given breaks), `SmSurface::EvaluateNormal` (`surface.surface_p7`: the surface
  normal and every sample of `SurfaceCurveMaxDistanceBetween`),
  `SmSurface::Evaluate2ndDerivatives` (`surface.surface_p1`: `EvaluateSurface1stDerivDerivs`)
  and `SmSurface::ReadFromDB` (`surface.surface_p2`: every complete `ReadFromDB`
  record), plus a few `curve.curve_p5` solver callees.
- **`GetKnots` with `pOptIvl`** on an uncached curve reads `rKnots[0]` after asking the UV
  curve for its knots in the interval: an interval holding no UV knot reads an empty
  array (a crash in the harness). It also reads `GetAt(lCnt - 1)` when the classification
  has no interval. The port returns `InvalidInput`; the ffi only passes intervals holding
  an end of the unedited UV curve. The cached path ignores `pOptIvl` and returns every
  break (multiplicities `4, 2, ..., 2, 4`); the uncached path caches the interval's
  breaks only.
- **`SurfaceCurveMaxDistanceBetween`** sets `bDoPrecise = FALSE` unconditionally before its
  precise refinement, so the `SmSurface::LocalCurveSolve` / `LocalCurveSolve` code after
  it is dead (not ported). With `lNumSamples == 1` it divides `0.0 / 0.0` (the ffi rejects
  one sample). `GetDegree` is not overridden (3), so the line/line special case never
  applies.
- **`EvaluateSurface1stDerivDerivs`'** zero fill (`for (ii = 2; ii > lNumDers; ii++)` inside
  `if (lNumDers > 1)`) never runs; only the first two derivatives are written.
- **Caches through `const_cast`.** `GetKnots`, `GetOrCreateBSApproxPointer` and the const
  methods calling them update the caches; `Trim` drops the breaks but keeps the B-spline
  approximation (a later `GetOrCreateBSApproxPointer` without breaks reuses the stale
  approximation when its gap is small enough), `EditParameterization` rescales the
  breaks only, `ReverseParameterization` touches neither. The port keeps all of this;
  the `Curve` dispatch (`&self`) runs these methods on a copy.
- **Binary records cut short.** A binary `ReadFromDB` record truncated inside the UV
  curve's B-spline data gives `SM_ERR` upstream (after reading garbage counts) and
  `InvalidInput` here (counts bounded by the bytes left, as "header-declared sizes"
  above); the generator damages records by replacing the UV curve type instead.
- **Stubs left.** Three `UNPORTED(curve.crv_on_surf)` sites cannot be wired yet:
  `geom/global_solver.rs` (`SmCrvOnSurf::EvaluatePoint` on a tree node's owner object,
  which is only an `ObjectRef` handle here; the solver would have to carry the curve, as
  `CpGlobalSolver::curve` does), `nurbs/b_spline_surface_n_lib/p2.rs`
  (`SmTangentField::ApproximateCurve`, inherited from `SmCrvOnSurf`: the tangent field is
  not a `Curve` kind and `Curve::approximate_curve_base` is not generic; its evaluation
  also needs `surface.surface_p7`) and the `SmTangentField` arm of `SmCurve::ReadFromDB`
  in `curve/curve/p2.rs` (no tangent-field curve kind; unreachable for now: the
  `SmCrvOnSurf` part of the record defers on `SmSurface::ReadFromDB` first). The
  `DeformToEndPointTargets` branch for curves on surfaces is retagged
  `UNPORTED(surface.surface_p5)`: it is taken only with the `pOptSurfCrvOnSurf` argument
  the port drops, and needs `SmSurface::DropPoint`.
- **Wired.** `SmCurve::ReadFromDB(SmCrvOnSurf_TYPE)` (curve.curve_p2) and the
  `SmCrvOnSurf` part of `SmTangentField::ReadFromDB` (curve.tangent_field: its 2
  `curve.crv_on_surf` deferrals now pass; that read's status is ignored upstream).
  `ApproximateCurve`, `DropPoint` and `CalculateBoundingBox` now dispatch to the
  `SmCrvOnSurf` overrides (`Curve::approximate_curve_base` / `Curve::drop_point_base` keep
  the base bodies). `smsurf_LiftSecondDerivative` / `smsurf_LiftThirdDerivative`
  (`surface.surface_p2`) are co-ported in `curve/crv_on_surf.rs`.

## curve.ellipse_p1 (`SmEllipse` / `SmCircle` / `SmPolarConversion`, part 1)

- **`ConvertTo2D` uses `dThetaXRad` for both new axes** when the projected axes are no
  longer perpendicular (`sNewY = R1 cos(ThetaX) X + R2 sin(ThetaX) Y`, a copy of the
  `sNewX` line): the projected ellipse gets equal axes and `SetCanonical` of two parallel
  vectors (its status ignored). Ported as written; `dThetaYRad` is computed and unused.
- **`SmCircle::CreateSimpleOffset`** tests `IsParallelTo(+Z)` first, and `IsParallelTo`
  accepts anti-parallel vectors, so the `-Z` branch (radius minus distance) is dead: an
  offset with the anti-parallel normal grows the radius too. Ported as written.
- **Uninitialised reads upstream.** `SmCircle(P0, P1, P2, ..)` ignores the status of
  `smgu_CircleFrom3Points` (collinear points leave its outputs uninitialised; the shim
  checks the status and does not build); `AssertValid` / `Trim` use the STEP/NURB
  conversion result even when it failed; `ConvertTo2D` reads `adSols[1]` when the
  quadratic has fewer than two roots; `SmPolarConversion::ReadFromDB` reads uninitialised
  locals for unreadable ASCII headers, and `ConvertToNURBSParameter` /
  `ConvertToPolarParameter` / `IsCurrent` index empty or short tables (a record whose
  tables are empty crashes `AdjustSTEPInterval`; the generator only writes tables as
  `SetUpPolarConversion` builds them). The port returns `InvalidInput` in all these cases.
- **Kernel threading.** Joining `SmEllipse` to `Curve` (`CurveKind::Ellipse`) needs the
  kernel's zone tolerance in every override that rebuilds the polar converter
  (`STEPInversion` of the seam): `Curve::{copy, convert_to_2d, convert_t_from_nurbs_to_step,
  step_inversion, edit_parameterization, reverse_parameterization, transform,
  read_from_db, create_analytic_curve, approximate_curve}`, `CurveCache::analytic_curve`,
  `analytic_curve_intersect`, `fix_chain_curves_list`, `crvs_fix_gaps`,
  `BSplineCurve::{create_analytic_curve, create_simple_offset}`,
  `Curve::{calculate_bounding_box, is_planar, approximate_curve_base}`, the curve-on-surface
  methods that copy, reparameterise, approximate or read their UV curve, and the projected /
  offset / in-volume / tangent-field readers and wrappers now take `&Kernel`.

## surface.surface_p2 (`SmSurface` part 2: object, dispatch, `smsurf_*`, statics)

- **`smsurf_ComputeSurfacePointType` writes the v-side classification to the u-side
  output** (`if (pOptSurfSideDVV) *pOptSurfSideDUU = sSurfSideDVV`); the v output is never
  written and asking for the v side without the u side dereferences `NULL`. Ported as
  written (the v output is a flag); the ffi wrapper rejects v-without-u.
- **`sm_MergeDoubleArrays` stops when either array is exhausted**: the tail of the other
  array is dropped (the `while` needs both). Ported as written.
- **`SmDerivSurfDefinition::CreateCrossSectionCurves`** takes the Taylor third-derivative
  coefficient from the *second* derivatives (`sD = pOptStart2ndDerivs->GetAt(ii)`) when no
  end values are given. Ported as written.
- **`sm_SetUpBreaks`** reads `rBreaks[0]` of an empty array; `SmLSIGlobalSolver::
  BranchMayContainAnswers` reads an uninitialised hit count when `SmExtent3d::IntersectLine`
  fails (zero line vector). The port returns `InvalidInput` / the line error.
- **The `Surface` type** (`surface::surface::p2::Surface`, `SurfaceKind`) is introduced here
  with the B-spline as its only kind; later surface units add their kinds. Code generic over
  a surface (tangent fields, `SurfaceMaxDistanceBetween`, `SetIntersectionCurveThroughPoint`)
  now takes any `SurfaceEvaluate` (implemented by `Surface` and `BSplineSurface`). Solution
  objects are opaque handles, so `geom.solution_array`'s surface evaluations keep their
  `UNPORTED(surface.surface_p2)` markers until `SolutionObject` can reach a surface.
- **`smsurf_GetSurfaceCache` / `sm_GetOriginalSurface`** need the surface caches (no
  `SmCacheMgr` surface cache exists yet): `Unported(surface.surface_cache_p1)`; `geom.grid`'s
  `ExpandHierarchy` marker moved there.
## curve.composite_curve_p1 (`SmCompositeCurve`, part 1)

- **`GetKnots` reads past the knot array.** After the first segment, `lLastIndex =
  rKnots.GetSize()` (not `GetSize() - 1`), so the first transition reads
  `rKnots.GetAt(GetSize())` and writes `pKnotMultiplicities->SetAt(GetSize(), ..)`, one
  past the end (`SM_ASSERT_BREAK` is compiled out): any composite of two or more segments.
  With `pOptIvl` the trim loop `for(ii=GetSize()-1; ii>=0; ii--)` runs on an unsigned
  `ULONG` and never ends (it reads `rKnots[ULONG_MAX]`). The port returns `InvalidInput`
  in both cases (after the segment `GetKnots` calls upstream makes first); difftest only
  sends composites of at most one segment, without an interval.
- **`ApproximateCurve` dereferences `NULL`** when `CreateByJoining` fails before making a
  curve (unconnected segments, no segments): `rpNewBSplineCurve->AssertValid()`. The port
  returns the join's error; the ffi only sends connected 3D B-spline / Hermite chains.
  `AssertValid` of the joined curve reaches `GlobalPropertyAnalysis`
  (`LocalPropertyAnalysis`, unit `curve.curve_p5`) for every non-degenerate, non-analytic
  join, so these cases are deferred.
- **`CalculateBoundingBox` shifts the segment intervals from `0`**, not from the first
  segment's start (as `GetNaturalInterval` does), and passes the *shifted* interval to the
  segment's own `CalculateBoundingBox` (the B-spline override clamps it to its range).
  Ported as written. `*pBBox` is written unconditionally (`NULL` crashes); the port always
  computes the axis aligned box and rejects a request without it.
- **`sm_CreateCorner` (chamfer) reads `sSolutions[0]`** of an empty intersection array
  when the chamfer line misses an extension line. The port returns `InvalidInput`; the
  shim copy of the static returns `SM_ERR_INVALID_INPUT` at that point (its only change).
- **Copy constructor** ignores each segment's `Copy` status and leaves a `NULL` segment
  curve (Hermite curves have no `Copy`), which later calls dereference; the port returns
  the error, the ffi sends no Hermite segments to `Copy`.
- **`sm_FlushCrv`** is listed as unused by the call graph (another file has a static
  function of the same name) but is reached from `BuildCompositesFromCurves`; it is ported
  in this unit.

## curve.composite_curve_p2 (`SmCompositeCurve`, part 2)

- **`sm_CreateCorner` chamfer crash reaches the offsetters.** `CreateOffsetsOfManyCurves`
  (and `CreateTrimmedOffsets`) with `SM_OC_LINEAR_CHAMFER` segfault in upstream
  `sm_CreateCorner` (`sSolutions[0]` of an empty intersection array, see
  `curve.composite_curve_p1`) whenever the chamfer line misses an extension line, which
  random planar polygons hit often. The port returns `InvalidInput` there; the ffi sends
  the offsetters fillet and linear-extension corners only.
- **`sm_CheckBowTies` indexes `crOffsets` by the original curve count** (`crOffsets[0]`,
  `crOffsets[lNumOrig-1]`) and `crOrigCurves[0]` without checks; `IntersectAndTrimOffsets`
  ignores its status. Fewer raw offsets than original curves (degenerate offsets dropped
  with one-sided offsetting) read past the array. The port returns `InvalidInput` and the
  trimmer goes on without bow-tie points; the ffi requires `1 <= originals <= raws` for
  `IntersectAndTrimOffsets` (the shim's copy of `sm_CheckBowTies` returns
  `SM_ERR_INVALID_INPUT` at those points).
- **`SmCompositeCurve::ReadFromDB` ignores the segment statuses.** A segment without a
  curve, or whose curve fails to read, keeps a `NULL` curve (every later use
  dereferences it); the port returns `InvalidInput`, the shim reports such a composite as
  `SM_ERR_INVALID_INPUT`. An unreadable ASCII segment count leaves `SM_UNDEF_ULONG`
  (123456789) segments to `calloc` and read with uninitialised locals; the port rejects
  counts the remaining input cannot hold.
- **`MakeCompositeNurb` is `const` but `CreateByJoining` raises the segments' degree in
  place**; the port takes `&mut self` and writes the raised B-spline segments back
  (analytic segments derived from `SmBSplineCurve` join through a copy).
- **Deferred cases** (unit `curve.curve_p5`): most trimmed offsets reach
  `SmCCIntersectENFO::Evaluate` (curve/curve intersection), `SmCurve::IsArc` (offset
  approximation) or `SmCurve::LocalPropertyAnalysis` (`sm_FixBowTie`, classification).

## surface.curve_bounded_surface

- **`SmCurveBoundedSurface::operator==` reads past the UV array.** The UV loop runs to the
  number of 3D boundaries; with more 3D than UV boundaries (and equal counts, flags and
  3D curves) upstream reads `m_vUVBoundaries[i]` out of bounds and crashed the difftest
  (`drop_curve_eq_other_same` before the guard). The port stops at the end of the UV
  array; the ffi rejects `EqOther` when `n3d > nuv`.
- **`SmCurveBoundedSurface::ReadFromDB`** reads the UV boundaries with the type of the last
  3D boundary (`l3DType`, `0` when there is none, so any UV boundary then fails with
  `SM_ERR`) and drops the implicit-outer flag it reads (the new surface gets `FALSE`). The
  port keeps both; the difftest covers them with `WriteToDB`-layout ASCII records.
- **Deferred cases**: the evaluation overrides call basis-surface virtuals the Rust
  `Surface` dispatch does not have yet (`surface.surface_p1`: `EvaluateSimple`,
  `EvaluateSTEP`, `EvaluateSTEPPoint`, `SmSurface::operator==`; `surface.surface_p3`:
  `EvaluateGeometric`, `EvaluateNormalSection`; `surface.surface_p5`:
  `EvaluateNormalAtSingularity`; `surface.surface_p7`: `EvaluateNormal`). `SmCrvOnSurf` and
  `SmIsoCurve` still hold a B-spline surface; reading one over a curve bounded surface
  returns `Unported` (`surface.surface_p1`).

## surface.surface_cache_p1

- **`GetSurfaceContinuity` skips the last internal v knot.** The v loop runs
  `iv + 1 < size - 1` (the u loop `iu + 1 < size`), so the continuity at the last
  internal v knot is never looked at. Kept; the difftest covers it.
- **`sm_CullArray` on an empty array** ends with `SetSize(1)`, which callocs one `0.0`
  element; the port returns `[0.0]` too.
- **`BuildTreeTop` re-initialises `m_sAuxMgr`** (active count 0, blocks kept), so the
  head nodes it makes reuse the aux memory of any earlier `SM_AD_AUX_DATA` /
  `SM_AD_BEZIER_SURFACE_HEAD` nodes; upstream only calls it on first generation
  `SmBezierPatch` nodes, which the difftest (and the ffi) keep to.
- **`SmBezierAux2d::BuildPolarBox`** dereferences `mBA_pSurface` of every node on the
  chain; `BuildTreeTop` heads have a null one until `BuildTree` resets them to the owner
  surface. The port returns `InvalidInput` for a node without surface.
- **`SmTreeNode::GetFirstBezierPatch` from a bare tree.** The decomposition data
  (`SmBezierPatch` in `m_sBezMgr`, `SmBezierAux2d` in `m_sAuxMgr`, the subdivision
  surfaces) lives in the `SurfaceCache` arenas; tree nodes carry the arena slot
  (`SurfaceAux::slot`). `SurfaceCache::first_bezier_patch` answers from the cache, the
  tree-only `Tree::first_bezier_patch` (and `SmSurfaceGridElement::Init`,
  `sm_CsArePerpenDisjoint`, which start from a bare tree node) stay `Unported` until the
  tree construction (`surface.surface_cache_p3`) hands the data to the tree, as
  `CurveCache::build_tree` does with `set_curve_data`.
- **Deferred cases**: every polar box (`SmBSplineSurface::CalculateBoundingBox` samples
  `SmSurface::EvaluateNormal`, `surface.surface_p7`), boundary bounding planes with
  collinear samples (straight boundaries: `EvaluateNormal`), every
  `sm_ComputeBezierApproximation` (`SmSurface::IsSingularity`, `surface.surface_p5`) and
  `GetFace` (`surface.surface_p1`).

## curve.curve_p5 (`SmCurve`, part 5)

No Rust-vs-C++ disagreement: 3067 of 3067 non-deferred difftest cases bit-identical (97
upstream-derived and edge-case fixtures: `TestSmCreateRectangle` / `TestSmCreateRegularPolygon`
edges through clean-up, duplicate removal, `IsLine` / `IsLinear` / `IsArc` / `Length` /
`IsDomainBoundary`, the `my_test_reversed_line_join` lines, closest-point inversions, Newton
Jacobians; plus 3000 generated; a 20000-seed run is clean; `--perturb` detects every case).
30 cases are deferred: `Length` of spans of degree above 1 (`SmIntegrator::IntegrateIt`, unit
`mass.integrator`; 28) and gap closing that reaches `SmCurve::GetNaturalInterval` through a
solution handle (an `UNPORTED(curve.curve_p2)` stub in `geom::solution_array`; 2).
Upstream behaviour worth knowing:

- **`CrvsCleanup` crashes (SIGSEGV)** when the curve next to a removed tiny curve is closed:
  `TrimGap` between the previous curve and a closed curve (end points equal) dies inside
  upstream (reproduced with a cubic segment, a tiny segment and a closed quadratic loop).
  The port completes the clean-up. Difftest never generates a closed curve next to a tiny
  one. `CrvsCleanup` also casts every neighbour to `SmBSplineCurve` for `TrimGap`; the port
  returns `InvalidInput` for any other kind there and the ffi only sends B-splines.
- **`Length` casts its cache curve to `SmBSplineCurve`** and integrates any curve the cache
  tessellates, Hermite curves included (the Rust cache tessellates them as upstream does),
  so a Hermite `Length` is deferred like any curved span, not the end point distance the
  comment promises.
- **`SmFindCCNormalENFO::Evaluate`** reads `m_cpOptVectors[0..1]` without a `NULL` check for
  `SM_SO_SIGNED_PIVOT_MINIMIZE` (the port returns `InvalidInput`), and the iteration count
  that triggers the curvature "step back" only counts evaluations with a Jacobian.
- **`sm_IsPointSolutionAcrossClosedSeamBoundary`** compares the solution's object pointers
  with its curves; the port's caller (`GlobalCoincidenceChecker`) passes whether the first
  object is `this` (the split solutions always name `[this, other]`).
- **`SmCurve::CacheMakeOrValidate`** is reached in the port only through
  `make_curve_cache` (building) and `validate_curve_cache` (type check, reuse);
  `GetOrCreateObjectCache` on an object without the curve returns `InvalidInput` when a
  new curve cache would be needed.
- **Wiring this unit exposed four harness / port gaps in dependent units** (their cases
  were deferred on `curve.curve_p5` before):
  - `SmPolarConversion::ConvertToNURBSParameter` writes the polar-table guess to
    `rdNurbsParameter` before the point inversion; when the inversion fails (`SM_ERR`)
    callers that ignore the status (`SmEllipse::AssertValid`) use that guess. The ellipse
    port now does the same (`convert_to_nurbs_parameter_or_written`); without a usable table
    nothing is written and upstream reads an uninitialised `dNurbT` (UB): `curve.ellipse_p1`
    no longer generates those cases.
  - `SmSolution::IsEqual` scales its parameter tolerance by the object's natural interval;
    the `curve.curve_p3` / `curve.curve_p7` runners' solution handles now are curve objects
    with the curve's interval (as upstream's `SmCurve*`), so near-equal minima merge.
  - `GlobalPointSolve` answers from the cache's analytic curve, whose pointer names the
    solution: `Curve::global_point_solve_step` takes that handle (`analytic`).
  - `SmGlobalSolver::SolveIt` initialises `sStartBranch[i]` only for `i < m_lNumTrees`; a
    one-tree solve whose top node is a leaf hands an uninitialised `apBranch[1]` to
    `LocalSolve`, and `SmCurvePropertyGS::LocalSolve`'s angle special case copies it into
    `m_apNodes[1]` of a one-object solution. The `curve.curve_p7` harness reports the second
    node only for two-object solutions.


## surface.trim_srf_cache: resumed handoff port (2026-10-13)

Resumed the interrupted `152e005` WIP under the draft-PR coordination workflow.
The constructor and type/state queries, PointTest bypass, and edge-use insertion
were compared with the pinned C++ reference: 620 cases (20 fixtures and 600 seeded
cases), 458 bit-identical, 0 failed, 162 deferred. The perturbation check detected
all 458 perturbed outputs. The touched surface.surface_cache_p1 runner also passes
its non-deferred cases (872 total, 709 identical, 163 deferred).

Deferred paths: surface.surface_cache_p3 (BuildTree, FindUVNodes, SubdivideNode;
103 cases), surface.surface_p1 (GetFace; 33), surface.surface_p3 (GetSingularities;
26). ImplantVertexuse and ImplantPolyEdge have translated bodies but cannot yet
execute past Evaluate1stDerivatives / MapTo2d (surface.surface_p1); Rust regressions
check that these explicit deferrals preserve state. They are not claimed as
C++-verified. The base MarkFaceContainment dispatch remains unported before it
could reach the trim-specific PointClassify; it must use that override when wired.
The face-build path preserves the temporary brep-editing guard through explicit
unported topology accessors, restoring the flag on both success and failure. These dependencies prevent end-to-end trimmed-face use.

Fixtures exercise prepend order, shared boundaries, degenerate and curved trim
curves, existing face extents and the deferred subdivision/pole/face paths. File
fixtures use Surface.sms and offset_nurb.sms from the allowed upstream corpus.
The C++ shim deliberately excludes classifying a surface without a face: upstream
would dereference null. FFI regression tests reject this path, truncated/trailing
records, invalid counts and non-finite coordinates before entering C++.

### surface.trim_srf_cache: review follow-ups (2026-10-13)

- **Test gap closed.** The generator's negative-area face extents for
  `ImplantEdgeuse` were rejected by the FFI validator (`rd.domain()` requires ordered
  bounds) and silently redrawn, so C++ never saw them. The validator now only checks
  finiteness; difftest is 620 cases, 462 bit-identical, 0 failed, 158 deferred
  (surface.surface_cache_p3 100, surface.surface_p1 32, surface.surface_p3 26);
  `--perturb` detects 462/462.
- **MarkFaceContainment dispatch gap (no UNPORTED marker possible).**
  `TrimSrfCache::build_tree` calls `SurfaceCache::mark_face_containment`, which uses
  the base `SurfaceCache::point_classify` (always `SM_ERR`). Upstream
  (`SmTrimSrfCache.cpp:343`) reaches the virtual override `SmTrimSrfCache::PointClassify`
  (`:1048`). That override belongs to surface.trim_srf_cache itself
  (`harness/callgraph/units.json`) and is already ported as
  `TrimSrfCache::point_classify`, so an `UNPORTED(<unit>)` marker would point at the
  wrong porter. The call site carries a `DISPATCH GAP` comment instead. It is
  unreachable today (the face path returns `Unported` earlier, at
  `CreateUVTrimCurves`), but whoever wires topology.face_p3 / topology.face_p4 here
  must make `mark_face_containment` dispatch to the trimmed override (e.g. take a
  classifier argument) before any face-backed case can pass.

## surface.plane_p1 (`SmPlane`, part 1)

All non-deferred cases agree bit for bit with upstream (`max_rel_err = 0`). Upstream
behaviour worth knowing:

- **`SmPlane::Reparameterize` keeps the origin.** The new origin (`sOriginMax`) is only
  computed inside `SM_DEBUG_CODE`. The release build changes the analytic domain and the
  scales but keeps `m_vPosition`, so the analytic plane no longer matches its NURB unless
  the new domain starts where the old one did. The port keeps this (fixture
  `edge_reparameterize_keeps_origin`).
- **Lazy NURB through `const_cast`.** An `SmPlane` built by its constructors has no NURB.
  `const` methods that need it (`GetNaturalUVDomain`, so `CoincidenceCheck`,
  `CreateIsoParametricCurve`, `DropPointFast(SM_SO_NORMALIZE)` via `SmTol::MapTo2d`) make
  it on `this` through `GetOrCreateGwNurbPointer`. So does the `SmBSplineSurface` copy
  constructor, on the *source*. The port makes it on a copy (`Plane::base_with_nurb`), so
  the source keeps none. Only `ToggleSwapUVBit` (which leaves the NURB alone) and
  `operator==` (which compares NURBs) can see the difference. The runner generates those
  two only while both sides agree, and the adversarial fixtures (`zz_adv.json`) leave
  out the sequences that observe it (`==` after `Copy`, `ToggleSwapUVBit` after
  `CreateIsoParametricCurve` or `CoincidenceCheck`). The shim copies planes without a NURB by rebuilding
  them from their analytic data, because its own copies would otherwise change plane A.
- **`SmPlane::Evaluate` with `bOnlyUpperHalf` and more u than v derivatives** writes
  outside its array (the `ULONG` index `lHighestVDeriv - k` wraps). The port returns
  `InvalidInput`, and the ffi wrapper rejects that input.
- **Deferred paths:** `CoincidenceCheck` with a surface that is not an `SmPlane` needs
  `SmSurface::IsPlanar` (`surface.surface_p7`), then `CreateNaturalUVTrimCurves`
  (`surface.surface_p6`) and `DropPoint` (`surface.surface_p5`). `DropCurve` with a curve
  whose control points must be clamped to the domain, or with a curve that is neither a
  line nor a B-spline kind, needs the general `SmSurface::DropCurve`
  (`intersect.surface_drop_lib_p3`). `GlobalLineIntersect` of a line lying in the plane
  that meets the domain at a single line parameter needs the general
  `SmSurface::GlobalLineIntersect` through the surface cache (`surface.surface_p8`).
- **`SmSurfaceCache` and planes.** Upstream the `SM_CAST_PTR(SmBSplineSurface, plane)`
  in the cache succeeds and the virtual calls reach the plane. The port's cast gives the
  plane's B-spline part, and the two virtual paths return `SolidError::Unported`
  (marker `UNPORTED(surface.plane_p1)`): `SmPlane::CreateIsoParametricCurve` (ported, but
  it makes an `SmLine` and the cache stores its iso curves as `BSplineCurve`, which
  would drop the line's overrides; wiring needs `Curve` storage in the cache) and the
  inherited `SmBSplineSurface::CalculateBoundingBox` (which dispatches back to the plane
  through `IsKindOf` and `SmPlane::EvaluateNormal`; the plane does not re-dispatch it yet).
- **Undefined behaviour the harness keeps away from upstream** (found in review):
  `SmPlane(Origin, Normal)` with `|n|^2 < SM_EFF_ZERO_SQ`: `MakeUnitOrthoVectors` fails,
  the status is ignored and the axes stay uninitialised (the ffi encoder rejects a normal
  with `|n|^2 < 1e-24`). `Reverse` and `RebuildSTEPFromNURBParameters` on a plane whose
  NURB cannot be made (degenerate or inverted domain) pass a `NULL` NURB to
  `N_SrfReverse` / `SmBSplineSurface::EvaluatePoint` and segfault; the shim checks
  `GetOrCreateGwNurbPointer()` first and reports `SM_ERR`, which is what the port
  returns. `CreateExtendedSurface` with a zero distance loops forever (the halving never
  falls below `dDist / 100`); the ffi encoder accepts only positive distances.
- **`CreateExtendedSurface` on a plane** runs the `SmBSplineSurface` body with the
  `SmPlane` overrides of `IsSingularity` (none) and `IsClosed` (never), so a plane
  extends where a plain B-spline would still be waiting on `surface.surface_p5` /
  `surface.surface_p7`.

## surface.unbend_volume (`SmUnbendVolume`)

All non-deferred difftest cases agree bit for bit with upstream (`max_rel_err = 0`, 1874
default cases, a 30,374-case run clean). The 67 deferred cases are `IsCurveInParamDomainSimple`
(66: `SmCurve::HasPlaneCrossings` needs `SmSurface::GlobalCurveIntersect`, unit
`surface.surface_p8`) and `ReadFromDB` (1: base read `SmVolume::ReadFromDB`, unit
`surface.volume_p2`). No upstream test builds an unbend volume. `SmVolume::EvaluatePoint` and
`SmVolume::IsPointInParamDomain` (unit `surface.volume_p2`) are co-ported (marker
`CO-PORTED(surface.volume_p2)`); `SmCrvInVolume::EvaluatePoint` now uses the former. Upstream
behaviour worth knowing (all ported as written):
- **`IsPointInParamDomainSimple` is always `TRUE`.** The inline header body calls
  `IsOnBoundarySimple(P, bB, bB, bB)` with one `SmBoolean` for all three out-flags; the last
  write `rbOnW = FALSE` clears it, so `return(rbOnU || rbOnV)` is `FALSE` for every point. As a
  result degenerate segments in `IsLineInParamDomainSimple` are always "in", and the domain check
  of `GlobalPointSolveSimple` never fails (it also checks the *projection* point as if it were a
  parameter point). `IsOnBoundarySimple` called with distinct flags works as documented.
- **`GW_MAX_DERIV` is 5** (`SmNurbsCrv.h`), not 3: `Evaluate` / `EvaluateSimple` accept
  `n = 4, 5` and return zero derivatives above the third. With a next map and `n > 3`, the
  `Map` call fails before the 64-entry `aNextDerivs` buffer could overflow.
- **`EvaluateBoundingBoxSimple` maps the box edges through the whole volume.** The lines that
  would make the volume a simple map for the duration are commented out ("do not compile on
  gcc"), so the "ProjSpace" boxes include the orient and next maps. A bounded box with a side
  of zero length makes `SmLine::CreateLineSegment` fail on that edge; upstream then
  dereferences a null `pLine` (first edge) or the line the previous `SmCrvInVolume` already
  deleted (owner flag 1). The port returns `InvalidInput`; the ffi rejects such boxes.
- **`operator==`** overwrites the neutral-distance comparison with the singularity-tolerance
  comparison: unbends differing only in neutral distance compare equal.
- **`IsLineInParamDomainSimple`** tests the end points' discontinuity with
  `SM_ARE_SAME(P.y, m_dSingularityTol)` (against the tolerance, not `0`).
- **Constructor repair** has the same `Unitize` status comparison as the twist volume (only
  the last zero-vector branch runs); a neutral distance below the clamped singularity tolerance
  becomes `1.0`. The default constructor gives neutral distance `0` and tolerance
  `SM_UNDEF_DOUBLE`.

## surface.surf_of_revolution_p1 (2026-10-13)

Ported `SmSurfOfRevolution` part 1 (`crates/solidcore/src/surface/surf_of_revolution/p1.rs`)
with `CreateCanonical`, `MakeNurb` and `MakeNurbWithSweepParams` co-ported (units `_p2`,
`_p3`). difftest: 807 cases (207 fixtures, 600 seeded), 776 bit-identical, 0 failed,
31 deferred (`IntersectWithPlane` of a planar generator needs `SmPlane`,
`surface.plane_p1`); `--perturb` detected 776/776.

- **`IntersectWithPlane` after `surface.plane_p1` (merge of main, 2026-10-13).** The
  `UNPORTED(surface.plane_p1)` stub (`SmPlane::GetPosition`) is wired: the method takes a
  real `Plane` and the difftest builds it with `Plane::create_canonical`, as the shim. The
  31 planar-generator cases stay deferred, now on `surface.surface_p1`: before any branch
  upstream takes `crPlane.CalculateBoundingBox(GetNaturalUVDomain(), NULL, &sPlanePBox,
  NULL)` with the default `bExpandPosBoxesByZoneTol3d = TRUE`, which expands the pseudo
  box by `GetFace() ? GetFace()->GetTolerance() : ...` (`SmSurface::GetFace`, unported;
  upstream ignores the status, `Unported` still propagates). The body below the plane box
  (perpendicular / parallel axis cases) is not ported: both of its curve-producing paths
  call `SmPlane::TrimCurveToPlaneDomain` (`surface.plane_p2`), so it stays behind an
  `UNPORTED(surface.plane_p2)` stub for that unit's porter, unreachable until `GetFace` is.

- **Generator owner stubs not wired.** `SmLine`, `SmEllipse`, `SmHyperbola` and
  `SmParabola::MakeNurb` reparameterise a generator owned by a surface of revolution to
  `((SmSurfOfRevolution*)m_pOwner)->GetGenDirParamExtent()`. The Rust curves only hold an
  `ObjectRef` (type code + key) for their owner, which cannot reach the surface, so the
  four `UNPORTED(surface.surf_of_revolution_p1)` stubs stay (they return `Unported` for
  such an owner). `SurfOfRevolution::gen_dir_param_extent` is ported; wiring needs an owner
  model that can hand a curve its owning surface's data. The Rust `SurfOfRevolution` leaves
  its generator unowned, so it never reaches the stubs; upstream's owner-driven step only
  differs when `MakeNurbWithSweepParams` adjusts a generator whose STEP interval differs
  from the surface's V domain (never through `CreateCanonical`).
- **Upstream re-entrancy hazard (avoided by the harness).** The constructor sets the
  generator's owner, then evaluates it. An analytic generator without NURB (an `SmLine`
  fresh from its two-point constructor) then runs `MakeNurb`, which asks the owner for
  `GetGenDirParamExtent`, whose `GetNaturalUVDomain` makes the surface's NURB through the
  virtual `MakeNurb` from inside the constructor. The difftest shim makes the line's NURB
  first; the Rust port evaluates the generator without the owner detour.
- `RebuildPolarConverter` dereferences a null iso curve when `CreateIsoParametricCurve`
  fails (upstream only asserts); the port returns the error.
- `sm_TestForIsoCircles` leaves outputs unwritten when no iso curve is non-degenerate or no
  two iso planes intersect; with parallel planes after the first common line, it keeps
  comparing the last line found (stale `smgu_IntersectTwoPlanes` outputs). The port returns
  the unwritten outputs as `None` and keeps the stale-line behaviour.
- **Review fixes (PR #15).** `CreateSurfOfRevolution` reparametrises the sweep circle with
  arc length when the generator is an `SmLine` parallel to the axis within 0.001 degrees
  (a cylinder: sweep knots span the arc length, `0..2*pi` for radius 1, not `0..1`); the
  port missed it because the generator reached `BSplineSurface::create_surf_of_revolution`
  as its plain B-spline base. `make_nurb_with_sweep_params` now passes the line vector to
  `create_surf_of_revolution_with_line_vector`. `CreateCanonical` ignores the status of
  `Unitize`, so a zero axis (length squared below `SM_EFF_ZERO_SQ`) becomes `(0, 0, 1)`;
  the port does the same (`unitize_or_z`). `SetOutOfBoundsEnabled` also sets the
  generator's flag, as upstream's override; the shim now calls it instead of writing the
  base flag. The FFI accepts any finite `CreateCanonical` axis, and only unit placement
  axes for the constructor: a Y axis with `|y|^2 - 1 > SM_EFF_ZERO` makes
  `SmBSplineCurve::IsOnPlane` (via `smgu_PlanePointDistance`) fail before writing
  `bOnPlane`, which the constructor then reads uninitialised for a planar non-line
  generator. A zero sweep (empty angular domain) stays rejected by the FFI guard.

## surface.bend_volume (`SmBendVolume`)

All non-deferred difftest cases agree bit for bit with upstream (`max_rel_err = 0`, 2347
default cases, a 30,847-case run clean, `--perturb` SENSITIVE). The 75 deferred cases are
`IsCurveInParamDomainSimple` (74: `SmCurve::IsOnPositiveSideOfPlanes`, unit
`curve.curve_p9`, open in PR #13) and `ReadFromDB` (1: base read `SmVolume::ReadFromDB`, unit
`surface.volume_p2`). No upstream test builds a bend volume. Upstream behaviour worth knowing
(all ported as written):
- **`EvaluateBoundingBoxSimple` is analytic and has slips.** With `u` reaching
  `SM_INFINITE_PARAMETER`, the bounded `y` maximum is taken from the *cosines* of the end
  angles, and the `y` minimum is unbounded whenever `dThetaMin < 0 || dThetaMax <= 0`; the
  `SM_IS_CONTAINED` angle tests are not reduced to one period. The result is set with
  `SetMinMax`, whose failure for an inverted box (status ignored) leaves the caller's box as
  it was (a fresh `SmExtent3d` stays in its `Init` state).
- **`TrimParamBoundingBoxSimple`** clamps each `v` end to the primary period separately, so a
  box crossing `v = pi d` wraps to an inverted box; `SetMinMax` then fails silently and the
  output box keeps its previous contents (status `SM_SUCCESS`). It `Init`s the output and
  returns `SM_ERR` when the box is entirely below `u = singularity_tol` or outside the period.
- **`InvEvaluateGuessPointSimple`** returns `SM_ERR` within `GetLegalMinU()` of the axis but
  still writes its guess (`theta = 0`); on the seam (`theta = +-pi` within the scaled zero) it
  writes two guesses. `GlobalPointSolveSimple` ignores the parameter domain and never fails.
- **`IsBoundedSimple` is `TRUE`** (the half space and one period), so the base `IsBounded`
  is `TRUE` for every compound volume containing a bend, although the natural domain
  `[tol, inf] x [-pi d, pi d] x [-inf, inf]` is unbounded in `u` and `w`.
- **Constructor repair** has the same `Unitize` status comparison as the twist and unbend
  volumes (only the last zero-vector branch runs); a neutral distance below the clamped
  singularity tolerance becomes `1.0`. The default constructor gives neutral distance `0` and
  tolerance `SM_UNDEF_DOUBLE`, so its evaluations divide by zero (NaN / infinite values on
  both sides). `GW_MAX_DERIV` is not used: `Evaluate` / `EvaluateSimple` reject `n > 3`.

## topology.brep_data (`SmBrepData` readers, 2026-10-13)

- **Records start zero-filled.** `SmTArray::SetSize` `calloc`s / `memset`s new elements
  without running constructors, so every `Sm*Data` record read by `ReadFromDB` starts
  with all fields `0` (index fields, `UNSURE` flags, extents `[0,0]`, points `(0,0,0)`),
  not the header's member defaults. It matters where a field is not read (unknown shell,
  loop or edge use types; the edge interval and face domain when `SetMinMax` refuses
  `min > max`). The port's `*Data::zeroed()` reproduce it; `Default` is the C++
  constructor. Confirmed by the `extreme_values_v30` and `unknown_types_*` fixtures.
- **Tolerance build.** The reference build is `SM_USE_OLDTOL`: zone tolerances are read
  only for `version <= 43`, the `SM_USE_NEWTOL` fields (`m_bIsSmallTopology`, model size
  estimate) never.
- **Statuses.** Every record read and `ReadIndexedAttributesFromDB` status is ignored
  upstream. A failed ASCII attribute line skip (over 1000 characters, or a `0xFF` byte)
  keeps the indices read but creates no placeholders; `geom.attribute` gained
  `read_indexed_attributes_from_db_partial` for that (geom.attribute difftest unchanged).
- **FFI guard.** `solidcore_ffi::units::topology_brep_data` simulates the whole read and
  rejects any stream on which an extraction would fail or run short (upstream then reads
  uninitialised locals, UB-75), counts of `SM_BIG_ULONG` or more, geometry attribute
  indices past the table after a failed line skip (UB-76), closed composites without
  segments (UB-77), and geometry kinds it does not model (B-spline curves/surfaces,
  version <= 32 composites and offsets, type code 1). Upstream test files (`.smb` under
  `TestFiles/pt_TestFiles`, 186 read with `ReadFromFile`, 170 also with `ReadFromDB`, and
  the two part files' B-reps at version 32) are trusted past the first unknown kind.
- **Record loops stop on a failed stream (security review, 2026-10-13).** The one-byte
  per record bound (`checked_count`) assumed every record consumes input, but records
  ignore their read status (UB-75): once the stream fails or hits end of data each
  further record consumes nothing and is still appended zero-filled, so a count close
  to the remaining byte count allocated `count * size_of::<record>()` (a 100 MB file
  could cost tens of GB, per array). `ensure_more` now checks the stream before each
  record, composite item and old composite segment and returns `InvalidInput`
  ("stream ended before all declared records were read"). It runs before a record,
  never after the last, so a stream whose final record ends exactly at end of data
  (eofbit set, no trailing newline) reads fully. Only streams that are undefined
  upstream (UB-75) change; the difftest results are unchanged.
- **Deferred.** Offset surfaces (`surface.offset_surface`) and, in version 38 files,
  planes, cones and extrusions (`surface.plane_p2`, `surface.cone_p1`,
  `surface.surf_of_extrusion_p2`) are `Unported` on the Rust side.
- **Old composite curves.** Composite curve records of version <= 32 pass `n + 1` gaps
  (first start gap, then each end gap) to a constructor that reads `n`: the segment gaps
  come out shifted (WR-54). Ported as is. Their evaluation returns `SM_ERR` on both
  sides in the fixtures.

## intersect.surface_intersector_p1 (`SmSurfaceIntersector`, part 1, 2026-10-13)

- **Point arena.** `SmTsectPnt`s live in an arena of the Rust `SurfaceIntersector`
  (`alloc_point`, `TsectId`); the three intrusive `SmTList`s are `Vec<TsectId>`.
  Linking a point already on a list (undefined upstream: it corrupts both lists) is
  `InvalidInput` in Rust; the shim does not run such an `Add*`/`Assign` op and both
  sides report the op status `-2` (`ffi::REJECTED`). Later parts (`_p2` .. `_p5`) reach
  the lists through the `pub(crate)` fields.
- **Co-ported `SmSurface::IsClosed`.** The constructor needs `IsClosed(dom, SM_SP_U/V)`
  (unit `surface.surface_p7`, not ported). It is ported as a private helper
  (`surface_is_closed`, default tolerance, no continuity) together with the one-line
  wrappers `EvaluateDirectionalDerivs` / `Evaluate1stDerivatives` (`surface.surface_p1`).
  Statuses upstream ignores there keep the previous iteration's values (upstream's
  locals live across the sample loop); a failed `smgu_FrenetFromDerivatives` leaves a
  zero curvature vector, as upstream's partial write does. Replace the helper with the
  real `Surface::is_closed` when `surface.surface_p7` lands.
- **`SER` in boolean methods.** `IsPointOnCurve` / `sm_IsPointOnCurve` return the
  status of a failing call as `TRUE`; the port returns `Ok(true)` (upstream-issues
  WR-52). `AddStartPoint`'s "replace" branch is a no-op (WR-50);
  `IsPointOnStartPoint` compares squared distances with the tolerance (WR-51).
- **Difftest.** Programs over planes and B-spline surfaces (open patches, rational
  patches, closed tubes that pass `IsClosed`, self-intersection), sub-, cut, disjoint,
  degenerate and fillet domains, scales 1e-6 .. 1e5. `TestSpanAccuracy` leaves its
  deviation unset on an angle rejection; the shim reports `-777` (`ffi::UNSET`) there.
  Deferred: refinement of crossing/tangent-curve/singular points
  (`LocalSurfaceIntersect`), `StepBackFromSingularity` (`LocalPointSolve`),
  `sm_ComputeCurveType` off planes or with parallel normals, `IsPointOnCurve` in its
  10x-tolerance band (`SmSurface::DropPoint`) and bounded/exact start point pairs
  (`smsurf_EvaluateNormalSection`). `EvaluateLawPoint` (needs `SmFilletLaw` and
  `SmOffsetSurface`) has no cases: the shim cannot build an offset-surface intersector
  the Rust side can mirror.

## surface.b_spline_volume_n_lib

- `SmBSplineVolume::CalculateContinuitiesSimple` with `bCalcGeometric == TRUE` (the
  default, and what `HasDiscontinuitiesSimple` passes by default) samples iso-parametric
  curves through `EvaluateIsoParametricCurveSimple`, which is `SM_ERR` without
  `USE_VOLUMES`; the call fails at the first sample after `rParams` holds the knots,
  `rConts` is empty and the minimum is `SM_CT_CINFINITY`. `HasDiscontinuitiesSimple`
  ignores the status, so a B-spline volume reports no discontinuities unless asked for
  representational ones. The unreachable rest of the loop samples `sKnots2[ii]` (not
  `[jj]`), reading past `sKnots2` when it has fewer samples than `sKnots1` (stale stack
  memory); the port returns `InvalidInput` there.
- Representational continuities (`bCalcGeometric == FALSE`) size `rConts` to the knot
  count but fill only the first `count - 1` entries; the last stays zero
  (`SM_CT_UNDEFINED`), and the first (an end knot) is `SM_CT_C0` for clamped knots, so
  `HasDiscontinuitiesSimple(.., FALSE)` is `TRUE` for every clamped volume.
- `SmVolume::HasDiscontinuitiesSimple`'s `bCalcGeometric` had been dropped in the Rust
  dispatch (`Volume::has_discontinuities_simple`); it is now a parameter. The
  `IsCurveIn*Domain` / `IsLineInParamDomain` volume entry points take a `&Kernel` for the
  curve bounding boxes the B-spline override computes.

## sweep.api_surfaces: review fixes (PR #37, 2026-10-13)

- The FFI guard (`crates/solidcore-ffi/src/units/sweep_api_surfaces.rs`) used to reject
  2D NURB data (`z = NL_NOZ`) and any `dim != 3` for the `Sweep`, `Ruled`, `Skin` and
  `Revolution` curves. Upstream `CreateSweepSurface`, `CreateRuledSurface`,
  `CreateSkinnedSurface` and `SmApiCreateSurfaceRevolution` take these curves without
  crashing (reviewer probe: 4081 generated cases plus explicit 2D fixtures, 0 failures,
  0 crashes). The guard now only requires positive weights (non-positive weights stay
  rejected outside `Extrude`); the sweep, ruled and skin generators produce 2D curves.
  This was lost coverage, not a numerical disagreement: the Rust port already agreed.
- 2D NURB revolution (`adv_rev_2d_nurb`, `adv_rev_dim2_3ddata`) runs through the C++
  without crashing; the Rust side is deferred on `CreateRotationalSweep`
  (`primitive.primitive_creation_p3`), so it is compared once that unit lands.
- `SmApiCreateSurfaceRevolution` returns `SM_ERR` when `SmCurve::Copy` fails (allocation
  failure only); the Rust `BSplineCurve::copy` is infallible, so the port has no such
  path (documented in the module's Deviations).
- `fixtures/sweep.api_surfaces/adversarial.json` (59 cases): sweep vectors at and just
  above `SM_EFF_ZERO`, scales 1e-9..1e9, 1D/2D/rational extrudes, degree 17, rational
  and point-degenerate sweep paths, ruled surfaces across degree/domain, 12-profile and
  mixed-kind skins, coincident profiles, skin-primitive validation (counts of 2^20,
  all-NULL curves, zero profiles), 2D ruled/skin/sweep/revolution. All pass or defer.

## surface.surface_p7 (`SmSurface`, part 7)

- **Co-ported callees.** `SmSurface::Evaluate1stDerivatives`, `Evaluate2ndDerivatives` and
  `EvaluateDirectionalDerivs` (unit `surface.surface_p1`) are non-virtual one-call wrappers of
  the virtual `Evaluate` that nearly every function here calls first; stubbing them would
  have deferred the whole unit. They live in `surface::surface::p7`, generic over
  `SurfaceEvaluate`, and are recorded as `CO-PORTED` in `surface.surface_p1`'s notes. Their
  stubs elsewhere (`b_spline_surface::p2`, `b_spline_surface_n_lib::p2`, `trim_srf_cache`,
  `crv_on_surf`) were wired too.
- **`SurfaceEvaluate::evaluate_normal`.** Code generic over the surface type
  (`TangentField<S>`) needs `EvaluateNormal`'s virtual dispatch; the trait method defaults to
  the `SmSurface` body (what `SmBSplineSurface` runs) and `Surface` dispatches to the plane and
  curve-bounded overrides. `surface_cache::p1`'s stub assumed no ported kind overrides
  `EvaluateNormal`; planes and curve-bounded surfaces do, so it now dispatches.
- **`IsClosed` and a failed `smgu_FrenetFromDerivatives`.** Upstream writes the position,
  the tangent and a zero curvature vector before returning `SM_ERR` for a zero tangent;
  `base_is_closed` now leaves exactly that (found by comparing with the copy co-ported in
  `intersect.surface_intersector_p1`, PR #33, which this unit's functions replace).
- **Mislabelled stub.** `b_spline_surface::p1::is_closed` was marked
  `UNPORTED(surface.surface_p5)` for `SmSurface::IsClosed`, which is this unit's; wired.
  `IsPlanar` of a B-spline defers on `SmSurface::GetFace` (`surface.surface_p1`), reached
  through `b_spline_surface::p1::owner_zone_tolerance`.
- **Upstream quirks kept.** `GetFlatCorners` masks its return with `&=` while filling the
  points; `SmSSGlobalSolver::BranchMayContainAnswers` returns `SM_SUCCESS` (`FALSE`) for a
  node outside its face and `NER`'s status (`TRUE`) for a missing patch; `sm_GoodDropResult`
  swaps the guess/result distance names; `FindDegenParamForDirection` compares a sample
  fraction with the parameter `dS`, and on Newton failure returns `SM_ERR` with `rUV` moved
  (the port returns `DegenParam { converged: false }`).
- **Not difftested.** `SmSPGlobalSolver` / `SmSSGlobalSolver::BranchMayContainAnswers` need
  surface cache trees with Bezier patch data (`surface.surface_cache_p3`); their non-normalize
  path is the green base `SmGlobalSolver::BranchMayContainAnswers`. The shim's
  `sm_LoadGuesses` copy takes the owner surface directly (two lines marked `ADAPTED`):
  reaching it through a tree node needs a real `SmSurfaceCache`.
- **Slow upstream solves.** `GlobalSurfaceSolve` (minimize) between two random surfaces
  that intersect runs for minutes in the C++ (a continuum of zero-distance answers along
  the intersection curve), and the global solves on 1e-4-scale surfaces with a 1e-6
  tolerance take tens of seconds. These ops always defer on the Rust side, so the
  generator keeps them at unit scale, drops `SM_SO_NORMALIZE` for `GlobalCurveSolve`, and
  covers `GlobalSurfaceSolve` with fixtures on separated surfaces only.
- **Curve-bounded surfaces** (without boundary curves) are generated too, so the
  `EvaluateNormal` / `IsClosed` dispatch of that kind is compared.
- **Generator note.** Ordinary random nets almost never reach `EvaluateNormal`'s fall-backs
  (zero-corrected first derivatives keep the cross product alive); the runner adds poles,
  exactly folded nets and pinched corners and samples boundaries, and the fixtures hit each
  branch (pole: second-derivative substitution; fold: zero result; pinch: step-off
  extrapolation).

## intersect.point_class (`SmPointClassification`)

- **Review fixes (PR #21).** The constructor takes an `SmZoneTol3d`, already converted, and
  stores it as is (converting again turned `-SM_USE_DEFAULT` into the system tolerance).
  `MergeIntoObject` runs `SmVertex::SetTolerance` in the reference build
  (`SM_USE_OLDTOL`); it is an `UNPORTED(topology.vertex)` stub. Topology kinds without
  their own `GetTolerance` (region, faceuse, loop, loopuse, vertexuse) use
  `SmTopology::GetTolerance` (`SM_ZONE_TOL_3D`), and a faceuse `SmTopology::IsConnectedTo`
  (`FALSE`); loop, loopuse and vertexuse do override `IsConnectedTo` and stay stubs. The
  shim now builds real `SmEllipse` / `SmCircle` curves, `SetClassObject(class, NULL)` is
  accepted for every class, and loads may read up to the last solution parameter.

- **`SM_PC_POINT` stores an `SmPoint3d*` cast to `SmObject*`.** Upstream then calls virtual
  `SmObject` members on it in `ComputePointParameters` (`SmTol::GetZoneTol3d(m_pObject)`),
  `GetGap3d` with a surface and `SmTol::GetObjZoneTol3d` / `GetXSectTol3d` (the same call),
  `IsConnectedTo` and `MergeIntoObject` (`GetType`): undefined behaviour. The port treats
  a point like any other non-topology object (system zone tolerance, not connected, not
  mergeable); the shim and the runner never send those ops for a point classification.
- **`GetGap3d(pSurface)` returns `SM_ERR` as the gap value** (1001.0) when the object
  point has no single drop point on the surface; the port does the same. Callers must not
  read it as a distance.
- **Release-build behaviour.** The reference build has no `SM_DEBUG_CODE`, so
  `SetTParam`'s `bSnapToIvl` does nothing and `SetClassObject` accepts any object for any
  class. `FindObjPoint3d`'s `SM_CAST_PTR` still checks the kind, so a mismatched class
  and object gives `SM_ERR_NULL_POINTER` (ported as such). Scripts only pair a class with
  an object of its kind.
- **Stand-in objects.** The shim builds vertices, edges and faces without a B-rep and
  `SmObject` stand-ins for volumes, regions, edgeuses and "other" objects. Upstream
  dereferences a stand-in in `FindObjPoint3d` / `GetGap3d` (volume, edgeuse),
  `ComputePointParameters`, `GetBrep` and `GetCurveObject` (edgeuse) and `MergeIntoObject`
  (region, face, edge), so those ops are rejected at run time on both sides and dropped by
  the generator. A stand-in region's zone tolerance is the system one upstream (it does
  not answer `IsKindOf(SmTopology_TYPE)`); the port asks `topology.shell` (deferred).
- **Deferred (356 of 2479 cases):** the surface point solve of `ComputePointParameters`
  and `GetGap3d` (`surface.surface_p4` 256; `SmSurface::EvaluateNormal`, which follows
  it, was wired by `surface.surface_p7`, PR #24) and every topology member (`topology.vertex` 41,
  `topology.edge` 27, `topology.face_p2` 21, `topology.face_p4` 5, `topology.shell` 3,
  `topology.edgeuse_p1` 3). The volume branch (`SmVolume::GlobalPointSolve` /
  `EvaluatePoint`, `surface.volume_p2`) is ported with stubs but not exercised: the shim
  has no real volume to classify to.
- **`SmSpaceBend` / `SmSpaceUnbend` type queries** are compared through the static
  `GetClassType` / `GetClassTypeString` (an instance needs the unported bend units); the
  `IsKindOf` chains are unit-tested only.

## intersect.surface_silhouette (`SmSurfaceSilhouette`, PR #45 review, 2026-10-14)

No Rust-vs-C++ disagreement: 540 of 540 non-deferred difftest cases bit-identical (709
cases, 169 deferred; `SmSurface::EvaluateNormal` and `DropVectors` now call the
`surface.surface_p7` port).

- **Planes outside their extent.** `SmPlane` evaluates linearly everywhere, so
  `ComputePointValues` / `DoesPointLieOnCurve` points and tracer domains outside the
  plane's `SmExtent2d` are valid in C++. The ffi guard only bounds them to the natural
  domain for B-splines (upstream evaluates those unclamped); six `advx_plane_outside_*`
  fixtures (parallel, perspective, eye above) cover it and match exactly.
- **Plane prev-point singularity.** `adv_plane_prev_*` fixtures view along or near a
  plane axis with `use_prev`, and `adv_plane_persp_*` put the eye in the plane or exactly
  on the surface point (`Unitize` of a zero vector gives `(0,0,1)`); all match. Tiny
  (1e-6) and huge (1e5) planes, a parabolic cylinder viewed along its rulings (parabolic
  branch, "hit singularity directly" recursion) and saddle chains with angle tolerance
  None/1e-3/1.5/0 are covered too; 3 of the 31 defer on `surface.surface_p3`.
- **`UNPORTED(surface.plane_p1)` stub (pre-existing on `main`).**
  `surface/surface_cache/p1.rs` (~line 272) stubs `SmBSplineSurface::CalculateBoundingBox`
  on a plane as `UNPORTED(surface.plane_p1)`, although `surface.plane_p1` is already
  `partial`: the inherited B-spline body re-dispatches to plane overrides the port has no
  hook for yet. It causes this unit's 6 `surface.plane_p1` deferrals; whoever wires it
  should re-run this difftest.
- **`SurfaceCache::polar_box_unstored`** rebuilds the polar box from the nearest stored
  ancestor on every query instead of storing the chain as `SmBezierAux2d::GetPolarBox`
  does. Same values (checked against `surface.surface_cache_p1` and
  `intersect.surface_tracer` difftests); only repeated work differs.

## surface.surface_cache_p3 (`SmSurfaceCache`, part 3: tree build and subdivision, 2026-10-14)

- **Patch boxes of ownerless surfaces.** The construction Bezier patches get their
  boxes from `SmBSplineSurface(pBezier, TRUE, ..)` surfaces that have no face or brep,
  so `CalculateBoundingBox` expands them by `SM_ZONE_TOL_3D`. The B-spline port cannot
  ask `SmSurface::GetFace` (unported) and would defer every case; p3 computes the boxes
  unexpanded and expands them by `ZONE_TOL_3D` itself (`patch_boxes`). When
  `surface.surface_p1` ports `GetFace`, this can go back to the plain call.
- **Planes are B-splines.** `IsKindOf(SmBSplineSurface_TYPE)` holds for `SmPlane`;
  `BuildTreeBase` takes the plane's NURB (`Plane::base_with_nurb`), and `bspline_of`
  (part 1) answers the kind test for both. Treating planes as non-B-spline gave
  bicubic approximations where upstream splits the plane's own NURB.
- **Shim crash.** `SubdivideToTolerances` on an aux-data start node without a surface
  dereferences a null `mBA_pSurface` in `TestAgainstTolerances` (segfault); the FFI
  rejects such start nodes (aux type 6 needs the surface flag and a B-spline).
- **Recursion.** `SubdivideToTolerances` recurses into both children without a bound;
  the port stops past `MAX_SUBDIVISION_DEPTH` (512) with `InvalidInput`. Generated
  tolerances are size-relative (chord >= 1e-3 size, side >= 1e-2 size, angle >= 0.05)
  so the C++ side terminates too.
- **`BuildTree` flags.** `BuildTree` turns point testing off and marks face
  containment as under construction for the build (against recursion back into
  `BuildTree`), restoring both afterwards; `build_tree` does the same around its body.
- **Wired back-edges:** `trim_srf_cache` (`BuildTree`, `FindUVNodes`, `SubdivideNode`)
  and part 1's `GetTree` (the `existing_tree` stand-in is gone; `mark_face_containment`
  calls `get_tree`).
- **Not wired (needs a design):** `geom.tree` `GetFirstBezierPatch`
  (`SmBezierPatch::GetBezierPtr`, `SmBSplineSurface::GetGwNurbPointer`), `geom.grid`
  `SmSurfaceGridElement::Init` (`GetPseudoBox`) and `surface.surface_p2`
  (`GetPolarBox`) read patch data from a bare `Tree`, but the patches live in the
  surface cache (`BezierPatchRef` is an empty enum). A hand-over like the curve cache's
  `set_curve_data` would close them. The callees are ported (`surface.surface_cache_p1`,
  `nurbs.b_spline_surface_p1`), so the call sites carry `DISPATCH GAP` comments and their
  `Unported` errors name those owning units (review of PR #39).
- **Boundary edge uses on a split (review of PR #39).** `SubdivideNode` moves a boundary
  parent's edge uses into the children through the virtual `ImplantEdgeuse`; the port
  called the base class's (always `SM_ERR`, ignored) from the trimmed surface cache too,
  so the edge uses were dropped. The trim runner printed only the input nodes, so CI did
  not see it. Now `SurfaceCache::subdivide_node_split` does the split and
  `TrimSrfCache` re-implants each edge use with its own `implant_edgeuse` (into one
  child, no subdivision); that needs `SmEdgeuse::GetUVTrimCurve`, so those cases defer
  on `topology.edgeuse_p1` (poly edges on `query.poly`). The trim runner prints every
  node of the tree; fixture `implant_then_split_boundary_leaf` covers it.
- **Build/Get not compared yet.** Every `Build` / `Get` case defers on
  `CalculateContinuities` (`surface.surface_p6`), so the first-generation node loop of
  `BuildTreeBase`, the `BuildTree` cleanup and `GetTree` have no C++ comparison until
  that unit lands.
- **Dispatch gaps.** `GetTree` dispatches `BuildTree()` virtually (the trimmed surface
  cache's on an `SmTrimSrfCache`); `SurfaceCache::get_tree` builds the base tree, and
  `subdivide_node` does not repeat upstream's `GetTree()` call (it needs a built tree).
- **Planes in the tolerance test.** An `SM_AD_AUX_DATA` node holding a plane reads the
  plane's NURB (`GetGwNurbPointer` on an `SmBSplineSurface`); the port's plane makes its
  NURB lazily, so the test uses `Plane::base_for_query`. Found once the generator put
  planes on aux start nodes (the FFI had rejected them; upstream handles them).
