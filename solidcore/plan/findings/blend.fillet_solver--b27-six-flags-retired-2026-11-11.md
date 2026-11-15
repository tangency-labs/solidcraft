# Fillet solver: six fixes become the default, 25 golden rows move (2026-11-11)

- Flags retired (tangency-labs/solidcraft#14246, batch B27): `fillet_rail_rim_on_face`
  (WR-301), `fillet_torus_center_on_axis` (WR-422), `fillet_torus_span_from_start_section`
  (WR-568), `fillet_torus_rail_on_axis` (WR-592), `fillet_tangent_point_one_sided_step`
  (WR-595) and `fillet_trace_from_tangent_point_toward_next` (WR-603). Each WR record has an
  addendum of 2026-11-11.
- `cargo run --profile goldens -p goldens -- --all --goldens` at the fix: 283 units,
  281 pass; `blend.fillet_executive_p3` changed=14 and `blend.fillet_solver_p4` changed=11,
  added, removed and broken 0 in both.
- Which flag moves which row: the two units ran on the code before this change with one flag
  on at a time (a temporary switch in `Kernel::new`, not committed), and with WR-595 and
  WR-603 together, which the fork has as a pair:

| flag | `fillet_executive_p3` | `fillet_solver_p4` |
|---|---|---|
| WR-301 rail rim on face | 0 | 0 |
| WR-422 torus centre on axis | 1 | 0 |
| WR-568 torus span | 2 | 0 |
| WR-592 torus rail on axis | 5 | 0 |
| WR-595 one-sided step | 6 | 11 |
| WR-603 trace toward next | 7 | 0 |
| WR-595 and WR-603 | 7 | 11 |

  The 14 rows of `fillet_executive_p3` are the union: 2 (WR-568), 5 (WR-592, one of them
  also moved by WR-422) and 7 (WR-595 with WR-603). WR-301 moves no row; its tests
  (`tests/fillet_wr300_320.rs`) and fixtures 1080 and 4055 show it.
- Blessing rewrote three more rows within the goldens' tolerance
  (`attributed-box-cylinder-arc-round-seam-quarter-r2`, relative 6.4e-12; `gen0132` and
  `gen0139` of `fillet_solver_p4`, relative 1e-12). They keep their old values, so the diff
  holds only the rows that move.

## WR-568: a round along a boolean's arc is built (2 rows)

`attributed-box-cylinder-arc-round-seam-opposite-r2` and `-three-quarter-r2`: a box 40 x 10 x 15
joined to a cylinder r 10, the concave arc where the box's top meets the cylinder rounded
r 2. The box points 180 and 270 degrees from the cylinder's seam, so the arc's `SmCircle`
starts at an angle of its own frame, which the old code took off the span: the torus came
out short and the round failed.

| case | old | new |
|---|---|---|
| `attributed-box-cylinder-arc-round-seam-opposite-r2` | 1001; 10 v, 15 e, 8 f | 0; 12 v, 18 e, 9 f |
| `attributed-box-cylinder-arc-round-seam-three-quarter-r2` | 1001; 10 v, 15 e, 8 f | 0; 12 v, 18 e, 9 f |

Why right: the same box at 90 degrees (`seam-quarter-r2`), which main already built, gives
12, 18 and 9 too, and the first two numbers of the attributes digest agree to 1.3e-10
relative (459392.61597 at 90 degrees; 459392.61592 and 459392.61603 at 180 and 270). The
digest's third number depends on where the faces lie, so it differs between the three
directions.

## WR-422 and WR-592: a rail point on the arc's axis (5 rows)

- `big-rad-filleted_box-r1.0-top-edges` (WR-422 and WR-592 each): status 0 and 20, 36, 18
  before and after; volume 564.2389452228 and every mass property agree to 1.2e-15
  relative. The topology digest's blocks trade 8 between them (281291 + 285980 = 281283 +
  285988) and the first geometry block's third number grows by exactly 224, while every
  other geometry number agrees to 1e-14: by the digests, the same solid with its entities
  in another order.
- The four `issue920-cylinder-rim-*-r10.0` rows (WR-592): a cylinder's rim rounded at its
  own radius, so the rail runs on the axis. Status, counts, topology digest and the mass
  check unchanged; the edge2 rows' geometry digest moves by 1.2e-16 relative. The rail
  point now lies on the axis instead of up to the solver's tolerance from it.

