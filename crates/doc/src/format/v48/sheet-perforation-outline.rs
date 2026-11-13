//! A perforation field can be bounded by a sketch: a `sheet_perforate` feature saves an optional `sketch` and
//! `profiles` (its outline), and a sheet's perforation the outline in flat coordinates (`outline`). Holes go only
//! inside the outline. Older designs need no transformation: a field without an outline covers the whole sheet as
//! before. The last release refuses a design saved with this (the newer format number).
