Updates: WR-69

`Kernel::fixes.dupin_bottom_hyperbola_axis` opts into correcting the bottom
hyperbola's second axis in `MakeDupin` to `1/sqrt(abs(k1))`. It is off by default:
the pinned C++ (`SmAdvSurfaceIntersector.cpp:1122`) and the default Rust path keep
using `1/sqrt(abs(k2))`. The top and bottom conjugate branches should have the
same axis lengths `(1/sqrt(abs(k2)), 1/sqrt(abs(k1)))`, with opposite rotations.

The flag applies to the public `AdvSurfaceIntersector::make_dupin` operation and
to both indicatrices constructed by its singular-point classifier, including
classification reached through `SurfaceIntersector::compute_point_values`.

`crates/solidcore/tests/wr69_dupin_bottom_hyperbola.rs` checks unequal-curvature
saddles `z = (4u²-v²)/2` and `z = (u²-9v²)/2`, and the equal-magnitude control
`z = (4u²-4v²)/2`, against both reference normals. It verifies all four conics'
axis lengths with the flag off and on, and the bottom branch's rotation. The
unequal-curvature assertion with the flag enabled fails before the correction.
The existing `intersect.adv_surface_intersector` difftest retains default C++
parity coverage; no tolerance or fixture is changed.