| case | volume old | volume new | largest mass change |
|---|---|---|---|
| `issue920-cylinder-rim-edge1-constant-r10.0` | 1570.796376097398 | 1570.796376097398 | 1.2e-11 |
| `issue920-cylinder-rim-edge1-variable-constant-law-r10.0` | 1570.796376097398 | 1570.796376097398 | 1.2e-11 |
| `issue920-cylinder-rim-edge2-constant-r10.0` | 1570.796380667838 | 1570.796380667838 | 2.3e-10 |
| `issue920-cylinder-rim-edge2-variable-constant-law-r10.0` | 1570.796380667838 | 1570.796380667838 | 2.3e-10 |

  The volumes agree to the 15 digits shown (under 5e-13); the largest change is in a moment
  of inertia, absolute.

## WR-595 and WR-603: a trace from a tangent point (7 rows)

Upstream's `Fillet/brepForFillet2.smb` and `Fillet/test5.smb`, one edge rounded r 0.5. The
round's trace starts at a tangent point: it now starts toward the next through point and
takes the point's tangent from the forward step.

| case | volume old | volume new | relative |
|---|---|---|---|
| `stubs4-degenerate-brepForFillet2-e5-r0.5` | 1440.251409 | 1440.251204 | 1.4e-7 |
| `stubs4-degenerate-brepForFillet2-e7-r0.5` | 1434.446892 | 1434.446720 | 1.2e-7 |
| `stubs4-degenerate-brepForFillet2-e9-r0.5` | 1440.267312 | 1440.267307 | 3.7e-9 |

`test5` records no volume (mass status 1007 before and after); the third number of its first
geometry digest block:

| case | old | new | relative |
|---|---|---|---|
| `stubs4-degenerate-test5-e2-r0.5` | 18376.163061 | 18376.161971 | 5.9e-8 |
| `stubs4-degenerate-test5-e3-r0.5` | 21582.090046 | 21582.089480 | 2.6e-8 |
| `stubs4-degenerate-test5-e5-r0.5` | 23475.246059 | 23475.246803 | 3.2e-8 |
| `stubs4-degenerate-test5-e9-r0.5` | 31560.318670 | 31560.320444 | 5.6e-8 |

- Status, vertex, edge and face counts, the topology digest and the mass status are the same
  before and after in all seven. Every number of the `test5` geometry digests moves by under
  1.3e-7 relative.
- No closed form exists for these rounds. The rows move by the trace's own accuracy, and the
  path is the fork's (tangency-labs/smlib@68fb916 and f8fd83e), which SolidCraft has run
  since both flags went into its presets. `tests/wr603_trace_from_tangent_point.rs` checks
  the peg round of WR-603 against its added volume.

## WR-595 in the unit: `compute_point_values` at a tangent point (11 rows)

`gen0027`, `gen0049`, `gen0137`, `gen0163`, `gen0171`, `gen0221`, `gen0349`, `gen0377`,
`gen0532`, `gen0580`, `gen0594`: generated calls of `FilletSolver::compute_point_values`. Only
the point record (its position, derivatives and tangent, digested to three numbers) moves;
the status (0, or 1001 in `gen0027` and `gen0580`) and the offsets do not. The changed
line only acts on a point of type `TangentPoint` for which both steps found a point, so
every moved case is one: its tangent is now the one-sided difference to the forward step,
not the central difference across the point. These inputs are random, so no row has an
independent value; the behaviour is the fork's (tangency-labs/smlib@f8fd83e).

| case | point digest old | point digest new |
|---|---|---|
| gen0027 | 19.6195, 35.3171, 447.510 | 19.6195, 35.3171, 447.515 |
| gen0049 | -15.0733, 48.2029, -770.002 | -14.3020, 48.9731, -788.103 |
| gen0137 | 9.33552, 46.8725, 169.281 | 13.5665, 46.9896, 253.370 |
| gen0163 | 10.7208, 55.5416, 62.6894 | 9.43496, 55.1996, -52.7881 |
| gen0171 | 13.1822, 39.4873, 338.803 | 10.9999, 39.2268, 179.702 |
| gen0221 | 1.62905, 27.9384, -182.434 | 3.77574, 28.0244, -29.3368 |
| gen0349 | 5.15516, 39.8915, -205.571 | 2.38129, 39.9990, -279.335 |
| gen0377 | -6592.91, 13883.7, -141797 | -6591.73, 13882.6, -141749 |
| gen0532 | -1120.67, 3006.98, -11024.5 | -6100.37, 7799.90, -55775.0 |
| gen0580 | 5502761, 10005454, 89687682 | 5502763, 10005454, 89687799 |
| gen0594 | 630551, 630561, 6648049 | 520572, 520580, 5108768 |
