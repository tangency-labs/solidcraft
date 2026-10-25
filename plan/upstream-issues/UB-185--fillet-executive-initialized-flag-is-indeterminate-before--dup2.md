#### UB-185. Fillet executive initialized flag is indeterminate before parameter setup
- **Severity:** Crash / undefined behaviour
- **Area:**
- **Unit:** `blend.fillet_executive_p3` (constructor owned by executive p1).
- **Location:** `source/SMLib/src/SmFilletExecutive.cpp:487-516,1574`.
- **Trigger:** Construct `SmFilletExecutive` and call `DoFillet` before parameter
  setup. Its constructor never initializes `m_bIsInitialized`, but `DoFillet`
  immediately reads that member for its precondition check.
- **How found:** Source inspection while porting the three executive methods.
- **Confirmed:** no; no uninitialized native read was executed.
- **Guard:** The native fixture's derived executive explicitly writes
  `m_bIsInitialized` for every case before invoking an operation. Rust requires
  callers to supply an initialized `bool` in the executive state.
