//! A point-to-point Align can save `axes`: the directions it moves along, as the chosen world
//! X/Y/Z axes in the frame of the bodies being moved (if it is absent, all three axes are used and
//! the offset is applied in full). Older designs need no transformation: they align on all three
//! axes as before. The last release refuses a design that has this (it refuses the newer format
//! number) rather than moving the bodies on all three axes.
