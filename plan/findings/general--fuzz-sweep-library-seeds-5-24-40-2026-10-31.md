# General: fuzz sweep of the blend, boolean, sweep, offset, topology and heal units with new seeds (2026-10-31)

Unit: 81 green/partial units of the `blend.*`, `boolean.*`, `sweep.*`, `offset.*`,
`topology.*` and `heal.*` families (every one in `plan/units/` except those other work
owns: `sweep.api_brep`, `topology.face_p2`, `topology.edgeuse_p2`, `topology.edgeuse_p3`,
`topology.brep_p7`, `blend.fillet_geom_p3`). Branch `claude/fuzz-sweep-library`, base
`origin/main` 6284ffcdd.

**Runs.** The nightly runs `--fuzz 300` with the default `--seeds 16`. Fuzz case `i` is
seeded by `fnv1a64("fuzz:" + unit) + i` and picks its base case from the unit's normal
cases, so a different `--seeds` changes the cases only for units with generated cases;
units whose cases are all fixtures get the nightly's cases again. The sweep therefore ran
three passes, each under `ulimit -v 16000000` and `timeout 2400` (the child's own 4 GiB
limit and 20 s case timeout unchanged):

```sh
difftest --unit <u> --fuzz 300 --seeds 5
difftest --unit <u> --fuzz 300 --seeds 24
difftest --unit <u> --fuzz 600 --seeds 40   # cases 300..599 are new for fixture-only units
```

That is 81 x 1200 = 97 200 fuzz cases. Between the first two passes the verdict counts
changed for 27 of the 81 units (the seed reached their cases); the other 54 got the
nightly's 300 cases again in passes 1 and 2 and 300 new ones in pass 3.

**Library (crates/solidcore): no Rust-side crash, panic or allocation abort.** Every Rust
`crash` verdict was a runner fault (below); no `panic` verdict on either side.

**Runner faults fixed (PR #502's convention: the Rust side applies the FFI's check).**

| unit | seeds | cases | class | cause |
|---|---|---|---|---|
| topology.brep_validation | 5, 24 | 17 (e.g. fuzz00074) | crash: rust abort `memory allocation of 32 bytes failed` | `grid_faces` 2^31..i64::MAX; FFI refuses > 512, the runner's `sheet_grid` built faces until 4 GiB |
| topology.shell | 5, 24, 40 | fuzz00105, 00146, 00180, 00230, 00238, 00241, 00288 | timeout (rust) | `brep.networks` 2^31..i64::MAX; the export refuses > 3, the runner's builder wired networks |
| topology.vertexuse | 24 | fuzz00086 | timeout (rust, 34 s) | control point `1e300`; export refuses (HANG-29), Rust built the face |
| topology.face_p3 | 24 | fuzz00127 | timeout (rust, 42 s) | as vertexuse (HANG-29) |
| topology.brep_p4 | 5, 24, 40 | fuzz00285 | timeout (rust, 33 s) | weight `1e-300` among `-f64::MAX`; export refuses (mixed rationality) |

Each fix runs the FFI's pure-Rust validation (`topology_brep_validation::validate`, new;
`topology_edgeuse_p1::validate_export`, new; `topology_vertexuse::validate_block`) on the
block `run_cpp` sends, before the Rust side builds anything. Regressions in
`crates/difftest/tests/fuzz_corpus/<unit>.json`.

**C++ crashes (upstream; replayed Rust-only with `cpp_skip`: the Rust side returns `ok` on
the first three, and the offset runner's Rust side refuses fuzz00560 before the library
runs: `automatic B-rep tolerance is outside this Rust fixture projection`).**

| unit | seeds | case | mutation |
|---|---|---|---|
| topology.brep_p8 | 40 | fuzz00307 | `make_faces` with `±f64::MAX` uv weights, `/curves/2/uv/cpts/1/1 = -0.0` |
| topology.proto_topology_p2 | 40 | fuzz00353 | B-spline knot `u[9] = -0.0` (non-monotone knots) |
| topology.trimming_tools | 40 | fuzz00586 | `order` of an ellipse whose `y` axis has `-1e300` |
| offset.offset_executive_p2 | 40 | fuzz00560 | `extended_sphere_outset_huge`, `member_approx_tol = 1e-300`, `tolerance = 0` |

These are FFI guard gaps (the C++ reference segfaults); not changed here.

**Rust timeouts left open (library speed, both sides agree where both run).**

- `topology.brep_mirror` nightly fuzz00100 (origin `-1e300`): C++ 3.3 s, Rust 59 s, same
  result. Time is in `SurfaceCache::subdivide_to_tolerances_with` under
  `create_uv_trim_curves` -> `drop_curve_trace` after the mirror (HANG-15 regime:
  bounded by `MAX_SUBDIVISION_NODES`).
- `blend.fillet_executive_p3` fuzz00085 (all three passes; `two_offset_chamfer` origin
  `1e300`): the C++ setup fails fast (`no edge at the rim height`), the Rust setup's
  `boolean_difference` spends > 150 s and ~1 GB in `SurfaceCache` subdivision under
  `BrepQuery::classify`. Same regime.
- `topology.api_projection` (seed 5 fuzz00094; 24 fuzz00237; 40 fuzz00106, 00298, 00444):
  silhouettes of `nurb_fillet.smb`, near-unmutated inputs; C++ 0.5 s, Rust 16-27 s. Time
  is in `Hcr::is_point_visible` -> `RayTracer::add_surface_with_fetched_cache` ->
  `SurfaceCache::compute_polar_box` (polar boxes recomputed per visibility query?). Not
  investigated further.
- `offset.offset_executive_p2` fuzz00013 (24), fuzz00475 (40): C++ 4.5 s, Rust 12-22 s.
- `topology.brep_p3` nightly fuzz00282: already recorded in
  `plan/findings/topology.brep_p3--turn-nurbs-offset-1e300-rust-side-runs-25-s-fuzz00282.md`.

C++ timeouts seen (upstream): `topology.brep_constructor` fuzz00128, `boolean.api_brep`
fuzz00473, `offset.offset_executive_p2` fuzz00092, fuzz00317 (seed 40). The nightly's
`boolean.merge_p2` fuzz00291 (a C++ SIGSEGV only after 91 preceding cases) did not recur
under these seeds.
