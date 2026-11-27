//! A drawing view can leave out single bodies of a part (#22107): its settings save `hide_bodies`,
//! a list of `{"path": [ids], "body": "<definition's body name>"}`, `path` `[]` for the design's own
//! bodies. A view can also draw only the bodies its design shows: its settings save
//! `shows: "design"`, and it then leaves out each body the browser's eyes hide (its own, its
//! placements', its folders' and groups'); `"all"` draws every body, as before. Both follow the
//! view's parent as `hide` does. An older drawing has neither, and each view draws every body as
//! before; nothing is rewritten, so this change has no step.
//!
//! A release from v0.48.0 on opens a drawing that has them with a warning about the newer format:
//! it sets aside each view that saves `hide_bodies` or `shows`, with the views projected or cut from
//! it and the dimensions, balloons and notes on them, lists them as not understood, and keeps them
//! unchanged through a save. Releases before v0.48.0 refuse the drawing.
