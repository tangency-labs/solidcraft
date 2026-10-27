//! Frames: the profile library structural members are made of (#4003).
//!
//! A profile is a closed cross-section, an outline and its holes, built in code from a few
//! dimensions ([`Profile`]). Every built-in profile is SolidCraft's own parametric definition:
//! no vendor's or standard's size table is shipped. A dimension left out takes a default worked
//! out from the others, so `{"kind": "rect_tube", "width": 40, "height": 40, "wall": 3}` is a
//! complete profile. Any other section (a vendor's extrusion, say) comes from a sketch.
//!
//! Features and commands hold a profile as a [`ProfileSpec`]: each dimension an expression over
//! the design's parameters, worked out into a [`Profile`] when the feature is evaluated.
//!
//! Coordinates are millimetres in the profile's own plane, x across and y up, with the centre
//! of the profile's bounding box at the origin. The outline runs counter-clockwise and holes
//! clockwise, as [`Region2`] expects.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Loop2, Region2, Seg2, Vec2, Vec3};

use crate::expr::Kind;
use crate::{DocError, Expr, Result};

#[cfg(test)]
#[path = "frame_tests.rs"]
mod tests;

/// Most members one frame member feature makes.
pub const MAX_MEMBERS: usize = 1000;
/// The largest profile dimension (mm).
pub const MAX_PROFILE_SIZE: f64 = 10_000.0;
/// The smallest profile dimension (mm): walls and sides thinner than this are refused, and so
/// are radii smaller than this but not 0.
pub const MIN_PROFILE_DIMENSION: f64 = 0.01;
/// How far from vertical (radians) a member's line may lean and still count as vertical for its
/// orientation ([`member_axes`]).
pub const VERTICAL_TOLERANCE: f64 = 1e-6;

/// A built-in profile and its dimensions (mm). `Option` dimensions take a default worked out from
/// the profile's size ([`Profile::filled`] says which).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Profile {
    /// A solid rectangle (flat bar, square bar), its corners optionally rounded.
    Bar {
        width: f64,
        height: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        corner_radius: Option<f64>,
    },
    /// A solid circle.
    RoundBar { diameter: f64 },
    /// A rectangular or square hollow section. The outer corner radius defaults to twice the
    /// wall; the inner radius is the outer one less the wall.
    RectTube {
        width: f64,
        height: f64,
        wall: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outer_radius: Option<f64>,
    },
    /// A round hollow section (pipe or tube).
    RoundTube { diameter: f64, wall: f64 },
    /// An L angle: one leg `width` along x, the other `height` along y, both `thickness` thick,
    /// the heel at the bottom left. The root radius defaults to the thickness, the toe radius to
    /// half of it, both made smaller where the legs leave less room.
    Angle {
        width: f64,
        height: f64,
        thickness: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        root_radius: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        toe_radius: Option<f64>,
    },
    /// A channel with parallel flanges, open towards +x: the web `web` thick along the left side,
    /// flanges `width` long and `flange` thick. The root radius defaults to the web thickness (less
    /// where the flanges or the web leave less room), the toe radius to none.
    Channel {
        height: f64,
        width: f64,
        web: f64,
        flange: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        root_radius: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        toe_radius: Option<f64>,
    },
    /// An I or H beam with parallel flanges. The root radius defaults to the web thickness (less
    /// where the flanges or the web leave less room).
    IBeam {
        height: f64,
        width: f64,
        web: f64,
        flange: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        root_radius: Option<f64>,
    },
}

/// One built-in profile kind, for listing.
#[derive(Clone, Debug, Serialize)]
pub struct ProfileKind {
    pub kind: &'static str,
    pub label: &'static str,
    /// The dimensions it needs.
    pub required: &'static [&'static str],
    /// The dimensions that default when left out.
    pub optional: &'static [&'static str],
    /// A complete example, as JSON.
    pub example: Profile,
}

