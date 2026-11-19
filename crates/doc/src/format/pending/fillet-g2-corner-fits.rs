//! A fillet with `corner: g2` can carry `fit: reduce` or `fit: taper`, where before only
//! `fit: exact` was accepted with it: `reduce` builds every edge and corner at the largest single
//! radius that fits, and `taper` builds where the radius fits. Older designs have no such fillet, so
//! nothing in them is rewritten. The last release opens a design that has one with a warning about
//! the newer format, reports that fillet as a G2 corner built for `fit: exact` only, and keeps it
//! as saved when the design is saved.
