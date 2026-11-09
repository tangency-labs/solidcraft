//! An edge flange feature's `position` can also be `material_outside` (the inner virtual sharp on
//! the picked edge, so at 90° the flange's inner face is flush with the edge's face) or `tangent`
//! (the bend's outer face tangent to the edge's face; below 90° the bend's end on that face),
//! #5941. Older designs need no step: their
//! positions (inside, outside, middle) are saved and built as before. The last release refuses a
//! design that has them (it refuses the newer format number).
