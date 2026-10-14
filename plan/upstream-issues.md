# Upstream SMLib issues

Defects and hazards in the upstream C++ (`source/SMLib`, `source/SM_API`) found while
porting it to Rust. Problems only: porting notes, harness details and Rust-side choices
stay in `plan/findings.md`. Behaviour that is odd but plausibly intended is left out.

- **Upstream commit:** `c9c5979` (NVIDIA usd-brep). Paths are relative to the upstream
  root. Line numbers were checked against that commit.
- **Security.** Entries marked **[PSIRT]** are crafted-file memory or stack exhaustion,
  or out-of-bounds or uninitialised reads that file input can reach. Per upstream
  `SECURITY.md`, report these to NVIDIA PSIRT (psirt@nvidia.com or the web form), not as
  public GitHub issues. The other out-of-bounds entries need a bad argument from the
  calling code. Whoever files them should still decide whether they count as security
  issues.
- **Fields.**
  - *How found:* "read during porting" means source reading only. The other values are
    "generated difftest case", "adversarial / edge-case fixture", or "crash in the C++
    reference run" (the difftest harness linking the upstream build).
  - *Confirmed:* **yes** only when a C++ run showed the behaviour: a crash, or a named
    difftest case or fixture that compares against it. Otherwise **no**. In most "no"
    cases the harness deliberately keeps that input away from the C++ side.
- **Build.** The reference build is the upstream release configuration (`-O2`, no
  `SM_DEBUG_CODE`, `SM_USE_OLDTOL`) with `-ffp-contract=off`, on x86-64 Linux / LP64.

Severity order: crash/UB, hang, nondeterminism, wrong result, docs. Entries are grouped
by area within each severity.

## Crash / undefined behaviour

### File reading

#### UB-1. `SmTopologyData::ReadFromDB` crashes on old ASCII `.smb` files [PSIRT]
- **Location:** `source/SMLib/src/SmBrepData.cpp:4307`, `SmTopologyData::ReadFromDB` (def. `:4261`). Reached from `SmApiReadBrepFromFile` (`source/SM_API/src/SmApiImportExport.cpp:12`) and `SmBrep::ReadFromFile` (`SmBrep.cpp:31982`).
- **What happens:** Reading `DB version 30` ASCII files writes through a NULL `SmShellData` (the `lDBVersionNumber <= 31` branch). Depending on heap state, the read either segfaults or fails with a non-success status.
- **Trigger:** `SmApiReadBrepFromFile` on `TestFiles/pt_TestFiles/SmallBox.smb` or `Platform.smb` read as ASCII.
- **How found:** crash in the C++ reference run (geom.object, geom.tools fixtures).
- **Confirmed:** yes (intermittent; depends on heap layout).
- **Severity:** crash/UB.
- **Cause:** These files are part files (`ReadPartFromFile`, version 32). Read as version 30 the ASCII extractions fail, the uninitialised section count `lCount` reaches `SetSize` as `SM_BIG_ULONG` or more, `SetSize` refuses it and leaves the array `NULL`, and the record loop calls `ReadFromDB` on `NULL + i` (see UB-75).
- **Rust port:** `topology.brep_data` reads B-reps without this: unreadable counts are bounded by the input left and return `InvalidInput`. Its difftest reads both files correctly as version 32 (fixtures `part_SmallBox_db_v32`, `part_Platform_db_v32`).

#### UB-2. Readers allocate whatever counts a file header declares [PSIRT]
- **Location:** `SmBSplineSurface.cpp:6367` `SmBSplineSurface::ReadFromDB`, `SmBSplineCurve.cpp:7237` `SmBSplineCurve::ReadFromDB` and the other `ReadFromDB` readers. Also `SmCompositeCurve.cpp:4169` `SmCompositeCurve::ReadFromDB`: an unreadable ASCII segment count leaves `SM_UNDEF_ULONG` (123456789), which is then `calloc`'d and read with uninitialised locals.
- **What happens:** Counts from the header go straight to `malloc`/`calloc` and are not checked against the data left. `n + p + 1` can overflow. ASCII readers keep going with stale values after a missing field.
- **Trigger:** A `.smc`/`.sms` record whose header declares e.g. `n = 2^40` control points followed by a few bytes.
- **How found:** read during porting (commit security review of `nurbs.b_spline_surface_p1`).
- **Confirmed:** no.
- **Severity:** crash/UB (memory exhaustion, integer overflow).
- **Rust port:** Checks each count against the bytes left (`DatabaseReader::ensure_available`) after upstream's own validation, and returns `InvalidInput`.

#### UB-3. Polymorphic curve readers recurse without a depth limit [PSIRT]
- **Location:** `SmCurve.cpp:23709` `SmCurve::ReadFromDB` dispatching to `SmProjectedCurve.cpp:690` `SmProjectedCurve::ReadFromDB` (and to the offset and iso curve readers), which read their base curve through `SmCurve::ReadFromDB` again.
- **What happens:** Each nesting level costs a few bytes of input and one stack frame chain, so a crafted file overflows the stack.
- **Trigger:** A file of N nested projected-curve records with N large (thousands).
- **How found:** read during porting (commit security review of `curve.projected_curve`).
- **Confirmed:** no.
- **Severity:** crash/UB (stack exhaustion).
- **Rust port:** Limits nesting to 64 levels (`DatabaseReader::enter_nested`) and returns `InvalidInput` for deeper input.

#### UB-4. Binary short reads return success with uninitialised values [PSIRT]
- **Location:** `SmDatabaseIO.cpp:566`/`:598` `SmDatabaseIO::ReadLong`, `:473` `SmDatabaseIO::ReadLongs` (`version >= 40` path).
- **What happens:** When the data ends, the reads land in uninitialised locals and the functions still return `SM_SUCCESS`. Callers then use the garbage, for example as counts.
- **Trigger:** A binary stream truncated inside a `long` field.
- **How found:** read during porting.
- **Confirmed:** no. The difftest compares only the status there.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-5. `ReadType` overflows its 256-byte buffer unless libstdc++ bounds the read [PSIRT]
- **Location:** `SmDatabaseIO.cpp:400`, `SmDatabaseIO::ReadType` (`istream >> char*` into `sBuff[256]`).
- **What happens:** An ASCII word longer than 255 bytes overflows `sBuff`. GCC 11+ libstdc++ only caps the extraction under `__OPTIMIZE__`, so `-O0` builds and other standard libraries overflow.
- **Trigger:** An ASCII record whose type word is 300 non-space bytes, read by a `-O0` build.
- **How found:** read during porting.
- **Confirmed:** no. The `-O2` reference build caps the read.
- **Severity:** crash/UB.
- **Rust port:** Caps the word at 255 bytes, as the optimised build does.

#### UB-6. Other ASCII readers use uninitialised locals after a failed extraction [PSIRT]
- **Location:** `SmAttribute.cpp:210` `SmAttributeData::ReadIndexedAttributesFromDB` (uninitialised count when the stream is already failed or at EOF; a count near `ULONG_MAX` wraps `SetSize`); `SmLine.cpp:2505` `SmLine::ReadFromDB` and `SmBSplineCurve::ReadFromDB` (short header leaves `n`, `p` uninitialised before the allocation); `SmEllipse.cpp:1488` `SmPolarConversion::ReadFromDB`; `SmConic.cpp:905` `SmConic::ReadFromDB` (uninitialised `lConicType`); `SmCrvInVolume.cpp:1404` `SmCrvInVolume::ReadFromDB` (dereferences a NULL volume for `SmVolume_TYPE`).
- **What happens:** Once `failbit`/`eofbit` is set, `>>` stores nothing, and the readers go on using the unset locals.
- **Trigger:** A record cut short in its header, or holding a non-numeric token where a count is expected.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` for unreadable headers and bounds every count.

#### UB-75. `SmBrepData` readers use uninitialised locals and unchecked `SetSize` [PSIRT]
- **Location:** `source/SMLib/src/SmBrepData.cpp:2933` `SmBrepData::ReadFromDB` (section count `lCount` uninitialised at `:2942`; every `SetSize(lCount)` unchecked); `:3245` (`lEdgeCount`, `lEdge` uninitialised in the composite edge loop); `:5098` `SmEdgeData::ReadFromDB` (`dMin`, `dMax` uninitialised); `:3613` `SmBrepData::ReadFromFile` (`lDBVersionNumber` uninitialised when the ASCII header extraction stores nothing; the binary `ReadLong` status is ignored); `:5393` `ReadCurveFromDB` composite branch (`lNumSegments`, `lDimension`, `lClosed`, `dGapStart`, `dGapEnd`).
- **What happens:** After a failed ASCII extraction (stream at its end, or a non-numeric token) these locals keep indeterminate values and drive loops and allocations. `SmTArray::SetSize` refuses sizes of `SM_BIG_ULONG` (99999999) or more and `SM_UNDEF_ULONG` and leaves the array empty or `NULL`, but the record loops still index it up to the count; record `ReadFromDB` statuses are ignored.
- **Trigger:** A truncated ASCII `.smb`; a section count of `100000000` with a few records after it.
- **How found:** read during porting (`topology.brep_data`); UB-1 is one instance.
- **Confirmed:** no (the difftest guard keeps such streams away from the C++ side), except through UB-1.
- **Severity:** crash/UB.
- **Rust port:** Uses `0` for the uninitialised locals, rejects counts the remaining input cannot hold (`InvalidInput`), appends records as they are read, and stops a record loop with `InvalidInput` once the stream has failed or hit end of data before the declared count (otherwise each further record would consume no input and the count bound would not cap memory).

#### UB-76. `ReadCurveFromDB` indexes `rAllAttributes` past its end after a failed attribute line skip [PSIRT]
- **Location:** `source/SMLib/src/SmBrepData.cpp:5480` `SmBrepData::ReadCurveFromDB` and `:5736` `ReadSurfaceFromDB`, after `SmAttribute.cpp:260` (`GOTO_NEXT_ATTLINE` in `SmAttributeData::ReadIndexedAttributesFromDB`).
- **What happens:** When an ASCII attribute line has more than 1000 characters (or a `0xFF` byte) after its indices, `ReadIndexedAttributesFromDB` returns `SM_ERR` with the indices filled in but before it grows `rAllAttributes` and creates the placeholders. The callers ignore the status and read `rAllAttributes[index]` past the array's end (curves then `NER` a `NULL` slot; surfaces skip it).
- **Trigger:** A curve record of a version 31+ `.smb` read with attributes whose attribute line is `1 7` followed by 1001 blanks.
- **How found:** read during porting (`topology.brep_data`).
- **Confirmed:** no for the geometry readers (guarded). The record readers' behaviour (indices kept, no placeholders) is compared by the fixture `region_attribute_line_too_long_v33`.
- **Severity:** crash/UB.
- **Rust port:** Keeps the indices as upstream (`read_indexed_attributes_from_db_partial`), returns `InvalidInput` for an index past the table and `SM_ERR_NULL_POINTER` for an empty slot of a curve.

#### UB-77. Old composite curves: a closed one without segments reads `sGaps[ULONG_MAX]`; nesting is unbounded [PSIRT]
- **Location:** `source/SMLib/src/SmBrepData.cpp:5456` (`new SmCompositeCurve(lDimension, sCurves, bClosed, &sSenses, &sGaps)`) into `SmCompositeCurve.cpp:1090` (`(*cpGaps)[m_lNumSegments-1]`); recursion at `SmBrepData.cpp:5445`.
- **What happens:** A version 32 or older composite curve record with segment count 0 and the closed flag set indexes the empty gap array at `ULONG_MAX`. Each nested composite record recurses into `ReadCurveFromDB` with no depth limit.
- **Trigger:** `Curve Type 14032` / `0 3 1`; or thousands of nested composite records.
- **How found:** read during porting (`topology.brep_data`).
- **Confirmed:** no (guarded).
- **Severity:** crash/UB (out-of-bounds read, stack exhaustion).
- **Rust port:** `CompositeCurve::with_curves` returns `InvalidInput`; nesting is bounded by `DatabaseReader::enter_nested` (64).

### NLib: knots and basis functions

#### UB-7. `N_BasisEval` uses an uninitialised span for an unknown side flag
- **Location:** `NL_BasisBasic.cpp:2899`, `N_BasisEval` (def. `:2838`).
- **What happens:** `N_BasisFindSpan` returns `NL_CAL_ERR` for a flag that is neither `NL_LEFT` nor `NL_RIGHT`. The return value is ignored and the unset span `i` is used to index `U`.
- **Trigger:** `N_BasisEval(knt, p, u, /*flg=*/0, N, &spn)` with `u` strictly inside the domain.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Takes a `Side` enum, so the case cannot be expressed.

#### UB-8. Span and basis routines read outside short knot vectors
- **Location:** `NL_BasisBasic.cpp:1429` `N_BasisFindSpan`, `:2838` `N_BasisEval` (stack arrays `left/right[NL_MAXDEG + 2]` at `:2846`).
- **What happens:** With `m < 2p+1` the routines read outside `U`. With `p > NL_MAXDEG + 1`, `N_BasisEval` overflows its stack arrays.
- **Trigger:** `p = 3` with knots `[0,0,0,1,1]`; or `p = 20`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-9. `N_BasisDerivs` overflows an `int`
- **Location:** `NL_BasisBasic.cpp:3154`, `N_BasisDerivs` (`r *= (p - k);`).
- **What happens:** The factor `p!/(p-k)!` is an `int` and is computed one step past the last row it scales, so it overflows (signed overflow, UB).
- **Trigger:** `p >= 13` with `der >= 10`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-10. `N_BasisDerivsArray` walks spans with no bound
- **Location:** `NL_BasisAdv.cpp:1122`, `N_BasisDerivsArray`.
- **What happens:** Only `u[0]` and `u[n]` go through `N_BasisFindSpan`. The inner spans are found by walking forward with no upper bound, so they read past `U[m]`. The same `int` factor overflow as UB-9 applies.
- **Trigger:** `NL_LEFT` with an inner parameter equal to `U[m-p]` of a clamped vector; or any inner parameter past the end.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-11. `N_BasisInsertKnots` reads one past (or before) `X`
- **Location:** `NL_BasisAdv.cpp:253`, `N_BasisInsertKnots`.
- **What happens:** After the last new knot is inserted, the merge step compares `X[mx+1]`. An empty `X` reads `X[-1]`.
- **Trigger:** Any call: one slot past the end is always read. Or an empty insert vector.
- **How found:** read during porting.
- **Confirmed:** no. The read never changes the output in practice.
- **Severity:** crash/UB.
- **Rust port:** Skips the comparison. Empty `X` is `InvalidInput`.

#### UB-12. `N_GetCompatibleKnotVectorToTol` reads `U[-1]` and can overflow its buffer
- **Location:** `NL_BasisBasic.cpp:2483`, `N_GetCompatibleKnotVectorToTol`.
- **What happens:** The "move to average" test reads `U[-1]` when a vector's first knot is above the current smallest knot. It writes into a buffer of `sum(m)+1` slots per vector, which a vector with fewer than `k-1` knots overflows.
- **Trigger:** Two vectors where the second starts above the first's first knot; or `k` vectors one of which has fewer than `k-1` knots.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Skips the test for such vectors. The short-vector case is `InvalidInput`.

#### UB-13. `N_MakeKnotsCompatible` / `N_GetCompatibleKnotArrayMult` read past `U[m]`
- **Location:** `NL_BasisBasic.cpp:3665` `N_MakeKnotsCompatible`, `NL_BasisAdv.cpp:119` `N_GetCompatibleKnotArrayMult`.
- **What happens:** Both read past `U[m]` on several inputs. Their output buffers are caller-allocated and unbounded.
- **Trigger:** Degree-0 vectors whose ends differ by less than `NL_PTOL`; inner knots within `NL_PTOL` of an end; unclamped vectors.
- **How found:** read during porting.
- **Confirmed:** no. The shim pads with `+inf` so that upstream stays in bounds.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` for the out-of-range reads.

