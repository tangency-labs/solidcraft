Updates: WR-409

#### WR-409 (update, 2026-11-12). `sheet_sweep_joins_profile_wires` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch B43, which retires `sheet_sweep_joins_profile_wires` and `sweep_frame_on_planar_path_normal`). `join_profile_wire_ends` (`primitive/primitive_creation/p3/sweeps.rs`) always glues an open profile's wire ends that meet before a partial revolve or a repeated linear sweep.

- **Tests:** `issue_1113_open_chain_revolves_into_a_joined_sheet` runs on `Kernel::default()` (two faces, 6 vertices, 7 edges, the faces' areas 14 pi and 16 pi to 1e-8). New: `issue_1463_drilled_hole_profile_revolves_part_way_into_a_joined_sheet` (same file), the goldens cases `issue1463_drilled_hole_{pz,mz}_120`: valid at Level 2, 2 faces, 6 edges, 5 vertices, both faces cones, areas 16 pi / 3 and 2 pi sqrt 5 / 3 to 1e-8.
- **Results:** the goldens rows B43 moves, with their old and new values:
  `plan/findings/sweep.api_brep--b43-two-flags-retired-2026-11-12.md`.
