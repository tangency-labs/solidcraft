#### UB-38. Point trees divide by zero in `AddToSpatialTree`
- **Severity:** Crash / undefined behaviour
- **Area:** Spatial structures and small geometry
- **Location:** `SmTree.cpp:255` `SmTree::AddToSpatialTree` (point-tree constructors at `:110`/`:164`).
- **What happens:** A point tree never initialises its object-list memory manager, so adding an object divides by zero.
- **Trigger:** `SmTree(point, size, ...)` then `AddToSpatialTree`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Refuses the operation.
