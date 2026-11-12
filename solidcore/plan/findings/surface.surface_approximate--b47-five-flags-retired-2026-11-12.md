# Five surface fixes become the only path: 133 golden rows in ten units (2026-11-12)

- Flags retired (#14246 batch B47): `approximate_surface_bezier_starter` (WR-573),
  `approximate_surface_checks_every_row` (WR-726), `extrusion_step_uses_clamped_uv` (WR-61),
  `revolution_frame_from_profile_origin` (WR-738), `revolution_offset_axis_both_sides` (WR-343).
  The addenda dated 2026-11-12 under `plan/upstream-issues/` name each retirement.
- All units at the fix (`cargo run --profile goldens -p goldens -- --all --goldens`): 283 units,
  273 pass; ten units move 133 rows, added 1 (`surface.surface_approximate/gen0381`, which leaves
  `known_failing.json`), removed 0, broken 0.
- Which flag moves which row: main's code with one flag switched on at a time (a temporary patch
  to `Kernel::new`, not committed), the ten units each time. The rows add up: WR-573 42, WR-726 31
  (plus gen0381), WR-61 9, WR-738 36, WR-343 14. No row moves under two flags.
- Rows that bless rewrote only within the goldens' tolerance (`1e-9` relative) keep their old
  text, so the diff holds the 133 moved rows only.
- Run time: `surface.surface_approximate` takes 250 s at the fix, 135 s on main (WR-573 alone 220 s,
  WR-726 alone 128 s): the starters main returned no surface for are now fitted, pass by pass.

## Why the new rows are right

### `surface.surface_approximate` (74 rows: WR-573 42, WR-726 32 with gen0381)

A row records the status, the achieved tolerance and the surface. A temporary test (not committed)
ran each moved case at the fix and printed its status and achieved tolerance next to the requested
one (`approx_tol3d`: `USE_DEFAULT` = 9876543.21 is 5e-6, a negative tolerance counts by its size).
In all 74 rows either a surface comes back with an achieved tolerance at or below the requested
one, or the call fails with `SM_ERR` and an achieved tolerance above it.

| rows | flag | old | new |
|---|---|---|---|
| 18 | WR-573 | success, no surface | surface within tolerance |
| 24 | WR-573 | success, no surface | `SM_ERR`, tolerance missed |
| 12 | WR-726 | surface | finer surface within tolerance |
| 19 | WR-726 | surface | `SM_ERR`, tolerance missed |
| 1 | WR-726 | known failing (gen0381) | `SM_ERR`, 0.546 for 1.38e-3 |

- WR-573: these starters have simple inner knots (the fixtures `simple_knot_cubic_patch_index`,
  `offset_simple_knot_cubic`, `issue1501_offset_wave_simple_knots` and 39 generated ones). Main's
  patches ran past the control net and it returned success with no surface and an achieved
  tolerance of -1 (nothing tested). Where the search then reaches the tolerance there is a surface;
  where its last pass misses it, `SM_ERR`.
- WR-726: main accepted a pass when its last row of patches was within the tolerance, so it
  returned surfaces whose earlier rows were not. The quarter cylinder fixtures (issue #1441's
  offset quarter cylinder and its wrapped copies) now give a finer net (204 numbers instead of
  184) at 8.99e-4 for 1e-3; `approximates_an_offset_quarter_cylinder` checks the distance to the
  offset surface.

### `surface.surf_of_extrusion_p1` (9 rows, WR-61)

Every moved case evaluates a parameter outside the surface's domain with out-of-bounds evaluation
off (`review4_*_oob0_*`, `review_*_eval_out`, `gen0404`). The evaluators now clamp to the domain:
for `review4_line_oob0_eval0` (line (0, 0, 0)-(1, 2, 3) swept along z, u = -20, v = 2) the point is
(0, 0, 2), the start of the line lifted by 2, where main extrapolated to u = -20. The unit tests
`step_evaluators_clamp_to_the_surface_domain` and `step_clamp_reads_the_analytic_domain_with_out_of_bounds_off`
check the clamped values exactly.

### Surfaces of revolution: the frame (36 rows, WR-738)

