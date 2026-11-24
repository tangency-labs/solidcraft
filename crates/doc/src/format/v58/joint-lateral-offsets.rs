//! Joint offsets along the attachment frame's x and y are saved separately as `offset_xy`.
//! Designs without the field receive zero additional lateral offsets and retain their saved
//! attachment frames, including older frames with lateral offsets already baked into them.
//! The last release warns about the newer design format, opens what it understands and keeps
//! the unfamiliar fields when saving.
