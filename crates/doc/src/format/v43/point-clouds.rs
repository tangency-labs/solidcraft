//! A design can hold point clouds (`point_clouds`: scanned points kept as a reference, #5151),
//! their points and colours a binary part of the design file (base64 in a plain JSON design), with
//! how they are coloured (`color_by`, `color`, `height_range`).
//! Older designs need no transformation: they have none. The last release refuses a design
//! saved with this (the newer format number).
