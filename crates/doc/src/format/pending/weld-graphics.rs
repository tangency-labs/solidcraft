//! Save weld graphics and their symbols together, using nine stable type ids, named choices,
//! optional ISO 4063 process numbers and density, type-specific millimetre sizes and optional
//! intermittent spans (count, length and centre pitch). Texture is derived from the process.
//! Older designs have no welds; this pre-merge shape needs no migration. The last release
//! warns about the newer format and preserves the weld data it cannot read when saving.
