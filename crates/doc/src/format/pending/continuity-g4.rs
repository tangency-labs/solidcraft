//! A continuity order may be saved as `g4` (the curvature's change also changes smoothly across
//! the seam) wherever a design saves one (`g0` … `g3` before): a patch's, blend's, boundary's,
//! loft's or match surface's continuity. An older design has none and means what it did. The last
//! release refuses a design with this change (the newer format number) instead of misreading it.
