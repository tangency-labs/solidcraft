//! A sketch may keep blocks (`blocks`, #4069): sets of its curves that keep their shape while
//! the set moves, turns and, with scale free, scales as one. Each block names its member curves,
//! its own construction line (the axis, saved with the sketch's other curves), its scale, the
//! axis length when the shape was captured, each member point's place in the block's frame
//! (with `loose`, the points that are members on their own), each member circle's or ellipse's
//! radius in units of that length, and the constraints it holds (kept, not solved, while the
//! block exists). `show_held`, saved only when true, draws those constraints and dimensions
//! while the block is closed; older releases ignore it. A block open for editing its contents (`open`, saved only when the design is
//! saved while its sketch is edited) holds nothing and keeps the constraints inside it that drove
//! when it was opened, the curves and points that were outside it, and its own points then. A
//! design with an open block, in a model sketch or a drawing's sketch, opens with the block
//! closed: its members are the ones it lists, its new shape is captured, and nothing the `open`
//! data leaves unlisted is taken in, so stale or hostile `open` data cannot pull other curves
//! into it. Older designs have no blocks, and nothing changes for them. The last release warns
//! about the newer format number, opens the design and keeps the blocks, but doesn't solve them:
//! there the axis is a plain construction line and every held constraint drives. A block that
//! was turned or resized conflicts with its held constraints, so that sketch fails to solve in
//! the last release until they are fixed. An open block holds nothing, so the last release reads
//! it as this version solves it while it is open.
