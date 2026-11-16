# Green-unit coverage pass (2026-10-21, `tests/green-coverage`)

- An llvm-cov run of `difftest --green` (solidcore 79.8% of lines executed; green units
  93.1%, partial 70.8%) listed the ported spans no case ran. 458 cases in
  `fixtures/<unit>/coverage.json` (44 green units) now execute many of them, each citing the
  upstream branch and the Rust line. Spans classified as Rust-only, defensive or unreachable
  from upstream callers were left alone. Recipe: build the difftest binary with
  `RUSTFLAGS="-C instrument-coverage"` into its own target dir, run `--green` with
  `LLVM_PROFILE_FILE` set, `llvm-profdata merge` and `llvm-cov export` (rustup
  `llvm-tools`), ignoring the difftest and ffi crates.
- **`curve.composite_curve_p2`: a segment transition code above 13 in a composite DB
  record.** Upstream reads it into `SmContinuityType` unchecked (14 and 15 are within the
  enum's value range, so not undefined) and carries it along; Rust refuses the record with
  `InvalidInput` ("transition code outside SmContinuityType", `composite_curve/p2.rs`), so a
  crafted file reads in C++ and fails in Rust. Left as a deliberate deviation (no fixture
  holds such a code; the generators never write one).
