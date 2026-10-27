//! Finite tapped-hole catalogue. Units are millimetres internally.
//! See docs/thread-catalogue.md for sources and the limits of the drill recommendations.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TappedThreadFamily {
    Metric,
    Unc,
    Unf,
    G,
    Rp,
    /// ISO 7-1's internal taper thread (`Rc 1/2`).
    Rc,
    /// ASME B1.20.1's taper pipe thread, internal (`1/2-14 NPT`).
    Npt,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TappedThread {
    pub family: TappedThreadFamily,
    pub designation: String,
    pub major_diameter: f64,
    pub pitch: f64,
    pub tap_drill: f64,
}

impl TappedThread {
    pub fn profile_degrees(&self) -> f64 {
        match self.family {
            TappedThreadFamily::G | TappedThreadFamily::Rp | TappedThreadFamily::Rc => 55.0,
            _ => 60.0,
        }
    }
}

fn pipe_size(size: crate::pipe_thread_catalog::PipeThread) -> TappedThread {
    use crate::pipe_thread_catalog::PipeFamily;
    TappedThread {
        family: match size.family() {
            PipeFamily::G => TappedThreadFamily::G,
            PipeFamily::Rp => TappedThreadFamily::Rp,
        },
        designation: size.designation(),
        major_diameter: size.size().major,
        pitch: size.pitch(),
        tap_drill: size.process_bore().0,
    }
}

/// The drill of an internal taper thread (Rc, NPT): `exact` is the standard's internal minor
/// diameter where the thread ends (mm), the reference plane's minor at the face (d - 2h for ISO
/// 7-1, E1 - h for NPT) less the 1 in 16 taper over the thread's length (ISO 7-1's useful thread,
/// NPT's effective thread L2), so a tap cuts full form over the whole length. The hole is
/// drilled with the largest stocked drill not larger than that (`name`, `diameter`): metric for
/// Rc, inch for NPT. Above the drill series it is bored to `exact` (no `name`).
#[derive(Clone, Debug, PartialEq)]
pub struct TaperDrill {
    pub exact: f64,
    pub diameter: f64,
    pub name: Option<String>,
}

/// Metric twist drill diameters (mm), DIN 345:2006 (taper shank twist drills, the series up to
/// 100 mm), as reproduced at <https://www.fasten.it/en/Norms/Norm/DIN%20345>; three digits that
/// reproduction drops are restored from their neighbours (15.25, 34.5, 54).
const DIN_345: &[f64] = &[
    3.0, 3.2, 3.5, 3.8, 4.0, 4.2, 4.5, 4.8, 5.0, 5.2, 5.5, 5.8, 6.0, 6.2, 6.5, 6.8, 7.0, 7.2, 7.5, 7.8, 8.0, 8.2, 8.5, 8.8, 9.0, 9.2, 9.5, 9.8, 10.0,
    10.2, 10.5, 10.8, 11.0, 11.2, 11.5, 11.8, 12.0, 12.2, 12.5, 12.8, 13.0, 13.2, 13.5, 13.8, 14.0, 14.25, 14.5, 14.75, 15.0, 15.25, 15.5, 15.75,
    16.0, 16.25, 16.5, 16.75, 17.0, 17.25, 17.5, 17.75, 18.0, 18.25, 18.5, 18.75, 19.0, 19.25, 19.5, 19.75, 20.0, 20.25, 20.5, 20.75, 21.0, 21.25,
    21.5, 21.75, 22.0, 22.25, 22.5, 22.75, 23.0, 23.25, 23.5, 23.75, 24.0, 24.25, 24.5, 24.75, 25.0, 25.25, 25.5, 25.75, 26.0, 26.25, 26.5, 26.75,
    27.25, 27.5, 27.75, 28.0, 28.25, 28.5, 28.75, 29.0, 29.25, 29.5, 29.75, 30.0, 30.25, 30.5, 30.75, 31.0, 31.25, 31.5, 31.75, 32.0, 32.5, 33.0,
    33.5, 34.5, 35.0, 35.5, 36.0, 36.5, 37.0, 37.5, 38.0, 38.5, 39.0, 39.5, 40.0, 40.5, 41.0, 41.5, 42.0, 42.5, 43.0, 43.5, 44.0, 44.5, 45.0, 45.5,
    46.0, 46.5, 47.0, 47.5, 48.0, 48.5, 49.0, 49.5, 50.0, 50.5, 51.0, 52.0, 53.0, 54.0, 55.0, 56.0, 57.0, 58.0, 59.0, 60.0, 61.0, 62.0, 63.0, 64.0,
    65.0, 66.0, 67.0, 68.0, 69.0, 70.0, 71.0, 72.0, 73.0, 74.0, 76.0, 77.0, 78.0, 79.0, 80.0, 81.0, 82.0, 83.0, 84.0, 85.0, 86.0, 87.0, 88.0, 89.0,
    90.0, 91.0, 92.0, 93.0, 94.0, 95.0, 96.0, 97.0, 98.0, 99.0, 100.0,
];

