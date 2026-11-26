//! A Fillet's `fit` can be `keep_edge`: a concave edge whose flat wall is lower than the radius is
//! rounded by a round tangent to the floor only that ends on the wall's top edge (the top face and
//! its edge stay as they are); every other edge is rounded as usual. A design without it saves
//! `fit: exact`, `reduce` or `taper` or none, as before, so no step is needed. The last release
//! warns about the newer format number, opens what it can read and keeps the rest
//! (`crates/doc/src/newer_file.rs`): a fillet whose `fit` it does not know it reads as the default
//! and saves back as it came.
