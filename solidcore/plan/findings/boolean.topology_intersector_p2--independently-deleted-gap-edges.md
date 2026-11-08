# boolean.topology_intersector_p2: independently deleted gap edges (2026-10-25)

`TopologyIntersector::fix_gaps` carried the pinned C++ condition requiring both
combinations to delete an edge before either relationship was removed. The Rust
arena prevents a use-after-free, but the maps still exposed a deleted edge.
The port now carries tangency-labs/smlib be3fc10: unmate each deleted edge,
removing a directly mated pair together and otherwise removing each actual mate.
The regression covers deletion on either side alone, a directly mated pair,
two unrelated pairs, no deletion, missing relationships, and repeated cleanup.
It fails on the original one-sided branch. This changes only the documented
stale-topology state; no differential case or tolerance is excluded or relaxed.

PR: [#642](https://github.com/tangency-labs/solidcore/pull/642), fixing
[#432](https://github.com/tangency-labs/solidcore/issues/432).
