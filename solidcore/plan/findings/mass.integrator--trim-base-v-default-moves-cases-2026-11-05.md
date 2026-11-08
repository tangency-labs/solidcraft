# mass.integrator: with the trims' lowest v as the only integration base, 165 cases of 4 units differ from C++ v1.0.1 (2026-11-05)

- **Units:** `mass.integrator` (38 cases), `boolean.api_brep` (53), `blend.fillet_executive_p3`
  (72) and `offset.offset_executive_p2` (2). They are known mismatches in
  `crates/difftest/known_mismatches/<unit>.toml`. `mass.integrator` and `boolean.api_brep` go
  from `green` to `partial`; the other two were `partial` already. A further 16
  `mass.integrator` cases are WR-563 itself and are parked as C++ defects.
- **Change:** #1556 deletes `UpstreamFixes::precise_properties_trim_base_v` (WR-563, and its
  addendum of 2026-11-05). Each iso-u integral now starts at the trims' lowest v. The C++
  reference starts at the face UV domain's lowest v.

## Why they move

Any base gives the same loop integral, because a different base adds a function of u alone and
its integral round a closed loop is zero. The integrands differ, though, so the adaptive
Runge-Kutta steps differ too. On a face whose domain reaches below its trims, each result moves
by an amount within the integration error that the requested accuracy allows. The moved cases
are the disk (kind 1) and annulus (kind 2) faces that `CreateFacesWithCurves` builds:
- `circular_aggregate_queries.json`: `{disk,annulus}_{face_area,face_centroid,brep_area}_eps1e-06`
  (6).
- `issue1556_unbounded_v.json`: `issue1556_disk_bounded_eps{1e-06,0.001}_orientation{0,1}_flags{3,63}`
  (8).
- `review.json`: `review_face_kind{1,2}_eps{0.001,0.0001,1e-06}_orientation{0,1}_flags{1,63}`
  (24).

## Neither side is wrong

Against the closed-form areas (pi/4 for the disk, 0.21 pi for the annulus), both sides hold the
requested accuracy:
- accuracy 1e-6: C++ -3.8e-9 to +2.8e-8, solidcore -6e-10 to +1.1e-8;
- accuracy 1e-4: C++ +1.6e-6 to +2.5e-6, solidcore -2.0e-6 to +1.2e-6;
- accuracy 1e-3: C++ -9.2e-6 to +3.2e-5, solidcore +1.3e-6 to +2.0e-5.

The largest relative difference between the two sides in any compared value is 1.75e-8 at
accuracy 1e-6, 4.5e-6 at 1e-4 and 4.4e-5 at 1e-3. The one exception is a centroid coordinate
whose exact value is 0 (`{disk,annulus}_face_centroid_eps1e-06`, values.out[1] and [2]). There
the C++ gives 2e-9 to 4.6e-9 and solidcore 4e-10 to 1.2e-9. The C++ is not wrong for these cases, so no entry carries
`cpp_defect`. The 16 unbounded and half-open v cases are WR-563 itself, where the C++ is wrong.
Those cases are parked with `cpp_defect = "WR-563"`.

## The other units

`boolean.api_brep`, `blend.fillet_executive_p3` and `offset.offset_executive_p2` compare face
areas, volumes, mass properties and the property attributes computed from them. The same
branch with only the base put back to the domain's lowest v (a local build, not committed) gives
every one of the 127 cases a pass. Every mismatch is in `values.areas`, `values.measures`,
`values.attributes`, `values.volumes` or `values.mass_properties`, and no topology or status
changes. `sweep.api_brep` and `corpus.parity` do not move.

## The integrator does not hold its accuracy, whatever the base

The largest move is a NURBS quarter disc of radius 5 (exact area 6.25 pi), a face of
`rotational_sweep_face0` and its variants in `boolean.api_brep`. Its domain is the unit square
and its trims' lowest v is 3/11. At the unit's accuracy 1e-4 the C++ gives -6.1e-6 relative and
solidcore gives -5.1e-4. A probe on that face, with every other input fixed and only the base
changed:

| base v | accuracy 1e-3 | 1e-4 | 1e-5 |
|---|---|---|---|
| 0 (the domain's) | -5.1e-3 | -6.1e-6 | -7.7e-7 |
| 0.2 | -4.2e-3 | -1.6e-6 | -1.1e-7 |
| 0.25 | -4.5e-3 | -4.9e-4 | -7.2e-8 |
| 3/11 (the trims') | -4.9e-3 | -5.1e-4 | -5.9e-8 |
| 0.3 | -3.6e-3 | -4.3e-4 | -3.9e-8 |
| 0.5 | -4.8e-5 | -1.1e-6 | -6.0e-8 |
| 1.0 | -3.8e-7 | +1.2e-7 | -8.8e-8 |

Both bases converge to 6.25 pi (-1.9e-14 at 1e-10). At 1e-3, the domain's base itself misses
by five times the accuracy. The adaptive error control does not bound the result's error on
this face. Which base lands outside the accuracy at a given setting is arbitrary. This is a
separate defect of the precise-property integration and is not caused by this change: the
integration-base fix stands, and the error control is a follow-on item.

Resolved by the follow-on (plan/findings/mass.integrator--quadrature-step-estimate-moves-cases-2026-11-05.md):
a stale integrand where the trim's du/dt is zero, and a step error estimate blind to g^(5).
