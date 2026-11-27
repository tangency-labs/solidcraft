# Synthetic plane records

These records describe a 10 × 30 rectangle, with four degree-one UV boundary curves.
They were prepared locally for the internal body-record round-trip investigation;
no private design or external CAD output is used.

For origin O and the stored axes U and V, the four vertices are O, O+10U,
O+10U+30V and O+30V. The expected area is 300 |U×V|, the centroid is
O+5U+15V, and the bounds are the componentwise extrema of those four corners.
The sheet volume is zero. `origin_only` translates the oblique record by
(12,-7,23), retaining its axes and UV boundary.

`expected.json` retains the original analytically derived values used in the
archived synthetic counterchecks. They are not re-recorded from this change's
output. The tests use the existing geometric tolerance convention and compare
the stored plane coefficients and modeled domains exactly, separately from the
known independent line-endpoint reconstruction behavior.
