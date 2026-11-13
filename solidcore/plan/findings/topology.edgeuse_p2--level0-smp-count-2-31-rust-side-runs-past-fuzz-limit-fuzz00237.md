# topology.edgeuse_p2: an `IsEdgeuseSideInSurface` sample count of 2^31 kept the Rust side sampling where the FFI refuses the C++ (nightly fuzz00237) (2026-10-30)

- **Unit:** `topology.edgeuse_p2` (`SmEdgeuse::IsEdgeuseSideInSurface`, the level-0
  `edgeuse_side` cases the runner delegates to `topology_face_p4::level0`).
- **Inputs:** nightly fuzz case `topology.edgeuse_p2/fuzz00237` of run 41040730716
  (2026-10-30): base `level0_surf_sphere150.smb_0_op3`, mutations `/level0/periodic_u = -1`,
  `/brep/rebuild = true`, `/level0/smp_count = 2147483648`; `surf_sphere150.smb` with
  rebuilt UV trim curves, `periodic_u = periodic_v = -1` (NULL). Verdict: `timeout`,
  `rust: no result after 20 s`.
- **Both sides:**
  - C++: not run. The FFI (`solidcore-ffi` `topology_edgeuse_p2::pole`) refuses the input
    before calling in: `invalid argument: too many samples` (`smp_count` above
    `MAX_POLE_SAMPLES`, 64). The runner reports it as the harness error `c++ level-0
    helper: invalid argument: too many samples`, so the fuzz driver runs the Rust side on
    the input on purpose.
  - Rust: `level0::rust_record` calls `topology::edgeuse::p2::is_edgeuse_side_in_surface`
    with `smp_count = 2^31` for every edge use; the port, faithful to upstream, drops that
    many points of the edge curve onto the surface (`for ii in 0..smp_count`), a time loop
    with no allocation sized by the count, and ran past the driver's 20 s limit.
- **Cause:** a port faithful to upstream asked for 2^31 samples by a fuzzed harness input
  the FFI keeps the C++ out of (the class of issue #504). Not a library defect: the count
  is the caller's explicit work request, the loop is linear and finite, and the only
  library caller (`topology::loopuse::is_loopuse_side_in_surface`) passes upstream's
  constant 6. The library is unchanged.
- **Fix (harness, PR #502's convention):** `topology_edgeuse_p2::pole`'s checks are split
  out as `validate_pole(path, smp_count, periodic_u, periodic_v, block)` (pure Rust: the
  B-rep block is checked by `topology_vertexuse::validate_block`, no C++ call); `pole`'s
  behaviour is identical. The level-0 runner's Rust side applies `validate_pole` to the
  same arguments and B-rep block as `run_cpp` before building its B-rep, so the case is a
  harness error on both sides (`level-0 helper: invalid argument: too many samples`).
- **Regression:** `crates/difftest/tests/fuzz_corpus/topology.edgeuse_p2.json`
  (`fuzz00237_edgeuse_side_smp_count_2_31`) and the runner tests (in
  `topology_face_p4/level0.rs`) `huge_sample_count_is_refused_on_both_sides` and
  `max_sample_count_runs_on_both_sides`. A gate fixture is not possible: a harness error
  always fails a case.
