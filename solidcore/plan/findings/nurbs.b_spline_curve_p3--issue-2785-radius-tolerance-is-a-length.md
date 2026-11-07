# `SmBSplineCurve::AssertValid` tests 13/14 compare a radius of curvature with a speed

Unit: `nurbs.b_spline_curve_p3` (`crates/solidcore/src/nurbs/b_spline_curve/p3.rs`).
Issue: tangency-labs/solidcore#2785. Fix: this PR (a redo of #2859, reverted by #3019).

![A three-panel schematic. A: one cubic Bezier drawn once, with two parameter axes under it over the same span, one from 0 to 1 (T = 1) and one from 0 to 50 (T = 50), and the note that the same span carries 50x more parameter so the average speed is 50x smaller. B: the bend magnified, with the osculating circle and its radius drawn from the bend to the centre, r = 5.82e-6, the same at T = 1 and at T = 50. C: log-log, domain length T against length: the bend's radius a level black line, the speed tolerance a dashed falling line crossing it at T = 24.3, the length tolerance a level blue line above both, with vertical markers at T = 1 and T = 50 and the verdict at each.](issue-2785-radius-tolerance.svg)

*Figure. The mechanism, drawn for #2859 by @saosebastiao in VectorCraft 0.21.1 through its MCP
server and exported with live text (`outlineText: false`); `issue-2785-radius-tolerance.vectorcraft`
beside it is the source. Kept here rather than redrawn: it is the same half of the defect this fix
rests on, and this record adds the half it does not show (§The rig case).*

## The defect

`BSplineCurve::assert_valid` builds one tolerance and used it for two different kinds of quantity:

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

## The rig case: `crashes/2992-join-disk-and-36-twisted-loft-blades`

#2859 was reverted because this case's `union_all` (call 111) came back "the result is not a closed
solid" (`is_outward_solid` runs `check_brep` at `Level0`, whose `AssertValid` is the check). With
the tolerance change re-applied on main `b83367a49` and a print at the 13/14 report site
(`KERNEL_CASES_CASE=2992`), the case reports **74 times, every one test 13 (a 2D UV trim), none
test 14** (the edges' 3D curves).

The trim that decides it (`key=…21995`, one of 74, all of them the same shape):

| quantity | value |
|---|---|
| mapped trim size (constant metric at the trim's middle) | 42.6668 |
| `size_t` (the trim's domain length) | 38.5956 |
| old tolerance `(mapped/T)/1e4` | 1.1055e-4 |
| new tolerance `mapped/1e4` | 4.2667e-3 |
| the search's radius at the solution | 7.7936e-4 |
| radius of the true image `S(u(t), v(t))` there | 8.4959e-4 |
| solution parameter | 1.829112777997185 of [0, 38.5956] |

Four things this measures against the issue's read of the case:

1. **The new tolerance is not stricter by the trim's `T`; it is looser by it.** `new = T * old` =
   38.6x. The report is new because the radius is `7.79e-4`: it passed `1.11e-4` by 7.0x and fails
   `4.27e-3` by 5.5x.
2. **The true curve on the surface has the same small radius.** `8.4959e-4`, from
   `C' = S_u u' + S_v v'` and `C'' = S_uu u'^2 + 2 S_uv u' v' + S_vv v'^2 + S_u u'' + S_v v''` at
   that parameter, evaluated on the surface. The constant metric at the middle is not what makes it
   tight: it is 9 % off the true radius there, and no report on this case differs by more than a
   factor 1.1 between the two.
3. **The 3D curve is G1 and so is the image**, but the image is C1 and not C2. The trim crosses
   the surface's v knot at `v = 12.000002` where the surface's own speed jumps, `|S_v|` 8.456 to
   13.005, and its `v'` compensates, 0.1174 to 0.0763 — `0.993` either side — so the 3D tangent is
   continuous while `v''` is not. The composite's tangent either side differs by `6.4e-5` rad.
4. **The small radius is a sub-nanometre S-bend at a knot cluster.** The pcurve's knots there are
   `…, 1.817552, 1.826094, 1.828625, 1.828959, 1.829113`, and the solution sits on the last of
   them; the span after it is the whole rest of the curve. `c1 = (4.3e-4, 7.61e-2)`,
   `c2 = (-3.9e-5, -785.4)`. The image's tangent against the tangent at the solution:

   | offset (x domain length) | -1e-6 | -1e-5 | -1e-4 | -1e-3 | +1e-6 | +1e-3 |
   |---|---|---|---|---|---|---|
   | tangent turn (rad) | 1.817e-2 | 1.800e-5 | 1.798e-4 | 1.771e-3 | 1.80e-6 | 1.804e-3 |

   The tangent swings 1.8e-2 rad and returns within `3e-5` mm: an S-bend of about `5e-7` mm
   amplitude and `8.5e-4` mm radius. The edge's own 3D curve does not report — its radius is far
   above its tolerance — so nothing at the model's scale is a defect here.

## Why the fraction cannot be re-fitted

A test `radius < size / X` has to satisfy both of:

| case | `radius / size` | verdict the corpus needs |
|---|---|---|
| `wr_668_zero_speed_tangent_end_is_no_kink` (`zero_speed_end_curve`, radius `6.7e-5` on a box of `sqrt 2`) | 4.74e-5 | report |
| this trim | 1.83e-5 (`7.79e-4 / 42.67`) | accept |

`report` needs `X < 21100`, `accept` needs `X > 54700`: no `X` does both. The window #2859 found
(`1e4` inside `4.71e-5 … 8.96e-5` of the box, between `wr_668` and #2343's countersink trim) is
closed by this case, so the redo cannot stay a fraction of the curve's box where a UV trim is
concerned. Fitting the fraction to `1/20000` would also satisfy both and is refused: it is a number
chosen to keep two cases passing, not derived from anything.

## The fix

Two changes, both on the radius tests' half of `assert_valid`:

1. **The tolerance is a length.** `radius_tol = size_xyz / 1e4` (`1e-03` for an unbounded curve):
   upstream's own `1/10000` fraction, applied to the right quantity, and the only scale in the
   function that does not involve `size_t`. `tol` stays the speed tests 11/12 need, and
   `zero_speed_radius_stretch` gets a length to divide by a growth rate.
2. **Tests 13/14 do not run on a UV trim** (`Kernel::fixes.uv_trim_checks_on_surface`'s regime,
   WR-560). A trim is a parameter-space representation of its edge's 3D curve; only that curve is
   the model's geometry, and the edge's own check runs this test on it (14). The trim's image
   carries its pcurve's bends, which is what this case measures. WR-560's mapping at the trim's
   middle was the earlier answer to the same problem in the other direction — in raw (u, v) the
   trim's radius is the surface's anisotropy, in the mapped metric it is the pcurve's bends — and
   both are false positives.

The trade, named: a pcurve whose *own* shape is bad is no longer reported by 13/14. Its other
tests still run (3, 5/6, 11/12, 15), and the curve it represents is checked on the edge. What the
test loses with it is WR-560's `issue_891` trim reported in raw (u, v), which was the false
positive the mapping was added for.

## What moves in the corpus

Nothing on the default path: `bspline_check_min_radius` is `false` in `UpstreamFixes::default()`,
so `min_radius` is never read and 13/14 never fire, and `uv_trim_checks_on_surface` is `false`
there too, so the trim branch does not run at all. The difftest and the goldens are blind to this
change; `issue_2785_radius_tolerance_is_a_length_not_a_speed` pins that at four `T`.

With the fixes on (the kernel rig, `crates/solidcore/tests/kernel_cases/fixes.json`):

| area | cases | pass | unsupported | known-failing | fail |
|---|---|---|---|---|---|
| crashes | 12 | 9 | 3 | 0 | 0 |
| booleans, sweeps, sheet-metal, surfaces | 359 | 252 | 107 | 0 | 0 |
| fillets | 274 | 239 | 16 | 19 | 0 |
| shell, import, construction, threads, history, assemblies, plastic, inspect, tessellation, continuity, solids, frames | 264 | 143 | 120 | 1 | 0 |
| **all 909** | 909 | 643 | 246 | 20 | **0** |

`issue_2343_curved_setback::countersink_asked_corner_is_certified_with_its_nets` passes, and so
does `issue_2574_straight_trim_min_radius`'s tight bend beside a domain end — the case that decided
#2859's boundary-end skip. Neither needs the skip any more: both trims are simply not radius-tested,
so #2859's second change is not part of this one.

## What this leaves for #2785's flag

WR-382's `bspline_check_min_radius` is still off by default, so this reaches no user until the flag
is retired. Retiring it is the follow-up #2859 listed and this record does not take: it needs a
sweep of every case it makes the check run, now that 13/14 no longer fires on trims.

The alternative design, not taken: gate `radius_tol` at the model's tolerance — the edge's `tau_e`,
or the zone tolerance for a bare curve, as test 15 already does with `SM_ZONE_TOL_3D / 10`. It
accepts this trim (`8.5e-4` against a model tolerance of about `1e-5`) and `wr_668` still reports,
and it is the reading the issue's own "gate at part scale" note points at, but it turns a
part-relative kink test into an absolute one and needs the entity's tolerance plumbed into
`assert_valid`. Measured, not taken; if the maintainers want that instead, it replaces change 2 and
change 1 stays.
