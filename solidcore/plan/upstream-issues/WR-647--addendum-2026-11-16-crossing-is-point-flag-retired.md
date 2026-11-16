Updates: WR-647

#### WR-647 (addendum 2026-11-16): a crossing or a near approach is a point; `simple_coincidence_crossing_is_point` is retired

- **Status in the port:** `UpstreamFixes::simple_coincidence_crossing_is_point` is gone. `Curve::simple_coincidence_checker` (`curve/curve/p6.rs`) always returns a range whose separation changes sides between its ends as the crossing inside it (fork c2e3d9d), and one whose separation keeps one side without vanishing as the point where the curves come nearest (fork 04b42b4). Only curves whose separation vanishes coincide over a range.
- **Retired with `curve_contacts_at_curve_ends` (WR-455):** see that record's addendum of the same date for the goldens case the two broke and the consumer fix (WR-543 addendum).
- **Removed:** the upstream path, the field and its doc, the preset line in `crates/smlib/src/core.rs`, the `fixes.json` and `issue_2260` entries and the flag settings in the integration tests.
- **Tests:** the flag-off halves are deleted with the path: `wr_647_crossing_is_point_is_off_by_default`, `wr_647_concentric_arcs_within_tolerance_are_a_range_by_default` and `issue_1979_ring_lip_union_is_invalid_by_default`. `issue_1979_ring_lip_union_is_valid_at_every_offset`, `wr_647_concentric_arcs_within_tolerance_are_their_nearest_point`, `wr_647_the_same_arc_twice_stays_a_range` and `an_arc_and_its_shorter_copy_coincide_over_the_copy` run on `Kernel::default()`.
