Updates: WR-501

#### WR-501 (update). The major-radius guard measured through its public reader on standard tori

- **Where the guard is read:** only `Torus::is_nurb_surface_torus` (`crates/solidcore/src/surface/torus.rs`,
  `let major_vec = if k.fixes.torus_major_radius_from_origin { .. }`) reads
  `fixes.torus_major_radius_from_origin`; in library code that recogniser is called only by the
  exact-fillet quadric read (`crates/solidcore/src/blend/fillet_exact.rs`, guarded by
  `fixes.fillet_blend_corner`). No revolve, primitive or check operation calls it, so the flag has
  no operation-level effect; it is observable at the reader itself, which is public.
- **Measured (2026-11-02, commit 470496f29, `cargo +1.98.0`, `RUSTC_WRAPPER`/`RUSTUP_TOOLCHAIN`
  unset, this machine):** the public reader was called on the NURB of tori that public
  constructions built, once with `Kernel::default()` (flag off) and once with only
  `fixes.torus_major_radius_from_origin = true`; every other fix defaults. Reported values by
  construction (major radius, exact bits in brackets; minor, `inside_out`, axis placement, step
  domain and NURB control points compared too):

  | Public construction | Torus R, r | Reader off (upstream) | Reader on |
  |---|---|---|---|
  | `create_torus_surface`, 10 / 3 at the origin | 10, 3 | `Some` 10.0 `[4024000000000000]` | `Some` 10.00000000000000533 `[4024000000000003]` |
  | `create_torus_surface`, 13.1934 / 1.25 at (1, 2, 3) | 13.1934, 1.25 | `Some` 13.19339999999999868 `[402a6305532617c1]` | `Some` 13.19340000000003243 `[402a6305532617d4]` |
  | `create_torus` (B-rep), 5 / 2 | 5, 2 | `Some` 5.00000000000000178 `[4014000000000002]` | `Some` 5.00000000000000355 `[4014000000000004]` |
  | `Torus::create_canonical`, 10 / 3, all four swap/inside-out variants | 10, 3 | `Some` 10.0 `[4024000000000000]` | `Some` 10.00000000000000533 `[4024000000000003]` |
  | `create_rotational_sweep` of one whole circle (issue #1491 profile) | 30, 3 | `Some` 29.99999999999998934 `[403dfffffffffffd]` | `Some` 29.99999999999996092 `[403dfffffffffff5]` |
  | `create_rotational_sweep` of a quarter arc 0..90 | 5, 1 | `Some` 5.00000000000000089 `[4014000000000001]` | `Some` 5.00000000000001510 `[4014000000000011]` |
  | `create_rotational_sweep` of two half circles (torus R 5, r 1) | 5, 1 | `Some` **5.0990195135927845** `[401465655f122ff6]` | `Some` **4.99999999999999467** `[4013fffffffffffa]` |
  | `create_rotational_sweep` of a horn torus (R = r = 14) | 14, 14 | `Some` **19.79898987322332715** `[4033cc8a99af5452]` | `Some` **13.99999999999999289** `[402bfffffffffffc]` |
  | control: `Torus::new` with V domain -90..270 (the #1470 case, already unit-tested) | 13.1934, 1.25 | `Some` 13.25248292057000477 `[402a814570fd4b21]` | `Some` 13.19339999999968605 `[402a630553261711]` |

- **What this shows.** For tori whose NURB tube starts on the equator (the primitive, the canonical
  constructor, the whole-circle revolve, the quarter arc) both readings are the same torus and the
  major radius differs only at ULP scale — 2-19 ULP across the recorded rows (the 13.1934/1.25 row
  spans 19 ULP, the quarter-arc revolve 16, the whole-circle revolve 8, the 10/3 rows 3, the B-rep
  5/2 row 2; ≤ 2.842e-15 relative throughout); every other reported field is
  identical. **A whole-turn revolve of half circles — a public torus construction — builds a**
  torus whose NURB tube runs -90..90 degrees, so the first sweep circle is at the bottom of the
  tube: upstream's radius is `sqrt(R^2 + r^2)` (5.0990195135927845 for R 5, r 1; 19.7989898732233
  for the horn torus R = r = 14) while the constructed face is R (5 and 14). The opt-in reports R
  and the tube interval's start moves from 95.65496623701031 and 112.5 degrees to 90 in the same
  runs. `inside_out` is `false` in both readings everywhere, as the record's fix note predicts.
- **Regression:** `crates/solidcore/tests/wr501_torus_major_radius_from_origin.rs` revolves two
  half circles into two torus faces (R 5, r 1) with `fixes.revolve_analytic_profile_faces`, then
  reads each face's NURB through the public recogniser with the flag off and on: off is
  `sqrt(R^2 + r^2)` and on is R (1e-9 relative), both `Some` with `inside_out` false and the same
  axis. Mutation-checked both ways: with the guard forced on the off half fails (major
  4.999999999999995), with it forced off the on half fails (major 5.0990195135927845); the kernel
  was then restored byte-identically (sha256
  `639be1668e2d3b3701eb820ff19c6b0377a894172d542a816be8be44f8d29363`, no source diff).
- **Scope and limits.** Direct reader level only: no public operation was shown to reach the
  recogniser (the revolve that builds the tori above never calls it), so no operation-level
  difference is claimed. The C++ reference was not rerun here; the upstream reading is the port's
  default path, documented in the record. `torus_signed_gen_curve_interval` and every other fix
  stay at their defaults in the measurement, so the tube interval reported above is upstream's
  unsigned one (a full turn collapses to its start). No tolerance, default input or difftest case
  changed; the opt-in remains off by default.
