# topology.edge — exact private revolution transport (issue #294)

The private edge shim and importer reuse the shared lossless record helpers for
native SmSurfOfRevolution (15020), with a bitwise independent B-spline-base check.
Malformed records remain harness errors; nested heads are rejected without
recursive parsing. Concrete geometry, owner/tolerance metadata and raw knots
retain their existing transport. No shared numerical implementation changes.

Fresh native default results on main 7c74a05f6: 1787 cases, 1595 value passes,
192 deferrals. With the private adapter: 1667 value passes, 120 deferrals, zero
failures or empty passes; max relative error zero. The unit remains partial.

The 72 cases compare 120 flag/index/status values at zero relative error;
topological distance remains the upstream 1e20 sentinel. This is not a claim of
new nonzero geometric-distance coverage. Relative tolerance stays 1e-9, with no
edge absolute floors. Remaining native cone/sphere/extrusion transport and the
six DropCurve dependencies remain explicit; active DropCurve work belongs to
PR #463. No competing implementation is included.

The per-case outputs, separate earlier six-case dependency audit and validation
are documented in [the edge report](issue-294-edge-revolution-report.md).
