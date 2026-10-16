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

After wiring `mass.integrator` (PR #119), all 3076 cases pass bit-identically with
zero deferred: the 85 remaining curved-length / arc-length inversion cases now
run. The unit is green.


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
  not ported (curve_p6's own limit, no longer tagged to surface.surface_p5): it is taken only with the
  `pOptSurfCrvOnSurf` argument the port drops, and needs `SmSurface::DropPoint`.
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
After wiring `mass.integrator`, 3095 of 3097 cases pass bit-identically. The 28
curved-span `Length` cases now pass; two gap-closing cases still reach
`SmCurve::GetNaturalInterval` through a solution handle (`UNPORTED(curve.curve_p2)`
in `geom::solution_array`).
Upstream behaviour worth knowing:

- **`CrvsCleanup` crashes (SIGSEGV)** when the curve next to a removed tiny curve is closed:
  `TrimGap` between the previous curve and a closed curve (end points equal) dies inside
  upstream (reproduced with a cubic segment, a tiny segment and a closed quadratic loop).
  The port completes the clean-up. Difftest never generates a closed curve next to a tiny
  one. `CrvsCleanup` also casts every neighbour to `SmBSplineCurve` for `TrimGap`; the port
  returns `InvalidInput` for any other kind there and the ffi only sends B-splines.
- **`Length` casts its cache curve to `SmBSplineCurve`** and integrates any curve the cache
  tessellates, Hermite curves included (the Rust cache tessellates them as upstream does),
  so Hermite `Length` uses numerical integration, rather than the end point distance
  the comment promises.
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

## surface.surface_p9

- Owns `FindTessellationSplits`, `CreateIsoBoundaries` and the CSI leaf `LocalSolve`
  body in `SmSurface.cpp:11644,13887,14877`. Default report: 778 cases (178 fixtures
  and 600 fixed-seed generated), 727 bit-identical, zero failed, 51 deferred,
  max relative error zero, no absolute floors.
- The 50 diagonal-tessellation deferrals reach curve classification in
  `intersect.curve_class_p4`. The CSI helpers now call `surface.surface_p1`;
  the disjoint fixture passes and the crossing fixture reaches `surface.surface_p6`.
  Later p4/p5/p6 callees and the identical-solution virtual remain explicit
  Unported back-edges. End-to-end intersections are not yet verified.
- **CO-PORTED(surface.surface_p1).** Reuse its existing `CsiGlobalSolver` and
  constructor, extending its state with optional borrowed curve/cache/kernel
  context for leaf solves. No duplicate solver type. `Step` surfaces explicitly
  defer their generic iso-curve construction to `surface.surface_p5`.
- Sixteen explicit real-plane cases with diagonals disabled and maximum side
  lengths including -0.5 and 0.5 pass against C++; diagonal classification still
  defers to its owning unit.
- CSI fixture caches use a nonzero chord tolerance. Cache preparation failures
  are harness failures, never matching operation errors. The reference solver
  must also return success. The surface fixture is one planar quadratic patch;
  Rust assembles its one-leaf arena because general surface tree construction is
  not available yet. Further CSI coverage must use fully equivalent constructed
  caches when those dependencies land.
- B-spline iso extraction is exact, so the approximation tolerance has no effect
  for that supported kind. Curve-bounded surface virtuals defer to surface_p5.
  The maximum-side branch retains the upstream last-sampled length for its
  equally-spaced-point tolerance, even if another sampled curve was longest.
- Input-driven tessellation recursion has an explicit depth limit of two (the
  recursive call sets maximum side length to zero). Split arrays are bounded to
  1,000,000 entries including endpoints. Invalid/nonfinite/negative derived counts
  return an error instead of converting to an unsigned count. The FFI excludes
  the all-degenerate maximum-side case where upstream's negative count conversion
  is unsafe. Two kernel tests cover analytic boundaries and excessive counts.
- Ignored non-Unported tessellation failures leave empty returned arrays in Rust;
  the C++ out-array API can retain partial arrays on failure. This deviation is
  documented in the module header. All Unported errors propagate explicitly.
  PR #6 review found an incorrect default degeneracy tolerance (1e-5 instead of
  1e-12) and dropped maximum-side arguments. A new 96-case corpus reproduced 32
  disagreements before those fixes and is bit-identical afterwards. Coverage now
  includes poles, nearly degenerate boundaries, tiny patches, subdomains, angle-only
  tessellation, negative maximum side lengths and real analytic SmPlane instances.
  The C++ guard tests the actual six sampled iso curves and rejects only the
  all-degenerate positive maximum-side case below 1 where the negative count cast
  is undefined, within the runner's documented finite input bounds.
- Wired FindTessellationSplits into SurfaceCache::SubdivideByBlock, with the caller's
  Kernel passed through SubdivideToTolerances. Cache p2 compares 808/905 cases
  (97 deferred); p3 compares 450/600 (150 deferred). No tolerances changed.
- CSI resolves the original owner handle against the supplied surface cache.
  IsClosed and tangent-plane drop now call the already ported surface_p7 operations.
  `surface_to_evaluate`, `is_node_valid` and `is_point_valid` call p1 directly.
  The single crossing fixture is deferred at its next genuine missing dependency.


- **Latest-main revolution kind.** The surface dispatcher now includes
  `SurfOfRevolution`. Borrow its underlying B-spline for implicit iso evaluation;
  use its real virtual iso constructor for extracted boundaries, retaining the
  returned circle's NURB. Eighteen canonical-cylinder comparisons pass (sixteen
  non-diagonal splitting cases and two boundary cases). This is a focused fixture
  surface, not a claim of exhaustive coverage of all revolution geometries.
  The reference owns its generator and checks `MakeNurb` setup independently.
  CSI inputs retain the previous B-spline cache construction; the FFI rejects
  revolution CSI because the one-leaf Rust fixture adapter does not model it.

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

- **Port deviation: `natural_uv_domain` does not cache the NURB (found by difftest
  `--ssi-corpus`, 2026-10-14).** Upstream `SmBSplineSurface::GetNaturalUVDomain` makes the
  plane's NURB in place (`GetOrCreateGwNurbPointer` through a const cast) and keeps it.
  `Plane::natural_uv_domain`, like the port's other read-only queries, makes it on a copy
  and caches nothing. `IntersectWithPlane`'s parallel branch (`SmPlane.cpp:2183`) returns
  "no intersection, done" when either plane has no NURB, and otherwise tests for
  coincidence and sends coincident planes to the general intersector. So after the same
  call sequence, coincident planes (corpus cases `c6_planes_identical`,
  `c6_planes_overlap`) gave no curves in Rust and four `COINCIDENT` boundary curves in C++.
  The deviation is noted in `surface::plane::p1` (and `p2`); the follow-up to model the
  cache is in this unit's `notes` in `plan/modules.toml`.

## surface.unbend_volume (`SmUnbendVolume`)

All non-deferred difftest cases agree bit for bit with upstream (`max_rel_err = 0`, 1874
default cases, a 30,374-case run clean). The 67 deferred cases are `IsCurveInParamDomainSimple`
(66: `SmCurve::HasPlaneCrossings` needs `SmSurface::GlobalCurveIntersect`, unit
`surface.surface_p8`) and `ReadFromDB` (1: base read `SmVolume::ReadFromDB`, unit
`surface.volume_p2`). No upstream test builds an unbend volume. `SmVolume::EvaluatePoint` and
`SmVolume::IsPointInParamDomain` (unit `surface.volume_p2`) were co-ported here; when
`surface.volume_p2` merged, the co-ports were replaced by its versions
(`Volume::evaluate_point`, `Volume::is_point_in_param_domain`, which also keeps the unbend's own
image when its next map fails, as below) and `ReadFromDB` now reads the base data: 1874 cases,
1808 pass, 66 deferred (`surface.surface_p8`). Upstream behaviour worth knowing (all ported as
written):
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

## surface.surf_of_revolution_p3 (2026-10-13)

- **`SurfaceKind::SurfOfRevolution`.** The surface of revolution joined the `SmSurface`
  dispatch (`surface::surface::p2`) to wire `SmSurfOfRevolution::ReadFromDB` into
  `SmSurface::ReadFromDB`. `Surface::reverse`, `swap_uv`, `reparameterize`, `transform`,
  `drop_point_fast` and `convert_uv_from_*` now take the `Kernel` (the surface's
  overrides, and the virtual `RebuildSTEPFromNURBParameters` that the inherited
  `SmBSplineSurface::Reverse` / `SwapUV` reach, need it); `intersect_with_plane` takes a
  `Plane`. `STEPInversion`, `AdjustSTEPUVDomain` and `TrimWithDomain` of the kind are
  stubbed to `surface.surf_of_revolution_p2`.
- **Surface cache iso curves.** `SmSurfaceCache` reaches the virtual
  `CreateIsoParametricCurve`, which makes an `SmCircle` in the sweep direction of a
  surface of revolution; the cache stores its iso curves as `BSplineCurve`, so that call
  stays an `UNPORTED(surface.surface_cache_p1)` stub (as the plane's `SmLine` iso
  curves do, retagged from `surface.plane_p1` in the review fixes) until the cache stores
  `Curve`s. Both are tagged with the unit that owns that storage
  (`SmSurfaceCache::GetIsoBoundaryCurves`), not with the already-partial units that
  ported the overrides, so whoever changes the storage finds them by grep.
- **Polar converter records.** A surface of revolution's `SmPolarConversion` owns its
  sweep circle, so `WriteToDB` stores it (`bHasCurve`); it is read by
  `PolarConversion::read_from_db_with_curve` (curve.ellipse_p1).
- **Cones and spheres.** `IntersectWithCone` / `IntersectWithSphere` take the other
  surface through the `Revolved` trait (its `SmSurfOfRevolution` part and its virtual
  `ConvertDomainFromNURBSToSTEP`); `SmCone` / `SmSphere` should implement it when ported,
  which turns the 44 deferred cases into compared ones.
- **Inexact polar conversions.** `ConvertUVFromSTEPToNURBS` without polar conversion and
  `DropPointFast` with an inexact angle need `SmSurface::LocalPointSolve`
  (`surface.surface_p5`). They are unreachable in both C++ and Rust, not just
  unreached: `SmPolarConversion::ConvertToNURBSParameter`
  (`PolarConversion::convert_to_nurbs_parameter`) reports an exact conversion whenever
  it succeeds, so the inexact iso-curve inversion never runs.
- **Review fixes (PR #29).** The FFI wrappers rejected valid inputs: zero-width drop,
  intersection and update domains, and zero drop / intersection tolerances. Upstream
  handles both without undefined behaviour (198 such cases: C++ does not crash, Rust is
  bit-identical), so those domains now need only be ordered and tolerances only `>= 0`;
  `Reparameterize` keeps strictly positive extents (a zero-width reparameterisation
  divides by zero in NLib). Fixtures `review_adversarial_zero_width.json` (144) and
  `review_adversarial_zero_tol.json` (54). `review_adversarial.json` (3497 cases) is a
  subset of the reviewer's 6125-case adversarial set (every non-drop case, every third
  drop case; all 6125 were bit-identical or deferred): torus / cylinder / cone / sphere /
  partial-sweep / Bezier generators, swapped and at 1e-4 and 1e5 scale, drops on the
  axis and seams, coaxial / tangent / coincident / disjoint / wrapping intersections,
  non-uniform and negative-scale transforms, rebuilds, reverse / swap / update /
  reparameterize. Its deferred cases are the `tol = None` path (`surface.surface_p1`).
- **Not reachable today.** (1) `Surface::reverse` / `swap_uv` of a surface of revolution
  use the stored NURB; upstream calls the virtual `GetOrCreateGwNurbPointer`, which would
  run `SmSurfOfRevolution::MakeNurb` for a surface without one (none exists: every
  constructed surface has its NURB). (2) `convert_uv_from_*_or_written` (callers that
  ignore the status, e.g. `UpdateAnalyticalDomain`) model `rSTEPUV` as written only after
  `Point2dMapToDomain` / `ConvertToPolarParameter` succeed; if either C++ call wrote its
  out-parameter partway before failing, such a caller would see a different value. No
  input found reaches a failure there.
- **Review fixes, round 2 (PR #29).** The FFI frame guard rejected scale vectors with any
  component below `1e-6` in magnitude, for both `Transform` and the rebuild edit. Upstream
  handles all of them without undefined behaviour: `Transform` has its own
  `cpOptScale->x < SM_EFF_ZERO` -> `SM_ERR` branch, and `SmBSplineSurface::Transform`
  accepts zero and negative components. Because of the guard, the boundary of that branch
  (`0 < s < 1e-12` against `1e-12 <= s < 1e-6`) was never compared. The guard now only
  requires finite values. `review2_scale.json` has 1092 cases: scales 1e-7, 1e-9, 1e-11,
  1e-13, -1e-7, 2e-7, zero, mixed zero and tiny components and +-1e-300, on every test
  surface at model scales 1, 1e-4 and 1e5. All are bit-identical and none crash.
  `review2_extra_adversarial.json` (4654 cases) is a subset of the reviewer's 11010-case
  second adversarial set: every case other than drops and `ToNurbs` conversions, plus every
  third drop and every third conversion. All 11010 were bit-identical or deferred. It
  covers coincident generators with an apex end, flipped axes, wrapped and partial sweeps,
  swapped surfaces, a tangent torus and cylinder, drops at seams, poles and generator
  seams with pointing vectors, out-of-range `ToNurbs` angles, tiny and huge
  reparameterisations, and near-identity transforms and rebuilds. A missing bounding box
  in `IntersectWithSurfOfRevolution` now reports the intersection's operation path.
- **`SmSurface` dispatch after `surface.surface_p7`.** `EvaluateNormal`, `IsClosed` and
  `IsPlanar` are not overridden by `SmSurfOfRevolution`, so a surface of revolution
  runs the base bodies like a B-spline surface. `IsPlanar` reaches
  `SmBSplineSurface::CalculateBoundingBox` on the surface's NURB.
  Likewise `EvaluateGeometric` / `EvaluateNormalSection` in `intersect.surface_silhouette`
  (stubbed to `surface.surface_p3`, as for a B-spline surface), and `BuildTreeBase` in
  `surface.surface_cache_p3` decomposes the surface's NURB (`GetOrCreateGwNurbPointer`:
  `MakeNurb` on a copy when it has none).
- **Review fixes, round 3 (PR #29).** The runner reused p1's `status()`, which reports
  every Rust error other than `Upstream` as `-1`. `UpdateAnalyticalDomain` on a swapped
  surface with a B-spline generator and a zero-width U domain (`[0.5, 0, 0.5, 1]`)
  reaches `SmBSplineCurve::Trim` through `AdjustSTEPInterval`, which returns
  `SM_ERR_INVALID_INPUT` (1007, "trim points too close together"); Rust returns
  `SolidError::InvalidInput` (faithful), but the case was reported as a failure. The p3
  runner now maps `InvalidInput` to 1007, as the `nurbs.b_spline_curve_p2` and
  `intersect.surface_tracer` runners do. Fixtures: `review3_update_bspline_generator.json`
  (240 update cases), `review3_adversarial.json` (5829: coaxial cone / cylinder / sphere /
  torus / disk / axis-line intersections at tolerances None, 0, 1e-15, 1e-9, 1e-3 and 10,
  off-axis and tilt sweeps up to 1e-5, 1e8 scale, transform scales at the `SM_EFF_ZERO`
  boundary, seam / axis / pole drops) and `review3b_adversarial.json` (10240: B-spline and
  arc generators on five sweeps, swapped and unswapped, every op including `read_db`
  ascii / binary v30 / v33; the 240 cases duplicated in the update fixture were dropped).
  All are bit-identical or deferred.
- **Self-tagged iso-curve stubs.** The `UNPORTED(surface.surface_cache_p1)` stubs for
  `SmPlane` / `SmSurfOfRevolution::CreateIsoParametricCurve` name a unit that is already
  `partial`, against AGENTS 4a. This is deliberate (see "Surface cache iso curves"
  above): the cache stores iso curves as `BSplineCurve`, so the `SmLine` / `SmCircle`
  results cannot be stored until it stores `Curve`s. Whoever changes that storage must
  wire both stubs.
- **Not reachable today (3).** `SwapUV` of a surface of revolution toggles the swap bit
  after the inherited base swap; upstream toggles it between `GetCanonical` and
  `SetCanonical`. These differ only if `SetCanonical` fails, which is not modelled.
- **Tiny-scale coverage (follow-up for `surface.surf_of_revolution_p1`).** p1's FFI guard
  rejects generator lines shorter than `1e-6` and circles with radius `<= 1e-6`
  (absolute thresholds), so no surface below about `1e-6` model scale can be compared
  (the reviewer's `1e-8` scale cases were rejected). If upstream handles such inputs
  without undefined behaviour, the thresholds should become relative.

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

## intersect.topology_solver_p1

- Upstream issues UB-85 (`MayContainAnswer` reads uninitialised `IntersectLine`
  results for a ray shorter than `SM_EFF_ZERO`), UB-86 (edge solvers dereference a free
  curve's missing edge) and WR-62 (`SmTLSPointVertex::SolveIt` ray firing falls through
  into the 3D signed case; still unconfirmed against the C++: even single-call fixtures
  are deferred because `add_sorted_solution` reaches the `SmTol::GetZoneTol3d` stub of
  `topology.brep_p1` for vertex objects). The port keeps the WR-62 fall-through as
  written. (Renumbered when other units took the numbers on `main` first: filed as
  UB-73, UB-74 and WR-48, then UB-77, UB-78 and WR-52, then UB-78, UB-79 and WR-55,
  then UB-80, UB-81 and WR-59, merged as UB-80, UB-81 and WR-61 while
  `surface.surf_of_extrusion_p1` held those numbers, then UB-85, UB-86 and WR-62.)
- The FFI guard refuses an option vector shorter than `1e-6` only where upstream is
  undefined or unverified: the ray of `MayContainAnswer` (`Op::May` and tree solves with
  `SM_SO_RAYFIRE`) and every vector the curve solvers hand to
  `SmCurve::GlobalPointSolve` / `GlobalCurveSolve`. Zero and tiny vectors for the vertex
  solvers and the other `MayContainAnswer` branches are defined (`smgu_LineClosestPoint`
  gives `0`, `smgu_PointProjectToPlane` NaN) and match bit for bit
  (`zero_vectors.json`, 156 cases).
- Wiring notes for later porters: the `curve_global_curve_solve` stub
  (`UNPORTED(curve.curve_p9)`) receives both solvers' intervals (`m_vInterval`,
  `m_vInterval2`, swapped under `m_bSwapOrder`) and the operation, tolerance, best
  answer, option vectors and `SM_SR_ALL` exactly as upstream passes them; the
  `face_point_classify` stub must return the class even when `PointClassify` fails
  (every caller ignores its status); `face_silhouettes` calls `surface_cache`
  (`surface.surface_p2`) through `TopologyObjects::surface`, and resolving the cache
  id to `SurfaceCache::has_cached_curves` is left for `surface.surface_p1`.
- Untested until their callees land: `SmTLSPointFace`, `sm_GetFaceSilhouettes` and
  every tree case reaching `SmTopologySolver::LocalSolve` (`surface.surface_p4`,
  `surface.surface_p8`, `intersect.topology_solver_p2`); the upstream tests only reach
  the curve and vertex paths.

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

## surface.cone_p1 (`SmCone` / `SmCylinder`, part 1) (2026-10-13)

- **Virtual calls back into the cone.** The inherited `SmSurfOfRevolution::MakeNurb`
  rebuilds the polar converter through the virtual `CreateIsoParametricCurve`, i.e.
  `SmCone::CreateIsoParametricCurve` (a circle averaged from three iso-curve points), not
  the surface-of-revolution one. `SurfOfRevolution` now takes the iso curve as a hook
  (`make_nurb_with_sweep_params_using`, `rebuild_polar_converter(k, iso)`, shared with
  `surface.torus`, which added the same hook on `main`); `Cone::make_nurb`
  passes the cone's. A unit porting `SmSphere` needs the same.
- **Owner re-entrancy in the unbounded constructor.** `pNewLine->AdjustSTEPInterval(apex,
  INF)` runs after `SetOwner(this)`: the line's `MakeNurb` asks the cone for
  `GetGenDirParamExtent`, whose `GetNaturalUVDomain` makes the cone's NURB (virtual
  `MakeNurb`, inside the constructor) and the line is reparameterised to it.
  `Cone::new_unbounded` replays these steps explicitly; `CreateCanonical` then makes the
  NURB again. Bit-identical in difftest.
- **Faithful upstream quirks** (also in `plan/upstream-issues.md`): the
  `CreateCanonical(semi-angle, pOptHeight)` bounded branch passes `FALSE` as
  `bInsideOut` and the caller's `bInsideOut` as `bMakeNurbGenCurve`; the
  `CreateCanonical(bot, top, height)` branch for `height <= 0` passes the top radius as
  the semi-angle; `sm_TotalCoincidenceChecker` compares the wrong end points for reversed
  lines and then returns `FALSE` (status `0`, not `SM_SUCCESS`); `GetApex` returns
  `SM_ERR` for a cylinder (callers use its outputs).
- **Harness guards.** `SmPolarConversion::ConvertToNURBSParameter` dereferences the polar
  curve unchecked: a cone whose converter could not be set up (e.g. a negative radius)
  would crash in `ConvertUVFromSTEPToNURBS` / `CreateIsoCircleFromPoint`; the shim answers
  `SC_ERR_ARG` for those ops and leaves `GetHeight` out of the state (it reads
  uninitialised points when `EvaluatePoint` fails). `CreateIsoCircleFromPoint` ignores
  the `STEPInversion` status and reads its height uninitialised when the generator drop
  fails; the shim replays that inversion first and answers `SC_ERR_ARG`, the runner does
  the same test. `AdjustSTEPUVDomain` with a reversed sweep interval (`[90, 10]`) never
  returns in the C++ reference (`SmSurfOfRevolution::AdjustSTEPUVDomain`, unported in
  Rust): the FFI rejects a reversed sweep there. Everything else (parallel or zero
  placement axes, zero or negative heights, semi-angles beyond 90 degrees, reversed or
  over-long bounded sweeps) is accepted and compared: upstream returns statuses or builds
  degenerate surfaces.
- **Not compared yet.** `SmSurfOfRevolution::STEPInversion` is co-ported for the planar
  generator every cone has (its non-planar branch is `Unported`, unit
  `surface.surf_of_revolution_p2`). `AdjustSTEPUVDomain` past its apex checks, `Transform`
  past its argument checks and `ReadFromDB` past the cone's own data wait on
  `surface.surf_of_revolution_p2` / `_p3` (deferred). `ReadFromDB` is unit-tested only:
  the `surface.surface_p2` shim dumps B-spline surfaces only. The `SmSurface::ReadFromDB`
  dispatch now reads the cone data and defers on `surface.surf_of_revolution_p3`.
