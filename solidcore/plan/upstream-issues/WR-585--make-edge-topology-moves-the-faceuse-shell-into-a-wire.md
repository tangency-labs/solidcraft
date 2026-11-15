#### WR-585. `MakeEdgeTopology` moves a whole faceuse shell into a wire shell for an edge joining them
- **Severity:** Performance (the size of the body for each such edge; the result's topology is the same)
- **Area:** Booleans / topology (edge construction)
- **Location:** `source/SMLib/src/SmBrep.cpp`, `SmBrep::MakeEdgeTopology`: when the edge's two vertices are in different shells of the region it calls `pEndShell->TransferChildrenTo(pStartShell, NULL)` and deletes the end shell.
- **What happens:** when the start vertex is in a wire shell and the end vertex in a faceuse shell, every face use of the faceuse shell moves into the wire shell, and `SmShell::GetWireEdges` (inside `TransferChildrenTo`) passes over every edge of the B-rep to find the faceuse shell's wires. `SmMerge` makes edges from the other body this way: joining two level-3 Menger sponges made 642 such edges, half of them from the faceuse shell, and moved 3.7 million face uses.
- **Trigger:** a new edge from a wire shell's vertex (start) to a vertex of a faceuse shell (end) in the same region; smallest repro a cube with a wire edge in its infinite region, then an edge from the wire's free end to a cube corner.
- **How found:** SolidCraft. The fork's fix is tangency-labs/smlib@be8ac73 ("booleans: a new edge from a faceuse shell to a wire keeps the faceuse shell"), which is not merged upstream: the faceuse shell takes the wire shell's wires (`pStartShell->TransferChildrenTo(pEndShell, NULL)`) and its place in the region's shell list (`Remove`, `InsertAfter`), and the wire shell is deleted. The shell left holds the same face uses in the same order and the same wires in the same place among the region's shells.
- **Confirmed:** no C++ run was made here. The Rust port follows the pinned C++ (`3fd5b95`).
- **Rust port:** behaves as upstream by default. With `Kernel::fixes.boolean_wire_edge_keeps_faceuse_shell`, `topology::brep::p2::make_edge_topology` keeps the faceuse shell (its `ShellId` survives instead of the wire shell's).

  Test: `crates/solidcore/tests/fork_be8ac73_wire_edge_keeps_faceuse_shell.rs`.