/// Every built-in profile kind.
pub fn profile_kinds() -> Vec<ProfileKind> {
    vec![
        ProfileKind {
            kind: "bar",
            label: "Bar",
            required: &["width", "height"],
            optional: &["corner_radius"],
            example: Profile::Bar { width: 40.0, height: 10.0, corner_radius: None },
        },
        ProfileKind { kind: "round_bar", label: "Round Bar", required: &["diameter"], optional: &[], example: Profile::RoundBar { diameter: 20.0 } },
        ProfileKind {
            kind: "rect_tube",
            label: "Rectangular Tube",
            required: &["width", "height", "wall"],
            optional: &["outer_radius"],
            example: Profile::RectTube { width: 40.0, height: 40.0, wall: 3.0, outer_radius: None },
        },
        ProfileKind {
            kind: "round_tube",
            label: "Round Tube",
            required: &["diameter", "wall"],
            optional: &[],
            example: Profile::RoundTube { diameter: 42.4, wall: 3.2 },
        },
        ProfileKind {
            kind: "angle",
            label: "L-Angle",
            required: &["width", "height", "thickness"],
            optional: &["root_radius", "toe_radius"],
            example: Profile::Angle { width: 50.0, height: 50.0, thickness: 5.0, root_radius: None, toe_radius: None },
        },
        ProfileKind {
            kind: "channel",
            label: "Channel",
            required: &["height", "width", "web", "flange"],
            optional: &["root_radius", "toe_radius"],
            example: Profile::Channel { height: 100.0, width: 50.0, web: 6.0, flange: 8.0, root_radius: None, toe_radius: None },
        },
        ProfileKind {
            kind: "i_beam",
            label: "I-Beam",
            required: &["height", "width", "web", "flange"],
            optional: &["root_radius"],
            example: Profile::IBeam { height: 100.0, width: 50.0, web: 4.5, flange: 7.0, root_radius: None },
        },
    ]
}

/// The label of a profile dimension, as the feature's inputs show it.
pub fn dimension_label(name: &str) -> &'static str {
    match name {
        "width" => "Width",
        "height" => "Height",
        "wall" => "Wall",
        "diameter" => "Diameter",
        "thickness" => "Thickness",
        "web" => "Web thickness",
        "flange" => "Flange thickness",
        "corner_radius" => "Corner radius",
        "outer_radius" => "Outer radius",
        "root_radius" => "Root radius",
        "toe_radius" => "Toe radius",
        _ => "Dimension",
    }
}

/// A built-in profile as a feature saves it and a command takes it: its kind and each dimension
/// as an expression over the design's parameters (`"tube_wall"`, `"2 in"`, a number). Saved as
/// one JSON object, `{"kind": "rect_tube", "width": "40", ...}`; numbers read as their text. A
/// length without a unit is millimetres here: commands give bare numbers the design's unit first.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileSpec {
    pub kind: String,
    pub dims: BTreeMap<String, Expr>,
}

impl ProfileSpec {
    /// The profile, each dimension worked out by `eval` (given the dimension's name and its
    /// expression, returning millimetres).
    pub fn resolve(&self, mut eval: impl FnMut(&str, &Expr) -> Result<f64>) -> Result<Profile> {
        let mut o = serde_json::Map::new();
        o.insert("kind".into(), serde_json::Value::String(self.kind.clone()));
        for (name, e) in &self.dims {
            let v = eval(name, e)?;
            let n = serde_json::Number::from_f64(v).ok_or_else(|| invalid(format!("profile `{name}` is not a finite length")))?;
            o.insert(name.clone(), serde_json::Value::Number(n));
        }
        serde_json::from_value(serde_json::Value::Object(o)).map_err(|e| invalid(format!("profile: {e}")))
    }
}

impl Serialize for ProfileSpec {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut m = s.serialize_map(Some(self.dims.len() + 1))?;
        m.serialize_entry("kind", &self.kind)?;
        for (k, v) in &self.dims {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

impl<'de> Deserialize<'de> for ProfileSpec {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        use serde::de::Error;
        let mut m = BTreeMap::<String, serde_json::Value>::deserialize(d)?;
        let kind = match m.remove("kind") {
            Some(serde_json::Value::String(k)) => k,
            _ => return Err(D::Error::custom("a profile needs `kind` (bar, round_bar, rect_tube, round_tube, angle, channel or i_beam)")),
        };
        let kinds = profile_kinds();
        let info = kinds.iter().find(|k| k.kind == kind).ok_or_else(|| {
            D::Error::custom(format!("unknown profile kind `{kind}` (bar, round_bar, rect_tube, round_tube, angle, channel or i_beam)"))
        })?;
        let mut dims = BTreeMap::new();
        for (name, v) in m {
            if !info.required.contains(&name.as_str()) && !info.optional.contains(&name.as_str()) {
                return Err(D::Error::custom(format!("a {kind} profile has no dimension `{name}`")));
            }
            let text = match v {
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::String(t) if !t.trim().is_empty() && t.len() <= 4096 => t,
                _ => return Err(D::Error::custom(format!("profile `{name}` must be a number or an expression"))),
            };
            dims.insert(name, Expr::new(text));
        }
        if let Some(missing) = info.required.iter().find(|r| !dims.contains_key(**r)) {
            return Err(D::Error::custom(format!("a {kind} profile needs `{missing}`")));
        }
        Ok(ProfileSpec { kind, dims })
    }
}

impl crate::walk::Walk for ProfileSpec {
    fn walk_exprs<'a>(&'a self, f: &mut dyn FnMut(crate::walk::ExprItem<'a>)) {
        for (name, e) in &self.dims {
            f((dimension_label(name), e, Kind::Length, crate::walk::ExprRole::Input));
        }
    }
    fn walk_exprs_mut<'a>(&'a mut self, f: &mut dyn FnMut(crate::walk::ExprItemMut<'a>)) {
        for (name, e) in self.dims.iter_mut() {
            f((dimension_label(name), e, Kind::Length, crate::walk::ExprRole::Input));
        }
    }
}

pub use crate::eval::frame_member::{member_line_ids, member_names};

/// Where a frame member's cross-section comes from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum MemberProfile {
    /// A built-in profile, from its dimensions.
    Library { profile: ProfileSpec },
    /// The profiles of a sketch, in the sketch's own x and y, its origin the profile's origin.
    /// Every profile (the default) means the material as nested loops alternate: an outline is
    /// material, a loop inside it a hole, a loop inside that material again, so a tube drawn as
    /// two circles is its wall.
    Sketch {
        sketch: crate::SketchId,
        #[serde(default = "crate::document::all_profiles")]
        #[walk(profiles_of = sketch)]
        profiles: crate::ProfileSel,
    },
}

