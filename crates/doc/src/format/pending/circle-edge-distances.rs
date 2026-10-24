//! A sketch can hold two more dimensions to the edge of a circle or arc (#4064):
//! `point_circle_distance`, from a point `p` to the edge of circle or arc `c` (saved with its
//! `value` and, for a point within the circle, `inside`), and `circle_circle_distance`, between
//! the edges of circles or arcs `a` and `b` (saved with its `value` and, unless they are apart,
//! `circles`: `crossing` for how far they overlap, `nested` for the narrowest gap between one
//! inside the other). Older designs need no transformation: they have no such dimensions; the
//! last release asks for an update instead of meeting a constraint type it doesn't know.
