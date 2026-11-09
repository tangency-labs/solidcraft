# General: edge-tolerance reads in curve trims and STEPInversion, audited and not changed (2026-10-31)

**Units:** blend.fillet_solver (`curve_blend_sih`), blend.fillet_removal, topology.brep_p1, topology.brep_p2, topology.brep_p4, topology.face_p1 (trims), and curve.ellipse_p1 / curve.ellipse_p2 (`STEPInversion`).

**Summary:** in two places, upstream reads the tolerance of the edge that holds a curve, and the port reads the system zone tolerance (1e-5). A static audit found 6 trim sites and the `STEPInversion` callers. C++ comparisons and an isolated probe found no input where the two tolerances give different results. So nothing was changed, because a change that no test can tell apart does not land.

## 1. Trims of edge-held curves

**Upstream:**
- `SmBSplineCurve::Trim` (`SmBSplineCurveNLib.cpp:4469`) removes trimmed end spans shorter than `MapTo1d(SmTol::GetZoneTol3d(this))`.
- For a curve, `GetZoneTol3d` is the holding edge's `GetTolerance()` (`SmTol.cpp:785`, `SM_USE_OLDTOL`; `SmCurve::GetEdge`, `SmCurve.cpp:4210`). Copies and new curves have no edge, so they get the system tolerance.

**Port:** `Curve::trim` always uses the system tolerance. `Curve::trim_with_zone_tol` and `topology::edge` provide what a caller needs to pass the edge's tolerance.

**Audit:** of the 111 non-test trim calls, about 95 trim copies or new curves and match. These 6 trim a curve still held by an edge:

| # | Rust | C++ |
| --- | --- | --- |
| 1 | `blend/fillet_solver/curve_blend_sih.rs`, prev UV and rail trims | `SmFilletSolver.cpp:8499-8500` |
| 2 | `blend/fillet_removal.rs`, side-edge curve before `JoinWith` | `SmFilletRemoval.cpp:~1250` |
| 3 | `topology/brep/p1.rs` `sm_MakeVertexSplitUVTrimCurve`, attached curve | `SmBrep.cpp:21431, 21438` |
| 4 | `topology/brep/p2.rs` `trim_uv_trim_curve` | `GetOrCreateUVTrimCurve(pUV)` then `pUV->Trim` |
| 5 | `topology/brep/p4.rs` `shrink_geometry` | `SmBrep::ShrinkGeometry` |
| 6 | `topology/face/p1.rs` `shrink_geometry_with` | `SmFace::ShrinkGeometry` (`SmFace.cpp:18566-`) |

**C++ evidence (site 5, the only site a runner reaches with a set edge tolerance):**
- A trial `topology.brep_p4` op ran every edge's `SetTolerance(tol, FALSE, FALSE)`, then `ShrinkGeometry()`.
- It ran on 34 file cases (15 `pt_TestFiles` B-reps and 2 unit fixtures, at tolerances 0 and 0.05) and 3 wire cases.
  - The wire cases are a degree-2 edge curve with a knot at 0.5: 1e-9 long over [0.1, 0.55] at tolerance 0; over [0.1, 0.5000001] at tolerance 0; over [0.1, 0.501] at tolerance 0.05.
- Rust and C++ agreed on every case, both with the port unchanged and with the edge's tolerance passed. brep_p4 had 364 cases, 349 passed and 0 failed, the same before and after.

A draft Rust regression for site 3 expected knots (0.0, 0.5, 0.55) with multiplicities (3, 1, 3). With the edge's tolerance it got (0.55, 1.0) with (3, 3). Its setup did not do what was intended, so it is no evidence either way.

## 2. STEPInversion's default zone tolerance

**Upstream:** `SmEllipse::STEPInversion` (`SmEllipse.cpp:2027-2040`), with its default `dZoneTol3d = 0` (every caller passes it):

```cpp
const SmEdge *pEdge = GetEdge();
if ( dZoneTol3d != 0.0 )      { dZTol3d = dZoneTol3d; }
else if ( pEdge )             { dZTol3d = SmTol::GetZoneTol3d( (const SmObject*)pEdge ); }
if (dZTol3d == 0.0 )          { dZTol3d = SmTol::GetZoneTol3d( GetContext() ); }
dApproxTol3d = dZTol3d / 100.;
```

`dApproxTol3d` decides one thing: on a closed ellipse, a point within it of the domain-end point returns the domain start, not the end.

**Port:** `ellipse/p2.rs` `step_inversion(zone_tol)` uses `zone_tol`, else the context. It has no edge branch, because a bare `Ellipse` cannot reach its edge. Every caller passes `0.0`:
- polar conversion: `p1.rs:280` and `:417`, upstream `SmEllipse.cpp:697` and `:923`;
- `AssertValid`: `p1.rs:1367`;
- `MakeNurb`'s domain recompute: `p1.rs:1773-1774`, upstream `:3611-3612`. `make_nurb_owned_with_edge_tol` already holds the edge's tolerance there;
- `p1.rs:2108`;
- `Curve::step_inversion`: `curve/p2.rs:1593`, used by `offset_geometry_creation` `exact_piece`.

**Evidence (`MakeNurb`'s recompute, the one caller that holds the edge's tolerance):**
- An isolated probe (not merged) ran `make_nurb_owned_with_edge_tol` on 28 (radius, domain) groups: radii 1, 10, 100 and 1000, and seven domains. These were [1e-5, 360.00001], [5e-5, 360.00005], [2e-4, 360.0002], [0, 360], [-1e-5, 359.99999], [1e-5, 360] and [180.00001, 540.00001].
- Each group ran with no edge tolerance, 1e-3 and 1e-2. Every group returned the same domain at all three tolerances: 0 of 28 differ.
- The probe did not measure the end distances themselves.
- The other callers work on a bare curve. No runner sets an edge tolerance on an ellipse edge, so there is no C++ evidence for them either way.

## Decision and how to reopen

Not changed: the trial op, its fixtures, the per-site trim changes and the `MakeNurb` edit were not merged.

To reopen either part, you need a C++ comparison that shows the divergence:
- **Trims:** a runner op that sets an edge tolerance on one of sites 1-4 or 6, with a case where the trimmed end span lies between the system tolerance and the edge's.
- **STEPInversion:** a `curve.ellipse` op with a recorded owner tolerance, or a topology op on a circle edge with a set tolerance. The case needs a point between the two `/100` thresholds from a closed ellipse's end.
- Plumbing sketch for STEPInversion:
  - `Curve::owner_zone_tol_3d` already records an edge's tolerance on evaluation copies. It could reach `Ellipse::step_inversion` (as `Option<f64>`) from `Curve::step_inversion`, `convert_to_polar_parameter` and `find_parameter`.
  - A curve with an edge owner but no recorded tolerance would fail with `Unported`, as `curve_cache::curve_approx_tol_3d` does.
