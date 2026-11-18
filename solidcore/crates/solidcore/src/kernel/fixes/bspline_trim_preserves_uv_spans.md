`BSplineCurve::trim` removes short end spans only for model-space curves
(dimension 3), preserving exact UV trims in surface parameter units
(WR-441, #1114). A short UV end span whose control points all lie within check
test #15's pole tolerance (`SM_ZONE_TOL_3D / 10`) is still removed as upstream
removes it, so the trim passes the check (#2129). Off by default for pinned SMLib
compatibility.
