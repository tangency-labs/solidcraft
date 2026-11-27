//! A drawing view can draw only the bodies its design shows (#22107): its settings save
//! `shows: "design"`, and it then leaves out each body the browser's eyes hide (its own, its
//! placements', its folders' and groups'); `"all"` draws every body, as before. It follows the
//! view's parent as `hide` does. An older drawing has no `shows`, and each view draws every body as
//! before; nothing is rewritten, so this change has no step.
//!
//! The last release opens a drawing that has it with a warning about the newer format: it sets
//! aside each view that saves `shows`, with the views projected or cut from it and the dimensions,
//! balloons and notes on them, lists them as not understood, and keeps them unchanged through a
//! save.
