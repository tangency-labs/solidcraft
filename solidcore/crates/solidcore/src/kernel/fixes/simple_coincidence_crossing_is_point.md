`SmCurve::SimpleCoincidenceChecker` returns a candidate coincidence range whose curves'
separation keeps one side at its two ends (taken across the curve) and whose largest
distance exceeds `SM_EFF_ZERO` scaled by the ends' size as the one point where the
curves come nearest (`LocalCurveSolve` minimum from the range's middle; `SM_ERR` when
none is found): only curves whose separation vanishes coincide over a range (the fork's
tangency-labs/smlib@04b42b4, case 3e). With `GlobalCurveIntersect`'s
`drop_ranges_of_tangencies` this makes a section tangent to a floor arc at their shared
end one point (#18064). A range whose separation changes sides (the fork's c2e3d9d) is
decided on the merged range in every kernel (WR-647, #18392).
