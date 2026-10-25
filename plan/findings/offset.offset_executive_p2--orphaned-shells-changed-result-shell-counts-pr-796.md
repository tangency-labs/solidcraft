# offset.offset_executive_p2 / topology.brep_p2 / topology.brep_p7: orphaned shells changed result shell counts (PR #796)

Wiring `SmOffsetExecutive::PiecewiseMerge` to `SmMerge::PiecewiseMerge` made the offset
results comparable, and ten sheet/round cases then disagreed only in the result's shell
count (C++ 1, Rust 4). `SmBrep::MakeEdgeTopology` (copying a face's wire edges between
shell vertices) and `SmBrep::GlueEdgesGeneral` `delete` the shell they merge away; the
ports only unlinked it, so every merged vertex shell stayed in the arena as an empty,
region-less record that `Brep::shells().len()` counts. Both now destroy it
(`topology::shell::destroy`: `SM_NO_DESTRUCTION`, record, attribute uses). The other
unlinked-only deletes (a deleted vertex shell's vertex use, edge uses, regions in p7)
still stay as orphans; any runner that counts arena records instead of traversing the
topology will see them. No other unit's difftest counts changed.
