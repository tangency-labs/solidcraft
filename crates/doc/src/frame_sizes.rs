//! Named standard steel sections (`RSC 203x89`, `SHS 100x100x5`…): the sizes a member can be
//! made of by name. Each is a built-in [`Profile`] with the standard's published dimensions, in
//! millimetres and degrees, so picking a size never means typing them.
//!
//! The dimensions are facts from openly published tables, with the source of each family in
//! [`Family::source`]; no other CAD product's library is used. Each entry also keeps the table's
//! area, which the tests hold the built section to (within the table's three significant figures).
//!
//! - RSC: taper flange channels of BS 4-1:1993 (withdrawn, still met in older designs). The inner
//!   faces of the flanges slope at 5°, and the flange thickness is the one at the middle of the
//!   flange's inside length, between the web and the toe.
//! - PFC: parallel flange channels of BS 4-1:2005.
//! - RSA: equal angles of BS EN 10056-1 (and five sizes the table adds to that range).
//! - SHS, RHS and CHS: hot-finished hollow sections of BS EN 10210-2. Square and rectangular ones
//!   have corner radii of 1.5 × the wall outside and 1.0 × the wall inside, as that standard
//!   gives them for working out section properties.

use super::Profile;

#[cfg(test)]
#[path = "frame_sizes_tests.rs"]
mod tests;

/// A family of standard sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Rsc,
    Pfc,
    Rsa,
    Shs,
    Rhs,
    Chs,
}

impl Family {
    pub const ALL: [Family; 6] = [Family::Rsc, Family::Pfc, Family::Rsa, Family::Shs, Family::Rhs, Family::Chs];

    /// The letters a size's name starts with.
    pub fn prefix(self) -> &'static str {
        match self {
            Family::Rsc => "RSC",
            Family::Pfc => "PFC",
            Family::Rsa => "RSA",
            Family::Shs => "SHS",
            Family::Rhs => "RHS",
            Family::Chs => "CHS",
        }
    }

    /// The standard the family's sizes come from.
    pub fn standard(self) -> &'static str {
        match self {
            Family::Rsc => "BS 4-1:1993 (withdrawn), taper flange channels",
            Family::Pfc => "BS 4-1:2005, parallel flange channels",
            Family::Rsa => "BS EN 10056-1, equal angles",
            Family::Shs => "BS EN 10210-2, hot-finished square hollow sections",
            Family::Rhs => "BS EN 10210-2, hot-finished rectangular hollow sections",
            Family::Chs => "BS EN 10210-2, hot-finished circular hollow sections",
        }
    }

    /// Where the family's dimensions were read (2026-11-12).
    pub fn source(self) -> &'static str {
        match self {
            Family::Rsc => {
                "BS 4:1993 taper flange channel table (dimensions, radii, 5° flange taper, area): roymech.org/Useful_Tables/Sections/chanA_dimprop"
            }
            Family::Pfc => {
                "Steel for Life Blue Book, parallel flange channels: steelforlifebluebook.co.uk/pfc/ec3-ukna/section-properties-dimensions-properties"
            }
            Family::Rsa => {
                "Steel for Life Blue Book, equal angles: steelforlifebluebook.co.uk/l-equal/ec3-ukna/section-properties-dimensions-properties"
            }
            Family::Shs => {
                "Steel for Life Blue Book, hot-finished SHS: steelforlifebluebook.co.uk/hfshs/ec3-ukna/section-properties-dimensions-properties"
            }
            Family::Rhs => {
                "Steel for Life Blue Book, hot-finished RHS: steelforlifebluebook.co.uk/hfrhs/ec3-ukna/section-properties-dimensions-properties"
            }
            Family::Chs => {
                "Steel for Life Blue Book, hot-finished CHS: steelforlifebluebook.co.uk/hfchs/ec3-ukna/section-properties-dimensions-properties"
            }
        }
    }

    /// The family a name's letters mean: its own prefix, or a name other tables use for it
    /// (`EA` and `L` for equal angles).
    fn from_prefix(p: &str) -> Option<Family> {
        match p {
            "RSC" => Some(Family::Rsc),
            "PFC" => Some(Family::Pfc),
            "RSA" | "EA" | "L" => Some(Family::Rsa),
            "SHS" => Some(Family::Shs),
            "RHS" => Some(Family::Rhs),
            "CHS" => Some(Family::Chs),
            _ => None,
        }
    }
}

