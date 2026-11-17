Updates: WR-493

#### WR-493 (update, 2026-11-17). `found_torus_sphere_keep_tested_nurb` is removed

A torus or sphere found in a NURBS now always keeps a copy of the tested NURB (`Torus::is_nurb_surface_torus`, `Sphere::is_nurb_surface_sphere`). The sphere's half-meridian test keeps upstream's `EFF_ZERO` plus the angles' rounding at the points' size (#15122). The fork's `360 * EFF_ZERO` is not taken: `tests/far_sphere_recognised.rs` holds the allowance to that rounding and no more. The polar-curve sweep interval of the 2026-10-31 addendum applies to every found sphere. The fix is now unconditional and the flag is gone. Removed: the field `UpstreamFixes::found_torus_sphere_keep_tested_nurb`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** goldens of the files whose spheres and tori are found while reading move: `corpus.parity`, `topology.*` on `sphere.smb`, `surf_sphere150.smb` and `Fillet/sphere_plane.smb`, `topology.api_files` writes, and fillets on `Fillet/box_w_slot.smb`, `Fillet/sphere_plane.smb` and `Fillet/sphere_diff_3cylinders.smb`. The read faces now carry the file's own NURB and keep the file's UV trims. Before, the trims were dropped and rebuilt on the refit surface with a grown tolerance. That rebuild hid an edge whose tolerance the healer set under its edge/face gap; that is fixed separately (healed edge tolerances cover their gap).
- **Tests:** `tests/wr493_class_proofs.rs` checks four things on the default kernel:
  - every found surface is the file's NURB bit for bit, and the file's trims stay on it (the healer still rebuilds a trim that misses its edge, as on any surface);
  - the `ss_plane_sph` areas equal the area their edge loop bounds on the sphere (Gauss-Bonnet line integral);
  - `Fillet/sphere_plane.smb` is closer to its closed-form area;
  - a point 1e-7 outside the sphere's pole classifies into the infinite region.

  `tests/wr493_corner_sphere_polar_domain.rs` checks the corner sphere's volume in closed form. The `surface.torus` and `surface.sphere` unit tests (`issue_1433_*`, `wr493_*`) run on the default kernel.
