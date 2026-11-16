# General: WR-464 `offset_missing_surface_fails` cannot diverge with the port's surface kinds (2026-10-31)

- **Flag:** `Kernel::fixes.offset_missing_surface_fails` (WR-464, #1072).
- **Audit:** class B. `offset::offset_executive::face::tests::collapse_thresholds_of_the_analytic_surfaces` reaches the guard on both sides of each collapse threshold; nothing fails forced off.
- **Why:** with the flag, `FaceuseOffset` fails on a missing offset surface unless `Surface::offset_collapses` says the surface collapsed. In the port, `Surface::create_offset_surface` returns no surface only for a cone, cylinder, sphere or torus whose new radius is below the same tolerance `offset_collapses` tests (`surface/cone/p2.rs`, `sphere.rs`, `torus.rs`), or for a surface of revolution under WR-567, whose collapse test `offset_collapses` also uses. So "missing without collapse" never happens, as the record's trigger says ("none in the current surface kinds").
- **Consequence:** no operation can show the fork's behaviour until a surface kind's offset can fail to a missing surface for another reason. The flag is a guard for such code, inert today.
