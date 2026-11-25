# mass.integrator: `brep_volume` fails with `SM_ERR` on a whole revolved sphere face, with the WR-471 flag on or off (2026-11-01)

**Unit:** mass.integrator (`crates/solidcore/src/mass/integrator.rs`, `mass::integrator::properties::brep_volume`). Found through the WR-471 test of `sweep.api_brep`; the WR-471 flag is not the cause.

**Input.** `create_rotational_sweep` of one curve, the quadratic B-spline half circle of radius 5 (`BSplineCurve::create_circle_segment`, 0 to 180 degrees, in the XZ plane), turned 360 degrees about the Z axis, no caps. The circle's centre is off the axis by `off`. The result is one face, one edge and two vertices, a manifold solid of the sphere's volume 4/3 π 5³.

**Result (Rust, probe batch 54j075, `brep_volume` at accuracy 1e-8).** Every leg fails the same way, `Upstream { operation: "mass::integrator", code: 1001 }`, the ODE integrator giving up (step-size underflow, step limit or a non-finite integrand). Area (`brep_area`, 1e-8) and the manifold test succeed on every leg.

| WR-471 flags | off | face | vertices (distance from axis, z) | area (4π·25 = 314.15926536) | volume |
| --- | --- | --- | --- | --- | --- |
| on | 3e-7 | sphere | 1.5e-15, +5; 9.2e-16, −5 (the poles) | 314.15926331 | `SM_ERR` |
| on | 0 | sphere | the poles | 314.15927038 | `SM_ERR` |
| on | 1e-9 | sphere | the poles | 314.15926841 | `SM_ERR` |
| on | 1e-12 | sphere | the poles | 314.15926361 | `SM_ERR` |
| on | −3e-7 | revolution | 3e-7, ±5 | 314.15923516 | `SM_ERR` |
| off | 3e-7 | sphere | 5, 0 (the seam's ends on the equator) | 314.15927051 | `SM_ERR` |
| off | 0 | sphere | 5, 0 | 314.15926628 | `SM_ERR` |

"WR-471 flags" are `revolve_analytic_profile_faces` and `revolve_arc_centre_near_axis_is_sphere`.

So the failure is the integrator's on a whole revolved sphere face, whatever its parametrisation and wherever its seam vertices are, including the face the port builds with every flag off.

**C++.** Pending. Whether the pinned C++ integrates the flag-off face (`SmApiBrepComputeVolume` of `SmApiCreateRotationalSweep` of the same arc) decides between a Rust parity bug, where Rust fails and C++ integrates, and a limitation both share. The comparison is the `sweep.api_brep` rotational `observe_geometry` cases `wr471_rot_half_circle_whole_sphere_off_3e-7` and `_off_0`. Until it lands, this record makes no claim either way, and an addendum records the outcome.

**Status.** The WR-471 test asserts the face kind, counts, pole placement, manifold test and area. Its volume assertion is kept as the ignored test `sweep::api_brep::tests::wr471_whole_sphere_volume`, which names this file. If the C++ comparison shows a parity bug, the integrator fix is its own work item.
