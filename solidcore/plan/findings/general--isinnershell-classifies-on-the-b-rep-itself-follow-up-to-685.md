# IsInnerShell classifies on the B-rep itself (follow-up to #685, 2026-10-24)

- **Root cause of the region-nesting mismatches parked by #685.** The disagreements in
  topology.brep_p6 `make_face.json#89..95` were missing UV trim curves.
  - `topology_traverser::is_inner_shell` ran its rays and `GetPointsInFace` on a
    `b.clone()` scratch copy.
  - `topology::face::p2::points_in_face` ran `CalculateUVDomainFromUVTrimCurves` and
    its iso-line `CurveOnClassify` on further copies.
  - Upstream's `const` face queries leave the UV trim curves they create
    (`GetOrCreateUVTrimCurve`) on the B-rep. The C++ export had a stored trim on
    edge uses where Rust had none (two_disjoint_cubes geom.len cpp 1385, rust 1232).
  - Found by logging every `set_uv_trim_curve` with its B-rep address: 15 trims landed
    on the final B-rep and 18 on copies. All 18 came from `is_inner_shell`, through
    `global_line_intersect_held` (10), `points_in_face`'s `classify_curve_on_face` (6)
    and `calculate_uv_domain_from_uv_trim_curves` (2).
- **Fix.**
  - `is_inner_shell`, `points_in_face` and `Shell::is_inner_shell` take
    `&mut Brep` and run on the caller's B-rep.
  - The iso-line classification copies the UV trim curves, raised edge/vertex
    tolerances and gap caches back from its `CurveOnClassifyCache` snapshot
    (`apply_uv_trim_updates`).
  - The brep_p6 geometry and topology now match C++ exactly.
- **Still differs (kept in `known_mismatches.toml`): the Mark2 counter.** Upstream
  takes one more mark per sampled face (two_disjoint_cubes cpp 28, rust 16; one
  cube 14 vs 8).
  - It comes from `SmCurveClassification::IntersectTSBound`'s trimmed-surface-cache
    branch (`SmCurveClass.cpp:16242`, `SmNewMarkAndLock` while walking the cache's
    UV nodes).
  - The port's `query::face::p2::intersect_ts_bound` walks every loop use when the
    caller supplies no trimmed cache. This is the gap recorded under the #639 entry
    above ("IntersectTSBound without the face's trimmed cache").
  - Porting the cached branch needs the face-owned trimmed cache that
    `smsurf_GetSurfaceCache` builds (surface.trim_srf_cache), plus `&mut Kernel`
    through the classification for the mark. That is a separate change.
- **The skin cases.** sweep.api_surfaces `skin_primitive_annular`,
  primitive.primitive_creation_p2 `skin_concentric_loops`, and creation_p2 gen0201
  and gen0214 (which passed before) now go further and defer at
  `TrimSrfCache::build_tree_with_face for a face-owned surface`
  (surface.trim_srf_cache): with the trims kept, a later face query needs the
  face-owned trimmed cache. Their known-mismatch entries are removed.
- **#614's copy-based `classify_face_interval` path is not involved.** Switching the
  `query::face::p2` curve-on-classify between the read-only and mutable interval
  classification changed nothing for these cases (probe recorded in the #685 entry).
  This fix does not change that path.
- **Blocker: topology.topology_traverser (green on main) loses 14 passes.** Running
  `IsInnerShell` on the caller's B-rep turns its op-7 cases (constructed_05..07,
  adversarial_02..06, upstream_files_00/01, gen0001/0003/0012/0013) from pass to
  deferred at `SmFace handle of a surface's face` (topology.face_p2). The backtrace runs
  `is_inner_shell` -> `face::p4::global_line_intersect_held` ->
  `surface::p1::get_surface_cache` -> `make_trim_srf_cache`.
  - On main the scratch copy came from `Brep::clone`, which drops geometry owners
    (open PR #681). The ray's surface looked free, so the port built an untrimmed
    cache. On the caller's B-rep the surface keeps its face owner, and a face-owned
    trimmed cache needs a face handle bound to it (`bind_trimmed_face`), which the
    ray path (`RaySurface` handles) does not supply.
  - The values matched C++ before only because both sides' answers agree for
    these inputs. Upstream builds the face's `SmTrimSrfCache`.
  - Neither copy-back is the cause: putting `CalculateUVDomainFromUVTrimCurves`
    back on a copy, or dropping `apply_uv_trim_updates`, leaves the 14 deferrals.
- **Proposed fix (larger than this change).** Give `global_line_intersect_held` (and
  the ray world of `IsInnerShell`) a face-bound trimmed surface cache, as
  `smsurf_GetSurfaceCache` builds for a face-owned surface. Then port
  `IntersectTSBound`'s cached branch with its `SmNewMarkAndLock`, taking
  `&mut Kernel`. Together these unblock the traverser cases, the skin cases'
  `build_tree_with_face` deferral, and the brep_p6 Mark2 counter. Until then the
  in-place `IsInnerShell` cannot land without turning a green unit partial.
