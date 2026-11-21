Updates: WR-47

#### WR-47 (update, 2026-11-14). `database_io_byte_swaps_read_boolean_and_u16` is removed

`DatabaseReader::read_boolean` now byte-swaps whenever the stream's byte swap is active, as `read_booleans` does (`SmDatabaseIO.cpp:547`). Pinned SMLib's `ReadBoolean` (`:634`) ignores `m_bByteSwap`, so a byte-swapped stream's `1` read as `0x0100_0000`. `byte_swap_32_u16` now swaps the two bytes; pinned `ByteSwap32(unsigned short)` returns `0` for every input. The fix is unconditional and the flag is gone.

Removed:
- the field `UpstreamFixes::database_io_byte_swaps_read_boolean_and_u16`;
- SolidCraft's preset line;
- its entries in `fixes.json` and in the SolidCraft preset list of `issue_2260_gn_pipe_refined_range_end.rs`;
- the reader's opt-in field with `DatabaseReader::with_fixes`, `set_fixes` and `byte_swap_read_boolean`;
- `byte_swap_32_u16_with_fixes`.

No reader outside the module ever configured the opt-in. So before, every B-rep read took the pinned path, whatever the kernel's flag.

- **Who sees it:** a reader byte-swaps only for an `SM_BYTESWAP` file, or on a big-endian machine. SolidCraft reads and writes `.smb` files as ASCII or binary on little-endian targets (x86-64, aarch64, wasm32), so its files read the same. Only foreign-endian binary data changes. Such data now reads its booleans as written: a region's `is_void` of `1`, for example, reads `1`, not `0x0100_0000`.
- **Results:** 62 rows of the goldens unit `topology.database_io` move, all `read_boolean` steps on a byte-swapped stream. Each value becomes the 32-bit byte swap of the old one, and nothing else in the row changes. The fixture `byteswap_be` reads the bytes `00 00 00 01` of an `SM_BYTESWAP` stream as `1` (old `16777216`).
- **Tests:** `crates/solidcore/src/topology/database_io.rs`: `wr47_foreign_endian_read_boolean_vs_read_booleans` (the flag-off halves removed; `RegionData`'s big-endian `is_void` reads `1`), `wr47_byte_swap_32_u16_swaps_the_two_bytes`. Goldens: `topology_database_io::tests::read_boolean_reads_as_read_booleans_of_one` checks that every case of the unit records the same output with each `read_boolean` replaced by `read_booleans 1`.
