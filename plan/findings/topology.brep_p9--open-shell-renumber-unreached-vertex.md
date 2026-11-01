# topology.brep_p9: `Fillet/open_shell.smb` fails building the Rust input with `renumber: unreached vertex` (2026-11-01)

- **Unit:** `topology.brep_p9` runner (`crates/difftest/src/units/topology_brep_p9.rs`). This is a harness input-build
  failure, not a Rust-vs-C++ disagreement in any ported function.
- **Inputs:** any p9 case on `{"kind": "file", "file": "Fillet/open_shell.smb"}`.
  - Reproduced on op 0 (`FindInfiniteRegion`), on op 1 (`OrientSheetSurfaces`, selector 0, `same` true and false) and on
    op 4 (`MakeVertexSplitEdge`, selector 0, variant 0, parameter 0, tolerances `[0.001, 0.001]`).
  - Minimal repro: `{"name": "open_shell_op0", "inputs": {"op": 0, "brep": {"kind": "file", "file": "Fillet/open_shell.smb"}}}`
    in a fixture file under `fixtures/topology.brep_p9/`, then
    `cargo run --profile difftest -p difftest --features cpp -- --unit topology.brep_p9`.
- **Values:** the C++ side runs. The Rust side fails every op with the harness error `rust: renumber: unreached vertex`, so
  nothing is compared.
- **Where:** the error is raised while the Rust input is built, before any op runs:
  - `own_builder::read_with` (`crates/difftest/src/own_builder.rs:499`) calls `renumber_with` (`:1504`).
  - `renumber_with` calls `renumber_walk` (`:1534`), whose vertex mapper fails at `:1432`.
  - So a reached object refers to a vertex that `walk_order` did not reach. The same message on every op shows it is
    unrelated to `OrientSheetSurfaces`.
- **The file:** the Rust reader reads it as a one-shell sheet of 6 faces with `is_sheet` and `is_oriented_sheet` both true.
  That was established by a throwaway scan of `pt_TestFiles` with `topology::brep::p2::is_sheet` /
  `is_oriented_sheet`; the scan is not committed.
- **Suspected cause:** not investigated. The Rust-side traversal (`walk_order` under the p9 runner's
  `Conventions::EDGEUSE_GRAPH`) misses a vertex that the imported topology references. Either the Rust reader imports this
  file's topology differently, or the walk does not cover a structure the file has, such as a vertex reached only through a
  path the walk skips. Only the p9 runner was checked.
- **PR:** found while adding the multi-face `OrientSheetSurfaces` cases (#2137), which leave this file out.