/// One standard size.
#[derive(Clone, Debug, PartialEq)]
pub struct StandardSize {
    pub family: Family,
    /// Its name, such as `RSC 203x89` or `SHS 100x100x6.3`.
    pub name: String,
    /// Its section, every dimension given.
    pub profile: Profile,
    /// The area the table gives, mm².
    pub area: f64,
    /// Not in the standard's own range: a size the table adds to it.
    pub additional: bool,
    /// The numbers its name is matched by; a channel's mass per metre may follow them.
    numbers: Vec<f64>,
    mass: Option<f64>,
}

/// Numbers for a size's name, as the table prints them (no trailing `.0`), joined by `x`.
fn joined(v: &[f64]) -> String {
    v.iter().map(|x| super::short(*x)).collect::<Vec<_>>().join("x")
}

fn size(family: Family, numbers: Vec<f64>, mass: Option<f64>, profile: Profile, area_cm2: f64, additional: bool) -> StandardSize {
    let mut shown = numbers.clone();
    // A PFC is named with its mass, as BS 4-1:2005 names it (`PFC 200x90x30`).
    if family == Family::Pfc
        && let Some(m) = mass
    {
        shown.push(m);
    }
    StandardSize { family, name: format!("{} {}", family.prefix(), joined(&shown)), profile, area: area_cm2 * 100.0, additional, numbers, mass }
}

/// Every standard size, family by family in [`Family::ALL`]'s order, each family as its table
/// lists it.
pub fn standard_sizes() -> Vec<StandardSize> {
    let mut out = Vec::new();
    for &(name, mass, d, b, tw, tf, r1, r2, a) in RSC {
        let numbers = name.split('x').filter_map(|v| v.parse().ok()).collect();
        let profile = Profile::Channel {
            height: d,
            width: b,
            web: tw,
            flange: tf,
            root_radius: Some(r1),
            toe_radius: Some(r2),
            flange_slope: Some(RSC_FLANGE_SLOPE),
        };
        out.push(size(Family::Rsc, numbers, mass.parse().ok(), profile, a, false));
    }
    for &(name, h, b, tw, tf, r, a) in PFC {
        let mut numbers: Vec<f64> = name.split('x').filter_map(|v| v.parse().ok()).collect();
        let mass = numbers.pop();
        let profile =
            Profile::Channel { height: h, width: b, web: tw, flange: tf, root_radius: Some(r), toe_radius: Some(0.0), flange_slope: Some(0.0) };
        out.push(size(Family::Pfc, numbers, mass, profile, a, false));
    }
    for &(leg, t, r1, r2, a, in_range) in RSA {
        let profile = Profile::Angle { width: leg, height: leg, thickness: t, root_radius: Some(r1), toe_radius: Some(r2) };
        out.push(size(Family::Rsa, vec![leg, leg, t], None, profile, a, !in_range));
    }
    for (family, table) in [(Family::Shs, SHS), (Family::Rhs, RHS)] {
        for &(h, b, t, a) in table {
            // Named depth by width (`RHS 200x100`), as the tables are: the depth is the profile's height.
            let profile = Profile::RectTube { width: b, height: h, wall: t, outer_radius: Some(1.5 * t), inner_radius: Some(t) };
            out.push(size(family, vec![h, b, t], None, profile, a, false));
        }
    }
    for &(d, t, a) in CHS {
        out.push(size(Family::Chs, vec![d, t], None, Profile::RoundTube { diameter: d, wall: t }, a, false));
    }
    out
}

/// The longest size name looked up, in bytes.
const MAX_NAME: usize = 64;

/// A name's family letters and its numbers: `rsc 203 × 89`, `RSC203x89` and `RSC 203x89x30` all
/// read as RSC with 203 and 89 (and 30).
fn parse(name: &str) -> Option<(String, Vec<f64>)> {
    if name.len() > MAX_NAME {
        return None;
    }
    let t: String = name.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase().replace(['×', '*'], "X");
    let split = t.find(|c: char| c.is_ascii_digit())?;
    let (prefix, rest) = t.split_at(split);
    let numbers = rest.split('X').map(|v| v.parse::<f64>().ok().filter(|x| x.is_finite())).collect::<Option<Vec<_>>>()?;
    Some((prefix.to_string(), numbers))
}

