//! Thread features save `point` to continue onto a screw's point and `point_depth` to keep
//! the full depth there. Older designs omit both fields, so they do not follow the point.
//! The last release warns about the newer format, opens what it can read and keeps the rest.
