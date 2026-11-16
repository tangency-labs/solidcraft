//! Tab and Slot: where the tabs go along each stretch of a plate joint, and the laser cut
//! outlines of the tabs and their slots.
//!
//! Upstream: no counterpart. Plain float maths on numbers the caller has read from exact faces
//! ([`crate::plate_contact`] finds the plate joints and their stretches); no contact code here.
//!
//! Words (design 6.1): a stretch `[a, b]` of length `L = b − a`; `W` the tab width; `e` the end
//! distance; `U = L − 2e` the usable length; `g` the space between neighbouring tabs; `q` the
//! spacing (tab start to tab start); `c` the slot clearance; `g_min` the least space between two
//! tabs, from the rules of design 6.3 (for laser cuts [`laser_g_min`]). Tab `k` of a stretch runs
//! `u ∈ [u_k, u_k + W]`, numbered in increasing `u`.
//!
//! | Rust                         | Upstream |
//! |------------------------------|----------|
//! | [`LENGTH_SLACK`], [`MAX_TABS`] | none   |
//! | [`Layout`], [`StretchSkip`], [`place`] | none |
//! | [`laser_g_min`]              | none     |
//! | [`TabSkip`], [`tab_outline`] | none     |
//! | [`slot_outline`]             | none     |

use crate::error::{Result, SolidError};
use crate::math::Point2;
use crate::prism::profile::{Loop2, Region2, Seg2};

/// The relative slack with which a spacing fits a whole number of times (design 5.1, 6.2): `n`
/// spacings fit in `U − W` when `n q ≤ U − W + LENGTH_SLACK · max(U − W, n q)`. Picked well over
/// the rounding of a few f64 sums and products of lengths and well under any length a person
/// types on purpose.
pub const LENGTH_SLACK: f64 = 1.0e-9;

/// The most tabs [`place`] lays out on one stretch; more is refused, not allocated. Far past
/// what the tool-face cap of design 6.4 lets one cut take.
pub const MAX_TABS: usize = 100_000;

const OP: &str = "tab_slot";

/// How the tabs of a stretch are laid out (design 6.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layout {
    /// `N = max(1, round((U + 3W) / (4W)))`, then at most as many as `g_min` lets fit.
    Auto,
    /// This many tabs per stretch; one is centred, more run from end distance to end distance.
    Count(u32),
    /// The most tabs at this spacing (tab start to tab start) that fit, centred in `U`.
    Spacing(f64),
}

/// Why a stretch (or, for [`StretchSkip::SpacingTooSmall`], its plate joint) gets no tabs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StretchSkip {
    /// `too_short`: the stretch is `length` long and needs `need` (one tab and an end distance at
    /// each end, or the asked count with spaces of `g_min`); `most` tabs would fit (0 when not
    /// even one does).
    TooShort {
        /// The stretch's length `L`.
        length: f64,
        /// The length the asked layout needs.
        need: f64,
        /// The most tabs that fit with spaces of at least `g_min`.
        most: usize,
    },
    /// `spacing_too_small`: the spacing leaves `space = q − W` between tabs, less than `least`.
    SpacingTooSmall {
        /// The space the spacing leaves between neighbouring tabs.
        space: f64,
        /// The least space allowed, `g_min`.
        least: f64,
    },
}

/// The least space between two laser-cut tabs (design 6.3, laser row): the web of the slot plate
/// between two slots, `2c + min_land`.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a clearance or land that is not finite and non-negative.
pub fn laser_g_min(clearance: f64, min_land: f64) -> Result<f64> {
    if !(non_negative(clearance) && non_negative(min_land)) {
        return Err(invalid(
            "clearance and land must be finite and non-negative",
        ));
    }
    Ok(2.0 * clearance + min_land)
}

/// The tab starts `u_k` of the stretch `[a, b]` (design 6.2, 6.3), in increasing order, or why
/// it gets none. Every `u_k` is computed from `k` in one expression, never by adding pitches.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a stretch whose ends are not finite and ordered, a tab width
/// or spacing that is not finite and positive, an end distance or `g_min` that is not finite and
/// non-negative, or a count of 0.
pub fn place(
    stretch: (f64, f64),
    layout: Layout,
    width: f64,
    end_distance: f64,
    g_min: f64,
) -> Result<std::result::Result<Vec<f64>, StretchSkip>> {
    let (a, b) = stretch;
    if !(a.is_finite() && b.is_finite() && a <= b) {
        return Err(invalid("a stretch's ends must be finite and ordered"));
    }
    if !(width.is_finite() && width > 0.0) {
        return Err(invalid("the tab width must be finite and positive"));
    }
    if !(non_negative(end_distance) && non_negative(g_min)) {
        return Err(invalid(
            "the end distance and least space must be finite and non-negative",
        ));
    }
    let length = b - a;
    if !length.is_finite() {
        return Err(invalid("a stretch's length must be finite"));
    }
    let usable = length - 2.0 * end_distance;
    // The most tabs with spaces of at least g_min: (N − 1) g_min + N W ≤ U.
    let most = if usable < width {
        0
    } else {
        whole((usable + g_min) / (width + g_min)).max(1)
    };
    let too_short = |need: f64| Ok(Err(StretchSkip::TooShort { length, need, most }));
    match layout {
        Layout::Count(0) => Err(invalid("the count must be at least 1")),
        Layout::Count(count) => {
            let n = count as usize;
            let need = (n as f64 - 1.0) * g_min + n as f64 * width + 2.0 * end_distance;
            if usable < width {
                return too_short(width + 2.0 * end_distance);
            }
            if n == 1 {
                return Ok(Ok(vec![a + (length - width) / 2.0]));
            }
            let space = (usable - n as f64 * width) / (n as f64 - 1.0);
            if space < g_min {
                return too_short(need);
            }
            let start = a + end_distance;
            starts(n, |k| start + k as f64 * (width + space))
        }
        Layout::Auto => {
            if usable < width {
                return too_short(width + 2.0 * end_distance);
            }
            let wanted = whole(((usable + 3.0 * width) / (4.0 * width)).round()).max(1);
            let n = wanted.min(most);
            if n == 1 {
                return Ok(Ok(vec![a + (length - width) / 2.0]));
            }
            let space = (usable - n as f64 * width) / (n as f64 - 1.0);
            let start = a + end_distance;
            starts(n, |k| start + k as f64 * (width + space))
        }
        Layout::Spacing(q) => {
            if !(q.is_finite() && q > 0.0) {
                return Err(invalid("the spacing must be finite and positive"));
            }
            if q - width < g_min {
                return Ok(Err(StretchSkip::SpacingTooSmall {
                    space: q - width,
                    least: g_min,
                }));
            }
            if usable < width {
                return too_short(width + 2.0 * end_distance);
            }
            let room = usable - width;
            if !(room / q <= MAX_TABS as f64) {
                return Err(invalid("more tabs on one stretch than MAX_TABS"));
            }
            let fits = |m: usize| {
                let span = m as f64 * q;
                span <= room + LENGTH_SLACK * room.max(span)
            };
            let mut gaps = whole(room / q);
            while gaps > 0 && !fits(gaps) {
                gaps -= 1;
            }
            while fits(gaps + 1) {
                gaps += 1;
            }
            let group = gaps as f64 * q + width;
            let start = a + end_distance + (usable - group) / 2.0;
            starts(gaps.saturating_add(1), |k| start + k as f64 * q)
        }
    }
}

