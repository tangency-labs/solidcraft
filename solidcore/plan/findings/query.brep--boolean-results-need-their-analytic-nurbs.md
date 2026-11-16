# query.brep: a boolean result's swept faces have no NURB, and the port's bounding box cannot make one (2026-10-26)

Unit: `query.brep` (`SmBrep::CalculateBoundingBox`) with `interface` in
`nurbs.b_spline_surface_p1` (`SmBSplineSurface::CalculateBoundingBox`) and
`surface.surface_p1` (`SmSurface::CalculateBoundingBox`). Found while validating
tangency-labs/solidcore#434; **not fixed** by that PR.

## What happens

After any boolean, the result's `SmSurfOfExtrusion` faces have `m_pNurb == NULL` (their
base `SmBSplineSurface` holds no NURB; the generating curve and extrusion vector are what
they carry). `SmBrep::CalculateBoundingBox` boxes each face, and
`SmBSplineSurface::CalculateBoundingBox` reaches its control net through
`((SmBSplineSurface *)this)->GetOrCreateGwNurbPointer()`, whose virtual `MakeNurb()` for an
`SmSurfOfExtrusion` rebuilds it (`SmSurfOfExtrusion.cpp`, `MakeNurb`). The Rust port calls
`BSplineSurface::nurb_or_err` there — a `const` query that cannot make one — and returns
`SM_ERR` (1001):

```
Upstream { operation: "nurbs::b_spline_surface::p1::BSplineSurface::calculate_bounding_box",
           code: 1001 }
```

`mass::integrator::properties::brep_volume` plans its sampling from that box, so the volume
of such a body fails too.

## Reproduce (Rust, no difftest case reaches it)

Two 12 x 60 walls swept along half circles and joined, or even two disjoint swept walls
unioned: the union returns `Ok`, then `query::brep::calculate_bounding_box` fails. A plain
`SmBrep::Transform` of one wall does not lose the NURB (measured), only the boolean does.
Per-face, only the `SmSurfOfExtrusion` faces fail; the caps (planes, B-spline tops) box
fine.

## Why it is not a C++ disagreement in this form

The C++ reference does not fail: it makes the NURB. So this is a port gap, not a declared
divergence — a Rust failure where C++ succeeds — and no difftest unit currently exercises
it (`BooleanPostProcess`/merge cases compare other values). Fixing it means letting the
`const` box path create the NURB, e.g. through the same "make it before the query on the
caller's `&mut` B-rep" shape `Surface::split_at` uses (`topology.face_p1`, PR #637), or by
threading the mutation through `LazyBrep`.

## Workaround used by the harness

`crates/difftest/src/solidcraft.rs` (`make_missing_nurbs`) makes the missing NURBs on the
body before measuring, with a comment pointing here. It changes no production behaviour.
