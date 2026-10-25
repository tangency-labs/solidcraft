# blend.fillet_standard_solver_p1 (`SmFilletStandardSolver.cpp` part 1, 2026-10-16; partial, no disagreements)

- **Corpus.** 3839 cases (25 fixture, 709 first-review and 2625 second-review fixtures,
  480 generated): no failure at max_rel_err 0, 42 deferred, 11 rejected. After
  surface.offset_surface landed (merge of main, 2026-10-18) the Jacobians and
  `SmConstantDistanceFS::LoadInitialValues` with `rbDoSurf2Calcs` run on the ported
  `SmOffsetSurface` and are compared; the variable-radius `SetupOffsetValues` stays
  deferred (blend.fillet_executive_p1); a few inputs the shim's dry run rejects are
  excluded (below).
- **Fixed in nlib.srf_adv_p5 (Rust only).** `N_AdjustDerivSrf` adjusts the derivative
  strips of `N_CreateSkinSrfBoundaryContinuity` in place. For the swap-then-extend cases
  (`cst`/`usg` Left/Top, Bottom/Right, Right/Bottom, Top/Left) it swaps u and v before
  `N_BezSrfExtend4D`, whose input check (`p != n`: the strip is not Bezier in the swapped
  direction) can fail; the strip then stays swapped, and `SmBSplineSurface::
  CreateSkinnedSurface`'s retry without the knot vector adjusts the swapped strip (swap
  back, extend the Bezier direction: success). The port returned the error without the
  swap, so its retry failed again and skinned without derivative surfaces. Hit by every
  `G1` circular-section skin with approximated (multi-span cubic) arcs: the C++ result
  equalled `CreateSkinnedSurface` run in C++ on the Rust-built inputs, the Rust skin
  differed. `srf_adv::p5` now leaves the strip swapped on that failure;
  nlib.srf_adv_p5, nurbs.b_spline_surface_n_lib_p2 and blend.fillet_custom_solver
  difftests unchanged (3073 / 654 / 448 cases, max_rel_err 0).
- **Shim dry runs (rejected cases).** Two inputs read out of bounds upstream only after
  work the FFI validator cannot predict; the shim repeats the steps leading there on
  copies (same upstream calls) and returns `-1`, which the runner records as rejected:
  ruled / revolved circular sections whose first-pass unique knot counts differ (UB-202)
  and blend-curve skins through a point section or with `G1` derivative surfaces
  (UB-203), plus a blend knot insertion that adds no knot (HANG-34). The `G1` blend skin
  is always undefined upstream (the first `cmp = NL_YES` attempt pairs a 2-point
  derivative curve with degree 3-7 sections); the port's bounds-checked skinning fails
  that attempt and retries as upstream does after a failure.
- **Defaults kept.** `SmBSplineCurve::Evaluate` from the generators (static type
  `SmBSplineCurve`) uses `bNonZeroTangents = FALSE`; `SmCurveBasedFS::EvaluateBaseCurve`
  calls `SmCurve::Evaluate` (`TRUE`). `IsArc` of a B-spline curve is the `SmCurve` base
  (`IsDegenerate(tol)` first). `ContainsValue` is inclusive with tolerance 0.
  `CreateSurfaceFromCurves` keeps rail 1's knots from before the three-curve
  synchronisation; the circular `G1` derivative strips use the rail 2 tangent, the blend
  ones the centre line tangent; blend skins pass `bSyncronized = TRUE`, circular ones
  `FALSE`. `GetSingularities(NULL, NULL, &lSing)` reads the approximate singular sides.
