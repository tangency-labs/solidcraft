//! The design's profile library (#4003): named frame profiles that members refer to by id, each
//! with its placement points and stock data.
//!
//! A library profile's section is a built-in profile (its dimensions expressions over the design's
//! parameters) or a drawn section held by the entry itself, copied from a sketch, so the library
//! never depends on a sketch in the timeline. Editing an entry changes every member made from it.
//!
//! Placement points are the points of a profile that can sit on a member's line: the points every
//! profile has (its origin, centre, centroid, and the corners and edge middles of its bounding box),
//! the points its built-in kind generates (an angle's heel, a channel's web faces...), and the
//! points the entry names itself. All are worked out from the exact section.

use serde::{Deserialize, Serialize};
use solidcore_record::{Region2, Seg2, Vec2};

use crate::frame::{self, Anchor, Profile, ProfileSpec};
use crate::{DocError, Expr, Result};

#[cfg(test)]
#[path = "frame_library_tests.rs"]
mod tests;

/// Most profiles one design's library holds.
pub const MAX_LIBRARY_PROFILES: usize = 1000;
/// Most placement points an entry names itself.
pub const MAX_NAMED_POINTS: usize = 128;
/// Longest name (an entry's, a category level's, a placement point's), in bytes once trimmed.
pub const MAX_LIBRARY_NAME: usize = 96;
/// Longest material or description text, in bytes once trimmed.
pub const MAX_LIBRARY_TEXT: usize = 256;
/// Most category levels above an entry's name.
pub const MAX_CATEGORY_LEVELS: usize = 3;
/// Most regions a drawn section may hold.
pub const MAX_DRAWN_REGIONS: usize = 64;
/// Most segments in all the loops of a drawn section.
pub const MAX_DRAWN_SEGMENTS: usize = 4096;

/// The names of the placement points every profile has, in the order they are listed: the
/// spellings of [`Anchor`].
pub const COMMON_POINTS: &[&str] =
    &["origin", "center", "centroid", "top_left", "top", "top_right", "left", "right", "bottom_left", "bottom", "bottom_right"];

/// One profile of the library.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LibraryProfile {
    /// Never reused in the design (`lp1`, `lp2`...): what members refer to.
    pub id: String,
    /// Unique ignoring case: `SHS 40x40x3`.
    pub name: String,
    /// The levels the profile is filed under in pickers, outermost first: `["Steel", "SHS"]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub category: Vec<String>,
    pub section: LibrarySection,
    /// Placement points the entry names itself, beyond the ones every profile has and the ones
    /// its built-in kind generates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<NamedPoint>,
    /// The placement point a member made from it uses unless it names another. None: `origin`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_point: Option<String>,
    /// The material the stock is, by name, as the design's materials name them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// What the cut list calls it. None: the entry's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The length the stock comes in, a length expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock_length: Option<Expr>,
}

/// Where a library profile's section comes from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum LibrarySection {
    /// A built-in profile from its dimensions.
    BuiltIn { profile: ProfileSpec },
    /// Regions drawn in the profile's own x and y around its origin, held by the entry: one piece
    /// of material, holes and all.
    Drawn { regions: Vec<Region2> },
}

/// A placement point an entry names, in the profile's own coordinates (mm).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedPoint {
    pub name: String,
    pub at: [f64; 2],
}

/// A library profile worked out in a design: its section, the built-in profile it came from
/// (none for a drawn one), and every placement point.
#[derive(Clone, Debug)]
pub struct ResolvedProfile {
    pub regions: Vec<Region2>,
    pub builtin: Option<Profile>,
    pub points: Vec<(String, Vec2)>,
}

