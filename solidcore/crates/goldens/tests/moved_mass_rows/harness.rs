//! Classifies moved `mass_properties` rows of a fillet unit against a certified enclosure
//! of each body's true value (shared by the tests that classify a change's moved rows).
//!
//! For a moved entry, the old value `o` and the new `n` are exact floats, so which is nearer
//! the true value `I` depends only on which side of their midpoint `m` the true value lies.
//! With `[lo, hi]` a certified enclosure of `I` and `m` bracketed by the floats either side
//! of its rounded value ([`side`]): CLOSER when the whole enclosure is strictly on `n`'s
//! side of `m`, FARTHER when it is strictly on `o`'s, EQUAL when it holds `m`.

use goldens::case::repo_root;
use goldens::compare::values_agree;
use goldens::{registry, CaseContext};
use solidcore::mass::enclosure::{enclose_mass_properties, Bounds};
use solidcore::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use solidcore::Kernel;

/// The goldens' default `--seeds`.
const SEEDS: usize = 16;

/// Where a certified enclosure of the true value lies against two estimates of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Side {
    /// Strictly on the new value's side of their midpoint: the new value is nearer.
    Closer,
    /// Strictly on the old value's side: the new value is farther.
    Farther,
    /// Holding (or touching) the midpoint: neither can be ranked.
    Equal,
}

/// The side of `[lo, hi]` against the midpoint of `o` (old) and `n` (new). `None` when they
/// did not move (`o == n`), for an empty enclosure, or for a value that is not finite (every
/// comparison is written so that NaN fails). The midpoint `(o + n) / 2` is bracketed by the
/// floats either side of its rounded value: the sum rounds by at most half a unit in its last
/// place, and the halving is exact.
pub fn side(o: f64, n: f64, lo: f64, hi: f64) -> Option<Side> {
    if !(o.is_finite() && n.is_finite() && lo <= hi) || o == n {
        return None;
    }
    let m = (o + n) / 2.0;
    if !m.is_finite() {
        return None;
    }
    let (m_lo, m_hi) = (m.next_down(), m.next_up());
    let (above, below) = (lo > m_hi, hi < m_lo);
    Some(if (n > o && above) || (n < o && below) {
        Side::Closer
    } else if (n > o && below) || (n < o && above) {
        Side::Farther
    } else {
        Side::Equal
    })
}

/// What an EQUAL entry certifies: the new value is within `|n - o| / 2 + w` of the true one.
/// Whether that bound is within the record's accuracy (`accuracy * scale`); where it is
/// not, the enclosure is too wide for EQUAL to say the new value is as good as asked.
pub fn within_accuracy(o: f64, n: f64, band: Bounds, accuracy: f64, scale: f64) -> bool {
    (n - o).abs() / 2.0 + band.width() <= accuracy * scale
}

