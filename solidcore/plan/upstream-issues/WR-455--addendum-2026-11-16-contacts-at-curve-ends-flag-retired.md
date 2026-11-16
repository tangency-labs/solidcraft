Updates: WR-455

#### WR-455 (addendum 2026-11-16): a contact at a curve's end always moves there; `curve_contacts_at_curve_ends` is retired

- **Status in the port:** `UpstreamFixes::curve_contacts_at_curve_ends` is gone. `Curve::global_curve_intersect` (`curve/curve/p8.rs`) always runs `move_contacts_to_curve_ends` after `sm_RefineCCSolutions`: a point solution moves to an end of either curve when that end lies within tolerance of the other curve and the curves stay within tolerance of each other over the whole stretch, measured on each curve. Part (1), `offset_stitch_at_body_tolerance`, keeps its own flag.
- **Retired with `simple_coincidence_crossing_is_point` (WR-647):** each one alone, and both together, turned the goldens case `blend.fillet_executive_p1` `remove_topo_edges_mouse_two_struts` into `create_by_joining: segments not end to end`. The defect was in the consumer, `longest_drop_run` (WR-543 addendum of the same date), which now joins only pieces the join accepts.
- **Removed:** the upstream path, the field and its doc, the preset line in `crates/smlib/src/core.rs`, the `fixes.json` and `issue_2260` entries and the flag settings in the integration tests.
- **Tests:** `issue_855_offset_stitch_keeps_the_bodys_tolerance` no longer loops over the contact setting: the stitch fix still gives the closed form 4564.924657 and the default still refuses (1001).
