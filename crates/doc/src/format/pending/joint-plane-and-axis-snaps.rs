//! Joint origins can snap to origin planes/axes and construction geometry.
//!
//! Adds `Snap::Plane` and `Snap::Axis` to joint snap variants. Older releases
//! do not know these snap types and ask for an update when opening a design
//! that uses them. Older designs without plane or axis joints open unchanged.