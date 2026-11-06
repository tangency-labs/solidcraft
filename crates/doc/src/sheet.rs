//! Sheet metal: rules (thickness, K-factor, bend radius, reliefs as expressions that may use
//! parameters), and the sheet body model behind every sheet-metal feature.
//!
//! A sheet body is kept as its flat pattern: the base panel, flanges hanging off panel edges
//! (each a bend zone of width BA = θ·(R + K·T) followed by a flat panel), and cut-outs, all in
//! the flat frame (the base panel's bottom face; thickness grows along its normal). Folding maps
//! each flange's flat coordinates through its parents' bends: past a bend line, a point loses
//! the bend zone's width and turns by θ about the bend axis (R from the inner face). The folded
//! solid is built from extruded pieces (panels, and bend zones as annular sectors) joined
//! together; the flat solid is the outline extruded through the thickness.

use serde::{Deserialize, Serialize};
use solvecraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solvecraft_kernel::{self as kernel, Body, EdgeSpec, FaceSpec, SurfSpec};

use crate::expr::{self, Kind, Value};
use crate::{DocError, Document, IDENTITY, Mat, Result, apply_point, apply_vector, mat_inverse, mat_mul};

// Bound the aggregate geometry, including sampled curves and intermediate clipping pieces.
// These are work limits, never geometry truncation: a larger input returns a useful error.
const MAX_FLAT_POINTS: usize = 16_384;
const MAX_FLAT_PIECES: usize = 4096;
const MAX_FLAT_WORK: usize = 500_000_000;

fn flat_budget_error() -> DocError {
    DocError::Invalid("sheet metal flat geometry exceeds the bounded processing budget".into())
}

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

fn flat_polyline(lp: &Loop2) -> Result<Vec<Vec2>> {
    if lp.segs.len() > MAX_FLAT_POINTS {
        return Err(flat_budget_error());
    }
    let mut points = Vec::new();
    for seg in &lp.segs {
        let p = seg.polyline(1e-3);
        let add = p.len().saturating_sub(1);
        if points.len().saturating_add(add) > MAX_FLAT_POINTS {
            return Err(flat_budget_error());
        }
        points.extend(p.iter().take(add));
    }
    Ok(points)
}

fn flat_points(polys: &[Vec<Vec2>]) -> Result<usize> {
    let n = polys.iter().try_fold(0usize, |n, p| n.checked_add(p.len()).ok_or_else(flat_budget_error))?;
    if n > MAX_FLAT_POINTS || polys.len() > MAX_FLAT_PIECES {
        return Err(flat_budget_error());
    }
    Ok(n)
}

fn planar_region(region: &Region2) -> Result<Region2> {
    if region.holes.len() > MAX_FLAT_PIECES {
        return Err(flat_budget_error());
    }
    region_complexity(std::slice::from_ref(region))?;
    let convert = |lp: &Loop2| -> Result<Loop2> {
        if lp.segs.iter().all(|s| matches!(s, Seg2::Line { .. } | Seg2::Arc { .. })) {
            flat_polyline(lp)?;
            Ok(lp.clone())
        } else {
            // Keep free-form profiles through controlled 0.001 mm chord sampling.
            Ok(Loop2::polygon(&flat_polyline(lp)?))
        }
    };
    Ok(Region2 {
        outer: convert(&region.outer)?.ccw(),
        holes: region.holes.iter().map(|h| convert(h).map(|l| l.ccw().reversed())).collect::<Result<_>>()?,
    })
}

fn region_complexity(regions: &[Region2]) -> Result<(usize, usize)> {
    regions_complexity(regions.iter())
}

fn regions_complexity<'a>(regions: impl Iterator<Item = &'a Region2>) -> Result<(usize, usize)> {
    let (mut edges, mut points) = (0usize, 0usize);
    let mut pieces = 0usize;
    for region in regions {
        pieces += 1;
        if pieces > MAX_FLAT_PIECES || region.holes.len() > MAX_FLAT_PIECES {
            return Err(flat_budget_error());
        }
        for lp in std::iter::once(&region.outer).chain(&region.holes) {
            edges = edges.saturating_add(lp.segs.len());
            if edges > MAX_FLAT_POINTS {
                return Err(flat_budget_error());
            }
            points = points.saturating_add(flat_polyline(lp)?.len());
            if points > MAX_FLAT_POINTS {
                return Err(flat_budget_error());
            }
        }
    }
    Ok((edges, points))
}

pub(crate) fn material_boolean(a: &[Region2], b: &[Region2], op: kernel::BoolOp, work: &mut FlatWork) -> Result<Vec<Region2>> {
    let (ae, ap) = region_complexity(a)?;
    let (be, bp) = region_complexity(b)?;
    let edges = ae.saturating_add(be);
    work.charge(edges.saturating_mul(edges).saturating_add(edges.saturating_mul(ap.saturating_add(bp))).saturating_mul(8))?;
    let regions = kernel::planar_regions_boolean(a, b, op, 1e-7)?;
    region_complexity(&regions)?;
    Ok(regions)
}

// Merge in balanced rounds so hundreds of disjoint holes or adjoining panels do not
// repeatedly arrange a growing accumulated boundary against just one small region.
pub(crate) fn material_union(regions: Vec<Region2>, work: &mut FlatWork) -> Result<Vec<Region2>> {
    region_complexity(&regions)?;
    let mut groups: Vec<Vec<Region2>> = regions.into_iter().map(|r| vec![r]).collect();
    while groups.len() > 1 {
        let mut next = Vec::new();
        let mut iter = groups.into_iter();
        while let Some(a) = iter.next() {
            match iter.next() {
                Some(b) => next.push(material_boolean(&a, &b, kernel::BoolOp::Union, work)?),
                None => next.push(a),
            }
        }
        groups = next;
    }
    Ok(groups.pop().unwrap_or_default())
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
    #[serde(default = "rule_corner_relief_shape")]
    pub corner_relief_shape: String,
    /// Inner radius of flat hems.
    #[serde(default = "rule_hem_gap")]
    pub hem_gap: String,
    /// Gap between flanges that meet at a corner.
    #[serde(default = "rule_gap")]
    pub gap: String,
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
    "square".into()
}
fn rule_hem_gap() -> String {
    "0.01 mm".into()
}