Rows of `surf_of_revolution_p1` (7), `_p2` (8), `_p3` (15), `blend.fillet_solver_p4` (2),
`surface.offset_surface` (1), `surface.cone_p1` (2) and `surface.surface_point_solve_p3` (1).
All are `create_canonical` surfaces whose profile interval holds parameter 0 (an arc on
[-146°, 122°]), or whose reference point is near the axis by angle but not by distance. The
generator and the axis are unchanged, so the surface is the same point set; only the frame's
X axis, where u = 0, turns. The rows record parameters, NURB nets and iso curves measured from it.

- `to_nurbs` rows whose second coordinate moves (`surf_of_revolution_p3` gen0637, gen0650, gen0808)
  have `swap_uv`: their second coordinate is the angle.
- `blend.fillet_solver_p4` gen0189 and gen0281 solve a fillet point on revolutions of lines at
  coordinates near 1e6, given by surface parameters. In each, one line starts on the axis to
  rounding (3.3e-11 and 5.8e-11 from it, 5.5e5 and 8.1e5 from the axis point). Main's test (within 1° of
  the axis direction) calls it on the axis and turns to the 45% point; WR-738's test
  (`|Z x (P - O)|` below `SM_EFF_ZERO` = 1e-12, an absolute bound) does not, so the frame's X axis
  points along the rounding residue. Every X axis perpendicular to the axis gives the same full
  turn, so the surfaces are the same; the given parameters name other points of them. A bound
  relative to the coordinates would keep main's frame here; that is outside this retirement.
- Check on `surf_of_revolution_p1/gen0052` (`to_step`): the angle moves from 180.84860962° to
  171.10481566°, the profile parameter stays at -93.2734222°. The X axes of main (towards the arc's
  start, -145.97°) and of the fix (towards its point at 0°) are 9.74379395938° apart about the axis;
  the angle moved by 9.74379395937°.
- `surface.cone_p1` (`iso_circle`, 2 rows): the reference shim refuses the op (`-1002`) when its
  STEP inversion of the point fails. The inversion now solves both sides of the axis, so the op
  runs. `review2_std_bnd_neg_h_oob_icb8` builds the circle at height 0 (radius 1; tolerance 1);
  `review2_std_semi_unb_120_oob_ic9` refuses with `SM_ERR`: its point (0, -2, 2) is 2 from the axis
  where the cone's radius (`Cone::radius`, 1 + h tan 120° at h = 2) is -2.46, so it lies 0.46 off
  the surface (tolerance 1e-6).
- `surface.surface_point_solve_p3/matrix_1143`: the solved point is the same to 2e-16; its residual
  moves from 6.6486136e-10 to 6.6486168e-10 (3e-16 absolute).

### Surfaces of revolution: the offset at the axis (14 rows, WR-343)

Rows of `surf_of_revolution_p2` (11 `offset` ops, among them the review cases
`review_offset_axis_crossing_merge` and `review_circle_touch_axis_offset`), `surface.offset_surface`
(`gen1740`) and `offset.offset_geometry_creation` (`gen0330`, `gen0492`, offsets of the torus
fixtures). Their offset generators cross the axis below the frame's origin, where main's axis
segment (from the origin along +z only) did not reach: main kept a stretch of negative radius,
the fix cuts it at the axis and keeps the generatrix's side. `issue_973_offset_cut_at_axis_below_origin`
checks all four end points of its two offset generators against the closed form.

