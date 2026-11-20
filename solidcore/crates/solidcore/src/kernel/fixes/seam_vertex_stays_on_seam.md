A vertex that ends a seam edge (an edge one face use runs along twice,
[`crate::topology::vertex::seam_edge`]) stays on its seam through a boolean. A
vertex definition on it, or on a seam edge it will split (an edge crossing the seam
within tolerance, #18689), is put onto the seam at the seam's point under the
position `SmVertexDefinition::Resolve` works out, and when
`GlueCoincidentBrepVertices` merges the vertices of one proto vertex it keeps the
operand's own seam vertex over another's. Upstream moves a seam's vertex towards
the other object, or keeps a nearby crossing's vertex in its place, and the seam
edge then ends off the seam, which the face's seam test refuses (test 36) when
another edge runs beside the seam there: a boss joined under a half torus's flat
face at its seam, a notch cut beside a capped ball's seam corner (#18274;
solidcore-native, no fork commit).