- **Merge renumbering.** On merging with `main` the cone's UB-78, WR-55 and WR-56 became
  UB-81, WR-61 and WR-62 (units merged first had taken those ids). Those still collided
  on `main` (with `intersect.topology_solver_p1` and `surface.surf_of_extrusion_p1`), so
  PR #61 renumbered the cone's UB-80, UB-81, WR-61 and WR-62 to UB-87, UB-88, WR-63 and
  WR-64; HANG-11 is unchanged. The id lists below are historical.
- **Review fixes.** Upstream-issue ids renumbered to UB-78, UB-80, WR-55, WR-56 (UB-73 to
  UB-77, UB-79 and WR-48 to WR-54 were taken by units merged first). Deviations now also note:
  `Cone::new_unbounded` propagates an error from `GetApex` (upstream ignores its status
  and reads an uninitialised `dApexParam`); the inversion enables out-of-bounds on a clone
  of the generator where upstream's `const` `STEPInversion` sets it on the generator for
  good (same results, the flag already propagates). **Revisit with
  `surface.surf_of_revolution_p3`:** ASCII `Cone::read_from_db` leaves the radius and
  semi-angle at `UNDEF_DOUBLE` after a failed numeric extraction, where C++11 `>>` stores
  `0`; not observable while `ReadFromDB` ends in `Unported`. The reviewer's adversarial
  cases are in `fixtures/surface.cone_p1/review.json` (3167, pruned from 3607 by keeping
  one status-only off-surface inversion per point and cone instead of one per tolerance
  and guess); all pass bit-identically.
- **Second review fixes.** The cone's uninitialised-read issue is UB-80 (main took UB-79
  for `intersect.point_class`). Deviations now also note that `CreateIsoCircleFromPoint`
  propagates an error from `ConvertUVFromSTEPToNURBS`, where upstream ignores the status
  and, with `swap_uv`, uses the `x` value the polar converter already wrote; it needs a
  polar-inversion failure on a valid polar curve, which no case reaches. The second
  adversarial set is `fixtures/surface.cone_p1/review2.json` (3875 cases, pruned from
  82,910 run against the C++ with 0 failures): semi-angles -30, 89, 89.9, 90 and 120
  degrees, bounded cones with a zero radius (inside-out, swap), `make_nurb_gen`
  generators, near-cylinders, negative and 1e-6 heights, far/tiny/rotated/skewed/non-unit
  placements, seam/apex/past-apex inversions, iso circles 1e-7 off the surface,
  derivative orders up to 4, cross-class equality and reversed/partial/ellipse
  coincidence pairs. Difftest: 8794 cases, 8446 pass bit-identically, 0 failed, 348
  deferred (`surface.surf_of_revolution_p2` 283, `_p3` 65); perturbation 8446/8446.
- **Third review fixes.** `Cone::type_string` (`GetTypeString`) returned the class name;
  upstream's `SM_COMMON(cl, sc, ty)` returns `_T(#ty)`, i.e. `"SmCone_TYPE"` /
  `"SmCylinder_TYPE"`, and `GetClassString` (`#cl`) returns `"SmCone"` / `"SmCylinder"`.
  Fixed, and both strings are now part of the compared state (shim and runner). The same
  bug is still in `SurfOfRevolution::type_string` (`surface::surf_of_revolution::p1`,
  returns `"SmSurfOfRevolution"` instead of `"SmSurfOfRevolution_TYPE"`; unit
  `surface.surf_of_revolution_p1`, already merged, not changed here). Pinned-toolchain
  clippy lints in the runner's generator fixed without changing its RNG draw order. The
  third adversarial set is `fixtures/surface.cone_p1/review3.json` (4100 cases: every
  derivative order pair incl. out of range, NURB eval at knot boundaries with every flag
  combination, `to_nurbs` at u = 720 / -725 / 1e6, iso curves with reversed, zero-size and
  over-range trims, inversion and iso circles with tolerances 0..10*scale and seam
  guesses, radius-boundary heights in `adjust`, near-identity and negative-scale
  transforms, cross-mode equality, partial/reversed circle coincidence at 1e-5..1e5 radii,
  near-coincident lines). Difftest: 12894 cases, 12426 pass bit-identically, 0 failed,
  468 deferred (`surface.surf_of_revolution_p2` 403, `_p3` 65); perturbation 12426/12426.
- **Fourth review fixes.** `sm_TotalCoincidenceChecker` takes its tolerance as a
  `const SmApproxTol3d` (callers pass a `double`): the conversion takes `fabs` and maps
  `SM_USE_DEFAULT` to half the zone tolerance; `total_coincidence_checker` now applies
  `approx_tol3d`. `CreateIsoCircleFromPoint`'s ignored `ConvertUVFromSTEPToNURBS` status
  is reachable: unbounded cones with a semi-angle just above the cylinder test (`5e-11`
  degrees) have their polar circle ~5e11 below the origin and the polar conversion
  fails. With `swap_uv` upstream then uses the sweep value the converter wrote (now
  ported); without it (or with nothing written) it reads an unset value (UB-87): the
  shim and runner refuse those inputs. The fourth adversarial set is
  `fixtures/surface.cone_p1/review4.json` (7800 cases, pruned from 35,138 run against
  the C++: tiny/negative semi-angles with every swap/inside-out, zero/over-long/negative
  sweeps, zero heights, apex-crossing cones, negative and `SM_USE_DEFAULT` coincidence
  tolerances, sub-domain and closed-vs-arc circle and line coincidences). `Cone::apex`
  propagates a failed generator inversion where upstream's `GetPositivePoint` and
  `STEPInversion` ignore `GetApex`'s status (deviation noted; no case reaches it).
  Difftest: 20694 cases, 20173 pass bit-identically, 0 failed, 521 deferred
  (`surface.surf_of_revolution_p2` 456, `_p3` 65); perturbation 20173/20173.

## surface.surface_p6 (`SmSurface`, part 6; partial, no disagreements)

- **Coverage.** Every non-deferred case agrees bit for bit (`max_rel_err 0`). Most of the
  unit sits behind `surface.surface_p4`/`_p5`: `GetSampleValues` defers at `DropPoint`
  once its input checks pass, `LocalCurveIntersect` always defers
  (`SmCSIntersectENFO::Evaluate`), `SimpleCoincidenceChecker` compares only disjoint boxes
  (`GlobalPointSolve` next), `ClassifyIntersectingRangeSolution` only point solutions
  (`LocalPointSolve` next), `SmSSGlobalSolver::IdenticalSolutions` defers at the
  singularity test of non-plane surfaces, and `sm_SCLocalSolve` compares the boundary
  curve/curve solves with `bSkipNormalize` (the point solves and the curve/surface solves
  defer). `CalculateContinuities` and `CreateNaturalUVTrimCurves` compare fully on
  B-spline, plane and revolution surfaces (also true cones with their apex on the axis).
- **`CreateIsoParametricCurve` dispatch.** The base body is `surface.surface_p5`; the
  B-spline, plane (`SmLine`) and revolution (`SmCircle`) overrides are ported and are what
  both functions call, so the iso curves keep their own `IsDegenerate`, `GetKnots` and
  `CalculateContinuities`.
- **`sm_SCLocalSolve` shim.** The verbatim copy takes the evaluated surface, the patch
  surface and the node boxes instead of the tree nodes. PR #106 replaces its former
  `-2` stop marker with the real `sm_SpLocalSolve` reference shared with the p5 shim.
- **Upstream quirks kept.** `SmSSGlobalSolver::IdenticalSolutions` reads the surface
  parameters from `m_vStart[1..4]` (the solutions hold them in `[0..3]`);
  `CalculateContinuities` keeps the knots of a replaced degenerate iso curve;
  `GetSampleValues`' degenerate-tangent test scales `SM_EFF_ZERO` by `1.0 *` (not `1.0 +`)
  the position's largest coordinate.
- **Upstream crash.** UB-100: `GetSampleValues` with `SM_TT_DIR_TO_REFERENCE_CURVE` and no
  reference curve dereferences `NULL` (found by a generated case); the shim refuses it.
- **Wiring.** Surface caches now build (their tree needs `CalculateContinuities`), so
  `intersect.surface_tracer`'s runner runs `DoTrace` on the cache instead of a placeholder
  harness error, and `curve.iso_curve` has no deferred case left (green). The two
  remaining `UNPORTED(surface.surface_p6)` markers in `surface_cache::p1` and
  `surface::p7::is_planar` stood for the base `CalculateBoundingBox` of curve-bounded and
  STEP surfaces, which needs a kernel those signatures do not take: now DISPATCH GAP notes
  naming `surface.surface_p1`. Once the kernel is plumbed, the next blocker there is
  `surface.surface_p5`: the cache build's `CalculateContinuities` calls the base
  `CreateIsoParametricCurve` for these kinds (the comments at both sites say so).
- **Review fixes (PR #92, 2026-10-15).**
  - *FFI guard on revolutions.* The guard refused any generator with an endpoint within
    `1e-3` of the axis, which kept true cones (apex on the axis) away from the C++, and with
    them the degenerate-side branches of `CreateNaturalUVTrimCurves` (remove degenerate),
    `CalculateContinuities` (degenerate retry) and the revolution's degenerate-circle iso
    curve. It now refuses only a generator with both ends on the axis (no surface swept).
    The C++ neither crashes nor disagrees on the apex cases: 12 cone-apex
    `cont`/`trim` fixtures (also with the generator reversed) are bit-identical, and a
    reviewer's 4000-seed run of every op with apex generators enabled gave 4107 cases, 0
    failed. Even a generator lying entirely on the axis did not crash the C++, but it sweeps
    no surface, so it stays refused.
  - *`sm_SCLocalSolve` iso-curve dispatch gap.* `boundary_iso_curve` (`surface::surface::p6`)
    builds `SmIsoCurve` only over a B-spline; for plane, revolution, curve-bounded and STEP
    surfaces it returns `Unported` naming `curve.iso_curve` (green), because the ported
    `IsoCurve` holds an `SmBSplineSurface` and would drop the kind's evaluation overrides.
    The FFI `scls` guard ("this op needs a B-spline surface") keeps every non-B-spline
    surface from the C++, so the gap is never exercised. A plane with `process = false` or
    `skip = true` would already be comparable and is still refused. Whoever generalises
    `IsoCurve` over `Surface` should wire this site and relax the guard.
  - *`sc_local_solve` result.* It builds a fresh `Solution` from `LocalCurveSolve`'s result
    instead of copying upstream's whole `sSolution` (end values, objects). The runner
    compares type, `nvars`, value and t/u/v only, and the path defers today
    (`surface.surface_p4`): recheck it when p4 lands.
  - *`surface.plane_p1` (pre-existing, not this unit).* `SmPlane.cpp:596/600` ignores the
    status of `CreateNaturalUVTrimCurves` and goes on with whatever curves were appended;
    `plane::p1` propagates it with `?`. This differs only on SM errors (e.g. a degenerate
    line), not on `Unported`; a faithful fix needs the partial curve lists on error.

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
  `EvaluatePoint`, `surface.volume_p2`) is wired (`Volume::global_point_solve` with
  compounding, `Volume::evaluate_point`) but not exercised: the shim has no real volume
  to classify to. A found answer with an empty solution array (upstream reads
  `sSolutionArray[0]` past the end) is `SM_ERR`.
- **`SmSpaceBend` / `SmSpaceUnbend` type queries** are compared through the static
  `GetClassType` / `GetClassTypeString` (an instance needs the unported bend units); the
  `IsKindOf` chains are unit-tested only.
## surface.volume_p2 (`SmVolume`, part 2)

Difftest: 2265 cases (265 fixtures: `TestSmTransform`-derived boxes, points and inverses,
edge cases, and 210 review cases in `review_adversarial.json` over scaled, permuted,
skew and degenerate bases, partially unbounded, huge and tiny boxes and oriented chains;
2000 generated), 2263 pass, 2 deferred (`surface.unbend_volume`),
max_rel_err 0 (after merging `surface.b_spline_volume_n_lib`, whose B-spline
`IsCurveInParamDomainSimple` / `HasDiscontinuitiesSimple` the 90 formerly deferred cases
reach). A 60,000-case run was also clean. The shim
builds plain, transform, twist (component constructor), B-spline and default unbend
maps, each but the twist optionally oriented.

