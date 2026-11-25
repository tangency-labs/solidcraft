Updates: WR-655

#### WR-655 (update, 2026-11-14). `ascii_doubles_round_trip` is removed; B-rep files keep every double

`write_brep_to_file` (`BrepData::write_to_file_with_nurbs`) always writes an ASCII file's finite doubles in the shortest
text that reads back as the same double (`database_io::round_trip`). The opt-in described in the 2026-11-01 addendum no
longer exists. Removed: the field, SolidCraft's preset line, and its entries in `kernel_cases/fixes.json` and the
`issue_2260_gn_pipe_refined_range_end.rs` flag list. Kept: `DatabaseWriter` still writes upstream's `%.16f` unless made
`with_round_trip_doubles`, and `BrepData::write_to_file` (the upstream-format writer the unit tests compare
against the reference's text) still writes `%.16f`. Binary files are unchanged.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes.
- **Goldens:** the 114 ASCII `write_bytes` rows of `topology.api_files` move (their file text). No binary row moves.
  `topology_api_files::tests::ascii_files_carry_every_double_exactly` checks every such case: each finite double in the
  file is the shortest text that reads back as itself, and the file read back and written as a binary file gives the same
  bytes as the case's binary file read back and written the same way.
- **Non-finite values:** `inf` and `nan` are still written as `fixed16` text, which the reader does not take back;
  that predates this change.
- **Tests:** `wr655_ascii_doubles_round_trip.rs`'s B-rep test runs on the default kernel;
  `by_default_the_file_is_upstreams_and_moves_the_corners`, which asserted the loss, is removed.
