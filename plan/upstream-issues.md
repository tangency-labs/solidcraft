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
by area within each severity, so IDs are in order within each area but not across a
whole severity.

**Numbering.** A new entry takes the next free number in its series (`UB-`, `HANG-`,
`ND-`, `WR-`, `DOC-`) at the moment its PR merges. If another PR took that number in the
meantime, whoever merges renumbers the new entry and every reference to it (here, in
`plan/findings.md`, `plan/modules.toml` and code comments). Numbers are not reused.

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
- **Reconfirmed (boolean.merge_p5, PR #161):** `SmallBox.smb` passed to the shared `ReadFromFile` fixture builder segfaulted in `SmTopologyData::ReadFromDB` (GDB backtrace, 2026-10-16). The new unit's FFI validator rejects the part-container header before invoking that B-rep reader; solid-stage fixtures use the actual B-rep stream `box_with_large_fillet.smb`.

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

#### UB-91. `SmSurfOfExtrusion::ReadFromDB` reads an uninitialised generator type from a failed ASCII stream [PSIRT]
- **Location:** `source/SMLib/src/SmSurfOfExtrusion.cpp:4560` `SmSurfOfExtrusion::ReadFromDB` (def. `:4480`), through `SmDatabaseIO::ReadType` (`SmDatabaseIO.cpp:400`); also the locals `eOrientation` and `bSwapUV` (`:4506`-`:4507`).
- **What happens:** The ASCII branch extracts the record without checking the stream. Once an extraction fails, every later `>>` leaves its (uninitialised) local untouched, and the ASCII `ReadType` returns `SM_SUCCESS` without writing `lGenCurveType`, which then selects the curve reader.
- **Trigger:** An ASCII `SmSurfOfExtrusion` record cut short, or with a non-numeric field, before its generator's `Object Type` line.
- **How found:** read during porting (`surface.surf_of_extrusion_p2`).
- **Confirmed:** no. The FFI guard only passes records the strict stream simulator of `topology.brep_data` reads completely.
- **Severity:** crash/UB (uninitialised read).
- **Rust port:** `read_type` reports the unread type and `read_from_db` returns `InvalidInput`.

#### UB-92. `SmSurfOfExtrusion::ReadFromDB` casts any curve kind to `SmBSplineCurve` [PSIRT]
- **Location:** `source/SMLib/src/SmSurfOfExtrusion.cpp:4562`-`4565` `SmSurfOfExtrusion::ReadFromDB`.
- **What happens:** The generator read by `SmCurve::ReadFromDB` is only checked with `SM_ASSERT_MSG(pGenCurve->IsKindOf(SmBSplineCurve_TYPE))` (compiled out in release) and then stored as `SmBSplineCurve*`. A composite (or other non-B-spline) curve record makes every later generator call use the wrong class layout.
- **Trigger:** An `SmSurfOfExtrusion` record whose generator is an `SmCompositeCurve` record.
- **How found:** read during porting (`surface.surf_of_extrusion_p2`).
- **Confirmed:** no. The FFI guard only passes `SmBSplineCurve` generator records; the shim also refuses a read generator that is not one.
- **Severity:** crash/UB (type confusion).
- **Rust port:** Returns `InvalidInput`.

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

#### UB-99. `CreateSkinnedSurface` retries skinning with derivative surfaces whose knot vector was freed
- **Location:** `source/SMLib/src/SmBSplineSurfaceNLib.cpp:5613` and `:5636` (`SmBSplineSurface::CreateSkinnedSurface`: the first `N_CreateSkinSrfBoundaryContinuity` call, then the retry without knots, both with the same `pDirArray`); `NL_SrfAdv.cpp:14275` (`N_CrvFromCPolygonAndKnotVector( &curA, pol, qs, knv )`), `:14434` (`N_CrvsMakeCompatibleKnotTol`), `:13316` (`N_AdjustDerivSrf( der[0], ... )`); `NL_CrvTool.cpp:1502` (`N_CrvRefine` frees the old curve in place).
- **What happens:** In `N_CreateSkinSrfBoundaryContinuity` the cross-boundary derivative curve `curA` is built over `der[0]`'s own `NL_KNOTVECTOR` (pointer shared, not copied). Making the curves compatible refines `curA` in place, and `N_CrvRefine` frees the old knot vector, so `der[0]` is left pointing at freed memory. If that first call then fails, `CreateSkinnedSurface` calls it again with the same derivative surfaces, and `N_AdjustDerivSrf` → `N_BezSrfExtend4D` → `N_SrfGetCPtsDegreesAndKnots` reads the freed knot vector.
- **Trigger:** `SmEiffelTowerCrossSectionFSG::CreateSurface` (`SmFilletCustomSolver.cpp:845`) on two quadratic, 4-point, non-rational rails over the same knot vector `[-0.9038, x3, 0.6515, 2.2069 x3]` with three fillet points, `tol = 1e-6`: the `blend.fillet_custom_solver` difftest case `gen0137` (seed `12772265007024949959`). That input is valid; the skinning branch's first attempt fails and the retry reads freed memory. Other callers that pass derivative surfaces to `CreateSkinnedSurface` (`SmFilletStandardSolver.cpp:3904`, `:4458`, `SmPrimitiveCreation.cpp:6811`, `SmSurface.cpp:3536`) may reach the same retry; not tested.
- **How found:** ASan build of the C++ reference (SMLib, SM_API and the shims built with `-fsanitize=address`) running the `blend.fillet_custom_solver` difftest corpus.
- **Confirmed:** yes. ASan reports `heap-use-after-free` (16-byte region allocated in `N_BezSrfExtend4D` via `N_AdjustDerivSrf` in the first call, freed by `N_FreeCrv` in `N_CrvRefine` from `N_CrvsMakeCompatibleKnotTol`, read in the retry). The release build does not crash on this case and its output matches the Rust port, but that result depends on the freed memory's contents.
- **Severity:** crash/UB (heap use after free).
- **Rust port:** Retries with the derivative surfaces as the first attempt left them; it reads no freed storage. The FFI guard cannot predict a failed first skinning attempt without running it, so it does not reject such inputs (see `plan/findings.md`, `blend.fillet_custom_solver`).

#### UB-107. `CreateExtendedSurface` retries an in-place extension on half-written arrays
- **Location:** `source/SMLib/src/SmBSplineSurfaceNLib.cpp:4568`, `:4630`, `:4697`, `:4759` (`SmBSplineSurface::CreateExtendedSurface`: `while(0 != N_SrfExtendByDist(&esur, dExtDist, ..., &esur, ...))` halves the distance and retries on the same `esur`); `NL_SrfAdv.cpp:6137`-`6141` (`N_SrfExtendByParamDist`, in place: `N_AllocSrfArrays( surP, ... )` gives `surP` fresh arrays, the old ones stay in the local `surA`), `:7035`, `:7104` (`NL_ERROR( NL_WEI_ERR )` in the `NL_CMAX` branch exits before every new control point is written; `surA` is freed only on success, `:7207`).
- **What happens:** When an in-place extension fails part way (for example an `NL_CMAX` extension that would create a weight outside the allowed range), `esur` is left on the newly allocated, partly written arrays. The retry at half the distance reads them, including the entries never written. Its result depends on what earlier calls left in that memory.
- **Trigger:** `CreateExtendedSurface(dist = 0.28814945128446756, SM_CT_CINFINITY)` (both directions) on the rational bilinear-by-linear B-spline of `surface.surface_p2` generated case `gen0016` (seed `9548444236539953400`; form 11, 2x3 net, v knots `[0, 0, 0.8105925733594667, 1, 1]`): the V-max extension at the full distance fails with `NL_WEI_ERR` and is retried.
- **How found:** generated difftest case (`surface.surface_p2`, once `IsSingularity` stopped deferring it).
- **Confirmed:** yes. In a fresh process the call returns `SM_ERR` every time; after a few other extensions in the same process the same call returns `SM_SUCCESS`. The separate, non-in-place `N_SrfExtendByDist` calls of that sequence (`nlib.srf_adv_p1` shim, error state cleared) agree with the Rust port at every step.
- **Severity:** crash/UB (uninitialised read).
- **Rust port:** `N_SrfExtendByDist` returns a new surface, so a failed attempt leaves the surface unchanged and the retry extends the original (the documented in-place deviation of `nurbs::srf_adv::p3`). The `surface.surface_p2` generator leaves out extension cases whose first attempt at some side fails (see `plan/findings.md`).

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
- **How found:** read during porting; PR #164 maintainer review reproduced the path through healer target catch-up.
- **Confirmed:** yes by the maintainer's release-C++ SIGSEGV in `FindCoincidentPairs <- Cache_CoinVertices <- HealTgts <- UpdateTargetLists` (PR #164). A box with cached edge/vertex/face-gap props, forged done op 6/8/9/16/26, no initial coincidence cache, and an existing vertex added to its targets reaches the empty incremental bucket list.
- **Severity:** crash/UB.
- **Rust port:** Checks no bucket. The healer FFI shim also refuses incremental catch-up with remaining old vertices and an uninitialized voxel extent. No prior vertices, a real initial coincidence cache, catch-up below step 6, and removal of all prior vertices remain accepted (direct FFI regressions in PR #164).

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

#### UB-85. `SmTopologySolver::MayContainAnswer` reads uninitialised ray/box results
- **Location:** `SmTopologySolver.cpp:2909`, `SmTopologySolver::MayContainAnswer` (def. `:2716`), `SM_SO_RAYFIRE` branch.
- **What happens:** The status of `SmExtent3d::IntersectLine` is ignored. When it fails (`SM_ERR_INVALID_INPUT` for a ray vector shorter than `SM_EFF_ZERO` or an undefined box), `lNumFound`, `dTEnter` and `dTExit` are read uninitialised and decide whether the branch is culled.
- **Trigger:** A ray-fire topology solve whose option vector is (near) zero.
- **How found:** read during porting (`intersect.topology_solver_p1`).
- **Confirmed:** no (the difftest guard keeps a ray vector shorter than `1e-6` from `MayContainAnswer`; zero and tiny option vectors elsewhere are defined upstream and are difftested by `fixtures/intersect.topology_solver_p1/zero_vectors.json`).
- **Severity:** crash/UB (uninitialised read).
- **Rust port:** Returns `InvalidInput`.

#### UB-86. Topology edge solvers dereference a curve's missing edge
- **Location:** `SmTopologySolver.cpp:557`/`:582` `SmTLSPointEdge::SolveIt`, `:1079`/`:1101` `SmTLSCurveEdge::SolveIt`, `:1359`/`:1380` `SmTLSEdgeEdge::SolveIt`.
- **What happens:** `pCurve->GetEdge()` is only checked by an `SM_ASSERT_MSG` (compiled out); with no shape (`m_pShape == NULL`) a curve that defines no edge is dereferenced as `NULL->GetInterval()`.
- **Trigger:** Any of these local solvers called on a free curve (no owning edge) outside a shape solve.
- **How found:** read during porting (`intersect.topology_solver_p1`).
- **Confirmed:** no (the difftest guard passes only curves that own an edge).
- **Severity:** crash/UB (bad argument from calling code).
- **Rust port:** Returns `InvalidInput`.

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

#### UB-80. `SmSurfOfExtrusion::EvaluateSTEP` lets order 5 overrun `sm_CrvDerivs`
- **Location:** `SmSurfOfExtrusion.cpp:920`-`922` (`EvaluateSTEP`, def. `:897`: `lMaxDeriv = 5; // 5: MAXDER in NLib`), `:959` (`m_pGenCurve->EvaluateSTEP(dU, lHighestUDeriv, ..)`); `SmNurbsCrv.cpp:1281` `sm_CrvDerivs` (see UB-33).
- **What happens:** The guard allows order 5, but `NL_MAXDER` is 4. A B-spline generator evaluates through `SmBSplineCurve::Evaluate` -> `sm_CrvDerivs`, which writes past its fixed derivative rows: `*** stack smashing detected ***`. Analytic generators (`SmLine`, `SmCircle`, `SmEllipse`) evaluate order 5 fine.
- **Trigger:** `EvaluateSTEP(uv, 5, 5, ..)` on an extrusion of any `SmBSplineCurve` (degrees 1 to 3 tried).
- **How found:** crash in the C++ reference run (`surface.surf_of_extrusion_p1` generated cases).
- **Confirmed:** yes.
- **Severity:** crash/UB (stack buffer overflow).
- **Rust port:** The B-spline evaluation rejects order 5 and `evaluate_step` returns the error. The harness keeps order 5 of a B-spline generator away from the C++.

#### UB-81. `sm_IntersectGenerators` reads unset offsets and a possibly NULL curve list
- **Location:** `SmSurfOfExtrusion.cpp:2978`-`2986` (`sm_IntersectGenerators`, def. `:2658`; arrays declared `:2725`-`:2726`).
- **What happens:** For a range solution, `pOpt3DCurves->GetSize()` is read before the `pOpt3DCurves != NULL` test, and `pOffsetCurve[jj][k]` / `dOffsetLength[jj][k]` are compared although they are only set when an end of the overlap interval equals one of four bounds exactly (`==` on doubles) and the end overlaps at all.
- **Trigger:** A range solution of the projected generator intersection whose overlap ends match none of the four bounds, or a caller passing `NULL` curves.
- **How found:** read during porting.
- **Confirmed:** no (its projected solve, `SmCurve::GlobalCurveSolve`, is not ported yet, so no range case is compared).
- **Severity:** crash/UB.
- **Rust port:** A missing curve list has size 0; unset offsets compare unequal (the case falls to "too complicated").

#### UB-82. `SmSurfOfExtrusion` constructors and copy constructor trust failed calls
- **Location:** `SmSurfOfExtrusion.cpp:104`-`109` and `:204`-`209` (constructors: `SE(IsOnPlane(..))`, then read `bOnPlane`); `:70`, `:162`, `:170` (`SE` of the generator evaluations whose points are then used); `:252`-`:259` (copy constructor: `SE(SM_ERR)` for a NULL or non-B-spline generator copy, then `m_pGenCurve->SetOwner(this)`).
- **What happens:** A failed `IsOnPlane` leaves `bOnPlane` uninitialised; a failed evaluation leaves the tolerance point or origin unset; copying a surface without generator dereferences NULL.
- **Trigger:** Copying the empty I/O surface; a generator whose evaluation fails.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns the error (`SM_ERR_NULL_POINTER` for the copy).

#### UB-83. `SmSurface::GetDomainWithContinuity` dereferences a missing surface cache
- **Location:** `SmSurface.cpp:12056`, `SmSurface::GetDomainWithContinuity` (base body).
- **What happens:** When `smsurf_GetSurfaceCache` returns `NULL` (neither a trimmed surface cache nor a surface cache can be built), `SE(SM_ERR)` only reports it and the body goes on to call `GetUContinuitiesArray()` through the null pointer.
- **Trigger:** `GetDomainWithContinuity` on a curve-bounded surface or plane whose cache build fails (generated `surface.surface_p1` `dom_cont` cases).
- **How found:** crash in the C++ reference run.
- **Confirmed:** yes (segfault; the `surface.surface_p1` shim now checks the cache first and reports `SM_ERR`).
- **Severity:** crash/UB.
- **Rust port:** Returns the cache build error.

#### UB-84. `SmSurface::CalculateBoundingBox` of a polar box recurses without bound
- **Location:** `SmSurface.cpp:5397`, `SmSurface::CalculateBoundingBox` (base body), with `SmSurfaceCache.cpp:1184`, `SmBezierAux2d::BuildPolarBox`.
- **What happens:** For a polar box, the base body asks the top node of the surface's own cache for it (`SmBezierAux2d::GetPolarBox`). Building that box calls the node surface's `CalculateBoundingBox`, which for a curve-bounded or STEP surface is the same base body again. Nothing ends the cycle, so the stack overflows.
- **Trigger:** `CalculateBoundingBox` with `pPolarBox != NULL` on an `SmCurveBoundedSurface` or `SmSTEPSurface` (any surface kind that does not override `CalculateBoundingBox`).
- **How found:** the maintainer's review of `surface.surface_p1` (PR #43), by printing the polar box in the shim; gdb shows about 58k frames cycling through those three functions.
- **Confirmed:** yes (stack overflow in the C++ reference build).
- **Severity:** crash/UB.
- **Rust port:** `Surface::base_calculate_bounding_box` and the surface caches' `calculate_bounding_box` refuse a polar box of those kinds with `InvalidInput` (bounded on wasm). The `surface.surface_p1` FFI never asks the C++ for it.

#### UB-87. `SmCone::CreateIsoCircleFromPoint` and `GetHeight` read uninitialised parameters
- **Location:** `SmCone.cpp:1679`-`1682` `SmCone::CreateIsoCircleFromPoint` (def. `:1620`); `SmCone.cpp:2265`-`2266` `SmCone::GetHeight` (def. `:2251`).
- **What happens:** `CreateIsoCircleFromPoint` ignores the statuses of `STEPInversion` and `ConvertUVFromSTEPToNURBS`; when the generator drop in `SmSurfOfRevolution::STEPInversion` finds no solution the height of `sStepUV` is never written and is used anyway. `GetHeight` ignores `EvaluatePoint`'s status and subtracts uninitialised points when the conversion fails (no polar curve).
- **Trigger:** `CreateIsoCircleFromPoint` with a reference point just outside the height interval of a steep bounded cone and a tolerance below the in-plane gap the clamp leaves; `CreateIsoCircleFromPoint` on `SmCone::CreateCanonical(O, 1, 5e-11 deg, NULL)` (no `bSwapUV`) at `(-1, 1e-13, 0.7)`, where the polar conversion fails and `sNurbUV.y` (a defaulted `SmVector2d`) is used unset; `GetHeight` of the cone of UB-81.
- **How found:** read during porting (`surface.cone_p1`).
- **Confirmed:** no. The shim replays the inversion and refuses those inputs.
- **Severity:** crash/UB (uninitialised read).
- **Rust port:** Returns the error.

#### UB-88. `SmCone` STEP-to-NURB conversion dereferences a missing polar curve
- **Location:** `source/SMLib/src/SmEllipse.cpp:489`, `SmPolarConversion::ConvertToNURBSParameter` (def. `:467`), reached from `SmCone.cpp:1808` `SmCone::ConvertUVFromSTEPToNURBS` and `:1682` `SmCone::CreateIsoCircleFromPoint`.
- **What happens:** `m_cpCurve->GetSTEPInterval()` is called without the `NER(m_cpCurve)` check that `ConvertToPolarParameter` has. A cone whose polar converter could not be set up has no curve; `SmSurfOfRevolution::MakeNurbWithSweepParams` ignores `RebuildPolarConverter`'s status, so `CreateCanonical` still succeeds.
- **Trigger:** `new SmCylinder(O, X, Y, -1.8, 0, 360, 4.3, FALSE, FALSE, FALSE)` (a negative radius) and `MakeNurb()`, which succeeds without polar curve, then `ConvertUVFromSTEPToNURBS`.
- **How found:** read during porting (`surface.cone_p1`).
- **Confirmed:** no. The shim refuses the op for a cone without polar curve.
- **Severity:** crash/UB (null dereference).
- **Rust port:** Returns `InvalidInput`.

#### UB-100. `SmDerivSurfDefinition::GetSampleValues` dereferences a missing tangent reference
- **Location:** `source/SMLib/src/SmSurface.cpp:3127`, `SmDerivSurfDefinition::GetSampleValues` (def. `:2941`); the check it relies on is `SmDerivSurfDefinition::AssertValid` test 2 (`:2286`).
- **What happens:** For `SM_TT_DIR_TO_REFERENCE_CURVE`, `m_pTangentReference->EvaluatePoint` is called unchecked. `AssertValid` accepts a `NULL` reference (its last clause, `m_pTangentReference == NULL`, holds for every tangency type), so the definition passes the state check and the call dereferences `NULL`.
- **Trigger:** `GetSampleValues` with `m_eTangencyType = SM_TT_DIR_TO_REFERENCE_CURVE`, `m_pTangentReference = NULL` and a boundary curve that drops onto the surface.
- **How found:** crash in the C++ reference run (generated difftest case of `surface.surface_p6`).
- **Confirmed:** yes (segfault).
- **Severity:** crash/UB (null dereference).
- **Rust port:** Returns `SM_ERR_NULL_POINTER`; the shim refuses the input.

#### UB-123. `SmCone::IntersectWithCone` dereferences an absent tolerance for coincident cylinders
- **Location:** `source/SMLib/src/SmCone.cpp:2832,2860`, both `IntersectSurfaceBoundaries` calls in the `bCoincidence` branch of `SmCone::IntersectWithCone`.
- **What happens:** The routine computes a default local `dTol` but dereferences `pdOptApproxTol3d` instead when passing the tolerance to the boundary calls.
- **Trigger:** Distinct objects representing coincident, overlapping cylinders with `pdOptApproxTol3d == NULL`, after the bounding-box, angular, radial and axial early exits.
- **How found:** read during porting (`surface.cone_p3`, PR #131).
- **Confirmed:** no. Native guard tests reject this path and preserve defined default-tolerance calls (same object, disjoint heights, nested radii); an intentional undefined null dereference was not executed.
- **Severity:** crash/UB (null dereference).
- **Rust port:** Represents optional tolerances with `Option`; coincident-boundary calls currently defer to the missing Cone/Cylinder adapter in `surface.surface_p2` (#76/#133); the boundary algorithm itself merged in #120. The FFI adapter rejects exactly the native undefined path before its boundary calls.

### Intersection

#### UB-89. `SmVolume::ReadFromDB` fills NULL volumes and uses unread map types [PSIRT]
- **Location:** `SmVolume.cpp:8104`, `SmVolume::ReadFromDB` (the `SmVolume_TYPE` branch).
- **What happens:** (1) A next map of type `SmVolume_TYPE` recurses with `rpNewVolume->m_pNextMap` (NULL) as the volume to fill, and the branch writes `rpNewVolume->m_pNextMap` through it. A top-level `SmVolume_TYPE` read does the same with the caller's NULL. (2) `rDB.ReadType(lNextType)` / `ReadType(lOrientType)` statuses are ignored; at end of data the type stays uninitialised and is switched on. (3) The orient map read is cast to `SmTransform*` whatever its type, and a failed read leaves it NULL; `RefreshInvOrientMap` then copies, inverts and simplifies it (a NULL or mistyped object). The nested reads' statuses are ignored throughout.
- **Trigger:** An ASCII transform volume whose base data says `1` then `Object Type 22001` (next map), or `1` then `Object Type 22011` / `22004` (orient map), or ends right after a `1` flag.
- **How found:** read during porting.
- **Confirmed:** no. The harness's strict text grammar rejects these inputs.
- **Severity:** crash/UB.
- **Rust port:** Returns `SolidError::Unsupported`, which the derived readers do not ignore. Nested reads are bounded by `DatabaseReader::enter_nested`.

#### UB-90. Volume inverse maps reach UB-42 through `MapPseudoBox` and `EvaluateBoundingBox`
- **Location:** `SmVolume.cpp:3907` `InvMapPoint` -> `:2058` `MapPseudoBox` (trim branch: `EvaluateBoundingBoxSimple(sParamBox, NULL, ...)`); `:3411` `InvEvaluatePoint` and `:4512` `GlobalPointSolve` -> `:1692` `EvaluateBoundingBox(*pOptParamDomain, NULL, ...)`.
- **What happens:** With a search domain and compounding, an unoriented `SmTransform` map with next maps (or, for `EvaluateBoundingBox`, any transform map, and the orient map for in-space boxes) receives a NULL parameter pseudo box, and `SmTransform::EvaluateBoundingBoxSimple` dereferences it (UB-42).
- **Trigger:** `InvMapPoint(P, NULL, ok, pts, gaps, &domain, TRUE)` on `SmTransform` -> `SmTwistVolume`.
- **How found:** crash in the C++ reference run (segfault in `SmPseudoBox::AreAligned` under `InvMapPoint` -> `MapPseudoBox`).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Returns `SolidError::Unsupported`; the FFI guard of `surface.volume_p2` models these call paths and rejects them.

#### UB-74. `PointBasedSurfaceIntersect` writes past its 20-point array
- **Location:** `source/SMLib/src/SmSurface.cpp:30588` `SmSurface::PointBasedSurfaceIntersect` (UV point version).
- **What happens:** The points go into `SmTsectPnt saTsectPnts[20]`; the size limit and the equal-length check of the two point arrays are `SM_ASSERT`s only, so 20 or more points write past the stack array, and a shorter second array is read past its end.
- **Trigger:** 20 point pairs, or two UV point arrays of different lengths.
- **How found:** read during porting (`intersect.surface_intersector_p1`).
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** `Surface::point_based_surface_intersect` returns `InvalidInput` for those inputs.

#### UB-97. `ComputeSingularityPoint` can write past `aSingDirections[8]`
- **Location:** `source/SMLib/src/SmAdvSurfaceIntersector.cpp:164` (`SmVector3d aSingDirections[8]`), filled by `sm_AddSingDir` (`:54`) from the Dupin indicatrix intersections at `:616`-`:685`; `aSingUVSurf1/2[8]` and `lInsideIndex[8]` at `:775`-`:776`.
- **What happens:** Every intersection of the above (and below) indicatrix curves of the two surfaces adds a direction unless one within `acos(0.9)` (about 26 degrees) is stored already. Up to 13 directions pass that test, but the array holds 8 and `sm_AddSingDir` does not check the count (upstream's own comment: "we have experienced examples with 8 vectors"). Saddle points generate four hyperbolas per surface, two of which are different conics (see WR-69), so the count is not bounded by the 4 + 4 of two central conics.
- **Trigger:** Two non-planar surface points with parallel normals, no shared principal curvature, whose indicatrices meet in more than 8 directions more than 26 degrees apart.
- **How found:** read during porting.
- **Confirmed:** no (the difftest shim predicts the count and does not run such inputs; no generated case reached it).
- **Severity:** crash/UB.
- **Rust port:** the array keeps 8 slots; a ninth direction is `InvalidInput`.

#### UB-79. `SmPointClassification` calls `SmObject` virtuals on an `SmPoint3d*`
- **Location:** `SmPointClass.cpp:668` (`ComputePointParameters`: `SmTol::GetZoneTol3d(m_pObject)`), `:435` (`GetGap3d` with a surface, through `GetObjZoneTol3d`), `:493` (`IsConnectedTo`: `GetType`), `:917` (`MergeIntoObject`: `GetType`).
- **What happens:** `SM_PC_POINT` classifications store an `SmPoint3d*` cast to `SmObject*` (`SetClassObject`), and these paths call virtual `SmObject` members on it. `SmPoint3d` has no `SmObject` vtable, so the call is undefined behaviour.
- **Trigger:** `SetClassObject(SM_PC_POINT, (SmObject*)&point)` followed by `ComputePointParameters`, `GetGap3d(surface)`, `IsConnectedTo` or `MergeIntoObject`.
- **How found:** read during porting.
- **Confirmed:** no (the harness keeps these calls away from the C++).
- **Severity:** crash/UB.
- **Rust port:** Treats a point like any other non-topology object (system zone tolerance, not connected, not mergeable).

#### UB-93. `SmSurfOfExtrusion::IntersectWithCylinder` makes the iso circle of an unbounded cylinder from unset values
- **Location:** `source/SMLib/src/SmSurfOfExtrusion.cpp:3223` `SmSurfOfExtrusion::IntersectWithCylinder` (def. `:3164`), then `:3255` `sm_IntersectGenerators(*this, dTol, *pOtherGenCurve, ...)`.
- **What happens:** The other generator is `CreateIsoCircleFromPoint` at the bottom of the cylinder's height interval, whose status is ignored. For an unbounded cylinder that point lies at `-SM_INFINITE_PARAMETER` on the axis, where the cone's STEP inversion leaves its outputs unset and `CreateIsoCircleFromPoint` reads them (UB-80); a tolerance of `0` does the same for bounded cylinders. When the circle stays `NULL` and the extrusion is parallel to the axis, `sm_IntersectGenerators` dereferences it.
- **Trigger:** `IntersectWithCylinder` with `SmCylinder::CreateCanonical(O, 0.6)`, or with a bounded `SmCone` of equal radii and `pdOptApproxTol3d = 0`.
- **How found:** generated difftest cases (`surface.surf_of_extrusion_p2`: the shim's replay of the inversion rejected the generated unbounded cylinders and zero tolerances; the generator now avoids them).
- **Confirmed:** yes (the replayed `STEPInversion` left its outputs unset).
- **Severity:** crash/UB (uninitialised read, null dereference).
- **Rust port:** A failed iso circle is `None`; the generators' intersection returns `SM_ERR_NULL_POINTER` where upstream dereferences it.

#### UB-94. `SmSurfOfExtrusion` intersections dereference the optional 3D curve array
- **Location:** `source/SMLib/src/SmSurfOfExtrusion.cpp:2506` (`IntersectWithPlane`), `:3272` (`IntersectWithCylinder`), `:3400` (`IntersectWithSurfOfExtrusion`).
- **What happens:** `pOpt3DCurves` is documented as "NULL to ignore", but the closing loops call `pOpt3DCurves->GetSize()` unconditionally (and `sm_IntersectGenerators` does the same for range solutions).
- **Trigger:** Any of the three with `pOpt3DCurves = NULL` that reaches its closing loop (e.g. `IntersectWithPlane` of a plane parallel to the extrusion).
- **How found:** read during porting (`surface.surf_of_extrusion_p2`).
- **Confirmed:** no. The shim always passes every output array.
- **Severity:** crash/UB (null dereference).
- **Rust port:** Always produces every output.

#### UB-95. `SmSurfOfExtrusion::IntersectWithPlane` compares with an uninitialised first point
- **Location:** `source/SMLib/src/SmSurfOfExtrusion.cpp:2348` (`sFirstCrvPt`), `:2454` (`IntersectWithPlane`).
- **What happens:** `sFirstCrvPt` is only set when the first plane intersection solution makes a curve. When it does not (no directrix, or the directrix misses the plane domain), a later solution on a closed generator compares its point with the uninitialised `sFirstCrvPt`, and may then `RemoveAt(0)` from an empty curve array.
- **Trigger:** A plane parallel to the extrusion of a closed generator, bounded so that the directrix at the first hit misses the plane domain while a later one hits it.
- **How found:** read during porting (`surface.surf_of_extrusion_p2`).
- **Confirmed:** no.
- **Severity:** crash/UB (uninitialised read).
- **Rust port:** The comparison is false without a first point.

#### UB-96. `SmTLSEdgeVertex::SolveIt` treats its vertex operand as a curve
- **Location:** `source/SMLib/src/SmTopologySolver.cpp:1014`-`1037`, specifically the `pCurve` assignments at `:1032`-`:1033`.
- **What happens:** After selecting the vertex from pObj2 (normal order) or pObj1 (swapped), the method casts that SAME object to SmCurve and calls GetEdge. A SmVertex is not a SmCurve, so this is an invalid downcast/member access, regardless of the caller's otherwise correct curve/vertex argument order.
- **Trigger:** `SmTLSEdgeVertex::SolveIt` with a real edge curve and vertex, in either argument order.
- **How found:** read during porting `intersect.topology_solver_p2` (PR #60).
- **Confirmed:** no. SolveIt is deliberately not executed. The C++ reference runtime-type probe `sc_intersect_topology_solver_p2_edge_vertex_operand` constructs a real shell vertex and confirms `IsKindOf(SmCurve_TYPE)` is false. The undefined SolveIt cast itself is deliberately NOT executed; no particular crash or numeric outcome is claimed.
- **Severity:** undefined behavior.
- **Rust port:** validates the selected operand's class and returns InvalidInput. Separate native class-probe and Rust regressions cover both orders; there is no fabricated native solver answer.

#### UB-98. `SmCopyBrepMap::SetAt` / `GetAt` release `SM_ASSERT`s dereference null objects
- **Location:** `source/SMLib/src/SmCopyBrepMap.cpp:21` `SetAt`, `:48` `GetAt`; the release `SM_ASSERT(a)` is `if(a) {}` (`source/SMLib/inc/SmMessages.h:714`), so its expression is still evaluated.
- **What happens:** `SetAt` evaluates `pCopyObj->GetType() == pOrigObj->GetType()`, a virtual call through a null pointer when either object is null. `GetAt` does the same with the original it looks up, which is null for an object the map has no entry for.
- **Trigger:** `SetAt(NULL, p)`, `SetAt(p, NULL)`, or `GetAt(p)` for a `p` never passed to `SetAt`.
- **How found:** crash in the C++ reference run (`topology.proto_topology_p1` list scripts).
- **Confirmed:** yes (segfault in `SmCopyBrepMap::SetAt`).
- **Severity:** crash/UB.
- **Rust port:** `CopyBrepMap::set_at` takes non-null objects; `get_at` of an object without an entry is `InvalidInput`. The shim refuses both.

#### UB-100. `SmPolyBrep::CalculateBoundingBox` dereferences the null point array when asked for a pseudo box
- **Location:** `source/SMLib/src/SmPoly.cpp:2734` (`SmPointSet3d sPointSet(*pOptPolyPoints, FALSE)`), `SmPolyBrep::CalculateBoundingBox`.
- **What happens:** In the branch without `pOptPolyPoints`, a requested pseudo box fills a local `sPolyPoints` and then builds the point set from `*pOptPolyPoints`, which is `NULL` in that branch.
- **Trigger:** `CalculateBoundingBox(box, &pseudoBox)` (no point array).
- **How found:** read during porting (`query.poly`).
- **Confirmed:** no (the FFI wrapper rejects the combination).
- **Severity:** crash/UB (null dereference).
- **Rust port:** `PolyBrep::calculate_bounding_box` returns `InvalidInput`.

#### UB-101. `SmPolyBrep::GetPolyEdges` with a common edge use reads loops as edges
- **Location:** `source/SMLib/src/SmPoly.cpp:6505` (`(SmPolyEdge*) pPolyFace->GetList()`), `SmPolyBrep::GetPolyEdges`.
- **What happens:** The filtered branch walks each face's `m_pList`, which holds the face's loops, casts them to `SmPolyEdge*`, reads `GetOriginalEdgeuse()` at the edge member offset of an `SmPolyLoop`, and would add loop pointers to the edge array.
- **Trigger:** `GetPolyEdges(edges, pEdgeuse)` with a non-null edge use.
- **How found:** read during porting (`query.poly`).
- **Confirmed:** no (the harness never passes an edge use).
- **Severity:** crash/UB (type confusion, out-of-object read).
- **Rust port:** `PolyBrep::poly_edges(Some(..))` returns `InvalidInput`.

#### UB-102. `SmSrfInVolume::Evaluate` with orders (0,0) and `bOnlyUpperHalf = FALSE` writes `Duv` past the output array
- **Location:** `source/SMLib/src/SmSrfInVolume.cpp:598`-`601` (the `lTempNumDerivsToEval = 2` branch), `:~720` onward (the `Duv` store), `SmSrfInVolume::Evaluate`.
- **What happens:** With `lHighestUDeriv == lHighestVDeriv <= 1` and `bOnlyUpperHalf == FALSE` the second-derivative branch is taken, and `Duv` is stored at an index one past the single entry a (0,0) request sizes the caller's array to.
- **Trigger:** `Evaluate(uv, 0, 0, D, FALSE)` with a one-entry `D`.
- **How found:** crash in the C++ reference run (the shim's exit 139 during `surface.srf_in_volume` difftest development).
- **Confirmed:** yes (segfault in the shim; the shim now sizes the array to avoid it).
- **Severity:** crash/UB (out-of-bounds write).
- **Rust port:** Skips the out-of-range `Duv` write and returns success with `D`; documented and tested.

#### UB-103. `SmSrfInVolume::STEPInversion` indexes an empty point array when the volume inverse succeeds with no points
- **Location:** `source/SMLib/src/SmSrfInVolume.cpp:927`-`928` (`sParamPoints[0]`), `SmSrfInVolume::STEPInversion`.
- **What happens:** After `InvEvaluatePoint` / `InvMapPoint` reports `bSuccess == TRUE` the code passes `sParamPoints[0]` to the base surface without checking that the array holds a point. A volume inverse that succeeds with no points leaves it empty, and the reference passes a null reference (segfault in `SmSurfacePointSolver`).
- **Trigger:** `STEPInversion` of a point the volume's inverse accepts but returns no point for (found by the generated `surface.srf_in_volume` cases once non-transform maps were allowed).
- **How found:** crash in the C++ reference run (difftest case `gen1653`).
- **Confirmed:** yes (segfault, exit 139).
- **Severity:** crash/UB (null dereference).
- **Rust port:** returns `SM_ERR` when there is no first point; the shim probes the same volume call and reports `SM_ERR` instead of calling `STEPInversion`.

#### UB-104. Gap records and gap functions dereference null or index before their arrays
- **Location:** `source/SMLib/inc/SmGap.h:371` (`SmVertexEdgeGap::Set`), `source/SMLib/src/SmGap.cpp:134` (`SmGap::GetGapEndPoints`), `:1890`-`1895` and `:1773` (`SmGapFunction::BuildSampleSet`).
- **What happens:** `SmVertexEdgeGap::Set` (and its constructor) caches the gap whenever the parameter is defined, also with a NULL vertex use; `GetGapEndPoints` then calls `GetCurve()` on the NULL edge (`SM_ASSERT_MSG` is compiled out). The same happens for an edge parameter whose edge has no curve, a face UV point whose face has no surface, and `SmVertexEdgeGap::GetEdge` of a vertex use without edge use. `BuildSampleSet` with a sample count of `0` writes `sSampleNormalizedVals[m_lSampleCntU-1]` (index `ULONG_MAX`), or for B-spline geometry computes `(0-1)/n` counts; a `v` count of `0` on a surface writes past the array and returns a pointer into an empty sample array.
- **Trigger:** `SmVertexEdgeGap(NULL, 0.5)`; `SmSrfPtGapFunction(..., 0)` then `GetMaxGapSample`.
- **How found:** read during porting (`heal.gap`).
- **Confirmed:** no (the harness predicts and avoids the null dereferences, and keeps counts at least 1).
- **Severity:** crash/UB (needs a bad argument from the calling code).
- **Rust port:** `heal::gap` returns `InvalidInput`.

#### UB-155. Volume discontinuity W planes index the V parameter array
- **Location:** `source/SMLib/src/SmCurveClass.cpp:10071-10072`, `SmCurveClassification::Intersect3DDiscontinuities` (volume).
- **What happens:** The W-plane loop takes its coordinates from `m_sParamsV[kk]` while iterating `m_sParamsW`. Different V/W values put the planes at the wrong W positions; more W than V parameters reads past V's array.
- **Trigger:** unequal V/W discontinuity parameter arrays.
- **How found:** read during porting `intersect.curve_class_p8` (PR #152).
- **Confirmed:** no; native fixtures use equal V/W lists and never execute the out-of-bounds case.
- **Severity:** crash/UB; wrong result for in-bounds unequal values.
- **Rust port:** retains V coordinates, with checked indexing returning an error. The plane-intersection path remains dependency-deferred.

### Topology

#### UB-105. `SmVertex::GetMaxUpDimGap3d` adds to a NULL gap array
- **Location:** `source/SMLib/src/SmVertex.cpp:2978` `SmVertex::GetMaxUpDimGap3d` (`pOptGapArray->Add(...)` for both gap kinds); reached by `GetMaxGap3d()` with its default arguments.
- **What happens:** The gap array is optional (default `NULL`) and is reset only when given, but every gap longer than `pOptTol3d` (all gaps without `pOptTol3d`) is added through `pOptGapArray->Add` without a check: a call without an array dereferences `NULL` as soon as one vertex use has a vertex/edge or vertex/face gap.
- **Trigger:** `pVertex->GetMaxGap3d()` (or `GetMaxUpDimGap3d()`) on any vertex of a wire edge with a curve or of a face with a surface.
- **How found:** read during porting (unit `topology.vertex`).
- **Confirmed:** no (the difftest shim always passes an array and reports the null dereference when none was asked for and a gap was added).
- **Severity:** crash.
- **Rust port:** `topology::vertex::max_up_dim_gap_3d` returns `SolidError::InvalidInput` when a gap is to be listed without a list.

#### UB-106. `SmVertex::AreEdgesTangent` / `IsTopologicalVertex` read an uninitialised tangent
- **Location:** `source/SMLib/src/SmVertex.cpp:70` `AreEdgesTangent` (all four `SE(GetEndTangent(...))`), `:716` `IsTopologicalVertex` (the `SE(GetEndTangent(FALSE, ...))` branches).
- **What happens:** `SmVector3d()` leaves its members uninitialised; a failed `SmEdge::GetEndTangent` (`SER` on the curve evaluation) leaves the tangent unset, its status is ignored and the tangent is negated and passed to `AngleBetween`. An edge without curve makes `GetEndTangent` dereference `NULL`.
- **Trigger:** an edge whose curve cannot be evaluated at its interval end, or a curveless edge at the vertex (in `IsTopologicalVertex` when the other edge has a curve).
- **How found:** read during porting (unit `topology.vertex`).
- **Confirmed:** no (the difftest shim checks the evaluations first and does not call these).
- **Severity:** crash/UB.
- **Rust port:** `SolidError::InvalidInput`.

#### UB-130. `sm_CheckEdgeOnAxis` reads `sSolutions[1]` with one solution
- **Location:** `source/SMLib/src/SmRotationalSweepGeometry.cpp:224`-`226` (`sm_CheckEdgeOnAxis`, file static), reached by `SmRotationalSweepGeometry::EdgeSweepHigher` and `EdgeSweepSame`.
- **What happens:** For a curved edge with both ends on the rotation axis the curve/axis intersection is tested with `nSolutions == 0 || nSolutions > 1 || sSolutions[1].m_eSolutionType == SM_ST_SINGLE_VALUE`. With exactly one solution `sSolutions[1]` is past the array's size (the release `operator[]` does not check). It lies inside the allocation (`SM_SOLUTION_ARRAY_MIN_SIZE` is 4, `smos_Calloc`), so it reads a zero-filled slot (`SM_ST_NODES_ONLY`) or a solution left behind by an earlier `RemoveAt` of the intersector. The single solution is then treated as a coincident range (index `0` was clearly meant).
- **Trigger:** a curved (not line-like) edge with both end vertices on the axis whose intersection with the axis is one solution (a curve partly running along the axis, a closed curve touching it at its seam).
- **How found:** read during porting (unit `sweep.rotational_sweep_geometry`).
- **Confirmed:** in part: the C++ build and the port agree on every such fixture (`nurb_zigzag_on_axis*`, `circle_touching_axis*`), consistent with a zero-filled slot there; a stale slot cannot be excluded in general.
- **Severity:** UB (out-of-range read; wrong result if the slot is stale).
- **Rust port:** reads a zero-filled slot (`SM_ST_NODES_ONLY`) deterministically (`sweep::rotational_sweep_geometry::check_edge_on_axis`).

#### UB-119. `SmTrimmingTools::FindOuterLoops` reads past its output arrays for a zero-area loop
- **Location:** `source/SMLib/src/SmTrimmingTools.cpp:2275` (`if(sLoopAreas[ii] <= 0.0) continue;`), `:2426`-`2427` (`rsInLoopCounts[0] = sOutLoops[0]`, `sOutLoops[ii] - sOutLoops[ii-1]`).
- **What happens:** A loop whose tessellation encloses zero area has `sLoopAreas[ii] == 0` and is skipped as if already processed when the next outer loop is picked (`<= 0.0`). The inner-loop pass skips only `< 0`, so a zero-area loop inside an outer loop is still classified as its inner loop (no UB); one that no outer loop contains is never classified, so `sOutLoops` gets fewer entries than there are loops (none at all when every loop has zero area). The translation back to counts still reads `sOutLoops[0 .. lNumLoops-1]`, past its size (inside its reserved capacity: uninitialised values), and writes them to `rsInLoopCounts`; the curves of the dropped loop vanish from `rsInCurves`.
- **Trigger:** `FindOuterLoops` on a loop of collinear curves (e.g. `(0,0)-(2,0)`, `(2,0)-(1,0)`, `(1,0)-(0,0)`), alone or beside (not inside) a real loop.
- **How found:** adversarial / edge-case fixture `outer_zero_area_loop` (unit `topology.trimming_tools`).
- **Confirmed:** no (the shim sees the dropped curves and reports the call instead of the garbage counts).
- **Severity:** UB (uninitialised read).
- **Rust port:** `topology::trimming_tools::find_outer_loops` returns `SolidError::InvalidInput`.

#### UB-120. `SmTrimmingTools::OrderCurvesIntoLoops` mishandles a NULL curve
- **Location:** `source/SMLib/src/SmTrimmingTools.cpp:1800` (`if (pC == NULL) continue;`), `:1890` (`sVertexPoints[2*nCurveIndex]`), `:1871` (`if(pC == NULL) break;`), `:2078` (`pC->IsOnPlane(...)`).
- **What happens:** A NULL entry adds no end points, so every later curve reads the end points of the curve after it (and the last ones read past the array); in the loop search a NULL curve that is the first unseen one ends the search early; the planarity test finally dereferences it.
- **Trigger:** `crCurves` with a NULL entry.
- **How found:** read during porting (unit `topology.trimming_tools`).
- **Confirmed:** no (the difftest wrapper never passes NULL curves).
- **Severity:** crash.
- **Rust port:** takes a slice of curves (no NULL entries).
#### UB-157. `CreateBlendSurface` with rails of different natural intervals writes past `sIntParams`
- **Location:** `SmBSplineSurfaceNLib.cpp:5305`-`5378` (`SmBSplineSurface::CreateSkinnedSurface`, reads `sIntParams[lNumSections+ii]` at `:5366` and writes it at `:5378`); reached from `SmSurface::CreateBlendSurface` (`SmSurface.cpp:15375`, via `SmDerivSurfDefinition::SkinDerivSurface`, `:3503`), which passes only one parameter per cross section (`sParams`, 101 values). `CreateBlendSurface` also ignores the statuses of both `GetSampleValues` calls (`SmSurface.cpp:15443`, `:15452`) and goes on with partly filled sample arrays.
- **What happens:** With two rails, `CreateSkinnedSurface` expects `2 * lNumSections` intersection parameters. When `SyncronizeKnotsOfCurves` changes the natural interval of rail 2, the loop reads and writes past the end of the `SmTArray` copy: heap corruption.
- **Trigger:** `CreateBlendSurface` with two B-spline curves whose natural intervals differ (e.g. `[0, 1]` and `[0, 2]` knot vectors) on surfaces the curves can be dropped onto.
- **How found:** crash in the C++ reference run (`surface.surface_p8`, generated blend cases: `corrupted size vs. prev_size` in `N_EndNurbs` under `CreateSkinnedSurface`).
- **Confirmed:** yes (heap corruption without the shim's probe; with it, 2000 cases run clean).
- **Severity:** crash/UB.
- **Rust port:** The sample statuses fail the call at the first failing sample (`SM_ERR`). Skinning with too few intersection parameters returns `InvalidInput` ("too few intersection parameters"). The shim probes both conditions (the `GetSampleValues` pair and the rail synchronisation on copies) and reports `SM_ERR` / `1007` instead of running the undefined part.

#### UB-111. `SmApiCreateDraftSweep` with no curves crashes
- **Location:** `source/SM_API/src/SmApiBrep.cpp:1310` `SmApiCreateDraftSweep` -> `source/SMLib/src/SmPrimitiveCreation.cpp:3604` `SmPrimitiveCreation::CreateTaperExtrude`.
- **What happens:** Unlike `SmApiCreateTaperExtrude` (`:1640`), the draft-sweep wrapper does not reject an empty curve array; `CreateTaperExtrude` has no check for it either and segfaults.
- **Trigger:** `SmApiCreateDraftSweep` with an empty `rCurvesToSweep` (height `1`, angle `5`, caps `3`, either corner type).
- **How found:** adversarial / edge-case fixture (`sweep.api_brep` probe case).
- **Confirmed:** yes.
- **Severity:** crash.
- **Rust port:** `create_draft_sweep` copies the (empty) list as upstream and is a stub until `primitive.primitive_creation_p2` lands. The FFI rejects an empty draft-sweep profile.

#### UB-112. `SmPrimitiveCreation::CreateTaperExtrude` crashes on a profile curve below about twice the zone tolerance
- **Location:** `source/SMLib/src/SmPrimitiveCreation.cpp:3604` `SmPrimitiveCreation::CreateTaperExtrude` (chains the curves with `SmCurve::FixChainCurvesList(.., 2*dZoneTol, TRUE)` at `:3672`, then uses them unchecked); reached from `SmApiCreateDraftSweep` and `SmApiCreateTaperExtrude`.
- **What happens:** A profile made of a short curve segfaults. Which dereference fails was not located (the chaining step may drop the short curve and leave `NULL` entries that later code dereferences).
- **Trigger:** `SmApiCreateDraftSweep` / `SmApiCreateTaperExtrude` (height `1`, angle `7.04`, caps `3`) with one `SmLine` of length `1e-5` or `1.2e-5` (length `2e-5` works), a four-line square of side `1e-6`, or an `SmApiCreateCircle` circle of radius `1.72e-6` with height `7.05e-6` (radius `2e-6` with height `1` works).
- **How found:** generated difftest case (`sweep.api_brep`), then adversarial probes.
- **Confirmed:** yes.
- **Severity:** crash.
- **Rust port:** The wrappers are stubs until `primitive.primitive_creation_p2` lands. The FFI rejects draft / taper profiles with a curve extent below `2e-5`; the generator keeps those profiles at scale `>= 1e-3`.

#### UB-113. `SmPrimitiveCreation::CreateSweepAlongPlanarPath` crashes on a curve that is not an `SmBSplineCurve`
- **Location:** `source/SMLib/src/SmPrimitiveCreation.cpp:5698` `SmPrimitiveCreation::CreateSweepAlongPlanarPath` (static `PositionProfileCurves` uses the result of `rProfileCurves[ii]->Copy(..)` without checking its status; the path curves are C-cast to `SmBSplineCurve*`); reached from `SmApiCreateSweepAlongPlanarPath` (`source/SM_API/src/SmApiBrep.cpp:1420`), which only checks the list sizes and the cap selector.
- **What happens:** A profile curve without `Copy` (base `SmCurve::Copy` returns `SM_ERR` and a `NULL` copy) is dereferenced; a path curve of another kind is used as an `SmBSplineCurve`. Both segfault.
- **Trigger:** `SmApiCreateSweepAlongPlanarPath` with profile `[SmHermiteCurve((0,0,0),(1,0,0),(1,1,0),(1,0,0))]` and path `[SmApiCreateLineSegment((0,0,0),(0,0,10))]` (move `TRUE`, caps `3`); or profile `[SmApiCreateCircle((0,0,0), 5)]` with that Hermite curve as the path.
- **How found:** adversarial review of `sweep.api_brep` (PR #110).
- **Confirmed:** yes (segfault in `CreateSweepAlongPlanarPath` / `SmBSplineCurve` copy constructor under gdb).
- **Severity:** crash.
- **Rust port:** `create_sweep_along_planar_path` is a stub until `primitive.primitive_creation_p2` lands, whose porter must return an error instead. The FFI rejects a Hermite curve in either `PlanarPath` list.

#### UB-114. `SmPrimitiveCreation::CreateCurveSweep` crashes on a profile without a B-spline curve
- **Location:** `source/SMLib/src/SmPrimitiveCreation.cpp:4882` `SmPrimitiveCreation::CreateCurveSweep`: profile entries that are not `SmBSplineCurve` (including `NULL`) are skipped, then `sNurbs[0]` is read at `:5054` (profile reference point when `bMoveProfileCenterToPath` is false) without checking that `sNurbs` is non-empty; reached from `SmApiCreateCurveSweep` (`source/SM_API/src/SmApiBrep.cpp:1475`), which only checks that the profile list is non-empty.
- **What happens:** Segfault.
- **Trigger:** `SmApiCreateCurveSweep` with profile `[NULL]` or `[SmHermiteCurve(..)]`, path `SmApiCreateCurve((0,0,0),(0,0,3),(0,0,7),(0,0,10))`, no scale curves and `NULL` options (or options with `bMoveProfileCenterToPath = FALSE`). With `bMoveProfileCenterToPath = TRUE` the same profiles return without crashing (all 32 combinations of the other flags and scaling probed).
- **How found:** adversarial review of `sweep.api_brep` (PR #110).
- **Confirmed:** yes (segfault in `CreateCurveSweep` under gdb).
- **Severity:** crash.
- **Rust port:** `create_curve_sweep` is a stub until `primitive.primitive_creation_p3` lands, whose porter must return an error instead. The FFI rejects a curve-sweep profile without a B-spline curve unless the options move the profile center to the path.

#### UB-108. Public topology queries dereference edges whose curve was removed
- **Location:** `source/SM_API/src/SmApiQueries.cpp` (`SmApiGetClosestPoint`, `SmApiBrepClosestPoint`, `SmApiBrepClosestPointAll`, `SmApiBrepRelationship`, `SmApiBrepDistance`, `SmApiFindEdge`, `SmApiFindFaces`, `SmApiEdgeClosestPoint`, `SmApiFaceInternalPoint`, `SmApiFaceClassifyUV`, `SmApiFaceClosestPoint`, `SmApiBrepBoundingBox`, `SmApiFaceBoundingBox`) and their topology-query callees.
- **Trigger:** Construct a line/B-spline wire or a face-bearing B-rep, then clear an edge's geometry with `SetCurve(nullptr, TRUE, FALSE, FALSE)`. Solver/picker paths dereference the missing curve. Face closest point crashes on a box; interior-point/classification paths crash on vertex-loop/holed faces when they reach that boundary. Tight B-rep/face boxes also crash on a curved quadratic patch with a missing boundary curve.
- **How found:** PR #125 review, followed by 220 isolated subprocess probes of line/B-spline wires, boxes, vertex-loop and holed faces with both box modes, extended to 308 inputs to include planar and curved patches.
- **Confirmed:** yes; affected native processes terminated with SIGSEGV. After the FFI guards were expanded, 396 isolated probes completed without a crash or timeout (192 guarded, 204 native calls completed). Regression tests preserve defined missing-curve edge queries and null top-level inputs.
- **Severity:** crash.

#### UB-109. `SmCurveClassification::FindInterval` / `InsertPointClass` / `sm_FixProblemsBySqueezing` read the first interval of an empty classification
- **Location:** `source/SMLib/src/SmCurveClass.cpp:5839` (`FindInterval`: `(*this)[0]` is read before any size check; the next test at `:5899` does check `GetSize() > 0`). Reached from `IsCoincident` (`:6995`, `:7007`), `InsertPointClass` (`:7304`) and, for the other classification, `sm_FixProblemsBySqueezing` (`:3593`, `:3602`).
- **What happens:** with `GetSize() == 0` the first test reads `m_pData[0]` of an array without intervals (null or past the allocation).
- **Trigger:** a classification whose intervals were all removed or never set, searched for a parameter (a curve with no classified interval, or the empty `rCurveClassOther` of a squeeze).
- **How found:** read during porting (unit `intersect.curve_class_p3`).
- **Confirmed:** no (the difftest shim rejects these scripts).
- **Severity:** crash/UB.
- **Rust port:** `SolidError::InvalidInput`.

#### UB-110. `SmCurveClassification::IsCoincident` walks past the intervals when its end index wraps or a search failed
- **Location:** `source/SMLib/src/SmCurveClass.cpp:7016` (`lIndex2--` of an unsigned `ULONG`) and `:7020`-`7022` (`m_pData[ii]` for `ii <= lIndex2`).
- **What happens:** when the end parameter classifies to the start of interval 0 (`ePosition2 == SM_IP_START`, `lIndex2 == 0`) the decrement wraps to the largest `ULONG` and the loop reads `m_pData[ii]` for every `ii` until it finds a mid classification that is not an edge: when all the intervals classify to edges it reads past the array. The same loop runs with `lIndex == 999999` (the value `FindInterval` leaves when it fails; both statuses are ignored) after a failed search.
- **Trigger:** `IsCoincident(rInterval, tol)` with `rInterval.GetMax()` equal (within tolerance) to the start of the curve classification, or with a parameter off either end of the classification that does not snap, and every interval mid classified to an edge.
- **How found:** read during porting (unit `intersect.curve_class_p3`).
- **Confirmed:** no (the difftest shim replays the walk and rejects scripts that leave the intervals).
- **Severity:** crash/UB (out-of-bounds read).
- **Rust port:** `SolidError::InvalidInput` when the walk leaves the intervals; otherwise identical (the wrap is kept as an unbounded end).

#### UB-115. `CreateVertexDefinition` / `CreateProtoVertex` use an unset position and gap
- **Location:** `source/SMLib/src/SmProtoTopology.cpp:3754` `SmProtoTopologyManager::CreateVertexDefinition`, `:3961` `SmProtoTopologyManager::CreateProtoVertex` (the ignored `sm_FindPositionFromPCs` calls; `sm_FindPositionFromPCs` at `:84`).
- **What happens:** Both ignore the status of `sm_FindPositionFromPCs`. On its error returns (neither classification has parameters, an object point cannot be evaluated) the local `SmPoint3d` (`SmVector3d() = default`) and, on the early returns, the deviation `double` are never written, and are then used as the definition's position and gap / the proto vertex's position and tolerance.
- **Trigger:** two classifications without parameters, or one on a region (`FindObjPoint3d` of `SM_PC_REGION` is `SM_ERR`), e.g. `CreateProtoVertex` of a region and a face classification.
- **How found:** read during porting (unit `topology.proto_topology_p2`).
- **Confirmed:** no (the difftest shim replays `sm_FindPositionFromPCs` on copies and refuses the call when it fails).
- **Severity:** UB (uninitialised read).
- **Rust port:** `SolidError::InvalidInput`.

#### UB-116. `SmProtoEdge::AddEdgeDefinition` compares uninitialised tangents
- **Location:** `source/SMLib/src/SmProtoTopology.cpp:3189` and `:3190`, `SmProtoEdge::AddEdgeDefinition(SmEdgeDefinition*)` (def. `:3173`).
- **What happens:** The statuses of both `EvaluateMid(NULL, &tan)` calls are ignored; `SmProtoEdge::Evaluate` / `SmEdgeDefinition::Evaluate` return before writing the tangent when an evaluation fails (and dereference `NULL` for a definition without 3d curve), so the reversal decision reads an uninitialised `SmVector3d`.
- **Trigger:** adding a definition to a non-empty proto edge whose (or whose definitions') curve fails to evaluate at the proto edge's mid parameter, or has no curve.
- **How found:** read during porting (unit `topology.proto_topology_p2`).
- **Confirmed:** no (the difftest shim evaluates both first and refuses the call when either fails).
- **Severity:** UB.
- **Rust port:** `SolidError::InvalidInput`.

#### UB-117. `UpdateEdges` dereferences its optional calling vertex definition
- **Location:** `source/SMLib/src/SmProtoTopology.cpp:5074`, `SmProtoTopologyManager::UpdateEdges` (def. `:4975`).
- **What happens:** `pCallingVD` is documented "Optional ..., NULL to ignore, default:[NULL]" (and compared against at `:5013`), but once an edge was split in two `pCallingVD->GetProtoVertex()` dereferences it unconditionally.
- **Trigger:** `UpdateEdges(iWhich, pOldEdge, {pOldEdge, pNewEdge})` without a calling definition.
- **How found:** read during porting (unit `topology.proto_topology_p2`).
- **Confirmed:** no (the difftest shim refuses it).
- **Severity:** crash.
- **Rust port:** `SolidError::InvalidInput`.

#### UB-118. `SmProtoEdge::Resolve` / `SmProtoVertex::Resolve` dereference null and deleted definitions
- **Location:** `source/SMLib/src/SmProtoTopology.cpp:3440` and `:3454`, `SmProtoEdge::Resolve` (def. `:3391`); `:2338`/`:2356`, `SmProtoVertex::Resolve` (def. `:2278`).
- **What happens:** For a proto edge from a proto vertex to itself whose curve is longer than twice the tolerance with ends farther apart, `m_pStartPV` is dereferenced although both ends may be `NULL`, and `pVD0` (`GetClosestVertexDefinition`, `NULL` for a proto vertex without definitions) is dereferenced. In both resolves a definition listed twice is compared with itself (`CanCombine...` picks the second) and deleted, leaving the other entry dangling for the next comparisons; two deleted edge definitions sharing a curve free it twice (`Destruct`), and a deleted curve still used by a kept definition is used after free.
- **Trigger:** a closed proto edge on a proto vertex without vertex definitions; a duplicated entry in the definitions list (e.g. `CombineProtoEdge` of edges sharing a definition).
- **How found:** read during porting (unit `topology.proto_topology_p2`).
- **Confirmed:** no (the difftest shim replays both loops and refuses those calls).
- **Severity:** crash.
- **Rust port:** `SolidError::InvalidInput` for the null proto vertex and definition; definitions are ids (no double delete).

#### UB-121. `SmSurface::IntersectSurfaceBoundaries` dereferences an optional NULL curve array
- **Location:** `source/SMLib/src/SmSurface.cpp:21903` `SmSurface::IntersectSurfaceBoundaries` (`pOpt3DCurves->Add( pCrvCopy )` in the segment loop).
- **What happens:** `pOpt3DCurves` is declared like the other optional outputs (the header calls it `pOpt...`, the comment "required") and is reset only when non-NULL, but a kept boundary segment is added through it unconditionally: a NULL array is dereferenced.
- **Trigger:** `pOpt3DCurves == NULL` and two coincident surfaces with a boundary piece of the other surface inside this one.
- **How found:** read during porting (unit `surface.surface_p11`).
- **Confirmed:** no (the difftest shim always passes the array).
- **Severity:** crash/UB.
- **Rust port:** `Surface::intersect_surface_boundaries` always returns the curves.

#### UB-125. `sm_InsertSolutions` dereferences a null edge curve while ignoring short tangential ranges
- **Location:** `source/SMLib/src/SmCurveClass.cpp:4713`, `:4734`, within `sm_InsertSolutions` (`:4158`).
- **What happens:** `pEdgeCurve` is set only for an `SmEdge` target. For a bare curve or plane target, a forward range shorter than 5% of the classification interval reaches `SmTol::MapTo1d(..., *pEdgeCurve)` when tangencies are ignored. Ranges converted to single points by the earlier endpoint/arc-length test do not reach this dereference.
- **Trigger:** ignore tangencies; bare curve or plane target; a non-collapsing range with `start <= end` and relative interval length below `0.05`.
- **How found:** source review and FFI guard design during `intersect.curve_class_p5` (PR #141).
- **Confirmed:** no native crash executed; the shim reproduces the preceding native collapse predicates and rejects only ranges that would reach the null dereference.
- **Severity:** crash / undefined behaviour.
- **Rust port:** invalid-input error for the undefined branch; valid collapsed ranges and reversed ranges remain supported.

#### UB-126. `SmCurveClassification::IntersectVertices` dereferences an absent surface owner
- **Location:** `source/SMLib/src/SmCurveClass.cpp:14692`, `pOwner->IsKindOf(SmFace_TYPE)`.
- **What happens:** when the classification has no face, the method dereferences `crSurface.GetOwner()` without checking for null, even for an empty vertex list. Its large-gap branch also dereferences the vertexuse returned by `GetVertexuseOfFace` without checking it.
- **Trigger:** a face-less classification and an unowned surface; or large-gap processing of a vertex without a vertexuse on the selected face.
- **How found:** source review and FFI guard design during `intersect.curve_class_p5` (PR #141).
- **Confirmed:** no native crash executed. The oracle supplies a real face owner and excludes nonempty large-gap tests of its standalone vertices.
- **Severity:** crash / undefined behaviour.
- **Rust port:** continues with no face when the surface has no owner, instead of dereferencing a null owner. Unresolved vertex/face callback data returns `InvalidInput` instead of dereferencing a missing vertexuse; a missing face skips the gap lookup and retains the default intersection tolerance.

#### UB-134. `ExtractGeometry` slices a lazy analytic UV curve into an uninitialized B-spline
- **Location:** `source/SMLib/src/SmCurveClass.cpp:13524`-`:13534`, the `SmBSplineCurve` copies of the two UV curves and their `Trim` calls.
- **What happens:** An `SmLine` is a B-spline subclass but can have no materialized NURB. The explicit base copy keeps that null NURB; trimming the sliced curve calls `SmBSplineCurve::Evaluate` through `SmTol::MapTo1d` and dereferences it.
- **Trigger:** a common classified interval and a requested UV output whose valid `SmLine` has not had `MakeNurb` called. Reproduced with a 3D circle of radius 1e6, five face-classified intervals over `[0,1]`, and both UV line outputs.
- **How found:** native differential generation for `intersect.curve_class_p5`, then debugger tracing of the failing input and stack on Linux x86_64 / GCC 15.2.0.
- **Confirmed:** yes: SIGSEGV in `SmBSplineCurve::Evaluate`, called by `SmTol::MapTo1d`, `SmBSplineCurve::Trim` and `SmCurveClassification::ExtractGeometry`. The oracle now materializes both UV line NURBs before calling the upstream method; the fixed regression retains the input.
- **Severity:** crash / undefined behaviour.
- **Rust port:** materializes a line's B-spline base before slicing it, returning an error if that fails.

#### UB-122. `SmCurveClassification::MergeInterval` dereferences an unresolved start vertex
- **Location:** `source/SMLib/src/SmCurveClass.cpp:17300-17347` (UNKNOWN start recovery), `:17650-17679` (unchecked `pStartV`).
- **What happens:** An UNKNOWN start remains without an object when the interior is EDGE or VERTEX, or when projection onto a FACE interior fails. Full nondegenerate merging still casts that object to `SmVertex*` and dereferences it while checking its point or obtaining its B-rep.
- **Trigger:** Unit line, interval `[0,1]`, classifications UNKNOWN/EDGE/UNKNOWN, full merge and either fillet flag. A stored start point `(10,10,10)` with UNKNOWN/FACE/UNKNOWN on the fixture's unit box also fails to project and leaves the start unresolved.
- **How found:** PR #138 port audit: 6,860 isolated merge probes found 64 native SIGSEGVs; follow-up stored-point probes exposed failed FACE projection. Regression tests cover both causes, successful FACE projection, vertices-only merging and constant-curve early returns.
- **Confirmed:** yes, native subprocesses terminated with SIGSEGV before the oracle preflight was added.
- **Severity:** crash.
- **Rust port:** Topology mutation remains explicitly deferred at p2. The native oracle rejects unresolved-start full merges, preserving degenerate and vertices-only paths; rejected fixtures are not differential passes.

#### UB-131. `SmFaceProps::SetProps_Gaps` returns early, so the "face changed: re-run Gaps" path of `SetProps_Stage2` / `Stage3` cannot work
- **Location:** `source/SMLib/src/SmFaceProps.cpp:768` `SetProps_Gaps` (`if(m_ePropStage >= SM_PROPSTAGE_GAPSa) return(SM_SUCCESS)` before `m_pFace = cpFace`), `:875`-`:878` `SetProps_Stage2` (`if(m_ePropStage < SM_PROPSTAGE_GAPS || m_pFace != cpFace) SER(SetProps_Gaps(...))`, then `:898` `m_pFace->GetSurface()`), `:1258`-`:1261` `SetProps_Stage3` (the same test around `SetProps_Stage2`); reached through the public `SmFaceProps::SetProps` / `SetProps_Stage2` / `SetProps_Stage3`.
- **What happens:** Stage 2 and 3 call `SetProps_Gaps` (and `SetProps_Stage2`) again when the props belong to another face (`m_pFace != cpFace`), to recompute them for `cpFace`. But those functions return at once once the stage is at or past `GAPSa` (`2a`), and that stage is exactly what the props are in when the face differs, so `m_pFace` never changes. With `m_pFace == NULL` (a `ReSet` past the gaps clears the stage only to `GAPS`/`2`, a stage seeded without a face) the next line dereferences it: a crash. With `m_pFace` set to another face the stage computes the surface, poles, loops and tolerance of the old face and stores them as the props of `cpFace`.
- **Trigger:** `SmFaceProps p; p.ReSet(SM_PROPSTAGE_2a); p.SetProps(SM_PROPSTAGE_2, face, 0, SM_HO_FIX_TOLSIZES)` (a stage past `GAPS` and no face); or `p.SetProps_Stage2(face)` on props that were set for another face. No crafted file is needed: only a call sequence of the calling code, so this is not marked [PSIRT].
- **How found:** crash in the C++ reference run (the FFI guard of `heal.face_props` rejected the two scripts a maintainer's probes found, `[[1,3,0,0,0],[0,4,26,0,0]]` and `[[5,0,0,2,0],[6,2,26,0,0]]`).
- **Confirmed:** yes (segfault of the C++ reference in those two scripts; they are now regression fixtures of the guard, `fixtures/heal.face_props/upstream.json` cases `rejected_*`).
- **Severity:** crash/UB (needs a bad call sequence from the calling code); wrong result when the face differs.
- **Cause:** the early return of `SetProps_Gaps` was written for "already done or being done" and also swallows the "recompute for a new face" request.
- **Rust port:** `heal::face_props::FaceProps` keeps upstream's early return (same results where upstream is defined); a stage past the gaps without face gives `SolidError::InvalidInput` instead of dereferencing. The `ffi::validate` of the unit simulates the stage and face pointer and rejects the scripts that would crash.

#### UB-128. `SmFace::ClassifyLoops` crashes for a plane with a degenerate parameter domain and edge loops (`SetProps_Stage3`)
- **Location:** `source/SMLib/src/SmFaceProps.cpp` `SetProps_Stage3` -> `SmFace::ClassifyLoops` -> `sms_FindAndClassifyLoopBisector` -> `CurveOnClassify` -> `SmCurveClassification::IntersectVertices` -> `GlobalPointSolve` -> ... -> `SmCrvOnSurf::GetKnots`, which ends in a SIGSEGV in `SmCurveClassification::IntersectUVDiscontinuties`. Reached through the public `SmFaceProps::SetProps(SM_PROPSTAGE_3, ...)` and `SetProps_Stage3`.
- **What happens:** the face's surface is an `SmPlane` whose domain is `[0, 0, t, t]` for a tiny `t`, and the face has edge loops: the discontinuity search of the curve classification dereferences an invalid pointer. A vertex loop, or stopping at `SetProps(2)` (or `SmLoopProps::SetProps`), does not crash.
- **Trigger:** plane domain `[0, 0, t, t]` with an outer edge loop, then `SetProps(SM_PROPSTAGE_3, face, 0, 26)` or `SetProps_Stage3`. The result is not monotone in `t`: the reference crashes at `t` = 1e-300, 1e-100, 1e-14, 1e-12 and 1e-10 and survives at 1e-11 and 1e-9. No crafted file is needed (a bad parameter domain from the calling code), so this is not marked [PSIRT].
- **How found:** crash in the C++ reference run (a maintainer's adversarial probes of `heal.face_props`).
- **Confirmed:** yes (segfault of the C++ reference; regression fixtures `rejected_degenerate_plane_stage3_*` of `fixtures/heal.face_props/upstream.json`, which the guard rejects without calling the C++).
- **Severity:** crash/UB (needs a degenerate surface domain from the calling code).
- **Cause:** not analysed beyond the backtrace; the discontinuity search of the surface-curve knots is not prepared for a (nearly) zero-size parameter domain.
- **Rust port:** `FaceProps::set_props_stage3` reaches `SmFace::ClassifyLoops` only through `UNPORTED(query.face_p4)`, so it returns `Unported` there (no crash). The `ffi::validate` guard of `heal.face_props` rejects a script that can reach Stage 3 on a plane with edge loops whose domain side is below 1e-6 (the bound is deliberately wider than the crashing values, since the crash is not monotone in the size).

#### UB-132. `SmProtoTopologyManager::ResolveOverlappingProtoEdges` reads a deleted proto edge when an edge is listed twice in the manager
- **Location:** `source/SMLib/src/SmProtoTopology.cpp:7032`-`7035` (`ResolveOverlappingProtoEdges`, the `rPEs[ii]` loop whose index repeats after a group was removed) with `source/SMLib/inc/SmProtoTopology.h:798` (`AddProtoEdge` does not de-duplicate) and `SmProtoTopology.cpp:4346` (`RemoveProtoEdge` removes the first copy only).
- **What happens:** Removing an overlap group deletes its proto edges and `RemoveProtoEdge` drops only the first entry of each from `m_sPEs`; a second entry of the same pointer stays in the list, so the repeated-index loop calls `GetOverlappingPEGroup` on the deleted edge (use after free).
- **Trigger:** two proto edges overlapping each other symmetrically (0 and 1) where one of them was added to the manager twice (`AddProtoEdge` repeated), then `ResolveOverlappingProtoEdges`. The C++ reference run exits with signal 11.
- **How found:** adversarial review of the difftest guard (the maintainer's crashing script).
- **Confirmed:** yes (the script was run on the C++ reference; the case `edge_proto_p3_rejected_duplicate_manager_entry_0`/`_1` now refuses it on both sides).
- **Severity:** crash/UB.
- **Rust port:** proto edges are arena ids that are never freed, so the repeat reads a deleted-edge slot and finds no group; nothing is read after free.

#### UB-137. `SmFace::SplitAtSeam` recurses on itself with another face's seam classifications
- **Location:** `source/SMLib/src/SmFace.cpp:18098-18149` `SmFace::SplitAtSeam` (the `bMergeU && bMergeV` branch); the crash is in `SmCurveClassification::MergeInterval` (`MergeIntervals`).
- **What happens:** After merging the U seam, the loop over the faces the merge made computes `pTgtFace->HasSeamProblem(sThisCrvClassU, sThisCrvClassV, ..)` but then calls `SplitAtSeam(sThisCrvClassU, ..)` on `this`, not on `pTgtFace`. `MergeIntervals` then splits `this` along a classification of another face and dereferences invalid topology.
- **Trigger:** `MoveSeam` with `bOkToSplit` (or `SplitAtSeam`) on a face that is treated as closed (by the face properties' closed flags) or is periodic in either direction, with seam problems to merge in both directions, e.g. `vase_rev.smb` (UV trim curves rebuilt), face 5 of the traversal order, with an `SmFaceProps` giving `m_eBadCrossedSeam = m_eBadMissingSeam = SM_SP_BOTH`, `m_bClosedU = m_bClosedV = TRUE` (difftest case `vase_rev.smb_1_f5_op9` of `topology.face_p1`).
- **How found:** crash in the C++ reference run.
- **Confirmed:** yes (SIGSEGV in `SmCurveClassification::MergeInterval` called from the recursive `SmFace::SplitAtSeam`, gdb backtrace).
- **Severity:** crash.
- **Rust port:** Faithful (recurses on the same face, bounded by `MAX_SPLIT_AT_SEAM_DEPTH`). `MergeIntervals` is wired (`intersect.curve_class_p5`), but its topology edits (`MergeInterval`, `intersect.curve_class_p2`) still defer, so the U merge makes no new face and the branch is not reached yet. The difftest shim keeps the C++ side away from every input that can reach it.

#### UB-138. `SmFace::MoveSeam` uses the joined surface without checking `JoinSurface`
- **Location:** `source/SMLib/src/SmFace.cpp:17539-17607` `SmFace::MoveSeam`.
- **What happens:** `eStat = pRightSurface->JoinSurface(.., pJoinedSurface)` is never checked. When the join fails, `pJoinedSurface` is still `NULL` (first direction) and is dereferenced by `CopyAndAddAnalytics` for an analytic surface or handed to `SmBrep::ReplaceSurface` / `SetSurface`; in the second direction it still points to the first direction's surface, which the face already owns.
- **Trigger:** a seam move whose split halves cannot be joined within the surface's zone tolerance (for example face properties that report a seam problem in a direction the surface is not closed in). Not reproduced: `MoveSeam` without split ran crash-free on the 15 `pt_TestFiles` B-reps of the `topology.face_p1` difftest (PR #148 review).
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash.
- **Rust port:** Returns the join's error.

#### UB-136. Fillet geometry substitution reads a missing partner interval
- **Location:** `source/SMLib/src/SmCurveClass.cpp:16830` (`sm_SetDoFilletingGeometry`), called by `SmCurveClassification::MergeInterval`; unchecked indexing in `source/SMLib/inc/SmCurveClass.h:508`.
- **What happens:** Fillet merging indexes both classifications at the target interval without checking the paired classification's size. An empty partner yields a null read; a shorter partner permits an out-of-range interval read.
- **Trigger:** `a = [0,.5] UNKNOWN/UNKNOWN/UNKNOWN + [.5,1] VERTEX/EDGE/VERTEX`, target index 1, empty `b`, flags 2 or 3. FACE and REGION interiors also reach the same invalid access. A one-interval partner at index 1 is invalid even when it does not crash.
- **How found:** maintainer review of PR #138; the existing matrix paired identical one-interval arrays and used index 0.
- **Confirmed:** yes, the maintainer reproduced SIGSEGV for all five curve kinds in full and vertices-only modes (13 crashing probes in total), plus the one-interval out-of-range read.
- **Severity:** crash / out-of-range read.
- **Rust port:** Kernel topology mutation remains deferred. Both the safe FFI validator and the C++ shim require equally sized partners for op 3 with fillets. Six `rejected_fillets_*` fixtures check rejection; 660 isolated probes independently exercise both guards at first and last indices of 1/2/3/32 intervals. Valid equal-sized pairs and non-fillet calls remain accepted.

### Healer target management

#### UB-163. Uncached healer props removal leaves targets that resize into NULL props holes
- **Location:** `source/SMLib/src/SmHealData.cpp:1332-1333` (`RemoveVertexProps_FromLists` and analogous edge/face helpers), `:1224-1226` (`UpdateTargetLists`); later `Cache_CoinVertices` / `Cache_DegenFaces`.
- **What happens:** removing a props record whose topology pointer has never been cached removes the props but not its target. A later nonempty add list resizes every props array from the target count and initializes only the new-target tail, leaving NULL holes for the unmatched old targets. Native coincidence/degeneracy caching can then dereference invalid state. Even a null add causes that resize. A later `Get*Props_ForTgts` can also index past a props list shortened by an uncached removal.
- **Trigger:** box with all targets, no caches: set done=9, update through 6, remove vertices 0/1 and add vertex 0 twice. Another trigger: remove uncached face 0 and add NULL. With edge/vertex caches, done=12 and update through 9, removing/adding uncached face 0 reaches `Cache_DegenFaces`. A prior `Remove*Props_FromLists` can introduce the same drift.
- **How found:** crash in the C++ reference run during the maintainer's step-33 sequence fuzzing ([PR #164, round 2](https://github.com/tangency-labs/solidcore/pull/164#pullrequestreview-5554396459)); source inspection confirmed the mismatched list sizes and resize boundary.
- **Confirmed:** yes by that reference run: 31 crashing scripts (7 coincidence-cache, 21 harness reads of NULL props, 3 degeneracy-cache). Requires inconsistent caller-controlled healer state; ordinary end-to-end healing was not the trigger.
- **Severity:** crash/UB.

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

#### HANG-11. `SmCone::AdjustSTEPUVDomain` never returns for a reversed sweep interval
- **Location:** `SmCone.cpp:458` `SmCone::AdjustSTEPUVDomain` -> `SmSurfOfRevolution.cpp:300` `SmSurfOfRevolution::AdjustSTEPUVDomain` (`Reparameterize`, then `MakeNurb`).
- **What happens:** The C++ reference run did not finish within 7 minutes. The domain is stored and the NURB rebuilt without checking `start <= end`. Which loop spins was not located.
- **Trigger:** `SmCone::CreateCanonical(O, 1, 2, 3, cone)`, then `AdjustSTEPUVDomain(SmExtent2d(90, 0, 10, 1))`.
- **How found:** adversarial / edge-case fixture (`surface.cone_p1`).
- **Confirmed:** yes.
- **Severity:** hang.
- **Rust port:** `SmSurfOfRevolution::AdjustSTEPUVDomain` is not ported yet (`Unported`); the FFI rejects a reversed sweep.

#### HANG-12. `DropPoint` never returns on a curve reparameterised to a zero-length interval
- **Location:** `SmProtoTopology.cpp:1206` `SmEdgeDefinition::Reparameterize` (calls `SmCurve::EditParameterization` with the new domain, unchecked); the loop is in the curve's local point solve (`SmCurve::LocalPointSolve` -> `SmLocalSolve1d::SolveIt` with the `FindPCExtrema` evaluation), reached from `SmCurve::DropPoint`.
- **What happens:** `Reparameterize(SmExtent1d(a, a))` collapses a line to a point (every evaluation returns the same point). A later `DropPoint` on it, e.g. from `SmEdgeDefinition::IsInteriorPoint`, does not return.
- **Trigger:** An edge definition on a bounded `SmLine`, `Reparameterize(SmExtent1d(-2.4753, -2.4753), FALSE)`, then `IsInteriorPoint` on a definition using the same line (any tolerance).
- **How found:** generated difftest case (`topology.proto_topology_p1`).
- **Confirmed:** yes (the C++ reference run did not finish).
- **Severity:** hang.
- **Rust port:** Same loop (the Rust run of the same script did not finish either; the loop belongs to the curve point-solve units). The shim refuses a zero-length `Reparameterize` domain.

#### HANG-15. Tiny surfaces build a surface cache of millions of nodes
- **Location:** `SmSurfaceCache.cpp:4495` `SmSurfaceCache::SubdivideToTolerances` (stops only at `m_dMinimumSideLengthRatioUV`, `:3932`-`:3949`), through `smsurf_GetSurfaceCache` from `SmSurface::GlobalLineIntersect` / `GlobalCurveIntersect`.
- **What happens:** The cache tolerances are absolute. A noisy cubic B-spline surface about `5e-4` across fails the angle test down to the minimum side ratio in both directions, so every patch splits to about `2^20` leaves.
- **Trigger:** `GlobalCurveIntersect` of a line with a 5 x 6 net of scale `1e-4` (control points jittered by 20 %, z noise `1e-8`), sliver domain `[1.6112222, -0.986, 1.6112223, 1.07]`: 15 s in the C++, 121 s in the port. The same net scaled by `1e3` takes 4 ms.
- **How found:** generated difftest case (`surface.surface_p8`; the generator seemed to hang).
- **Confirmed:** yes (C++ 15.4 s, Rust 121.5 s for the one case).
- **Severity:** hang (minutes per call; bounded by the minimum side ratio).
- **Rust port:** Same bound (`MAX_SUBDIVISION_DEPTH` does not limit the node count). The generator uses scale `1e-3` instead of `1e-4`.

#### HANG-13. `SmApiCreateLinearSweep` of a full circle over a negative distance never returns
- **Location:** `source/SM_API/src/SmApiBrep.cpp:1196` `SmApiCreateLinearSweep` (and `SmApiCreateLinearSweepWithRepetitions`, `:1685`) -> `source/SMLib/src/SmPrimitiveCreation.cpp:1795` `SmPrimitiveCreation::CreateLinearSweep` -> (one repetition) `:1150` `CreateLinearSweep`, which builds the analytic domain `SmExtent2d(0, 0, 1, dSweepDist)` without ordering it.
- **What happens:** The C++ reference run did not finish within 5 minutes. Which loop spins was not located. The same sweep with a positive distance, or with the direction reversed instead of the distance, finishes at once; arcs short of 360 degrees, lines and line rectangles with a negative distance also finish.
- **Trigger:** `SmApiCreateLinearSweep` of one `SmApiCreateCircle` circle (B-spline or `SmCircle`, e.g. radius `5` at the origin) along `(0, 0, 1)` over distance `-10`, capped or not; `SmApiCreateLinearSweepWithRepetitions` with one repetition likewise.
- **How found:** generated difftest case (`sweep.api_brep`), then adversarial probes.
- **Confirmed:** yes (the C++ reference run did not finish).
- **Severity:** hang.
- **Rust port:** The wrappers are stubs until `primitive.primitive_creation_p3` lands. The FFI rejects a negative distance with a full circle (B-spline, analytic, or an arc spanning 360 degrees); the generator sweeps circles over positive distances.

#### HANG-14. `SmCurveClassification::Homogenize` repeats a failed final squeeze forever
- **Location:** `source/SMLib/src/SmCurveClass.cpp:6520-6530` (`Homogenize`), `:3715-3741` (`sm_SqueezeOutSmallestInterval`).
- **What happens:** The two final size-equalization loops ignore the squeeze status. If `ConnectIntervals` fails without removing an interval, the size difference remains and the same loop repeats indefinitely.
- **Trigger:** Unequal classification sizes after the earlier homogenization heuristics, with a smallest interval that `ConnectIntervals` cannot remove.
- **How found:** maintainer review of PR #138 and source inspection.
- **Confirmed:** source path only; no native hang reproduction was run.
- **Severity:** hang.
- **Rust port:** Ordinary squeeze statuses are ignored through `ignore_status`, preserving `Unported` propagation; a documented no-progress check returns an error if the interval count did not decrease.

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
- **Rust port:** Uses ordered maps / insertion order on arena ids. `SmTopologyIntersector`'s mate maps (`GetMappedBrepObjects`, `SmTopologyIntersector.cpp:945`; `GetMappedOtherObjects`, `CoincidenceNotify`) and subset (`GetSubset`) are `BTreeMap`/`BTreeSet`s over the caller's object handles in `boolean::topology_intersector::p1`, so their lists come out in handle order; the `boolean.topology_intersector_p1` shim sorts upstream's lists by export handle before comparing.

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

#### WR-62. `SmTLSPointVertex::SolveIt` ray firing falls through into the 3D signed case
- **Location:** `SmTopologySolver.cpp:206-262`, `SmTLSPointVertex::SolveIt` (def. `:116`).
- **What happens:** The `SM_SO_RAYFIRE` case has no `break`, so the `SM_SO_3D_SIGNED_DIRECTED_MINIMIZE` case runs after it: it recomputes the ray parameter without the rayfire snap to the ray start (`dParam < 0.0 -> 0.0`), overwrites `dDistSq`, and sets the sign from the unsnapped parameter (negated again when the object order is swapped). A vertex just behind the ray start (within `SM_EFF_ZERO` of it) gets a small negative ray distance instead of `0`, and a swapped call returns negated ray distances.
- **Trigger:** `SM_SO_RAYFIRE` point/vertex solve from `(1, 0, 0)` along `(1, 0, 0)` with a vertex at `(1 - 1e-13, 0, 0)` (fixture `point_vertex_rayfire_behind_start`).
- **How found:** read during porting (`intersect.topology_solver_p1`).
- **Confirmed:** no (the C++ runs the fixture, but the Rust side defers it: the solution comparison needs vertex zone tolerances). The review's single-call fixtures `adv_rayfire_behind_start_single_*` are deferred the same way (`add_sorted_solution` reaches the `SmTol::GetZoneTol3d` stub of `topology.brep_p1` for vertex objects).
- **Severity:** wrong result.
- **Rust port:** Ported as is (the fall-through kept).

#### WR-91. `SmVertex::GetVertexuseOfFace` ignores its face
- **Location:** `source/SMLib/src/SmVertex.cpp:225` `SmVertex::GetVertexuseOfFace` (the face test inside the `// Remove Composite` comment, `:258`).
- **What happens:** The test of the vertex use's face against `pFace` was commented out with the composite face code, leaving its block unconditional: for any non-null `pFace` the function returns the first vertex use (of any face) on its face's upward face use, else the last vertex use with a face use, whichever face it belongs to.
- **Trigger:** a vertex on two or more faces.
- **How found:** read during porting (unit `topology.vertex`).
- **Confirmed:** no (the Rust side is deferred on `SmFace::GetUpwardFaceuse`, `topology.face_p2`).
- **Severity:** wrong result.
- **Rust port:** Faithful (`topology::vertex::vertexuse_of_face`).

#### WR-92. `SmVertex::IsTopologicalVertex` reports a failed tangent evaluation and cusps as topological
- **Location:** `source/SMLib/src/SmVertex.cpp:716` `SmVertex::IsTopologicalVertex` (`SER(pEdgeN->GetEndTangent(TRUE, ...))`, and the angle test `fabs(dAngleRad - SM_PI) > dAngLimitRad`).
- **What happens:** As WR-52, `SER` returns the failing `SmStatus` of `GetEndTangent` at an edge's start as the `SmBoolean` result, so the vertex counts as topological. The angle test also accepts edges meeting at `PI` (a cusp, the curves doubling back), which upstream flags itself ("gwc: is this a bug? it counts cusps").
- **Trigger:** two different curves meeting at equal parameters whose start tangent cannot be evaluated; two edges doubling back on each other at the vertex.
- **How found:** read during porting (unit `topology.vertex`); the cusp case is compared by the difftest (`turn180p0_*`, `turn179p51_*` fixtures).
- **Confirmed:** yes for the cusp case (the C++ returns `TRUE`).
- **Severity:** wrong result.
- **Rust port:** Faithful.

#### WR-96. `SmCurveLoops::GetLoopStartEnd` starts a later loop at the previous loop's last curve
- **Location:** `source/SMLib/src/SmTrimmingTools.cpp:4127` (`rlStart = rlEnd;`).
- **What happens:** `rlEnd` is the index of the last curve of the loop just passed, so `rlStart` of the next loop should be `rlEnd + 1`; it is one less. For loops of 3 and 4 curves, curve 4 gets the range `[2, 6]`. `FindAdjacentLoopCurve` then steps from the first curve of a later loop to its predecessor in the previous loop instead of wrapping to the loop's last curve (and a first loop of zero curves wraps `rlEnd` below zero).
- **Trigger:** `GetLoopStartEnd` / `FindAdjacentLoopCurve` / `SampleNearbyPoints` on any loop after the first.
- **How found:** read during porting (unit `topology.trimming_tools`).
- **Confirmed:** yes (fixture `loops_start_end_second_loop`).
- **Severity:** wrong result.
- **Rust port:** Faithful to the code (`CurveLoops::loop_start_end`).

#### WR-97. `SmTrimmingTools::IsIsoCurve(curve, side, low, tol)` returns an error status as `TRUE`
- **Location:** `source/SMLib/src/SmTrimmingTools.cpp:3632` (`SER (cpUVCurve->GetCanonical (...))` in a function returning `SmBoolean`).
- **What happens:** When `GetCanonical` fails its nonzero status is returned as the `SmBoolean`, so a curve whose control points cannot be read counts as an iso curve (and `FixBackwardsOuterLoopOnSeam` then mirrors it).
- **Trigger:** a UV trim curve without NURB.
- **How found:** read during porting (unit `topology.trimming_tools`).
- **Confirmed:** no (the difftest wrapper only passes curves with a NURB).
- **Severity:** wrong result.
- **Rust port:** Faithful to the code (`is_iso_curve_at` returns `true`).

#### WR-98. `SmTrimmingTools::MirrorCurve` divides rational control points by their weights twice
- **Location:** `source/SMLib/src/SmTrimmingTools.cpp:3516`-`3517`.
- **What happens:** `GetCanonical` already returns Euclidean control points; the loop divides each one by its weight again before `SetCanonical`, so a rational UV curve is moved and distorted, not just mirrored.
- **Trigger:** `MirrorCurve` (reached from `FixBackwardsOuterLoopOnSeam`) on a rational UV curve with weights other than 1.
- **How found:** read during porting (unit `topology.trimming_tools`).
- **Confirmed:** yes (fixture `mirror_rational_divides_twice`).
- **Severity:** wrong result.
- **Rust port:** Faithful to the code (`mirror_curve`).
#### WR-95. `SmProtoEdge::CombineProtoEdge` moves only every other edge definition
- **Location:** `source/SMLib/src/SmProtoTopology.cpp:3332`, `SmProtoEdge::CombineProtoEdge` (def. `:3296`).
- **What happens:** The loop `for(ii=0; ii<pOtherPE->m_sEdgeDefs.GetSize(); ii++)` adds `m_sEdgeDefs[ii]` to this proto edge, which (through `AddEdgeDefinition`, the definition's owner being `pOtherPE`) removes it from `pOtherPE`'s list, then advances `ii`: the definition that moved into slot `ii` is skipped. Of `n` definitions only those at even positions move; the rest stay on `pOtherPE`, whose ends were already moved to this proto edge's.
- **Trigger:** `CombineProtoEdge` of a proto edge with two or more definitions.
- **How found:** adversarial / edge-case fixture `edge_pe_combine_every_other_unit` (unit `topology.proto_topology_p2`).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.

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

#### WR-61. `SmSurfOfExtrusion::EvaluateSTEP` / `EvaluateSTEPPoint` clamp a copy they never use
- **Location:** `SmSurfOfExtrusion.cpp:941`-`955` (`EvaluateSTEP`: clamps `sUV`, evaluates `crUV.x`, `crUV.y`); `:1015`-`1024` (`EvaluateSTEPPoint`, same).
- **What happens:** "Per SMLib practice clamp input point to surface domain" clamps a local copy, but both evaluate the caller's unclamped parameters. With out-of-bounds evaluation disabled, a v outside the STEP domain still extrapolates along the extrusion, and an analytic generator is evaluated outside its domain.
- **Trigger:** `EvaluateSTEPPoint((u, vmax + 1))` with out-of-bounds evaluation off.
- **How found:** read during porting; generated difftest cases evaluate outside the domain on both sides.
- **Confirmed:** yes (`surface.surf_of_extrusion_p1` generated `eval_step` cases outside the domain agree with the unclamped port).
- **Severity:** wrong result.
- **Rust port:** Faithful (evaluates the unclamped parameters).

#### WR-63. `SmCone::CreateCanonical` overloads misuse their arguments
- **Location:** `SmCone.cpp:341`-`348` `SmCone::CreateCanonical(Origin, BotRadius, SemiAngleDeg, pOptHeight, bSwapUV, bInsideOut)` (def. `:308`); `SmCone.cpp:392`-`400` `SmCone::CreateCanonical(Origin, BotRadius, TopRadius, Height, ...)` (def. `:375`).
- **What happens:** With a height, the first overload passes `FALSE` as the bounded constructor's `bInsideOut` and the caller's `bInsideOut` as `bMakeNurbGenCurve`, so an inside-out request is dropped. For `Height <= 0` the second overload builds the infinite cone with `TopRadius` as the semi-angle in degrees.
- **Trigger:** `CreateCanonical(O, 1.0, 30.0, &h, cone, FALSE, TRUE)` gives `GetInsideOut() == FALSE`; `CreateCanonical(O, 1.0, 30.0, 0.0, cone)` gives a 30 degree cone.
- **How found:** read during porting; compared by fixtures `canonical_semi_with_height_inside_is_mngc`, `canonical_height_zero_top_is_semi` (`surface.cone_p1`).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Reproduces both.

#### WR-64. `sm_TotalCoincidenceChecker` misses reversed coincident lines and returns `FALSE` as a status
- **Location:** `SmCone.cpp:2501` and `:2508`, `sm_TotalCoincidenceChecker` (file static, def. `:2454`).
- **What happens:** For lines, the reversed test computes `dDist_E_S` from `sPt01` and `sPt11` (the end of this line and the start of the other, instead of `sPt10`), so reversed coincident lines are never found. The function then returns `FALSE` (0), which is not `SM_SUCCESS`; callers' `SER` turn it into an error return.
- **Trigger:** Lines `(0,0,0)-(1,2,3)` and `(1,2,3)-(0,0,0)`.
- **How found:** read during porting; compared by fixture `coinc_reversed_line` (`surface.cone_p1`, a verbatim copy of the static in the shim).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`SmStatus` 0 error).

#### WR-71. `SmSurfPtSolveStepper::DropToIsoCurve` passes the same variable as guess and output of `DropPoint`
- **Location:** `SmSurfacePointSolve.cpp:4733` (`pIsoCurve->DropPoint(.., &dGuessParam, bGotThisDrop, dGuessParam, dDist)`), `SmCurve::DropPoint` at `SmCurve.cpp:3290`.
- **What happens:** `DropPoint` zeroes its output parameter before it reads the guess, so the guess (the crease parameter clamped to the section) is always `0`.
- **Trigger:** Any call (the method is not used upstream).
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result (a poorer guess for the local solve).
- **Rust port:** Passes `Some(0.0)`, as upstream's code ends up doing.

#### WR-72. The directed-maximise `SmSurfacePointSolver` ignores a failed `CalculateBoundingBox`
- **Location:** `SmSurfacePointSolve.cpp:463` (`m_pSrf->CalculateBoundingBox(m_sDomain, &sBBox, NULL, NULL)`), constructor at `:406`.
- **What happens:** The status is not checked. When the box cannot be built (for example a curve-bounded surface without boundary curves, whose surface cache fails on the base `SmSurface::GetKnots`), the box stays in its inside-out `Init()` state and the test point becomes a corner of it, near `±SM_BIG_DOUBLE`.
- **Trigger:** `SM_SO_3D_SIGNED_DIRECTED_MAXIMIZE` on a surface whose cache cannot be built.
- **How found:** generated difftest case (`surface.surface_point_solve_p1`, curve-bounded surfaces).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Reproduces it (the box stays `Extent3d::new()`); `SolidError::Unported` still propagates.

#### WR-73. `RemoveCoincidentNoEdgeProtoVertex` returns `SM_SUCCESS` as TRUE
- **Location:** `SmProtoTopology.cpp:6925` `SmProtoTopologyManager::RemoveCoincidentNoEdgeProtoVertex`: `if(eStat != SM_SUCCESS || sSols.GetSize() == 0) { return SM_SUCCESS; }` in a function returning `SmBoolean`.
- **What happens:** When the point solve fails or the proto vertex is not on the proto edge's curve, the function reports `TRUE` ("coincident and removed"), although nothing was removed.
- **Trigger:** A proto vertex off the first edge definition's curve.
- **How found:** generated difftest case (`topology.proto_topology_p1`).
- **Confirmed:** yes (the C++ run returns 1 with the proto vertex still listed).
- **Severity:** wrong result.
- **Rust port:** Same result (`true`), for parity.

### Intersection

#### WR-65. `SmVolume` "simple" box samplers and inverse solvers mix simple and compound maps
- **Location:** `SmVolume.cpp:5865` `EvaluateBoundingBoxSimple` and `:6057` `EvaluatePseudoBoxSimple` (sample `EvaluatePoint` and `Evaluate1stDerivatives` with compounding); `:919` `IsPointInParamDomain` (passes the last out-space image to the next map's in-space check); `:6857` `LocalPointSolveSimple` (solution value is the distance to the compound image); `:6665` `InvEvaluatePointSimple` (a failed local solve forces `rbSuccess = FALSE` after a successful global solve); `:3411` `InvEvaluatePoint`, `:3907` `InvMapPoint` (always report success); `:4512` `GlobalPointSolve` (found flag of the last point only).
- **What happens:** The projection-space boxes of a compound volume are boxes of its last out-space; domain checks and solution values use the wrong space; success flags are lost or always set.
- **Trigger:** `EvaluatePseudoBoxSimple` of a volume with a next map; `IsPointInParamDomain` of a chain with a plain `SmVolume` second map.
- **How found:** read during porting.
- **Confirmed:** yes (surface.volume_p2 difftest: the Rust port reproduces these and matches the C++ on generated cases and the `twist_then_base_*` fixtures).

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

#### WR-66. `SmSurfOfExtrusion::IsNurbSurfaceSurfOfExtrusion` returns a failing status as `TRUE`
- **Location:** `source/SMLib/src/SmSurfOfExtrusion.cpp:3520`, `:3534`, `:3539` (`SER` in `IsNurbSurfaceSurfOfExtrusion`, def. `:3447`, which returns `SmBoolean`).
- **What happens:** `SER` returns the failing `SmStatus` (a non-zero value) from a function returning `SmBoolean`, so a failed iso curve or control polygon reports "is an extrusion" with `rpSurfOfExtrusion == NULL`.
- **Trigger:** A test surface whose iso curve extraction fails.
- **How found:** read during porting (`surface.surf_of_extrusion_p2`).
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Returns the error (`Err`); the shim reports the raw value.

#### WR-69. `MakeDupin`'s bottom hyperbola uses `k2` for both axes
- **Location:** `source/SMLib/src/SmAdvSurfaceIntersector.cpp:1120`-`1122` `SmAdvSurfaceIntersector::MakeDupin`.
- **What happens:** The four hyperbolas of a saddle point should be one indicatrix branch pair `(1/sqrt|k1|, 1/sqrt|k2|)` and its conjugate pair `(1/sqrt|k2|, 1/sqrt|k1|)`. The top hyperbola uses the latter, the bottom one `(1/sqrt|k2|, 1/sqrt|k2|)`, so the bottom curve is a different conic and the singular directions found from it are wrong unless `|k1| == |k2|`.
- **Trigger:** `MakeDupin` (or `ComputeSingularityPoint`) at a hyperbolic point with `|k1| != |k2|`.
- **How found:** read during porting.
- **Confirmed:** yes (`intersect.adv_surface_intersector` difftest: generated `dupin` ops and `edge_dupin_kinds` compare the conics).
- **Severity:** wrong result.
- **Rust port:** Faithful.

#### WR-70. `SmAdvSurfaceIntersector::SolveBranch` never tests Gauss map separation
- **Location:** `source/SMLib/src/SmAdvSurfaceIntersector.cpp:1383` and `:1401` `SmAdvSurfaceIntersector::SolveBranch`.
- **What happens:** The method sets `m_bSeperateGaussMaps = TRUE` on entry and then calls `BranchIsGaussSeparate` only `if (!m_bSeperateGaussMaps)`, so the polar box test and the whole "separable" branch (one start point per separable sub-tree pair, the documented purpose of the class) are dead code: every intersecting leaf pair gives its own candidate.
- **Trigger:** Any `SolveIt(SM_SO_INTERSECT, ..)` on an `SmAdvSurfaceIntersector`.
- **How found:** read during porting.
- **Confirmed:** yes (`intersect.adv_surface_intersector` difftest: `solve` and `solve_branch` ops agree on one solution per intersecting leaf pair).
- **Severity:** wrong result (performance and duplicate start points).
- **Rust port:** Faithful.

#### WR-67. `SmBSplineCurve::GetControlPolygon` zeroes the weights array of a non-rational curve it was asked to fill
- **Location:** `SmBSplineCurve.cpp` (`GetControlPolygon`); read by `SmPolarConversion::SetUpPolarConversion` (`SmCircle` / `SmEllipse`).
- **What happens:** For a non-rational NURB the function sizes the weights array and then `ReSet()`s it, which zeroes the buffer and sets the size to 0. `SetUpPolarConversion` still indexes the buffer (`sWeights[ii*2-2 ..]`), reads `0.0` for every span, and so classifies every span as having constant weights (an exact conversion) and ends with `m_bPolarConversionPossible = TRUE`.
- **Trigger:** `SmSurfOfRevolution::TrimWithDomain` to a very short sweep, line generator `(0.5,0,1)-(3,0,1)` about Z, `swap`, trimmed to `[0, 0.5, 1, 0.5000001]`: the polar circle keeps the given NURB (knots `[0.5,0.5,0.5,0.5000001 x3]`, weights `NL_NOW`) instead of being rebuilt.
- **How found:** PR #51 review (adversarial difftest of `surface.surf_of_revolution_p2`).
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result (a differently parameterised polar circle; the geometry is the same).
- **Rust port:** `surface::...::ellipse::p2::set_up_polar_conversion` uses `0.0` weights for a non-rational NURB, as upstream reads them.

#### WR-68. `IsNurbSurfaceSurfOfRevolution` returns a failure status as `TRUE` with a NULL surface
- **Location:** `SmSurfOfRevolution.cpp:4659`, `:4676` (`SER(sm_TestForIsoCircles(..))` in a function returning `SmBoolean`). Same family as WR-52.
- **What happens:** `SER` returns the status (`SM_ERR` = 1001), which is truthy, so a failed iso-circle test is reported as "found" while `rpSurfOfRevolution` stays `NULL`. Callers that test the return value dereference or trust a missing surface.
- **Trigger:** a rational net whose generator lies on the rotation axis.
- **How found:** PR #51 review.
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result.
- **Rust port:** returns `Err(SM_ERR)` from `test_for_iso_circles`; the difftest shim records the C++ return value as is.

#### WR-74. `GlobalSurfaceIntersect` misses isolated tangential contact points
- **Location:** `SmSurface::GlobalSurfaceIntersect` (`SmSurface.cpp`) and what it dispatches to: `SmAdvSurfaceIntersector` / `SmSurfaceIntersector::DoIntersection` (`SmSurfaceIntersector.cpp`) for B-spline pairs, and the `SmSurfOfRevolution` override (inherited by `SmTorus`) for the torus/sphere pair. Cause not traced.
- **What happens:** Two surfaces that touch at single points (normals parallel there, no curve) come back with no output, or with only some of the points. Called with `SmApproxTol3d(1e-6)`, edges on, natural domains.
- **Trigger (ssi-corpus cases):** `c3_sphere_torus_outer`, `c3n_four_points` (0 of 4 points), `c3n_parab_apex`, `c3n_parab_parab_apex`, `y23_t6_ex4_tangential_point`: no output at all. `y23_fig1g_three_points`: one `TOUCHING` point, about 1.9 away from two of the three true points. `y23_t2_ex6_loop_and_point`: the crossing loop is found, the isolated point (0.41 away) is not. Verdict `INCOMPLETE` for all 7.
- **How found:** ssi-corpus, exact ground truth (jyviko's corpus, commit `b04c8a6`, run with difftest `--ssi-corpus`; same verdicts as the corpus' own `score.py`).
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result (missed intersection).
- **Rust port:** not reached yet (the general path needs `intersect.surface_intersector_p5`, the torus and sphere are not surface kinds yet); `--ssi-corpus` shows whether the port reproduces it.

#### WR-75. `GlobalSurfaceIntersect` misses tangential intersection curves or reduces them to points
- **Location:** `SmSurface::GlobalSurfaceIntersect` -> `SmAdvSurfaceIntersector` / `SmSurfaceIntersector::DoIntersection` (`SmSurfaceIntersector.cpp`). Cause not traced.
- **What happens:** Where two surfaces are tangent along a curve, the curve is not traced. Either nothing comes back, or a few `TOUCHING` points or a short piece of the curve.
- **Trigger (ssi-corpus cases):** `c2n_quartic_circle` (a closed tangent circle; no output). `y23_fig1f_high_order_contact` (two `TOUCHING` points; the branch is 1% covered). `y23_t2_ex1_tangent_line` (points and short pieces; 2% covered). `y23_t2_ex2_tangential_curve` (23% covered). Verdict `INCOMPLETE` for all 4.
- **How found:** ssi-corpus, exact ground truth.
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result (missed intersection).
- **Rust port:** not reached yet (`intersect.surface_intersector_p5`).

#### WR-76. `GlobalSurfaceIntersect` returns tangent curves twice
- **Location:** `SmSurface::GlobalSurfaceIntersect` -> `SmAdvSurfaceIntersector` / `SmSurfaceIntersector::DoIntersection`. Cause not traced; the duplicates suggest tracing from both ends of a tangent branch without merging.
- **What happens:** A tangent branch comes back as two (or more) curves lying on top of each other, sometimes also about 3.4e-5 off the surfaces (approximation tolerance 1e-6). An open tangent line overlaid by its own copy also reads as a closed curve.
- **Trigger (ssi-corpus cases):** `c2n_cylN_on_planeN` (a NURBS cylinder lying on a NURBS plane: two `TANGENT` curves, 100% overlap), `y23_t6_ex1_tangent_line` (100% overlap, up to 3.44e-5 from the surfaces), `y23_t2_ex4_tangential_curve_saddle` (58% overlap, up to 3.40e-5 off): verdict `WRONG TOPOLOGY`. `y23_t6_ex6_four_boundary_curves`: several `CROSSING` curves are copies of one another and branch 1 is only 88% covered: verdict `INCOMPLETE`.
- **How found:** ssi-corpus, exact ground truth.
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result (duplicated topology).
- **Rust port:** not reached yet (`intersect.surface_intersector_p5`).

#### WR-77. `GlobalSurfaceIntersect` runs curves through singular points instead of splitting them there
- **Location:** `SmSurface::GlobalSurfaceIntersect` and its dispatch: the `SmCone` override (cylinder/cylinder), the `SmSphere` override (sphere/cylinder), `SmAdvSurfaceIntersector` / `SmSurfaceIntersector::DoIntersection` (B-spline and torus pairs). Cause not traced.
- **What happens:** Where intersection branches cross (a singular point of the intersection, with the surfaces tangent there), the output curves pass through the crossing instead of ending at it, so no curve end lies at the singular point and the branch structure is wrong.
- **Trigger (ssi-corpus cases):** `c4_steinmetz` (two equal cylinders; 1 of 2 crossings not split), `c4_viviani_figure8` and `c4n_viviani_nurbs` (Viviani's curve; the double point not split), `y23_fig1a_two_loops_cross`, `y23_t6_ex3_two_crossing_curves`, `y23_t3_ex4_torus_ellipsoid`: verdict `WRONG TOPOLOGY`. In `y23_t2_ex3_loop_two_curves` (8% of a branch covered) and `y23_fig1d_two_quads` (one branch 1% covered) the crossings are also not split, next to missed branches: verdict `INCOMPLETE`. 8 cases.
- **How found:** ssi-corpus, exact ground truth.
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result (wrong topology).
- **Rust port:** not reached yet (`surface.cone_p3`, `surface.sphere`, `intersect.surface_intersector_p5`).

#### WR-78. Surfaces within the approximation tolerance of each other come back as coincident regions
- **Location:** `SmSurface::GlobalSurfaceIntersect` (plane against a NURBS plane through the general path; the `SmSphere` override for two spheres). Cause not traced.
- **What happens:** Two planes at an angle of 1e-6 to 1e-8 rad, and two planes or spheres 1e-7 apart, are reported as coincident: four `COINCIDENT` region boundary curves along the domain edges. The crossing line of the tilted planes is missed (half of it covered by chance), and the parallel pairs, which do not intersect at all, get a spurious region. Run at `SmApproxTol3d(1e-6)`: the surfaces stay within that tolerance of each other, so this may be the intended tolerance semantics. It is listed because the exact answer differs.
- **Trigger (ssi-corpus cases):** `c7_angle_planes_1e-6`, `c7_angle_planes_1e-7`, `c7_angle_planes_1e-8` (verdict `INCOMPLETE`). `c7_offset_planes_1e-7`, `c7_offset_spheres_1e-7` (verdict `WRONG TOPOLOGY`). The same pairs at 1e-3 to 1e-5 pass.
- **How found:** ssi-corpus, exact ground truth.
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result (near-degenerate input).
- **Rust port:** not reached yet (`intersect.surface_intersector_p5`, `surface.sphere`).

#### WR-79. Small closed intersection loops are missed or collapsed to one point
- **Location:** `SmSurface::GlobalSurfaceIntersect`: the `SmPlane` override (plane/sphere) and `SmAdvSurfaceIntersector` / `SmSurfaceIntersector::DoIntersection` (NURBS paraboloid/plane). Cause not traced.
- **What happens:** A closed loop of radius 1e-4 to 1e-7 is either not found or comes back as one `TOUCHING` point at its centre. This happens even when the radius (1e-4, 1e-5) is far above the approximation tolerance (1e-6).
- **Trigger (ssi-corpus cases):** `c7_tiny_loop_sphere_1e-4`, `_1e-5`, `_1e-6`, `_1e-7` (a sphere dipping just into a plane, a loop of diameter 2e-4 down to 2e-7: one `TOUCHING` point at the loop's centre, as far from the loop as its radius). `c7_tiny_loop_parab_1e-4`, `_1e-5`, `_1e-6`, `_1e-7` (a NURBS paraboloid cut by the plane `z = 1e-8` and the like, just above its apex: no output). Verdict `INCOMPLETE` for all 8.
- **How found:** ssi-corpus, exact ground truth.
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result (missed intersection).
- **Rust port:** not reached yet (`surface.sphere`, `intersect.surface_intersector_p5`).

#### WR-80. `GlobalSurfaceIntersect` fails with `SM_ERR_NULL_POINTER` on two B-spline pairs
- **Location:** `SmSurface::GlobalSurfaceIntersect` -> `SmAdvSurfaceIntersector` / `SmSurfaceIntersector::DoIntersection`. The NULL that triggers the status was not traced.
- **What happens:** The call returns `SM_ERR_NULL_POINTER` (1006) with no curves, although the surfaces are valid and the intersection exists: two NURBS sheets in 4th-order contact along a parabola, and two tube-like arms crossing in an X (two loops with two singular points).
- **Trigger (ssi-corpus cases):** `c2n_quartic_contact_parabola`, `y23_t2_ex5_two_loops`. Verdict `ERROR`.
- **How found:** ssi-corpus, exact ground truth.
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result (failure on valid input).
- **Rust port:** not reached yet (`intersect.surface_intersector_p5`).

#### WR-81. `SmCPolyFace::RemovePolyFace` of a non-member removes the first member
- **Location:** `source/SMLib/src/SmPoly.cpp:13195` `SmCPolyFace::RemovePolyFace`.
- **What happens:** The status of `FindElement` is ignored and the index starts at 0, so a face that is not a member removes member 0. `~SmPolyFace` calls it for any face whose `m_pCPolyFace` still points at the composite, for example after an earlier `RemovePolyFace` of that face.
- **Trigger:** `RemovePolyFace(f)` twice, or deleting a face after removing it from its composite.
- **How found:** generated difftest case (`query.poly` mutate ops).
- **Confirmed:** yes (`query.poly` generated `cpoly_remove` cases agree with the port, which keeps the behaviour).
- **Severity:** wrong result.
- **Rust port:** Faithful.

#### WR-82. `SmCPolyFace::AddPolyFace` accepts a composite face, so composite queries recurse without bound
- **Location:** `source/SMLib/src/SmPoly.cpp` `SmCPolyFace::AddPolyFace`, and the composite overrides of `GetPolyEdges`, `GetNormal`, `CalculateBoundingBox` and `GetOKBackPtrs`.
- **What happens:** `AddPolyFace` takes any `SmPolyFace`, including another composite or the composite itself, and sets that face's `m_pCPolyFace` (overwriting a composite's own). The composite overrides then call the same virtual on every member, so a composite that contains itself recurses until the stack overflows.
- **Trigger:** `c->AddPolyFace(c)` followed by `c->GetPolyEdges(...)` (or `GetNormal`, `CalculateBoundingBox`, `GetOKBackPtrs`). Upstream callers only add shell faces (`SmLineSegClass.cpp:1879-1883`).
- **How found:** review of the `query.poly` port; the Rust mirror overflowed its stack on the same sequence.
- **Confirmed:** no (by reading; not run against the C++ build).
- **Severity:** crash (stack overflow) on misuse.
- **Rust port:** Deviates: `add_cpoly_face` / `cpoly_add_poly_face` return `InvalidInput` for a composite face.

#### WR-83. `SmVertexuse::ComputeUVSector` miscounts opposite orientations (operator precedence)
- **Location:** `source/SMLib/src/SmVertexuse.cpp:993`-`994`.
- **What happens:** `lOppositeCount = FU == OPP ? 1 : 0 + LU == OPP ? 1 : 0` parses as `FU == OPP ? 1 : (LU == OPP ? 1 : 0)`, so the count is never 2: a sector whose face use and loop use are both opposite gets the complementary angle (`EndTan` to `BegTan`) instead of `BegTan` to `EndTan`.
- **Trigger:** an edge vertex use on an opposite face use of an opposite (inner) loop use.
- **How found:** read during porting.
- **Confirmed:** no (the path is deferred in the difftest until `topology.face_p2` lands).
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`topology::vertexuse::compute_uv_sector`).

#### WR-84. `SmEdgeuse::IsConvexRadialSector` reports a failed binormal evaluation as convex
- **Location:** `source/SMLib/src/SmEdgeuse.cpp:3592` `SmEdgeuse::IsConvexRadialSector` (the two `SER(EvaluateBinormalStepOff(...))`).
- **What happens:** `SER` returns the non-zero `SmStatus` from a function returning `SmBoolean`, so a failing step-off evaluation (stepping off the face's UV domain, a degenerate binormal) makes the sector count as convex (`TRUE`), as in WR-52.
- **Trigger:** A sample whose step-off point leaves the face's UV domain (large edge tolerance with `bOptStepOff`, sliver faces), or an edge use whose binormal cannot be evaluated.
- **How found:** read during porting (unit `topology.edgeuse_p2`).
- **Confirmed:** no (the step-off path is deferred on `topology.edgeuse_p3`).
- **Severity:** wrong result.
- **Rust port:** Faithful (`Ok(true)` on such an error).

#### WR-85. `SmEdgeuse::CheckAndSetUVTrimCurve` deletes the curve it says the caller keeps
- **Location:** `source/SMLib/src/SmEdgeuse.cpp:4732` `SmEdgeuse::CheckAndSetUVTrimCurve`; contract in `SmEdgeuse.h:160`.
- **What happens:** The header says a rejected `pUVTrimCurve` stays "managed by caller", but the curve was attached first and the restoring `SetUVTrimCurve(pOldUVTrimCurve, 0.0, TRUE, ...)` deletes it (it is the attached curve), so a caller that deletes it frees it twice. Passing the curve already attached is worse: the first `SetUVTrimCurve` does nothing and the accepted branch then deletes `pOldUVTrimCurve`, the attached curve, leaving a dangling pointer. An `SER` return inside the accepted branch (the precise distance measurement, `CreateUVTrimCurve`) leaks the detached old curve.
- **Trigger:** Any UV curve that is not good enough; any call with `GetUVTrimCurve()` as the argument.
- **How found:** read during porting (unit `topology.edgeuse_p2`); the difftest shim relies on the deletion (it never frees the curve it passes).
- **Confirmed:** no.
- **Severity:** crash (double free / use after free).
- **Rust port:** Takes the curve by value: it is attached or dropped; the attached curve cannot be passed.

#### WR-86. `SmFace::GetPointsInFace` samples a rectangular face from `0`, not from its domain
- **Location:** `source/SMLib/src/SmFace.cpp:21206`-`21230`, `SmFace::GetPointsInFace` (def. `:21167`).
- **What happens:** On a face with `GetRectangularTrim() == TRUE` the sample points are `(k * dUInc, j * dVInc + dither)` with `dUInc` / `dVInc` fractions of the UV domain's lengths: the domain minimum is never added, so a face whose domain does not start at `(0, 0)` gets points outside it (and the positions and normals of those points). `dUInc` is also divided by `min(2 * count, 30) + 1` before the rectangular branch resets the column count to `min(count, 30)`, so the columns do not span the domain.
- **Trigger:** `GetPointsInFace(1, ..)` on a rectangularly trimmed face with UV domain `[3, 4] x [-7, -6.5]` returns `uv = (1/3, 0.25)`.
- **How found:** adversarial / edge-case fixture.
- **Confirmed:** yes, by the C++ build (fixture `points_dom_offset_1` of `topology.face_p2`).
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`topology::face::p2::points_in_face`).

#### WR-87. `sm_CheckSeamCrossing` returns `SM_SUCCESS` as its `SmBoolean`
- **Location:** `source/SMLib/src/SmFace.cpp:145`, `sm_CheckSeamCrossing` (def. `:132`).
- **What happens:** For a surface closed in neither direction it returns `SM_SUCCESS` (`1000`), which reads as `TRUE` ("a seam was crossed"), from a function documented to return whether a shift happened.
- **Trigger:** `bClosedU == bClosedV == FALSE`.
- **How found:** read during porting.
- **Confirmed:** yes, by the C++ build (fixture `seam_neither` of `topology.face_p2`). Its only caller (`SmFace::ComputeLoopOrientation`) ignores the return value, so nothing observable depends on it today.
- **Severity:** wrong result (latent).
- **Rust port:** Reproduces it (`topology::face::p2::check_seam_crossing` returns `true`).

#### WR-93. `SmLoopuse::IsClosed3d` never reports a loop open
- **Location:** `SmLoopuse.cpp:873`-`885` (`SmLoopuse::IsClosed3d`; also reached through `SmLoop::IsClosed3d`, `SmLoop.cpp:248`).
- **What happens:** For a gap over its cross-section tolerance it computes `dOverTol = sXSectTol3d - sqrt(dGap3dSq)`, which is negative there, and keeps it only when `dOverTol > dMaxOverTol` (initially `0.0`). No gap ever qualifies, so `pMaxOverTolEdgeuse` stays `NULL` and the loop is always reported closed, with the largest gap and its tolerance as outputs. `bCheckAllGaps == FALSE` still stops the scan at the first gap over tolerance, so the reported "largest" gap is then only the largest seen so far.
- **Trigger:** Any edge loop with an end-to-end gap larger than the sum of the two edge uses' tolerances.
- **How found:** read during porting; the difftest compares it (unit `topology.loopuse`, `closed3d` cases).
- **Confirmed:** yes: the `closed3d` cases of `fixtures/topology.loopuse/tolerance_limit.json` set every edge tolerance to 0, 1e-15 or -1e-12 so that real end gaps exceed `XSectTol3d`; both sides still report those loops closed (with the largest gap seen, also when `bCheckAllGaps == FALSE` stops early).
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`topology::loopuse::is_closed_3d`).

#### WR-154. Discontinuity classifiers select boundary solution slots with V+V offsets
- **Location:** `source/SMLib/src/SmCurveClass.cpp:9241`, `:9654`, `:10121-10129`.
- **What happens:** The UV and 3D surface classifiers select the last boundary's solution array using `2 * V_count - 1`, while the later consumer selects its curve using `U_count + V_count - 1`. The volume classifier repeats V in the offsets for its last three boundary slots too. Unequal counts can associate an internal boundary's solutions with a different outer boundary, or omit an outer solution array.
- **Trigger:** different U/V knot or discontinuity counts, with intersections on the affected boundaries.
- **How found:** read during porting `intersect.curve_class_p8` (PR #152).
- **Confirmed:** no numerical reproduction isolated; native unequal-count fixtures are dependency-deferred in Rust before downstream classification.
- **Rust port:** preserves these exact slot expressions; crossing classifications are uncertified until solution insertion is available.

#### WR-155. Two volume boundary solution arrays share the same borrowed storage
- **Location:** `source/SMLib/src/SmCurveClass.cpp:10002`, `sSolutions4(20,aData4), sSolutions5(20,aData4)`.
- **What happens:** Both arrays initially refer to `aData4`. Writing the sixth boundary's solutions can overwrite the fifth boundary's saved solutions before the later classification scan. `aData5` is not declared; the arrays do not use independent buffers.
- **Trigger:** both affected boundary slots contain results that remain in their borrowed arrays.
- **How found:** read during porting `intersect.curve_class_p8` (PR #152).
- **Confirmed:** no isolated native reproduction.
- **Rust port:** independent owned vectors; this aliasing behavior is not certified or reproduced. All volume plane-intersection paths remain deferred to `surface.surface_p8`; revisit the output lifecycle when wiring that dependency.

#### WR-156. The volume discontinuity unaligned check tests U three times
- **Location:** `source/SMLib/src/SmCurveClass.cpp:10025-10027`.
- **What happens:** The rejection predicate repeats `m_bUnalignedU` and never checks V or W, so a V/W-only unaligned discontinuity can proceed into the aligned-plane algorithm.
- **Trigger:** a discontinuity report with U aligned and V or W unaligned.
- **How found:** read during porting `intersect.curve_class_p8` (PR #152).
- **Confirmed:** no native execution of the unsupported report.
- **Rust port:** retains the U-only predicate, then rejects empty/malformed coordinate lists safely; custom discontinuity reports are outside the oracle's coverage.

#### WR-157. `CheckIfHaveExistingSSI` re-copies the accumulated curve lists on every match
- **Location:** `source/SMLib/src/SmTopologyIntersector.cpp:184-249`, `SmTopologyIntersector::CheckIfHaveExistingSSI`.
- **What happens:** For each table entry matching the surface pair the function appends the entry's arrays to the outputs and then runs `for (jj=0; jj<r3DCurves.GetSize(); jj++)` over the *whole* accumulated output, replacing every 3D and UV B-spline curve by a fresh copy. With two matching entries the first entry's copies are copied once more and those first copies leak; with one entry the behaviour is as intended. A 3D curve that is not an `SmBSplineCurve` is `SER(SM_ERR)` after the output arrays were already partly filled with copies (also leaked by a caller that treats the status as fatal).
- **Trigger:** `RegisterExistingSSI` twice for the same `(pBrepSurface, pOtherSurface)`, then `CheckIfHaveExistingSSI` for that pair (fixture `analytic_tsurf_pair_table`, `fixtures/boolean.topology_intersector_p1/upstream.json`).
- **How found:** read during porting (`boolean.topology_intersector_p1`, PR #146); the difftest runs it in the release build.
- **Confirmed:** no. The fixture runs the path in the C++ build and the doubly copied curves compare equal to the originals, so neither the extra copy nor the leak is observable in a result; both follow from source inspection.
- **Severity:** memory leak / wasted work; results unaffected.
- **Rust port:** Faithful: the re-copies go to the caller's curve store and the earlier handles stay unreferenced; the non-B-spline case reports the `SM_ERR` in `ExistingSsi::status` with the outputs filled so far.

### Healing

#### WR-88. `BuildSampleSet` spaces the `v` samples of non-B-spline surfaces by array index
- **Location:** `source/SMLib/src/SmGap.cpp:1903`-`1907`, `SmGapFunction::BuildSampleSet`.
- **What happens:** The interior `v` samples are set to `jj / (m_lSampleCntV - 1)` where `jj` is the array index, which starts at `m_lSampleCntU + 1`, not the `v` sample number. The interior `v` parameters are therefore well outside `[0, 1]` (the sample domain is extrapolated).
- **Trigger:** a surface gap function over a non-B-spline surface (for example an `SmCurveBoundedSurface`) with `SetSampleCnt(u, v)`, `v > 2`.
- **How found:** read during porting; compared in fixture `srf_pt_curve_bounded_v_stride` (`fixtures/heal.gap/upstream.json`).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`heal::gap::GapFunction::build_sample_set`).

#### WR-89. `BuildSampleSet` samples B-spline knot spans from the first knot, not the sample interval
- **Location:** `source/SMLib/src/SmGap.cpp:1845`-`1859` (`u`), `:1867`-`1881` (`v`).
- **What happens:** After finding the knot indices `lStartIndxU..lEndIndxU` of the sample interval, the span loop walks `sUniqueKnotsU[0..lIvlCntU]` instead of `sUniqueKnotsU[lStartIndxU..lEndIndxU]`, and the normalised values (relative to the whole knot range) are then mapped onto the sample interval with `m_sSampleIvlU.Evaluate`. A sample interval that is not the whole knot range is sampled at the wrong parameters (the loop also overwrites the last end sample set "to avoid numerical tolerances").
- **Trigger:** `SmCrvSrfGapFunction` (or any gap function from B-spline geometry) over an edge interval smaller than the curve's knot range.
- **How found:** read during porting; compared in fixture `crv_pt_bspline_sub_interval`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Reproduces it.

#### WR-90. `SmGapArray::Append` does not update the longest-gap index
- **Location:** `source/SMLib/inc/SmGap.h:882`.
- **What happens:** `Append` adds the other array's gaps without comparing them with the current longest, so `GetMaxGap` / `GetMaxGapLength` can miss an appended longer gap (an empty array stays without a longest gap).
- **Trigger:** `a.Add(len 1)`, `b.Add(len 2)`, `a.Append(b)`.
- **How found:** read during porting; compared in fixture `gap_array_append`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`heal::gap::GapArray::append`).

#### WR-99. `SmSurface::LocalSurfaceIntersect` retries maximum boundaries after a found boundary solution
- **Location:** `source/SMLib/src/SmSurface.cpp:27782`, `:27804`, `:27826` (`SmSurface::LocalSurfaceIntersect`, step 4: the boundary curve solves at the point where the Newton solve stopped out of bounds).
- **What happens:** The conditions read `(!bFoundSolution && SM_ARE_SAME(x, min)) || SM_ARE_SAME(x, max)`: the `!bFoundSolution` guard binds to the minimum test only, so a solution found on an earlier boundary is solved again (and replaced, or lost when that solve fails) whenever the stop point is also on a later maximum boundary.
- **Trigger:** `bDoBoundaryTesting`, a Newton solve stopped by the boundary handler at a corner of the domain whose second side is a maximum `v` (or the other surface's maximum).
- **How found:** read during porting.
- **Confirmed:** no (the boundary curve solve, `sm_IntersectSrfBndryCrv`, is unit `surface.surface_p8`; those cases defer).
- **Severity:** wrong result.
- **Rust port:** Reproduces it.

### Numerical integration

#### WR-94. `SmIntegrator::IntegrateIt` hides integration/evaluator failure as zero
- **Location:** `SmIntegrator.cpp:108-122`, `SmIntegrator::IntegrateIt`.
- **What happens:** If `RungeKuttaODEIntegrate` fails, the scalar wrapper sets its
  result to `0.0` and returns `SM_SUCCESS`. A failed numerical integral is therefore
  indistinguishable from a valid zero, including for callers computing curve length.
- **Trigger:** Integrate the constant function `1` over `[0,1]` at accuracy `0`,
  or an evaluator that returns `SM_ERR` partway through the interval.
- **How found:** port source review and differential fixtures `zero_accuracy_0`
  and `evaluator_error_0` in `fixtures/mass.integrator/integrator.json`.
- **Confirmed:** yes, the actual C++ scalar wrapper reports success with zero;
  the direct ODE wrapper reports failure for the same zero-accuracy integral.
- **Severity:** wrong result / hidden numerical failure.
- **Rust port:** retained for parity; `Unported` still propagates.

#### WR-101. `SmProtoEdge::Split` gives the upper child proto edge the empty interval `[dT, dT]`
- **Location:** `source/SMLib/src/SmProtoTopology.cpp:3037`-`3038` (`SmProtoEdge::Split`).
- **What happens:** After `this->m_pXSectInterval.SetMinMax(min, dT)` the next line sets the new upper proto edge to `SetMinMax(dT, m_pXSectInterval.GetMax())`, which reads the maximum of `this` that was just changed to `dT`. The upper child gets `[dT, dT]` instead of `[dT, oldMax]` (its edge definitions are split correctly, by `SmEdgeDefinition::Split`).
- **Trigger:** Any interior split of a proto edge (from `SplitIntersectingProtoEdges`).
- **How found:** read during porting.
- **Confirmed:** no (the split needs `topology.proto_topology_p2`, so the difftest cases that reach it are deferred).
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`ProtoTopologyManager::proto_edge_split`).

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

#### DOC-4. Revolution STEP equal-order assertion conflicts with boundary iso evaluation
- **Location:** `source/SMLib/src/SmSurfOfRevolution.cpp:2770-2793`; caller `SmIsoCurve::Evaluate` in `SmIsoCurve.cpp:123-160`.
- **What happens:** The STEP evaluator documents and asserts equal U/V derivative orders, but iso curves request `(k, 0)` or `(0, k)`. Release C++ computes sweep-only `(k, 0)` derivatives correctly; rejecting every unequal order prevents legitimate boundary point solves. When V exceeds U, generator derivatives beyond `min(U, V)` retain the default `SmVector3d` `SM_UNDEF_DOUBLE` sentinel.
- **Trigger:** `LocalPointSolve_1` on a STEP-wrapped revolution at its minimum-V boundary, regression `regression_matrix_0363` in `fixtures/surface.surface_point_solve_p3/step_revolution_regressions.json`.
- **How found:** native differential regression while porting `surface.surface_point_solve_p3` (PR #106).
- **Confirmed:** yes for the release build: the sweep-only boundary solve returns U `158.85355268828013`; the assertion conflict and sentinel initialization were confirmed by source inspection. Debug builds were not run.
- **Severity:** docs / debug assertion.
- **Rust port:** Matches release C++ rather than enforcing the assertion; derivative orders remain bounded to 3.
