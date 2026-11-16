Updates: WR-652

#### WR-652 (update, 2026-11-14). `plane_recognition_origin_after_axes` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch S-plane). `Plane::is_nurb_surface_plane` (`surface/plane/p2.rs`) places the recognised plane's origin along its final, perpendicular axes; upstream's placement along the unturned Y axis is gone.

- **Tests:** `crates/solidcore/tests/issue_1056_plane_recognition_origin_after_axes.rs` keeps its corrected half on `Kernel::default()` (corner gap 0 at v = 4, `40 * TURN` at v = 5); the half that pinned upstream's `v_min * 40 * TURN` gap is gone. `surface/plane/p2.rs::tests::plane_origin_after_axes_is_domain_local` checks the same closed forms for v minima 4 and -5, and the zero-minimum and orthogonal cases against the surface.
- **Results:** no goldens row moves on the library's default kernel (SolidCraft's preset had the flag on).