/// Letter drill sizes (in), ASME B94.11M (twist drills).
const LETTER_DRILLS: &[(char, f64)] = &[
    ('A', 0.234),
    ('B', 0.238),
    ('C', 0.242),
    ('D', 0.246),
    ('E', 0.250),
    ('F', 0.257),
    ('G', 0.261),
    ('H', 0.266),
    ('I', 0.272),
    ('J', 0.277),
    ('K', 0.281),
    ('L', 0.290),
    ('M', 0.295),
    ('N', 0.302),
    ('O', 0.316),
    ('P', 0.323),
    ('Q', 0.332),
    ('R', 0.339),
    ('S', 0.348),
    ('T', 0.358),
    ('U', 0.368),
    ('V', 0.377),
    ('W', 0.386),
    ('X', 0.397),
    ('Y', 0.404),
    ('Z', 0.413),
];

/// Fractional drill sizes by 64ths of an inch up to this many 64ths (1 1/2 in), ASME B94.11M.
const FRACTIONAL_64THS: u32 = 96;

/// A fraction of an inch in lowest terms: `11/16 in`, `1 1/4 in`.
fn inch_fraction(n64: u32) -> String {
    let (whole, mut num, mut den) = (n64 / 64, n64 % 64, 64);
    while num > 0 && num % 2 == 0 {
        num /= 2;
        den /= 2;
    }
    match (whole, num) {
        (0, _) => format!("{num}/{den} in"),
        (w, 0) => format!("{w} in"),
        (w, _) => format!("{w} {num}/{den} in"),
    }
}

/// The largest stocked drill not larger than `exact` mm: metric (DIN 345) or inch (letter and
/// fractional drills, ASME B94.11M). None past the series.
fn stocked_drill(exact: f64, inch: bool) -> Option<(f64, String)> {
    let fits = |d: f64| d <= exact + 1e-9;
    if !inch {
        let max = DIN_345.iter().copied().fold(0.0, f64::max);
        if exact > max + 1.0 {
            return None;
        }
        return DIN_345
            .iter()
            .copied()
            .filter(|d| fits(*d))
            .fold(None, |a: Option<f64>, d| Some(a.map_or(d, |a| a.max(d))))
            .map(|d| (d, format!("{d} mm")));
    }
    if exact > f64::from(FRACTIONAL_64THS) / 64.0 * 25.4 + 25.4 / 64.0 {
        return None;
    }
    let letters = LETTER_DRILLS.iter().map(|(c, d)| (d * 25.4, format!("{c} ({d:.3} in)")));
    let fractions = (1..=FRACTIONAL_64THS).map(|n| (f64::from(n) / 64.0 * 25.4, inch_fraction(n)));
    letters.chain(fractions).filter(|(d, _)| fits(*d)).max_by(|a, b| a.0.total_cmp(&b.0))
}

