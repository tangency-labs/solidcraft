//! A joint whose limits were typed as expressions over the design's parameters keeps them in its
//! `exprs`: `"limits": [[minimum, maximum], ...]` by value index, each end the expression text or
//! `null` for a number (#20242). The numbers stay in the joint's `limits` as before, worked out
//! again whenever the design changes. An older design has no limit expressions and needs none.
//! The last release warns about the newer format, opens the design and places the joint with the
//! numbers its limits last had, without following the parameters; a save there drops the limit
//! expressions.