- **Edge-use constructor.** Since topology.edgeuse_p3 ported `EvaluateBinormal`, the
  `SmConstantRadiusFS` edge-use constructor runs on the B-reps of topology.edgeuse_p1
  (second shim `blend_fillet_standard_solver_p1_eu.cpp`, the C++ B-rep graph imported
  on the Rust side). Its `SE(EvaluateBinormal(..))` statuses are only signalled: after
  a failure the binormal keeps `SM_UNDEF_DOUBLE` and the face use normal is zero (or
  `SM_UNDEF_DOUBLE` before the loop check), so every failure ends at the
  tangent-surface `SM_ERR`; the port does the same and propagates only `Unported`.
  The only upstream solid with edges between exactly imported faces that already
  have UV trim curves is `box_with_large_fillet.smb` (planes and B-splines); its
  constructions are compared (radii, orientations, offsets, faces, offset points,
  both edge uses' state). The other solids defer on their surface kinds (cone,
  sphere, extrusion, revolution) or on `SmSurface::DropCurve`
  (intersect.surface_drop_lib_p3) when a UV trim curve must be made; the laminas
  compare the tangent-surface exit.
- **Offset surface copies.** `rpSurface2->LocalPointSolve` / `GlobalPointSolve` in
  `SmConstantDistanceFS::LoadInitialValues` run on a copy of the solver's offset (the
  generic point solvers take the surface shared), so an extension made there is not
  kept, as blend.fillet_custom_solver. The out-of-bounds `SmTemporaryChangeValue` is
  applied in place and restored. `crSurface1` is an `InitialSurface` (the solver's own
  rail, or another offset) because upstream's callers always pass
  `*GetSurface(lRailIndex)` (see the second review below). No case disagrees.
- **Not compared.** `SmVariableRadiusFS::SetExtensionFlags` starts with
  `SmFilletSolver::SetExtensionFlags`, which dereferences the fillet executive
  (blend.fillet_solver_p1): unit tested only. The Jacobians and the edge-use branch
  of `LoadInitialValues` run in C++ with cleared edge uses (as
  blend.fillet_custom_solver). That edge-use branch (UV trim curve guess, `IsClosed`
  snapping, `LocalPointSolve`, the 30-150 degree and singularity checks) was read
  line by line against upstream in the PR #168 review and looks faithful; a
  follow-up with real edge uses would compare it.
- **PR #168 review fixes (2026-10-19).**
  - *Runner allocations from case sizes.* The Rust runner built the `LoadJacobian`
    matrix and called `EvaluateBaseCurve` with the case's sizes unchecked; the fuzz
    child runs Rust whenever the C++ side returns a harness error, so a `2^31 x 12`
    Jacobian or `2^31` derivatives aborted it ("memory allocation of ... failed").
    Both are now harness errors above `ffi::MAX_ARRAY` / `ffi::MAX_DERIV`, as the
    FFI validator bounds them.
  - *Reversed plane domains.* The runner built plane domains (and the `SsExt`
    domain) with `Extent2d::from_points`, which swaps reversed corners, while the
    shim's `SmExtent2d(u0, v0, u1, v1)` stores them as given: `fuzz00403`/`00484`
    disagreed (`SsExt` out[0] cpp 0, rust -0.313). Now `Extent2d::from_bounds`.
  - *FFI guard gap: reversed plane under an offset surface.* `SmPlane::MakeNurb`
    takes the NURB knots from the plane's domain, so a reversed domain gives NLib a
    decreasing knot vector. With a solver's offset of such a plane, `LoadJacobian`
    extends it through `sm_ExtendSurface`; for a domain reversed by `>= 1e10` (`1e9`
    and less run) `N_SrfEvalPtDerivsUnbounded` fails and the retry segfaults in
    `N_SrfGetParameterBounds` (UB-187; found by `--fuzz 3000` once a `cr_jac` fixture gave
    it a base). The validator now rejects reversed plane domains for the solvers'
    base surfaces (`Cr`, `CrJac`, `Cd*`, `Vr*`); `SsExt` (which only reads the
    domain) keeps them. The generators never make reversed planes.
  - *UB-186 (not detected by the shim).* A circular `G1` skin with approximated arcs
    whose second pass inserts knots within one ulp of existing ones (`5/11` against
    `5 * (1/11)`) leaves one section with 39 control points against 41 after
    `N_CrvsMakeCompatibleKnotTol` + `N_CrvElevateDegree` on `CreateSkinnedSurface`'s
    retry, and `N_CreateSkinSrfBoundaryContinuity` reads past it
    (plan/upstream-issues.md UB-186). Two of the reviewer's 708 adversarial cases
    reach it (`rev_twist_n6p3_t0.05_m{0,1}_c1_dr1e-06_g1_s10000.0`: out.len cpp 1765,
    rust 1839; the C++ fillet's v knots equal a C++ `CreateSkinnedSurface` call on
    the same inputs, so the C++ value is the UB one). They are not fixtures. The
    shim's circular dry run does not detect this yet (it does for UB-202/203); the
    generated corpus never reaches it.
  - *Out of scope, noted.* The nurbs.b_spline_surface_n_lib_p1/_p2 runners and shims
    build curves with `SmBSplineCurve(gw, 3, FALSE)` / `from_nurb(.., false)`, which
    runs `sm_FixupKnotVector` and snaps knots within an ulp of each other, so the
    skin and ruled-surface difftests can never feed them the near-duplicate knots
    that reach UB-186.
  - The reviewer's 708 adversarial cases (ruled, revolved and skinned circular and
    blend-curve sections with twisted rails, near-equal radii, tiny tolerances,
    scales 1e-6..1e4; `EvaluateBaseCurve` extensions; the two reversed-domain
    `SsExt` cases) and a planes `cr_jac` case are fixtures
    (`fixtures/blend.fillet_standard_solver_p1/review_adversarial.json`): all pass
    at max_rel_err 0.
