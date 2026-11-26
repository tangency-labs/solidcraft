//! A Surface Revolve may save `"stays_surface": true`: a full turn that closes its walls (a
//! closed profile, or a curve from the axis back to it) is a closed surface body, not a solid,
//! as every other surface operation's result is a surface body; Stitch makes a solid of it. New
//! surface revolves save it. A design saved before it has no `stays_surface`, and a closed full
//! turn there is a solid, as it has been since Surface Revolve was added, so the joins, cuts and
//! measurements after it keep their result. The last release opens a design that has it with a
//! warning about the newer format number, keeps the field it doesn't read, and makes those closed
//! full turns solids.
