Updates: WR-589

#### WR-589 (update, 2026-11-04). Two upstream test files trigger it

Two more `topology.brep_p10` cases from #1828 hit the V pass's U-knot count, on upstream test files rather than crafted sheets. Both bicubic sheets have more V knots than U knots, and every V knot is a simple knot: the surface is C2 there and its third V derivative jumps (by 1.4 to 1.8 times its size).

- `subdivide_surf_rev_cinf` (`surf_rev.smb`, `SM_CT_CINFINITY`): one U knot (u = 11.71) and three V knots (v = 7.65, 15.53, 23.41). The C++ reference cuts at the U knot and the first V knot only: 4 faces, with the knots at v = 15.53 and 23.41 left inside them. solidcore cuts along all four: 8 faces.
- `subdivide_nurb_nurb_2_cinf` (`Fillet/nurb_nurb_2.smb`, `SM_CT_CINFINITY`): no U knot and three V knots (v = 8.89, 16.67, 25.56). The C++ reference makes no cut: 1 face. solidcore cuts along the three: 4 faces.

**Confirmed:** yes. With the Rust V pass limited to the U knots' count, as upstream does, after the no-discontinuity return, both cases and the parked `subdivide_u1v2_cinf` and `subdivide_u0v1_cinf` equal the C++ reference bit for bit (`max_rel_err=0`). That was a local diagnostic and is not committed. The SolidCraft fork (tangency-labs/smlib@6eecf04) counts the V knots, as solidcore does. The two cases are known mismatches with `cpp_defect = "WR-589"` for as long as `topology.brep_p10` is gated on the frozen v1.0.1 difftest. `crates/solidcore/tests/subdivide_face_v_knots.rs` checks the knots, the jump of the third V derivative at each V knot, the vertices at both ends of each knot's iso line and the face counts (8 and 4).
