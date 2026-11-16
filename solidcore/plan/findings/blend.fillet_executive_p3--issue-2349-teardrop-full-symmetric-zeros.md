# blend.fillet_executive_p3: the teardrop's symmetric zeros differ at 1.9e-8 relative (#2349, 2026-11-06)

Units: `blend.fillet_executive_p3`. PR: https://github.com/tangency-labs/solidcore/pull/2381.

## Case and values

`issue-2349-teardrop-full-r0.5` (`fixtures/blend.fillet_executive_p3/issue_2349_closed_angle.json#5`):
the radius-0.5 round of the closed teardrop rim, built in full, then its mass properties.
Topology and the other 282 compared values agree. Two values differ:

| Field | Quantity | C++ | Rust | rel |
|---|---|---|---|---|
| `values.mass_properties[4]` | centroid y | -1.420937672371026e-6 | -1.4209376993505527e-6 | 1.9e-8 |
| `values.mass_properties[9]` | product yz | -6.38053826782007e-3 | -6.380538385912757e-3 | 1.85e-8 |

Both quantities are odd in y. The operand is mirror-symmetric about y = 0 (poles `(0,0,0)`,
`(20,-10,0)`, `(20,10,0)`, `(0,0,0)` extruded along z), so for a correct round both are
exactly 0. Neither side is near 0: both kernels build the same asymmetric round and agree on
its asymmetry to 2.7e-14 absolute in the centroid. The radius-3 full case passes.

## Suspected cause

The closed round's first span from the tip, at one end only, is off the true round by up to
7.9e-4 (finding `blend.fillet_corner_p3--issue-2349-closed-round-start-span`, #2800). One
end wrong and the other right breaks the mirror symmetry, which gives the nonzero centroid y.
The comparison is then of two values that should be 0, at a relative tolerance. The case
passed (796 of 798) before #2400. Since #2400 the two sides round that error differently in the
last digits. The difference is not traced to a single operation.

## Status

Recorded as a known mismatch of `blend.fillet_executive_p3`. It comes off when the round's
first span is on the true round: then both values are 0 up to rounding on each side, and the
case is graded by its closed form, centroid y = 0 and product yz = 0.
