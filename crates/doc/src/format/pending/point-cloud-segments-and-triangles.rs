//! A point cloud keeps the segments and triangles its file draws over its points (#8817):
//! `segments` and `triangles` in its `complex`, the indices of their points, and a column of named
//! numbers may lie on them (`on`: `segment` or `triangle`; a column that does not say is on the
//! points, as every design saved before this has it). Older designs need no transformation: their
//! clouds are points alone. The last release refuses a design saved with this (the newer format
//! number) instead of opening its clouds as loose points without their edges and faces.
