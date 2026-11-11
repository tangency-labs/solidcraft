#### UB-4. Binary short reads return success with uninitialised values [PSIRT]
- **Severity:** Crash / undefined behaviour
- **Area:** File reading
- **Location:** `SmDatabaseIO.cpp:566`/`:598` `SmDatabaseIO::ReadLong`, `:473` `SmDatabaseIO::ReadLongs` (`version >= 40` path).
- **What happens:** When the data ends, the reads land in uninitialised locals and the functions still return `SM_SUCCESS`. Callers then use the garbage, for example as counts.
- **Trigger:** A binary stream truncated inside a `long` field.
- **How found:** read during porting.
- **Confirmed:** no. The difftest compares only the status there.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.
