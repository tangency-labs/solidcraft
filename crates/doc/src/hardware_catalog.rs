//! Press-in sheet metal hardware: what a sheet needs for a self-clinching nut, flush nut, stud or
//! standoff (#17752). A finite table of metric sizes, in millimetres. Sources, and how a value
//! was chosen where makers differ, are in docs/hardware-catalogue.md.

use crate::{TappedThreadFamily, parse_tapped_thread};

/// The kinds of press-in hardware, in table order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareKind {
    /// A self-clinching nut: its head stands proud of the sheet on the side it is pressed in from.
    Nut,
    /// A self-clinching nut that ends flush with the sheet on the other side.
    FlushNut,
    /// A self-clinching stud (a threaded pin).
    Stud,
    /// A self-clinching standoff (a threaded spacer, blind or through).
    Standoff,
}

impl HardwareKind {
    /// The name the commands and the design file use.
    pub fn name(self) -> &'static str {
        match self {
            HardwareKind::Nut => "nut",
            HardwareKind::FlushNut => "flush_nut",
            HardwareKind::Stud => "stud",
            HardwareKind::Standoff => "standoff",
        }
    }

    /// The name for people to read, in messages: `flush nut`.
    pub fn label(self) -> &'static str {
        match self {
            HardwareKind::Nut => "nut",
            HardwareKind::FlushNut => "flush nut",
            HardwareKind::Stud => "stud",
            HardwareKind::Standoff => "standoff",
        }
    }

    pub fn from_name(text: &str) -> Option<Self> {
        [HardwareKind::Nut, HardwareKind::FlushNut, HardwareKind::Stud, HardwareKind::Standoff].into_iter().find(|k| k.name() == text.trim())
    }

    /// Whether the hardware has a length of its own (a stud's or a standoff's); a nut has none.
    pub fn has_length(self) -> bool {
        matches!(self, HardwareKind::Stud | HardwareKind::Standoff)
    }
}

/// How far a mounting hole may be over its size: every maker prints +0.08 / -0.00.
pub const HOLE_TOLERANCE: f64 = 0.08;

/// One size of one kind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HardwareSize {
    pub kind: HardwareKind,
    /// ISO metric coarse designation, as `parse_tapped_thread` writes it (`M3`).
    pub thread: &'static str,
    /// The mounting hole's diameter (mm); the hole may be up to [`HOLE_TOLERANCE`] over it.
    pub hole: f64,
    /// The thinnest sheet the hardware is made for (mm).
    pub min_sheet: f64,
    /// The least distance from the hole's centre to a sheet edge (mm). Near a bend it is the
    /// distance to where the flat sheet ends and the bend starts.
    pub edge: f64,
}

impl HardwareSize {
    /// The part's id: `<kind>.<thread>` in lower case, as the design saves it (`nut.m3`).
    pub fn part(&self) -> String {
        format!("{}.{}", self.kind.name().replace('_', "-"), self.thread.to_ascii_lowercase())
    }
}

const fn row(kind: HardwareKind, thread: &'static str, hole: f64, min_sheet: f64, edge: f64) -> HardwareSize {
    HardwareSize { kind, thread, hole, min_sheet, edge }
}

use HardwareKind::{FlushNut, Nut, Standoff, Stud};

/// Sorted by kind, then by size.
const TABLE: &[HardwareSize] = &[
    row(Nut, "M2.5", 4.25, 0.8, 4.8),
    row(Nut, "M3", 4.25, 0.8, 4.8),
    row(Nut, "M4", 5.4, 0.8, 6.9),
    row(Nut, "M5", 6.4, 0.8, 7.1),
    row(Nut, "M6", 8.75, 1.4, 8.6),
    row(Nut, "M8", 10.5, 1.4, 9.7),
    row(FlushNut, "M2.5", 4.4, 1.5, 6.0),
    row(FlushNut, "M3", 4.4, 1.5, 6.0),
    row(FlushNut, "M4", 7.4, 1.5, 7.2),
    row(FlushNut, "M5", 7.92, 1.5, 8.8),
    row(FlushNut, "M6", 8.75, 3.2, 8.8),
    row(Stud, "M3", 3.0, 1.0, 5.6),
    row(Stud, "M4", 4.0, 1.0, 7.2),
    row(Stud, "M5", 5.0, 1.0, 7.2),
    row(Stud, "M6", 6.0, 1.6, 7.9),
    row(Stud, "M8", 8.0, 2.4, 9.6),
    row(Standoff, "M3", 4.2, 1.0, 6.0),
    row(Standoff, "M4", 7.2, 1.3, 8.0),
    row(Standoff, "M5", 7.2, 1.3, 8.0),
];

