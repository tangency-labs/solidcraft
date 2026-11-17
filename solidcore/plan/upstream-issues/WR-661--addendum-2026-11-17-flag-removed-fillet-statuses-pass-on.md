Updates: WR-661 (`WR-661--fillet-code-drops-statuses.md`)

#### WR-661 (update, 2026-11-17). `fillet_statuses_pass_on` is removed

The fillet executive, solvers, corners and geometry now always return the statuses SMLib drops and the fork passes on (tangency-labs/smlib 873c7c9, 66b02de, 9b7cd48). Propagation is the only path, and the flag is gone (tangency-labs/solidcraft#14246, group G1): the field, every ignore branch at the sites (`passed_on` with the flag, and the helpers left unused), `TemporaryRadiusChange`'s `*_with_fixes` variants, SolidCraft's preset line, its `fixes.json` entry and every test setting. The pass-on unit tests keep their pass-on half.

One site keeps a result (not upstream, which ignores this status for every body): `finish_merged_fillet`'s `OrientTrimmedSurfaces`. When the stitched result of an open input still has lamina edges, the closed-solid orientation's "gathered faceuses from both sides of some face" assert (`SmBrep.cpp`:24796, ported as `topology::brep::p3::GATHERED_BOTH_SIDES`) can fire on an open sheet that is orientable: walking a face use's neighbours crosses the sheet's boundary edges to its other side. The executive keeps the body only when it is the input sheet filleted (`blend::fillet_executive::session::finish::OpenInput`):
  - every end vertex of every lamina edge of the result lies on the input's boundary. This stands in for "every lamina edge was a lamina edge of the input", because the fillet rebuilds the edges it cuts back, so the result's boundary edges are new edges on the input's boundary curves;
  - no lamina edge runs along another (a seam the stitch did not join);
  - once the filleting and split are done, the kept body passes `check_brep` level 2. At orientation time the new edges are not yet validated, so level 2 cannot run there.

Otherwise the 1005 is returned and the body is restored. Every other orientation failure, and this one on a body without lamina edges, is passed on. ARGUED, not PROVEN: the assert fires on an orientable open sheet. `orient_ray_votes` can swap faces before the assert, and the `faces_to_swap_uv` faces are swapped at loop entry, so the kept body's orientation rests on the level-2 check and on `StitchAndOrient` unifying an open body's normals afterwards. `chamfer_fillet` never stitches or orients, so the exception is never reached there.

- **Results** (every golden blessed on main and with the change, compared row by row; 39 rows move, with WR-604's `Trim`):
  - `blend.fillet_geom_p1`, 24 rows, success to 1001/1007. Each asks `SmFilletGeom::Trim` for a trim that does not exist: a rail or surface trim at a parameter outside the curve's range (`BSplineCurve::trim` `ParameterOutside`: `fb_trim_outside`, `gen0139`, `gen0185`, `gen0438`, `gen0476`, `gen1351`, `gen1477`, `trim_failed_{rail0,rail1,other1,other2,center_line}`), an interval that misses the curve's or surface's domain (`Extent1d::intersect` `Disjoint`, several against the inside-out extent [1e20, −1e20]: `gen0245`, `gen0829`, `gen1440`, `gen1452`, `gen1609`, `gen1626`, `gen1647`, `gen1709`, `trim_disjoint_rail_interval`, `trim_disjoint_surface_domain`), or two trim points closer than the curve can be cut (`fb_trim_point`, `trim_failed_surface_point`). Main returned success with the rail untrimmed. PROVEN by `goldens` `blend_fillet_geom_p1::tests::wr661_moved_trims_ask_for_a_trim_that_does_not_exist`, which asserts for every row that the requested parameter lies outside [lo, hi], or the intervals are empty or apart, or the trim's two points coincide.
  - `blend.fillet_geom_p5`, 13 rows, 0 to 1007. `TrimRailCurves` asks `Extent1d::set_min_max` for an interval that ends before it starts ("assignment would make a negative interval"). PROVEN by `blend_fillet_geom_p5::tests::wr661_moved_rail_trims_end_before_they_start`. These rows are shared with `fillet_rail_trim_at_near_knot` (WR-664, #14246 item 31), whose retirement moves the same trims.
  - `blend.fillet_geom_p3` `gen0210`, 0 to 1007: the recalculation trims a rail that ends before it starts (the same `set_min_max` refusal). PROVEN by `blend_fillet_geom_p3::tests::wr661_moved_recalculation_trims_a_rail_that_ends_before_it_starts`.
  - `blend.fillet_custom_solver` `adv_arc_and_line_rails`: 1001 before and after. The failed `InsertOneKnot` is now returned where it fails, so the second rail is no longer edited after the first rail's insert fails; only the recorded rail-2 state differs. ARGUED: a refusal either way, and the fork returns at the same `SER`.
- **Kept, with a fix:** `blend.fillet_executive_p3` `stubs4-degenerate-test5-e2-r0.5` and `stubs4-shell_fillet2-e5-r0.5` fillet open sheets (4 and 3 faces). Passing the orientation status on refused them (1005). Main's results (5 and 4 faces) pass `check_brep` level 2, and with the narrowed exception above they build as on main. Tests: `blend_fillet_executive_p3::end_to_end::wr661_open_sheets::{a_degenerate_test5_sheet_fillet_is_kept, a_shell_fillet2_sheet_fillet_is_kept}` (one face added, still open, level 2 clean).
- **Refused, with the narrowed exception:** `blend.fillet_executive_p3` `stubs4-open_shell-e5-r0.5` (lamina 12, #13733). Its kept body fails level 2 once the filleting is done, so it returns 1005; it is removed from `known_failing.json`. Test: `wr661_open_sheets::an_open_shell_sheet_fillet_that_fails_level_2_is_refused`.
- **Unit tests** (`session::finish::open_input_tests`):
  - `a_closed_input_left_open_is_refused`;
  - `an_open_input_is_kept_only_with_its_boundary`: a face removed from an open body adds boundary away from the input's;
  - `an_open_seam_is_refused`: a tube with one side unstitched, level 2 clean.

  Revert runs:
  - without the seam rule, `an_open_seam_is_refused` fails;
  - without the boundary rule, `an_open_input_is_kept_only_with_its_boundary` fails;
  - without the deferred level-2 check, the open-shell test fails.

  The "input was open" test is implied by the boundary rule (an empty boundary holds no point), so no test separates it.