#### UB-14. `NL_PascalTri[18][18]` is indexed past its end
- **Location:** `source/SMLib/inc/NL_Globals.h:67` (the table); users include `NL_Bezier.cpp:105`, `:118` (elevation / product matrices) and `:5417` `N_BezCrvExtend` (hence `N_BezSrfExtend4D`).
- **What happens:** Degree sums above 17 index past the table.
- **Trigger:** Elevating a degree-10 Bezier by 10.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-15. `N_BezReduceDegree` with `p = 1` reads unwritten / negative indices
- **Location:** `NL_Bezier.cpp:2540`, `N_BezReduceDegree`.
- **What happens:** With `p = 1` it reads the never-written `alf[0]` and `Qw[-1]`.
- **Trigger:** `N_BezReduceDegree` on a degree-1 segment.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` for `p < 2`.

#### UB-16. Bezier matrices returned in freed stack storage
- **Location:** `NL_Bezier.cpp:301` `N_BezElevateDegree` (matrix built at `:319` on local stacks freed at `:353`). The same pattern appears in `N_BezSrfElevateDegree` and `N_BezFuncMultiplyBez*`.
- **What happens:** When the caller passes a NULL `NL_RMATRIX`, the matrix is built on a local `NL_STACKS` that `N_EndNurbs` frees before returning. The caller is left holding dangling storage.
- **Trigger:** Call `N_BezElevateDegree` with a NULL `dm`, then reuse `dm`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB (use after free).
- **Rust port:** Builds a local matrix when the caller's is null. Callers wanting reuse build it first.

### NLib: matrices and polynomials

#### UB-17. Banded and triangular matrix routines index outside rows
- **Location:** `NL_Math.cpp:5714` `N_RealMatrixMultiply`, `:6300` `N_RealMatrixTranspose` (e.g. `:6395`), `:6463` `N_RealMatrixMultiplyPtArray` (`:6538`).
- **What happens:** With an even bandwidth, `j - i + bw/2` reaches `bw`, one column past the row. An in-place banded transpose writes past its new `(n+1)`-column rows when `bw > n + 1`. Non-square triangular operands read outside their rows.
- **Trigger:** A banded matrix with `bw = 2`; an in-place transpose with `bw > n + 1`; a 2x3 triangular operand.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-18. `N_SrfPowerBasisEvalDerivs` overflows `up[NL_MAXDEG+1]`
- **Location:** `NL_SrfPoly.cpp:286` (array), `:315` (`vp = &up[p + 1]`), `:333` (write), in `N_SrfPowerBasisEvalDerivs` (def. `:276`). Also reached from the power-basis `SmBSplineSurface` `EvaluateSimple`.
- **What happens:** The v-power rows are stored after the `p+1` u-powers in the same 18-entry stack array. The writes overrun it when `p + 1 + min(vdr,q)*(q+1) + q > 17`.
- **Trigger:** A bicubic surface with `vdr = 3`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB (stack buffer overflow).
- **Rust port:** Returns `InvalidInput`.

#### UB-19. `N_SrfPowerBasisEvalDerivs` rational branch tests a stale variable
- **Location:** `NL_SrfPoly.cpp:491`, `N_SrfPowerBasisEvalDerivs`.
- **What happens:** The final loop tests `k1 EQ k2`. `k1` is a leftover loop variable (`(k2+1)*(q+1)`); the intended test is `udr == vdr`. With `udr > vdr` the loop reads past `SD`/`CwDers`.
- **Trigger:** A rational surface with `mfl = NL_TRUE`, `udr = 2`, `vdr = 1`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Keeps the test so results match. The out-of-range case is `InvalidInput`.

#### UB-20. `SmPolynomial::SolveQuarticEqn` reads an uninitialised root
- **Location:** `SmPolynomial.cpp:365`, `SmPolynomial::SolveQuarticEqn` (def. `:279`).
- **What happens:** `z = adCSol[0]` is taken from the resolvent cubic without checking the root count. With `dZeroTolerance > 1` the cubic degrades to a quadratic that may have no roots.
- **Trigger:** Any quartic with `r != 0` and zero tolerance `> 1`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Uses `z = 0.0`.

#### UB-21. `SmMatrix::Compute3x3EigenVectors` returns uninitialised eigenvectors
- **Location:** `SmMatrix.cpp:1482`, `SmMatrix::Compute3x3EigenVectors`.
- **What happens:** It ignores the `EigenSystemJacobiSolver` status. For a non-symmetric matrix it returns `SM_SUCCESS` with eigenvectors read from an uninitialised `SmMatrix`. It is also `const` but mutates `this` through a cast.
- **Trigger:** A non-symmetric 3x3 matrix.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Reports `SM_ERR`.

#### UB-22. `SmMatrix::LUDecompose` uses a fixed stack buffer it does not check
- **Location:** `SmMatrix.cpp:662`, `SmMatrix::LUDecompose`.
- **What happens:** It scales with a stack `double[MAX_MATRIX_SIZE]` (1000). Only `SolveLinearSystem` (`:843`) checks the size.
- **Trigger:** `LUDecompose` called directly on a 1001x1001 matrix.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Uses a `Vec`. The harness limits sizes.

### NLib: curves, fitting, surfaces

#### UB-23. `N_CrvSumDiffCrv` corrupts the heap for rational sums
- **Location:** `NL_CrvAdv.cpp:10058` `N_CrvSumDiffCrv`; products at `:10136`/`:10142`, sums at `:10155`/`:10160`.
- **What happens:** `N_CrvFuncMultiply*` rescales the knot vector of an operand in place between the two products. When `tmp` comes out with more control points than `num`, the loop over `tmp`'s `n` writes past `num`'s control points.
- **Trigger:** A rational sum of two curves over different parameter ranges, or with interior knots closer than `NL_PTOL` but not equal.
- **How found:** crash in the C++ reference run (`double free or corruption`, `free(): invalid next size`).
- **Confirmed:** yes.
- **Severity:** crash/UB (heap overflow).
- **Rust port:** Returns `InvalidInput`.

#### UB-24. `ST_curxcrr` in place frees an uninitialised curve
- **Location:** `NL_CrvAdv.cpp:4179`-`4182` (`EXIT: if( curP EQ curQ ) N_FreeCrv( &curA, SP );`); `curA` is assigned only at `:3724`. Function `ST_curxcrr` (def. `:3540`).
- **What happens:** An in-place `NL_G2` call that recurses, or fails before `:3724`, reaches `EXIT` and frees stack garbage.
- **Trigger:** In-place `N_CrvExtendByParamDist` with `NL_G2` and a parameter more than `0.9213` ranges outside the curve.
- **How found:** crash in the C++ reference run (SIGSEGV).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Has no such free and completes the extension.

#### UB-25. `N_CrvApproxSpiral` frees uninitialised stacks on its own input errors
- **Location:** `NL_Spiral.cpp:108`, `:111`, `:120`, `:123` (`NL_ERROR` checks) run before `N_InitNurbs(&SL)` at `:148`. The `EXIT` path calls `N_EndNurbs(&SL)`. Function `N_CrvApproxSpiral` (def. `:88`).
- **What happens:** Invalid input jumps to `EXIT`, which frees uninitialised local stacks. `SmBSplineCurve::CreateHelixSegment` (`SmBSplineCurveNLib.cpp:539`) validates first, but its checks let some of these inputs through: height in `[-SM_EFF_ZERO, NL_MTOL)`, both radii below `NL_MTOL`, turns in `[SM_EFF_ZERO, NL_MTOL)`.
- **Trigger:** `height = 0`, `r0 = r1 = 2`, `turns = 5`.
- **How found:** crash in the C++ reference run (`free(): invalid pointer`).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Returns `NlibCode::Input` (helix: `SM_ERR`).

#### UB-26. `N_CreateRevolvedSrf` overflows `wi[9]`
- **Location:** `NL_SrfCommon.cpp:1408`, `N_CreateRevolvedSrf` (def. `:1398`).
- **What happens:** The circle weights from `N_CalcCircWeights` go into `NL_REAL wi[9]`. A quartic revolution of 241 degrees or more has 13 weights.
- **Trigger:** `ctp = NL_QUARTIC`, `al = 270` degrees.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB (stack buffer overflow).
- **Rust port:** Keeps all the weights.

#### UB-27. `N_FitCrvFirstDerivAndKnots` writes `M[2i+1][-1]`
- **Location:** `NL_CrvFit.cpp:7923`, `N_FitCrvFirstDerivAndKnots`.
- **What happens:** When the span of `u[i]` gives `l == 0`, the routine writes column `-1` of row `2i+1`. The banded matrix is one row-major block, so this overwrites the last entry of row `2i` with a wrong value.
- **Trigger:** Knots for which `u[i]` falls in a span giving `l == 0` (not produced by its driver `N_FitCalcKnotsDerivs`).
- **How found:** adversarial / edge-case fixture `first_derivs_row_underflow_p2`.
- **Confirmed:** yes. The C++ matches the port bit for bit through the bad write.
- **Severity:** crash/UB (out-of-row write; silently wrong matrix).
- **Rust port:** Writes exactly there so that results match.

#### UB-28. Curve fitting routines index outside their inputs
- **Location:** `NL_CrvFit.cpp:2458` `N_FitCalcKnotVectorEndDerivs` (and `N_FitCalcKnotVectorDeriv`); `:5461` `N_FitCrvApproxKnots`; `:2273` `N_FitCrvInterpGivenParams`; `:2716` `N_FitCrvDerivs` / `N_FitCubicSplineInterp`.
- **What happens:**
  - `p == 1` with one parameter reads `u[1]` / `u[-1]`.
  - `N_FitCrvApproxKnots` copies `n+p+2` knots before checking the count, writes `Pw[1]` for `n == 0`, and indexes `B` correctly only for non-decreasing parameters.
  - `2 <= n < p` gives `N_BasisEval` too few knots.
  - A zero end derivative is estimated from `P[0..=3]` whatever the point count, which reads out of bounds below four points.
- **Trigger:** `N_FitCrvDerivs` with 3 points and zero `Ds`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-29. Curve tools trust their inputs
- **Location:** `NL_CrvTool.cpp:65` `N_CrvInsertKnot`, `:1369` `N_CrvRefine`, plus `N_CrvSplit`, `N_CrvDecomposeBez`, `N_CrvExtractCrvSeg`, `N_CrvRemoveKnotMaxErr`; `NL_SrfSymbol.cpp:4165` `N_CrossBoundDerivCrvNonRatSrf`.
- **What happens:** No validation. Unclamped knots, multiplicity above `p`, out-of-range `ul`/`ur`, or a too-small surface index outside the arrays.
- **Trigger:** `N_CrvInsertKnot` on a curve with an inner knot of multiplicity `p+1`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-30. Reparametrisation writes at negative indices
- **Location:** `NL_CrvBasic.cpp:5005` `N_CrvReparamToInterval`; `NL_SrfBasic.cpp:5095` `N_SrfReparamToInterval`.
- **What happens:** They write `U[m-p..]` for `m < p` (and `U[r-p..]` for `r < p`).
- **Trigger:** A curve record with `m < p`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`, leaving the curve unchanged.

#### UB-31. NLib curve helpers read values their callees did not write
- **Location:**
  - `NL_CrvBasic.cpp:3256` `N_CrvGetNumAndDenom`: allocates the numerator, then fails before writing it.
  - `NL_CrvBasic.cpp:5364` `N_CrvIsPlanar`: reads `ang` after a failed `N_VectorsAngle`.
  - `NL_CrvAdv.cpp:12688` `N_LinearMultiplyCrv`: ignores the `N_CrvGetGrevilleAbscissa` error, then uses the parameter.
  - `NL_CrvCommon.cpp:75` `N_CreateQuadraticArcTol`: reads `N_IsectLineLine`'s unset `ints` flag.
  - `NL_Geom.cpp:2868` `N_DistPtLineSeg` and `:6068` `N_CPtsAreColinear`: no distance written for NaN input, which is then compared.
