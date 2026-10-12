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
