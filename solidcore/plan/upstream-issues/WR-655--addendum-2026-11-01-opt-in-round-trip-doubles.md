Updates: WR-655

#### WR-655 (update). Opt-in ASCII doubles that read back unchanged

- **Rust port:** `Kernel::fixes.ascii_doubles_round_trip` (off by default) makes `write_brep_to_file` (`BrepData::write_to_file_with_nurbs`) write every ASCII double in the shortest form that reads back as the same double, in exponent notation (`3.0000000000000004e-1`; `inf`, `-inf`, `nan` and `-nan` as before), instead of the `%.16f` text the C++ writer produces. The text comes from `database_io::round_trip`, chosen per writer with `DatabaseWriter::with_round_trip_doubles` and `BrepData::write_to_file_with`. The ellipse's polar conversion rows, which formatted their doubles directly, go through the same switch (`DatabaseWriter::f64_text`). Every ASCII reader reads both forms: `>>` takes the exponent.
- **Default:** unchanged. `fixed16` writes the same bytes as the C++ writer, and the difftest still compares them.
- **Regression:** `crates/solidcore/tests/wr655_ascii_doubles_round_trip.rs` checks four things.
  - The record's example: `0.1 + 0.2` is written `0.3000000000000000` by default and `3.0000000000000004e-1` with the field.
  - A line of doubles (`0.1 + 0.2`, 1/3, 2/3e-5, 1e-300, ±0, 123456789.123, -1/7, `f64::MAX`) reads back bit for bit with the field. By default, `0.1 + 0.2` reads back as 0.3, 1e-300 as 0, and -1/7 moves; the values of 1 and above keep their 17 digits.
  - A box whose corners are at such doubles, written as an ASCII B-rep file and read back with the field on, has the same corners bit for bit.
  - By default, the same file holds `%.16f` text and its corners move.
- **Scope:** only the text of ASCII doubles changes when the field is on. Binary files, integers and the file layout are unchanged. The reader is untouched.
