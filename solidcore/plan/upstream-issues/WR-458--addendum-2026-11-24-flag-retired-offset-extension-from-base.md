Updates: WR-458

#### WR-458 (update, 2026-11-24). `offset_extension_from_base` is removed; an offset always extends its base

An offset surface evaluated past its base's domain now always extends the base itself (`extend_from_base` in `crates/solidcore/src/surface/offset_surface.rs`), on every side that can grow at once, each by the domain's size times the least power of two that reaches the point and never less than the extension it replaces. Two copies of one offset therefore give the same point at the same parameters whatever each evaluated first. Removed: the field, its fixes doc, the goldens preset arm and the tests' flag lines and flag-off halves (tangency-labs/solidcraft#18921).

`extend_toward` evaluates a second time on the stored copy when the first `N_SrfEvalPtDerivsUnbounded` call fails after storing it, as upstream's `sm_ExtendSurface` does. Without it the offset of a B-spline cone whose apex is a row of its net refused every point past its domain.

- **Tests:** `issue_419_offset_extension_does_not_depend_on_evaluation_order`, `offset_of_a_cone_with_an_apex_row_extends`, `offset_extension_parameters_do_not_depend_on_its_extent`.
- **Goldens:** see the WR-633 addendum of the same date; the rows of both retirements are certified together.
