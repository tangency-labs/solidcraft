//! Where the tabs of a plate joint go along one length: the layout shared by the joint
//! tool's routes. The plates route measures an overlap on a plate and the sheet metal route
//! measures a length along an edge on a panel's surface (curved or not); both hand that one
//! length here and get back tab spans along it. Nothing here knows about geometry.
//!
//! A finger joint is the case where the gaps are as wide as the tabs: `Spacing::Equal` with
//! `Ends::Tab`, `count` = fingers + 1 and `width` = length / (2 × fingers + 1). One plate keeps
//! the spans and the other keeps what lies between them.
//!
//! Randomization changes each tab's width and never its position, so every tab keeps the
//! center the spacing gave it. That keys a joint so it fits one way only, while the spacing
//! the user asked for still holds.

use crate::{DocError, Result};

/// Lengths closer than this are equal: tabs this close count as touching, not overlapping.
const TOLERANCE: f64 = 1e-9;

/// A tab never shrinks below this fraction of the nominal width.
const MIN_WIDTH_FRACTION: f64 = 0.1;

/// How tabs are spread along the length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spacing {
    /// `count` tabs spread evenly.
    Equal { count: usize },
    /// Tabs `pitch` apart, center to center, from the start, as many as fit up to `count`.
    Distance { pitch: f64, count: usize },
    /// As many tabs as make the pitch closest to `pitch` (the next count up when `round_up`),
    /// then spread evenly.
    BestFit { pitch: f64, round_up: bool, parity: Parity },
}

/// Whether a best fit count may be any count, or only even or odd ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Parity {
    #[default]
    Any,
    Even,
    Odd,
}

/// What sits at the two ends of the length: a gap or a tab. On a closed length (a loop),
/// which has no ends, it is what sits where the loop closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ends {
    #[default]
    Gap,
    Tab,
}

/// Seeded width randomization.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Randomize {
    pub seed: u64,
    /// Each tab's width changes by up to this much either way.
    pub variation: f64,
}

/// Everything about a layout except the length it is laid along.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The nominal tab width.
    pub width: f64,
    pub spacing: Spacing,
    pub ends: Ends,
    /// Keep-out lengths at the start and the end. Either one above zero makes a closed length
    /// laid out as an open one from its start.
    pub offsets: (f64, f64),
    pub randomize: Option<Randomize>,
}

/// One tab, from `start` to `end` along the length. On a closed length the tab on the point
/// where the loop closes runs past 0 or past the length; the caller carries it across.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub start: f64,
    pub end: f64,
}

impl Span {
    pub fn center(&self) -> f64 {
        (self.start + self.end) / 2.0
    }

    pub fn width(&self) -> f64 {
        self.end - self.start
    }
}

