//! [`super::TRIM_COVER`]: the rows only the kept-trim cover moved
//! (tangency-labs/solidcraft#19295), each with the hash of main's row from
//! [`super::record`] run on main.

/// The moved rows: unit, case, main's row hash.
pub(super) static TRIM_COVER: &[(&str, &str, u64)] = &[];
