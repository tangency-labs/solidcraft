#2991: the mirrored shell-rim's chamfer strips whose surface kind is `cone` are not a
definition yet. At the rounded-inside corner of the shelled box (solidcore#2903, the
SolidCraft build that formerly refused the first corner), a chamfer's band over an inner
vertical round's arc is a cone, but it carries no procedural blend — the round strips of
the same rim are `exact-round` sections (surface-of-revolution/cone) and are definitions;
the chamfer's four plane strips over the walls are. Measured in
`wr649_band_between_rail_ends.rs` (once per capture, at the first mirror corner).
