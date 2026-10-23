//! A design can hold frame trims (#4003): a `frame_trim` feature with the `members` it trims (each
//! a frame member feature by name and a `line` of it), what it trims them `to` (a `member` and an
//! optional `side` of it, or a `plane`), which `end` moves (default: the nearer) and the weld `gap`.
//! A saved model keeps each member's `frame_members` record (its body and where its ends are); a
//! saved model without them is read as having none. Older designs need no transformation: they
//! have no such feature. The last release refuses a design saved with this (the newer format
//! number).
