//! An edge flange can be built from a profile sketch (#3516): `sheet_flange` features save the
//! sketch as `profile`. An older design has no such key and every flange stays a rectangle. The
//! last release opens a design that has it with a newer-format warning and builds such a flange
//! as a plain rectangle `height` high along its whole edge.
