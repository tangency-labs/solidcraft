//! Finite tapped-hole catalogue. Units are millimetres internally.
//! See docs/thread-catalogue.md for sources and the limits of the drill recommendations.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TappedThreadFamily {
    Metric,
    Unc,
    Unf,
    G,
    Rp,
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
            TappedThreadFamily::G | TappedThreadFamily::Rp => 55.0,
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
/// catalogue and include their UNC/UNF suffix. G/Rp use explicit process drills; modeled Thread stays metric-only.
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

    #[test]
    fn tapped_catalogue_has_unique_valid_sizes_and_round_trips() {
        let mut names = std::collections::HashSet::new();
        for family in [TappedThreadFamily::Metric, TappedThreadFamily::Unc, TappedThreadFamily::Unf, TappedThreadFamily::G, TappedThreadFamily::Rp] {
            let sizes = tapped_thread_sizes(family);
            if family != TappedThreadFamily::Metric {
                assert_eq!(sizes.len(), if matches!(family, TappedThreadFamily::G | TappedThreadFamily::Rp) { 6 } else { 14 });
            }
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
        for invalid in ["1/4-20 UNF", "#10-32 UNC", "1/4-19 BSP", "Rc1/4", "R1/4", "1/4-18 NPT", "1/4-20", "M8xNaN", "M8x8"] {
            assert!(parse_tapped_thread(invalid).is_none(), "{invalid}");
        }
    }
}