![The meridian plane of the issue 973 surface: the axis, the frame origin on it, the generator from (0, 0, -1) to (4, 0, -5), its offset away from the axis, and its offset towards the axis, which crosses the axis at z = -1 - sqrt(2). Upstream's axis segment runs up from the origin and misses the crossing; the fix's spans the offset's whole height.](surface.surf_of_revolution--offset-cut-at-the-axis-below-the-origin.svg)

*The offset towards the axis is cut where it crosses it; upstream keeps the dashed part past it.*

## Every row

Old -> new values as the golden stores them (arrays of up to 16 numbers in full, longer ones as
their count and the first block's sum).

### surface.surface_approximate (74 rows)

- `curve_bounded_cylinder` (WR-726): out: n 184, first sum 15566.74928 -> out: n 204, first sum 15628.78678 (requested 0.001, achieved 0.000898756)
- `curve_bounded_just_copy` (WR-726): out: n 184, first sum 15566.74928 -> out: n 204, first sum 15628.78678 (requested 0.001, achieved 0.000898756)
- `gen0008` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.046544378290028623,0.0] (requested 1.5151e-06, achieved 0.0465444)
- `gen0014` (WR-726): out: n 1202, first sum 254860757.3 -> out: [1001.0,1004112.8532259027,0.0] (requested 0.225753, achieved 1.00411e+06)
- `gen0017` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.020595937621324497,0.0] (requested 0.00264246, achieved 0.0205959)
- `gen0028` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.016413593121751058,0.0] (requested 1.15385e-06, achieved 0.0164136)
- `gen0060` (WR-573): out: [0.0,-1.0,0.0] -> out: n 142, first sum 15145.61193 (requested 0.000956969, achieved 4.996e-16)
- `gen0062` (WR-726): out: n 594, first sum 19046.25145 -> out: [1001.0,4.023979816229703,0.0] (requested 1, achieved 4.02398)
- `gen0065` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.0029210410795245377,0.0] (requested 5e-06, achieved 0.00292104)
- `gen0073` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,4.817904104538713e-07,0.0] (requested 1e-08, achieved 4.8179e-07)
- `gen0080` (WR-726): out: n 34526, first sum 16336.03343 -> out: [1001.0,799.3748293739277,0.0] (requested 1.39378e-06, achieved 799.375)
- `gen0082` (WR-726): out: n 216, first sum 15455.63576 -> out: [1001.0,0.11719779725830086,0.0] (requested 5e-06, achieved 0.117198)
- `gen0084` (WR-726): out: n 910, first sum 14385.25789 -> out: [1001.0,0.015286884767212525,0.0] (requested 0.0032924, achieved 0.0152869)
- `gen0085` (WR-573): out: [0.0,-1.0,0.0] -> out: n 170, first sum 15412.85781 (requested 5e-06, achieved 3.97205e-15)
- `gen0097` (WR-573): out: [0.0,-1.0,0.0] -> out: n 150, first sum 15145.11754 (requested 8.16175e-07, achieved 6.28037e-16)
- `gen0101` (WR-573): out: [0.0,-1.0,0.0] -> out: n 150, first sum 15382.465 (requested 0.00239282, achieved 2.26442e-15)
- `gen0117` (WR-726): out: n 310, first sum 15192.48655 -> out: n 312, first sum 15191.17452 (requested 0.00130784, achieved 0.00118715)
- `gen0131` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.0029973409689794013,0.0] (requested 0.00102197, achieved 0.00299734)
- `gen0139` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,1104.3950770781375,0.0] (requested 1000, achieved 1104.4)
- `gen0140` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,3.476496676255416e-05,0.0] (requested 5e-06, achieved 3.4765e-05)
- `gen0142` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.000772678801436295,0.0] (requested 0.000563528, achieved 0.000772679)
- `gen0150` (WR-573): out: [0.0,-1.0,0.0] -> out: n 284, first sum 15276.27 (requested 0.00102232, achieved 0.000348235)
- `gen0152` (WR-573): out: [0.0,-1.0,0.0] -> out: n 82, first sum 15204.11082 (requested 5e-06, achieved 1.77636e-15)
- `gen0159` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.10440668582753658,0.0] (requested 5e-06, achieved 0.104407)
- `gen0163` (WR-726): out: n 1202, first sum 15450.54378 -> out: [1001.0,0.016705497909646823,0.0] (requested 0.00310834, achieved 0.0167055)
- `gen0170` (WR-573): out: [0.0,-1.0,0.0] -> out: n 758, first sum 15160.00016 (requested 1e-09, achieved 4.38789e-10)
- `gen0184` (WR-726): out: n 2562, first sum 15492.68866 -> out: [1001.0,0.4287885036687718,0.0] (requested 0.275788, achieved 0.428789)
- `gen0187` (WR-726): out: n 2100, first sum 15389.56103 -> out: [1001.0,0.10695810274694764,0.0] (requested 0.00177256, achieved 0.106958)
- `gen0193` (WR-726): out: n 10054, first sum 15388.51785 -> out: n 10390, first sum 15433.41929 (requested 5e-06, achieved 4.93785e-06)
- `gen0194` (WR-726): out: n 430, first sum 14430.79958 -> out: n 514, first sum 14445.04768 (requested 0.00260479, achieved 0.00213861)
- `gen0198` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,2.621617324553154e-06,0.0] (requested 1.47481e-06, achieved 2.62162e-06)
- `gen0202` (WR-726): out: n 150, first sum 15027.13736 -> out: [1001.0,0.4656540030115775,0.0] (requested 0.200214, achieved 0.465654)
- `gen0204` (WR-573): out: [0.0,-1.0,0.0] -> out: n 150, first sum 15150.31415 (requested 7.17309e-07, achieved 8.00593e-16)
- `gen0209` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.002789624015908364,0.0] (requested 0.00215219, achieved 0.00278962)
- `gen0217` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.011836907557934699,0.0] (requested 2.25581e-06, achieved 0.0118369)
- `gen0224` (WR-573): out: [0.0,-1.0,0.0] -> out: n 3522, first sum 15235.99957 (requested 1e-08, achieved 5.27681e-09)
- `gen0226` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.016068016380867373,0.0] (requested 0.00141482, achieved 0.016068)
- `gen0235` (WR-573): out: [0.0,-1.0,0.0] -> out: n 192, first sum 15167.03145 (requested 0.0743657, achieved 0.0122023)
- `gen0250` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.001130920677656001,0.0] (requested 0.000619082, achieved 0.00113092)
- `gen0265` (WR-726): out: n 1422, first sum 2.308078675e+13 -> out: [1001.0,248881929874.52722,0.0] (requested 10, achieved 2.48882e+11)
- `gen0267` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.10378410025796724,0.0] (requested 0.0012147, achieved 0.103784)
- `gen0272` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.013105380031664254,0.0] (requested 0.00246033, achieved 0.0131054)
- `gen0275` (WR-726): out: n 430, first sum 15547.68894 -> out: n 474, first sum 15559.87157 (requested 0.00221586, achieved 0.00197381)
- `gen0276` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.9108579385852134,0.0] (requested 9.69193e-07, achieved 0.910858)
- `gen0282` (WR-573): out: [0.0,-1.0,0.0] -> out: n 122, first sum 15071.00006 (requested 1e-07, achieved 4.31345e-08)
- `gen0287` (WR-573): out: [0.0,-1.0,0.0] -> out: n 2786, first sum 15438.19196 (requested 0.00256914, achieved 0.00246246)
- `gen0288` (WR-573): out: [0.0,-1.0,0.0] -> out: n 116, first sum 15068.50073 (requested 5e-06, achieved 4.06053e-06)
- `gen0294` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.08423910237000219,0.0] (requested 5e-06, achieved 0.0842391)
- `gen0297` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,45457284320282.61,0.0] (requested 0, achieved 4.54573e+13)
- `gen0299` (WR-726): out: n 352, first sum 15142.72336 -> out: [1001.0,1.0232805440672476e-09,0.0] (requested 1e-09, achieved 1.02328e-09)
- `gen0312` (WR-726): out: n 1014, first sum 15163.86785 -> out: [1001.0,0.17599829158680422,0.0] (requested 0.00149895, achieved 0.175998)
- `gen0317` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.0011264268224270937,0.0] (requested 2.11138e-06, achieved 0.00112643)
- `gen0333` (WR-726): out: n 116, first sum 15260.05586 -> out: [1001.0,0.06986167971141,0.0] (requested 0.00326581, achieved 0.0698617)
- `gen0339` (WR-726): out: n 1006, first sum 15181.99789 -> out: n 910, first sum 15181.99812 (requested 1e-08, achieved 9.14075e-09)
- `gen0344` (WR-573): out: [0.0,-1.0,0.0] -> out: n 82, first sum 15055.00002 (requested 5e-06, achieved 7.75126e-08)
- `gen0347` (WR-726): out: n 866, first sum 10606153.9 -> out: n 1140, first sum 10436697.26 (requested 100, achieved 96.31)
- `gen0351` (WR-726): out: n 1226, first sum -1.150875092e+10 -> out: [1001.0,1114772477.5853817,0.0] (requested 0.00263127, achieved 1.11477e+09)
- `gen0357` (WR-573): out: [0.0,-1.0,0.0] -> out: n 150, first sum 15307.65723 (requested 0.00282072, achieved 2.22045e-15)
- `gen0363` (WR-726): out: n 1226, first sum -9195201622 -> out: [1001.0,1110000664.1427076,0.0] (requested 0.00342223, achieved 1.11e+09)
- `gen0364` (WR-726): out: n 390, first sum 15519.11627 -> out: [1001.0,0.3698797955277302,0.0] (requested 0.336171, achieved 0.36988)
- `gen0367` (WR-726): out: n 594, first sum 109911249.8 -> out: [1001.0,68106.11406241925,0.0] (requested 1000, achieved 68106.1)
- `gen0370` (WR-726): out: n 266, first sum 2.595511372e+10 -> out: [1001.0,161512933.98002112,0.0] (requested 0.316304, achieved 1.61513e+08)
- `gen0373` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,1.8619006149354548e-16,0.0] (requested 0, achieved 1.8619e-16)
- `gen0374` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.001183010532713599,0.0] (requested 0, achieved 0.00118301)
- `gen0381` (WR-726): (known failing, no row) -> out: [1001.0,0.5460107506506426,0.0] (requested 0.00138129, achieved 0.546011)
- `gen0389` (WR-573): out: [0.0,-1.0,0.0] -> out: [1001.0,0.0026650155416503584,0.0] (requested 0.000782867, achieved 0.00266502)
- `gen0395` (WR-726): out: n 2510, first sum -1221.276073 -> out: [1001.0,4088.13408350007,0.0] (requested 0.00174909, achieved 4088.13)
- `issue1501_offset_wave_simple_knots` (WR-573): out: [0.0,-1.0,0.0] -> out: n 240, first sum 15665.14101 (requested 0.001, achieved 0.000918398)
- `offset_simple_knot_cubic` (WR-573): out: [0.0,-1.0,0.0] -> out: n 186, first sum 15260.33855 (requested 0.001, achieved 0.000961809)
- `offset_zero_cylinder` (WR-726): out: n 184, first sum 15566.74928 -> out: n 204, first sum 15628.78678 (requested 0.001, achieved 0.000898756)
- `quarter_cylinder_bare` (WR-726): out: n 184, first sum 15566.74928 -> out: n 204, first sum 15628.78678 (requested 0.001, achieved 0.000898756)
- `simple_knot_cubic_patch_index` (WR-573): out: [0.0,-1.0,0.0] -> out: n 118, first sum 15167.2 (requested 0.001, achieved 1.33234e-15)
- `volume_cylinder` (WR-726): out: n 184, first sum 15561.63605 -> out: n 204, first sum 15621.48975 (requested 0.001, achieved 0.000898756)
- `volume_just_copy` (WR-726): out: n 184, first sum 15561.63605 -> out: n 204, first sum 15621.48975 (requested 0.001, achieved 0.000898756)

