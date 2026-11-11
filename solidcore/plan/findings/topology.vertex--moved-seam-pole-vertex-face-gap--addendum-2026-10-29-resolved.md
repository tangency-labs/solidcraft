Updates: plan/findings/topology.vertex--moved-seam-pole-vertex-face-gap.md

# Resolved: the imported face surfaces had no face owner (2026-10-29)

Not a kernel divergence. The `topology.vertex` runner builds the Rust B-rep of an
`.smb` source from the shim's export, and that import added each face's surface without
an owner. Upstream `SmFace::SetSurface` owns it (`GetOwner() == face`).

`SmSurface::LocalPointSolve_1` takes its loose tolerance from that owner
(`SmTol::GetZoneTol3d(GetOwner())`, the face tolerance, here `1e-5`). Without an owner it
uses `SM_EFF_ZERO_SQRT * 100` (`1e-4`). After a solve converges on a domain boundary, it
jumps the seam only when the gap exceeds the loose tolerance.

On `corner_sphere_v0_off1em5` (gdb on both sides), the vertex use whose guess sits on the
seam at `u = umax` converged on that boundary with gap `1.0198e-5`:
- C++ (loose `1e-5`): `IsOnSeam` holds and the solve jumps to `u = 0`.
  `ComputeUVPoint`'s half-domain check rejects that, `GlobalPointSolve` runs, and the gap
  is `1.1007e-5`.
- Rust (loose `1e-4`): no jump, so the solution at `u = umax` is kept and the gap is
  `1.0198e-5`.

The torus cases are the same seam jump. Every step before the solve matched.

Fix (harness only): the import owns each face surface under the arena identity of the
runner's conventions, and `add_face_record` records the face tolerance on it. The 9
`corner_{sphere,torus,toruswvy}_v{0,1,2}_off1em5` entries are removed from
`crates/difftest/known_mismatches/topology.vertex.toml`. No C++ state reaches the Rust
side beyond the existing export import.
