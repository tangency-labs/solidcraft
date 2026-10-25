# topology.shell: native region ordering and filtering (PR #80)

46 ordering cases compare real circular shell lists after inserting actual empty
shells at the head. Gathering complete face-use shells with native owner-list
operations covers the multiple-shell branch without splitting their geometry.
Four closed-shell cases reach SmTopologyTraverser::IsInnerShell; they deferred on
topology.topology_traverser in PR #80 and, since PR #154, on topology.face_p1 (see
"integration with main" below); native reference setup and calls still run.

168 filtering cases use real SmLongAttribute objects and AttributeStore entries,
testing no/even/all tags, NULL versus present filters, and ids 0, 9999, 2^53+1 and
u64::MAX through a uint64_t C ABI. Ordering outputs compare actual integer lists.
The complete unit has 785 cases: 767 exact comparisons, zero failures, 18 genuine
callee deferrals. All 767 compared cases detect perturbation. No tolerance changes.
The other seven affected units retain zero failures and full sensitivity before
the final official-main integration. Final integration gates are recorded in the PR.

Merged PR #88 supplies the real SmEdge::IsWire coport; the shell helper now
delegates to it. Native vertex-use ordering remains in Vertex::vertexuses for
shell traversal; PR #88's cache-reset scans keep their existing behavior.
