//! Designs save the sizes construction planes were dragged to (`plane_extents`). Older designs
//! have none, so their planes are drawn at their automatic size (it follows the zoom and is grown
//! past the bodies the plane crosses); an empty map is omitted. The last release opens a design
//! that has them with a newer-format warning and keeps the sizes for the next save
//! (`crate::newer_file`).