- **What happens:** Results depend on uninitialised memory.
- **Trigger:** A degree-0 curve for `N_LinearMultiplyCrv`; zero end tangents for `N_CreateQuadraticArcTol`; a zero weight for `N_CPtsAreColinear`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Propagates the callee error (or returns NaN / `false` where documented).

#### UB-32. NLib surface routines read out of range or uninitialised data
- **Location:**
  - `NL_SrfAdv.cpp:696` `ST_surxsdr`: the degree-1 reflection fallback reads `Rw[ii][kby ± 2]` outside its row. Also reached through `N_SrfExtendByDist` (`:661`).
  - `:10401` `N_CreateSkinSpine`: reads `pSpineParams[-1]` for one profile with end derivatives.
  - `:17403` `N_ApproxCrossBoundaryDerivs`: ignores the `N_VectorsAngle` status, then inserts an uninitialised knot `UX[0]`.
  - `:13262` `N_CreateSkinSrfBoundaryContinuity`: uses an uninitialised derivative curve with `NL_UVDIR`.
- **What happens:** Out-of-range or uninitialised reads. Upstream then "succeeds" with whatever it read.
- **Trigger:** G1/G2 extension in a degree-1 direction with a degenerate first reflection normal; `eps = 0` with an exactly aligned end tangent.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` or the angle error.

#### UB-33. `sm_CrvDerivs` / `sm_CrvRationalBasisDerivs` overrun fixed derivative arrays
- **Location:** `SmNurbsCrv.cpp:1281`-`1288` (`sm_CrvDerivs`: `BD_STACK[GW_MAX_DERIV]`, but the guard is `der > GW_MAX_DERIV`); `SmNurbsCrv.cpp:1182` `sm_CrvRationalBasisDerivs`, reached from `SmOffsetCurve::Evaluate` (`SmOffsetCurve.cpp:141`).
- **What happens:** `der == 5` writes past the row arrays. A zero-distance offset of a rational B-spline evaluated with more than `NL_MAXDER` (4) derivatives overflows a fixed stack array.
- **Trigger:** `sm_CrvDerivs(..., der = 5, ...)`; `SmOffsetCurve` (distance 0, rational base) `Evaluate` with 5 derivatives.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB (stack buffer overflow).
- **Rust port:** Rejects `der >= 5`. The harness limits offset evaluation to 4 derivatives.

### Spatial structures and small geometry

#### UB-34. `SmObjsInVoxels::IncMaxItemCount` re-points buckets into the array it is about to free
- **Location:** `source/SMLib/inc/SmHashTable.h:1803` (bucket items re-pointed at `&m_sPointItems.GetAt(id)`, which is still the old array) then `:1810` (`SetArray` frees the old array). Function `SmObjsInVoxels<TYPE>::IncMaxItemCount` (def. `:1747`).
- **What happens:** Once a model that still has items in buckets grows, every bucket item dangles, and `FindCoincidentPairs` reads freed memory. Upstream comments at `SmHashTable.h:2113` and `SmHealData.cpp:3126` describe the hazard and reserve capacity up front. The growth path itself is unchanged.
- **Trigger:** `SmObjsInVoxels::Add` beyond the reserved size (`size + 7 > GetDataSize()`) while items are in buckets, then `FindCoincidentPairs`.
- **How found:** read during porting.
- **Confirmed:** no. The shim refuses these programs.
- **Severity:** crash/UB (use after free).
- **Rust port:** Stores indices, not pointers.

#### UB-35. `MapPointToVoxel` converts negative doubles to `ULONG`
- **Location:** `SmHashTable.h:1957`-`1959`, `SmObjsInVoxels<TYPE>::MapPointToVoxel`.
- **What happens:** `(ULONG)((x - min) / len)` is UB for negative values. On x86-64 g++ it wraps, so a point below the box lands in the top voxel after the clamp (the comment says the low end). NaN maps high; `+inf` maps to 0.
- **Trigger:** Add a point with `x < box.min.x`.
- **How found:** read during porting; generated difftest cases.
- **Confirmed:** yes. The port emulates the x86-64 conversion and matches.
- **Severity:** crash/UB (wrong voxel in practice).
- **Rust port:** Emulates the x86-64 result (`ulong_from_f64`).

#### UB-36. `FindCoincidentPairs` reads `GetAt(0)` of an empty bucket list
- **Location:** `SmHashTable.h:2360`, `SmObjsInVoxels<TYPE>::FindCoincidentPairs`.
- **What happens:** An empty `pOptBucketIndices` is read at index 0.
- **Trigger:** `FindCoincidentPairs` with an empty, non-NULL bucket index array.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Checks no bucket.

#### UB-37. `SmPolarBox::Union` loops over an unset count
- **Location:** `SmPolarBox.cpp:1031` `SmPolarBox::Union`; `:930` `GetBoundaryVectors`.
- **What happens:** `GetBoundaryVectors` returns `SM_ERR_INVALID_INPUT` without setting its count when one box is `SM_PS_UNINITIALIZED` (possible after `ReSet`, which keeps a stale basis). `Union` then loops over the unset count.
- **Trigger:** `ReSet` box A, then `A.Union(B)` with aligned stale bases.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-38. Point trees divide by zero in `AddToSpatialTree`
- **Location:** `SmTree.cpp:255` `SmTree::AddToSpatialTree` (point-tree constructors at `:110`/`:164`).
- **What happens:** A point tree never initialises its object-list memory manager, so adding an object divides by zero.
- **Trigger:** `SmTree(point, size, ...)` then `AddToSpatialTree`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Refuses the operation.

#### UB-39. `SmGrid` indexing and conversion hazards
- **Location:** `SmGrid.cpp:881` `Step3DDDA` (stepping after an exit indexes voxel `m_lOut*`); `:1143` `GetVoxelAddress` (NaN to `ULONG` for a zero-size axis); `:369` `~SmGrid` (reads `m_aVoxels[0][0]` with a zero voxel count); `:525` `AddElement(..., SM_GE_HIERARCHY, ...)` (dereferences a NULL element).
- **What happens:** Out-of-bounds reads, UB conversions, NULL dereference.
- **Trigger:** Call `Step3DDDA` again after `rbExitedGrid = TRUE`; build a grid with a zero extent on one axis.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`, or a saturating cast / constructor check.

#### UB-40. `smgu_PolygonArea` with no points reads `sPoints[-1]`
- **Location:** `SmGeomUtility.cpp:274` (`sLastPoint = sPoints[lNumPoints-1]`), `smgu_PolygonArea` (def. `:265`). Reached from `SmPointSet3d::GetArea` (`source/SMLib/inc/SmPointSet.h:358`).
- **What happens:** `lNumPoints - 1` wraps to `ULONG_MAX`.
- **Trigger:** `smgu_PolygonArea` with 0 points; `GetArea` on an empty point sequence.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` (point set: area `-1`).

#### UB-41. `SmPointSet3d::GetTightPBox` on an empty planar set reads `m_pData[0]`
- **Location:** `source/SMLib/inc/SmPointSet.h:181` `GetTightPBox`, via `SmPointSet3d::SetUpTightPseudoBox` (`SmPointSet.cpp:631`).
- **What happens:** An empty set built with a normal is classified planar, and the planar extra work reads the first point.
- **Trigger:** An empty `SmPointSet3d` with a normal, then `GetTightPBox()`.
- **How found:** crash in the C++ reference run (segfault).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Skips the extra work and uses `(0,0,0)` for missing points.

#### UB-42. `SmTransform::EvaluateBoundingBoxSimple` dereferences a NULL pseudo box
- **Location:** `SmTransform.cpp:1224`, `SmTransform::EvaluateBoundingBoxSimple`.
- **What happens:** When an output box is requested, `AreAligned(*pOptParamPseudoBox, ...)` runs before the `NULL` check.
- **Trigger:** The default call (`pOptParamPseudoBox = NULL`) with an output box requested.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-43. Small object-model NULL dereferences
- **Location:** `source/SMLib/inc/SmVector2d.h:541` `SmVector2d::operator[]` (non-const, index > 1); `source/SMLib/inc/SmObject.h:331`-`332` `SmTemporaryChangeValue::UseOrigValue` / `UseTempValue` on a default-constructed guard.
- **What happens:** Both dereference NULL.
- **Trigger:** `v[2] = 1.0` on an `SmVector2d`; `SmTemporaryChangeValue<double> g; g.UseOrigValue();`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `SolidError::Upstream { code: 1001 }`, or does nothing.

#### UB-44. Attribute lifetime bugs
- **Location:**
  - `SmAttribute.cpp:1619` `SmAttribute::~SmAttribute`: deletes a dependent attribute twice, through `RemoveAttribute` -> `RemoveUser`. A user that does not list the attribute leaves dangling lists.
  - `:816` `SmMergeRegionAttribute::Merge`: dereferences NULL when both inputs have a merge-region attribute and the result does not, and casts any `SM_AI_OTHER_REGION_ID` attribute to the merge-region class.
  - The colour average casts whatever `SM_AI_COLOR` attribute it finds.
  - `:1038` `SmMergeRegionAttribute::SetRegions`: writes past the "other" lists when they are shorter.
  - `SmAObject.cpp:88` `SmAObject::Notify(SM_NO_REG_PROPAGATION)`: with two or more other regions it indexes the refilled list with the previous count (stale or out-of-range), and `SET_FROM_OTHER` can read an attribute it just deleted.
- **What happens:** Double delete, use after free, NULL dereference, bad casts, out-of-bounds writes.
- **Trigger:** Delete a dependent attribute that still has users; merge two merge-region-attributed objects into one without.
- **How found:** read during porting.
- **Confirmed:** no. The difftest skips these scripts.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-45. A tracker started twice leaves a dangling callback
- **Location:** `SmTrackTopologyChanges.cpp:434` `StartTracking`, `:512` `StopTracking`.
- **What happens:** The second `StartTracking` stores the tracker's own callback as "previous". `StopTracking` (and the destructor) then does nothing, and the context keeps a dangling `SmNotifyCallback*`. The next `Notify` calls into a destroyed object.
- **Trigger:** `t.StartTracking(); t.StartTracking();` destroy `t`, then any notify.
- **How found:** read during porting.
- **Confirmed:** no. The shim skips destroying a tracker in that state.
- **Severity:** crash/UB (use after free).
- **Rust port:** Identifies callbacks by key, so the case cannot arise.

### Curves

#### UB-46. NURB-less curves are dereferenced
- **Location:**
  - `SmBSplineCurve.cpp:43`: `SmBSplineCurve(lDim)` has a NULL NURB, which nearly every query dereferences (`GetOrCreateGwNurbPointer` asserts in debug only).
  - `SmHyperbola.cpp:422` `MakeNurb` (`:441`, `:470`) and `SmParabola.cpp:419` (`:435`): when `smgu_LineLineClosestPoint` fails, `MakeNurb` fails. The constructor ignores this, and `GlobalPointSolveSTEP` then dereferences the missing NURB in the curve cache.
- **What happens:** NULL dereference.
- **Trigger:** A hyperbola or parabola with a degenerate interval (parallel end tangents) or zero focal distance, then `GlobalPointSolveSTEP`.
- **How found:** read during porting.
- **Confirmed:** no. The shim refuses these inputs.
- **Severity:** crash/UB.
- **Rust port:** Returns `SM_ERR`.

#### UB-47. `SmBSplineCurve` helpers write or read past their arrays
- **Location:**
  - `SmBSplineCurve.cpp:5109` `GetControlPointsExpert`: writes two values per point for a dimension-1 curve.
  - `SmBSplineCurveNLib.cpp:2723` `InterpolatePoints`: copies `cpOptParams->GetSize()` values into an array of `m + 1`.
  - `SmBSplineCurveNLib.cpp:3038` `DeformToEndPointTargets`: ignores `DegreeElevate`'s status. On a zero-length knot interval it then runs degree-3 indexing on the degree-1 curve.
  - `SmBSplineCurve.cpp:5922` `RemoveExtraKnots`: on a borrowed NURB, swaps the copy in but keeps `m_bNurbIsBorrowed = TRUE`, and its temporary frees the borrowed curve.
- **What happens:** Out-of-bounds reads/writes and freeing memory the object does not own.
- **Trigger:** `DeformToEndPointTargets` on a line whose NURB has knots `[a,a,a,a]`; `InterpolatePoints` with 3 points and 4 parameters.
- **How found:** read during porting. The `DeformToEndPointTargets` case ran in the C++ build without a visible failure.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-48. Copying a curve that owns a Hermite base
- **Location:** `SmProjectedCurve.cpp:76`, `SmOffsetCurve.cpp:53`, `SmCompositeCurve.cpp:1111`, `SmCrvInVolume.cpp:102` (copy constructors).
- **What happens:** `SmHermiteCurve` has no `Copy` (base `SmCurve::Copy` returns `SM_ERR`), and the copy constructors ignore the status. The results:
  - a projected curve with a NULL base;
  - an offset curve sharing the base pointer, then a double free;
  - a composite curve with a NULL segment;
  - an in-volume curve with both objects owning one curve, then a double delete.
- **Trigger:** Copy an `SmOffsetCurve` that owns an `SmHermiteCurve` base.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB (double free, NULL dereference).
- **Rust port:** Returns the `SM_ERR`, or owns the base by value.

#### UB-49. `SmCrvInVolume` reads empty arrays
- **Location:** `SmCrvInVolume.cpp:911` `GenerateMults`, `:992` `GetNumberNaturalKnots`, `:623` `CalculateContinuities`.
- **What happens:** `GenerateMults`' state check returns before sizing its output, so `GetNumberNaturalKnots` reads past an empty array. With no breaks, `CalculateContinuities` reads `SetAt(0)` of an empty array and loops on `size - 1` underflow.
- **Trigger:** `GetKnots` with an interval on a cached curve; `CalculateContinuities` with no breaks.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-50. `SmCurve::FixChainCurvesList` uses deleted curves
- **Location:** `SmCurve.cpp:946` (also `:968`, `:989`, `:1010`), `SmCurve::FixChainCurvesList` (def. `:854`).
- **What happens:** With `bDeleteDuplicates`, `delete pBSp1; pBSp1 = NULL;` leaves the dangling pointer in `r3DCurves[j]`. The next scan dereferences it, and a reversed duplicate can be deleted twice. A later branch can pick the NULL local as the next link and dereference it.
- **Trigger:** Two identical B-spline segments, one reversed, with `bDeleteDuplicates = TRUE`.
- **How found:** read during porting.
- **Confirmed:** no. The generator drops these cases.
- **Severity:** crash/UB (use after free, double free).
- **Rust port:** Removes the duplicate once. A link choice equal to the deleted curve is `InvalidInput`.

#### UB-51. `sm_UnitizeTangent` reads `rPVV[2]` of a 2-element array
- **Location:** `SmCurve.cpp:4549` `sm_UnitizeTangent`, called from `:4620` `sm_FilterSolution` with `SmVector3d sPV1[2]` / `sPV2[2]`.
- **What happens:** When the first derivative is not longer than `SM_EFF_ZERO`, the second derivative is read from `rPVV[2]`, past the stack array.
- **Trigger:** A curve whose first and second derivatives both vanish at the solution.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Reads `Vec3::UNDEF` there.

#### UB-52. Curve solvers dereference optional pointers without checks
- **Location:**
  - `SmCurve.cpp:14907` `SmCurvePropertyEFO::Evaluate`: `SM_CP_PROJECTED_POINT_*` reads `m_cpOptVectors`; `SM_CP_DISTANCE_TO_POINT` / `SM_CP_CYLINDER_INTERSECTION` read `m_cpdPropertyValue`. Also, `SM_CP_MAXIMIZE_RADIUS_OF_CURVATURE` with zero curvature falls through into a 0/0.
  - `:17170` `SmFindCCNormalENFO::Evaluate`: reads `m_cpOptVectors[0..1]` for `SM_SO_SIGNED_PIVOT_MINIMIZE`.
  - `:4881` `SmCCIGlobalSolver::LocalSolve`: casts each leaf's `m_pData` to `SmBezierSpan` (the aux-type check is an `SM_ASSERT`), which reads past an `SmBezierAux1d` on pruned trees.
  - `SmGlobalSolver.cpp:130` `SmGlobalSolver::SolveIt`: initialises `sStartBranch[i]` only for `i < m_lNumTrees`, so a one-tree solve passes an uninitialised `apBranch[1]`.
- **What happens:** NULL dereference, out-of-object reads, uninitialised pointer use.
- **Trigger:** A property solve of `SM_CP_PROJECTED_POINT_X` with no vectors.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-53. `SmCurve::FindParameterAtArcLength` reads `sKnots[(ULONG)-1]`
- **Location:** `SmCurve.cpp:3928`, `SmCurve::FindParameterAtArcLength`.
- **What happens:** Stepping backwards from a start within `dParamTol` below the first knot leaves `ik = -1`, which is then used as an index.
- **Trigger:** `dArcLength < 0` from `t = U[0] - 0.5 * dParamTol`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-54. `CrvsFixGaps` / `CrvsCleanup` cast every curve to `SmBSplineCurve`
- **Location:** `SmCurve.cpp:1184` `SmCurve::CrvsFixGaps`, `:1106` `SmCurve::CrvsCleanup`.
- **What happens:** The non-virtual `SetControlPoint` (and `TrimGap`) are called through a bad cast for other curve kinds.
- **Trigger:** `CrvsFixGaps` on a list containing an `SmLine` or `SmEllipse`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Moves end points of B-splines only and returns `InvalidInput` for other kinds.

#### UB-55. `SmCurve::CrvsCleanup` crashes next to a closed curve
- **Location:** `SmCurve.cpp:1106`, `SmCurve::CrvsCleanup` (`TrimGap` between the previous curve and a closed curve).
- **What happens:** Removing a tiny curve whose neighbour is closed (equal end points) segfaults inside `TrimGap`.
- **Trigger:** A cubic segment, then a tiny segment, then a closed quadratic loop.
- **How found:** crash in the C++ reference run (SIGSEGV; reproduced with the trigger above).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Completes the clean-up.

#### UB-56. `SmCurve::DeformToEndPointTargets` uses a failed approximation
- **Location:** `SmCurve.cpp:21563`, `SmCurve::DeformToEndPointTargets`.
- **What happens:** It ignores `ApproximateCurve`'s status, then calls the B-spline deform through `*ppOptNewCurve`, which is NULL on failure.
- **Trigger:** A curve kind whose `ApproximateCurve` fails.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns the error.

#### UB-57. Conic and ellipse uninitialised reads
- **Location:**
  - `SmConic.cpp:461` `SmConic::IntersectImplicitConic`: solves a quartic of uninitialised coefficients for `SM_CT_UNKNOWN`.
  - `SmCircle.cpp` three-point constructor: ignores `smgu_CircleFrom3Points`' status, so collinear points leave the outputs unset.
  - `SmEllipse.cpp:1674` `SmEllipse::ConvertTo2D`: reads `adSols[1]` with fewer than two roots.
  - `SmEllipse.cpp:467` `SmPolarConversion::ConvertToNURBSParameter` / `ConvertToPolarParameter` / `IsCurrent`: index empty or short tables. `SmEllipse::AssertValid` (`:4093`) uses an uninitialised `dNurbT` when no table entry is written.
- **What happens:** Uninitialised reads, out-of-bounds table reads.
- **Trigger:** `SmCircle(P0, P1, P2)` with three collinear points.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-58. `SmCompositeCurve::GetKnots` reads past its array, and loops forever with an interval
- **Location:** `SmCompositeCurve.cpp:524` (`lLastIndex = rKnots.GetSize()`), `:569` (`for(ii=rKnots.GetSize()-1; ii>=0; ii--)` on an unsigned `ULONG`), `SmCompositeCurve::GetKnots` (def. `:505`).
- **What happens:** For any composite of two or more segments, the first transition reads `rKnots.GetAt(GetSize())` and writes `SetAt(GetSize(), ...)`. With `pOptIvl`, the trim loop never ends and reads `rKnots[ULONG_MAX]`.
- **Trigger:** `GetKnots` on a two-segment composite; again with an interval.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB (also a hang).
- **Rust port:** Returns `InvalidInput`.

#### UB-59. `SmCompositeCurve` NULL dereferences
- **Location:** `SmCompositeCurve.cpp:595` `ApproximateCurve` (`rpNewBSplineCurve->AssertValid()` after `CreateByJoining` fails without making a curve); `:392` `CalculateBoundingBox` (`*pBBox` is written unconditionally).
- **What happens:** NULL dereference.
- **Trigger:** `ApproximateCurve` on unconnected segments; `CalculateBoundingBox` with `pBBox = NULL`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns the join error / `InvalidInput`.

#### UB-60. `sm_CreateCorner` reads `sSolutions[0]` of an empty array
- **Location:** `SmCompositeCurve.cpp:2178`, `sm_CreateCorner` (def. `:1785`). Reached from `CreateOffsetsOfManyCurves` and `CreateTrimmedOffsets` with `SM_OC_LINEAR_CHAMFER`.
- **What happens:** When the chamfer line misses an extension line, the empty intersection array is read at index 0.
- **Trigger:** Linear-chamfer offsets of random planar polygons hit it often.
- **How found:** crash in the C++ reference run (segfault in the offsetters).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-61. `sm_CheckBowTies` indexes by the original curve count
- **Location:** `SmCompositeCurve.cpp:2352`, `sm_CheckBowTies`.
- **What happens:** Reads `crOffsets[0]`, `crOffsets[lNumOrig-1]` and `crOrigCurves[0]` without checks. `IntersectAndTrimOffsets` ignores its status.
- **Trigger:** One-sided offsetting that drops degenerate offsets, leaving fewer raw offsets than original curves.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` and goes on without bow-tie points.