/// One member: the member a `FrameMember` feature lays on one of its lines. A member is known by
/// its feature and line, not by its body, so its trims (and later its cut list row) stay with it
/// when its body is rebuilt or renamed.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, solidcraft_derive::Walk)]
pub struct MemberRef {
    pub feature: crate::FeatureName,
    pub line: String,
}

/// A side of a member: of its profile's bounding box, in the profile's own x and y (`right` is
/// +x, `top` +y), so the side follows the member when its profile turns or is mirrored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Right,
    Left,
    Top,
    Bottom,
}

/// What a member is trimmed (or extended) to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TrimTarget {
    /// A side of another member (a butt joint), worked out from that member's line and profile,
    /// so it follows sketch and parameter edits. No side: the side the trimmed member meets.
    Member {
        member: MemberRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        side: Option<Side>,
    },
    /// A plane: an origin or construction plane, or a planar face of a body.
    Plane { plane: crate::PlaneRef },
}

/// Which end of a member a trim moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrimEnd {
    /// The end nearer the target.
    #[default]
    Auto,
    /// The end at its line's start.
    Start,
    /// The end at its line's end.
    End,
}

impl TrimEnd {
    pub fn is_auto(&self) -> bool {
        *self == TrimEnd::Auto
    }
}

/// Most targets one trim feature trims to.
pub const MAX_TRIM_TARGETS: usize = 256;
/// The largest weld gap (mm).
pub const MAX_TRIM_GAP: f64 = 1000.0;

/// A member as the model holds it after the features so far: its body, and where its ends are, so
/// a trim rebuilds it from its feature's line and profile with its ends moved.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemberRecord {
    /// The `FrameMember` feature, by id, and the line.
    pub feature: u64,
    pub line: String,
    pub body: String,
    /// The plane each end (start, end) is cut in, its normal out of the member; none: square at
    /// its line's end.
    pub ends: [Option<solidcraft_geom::Plane>; 2],
    /// Shared body of the last member/trim build. Body identity detects even a symmetric
    /// rotation or a modification entirely inside the outline. Saved states preserve whether
    /// the live body still shares this shape; old caches are evaluated again.
    #[serde(skip)]
    pub(crate) built: Option<solidcraft_kernel::Body>,
}

impl MemberRecord {
    /// Whether this is the body last built by Frame Member or Trim/Extend.
    /// A later body operation, including a symmetric move, invalidates that identity.
    pub fn is_unchanged(&self, body: &solidcraft_kernel::Body) -> bool {
        self.built.as_ref().is_some_and(|built| built.same(body))
    }
}

impl Anchor {
    pub fn is_origin(&self) -> bool {
        *self == Anchor::Origin
    }
}

/// The point of a profile that sits on a member's line (or on `at` when a profile is inserted):
/// the profile's own origin (a built-in profile's bounding-box centre, a sketch profile's sketch
/// origin), its centroid, or a corner or edge middle of its bounding box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    #[default]
    Origin,
    Center,
    Centroid,
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

/// A profile's section properties (mm², mm, mm⁴). Second moments are about axes through the
/// centroid, parallel to the profile's x and y.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Section {
    pub area: f64,
    pub centroid: [f64; 2],
    /// ∫ y² dA about the centroid: bending about the x axis.
    pub ixx: f64,
    /// ∫ x² dA about the centroid: bending about the y axis.
    pub iyy: f64,
    pub ixy: f64,
    /// The bounding box's size.
    pub width: f64,
    pub height: f64,
}

fn invalid(msg: impl Into<String>) -> DocError {
    DocError::Invalid(msg.into())
}

