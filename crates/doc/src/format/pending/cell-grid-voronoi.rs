//! A `cell_grid` feature can save `"shape": "voronoi"` (#22566): irregular convex cells, the
//! Voronoi cells of the hexagonal grid's centres, each moved by a pseudo-random offset. With it
//! come three optional fields, each absent when not given: `seed` (a whole-number expression, which
//! pattern), `irregularity` (an expression from 0 to 1, how far the centres move) and
//! `corner_radius` (a length expression, the rounding of each cell corner). Older designs have none
//! of these, so nothing in them is rewritten. The last release does not know the shape: it opens a
//! design that has it with a warning about the newer format, keeps the feature's place on the
//! timeline with an error saying a newer SolidCraft is needed to build it, builds the rest and keeps
//! the feature as it came when saving; it ignores the three fields on any other shape.
