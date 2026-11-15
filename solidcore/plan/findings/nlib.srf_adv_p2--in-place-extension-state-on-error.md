# nlib.srf_adv_p2: in-place extension state on error, exposed by 1.0.1's N_CrvRemoveKnot change (2026-10-26)

**Unit:** `nurbs::srf_adv::p2` (`ST_surxsdr`, the core of `N_SrfExtendByDist`) and its
in-place caller `extend_side` in `nurbs::b_spline_surface_n_lib::p1`
(`SmBSplineSurface::CreateExtendedSurface`'s halving/doubling loops).

**Inputs:** `surface.surf_of_extrusion_p2` `lazy_big_extend_cinf` and
`lazy_big_extend_dir_umax` (`fixtures/surface.surf_of_extrusion_p2/lazy_nurb.json` #67,
#69): the recognised extrusion over u knots `[2,2,2,3,5,5,5]`, extended by 0.25 (both
directions, C-infinity) and by 0.05 (UMax).

**Values (u-max end knot after the extension):**

| run | cinf | dir_umax |
|-----|------|----------|
| main Rust = C++ c9c5979 | 5.000586556 | Ok, 5.000117327 |
| pin101 Rust before the fix | 5.000293302 | Err 1001 |
| C++ 1.0.1 | 5.000879753 | Ok |
| pin101 Rust after the fix | 5.000879753379812 | Ok, 5.000175987108615 |

**Cause.** Every upstream caller runs `ST_surxsdr` in place (`N_SrfExtendByDist(&esur, ..,
&esur)`). Its closing `N_SrfRemoveKnotConditional(surQ, .., surQ)` (unchanged in 1.0.1)
finishes the removal and then returns the error flag of its last failed
`N_BasisFindGlobalMax`; `ST_surxsdr` then returns that error with the extended,
knot-reduced surface already written into `surQ`. `CreateExtendedSurface` halves the
distance and retries on that modified `esur`, so the extensions accumulate. The port
returned the extended surface only on success: on the error `extend_side` kept the
original surface and retried from it (a smaller final extension, and for `dir_umax` the
1/100 floor, `SM_ERR`). The `srf_adv/p2.rs` Deviations bullet claimed the in-place mode
always leaves the caller to replace its surface with a returned one.

**Why it surfaced only at 1.0.1.** In c9c5979 the curve `N_CrvRemoveKnot` returned the
error of its last failed `N_BasisFindGlobalMax`, so the iso-curve extensions failed on
the same searches before the surface removal was reached, on both sides. 1.0.1 falls
back to a bound of 1 there (`error = NL_NO`), so the extension reaches the surface
removal, whose global-max search fails identically in both builds (gdb trace:
`scratchpad/e3/grk2.log`, `N_BasisFindGlobalMax` returning 1 at the step where Rust
halved and C++ continued from u = 5.000586).

**Fix (pin/v1.0.1).** `srf_adv::p2::extend_by_dist_in_place(esur, ..)` tracks what
upstream's in-place `surQ` holds when `ST_surxsdr` fails:
- *Unwritten* (an error before `N_AllocSrfArrays` gives `surQ` new arrays: no iso-curve
  extended, the recursion limit, the `surB` copy): `esur` stays the input, as upstream.
- *Written*: a failing closing `N_SrfRemoveKnotConditional` (the G1R removal, the two
  reflection/Cmax removals) finishes and leaves `surQ` complete; `esur` becomes the
  surface that removal produced (`srf_tool::p2::remove_knot_conditional_with_status`).
  When the second recursive step fails while still *Unwritten*, `surQ` holds the first
  step's (complete) surface, which `esur` becomes.
- *Half-written*: an error after `N_AllocSrfArrays` and before `surQ` is complete (the
  `Cmax` weight check's `NL_WEI_ERR`, a failed knot insertion or solve) leaves partly
  written arrays that upstream's retry reads uninitialised: UB-107
  (`plan/upstream-issues/UB-107--createextendedsurface-retries-an-in-place-extension-on-half.md`).
  The port cannot reproduce that read and keeps the input there, as before; that
  remains the documented UB-107 deviation.

`extend_side` uses the variant in its halving and doubling loops. The `srf_adv/p2.rs`
Deviations bullet now states the three cases and cites UB-107. `srf_adv/p3.rs`'s note
(the in-place mode of `N_SrfExtendByParamDist`) is unaffected: no Rust caller retries a
failed `extend_by_param_dist` on its argument. The `surface.surface_p2` generator still
leaves out extension cases whose first attempt fails (`extension_retries_in_place`);
it could now admit the *Written* failures, but keeping it whole is conservative, since
the generator cannot tell the two failure kinds apart without the in-place state.
