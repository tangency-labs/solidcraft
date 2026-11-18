//! A profile pick that names every curve around a profile and its holes (`whole_boundaries`) may
//! now name one curve in two holes. A curve that splits an island in two bounds both halves, and
//! both halves are holes of the profile around the island (#17613). What is saved is unchanged,
//! and older designs are read as they were. The last release warns about the newer format number,
//! opens the design and keeps a feature whose pick names a curve in two holes as a feature it
//! can't read.
