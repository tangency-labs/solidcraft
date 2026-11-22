Updates: WR-408

`SmTorus::IsNurbSurfaceTorus` is now ported (`Torus::is_nurb_surface_torus`,
`crates/solidcore/src/surface/torus.rs`, tangency-labs/solidcore#1433). Its generator
interval has the same unsigned `AngleBetween` defect; the existing opt-in
`Kernel::fixes.torus_signed_gen_curve_interval` now covers it too, through the same
`torus_gen_curve_interval`, with the fork's (tangency-labs/smlib@d9f20b0) zero tolerance
`SM_EFF_ZERO` there (`SM_EFF_ZERO_DEG` in the rebuild), and a failed signed angle makes
the test answer "not a torus", as the fork. No new flag. Default off: the `surface.torus`
`issue1433_torus_*` fixtures (including intervals below the equator) compare upstream's
unsigned angles. Test with the flag on: `torus_recognition_partial_scaled_and_non_matching`.
