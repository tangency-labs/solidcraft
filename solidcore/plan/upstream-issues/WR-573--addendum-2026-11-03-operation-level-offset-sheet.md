Updates: WR-573

#### WR-573 addendum (2026-11-03). The offset sheet itself, built by sweep and offset, reaches it on both engines
- **Severity:** Wrong result
- **Area:** Surfaces
- **What changed:** the record's trigger now runs from the operations, not from a hand-built
  `OffsetSurface` (x86-64 Linux):
  - **The sheet:** the record's wave as an `SmBSplineCurve` (`CreateCanonical`), swept 10
    along +y with `SmApiCreateLinearSweep` (no caps), then `SmApiOffsetBrepFull` by 0.5
    (extended, self-intersections on). One face, on an `SmOffsetSurface` (type 15400) of the
    sweep's surface of extrusion (type 15030). The B-rep's approximation tolerance
    (`SmTol::GetApproxTol3d(pBrep)`) is 5e-6.
  - **The calls:** `ApproximateSurface` of that face's surface as `SmBrepData`'s
    `bWriteAsBSplines` writer calls it (the B-rep's tolerance, 3 subdivisions, no copy) and
    as the flat export calls it (5 subdivisions, B-splines copied; #1441).
  - **Pinned C++ (`3fd5b95`, v1.0.1):** both calls return `SM_SUCCESS` with no surface and
    an achieved tolerance of -1 (nothing was tested).
  - **Pinned C++ with the fix only:** `SmSurface.cpp` with tangency-labs/smlib@eae96cb's
    change, recompiled with the reference flags and linked ahead of the pinned
    `libSMLib.a`. Both calls give a 33 x 4 B-spline surface with an achieved tolerance of
    1.1245747026548022e-05; the record's `(1e-3, 3, FALSE)` call on the same face gives
    0.0016208280437303383, the value of the record's hand-built case.
  - **Rust port:** the default returns `Ok` with no surface for both calls, as the C++. With
    `Kernel::fixes.approximate_surface_bezier_starter` both give the C++ fixed result: 33 x 4
    control points, achieved tolerance 1.1245747026548022e-05 (the same to the 17 digits
    printed). On a 21 x 21 grid over the face's domain the surface stays within 7.62e-6 of
    the offset surface.
  - **Above the requested tolerance:** 1.12e-5 is 2.25 times the requested 5e-6, on both
    engines, for the reason the record gives (a pass is accepted on its last row of
    patches). From its source (not run), `SmBrepData`'s writer then signals its "not
    within tolerance" warning and writes the surface.
- **Regression evidence:** `crates/solidcore/tests/wr573_offset_wave_export.rs`
  (`without_the_flag_the_writer_succeeds_with_nothing_wr573`, which pins the defect until the
  flag becomes the default, and `the_flag_approximates_the_offset_face`).
- **Tracking:** tangency-labs/solidcore#1954 (audit slice), #1501, #1441, #1712.
