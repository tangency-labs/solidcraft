//! Sheet metal rules, and what the sheets made from surfaces share (#6748).
//!
//! A rule (thickness, K-factor, bend radius, reliefs and gaps) is kept as expressions that may use
//! the design's parameters, and evaluated to [`RuleValues`] for each sheet: a sheet's thickness
//! and radii are its rule's. Beside the rules are the records and limits every sheet made from
//! surfaces (`SurfaceSheet`) keeps or reads: where an edge flange's height is measured from
//! ([`HeightFrom`]), the bores, pockets and perforation fields of its flat, its bends as the bend
//! table lists them ([`BendInfo`]), the letters it carries ([`Letters`]), the work its flat may
//! take ([`FlatWork`]), the model's names of its free edges ([`sheet_edge_names`]), and what each
//! face of its body was made for (`face_names`). The `sheet:` face names designs saved before
//! keep ([`sheet_face_name`]) are read here too; no sheet names a face so now.

use serde::{Deserialize, Serialize};
use solidcore_record::{Loop2, Region2, Seg2, Vec2, Vec3};
use solidcraft_geom::Aabb2;
use solidcraft_geom::prelude::*;

use crate::expr::{self, Kind, Value};
use crate::{DocError, Document, Result};

#[path = "sheet_face_names.rs"]
mod face_names;
pub use crate::sheet_surface_convert::bend_edge_names;
pub use face_names::body_sheet_face_names;
pub(crate) use face_names::{FaceNamer, FaceRole, carried, ranked, rims};

/// The most work one sheet's flat may take (see [`FlatWork`]).
const MAX_FLAT_WORK: usize = 500_000_000;

/// Starts the warning a fold gives when its `fixed` point touches no material: a recompute keeps
/// the side the point lies on, and the `sheet.fold` command refuses such a point (#263).
pub const FOLD_FIXED_OFF_SHEET: &str = "the fold's fixed point is not on the sheet";

fn flat_budget_error() -> DocError {
    DocError::Invalid("sheet metal flat geometry exceeds the bounded processing budget".into())
}

/// The work left to a sheet's flat: each planar boolean its flat is cut and joined by charges what
/// it may cost, and a flat that would take more than [`MAX_FLAT_WORK`] returns an error rather than
/// running on. A limit on work, never on the geometry.
pub(crate) struct FlatWork(usize);

impl FlatWork {
    pub(crate) fn new() -> Self {
        Self(MAX_FLAT_WORK)
    }
    pub(crate) fn charge(&mut self, work: usize) -> Result<()> {
        self.0 = self.0.checked_sub(work).ok_or_else(flat_budget_error)?;
        Ok(())
    }
}

/// A sheet metal rule. Values are expressions; `Thickness` inside them is the rule's thickness.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetRule {
    pub name: String,
    pub thickness: String,
    pub k_factor: String,
    pub bend_radius: String,
    #[serde(default = "rule_relief_width")]
    pub relief_width: String,
    #[serde(default = "rule_relief_depth")]
    pub relief_depth: String,
    #[serde(default = "rule_corner_relief")]
    pub corner_relief: String,
    /// What is cut where two bends meet at a corner of a panel: `notch` (nothing more: the
    /// bends end square at the corner), `round` or `rectangular` (a relief `corner_relief`
    /// across, centered on the corner the two bends share), or `tear` (no relief: each bend's
    /// end is cut straight from that corner to where its flange's end starts).
    #[serde(default = "rule_corner_relief_shape")]
    pub corner_relief_shape: String,
    /// The space between a flat hem's leg and the face it folds over (its bend's inner radius is
    /// half of it).
    #[serde(default = "rule_hem_gap")]
    pub hem_gap: String,
    /// Gap between flanges that meet at a corner.
    #[serde(default = "rule_gap")]
    pub gap: String,
    /// How the bend angles of its parts read in the DXF bend notes: `bend` (how far the sheet
    /// turns from flat) or `included` (the angle left between the bend's two sides, as a press
    /// brake is set). A choice, not an expression; saved only when it isn't `bend`.
    #[serde(default = "rule_angle_display", skip_serializing_if = "is_bend_display")]
    pub angle_display: String,
}

