# `SmBSplineCurve::AssertValid` tests 13/14 compare a radius of curvature with a speed

Unit: `nurbs.b_spline_curve_p3` (`crates/solidcore/src/nurbs/b_spline_curve/p3.rs`).
Issue: tangency-labs/solidcore#2785. Fix: this PR (a redo of #2859, reverted by #3019; the design
below follows the review of #3064).

![A three-panel schematic. A: one cubic Bezier drawn once, with two parameter axes under it over the same span, one from 0 to 1 (T = 1) and one from 0 to 50 (T = 50), and the note that the same span carries 50x more parameter so the average speed is 50x smaller. B: the bend magnified, with the osculating circle and its radius drawn from the bend to the centre, r = 5.82e-6, the same at T = 1 and at T = 50. C: log-log, domain length T against length: the bend's radius a level black line, the speed tolerance a dashed falling line crossing it at T = 24.3, the length tolerance a level blue line above both, with vertical markers at T = 1 and T = 50 and the verdict at each.](issue-2785-radius-tolerance.svg)

*Figure. The mechanism, drawn for #2859 by @saosebastiao in VectorCraft 0.21.1 through its MCP
server and exported with live text (`outlineText: false`); `issue-2785-radius-tolerance.vectorcraft`
beside it is the source.*

## The defect

`BSplineCurve::assert_valid` built one tolerance and used it for two different kinds of quantity:

```rust
let size_t = domain.length();
let tol = if bounded { (size_xyz / size_t) / 10000.0 } else { 1e-03 };  // a speed
let (mut radius_sized, mut radius_tol, mut radius_sep) = (sized, tol, min_sep);  // a length
```

`size_xyz / size_t` is a length per parameter — the curve's average speed, the right unit for tests
11/12, which compare `|C'|`, and for the `speed_clear` bound above them. A radius of curvature is a
**length**, so tests 13/14 compared a length with a speed, and stretching a curve's knot vector
moved the verdict although the curve in space did not change. The mapped UV-trim branch
(`radius_tol = (mapped_size / size_t) / 10000.0`) had the same shape, and
`zero_speed_radius_stretch` (WR-668) divides `radius_tol` by the radius' growth rate to get a
stretch in *parameter* units, which the speed made dimensionless.

## The fraction is upstream's own, and it is a fraction of a length

`SmBSplineCurve::AssertValid` (SmBSplineCurve.cpp:7883) reads

```cpp
double dTol    = IsBounded() ? (dSizeXYZ/dSizeT) / 10000        // dTol proportional to average curve speed
                             : 1e-03 ;
```

and directly above it, commented out by `gwc`:

```cpp
// gwc:removed old tolerances in favor of tolerances proportional to average curve speed
//      double dTol    = IsBounded() ? dSizeXYZ / 10000                 // dTol proportional to length
```

So `1/10000` is not fitted here: it is upstream's own fraction, and the quantity upstream applied
it to **before** `gwc` changed it was the length `dSizeXYZ`, not the average speed. The fix
restores upstream's own length form. Written as the ratio it is, the test is
`r_min / L >= 1/10000` with `r_min` and `L` measured on the same image of the curve — a
dimensionless comparison, which is why the parameterisation, the units of the model and the
anisotropy of a surface's (u, v) cannot move it.

## Measured: the issue's curve

`(0,0) (h,0) (h,h) (1,1)`, `h = 1e-4`, on `[0,0,0,0,T,T,T,T]`: the same curve in space for every
`T`, smallest radius `5.815568702310391e-6` (the search's first solution is `5.8155015634573e-6`,
#2637).

| `T` | old tolerance `sqrt(2)/T/1e4` | new tolerance `sqrt(2)/1e4` | old verdict | new verdict |
|---|---|---|---|---|
| 1 | 1.414212e-4 | 1.414212e-4 | reported | reported |
| 20 | 7.071068e-6 | 1.414212e-4 | reported | reported |
| 50 | 2.828425e-6 | 1.414212e-4 | **not reported** | reported |
| 1e4 | 1.414212e-8 | 1.414212e-4 | **not reported** | reported |

The old tolerance crosses the true radius at `T = 24.3177`. `issue_2785_radius_tolerance_is_a_
length_not_a_speed` pins the same radius and the same tolerance at seven `T`, and
`issue_2785_a_regular_bend_is_accepted_at_every_parametrisation` the accepting side.

## A UV trim is measured on its image, not skipped

The review of #3064 asked for two things that this record settles:

1. **Tests 13/14 must run on a UV trim.** They do. A trim's lengths are those of its image on its
   face's surface, so the ratio is taken there: the trim mapped by the surface's first fundamental
   form at its middle (WR-560's `sm_TrimMetric` / `sm_MappedTrim`, which test 15 already uses),
   with the mapped trim's extent and solution spacing. Both sides of the comparison are then
   measured on the same image. `issue_891_uv_trim_radius_is_measured_on_its_surface` is WR-560's
   own case: a r 1 circle's exact trim on a plane spanning 600 x 3 is an ellipse of radius 8.3e-6
   in raw (u, v) — reported — and the circle of radius 1 on the plane, which is not. A trim whose
   image really bends is still reported: `issue_2785_a_uv_trim_whose_image_bends_is_reported` puts
   the issue's cubic in a 1 x 1 plane's parameters and reads test 13 at radius 5.8155e-6 against
   `sqrt(2)/1e4`.
2. **The rig case must pass on its own geometry.** It does not, and the cause is not the check.
   See below; the check is right about what it measures.

## A radius minimum on the curve's own end is not a kink

Restored from #2859. The first-derivative test (11/12) passes over solutions on the natural
boundary ends with the comment "they are not a problem"; the radius test read them, so a curve
whose tightest bend is at its own end was refused. `crashes/2992`'s trims are not the only case:
`issue_2343_curved_setback::countersink_asked_corner_is_certified_with_its_nets` (every flag on)
refuses the countersink corner with test 13 at radius `1.51e-4` against `1.69e-4`, at
`owner_param = 0.0` — the trim's own domain start. With the boundary-end solutions passed over the
corner is certified again, and the test is the regression test for this half.

## The rig case: `crashes/2992-join-disk-and-36-twisted-loft-blades`

#2859 was reverted because this case's `union_all` (call 111) came back "the result is not a closed
solid" (`is_outward_solid` runs `check_brep` at `Level0`, whose `AssertValid` is the check). With
the length tolerance and the mapped measurement, the case still refuses, and the trims it reports
are not kinked: their images have a curvature spike where their 3D curves are smooth.

Measured on the union's result body (the corpus's raw counts, 185 faces, 549 edges, 366 vertices;
the rig's `fixes.json` with `bspline_check_min_radius` off so that the union completes). Of 1898 UV
trims, the smallest image radius is `1.708027e-4` at `t = 60.694388050` of `[0, 61.38314]`, where
the 3D curve's radius is `234.2`; the mapped size is `75.68592`, so the mapped length tolerance is
`7.5686e-3` — the report is 44x below it. The next ones are `2.665216e-4` at `54.948894080` (3D
radius `75.1`) and `4.322271e-4` at `60.694447967` (3D radius `234.5`).

The trim that decides it, at its smallest image radius: 121 control points, 41 distinct knots,
every interior knot of multiplicity 3 — one cubic Bézier piece per 3D curve span — on a loft whose
`v_knots` are the integers `0…19` (so the surface is only C0 in v at each of them, `|S_v|`
jumping). The spike sits on the last knot of the cluster `54.943193, 54.948888, 54.948969`, where
the trim crosses the surface's `v = 12` line:

| offset from the spike, x the trim's domain | `\|C'\|` of the image | image radius | the 3D curve's radius | image − 3D curve | tangent, image vs 3D |
|---|---|---|---|---|---|
| −1e-6 | 1.000000 | 75.244 | 75.244 | 3e-14 | 1e-12 rad |
| −1e-7 | 1.000000 | 3.976e-4 | 75.434 | 5e-14 | 1e-10 rad |
| −1e-8 | 1.142142 | 6.566e-4 | 75.402 | 4.0e-7 | 1.16e-2 rad |
| −1e-9 | 0.715812 | 2.652e-4 | 75.379 | 2.9e-7 | 3.70e-2 rad |
| 0 | 0.716720 | **2.665e-4** | 75.389 | 2.7e-7 | 3.68e-2 rad |
| +1e-7 | 0.802034 | 4.254e-4 | 75.368 | 1.2e-6 | 2.30e-2 rad |
| +1e-6 | 1.071769 | 4.052e-3 | 75.004 | 5.5e-7 | 6.2e-3 rad |
| +1e-5 | 1.000000 | 75.255 | 75.431 | 5e-12 | 1.7e-8 rad |

The image is smooth to `2.7e-7` mm — 74x inside the edge's `2e-5` mm tolerance — and its tangent
turns `3.7e-2` rad and returns within `1e-5` of the domain. Its speed dips to `0.717` where the 3D
curve's is `1`. So the bend is an artefact of how the trim is represented, not of the model's
geometry: the drop (`sm_AddToCurve`, WR-753) emits one Bézier piece per 3D curve span, and the
surface's C0 knot line at `v = 12` falls `1.8e-5` inside a span, so the dropped UV derivative's
jump there cannot be carried by the piece. The fix is in the drop, not in the check: break the
pieces at the surface's knot lines. That moves every pcurve crossing one and needs its own sweep;
it is WR-753 and not this PR.

**Measuring on the mapped trim does not avoid this**, which is why the review's alternative was
measured rather than assumed: the mapped trim is a linear image of the same pcurve, so the notch is
in it too (the author's earlier measurement read `7.79e-4` on the mapped trim against `8.50e-4` on
the true image), and the true image's radius is smaller still (`1.7e-4` here). Neither the raw
(u, v) radius nor the mapped one is the model's geometry; both carry the pcurve's own notch.

## What moves in the corpus

`bspline_check_min_radius` is `false` in `UpstreamFixes::default()`, so `min_radius` is never read
and 13/14 never fire on the default path; the goldens and the difftest cannot see this change, and
`issue_2785_radius_tolerance_is_a_length_not_a_speed` pins that at four `T`. `uv_trim_checks_on_
surface` is `false` there too, so the mapped branch does not run on the default path either.
Goldens `nurbs.b_spline_curve_p3`: `cases=2460 changed=0 added=0 removed=0 broken=0
known_failing=0`.

With the rig's `fixes.json` (both flags on), the whole rig — `cargo test --profile goldens -p
solidcore-ops --test kernel_cases -- --ignored`, 909 cases:

| area | cases | pass | fail | unsupported | known-failing |
|---|---|---|---|---|---|
| crashes | 12 | 8 | **1** | 3 | 0 |
| booleans | 124 | 112 | 0 | 12 | 0 |
| fillets | 274 | 239 | 0 | 16 | 19 |
| sweeps | 95 | 88 | 0 | 7 | 0 |
| sheet-metal | 78 | 12 | 0 | 66 | 0 |
| surfaces | 62 | 40 | 0 | 22 | 0 |
| shell | 36 | 33 | 0 | 2 | 1 |
| the remaining areas and ladder tiers | 228 | 111 | 0 | 117 | 0 |
| **all 909** | 909 | 643 | **1** | 245 | 20 |

The one failure is `crashes/2992-join-disk-and-36-twisted-loft-blades`, and it is WR-753: the
union refuses because a trim's image has a curvature spike that its 3D curve does not. Every other
case is unchanged, and `issue_2343_curved_setback::countersink_asked_corner_is_certified_with_its_
nets` passes again with the boundary-end solutions passed over.

## What this leaves for #2785's flag

WR-382's `bspline_check_min_radius` is still off by default, so this reaches no user until the flag
is retired. Retiring it is the follow-up #2859 listed: it needs a sweep of every case it makes the
check run, and WR-753 first, or the sweep refuses `crashes/2992-…`.

The other alternative, measured and not taken: gate `radius_tol` at the model's tolerance — the
edge's `tau_e`, or `SM_ZONE_TOL_3D` for a bare curve, as test 15 does with `SM_ZONE_TOL_3D / 10`.
It accepts this trim (`2.7e-4` against an edge tolerance of `2e-5`) and would keep the rig green,
but it turns a part-relative test into an absolute one, it needs the entity's tolerance plumbed into
`assert_valid`, and it does not fix the notch — it hides it. WR-753 is the root cause.

Two more follow-ups, not in this PR:

1. **WR-753** — break the drop's UV pieces at the surface's knot lines, so no piece straddles a C0
   line. That is what makes `crashes/2992-…` pass tests 13/14 on its own geometry; it moves every
   pcurve that crosses a surface knot line and needs its own sweep.
2. `radius_sep` is `size_xyz / max2(20., size_t)` (and `mapped_size / max2(20., size_t)` for a
   trim): a length over a parameter count below 20, a length per parameter above it. It is the
   search's resolution rather than a compared quantity, so it cannot move a verdict the way the
   tolerance did, but it is the one mixing left in the function and wants the same audit.
