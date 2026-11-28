//! A cell grid can go on a whole-turn cylinder wall instead of a sketch's profiles (#23010): it
//! saves the wall's point as `face` and its body as `body`, and leaves `sketch` out. Older designs
//! keep the `sketch` they saved and get no `face` or `body`. The last release warns about the newer
//! format number, opens the design and keeps a cell grid with no `sketch` as a feature it can't
//! read.