/// `v` as a dimension: finite, at least [`MIN_PROFILE_DIMENSION`] and at most
/// [`MAX_PROFILE_SIZE`].
fn dim(name: &str, v: f64) -> Result<f64> {
    if v.is_finite() && (MIN_PROFILE_DIMENSION..=MAX_PROFILE_SIZE).contains(&v) {
        Ok(v)
    } else {
        Err(invalid(format!("profile `{name}` must be between {MIN_PROFILE_DIMENSION} and {MAX_PROFILE_SIZE} mm (got {v})")))
    }
}

/// `v` as a radius: 0 (a sharp corner), or from [`MIN_PROFILE_DIMENSION`] up to
/// [`MAX_PROFILE_SIZE`].
fn radius(name: &str, v: f64) -> Result<f64> {
    if v == 0.0 || (v.is_finite() && (MIN_PROFILE_DIMENSION..=MAX_PROFILE_SIZE).contains(&v)) {
        Ok(v)
    } else {
        Err(invalid(format!("profile `{name}` must be 0 or between {MIN_PROFILE_DIMENSION} and {MAX_PROFILE_SIZE} mm (got {v})")))
    }
}

/// `a` less than `b`.
fn less(a_name: &str, a: f64, b_name: &str, b: f64) -> Result<()> {
    if a < b { Ok(()) } else { Err(invalid(format!("profile `{a_name}` ({a}) must be less than {b_name} ({b})"))) }
}

/// `a` less than half of `b`.
fn less_half(a_name: &str, a: f64, b_name: &str, b: f64) -> Result<()> {
    if 2.0 * a < b { Ok(()) } else { Err(invalid(format!("profile `{a_name}` ({a}) must be less than half {b_name} ({})", b / 2.0))) }
}

/// Radius `r` at most `room` (with a little slack for rounding).
fn at_most(what: &str, r: f64, room: f64, why: &str) -> Result<()> {
    if r <= room + 1e-9 * (1.0 + room.abs()) {
        Ok(())
    } else {
        Err(invalid(format!("profile {what} ({r}) must be at most {} mm: {why}", room.max(0.0))))
    }
}

/// A number for a name: up to four decimals, without trailing zeros.
fn short(x: f64) -> String {
    let t = format!("{x:.4}");
    let t = if t.contains('.') { t.trim_end_matches('0').trim_end_matches('.') } else { &t };
    if t == "-0" { "0".into() } else { t.to_string() }
}

