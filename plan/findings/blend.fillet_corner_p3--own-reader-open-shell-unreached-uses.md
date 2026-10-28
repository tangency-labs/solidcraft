# blend.fillet_corner_p3: the own reader's open_shell.smb keeps unreached edge uses named by a gap cache (2026-10-28)

Unit: `blend.fillet_corner_p3` (harness; the reader is `topology.brep_p7`).

Inputs: every `create` case on `Fillet/open_shell.smb` (31 cases, `open_shell_eu*_m*_*`).

What happens: the difftest Rust side now reads `.smb` sources with the Rust reader
(`crate::own_reader`) and renumbers the result into the shim's export traversal order
(`own_builder::finish`), instead of importing the C++ export. For `open_shell.smb` the
renumbering fails with `renumber: unreached edgeuse`:

- The file has 22 edges, 17 vertices and 33 edge-use pairs (66 edge uses). The shim's
  traversal of the C++ B-rep after `ReadFromFile` reaches 62 edge uses, 21 edges and
  16 vertices (its export counts).
- The Rust B-rep after `read_from_file` still holds all 66 edge uses, 22 edges and 17
  vertices in its arenas. The traversal (`own_builder::walk`, the shim's `walk`) reaches
  62 / 21 / 16, the same counts as the C++ export. The 4 edge uses it does not reach
  (arena 4, 5, 54, 55) all name edge 2. Their `cw`/`ccw` point at reached edge uses, but
  no reached record points back at them.
- A reached edge use (arena 2) has a cached edge-to-CCW-edge gap
  (`m_sEdgeCCWEdgeGap3d`) whose second edge use is arena 4, an unreached one. The
  renumbering maps every edge-use reference, so it cannot place it.

Suspected cause: the read drops edge 2 (a degenerate or redundant edge of the file)
from the loops after the gap was cached. The Rust port unlinks the edge's uses but leaves
their records in the arena. Whether upstream deletes them (which would leave a dangling
pointer in the cached gap) or also leaves them unreached is not visible from the export,
which carries only the gap's `IsInit` flag. Not yet determined: whether this is a Rust
divergence or the same state in both kernels.

Handling: `blend.fillet_corner_p3` keeps this one file on the C++ export import
(`KEPT_ON_IMPORT` in `crates/difftest/src/units/blend_fillet_corner_p3.rs`). All its other
sources use the own reader. To resolve it, settle what upstream does with the dropped
edge's uses, then either delete the records in the Rust reader or have the renumbering
map a reference to an unreached use the way the C++ state does.

PR: difftest own-reader rollout, group E (blend units).
