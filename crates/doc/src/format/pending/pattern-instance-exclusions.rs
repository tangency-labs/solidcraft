//! Rectangular, circular and path patterns can omit whole placements and individual source
//! openings or bodies. Whole-placement `skip` stores pattern steps; `skip_members` stores
//! steps, the source feature ID, and a persistent sketch point, profile, body or singleton
//! identity. Command instance numbers and member indices are resolved before saving.
//! Copy body names remain pinned to their logical slots, including omitted copies.
//! Released designs have no exclusions and keep every copy. The last release warns about
//! the newer format and preserves fields it cannot read when saving.
