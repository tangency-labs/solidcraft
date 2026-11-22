# surface.surface_p2: S2b-4 slice 1, `SetSurface`, the API evaluation checks and the offset extension make a lazy analytic's NURB (2026-10-27)

Updates: plan/findings/surface.surface_p2--s2b4-lazy-domain-suspects.md

Slice 1 threads the kernel into three of the listed sites. Each now reads the domain or
NURB as upstream does (`GetNaturalUVDomain` / `GetOrCreateGwNurbPointer`). Every caller
already held a kernel.

| Site (`crates/solidcore/src/`) | Change | Callers given `k` |
|---|---|---|
| `topology/face/p1.rs` `store_surface` (`SmFace::SetSurface`) | `face.uv_domain` from `natural_uv_domain_with_nurb(k)`, read before the face is touched | `set_surface`, `set_surface_notified` and their callers: `topology/brep/p4.rs` `swap_face_surfaces` (`DeleteEdge`), `topology/brep/p2.rs` `replace_surface`, `topology/face/p1.rs` `move_seam_notified`, `blend/fillet_executive/p1.rs` `remove_topo_edges`, the `topology.face_p1` runner |
| `query/brep.rs` `checked_surface` (`SmApiEvaluateSurfacePoint/Normal/Derivatives`) | parameter check against `natural_uv_domain_with_nurb(k)` | the three evaluation entry points |
| `surface/offset_surface.rs` `nurb_of`, `extend_toward` (`sm_ExtendSurface`) | the NURB through `Surface::kind_with_nurb(k)`, so a lazy base is extended as upstream. Previously it failed with `SM_ERR_NULL_POINTER`, even after the offset's own evaluations had made and kept the NURB | `extend_surface`, `extend_from_base` |

`Surface::natural_uv_domain_with_nurb` itself (`surface/surface/p1.rs`) gains the surface of extrusion arm. `SmSurfOfExtrusion` does not override `GetNaturalUVDomain`, so a lazy extrusion answers its made NURB's domain. Before, a lazy extrusion over a non-B-spline generator (a line) answered the NURB-less base's empty domain. The slice's lazy test exposed it.

Tests:
- `crates/solidcore/tests/lazy_analytic_nurb.rs` compares `evaluate_surface_point` on each lazy kind with the made one.
- `an_offset_extends_a_lazy_base_as_a_made_one` compares an offset's extension past a lazy cylinder base with one past the made base.
- The `topology::face::p1` unit test checks the face domain `set_surface` gives a lazy cone.

Not in this slice:
- `IsoCurve`: `from_surface` callers in `surface/surface/p5.rs`, `p6.rs` and `blend/fillet_solver/p3.rs` have no kernel, and `natural_interval` is reached through kernel-free `Curve` dispatch.
- `TrimSrfCache::new`: `Surface::attach_trim_cache` has no kernel, and open PR #1307 edits `trim_srf_cache.rs`.
- `domain_with`: both callers are kernel-free accessors (`OffsetSurface::natural_uv_domain` and the `SurfaceEvaluate` impl). `OffsetSurface::natural_uv_domain_with_nurb` is already the lazy-safe form.
- `base_step_uv_domain`: only B-spline, curve-bounded, STEP and offset kinds reach it, so only an offset over a lazy base can be lazy there. Fixing it means threading `k` through `Surface::step_uv_domain`.
- The fillet helpers, `set_create_fail`, the generic `SurfaceEvaluate` paths and the trait-impl operands.
