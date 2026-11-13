Updates: WR-502

#### WR-502 (update, 2026-11-11). `fillet_corner_sphere_from_four_arcs` is removed

`BSplineSurface::create_sphere_from_arcs` (`nurbs/b_spline_surface/p2.rs`) now always makes the whole sphere through four arcs of one centre and radius that close a loop. That is the only path now, and the flag is gone. Upstream's refusal of anything but three arcs ("only works for 3 now") is no longer reachable in the Rust kernel. Removed: the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** see the retiring PR for the goldens run on a main export. SolidCraft's preset and the kernel rig's `fixes.json` already had the flag on.
- **Tests:** `four_arcs_get_their_sphere` (every sampled point 2 from the centre within 1e-9) and `pyramid_apex_is_the_ball` (`fork_smlib125_four_round_corners.rs`) run on the default kernel. The upstream refusal halves are deleted; the WR-103 test now checks that four arcs of one centre still get their sphere with `sphere_from_arcs_common_centre` on.
