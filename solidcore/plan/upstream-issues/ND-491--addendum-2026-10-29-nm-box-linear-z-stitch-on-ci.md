Updates: ND-491

#### ND-491 (update, 2026-10-29). `nm_box_linear_z_stitch` returned `SM_ERR` on main CI
- **Trigger:** `sweep.topology_sweep` case `nm_box_linear_z_stitch`, the body of the 2026-10-27 addendum
  (`ND-491--addendum-2026-10-27-box-with-wire-stitch.md`).
- **Observed:** main CI run 40862396783 at 881b040f9 compared `values.points.len` cpp=0 rust=264: the
  C++ side returned `SM_ERR`. The next main run (40865582988, bbdc9a8d1) passed with the same reference build.
  This is the 7/1000 outcome the 2026-10-27 addendum measured, seen under a CI process's heap layout.
- **Rust:** the port gives the C++ majority result (3 regions, 15 faces, 33 edges, 22 vertices).
- **Status:** the case is parked like its seven siblings (`cpp_defect = "ND-491"` in
  `crates/difftest/known_mismatches/sweep.topology_sweep.toml`), so main's CI no longer depends on the
  heap layout. Removing the marker runs it again.
