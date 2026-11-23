//! A canvas saves `anchor: {sketch, bottom_left, bottom_right, top_left?}`: the sketch points
//! the image's corners follow, so that sketch's dimensions and constraints place and size it,
//! and stretch it when its top-left corner is a point too (`canvas.frame_sketch` makes one;
//! `canvas.edit` sets or clears it).
//!
//! An older design has none: its canvases and decals keep their own centre, width and angle.
//!
//! The last release opens a design with an anchored canvas with a warning about the newer
//! format, shows the canvas where its own centre, width and angle place it, and keeps the field
//! it can't read for the next save (`crates/doc/src/newer_file.rs`).
