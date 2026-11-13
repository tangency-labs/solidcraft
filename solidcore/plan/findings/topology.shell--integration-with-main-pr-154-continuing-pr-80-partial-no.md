# topology.shell: integration with main (PR #154, continuing PR #80; partial, no disagreements)

- **Owners on main.** `topology.vertex`, `topology.edge` and `topology.topology_traverser` landed
  while #80 was open. The shell's private copies of `SmVertex::GetEdges`, `SmEdge::GetVertices` /
  `GetEdgeuses` / `IsWire` were replaced by calls to `topology::vertex::edges` and
  `topology::edge::{vertices, edgeuses, is_wire}` (no filters, as upstream calls them). A NULL
  vertex returned by `GetVertices` is a NULL dereference in `GetWireEdges` (`IsMarked`): error.
  `Shell::is_inner_shell` calls `topology_traverser::is_inner_shell` (it needs `&mut Kernel`, so
  `order_region_shells` takes one); its closed-shell cases now defer on `topology.face_p1`
  (`points_in_face`) instead of on the traverser.
- **Stubs wired.** `mass::integrator` `SmRegion::IsVoid` (`is_void != 0`, so `UNSURE` counts as
  void, as the C truth test), `topology_traverser::remove_shell` (`SER(TransferChildrenTo(.., NULL))`)
  and `faceuse_shell_replacement` (`Remove` / `PostInsert`, statuses ignored upstream; they cannot
  fail on a consistent B-rep, so `shell::move_faceuse` reports an inconsistent one as an error).
  The 7 traverser cases deferred on `topology.shell` now pass.
- **Harness.** The `sc_topology_fixture.h`-style exports (vertexuse, vertex, edge, face_p2 shims,
  and heal.gap's copy) already carried each region's shell list and each shell's type and face-use
  list; the Rust importers dropped them. They now fill `Region::shells`, `Shell::shell_type` and
  `Shell::members`. (First version of this PR inferred the wire/vertex shell heads from arena
  order and left the void flags and the infinite region unset; superseded by the review round
  below.)
