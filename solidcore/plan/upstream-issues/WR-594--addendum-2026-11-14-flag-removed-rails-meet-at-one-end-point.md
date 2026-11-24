Updates: WR-594

#### WR-594 (update, 2026-11-14). `fillet_rails_meet_at_one_end_point` is removed; rails whose ends meet within tolerance always end at one point

`CircularCrossSectionFsg::create_surface_from_curves` (`blend/fillet_standard_solver/p1.rs`) always moves both rails' end
control points to their midpoint, keeping their weights, at an end where the rails' end points lie within the
approximation tolerance of each other. The opt-in described in the original record no longer exists. Removed: the field
`fillet_rails_meet_at_one_end_point` and its docs, SolidCraft's preset line, the goldens case-name mapping, its entries in
`kernel_cases/fixes.json` and the `issue_2260_gn_pipe_refined_range_end.rs` flag list, the test settings in `wr650`,
`issue_2020` and `issue_1945`, and the flag-off test in `wr_594_rails_meet_at_one_end_point.rs`.

- **Why it is correct.** The cross sections built next make a point section wherever the rails are closer than that
  same tolerance, at the first rail's point. If the second rail keeps its own end, the skinner refuses it for missing
  the point section by more than `MODEL_TOL` (1e-7), or keeps a near pole that the surface check refuses (this record's
  trigger, 6.5e-12). Ends closer than the tolerance are one point to that tolerance, and their midpoint lies within half
  of it of both. Each rail's end moves by at most half the tolerance.
- **A real operation now reaches it.** The finding `general--flag-round3-wr594-no-operation-reaches-the-meeting-rails`
  found no B-rep operation that reached the guard. tangency-labs/solidcraft#16686 has one: the slot round in
  `solidcore-ops/tests/tangent_rollover.rs`. Its overflow-first lead pass traces past an End to where the rails meet, at
  (-36, -90, 10), and stops 1.66e-5 short. The skinner refused that last section (`Input`, section 28 of 28). With the
  rails' ends meeting, the trace skins, and the blend is refused as its rail leaving its face at the slot. That is
  pinned in #16728.
- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes.
- **Goldens:** no row moves. `--core-only` on cb44b9870 (from main 7f7eedbce): 283 units, 283 pass. The kernel rig and
  every test pass as well.
- **Tests:** `rails_end_at_one_point` runs on the default kernel with its assertions unchanged.
