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
