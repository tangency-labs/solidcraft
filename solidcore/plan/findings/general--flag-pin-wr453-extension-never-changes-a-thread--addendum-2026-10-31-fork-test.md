# General: WR-453 addendum: the extension matters in the fork's grazing join (2026-10-31)

Updates: plan/findings/general--flag-pin-wr453-extension-never-changes-a-thread.md

On the fork (replay/1.0.1 0f7b209, built from source), disabling `ExtendEndToProtoVertex` leaves its internal M10 thread test passing, but fails `booleans` `grazing_edge_crossing` (not sound). The port shows the same thing. With the best crossing and the interval trim, the join fails the check without `imprint_extends_edge_to_proto_vertex` (an edge end 9.3e-5 from its vertex), and is sound with every edge end within 1e-5 of its vertex with it. Added to `crates/solidcore/tests/fork_c9db18e_grazing_edge_crossing.rs` (fails with the flag forced off). The fork's M10 nut minus its screw-swept groove, read into the port, gives vertex tolerances of 1e-5 either way.