impl SheetRule {
    /// The expressions, by field name (as commands name them), with the kind each evaluates to.
    /// (`corner_relief_shape` is a choice, not an expression.)
    pub fn fields(&self) -> [(&'static str, &String, Kind); 8] {
        [
            ("thickness", &self.thickness, Kind::Length),
            ("k_factor", &self.k_factor, Kind::Unitless),
            ("bend_radius", &self.bend_radius, Kind::Length),
            ("relief_width", &self.relief_width, Kind::Length),
            ("relief_depth", &self.relief_depth, Kind::Length),
            ("corner_relief", &self.corner_relief, Kind::Length),
            ("hem_gap", &self.hem_gap, Kind::Length),
            ("gap", &self.gap, Kind::Length),
        ]
    }

    /// The expressions to change, by field name, in the order of [`SheetRule::fields`].
    pub fn fields_mut(&mut self) -> [(&'static str, &mut String); 8] {
        [
            ("thickness", &mut self.thickness),
            ("k_factor", &mut self.k_factor),
            ("bend_radius", &mut self.bend_radius),
            ("relief_width", &mut self.relief_width),
            ("relief_depth", &mut self.relief_depth),
            ("corner_relief", &mut self.corner_relief),
            ("hem_gap", &mut self.hem_gap),
            ("gap", &mut self.gap),
        ]
    }
}

/// The design parameters a rule's `field` expression `e` reads: outside the rule's own
/// `thickness`, `Thickness` is the rule's thickness (sheet metal and plastic rules alike).
pub fn rule_references(field: &str, e: &str) -> Vec<String> {
    let mut names = expr::references(e);
    if field != "thickness" {
        names.retain(|n| n != "Thickness");
    }
    names
}

fn rule_gap() -> String {
    "Thickness".into()
}

fn rule_relief_width() -> String {
    "Thickness".into()
}
fn rule_relief_depth() -> String {
    "Thickness * 0.5".into()
}
fn rule_corner_relief() -> String {
    "Thickness * 4.0".into()
}
fn rule_corner_relief_shape() -> String {
    ReliefShape::Notch.name().into()
}
/// Why a bend radius of 0 is refused wherever it is given (a rule, a fold, a flange): every bend
/// keeps an inside radius. At 0 the inside fiber would close to nothing (all of its length
/// compressed away), no press brake or forming tool makes that, and the part could not be built
/// either: the bend's end wall would join a line on one face of the sheet to an arc on the other.
pub(crate) const KNIFE_EDGE: &str =
    "a bend radius of 0 can't be made: no material bends to a knife edge, so give the smallest inside radius your material and tooling form";

fn rule_hem_gap() -> String {
    "0.01 mm".into()
}
fn rule_angle_display() -> String {
    AngleDisplay::Bend.name().into()
}
/// A rule that reads bend angles as bend angles leaves the setting out of the design, which
/// then saves as it did before the setting existed.
fn is_bend_display(s: &str) -> bool {
    AngleDisplay::parse(s) == Some(AngleDisplay::Bend)
}

impl SheetRule {
    /// Default: Steel (mm), 2.5 mm, K 0.44, bend radius = Thickness.
    pub fn steel() -> SheetRule {
        SheetRule {
            name: "Steel (mm)".into(),
            thickness: "2.5 mm".into(),
            k_factor: "0.44".into(),
            bend_radius: "Thickness".into(),
            relief_width: rule_relief_width(),
            relief_depth: rule_relief_depth(),
            corner_relief: rule_corner_relief(),
            corner_relief_shape: rule_corner_relief_shape(),
            hem_gap: rule_hem_gap(),
            gap: rule_gap(),
            angle_display: rule_angle_display(),
        }
    }
}

/// The design's sheet metal rules and the one new sheet bodies use.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SheetSettings {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<SheetRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
}

impl SheetSettings {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty() && self.active.is_none()
    }
}

/// The shape of the relief cut where two bends meet at a corner of a panel (a rule's
/// `corner_relief_shape`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReliefShape {
    /// Nothing is cut: the bends end square at the corner.
    #[default]
    Notch,
    /// A round about the corner of the bends, through the angle the panel fills there.
    Round,
    /// A square with a corner on the corner of the bends (a kite where the panel's corner isn't
    /// square: both cuts leave their edge at a right angle).
    Rectangular,
    /// No relief: both bends run to their corner, and each one's end is cut straight, in the
    /// flat, from that corner to where its flange's closed end starts.
    Tear,
}

impl ReliefShape {
    pub fn parse(s: &str) -> Option<ReliefShape> {
        match s.trim().to_ascii_lowercase().as_str() {
            "notch" => Some(ReliefShape::Notch),
            "round" => Some(ReliefShape::Round),
            "rectangular" => Some(ReliefShape::Rectangular),
            "tear" => Some(ReliefShape::Tear),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ReliefShape::Notch => "notch",
            ReliefShape::Round => "round",
            ReliefShape::Rectangular => "rectangular",
            ReliefShape::Tear => "tear",
        }
    }
}

