//! Drawings save their tables (`tables`, #3874): each on one of the drawing's sheets, a parts list or
//! a revision table, with its definition and its place. Rows are never saved. An older drawing has no
//! tables. The last release refuses a drawing saved with them, as a newer format.
