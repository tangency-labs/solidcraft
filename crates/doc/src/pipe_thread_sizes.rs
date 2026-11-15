//! The sizes of the pipe-thread forms a modelled thread cuts: ISO 7-1 (55°, taper R external and
//! Rc internal; its gauge-plane diameters are also the parallel G sizes of ISO 228-1) and ASME
//! B1.20.1 NPT (60°, taper). Lengths and diameters in mm; the taper is 1 in 16 on the diameter.
//!
//! Sources (docs/threads.md):
//! - ISO 7-1:1978, Table 1, columns 1 to 8 and 15, as the public preview prints them:
//!   <https://cdn.standards.iteh.ai/samples/2856/96e1948d3d734f28b2563bcab2920245/ISO-7-1-1978.pdf>.
//! - NBS Handbook H28 (1957) Part II, Table VII.2, "Basic dimensions of American Standard taper
//!   pipe threads, NPT" (a U.S. government publication):
//!   <https://nvlpubs.nist.gov/nistpubs/Legacy/hb/nbshandbook28supp1957pt2.pdf>.

const INCH: f64 = 25.4;

/// One taper pipe-thread size, by the plane its diameters are given at (the reference plane).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PipeSize {
    /// The nominal size as the standard writes it: `1/2`, `1 1/4`.
    pub nominal: &'static str,
    /// Threads per inch.
    pub tpi: f64,
    /// The major diameter at the reference plane: ISO 7-1's gauge diameter d, NPT's E1 + h.
    pub major: f64,
    /// From the small end of the external thread to the reference plane: ISO 7-1's basic gauge
    /// length, NPT's hand-tight engagement L1. The internal thread has it at its face.
    pub gauge: f64,
    /// The external thread's length: ISO 7-1's useful thread for the basic gauge length (not
    /// less than), NPT's effective thread L2.
    pub length: f64,
}

impl PipeSize {
    pub fn pitch(&self) -> f64 {
        INCH / self.tpi
    }
}

/// ISO 7-1:1978 Table 1: designation, threads per 25.4 mm, major (gauge) diameter d, basic gauge
/// length, useful thread for the basic gauge length (columns 1, 2, 5, 8 and 15).
const ISO_7_1: &[(&str, f64, f64, f64, f64)] = &[
    ("1/16", 28.0, 7.723, 4.0, 6.5),
    ("1/8", 28.0, 9.728, 4.0, 6.5),
    ("1/4", 19.0, 13.157, 6.0, 9.7),
    ("3/8", 19.0, 16.662, 6.4, 10.1),
    ("1/2", 14.0, 20.955, 8.2, 13.2),
    ("3/4", 14.0, 26.441, 9.5, 14.5),
    ("1", 11.0, 33.249, 10.4, 16.8),
    ("1 1/4", 11.0, 41.910, 12.7, 19.1),
    ("1 1/2", 11.0, 47.803, 12.7, 19.1),
    ("2", 11.0, 59.614, 15.9, 23.4),
    ("2 1/2", 11.0, 75.184, 17.5, 26.7),
    ("3", 11.0, 87.884, 20.6, 29.8),
    ("4", 11.0, 113.030, 25.4, 35.8),
    ("5", 11.0, 138.430, 28.6, 40.1),
    ("6", 11.0, 163.830, 28.6, 40.1),
];

/// H28 Table VII.2 (inches): nominal size, outside diameter of pipe D, threads per inch, pitch
/// diameter at the beginning of the external thread E0, hand-tight engagement L1, effective
/// thread L2 (columns 1, 2, 3, 5, 6 and 9).
const NPT: &[(&str, f64, f64, f64, f64, f64)] = &[
    ("1/16", 0.3125, 27.0, 0.27118, 0.160, 0.2611),
    ("1/8", 0.405, 27.0, 0.36351, 0.1615, 0.2639),
    ("1/4", 0.540, 18.0, 0.47739, 0.2278, 0.4018),
    ("3/8", 0.675, 18.0, 0.61201, 0.240, 0.4078),
    ("1/2", 0.840, 14.0, 0.75843, 0.320, 0.5337),
    ("3/4", 1.050, 14.0, 0.96768, 0.339, 0.5457),
    ("1", 1.315, 11.5, 1.21363, 0.400, 0.6828),
    ("1 1/4", 1.660, 11.5, 1.55713, 0.420, 0.7068),
    ("1 1/2", 1.900, 11.5, 1.79609, 0.420, 0.7235),
    ("2", 2.375, 11.5, 2.26902, 0.436, 0.7565),
    ("2 1/2", 2.875, 8.0, 2.71953, 0.682, 1.1375),
    ("3", 3.500, 8.0, 3.34062, 0.766, 1.2000),
    ("3 1/2", 4.000, 8.0, 3.83750, 0.821, 1.2500),
    ("4", 4.500, 8.0, 4.33438, 0.844, 1.3000),
    ("5", 5.563, 8.0, 5.39073, 0.937, 1.4063),
    ("6", 6.625, 8.0, 6.44609, 0.958, 1.5125),
    ("8", 8.625, 8.0, 8.43359, 1.063, 1.7125),
    ("10", 10.750, 8.0, 10.54531, 1.210, 1.9250),
    ("12", 12.750, 8.0, 12.53281, 1.360, 2.1250),
];

