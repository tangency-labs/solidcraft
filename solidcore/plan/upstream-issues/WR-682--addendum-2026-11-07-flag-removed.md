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
- **Tests:** `wr682_surface_plane_curve` keeps the trace's tests on the default kernel. Its
  flag-off case is removed, and the cases that compared the flag on and off assert those answers
  themselves. The tests that set the flag drop the setting.
