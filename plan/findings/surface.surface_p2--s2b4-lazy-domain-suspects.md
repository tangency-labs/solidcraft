# surface.surface_p2: S2b-4 candidates, `natural_uv_domain_if_built` sites that may see a lazy surface (2026-10-26)

**Unit:** `surface.surface_p2` (`Surface::natural_uv_domain_if_built`, renamed from
`Surface::natural_uv_domain` in S2b-3), and the callers listed below.

**What:** S2b-3 made every caller of the kernel-free surface domain choose between
`natural_uv_domain_with_nurb(k)?` (upstream's `GetNaturalUVDomain`, which makes a lazy
analytic's NURB first) and `natural_uv_domain_if_built()` (the stored kind's domain: a lazy
cone, sphere, torus or surface of revolution answers its NURB-less base's empty domain). The
rule was to use `_with_nurb` wherever a kernel is reachable. The sites below keep
`_if_built` only because no kernel is reachable there, although their receiver can be a
face or analytic surface, so a lazy one would read an empty domain where upstream reads the
made NURB's. No difftest case is known to reach them with a lazy surface; they are listed
so they are not silently accepted.

| Site (`crates/solidcore/src/`) | Function | Receiver | Why no kernel | Why it may be lazy |
|---|---|---|---|---|
| `blend/fillet_geom/p1.rs:2459` | `SolutionObject/ParameterDomains::natural_uv_domain (impl)` | `Self::Surface(s)` | kernel-free trait method | solve operand can be a face surface |
| `blend/fillet_geom/p1.rs:2488` | `same trait, other impl` | `self.0` | kernel-free trait method | solve operand can be a face surface |
| `blend/fillet_geom/p1.rs:3909` | `check_seam_out` | `surf` | bool fn, no k parameter | fillet/face surface on a seam check |
| `blend/fillet_geom/p1.rs:3957` | `check_seam_on` | `surf` | bool fn, no k parameter | same |
| `blend/fillet_intersector.rs:703` | `base_natural_uv_domain` | `offset rail base_surface()` | no k in the impl | base is the original face surface |
| `blend/fillet_solver/p4.rs:310` | `new (constructor)` | `base_surfaces[0]` | constructor, no k | base surfaces are face surfaces |
| `curve/iso_curve.rs:171` | `from_surface_inner` | `c.surface` | constructor, no k | iso curve of a face surface |
| `curve/iso_curve.rs:244` | `create_uv_iso_line` | `self.surface` | no k parameter (Result fn) | same |
| `curve/iso_curve.rs:280` | `natural_interval` | `self.surface` | kernel-free accessor | same; feeds every iso-curve interval |
| `curve/tangent_field.rs:169` | `new` | `generic S: SurfaceEvaluate` | trait has no kernel | S = Surface can be lazy |
| `nurbs/b_spline_curve_n_lib.rs:1238` | `set_intersection_curve_through_point` | `generic S: SurfaceEvaluate` | trait has no kernel | same |
| `surface/surface/p7.rs:243` | `base_evaluate_normal` | `generic S` | trait has no kernel | same (normal at domain limits) |
| `surface/surface/p7.rs:2111` | `compute_higher_order_derivs` | `generic S` | trait has no kernel | same |
| `surface/surface/p7.rs:2185` | `compute_inward_vector` | `generic S` | trait has no kernel | same |
| `surface/surface/p2.rs:3582` | `SurfaceEvaluate for Surface::natural_uv_domain_if_built` | `self` | trait has no kernel | root of the generic cases above |
| `intersect/topology_solver/p1.rs:949` | `natural_uv_domain (trait impl)` | `objects.surface(*surface)` | kernel-free trait method | topology solve operand surfaces |
| `query/brep.rs:994` | `checked_surface` | `surface` | no k parameter (Result fn) | API evaluate entry points on face surfaces |
| `query/face/p2.rs:316` | `natural_uv_domain (trait impl)` | `face surface` | kernel-free trait method | face surface |
| `surface/curve_bounded_surface.rs:305` | `CurveBoundedSurface::natural_uv_domain` | `self.basis` | kernel-free accessor | basis can be an analytic |
| `surface/srf_in_volume.rs:521` | `SrfInVolume::natural_uv_domain` | `self.surface` | kernel-free accessor | inner surface can be an analytic |
| `surface/offset_surface.rs:1960` | `domain_with` | `base` | kernel-free helper | offset over a lazy base (the _with_nurb form exists: OffsetSurface::natural_uv_domain_with_nurb) |
| `surface/surface/p2.rs:3770` | `base_step_uv_domain` | `surface` | kernel-free helper | GetSTEPUVDomain default for every kind |
| `surface/surface/p5.rs:2435` | `surface_operand` | `s` | closure in a non-Result fn, no k | solve operand domain |
| `surface/trim_srf_cache.rs:358` | `TrimSrfCache::new` | `surface` | constructor, no k | trimmed cache of a face surface (domain of the cache) |
| `topology/edgeuse/p3.rs:1037` | `set_create_fail` | `surface` | non-Result diagnostic record | failure record only (low) |
| `topology/face/p1.rs:1033` | `store_surface` | `surface` | no k parameter | face.uv_domain set from a newly stored (possibly lazy) surface |

Not listed: `_if_built` sites whose receiver is provably built (a B-spline surface just
made in `primitive_creation/p3/construct.rs` and `p2/taper.rs`, extended surfaces in
`blend/fillet_corner/p1.rs`, the never-lazy kinds arm of
`Surface::natural_uv_domain_with_nurb`, an offset's extension in `follow_extension_of` and
in `OffsetSurface::select_surface`), and test modules.

Related, not a domain call: `surface/offset_surface.rs` `extend_toward` reads the base's NURB
through the kernel-free `nurb_of`, which returns `SM_ERR_NULL_POINTER` for a lazy analytic
base where upstream's `GetOrCreateGwNurbPointer` would make it. Since main's #419
(`fixes.offset_extension_from_base`), `extend_from_base` takes the kernel and reads the domains
through `_with_nurb`, but still extends through `extend_toward`. S2b-4 can thread `k` into
`extend_toward` / `nurb_of` the same way.

**Plan (S2b-4, a follow-up PR):**
- Thread `k` into the kernel-free helpers and constructors above where every caller has
  one (`IsoCurve`, `TrimSrfCache::new`, `store_surface`, `checked_surface`,
  `domain_with`, `base_step_uv_domain`, the fillet helpers, `set_create_fail`).
- Give `SurfaceEvaluate` a kernel-taking domain method (or pass `k` to the generic
  callers) for the generic paths in `surface/surface/p7.rs`, `curve/tangent_field.rs` and
  `nurbs/b_spline_curve_n_lib.rs`.
- For the kernel-free trait impls of solve operands (`fillet_geom/p1.rs`,
  `topology_solver/p1.rs`, `query/face/p2.rs`) and the CurveBounded/SrfInVolume
  accessors, make the NURB before the operand is built, or carry the kernel in the
  operand.
- Guard the lazy-unsafe analytic base getters reached through `Surface::kind()`
  (`Cone::rev().base()`, `Sphere`/`Torus` `base().base()`, `SurfOfRevolution::base()`),
  which S2b-3 left untouched.
- Each change is checked against the difftest gate (no case movement except cases that
  newly pass).

**PR:** S2b-3 (`fix/s2b3-natural-uv-domain-rename`).
