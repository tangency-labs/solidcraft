Updates: WR-721

#### WR-721 (update, 2026-11-13). `plane_first_torus_section_exact` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch A13c). With a plane first, `global_surface_intersect_into` (`surface/plane/p2.rs`) takes a torus's own plane section, the orders and UV curves swapped as for a surface of revolution; `SurfOfRevolution`'s plane section through the axis (`surface/surf_of_revolution/p1.rs`) rotates an ellipse-kind generator (a torus's circle, or any revolved arc) as the curve it is, so a meridian keeps its kind. A plane the torus's own case does not solve (oblique, or parallel to the axis off it) still goes on to the general intersection.

- **Tests:** `a_plane_meets_a_torus_in_its_exact_circles_in_either_order` runs on `Kernel::default()`; its half that pinned upstream's marched sections is gone.
- **Results:** 35 goldens rows in 5 units move, on the library's default kernel (SolidCraft's preset had the flag on).
  - `surface.torus` 5, `surface.surf_of_revolution_p1` 13, `surface.surf_of_revolution_p2` 5: a section through the axis comes back as a circle (ellipse kind) instead of a B-spline. Its NURB is the same to 1.6e-15 and every point of it lies on its circle to 1.4e-14; `surface.torus` also writes the circle's frame and radii.
  - `topology.face_p3` 3: a plane touching a torus along its top circle, with the plane first, now gives the same circle as with the torus first, byte for byte.
  - `boolean.api_brep` 9: boxes with a face tangent to a torus along an arc (#383 seeds 4 and 86) and a box through a horn torus. Volumes and face areas move within the measuring accuracy (1e-6 and 1e-4 relative); for the horn torus they move closer to independently integrated values (volume 4.4e-6 to 2.6e-8 off). With post-processing, seed 4's intersection has 9 edges and 6 vertices instead of 10 and 7; seed 86's difference lists its two pieces in the other order.