/// Every size the table has, in table order.
pub fn hardware_sizes() -> &'static [HardwareSize] {
    TABLE
}

/// The sizes of one kind, in table order.
pub fn hardware_sizes_of(kind: HardwareKind) -> Vec<HardwareSize> {
    TABLE.iter().filter(|s| s.kind == kind).copied().collect()
}

/// The row for a kind and a thread written as the Hole command reads it (`M3`, `m3`, `M3x0.5`).
/// None: the thread is not an ISO metric coarse size, or the table has no row for it.
pub fn hardware_size(kind: HardwareKind, thread: &str) -> Option<HardwareSize> {
    let parsed = parse_tapped_thread(thread)?;
    if parsed.family != TappedThreadFamily::Metric {
        return None;
    }
    TABLE.iter().find(|s| s.kind == kind && s.thread == parsed.designation).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table's own rules: sorted, no row twice, every number a sane length, the hole no
    /// smaller than the thread's major diameter for a stud (its pin goes through) and the nut
    /// kinds' holes above the thread, the edge distance more than half the hole.
    #[test]
    fn table_is_sorted_and_sane() {
        let t = hardware_sizes();
        assert!(!t.is_empty());
        for w in t.windows(2) {
            let key = |s: &HardwareSize| (s.kind, parse_tapped_thread(s.thread).map(|p| p.major_diameter).unwrap_or(f64::NAN));
            assert!(key(&w[0]) < key(&w[1]), "{:?} before {:?}", w[0], w[1]);
        }
        for s in t {
            let thread = parse_tapped_thread(s.thread).unwrap_or_else(|| panic!("{} is not a thread", s.thread));
            assert_eq!(thread.designation, s.thread);
            assert_eq!(thread.family, TappedThreadFamily::Metric);
            assert!(s.hole >= thread.major_diameter - 1e-9, "{s:?}");
            assert!(s.min_sheet > 0.0 && s.min_sheet < 10.0, "{s:?}");
            assert!(s.edge > s.hole / 2.0, "{s:?}");
            assert_eq!(hardware_size(s.kind, s.thread), Some(*s));
        }
    }

    #[test]
    fn looks_up_by_kind_and_written_thread() {
        let m3 = hardware_size(HardwareKind::Nut, "m3").unwrap();
        assert_eq!((m3.hole, m3.min_sheet, m3.edge), (4.25, 0.8, 4.8));
        assert_eq!(m3.part(), "nut.m3");
        assert_eq!(hardware_size(HardwareKind::FlushNut, "M5").unwrap().part(), "flush-nut.m5");
        // Fine pitch is another size; a thread the kind lacks, or not metric, has no row.
        assert_eq!(hardware_size(HardwareKind::Nut, "M3x0.35"), None);
        assert_eq!(hardware_size(HardwareKind::Standoff, "M6"), None);
        assert_eq!(hardware_size(HardwareKind::Stud, "M2.5"), None);
        assert_eq!(hardware_size(HardwareKind::Nut, "1/4-20 UNC"), None);
        assert_eq!(hardware_size(HardwareKind::Nut, ""), None);
        assert_eq!(HardwareKind::from_name("flush_nut"), Some(HardwareKind::FlushNut));
        assert_eq!(HardwareKind::from_name("hex"), None);
        assert_eq!(hardware_sizes_of(HardwareKind::Stud).len(), 5);
    }
}
