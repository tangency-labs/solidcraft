# offset.offset_geometry_creation: face-bound ExtendTrimEdges cache (A) — 24 gains but two iso-lift mismatches

The face-cache/solve follow-up measured `offset.offset_geometry_creation` on 898 cases
(Linux x86_64, C++ reference from the pinned upstream) on base `89c94bf` (before #463):

- baseline: 795 pass, 103 deferred, 0 fail.
- point-test-off solves + domain-cache threading + `GlobalCurveIntersect` cache prep +
  difftest face-owner fallback (the shipped subset): 797 pass, 101 deferred, 0 fail.
- plus the face-bound `ExtendTrimEdges` drop cache (A): 819 pass, 77 deferred, **2 fail**
  (`gen0248`, `gen0508`).
- A + an `iso_line_outside_domain` guard (fall back to the plain cache for a degree-1 iso
  line entirely outside the extended domain): 818 pass, 79 deferred, **1 fail** (`gen0508`).

A's 24 deferred->pass cases: gen0016, gen0027, gen0029, gen0105, gen0128, gen0173,
gen0197, gen0231, gen0236, gen0251, gen0307, gen0341, gen0350, gen0394, gen0434,
gen0435, gen0449, gen0494, gen0625, gen0641, gen0749, cone_edgeuse_offset_extend,
torus_edgeuse_offset_extend, cone_mate_extend_local. The guard lost three of them
(`gen0027`, `gen0394`, `gen0449`) and made two cases A had deferred pass
(`cylinder_seam_vertex_and_edge_caps`, `gen0218`), so it did not meet the acceptance
condition and did not stop `gen0508`. A is therefore not shipped. On merged main
`422f71321` the shipped subset alone is 820 pass / 78 deferred / 0 fail; main is
818 / 80 / 0, and the only transitions are the two genuine gains
(`cylinder_seam_vertex_and_edge_caps`, `gen0218`), with no pass->defer, pass->fail or
relocation.

**gen0248.** Rust's `uv_ext` is degree 1 with two control points at
u=30.614674589207223, v=-153.07..168.38, on the extended domain 0..10; the port's
`extract_iso_curve` therefore takes the iso branch, clamps u to 10 and returns an
`SmLine` at u=0 (the original radial line). C++ keeps the unclamped u=30.61467458920722
and returns an `SmBSplineCurve`. Both sides select the same edge-use/radial lines, the
faces are `SmPlane` (type 15011), and the cache bind is not the differentiator (a plain
bound cache fails the same way). Skipping the iso branch entirely changes the state
vector length 168 -> 316 and is not a fix.

**gen0508.** The extended iso line is at u=28.5607 **inside** domain 0..29.0102, so the
guard does not trigger. The lifted curve is B-spline 14033; the first state mismatch is
the other UV curve kind (C++ 14033 vs Rust 14010) with identical control points
(x=0.801871609846126, y=0..1.519711141793538). The representation differs after
`CreateExtendedCurve` / `RemoveExtraKnots` or in the lift/drop, not in the cache.

These A measurements are before #463 (`intersect.surface_drop_lib_p3`); re-test A after
#463 before pursuing it. A follow-up should compare `SmBSplineCurve::CreateExtendedCurve`
/ `RemoveExtraKnots` for a degree-1 line and the `LiftCurve` iso branch, then remove the
guard.
