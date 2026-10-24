//! Partial record readers for embedded transmit fragments.
//!
//! These readers preserve the bounded cadmpeg/sldkit compatibility contract:
//! they recognize individual records, retain exact read spans and leave all
//! other bytes uninterpreted. They do not frame a complete document, establish
//! delta type 3/4 layouts, or certify final saved state. For complete schema-aware
//! parsing use [`crate::parse_xb`] and [`crate::brep::map_xb_brep`].
//!
//! Inputs are already extracted body slices. No container, configuration, IR,
//! length-unit conversion or source-file hash belongs to this module.
//! See `PARTIAL_READERS.md` and per-file Apache-2.0 notices for provenance.
#![allow(clippy::pedantic)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used, clippy::panic))]

pub mod analytic;
pub mod blend;
pub mod intersection;
mod layout;
pub mod native_fin;
pub mod native_hierarchy;
pub mod offset;
pub mod spline;
pub mod subset;
pub mod sweep;
pub mod topology;
mod view;

/// Exact bytes interpreted by one bounded reader, relative to its input slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadSpan {
    pub offset: usize,
    pub byte_len: usize,
    pub tag: String,
    pub source_record_id: Option<u16>,
}
impl ReadSpan {
    pub(super) fn new(start: usize, end: usize, tag: &str, source_record_id: Option<u16>) -> Self {
        Self { offset: start, byte_len: end.saturating_sub(start), tag: tag.to_owned(), source_record_id }
    }
}

/// Bounded native body ownership, with source offsets and wire identities.
#[derive(Debug, Clone)]
pub struct BodyRecord {
    pub attr: u16,
    pub kind: crate::brep::BodyKind,
    pub refs: Vec<u16>,
    pub offset: usize,
    pub regions: Vec<RegionRecord>,
}
#[derive(Debug, Clone)]
pub struct RegionRecord {
    pub attr: u16,
    pub offset: usize,
    pub shells: Vec<ShellRecord>,
}
#[derive(Debug, Clone)]
pub struct ShellRecord {
    pub attr: u16,
    pub offset: usize,
    pub refs: Vec<u16>,
}
