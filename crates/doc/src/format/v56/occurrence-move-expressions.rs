//! An occurrence keeps the moves whose translation was typed as expressions over the design's
//! parameters (`expressed_moves`: per axis the expression, what it last worked out to and the
//! axes it was typed in, and whether those were world axes), and moves on with them whenever a parameter they name changes. Older
//! designs need no transformation: an occurrence without `expressed_moves` keeps its transform as
//! before. The last release warns about the newer format number and opens the design, placing the
//! occurrence where it was saved and dropping the expressions.
