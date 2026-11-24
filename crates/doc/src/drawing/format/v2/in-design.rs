//! A drawing lives in the file of the design it draws (#3890): it has an `id`, unique among its
//! design's drawings, and at most one source, the design it is in, which has an `id` and no `path`.
//! No step: no release saved a drawing before this, and a drawing file of its own is no longer
//! read, so an older drawing is refused as damaged. The last release refuses a design that holds a
//! drawing (the design's newer format number).
