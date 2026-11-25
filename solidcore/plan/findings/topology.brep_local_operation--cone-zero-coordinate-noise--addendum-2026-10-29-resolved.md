# topology.brep_local_operation: cone zero-coordinate differences were a knot-side mix-up (2026-10-29)

Updates: plan/findings/topology.brep_local_operation--cone-zero-coordinate-noise.md

Resolved: not solver noise. Two probes on `cone_face0_{out,same,tilt}` traced both
mismatches to one root: the cone's unit normal at face 0's mid-UV,
uv (7.653668647301795, 5.5901699437494754).

- **Inputs agree.** At that uv, the point and both first derivatives (`Evaluate1stDerivatives(uv, TRUE, TRUE, ..)`
  and `Surface::evaluate_1st_derivatives(k, uv, true, true)`) were bitwise equal.
- **The normals did not.**
  - C++: (0.44721359549995771, -3.1776437161565099e-16, -0.89442719099991608).
    This is exactly `Unitize(Su x Sv)` of those derivatives.
  - Rust: (0.44721359549995765, -2.1028134743827771e-16, -0.8944271909999161).
- **Cause: the side flags.**
  - The shim calls `EvaluateNormal(uv, TRUE, TRUE, ..)`: from the left, `Side::Left`.
  - The runner's `planes` passed `Side::Right`, and so did `local_operation`'s two
    `EvaluateNormal(.., TRUE, TRUE, ..)` calls.
  - The mid-UV lies on a knot of the cone's NURB, so the right-hand span gives
    derivatives about 1 ulp off.
- **How it reached the samples.** The replacement plane is built from that normal.
  - Its frame differed by about 1 ulp, and its origin's zero `y` carried the normal's
    residue (`cone_face0_tilt`: -7.28e-16 vs -8.10e-16).
  - The extended offset's vertex solve amplified it. The start vertex of the plane's
    line edge moved 7.86e-11 along an otherwise identical line, which is the
    `cone_face0_out` edge `t0` of 1.4e-14 vs -7.9e-11.

Fix: both the runner and `local_operation` evaluate from the left (`Side::Left`), as
upstream. The two cone known-mismatch entries are removed; no floor was added.
