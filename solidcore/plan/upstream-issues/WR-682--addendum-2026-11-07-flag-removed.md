Updates: WR-682

#### WR-682 (update, 2026-11-07). `fillet_surface_plane_curve_on_plane_equation` is removed: the surface-plane curve is traced on the plane's equation on every kernel

The fork's surface-and-plane trace (5bdcddf) precedes its G^n section, and every G^n test here
turned it on with the section. With `fillet_gn_section_generator` removed (WR-685's addendum of
2026-11-06), the default kernel's rounds need it too. Without it, a variable round's
`CalcFilletIntSideFace` finds no curve where the round meets the end face (1001). That failed:
- the box edge's linear round 0.5 -> 2 (`every_fillet_entry_point_leaves_the_cached_topology_current`);
- the 20 cube's law 3, 2, 4, 2, 3;
- the 2 -> 30 cliff overflow.

So the flag is removed and `sm_SrfSrfIntersection` takes the trace wherever the intersectors
found no curve (`fillet_geom::p2`).

- **Results:** a surface and a plane that cross at two given points are traced past the plane's
  domain, where upstream's intersectors stopped (1001). The analytic intersector's answers stand,
  as before:
  - a flat surface meets the plane in its exact line;
  - a surface that touches the plane, or an end off the surface, is not traced;
  - the cylinder as itself keeps its intersector's 1001 between points past the patch.
- **Fixes to the trace itself (not the fork's).** The goldens' generated
  `blend.fillet_geom_p2` cases showed two defects it carried from the fork:
  - **It left the surface.** The march went on along the B-spline's extrapolation (`gen0014`:
    u = -6e5 on a domain of [0, 1]), and each refining pass split every span that missed,
    doubling the stations until the allocation aborted (4.9 GB). A station off the surface's
    domain, in a direction the surface isn't closed in, is now the curve leaving the surface,
    and the trace refuses it (1001, as main's result). The trace also holds at most
    `MAX_STATIONS`, the march's own 100 000.
  - **Coinciding ends** (`gen0123`) are no curve between two points. They're not the trace's
    event, and the intersectors answer them as before (1007).
- **Known failing:** upstream's `cyl_on_box1.smb` edge 12 r 2 (the goldens'
  `private-upstream-rollover-404-cyl-on-box1-edge12-r2`) fails in the rollover (1001) once the
  trace is taken. Main's default built it. The trace's curves agree with the two-points
  intersector's to the tolerance, so the cause isn't found yet (tangency-labs/solidcraft#13694, moved from tangency-labs/solidcore#3072).
- **Tests:** `wr682_surface_plane_curve` keeps the trace's tests on the default kernel. Its
  flag-off case is removed, and the cases that compared the flag on and off assert those answers
  themselves. The tests that set the flag drop the setting.
