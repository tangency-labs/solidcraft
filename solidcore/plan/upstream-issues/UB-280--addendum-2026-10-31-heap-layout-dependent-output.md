Updates: UB-280

#### UB-280 (update, 2026-10-31). When the crash does not happen, the result depends on the heap layout
- **Location:** as UB-280 (`source/SMLib/src/SmFeatureExecutive.cpp:1922`, `SmFeatureExecutive::ExtSurfaceRebuild`).
- **What happens:** the defect is the same (a call through a NULL function pointer). Whether it crashes, and what the C++ returns when it does not, depend on the heap layout the case runs on. The same input gives a crash in one build or run, a result in another, and different results in different builds.
- **Trigger:** as UB-280. Shown by `blend.feature_executive` cases `single_face_box_with_large_fillet_2` and `single_face_box_with_large_fillet_4` (`box_with_large_fillet.smb`, `AddFeature({face 2})` or `({face 4})`, `DoDefeature`).
- **How found:** two builds of the difftest harness that differ only in harness and Rust code the C++ never reaches (batch 53, 2026-10-31). Each case's C++ side runs in a child process forked from the harness, so its heap starts from the parent's layout.
  - The Rust outputs are identical in both builds: Rust-only digests of all 149 shared cases of the unit.
  - The C++ digests of `_2` (`57cad26e21ae7a03` in one build, `001632979ce91340` in the other) and of `_4` (`001632979ce91340`, then `9d323b9ff9c10966`) differ. Each is stable across repeated runs in one build.
  - Run alone, `_2`'s C++ side returns a result in one build and crashes in the other.
  - In the second build, `difftest --heap-pads 1,3,8` flags `_4` as address-dependent: one padded run gives a different record.
- **Confirmed:** yes, on the C++ build (batch 53). `_2` is parked with `cpp_defect = "UB-280"`; `_4` stays counted, see the known-mismatch file.
