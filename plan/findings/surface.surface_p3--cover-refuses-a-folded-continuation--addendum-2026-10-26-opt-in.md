# surface.surface_p3: the fold refusal is opt-in; the seven cases match C++ again (2026-10-26)

Updates: plan/findings/surface.surface_p3--cover-refuses-a-folded-continuation.md

- The fold refusal now runs only with `Kernel::fixes.cover_refuses_folded_continuation`
  (and the sweep-frame half only with `Kernel::fixes.sweep_frame_on_planar_path_normal`),
  both off by default (plan/upstream-issues/WR-407 addendum of 2026-10-26).
- With the default kernel the seven generated cases (gen0041, gen1448, gen1514, gen1567,
  gen2066, gen2303, gen2376) cover as C++ does and pass at 1e-9: their known-mismatch
  entries (`crates/difftest/known_mismatches/surface.surface_p3.toml`) and the seven
  ratchet waivers in `plan/difftest-waivers.toml` are removed, and the unit is green
  again.
- The divergence described in the original entry is what a kernel with the field on
  gives; difftest runs the default.
