#### UB-91. `SmSurfOfExtrusion::ReadFromDB` reads an uninitialised generator type from a failed ASCII stream [PSIRT]
- **Severity:** Crash / undefined behaviour
- **Area:** File reading
- **Location:** `source/SMLib/src/SmSurfOfExtrusion.cpp:4560` `SmSurfOfExtrusion::ReadFromDB` (def. `:4480`), through `SmDatabaseIO::ReadType` (`SmDatabaseIO.cpp:400`); also the locals `eOrientation` and `bSwapUV` (`:4506`-`:4507`).
- **What happens:** The ASCII branch extracts the record without checking the stream. Once an extraction fails, every later `>>` leaves its (uninitialised) local untouched, and the ASCII `ReadType` returns `SM_SUCCESS` without writing `lGenCurveType`, which then selects the curve reader.
- **Trigger:** An ASCII `SmSurfOfExtrusion` record cut short, or with a non-numeric field, before its generator's `Object Type` line.
- **How found:** read during porting (`surface.surf_of_extrusion_p2`).
- **Confirmed:** no. The FFI guard only passes records the strict stream simulator of `topology.brep_data` reads completely.
- **Severity:** crash/UB (uninitialised read).
- **Rust port:** `read_type` reports the unread type and `read_from_db` returns `InvalidInput`.
