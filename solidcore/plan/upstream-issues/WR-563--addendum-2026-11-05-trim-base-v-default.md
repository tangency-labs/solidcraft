Updates: WR-563

#### WR-563 (addendum 2026-11-05). The trims' lowest v is the only base; the C++ reference is wrong on the unbounded and half-open v domains

- **Change:** `UpstreamFixes::precise_properties_trim_base_v` is deleted (#1556).
  `SmFace::ComputePreciseProperties`' port (`crate::mass::integrator::properties`) always
  starts each iso-u integral of `SmODEAreaIFEO` at the lowest v of the face's UV trim curves,
  kept no lower than the face UV domain's lowest v. No path starts at the domain's lowest v.
- **Confirmed:** yes. The C++ reference (v1.0.1) is wrong on these 16 cases of
  `fixtures/mass.integrator/issue1556_unbounded_v.json`. Each is a disk of radius 0.5 (area
  pi/4) on a plane whose UV domain is unbounded or half-open in v:
  - `issue1556_disk_{unbounded,halfopen}_eps1e-06_orientation{0,1}_flags3`: C++ area
    0.7857377377804369 (+4.3e-4 relative at accuracy 1e-6) and first moment y -209.77691650390625.
    solidcore gives 0.7853981643758157 (+1.25e-9) and -0.1649336165948741, and the same disk with
    the domain bounded to (-1, 1) gives -0.16493361681648178 on the C++.
  - `issue1556_disk_{unbounded,halfopen}_eps0.001_orientation{0,1}_flags3`: C++ area
    21.9905123176286, 28 times pi/4. solidcore gives 0.7854140020563323 (+2.0e-5 at accuracy 1e-3),
    the same value as on the bounded domain.
  - `issue1556_disk_{unbounded,halfopen}_eps{1e-06,0.001}_orientation{0,1}_flags63`: C++
    returns status 1001 (`SM_ERR`) and no values. The large terms cannot meet the
    per-component error scales of the moments. solidcore returns all 26 components.
- **Bounded domains:** the disk and annulus faces with a bounded v domain whose lowest v lies
  below the trims also move: 38 cases of `mass.integrator`, each within the requested
  accuracy of the closed form, on both sides. Neither side is wrong there, so they are ordinary
  known mismatches (`plan/findings/mass.integrator--trim-base-v-default-moves-cases-2026-11-05.md`).
- **Tests:** `crates/solidcore/tests/issue_1556_unbounded_v_face_properties.rs` (the default
  kernel; it fails with the domain's base).
