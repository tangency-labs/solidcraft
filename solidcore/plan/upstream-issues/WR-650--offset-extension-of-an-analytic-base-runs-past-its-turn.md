#### WR-650. The fork's analytic offset extension (WR-633) runs round a cylinder's, cone's, sphere's or torus's axis past a full turn
- **Severity:** Wrong result
- **Area:** Surfaces (offset surface)
- **Location:** the SolidCraft fork's `source/SMLib/src/SmOffsetSurface.cpp`, `sm_ExtendAsItself` and `sm_ExtendSurface` (tangency-labs/smlib@de0a6b8, branch `replay/1.0.1`). This is not in the pinned SMLib (`3fd5b95`), which has neither function's extension from the base.
- **What happens:** the extension from the base (WR-458) grows every side by at least one base domain, and nothing stops it at a full turn. A 294 deg arc is carried on 294 deg past each end, so it covers 882 deg, and later evaluations double the reach further. de0a6b8's piecewise NLib continuation also reparameterizes each piece, so a point half a domain past a half cylinder's end does not lie at 270 deg. The round solver's rails and corner solves (`SmFilletSolver::RailEdgeuseIntersect`, `CalcCornerVertGeom`) step out more than a turn away, although their first requests lie only 18 and 4.5 deg past the ends.
- **Trigger:** an offset surface on a partial cylinder, cone, sphere or torus, evaluated past its arc with extension allowed, with `offset_extension_from_base` and `offset_extension_as_itself` on. Example: a partial cylinder r 5 from 0 to 294 deg, offset 4.
- **How found:** SolidCraft. The fork fixes it in tangency-labs/smlib@93db777 and @b298cc2 (#1986).
  - 93db777 builds the extension directly from the analytic surface: the base's spans are kept, and new ones are joined with a knot of full multiplicity. Rows round the axis continue in rational quadratic arcs of a quarter turn. A sphere's or torus's generator continues as a circle in the first column, turned and scaled to the others. A cone's line continues straight.
  - b298cc2 splits the gap that closes the turn in two, one half past either end, into equal arcs of at most a quarter turn (`sm_RotTurn`). An extension that would reach round the turn ends at the closure, and `sm_ExtendSurface` wraps parameters past it, as for a periodic surface.
- **Confirmed:** no C++ run of the fork was made. The Rust port of de0a6b8 (main before this fix, both flags on) reproduces the problem on the 294 deg partial cylinder, evaluated from 4.5 to 900 deg past either end:
  - the extension reaches 6.35 turns more than one turn;
  - its ends lie 94 deg off the closure at -33 deg;
  - a point and the point a turn back differ by 17.5.

  On the half cylinder (0 to 180 deg), half a domain past the end lies 32.6 deg off 270, and a domain past it 76.4 deg off 360.
- **Rust port:** behaves as upstream by default. The fix completes the WR-633 fork change, so it is under that change's flag, `Kernel::fixes.offset_extension_as_itself` (with `offset_extension_from_base`). `extend_as_itself` is now a port of the 93db777 + b298cc2 construction (`rot_turn`, `arc_spans`, `turn_about`, `radial`; at most 4096 spans per end), and `extend_from_base` closes the turn and wraps (`crates/solidcore/src/surface/offset_surface.rs`). With the fix, on the 294 deg arc:
  - every point lies on radius 9 (to 3.6e-15) and on the point a turn back (to 1.4e-14; the fork's test reports 2.67e-14);
  - the extension is exactly one turn, its closure at -33 deg (to 2.8e-14 deg);
  - inside the arc the points do not move.

  On the half cylinder, the points half a domain and a domain past its end lie at 270 and 360 deg (to 1.4e-14).

  Tests: `surface::offset_surface::tests::wr650_partial_cylinder_extension_closes_its_turn` and `surface::offset_surface::tests::wr650_half_cylinder_extension_continues_in_quarter_turns`. The fork's other check (the tabbed plate's mitred rounds, removed volume 9.204197946 against 9.204199837) needs a SolidCraft model that solidcore does not have, and is not reproduced.
