#### WR-90. `SmGapArray::Append` does not update the longest-gap index
- **Severity:** Wrong result
- **Area:** Healing
- **Location:** `source/SMLib/inc/SmGap.h:882`.
- **What happens:** `Append` adds the other array's gaps without comparing them with the current longest, so `GetMaxGap` / `GetMaxGapLength` can miss an appended longer gap (an empty array stays without a longest gap).
- **Trigger:** `a.Add(len 1)`, `b.Add(len 2)`, `a.Append(b)`.
- **How found:** read during porting; compared in fixture `gap_array_append`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`heal::gap::GapArray::append`).
