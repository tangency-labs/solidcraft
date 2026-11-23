Updates: WR-585

#### WR-585 (update, 2026-11-12). `boolean_wire_edge_keeps_faceuse_shell` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch A15a, which retires four topology flags). `make_edge_topology` (`topology/brep/p2.rs`), joining a wire start shell to a faceuse end shell, always moves the wires into the faceuse shell, puts it in the wire shell's place and deletes the wire shell.

- **Tests:** `fork_be8ac73_wire_edge_keeps_faceuse_shell.rs` keeps `fix_keeps_the_faceuse_shell`; `upstream_keeps_the_wire_shell` is gone. The goldens' flag lookup loses its arm.
- **Results:** the batch survey puts the flag in group A, and the full gate at the retiring change confirms it: no goldens row and no kernel rig case changes.
