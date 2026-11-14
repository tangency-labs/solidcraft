//! A design saves `geometry_copies`: for each copy of another design file's public geometry, its
//! `name`, the `file` and `publication` it takes, the file's digest at the last read, the
//! component its features are in and its `items` (a published name and the construction feature
//! that holds it here). The copied planes, axes and points are ordinary construction features
//! with fixed geometry. A design without a copy saves nothing, and older designs read as having
//! none. The last release opens a design that has copies with a warning: it reads the
//! construction features and keeps the `geometry_copies` it can't read.
