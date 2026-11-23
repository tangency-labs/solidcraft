//! A frame member can save `corner: "auto"` (each corner finished by its angle), `fit_ends` (free
//! ends fitted to the tubes of earlier Frame Members; saved only when true) and, in `corners`, a
//! `members` list of two or more ids where it held exactly two. An older design has none of these,
//! and a two-id override reads as before, so it opens and builds as it did. A build before this
//! change opens a design that uses them with a newer-format warning and keeps the fields it cannot
//! read.
