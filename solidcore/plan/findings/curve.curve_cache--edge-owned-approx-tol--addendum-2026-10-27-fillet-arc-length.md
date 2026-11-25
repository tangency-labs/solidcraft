# curve.curve_cache: fillet arc-length walks retain the live edge tolerance (2026-10-27)

Updates: `curve.curve_cache--edge-owned-approx-tol--addendum-2026-10-27-length.md`.

Units: `curve.curve_p3`, `blend.fillet_geom_p2`; caller comparison:
`blend.fillet_executive_p3`. PR: #1334 (shared integration repair for the
children of #1286).

## Observed regression

After the common-edge length repair, canonical run 40552558565 passes the
sweep but fails the exact-base ratchet: eleven formerly passing fillet cases
now defer at `SmTol::GetZoneTol3d(const SmObject*)`. The same ratchet failure
appears on #1335, #1346 and #1347. No baseline, fixture, guard, waiver,
known-mismatch registry or tolerance is changed by this repair.

Affected cases: `curve-sih-box_diff_cyl-all-edges-{r0.1,r0.25,r0.5,r1.0,step2}`,
`source-file-{box_diff_cyl,corner_box,nurb_box}-all-edges`,
`stubs4-Heart-all-edges-r0.1`, `sweep4-brepForFillet2-all-r0.1`, and
`sweep5-nurb_box1-all-r0.5`.

A narrowed LLDB run observes `Curve::length_base_with_zone_tol` called by
`find_parameter_at_arc_length`, `fillet_geom::p2::arc_length_param`,
`calc_rail_int_rail_with_solvers`, and the live fillet executive. The rail
adapter has the actual target B-rep and edge, but the arc-length solver did
not carry that edge's tolerance into its repeated length caches.

## Repair and boundary

The crate-private owner-aware arc-length entry point forwards an optional
resolved edge tolerance through all four length calls: initial span length,
subsequent spans, initial Newton estimate and Newton updates. The public
entry point retains its signature and ordinary unresolved-owner behavior.
Integration accuracy, sign handling, formulas, iteration order and bounds are
unchanged; no extra geometry copy is introduced.

The fillet rail, setback and cross-section adapters supply the actual live
edge tolerance. The explicit reversed curve copy in the setback branch has
no owner upstream (`SmFilletGeom.cpp:3344-3349`), so it continues to use no
edge override. Other unresolved owned-curve callers are not silently given
a system-tolerance fallback.

## Focused native evidence

Matching upstream 1.0.1 (`3fd5b9548d19`), macOS ARM64:

- `blend.fillet_executive_p3`: 592 total, 586 value-comparing passes, two
  legitimate rejections, four existing known mismatches, zero failures and
  zero deferrals. The unit remains partial; its pre-existing known mismatches
  account for the unit-level maximum relative error of 1.
- Every one of the eleven formerly deferred cases now passes with zero
  relative error, comparing 891-2,549 floating values and 491-1,417 exact
  topology fields per case. This proves restored comparison, not that every
  fixture is a successful fillet rather than a legitimate refusal.
- `curve.curve_p3`: 3,081 value-comparing passes, zero failures/deferrals,
  maximum relative error zero.
- `blend.fillet_geom_p2`: 351 value-comparing passes, zero failures/deferrals,
  maximum relative error zero.
- Perturbation of `curve-sih-box_diff_cyl-all-edges-r0.1` is detected (1/1).

The existing native fixtures are the regression coverage; no new synthetic
wiring test or source-text assertion is added.
