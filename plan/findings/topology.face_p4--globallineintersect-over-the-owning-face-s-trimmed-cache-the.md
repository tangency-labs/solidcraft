# topology.face_p4 — GlobalLineIntersect over the owning face's trimmed cache: the 19 "singular point" mismatches (area 8b)

Follow-up to the entry above (owning-face handle, #537): the 19 failures of the wired
`SmFace::GlobalLineIntersect` (`cone.smb_0_f1_op19`, `cone.smb_1_f2_op19`,
`cone.smb_f0_tol4867..4869`, `_f1_tol4885..4887`, `_f2_tol4903..4905`,
`surf_sphere150.smb_0_f1_op17`, `_1_f2_op17`, `_f0_tol5302/5304`, `_f1_tol5320/5322`,
`_f2_tol5338/5340`) were not a singularity bug. Traced per stage (gdb on the C++
difftest binary: `LocalCurveIntersect`, `AddSortedSolution`, `CalculateBoundingBox`,
`SmPolarBox::HasPerpendicularToVector`; eprintln on the Rust side), three causes:

- **Harness: different geometry on the two sides.** The face_p4 shim's `fp4_normalise`
  replaced every analytic face surface by a plain `SmBSplineSurface` of its NURB, but the
  shared export sends cones/spheres/tori/revolutions/extrusions exactly
  (`sc_analytic_export.h`) and the Rust import rebuilds them as their own kind: C++ ran
  `SmBSplineSurface::LocalCurveIntersect`, Rust `Cone`. The shim now keeps the surfaces
  `put_exact_head` exports (as `heal_gap.cpp`'s normalise). Before: `tol4867` C++ `v=0`
  (B-spline Newton), Rust `3.94e-31`.
- **Trimmed cache without a surface name.** `face_surface_cache(.., None)` left every node's
  surface handle `None` after the build (upstream's cleanup sets every `mBA_pSurface` to
  `m_cpSurface`), so `polar_box_unstored` failed ("node has no surface") and the leaf
  solve's early exit (`!GetPolarBox().HasPerpendicularToVector(line)`) never fired: Rust
  tried 4 guesses per leaf where C++ returned after the first hit. The line path names
  the surface `EntityRef::Face(f)`.
- **Analytic polar boxes.** `SmBSplineSurface::CalculateBoundingBox` tests
  `IsKindOf(SmCone/SmSphere/SmTorus)` (virtual) to sample the polar box from 10 normals
  (`EvaluateNormal`, virtual); the port called it on the B-spline base, which is never one
  of those kinds, so cones got the control-net polar box (Unbounded at every level where
  C++ has bounded boxes, e.g. u 0..7.65). `BSplineSurface::calculate_bounding_box_of`
  takes the derived surface; both dispatchers (`Surface::calculate_bounding_box`, the
  surface cache's) pass it.

Also ported: `sm_IsPointValid` in the LSI leaf solve answers with the trimmed cache's
`PointTest` (face classification) instead of the base domain test (no case changed).
Result: `topology.face_p4` 5506 cases, 5141 pass, 0 failed, 365 deferred, max_rel_err 0.
Possibly the same latent gap: `query.face_p1`'s `make_trim_srf_cache_with_face(.., None)`
also leaves node surfaces unnamed (its point solve runs with point testing off; no
failing case).