#### UB-62. `SmCrvOnSurf::GetKnots` reads `rKnots[0]` of an empty array
- **Location:** `SmCrvOnSurf.cpp:1220`, `SmCrvOnSurf::GetKnots` (def. `:1145`).
- **What happens:** On an uncached curve with `pOptIvl`, an interval holding no UV knot leaves `rKnots` empty, and `rKnots[0]` is read anyway. It also reads `GetAt(lCnt - 1)` when the classification has no interval.
- **Trigger:** `GetKnots` with an interval strictly between two UV knots.
- **How found:** crash in the C++ reference run.
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

### Surfaces and volumes

#### UB-63. `SmCurveBoundedSurface::operator==` reads past the UV boundary array
- **Location:** `SmCurveBoundedSurface.cpp:122`, `SmCurveBoundedSurface::operator==` (def. `:83`).
- **What happens:** The UV loop runs to the number of 3D boundaries, so `m_vUVBoundaries[i]` is read out of bounds when there are more 3D boundaries than UV boundaries.
- **Trigger:** Two equal surfaces with 2 3D boundaries and 1 UV boundary.
- **How found:** crash in the C++ reference run (difftest case `drop_curve_eq_other_same`).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Stops at the end of the UV array.

#### UB-64. `sm_CreateNonRationalCurve` crashes for a rational curve on a base surface
- **Location:** `SmBSplineSurfaceNLib.cpp:2906`, `sm_CreateNonRationalCurve` (file static).
- **What happens:** Segfaults, presumably in `SmSurface::DropCurve` on a surface with no face or B-rep. The root cause was not verified.
- **Trigger:** `DropCurveCurve.smc` with unit weights (rational) on `DropCurveSurface.sms`.
- **How found:** crash in the C++ reference run.
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** The drop path is not ported yet (`Unported`).

#### UB-65. `CreateCornerBlend` and patch builders dereference NULL or leave dangling inputs
- **Location:** `SmBSplineSurfaceNLib.cpp:3008` `CreateCornerBlend`; `:4296` `CreateCoonsPatch` (also `CreateNSidedPatch`, `JoinSurface`).
- **What happens:**
  - `CreateCornerBlend` dereferences NULL for a surface without a UV curve, for a rational 3D curve with a surface, and for n-sided bilinear blends with missing UV curves or surfaces. Its `NER` check fires only where the null count first exceeds the limit.
  - The patch builders hand their inputs' NURBs to NLib, which reallocates them on the method's local stack. The caller's objects dangle after the call.
- **Trigger:** `CreateCornerBlend` with a surface but a NULL UV curve.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`, and stores the changed NURBs back.

#### UB-66. `smsurf_ComputeSurfacePointType` writes the v result to the u output
- **Location:** `SmSurface.cpp:30027`, `smsurf_ComputeSurfacePointType`.
- **What happens:** `if (pOptSurfSideDVV) *pOptSurfSideDUU = sSurfSideDVV`. The v output is never written, and asking for the v side without the u side dereferences NULL.
- **Trigger:** `pOptSurfSideDUU = NULL`, `pOptSurfSideDVV != NULL`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB (also a wrong result).
- **Rust port:** Ported as written. The v-without-u request is rejected.

#### UB-67. Surface helpers read empty arrays or unset counts
- **Location:** `SmSurface.cpp:24009` `sm_SetUpBreaks` (reads `rBreaks[0]` of an empty array); `:17963` `SmLSIGlobalSolver::BranchMayContainAnswers` (uses an unset hit count when `SmExtent3d::IntersectLine` fails on a zero line vector); `SmSurfaceCache.cpp:1184` `SmBezierAux2d::BuildPolarBox` (dereferences a NULL `mBA_pSurface` on `BuildTreeTop` heads).
- **What happens:** Out-of-bounds or uninitialised reads, NULL dereference.
- **Trigger:** A line solve with a zero direction vector.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` or the line error.

#### UB-68. `SmPlane` hazards
- **Location:**
  - `SmPlane.cpp:1536` `SmPlane::Evaluate`: `lHighestVDeriv - k` wraps as a `ULONG` with `bOnlyUpperHalf` and more u than v derivatives, and writes outside the array.
  - `SmPlane.cpp:83` `SmPlane(Origin, Normal)`: with `|n|^2 < SM_EFF_ZERO_SQ`, `MakeUnitOrthoVectors` fails, the status is ignored and the axes stay uninitialised.
  - `SmBSplineSurfaceNLib.cpp:7223` `SmBSplineSurface::Reverse` and `SmPlane.cpp:4285` `SmPlane::RebuildSTEPFromNURBParameters`: on a plane whose NURB cannot be made (degenerate or inverted domain), they pass a NULL NURB to `N_SrfReverse` / `EvaluatePoint`.