/// NPT's thread height h = 0.8 p (H28 VII.2.2).
pub const NPT_HEIGHT: f64 = 0.8;

/// The ISO 7-1 sizes (R, Rc; their diameters are ISO 228-1's G sizes too).
pub fn iso_7_1() -> impl Iterator<Item = PipeSize> {
    ISO_7_1.iter().map(|&(nominal, tpi, major, gauge, length)| PipeSize { nominal, tpi, major, gauge, length })
}

/// The NPT sizes, the major diameter at the hand-tight plane E1 + h (E1 = E0 + L1/16).
pub fn npt() -> impl Iterator<Item = PipeSize> {
    NPT.iter().map(|&(nominal, _, tpi, e0, l1, l2)| {
        let e1 = e0 + l1 / 16.0;
        PipeSize { nominal, tpi, major: (e1 + NPT_HEIGHT / tpi) * INCH, gauge: l1 * INCH, length: l2 * INCH }
    })
}

/// The pipe's outside diameter D an NPT size is cut on (mm).
pub fn npt_pipe_od(nominal: &str) -> Option<f64> {
    NPT.iter().find(|r| r.0 == nominal).map(|r| r.1 * INCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The transcribed ISO 7-1 rows against the standard's own relations: P = 25.4/n, h =
    /// 0.640327 P, d2 = d - h, d1 = d - 2h (columns 3, 4, 6, 7, rounded to 3 decimals there).
    #[test]
    fn iso_7_1_rows_follow_their_formulas() {
        let cols: [(f64, f64, f64, f64); 15] = [
            (0.907, 0.581, 7.142, 6.561),
            (0.907, 0.581, 9.147, 8.566),
            (1.337, 0.856, 12.301, 11.445),
            (1.337, 0.856, 15.806, 14.950),
            (1.814, 1.162, 19.793, 18.631),
            (1.814, 1.162, 25.279, 24.117),
            (2.309, 1.479, 31.770, 30.291),
            (2.309, 1.479, 40.431, 38.952),
            (2.309, 1.479, 46.324, 44.845),
            (2.309, 1.479, 58.135, 56.656),
            (2.309, 1.479, 73.705, 72.226),
            (2.309, 1.479, 86.405, 84.926),
            (2.309, 1.479, 111.551, 110.072),
            (2.309, 1.479, 136.951, 135.472),
            (2.309, 1.479, 162.351, 160.872),
        ];
        for (s, (p, h, d2, d1)) in iso_7_1().zip(cols) {
            assert!((s.pitch() - p).abs() < 5e-4, "{}", s.nominal);
            assert!((0.640327 * s.pitch() - h).abs() < 5e-4, "{}", s.nominal);
            assert!((s.major - h - d2).abs() < 1.5e-3 && (s.major - 2.0 * h - d1).abs() < 1.5e-3, "{}", s.nominal);
        }
    }

    /// The transcribed NPT rows against H28's formulas: E0 = D - (0.05 D + 1.1) p and L2 = (0.80 D
    /// + 6.8) p (VII.3.4 and VII.3.5).
    #[test]
    fn npt_rows_follow_their_formulas() {
        for &(n, d, tpi, e0, _, l2) in NPT {
            let p = 1.0 / tpi;
            assert!((d - (0.05 * d + 1.1) * p - e0).abs() < 2e-5, "{n}");
            assert!(((0.80 * d + 6.8) * p - l2).abs() < 1e-4, "{n}");
        }
        let half = npt().find(|s| s.nominal == "1/2").unwrap();
        // E1 = 0.77843 in (column 7), h = 0.8/14.
        assert!((half.major - (0.77843 + 0.8 / 14.0) * INCH).abs() < 1e-4);
    }
}