After `surface.unbend_volume` merged (PR #20): its co-ports of `EvaluatePoint` and
`IsPointInParamDomain` were replaced by this unit's versions, the unbend overrides
(`EvaluateBoundingBoxSimple`, `GlobalPointSolveSimple`, `LocalPointSolveSimple`) and
`SmUnbendVolume::ReadFromDB` are dispatched, and `SmUnbendVolume::ReadFromDB` reads the
base data. The shim also builds component-constructed unbends (kind `5`), the generator
puts unbends in any op, and `fixtures/surface.volume_p2/unbend.json` adds 2290 unbend
cases (frames tilted, huge, repaired, neutral distance below the tolerance, negative,
default; chained with transforms, twists and other unbends; every op; unbend database
texts). Difftest: 4555 cases, 4442 pass, 113 deferred (`surface.surface_p8`: the
unbend's `IsCurveInParamDomainSimple` needs `SmCurve::HasPlaneCrossings`), max_rel_err
0; a 62,555-case run is clean.
- **Unbend partial outputs.** `SmUnbendVolume::Evaluate` writes its own image into the
  caller's array before its next map's `Map`, like the twist (found by the 30k-seed
  run: `EvaluatePseudoBoxSimple` of an unbend followed by a B-spline whose domain the
  sample leaves).
- **Unbend box guard.** `SmUnbendVolume::EvaluateBoundingBoxSimple` uses a null or
  deleted line when a bounded box has a zero-length edge (unit `surface.unbend_volume`).
  The FFI accepts an unbend box mapping only on the caller's own box (`EvaluateBoundingBox`
  and `EvaluateBoundingBoxSimple` on map 0, the domain box of `InvEvaluatePoint` /
  `GlobalPointSolve`) with every side at least `1e-9` or unbounded; a box computed by an
  earlier map or by `MapPseudoBox` is rejected whatever its size.

After `surface.bend_volume` merged (PR #22): the bend overrides
(`EvaluateBoundingBoxSimple`, `GlobalPointSolveSimple`, `LocalPointSolveSimple`) and
`SmBendVolume::ReadFromDB` are dispatched, and `SmBendVolume::ReadFromDB` reads the base
data (the bend unit's deferred `read_db` case now passes). `SmBendVolume::Evaluate`
also writes its own image before its next map's `Map`, so `evaluate_point_written` keeps
it like the twist's and unbend's. The shim builds default and component-constructed
bends (kinds `6`, `7`), the generator puts bends in any op and bend texts in `ReadDb`,
and `fixtures/surface.volume_p2/bend.json` adds 5014 cases (frames standard, tilted,
huge, tiny and negative neutral distance, below the tolerance, repaired zero/parallel
axes, zero tolerance; chained with transforms, twists, unbends, bends and a plain
volume; every op; bend database texts; the call paths the FFI guard rejects are left
out). Difftest: 11220 cases, 10922 pass, 298 deferred (162 `curve.curve_p9`: the bend's
`IsCurveInParamDomainSimple` needs `SmCurve::IsOnPositiveSideOfPlanes`; 136
`surface.surface_p8`), max_rel_err 0; a 109,220-case run is clean. The `ReadDb` guards of
`surface.bend_volume` and `surface.unbend_volume` now apply this unit's strict text
grammar (`surface_volume_p2::check_text`), since their readers end in the base read.

Notes for later agents:
- **Partial outputs of ignored statuses.** `SmTwistVolume::Evaluate` writes the twist's
  own image into the caller's array before its next map's `Map` fails. Where upstream
  ignores `EvaluatePoint`'s status (box samplers, `IsPointInParamDomain`, gap loops) the
  point is therefore the twist's first out-space image, not the previous value
  (`Volume::evaluate_point_written`). The base `Evaluate` writes nothing on failure.
- **Undefined call paths.** `SmTransform::EvaluateBoundingBoxSimple` with a NULL
  parameter pseudo box (UB-42) is reachable from `EvaluateBoundingBox`, `MapPseudoBox`,
  `InvMapPoint`, `InvEvaluatePoint` and `GlobalPointSolve` (UB-90; it crashed the C++
  run). The port returns `SolidError::Unsupported` at the dispatch (also for a twist's
  unbounded box without a box output). The FFI guard models the call paths over the
  chain's kinds and orient flags; whether the box/line domain checks that precede them
  pass is structural (a plain `SmVolume` later in the chain fails them), except for a
  B-spline map, where the guard rejects as if the check passed.
- **`ReadFromDB`** (UB-89): a next map of `SmVolume_TYPE`, an orient map that is not a
  transform or fails to read, and a map type missing after a "has map" flag are
  undefined upstream and `SolidError::Unsupported` here. The FFI only accepts ASCII text
  in a strict token grammar (one well-formed token per value) so it
  can predict those cases. `SmTransform::ReadFromDB` and `SmTwistVolume::ReadFromDB` now
  return the whole volume (`Transform::read_from_db(db, version) -> Volume`).
- **Wiring.** `UNPORTED(surface.volume_p2)` stubs replaced in `geom::transform`
  (`EvaluatePoint` in the unaligned box branch now a function argument; `ReadFromDB`),
  `surface::twist_volume` (`ReadFromDB`), `curve::crv_in_volume` (`EvaluatePoint`,
  `ReadFromDB`) and `geom::tol` (`GetScaledZero(SmVolume)`). `geom.transform` and
  `surface.twist_volume` have no deferred cases left; `curve.crv_in_volume` keeps only
  `intersect.curve_class_p4`.

## intersect.adv_surface_intersector (`SmAdvSurfaceIntersector`, 2026-10-14)

- **Lazy polar-box build not compared.** The shim's tree reader rejects patch/head nodes
  without polar-box vectors, so the lazy-build path of `AdvSurfaceIntersector::polar_box`
  (`cache.clone().polar_box(id)`) never runs against the C++. It only matters through
  `BranchIsGaussSeparate`, which upstream `SolveBranch` never reaches (WR-70). Once
  `surface.surface_cache_p3` can build real trees, add a direct `OP_GAUSS` case on a real
  cache.
- **`FindInteriorCurves` after `LocalSurfaceIntersect` not compared.** `IsPointOnCurve`,
  `ComputePointValues`, `TraceCurve` and `FlushCurve` sit behind unported units
  (`surface.surface_p11`, `intersect.surface_intersector_p2`, `_p4`). The port propagates
  `Unported` from `ComputePointValues` where upstream `continue`s on failure; other errors
  still `continue`. When those units land, re-run the 16 deferred cases and re-check that
  branch.
- **Review fixtures.** `fixtures/intersect.adv_surface_intersector/review_adversarial_singular.json`
  holds 398 reviewer singular-point programs: saddle/saddle, bowl/bowl and saddle/cylinder
  Dupin intersections; curvatures at `EFF_ZERO_SQRT` and at a 1000x ratio; principal frames
  swapped or rotated by tiny angles; opposite normals; trace direction -1; start points on
  domain corners; the point itself or another point as the last curve node; zero previous
  derivatives; 1e-3 and 1e3 scales. The reviewer's generator made 400; the 398 kept are the
  ones it accepted as defined in the C++. All 398 pass bit-identical.

## surface.plane_p2 (`SmPlane`, part 2, review fixes 2026-10-14)

- **Difftest runners must convert a raw tolerance the way the shim does.** The shims read
  `IntersectWithPlane` / `GlobalSurfaceIntersect` tolerances into an `SmApproxTol3d`, whose
  `double` constructor stores `fabs(tol)` and maps `SM_USE_DEFAULT` (9876543.21) to
  `s_ZoneTol3d / 2` (`SmTol.cpp`). The library functions take an already-converted
  `SmApproxTol3d` (non-negative by type), so the runner applies
  `geom::tol::approx_tol3d(k, t)` first. Before this fix, plane z=0 against plane x=1 over
  [-5,5]x[-3,3] with tol=-1 gave the C++ crossing line (44 values) but no curve in Rust
  (7 values: `TrimPlaneLineToPlaneDomain` saw a negative tolUV); tol=9876543.21 gave 44 vs
  34 values. The generator only produced positive tolerances. The same conversion is now
  applied in the `surface.surf_of_revolution_p1` runner, whose FFI guard used to reject a
  negative plane tolerance (now accepts any finite value, as the C++ does).
- **`MakeNewPlaneEllipse` with a zero radius** appends the degenerate line curve to
  `rCrvs` even when it is `NULL`; the port drops the `None` (documented inline). No input
  in the difftest reaches it (unreachable in practice).

## surface.surf_of_extrusion_p1 (`SmSurfOfExtrusion`, part 1, 2026-10-13)

PARTIAL: 973 difftest cases (173 fixture, 800 generated), 880 bit-identical, 93 deferred
(52 `STEPInversion` of non-perpendicular generators, `SmBSplineSurface::STEPInversion` ->
`surface.surface_p1` solves; 41 `sm_IntersectGenerators` of parallel sweeps,
`SmCurve::GlobalCurveSolve` -> `curve.curve_p9`). The shim builds B-spline, `SmLine`,
`SmCircle`, `SmEllipse`, `SmHyperbola` and `SmParabola` generators and every orientation (`SM_CO_LINEAR`,
`_PERPENDICULAR`, `_INDEPENDENT`, `_COPLANAR`, `_NOT_PLANAR`) is reached.

- **Generator owner step.** The C++ constructors make the surface its generator's owner.
  `SmLine::MakeNurb` (run by `AdjustSTEPInterval` when the interval changes and the line has
  a NURB) then reparameterises the line's NURB to the owner's `GetGenDirParamExtent`.
  `AdjustSTEPUVDomain` with an unswapped surface keeps that (its `Reparameterize` call
  returns early because it compares the new STEP domain with the equal NURB domain), so the
  rebuilt surface's generator knots are the old interval's. The Rust curves only hold an
  `ObjectRef` owner, so the `UNPORTED(surface.surf_of_extrusion_p1)` stubs in the line,
  ellipse, hyperbola and parabola `MakeNurb` stay (as for `surface.surf_of_revolution_p1`);
  `SurfOfExtrusion` leaves its generator unowned and applies the line step itself
  (`adjust_gen_step_interval`; disabling it fails `linear_adjust_owner_step` and 5 generated
  cases). `SmHyperbola` and `SmParabola` take the same step at the end of every `MakeNurb`
  (nothing overwrites it), so the surface applies it for them too (without it, `adjust` of
  a hyperbola or parabola extrusion disagrees: 31 of 6000 generated cases, rel. err ~1). The ellipse's owner step is overwritten upstream by `AdjustSTEPInterval`'s own
  `EditParameterization`, so it is not repeated (one affine reparameterisation instead of
  two: the knots then agree within rounding).
- **Curve solve handles.** The generator solves (`GlobalPointSolve`, `GlobalPointSolveSTEP`,
  `GlobalCurveIntersect`) name the generator with a handle that answers
  `GetNaturalInterval`; without it the solution merge keeps near-duplicate minima (a tiny
  scale `DropPointFast` returned 2 solutions against upstream's 1).
- **Upstream defects** (in `plan/upstream-issues.md`): `EvaluateSTEP` order 5 of a B-spline
  generator smashes the stack (UB-80, confirmed; the FFI rejects it), `sm_IntersectGenerators`
  reads unset offsets (UB-81), constructor / copy hazards (UB-82), the unused clamp in
  `EvaluateSTEP` / `EvaluateSTEPPoint` (WR-61, ported as is).
- With a full-circle `SmCircle` generator a seam point gives one solution, so `STEPInversion`
  reports `SM_LT_INTERIOR`; the U-seam branch is reached with a closed tangent-continuous
  B-spline (fixtures `closed_seam_*`), as are `DropPointFast`'s in-pointing seam skips.
- **Review fixes (PR #28).** The hyperbola / parabola owner step above was missing (only
  `SmLine` had it): the shim then built no hyperbola or parabola generator, so the
  `CurveKind::Hyperbola` / `Parabola` branches were never compared. The shim, FFI and runner
  now build both (88 adversarial fixtures in `review_extra_cases.json`). The runner mapped
  `SolidError::InvalidInput` to `-1` instead of `SM_ERR_INVALID_INPUT` (1007): 14 of 6000
  seeds failed falsely on directrices of a zero extrusion vector. After the fixes 6000
  seeds give 0 failed (max rel. err 2.1e-13, ellipse rounding).
- **Lazy NURB.** Upstream's `const` `ConvertUVFromSTEPToNURBS`, `ConvertUVFromNURBSToSTEP`,
  `DropPointFast` and `GetGenDirParamExtent` make a missing NURB; the Rust `&self` methods
  cannot. The conversions and `DropPointFast` now return `SM_ERR` without a NURB (they used
  to read a `(0, 0, 0, 0)` domain silently); `gen_dir_param_extent` still reads it as is.
  The shim always calls `MakeNurb`, so this is not compared.
- **Not yet compared.** `sm_IntersectGenerators` (all deferred on `curve.curve_p9`) is
  checked only by reading; it needs a real difftest once that unit lands. The FFI's guards
  on `v0 == v1` constructor domains hides inputs the C++ handles (both sides then fail the
  build; not relaxed).
- **Second review (PR #28).** `SmHyperbola` / `SmParabola::GlobalPointSolveSTEP` start with
  `AdjustSTEPInterval(crInterval)`; with out-of-bounds evaluation on, `STEPInversion` passes
  the generator's maximum analytic domain, so the owned generator is rebuilt and its
  `MakeNurb` owner step reparameterises the NURB to `GetGenDirParamExtent`. The port solved
  on an unowned copy without that step: the solve converged elsewhere within tolerance
  (STEP u rel. err up to 9.4e-3 near `u = 0`, 3.8e-10 in the generated cases). The copy now
  takes the owner step before the solve (bit-identical after). Upstream leaves the owned
  generator adjusted to the solve interval after this `const` call; the port does not.
  The FFI's circle / ellipse radius and line length guards (`<= 1e-6`) hid valid tiny
  generators (radii down to `5e-324` agree); they now reject only non-positive sizes.
  Tiny hyperbolas and parabolas (`a`, `b`, `f` of `1e-7` and below) crash the C++ build
  (segfault), so their `1e-6` guards stay. 1082 cases in `review2_extra_cases.json`.

## intersect.ray_tracer: `surface_leaf` vs upstream's `SE` on degenerate patches (fixed)

- **Unit / code:** `intersect.ray_tracer`, `surface_leaf` (`SmSurfaceGridElement::Init`,
  `SmGrid.cpp:252-290`).
- **Inputs:** (a) a bilinear Bezier patch with all four control points at (5,5,5);
  (b) a patch with `P00 == P10` and `P01 == P11` (du == 0 everywhere). Fixtures
  `degenerate_patch_all_corners_equal` and `degenerate_patch_du_zero`.
- **C++:** `SE(Evaluate)`, `SE(Unitize)` and `SE(EvaluateNormal)` signal the error and
  keep going, so `FireRay` returns a normal record (no error).
- **Cause:** asking for (1,1) derivatives makes `SmBSplineSurface::Evaluate` take its
  "not enough derivatives" branch, `SER(Evaluate(crUV, 2, 2, ...))`
  (`SmBSplineSurfaceNLib.cpp:8335`), which returns before the fixed-up tangents are
  written back, so `sEval` keeps the plain NLib derivatives. That is what the ported
  `evaluate` gives with `non_zero_tangents: false`. (The first version of this entry
  wrongly said the match had to wait for `surface.surface_p8`.)
- **Fix:** `surface_leaf` retries a failed `evaluate` with `non_zero_tangents: false` and
  keeps the cross product when `evaluate_normal` fails; `Unported` still propagates.
  The leaf's mid values are not compared directly by difftest; only the ray record is.
- **No remaining deviation:** upstream `SmSurface::EvaluateNormal` sets `(0,0,0)` and
  returns `SM_SUCCESS` when every attempt fails (`SmSurface.cpp:11459-11462`), and the
  port's `base_evaluate_normal` does the same, so the leaf's failure arm is effectively
  unreachable (only `Unported` errors).

## intersect.surface_intersector_p2 (`SmSurfaceIntersector`, part 2, 2026-10-14)

- **Gap walk bound.** `ClassifyBoundaryStartPointIntersection` walks the gap function
  with steps growing by 5 until a sample leaves a domain. With a zero UV tangent on both
  surfaces the samples never move until `delta` overflows to `inf`; then `0 * inf` is
  NaN and `ContainsPoint2d` treats a NaN point as inside, so upstream never ends. The
  port stops with `SM_ERR` after `MAX_GAP_SAMPLES` (2000) samples. The shim's
  `is_rejected_classify` rejects every point with both UV tangents zero (or a non-finite
  UV value), so the early returns before the walk (tangent point, both steps leaving the
  domain, tangent-curve / singularity / coincidence points off a tangent natural
  boundary) are not compared for such points. Narrowing the guard to the walk itself
  would need the shim to replay the pre-walk classification; not done. A review run
  with the zero-tangent part of the guard disabled finished normally (46 more classify
  ops agreed, 9 more deferred, the C++ never hung on those inputs), so the guard is
  wider than needed; the pre-walk branches are already covered by points with non-zero
  tangents, so this costs input coverage, not branch coverage.
- **DoBoundaryPlaneTest Tangent/Coincident unreachable.** The interval branch only
  produces `SM_TC_CROSSING` in the difftest: with the base intersector, endpoint point
  types of `TangentCurve` or higher cannot be reached (`ComputeSingularityPoint`
  returns `SM_ERR`), so the Tangent and Coincident classifications are uncompared. They
  are unreachable here, not skipped by a guard.
- **Re-run when dependencies land.** `ComputePointValues` step 7 (the "found" branch
  and the else branch) and the gap walk past its first in-domain sample are fully
  deferred on `surface.surface_p3` and `surface.surface_p5`; re-run this unit's
  difftest when those units become green/partial.
- **NaN curve intersection tolerance.** `DoBoundaryPlaneTest` with a zero tolerance and
  an empty box divides 0 by 0. The C++ and the Rust agree on the result (fixture
  `adv_graze_nan`), so the generator no longer excludes it.
- **FlushCurve not guarded.** Curve parameters that do not increase reach
  `SmBSplineCurve::CreateCanonical`, whose `N_IgesValidate` rejects decreasing knots
  with `SM_ERR` (equal knots pass); both sides agree, so the shim runs every `Flush`
  (the earlier `is_rejected_flush` hid about a fifth of the generated flushes and the
  whole `sm_FlushCrv` error path).
- **Input bound.** The FFI accepts finite inputs up to `MAX_COORD = 1e9` (as other
  units do), so huge-scale inputs beyond 1e9 are not compared.
- **EvaluateNormal wired.** With `surface.surface_p7` partial on `main`,
  `surface_evaluate_normal` now calls `Surface::evaluate_normal` (B-spline, plane and
  curve-bounded over either basis); the 77 cases previously deferred on it are compared
  and pass. `SmCurveBoundedSurface` does not override `IsSingularity`, so a curve-bounded
  surface runs the base `SmSurface::IsSingularity` and stays deferred on
  `surface.surface_p5` (correctly, not an over-deferral).

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

## surface.surf_of_revolution_p2 (`SmSurfOfRevolution`, part 2)

- `CreateOffsetSurface` merges the near-duplicate axis intersections through the solutions' curve objects (their natural intervals), so the port names the two curves with handles that carry their intervals ([`CurveHandle`]); without them 5 point solutions stay unmerged and the offset generator is not trimmed at the axis.
- `BSplineSurface::create_surf_of_revolution` with a given sweep circle ignores the status of the circle's `Transform` (it refuses `|scale| < SM_EFF_ZERO` before changing anything, so the untransformed copy is used).
- Upstream quirks recorded in `plan/upstream-issues.md`: WR-67 (`GetControlPolygon` zeroes the weights a polar conversion then reads) and WR-68 (`IsNurbSurfaceSurfOfRevolution` returns a status as `TRUE`).
- `Surface::global_surface_intersect` for a surface of revolution dispatches to `SurfOfRevolution::global_surface_intersect`, which falls back to the free `base_global_surface_intersect(this, ..)` (shared with `surface.plane_p2`, PR #40).

## surface.surface_p1 (`SmSurface` part 1 and `SmSTEPSurface`, 2026-10-14)

- **Shim guard.** The base `GetDomainWithContinuity` dereferences a `NULL` surface cache
  when none can be built (upstream-issues UB-83). The shim checks
  `smsurf_GetSurfaceCache` first and reports `SM_ERR`, which is what the Rust port returns
  (the cache error). The check builds and stores the cache, so the call that follows uses
  the same cache.
- **B-spline back-edges.** `SmBSplineSurface::IsDegenerateCurve` and
  `IsDegeneratePoint` (sub-domains) pass the call to the `SmSurface` base body; the
  B-spline methods run it on a copy wrapped as a `Surface`, which has no owner (the owner
  is held by the `Surface` wrapper). `CalculateBoundingBox` grows its boxes by
  `GetFace() ? face tolerance : SM_ZONE_TOL_3D`: a bare B-spline uses `SM_ZONE_TOL_3D`,
  and `Surface::calculate_bounding_box` returns `Unported(topology.face_p2)` first for a
  B-spline or plane with a face.
- **trim_srf_cache face cases.** They used to defer through the `GetFace` stub of the
  B-spline bounding box. With it wired, the Rust side ran without a face and returned
  `SM_ERR_NULL_POINTER` where the C++ side had a brep face. The runner now defers them
  on `SmBrep::CreateFaceFromSurface` (`topology.brep_p8`), the step the Rust side lacks.
- **Not wired**, still `UNPORTED(surface.surface_p1)`: `Grid::expand_hierarchy` (needs a
  surface handle type for the grid elements), the surface cache's
  `CalculateBoundingBox` of curve-bounded and STEP surfaces (called while that cache is
  being built), and the `SmIsoCurve` / `SmCrvOnSurf` readers of non-B-spline surfaces
  (those curves hold an `SmBSplineSurface`).

## topology.edgeuse_p1 (`SmEdgeuse` / `SmFaceuse` part 1, 2026-10-14; partial, no disagreements)

- **B-rep import.** No Rust B-rep reader or builder exists yet (units `topology.brep_*`,
  `topology.proto_topology_*`). The shim builds real `SmBrep`s (15 upstream `.smb` files
  through `SmBrep::ReadFromFile`, optionally with `RebuildUVTrimCurves`; wire edges and
  faces from B-spline curves/surfaces and `SmPlane`s; the wire network of prog_test
  `my_test_topology`) and exports the graph in traversal order from raw members
  (pointer-to-member access for the protected ones), never through the functions under
  test. The runner imports it into `topology::Brep` and compares the ported functions'
  results as traversal indices. UV trim curves are imported exactly: they are often
  `SmLine`s (upstream converts linear UV curves), whose evaluation differs from a plain
  B-spline's by an ulp (`Mouse.smb` gave `0` vs `2.2e-16` before lines were imported as
  `Line`s, via `Line::from_parts`). Edge 3D curves of other kinds are imported as their
  B-spline part; they are only evaluated before `SmSurface::DropPoint`, which is deferred.
  Wire that import exactly when `surface.surface_p5` lands.
- **Not wired**, still `UNPORTED(topology.edgeuse_p1)`: `surface::trim_srf_cache`
  (`SmEdgeuse::GetUVTrimCurve` of the cache's edge uses), `intersect::point_class`
  (`SmEdgeuse::GetBrep`, `GetEdge`) and the `SmTol` object dispatch in `geom::tol`. Those
  callers hold opaque object handles and no `Brep`, so the edge use record cannot be
  reached; wiring needs them to carry the B-rep (or a resolver) first.
- **Co-ports.** The use records (`Region`, `Faceuse`, `Loopuse`, `Edgeuse`, `Vertexuse`),
  the new skeleton fields and the small accessors of `topology.edge`, `topology.loopuse`,
  `topology.vertexuse`, `topology.shell` and `topology.proto_topology_p1` are minimal; an
  owner's intrusive circular list is a `Vec` in list order from its head, and an edge use's
  `m_pNext` / `m_pLast` are its neighbours there. Their porters extend these (see the
  CO-PORTED notes in `plan/modules.toml`).
- **Review fixes (PR #55).** FFI guards widened to what the C++ handles: values up to
  `1e300` (was `1e6`) and `uv` within 100 domain lengths of a B-spline surface's domain
  (plane `uv` up to `1e6`). Every previously rejected huge-scale or far-out-of-domain case
  tried (points to `1e200`, nets scaled `1e6`/`1e8`, `uv` 50 domains out) agrees exactly.
  The Seam case generator copied curve 0's knots onto curve 1 of a different degree
  (invalid curve; `--seeds 6000` case `gen5286` was a harness error); it now requires
  equal degrees.

## surface.surf_of_extrusion_p2 (`SmSurfOfExtrusion`, part 2, 2026-10-14)

- **Not a `SurfaceKind` yet.** `SurfOfExtrusion` does not join
  `surface::surface::SurfaceKind`: every `SmSurfOfExtrusion` override of a `SmSurface`
  virtual (`AdjustSTEPUVDomain`, `Reparameterize`, `Transform`, `TrimWithDomain`,
  `STEPInversion`, `DropPointFast`, the UV conversions, `MakeNurb`) needs a `&Kernel`
  (generator edits and solves), and the `Surface` dispatchers in `surface::surface::p2`
  take none. Adding the variant means threading a `Kernel` through those dispatchers and
  their callers (a cross-unit refactor), so this unit ports `GlobalSurfaceIntersect` with
  an explicit `IntersectTarget` (plane, cone, surface of extrusion, or any `Surface`)
  and `Surface::read_from_db` reads the whole `SmSurfOfExtrusion` record
  (`SurfOfExtrusion::read_from_db`, stream consumed and errors as upstream) and then
  returns `Unported { upstream: "SurfaceKind::SurfOfExtrusion", unit:
  "surface.surf_of_extrusion_p2" }` (a `DISPATCH GAP`, no `UNPORTED` marker); the
  2 `topology.brep_data` cases reading `Fillet/eg1_srf1.smb` and
  `Fillet/blade_fillet_fails.smb` stay deferred on it. Whoever threads the `Kernel`
  through the `Surface` dispatchers should add the variant (B-spline queries go to
  `SurfOfExtrusion::base`, the overrides above to the p1/p2 methods) and wire that stub.
- **Pass a curve handle to `GlobalPropertyAnalysis`.** The global solver's duplicate test
  (`IdenticalSolutions`) widens its parameter tolerance by `1e-4` of the objects' natural
  intervals. With `PropertyAnalysisOptions::handle = None` the solutions name no object,
  the interval term is 0, and near-duplicate hits upstream merges (a few `1e-9` apart in
  the parameter) all survive: `IntersectWithPlane` then made extra directrices. Callers
  must pass a handle whose `SolutionObject::natural_interval` is the curve's interval
  (here `GenHandle`).
- **`IntersectWithCylinder` and unbounded cylinders.** Upstream makes the other generator
  with `CreateIsoCircleFromPoint` at the bottom of the cylinder's heights; for an
  unbounded cylinder (or a zero tolerance) the cone's STEP inversion leaves its outputs
  unset and upstream reads them (`plan/upstream-issues.md` UB-93). The shim replays the
  inversion (as the `surface.cone_p1` shim does) and refuses those inputs; the generator
  only makes bounded `SmCone`s with tolerances it can meet. `GlobalSurfaceIntersect` only
  special-cases `SmCone_TYPE`, so an `SmCylinder` always goes to the general intersector.
- **`GlobalSurfaceIntersect` difftest.** When the special case needs more (or the other
  surface has none) upstream runs `SmAdvSurfaceIntersector`; the shim does not run it
  (the Rust side is deferred on `intersect.adv_surface_intersector`) and answers `-3`.
- **State of a found `IsNurbSurfaceSurfOfExtrusion` surface.** Its `Reparameterize`
  returns early (no NURB made) when the domains already agree; upstream's state queries
  (`GetGenDirParamExtent`) would make the NURB lazily, so the runner records whether the
  NURB exists, makes it on both sides, then compares the state.
- **Review (PR #57): `sm_IntersectGenerators` status.** Upstream `IntersectWithCylinder`
  and `IntersectWithSurfOfExtrusion` ignore the status `sm_IntersectGenerators` returns
  (keeping whatever outputs and needs-more flag it set); Rust propagates it with `?`. Not
  reachable today: every failure past the parallel test goes through the unported
  `GlobalCurveSolve` (`curve.curve_p9`). When `curve.curve_p9` lands, swallow non-`Unported`
  errors there (keep the partial curves/types and the needs-more flag), which needs
  `intersect_generators` (`surface.surf_of_extrusion_p1`) to hand back its partial result
  on failure.
- **Review (PR #57): generator record coverage.** The FFI `check_record` accepts only
  generator records of type `SmBSplineCurve` (14033). Upstream `WriteToDB` writes the
  generator's own type, and `SmLine`, `SmEllipse`, `SmCircle`, `SmHyperbola` and
  `SmParabola` records are valid too. `ReadFromDB` does not branch on the generator kind and
  built surfaces cover every kind, so this is a coverage gap only; extending the
  `topology.brep_data` Sim to line and conic records would close it.
- **Review (PR #57): self-tagged stub.** The `UNPORTED(surface.surf_of_extrusion_p2)` stub
  in `Surface::read_from_db` named this (now `partial`) unit, so a back-edge grep would
  never pick it up again. It is now a `DISPATCH GAP` (as for `SmCone` / `SmTorus`, the
  reader is ported and the error still names this unit, so the 2 `topology.brep_data`
  cases stay deferred). Its real owner is whoever threads a `Kernel` through the
  `Surface` dispatchers; raised in `plan/STATUS.md` Open issues for the coordinator.

## surface.surf_of_revolution_p3 merge with surface.torus, surface.cone_p1, surface.surface_p1 (2026-10-14)

- **Back-edges wired.** `SmTorus::Transform` / `SmCone::Transform` finish in
  `SurfOfRevolution::transform_using`, whose polar-converter rebuild takes the derived
  class's iso-curve maker (upstream's virtual `CreateIsoParametricCurve`). This follows
  #23's `SweepIsoFn` API. `SmTorus::ReadFromDB` and `SmCone::ReadFromDB` read the parent
  record with `SurfOfRevolution::read_from_db`. `Cone::transform` now takes `&Kernel`.
- **Dispatch gap.** `Surface::read_from_db` reads `SmCone` / `SmCylinder` / `SmTorus`
  records in full but still returns `Unported` naming `surface.cone_p1` / `surface.torus`,
  because neither kind is a `SurfaceKind` variant yet. Whoever adds those variants (with
  their `SmSurface` overrides) should return the surface there.
- **`SurfaceKind::SurfOfRevolution` in surface_p1's dispatch.** `EvaluateSTEP(Point)`
  dispatches to the override. `EvaluateSimple`, `CalculateBoundingBox`,
  `IsDegeneratePoint/Curve` and `sm_CrossedSeamOrPole` use the inherited
  `SmBSplineSurface` code on the NURB. `Surface::convert_domain_*` now take `&Kernel` and
  return the conversion errors (`SER` upstream). `ConvertDomainFromNURBSToSTEP` has one
  body (`surface::surface::p1::convert_domain_from_nurbs_to_step_with`), shared with
  `SurfOfRevolution`. A reversed `u` interval is `InvalidInput`
  (`SmExtent2d::SetMinMax` returns `SM_ERR_INVALID_INPUT`), where surface_p1 had mapped
  it to `SM_ERR`.
- **Zone tolerance.** With surface_p1's bounding box, `IntersectWithSurfOfRevolution`
  without approximation tolerance no longer defers. The NURB has no owner, so no face
  tolerance applies.

## surface.surface_p5 (PR #50): stubs that cannot be wired yet (2026-10-14)

- **`SmIsoCurve` over non-B-spline surfaces.** `IsoCurve` holds a `BSplineSurface`, so
  `Surface::create_iso_parametric_curve` (the `SmSurface` body, used for curve-bounded,
  STEP and the other non-overriding kinds) and `sp_local_solve`'s boundary curves return
  `Unported` (`curve.iso_curve`) for those kinds. Upstream builds the iso curve over the
  surface itself; fixing it means `IsoCurve` holding a `Surface` (a change to
  `curve.iso_curve` and its users).
- **Plane iso curves.** Upstream's `SmIsoCurve` over an `SmPlane` evaluates through the plane's analytic
  `Evaluate`; the Rust `IsoCurve` holds the plane's NURB, and the two differ in the last bits (a
  difftest of `sm_SpLocalSolve`'s boundary solves on planes disagreed at 1e-9 on values that converge to
  zero). `iso_curve_of` therefore defers on planes (`curve.iso_curve`) until `IsoCurve` holds a `Surface`.
- **Not wired (no stub tagged to surface.surface_p5 is left):** the `SmCrvOnSurf` branch of `curve_p6` needs the dropped
  `pOptSurfCrvOnSurf` and `pOptCrvOrientation` arguments of `DropPoint`; `SmSurface::DropPoint` and
  `LocalPointSolve` of the generic callers (`curve::curve::p2`, `nurbs::b_spline_curve_n_lib`) go through
  `SurfaceEvaluate::{drop_point_minimize, local_point_solve_minimize}`.
- **Kernel arguments.** PR #106 threads the caller's kernel through `Surface::drop_point`,
  its fast/local/global solves and their callers. `Surface::cover_other_coincident_surface`
  still calls `convert_domain_from_*` with a default `Kernel`.

## intersect.curve_class_p1 (`SmCurveClassification`, part 1, PR #47 review, 2026-10-14)

- **CI red on clippy with the C++ feature.** `cargo clippy --workspace --all-targets
  --features difftest/cpp -- -D warnings` reported 11 errors in the runner
  (`crates/difftest/src/units/intersect_curve_class_p1.rs`: identical match arms in
  `gen_tol`, needless range loop and unchecked indexing in `gen_script`, `range_plus_one`,
  `default_trait_access`). A plain `cargo clippy` does not build the `cpp`-gated
  runner, so run clippy with `--features difftest/cpp` too. Fixed without changing the RNG call order: the
  generated cases are identical in name, value count and outcome.
- **UV trim curves only reached by fixtures.** The generator always passes `uvc: []` to
  `xsect_curve`, so `SetIntersectionCurveThroughPoint` with UV trim curves is covered
  only by `fixtures/intersect.curve_class_p1/adversarial.json` (75 cases: UV-curve,
  huge/tiny-scale and out-of-range-UV xsect/apply cases, analytic Line/Circle/Ellipse/
  Hermite source curves, a container case past `MIN_ARRAY_SIZE`). At unit and huge scale
  the UV-curve cases defer on `SmSurface::LocalPointSolve` (`surface.surface_p5`, 20
  cases); only the tiny-scale ones are compared until that unit lands.
- **SmTol::MapTo2d for a UV curve (second review).** `sm_GetXSectTol2dOr3d_ForClassifyPair`
  case 1 (2D curve with `m_cpSurface1`) was stubbed `UNPORTED(surface.surface_p1)` after
  that unit became partial. Its UV-curve branch is now ported as the public
  `surface::surface::p1::map_to_2d_on_curve` (co-port of geom.tol's overload;
  `SmCurve::Evaluate` then `map_to_2d_dir`,
  through `SmTol2d` and `SmZoneTol3d` as upstream); only a curve of another dimension
  still needs `SmSurface::DropPoint` (`surface.surface_p5`, unreachable from the classify
  pair). Covered by `fixtures/intersect.curve_class_p1/review_tol_pair.json` (37 cases:
  flat, curved, pole-degenerate, huge and tiny B-spline surfaces and a plane; zero-tangent,
  collapsed-edge, out-of-bounds and Hermite UV curves; USE_DEFAULT, negative, 0 and 1e3
  tolerances; in- and out-of-domain params), and half the generated TolPair ops now use a
  fresh UV curve on a surface. All pass with max_rel_err 0.
- **DISPATCH GAP: SmCrvOnSurf over a non-B-spline surface.** `cull_or_refine_cci_in_3d`
  (`sm_CullOrRefineCCIIn3d`) builds an `SmCrvOnSurf` over the classification surface, but
  `curve::crv_on_surf::CrvOnSurf` holds only a `BSplineSurface`, so a plane (or any other
  surface) returns `Unported { unit: "curve.crv_on_surf" }` (24 deferred cases, all
  Cull on planes). There is no `UNPORTED(..)` marker, as in `curve/crv_on_surf.rs`:
  whoever generalises `CrvOnSurf` over `Surface` should grep `DISPATCH GAP` and wire this
  site. It was tagged `surface.surface_p1` before, which is partial and not the gap.
- **Not compared.** The shim never builds an `SmSurfOfRevolution` or `SmSurfOfExtrusion`,
  so the `SurfOfRevolution` arm of `out_of_bounds()` (the base-flag-only
  `SmTemporaryChangeValue`) is ported but not difftested; planes and B-spline surfaces are.

## query.poly (`SmPolyBrep` and its poly objects, 2026-10-14)

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

## intersect.surface_intersector_p4

- **No virtual dispatch in the intersector parts.** `SmSurfaceIntersector`'s
  `TraceCurve` / `DoPointIntersection` reach `ComputeNextPoint`, `ComputePointValues`
  (which calls `ComputeSingularityPoint` for parallel normals), `FlushCurve` and
  `TestSpanAccuracy`, all virtual upstream. Parts 1-4 port them as methods of the basic
  `SurfaceIntersector`, so they always run the basic versions. `SmAdvSurfaceIntersector`
  overrides `ComputeSingularityPoint` (and `SmFilletIntersector` several more), so the
  part 4 functions cannot replace the advanced intersector's `TraceCurve` stub
  (`intersect/adv_surface_intersector.rs`) or `SmSurface::PointBasedSurfaceIntersect`'s
  `DoPointIntersection` (`surface/surface/p2.rs`, run on an `SmAdvSurfaceIntersector`).
  Those stubs stay `UNPORTED(intersect.surface_intersector_p4)` until the parts take a
  trait for their virtual calls.
- **Real-value coverage is narrow until `surface.surface_p11`.** The basic intersector
  refines every crossing, tangent-curve and singular point with
  `SmSurface::LocalSurfaceIntersect`, and the basic `ComputeSingularityPoint` fails
  (`SM_ERR`) for parallel normals. So on planes the traces that run without deferral
  start at raw points of the types it does not refine (unknown, tangent, coincident,
  touching), and `DoPointIntersection` on coincident or parallel planes fails in its
  `ComputePointValues` (`SM_ERR`, compared with all evaluated point values).
  `FindInteriorCurves` always defers (global surface solve).

## intersect.topology_solver_p2 (PR #60, 2026-10-14)

- **Scope.** All 15 owned functions have Rust bodies: the shared VertexEdge and
  EdgeEdge constructors reuse p1, followed by local traversal, six local solves,
  five public B-rep/shape entrypoints and the ordered tree-set helper. Traversal
  preserves list order, pair culling and the child-one subdivision test. The local
  solver state is restored on errors. No numerical tolerance has changed.
- **Default corpus.** 2,429 cases: 600 generated traversal cases, 2 traversal
  fixtures, 180 CurveVertex cases and 1,647 static/face/operation/request/tolerance/scale/subdivision
  cases. 2,384 compare exactly; zero fail; 45 defer (surface_p11 20, surface_p4 8,
  surface_p6 8, surface_p8 4, topology.brep_p1 2, topology.edge 3). Counts include
  rejected-operation and empty-shape cases, not only geometric solutions.
- **Concrete reference construction.** Static B-rep fixtures use real native
  `MakeShellVertex` bodies; face fixtures use `CreateFaceFromSurface`, and EdgeFace
  uses `CreateWireEdgeFromCurve`. The Rust object adapter independently supplies
  equivalent geometry and cache tree views. Native tolerances are also compared.
  These adapters do not port SmBrepCache or nonempty SmShape construction.
  Native preflight tests require construction to succeed even when a Rust case
  defers. Setup failure is a harness failure, never a deferred/pass result.
- **Point-tree storage.** The shared p1 tree fixture uses managed nodes on both
  sides. A separate native probe finds zero entries in `GetObjectList` for the
  external-node constructor and one for the managed-node constructor. The old
  inconsistent fixture could skip every C++ comparison in SolveTwoTrees.
- **Node metadata.** SmSolution has five node slots although the global traversal
  supports three trees. Static fixtures compare all five presence flags, never
  pointer addresses or dereferences of temporary native query-tree nodes.
  The added 1,080 static cases use 12/11 and 40/30 vertices, scales 1/1e3/1e6
  and tolerances 2/50 to exercise subdivision. Presence flags do not certify
  arbitrary multi-node B-rep node identities.
  Generated traversal cases use the shared p1 protocol.
- **Typed shape conversion.** Shape trees store VertexKey, EdgeKey and FaceKey.
  The topology adapter resolves these to vertices, edge CURVES and face SURFACES,
  as the native spatial lists do. Mapping the edge/face object itself is wrong.
  A regression distinguishes topology handles from geometry handles; tree mapping
  separately checks links, neighbors, node classification, ownership and errors.
  Nonempty Shape creation still requires unported topology queries.
- **Undefined EdgeVertex operand.** SmTopologySolver.cpp:1032 selects pObj2 as
  pCurve without swapping, or pObj1 when swapping: the SAME vertex selected just
  above. The cast then calls GetEdge on a vertex. The Rust port rejects that class
  with InvalidInput instead of fixing the operand. A native runtime-type probe
  verifies the selected vertex is not a curve without performing the invalid cast;
  a separate Rust test exercises both argument orders. No C++ SolveIt result is
  fabricated or counted as differential coverage of this undefined operation.
- **Remaining face paths.** Face exclusion is compared; included faces explicitly
  defer in surface/query callees. Coincidence classification still needs the p4
  curve-classification constructor overload. Range endpoints need
  `SmSurface::GlobalPointSolve` in surface_p4 on the SAME shared cache with point
  testing disabled around BOTH solves;
  an independent owned-cache copy would not suppress the solver's trimming.
  No unused independent cache is constructed before this explicit deferral.
- **Signed endpoint behavior.** Native B-rep/curve and shape/curve entrypoints
  currently test only the start (`i < 1`). Only the start parameter tuple is
  rewritten for appended point solutions; the port preserves these choices.
- **Back-edges.** p1's GlobalSolve local hook and six local-class dispatches now
  call p2. Its 87 former traversal deferrals clear: 3,248/3,532 compare exactly,
  with 284 remaining genuine foreign-callee deferrals. GeomTree compares
  611/618; GeomTools compares 721/913. Its remaining
  B-rep point-query marker names the missing topology.brep_cache adapter,
  rather than this now-ported unit.
- **Final validation.** Against current official main, all required
  local gates pass. Sensitivity detects every non-deferred case in all four
  affected units (2,384, 3,248, 611 and 721). Full validation evaluates 142 units:
  89 green, 53 partial, zero failed. Default reports match the full run byte for
  byte. Report paths and scope limits above remain authoritative.
- **Review regression.** A node whose auxiliary type is not ObjectList is empty
  in native LocalSolve, which ignores GetObjectList's status. The Rust traversal
  now preserves that behavior for both operands while retaining structural
  checks on actual object lists. Separate native and Rust tests verify success
  with zero solutions in both orders. Part-2 regressions live in p2; only their
  shared scene builder remains in p1's test module.

## curve.curve_p9: zero-end-tangent Hermite pair, loose tolerance (one case in 30,093)

`difftest --unit curve.curve_p9 --seeds 30000` (generator after the review-13 changes) has
one disagreement, `gen21507` (seed 7577090626449511304): `SM_SO_3D_SIGNED_DIRECTED_MINIMIZE`
(9), `SM_SR_ALL`, `tol` 0.5, two near-identical cubic Hermite curves (`p1` differs by about
0.1, `d1` identical, `d2` = (0,0,0) on both), first interval `[0.9424, 1.0934]`, second
`[0, 1]`. First solution value: C++ `6.019854387120155e-2`, Rust `6.026870704308737e-2`
(relative error 5.8e-2). The result is stable under clamping the first interval to
`[.., 1]`, and the case agrees bit-for-bit when `tol` is 0.01, the first interval is
`[0, 1]`, or `d2` is non-zero (`d1` or `(1e-3, 0, 0)`).

**Root cause: upstream reads an uninitialised Jacobian (undefined behaviour), not a porting
bug and not floating-point ordering.** Traced with gdb on the C++ side against a Rust trace
(local `eprintln!`, removed again):

- `SmMatrix::SmMatrix(rows, cols)` does not initialise its inline storage (`m_adMat`, used
  up to 6x6; only the heap case callocs). `SmLocalSolveNd::SolveIt` declares
  `SmMatrix sJacobian(lNumFun, lNumFun)` and uses it after the first `Evaluate` without
  checking that the evaluator wrote it.
- `SmCurve::LocalCurveIntersect` (here on the two `SmProjectedCurve`s, along `vec[0]`)
  runs a first Newton solve whose last accepted iterate is `(0.99998324, 1.0)`: a clamped
  step to `t = 1`, where the Hermite's end tangent is zero. Its second solve
  (`sLS2`, "start again at the solution") is started from that point. There
  `SmCCIntersectENFO::Evaluate` takes the tangent branch (gap squared < 0.01, tangents
  parallel within 0.001 degrees) and calls `SmCCTangentIntENFO::StaticEvaluate` without
  checking its status; that returns `SM_ERR` (squared tangent length below `SM_EFF_ZERO`)
  before writing `rF` or the Jacobian. `rF` keeps the solver's `SM_BIG_DOUBLE` fill, so
  `J * step = -F` is solved with whatever the stack slot of `sJacobian` holds.
- In C++ that slot still holds the Jacobian of the previous `SolveIt` on the same stack
  address (`[1.387e-7, -1.387e-7, -4.813e-2, 4.812e-2]`, from the first solve's
  iterate at `(0.94281, 0.94283)`). The resulting step lands at `(0.94237, 0.5)` and the
  iteration ends at `(0.94237, 0.94236)`, value `6.0199e-2`. Rust's `Matrix::new` zeroes
  the Jacobian, so the system is singular (`BadJacobian`), the second solve fails, the
  first solution `(0.99998, 1.0)` is kept and the later stages end at `(1, 1)`, value
  `6.0269e-2`.
- Every residual and Jacobian of the first solve, and of the second solve's first
  evaluation, matches C++ bit for bit (`F = [-1.0452749776845849e-12,
  2.3103247408282146e-3]` at the third iterate, same `x` values at every evaluation
  before the divergence), so the Hermite cache, the projected curve evaluation and the
  Newton code are not involved.
- Proof: stopping the C++ run in gdb at the second solve's first evaluation and zeroing
  `sJacobian`'s four doubles makes the same case agree with Rust exactly (both
  `[1, 1, 2, 2, 0, 1, 6.026870704308737e-2, 1, 1]`, PASS, relative error 0).

Not fixable without copying uninitialised stack garbage, so the port keeps the
zero-initialised matrix (`geom::local_solve_nd`, outside this unit) and `curve.curve_p9`
is unchanged. The failing shape is a zero-length end tangent at a clamped parameter plus
a gap below 0.1 and a loose tolerance, so it only shows in large seed counts; the
default 3,093-case set (which the gate and CI run) is clean, and the previous generator's
30,055-case run was clean.

## intersect.surface_intersector_p5 (`SmSurfaceIntersector::DoIntersection`, 2026-10-14; partial, no disagreements)

- **Dispatch.** Every upstream caller runs `DoIntersection` on an
  `SmAdvSurfaceIntersector`, so it is ported on `AdvSurfaceIntersector` (its
  `SolveBranch`/`LocalSolve` overrides answer the `SM_SO_INTERSECTION_TEST` solve, its
  `FindInteriorCurves` override finds the loops). DISPATCH GAP for the porters of
  `surface.surface_p8` / `surface.surface_p11`: `TraceCurve` (part 4) calls the basic
  `ComputePointValues` -> `ComputeSingularityPoint` (`SM_ERR`), not the advanced
  override, so a traced point with parallel normals will disagree once start points
  can be found. Today every start point needs `SmSurface::GlobalCurveIntersect`
  (`surface.surface_p8`) or `SmSurface::LocalSurfaceIntersect` (`surface.surface_p11`),
  so no compared case reaches it.
- **Surface caches.** The real cache build needs `SmSurface::CalculateContinuities`
  (`surface.surface_p6`): every case without hand-built trees defers there. The runner
  swaps hand-built trees into the real C++ caches (`m_pTree`, restored after the call)
  and gives the Rust side caches holding the same trees. `SmCurveBoundedSurface`s
  without boundary curves get no C++ cache (`NER` -> `SM_ERR_NULL_POINTER`); they run
  only without trees.
- **Plane iso curves.** `SmPlane` boundary planes need the cache's
  `SmPlane::CreateIsoParametricCurve` (self-tagged `surface.surface_cache_p1` gap: the
  cache stores iso curves as B-splines, a plane makes `SmLine`s), so plane scenes that
  reach `DoBoundaryPlaneTest` defer; the generator mostly uses the same planes as
  bilinear B-splines, which compare.
- **Back-edges.** `base_global_surface_intersect` (surface_p2; plane_p2 and
  surf_of_revolution_p2 fall back to it) and `CreateParallelProjectionCurves`
  (surface_p7, whole body ported: it now returns `None` UV curves for `NULL`
  entries and takes the kernel) call it; their deferrals move to `surface.surface_p6`
  (surface caches). `surf_of_extrusion_p2`'s fall-back stays a stub tagged
  `intersect.adv_surface_intersector` (its output type differs).
- **Review (PR #82).** 94 adversarial fixture cases
  (`fixtures/intersect.surface_intersector_p5/review_adversarial.json`: zero, negative,
  huge and corner-gap tolerances, angle tolerances, collapsed corners, all 16 corner
  pairings, sector-boundary epsilon, sub/extended/reversed domains, scales 1e-7..1e7,
  rational bilinears, self intersection, a point-degenerate surface): 47 pass
  bit-identical, 0 fail, 47 defer. Never compared: the trace loop, the backward
  re-trace and the degenerate-curve sweep (every start point needs `surface.surface_p8`
  / `surface.surface_p11`). When those land, recheck the DISPATCH GAP above together
  with the fact that `DoIntersection` ignores `TraceCurve` errors: the gap would then
  silently turn a curve into a through point rather than fail. Open follow-ups:
  `surf_of_extrusion/p2.rs` still stubs `DoIntersection` under the (already partial)
  `intersect.adv_surface_intersector` tag and should be wired; `do_intersection_with_caches`
  leaves `point_test_enabled = false` / `process_boundary_curves = true` on the
  caller's caches (upstream restores them; the solver borrows the caches for `'t`,
  so restoring needs an API change); `FindBoundaryStartPoints` runs on a full clone
  of the caches (trees included), a cost only. The upstream prog_tests
  (`my_test_ssi`, `my_test_parallel_projection`, ...) are not fixtures yet: they
  would all defer on the real cache build (`surface.surface_p6`).

## topology.proto_topology_p1 (`SmOwningTopology`, `SmAssembly`, `SmCopyBrepMap`, `SmProtoTopology` part 1, API transforms, 2026-10-14; partial, no disagreements)

- **Harness.** One shim entry runs scripts of three kinds (list, proto, transform); the
  runner interprets the same script against the port and applies the shim's run-time
  refusals to its own state, so an op the shim refuses as undefined upstream (null or
  deleted pointer dereference, use after free in a destructor, a list walk that would
  not end) ends both records with `-2`, and a state divergence shows up as a mismatch.
  Proto ops are `[code, args...]` arrays exactly as the shim reads them.
- **Upstream defects found:** UB-98 (`SmCopyBrepMap` release asserts call `GetType()`
  on null objects; crashed the reference run), HANG-12 (`DropPoint` never returns after
  `Reparameterize` to a zero-length domain; the Rust curve point solve loops the same
  way, which belongs to the curve units), WR-73 (`RemoveCoincidentNoEdgeProtoVertex`
  returns `SM_SUCCESS` as TRUE; ported for parity). The shim refuses the UB-98 calls and
  zero-length `Reparameterize`.
- **Deferred** (`Unported`): the B-rep walks of `sm_AreConnected` and point
  classifications on bare topology objects (topology.vertex, topology.edge,
  topology.face_p2), `SmProtoVertex::ContainsPoint` (query.face_p2), surface point
  solves (surface.surface_p4), `SmBrep::Transform` (topology.brep_p5) and
  `SmPolyBrep::Transform` (query.poly).
- **Co-port wired:** `topology::edgeuse::p1::owning_topology_is_kind_of` (CO-PORTED in
  topology.edgeuse_p1, PR #55) now calls `owning_topology::is_kind_of`; edgeuse_p1
  difftest unchanged (2449 pass, 273 deferred).

## boolean.api_brep (`SmApiBrep.cpp` boolean entry points, PR #81, 2026-10-14)

- **Scope.** Thin wrappers over `SmMerge`; everything past input validation is deferred
  (`boolean.merge_p2` for the `SmMerge` constructor, `boolean.merge_p3` for
  `merge_breps` / `BooleanLists` / `BooleanTrees`). `SmMergeOptions` is co-ported in
  `boolean::merge::p1`. The C++ shim already runs real box/sphere booleans and emits
  `status, has, is_first, kept, faces, edges, vertices, regions, manifold_solid` (see
  `crates/solidcore-ffi/src/units/boolean_api_brep.rs`), so the deferred cases become
  comparisons as soon as the merge units land and `primitive.api_primitives` lets the
  runner build the same operands in Rust (today the Rust side passes empty `Brep`s).
- **Unrepresentable checks.** `NULL` operands, the same B-rep passed twice and `NULL` /
  duplicate list entries (`SmApiValidateBooleanOperands`,
  `SmApiMergeBrepsInputsAreValid`) cannot occur with owned Rust values; those upstream
  test cases (`TestSmBoolean`, `TestSmMergeBreps`) have no difftest counterpart.
- **Not owned.** `SmApiBooleanWithCurves`, `SmApiNonManifoldBoolean` and
  `SmApiPiecewiseMerge` (same file) are in no unit of `harness/callgraph/units.json`.
- **Performance.** Upstream `SmApiCreateSphere` takes 15-30 s at radius ~5e-5 (scale
  1e-4); the generator stops tiny scales at 1e-3. Not investigated further (unit
  `primitive.api_primitives`).

## blend.fillet_custom_solver (PR #78 review fixes, 2026-10-14)

- **Corpus.** 448 cases (48 fixture, 400 generated): 359 pass at max_rel_err 0, 89
  deferred (surface.offset_surface). The earlier note's "17 fixture, 405 generated"
  split was wrong (it was 22 + 400). 26 review fixtures (`adv_*`) cover antiparallel,
  identical and nearly parallel line rails, zero radius, `P2 == P1`, collinear
  points, rational / double-knot / polyline / quadratic rails, tolerance equal to the
  rail gap, `r2 > r1`, angles past pi, arc + line, non-coaxial and reversed arcs,
  multi-span arcs and tolerance 0.
- **FFI guard for `CreateSurface` intervals.** Rails over different natural
  intervals are undefined in C++ only in the skinning branch (rail synchronisation
  reads `pIntersectionsWithRails` past `k`). The guard now also admits two
  non-rational 2-point degree-1 rails at least `1e-7` long whose directions agree to
  within `5e-7` rad (half the `SM_EFF_ZERO_SQRT` sweep test, so they always take the
  ruled branch); `EvaluatePoint` clamps rail 2's off-domain parameters. The
  surface-of-revolution branch (both rails arcs) is also valid with different
  intervals but stays guarded, because predicting `IsArc` in the guard is not cheap.
- **Not compared yet.** The C++ shim clears the solvers' edge uses, so the
  edge-use / `GlobalPointSolve` / `IsClosed` / `DropPoint` branch of both
  `LoadInitialValues` is never run on either side (Rust reaches it only after the
  Unported offset `EvaluatePoint`). A static review found it matches upstream
  (`IsClosed` tol 0.0, `Evaluate`'s `bNonZeroTangents = TRUE`, the face/surface
  identity test, `EFF_ZERO_SQ` for the base `GlobalPointSolve`, the pre-extension
  domain for `LocalPointSolve`). Compare it once surface.offset_surface lands.
- **Wiring note.** `offset_compute_inward_vector` is tagged
  `UNPORTED(surface.offset_surface)` (`SmOffsetSurface::Evaluate`). Wire it
  through the ported `surface::surface::p7::compute_inward_vector`, not a new copy.

## topology.face_p2 (`SmFace` part 2, 2026-10-15; partial, no disagreements)

- **Harness.** As `topology.vertexuse`, the Rust side imports the C++ B-rep export. The
  export now carries each face's raw `m_pFU`, `m_vUVDomain`, `m_bRectangularTrim`,
  `m_sZoneTol3d` and whether its surface is owned by it, real `SmPlane`s (origin, axes,
  scales, analytic domain; rebuilt with `Plane::new`) and the vertex/face gap's face UV.
  The `topology.vertexuse` shim and runner were extended the same way when its
  `SmFace::GetUVDomain` / `GetTolerance` stubs were wired (its plane faces used to be
  imported as B-splines; its deferrals on `topology.face_p2` are gone, 0 failures).
- **Inexact geometry.** Several upstream `.smb` files (`Mouse`, `box_with_large_fillet`,
  `cone`, `sphere`, `surf_extrude`, `surf_rev`, `surf_sphere150`, `torus`, `toruswvy`,
  `vase_rev`) have face surfaces that are `SmBSplineSurface` subclasses other than
  `SmPlane`, and some edges are `SmEllipse`s; the export carries their B-spline part only.
  A first run showed what that costs: `GetPointsInFace` on `sphere.smb` returned a normal
  `z` of `7.5e-17` in C++ and `3.0e-16` in Rust, while the same B-spline surface as a
  standalone `SmBSplineSurface` gives `3.0e-16` in both (the subclass evaluates
  differently). Ops that evaluate such a face surface or (`sm_FindBestUVCurve`) edge curve
  therefore report `inexact_geometry` on both sides (both runners); they compare once the
  surface subclasses can be rebuilt in Rust.
- **`sm_FindBestUVCurve` on planes** is deferred (`curve.crv_on_surf`): the ported
  `CrvOnSurf` holds an `SmBSplineSurface` only.
- **Opaque handles.** `UNPORTED(topology.face_p2)` sites remain where the caller holds a
  face only as a generic handle or an `ObjectRef` (no `Brep` to look the face up in):
  `geom::object` (extent of faces), `geom::solution_array` (`face_surface`,
  `face_uv_domain`), `geom::tol` (comments for `GetScaledZero` / `UpdateObjectTolerance`),
  `intersect::point_class` (4), `intersect::curve_class::p1::compute_face_point_class`,
  `intersect::topology_solver::p1::face_silhouettes` (`CreateCurvesByTrimming`),
  `surface::plane::p2` / `surface::surface::p1` / `surface::surface::p7` (the owning face's
  tolerance), `surface::surface_cache::p1` (the surface's face handle). They need a way
  from those handles to the B-rep before they can call `topology::face::p2`.
- **Upstream defects:** WR-86 (`GetPointsInFace` samples a rectangular face from `0`, not
  from its domain), WR-87 (`sm_CheckSeamCrossing` returns `SM_SUCCESS` as `TRUE`).

## topology.edgeuse_p2 (SmEdgeuse part 2: sector tests, step-off binormal, UV trim curve checks, Notify)

- **Shared B-rep harness.** The B-rep construction/export helpers of the
  `topology.edgeuse_p1` shim moved to `harness/cpp/shim/sc_edgeuse_graph.h` (both shims
  include it). The export now also carries each face's `GetUVDomain()`,
  `GetTolerance()` and surface type code, the B-rep's `GetTolerance()`, and an `SmPlane`
  face surface as `1 O N` (it used to go out as its B-spline part); the
  `topology.edgeuse_p1` runner imports them (`Face::uv_domain`, `Face::tolerance`,
  `Brep::tolerance`, a `Plane` rebuilt from origin and normal, not bit-exact: only its
  kind is observed off deferred paths).
- **Inexact geometry is deferred, not compared.** The import holds an analytic
  `SmBSplineSurface` subclass (SmCone/SmCylinder 15012/13, SmSphere 15014, SmTorus,
  SmSurfOfExtrusion 15030, SmSurfOfRevolution ...) and an analytic `SmBSplineCurve`
  subclass only as their B-spline parts. `CheckAndSetUVTrimCurve` evaluates both, and on
  `cone.smb` the B-spline part of the `SmCone` differs from `SmCone::EvaluatePoint` by
  ~1e-15 (distances 6.2e-15 vs 4.0e-15). The runner therefore defers such cases (on the
  unit of the missing `SurfaceKind`, as `Surface::read_from_db` does, or on
  `topology.brep_p7` `SmBrep::ReadFromFile` where only an exact Rust B-rep reader can
  hold the geometry). Most upstream `.smb` faces are analytic; compared
  `CheckAndSetUVTrimCurve` cases come from `Endcap.smb`, `ellips_ts2.smb`, `Mouse.smb`
  B-spline faces and `CreateFaceFromSurface` B-spline faces.
- **Deferred everywhere else.** `IsTangentSector` (non-lamina), `IsConvexRadialSector`
  (with samples), `EvaluateBinormalStepOff` and `RebuildUVTrimCurve` reach
  `SmEdgeuse::EvaluateBinormal` / `CreateUVTrimCurve` (`topology.edgeuse_p3`); the shim
  skips ("not run", `-3`) those it cannot run safely (wire edge uses, missing geometry),
  which the Rust side always defers. `SM_NO_REG_PROPAGATION` is not sent to an edge use
  (the region-attribute handler would dereference the caller as a region).

## query.tess: BuildTree on a plane face needs the plane's NURB (review of PR #83)

- **Bug fixed.** `SmTessSrfCache::BuildTree` builds its top node from
  `pBSS->GetNaturalUVDomain()` and `SmBSplineSurface(*pBSS)`. Both read the NURB
  through the virtual `GetOrCreateGwNurbPointer`, which makes an `SmPlane`'s or
  `SmSurfOfRevolution`'s missing NURB first. The first port copied `p.base()` as it
  was; a Rust `Plane::new` has no NURB, so the top node got a zero uv domain and
  the tree was degenerate (`adv_plane_build`: node_geom[0] C++ -1.5, Rust 0; 16 of
  40 generated plane cases failed). The port now uses `Plane::base_with_nurb()`, and
  for a surface of revolution runs `make_nurb` on a copy when the NURB is missing.
  A failed `MakeNurb` copies the part as is. Upstream makes the NURB on the face
  surface itself; the port does it on the copy.
- **Coverage gap that hid it.** The shim built only `SmBSplineSurface`. It now also
  builds a bounded `SmPlane(o, x, y, scale, D)` (surface kind 1, same guards as
  surface.surface_cache_p3); 12% of generated cases and 8 adversarial fixtures use
  it. Still not compared: `SmSurfOfRevolution` faces, face-owned surfaces (the
  `GetTree` rebuild branch), CurveBounded/STEP faces (base `BuildTree`, unported
  until surface.surface_p6), and inputs the FFI rejects although upstream defines
  them (Implant without a tree, Test without a driver, Mark without a poly B-rep).

## blend.fillet_custom_solver (FFI validator audit, 2026-10-14)

- **Validator vs shim.** `validate()` reads every op's input in the shim's order and
  counts, and its bounds match what upstream indexes: `crX[s1|s2 (+1)]` with
  `lSurf1Off`/`lSurf2Off` derived exactly as `LoadJacobian` does (offset surfaces share
  their base pointer, so selectors 1/3 are base A, 2 is B or A, 4 is the copy), `rF[ne..ne+2]`,
  Jacobian rows `ne..ne+2` and columns `s1, s1+1, s2, s2+1`; `LoadInitialValues` reads
  `rGuessT[s1_index (+1)]`, `rIntervals[s1_index (+1)]` and writes `rIntervals[ng (+1)]`
  (`SmExtentNd` has inline storage for 10 and allocates the rest). `CreateSurface` only
  reads `crFilletPoints[0..k)` after checking `k` against both rails' unique knot counts,
  and `N_CrvsMakeCompatibleAdjKnots` rescales only when the rails' intervals differ. The
  output buffer is retried at the size the shim reports. No gap found there.
- **ASan run.** SMLib, SM_API and the shims built with `-fsanitize=address` (a private
  build, not `bin/cpp`): 20,000 fuzzed `eval` inputs (all five ops, boundary counts and
  offsets, same / different rail intervals) were clean. One case in the 448-case corpus
  was not: `gen0137` (a valid `Create`, same-interval quadratic rails) reads freed memory
  in `CreateSkinnedSurface`'s skinning retry (upstream UB-99). The FFI guard cannot
  predict that the first skinning attempt fails without running NLib's skinning, so it
  still admits such inputs; `gen0137` passes against the release C++ (whose output there
  depends on the freed memory's contents), and its result should not be taken as proof
  of upstream behaviour.

## difftest `--ssi-corpus` (jyviko's SSI corpus, 2026-10-14)

- **Correctness report, not a gate.** `--ssi-corpus` grades both engines against the exact
  ground truth of jyviko's corpus (`corpus/ssi/`, fetched by `scripts/fetch-ssi-corpus.sh`).
  At approximation tolerance 1e-6 the C++ reference scores PASS=81, INCOMPLETE=25,
  WRONG TOPOLOGY=11, ERROR=2, the same as the corpus' published SMLib figure (81/119). The
  38 misses are in `plan/upstream-issues.md` WR-74 to WR-80. A faithful port should score
  the same. The Rust scorer (`crates/difftest/src/ssi/`) gives the same verdict and the same
  flags as the corpus' `score.py` on all 119 C++ outputs. Distances agree to roundoff
  (1e-16 to 1e-12 absolute).
- **Plane NURB deviation found.** Coincident planes took a different branch in Rust and in
  C++. This is a port deviation of `surface.plane_p1`, recorded under that unit's
  heading above (newest entry). The corpus harness builds each plane's NURB
  (`Plane::make_nurb`) before intersecting, which mirrors the C++ runner's state after
  `GetNaturalUVDomain`; `c6_planes_identical` / `c6_planes_overlap` are then deferred on
  `intersect.surface_intersector_p5`.
- **Rust coverage at the time of writing:** 3 cases answered (plane/plane, identical to the
  C++ and PASS), 116 deferred. 62 are deferred on `SmSurfaceIntersector::DoIntersection`
  (`intersect.surface_intersector_p5`), 24 on spheres (`surface.sphere`), 21 on cones and
  cylinders (`surface.cone_p3`), 9 on tori (`SmTorus` is ported but has no `SurfaceKind`
  variant yet; deferred on `surface.surface_p2`, which owns the enum).

## topology.vertexuse (`SmVertexuse`, 2026-10-14; partial, no disagreements)

- **B-rep import.** As `topology.edgeuse_p1`: the shim builds real `SmBrep`s (15 upstream
  `.smb` files, wire edges, faces from B-spline surfaces and `SmPlane`s, the wire network
  of prog_test `my_test_topology`) and exports the graph (now with each vertex use's
  vertex/edge gap state and `m_sUVPoint`); the runner imports it into `topology::Brep`.
- **What is compared.** Owner queries, predicates, `IsConnectedTo` against every target
  kind, the gap getters/calculators/`InitVertexFaceGap` and `Notify` (cache states of
  every vertex use afterwards, so sibling resets are checked), `ComputeUVPoint` /
  `ComputeUVSector` / `IsSectorMissingSeam` / `IsOnPole` on shell vertex uses and wire edge
  vertex uses (statuses and outputs). On faces these defer: `SmFace::GetUVDomain`
  (`topology.face_p2`) is read before anything else, `GetSingularities`
  (`surface.surface_p3`) gates `IsOnPole`, and setting a gap needs `SmVertexEdgeGap::Set`
  / `SmVertexFaceGap::Set` (`heal.gap`). The rest of `ComputeUVPoint` / `ComputeUVSector`
  is ported behind further stubs (`NormalizedEvaluate`, `LocalPointSolve`,
  `GlobalPointSolve`, `IsSingularity`, `IsOnSeam`, `SmLoopuse::GetOrientation`) and is
  not yet compared; re-run the difftest when those land.
- **Vertex use lists.** `SmVertex` keeps no use list in the port yet; the uses of a vertex
  are found by scanning the B-rep's vertex uses (only used to reset caches, so order is
  not observable). `topology.vertex` should add the list.
- **Upstream bug.** `ComputeUVSector`'s opposite-orientation count has an operator
  precedence bug (`plan/upstream-issues.md` WR-83); ported as parsed.
- **Not wired**, still `UNPORTED(topology.vertexuse)`: `surface::trim_srf_cache`
  (`ComputeUVPoint`, `GetVertex`) and `intersect::point_class` (`IsConnectedTo`). Those
  callers hold opaque object handles without a `Brep` (as for `topology.edgeuse_p1`).

## topology.loopuse (`SmLoop` / `SmLoopuse`, 2026-10-15; partial, no disagreements)

- **Harness.** The runner reuses unit `topology.edgeuse_p1`'s B-reps (15 upstream `.smb`
  files with and without `RebuildUVTrimCurves`, the `my_test_topology` wire network, wire
  edges, plane and B-spline faces incl. the tiny/huge/pinched/sub-domain faces of
  `topology.edgeuse_p2`'s fixtures) and its export/import. The shared export
  (`sc_edgeuse_graph.h`, and the copies in the `topology.vertexuse`, `topology.face_p2`
  and `query.edge` shims) now carries each loop use's `m_eOrientation` (new field
  `topology::Loopuse::orientation`). The edgeuse export still lacks each face's `m_pFU`;
  the loopuse shim has a `FaceFu` op and the runner patches it into the import.
- **Mutating ops** (`FlipLoopOrientation`, `CollectEdgeuses`, `SetEdgeuse`, `SetLoop`)
  leave graphs `~SmBrep` cannot free (a loop use pointing at another loop is freed
  twice): the shim snapshots the loop/edge/vertex use links and restores them before
  deleting the B-rep, after recording the state it compares.
- **Deferred.** `FlipLoopOrientation` of any loop on a face (`SmFace::Notify`,
  `topology.face_p3`; every loop of the test B-reps is on a face, so the flip itself is
  only checked by unit tests), `GetUVCornerBoundaries` of an edge loop
  (`GetOrCreateUVTrimCurve`, `topology.edgeuse_p3`), `IsMissingSeamAtVertices` on closed
  surfaces (sector computation) and the inexact-geometry cases (as `topology.edgeuse_p2`).
- **Upstream bug.** `IsClosed3d` never reports a loop open (WR-93); ported as is.
- **Review fixes (PR #102).** No case had reached `IsClosed3d`'s over-tolerance branch
  (`SmLoopuse.cpp:851-866`): over 3,600 closed3d cases the largest gap was 0.039 of its
  `XSectTol3d`. The `Closed3d` op now takes an optional `edge_tol` that sets every
  `SmEdge::m_sZoneTol3d` (stored as `fabs`) / `Edge::tolerance` first;
  `fixtures/topology.loopuse/tolerance_limit.json` (1,984 cases: 0, 1e-15, -1e-12, 1e-9,
  `bCheckAllGaps` both ways, loop and loop use) reaches the branch 176 times (88 with the
  early stop), all bit-identical. The shim's `closed3d_safe` now predicts that early stop
  from `SmTol::GetXSectTol3d(GetZoneTol3d(a), GetZoneTol3d(b))`, as upstream, not the raw
  edge use tolerances. `fixtures/topology.loopuse/review_adversarial.json` keeps a
  deterministic 4,712-case subset of the reviewer's 78,120-case sweep (every op on loop
  indices 0..23 of each B-rep; the full sweep passed with 0 failures).
- **Guard wider than the C++.** In `FlipLoopOrientation`'s vertex branch, both the shim
  and the Rust reject a mate vertex use that is not owned by a loop use. Upstream only
  reassigns `m_pSorLUorEU` there (not undefined); `topology::VertexuseOwner` cannot hold
  that state and the input is malformed, so the case stays rejected on both sides.
- **For `surface.surface_p5`.** `GetUVCornerBoundaries` uses the UV point `DropPoint`
  writes whether or not `bSuccess` is set (statuses ignored by `SE` in release); the
  `drop_point` stub must return `Some` for any written point, `None` only when the
  out-param is left unwritten. Unreachable today
  (`GetOrCreateUVTrimCurve` is `UNPORTED(topology.edgeuse_p3)`).
- **Duplicate to merge.** `query::r#loop::uv_corner_boundaries` (co-ported on the borrowed
  `FaceLoop` view) duplicates `topology::loopuse::uv_corner_boundaries`; merge them when
  `query.loop` moves to the arena topology.
- **Not wired**, still `UNPORTED(topology.loopuse)`: `intersect::point_class`
  (`SmLoop::IsConnectedTo`, `SmLoopuse::IsConnectedTo`) and `surface::trim_srf_cache`
  (`GetEdgeuses`, `GetVertexuses`). Those callers hold opaque object handles without a
  `Brep` (as for `topology.edgeuse_p1` / `topology.vertexuse`). Wired: the co-ports of
  `topology.edgeuse_p1` (`loopuse_edgeuses`, `loopuse_shell`), `topology.face_p2`
  (`loopuse_vertexuses`), `query.loop` (`calc_quad_num`) and the
  `SmLoopuse::GetOrientation` stub of `topology.vertexuse` now call this unit.
- **After merging main (topology.vertex, topology.edge, heal.gap).** `IsConnectedTo`
  with vertex/edge targets now calls `topology::vertex::is_connected_to` /
  `topology::edge::is_connected_to`; the shim runs it, with a null-dereference status
  when the loop has no loop use (or, for a vertex, its loop use's vertices cannot be
  walked), so the 99 formerly deferred cases pass (8854 cases, 7950 pass, 904
  deferred). The loop use orientation is added to the export copies in the
  `topology.vertex`, `topology.edge` and `heal.gap` shims and their importers.
  `topology::vertex::loopuse_vertices` and the private `topology::edge::loopuse_edges`
  (co-ports) duplicate this unit's `GetVertices` / `GetEdges` walk; merge them later.
- **`surface.surface_p5` merged, `DropPoint` stub kept.** `GetUVCornerBoundaries`'s
  `UNPORTED(surface.surface_p5)` `drop_point` stays a stub: it is reached only after
  `GetOrCreateUVTrimCurve` (`UNPORTED(topology.edgeuse_p3)`), so no case can compare it,
  and it needs "was the out-param written" (upstream writes `rResultUV` with a NULL guess
  only on success paths), which `Surface::drop_point` does not report (it returns
  `Vec2::UNDEF`). Wire it with that distinction when `topology.edgeuse_p3` lands.

## intersect.surface_drop_lib_p1 (`SmSurfaceDropLib.cpp` part 1, 2026-10-15; partial, no disagreements)

- **Ignored evaluation statuses.** `SmDropPt::IsMovingAlongBoundary` ignores the statuses
  of its `Evaluate1stDerivatives` / `EvaluateNormal` calls, and `TestSpanAccuracy` those of
  the two `EvaluatePoint`s of its domain clamp check. This is not undefined behaviour:
  `SmVector3d` / `SmPoint3d` members default to `SM_UNDEF_DOUBLE`, so upstream goes on with
  those defaults or whatever the evaluator wrote before it failed. The port does not
  model that path (a failed evaluation is an error), and the shim reports such inputs as
  `REJECTED` on both sides. The guard has never fired: 0 of the generated moving/span
  cases and 0 of the review's ~3600 pole, cone-apex and collapsed-edge cases, so no valid
  input is hidden today. If an evaluator failure is ever reached, port it with
  `SM_UNDEF_DOUBLE` vectors instead of rejecting.
- **`sm_FixBoundaryCurves`** on `3k` points (`k >= 2`) reads one point past the array
  (`SM_ASSERT` compiled out): undefined, rejected by the shim, `InvalidInput` in Rust.
- **Coverage limit.** `DropIsoCurve` grows its `dKnots[100]` `SmTArray` buffer past 100
  knots; the FFI caps B-spline curves at 64 control points, so that growth is not
  difftested (the Rust side has no limit).
- **Review fixes (PR #98).** Clippy with `--features difftest/cpp` failed in the runner
  (collapsible `if`, unchecked indexing in two generators); fixed without changing the rng
  draw order, so the generated cases are byte-identical. 328 review fixtures added
  (`fixtures/intersect.surface_drop_lib_p1/review_poles.json`, subsampled from 3596; none
  undefined in C++): all pass bit-identical or defer on `surface.surface_p3`.
## topology.vertex (`SmVertex`, 2026-10-15; partial, no disagreements)

- **Vertex use lists.** `topology::Vertex` gained `vertexuses` (`m_pList`, upstream list
  order; `GetPrimaryVertexuse` is its head) and `poly_vertex` (`m_pPolyVertex3D`). Every
  B-rep builder must keep `vertexuses` in step with the vertex use records: the
  `topology.vertexuse` / `topology.edgeuse_p1` runners fill it in arena order (their
  shims do not export the list), the `topology.vertex` shim exports the real list. The
  co-ported scans in `topology::vertexuse` now call `topology::vertex`.
- **`m_bExact` does not exist in the reference build.** `SM_TOLERANT_WATCH` is defined
  only by `NL_DataStruct.h`, which is not seen before `SmVertex.h`; the shim cannot even
  name `m_bExact`. `IsExact` is therefore always `TRUE` and `SetExact` does nothing; the
  port does the same (no field).
- **Uses that no owner chain reaches.** In prog_test `my_test_topology`'s wire network a
  vertex's use list holds vertex uses that the region/shell/edge walk never visits (how
  they got there was not investigated). The vertex shim registers the vertices' list
  members too; the earlier topology shims' exports omit them.
- **Chains of wire edges.** The shim adds a B-rep kind built as prog_test
  `my_test_edge_geometry_repairs` builds its wire (`MakeShellVertex`,
  `MakeWireEdgeVertex`, `MakeWireEdge`) from real `SmLine`s (optionally
  re-parameterised), degree-1 `SmBSplineCurve` lines and B-spline curves, so
  `IsTopologicalVertex` / `AreEdgesTangent` are compared on lines (the `IsLine` branch),
  on curves meeting at equal and unequal parameters, around the 0.5 degree limit, on
  cusps (upstream counts them, WR-92) and on reversed edges.
- **Curve identity.** Upstream compares edge curves by pointer in
  `IsTopologicalVertex`; each Rust edge owns its curve, so a B-rep whose edges share a
  curve object is rejected by the runner (none of the fixtures do).
- **Upstream bugs.** `GetVertexuseOfFace` ignores its face (WR-91),
  `GetMaxUpDimGap3d` adds to a `NULL` gap array by default (UB-105), the tangent tests
  read an uninitialised tangent after a failed evaluation (UB-106). Ported as written
  where defined; the undefined cases are `InvalidInput` and skipped by the shim.
- **Review fixtures (PR #96 review).** `fixtures/topology.vertex/review.json` (1408 cases,
  all pass, none skipped by the shim) adds what the generated chains never reach:
  rational edges at a vertex with two edges (`SmBSplineCurve::IsLine` handing a rational
  curve to the sampling `SmCurve::IsLine`), rational arcs meeting lines and arcs at equal
  and unequal parameters, turns at 0.49/0.51/179.49/179.51/180/90 degrees, 1e-12..1e12
  scales and tangent / gap / `SetTolerance` limits. Still not compared: `SmCircle` /
  `SmEllipse` edges at a vertex (the chain shim builds only `SmLine` and
  `SmBSplineCurve`); the shim reports `-2` (skipped) for an `IsTopologicalVertex` where
  upstream's SER would return before reading the uninitialised tangent, and the FFI
  rejects NaN tolerances that C++ handles (coverage only, no wrong result).
  `Brep::add_vertexuse_record` does not append to `Vertex::vertexuses`; a linking helper
  or a debug consistency check would stop silent drift once library builders create
  vertex uses.
- **Not wired**, still `UNPORTED(topology.vertex)`: the generic-handle callers in
  `intersect::point_class` (`GetBrep`, `IsConnectedTo`, `GetPoint`, `SetPoint`,
  `GetTolerance`, `SetTolerance`, `SetExact`, `GetScaledZero`), `geom::solution_array`
  (`GetPoint`), `geom::object` (`GetPoint`), `geom::tol` (comments),
  `surface::trim_srf_cache` (`GetTolerance`, behind `UNPORTED(topology.vertexuse)`),
  `topology::proto_topology::p1::proto` (`GetVertexuses`, `GetPoint`) and
  `query::poly` (`Get/SetPolyVertex` of an original vertex key). They hold opaque object
  handles (or `ObjectKey`s) that cannot reach a `Brep` vertex yet (as for
  `topology.edgeuse_p1` and `topology.vertexuse`); whoever gives those handles a B-rep
  should call `topology::vertex`. `geom::tools`' `GetTolerance` is wired (its 60
  deferred cases now pass).
- **After merging main (topology.face_p2, heal.gap).** `SmFace::GetUpwardFaceuse` is
  wired to `topology::face::p2::upward_faceuse`; the shim now exports each face's
  upward face use (`m_pFU`) and the runner sets `Face::faceuse` from it. Run: 5508
  cases, 4627 pass, 0 fail, 881 deferred (topology.edgeuse_p3 399, surface.surface_p3
  382, topology.face_p3 76, topology.edge 24).

## surface.surface_p10 (`sm_GlobalCurveIntersectCombineSolutions`, `SmLSIGlobalSolver::LocalSolve`, PR #93 review, 2026-10-15; partial, no disagreements)

- **Adversarial fixtures.** 31 review cases were added to `fixtures/surface.surface_p10/cases.json`
  (`adv_*`): inverted, degenerate and ray-degenerate line intervals, a 1e-13 line vector,
  zero tolerances with a grazing line, rays starting on, above and in the plane of the patch,
  an interval ending within 2*tol3d of the patch, domains inside by EFF_ZERO / a v sliver /
  a touching corner, the point/range swap in the closest-solution search (point index left
  on a range), the deviation and length exceptions for touching ranges, reverse-u overlap,
  overlap not between, small ranges with start dev > end dev, culling with invalid nodes,
  nvars=2 and a huge tolerance. Rust and C++ agree on all of them or both defer. Run: 867
  cases, 515 pass, 0 fail, 352 deferred; `--perturb` 515/515. Seven review candidates were
  left out because the FFI guard rejects them (inverted interval/domain, `ivl=[0.5,0.5]`
  combine, negative tolerance, and the three below).
- **FFI guard stricter than needed for the line inputs.** `MAX_COORD = 1e9` (the repo
  convention) rejects line points/vectors such as 1e12 and infinite-parameter intervals
  (+-1e30), which the C++ line solve would handle. Those inputs are therefore not compared.
  A later pass could relax the guard for the line of the line solve only.
- **`LsiGlobalSolver::new` (surface.surface_p1) leaves `surface_cache` and `kernel` as
  `None`.** A library caller that builds the solver with `new()` and runs it gets
  `SM_ERR_NULL_POINTER` from `local_solve` (formerly `Unported`). No library caller exists
  yet; whoever adds one must set both fields (as the surface_p10 runner does) or extend the
  constructor.
- **Coverage limit.** Every line-solve case that passes the box culls reaches
  `SmSurface::LocalCurveIntersect` (surface.surface_p6) and is deferred, so the guess drops,
  filtering, polar-box exit, line-end `LocalPointSolve` and the patch-boundary iso and
  coincidence code are checked only by reading (against SmSurface.cpp:18075-18823: guess
  order [first, .5, .1, .9], dUVEps only on the first containment test, the
  degenerate-interval exit only on the V boundary, the unexpanded pseudo box passed to
  `SimpleCoincidenceChecker`, `IsDegenerate` defaulting to `SM_EFF_ZERO`). Re-run this
  unit's difftest when surface.surface_p6 lands.

## surface.surface_point_solve_p3 — integration of the new point solvers (PR #106)

- `curve.iso_curve` now stores a concrete `Surface`, so boundary solves retain the
  analytic `SmPlane` evaluator and the original surface-of-revolution / STEP
  dispatch. The previously documented B-spline-only dispatch gap is removed;
  missing *surface* operations still propagate their owning unit's `Unported`.
- Temporary solver copies retain their surface owner and the exact NURBS data of
  B-splines, planes and surfaces of revolution. `Surface::clone()` is the upstream
  copy constructor: it resets the owner and can fix knots / construct a plane's NURB,
  so it cannot stand in for `SmTemporaryChangeValue` during a solve.
- `CheckCrease` is disabled by upstream's `constexpr sbDoCreaseSubdomains = FALSE`.
  The port deliberately returns `(false, false)` without evaluating a crease.
- The local driver retains the 35-iteration limit and the separate status, found
  flag, termination reason and iteration count. `FindNextStep` also preserves the
  upstream plane-converged branch assigning `sNREval`, rather than silently changing
  the C++ algorithm.
- `Surface::local_point_solve`, `Surface::drop_point` and the patch-local point
  solver take the caller's kernel. The context is threaded through their geometry,
  intersection and topology callers, including `SurfaceEvaluate` wrappers; local
  and global fallbacks no longer replace caller tolerances with defaults.
- Boundary regression `matrix_0363` exposed an overly strict Rust guard in
  `SurfOfRevolution::evaluate_step`: release C++ accepts sweep-only derivative
  orders such as `(2, 0)`, despite its debug-only equal-order assertion. These
  outputs are fully evaluated and required by `SmIsoCurve`. The guard now matches
  release C++; other unequal orders retain `SmVector3d`'s default `UNDEF` values
  for generator derivatives beyond `min(hu, hv)`, as upstream does.
- Evaluation copies of nested STEP, curve-bounded and volume surfaces preserve
  exact nested geometry and ownership; a preflight walk bounds every owned
  surface, trim-curve and volume-map branch to 64 levels before recursive copying.
- **PR #106 review:** the seam jump uses `!new_uv.approx_ne(uv)`, preserving the
  left operand and the scaled tolerance of `SmVector2d::operator!=`. Two native
  regressions failed before the fix (`cpp U=0`, `rust U=1e-6`) and pass after it;
  four controls just beyond the tolerance still jump as upstream does. The large
  V coordinate makes the short U seam tolerance-equivalent without violating the
  native harness's minimum parameter-range guard.
- Six seam-threshold fixtures and 36 STEP-wrapped sphere-, torus- and cone-shaped
  revolution fixtures cover both seam sides, poles and three solver operations.
  These use actual `SmSurfOfRevolution` generators, not B-spline stand-ins.
- Integrated the final p4 back-edges from merged PR #49 and the latest p5
  back-edges from PR #50. The remaining `heal.gap` surface drop now calls the real
  `Surface::drop_point`, preserving its optional guess and ignored ordinary status.
  No `UNPORTED(surface.surface_p4)` or `UNPORTED(surface.surface_p5)` sites remain.
- Reaching `sm_SpLocalSolve` through p6 uncovered a stale native shim sentinel:
  its adapted `sm_SCLocalSolve` returned `SC_SP_LOCAL_SOLVE` instead of calling
  the reference solver. p5/p6 now include the same verbatim `sm_LoadGuesses`,
  `sm_PassSPConvergenceTest` and `sm_SpLocalSolve` bodies from a shared shim header.
  No reference numerical logic or differential tolerance is changed.
- **After #50 merged:** merged `origin/main` (`51ec6119`) and retained the caller's
  kernel in `heal.gap` while preserving `DropPoint`'s failure defaults (`false`,
  the supplied guess or `UNDEF`, and `BIG_DOUBLE`). Full local CI passed: 1152
  workspace tests; native difftest 169 units (92 green, 77 partial), 410264 passing
  cases, 11837 deferred, zero failures. The p3/p5/p6/heal.gap perturbation checks
  detect every compared case.
- The exact temporary copy remains necessary to preserve the immutable surface's
  out-of-bounds flag on all exits. Its owned-geometry preflight still bounds copy
  depth to 64. A borrowing evaluation adapter would be a separate optimization.

## heal.gap (`SmGap`, `SmGapArray`, `SmGapSample`, `SmGapFunction`, 2026-10-15; partial, no disagreements)

- **What is compared.** Gap functions (curve or surface to point, curve or surface)
  over real `SmBSplineCurve`, `SmHermiteCurve`, `SmLine`, `SmEllipse`, `SmCircle`,
  `SmCrvOnSurf` curves and `SmBSplineSurface`, `SmPlane`, `SmCurveBoundedSurface`
  surfaces: status, adjusted sample counts, sample state, extreme indices, all-normal
  flag, other-parameter ranges and every sample (drop type, both positions, tolerance
  point, parameters, tolerance, in-tolerance flag), through `GetMaxGapSample` and
  `GetSampleSet`, with and without stopping at the first big gap, after `SetSampleCnt` /
  `SetXSectTol3d`; single `Evaluate` calls with and without drop guesses;
  `sm_AddUniqueGetOrderedIndex`; `SmGapSample::SetCanonical`; `SmGapArray`; and the six
  gap records set on real B-reps (constructor = `Set`, every accessor,
  `GetGapEndPoints`, `CacheGap`). All bit-identical (max_rel_err 0).
- **Topology cases** build the B-rep exactly as the `topology.vertexuse` shim (the code is
  copied into `harness/cpp/shim/heal_gap.cpp`), and the Rust side imports the same
  export with that runner's importer (made `pub(crate)`). The importer rebuilds every
  face surface as a plain B-spline of its NURB (an `SmPlane` as a plane, since the
  topology.face_p2 merge) and every edge curve as a B-spline (or `SmLine`), so the shim
  replaces the other analytic face surfaces and non-line edge curves of the C++ B-rep by
  plain B-spline copies of their NURBs before the op (the originals are leaked). Without that, gaps between nearly coincident points disagreed at ULP level
  (e.g. `0` vs `8.9e-16` on `disk.smb`): the analytic and NURB evaluations differ in the
  last bits, which a distance of near-coincident points amplifies. Parameters are given
  as fractions of the selected edge's interval / the face surface's domain, resolved on
  the imported graph by both sides.
- **Deferred:** curve drops onto an `SmCrvOnSurf`
  (`SmCurveClassification` construction, `intersect.curve_class_p4`), and trim-curve gaps
  on edge uses without a UV trim curve (creating one in
  `SmEdgeuse::GetOrCreateUVTrimCurve`, `topology.edgeuse_p3`; the existing-curve branch
  is read in place).
- **Back-edges wired:** `SmVertexEdgeGap::Set` / `SmVertexFaceGap::Set` in
  `topology::vertexuse` (`CalcVertexEdgeGap`, `CalcVertexFaceGap`) and
  `SmEdgeEdgeGap::SetEdgeEdgeGap` / `SmCrvSrfGapFunction` in `topology::edgeuse::p1`
  (`CalcEdgeCCWEdgeGap`, `CalcMaxEdgeFaceGap`, which now take the kernel). Their
  runners compare only the cache states of those gaps, so the gap values are compared
  here. `CalcMaxEdgeFaceGap` uses the context-less `SmTol::GetXSectTol3d(NULL)` (the
  records carry no context). PR #106 wires the surface drop for both gap functions.
- **Upstream bugs** ported as is: `plan/upstream-issues.md` WR-88 (non-B-spline `v`
  samples spaced by array index), WR-89 (B-spline spans walked from the first knot),
  WR-90 (`SmGapArray::Append` keeps the old longest gap), UB-104 (null dereferences and
  zero counts; the port returns `InvalidInput`).
- `SmBrep::ReadFromFile` crashes on `SmallBox.smb` (UB-1), so topology fixtures use the
  `topology.vertexuse` file list.

- Full-suite integration regressions exposed two existing dispatch/output gaps.
  `BSplineSurface::STEPInversion`'s guessed local solve now uses the original
  concrete surface (and caller kernel), retaining `SmPlane`'s virtual evaluator.
  `SmSurfaceEval` now retains the B-spline position/derivative matrix written
  before zero-tangent correction returns `SM_ERR`. C++ accessors ignore that
  failure and read those values; discarding them caused false convergence on
  collapsed surfaces. Native regressions cover the initial norms, next step,
  local driver and callers (`surface.surface_p4/p5`).
- `SurfPtSolveStepper::Normal` now ignores ordinary failed-normal status and
  reads the cached normal, like C++ (missing dependencies still propagate).
  A source audit also corrected `SurfPtGlobalSolver::Solve` to retain a failed
  `RefineGuessByGrid` status internally rather than returning it from `Solve`.
- The collapsed Normalize regression has no usable rough guess: both engines
  return `SM_ERR` with an empty solution array. The p3 and existing p4 runners
  now retain that array count on error, matching their C++ wire format; p3 also
  compares restoration of out-of-bounds evaluation on this failure path.

## sweep.api_brep (`SmApiBrep.cpp` sweep entry points, PR #110, 2026-10-15)

- **Partial, no disagreements.** Every entry point validates and copies its inputs as
  upstream, then stubs the `SmPrimitiveCreation` sweep it wraps
  (`primitive.primitive_creation_p2` / `_p3`); the compared cases are the API-level
  rejections and copy failures.
- **Kernel crashes fenced off by the FFI** (`plan/upstream-issues.md`): UB-111 (`Draft`
  with no curves), UB-112 (`Draft` / `Taper` on a profile curve below about
  `2 * SM_ZONE_TOL_3D`), HANG-13 (negative-distance linear sweep of a full circle),
  UB-113 (`PlanarPath` with a Hermite curve in either list: the failed profile `Copy`
  and the unchecked `SmBSplineCurve*` cast), UB-114 (`CurveSweep` whose profile holds no
  B-spline curve reads `sNurbs[0]` of an empty list unless `bMoveProfileCenterToPath`).
  Each guard fires only where the kernel is reached: a list holding a `Null` or
  `Hermite` curve fails in `SmApiCopyCurvesForConsumingSweep` first, and a capped
  `LinearReps` with other than one repetition is refused by `CreateLinearSweep` with
  `SM_ERR` before it sweeps. The porters of `primitive.primitive_creation_p2` / `_p3`
  must return an error in those UB cases instead of crashing.
- **For the p3 porter:** `SmApiCreateCurveSweep` calls `CreateCurveSweep` with the
  default `bTestContinuity = TRUE`.
- **Review fixtures** (`fixtures/sweep.api_brep/review.json`, 132 cases): empty lists for
  plain `Linear` / `Rotational`, guard edges (`-0.0` distance, 359.9999 degree arc, `Null`
  / `Hermite` before a full circle or tiny curve, capped reps 0 / 2), and the 96
  `CurveSweep` profiles without a B-spline curve with `bMoveProfileCenterToPath` set (all
  other flags, scaled and unscaled), which run without crashing in C++.

## intersect.surface_drop_curve (`SmSurfaceDropCurve`, PR #107 review, 2026-10-15; partial, no disagreements)

- **Iso lines at C0 knots dispatch to the ported overrides.** `FindBoundaryStartPoints`
  builds the surface's interior C0 iso lines through the virtual
  `CreateIsoParametricCurve`. The first port stubbed it for every kind but the plane;
  it now calls `BSplineSurface::create_iso_parametric_curve` (default options: no domain,
  no UV line) and `SurfOfRevolution::create_iso_parametric_curve` (no domain), as
  `surface/surface/p6.rs` does. Only curve-bounded, step and volume surfaces stay on the
  `surface.surface_p5` stub. A `Boundary` op on a B-spline surface with a C0 u-knot (on
  and off surface) now gets past the iso lines and the curve intersections and is
  deferred later, at `SmSurface::GlobalPointSolve` (`surface.surface_p4`) or
  `intersect.curve_class_p4`.
- **`TraceCurve2` without a knot vector segfaults the C++.** When the plane `DropCurve`
  does not return exactly one curve, `TraceCurve2` falls back to
  `SmSurfaceTracer::TraceCurve`, whose `SmSurfaceDropCurve::ComputeStepSize` reads the
  null `m_pCurveKnotVector`. Trigger: plane over `[0,10]^2`, segment
  `(-5,-5,0)-(-1,-1,0)`, breaks `[0,1]`, no knots, ops `cpv(0, [-5,-5])` then `trace2`
  (difftest exit 139). The FFI now rejects `Trace2` unless a non-empty knot vector is
  given or made by an earlier `Boundary`/`DoTrace` (`invalid argument: trace without a
  knot vector`), like `Step`. Upstream callers always set the knot vector before tracing.
- **Off-surface `FindBoundaryStartPoints` on a plane** returns
  `Unported(curve.crv_on_surf)`: the Rust `CrvOnSurf` holds only a `BSplineSurface`,
  while upstream `SmPlane` is an `SmBSplineSurface` (dispatch gap, as in
  `intersect.curve_class_p1`). Unreachable today, because `surface.surface_cache_p1`
  defers the plane's iso boundary curves first. Revisit when `crv_on_surf` takes a
  general `Surface`.
- **Coverage is thin because of dependencies.** Every B-spline `Cpv` is deferred
  (`IsSingularity`), and `Lps`/`Span`/`Next` past their early exits, `Boundary` and
  `DoTrace` are always deferred (surface point solves). The real comparisons are `Cpv`,
  the `Next` early exits, `Trace2` and `Interior` on planes, plus `Step`, `AddBreak`,
  `TestPoint`, `Graze` and the hooks. `OP_SPAN` only runs with `curr == next`; add a real
  two-point span once `LocalPointSolve` lands.
- **Review fixtures** (`fixtures/intersect.surface_drop_curve/review_adversarial.json`,
  33 cases, 30 compared, all bit-exact): vertical and near-vertical dropped tangents, a
  B-spline curve with a zero start tangent, unsorted, short and out-of-range knot vectors
  with negative and zero steps, antiparallel and near-parallel grazes, `sm_TestPoint` at a
  collapsed B-spline edge (`EvaluateNormal` path), a C0 B-spline surface `Boundary`, the
  `Next` boundary and singular early exits, `Trace2` with breaks below the start and with
  the start outside the domain, and `Interior` on curves crossing or outside the domain,
  at 1e6 and 1e-6 scales.
- **Back-edges wired at merge (2026-10-15).** After `surface.surface_p4`, `surface.surface_p5`
  and `intersect.surface_drop_lib_p1` landed on main, the stubs for `IsSingularity`
  (`EFF_ZERO`, no point-only test), `LocalPointSolve` (`SM_SO_NORMALIZE`),
  `GlobalPointSolve`, `IsOnSeam` (no tolerance), the base `CreateIsoParametricCurve` and
  `DropIsoCurve` now call the ported code; `ComputePointValues`, `LocalPointSolve`,
  `ComputeNextPoint` and `ComputeSpanDeviation` take the kernel. Difftest: 473 passed,
  0 failed, 74 deferred (was 341/206), now deferred at `surface.surface_point_solve_p3`
  (56), `surface.surface_cache_p1` (11), `intersect.curve_class_p4` (5) and
  `intersect.surface_drop_lib_p3` (2).

## mass.integrator (`SmIntegrator`, `SmODEIntegrator`, precise properties)

1528 default cases: 1520 pass, zero fail, eight deferred; maximum relative error
`4.979280456316938e-14`, without absolute floors. The C++ shim calls the actual
scalar/ODE integrators, edge precise properties and public length query, and
`SmFace::ComputePreciseProperties` on real rectangular/rotated plane faces,
disks, annuli and curved quadratic B-spline patches. For the face integration,
the actual input oriented trim curves, UV domain and bounding-box-derived area
estimate are exported from C++ and imported into Rust;
results and moments are computed separately. All 63 precise-property masks are
covered in both orientations, following `sm_CheckPrecisePropertySelection` in
`tests/SM_API_test/src/TestSmBrep.cpp:244`. Generated cases cover state-dependent
ODEs, scalar polynomial/sinusoidal/exponential functions, reversed intervals,
error scales, sub-intervals and small/large magnitudes. Values, adaptive steps,
mutated scales and accepted/reduced step counts are compared.

- **Review regression:** 155 additional cases cover full-circle trims and circular
  holes, both orientations, accuracies from `1e-3` to `1e-8`, tight/zero accuracies
  exercising numerical failures and retries, non-axis-aligned plane bounds, curved
  B-spline surface properties, and nested `AreaIntegrand::evaluate` compared with
  `SmSurface::Area` on rectangular sub-domains. Edge cases include exact rational
  quarter circles, non-unit weights, cusps, constant curves, zero-length intervals,
  multiple accuracies and `1e-9`/`1e9` scales. Before the fix, 16 circle-boundary
  cases failed (maximum relative error `1.4907788505308581e-4`); all now pass.
  `SmODEAreaIFEO::Evaluate` leaves its output untouched for a zero U tangent;
  the adapter now seeds scratch from the particular integrator derivative array,
  preserving zeroed stage buffers and the main loop's previous derivative separately.
  No tolerance or absolute floor changed.
- **Upstream behavior retained:** `RungeKuttaODEIntegrate` starts its accumulator
  at zero regardless of `ystart`, except its early return (equal endpoints or
  first step equal to `SM_EFF_ZERO` to scaled tolerance), which leaves it alone.
  `IntegrateIt` ignores its algorithm selector and returns success with zero on
  ODE/evaluator failure; see WR-94 in `plan/upstream-issues.md`.
- **Safety:** zero-based component slices and closures replace one-based array
  subclasses. Dimensions are bounded at 4096, the step budget at 1,000,000 and
  rejected-step retries at 1024. Active integrations reject NaN controls and
  non-finite evaluations; no input-driven recursion. Infinite accuracy/scale floors and unused non-finite
  initial values agree with C++ in a direct harness test; equal endpoints return
  before checking unused controls. Scalar failure masking never masks `Unported`.
- **Deferred:** B-rep bounding boxes (`query.brep`, 3), face bounding boxes
  (`query.face_p1`, 2), B-rep enumeration (`topology.brep_p1`, 2), and thick-face
  offset evaluation (`surface.offset_surface`, 1). Oriented trim construction
  still calls `UNPORTED(topology.face_p1)` in the topology-facing face entry;
  callers with oriented boundaries can use `boundary_properties` directly.
- **Back-edges:** all three `UNPORTED(mass.integrator)` sites are wired: curved
  lengths in `curve.curve_p5`, area strips in `surface.surface_p1`, and the inner
  precise-property integration in `topology.face_p2`.

## surface.surface_p3 back-edges into surface.surface_p5 (finishing PR #52, 2026-10-15)

- **Wired in `surface::surface::p5`:** `GetSingularities` in `IsSingularity` (status-free
  upstream, so `ignored()` with its initial outputs `SM_SS_NONE` and uninitialised poles),
  the `SmFindPSNormalENFO` Newton solve of `LocalPointSolve_0` (`SER(sLS.SolveIt(...))`
  is `?`), the UV-guess `LocalSurfaceSurfaceIntersect` of the image-space overload (status
  ignored upstream: no answer), `CoverCoincidentSurface` in `CoverOtherCoincidentSurface`'s
  `Cover` lambda (a failing call is no cover and leaves `rdMaxDist = SM_BIG_DOUBLE`, its
  first statement), and `EvaluateNormalSection(uv, TRUE, TRUE, ..)` in `sm_FilterSolution`
  (status ignored; curvature `0.0` keeps the line branches upstream takes when the section
  fails).
- **Kernel threading.** `IsSingularity`, `EvaluateNormalAtSingularity` and
  `CoverOtherCoincidentSurface` take `&Kernel` (`GetSingularities` needs the approximation
  tolerance). Callers without a kernel use `Kernel::default()`: the `IdenticalSolutions`
  overrides in p5, `LocalCurveIntersect` (p6), `sm_ComputeBezierApproximation`
  (surface_cache p1) and `CreateExtendedSurface` (b_spline_surface_n_lib p1). The
  `SmSSGlobalSolver` override uses the kernel it holds.
- **`CoverOtherCoincidentSurface` changes the covering surface.** It now takes both
  surfaces `&mut`. `topology::edge::is_topological` therefore takes `&mut Brep` and a
  `&Kernel`; it moves the two face surfaces out of their faces for the call (keeping their
  owners) and puts them back, so an expansion stays on the face, as upstream.
- **`surface.surface_p6` shim runs `sm_SpLocalSolve`.** It used to stop with `-2` where
  `sm_SCLocalSolve` calls it, and the Rust side deferred there. With `GetSingularities` and
  the `SmFindPSNormalENFO` solve wired, three generated cases (gen0073, gen0882, gen1429)
  ran the whole Rust point solve, so the shim now holds the same verbatim
  `sm_SpLocalSolve`/`sm_LoadGuesses`/`sm_PassSPConvergenceTest` copies as
  `surface_surface_p5.cpp`. All three agree. (Merging PR #106 replaced the copies with
  the shared `harness/cpp/shim/surface_sp_local_solve.hpp` that both shims include.)
- **`surface.surface_p2` extension cases with a failed first attempt are not generated.**
  Upstream retries an in-place `N_SrfExtendByDist` on half-written arrays (UB-107 in
  `plan/upstream-issues.md`); generated case gen0016 returned `SM_ERR` in C++ in a fresh
  process and `SM_SUCCESS` after other extensions. The generator emulates the side
  sequence with the Rust port and skips a case when any side fails at the full distance.

## topology.brep_cache (partial; PR #115, 2026-10-15)

- `SmBrepCache` polygon tree construction, type/accessor queries and exact
  vertex/line-edge resizing compare in 663 cases: 662 bit-identical passes,
  zero failures, one explicit `query.brep` deferral. No absolute floors.
  The source `my_test_extent3d` points are re-expressed as cache geometry;
  fixed regressions and 600 seeded cases cover polygon degeneracies, shared
  boundaries, tree split thresholds, tolerance extremes and scale changes.
- Exact `BuildTrees` stops at `SmBrep::CalculateBoundingBox` (`query.brep`,
  currently PR #101); its exact geometry insertion paths are implemented but
  not certified. The subsequent `SmBrep::GetVertices/GetEdges/GetFaces` calls
  explicitly defer to `topology.brep_p1`; arena allocation order is not the
  B-rep linked-list order and includes unlinked records. Face resizing needs
  `SmFace::CalculateBoundingBox`
  (`query.face_p1`, PR #105) and `SmFace::GetEdges` (`topology.face_p4`). Those
  calls retain correctly owned `UNPORTED` markers. `BuildPolyTrees`, `GetBBox`,
  drawing and memory accounting are not functions assigned to this unit.
- The checked FFI retains native negative-tolerance (`fabs`) and
  `SM_USE_DEFAULT` handling. A separate native differential regression compares
  full Rust/C++ cache outputs for both; exact fixture tolerances go through
  `zone_tol3d` just like their native typed arguments.
- Geometry owner keys need not equal arena indices. Build/resize and the solver
  adapter take an explicit identity mapping; resize cases use keys offset by
  1,000,000,000. Cache invalidation and source identity remain with the caller,
  as for existing curve/surface cache factories. `geom::tools::find_face` now
  takes the caller's kernel, B-rep id, owner keys and intermediate-owner resolver;
  it never creates a default context or assumes arena-index keys. The solver
  adapter uses that B-rep id for shape membership. `Curve::edge` follows the
  supplied generated-curve/edgeuse owner chain in both resizing and solving;
  a native regression compares chained and direct ownership. Reverse edge/face
  lookup deliberately scans arena ids using the caller's key mapping: it resolves
  identity, not B-rep list membership, and does not affect traversal order.
  Native surface/curve containment uses zero tolerance, vertices use `EFF_ZERO`.
  `CacheSource` and typed object handles avoid references from the cache back into mutable geometry.
- The shim builds exact resize edges with `MakeShellVertex` plus
  `MakeWireEdgeVertex`, preserving the two distinct endpoints supplied to the
  Rust fixture. `CreateWireEdgeFromCurve` would intentionally `GlueVertices`
  below the model tolerance, changing the graph and the final vertex tolerance
  when the two endpoint setters touch the same vertex. This fixture correction
  cleared 53 small-scale mismatches; no comparison tolerance was changed.
- All incoming `UNPORTED(topology.brep_cache)` sites are wired. The object-only
  SAG validator now requests source geometry for new caches, matching the
  existing curve/surface factory convention. Its runner calls the real cache
  factory; 131 cache deferrals now correctly name `query.brep` (total remains
  168, including the 37 `topology.shell` cases). Generic topology adapters can
  expose their cache through `TopologyObjects::brep_cache`; existing explicit
  tree adapters continue to work. Exact end-to-end B-rep solves still await the
  dependencies above.

## topology.trimming_tools (`SmTrimmingTools`, `SmCurveLoops`, 2026-10-15; partial, no disagreements)

- **What is compared.** Geometry ops on real `SmBSplineCurve`, `SmLine`, `SmEllipse`
  (full ellipses as one-curve loops) and quadratic circle arcs, and on `SmBSplineSurface`
  (open, closed in u or v, pinched, C0 kinks, rational), `SmPlane` and
  `SmCurveBoundedSurface` (no boundary curves): `OrderCurvesIntoLoops` followed by
  `FindOuterLoops` (ordered indices, loop counts, orientations, plane point and normal,
  box, outer loop counts; the upstream tests `TestSmOrderCurves`,
  `TestSmCreatePlanarFaces`, `TestSmCreatePlanarCircle` re-expressed as data),
  `FindOuterLoops` on given loops, `MirrorCurve` (the resulting NURB), both `IsIsoCurve`s,
  `IsIsoCurveOnSeam`, `SurfaceContinuityCheck`, `sm_IsLoopUVGapWithinTolerance` (copied
  verbatim into the shim), `Min`, the null-region error of
  `TrimSurfaceWithModelSpaceCurves`, and scripts of `SmCurveLoops` /
  `SmIsoXSectSegment` / `SmCurveIntersections` operations (new iso-curve copies compared
  by their interval and five samples). All bit-identical (max_rel_err 0).
- **Conic UV trim curves** (review of PR #124). `SmEllipse`, `SmHyperbola` and `SmParabola`
  are `SmBSplineCurve`s: `MirrorCurve`, both `IsIsoCurve`s, `IsIsoCurveOnSeam` and the
  `EditEndPoint` repairs run the inherited `SmBSplineCurve` methods on them. The port
  first rejected them and the shim's guard hid `SmLine` / `SmEllipse` from the UV curve
  ops; `rv_ellipse.json` (ellipses incl. inside-out arcs and 1e-6 radii, lines) now
  passes bit-identical.
- **B-rep ops** use the `topology.vertexuse` B-reps (the 15 `.smb` files of that runner
  and generated faces) with the same geometry normalisation as `heal.gap` (analytic face
  surfaces and edge curves replaced by plain B-spline copies before the op):
  `AreUVCurvesPresent`, `GetEdgeUVExtremes`, `CheckLoop` (closure, status and the assert
  reports), `CheckFace` up to its first loop orientation, `FixTrimLoopTinyParameterSpaceGaps`,
  `EnsureLastEdgeuseUVCorrect`, `FixBackwardsOuterLoopOnSeam` (every edge use's UV trim
  curve compared afterwards). An optional pre-step mirrors one UV trim curve of the loop
  across the face surface's domain with `MirrorCurve` on both sides, so that loops get
  seam jumps and large UV gaps: it makes `FixBackwardsOuterLoopOnSeam` repair loops and
  `CheckLoop` fail with closure 2 or 1.
- **Shim guards.** The shim skips (reports `-2`) what upstream would dereference null:
  loops without loop use, edge uses with neither a UV trim curve nor vertex uses, faces
  without surface, and in `FixTrimLoopTinyParameterSpaceGaps` a gap to close at an edge
  use without UV trim curve (found by a dry run of the gap test; the edits of a pair
  touch only that pair's curve ends). `FindOuterLoops` reads past its arrays when a loop
  has zero area (UB-119): the shim detects the dropped curves and reports `-2`, the port
  returns `InvalidInput`.
- **`sm_IsLoopUVGapWithinTolerance` on a plane** extracts from `GetOrCreateGwNurbPointer`:
  the port uses `Plane::base_with_nurb` (a plane made by `Plane::from_normal` has no NURB
  until asked; with the raw base the certification was `false` where the C++ said
  `true`).
- **Deferred:** everything past `SmFace::ComputeLoopOrientation` (`topology.face_p3`):
  `CheckFace` after its first `CheckLoop`, `FixTrimLoopOrientation`, `FixTrimLoopOrder`
  (whose `SmEdge::GetUVTrimCurveOfSurface` also defers at
  `SmEdgeuse::GetOrCreateUVTrimCurve`, `topology.edgeuse_p3`); `SplitAtSeams`,
  `ClearUVTrimCurves`, `CreateUVTrimCurves`; the trim of model-space curves
  (`SmBrep::SetTolerance`, `topology.brep_p2`); vertex-use UV points of edge uses without UV
  trim curve (`ComputeUVPoint`'s deferrals). Counts after merging main (PR #52,
  `surface.surface_p3`): 3263 cases, 2842 pass, 421 deferred (`topology.edgeuse_p3` 193,
  `topology.face_p3` 219, `topology.face_p4` 9); the 265 `CheckLoop` cases that deferred at
  `GetSingularities` now pass bit-identical.
- **Back-edges wired:** `SmSurface::CreatePlaneWith3DCurves` (`surface.surface_p2`) now
  calls the ported `OrderCurvesIntoLoops` / `FindOuterLoops` (it takes the kernel); its
  difftest's one `topology.trimming_tools` deferral passes.
- **Upstream bugs** ported as is: WR-96 (`GetLoopStartEnd` start index), WR-97
  (`IsIsoCurve` returns a failed `GetCanonical` as `TRUE`), WR-98 (`MirrorCurve` divides
  rational points by their weights twice); UB-119, UB-120 (null curves) are rejected.
## Merging PR #106 (surface.surface_point_solve_p3) after PR #52 (2026-10-15)

- **Newly compared cases exposed two port bugs.** With `GetSingularities`/`SmFindPSNormalENFO`
  (PR #52) and the point solves (PR #106) both wired, cases that used to defer on one or the
  other now run end to end:
  - `SmSurfPtGlobalSolver::Solve` (`surface.surface_point_solve_p2`) set
    `m_apObjects[0] = m_pSrf` on every `LocalPointSolve` result. Upstream only gets that from
    `LocalPointSolve_1`; the `LocalPointSolve_0` path (MAXIMIZE) leaves the default solution's
    objects untouched. 52 `surface.surface_point_solve_p3` and 2 `surface_point_solve_p2`
    MAXIMIZE cases on planes disagreed on the object slot; the port now sets it only when the
    local solve reports an object (`m_lNumObjects == 1`).
  - `CoverCoincidentSurface` (`surface.surface_p3`) propagated the status of
    `AdjustSTEPUVDomain`; upstream ignores it (`SmSurface.cpp:8073,8266`). Generated case
    gen1979 (a rational B-spline that covers itself) failed with `SM_ERR` from
    `expand_boundary_span` in Rust, while C++ succeeded. The status is now ignored.
- `topology.loopuse`'s `DropPoint` stub (`UNPORTED(surface.surface_p5)`) is wired: with no
  guess and no solution array, `DropPoint` writes `rResultUV` only when it ends with
  `rbSuccess` set (`SE(...)` ignores the status, `SmLoop.cpp:441-442`).
- **`intersect.surface_drop_curve` planes now carry their NURB in the difftest.** With the
  point solves wired, 15 generated plane cases that used to defer on `LocalPointSolve_1`
  compared ULP-level differences (a converged UV one ulp apart, so `m_dSurfNormLineDist`
  near zero disagreed at relative error 1). Cause: C++ `SmPlane`'s const queries make and keep
  the NURB (`GetOrCreateGwNurbPointer` through a `const_cast`), and an `SmPlane` that has it
  takes a different number of `LocalPointSolve_1` steps than a fresh one (same inputs: fresh
  `u = 0.0023719945901309069` after 1 step, after `ComputePointValues` `0.0023719945901309065`
  after 0). The Rust plane never keeps a NURB made by a read-only query (deviation documented in
  `surface::plane::p1`). The shim now calls `GetOrCreateGwNurbPointer()` and the runner
  `Plane::make_nurb()` right after construction, so both sides start from the same state:
  527 pass, 20 deferred, 0 failed.
- **`intersect.surface_drop_lib_p2` (#112) had the same plane NURB state issue.** Its four
  stubs were wired here (`IsSingularity` both overloads and `LocalPointSolve` with `SER` as
  `?` and the caller's kernel, which `sm_AddToCurve` now takes in place of its unused
  `crContext`; `EvaluateGeometric` with its status ignored, curvatures `0.0` on failure).
  Eight generated plane `GetNextPoint` cases then disagreed at ULP level in the same way
  (`GetNextPoint`'s start evaluation makes the C++ plane's NURB). Its shim and runner now
  make the NURB up front too: 1304 pass, 227 deferred (surface.offset_surface), 0 failed.

## query.face_p2 (`SmFace::PointClassify`, `CurveOnClassify`, `sm_ClassifyParamOnFaces`, `SmProtoVertex` containment, 2026-10-15; partial, no disagreements)

- **Harness.** The shim builds the B-reps of unit `topology.face_p2` (upstream `.smb` files,
  `CreateFaceFromSurface` over B-spline surfaces and real `SmPlane`s) and exports them in that
  unit's layout; the runner imports them with that unit's importer. Edits overwrite stored
  face/edge/vertex tolerances (tolerance-limit cases) and build missing UV trim curves
  (`GetOrCreateUVTrimCurve`, which the port cannot do yet: unit `topology.edgeuse_p3`).
- **Trimmed surface caches.** `SmBrep::ReadFromFile` (notably with `RebuildUVTrimCurves`)
  leaves an `SmTrimSrfCache` on some face surfaces, and `PointClassify` then answers from its
  leaf nodes first. B-rep surfaces have no object cache in the port (`SmCacheMgr` has no
  counterpart), so the shim deletes the cache of every face it classifies against: both sides
  classify through the vertices and loops. The cache branch is ported (the caller passes the
  cache) but not compared; with the caches left in place, the 7 affected generated cases agreed
  on the classification anyway.
- **Inexact faces.** Faces of surfaces of revolution / extrusion, cones, spheres and tori (and
  their edge curves) are exported as their B-spline parts only; such cases report
  `inexact_geometry` on both sides and are excluded from the fixtures (`Mouse.smb` faces 0 and
  1, `box_with_large_fillet.smb` face 6, and the single faces of `cone`, `sphere`,
  `surf_extrude`, `surf_rev`, `surf_sphere150`, `torus`, `toruswvy`, `vase_rev`).
- **`SM_ERR` returns** (a loop that cannot classify the point) do not occur on these B-reps;
  the status is compared on every case.
- **Not wired**, still `UNPORTED(query.face_p2)`: `surface::trim_srf_cache`,
  `surface::surface::p4` (`GlobalPointSolve` on a face-owned surface), `intersect::ray_tracer` and `intersect::topology_solver::p1` call `PointClassify` on opaque
  face handles without a `Brep`; `topology::face::p2` and `intersect::topology_solver::p2`
  reach `CurveOnClassify` only after `UNPORTED(intersect.curve_class_p4)`.

## intersect.curve_class_p3 (`SmCurveClassification` part 3: `FindInterval`, `FindPointClass`, `IsCoincident`, `InsertPointClass`, `RemovePointClass`, `MergePointClass`, `CleanupAndValidate`, `sm_ChangeEdgePointClassToNearVertex`, `sm_FixProblemsBySqueezing`, 2026-10-16; partial, no disagreements)

- **Why `UNPORTED(topology.edge)` / `UNPORTED(topology.vertex)` remain** (`SmEdge::GetVertex`,
  `GetInterval`, `GetCurve`, `SmVertex::GetPoint`): the classification refers to objects through
  the opaque handle `O: ClassObject`, as in `intersect::point_class`. A vertex or edge handle
  carries no `Brep`, so the port cannot resolve it to an arena id; the stubs are not waiting on
  the units' ports but on handle-to-B-rep resolution. The shim rejects the scripts that would
  walk a bare stand-in edge (undefined in C++: no edgeuse; UB-109/UB-110 cover the other
  undefined reads).
- **Other stubs** are the maintainer's open PRs: `intersect.curve_class_p2` (PR #85: `InsertAt`,
  `EditIntervalMin/Max`, `ConnectIntervals`, `GetClassifyCrvLoc`) and `intersect.curve_class_p7`
  (PR #65: `GetPointClassByIndex`, `GetPointClassMateByIndex`, `AreCoincidentMaybe`). Cases that
  reach them are `deferred`.
- **`sm_FixProblemsBySqueezing`** ends with a tail call; the port restarts a loop at the same
  point (visit order unchanged), bounded by interval count + 1.

## query.api_queries

- **Native missing-curve crashes (2026-10-16, PR #125).** Clearing an edge's
  curve with `SetCurve(nullptr, TRUE, FALSE, FALSE)` leaves topology that several
  public queries dereference. The review identified closest B-rep queries,
  B-rep relationship/distance, edge/face picking and edge closest point. An
  additional 220 isolated native probes (line/B-spline wires, boxes, a face with
  vertex loops and a holed face; both box modes) confirmed crashes in face closest
  point on boxes and in interior-point/classification/closest-point paths on the
  latter faces. Extending the probes to 308 inputs also found tight B-rep/face
  bounding-box crashes on the curved quadratic patch. These are native SIGSEGVs,
  not Rust/C++ numerical disagreements.
  The FFI rejects those missing-curve combinations before construction; native
  null-input checks and the defined missing-curve edge queries remain testable.
  After the guards were expanded, 396 isolated probes over nine construction kinds
  and both box modes completed without a crash or timeout (192 rejected by guards,
  204 native calls completed).
- **Solution point extraction.** The query adapter follows `SmSolution::GetPoint`'s
  zero parameter advance for a preceding Surface (Face advances by two), and its
  ignored ordinary evaluation failures leave the point undefined. Unported
  geometry still propagates. Two core regression tests cover both behaviors.
- **Null-pointer coverage.** Typed Rust APIs cannot receive native null pointers.
  The 25 native null contracts are FFI tests, not differential passes with a
  synthesized Rust status.

## heal.heal_data_p2 (`SmHealData` p2 steps, `SmEdgeProps`, `SmVertexProps`, problem arrays; 2026-10-16 review, PR #128; partial, no disagreements)

- **Review cases.** `fixtures/heal.heal_data_p2/review_adv.json` (25 cases): edge-length
  histogram buckets 0, 1 (the x86-64 wrapping `(ULONG)floor(log10(a))` path; the C++
  reference build is the oracle for that conversion, as for every existing sub-unit
  edge length) and 10, tiny wires that give real coincident-vertex pairs followed by
  `RemoveVertexProps` / `RemoveEdgeProps` on a live `SmPairProbArray`, incremental
  coincident-vertex re-runs, duplicate targets, 1e4 and mixed-scale boxes, a 1-voxel
  flat box, `ResetEdgeProps(FALSE)` / `ResetVertexProps` re-runs. 21 pass bit-identical,
  4 deferred (`topology.edgeuse_p3`, box vertex/face gaps), 0 failed.
- **Coverage gaps (not compared yet).** No case sets `m_bBadUncontainedEdge`, so the
  `SetMin`/`SetMax` clamping of `Fix_UncontainedEdges` is never compared: the
  `topology.vertexuse` wire builder rejects an edge interval outside the curve domain.
  It needs an `.smb` fixture or a shim kind that builds such an edge. Vertex
  `m_bBadSmallZoneTol3d`, vertex `m_bWarnLargeGap3d` and face Small/LargeZoneTol3d are
  never true: they need vertex/face gaps, deferred on `topology.edgeuse_p3`; re-run when
  it lands. Line-by-line reading finds the Rust matches upstream on these branches.
- **Analytic geometry.** The shim (like `heal.gap` and the shared topology import)
  copies circles, ellipses, spheres, etc. as B-splines, keeping only `SmLine` and
  `SmPlane`. This unit does not branch on geometry kind itself, but its callees
  (`ApproximateLength`, `IsDegenerate`, `GetNaturalInterval`, the gap computations) do,
  so their analytic paths are not exercised through this unit.
- **`Cache_CoinVertices` no-work path** leaves `*pbNewProbs` unwritten upstream; the
  port returns `false` (documented in the file's Deviations; `HealTgts` initialises the
  flag to `FALSE`, so no observable difference).
- **Test-only merge.** The branch carries PR #122's (`heal.face_props`) head 845627d3,
  whose three unit-test fixes are needed for `cargo test -p solidcore`. Merge #122 first
  (or with this PR) so the CO-PORTED types are not duplicated.

## topology.proto_topology_p2 (`SmProtoTopology.cpp` part 2, PR #114, 2026-10-15; partial, no disagreements)

- **B-rep access without UNPORTED stubs into partial units.** The members these functions
  read or write through `SmEdge*` / `SmFace*` / `SmVertex*` (`GetInterval`, `GetCurve`,
  `GetTolerance`, `GetUVDomain`, `SetUVDomain`, `GetSurface`, `GetEdgeuseOfFace`,
  `IsConnectedToFace`) and the allocation of curve copies go through the new trait
  `topology::proto_topology::ProtoBrep` (extends `ProtoGeometry`), implemented by the owner
  of the handles. A `topology::Brep`-backed implementation is the natural next step when
  the topology intersector is ported. The difftest pool implements it with the members the
  shim gives its stand-ins (new pool codes: an edge with interval and zone tolerance, a
  vertex with zone tolerance). `SmTol::GetZoneTol3d` of a vertex/edge/face uses the trait;
  of anything else `intersect::point_class::object_zone_tol_3d`.
- **Harness.** The proto-script engine of `topology.proto_topology_p1` is shared: ops 91 to
  108 run in its shim (`run_p2_op`) and are interpreted by
  `crates/difftest/src/units/topology_proto_topology_p2.rs::run_op` (the p1 runner exposes
  its engine `pub(super)`; its own generator and report are unchanged). The shim refuses
  exactly the calls that would crash or read uninitialised values, worked out by replaying
  the call's control flow with const queries (`sm_FindPositionFromPCs` on copies, the
  pairwise resolve loops with `CanCombine*`, the `GetFinalVertex` calls, the evaluations);
  the port reports those as `InvalidInput`, which the interpreter turns into the same
  refusal, so a refusal on one side only is a mismatch. Two conservative guards are
  mirrored explicitly: duplicated entries in a list a resolve deletes from, and zero-length
  intervals where a reparameterisation can follow (HANG-12). Objects a call makes are
  numbered after the op in the port's order (`register_new`; only new definitions carry
  new curves, so a curve freed by `Destruct` is never re-registered).
- **Not comparable on bare stand-ins (deferred or refused):** `MergeIntoObject` on
  regions/faces/edges (no B-rep; `topology.brep_p9`), `MakeEdgeInFace`
  (`topology.brep_p5`), `UpdateFaces` (`CalculateUVDomainFromUVTrimCurves`,
  `topology.face_p1`; the shim never runs it), point classifications on bare topology
  (`FindObjPoint3d`/`GetTolerance` of edges, vertices, faces in `intersect::point_class`,
  dispatch gaps there), `SmProtoVertex::ContainsPoint` (`query.face_p2`).
- **Deviations:** upstream leaks the proto edge / curve copies on the error returns of
  `CreateProtoEdge` and `AddEdgeDefinition(crCI1, crCI2)` (and returns the leaked proto
  edge in `rpNewPE`); the port makes nothing on those paths, and the shim reports `-1`
  for the proto edge on an error return. `FindMatchingEdgeDefinition`'s `rbRev` is always
  `FALSE` and dropped. `UpdateEdges`' `cbiDoSplit` branch (`static constexpr FALSE`) is not
  ported.
- **Upstream defects:** UB-115 (`CreateVertexDefinition` / `CreateProtoVertex` read an
  unset position and gap), UB-116 (`AddEdgeDefinition` reads uninitialised tangents),
  UB-117 (`UpdateEdges` dereferences its optional calling definition), UB-118 (null and
  deleted definitions in the two `Resolve`s), WR-95 (`CombineProtoEdge` moves every other
  definition; confirmed by `edge_pe_combine_every_other_*`).

## surface.cone_p3: analytic intersections and dispatch handoff (PR #131)

The cone part-3 port keeps complete analytic and NURBS intersection curve records,
UV curve presence, classifications and deviations in its differential protocol.
The direct methods operate on bare geometry; face-owned tolerance context is not
available through the standalone `Cone` type.

Two unfinished callees are isolated in `surface/cone/p3.rs`:

- `ellipse_trim_intervals`: wire to `surface.cone_p2`'s
  `sm_GetEllipseTrimIntervals` when PR #71 lands. This enables oblique elliptical
  plane sections and the trimmed ellipses of intersecting equal-radius cylinders.
  PR #71 currently takes `&Ellipse`, while this stub retains raw center/axes/radii;
  whichever PR lands second must adapt the signature and rerun cone part 3.
- `boundary_intersections`: #120 merged `Surface::intersect_surface_boundaries`,
  but its operands require a `SurfaceKind::Cone` adapter absent from official main.
  This adapter belongs to active #76/#133; the stub now names `surface.surface_p2`.
  Do not substitute a B-spline/revolution wrapper: it erases Cone/Cylinder virtual
  methods. Once the typed adapter merges, wire the boundary API and rerun every
  formerly deferred case without claiming its output is certified in advance.
  The owned caller already retains upstream's second-call status overwrite, duplicate
  removal order, coincident classification and UV-array ordering.

The global dispatch entry is `Cone::global_surface_intersect_with`. Its general
intersector callback receives the original cone, both domains, the other surface,
both edge flags and both optional tolerances. This preserves cone virtual methods
until the shared enum can hold cones with the caller's Kernel (issue #76); it does
not substitute a default Kernel or a plain B-spline/revolution. Plane, cone,
revolution and extrusion dispatch are present. Sphere dispatch and integration in
`Surface::global_surface_intersect` await their concrete enum kinds. In particular,
upstream's `SmCylinder_TYPE` other operand intentionally does not match its
`SmCone_TYPE` switch arm.

Resolved regression `cylinder_ends_near_touch`: two radius-1, height-3 cylinders
with origins `(0,0,0)` and `(1,0,3.00000005)`, full sweeps and approximation tolerance
`1e-7`. C++ returns success with no curves; the first Rust translation returned
`SM_ERR` (1001). `SmSurfOfRevolution::STEPInversion` explicitly writes EXTERIOR
when its generator solve returns no solutions, before returning SM_ERR. The native
intersection ignores that status and skips the EXTERIOR result. The port now
recognizes that specific no-solution error and skips it without fabricating or
reading an unwritten UV coordinate. Other errors, including Unported, propagate.

### Apple Clang/libc++ baseline limitation observed during PR #131

The complete `difftest --green` run on the Mac aborts with SIGABRT (exit 134)
after `nlib.math`, inside the existing `topology.database_io` unit. This is
independently reproducible without running cone code:

```sh
nice -n 10 bin/difftest/difftest --unit topology.database_io --filter gen0185 --no-report
```

Generated seed: `573100365250327377`. Running the 59 database fixtures separately
also shows libc++ numeric-extraction differences: `surface_sms_ascii`'s first
compared double is C++ `-7777.25` (the extraction sentinel) versus Rust
`4.404800473510645`; overflow/underflow, incomplete exponents and hex extraction
fixtures disagree too. The Rust reader explicitly implements libstdc++ semantics.
The source of `SmDatabaseIO::ReadType` contains unbounded extraction into
`char sBuff[SM_MAXSIZE]`; this is a candidate for the generated abort, but an abort
backtrace was not obtained. No reader semantics, fixtures or tolerances were
changed to hide these platform differences. Full certification of the project
suite still requires the supported GNU C++/libstdc++ CI environment.

Local validation used separate Clang compatibility patches; those patches are
excluded from this unit PR after scope review and preserved at `d8d20ff1` on
`support/apple-clang-harness`. The two affected harness units passed independently:
`geom.geom_utility_p1` 4185/4185 and `curve.projected_curve` 2727/2727, both
bit-identical. `surface.cone_p3` passes 1187 cases with 121 dependency deferrals;
a 10108-case stress run has 9048 passes, 1060 dependency deferrals and zero
failures, all compared values bit-identical. Perturbation detects every one of
1187 comparable default cases.

Crossing-axis coverage uses equal radii and an axis intersection inside both finite
segments, including generated tiny/huge scales and subdomains; these cases reach
`sm_AddEllipseIntervalsTrimmedToTwoCones` and explicitly defer on cone part 2.


### PR #131 review follow-up against earlier ports

Review examples: [#130 resource-exhaustion note](https://github.com/bherbruck/solidcore/pull/130#issuecomment-6206051059),
[#125 native-guard and status review](https://github.com/bherbruck/solidcore/pull/125#pullrequestreview-5540211429),
and [#123 branch/order/perturbation review](https://github.com/bherbruck/solidcore/pull/123#issuecomment-6205894305).

Found and fixed an overbroad native guard: `GlobalSurfaceIntersect` switches on
exact `SmCone_TYPE`; a `SmCylinder_TYPE` target goes to the general intersector.
The missing-optional-tolerance guard now follows that dispatch before checking the
coincident analytic-cylinder path. A native regression uses two coincident
radius-1, height-3 surfaces with the second constructed as `SmCylinder` and no
optional tolerance; the old guard returned `SC_ERR_ARG`, while the corrected
adapter completes the actual native general call. The Rust callback dispatch has
an existing matching regression. This native safety test is not counted as a
Rust/C++ numerical comparison.

The added native matrix covers direct/global plane and cone calls, both first
operand classes, swap/inside-out flags, full/subdomains and omitted, zero,
positive, negative and system-default tolerances (320 calls). Fourteen constructor
boundary probes exercise zero/negative radii and heights, tiny height, zero,
reversed and oversized sweeps, zero axes and scaled axes. Malformed/nonfinite
record checks remain FFI contract tests rather than artificial difftest passes.
All `solidcore-ffi` tests were run locally with `cpp`, serially; the default
workspace tests do not enable this feature. The proposed CI step was removed
after scope review: this port PR leaves repository workflows unchanged.

The resource audit of `surface/cone/p3.rs` found no link-following walks, recursion,
file readers, input-declared allocation sizes, or numeric convergence loops. Its
loops traverse fixed pairs, returned curve/interval collections, or the existing
output prefix during deduplication. Periodic interval intersection in the ellipse
helper is currently behind the `surface.cone_p2` deferral and must be rechecked
when that dependency is wired. This audit does not certify unported dependencies.

Coverage limits remain explicit: oblique ellipse trimming, intersecting-axis
ellipse output and coincident-boundary deduplication are ported but cannot yet be
compared beyond their dependency entry points. Revolution/extrusion global
branches have Rust implementations but this unit's native runner constructs only
plane/cone/cylinder targets; sphere/shared-enum dispatch remains a handoff gap.
The review did not promote these paths to green or count them as compared.

Review validation: 1308 default cases, 1187 bit-identical comparisons, 121
explicit dependency deferrals, zero failures; all 1187 perturbations detected.
The 10108-case stress run has 9048 comparisons, 1060 deferrals and zero failures.
All 32 new tolerance fixtures compare actual returned geometry/classifications
rather than synthetic errors (`cases_both_errored = 0` for the complete report).
All 33 native FFI tests, formatting, and strict workspace Clippy with the C++
feature pass after the review changes and merge of official main `04a2b106`.

### PR #131 contribution scope

The repository contribution guide requires one PR per port unit. Scope review
removed the proposed CI workflow step and the three shared Apple Clang/runtime
fixes from this PR; existing native safety tests remain in the unit's FFI module.
The platform patches are preserved in commit `d8d20ff1` on the local
`support/apple-clang-harness` branch for separate submission. This PR has no CI
workflow or unrelated-unit shim/build-script delta against official main.

The recorded Mac native runs used those platform patches as build prerequisites.
They do not certify a clean upstream harness build on macOS. The supported
Linux/GNU full gate still requires maintainer approval for the fork workflow.

Independent read-only subagent review found no confirmed numerical or safety
defect in the cone implementation. Its 384 extra horizontal-cut cases at
radii/heights `1e-12..1e12`, with default/zero/negative/large tolerances, produced
294 full curve comparisons and 90 matching constructor-status results, zero
mismatches and zero deferrals. Those exploratory checks used existing compiled
artifacts; they do not replace the committed report or certify the scoped head's
clean native build. The review confirmed the documented deferred/uncovered paths
and recommended the CI/platform scope separation above. Formatting and strict
plain workspace Clippy pass after that separation.

### PR #131 maintainer review follow-up

[Review 5543522828](https://github.com/bherbruck/solidcore/pull/131#pullrequestreview-5543522828)
reported 1308 cone cases (1187 bit-identical passes, 121 dependency deferrals),
181 units with zero failures in the maintainer's full GNU run, and 212 extra
adversarial cases (174 exact comparisons, 31 deferrals, seven guarded refusals,
zero mismatches). These are the reviewer's results for the prior numerical
revision, not certification of later edits or the latest CI run.

1. Workflow and build-script changes were already removed at `20189dd0` and
   remain absent. The maintainer explicitly reserves those changes for a separate
   maintainer PR.
2. The optional-tolerance null dereference is now UB-123 under Crash / undefined
   behaviour, with standard How found / Confirmed fields. It remains source-found
   and unconfirmed by execution of undefined behaviour.
3. The SSI corpus builder's missing cone/cylinder variants now name
   `surface.surface_p2`, matching the adjacent torus dispatch gap. A source check confirms no
   `UNPORTED(surface.cone_p3)` markers remain in `crates/`.
4. Coincident-boundary deduplication checks both result bundles' UV, type and
   deviation vector lengths against their 3D curves before removing anything.
   The regression covers each vector being shorter/longer in either bundle
   (16 malformed combinations), returning `SolidError::sm_err(OP)`.
5. The general callback's result replaces the analytic result, matching
   `SmSurfaceIntersector::DoIntersection` resetting all outputs. No currently
   compared path returns analytic curves and also requests general intersection.
6. The module Deviations now state that start-plane transform failures propagate
   in Rust, while native STEPInversion initializes EXTERIOR before that transform
   and its cone caller skips the failed point. This is the reviewer's permitted
   documentation option, not a newly claimed parity fix.

After merging official main `7962b462`, all 1192 kernel tests pass, including the
new malformed-boundary regression. Formatting, strict plain workspace Clippy
and the wasm32 check also pass. Native revalidation uses the separate disposable
checkout and platform prerequisites described above; they are not submitted.

Fresh native merged-tree comparison passes: cone_p3 1308 / 1187 / 0 / 121,
surface_p2 869 / 858 / 0 / 11, and trimming_tools 3263 / 2842 / 0 / 421
(total / pass / fail / defer). All three reports reproduce byte for byte.
Sensitivity detects 1187/1187, and the 10108-case cone stress run has 9048 exact
passes, 1060 dependency deferrals and zero failures. The focused three cone tests
pass after adding a well-formed, unequal-curve-count bundle pair to the boundary
contract regression. The simple SSI ownership retag is checked directly rather
than adding a test that only repeats its static labels.
Strict C++-feature workspace Clippy also passes in the disposable validation
checkout. The platform prerequisites remain excluded from the submitted branch.
## surface.surface_p11 (`LocalSurfaceIntersect`, `SmCSGlobalSolver` / `SmSSGlobalSolver::LocalSolve`, `IntersectSurfaceBoundaries`, `LiftCurve`, PR #120, 2026-10-15; partial)

- **Known disagreement (not generated): boundaries of an `SmPlane`.** In
  `IntersectSurfaceBoundaries` the other surface's boundary curves come from
  `CreateIsoBoundaries`; for an `SmPlane` upstream they are `SmLine`s (the plane's
  `CreateIsoParametricCurve` override), while the port's `Surface::create_iso_boundaries`
  (unit `surface.surface_p9`) returns `BSplineCurve`s. Copy, trim and evaluation then run
  as B-splines: plane/plane cases differ near zero coordinates (e.g. `cpp=0`,
  `rust=2.78e-17`, relative error 1; four of the first 600 generated cases with a plane as
  the other surface). The generator therefore uses patches for the other surface (this
  surface may be a plane). Fix belongs to `surface.surface_p9`: return the typed iso curve
  (`SmLine` / `SmCircle`) from `create_iso_boundaries`.
- **Global solves wired.** `Surface::global_curve_solve` / `global_surface_solve` (unit
  `surface.surface_p7`) now take the kernel and the solution handles, build the surface
  (and curve) caches for the solve (no cache store: `CacheMakeOrValidate`, unit
  `surface.surface_p1`) and run `SmCSGlobalSolver` / `SmSSGlobalSolver` with the leaf
  solves of this unit; the solvers borrow the caches (`CsGlobalSolver::curve` /
  `surface_cache`, `SsGlobalSolver::caches`), as `SmLSIGlobalSolver` does.
  `SmSSGlobalSolver::BranchMayContainAnswers` uses the caches' polar boxes when it has
  them. `global_surface_solve_with` also returns the solution nodes' boxes and patch
  domains (`SM_SR_NODES`, read by `SmSurfaceIntersector::FindInteriorCurves`, now wired).
- **Unported callees** (deferred): `sm_IntersectSrfBndryCrv` and `sm_ExtractIsoCurve`
  (`surface.surface_p8`; `LiftCurve` defers at its first call) and `LocalPointSolve`
  through `DropPoint` of a B-spline in `IntersectSurfaceBoundaries`
  (`surface.surface_point_solve_p3`). `SmSurface::LocalSurfaceSolve` is wired to unit
  `surface.surface_p3` (merged).
- **Dispatch gap: `LiftCurve` on a surface that is not a B-spline.** `SmCrvOnSurf` holds
  any `SmSurface` upstream; the ported `CrvOnSurf` (unit `curve.crv_on_surf`) holds an
  `SmBSplineSurface`, so `LiftCurve` of a plane, surface of revolution, curve-bounded,
  in-volume or STEP surface returns `Unported` naming `curve.crv_on_surf` past the iso
  curve test (as the `SmIsoCurve` gaps of parts 5, 6 and 9). Unreachable in the difftest
  until `sm_ExtractIsoCurve` (`surface.surface_p8`) lands.
- **Unbounded surface cache subdivision (fixed).** A difftest run of this unit with
  `surface.surface_p7` reached 3.5 GB RSS within 84 s and aborted ("memory allocation of
  3523215360 bytes failed": the `BezierPatch` array of a `SurfaceCache` growing past 2^21
  patches in `SubdivideToTolerances`). The `surface.surface_p7` generator ran the Rust
  side of each candidate case (now a real global solve) before its scale filter, so
  extreme-scale surfaces were subdivided to the default tolerances into millions of
  patches (upstream does the same, for minutes). Fixes: the generator checks both
  surfaces' scales before the Rust run, and `SubdivideToTolerances` (unit
  `surface.surface_cache_p3`) refuses to split once the tree holds
  `MAX_SUBDIVISION_NODES` (2^20) nodes, with `InvalidInput` (a generated
  `surface.surface_p1` `GetDomainWithContinuity` case at `1e-4` scale needs more than
  2^18; no fixture comes near 2^20). Peak RSS of the `surface.surface_p7` run is now
  about 80 MB.
- **Global solve comparison.** The `surface.surface_p7` runner's `GlobalCurveSolve` and
  `GlobalSurfaceSolve` cases compare the first 16 solutions (type, value, variables, end
  parameters of a range), not only their count. Its generator covers minimize, maximize,
  normalize and signed directed minimize curve solves and generated surface/surface solves
  (a bumpy patch lifted 0.05..2 above a B-spline surface's control hull, so minimizing
  never meets a continuum of zero distances; intersect for the `SM_SR_NODES` node pairs on
  crossing surfaces). `SM_SO_3D_SIGNED_DIRECTED_MAXIMIZE` is not generated: it is not one
  of the operations `sm_SCLocalSolve` accepts (its `SM_ASSERT`; the Rust side returns
  `InvalidInput`, the release C++ carries on), and a trial of the generator with it gave
  13 of 3660 `surface.surface_p7` cases that disagreed for exactly that reason.
- **Upstream issues:** UB-121 (`IntersectSurfaceBoundaries` dereferences a NULL curve
  array), WR-99 (`LocalSurfaceIntersect` boundary retries: `!bFoundSolution` binds to
  the minimum test only; reproduced).


### PR #131 after #120 merged (official main `0d6885b2`)

Merged main and preserved both findings sections. `Surface::intersect_surface_boundaries`
is now available, but it accepts `Surface` operands and main still lacks
`SurfaceKind::Cone`. Wiring via the B-spline/revolution base would discard native
Cone/Cylinder virtual dispatch. The remaining adapter belongs to active issue #76 /
PR #133; no part of that claim is duplicated here. The cone boundary stub and its
30 report deferrals now name `surface.surface_p2`, not the already-ported p11 unit.
The p11 findings above also record a typed iso-boundary limitation; reassess that
coverage when the cone adapter is wired rather than certifying it in advance.

Fresh merged-tree checks: 1194 kernel tests, formatting, strict plain workspace
Clippy and wasm32 pass. Native comparison gives:

```
PARTIAL surface.cone_p3 cases=1308 passed=1187 failed=0 deferred=121 max_rel_err=0e0 blocked_by=surface.cone_p2(91),surface.surface_p2(30)
PARTIAL surface.surface_p2 cases=869 passed=858 failed=0 deferred=11 max_rel_err=0e0 blocked_by=surface.cone_p2(10),surface.surface_p8(1)
PARTIAL surface.surface_p11 cases=600 passed=498 failed=0 deferred=102 max_rel_err=0e0 blocked_by=surface.surface_p8(102)
PARTIAL topology.trimming_tools cases=3263 passed=2842 failed=0 deferred=421 max_rel_err=0e0 blocked_by=topology.edgeuse_p3(193),topology.face_p3(219),topology.face_p4(9)
SENSITIVE surface.cone_p3 perturbed=1187 detected=1187 unperturbed=0 deferred=121
PARTIAL surface.cone_p3 cases=10108 passed=9048 failed=0 deferred=1060 max_rel_err=0e0 blocked_by=surface.cone_p2(803),surface.surface_p2(257)
```

Only the 30 cone dependency records and their blocker summary change; compared
values and all three other reports reproduce committed bytes. No deferred path
is counted as a pass. Native validation used the separate disposable Mac checkout
and the excluded platform prerequisites. The first runner build exhausted local
disk space before comparisons ran. After clearing this thread's old build cache,
the retry used `--config 'profile.difftest.package.difftest.opt-level=0'` for the
runner only; kernel and C++ build settings are unchanged. No configuration change
is submitted. Full clean Linux/GNU certification remains outstanding; the known
independent Apple libc++ database abort was not rerun.

Strict C++-feature workspace Clippy also passes in the same disposable checkout
with the runner-only profile override. No platform prerequisites enter this PR.