/// How a rule's bend angles read in the DXF bend notes (a rule's `angle_display`). The bend
/// table gives both angles, and a flange's `angle` is always the bend angle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AngleDisplay {
    /// How far the sheet turns from flat: a flange at 58° reads 58°.
    #[default]
    Bend,
    /// The angle left between the bend's two sides, 180° less the bend angle: a 58° bend reads
    /// 122°, a 90° bend 90°.
    Included,
}

impl AngleDisplay {
    pub fn parse(s: &str) -> Option<AngleDisplay> {
        match s.trim().to_ascii_lowercase().as_str() {
            "bend" => Some(AngleDisplay::Bend),
            "included" => Some(AngleDisplay::Included),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AngleDisplay::Bend => "bend",
            AngleDisplay::Included => "included",
        }
    }

    pub fn is_bend(&self) -> bool {
        *self == AngleDisplay::Bend
    }

    /// A bend of `bend_deg` degrees as this display reads it.
    pub fn of(self, bend_deg: f64) -> f64 {
        match self {
            AngleDisplay::Bend => bend_deg,
            AngleDisplay::Included => 180.0 - bend_deg,
        }
    }
}

/// A rule's values (mm, unit-less K).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct RuleValues {
    pub thickness: f64,
    pub k_factor: f64,
    pub bend_radius: f64,
    pub relief_width: f64,
    pub relief_depth: f64,
    pub corner_relief: f64,
    pub corner_relief_shape: ReliefShape,
    pub hem_gap: f64,
    pub gap: f64,
    pub angle_display: AngleDisplay,
}

/// A sheet rule's thickness is less than this (mm).
const MAX_SHEET_THICKNESS: f64 = 1e4;

/// The smallest gap closed corners are given: closer than this the flanges' ends would touch.
const MIN_CORNER_GAP: f64 = 0.01;

impl Document {
    /// A rule by name, the active one, or the default steel rule.
    pub fn sheet_rule(&self, name: Option<&str>) -> SheetRule {
        let want = name.or(self.sheet.active.as_deref());
        want.and_then(|n| self.sheet.rules.iter().find(|r| r.name == n))
            .or_else(|| self.sheet.rules.first())
            .cloned()
            .unwrap_or_else(SheetRule::steel)
    }

    /// Evaluate a rule: `Thickness` in its expressions is the rule's thickness, even where the
    /// design has a parameter of that name (#9623); only the rule's `thickness` itself reads it.
    pub fn rule_values(&self, vals: &std::collections::BTreeMap<String, Value>, r: &SheetRule) -> Result<RuleValues> {
        self.rule_values_at(vals, r, None)
    }

