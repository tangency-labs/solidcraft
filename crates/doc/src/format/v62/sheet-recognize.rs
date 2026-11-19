//! Recognize Sheet Metal (#6748): a new feature type, `sheet_recognize`, saves the body that is
//! sheet metal already, the face whose side is the sheet's surface as `face` (none: the body's
//! largest face), the faces it leaves out as `skip`, its bend edges as `bends` and its rule. An
//! older design has none. The last release warns about the newer format number and keeps the
//! feature as one of a newer file, unread.