- **What happens:** Out-of-bounds write, uninitialised axes, NULL dereference.
- **Trigger:** `SmPlane(O, (0,0,1e-13))`; `Reverse` on a plane with an inverted domain.
- **How found:** read during porting (review of `surface.plane_p1`).
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` / `SM_ERR`.

#### UB-69. `SmSurfOfRevolution` constructor reads an uninitialised `bOnPlane`
- **Location:** `SmSurfOfRevolution.cpp:147`-`149` (constructor, def. `:94`); `SmBSplineCurve.cpp:5433` `SmBSplineCurve::IsOnPlane`; `SmGeomUtility.cpp:3829` `smgu_PlanePointDistance`.
- **What happens:** `IsOnPlane` fails before writing `bOnPlane` when `smgu_PlanePointDistance` rejects the normal (`|y|^2 - 1 > SM_EFF_ZERO`). The constructor only `SE`s the status, then reads `bOnPlane`.
- **Trigger:** A planar non-line generator with a placement Y axis of length 1.1.
- **How found:** read during porting (PR #15 review).
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Accepts only unit placement axes for the constructor.

#### UB-70. `SmSurfOfRevolution` constructor re-enters itself through the generator
- **Location:** `SmSurfOfRevolution.cpp:94` (constructor); `:202` `RebuildPolarConverter`.
- **What happens:** The constructor sets the generator's owner, then evaluates it. A fresh `SmLine` generator with no NURB runs `MakeNurb`, which asks the owner for `GetGenDirParamExtent`. That makes the surface's NURB through the virtual `MakeNurb` from inside the constructor. Separately, `RebuildPolarConverter` dereferences a NULL iso curve when `CreateIsoParametricCurve` fails (only asserted).
- **Trigger:** `SmSurfOfRevolution` built on an `SmLine` from its two-point constructor.
- **How found:** read during porting.
- **Confirmed:** no. The shim makes the line's NURB first.
- **Severity:** crash/UB.
- **Rust port:** Evaluates the generator without the owner detour, and returns the error.

#### UB-71. Volume NULL dereferences
- **Location:** `SmVolume.cpp:634` `SmVolume::RefreshInvOrientMap` (calls `MakeMapSimple` through NULL without an orient map); `SmTwistVolume.cpp:1106` `SmTwistVolume::EvaluateBoundingBoxSimple` (writes `*pProjBox` unconditionally in the unbounded branch; `(long)(theta / 2 PI)` of NaN); `SmBSplineVolume.cpp:2645` `SmBSplineVolume::InvEvaluateGuessPointSimple` (indexes knots by control-point index; only `SM_ASSERT`).
- **What happens:** NULL dereference, UB conversion, out-of-bounds knot reads.
- **Trigger:** `RefreshInvOrientMap` on a volume with no orient map.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.

#### UB-72. Curve-in-domain check of a default `SmUnbendVolume` crashes
- **Location:** `SmVolume.cpp:1538` `SmVolume::IsCurveInInSpaceDomain` -> `SmUnbendVolume.cpp:2467` `SmUnbendVolume::IsCurveInParamDomainSimple`.
- **What happens:** Segfaults in the unbend volume's curve check. That check builds its discontinuity half plane with the interval `SmExtent1d(m_dSingularityTol, -SM_BIG_DOUBLE)` (`:2479`), i.e. min above max. Whether this inverted plane domain is the cause was not verified.
- **Trigger:** A default-constructed `SmUnbendVolume` used as an orient map, then `IsCurveInInSpaceDomain` with a line.
- **How found:** crash in the C++ reference run.
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** The unbend volume is not ported yet. The harness rejects these chains.

#### UB-73. `SmBSplineVolume::CalculateContinuitiesSimple` samples `sKnots2[ii]` past its end
- **Location:** `source/SMLib/src/SmBSplineVolumeNLib.cpp:602` and `:635` (`SmBSplineVolume::CalculateContinuitiesSimple`, def. `:456`); also `:667` (`rConts[kk]`).
- **What happens:** The inner sampling loop over `jj` reads `sKnots2[ii]` (the outer index) instead of `sKnots2[jj]`, so it samples one iso-curve per `ii` repeatedly and reads past the end of `sKnots2` (into its 256-entry stack buffer: stale or uninitialised values) whenever `sKnots1` has more samples than `sKnots2`. `rConts[kk]` likewise assumes every iso-curve has as many knots as the first one classified.
- **Trigger:** `CalculateContinuitiesSimple(.., bCalcGeometric = TRUE)` on a volume whose two other directions have different sample counts (e.g. 5 and 3), in a build where `EvaluateIsoParametricCurveSimple` succeeds (`USE_VOLUMES`).
- **How found:** read during porting.
- **Confirmed:** no (the reference build has no `USE_VOLUMES`, so the loop stops at the first sample; see WR-48).
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` where upstream would read past the arrays.

#### UB-78. `SmApiCreateSkinFromFaces` with no faces crashes
- **Location:** `source/SM_API/src/SmApiPrimitives.cpp:1335` `SmApiCreateSkinFromFaces` -> `source/SMLib/src/SmPrimitiveCreation.cpp:6711` `SmPrimitiveCreation::CreateSkinFromFaces` -> `SmBSplineSurfaceNLib.cpp` `SmBSplineSurface::CreateSkinnedSurface`.
- **What happens:** With an empty face array (and no curves, which the API always passes), `CreateSkinFromFaces` still runs one layer and calls `CreateSkinnedSurface` with zero cross sections, which has no check for that and segfaults.
- **Trigger:** `SmApiCreateSkinFromFaces` with an empty `rFaces`.
- **How found:** crash in the C++ reference run (`sweep.api_surfaces` probe case).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** `create_skin_from_faces` is a stub until `primitive.primitive_creation_p3` lands; `BSplineSurface::create_skinned_surface` returns `InvalidInput` for no cross sections. The harness passes at least one face.
### Intersection

#### UB-74. `PointBasedSurfaceIntersect` writes past its 20-point array
- **Location:** `source/SMLib/src/SmSurface.cpp:30588` `SmSurface::PointBasedSurfaceIntersect` (UV point version).
- **What happens:** The points go into `SmTsectPnt saTsectPnts[20]`; the size limit and the equal-length check of the two point arrays are `SM_ASSERT`s only, so 20 or more points write past the stack array, and a shorter second array is read past its end.
- **Trigger:** 20 point pairs, or two UV point arrays of different lengths.
- **How found:** read during porting (`intersect.surface_intersector_p1`).
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** `Surface::point_based_surface_intersect` returns `InvalidInput` for those inputs.

#### UB-79. `SmPointClassification` calls `SmObject` virtuals on an `SmPoint3d*`
- **Location:** `SmPointClass.cpp:668` (`ComputePointParameters`: `SmTol::GetZoneTol3d(m_pObject)`), `:435` (`GetGap3d` with a surface, through `GetObjZoneTol3d`), `:493` (`IsConnectedTo`: `GetType`), `:917` (`MergeIntoObject`: `GetType`).
- **What happens:** `SM_PC_POINT` classifications store an `SmPoint3d*` cast to `SmObject*` (`SetClassObject`), and these paths call virtual `SmObject` members on it. `SmPoint3d` has no `SmObject` vtable, so the call is undefined behaviour.
- **Trigger:** `SetClassObject(SM_PC_POINT, (SmObject*)&point)` followed by `ComputePointParameters`, `GetGap3d(surface)`, `IsConnectedTo` or `MergeIntoObject`.
- **How found:** read during porting.
- **Confirmed:** no (the harness keeps these calls away from the C++).
- **Severity:** crash/UB.
- **Rust port:** Treats a point like any other non-topology object (system zone tolerance, not connected, not mergeable).

## Hang

#### HANG-1. `SmExtent1d::PeriodicWrap` steps one period per iteration
- **Location:** `SmExtent1d.cpp:907` `SmExtent1d::PeriodicWrap`. Reached from `SmExtent2d.cpp:1038` `ContainsPeriodicPoint2d` and from the `SmLocalSolveNd` periodic clamping (`SmLocalSolveNd.cpp:90`).
- **What happens:** The run time is proportional to distance / period.
- **Trigger:** Period `1e-9`, value `1e15` (~1e24 iterations); a diverging Newton step of `1e8` with period `0.01` (~1e10).
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** hang.
- **Rust port:** Same loop (faithful). The generator avoids such ratios.

#### HANG-2. `N_BasisFindGlobalMax` loops when its step is below the knot resolution
- **Location:** `NL_BasisBasic.cpp:1751`, `N_BasisFindGlobalMax`.
- **What happens:** The bracketing step `(U[i+p+1]-U[i])/(10p)` is below half an ulp of the knots, so the loop state never changes.
- **Trigger:** Knots around `1e17` with a span of `1`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** hang.
- **Rust port:** Returns `InvalidInput` when the state repeats.

#### HANG-3. Knot merges loop forever on NaN or non-positive tolerance
- **Location:** `NL_BasisBasic.cpp:2483` `N_GetCompatibleKnotVectorToTol` (`tol <= 0`, or NaN knots); `:3665` `N_MakeKnotsCompatible`; `NL_BasisAdv.cpp:253` `N_BasisInsertKnots`; `NL_BasisAdv.cpp:119` `N_GetCompatibleKnotArrayMult`; `NL_SrfAdv.cpp:13051` `N_MergeKnotVectors` (all with NaN knots).
- **What happens:** No knot compares, so no iteration advances.
- **Trigger:** A knot vector containing NaN; `tol = 0`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** hang.
- **Rust port:** Returns `InvalidInput`.

#### HANG-4. Conic angle loops
- **Location:** `NL_CrvBasic.cpp:1863` `N_CrvGetConicData`; `NL_CrvCommon.cpp:1090` `N_CreateEllipticalArcAngTypeTol`.
- **What happens:** These loop forever for non-finite angles, or where `ae + 360 == ae`.
- **Trigger:** `ae = inf`; `ae = 1e17`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** hang.
- **Rust port:** Returns `InvalidInput` for non-finite angles and after 2^20 turns.

#### HANG-5. `N_ApproxCrvWithPolyline` subdivides forever below floating-point resolution
- **Location:** `NL_CrvGeom.cpp:1531`, `N_ApproxCrvWithPolyline`.
- **What happens:** The degenerate-segment test `dd < tolsq` never succeeds, so coincident control points keep splitting. With `tol <= 0` the last vertex is also never appended.
- **Trigger:** `tol = 0`; or a relative tolerance on a zero-length curve.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** hang.
- **Rust port:** Stops after `POLYLINE_MAX_SPLITS` with `NotConverged`.

#### HANG-6. Zero-distance G2 curve and surface extensions
- **Location:** `NL_CrvAdv.cpp:640` `ST_curxcdr` (`NL_G2` parameter search); `NL_SrfAdv.cpp:661` `N_SrfExtendByDist` / `:696` `ST_surxsdr` (G1/G2 extend the iso-curves with `ST_curxcdr` `NL_G2`).
- **What happens:** The step `du = range * ldst / d1` is zero, or no longer changes the parameter, so the search never ends. A negative distance reads outside the knot range.
- **Trigger:** `N_SrfExtendByDist(..., dist = 0, NL_G1, ...)`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** hang.
- **Rust port:** Fails with `NotConverged` when a step changes nothing.

#### HANG-7. Periodic shifts in unbounded evaluation
- **Location:** `NL_CrvAdv.cpp:4257` `N_CrvEvalUnboundedPtsAndDerivs`; `NL_SrfAdv.cpp:5446` `N_SrfEvalPtDerivsUnbounded`.
- **What happens:** `while` loops shift the parameter by `ue - us` until it is in range. They never end once the shift no longer changes the parameter, and take ~1e12 steps for far parameters.
- **Trigger:** A G1-closed curve evaluated at `u = 1e20`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** hang.
- **Rust port:** Returns `NotConverged` after a bounded number of shifts.

#### HANG-8. `SmBSplineCurve::DampenKnotSpacing` cascades without end
- **Location:** `SmBSplineCurveNLib.cpp:1082`, `SmBSplineCurve::DampenKnotSpacing`.
- **What happens:** Each pass halves every span that is more than `dMaximumDeltaFactor` times longer than a neighbour. For factors below about 2 the halving cascades without end, and a factor of 1 or less never finishes.
- **Trigger:** Unique knots `0 .3 .6 1`, factor `1.2`.
- **How found:** read during porting.
- **Confirmed:** no. The harness simulates the splitting and rejects such inputs.
- **Severity:** hang.
- **Rust port:** Stops after `DAMPEN_SPLIT_LIMIT` splits with `NotConverged`.

#### HANG-9. `CreateExtendedSurface` with distance 0 never stops halving
- **Location:** `SmBSplineSurfaceNLib.cpp:4469` `SmBSplineSurface::CreateExtendedSurface`; halving tests `if (dExtDist < dDist/100.0)` at `:4574`, `:4636`, `:4703`, `:4764`. It also runs for `SmPlane`.
- **What happens:** After each failed extension the distance is halved until it drops below `dDist / 100`, which never happens for `dDist == 0` (or NaN).
- **Trigger:** `CreateExtendedSurface` with a distance of `0`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** hang.
- **Rust port:** Stops after 1100 halvings with `NotConverged`.

#### HANG-10. `SmCurve::GlobalCoincidenceChecker` recurses without a depth bound
- **Location:** `SmCurve.cpp:19950`, `SmCurve::GlobalCoincidenceChecker`.
- **What happens:** It recurses on closed and half-closed curves with no limit. The recursive calls also force `bSnapToEnds = TRUE`.
- **Trigger:** Two coincident closed curves.
- **How found:** read during porting.
- **Confirmed:** no. Unbounded recursion was not observed.
- **Severity:** hang (potential stack exhaustion).
- **Rust port:** Stops at 200 nested splits with `InvalidInput`.

## Nondeterminism

#### ND-1. `srand(time(NULL))` in face sampling and shell classification
- **Location:** `SmFace.cpp:21183` `SmFace::GetPointsInFace` (def. `:21167`); `SmShell.cpp:888`, `:905` (in `SmShell::IsInnerShellOf`, def. `:827`).
- **What happens:** The C RNG is reseeded from the clock and used to dither sample parameters / ray directions. Results differ between runs, and the global `rand()` state is clobbered for the whole process.
- **Trigger:** `GetPointsInFace` with `lPointCount > 1`; inner-shell tests on wire shells.
- **How found:** read during porting (static scan of the call-graph closure, `plan/closure.md`).
- **Confirmed:** no.
- **Severity:** nondeterminism.
- **Rust port:** Draws from the seeded `kernel.rng`.

#### ND-2. Results that depend on pointer-keyed container order
- **Location:** `source/SMLib/inc/SmMapPtrToPtr.h:42`, `:115` (`std::unordered_map` keyed by pointers); `source/SMLib/inc/SmRelation.h:52` `SmRelation` (`GetSeconds`, `GetAllFirsts`, `GetAllSeconds`); `SmBrep.cpp:28891` (`SmBrep::Transform`, range-for over an edge set); `SmFilletExecutive.cpp:531`; `SmHCR.cpp:312`; `SmTopologyIntersector.cpp:945`.
- **What happens:** Iteration order is pointer-hash order, so it can change from run to run with heap layout. So can anything order-dependent downstream (created topology order, tie-breaks).
- **Trigger:** Any of these paths on the same input under different allocators or ASLR.
- **How found:** read during porting (static scan, `plan/closure.md`).
- **Confirmed:** no. The shims sort upstream results before comparing.
- **Severity:** nondeterminism.
- **Rust port:** Uses ordered maps / insertion order on arena ids.

