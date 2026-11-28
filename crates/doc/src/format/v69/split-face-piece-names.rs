//! Split Face names the pieces it cuts a face into: each is the face it came from qualified by
//! the feature and the piece's side of its tool (`F1:+z#[F7:split:in]`, inside F7's loop;
//! `F7:split:+` and `F7:split:-` either side of a plane or a tool face; `F7:split:left` and
//! `F7:split:right` of an open sketch curve; `F7:split:in:2` inside the second of several curves).
//! Designs save these names in references and appearances made on a piece. Older designs named the
//! pieces by the faces around them (`F1:+z#[F1:+x,F1:+y]`, or the face's bare name for the piece
//! nothing around it told apart); each piece keeps that name as its former one, so a reference
//! saved then still finds the same piece, and every result is unchanged. The last release opens a
//! design that holds the new names with a newer-format warning (`crate::newer_file`) and keeps
//! what it can't find for the next save.