fn same(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1e-9)
}

impl StandardSize {
    /// Whether this is the size `numbers` names: its numbers, or a channel's numbers followed by
    /// its mass per metre.
    fn named_by(&self, numbers: &[f64]) -> bool {
        same(&self.numbers, numbers)
            || self.mass.is_some_and(|m| numbers.split_last().is_some_and(|(last, first)| (last - m).abs() <= 1e-9 && same(&self.numbers, first)))
    }
}

/// The standard size `name` names, whatever its spacing, case or `×`: `RSC 203x89`,
/// `rsc203x89`, `PFC 200x90` or `PFC 200x90x30`, `EA 50x50x5` for `RSA 50x50x5`. A square
/// section named as an RHS is found as the SHS it is (`RHS 100x100x5`).
pub fn find(name: &str) -> Option<StandardSize> {
    let (prefix, numbers) = parse(name)?;
    let mut family = Family::from_prefix(&prefix)?;
    if family == Family::Rhs && numbers.len() == 3 && numbers.first() == numbers.get(1) {
        family = Family::Shs;
    }
    standard_sizes().into_iter().find(|s| s.family == family && s.named_by(&numbers))
}

/// The flange slope of a BS 4-1 taper flange channel, degrees.
pub const RSC_FLANGE_SLOPE: f64 = 5.0;

