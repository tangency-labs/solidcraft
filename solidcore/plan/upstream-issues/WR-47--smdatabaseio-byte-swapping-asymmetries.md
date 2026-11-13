#### WR-47. `SmDatabaseIO` byte-swapping asymmetries
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `SmDatabaseIO.cpp:632` `ReadBoolean` (never byte-swaps), `:547` `ReadBooleans` (does). `ByteSwap32(unsigned short)` always returns 0.
- **What happens:** Booleans read singly from a foreign-endian file come out wrong.
- **Trigger:** A big-endian binary file read on x86-64.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Kept as is.