impl Profile {
    /// The kind's id (`rect_tube`, `angle`…).
    pub fn kind(&self) -> &'static str {
        match self {
            Profile::Bar { .. } => "bar",
            Profile::RoundBar { .. } => "round_bar",
            Profile::RectTube { .. } => "rect_tube",
            Profile::RoundTube { .. } => "round_tube",
            Profile::Angle { .. } => "angle",
            Profile::Channel { .. } => "channel",
            Profile::IBeam { .. } => "i_beam",
        }
    }

    /// A short description from the kind and its main sizes, such as `Rect Tube 40x40x3`: what a
    /// member body is named after.
    pub fn description(&self) -> String {
        let d = |v: &[f64]| v.iter().map(|x| short(*x)).collect::<Vec<_>>().join("x");
        match *self {
            Profile::Bar { width, height, .. } => format!("Bar {}", d(&[width, height])),
            Profile::RoundBar { diameter } => format!("Round Bar {}", short(diameter)),
            Profile::RectTube { width, height, wall, .. } => format!("Rect Tube {}", d(&[width, height, wall])),
            Profile::RoundTube { diameter, wall } => format!("Round Tube {}", d(&[diameter, wall])),
            Profile::Angle { width, height, thickness, .. } => format!("Angle {}", d(&[width, height, thickness])),
            Profile::Channel { height, width, web, .. } => format!("Channel {}", d(&[height, width, web])),
            Profile::IBeam { height, width, web, .. } => format!("I-Beam {}", d(&[height, width, web])),
        }
    }

    /// The profile with every defaulted dimension written out. A default radius is made smaller
    /// where the profile leaves less room for it than its usual size (a short leg, a narrow
    /// flange), so leaving radii out never makes a profile that does not fit.
    pub fn filled(&self) -> Profile {
        let room = |x: f64| if x.is_finite() { x.max(0.0) } else { 0.0 };
        match *self {
            Profile::Bar { width, height, corner_radius } => Profile::Bar { width, height, corner_radius: Some(corner_radius.unwrap_or(0.0)) },
            Profile::RectTube { width, height, wall, outer_radius } => {
                let r = outer_radius.unwrap_or_else(|| (2.0 * wall).min(width.min(height) / 2.0));
                Profile::RectTube { width, height, wall, outer_radius: Some(r) }
            }
            Profile::Angle { width, height, thickness: t, root_radius, toe_radius } => {
                // Root and toe share the inside length of the shorter leg.
                let leg = room(width.min(height) - t);
                let (r1, r2) = match (root_radius, toe_radius) {
                    (Some(r1), Some(r2)) => (r1, r2),
                    (Some(r1), None) => (r1, (t / 2.0).min(room(leg - r1))),
                    (None, Some(r2)) => (t.min(room(leg - r2)), r2),
                    (None, None) => {
                        let k = if 1.5 * t > leg { leg / (1.5 * t) } else { 1.0 };
                        (t * k, t / 2.0 * k)
                    }
                };
                Profile::Angle { width, height, thickness: t, root_radius: Some(r1), toe_radius: Some(r2) }
            }
            Profile::Channel { height, width, web, flange, root_radius, toe_radius } => {
                let r2 = toe_radius.unwrap_or(0.0);
                // The root shares a flange's inside length with the toe, and the web's inside
                // height with the other root.
                let r1 = root_radius.unwrap_or_else(|| web.min(room(width - web - r2)).min(room(height - 2.0 * flange) / 2.0));
                Profile::Channel { height, width, web, flange, root_radius: Some(r1), toe_radius: Some(r2) }
            }
            Profile::IBeam { height, width, web, flange, root_radius } => {
                let r = root_radius.unwrap_or_else(|| web.min(room(width - web) / 2.0).min(room(height - 2.0 * flange) / 2.0));
                Profile::IBeam { height, width, web, flange, root_radius: Some(r) }
            }
            Profile::RoundBar { .. } | Profile::RoundTube { .. } => self.clone(),
        }
    }

    /// The bounding box's width and height (mm), before validation.
    pub fn size(&self) -> (f64, f64) {
        match *self {
            Profile::Bar { width, height, .. }
            | Profile::RectTube { width, height, .. }
            | Profile::Angle { width, height, .. }
            | Profile::Channel { width, height, .. }
            | Profile::IBeam { width, height, .. } => (width, height),
            Profile::RoundBar { diameter } | Profile::RoundTube { diameter, .. } => (diameter, diameter),
        }
    }

    /// The profile's cross-section, the bounding box centred on the origin. An error names the
    /// dimension that is out of range or does not fit.
    pub fn region(&self) -> Result<Region2> {
        let f = self.filled();
        match f {
            Profile::Bar { width, height, corner_radius } => {
                let (w, h) = (dim("width", width)?, dim("height", height)?);
                let r = radius("corner_radius", corner_radius.unwrap_or(0.0))?;
                if r > w.min(h) / 2.0 {
                    return Err(invalid(format!("profile `corner_radius` ({r}) must be at most half the smaller side ({})", w.min(h) / 2.0)));
                }
                Ok(Region2 { outer: rounded_rect(w, h, r)?, holes: Vec::new() })
            }
            Profile::RoundBar { diameter } => Ok(Region2 { outer: Loop2::circle(Vec2::ZERO, dim("diameter", diameter)? / 2.0), holes: Vec::new() }),
            Profile::RectTube { width, height, wall, outer_radius } => {
                let (w, h, t) = (dim("width", width)?, dim("height", height)?, dim("wall", wall)?);
                less_half("wall", t, "the smaller side", w.min(h))?;
                let ro = radius("outer_radius", outer_radius.unwrap_or(0.0))?;
                if ro > w.min(h) / 2.0 {
                    return Err(invalid(format!("profile `outer_radius` ({ro}) must be at most half the smaller side ({})", w.min(h) / 2.0)));
                }
                let ri = (ro - t).max(0.0);
                Ok(Region2 { outer: rounded_rect(w, h, ro)?, holes: vec![rounded_rect(w - 2.0 * t, h - 2.0 * t, ri)?.reversed()] })
            }
            Profile::RoundTube { diameter, wall } => {
                let (d, t) = (dim("diameter", diameter)?, dim("wall", wall)?);
                less_half("wall", t, "the diameter", d)?;
                Ok(Region2 { outer: Loop2::circle(Vec2::ZERO, d / 2.0), holes: vec![Loop2::circle(Vec2::ZERO, d / 2.0 - t).reversed()] })
            }
            Profile::Angle { width, height, thickness, root_radius, toe_radius } => {
                let (a, b, t) = (dim("width", width)?, dim("height", height)?, dim("thickness", thickness)?);
                less("thickness", t, "the shorter leg", a.min(b))?;
                let r1 = radius("root_radius", root_radius.unwrap_or(0.0))?;
                let r2 = radius("toe_radius", toe_radius.unwrap_or(0.0))?;
                at_most("`toe_radius`", r2, t, "the thickness")?;
                at_most("`root_radius` + `toe_radius`", r1 + r2, a.min(b) - t, "the inside length of the shorter leg")?;
                let pts = [(0.0, 0.0), (a, 0.0), (a, t), (t, t), (t, b), (0.0, b)];
                let radii = [0.0, 0.0, r2, r1, r2, 0.0];
                Ok(Region2 { outer: rounded_polygon(&shifted(&pts, a, b), &radii)?, holes: Vec::new() })
            }
            Profile::Channel { height, width, web, flange, root_radius, toe_radius } => {
                let (h, b, tw, tf) = (dim("height", height)?, dim("width", width)?, dim("web", web)?, dim("flange", flange)?);
                less("web", tw, "width", b)?;
                less_half("flange", tf, "the height", h)?;
                let r1 = radius("root_radius", root_radius.unwrap_or(0.0))?;
                let r2 = radius("toe_radius", toe_radius.unwrap_or(0.0))?;
                at_most("`toe_radius`", r2, tf, "the flange thickness")?;
                at_most("`root_radius` + `toe_radius`", r1 + r2, b - tw, "the inside length of a flange")?;
                at_most("`root_radius`", r1, (h - 2.0 * tf) / 2.0, "half the web's inside height")?;
                let pts = [(0.0, 0.0), (b, 0.0), (b, tf), (tw, tf), (tw, h - tf), (b, h - tf), (b, h), (0.0, h)];
                let radii = [0.0, 0.0, r2, r1, r1, r2, 0.0, 0.0];
                Ok(Region2 { outer: rounded_polygon(&shifted(&pts, b, h), &radii)?, holes: Vec::new() })
            }
            Profile::IBeam { height, width, web, flange, root_radius } => {
                let (h, b, tw, tf) = (dim("height", height)?, dim("width", width)?, dim("web", web)?, dim("flange", flange)?);
                less("web", tw, "width", b)?;
                less_half("flange", tf, "the height", h)?;
                let r = radius("root_radius", root_radius.unwrap_or(0.0))?;
                at_most("`root_radius`", r, (b - tw) / 2.0, "the inside length of a flange")?;
                at_most("`root_radius`", r, (h - 2.0 * tf) / 2.0, "half the web's inside height")?;
                let (x, y, w, f) = (b / 2.0, h / 2.0, tw / 2.0, h / 2.0 - tf);
                let pts = [(-x, -y), (x, -y), (x, -f), (w, -f), (w, f), (x, f), (x, y), (-x, y), (-x, f), (-w, f), (-w, -f), (-x, -f)];
                let radii = [0.0, 0.0, 0.0, r, r, 0.0, 0.0, 0.0, 0.0, r, r, 0.0];
                let pts: Vec<Vec2> = pts.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
                Ok(Region2 { outer: rounded_polygon(&pts, &radii)?, holes: Vec::new() })
            }
        }
    }
}

