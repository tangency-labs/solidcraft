//! Surface Sweep, Surface Loft, Offset Surface, Ruled Surface and Extend may save
//! `"stays_surface": true`: a result whose shells close (a closed profile swept round a closed
//! path, an offset of faces that close, an extension that meets itself) is a closed surface body,
//! not a solid, as Surface Revolve's has been since `surface-revolve-stays-surface.rs`; Stitch
//! makes a solid of it. New features save it. A design saved before it has no `stays_surface` on
//! these features, and a result there that closes is a solid, as it always was, so the joins,
//! cuts and measurements after it keep their result. The last release opens a design that has it
//! with a warning about the newer format number, keeps the field it doesn't read, and makes those
//! closed results solids.
