Updates: WR-474

## Whole-body wire bounds pin the interval rule (#1954)

The existing `issue_1054_tight_box_interval_knots.rs` regressions call the
curve helper directly. Three additional tests construct a real wire B-rep
with `create_wire_edge_from_curve` and call the public
`query::brep::calculate_tight_bounding_box`. Only
`curve_tight_box_interval_knots` differs between the kernels. No production
code, flag default, tolerance or native comparison policy changes.

- A radius-50 quadratic quarter-circle curve retained by an edge over its
  first half gives `(0, 0, 0)`–`(50, 50, 0)` with the flag off, and
  `(50/sqrt(2), 0, 0)`–`(50, 50/sqrt(2), 0)` with it on. Pinned C++
  `3fd5b9548d19` reproduces the off result through the same wire construction
  and whole-body query (its numerical x minimum is `3.061616997868383e-15`).
- A degree-one curve with peaks at parameters 1, 3 and 5, queried over
  `[2.5, 3.5]`, gives `(1, 0, 0)`–`(6, 20, 0)` off and
  `(2.5, 2, 0)`–`(3.5, 4, 0)` on. Both edge endpoints have y=2, so the
  interior y=4 maximum is asserted independently of the endpoint bounds.
- With the interval `[2, 4]`, the on result is
  `(2, 0, 0)`–`(4, 4, 0)`: knots at the interval boundary remain represented
  by the endpoints. Whole-curve controls for both curve kinds agree with
  the full-domain box with either flag setting.

Each constructed body has zero faces, one edge and two vertices. The tests
also verify the stored edge interval and the retained curve's natural
interval; the constructor has not silently shortened the underlying curve.

Adversarial check on main `20466d554`: forcing the production flag read off
fails all three new tests; omitting the interior-knot additions fails both
peak-sensitive tests. After byte-for-byte restoration and rebuilding, all
eight tests in the binary pass. A separate seed-474 probe checks 48 public
body-query subintervals at radii 0.001, 50 and 1,000,000 against the explicit
quadratic-rational circle formula. All 48 pass. These fault/probe results
are separate from the native comparison gates.

This resolves only WR-474's public whole-body wire-query coverage gap in
the audit, not all of #1954. It does not claim an imported sphere-cap face,
an application importer, or every face-query caller has been exercised.
