//! A ruled surface saves the handles of its free strips' far curves: `far`, per chain edge, the
//! interior control points of the curve a strip whose rulings turn freely along its edge is ruled
//! to (or null for the kernel's default curve). An older design has none and gets the default
//! curves. The last release refuses a design with this change (the newer format number).
