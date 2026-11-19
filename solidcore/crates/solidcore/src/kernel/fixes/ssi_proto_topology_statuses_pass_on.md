Surface/surface intersection and proto-topology pass on the statuses SMLib drops;
where a helper answers a question it answers "no" when a point it compares cannot
be evaluated, instead of comparing unset values (`IsPointOnCurve`, `AddStartPoint`'s
tangent test, `sm_ComputeCurveType`, `sm_AreSameCurve`, `IsInteriorPoint`,
`sm_IsInterior`, `sm_AreOverlapping`, `sm_WhichPVToCombine`) (WR-662, #2009 group
C; tangency-labs/smlib bcc6f01). Off by default for pinned parity.
