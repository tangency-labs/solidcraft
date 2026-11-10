# mass.integrator: addendum (2026-11-01): the whole revolved sphere's volume fails at accuracy 1e-8 in the pinned C++ as well, a limitation both share

Updates: plan/findings/mass.integrator--whole-revolved-sphere-volume-fails.md

That record left its C++ verdict pending. The C++ reference fails in the same way, so this is a limitation the port shares with upstream, not a Rust parity bug.

- **Comparison:** the `sweep.api_brep` rotational `observe_geometry` cases `wr471_rot_half_circle_whole_sphere_off_3e-7` and `wr471_rot_half_circle_whole_sphere_off_0` (`fixtures/sweep.api_brep/wr471-rotational-whole-sphere-volume.json`).
  - Input: `SmApiCreateArc` (the quadratic B-spline arc) of radius 5 in the XY plane, from −90 to 90 degrees, centred 3e-7 or 0 off the Y axis, turned 360 degrees about Y with no caps, all flags off. This is the record's flag-off input turned onto the Y axis.
  - Each side computes the volume of the result twice, `SmApiBrepComputeVolume` / `brep_volume` at accuracy 1e-6 and at 1e-8.
- **Result** (probe batches 54j092, 54j0926 and 54j25): C++ and Rust agree on every value, max_rel_err 0.
  - At 1e-6 both integrate: 523.598956772222 (centre off by 3e-7) and 523.5988331275332 (centre on the axis), against 4/3 π 5³ = 523.5987755982989, so 3.5e-7 and 1.1e-7 relative.
  - At 1e-8 both return `SM_ERR` (1001).
- **Verdict:** a limitation both share, at the tighter accuracy, of the integrator on a whole revolved sphere face. Rust matches the pinned C++, and no Rust fix is owed for parity.
- **Status:**
  - `sweep::api_brep::tests::wr471_whole_sphere_volume` stays ignored, now because upstream fails it too (it asks for 1e-8).
  - The two difftest cases pass and stay in the gate as plain passes, recording both accuracies.
  - A better integrator for this face would be an opt-in improvement over upstream, not a parity fix. None is planned.
