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

/// The drill of an internal taper thread (Rc, NPT). `diameter` is the tap maker's tabulated
/// drill for a straight (not reamed) hole; `exact` is the standard's internal minor diameter where
/// the thread ends (mm), the reference plane's minor at the face (d - 2h for ISO 7-1, E1 - h for
/// NPT) less the 1 in 16 taper over the thread's length (ISO 7-1's useful thread, NPT's effective
/// thread L2), which the dialog shows beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct TaperDrill {
    pub exact: f64,
    pub diameter: f64,
}

/// Rc (ISO 7-1, "PT") drills: Yamawa YEU23, printed p.709 ("Taper Internal Pipe Threads -
/// Rc(BSPT)-PT"), the maximum size of a straight bored hole when the threads have non-full
/// threads (the same values as the Rc tap's bored hole A on printed p.121), for the sizes of
/// ISO 7-1's table it lists (1/16 to 4).
/// <https://www.yamawa.eu/files/Yamawa_European_Catalogue_YEU23.pdf>
const RC_DRILLS: &[(&str, f64)] = &[
    ("1/16", 6.1),
    ("1/8", 8.1),
    ("1/4", 10.7),
    ("3/8", 14.2),
    ("1/2", 17.6),
    ("3/4", 23.0),
    ("1", 29.0),
    ("1 1/4", 37.6),
    ("1 1/2", 43.5),
    ("2", 55.0),
    ("2 1/2", 70.3),
    ("3", 82.8),
    ("4", 107.6),
];

/// NPT drills: Yamawa YEU23, printed p.710 ("American Taper Pipe Threads (NPT)", referring to
/// ASME B1.20.1), the maximum size of a straight bored hole, 1/16 to 3.
const NPT_DRILLS: &[(&str, f64)] = &[
    ("1/16", 6.05),
    ("1/8", 8.39),
    ("1/4", 10.85),
    ("3/8", 14.27),
    ("1/2", 17.60),
    ("3/4", 22.91),
    ("1", 28.78),
    ("1 1/4", 37.50),
    ("1 1/2", 43.57),
    ("2", 55.58),
    ("2 1/2", 66.28),
    ("3", 82.05),
];

/// The drill of an Rc or NPT size the tap maker tabulates; none for the others.
fn taper_drill(family: TappedThreadFamily, size: &crate::pipe_thread_sizes::PipeSize) -> Option<TaperDrill> {
    let (table, h) = match family {
        TappedThreadFamily::Npt => (NPT_DRILLS, crate::pipe_thread_sizes::NPT_HEIGHT * size.pitch()),
        _ => (RC_DRILLS, 0.640327 * size.pitch()),
    };
    let diameter = table.iter().find(|(n, _)| *n == size.nominal)?.1;
    Some(TaperDrill { exact: size.major - 2.0 * h - size.length / 16.0, diameter })
}

/// The pipe sizes of a taper family.
fn taper_sizes(family: TappedThreadFamily) -> Vec<crate::pipe_thread_sizes::PipeSize> {
    match family {
        TappedThreadFamily::Rc => crate::pipe_thread_sizes::iso_7_1().collect(),
        TappedThreadFamily::Npt => crate::pipe_thread_sizes::npt().collect(),
        _ => Vec::new(),
    }
}

impl TappedThread {
    /// An Rc or NPT size's drill: the tap maker's and the standard's exact value.
    pub fn taper_drill(&self) -> Option<TaperDrill> {
        taper_sizes(self.family).iter().find(|z| (z.major - self.major_diameter).abs() < 1e-9).and_then(|z| taper_drill(self.family, z))
    }
}

/// Why an Rc or NPT designation is no tapped hole's: a size of its standard the tap maker's table
/// has no drill for. None for any other text.
pub fn untabulated_taper_tap(text: &str) -> Option<String> {
    use crate::thread_forms::ThreadForm;
    let (family, form, listed) = match ThreadForm::of_designation(text) {
        Some(ThreadForm::Bspt) if ThreadForm::pipe_internal(text) => (TappedThreadFamily::Rc, ThreadForm::Bspt, "Rc 1/16 to Rc 4"),
        Some(ThreadForm::Npt) => (TappedThreadFamily::Npt, ThreadForm::Npt, "1/16 to 3 NPT"),
        _ => return None,
    };
    let size = form.parse(text, 1).ok()?;
    let z = taper_sizes(family).into_iter().find(|z| (z.major - size.major).abs() < 1e-9)?;
    taper_drill(family, &z).is_none().then(|| {
        format!("no tap drill is tabulated for `{}` (Yamawa's table, which the tapped-hole drills come from, lists {listed}): it is not offered as a tapped hole", text.trim())
    })
}

