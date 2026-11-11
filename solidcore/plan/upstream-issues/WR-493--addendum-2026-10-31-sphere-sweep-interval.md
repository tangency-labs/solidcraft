#### Updates: WR-493 (2026-10-31). A kept NURB's sphere takes its sweep interval from its polar curve

- **Found:** #2073. With `Kernel::fixes.found_torus_sphere_keep_tested_nurb`, `RebuildSTEPFromNURBParameters` takes the sweep interval from the first iso circle and the polar converter from another. On a small corner sphere the two differ past the check's tolerances, 98.999999999999 against 98.99999999222109 degrees at R 0.02, so `SmSurfOfRevolution` tests #6 and #7 fail on a sphere within 3e-12 of its radius.
- **Effect:** port, wedges with all edges rounded, 44 angles, R 0.01 to 3.5, 518 successful rounds: 59 bodies rejected with the flag off, 153 with it on, 19 with this change. Faces and success are the same in all 518, volume within 5.6e-8 relative at accuracy 1e-6.
- **Rust port:** under the same flag, `Sphere::is_nurb_surface_sphere` sets the U interval of the STEP domain to the polar curve's after the rebuild. Test: `tests/wr493_corner_sphere_polar_domain.rs`.
- **Confirmed:** port only. The fork's C++ with tangency-labs/smlib@7ea6a96 was not run, so this departs from the fork's flag as far as is known. The torus branch is unchanged.
- **Left:** `SmSurfOfRevolution` test #10 on cones, test #7 on other surfaces and `SmVertex` test #1 on these bodies (#2073).
