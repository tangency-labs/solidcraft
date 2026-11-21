//! A joint origin can name the occurrences it is `inside` (`"inside": [ids]`, under its
//! `occurrence`): set when it is picked on a part in a sub-assembly placed more than once, on
//! another copy than the first. The origin is then on that copy, and the part's own geometry is
//! found through those occurrences (#11426). Older designs need no transformation: their origins
//! have nothing inside and are written as before. The last release opens a design that has such
//! an origin with a warning and keeps the field it can't read; there the origin's part is looked
//! for in the copy's own bodies, so its joint reports an error instead of landing on the first
//! copy.
