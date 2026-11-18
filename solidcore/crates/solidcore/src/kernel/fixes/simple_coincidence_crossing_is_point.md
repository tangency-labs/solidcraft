`SmCurve::SimpleCoincidenceChecker` returns a candidate coincidence range whose curves'
separation points to opposite sides at its two ends (both separations longer than
`SM_EFF_ZERO` scaled by the ends' size) as the one crossing `LocalCurveIntersect`
finds inside it, started where the separation taken as linear between the ends
vanishes (`SM_ERR` when none is found). Upstream returns the range: two equal circles
a little apart graze within tolerance over a stretch set by the subdivision, and the
range becomes a tiny edge, so a ring unioned with a half-annulus prism whose r 5 walls
are 1e-4 apart is non-manifold and at 1e-3 not a solid (WR-647, #1979; the fork's
tangency-labs/smlib@c2e3d9d). A range whose separation keeps one side without
vanishing (two curves running close within tolerance without touching, or a
tangency) is the one point where the curves come nearest (`LocalCurveSolve`
minimum from the range's middle); only curves whose separation vanishes coincide
over a range (the fork's 04b42b4).
