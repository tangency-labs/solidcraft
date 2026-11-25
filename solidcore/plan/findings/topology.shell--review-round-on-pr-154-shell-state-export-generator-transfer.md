# topology.shell: review round on PR #154 (shell state export, generator, transfer fixtures)

- **Shared shell state.** Every exporter (`sc_topology_fixture.h`, and the private
  `export_graph` of the `topology.vertex`, `topology.edge`, `topology.face_p2`, `heal.gap` and
  `query.face_p2` shims) now appends `sc_topology_fixture::append_shell_state` (version 3: infinite region,
  region void flags, wire/vertex shell heads, vertex-use lists, face-use next/last links), read
  from the native records. The four Rust importers read it through
  `topology_vertexuse::read_shell_state`; `set_shell_heads` and the arena-order vertex-use lists
  are gone. Their B-reps therefore carry the native `infinite_region` and its void flag (natively
  `SetIsVoid(TRUE)` for the infinite region; `Region::new_with_infinite_region` of #130 uses 1).
  Preconditions that made the exporter fail in #80: every walk now registers the vertex-use lists
  (`walk(.., include_vertex_lists = true)`, and the same closure in the private walks) so
  `identity()` never meets an unregistered vertex use. All ten affected units (`query.face_p2` imports through `topology_face_p2::import`) were re-run with
  unchanged case, pass and deferred counts, except `topology.edge`: the GetEdges-first registration
  changes which edge an edge selector picks, and 5 cases moved from deferred (`topology.edgeuse_p3` 67
  -> 62) to passing (1710/77 -> 1715/72); 0 failures, `--perturb` SENSITIVE everywhere.
- **`SmBrep::GetEdges` order.** `topology::shell::brep_get_edges` (co-port, `topology.brep_p1`) is
  the edge arena in insertion order, which equals `m_pEdgeListHead` order only if the importer
  registered `GetEdges` first. All five walks now do (`brep->GetEdges` before anything else), so
  the arena order of imported B-reps is the native order and `remove_shell` ->
  `transfer_children(.., None)` -> `wire_edges` picks the same Unknown-recipient head as C++. Edge
  indices in those units' fixtures therefore select different (still identical on both sides)
  edges than before.
- **Generator and fixtures.** `topology.shell`'s generator draws every operation (query, wires,
  `TransferChildrenTo`, type members, ray, degeneracy, region order and filter) over vertex, face,
  network, `.smb` and the new `mixed` native fixture (`5 has_face [face] networks`: an optional
  face plus 0..3 wire networks in one B-rep), with random shell pairs, NULL and duplicate wire
  candidates, `pre` transfers and arbitrary `SM_TYPE` codes (all 64-bit patterns). 126 new fixture
  cases (`fixtures/topology.shell/transfer.json`, `mixed_*`) cover face -> wire (the wire
  recipient becomes a face-use shell, wire edge uses stay), wire -> face, wire -> wire (two wire
  shells, candidate subsets and duplicates), face-use shell holding wires -> empty (via `pre`),
  candidate filtering on a face-use source, chained transfers, and 9 rejected inputs. Probe
  (`--seeds 3000`): 3895 cases, 0 failed, no crash, no new undefined behaviour.
- **Transfer shim guard (reviewer note 11).** `sc_topology_shell_transfer` rejects (SC_ERR_ARG; both
  sides then return `rejected_input`) exactly what dereferences NULL: a candidate that is NULL or
  has no primary edge use when the branch reads it, and `GetWireEdges` (no candidate list, or the
  final per-shell snapshot) on a wire shell without list head (`GetWireEdge()` NULL). That last
  state cannot be built with native constructors, so it has no regression fixture (the other
  guard has the 9 `*_null_candidate` cases).
- **RayFire is compared on the empty-shape branch only (harness-limited).** `EmptyRayWorld`
  represents no geometry, so every compared ray case takes the no-solution branch (144
  `ray_empty_boundary`, 4 Endcap). Solution selection, negative / too-close parameter branches,
  `LoadFromIntersection` and the line-segment + `LocalCurveSector` path are not compared. The 8
  `ray_closed_*` cases need `topology::brep_cache::BrepObjects` over a built `BrepCache`
  (`query.brep`) and `SmBrep::LocalCurveSector` (`topology.brep_p2`); until then
  `EmptyRayWorld::shell_ray_shape` defers any non-empty boundary-face shape as
  `UNPORTED(query.brep)`, so they stay deferred instead of failing when `query.face_p1` lands.
  Whoever ports `query.brep` / `topology.brep_p2` replaces `EmptyRayWorld` by a `BrepObjects` world.
- **Mark counters (review round 2).** `SmShell::GetWireEdges`, `GetMatedShells` and `RayFire` take
  `SmNewMarkAndLock`, which advances and locks a context mark; the Rust functions do the same
  (`geom::context::with_mark_lock`, `ALL_MARKS`) and take `&mut Kernel`. The difftest shim compares
  the advance of the 5 context mark counters per call (the shim context is shared, so deltas) and
  that no mark stays locked. `SmShape::SmShape` also locks a mark for non-empty inputs: that part is
  `geom.object`'s (`Shape::new` is Unported for non-empty shapes), so RayFire's real path will have
  to be re-checked for the combined counter advance when it lands.
- **Void / infinite-region branches of `geom.attribute` / `geom.a_object`.** `geom.attribute` now
  covers them (ops `SetIsVoid` and `MakeInfinite` in `geom_attribute.cpp`, mirrored by
  `RegionScene::set_is_void` / `make_infinite`; mutating the INFINITE arm of `add_region` fails
  generated cases). `geom_a_object.cpp` still never calls `SmRegion::SetIsVoid`, so
  `other_solid = false` (a_object.rs; `SmAObject.cpp:310`) is compared only on the solid side;
  `geom.a_object` is `partial` (131 deferred on `query.brep`), not promoted.
- **Library region lookup.** `RegionLookup` has no library implementation; region handlers reached
  without a context return `Unported` (`topology.shell`, with `justified:`). Both sites are
  tagged `REGION-LOOKUP(#86)` (grep it) and justified by the "borrowed region context" section.
