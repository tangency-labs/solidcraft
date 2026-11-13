//! Designs save frame_end_cap features: the capped end faces as persistent face anchors with
//! their members' feature and line, the thickness, the inset as a length or a ratio of the
//! thickness, the corner chamfer and the direction. Older designs have no end caps and need no
//! migration. The last release refuses this newer format rather than dropping the cap bodies.