/// The tab spans along `length`, in order from its start. `closed` is true when the length
/// is a loop, such as a disc's rim.
pub fn lay_out(length: f64, closed: bool, layout: &Layout) -> Result<Vec<Span>> {
    let invalid = |why: &str| Err(DocError::Invalid(why.into()));
    let Layout { width, spacing, ends, offsets: (start_offset, end_offset), randomize } = *layout;
    if !(length.is_finite() && length > TOLERANCE) {
        return invalid("the length to place tabs along is zero");
    }
    if !(width.is_finite() && width > TOLERANCE) {
        return invalid("the tab width must be above zero");
    }
    if !(start_offset.is_finite() && end_offset.is_finite() && start_offset >= 0.0 && end_offset >= 0.0) {
        return invalid("tab offsets must be zero or more");
    }
    let usable = length - start_offset - end_offset;
    if usable <= TOLERANCE {
        return invalid("the offsets leave no length for tabs");
    }
    let has_offsets = start_offset > 0.0 || end_offset > 0.0;
    // A loop spaced without offsets fits one tab per pitch all the way round; anything else
    // fits one more, for its far end.
    let around_loop = closed && !has_offsets && !matches!(spacing, Spacing::Distance { .. });
    let count = match spacing {
        Spacing::Equal { count } => count,
        Spacing::Distance { pitch, count } => {
            if !(pitch.is_finite() && pitch > TOLERANCE) {
                return invalid("the tab pitch must be above zero");
            }
            count
        }
        Spacing::BestFit { pitch, round_up, parity } => {
            if !(pitch.is_finite() && pitch > TOLERANCE) {
                return invalid("the target tab pitch must be above zero");
            }
            best_fit_count(usable / pitch + if around_loop { 0.0 } else { 1.0 }, round_up, parity)
        }
    };
    if count == 0 {
        return invalid("no tabs: the tab count is zero");
    }

    let mut spans = if around_loop {
        loop_spans(length, width, count, ends)
    } else {
        match spacing {
            Spacing::Distance { pitch, .. } => distance_spans(width, pitch, count, start_offset, usable),
            _ => equal_spans(width, count, ends, start_offset, usable),
        }
    };
    if spans.is_empty() {
        return invalid("no tab fits: the tabs are wider than the length");
    }
    if let Some(gap) = smallest_gap(&spans, length, around_loop) {
        if gap < -TOLERANCE {
            return invalid("the tabs overlap: use fewer or narrower tabs, or a longer pitch");
        }
        if let Some(Randomize { variation, .. }) = randomize {
            if !(variation.is_finite() && variation >= 0.0) {
                return invalid("the width variation must be zero or more");
            }
            // Two neighbors can each grow by half the variation into the gap between them, so
            // a variation below the smallest gap keeps every pair apart whatever the seed.
            if variation > 0.0 && variation >= gap - TOLERANCE {
                return Err(DocError::Invalid(format!("the width variation ({variation}) must be less than the smallest gap between tabs ({gap})")));
            }
        }
    }
    if let Some(Randomize { seed, variation }) = randomize.filter(|r| r.variation > 0.0) {
        let lo = start_offset;
        let hi = length - end_offset;
        for (index, span) in spans.iter_mut().enumerate() {
            let center = span.center();
            let varied = (width + variation * unit_draw(seed, index as u64)).max(width * MIN_WIDTH_FRACTION);
            span.start = center - varied / 2.0;
            span.end = center + varied / 2.0;
            if !around_loop {
                // A tab at an end that grows past it stops there: the length ends.
                span.start = span.start.max(lo);
                span.end = span.end.min(hi);
            }
        }
    }
    Ok(spans)
}

/// `count` tabs of `width` spread evenly over `usable`, which starts at `start`.
fn equal_spans(width: f64, count: usize, ends: Ends, start: f64, usable: f64) -> Vec<Span> {
    let n = count as f64;
    if n * width > usable + TOLERANCE {
        return Vec::new();
    }
    let (gap, first) = match ends {
        // |gap|tab|gap|tab|gap|
        Ends::Gap => {
            let gap = (usable - n * width) / (n + 1.0);
            (gap, start + gap)
        }
        // A single tab with tabs at the ends sits in the middle.
        Ends::Tab if count == 1 => (0.0, start + (usable - width) / 2.0),
        // |tab|gap|tab|gap|tab|
        Ends::Tab => ((usable - n * width) / (n - 1.0), start),
    };
    (0..count)
        .map(|i| {
            let s = first + (gap + width) * i as f64;
            Span { start: s, end: s + width }
        })
        .collect()
}

/// Tabs of `width` whose centers are `pitch` apart, the first flush with `start`, up to
/// `count` of them and no further than the end of `usable`.
fn distance_spans(width: f64, pitch: f64, count: usize, start: f64, usable: f64) -> Vec<Span> {
    let end = start + usable;
    (0..count)
        .map(|i| Span { start: start + pitch * i as f64, end: start + pitch * i as f64 + width })
        .take_while(|span| span.end <= end + TOLERANCE)
        .collect()
}