### surface.surf_of_extrusion_p1 (9 rows)

- `gen0404` (WR-61 `eval_step`): build: [0.0]; out: n 48, first sum 0.1140089018; status: [0.0] -> build: [0.0]; out: n 48, first sum 0.5967790265; status: [0.0]
- `review4_circ_oob0_eval0` (WR-61 `eval_step`): build: [0.0]; out: n 48, first sum 10.23970742; status: [0.0] -> build: [0.0]; out: n 48, first sum 11.03428672; status: [0.0]
- `review4_line_oob0_eval0` (WR-61 `eval_step`): build: [0.0]; out: n 48, first sum -111; status: [0.0] -> build: [0.0]; out: n 48, first sum 9; status: [0.0]
- `review4_line_oob0_eval1` (WR-61 `eval_step`): build: [0.0]; out: n 48, first sum 27; status: [0.0] -> build: [0.0]; out: n 48, first sum 15; status: [0.0]
- `review4_line_oob0_eval3` (WR-61 `eval_step`): build: [0.0]; out: n 48, first sum 309; status: [0.0] -> build: [0.0]; out: n 48, first sum 15; status: [0.0]
- `review_hugeh_eval_out` (WR-61 `eval_step`): build: [0.0]; out: n 27, first sum 128284.9639; status: [0.0] -> build: [0.0]; out: n 27, first sum 128286.9639; status: [0.0]
- `review_hyp_eval_out` (WR-61 `eval_step`): build: [0.0]; out: n 27, first sum 13.07859639; status: [0.0] -> build: [0.0]; out: n 27, first sum 15.07859639; status: [0.0]
- `review_par_eval_out` (WR-61 `eval_step`): build: [0.0]; out: n 27, first sum 3.5; status: [0.0] -> build: [0.0]; out: n 27, first sum 5.5; status: [0.0]
- `review_tinyp_eval_out` (WR-61 `eval_step`): build: [0.0]; out: n 27, first sum -0.9952; status: [0.0] -> build: [0.0]; out: n 27, first sum 1.0048; status: [0.0]

