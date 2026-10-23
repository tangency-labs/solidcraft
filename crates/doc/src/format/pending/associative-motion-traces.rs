//! Associative motion traces retain their occurrence path, point reference, joint bounds or
//! study identity, sample count and assembly branch seeds, alongside the generated sketch.
//! Older designs default to no associations: their saved curves remain fixed. Readers from
//! the last release refuse the newer format number instead of silently dropping associations.