/// `count` tabs one pitch apart all the way round a loop of `length`. With tabs at the ends a
/// tab is centered where the loop closes, so the first span starts below 0.
fn loop_spans(length: f64, width: f64, count: usize, ends: Ends) -> Vec<Span> {
    let pitch = length / count as f64;
    let first = match ends {
        Ends::Gap => pitch / 2.0,
        Ends::Tab => 0.0,
    };
    (0..count)
        .map(|i| {
            let c = first + pitch * i as f64;
            Span { start: c - width / 2.0, end: c + width / 2.0 }
        })
        .collect()
}

/// The smallest gap between neighboring tabs, counting the one across the closing point of a
/// loop. Negative when two overlap; `None` for a single tab on an open length.
fn smallest_gap(spans: &[Span], length: f64, around_loop: bool) -> Option<f64> {
    let mut gaps: Vec<f64> = spans.windows(2).map(|w| w[1].start - w[0].end).collect();
    if let (true, Some(first), Some(last)) = (around_loop, spans.first(), spans.last()) {
        gaps.push(first.start + length - last.end);
    }
    gaps.into_iter().reduce(f64::min)
}

/// The best fit count for a raw count, at least 1, honoring the parity. A count with the wrong
/// parity moves up by one, toward more tabs.
fn best_fit_count(raw: f64, round_up: bool, parity: Parity) -> usize {
    let n = if round_up { raw.ceil() } else { raw.round() };
    let n = if n.is_finite() && n >= 1.0 { n as usize } else { 1 };
    match parity {
        Parity::Even if n % 2 == 1 => n + 1,
        Parity::Odd if n % 2 == 0 => n + 1,
        _ => n,
    }
}

