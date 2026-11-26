# surface.cone_p2: cylinder NURB rebuild preserves its analytic type (#713, 2026-10-24)

PR: https://github.com/tangency-labs/solidcore/pull/724

The following records the original, unconditional fix before the UpstreamFixes
integration on 2026-10-26; the new opt-in validation is recorded in
[the current addendum](surface.cone_p2--cylinder-nurb-rebuild-issue713--addendum-2026-10-26-upstreamfixes.md).

- **Reproducer:** `fixtures/surface.cone_p2/issue713.json`, particularly
  `issue713_reported_swap0_inside0`. Build a real cylinder of radius
  `9.0000000000985`, height 3, sweep [0, 360], then multiply the last generator
  row's homogeneous X/Y coordinates by `9.0000000000757 / 9.0000000000985`,
  leaving Z and weights untouched. The swapped-UV case reaches the same defect.
- **Before:** pinned C++ `c9c5979` and Rust both return success (1000), radius
  `9.0000000000985008`, and semi-angle `-4.3550699142343645e-10` degrees. The
  object is still an `SmCylinder`, whose analytic semi-angle must be zero. A
  standalone C++ probe linked against the prebuilt reference confirms these values.
  The unmodified Rust implementation matches C++ on all 1,594 cases, including
  all 24 new fixtures, with `max_rel_err=0`. See
  [WR-385](../upstream-issues/WR-385--cylinder-rebuild-gets-semi-angle-from-rounded-radii.md).
- **Correction:** only actual cylinder objects use the mean of end-circle radii
  within `EFF_ZERO_SQRT * (1 + bottom_radius)`, and reset their semi-angle to zero.
  Larger radius differences return `SM_ERR` before changing analytic fields.
  Ordinary cones, including shallow ones, retain the original numerical rules.
  The NURB net is preserved. This is the reconstruction rule described in #713;
  it does not change any difftest tolerance or absolute floor.
- **Regression checks:** the original implementation fails both the zero-angle
  test and the tapered-cylinder rejection test. With the correction, all 11
  cone-p2 unit tests pass, including the four new regression tests (both UV
  orders and orientations, three scales, shallow cones, and a 1/256-turn arc
  carrying a stale semi-angle).
- **Reference limitation:** the shim still invokes the original C++ rebuild.
  The new `ScaleLastXy` edit changes only its input geometry; it does not patch,
  filter, or replace the oracle. Intentional differences from this known-buggy
  reference must remain visible; this change cannot be called parity-green.
- **Differential result after the correction (base `6ff988667`):**
  `FAIL surface.cone_p2 cases=1594 passed=1542 failed=52 deferred=0 max_rel_err=1e0`.
  Twenty new fixtures expose the issue (12 radius/angle corrections and eight
  rejected tapers). All four unchanged-cylinder controls still match. The other
  32 cases below are existing generated cylinder rebuilds: each differs only at
  `values.out[5]` (semi-angle), where Rust now returns exactly zero instead of
  retaining the constructor's tiny nonzero angle. No ordinary-cone case differs.
  The unit is `blocked`, following AGENTS.md's parity gate; no case is dropped,
  deferred, or added to the known-mismatch list to conceal the disagreement.

| Existing case (unchanged input/generator) | C++ semi-angle (degrees); Rust = 0 |
|---|---:|
| `gen0048` | `5.225873978694647e-16` |
| `gen0054` | `9.964836485758671e-16` |
| `gen0085` | `6.47525274738241e-16` |
| `gen0093` | `1.2087564401974375e-15` |
| `gen0094` | `-5.853860964539518e-16` |
| `gen0100` | `1.0867760996748623e-15` |
| `gen0102` | `-1.8652161540024257e-15` |
| `gen0109` | `-7.437972048089543e-16` |
| `gen0152` | `2.6994994052578342e-16` |
| `gen0191` | `-1.6695138099233304e-16` |
| `gen0198` | `2.743485941270129e-15` |
| `gen0304` | `-4.801719985629747e-16` |
| `gen0322` | `2.562545516410313e-16` |
| `gen0332` | `-3.533173908217477e-16` |
| `gen0379` | `6.357466543768908e-16` |
| `gen0404` | `3.0409347491618134e-15` |
| `gen0459` | `-2.322129756978726e-16` |
| `gen0545` | `2.324429061150315e-16` |
| `gen0550` | `1.007922203782428e-15` |
| `gen0554` | `-6.149532159122077e-15` |
| `gen0564` | `3.2679436974909365e-15` |
| `gen0595` | `7.745859825732795e-16` |
| `gen0678` | `1.7430297133860378e-15` |
| `gen0753` | `-1.8619649250225428e-15` |
| `gen0793` | `-1.2625176496682728e-15` |
| `gen0794` | `-2.5180536586586683e-16` |
| `gen0851` | `-9.994205749126866e-16` |
| `gen0861` | `1.695420591715487e-15` |
| `gen1074` | `3.7079467877699714e-16` |
| `gen1105` | `8.994208093951507e-16` |
| `gen1160` | `2.3168948145959036e-15` |
| `gen1180` | `2.1704916129566164e-16` |
