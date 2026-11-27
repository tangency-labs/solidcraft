//! A face pattern can stand its copies on more faces of the same body (`more_faces`, with `face`:
//! one region, the edges they share no edges of it) and lay them out by distance on the surface
//! (`layout`: `{"type": "spacing", "spacing", "row_spacing"?}`, `{"type": "spiral", "count"}`,
//! `{"type": "fill", "count"}` or `{"type": "geodesic", "frequency"}`), in place of the grid and
//! `random`; and `fit_edges` stands the layout's outer copies flush with the face's edges, the edge
//! margin moving them in. Designs without them are unchanged (their grid copies at the centres
//! of their cells), so nothing in them is rewritten. A release that
//! does not know them warns about the newer format number and reads such a pattern as its grid.
