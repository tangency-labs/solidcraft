//! The blend corner's Setback: where every edge at a vertex is blended, three or more, each
//! blend stops at its setback from the vertex and the hole they leave is filled by the
//! setback split ([`crate::blend::blend_setback`]), to G1, or to G2 for curvature-continuous
//! sections (or rounds eased into them).
//!
//! Upstream: none (pinned SMLib builds such a vertex as an `NxN` corner). The fork's
//! `source/SMLib/src/SmBlendCornerSetback.cpp`, `source/SMLib/inc/SmBlendCorner.h`
//! (tangency-labs/smlib 61547af, 1506e2f, ac8b4f8, f1d81f4; smlib#104).
//!
//! | Rust                         | Fork                                                  |
//! |------------------------------|-------------------------------------------------------|
//! | [`SetbackCorner`]            | `SmBlendCorner`'s `m_vSetBacks`, `m_lFillOrder`, `m_bConvexSetBack`, `m_bSetBackOverflows`, `m_pFill` |
//!
//! Deviations: the Setback's own members are kept in [`SetbackCorner`] on the corner
//! ([`FilletCorner::setback`]) from its creation (`SetFillOrder`) until its patches are
//! faces. Opt-in: `Kernel::fixes.fillet_blend_corner` (WR-697).
//!
//! [`FilletCorner::setback`]: crate::blend::fillet_corner::p3::FilletCorner::setback

use crate::blend::blend_setback::Fill;

/// The Setback's own members on a blend corner that is one.
#[derive(Debug, Clone)]
pub struct SetbackCorner {
    /// `m_vSetBacks`: each blended edge's setback, in the corner's `m_pAllEUs` order.
    pub setbacks: Vec<f64>,
    /// `m_lFillOrder`: the fill's order (1: G1, 2: G2).
    pub fill_order: usize,
    /// `m_bConvexSetBack`: whether the corner's edges are convex.
    pub convex: bool,
    /// `m_bSetBackOverflows`: the rule's setbacks do not fit, the blends overflow (refused
    /// if the corner is built).
    pub overflows: bool,
    /// `m_pFill`: the fill, from the edge geometry until the patches take it.
    pub fill: Option<Fill>,
}

impl Default for SetbackCorner {
    /// The constructor's: `m_lFillOrder(1)`, `m_bConvexSetBack(TRUE)`,
    /// `m_bSetBackOverflows(FALSE)`, no setbacks and no fill.
    fn default() -> Self {
        Self {
            setbacks: Vec::new(),
            fill_order: 1,
            convex: true,
            overflows: false,
            fill: None,
        }
    }
}

impl SetbackCorner {
    /// The Setback of fill order `order` (`SetFillOrder`).
    #[must_use]
    pub fn with_order(order: usize) -> Self {
        Self {
            fill_order: order,
            ..Self::default()
        }
    }
}
