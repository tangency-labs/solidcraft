#### UB-76. `ReadCurveFromDB` indexes `rAllAttributes` past its end after a failed attribute line skip [PSIRT]
- **Severity:** Crash / undefined behaviour
- **Area:** File reading
- **Location:** `source/SMLib/src/SmBrepData.cpp:5480` `SmBrepData::ReadCurveFromDB` and `:5736` `ReadSurfaceFromDB`, after `SmAttribute.cpp:260` (`GOTO_NEXT_ATTLINE` in `SmAttributeData::ReadIndexedAttributesFromDB`).
- **What happens:** When an ASCII attribute line has more than 1000 characters (or a `0xFF` byte) after its indices, `ReadIndexedAttributesFromDB` returns `SM_ERR` with the indices filled in but before it grows `rAllAttributes` and creates the placeholders. The callers ignore the status and read `rAllAttributes[index]` past the array's end (curves then `NER` a `NULL` slot; surfaces skip it).
- **Trigger:** A curve record of a version 31+ `.smb` read with attributes whose attribute line is `1 7` followed by 1001 blanks.
- **How found:** read during porting (`topology.brep_data`).
- **Confirmed:** no for the geometry readers (guarded). The record readers' behaviour (indices kept, no placeholders) is compared by the fixture `region_attribute_line_too_long_v33`.
- **Severity:** crash/UB.
- **Rust port:** Keeps the indices as upstream (`read_indexed_attributes_from_db_partial`), returns `InvalidInput` for an index past the table and `SM_ERR_NULL_POINTER` for an empty slot of a curve.
