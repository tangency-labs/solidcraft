Updates: WR-703

#### WR-703 (addendum 2026-11-06): keep features is the default; its flag is retired (#2426)

- **Status in the port:** `UpstreamFixes::fillet_keep_features` is gone. A boss or cut inside a round's band is kept on `Kernel::default()`, with the walls swept in a body of their own (`sweep_walls`). The fork still sweeps them in the sheet.
- **Removed:** the path where the round's merge dropped the feature. No application reached it: SolidCraft's fix set had the flag on. Letting a round take a feature is a future option (`docs/generic-blend.md`).
- **Tests:** the keep-off cases of `crates/solidcore/tests/fork_eb883ad_keep_features.rs` and `issue_2404_build_near_size_bound.rs` are deleted with the path; `issue_2370_keep_features_band_clear_of_boss.rs` now checks the closed form alone, as there is no keep-off body to compare with.
