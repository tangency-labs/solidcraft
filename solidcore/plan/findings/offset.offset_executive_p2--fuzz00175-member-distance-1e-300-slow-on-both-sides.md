# offset.offset_executive_p2: fuzz00175 (member distance 1e-300) takes over 20 s on both sides (2026-11-02)

- **Unit / case:** `offset.offset_executive_p2`, nightly fuzz `fuzz00175` (run 41438521478, `rust: no result after 20 s`): base `mate_two_curves_problemShell15_face8_outset` with `/config/member_distance = 1e-300`.
- **Both values:** the case finishes on both sides and is compared (`ran`). Replayed alone on main `340782351` (x86-64): C++ 20.9 s, Rust 31.5 s. A local `--fuzz 300` run reported it as `cpp: no result after 20 s`, so which side crosses the fuzz's 20 s depends on the machine.
- **Where the time goes (gdb samples):** the same place on both sides, `SubdivideToTolerances` of the edge-use offset surfaces' caches under `DoExtendedOffset` -> `BuildOffsetOfEdgeuses` -> `EdgeuseOffset`.
- **Not a Rust hang:** the C++ also takes over 20 s, and every Rust loop on the path is bounded. #2393 leaves it. If the nightly keeps flagging it, the options are an FFI bound on the member distance on both sides (with an upstream-issues record, if the C++ time is judged a defect) or a longer fuzz timeout for this unit.
