#2991: the mirrored shell-rim's chamfer strips whose surface kind is `cone` are not a
definition yet. At the rounded-inside corner of the shelled box (solidcore#2903, the
SolidCraft build that formerly refused the first corner), a chamfer's band over an inner
vertical round's arc is a cone, but it carries no procedural blend — the round strips of
the same rim are `exact-round` sections (surface-of-revolution/cone) and are definitions;
the chamfer's four plane strips over the walls are. Measured in
`wr649_band_between_rail_ends.rs` (once per capture, at the first mirror corner).

#17875: a cylinder's rim chamfered by its radius is a traced `cone` net too. The offsets meet
on the axis, so the chamfer is a cone to its apex whose centre line and top rail are one
point; `LinearCrossSectionFsg::definition_surface` has no section to run along and returns
`None`, and the chamfer is built as before (`create_surface`). Measured in
`issue_2489_blend_faces_are_definitions.rs` ("cylinder rim chamfered by its radius", one
bare chamfer face, kind `cone`).