#### ND-3. A function-local static fixed by the first call
- **Location:** `SmFilletSolver.cpp:3604`, `SmFilletSolver::ComputePointValues` (def. `:3544`).
- **What happens:** `static double sdStaticStep = (rTsectPnt.m_ePointType == SM_IP_TANGENT_POINT) ? 1.0e-3 : 1.0e-5;` is initialised on the first call only. Every later call in the process uses the step chosen for the first point's type, so results depend on call history. (`SmFilletExecutive.cpp:1503`-`1504` also use function statics, but with constant initialisers.)
- **Trigger:** Two fillet runs in one process whose first intersection points differ in type, run in either order.
- **How found:** read during porting (static scan, `plan/closure.md`).
- **Confirmed:** no. The fillet units are not ported yet.
- **Severity:** nondeterminism (order-dependent results).
- **Rust port:** Not ported yet. The plan is to compute the step per call.

## Wrong result

### Geometry and tolerances

#### WR-1. Two-context `SmNewMarkAndLock` advances and locks the wrong context
- **Location:** `source/SMLib/inc/SmContext.h:528`-`529` (binary constructor).
- **What happens:** With `p1 != p2` it calls `m_pContext1->NewMark(m_eMarkType1)` (context 1 advanced twice) and `LockMark(m_pContext2, m_eMarkType1)` (wrong mark type). The destructor unlocks `m_eMarkType2`, so a stray lock stays on context 2.
- **Trigger:** `SmNewMarkAndLock(c1, c2)` where the free mark types of `c1` and `c2` differ.
- **How found:** adversarial / edge-case fixture `boolean_pair_two_contexts`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`MarkLock::new_pair`).

#### WR-2. Release builds do not enforce mark locks
- **Location:** `SmContext.cpp:331` `SmContext::NewMark` (and `LockMark` / `UnlockMark`).
- **What happens:** The lock checks are `SM_ASSERT*` only, so in release builds locked marks can be advanced and double locks are silent.
- **Trigger:** `NewMark` on a locked mark type.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result (release-only).
- **Rust port:** Drops the assertions (matches release).

#### WR-3. `SmExtent3d::ContainsLineSeg3d` accepts one end inside
- **Location:** `SmExtent3d.cpp:1235` (`bRtn |= ContainsPoint3d(rEndPoint, dTol)`), `SmExtent3d::ContainsLineSeg3d` (def. `:1227`).
- **What happens:** The comment at `:1233` says the box contains the segment if it contains both points, but the code ORs the two tests.
- **Trigger:** A segment from inside to far outside the box returns TRUE.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-4. `SmExtent3d::ApproximateUnbounded` computes max from the replaced min
- **Location:** `SmExtent3d.cpp:1760`, `SmExtent3d::ApproximateUnbounded` (`SM_BT_UNBOUNDED_MAX` branch).
- **What happens:** Each axis's max is computed from the already replaced min. The optional expanded extent is left untouched when `SetMinMax` rejects it.
- **Trigger:** An extent with infinite min and finite max on one axis.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is. The rejected second extent is `None`.

#### WR-5. `SmExtent3d::Circumscribe` reads the axis map backwards
- **Location:** `SmExtent3d.cpp:129`, `SmExtent3d::Circumscribe(SmPseudoBox)`.
- **What happens:** `IsAxisAligned` fills `alAxisMap[i]` with the axis that basis `i` is parallel to, but `Circumscribe` reads it as the basis parallel to axis `i`. Swaps and sign flips come out right; 3-cycles do not.
- **Trigger:** Basis `(y, z, x)` with intervals `[1,2], [3,5], [-7,-6]`: gives min `(3, -7, 1)` instead of `(-7, 1, 3)`.
- **How found:** adversarial / edge-case fixture `circumscribe_axis_cycle_quirk`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-6. `SmPeriodicExtent1d::AddInterval` on an empty extent gives the full period
- **Location:** `SmPeriodicExtent1d.cpp:262`, `SmPeriodicExtent1d::AddInterval`.
- **What happens:** The empty branch tests `(m_dPeriod - |m_dMax - m_dMin|) < ScaledZero` on the empty sentinels `[1e20, -1e20]`. This is true for any period below `2e20`, so the result is `[0, period]` full instead of `[from, to]`.
- **Trigger:** Empty extent, period 1, `AddInterval(0.2, 0.3)`.
- **How found:** adversarial / edge-case fixture `add_empty_quirk`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-7. `SmPseudoBox::DistanceToPoint` returns a status as a distance
- **Location:** `SmPseudoBox.cpp:772`, `SmPseudoBox::DistanceToPoint`.
- **What happens:** Returns `SM_ERR` (1001.0) as the distance when the point is outside more than three planes. When `smgu_IntersectTwoPlanes` / `ThreePlanes` fail, it uses the `SM_UNDEF_DOUBLE` line or vertex.
- **Trigger:** An inside-out box (`Init()` state).
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-8. `SmTransform::EvaluateBoundingBoxSimple` aligned branch is not the image of the box
- **Location:** `SmTransform.cpp:1224`, `SmTransform::EvaluateBoundingBoxSimple`.
- **What happens:** It scales parameter intervals by `1/|axis|` and shifts them by `origin·axis`, which is not the transformed box (e.g. under a pure translation).
- **Trigger:** A translation-only transform with an aligned parameter box.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-9. `SmPolarBox::AddVector3d` relabels a region as an arc without transposing
- **Location:** `SmPolarBox.cpp:696`, `SmPolarBox::AddVector3d` (`SM_PS_REGION` case).
- **What happens:** When the x range collapses, the basis is rotated and the state becomes `SM_PS_ARC`, but the domain is re-set to its own bounds. The arc's span stays in y while arc code reads x.
- **Trigger:** A region box whose x range collapses below `SM_EFF_ZERO_SQRT`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-10. `smgu_BestFitLineToPoints` takes a fixed eigenvector, not the best-fit direction
- **Location:** `SmGeomUtility.cpp:4223`, `smgu_BestFitLineToPoints`. `smgu_BestFitPlaneToPoints` uses the same vector as a plane normal.
- **What happens:** It takes `aEigenVectors[2]` of `C = del*I - M`. The comment calls this the largest eigenvalue's vector, but the least-squares direction minimises `v^T C v`. In practice the result depends on the Jacobi solver and the sort.
- **Trigger:** Axis-aligned collinear points give a direction perpendicular to them (classified `SM_PST_SCATTERED`). Points along `(1,1,1)` give the right direction.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-11. `smgu_CircleFindSilhouettePoints` perspective case omits the centre
- **Location:** `SmGeomUtility.cpp:2185`, `smgu_CircleFindSilhouettePoints`.
- **What happens:** The perspective case returns rotated unit directions times the radius, without adding the circle centre. The parallel case adds it.
- **Trigger:** A circle centred at `(10,0,0)` viewed in perspective.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-12. `SmObjsInVoxels::FindCoincidentPairs` misses pairs and changes the model
- **Location:** `source/SMLib/inc/SmHashTable.h:2360`, `SmObjsInVoxels<TYPE>::FindCoincidentPairs`.
- **What happens:** `pBegPointItem` / `rBegPoint` are bound once per bucket and never advanced, so only head-vs-other pairs are found. `rEndPoint` is a reference, so `rEndPoint = pEndPointItem->m_sTgtPoint3d` overwrites stored points during the search.
- **Trigger:** Three coincident points in one voxel. The pair not involving the head is never reported.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-13. `SmHashTable::IncMaxItemCount` zeroes bucket items after all were removed
- **Location:** `source/SMLib/inc/SmHashTable.h:804`, `SmHashTable<TYPE>::IncMaxItemCount`.
- **What happens:** Records are copied only while `GetUsedBucketItemCount() > 0`. Otherwise they come back calloc-zeroed (bucket 0, NULL item, id 0), and a later `Remove` of such an item walks bucket 0 while the counters still increment.
- **Trigger:** Add items, remove all, grow, then remove one.
- **How found:** generated difftest case.
- **Confirmed:** yes ("reproduced").
- **Severity:** wrong result.
- **Rust port:** Reproduced.

#### WR-14. `SmPointSet3d::SetUpTightPseudoBox` discards its projections
- **Location:** `SmPointSet.cpp:631`, `SmPointSet3d::SetUpTightPseudoBox`.
- **What happens:** `sBegPt0.ProjectPointToPlane(..);` is `const` and returns the projection, which is discarded. Angles are measured between unprojected points.
- **Trigger:** A slightly non-planar point set: the basis differs in the last bits from a projecting implementation.
- **How found:** generated difftest case (a projecting port differed at ~1e-15).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-15. `SmPointSet3d::SetUpCentroid` drops 50 points for sizes that are multiples of 50
- **Location:** `SmPointSet.cpp:410`, `SmPointSet3d::SetUpCentroid`.
- **What happens:** The first block holds `size % 50` points and the loop runs `size / 50 + (extras ? 1 : 0)` times. For multiples of 50, the last 50 points are never added (50 points give `(0,0,0)`). `m_bCentroidSet` is never set, and after `GetNormal` the centroid comes from the best-fit plane instead, so `GetCentroid` depends on call order.
- **Trigger:** `GetCentroid` on 50 points.
- **How found:** adversarial / edge-case fixtures `centroid_blocks_*`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

### Topology, attributes, solvers

#### WR-16. `SmTopology::Notify` merge uses the deleted object as the source
- **Location:** `SmTopology.cpp:220` (and `:239`), `SmTopology::Notify` (def. `:187`).
- **What happens:** `SM_NO_MERGE_IN_BREP` takes `pData2` (the deleted object) as the source. Upstream marks this itself: "GWC:TODO try typo bug fix". Merges also OR the target flags into both targets but copy none from the source.
- **Trigger:** Merge notify of two topology objects with different flags.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-17. `StopTracking` clears a foreign callback
- **Location:** `SmTrackTopologyChanges.cpp:512`, `SmTrackTopologyChanges::StopTracking`.
- **What happens:** If the slot was empty at start, `StopTracking` clears whatever callback is installed now, including one installed by someone else since.
- **Trigger:** Start tracker A, install callback B, stop A: B is removed.
- **How found:** adversarial / edge-case fixture `track_stop_clears_foreign`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-18. Attribute copy, add and propagation inconsistencies
- **Location:**
  - `SmAObject.cpp:540` `SmAObject::AddAttribute`: skips only if `FindAttribute(id)` returns that same attribute, so a second attribute with the same id is added, and re-adding duplicates the entry and the user.
  - `SmAttribute.cpp:1101` `SmColorAttribute::PropagateRegionsThroughBooleans`: copies itself, not the colour it found.
  - `SmLongAttribute`, `SmVector3dAttribute` and `SmColorAttribute::MakeCopy` drop the copy behaviour.
  - `SmAttribute.cpp:17` `GOTO_NEXT_ATTLINE`: treats byte `0xFF` as EOF.
- **What happens:** Duplicated entries, wrong propagated colour, lost `SM_AB_STANDALONE_COPY`, early end of input.
- **Trigger:** Add two attributes with the same id; union two bodies where only one has a colour.
- **How found:** generated difftest case (attribute scripts).
- **Confirmed:** yes (all compared against C++).
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-19. `SmLocalSolve1d` without an interval steps against Newton
- **Location:** `source/SMLib/inc/SmLocalSolve1d.h:290` (constructor without interval); `SmLocalSolve1d.cpp:751` `SmLocalSolve1d::SolveIt`.
- **What happens:** The interval stays `SmExtent1d()` (`[1e20, -1e20]`), so the step limit `GetLength() * 0.08 + SM_EFF_ZERO` is negative and every step becomes `±1.584e19` against the Newton direction. With no interval and a failed first evaluation, `bFoundAnswer` is read uninitialised.
- **Trigger:** `SmLocalSolve1d(evaluator)` then `SolveIt`.
- **How found:** adversarial / edge-case fixtures `no_interval_*`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is (`bFoundAnswer` starts `false`).

#### WR-20. `SmLocalSolveNd::SolveIt` reports a failed first evaluation as found
- **Location:** `SmLocalSolveNd.cpp:90`, `SmLocalSolveNd::SolveIt`.
- **What happens:** If the first `Evaluate` fails, it returns `SM_SUCCESS` with `rbFoundSolution = TRUE` (the guess) and `SM_TR_UNABLE_TO_CONVERGE`. `bFoundAnswer` is read even when the evaluator never writes it, and the constructor does not set `m_lCurrentIteration`.
- **Trigger:** An evaluator that fails on the first call.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is (state initialised).

#### WR-21. `SmGlobalSolver::IdenticalSolutions` prefers the smaller value when maximising
- **Location:** `SmGlobalSolver.cpp:1865`, `SmGlobalSolver::IdenticalSolutions`.
- **What happens:** An `SM_SO_MAXIMIZE` solve can replace an interior maximum by a nearby span-boundary point with a smaller value.
- **Trigger:** Maximum distance on a closed Hermite curve: `0.42572...` replaced by `0.42578125`.
- **How found:** generated difftest case.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-54. Old composite curve records get their segment gaps shifted by one
- **Location:** `source/SMLib/src/SmBrepData.cpp:5438` (`sGaps` holds the first segment's start gap, then every end gap) passed to `SmCompositeCurve.cpp:1050` (which reads `(*cpGaps)[i]` as segment `i`'s end gap).
- **What happens:** For a version 32 or older composite curve, segment `i` gets the stored end gap of segment `i - 1` as its end gap (segment 0 gets its own start gap); the last end gap is never used. The branch is marked "untested code" upstream.
- **Trigger:** A closed two-segment composite record with gaps `0.5 0.25` and `0 3`: segment 0 gets gaps `0.25 / 0.5` instead of `0.5 / 0.25`.
- **How found:** read during porting (`topology.brep_data`).
- **Confirmed:** no (gaps are not in the difftest dump).
- **Severity:** wrong result.
- **Rust port:** Faithful to the code.

### NLib

