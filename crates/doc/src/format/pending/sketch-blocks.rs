//! A sketch may keep blocks (`blocks`, #4069): sets of its curves that keep their shape while
//! the set moves, turns and, with scale free, scales as one. Each block names its member curves,
//! its own construction line (the axis, saved with the sketch's other curves), its scale, the
//! axis length when the shape was captured, each member point's place in the block's frame,
//! each member circle's or ellipse's radius in units of that length, and the constraints it
//! holds (kept, not solved, while the block exists). Older designs have no blocks, and nothing
//! changes for them. The last release warns about the newer format number, opens the design and
//! keeps the blocks, but doesn't solve them: there the axis is a plain construction line and every
//! held constraint drives. A block that was turned or resized conflicts with its held
//! constraints, so that sketch fails to solve in the last release until they are fixed.
