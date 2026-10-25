# topology.shell WIP: complete native topology snapshots (PR #80)

Adding the actual vertex-use list order exposed an incomplete fixture graph:
the old export followed a wire shell's initial radial edge-use head but did not
follow each vertex's complete vertex-use list. The upstream `my_test_topology`
network therefore had live list members missing from the identity map. Export
emitted its explicit unregistered-pointer sentinel (-3), and Rust import failed.
The expanded run reported 8 shell harness failures and 133 edge-use harness
failures; these are setup failures, not comparable operation results or deferrals.

The snapshot now registers every B-rep edge in the actual global edge-list order
and closes the graph through every vertex-use list. This also retains the edge
order needed by the non-wire branch of GetWireEdges, instead of guessing that a
region-first traversal matches the B-rep edge list. A direct native probe confirms
the network contains 5 edges, 10 edge uses, 10 vertex uses and 4 vertices, with no
unregistered pointer in its snapshot. Native GetWireEdges returns traversal IDs
`0, 2, 4, 1, 3`; the order is intentionally neither sorted nor forced by the
initial edge. No cases, tolerances or pointer checks were removed or weakened.

The complete rerun passes all 144 shell comparisons and restores edge-use p1 to
2,449 passes with 273 genuine deferrals out of 2,722 cases, zero failures. Strict
C++ Clippy, formatting and the three existing kernel regressions pass.
Perturbation detects every compared case in both units. The unit remains
todo/draft while the remaining owned operations and backedges are implemented. Issue #86 coordinates
the actual region lookup context needed by attribute operations; it is separate
from this fixture correction and from the excluded SSI work.
