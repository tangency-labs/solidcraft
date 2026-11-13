Updates: WR-600

#### WR-600 (update, 2026-11-03). The blend corner's End takes a tangent seam as a tangent end without the flag

The blend corner's End (`fillet_blend_corner`, WR-697) is the Nx1 corner's construction, and the fork's blend corner carries 7992ec7 and 856b54e, which the port has behind `fillet_nx1_tangent_side_edge_same_convexity` (tangency-labs/solidcore#2500).

- **Defect:** with the blend corner and without this flag, an End whose rail crosses a tangent seam (#2089's r 1 crease round ending on the cylinder, its rail on the r 2 round's torus at the torus/cylinder contact) took the rail end as the rail's crossing of the end face, which only touches the rail there (the point came out at the rail's start), and marched the cap edge into the tangency: fillet code 5.
- **Change:** `fillet_corner::topology::tangent_seam_ends` is this flag or `fillet_blend_corner`; the four read sites (the side edges' convexity and tangent sides, the run-on past the corner's vertex in `fillet_solver/p2.rs`, the tangent end rib and the cap edge along the seam in `fillet_geom/p5.rs`) use it. The flag stays: it is the Nx1 corner's fix on the old path, and SolidCraft's set turns it on.
- **Test:** `the_blend_corner_alone_rounds_the_crease_to_its_tangent_end` in `crates/solidcore/tests/issue_2089_tangent_tracer_crease_round.rs` (one lump, check level 2 clean, the round within 8.8e-5 of its balls' envelope, its tolerance 2.7e-4).
