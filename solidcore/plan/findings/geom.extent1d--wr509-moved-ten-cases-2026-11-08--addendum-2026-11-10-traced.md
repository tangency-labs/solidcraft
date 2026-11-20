# Addendum: the WR-509 cases in curve.ellipse_p2 and query.face_p3, traced and checked

Addendum to `geom.extent1d--wr509-moved-ten-cases-2026-11-08.md`, which left the three
`curve.ellipse_p2` cases untraced and the `query.face_p3` sector unchecked.

**How measured.** main `bf5cd7a88`, `--profile goldens`, Windows. A temporary `eprintln!` in each
runner printed every operation's output. The cases were run as they are, then with the WR-509
branch of `Extent1d::contains_periodic_value` put back to the C++ rule (set the value to the
interval's start). With the C++ rule, all four cases give exactly their goldens from before
WR-509. With the fix, they give their current goldens. Nothing else in these cases moves.

## `curve.ellipse_p2`: one angle per case, now the point's own

Each case's record changes in one entry, an angle in degrees that `step_inversion` maps through
`contains_periodic_value`. The check is the closed form the angle is defined by:
`atan2(rx / ry * dy, dx)` of the point's coordinates in the ellipse's frame (x the unit x axis,
y the y axis made orthogonal to it), put into the period that starts at the domain's start.
It was computed in Python from the case's inputs, apart from the kernel.

| Case | Entry | Domain | Was (C++ rule) | Now | Closed form |
|---|---|---|---|---|---|
| `gen0042` | `rec[17]`, `StartPlane`'s angle | [0, 90] | 0.0 | -1.8598415920223488e-5 | -1.85984159632e-5 |
| `gen0855` | `rec[5]`, `StepInv`'s angle | [-74.175414, 22.039457] | -74.175414 | -74.17541032277933 | -74.1754103228 |
| `gen1601` | `rec[5]`, `StepInv`'s angle | [0, 90] | 0.0 | -3.2997056678141234e-6 | -3.29970569468e-6 |

The new angles agree with the closed form to 4.3e-14 degrees. The old ones were off by the
whole move (1.9e-5, 3.7e-6 and 3.3e-6 degrees). In `gen0855` the point lies inside the arc,
3.7e-6 degrees past its start. `atan2`'s [0, 360) put it at the top of the period, and the C++
rule then moved it to the start. In the other two cases the point lies that far before the start,
within the containment tolerance. Each entry accounts for its block's whole change: one entry
changed by d, at index j, moves the three sums by d, |d| and (j + 1) d, which is the change
recorded.

## `query.face_p3` `adv_sec_b8_e2_m0_f1e-09_c2_t0.0_0.0`: why it now reaches the query

The case crosses edge use 2 of `sphere.smb`'s face at edge parameter 1.5307337294603592e-8,
1e-9 of the edge's interval [0, 15.307...] from its start. It then classifies the UV line from
that crossing to the face's UV corner (0, 0).

- **Why it was rejected.** With the C++ rule, `DropPoint` of the crossing returned the UV
  (0, 0), the corner itself: the crossing's v was moved onto the start. The line from the
  crossing to the corner then had zero length, and the runner's own check (`start.distance(end)
  < 1e-9`) rejected the case. None of the face or edge-use checks in `sector_setup` was involved.
- **Now.** `DropPoint` returns (0, 1.5307338653379647e-8). That v is the crossing's edge
  parameter to 1.4e-15. The line is 1.53e-8 long, and the case reaches `sector_classify_curve_on`.
- **The sector `[1, 1, 5]`.** The query's step-off distance starts at the intersection tolerance
  of the zone tolerances (edge 0, curve 1.0). That is far longer than this 1.53e-8 line, so the
  step runs off the curve on its first round. The query reports that with possible problem 1 and
  `ClassifyProblem::ShortInterval` (5), the code for "the curve interval is shorter than the
  step-off". This half of the result follows from the line's length. The inside flag that comes
  with it is marked doubtful by the query itself (the line lies along the face's u = 0 side), and
  it is not checked here against an independent classification.