### surface.surf_of_revolution_p1 (7 rows)

- `gen0052` (WR-738 `to_step`): build: [0.0]; out: [180.84860962098946,-93.27342223079216]; status: [0.0] -> build: [0.0]; out: [171.10481566161275,-93.27342223079216]; status: [0.0]
- `gen0061` (WR-738 `directrix`): build: [0.0]; out: n 60, first sum 14386.30753; status: [0.0] -> build: [0.0]; out: n 60, first sum 14380.63623; status: [0.0]
- `gen0184` (WR-738 `iso`): build: [0.0]; out: n 57, first sum 14218.2252; status: [0.0] -> build: [0.0]; out: n 57, first sum 14260.27235; status: [0.0]
- `gen0259` (WR-738 `start_plane`): build: [0.0]; out: [1.0648073416517736,1.199218327003228,-0.36554872688804285,1.0,153.91068538477188,1.0]; status: [0.0] -> build: [0.0]; out: [1.056398047914386,1.5499503341938334,-0.36554872688804285,1.0,149.17954822171788,1.0]; status: [0.0]
- `gen0329` (WR-738 `directrix`): build: [0.0]; out: n 60, first sum 14467.44267; status: [0.0] -> build: [0.0]; out: n 60, first sum 14439.84247; status: [0.0]
- `gen0341` (WR-738 `start_plane`): build: [0.0]; out: [-6.700176938643628,-3.357395923882631,2.6819422214491437,1.0,129.95724729768597,1.0]; status: [0.0] -> build: [0.0]; out: [-6.654514455177553,-3.3231040753908836,2.7324075544640247,1.0,127.97973993307939,1.0]; status: [0.0]
- `gen0436` (WR-738 `to_step`): build: [0.0]; out: [-3.619929122494868e-15,1.797055921200299]; status: [0.0] -> build: [0.0]; out: [1.0118009489537107e-15,1.797055921200299]; status: [0.0]

