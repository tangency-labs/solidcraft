//! A look saves `grain` when it is a wood with procedural grain: the species and the settings
//! that differ from the species' own (the grain's axis, seed, scale, ring spacing and the rest,
//! `crate::wood_grain::Grain`). Older designs omit it and keep their woods in one plain colour,
//! as they were drawn. The last release opens a design that has it with a warning, keeps the
//! field it cannot read, and draws the wood in the look's colour.
