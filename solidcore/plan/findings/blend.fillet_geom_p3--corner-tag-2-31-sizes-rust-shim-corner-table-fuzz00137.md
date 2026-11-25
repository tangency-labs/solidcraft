# blend.fillet_geom_p3: a corner tag of 2^31 made the Rust side size its shim corner table by the tag and abort allocating 384 GiB (nightly fuzz00137) (2026-10-27)

- **Unit:** `blend.fillet_geom_p3` (`SmFilletVertex::CalcCornerVertGeom` through the runner's
  shim model). PR #1367.
- **Inputs:** nightly fuzz case `blend.fillet_geom_p3/fuzz00137` of runs 40177686438
  (2026-10-25) and 40298730423 (2026-10-26): base `gen0137`, mutations `/brep/rebuild = true`,
  `/model/vertices/0/uv/1 = -1e+300`, `/model/vertices/0/corner = 2147483648`; op `corner_vert`
  with `v: 0` on `box_with_large_fillet.smb`; the model holds one vertex (`vtype: 7`,
  `pc_class: 100`) and no solvers, geometries or edges. Verdict both nights: `crash`,
  `rust: killed by signal 6 (memory allocation of 412316860608 bytes failed)` (2026-10-25:
  `360777253032` bytes, before `FilletCorner` grew from 168 to 192 bytes).
- **Both sides:**
  - C++: not run. The FFI (`solidcore-ffi` `blend_fillet_geom_p3::eval`, `validate_model`)
    refuses the input before calling in: `invalid argument: index 2147483648 out of range
    (64)`, the vertex's corner tag read with `opt_below(rd, MAX_LIST)`. The bound exists
    because the C++ shim's corner table is a vector indexed by tag (`Model::corner_tag`
    grows it to the tag). The runner reports it as the harness error `c++: invalid
    argument: index 2147483648 out of range (64)`, so the fuzz driver runs the Rust side on
    the input on purpose (the verdict would be `rejected` had the Rust side returned).
  - Rust: `rust` -> `shim_corners` builds "one corner per tag the model uses, indexed by
    tag": `count = max tag + 1`, then `(0..count).map(|_| FilletCorner::new(..)).collect()`.
    `Vec::<FilletCorner>::from_iter` reserves the whole table up front
    (`RawVecInner::with_capacity_in` -> `malloc(412316860608)`, 2^31 x 192 bytes); Linux
    refuses the request (the fuzz driver's child runs under `ulimit -v`, and the runner
    has no 384 GiB anyway) and Rust's allocation error handler aborts the child (signal 6).
    macOS grants the request and does not enforce `ulimit -v`, so the nightly command
    (`--fuzz 300`) on the unfixed binary reports the same case as `timeout fuzz00137: rust:
    no result after 20 s` there: the child pages in the fill until the driver kills it (the
    driver starts its child through `/bin/sh`, whose SIP protection also drops a
    `DYLD_INSERT_LIBRARIES` allocator interposer). The single case replayed directly
    (`--fuzz-exec`) with `malloc` interposed to refuse requests over 1 GiB gave the
    nightly's exact message and this backtrace.
- **Cause:** the runner's Rust side had no counterpart of the FFI's corner-tag bound: the
  Rust model was built from the raw inputs while only the C++ input vector was validated.
  AGENTS.md section 1 (never pre-reserve from an input-declared count) applied to the
  harness as much as to the library.
- **Fix (harness, PR #502's convention, as #1358):** `blend_fillet_geom_p3::eval` is split
  into `check` / `validate` (every check it made before calling the shim) and the call; the
  runner's Rust side applies `validate` to the same input vector the C++ side gets (built by
  the same `model_input`), before `RustModel::build`, so the case is a harness error on both
  sides (`c++: invalid argument: index 2147483648 out of range (64)` / `invalid argument:
  index 2147483648 out of range (64)`) and `shim_corners` is never reached with an
  arbitrary tag (`fuzz` verdict `rejected`). Regression:
  `crates/difftest/tests/fuzz_corpus/blend.fillet_geom_p3.json`
  (`fuzz00137_corner_vert_corner_tag_2_31`) and the runner test
  `huge_corner_tag_is_refused_on_both_sides`. A gate fixture is not possible: a harness
  error always fails a case, and an `expected_rejection` fixture never runs the Rust side.
- **Not changed:** no kernel code, tolerance, guard threshold or fixture; `eval`'s behaviour
  is identical (an unbuilt B-rep still runs the shim on the block alone). The gate stays at
  325 cases, 318 pass, 7 rejected, 0 failed, `max_rel_err 0`.
