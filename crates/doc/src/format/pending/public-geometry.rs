//! A design saves `public_geometry`: named groups of a skeleton component's sketches and
//! construction planes, axes and points, each group a `name` and its `items` (a published name
//! and the feature it is). A design without one saves nothing, and older designs read as having
//! none. The last release opens a design that has public geometry with a warning and keeps the
//! `public_geometry` it can't read.
//!
//! A design also saves `geometry_copies`: for each copy of another design file's public
//! geometry, its `name`, the `file` and `publication` it takes, the file's digest at the last
//! read, the component its features are in and its `items` (a published name and the feature
//! that holds it here). The copied sketches, planes, axes and points are ordinary features with
//! fixed geometry. A design without a copy saves nothing, and older designs read as having none.
//! The last release opens a design that has copies with a warning: it reads the features and
//! keeps the `geometry_copies` it can't read.
