Updates: WR-755

#### WR-755 (addendum 2026-11-08): the dx = 1e-2, A - B case that failed `check` is fixed by WR-756

- **Status in the port:** the Rust port's line "dx = 1e-2: A - B has the right volume but fails `check` ... a separate cause not fixed here" is out of date. That separate cause is WR-756 (a vertex kept a sliver off one of its faces), and `place_vertices_on_face_corners` fixes it: dx = 1e-2 is valid in both orders.
- **Tests:** `crates/solidcore-ops/tests/issue_3012_filleted_boxes_apart.rs` now runs dx = 1e-4, 1e-3 and 1e-2, both orders, and asserts `check(All)` is valid.