impl LibraryProfile {
    /// The length expressions the entry holds, with their labels: the built-in profile's
    /// dimensions and the stock length.
    pub fn exprs(&self) -> Vec<(&'static str, &Expr)> {
        let mut out: Vec<(&'static str, &Expr)> = Vec::new();
        if let LibrarySection::BuiltIn { profile } = &self.section {
            out.extend(profile.dims.iter().map(|(name, e)| (frame::dimension_label(name), e)));
        }
        if let Some(e) = &self.stock_length {
            out.push(("Stock length", e));
        }
        out
    }

    /// The expressions to change, as [`LibraryProfile::exprs`] lists them.
    pub fn exprs_mut(&mut self) -> Vec<&mut Expr> {
        let mut out: Vec<&mut Expr> = Vec::new();
        if let LibrarySection::BuiltIn { profile } = &mut self.section {
            out.extend(profile.dims.values_mut());
        }
        if let Some(e) = &mut self.stock_length {
            out.push(e);
        }
        out
    }

    /// The design parameters the entry reads.
    pub fn references(&self) -> Vec<String> {
        self.exprs().into_iter().flat_map(|(_, e)| crate::expr::references(e)).collect()
    }

    /// The entry worked out: each expression by `eval` (given its label, expression and kind,
    /// returning millimetres or radians).
    pub fn resolve(&self, mut eval: impl FnMut(&str, &Expr, crate::expr::Kind) -> Result<f64>) -> Result<ResolvedProfile> {
        let (regions, builtin) = match &self.section {
            LibrarySection::BuiltIn { profile } => {
                let p = profile.resolve(|name, e, kind| eval(frame::dimension_label(name), e, kind))?;
                (vec![p.region()?], Some(p))
            }
            LibrarySection::Drawn { regions } => {
                check_drawn(regions)?;
                (regions.clone(), None)
            }
        };
        let points = placement_points(&regions, builtin.as_ref(), &self.points);
        Ok(ResolvedProfile { regions, builtin, points })
    }

    /// Checks what an entry can be checked for without the design: its names, levels, texts and
    /// point names, and a drawn section's size. An error names what is wrong.
    pub fn check(&self) -> Result<()> {
        check_name("name", &self.name)?;
        if self.category.len() > MAX_CATEGORY_LEVELS {
            return Err(invalid(format!("a profile is filed under at most {MAX_CATEGORY_LEVELS} category levels")));
        }
        for level in &self.category {
            check_name("category", level)?;
        }
        for (key, text) in [("material", &self.material), ("description", &self.description)] {
            if let Some(t) = text
                && (t.trim().is_empty() || t.trim().len() > MAX_LIBRARY_TEXT)
            {
                return Err(invalid(format!("`{key}` must be 1…{MAX_LIBRARY_TEXT} bytes")));
            }
        }
        if self.points.len() > MAX_NAMED_POINTS {
            return Err(invalid(format!("at most {MAX_NAMED_POINTS} named placement points")));
        }
        let mut seen = std::collections::HashSet::new();
        for p in &self.points {
            check_name("point name", &p.name)?;
            if !point_name_chars(&p.name) {
                return Err(invalid(format!("placement point `{}`: use letters, digits, `_` and `-` only", p.name)));
            }
            if COMMON_POINTS.contains(&p.name.as_str()) {
                return Err(invalid(format!("`{}` is a placement point every profile has already", p.name)));
            }
            if !seen.insert(p.name.to_lowercase()) {
                return Err(invalid(format!("placement point `{}` is named twice", p.name)));
            }
            if !p.at.iter().all(|v| v.is_finite() && v.abs() <= frame::MAX_PROFILE_SIZE) {
                return Err(invalid(format!("placement point `{}` must lie within {} mm of the origin", p.name, frame::MAX_PROFILE_SIZE)));
            }
        }
        if let LibrarySection::Drawn { regions } = &self.section {
            check_drawn(regions)?;
        }
        Ok(())
    }
}

/// Placement point names are word-like so they read in commands and file keys alike.
fn point_name_chars(name: &str) -> bool {
    name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

fn check_name(what: &str, name: &str) -> Result<()> {
    let n = name.trim();
    if n.is_empty() || n.len() > MAX_LIBRARY_NAME || n != name {
        return Err(invalid(format!("a profile's {what} must be 1…{MAX_LIBRARY_NAME} bytes without spaces at either end")));
    }
    Ok(())
}

/// A drawn section is one or more regions of a bounded number of finite segments, each with area.
fn check_drawn(regions: &[Region2]) -> Result<()> {
    if regions.is_empty() || regions.len() > MAX_DRAWN_REGIONS {
        return Err(invalid(format!("a drawn section holds 1…{MAX_DRAWN_REGIONS} regions")));
    }
    let segs: usize = regions.iter().map(|r| r.outer.segs.len() + r.holes.iter().map(|h| h.segs.len()).sum::<usize>()).sum();
    if segs > MAX_DRAWN_SEGMENTS {
        return Err(invalid(format!("a drawn section has at most {MAX_DRAWN_SEGMENTS} segments")));
    }
    for r in regions {
        for l in std::iter::once(&r.outer).chain(&r.holes) {
            if l.segs.is_empty() {
                return Err(invalid("a drawn section has an empty loop"));
            }
            if !l.segs.iter().all(seg_finite) {
                return Err(invalid(format!("a drawn section must lie within {} mm of its origin", frame::MAX_PROFILE_SIZE)));
            }
        }
        if !(frame::section(r).area > 0.0) {
            return Err(invalid("a drawn section's region has no area"));
        }
    }
    Ok(())
}

fn seg_finite(s: &Seg2) -> bool {
    let ok = |p: &Vec2| p.x.is_finite() && p.y.is_finite() && p.x.abs() <= frame::MAX_PROFILE_SIZE && p.y.abs() <= frame::MAX_PROFILE_SIZE;
    let num = |v: f64| v.is_finite() && v.abs() <= frame::MAX_PROFILE_SIZE;
    match s {
        Seg2::Line { a, b } => ok(a) && ok(b),
        Seg2::Arc { center, radius, start, sweep } => ok(center) && num(*radius) && *radius > 0.0 && start.is_finite() && sweep.is_finite(),
        Seg2::Cubic { p0, p1, p2, p3 } => [p0, p1, p2, p3].into_iter().all(ok),
        Seg2::Conic { a, apex, b, w } => ok(a) && ok(apex) && ok(b) && w.is_finite() && *w > 0.0,
        Seg2::Bezier { p, .. } => p.iter().all(ok),
    }
}

fn invalid(msg: impl Into<String>) -> DocError {
    DocError::Invalid(msg.into())
}

/// Every placement point of a profile made of `regions`: the points every profile has, then the
/// ones `builtin`'s kind generates, then `named`. A name appears once; the first wins.
pub fn placement_points(regions: &[Region2], builtin: Option<&Profile>, named: &[NamedPoint]) -> Vec<(String, Vec2)> {
    let mut out: Vec<(String, Vec2)> = Vec::new();
    for (name, anchor) in COMMON_POINTS.iter().zip(ANCHORS) {
        out.push(((*name).to_string(), frame::anchor_point(regions, *anchor)));
    }
    if let Some(p) = builtin {
        out.extend(generated_points(p).into_iter().map(|(n, at)| (n.to_string(), at)));
    }
    out.extend(named.iter().map(|p| (p.name.clone(), Vec2::new(p.at[0], p.at[1]))));
    let mut seen = std::collections::HashSet::new();
    out.retain(|(n, _)| seen.insert(n.clone()));
    out
}

/// The anchors in the order of [`COMMON_POINTS`].
const ANCHORS: &[Anchor] = &[
    Anchor::Origin,
    Anchor::Center,
    Anchor::Centroid,
    Anchor::TopLeft,
    Anchor::Top,
    Anchor::TopRight,
    Anchor::Left,
    Anchor::Right,
    Anchor::BottomLeft,
    Anchor::Bottom,
    Anchor::BottomRight,
];

/// The placement points a built-in profile's kind generates beyond those every profile has, in the
/// profile's coordinates (its bounding box centred on the origin). Inside corners are where the
/// faces meet before any rounding, so a point stays put when a radius changes.
pub fn generated_points(p: &Profile) -> Vec<(&'static str, Vec2)> {
    let v = Vec2::new;
    match p.filled() {
        Profile::Bar { .. } | Profile::RoundBar { .. } => Vec::new(),
        Profile::RectTube { width, height, wall: t, .. } => {
            let (x, y) = (width / 2.0, height / 2.0);
            vec![
                ("inner_top_left", v(-x + t, y - t)),
                ("inner_top", v(0.0, y - t)),
                ("inner_top_right", v(x - t, y - t)),
                ("inner_left", v(-x + t, 0.0)),
                ("inner_right", v(x - t, 0.0)),
                ("inner_bottom_left", v(-x + t, -y + t)),
                ("inner_bottom", v(0.0, -y + t)),
                ("inner_bottom_right", v(x - t, -y + t)),
                ("wall_top", v(0.0, y - t / 2.0)),
                ("wall_left", v(-x + t / 2.0, 0.0)),
                ("wall_right", v(x - t / 2.0, 0.0)),
                ("wall_bottom", v(0.0, -y + t / 2.0)),
            ]
        }
        Profile::RoundTube { diameter, wall: t } => {
            let (r, m) = (diameter / 2.0 - t, diameter / 2.0 - t / 2.0);
            vec![
                ("inner_top", v(0.0, r)),
                ("inner_left", v(-r, 0.0)),
                ("inner_right", v(r, 0.0)),
                ("inner_bottom", v(0.0, -r)),
                ("wall_top", v(0.0, m)),
                ("wall_left", v(-m, 0.0)),
                ("wall_right", v(m, 0.0)),
                ("wall_bottom", v(0.0, -m)),
            ]
        }
        Profile::Angle { width, height, thickness: t, .. } => {
            let (x, y) = (width / 2.0, height / 2.0);
            vec![
                ("heel", v(-x, -y)),
                ("heel_inside", v(-x + t, -y + t)),
                ("toe_x", v(x, -y + t / 2.0)),
                ("toe_y", v(-x + t / 2.0, y)),
                ("leg_x_inside", v((t + width) / 2.0 - x, -y + t)),
                ("leg_y_inside", v(-x + t, (t + height) / 2.0 - y)),
            ]
        }
        Profile::Channel { height, width, web, flange, .. } => {
            let (x, y) = (width / 2.0, height / 2.0);
            vec![
                ("web_back", v(-x, 0.0)),
                ("web_back_top", v(-x, y)),
                ("web_back_bottom", v(-x, -y)),
                ("web_inside", v(-x + web, 0.0)),
                ("web_inside_top", v(-x + web, y - flange)),
                ("web_inside_bottom", v(-x + web, -y + flange)),
                ("web_mid", v(-x + web / 2.0, 0.0)),
                ("flange_tip_top", v(x, y - flange / 2.0)),
                ("flange_tip_bottom", v(x, -y + flange / 2.0)),
            ]
        }
        Profile::IBeam { height, width, web, flange, .. } => {
            let (x, y) = (width / 2.0, height / 2.0);
            vec![
                ("flange_top_inside", v(0.0, y - flange)),
                ("flange_bottom_inside", v(0.0, -y + flange)),
                ("flange_top_mid", v(0.0, y - flange / 2.0)),
                ("flange_bottom_mid", v(0.0, -y + flange / 2.0)),
                ("web_left", v(-web / 2.0, 0.0)),
                ("web_right", v(web / 2.0, 0.0)),
                ("flange_tip_top_left", v(-x, y - flange / 2.0)),
                ("flange_tip_top_right", v(x, y - flange / 2.0)),
                ("flange_tip_bottom_left", v(-x, -y + flange / 2.0)),
                ("flange_tip_bottom_right", v(x, -y + flange / 2.0)),
            ]
        }
    }
}

/// The point named `name` among `points`, matched exactly, then ignoring case. An error lists the
/// names there are.
pub fn find_point(points: &[(String, Vec2)], name: &str) -> Result<Vec2> {
    let hit = points.iter().find(|(n, _)| n == name).or_else(|| points.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)));
    hit.map(|(_, at)| *at).ok_or_else(|| {
        let names: Vec<&str> = points.iter().map(|(n, _)| n.as_str()).collect();
        invalid(format!("the profile has no placement point `{name}` (it has {})", names.join(", ")))
    })
}

