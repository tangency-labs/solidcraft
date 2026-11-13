Updates: WR-685

#### WR-685 (update, 2026-11-06). `fillet_gn_section_generator` is removed: the G^n section and the generic blend's checks on every kernel

The default kernel now builds rounds and refuses blends as SolidCraft's set already did
(tangency-labs/solidcore#2273). `UpstreamFixes::fillet_gn_section_generator` is removed, and
every read site takes the fix's side:

- **Sections.** A blend-curve round (`SM_FSG_BLEND_CURVE`) is the G^n section, of order 2 to 4
  and thumbweight below 1 or the equal-area default. Another order or thumbweight is invalid
  input, including upstream's G1 and thumbweight 1. `SmApiVariableRadiusFillet` takes the G^n
  default thumbweight.
- **Variable rounds are the union of their balls.** A variable-radius or constant-distance
  solver's circle section is the balls' touch circle. A law's corners are through points of the
  trace, the round is skinned piece by piece between them, and it is built there as the union
  of the balls (the zone at a dip, the crossing at a peak). The cliff overflow's law check uses
  the touch circle.
- **The ease** before a setback blends into the order-2 G^n section at thumbweight 0. Its
  at-thirds blend curve is gone (WR-525's addendum of 2026-11-06).
- **Checks before the corners** (WR-696):
  - `BallIsRegular`: each face encloses the ball and every point of its section is progressive.
  - The collapsed offset.
  - `BandCutsThePart`.
  - The fit sizes the refusals note, which return `SolidError::FilletRefused`.

  `BallFitsItsFaces` and its flag `fillet_refuse_ball_not_fitting` never ran with them, and are
  removed (WR-462's addendum of 2026-11-06).

The G^n section's prerequisites go with it: `fillet_surface_plane_curve_on_plane_equation`
(WR-682) and `fillet_corner_face_curve_halves` (WR-659). The blend curve's dead revolve,
`fillet_blend_curve_revolved_on_arc` (WR-581), goes too. Each has its own addendum of 2026-11-07.

**Results that change, on kernels without the flag** (the default, and any set that left it off):

- **A variable round is its balls' envelope**, not the great-circle sweep:
  - On the 20 cube, the law 6, 6, 1, 1 removes 64.1638, within 6.7e-5 of the envelope's 64.15953
    by quadrature. The great circles removed 65.6068 (`wr646_variable_round_follows_law`).
  - The 2 -> 10 round on the 10 x 20 x 30 box leaves 5831.2451, within 5.5e-5 of the envelope's
    removed volume, where the great circles left 5822.5865 (`wr609_trace_reaches_rails`).
  - The cliff overflows 2 -> 11, 20, 30, 50 leave the fork's G^n volumes (5802.4277...,
    5503.8406..., 5184.0105..., 4693.4880...), where the great circles left up to 1147 less
    (`wr631_cliff_overflow_construction`).
- **A law with corners is built as the union of its balls.**
  - The 20 cube's edge with the law 3, 2, 4, 2, 3 is a valid body. On main it came back with a
    round whose net doubles back on itself (`check_brep` #5); with WR-708's exact circle alone it
    also had coincident control points (#15).
  - 4 -> 2 -> 4 has its two sides and the corner ball's zone: three faces where there was one
    (`fillet_law_face_owned_drop_point`).
  - Regression test: `wr696_default_variable_rounds`.
- **A round whose balls fold is refused** (`NoRoom`, `BallsFold`, naming the radius), with the
  trace reach (WR-609). On the bare default kernel the folded law 2, 2, 40, 3, 2 still fails in
  the round's surface (1001), as before.
- **A ball that does not fit is refused before the corners** (`BallDoesNotFit`). It names the
  same sizes the later checks did: the filleted box's top loop at r > 1 names 1, and the
  chamfers past the R5 rounds name 5. Before, these reported `RailPastAxis`.
- **A blend-curve request** with thumbweight 1 or G1 is invalid input (1007), where the blend
  curve was built.

**Fixed here, with the checks now on every kernel (not the fork's):**
- **Sheets.** `BandCutsThePart` and the break-through check ask whether the blend adds material
  (`IsConcaveEdge`), and that failed (`SM_ERR`) on an edge with no use on a material side. So
  every round on an open sheet failed, on SolidCraft's set too: the goldens' `stubs4` rounds of
  upstream's `test8`, `shell_fillet`, `shell_fillet2`, `sphere_plane`, `coons_01_03_29`,
  `test5` and `open_shell`. A sheet's edge bounds no material, so there's no part to cut and
  nothing to judge (`edge_bounds_material`; `wr696_band_cuts::a_round_on_a_sheet_has_no_part_to_cut`).

**Recorded, not fixed here:**
- The mirrored knife edge's r 2 round under WR-643's flags is refused as a fold where it is not
  one (tangency-labs/solidcore#2963).
- An R 10 cylinder's rim rounded G2 at r 9.99 runs for over 20 minutes on the G^n section, as
  on SolidCraft's set (tangency-labs/solidcore#2964).

**Tests:** those that set the flag drop the setting, and on/off comparisons keep their on half.
The fork ea59e9c's great-circle mark of the variable round is replaced by the default kernel's
round on its cone (`fork_ea59e9c_section_orders`). The tests of the blend curve's own behaviour
are removed with it.
