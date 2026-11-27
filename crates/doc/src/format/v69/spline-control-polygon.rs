//! Control splines can save construction guides that share their control points. A separate
//! ownership field lets 0.48 read the sides as ordinary construction lines and preserve the
//! ownership metadata. Sketches can hide the guides without removing dimensions or constraints.
//! Older designs keep their control splines without stored guides.
