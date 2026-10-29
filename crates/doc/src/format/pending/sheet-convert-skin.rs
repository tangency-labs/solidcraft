//! Convert to Sheet Metal can go through the skin (#6748): a new feature type,
//! `sheet_skin_convert`, saves the body, its bend edges, the faces it skips as `skip` and the
//! material's side as `direction` (inside unless saved), with no fixed face. The model cache keeps
//! the converted body's flat pattern as `skins`. An older design has neither and converts as
//! before. The last release refuses the newer design format instead of dropping them.
