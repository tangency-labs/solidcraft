//! A box saves the frame it stands in: `axis`, the way its height runs, and `x_axis`, the way its
//! length runs across that. Together they let a box stand on the face it was drawn on, the way a
//! cylinder already stood on one through its `axis`, instead of only ever being square to the
//! world.
//!
//! An older design has neither, and reads back as the box it always was: `axis` is +Z and
//! `x_axis` +X, which is the frame every box was built in, so nothing it holds moves. A box that
//! is still square to the world saves them all the same and is built by the same code as before,
//! down to the sheet-metal cut it takes part in.
//!
//! The last release refuses a design that has them, since the format number it carries is newer
//! than the one that release reads.
