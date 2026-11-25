Updates: WR-15

#### WR-15 (update, 2026-11-05). All centroid blocks by default

`PointSet3d::centroid` now uses the former all-blocks path and caches its result.
For 100 copies of `(2, 4, 6)` queried before axes, the former default returned
`(1, 2, 3)`; the mean is `(2, 4, 6)`. Empty sets still return zero. The existing
weighted block-sum arithmetic and its IEEE overflow behavior are unchanged.
Axes computed without a supplied normal can replace the cached centroid with the
best-fit plane centroid; arbitrary floating-point inputs are not promised bitwise
call-order identity.

The flag `point_set_centroid_all_blocks` and methods `centroid_opt_in` and
`centroid_with_kernel` are removed; callers should use `centroid()`. This is a
source-breaking API change. The historical C++ finding remains confirmed; no C++
was built or run for this change.

The Rust golden changes 19 centroid vectors in 15 cases, retaining all 2,032 case
IDs and every non-centroid field. For each changed vector, an independent
100-digit sum of the input binary64 coordinates divided by the point count agrees
with the new mean (largest absolute difference `7.105427357601002e-15`). The old
vectors instead agree with omitting the final 50 points while dividing by the
original count. The arithmetic-mean definition is
[`sum(Y_i) / N` (NIST)](https://www.itl.nist.gov/div898/handbook/eda/section3/eda351.htm);
it is an oracle, not a claim about floating-point error bounds.

Same-host unchanged-main runs isolate the change to those 15 cases. Three
tight-box differences (`gen0219`, `gen0227`, `gen0423`) and 164 rounding-only
record differences also occur on unchanged main; their golden fields are left
untouched, not blessed as part of this fix.