/// The next free entry id after those in `library`.
pub fn next_id(library: &[LibraryProfile]) -> String {
    let n = library.iter().filter_map(|p| p.id.strip_prefix("lp")?.parse::<u64>().ok()).max().unwrap_or(0);
    format!("lp{}", n.saturating_add(1))
}

/// The entry `key` names: by id, then by name ignoring case.
pub fn find<'a>(library: &'a [LibraryProfile], key: &str) -> Option<&'a LibraryProfile> {
    library.iter().find(|p| p.id == key).or_else(|| library.iter().find(|p| p.name.eq_ignore_ascii_case(key.trim())))
}

impl crate::Document {
    /// The library profile with id `id`.
    pub fn library_profile(&self, id: &str) -> Option<&LibraryProfile> {
        self.frame_profiles.iter().find(|p| p.id == id)
    }

    /// The ids of the features whose members are made from library profile `id`.
    pub fn features_using_library_profile(&self, id: &str) -> Vec<u64> {
        self.features.iter().filter(|f| f.kind.library_profile() == Some(id)).map(|f| f.id).collect()
    }
}

impl crate::FeatureKind {
    /// The library profile a frame member feature is made from, by id.
    pub fn library_profile(&self) -> Option<&str> {
        match self {
            crate::FeatureKind::FrameMember { profile: frame::MemberProfile::LibraryProfile { id }, .. } => Some(id.as_str()),
            _ => None,
        }
    }
}