- **PR #168 second review fixes (2026-10-19).**
  - *`crSurface1` identity.* `load_initial_values` took `crSurface1` as a separate
    `&mut OffsetSurface`, and the runner passed a clone of the solver's rail. Every
    upstream caller passes `*GetSurface(lRailIndex)` (SmFilletSolver.cpp:3009, 3372,
    3997, 4011), so the base extension made by `crSurface1.EvaluatePoint` stays on
    the solver and grows that rail's natural UV domain; when `crSurface1` is the
    other rail it is `rpSurface2` itself, whose `GlobalPointSolve` then runs on the
    extended domain. 72 of 960 new `cd_init` cases (`rail 0, sel1 2` / `rail 1,
    sel1 1`, guess (1.2, -0.3) outside the domain) disagreed: e.g.
    `rv2i_cdinit_bump_pl_1_[1.2, -0.3]_1_0.5` out[21] cpp 1.2000000000000002, rust
    1.0; out[22] cpp -0.2999999999999998, rust 0.0 (Rust extended the clone and
    clamped). Now `InitialSurface::{Rail(i), Other(&mut OffsetSurface)}` resolved
    against the solver; when it is `rpSurface2` the out-of-bounds change is applied
    once. The runner passes `Rail(0/1)` for `sel1` 1/2, as the shim does.
  - *Fixtures.* `review2_cd_init.json` (960 `cd_init` cases over bump, cone, cylinder
    and plane rails, guesses inside, on and outside the domain, all three `sel1`),
    `review2_jac_ext.json` (1560 Jacobian / extension cases) and
    `review2_closed_torus_base.json` (105 closed / torus-base cases; its 18 `G1`
    blend-curve skins are UB-203b and are left out). All pass at max_rel_err 0.
  - *Same gap in blend.fillet_custom_solver (not fixed here).* `EiffelTowerFs` /
    `SecantConstantRadiusFs::load_initial_values` take `surface1: &mut OffsetSurface`
    with a cloned rail in the runner, so they very likely have the same identity gap;
    a follow-up there should use the same `InitialSurface` model.
  - *Not compared.* `EvaluateBaseCurve`'s B-spline-based branch on `SmEllipse` /
    `SmCircle` bases: the reviewer's 48 ellipse/circle extension cases pass with real
    values, but the FFI validator first needs radius > 0 and a bounded increasing
    domain (`--fuzz` segfaulted at radius 0 and hung at domain min 1e300), so they are
    not in this PR. The production `SmBlendCurveCrossSectionFSG` path with
    `bNeedG1Continuity = TRUE` (constant-radius solvers) is never compared, because
    every `G1` blend-curve skin is UB-203b and the shim rejects it. UB-186 is still
    not detected by the shim; no fixture reaches it.
