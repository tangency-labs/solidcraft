# topology.brep_data (`SmBrepData` readers, 2026-10-13)

- **Records start zero-filled.** `SmTArray::SetSize` `calloc`s / `memset`s new elements
  without running constructors, so every `Sm*Data` record read by `ReadFromDB` starts
  with all fields `0` (index fields, `UNSURE` flags, extents `[0,0]`, points `(0,0,0)`),
  not the header's member defaults. It matters where a field is not read (unknown shell,
  loop or edge use types; the edge interval and face domain when `SetMinMax` refuses
  `min > max`). The port's `*Data::zeroed()` reproduce it; `Default` is the C++
  constructor. Confirmed by the `extreme_values_v30` and `unknown_types_*` fixtures.
- **Tolerance build.** The reference build is `SM_USE_OLDTOL`: zone tolerances are read
  only for `version <= 43`, the `SM_USE_NEWTOL` fields (`m_bIsSmallTopology`, model size
  estimate) never.
- **Statuses.** Every record read and `ReadIndexedAttributesFromDB` status is ignored
  upstream. A failed ASCII attribute line skip (over 1000 characters, or a `0xFF` byte)
  keeps the indices read but creates no placeholders; `geom.attribute` gained
  `read_indexed_attributes_from_db_partial` for that (geom.attribute difftest unchanged).
- **FFI guard.** `solidcore_ffi::units::topology_brep_data` simulates the whole read and
  rejects any stream on which an extraction would fail or run short (upstream then reads
  uninitialised locals, UB-75), counts of `SM_BIG_ULONG` or more, geometry attribute
  indices past the table after a failed line skip (UB-76), closed composites without
  segments (UB-77), and geometry kinds it does not model (B-spline curves/surfaces,
  version <= 32 composites and offsets, type code 1). Upstream test files (`.smb` under
  `TestFiles/pt_TestFiles`, 186 read with `ReadFromFile`, 170 also with `ReadFromDB`, and
  the two part files' B-reps at version 32) are trusted past the first unknown kind.
- **Record loops stop on a failed stream (security review, 2026-10-13).** The one-byte
  per record bound (`checked_count`) assumed every record consumes input, but records
  ignore their read status (UB-75): once the stream fails or hits end of data each
  further record consumes nothing and is still appended zero-filled, so a count close
  to the remaining byte count allocated `count * size_of::<record>()` (a 100 MB file
  could cost tens of GB, per array). `ensure_more` now checks the stream before each
  record, composite item and old composite segment and returns `InvalidInput`
  ("stream ended before all declared records were read"). It runs before a record,
  never after the last, so a stream whose final record ends exactly at end of data
  (eofbit set, no trailing newline) reads fully. Only streams that are undefined
  upstream (UB-75) change; the difftest results are unchanged.
- **Deferred.** Offset surfaces (`surface.offset_surface`) and, in version 38 files,
  planes, cones and extrusions (`surface.plane_p2`, `surface.cone_p1`,
  `surface.surf_of_extrusion_p2`) are `Unported` on the Rust side.
- **Old composite curves.** Composite curve records of version <= 32 pass `n + 1` gaps
  (first start gap, then each end gap) to a constructor that reads `n`: the segment gaps
  come out shifted (WR-54). Ported as is. Their evaluation returns `SM_ERR` on both
  sides in the fixtures.
