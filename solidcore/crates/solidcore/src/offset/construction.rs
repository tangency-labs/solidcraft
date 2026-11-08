//! Shell, offset and thicken as one construction (tangency-labs/solidcore#2350): exact
//! or certified face offsets (S1), mitres (S2), their arrangement (S3), the
//! winding-number cavity rule (S4) and extraction (S5). Opt-in,
//! `UpstreamFixes::shell_one_construction` (WR-718).
//!
//! Upstream: none (solidcore-native; the design is SMLib's, approved for SMLib and moved
//! to solidcore; the fork has no code of it).
//!
//! | Rust              | Upstream |
//! |-------------------|----------|
//! | [`bernstein`]     | none     |
//! | [`sheets`]        | none (S1; replaces `N_SrfOffset` and the fold check) |
//!
//! Built so far: S1. The stages after it (S2 to S5) are not built; until they are,
//! the flag runs S1 inside today's executive: B-spline faces whose offset folds are
//! refused naming the largest wall that works
//! ([`crate::error::offset::OffsetFailure::Folds`]), and every implicit offset surface is
//! replaced by its certified B-spline instead of `N_SrfOffset`'s sampled one.
//!
//! Deviations: not upstream.

pub mod bernstein;
pub mod sheets;
