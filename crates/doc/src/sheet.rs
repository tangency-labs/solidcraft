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
use solidcraft_geom::prelude::*;
use solidcraft_geom::{Aabb2, Ring};

use crate::expr::{self, Kind, Value};
use crate::{DocError, Document, Result};

#[path = "sheet_face_names.rs"]
mod face_names;
pub use crate::sheet_surface_convert::bend_edge_names;
pub(crate) use face_names::{FOLD_ROLE, FaceNamer, FaceRole, carried, carried_from, fold_cap_roles, ranked};

/// The most work one sheet's flat may take (see [`FlatWork`]).
const MAX_FLAT_WORK: usize = 500_000_000;

/// A tab's sketch is not on a flat face of the sheet.
pub const TAB_OFF_SHEET: &str = "draw the tab on a flat face of a sheet metal body (either side), or in a plane level with one";
/// A tab's profile does not meet the face it is drawn on.
pub const TAB_APART: &str = "the tab must touch or overlap the outline of the face it is drawn on: draw it from an edge of that face out";
/// A tab reaches into a bend.
pub const TAB_IN_BEND: &str = "the tab reaches into a bend: keep it on the flat of its panel, clear of the bend zones";
/// A tab lands on other material of the sheet.
pub const TAB_OVERLAPS: &str = "the tab would overlap another panel of the sheet once folded or in the flat pattern";
/// A tab more than one panel (or sheet) would take.
pub const TAB_SEVERAL: &str = "not supported yet: a tab that would join more than one panel; draw it out from an edge of one panel only";
/// A tab lies inside the face already.
pub const TAB_ADDS_NOTHING: &str = "the tab adds no material: it lies inside the face already; draw it out past an edge of the face";
/// A tab fills where a cut took a bend away, out past where the bend's other side began.
pub const TAB_PAST_BEND: &str = "not supported yet: a tab that fills where a cut took a bend away must end before the far side of that bend; past it, the tab would run into the next panel in the flat pattern";
/// Part of what a tab fills where a cut took material away would still be cut away.
pub const TAB_CUT_AWAY: &str =
    "not supported yet: part of the tab would still be cut away in the flat pattern, by another cut or a relief; keep the tab clear of them";

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
    /// What is cut beside a bend's end where its panel runs on past it: `rectangular` (a slot
    /// `relief_width` wide, from `relief_depth` inside the panel's tangent line out), `obround`
    /// (that slot with a half round inner end) or `tear` (no material: the panel is torn from the
    /// bend along the bend's end). A choice, not an expression; saved only when it isn't
    /// `rectangular`.
    #[serde(default = "rule_bend_relief_shape", skip_serializing_if = "is_rectangular_bend_relief")]
    pub bend_relief_shape: String,
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
    /// The material the rule is for (`Aluminium 5052-H32`), as the standard library or a rule
    /// pack names it, or as typed. A label for the people reading the design: nothing is
    /// computed from it. Saved only when it is set (#18304).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub material: String,
    /// Where the rule's bends get their K-factor: `k_factor` (the rule's `k_factor`), or
    /// `deduction` or `allowance` (worked out from `unfold_value`, the 90° bend deduction or
    /// allowance a press brake chart gives for this thickness and bend radius). A choice, not an
    /// expression; saved only when it isn't `k_factor`.
    #[serde(default = "rule_unfold_method", skip_serializing_if = "is_k_factor_unfold")]
    pub unfold_method: String,
    /// The 90° bend deduction or allowance (a length expression), read only when `unfold_method`
    /// is `deduction` or `allowance`. Saved only when it isn't empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unfold_value: String,
}

