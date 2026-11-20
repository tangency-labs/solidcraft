Chamfers meeting at a vertex where every edge is chamfered, each chamfer a plane
between two planes, are closed by the chamfer planes themselves, exactly: the blend
corner's chamfer event ([`crate::blend::blend_corner::chamfer`]), in place of the
pinned 3x3 corner's chamfer branch (`SmFilletConvexNxNCorner` with `SetChamfer`,
convex and concave alike). The corner type is the session's
(`FilletSession::set_chamfer_corner`): the Mitre (the default), where each chamfer
runs on to the others and three of them meet at the point M of their 3x3 solve,
pairwise along the exact lines from the rails' crossings to M, with no corner face;
or the Facet, one plane through the rails' crossings whose edges are the chamfers'
end sections, exact lines in their planes. A mitre point outside the corner (a
chamfer much wider than the others) is refused with `FilletErrorType::NoRoom`,
naming the largest size of that chamfer that keeps it inside, less the fit margin,
as the refusal's fit; the facet is refused there too, its plane leaving the
chamfers. Four or more chamfers whose crossings lie in one plane build the facet
(their mitre is refused, naming why); where their crossings are not in one plane the
corner is a patch, whichever type was asked: the chamfers stop at their crossings and
n bilinear quadrilaterals round the crossings' centroid close the skew polygon of
their end sections, G0 to each other and to each chamfer (at four, the one bilinear
patch of the crossings), within the crossings' convex hull and so in the corner.
Chamfer planes so nearly coplanar that the points where they
meet are not determined to the part's rounding are refused with
`FilletErrorType::CornerUnsupported`, naming that. Three chamfers of both convexities between planes are refused as bevels, naming the vertex, until the event builds their mitre. The pinned corner marches the
mitre's edges as surface intersections, refuses a mitre of four or more chamfers with
no reason, and builds its corner face as a Coons patch where the chamfers are unequal
(WR-716, #2345; own design, smlib#89's brief, the owner's answers on #2345). For the
port in progress only: at parity it becomes the default and the pinned corner's
chamfer branch is deleted (WR-716's retirement plan).