/// A value in [-1, 1] fixed by the seed and the tab's index (SplitMix64 on seed and index).
/// Each tab's draw depends only on its own index, so adding a tab at the far end leaves the
/// widths of the others as they were.
fn unit_draw(seed: u64, index: u64) -> f64 {
    let mut z = seed.wrapping_add(index.wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(width: f64, spacing: Spacing, ends: Ends) -> Layout {
        Layout { width, spacing, ends, offsets: (0.0, 0.0), randomize: None }
    }

    fn assert_spans(got: &[Span], want: &[(f64, f64)]) {
        assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
        for (g, w) in got.iter().zip(want) {
            assert!((g.start - w.0).abs() < 1e-9 && (g.end - w.1).abs() < 1e-9, "{got:?} vs {want:?}");
        }
    }

    #[test]
    fn equal_with_gaps_at_the_ends() {
        // 100 long, 3 tabs of 10: 70 of gap in 4 gaps of 17.5.
        let spans = lay_out(100.0, false, &layout(10.0, Spacing::Equal { count: 3 }, Ends::Gap)).unwrap();
        assert_spans(&spans, &[(17.5, 27.5), (45.0, 55.0), (72.5, 82.5)]);
    }

    #[test]
    fn equal_with_tabs_at_the_ends() {
        let spans = lay_out(100.0, false, &layout(10.0, Spacing::Equal { count: 3 }, Ends::Tab)).unwrap();
        assert_spans(&spans, &[(0.0, 10.0), (45.0, 55.0), (90.0, 100.0)]);
    }

    #[test]
    fn one_tab_with_tabs_at_the_ends_is_centered() {
        let spans = lay_out(100.0, false, &layout(10.0, Spacing::Equal { count: 1 }, Ends::Tab)).unwrap();
        assert_spans(&spans, &[(45.0, 55.0)]);
    }

    #[test]
    fn a_finger_joint_is_equal_spacing_with_gaps_as_wide_as_tabs() {
        // Three fingers on a 70 overlap: seven segments of 10, one plate keeps four of them.
        let spans = lay_out(70.0, false, &layout(10.0, Spacing::Equal { count: 4 }, Ends::Tab)).unwrap();
        assert_spans(&spans, &[(0.0, 10.0), (20.0, 30.0), (40.0, 50.0), (60.0, 70.0)]);
    }

    #[test]
    fn offsets_keep_tabs_off_the_ends() {
        let mut l = layout(10.0, Spacing::Equal { count: 2 }, Ends::Tab);
        l.offsets = (5.0, 15.0);
        let spans = lay_out(100.0, false, &l).unwrap();
        assert_spans(&spans, &[(5.0, 15.0), (75.0, 85.0)]);
    }

    #[test]
    fn distance_places_as_many_as_fit() {
        // Pitch 30 from the start: tabs at 0, 30, 60, 90; a fifth would end past 100.
        let spans = lay_out(100.0, false, &layout(10.0, Spacing::Distance { pitch: 30.0, count: 10 }, Ends::Gap)).unwrap();
        assert_spans(&spans, &[(0.0, 10.0), (30.0, 40.0), (60.0, 70.0), (90.0, 100.0)]);
        let spans = lay_out(100.0, false, &layout(10.0, Spacing::Distance { pitch: 30.0, count: 2 }, Ends::Gap)).unwrap();
        assert_eq!(spans.len(), 2);
    }

    #[test]
    fn best_fit_rounds_and_honors_parity() {
        // 100 / 30 + 1 = 4.33: rounds to 4, rounds up to 5, odd only makes 5.
        let fit =
            |round_up, parity| lay_out(100.0, false, &layout(10.0, Spacing::BestFit { pitch: 30.0, round_up, parity }, Ends::Tab)).unwrap().len();
        assert_eq!(fit(false, Parity::Any), 4);
        assert_eq!(fit(true, Parity::Any), 5);
        assert_eq!(fit(false, Parity::Odd), 5);
        assert_eq!(fit(false, Parity::Even), 4);
        assert_eq!(fit(true, Parity::Even), 6);
    }

    #[test]
    fn a_loop_without_offsets_goes_round_one_pitch_apart() {
        let gaps = lay_out(120.0, true, &layout(10.0, Spacing::Equal { count: 4 }, Ends::Gap)).unwrap();
        assert_spans(&gaps, &[(10.0, 20.0), (40.0, 50.0), (70.0, 80.0), (100.0, 110.0)]);
        let tabs = lay_out(120.0, true, &layout(10.0, Spacing::Equal { count: 4 }, Ends::Tab)).unwrap();
        assert_spans(&tabs, &[(-5.0, 5.0), (25.0, 35.0), (55.0, 65.0), (85.0, 95.0)]);
        // Best fit on a loop counts one tab per pitch: 120 / 30 = 4.
        let fit = lay_out(120.0, true, &layout(10.0, Spacing::BestFit { pitch: 30.0, round_up: false, parity: Parity::Any }, Ends::Gap));
        assert_eq!(fit.unwrap().len(), 4);
    }

    #[test]
    fn a_loop_with_an_offset_is_laid_out_from_its_start() {
        let mut l = layout(10.0, Spacing::Equal { count: 2 }, Ends::Tab);
        l.offsets = (10.0, 10.0);
        let spans = lay_out(120.0, true, &l).unwrap();
        assert_spans(&spans, &[(10.0, 20.0), (100.0, 110.0)]);
    }

    #[test]
    fn refuses_tabs_that_do_not_fit_or_overlap() {
        assert!(lay_out(100.0, false, &layout(10.0, Spacing::Equal { count: 11 }, Ends::Gap)).is_err());
        assert!(lay_out(100.0, false, &layout(10.0, Spacing::Equal { count: 0 }, Ends::Gap)).is_err());
        assert!(lay_out(100.0, false, &layout(10.0, Spacing::Distance { pitch: 5.0, count: 3 }, Ends::Gap)).is_err());
        assert!(lay_out(100.0, false, &layout(200.0, Spacing::Distance { pitch: 5.0, count: 3 }, Ends::Gap)).is_err());
        assert!(lay_out(40.0, true, &layout(15.0, Spacing::Equal { count: 3 }, Ends::Gap)).is_err());
        let mut l = layout(10.0, Spacing::Equal { count: 1 }, Ends::Gap);
        l.offsets = (60.0, 40.0);
        assert!(lay_out(100.0, false, &l).is_err());
    }

    fn randomized(seed: u64, variation: f64) -> Layout {
        let mut l = layout(10.0, Spacing::Equal { count: 5 }, Ends::Gap);
        l.randomize = Some(Randomize { seed, variation });
        l
    }

    #[test]
    fn randomization_keeps_every_center_and_varies_widths() {
        let plain = lay_out(200.0, false, &layout(10.0, Spacing::Equal { count: 5 }, Ends::Gap)).unwrap();
        let varied = lay_out(200.0, false, &randomized(1000, 4.0)).unwrap();
        assert_eq!(plain.len(), varied.len());
        for (p, v) in plain.iter().zip(&varied) {
            assert!((p.center() - v.center()).abs() < 1e-9);
            assert!((v.width() - 10.0).abs() <= 4.0 + 1e-9);
        }
        let widths: Vec<f64> = varied.iter().map(Span::width).collect();
        assert!(widths.windows(2).any(|w| (w[0] - w[1]).abs() > 1e-3), "{widths:?}");
    }

    #[test]
    fn the_same_seed_gives_the_same_widths_and_another_seed_others() {
        let a = lay_out(200.0, false, &randomized(1000, 4.0)).unwrap();
        let b = lay_out(200.0, false, &randomized(1000, 4.0)).unwrap();
        let c = lay_out(200.0, false, &randomized(1001, 4.0)).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn widths_never_go_below_a_tenth_of_nominal() {
        // Gaps of 25 allow a variation up to 25, far more than the width of 10.
        let spans = lay_out(200.0, false, &randomized(7, 24.0)).unwrap();
        assert!(spans.iter().all(|s| s.width() >= 1.0 - 1e-9), "{spans:?}");
    }

    #[test]
    fn no_seed_makes_tabs_overlap() {
        // 5 tabs of 10 on 100: gaps of 50 / 6.
        let gap = 50.0 / 6.0;
        for seed in 0..2000 {
            let mut l = layout(10.0, Spacing::Equal { count: 5 }, Ends::Gap);
            l.randomize = Some(Randomize { seed, variation: gap * 0.999 });
            let spans = lay_out(100.0, false, &l).unwrap();
            assert!(spans.windows(2).all(|w| w[1].start > w[0].end), "seed {seed}: {spans:?}");
        }
        let mut l = layout(10.0, Spacing::Equal { count: 5 }, Ends::Gap);
        l.randomize = Some(Randomize { seed: 1000, variation: gap });
        assert!(lay_out(100.0, false, &l).is_err());
    }

    #[test]
    fn tabs_at_the_ends_stop_at_the_ends_when_they_grow() {
        let mut l = layout(10.0, Spacing::Equal { count: 3 }, Ends::Tab);
        l.offsets = (2.0, 2.0);
        for seed in 0..200 {
            l.randomize = Some(Randomize { seed, variation: 5.0 });
            let spans = lay_out(100.0, false, &l).unwrap();
            assert!(spans.first().unwrap().start >= 2.0 - 1e-9 && spans.last().unwrap().end <= 98.0 + 1e-9);
        }
    }

    #[test]
    fn a_loop_counts_the_gap_across_its_closing_point() {
        // 4 tabs of 10 round 60: every gap 5, including the one across the closing point.
        let mut l = layout(10.0, Spacing::Equal { count: 4 }, Ends::Tab);
        l.randomize = Some(Randomize { seed: 3, variation: 5.0 });
        assert!(lay_out(60.0, true, &l).is_err());
        l.randomize = Some(Randomize { seed: 3, variation: 4.9 });
        let spans = lay_out(60.0, true, &l).unwrap();
        assert!(spans[0].start + 60.0 > spans[3].end);
    }

    #[test]
    fn unit_draws_cover_the_range_evenly() {
        let draws: Vec<f64> = (0..10_000).map(|i| unit_draw(1000, i)).collect();
        assert!(draws.iter().all(|d| (-1.0..=1.0).contains(d)));
        let mean = draws.iter().sum::<f64>() / draws.len() as f64;
        assert!(mean.abs() < 0.03, "{mean}");
        assert!(draws.iter().any(|d| *d > 0.99) && draws.iter().any(|d| *d < -0.99));
    }
}