impl SheetRule {
    /// Fusion's default: Steel (mm), 2.5 mm, K 0.44, bend radius = Thickness.
    pub fn steel() -> SheetRule {
        SheetRule {
            name: "Steel (mm)".into(),
            thickness: "2.5 mm".into(),
            k_factor: "0.44".into(),
            bend_radius: "Thickness".into(),
            relief_width: rule_relief_width(),
            relief_depth: rule_relief_depth(),
            corner_relief: rule_corner_relief(),
            corner_relief_shape: "laser_weld".into(),
            hem_gap: rule_hem_gap(),
            gap: rule_gap(),
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

/// A rule's values (mm, unit-less K).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct RuleValues {
    pub thickness: f64,
    pub k_factor: f64,
    pub bend_radius: f64,
    pub relief_width: f64,
    pub relief_depth: f64,
    pub corner_relief: f64,
    pub hem_gap: f64,
    pub gap: f64,
}

impl Document {
    /// A rule by name, the active one, or the default steel rule.
    pub fn sheet_rule(&self, name: Option<&str>) -> SheetRule {
        let want = name.or(self.sheet.active.as_deref());
        want.and_then(|n| self.sheet.rules.iter().find(|r| r.name == n))
            .or_else(|| self.sheet.rules.first())
            .cloned()
            .unwrap_or_else(SheetRule::steel)
    }

    /// Evaluate a rule: `Thickness` in its expressions is the rule's thickness.
    pub fn rule_values(&self, vals: &std::collections::BTreeMap<String, Value>, r: &SheetRule) -> Result<RuleValues> {
        let look = |n: &str| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`")));
        let t = expr::eval_with(&r.thickness, &look)?.to_kind(Kind::Length)?;
        if !(t > 0.0 && t < 1e4) {
            return Err(DocError::Invalid("the sheet thickness must be positive".into()));
        }
        let with_t = |e: &str, k: Kind| -> Result<f64> {
            let f = |n: &str| if n == "Thickness" && !vals.contains_key("Thickness") { Ok(Value::length(t)) } else { look(n) };
            expr::eval_with(e, &f)?.to_kind(k)
        };
        let v = RuleValues {
            thickness: t,
            k_factor: with_t(&r.k_factor, Kind::Unitless)?,
            bend_radius: with_t(&r.bend_radius, Kind::Length)?,
            relief_width: with_t(&r.relief_width, Kind::Length)?,
            relief_depth: with_t(&r.relief_depth, Kind::Length)?,
            corner_relief: with_t(&r.corner_relief, Kind::Length)?,
            hem_gap: with_t(&r.hem_gap, Kind::Length)?,
            gap: with_t(&r.gap, Kind::Length)?,
        };
        if !(0.0..=1.0).contains(&v.k_factor) || v.bend_radius < 0.0 {
            return Err(DocError::Invalid("the K-factor must be 0…1 and the bend radius positive".into()));
        }
        Ok(v)
    }
}

/// A flange: a bend along a panel edge and the flat panel past it (flat coordinates).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SheetFlange {
    /// The flange it hangs off (None = the base panel).
    pub parent: Option<usize>,
    /// Bend start line: from `p` along `d` for `len`; `n` points into the flange.
    pub p: Vec2,
    pub d: Vec2,
    pub len: f64,
    pub n: Vec2,
    /// Bend angle (radians; positive bends toward the top face, +z of the flat frame).
    pub angle: f64,
    /// Inner bend radius.
    pub radius: f64,
    /// Flat length of the panel past the bend zone.
    pub leg: f64,
    /// The panel runs on past the bend's ends by these (toward a neighbouring flange).
    #[serde(default)]
    pub ext: (f64, f64),
    /// A fold's panel: the part of the sheet past the bend (flat coordinates, counter-clockwise)
    /// instead of a rectangle `leg` long.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Vec<Vec2>>,
    /// Which feature made it: `F<feature>.<k>` (persistent edge names).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tag: String,
}

/// A sheet metal body (see the module docs).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SheetBody {
    pub body: String,
    pub rule: String,
    pub t: f64,
    pub k: f64,
    /// The flat frame: base panel bottom face; z = thickness direction.
    pub frame: Plane,
    pub base: Region2,
    pub flanges: Vec<SheetFlange>,
    /// Cut-outs (flat coordinates) through the sheet.
    pub holes: Vec<Region2>,
    /// Planar machining in flat coordinates, with depths measured from the flat bottom.
    /// Derived from feature history; these records are not a document schema change.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pockets: Vec<SheetPocket>,
    /// Unfolded.
    pub flat: bool,
    /// Gap between flanges that meet at a corner.
    pub gap: f64,
    /// Depth of the corner relief.
    pub corner_relief: f64,
    /// Shape of the corner relief: "square", "round" or "laser_weld".
    pub corner_relief_shape: String,
    /// The base sketch's lines (id, ends in flat coordinates), naming the base panel's edges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub base_curves: Vec<(String, Vec2, Vec2)>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SheetPocket {
    pub region: Region2,
    pub z0: f64,
    pub z1: f64,
}

/// A piece of the sheet: a panel (None = the base) or a flange's bend zone.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Piece {
    Panel(Option<usize>),
    Zone(usize),
}

/// A flat piece of the sheet (counter-clockwise outer loop, clockwise holes).
#[derive(Clone, Debug)]
struct Cell {
    piece: Piece,
    outer: Vec<Seg2>,
    holes: Vec<Vec<Seg2>>,
}

fn rev_edge(e: EdgeSpec) -> EdgeSpec {
    match e {
        EdgeSpec::Line { a, b } => EdgeSpec::Line { a: b, b: a },
        EdgeSpec::Arc { a, b, mid } => EdgeSpec::Arc { a: b, b: a, mid },
    }
}

fn dedup(p: &[Vec2]) -> Vec<Vec2> {
    let mut v: Vec<Vec2> = Vec::new();
    for q in p {
        if v.last().is_none_or(|l| l.dist(*q) > 1e-9) {
            v.push(*q);
        }
    }
    if v.len() > 1 && v.first().zip(v.last()).is_some_and(|(a, b)| a.dist(*b) < 1e-9) {
        v.pop();
    }
    v
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
    /// Centre line in flat coordinates.
    pub start: Vec2,
    pub end: Vec2,
}

fn rot_about(o: Vec3, k: Vec3, a: f64) -> Mat {
    crate::rigid(Vec3::ZERO, o, k, a)
}

fn translate(t: Vec3) -> Mat {
    let mut m = IDENTITY;
    m[3] = [t.x, t.y, t.z, 1.0];
    m
}

fn v3(v: Vec2) -> Vec3 {
    Vec3::new(v.x, v.y, 0.0)
}

fn plane_of(m: &Mat, o: Vec3, x: Vec3, y: Vec3) -> Result<Plane> {
    Plane::new(apply_point(m, o), apply_vector(m, x), apply_vector(m, y)).ok_or_else(|| DocError::Invalid("sheet: degenerate plane".into()))
}

fn rect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Region2 {
    let mut lp = Loop2::polygon(&[a, b, c, d]);
    if lp.signed_area() < 0.0 {
        lp = lp.reversed();
    }
    Region2 { outer: lp, holes: Vec::new() }
}

/// Round to a 1e-9 mm grid (keeps trimmed edges exactly where the kernel expects them).
fn snap(v: Vec2) -> Vec2 {
    let r = |x: f64| (x * 1e9).round() / 1e9;
    Vec2::new(r(v.x), r(v.y))
}

/// Clip a polygon to the half-plane (p − o)·n ≥ 0.
fn clip(poly: &[Vec2], o: Vec2, n: Vec2) -> Vec<Vec2> {
    let mut out = Vec::new();
    let m = poly.len();
    for i in 0..m {
        let (Some(&a), Some(&b)) = (poly.get(i), poly.get((i + 1) % m)) else { continue };
        let (da, db) = ((a - o).dot(n), (b - o).dot(n));
        if da >= 0.0 {
            out.push(a);
        }
        if (da >= 0.0) != (db >= 0.0) {
            let t = da / (da - db);
            out.push(snap(a + (b - a) * t));
        }
    }
    out
}

impl SheetBody {
    /// Bend allowance of a flange.
    pub fn allowance(&self, f: &SheetFlange) -> f64 {
        f.angle.abs() * (f.radius + self.k * self.t)
    }

    /// Fold transform of flange `i`'s panel (flat 3D coordinates → folded, in the flat frame).
    pub fn fold(&self, i: usize) -> Mat {
        let mut chain = Vec::new();
        let mut cur = Some(i);
        for _ in 0..1000 {
            let Some(c) = cur else { break };
            chain.push(c);
            cur = self.flanges.get(c).and_then(|f| f.parent);
        }
        let mut m = IDENTITY;
        for c in chain.into_iter().rev() {
            if let Some(f) = self.flanges.get(c) {
                m = mat_mul(&m, &self.bend_motion(f));
            }
        }
        m
    }

    /// The fold of one bend: drop the bend zone, then turn about the bend axis.
    fn bend_motion(&self, f: &SheetFlange) -> Mat {
        let ba = self.allowance(f);
        let (axis_o, k, a) = self.bend_axis(f);
        mat_mul(&rot_about(axis_o, k, a), &translate(v3(f.n) * -ba))
    }

    /// Bend axis point, direction and signed turn (flat coordinates).
    fn bend_axis(&self, f: &SheetFlange) -> (Vec3, Vec3, f64) {
        let up = f.angle >= 0.0;
        let z = if up { self.t + f.radius } else { -f.radius };
        let o = v3(f.p) + Vec3::Z * z;
        let k = v3(f.n).cross(Vec3::Z);
        (o, k, if up { f.angle.abs() } else { -f.angle.abs() })
    }

    /// Flat region of a flange's panel.
    pub fn panel(&self, f: &SheetFlange) -> Region2 {
        if let Some(sh) = &f.shape {
            return Region2 { outer: Loop2::polygon(sh).ccw(), holes: Vec::new() };
        }
        let ba = self.allowance(f);
        let a = f.p + f.n * ba - f.d * f.ext.0;
        let b = f.p + f.n * ba + f.d * (f.len + f.ext.1);
        let a_far = a + f.n * f.leg;
        let b_far = b + f.n * f.leg;
        let cr = self.corner_relief.min(f.leg);
        let is_v = self.corner_relief_shape == "laser_weld" && cr > 1e-9;
        let mut pts: Vec<Vec2> = vec![a, b];
        if is_v && f.ext.1 > 1e-9 {
            pts.push(f.p + f.n * ba + f.d * (f.len + cr));
        }
        pts.push(b_far);
        pts.push(a_far);
        if is_v && f.ext.0 > 1e-9 {
            pts.push(f.p + f.n * ba - f.d * cr);
        }
        Region2 { outer: Loop2::polygon(&pts).ccw(), holes: Vec::new() }
    }

    /// Flat region of a flange's bend zone.
    pub fn zone(&self, f: &SheetFlange) -> Region2 {
        let ba = self.allowance(f);
        let (u0, u1) = self.zone_span(f);
        let (a, b) = (f.p + f.d * u0, f.p + f.d * u1);
        rect(a, b, b + f.n * ba, a + f.n * ba)
    }

    /// The part of a bend along its parent's edge: where a later flange trimmed that edge back
    /// (a corner), the bend stops and the flange panel runs on alone (a corner relief).
    pub fn zone_span(&self, f: &SheetFlange) -> (f64, f64) {
        if f.parent.is_some() {
            return (0.0, f.len);
        }
        let pts = self.base.outer.ccw().polyline(1e-3);
        let m = pts.len();
        let mut span: Option<(f64, f64)> = None;
        for k in 0..m {
            let (Some(&a), Some(&b)) = (pts.get(k), pts.get((k + 1) % m)) else { continue };
            let on = (a - f.p).dot(f.n).abs() < 1e-6 && (b - f.p).dot(f.n).abs() < 1e-6;
            if !on {
                continue;
            }
            let (ua, ub) = ((a - f.p).dot(f.d), (b - f.p).dot(f.d));
            let (lo, hi) = (ua.min(ub).max(0.0), ua.max(ub).min(f.len));
            if hi > lo + 1e-9 {
                span = Some(span.map_or((lo, hi), |(x, y)| (x.min(lo), y.max(hi))));
            }
        }
        span.unwrap_or((0.0, f.len))
    }

    fn frame_mat(&self) -> Mat {
        let fr = &self.frame;
        let n = fr.normal();
        [[fr.x.x, fr.x.y, fr.x.z, 0.0], [fr.y.x, fr.y.y, fr.y.z, 0.0], [n.x, n.y, n.z, 0.0], [fr.origin.x, fr.origin.y, fr.origin.z, 1.0]]
    }

    /// Flat coordinates (x, y, z) → world, through the fold of flange `i` (None = base).
    pub fn world(&self, i: Option<usize>) -> Mat {
        let f = match i {
            Some(i) if !self.flat => self.fold(i),
            _ => IDENTITY,
        };
        mat_mul(&self.frame_mat(), &f)
    }

    /// Where a flat point of a piece lands (folded), at height `z` in the sheet.
    fn map3(&self, piece: Piece, q: Vec2, z: f64) -> Vec3 {
        match piece {
            Piece::Panel(i) => apply_point(&self.world(i), v3(q) + Vec3::Z * z),
            Piece::Zone(i) => {
                let Some(f) = self.flanges.get(i) else { return Vec3::ZERO };
                let s = (q - f.p).dot(f.n);
                let q0 = q - f.n * s;
                let neutral = (f.radius + self.k * self.t).max(1e-12);
                let phi = (s / neutral).clamp(0.0, f.angle.abs());
                let (axis_o, k, turn) = self.bend_axis(f);
                let m = mat_mul(&self.world(f.parent), &rot_about(axis_o, k, turn.signum() * phi));
                apply_point(&m, v3(q0) + Vec3::Z * z)
            }
        }
    }

    /// A flat edge of a piece as a 3D edge at height `z`.
    fn edge3(&self, piece: Piece, e: &Seg2, z: f64) -> Result<EdgeSpec> {
        let (a, b) = (e.start(), e.end());
        let (pa, pb) = (self.map3(piece, a, z), self.map3(piece, b, z));
        match (piece, e) {
            (Piece::Panel(_), Seg2::Line { .. }) => Ok(EdgeSpec::Line { a: pa, b: pb }),
            (Piece::Panel(_), Seg2::Arc { .. }) => Ok(EdgeSpec::Arc { a: pa, b: pb, mid: self.map3(piece, e.point_at(0.5), z) }),
            (Piece::Zone(i), Seg2::Line { .. }) => {
                let f = self.flanges.get(i).ok_or_else(|| DocError::Invalid("bend".into()))?;
                let dlt = b - a;
                if dlt.dot(f.n).abs() < 1e-9 * dlt.len().max(1.0) {
                    Ok(EdgeSpec::Line { a: pa, b: pb })
                } else if dlt.dot(f.d).abs() < 1e-9 * dlt.len().max(1.0) {
                    Ok(EdgeSpec::Arc { a: pa, b: pb, mid: self.map3(piece, (a + b) * 0.5, z) })
                } else {
                    Err(DocError::Invalid("not supported yet: cut-out edges running across a bend at an angle".into()))
                }
            }
            (Piece::Zone(_), _) => Err(DocError::Invalid("not supported yet: curved cut-outs across a bend".into())),
            (_, _) => Err(DocError::Invalid("not supported yet: spline or conic cut-outs in sheet metal".into())),
        }
    }

    /// The pieces of the sheet as cells: base and flange panels and bend zones, each less
    /// the combined cut-out material. Planar clipping retains holes and disconnected islands.
    fn cells_with_work(&self, work: &mut FlatWork) -> Result<Vec<Cell>> {
        if self.flanges.len().saturating_mul(2).saturating_add(1) > MAX_FLAT_PIECES || self.holes.len() > MAX_FLAT_PIECES {
            return Err(flat_budget_error());
        }
        let raw = self
            .flanges
            .iter()
            .fold(self.base.outer.segs.len(), |n, f| n.saturating_add(f.shape.as_ref().map_or(4, Vec::len)))
            .saturating_add(self.holes.iter().fold(0usize, |n, h| n.saturating_add(h.outer.segs.len())));
        if raw > MAX_FLAT_POINTS {
            return Err(flat_budget_error());
        }
        let mut pieces: Vec<(Piece, Region2)> = self.panels().into_iter().map(|(pi, r)| (Piece::Panel(pi), r)).collect();
        for (i, f) in self.flanges.iter().enumerate() {
            pieces.push((Piece::Zone(i), self.zone(f)));
        }
        let mut cuts = Vec::new();
        let mut cut_points = 0usize;
        for hole in &self.holes {
            let region = planar_region(hole)?;
            cut_points = cut_points.saturating_add(region_complexity(std::slice::from_ref(&region))?.1);
            if cut_points > MAX_FLAT_POINTS {
                return Err(flat_budget_error());
            }
            cuts.push(region);
        }
        cuts.extend(self.through_pocket_regions(work)?);
        let cuts = material_union(cuts, work)?;
        let mut out = Vec::new();
        let mut points = 0usize;
        for (piece, region) in pieces {
            let panel = planar_region(&region)?;
            let material = if cuts.is_empty() { vec![panel] } else { material_boolean(&[panel], &cuts, kernel::BoolOp::Cut, work)? };
            for region in material {
                points = points.saturating_add(region_complexity(std::slice::from_ref(&region))?.1);
                if points > MAX_FLAT_POINTS || out.len() >= MAX_FLAT_PIECES {
                    return Err(flat_budget_error());
                }
                out.push(Cell { piece, outer: region.outer.segs, holes: region.holes.into_iter().map(|l| l.segs).collect() });
            }
        }
        Ok(out)
    }
    /// Faces of the solid made by `cells` between heights `z0` and `z1`.
    fn faces(&self, cells: &[Cell], z0: f64, z1: f64, work: &mut FlatWork) -> Result<Vec<FaceSpec>> {
        // Split straight edges where other cells' corners touch them, so neighbours share edges.
        let edges = cells
            .iter()
            .fold(0usize, |n, c| n.saturating_add(c.outer.len()).saturating_add(c.holes.iter().fold(0usize, |m, h| m.saturating_add(h.len()))));
        if edges > MAX_FLAT_POINTS || cells.len() > MAX_FLAT_PIECES {
            return Err(flat_budget_error());
        }
        work.charge(edges.saturating_mul(edges).saturating_mul(4))?;
        let corners: Vec<Vec2> = cells.iter().flat_map(|c| c.outer.iter().chain(c.holes.iter().flatten()).map(Seg2::start)).collect();
        let mut split_edges = 0usize;
        let mut split_loop = |segs: &[Seg2]| -> Result<Vec<Seg2>> {
            let mut v = Vec::new();
            for e in segs {
                let Seg2::Line { a, b } = *e else {
                    split_edges += 1;
                    if split_edges > MAX_FLAT_POINTS {
                        return Err(flat_budget_error());
                    }
                    v.push(*e);
                    continue;
                };
                let d = b - a;
                let l2 = d.len2();
                let mut ts: Vec<f64> = corners
                    .iter()
                    .filter_map(|p| {
                        let t = (*p - a).dot(d) / l2.max(1e-30);
                        ((t > 1e-9 && t < 1.0 - 1e-9) && (a + d * t).dist(*p) < 1e-7).then_some(t)
                    })
                    .collect();
                ts.sort_by(|x, y| x.total_cmp(y));
                ts.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
                let mut prev = a;
                for t in ts {
                    split_edges += 1;
                    if split_edges > MAX_FLAT_POINTS {
                        return Err(flat_budget_error());
                    }
                    let q = a + d * t;
                    v.push(Seg2::Line { a: prev, b: q });
                    prev = q;
                }
                split_edges += 1;
                if split_edges > MAX_FLAT_POINTS {
                    return Err(flat_budget_error());
                }
                v.push(Seg2::Line { a: prev, b });
            }
            Ok(v)
        };
        let mut split_cells = Vec::new();
        for c in cells {
            let outer = split_loop(&c.outer)?;
            let holes = c.holes.iter().map(|h| split_loop(h)).collect::<Result<_>>()?;
            split_cells.push(Cell { piece: c.piece, outer, holes });
        }
        // Reserve the complete adjacency scan before visiting any wall.
        work.charge(split_edges.saturating_mul(split_edges).saturating_mul(2))?;
        let cells = split_cells;
        let near = |p: Vec2, q: Vec2| p.dist(q) < 1e-6;
        // Shared with a neighbour: the same flat edge the other way, landing in the same place
        // (flat neighbours across a corner of two bends do not).
        let shared = |ci: usize, e: &Seg2| {
            let Some(me) = cells.get(ci) else { return false };
            let here = self.map3(me.piece, e.point_at(0.5), z0);
            cells.iter().enumerate().any(|(cj, c)| {
                cj != ci
                    && c.outer.iter().chain(c.holes.iter().flatten()).any(|g| {
                        near(g.start(), e.end())
                            && near(g.end(), e.start())
                            && matches!((g, e), (Seg2::Line { .. }, Seg2::Line { .. }))
                            && self.map3(c.piece, g.point_at(0.5), z0).dist(here) < 1e-6
                    })
            })
        };
        let mut faces = Vec::new();
        for (ci, c) in cells.iter().enumerate() {
            // Top (z1, outward +z) and bottom (z0, outward −z, loops reversed).
            let loops = |z: f64, rev: bool| -> Result<Vec<Vec<EdgeSpec>>> {
                std::iter::once(&c.outer)
                    .chain(c.holes.iter())
                    .map(|l| {
                        let mut v: Vec<EdgeSpec> = l.iter().map(|e| self.edge3(c.piece, e, z)).collect::<Result<_>>()?;
                        if rev {
                            v = v.into_iter().rev().map(rev_edge).collect();
                        }
                        Ok(v)
                    })
                    .collect()
            };
            let centre = c.outer.iter().map(Seg2::start).fold(Vec2::default(), |a, b| a + b) * (1.0 / c.outer.len().max(1) as f64);
            for (z, rev, sign) in [(z1, false, 1.0), (z0, true, -1.0)] {
                let surface = match c.piece {
                    Piece::Panel(_) => SurfSpec::Plane,
                    Piece::Zone(i) => {
                        let f = self.flanges.get(i).ok_or_else(|| DocError::Invalid("bend".into()))?;
                        let ss: Vec<f64> = c.outer.iter().map(|e| (e.start() - f.p).dot(f.n)).collect();
                        let (s0, s1) = (ss.iter().cloned().fold(f64::MAX, f64::min), ss.iter().cloned().fold(f64::MIN, f64::max));
                        let us: Vec<f64> = c.outer.iter().map(|e| (e.start() - f.p).dot(f.d)).collect();
                        let (u0, u1) = (us.iter().cloned().fold(f64::MAX, f64::min), us.iter().cloned().fold(f64::MIN, f64::max));
                        let neutral = (f.radius + self.k * self.t).max(1e-12);
                        let (axis_o, k, turn) = self.bend_axis(f);
                        let wp = self.world(f.parent);
                        let q = |u: f64| f.p + f.n * s0 + f.d * u;
                        SurfSpec::Revolved {
                            a: self.map3(c.piece, q(u0), z),
                            b: self.map3(c.piece, q(u1), z),
                            axis_origin: apply_point(&wp, axis_o),
                            axis: apply_vector(&wp, k),
                            angle: turn.signum() * (s1 - s0) / neutral,
                        }
                    }
                };
                let p = self.map3(c.piece, centre, z);
                let outward = (self.map3(c.piece, centre, z + 1.0) - p) * sign;
                faces.push(FaceSpec { loops: loops(z, rev)?, surface, probe: Some((p, outward)) });
            }
            // Walls along edges no other cell shares.
            for e in c.outer.iter().chain(c.holes.iter().flatten()) {
                if shared(ci, e) {
                    continue;
                }
                let (a, b) = (e.start(), e.end());
                let bottom = self.edge3(c.piece, e, z0)?;
                let top = rev_edge(self.edge3(c.piece, e, z1)?);
                let up_b = EdgeSpec::Line { a: self.map3(c.piece, b, z0), b: self.map3(c.piece, b, z1) };
                let down_a = EdgeSpec::Line { a: self.map3(c.piece, a, z1), b: self.map3(c.piece, a, z0) };
                let surface = match (c.piece, e) {
                    (Piece::Panel(_), Seg2::Arc { .. }) => {
                        let (pa, pb, pm) = (self.map3(c.piece, a, z0), self.map3(c.piece, b, z0), self.map3(c.piece, e.point_at(0.5), z0));
                        SurfSpec::Extruded { a: pa, b: pb, mid: pm, dir: self.map3(c.piece, a, z1) - pa }
                    }
                    _ => SurfSpec::Plane,
                };
                let mid = e.point_at(0.5);
                let tdir = e.point_at(0.51) - e.point_at(0.49);
                let right = Vec2::new(tdir.y, -tdir.x);
                let pm = self.map3(c.piece, mid, (z0 + z1) / 2.0);
                let outward = self.map3(c.piece, mid + right * 1e-3, (z0 + z1) / 2.0) - pm;
                faces.push(FaceSpec { loops: vec![vec![bottom, up_b, top, down_a]], surface, probe: Some((pm, outward)) });
            }
        }
        Ok(faces)
    }

    /// The folded (or, when unfolded, flat) solid, sewn from its faces.
    pub fn solid(&self) -> Result<Body> {
        if self.flat {
            return self.flat_solid();
        }
        let mut work = FlatWork::new();
        let cells = self.cells_with_work(&mut work)?;
        // Validate the bounded material first, then build close to the sheet's own
        // origin. Fitting planes after a distant component transfer loses precision.
        let mut local = self.clone();
        local.frame = Plane::XY;
        let body = kernel::sew(&local.faces(&cells, 0.0, self.t, &mut work)?)?;
        let body = local.machine_pockets(body, &mut work)?;
        self.place_local_solid(body)
    }

    /// The flat pattern as one extrusion (outline with its cut-outs).
    pub fn flat_solid(&self) -> Result<Body> {
        let regions = self.flat_regions()?;
        let mut local = self.clone();
        local.frame = Plane::XY;
        local.flat = true;
        let body = kernel::extrude_planar_regions(&Plane::XY, &regions, 0.0, self.t)?;
        let body = local.machine_pockets(body, &mut FlatWork::new())?;
        self.place_local_solid(body)
    }

    fn place_local_solid(&self, body: Body) -> Result<Body> {
        if self.frame == Plane::XY { Ok(body) } else { Ok(kernel::transform_matrix(&body, self.frame_mat())?) }
    }

    /// Retain bounded derived machining records, without duplicating identical replay work.
    pub(crate) fn record_pocket(&mut self, region: Region2, z0: f64, z1: f64, work: &mut FlatWork) -> Result<()> {
        if !z0.is_finite() || !z1.is_finite() || z0 < 0.0 || z1 > self.t || z0 >= z1 {
            return Err(DocError::Invalid("invalid sheet pocket depth".into()));
        }
        if self.pockets.len() > 1000 {
            return Err(flat_budget_error());
        }
        let pocket = SheetPocket { region, z0, z1 };
        let (old_edges, old_points) = regions_complexity(self.pockets.iter().map(|p| &p.region))?;
        let (new_edges, new_points) = region_complexity(std::slice::from_ref(&pocket.region))?;
        work.charge(
            old_edges.saturating_add(old_points).saturating_add(new_edges.saturating_add(new_points).saturating_mul(self.pockets.len().max(1))),
        )?;
        if self.pockets.contains(&pocket) {
            return Ok(());
        }
        if self.pockets.len() >= 1000 {
            return Err(flat_budget_error());
        }
        if old_edges.saturating_add(new_edges) > MAX_FLAT_POINTS || old_points.saturating_add(new_points) > MAX_FLAT_POINTS {
            return Err(flat_budget_error());
        }
        self.pockets.push(pocket);
        Ok(())
    }

    /// Certify whether a planar panel has material outside a retained thickness interval.
    /// Classification uses exact planar arrangements at every pocket depth boundary.
    pub(crate) fn material_outside_depth(&self, panel: &Region2, z0: f64, z1: f64, work: &mut FlatWork) -> Result<bool> {
        if !z0.is_finite() || !z1.is_finite() || z0 < 0.0 || z1 > self.t || z0 >= z1 {
            return Err(DocError::Invalid("invalid sheet retained depth".into()));
        }
        if self.pockets.len() > 1000 {
            return Err(flat_budget_error());
        }
        regions_complexity(self.pockets.iter().map(|p| &p.region))?;
        let mut levels = vec![0.0, self.t, z0, z1];
        for p in &self.pockets {
            if !p.z0.is_finite() || !p.z1.is_finite() || p.z0 < 0.0 || p.z1 > self.t || p.z0 >= p.z1 {
                return Err(DocError::Invalid("invalid sheet pocket depth".into()));
            }
            levels.extend([p.z0, p.z1]);
        }
        levels.sort_by(f64::total_cmp);
        levels.dedup();
        work.charge(levels.len().saturating_mul(self.pockets.len().max(1)))?;
        for zs in levels.windows(2) {
            if zs[0] >= z0 && zs[1] <= z1 {
                continue;
            }
            let active: Vec<_> = self.pockets.iter().filter(|p| p.z0 <= zs[0] && p.z1 >= zs[1]).map(|p| p.region.clone()).collect();
            let removed = material_union(active, work)?;
            if !material_boolean(std::slice::from_ref(panel), &removed, kernel::BoolOp::Cut, work)?.is_empty() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Replay exact planar machining after forming the full-thickness material.
    fn machine_pockets(&self, mut body: Body, work: &mut FlatWork) -> Result<Body> {
        if self.pockets.len() > 1000 {
            return Err(flat_budget_error());
        }
        regions_complexity(self.pockets.iter().map(|p| &p.region))?;
        let panels = if self.flat {
            vec![(None, self.flat_regions()?)]
        } else {
            self.material_panels(None, work)?.into_iter().map(|(pi, r)| (pi, vec![r])).collect()
        };
        for pocket in &self.pockets {
            if !pocket.z0.is_finite() || !pocket.z1.is_finite() || pocket.z0 < 0.0 || pocket.z1 > self.t || pocket.z0 >= pocket.z1 {
                return Err(DocError::Invalid("invalid sheet pocket depth".into()));
            }
            if !self.flat {
                for flange in &self.flanges {
                    if flat_regions_overlap(&pocket.region, &self.zone(flange), work)? {
                        return Err(DocError::Invalid(
                            "a sheet pocket crosses a bend zone; planar pocket machining cannot yet preserve curved bend material".into(),
                        ));
                    }
                }
            }
            for (pi, panel) in &panels {
                let clipped = material_boolean(panel, std::slice::from_ref(&pocket.region), kernel::BoolOp::Intersect, work)?;
                if clipped.is_empty() {
                    continue;
                }
                let w = if self.flat { self.frame_mat() } else { self.world(*pi) };
                let plane = plane_of(&w, Vec3::ZERO, Vec3::X, Vec3::Y)?;
                // BRep replay is costlier than planar clipping; share the command budget.
                work.charge(100_000)?;
                let tool = kernel::extrude_planar_regions(&plane, &clipped, pocket.z0, pocket.z1)?;
                body = kernel::boolean(&body, &tool, kernel::BoolOp::Cut)?
                    .ok_or_else(|| DocError::Invalid("sheet pocket removes all sheet material".into()))?;
            }
        }
        Ok(body)
    }

    /// Actual curved material, used to certify unsupported folded machining before mutation.
    pub(crate) fn unmodeled_solid(&self, plane: &Plane) -> Result<Option<Body>> {
        if self.flat || self.flanges.is_empty() {
            return Ok(None);
        }
        let mut work = FlatWork::new();
        let cells: Vec<_> = self
            .cells_with_work(&mut work)?
            .into_iter()
            .filter(|c| match c.piece {
                Piece::Zone(_) => true,
                Piece::Panel(pi) => apply_vector(&self.world(pi), Vec3::Z).cross(plane.normal()).len() > 1e-6,
            })
            .collect();
        if cells.is_empty() {
            return Ok(None);
        }
        Ok(Some(kernel::sew(&self.faces(&cells, 0.0, self.t, &mut work)?)?))
    }

    fn through_pocket_regions(&self, work: &mut FlatWork) -> Result<Vec<Region2>> {
        if self.pockets.is_empty() {
            return Ok(Vec::new());
        }
        if self.pockets.len() > 1000 {
            return Err(flat_budget_error());
        }
        regions_complexity(self.pockets.iter().map(|p| &p.region))?;
        let mut levels = vec![0.0, self.t];
        for p in &self.pockets {
            if !p.z0.is_finite() || !p.z1.is_finite() || p.z0 < 0.0 || p.z1 > self.t || p.z0 >= p.z1 {
                return Err(DocError::Invalid("invalid sheet pocket depth".into()));
            }
            levels.extend([p.z0, p.z1]);
        }
        levels.sort_by(f64::total_cmp);
        levels.dedup();
        work.charge(levels.len().saturating_mul(self.pockets.len()))?;
        // Most pockets leave an untouched thickness layer. Certify that cheap case
        // before arranging hundreds of unrelated profiles at every depth level.
        if levels.windows(2).any(|zs| !self.pockets.iter().any(|p| p.z0 <= zs[0] && p.z1 >= zs[1])) {
            return Ok(Vec::new());
        }
        let mut through: Option<Vec<Region2>> = None;
        for zs in levels.windows(2) {
            let active: Vec<_> = self.pockets.iter().filter(|p| p.z0 <= zs[0] && p.z1 >= zs[1]).map(|p| p.region.clone()).collect();
            if active.is_empty() {
                return Ok(Vec::new());
            }
            let removed = material_union(active, work)?;
            through = Some(match through {
                None => removed,
                Some(previous) => material_boolean(&previous, &removed, kernel::BoolOp::Intersect, work)?,
            });
            if through.as_ref().is_some_and(Vec::is_empty) {
                return Ok(Vec::new());
            }
        }
        Ok(through.unwrap_or_default())
    }

    /// Pocket operations clipped to the surviving flat material, with exact depth intervals.
    pub fn machining(&self) -> Result<Vec<SheetPocket>> {
        let material = self.flat_regions()?;
        let mut work = FlatWork::new();
        let mut out = Vec::new();
        for pocket in &self.pockets {
            for region in material_boolean(&material, std::slice::from_ref(&pocket.region), kernel::BoolOp::Intersect, &mut work)? {
                if out.len() >= 1000 {
                    return Err(flat_budget_error());
                }
                out.push(SheetPocket { region, z0: pocket.z0, z1: pocket.z1 });
            }
        }
        Ok(out)
    }

    /// Actual flat material boundaries after cut-outs. The same panel and cut regions build
    /// the folded solid; planar unions remove panel/bend seams and turn edge-crossing cuts
    /// into notches. Lines and circular arcs remain exact through clipping and merging.
    pub fn flat_regions(&self) -> Result<Vec<Region2>> {
        let regions = self.flat_material()?;
        if regions.is_empty() {
            return Err(DocError::Invalid("empty flat pattern".into()));
        }
        Ok(regions)
    }

    pub(crate) fn flat_material(&self) -> Result<Vec<Region2>> {
        let mut work = FlatWork::new();
        // Merge structural pieces before cutting: clipping each panel first introduces
        // coincident notch endpoints at seams, while a single material cut has no seams.
        if self.flanges.len().saturating_mul(2).saturating_add(1) > MAX_FLAT_PIECES || self.holes.len() > MAX_FLAT_PIECES {
            return Err(flat_budget_error());
        }
        let raw = self.flanges.iter().fold(self.base.outer.segs.len(), |n, f| n.saturating_add(f.shape.as_ref().map_or(4, Vec::len)));
        if raw > MAX_FLAT_POINTS {
            return Err(flat_budget_error());
        }
        let mut regions = Vec::new();
        let mut panel_points = 0usize;
        for (_, r) in self.panels() {
            let r = planar_region(&r)?;
            panel_points = panel_points.saturating_add(region_complexity(std::slice::from_ref(&r))?.1);
            if panel_points > MAX_FLAT_POINTS {
                return Err(flat_budget_error());
            }
            regions.push(r);
        }
        for f in &self.flanges {
            regions.push(planar_region(&self.zone(f))?);
            region_complexity(&regions)?;
        }
        let regions = material_union(regions, &mut work)?;
        let mut cuts = Vec::new();
        let mut cut_points = 0usize;
        for hole in &self.holes {
            let region = planar_region(hole)?;
            cut_points = cut_points.saturating_add(region_complexity(std::slice::from_ref(&region))?.1);
            if cut_points > MAX_FLAT_POINTS {
                return Err(flat_budget_error());
            }
            cuts.push(region);
        }
        cuts.extend(self.through_pocket_regions(&mut work)?);
        let cuts = material_union(cuts, &mut work)?;
        let regions = if cuts.is_empty() { regions } else { material_boolean(&regions, &cuts, kernel::BoolOp::Cut, &mut work)? };
        Ok(regions)
    }
    /// Outline loops of the flat pattern (flat coordinates): the pieces' edges, shared ones left out.
    pub fn outline(&self) -> Result<Vec<Vec<Vec2>>> {
        if self.flanges.len().saturating_mul(2).saturating_add(1) > MAX_FLAT_PIECES {
            return Err(flat_budget_error());
        }
        let mut polys: Vec<Vec<Vec2>> = vec![dedup(&flat_polyline(&self.base.outer.ccw())?)];
        for f in &self.flanges {
            polys.push(dedup(&flat_polyline(&self.zone(f).outer.ccw())?));
            if f.leg > 1e-9 {
                polys.push(dedup(&flat_polyline(&self.panel(f).outer.ccw())?));
            }
            flat_points(&polys)?;
        }
        let loops = merge_loops(&polys, &mut FlatWork::new())?;
        if loops.is_empty() {
            return Err(DocError::Invalid("empty flat pattern".into()));
        }
        Ok(loops)
    }

    /// The bends: angle, radius, direction, allowance and centre line (flat coordinates).
    pub fn bends(&self) -> Vec<BendInfo> {
        self.flanges
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let ba = self.allowance(f);
                let (u0, u1) = self.zone_span(f);
                let a = f.p + f.n * (ba / 2.0) + f.d * u0;
                BendInfo {
                    index: i + 1,
                    angle_deg: f.angle.abs().to_degrees(),
                    radius: f.radius,
                    up: f.angle >= 0.0,
                    allowance: ba,
                    length: u1 - u0,
                    start: a,
                    end: a + f.d * (u1 - u0),
                }
            })
            .collect()
    }

    /// Topology counted as Fusion does: a round hole is one face bounded by two closed circular
    /// edges (one vertex each), where the sewn solid splits it into arcs. Returns the change to
    /// the sewn (faces, edges, vertices).
    pub fn round_hole_correction(&self) -> (i64, i64, i64) {
        let (mut df, mut de, mut dv) = (0i64, 0i64, 0i64);
        let Ok(regions) = self.flat_regions() else { return (0, 0, 0) };
        for h in regions.iter().flat_map(|r| &r.holes) {
            let arcs: Vec<(Vec2, f64)> =
                h.segs.iter().filter_map(|s| if let Seg2::Arc { center, radius, .. } = s { Some((*center, *radius)) } else { None }).collect();
            let n = h.segs.len() as i64;
            let full =
                !arcs.is_empty() && arcs.len() == h.segs.len() && arcs.iter().all(|(c, r)| c.dist(arcs[0].0) < 1e-9 && (r - arcs[0].1).abs() < 1e-9);
            if full && n > 1 {
                df += 1 - n;
                de += 2 - 3 * n;
                dv += 2 - 2 * n;
            }
        }
        (df, de, dv)
    }

    /// Bounding size of the flat pattern (x, y in the flat frame).
    pub fn flat_size(&self) -> Result<(f64, f64)> {
        let loops: Vec<Vec<Vec2>> = self.flat_regions()?.iter().map(|r| r.outer.polyline(1e-3)).collect();
        let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
        for p in loops.iter().flatten() {
            lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        Ok((hi.x - lo.x, hi.y - lo.y))
    }

    /// Panels with their regions and world placements: (flange index or None, region).
    fn panels(&self) -> Vec<(Option<usize>, Region2)> {
        let mut v = vec![(None, self.base.clone())];
        for (i, f) in self.flanges.iter().enumerate() {
            if f.leg > 1e-9 {
                v.push((Some(i), self.panel(f)));
            }
        }
        v
    }

    /// The panel edge nearest a world point: (panel, edge a, edge b in flat coordinates, on the
    /// top face?, distance).
    pub fn edge_at(&self, p: Vec3) -> Option<(Option<usize>, Vec2, Vec2, bool, f64)> {
        let mut best: Option<(Option<usize>, Vec2, Vec2, bool, f64)> = None;
        for (pi, r) in self.panels() {
            let w = self.world(pi);
            let Some(inv) = mat_inverse(&w) else { continue };
            let q = apply_point(&inv, p);
            let pts = r.outer.ccw().polyline(1e-3);
            let m = pts.len();
            for i in 0..m {
                let (Some(&a), Some(&b)) = (pts.get(i), pts.get((i + 1) % m)) else { continue };
                if a.dist(b) < 1e-9 {
                    continue;
                }
                for (z, top) in [(0.0, false), (self.t, true)] {
                    let q2 = Vec2::new(q.x, q.y);
                    let d = b - a;
                    let t = ((q2 - a).dot(d) / d.len2()).clamp(0.0, 1.0);
                    let dist = ((a + d * t) - q2).len().hypot(q.z - z);
                    if best.as_ref().is_none_or(|x| dist < x.4) {
                        best = Some((pi, a, b, top, dist));
                    }
                }
            }
        }
        best
    }

    /// Trim a panel's edge a→b back by `by` (toward the panel interior).
    fn trim(&mut self, panel: Option<usize>, a: Vec2, b: Vec2, by: f64) -> Result<()> {
        if by.abs() < 1e-12 {
            return Ok(());
        }
        let d = (b - a).normalized().ok_or_else(|| DocError::Invalid("edge".into()))?;
        // Interior side of a counter-clockwise loop is to the left.
        let inward = Vec2::new(-d.y, d.x);
        match panel {
            None => {
                let pts = self.base.outer.ccw().polyline(1e-3);
                let o = a + inward * by;
                let mut cut = clip(&pts, o, inward);
                // Only the material along this edge goes: other parts of the outline past the
                // line (tabs side by side on one line) stay.
                let mut work = FlatWork::new();
                let off = merge_loops(&[clip(&pts, o, inward * -1.0)], &mut work)?;
                if off.len() > 1 {
                    let mid = (a + b) * 0.5;
                    let touches = |p: &Vec<Vec2>| {
                        let m = p.len();
                        (0..m).filter_map(|i| Some((*p.get(i)?, *p.get((i + 1) % m)?))).any(|(p0, p1)| {
                            let e = p1 - p0;
                            let t = if e.len2() > 1e-18 { ((mid - p0).dot(e) / e.len2()).clamp(0.0, 1.0) } else { 0.0 };
                            (p0 + e * t).dist(mid) < 1e-6
                        })
                    };
                    let mut keep = vec![cut];
                    keep.extend(off.into_iter().filter(|p| !touches(p)));
                    cut = match merge_loops(&keep, &mut work)?.as_slice() {
                        [one] => one.clone(),
                        _ => return Err(DocError::Invalid("not supported yet: a flange setback that splits the sheet".into())),
                    };
                }
                if cut.len() < 3 {
                    return Err(DocError::Invalid("the flange's setback removes the whole panel".into()));
                }
                self.base = Region2 { outer: Loop2::polygon(&cut).ccw(), holes: self.base.holes.clone() };
            }
            Some(i) => {
                // A flange panel: its far edge shortens the leg; side edges are not trimmed.
                let Some(f) = self.flanges.get_mut(i) else { return Err(DocError::Invalid("flange".into())) };
                if inward.dot(f.n * -1.0) > 0.99 {
                    f.leg -= by;
                    if f.leg < -1e-9 {
                        return Err(DocError::Invalid("the flange's setback is longer than the panel".into()));
                    }
                } else {
                    return Err(DocError::Invalid("not supported yet: a flange on the side of another flange".into()));
                }
            }
        }
        Ok(())
    }

    /// Add an edge flange on the panel edge nearest `pick` (world): `height` to the outer face
    /// from the virtual sharp, `angle` (0…180°), inside / outside / middle bend position.
    pub fn add_flange(&mut self, pick: Vec3, height: f64, angle: f64, radius: f64, position: &str, flip: bool) -> Result<usize> {
        self.add_flanges(&[pick], height, angle, radius, position, flip)?.into_iter().next().ok_or_else(|| DocError::Invalid("flange".into()))
    }

    /// Edge flanges on several edges at once: every edge is trimmed back first, so flanges on
    /// adjacent edges stop at each other's bends (the corners stay open).
    pub fn add_flanges(&mut self, picks: &[Vec3], height: f64, angle: f64, radius: f64, position: &str, flip: bool) -> Result<Vec<usize>> {
        if !(angle > 1e-6 && angle <= std::f64::consts::PI - 1e-6) {
            return Err(DocError::Invalid("the flange angle must be between 0 and 180°".into()));
        }
        if picks.is_empty() || picks.len() > 1000 {
            return Err(DocError::Invalid("pick 1…1000 edges".into()));
        }
        let sharp = (radius + self.t) * (angle / 2.0).tan();
        let setback = match position {
            "inside" | "" => sharp,
            "outside" => 0.0,
            "middle" | "mid" => sharp / 2.0,
            other => return Err(DocError::Invalid(format!("unknown bend position `{other}` (inside, outside or middle)"))),
        };
        let leg = height - sharp;
        if leg < -1e-9 {
            return Err(DocError::Invalid(format!("the flange height must be at least {sharp:.3} mm for this bend")));
        }
        // Find every edge before anything moves.
        let mut found = Vec::new();
        for p in picks {
            let (panel, a, b, top, dist) = self.edge_at(*p).ok_or_else(|| DocError::Invalid("no sheet edge there".into()))?;
            if dist > self.t * 2.0 + 1e-3 {
                return Err(DocError::Invalid("pick an edge of the sheet's top or bottom face".into()));
            }
            let d = (b - a).normalized().ok_or_else(|| DocError::Invalid("edge".into()))?;
            found.push((panel, a, b, d, top));
        }
        for (panel, a, b, _, _) in &found {
            self.trim(*panel, *a, *b, setback)?;
        }
        let mut made = Vec::new();
        for (panel, a, b, d, top) in found {
            let outward = Vec2::new(d.y, -d.x);
            let line_o = a - outward * setback;
            // The trimmed edge: along d, on the moved line.
            let (start, len) = match panel {
                None => {
                    let pts = self.base.outer.ccw().polyline(1e-3);
                    let m = pts.len();
                    let on_line: Vec<(Vec2, f64)> = (0..m)
                        .filter_map(|k| {
                            let (p0, p1) = (*pts.get(k)?, *pts.get((k + 1) % m)?);
                            let e = p1 - p0;
                            let on = (p0 - line_o).dot(outward).abs() < 1e-6 && (p1 - line_o).dot(outward).abs() < 1e-6;
                            (on && e.dot(d) > 0.0).then_some((p0, e.len()))
                        })
                        .collect();
                    // Of several edges on the line (tabs side by side), the one the pick was on.
                    let (ua, ub) = (a.dot(d), b.dot(d));
                    on_line
                        .iter()
                        .find(|(p0, len)| (p0.dot(d) + len).min(ub) - p0.dot(d).max(ua) > 1e-9)
                        .or_else(|| on_line.first())
                        .copied()
                        .ok_or_else(|| DocError::Invalid("the flange's edge was trimmed away".into()))?
                }
                Some(i) => {
                    let f = self.flanges.get(i).ok_or_else(|| DocError::Invalid("flange".into()))?;
                    let u0 = (line_o - f.p).dot(f.d).clamp(0.0, f.len);
                    (f.p + f.n * (self.allowance(f) + f.leg) + f.d * u0, f.len)
                }
            };
            let up = top != flip;
            self.flanges.push(SheetFlange {
                parent: panel,
                p: snap(start),
                d,
                len,
                n: outward,
                angle: if up { angle } else { -angle },
                radius,
                leg: leg.max(0.0),
                shape: None,
                tag: String::new(),
                ext: (0.0, 0.0),
            });
            made.push(self.flanges.len() - 1);
        }
        self.close_corners(&made);
        Ok(made)
    }

    /// Where two new 90° flanges on the base meet at a corner, run their panels on toward each
    /// other until the gap between their inner corners is the rule's gap.
    fn close_corners(&mut self, made: &[usize]) {
        let ends: Vec<(usize, bool, Vec2)> = made
            .iter()
            .filter_map(|&i| self.flanges.get(i).map(|f| (i, f)))
            .filter(|(_, f)| f.parent.is_none() && (f.angle.abs() - std::f64::consts::FRAC_PI_2).abs() < 1e-9)
            .flat_map(|(i, f)| [(i, false, f.p), (i, true, f.p + f.d * f.len)])
            .collect();
        for &(i, at_end, c) in &ends {
            let Some(f) = self.flanges.get(i).cloned() else { continue };
            let other = ends.iter().find(|&&(j, _, c2)| j != i && c2.dist(c) < 1e-6).and_then(|&(j, _, _)| self.flanges.get(j).cloned());
            let Some(g) = other else { continue };
            if g.n.dot(f.n).abs() > 1e-6 || (g.angle > 0.0) != (f.angle > 0.0) {
                continue;
            }
            // The neighbour's setback: its bend starts that far in from the corner of the outer faces.
            let setback = (g.radius + self.t) * (g.angle.abs() / 2.0).tan();
            let ext = setback - (self.t + self.gap / std::f64::consts::SQRT_2);
            if ext <= 1e-9 {
                continue;
            }
            if let Some(x) = self.flanges.get_mut(i) {
                if at_end {
                    x.ext.1 = ext;
                } else {
                    x.ext.0 = ext;
                }
            }
        }
    }

    /// A flat hem: a 180° bend with a tiny radius at the panel edge, `length` to its outer face.
    pub fn add_hem(&mut self, pick: Vec3, length: f64, gap: f64, flip: bool) -> Result<usize> {
        let (panel, a, b, top, dist) = self.edge_at(pick).ok_or_else(|| DocError::Invalid("no sheet edge there".into()))?;
        if dist > self.t * 2.0 + 1e-3 {
            return Err(DocError::Invalid("pick an edge of the sheet".into()));
        }
        let d = (b - a).normalized().ok_or_else(|| DocError::Invalid("edge".into()))?;
        let outward = Vec2::new(d.y, -d.x);
        let leg = length - (gap + self.t);
        if leg < 0.0 {
            return Err(DocError::Invalid("the hem is shorter than its bend".into()));
        }
        // A hem folds back over the face it starts from (toward the bottom unless flipped).
        let up = top != flip;
        let up = !up;
        self.flanges.push(SheetFlange {
            parent: panel,
            p: a,
            d,
            len: a.dist(b),
            n: outward,
            angle: if up { std::f64::consts::PI } else { -std::f64::consts::PI },
            radius: gap,
            leg,
            ext: (0.0, 0.0),
            shape: None,
            tag: String::new(),
        });
        Ok(self.flanges.len() - 1)
    }

    /// Fold a panel (the base or a flange's) along a line on its top or bottom face (world
    /// points `a`, `b`). The bend zone (width BA = θ·(R + K·T)) is taken out of the existing
    /// sheet, so the flat pattern keeps its size; `position` puts the line at the zone's centre
    /// (`centerline`), start or end (`start`/`end`, from the fixed side), or on the mould line
    /// (`mould`: where the outer faces meet, so the fixed side measures the same to it folded as
    /// flat). On the base the side holding `fixed` (default: the larger side) stays put; on a
    /// flange the side toward its own bend does. `flip` folds the other way.
    #[allow(clippy::too_many_arguments)]
    pub fn add_fold(&mut self, a: Vec3, b: Vec3, angle: f64, radius: f64, position: &str, flip: bool, fixed: Option<Vec3>) -> Result<usize> {
        if !(angle > 1e-6 && angle <= std::f64::consts::PI - 1e-6) {
            return Err(DocError::Invalid("the fold angle must be between 0 and 180°".into()));
        }
        if !(radius >= 0.0) {
            return Err(DocError::Invalid("the bend radius must not be negative".into()));
        }
        // The panel the line lies on: both ends on its top or bottom face, the middle inside it.
        let on_face = |q: Vec3| q.z.abs() < 1e-4 || (q.z - self.t).abs() < 1e-4;
        let mut found = None;
        for (pi, r) in self.panels() {
            let Some(inv) = mat_inverse(&self.world(pi)) else { continue };
            let (fa, fb) = (apply_point(&inv, a), apply_point(&inv, b));
            if !(on_face(fa) && on_face(fb) && (fa.z - fb.z).abs() < 1e-4) {
                continue;
            }
            let pts = dedup(&r.outer.ccw().polyline(1e-3));
            let (a2, b2) = (Vec2::new(fa.x, fa.y), Vec2::new(fb.x, fb.y));
            if point_in(&pts, (a2 + b2) * 0.5) || polys_cross(&pts, &[a2, b2, (a2 + b2) * 0.5]) {
                found = Some((pi, inv, a2, b2, pts));
                break;
            }
        }
        let (panel, inv, a2, b2, pts) = found.ok_or_else(|| DocError::Invalid("the fold line must lie on a flat face of the sheet".into()))?;
        let d = (b2 - a2).normalized().ok_or_else(|| DocError::Invalid("the fold line has no length".into()))?;
        let mut n = Vec2::new(-d.y, d.x);
        let l0 = a2.dot(n);
        // Which side moves. A flange keeps the side its own bend is on.
        let on_line = n * l0;
        let side_area = |m: Vec2| signed_area(&clip(&pts, on_line, m)).abs();
        let moves_n = match (panel, fixed) {
            (Some(j), _) => {
                let f = self.flanges.get(j).ok_or_else(|| DocError::Invalid("flange".into()))?;
                let ba = self.allowance(f);
                // The panel's attaching edge must stay whole on the fixed side.
                let (e0, e1) = (f.p + f.n * ba - f.d * f.ext.0, f.p + f.n * ba + f.d * (f.len + f.ext.1));
                let (s0, s1) = (e0.dot(n) - l0, e1.dot(n) - l0);
                if s0 * s1 < -1e-12 {
                    return Err(DocError::Invalid("a fold on a flange must run across it, clear of the flange's own bend".into()));
                }
                (s0 + s1) < 0.0
            }
            (None, Some(p)) => {
                let q = apply_point(&inv, p);
                let q2 = Vec2::new(q.x, q.y);
                // A point in the panel's slab must be on material: inside the outline and
                // outside every cut-out (a side face of either counts as material).
                let near = |poly: &[Vec2]| {
                    let m = poly.len();
                    (0..m).any(|i| match (poly.get(i), poly.get((i + 1) % m)) {
                        (Some(&a), Some(&b)) => {
                            let d = b - a;
                            let t = if d.len2() > 1e-18 { ((q2 - a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
                            (a + d * t).dist(q2) < 1e-3
                        }
                        _ => false,
                    })
                };
                let in_slab = q.z > -1e-4 && q.z < self.t + 1e-4;
                let in_hole = self.holes.iter().any(|h| h.contains(q2) && !near(&h.outer.ccw().polyline(1e-3)));
                if in_slab && ((!point_in(&pts, q2) && !near(&pts)) || in_hole) {
                    return Err(DocError::Invalid(format!(
                        "the fixed point ({:.3}, {:.3}, {:.3}) is not on the sheet: pick a point on the face of the side that stays",
                        p.x, p.y, p.z
                    )));
                }
                q2.dot(n) < l0
            }
            (None, None) => side_area(n) <= side_area(n * -1.0),
        };
        if !moves_n {
            n = n * -1.0;
        }
        let l = a2.dot(n);
        let probe = SheetFlange { parent: panel, p: a2, d, len: 1.0, n, angle, radius, leg: 0.0, ext: (0.0, 0.0), shape: None, tag: String::new() };
        let ba = self.allowance(&probe);
        let s0 = match position {
            "centerline" | "center" | "centre" => l - ba / 2.0,
            "start" => l,
            "end" => l - ba,
            "mould" | "mold" => l - (radius + self.t) * (angle / 2.0).tan(),
            o => return Err(DocError::Invalid(format!("unknown fold position `{o}` (centerline, start, end or mould)"))),
        };
        let fixed_part = clip(&pts, n * s0, n * -1.0);
        let moving = clip(&pts, n * (s0 + ba), n);
        let strip = clip(&clip(&pts, n * s0, n), n * (s0 + ba), n * -1.0);
        if signed_area(&fixed_part).abs() < 1e-9 || signed_area(&moving).abs() < 1e-9 {
            return Err(DocError::Invalid("the fold line must cross the sheet with material on both sides of the bend".into()));
        }
        // Separate tabs on one line (the zone falls apart into several pieces): fold the ones
        // the line's points span, each as its own bend.
        let strips = merge_loops(std::slice::from_ref(&strip), &mut FlatWork::new())?;
        if strips.len() > 1 {
            let span = (a2.dot(d).min(b2.dot(d)), a2.dot(d).max(b2.dot(d)));
            let angle = if flip { -angle } else { angle };
            return self.fold_tabs(panel, &pts, (n, d), (s0, ba), span, (angle, radius), &strips);
        }
        // The bend zone must be a rectangle across the panel (a slanted or curved edge would
        // run across the bend as a helix, which the sheet's faces can't carry yet).
        let us: Vec<f64> = strip.iter().map(|q| q.dot(d)).collect();
        let (u0, u1) = (us.iter().cloned().fold(f64::MAX, f64::min), us.iter().cloned().fold(f64::MIN, f64::max));
        if ((u1 - u0) * ba - signed_area(&strip).abs()).abs() > 1e-6 * (u1 - u0).max(1.0) * ba.max(1.0) {
            // A corner of the outline inside the zone (e.g. a fold right at a tab's root): name
            // it and how far the line has to move to clear it.
            let corners: Vec<(Vec2, f64)> = pts.iter().map(|q| (*q, q.dot(n) - s0)).filter(|(_, s)| *s > 1e-6 && *s < ba - 1e-6).collect();
            if let Some(&(c, _)) = corners.first() {
                let on = corners.iter().fold(0.0_f64, |m, (_, s)| m.max(*s));
                let back = corners.iter().fold(0.0_f64, |m, (_, s)| m.max(ba - *s));
                let w = apply_point(&self.world(panel), Vec3::new(c.x, c.y, self.t));
                return Err(DocError::Invalid(format!(
                    "the bend zone ({ba:.3} mm wide) reaches a corner of the sheet's outline at ({:.3}, {:.3}, {:.3}): \
                     move the fold line at least {on:.3} mm toward the side that folds or {back:.3} mm toward the side that stays",
                    w.x, w.y, w.z
                )));
            }
            return Err(DocError::Invalid("not supported yet: folds whose bend zone crosses a slanted or curved edge of the sheet".into()));
        }
        let up = !flip;
        let leg = moving.iter().map(|q| q.dot(n) - s0 - ba).fold(0.0, f64::max);
        let p0 = n * s0 + d * u0;
        let ccw = |mut v: Vec<Vec2>| {
            if signed_area(&v) < 0.0 {
                v.reverse();
            }
            v.into_iter().map(snap).collect::<Vec<_>>()
        };
        match panel {
            None => self.base = Region2 { outer: Loop2::polygon(&fixed_part).ccw(), holes: self.base.holes.clone() },
            Some(j) => {
                if let Some(f) = self.flanges.get_mut(j) {
                    f.shape = Some(ccw(fixed_part.clone()));
                }
            }
        }
        self.flanges.push(SheetFlange {
            parent: panel,
            p: snap(p0),
            d,
            len: u1 - u0,
            n,
            angle: if up { angle } else { -angle },
            radius,
            leg,
            ext: (0.0, 0.0),
            shape: Some(ccw(moving)),
            tag: String::new(),
        });
        let i = self.flanges.len() - 1;
        // Flanges on the moving part's edges now hang off the fold and move with it.
        for k in 0..i {
            let Some(f) = self.flanges.get(k) else { continue };
            if f.parent != panel {
                continue;
            }
            let (e0, e1) = ((f.p - p0).dot(n), (f.p + f.d * f.len - p0).dot(n));
            if e0.min(e1) >= ba - 1e-9 {
                if let Some(f) = self.flanges.get_mut(k) {
                    f.parent = Some(i);
                }
            } else if e0.max(e1) > 1e-9 {
                return Err(DocError::Invalid("not supported yet: a fold through a flange's bend".into()));
            }
        }
        // Cut-outs may cross the bend where their edges run along or across it (the faces
        // check the rest when the sheet is built).
        Ok(i)
    }

    /// A fold whose bend zone falls apart into several pieces (`strips`: tabs on one line). The
    /// pieces the line's points span (`span`, along `d`) bend, each with the material beyond it;
    /// the rest of the panel stays put. `n` points to the moving side, the zone is `s0..s0 + ba`.
    #[allow(clippy::too_many_arguments)]
    fn fold_tabs(
        &mut self,
        panel: Option<usize>,
        pts: &[Vec2],
        (n, d): (Vec2, Vec2),
        (s0, ba): (f64, f64),
        span: (f64, f64),
        (angle, radius): (f64, f64),
        strips: &[Vec<Vec2>],
    ) -> Result<usize> {
        const EPS: f64 = 1e-6;
        let urange = |p: &[Vec2]| p.iter().fold((f64::MAX, f64::MIN), |(lo, hi), q| (lo.min(q.dot(d)), hi.max(q.dot(d))));
        let overlap = |a: (f64, f64), b: (f64, f64)| a.1.min(b.1) - a.0.max(b.0);
        let mut work = FlatWork::new();
        // The material beyond the zone, piece by piece, and the zone piece each one hangs from.
        let beyond = merge_loops(&[clip(pts, n * (s0 + ba), n)], &mut work)?;
        let far_edges = |p: &[Vec2]| -> Vec<(f64, f64)> {
            let m = p.len();
            (0..m)
                .filter_map(|i| Some((*p.get(i)?, *p.get((i + 1) % m)?)))
                .filter(|(a, b)| (a.dot(n) - s0 - ba).abs() < EPS && (b.dot(n) - s0 - ba).abs() < EPS)
                .map(|(a, b)| (a.dot(d).min(b.dot(d)), a.dot(d).max(b.dot(d))))
                .collect()
        };
        let strip_u: Vec<(f64, f64)> = strips.iter().map(|s| urange(s)).collect();
        let mut hangs: Vec<Vec<usize>> = vec![Vec::new(); strips.len()];
        for (k, part) in beyond.iter().enumerate() {
            let edges = far_edges(part);
            let on: Vec<usize> = (0..strips.len()).filter(|&i| strip_u.get(i).is_some_and(|&u| edges.iter().any(|&e| overlap(e, u) > EPS))).collect();
            match on.as_slice() {
                [i] => {
                    if let Some(h) = hangs.get_mut(*i) {
                        h.push(k);
                    }
                }
                _ => return Err(DocError::Invalid("not supported yet: folding tabs that join again beyond the bend".into())),
            }
        }
        let chosen: Vec<usize> = (0..strips.len()).filter(|&i| strip_u.get(i).is_some_and(|&u| overlap(u, span) > EPS)).collect();
        if chosen.is_empty() {
            return Err(DocError::Invalid("the fold line must cross the sheet with material on both sides of the bend".into()));
        }
        // What stays: the panel short of the zone plus the zone pieces (and what hangs from
        // them) that don't fold.
        let mut keep = vec![clip(pts, n * s0, n * -1.0)];
        for i in 0..strips.len() {
            if chosen.contains(&i) {
                continue;
            }
            keep.extend(strips.get(i).cloned());
            keep.extend(hangs.get(i).into_iter().flatten().filter_map(|&k| beyond.get(k).cloned()));
        }
        let fixed = match merge_loops(&keep, &mut work)?.as_slice() {
            [one] if signed_area(one) > 1e-9 => one.clone(),
            _ => return Err(DocError::Invalid("not supported yet: a fold that splits the part that stays".into())),
        };
        let mut made = Vec::new();
        for &i in &chosen {
            let (Some(strip), Some(&(u0, u1))) = (strips.get(i), strip_u.get(i)) else { continue };
            // Each piece of the zone must be a rectangle (see `add_fold`).
            if ((u1 - u0) * ba - signed_area(strip).abs()).abs() > 1e-6 * (u1 - u0).max(1.0) * ba.max(1.0) {
                return Err(DocError::Invalid("not supported yet: folds whose bend zone crosses a slanted or curved edge of the sheet".into()));
            }
            let moving = match hangs.get(i).map(Vec::as_slice) {
                Some([k]) => beyond.get(*k).cloned().ok_or_else(|| DocError::Invalid("fold".into()))?,
                _ => return Err(DocError::Invalid("the fold line must cross the sheet with material on both sides of the bend".into())),
            };
            let leg = moving.iter().map(|q| q.dot(n) - s0 - ba).fold(0.0, f64::max);
            made.push((u0, u1, n * s0 + d * u0, leg, moving));
        }
        // Flanges already on this panel: those beyond a folding tab move with it.
        let mut moves = Vec::new();
        for k in 0..self.flanges.len() {
            let Some(f) = self.flanges.get(k) else { continue };
            if f.parent != panel {
                continue;
            }
            let (e0, e1) = (f.p.dot(n) - s0, (f.p + f.d * f.len).dot(n) - s0);
            let fu = (f.p.dot(d).min((f.p + f.d * f.len).dot(d)), f.p.dot(d).max((f.p + f.d * f.len).dot(d)));
            let crosses = Err(DocError::Invalid("not supported yet: a fold through a flange's bend".into()));
            match made.iter().position(|m| fu.0 >= m.0 - 1e-9 && fu.1 <= m.1 + 1e-9) {
                Some(j) if e0.min(e1) >= ba - 1e-9 => moves.push((k, j)),
                Some(_) if e0.max(e1) > 1e-9 => return crosses,
                None if e0.max(e1) > 1e-9 && made.iter().any(|m| overlap((m.0, m.1), fu) > EPS) => return crosses,
                _ => {}
            }
        }
        let ccw = |mut v: Vec<Vec2>| {
            if signed_area(&v) < 0.0 {
                v.reverse();
            }
            v.into_iter().map(snap).collect::<Vec<_>>()
        };
        match panel {
            None => self.base = Region2 { outer: Loop2::polygon(&fixed).ccw(), holes: self.base.holes.clone() },
            Some(j) => {
                if let Some(f) = self.flanges.get_mut(j) {
                    f.shape = Some(ccw(fixed));
                }
            }
        }
        let first = self.flanges.len();
        for (u0, u1, p0, leg, moving) in made {
            self.flanges.push(SheetFlange {
                parent: panel,
                p: snap(p0),
                d,
                len: u1 - u0,
                n,
                angle,
                radius,
                leg,
                ext: (0.0, 0.0),
                shape: Some(ccw(moving)),
                tag: String::new(),
            });
        }
        for (k, j) in moves {
            if let Some(f) = self.flanges.get_mut(k) {
                f.parent = Some(first + j);
            }
        }
        Ok(first)
    }

    /// Name a panel edge (flat coordinates a→b on `panel`, top or bottom face): base edges by the
    /// base sketch line they lie on, flange edges by the flange (its tag) and side.
    fn edge_name_of(&self, panel: Option<usize>, a: Vec2, b: Vec2, top: bool) -> Option<String> {
        let face = if top { "top" } else { "bottom" };
        let d = (b - a).normalized()?;
        match panel {
            None => {
                let on = |p: Vec2, s: Vec2, e: Vec2| {
                    let t = e - s;
                    t.len() > 1e-9 && ((p - s).cross(t) / t.len()).abs() < 1e-6
                };
                self.base_curves.iter().find(|(_, s, e)| on(a, *s, *e) && on(b, *s, *e)).map(|(id, _, _)| format!("base:{id}:{face}"))
            }
            Some(i) => {
                let f = self.flanges.get(i)?;
                if f.tag.is_empty() {
                    return None;
                }
                let side = if d.cross(f.d).abs() < 1e-6 {
                    // Along the bend: the far edge (or the bend side, never picked).
                    if ((a - f.p).dot(f.n)) > self.allowance(f) + 1e-6 { "far" } else { "bend" }
                } else if ((a + b) * 0.5 - f.p).dot(f.d) < f.len / 2.0 {
                    "side0"
                } else {
                    "side1"
                };
                Some(format!("{}:{side}:{face}", f.tag))
            }
        }
    }

    /// The persistent name of the sheet edge nearest a world point.
    pub fn edge_name(&self, pick: Vec3) -> Option<String> {
        let (panel, a, b, top, _) = self.edge_at(pick)?;
        self.edge_name_of(panel, a, b, top)
    }

    /// A world point on the sheet edge with this name (the one nearest `near` if several).
    pub fn edge_by_name(&self, name: &str, near: Vec3) -> Option<Vec3> {
        let mut best: Option<(f64, Vec3)> = None;
        for (pi, r) in self.panels() {
            let pts = r.outer.ccw().polyline(1e-3);
            let m = pts.len();
            let w = self.world(pi);
            for i in 0..m {
                let (Some(&a), Some(&b)) = (pts.get(i), pts.get((i + 1) % m)) else { continue };
                if a.dist(b) < 1e-9 {
                    continue;
                }
                for (z, top) in [(0.0, false), (self.t, true)] {
                    if self.edge_name_of(pi, a, b, top).as_deref() == Some(name) {
                        let mid = (a + b) * 0.5;
                        let q = apply_point(&w, Vec3::new(mid.x, mid.y, z));
                        if best.is_none_or(|(d, _)| q.dist(near) < d) {
                            best = Some((q.dist(near), q));
                        }
                    }
                }
            }
        }
        best.map(|(_, q)| q)
    }

    /// Map a region in a world plane to flat coordinates through a panel's placement.
    pub fn region_to_flat(&self, panel: Option<usize>, plane: &Plane, r: &Region2) -> Result<Region2> {
        region_complexity(std::slice::from_ref(r))?;
        let inv = mat_inverse(&self.world(panel)).ok_or_else(|| DocError::Invalid("invalid sheet panel frame".into()))?;
        let map = |p: Vec2| {
            let q = apply_point(&inv, plane.to_world(p));
            Vec2::new(q.x, q.y)
        };
        // A rigid map of the plane: lines and arcs stay lines and arcs (mirrored or not).
        let (o, ex, ey) = (map(Vec2::default()), map(Vec2::new(1.0, 0.0)), map(Vec2::new(0.0, 1.0)));
        let (ax, ay) = (ex - o, ey - o);
        let mirrored = ax.cross(ay) < 0.0;
        let rot = ax.y.atan2(ax.x);
        let seg = |sg: &Seg2| -> Seg2 {
            match *sg {
                Seg2::Line { a, b } => Seg2::Line { a: map(a), b: map(b) },
                Seg2::Arc { center, radius, start, sweep } => {
                    if mirrored {
                        Seg2::Arc { center: map(center), radius, start: rot - start, sweep: -sweep }
                    } else {
                        Seg2::Arc { center: map(center), radius, start: rot + start, sweep }
                    }
                }
                other => Seg2::Line { a: map(other.start()), b: map(other.end()) },
            }
        };
        let map_loop = |lp: &Loop2| -> Result<Loop2> {
            let curved_other = lp.segs.iter().any(|x| !matches!(x, Seg2::Line { .. } | Seg2::Arc { .. }));
            if curved_other {
                Ok(Loop2::polygon(&flat_polyline(lp)?.into_iter().map(map).collect::<Vec<_>>()))
            } else {
                Ok(Loop2 { segs: lp.segs.iter().map(seg).collect() })
            }
        };
        Ok(Region2 {
            outer: map_loop(&r.outer)?.ccw(),
            holes: r.holes.iter().map(|h| map_loop(h).map(|l| l.ccw().reversed())).collect::<Result<_>>()?,
        })
    }

    /// Panels whose plane is parallel to `plane` (world): (panel, region).
    pub fn parallel_panels(&self, plane: &Plane) -> Vec<(Option<usize>, Region2)> {
        self.panels()
            .into_iter()
            .filter(|(pi, _)| {
                let w = self.world(*pi);
                let n = apply_vector(&w, Vec3::Z);
                n.cross(plane.normal()).len() < 1e-6
            })
            .collect()
    }

    pub(crate) fn material_panels(&self, plane: Option<&Plane>, work: &mut FlatWork) -> Result<Vec<(Option<usize>, Region2)>> {
        Ok(self
            .cells_with_work(work)?
            .into_iter()
            .filter_map(|c| {
                let Piece::Panel(pi) = c.piece else {
                    return None;
                };
                if plane.is_some_and(|p| apply_vector(&self.world(pi), Vec3::Z).cross(p.normal()).len() > 1e-6) {
                    return None;
                }
                Some((pi, Region2 { outer: Loop2 { segs: c.outer }, holes: c.holes.into_iter().map(|segs| Loop2 { segs }).collect() }))
            })
            .collect())
    }
}

fn point_in(poly: &[Vec2], p: Vec2) -> bool {
    let mut inside = false;
    let m = poly.len();
    for i in 0..m {
        let (Some(&a), Some(&b)) = (poly.get(i), poly.get((i + m - 1) % m)) else { continue };
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

/// The boundary of a union of counter-clockwise polygons that meet edge to edge: edges are
/// split where corners touch them, edges running both ways cancel, the rest chain into loops
/// (outer loops counter-clockwise, holes clockwise; straight runs merged).
fn merge_loops(polys: &[Vec<Vec2>], work: &mut FlatWork) -> Result<Vec<Vec<Vec2>>> {
    let n = flat_points(polys)?;
    work.charge(n.saturating_mul(n).saturating_mul(2))?;
    let mut segs: Vec<(Vec2, Vec2)> = Vec::new();
    for p in polys {
        let m = p.len();
        for i in 0..m {
            if let (Some(&a), Some(&b)) = (p.get(i), p.get((i + 1) % m)) {
                segs.push((a, b));
            }
        }
    }
    let pts: Vec<Vec2> = segs.iter().flat_map(|(a, b)| [*a, *b]).collect();
    let mut split: Vec<(Vec2, Vec2)> = Vec::new();
    for (a, b) in &segs {
        let d = *b - *a;
        let l2 = d.len2();
        if l2 < 1e-18 {
            continue;
        }
        let mut ts: Vec<f64> = pts
            .iter()
            .filter_map(|p| {
                let t = (*p - *a).dot(d) / l2;
                ((t > 1e-9 && t < 1.0 - 1e-9) && (*a + d * t).dist(*p) < 1e-7).then_some(t)
            })
            .collect();
        ts.push(0.0);
        ts.push(1.0);
        ts.sort_by(|x, y| x.total_cmp(y));
        ts.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
        for w in ts.windows(2) {
            if split.len() >= MAX_FLAT_POINTS {
                return Err(flat_budget_error());
            }
            split.push((*a + d * w[0], *a + d * w[1]));
        }
    }
    work.charge(split.len().saturating_mul(split.len()).saturating_mul(2))?;
    let same = |p: Vec2, q: Vec2| p.dist(q) < 1e-6;
    let keep: Vec<(Vec2, Vec2)> = split
        .iter()
        .enumerate()
        .filter(|(i, (a, b))| !split.iter().enumerate().any(|(j, (c, d))| j != *i && same(*a, *d) && same(*b, *c)))
        .map(|(_, s)| *s)
        .collect();
    let mut loops = Vec::new();
    let mut used = vec![false; keep.len()];
    for s0 in 0..keep.len() {
        if used.get(s0).copied().unwrap_or(true) {
            continue;
        }
        let mut lp = Vec::new();
        let mut cur = s0;
        for _ in 0..keep.len() + 1 {
            if let Some(u) = used.get_mut(cur) {
                *u = true;
            }
            let Some(&(a, b)) = keep.get(cur) else { break };
            lp.push(a);
            match (0..keep.len()).find(|j| !used.get(*j).copied().unwrap_or(true) && keep.get(*j).is_some_and(|(c, _)| same(*c, b))) {
                Some(n) => cur = n,
                None => break,
            }
        }
        let m = lp.len();
        let clean: Vec<Vec2> = (0..m)
            .filter_map(|i| {
                let (p, q, r) = (*lp.get((i + m - 1) % m)?, *lp.get(i)?, *lp.get((i + 1) % m)?);
                ((q - p).cross(r - q).abs() > 1e-9 * (q - p).len().max(1.0) * (r - q).len().max(1.0)).then_some(q)
            })
            .collect();
        if clean.len() >= 3 {
            loops.push(clean);
        }
    }
    Ok(loops)
}

fn signed_area(p: &[Vec2]) -> f64 {
    let m = p.len();
    (0..m).filter_map(|i| Some(p.get(i)?.cross(*p.get((i + 1) % m)?))).sum::<f64>() / 2.0
}

/// Do two polygons' edges cross?
fn polys_cross(a: &[Vec2], b: &[Vec2]) -> bool {
    let seg = |p: &[Vec2], i: usize| -> Option<(Vec2, Vec2)> { Some((*p.get(i)?, *p.get((i + 1) % p.len())?)) };
    (0..a.len()).filter_map(|i| seg(a, i)).any(|(p, q)| {
        (0..b.len()).filter_map(|j| seg(b, j)).any(|(r, s)| {
            let d1 = (q - p).cross(r - p);
            let d2 = (q - p).cross(s - p);
            let d3 = (s - r).cross(p - r);
            let d4 = (s - r).cross(q - r);
            d1 * d2 < -1e-12 && d3 * d4 < -1e-12
        })
    })
}

/// Bounded overlap predicate, including crossing-only cuts whose vertices lie outside a panel.
pub(crate) fn flat_regions_overlap(a: &Region2, b: &Region2, work: &mut FlatWork) -> Result<bool> {
    let (a, b) = (planar_region(a)?, planar_region(b)?);
    Ok(!material_boolean(&[a], &[b], kernel::BoolOp::Intersect, work)?.is_empty())
}

/// A sheet from a closed profile (base flange): the region in `plane`, thickness along its normal.
pub fn base_flange(body: &str, rule: &str, rv: &RuleValues, plane: Plane, region: Region2, corner_relief_shape: &str) -> SheetBody {
    let holes = region.holes.into_iter().map(|h| Region2 { outer: h.ccw(), holes: Vec::new() }).collect();
    SheetBody {
        body: body.into(),
        rule: rule.into(),
        t: rv.thickness,
        k: rv.k_factor,
        frame: plane,
        base: Region2 { outer: region.outer, holes: Vec::new() },
        flanges: Vec::new(),
        holes,
        pockets: Vec::new(),
        flat: false,
        gap: rv.gap,
        corner_relief: rv.corner_relief,
        corner_relief_shape: corner_relief_shape.to_string(),
        base_curves: Vec::new(),
    }
}

/// A sheet from an open chain of lines (contour flange): `pts` in `plane`, the material on the
/// right of the travel (or the left when `flip`), `width` along the plane normal.
pub fn contour_flange(body: &str, rule: &str, rv: &RuleValues, plane: &Plane, pts: &[Vec2], width: f64, flip: bool, corner_relief_shape: &str) -> Result<SheetBody> {
    if pts.len() < 2 {
        return Err(DocError::Invalid("a contour flange needs an open chain of lines".into()));
    }
    let (t, r) = (rv.thickness, rv.bend_radius);
    let seg_dir = |i: usize| -> Result<(Vec2, f64)> {
        let (Some(a), Some(b)) = (pts.get(i), pts.get(i + 1)) else { return Err(DocError::Invalid("chain".into())) };
        let l = a.dist(*b);
        Ok(((*b - *a).normalized().ok_or_else(|| DocError::Invalid("zero-length line in the contour".into()))?, l))
    };
    let side = if flip { -1.0 } else { 1.0 };
    // Material side in the sketch: right of travel.
    let (d0, _) = seg_dir(0)?;
    let m0 = Vec2::new(d0.y, -d0.x) * side;
    let start = pts.first().copied().unwrap_or_default();
    let (x, z) = (plane.dir_to_world(d0), plane.dir_to_world(m0));
    let frame = Plane::new(plane.to_world(start), x, z.cross(x)).ok_or_else(|| DocError::Invalid("contour frame".into()))?;
    // Width along the sketch normal: +y or −y of the frame.
    let ysign = if frame.y.dot(plane.normal()) >= 0.0 { 1.0 } else { -1.0 };
    let n = pts.len() - 1;
    // Turn at each corner: angle and whether it bends toward the material (up).
    let mut corners = Vec::new();
    for i in 0..n.saturating_sub(1) {
        let (da, _) = seg_dir(i)?;
        let (db, _) = seg_dir(i + 1)?;
        let cross = da.cross(db) * side;
        let ang = da.dot(db).clamp(-1.0, 1.0).acos();
        if ang < 1e-9 {
            return Err(DocError::Invalid("straight corners in a contour flange".into()));
        }
        // Right-hand material: turning right (negative cross) bends toward it.
        corners.push((ang, cross < 0.0));
    }
    let setback = |ang: f64, up: bool| if up { (r + t) * (ang / 2.0).tan() } else { r * (ang / 2.0).tan() };
    let mut sheet = SheetBody {
        body: body.into(),
        rule: rule.into(),
        t,
        k: rv.k_factor,
        frame,
        base: Region2 { outer: Loop2 { segs: Vec::new() }, holes: Vec::new() },
        flanges: Vec::new(),
        holes: Vec::new(),
        pockets: Vec::new(),
        flat: false,
        gap: rv.gap,
        corner_relief: rv.corner_relief,
        corner_relief_shape: corner_relief_shape.to_string(),
        base_curves: Vec::new(),
    };
    let (y0, y1) = if ysign > 0.0 { (0.0, width) } else { (-width, 0.0) };
    // First segment: the base panel.
    let (_, l0) = seg_dir(0)?;
    let end0 = l0 - corners.first().map(|(a, u)| setback(*a, *u)).unwrap_or(0.0);
    if end0 <= 0.0 {
        return Err(DocError::Invalid("a contour line is shorter than its bends".into()));
    }
    sheet.base = rect(Vec2::new(0.0, y0), Vec2::new(end0, y0), Vec2::new(end0, y1), Vec2::new(0.0, y1));
    let mut parent: Option<usize> = None;
    let mut at = end0;
    for (i, (ang, up)) in corners.iter().enumerate() {
        let (_, l) = seg_dir(i + 1)?;
        let before = setback(*ang, *up);
        let after = corners.get(i + 1).map(|(a, u)| setback(*a, *u)).unwrap_or(0.0);
        let leg = l - before - after;
        if leg < -1e-9 {
            return Err(DocError::Invalid("a contour line is shorter than its bends".into()));
        }
        // The material flips sides relative to the panel when the bend goes down.
        let f = SheetFlange {
            parent,
            p: Vec2::new(at, y0),
            d: Vec2::new(0.0, 1.0),
            len: y1 - y0,
            n: Vec2::new(1.0, 0.0),
            angle: if *up { *ang } else { -*ang },
            radius: r,
            leg: leg.max(0.0),
            ext: (0.0, 0.0),
            shape: None,
            tag: String::new(),
        };
        let ba = sheet.allowance(&f);
        sheet.flanges.push(f);
        parent = Some(sheet.flanges.len() - 1);
        at += ba + leg.max(0.0);
    }
    Ok(sheet)
}

/// Persistent names of the sheet edges nearest each world point (empty when none).
pub fn sheet_edge_names(st: &crate::ModelState, pts: &[Vec3]) -> Vec<String> {
    pts.iter()
        .map(|p| {
            st.sheets
                .iter()
                .filter_map(|sh| sh.edge_at(*p).map(|e| (e.4, sh)))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .and_then(|(_, sh)| sh.edge_name(*p))
                .unwrap_or_default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steel() -> RuleValues {
        RuleValues {
            thickness: 2.5,
            k_factor: 0.44,
            bend_radius: 2.5,
            relief_width: 2.5,
            relief_depth: 1.25,
            corner_relief: 10.0,
            hem_gap: 0.01,
            gap: 2.5,
        }
    }

    fn plate(w: f64, h: f64) -> SheetBody {
        let r = rect(Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h));
        base_flange(
            "Body1",
            "Steel (mm)",
            &steel(),
            Plane::new(Vec3::ZERO, Vec3::X, Vec3::Y)
                .unwrap_or_else(|| Plane::named("XY").unwrap_or(Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Y })),
            r,
            "square",
        )
    }

    fn vol(b: &Body) -> f64 {
        kernel::measure(b).map(|m| m.volume).unwrap_or(f64::NAN)
    }

    #[test]
    fn edge_flange_matches_fusion() {
        // Oracle 68: 100 x 60 plate, 20 mm flange on the y = 0 top edge.
        let mut s = plate(100.0, 60.0);
        s.add_flange(Vec3::new(50.0, 0.0, 2.5), 20.0, std::f64::consts::FRAC_PI_2, 2.5, "inside", false).unwrap();
        let b = s.solid().unwrap();
        assert!((vol(&b) - 18972.622).abs() < 2.0, "{}", vol(&b));
        let (w, h) = s.flat_size().unwrap();
        assert!((w - 100.0).abs() < 1e-9 && (h - 75.654867).abs() < 1e-5, "{w} {h}");
        let bends = s.bends();
        assert_eq!(bends.len(), 1);
        assert!((bends[0].allowance - std::f64::consts::FRAC_PI_2 * 3.6).abs() < 1e-9);
        // Unfolded: the same volume, flat.
        s.flat = true;
        let f = s.solid().unwrap();
        assert!((vol(&f) - 100.0 * 75.654867 * 2.5).abs() < 2.0);
        // Oracle 70: 45°, height 15.
        let mut s = plate(100.0, 60.0);
        s.add_flange(Vec3::new(50.0, 0.0, 2.5), 15.0, std::f64::consts::FRAC_PI_4, 2.5, "inside", false).unwrap();
        assert!((vol(&s.solid().unwrap()) - 18450.777).abs() < 2.0);
        assert!((s.flat_size().unwrap().1 - 73.685298).abs() < 1e-5);
    }

    #[test]
    fn contour_flange_matches_fusion() {
        // Oracle 72: XZ chain (0,0)→(40,0)→(40,−30)→(70,−30), 50 wide.
        let xz = Plane::new(Vec3::ZERO, Vec3::X, Vec3::new(0.0, 0.0, -1.0)).unwrap();
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(40.0, 0.0), Vec2::new(40.0, -30.0), Vec2::new(70.0, -30.0)];
        let s = contour_flange("Body1", "Steel (mm)", &steel(), &xz, &pts, 50.0, false, "square").unwrap();
        let b = s.solid().unwrap();
        assert!((vol(&b) - 12097.622).abs() < 1.2, "{}", vol(&b));
        let m = kernel::measure(&b).unwrap();
        assert!((m.bbox.max.z - 32.5).abs() < 1e-6 && (m.bbox.max.y - 50.0).abs() < 1e-6, "{:?}", m.bbox);
        assert!((s.flat_size().unwrap().0 - 96.309734).abs() < 1e-5, "{:?}", s.flat_size());
    }

    #[test]
    fn hem_and_holes() {
        // Oracle 73: flange then a flat hem of 8 on its top edge.
        let mut s = plate(100.0, 60.0);
        s.add_flange(Vec3::new(50.0, 0.0, 2.5), 20.0, std::f64::consts::FRAC_PI_2, 2.5, "inside", false).unwrap();
        s.add_hem(Vec3::new(50.0, 0.0, 20.0), 8.0, 0.01, false).unwrap();
        let b = s.solid().unwrap();
        assert!((vol(&b) - 21334.723).abs() < 2.0, "{}", vol(&b));
        assert!((s.flat_size().unwrap().1 - 84.632035).abs() < 1e-4, "{:?}", s.flat_size());
        // A hole in the base and a slot across the bend.
        let mut s = plate(80.0, 50.0);
        s.add_flange(Vec3::new(40.0, 0.0, 2.5), 20.0, std::f64::consts::FRAC_PI_2, 2.5, "inside", false).unwrap();
        s.holes.push(Region2 { outer: Loop2::circle(Vec2::new(40.0, 30.0), 3.0), holes: vec![] });
        let v0 = vol(&s.solid().unwrap());
        s.holes.push(rect(Vec2::new(35.0, -12.0), Vec2::new(45.0, -12.0), Vec2::new(45.0, 10.0), Vec2::new(35.0, 10.0)));
        let v1 = vol(&s.solid().unwrap());
        let ba = std::f64::consts::FRAC_PI_2 * 3.6;
        let want = 10.0 * 2.5 * (22.0 - ba) + 10.0 * std::f64::consts::FRAC_PI_2 * (2.5 + 1.25) * 2.5;
        assert!((v0 - v1 - want).abs() < 1.0, "{v0} {v1} {want}");
        s.flat = true;
        assert!(s.solid().is_ok());
    }

    #[test]
    fn disconnected_material_keeps_each_hole_with_its_own_region() {
        let mut s = plate(100.0, 60.0);
        s.holes.push(rect(Vec2::new(45.0, -5.0), Vec2::new(55.0, -5.0), Vec2::new(55.0, 65.0), Vec2::new(45.0, 65.0)));
        s.holes.push(Region2 { outer: Loop2::circle(Vec2::new(20.0, 30.0), 5.0), holes: vec![] });
        let regions = s.flat_regions().unwrap();
        assert_eq!(regions.len(), 2);
        let left = regions.iter().find(|r| r.outer.contains(Vec2::new(10.0, 10.0))).unwrap();
        let right = regions.iter().find(|r| r.outer.contains(Vec2::new(90.0, 10.0))).unwrap();
        assert_eq!(left.holes.len(), 1);
        assert!(left.holes[0].segs.iter().all(|s| matches!(s, Seg2::Arc { radius, .. } if (*radius - 5.0).abs() < 1e-9)));
        assert!(right.holes.is_empty());
        let area: f64 = regions.iter().map(Region2::area).sum();
        assert!((area - (5400.0 - std::f64::consts::PI * 25.0)).abs() < 1e-6);
        assert!((vol(&s.flat_solid().unwrap()) - area * 2.5).abs() < 0.1);
    }

    #[test]
    fn ring_cut_retains_exact_central_material_island() {
        let mut s = plate(100.0, 60.0);
        s.holes.push(Region2 { outer: Loop2::circle(Vec2::new(50.0, 30.0), 10.0), holes: vec![Loop2::circle(Vec2::new(50.0, 30.0), 5.0)] });
        let regions = s.flat_regions().unwrap();
        assert_eq!(regions.len(), 2);
        let island = regions.iter().find(|r| r.outer.contains(Vec2::new(50.0, 30.0))).unwrap();
        assert!(island.holes.is_empty());
        assert!(island.outer.segs.iter().all(|s| matches!(s, Seg2::Arc { radius, .. } if (*radius - 5.0).abs() < 1e-9)));
        let area: f64 = regions.iter().map(Region2::area).sum();
        assert!((area - (6000.0 - std::f64::consts::PI * 75.0)).abs() < 1e-6);
        assert!((vol(&s.flat_solid().unwrap()) - area * 2.5).abs() < 0.1);
    }

    #[test]
    fn freeform_profile_uses_bounded_sampling_with_exact_round_holes() {
        let mut s = plate(100.0, 60.0);
        s.base.outer.segs = vec![
            Seg2::Line { a: Vec2::ZERO, b: Vec2::new(100.0, 0.0) },
            Seg2::Line { a: Vec2::new(100.0, 0.0), b: Vec2::new(100.0, 60.0) },
            Seg2::Cubic { p0: Vec2::new(100.0, 60.0), p1: Vec2::new(200.0 / 3.0, 80.0), p2: Vec2::new(100.0 / 3.0, 80.0), p3: Vec2::new(0.0, 60.0) },
            Seg2::Line { a: Vec2::new(0.0, 60.0), b: Vec2::ZERO },
        ];
        s.holes.push(Region2 { outer: Loop2::circle(Vec2::new(50.0, 30.0), 3.0), holes: vec![] });
        let regions = s.flat_regions().unwrap();
        assert!(regions[0].outer.segs.iter().all(|s| matches!(s, Seg2::Line { .. })));
        assert!(regions[0].holes[0].segs.iter().all(|s| matches!(s, Seg2::Arc { .. })));
        // The cubic has x=100(1-t), y=60+60t(1-t), hence an exact blank area of 7000.
        let expected = (7000.0 - std::f64::consts::PI * 9.0) * 2.5;
        assert!((vol(&s.solid().unwrap()) - expected).abs() < 0.5);
        assert!((vol(&s.flat_solid().unwrap()) - expected).abs() < 0.5);
    }

    #[test]
    fn flat_geometry_budgets_reject_aggregate_and_intermediate_work() {
        let mut s = plate(100.0, 60.0);
        // Each arc is individually legal, but their combined sampled points exceed the cap.
        s.base.outer.segs = vec![Seg2::Arc { center: Vec2::ZERO, radius: 1000.0, start: 0.0, sweep: std::f64::consts::TAU }; 8];
        assert!(s.flat_regions().unwrap_err().to_string().contains("budget"));
        let mut s = plate(100.0, 60.0);
        s.holes = vec![Region2 { outer: Loop2::circle(Vec2::new(20.0, 20.0), 1.0), holes: vec![] }; MAX_FLAT_PIECES + 1];
        assert!(s.flat_regions().unwrap_err().to_string().contains("budget"));
        let p = vec![vec![Vec2::ZERO; MAX_FLAT_POINTS]];
        assert!(merge_loops(&p, &mut FlatWork::new()).unwrap_err().to_string().contains("budget"));
        // Exhausting a clipping/merge allowance must report an error before work begins.
        assert!(merge_loops(&[vec![Vec2::ZERO, Vec2::X, Vec2::Y]], &mut FlatWork(0)).is_err());
    }

    #[test]
    fn overlapping_round_cutouts_union_with_exact_arc_boundaries() {
        let mut s = plate(100.0, 60.0);
        s.holes.push(Region2 { outer: Loop2::circle(Vec2::new(20.0, 30.0), 5.0), holes: vec![] });
        s.holes.push(Region2 { outer: Loop2::circle(Vec2::new(22.0, 30.0), 5.0), holes: vec![] });
        let regions = s.flat_regions().unwrap();
        assert_eq!(regions[0].holes.len(), 1);
        assert!(regions[0].holes[0].segs.iter().all(|s| matches!(s, Seg2::Arc { radius, .. } if (*radius - 5.0).abs() < 1e-9)));
        let intersection = 50.0 * (0.2f64).acos() - (96.0f64).sqrt();
        let area = 6000.0 - (50.0 * std::f64::consts::PI - intersection);
        assert!((regions[0].area() - area).abs() < 1e-6);
        assert!((vol(&s.solid().unwrap()) - area * 2.5).abs() < 0.1);
    }

    #[test]
    fn nested_and_repeated_round_cutouts_keep_only_the_largest_exact_boundary() {
        for reverse in [false, true] {
            let mut s = plate(100.0, 60.0);
            let mut radii = vec![3.0, 5.0, 5.0];
            if reverse {
                radii.reverse();
            }
            for radius in radii {
                s.holes.push(Region2 { outer: Loop2::circle(Vec2::new(20.0, 30.0), radius), holes: vec![] });
            }
            let regions = s.flat_regions().unwrap();
            assert_eq!(regions[0].holes.len(), 1);
            assert!((regions[0].area() - (6000.0 - std::f64::consts::PI * 25.0)).abs() < 1e-6);
            let volume = vol(&s.solid().unwrap());
            // Body measure tessellates curved faces; the exact region area above is the
            // stronger analytic check, while this follows the existing sheet measure budget.
            assert!((volume - (6000.0 - std::f64::consts::PI * 25.0) * 2.5).abs() < 0.1, "{volume}");
            assert_eq!(s.round_hole_correction(), (-1, -4, -2));
        }
    }

    #[test]
    fn sewing_budget_rejects_edge_splitting_growth_before_allocating_faces() {
        let s = plate(100.0, 60.0);
        let cells: Vec<Cell> = (0..250)
            .map(|i| Cell {
                piece: Piece::Panel(None),
                outer: Loop2::polygon(&[Vec2::new(i as f64, 0.0), Vec2::new(250.0, 0.0), Vec2::new(i as f64, 1.0)]).segs,
                holes: vec![],
            })
            .collect();
        assert!(s.faces(&cells, 0.0, 2.5, &mut FlatWork::new()).unwrap_err().to_string().contains("budget"));
        assert!(s.faces(&cells[..1], 0.0, 2.5, &mut FlatWork(0)).unwrap_err().to_string().contains("budget"));
    }

    #[test]
    fn pocket_geometry_and_depth_budgets_are_checked_before_replay() {
        let pocket = SheetPocket {
            region: rect(Vec2::new(20.0, 20.0), Vec2::new(30.0, 20.0), Vec2::new(30.0, 30.0), Vec2::new(20.0, 30.0)),
            z0: 0.0,
            z1: 1.0,
        };
        let mut s = plate(100.0, 60.0);
        s.pockets = vec![pocket.clone(); 1001];
        assert!(s.flat_solid().unwrap_err().to_string().contains("budget"));
        s.pockets = vec![pocket.clone()];
        s.pockets[0].region.outer.segs = vec![Seg2::Line { a: Vec2::ZERO, b: Vec2::X }; MAX_FLAT_POINTS + 1];
        assert!(s.flat_solid().unwrap_err().to_string().contains("budget"));
        for (z0, z1) in [(f64::NAN, 1.0), (-1.0, 1.0), (0.0, 3.0), (1.0, 1.0)] {
            s.pockets = vec![SheetPocket { z0, z1, ..pocket.clone() }];
            assert!(s.flat_solid().unwrap_err().to_string().contains("depth"));
        }
    }
    #[test]
    fn pocket_recording_deduplicates_at_limit_and_rejects_growth_before_mutation() {
        let mut s = plate(100.0, 60.0);
        let pocket =
            SheetPocket { region: rect(Vec2::new(1.0, 1.0), Vec2::new(2.0, 1.0), Vec2::new(2.0, 2.0), Vec2::new(1.0, 2.0)), z0: 0.0, z1: 0.5 };
        s.pockets = vec![pocket.clone(); 1000];
        let mut work = FlatWork::new();
        s.record_pocket(pocket.region.clone(), pocket.z0, pocket.z1, &mut work).unwrap();
        assert_eq!(s.pockets.len(), 1000);
        assert!(s.record_pocket(pocket.region.clone(), 0.0, 0.6, &mut work).is_err());
        assert_eq!(s.pockets.len(), 1000);
        s.pockets.push(pocket.clone());
        assert!(s.record_pocket(pocket.region, 0.0, 0.5, &mut work).is_err());
    }
}
