//! A design can keep named Section Analyses (`"saved_sections": [{"name", "planes": [{"id",
//! "origin", "normal", "hidden"?}], "hatch"}]`), so one can be shown in place of another (#20649).
//! The field is saved only when the design has one. An older design has none and opens as before,
//! and a design that keeps none saves exactly as it did. The last release opens a design that has
//! them with a warning about the newer format and keeps what it can't read.