#### WR-22. `ST_surxsdr` returns a stale error number past its recursion limit
- **Location:** `NL_SrfAdv.cpp:696`, `ST_surxsdr` (`depth > 10`).
- **What happens:** It returns `1` without `N_ErrSet`, so `NL_ERROR` holds whatever was recorded last. That can be an ignored iso-curve extension error, `NL_GEO_ERR`, or `0` after a callee's `N_ErrClear`. `ST_BezCalcArcLength` (`NL_Bezier.cpp:4713`) and `ST_curxcdr` / `ST_curxcrr` also return bare flags.
- **Trigger:** `DropCurveSurface.sms` extended 900 units in v: `eno = 17` at one end, `0` at the other.
- **How found:** adversarial / edge-case fixture.
- **Confirmed:** yes.
- **Severity:** wrong result (error reporting).
- **Rust port:** Returns `NotConverged`.

#### WR-23. `sm_CrvTrim` returns an error flag that callers read as success
- **Location:** `SmNurbsCrv.cpp:729`, `sm_CrvTrim`.
- **What happens:** It returns the flag value `NL_INP_ERR` (14), not `NL_YES`, for `ur <= ul`, without `N_ErrSet`. Callers comparing with `NL_YES` treat it as success.
- **Trigger:** `sm_CrvTrim` with `ur == ul`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Returns `InvalidInput`.

#### WR-24. `N_KnotVectorIsValid` accepts degenerate vectors
- **Location:** `NL_BasisBasic.cpp:172`, `N_KnotVectorIsValid`.
- **What happens:** Accepts `p+1` equal knots (e.g. `[2,2,2,2]` with `p = 3`), and a single knot with `p = 0`. Later routines index such vectors as if they had a span.
- **Trigger:** `[2,2,2,2]`, `p = 3`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result (validation gap).
- **Rust port:** Accepts them too (faithful).

### Curves

#### WR-25. `SmBSplineCurve` and `SmCurve` declare different default `bNonZeroTangents`
- **Location:** `source/SMLib/inc/SmBSplineCurve.h:481` (`bNonZeroTangents=FALSE`) vs `source/SMLib/inc/SmCurve.h:396` (`TRUE`); used in `SmBSplineCurve.cpp:2997` `SmBSplineCurve::Evaluate`.
- **What happens:** Default arguments bind by static type. The recursion inside `SmBSplineCurve::Evaluate` for a zero first derivative with one derivative asked runs with `FALSE` and returns the zero derivative, so the non-zero-tangent fix only works when 2+ derivatives are asked.
- **Trigger:** A B-spline with coincident first two control points: `Evaluate(t0, 1, ...)` through an `SmCurve&`.
- **How found:** adversarial / edge-case fixture `zero_tangent_start_nd1`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Passes `false` there (matches).

#### WR-26. `SmHermiteCurve::Evaluate` leaves the last slot unset
- **Location:** `SmHermiteCurve.cpp:299` (`for (ULONG i=4; i<lNumDerivatives; i++)`), `SmHermiteCurve::Evaluate` (def. `:283`).
- **What happens:** The zeroing loop stops one short, so `aPointAndDerivatives[n]` keeps the caller's value (`SM_UNDEF_DOUBLE` for a fresh array).
- **Trigger:** `Evaluate` with `lNumDerivatives = 5`.
- **How found:** read during porting.
- **Confirmed:** no. The shim pre-fills zeros.
- **Severity:** wrong result.
- **Rust port:** Returns zero there.

#### WR-27. `SmConic::Evaluate` zero-tangent fix reads a slot before writing it
- **Location:** `SmConic.cpp:125`, `SmConic::Evaluate`.
- **What happens:** With `bNonZeroTangents` and two derivatives, the fix reads `aPointAndDerivatives[2]` before the second derivative is written there, i.e. the caller's leftover value.
- **Trigger:** A conic with a tiny first derivative, evaluated into a reused array.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Reads `Vec3::UNDEF` (matches fresh arrays).

#### WR-28. `SmProjectedCurve::MakeOrthoNormal` is inverted
- **Location:** `SmProjectedCurve.cpp:219`-`221` (`if(bProjInit && bProjZero) { m_sProjVec.Unitize() ; } else { return( SM_ERR ) ; }`), and the same for `m_sAuxData`; `SmProjectedCurve::MakeOrthoNormal` (def. `:215`).
- **What happens:** It unitizes only a zero vector (giving `(0,0,1)`) and returns `SM_ERR` for every non-zero vector, leaving it un-normalised. A non-unit view normal gives a skewed projection.
- **Trigger:** A projected curve with view vector `(0, 0, 2)`.
- **How found:** generated difftest case (both sides agree on the skew).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-29. `SmProjectedCurve::Transform` only works for `m_bOwnsCurve == UNSURE`
- **Location:** `SmProjectedCurve.cpp:434` (`if (m_bOwnsCurve < 2)`), `SmProjectedCurve::Transform` (def. `:417`).
- **What happens:** Curves that own (`TRUE`, e.g. everything from `ReadFromDB`) or borrow (`FALSE`) their base fail with `SM_ERR`, after a pre-edit notify.
- **Trigger:** `Transform` on a projected curve read from a file.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-30. `SmIsoCurve::CreateUVIsoLine` tests `SM_SP_U` in both branches
- **Location:** `SmIsoCurve.cpp:34`, `:38`, `SmIsoCurve::CreateUVIsoLine` (def. `:24`).
- **What happens:** A constant-v iso curve gets `SM_ERR_INVALID_INPUT`. The constant-u branch builds the UV line over the domain's u interval although the line runs along v.
- **Trigger:** `CreateUVIsoLine` for `SM_SP_V`.
- **How found:** generated difftest case (`*_uvline`).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-31. `SmEllipse::ConvertTo2D` uses `dThetaXRad` for both axes
- **Location:** `SmEllipse.cpp:1740`, `SmEllipse::ConvertTo2D` (def. `:1674`).
- **What happens:** `sNewY` copies the `sNewX` formula with `dThetaXRad`. The projected ellipse gets equal axes, and `SetCanonical` of two parallel vectors runs with its status ignored. `dThetaYRad` is computed and unused.
- **Trigger:** An ellipse whose projected axes are not perpendicular.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-32. `SmCircle::CreateSimpleOffset` never shrinks for the anti-parallel normal
- **Location:** `SmCircle.cpp:339`-`340`, `SmCircle::CreateSimpleOffset` (def. `:321`).
- **What happens:** `IsParallelTo(+Z)` also accepts anti-parallel vectors, so the `-Z` branch (radius minus distance) is dead.
- **Trigger:** Offset with normal `-Z`: the radius grows.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-33. `SmCrvInVolume::GenerateMults` indexes continuities by knot index
- **Location:** `SmCrvInVolume.cpp:968`, `SmCrvInVolume::GenerateMults` (def. `:911`).
- **What happens:** `m_sConts[ii]` uses the knot index `ii`, not the matched break index, so `GetKnots`' cached path reports the wrong breaks' multiplicities. The cached and computed paths also disagree (4 vs 3 at the ends).
- **Trigger:** `GetKnots` with an interval on a cached curve.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-34. `SmCompositeCurve::CalculateBoundingBox` shifts intervals from 0
- **Location:** `SmCompositeCurve.cpp:392`, `SmCompositeCurve::CalculateBoundingBox`.
- **What happens:** Segment intervals are shifted from `0`, not from the first segment's start (as `GetNaturalInterval` does), and the shifted interval is passed to each segment.
- **Trigger:** A composite whose first segment starts at `t = 5`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-35. `SmCurvePropertyGS::BranchMayContainAnswers` corner and angle bugs
- **Location:** `SmCurve.cpp:12219` (`double dZV = 1.0 * j;`), `SmCurvePropertyGS::BranchMayContainAnswers` (def. `:12141`).
- **What happens:** Branches are pruned or kept incorrectly:
  - The "corner" points use `j` for z, so only 4 distinct box points are tried, each twice.
  - The `SM_CP_PERPENDICULAR_TO_VECTOR` / `SM_CP_MAXIMA_TO_PLANE` test adds degrees to radians before comparing with `PI/2`.
  - `SmCurvePropertyGS` also declares its own `m_cpOptVectors`, which shadows `SmGlobalSolver`'s (`source/SMLib/inc/SmGlobalSolver.h:98`), so the property vectors never reach the base checks.
- **Trigger:** A perpendicular-to-vector property solve.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-36. `SmCurve::CurveMaxDistanceBetween` with one sample divides 0/0
- **Location:** `SmCurve.cpp:2507` `SmCurve::CurveMaxDistanceBetween`; `SmCrvOnSurf.cpp:1663` `SmCrvOnSurf::SurfaceCurveMaxDistanceBetween`.
- **What happens:** `ii / (lNumSamples - 1)` is `0.0 / 0.0` for one sample, so both curves are evaluated at NaN. `SurfaceCurveMaxDistanceBetween` also sets `bDoPrecise = FALSE` unconditionally, so its precise refinement is dead code.
- **Trigger:** `lNumSamples = 1`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Computes the same NaN. The harness rejects one sample.

#### WR-37. `SmBSplineCurve::CreateSimpleOffset` passes the match flag in the wrong slot
- **Location:** `SmBSplineCurve.cpp:2475`, `SmBSplineCurve::CreateSimpleOffset`.
- **What happens:** It calls `ApproximateCurve(..., bOptMatchParameterization, FALSE)`. The caller's match flag lands in `bOptCreateAnalytics`, so parameterisation matching is always off.
- **Trigger:** `CreateSimpleOffset(..., bOptMatchParameterization = TRUE)`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

### Surfaces and volumes

