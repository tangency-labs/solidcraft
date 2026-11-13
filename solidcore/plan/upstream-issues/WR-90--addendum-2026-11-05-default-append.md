Updates: WR-90

#### WR-90 (update, 2026-11-05). Correct maximum on every append

- `GapArray::append` now inserts each record through `add`, preserving sequential insertion semantics. Finite lengths retain the earliest maximum; nonfinite lengths follow the existing IEEE comparison behavior, including a leading NaN remaining selected. A cached-maximum merge is insufficient for a source such as `[NaN, 3]` appended to `[1]`. The WR-90 flag and the public `append_opt_in` and `append_with_kernel` methods are removed; this is a source-breaking API cleanup.
- Independent Rust regressions scan stored lengths, including empty arrays, ties, extreme scales, a chained append, NaNs, infinities and negative values. For `[0.5]` appended with `[1, 3, 2]`, the correct maximum is length 3 at index 2 rather than the previous length 0.5 at index 0. For `[1]` appended with `[NaN, 3]`, sequential insertion selects 3 at index 2.
- The existing WR-90 record's C++ confirmation is historical, not a new native execution. The `heal.gap` diagnostic emits the appended array's maximum index and length; cases with a longer appended maximum may now disagree with C++. No diagnostic cases or expectations were removed or marked, and this change has not itself run the C++ diagnostic.
