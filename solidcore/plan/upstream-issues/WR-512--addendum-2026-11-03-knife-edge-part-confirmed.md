Updates: WR-512

#### WR-512 addendum (2026-11-03). Confirmed with the C++ on #1522's knife-edge part; the cube case does not reach it
- **Severity:** Wrong result
- **Area:** Intersection
- **What changed:** **Confirmed:** yes. The record's model-level trigger now runs on both
  engines (x86-64 Linux):
  - **The part:** #1522's knife-edge part, built from `SmApiCreateBox`,
    `SmApiCreateCylinder`, `SmApiBooleanDifference` and `SmApiBooleanUnion` as the fork's capi
    test builds it (tangency-labs/smlib@d37ca80, `capi/tests/ops_kernel.cpp`). Its top front
    edge is rounded R2, then the arc wall's top edge R2, each with `SmApiFilletEdges` (circular,
    G2, thumbweight 0) on the edge nearest the capi test's pick point.
  - **Pinned C++ (`3fd5b95`, v1.0.1):** the front round builds (9556.0493812803); the arc round
    fails with status 1001.
  - **Pinned C++ with the fix only:** the line `rTsectPnt.CrvDeriv() =
    aSingDirections[lInsideIndex[0]]` added before the angle search in
    `SmAdvSurfaceIntersector::ComputeSingularityPoint` (`SmAdvSurfaceIntersector.cpp`, after `:835`),
    that file recompiled with the reference flags and linked ahead of the pinned
    `libSMLib.a`. The arc round builds one manifold solid of volume 9531.3100142533
    (`SmApiBrepComputeVolume` at 1e-8). So this line alone turns the failure into the round.
  - **Rust port:** the default fails with 1001, as the C++. With
    `Kernel::fixes.ssi_singular_start_inside_direction` it builds the C++ fixed body: one
    manifold solid, `check_brep` clean, volume 9531.3100142533 (the same to the 10 decimals
    printed). With every fix in `UpstreamFixes::all()` it still fails without this one; with
    it, the volume is 9531.3100487442 (3.6e-9 relative: the other fixes build the round faces
    their own way).
  - **The cube case:** #1522's 20 cube, its top back edge rounded r3, then the end face's top
    edge r1, builds to 7957.5052483679 on the pinned C++, with and without the fix, and on the
    Rust default with and without the flag (7957.5052483753 with every fix, either way). It
    does not reach this start point.
- **Regression evidence:** `crates/solidcore/tests/wr512_singular_start_inside_direction.rs`
  (`default_kernel_refuses_the_arc_round`, `the_flag_builds_the_arc_round`,
  `every_other_fix_still_refuses_the_arc_round`, `every_fix_builds_the_arc_round`).
- **Tracking:** tangency-labs/solidcore#1954 (audit slice), #1522, #1576.
