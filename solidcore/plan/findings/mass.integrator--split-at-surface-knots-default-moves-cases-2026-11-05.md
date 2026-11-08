# mass.integrator: precise properties always integrate between the surface's knots (2026-11-05)

- **Units:** `mass.integrator` (34 known mismatches re-valued, 2 re-worded), `boolean.api_brep`
  (27 re-valued), `topology.face_p2` (5 re-valued), `blend.fillet_executive_p3` (71 new) and
  `offset.offset_executive_p2` (1 new). Every unit was already `partial`.
- **Change:** #1435 (WR-494) becomes the only path: `UpstreamFixes::precise_properties_split_at_surface_knots`
  is deleted. `SmFace::ComputePreciseProperties`' port integrates each UV trim curve between its
  knots and its crossings of the surface's u knot lines, and each iso-u integral of
  `SmODEAreaIFEO` between the surface's v knots. Upstream steps across them.
- **Why every knot, not only the C0 ones:** the integrand is built from the surface's first
  derivatives, so at a knot of multiplicity m in a direction of degree p it is C^(p-m-1):
  discontinuous where m = p (a rational circle's double knot), with a kink or a jump in a higher
  derivative elsewhere. A Runge-Kutta step's error estimate, embedded or fifth-order, is the
  leading term of an expansion that needs the integrand C^5 across the step; that holds at a knot
  only for p - m >= 6. Splitting where m = p alone removes the step underflow but leaves steps
  across C0-C4 knots accepted on an estimate that is not their error's order. The cost is the
  knot spans the steps would otherwise have shrunk toward; the trim curve's own knots were
  already breaks.
- **Regression it fixes:** with the fifth-order estimate (b3c30ed80)
  `issue1435_multispan_full_eps1e-08_orientation{0,1}_flags63` failed (SM_ERR, step underflow at
  the double v knot 0.5). They now succeed: at the requested 1e-8 Rust is 2.8e-11 off a 1e-10
  reference (largest component error over the largest magnitude), C++ 1.2e-10.

## The moved cases, against a tight reference

References are Rust's own values at a tighter accuracy (the runner's accuracy replaced, the
kernel otherwise default): solidcore checked against itself, not an independent reference.
Independent checks (closed forms, review finding 3): `blend.fillet_executive_p3`
`attributed-box-edge0-r2` round 10 pi Rust 5.3e-8, C++ 1.6e-7, end faces 96 + pi Rust -6.9e-8,
C++ -5.6e-8; `attributed-step-edge0-r1` round 5 pi Rust 5.3e-8, C++ 1.6e-7, end faces
149 + pi/4 Rust -1.5e-8, C++ -1.9e-8; `real-box-single-edge-radius-half` volume 5995 + 5 pi/4
Rust 6.3e-9, C++ 8.1e-9, centroid x Rust 1.3e-8, C++ 1.4e-8. The other entries say "checked
only against a tighter solidcore run". The figures are the largest error over the compared components
(centroid by the centroid's largest magnitude, products of inertia by the inertia's).

- `mass.integrator`, `issue1435_multispan.json` (reference 1e-10, with the piece ends evaluated
  from below, see below): Rust holds every requested accuracy: 1e-3 cases 1.2e-6 to 6.0e-5,
  1e-6 cases 7.4e-10 to 2.5e-8, 1e-8 cases 7.9e-12 to 3.1e-10. C++ misses its accuracy on
  `sub_eps0.001_*_flags1` (1.1e-3) and is at 9.1e-7 on `full_eps1e-06_*_flags1`.
- `blend.fillet_executive_p3` (requested 1e-4, reference 1e-8): Rust 4.7e-12 to 4.5e-7, C++
  5.9e-10 to 1.7e-5 (each entry gives both). E.g. `attributed-box-edge0-r2` (a quarter cylinder
  r 2 on an edge of 10, exact 10 pi): Rust 31.4159282054, C++ 31.4159314664, 5.3e-8 and 1.5e-7
  off the closed form.
- `boolean.api_brep` (requested 1e-4 areas, 1e-6 volumes; reference 1e-9), 15 of the 27
  checked: Rust 4.4e-8 to 4.2e-6, C++ 1.8e-6 to 2.0e-4 (C++ misses 1e-4 on `more0106`,
  `issue2177_crescent_on_top_of_rim_booleaned_measured`, `attributed_sphere_box_intersection`
  and `torus_guard_huge_scale_box_intersection`).
- `offset.offset_executive_p2` `issue1528_box_cylinder_api_shell_2_volume_first` (requested 1e-6,
  reference 1e-10): Rust 4.9e-12, C++ 2.7e-8.
- `topology.face_p2` `extra*` (the iso-curve integrand op): the same integral by other pieces,
  within the requested accuracy times the case's error scale, as before.
- `blend.fillet_executive_p3` `source-file-corner_box-edge9` still fails with the same values as
  before this branch (`result_geometry[212]` cpp 9.528318886810855e-1, rust 9.528316525973994e-1):
  not an integration case.

## What is left

- A piece ending at a v knot is evaluated there from the span above (upstream's
  `bVFromLeft = TRUE`), so at a C0 knot the piece's last sample is the next span's value.
  `issue1435_multispan_full_*_flags63` then fails at accuracies 1e-9 and tighter (step underflow
  approaching v = 0.5 from below), and the sub-domain at 1e-10. With
  `precise_properties_span_end_from_below` (WR-670) on, the full face holds at 1e-11. Done
  2026-11-05: WR-670 is the only path
  (mass.integrator--span-end-from-below-default-moves-cases-2026-11-05.md).
- `integrate_opt_in` (curve length) still accepts on the embedded estimate alone.