### surface.surf_of_revolution_p2 (19 rows)

- `gen0042` (WR-343 `offset`): build: [0.0]; out: n 333, first sum -inf; status: [0.0] -> build: [0.0]; out: n 291, first sum -inf; status: [0.0]
- `gen0082` (WR-343 `offset`): build: [0.0]; out: n 4701, first sum -inf; status: [0.0] -> build: [0.0]; out: n 7893, first sum -inf; status: [0.0]
- `gen0092` (WR-738 `offset`): build: [0.0]; out: n 1845, first sum -inf; status: [0.0] -> build: [0.0]; out: n 1761, first sum -inf; status: [0.0]
- `gen0101` (WR-343 `offset`): build: [0.0]; out: n 2559, first sum -inf; status: [0.0] -> build: [0.0]; out: n 753, first sum -inf; status: [0.0]
- `gen0168` (WR-343 `offset`): build: [0.0]; out: n 415, first sum 30350.19326; status: [0.0] -> build: [0.0]; out: n 415, first sum 30893.99302; status: [0.0]
- `gen0199` (WR-738 `build`): build: [0.0]; out: n 331, first sum 29909.79442 -> build: [0.0]; out: n 331, first sum 29921.0605
- `gen0236` (WR-343 `offset`): build: [0.0]; out: n 331, first sum 30161.0456; status: [0.0] -> build: [0.0]; out: n 331, first sum 28873.88458; status: [0.0]
- `gen0251` (WR-738 `adjust`): build: [0.0]; out: n 331, first sum 31395.90324; status: [0.0] -> build: [0.0]; out: n 331, first sum 31387.13815; status: [0.0]
- `gen0255` (WR-343 `offset`): build: [0.0]; out: n 415, first sum 30503.84519; status: [0.0] -> build: [0.0]; out: n 415, first sum 29883.4412; status: [0.0]
- `gen0329` (WR-738 `is_nurb`): build: [0.0]; out: n 247, first sum 30242.64304; status: [1.0] -> build: [0.0]; out: n 247, first sum 30242.01894; status: [1.0]
- `gen0368` (WR-343 `offset`): build: [0.0]; out: n 1005, first sum -inf; status: [0.0] -> build: [0.0]; out: n 1131, first sum -inf; status: [0.0]
- `gen0490` (WR-738 `trim`): build: [0.0]; out: n 331, first sum 30400.73745; status: [0.0] -> build: [0.0]; out: n 331, first sum 30397.20058; status: [0.0]
- `gen0492` (WR-343 `offset`): build: [0.0]; out: n 331, first sum 30101.03753; status: [0.0] -> build: [0.0]; out: n 247, first sum 30585.77127; status: [0.0]
- `gen0530` (WR-738 `adjust`): build: [0.0]; out: n 417, first sum 30348.83147; status: [0.0] -> build: [0.0]; out: n 417, first sum 30517.23051; status: [0.0]
- `gen0654` (WR-738 `adjust`): build: [0.0]; out: n 175, first sum 31268.55007; status: [0.0] -> build: [0.0]; out: n 175, first sum 31268.30354; status: [0.0]
- `gen0784` (WR-738 `adjust`): build: [0.0]; out: n 175, first sum 29710.9047; status: [0.0] -> build: [0.0]; out: n 175, first sum 29674.94916; status: [0.0]
- `gen0859` (WR-343 `offset`): build: [0.0]; out: n 279, first sum 31344.55107; status: [0.0] -> build: [0.0]; out: n 279, first sum 32033.55865; status: [0.0]
- `review_circle_touch_axis_offset` (WR-343 `offset`): build: [0.0]; out: n 415, first sum 31341.56261; status: [0.0] -> build: [0.0]; out: n 499, first sum 31100.17336; status: [0.0]
- `review_offset_axis_crossing_merge` (WR-343 `offset`): build: [0.0]; out: n 1215, first sum -inf; status: [0.0] -> build: [0.0]; out: n 1173, first sum -inf; status: [0.0]

