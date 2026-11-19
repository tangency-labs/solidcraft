A knife edge (a cusp: faces tangent along the edge that fold back on each other,
their binormals pointing the same way) is rounded with the ball touching both faces
back from the edge: `SmEdge::GetBlendEdgeuse` no longer takes it for a smooth edge,
the constant radius, constant distance and variable radius solvers offset each face
to the side the other lies on, from the difference of the faces' normal curvatures
across the edge where the faces are tangent (`sm_SectorSides`), and the Nx1
corner's degeneracy test takes anti-parallel side normals for an ordinary corner.
Upstream refuses the edge as tangent (WR-570; the fork's tangency-labs/smlib@b7ec55b).
