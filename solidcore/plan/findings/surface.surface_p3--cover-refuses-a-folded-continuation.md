# surface.surface_p3: `CoverCoincidentSurface` refuses a continuation that folds (2026-10-26)

Unit: `surface.surface_p3` (and the sweep-frame half in `primitive.primitive_creation_p3`).
Upstream issue: tangency-labs/solidcore#434, fixed on the fork by
tangency-labs/smlib `1b6f5fa`. PR: the one carrying this file.

The port now carries both halves of that fix:

1. **A planar path's sweep frame** (`primitive/primitive_creation/p3/geometry.rs`,
   `prepare_curve_sweep`): for a planar path the frame that turns the path into the sweep's
   global frame starts with its z on the plane's normal instead of on
   `MakeUnitOrthoVectors` of the start tangent. With a start tangent along +y the old z
   lay *in* the path's plane, the path did not come out in the frame's XY plane, and the
   swept wall's flat top came out 2.8e-5 mm off its plane against a 1e-5 zone tolerance.
2. **A fold rule for a cover past the natural domain** (`surface/surface/p3.rs`,
   `CoverCoincidentSurface`): when the merge would extend a B-spline-based surface past
   its domain and back, the continued surface must not fold over the domain the merge
   uses. Its `Du x Dv` normal may neither vanish (below `EFF_ZERO_SQRT` of the normal at
   the original domain's centre) nor turn back (`dot <= 0` with the previous grid sample)
   between the neighbouring samples of a 21 x 21 grid. A failure returns "not coincident",
   so the faces keep the edge between them.

## What #434 reports, and what the port did

`smlib_sweep` two 12 x 60 walls along half circles (A: r 44..56 over 0..180 degrees, B:
r 52..64 over 90..270 degrees) and join them. The port as it stood (upstream `c9c5979`)
fails the join: wall A's top is 2.8e-5 off `z = 60` (measured on the port: cap control
points `z` max `60.00002796039269`), and the union ends in
`boolean::merge::p2::make_faceuse` status 1001. With fix 1 the caps are exactly planar
(`z` max `60.00000000000003`) and the union is one valid solid, 1 lump, 12 faces, 30
edges, 20 vertices, volume 223932.681 against the closed form 223932.717 (rel 1.6e-7,
the sweep's own approximation of the circular offsets).

## The 7 new known mismatches

`crates/difftest/known_mismatches/surface.surface_p3.toml`: `gen0041`, `gen1448`,
`gen1514`, `gen1567`, `gen2066`, `gen2303`, `gen2376`. The generated cases build random
rational surfaces whose continuation past the natural domain folds: at a corner of the
union domain that lies outside the original domain (e.g. `u = -1.8775`, original u
`-1.9700 .. 0.9193`, original v `-0.1915 .. 2.0307`, the sample at `v = -0.2359`), the
normal turns back, `dot` about `-0.93`. C++ `success = true` (it merges across the fold);
the port returns `success = false` with 7 instead of 11 compared values. These are
declared, deliberate divergences, not unfixed port bugs: they become `deviation(#434)`
when difftest has that outcome (#989). `surface.surface_p3` is `partial` for them until
then.

The two swept-wall cases do **not** depend on the fold rule in this kernel: their tops
merge onto the planar top's own extension (which does not fold), with or without it. The
rule is carried because it is the same merge path the fork hit on this very case (there
the merged face was rebuilt on the *swept* surface's folded continuation: UV gaps and
volume 157833 for 223933), and because a caller can reach it — the unit test
`cover_refuses_a_continuation_that_folds` drives a continuation whose `Du` vanishes at
`u = 1.5` and returns "not coincident" instead of extending the domain to `u = 1.5`.

## Related, found while validating (not fixed here)

A boolean result's `SmSurfOfExtrusion` faces carry no base NURB, and the port's
`SmBSplineSurface::CalculateBoundingBox` is a `const` method that returns `SM_ERR` where
upstream's `GetOrCreateGwNurbPointer` makes the NURB. So `SmBrep::CalculateBoundingBox`
(and the volume integrator that plans its sampling with it) fails on any boolean result
with a swept face — including a control pair of disjoint walls that never merge. The
`--solidcraft-cases` harness makes the missing NURBs explicitly before measuring
(`make_missing_nurbs` in `crates/difftest/src/solidcraft.rs`); the port itself still fails.
Recorded in `plan/findings/query.brep--boolean-results-need-their-analytic-nurbs.md`.