### surface.surf_of_revolution_p3 (15 rows)

- `gen0052` (WR-738 `to_nurbs`): build: [0.0]; out: [0.794606939681873,-1.1975216339726882]; status: [0.0] -> build: [0.0]; out: [0.7668146372816801,-1.1975216339726882]; status: [0.0]
- `gen0240` (WR-738 `copy`): build: [0.0]; out: n 248, first sum 30316.21087; status: [0.0] -> build: [0.0]; out: n 248, first sum 30314.75713; status: [0.0]
- `gen0246` (WR-738 `swap`): build: [0.0]; out: n 331, first sum 30358.86501; status: [0.0] -> build: [0.0]; out: n 331, first sum 30358.63694; status: [0.0]
- `gen0348` (WR-738 `update`): build: [0.0]; out: n 289, first sum 29772.86668; status: [0.0] -> build: [0.0]; out: n 289, first sum 29973.05179; status: [0.0]
- `gen0422` (WR-738 `to_nurbs`): build: [0.0]; out: [0.6073151686569089,0.21182382616090017]; status: [0.0] -> build: [0.0]; out: [0.6188740940019568,0.21182382616090017]; status: [0.0]
- `gen0435` (WR-738 `to_nurbs`): build: [0.0]; out: [0.0,-1.4938106444378023]; status: [0.0] -> build: [0.0]; out: [0.00022029345919263152,-1.4938106444378023]; status: [0.0]
- `gen0444` (WR-738 `reverse`): build: [0.0]; out: n 415, first sum 30351.54025; status: [0.0] -> build: [0.0]; out: n 415, first sum 30344.99313; status: [0.0]
- `gen0462` (WR-738 `to_nurbs`): build: [0.0]; out: [0.0,0.27442760919883785]; status: [0.0] -> build: [0.0]; out: [0.0024955948462567473,0.27442760919883785]; status: [0.0]
- `gen0598` (WR-738 `swap`): build: [0.0]; out: n 247, first sum 30179.86981; status: [0.0] -> build: [0.0]; out: n 247, first sum 30177.38084; status: [0.0]
- `gen0629` (WR-738 `to_nurbs`): build: [0.0]; out: [0.8721515637142568,0.0]; status: [0.0] -> build: [0.0]; out: [0.8848737795653722,0.0]; status: [0.0]
- `gen0637` (WR-738 `to_nurbs`): build: [0.0]; out: [0.16476778247020873,0.8433855990942796]; status: [0.0] -> build: [0.0]; out: [0.16476778247020873,0.8544575028947936]; status: [0.0]
- `gen0650` (WR-738 `to_nurbs`): build: [0.0]; out: [1.2856814046429315,1.0]; status: [0.0] -> build: [0.0]; out: [1.2856814046429315,0.6905218664371997]; status: [0.0]
- `gen0736` (WR-738 `swap`): build: [0.0]; out: n 289, first sum 29635.37828; status: [0.0] -> build: [0.0]; out: n 289, first sum 29367.11663; status: [0.0]
- `gen0808` (WR-738 `to_nurbs`): build: [0.0]; out: [-2.05301802277461,0.0637711762079021]; status: [0.0] -> build: [0.0]; out: [-2.05301802277461,0.0]; status: [0.0]
- `gen0896` (WR-738 `update`): build: [0.0]; out: n 405, first sum 29861.38547; status: [0.0] -> build: [0.0]; out: n 405, first sum 29832.28703; status: [0.0]