/// Points given with the heel at the origin, moved so a `w` × `h` bounding box is centred on it.
fn shifted(pts: &[(f64, f64)], w: f64, h: f64) -> Vec<Vec2> {
    pts.iter().map(|&(x, y)| Vec2::new(x - w / 2.0, y - h / 2.0)).collect()
}

fn rounded_rect(w: f64, h: f64, r: f64) -> Result<Loop2> {
    let (x, y) = (w / 2.0, h / 2.0);
    rounded_polygon(&[Vec2::new(-x, -y), Vec2::new(x, -y), Vec2::new(x, y), Vec2::new(-x, y)], &[r; 4])
}

/// The counter-clockwise polygon `pts` with each corner rounded by its radius in `radii` (0: a
/// sharp corner). Convex and concave corners alike: the arc is tangent to both edges. An error
/// when the arcs at the two ends of an edge need more than its length.
pub fn rounded_polygon(pts: &[Vec2], radii: &[f64]) -> Result<Loop2> {
    let n = pts.len();
    if n < 3 || radii.len() != n {
        return Err(invalid("a profile polygon needs 3 or more corners, each with a radius"));
    }
    // Each corner as (where its arc starts, where it ends, the arc).
    let mut corners: Vec<(Vec2, Vec2, Option<Seg2>)> = Vec::with_capacity(n);
    for i in 0..n {
        let (Some(&prev), Some(&v), Some(&next), Some(&r)) = (pts.get((i + n - 1) % n), pts.get(i), pts.get((i + 1) % n), radii.get(i)) else {
            return Err(invalid("a profile polygon corner is missing"));
        };
        let (Some(d1), Some(d2)) = ((v - prev).normalized(), (next - v).normalized()) else {
            return Err(invalid("a profile polygon has two corners in one place"));
        };
        let turn = d1.cross(d2).atan2(d1.dot(d2));
        if r <= 0.0 || turn.abs() < 1e-12 {
            corners.push((v, v, None));
            continue;
        }
        if turn.abs() > std::f64::consts::PI - 1e-9 {
            return Err(invalid("a profile polygon turns back on itself"));
        }
        let l = r * (turn.abs() / 2.0).tan();
        let (t1, t2) = (v - d1 * l, v + d2 * l);
        let side = if turn > 0.0 { d1.perp() } else { d1.perp() * -1.0 };
        let center = t1 + side * r;
        corners.push((t1, t2, Some(Seg2::Arc { center, radius: r, start: (t1 - center).angle(), sweep: turn })));
    }
    let mut segs = Vec::with_capacity(2 * n);
    for i in 0..n {
        let (Some(&(_, out, arc)), Some(&(inn, _, _)), Some(&a), Some(&b)) =
            (corners.get(i), corners.get((i + 1) % n), pts.get(i), pts.get((i + 1) % n))
        else {
            return Err(invalid("a profile polygon corner is missing"));
        };
        if let Some(arc) = arc {
            segs.push(arc);
        }
        // The edge left between the two arcs must run the edge's way (or be empty).
        let along = (inn - out).dot(b - a);
        if along < -1e-9 * (1.0 + a.dist(b)) {
            return Err(invalid(format!("profile corner radii do not fit along an edge {:.4} mm long", a.dist(b))));
        }
        if out.dist(inn) > 1e-9 {
            segs.push(Seg2::Line { a: out, b: inn });
        }
    }
    Ok(Loop2 { segs })
}

