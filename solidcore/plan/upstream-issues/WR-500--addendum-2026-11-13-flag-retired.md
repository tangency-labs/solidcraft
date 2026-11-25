Updates: WR-500

#### WR-500 (addendum 2026-11-13): a torus always keeps its inside-out flag; the flag is retired (#16250)

- **Status in the port:** `UpstreamFixes::torus_keeps_inside_out` is gone. On `Kernel::default()`, `Torus::new` takes `inside_out` from its generator's bit (`TorusOptions::inside_out` for the circle it makes, the curve's own for `TorusOptions::circle_nurb`), and `Clone` copies it, so a torus and its generating circle always agree (`SmSurfOfRevolution` Test #11).
- **Why:** without the flag, an inside-out torus said it was not inside out, and both STEP writers (`step-export`'s `brep_to_model` and solidcore-step's record writer) wrote its face with the wrong `same_sense`. SolidCraft's fix set had the flag on, so its exports were already right.
- **Removed:** the upstream path where the constructor left the flag `false` and copies reset it, the `keeps_inside_out` field, the preset line in `crates/smlib/src/core.rs` and the `fixes.json` entry.
- **Tests:** the flag-off half of `issue_1490_inside_out_torus_keeps_its_flag` is deleted with the path. Both `same_sense_follows_swapped_and_inside_out_parameterisations` tests run on the default kernel. `an_inside_out_torus_is_written_right_side_out_and_reads_back` (solidcore-step) exports a solid bounded by an inside-out torus and reads it back.
