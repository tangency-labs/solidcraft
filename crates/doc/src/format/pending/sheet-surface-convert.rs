//! Convert to Sheet Metal converts a body from its faces (#6748): a new feature type,
//! `sheet_surface_convert`, saves the body, its bend edges, the faces it skips as `skip` and the
//! material's side as `direction` (inside unless saved), with no fixed face. A conversion of
//! picked faces (`sheet_convert`) saves the faces picked after the first as `faces` and the
//! material's side as `direction`. A surface body converts too, its faces the sheet's surface. The
//! model cache keeps each sheet made from surfaces as `surface_sheets`. An older design has none
//! of these; its conversions are of the face it picked, which now builds the sheet from the
//! body's faces. The last release refuses the newer design format instead of dropping them.
