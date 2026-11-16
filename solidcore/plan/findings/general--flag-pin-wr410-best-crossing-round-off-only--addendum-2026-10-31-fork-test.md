# General: WR-410 addendum: the fork's grazing join diverges once the imprint end fits are on (2026-10-31)

Updates: plan/findings/general--flag-pin-wr410-best-crossing-round-off-only.md

The fork's capi test (c9db18e, `booleans` `grazing_edge_crossing`: a lofted slab leaning on a rod, joined with a pad with r 3 rounds) fails SMLib's check (test 61, an edge end 9.3e-5 from its vertex) in the port by default. With the imprint end fits (`imprint_trims_edge_to_proto_vertex`, `imprint_extends_edge_to_proto_vertex`), as the fork runs them, it fails without `boolean_vertex_by_best_crossing` and is sound with it. Ported as `crates/solidcore/tests/fork_c9db18e_grazing_edge_crossing.rs` (fails with the flag forced off). The hunk does not reverse cleanly on the fork's replay/1.0.1, so the fork side was not reverted.
