Updates: WR-47

#### WR-47 (update, 2026-11-01). Foreign-endian `ReadBoolean` byte-swapping and `ByteSwap32(unsigned short)` opt-in fix

Both defects in the record are present in pinned v1.0.1 (`3fd5b95`) `source/SMLib/src/SmDatabaseIO.cpp`:

1. **`SmDatabaseIO::ReadBoolean` (`SmDatabaseIO.cpp:634-639`, `:632` at `c9c5979`) vs `SmDatabaseIO::ReadBooleans` (`:547-564`):** `ReadBoolean` reads `sizeof(SmBoolean)` (4) raw bytes directly into `bBoolean` without checking `m_bByteSwap`, whereas `ReadBooleans` swaps each 4-byte `SmBoolean` with `ByteSwap32` when `m_bByteSwap` is active.
2. **`SmDatabaseIO::ByteSwap32(unsigned short)` (`SmDatabaseIO.cpp:113-118`):** promotes the 16-bit `nLongNumber` to a 32-bit `unsigned int`, shifts `(nLongNumber & 0x000000FF) << 24` and `(nLongNumber & 0x0000FF00) << 8` into bits `16..31` (while `& 0x00FF0000` and `& 0xFF000000` are zero), and truncates the sum back to the 16-bit `unsigned short` return type, returning `0` for every input (only reached by `ReadShort`/`WriteShort` if `sizeof(short) != 2`).

- **What happens (default, pinned):**
  - On a foreign-endian binary stream (`FileType::ByteSwap` on little-endian x86-64/wasm32, or `set_byte_swap(true)`), `DatabaseReader::read_boolean` returns the unswapped 4-byte integer (e.g. `0x0100_0000` for big-endian `1i32` `[0, 0, 0, 1]`, `0x0200_0000` for `2i32` `UNSURE` / `SM_ORIENTATION_OPPOSITE`), disagreeing with `DatabaseReader::read_booleans` (`1`, `2`) and corrupting multi-valued `SmBoolean` fields such as `RegionData::is_void` and use orientations.
  - `byte_swap_32_u16` returns `0` for every `u16` input.
- **Fix (opt-in):** `Kernel::fixes.database_io_byte_swaps_read_boolean_and_u16`, default `false` (pinned v1.0.1 C++ parity).
  - `DatabaseReader::with_fixes` and `DatabaseReader::set_fixes` configure `DatabaseReader::byte_swap_read_boolean` from `&UpstreamFixes` (preserved across `DatabaseReader::open_for_read`); when both `reader.byte_swap()` and `reader.byte_swap_read_boolean()` are `true`, `DatabaseReader::read_boolean` byte-swaps the 4-byte `SmBoolean` via `byte_swap_u32`, matching `DatabaseReader::read_booleans`.
  - `byte_swap_32_u16_with_fixes(x, fixes)` returns `byte_swap_u16(x)` (`x.swap_bytes()`) when enabled and `byte_swap_32_u16(x)` (`0`) when disabled.
  - Whole-file entry points (`BrepData::read_from_file`, `read_part_from_file`) construct `DatabaseReader::new()` directly and are left for a separate wiring follow-up; write-side `WriteBoolean` (`SmDatabaseIO.cpp:347`) is outside `WR-47`.
- **Tests:** `wr47_flag_lookup_and_reader_configuration`, `wr47_foreign_endian_read_boolean_vs_read_booleans`, and `wr47_byte_swap_32_u16_default_zero_vs_opt_in_swap` in `crates/solidcore/src/topology/database_io.rs`.
- **Confirmed:** read at the pin (`3fd5b95`, `SmDatabaseIO.cpp:113-118`, `:547-564`, `:634-639`); default behaviour stays bit-for-bit identical to the pinned reference. `WR-47` is an archive record ID, so no numeric GitHub issue is closed here.
