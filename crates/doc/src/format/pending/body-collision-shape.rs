//! A design saves a collision shape per body (`collision_shapes`, by body name): what it looks like
//! to a simulator. A body not listed exports as the export decides, and older designs list none,
//! so they export as before. The last release refuses a design with this format number.
