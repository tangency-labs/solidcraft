#### UB-75. `SmBrepData` readers use uninitialised locals and unchecked `SetSize` [PSIRT]
- **Severity:** Crash / undefined behaviour
- **Area:** File reading
- **Location:** `source/SMLib/src/SmBrepData.cpp:2933` `SmBrepData::ReadFromDB` (section count `lCount` uninitialised at `:2942`; every `SetSize(lCount)` unchecked); `:3245` (`lEdgeCount`, `lEdge` uninitialised in the composite edge loop); `:5098` `SmEdgeData::ReadFromDB` (`dMin`, `dMax` uninitialised); `:3613` `SmBrepData::ReadFromFile` (`lDBVersionNumber` uninitialised when the ASCII header extraction stores nothing; the binary `ReadLong` status is ignored); `:5393` `ReadCurveFromDB` composite branch (`lNumSegments`, `lDimension`, `lClosed`, `dGapStart`, `dGapEnd`).
- **What happens:** After a failed ASCII extraction (stream at its end, or a non-numeric token) these locals keep indeterminate values and drive loops and allocations. `SmTArray::SetSize` refuses sizes of `SM_BIG_ULONG` (99999999) or more and `SM_UNDEF_ULONG` and leaves the array empty or `NULL`, but the record loops still index it up to the count; record `ReadFromDB` statuses are ignored.
- **Trigger:** A truncated ASCII `.smb`; a section count of `100000000` with a few records after it.
- **How found:** read during porting (`topology.brep_data`); UB-1 is one instance.
- **Confirmed:** no (the difftest guard keeps such streams away from the C++ side), except through UB-1.
- **Severity:** crash/UB.
- **Rust port:** Uses `0` for the uninitialised locals, rejects counts the remaining input cannot hold (`InvalidInput`), appends records as they are read, and stops a record loop with `InvalidInput` once the stream has failed or hit end of data before the declared count (otherwise each further record would consume no input and the count bound would not cap memory).
