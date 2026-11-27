//! A `SurfaceMidsurface` feature (Midsurface, #11439): `faces`, two points a pair (each on one
//! face of a pair of faces that face each other across material, in pairs), optional `body`,
//! `max_thickness`, optional `min_thickness`, `min_aspect` (a number, default 5) and
//! `suppress_holes_below`, and `carry_holes` and
//! `stitch` (saved only when false) and `remove_original` (saved only when true). It makes the
//! surface halfway between each pair, joined where walls meet. A new feature type: older designs
//! have none, so nothing is rewritten. The last release warns about the newer format number,
//! cannot build the feature (it reports the unknown feature type) and keeps it when saving.
