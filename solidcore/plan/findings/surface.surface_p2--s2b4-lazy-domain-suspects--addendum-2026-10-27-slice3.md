# surface.surface_p2: S2b-4 slice 3, code generic over `SurfaceEvaluate` reads a lazy analytic's domain from its made NURB (2026-10-27)

Updates: plan/findings/surface.surface_p2--s2b4-lazy-domain-suspects.md

Slice 3 resolves the five generic `SurfaceEvaluate` rows of the list. Their receiver is a
type parameter, so the inherent `Surface::natural_uv_domain_with_nurb` was out of reach.
The trait now carries upstream's virtual `GetNaturalUVDomain` with the kernel:
`SurfaceEvaluate::natural_uv_domain_with_nurb(&self, k) -> Result<Extent2d>`.

Why a trait method rather than only passing `k` to the generic callers: those callers
already hold `k` (except `TangentField::new`). What they lacked was a way to reach the
lazy-safe domain through the type parameter, and in upstream that dispatch is the virtual.
The method has the name and signature of the inherent `Surface` method, so on a concrete
`Surface` both resolve to the same body.

| Site (`crates/solidcore/src/`) | Change |
|---|---|
| `surface/surface/p2.rs` `SurfaceEvaluate` | new required method. `Surface` delegates to its inherent `_with_nurb`. `BSplineSurface` answers its own domain (it is its own NURB). |
| `surface/offset_surface.rs` `OffsetEval` impl | the call's extension, else the base, each through `_with_nurb(k)`: the same surface `domain_with` picks, read lazy-safe |
| `surface/surface/p7.rs` `base_evaluate_normal` | the normal's domain (read at degenerate first derivatives, e.g. a pole) via the trait method |
| `surface/surface/p7.rs` `compute_higher_order_derivs` | the step-direction domain test via the trait method |
| `surface/surface/p7.rs` `compute_inward_vector` | the domain midpoint via the trait method |
| `curve/tangent_field.rs` `TangentField::new` | gains `k`; a `None` domain is the surface's `_with_nurb` domain. Callers updated: `TangentField::create`, `nurbs/b_spline_surface_n_lib/p2.rs` `interpolated_tangent_field_curve`, the `curve.tangent_field` runner, unit tests |
| `nurbs/b_spline_curve_n_lib.rs` `set_intersection_curve_through_point` | the projection domain via the trait method |

For a built surface the trait method answers what `natural_uv_domain_if_built` did, so only
inputs that reach these paths with a lazy analytic can change, and those now follow
upstream. The kernel-free `SurfaceEvaluate::natural_uv_domain_if_built` stays on the trait
for now. It can be removed once no generic code calls it.

Tests: `crates/solidcore/tests/lazy_generic_surface_domain.rs` compares a lazy cone and a
lazy pole-to-pole sphere with copies whose NURB was made first, bit for bit, on:
- the trait domain;
- `base_evaluate_normal` inside and on both v limits (the sphere's poles);
- `compute_inward_vector`;
- `compute_higher_order_derivs`;
- a `TangentField::new(k, .., None)` evaluated mid-curve.

It also checks that the trait domain of a lazy cone is the made NURB's, not the empty
kernel-free answer.

Still open from the list: the fillet helpers (`blend/fillet_geom/p1.rs`,
`fillet_intersector.rs`, `fillet_solver/p4.rs`), the kernel-free trait-impl operands
(`fillet_geom/p1.rs`, `intersect/topology_solver/p1.rs`, `query/face/p2.rs`), the
CurveBounded and SrfInVolume accessors, `TrimSrfCache::new`, `domain_with`,
`base_step_uv_domain`, `surface_operand` and `set_create_fail`.

**PR:** S2b-4 (stacked: slice 1, slice 2 and its missed callers, slice 3).
