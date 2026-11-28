//! A drawing's balloon, and a note anchored on an edge, can save `"along"` (#22195): where on its
//! edge the arrow or leader ends, as a part of the edge's length from its start (0 to 1).
//! `drawing.balloon.add` and `drawing.note.add` with `at_edge: picked` set it; without it the
//! arrow ends on the edge's middle, which is what every older drawing has, so nothing in them is
//! rewritten. The last release does not know the key: it opens a drawing that has it with a
//! warning about the newer format, and keeps each balloon or note that has it as saved, through a
//! save, without drawing it.
