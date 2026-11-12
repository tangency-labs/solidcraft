//! A design saves `public_geometry`: named groups of a skeleton component's sketches and
//! construction planes, axes and points, each group a `name` and its `items` (a published name
//! and the feature it is). A design without one saves nothing, and older designs read as having
//! none. The last release opens a design that has public geometry with a warning and keeps the
//! `public_geometry` it can't read.
