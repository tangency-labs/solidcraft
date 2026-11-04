//! A perforation field's holes can be hexagons or slots: a `sheet_perforate` feature saves optional `shape` ("round",
//! "hex", "slot"), `length` (a slot's) and `row_pitch`, a sheet's perforation `shape` (`"hex"` or `{"slot": {"length":
//! …}}`) and `row_pitch`. Older designs need no transformation: without them a field is round holes on the pitch's
//! rows, as before. The last release refuses a design saved with this (the newer format number).
