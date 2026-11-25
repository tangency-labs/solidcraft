Updates: plan/findings/offset.offset_executive_p2--issue-1453-cylinder-rim-split-vertex-differs.md

# offset.offset_executive_p2: the #1453 rim split vertex difference is ND-2 (the order of an unordered_map walk), not a port divergence (2026-10-30)

**Correction.** The selector `{"origin": "source", "index": 1}` names the **bottom** cap
(z = 0) on both sides, not the top: a cylinder's faces are `[cylinder, bottom plane, top
plane]` in Rust and C++ alike. The open rim and its split vertex lie at z = 0. Opening the
top (index 2) gives the same difference mirrored to z = 10.

**Cause.** Both sides split the inner rim circle at the same two points, in the same order:
`SmBrep::MakeVertexSplitEdge` runs for 135 degrees first and then for 225 degrees. This was
checked with an instrumented, out-of-tree copy of upstream `SmBrep.cpp` linked ahead of the
reference library. `SmMerge::BooleanPostProcess` then walks `m_sIntersectionVertices`,
which `SmMerge::ManifoldBoolean` fills from `m_vTI.GetMappedBrepObjects(SmVertex_TYPE, ..)`.
That call iterates the pointer-keyed `std::unordered_map` `m_pOToB`
(plan/upstream-issues ND-2). The C++ walk visits [225, 135, 0 degrees]. The port lists the
map in the order of its handles (`TopologyIntersector::mapped_brep_objects`, by design for
ND-2), so it visits [0, 135, 225 degrees]. The first vertex visited is topological and
`DeleteTopologicalVertex` removes it. The merged edge then has a different parameter at the
other split vertex, so that vertex is no longer topological and stays.

| | deleted | kept (the `values.result.geometry` vertex) |
|---|---|---|
| C++ | 225 degrees | (-2.8284215886295061, 2.8284294867261224, -1.5025837514564226e-17) |
| Rust | 135 degrees | (-2.828433304880372, -2.8284192630706975, 4.9267255274271454e-17) |

The 4 differing values of 21 (`geometry[1]`, `[2]`, `[3]`, `[5]`) are the bounds of this
one vertex. Topology, the split parameters (0.37499978932965844 against
0.37499978932965805) and every other vertex agree.

**Reproducibility.** For this input the C++ order is the reverse of the insertion order.
That is libstdc++'s iteration order for pointer keys when no buckets collide and no rehash
reorders the list. In general the order depends on heap addresses and the rehash history.
The order held steady under the default allocator and four `GLIBC_TUNABLES`/`MALLOC_*`
settings, but it is not a function of the input, so the port cannot reproduce it
faithfully. Visiting in reverse handle order would match this case only by imitating the
allocator, and it would change the post-process order of every boolean.

**Status.** Not a port divergence. The case stays out of the difftest runner, and there is
no known-mismatch entry. With `Kernel::fixes.offset_analytic_mate_edges` the rim is one
exact circle and no split vertex exists (`crates/solidcore/tests/issue_1453_shelled_cylinder_exact_edges.rs`).
