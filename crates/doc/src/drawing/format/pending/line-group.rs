//! Drawings save the ISO 128 line group their lines are drawn in (`line_group`, #4890 D2d), and a
//! sheet or a view may save its own. An older drawing has none and draws in the 0.5 mm group. The
//! last release refuses a drawing saved with one, as a newer format.