/// Gauss–Legendre nodes and weights on [0, 1] (5 points).
const GL5: [(f64, f64); 5] = [
    (0.046_910_077_030_668, 0.118_463_442_528_094_5),
    (0.230_765_344_947_158_5, 0.239_314_335_249_683_2),
    (0.5, 0.284_444_444_444_444_4),
    (0.769_234_655_052_841_5, 0.239_314_335_249_683_2),
    (0.953_089_922_969_332, 0.118_463_442_528_094_5),
];

/// The boundary integrals of one loop, as it runs (counter-clockwise positive): area, ∫x dA,
/// ∫y dA, ∫x² dA, ∫y² dA, ∫xy dA, by Green's theorem.
fn loop_moments(l: &Loop2) -> [f64; 6] {
    let mut m = [0.0; 6];
    for seg in &l.segs {
        let pieces = match seg {
            Seg2::Line { .. } => 1,
            _ => 16,
        };
        for k in 0..pieces {
            let h = 1.0 / pieces as f64;
            for (x, wt) in GL5 {
                let t = (k as f64 + x) * h;
                let (p, dp) = (seg.point_at(t), seg.derivative(t));
                let w = wt * h;
                let terms = [
                    p.x * dp.y,
                    0.5 * p.x * p.x * dp.y,
                    -0.5 * p.y * p.y * dp.x,
                    p.x * p.x * p.x * dp.y / 3.0,
                    -p.y * p.y * p.y * dp.x / 3.0,
                    0.5 * p.x * p.x * p.y * dp.y,
                ];
                for (acc, v) in m.iter_mut().zip(terms) {
                    *acc += w * v;
                }
            }
        }
    }
    m
}

/// Points of `l` that include its extremes along x and y: every segment's ends, and where an arc
/// passes a quarter turn. Free-form segments add the polyline points within 1 µm.
fn extreme_points(l: &Loop2) -> Vec<Vec2> {
    let mut out = Vec::new();
    for seg in &l.segs {
        match *seg {
            Seg2::Line { a, b } => out.extend([a, b]),
            Seg2::Arc { center, radius, start, sweep } => {
                out.extend([seg.start(), seg.end()]);
                let (lo, hi) = if sweep >= 0.0 { (start, start + sweep) } else { (start + sweep, start) };
                let quarter = std::f64::consts::FRAC_PI_2;
                let mut k = (lo / quarter).ceil();
                // An arc turns at most once around: five quarter turns at most.
                for _ in 0..5 {
                    let t = k * quarter;
                    if t > hi {
                        break;
                    }
                    out.push(center + Vec2::from_angle(t) * radius);
                    k += 1.0;
                }
            }
            _ => out.extend(seg.polyline(1e-3)),
        }
    }
    out
}