/// The drill of an Rc or NPT size ([`TaperDrill`]).
fn taper_drill(family: TappedThreadFamily, size: &crate::pipe_thread_sizes::PipeSize) -> TaperDrill {
    let p = size.pitch();
    let h = if family == TappedThreadFamily::Npt { crate::pipe_thread_sizes::NPT_HEIGHT * p } else { 0.640327 * p };
    let exact = size.major - 2.0 * h - size.length / 16.0;
    match stocked_drill(exact, family == TappedThreadFamily::Npt) {
        Some((diameter, name)) => TaperDrill { exact, diameter, name: Some(name) },
        None => TaperDrill { exact, diameter: (exact * 1000.0).floor() / 1000.0, name: None },
    }
}

impl TappedThread {
    /// An Rc or NPT size's drill: the standard's exact value and the stocked drill it is.
    pub fn taper_drill(&self) -> Option<TaperDrill> {
        let sizes = match self.family {
            TappedThreadFamily::Rc => crate::pipe_thread_sizes::iso_7_1().collect::<Vec<_>>(),
            TappedThreadFamily::Npt => crate::pipe_thread_sizes::npt().collect(),
            _ => return None,
        };
        sizes.iter().find(|z| (z.major - self.major_diameter).abs() < 1e-9).map(|z| taper_drill(self.family, z))
    }
}

/// An internal taper thread (Rc, NPT) of the size tables a modelled thread cuts
/// ([`crate::pipe_thread_sizes`]), drilled with [`TaperDrill`]'s stocked drill.
fn taper_size(family: TappedThreadFamily, size: crate::pipe_thread_sizes::PipeSize) -> TappedThread {
    use crate::thread_forms::ThreadForm;
    let p = size.pitch();
    let form = if family == TappedThreadFamily::Npt { ThreadForm::Npt } else { ThreadForm::Bspt };
    let designation = form.format(size.major, p, p);
    let designation = if form == ThreadForm::Bspt { designation.replacen("R ", "Rc ", 1) } else { designation };
    let tap_drill = taper_drill(family, &size).diameter;
    TappedThread { family, designation, major_diameter: size.major, pitch: p, tap_drill }
}

// Diameter/pitch pairs verified against Guhring, plus Yamawa for M12x1.25.
const METRIC_FINE: &[(f64, f64)] = &[
    (3.0, 0.35),
    (4.0, 0.5),
    (5.0, 0.5),
    (6.0, 0.75),
    (8.0, 1.0),
    (10.0, 1.0),
    (10.0, 1.25),
    (12.0, 1.0),
    (12.0, 1.25),
    (12.0, 1.5),
    (14.0, 1.5),
    (16.0, 1.5),
    (18.0, 1.5),
    (20.0, 1.5),
    (24.0, 2.0),
];