impl SheetRule {
    /// The expressions, by field name (as commands name them), with the kind each evaluates to.
    /// (`corner_relief_shape`, `angle_display` and `unfold_method` are choices, not expressions.)
    /// `unfold_value` may be empty: only a `deduction` or `allowance` rule reads it.
    pub fn fields(&self) -> [(&'static str, &String, Kind); 9] {
        [
            ("thickness", &self.thickness, Kind::Length),
            ("k_factor", &self.k_factor, Kind::Unitless),
            ("bend_radius", &self.bend_radius, Kind::Length),
            ("relief_width", &self.relief_width, Kind::Length),
            ("relief_depth", &self.relief_depth, Kind::Length),
            ("corner_relief", &self.corner_relief, Kind::Length),
            ("hem_gap", &self.hem_gap, Kind::Length),
            ("gap", &self.gap, Kind::Length),
            ("unfold_value", &self.unfold_value, Kind::Length),
        ]
    }

    /// The expressions to change, by field name, in the order of [`SheetRule::fields`].
    pub fn fields_mut(&mut self) -> [(&'static str, &mut String); 9] {
        [
            ("thickness", &mut self.thickness),
            ("k_factor", &mut self.k_factor),
            ("bend_radius", &mut self.bend_radius),
            ("relief_width", &mut self.relief_width),
            ("relief_depth", &mut self.relief_depth),
            ("corner_relief", &mut self.corner_relief),
            ("hem_gap", &mut self.hem_gap),
            ("gap", &mut self.gap),
            ("unfold_value", &mut self.unfold_value),
        ]
    }
}

/// The design parameters a rule's `field` expression `e` reads: outside the rule's own
/// `thickness`, `Thickness` is the rule's thickness (sheet metal and plastic rules alike).
pub fn rule_references(field: &str, e: &str) -> Vec<String> {
    #[cfg(test)]
    crate::params::param_rows_tests::count_rule_read();
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
fn rule_bend_relief_shape() -> String {
    BendReliefShape::Rectangular.name().into()
}
/// A rule with the rectangular bend relief leaves the setting out of the design, which then
/// saves as it did before the setting existed.
fn is_rectangular_bend_relief(s: &str) -> bool {
    BendReliefShape::parse(s) == Some(BendReliefShape::Rectangular)
}
/// Why a bend radius of 0 is refused wherever it is given (a rule, a fold, a flange): every bend
/// keeps an inside radius. At 0 the inside fiber would close to nothing (all of its length
/// compressed away), no press brake or forming tool makes that, and the part could not be built
/// either: the bend's end wall would join a line on one face of the sheet to an arc on the other.
pub(crate) const KNIFE_EDGE: &str =
    "the bend radius must be more than 0: use your tooling's real inside radius, or a tiny one like 0.001 mm for a sharp look";

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
fn rule_unfold_method() -> String {
    UnfoldMethod::KFactor.name().into()
}
/// A rule that unfolds from its K-factor leaves the setting out of the design, which then saves
/// as it did before the setting existed.
fn is_k_factor_unfold(s: &str) -> bool {
    UnfoldMethod::parse(s) == Some(UnfoldMethod::KFactor)
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
            bend_relief_shape: rule_bend_relief_shape(),
            hem_gap: rule_hem_gap(),
            gap: rule_gap(),
            angle_display: rule_angle_display(),
            material: String::new(),
            unfold_method: rule_unfold_method(),
            unfold_value: String::new(),
        }
    }

    /// A rule named `name` with the default rule's reliefs, gaps and angle display, for a pack
    /// row or library entry that gives only some values.
    pub fn named(name: &str) -> SheetRule {
        SheetRule { name: name.into(), ..SheetRule::steel() }
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

/// The shape of the bend relief cut beside a bend's end where its panel runs on past it (a
/// rule's `bend_relief_shape`, or an edge flange's own).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BendReliefShape {
    /// A slot `relief_width` wide from `relief_depth` inside the panel's tangent line out past
    /// the flange.
    #[default]
    Rectangular,
    /// The same slot, its inner end a half round `relief_width` across: no sharp inside corner
    /// for a laser or a crack to start from.
    Obround,
    /// No material removed: the panel is torn from the bend along the bend's end as it folds.
    Tear,
}

impl BendReliefShape {
    pub const ALL: [BendReliefShape; 3] = [BendReliefShape::Rectangular, BendReliefShape::Obround, BendReliefShape::Tear];

    pub fn parse(s: &str) -> Option<BendReliefShape> {
        match s.trim().to_ascii_lowercase().as_str() {
            "rectangular" => Some(BendReliefShape::Rectangular),
            "obround" => Some(BendReliefShape::Obround),
            "tear" => Some(BendReliefShape::Tear),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            BendReliefShape::Rectangular => "rectangular",
            BendReliefShape::Obround => "obround",
            BendReliefShape::Tear => "tear",
        }
    }

    pub fn is_rectangular(&self) -> bool {
        *self == BendReliefShape::Rectangular
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

/// Where a rule's bends get their K-factor (a rule's `unfold_method`). Every bend still unfolds
/// from one K-factor: a deduction or allowance rule only works that K out from its 90° value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnfoldMethod {
    /// The rule's `k_factor`.
    #[default]
    KFactor,
    /// The 90° bend deduction: how much shorter the flat blank is than the two flange lengths
    /// measured to the outside of the bend (the outer sharp).
    Deduction,
    /// The 90° bend allowance: the flat length of the bend itself, along the neutral axis.
    Allowance,
}

impl UnfoldMethod {
    /// Reads `k_factor`, `deduction` or `allowance`, ignoring case and blanks around it, and the
    /// longer spellings `k-factor`, `k factor`, `kfactor`, `bend_deduction` and `bend_allowance`.
    pub fn parse(s: &str) -> Option<UnfoldMethod> {
        match s.trim().to_ascii_lowercase().replace([' ', '-'], "_").as_str() {
            "k_factor" | "kfactor" => Some(UnfoldMethod::KFactor),
            "deduction" | "bend_deduction" => Some(UnfoldMethod::Deduction),
            "allowance" | "bend_allowance" => Some(UnfoldMethod::Allowance),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            UnfoldMethod::KFactor => "k_factor",
            UnfoldMethod::Deduction => "deduction",
            UnfoldMethod::Allowance => "allowance",
        }
    }

    /// The K-factor a 90° bend deduction or allowance `value` gives for a sheet `thickness` thick
    /// bent at inner radius `radius` (all mm). `None` for [`UnfoldMethod::KFactor`], which has no
    /// such value.
    ///
    /// At 90° the bend allowance is BA = π/2 · (R + K·T), and the outside setback (from the outer
    /// sharp to where the bend starts) is (R + T)·tan 45° = R + T, so the bend deduction is
    /// BD = 2·(R + T) − BA. Hence K = (2·BA/π − R) / T.
    pub fn k_factor(self, value: f64, thickness: f64, radius: f64) -> Option<f64> {
        let allowance = match self {
            UnfoldMethod::KFactor => return None,
            UnfoldMethod::Allowance => value,
            UnfoldMethod::Deduction => 2.0 * (radius + thickness) - value,
        };
        Some((allowance / std::f64::consts::FRAC_PI_2 - radius) / thickness)
    }

    /// The 90° bend deduction or allowance (mm) that gives `k_factor` for a sheet `thickness`
    /// thick bent at inner radius `radius`: the inverse of [`UnfoldMethod::k_factor`]. `None` for
    /// [`UnfoldMethod::KFactor`].
    pub fn value(self, k_factor: f64, thickness: f64, radius: f64) -> Option<f64> {
        let allowance = std::f64::consts::FRAC_PI_2 * (radius + k_factor * thickness);
        match self {
            UnfoldMethod::KFactor => None,
            UnfoldMethod::Allowance => Some(allowance),
            UnfoldMethod::Deduction => Some(2.0 * (radius + thickness) - allowance),
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
    pub bend_relief_shape: BendReliefShape,
    pub hem_gap: f64,
    pub gap: f64,
    pub angle_display: AngleDisplay,
}

/// A sheet rule's thickness is less than this (mm).
const MAX_SHEET_THICKNESS: f64 = 1e4;

/// The smallest gap closed corners are given: closer than this the flanges' ends would touch.
pub(crate) const MIN_CORNER_GAP: f64 = 0.01;

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
        let method = UnfoldMethod::parse(&r.unfold_method)
            .ok_or_else(|| DocError::Invalid(format!("unknown unfold method `{}` (k_factor, deduction or allowance)", r.unfold_method)))?;
        let bend_radius = with_t(&r.bend_radius, Kind::Length)?;
        // Before the K-factor: a deduction or allowance rule reads the radius to work it out.
        if !(bend_radius >= 0.0) {
            return Err(DocError::Invalid("the bend radius must not be negative".into()));
        }
        let k_factor = match method {
            UnfoldMethod::KFactor => with_t(&r.k_factor, Kind::Unitless)?,
            UnfoldMethod::Deduction | UnfoldMethod::Allowance => {
                let what = if method == UnfoldMethod::Deduction { "deduction" } else { "allowance" };
                if r.unfold_value.trim().is_empty() {
                    return Err(DocError::Invalid(format!("a rule that unfolds by bend {what} needs its 90° bend {what} (`unfold_value`)")));
                }
                let value = with_t(&r.unfold_value, Kind::Length)?;
                let k = method.k_factor(value, t, bend_radius).unwrap_or(f64::NAN);
                // Said in the chart's terms: which K the value gave, and the range it must be in.
                if !(0.0..=1.0).contains(&k) {
                    let (lo, hi) = match method {
                        UnfoldMethod::Deduction => (
                            2.0 * (bend_radius + t) - std::f64::consts::FRAC_PI_2 * (bend_radius + t),
                            2.0 * (bend_radius + t) - std::f64::consts::FRAC_PI_2 * bend_radius,
                        ),
                        _ => (std::f64::consts::FRAC_PI_2 * bend_radius, std::f64::consts::FRAC_PI_2 * (bend_radius + t)),
                    };
                    // Lengths in the design's unit, rounded as a chart reads (4 decimals): an inch
                    // design reads inches, and float noise such as 1.5875000000000001 never shows.
                    let len = |mm: f64| crate::display_units::format_length(mm, &self.units, 4);
                    return Err(DocError::Invalid(format!(
                        "the 90° bend {what} {} gives a K-factor of {k:.4}, outside 0 to 1: for {} thick at bend radius {} it must be from {} to {}",
                        len(value),
                        len(t),
                        len(bend_radius),
                        len(lo),
                        len(hi)
                    )));
                }
                k
            }
        };
        let v = RuleValues {
            thickness: t,
            k_factor,
            bend_radius,
            relief_width: with_t(&r.relief_width, Kind::Length)?,
            relief_depth: with_t(&r.relief_depth, Kind::Length)?,
            corner_relief: with_t(&r.corner_relief, Kind::Length)?,
            corner_relief_shape: ReliefShape::parse(&r.corner_relief_shape).ok_or_else(|| {
                DocError::Invalid(format!("unknown corner relief shape `{}` (notch, round, rectangular or tear)", r.corner_relief_shape))
            })?,
            bend_relief_shape: BendReliefShape::parse(&r.bend_relief_shape)
                .ok_or_else(|| DocError::Invalid(format!("unknown bend relief shape `{}` (rectangular, obround or tear)", r.bend_relief_shape)))?,
            hem_gap: with_t(&r.hem_gap, Kind::Length)?,
            gap: with_t(&r.gap, Kind::Length)?,
            angle_display: AngleDisplay::parse(&r.angle_display)
                .ok_or_else(|| DocError::Invalid(format!("unknown bend angle display `{}` (bend or included)", r.angle_display)))?,
        };
        // Each value named on its own (#9625). A radius of 0 is refused (see [`KNIFE_EDGE`]).
        if !(0.0..=1.0).contains(&v.k_factor) {
            return Err(DocError::Invalid("the K-factor must be between 0 and 1".into()));
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetBore {
    pub panel: usize,
    pub base: Vec3,
    pub axis: Vec3,
    pub radius: f64,
    pub height: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<BoreSource>,
}

/// What drilled a bore and its role (e.g. `point:p2:bore`), when drilled at a sketch point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct BoreSource {
    pub feature: u64,
    pub role: String,
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

/// The mounting hole of press-in hardware (a self-clinching nut, stud or standoff) in the flat
/// (#17752): the hardware the hole is for, and the hole's circle, in the flat's own coordinates as
/// the sheet keeps them (`SurfaceSheet::shown` reads them as the flat pattern is read).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetHardware {
    /// The hole feature that cut it (or the pattern or mirror that copied it).
    pub feature: u64,
    pub kind: crate::hardware_catalog::HardwareKind,
    /// The thread as written (`M3`).
    pub thread: String,
    /// A stud's or standoff's length (mm); none for a nut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<f64>,
    /// The id of the part that stands for it (`nut.m3`).
    pub part: String,
    pub center: Vec2,
    /// The hole's radius (mm).
    pub radius: f64,
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

/// A [`Region2`] pre-tessellated at `1e-3` mm with per-loop bounding boxes (inflated by `1e-9`)
/// for fast repeated point-in-region queries identical to [`Region2Geom::contains`].
pub(crate) struct PolyRegion {
    outer: (Aabb2, Ring<Vec2>),
    holes: Vec<(Aabb2, Ring<Vec2>)>,
}

impl PolyRegion {
    pub(crate) fn from_region(r: &Region2) -> Self {
        let loop_ring = |l: &Loop2| {
            let ring = l.polyline(1e-3);
            let bounds = ring.iter().copied().collect::<Aabb2>().inflated(1e-9);
            (bounds, ring)
        };
        Self { outer: loop_ring(&r.outer), holes: r.holes.iter().map(loop_ring).collect() }
    }

    pub(crate) fn contains(&self, p: Vec2) -> bool {
        self.outer.0.contains(p) && self.outer.1.contains(p) && !self.holes.iter().any(|(b, h)| b.contains(p) && h.contains(p))
    }
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
    fn push_ring(segs: &mut Vec<(Vec2, Vec2)>, pts: &Ring<Vec2>) {
        for (&a, &b) in pts.edges() {
            segs.push((a, b));
        }
    }
    let poly_regions: Vec<PolyRegion> = regions
        .iter()
        .map(|r| {
            let pr = PolyRegion::from_region(r);
            push_ring(&mut segs, &pr.outer.1);
            for (_, h) in &pr.holes {
                push_ring(&mut segs, h);
            }
            pr
        })
        .collect();
    let poly_zones: Vec<PolyRegion> = zones
        .iter()
        .map(|z| {
            let pz = PolyRegion::from_region(z);
            push_ring(&mut segs, &pz.outer.1);
            pz
        })
        .collect();
    let Aabb2 { min: lo, max: hi } = segs.iter().map(|(a, _)| *a).collect();
    if !(lo.x <= hi.x) || !(p.pitch > 0.0) {
        return Vec::new();
    }
    // The rows and columns span the flat, or the outline's part of it; the bucket grid below stays the flat's.
    let mut field = Aabb2 { min: lo, max: hi };
    let poly_outline: Option<Vec<PolyRegion>> = if let Some(outline) = &p.outline {
        let n = segs.len();
        let out_polys: Vec<PolyRegion> = outline
            .iter()
            .map(|r| {
                let pr = PolyRegion::from_region(r);
                push_ring(&mut segs, &pr.outer.1);
                for (_, h) in &pr.holes {
                    push_ring(&mut segs, h);
                }
                pr
            })
            .collect();
        field = field.intersection(&segs.get(n..).unwrap_or(&[]).iter().map(|(a, _)| *a).collect::<Aabb2>());
        if !(field.min.x <= field.max.x && field.min.y <= field.max.y) {
            return Vec::new();
        }
        Some(out_polys)
    } else {
        None
    };
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
            if poly_regions.iter().any(|r| r.contains(c))
                && poly_outline.as_ref().is_none_or(|o| o.iter().any(|r| r.contains(c)))
                && !poly_zones.iter().any(|z| z.contains(c))
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
    fn a_rule_reads_its_bend_relief_shape_and_saves_it_only_when_it_is_not_rectangular() {
        let doc = Document::default();
        let (vals, _) = doc.param_values();
        let steel = SheetRule::steel();
        assert_eq!(doc.rule_values(&vals, &steel).unwrap().bend_relief_shape, BendReliefShape::Rectangular);
        assert!(serde_json::to_value(&steel).unwrap().get("bend_relief_shape").is_none());
        for (text, shape) in [(" Obround ", BendReliefShape::Obround), ("TEAR", BendReliefShape::Tear), ("rectangular", BendReliefShape::Rectangular)]
        {
            let rule = SheetRule { bend_relief_shape: text.into(), ..steel.clone() };
            assert_eq!(doc.rule_values(&vals, &rule).unwrap().bend_relief_shape, shape, "{text}");
        }
        let obround = SheetRule { bend_relief_shape: "obround".into(), ..steel.clone() };
        let saved = serde_json::to_value(&obround).unwrap();
        assert_eq!(saved["bend_relief_shape"], "obround");
        let back: SheetRule = serde_json::from_value(saved).unwrap();
        assert_eq!(back, obround);
        let err = doc.rule_values(&vals, &SheetRule { bend_relief_shape: "round".into(), ..steel }).unwrap_err().to_string();
        assert!(err.contains("rectangular, obround or tear"), "{err}");
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

    #[test]
    fn the_rule_unfolds_by_k_factor_bend_deduction_or_bend_allowance() {
        // A rule saved before the setting existed unfolds from its K-factor, and saves as it did.
        let old: SheetRule = serde_json::from_str(r#"{"name":"Old","thickness":"2 mm","k_factor":"0.44","bend_radius":"2 mm"}"#).unwrap();
        assert_eq!((old.unfold_method.as_str(), old.unfold_value.as_str()), ("k_factor", ""));
        assert_eq!(SheetRule::steel().unfold_method, "k_factor");
        let saved = serde_json::to_value(&old).unwrap();
        assert!(saved.get("unfold_method").is_none() && saved.get("unfold_value").is_none(), "{saved}");
        let (doc, vals) = (Document::default(), std::collections::BTreeMap::new());
        let k = |rule: &SheetRule| doc.rule_values(&vals, rule).map(|v| v.k_factor);
        assert_eq!(k(&old).unwrap(), 0.44);
        // Hand-worked for T 2, R 2, K 0.44: BA90 = π/2·(R + K·T) = π/2·2.88 = 4.523893421169302,
        // and BD90 = 2·(R + T) − BA90 = 8 − BA90 = 3.4761065788306977. Each gives back K 0.44.
        let by = |method: &str, value: &str| SheetRule { unfold_method: method.into(), unfold_value: value.into(), ..old.clone() };
        for (method, value) in
            [("allowance", "4.523893421169302 mm"), ("deduction", "3.4761065788306977 mm"), (" Bend_Deduction ", "3.4761065788306977")]
        {
            let got = k(&by(method, value)).unwrap();
            assert!((got - 0.44).abs() < 1e-12, "{method} {value}: K {got}");
        }
        // A chart's round value: BD90 3.5 mm makes BA90 4.5 mm, K = (4.5/(π/2) − 2)/2.
        let chart = by("deduction", "Thickness * 1.75");
        assert!((k(&chart).unwrap() - 0.432394487827058).abs() < 1e-12);
        // A deduction rule ignores its K-factor expression, which may even no longer evaluate.
        assert!((k(&SheetRule { k_factor: "Missing".into(), ..chart.clone() }).unwrap() - 0.432394487827058).abs() < 1e-12);
        // The setting and value are saved, and read back the same.
        let saved = serde_json::to_value(&chart).unwrap();
        assert_eq!((saved["unfold_method"].as_str(), saved["unfold_value"].as_str()), (Some("deduction"), Some("Thickness * 1.75")));
        assert_eq!(serde_json::from_value::<SheetRule>(saved).unwrap(), chart);
        for method in [UnfoldMethod::KFactor, UnfoldMethod::Deduction, UnfoldMethod::Allowance] {
            assert_eq!(UnfoldMethod::parse(method.name()), Some(method));
        }
        assert_eq!(UnfoldMethod::KFactor.k_factor(1.0, 2.0, 2.0), None);
        // `value` is the inverse: K 0.44 gives back the chart values above, and those give K 0.44.
        assert_eq!(UnfoldMethod::KFactor.value(0.44, 2.0, 2.0), None);
        for (method, want) in [(UnfoldMethod::Allowance, 4.523893421169302), (UnfoldMethod::Deduction, 3.4761065788306977)] {
            let value = method.value(0.44, 2.0, 2.0).unwrap();
            assert!((value - want).abs() < 1e-12, "{method:?}: {value}");
            assert!((method.k_factor(value, 2.0, 2.0).unwrap() - 0.44).abs() < 1e-12, "{method:?}");
        }
        // A value whose K falls outside 0…1 is refused, naming the range the chart value must be in:
        // BA90 from π/2·R = 3.1416 to π/2·(R + T) = 6.2832 mm, BD90 from 8 − 6.2832 to 8 − 3.1416.
        // Lengths read in the design's unit with 4 decimals, as the shared display formatter gives them.
        let err = k(&by("allowance", "10 mm")).unwrap_err().to_string();
        assert!(
            err.contains(
                "the 90° bend allowance 10.0000 mm gives a K-factor of 2.1831, outside 0 to 1: for 2.0000 mm thick at bend radius 2.0000 mm"
            ) && err.contains("it must be from 3.1416 mm to 6.2832 mm"),
            "{err}"
        );
        let err = k(&by("deduction", "1 mm")).unwrap_err().to_string();
        assert!(err.contains("the 90° bend deduction 1.0000 mm gives a K-factor of") && err.contains("from 1.7168 mm to 4.8584 mm"), "{err}");
        // An inch design reads inches: 1/16 in sheet and radius (1.5875 mm, no float noise shown), BA90
        // 0.5 in gives K 4.0930, and BA90 must be from π/2·R = 0.0982 in to π/2·(R + T) = 0.1963 in.
        let inch = Document { units: "in".into(), ..Document::default() };
        let sixteenth = SheetRule { thickness: "0.0625 in".into(), bend_radius: "Thickness".into(), ..by("allowance", "0.5 in") };
        let err = inch.rule_values(&vals, &sixteenth).unwrap_err().to_string();
        assert!(
            err.contains("the 90° bend allowance 0.5000 in gives a K-factor of 4.0930, outside 0 to 1: for 0.0625 in thick at bend radius 0.0625 in")
                && err.contains("it must be from 0.0982 in to 0.1963 in"),
            "{err}"
        );
        // Without a value, or with a method no one knows, the rule says what is missing.
        let err = k(&by("deduction", " ")).unwrap_err().to_string();
        assert!(err.contains("needs its 90° bend deduction (`unfold_value`)"), "{err}");
        for text in ["neutral", "y_factor", ""] {
            let err = k(&by(text, "3 mm")).unwrap_err().to_string();
            assert!(err.contains(&format!("unknown unfold method `{text}` (k_factor, deduction or allowance)")), "{err}");
        }
        // Hostile values never panic: they are refused or give a K in range.
        for value in ["0 mm", "-5 mm", "1e308 mm", "1e-308 mm", "(1/0) * 1 mm"] {
            for method in ["deduction", "allowance"] {
                if let Ok(got) = k(&by(method, value)) {
                    assert!((0.0..=1.0).contains(&got), "{method} {value}: {got}");
                }
            }
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

    #[test]
    fn perforation_grid_matches_uncached_region_contains_bit_for_bit() {
        fn ref_perforation_grid(regions: &[Region2], avoid: &[Region2], p: &SheetPerforation) -> Vec<Vec2> {
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

        let rect = |x0: f64, y0: f64, x1: f64, y1: f64| Loop2::polygon(&[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]);
        let panel = Region2 {
            outer: rect(0.0, 0.0, 120.0, 80.0),
            holes: vec![
                Loop2::circle(Vec2::new(25.0, 25.0), 8.0).reversed(),
                Loop2::circle(Vec2::new(60.0, 40.0), 10.0).reversed(),
                Loop2::circle(Vec2::new(95.0, 55.0), 7.0).reversed(),
                rect(85.0, 12.0, 105.0, 28.0).reversed(),
            ],
        };
        let avoid = vec![
            Region2 { outer: rect(0.0, 36.0, 120.0, 44.0), holes: Vec::new() },
            Region2 { outer: rect(45.0, 0.0, 52.0, 80.0), holes: vec![Loop2::circle(Vec2::new(48.5, 65.0), 1.5).reversed()] },
        ];
        let outline = vec![Region2 { outer: rect(8.0, 8.0, 112.0, 72.0), holes: vec![Loop2::circle(Vec2::new(30.0, 60.0), 6.0).reversed()] }];

        for (square, from_right, with_outline) in [(false, false, false), (true, false, false), (false, true, true), (true, true, true)] {
            let perf = SheetPerforation {
                feature: 1,
                diameter: 3.0,
                pitch: 6.0,
                margin: 2.0,
                square,
                from_right,
                outline: with_outline.then(|| outline.clone()),
            };
            let got = perforation_grid(std::slice::from_ref(&panel), &avoid, &perf);
            let want = ref_perforation_grid(std::slice::from_ref(&panel), &avoid, &perf);
            assert!(!got.is_empty(), "expected non-empty perforation grid");
            assert_eq!(got, want, "square={square} from_right={from_right} with_outline={with_outline}");
        }
    }
}