/// A moved row: its case, the old `mass_properties` and the new.
pub type Row = (&'static str, [f64; 12], [f64; 12]);

/// The names of a record's entries.
pub const NAMES: [&str; 12] = [
    "area",
    "volume",
    "mass",
    "centroid x",
    "centroid y",
    "centroid z",
    "inertia x",
    "inertia y",
    "inertia z",
    "product yz",
    "product zx",
    "product xy",
];

/// The moved rows of one unit and how to classify them.
pub struct Table {
    /// The unit whose rows moved.
    pub unit: &'static str,
    /// The prefix of each printed line.
    pub tag: &'static str,
    /// The moved rows.
    pub rows: &'static [Row],
    /// The tests the rows are split into, by body.
    pub parts: usize,
    /// The width asked of each enclosure, relative to its value's scale.
    pub width: f64,
    /// The rows' integration accuracy, relative to each value's scale.
    pub accuracy: f64,
}

/// A body enclosed already: its rows (old, new), its result geometry, and each entry's
/// enclosure and scale.
type Enclosed = ([f64; 12], [f64; 12], Vec<f64>, Vec<Bounds>, Vec<f64>);

/// A classification: `(case, entry, side)` for each moved entry in order, and the entries
/// whose [`within_accuracy`] bound is not met.
pub type Classified = (Vec<(&'static str, usize, Side)>, Vec<(&'static str, usize)>);

impl Table {
    /// The part row `index` is classified in: rows with the same old and new rows share
    /// one, so a body met twice is enclosed once.
    pub fn part_of(&self, index: usize) -> usize {
        let (_, old, new) = self.rows[index];
        let first = self
            .rows
            .iter()
            .position(|r| r.1 == old && r.2 == new)
            .unwrap();
        first % self.parts
    }

    /// The moved entries of the rows, in order.
    pub fn moved(&self) -> Vec<(&'static str, usize)> {
        self.rows
            .iter()
            .flat_map(|(name, old, new)| {
                (0..12)
                    .filter(|&i| old[i].to_bits() != new[i].to_bits())
                    .map(move |i| (*name, i))
            })
            .collect()
    }

    /// Classifies every moved entry of the rows in `part`.
    pub fn classify(&self, part: usize) -> Classified {
        let tag = self.tag;
        let ctx = CaseContext {
            root: repo_root(),
            seeds: SEEDS,
        };
        let reg = registry();
        let unit = reg.iter().find(|u| u.id() == self.unit).unwrap();
        let cases = unit.cases(&ctx).unwrap();
        let mut classes = Vec::new();
        let mut wide = Vec::new();
        let mut enclosed: Vec<Enclosed> = Vec::new();
        let (mut rows, mut entries, mut equal) = (0_usize, 0_usize, 0_usize);
        for (index, &(name, old, new)) in self.rows.iter().enumerate() {
            if self.part_of(index) != part {
                continue;
            }
            rows += 1;
            let case = cases.iter().find(|c| c.name == name).unwrap();
            goldens::dump::capture();
            let out = unit.run_rust(case);
            let captured = goldens::dump::take();
            let out = out.unwrap();
            // A bless keeps a committed line that agrees with the run under `values_agree`
            // (`1e-9` relative), so the run is held to that, and the classes to the
            // committed row.
            let run: Vec<f64> = out.values.get("mass_properties").unwrap().clone();
            assert_eq!(run.len(), 12, "{name}");
            for (i, (&x, &y)) in run.iter().zip(&new).enumerate() {
                assert!(
                    values_agree(x, y, None),
                    "{name}: the run's {} {x} does not reproduce the new row {y}",
                    NAMES[i]
                );
            }
            let geometry: Vec<f64> = out.values.get("result_geometry").unwrap().clone();
            let known = enclosed
                .iter()
                .find(|(o, n, g, _, _)| *o == old && *n == new && *g == geometry);
            let (bands, scales) = if let Some((_, _, _, bands, scales)) = known {
                eprintln!("{name}: the body of an earlier row (same rows, same result geometry)");
                (bands.clone(), scales.clone())
            } else {
                let mut bodies = captured.into_iter().filter(|c| c.label == "mass");
                let body = bodies.next().unwrap().brep.unwrap();
                assert!(bodies.next().is_none(), "{name}: more than one mass body");
                let k = Kernel::default();
                let mut b = body.clone_standing_for_self().unwrap();
                let r = 0.5
                    * calculate_bounding_box(&k, &mut b, BoundingBoxOptions::default())
                        .unwrap()
                        .size()
                        .length();
                let e = enclose_mass_properties(&k, &mut b, self.width).unwrap();
                if let Some((q, stop)) = e.short {
                    eprintln!("{name}: the {q:?} enclosure stopped at {stop:?}, its band wider");
                }
                let v = e.volume.hi;
                let vr2 = v * r * r;
                let mut bands = vec![e.area, e.volume, e.volume];
                let mut scales = vec![e.area.hi, v, v];
                for (group, scale) in [e.centroid, e.inertia, e.products]
                    .into_iter()
                    .zip([r, vr2, vr2])
                {
                    bands.extend(group);
                    scales.extend([scale; 3]);
                }
                enclosed.push((old, new, geometry, bands.clone(), scales.clone()));
                (bands, scales)
            };
            for i in 0..12 {
                let (o, n, band) = (old[i], new[i], bands[i]);
                let Some(class) = side(o, n, band.lo, band.hi) else {
                    assert_eq!(
                        o.to_bits(),
                        n.to_bits(),
                        "{name}: {} not classified",
                        NAMES[i]
                    );
                    continue;
                };
                entries += 1;
                if class == Side::Equal {
                    equal += 1;
                }
                eprintln!(
                    "{tag} {class:?} {name}: {} old {o:e}, now {n:e}, move/scale {:e}, \
                     enclosure [{:e}, {:e}], w/scale {:e}",
                    NAMES[i],
                    (n - o).abs() / scales[i],
                    band.lo,
                    band.hi,
                    band.width() / scales[i]
                );
                classes.push((name, i, class));
                if !within_accuracy(o, n, band, self.accuracy, scales[i]) {
                    eprintln!("{tag} WIDE {name}: {}", NAMES[i]);
                    wide.push((name, i));
                }
            }
        }
        eprintln!(
            "{tag} SUMMARY part {part}: {rows} rows, {} bodies, {entries} moved entries: \
             {equal} EQUAL",
            enclosed.len()
        );
        (classes, wide)
    }

    /// The entries of `table` whose row is in `part`.
    pub fn in_part<T: Copy>(&self, table: &[T], part: usize, case: impl Fn(&T) -> &str) -> Vec<T> {
        table
            .iter()
            .copied()
            .filter(|e| {
                let index = self.rows.iter().position(|r| r.0 == case(e)).unwrap();
                self.part_of(index) == part
            })
            .collect()
    }

    /// Classifies `part` and checks it against `expected` and `wide`: no entry FARTHER
    /// unless `expected` pins it so.
    pub fn check(
        &self,
        part: usize,
        expected: &[(&'static str, usize, Side)],
        wide: &[(&'static str, usize)],
    ) {
        let (classes, too_wide) = self.classify(part);
        assert_eq!(
            classes,
            self.in_part(expected, part, |e| e.0),
            "{classes:#?}"
        );
        assert_eq!(too_wide, self.in_part(wide, part, |e| e.0), "{too_wide:#?}");
    }
}