// Guhring CUT TAPS approx75% recommendations, not form-tap drills.
// Major diameters of numbered screws are from PEM; inch pitches convert exactly by25.4/TPI.
const UNIFIED: &[(&str, TappedThreadFamily, f64, u32, f64)] = &[
    ("#4-40 UNC", TappedThreadFamily::Unc, 2.8448, 40, 2.2),
    ("#4-48 UNF", TappedThreadFamily::Unf, 2.8448, 48, 2.32),
    ("#6-32 UNC", TappedThreadFamily::Unc, 3.5052, 32, 2.7051),
    ("#6-40 UNF", TappedThreadFamily::Unf, 3.5052, 40, 2.8702),
    ("#8-32 UNC", TappedThreadFamily::Unc, 4.1656, 32, 3.4),
    ("#8-36 UNF", TappedThreadFamily::Unf, 4.1656, 36, 3.4544),
    ("#10-24 UNC", TappedThreadFamily::Unc, 4.826, 24, 3.7973),
    ("#10-32 UNF", TappedThreadFamily::Unf, 4.826, 32, 4.0386),
    ("#12-24 UNC", TappedThreadFamily::Unc, 5.4864, 24, 4.45),
    ("#12-28 UNF", TappedThreadFamily::Unf, 5.4864, 28, 4.6),
    ("1/4-20 UNC", TappedThreadFamily::Unc, 6.35, 20, 5.1054),
    ("1/4-28 UNF", TappedThreadFamily::Unf, 6.35, 28, 5.45),
    ("5/16-18 UNC", TappedThreadFamily::Unc, 7.9375, 18, 6.5278),
    ("5/16-24 UNF", TappedThreadFamily::Unf, 7.9375, 24, 6.9088),
    ("3/8-16 UNC", TappedThreadFamily::Unc, 9.525, 16, 7.9375),
    ("3/8-24 UNF", TappedThreadFamily::Unf, 9.525, 24, 8.5),
    ("7/16-14 UNC", TappedThreadFamily::Unc, 11.1125, 14, 9.3472),
    ("7/16-20 UNF", TappedThreadFamily::Unf, 11.1125, 20, 9.95),
    ("1/2-13 UNC", TappedThreadFamily::Unc, 12.7, 13, 10.715625),
    ("1/2-20 UNF", TappedThreadFamily::Unf, 12.7, 20, 11.4),
    ("5/8-11 UNC", TappedThreadFamily::Unc, 15.875, 11, 13.6),
    ("5/8-18 UNF", TappedThreadFamily::Unf, 15.875, 18, 14.5),
    ("3/4-10 UNC", TappedThreadFamily::Unc, 19.05, 10, 16.5),
    ("3/4-16 UNF", TappedThreadFamily::Unf, 19.05, 16, 17.5),
    ("7/8-9 UNC", TappedThreadFamily::Unc, 22.225, 9, 19.5),
    ("7/8-14 UNF", TappedThreadFamily::Unf, 22.225, 14, 20.4),
    ("1-8 UNC", TappedThreadFamily::Unc, 25.4, 8, 22.25),
    ("1-12 UNF", TappedThreadFamily::Unf, 25.4, 12, 23.3),
];

fn metric_size(major_diameter: f64, pitch: f64) -> TappedThread {
    TappedThread {
        family: TappedThreadFamily::Metric,
        designation: crate::thread_forms::ThreadForm::Metric.format(major_diameter, pitch, pitch),
        major_diameter,
        pitch,
        // Preserve the existing metric estimate and rounding used by solid.hole.
        tap_drill: ((major_diameter - pitch) * 1000.0).round() / 1000.0,
    }
}

/// Catalogue choices for tapped holes only. Automatic Thread sizing is unchanged.
pub fn tapped_thread_sizes(family: TappedThreadFamily) -> Vec<TappedThread> {
    if matches!(family, TappedThreadFamily::G | TappedThreadFamily::Rp) {
        return crate::pipe_thread_catalog::pipe_thread_sizes().map(pipe_size).filter(|s| s.family == family).collect();
    }
    match family {
        TappedThreadFamily::Rc => return crate::pipe_thread_sizes::iso_7_1().map(|z| taper_size(family, z)).collect(),
        TappedThreadFamily::Npt => return crate::pipe_thread_sizes::npt().map(|z| taper_size(family, z)).collect(),
        _ => {}
    }

    if family == TappedThreadFamily::Metric {
        let mut sizes: Vec<_> = crate::metric_thread_sizes().iter().chain(METRIC_FINE).map(|&(d, p)| metric_size(d, p)).collect();
        sizes.sort_by(|a, b| a.major_diameter.total_cmp(&b.major_diameter).then(b.pitch.total_cmp(&a.pitch)));
        sizes
    } else {
        UNIFIED
            .iter()
            .filter(|(_, f, _, _, _)| *f == family)
            .map(|&(name, family, major_diameter, tpi, tap_drill)| TappedThread {
                family,
                designation: name.into(),
                major_diameter,
                pitch: 25.4 / f64::from(tpi),
                tap_drill,
            })
            .collect()
    }
}