#### WR-38. `SmBSplineSurface` data constructor zeroes the v knots when `s > r`
- **Location:** `SmBSplineSurface.cpp:87` (`smos_MemCpy(pNewSur->knv->U, V, (s+1) doubles, (r+1) doubles)`), constructor at `:63`.
- **What happens:** The v knots are copied with the u knot count as the destination size, so the copy is refused (status only `SE`'d) when `s > r`. The v knots stay 0 from calloc, and `sm_FixupKnotVector` then runs on zeros.
- **Trigger:** `SmBSplineSurface(n, m, p, q, r, s, Pw, U, V)` with more v knots than u knots.
- **How found:** generated difftest case (every such `data` case).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Reproduces it.

#### WR-39. `SmBSplineSurface::IsParallelToVector` returns a status as a boolean and uses the wrong overload
- **Location:** `SmBSplineSurface.cpp:3146`, `SmBSplineSurface::IsParallelToVector`.
- **What happens:**
  - It returns `SER(sTestVec.Unitize())`'s `SM_ERR_INVALID_INPUT` as an `SmBoolean`, so a zero vector reads as TRUE.
  - `smgu_LinePointDistance(LinePnt, LineEnd, Pnt, d)` binds the (point, direction) overload, so the second net point is used as the direction.
- **Trigger:** A zero test vector.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-40. `SmBSplineSurface::TrimWithDomain` v snapping uses `idxMin` for both boundaries
- **Location:** `SmBSplineSurfaceNLib.cpp:7455`, `SmBSplineSurface::TrimWithDomain`.
- **What happens:** The v loop's upper-boundary branch writes `idxMin`, so `idxMax` stays `-1` and `sm_CheckTrimBoundaries` never widens `vr`. The u loop's "slightly above the knot" branch does not set `idxMax`. Only the u boundaries are checked for being too close.
- **Trigger:** A v trim boundary just above an interior knot.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-41. `SmPlane::Reparameterize` does not move the origin in release builds
- **Location:** `SmPlane.cpp:3065` `SmPlane::Reparameterize`. The new origin `sOriginMax` (`:3086`) is used only by the `SM_DEBUG_CODE` check (`:3090`-`3099`). `m_vPosition` is never updated; only the domain and scales change (`:3115`-`3116`).
- **What happens:** The analytic plane no longer matches its NURB unless the new domain starts where the old one did.
- **Trigger:** Reparameterize a plane from `[0,1]^2` to `[2,3]^2`.
- **How found:** adversarial / edge-case fixture `edge_reparameterize_keeps_origin`.
- **Confirmed:** yes.
- **Severity:** wrong result (release-only).
- **Rust port:** Ported as is.

#### WR-42. `SmSurfaceCache::GetSurfaceContinuity` skips the last internal v knot
- **Location:** `SmSurfaceCache.cpp:1711` (`iv+1<rVConts.GetSize()-1`), `SmSurfaceCache::GetSurfaceContinuity` (def. `:1672`).
- **What happens:** The v loop stops one earlier than the u loop, so the continuity at the last internal v knot is never checked.
- **Trigger:** A surface whose only C0 v knot is the last internal one.
- **How found:** generated difftest case.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-43. `SmCurveBoundedSurface::ReadFromDB` reads UV boundaries with the 3D type
- **Location:** `SmCurveBoundedSurface.cpp:709`, `SmCurveBoundedSurface::ReadFromDB`.
- **What happens:** The UV boundaries are read with the type of the last 3D boundary (`l3DType`, `0` when there is none, so every UV boundary then fails with `SM_ERR`). The implicit-outer flag that was read is dropped.
- **Trigger:** A record with UV boundaries but no 3D boundaries.
- **How found:** generated difftest case (`WriteToDB`-layout ASCII records).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

#### WR-44. Small `SmSurface` helper bugs
- **Location:**
  - `SmSurface.cpp:11574` `sm_MergeDoubleArrays`: stops when either array is exhausted, dropping the other's tail.
  - `:3303` `SmDerivSurfDefinition::CreateCrossSectionCurves`: takes the third-derivative Taylor coefficient from the second derivatives.
  - `SmCrvOnSurf.cpp:811` `EvaluateSurface1stDerivDerivs`: the zero fill `for (ii = 2; ii > lNumDers; ii++)` never runs.
- **What happens:** Silently wrong merged arrays, cross sections and derivative slots.
- **Trigger:** `sm_MergeDoubleArrays([0,1,2], [0.5])`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as written.

#### WR-45. `smvol_LiftThirdDerivative` uses the wrong terms
- **Location:** `SmVolume.cpp:9851` (surface form), `:10126` (volume form), `smvol_LiftThirdDerivative`.
- **What happens:** `crDxxz` is used where `Dxzz` belongs in `rDuuv`, `rDuvv`, `rDuuw`, `rDuww`, `rDvvw`, `rDvww`. `rDuww` is built from `Fuv`/`Fv` instead of `Fuw`/`Fw`, and `rDvww` from `Fvv`/`Fv` instead of `Fvw`/`Fw`.
- **Trigger:** Third mixed derivatives of any compound volume (and of `SmSrfInVolume`).
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Keeps the formulas so results match.

#### WR-46. `SmTwistVolume` sign, equality and constructor bugs
- **Location:**
  - `SmTwistVolume.cpp:1045` `EvaluateSimple`: `Duw` is `(-per sin a, -per cos a, 0)`, but the header and `Evaluate` use `(-per sin a, per cos a, 0)`.
  - `:210`-`211` `operator==`: the minimum comparison overwrites the period comparison.
  - `:38`/`:132` constructors: compare `Unitize`'s `SM_ERR_INVALID_INPUT` with `SM_ERR`, so a zero axis skips the intended repair. A too-small period becomes `|dPeriodMinDist|`.
- **What happens:** Wrong mixed derivatives; twists that differ only in period compare equal.
- **Trigger:** Two twist volumes differing only in period: `==` is TRUE.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as written.

#### WR-47. `SmDatabaseIO` byte-swapping asymmetries
- **Location:** `SmDatabaseIO.cpp:632` `ReadBoolean` (never byte-swaps), `:547` `ReadBooleans` (does). `ByteSwap32(unsigned short)` always returns 0.
- **What happens:** Booleans read singly from a foreign-endian file come out wrong.
- **Trigger:** A big-endian binary file read on x86-64.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Kept as is.

#### WR-48. `SmBSplineVolume::HasDiscontinuitiesSimple` never reports geometric discontinuities
- **Location:** `source/SMLib/src/SmBSplineVolumeNLib.cpp:242` (`HasDiscontinuitiesSimple`, def. `:226`) -> `:606` (`CalculateContinuitiesSimple`) -> `source/SMLib/src/SmBSplineVolume.cpp:2364` (`EvaluateIsoParametricCurveSimple`, `SM_ERR` in its `#else` branch).
- **What happens:** With the default `bCalcGeometric = TRUE`, `CalculateContinuitiesSimple` fails at its first iso-curve (no `USE_VOLUMES`), leaving the knots in `rParams`, `rConts` empty and the minimum `SM_CT_CINFINITY`. `HasDiscontinuitiesSimple` ignores the status and returns `FALSE` with empty continuity lists, even for a volume with `C0` knot planes.
- **Trigger:** `HasDiscontinuitiesSimple(&d)` on any `SmBSplineVolume`, e.g. one with a cubic `U` knot vector holding a triple interior knot.
- **How found:** adversarial / edge-case fixture.
- **Confirmed:** yes (fixtures `hasdisc_*_geom`, `conts_*_e3_geom` of `surface.b_spline_volume_n_lib`).
- **Severity:** wrong result.
- **Rust port:** Ported as written.

#### WR-49. Representational volume continuities leave the last knot `SM_CT_UNDEFINED`
- **Location:** `source/SMLib/src/SmBSplineVolumeNLib.cpp:505`-`:518` (`SmBSplineVolume::CalculateContinuitiesSimple`, `bCalcGeometric == FALSE`).
- **What happens:** `rConts` is sized to the knot count but the loop fills only the first `count - 1` entries, so the last knot's continuity is the zero fill `SM_CT_UNDEFINED`, while the first (an end knot) gets a degree-minus-multiplicity value (`SM_CT_C0` when clamped) instead of the documented `SM_CT_DISCONTINUOUS`. The minimum therefore includes the start knot but not the end knot, and `HasDiscontinuitiesSimple(.., FALSE)` is `TRUE` for every clamped volume.
- **Trigger:** `CalculateContinuitiesSimple(SM_VP_U, .., FALSE)` on a trilinear unit cube volume: conts `[C0, UNDEFINED]`.
- **How found:** adversarial / edge-case fixture.
- **Confirmed:** yes (fixtures `conts_*_rep` of `surface.b_spline_volume_n_lib`).
- **Severity:** wrong result.
- **Rust port:** Ported as written.

#### WR-59. `SmBendVolume::EvaluateBoundingBoxSimple` pseudo box does not contain the image
- **Location:** `source/SMLib/src/SmBendVolume.cpp:1555` (pseudo box `dXMin`), `:1480` (axis box `dYMax`, unbounded-`u` branch).
- **What happens:** The pseudo box along the mid-angle direction takes its `x` minimum as `sin(thetaEnd) * uMin` (the `cos` variable on the line above is commented out). Once the half angle exceeds `pi/2` the image reaches negative `x` (down to `cos(thetaEnd) * uMax`), but the box starts at a positive value, so the "bounding" pseudo box misses part of the mapped box. Separately, with `u` reaching `SM_INFINITE_PARAMETER` and both end angles at most `0`, the axis box's `y` maximum uses the cosines of the end angles instead of the sines (too large, still containing).
- **Trigger:** Bend with neutral distance `1`, parameter box `u` in `[0.5, 1.5]`, `v` in `[-2, 2]`: pseudo `x` interval starts at `sin(2) * 0.5 = 0.45`, while the corner at `u = 1.5, v = 2` lies at `x = cos(2) * 1.5 = -0.62`.
- **How found:** read during porting.
- **Confirmed:** yes (fixtures `b_box_unit_both_poles`, `b_box_unit_inf_u_down` of `surface.bend_volume` compare the values).
- **Severity:** wrong result.
- **Rust port:** Ported as written.

#### WR-60. `SmBendVolume::TrimParamBoundingBoxSimple` keeps stale output for boxes crossing the period end
- **Location:** `source/SMLib/src/SmBendVolume.cpp:2180`-`:2184`.
- **What happens:** Each `v` end is clamped to the primary period separately with `SM_CLAMP_TO_PERIOD`, so a box crossing `v = pi d` gets `vMin > vMax`. The final `SetMinMax` then fails, its status is ignored, and the function returns `SM_SUCCESS` with the caller's output box unchanged (whatever it held before).
- **Trigger:** Bend with neutral distance `1`, parameter box `v` in `[2, 4.5]`.
- **How found:** adversarial / edge-case fixture.
- **Confirmed:** yes (fixture `trim_unit_across_period` of `surface.bend_volume`).
- **Severity:** wrong result.
- **Rust port:** Ported as written.

### Intersection

#### WR-50. `AddStartPoint` "replaces" a bounded start point by assigning a local pointer
- **Location:** `source/SMLib/src/SmSurfaceIntersector.cpp:3901` `SmSurfaceIntersector::AddStartPoint`.
- **What happens:** For a bounded/exact start point pair whose stored point is the bounded one, the "replace pTSP with rStartTSP" branch only sets the loop variable `pTSP = &rStartTSP` and reports a match. The exact candidate is dropped and the bounded point stays in `m_vStartPoints`, the opposite of what the comment (and the function header) describe.
- **Trigger:** A bounded start point already in the list, then an exact one within `2 * m_dThisApproxTol3d` with a matching curve tangent.
- **How found:** read during porting.
- **Confirmed:** no (the branch needs `smsurf_EvaluateNormalSection`, which the Rust side does not have yet, so those difftest cases are deferred).
- **Severity:** wrong result.
- **Rust port:** Faithful (the candidate is not added).

#### WR-51. `IsPointOnStartPoint` compares squared distances with a length tolerance
- **Location:** `source/SMLib/src/SmSurfaceIntersector.cpp:7515` `SmSurfaceIntersector::IsPointOnStartPoint`.
- **What happens:** `DistanceBetweenSquared(...) < m_dThisApproxTol3d`: the effective tolerance is `sqrt(tol)`, far looser than intended for small tolerances (and tighter above 1).
- **Trigger:** Any call; e.g. `tol = 1e-6` accepts points 1e-3 apart.
- **How found:** read during porting.
- **Confirmed:** yes (`intersect.surface_intersector_p1` difftest, `edge_start_points` and generated `on_start` ops agree with the squared test).
- **Severity:** wrong result.
- **Rust port:** Faithful.

#### WR-52. `SER` inside `SmBoolean` functions reports a failure as `TRUE`
- **Location:** `source/SMLib/src/SmSurfaceIntersector.cpp:4195` `sm_IsPointOnCurve` and `:7376`, `:7391` `SmSurfaceIntersector::IsPointOnCurve`.
- **What happens:** `SER(status)` returns the non-zero `SmStatus` from a function returning `SmBoolean`, so a failing point solve, curve evaluation or curve drop makes the point count as "on the curve".
- **Trigger:** A curve whose `EvaluatePoint` or `DropPoint` fails, or a failing Hermite point solve.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Faithful (`Ok(true)` on such an error).

#### WR-53. `TestSpanAccuracy` compares a tangent length with an angle tolerance
- **Location:** `source/SMLib/src/SmSurfaceIntersector.cpp:3448` `SmSurfaceIntersector::TestSpanAccuracy` (upstream's own `GWC_NEEDS_WORK` note).
- **What happens:** `rNextTSP.CrvDeriv().Length() < m_dSteppingParallelTolRadians` (`SM_EFF_ZERO`): a length is tested against a radian tolerance, so the check only catches (near) zero tangents.
- **Trigger:** A next point with a tangent shorter than `1e-12`.
- **How found:** read during porting.
- **Confirmed:** yes (`intersect.surface_intersector_p1` difftest, `edge_span_angle`).
- **Severity:** wrong result (unit mix-up).
- **Rust port:** Faithful.

#### WR-55. `SmSurface::GetFlatCorners` destroys its return value while filling the corner points
- **Location:** `SmSurface.cpp:12691`, `SmSurface::GetFlatCorners` (def. `:12655`).
- **What happens:** The optional-output block tests `if(lRtn &= SM_FC_UMIN_VMIN)` (and so on) with `&=`, so asking for the points masks the flags down to at most one bit (usually none), and later corners are evaluated only if all earlier masks kept a bit.
- **Trigger:** `GetFlatCorners(&points)` on a surface with two or more flat corners.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`Surface::flat_corners`; `IsFlatCorner` is not ported yet).

#### WR-56. `sm_GoodDropResult` swaps the guess and result distances
- **Location:** `SmSurface.cpp:10349`, `sm_GoodDropResult` (file static, def. `:10256`).
- **What happens:** `dDsqGuess` holds the distance to the result point and `dDsqResult` the distance to the guess point, so the final "result farther than the guess" test rejects drops that improved on the guess. The caller then runs the expensive global solve.
- **Trigger:** A local drop whose result is closer to the point than the guess, with a small residual that passes the earlier tests.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result (performance: an unneeded global solve).
- **Rust port:** Reproduces it (`good_drop_result`).

#### WR-57. `SmSurface::FindDegenParamForDirection` compares a sample fraction with a parameter
- **Location:** `SmSurface.cpp:6948`, `SmSurface::FindDegenParamForDirection` (def. `:6757`).
- **What happens:** The "nearly as good and closer to the input" tie-break compares `dThisParam` (a fraction in `[0, 1]` of the sample domain) with `dS` (the actual parameter), so it only means "closer" when the singular parameter runs over `[0, 1]`.
- **Trigger:** A degenerate boundary whose singular parameter interval is not `[0, 1]`, with two nearly equally good samples.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result (start guess of the Newton solve).
- **Rust port:** Reproduces it.

#### WR-58. `SmPointClassification::GetGap3d` returns `SM_ERR` as a distance
- **Location:** `SmPointClass.cpp:464`, `SmPointClassification::GetGap3d` (def. `:418`).
- **What happens:** When the object point has no single drop point on the given surface (even at 100x the zone tolerance), the function, which returns a `double` gap, returns `SM_ERR` (1001.0), which callers read as a large gap.
- **Trigger:** `GetGap3d(pSurface)` for an object point far from `pSurface`.
- **How found:** read during porting.
- **Confirmed:** no (the surface point solve is not ported yet, so these cases defer).
- **Severity:** wrong result.
- **Rust port:** Returns `SM_ERR` as the gap value too.

## Docs

#### DOC-1. `NL_LEFT` / `NL_RIGHT` documentation is swapped
- **Location:** `NL_BasisBasic.cpp:1433`-`1434` (parameter comment of `N_BasisFindSpan`), `:1506`, `:1522` (case comments).
- **What happens:** The comments say `NL_LEFT` means `U[spn] < u <= U[spn+1]`. The code (`while( u LT U[mid] OR u GE U[mid + 1] )`) does the opposite: `NL_LEFT` gives `U[spn] <= u < U[spn+1]`. `NL_RIGHT` is likewise swapped.
- **Trigger:** `u` equal to an interior knot.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** docs.
- **Rust port:** `Side` documents what the code does.

#### DOC-2. `N_GetCompatibleKnotVector` doc says multiplicities are not carried over
- **Location:** `NL_BasisBasic.cpp:2339` (`N_GetCompatibleKnotVector` doc comment); behaviour in `:2483` `N_GetCompatibleKnotVectorToTol`.
- **What happens:** Each distinct knot gets the highest multiplicity it has in any vector, unlike the doc says.
- **Trigger:** Vectors `[0,0,.5,1,1]` and `[0,0,.5,.5,1,1]`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** docs.
- **Rust port:** Faithful to the code.

#### DOC-3. "Small topology" tolerances are 1000x larger, not tighter
- **Location:** `SmTol.cpp:661` (also `:806`, `:892`), `SmTol::GetZoneTol3d`.
- **What happens:** `GetZoneTol3d(SM_ST_SMALL_SIZE)` returns `s_LargeSmallSizeRatio * s_ZoneTol3d` (default 1000x), despite comments that promise tighter tolerances. The `const SmContext*` overloads ignore the small flag.
- **Trigger:** `GetZoneTol3d(SM_ST_SMALL_SIZE)`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** docs.
- **Rust port:** Faithful to the code.