    /// Evaluate a rule for a sheet `thickness` thick (a converted body's own thickness), or the
    /// rule's thickness: `Thickness` in its expressions is the sheet's.
    pub fn rule_values_at(&self, vals: &std::collections::BTreeMap<String, Value>, r: &SheetRule, thickness: Option<f64>) -> Result<RuleValues> {
        let look = |n: &str| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`")));
        let t = match thickness {
            Some(t) => t,
            None => expr::eval_with(&r.thickness, &look)?.to_kind(Kind::Length)?,
        };
        if !(t > 0.0 && t < MAX_SHEET_THICKNESS) {
            return Err(DocError::Invalid("the sheet thickness must be more than 0 and less than 10000 mm".into()));
        }
        let with_t = |e: &str, k: Kind| -> Result<f64> {
            let f = |n: &str| if n == "Thickness" { Ok(Value::length(t)) } else { look(n) };
            expr::eval_with(e, &f)?.to_kind(k)
        };
        let v = RuleValues {
            thickness: t,
            k_factor: with_t(&r.k_factor, Kind::Unitless)?,
            bend_radius: with_t(&r.bend_radius, Kind::Length)?,
            relief_width: with_t(&r.relief_width, Kind::Length)?,
            relief_depth: with_t(&r.relief_depth, Kind::Length)?,
            corner_relief: with_t(&r.corner_relief, Kind::Length)?,
            corner_relief_shape: ReliefShape::parse(&r.corner_relief_shape).ok_or_else(|| {
                DocError::Invalid(format!("unknown corner relief shape `{}` (notch, round, rectangular or tear)", r.corner_relief_shape))
            })?,
            hem_gap: with_t(&r.hem_gap, Kind::Length)?,
            gap: with_t(&r.gap, Kind::Length)?,
            angle_display: AngleDisplay::parse(&r.angle_display)
                .ok_or_else(|| DocError::Invalid(format!("unknown bend angle display `{}` (bend or included)", r.angle_display)))?,
        };
        // Each value named on its own (#9625). A radius of 0 is refused (see [`KNIFE_EDGE`]).
        if !(0.0..=1.0).contains(&v.k_factor) {
            return Err(DocError::Invalid("the K-factor must be between 0 and 1".into()));
        }
        if !(v.bend_radius >= 0.0) {
            return Err(DocError::Invalid("the bend radius must not be negative".into()));
        }
        if v.bend_radius == 0.0 {
            return Err(DocError::Invalid(KNIFE_EDGE.into()));
        }
        // Flanges closed at a corner with no gap would touch, as no formed part's do.
        if !(v.gap >= MIN_CORNER_GAP) {
            return Err(DocError::Invalid(format!(
                "the gap must be at least {MIN_CORNER_GAP} mm: flanges closed at a corner with no gap would touch"
            )));
        }
        Ok(v)
    }
}

/// The whole of a picked edge: the span of an edge flange that isn't split into segments.
pub const WHOLE_EDGE: (f64, f64) = (0.0, 1.0);

/// The tolerance on a segment list's junctions. `sheet.flange` accepts a list whose stretches meet
/// within this, and its build snaps such a junction shut, so every list the command accepts tiles
/// the edge with no gap and no overlap: stretches that match in angle, height and side merge into
/// the bend the whole-edge flange builds, and stretches that differ build one bend each (#5927,
/// #5928). Both sides read this one number: a list can never pass validation and then stay split.
pub const SEGMENT_JOIN_EPS: f64 = 1e-9;

/// Where an edge flange's height is measured from (#5938). Every one measures along the flange,
/// up to its far edge; they differ in where the measure starts, so in how much of the height is
/// the bend rather than the leg (the flat panel past the bend). With bend angle θ, inner radius
/// R and thickness T, the leg is the height less [`HeightFrom::before_leg`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HeightFrom {
    /// From the outer virtual sharp, where the lines of the panel's and the flange's outer faces
    /// meet, along the outer face: (R + T)·tan(θ/2) of the height is before the leg.
    #[default]
    Outer,
    /// From the inner virtual sharp, where the lines of the two inner faces meet, along the inner
    /// face: R·tan(θ/2) of the height is before the leg.
    Inner,
    /// For bends past 90°: from the line square to the flange that touches the outside of the
    /// bend (at the point a quarter turn before the bend's end), R + T before the leg. Up to 90°
    /// it is the outer virtual sharp, (R + T)·tan(θ/2); at 90° the two are the same line (the
    /// panel's outer face), so the height does not jump as the angle passes 90°.
    Tangent,
}

impl HeightFrom {
    pub const NAMES: [&'static str; 3] = ["outer", "inner", "tangent"];

    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "outer" => Some(Self::Outer),
            "inner" => Some(Self::Inner),
            "tangent" => Some(Self::Tangent),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Outer => "outer",
            Self::Inner => "inner",
            Self::Tangent => "tangent",
        }
    }

    /// How much of the height lies before the leg starts, for a bend of `angle` (radians), inner
    /// `radius` and `thickness`: the smallest height that fits (a flange with no leg).
    pub fn before_leg(self, angle: f64, radius: f64, thickness: f64) -> f64 {
        match self {
            Self::Outer => (radius + thickness) * (angle / 2.0).tan(),
            Self::Inner => radius * (angle / 2.0).tan(),
            Self::Tangent if angle <= std::f64::consts::FRAC_PI_2 => (radius + thickness) * (angle / 2.0).tan(),
            Self::Tangent => radius + thickness,
        }
    }

    /// What the measure starts at, for messages.
    pub fn datum(self) -> &'static str {
        match self {
            Self::Outer => "the outer sharp",
            Self::Inner => "the inner sharp",
            Self::Tangent => "the bend's tangent",
        }
    }
}

/// The most lines a path flange's path may have (each one flange and its bend).
pub const MAX_PATH_LINES: usize = 64;

/// A straight round bore of panel `panel`, in that panel's chart (see `SurfaceSheet::bores`): a
/// cylinder from `base` along the unit `axis` for `height`, cut only from that panel's material.
/// A bore that names no panel has no place to be cut.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetBore {
    pub panel: Option<usize>,
    pub base: Vec3,
    pub axis: Vec3,
    pub radius: f64,
    pub height: f64,
}

/// A perforation field from a `sheet.perforate` feature (#732).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetPerforation {
    pub feature: u64,
    pub diameter: f64,
    pub pitch: f64,
    pub margin: f64,
    pub square: bool,
    /// The grid runs from the flat's right edge instead of its left: the field of a mirrored
    /// copy, whose holes are the mirror image of its source's.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub from_right: bool,
    /// The field's outline in the flat: holes only inside these regions, margin clear of their
    /// edges, on a grid that starts from their bounding box. None: the whole flat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<Vec<Region2>>,
}

/// A pocket in the flat: `region` recessed from `z0` to `z1` through the thickness, z 0 the
/// bottom and z t the top. A sheet made from surfaces is an oriented surface: the input surfaces
/// with their outward normals, continuous across every bend. Its top is the normal side,
/// everywhere, folded or flat; its bottom is the face one thickness into the material. A Convert
/// pick names which face is the surface, `flip` reverses it, and a sketch sheet's surface faces
/// along its sketch's normal. Nothing about a sheet's faces is a spatial z.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetPocket {
    pub region: Region2,
    pub z0: f64,
    pub z1: f64,
}

/// A bend for the bend table.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BendInfo {
    pub index: usize,
    pub angle_deg: f64,
    pub radius: f64,
    pub up: bool,
    pub allowance: f64,
    pub length: f64,
    /// Center line in flat coordinates, from `start` to `end`. A bend row runs along the bend with
    /// the material of the panel the bend was made on (the joint's `a` panel) on the left of its
    /// direction, as the flat is read.
    pub start: Vec2,
    pub end: Vec2,
}

impl BendInfo {
    /// The angle left between the bend's two sides (degrees): 180° less `angle_deg`.
    pub fn included_angle_deg(&self) -> f64 {
        AngleDisplay::Included.of(self.angle_deg)
    }
}

/// Whether every segment of `lp` lies on one of `curves` (its ends and its middle, within
/// 0.0001 mm): the loop is made of those curves, or of parts of them.
pub fn loop_lies_on(lp: &Loop2, curves: &[Seg2]) -> bool {
    const TOL: f64 = 1e-4;
    let near = |p: Vec2| curves.iter().any(|c| c.point_at(c.closest_param(p)).dist(p) <= TOL);
    !lp.segs.is_empty() && lp.segs.iter().all(|g| near(g.start()) && near(g.mid()) && near(g.end()))
}

/// Tells which loops of a sheet's flat pattern are letters: the text it carries (a sheet's
/// `labels`).
#[derive(Clone, Debug)]
pub struct Letters {
    /// Each text region's box and the segments of its outlines.
    regions: Vec<(Aabb2, Vec<Seg2>)>,
}

impl Letters {
    /// No text: a sheet that carries none.
    pub fn none() -> Letters {
        Letters { regions: Vec::new() }
    }

    /// The text regions `labels` (flat coordinates), each a letter with its counters.
    pub fn of(labels: &[Region2]) -> Letters {
        let regions = labels
            .iter()
            .map(|r| {
                (
                    r.outer.polyline(1e-3).bounds().inflated(0.01),
                    std::iter::once(&r.outer).chain(&r.holes).flat_map(|lp| lp.segs.iter().copied()).collect(),
                )
            })
            .collect();
        Letters { regions }
    }

    /// Whether `lp` is made of the outlines of text the sheet carries: a letter, or one of its
    /// counters.
    pub fn is_letter(&self, lp: &Loop2) -> bool {
        let Some(p) = lp.segs.first().map(Seg2::start) else { return false };
        self.regions.iter().any(|(b, curves)| b.contains(p) && loop_lies_on(lp, curves))
    }
}

/// The name a sketch on a flat face of a sheet kept in designs saved before:
/// `sheet:<body>:<panel>:<top|bottom>`. No sheet names a face so now (a sheet made from surfaces
/// reports what each face of its body was made for, and the model names the face, #7990), so such
/// a name finds no face: a sketch attached by one has lost its face.
pub fn sheet_face_name(body: &str, face: &str) -> String {
    format!("sheet:{body}:{face}")
}

/// Point the sheet face a sketch plane is attached to (also beneath an offset or a rotation)
/// at body `new`, when it is on body `old`.
pub fn rename_plane_sheet_body(plane: &mut crate::PlaneRef, old: &str, new: &str) {
    match plane {
        crate::PlaneRef::Face { name: Some(name), anchor: None, .. } => {
            let renamed = split_sheet_face_name(name).filter(|(body, _)| *body == old).map(|(_, face)| sheet_face_name(new, face));
            if let Some(renamed) = renamed {
                *name = renamed;
            }
        }
        crate::PlaneRef::Offset { base, .. }
        | crate::PlaneRef::ThroughPoint { base, .. }
        | crate::PlaneRef::AtAngle { base, .. }
        | crate::PlaneRef::Flipped { base, .. } => rename_plane_sheet_body(base, old, new),
        _ => {}
    }
}

/// The sheet face a sketch plane is attached to (also beneath an offset or a rotation): its
/// [`sheet_face_name`].
pub fn plane_sheet_face(plane: &crate::PlaneRef) -> Option<&str> {
    match plane {
        crate::PlaneRef::Face { name: Some(name), anchor: None, .. } if split_sheet_face_name(name).is_some() => Some(name),
        crate::PlaneRef::Offset { base, .. }
        | crate::PlaneRef::ThroughPoint { base, .. }
        | crate::PlaneRef::AtAngle { base, .. }
        | crate::PlaneRef::Flipped { base, .. } => plane_sheet_face(base),
        _ => None,
    }
}

/// The body and the `<panel>:<side>` of a [`sheet_face_name`].
pub fn split_sheet_face_name(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix("sheet:")?;
    let (head, side) = rest.rsplit_once(':')?;
    let (body, panel) = head.rsplit_once(':')?;
    (!body.is_empty() && !panel.is_empty() && !side.is_empty()).then_some(())?;
    Some((body, rest.get(body.len() + 1..)?))
}

/// Persistent names of the sheet edges at each world point (empty when none): on a sheet made
/// from surfaces (#6748), the model's name for the edge of its body at a free edge of the sheet
/// (`<face A>|<face B>`, #7990), as every feature's edges are named.
pub fn sheet_edge_names(st: &crate::ModelState, pts: &[Vec3]) -> Vec<String> {
    pts.iter()
        .map(|p| {
            st.surface_sheets
                .iter()
                .filter(|s| !s.unfolded)
                .find_map(|s| {
                    s.free_edge_at(*p).ok().flatten()?;
                    let b = st.body(&s.body)?;
                    crate::naming::edge_names_at(b, std::slice::from_ref(p)).into_iter().next().filter(|n| !n.is_empty())
                })
                .unwrap_or_default()
        })
        .collect()
}

/// Hole centers of a perforation field (#732) on a flat `regions`: a staggered (60°, odd rows
/// shifted half a pitch) or square grid from the flat's lower-left corner plus margin + radius; a
/// hole is kept where it is at least margin clear of the material's edges and cut-outs, and of
/// the regions in `avoid`. A field with an outline keeps its holes inside the outline and margin
/// clear of its edges too, on a grid from the lower-left corner of the outline's bounding box
/// (within the flat's).
pub fn perforation_grid(regions: &[Region2], avoid: &[Region2], p: &SheetPerforation) -> Vec<Vec2> {
    let zones = avoid;
    let clear = p.diameter / 2.0 + p.margin;
    let mut segs: Vec<(Vec2, Vec2)> = Vec::new();
    let mut add = |l: &Loop2| {
        let pts = l.polyline(1e-3);
        for (&a, &b) in pts.edges() {
            segs.push((a, b));
        }
    };
    for r in regions {
        add(&r.outer);
        r.holes.iter().for_each(&mut add);
    }
    zones.iter().for_each(|z| add(&z.outer));
    let Aabb2 { min: lo, max: hi } = segs.iter().map(|(a, _)| *a).collect();
    if !(lo.x <= hi.x) || !(p.pitch > 0.0) {
        return Vec::new();
    }
    // The rows and columns span the flat, or the outline's part of it; the bucket grid below stays the flat's.
    let mut field = Aabb2 { min: lo, max: hi };
    if let Some(outline) = &p.outline {
        let n = segs.len();
        for r in outline {
            for l in std::iter::once(&r.outer).chain(&r.holes) {
                segs.extend(l.polyline(1e-3).edges().map(|(&a, &b)| (a, b)));
            }
        }
        field = field.intersection(&segs.get(n..).unwrap_or(&[]).iter().map(|(a, _)| *a).collect::<Aabb2>());
        if !(field.min.x <= field.max.x && field.min.y <= field.max.y) {
            return Vec::new();
        }
    }
    // Segments bucketed in a grid, so each hole only checks the edges near it.
    let cell = (2.0 * clear).max(p.pitch).max(1.0);
    let (nx, ny) = (((hi.x - lo.x) / cell) as usize + 1, ((hi.y - lo.y) / cell) as usize + 1);
    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); nx.saturating_mul(ny)];
    let idx = |x: f64, y: f64| -> (usize, usize) {
        (((x - lo.x) / cell).floor().clamp(0.0, (nx - 1) as f64) as usize, ((y - lo.y) / cell).floor().clamp(0.0, (ny - 1) as f64) as usize)
    };
    for (i, (a, b)) in segs.iter().enumerate() {
        let (i0, j0) = idx(a.x.min(b.x) - clear, a.y.min(b.y) - clear);
        let (i1, j1) = idx(a.x.max(b.x) + clear, a.y.max(b.y) + clear);
        for j in j0..=j1 {
            for ii in i0..=i1 {
                if let Some(c) = grid.get_mut(j * nx + ii) {
                    c.push(i);
                }
            }
        }
    }
    let dist = |c: Vec2, (a, b): (Vec2, Vec2)| -> f64 {
        let (d, l) = (b - a, (b - a).dot(b - a));
        let t = if l > 0.0 { ((c - a).dot(d) / l).clamp(0.0, 1.0) } else { 0.0 };
        c.dist(a + d * t)
    };
    let dy = if p.square { p.pitch } else { p.pitch * 3f64.sqrt() / 2.0 };
    let mut out = Vec::new();
    let mut row = 0usize;
    let Aabb2 { min: flo, max: fhi } = field;
    loop {
        let y = flo.y + clear + row as f64 * dy;
        if y > fhi.y - clear + 1e-9 {
            break;
        }
        let shift = if !p.square && row % 2 == 1 { p.pitch / 2.0 } else { 0.0 };
        // Steps across the row from its first hole; a mirrored field counts from the right.
        let mut step = 0.0;
        while flo.x + clear + shift + step <= fhi.x - clear + 1e-9 {
            let x = if p.from_right { fhi.x - clear - shift - step } else { flo.x + clear + shift + step };
            let c = Vec2::new(x, y);
            let (i, j) = idx(x, y);
            let near = grid.get(j * nx + i).map(Vec::as_slice).unwrap_or(&[]);
            if regions.iter().any(|r| r.contains(c))
                && p.outline.as_ref().is_none_or(|o| o.iter().any(|r| r.contains(c)))
                && !zones.iter().any(|z| z.contains(c))
                && near.iter().all(|&k| segs.get(k).is_none_or(|&s| dist(c, s) >= clear - 1e-9))
            {
                out.push(c);
            }
            step += p.pitch;
        }
        row += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loops_lie_on_the_curves_they_are_made_of() {
        let circle = Loop2::circle(Vec2::new(5.0, 5.0), 3.0);
        let curves: Vec<Seg2> = circle.segs.clone();
        // The loop itself, started elsewhere and run the other way, and a loop of its halves.
        assert!(loop_lies_on(&circle, &curves));
        assert!(loop_lies_on(&Loop2::circle_from(Vec2::new(5.0, 5.0), 3.0, 1.0).reversed(), &curves));
        let halves: Vec<Seg2> = circle.segs.iter().flat_map(|g| [g.split_half().0, g.split_half().1]).collect();
        assert!(loop_lies_on(&Loop2 { segs: halves }, &curves));
        // Another circle, a square and an empty loop do not.
        assert!(!loop_lies_on(&Loop2::circle(Vec2::new(5.0, 5.0), 3.1), &curves));
        assert!(!loop_lies_on(&Loop2::circle(Vec2::new(5.0, 5.0), 2.0), &curves));
        assert!(!loop_lies_on(&Loop2::polygon(&[Vec2::ZERO, Vec2::X, Vec2::new(1.0, 1.0), Vec2::Y]), &curves));
        assert!(!loop_lies_on(&Loop2 { segs: Vec::new() }, &curves));
        // A loop only partly on them (a letter across the sheet's edge) is not on them.
        let notch = Loop2 { segs: vec![circle.segs[0], Seg2::Line { a: circle.segs[0].end(), b: Vec2::new(50.0, 50.0) }] };
        assert!(!loop_lies_on(&notch, &curves));
    }

    #[test]
    fn a_sheet_knows_the_letters_it_carries() {
        // A recorded text region with a counter: its outline and its counter are letters, the same
        // circle elsewhere, a loop near it that is another size and a loop far away are not.
        let counter = Loop2::circle(Vec2::new(20.0, 20.0), 2.0).reversed();
        let letters = Letters::of(&[Region2 { outer: Loop2::circle(Vec2::new(20.0, 20.0), 5.0), holes: vec![counter.clone()] }]);
        assert!(letters.is_letter(&Loop2::circle(Vec2::new(20.0, 20.0), 5.0)));
        assert!(letters.is_letter(&counter.reversed()));
        assert!(!letters.is_letter(&Loop2::circle(Vec2::new(20.0, 20.0), 4.0)));
        assert!(!letters.is_letter(&Loop2::circle(Vec2::new(70.0, 20.0), 5.0)));
        // A sheet that carries none has no letters.
        assert!(!Letters::none().is_letter(&Loop2::circle(Vec2::new(20.0, 20.0), 5.0)));
        assert!(!Letters::of(&[]).is_letter(&Loop2::circle(Vec2::new(20.0, 20.0), 5.0)));
    }

    #[test]
    fn sheet_face_names_split_from_the_right() {
        assert_eq!(split_sheet_face_name(&sheet_face_name("Body1", "F7.0:top")), Some(("Body1", "F7.0:top")));
        // A body name may hold colons; the panel and the side never do.
        assert_eq!(split_sheet_face_name("sheet:Part:2:base:bottom"), Some(("Part:2", "base:bottom")));
        for name in ["", "sheet", "sheet:Body1", "sheet:Body1:top", "sheet::base:top", "sheet:Body1::top", "sheet:Body1:base:", "Body1:base:top"] {
            assert_eq!(split_sheet_face_name(name), None, "{name}");
        }
    }

    #[test]
    fn the_rule_names_its_corner_relief_shape() {
        // A rule saved before the shape existed reads as the notch it always had.
        let old: SheetRule = serde_json::from_str(r#"{"name":"Old","thickness":"2 mm","k_factor":"0.5","bend_radius":"1 mm"}"#).unwrap();
        assert_eq!(old.corner_relief_shape, "notch");
        assert_eq!(SheetRule::steel().corner_relief_shape, "notch");
        let (doc, vals) = (Document::default(), std::collections::BTreeMap::new());
        assert_eq!(doc.rule_values(&vals, &old).unwrap().corner_relief_shape, ReliefShape::Notch);
        let shapes =
            [("round", ReliefShape::Round), (" Rectangular ", ReliefShape::Rectangular), ("NOTCH", ReliefShape::Notch), ("Tear", ReliefShape::Tear)];
        for (text, shape) in shapes {
            let rule = SheetRule { corner_relief_shape: text.into(), ..old.clone() };
            assert_eq!(doc.rule_values(&vals, &rule).unwrap().corner_relief_shape, shape);
            assert_eq!(ReliefShape::parse(shape.name()), Some(shape));
        }
        // No other name is a shape.
        for text in ["square", "circle", "rip", ""] {
            let err = doc.rule_values(&vals, &SheetRule { corner_relief_shape: text.into(), ..old.clone() }).unwrap_err().to_string();
            assert!(err.contains(&format!("unknown corner relief shape `{text}` (notch, round, rectangular or tear)")), "{err}");
        }
    }

    #[test]
    fn the_rule_names_how_its_bend_angles_read() {
        // A rule saved before the setting existed reads bend angles, and saves as it did.
        let old: SheetRule = serde_json::from_str(r#"{"name":"Old","thickness":"2 mm","k_factor":"0.5","bend_radius":"1 mm"}"#).unwrap();
        assert_eq!(old.angle_display, "bend");
        assert_eq!(SheetRule::steel().angle_display, "bend");
        assert!(serde_json::to_value(&old).unwrap().get("angle_display").is_none());
        let (doc, vals) = (Document::default(), std::collections::BTreeMap::new());
        assert_eq!(doc.rule_values(&vals, &old).unwrap().angle_display, AngleDisplay::Bend);
        for (text, display) in [("included", AngleDisplay::Included), (" Included ", AngleDisplay::Included), ("BEND", AngleDisplay::Bend)] {
            let rule = SheetRule { angle_display: text.into(), ..old.clone() };
            assert_eq!(doc.rule_values(&vals, &rule).unwrap().angle_display, display);
            assert_eq!(AngleDisplay::parse(display.name()), Some(display));
        }
        // An included rule is saved with the setting and read back the same.
        let included = SheetRule { angle_display: "included".into(), ..old.clone() };
        let saved = serde_json::to_value(&included).unwrap();
        assert_eq!(saved["angle_display"], "included");
        assert_eq!(serde_json::from_value::<SheetRule>(saved).unwrap(), included);
        // No other word is a display.
        for text in ["deflection", "inside", ""] {
            let err = doc.rule_values(&vals, &SheetRule { angle_display: text.into(), ..old.clone() }).unwrap_err().to_string();
            assert!(err.contains(&format!("unknown bend angle display `{text}` (bend or included)")), "{err}");
        }
    }

    /// The included angle is what a bend leaves between its sides: 180° less the bend angle, so a
    /// 90° bend reads the same both ways. The bend's other numbers do not enter.
    #[test]
    fn the_included_angle_is_180_less_the_bend_angle() {
        // 180 − 58 = 122, 180 − 90 = 90, 180 − 120 = 60.
        for (deg, included) in [(58.0, 122.0), (90.0, 90.0), (120.0, 60.0)] {
            let bend = BendInfo {
                index: 0,
                angle_deg: deg,
                radius: 2.5,
                up: true,
                allowance: 0.0,
                length: 100.0,
                start: Vec2::ZERO,
                end: Vec2::new(100.0, 0.0),
            };
            assert!((bend.included_angle_deg() - included).abs() < 1e-9, "{bend:?}");
            assert!((AngleDisplay::Bend.of(bend.angle_deg) - deg).abs() < 1e-9);
            assert!((AngleDisplay::Included.of(bend.angle_deg) - included).abs() < 1e-9);
        }
    }
}
