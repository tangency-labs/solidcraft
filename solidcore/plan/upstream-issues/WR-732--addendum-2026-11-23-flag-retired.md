Updates: WR-732

#### WR-732 (update, 2026-11-23). `brep_ray_classifies_current_trims` is removed; the ray always classifies against current trims while editing

`query::brep::ray::face_surface_cache` builds the tree of a face whose loops close against its current trims
whenever the B-rep is being edited (`MappedCacheFace::implanting_while_editing`), and a face whose loops are open
mid-edit, or whose trims fail to implant, is `SolidError::FaceTrimsUnavailable` naming the face. Upstream builds the
tree untrimmed while editing and answers right only when it reuses a cache built before the edit. Removed: the field
and its doc file, the flag's entries in the tests' flag lists, and the flag-off half of
`an_open_loop_mid_edit_is_refused_naming_its_face`.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes.
- **Goldens (#18934, measured on `main` at e97d2814d7):** 55 rows in 7 units move, 44 beyond the comparator; no
  status, error or deferral changes. The read of
  `Fillet/sphere_diff_3cylinders.smb` makes the five UV trims #20179's classification made (edge uses 127, 129, 130,
  132, 135) by the implant instead, their control points up to 2.7e-9 off in UV, and computes four of the vertex/face
  gaps setting them cleared again: `blend.fillet_executive_p3` 10 rows (mass only), `corpus.parity` 1 (the area is
  main's value before #20179 again), `topology.face_p3` 12. The stitch cases of `heal.stitch_p1` and `heal.stitch_p2`
  (27 lines; 30 ledger rows, three more within block sums) move edge and vertex tolerances both ways. In the
  19 `cov_cube1`/`cov_cube5` cases, two vertices each end at twice their gap to their edge uses' starts, the margin
  `topology::face::p3`'s UV trim gaps give (main kept the gap plus the zone tolerance; the gaps do not move).
  `cov_cube_gap_at_edge_tol_nest` raises edges 2 and 16, halves edges 4 and 13 and lowers vertices 8, 13 and 14.
  Every moved stitch tolerance is a row of `certify::trim_tolerance`, certified there. `blend.fillet_executive_p3`'s
  `bottle-e8-r0.1` (mass, 4.5e-9) and the generated `topology.brep_cutting` `gen0049`/`gen0050` and
  `topology.brep_p10` `gen0026`/`gen0298` move as well; their path through the ray was not traced further. The
  goldens' replays and ledgers (`tolsub`, `refit`, `wr493_rows`, `certify::trim_tolerance`) put main's read
  back or hold the row with the change; see #18934's PR.
- **Tests:** `crates/solidcore/tests/brep_query_cache_lifetime.rs` runs the WR-732 tests on `Kernel::default()`.