/// BS 4-1:1993 taper flange channels: (depth × width, mass kg/m, D, B, web, flange, root radius, toe radius, area cm²).
const RSC: &[(&str, &str, f64, f64, f64, f64, f64, f64, f64)] = &[
    ("432x102", "65", 431.8, 101.6, 12.2, 16.8, 15.2, 4.8, 83.4),
    ("381x102", "55", 381.0, 101.6, 10.4, 16.3, 15.2, 4.8, 70.1),
    ("305x102", "46", 304.8, 101.6, 10.2, 14.8, 15.2, 4.8, 58.9),
    ("305x89", "42", 304.8, 88.9, 10.2, 13.7, 13.7, 3.2, 53.3),
    ("254x89", "36", 254.0, 88.9, 9.1, 13.6, 13.7, 3.2, 45.4),
    ("254x76", "28", 254.0, 76.2, 8.1, 10.9, 12.2, 3.2, 35.9),
    ("229x89", "33", 228.6, 88.9, 8.6, 13.3, 13.7, 3.2, 41.6),
    ("229x76", "26", 228.6, 76.2, 7.6, 11.2, 12.2, 3.2, 33.2),
    ("203x89", "30", 203.2, 88.9, 8.1, 12.9, 13.7, 3.2, 37.9),
    ("203x76", "24", 203.2, 76.2, 7.1, 11.2, 12.2, 3.2, 30.4),
    ("178x89", "27", 177.8, 88.9, 7.6, 12.3, 13.7, 3.2, 34.1),
    ("178x76", "21", 177.8, 76.2, 6.6, 10.3, 12.2, 3.2, 26.6),
    ("152x89", "24", 152.4, 88.9, 7.1, 11.6, 13.7, 3.2, 30.4),
    ("152x76", "18", 152.4, 76.2, 6.4, 9.0, 12.2, 2.4, 22.8),
    ("127x64", "15", 127.0, 63.5, 6.4, 9.2, 10.7, 2.4, 19.0),
    ("102x51", "10", 101.6, 50.8, 6.1, 7.6, 9.1, 2.4, 13.3),
    ("76x38", "7", 76.2, 38.1, 5.1, 6.8, 7.6, 2.4, 8.56),
];
/// BS 4-1:2005 parallel flange channels: (designation, h, b, web, flange, root radius, area cm²).
const PFC: &[(&str, f64, f64, f64, f64, f64, f64)] = &[
    ("430x100x64", 430.0, 100.0, 11.0, 19.0, 15.0, 82.1),
    ("380x100x54", 380.0, 100.0, 9.5, 17.5, 15.0, 68.7),
    ("300x100x46", 300.0, 100.0, 9.0, 16.5, 15.0, 58.0),
    ("300x90x41", 300.0, 90.0, 9.0, 15.5, 12.0, 52.7),
    ("260x90x35", 260.0, 90.0, 8.0, 14.0, 12.0, 44.4),
    ("260x75x28", 260.0, 75.0, 7.0, 12.0, 12.0, 35.1),
    ("230x90x32", 230.0, 90.0, 7.5, 14.0, 12.0, 41.0),
    ("230x75x26", 230.0, 75.0, 6.5, 12.5, 12.0, 32.7),
    ("200x90x30", 200.0, 90.0, 7.0, 14.0, 12.0, 37.9),
    ("200x75x23", 200.0, 75.0, 6.0, 12.5, 12.0, 29.9),
    ("180x90x26", 180.0, 90.0, 6.5, 12.5, 12.0, 33.2),
    ("180x75x20", 180.0, 75.0, 6.0, 10.5, 12.0, 25.9),
    ("150x90x24", 150.0, 90.0, 6.5, 12.0, 12.0, 30.4),
    ("150x75x18", 150.0, 75.0, 5.5, 10.0, 12.0, 22.8),
    ("125x65x15", 125.0, 65.0, 5.5, 9.5, 12.0, 18.8),
    ("100x50x10", 100.0, 50.0, 5.0, 8.5, 9.0, 13.0),
];
/// BS EN 10056-1 equal angles: (leg, thickness, root radius, toe radius, area cm², in the BS EN 10056-1 range).
const RSA: &[(f64, f64, f64, f64, f64, bool)] = &[
    (200.0, 24.0, 18.0, 9.0, 90.6, true),
    (200.0, 20.0, 18.0, 9.0, 76.3, true),
    (200.0, 18.0, 18.0, 9.0, 69.1, true),
    (200.0, 16.0, 18.0, 9.0, 61.8, true),
    (150.0, 18.0, 16.0, 8.0, 51.2, false),
    (150.0, 15.0, 16.0, 8.0, 43.0, true),
    (150.0, 12.0, 16.0, 8.0, 34.8, true),
    (150.0, 10.0, 16.0, 8.0, 29.3, true),
    (120.0, 15.0, 13.0, 6.5, 34.0, false),
    (120.0, 12.0, 13.0, 6.5, 27.5, true),
    (120.0, 10.0, 13.0, 6.5, 23.2, true),
    (120.0, 8.0, 13.0, 6.5, 18.8, false),
    (100.0, 15.0, 12.0, 6.0, 28.0, false),
    (100.0, 12.0, 12.0, 6.0, 22.7, true),
    (100.0, 10.0, 12.0, 6.0, 19.2, true),
    (100.0, 8.0, 12.0, 6.0, 15.5, true),
    (90.0, 12.0, 11.0, 5.5, 20.3, false),
    (90.0, 10.0, 11.0, 5.5, 17.1, true),
    (90.0, 8.0, 11.0, 5.5, 13.9, true),
    (90.0, 7.0, 11.0, 5.5, 12.2, true),
    (80.0, 10.0, 10.0, 5.0, 15.1, true),
    (80.0, 8.0, 10.0, 5.0, 12.3, true),
    (75.0, 8.0, 9.0, 4.5, 11.4, true),
    (75.0, 6.0, 9.0, 4.5, 8.73, true),
    (70.0, 7.0, 9.0, 4.5, 9.4, true),
    (70.0, 6.0, 9.0, 4.5, 8.13, true),
    (65.0, 7.0, 9.0, 4.5, 8.73, true),
    (60.0, 8.0, 8.0, 4.0, 9.03, true),
    (60.0, 6.0, 8.0, 4.0, 6.91, true),
    (60.0, 5.0, 8.0, 4.0, 5.82, true),
    (50.0, 6.0, 7.0, 3.5, 5.69, true),
    (50.0, 5.0, 7.0, 3.5, 4.8, true),
    (50.0, 4.0, 7.0, 3.5, 3.89, true),
    (45.0, 4.5, 7.0, 3.5, 3.9, true),
    (40.0, 5.0, 6.0, 3.0, 3.79, true),
    (40.0, 4.0, 6.0, 3.0, 3.08, true),
    (35.0, 4.0, 5.0, 2.5, 2.67, true),
    (30.0, 4.0, 5.0, 2.5, 2.27, true),
    (30.0, 3.0, 5.0, 2.5, 1.74, true),
    (25.0, 4.0, 3.5, 1.75, 1.85, true),
    (25.0, 3.0, 3.5, 1.75, 1.42, true),
    (20.0, 3.0, 3.5, 1.75, 1.12, true),
];
/// BS EN 10210-2 hot-finished SHS: (h, b, wall, area cm²).
const SHS: &[(f64, f64, f64, f64)] = &[
    (40.0, 40.0, 3.2, 4.6),
    (40.0, 40.0, 3.6, 5.1),
    (40.0, 40.0, 4.0, 5.59),
    (40.0, 40.0, 5.0, 6.73),
    (40.0, 40.0, 6.3, 8.07),
    (50.0, 50.0, 3.2, 5.88),
    (50.0, 50.0, 3.6, 6.54),
    (50.0, 50.0, 4.0, 7.19),
    (50.0, 50.0, 5.0, 8.73),
    (50.0, 50.0, 6.3, 10.6),
    (50.0, 50.0, 8.0, 12.8),
    (60.0, 60.0, 3.2, 7.16),
    (60.0, 60.0, 3.6, 7.98),
    (60.0, 60.0, 4.0, 8.79),
    (60.0, 60.0, 5.0, 10.7),
    (60.0, 60.0, 6.3, 13.1),
    (60.0, 60.0, 8.0, 16.0),
    (70.0, 70.0, 3.2, 8.44),
    (70.0, 70.0, 3.6, 9.42),
    (70.0, 70.0, 4.0, 10.4),
    (70.0, 70.0, 5.0, 12.7),
    (70.0, 70.0, 6.3, 15.6),
    (70.0, 70.0, 8.0, 19.2),
    (70.0, 70.0, 10.0, 22.9),
    (80.0, 80.0, 3.2, 9.72),
    (80.0, 80.0, 3.6, 10.9),
    (80.0, 80.0, 4.0, 12.0),
    (80.0, 80.0, 5.0, 14.7),
    (80.0, 80.0, 6.3, 18.1),
    (80.0, 80.0, 8.0, 22.4),
    (80.0, 80.0, 10.0, 26.9),
    (90.0, 90.0, 3.6, 12.3),
    (90.0, 90.0, 4.0, 13.6),
    (90.0, 90.0, 5.0, 16.7),
    (90.0, 90.0, 6.3, 20.7),
    (90.0, 90.0, 8.0, 25.6),
    (90.0, 90.0, 10.0, 30.9),
    (100.0, 100.0, 3.6, 13.7),
    (100.0, 100.0, 4.0, 15.2),
    (100.0, 100.0, 5.0, 18.7),
    (100.0, 100.0, 6.3, 23.2),
    (100.0, 100.0, 8.0, 28.8),
    (100.0, 100.0, 10.0, 34.9),
    (120.0, 120.0, 4.0, 18.4),
    (120.0, 120.0, 5.0, 22.7),
    (120.0, 120.0, 6.3, 28.2),
    (120.0, 120.0, 8.0, 35.2),
    (120.0, 120.0, 10.0, 42.9),
    (120.0, 120.0, 12.5, 52.1),
    (140.0, 140.0, 5.0, 26.7),
    (140.0, 140.0, 6.3, 33.3),
    (140.0, 140.0, 8.0, 41.6),
    (140.0, 140.0, 10.0, 50.9),
    (140.0, 140.0, 12.5, 62.1),
    (140.0, 140.0, 14.2, 69.3),
    (140.0, 140.0, 16.0, 76.6),
    (150.0, 150.0, 4.0, 23.2),
    (150.0, 150.0, 5.0, 28.7),
    (150.0, 150.0, 6.3, 35.8),
    (150.0, 150.0, 8.0, 44.8),
    (150.0, 150.0, 10.0, 54.9),
    (150.0, 150.0, 12.5, 67.1),
    (150.0, 150.0, 14.2, 75.0),
    (150.0, 150.0, 16.0, 83.0),
    (160.0, 160.0, 5.0, 30.7),
    (160.0, 160.0, 6.3, 38.3),
    (160.0, 160.0, 8.0, 48.0),
    (160.0, 160.0, 10.0, 58.9),
    (160.0, 160.0, 12.5, 72.1),
    (160.0, 160.0, 14.2, 80.7),
    (160.0, 160.0, 16.0, 89.4),
    (180.0, 180.0, 6.3, 43.3),
    (180.0, 180.0, 8.0, 54.4),
    (180.0, 180.0, 10.0, 66.9),
    (180.0, 180.0, 12.5, 82.1),
    (180.0, 180.0, 14.2, 92.0),
    (180.0, 180.0, 16.0, 102.0),
    (200.0, 200.0, 5.0, 38.7),
    (200.0, 200.0, 6.3, 48.4),
    (200.0, 200.0, 8.0, 60.8),
    (200.0, 200.0, 10.0, 74.9),
    (200.0, 200.0, 12.5, 92.1),
    (200.0, 200.0, 14.2, 103.0),
    (200.0, 200.0, 16.0, 115.0),
    (200.0, 200.0, 17.5, 124.0),
    (220.0, 220.0, 8.0, 67.2),
    (220.0, 220.0, 10.0, 82.9),
    (220.0, 220.0, 12.5, 102.0),
    (220.0, 220.0, 14.2, 115.0),
    (220.0, 220.0, 16.0, 128.0),
    (220.0, 220.0, 17.5, 138.0),
    (250.0, 250.0, 5.0, 48.7),
    (250.0, 250.0, 6.3, 61.0),
    (250.0, 250.0, 8.0, 76.8),
    (250.0, 250.0, 10.0, 94.9),
    (250.0, 250.0, 12.5, 117.0),
    (250.0, 250.0, 14.2, 132.0),
    (250.0, 250.0, 16.0, 147.0),
    (250.0, 250.0, 17.5, 159.0),
    (260.0, 260.0, 10.0, 98.9),
    (260.0, 260.0, 12.5, 122.0),
    (260.0, 260.0, 14.2, 137.0),
    (260.0, 260.0, 16.0, 153.0),
    (260.0, 260.0, 17.5, 166.0),
    (300.0, 300.0, 6.3, 73.6),
    (300.0, 300.0, 8.0, 92.8),
    (300.0, 300.0, 10.0, 115.0),
    (300.0, 300.0, 12.5, 142.0),
    (300.0, 300.0, 14.2, 160.0),
    (300.0, 300.0, 16.0, 179.0),
    (300.0, 300.0, 17.5, 194.0),
    (350.0, 350.0, 8.0, 109.0),
    (350.0, 350.0, 10.0, 135.0),
    (350.0, 350.0, 12.5, 167.0),
    (350.0, 350.0, 14.2, 189.0),
    (350.0, 350.0, 16.0, 211.0),
    (350.0, 350.0, 17.5, 229.0),
    (400.0, 400.0, 8.0, 125.0),
    (400.0, 400.0, 10.0, 155.0),
    (400.0, 400.0, 12.5, 192.0),
    (400.0, 400.0, 14.2, 217.0),
    (400.0, 400.0, 16.0, 243.0),
    (400.0, 400.0, 17.5, 264.0),
];
/// BS EN 10210-2 hot-finished RHS: (h, b, wall, area cm²).
const RHS: &[(f64, f64, f64, f64)] = &[
    (50.0, 30.0, 3.2, 4.6),
    (50.0, 30.0, 3.6, 5.1),
    (50.0, 30.0, 4.0, 5.59),
    (50.0, 30.0, 5.0, 6.73),
    (50.0, 30.0, 6.3, 8.07),
    (60.0, 40.0, 3.2, 5.88),
    (60.0, 40.0, 3.6, 6.54),
    (60.0, 40.0, 4.0, 7.19),
    (60.0, 40.0, 5.0, 8.73),
    (60.0, 40.0, 6.3, 10.6),
    (60.0, 40.0, 8.0, 12.8),
    (80.0, 40.0, 3.2, 7.16),
    (80.0, 40.0, 3.6, 7.98),
    (80.0, 40.0, 4.0, 8.79),
    (80.0, 40.0, 5.0, 10.7),
    (80.0, 40.0, 6.3, 13.1),
    (80.0, 40.0, 8.0, 16.0),
    (90.0, 50.0, 3.2, 8.44),
    (90.0, 50.0, 3.6, 9.42),
    (90.0, 50.0, 4.0, 10.4),
    (90.0, 50.0, 5.0, 12.7),
    (90.0, 50.0, 6.3, 15.6),
    (90.0, 50.0, 8.0, 19.2),
    (90.0, 50.0, 10.0, 22.9),
    (100.0, 50.0, 3.2, 9.08),
    (100.0, 50.0, 3.6, 10.1),
    (100.0, 50.0, 4.0, 11.2),
    (100.0, 50.0, 5.0, 13.7),
    (100.0, 50.0, 6.3, 16.9),
    (100.0, 50.0, 8.0, 20.8),
    (100.0, 50.0, 10.0, 24.9),
    (100.0, 60.0, 3.2, 9.72),
    (100.0, 60.0, 3.6, 10.9),
    (100.0, 60.0, 4.0, 12.0),
    (100.0, 60.0, 5.0, 14.7),
    (100.0, 60.0, 6.3, 18.1),
    (100.0, 60.0, 8.0, 22.4),
    (100.0, 60.0, 10.0, 26.9),
    (120.0, 60.0, 3.6, 12.3),
    (120.0, 60.0, 4.0, 13.6),
    (120.0, 60.0, 5.0, 16.7),
    (120.0, 60.0, 6.3, 20.7),
    (120.0, 60.0, 8.0, 25.6),
    (120.0, 60.0, 10.0, 30.9),
    (120.0, 80.0, 3.6, 13.7),
    (120.0, 80.0, 4.0, 15.2),
    (120.0, 80.0, 5.0, 18.7),
    (120.0, 80.0, 6.3, 23.2),
    (120.0, 80.0, 8.0, 28.8),
    (120.0, 80.0, 10.0, 34.9),
    (150.0, 100.0, 4.0, 19.2),
    (150.0, 100.0, 5.0, 23.7),
    (150.0, 100.0, 6.3, 29.5),
    (150.0, 100.0, 8.0, 36.8),
    (150.0, 100.0, 10.0, 44.9),
    (150.0, 100.0, 12.5, 54.6),
    (160.0, 80.0, 4.0, 18.4),
    (160.0, 80.0, 5.0, 22.7),
    (160.0, 80.0, 6.3, 28.2),
    (160.0, 80.0, 8.0, 35.2),
    (160.0, 80.0, 10.0, 42.9),
    (160.0, 80.0, 12.5, 52.1),
    (200.0, 100.0, 4.0, 23.2),
    (200.0, 100.0, 5.0, 28.7),
    (200.0, 100.0, 6.3, 35.8),
    (200.0, 100.0, 8.0, 44.8),
    (200.0, 100.0, 10.0, 54.9),
    (200.0, 100.0, 12.5, 67.1),
    (200.0, 100.0, 14.2, 75.0),
    (200.0, 100.0, 16.0, 83.0),
    (200.0, 120.0, 5.0, 30.7),
    (200.0, 120.0, 6.3, 38.3),
    (200.0, 120.0, 8.0, 48.0),
    (200.0, 120.0, 10.0, 58.9),
    (200.0, 120.0, 12.5, 72.1),
    (200.0, 120.0, 14.2, 80.7),
    (200.0, 120.0, 16.0, 89.4),
    (200.0, 150.0, 5.0, 33.7),
    (200.0, 150.0, 6.3, 42.1),
    (200.0, 150.0, 8.0, 52.8),
    (200.0, 150.0, 10.0, 64.9),
    (200.0, 150.0, 12.5, 79.6),
    (200.0, 150.0, 14.2, 89.2),
    (200.0, 150.0, 16.0, 99.0),
    (250.0, 100.0, 5.0, 33.7),
    (250.0, 100.0, 6.3, 42.1),
    (250.0, 100.0, 8.0, 52.8),
    (250.0, 100.0, 10.0, 64.9),
    (250.0, 100.0, 12.5, 79.6),
    (250.0, 100.0, 14.2, 89.2),
    (250.0, 100.0, 16.0, 99.0),
    (250.0, 150.0, 5.0, 38.7),
    (250.0, 150.0, 6.3, 48.4),
    (250.0, 150.0, 8.0, 60.8),
    (250.0, 150.0, 10.0, 74.9),
    (250.0, 150.0, 12.5, 92.1),
    (250.0, 150.0, 14.2, 103.0),
    (250.0, 150.0, 16.0, 115.0),
    (250.0, 150.0, 17.5, 124.0),
    (300.0, 100.0, 5.0, 38.7),
    (300.0, 100.0, 6.3, 48.4),
    (300.0, 100.0, 8.0, 60.8),
    (300.0, 100.0, 10.0, 74.9),
    (300.0, 100.0, 12.5, 92.1),
    (300.0, 100.0, 14.2, 103.0),
    (300.0, 100.0, 16.0, 115.0),
    (300.0, 100.0, 17.5, 124.0),
    (300.0, 200.0, 5.0, 48.7),
    (300.0, 200.0, 6.3, 61.0),
    (300.0, 200.0, 8.0, 76.8),
    (300.0, 200.0, 10.0, 94.9),
    (300.0, 200.0, 12.5, 117.0),
    (300.0, 200.0, 14.2, 132.0),
    (300.0, 200.0, 16.0, 147.0),
    (300.0, 200.0, 17.5, 159.0),
];
/// BS EN 10210-2 hot-finished CHS: (diameter, wall, area cm²).
const CHS: &[(f64, f64, f64)] = &[
    (42.4, 3.2, 3.94),
    (42.4, 3.6, 4.39),
    (42.4, 4.0, 4.83),
    (42.4, 5.0, 5.87),
    (48.3, 3.2, 4.53),
    (48.3, 3.6, 5.06),
    (48.3, 4.0, 5.57),
    (48.3, 5.0, 6.8),
    (48.3, 6.3, 8.31),
    (60.3, 3.2, 5.74),
    (60.3, 3.6, 6.41),
    (60.3, 4.0, 7.07),
    (60.3, 5.0, 8.69),
    (60.3, 6.3, 10.7),
    (60.3, 8.0, 13.1),
    (76.1, 3.2, 7.33),
    (76.1, 3.6, 8.2),
    (76.1, 4.0, 9.06),
    (76.1, 5.0, 11.2),
    (76.1, 6.3, 13.8),
    (76.1, 8.0, 17.1),
    (88.9, 3.2, 8.62),
    (88.9, 3.6, 9.65),
    (88.9, 4.0, 10.7),
    (88.9, 5.0, 13.2),
    (88.9, 6.3, 16.3),
    (88.9, 8.0, 20.3),
    (88.9, 10.0, 24.8),
    (114.3, 3.6, 12.5),
    (114.3, 4.0, 13.9),
    (114.3, 5.0, 17.2),
    (114.3, 6.3, 21.4),
    (114.3, 8.0, 26.7),
    (114.3, 10.0, 32.8),
    (139.7, 3.6, 15.4),
    (139.7, 4.0, 17.1),
    (139.7, 5.0, 21.2),
    (139.7, 6.3, 26.4),
    (139.7, 8.0, 33.1),
    (168.3, 5.0, 25.7),
    (168.3, 6.3, 32.1),
    (168.3, 8.0, 40.3),
    (168.3, 10.0, 49.7),
    (168.3, 12.5, 61.2),
    (193.7, 5.0, 29.6),
    (193.7, 6.3, 37.1),
    (193.7, 8.0, 46.7),
    (193.7, 10.0, 57.7),
    (193.7, 12.5, 71.2),
    (193.7, 14.2, 80.1),
    (193.7, 16.0, 89.3),
    (219.1, 5.0, 33.6),
    (219.1, 6.3, 42.1),
    (219.1, 8.0, 53.1),
    (219.1, 10.0, 65.7),
    (219.1, 12.5, 81.1),
    (219.1, 14.2, 91.4),
    (219.1, 16.0, 102.0),
];
