# WR-773: N_SrfJoin removes the joint knot on a net that mixes weights with NL_NOW

- Severity: wrong result (a joined surface off the two pieces by more than `tol`).
- Source: `3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`.
- Location: `NL_SrfTool.cpp`, `N_SrfJoin` (both direction branches: the control
  point loading, then `N_SrfRemoveKnotConditional`, then the loop that sets `NL_NOW`
  weights to `1.0`).
- Trigger: joining a rational and a non-rational surface. The joined net takes the
  rational piece's points with their weights and the non-rational piece's points with
  the marker `NL_NOW` (`-DBL_MAX`); only the averaged joint row gets a real weight.
  `N_SrfRemoveKnotConditional` then runs on that net: it reads the rationality from
  `Pw[0][0]` alone (`N_IsSrfRat`), so it takes `-DBL_MAX` as a weight, or, when
  `Pw[0][0]` is non-rational, drops the real weights. The loop that
  sets `NL_NOW` to `1.0` runs only after the removal, which has then decided from
  those weights.
- How found: `nlib.srf_tool_p3` goldens, whose generator joins a rational and a
  non-rational surface in some of its join cases. With the fix, 14 of its 2524 cases
  change; the other mixed joins keep their goldens. In 12 of the 14,
  `tol` is `-1`, so no knot may go, yet the old order removed the joint knot once and
  left a surface `0.23` to `7.0e3` off the exact join (largest
  distance on a 41 x 41 grid of the domain); the corrected join keeps the knot and is
  the exact join. In the other two (`tol = 1e3`) the removal takes a different number
  of knots, within `tol` before and after.
- Figure: `plan/findings/nlib.srf_tool_p3--wr-773-join-order/join-order.svg`, the
  three steps of the join in upstream's order and in solidcore's.
- Confirmation: read in the pinned source (the order of loading, removal and the
  `NL_NOW` loop; `N_SrfRemoveKnotConditional` checks no consistency). Not run: the
  C++ is a reading reference only.
- Rust: `nurbs::srf_tool::p3::join` gives the non-rational piece weights `1.0` before
  loading its points (`make_rat_compatible`, as `N_MakeSrfsCompatible` does). The
  removal then runs on the net upstream has only after its `NL_NOW` loop (the joint
  weight `0.5 * w1 + 0.5 * w2` equals upstream's `0.5 * (w1 + w2)` while `w1 + w2` is
  finite). No flag: the corrected join is the only one.
