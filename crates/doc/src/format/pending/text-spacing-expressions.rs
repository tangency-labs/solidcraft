//! A sketch text's letter spacing and line spacing can be expressions of parameters, as its
//! height already can (#21814): the link source's `exprs` saves `letter_spacing` (a length) and
//! `line_spacing` (a factor) next to `x`, `y`, `height` and `angle`, and `style` keeps the numbers
//! they came to last. Older designs never have these fields, so no step is needed: their spacing
//! is the number in `style`, as before. The last release warns about the newer format number and
//! opens the design; it does not know the two fields, so the text keeps the spacing numbers it was
//! saved with and no longer follows the parameters.