/// An internal taper thread (Rc, NPT) of the size tables a modelled thread cuts
/// ([`crate::pipe_thread_sizes`]), drilled with the tap maker's drill ([`TaperDrill`]); none for
/// a size without one.
fn taper_size(family: TappedThreadFamily, size: crate::pipe_thread_sizes::PipeSize) -> Option<TappedThread> {
    use crate::thread_forms::ThreadForm;
    let p = size.pitch();
    let form = if family == TappedThreadFamily::Npt { ThreadForm::Npt } else { ThreadForm::Bspt };
    let designation = form.format(size.major, p, p);
    let designation = if form == ThreadForm::Bspt { designation.replacen("R ", "Rc ", 1) } else { designation };
    let tap_drill = taper_drill(family, &size)?.diameter;
    Some(TappedThread { family, designation, major_diameter: size.major, pitch: p, tap_drill })
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
        TappedThreadFamily::Rc | TappedThreadFamily::Npt => return taper_sizes(family).into_iter().filter_map(|z| taper_size(family, z)).collect(),
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
/// catalogue and include their UNC/UNF suffix. G/Rp use explicit process drills, Rc/NPT the tap
/// maker's tabulated drills ([`TaperDrill`]); an Rc or NPT size without one is none.
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

    /// Rc and NPT holes: the size tables' diameters, the tap maker's drill (Yamawa YEU23 pp.709
    /// and 710) and the standard's minor where the thread ends (Rc 1/2: d1 18.631 at the face less
    /// 13.2 mm of useful thread at 1 in 16; 1/2 NPT: E1 - h = 0.77843 - 0.05714 in at the face less
    /// L2 = 0.5337 in at 1 in 16), a size without a tabulated drill refused, and no external R.
    #[test]
    fn internal_taper_threads_take_the_tap_makers_drill() {
        let rc = parse_tapped_thread("Rc1/2").unwrap();
        assert_eq!((rc.family, rc.designation.as_str(), rc.major_diameter, rc.tap_drill), (TappedThreadFamily::Rc, "Rc 1/2", 20.955, 17.6));
        let d = rc.taper_drill().unwrap();
        assert!((d.exact - (18.631 - 13.2 / 16.0)).abs() < 2e-3, "{d:?}");
        let npt = parse_tapped_thread("1/2 NPT").unwrap();
        assert_eq!((npt.family, npt.designation.as_str(), npt.tap_drill), (TappedThreadFamily::Npt, "1/2-14 NPT", 17.6));
        let d = npt.taper_drill().unwrap();
        assert!((d.exact - (0.77843 - 0.8 / 14.0 - 0.5337 / 16.0) * 25.4).abs() < 2e-3, "{d:?}");
        assert_eq!(rc.profile_degrees(), 55.0);
        assert_eq!(npt.profile_degrees(), 60.0);
        assert!(parse_tapped_thread("R 1/2").is_none(), "an external taper thread is no hole's");
        for untabulated in ["Rc 5", "Rc 6", "4-8 NPT", "12 NPT"] {
            assert!(parse_tapped_thread(untabulated).is_none(), "{untabulated}");
            assert!(untabulated_taper_tap(untabulated).is_some_and(|e| e.contains("no tap drill is tabulated")), "{untabulated}");
        }
        assert!(untabulated_taper_tap("Rc 1/2").is_none() && untabulated_taper_tap("M8").is_none());
        // Each tabulated drill lies within a pitch of the standard's minor where the thread ends.
        for family in [TappedThreadFamily::Rc, TappedThreadFamily::Npt] {
            for s in tapped_thread_sizes(family) {
                let d = s.taper_drill().unwrap();
                assert!((d.diameter - d.exact).abs() < s.pitch, "{}: {d:?}", s.designation);
            }
        }
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
                TappedThreadFamily::Rc => RC_DRILLS.len(),
                TappedThreadFamily::Npt => NPT_DRILLS.len(),
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
