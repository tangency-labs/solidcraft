//! Drawings save their balloons (`balloons`, #3874 W26): each on one of the drawing's views, pointing
//! at a placed part by a named edge of it (the edge's body, the feature that made it, its name and
//! the occurrences the body is placed through), with its place on paper from the view's centre. An
//! older drawing has no balloons. The last release refuses a drawing saved with them, as a newer
//! format.