/// Explicit metric custom sizes remain supported. Unified sizes must be in the verified
/// catalogue and include their UNC/UNF suffix. G/Rp use explicit process drills, Rc/NPT the drill
/// derived from their standards ([`taper_size`]).
pub fn parse_tapped_thread(text: &str) -> Option<TappedThread> {
    if text.len() > 128 {
        return None;
    }
    if let Some((d, p)) = crate::parse_metric_thread(text) {
        return Some(metric_size(d, p));
    }
    if let Some(size) = crate::pipe_thread_catalog::parse_pipe_thread(text) {
        return Some(pipe_size(size));
    }
    // An internal taper thread: Rc or NPT (an external R is no hole's).
    use crate::thread_forms::ThreadForm;
    let taper = match ThreadForm::of_designation(text) {
        Some(ThreadForm::Bspt) if ThreadForm::pipe_internal(text) => Some((TappedThreadFamily::Rc, ThreadForm::Bspt)),
        Some(ThreadForm::Bspt) => return None,
        Some(ThreadForm::Npt) => Some((TappedThreadFamily::Npt, ThreadForm::Npt)),
        _ => None,
    };
    if let Some((family, form)) = taper {
        let size = form.parse(text, 1).ok()?;
        return tapped_thread_sizes(family).into_iter().find(|s| (s.major_diameter - size.major).abs() < 1e-9);
    }
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_uppercase();
    let &(name, family, major_diameter, tpi, tap_drill) = UNIFIED.iter().find(|row| row.0 == normalized)?;
    Some(TappedThread { family, designation: name.into(), major_diameter, pitch: 25.4 / f64::from(tpi), tap_drill })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn parallel_pipe_catalogue_is_available_to_tapped_holes() {
        for label in ["G 1/4", "Rp 1/4"] {
            let s = parse_tapped_thread(label).unwrap();
            assert_eq!(s.profile_degrees(), 55.0);
            assert_eq!(s.major_diameter, 13.157);
            assert_eq!(s.tap_drill, if s.family == TappedThreadFamily::G { 11.75 } else { 11.4 });
            assert!((s.pitch - 25.4 / 19.0).abs() < 1e-12);
        }
    }

    /// Rc and NPT holes: the size tables' diameters, and the stocked drill just under the internal
    /// minor where the thread ends (Rc 1/2: d1 18.631 at the face less 13.2 mm of useful thread at
    /// 1 in 16, 17.807, drilled 17.75 mm; 1/2 NPT: E1 - h = 0.77843 - 0.05714 in at the face less
    /// L2 = 0.5337 in at 1 in 16, 0.6879 in, drilled 11/16 in), and no external R.
    #[test]
    fn internal_taper_threads_drill_to_their_minor_where_the_thread_ends() {
        let rc = parse_tapped_thread("Rc1/2").unwrap();
        assert_eq!((rc.family, rc.designation.as_str(), rc.major_diameter), (TappedThreadFamily::Rc, "Rc 1/2", 20.955));
        let d = rc.taper_drill().unwrap();
        assert!((d.exact - (18.631 - 13.2 / 16.0)).abs() < 2e-3, "{d:?}");
        assert_eq!((rc.tap_drill, d.name.as_deref()), (17.75, Some("17.75 mm")));
        let npt = parse_tapped_thread("1/2 NPT").unwrap();
        assert_eq!((npt.family, npt.designation.as_str()), (TappedThreadFamily::Npt, "1/2-14 NPT"));
        let d = npt.taper_drill().unwrap();
        assert!((d.exact - (0.77843 - 0.8 / 14.0 - 0.5337 / 16.0) * 25.4).abs() < 2e-3, "{d:?}");
        assert_eq!((npt.tap_drill, d.name.as_deref()), (11.0 / 16.0 * 25.4, Some("11/16 in")));
        // A letter drill where it is the closer stocked size (1/8 NPT), and a bore past the series.
        let small = parse_tapped_thread("1/8 NPT").unwrap().taper_drill().unwrap();
        assert!(small.diameter <= small.exact && small.name.is_some(), "{small:?}");
        let large = parse_tapped_thread("Rc 6").unwrap().taper_drill().unwrap();
        assert!(large.name.is_none() && (large.diameter - large.exact).abs() < 1e-3, "{large:?}");
        for family in [TappedThreadFamily::Rc, TappedThreadFamily::Npt] {
            for s in tapped_thread_sizes(family) {
                let d = s.taper_drill().unwrap();
                assert!(d.diameter <= d.exact + 1e-9 && d.exact - d.diameter < 0.02 * d.exact + 0.4, "{}: {d:?}", s.designation);
            }
        }
        assert_eq!(rc.profile_degrees(), 55.0);
        assert_eq!(npt.profile_degrees(), 60.0);
        assert!(parse_tapped_thread("R 1/2").is_none(), "an external taper thread is no hole's");
    }

    #[test]
    fn tapped_catalogue_has_unique_valid_sizes_and_round_trips() {
        let mut names = std::collections::HashSet::new();
        for family in [
            TappedThreadFamily::Metric,
            TappedThreadFamily::Unc,
            TappedThreadFamily::Unf,
            TappedThreadFamily::G,
            TappedThreadFamily::Rp,
            TappedThreadFamily::Rc,
            TappedThreadFamily::Npt,
        ] {
            let sizes = tapped_thread_sizes(family);
            let count = match family {
                TappedThreadFamily::Metric => sizes.len(),
                TappedThreadFamily::G | TappedThreadFamily::Rp => 6,
                TappedThreadFamily::Rc => 15,
                TappedThreadFamily::Npt => crate::pipe_thread_sizes::npt().count(),
                _ => 14,
            };
            assert_eq!(sizes.len(), count, "{family:?}");
            for size in sizes {
                assert!(names.insert(size.designation.clone()));
                assert_eq!(parse_tapped_thread(&size.designation), Some(size.clone()));
                assert!(size.major_diameter.is_finite() && size.pitch.is_finite() && size.tap_drill.is_finite());
                assert!(size.pitch > 0.0 && size.tap_drill > 0.0 && size.tap_drill < size.major_diameter);
            }
        }
    }

    #[test]
    fn tapped_catalogue_converts_inches_and_uses_tabulated_cut_drills() {
        let coarse = parse_tapped_thread(" 1/4-20 unc ").unwrap();
        assert_eq!(coarse.designation, "1/4-20 UNC");
        assert_eq!(coarse.major_diameter, 6.35);
        assert_eq!(coarse.pitch, 1.27);
        assert_eq!(coarse.tap_drill, 5.1054);
        assert!((coarse.tap_drill - (coarse.major_diameter - coarse.pitch)).abs() > 0.02);
        let fine = parse_tapped_thread("#10-32 UNF").unwrap();
        assert_eq!(fine.major_diameter, 4.826);
        assert_eq!(fine.pitch, 25.4 / 32.0);
        assert_eq!(fine.tap_drill, 4.0386);
    }

    #[test]
    fn tapped_catalogue_preserves_metric_custom_and_rejects_unverified_threads() {
        for (name, d, p, drill) in [("M8", 8.0, 1.25, 6.75), ("M8x1", 8.0, 1.0, 7.0), ("M8.5x1", 8.5, 1.0, 7.5)] {
            let size = parse_tapped_thread(name).unwrap();
            assert_eq!((size.major_diameter, size.pitch, size.tap_drill), (d, p, drill));
        }
        for invalid in ["1/4-20 UNF", "#10-32 UNC", "1/4-19 BSP", "Rc 7/8", "R1/4", "1/4-27 NPT", "1/4-20", "M8xNaN", "M8x8"] {
            assert!(parse_tapped_thread(invalid).is_none(), "{invalid}");
        }
    }
}