### surface.cone_p1 (2 rows)

- `review2_std_bnd_neg_h_oob_icb8` (WR-738 `iso_circle`): build: [1000.0]; out: []; status: [-1002.0] -> build: [1000.0]; out: n 56, first sum 14420.82843; status: [1000.0]
- `review2_std_semi_unb_120_oob_ic9` (WR-738 `iso_circle`): build: [1000.0]; out: []; status: [-1002.0] -> build: [1000.0]; out: []; status: [1001.0]

### blend.fillet_solver_p4 (2 rows)

- `gen0189` (WR-738 `point_values`): fillet_point_step: [1e-05]; offsets: [-63589.196620918556,63589.196620918556]; point: n 80, first sum -5197450.253 -> fillet_point_step: [1e-05]; offsets: [-63589.196620918556,63589.196620918556]; point: n 80, first sum -5409431.087
- `gen0281` (WR-738 `point_values`): fillet_point_step: [0.001]; offsets: [123313.9716096632,123313.9716096632]; point: n 80, first sum -11003044.26 -> fillet_point_step: [0.001]; offsets: [123313.9716096632,123313.9716096632]; point: n 80, first sum -78544753.77

### surface.offset_surface (2 rows)

- `gen1268` (WR-738): out: n 154, first sum 91749.73271 -> out: n 154, first sum 91752.27761
- `gen1740` (WR-343): out: n 155, first sum 76061.50659 -> out: n 155, first sum 76067.9005

### surface.surface_point_solve_p3 (1 row)

- `matrix_1143` (WR-738 `global`): out: [1.0,1.0,0.3285420301022051,-0.24305095022573142,6.648613582037027e-10,1.0,2.0,1.0,1.0]; status: [0.0] -> out: [1.0,1.0,0.32854203010220506,-0.24305095022573123,6.648616832786346e-10,1.0,2.0,1.0,1.0]; status: [0.0]

### offset.offset_geometry_creation (2 rows)

- `gen0330` (WR-343): state: n 455, first sum 14925.90205 -> state: n 529, first sum 15801.35173
- `gen0492` (WR-343): state: n 781, first sum 15917.32169 -> state: n 781, first sum 14895.50505