/// `n` starts from `at(k)`, or a refusal past [`MAX_TABS`].
fn starts(
    n: usize,
    at: impl Fn(usize) -> f64,
) -> Result<std::result::Result<Vec<f64>, StretchSkip>> {
    if n > MAX_TABS {
        return Err(invalid("more tabs on one stretch than MAX_TABS"));
    }
    Ok(Ok((0..n).map(at).collect()))
}

/// Why one tab is not made.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TabSkip {
    /// `tab_too_short`: the tab's length `t_S + protrusion` is not positive (design 7.1).
    TooShort {
        /// The tab's length.
        length: f64,
    },
}

/// Tab `k`'s outline in the tab plate's frame `(u, z)` (design 7.1): `u ∈ [u_k, u_k + W]`,
/// `z ∈ [−h, δ]`, with `h = t_S + protrusion` the tab's length and `δ` the level of T's end face
/// above P. Counter-clockwise.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a non-finite start, length or level, or a width that is not
/// finite and positive.
pub fn tab_outline(
    start: f64,
    width: f64,
    length: f64,
    level: f64,
) -> Result<std::result::Result<Region2, TabSkip>> {
    if !(start.is_finite() && length.is_finite() && level.is_finite()) {
        return Err(invalid("a tab's start, length and level must be finite"));
    }
    if !(width.is_finite() && width > 0.0) {
        return Err(invalid("the tab width must be finite and positive"));
    }
    if length <= 0.0 {
        return Ok(Err(TabSkip::TooShort { length }));
    }
    Ok(Ok(rectangle((start, -length), (start + width, level))))
}

/// Slot `k`'s laser outline in P's frame `(u, w)` (design 7.2): `u ∈ [u_k − c, u_k + W + c]`,
/// `w ∈ [w_A − c, w_B + c]`, with `w_A < w_B` the levels of the tab plate's faces A and B.
/// Counter-clockwise.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a negative or non-finite clearance (refused before
/// evaluation, design 7.2), a non-finite start, a width that is not finite and positive, or
/// faces that are not finite and ordered.
pub fn slot_outline(start: f64, width: f64, faces: (f64, f64), clearance: f64) -> Result<Region2> {
    if !non_negative(clearance) {
        return Err(invalid("the clearance must be finite and non-negative"));
    }
    if !(start.is_finite() && width.is_finite() && width > 0.0) {
        return Err(invalid(
            "a slot's start must be finite and its width positive",
        ));
    }
    let (wa, wb) = faces;
    if !(wa.is_finite() && wb.is_finite() && wa < wb) {
        return Err(invalid("the tab plate's faces must be finite and ordered"));
    }
    Ok(rectangle(
        (start - clearance, wa - clearance),
        (start + width + clearance, wb + clearance),
    ))
}

/// The counter-clockwise rectangle with corners `lo` and `hi` (`lo < hi` both ways).
fn rectangle(lo: (f64, f64), hi: (f64, f64)) -> Region2 {
    let p = [
        Point2::new(lo.0, lo.1),
        Point2::new(hi.0, lo.1),
        Point2::new(hi.0, hi.1),
        Point2::new(lo.0, hi.1),
    ];
    Region2 {
        outer: Loop2 {
            segs: (0..4)
                .map(|i| Seg2::Line {
                    a: p[i],
                    b: p[(i + 1) % 4],
                })
                .collect(),
        },
        holes: Vec::new(),
    }
}

/// The whole part of a non-negative finite `x`, saturating (a count no plate reaches).
fn whole(x: f64) -> usize {
    if x.is_finite() && x > 0.0 {
        // Saturating cast: x is finite and positive.
        x.floor().min(usize::MAX as f64) as usize
    } else {
        0
    }
}

fn non_negative(x: f64) -> bool {
    x.is_finite() && x >= 0.0
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: OP,
        reason,
    }
}

#[cfg(test)]
mod tests;