/// The section properties of `region` (outer loop and holes, whichever way each runs).
pub fn section(region: &Region2) -> Section {
    let mut m = loop_moments(&region.outer.ccw());
    for hole in &region.holes {
        for (acc, v) in m.iter_mut().zip(loop_moments(&hole.ccw())) {
            *acc -= v;
        }
    }
    let [a, sx, sy, jyy, jxx, jxy] = m;
    let (cx, cy) = if a.abs() > 1e-300 { (sx / a, sy / a) } else { (0.0, 0.0) };
    let (mut lo, mut hi) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for p in extreme_points(&region.outer) {
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    let (width, height) = if lo.x <= hi.x { (hi.x - lo.x, hi.y - lo.y) } else { (0.0, 0.0) };
    Section { area: a, centroid: [cx, cy], ixx: jxx - a * cy * cy, iyy: jyy - a * cx * cx, ixy: jxy - a * cx * cy, width, height }
}

/// Where `anchor` is on the profile made of `regions`, in the profile's own coordinates.
pub fn anchor_point(regions: &[Region2], anchor: Anchor) -> Vec2 {
    let Some((lo, hi)) = profile_bounds(regions) else { return Vec2::ZERO };
    let mid = Vec2::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
    match anchor {
        Anchor::Origin => Vec2::ZERO,
        Anchor::Center => mid,
        Anchor::Centroid => {
            let (mut a, mut c) = (0.0, Vec2::ZERO);
            for r in regions {
                let s = section(r);
                a += s.area;
                c += Vec2::new(s.centroid[0], s.centroid[1]) * s.area;
            }
            if a > 1e-300 { c / a } else { mid }
        }
        Anchor::TopLeft => Vec2::new(lo.x, hi.y),
        Anchor::Top => Vec2::new(mid.x, hi.y),
        Anchor::TopRight => hi,
        Anchor::Left => Vec2::new(lo.x, mid.y),
        Anchor::Right => Vec2::new(hi.x, mid.y),
        Anchor::BottomLeft => lo,
        Anchor::Bottom => Vec2::new(mid.x, lo.y),
        Anchor::BottomRight => Vec2::new(hi.x, lo.y),
    }
}

/// `region` mirrored across its y axis (x → −x), its loops still running the way [`Region2`]
/// expects.
pub fn mirrored(region: &Region2) -> Region2 {
    let m = |p: Vec2| Vec2::new(-p.x, p.y);
    let seg = |g: &Seg2| match *g {
        Seg2::Line { a, b } => Seg2::Line { a: m(a), b: m(b) },
        Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center: m(center), radius, start: std::f64::consts::PI - start, sweep: -sweep },
        Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic { p0: m(p0), p1: m(p1), p2: m(p2), p3: m(p3) },
        Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: m(a), apex: m(apex), b: m(b), w },
        Seg2::Bezier { n, p } => Seg2::Bezier { n, p: p.map(m) },
    };
    let lp = |l: &Loop2| Loop2 { segs: l.segs.iter().map(seg).collect() }.reversed();
    Region2 { outer: lp(&region.outer), holes: region.holes.iter().map(lp).collect() }
}

/// The directions a member's profile x and y take in the world, for a member running along `d`
/// (any length): the profile's y points up (world +Z) as far as the line allows and x = y × d, so
/// x × y runs along the line; a vertical member's x is world +X (a line leaning less than
/// [`VERTICAL_TOLERANCE`] counts as vertical, so rounding never turns a post). `angle` (radians) then turns the
/// profile counter-clockwise about the line, seen looking back from its end. `None` for a
/// direction of no length.
pub fn member_axes(d: Vec3, angle: f64) -> Option<(Vec3, Vec3)> {
    let d = d.normalized()?;
    let up = Vec3::Z - d * Vec3::Z.dot(d);
    // `up` is as long as the sine of the line's lean from vertical.
    let (x, y) = match up.normalized().filter(|_| up.len() > VERTICAL_TOLERANCE) {
        Some(y) => (y.cross(d), y),
        None => (Vec3::X, d.cross(Vec3::X)),
    };
    let (c, s) = (angle.cos(), angle.sin());
    Some((x * c + y * s, y * c - x * s))
}

/// The bounding box (lowest and highest corner) of a profile's regions; `None` without any.
pub fn profile_bounds(regions: &[Region2]) -> Option<(Vec2, Vec2)> {
    let (mut lo, mut hi) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for p in regions.iter().flat_map(|r| extreme_points(&r.outer)) {
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    (lo.x <= hi.x && lo.y <= hi.y).then_some((lo, hi))
}

/// `region` turned by `angle` (radians, counter-clockwise) about its `anchor` point and moved so
/// that point lands on `at`.
pub fn placed(region: &Region2, anchor: Vec2, at: Vec2, angle: f64) -> Region2 {
    let (c, s) = (angle.cos(), angle.sin());
    let map = |p: Vec2| {
        let q = p - anchor;
        at + Vec2::new(c * q.x - s * q.y, s * q.x + c * q.y)
    };
    let seg = |g: &Seg2| match *g {
        Seg2::Line { a, b } => Seg2::Line { a: map(a), b: map(b) },
        Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center: map(center), radius, start: start + angle, sweep },
        Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic { p0: map(p0), p1: map(p1), p2: map(p2), p3: map(p3) },
        Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: map(a), apex: map(apex), b: map(b), w },
        Seg2::Bezier { n, p } => Seg2::Bezier { n, p: p.map(map) },
    };
    let lp = |l: &Loop2| Loop2 { segs: l.segs.iter().map(seg).collect() };
    Region2 { outer: lp(&region.outer), holes: region.holes.iter().map(lp).collect() }
}
