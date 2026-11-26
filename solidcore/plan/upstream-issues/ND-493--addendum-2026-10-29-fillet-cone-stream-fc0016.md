Updates: ND-493

#### ND-493 (update, 2026-10-29). A second case: generated `fc0016` of `topology.brep_local_operation`

- **Case:** `fc0016` of the `<unit>:fillet_cone` seed stream (seed 12757803558688034937):
  `Solids/nurb_fillet.smb` with faces 17, 10 and 5 (`GetFaces` order) replaced by planes
  through their mid-UV points at offset 0, no tilt. Face 5 at offset 0 is the case already
  parked under ND-493 (`nurb_fillet_face5_same`).
- **What happens:** the C++ result's entity count changes from run to run. The graph's
  geometry block (vertex use UVs, vertex points and tolerances, face and edge data) has 255
  values in most runs, equal to the Rust result, and 765 in others: the stitch keeps three
  times the entities when the clock-seeded inner-shell test decides differently.
- **How found:** PR #1596's `difftest-sweep` failed on `fc0016` with `cpp=765 rust=255`.
  Then 10 runs of `--unit topology.brep_local_operation --filter fc0016`, each in a fresh
  process, 1 s apart, on `origin/difftest/blo-fillet-cone-uv-coverage`: 9 passed (C++ and
  Rust agree at 255), 1 failed with C++ 765 and Rust 255. The Rust side (seeded
  `kernel.rng`) gave 255 in all 10.
- **Confirmed:** yes, nondeterminism of the C++ reference on this input (the run-to-run
  variation above); same path and mechanism as the original entry.
- **Rust port:** unchanged. The difftest parks `fc0016` with `cpp_defect = "ND-493"`
  (`crates/difftest/known_mismatches/topology.brep_local_operation.toml`).
