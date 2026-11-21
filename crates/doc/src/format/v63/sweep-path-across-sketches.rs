//! A sweep's path may join the curves of several sketches (#16168): a curve picked in a sketch
//! other than the path's own is saved as `{"sketch": <sketch>, "curve": <id>}`, while a curve
//! of the path's own sketch keeps saving as its id alone. An older design has only plain ids,
//! so it needs no rewrite; a build before this change opens such a design with a
//! newer-format warning and refuses the sweep's path curve it cannot place.
