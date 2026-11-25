# topology.proto_topology_p1: Public extrusion scale inherited conversion (2026-10-24)

`topology.proto_topology_p1`: a finite `SurfOfExtrusion` with a genuine quadratic
B-spline generator, materialized NURB and public `CreateFaceFromSurface` topology
passed identity, translation and uniform scale, but failed anisotropic scale and
signed `ScaleByPt`. One interior surface coordinate stayed 2.68 in Rust while native
returned 5.36 / -2.36. Native `SmSurfOfExtrusion` inherits `SmBSplineSurface`, so
`SmBrep::TurnToNURBS` copies it before transforming. Adding that inherited kind to
`brep::p3::{as_bspline_surface,with_nurb}` fixes all seven real-face cases, including
odd/even reflections, without changing owner identities.

Separate remaining provider gap: without explicit `MakeNurb` on either side before
face creation, all five initial extrusion scripts stop at Rust face status 1006.
`surface::surface::p6::create_iso_parametric_curve` reads the extrusion's empty base
instead of obtaining its lazy NURB through the native virtual path. These transform
fixtures explicitly materialize the real surface on both sides; they do not fix or
claim coverage of that lazy-construction path.

Preserved from PR #758 during the per-entry findings migration.
