# boolean.merge_p3: shell vertex on a box edge finds no edge solution: resolved (2026-10-25)

Root cause of the entry above: a difftest runner input defect, not branch culling. The
runner's operand loader (`crates/difftest/src/units/boolean_merge_p3.rs`, `Pool::load`)
imports the C++-exported box through the vertexuse importer, which carries no owner for
edge curves, and only re-keyed the face surfaces. `BrepCache::build_trees` adds a curve
to the curve tree only when `curve.owner()` is its edge (upstream `SmBrepCache::BuildTrees`,
`pCurve->GetOwner() == pE`), so the Rust curve tree was empty (an instrumented run showed
its root node, the `(-1.501326..11.501326)^3` box from BuildTrees' 15% expansion, with no
objects and no children) and the PointEdge pass of `BrepPointSolve` had nothing to solve.
In C++ each `SmEdge` owns its curve. The loader now sets each edge curve's owner to its
edge (`arena_identity(CacheObject::Edge(e))`), as `boolean_merge_p4.rs` `own_geometry`
already did; both cases pass (13 edges, the edge `(0,0,0)-(10,0,0)` split at `(5,0,0)`)
and leave `known_mismatches.toml`. No library code changed.
