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

#### UB-3. Polymorphic curve and surface readers recurse without a depth limit [PSIRT]
- **Location:** `SmCurve.cpp:23709` `SmCurve::ReadFromDB` dispatching to `SmProjectedCurve.cpp:690` `SmProjectedCurve::ReadFromDB` (and to the offset and iso curve readers, e.g. `SmOffsetCurve.cpp:567` `SmOffsetCurve::ReadFromDB`), which read their base curve through `SmCurve::ReadFromDB` again; likewise `SmSurface.cpp:34641` `SmSurface::ReadFromDB` and `SmOffsetSurface.cpp:2197` `SmOffsetSurface::ReadFromDB` for surfaces.
- **What happens:** Each nesting level costs a few bytes of input and one stack frame chain, so a crafted file overflows the stack.
- **Trigger:** A file of N nested projected-curve records with N large (thousands). Two concrete triggers (reader fuzz, 2026-10-20): `Fillet/eg1_srf2.smb` with its first 3D curve header preceded by 200000 copies of `Object Type 14036` / `Curve Dim 3` / `0 0 1` / `0.1` / `0 1` / `0` / `//base` (offset curves), or its surface header preceded by 200000 copies of `Object Type 15400` / `0.1` / `0` / `0` / `0` / `0` / `//base` (offset surfaces).
- **How found:** read during porting (commit security review of `curve.projected_curve`); confirmed by the `.smb` reader fuzz (`crates/difftest/examples/reader_fuzz.rs`).
- **Confirmed:** yes. `SmBrepData::ReadFromFile` (ASCII) on both 200000-level files segfaults (SIGSEGV, stack overflow) in the C++ reference build, with an 8 MiB stack; 200 levels read fine. The Rust port returns `InvalidInput` for both.
- **Severity:** crash/UB (stack exhaustion).
- **Rust port:** Limits nesting to 32 levels (`DatabaseReader::enter_nested`; 64 before the wasm32 stack review) and returns `InvalidInput` for deeper input.

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
- **Platform dependence:** confirmed 2026-10-21 on macOS arm64 (Apple clang 21, libc++, C++17): libc++ does not bound `>> char*`, so the `-O2` build overflows too. `topology.database_io` gen0185 (and gen0576, gen0883, gen1324, gen1363, gen1519, gen1590, gen1652, gen1783) abort in `__stack_chk_fail` <- `SmDatabaseIO::ReadType`. Only libstdc++ under `__OPTIMIZE__` makes the reference build safe. GitHub issue #450.

#### UB-6. Other ASCII readers use uninitialised locals after a failed extraction [PSIRT]
- **Location:** `SmAttribute.cpp:210` `SmAttributeData::ReadIndexedAttributesFromDB` (uninitialised count when the stream is already failed or at EOF; a count near `ULONG_MAX` wraps `SetSize`); `SmLine.cpp:2505` `SmLine::ReadFromDB` and `SmBSplineCurve::ReadFromDB` (short header leaves `n`, `p` uninitialised before the allocation); `SmEllipse.cpp:1488` `SmPolarConversion::ReadFromDB`; `SmConic.cpp:905` `SmConic::ReadFromDB` (uninitialised `lConicType`); `SmCrvInVolume.cpp:1404` `SmCrvInVolume::ReadFromDB` (dereferences a NULL volume for `SmVolume_TYPE`).
- **What happens:** Once `failbit`/`eofbit` is set, `>>` stores nothing, and the readers go on using the unset locals.
- **Trigger:** A record cut short in its header, or holding a non-numeric token where a count is expected.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` for unreadable headers and bounds every count.
- **Reconfirmed (surface.plane_p2 fuzz00842, 2026-10-22):** `SmPlane::ReadFromDB` (`SmPlane.cpp:3590`) of an ASCII stream with an empty plane header reads the B-spline header as plane values; `SmBSplineSurface::ReadFromDB` (`SmBSplineSurface.cpp:6401`) then reads `m` from `.69...`, the stream fails and `n m p q rat` stay uninitialised. Three runs of the reference build returned surfaces of 40251, 62874 and 65074 record values and one segfaulted (SIGSEGV). The plane_p2 runner refuses a header of other than 15 values.
- **Platform dependence:** confirmed 2026-10-21 on macOS arm64 (libc++). libc++ sets `failbit` when it extracts a subnormal double (strtod `ERANGE` underflow; libstdc++ stores the value and does not fail), so an ASCII `.smb` holding a subnormal (`topology.brep_data` generator, e.g. gen0010 `4.7e-321`) fails the stream and every later `>>` leaves its local unset. gen0018 then hangs in `SmAttributeData::ReadIndexedAttributesFromDB` (`SmAttribute.cpp:224`-`229`, uninitialised `ULONG lCnt` drives the loop) under `SmFaceData::ReadFromDB`; gen0110 segfaults in `SmShellData::ReadFromDB`. Of 2379 brep_data cases, 84 SIGSEGV and 228 hang on macOS; 0 fail on x86-64 Linux. GitHub issue #450.

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
- **Rust port:** `CompositeCurve::with_curves` returns `InvalidInput`; nesting is bounded by `DatabaseReader::enter_nested` (32).

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

#### UB-164. `SmBrep::MakeTopologyFromData` uses the cross-reference indices of an accepted `.smb` unchecked [PSIRT]
- **Location:** `source/SMLib/src/SmBrep.cpp:19284`-`20660` `SmBrep::MakeTopologyFromData`, reached from `SmBrep::ReadFromFile` after `SmBrepData::ReadFromFile` succeeds. For example `:20343` indexes `m_vFaces[rFaceuseData.m_lFace]` and dereferences the face it finds; `:20342` indexes `m_vFaceuses` with the shell's `m_lFaceuseStart + kk`, and the non-manifold edge use links (`m_lNextEU`, `m_lMateNextEU`, `:20247`-`:20263`) index `m_vEdgeuses` with file values the same way, as do `m_vSurfaces[rF.m_lSurface]` and `m_vShells[rR.m_lStartShell + jj]`. `SmTArray::operator[]` checks the index only under `SM_DEBUG_CODE` (`SmTArray.h:454`).
- **What happens:** `SmBrepData::ReadFromFile` checks the stream syntax, not the cross-references between its sections. `MakeTopologyFromData` then indexes the section arrays with the face, faceuse and edge use indices read from the file, without range checks, and follows the pointers it finds there.
- **Trigger:** An ASCII `.smb` with one index field overwritten so that `SmBrepData::ReadFromFile` still succeeds, e.g. `plane_square.smb` with faceuse 0's face index `0` changed to `9` (the file has one face).
- **How found:** crash in the C++ reference run (`topology.brep_p7` difftest, guard disabled for the measurement). gdb: SIGSEGV in `SmBrep::MakeTopologyFromData` <- `SmBrep::ReadFromFile`.
- **Confirmed:** yes. Single-byte overwrites (digits, `-`, `.`) accepted by `SmBrepData::ReadFromFile`, run one per process: 11 of 600 crashed with SIGSEGV (rc=139; 100 streams each of `Endcap`, `Mouse`, `box_with_large_fillet`, `cone`, `disk`, `ellips_ts2`: 2, 1, 2, 4, 1, 1). A batch of 100 such streams per file crashed in 13 of the 15 `pt_TestFiles` (all but `torus`, `vase_rev`).
- **Severity:** crash/UB (out-of-bounds read, wild pointer dereference).
- **Rust port:** `MakeTopologyFromData` belongs to `topology.brep_p10` (unported; `p7::read_from_file` defers there). Its port must range-check every cross-reference index and return `InvalidInput`. The `topology.brep_p7` difftest keeps such streams away from the C++ side (`cpp-guard:unvalidated-topology`).
- **Healer trigger (2026-10-20, measured 2026-10-23):** healing (`bAlwaysHeal` or an old healer version) of a B-rep built from cross-references that are in range but inconsistent: an edge use linked into a loop its record does not name, a loop's edge use count or start changed, an edge on another edge's curve, an edge use type changed. `MakeTopologyFromData` builds them, and the healer walks the result unchecked. Confirmed: yes. Single `Set` patches over the upstream `.smb` files with healing forced: 21 SIGSEGV and 2 hangs in 400 cases (e.g. `Tess/nurby_box_cyl_sphere.smb` with edge 1's curve index `1` changed to `6`: SIGSEGV when healed, clean with healing off). Whether a given patch crashes depends on the geometry. The `topology.brep_p10` runner's `heal_guard` rejects healing whenever `Set` patches change data that the build accepts (the Rust build, healing off, does not return `InvalidInput`).

#### UB-223. `SmIsoCurve` / `SmProjectedCurve` / `SmBSplineSurface` readers cast unchecked file values to enums [PSIRT]
- **Location:** `source/SMLib/src/SmIsoCurve.cpp:421` (`(SmSurfParamType) lSurfParam`, `SmIsoCurve::ReadFromDB`, def. `:369`); `SmProjectedCurve.cpp:785` (`(SmProjectionType) lProjType`, `SmProjectedCurve::ReadFromDB`, def. `:690`); `SmBSplineSurface.cpp:6515` (`(SmBSplineSurfaceForm) lBSplineSurfaceForm`, reached from the iso curve's nested surface).
- **What happens:** A `ULONG` read from the file is cast to an enum without a range check. Values outside the enum's range of values (`SmSurfParamType`, `SmBSplineSurfaceForm`: 0..=15; `SmProjectionType`: 0..=3) are undefined behaviour; in the reference build the raw value is stored and later read back (`m_eProjType` comes back as `-2147483648` for `2147483648`). In the ASCII branches an extraction from an already-failed stream leaves `lSurfParam`, `bFromLeft` (`SmIsoCurve.cpp:398`, `:400`) and `lProjType` (`SmProjectedCurve.cpp:729`, `:754`) uninitialised before the same cast.
- **Trigger:** `curve.iso_curve` fuzz `fuzz00479` (`/db/sp = 2147483648`), `fuzz01374` (`/db/form = 9223372036854775807`); `curve.projected_curve` fuzz `fuzz00412` (`/ptype = 2147483648`), `fuzz00488` (`/ptype = 4`).
- **How found:** generated difftest case (fuzz sweep, `--fuzz 2000`).
- **Confirmed:** no (no crash; the C++ run kept and returned the out-of-range value).
- **Severity:** crash/UB.
- **Rust port:** Maps unknown values to `SM_SP_UNKNOWN` / `SM_PT_UNKNOWN`; the difftest runners keep out-of-range values away from the C++ side (`curve_iso_curve.rs`, `curve_projected_curve.rs`).

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
- **Confirmed:** yes, by valgrind: a conditional jump on an uninitialised value in `N_BasisInsertKnots` (under `N_CreateSkinSrfBoundaryContinuity`, `topology.brep_p1` edge-edge blends through `CreateBlendSurface`); no invalid read, the slot lies inside the allocation.
- **Why the output cannot depend on it:** the read is in the merge's `else` branch (`NL_BasisAdv.cpp:291-295`). After `S[++ms] = X[j++]` takes the last insert knot (`j = mx + 1`), `R[i] LT X[j]` reads `X[mx+1]`. If it is true, `R[i]` is appended now; if it is false (or reads garbage), the loop continues (`i < mr - p` still holds) and the next pass takes the `j GT mx` branch, which appends the same `R[i]`. Either way `S` is identical, so running the reference through it is safe, and no exact guard exists (every knot insertion reads it). valgrind runs suppress exactly this frame with `harness/valgrind/ub11.supp` (`Memcheck:Cond`, `fun:N_BasisInsertKnots`).
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
- **Trigger:** Degree-0 vectors whose ends differ by less than `NL_PTOL`; inner knots within `NL_PTOL` of an end; unclamped vectors. For `N_MakeKnotsCompatible` also: degree 0 on a vector whose end knots repeat (a degree-1 clamped vector passed with degree 0, or a duplicated end knot), an inner knot above the end knot (also after the rescale), and a vector with fewer than `p + 1` equal end knots. For `N_GetCompatibleKnotArrayMult` also: degree 0 on vectors whose end knots repeat (`R[mr-1] == R[mr]` matched against `S[j]` advances both indices past the ends), an inner knot above the end knot, and an end knot replaced by a smaller value.
- **How found:** read during porting; `N_MakeKnotsCompatible` also by `difftest --fuzz` on `nlib.basis_basic` (17 of 3000 fuzz cases, 249 of 30000, where the C++ ran on the shim's `+inf` padding and Rust reported the read); `N_GetCompatibleKnotArrayMult` also by `difftest --fuzz` (issue #182: 55 of 3000 fuzz cases, 501 of 30000, same `+inf` padding).
- **Confirmed:** no. The ffi now rejects exactly these inputs for both routines: the shim replays each routine's rescale (`N_BasisReparam` itself) and index walk and checks each index before the read, so upstream never runs on them. `N_GetCompatibleKnotArrayMult` is guarded by `sc_nlib_basis_adv_compat_mult_walk` and `N_MakeKnotsCompatible` by `sc_nlib_basis_basic_make_compatible_walk`; the `+inf` padding stays as a second line of defence.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` for the out-of-range reads. The `nlib.basis_adv` difftest compares this with the shim's walk check for `N_GetCompatibleKnotArrayMult`, and the `nlib.basis_basic` difftest for `N_MakeKnotsCompatible` (`read_past_end` on both sides; fixtures in `fixtures/nlib.basis_basic/fuzz_regressions.json`).

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

#### UB-196. `N_BasisInsertKnots` writes past its output for knot vectors shorter than `2p+2`
- **Location:** `NL_BasisAdv.cpp:253`, `N_BasisInsertKnots` (copies at `:275-276` and `:302-303`).
- **What happens:** The routine always copies `R[0..=p]` and `R[mr-p..=mr]` into `S`. With fewer than `2p+2` knots these ranges overlap, so it writes `2p+2+mx+1` knots instead of the documented `mr+mx+2` and overruns a caller buffer of that size (heap corruption).
- **Trigger:** `mr < 2p+1` with `X[0] > R[p]` and `X[mx] < R[mr-p]`, which needs an unsorted `R` (e.g. `p = 1`, `R = [4294967296, 0]`, `X = [1e300, 1.5, 2.5, 3.5]`). The range check rejects such `R` when it is sorted.
- **How found:** crash in the C++ reference run (`difftest --fuzz 30000` on `nlib.basis_adv`, issue #182: two cases aborted in `malloc` with a corrupted heap).
- **Confirmed:** yes, by those two aborts (fixtures `fuzz_insert_short_unsorted_r`, `fuzz_insert_short_r_four_new_knots` in `fixtures/nlib.basis_adv/fuzz_regressions.json`). The shim now sizes `S` for every knot written, so the reference runs them and the results are compared.
- **Severity:** crash/UB.
- **Rust port:** Returns all `2p+2+mx+1` knots (a `Vec` grown as written), as upstream computes them.

#### UB-238. `SmBSplineCurve::CreateCanonical` corrupts the heap for a NaN knot
- **Location:** `SmApiCreateCanonicalCurve` -> `SmBSplineCurve::CreateCanonical` and its NLib curve construction; the write itself was not isolated (no valgrind on the reference machine). glibc aborts later: `free()` in `N_EndNurbs` (`NL_Util.cpp:401`, "double free or corruption (out)") or the next `realloc`.
- **Trigger:** `--fuzz 30000` of `primitive.api_curves`: `fuzz21945` (degree 4, knots `[0, 0.18, 0.54, 0.91, NaN]`, multiplicities `[5, 3, 2, 1, 5]`) and `fuzz21932` (degree 5, knots `[0, NaN, 1.14]`, multiplicities `[6, 2, 6]`); a third NaN-knot case (`fuzz08941`) returned a record instead.
- **How found:** `--fuzz 30000` on main `16f04231`; gdb backtraces of both aborts.
- **Confirmed:** yes (SIGABRT from glibc's heap checks in the reference build, both cases).
- **Severity:** crash (heap corruption).
- **Rust port:** the `primitive.api_curves` FFI refuses a non-finite knot for the canonical curve. The Rust side returns an error status on `fuzz08941`. Number follows the open PRs' UB-237; renumber on merge if taken.

#### UB-250. `N_IgesClampSrf` writes past its new knot vector when a knot is NaN
- **Location:** `source/SMLib/src/NL_Iges.cpp:817` `N_IgesClampSrf`: the span searches (`:980-1003` for v, as for u) assume finite, nondecreasing knots and bounds inside them; the knot copy (`:1096-1103`) then writes more knots than the vector it sized holds. Reached from `N_Iges128Srf` (`:757`), e.g. `SmBSplineSurface::CreateCanonical` for a surface of revolution.
- **What happens:** For v knots `360 360 360 NaN NaN 540 540 540` (degree 2) clamped to `[360, 540]`, every comparison with NaN is false, so the searches give `span1 = 4`, `mult1 = 3`, `span2 = 5`, `mult2 = 3`. The new vector gets `span2 - span1 - mult2 + 2q + 1 = 3`, i.e. 4 knots (32 bytes); the copy writes 3 + 0 + 3 = 6 of them, the last two past the end in the `vb[1]` loop at `:1102-1103`.
- **Trigger:** a STEP surface of revolution with its sweep domain from 360 degrees (tangency-labs/solidcore#428), whose B-spline gets the knots above (tangency-labs/solidcore#427).
- **How found:** AddressSanitizer on SolidCraft's STEP import through SMLib (#427: heap-buffer-overflow, WRITE of size 8 at `NL_Iges.cpp:1103`, 0 bytes after a 32-byte region); the counts above were traced by hand at `c9c5979`.
- **Confirmed:** yes (ASan, reported in #427).
- **Severity:** crash/UB (heap-buffer-overflow).
- **Fix:** tangency-labs/smlib@0a7e06d refuses, before clamping, knot vectors that aren't finite (within `1e300`) and nondecreasing, and bounds that aren't increasing and strictly inside the knots.
- **Rust port:** `nurbs::iges::clamp_surface` indexes with checks, so it refuses this case (`InvalidInput`, "index outside the arrays") instead of writing past the end (test `clamp_surface_refuses_the_427_knots`). The fork's guard is not ported: it also refuses knots the reference build handles, e.g. infinite or `f64::MAX` planes (`primitive.api_primitives` fixtures `plane_surface_len_inf`, `rv_plane_l_max`), and a NaN interior knot between clamped ends, which upstream leaves in place on its "already clamped" path without writing anything.

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
- **Fuzz sweep trigger (2026-10-20):** `smgu_BestFitPlaneToPoints` (`SmGeomUtility.cpp:4478`, call at `:4555`) on a point set with coordinates near `±1e300`. The scatter matrix overflows to `inf`, `inf - inf` is NaN, `IsSymmetric` (`SmMatrix.cpp:222`) fails, `EigenSystemJacobiSolver` returns `SM_ERR` before writing `sV`, and `Compute3x3EigenVectors` returns `SM_SUCCESS` with the uninitialised small-matrix storage as eigenvectors. Cases: `geom.point_set` `fuzz00533` (`/pts/9/2 = 1e+300`, `/pts/0/1 = 1e+300`) and `fuzz01968` (`fuzz-out/verify2000/geom.point_set/`), 12 more in `fuzz-out/geom/geom.point_set/`. The C++ reports success while Rust returns `SM_ERR` (1001), so the ignored status is confirmed in a C++ run. The uninitialised values were not observed directly.

#### UB-22. `SmMatrix::LUDecompose` uses a fixed stack buffer it does not check
- **Location:** `SmMatrix.cpp:662`, `SmMatrix::LUDecompose`.
- **What happens:** It scales with a stack `double[MAX_MATRIX_SIZE]` (1000). Only `SolveLinearSystem` (`:843`) checks the size.
- **Trigger:** `LUDecompose` called directly on a 1001x1001 matrix.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Uses a `Vec`. The harness limits sizes.

#### UB-204. `SmMatrix(rows, cols)` leaves its inline storage uninitialised, and `SmLocalSolveNd` solves with it when an evaluator fails
- **Location:** `SmMatrix.h:39` (constructor, "leaves mat values uninitialized"; inline storage `m_adMat[36]` at `:30`), `SmLocalSolveNd.cpp:115` (`SmMatrix sJacobian(lNumFun, lNumFun)`, used by `SolveIt` whatever the evaluator did), `SmCurve.cpp:16680` (and `:18834`): `SmCCIntersectENFO::Evaluate` (def. `:16589`) calls `SmCCTangentIntENFO::StaticEvaluate` and ignores its status.
- **What happens:** When the curves are within 0.1 of each other and an evaluator fails before it writes `F` and the Jacobian (here: a tangent of length below `SM_EFF_ZERO`), `SolveIt` still solves `J * step = -F` with the matrix it never filled. In a 6x6-or-smaller system that is the stale stack slot of an earlier solve, so the step, and the solution it leads to, depends on what ran before.
- **Trigger:** `SM_SO_3D_SIGNED_DIRECTED_MINIMIZE` (9), tolerance 0.5, two near-identical cubic Hermite curves whose end tangent `d2` is zero, the first interval `[0.9424, 1.0934]` (a Newton step clamped to `t = 1`).
- **How found:** generated difftest case `gen21507` (`curve.curve_p9`, `--seeds 30000`), traced with gdb: zeroing the four Jacobian doubles at the second solve makes C++ and Rust agree bit for bit.
- **Confirmed:** yes (that case).
- **Severity:** crash/UB (an uninitialised read; the result is nondeterministic across call histories).
- **Rust port:** `Matrix::new` zeroes its storage, so the system is singular, the solve reports `BadJacobian` and the previous solution is kept: deterministic, and the safe reading. The curve_p9 generator does not hand this shape to the C++ reference (`reads_uninitialised_jacobian`). See `plan/findings.md`, "curve.curve_p9: zero-end-tangent Hermite pair, loose tolerance".

#### UB-240. `N_InitRealMatrix` leaves the matrix type indeterminate
- **Location:** `source/SMLib/src/NL_Math.cpp:3617` `N_InitRealMatrix` (sets `n`, `m`, `RM` and `bw`, not `mtp`); read by `N_RealMatrixIsNULL` (`:4707`, through `N_GetRealMatrixData`), which every `N_CheckMemRealMatrix` starts with.
- **What happens:** A matrix initialised to NULL has an indeterminate `NL_MATRIXTYPE`, and the null test loads it. The value does not decide the result (only `n`, `m`, `RM` and `bw` do), but loading an indeterminate enum is undefined behaviour.
- **Trigger:** a fillet that joins a tangent chain: `SmBSplineCurve::CreateByJoining` -> `DegreeElevate` -> `N_BezGetDegreeElevationMatrix` (UBSan reports the load of an indeterminate `NL_MATRIXTYPE`; tangency-labs/solidcore#370).
- **How found:** UBSan run of the SolidCraft C API (tangency-labs/smlib `capi/`).
- **Confirmed:** yes (UBSan, in the fork's C API tests).
- **Severity:** UB (indeterminate load); no observable effect in the reference build.
- **Fix:** tangency-labs/smlib a05e288 sets it to `NL_MT_FULL`, the type the next `N_SetRealMatrix` would give it anyway.
- **Rust port:** `RealMatrix::null` has type `MatrixType::Full` (the `Default`), as the fix (test `null_matrix_has_a_defined_full_type`).

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
- **Fuzz sweep trigger (2026-10-20):** `ul`/`ur` can be in range and the call still writes out of bounds. For `U = [0,0,0,0,1e-300,6,9,9,9,9]` (cubic, 6 control points), `N_CrvExtractCrvSeg(0, 1e-300)`: `N_BasisFindSpan` skips the too-short span, `ul = 0` lands in span 4, `spr - mlr < spl`, and upstream writes one knot past the `m + 1` it allocated for `UQ`. Found in the `nlib.srf_adv_p6` sweep. Its group may already list it, in which case keep one bullet. The Rust port returns `InvalidInput` (`crv_tool/p1.rs`, test `extract_seg_over_skipped_tiny_span_is_invalid_input`). Not confirmed.

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
- **Fuzz sweep trigger (2026-10-20):** a curve control point coordinate or weight of `1e300` / `1e-300`, or a start derivative `-1e300` (`nlib.srf_adv_p4` `fuzz00511`, `fuzz00921`, `fuzz01441`, `fuzz01762`): C++ returned a curve, Rust the angle error. Also an end angle exactly equal to `eps` or NaN leaves the knot uninitialised. The `nlib.srf_adv_p4` FFI now replays `N_VectorsAngle` in upstream's operation order and rejects the first-pass inputs that hit either case (`approx_angles_ok`). Confirmed: no (no crash).
- **Fuzz sweep trigger (2026-10-20):** a surface weight of `0.0` or `-0.0` (`nlib.srf_adv_p2` `fuzz01654`, `fuzz00394`): a zero weight makes Euclidean points infinite, the reflection plane cannot be normalised and C++ reads past its scratch rows and returns a surface; Rust errors. The `nlib.srf_adv_p1`/`p2`/`p3` FFIs require surface weights all `NOW` or all positive (`p4`: curve weights). Confirmed: no (no crash).

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

#### UB-225. `N_ApproxNurbsWithNonRatCrv` overflows its `NL_INDEX` sample count and writes past its parameter array
- **Location:** `source/SMLib/src/NL_CrvApprox.cpp:2698` `N_ApproxNurbsWithNonRatCrv`: `nu[i] = (NL_INDEX)ceil(f1 * gro)` at `:2778`-`2779` (`f1 = (1/tol)^exp`, `gro = max(deg, sqrt(max|C''| / 8))`), `mu += nu[i]` at `:2780`, `N_AllocReal1dArray(mu)` at `:2785`. Reached from `SmBSplineCurveNLib.cpp:524` `SmBSplineCurve::ApproximateRationalCurve` (def. `:497`) and so from `sm_CreateNonRationalCurve` (`SmBSplineSurfaceNLib.cpp:2906`) / `SmBSplineSurface::CreateCornerBlend`.
- **What happens:** For a tiny tolerance or a huge second derivative, `del` is beyond `2^63` (or infinite), so the cast is undefined and `mu` wraps. The parameter array is allocated from the wrapped sum and the sampling loop writes past it. The heap is corrupted; the process dies at once or several calls later.
- **Trigger:** `sm_CreateNonRationalCurve` (approximation of a rational curve) on `TestFiles/pt_TestFiles/curve0.smc` with `tol = 1e-300` (segfault); a rational cubic with one control point coordinate `1e300` (glibc `realloc(): invalid next size` in the same process); `double free or corruption (out)` 50 cases later in the same process.
- **How found:** crash in the C++ reference run (`difftest --fuzz 2000`, `nurbs.b_spline_surface_n_lib_p1` `fuzz00080`, `fuzz00651`, `fuzz00902`; files under `n2000/nurbs.b_spline_surface_n_lib_p1/`).
- **Confirmed:** yes.
- **Severity:** crash/UB (heap buffer overflow).
- **Rust port:** `nurbs::crv_approx` bounds the sample count. The `nurbs.b_spline_surface_n_lib_p1` FFI applies the `nlib.crv_approx` `NonRat` bounds to a rational curve (`APPROX_MIN_TOL = 1e-7 <= tol <= 1`, homogeneous data within `MAX_COORD = 1e6`); the `nlib.crv_approx` FFI already had them.

#### UB-234. `N_CrvOffsetPtSampling` casts an unchecked sample count to `NL_INDEX`
- **Location:** `source/SMLib/src/NL_CrvOffset.cpp:1380` `nu[i] = (NL_INDEX)del;` and `:1381` `mu += nu[i];` in `N_CrvOffsetPtSampling` (def. `:1217`), then `:1386` `N_AllocReal1dArray( mu, &SL )`. The same pattern is at `:297`/`:298` (`N_CrvOffsetFuncVariableDir`) and `:674`/`:675` (`N_CrvOffsetFuncConstantDir`).
- **What happens:** `del = ceil(f1 * gro)` comes from `N_CrvOffsetGetMax2ndDeriv` per Bezier segment. A NaN or infinite `Muu`, or a `del` above `LONG_MAX`, makes the double-to-integer conversion undefined. The per-segment counts are summed without an overflow check and the sum sizes the parameter and point arrays.
- **Trigger:** A rational curve with a non-positive weight (a pole: `Muu` non-finite or huge), or a tolerance near `0`. Fuzz cases `nlib.crv_offset` fuzz00366 / fuzz01037 / fuzz01299 (`/crv/cpts/k/3 = -1.0`) and fuzz00468 (`/tol = 1e+300`, `/deg = 4294967296`, control points at `+-1.8e308`).
- **How found:** read during porting (the `nlib.crv_offset` shim recomputes the count before the call); fuzz sweep (`difftest --fuzz 2000`) inputs reach it.
- **Confirmed:** no. The shim returns `SC_ERR_ARG` for a non-finite count or a total above `MAX_SAMPLES` (4000), and the FFI rejects non-positive weights.
- **Severity:** crash/UB.
- **Rust port:** Sums with `checked_add` and returns `InvalidInput` for an unrepresentable count or more than `MAX_OFFSET_SAMPLES` (16 384) samples.

#### UB-235. NLib fitting and approximation overflow `NL_INDEX` on huge caller counts
- **Location:** `NL_CrvApprox.cpp:2274` `N_ApproxCircArcWithCrvData` (`nc + p + 1` knots, `np = 4 * nc` at `:2444`, `:2526`); `NL_CrvFit.cpp:2000` `N_FitCrvCalcKnotVector`, `:2458` `N_FitCalcKnotVectorEndDerivs`, `:2562` `N_FitCalcKnotVectorDeriv`, `:4023` `N_FitCalcKnotVectorCrvApprox` (`n + p + 1`), `:2273` `N_FitCrvInterpGivenParams` (`n + p + 1` compared with the knot count), `:13320` `N_FitCrvWeightedLstSqKnots` (`mc + n`), `:14621` `N_FitCrvApproxClosedConditions` (`p + 1`, `k` derivatives allocated).
- **What happens:** Index sums and products on caller counts are signed `long` arithmetic with no overflow check. Near `LONG_MAX` they overflow (undefined behaviour), and a wrapped sum then sizes `N_Malloc` or indexes the knot array.
- **Trigger:** `N_ApproxCircArcWithCrvData(..., deg = 4, nc = 9223372036854775807, ...)` (fuzz `nlib.crv_approx` fuzz00475). `N_FitCrvInterp` with `p = 9223372036854775807` (`nlib.crv_fit_p1` fuzz00023). The bad argument has to come from the calling code. No file input reaches these calls.
- **How found:** fuzz sweep (Rust overflow panics at the mirrored lines).
- **Confirmed:** no. The FFI caps `nc` (`MAX_NC` = 400), counts and degrees.
- **Severity:** crash/UB.
- **Rust port:** `fit_arrays::add` / `mul` return `InvalidInput` on overflow. Where the sum is only compared with the knot count, an overflowing sum counts as a mismatch (`NL_INP_ERR`). `alloc` reserves fallibly and returns `NL_MEM_ERR`, as a failed `N_Malloc` does.

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
- **What happens:** `(ULONG)((x - min) / len)` is UB for negative values. On x86-64 g++ it wraps, so a point below the box lands in the top voxel after the clamp (the comment says the low end). NaN maps high; `+inf` maps to 0. aarch64 g++ saturates instead (negative and NaN map to 0, `+inf` to the top voxel), so the voxel depends on the platform. A box with a zero-length axis gives NaN or `+-inf` for every point, so `SmHealData::Cache_CoinVertices` on a flat or linear vertex set puts the vertices into different voxels on the two platforms.
- **Trigger:** Add a point with `x < box.min.x`, or any point to a box with a zero-length axis.
- **How found:** read during porting; generated difftest cases; on aarch64, difftest failures of `geom.hash_table` and `heal.heal_data_p2` on macOS arm64.
- **Confirmed:** yes. The port emulates the x86-64 conversion and matches. The aarch64 result was confirmed on macOS arm64 with GCC 13.4 (2026-10-17).
- **Severity:** crash/UB (wrong voxel in practice).
- **Rust port:** Emulates the x86-64 result (`ulong_from_f64`).
- **Platform dependence:** confirmed 2026-10-18 on arm64 (Ubuntu 24.04 GCC 13.3 and Apple clang). AArch64 `fcvtzu` saturates, so negatives and NaN map to voxel 0 and `+inf` maps high. A degenerate (single-point) vertex box makes the quotient `0/0 = NaN` on every axis: x86-64 files the point in voxel `(cnt-1, cnt-1, cnt-1)` plus 3 low-side neighbours, arm64 in voxel `(0,0,0)` with no neighbours. The arm64 reference build therefore disagrees with x86-64 in `geom.hash_table` and `heal.heal_data_p1`-`p3`, and a full `geom.hash_table` run on arm64 segfaults in `FindCoincidentPairs` (see `plan/findings.md`, General).

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
- **Fuzz sweep trigger (2026-10-20):** a ray-fire topology solve with ray radius `1e300` (`topology.shell` `fuzz-out/n2000/topology.shell/fuzz00814.json`, `/ray/radius = 1e+300`): the C++ returned normally, the Rust port refuses (`may_contain_answer`: ray/box intersection failed). Confirmed: no.

#### UB-86. Topology edge solvers dereference a curve's missing edge
- **Location:** `SmTopologySolver.cpp:557`/`:582` `SmTLSPointEdge::SolveIt`, `:1079`/`:1101` `SmTLSCurveEdge::SolveIt`, `:1359`/`:1380` `SmTLSEdgeEdge::SolveIt`.
- **What happens:** `pCurve->GetEdge()` is only checked by an `SM_ASSERT_MSG` (compiled out); with no shape (`m_pShape == NULL`) a curve that defines no edge is dereferenced as `NULL->GetInterval()`.
- **Trigger:** Any of these local solvers called on a free curve (no owning edge) outside a shape solve.
- **How found:** read during porting (`intersect.topology_solver_p1`).
- **Confirmed:** no (the difftest guard passes only curves that own an edge).
- **Severity:** crash/UB (bad argument from calling code).
- **Rust port:** Returns `InvalidInput`.

#### UB-221. `SmTreeNode` neighbour and side searches trust the node links
- **Location:** `source/SMLib/src/SmTree.cpp:3027`-`3236` `SmTreeNode::GetLeftNeighbors` / `GetRightNeighbors` / `GetBotNeighbors` / `GetTopNeighbors` (recurse up through `m_pParent` and into `m_pParent->m_pChild1/2`), `:3244`-`3510` `GetRightSide` and the other `Get*Side` searches (recurse into `m_pChild1` and `m_pChild2`), `:2749` `PropagateToParents` (reads the sibling `m_pParent->m_pChild1/2` and its `m_pData`).
- **What happens:** No null, consistency or cycle checks. A node with one child, a parent that does not list the node, a child whose parent pointer names another node, or cyclic parent links give a NULL dereference (SIGSEGV) or unbounded recursion (stack overflow).
- **Trigger:** A subdivision tree whose node links disagree, e.g. the `geom.tree` subdivision program with one node duplicated (`/nodes element 5 duplicated`, n2000 `fuzz00296`) or a child link cleared on one side only (`/nodes/28/c1 = -1`, `fuzz01815`). Trees built by upstream's own subdivision always have two distinct children and matching parent links.
- **How found:** crash in the C++ reference run (`difftest --unit geom.tree --fuzz 2000`: 14 cases, 11 SIGSEGV, 3 stack overflows; `fuzz-out/n2000/geom.tree/`).
- **Confirmed:** yes.
- **Severity:** crash/UB. Not file-reachable (needs hand-built or corrupted `SmTreeNode` links).
- **Rust port:** Arena ids; a missing node is `InvalidInput`. The `geom.tree` FFI (`well_formed`, `crates/solidcore-ffi/src/units/geom_tree.rs:152`) now rejects such link tables before calling C++.

#### UB-222. Size constructors and setters write through a failed `smos_Calloc`
- **Location:** `source/SMLib/src/SmMatrix.cpp:20` `SmMatrix(ULONG, ULONG)` and `:143` `SmMatrix::SetSize` (`smos_Calloc(1, sizeof(double)*rows*cols)` unchecked, product can wrap, then the row-pointer loop writes `m_ppdM[ii]`); `SmSolutionArray.cpp:1076` `SmSolutionArray::SetSize` (calloc result unchecked, then `smos_MemCpy`/writes; the `nNewMaxSize *= 2` loop never ends for a size above `2^63`, and `nNewMaxSize * sizeof(SmSolution)` can wrap) and `:1448` `InsertAt` (`nIndex + nCount` unchecked, then `smos_MemMove` and slot writes); `source/SMLib/inc/SmLocalSolve1d.h:159` `SmIterQueue1d(ULONG)` (`SetSize` refuses or fails softly, `Add` then writes `m_vArray[0]` of an empty array).
- **What happens:** For a caller-chosen size that cannot be allocated the object keeps a NULL buffer (or a too-small one after a wrapped product) and the next statement writes through it.
- **Trigger:** `SmMatrix(2^31, 5)`, `SmSolutionArray::SetSize(2^31)`, `InsertAt(2^32, s, 2^32)`, `SmIterQueue1d(2^31)` then `Add`. Upstream's own callers use small sizes (`SmIterQueue1d(8)`, matrix sizes from curve/surface degrees).
- **How found:** generated difftest case (fuzz sweep: the Rust ports aborted on these sizes in `geom.matrix`, `geom.solution_array`, `geom.local_solve1d`, `blend.fillet_custom_solver`; upstream source then read).
- **Confirmed:** no. The existing FFI limits (`geom.matrix` `MAX_DIM` 64, `geom.solution_array` `MAX_SIZE` 4096, `geom.local_solve1d` queue size 1..=1024) kept these sizes from the C++ side.
- **Severity:** crash/UB (NULL write, size wrap) and hang (`SetSize` above `2^63`). Not file-reachable.
- **Rust port:** `Matrix::try_new` / `Matrix::set_size`, `SolutionArray::try_set_size` / `try_insert_at` / `try_with_capacity`, `IterQueue1d::try_new` return `InvalidInput` with the object unchanged.

#### UB-238. Vertex/vertex and edge/edge topology solvers dereference a `NULL` object in a release `SM_ASSERT`
- **Location:** `SmTopologySolver.cpp:369`-`371` `SmTLSVertexVertex::SolveIt` (def. `:365`), `:1343`-`1345` `SmTLSEdgeEdge::SolveIt` (def. `:1339`); the release `SM_ASSERT(a)` is `if(a) {}` (`source/SMLib/inc/SmMessages.h:714`), so its expression is still evaluated.
- **What happens:** `SM_ASSERT(pObj1->IsKindOf(..))` and `SM_ASSERT(pObj2->IsKindOf(..))` run before any `NULL` check. With either object `NULL` the call dereferences it (SIGSEGV), although the solvers they forward to (`SmTLSPointVertex::SolveIt`, `SmTLSCurveEdge::SolveIt`) check the second object with `NER`.
- **Trigger:** `SolveIt(pV, NULL)` or `SolveIt(NULL, pV)` on an `SmTLSVertexVertex`, or the same with edge curves on an `SmTLSEdgeEdge`, in either swap order.
- **How found:** generated difftest case (`intersect.topology_solver_p1 --fuzz 30000`: `/calls/K/1 = -1` and `/calls/K/0 = -1` mutations, kinds vertex/vertex and edge/edge).
- **Confirmed:** yes. SIGSEGV in `SmTLSVertexVertex::SolveIt` in the reference build (19 fuzz cases); the FFI guard now refuses a `NULL` object for both solvers.
- **Severity:** crash/UB (bad argument from calling code).
- **Rust port:** Returns an error instead of dereferencing (`null_deref` for a `NULL` first object, the forwarded solvers' `NULL` check for the second). The difftest guard keeps these calls away from the C++ side.

### Curves

#### UB-46. NURB-less curves are dereferenced
- **Location:**
  - `SmBSplineCurve.cpp:43`: `SmBSplineCurve(lDim)` has a NULL NURB, which nearly every query dereferences (`GetOrCreateGwNurbPointer` asserts in debug only).
  - `SmHyperbola.cpp:422` `MakeNurb` (`:441`, `:470`) and `SmParabola.cpp:419` (`:435`): when `smgu_LineLineClosestPoint` fails, `MakeNurb` fails. The constructor ignores this, and `GlobalPointSolveSTEP` then dereferences the missing NURB in the curve cache.
- **What happens:** NULL dereference.
- **Trigger:** A hyperbola or parabola with a degenerate interval (parallel end tangents) or zero focal distance, then `GlobalPointSolveSTEP`. Also the `SmHyperbola` constructor (`SmHyperbola.cpp:66`, `SE(MakeNurb())`) on extreme but finite input, e.g. a semi axis of 1e300 or a centre at 9.2e18 with unit axes: the NURB is left NULL or with non-finite control points, and building a surface of revolution / extrusion / offset over it segfaults.
- **How found:** read during porting; the constructor trigger by `--fuzz 200` on the hyperbola generators of the `surface.surf_of_revolution_p1`, `surf_of_revolution_p3` and `offset_surface` runners (2026-10-21, 21 segfaults).
- **Confirmed:** yes for the constructor trigger (segfaults in the C++ reference run); the shims now refuse a hyperbola whose built NURB is NULL or has a non-finite control point.
- **Severity:** crash/UB.
- **Rust port:** Returns `SM_ERR`.
- **Fuzz sweep trigger (2026-10-20):** `SmParabola` with a non-finite-after-unitize frame (`/a/y/2 = 1e+300`, `curve.parabola` fuzz `fuzz01135`, op `gps_step`): `MakeNurb` fails, `GlobalPointSolveSTEP` -> `AdjustSTEPInterval` keeps the domain, then `SmCurve::GlobalPointSolve` builds the length cache through `SmBSplineCurve::EvaluatePoint`, which dereferences `GetOrCreateGwNurbPointer()` (`SmBSplineCurve.cpp:3300`). SIGSEGV in the C++ reference run. Change **Confirmed:** to yes (crash in the C++ reference run, fuzz sweep); the shim's earlier refusal did not cover this input, the FFI now replays `Adjust`/`MakeNurb` first (`crates/solidcore-ffi/src/units/curve_parabola.rs:131`).
- **Fuzz sweep trigger (2026-10-20):** an `SmCircle` / conic whose axis' squared length overflows (`1e300` in one component) builds no NURB; `SmBSplineCurve::EvaluatePoint` (`SmBSplineCurve.cpp:3300`) then dereferences it from `SmBrepConstructor::EndLoop` (`query.loop` `fuzz-out/n2000/query.loop/fuzz01585.json`, `/edges/0/curve/y/1 = 1e+300`) and from `SmEdgeDefinition::GetEndPosition` / `SmVertexDefinition::Resolve` (`topology.proto_topology_p1` `fuzz-out/n2000/topology.proto_topology_p1/fuzz00350.json`, `/objects/1/curve/conic/x/1 = -1e+300`). Both SIGSEGV in the C++ run, so Confirmed becomes yes for this trigger. Guards: `crates/solidcore-ffi/src/units/query_loop.rs:389`, `crates/solidcore-ffi/src/units/topology_proto_topology_p1.rs:557`.

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
- **Trigger:** Two identical B-spline segments, one reversed, with `bDeleteDuplicates = TRUE`. The duplicate test compares a candidate with the ends of the chain built so far (start, end, and the mid point of the end link), not with another curve: a line from a chain's end back to its start through its last link's mid point is deleted although it repeats no curve, while a square with a repeated side deletes nothing.
- **How found:** read during porting; reached through `SmPrimitiveCreation::CreateTaperExtrude` (`SmPrimitiveCreation.cpp:3672`, `dTol = 2 * SM_ZONE_TOL_3D`) in the PR #250 review.
- **Confirmed:** yes (PR #250): segfaults for two identical lines, a line and its reverse (gdb: `SmCurve::FixChainCurvesList` <- `CreateTaperExtrude`), two identical arcs, and lines `(0,0,0)-(3e-5,0,0)`, `(3e-5,0,0)-(1,0,0)`, `(1,0,0)-(0,0,0)`; with the first line `5e-5` long it runs. Squares with a repeated or reversed side run under each normal. One native process per input.
- **Severity:** crash/UB (use after free, double free).
- **Rust port:** Removes the duplicate once. A link choice equal to the deleted curve is `InvalidInput`.
- **Second call site:** `CreateTaperExtrude`'s end cap at the offset end (`iEndCaps` 2 or 3, `SmPrimitiveCreation.cpp:4368`, the chaining at `:4382`) chains a copy (`sOffs`) of the trimmed offset curves with `bDeleteDuplicates = TRUE`; a curve deleted there stays in `sTrimCrvs`, and the cleanup at `:4477` deletes it again. Native abort `double free or corruption (!prev)` (SIGABRT from the `CreateTaperExtrude` frame) for lines `(0,0,0)-(5e-5,0,0)`, `(5e-5,0,0)-(100,0,0)`, `(100,0,0)-(0,0,0)`, height 100, draft -24.9, normal -Z, caps 3 (a generated case); not with caps 0 or 1, nor with the first line 1 long. That the deletion is this call is from source reading (the only deleting step reached only with that cap).
- **Guard:** the `primitive.primitive_creation_p2` FFI repeats both of the taper's chaining passes (the second, reversed one whenever a normal is supplied) on the curves' end and mid points and refuses an input it would delete from; with the offset end cap it also refuses a closed chain narrower than `2 * SM_ZONE_TOL_3D`, the shape of every probe that crashed there.

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
- **Confirmed:** the circle failed-solve precondition is confirmed by the compiled native preflight in PR #213; the undefined constructor execution and the other subcases have not been run.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.
- **Fuzz sweep trigger (2026-10-20):** `SmPolarConversion::ReadFromDB` (`SmEllipse.cpp:1488`) accepts an exactness table shorter than `angles - 1` (here empty, `curve.ellipse_p1` fuzz `fuzz00284`: `/build/read_db/record/exact emptied`); `ConvertToNURBSParameter` then reads `m_vExactConversion[ii-1]` past the array (`SmEllipse.cpp:555`). SIGSEGV in the C++ reference run: change **Confirmed:** to yes and add **[PSIRT]** to the title (reachable from a crafted file). Also refine the `ConvertTo2D` bullet: the quadratic is at `SmEllipse.cpp:1732`; with one root only `dThetaYRad` (`:1735`) reads the uninitialised `adSols[1]`, and it is unused; only a zero-root solve makes the used `adSols[0]` uninitialised (fuzz `fuzz01889`, `/build/read_db/record/head/11 = -1e-300`, degenerate radius).

- **PR #213 witness:** `cylindrical_full_span_preflight` in `fixtures/primitive.primitive_creation_p1/primitives.json` (length 2, inner radius 1, outer radius 3, angles 0..360, identity frame). The shim calls the same three-point-circle solve before `CreateCylindricalBox`; a non-success result rejects the fixture before `SmCircle` reads the unset radius. This is a rejected fixture, not a parity pass or a claim that the constructor fails gracefully.

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
- **Fuzz sweep trigger (2026-10-20):** the `AssertValid` dereference is at `SmCompositeCurve.cpp:676` (`:595` is the def.). `CreateByJoining` also fails, leaving `rpNewBSplineCurve` NULL, for connected segments when a segment's knot span vanishes in the shift `u + (lastJoinedKnot - firstSegmentKnot)` (knot `1e-300` after a joined end at `~4.24`, `curve.composite_curve_p1` fuzz `fuzz00767`: `/comp/curves/1/bspline/oob = true`, `/comp/curves/2/bspline/u/4 = 1e-300`, op `approx`, `tol = 0.01`): `CreateCanonical` rejects the repeated knot. SIGSEGV in the C++ reference run: change **Confirmed:** to yes. FFI guard: `check_approx`, `crates/solidcore-ffi/src/units/curve_composite_curve_p1.rs:338`.

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

#### UB-224. `SmCompositeCurve(dim, curves, closed, senses, gaps)` indexes senses and gaps by the curve count
- **Location:** `source/SMLib/src/SmCompositeCurve.cpp:1073` (`(*cpSenses)[i]`), `:1090` (`(*cpGaps)[m_lNumSegments-1]`), `:1097` (`(*cpGaps)[i]`); constructor def. `:1050`.
- **What happens:** The optional sense and gap arrays are read once per curve with unchecked `operator[]` (the size check is an `SM_ASSERT_BREAK_MSG`, compiled out). A shorter array is read past its end; a closed composite with no curves and a gap array reads `cpGaps[ULONG_MAX]`.
- **Trigger:** Two curves with a one-entry `cpSenses` or `cpGaps`; `bIsClosed = TRUE`, no curves, `cpGaps` non-NULL.
- **How found:** generated difftest case (fuzz sweep: mutated sense/gap arrays of a different length than the curve list; the runners now pass exactly one entry per curve).
- **Confirmed:** no. The harness keeps these inputs away from the C++ side.
- **Severity:** crash/UB.
- **Rust port:** `CompositeCurve::new` returns `InvalidInput` for fewer senses or gaps than curves and for gaps of a closed composite without curves.

#### UB-233. `SmBSplineCurve::CreateByJoining` reads `cpOptSenses` past a shorter array
- **Location:** `source/SMLib/src/SmBSplineCurve.cpp:1188` `SmBSplineCurve::CreateByJoining`: the size match is only `SM_ASSERT`ed (`:1199`); `(*cpOptSenses)[ii]` is read for every segment at `:1271`.
- **What happens:** With fewer senses than segments, `SmTArray::operator[]` reads outside the array. The release build returned a joined curve.
- **Trigger:** `CreateByJoining` with two segments and an empty senses array.
- **How found:** generated difftest case (`difftest --fuzz 2000`, `nurbs.b_spline_curve_p2` `fuzz00360`, `fuzz01002`: C++ returned a curve, Rust an error).
- **Confirmed:** no (no crash; the out-of-range read is from source inspection).
- **Severity:** crash/UB (out-of-bounds read).
- **Rust port:** Returns `InvalidInput`. The `nurbs.b_spline_curve_p2` runner keeps fewer senses than segments from the C++ side.

#### UB-236. `sm_CreateBezSegments` copies control points past the polygon of a curve whose end knots are not clamped
- **Location:** `source/SMLib/src/SmNurbsCrv.cpp:478` `sm_CreateBezSegments` (end-of-span loop `while( ie LT m AND UP[ie] EQ UP[ie+1] ) ie++` at `:551`, copy `InitCP(Pw[ie-p+i],&NQw[i])` at `:596`). Reached from `SmCurveCache.cpp:1291` (the curve cache `DropPoint` builds) during `SmBrepConstructor::EndLoop`.
- **What happens:** The span walk assumes `p + 1` equal end knots. With an unclamped end, `ie` stops short of the clamp and the next-segment copy reads `Pw[ie - p + i]` past the last control point.
- **Trigger:** An edge curve of a loop being built whose last `p + 1` knots are not all equal (`query.loop` fuzz: one tail knot of a 3D edge B-spline set to `0.0`, `1.0` or `-0.0`).
- **How found:** crash in the C++ reference run (`difftest --unit query.loop --fuzz 2000`: `fuzz-out/n2000/query.loop/fuzz00183.json`, `fuzz00293.json`, `fuzz00592.json`).
- **Confirmed:** yes (SIGSEGV, 3 cases).
- **Severity:** crash/UB (out-of-bounds read).
- **Rust port:** not reached; the `query.loop` FFI wrapper rejects an edge curve whose last `p + 1` knots differ (`crates/solidcore-ffi/src/units/query_loop.rs:400`).

#### UB-238. `SetIntersectionCurveThroughPoint` builds the new UV trim curves from the address of a pointer slot
- **Location:** `source/SMLib/src/SmBSplineCurveNLib.cpp:2293` (`new SmBSplineCurve(rUVCurve[ii]->GetDim(), (gw_CURVE *)&sNewUVCrv[ii])`); `sNewUVCrv` is a `SmTArray<NL_CURVE *>` (`:1650`) filled with `N_AllocCrv` results in the knot-inserting cases (`:2018-2025`).
- **What happens:** `&sNewUVCrv[ii]` is the address of the array element holding the `NL_CURVE*`, not the curve, so the copy constructor reads a pointer and the bytes after it as a `gw_CURVE` and dereferences garbage (segfault in the reference build). The 3D curve's own copy two lines up uses `&sNewCrv` correctly.
- **Trigger:** any call with at least one UV trim curve whose parameter falls in a knot-inserting case (1, 4 or 8: a new knot before the start, inside a span away from its ends, or after the end), e.g. a piecewise C0 cubic over `[0,3]` in the plane `z = 0` with one UV curve on a bilinear plane and `dNewKnot = 0.5`.
- **How found:** crash in the C++ reference run (adversarial / edge-case fixture, `nurbs.b_spline_curve_n_lib`).
- **Confirmed:** yes (SIGSEGV running the case through the shim; the knot-moving cases 2, 3, 5, 6, 7 with the same curves run and match the port).
- **Severity:** crash/UB.
- **Rust port:** swaps in the inserted UV curves themselves (the evident intent). The `nurbs.b_spline_curve_n_lib` FFI guard only lets UV curves through when every span and node the parameter could select takes a knot-moving case.

#### UB-251. `SmBSplineCurve::DeformToEndPointTargets` reads the knot array that `N_CrvInsertKnot` freed
- **Location:** `source/SMLib/src/SmBSplineCurveNLib.cpp:3138`-`3139` (`pCurrCrv_Knts[nCurrCrv_KnotIndex-nCurrCrv_Degree-1]`), in the knot-insertion loop `:3120`-`3155`; the in-place `N_CrvInsertKnot(pCurrCrv, ..., pCurrCrv, ...)` at `:3144` frees and reallocates the knot array, and `pCurrCrv_Knts` / `nCurrCrv_KnotIndex` are only refreshed after the loop (`:3160`).
- **What happens:** When iteration `ii` 0 or 1 has inserted a knot, the `ii == 2` branch compares distances against knots read from the freed array (use after free). glibc malloc keeps the freed payload, and the stale index lands on the same knot value the new array holds, so Linux gets the intended choice. macOS malloc reuses the block, the `<` test picks the other knot (`ii++`), a different knot is inserted, and the deformed curve changes (control points off by 1e-3 to 1e-1 relative). Distinct from UB-47 (`:3038`, ignored `DegreeElevate` status) and UB-56 (`SmCurve.cpp:21563`).
- **Trigger:** `DeformToEndPointTargets` where region 1 needs a knot inserted and region 3 holds exactly one interior knot (`nCurrCrv_InReg3_KntCnt == 1`), e.g. `curve.curve_p6` gen0161 / gen0953 / gen2879 and `nurbs.b_spline_curve_n_lib` gen0695 / gen0720 / gen0880 / gen0932 / gen1279 / gen1374 / gen1482 (`op = "deform"`).
- **How found:** generated difftest cases failing on macOS arm64 only (the Rust result equals the Linux C++ result on both platforms); then ASan.
- **Confirmed:** yes. ASan on macOS arm64 (harness built with `-fsanitize=address,undefined`) reports `heap-use-after-free` READ at `SmBSplineCurveNLib.cpp:3138`, freed by `N_FreeReal1dArray` <- `N_FreeCrv` <- `N_CrvInsertKnot`, for curve_p6 gen0161 / gen0953 and n_lib gen0695 / gen1279. Latent on x86-64 Linux (benign under glibc malloc). GitHub issue #449.
- **Severity:** crash/UB (use after free; wrong result on non-glibc allocators).
- **Rust port:** Reads the live knot vector after each insertion (`crates/solidcore/src/nurbs/b_spline_curve_n_lib.rs`, `curr.knots()` in the insertion loop), which equals the glibc result.
- **Difftest follow-up (#449, PR #793):** On macOS arm64 only, the ten generated deform cases listed above use the existing `skipped(platform)` outcome, never a pass. The reason cites the upstream use-after-free at `SmBSplineCurveNLib.cpp:3127-3149` and that Rust matches the Linux reference. Linux still runs all ten comparisons; neither the Rust kernel nor the C++ reference is changed.

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
- **Fuzz sweep trigger (2026-10-20):** two more NULL-NURB paths. `SmBSplineSurface::Transform` (`SmBSplineSurface.cpp:5299`, `N_SrfTransform(GetOrCreateGwNurbPointer(), ..)`) on a bounded plane with an inverted domain (`u0 = 2147483648 > u1 = 5`; `v1 = -0.0 < v0 = 4`), and `SmPlane::IntersectWithPlane` (`SmPlane.cpp:2191`-`2192`, `:2274`, `:2284`) of non-parallel planes, which boxes a bounded plane with an empty or inverted domain through `SmBSplineSurface::CalculateBoundingBox` (`SmBSplineSurface.cpp:489`-`490` reads `->net`). `surface.plane_p2` `fuzz00149`, `fuzz00896`, `fuzz01393`, `fuzz01926`. Confirmed: yes (segfault). The `surface.plane_p2` FFI rejects both (`nurb_less`, `bounded_nurb_less`).
- **Fuzz sweep trigger (2026-10-20):** the surface cache of a plane with an inverted domain passes the NULL `m_pNurb` to `sm_CreateBezierPatchBlock` (`SmSurfaceCache.cpp:5042`, dereferenced at `:4795`) (`intersect.surface_intersector_p5` `fuzz-out/n2000/intersect.surface_intersector_p5/fuzz00950.json`, `/a/domain/2 = -1.0`; SIGSEGV, confirmed yes). Guards: `crates/solidcore-ffi/src/units/intersect_surface_intersector_p5.rs:238`, `crates/solidcore-ffi/src/units/intersect_surface_drop_lib_p2.rs:387`.

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
- **Rust port:** `primitive.primitive_creation_p3` (PR #233) implements `CreateSkinFromFaces`; its `BSplineSurface::create_skinned_surface` call returns `InvalidInput` for no cross sections. The p3 native shim and checked FFI reject that exact case before C++; a direct probe of the linked reference reconfirmed SIGSEGV. The legacy API remains blocked earlier at B-rep construction.

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
- **What happens:** `CreateIsoCircleFromPoint` ignores the statuses of `STEPInversion` and `ConvertUVFromSTEPToNURBS`; when the generator drop in `SmSurfOfRevolution::STEPInversion` finds no solution the height of `sStepUV` is never written and is used anyway. `GetHeight` also ignores `EvaluatePoint`'s status when the conversion fails (no polar curve), but that part is defined: the `SmPoint3d` outputs keep their `SM_UNDEF_DOUBLE` member initialisers, so two failures give height `0` (PR #226, issue #277).
- **Trigger:** `CreateIsoCircleFromPoint` with a reference point just outside the height interval of a steep bounded cone and a tolerance below the in-plane gap the clamp leaves; `CreateIsoCircleFromPoint` on `SmCone::CreateCanonical(O, 1, 5e-11 deg, NULL)` (no `bSwapUV`) at `(-1, 1e-13, 0.7)`, where the polar conversion fails and `sNurbUV.y` (a defaulted `SmVector2d`) is used unset; `GetHeight` of the cone of UB-81.
- **How found:** read during porting (`surface.cone_p1`).
- **Confirmed:** no. The shim replays the inversion and refuses those inputs.
- **Severity:** crash/UB (uninitialised read).
- **Rust port:** `CreateIsoCircleFromPoint` returns the error. `Cone::height` mirrors `GetHeight` (failed evaluations keep `Vec3::UNDEF`), and the `surface.cone_p2` difftest compares it, and the offset and trim ops that use it, on cones without a polar curve (`fixtures/surface.cone_p2/no_polar_curve.json`).

#### UB-88. `SmCone` STEP-to-NURB conversion dereferences a missing polar curve
- **Location:** `source/SMLib/src/SmEllipse.cpp:489`, `SmPolarConversion::ConvertToNURBSParameter` (def. `:467`), reached from `SmCone.cpp:1808` `SmCone::ConvertUVFromSTEPToNURBS` and `:1682` `SmCone::CreateIsoCircleFromPoint`.
- **What happens:** `m_cpCurve->GetSTEPInterval()` is called without the `NER(m_cpCurve)` check that `ConvertToPolarParameter` has. A cone whose polar converter could not be set up has no curve; `SmSurfOfRevolution::MakeNurbWithSweepParams` ignores `RebuildPolarConverter`'s status, so `CreateCanonical` still succeeds.
- **Trigger:** `new SmCylinder(O, X, Y, -1.8, 0, 360, 4.3, FALSE, FALSE, FALSE)` (a negative radius) and `MakeNurb()`, which succeeds without polar curve, then `ConvertUVFromSTEPToNURBS`.
- **How found:** read during porting (`surface.cone_p1`).
- **Confirmed:** no. The shim refuses the op for a cone without polar curve.
- **Severity:** crash/UB (null dereference).
- **Rust port:** Returns `InvalidInput`.

#### UB-171. `SmOffsetSurface::EvaluatePlainOrSimple` writes past `aDerivatives` with `bOnlyUpperHalf`
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:1442` (and the `Duuv` / `Duvv` writes after it), `SmOffsetSurface::EvaluatePlainOrSimple` (def. `:960`), reached from `Evaluate` and `EvaluateSimple`.
- **What happens:** With `bOnlyUpperHalf` the mixed derivatives are computed whenever the larger order allows them, but are written at their indices in the full `(hu+1)(hv+1)` layout. With more `v` than `u` derivatives they land past the caller's array (`hu = 0, hv = 2`: `Duv` at index 4 of a 3-entry array; also `0, 3` and `1, 3`).
- **Trigger:** `Evaluate(uv, 0, 2, TRUE, TRUE, TRUE /*bOnlyUpperHalf*/, a /*3 entries*/)` on an offset with a non-zero distance.
- **How found:** read during porting (`surface.offset_surface`).
- **Confirmed:** no. The harness rejects these orders.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` for a write past the array.

#### UB-172. `SmPlane::Evaluate` wraps its loop bound for `bOnlyUpperHalf` with more `u` than `v` derivatives
- **Location:** `source/SMLib/src/SmPlane.cpp:1536`, `SmPlane::Evaluate`: `ULONG lHighestVIndex = bOnlyUpperHalf ? lHighestVDeriv-k : lHighestVDeriv`.
- **What happens:** For `k > lHighestVDeriv` the unsigned subtraction wraps, and the zeroing loop runs far past the caller's array. A zero-distance `SmOffsetSurface` over a plane passes such requests straight to it.
- **Trigger:** `SmPlane::Evaluate(uv, 2, 0, .., TRUE /*bOnlyUpperHalf*/, a /*3 entries*/)`.
- **How found:** read during porting (`surface.offset_surface`).
- **Confirmed:** no. The harness rejects these orders over a plane.
- **Severity:** crash/UB.
- **Rust port:** The plane evaluation returns `InvalidInput` for an upper half with more `u` than `v` derivatives.

#### UB-173. The base-class box of an extended `SmOffsetSurface` recurses without bound
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:337`, `SmOffsetSurface::CalculateBoundingBox` (def. `:315`) calling `SmSurface::CalculateBoundingBox`, whose `smsurf_GetSurfaceCache` builds the offset's surface cache.
- **What happens:** A box domain not inside the base domain runs the base-class body, which builds the offset's surface cache. Its nodes cover the offset's natural domain, which is the extended surface's once there is one; each node outside the base domain runs the base-class body again, until the stack overflows. This is the pattern of UB-84.
- **Trigger:** An offset allowing extension: `TrimWithDomain` to a part of the base domain, an `EvaluatePoint` outside it (stores an extension), then `CalculateBoundingBox` over the original domain. Also `IsPlanar` (the `SmSurface` body, a box over `GetNaturalUVDomain()`) on an offset of an offset that allows extension, after the outer's `STEPInversion` made the inner store an extension.
- **How found:** crash in the C++ reference run (a generated `surface.offset_surface` case, 30,000-seed run); the `IsPlanar` path in a 100,000-seed run.
- **Confirmed:** yes (stack overflow).
- **Severity:** crash/UB (stack exhaustion).
- **Rust port:** Refuses the polar box of the base-class body for such an offset; the surface cache it would build waits on `surface.surface_p6`. The harness keeps these boxes away from the C++ side.

#### UB-174. `sm_ExtendSurface` can retry `N_SrfEvalPtDerivsUnbounded` with an uninitialised flag
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:517`, `sm_ExtendSurface` (`NL_FLAG gflg` uninitialised); `NL_SrfAdv.cpp` `N_SrfEvalPtDerivsUnbounded`.
- **What happens:** The first call copies the surface into `esur` before `N_SrfIsClosedSmooth`. When that test fails before it stores the closed directions (a knot or evaluation error), the retry finds `esur` set and reads the uninitialised `gflg` to decide between wrapping and extending.
- **Trigger:** An offset extension over a surface whose closedness test fails with a knot or evaluation error.
- **How found:** read during porting (`surface.offset_surface`).
- **Confirmed:** no.
- **Severity:** crash/UB (uninitialised read).
- **Rust port:** The extension state stays empty after such errors (`nlib.srf_adv_p1`), so the retry starts over.

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

#### UB-198. `SmApiCreateTorus` (surface overload) crashes on a non-finite origin, an infinite radius or overflowing radii
- **Location:** `source/SM_API/src/SmApiPrimitives.cpp:618` `SmApiCreateTorus` (`SmSurface*&`) -> `source/SMLib/src/SmTorus.cpp:172` `SmTorus::CreateCanonical` (only rejects radii not above `SM_EFF_ZERO`, so NaN and negative radii fail cleanly).
- **What happens:** Segfault while the torus and its NURB are built.
- **Trigger:** (only with both radii above `SM_EFF_ZERO`; otherwise `CreateCanonical` returns `SM_ERR` before it reads the origin, which the #160 review confirmed for NaN/inf origins and `+inf` radii paired with a zero, negative or NaN radius) origin `(NaN, 0, 0)` or `(0, inf, 0)` with radii `3, 1`; major or minor radius `+inf`; radii whose sum overflows (`1e308, 1e308`; `DBL_MAX, 1e293`); minor radius `DBL_MAX` with major `5` (major `DBL_MAX` with minor `1` and `DBL_MAX/2, DBL_MAX/2` are fine).
- **How found:** crash in the C++ reference run (primitive.api_primitives fixtures).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** `Torus::create_canonical` returns without panicking; the difftest guard (`solidcore_ffi::units::primitive_api_primitives::guard`) keeps these inputs from the C++ side, and only when both radii are above `SM_EFF_ZERO` (the C++ gate), so the cleanly rejected inputs are compared.

#### UB-199. `SmApiCreatePlane` (plane overload) with a NaN length or width indexes outside the NURB knot arrays
- **Location:** `source/SM_API/src/SmApiPrimitives.cpp:181` `SmApiCreatePlane` (`SmPlane*&`): `SmExtent2d::SetMinMax(0, 0, dLength, dWidth)` keeps NaN (its `min > max` test is false), then `SmPlane::AdjustSTEPUVDomain` -> `SmPlane::MakeNurb` (`SmPlane.cpp:2993`) -> `SmBSplineSurface::CreateCanonical` -> `N_Iges128Srf` clamps to NaN bounds.
- **What happens:** The clamp's span search finds no span for NaN and reads past the knot arrays: a NaN length segfaults; a NaN width returns `SM_SUCCESS` with a plane built from out-of-bounds data.
- **Trigger:** `SmApiCreatePlane((0,0,0), NaN, 3, SmPlane*&)` (segfault); `SmApiCreatePlane((0,0,0), 3, NaN, SmPlane*&)` (success).
- **How found:** crash in the C++ reference run (primitive.api_primitives fixtures `plane_surface_len_nan`, `plane_surface_wid_nan`, removed since).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** the bounded clamp returns `InvalidInput` (`nurbs::iges::clamp`), so `create_plane_surface` fails with `SM_ERR`; the difftest guard keeps NaN sizes from the C++ side.
#### UB-153. `SmCone::CreateOffsetSurface` dereferences an unchecked `CreateCanonical` result
- **Location:** `source/SMLib/src/SmCone.cpp:624`-`641`, the apex-truncation branch of `SmCone::CreateOffsetSurface` (def. `:533`).
- **What happens:** When one new radius falls below the tolerance, the truncated cone is made by `SmCone::CreateCanonical`, whose status is ignored; `pOffCone->GetSTEPUVDomain()` and the following calls then dereference it. The new height is `dNewRadius * dHeight / dOldRadius`: an old radius of `0` gives an infinite or NaN height, for which the bounded constructor's NURB construction recurses without bound; a failed `MakeNurb` leaves `pOffCone` `NULL`.
- **Trigger:** A bounded cone with bottom radius `0` and a negative top radius, offset so that the new bottom radius is at least the tolerance and the new top radius below it.
- **How found:** read during porting (`surface.cone_p2`, PR #153).
- **Confirmed:** no. The shim tries the same `CreateCanonical` first (with the same formulas) and refuses the input when it fails or the height is not finite.
- **Severity:** crash/UB (null dereference, unbounded recursion).
- **Rust port:** `create_canonical`'s error is returned; no recursion.

#### UB-154. `sm_GetEllipseTrimIntervals` reads the parameter of a failed line/plane intersection
- **Location:** `source/SMLib/src/SmCone.cpp:5399`-`5405` and `:5463`-`5477`, `sm_GetEllipseTrimIntervals` (def. `:5282`, file static).
- **What happens:** The statuses of `smgu_LinePlaneIntersect` for the two seam lines are ignored and `dParam[0]`, `dParam[1]` (uninitialised) are then compared and used. (The status of `ConvertDomainFromNURBSToSTEP` at `:5325` is ignored too, but that is defined: a failed corner conversion, e.g. of a cone without a polar curve, leaves the default-constructed domain, and a reversed one the NURB domain.)
- **Trigger:** A seam line of the trimmed cone shorter than `1e-12` (`ZERO_VEC_ER`, `SmGeomUtility.cpp:694`), e.g. a zero-height or reversed trim domain, or parallel to the ellipse plane within `SM_EFF_ZERO` (not reached in practice: the cone/cone ellipse planes are at least `SM_EFF_ZERO_SQRT / 2` from the seams, and the cone/plane path tests equally long, less steep generators first, `SmCone.cpp:3716`, `:3735`).
- **How found:** read during porting (`surface.cone_p2`, PR #153); reached through `IntersectWithCone` by the `surface.cone_p3` fuzz (issue #276: fuzz00656 `/domains/2 = -1e300`, fuzz09540 `/domains/5 = 1e300`, `differs=2` with `cpp=None rust=status_1007`).
- **Confirmed:** yes, with the C++ build: an instrumented replay shows both seam vectors are exactly `(0, 0, 0)` on the two fuzz inputs (the reversed domain evaluates both ends of each seam to the same point), so both `smgu_LinePlaneIntersect` calls fail and `dParam` is read uninitialised. Guards: the `surface.cone_p2` shim (`trim_defined`) and the `surface.cone_p3` shim (`harness/cpp/shim/surface_cone_p3.cpp`: `seam_lines_defined`, `cone_ellipse_seams_undefined`, `plane_ellipse_seams_undefined`) replay the upstream calls in the same order, with the same status checks, up to the two seam intersections, and refuse an input iff one fails (fixtures `fixtures/surface.cone_p3/ub154.json`: 7 rejections, 13 neighbours just inside validity compared).
- **Severity:** UB (uninitialised read).
- **Rust port:** The line/plane errors are returned. The ignored domain conversion is mirrored (fixture `surface.cone_p3/fuzz_trim_domain.json`).

#### UB-202. `SmCircularCrossSectionFSG::CreateSurfaceFromCurves` indexes past its cross sections in the second pass of a ruled or revolved surface
- **Location:** `source/SMLib/src/SmFilletStandardSolver.cpp:3233` (`SmCircularCrossSectionFSG::CreateSurfaceFromCurves`): the first pass skips to the last knot for sweeps and revolves (`:3435`), so it stores three cross sections; the second pass (`m_bApproximateArcs` with differing unique knot counts) indexes `sCrossSections[lKnotIndex]` with `lKnotIndex` the knot index (`:3504`, `:3613`, `:3636`, `:3756`), which is at least 3 for the last knot.
- **What happens:** `SmTArray::operator[]` reads (and on the replacement / mirror paths writes) past the array's size: an uninitialised or stale pointer is used as a cross section and deleted.
- **Trigger:** A degree-1 straight centre line with a curved rail (the ruled-surface test only rejects rails that are lines in another direction), approximated arcs (the default) whose sections at the first, second and last knots get different unique knot counts (different arc angles, or a point section where the rails meet).
- **How found:** read during porting.
- **Confirmed:** no. The `blend.fillet_standard_solver_p1` shim detects these inputs with a dry run of the first pass and does not run them.
- **Severity:** crash/UB (out-of-bounds read and write).
- **Rust port:** `SolidError::InvalidInput` at the out-of-range index.

#### UB-203. `SmBlendCurveCrossSectionFSG::CreateSurfaceFromCurves` skins incompatible data as synchronised
- **Location:** `source/SMLib/src/SmFilletStandardSolver.cpp:4458` (`SmBlendCurveCrossSectionFSG::CreateSurfaceFromCurves`: `CreateSkinnedSurface(..., bSyncronized = TRUE, ...)`), through `SmBSplineSurfaceNLib.cpp` `SmBSplineSurface::CreateSkinnedSurface` into `NL_SrfAdv.cpp:13262` `N_CreateSkinSrfBoundaryContinuity` with `cmp = NL_YES`, which reads the cross sections' and the derivative curves' control points with the first cross section's count.
- **What happens:** Two inputs are not compatible although the call says so: (a) a point cross section (`CreatePointCurve` where the rails meet within the tolerance, `:4132`) among the blend curves; (b) with `bNeedG1Continuity`, the cross-boundary derivative surfaces: the strips are ruled surfaces linear in u, `N_AdjustDerivSrf` (`NL_SrfAdv.cpp:13316`) swaps and extends them successfully (the blend curves are Bezier), and the first skinning attempt takes the 2-point derivative curve along the ruling as the cross-boundary data of a degree 3, 5 or 7 section. The skinning then reads past the derivative / point curve's control points.
- **Trigger:** (b) every `G1` blend-curve fillet that is skinned (not ruled or revolved), for example the `blend.fillet_standard_solver_p1` difftest case `gen2400` of a 6000-seed run (seed `7089734759426696442`: continuity 3, blend scale 2.5, `tol = 0.05`, straight non-parallel rails): the C++ surface holds denormals (`1.344e-321`, `3.686e-310`) and `6.8e61`.
- **How found:** generated difftest case.
- **Confirmed:** yes for (b) (the case above); no for (a).
- **Severity:** crash/UB (out-of-bounds read).
- **Rust port:** The bounds-checked skinning fails the first attempt (`InvalidInput`) and `CreateSkinnedSurface` retries as upstream does after a failure. The shim rejects both inputs (a dry run of the knot insertion and the ruled / revolved tests).

#### UB-204. `SmFilletSolver::GetSurface` recurses without bound through the geometry set for the surfaces
- **Location:** `source/SMLib/src/SmFilletSolver.cpp:136` (`SmFilletSolver::GetSurface`: `m_pFGForSurfaces->GetOffsetSurface(lRailIndex)`) and `source/SMLib/src/SmFilletGeom.cpp:9679` (`SmFilletGeom::GetOffsetSurface`: `m_pFilletSolver->GetSurface(lRailIndex)` when `m_pOffSurfs[lRailIndex]` is `NULL`).
- **What happens:** With a geometry set by `SetFilletGeomForSurfaces` whose own offset surface for the rail is `NULL` and whose `m_pFilletSolver` is the same solver, each function calls the other until the stack overflows.
- **Trigger:** `SetFilletGeomForSurfaces(pFG)` (`SmFilletGeom.cpp:11827`, `SmFilletCorner.cpp:7686-7693`, `:8092-8096`) on a geometry of the solver that never got its offset copies (`FinishConstruction` not run, or `SetOffsetSurface(i, NULL)` after it), then any `GetSurface(i)`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB (stack exhaustion).
- **Rust port:** `FilletSolver::surface` returns `Unported` while a geometry is set for the surfaces (`plan/findings.md`, "blend.fillet_solver_p1: the geometry for the surfaces"); no Rust caller sets one yet.

#### UB-186. `SmCircularCrossSectionFSG::CreateSurfaceFromCurves` `G1` skin retry interpolates cross sections of different control point counts
- **Location:** `source/SMLib/src/NL_SrfAdv.cpp:15096` (`N_CreateSkinSrfBoundaryContinuity`: `N_CrvGetCPts( cur[i], &nc, &Cw ); N_CopyCPt( Cw[j], &Qw[i] )` for `j` up to the first section's `nc`), reached from `SmFilletStandardSolver.cpp:3917` (`CreateSkinnedSurface` with the `G1` derivative surfaces) through `SmBSplineSurface::CreateSkinnedSurface`'s retry without the knot vector.
- **What happens:** The second pass of approximated arcs inserts `lMaxNumUniqueKnots - 2` evenly spaced knots into an unequal-radius section whose arc already has that many unique knots (`:3654`): each inserted knot is within one ulp of an existing one (`5/11` against `5 * (1/11)`). `N_CrvsMakeCompatibleKnotTol(cur, k, 0)` makes the sections and derivative curves compatible, but `N_CrvElevateDegree` then gives the section with the near-duplicate knots fewer control points than the others (39 against 41), and the interpolation loop reads one control point past that section's array.
- **Trigger:** `G1`, approximated arcs, a second pass, a section with `|r1 - r2| >= tol` whose first-pass arc has the maximum unique knot count, and a first skinning attempt that fails (the usual `N_AdjustDerivSrf` extension failure). Example: the circular case `rev_twist_n6p3_t0.05_m0_c1_dr1e-06_g1_s10000.0` of the `blend.fillet_standard_solver_p1` review (free-form centre, rails twisting from 0.4 to 2.6 rad, radius 1e4 with a 1e-6 relative mismatch, `tol = 0.01`, complement).
- **How found:** adversarial difftest case during review of PR #168; the counts were confirmed by calling `N_CrvsMakeCompatibleKnotTol` and `N_CrvElevateDegree` on the exact curves in a C++ build.
- **Confirmed:** yes (the C++ build reads `Pw[39]` of a 39-point section).
- **Severity:** crash/UB (out-of-bounds read).
- **Rust port:** The bounds-checked interpolation fails the attempt (`InvalidInput`) and `CreateSkinnedSurface` retries without derivative surfaces. The `blend.fillet_standard_solver_p1` shim does not detect these inputs yet; its generated cases do not reach them.

#### UB-187. `SmOffsetSurface` extension of a plane with a reversed domain reads a half-built NLib surface
- **Location:** `source/SMLib/src/SmPlane.cpp:3013-3016` (`SmPlane::MakeNurb`: the knots are the domain's corners as stored), `source/SMLib/src/SmOffsetSurface.cpp:527-533` (`sm_ExtendSurface`: a failed `N_SrfEvalPtDerivsUnbounded` is retried with the same `esur`), `source/SMLib/src/NL_SrfAdv.cpp:5470` (`N_SrfGetParameterBounds` on the partly extended `esur`).
- **What happens:** `SmPlane(o, x, y, scale, SmExtent2d(u0, v0, u1, v1))` keeps a reversed domain, so `MakeNurb` gives NLib a decreasing knot vector. Evaluating an offset of it outside that domain extends it: the first `N_SrfEvalPtDerivsUnbounded` call fails part-way through the extension, and the retry reads the `esur` it left behind.
- **Trigger:** `SmConstantRadiusFS::LoadJacobian` (via `SmSurface::Evaluate1stDerivatives` on its `SmOffsetSurface`) on a plane whose domain is `[0, 1] x [0, -1e10]`. Reversed by `1e9` or less, the calls return.
- **How found:** `difftest --fuzz 3000` of `blend.fillet_standard_solver_p1` (PR #168 review fixes), case built on a two-plane `cr_jac` fixture.
- **Confirmed:** yes (SIGSEGV in `N_SrfGetParameterBounds` under gdb).
- **Severity:** crash/UB (read of a freed or uninitialised surface).
- **Rust port:** no crash. The `blend.fillet_standard_solver_p1` FFI validator rejects a reversed plane domain for the solvers' base surfaces, so the case is not compared.
#### UB-179. `SmCone::ConvertUVFromSTEPToNURBS` dereferences a missing polar curve
- **Location:** `source/SMLib/src/SmCone.cpp:1808`, called by `IntersectWithCone` at `:2966-2967`; `source/SMLib/src/SmEllipse.cpp:491` (`SmPolarConversion::ConvertToNURBSParameter`). Construction ignores `RebuildPolarConverter`'s status at `SmSurfOfRevolution.cpp:1033`.
- **What happens:** A degenerate sweep can leave the polar converter without a circle even though `MakeNurb` returns success. STEP-to-NURBS conversion unconditionally reads `m_cpCurve->GetSTEPInterval()` and segfaults. `HavePolarConversion()` alone does not identify this: a converter with a circle but no exact table can still perform a defined numerical conversion.
- **Trigger:** Issue #207: equal-radius cones, first origin `(0,0,0)`, X `(1,0,0)`, Y `(0,-1e-300,0)`, radius `1.5628`, height `1.10780`, inside-out; second origin `(0,0.5539,0.5539)`, X `(1,0,0)`, Y `(0,0,1)`, same radius/height; full sweeps, tolerance `1e-7`, domains `[0,0,1,1, 0,0.125,1,0.875]`. Unitizing the tiny Y axis uses the `(0,0,1)` fallback, and the sweep iso curves used to build the converter are degenerate. Either operand can have the missing converter; direct cylinder intersection also reaches this call.
- **How found:** crash in the C++ reference fuzz run reported in issue #207; traced through cone construction and the virtual parameter-conversion call.
- **Confirmed:** yes (issue #207 reports native exit 139; guarded native probes confirm successful construction with a null polar curve and stop at the conversion without executing the null dereference).
- **Severity:** crash/UB.
- **Rust port:** Missing polar state returns an error. The differential adapter rejects only an attempted STEP-to-NURBS conversion with a null polar curve; self-intersection and disjoint early returns stay callable.

#### UB-226. `SmBSplineSurface::CreateCornerBlend` segfaults on a boundary curve with a `1e300` control point
- **Location:** `source/SMLib/src/SmBSplineSurfaceNLib.cpp:3008` `SmBSplineSurface::CreateCornerBlend`. The failing dereference was not located.
- **What happens:** Segfault. The curves are non-rational, so the `sm_CreateNonRationalCurve` approximation (UB-225) is not involved.
- **Trigger:** four non-rational cubic boundary curves (bilinear blend, `tol = 1e-3`, angle `0.05`) with one control point x coordinate set to `1e300`.
- **How found:** crash in the C++ reference run (`difftest --fuzz 2000`, `nurbs.b_spline_surface_n_lib_p2` `fuzz01761`; the C++ ran in process, so the harness reported a Rust-side signal 11).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Returns an error. The `nurbs.b_spline_surface_n_lib_p2` FFI rejects Euclidean control point coordinates above `MAX_COORD = 1e6`.

#### UB-227. Surface-of-revolution and extrusion constructors evaluate a generator that has no NURB
- **Location:** `source/SMLib/src/SmSurfOfRevolution.cpp:134` (constructor, def. `:94`) and `source/SMLib/src/SmSurfOfExtrusion.cpp:70` / `:170` (constructors, def. `:37`, `:138`) call `pGenCurve->EvaluatePoint` with the status only `SE`d; `source/SMLib/src/SmBSplineCurve.cpp:3299`-`3300` `SmBSplineCurve::EvaluatePoint` dereferences `GetOrCreateGwNurbPointer()->knt` without a NULL check.
- **What happens:** When the generator's `MakeNurb` fails, the constructor dereferences its NULL NURB. Seen for: a circle or ellipse whose axis has an overflowing squared length (`Unitize` makes it a zero vector, WR-159, so the circle gets no NURB); a hyperbola or parabola whose centre or size is near `1e300` (`MakeNurb`'s tangent intersection `smgu_LineLineClosestPoint` fails). Spheres and tori go through the revolution constructor. UB-82 lists the unchecked statuses of the extrusion constructor; this is the crash they lead to.
- **Trigger:** a sphere or torus whose generator circle has an axis component `-1e300` (`surface.sphere` `fuzz00721`, `surface.torus` `fuzz00120`, `fuzz01665`, `fuzz01864`); a surface of revolution or extrusion with a circle axis component `±1e300` (`surface.surf_of_revolution_p1` `fuzz00820`, `surface.surf_of_extrusion_p1` `fuzz01152`); an extrusion of a hyperbola with centre x `-1e300` or semi axis `1e300`, or of a parabola with focal distance `1e300` (`surface.surf_of_extrusion_p1` `fuzz00827`, `fuzz01228`, `fuzz00834`).
- **How found:** crash in the C++ reference run (`difftest --fuzz 2000`; 37 segfaults across `surface.sphere`, `surface.torus`, `surface.surf_of_revolution_p1`/`p2` and `surface.surf_of_extrusion_p1`/`p2`, files under `n2000/<unit>/`).
- **Confirmed:** yes.
- **Severity:** crash/UB (null dereference).
- **Rust port:** Returns the generator's error. The FFIs of `surface.sphere`, `surface.torus`, `surface.surf_of_revolution_p1` and `surface.surf_of_extrusion_p1` reject axes whose length is not finite; the extrusion FFI also bounds a hyperbola's or parabola's centre and sizes by `MAX_CONIC = 1e12` (a heuristic; the exact failure threshold was not derived).

#### UB-228. `SmSurfOfExtrusion::IntersectWithPlane` dereferences a failed plane projection of the generator
- **Location:** `source/SMLib/src/SmSurfOfExtrusion.cpp:2245` (`m_pGenCurve->CreatePlaneProjection(..., m_vExtrusionVec, pProjCurve)`, status ignored) and `:2276` (`pProjCurve->CalculateBoundingBox`) in `SmSurfOfExtrusion::IntersectWithPlane` (def. `:1870`).
- **What happens:** `N_CrvProjectOntoPlane` fails for an extrusion vector whose squared length overflows, `pProjCurve` stays NULL and is dereferenced.
- **Trigger:** a canonical extrusion of a B-spline with vector `(1e300, 0, 0)`, or of a parabola with vector `(1e-9, 0, 1e300)`, intersected with a non-parallel plane (`IntersectWithPlane` directly, or through the global surface intersection).
- **How found:** crash in the C++ reference run (`difftest --fuzz 2000`, `surface.surf_of_extrusion_p2` `fuzz00363`, `fuzz01373`).
- **Confirmed:** yes.
- **Severity:** crash/UB (null dereference).
- **Rust port:** Returns the projection error. The `surface.surf_of_extrusion_p2` FFI rejects plane intersections (`Plane`, `Global` with a plane) of a surface whose extrusion vector length is not finite.

#### UB-229. `SmSurfOfRevolution::CreateOffsetSurface` dereferences a failed axis segment
- **Location:** `source/SMLib/src/SmSurfOfRevolution.cpp:1413` (`SmLine::CreateLineSegment(..., origin, origin + dHeight * zAxis, pAxisLine)`, status ignored) and `:1417` (`*pAxisLine` passed to `GlobalCurveIntersect`) in `SmSurfOfRevolution::CreateOffsetSurface` (def. `:1244`).
- **What happens:** `dHeight` is 1.5 times the offset generator's box size. With an axis point near `2^63`, `origin + dHeight * zAxis == origin`, the segment is degenerate, `CreateLineSegment` fails and the NULL line is dereferenced.
- **Trigger:** `CreateOffsetSurface` of a revolution whose axis point has a coordinate `9223372036854775807` or `-9223372036854775808` (generator of unit size).
- **How found:** crash in the C++ reference run (`difftest --fuzz 2000`, `surface.surf_of_revolution_p2` `fuzz01805`, `fuzz01926`).
- **Confirmed:** yes.
- **Severity:** crash/UB (null dereference).
- **Rust port:** Returns the segment error. The `surface.surf_of_revolution_p2` FFI rejects `Offset` with an axis point coordinate above `MAX_AXIS_POINT = 1e12` (a heuristic bound; the exact threshold depends on the generator size).

#### UB-230. An unbounded `SmCone` whose apex height overflows recurses without bound while making its NURB
- **Location:** `source/SMLib/src/SmCone.cpp:499` `SmCone::GetApex` (apex at height `-bot / tan(semi)`); `SmSurfOfRevolution.cpp:957` `SmSurfOfRevolution::MakeNurbWithSweepParams`; `SmLine.cpp:1679` (`SmLine::MakeNurb` asks the owner's `GetGenDirParamExtent`); `SmSurfOfRevolution.cpp:799` `GetGenDirParamExtent`.
- **What happens:** The apex parameter is NaN when the height overflows, so the generator's STEP interval is NaN. `MakeNurbWithSweepParams` re-adjusts the generator line (`AdjustSTEPInterval` never sees two equal NaN intervals), the line's `MakeNurb` asks the cone's `GetGenDirParamExtent`, which makes the cone's NURB again. The stack overflows. Related to UB-70 (same owner detour).
- **Trigger:** `SmCone::CreateCanonical` (semi-angle overload, no height) with `bot = 1e300`, semi `6e-11` deg (build, `fuzz01891`) or `1e-9` deg (iso circle, `fuzz00916`), or `bot = -1e300` with semi `5.7e-11` / `6e-11` deg (iso circle `fuzz00168`, inversion `fuzz00494`), all `surface.cone_p1`. The guard also covers the bot/top/height overload with `h <= 0` (the top radius is then the semi-angle of an unbounded cone); that path did not crash in the run.
- **How found:** crash in the C++ reference run (`difftest --fuzz 2000`, `surface.cone_p1`: `fatal runtime error: stack overflow`).
- **Confirmed:** yes.
- **Severity:** crash/UB (stack overflow).
- **Rust port:** No recursion. The `surface.cone_p1` FFI rejects unbounded cones whose `bot / tan(semi)` is not finite, except a zero semi-angle (`|semi| <= SM_EFF_ZERO`, a cylinder, which `GetApex` treats as having no apex).

#### UB-231. `SmSurface::CreateNaturalUVTrimCurves` segfaults on a domain `1e-300` wide
- **Location:** `source/SMLib/src/SmSurface.cpp:9048` `SmSurface::CreateNaturalUVTrimCurves` (iso curves from `CreateIsoParametricCurve`, degenerate sides replaced by `SmLine::CreateLineSegment`). The failing dereference was not located.
- **What happens:** Segfault.
- **Trigger:** a plane `(o = 0, n = (0,0,1))` with domain `[0, 0] x [1e-300, 1e-9]`, side `0`, remove degenerate curves `TRUE`; `TestFiles/pt_TestFiles/offset_nurb.sms` with domain `[-1e-300, 0] x [0, 10]`, side `0`.
- **How found:** crash in the C++ reference run (`difftest --fuzz 2000`, `surface.surface_p6` `fuzz01069`, `fuzz01289`; still crashing in `verify3/surface.surface_p6/`).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Not guarded yet: the `surface.surface_p6` FFI still passes these inputs (open).

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

#### UB-158. `SmCurveClassification::InsertAt(0, ..)` into a non-empty classification never ends its shift loop
- **Location:** `SmCurveClass.cpp:5349` (`InsertAt`: `for(i1=m_lSize-1,ii=i1-nCount;ii>=nIndex;ii--,i1--)`).
- **What happens:** `ii` and `nIndex` are `ULONG`. With `nIndex == 0` the test `ii >= 0` is always true: after `ii = 0` the loop decrements `ii` to `ULONG_MAX` and copies `m_pData[ULONG_MAX]` into `m_pData[0]`, then keeps walking down memory.
- **Trigger:** `InsertAt(0, interval)` on a classification with at least one interval.
- **How found:** read during porting.
- **Confirmed:** no (the harness keeps this call away from the C++).
- **Severity:** crash/UB.
- **Rust port:** `CurveClassification::insert_at` returns `InvalidInput`.

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

#### UB-129. `SmSurface::DropCurve` uses results whose status it ignores
- **Location:** `source/SMLib/src/SmSurfaceDropLib.cpp:4590`-`4591` (`SmBSplineCurve::CreateLineSegment` of the pole line, status ignored, then `pUVTrimCurve->EditParameterization` on the pointer that stays `NULL` on a failure), `:5489` (`p3dCurve->EvaluatePoint(sInterval.GetMax(), sCurveEnd)`, status ignored, then `sCurveEnd` read), `:4232` (`SmTol::GetScaledZero(cr3dCurve)`, `SmTol.cpp:1400`, reads the point of an ignored `EvaluatePoint`) and `:4233` (`cr3dCurve.IsDegenerate(.., &crInterval)`, `SmCurve.cpp:14395`-`14427`: it reads the derivatives of `Evaluate` calls whose statuses it ignores; its asserted precondition, `crInterval` inside the natural interval, is compiled out and only matters for a curve kind whose `Evaluate` fails outside that interval).
- **What happens:** A failing callee leaves the output unset (a `NULL` curve pointer, an uninitialised point or derivative array) and the next statement uses it.
- **Trigger:** `CreateLineSegment` fails only for coincident ends (a natural domain shorter than `SM_EFF_ZERO` in the pole direction, which the domain checks above nearly exclude); the `EvaluatePoint`s fail only for curve kinds whose point evaluation can fail (`SmBSplineCurve`, `SmLine` and `SmCircle` clamp or extend instead); the `:4233` `IsDegenerate` case needs a curve kind whose `Evaluate` fails outside its natural interval, and `crInterval` reaching outside it (none in the harness: `SmBSplineCurve::Evaluate`/`EvaluatePoint` clamp, `SmBSplineCurve.cpp:3056`, `:3307`, and `SmLine` and `SmCircle` evaluate anywhere, so those intervals are not undefined).
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB (needs a bad argument or a failing evaluator).
- **Rust port:** `Surface::drop_curve_base` propagates those errors. The FFI rejects nothing for the `IsDegenerate` case (it accepts intervals outside the curve's range, zero-length and inverted ones; probed).

#### UB-155. Volume discontinuity W planes index the V parameter array
- **Location:** `source/SMLib/src/SmCurveClass.cpp:10071-10072`, `SmCurveClassification::Intersect3DDiscontinuities` (volume).
- **What happens:** The W-plane loop takes its coordinates from `m_sParamsV[kk]` while iterating `m_sParamsW`. Different V/W values put the planes at the wrong W positions; more W than V parameters reads past V's array.
- **Trigger:** unequal V/W discontinuity parameter arrays.
- **How found:** read during porting `intersect.curve_class_p8` (PR #152).
- **Confirmed:** no; native fixtures use equal V/W lists and never execute the out-of-bounds case.
- **Severity:** crash/UB; wrong result for in-bounds unequal values.
- **Rust port:** retains V coordinates, with checked indexing returning an error. The plane-intersection path remains dependency-deferred.

#### UB-175. `~SmProtoTopologyManager` frees proto edges before the proto vertices that still list them [PSIRT]
- **Location:** `source/SMLib/src/SmProtoTopology.cpp`, `SmProtoTopologyManager::~SmProtoTopologyManager` (the three `SmObjsDelete` locals), `SmProtoVertex::~SmProtoVertex` (:1881-1895), `SmProtoEdge::~SmProtoEdge` (:2645-2664), `SmProtoVertex::AddProtoEdge` (`inc/SmProtoTopology.h:502`, "DD1: Even if already there"), `SmProtoVertex::CombineProtoVertex` (:2598-2603); reached from `SmTopologyIntersector::IntersectInsertRelate` (`SmTopologyIntersector.cpp:1267`), which `SmMerge` (the boolean merge) and `SmFilletExecutive` call.
- **What happens:** The destructor declares the deleters for proto vertices, proto edges and proto faces in that order, so they run in reverse: the proto edges are freed before the proto vertices. Each `~SmProtoEdge` unlists itself once from its start vertex and, only when different, once from its end vertex. A proto edge whose two ends are the same vertex was listed there twice by `CreateProtoEdge` (`AddProtoEdge` adds duplicates), and `CombineProtoVertex` appends the merged vertex's edge entries without checking where those edges end. Either way an entry survives the edge phase, and `~SmProtoVertex` then calls `RemoveProtoVertex` on the freed edge. That is a read of freed memory, plus a write of a null pointer into it if the freed bytes happen to match the vertex address.
- **Trigger:** any intersection whose resolved proto topology has a proto edge with coinciding end vertices, or a combined vertex still listing an edge that no longer ends at it, when the manager is deleted.
- **How found:** pre-review of PR #203 (`boolean.topology_intersector_p2`): valgrind on generated `IntersectInsertRelate` programs (3 of 10).
- **Confirmed:** yes (valgrind "Invalid read" in `SmProtoEdge::RemoveProtoVertex <- ~SmProtoVertex <- ~SmProtoTopologyManager <- IntersectInsertRelate`, on a block freed in the same destructor and allocated by `CreateProtoEdge`; all frames are upstream code).
- **Security ([PSIRT]):** reachable from file data. Two B-reps read from files whose boolean (or fillet merge) produces such a proto edge reach it through the public boolean API, with no bad argument from the caller. The impact looks low: `~SmProtoEdge` nulls the fields that are read just before the block is freed, and nothing is allocated between the free and the read in single-threaded use, so the conditional write needs the block to have been reused. It is still a use-after-free on ordinary inputs, so it should be reported.
- **Severity:** crash/UB (use-after-free), after every output is final.
- **Rust port:** The arena teardown (`ProtoTopologyManager::destroy`) skips edges already deleted, so it is defined. `IntersectInsertRelate` detects the condition before the teardown (replaying the unlisting) and returns `SolidError::InvalidInput` after destroying the manager. The difftest shim runs a verbatim copy of `IntersectInsertRelate` that unlinks the stale entries before the delete and answers `-2` in that case; the shim's own manager teardown unlinks them too.

#### UB-242. A failed imprint is ignored, and the boolean goes on with half-edited B-reps
- **Location:** `source/SMLib/src/SmTopologyIntersector.cpp:1233` `IntersectInsertRelate` (def. `:1092`): the status of `ImprintIntersections` only reaches `SE_MSG` and is overwritten by `SetRelationships` (`:1264`); `source/SMLib/src/SmMerge.cpp:394` `PiecewiseMerge` (def. `:341`) ignores the status of its first `IntersectInsertRelate`.
- **What happens:** A failed imprint leaves both B-reps partly edited and inconsistent (vertex uses already freed by `MakeEdgeInFaceWithVU`), and the boolean goes on to bound, classify and merge them.
- **Trigger:** the shell of a box with a wall thicker than the box (ASan heap-use-after-free reads in `SmFace::CalculateBoundingBox`; tangency-labs/solidcore#372).
- **How found:** ASan run of the SolidCraft C API (tangency-labs/smlib `capi/`).
- **Confirmed:** yes (ASan, in the fork's C API tests); not reproduced in the solidcore difftest.
- **Severity:** crash/UB (use-after-free).
- **Fix:** tangency-labs/smlib af8db37: `IntersectInsertRelate` deletes the proto topology manager and returns the imprint's error, and `PiecewiseMerge` returns the error of its first `IntersectInsertRelate` (`SER`); the callers already propagate it.
- **Rust port:** carries the fix (`TopologyIntersector::intersect_insert_relate`, `merge::p3::piecewise_merge`; test `piecewise_merge_ends_on_a_failed_intersect_insert_relate`).

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
- **Correction (#521):** only the curveless edge is UB. At `c9c5979` `SmVector3d`'s members default to `SM_UNDEF_DOUBLE` (`source/SMLib/inc/SmVector3d.h:56-58`), so the tangent a failed `GetEndTangent` leaves is defined, and both functions then answer `TRUE`. See WR-256. The port and the shim now follow that; a null curve is still refused.

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

#### UB-210. `SmBrep::DeleteEdge` reads `pSurvF` outside the merge branch that sets it (unreachable on well-formed topology)
- **Location:** `source/SMLib/src/SmBrep.cpp:6043` (`pOptSurfaceToKeep != pSurvF->GetSurface()`) and `:6046` (`pDelF->GetSurface()`) in `SmBrep::DeleteEdge` (def. `:5752`); `pSurvF`/`pDelF` are set only inside `if(bMergeFace)` (`:5911`), while the surface-swap block's guard (`:6037-6039`) is only `!bSplitLoop && !(both loops inner)`.
- **What happens:** The two guards differ by the strut test (`bMergeFace = !bSplitLoop && !bVIsStrut && !bVRadIsStrut && !bothInner`). If an end of the edge were a strut end without a split loop, a non-NULL `pOptSurfaceToKeep` would dereference the NULL `pSurvF`.
- **Reachability:** none on well-formed topology. `SmEdgeuse::IsStrut` (`SmEdgeuse.cpp:294-301`) is `GetRadial() == m_pCW`, and `m_pCW` is in the edge use's own loop use. `pEURad = pEU->GetRadial()`, so `pEU->IsStrut()` puts `pEURad` in `pLU`, i.e. `pLURad == pLU` (`bSplitLoop`). For a manifold edge the edge-use list alternates orientations (`GetRadial`, `SmEdgeuse.cpp:3561-3578`, is the previous use for a use oriented like the primary, the next one otherwise), so `pEURad->GetRadial() == pEU` and `pEURad->IsStrut()` likewise implies `bSplitLoop`. Hence `!bSplitLoop` forces both strut flags false, and with `!bothInner` that is `bMergeFace`: `pSurvF` is set (and `NER`-checked). Only an edge-use list whose orientations do not alternate could reach it.
- **How found:** read during porting (unit `topology.brep_p4`).
- **Confirmed:** no; unreachable by the argument above (no fixture B-rep has a non-alternating manifold edge-use list).
- **Severity:** crash (theoretical; malformed topology only).
- **Rust port:** `topology::brep::p4::delete_manifold_edge` keeps a defensive `SolidError::InvalidInput` there (code comment cites UB-210).
#### UB-157. `CreateBlendSurface` with rails of different natural intervals writes past `sIntParams`
- **Location:** `SmBSplineSurfaceNLib.cpp:5305`-`5378` (`SmBSplineSurface::CreateSkinnedSurface`, reads `sIntParams[lNumSections+ii]` at `:5366` and writes it at `:5378`); reached from `SmSurface::CreateBlendSurface` (`SmSurface.cpp:15375`, via `SmDerivSurfDefinition::SkinDerivSurface`, `:3503`), which passes only one parameter per cross section (`sParams`, 101 values). `CreateBlendSurface` also ignores the statuses of both `GetSampleValues` calls (`SmSurface.cpp:15443`, `:15452`) and goes on with partly filled sample arrays.
- **What happens:** With two rails, `CreateSkinnedSurface` expects `2 * lNumSections` intersection parameters. When `SyncronizeKnotsOfCurves` changes the natural interval of rail 2, the loop reads and writes past the end of the `SmTArray` copy: heap corruption.
- **Trigger:** `CreateBlendSurface` with two B-spline curves whose natural intervals differ (e.g. `[0, 1]` and `[0, 2]` knot vectors) on surfaces the curves can be dropped onto.
- **How found:** crash in the C++ reference run (`surface.surface_p8`, generated blend cases: `corrupted size vs. prev_size` in `N_EndNurbs` under `CreateSkinnedSurface`).
- **Confirmed:** yes (heap corruption without the shim's probe; with it, 2000 cases run clean).
- **Severity:** crash/UB.
- **Rust port:** The sample statuses fail the call at the first failing sample (`SM_ERR`). Skinning with too few intersection parameters returns `InvalidInput` ("too few intersection parameters"). The shim probes both conditions (the `GetSampleValues` pair and the rail synchronisation on copies) and reports `SM_ERR` / `1007` instead of running the undefined part.
- **Also reached from:** `SmBrep::CreateEdgeEdgeBlend` (`SmBrep.cpp`, blend flags 1-3), which hands its trimmed, reparameterised edge rails to `CreateBlendSurface`: heap corruption in the `topology.brep_p1` difftest (valgrind: invalid read/write of 8 bytes just past a 128-double `sIntParams` copy, edge-edge blends on the upstream `.smb` files) once that shim ran the blend. The `topology.brep_p1` shim replays `CreateEdgeEdgeBlend` up to the blend call on its own rails and applies the same probe.

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
- **Fuzz sweep trigger (2026-10-20):** an open four-line square profile with one line endpoint z at `9223372036854775807` / `-9223372036854775808` (`SmApiCreateTaperExtrude`, angle 5, caps -5 or 3; `SmApiCreateDraftSweep`, angle 90, caps 3), and a profile of two identical `SmCircle`s (centre `(2.10, 1.35, 0.65)`, radius `6.89`; taper angle `-2.85`, height `30.2`, caps 1, fillet). `sweep.api_brep` `fuzz00538`, `fuzz01054`, `fuzz01493`, `fuzz01538`. Confirmed: yes (segfault). Not guarded yet.

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
- **Fuzz sweep trigger (2026-10-20):** `SmCurveClassification::IntersectVertices` -> `InsertPointClass` -> `FindInterval` with an empty classification and a vertex (`intersect.curve_class_p5` op 6, `n == 0`, `flag1` set; `fuzz-out/n2000/intersect.curve_class_p5/fuzz00653.json`, `fuzz01268.json`, `/bounds truncated to 1`). SIGSEGV in the C++ run: Confirmed becomes yes. Guard: `crates/solidcore-ffi/src/units/intersect_curve_class_p5.rs:90`.

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

#### UB-124. `GetIvlBound_EdgeuseObj` and `GetLoopCrossings` dereference the face, surface and curve before their NULL checks
- **Location:** `source/SMLib/src/SmCurveClass.cpp:20480-20488`, `SmCurveInterval::GetIvlBound_EdgeuseObj` (def. `:20458`); `:11342-11360`, `SmCurveClassification::GetLoopCrossings` (def. `:11303`).
- **What happens:** `GetIvlBound_EdgeuseObj` calls `pClassifyFace->GetUpwardFaceuse()` and `GetSurface()` in its locals, then tests `pClassifyFace == NULL` ("no work"); a classification without face reads `m_pFU` of `NULL`. It also needs the interval's back pointer (`GetCurveClassification()->GetCurve()`). `GetLoopCrossings` calls `pSurface->GetNaturalUVDomain()` (and, for a face without surface, `NULL->` through it) before `if(pFace == NULL || pSurface == NULL) return SM_SUCCESS`, and `pClassifyCrv->GetNaturalInterval()` before `if(pClassifyCrv == NULL ...)`; both early returns are dead.
- **Trigger:** `GetIvlBound_EdgeuseObj` on a classification without `SetFace`; `GetLoopCrossings` over loops of a face without surface, or of a classification without curve.
- **How found:** read during porting (unit `intersect.curve_class_p4`).
- **Confirmed:** no (the difftest shim refuses those calls).
- **Severity:** crash.
- **Rust port:** `SolidError::InvalidInput` (the dead "no work" returns are not reached).

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

#### UB-239. `SmProtoTopologyManager::SplitIntersectingProtoEdges` deletes a combined-away proto edge that proto faces and other proto edges still list
- **Location:** `source/SMLib/src/SmProtoTopology.cpp:6833` (`SplitIntersectingProtoEdges`, def. `:6656`: `CombineProtoEdge`, then `RemoveProtoEdge` and `delete pPE2ToCombine`); `:4346` `SmProtoTopologyManager::RemoveProtoEdge` unlists the edge from `m_sPEs` and its end proto vertices only; the stale entry is read in `GlueCoincidentBrepVertices` (`:5854`, the `GetCoincProtoEdges` walk at `:6245`).
- **What happens:** The proto edge combined away stays in the coincident-proto-edge lists of the proto faces it bounded, and in the overlapping lists of other proto edges. `GlueCoincidentBrepVertices` then walks those proto-face lists and reads the freed proto edge (a comment there already notes that the proto faces would need updating).
- **Trigger:** the inward shell of a 20 mm cube with a 10 mm wall (ASan heap-use-after-free; tangency-labs/solidcore#369).
- **How found:** ASan run of the SolidCraft C API (tangency-labs/smlib `capi/`).
- **Confirmed:** yes (ASan, in the fork's C API tests); not reproduced in the solidcore difftest.
- **Severity:** crash/UB (use-after-free).
- **Fix:** tangency-labs/smlib 93efd6d. `RemoveProtoEdge` drops every reference the listed proto faces and proto edges hold to the edge. `SplitIntersectingProtoEdges` first adds the surviving proto edge to the proto faces the removed one bounded. With the fix the shell above fails with `SM_ERR_NULL_POINTER` instead.
- **Rust port:** carries the fix at the library deletes rather than inside `RemoveProtoEdge` (`ProtoTopologyManager::forget_proto_edge` before the delete in `SplitIntersectingProtoEdges`, `ResolveOverlappingProtoEdges` and `RemoveEdgeDefinition`; `hand_over_coincident_proto_edge`; test `a_forgotten_proto_edge_leaves_every_face_and_overlap_list`). The difftest's script ops unlist or delete a proto edge on their own and dump the lists; there the port keeps upstream's lists, so they compare with the pinned C++. Without the fix nothing is freed (arena ids): the stale id names a deleted slot and the later passes fail with `InvalidInput`.

#### UB-169. `SmHCR(.., crViewVector, ..)` builds the view transform from uninitialised vectors for a zero view vector
- **Location:** `source/SMLib/src/SmHCR.cpp:144` `SmHCR::SmHCR` (view-vector constructor).
- **What happens:** The status of `MakeUnitOrthoVectors` is ignored. When the view vector cannot be unitized (squared length below `SM_EFF_ZERO_SQ`), or when its squared length overflows to infinity (`Unitize` then yields `(0, 0, 0)`, both arbitrary-axis crosses are zero and `SER(SM_ERR)` returns, `SmVector3d.cpp:199-205`), `sPerp1` and `sPerp2` (`SmVector3d() = default`) stay uninitialised and `SetSTEPCanonical` reads them.
- **Trigger:** `SmHCR(ctx, pBrep, SmVector3d(0, 0, 0), ..)`; `SmVector3d(1e200, 0, 0)`.
- **How found:** read during porting (`blend.hcr`).
- **Confirmed:** no (the difftest guard rejects such view vectors; generated cases use `[1e200, 0, 0]`).
- **Severity:** crash/UB.
- **Rust port:** `view_transform_for_vector` returns `InvalidInput`.

#### UB-170. `SmHCR::IntersectCurveWithSilhouettes` reads silhouette edges past the queried array [PSIRT]
- **Location:** `source/SMLib/src/SmHCR.cpp:2121` `SmHCR::IntersectCurveWithSilhouettes`.
- **What happens:** The loop that copies the silhouette edges found in the curve's box is bounded by the size of the vertex query (`sObjects`) instead of the edge query (`sObjectsInBox`). With more silhouette vertices than edges in the box it reads past the edge array (unchecked `operator[]` in release) and uses the values as edges; with fewer it skips edges.
- **Trigger:** A curve whose box holds an open chain of silhouette edges (one more vertex than edges), for example any `ComputeGlobalVisibility` whose silhouettes are open curves.
- **How found:** read during porting (`blend.hcr`).
- **Confirmed:** no (the difftest shim skips the call when the vertex count exceeds the edge count; the skipped-edge behaviour is compared).
- **Severity:** crash/UB.
- **Rust port:** Reads the edges up to the vertex count as upstream and returns `InvalidInput` at the first index past the edge list.

#### UB-189. `SmFace::ComputeLoopOrientation` reads an empty array for an edge loop use without edge uses
- **Location:** `source/SMLib/src/SmFace.cpp:1851` `SmFace::ComputeLoopOrientation` (`sEUs.GetLast()`), `source/SMLib/inc/SmTArray.h:1035`-`1037` (`GetLast` returns `m_pData[m_lSize-1]`).
- **What happens:** for a loop use of edge type whose edge use array is empty, `GetLast()` reads `m_pData[-1]` (an out-of-bounds read of the array header's neighbour, or of a null array).
- **Trigger:** an edge-type loop use with no edge uses. Looks unreachable from files: `SmBrepData` sets `SmEdgeuse_TYPE` for a loop use only together with its edge use array (`SmBrepData.cpp:706`-`712`), so a loaded loop use of edge type has edge uses; it needs a B-rep built or edited in memory by calling code.
- **How found:** read during porting (unit `topology.face_p3`).
- **Confirmed:** no (the difftest guard rejects such a B-rep on both sides).
- **Severity:** crash/UB (out-of-bounds read).
- **Rust port:** `SolidError::InvalidInput` for a loop use without edge uses. (The other `ComputeLoopOrientation` reads that looked undefined, `sVeryFirstUV`, are defined: `SmVector2d` has default member initialisers `SM_UNDEF_DOUBLE`, `SmVector2d.h:56`-`58`; the port starts from `Point2::UNDEF` and computes the closing span with it.)

#### UB-193. `SmFace::FaceIntersect` hands out deleted curves through `pOptCC3DCrvs` when called without classification arrays
- **Location:** `source/SMLib/src/SmFace.cpp:11669` `SmFace::FaceIntersect` (`pOptCC3DCrvs->Append( s3DCurves )`, `pOptThisCCUVCrvs`, `pOptOtherCCUVCrvs`), init block `:11325`-`11333`.
- **What happens:** the surface/surface curves are owned by the cleaners `sClean1`..`sClean3`, which are cleared only when `pOptThisCrvClasses` or `pOptOtherCrvClasses` is given. With `pOptCC3DCrvs` (or the UV arrays) but neither classification array, the caller receives pointers the cleaners delete at return; with the classification arrays but without `pOptCC3DCrvs`, the curves the classifications reference leak. In the coincident branch a failing edge curve `Copy` makes the inner `sClean` delete the copies in `s3DCurves`; `SmObjsDelete::Delete` NULLs each slot and resets the array (`SmTArray.h:2107`-`2115`), so there is no double free, but `sUVCurves1`/`sUVCurves2` keep NULL entries and `sCurveTypes` keeps stale `SM_TC_REGION_BOUNDARY` entries when they reach the intersector (a wrong result at most).
- **Quirk:** the init block resets `pOptThisUVCurves` and `pOptOtherUVCurves` twice and never resets `pOptThisCCUVCrvs` / `pOptOtherCCUVCrvs`, so those outputs append to whatever the caller passed.
- **Trigger:** `FaceIntersect` with `pOptCC3DCrvs != NULL` and both classification arrays `NULL`. The only upstream caller (`SmFeatureExecutive.cpp:1838`) passes `pOptThisCrvClasses` together with `pOptCC3DCrvs` and `pOptThisCCUVCrvs`, which is safe.
- **How found:** read during porting (unit `topology.face_p3`).
- **Confirmed:** no (the difftest shim passes neither array).
- **Severity:** misuse hazard (use after free for bad caller arguments), no file-reachable path.
- **Rust port:** `FaceIntersection` owns all curves and returns fresh vectors; the `!have_curves` block of the coincident branch replaces all four vectors (`s3DCurves`, `sUVCurves1`, `sUVCurves2`, `sCurveTypes`) with the intersector's. Outputs are return values, so the append-to-caller quirk has no counterpart: callers append the returned vectors themselves.

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

#### UB-180. `SmFace::GlobalLineIntersect` reads the first interval of an empty UV line classification
- **Location:** `source/SMLib/src/SmFace.cpp:11769`, `SmFace::GlobalLineIntersect` (def. `:11692`).
- **What happens:** For a range solution of the surface, `sm_FaceClassifyUVLine` classifies the UV line and, when it returns at most one interval, `sCC[0].m_vMid` is read without checking the size. An empty classification with `SM_SUCCESS` reads past the interval array.
- **Trigger:** A line lying in the face's surface (a range solution) whose `sm_FaceClassifyUVLine` succeeds without intervals.
- **How found:** read during porting (`topology.face_p4`).
- **Confirmed:** no.
- **Severity:** crash/UB (out-of-bounds read).
- **Rust port:** Returns `InvalidInput` for an empty classification.

#### UB-200. `SmApiCreateSphere` crashes for radii from about `7e9` and for a non-finite center
- **Location:** `source/SM_API/src/SmApiPrimitives.cpp:335` `SmApiCreateSphere` -> `source/SMLib/src/SmPrimitiveCreation.cpp:894` `SmPrimitiveCreation::CreateSphere`. The API accepts any finite radius up to `SM_BIG_DOUBLE / 100` (`1e18`).
- **What happens:** Segfault inside `CreateSphere`. A NaN or infinite center coordinate (not validated) also segfaults; `SmApiCreateTorus` (B-rep overload, no validation) segfaults on the same centers.
- **Trigger:** radius `7e9`, `1e10`, `1e15`, `1e18` or `-6.5e9` (`6e9`, `6.5e9` and `-6e9` are fine); center `(NaN, 0, 0)` or `(0, inf, 0)` with radius `1`.
- **How found:** crash in the C++ reference run (primitive.api_primitives fixtures and probes).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** not reached yet (`SmPrimitiveCreation` is unported, the call is deferred); the difftest guard keeps these inputs from the C++ side.

#### UB-201. `SmApiCreateSphereNoPole` crashes or hangs for small and large radii
- **Location:** `source/SM_API/src/SmApiPrimitives.cpp:870` `SmApiCreateSphereNoPole` -> `source/SMLib/src/SmPrimitiveCreation.cpp:6911` `SmPrimitiveCreation::CreateSphereNoPole` (tolerance `SM_ZONE_TOL_3D`). The API only rejects radius magnitudes not above `SM_EFF_ZERO`.
- **What happens:** Erratic below about `1e-4`: radius `1.0000001e-12`, `1e-9`, `1e-6`, `2e-6`, `5e-6`, `2e-5` segfault, `5e-5` and `1e-4` do not return, `1e-5` and `3e-4` and up succeed. From about `4e8` (`4e8`, `5e8`, `1e9`, `1e12`, `1e18`, either sign) it segfaults; `3e8` succeeds.
- **Trigger:** `SmApiCreateSphereNoPole((0,0,0), 2e-5, ..)`; `SmApiCreateSphereNoPole((0,0,0), 4e8, ..)`.
- **How found:** crash in the C++ reference run (primitive.api_primitives fixtures and probes).
- **Confirmed:** yes.
- **Severity:** crash/UB, hang.
- **Rust port:** not reached yet (deferred); the difftest guard passes only radii in `[3e-4, 3e8]` on to the C++.

#### UB-191. `CombineCoincidentVertices` deletes its isolated surviving vertex when inputs alias
- **Location:** `source/SMLib/src/SmBrep.cpp:1354-1366`, `SmBrep::CombineCoincidentVertices`.
- **What happens:** The zero-incident-edge branch notifies, sets the surviving vertex's point/tolerance, then calls `DeleteVertex(pVertexToDelete)` and returns `SM_SUCCESS`. If both input pointers are the same, the supposedly surviving vertex is deleted too. Nothing inside the function touches it afterwards, so the call itself is defined; a caller that keeps using the survivor pointer reads freed memory.
- **Trigger:** Pass the same isolated vertex as both `pSurvivingVertex` and `pVertexToDelete`. Aliases with incident edges go through `GlueVertices(v, v)` and keep the survivor.
- **How found:** PR #194 review and source inspection while porting `topology.brep_p5`; corrected in the second review round (the function was first thought undefined).
- **Confirmed:** no native reproduction of the caller hazard. The whole call is compared in difftest; only the shim's target-stage op, which reads the survivor afterwards, refuses this input.
- **Severity:** dangling caller-held survivor (caller use-after-free).
- **Rust port:** Ported as upstream: the isolated aliased combination deletes the vertex and succeeds. Rust ids cannot dangle into freed memory; a later lookup of the deleted id returns `NotFound`.

#### UB-159. `SmFilletGeom(SmFilletSolver*, SmFilletGeomType)` leaves `m_dOrientations` and `m_bCapped` uninitialised
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:8971-9004` (`SmFilletGeom::SmFilletGeom`).
- **What happens:** The constructor's initialiser list and loop set the type, solver, centre line, surface, split flag, status, offset surfaces, rails and deviations, but never `m_dOrientations[2]` or `m_bCapped[2]`. `m_bCapped` is assigned only by `SmFilletGeom::MakeFaceBrep` (`:9427`, `:9476`) and read by `SmFilletGeom::InsertIntersectionTopology` (`:12158`, `if (pV && !m_bCapped[ii])`) and `DumpLevel` (`:13306`): a geometry whose intersection topology is inserted before `MakeFaceBrep` has run reads indeterminate flags. `m_dOrientations` (`SmFilletGeom.h:546`) is never read or written after its declaration (the `m_dOrientations` of the solver classes are their own members), so it is merely dead and uninitialised.
- **Trigger:** `InsertIntersectionTopology` (or `DumpLevel`) on an `SmFilletGeom` before `MakeFaceBrep`.
- **How found:** read during porting (`blend.fillet_geom_p1`, PR #147).
- **Confirmed:** no (source inspection; the difftest never exports these members before they are set).
- **Severity:** UB (indeterminate reads).
- **Rust port:** `FilletGeom::new` starts them as `0.0` and `false`.

#### UB-213. `SmFilletEdge::CalcCrossSection` lifts `*pUVCurve1` when `sm_CreateUVCurve` succeeds without any curve
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:5759-5768` (`SmFilletEdge::CalcCrossSection`): `SER(sm_CreateUVCurve(.., p3DCurve, pUVCurve1, pUVCurve2))`, then `if (p3DCurve == NULL) { SER(pSurface1->LiftCurve(.., *pUVCurve1, pUVCurve1->GetNaturalInterval(), ..)); NER(p3DCurve); }`.
- **What happens:** `NER(p3DCurve)` comes after the lift and nothing checks `pUVCurve1` before it. When `sm_CreateUVCurve` returns `SM_SUCCESS` with neither a 3D curve nor a UV curve (both out-pointers stay `NULL`), `*pUVCurve1` and `pUVCurve1->GetNaturalInterval()` dereference a null pointer.
- **Trigger:** a fillet edge for which `sm_CreateUVCurve` (`:4212`) returns `SM_SUCCESS` with `rp3DCurve` and `rpUVCurve1` both `NULL`, i.e. a success exit that builds no curve.
- **How found:** line-by-line comparison while porting `blend.fillet_geom_p5` (PR #264); the shim guards the call.
- **Confirmed:** no (source inspection; the difftest shim refuses the input rather than running it, and no generated input has been seen to reach it).
- **Severity:** UB (null dereference).
- **Rust port:** `FilletBrep::calc_cross_section` returns `SM_ERR_NULL_POINTER` at the lift (`blend/fillet_geom/p5.rs`, the `UB-213` comment). The `blend.fillet_geom_p5` shim runs the same `sm_CreateUVCurve` call first (`p5_cross_runs`) and refuses (`SC_ERR_ARG`) an input where it succeeds with no curve, so no run reaches the dereference.

#### UB-214. `SmFilletEdge::CalcFilletIntSideFace` calls `pCorner->IsExtendedSurfacePatch()` before its `pCorner` test
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:6219` (the call) and `:6223` (`(pCorner) ? pCorner->GetExtendedSurface(pFace) : NULL`), `SmFilletEdge::CalcFilletIntSideFace`; the else branch (`:6234`) also calls `pCorner->GetFilletedVertex()` unchecked.
- **What happens:** the conditional that tests `pCorner` four lines after the dereference shows the author expected a null corner, but `pCorner->IsExtendedSurfacePatch()` (`SmFilletCorner.h:206`, an inline read of `m_bExtendedSurfacePatch`) already ran on it: a null `pCorner` reads a member through a null `this`.
- **Trigger:** `CalcFilletIntSideFace` on a fillet edge with no 3D curve whose primary edge use has a fillet geometry, with a `NULL` corner argument (the function's default; `SmFilletVertex::CalcFilletIntSideFace` callers pass the vertex's corner, which is not `NULL` in a real run).
- **How found:** line-by-line comparison while porting `blend.fillet_geom_p5` (PR #264); the shim's guard `p5_side_runs` models it.
- **Confirmed:** no (source inspection; the difftest shim refuses the input).
- **Severity:** UB (null dereference).
- **Rust port:** `FilletBrep::calc_fillet_int_side_face` returns `SM_ERR_NULL_POINTER` for the missing corner (`blend/fillet_geom/p5.rs`, the `UB-214` comment); the shim refuses (`SC_ERR_ARG`) the inputs that reach it.

#### UB-238. `SmFilletGeom::MakeRailEdgeuse` leaves a deleted vertex use linked in the fillet vertex when `NER(pBaseEU)` fails
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:10042-10052` (the allocation and `pFilletVertex->PostInsert(pNewVU)`), `:10076-10077` (`SmEdgeuse *pBaseEU = pFilletSolver->GetEdgeuse(lRailIndex); NER(pBaseEU);`), `:10089-10090` (`pBaseEU1->GetFace()`), `SmFilletGeom::MakeRailEdgeuse`.
- **What happens:** the new edge use and vertex use are created under `SmObjDelete sClean3` / `sClean2`, and the vertex use is inserted into `pFilletVertex`'s use list (`SER(pFilletVertex->PostInsert(pNewVU))`) before the base edge use is looked up. When the solver has no edge use for `lRailIndex`, `NER(pBaseEU)` returns and the clean-up deletes the vertex use while it is still linked; a vertex use's destructor does not unlink it, so `~SmFilletVertex` deletes the same object a second time (double free). When `GetEdgeuse(lRailIndex)` is set but `GetEdgeuse(1-lRailIndex)` is not, `pBaseEU1->GetFace()` dereferences a null pointer.
- **Trigger:** `MakeRailEdgeuse` with a non-null fillet solver and fillet vertex whose solver lacks one of its two base edge uses (a solver the executive has not set up).
- **How found:** line-by-line reading of `MakeRailEdgeuse` while adding its op to the `blend.fillet_geom_p5` shim (PR #264); `~SmFilletVertex` (`:1333-1342`) deletes every vertex use in its list and `~SmFilletVertexuse` (`SmFilletGeom.h:180`) / `~SmVertexuse` (`SmVertexuse.h:149`) do not unlink the use from the vertex.
- **Confirmed:** no (source inspection; the shim refuses the input rather than running it).
- **Severity:** UB (double free, null dereference).
- **Rust port:** `FilletBrep::make_rail_edgeuse` checks `NER(pFilletSolver)` and `NER(pFilletVertex)` and then defers at the edge-use constructor (`UNPORTED(topology.brep_p3)`); the shim refuses (`SC_ERR_ARG`) a solver without both base edge uses once those two checks pass, so the harness only compares the two defined early exits.

#### UB-162. `sm_RedistributeSeamUVTrimCurves` reads a fourth edge use that need not exist
- **Location:** `source/SMLib/src/SmBrep.cpp:12097`-`12105` (`sm_RedistributeSeamUVTrimCurves`): `SM_PTR_ARRAY(sNewEdgeuses, SmEdgeuse, 16)`, `pNewE->GetEdgeuses(sNewEdgeuses); SM_ASSERT(sNewEdgeuses.GetSize() == 4)`, then `sNewEdgeuses[0]` and `sNewEdgeuses[3]`.
- **What happens:** The assertion is a no-op in release builds. For a seam edge with fewer than four edge uses the function reads slot 3 of the 16-slot stack array, an uninitialised pointer, and calls `GetCWEdgeuse()` on it.
- **Trigger:** A UV trim curve given, a seam direction other than `SM_SP_NEITHER`, and an edge with one, two or three edge uses (a wire or lamina seam edge, or an edge whose other face is not attached yet).
- **How found:** source review while porting `topology.brep_p1` (PR #145); the difftest shim refuses the call for edges with fewer than four edge uses.
- **Confirmed:** no (source inspection only; the shim never runs it).
- **Severity:** crash/UB.
- **Rust port:** `topology::brep::p1::redistribute_seam_uv_trim_curves` returns `InvalidInput` for an edge with fewer than four edge uses.

#### UB-165. `SmBrep::MakeFaceWithCurves` with `pbOptCrvOnSurf` and no UV curves dereferences a null array
- **Location:** `source/SMLib/src/SmBrep.cpp:16591-16606` `SmBrep::MakeFaceWithCurves` (the `pbOptCrvOnSurf && *pbOptCrvOnSurf == TRUE` block after `MakeFace`).
- **What happens:** The UV curve array `cpOptUVCurves` is optional (only one of the 3D and UV arrays must be given), but the curve-on-surface block copies `cpOptUVCurves->GetAt(ii)` for `ii < cpOptUVCurves->GetSize()` without a null check, so a call with 3D curves only and `*pbOptCrvOnSurf == TRUE` dereferences null as soon as `MakeFace` succeeds (even for one empty loop).
- **Trigger:** `MakeFaceWithCurves(region, {0}, &curves3d, NULL, orients, {}, plane, domain, SM_OT_SAME, .., &TRUE)` (difftest cases `face_curves_cos_no_uv_*` of `topology.brep_p7`).
- **How found:** crash in the C++ reference run (pre-review fuzz of PR #155).
- **Confirmed:** yes (SIGSEGV).
- **Severity:** crash.
- **Rust port:** `SolidError::InvalidInput`; the difftest shim keeps such calls away from upstream.

#### UB-166. `sm_DecomposeSrf` writes past the surface cache's patch array when the constructor's knot snap raises the start multiplicity
- **Location:** `source/SMLib/src/SmNurbsSrf.cpp:1524` `sm_DecomposeSrf` (v-direction loop, `:1690`-`:1740`: `Qw = surA[iq][jq]->net->Pw` with no bound on `jq`; the u direction has the `if (iq >= nsu) NL_QUIT` guard, "B262"), called from `sm_CreateBezierPatchBlock` (`SmSurfaceCache.cpp:4848`) <- `SmSurfaceCache::BuildTreeBase` (`:4996`) <- `SmTrimSrfCache::BuildTree` <- `smsurf_GetSurfaceCache`, reached from `sm_SurfaceDropPoint` (`SmSurface::GlobalPointSolve`) and from `sm_ClassifyFacePoint` through `SmFace::CalculateUVDomainFromUVTrimCurves` -> `SmEdgeuse::CreateUVTrimCurve` -> `SmSurface::DropCurve`; also from `sm_SrfSrfIntersection` (`SmSurfaceIntersector::DoIntersection`, `SmSurfaceIntersector.cpp:6146`) and `SmSurface::GlobalCurveIntersect` (`SmSurface.cpp:17196`). The fixup: `sm_FixupKnotVector` (`SmNurbsCrv.cpp:386`), run by every `SmBSplineSurface` constructor (`SmBSplineSurface.cpp:135/136, 203/204, 295/296, 344/345`) with `SM_EFF_ZERO_PARAM` (`1.2e-8`).
- **What happens:** `sm_FixupKnotVector` first makes every knot less than `SmTol::GetScaledZero(u0, u1)` (`SM_EFF_ZERO * (1 + max(|u0|, |u1|))`, about `2e-12` on `[0, 1]`) above its already-snapped predecessor equal to it. A first interior knot that close to the start knot therefore becomes the start knot, and the start multiplicity rises to `q + 2`. Its second stage then walks the distinct knots and spreads a pair closer than `SM_EFF_ZERO_PARAM` to `1.01 * SM_EFF_ZERO_PARAM = 1.212e-8` apart: at the first pair the second knot moves up, at the last pair the first knot moves down (`vKts[i] = vKts[i+1] - dIncrement`), elsewhere both move away from their midpoint. Nothing keeps a moved knot off the start knot, so on a knot range under about `2.4e-8` the last-pair move lands a knot exactly on the start and raises its multiplicity the same way (a move below the start leaves a non-monotone vector, which runs or fails with status 1006, no crash). `N_BasisGetSpanCount` (exact comparisons from index `q`) does not count the zero-length first span, but `sm_DecomposeSrf` starts its v loop at `iev = q + 1` without comparing `VP[q]` with `VP[q + 1]`, so it extracts a patch for that zero-length span and then one more patch than the `nsu * nsv` the caller allocated: `surA[iq][nsv]` is read past the `SmTArray` and dereferenced (SIGSEGV). In u the same over-count reaches the B262 guard and the decomposition fails (`NL_QUIT`, an error, no crash). Curves are safe: `sm_CreateBezSegments` (`SmNurbsCrv.cpp:478`) skips a start over-multiplicity ("bad data") and bounds `iq` by the span count.
- **Trigger:** any surface whose v knot vector, after both fixup stages, has start multiplicity above `degree + 1`: by the snap, `V = [0, 0, 0, 1e-12, 1, 1, 1]` (degree 2) or `[0, 0, 0, 6.2e-31, 6.8e-31, 1, 1, 1]`; by the spread, `[0, 0, 0, 5e-9, 5e-9, 1.212e-8, 1.212e-8, 1.212e-8]` (the snap leaves it, the first pair moves `5e-9` up to `1.212e-8`, the last pair moves it down to exactly `0`); by the end snap first, `[0, 0, 0, 5e-9, 5e-9, E, E + d x3]` with `E = 1.212e-8` and `d` under the scaled zero (the end group snaps to `E` before the spread, which then moves `5e-9` to `E - E = 0`). `(1e-11, 2e-11)` interior knots run. The spread can also leave a non-monotone vector, which upstream handles without undefined behaviour (valgrind clean): `[0, 0, 0, 5e-9, 8e-9, 1.212e-8 x3]` (spread to `[.., 4e-9, 0, ..]`) fails with 1006, `[0, 0, 0, 5e-9, 5e-9, 1e-8 x3]` (a knot moved below the start) returns 1001 (no solution), `[0, 0, 0, 5e-9, 5e-9, E, E + 2e-12 x3]` solves with status 0. The same tiny gaps away from the start (`0.5`, `0.5 + 1e-13`) only raise an interior multiplicity, which the decomposition tolerates. The same vectors in u fail with 1006 (B262 guard). Only the callers that build the surface cache reach it.
- **How found:** the reviewer's timing fuzz of the `blend.fillet_geom_p1` FFI guards (PR #147 re-review; three reproducers, `SurfaceDropPoint` and `ClassifyFacePoint`), located with gdb; mechanism by source reading (`sm_FixupKnotVector`, `N_BasisGetSpanCount`, `sm_DecomposeSrf`). The spread-stage trigger: the round-3 review; the end-snap route: the round-4 review (three reproducers, gdb, same frames), which a Rust replica of the fixup had missed by one operation.
- **Confirmed:** yes (gdb backtraces in the reference build for the four reproducers; the FFI's rejection of exactly the constructed condition makes them rejected cases and the fuzz sweeps no longer crash).
- **Severity:** crash (out-of-bounds read and write through a stale pointer).
- **Rust port:** `nurbs::nurbs_srf::decompose_srf` bounds the patch index and returns an error. The `blend.fillet_geom_p1` shim reads back the knot vector the real constructor left and refuses (`SC_ERR_ARG`), for the six ops that build a surface cache, a surface whose constructed v vector has start multiplicity above `degree + 1`; that is the crash precondition, so it also refuses some runs that would complete because their path returns before building the cache (28 of 169 such programs in the round-5 review: `TwoPnts` 1001 and Fb-op exits before the cache fetch). It does not re-implement the fixup (a replica in the Rust wrapper missed the end-snap route). The u over-multiplicity, non-monotone constructed vectors, curves and the other ops are compared. Comparing the non-monotone vectors exposed three port bugs on the cache path, fixed in PR #147: the Rust caches held a `Clone` of their surface (the copy constructor, which runs the fixup a second time) where upstream holds a pointer, `compute_net_constants` returned the error of a failed `N_SrfEvalPt` that upstream ignores, and `subdivide_node` returned the error of a failed patch split that upstream's `GW_ERR` only logs (the children stay zero-filled).
#### UB-215. `SmTopologySweep::SweepEdgeHigh` uses the original edge curve after `MakeFaceWithCurves` freed it
- **Location:** `source/SMLib/src/SmTopologySweep.cpp:2162`-`2165` (`SmTopologySweep::SweepEdgeHigh`: `pOrigEdgeCurve->GetOwner()` after `pBrepToSweep->MakeFaceWithCurves(..)` at `:2127`); `source/SMLib/src/SmBrep.cpp:16134`-`16143` (`SmBrep::MakeFaceWithCurves` deletes each trim curve that is degenerate or whose `ApproximateLength` is below the zone tolerance, and clears its slot in the caller's array). The swept copy and the vertex sweep curves are looked up in `s3DCurves` again before use (`:2178`-`2183`, the B317 fix, `:2207`, `:2270`); the original-position copy is not.
- **What happens:** When the face built for a swept edge drops the copy of the edge's own curve because it is shorter than the zone tolerance, `SweepEdgeHigh` makes a virtual call through the freed curve. The release build segfaults loading `GetOwner` from its vtable.
- **Trigger:** Partial primitives of `SmPrimitiveCreation` on a fresh B-rep (zone tolerance `1e-5`), which sweep their profile edges with `SmRotationalSweepGeometry` (turns below `359.99999` degrees): `CreateCone(1, 1, top, 0, 90, identity)` with `top` from `1.2e-6` to `9e-6`, and the same with the height or both radii in that range (`1e-5` returns `SM_ERR`; `1e-9` to `1e-6` return `SM_ERR_INVALID_INPUT`); `CreateTorus(major, minor, 0, 90)` with `minor` `5e-7`, `1e-6` or `1.5e-6` and `major` `0`, `minor` or `0.01`, at origins `0`, `(1, -0.7, 0.3)` and `(1000, -700, 300)`, over turns `90`, `359` and `1e-9` (`minor = 2e-6` does not crash). Generated cases `gen0097`, `gen0145`, `gen0150`, `gen0197` and `gen0202` of `primitive.primitive_creation_p2`. Partial spheres never crash: the axis line sweeps to no face, and the arc's face fails the degenerate-surface test of `SweepEdgeHigh` first.
- **How found:** generated difftest case (`primitive.primitive_creation_p2`), then about 450 isolated native probes (one process per input).
- **Confirmed:** yes. Each crashing probe's backtrace (a scratch `SIGSEGV` handler, frames resolved with `addr2line`) ends in `SweepEdgeHigh` <- `DoSweepEdgesHigher` <- `DoSweep` <- `CreateRotationalSweep`; the faulting instruction is the vtable load after the `pOrigEdgeCurve` null test.
- **Severity:** crash.
- **Rust port:** `sweep::topology_sweep` keeps arena ids: a dropped curve has no edge in the face build's `curve_edges`, and `sweep_edge_high` returns an error (`required`) instead of reading it. The `primitive.primitive_creation_p2` FFI rejects partial cones that build a profile line shorter than the zone tolerance (mirroring the cone's own early failures) and partial tori whose minor circle is shorter than it.
#### UB-247. `CreateTaperExtrude`'s start cap deletes input curves a failed `CreatePlanarFaceWith3DCurves` left in edges
- **Location:** `source/SMLib/src/SmPrimitiveCreation.cpp:4327`-`4365` (`SmPrimitiveCreation::CreateTaperExtrude`, the start cap: `CreatePlanarFaceWith3DCurves(pRegion, r3DCurves, ..)`, and `bCleanupInputCurves`, cleared only on success); `source/SMLib/src/SmBrep.cpp:3834`-`3937` (`SmBrep::CreatePlanarFaceWith3DCurves`: `MakeFaceWithCurves` per outer loop under `SER`, its curves used by the new edges).
- **What happens:** With the start cap (`iEndCaps` 1 or 3) the taper passes its chained input curves to `CreatePlanarFaceWith3DCurves`, which requires each curve end to meet exactly one other end (its header, `:3800`-`3801`). When that fails, the taper keeps `bCleanupInputCurves` and deletes the curves at clean-up, although edges of the result B-rep still use them (its own memory note: "Adding a Start cap consumes the r3DCurves as the curves for NewEdges"). The release build then segfaults in `SmEdgeuse::~SmEdgeuse` as the shim deletes the result B-rep, or inside `CreateTaperExtrude`.
- **Trigger:** On a fresh B-rep, curves that close a loop while an end meets no other end or more than one, with the start cap: a line from `(0, 43)` into a unit square ending at its corner `(1, 0)` (difftest fuzz case `fuzz00880`, caps 3, normal +Z), the same line stopping `1e-3` short of the corner, two unit squares meeting at a corner, a line from a corner into the square (under -Z), a half circle across one side (under every normal). Each crashes under one normal or another and runs without the start cap. Curves that close no loop fail first and harmlessly, and clean loops ran in every probe, a self-crossing bow-tie and a square with a repeated or reversed side among them.
- **How found:** `difftest --unit primitive.primitive_creation_p2 --fuzz 1000` (PR #250, 2026-10-21), then about 400 isolated native probes (one process per input) over tails, squares, spikes and arcs, every cap and normal.
- **Confirmed:** yes. The fuzz case's backtrace (a scratch `SIGSEGV` handler, frames resolved with `addr2line`) is `SmEdgeuse::~SmEdgeuse` <- `SmEdgeuse::~SmEdgeuse` <- `SmBrep::~SmBrep` <- the shim; an arc crossed by a line under -Z crashes inside `CreateTaperExtrude`.
- **Severity:** crash (use after free).
- **Rust port:** `PrimitiveCreationAccess::planar_face` takes the curves by value, so a failed cap frees nothing an edge uses. The `primitive.primitive_creation_p2` FFI refuses, with the start cap, curves that close a loop while an end meets no other end or more than one within `2 * SM_ZONE_TOL_3D` (repeats of another curve left out); it still refuses some that run natively (a tail ending inside the square, a square with a diagonal, two squares at a corner in another order).

#### UB-211. `SmFilletVertex::CalcVertOnVert` dereferences a missing or non-vertex classification object
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:2337-2345` (`SmFilletVertex::CalcVertOnVert`), reached from `SmFilletVertex::CalcCornerVertGeom` (`:1434`) for an `SM_FV_ON_VERTEX` vertex.
- **What happens:** `(SmVertex*)GetPointClassObject()` is used without a check: a vertex whose point classification has no object dereferences `NULL` in `pVert->GetPoint()`, and an object of another kind (an edge use or face) is cast blindly and read as a vertex. There is no `NER` as in the sibling calculators.
- **Trigger:** `CalcCornerVertGeom()` on an `SM_FV_ON_VERTEX` fillet vertex whose classification object is `NULL` or not an `SmVertex`.
- **How found:** read during porting (`blend.fillet_geom_p3`, PR #242).
- **Confirmed:** no (the difftest shim refuses such models, `SC_ERR_ARG`).
- **Severity:** crash (NULL dereference) / UB (wrong-type cast).
- **Rust port:** `FilletBrep::calc_vert_on_vert` returns `SM_ERR_NULL_POINTER` without an object and `SolidError::InvalidInput` for an object that is not a vertex.

#### UB-185. `SmFilletVertex::CalcMateGeom` recurses without bound through a cycle of mates
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:3480-3490` (`SmFilletVertex::CalcMateGeom`: `pMainFV->CalcCornerVertGeom()`), with `CalcCornerVertGeom` (`:1434`) calling `CalcMateGeom` for an `SM_FV_MATE` vertex.
- **What happens:** A mate computes its main vertex (`GetMate(0)`) first, unprocessed, through `CalcCornerVertGeom`; when the main vertex is itself an unprocessed mate whose `GetMate(0)` leads back (a cycle of `SM_FV_MATE` vertices, including a vertex that is its own mate), the two calls recurse until the stack overflows. Nothing marks a vertex as in progress.
- **Trigger:** two unprocessed `SM_FV_MATE` fillet vertices that are each other's `GetMate(0)`.
- **How found:** read during porting (`blend.fillet_geom_p3`, PR #242).
- **Confirmed:** no (the difftest shim refuses mate cycles, `SC_ERR_ARG`).
- **Severity:** crash (stack overflow).
- **Rust port:** the nesting is bounded by `blend::fillet_geom::p3::MAX_MATE_DEPTH` (64; upstream's corners nest it at most once); deeper nesting is `SolidError::InvalidInput`.


#### UB-208. `SmFilletGeom::ReCalcFilletGeom` reads the first of no sorted rail-end points
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:11648` (`SmFilletGeom::ReCalcFilletGeom`: `sDir = sTsectPnts[0]->CrvDeriv();`), after the sort loop of `:11431-11507`.
- **What happens:** Without a march direction (`pOptMarchDir == NULL`) and with no curve on rail 0, the direction is taken from the first sorted rail-end point without checking that there is one. The sort loop skips every vertex that is unprocessed, `SM_FV_MATE` or `SM_FV_RAIL_X_EXTENDED_EDGEUSE`, and every point whose uv is unset (`-SM_BIG_DOUBLE`, `:11494` and `:11505`). When it skips them all, `sTsectPnts` is empty, so `sTsectPnts[0]` reads past the array (its stack buffer) and dereferences that pointer.
- **Trigger:** `ReCalcFilletGeom(FALSE, NULL, NULL)` on a geometry whose rails' end vertices are all unprocessed (or all mates), both rails present and rail 0 without a curve: the `recalc_setup_fails` fixture with every vertex `status` 0 and a solver whose `SetupOffsetValues` succeeds (`regress_recalc_no_dir_all_unprocessed`), or with every vertex a processed mate (`regress_recalc_no_dir_all_mates`).
- **How found:** reviewer's adversarial fixtures on PR #242 (core dump in the difftest process).
- **Confirmed:** yes (both regression fixtures segfault the reference build, SIGSEGV in the difftest process, when the shim's guard is disabled).
- **Severity:** crash (out-of-range read, then a wild pointer dereference).
- **Rust port:** unreachable today (`FilletBrep::re_calc_fillet_geom` stops at the fillet intersector's point intersection first); the march direction there takes the first point through `.first()` and returns `SolidError::InvalidInput` when there is none. The `blend.fillet_geom_p3` shim refuses (`SC_ERR_ARG`) exactly that model: no march direction, rail 0 without a curve, and a sort loop that would finish without a point.

#### UB-210. `SmFilletEdge::CalcFilletIntFillet` reads the context of a missing fillet corner
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:6051` (`SmFilletEdge::CalcFilletIntFillet`, from `:5959`: `sm_SrfSrfIntersection(m_cpCorner->GetContext(), ...)`); the failure branch after it calls `pCorner->RecordFilletError` (`:6072`).
- **What happens:** When the fillet/fillet edge has no B-spline 3D curve yet, the surface/surface intersection is called with the context of the edge's own fillet corner, which is never checked. An edge without corner (`m_cpCorner == NULL`) dereferences `NULL`. Upstream's corners make these edges with a corner, so only a caller that builds or calls one without it reaches this.
- **Trigger:** `CalcFilletIntFillet(crContext, pCorner)` on an `SM_FE_FILLET_X_FILLET` edge without curve or corner, both end vertices processed and both geometries with a solver and a fillet surface: fixture `regress_fif_no_curve_no_corner` (generated seed 748 of the 2000-seed sweep).
- **How found:** generated difftest case (the 2000-seed sweep while addressing the PR #242 review); gdb puts the SIGSEGV in `SmFilletEdge::CalcFilletIntFillet`.
- **Confirmed:** yes (SIGSEGV in the reference build before the shim refused it).
- **Severity:** crash (NULL dereference).
- **Rust port:** `FilletBrep::calc_fillet_int_fillet` returns `SM_ERR_NULL_POINTER` for an edge without corner on that path, before the unported intersection. The `blend.fillet_geom_p3` shim refuses (`SC_ERR_ARG`) a fillet/fillet edge without corner when it has no B-spline curve.

#### UB-232. `SmApiCreatePipeSweep` raises `SIGFPE` for a path with a `1e300` control point
- **Location:** `source/SM_API/src/SmApiBrep.cpp:1376` `SmApiCreatePipeSweep` -> `SmPrimitiveCreation::CreatePipeSweep`. The faulting instruction was not located (an integer division or `INT_MIN / -1`).
- **What happens:** The process is killed by signal 8.
- **Trigger:** `SmApiCreatePipeSweep(radius 0.7176, path = SmApiCreateCurve((0,0,0), (3.078,0,6.097), (3.326,0,12.19), (0.516,1e300,18.29)), cap TRUE)`.
- **How found:** crash in the C++ reference run (`difftest --fuzz 2000`, `sweep.api_brep` `fuzz01378`; still crashing in `verify2000/sweep.api_brep/`).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** The sweep wrappers are stubs until `primitive.primitive_creation_p2` lands; not guarded yet (open).

#### UB-237. Healing a face use built without a shell crashes in `SmBrep::TestClosureSplitRegion` [PSIRT]
- **Location:** `source/SMLib/src/SmBrep.cpp:27907` (`pFUStart->GetShell()->GetRegion()`), `SmBrep::TestClosureSplitRegion` (def. `:27845`), called from `SmHealData::Fix_BadSheets` (`SmHealData.cpp:4832`, def. `:4732`), reached from `SmBrep::MakeTopologyFromData` when the healer runs (`bHealerIsEnabled` and `bAlwaysHeal` or a file healer version below the current one).
- **What happens:** `MakeTopologyFromData` builds a face use whose record lies in no face-use shell without a shell. `Fix_BadSheets` hands it to `TestClosureSplitRegion`, which dereferences the null shell.
- **Trigger:** `.smb` data whose shells leave a face use record uncovered (the `topology.brep_p10` "spine" fixtures with the IGES patch: region 1 without shells), read with the healer enabled.
- **How found:** crash in the C++ reference run (`difftest --unit topology.brep_p10 --fuzz 2000`: `fuzz-out/n2000/topology.brep_p10/fuzz00353.json`, `fuzz00790.json`, `fuzz01898.json`, `fuzz01957.json`; mutations `/options/5 = true`, i.e. healer enabled).
- **Confirmed:** yes (SIGSEGV, 4 cases).
- **Severity:** crash/UB (null dereference). A crafted file with an old healer version reaches it through `ReadFromFile`.
- **Rust port:** not reached; the `topology.brep_p10` runner rejects healing requests on such data (`crates/difftest/src/units/topology_brep_p10.rs:182` `heal_guard`).

#### UB-252. A variable radius law that cannot be evaluated: `SetupOffsetValues` and `SmTemporaryRadiusChange::SetUp` read its unset output
- **Location:** `source/SMLib/src/SmFilletStandardSolver.cpp:2414-2434` `SmVariableRadiusFS::SetupOffsetValues`: `m_rLaw->Evaluate` (`:2420`) is called without checking its status, and `dValues[0]` goes into `*pdOffsetDist` (`:2428`) and both offset surfaces' distances (`:2429-2430`) whatever happened. `source/SMLib/src/SmFilletExecutive.cpp:5036-5037` `SmTemporaryRadiusChange::SetUp`: `pLaw->Evaluate(dEdgeT, sEdgeIvl, bRev, adValues)` unchecked, then `dEndRad = adValues[0]` builds the temporary constant law (`:5042-5043`, `:5067-5068`). `source/SMLib/src/SmFilletSolver.cpp:5254` and `:5321` `SmFilletSolver::StepSolve` ignore the status of `SetupOffsetValues`, which the function never reports (it returns `SM_SUCCESS` unconditionally). The other users of the law (`SmFilletStandardSolver.cpp:2075`, `:2539`) do check.
- **What happens:** `SmBSplineFilletLaw::Evaluate` fails where the law is evaluated past the fillet edge's ends and its curve cannot be extended, and leaves the output array unset. The unset `dValues[0]` (a radius of -3e-314 under the debugger, tangency-labs/solidcore#458) then becomes the offset distance of both offset surfaces, or the radius of the temporary constant law.
- **Trigger:** a variable round (`SM_FS_VARIABLE_RADIUS`) whose `SmBSplineFilletLaw` curve has zero speed, a constant law laid out as the radius in x and 0 in y (`SmBSplineCurve::CreateExtendedCurve` refuses it), on a cube edge or running into an R5 corner; the law the C API shim of tangency-labs/smlib built before 3759189 (found on tangency-labs/smlib#421).
- **How found:** issue #458, read at `c9c5979`.
- **Confirmed:** no (the maintainer saw the unset radius under the debugger; ASan/UBSan do not see an uninitialised read, and the round then fails with 1001 further on; no C++ run here).
- **Severity:** crash/UB (an uninitialised read).
- **Fix:** tangency-labs/smlib@9a12ad7 returns the failed evaluation from both (`SetUp` before it changes anything) and checks `SetupOffsetValues` in `StepSolve`.
- **Rust port:** `VariableRadiusFs::setup_offset_values` (`crates/solidcore/src/blend/fillet_standard_solver/p1.rs`) and `TemporaryRadiusChange::set_up` (`crates/solidcore/src/blend/fillet_executive/p2.rs`) already return the law evaluation's failure, `set_up` before it changes anything. `FilletSolver::step_solve` (`fillet_solver/p3.rs`) dropped the first one's failure with `ignore_status` and went on with the offsets as they were; it now returns it, as the fork's does (test `step_solve_returns_a_failed_setup_of_the_offsets`). Upstream defines nothing for that state, and its `SetupOffsetValues` never fails, so no result the difftest compares changes. The second `StepSolve` call (`:5321`) is behind the variable-radius branch that `step_solve` still defers (`GetOriginalCurve`, unit `blend.fillet_standard_solver_p1`); it must check the status when that branch is ported.

#### UB-253. `SmFace::HasSeamProblem` classifies a NULL seam curve when `CreateSeamCurve` makes none
- **Location:** `source/SMLib/src/SmFace.cpp:15661` (U) and `:15805` (V) `SmFace::HasSeamProblem` (def. `:15540`): the status of `pSurface->CreateSeamCurve(*pContext, dir, pSeamConst, bPeriodic)` is ignored, and `pSeamConst->GetNaturalInterval()` (`:15666`, `:15812`), `GetUVCurve()` (`:15667`, `:15732`; `:15813`, `:15880`) and the classification built on it follow unchecked. `source/SMLib/src/SmSurface.cpp:15124` `SmSurface::CreateSeamCurve` (def. `:15108`) leaves its output NULL when `bOptIsClosed != TRUE` and `IsClosed` says the surface is open in that direction. The one production caller, `SmFaceProps::SetProps_Stage2` (`SmFaceProps.cpp:974`), ignores the result.
- **What happens:** A periodicity flag that is set but not `TRUE` (e.g. `UNSURE`, `2`) enters the periodic branch, but `CreateSeamCurve` re-tests closure instead of trusting it. On a surface that is not closed in that direction it returns `SM_SUCCESS` with a NULL curve, which the classification dereferences.
- **Trigger:** `HasSeamProblem` with stage 2 face properties (`pOptFaceProps` at `SM_PROPSTAGE_2a` or later) and `*pOptPeriodicU` (or `V`) equal to `UNSURE` on a face whose surface is open in that direction. Not reachable from upstream's callers at `c9c5979`: `SetProps_Stage2` passes `&m_bClosedU`/`&m_bClosedV`, which it has just set from `IsClosed` (`SmFaceProps.cpp:909-910`), and every `IsClosed` override (`SmSurface.cpp:22157`, `SmPlane.h:432`, `SmSphere.cpp:3222`) returns only `TRUE` or `FALSE`. The other callers reach the branch only through `SetProps_Stage2`. A failed allocation in `CreateSeamCurve` would also leave it NULL.
- **How found:** issue #519 (from tangency-labs/smlib's status sweep), read at `c9c5979`.
- **Confirmed:** no (no C++ run; the harness has no route that passes `UNSURE`).
- **Severity:** crash/UB (null dereference) for a caller that passes such a flag.
- **Fix:** tangency-labs/smlib@3cba260 passes on `CreateSeamCurve`'s status and refuses a NULL seam curve (`NER`) before use. The same commit makes `HasSeamProblem` return a status rather than `SmBoolean` (WR-176) and passes on the statuses it ignored.
- **Rust port:** not affected. `classify_seam` (`crates/solidcore/src/topology/face/p4.rs`) returns `SolidError::InvalidInput` when `Surface::create_seam_curve` makes no curve, and that function's error when it fails, before any use. Its periodicity flags are `bool`, so it always builds the seam in a periodic direction, as upstream does for `TRUE`. `FindVertexSector`'s seam curves are `Option`s, used only when the surface is closed, as upstream. The rest of the fork's change is not ported: the port keeps `c9c5979`'s results, which difftest compares (`has_problem` stays as WR-176).

#### UB-254. `SmFillet3x2MixedConvexityCorner::CreateCornerPatch` uses a freed fillet edge listed twice in `m_vEdges`
- **Location:** `source/SMLib/src/SmFilletCorner.cpp:4692-4700` (`SmFillet3x2MixedConvexityCorner::CreateCornerPatch`, def. `:4623`): `m_vEdges.RemoveAt(lFoundIndex)` removes one entry of the on-edge fillet edge, then `delete pFEOnEdge`; the base `CreateCornerPatch` and `MakeFaceBrep` that follow walk `m_vEdges`.
- **What happens:** When the corner's edge list holds the on-edge edge more than once, the remaining entry points at the deleted edge, and the patch dereferences it.
- **Trigger:** a 3x2 corner whose `m_vEdges` repeats its `SM_FE_ON_EDGE` edge (the corner's own topology code never builds one; the difftest model can).
- **How found:** `difftest --unit blend.fillet_corner_p3 --fuzz 500` (PR #722), SIGSEGV in the shim.
- **Confirmed:** yes (C++ build: signal 11 in fuzz00157).
- **Severity:** crash/UB (use after free) for such an input.
- **Rust port:** not affected (the edge is an arena id; the deleted edge is reported as not found). The shim's pre-check (`blend_fillet_corner_p3_static.inc`) refuses the input.

### Healer target management

#### HANG-32. `SmBrep::CreateParallelProjectionCurves` spins on a subnormal approximation tolerance
- **Location:** `source/SMLib/src/SmBrep.cpp:4928`-`5012` (`SmBrep::CreateParallelProjectionCurves`, which calls `SmSurface::CreateParallelProjectionCurves` at `:4971`), then `SmSurfaceIntersector` / `SmSurface::LocalPointSolve`; the looping routine is not located.
- **What happens:** With `dThisApproxTol3d = 5e-324` (the smallest subnormal double), the projection runs for more than 15 minutes without returning; `1e-100` returns at once.
- **Trigger:** a caller-supplied subnormal approximation tolerance.
- **How found:** the maintainer's adversarial run in the review of #145 (at `5b1b28ae`).
- **Confirmed:** reported from the C++ reference run in that review; not re-run here (a 15-minute hang).
- **Severity:** hang. Not file-reachable (a caller tolerance argument).
- **Rust port:** unchanged. The `topology.brep_p1` difftest FFI refuses approximation tolerances below the smallest normal double (and the runner reports such cases as rejected on both sides).

#### HANG-39. `SmBrep::CreateCornerBlend` takes tens of seconds or more with a tangency tolerance below what doubles resolve
- **Location:** `source/SMLib/src/SmBrep.cpp:2488`-`2775` -> `SmBSplineSurface::CreateCornerBlend` -> `SmBSplineSurface::CreateCoonsPatch` -> `N_MakeSrfsCompatible` -> `N_SrfInsertKnots` (where a hung run sits).
- **What happens:** The cross-tangent curves are approximated to `dTangencyTolRadians` (`SmTangentField::ApproximateCurve`), which refines towards a tolerance no double computation meets; the over-refined curves then make the Coons patch's surface compatibility insert knots for tens of seconds. Measured on `plane_circ.smb` (three edge uses): `1e-23` returns in 0.4 s, `1e-24` in 1 s, `1e-25` in 22 s, `1e-26` in 47 s; `1e-27` and below, `0` and negative tolerances do not return within 60 s.
- **Trigger:** a corner blend with a tangency tolerance of `0` (difftest fuzz program of `topology.brep_p1`), negative, or below about `1e-24`.
- **How found:** difftest fuzzing of `topology.brep_p1` (the follow-up to #145); located with gdb on the running C++ reference.
- **Confirmed:** yes (C++ reference runs, timed).
- **Severity:** hang (unbounded slowness). A caller argument: no [PSIRT].
- **Rust port:** unchanged. The `topology.brep_p1` shim rejects a tangency tolerance below `1e-20` (four decades of margin above the measured knee) only where its replay of the call reaches the cross-tangent approximation; earlier statuses stay compared.

#### UB-195. `sm_CreateNonRationalCurve` reads the first dropped curve of an empty array [PSIRT]
- **Location:** `source/SMLib/src/SmBSplineSurfaceNLib.cpp:2938`-`2949` (`sm_CreateNonRationalCurve`, rational curve branch), reached from `SmBSplineSurface::CreateCornerBlend` (`:3100`-`3110`) and so from `SmBrep::CreateCornerBlend`.
- **What happens:** For a rational B-spline boundary curve with a neighbour surface, the curve is approximated, reparametrised and dropped onto the surface with `DropCurve`. `DropCurve` can return `SM_SUCCESS` with no curve; `SM_ASSERT(sUVCurves.GetSize() == 1)` is a release no-op and `pNewUVTrimCurve = sUVCurves[0]` reads the array's NULL data (SIGSEGV).
- **Trigger:** a corner blend whose ordered boundary curves include a rational B-spline edge curve of a tangent edge use whose drop onto its face surface yields no curve, with no earlier status stopping the call. The difftest met it on a face of a perturbed closed surface.
- **How found:** difftest fuzzing of `topology.brep_p1` (the follow-up to #145); located with gdb on a `-g` build of the C++ reference.
- **Confirmed:** yes (C++ reference run; the fault is at `:2949` with `m_lSize == 0`, `m_pData == NULL`).
- **Severity:** crash/UB. **[PSIRT]:** reachable from a B-rep read from a `.smb` file through `SmBrep::CreateCornerBlend`.
- **Rust port:** `nurbs::b_spline_surface_n_lib::p1::create_non_rational_curve` returns `InvalidInput` ("no dropped curve"). The `topology.brep_p1` shim refuses exactly those inputs by replaying `SmBrep::CreateCornerBlend` and the curve conversion on a copy of the B-rep up to the drop; the runner reports both sides as the null-dereference guard.

#### UB-163. Uncached healer props removal leaves targets that resize into NULL props holes
- **Location:** `source/SMLib/src/SmHealData.cpp:1332-1333` (`RemoveVertexProps_FromLists` and analogous edge/face helpers), `:1224-1226` (`UpdateTargetLists`); later `Cache_CoinVertices` / `Cache_DegenFaces`.
- **What happens:** removing a props record whose topology pointer has never been cached removes the props but not its target. A later nonempty add list resizes every props array from the target count and initializes only the new-target tail, leaving NULL holes for the unmatched old targets. Native coincidence/degeneracy caching can then dereference invalid state. Even a null add causes that resize. A later `Get*Props_ForTgts` can also index past a props list shortened by an uncached removal.
- **Trigger:** box with all targets, no caches: set done=9, update through 6, remove vertices 0/1 and add vertex 0 twice. Another trigger: remove uncached face 0 and add NULL. With edge/vertex caches, done=12 and update through 9, removing/adding uncached face 0 reaches `Cache_DegenFaces`. A prior `Remove*Props_FromLists` can introduce the same drift.
- **How found:** crash in the C++ reference run during the maintainer's step-33 sequence fuzzing ([PR #164, round 2](https://github.com/tangency-labs/solidcore/pull/164#pullrequestreview-5554396459)); source inspection confirmed the mismatched list sizes and resize boundary.
- **Confirmed:** yes by that reference run: 31 crashing scripts (7 coincidence-cache, 21 harness reads of NULL props, 3 degeneracy-cache). Requires inconsistent caller-controlled healer state; ordinary end-to-end healing was not the trigger.
- **Severity:** crash/UB.

#### UB-182. Building the surface cache of a B-spline surface with a near-zero first v knot span crashes [PSIRT]
- **Location:** `source/SMLib/src/SmNurbsSrf.cpp:1524` `sm_DecomposeSrf` (the v-direction patch loop from `:1665`; the u loop has the `iq >= nsu` quit at `:1653`, the v loop has no counterpart), reached through `SmSurfaceCache::BuildTreeBase` (`SmSurfaceCache.cpp:5042`, `sm_CreateBezierPatchBlock`) from `SmSurface::DropCurve` via `smsurf_GetSurfaceCache`.
- **What happens:** The process dies with SIGSEGV (in `sm_DecomposeSrf`) when the surface cache of a B-spline surface is built.
- **Trigger:** Surface of degree `q = 2` in v with v knots `0,0,0,d,1,1,1` and a UV domain overlapping the v range, `0 < d < 2e-12`; the limit scales with the knot magnitude (about `SM_EFF_ZERO * (1 + max |v knot|)`: `5 + d` crashes below about `7e-12`, `100 + d` below about `1.02e-10`). A tiny span at the end, in the interior or in the u direction does not crash. A knot vector whose start knot has multiplicity above `q + 1` (`0,0,-0.0,1,1` with `q = 1`: `-0.0 == 0.0`) crashes the same way, and so does a u vector with an end knot of multiplicity above `p + 1`; the same excess at the v end did not crash.
- **How found:** crash in the C++ reference run (`--fuzz` of `intersect.surface_drop_lib_p3`, mutation of one v knot).
- **Confirmed:** yes (the reference run and a GDB backtrace); the exact mechanism inside `sm_DecomposeSrf` was not traced.
- **Severity:** crash/UB.
- **Cause:** Not determined beyond `sm_DecomposeSrf`; the missing v-direction counterpart of the `[B262]` check is the likely place.
- **Rust port:** Nothing is emulated. The FFI guard of `intersect.surface_drop_lib_p3` rejects the triggers (first v span zero, or below `SM_EFF_ZERO * (1 + max |v knot|)` with a domain overlapping the v range; a u end knot of multiplicity above `p + 1`). The title says near-zero span; the multiplicity cases share the entry.

#### UB-194. `SM_HISTOGRAM_EDGELEN_INDEX` converts negative doubles to `ULONG`
- **Location:** `source/SMLib/inc/SmTArray.h:2395`-`2397` (macro); used by `SmHealData::Cache_EdgeProps` (`source/SMLib/src/SmHealData.cpp:2523`).
- **What happens:** For an edge length `a` from 1e-5 up to 1, `floor(log10(a))` is -5 to -1, and `(ULONG)` of a negative double is UB. x86-64 g++ wraps, and the `+ 6` wraps back to the intended bucket 1 to 5. aarch64 g++ saturates to 0, so all these edges land in bucket 6 (lengths 1 to 10).
- **Trigger:** `Cache_EdgeProps` on any B-rep with an edge shorter than 1 and at least 1e-5 long.
- **How found:** generated difftest cases of `heal.heal_data_p1`, `heal.heal_data_p2` and `heal.heal_data_p3` that fail on macOS arm64 only.
- **Confirmed:** yes. The C++ reference on macOS arm64 with GCC 13.4 puts these edges in bucket 6 (2026-10-17); the x86-64 build gives buckets 1 to 5.
- **Severity:** crash/UB (wrong histogram bucket in practice).
- **Rust port:** Emulates the x86-64 result (`edge_len_histogram_index`).
- **What happens (PR #237, arm64 Linux):** For `1e-5 <= a < 1` the macro computes `(ULONG)(smos_Floor(smos_Log10(a))) + 6` with a negative double inside the cast. That is UB. x86-64 g++ truncates signed and the `+ 6` wraps back to the intended bucket (0.01 -> 4). AArch64 `fcvtzu` saturates to 0, so every edge shorter than 1 lands in bucket 6 (`[1, 10)`). The sibling `SM_HISTOGRAM_GAPSIZE_INDEX` adds 9 before the cast and is fine.
- **Trigger (PR #237, arm64 Linux):** cache edge props of a B-rep with an edge of length in `[1e-5, 1)`, on an arm64 build.
- **How found (PR #237, arm64 Linux):** arm64 difftest failures of `heal.heal_data_p1`-`p3` (for example `heal.heal_data_p1` `gen0086`, bucket 4: arm64 C++ 0, x86-64 / Rust 4).
- **Confirmed (PR #237, arm64 Linux):** yes, by an arm64 C++ reference run (Ubuntu 24.04, GCC 13.3) and a standalone `fcvtzu` check. x86-64 CI matches the port.

#### UB-178. One-list `SmStitch::StitchVertexPairs` glues a repeated vertex after freeing it
- **Location:** `source/SMLib/src/SmStitch.cpp:3016`-`3047` (coincidence scan), `:3060`-`3190` (glue loop), one-list `StitchVertexPairs`.
- **What happens:** The coincidence scan adds every later list entry within tolerance of `pV` to `sCoinVertices`, so a vertex listed twice lands there twice. Gluing the first copy (`GlueVertices(pV, copy)` or `SqueezeEdge`) deletes it; the second copy is then read (`IsLaminaVertex`, `GetPoint`) and glued after it was freed. Only `pV == sCoinVertices[jj]` (a repeat of `pV` itself) is caught.
- **Trigger:** A vertex repeated in `rVertexList` and coincident with an earlier, different vertex that keeps it as the vertex to delete.
- **How found:** read during porting (`heal.stitch_p2`).
- **Confirmed:** no (source inspection). The difftest FFI refuses repeated vertices. Not file-reachable: the only in-tree caller (`SmBrep.cpp:27427`) passes `pFace->GetVertices`, which has no repeats, so no [PSIRT] tag.
- **Severity:** crash / use after free.
- **Rust port:** Reproduces the order; a deleted vertex id makes the later query return `NotFound` instead of reading freed memory.

#### UB-197. `SmStitch::DoStitching` keeps using an edge its failed glue deleted
- **Location:** `source/SMLib/src/SmStitch.cpp:2182`-`2213` (`DoStitching`, keep-`pTestE` branch of the edge gluing).
- **What happens:** With `m_bIgnoreProblems`, a failing `GlueEdgesGeneral(pTestE, .., pE)` that nevertheless deleted `pE` sets `sVEdges[jj]` to `NULL`, but the local `pE` is not reset: it is added to `sCandidateEdges` and compared with the remaining `sVEdges[kk]` (`GetOtherVertex`, `GetCurve`, `GetInterval`), all through a stale pointer.
- **Trigger:** `m_bIgnoreProblems` (e.g. `StitchIntoShell` with `bShellIsWellFormed == FALSE`), a coincident edge pair keeping the second edge, and a glue that fails after deleting the first.
- **How found:** read during porting (`heal.stitch_p2`).
- **Confirmed:** no (source inspection; `GlueEdgesGeneral` is unported, so no difftest case reaches it). Not file-reachable: `m_bIgnoreProblems` defaults to FALSE (`SmStitch.h:99`) and is set only by an API caller choosing `StitchIntoShell(bShellIsWellFormed=FALSE)` (`SmOffsetExecutive` passes TRUE), so no [PSIRT] tag.
- **Severity:** crash / use after free.
- **Rust port:** Reproduces the control flow; queries on the deleted edge id return `NotFound`.

#### UB-248. `sm_RefineRailCurveClassification` works on interval `ii`, not on small interval `ii`, and can read past the classification
- **Location:** `source/SMLib/src/SmFilletSolver.cpp:5596`-`5690` (`sm_RefineRailCurveClassification`).
- **What happens:** The loop counts `ii` from `crSmallIntervals.GetSize()` down to 1 but never reads `crSmallIntervals`: it refines classification interval `ii` itself (so the small intervals TestRollOver found are ignored unless they happen to be intervals `1..n`). It compares `ii` with the interval count taken before any `RemoveAt`, and a refinable interval `ii = size - 1` reads `rRailCurveClassification[ii + 1]`, past the end; after a removal later indices can also be past the end.
- **Trigger:** `TestRollOver` passing more small intervals than (interval count - 2), or a refinable second-to-last interval.
- **How found:** read during porting (unit `blend.fillet_solver_p3`).
- **Confirmed:** no (the refinement path defers at `RailEdgeuseIntersect`, unit `blend.fillet_solver_p2`).
- **Severity:** wrong result / out-of-bounds read.
- **Rust port:** Faithful loop and indexing; an index past the classification is `InvalidInput` instead of a read past the array.

#### UB-249. `SplitAndTrimFilletSurface` can index past a rail classification
- **Location:** `source/SMLib/src/SmFilletSolver.cpp:4800`-`4866` (`SmFilletSolver::SplitAndTrimFilletSurface`, the interval walk).
- **What happens:** When both current intervals end within `SM_EFF_ZERO_SQ`, both sides advance, but the second branch sets `bFound1 = TRUE` unconditionally after the first branch's search, so a first rail with no further good interval keeps looping with `lStartSearch1` one past its last interval (or on a non-good interval) and reads `(*pCC1)[lStartSearch1]`.
- **Trigger:** Rail classifications whose last good intervals end at the same parameter while the second rail has more good intervals after it.
- **How found:** read during porting (unit `blend.fillet_solver_p3`).
- **Confirmed:** no.
- **Severity:** out-of-bounds read.
- **Rust port:** Faithful flags; an index past the classification is `InvalidInput`.

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
- **Fuzz sweep trigger (2026-10-20):** `N_CreateEllipticalArcAngTypeTol` with a conic domain end of `-1e300` (`topology.proto_topology_p1` `fuzz-out/n2000/topology.proto_topology_p1/fuzz00000.json`; C++ timed out, Confirmed becomes yes). Guard: domain ends within `MAX_CONIC_ANGLE = 1e6` (`crates/solidcore-ffi/src/units/topology_proto_topology_p1.rs:539`).

#### HANG-5. `N_ApproxCrvWithPolyline` subdivides forever below floating-point resolution
- **Location:** `NL_CrvGeom.cpp:1531`, `N_ApproxCrvWithPolyline`.
- **What happens:** The degenerate-segment test `dd < tolsq` never succeeds, so coincident control points keep splitting. With `tol <= 0` the last vertex is also never appended.
- **Trigger:** `tol = 0`; or a relative tolerance on a zero-length curve.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** hang.
- **Rust port:** Stops after `POLYLINE_MAX_SPLITS` with `NotConverged`.
- **Fuzz sweep trigger (2026-10-20):** `N_CreateSweepScale` (`NL_SrfAdv.cpp:15353`, polyline at `:15516` within `E / 2`) with a sweeping tolerance below floating-point resolution of the trajectory: `E = 1e-300`, or a trajectory control point at `±1e300` / `-2^63` (`nlib.srf_adv_p6` `fuzz00381`, `fuzz00486`, `fuzz01019`, `fuzz01105`, `fuzz01236`, `fuzz01549`, `fuzz01582`, `fuzz01615`, `fuzz01772`); `SmBSplineSurface::CreateSweepSurface` with `tol = 1e-300` (`nurbs.b_spline_surface_n_lib_p1` `fuzz00530`); `SmApiCreateSweep` (fixed `dTol = 1e-5`, `SmApiSurfaces.cpp:328`) with a path control point at `±1e300` (`sweep.api_surfaces` `fuzz00106`, `fuzz00357`, `fuzz00476`, `fuzz00668`, `fuzz00746`, `fuzz00805`, `fuzz01000`, `fuzz01115`, `fuzz01151`, `fuzz01999`). Change "Confirmed" to yes (C++ timeouts). The `nlib.srf_adv_p6` and `nurbs.b_spline_surface_n_lib_p1` FFIs require a non-translational sweep tolerance of at least `MIN_SWEEP_TOL_FRACTION = 1e-6` times the trajectory's control polygon length; `sweep.api_surfaces` is not guarded yet (open).

#### HANG-6. Zero-distance G2 curve and surface extensions
- **Location:** `NL_CrvAdv.cpp:640` `ST_curxcdr` (`NL_G2` parameter search); `NL_SrfAdv.cpp:661` `N_SrfExtendByDist` / `:696` `ST_surxsdr` (G1/G2 extend the iso-curves with `ST_curxcdr` `NL_G2`).
- **What happens:** The step `du = range * ldst / d1` is zero, or no longer changes the parameter, so the search never ends. A negative distance reads outside the knot range.
- **Trigger:** `N_SrfExtendByDist(..., dist = 0, NL_G1, ...)`. A small non-zero distance does it too: `SmOffsetSurface::CreateExtendedSurface` by `1.7e-4` with `SM_CT_G1_G2` over a surface of revolution several units across did not finish.
- **How found:** read during porting; generated difftest case (`surface.offset_surface`).
- **Confirmed:** yes (timeout, the small-distance case).
- **Severity:** hang.
- **Rust port:** Fails with `NotConverged` when a step changes nothing.
- **Harness:** `surface.offset_surface` runs the C++ in process and refuses every
  `CreateExtendedSurface` step (in the FFI, and again in the shim), since a stalled or
  crawling search (e.g. `1e-20` on a unit surface, where each step moves) cannot be
  stopped from outside NLib.
- **Fuzz sweep trigger (2026-10-20):** `N_SrfExtendByDist` with `dist` `0.0`, `-0.0`, `±1e-300` and `cont` `NL_G1`/`NL_G2` (`nlib.srf_adv_p1` `fuzz00349`, `fuzz01120`, `fuzz01972`, `fuzz01998`; `nlib.srf_adv_p2` 14 cases, e.g. `fuzz00123`, `fuzz00438`), or a net coordinate of `±1e300` / `2^63` (iso-curve length overflows; `nlib.srf_adv_p1` `fuzz01548`, `nlib.srf_adv_p2` `fuzz00006`, `fuzz00184`, `fuzz00236` and 8 more); `SmBSplineSurface::CreateExtendedSurface` on a surface with a `1e300` control point (`nurbs.b_spline_surface_n_lib_p1` `fuzz01411`, `fuzz01639`). Confirmed: yes (C++ timeouts). The `nlib.srf_adv_p1`/`p2` FFIs require a non-zero distance whose estimated step `range * dist / iso-polygon bound` is at least `MIN_G2_STEP_EPS = 1024` ulps of the largest knot (`g2_extension_ok`); the `nurbs.b_spline_surface_n_lib_p1`/`p2` FFIs bound coordinates by `MAX_COORD = 1e6`.
- **Fuzz sweep trigger (2026-10-20):** a vanishing but non-zero `NL_G2` distance also hangs `ST_curxcdr` (`NL_CrvAdv.cpp:640`). Each step does change the parameter, but the search takes about `arc length / extension` steps. Fuzz `nlib.crv_adv_p1` fuzz00548 (`closedCurve.smc`, `N_CrvExtendByDist`, `NL_RELATIVE`, `dist = 1e-300`, start, `NL_G2`) and fuzz01037 (`crvTrim1.smc`, same). Rust timed out before the bound. The FFI already rejects G2 extensions below `MIN_G2_FRACTION` (1e-3), so these are not confirmed. The Rust port now also stops after `MAX_G2_STEPS` = 100 000 steps with `NotConverged` (`crv_adv/p1.rs:92`, checks `:700`, `:758`).

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
- **Trigger:** `CreateExtendedSurface` with a distance of `0`. Also an infinite distance (`inf / 2` never drops below `inf / 100`): `SmBrep::MateLaminaEdgeToSurface` (`SmBrep.cpp:22248`) extends by `dMaxDist * dSurfaceExtensionFactor`, and `dMaxDist` (the largest `GlobalPointSolve` distance of 21 edge points, `:22216-22229`) is infinite once a point's squared distance to the mating surface overflows (a mating plane offset `1.4e154` from a `10 x 10` plane face or `surf_extrude.smb`; `1e154` returns), or the product overflows (`1e154 * 1e300`).
- **How found:** read during porting; the infinite case by `topology.brep_p2`'s `difftest --fuzz 2000` (PR #149: three timeouts, mating plane origin `z = +-1e300`).
- **Confirmed:** the infinite case yes, by bisecting the offset (C++ returns at `1e154` in 0.9 s and never at `1e160` .. `1e300`, killed after 30 s; no gdb on the pod). The Rust port on the same inputs, with `SmFace::Notify` stubbed out to reach the extension, stops in `create_extended_surface_dir` with `NotConverged` after 1100 halvings from `1.4e154` on, which places the loop here.
- **Severity:** hang.
- **Rust port:** Stops after 1100 halvings with `NotConverged`. The `topology.brep_p2` FFI refuses a `Mate` whose probed `dMaxDist * factor` is not finite.

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

#### HANG-29. `SmCurve::GlobalCoincidenceChecker` splits a face boundary forever when a surface control point is huge
- **Number:** was HANG-16 in PR #147 until #175 took that id on main.
- **Location:** `SmCurve.cpp:19950` `SmCurve::GlobalCoincidenceChecker` (closed / half-closed split into thirds; the only recursion guard is the parameter length against `SM_ZONE_TOL_3D/10`), reached from `sm_ClassifyFacePoint` through `SmFace::CalculateUVDomainFromUVTrimCurves` -> `SmEdgeuse::CreateUVTrimCurve` -> `SmSurface::DropAndTrimCurve` -> `SmSurfaceDropCurve::FindBoundaryStartPoints` -> `SmCurve::GlobalCurveIntersect`.
- **What happens:** With huge coordinates, squared distances lose precision or overflow, so the closedness / polar-box tests keep asking for a split and every third is split again; the parameter-length guard is reached late or never. Non-termination is established only where squared distances overflow to infinity: on a 3 x 4 surface with one control point at `1e153` the call returns in 90 ms and at `1e154` never (an hour on one core). Below that the call terminates but is slow in a shape-dependent way from about `1e15` on, and uniform scaling triggers it as much as a single outlier does: on a 4 x 3 cubic-by-quadratic fixture (the pre-review's shape), one control point of `-1e15` in all three coordinates takes 54 s, `-1e20` 14 s, `-1e40` 10 s, `-1e50` 12 s, while the same magnitude in one coordinate only takes 0.14-0.20 s and `-1e10` takes 5 ms; both surfaces and the point scaled uniformly by `1e30`, `1e45` or `1e49` take 43-55 s, scaled by `1e10` 3 ms; a homogeneous `9.9e39` over a weight of `1e-10` (Euclidean `9.9e49`) 37 s; one control point at `-1e80` 10 s, `-1e100` 23 s, `-1e120`..`-1e150` 70-99 s; a homogeneous `1e150` under a weight of `1e-150` 34 s. A random sweep over the two ops reaching it found 14 calls in 40,000 above 2 s (worst 38 s), every one with a value of at least `3e15`, and none above 120 s.
- **Trigger:** `sm_ClassifyFacePoint` on a face whose surface has coordinates of magnitude `1e154` or more (never returns on the shape above); slow from about `1e15`.
- **How found:** `fuzz_guards` sweep of the `blend.fillet_geom_p1` FFI guards (PR #147), located with gdb; the weight bypass by the PR's pre-review; the shape dependence and the `1e15` onset by its re-review (timing probes and a 120 s-per-call fuzz).
- **Confirmed:** yes (gdb backtrace 25 frames deep in `GlobalCoincidenceChecker`; never returned; timings measured in the reference build).
- **Severity:** hang (above `1e154`); seconds to about a minute per call from `1e15`.
- **Rust port:** The FFI wrapper rejects, for the ops that reach `sm_ClassifyFacePoint` (`ClassifyFacePoint`, `FbSplitFace`), any value and any Euclidean control point (`Pw / w`) above `MAX_MAGNITUDE = 1e150`, the overflow regime (see `crates/solidcore-ffi/src/units/blend_fillet_geom_p1.rs`); slow inputs under it are valid upstream inputs and are accepted (the guard fuzz allows those two ops 180 s per call). The other ops take any finite value. The Rust `classify_face_point` still defers on `topology.face_p1`.
- **Fuzz sweep trigger (2026-10-20):** B-rep blocks with one surface control point coordinate at `+-1e300` time out in the C++ while the B-rep is built (call not isolated; consistent with HANG-29's `CreateUVTrimCurve` path) in `topology.edgeuse_p2` (6), `topology.edgeuse_p3` (8), `topology.face_p2` (3), `topology.face_p3` (3), `topology.loopuse` (1), `heal.heal_data_p1` (4) and `offset.offset_executive_p1` (2) (case files under `fuzz-out/n2000/<unit>/`, see the findings). Confirmed: yes for the hang (20 s plus retry); the location is not confirmed for these paths.

#### HANG-20. The global solvers never finish when a surface coordinate's own box arithmetic overflows
- **Location:** `SmGlobalSolver::SolveBranch` with each of its local solvers: `SmCSIGlobalSolver::LocalSolve` -> `SmCurve::SimpleCoincidenceChecker` -> `SmCurve::LocalPointSolve` -> `SmLocalSolve1d::SolveIt` (gdb after 25 s on the cliff program: eleven `SolveBranch` frames, the time in `LocalSolve`), reached from `SmSurface::GlobalCurveIntersect` (`SmFilletVertex::CalcCliffRailIntEdge`), from `SmSurface::GlobalPointSolve` (`sm_SurfaceDropPoint`) and from `SmSurface::PointBasedSurfaceIntersect` (`sm_TwoPntsIntersection`), all in `SmFilletGeom.cpp`.
- **What happens:** With a surface control point coordinate in the top decade of the double range the solve never returns, while the same call with the coordinate a little smaller returns `SM_ERR` in 1-3 ms (as it does at `-1e300`, `-1e200`, `-1e150`, `-1e100` and `-1`). Measured by replaying the three fuzz programs with the one coordinate substituted: the curve/surface solve returns at `-7e307` and never at `-8e307`, `-1.7e308` or `+-DBL_MAX` (in x, y or z alike); the point/surface solve on a bilinear surface returns at `-3e307` and never at `-7e307`; the two-point intersection, whose surface is rational (`w = 1.51`), returns at a homogeneous `-2e307` and never at `-3e307`. The onsets are a small factor (about 2.25 to 9) under `DBL_MAX`, so it is the coordinate itself overflowing in the solvers' box or step arithmetic, after which the branch tree never narrows; it is not HANG-29's squared-distance overflow, which starts near `1e154` and which these calls pass through unharmed.
- **Trigger:** any of those solves with a surface coordinate above roughly `2e307`..`8e307` (shape-dependent; the fuzz placed `NL_NOW` markers, `-DBL_MAX`, into coordinates).
- **How found:** `fuzz_guards` of the `blend.fillet_geom_p1` FFI with the re-review's widened specials (PR #147 re-review round: seed 4242 call 30194 `FbCliff`, seed 777 call 17007 `SurfaceDropPoint`, seed 4242 call 14995 `TwoPnts`), thresholds by replay (`replay_program`), loop located with gdb.
- **Confirmed:** yes (reference build; 45-180 s without returning, gdb backtrace).
- **Severity:** hang.
- **Rust port:** the FFI wrapper bounds every value (control points as Euclidean `Pw / w`) of a `SurfaceDropPoint`, `TwoPnts`, `FbVertOnVert` or `FbCliff` input by `MAX_SOLVER_MAGNITUDE = 1e307`, under every measured hang and at or under every measured return (`crates/solidcore-ffi/src/units/blend_fillet_geom_p1.rs`); the Rust cliff still defers on `intersect.surface_drop_lib_p1`, the point drops compare. The Rust solver is a faithful port and loops the same way: the PR #147 round-6 valgrind sample ran the rejected fixture `two_pnts_hang20_over_bound_rejected` on the Rust side alone and it had not returned after 300 s natively (the difftest never runs an input the FFI refuses on the Rust side: the runner applies the FFI's own validation first, flagged or not, after fuzz mutations that unflagged these fixtures timed out in Rust). A library-level bound belongs to the global solver's owning unit.

#### HANG-15. Tiny surfaces build a surface cache of millions of nodes
- **Location:** `SmSurfaceCache.cpp:4495` `SmSurfaceCache::SubdivideToTolerances` (stops only at `m_dMinimumSideLengthRatioUV`, `:3932`-`:3949`), through `smsurf_GetSurfaceCache` from `SmSurface::GlobalLineIntersect` / `GlobalCurveIntersect`.
- **What happens:** The cache tolerances are absolute. A noisy cubic B-spline surface about `5e-4` across fails the angle test down to the minimum side ratio in both directions, so every patch splits to about `2^20` leaves.
- **Trigger:** `GlobalCurveIntersect` of a line with a 5 x 6 net of scale `1e-4` (control points jittered by 20 %, z noise `1e-8`), sliver domain `[1.6112222, -0.986, 1.6112223, 1.07]`: 15 s in the C++, 121 s in the port. The same net scaled by `1e3` takes 4 ms.
- **How found:** generated difftest case (`surface.surface_p8`; the generator seemed to hang).
- **Confirmed:** yes (C++ 15.4 s, Rust 121.5 s for the one case).
- **Severity:** hang (minutes per call; bounded by the minimum side ratio).
- **Rust port:** Same bound (`MAX_SUBDIVISION_DEPTH` does not limit the node count). The generator uses scale `1e-3` instead of `1e-4`.

#### HANG-18. `MateLaminaEdgeToSurface` with a negative extension factor never returns
- **Location:** `NL_CrvAdv.cpp:640` `ST_curxcdr` (the G2 mirror-segment searches, `while( NL_YES )` at `:748` and `:839`), through `N_CrvExtendByDist` <- `ST_surxsdr` <- `N_SrfExtendByDist` <- `SmBSplineSurface::CreateExtendedSurface` <- `SmBrep::MateLaminaEdgeToSurface` (`SmBrep.cpp:22248`, `CreateExtendedSurface(ctx, dMaxDist*dSurfaceExtensionFactor, SM_CT_G1R, ..)`).
- **What happens:** Nothing rejects a negative distance. With `ldst < 0` the parameter step `du = (U[m]-U[0])*(ldst/d1)` is negative, so the search walks the parameter off the knot range (only one side is clamped) and its exits compare arc lengths against the negative `ldst` (`fabs(ldst - dr) < ldst * 0.05` is never true). Distinct from the long positive extensions of the offset hang (swarm registry HANG-17), which split and recurse.
- **Trigger:** `MateLaminaEdgeToSurface` with `dSurfaceExtensionFactor = -1` on a lamina edge whose points are off the mating surface: `Endcap.smb` edge 0, and edges 0 and 1 of a `10 x 10` plane face, mating plane `z = 0.5`, `dUVExtensionFactor = 10`, editing enabled. Factors `0`, `1.5` (the only upstream use) and `1e6` return normally.
- **How found:** `topology.brep_p2` pre-review guard fuzz (PR #149).
- **Confirmed:** yes (gdb backtrace inside `ST_curxcdr` <- `N_CrvExtendByDist` after 90 s; killed after 40 min in the sweep). The loop mechanism is from source inspection.
- **Severity:** hang.
- **Rust port:** `topology::brep::p2::mate_lamina_edge_to_surface` returns `InvalidInput` for a negative extension distance before `create_extended_surface`; the difftest FFI rejects a negative factor (`topology.brep_p2` runner test `mate_rejects_a_negative_extension_factor`).

#### HANG-28. `SmVertexuse::GetVertexFaceGap` never returns for a curved face with a huge corner
- **Location:** `SmVertexuse.cpp:1646` `SmVertexuse::GetVertexFaceGap` -> `CalcVertexFaceGap` (the vertex's drop onto the face surface).
- **What happens:** the gap of a face vertex never finishes computing; the exact loop is not identified. The cost climbs steeply with the corner's magnitude: there is a slow band below the first cut-off (`2e156`) recorded here.
- **Trigger:** `CreateFaceFromSurface` on a u-degree-2, v-degree-1 B-spline with three u spans (5 x 2 net, knots `0,0,0,.33,.66,1,1,1`) whose corner control point (a vertex position) has a large coordinate, then `GetVertexFaceGap` on its vertex uses (the shared topology fixture's `init_gaps`). Measured on the pod (30 s timeouts; u-degree 2 and 3, 3 and 6 spans, v-degree 1 and 2, last corner on x, y and z): up to `1.34e154` (about `sqrt(DBL_MAX)`) every shape returns within 0.09 s; from `1e155` the v-degree-1 shapes take 0.3-1.1 s; at `1e156` six do not return within 30 s (five on x or y, one on z), at `1.9e156` nine; `1.9e156` takes 103-114 s on z and does not return within 120 s on x (review of d12f88e9, `reviews/topology.brep_p2-d12f88e9.md`); `5e156` never returns. The v-degree-2 shapes measured stay fast at `1.9e156`. Weights (review of f3fe6094): the net is homogeneous (`w x, w y, w z, w`), so a rational corner's vertex is the stored value divided by `w`; a stored `1e154` with `w = 1e-3` (a vertex at `1e157`) does not return within 30 s, and a Cartesian `9.9e153` with `w = 10` (stored `9.9e154`) did not return on y. Below that, a rational corner with a large coordinate costs a bounded 0.6-3.9 s on x whatever its magnitude (`1e100` as `1e151`), under 0.1 s on y and z; the other control points' weights (`1e-3`, `1e3`) do not change this. The FFI bounds a rational corner at `1e151` on both the stored and the Cartesian coordinate. The same corner returns up to `1e300` on planes, degree-1 surfaces, single- and two-span biquadratic patches, and when the huge value is an interior control point.
- **How found:** `topology.brep_p2`'s `difftest --fuzz 2000` (PR #149, case fuzz00497: control point `y = 1e300`, a `LocalCurveSector` op; plain queries on the same B-rep hang too).
- **Confirmed:** yes, by C++ runs with 30-120 s timeouts and an `fprintf` trace inside the fixture build, which stops inside the third vertex use's `GetVertexFaceGap`. The pod cannot ptrace (`ptrace_scope` 2, no `CAP_SYS_PTRACE`), so gdb cannot attach and the inner loop is not pinned down (the reviewer suspects `ComputeUVPoint`'s drop, `SmVertexuse.cpp:1817`).
- **Severity:** hang.
- **Rust port:** the Rust side returns (no gap initialisation in the difftest import). The `topology.brep_p2` FFI refuses a degree >= 2 B-spline face with a corner control point coordinate above `1e154` (a rational corner above `1e151`, stored or Cartesian) when `init_gaps` is set (a margin below the slow band; the first bound, `2e156`, was inside it); this over-refuses the shapes that return (v-degree 2, single- and two-span patches).

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

#### HANG-38. `SmSurface::DropCurve` can subdivide the same span forever
- **Location:** `source/SMLib/src/SmSurfaceDropLib.cpp:5637` `SmSurface::DropCurve`, the span stack loop: a span that fails `TestSpanAccuracy` is split at `(sStart.m_dT + sEnd.m_dT) / 2.0` (`:5937`) and the split point pushed (`:6014`); a failing span is only accepted once it is shorter than `sInterval.GetLength() / 8000.0` (`:5824`).
- **What happens:** Nothing bounds the stack. When `1/8000` of the interval is at most one ulp of the span ends, a span of one ulp neither passes that test nor shrinks: the midpoint rounds to `sEnd.m_dT`, `GetNextPoint` succeeds there, and the same point is pushed again on every iteration, so the stack grows until memory runs out.
- **Trigger:** A non-degenerate curve interval of length `L` with `L / 8000 <= ulp(max |end|)` (some float spacing inside the interval is at least `L / 8000`) over a span that fails `TestSpanAccuracy`. A zero-length interval is not a trigger: `IsDegenerate` is `TRUE` on it, so no drop point gets `m_eDropDerivRtn == SM_SUCCESS` (`SmSurfaceDropLib.cpp:1894`-`1931`) and the keep-all rescue (`:5514`) needs a deviation below the tolerance.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** hang.
- **Rust port:** `intersect::surface_drop_lib::p3` bounds the stack at `MAX_SPAN_DEPTH` (64) drop points and returns `SolidError::NotConverged` past it (exact arithmetic needs about 15). The FFI rejects a non-degenerate interval (ends normalised, as `SmExtent1d` swaps them) of length `L` with `L / 8000 <= 2 ulp(max |end|)` (about 16000 ulps, twice the margin) and nothing else: zero-length, inverted and out-of-range intervals and any tolerance (zero and negative included) are accepted.

#### HANG-31. `SmApiCreateCylindricalBox` hangs on nearly degenerate arcs and full turns
- **Location:** `source/SM_API/src/SmApiPrimitives.cpp:766` `SmApiCreateCylindricalBox` -> `source/SMLib/src/SmPrimitiveCreation.cpp:424` `SmPrimitiveCreation::CreateCylindricalBox`, which builds the four boundary arcs with the three-point `SmCircle(start, mid, end)` constructor.
- **What happens:** Inputs the API's validation accepts never return (over 60 s) when an arc nearly degenerates: a sweep of exactly 360 degrees or within about `1e-12` of it (start and end points coincide), a short or flat inside arc, or an origin far from the arcs. An origin of `1e300` segfaults.
- **Trigger:** angles `0 .. 360`, `-90 .. 270`, `360 .. 720`, `0 .. 360 - 1e-12` (`0 .. 360 - 1e-8` returns); inside radius `1`, angles `0 .. 0.01` (`0 .. 0.012` returns); inside radius `1e-3`, angles `10 .. 10.2`; inside radius `1e-2`, angles `0 .. 0.13`; origin `(1e17, 0, 0)` or non-finite, radii `1, 2`, angles `0 .. 90`.
- **How found:** hang in the C++ reference run (primitive.api_primitives fixtures and probes).
- **Confirmed:** yes.
- **Severity:** hang (crash for the `1e300` origin).
- **Rust port:** not reached yet (deferred); the difftest guard passes on only inputs with an inside-arc chord of at least `1e-4`, a sagitta of at least `1e-8`, a sweep at least `1e-8` degrees short of a full turn and a finite origin within `1e6`.

#### HANG-16. Direct negative-distance surface extension moves away from its loop's exit
- **Location:** `source/SMLib/src/SmBSplineSurfaceNLib.cpp:4583-4591`, with equivalent loops at `:4645`, `:4712`, `:4773`, in `SmBSplineSurface::CreateExtendedSurface`.
- **What happens:** After a successful extension, `while (dExtDist < dDist/2.0)` doubles `dExtDist`. For a negative request, the doubled negative value moves away from the exit condition. The lower-level G1 path can accept a negative request, so success does not necessarily prevent entry into this loop.
- **Trigger:** Calling the surface-extension routine directly with a negative distance. The initial PR #175 probe manufactured this condition by giving an executive's geometry creator `-0.125`, producing an extension request of `-0.625`. This is **outside the offset executive's internal input contract**, not a reachable negative-distance path through the offset/shell entry points.
- **Reachability correction (PR #175 review):** `SmOffsetExecutive::OffsetBrep` (`SmOffsetExecutive.cpp:114-120`) and `ShellBrep` (`:206-223`) construct their geometry creators with `fabs(dOffsetDistance)` and carry the sign using `SetInset`. The local-operation caller (`SmBrep.cpp:12004-12008`) uses half a bounding-sphere radius. Consequently the executive's extension path receives a nonnegative magnitude. Negative public offset distances remain valid insets.
- **How found:** out-of-contract native extension probes during `offset.offset_executive_p1`, followed by maintainer review of the real callers.
- **Confirmed:** yes for eight-second timeouts in the initial out-of-contract native probes (four fixed inputs and 104 generated inputs, Linux x86_64 / GNU g++ 16.2.1). Infinite execution and the exact stuck instruction were not experimentally established; the divergent loop condition was confirmed by source inspection.
- **Severity:** hang on a direct negative extension request; not an offset-unit parity blocker.
- **Rust port:** The offset executive's safe FFI and native adapter reject negative internal extension magnitudes before constructing native geometry. Zero, signed zero and positive magnitudes remain covered. This guard does not reject signed public inset operations or readonly constructor/getter probes. See `plan/findings.md`. The Rust library itself (`BSplineSurface::create_extended_surface_dir`, also reached by `OffsetExecutive::get_create_extended_surface` with a negative distance) now bounds the doubling loop by `MAX_EXTEND_DOUBLINGS` (1101; a positive distance needs at most one more doubling than it had halvings) and returns `NotConverged`; before that the port looped as upstream does (Rust reached `dExtDist = -inf`, where every G1R extension of a plane still succeeds; fuzz `offset.offset_executive_p1` `/offset_distance = -1.0` / `-1e300`, 2026-10-21).

#### HANG-17. `ST_surxsdr` (surface extension by distance) explores an exponential split for surfaces with short iso-curves
- **Location:** `source/SMLib/src/NL_SrfAdv.cpp:696`-`960`, `ST_surxsdr` (the recursive body of `N_SrfExtendByDist`), reached through `SmBSplineSurface::CreateExtendedSurface(SM_SP_BOTH, 5 * offset, SM_CT_G1R)` from `SmOffsetExecutive::GetCreateExtendedSurface` (`SmOffsetExecutive.cpp`).
- **What happens:** The routine extends five sample iso-curves by the requested distance and averages their extension parameters. When that average exceeds `0.9313` of the surface's knot range it splits the distance and calls itself twice per level (`dist - d2` on the original, then `d2` on the result), with the depth only limited to 10. Each call extracts and extends five iso-curves of a surface that grows at every level, so a surface whose iso-curves are short relative to the distance (closed iso-curves of a surface of revolution extended by five offset distances) runs through up to `2^11` surface extensions.
- **Trigger:** `SmOffsetGeometryCreation::EdgeuseOffset` with `GetExtendConvexEdges()` (or `ExtendTrimEdges` / `MateEdges` / the extended-surface candidates of `TrimExtendedEdge`) on `pt_TestFiles/cone.smb`, `torus.smb` or `Endcap.smb` at offset distances `0.1`-`0.5`; `box_with_large_fillet.smb` and `Mouse.smb` (open iso-curves) return at once.
- **How found:** the first difftest run of `offset.offset_geometry_creation` (PR #156) stalled for 28 minutes; two gdb backtraces 20 s apart sat inside `N_SrfExtractIsoCrv` / `N_CrvExtendByDist` under `ST_surxsdr`; the built-in `*_edgeuse_offset_extend` cases reproduce it alone (cone, torus, endcap time out at 120 s; box, mouse finish).
- **Confirmed:** runs for more than 28 minutes (13.5 minutes of CPU) and was killed; not proven infinite (the recursion is bounded by depth 10, so it may be exponential rather than endless).
- **Severity:** hang in practice.
- **See also:** a negative extension distance makes the per-side doubling loop of `SmBSplineSurface::CreateExtendedSurface` (`SmBSplineSurfaceNLib.cpp:4583`) itself unbounded on any surface, planes included: HANG-16 above ("Direct negative-distance surface extension moves away from its loop's exit"). PR #156 met it through the same `GetCreateExtendedSurface` call on `plane_circ.smb`, `ellips_ts2.smb` and `sphere.smb` + `box_with_large_fillet.smb`, with the geometry creation itself given offset -0.25 (gdb backtraces in `ST_surxsdr` under `CreateExtendedSurface`). As HANG-16 records, real callers construct the geometry creation with `fabs(dOffsetDistance)`, so that input is outside the executive's internal contract.
- **Rust port:** `GetCreateExtendedSurface` is wired to `offset.offset_executive_p1`'s `OffsetExecutive::get_create_extended_surface`. The difftest shim seeds the executive's `m_vSurfExtSurf` cache before every op with each face surface of both B-reps and each store surface mapped to itself (`GetCreateExtendedSurface` returns a cached surface without extending, `SmOffsetExecutive.cpp:2123`), and the Rust runner seeds the port's cache the same way (`insert_extended_surface`), so neither this nor the negative-distance loop runs on either side and no guard status is needed.

#### HANG-26. The offset self-intersection of a surface of revolution's offset takes minutes or does not return
- **Location:** `SmOffsetGeometryCreation::FaceuseOffset` (`SmOffsetGeometryCreation.cpp`) -> `SmSurfOfRevolution::GlobalSurfaceIntersect` -> `SmSurface::GlobalSurfaceIntersect` -> `SmSurfaceIntersector::DoIntersection`, then either `SmAdvSurfaceIntersector::FindInteriorCurves` -> `SmSurface::LocalSurfaceIntersect` (`SmLocalSolveNd`, `SmSSINormalizeENFO`) or `SmGlobalSolver::SolveIt` -> `SmGlobalSolver::SolveBranch` recursing (over 35 frames) into `SmAdvSurfaceIntersector::LocalSolve` / `SmPseudoBox::AreDisjoint`.
- **What happens:** The self-intersection of the offset of an `SmSurfOfRevolution` face is slow at ordinary distances and erratic at large ones. Measured alone on the reference build: `vase_rev.smb` 19 s at distance 10, 24 s at 100, no return within 120 s at 1e3, 76 s at 1e4, 39 s at 1e5, 85 s at 1e6, 64 s at 1e7, no return within 120 s at 1e8 and 1e10 nor within 90 s at 1e200 to 1e300; at 1e50 to 1e150 the process exhausts a 4 GiB address-space limit (`std::bad_alloc`); `toruswvy.smb` takes 60 s at distance 2. Exact planes and plain B-spline surfaces return in under a second at every distance from 1e3 to 1e300.
- **Trigger:** `FaceuseOffset` of a face whose surface is an `SmSurfOfRevolution` (`vase_rev.smb`, `toruswvy.smb`), at any offset distance; the `difftest --fuzz` mutation `dist = 1e300` (fuzz01169) and the generated scripts gen0154 and gen0696 at ordinary distances.
- **How found:** `difftest --unit offset.offset_geometry_creation --fuzz 2000` (PR #156) reported timeouts; gdb backtraces after 12, 24 and 40 s; replays with the distance substituted.
- **Confirmed:** yes (reference build; gdb backtraces, timed replays). Not proven infinite: the recursion is bounded, but no run at the distances above returned within the limits given.
- **Severity:** hang in practice; memory exhaustion at some distances.
- **Rust port:** the difftest runner rejects a script whose Rust run reaches a non-local `FaceuseOffset` of an exact `SmSurfOfRevolution` face with self-intersection enabled (`Output::rejected` naming HANG-26, decided before either side runs); the shim keeps its own stop there (status `-6`, the script stops) as a safety net. HANG-20 (`blend.fillet_geom_p1`) is a different regime of the same solver (coordinates near the double range's overflow).

#### HANG-27. `SmCrvOnSurf::ApproximateCurve` of an offset-surface rail grows with the offset distance and stops returning
- **Location:** `SmOffsetGeometryCreation::EdgeuseOffset` (the offset-surface rail when no offset edge use is given) -> `SmCrvOnSurf::ApproximateCurve` -> `SmCrvOnSurf::GetOrCreateBSApproxPointer` -> `SmCurve::ApproximateCurve` -> `SmCurve::LocalPointSolve` -> `SmLocalSolve1d::SolveIt` -> `SmFindPCExtremaEFO::Evaluate` (gdb after 40 s).
- **What happens:** On the `Mouse.smb` shell fixture (`mouse_edgeuse_offset_shell`) the op takes 1 s at offset distance 1e4, 7 s at 1e5, 10 s at 1e6, 15 s at 1e7, 28 s at 1e8, 30 s at 1e10, 39-75 s at 1e50 to 1e200, 66 s at 1e280, and does not return within 90 s at 1e250 or 1e300 (either sign).
- **Trigger:** `EdgeuseOffset` with a huge offset distance; the `difftest --fuzz` mutation `dist = -1e300` (fuzz01796).
- **How found:** `difftest --fuzz 2000` of PR #156; gdb backtrace; replays with the distance substituted.
- **Confirmed:** yes (reference build). Not proven infinite.
- **Severity:** hang in practice.
- **Rust port:** the port builds the rail's `SmOffsetSurface` and defers at the curve on it (`UNPORTED(curve.crv_on_surf)`: the ported `CrvOnSurf` holds a B-spline surface only). The difftest shim and runner treat an `EdgeuseOffset` with `|distance| > 1e6` as a hang guard (the runner rejects a script whose Rust run reaches it, naming HANG-27; the shim keeps its own stop there, status `-6`, as a safety net). The bound is chosen for the fuzz time budget (20 s per side), not the termination boundary (about 1e250); the generator's distances (0.001 to 100) are far below it.

#### HANG-21. The surface cache of an offset of a small trimmed surface does not finish
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:337` (`SmSurface::CalculateBoundingBox` on the offset), `smsurf_GetSurfaceCache` and the `SmSurfaceCache` tree build.
- **What happens:** A box domain outside the trimmed base domain builds the offset's surface cache. For a base about `1e-3` across trimmed to a small part, the build did not finish within 15 s (other cases take milliseconds). The cause was not analysed.
- **Trigger:** A B-spline about `1e-3` across, offset by `7.8e-4`, `TrimWithDomain` to a small part of its domain, then `CalculateBoundingBox` over the original domain.
- **How found:** generated difftest case (`surface.offset_surface`, 30,000-seed run).
- **Confirmed:** yes (timeout).
- **Severity:** hang.
- **Rust port:** Waits on `surface.surface_p6` (the cache tree); the harness keeps these boxes away from the C++ side.

#### HANG-22. G2 extension retries enter an arc-length search with NaN
- **Location:** `source/SMLib/src/SmBSplineSurfaceNLib.cpp:4568-4576` (in-place retry); `source/SMLib/src/NL_CrvAdv.cpp:683-760` (`ST_curxcdr`).
- **What happens:** After failed in-place surface extensions, an intermediate rational iso-curve has zero endpoint weights. `N_CrvArcLength` reports success with a NaN length. `ST_curxcdr` derives NaN parameter increments and its termination comparisons never succeed. The retry is the in-place one of UB-107 (it reads the arrays a failed in-place extension left half-written), so this hang is one outcome of that read; restoring the surface before the retry has not been verified as a fix.
- **Trigger:** PR #62 original generated `gen5909`: offset of a revolution surface, trim, G2 extension by `0.6075400573597457`. The 60 exact flattened input doubles are retained in `crates/solidcore-ffi/tests/data/offset-gen5909.txt`.
- **How found:** Extended-seed difftest, then standalone cold C++ reproduction and debugger inspection. A fresh local context also hangs, so earlier cases and context reuse are not necessary.
- **Confirmed:** yes. Unmodified C++ times out; debugger captured `d1`, `du`, and search parameters as NaN.
- **Severity:** hang.
- **Harness:** `surface.offset_surface` refuses every `CreateExtendedSurface` step in process (see HANG-6), so this input stops at its extension step; an FFI test replays the exact input and checks the refusal. These are unsafe-reference exclusions, not passing difftest cases.
- **Fuzz sweep trigger (2026-10-20):** the NaN arc-length search is also reached directly through `N_CrvExtendByDist`, with no surface retry: fuzz `nlib.crv_adv_p1` fuzz01836, a rational quadratic with 5 control points and weights `[0, 2, 1, 0.5, 1]` (`/crv/cpts/0/3 = 0`), relative `dist = 3.0`, end, `NL_G2`. Rust timed out. The FFI rejects non-positive weights, so this is not confirmed. Rust stops at `MAX_G2_STEPS`.

#### HANG-34. `SmBlendCurveCrossSectionFSG::CreateSurfaceFromCurves` loops forever when a knot insertion adds no knot
- **Location:** `source/SMLib/src/SmFilletStandardSolver.cpp:4090` (`while (sKnots.GetSize() < 4)`: the mid-span knot is inserted into both rails and the knots are re-read). `SmCircularCrossSectionFSG::CreateSurfaceFromCurves` has the same loop with a bail-out (`:3336`).
- **What happens:** When `InsertOneKnot` fails or the mid-span value equals an existing knot (spans below the knot resolution), the knot count never reaches 4.
- **Trigger:** Rails whose largest knot span is too small to split.
- **How found:** read during porting.
- **Confirmed:** no. The `blend.fillet_standard_solver_p1` shim's dry run rejects such inputs.
- **Severity:** hang.
- **Rust port:** `SolidError::InvalidInput` when an insertion adds no knot.
#### HANG-25. `IntersectInsertRelate` does not return for a zero approximation tolerance
- **Location:** `source/SMLib/src/SmTopologyIntersector.cpp:1092-1270`, `SmTopologyIntersector::IntersectInsertRelate` (the full topology intersection at `m_dThisApproxTol3d`).
- **What happens:** With `m_dThisApproxTol3d` equal to `+0.0` or `-0.0`, the topology intersection did not finish within 180 s. Negative tolerances and tiny positive ones (down to the smallest subnormal) return within a second.
- **Trigger:** `SetThisApproxTol3d(0)`, then `IntersectInsertRelate` on two intersecting B-reps.
- **How found:** pre-review of PR #203 (the FFI's guard bypassed, timed C++ runs).
- **Confirmed:** yes (C++ run: no result after 180 s at `0` and `-0`; `-1e-5`, `-1e300`, `1e-12`, `1e-200` and `5e-324` return).
- **Severity:** hang (needs a bad argument from the calling code).
- **Rust port:** The callee loops are unported (`IntersectTopology` defers at `SmBrep::GetFaces`). The FFI refuses `IntersectInsertRelate` at a zero tolerance.

#### HANG-23. Polar set-up of a circle with a far STEP interval shifts one turn per iteration
- **Location:** `source/SMLib/src/SmEllipse.cpp:278-289` (`SmPolarConversion::SetUpPolarConversion`, "move to period of greatest overlap").
- **What happens:** Two `while` loops move the NURB's start/end angles by 360 degrees per iteration until their midpoint is within 180 degrees of the STEP interval's. The run time is proportional to the distance, and once the midpoint exceeds about `2^61` degrees adding 360 no longer changes it, so the loop never ends.
- **Trigger:** An `SmCone` built with start and end angles `9223372036854775807` (any cone/cone or cone/plane intersection through `MakeNurb`, which rebuilds the polar converter of its iso circles).
- **How found:** `surface.cone_p3` fuzzing (issue #207, fuzz02755: the Rust side timed out), then a native-only reproduction through `sc_surface_cone_p3_intersect` on Linux.
- **Confirmed:** yes (the native call produced no result within 60 s).
- **Severity:** hang.
- **Rust port:** Returns `NotConverged` after `curve::ellipse::p2::MAX_PERIOD_SHIFTS` (2^20) shifts or when a shift no longer changes the midpoint; the callers ignore that status as upstream ignores the set-up's, leaving the converter impossible.
- **Harness:** the `surface.cone_p3` FFI accepts cone angles only within upstream's documented `[-360, 360]` (`SmCone.h:205-206`). A first guard at about `3.8e8` degrees (the Rust bound) still let fuzzed intersections run past 20 s natively: every surface evaluation of such a cone goes through `SmEllipse::STEPInversion` and `SmExtent1d::PeriodicWrap`, one turn per iteration (HANG-1), confirmed with gdb backtraces.
- **Fuzz sweep trigger (2026-10-20):** `SmSurfOfExtrusion::AdjustSTEPUVDomain` of an ellipse extrusion to a domain whose u end is `9223372036854775807` degrees (`surface.surf_of_extrusion_p1` `fuzz00222`, `fuzz01418`); a revolution (sphere part / torus generator circle) whose v domain bound is `±2^63` or `-1e300` degrees, through `DropPoint` / `ConvertUVFromSTEPToNURBS` (`surface.surf_of_revolution_p3` `fuzz00476`, `fuzz01129`, `fuzz01853`). Confirmed: yes (C++ timeouts; the loop was not traced, so HANG-1 `PeriodicWrap` may be the one spinning). Not guarded yet (open).
#### HANG-35. Closed shells a few zone tolerances across take minutes to place in their region
- **Location:** `source/SMLib/src/SmBrep.cpp:8666` `SmBrep::FindAndSetInfiniteRegion` -> `:8004` `FindInfiniteRegion` (ray fire at `:8417`) -> `source/SMLib/src/SmShell.cpp:592` `SmShell::RayFire` -> `SmSurface::GlobalLineIntersect`. The solver tree (`SmGlobalSolver::SolveBranch`, more than twenty levels) checks each new solution with `SmLSIGlobalSolver::IdenticalSolutions` (`SmSurface.cpp:18843`) or `LocalCurveIntersect`, which re-derive the surface's singularities (`SmSurface::GetSingularities`, `:12384`) every time. Reached from `TestClosureSplitRegion` (`SmBrep.cpp:27654`, `:27845`), both through `CreateFaceFromSurface` -> `MakeFaceWithCurves` for an analytic face and through `SmStitch::DoStitching` -> `GlueEdgesGeneral` for a stitched sweep.
- **What happens:** Placing a small closed shell in its region takes minutes.
- **Trigger:** `SmPrimitiveCreation` primitives on a fresh B-rep (zone tolerance `1e-5`): `CreateSphere(r)` and the partial `CreateSphere(r, 0, 359.999995)` (p3's `CreateRotationalSweep` builds analytic faces from `359.99999` degrees up) for `r` from `8e-6` to `5e-5` (radius `1e-5` takes 87 s and succeeds; `1e-4` takes 7.7 s); `CreateSphere(r, 0, 359)` for `r` from `4e-5` to `1e-4` (20 s at `1e-4`); `CreateTorus(s, s, 0, 360)` for `s` from `1e-5` to `2e-5`, and `CreateTorus(s, s, 0, 359)` from `3e-6` to `5e-5` (16.5 s at `5e-5`); `CreateCone(s, s, s, 0, 359)` at `2e-5` and `(0, 359.999995)` at `1e-5`; `CreateSkinPrimitive` of two circles of radius `1e-5` (25-30 s with caps, 10 s without; the stitch's closure test, `TestClosureSplitRegion` -> `SmTopologyTraverser::IsInnerShell` -> `SmFace::GlobalLineIntersect`). Every primitive probe from `1.5e-4` up, spheres of `5e-6` or less, and skins from `1e-4` up return in milliseconds. (The static `CreateSweepAlongPlanarPath` of a `1e-5` circle along `1e-4` lines also runs past a minute, in `ProjectAndTrim` and the boolean's face intersections rather than the ray fire; at `1e-3` it takes 0.1 s.) Generated case `gen0072` of `primitive.primitive_creation_p2` (sphere `1e-5`, turn `360.0000000036`).
- **How found:** generated difftest case (`primitive.primitive_creation_p2`), then isolated native probes with 20-40 s limits.
- **Confirmed:** yes (C++ 87 s for the radius `1e-5` sphere; stack samples from a scratch `SIGPROF` handler all in the ray fire's line intersection, for the analytic and the stitched path).
- **Severity:** hang (seconds to minutes per call).
- **Rust port:** These cases defer at the face construction (`topology.brep_p8`, not ported yet), before the ray fire; `FindInfiniteRegion` (`topology.brep_p9`) and the stitch (`heal.stitch_p2`) are partial ports that these cases do not reach yet. p1's FFI requires radii above `1e-4`; the `primitive.primitive_creation_p2` FFI rejects spheres, tori and cones from half the zone tolerance to `1.5e-4` across at any turn, keeping inputs that fail before any face is built, and its generator keeps skins and planar paths at scale `1e-3` or more.

#### HANG-33. `SmStitch::StitchIntoShell` never returns when `dMaxStitchTol3d / 100` underflows
- **Location:** `source/SMLib/src/SmStitch.cpp:2756`-`2791` (`StitchIntoShell`).
- **What happens:** The loop starts at `dStitchTol3d = dMaxStitchTol3d / 100` and doubles it until it reaches `dMaxStitchTol3d` or no lamina edge is left. When the division underflows to `0`, doubling keeps it `0`, so `0 < dMaxStitchTol3d` holds forever and `DoStitching` runs again and again.
- **Trigger:** `0 < dMaxStitchTol3d` below about `5e-322` (e.g. `1e-323`) on a B-rep that keeps a lamina edge.
- **How found:** read during porting (`heal.stitch_p2`).
- **Confirmed:** yes (C++ difftest run with the shim guard disabled: `1e-323` on two sheets 0.05 apart was still running at 120 s; `1e-300` returns at once).
- **Severity:** hang.
- **Rust port:** Stops after `MAX_TOL_DOUBLINGS` (2200) doublings with `SolidError::InvalidInput`; the shim refuses the input (`-3`) and the runner applies the same test.

#### HANG-24. `SmStitch::StitchIntoSolid` never returns from a zero B-rep tolerance
- **Location:** `source/SMLib/src/SmStitch.cpp:2676`-`2711` (`StitchIntoSolid`).
- **What happens:** The loop starts at `GetTolerance() / 10` and doubles it while it is below `1/200` of the bounding box diagonal and lamina edges remain. A tolerance of `0` stays `0` (a negative one only grows more negative), so `DoStitching` runs forever. A positive tolerance always ends: against an infinite diagonal it doubles to infinity in about 2100 steps and `inf < inf` is false; a NaN tolerance or diagonal fails the loop test at once.
- **Trigger:** A B-rep tolerance `<= 0` with a positive box diagonal and a lamina edge that is never stitched.
- **How found:** read during porting (`heal.stitch_p2`).
- **Confirmed:** no (source inspection; the shim refuses a tolerance `<= 0` below the diagonal test, `-3`). The difftest block cannot set the B-rep tolerance, so no fixture reaches the guard.
- **Severity:** hang.
- **Rust port:** Stops after `MAX_TOL_DOUBLINGS` (2200) doublings with `SolidError::InvalidInput`.

#### HANG-40. `SmGrid::Step3DDDA` steps along an axis the ray does not move on when every next-plane parameter reaches `SM_BIG_DOUBLE`
- **Location:** `source/SMLib/src/SmGrid.cpp:881` `SmGrid::Step3DDDA` (axis choice at `:997`-`:1042`); `Setup3DDDA` (`:686`-`:745`) sets `m_lStep*` / `m_lOut*` only for axes with `|ray| >= SM_EFF_ZERO`.
- **What happens:** An axis the ray does not move along gets `m_dNextT = SM_BIG_DOUBLE` (`1e20`). When the moving axis's next plane is also at or past `1e20` (huge voxels or a tiny direction component), neither `<` test succeeds and the loop steps in Z (or Y) using an `m_lStep*` that was never set for this ray: uninitialised, or left over from an earlier ray. If it is 0, the address never changes and the `for(;;)` loop never ends. If it is ±1, the walk leaves the grid on the wrong axis and misses elements.
- **Trigger:** A grid over `[-1e-4, 1e300] x [-1e-4, 1.0001]^2` with 10x10x10 voxels, ray from `(1.000003, 0.5, 0.5)` along `(1, 0, 0)`, `Step3DDDA(1e20)` (`geom.grid` `fuzz09254`, `/grid/bbox/3 = 1e+300`; also `fuzz08036`, `fuzz-out/geom/geom.grid/`).
- **How found:** generated difftest case (10000-case fuzz; the Rust port, which zeroes the step, timed out).
- **Confirmed:** no. The existing `geom.grid` FFI coordinate bound (`MAX_COORD` 1e12) kept the box away from the C++ side.
- **Severity:** hang (with an uninitialised read). Not file-reachable.
- **Rust port:** `Grid::step_3ddda` returns `InvalidInput` after `1 + nx + ny + nz` steps in one call (`crates/solidcore/src/geom/grid.rs:872`). A walk that advances leaves the grid sooner than that.
- **Fuzz sweep trigger (2026-10-20):** `SmHCR::IsPointVisible` -> `SmRayTracer::FireRay` -> `SmGrid::Step3DDDA` in `blend.hcr` scenes with a curve or surface coordinate of `+-1e300` (`fuzz-out/n2000/blend.hcr/fuzz00041.json`, `fuzz00401.json`, `fuzz00490.json`, `fuzz00648.json`, `fuzz00901.json`, `fuzz01293.json`, `fuzz01948.json`; the sweep worker measured a scene of size `2e4` returning with one coordinate at `1e18` and not at `1e20`). The C++ timed out (20 s plus retry), so Confirmed becomes yes. The faithful Rust walk also timed out on `fuzz00700.json` and `fuzz01870.json` before the `grid.rs:872` step bound. Guard: the `blend.hcr` FFI rejects scene and op-7 point values above `MAX_SCENE_MAGNITUDE = 1e15` (`crates/solidcore-ffi/src/units/blend_hcr.rs:262`, `:178`), broader than the measured onset (follow-up).

#### HANG-41. `SmLine::Tessellate` runs `lMinimumNumberOfSegments` iterations and ignores failed `Add`s
- **Location:** `source/SMLib/src/SmLine.cpp:1955` `SmLine::Tessellate` (loops at `:1973`, `:2001`, `:2038`); `SmTArray<TYPE>::Add` (`source/SMLib/inc/SmTArray.h:1049`).
- **What happens:** Each requested output array gets `lMinimumNumberOfSegments + 1` entries through `Add` with no up-front size check. For a huge count the arrays grow until `SetAtGrow` fails; `Add` then returns `SM_BIG_ULONG`, which is ignored, and the loop runs on for the remaining count (up to ~1.8e19 iterations), finally returning `SM_SUCCESS` with short arrays.
- **Trigger:** `Tessellate` on a bounded line with `lMinimumNumberOfSegments = 2^63 - 1` and parameters or points requested (`curve.curve_p1` fuzz `fuzz01447`: `/minseg = 9223372036854775807`).
- **How found:** generated difftest case (fuzz sweep; the Rust port aborted on the allocation).
- **Confirmed:** no. The FFI caps the count (`MAX_SEGMENTS = 64`, `crates/solidcore-ffi/src/units/curve_curve_p1.rs`).
- **Severity:** hang (also memory exhaustion and a short result reported as success).
- **Rust port:** `Line::tessellate` reserves the `min_segments + 1` entries per requested array first and returns `InvalidInput` when that fails.

#### HANG-42. `N_SrfRemoveAllKnots` / `N_SrfRemoveKnotsConstraints` loop forever once a removal error bound is NaN
- **Location:** `source/SMLib/src/NL_SrfTool.cpp:3114` `N_SrfRemoveAllKnots` and `:11424` `N_SrfRemoveKnotsConstraints` (`dir = NL_UVDIR` direction choice).
- **What happens:** With a control point near `1e300` the u error bounds are NaN, so `bu < bv` is false and every `NL_UVDIR` step goes to v. Once v has nothing removable, each step removes nothing and leaves the selection state unchanged, and the loop never ends.
- **Trigger:** `N_SrfRemoveAllKnots(sur, tol = 0, NL_UVDIR)` on a generated surface with one net coordinate `1e300` (`nlib.srf_tool_p3` `fuzz00246`, `fuzz00619`, `fuzz00633`, `fuzz01530`, `fuzz01724`); `N_SrfRemoveKnotsConstraints(.., tol = 1, NL_UVDIR)` with a net coordinate `-1e300` (`nlib.srf_tool_p4` `fuzz00396`, `fuzz00456`, `fuzz01087`, `fuzz01339`, `fuzz01923`).
- **How found:** generated difftest case (`difftest --fuzz 2000`: C++ gave no result within 20 s).
- **Confirmed:** yes (timeout, 10 cases).
- **Severity:** hang.
- **Rust port:** `nurbs::srf_tool::p3::remove_all_knots` detects a step that changes nothing and returns `NotConverged`. The `nlib.srf_tool_p3`/`p4` FFIs only remove knots of nets with homogeneous coordinates within `MAX_REMOVAL_COORD = 1e60` and weights within `[1e-30, 1e30]` (conservative, not an exact test).

#### HANG-43. `SmApiCreateCurveSweepFromFaces` does not return for a path with a `1e300` control point
- **Location:** `source/SM_API/src/SmApiBrep.cpp:1530` `SmApiCreateCurveSweepFromFaces` -> `SmPrimitiveCreation::CreateCurveSweep`. The spinning loop was not located; the sweep-surface polyline approximation of HANG-5 is a candidate.
- **What happens:** No result within 20 s.
- **Trigger:** a circular face (radius `29.2` or `21.4`) swept along `SmApiCreateCurve` through four points, one with x `1e300` (options `[F,F,T,F,T]` without scale curve; `[T,F,T,T,F]` with a line scale curve).
- **How found:** generated difftest case (`difftest --fuzz 2000`, `sweep.api_brep` `fuzz01218`, `fuzz01458`; still timing out in `verify2000/sweep.api_brep/`).
- **Confirmed:** yes (timeout).
- **Severity:** hang.
- **Rust port:** `create_curve_sweep` is a stub until `primitive.primitive_creation_p3` lands; not guarded yet (open).

#### HANG-44. `N_CrvOffsetPtSampling` time grows quadratically with an unbounded sample count
- **Location:** `source/SMLib/src/NL_CrvOffset.cpp:1380`-`1381` (sample count per segment, no upper bound), `:1525` `N_CrvRemoveKnotsDerivConstraints( curQ, tol, NL_BOTH, der, curQ, SG )` on the interpolant through all `mu` samples.
- **What happens:** The count is `ceil((1/tol)^0.34 * max(2 deg, sqrt(Muu / 8)))` per segment, with no cap. The interpolant has `mu` control points, and the knot removal that follows takes time quadratic in `mu`. The Rust port, which runs the same algorithm, needs about 5 s for 23 000 samples. A pole makes `Muu` huge (727 541 samples for the case below), and that run did not finish.
- **Trigger:** `nlib.crv_offset` fuzz00366: a cubic, 8 control points, weight of control point 5 set to `-1.0` (`/crv/cpts/5/3 = -1.0`), `B = (0,0,1)`, `d = 0`, `deg = 3`, `NL_INHERITED`, `tol = 0.11799393559359235` (Rust: no result after 20 s). The same happens for valid positive-weight curves with a tiny `tol`.
- **How found:** fuzz sweep (Rust timeout on the faithful port).
- **Confirmed:** no. The shim refuses more than `MAX_SAMPLES` (4000) samples and the FFI refuses non-positive weights, so the C++ never ran this input.
- **Severity:** hang.
- **Rust port:** `InvalidInput` ("too many sample points") above `MAX_OFFSET_SAMPLES` = 16 384 (`crates/solidcore/src/nurbs/crv_offset.rs:52`, check at `:214`), a documented deviation. The bound needs a maintainer decision.

#### HANG-45. `SmBrep::CreateFaceFromSurface` of a rational B-spline sphere of radius about `1e-5` does not return
- **Location:** `SmBrep::CreateFaceFromSurface` -> `SmSurface::LocalCurveIntersect` -> `SmLocalSolveNd::SolveIt` inside `SmGlobalSolver::SolveBranch` (location as recorded by the sweep worker in the FFI guard comment; not re-checked with gdb).
- **What happens:** Building the face of a full rational biquadratic sphere (4 x 8 spans, weights `sqrt(0.5)`) of radius `1e-5` (and in the `1e-5`..`1e-4` range) never returns; `1e-6` and `1e-3` return. Shape-dependent, like UB-201 (small-radius `SmApiCreateSphereNoPole` hangs); may share its cause.
- **Trigger:** `query.api_queries` shim kind 9, radius `1e-5`, domain `[0,1]^2`, any B-rep op (fuzz mutated `op 3 -> 0`).
- **How found:** generated difftest case (`difftest --unit query.api_queries --fuzz 2000`: `fuzz-out/n2000/query.api_queries/fuzz00500.json`, base `rational_kind9_1e-05_north`, mutation `/op = 0`).
- **Confirmed:** yes (no result after 20 s and the retry).
- **Severity:** hang.
- **Rust port:** not reached; the FFI wrapper requires radius `>= 1e-3` for kind-9 B-rep ops (`crates/solidcore-ffi/src/units/query_api_queries.rs:83`).

#### HANG-46. `SmCurve::ApproximateCurve` never finishes for a break interval far from the origin
- **Location:** `SmCurve.cpp:1504` `SmCurve::ApproximateCurve` (the bisection loop around `sm_AddToCurve`; the only stop is a span shorter than `SM_EFF_ZERO`).
- **What happens:** Where coordinates reach about `1e10`, a Hermite span passes `sm_TestSpanAccuracy` only when it is tiny (about `1e-4` at `t = 5e10` with tolerance `1e-5`), so a break interval reaching `1e300` appends spans without end and the control polygon grows until memory runs out. When a failing span can no longer be split (its midpoint rounds onto an end and evaluates to that end's data), the loop repeats one state forever.
- **Trigger:** `ApproximateCurve` of a line with breaks `[-0.81, 5.69, 8.33, 1e300, 8.53]` and tolerance `1e-5` (`fuzz-out/n2000/curve.curve_p1/fuzz01255.json`); the port, which follows the same loop, aborted on a 6 GiB control-polygon allocation after 16 s.
- **How found:** fuzz-corpus replay against main (2026-10-22), located with gdb in the port.
- **Confirmed:** no (the `curve.curve_p1` FFI refuses values above its coordinate bound, so the reference build never runs it; the port's loop is a line-by-line port).
- **Severity:** hang (unbounded memory).
- **Rust port:** Returns `InvalidInput` past `MAX_APPROX_SPANS = 2^20` appended spans and when a failing span cannot be split (`curve/curve/p1.rs`).

#### HANG-47. `SmFindPCExtremaEFO::Evaluate` loops forever on an infinite tangent
- **Location:** `SmCurve.cpp:18304` (`SmFindPCExtremaEFO::Evaluate`, def. `:18258`): `while (sPV[1].LengthSquared() > 1.0e4)` divides the derivatives by ten.
- **What happens:** an infinite tangent stays infinite, so the loop never ends.
- **Trigger:** `GlobalCurveSolve` of a rational B-spline with `-1.8e308` weights (`fuzz-out/n2000/curve.curve_p9/fuzz00144.json`, `fuzz01222.json`).
- **How found:** fuzz-corpus replay of the Rust side alone (issue #504), located with gdb in the port.
- **Confirmed:** no (the `curve.curve_p9` FFI refuses mixed rationality, so the reference build never runs it; the port's loop is a line-by-line port).
- **Severity:** hang.
- **Rust port:** `InvalidInput` after `MAX_PC_SCALE_STEPS = 400` divisions (a finite tangent needs at most 310; `curve/curve/p3.rs`).

#### HANG-48. `SmCurve::GlobalCoincidenceChecker` makes `3^depth` calls on overflowing curves
- **Location:** `SmCurve.cpp:19950` (`SmCurve::GlobalCoincidenceChecker`): a closed or half closed interval is split in three and each piece checked recursively, with four end-point drops (global solves) per call.
- **What happens:** with `1e300` control points the split test keeps answering yes 15 to 30 levels deep, so the call count grows as `3^depth` (hundreds of thousands after 10 s in the port).
- **Trigger:** `fuzz-out/n2000/curve.curve_p8/fuzz01456.json`, `fuzz01856.json`, `surface.surface_p8/fuzz01603.json`, `curve.composite_curve_p2/fuzz00430.json`, `fuzz01007.json`.
- **How found:** fuzz-corpus replay of the Rust side alone (issue #504).
- **Confirmed:** no (the FFIs refuse the `1e300` values).
- **Severity:** hang (exponential time).
- **Rust port:** `InvalidInput` past `MAX_COINCIDENCE_CALLS = 2^18` calls per check (`curve/curve/p6.rs`); the global solves under it stop past `MAX_SOLVE_BRANCHES = 2^20` visits (`geom/global_solver.rs`).

#### HANG-49. `N_ApproxNurbsWithNonRatCrv` samples without bound and removes knots in quadratic time
- **Location:** `NL_CrvApprox.cpp:2780` (`N_ApproxNurbsWithNonRatCrv`, def. `:2698`): the sample count `mu` is the sum of the per-segment counts from the second-derivative bound.
- **What happens:** a weight of `-1` (a pole) asks for about ten million samples; all are interpolated and `N_CrvRemoveKnotsDerivConstraints` then shifts its arrays once per removed knot, quadratic in the count.
- **Trigger:** `fuzz-out/nlib-fit10k/nlib.crv_approx/fuzz05147.json`.
- **How found:** fuzz-corpus replay of the Rust side alone (issue #504).
- **Confirmed:** no (the FFI refuses non-positive weights).
- **Severity:** hang (quadratic time in an input-derived count).
- **Rust port:** `InvalidInput` past `MAX_APPROX_SAMPLES = 2^17` samples (`nurbs/crv_approx.rs`).

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
- **Confirmed:** most paths remain source-inspection findings; the original relation shims sort unordered membership before comparing. PR #175 observed the order leaking into `SmOffsetExecutive::SyncronizeOffsetEdgeuses`: two equal-minimum fixtures returned public `matched_first` IDs `[2,0]` in C++ versus `[0,2]` in deterministic Rust. The strict bubble sort (`SmOffsetExecutive.cpp:2485-2506`) preserves the unspecified `GetSeconds` tie order. Allocation-sensitive variation within the same executable was not measured.
- **Severity:** nondeterminism / unspecified order exposed as result order.
- **Rust port:** Uses ordered maps / insertion order on arena ids. Following the existing `SmRelation` comparison convention, the `offset.offset_executive_p1` runner canonicalizes only contiguous groups with exactly equal interval minima by traversal id on both sides. Distinct-minimum order, membership, multiplicity, null padding and all numerical values are still compared unchanged. No tolerance is relaxed, and this unspecified tie order is not a porting blocker. `SmTopologyIntersector`'s mate maps (`GetMappedBrepObjects`, `SmTopologyIntersector.cpp:945`; `GetMappedOtherObjects`, `CoincidenceNotify`) and subset (`GetSubset`) are `BTreeMap`/`BTreeSet`s over the caller's object handles in `boolean::topology_intersector::p1`, so their lists come out in handle order; the `boolean.topology_intersector_p1` shim sorts upstream's lists by export handle before comparing.

#### ND-3. A function-local static fixed by the first call
- **Location:** `SmFilletSolver.cpp:3604`, `SmFilletSolver::ComputePointValues` (def. `:3544`).
- **What happens:** `static double sdStaticStep = (rTsectPnt.m_ePointType == SM_IP_TANGENT_POINT) ? 1.0e-3 : 1.0e-5;` is initialised on the first call only. Every later call in the process uses the step chosen for the first point's type, so results depend on call history. (`SmFilletExecutive.cpp:1503`-`1504` also use function statics, but with constant initialisers.)
- **Trigger:** Two fillet runs in one process whose first intersection points differ in type, run in either order.
- **How found:** read during porting (static scan, `plan/closure.md`).
- **Confirmed:** no. The fillet units are not ported yet.
- **Severity:** nondeterminism (order-dependent results).
- **Rust port:** `Kernel::fillet_point_step` latch (per kernel). The `blend.fillet_solver_p4` difftest probes the C++ static (`sc_blend_fillet_solver_p4_static_step`) before every case that can reach the finite-difference block and seeds the latch with it; see `plan/findings.md` (blend.fillet_solver_p4).

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

#### WR-159. `SmVector3d::Unitize` turns a vector with an overflowing squared length into a zero vector
- **Location:** `source/SMLib/inc/SmVector3d.h:537-566`, `SmVector3d::Unitize`; reached from `SmAxis2Placement::SetCanonical` (`SmAxis2Placement.cpp:70-97`).
- **What happens:** The degenerate test is only `dLenSq < SM_EFF_ZERO_SQ`. A component of about 1.34e154 or more makes `LengthSquared()` infinite, so the vector passes the test and is divided by an infinite length. Finite components become 0 and the result is a silent zero vector (with a negative-zero component), not the documented `(0,0,1)` default, and the status is success.
- **Trigger:** An axis placement whose Y axis is `(0, -1e300, 1)`, unitized by `SetCanonical`, used in a cone/cone intersection (`SmCone::IntersectWithCone`). A component of 1e150 still unitizes correctly; 1e160 does not.
- **How found:** differential fuzzing of `surface.cone_p3` (`fuzz01093`).
- **Confirmed:** yes (C++ returns two lines with a zero Y axis: the polar conversion of that cone then fails, `SmEllipse.cpp:596`, and `IntersectWithCone` ignores that status, `SmCone.cpp:2966-2967`).
- **Severity:** wrong result.
- **Rust port:** `Vec3::unitize` is faithful and has the same overflow; such inputs agree with C++ (`surface.cone_p3` fixtures `fuzz_axis_overflow_zero_y*`).

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

#### WR-253. `SmLocalSolve1d::SolveOscillating` reads a failed guess's value as a sign change
- **Location:** `source/SMLib/src/SmLocalSolve1d.cpp:470-495` `SmLocalSolve1d::SolveOscillating`: a failed `Evaluate` of an interior guess `continue`s (`:478`), and the sign test after the loop (`:495`) reads `sTempIter.m_dFOfT` from the last guess whether or not its evaluation succeeded.
- **What happens:** when the last guess (0.99 of the way) fails, its `F` is whatever the evaluator left: an evaluator that zeroes its outputs before failing, as `SmCurvePropertyEFO::Evaluate` does (`SmCurve.cpp:14918`), leaves 0. `0 * F(curr) <= 0` passes as a sign change, the guess is bracketed with `F = 0`, and bisection returns it as converged (`SM_TR_FOUND_ANSWER_CONVERGED`) although `F` there is not zero.
- **Trigger:** an oscillating `SolveIt` whose evaluator fails at the last interior guess. In SolidCraft's rig, the radius-of-curvature test of `SmBSplineCurve::AssertValid` (once #475 is fixed) on a straight UV trim: box minus a y-axis cylinder tangent to the box bottom (`smlib_test_booleans` tangent_cylinders, seed 74 op 1) gives two "minima" of radius 0 (tangency-labs/solidcore#481).
- **How found:** issue #481; traced at `c9c5979`.
- **Confirmed:** no (the port reproduces it; no C++ run here).
- **Severity:** wrong result (a solution where there is none).
- **Rust port:** Faithful to the code: `LocalSolve1d::solve_oscillating` keeps the failed evaluation's value and brackets it the same way (test `failed_guess_is_read_as_a_sign_change_as_upstream`: `F = t^3 - 2t + 2` reports converged at the failed guess, where `F` is about 0.95, and nothing without the failure). Not changed, so the unit keeps the reference build's results.

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

#### WR-158. `SmEdgeuse::CreateUVTrimCurve`'s two-singular-end-points recovery joins its curve into a temporary array
- **Location:** `source/SMLib/src/SmEdgeuse.cpp:1859`-`1874` (`SmEdgeuse::CreateUVTrimCurve`, the `SM_CTC_TWOSINGULARENDPTS_RECOVERY` branch).
- **What happens:** After dropping both halves of the edge curve, the first half's UV curve and the second half's are stitched with `SmBSplineCurve::CreateByJoining`, and the joined curve is added to `s2ndUVCurves` (line 1871), the temporary array of the second half, which `SmObjsDelete` (`sClean5`) destroys when the block ends. The result array `sUVCurves` stays empty while `lResult` is set to `SM_SUCCESS` and `m_eCreatePathType` to `SM_CTC_TWOSINGULARENDPTS_RECOVERY`, so the later `sUVCurves.GetSize() == 0` test skips the iteration: the recovery never produces a trim curve (and the fail record claims it did).
- **Trigger:** an edge whose start and end both drop onto singularities of the face's surface (for example a meridian of a sphere from pole to pole) after the plain drop and the reversed drop failed.
- **How found:** read during porting (unit `topology.edgeuse_p3`).
- **Confirmed:** no (the Rust side defers the drop: `SmSurface::DropCurve` is `intersect.surface_drop_lib_p3`).
- **Severity:** wrong result (the intended recovery is dead code).
- **Cause:** `s2ndUVCurves.Add(pNewUVTrimCurve)` where `sUVCurves.Add(...)` was meant.
- **Rust port:** Reproduces it (`topology::edgeuse::p3::create_uv_trim_curve` builds the joined curve into the discarded second-half array and reports the recovery path).

#### WR-190. `SmFilletSolver::RecordFilletError` reports rail 0 for rail 1
- **Location:** `source/SMLib/src/SmFilletSolver.cpp:7930` (`SmFilletSolver::RecordFilletError`).
- **What happens:** `if ( pThisGeom->GetRail(1)->GetCurve() != NULL ) { apFilEdges.AddUnique( pThisGeom->GetRail(0) ); }`: the second rail is tested but the first is added, so the fillet edges handed to `SmFilletExecutive::NoteFilletError` never contain a geometry's second rail.
- **Trigger:** any fillet error recorded after a geometry got its second rail curve.
- **How found:** read during porting (unit `blend.fillet_solver_p2`).
- **Confirmed:** no (the fillet geometry queries are unported).
- **Severity:** wrong result (incomplete error report).
- **Cause:** copy-paste slip (`GetRail(0)` for `GetRail(1)`).
- **Rust port:** Reproduced (`FilletSolver::record_fillet_error` adds rail 0 where it tests rail 1); the list only feeds `SmFilletExecutive::NoteFilletError`, an `UNPORTED(blend.fillet_executive_p1)` stop, so it is not observable yet.

#### WR-176. `SmFace::HasSeamProblem` returns TRUE whenever it has to compute the face properties
- **Location:** `source/SMLib/src/SmFace.cpp:15650`, `SmFace::HasSeamProblem` (def. `:15540`).
- **What happens:** When `pOptFaceProps` is NULL or below `SM_PROPSTAGE_2a`, the function runs `SetProps_Stage2`, copies its seam classifications and enums to the outputs, and returns `SM_SUCCESS` (`1000`) from a function returning `SmBoolean`. The result is always TRUE ("seam problem"), even for a face whose seams are fine and whose output enums say `SM_SP_NEITHER`.
- **Trigger:** `HasSeamProblem` without face properties on a face of a periodic surface (e.g. faces of `TestFiles/pt_TestFiles/Mouse.smb`).
- **How found:** generated difftest case (`topology.face_p4`: a port returning FALSE there disagreed with the C++).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Returns `has_problem = true` there, as upstream.

#### WR-177. `SmFace::FindVertexSector` evaluates an empty domain for poles after its first vertex use
- **Location:** `source/SMLib/src/SmFace.cpp:9762`, `SmFace::FindVertexSector` (def. `:9408`; the pole points `:9788`-`:9803`).
- **What happens:** In the 3D branch (an edge use without UV trim curve), `SmExtent2d sSurfDomain` is declared inside the loop over vertex uses, but the natural domain is stored in it only on the pass that first fetches the poles (`bCheckedPoles`). On later passes a vertex at a pole gets `sEU1UV` from a default (inside-out) extent, which is then passed to `SectorClassifyCurveOn` as the singular UV point.
- **Trigger:** A vertex at a surface pole with two or more vertex uses on the face's upward face use, their edge uses without UV trim curves, and sector validation requested.
- **How found:** read during porting (`topology.face_p4`).
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Reproduces it (the domain is only set on the pass that fetches the poles).

#### WR-167. `SmTArray::FindElementInMaskedArray` tests `1<ii` instead of `1<<ii`
- **Location:** `source/SMLib/inc/SmTArray.h:1543`, `SmTArray<TYPE>::FindElementInMaskedArray` (def. `:1531`). Called by `sms_FindAndClassifyLoopBisector` (`SmFace.cpp:2588`) on `SmFaceProps::m_sPolePoints` with the `m_lPoles` side mask.
- **What happens:** The mask test is `lBitMask & (1<ii)`, a comparison, not a shift: elements 0 and 1 are never searched, and elements 2 and up are searched exactly when bit 0 of the mask (`SM_SS_UMIN`) is set, whatever their own bits. The bisector therefore never recognises an edge mid point on the u-min or v-min pole, and tests the u-max and v-max poles only when the u-min side is singular.
- **Trigger:** A face whose surface is singular at its v-min side (e.g. a sphere pole) with an edge mid point on that pole: the edge is not skipped.
- **How found:** read during porting.
- **Confirmed:** no (the bisector cases of the `query.face_p3` difftest defer before comparing a pole skip).
- **Severity:** wrong result.
- **Rust port:** Ported as is (`query::face::p3`, private `find_element_in_masked_array`).
#### WR-191. `SmMerge::ManifoldBoolean` leaves both contexts in boolean mode when it fails
- **Location:** `source/SMLib/src/SmMerge.cpp:2551`-`2552` (`SmMerge::ManifoldBoolean` sets `SetDoingBoolean(TRUE)` on the B-rep's and the other B-rep's contexts); they are cleared only at the three success returns (`:3114`-`3115`, `:4148`-`4149`, `:5095`-`5096`). Every error return in between (`SER` at `:2616`, `:2628`, `:2661`, `:3023` and later, `SER(SM_ERR)` at `:3394`, `:3621`) leaves them set.
- **What happens:** After a failed boolean, every later operation on those contexts runs in boolean mode (which delays surface cache construction, among other effects), until something resets the flag.
- **Trigger:** the static `SmPrimitiveCreation::CreateSweepAlongPlanarPath` (tolerance `1e-7`, both caps, profile centred on the path) of a circle of radius `1e-3` at the origin along the lines `(0,0,0)-(0,0,0.01)` and `(0,0,0.01)-(0.01,0,0.01)`: it returns `SM_ERR` with the context's doing-boolean flag set, and the next primitive on that context reports it still set (generated case `gen0092` of `primitive.primitive_creation_p2`, which made the three cases after it differ).
- **How found:** generated difftest case (`primitive.primitive_creation_p2`), then the same inputs in sequence in one process.
- **Confirmed:** yes (the flag is set after the failing sweep and after the following, unrelated primitive). That the flag comes from `ManifoldBoolean` is from source reading: it is the only unscoped set on that path without a restore (the `SmBrep` and `SmEdge` destructors save and restore it, the primitive entry points use `SmTemporaryChangeValue`).
- **Severity:** wrong result (state leaks into later calls).
- **Rust port:** The `ManifoldBoolean` port (`boolean.merge_p4`) keeps the same exits. The `primitive.primitive_creation_p2` shim clears the flag before each case.

#### WR-197. `SmMerge::MergeAddedTopology`'s declared defaults shift into the wrong flags
- **Location:** `source/SMLib/inc/SmMerge.h:357` (declaration) against `source/SMLib/src/SmMerge.cpp:5304` (definition), `SmMerge::MergeAddedTopology`.
- **What happens:** The declaration puts `bDoGluing = TRUE` ninth, followed by `bMakingManifoldSolid = FALSE`, `bFastEdgeCompare = TRUE`, `bDoRegionNesting = TRUE`, `bIgnoreProblems = TRUE`. The definition takes the four flags ninth to twelfth and the unused gluing flag last. A call relying on the defaults therefore runs with making-manifold-solid `TRUE` and fast edge compare `FALSE`, the opposite of what the declaration documents.
- **Trigger:** Any call that omits the trailing arguments. Both upstream callers (`MergeAddedFace`, `SmFeatureExecutive`) pass all four flags explicitly, so they are not affected.
- **How found:** read during porting (`boolean.merge_p2`).
- **Confirmed:** no (no upstream caller uses the defaults).
- **Severity:** wrong result (latent).
- **Rust port:** `MergeAddedOptions::default()` is what the declared defaults actually pass, `(TRUE, FALSE, TRUE, TRUE)`; `MERGE_ADDED_FACE_OPTIONS` is `MergeAddedFace`'s explicit `(FALSE, TRUE, TRUE, TRUE)`.

#### WR-256. `SmVertex::AreEdgesTangent` / `IsTopologicalVertex` answer `TRUE` when an end tangent cannot be evaluated
- **Location:** `source/SMLib/src/SmVertex.cpp:70` `AreEdgesTangent` (its four `SE(GetEndTangent(...))`), `:716` `IsTopologicalVertex` (the `SE(GetEndTangent(FALSE, ...))` branches); `SmEdge::GetEndTangent` (`SmEdge.cpp:476`) writes the tangent only after its `SER` on the curve evaluation.
- **What happens:** The ignored failure leaves the caller's `SmVector3d` as it was: its default `(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE)`, or in `IsTopologicalVertex` the line vector `IsLine` wrote when that edge's curve is a line. Its squared length (about `4.6e-122`) is within `SM_IS_ZERO_SQUARED`, so `AngleBetween` fails, sets the angle to `0` first, and the status is ignored. `AreEdgesTangent` then returns `TRUE` for any positive tolerance, and `IsTopologicalVertex` passes its angle test, so the vertex counts as removable although nothing is known about the edges' directions there. (At an edge's start `IsTopologicalVertex` returns the failed status itself as `TRUE`, WR-92.) This corrects UB-106, which took the tangent to be uninitialised.
- **Trigger:** an edge whose curve cannot be evaluated with one derivative at its interval end. B-spline curves and `SmLine`s cannot get there: `SmBSplineCurve::Evaluate` clamps the parameter into the natural interval, and `N_BasisFindSpan` (`NL_BasisBasic.cpp:1429`) steps past end spans shorter than `1e-12` before `sm_BasisDerivs` could reject a knot difference of `SM_EFF_ZERO_SQ`; a NaN parameter evaluates to NaN without an error. An edge whose curve is an `SmCompositeCurve` always reaches it: that class keeps `SmCurve::Evaluate`, which returns `SM_ERR` (`SmCurve.h:390-404`). So can other curve kinds whose `Evaluate` fails (a curve on a surface whose evaluation fails, for example).
- **How found:** issue #521 (values read after a failed call); traced in the source and probed with the `topology.vertex` difftest (`end_tangents.json`: a 1e-30 end span and zero end tangents, compared, none of which makes `GetEndTangent` fail); the composite-curve edge is the port's unit test `failed_end_tangent_reads_the_default_vector`.
- **Confirmed:** no (the `topology.vertex` harness builds line and B-spline edges only, and none of its inputs makes `GetEndTangent` fail).
- **Severity:** wrong result.
- **Rust port:** Faithful: `topology::vertex` reads `Vec3::UNDEF` for a tangent nothing wrote (it returned `SolidError::InvalidInput` before #521).

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

#### WR-166. `N_SrfIsClosedSmooth` reports an error after storing its result
- **Location:** `source/SMLib/src/NL_SrfNurb.cpp:5178` (and the v-closure loop), `N_SrfIsClosedSmooth` (def. `:5058`).
- **What happens:** A failed `N_VectorsAngle` (a zero-length tangent, as at a pole side) only breaks out of the span loop: `error` stays `NL_YES`, the closed directions are still stored in `*gnflg`, and the function returns the error. `N_SrfEvalPtDerivsUnbounded` then fails its first call for such a surface with `esur` holding the copy, and callers that retry (`sm_ExtendSurface`, `EvaluatePointOnExtendedSurface`) extend that copy, `sm_ExtendSurface` at the unpushed parameter.
- **Trigger:** An offset extension over a B-spline with a pole side.
- **How found:** generated difftest case (`surface.offset_surface`: the Rust port, which dropped the flag on that error, diverged from the C++).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** `Surface::is_closed_smooth_flag` returns both; `eval_pt_derivs_unbounded` keeps the copy and the flag, as upstream.

#### WR-168. `MakeWireEdge` / `MakeWireEdgeVertex` give the new edge a sliced copy of an edge-owned curve
- **Location:** `source/SMLib/src/SmBrep.cpp:21676`-`21678` (`SmBrep::MakeWireEdge`) and `:21801`-`21803` (`SmBrep::MakeWireEdgeVertex`).
- **What happens:** When the curve passed in already belongs to an edge (`pCurve->GetEdge() != NULL`), the new wire edge gets `new (pCurve->GetContext()) SmCurve(*pCurve)`: the `SmCurve` base-class copy constructor, which copies none of the derived class's geometry (an `SmBSplineCurve`'s NURB, a line's point and vector). The new edge's curve is a plain `SmCurve` whose evaluation falls back to the base class (`SM_ERR`). The vertex tolerance checks right after still evaluate the original curve, so the calls succeed.
- **Trigger:** `MakeWireEdge(NULL, v1, v2, pOtherEdge->GetCurve(), ...)` (or `MakeWireEdgeVertex`) with the curve of an existing edge.
- **How found:** read during porting (`topology.brep_p3`, PR #204).
- **Confirmed:** no (not run: the port and its difftest stop at `topology.shell` inside `MakeEdgeTopology` first). Update (PR #204, after topology.shell was wired): the wire makers now run and are compared (49 passing cases), but every wire fixture passes a curve no edge owns, so this branch is still not run.
- **Severity:** wrong result (an edge without usable geometry).
- **Rust port:** Deviation: the port clones the curve with its geometry (`Curve`'s `Clone`, the copy constructor of each curve kind); a geometry-less base curve cannot be represented.

### Curves

#### WR-190. `SmFilletEdge::CalcBlendingRail` builds its Hermite fallback as a degree-2 curve and drops a control point
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:6537-6565` (`SmFilletEdge::CalcBlendingRail`, the `dT1*dT2 < 0.0` branch: `SmBSplineCurve::CreateCanonical(crContext, lCurveDim, 2, sCntrlPoly, ..., sKnotMult = {4, 4}, ...)`).
- **What happens:** The four Bezier points of the cubic Hermite (`GetBezierPoints`) are passed with knot multiplicities 4 at both ends but degree 2. `CreateCanonical` accepts it and makes a quadratic of three points: start, the third Bezier point and end. The second point (start + start tangent / 3) is dropped, so the rail leaves its start in the wrong direction and is not tangent to the first rail. The same branch in `sm_InterpolateEndptsAndTangents` passes degree 3.
- **Trigger:** A blending rail whose end tangents' closest-approach parameters have opposite signs (`smgu_LineLineClosestPoint`: `dT1 * dT2 < 0`).
- **How found:** read during porting (`blend.fillet_geom_p6`, PR #247).
- **Confirmed:** yes: the difftest reaches the branch (83 of 3000 generated cases) and C++ and Rust agree bit for bit on the three-point quadratic.
- **Severity:** wrong result (tangency lost).
- **Rust port:** faithful (degree 2); `blend::fillet_geom::p6` tests pin the three-point result.

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

#### WR-211. `SmEllipse::AdjustSTEPInterval` collapses an arc moved a turn past its interval to one NURBS parameter
- **Location:** `source/SMLib/src/SmEllipse.cpp:1905` `SmEllipse::AdjustSTEPInterval` (documented range `[-360 <= Min <= Max <= 360]` at `:1907`): both ends go through `SmPolarConversion::ConvertToNURBSParameter` (`:1921-1922`), which clamps an angle outside the current arc to its ends (`:512-517`). Reached from `SmSurfOfRevolution::AdjustSTEPUVDomain` (`SmSurfOfRevolution.cpp:331`, whose own comment gives `0_to_360`).
- **What happens:** For a circle over `[0, 360]` (or `[0, 180]`) given `[360, 540]`, both ends clamp to the arc's end, so `sNewNurbsIvl` (`:1923`) has zero length and `EditParameterization` (`:1933`) collapses the curve to one parameter, without an error. The next reparameterisation divides by that length: `N_BasisReparam` (`NL_BasisBasic.cpp:2215`) gets `fac = (ur - ul) / 0`, keeps the end knots and makes every interior knot `ul + inf * 0 = NaN` (`:2221`), the knots `360 360 360 NaN NaN 540 540 540` of #428.
- **Trigger:** a STEP surface of revolution whose sweep domain starts at 360 degrees (SolidCraft schema 4 allows a periodic domain up to one turn past 2π; tangency-labs/solidcore#428).
- **How found:** following #428 and #427 (the NaN knots then overflow `N_IgesClampSrf`, `UB-250`); traced at `c9c5979`.
- **Confirmed:** no (the port reproduces each step; no C++ run here).
- **Severity:** wrong result (a zero-length curve, then NaN knots).
- **Fix:** tangency-labs/smlib@74bed94 takes an angle outside the arc whole turns back into it for a full-turn arc, and otherwise extends the arc's NURBS interval in proportion.
- **Rust port:** Faithful to the code: `Ellipse::adjust_step_interval` collapses the same way and the next reparameterisation gives the same NaN knots (test `step_interval_past_a_turn_collapses_as_upstream`). The fork's fix is not ported: the port follows the reference build, as for `UB-250`.
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

#### WR-153. `SmCone::IsNurbSurfaceCone` returns an error status as its `SmBoolean`
- **Location:** `source/SMLib/src/SmCone.cpp:4406`, `:4415`, `:4494` (`SER` around `CreateIsoParametricCurve`), `SmCone::IsNurbSurfaceCone` (def. `:4305`).
- **What happens:** `SER` returns the failing `SmStatus` (e.g. `SM_ERR` = 1001) from a function returning `SmBoolean`, so callers such as `SmBSplineSurface::CopyAndAddAnalytics` read it as `TRUE` with a `NULL` cone and return success without a surface.
- **Trigger:** An iso-curve extraction failure on a candidate cone NURB (e.g. a knot multiplicity above the degree).
- **How found:** read during porting (`surface.cone_p2`, PR #153).
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Returns the error.

#### WR-160. `SmOffsetSurface` third derivatives use wrong coefficients in `Cuuv` and `Cuvv`
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:1468` and `:1479`, `SmOffsetSurface::EvaluatePlainOrSimple`.
- **What happens:** The derivatives of the normal's numerator `C = Bu x Bv` take `3.0 * Buuv x Buv` in `Cuuv` and `3.0 * Buvv x Buv` in `Cuvv`. Differentiating `Cuu` and `Cvv` gives coefficients `1` and `-1`, so `Duuv` and `Duvv` of the offset are wrong whenever those cross products are not zero.
- **Trigger:** `Evaluate(uv, 2, 1, ..)` on a twisted surface: `Duuv` differs from a central difference of `Duu` by 11 % (relative), while `Duuu` agrees with one of `Duu` to `5e-10`.
- **How found:** read during porting; finite-difference check of the bit-identical port.
- **Confirmed:** yes (fixture `third_derivative_outputs` compares the C++ values with the port).
- **Severity:** wrong result.
- **Rust port:** Kept.

#### WR-161. `SmOffsetSurface` outputs `Buuv` in place of `Buvv` and `Bvvv`
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:1482` and `:1491`, `SmOffsetSurface::EvaluatePlainOrSimple`.
- **What happens:** `Duvv` is written as `Buuv + d * Nuvv` and `Dvvv` as `Buuv + d * Nvvv`; the base derivatives belong to `Buvv` and `Bvvv`.
- **Trigger:** `Evaluate(uv, 1, 2, ..)` and `Evaluate(uv, 0, 3, ..)`: against central differences of `Duv` and `Dvv` the results are off by 2.1 and 1.2 (relative).
- **How found:** read during porting; finite-difference check of the bit-identical port.
- **Confirmed:** yes (fixture `third_derivative_outputs`).
- **Severity:** wrong result.
- **Rust port:** Kept.

#### WR-162. `SmOffsetSurface` writes `Duv` over `Duu` for an upper half without `v` derivatives
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:1442`, `SmOffsetSurface::EvaluatePlainOrSimple`.
- **What happens:** With `bOnlyUpperHalf`, `hu = 2` and `hv = 0`, the mixed derivative is computed (the larger order is 2) and written at index `hv + 2 = 2`, the slot of `Duu`.
- **Trigger:** `Evaluate(uv, 2, 0, .., TRUE /*bOnlyUpperHalf*/, ..)` with a non-zero distance.
- **How found:** read during porting.
- **Confirmed:** yes (fixture `upper_half_slot_overwrite`).
- **Severity:** wrong result.
- **Rust port:** Kept.

#### WR-163. `SmOffsetSurface::SplitAt` gives the halves split copies of the base as extended surfaces
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:1682`, `SmOffsetSurface::SplitAt`.
- **What happens:** With an extended surface, the halves' `m_pExtendedSurface` come from splitting `m_pSurface` again instead of `m_pExtendedSurface`, so the extension is lost.
- **Trigger:** `SplitAt` on an offset that has stored an extension.
- **How found:** read during porting.
- **Confirmed:** no (fixture `extension_then_split` waits on `surface.surface_p3`).
- **Severity:** wrong result.
- **Rust port:** Kept.

#### WR-164. `SmOffsetSurface::SwapUV` with an extended surface negates the distance twice
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:1767` and `:1772`, `SmOffsetSurface::SwapUV`.
- **What happens:** Swapping the parameters flips the normal, so the distance is negated; with an extended surface it is negated once for it and once for the base, leaving it unchanged and the offset on the wrong side.
- **Trigger:** `SwapUV` on an owning offset that has stored an extension.
- **How found:** read during porting.
- **Confirmed:** yes (fixture `extension_sequence`).
- **Severity:** wrong result.
- **Rust port:** Kept.

#### WR-165. `SmOffsetSurface::STEPInversion` retries on the extended surface with the unwidened tolerance
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:1737`, `SmOffsetSurface::STEPInversion`.
- **What happens:** The base is inverted with the tolerance widened by the offset distance, but the retry on the extended surface uses `dDistanceTolerance`, so a point on the offset over the extension is not found.
- **Trigger:** `STEPInversion` of a point on the offset over its extended part.
- **How found:** read during porting.
- **Confirmed:** no (fixture `extension_then_step_inversion` waits on `surface.surface_p4`).
- **Severity:** wrong result.
- **Rust port:** Kept.

#### WR-213. `SmBendVolume::WriteToDB` writes ASCII values with no separator before their label
- **Location:** `source/SMLib/src/SmBendVolume.cpp:3255`-`3256` (`rFileOut << m_dBendNeutralDist << "BendVolume Bend NeutralDistance \n"`, same for `m_dSingularityTol`); read back at `:3328`-`3329` (`rFileIn >> pBendNeutralDist[0]`).
- **What happens:** The ASCII line is `0.75845918449404492BendVolume ...`. libstdc++ stops the double at `B`; libc++ `num_get` first gathers every character in `0-9a-fA-FxX+-pPiInN` (`0.758...Bend`), so the extraction fails and the bend volume reads back wrong. The round trip only works with libstdc++.
- **Trigger:** `WriteToDB` then `ReadFromDB` of an `SmBendVolume` (or a curve/surface in a bend volume) in ASCII, built against libc++.
- **How found:** generated difftest cases failing on macOS arm64 only (`surface.srf_in_volume` read_db with a bend volume, 25 cases, e.g. gen1970: `values.out.len: cpp=70 rust=102`).
- **Confirmed:** yes, on macOS arm64 (Apple clang 21, libc++); benign on x86-64 Linux (libstdc++). GitHub issue #450.
- **Severity:** wrong result (platform dependent).
- **Rust port:** Has no ASCII writer; `BendVolume::read_from_db` (`crates/solidcore/src/surface/bend_volume.rs`) reads the double as libstdc++ does (stops at the label), so it matches the reference build.

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

#### WR-100. `SmSurface::DropCurve` tests the domain's u length for the v degeneracy
- **Location:** `source/SMLib/src/SmSurfaceDropLib.cpp:4250` `SmSurface::DropCurve`: `bDegenSurfV = sUVDomain.XLength() < SM_EFF_ZERO` (the line above sets `bDegenSurfU` the same way).
- **What happens:** A drop domain degenerate in v is not reported (`SM_ERR_DEGENERATE_SURFACE`, `SM_CTC_DEGENSURFACE_FAIL`) and the drop goes on; `m_bDegenSurfV` of the failure record copies the u test.
- **Trigger:** Only a natural domain degenerate in v reaches it (a given domain shorter than `SM_EFF_ZERO` in either direction is replaced by the natural one), e.g. a plane built over a domain of zero height.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`Surface::drop_curve_base`).

#### WR-182. `SmSurfaceDropCurve::DoTrace` leaves its start points linked on the curve list
- **Location:** `source/SMLib/src/SmSurfaceDropCurve.cpp:1766` `SmSurfaceDropCurve::TraceCurve2`: `m_vCurvePoints.Append(&rStartPoint)`, called by `DoTrace` (`:1689`-`1703`) for every node of `m_vStartPoints`; `SmTList` is intrusive (`SmTracePnt : SmListNode`, `SmTList.h:224`-`244`).
- **What happens:** The start node is appended to `m_vCurvePoints` without being removed from `m_vStartPoints` (the base tracer removes coincident start points first, `SmSurfaceTracer.cpp:621`-`637`). `Append` overwrites the node's links, so `m_vStartPoints` (a head pointer) then walks the curve ring: the other start points drop out of it and every point later added to the curve shows up in it, and a later `Remove` from either list unlinks the node from the other. `DoTrace` itself iterates a snapshot of the start list, but a `TraceCurve` fallback walks `m_vStartPoints` in its end test and `AddStartPoint`.
- **Trigger:** Any `DoTrace` with a start point (every drop that reaches the trace), followed by a read of the start points or by more curve points.
- **How found:** difftest `intersect.surface_drop_curve` gen0354 (a trace once the offset surface and `SmSurface::DropCurve` were ported): after `DoTrace` and further `ComputeNextPoint`/`TraceCurve2` ops, the C++ start list held the three new curve points.
- **Confirmed:** yes (C++ build, gen0354).
- **Severity:** wrong result (list state; the traversals still end, the nodes live in the block manager).
- **Rust port:** Does not reproduce it: the start and curve lists are id `Vec`s (`intersect::surface_tracer`). The difftest reports a start list that shares a node with the curve list as `-3` on both sides instead of reading it.

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

#### UB-184. A failed `RegisterExistingSSI` leaves the intersection table's lists out of step
- **Location:** `source/SMLib/src/SmTopologyIntersector.cpp:276-315` (`SER(SM_ERR)` at :303), `SmTopologyIntersector::RegisterExistingSSI`; read at :211 by `CheckIfHaveExistingSSI`.
- **What happens:** `RegisterExistingSSI` adds both surfaces to `m_vBrepSurfaces` / `m_vOtherSurfaces` first and the results last. A 3D curve that is not an `SmBSplineCurve` (or a null entry) returns `SM_ERR` from inside the copy loop, so the results list stays one entry shorter (and the copies made so far leak). A later `CheckIfHaveExistingSSI` that matches that surface pair reads `m_vResults[ii]` with `ii` one past the results' size. `m_vResults` is an `SmTArray`, which allocates at least 16 zeroed slots, so there are three outcomes:
  - **Null dereference:** the failed registration was the table's first, so `m_pData` is still NULL.
  - **Zeroed slot:** otherwise, with no later registration. `m_p3DCurves` is NULL there, so the call reports an existing intersection with no curves.
  - **Wrong curves:** after any later successful registration, `ii` indexes the next pair's results, so the failed pair inherits another pair's curves (and every later pair is shifted). This is the likely case in practice.
- **Trigger:** `RegisterExistingSSI(s1, s2, {line or NULL}, ..)` for a non-B-spline 3D curve (an `SmLine`, `SmCompositeCurve`, ...), then `CheckIfHaveExistingSSI(s1, s2)`; also a `crUVCurves1/2` shorter than `cr3DCurves`, which the copy loop indexes in step with the 3D list.
- **How found:** read during porting (`boolean.topology_intersector_p2`, PR #203).
- **Confirmed:** no (source inspection; the difftest shim only registers B-spline curves and equal-length lists).
- **Severity:** undefined behaviour (null dereference) in the first case, a wrong result in the other two, after an error status.
- **Rust port:** Faithful on the table (surfaces added, results not). `CheckIfHaveExistingSSI` then fails with `SolidError::InvalidInput` ("intersection table lists differ in length") in all three cases. For the zeroed and misaligned cases, which are defined upstream, that is a deviation: the port refuses instead of reproducing a wrong result. A UV list shorter than the 3D list is `InvalidInput` in `register_existing_ssi`.

#### WR-170. `IntersectTopology` leaks the copies of registered intersection curves; a failed `IntersectInsertRelate` leaks its manager
- **Location:** `source/SMLib/src/SmTopologyIntersector.cpp:1781-1792` (and the deleters built only at :1874), `IntersectTopology`; :1161, :1188, :1212, `IntersectInsertRelate`.
- **What happens:** For a face pair registered in the intersection table, `CheckIfHaveExistingSSI` returns fresh copies of the registered curves and the loop `continue`s before the `SmObjsDelete` guards that would delete them, so every registered pair leaks its copies (once per call). `IntersectInsertRelate` allocates `m_pProtoTopoMgr` and returns through `SER_MSG` when `IntersectTopology` or `ResolveIntersections` fails, leaving the manager allocated and installed; the next call overwrites the pointer.
- **Trigger:** `RegisterExistingSSI` with curves for a face pair of the two B-reps, then `IntersectInsertRelate`; any failing `IntersectTopology` (e.g. a failing `CoincidenceCheck` or `FaceIntersect`).
- **How found:** read during porting (`boolean.topology_intersector_p2`, PR #203).
- **Confirmed:** no (source inspection).
- **Severity:** memory leak; results unaffected.
- **Rust port:** Faithful: the copies stay unreferenced in the caller's curve store and are not reported as deleted; after a failure the manager stays installed on the intersector.

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

#### UB-167. `SmApiStitch(SmBrep*, SmFace*)` always crashes: `SmMerge` dereferences its null other B-rep
- **Location:** `source/SM_API/src/SmApiBrep.cpp:1157`-`1188` (`SmApiStitch(pBrep, pFaceToStitch)`: `SmMerge sMerge( *pContext, pBrep, NULL )`), then `source/SMLib/src/SmMerge.cpp:98`-`125` (`SmMerge::SmMerge`: `if( dApproxTol3d < SM_EFF_ZERO )` ... `pOtherBrep->GetMinEdgeBBoxSize(dEdge2Size, pE2)`).
- **What happens:** The constructor comment says `pOtherBrep` "can now be Null, for the Stitch methods", but `SmApiStitch` passes the default `dApproxTol3d = 0`, so the automatic-tolerance branch runs and calls `GetMinEdgeBBoxSize` on the null other B-rep (SIGSEGV in `SmBrep::GetEdges` with `this == NULL`), before the face is looked at.
- **Trigger:** any call with a non-null B-rep and a non-null face, e.g. a `SmApiCreateBox` box and one of its faces.
- **How found:** difftest fuzzing of `heal.stitch_p1` (PR #196): 4 crashes in 2000 programs; gdb backtrace `SmBrep::GetEdges <- SmBrep::GetMinEdgeBBoxSize <- SmMerge::SmMerge <- SmApiStitch`.
- **Confirmed:** yes (C++ reference build, standalone reproducer under gdb).
- **Severity:** crash/UB.
- **Rust port:** `heal::stitch::p1::api_stitch_face` builds the merge with `MergeState::with_breps(.., None, 0.0, 0.0, None)` (PR #229), which returns `InvalidInput` where upstream dereferences the null other B-rep (after the primary's `GetMinEdgeBBoxSize`); the difftest FFIs refuse `SmApiStitch` with a B-rep and a face (`heal.stitch_p1`) and a computed tolerance without other B-rep (`boolean.merge_p2`).

#### UB-176. Two-list `SmStitch::StitchVertexPairs` glues a vertex twice
- **Location:** `source/SMLib/src/SmStitch.cpp:3242`-`3749` (the two-list `StitchVertexPairs`): each glue deletes `sUsingVerts2[...]` and nulls only its own slot.
- **What happens:** A vertex repeated in `rVertexList2`, or present in both lists, is glued (and deleted) once per occurrence, so a later `GlueVertices` gets an already-deleted vertex (double free / use after free). The sorting loops also index `sSortedIndex1` / `sSortedIndex2` with the unfiltered counts, reading past them when a list holds `NULL` entries.
- **Trigger:** caller-built lists with a repeated vertex, a vertex in both lists, or a `NULL` entry. `SmMerge::MergeAddedTopology` (`source/SMLib/src/SmMerge.cpp:5735`) reaches it with a vertex repeated in its caller's `pMergeVerts`, which it copies into `rVertexList2` unchecked: e.g. the ASCII `plane_square.smb` with vertices `[0, 0]`, no faces or edges, tolerance 10 (stitch tolerance 40).
- **How found:** source review while porting `heal.stitch_p1` (PR #196); the difftest FFI refuses repeats, shared vertices and `NULL`s. The `MergeAddedTopology` route: an adversarial `boolean.merge_p2` fixture run with that unit's duplicate guard relaxed (PR #229).
- **Confirmed:** yes (C++ reference build, through `MergeAddedTopology`: segfault, exit 139; the same request without the repeated vertex succeeds).
- **Severity:** crash/UB.
- **Rust port:** `heal::stitch::p1::Stitch::stitch_vertex_pairs` returns `InvalidInput` where the sorted index arrays would be read past; repeated vertices are a caller error the FFI refuses. `boolean.merge_p2`'s FFI refuses any entity listed twice in a `MergeAddedTopology` list.

#### UB-177. `SmStitch::DoSimpleFaceStitch` glues vertices that are already freed [PSIRT]
- **Location:** `source/SMLib/src/SmStitch.cpp:680`-`681` (`sKVertices.Add` / `sDVertices.Add` across all face pairs), `:785` (`pBrepToStitch->RemoveFaces(cpFacesToDelete)`), then the glue loop at `:840`-`916` (`pV1->IsLaminaVertex()`, `SelectTopologyToKeep`, `GlueVertices` / `SqueezeEdge`).
- **What happens:** The vertex pair lists are built with plain `Add` over every face pair, and the pair validation is per pair. Once it passes:
  - `RemoveFaces` deletes every vertex used only by the delete faces, and the glue loop then reads those freed `sDVertices` (use after free).
  - A face repeated in the delete list is freed twice.
  - A vertex that appears in more than one list entry is glued (the other vertex deleted by `GlueVertices` / `SqueezeEdge`) at its first entry and read again at the next (use after free). Nothing checks that a vertex is still alive. That happens for a vertex shared by two keep faces or by two delete faces, for a keep face repeated in the keep list, and for a vertex shared by a keep and a delete face (which pairs with itself, `GlueVertices(v, v)`).
  - Upstream's own comment calls the routine "never called" and its scenario (laminar copies of manifold faces) "questionable".
- **Trigger:** delete faces whose vertices no other face uses (any lamina face pair); a repeated delete face; or face lists whose faces share a vertex across pairs. The last case needs no bad argument: two distinct keep faces that are adjacent in a B-rep read from a file, stitched onto the two coincident faces of another shell (the laminar-copy scenario the routine is written for), put their common vertices in two entries.
- **How found:** source review while porting `heal.stitch_p1` (PR #196); the cross-pair repeat was found by the #196 delta review.
- **Confirmed:** no (source inspection only; the shim never runs the undefined part).
- **Severity:** crash/UB. **[PSIRT]:** the cross-pair repeat is reachable from geometry read from a file with well-formed `SmApiSimpleFaceStitch` face lists.
- **Rust port:** `heal::stitch::p1::Stitch::do_simple_face_stitch` refuses those inputs with `deref` (`InvalidInput`, the null-dereference guard) before removing any face: a repeated delete face, a vertex in more than one entry of the two vertex lists, or a delete vertex no other face uses. The difftest shim refuses exactly the same inputs after a read-only copy of the validation. A repeated vertex whose first entry would free nothing (squeeze skipped, a short manifold common edge, the `m_bMakingManifoldSolid` non-lamina skip, or the loop ending first with upstream's defined `SM_ERR`) is refused too, a deliberate over-refusal.

#### UB-192. `SmFilletSolver::PointOnPlaneSolve` writes solution values past `m_adUserDoubles`
- **Location:** `source/SMLib/src/SmFilletSolver.cpp:3054`-`3057` (`SmFilletSolver::PointOnPlaneSolve`, the found-solution copy).
- **What happens:** Every solution value after the four UV values is copied to `rTsectPnt.m_adUserDoubles[i-4]` with no bound; the array holds `SM_MAX_USER_DOUBLES` (4) values. The sibling copies in `RailEdgeuseIntersect` and `RailRailIntersect` check `lCnt == SM_MAX_USER_DOUBLES` and return `SM_ERR`.
- **Trigger:** a solver whose `GetJacobianSize` / `LoadInitialValues` give a solution vector longer than 8 values (a derived solver of calling code; the shipped solvers stay at or below 8).
- **How found:** read during porting (unit `blend.fillet_solver_p2`).
- **Confirmed:** no.
- **Severity:** crash/UB (out-of-bounds write into the `SmTsectPnt`).
- **Cause:** missing `SM_MAX_USER_DOUBLES` check.
- **Rust port:** `blend::fillet_solver::p2::FilletSolver::point_on_plane_solve` returns `InvalidInput` for a fifth extra value.

#### HANG-30. `SmApiStitch(SmBrep*)` never returns on a sheet with a coordinate near the top of the double range
- **Location:** `source/SM_API/src/SmApiBrep.cpp:1128`-`1151` (`SmApiStitch(pBrep)`) -> `SmBrep::StitchAndOrient` (unit `topology.brep_p5`); the looping routine is not located.
- **What happens:** A one-face sheet whose biquadratic surface has one control point at `-1e300` (the others about `1e-3`) makes `StitchAndOrient` run past 20 s without returning, also when run alone; the same class as HANG-20 (box arithmetic overflowing on a coordinate in the top decades of the double range).
- **Trigger:** `SmApiStitch` on such a sheet (difftest fuzz program `fuzz01923` of `heal.stitch_p1`).
- **How found:** difftest fuzzing of `heal.stitch_p1` (PR #196).
- **Confirmed:** yes (C++ reference run, reproduced alone by the fuzz harness).
- **Severity:** hang.
- **Rust port:** `heal::stitch::p1::api_stitch` defers at `SmBrep::StitchAndOrient` (`topology.brep_p5`); the difftest FFI bounds sheet values at 1e150 for the full-stitch API entry points.

#### HANG-37. B-spline surface extension does not return for a control point of magnitude 1e157 or more
- **Location:** `source/SMLib/src/SmBSplineSurfaceNLib.cpp:4583` (`SmBSplineSurface::CreateExtendedSurface`), reached from `SmOffsetExecutive::GetCreateExtendedSurface` (`source/SMLib/src/SmOffsetExecutive.cpp:2148`) with `SM_SP_BOTH`, five times the (nonnegative) offset magnitude and `SM_CT_G1R`.
- **What happens:** With one control point coordinate at `1e157` or beyond, the extension call gives no result within 25 s; the stuck loop was not isolated (HANG-16 and HANG-17 describe the same routine's doubling loop and its exponential split; a coordinate of that size overflows the squared distances the extension-by-distance code compares).
- **Trigger:** `--fuzz 2000` of `offset.offset_executive_p1` (case `fuzz00331`): a 3x3 degree-2 rational surface with one control point at `x = -1e300`, extension distance `-0.0`, approximation tolerance `4.5e-4`. The sign of the zero distance does not matter.
- **How found:** PR #282's fuzz gate on the tree merged with main; the case timed out alone as well. Timing with that coordinate replaced, each axis in turn (pod sc-builder-0, release build): `1e154`, `1e155`, `1e156` return in 25-45 ms on x, y and z; `1e157` and `1e158` give no result within 25 s on every axis; `1e160`, `1e180`, `1e200`, `1e250`, `1e300` likewise on x.
- **Confirmed:** yes (no result within 25 s, reproduced on 15 inputs across three axes; infinite execution not proven).
- **Severity:** hang on out-of-range geometry; not a parity blocker (no fixture or generated case reaches it).
- **Rust port:** the `offset.offset_executive_p1` FFI refuses control point coordinates (and coordinate/weight) above `MAX_COORDINATE = 1e154` in its `points()` guard, 1000x below the first slow value; the difftest runner's Rust side runs the same check, and the input is kept as a regression in `crates/difftest/tests/fuzz_regressions/`.

### Primitive construction

#### WR-209. `SmApiCreateBox` reports success for incomplete box topology
- **Location:** `source/SMLib/src/SmPrimitiveCreation.cpp:294-344` (`CreateBox`) and `source/SM_API/src/SmApiPrimitives.cpp:306-324` (`SmApiCreateBox`).
- **What happens:** `CreateBox` ignores all six `CreateFaceFromSurface` statuses. The API can return `SM_SUCCESS` with a non-null B-rep that lacks the expected six faces, twelve edges and eight vertices; callers cannot rely on that success status to select box faces.
- **Trigger:** Positive box dimensions `[1e-6, 1e-6, 1e-6]` at `[0, 0, 0]`, or `[1e-3, 1e-3, 1e-3]` at `[1e6, 1e6, 1e6]`, under the reference context's construction tolerances. Applying B-rep tolerance `1e-9` afterwards does not repair the failed construction.
- **How found:** adversarial review of `offset.offset_executive_p2` (PR #201), then fixed native fixtures `review_box_tiny_*` and `review_box_translated_*` in `fixtures/offset.offset_executive_p2/review_box_construction.json`.
- **Confirmed:** yes, Linux ARM64 C++ reference build at `c9c5979`; eight source/foreign and null/indexed-selector fixtures reject incomplete construction, while two ordinary-size controls construct the expected topology and reach the real API selection error.
- **Severity:** wrong result / hidden construction failure.
- **Rust port:** No box-construction parity is claimed by the input scaffold. The native fixture adapter checks actual topology counts immediately after construction and records an explicit constructor rejection, before selector resolution or offset/shell execution. It does not substitute a successful Rust shape or impose a size cutoff.

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

#### WR-103. `CreateSphereFromArcs`' centre check compares a frame with itself
- **Location:** `source/SMLib/src/SmBSplineSurface.cpp:2044`-`2065`, `SmBSplineSurface::CreateSphereFromArcs`: `const SmPoint3d & rOrigin = sRefFrame.GetOriginRef();` then, for each other arc, `IsArc(5, d3DTol, sRefFrame, ...)` and `sDiff = sRefFrame.GetOriginRef() - rOrigin`.
- **What happens:** `rOrigin` is a reference into `sRefFrame`, which the later `IsArc` calls overwrite, so `sDiff` is always zero: arcs about different centres pass the "OtherCurve center is not the same as 1st Curve cent" test, and every later use of `rOrigin` (the sphere centre) sees the centre of whichever arc `IsArc` examined last.
- **Trigger:** three arcs of equal radius about different centres; a generated `offset.offset_geometry_creation` case (`VertexOffset` with bounding edges from two B-reps: two torus circles about (10, 0, 0) and a sphere circle about the origin, all of radius 5) builds a sphere from them.
- **How found:** difftest of `offset.offset_geometry_creation` (PR #156): the port captured the first centre by value, rejected the arcs with `SM_ERR`, and disagreed with the C++, which returned three trimming curves.
- **Confirmed:** yes (the C++ build succeeds on the inputs above).
- **Severity:** a silent wrong result (a sphere from non-concentric arcs); no crash.
- **Rust port:** reproduces the bug. `nurbs::b_spline_surface::p2::BSplineSurface::create_sphere_from_arcs` first captured the first arc's centre by value (and so rejected non-concentric arcs that upstream accepts); since PR #156 it reads the current frame's origin at each use, as the aliased reference does, and the dead check is kept as a comment. That unit's `three_arcs_about_two_centres` fixture case pins the behaviour: three radius-5 arcs (120 degrees about the origin, 90 degrees about (10, 0, 0) sharing its start point, 60 degrees about (10, 0, 0)) from which the C++ builds a sphere; with the centre check put back, the port rejects them and the case fails.

#### WR-186. `SmStitch::DoStitching` never resets its candidate flags while splitting edges
- **Location:** `source/SMLib/src/SmStitch.cpp:1423`-`1500` (`DoStitching`, edge splitting at vertices).
- **What happens:** `bCandidateVtx` and `bCandidateEdge` start `TRUE` before the vertex loop and are set `FALSE` for the first vertex (edge) not on the candidate list, but never set back. Every later vertex and edge of the pass is treated as a non-candidate: pairs where both are later are skipped although they may be candidates, and the split vertex and edge are re-added to the candidate lists.
- **Trigger:** `DoStitching` with a candidate vertex or edge list and `m_bSplitEdgesWithVertices`.
- **How found:** read during porting (`heal.stitch_p2`).
- **Confirmed:** no (source inspection; every `DoStitching` case defers at `SmBrep::Notify`).
- **Severity:** wrong result.
- **Rust port:** Reproduces it.

#### WR-187. One-list `SmStitch::StitchVertexPairs` reports a cancelled squeeze as done
- **Location:** `source/SMLib/src/SmStitch.cpp:3127`-`3194` (one-list `StitchVertexPairs`).
- **What happens:** A second short common edge cancels the squeeze (`bDoSqueeze = bDoGlue = FALSE`), but `pSqueezeEdge` keeps the first short lamina edge. Nothing is glued or squeezed, yet the pair's vertex is added to `rDeletedVerts`, `pSqueezeEdge` to `rDeletedEdges`, the list entry is cleared and, keeping the second vertex, `pV` moves to it. The same happens when the short common edge is manifold (no edge reported).
- **Trigger:** Two coincident vertices joined by two short edges, or by one short manifold edge.
- **How found:** read during porting (`heal.stitch_p2`).
- **Confirmed:** no (source inspection).
- **Severity:** wrong result.
- **Rust port:** Reproduces it.

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

#### DOC-5. `SmApiCreateTaperExtrude` documents 90-180 degrees as an outer draft, but the code takes the side from the angle's sign
- **Location:** `source/SM_API/inc/SmApiBrep.h:318` (`dDraftAngleDeg`: "0-90 inner, 90-180 outer") and `source/SMLib/src/SmPrimitiveCreation.cpp:3609` (the same in the parameter comment of `SmPrimitiveCreation::CreateTaperExtrude`); behaviour at `:3728` and `:3731`. The method comment at `:3564-3565` agrees with the code ("typically 30 degrees for an inner taper, and -30 degrees for an outer taper").
- **What happens:** Only the sign picks the side (`dDraftAngleDeg < 0` gives `SM_OD_RIGHT_HAND_SIDE`, anything else `SM_OD_LEFT_HAND_SIDE`), and the in-plane offset is `|dHeight * tan(|dDraftAngleDeg|)|`. An angle between 90 and 180 degrees therefore drafts to the same side as a positive acute angle, by `|tan(180° - angle)|`: 135 gives the taper of 45, not an outer one. An outer taper needs a negative angle. Neither comment says that the curves must form one chain either: `SM_ASSERT(nChain == 1)` at `:3678` is the only check, and it compiles to nothing without `SM_DEBUG_CODE` (`SmMessages.h:714`), so a profile with an inner loop is not refused.
- **Trigger:** `SmApiCreateTaperExtrude` with `dDraftAngleDeg` 135 against 45 (same side, same offset), or with a profile of two closed loops.
- **How found:** read during porting, following tangency-labs/solidcore#413 (reported from SolidCraft's move onto SMLib).
- **Confirmed:** no.
- **Severity:** docs.
- **Rust port:** Faithful to the code: `PrimitiveCreation::create_taper_extrude` (`primitive/primitive_creation/p2/taper.rs`) takes the side from the sign, makes the offset positive and does not count the chains. Its doc comment, and that of the API wrapper `sweep::api_brep::create_taper_extrude` (which stops at `SolidError::Unported` until the B-rep constructor is ported), state that convention.


#### `ST_curxcdr` G1 fallback reads a third point of a two-point line (primitive.primitive_creation_p1, PR #213)

- **Location:** `source/SMLib/src/NL_CrvAdv.cpp:999-1006,1077-1084`.
- **Trigger:** a two-control-point line whose endpoint difference cannot be
  normalized by `N_VectorNormalize`, then G1 extension at the affected end.
  Finite coordinates around `1e300` overflow the computed vector magnitude.
  The fallback reads `Pw[2]` at the start or `Pw[n-2]` (index -1) at the end.
- **Found by:** the primitive path helper's 1,000-case FFI fuzz run (cases 427,
  670, 689 and 919), initially reporting history-dependent garbage extension
  intervals instead of Rust's checked error/unchanged curve.
- **Confirmation:** the linked native `N_VectorNormalize` fails for those exact
  two points; source inspection then proves the next read is out of bounds.
  The revised shim prevents that read by preflighting only extension stages
  actually selected by the upstream join and closed/periodic decisions. Huge
  single lines that are not extended remain accepted. Rejected cases are
  excluded from parity. The Rust NLib port already bounds its fallback lookup.

## CreateLinearSweep: negative full-circle distance does not terminate (PR #213 review, 2026-10-19)

- Location: `source/SMLib/src/SmPrimitiveCreation.cpp:1290-1360`, the full-circle
  analytic-cylinder branch at `SmCone::AdjustSTEPUVDomain`, before face construction.
- Trigger: one unit circular B-spline or analytic SmCircle in XY, sweep direction +Z, distance -1,
  with or without end caps. Positive and zero distances finish; negative line,
  skew-line and open-arc sweeps also finish.
- Found by new PR #213 review fixtures `linear_circle_cap{0,1}_distance-1`; the analytic generator family adds two further rejected witnesses.
  Confirmed by the native shim running the included upstream implementation:
  both isolated cases exceeded the 10/5-second limits; the combined run ran
  for over a minute without completing. An interrupted GDB run locates the loop
  in `N_CreateEllipticalArcAngTypeTol` via `SmEllipse::MakeNurb`,
  `SmCone::CreateIsoParametricCurve`, `SmSurfOfRevolution::RebuildPolarConverter`,
  `Reparameterize` and `SmCone::AdjustSTEPUVDomain`. Rust reaches its face-construction
  dependency, so these are rejected native fixtures, not parity comparisons.
- The shim preflight repeats the upstream full-circle angular/tolerance and
  axis-parallel tests and rejects only negative-distance inputs selecting that
  cylinder branch. No kernel tolerance is changed and negative line/open-arc
  fixtures remain accepted.

#### UB-188. `BooleanTrees` trusts executed leaf indices and binary operand pointers

- **Unit:** `boolean.merge_p3` (PR #245).
- **Location:** `source/SMLib/src/SmMerge.cpp:1850-1888`, `SmMerge::BooleanTrees`.
- **Trigger:** an executed nonnegative leaf outside `rBreps` indexes the array unchecked. An executed binary node dereferences its popped primary operand for `GetContext()` and both operands during the zero-tolerance merge constructor; null input slots can arise from repeated leaf indices. A repeated leaf that is never dereferenced by a later binary node is a defined null output and is accepted by the new guard.
- **How found:** source audit while implementing the postfix evaluator and its FFI guard. No crashing program was executed.
- **Confirmed: no.** No C++ reproduction was run; the source contains unchecked indexing and null dereferences.
- **Guard:** the `boolean_merge_p3` FFI and shim simulate reachable leaf consumption and binary stack/null state before building native objects. They preserve safe underflow errors, unknown-operation errors, repeated leaves with no dereference, and unreachable invalid leaves after a definite early error. The Rust evaluator bounds leaf indexing, keeps null slots, and returns `InvalidInput` at an attempted binary null dereference. No exploit fixture is included.

#### WR-196. `BooleanLists` slices derived surfaces into base `SmSurface` objects

- **Unit:** `boolean.merge_p3` (PR #245).
- **Location:** `source/SMLib/src/SmMerge.cpp`, `SmMerge::BooleanLists`, both input surface-copy loops and the final output surface-copy loop; `source/SMLib/src/SmSurface.cpp:731` (base copy constructor), `source/SMLib/inc/SmSurface.h:1096` (base natural domain).
- **Trigger:** a nonempty surface list without a preceding B-rep list. The function uses `new (*pContext) SmSurface(*pOrig)` rather than virtual `Copy`. This creates a concrete base object, discards the derived geometry, returns the base's zero natural domain, and makes the subsequent `IsKindOf(SmPlane_TYPE)` false. The intended planar dispatch therefore cannot be selected by these copied derived inputs. The output loop also constructs base-only copies.
- **How found:** source audit against the base constructor, base `GetNaturalUVDomain`, and the `SM_COMMON` virtual type methods while porting the list branches.
- **Confirmed: no.** No C++ reproduction was run; this entry records the source-level slicing behavior and the mismatch with the function's stated planar intent.
- **Rust handling:** the current `SurfaceKind` has no base-only variant. `boolean::merge::p3::copy_base_surface` defers that missing constructor branch specifically to the partial `surface.surface_p1` owner. It does not silently preserve derived geometry by invoking virtual `Surface::copy`; all subsequent list logic remains ported.

#### UB-190. Piecewise imprinting deletes an operand needed by later surfaces

- **Unit:** `boolean.merge_p3` (PR #245).
- **Location:** `source/SMLib/src/SmMerge.cpp:3103-3110`, `SmMerge::ManifoldBoolean` imprint early return; contrast the normal `SM_BO_PARTIAL_MERGE` retention at `:5056-5062` and `PiecewiseMerge`'s per-surface reuse of `pOtherBrep`.
- **Trigger:** `PiecewiseMerge` with `SmMergeOptions::m_bImprinting = TRUE` and `m_bKeepOtherBrep = FALSE`, with multiple other surfaces. The first partial merge deletes the original other B-rep. A later surface reassigns its saved pointer and reaches `SmTemporaryBrepTransform` / `CalculateBoundingBox` on freed memory. The early return omits the normal exit's partial-merge exception. Upstream's own piecewise callers do not supply imprinting options.
- **How found:** source comparison following the overseer's independent diagnostic of `upstream_piecewise_boxes_2` and `_3`, on two 10×10×10 boxes offset by 5 in X. This author did not debug or replay the crashing inputs.
- **Confirmed:** the separate diagnostic on `sc-builder-4` at `8df5981c` observed SIGSEGV and the original other B-rep's destruction during the first `ManifoldBoolean(SM_BO_PARTIAL_MERGE)` call. The next surface used that freed B-rep. This entry records the source-level lifetime defect; the provided diagnostic is archived in the swarm status directory.
- **Guard:** before native construction, the FFI and shim refuse family 4 when the other operand is a box (six surfaces), an options pointer is present, imprinting (`mask & 2`) is on, and keep-other (`mask & 16`) is off. A shell-vertex other operand has no surfaces and never enters the per-surface loop (`SmMerge.cpp:394-407,441,772-778`), so it stays accepted, including when the primary is a box. All other families, non-imprinting options, null options, and imprinting with keep-other remain accepted. The two original fixtures assert this specific refusal without executing C++; `mask | 16` variants keep the imprinting branch exercised. No kernel result is fabricated and no Rust dependency deferral is used to label the refusal.

#### UB-185. Fillet executive initialized flag is indeterminate before parameter setup
- **Unit:** `blend.fillet_executive_p3` (constructor owned by executive p1).
- **Location:** `source/SMLib/src/SmFilletExecutive.cpp:487-516,1574`.
- **Trigger:** Construct `SmFilletExecutive` and call `DoFillet` before parameter
  setup. Its constructor never initializes `m_bIsInitialized`, but `DoFillet`
  immediately reads that member for its precondition check.
- **How found:** Source inspection while porting the three executive methods.
- **Confirmed:** no; no uninitialized native read was executed.
- **Guard:** The native fixture's derived executive explicitly writes
  `m_bIsInitialized` for every case before invoking an operation. Rust requires
  callers to supply an initialized `bool` in the executive state.

#### UB-206. `SmFilletExecutive::SetFilletParameters` dereferences a null `pBrep` and a null `pOptVarRadLaws`
- **Location:** `source/SMLib/src/SmFilletExecutive.cpp:917` (`pBrep->GetContext()`) and `:998` (`pOptVarRadLaws->GetSize()`), the edge-pointer overload at `:865`.
- **What happens:** `pBrep` may be null (`:884` only skips the target assignment), but once the edge list is non-empty and the list sizes pass, `:917` reads its context. `pOptVarRadLaws` is optional (`NULL` default), yet the first variable-radius edge reads its size.
- **Trigger:** a null B-rep with a non-empty edge list whose type and value lists have one entry or one per edge; or a variable-radius type with a null law list (with a valid cross-section type, so the edge loop is reached).
- **How found:** read during porting (`blend.fillet_executive_p2`, PR #267).
- **Confirmed:** no. The shim refuses exactly these inputs, following upstream's order of early returns.
- **Severity:** crash/UB (null dereference).
- **Rust port:** `SM_ERR_NULL_POINTER` at the same points; the difftest compares both as guard errors.

#### UB-207. RETIRED 2026-10-20 (was: the elliptical `SmBSplineFilletLaw` keeps a null law curve after a failed interpolation)
- **Reason:** no triggering input exists. The pre-review of PR #267 probed zero span with equal radii, zero radii, span pi and span 1000, among others, and both sides built the full degree-2 curve every time; the interpolation parameters are the distinct `i / 100`. The port still returns an interpolation error where upstream would keep a null curve.

#### HANG-36. RETIRED 2026-10-20 (was: `CollectTangentEdgeSequence` never returns on a tangent cycle without its first solver)
- **Reason:** the trigger can't occur. The four `GetTangentFilletSolver` overrides (`SmFilletCorner.cpp:4448`, `:4573`, `:5141`, `:6869`) pair a corner's two solvers symmetrically, or return the solver itself for a closed edge, so every walk ends at NULL or back at `pFS`. A foreign corner makes `GetOtherFilletCorner` (`SmFilletSolver.cpp:235`) return NULL, but the walk only ever passes a corner the solver owns, so that NULL is not reached either. The port keeps its walk bound as a defence against adapter input.

#### WR-208. `SmFilletExecutive` leaks the edge-use arrays `LoadFilletSolver` puts in `m_vVertexCornerMap` for vertices that get no corner
- **Location:** `source/SMLib/src/SmFilletExecutive.cpp:1803-1808` (`LoadFilletSolver`: `new SmTArray<SmEdgeuse*>` per newly filleted vertex, stored in `m_vVertexCornerMap`); `:1844-1870` (`CreateFilletCorners` hands the array to `SmFilletCorner::Create` and replaces the map entry with the corner, which deletes it in `~SmFilletCorner`); the destructor (`~SmFilletExecutive`) never frees map values.
- **What happens:** an array leaks for every filleted vertex whose corner is never created: the executive is destroyed before `CreateFilletCorners`, or `SmFilletCorner::Create` returns `NULL` for the vertex (`continue` at `:1863`).
- **Trigger:** `SetFilletParameters` on edges of a box, then destroying the executive without `CreateFilletCorners` (48 + 128 bytes per filleted vertex).
- **How found:** valgrind over the `blend.fillet_executive_p2` difftest cases (PR #267).
- **Confirmed:** yes (valgrind: definitely lost blocks allocated in `SmFilletExecutive::LoadFilletSolver`, no invalid reads or writes).
- **Severity:** memory leak; results unaffected.
- **Related:** UB-209 (the same arrays, read as corners by `CollectTangentEdgeSequence`).
- **Rust port:** no leak (the executive owns its records).

#### UB-209. `CollectTangentEdgeSequence` calls `GetTangentFilletSolver` on the raw edge-use array of a vertex without a corner
- **Location:** `source/SMLib/src/SmFilletExecutive.cpp:1803-1808` (`LoadFilletSolver` stores a `SmTArray<SmEdgeuse*>` for each filleted vertex in `m_vVertexCornerMap`); `:1844-1863` (`CreateFilletCorners` replaces it with the corner only when `SmFilletCorner::Create` succeeds, and otherwise `continue`s and still returns success); `:1605` (`GetFilletCornerOfVertex` C-casts the map value to `SmFilletCorner*`, reached through `SmFilletSolver::GetFilletCorner`, `SmFilletSolver.cpp:221-226`); `:4620-4622` (`CollectTangentEdgeSequence`: `NER( pCorner0 )` passes, then `pCorner0->GetTangentFilletSolver( pFS )`; the same at `:4642`, `:4660`, `:4681`).
- **What happens:** for a vertex whose corner `Create` does not implement ("Corner Case Not Implemented", `SmFilletCorner.cpp:1304`, `:1317`, `:1375`), the map entry stays the edge-use array. The tangent walk takes it for a corner and makes a virtual call on an object of the wrong type.
- **Trigger:** through the public API: `DoFillet` runs `CreateFilletCorners`, then `CreateFilletBrep` (`:1938`) calls `ProcessSmoothEdgeSequences`, then `CollectTangentEdgeSequence`, on an edge set where one filleted vertex has no implemented corner (e.g. two filleted edges at an apex of five or more edges) and at least two solvers remain.
- **How found:** pre-review of PR #267 (`blend.fillet_executive_p2`), by source reading.
- **Confirmed:** no (no native reproduction built; the shim's executive ops never fillet such a vertex).
- **Severity:** crash/UB (virtual call through a mistyped pointer).
- **Related:** WR-208 (the same `m_vVertexCornerMap` arrays leak for vertices without a corner).
- **Rust port:** `ExecutiveAccess::solver_corner` yields `None` for a vertex without a corner, so the walk returns `SM_ERR_NULL_POINTER` (upstream's `NER`) instead.

#### WR-210. `MakeFacesWithCurves` counts each outer-loop group's curves with the patch's first loop counts
- **Location:** `source/SMLib/src/SmBrep.cpp:17590-17607` (`SmBrep::MakeFacesWithCurves`, split branch: the loop over `ll < lNumLoopsInGroup` reads `sThisPatchLoops.m_sLoopCounts[ll]`, but the curves and orientations from `sThisLoopGroup.m_sCurves[mm]` / `m_sOrients[mm]`).
- **What happens:** `sThisLoopGroup` holds only the current outer-loop group's loops, while the counts come from the patch's loop list from its start. For the first group of a sub-patch the two agree; for a later group the counts are those of the first group's loops, so the face gets the wrong number of curves per loop, and reading past the group's curves is an out-of-bounds read.
- **Trigger:** a surface with a `C0` crease and two disjoint outer loops (each with its own holes) on one side of the crease, so that `FindOuterLoops` returns two groups for one sub-patch.
- **How found:** read during porting (`topology.brep_p8`, PR #72).
- **Confirmed:** no (no native reproduction built).
- **Severity:** wrong topology, or UB (out-of-bounds read) when the first group has more curves.
- **Rust port:** keeps the counts; a read past the group's curves is `SolidError::InvalidInput`.

#### WR-212. `DoFillet` returns success with a body that is not a closed manifold solid
- **Location:** `source/SMLib/src/SmFilletExecutive.cpp`, the end of `DoFilleting`'s trim-and-merge path: `OrientTrimmedSurfaces` reports `bMaybeNotSolid`, which is ignored, and nothing after it checks the merged target. Line numbers are not given: they were not checked against `c9c5979`.
- **What happens:** an R4 round over the rim of a through hole merges into a target that is not a closed manifold solid, and `DoFillet` returns `SM_SUCCESS` for it. The callers get an invalid body as a result, not a refusal naming the edge.
- **Trigger:** a 40x20x10 block minus a through hole of r=3 centred at (20, 14.5), then `DoFillet` with radius 4 on the top long edge at y=20. The round covers y=16..20 and the hole reaches y=17.5, so the hole breaks into the round. This is SolidCraft's rig case `tests/kernel-cases/fillets/324-fillet-over-hole-overlap` (tangency-labs/solidcraft, branch `kernel-smlib`).
- **How found:** SolidCraft's kernel rig, through the tangency-labs/smlib fork (`smlib_blend` -> `SmFilletExecutive::DoFillet`); tangency-labs/solidcore#422.
- **Confirmed:** no (not reproduced with the reference build in this harness; seen on the fork's C++ in SolidCraft's rig).
- **Severity:** wrong result (an invalid body reported as success).
- **Rust port:** `FilletExecutive::do_fillet` refuses a target with an edge that is not `SmEdge::IsManifold`, with `SM_FILERR_BAD_MERGE` and the edge in the error info and in the error (`plan/findings.md`, `blend.fillet_executive_p3`). The cause of the broken shell is not found.

#### Issue #489. `FaceFaceIntersect` discards the intersection UV curves when inserting edges

- **Location:** `source/SMLib/src/SmTopologyIntersector.cpp:8246,8294`, the two `MakeEdgeInFace` calls in `SmTopologyIntersector::FaceFaceIntersect`.
- **What happens:** each face's classification retains the intersection UV curve, but edge insertion receives `NULL` instead. `SmBrep::MakeEdgeInFaceWithVU` projects the 3D curve when no UV trim is supplied, adding another approximation.
- **Trigger:** a new edge inserted by this face/face path with a current intersection UV curve available. Issue #489 reports 2–4e-6 gaps on threaded-shaft flank/shaft edges (the #474 reproduction).
- **How found:** issue #489 from SolidCraft's `kernel-smlib` regression rig; confirmed the explicit-null calls by reading the pinned source.
- **Confirmed:** source confirmed; the gap magnitude and complete shaft operation have not been reproduced with this harness's C++ reference.
- **Severity:** accuracy loss from an additional projection fit.
- **Rust port:** copies and forwards the classification's current UV curve in `TopologyIntersector::make_edge_for_interval`; absent/stale UVs retain projection. The path still defers at insertion adapters, so end-to-end gap elimination is not certified (see `plan/findings.md`, issue #489).

#### WR-214. `SmProtoEdge::Resolve` splits a self-loop proto edge onto one vertex, after `CanCombineVertexDefinitions` merged its ends
- **Location:** `source/SMLib/src/SmProtoTopology.cpp:3407`-`3462` (`SmProtoEdge::Resolve`, the `m_pStartPV == m_pEndPV` block: `if(dDist > 2*m_dTol)` at `:3435`, `if(bAddVtxDef)` at `:3450`, where the new definition copies `pVD0`'s two point classifications); `:6372`-`6378` (`SmProtoTopologyManager::CanCombineVertexDefinitions`, `sdTolFactor = 5.0`: definitions up to `5 * (tol1 + tol2)` apart are combined, the one on lower-dimensional topology kept); the failure surfaces at `source/SMLib/src/SmMerge.cpp:7071` (`SmMerge::MakeFaceuse`, the `else { SER(SM_ERR); }` of the edge-use orientation lookup).
- **What happens:** Proto vertices capture positions within `max(0.01, 2000 * tol)` (`SmTopologyIntersector.cpp:1159`). A crossing of two edges and a vertex of one B-rep within `5 * (tol1 + tol2)` of it end up in one proto vertex, and `SmProtoVertex::Resolve` keeps only the definition on the vertex. The short intersection curve between the two points then runs from that proto vertex back to itself; when its ends are more than `2 * m_dTol` apart, `Resolve` adds a second vertex definition copying the first one's classifications. Both definitions are on the same vertex in that B-rep, so `SmVertexDefinition::Imprint` merges both positions into it (`MergeIntoObject` moves it to their midpoint), while the other B-rep's edge is split twice and gets a short edge between two vertices whose mates are one vertex. `MakeFaceuse` finds the mate of the short edge's vertex is neither end of the mated edge and returns `SM_ERR`. The two thresholds (combine at `<= 5 * (tol1 + tol2)`, re-split at `> 2 * m_dTol`) disagree for distances in between.
- **Trigger:** `SmApiBooleanUnion` of a ring (cylinder r5 minus cylinder r3 about (0,10), z 0..2, turned 90 degrees so its circles' seam vertex is at (0,15,2)) and a lip ((cylinder r5 minus cylinder r4 about (e,10), z 2..20) intersected with a box y >= 10), brep tolerance 1e-5. The two r5 rims on the coplanar faces at z = 2 cross at (e/2, 15, 2), e/2 from the vertex: `SM_ERR` (1001) for 4e-5 < e <= 2e-4 (5e-5, 7e-5, 1e-4 and 1.5e-4 checked); e = 0, 1e-5, 2e-5, 3e-5, 2e-4 to 1e-2 succeed. With the seam at (5,10) (no vertex near the crossing) every offset succeeds. tangency-labs/solidcore#480 (SolidCraft regression rig, tangency-labs/solidcraft branch `kernel-smlib`; the rig's two-lip case returns 1066 mm^3 instead of 769.47).
- **How found:** issue #480; reproduced with an `SM_API` program against the reference build, then traced in a private instrumented copy (error returns of `SER`, `SmVertex::SetPoint`, vertex definition creation, combine and imprint).
- **Confirmed:** yes, on the C++ build (x86-64 Linux, g++, `c9c5979`). Skipping the split when the definition is on a vertex in either B-rep (treating the proto edge as degenerate, as `Resolve` does for a curve shorter than `2 * m_dTol`) makes every failing offset a manifold solid of 354.99989 mm^3 (analytic 354.99997), with the same topology as e = 3e-5; results at every other offset are unchanged, and `SM_API_test` and `prog_test` pass with and without it (the new branch is taken 0 times in either suite).
- **Severity:** Boolean failure (`SM_ERR`), or a wrong body where the caller does not check.
- **Rust port:** `ProtoTopologyManager::resolve_proto_edge` (`crates/solidcore/src/topology/proto_topology/p2.rs`) treats such a proto edge as degenerate when `split_self_loops_on_vertices` is off, which the Boolean's topology intersector sets (documented deviation); the default keeps upstream's split for the unit's difftest. The full union does not run in Rust yet (`plan/findings.md`, issue #480).

#### WR-254. `ManifoldBoolean` keeps a face lying in the slicing sheet in both parts of `SM_BO_SLICE`
- **Location:** `source/SMLib/src/SmMerge.cpp`, `SmMerge::ManifoldBoolean`, the coincident-face branches of steps 7.3e (`:3293`-`3306`) and 7.B (`:3527`-`3539`), where `case SM_BO_SLICE:` falls through to the `SM_BO_DIFFERENCE` rule.
- **What happens:** The difference rule deletes a coincident pair only when both faces are on the same side and not non-solid, judged from the void flags of the regions on each side. A slice's other body is always an oriented sheet (checked at `:2531`), which is void on both sides, so a pair is always either non-solid or on opposite sides and is always kept. A body face lying in the slicing sheet, or the sheet's piece of it, is then kept in both parts. In one part it has void on both sides and sits on non-manifold and lamina edges, so that part fails the check.
- **Trigger:** A box joined to a slab, sliced with `SM_BO_SLICE` by an oriented sheet on the plane x = -3.5 of the slab's face. Expected: the box part has 6 faces (volume 3434) and the slab part 8 faces (volume 17116.2). Actual: the box part has 7 faces, one at x = -3.5 outside the box. tangency-labs/solidcore#528 (SolidCraft regression rig, tangency-labs/solidcraft branch `kernel-smlib`).
- **How found:** issue #528.
- **Confirmed:** yes, by the reporter on the C++ build (SolidCraft rig); not rerun here.
- **Severity:** wrong result (an invalid part).
- **Rust port:** `coincident_keep` (`crates/solidcore/src/boolean/merge/p4.rs`) gives `Slice` its own rule in both coincident branches when the primary face bounds material (solid on exactly one side; documented deviation): the pair is kept when that material lies on the side the sheet's `SM_OT_SAME` faceuse bounds, and deleted otherwise. That is the side rule step 7.4d already applies to faces in a radial sector. The coincident faceuses point the same way (`SmEdge::FindRadialSector`), so the test is "the primary faceuse's region is solid" equals "the sheet faceuse is `SM_OT_SAME`". Primary faces with no material side (a sheet primary) or material on both sides keep upstream's difference rule, as do all other operations. The fork's fix (ee2a67b, branch `port-split-coplanar`) is described the same way for a solid. No difftest case reaches these branches yet: the `boolean.merge_p4` slice cases with coincident sheets (`op10-overlap-sheets`, `op10-equal-sheets`) defer earlier, and their primary is a sheet, which keeps upstream's rule.

#### WR-252. `TrimWithDomain` pushes a trim boundary one rounding error inside a knot 10 * SM_EFF_ZERO_SQRT away, leaving a sliver span
- **Location:** `source/SMLib/src/SmBSplineSurfaceNLib.cpp`, `SmBSplineSurface::TrimWithDomain`, the U snap loop (`if (ul >= dKnot || i == 0) ... else { ul = dKnot - 10.0 * dTolMin; ...}` and the mirrored `ur` branch), and the same in the V loop.
- **What happens:** a lower trim boundary within `dTolMin = SM_EFF_ZERO_SQRT * (1 + |ul|)` of a knot is snapped onto the knot when it is at or above it, but moved to `knot - 10 * dTolMin` when it is below it, by any amount down to one ULP (upper boundaries likewise). The new surface then starts with a span of `10 * dTolMin`, 2.1e-4 at u = 20. Two faces whose domains differ by one ULP get different knot structures.
- **Trigger:** `SmApiCreateTaperExtrude` of a 120x80 rectangle, height 40, draft 5 degrees, both caps; `SmFilletExecutive::DoFillet` r4 (circular, constant radius) on the four sloping corner edges. `SmFace::ShrinkGeometry` of the (-x,+y) round passes u_min = 20.15250701054303, one ULP below the knot 20.152507010543033 on which the identical (+x,-y) round's domain lies. The result is u knots 20.15229549 / 20.15250701. tangency-labs/solidcore#472 (SolidCraft rig shell/3576, tangency-labs/solidcraft branch `kernel-smlib`). The sliver makes the later `N_SrfOffset` fit diverge (#471): the inward shell by -2 fails with 1001.
- **How found:** issue #472; reproduced with an `SM_API` program against the reference build, traced with knot dumps at `SetFilletSurface` and in `SmFace::ShrinkGeometry`.
- **Confirmed:** yes, on the C++ build (x86-64 Linux, g++, `c9c5979`). Snapping a boundary at most `SM_EFF_ZERO_PARAM` inside a knot onto it (and passing that knot as the requested bound to `sm_CheckTrimBoundaries`) gives all four rounds the same knots with no sliver, and the -2 shell then succeeds (a manifold solid with 19 faces).
- **Severity:** a needlessly ill-conditioned surface (a sliver knot span) that makes later operations fail.
- **Rust port:** `BSplineSurface::trim_with_domain_with` with `TrimOptions::snap_roundoff_to_knots` (`crates/solidcore/src/nurbs/b_spline_surface_n_lib/p2.rs`), through `Surface::trim_with_domain_with` and `topology::face::p1::shrink_geometry_with`, which the fillet executive's face shrink uses. The default keeps upstream's behaviour for the unit's difftest (`plan/findings.md`, issue #472).
#### UB-212. `SmFilletCorner::SqueezeEdge` frees a closed corner edge at its first vertex use and reads it at its second
- **Location:** `source/SMLib/src/SmFilletCorner.cpp:2264-2304` (`SmFilletCorner::SqueezeEdge`): the loop over the deleted vertex's vertex uses (`:2273-2293`) and the deletion of the vertex (`:2295-2298`).
- **What happens:** The vertex uses of the vertex to delete are copied into `sVUs` first. For a vertex use whose edge is not a rail, the edge is deleted (`delete pFE`, `:2291`), which frees the edge's edge uses and their vertex uses. A non-rail edge with both ends at that vertex (a closed corner edge) has two entries in `sVUs`: the second is dereferenced (`pVU->GetEdgeuse()`, `:2277`) after the first iteration freed it. Separately, when the edge to squeeze is itself closed, `GetVertex()` and `GetOtherVertex()` (`:2268-2269`) name the same vertex, so `pSurvivingFV == pDeleteFV`: the rail vertex uses are "moved" to the vertex that `delete pDeleteFV` (`:2298`) then frees, and dangle.
- **Trigger:** A corner whose vertex to delete (`SM_FV_TO_BE_DELETED`) has a closed non-rail corner edge, or a closed edge passed as the edge to squeeze. Reached from `CalcCornerEdgeGeom` (`:1648`) for an edge whose `CalcCornerEdgeGeom` set `SM_FE_TO_BE_SQUEEZED`.
- **How found:** source reading while porting `SqueezeEdge` (`blend.fillet_corner_p1`, PR #266); a generated difftest case with a closed squeezed edge crashed the C++ reference before the harness refused it.
- **Confirmed:** no (the closed non-rail edge: source only; the harness refuses both paths; the closed squeezed edge crashed once in a generated run, not reproduced in isolation).
- **Severity:** crash/UB (use after free).
- **Rust port:** `FilletCorner::squeeze_edge` follows the same loop on ids (no frees), so it returns `NotFound` or deletes consistently; the difftest shim and runner refuse exactly the inputs whose loop reaches a freed vertex use or the dangling survivor, and compare the defined early-error paths (`SM_ERR` for an edge or vertex not in the corner's lists).

#### WR-254. `SmFilletCorner::MakeAnalyticCornerSurface` reports its `SER` / `NER` failures as `TRUE`
- **Location:** `source/SMLib/src/SmFilletCorner.cpp:3379`-`3680` (`SmFilletCorner::MakeAnalyticCornerSurface`, returns `SmBoolean`): `SER(SM_ERR)` for a fillet edge whose curve is not an `SmBSplineCurve` (`:3462`), and in the chamfer branch `SER(SM_ERR)` (`:3537`), `NER(pOrigFace)`, `NER(pFilletSurface)` and `SER(sm_SrfSrfIntersection(..))` (`:3586`). Its caller `SmFilletCorner::CreateCornerPatch` tests `if (!bSuccess)`.
- **What happens:** As WR-52, the non-zero status reads as `TRUE` ("an analytic surface was made") although no surface was added to `m_vSurfaces`. `CreateCornerPatch` then skips the Coons patch and calls `MakeFaceBrep` with fewer surfaces (none: it returns `SM_SUCCESS` without a face). In the chamfer branch the edge's curve has already been deleted (`SetCurve(NULL, TRUE)`) when `NER(pOrigFace)` / `NER(pFilletSurface)` or a failed intersection returns, so the corner keeps a fillet edge without curve.
- **Trigger:** a corner with three or four fillet edges, one of them without curve (or with a curve that is not a B-spline); or a chamfer corner whose non-linear rail-rail interpolating edge has no original face, a face without surface, or no intersection with the corner plane.
- **How found:** source reading while porting `MakeAnalyticCornerSurface` (`blend.fillet_corner_p2`, PR #641); the difftest's `analytic` cases with a curveless edge return `1` on both sides with no surface.
- **Confirmed:** yes, on the C++ build (`c9c5979`, the `blend.fillet_corner_p2` difftest: `MakeAnalyticCornerSurface` returns non-zero with `m_vSurfaces` empty).
- **Severity:** a corner without a corner face where a Coons patch would have filled it (a fillet result with a hole, or a failure later).
- **Rust port:** `FilletCorner::make_analytic_corner_surface` (`crates/solidcore/src/blend/fillet_corner/p2.rs`) reproduces it: those exits are `Ok(true)` (documented deviation note in the module header).
#### WR-255. `sm_FixUVTrimCurves` trims a UV trim curve at the vertex's drop point and stretches it back over the edge interval, sliding it off its edge
- **Location:** `source/SMLib/src/SmFace.cpp:7609`-`7687` (`sm_FixUVTrimCurves`, between "drop sUVVertex to each UVTrimCurve" and the end-weight reads): for each of the two trims meeting at the corner, `DropPoint` of the vertex's UV point, then `Trim(sTrimIvl)` at the drop parameter and `EditParameterization(sCurrIvl)` / `(sPrevIvl)` back over the old interval, before `EditEndPoint` moves the end onto the vertex's UV point. Called only from `SmFace::CreateUVTrimCurves` (`:8342`), which builds a face's trim cache.
- **What happens:** the trim is cut short by the overshoot past the vertex and then stretched linearly over the whole edge interval, so every interior parameter moves by the trimmed length relative to the edge's 3D curve. The trim still runs along the same UV path, but at a given parameter it no longer lies over the edge's point there. The end-point move alone is local and as large as the UV gap.
- **Trigger:** any trim cache build on a face with a corner UV gap that has a component along the trim, e.g. a point classification (`SmBrep::AssertValid` -> `FindInfiniteRegion` -> `RayFire` -> `SmTLSPointFace::SolveIt` -> `SmSurface::GlobalLineIntersect` -> `SmTrimSrfCache::BuildTree`). On SolidCraft ladder case 06-threads/004 (a cylinder R8 x 10 joined with R5 x 25, an M10 thread cut by a boolean), a 1.15e-6 corner gap left a thread-flank trim 5.05e-5 off its edge at the same parameter (a constant 1.0000057 knot ratio, end knot pinned), against an edge tolerance of 1e-5. Right after the boolean, all 144 trims were within tolerance. tangency-labs/solidcore#570.
- **How found:** issue #570 (SolidCraft regression rig on tangency-labs/solidcraft branch `kernel-smlib`), traced by the reporter; fixed in the fork as tangency-labs/smlib@5af7070 (`capi-solidcraft`), which drops the drop/`Trim`/`EditParameterization` step and keeps the end-point moves (06/004, 06/006 and 05/010 then pass; the 372-case rig is unchanged).
- **Confirmed:** yes, by the reporter on the C++ build (the fork's rig). Here: the source at `c9c5979` has the same code, and the port's regression test (below) shows the slide (interior knots 3.3333 -> 3.3324 for a 1e-3 overshoot on a 10 mm box edge) with the upstream code path.
- **Severity:** wrong geometry (trims outside their edge's tolerance after any classification), so a downstream check or mesher sees gaps that SMLib's own check does not report.
- **Rust port:** `topology::face::p2::fix_uv_trim_curves` follows the fork's fix (documented deviation): only the end control points move. Regression test `fix_uv_trim_curves_keeps_interior_parameters`. No difftest case reaches the removed branch: the `topology.face_p2` `FixUv` cases run on the fixture B-reps' own trims, which meet their vertices, so the drop point is the trim's end and upstream trims nothing; every unit's results are unchanged (`plan/findings.md`, issue #570).

## Note: duplicate IDs (2026-10-23)

Parallel PRs gave six IDs to more than one entry. They are not renumbered, because code
comments, shims, unit notes, findings and PRs already cite them; when citing one of these
IDs, add the function name so the entry is unambiguous. Each entry, by its section and
title (other references in the repo in brackets):

- **UB-100:** (a) Surfaces and volumes, `SmDerivSurfDefinition::GetSampleValues` missing
  tangent reference [`solidcore-ffi` `surface_surface_p6.rs`, `surface.surface_p6`
  notes, findings]; (b) Intersection, `SmPolyBrep::CalculateBoundingBox` null point array
  for a pseudo box.
- **UB-185:** (a) Topology, `SmFilletVertex::CalcMateGeom` unbounded recursion through a
  cycle of mates [`blend.fillet_geom_p3` notes, `MAX_MATE_DEPTH`]; (b) Numerical
  integration (near the end of the file), fillet executive `m_bIsInitialized`
  indeterminate before parameter setup [`blend_fillet_executive_p2`/`p3` shims and FFI,
  findings].
- **UB-204:** (a) NLib: matrices and polynomials, `SmMatrix(rows, cols)` uninitialised
  inline storage used by `SmLocalSolveNd` [difftest `curve_curve_p9.rs`, findings
  (#94)]; (b) Surfaces and volumes, `SmFilletSolver::GetSurface` unbounded recursion
  through the geometry set [findings, `blend.fillet_solver_p1` pseudo B-rep entry].
- **UB-210:** (a) Topology, `SmBrep::DeleteEdge` reads `pSurvF` outside the merge branch
  [`topology/brep/p4.rs`, `topology.brep_p4` notes]; (b) Topology,
  `SmFilletEdge::CalcFilletIntFillet` reads the context of a missing fillet corner
  [`blend_fillet_geom_p3` shim, runner and fixtures, `blend.fillet_geom_p3` notes].
- **UB-238** (four entries): (a) NLib: knots and basis functions,
  `SmBSplineCurve::CreateCanonical` heap corruption for a NaN knot [`solidcore-ffi`
  `primitive_api_curves.rs`, fuzz corpus]; (b) Spatial structures and small geometry,
  vertex/vertex and edge/edge topology solvers dereference `NULL` in a release
  `SM_ASSERT` [`solidcore-ffi` `intersect_topology_solver_p1.rs`]; (c) Curves,
  `SetIntersectionCurveThroughPoint` builds UV trim curves from the address of a pointer
  slot; (d) Topology, `SmFilletGeom::MakeRailEdgeuse` leaves a deleted vertex use linked
  in the fillet vertex [`blend_fillet_geom_p5` shim and FFI, `blend.fillet_geom_p5`
  notes, findings].
- **WR-190:** (a) Topology, attributes, solvers, `SmFilletSolver::RecordFilletError`
  reports rail 0 for rail 1 [`blend/fillet_solver/p2.rs`]; (b) Curves,
  `SmFilletEdge::CalcBlendingRail` builds its Hermite fallback as a degree-2 curve
  [`blend/fillet_geom/p6.rs`].

New entries take the next free number after the highest in their series, as the
Numbering paragraph at the top says.

#### WR-257. `N_SrfOffset` rejects normal extrusions by their length and destabilizes fits on sliver spans (#471)

- **Location:** `source/SMLib/src/NL_SrfOffset.cpp:854-892, 956-1108`.
- **Trigger:** a centered 120x80 rectangle, taper-extruded 40 mm with 5-degree draft,
  four sloping edges filleted r4, then shelled inward by 2 mm with the top removed.
  `fixtures/nlib.srf_offset/issue471-repro.cpp` records the actual offset inputs via
  a linker wrapper around the unmodified `N_SrfOffset`; `issue471.json` stores the
  four round surfaces.
- **What happens:** the last round has u knots 20.152295485472926 and
  20.152507010543033. `N_SrfOffset` succeeds but its net reaches approximately
  4125.906 mm from the origin; the full shell returns 1001 on this build. The
  extrusion test uses the length-bearing `AA`: `|AA x NN| = 2.1064e-6`, while
  normalizing AA gives 5.4909e-8, below `NL_MTOL = 1e-7`. The fallback assigns
  shared u/v patch counts, and its grid cap raises even previously skipped sliver
  counts to the input degree, clustering samples around nearly coincident knots.
- **Confirmed:** yes, pinned `c9c5979`, x86-64 Linux, unmodified C++ reference.
  The Rust port reproduces the defective control net. See the #471 findings entry
  for the opt-in correction and its validation limits.

### Surface

#### UB (next free). `SmSurface::...` (surface_p8) segfaults on extreme ~1e308 B-spline weights [PSIRT]
- **Location:** `source/SMLib` surface evaluation path reached via `surface.surface_p8` difftest (extreme coordinate ~1e308 with tiny control points).
- **What happens:** The C++ reference is killed by signal 11 (segfault) on a crafted B-spline surface whose net contains ~1e308 weights and ~1e-6 control points. Rust side does not crash.
- **How found:** generated difftest case (fuzz01012, saved under `fuzz-out/reverify-1024/surface.surface_p8/`).
- **Confirmed:** yes (crash in the C++ reference run).

### Surface

#### HANG-50. `SmSurfacePointSolve` / `SmSurfaceCache` subdivision does not terminate on extreme-weight B-spline surfaces [PSIRT]
- **Location:** `source/SMLib` surface point-solve / surface-cache subdivision path (reached via `surface.surface_point_solve_p3`).
- **What happens:** The C++ reference does not return within the 20 s per-case budget on a crafted B-spline surface whose net contains ~1e308 weights (the non-rational sentinel `-DBL_MAX`) and ~1e-6 control points, with `wrap = true` and a local solver. Related to issue #506 (SurfaceCache subdivision perf, 21 s Rust vs 11 s C++).
- **How found:** generated difftest case (fuzz00316, saved under `fuzz-out/reverify-1024/surface.surface_point_solve_p3/`).
- **Confirmed:** yes (C++ timeout at the 20 s budget).
#### UB-new. `SmBendVolume::WriteToDB` writes values with no separator before the label [PSIRT]
- **Location:** `SmBendVolume.cpp:3255-3256` (`m_dBendNeutralDist << "BendVolume ..."`); reader at `SmBendVolume.cpp:3328` (`rFileIn >> pBendNeutralDist[0]`).
- **What happens:** `WriteToDB` emits `0.758...BendVolume ...` with no separator, so libc++'s greedy `num_get` consumes the leading `B` of `BendVolume` as a hex-float/exponent tail and fails the extraction, while libstdc++ stops at `B`. New upstream defect (missing separator; also a libstdc++-only round trip).
- **Trigger:** A bend volume written by `WriteToDB` and re-read on a libc++ build (`surface.srf_in_volume` read_db cases).
- **How found:** macOS libc++ repro, GitHub issue #450.
- **Confirmed:** yes, 2026-10-24 on macOS arm64 (Apple clang 21, libc++). `surface.srf_in_volume` gen1970: `values.out.len: cpp=70 rust=102`.
- **Severity:** wrong result / UB.
- **Rust port:** unchanged; unit is only authoritative on a libstdc++ build.
- **Platform dependence:** libstdc++ only.


- **Confirmed (2026-10-24, issue #450):** UB-5 — macOS libc++ stack-protector abort in `topology.database_io` gen0185 (and gen0576, gen0883, gen1324, gen1363, gen1519, gen1590, gen1652, gen1783).
- **Confirmed (2026-10-24, issue #450):** UB-6 — hang/segv reached from a subnormal under libc++ in `topology.brep_data` gen0018 (hang) and gen0110 (segv).

#### UB-175 (addendum). Tiny Booleans reach the teardown through the public API
- **Trigger:** two boxes smaller than the proto vertex capture distance
  (`max(0.01, 2000 * tol)`, `SmTopologyIntersector.cpp:1159`), e.g. `boolean.api_brep`
  `gen0059` (`with_options`, operation 3, boxes about 0.005) and `gen0066` (`merge_breps`,
  operation 2, boxes about 0.008 and 0.004): the intersection points collapse into shared
  proto vertices, so proto edges start and end on one of them, and the manager's
  destructor reads the freed edges.
- **How found:** once `SmEdgeDefinition::Imprint` ran `SmBrep::MakeEdgeInFace` in the
  port, these cases reached the port's UB-175 check; the C++ goes on (results of 8 and 7
  records).
- **Confirmed:** by the port's teardown replay only; the C++ build was not instrumented.
