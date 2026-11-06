# `SmBSplineCurve::AssertValid` tests 13/14 compare a radius of curvature with a speed

Unit: `nurbs.b_spline_curve_p3` (`crates/solidcore/src/nurbs/b_spline_curve/p3.rs`).
Issue: tangency-labs/solidcore#2785. Fix: this PR.

![A three-panel schematic. A: one cubic Bezier drawn once, with two parameter axes under it over the same span, one from 0 to 1 (T = 1) and one from 0 to 50 (T = 50), and the note that the same span carries 50x more parameter so the average speed is 50x smaller. B: the bend magnified, with the osculating circle and its radius drawn from the bend to the centre, r = 5.82e-6, the same at T = 1 and at T = 50. C: log-log, domain length T against length: the bend's radius a level black line, the speed tolerance a dashed falling line crossing it at T = 24.3, the length tolerance a level blue line above both, with vertical markers at T = 1 and T = 50 and the verdict at each.](issue-2785-radius-tolerance.svg)

*Figure. The same curve at `T = 1` and `T = 50`: the same span carries 50x more parameter, so the speed-based tolerance falls through the bend's radius at `T = 24.3`, while the radius does not move.*

Drawn in VectorCraft 0.21.1 through its MCP server and exported from it with live text
(`outlineText: false`); `issue-2785-radius-tolerance.vectorcraft` beside it is the source, so the
figure can be reopened and changed rather than redrawn.

## The defect

`BSplineCurve::assert_valid` builds one tolerance and uses it for two different kinds of
quantity:

```rust
let size_t = domain.length();
let tol = if bounded { (size_xyz / size_t) / 10000.0 } else { 1e-03 };
...
let (mut radius_sized, mut radius_tol, mut radius_sep) = (sized, tol, min_sep);
```

`size_xyz / size_t` is a length per parameter — the curve's average speed. That is the right
unit for tests 11/12, which compare `|C'|`, a speed, and for the `speed_clear` bound below
them. A radius of curvature is a **length**, so tests 13/14 (2D and 3D) compared a length with
a speed. The mapped UV-trim branch (`radius_tol = (mapped_size / size_t) / 10000.0`) had the
same shape.

The consequence is that the verdict depends on the parametrisation: stretching the knot vector
multiplies `size_t` and divides the tolerance, so the same curve with the same bend is reported
or not depending on its domain. `zero_speed_radius_stretch` (WR-668) was affected too: it
divides `radius_tol` by the radius' growth rate to get a stretch in *parameter* units, which
needs `radius_tol` to be a length.

## Measured

The curve is the cubic Bézier `(0,0) (h,0) (h,h) (1,1)` with `h = 1e-4`, on the knot vector
`[0,0,0,0,T,T,T,T]`. The curve in space is identical for every `T`; only the parametrisation
differs. Its smallest radius of curvature is `5.815568702310391e-6` throughout (the check's own
search returns `5.8155015634573e-6`, #2637).

| `T` | old tolerance `sqrt(2)/T/10000` | new tolerance `sqrt(2)/10000` | old verdict | new verdict |
|---|---|---|---|---|
| 1 | 1.414213562373095e-4 | 1.414213562373095e-4 | reported | reported |
| 10 | 1.4142135623730951e-5 | 1.414213562373095e-4 | reported | reported |
| 20 | 7.0710678118654756e-6 | 1.414213562373095e-4 | reported | reported |
| 50 | 2.828e-6 | 1.414213562373095e-4 | **not reported** | reported |
| 1e2 | 1.414e-6 | 1.414213562373095e-4 | **not reported** | reported |
| 1e3 | 1.414e-7 | 1.414213562373095e-4 | **not reported** | reported |
| 1e4 | 1.414e-8 | 1.414213562373095e-4 | **not reported** | reported |

The old tolerance crosses the true radius at `T = 24.3177`, which is the `T` between 20 and 50
where the reports stop.

## The fix

Two changes, both in the radius tests' half of the function:

1. **The tolerance is a length.** `radius_tol` is `size_xyz / 10000.0` (and `mapped_size /
   10000.0` for a mapped UV trim): the same 1/10000 fraction of the curve's own extent that the
   speed tolerance is of its average speed, and the only scale in the function that does not
   involve `size_t`, so the verdict is invariant by construction. `tol` stays a speed for tests
   11/12 and for `speed_clear`.
2. **A radius minimum on a natural boundary end is skipped**, as the first-derivative loop above
   already skips one (the next section gives the case that decided it).

No flag: both changes are on the path `Kernel::fixes.bspline_check_min_radius` (WR-382, #475)
already gates. With that flag off — which is `UpstreamFixes::default()`, and so the difftest —
`min_radius` stays `BIG_DOUBLE` and tests 13/14 never fire, so the difftest cannot see this
change. `issue_2785_radius_tolerance_is_a_length_not_a_speed` pins that too.

## The case that decided the second change

`issue_2343_curved_setback::countersink_asked_corner_is_certified_with_its_nets`. A corner
trim's minimum radius is `1.5145154e-4`; the new length tolerance is `1.6900205e-4`, so the
tolerance change alone reported it (it had passed the old `1.0078e-4` by a factor 1.5, because
that trim's `size_t` 1.6770 nearly cancels its mapped size 1.6900). Measured: `size_xyz` 1.5229,
`size_t` 1.6770, mapped size 1.6900.

Its radius profile along the curve is a spike at the curve's own **start**, not a bend:

```
1.515e-4  8.533e-1  1.150e0  1.578e0  2.214e0  3.220e0  5.025e0  9.178e0
2.739e1   3.745e1  1.290e1  8.107e0  6.147e0  5.069e0  4.392e0  3.955e0
3.643e0   3.434e0  3.304e0  3.239e0  3.233e0
```

and its minimum is at parameter **exactly 0** of `[0, 1.677]`, with the first two control points
`3e-5` apart. There is no bend in that curve; there is one at its end, and the end is the WR-668
shape: a G2 blend's tangent end has radius growing as `|t - t_end|` from 0, so a small radius
there is inherent to the construction, not a defect in it.

The radius loop therefore now skips a solution on a natural boundary end **of a curve with
topology context** — `edge_ends` or `trim_surface` present, which is how `check_brep` validates
every edge's curve and every UV trim — as the first-derivative loop above it already does ("skip
solutions on the natural boundary ends - they are not a problem"): an end is where the curve
joins a neighbour or a vertex, and that joint's own checks (G1 across the knot, the vertex's,
WR-668's waiver) judge it. The radius test judging the end in isolation double-counts a thing
another check owns. A **bare curve** — `assert_valid` with no edge and no trim surface, a direct
caller — has no such joint, so its boundary solutions are kept and its end is reported.

The trade: a curve validated through a body that genuinely ends in a kink is no longer reported
by tests 13/14 — such a joint is what the vertex and G1 checks exist for, the same trade the
derivative loop made.

Both sides are pinned by a synthetic pair,
`issue_2785_a_bare_curve_reports_a_boundary_radius_minimum`: a cubic Bezier whose first control
span is `1e-7` of its size and whose remaining spans run flat, so its radius of curvature is
`1.5e-14` at `t = 0` and above the length tolerance past `t = 4.9e-5` — inside the search's
first interior sample (`1.22e-4`), so the boundary solution is the search's only sub-tolerance
one. Bare, `assert_valid` reports it at parameter 0; with an edge's context, the skip takes it
and the curve is accepted. (The earlier failure to build this pair was a wrong curve: the
smoothstep `(0,0) (1/3,0) (2/3,a/3) (1,a)` has interior sub-tolerance points, so a report
survives the filter whatever the context. The spike-shaped first span is what the countersink
trim has and what the pair needs.
`issue_2785_the_default_fixes_see_no_report_at_any_parametrisation` pins the difftest's side.)

## The fraction, freed

The tolerance change alone had pinned the fraction into a 1.90x window: `wr_668` must report at
`4.71e-5` of the curve's box, the countersink trim had to be accepted at `8.96e-5`, and
`1/10000` sat 11.6 % above the top — which is why the case moved. With the end minimum decided
by the skip instead, the window widens to `(4.71e-5, 4.0e-4)` (`assert_valid_accepts_regular_
curvature_and_linear_empty_analysis` holds the top: radius `2e-4` on a box of `0.5`) — **8.5x
wide**, with `1/10000` 2.1x above the floor and 4x below the ceiling. The constant is no longer
marginal, and stays as agreed on the issue.

The alternative considered was the **scale**, not the fraction: `docs/definitions-and-certificates.md`
§5 asks for a gate at part scale, and a UV trim's own box is not the part — this trim is 1.69 mm
on a much larger face. A face-scale tolerance would also accept this trim, but it needs the
face's size plumbed into `assert_valid` (an API change), it changes every trim's tolerance in the
corpus, and it would rescue the case by its face's size rather than at its cause. Rejected for
this fix; worth revisiting if a case shows a genuine interior near-cusp on a small trim being
missed.

## What this does to SolidCraft's reports

The check is the one that refuses a body SolidCraft has just built ("SMLib's check refuses the
result"). A report survives the tolerance change iff `size_t > radius / old_tolerance`; the ratio
is given per issue so triage can decide without re-running:

| SolidCraft issue | test | radius | old tolerance | survives iff `size_t` > |
|---|---|---|---|---|
| #12059 | 13 | 9.3833822e-8 | 1.4354766e-4 | 6.54e-4 |
| #12090 | 14 | 2.0557583e-3 | 9.403931e-3 | 0.2186 |
| #12319 | 13 | 2.2940238e-5 | 5.0182638e-5 | 0.4571 |
| #11582 | 13 | 2.6946122e-6 | 2.6884914e-3 | 1.00e-3 |
| #11754 | 13 | 2.6946122e-6 / 1.3805233e-6 | 2.6884914e-3 / 4.0427811e-3 | 1.00e-3 / 3.41e-4 |

None of these is closed by this fix alone. What it changes is that the verdict no longer depends
on the domain, so a report that survives is a real near-cusp in the geometry and can be fixed at
the construction that made it, instead of appearing or vanishing with the parametrisation.
