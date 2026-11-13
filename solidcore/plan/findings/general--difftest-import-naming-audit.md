# General: naming of B-reps the difftest runners import from C++ exports (2026-10-27)

**Why:** the heal.stitch_p1/p2 runners imported a C++ B-rep whose naming stayed
`NamingScheme::Unknown` with no geometry owner keys, while the region split under the
glue (`SplitClosed` -> `FindAndSetInfiniteRegion` -> `SmShell::RayFire`) names geometry
under notification identities overridden by recorded owners. Every shell ray failed with
an internal owner-lookup error, `find_infinite_region` treated that as a missed ray, and
the code took another path (22 mismatches; fixed in PR #1306 by calling
`Brep::use_arena_naming()` after the import,
`plan/findings/heal.stitch_p2--region-nesting-ray-fire-raises-other-edge-tolerances--addendum-2026-10-27-resolved.md`).

**Audit (main 870ea65b, reference 3fd5b95).** 53 runners that build their Rust B-rep from
a C++ export: blend.fillet_corner_p1..p3, blend.fillet_executive_p1..p3, blend.fillet_geom_p3/p6,
blend.fillet_solver_p1/p2/p4/p5, blend.fillet_standard_solver_p1/p2, blend.hcr,
boolean.merge_p1..p5, boolean.topology_intersector_p1/p3, geom.tools, heal.gap,
heal.heal_data_p2, heal.stitch_p1/p2, intersect.curve_class_p6, query.api_queries,
query.edge, query.face_p1, query.face_p4, surface.trim_srf_cache, topology.brep_p1/p4/p6/p7/p9,
topology.brep_validation, topology.edge, topology.edgeuse_p1..p3, topology.face_p1..p4,
topology.loopuse, topology.shell, topology.topology_traverser, topology.trimming_tools,
topology.vertex, topology.vertexuse.

Already recorded a naming: boolean.merge_p3 and the fillet end-to-end cases
(`use_arena_naming`), topology.brep_validation (`Notify`, with the exported owners),
topology.trimming_tools (per case).

Method: each run twice on the same build, once as is and once with a temporary probe that
called `use_arena_naming()` on every import whose naming was still `Unknown` (at the shared
importers: topology_vertexuse, topology_edgeuse_p1, topology_vertex, topology_edge,
topology_face_p2, query_api_queries), with temporary counters of the B-rep cache's
"edge/face owner is not in source B-rep" errors and of every error `find_infinite_region`
took as a failed ray.

Results:
- Owner-lookup errors: 0 in every runner, both runs. On main's fixtures no runner reaches
  a by-name owner lookup with a mismatched naming (the heal.stitch cases that did are the
  coverage cases PR #1297 adds).
- Failed rays: only boolean.merge_p2, 6, all `SM_ERR` (1001) from topology::brep::p2: a
  native status, retried as upstream.
- Outcomes: identical in 52 runners (same counts, same values). topology.edge changes:
  20 `set_curve` keep-old-curve fixtures (`line_*_fw_set_curve_keep`) fail at
  `values.state[20]` (C++ 1, Rust 0). That runner names its geometry with its own
  `key_of` identities and passes them to the library through its environment, so arena
  naming is not its import's naming; under `Arena`,
  `topology::edgeuse::p1::owner_ref` gives edge-use geometry an arena owner where it
  gives none under `Unknown`/`Caller` (suspected cause of the changed state flag). Its
  `Unknown` naming is correct and is left unchanged.

**Change (PR #1314).** The imports that set every owner to its arena identity now record
that naming: `topology_brep_p1::import_raw` (heal.stitch_p1/p2, geom.tools,
blend.fillet_executive_p2/p3, intersect.curve_class_p6, topology.brep_p1),
`boolean_merge_p4::import` (after `own_geometry`) and `query_api_queries::imported`. No
outcome changes. The other importers name surfaces under their own keys (face type,
`1 + index`), which is neither the arena nor the notification identity, so arena naming
would not describe them; they stay `Unknown`.

**Library.** `find_infinite_region` (topology::brep::p9) still retries any failed
`SmShell::RayFire` as upstream, but now propagates errors that have no upstream status
(unported, stale id, pending preparation, and the B-rep cache's own owner/handle
resolution errors) instead of treating them as a missed ray, so a naming mismatch shows up
as an error rather than as another region.
