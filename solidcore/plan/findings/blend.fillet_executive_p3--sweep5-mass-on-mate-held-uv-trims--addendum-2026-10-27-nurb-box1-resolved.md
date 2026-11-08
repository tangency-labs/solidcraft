# Resolved for nurb_box1; letter_g re-diagnosed: sweep5 mass on mate-held UV trims (2026-10-27)

Updates: plan/findings/blend.fillet_executive_p3--sweep5-mass-on-mate-held-uv-trims.md

The mate-held UV trims were not the cause. The two cases have different causes.

## `sweep5-nurb_box1-all-r0.5`: fixed

The face that differed is a triangular B-spline corner patch. `CreateOrientedTrimBoundary` at
mass time gave C++ 5 curves and Rust 4. C++ fills a 1.354e-5 UV gap at a non-pole corner
with a line. In Rust that gap had been closed earlier, so one edge's UV trim ended at (0,0)
instead of (0,1.354e-5).

The gap was closed during the fillet executive's `BooleanPostProcess`. Rust's
`SmEdge::IsTopological` (`topology::edge::is_topological`) passed the
analytic-or-planar test for the corner patch's edges, which C++ rejects. It then went on to
`sm_CoverOtherCoincidentFace` with refined domains, which runs `CreateUVTrimCurves` on the
patch, and its corner-gap repair (`sm_FixUVTrimCurves`) moved the trim end.

The root cause: `is_topological` took `face.surface.clone()`. `Surface::clone` (the
upstream copy constructor) drops the face owner. So `IsPlanar`'s pseudo box was not
expanded by the owning face's tolerance (`face_zone_tol_3d` returned `None`). The deviation
came out 0.101 against C++ 0.124, under the 0.115 approximation tolerance, and the patch
counted as planar. Upstream reads `pFace->GetSurface()` directly. The fix uses
`clone_for_evaluation()`, which keeps the owner. Per-face `IsTopological` return reasons, the
cover-call sequence and the corner-gap trace then match C++, and the case passes. Its
known-mismatch entry is removed.

## `sweep5-letter_g-e10-r1.0` and letter_g e17: still open (different cause)

- e10: on face 12, all four oriented trims (own and mate-held) agree except for a constant
  u shift. For example, the domain min is 10.6257394185912020 in C++ and
  10.6257394185912588 in Rust (5.7e-14, about 8 ulp). The adaptive area integration
  (`RungeKuttaODEIntegrate` at accuracy 1e-5, nested iso-curve integrals) grows this from
  5e-15 to 2e-9 relative over the first 340 samples. Trim 2's area then ends at 57.02166 in
  C++ against 57.02000 in Rust, with identical step counts.
- e17 (r=1.0): three trims with identical intervals integrate differently: rel 7e-4, 3.7e-4
  and 2.5e-4, with different step counts. The UV curve itself evaluates differently at
  1-2 ulp (for example x 3.38193023791009217 against 3.38193023791008995 at the same t).
  After the fix, e17 area is C++ 859.2081631689075 and Rust 859.1900062212871.

Both cases come from ulp-level differences in fillet geometry upstream of the mass
computation, which the integrator amplifies. They are not caused by the mass code or by
trim ownership. Next step: find where the fillet face's u parameterization (e10) and the
e17 UV trim's control data first differ, in the fillet face construction or trimming
stages.
