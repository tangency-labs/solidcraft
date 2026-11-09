//! Finite parallel internal pipe-thread data, in millimetres.
//! Data: Yamawa YEU23, printed pp.668 and707:
//! https://www.yamawa.eu/files/Yamawa_European_Catalogue_YEU23.pdf
//! G and Rp process recommendations are separate from basic thread dimensions.
//! G/BSPP and Rp are distinct families with cylindrical cutting-tap bores.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeFamily {
    G,
    Rp,
}
impl PipeFamily {
    pub const ALL: [Self; 2] = [Self::G, Self::Rp];
    pub fn label(self) -> &'static str {
        match self {
            Self::G => "G",
            Self::Rp => "Rp",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PipeSize {
    pub nominal: &'static str,
    pub tpi: u16,
    pub major: f64,
    pub pitch_diameter: f64,
    /// Basic minor diameter. This is separate from the process-drill recommendation.
    pub minor: f64,
    pub g_drill: f64,
    pub g_bore: f64,
    pub rp_drill: f64,
    pub rp_bore: f64,
}

const SIZES: &[PipeSize] = &[
    PipeSize {
        nominal: "1/8",
        tpi: 28,
        major: 9.728,
        pitch_diameter: 9.147,
        minor: 8.566,
        g_drill: 8.75,
        g_bore: 8.78,
        rp_drill: 8.5,
        rp_bore: 8.55,
    },
    PipeSize {
        nominal: "1/4",
        tpi: 19,
        major: 13.157,
        pitch_diameter: 12.301,
        minor: 11.445,
        g_drill: 11.75,
        g_bore: 11.78,
        rp_drill: 11.4,
        rp_bore: 11.5,
    },
    PipeSize {
        nominal: "3/8",
        tpi: 19,
        major: 16.662,
        pitch_diameter: 15.806,
        minor: 14.95,
        g_drill: 15.25,
        g_bore: 15.28,
        rp_drill: 14.8,
        rp_bore: 14.9,
    },
    PipeSize {
        nominal: "1/2",
        tpi: 14,
        major: 20.955,
        pitch_diameter: 19.793,
        minor: 18.631,
        g_drill: 19.0,
        g_bore: 19.04,
        rp_drill: 18.5,
        rp_bore: 18.55,
    },
    PipeSize {
        nominal: "3/4",
        tpi: 14,
        major: 26.441,
        pitch_diameter: 25.279,
        minor: 24.117,
        g_drill: 24.5,
        g_bore: 24.52,
        rp_drill: 24.0,
        rp_bore: 24.1,
    },
    PipeSize {
        nominal: "1",
        tpi: 11,
        major: 33.249,
        pitch_diameter: 31.77,
        minor: 30.291,
        g_drill: 30.75,
        g_bore: 30.77,
        rp_drill: 30.2,
        rp_bore: 30.25,
    },
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PipeThread {
    family: PipeFamily,
    size: &'static PipeSize,
}
impl PipeThread {
    pub fn family(self) -> PipeFamily {
        self.family
    }
    pub fn size(self) -> &'static PipeSize {
        self.size
    }
    pub fn designation(self) -> String {
        format!("{} {}", self.family.label(), self.size.nominal)
    }
    pub fn pitch(self) -> f64 {
        25.4 / f64::from(self.size.tpi)
    }
    pub fn profile_degrees(self) -> f64 {
        55.0
    }
    /// Pilot drill and recommended process bore, both from the process table.
    pub fn process_bore(self) -> (f64, f64) {
        match self.family {
            PipeFamily::G => (self.size.g_drill, self.size.g_bore),
            PipeFamily::Rp => (self.size.rp_drill, self.size.rp_bore),
        }
    }
}

pub fn pipe_thread_sizes() -> impl Iterator<Item = PipeThread> {
    PipeFamily::ALL.into_iter().flat_map(|family| SIZES.iter().map(move |size| PipeThread { family, size }))
}

pub fn parse_pipe_thread(text: &str) -> Option<PipeThread> {
    if text.len() > 80 {
        return None;
    }
    let text = text.trim();
    let (family, nominal) = if text.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("BSPP")) {
        (PipeFamily::G, text.get(4..)?)
    } else if text.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("Rp")) {
        (PipeFamily::Rp, text.get(2..)?)
    } else if text.get(..1).is_some_and(|p| p.eq_ignore_ascii_case("G")) {
        (PipeFamily::G, text.get(1..)?)
    } else {
        return None;
    };
    let size = SIZES.iter().find(|s| s.nominal == nominal.trim())?;
    Some(PipeThread { family, size })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    #[test]
    fn dimensions_and_process_references_match_the_source_tables() {
        for (nominal, tpi, major, pitch, minor, gd, gb, rd, rb) in [
            ("1/8", 28, 9.728, 9.147, 8.566, 8.75, 8.78, 8.5, 8.55),
            ("1/4", 19, 13.157, 12.301, 11.445, 11.75, 11.78, 11.4, 11.5),
            ("3/8", 19, 16.662, 15.806, 14.950, 15.25, 15.28, 14.8, 14.9),
            ("1/2", 14, 20.955, 19.793, 18.631, 19.0, 19.04, 18.5, 18.55),
            ("3/4", 14, 26.441, 25.279, 24.117, 24.5, 24.52, 24.0, 24.1),
            ("1", 11, 33.249, 31.770, 30.291, 30.75, 30.77, 30.2, 30.25),
        ] {
            for family in PipeFamily::ALL {
                let thread = parse_pipe_thread(&format!("{} {nominal}", family.label())).unwrap();
                let s = thread.size();
                assert_eq!((s.tpi, s.major, s.pitch_diameter, s.minor), (tpi, major, pitch, minor));
                assert_eq!(
                    thread.process_bore(),
                    match family {
                        PipeFamily::G => (gd, gb),
                        PipeFamily::Rp => (rd, rb),
                    }
                );
            }
        }
    }
    #[test]
    fn recognizes_internal_pipe_designations() {
        assert_eq!(pipe_thread_sizes().count(), 12);
        for family in ["G", "Rp"] {
            for nominal in ["1/8", "1/4", "3/8", "1/2", "3/4", "1"] {
                let s = parse_pipe_thread(&format!("{family} {nominal}")).unwrap();
                assert_eq!(parse_pipe_thread(&s.designation()), Some(s));
                assert_eq!(s.profile_degrees(), 55.0);
            }
        }
        assert_eq!(parse_pipe_thread("BSPP 1/4").unwrap().designation(), "G 1/4");
        assert!(parse_pipe_thread(&" ".repeat(81)).is_none());
        for bad in ["NPT 1/4", "R 1/4", "Rc 6.35", "Rc 2/8", "G 1/4 junk", "Rc NaN", "Rp", "M6", "1/4-20 UNC"] {
            assert!(parse_pipe_thread(bad).is_none(), "{bad}");
        }
    }
    #[test]
    fn nominal_labels_and_process_bores_are_distinct() {
        let g = parse_pipe_thread("G 1/4").unwrap();
        let rp = parse_pipe_thread("Rp 1/4").unwrap();
        assert_eq!(g.size().major, 13.157);
        assert_ne!(g.size().major, 25.4 / 4.0);
        assert_eq!(g.process_bore(), (11.75, 11.78));
        assert_eq!(rp.process_bore(), (11.4, 11.5));
        assert!((g.pitch() - 25.4 / 19.0).abs() < 1e-12);
        assert!(parse_pipe_thread("Rc 1/4").is_none());
    }
}
