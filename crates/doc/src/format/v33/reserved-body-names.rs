//! The design saves the body names a `body.rename` freed (`Body1` after the body it named was
//! renamed), so a name the design used is not used again.
//!
//! A design saved before this has no such names: it keeps the names it has, and the next body
//! it makes may take a name an earlier rename freed.
