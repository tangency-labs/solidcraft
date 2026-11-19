//! How hard each operation works (docs/design/rigor-dial.md): a design may save `rigor`, with
//! its default rigor (`default`) and the operations that override it (`ops`, by feature id).
//! Each record keeps what was asked (`set`: the dial from 0 to 1 and any knob set on its own)
//! and what it resolved to then (`built`: certification, tolerance, chord, samples, effort and
//! time budget), which the operation is built with. An operation with no record inherits the
//! default. Older designs have no `rigor` and need no rewriting: every operation builds with
//! the legacy rigor, the calls as they were before, so they reopen with the same faces. The
//! last release warns about the newer format number, opens what it can read and keeps the
//! rest; it builds every operation as legacy.
