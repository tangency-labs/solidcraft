//! [`super::TRIM_COVER`]: the rows only the kept-trim cover moved
//! (tangency-labs/solidcraft#19295), each with main's row hash from
//! [`super::record_main_hashes`] on main's goldens.

/// The moved rows: unit, case, main's row hash.
pub(super) static TRIM_COVER: &[(&str, &str, u64)] = &[];
