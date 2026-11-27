//! Joint lateral offsets (`offset_x`, `offset_y`) keep their parameter expressions in
//! `JointExprs` (`offset_x`, `offset_y`, omitted when absent) and re-evaluate on parameter
//! changes, matching `offset` and `angle`.
//! Older designs without these fields default both to `None` and keep their saved `offset_xy`.
//! The last release warns about the newer design format, opens what it understands and keeps
//! the unfamiliar fields when saving.
