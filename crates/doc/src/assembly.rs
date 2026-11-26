//! Components and occurrences: the design as a tree.
//!
//! The root component (id 0) is the design. Every other component lives in a parent component
//! through one or more occurrences, each with a rigid transform (instances share the component's
//! contents; a "Paste New" copy is a separate component). A component's sketches, construction
//! geometry and features are authored in its own frame (its origin); the model is evaluated in
//! those frames and placed in the world through the occurrence transforms when bodies are
//! gathered for display, measuring and export (`world_bodies`). Commands take world coordinates
//! and the engine maps a new feature's geometry into the active component's frame.

use serde::{Deserialize, Serialize};
use solidcore_record::{Plane, Vec3};
use solidcraft_geom::prelude::*;

use crate::{DocError, Document, FeatureKind, PlaneRef, Result};

/// Column-major 4×4 affine transform (`m[col][row]`).
pub type Mat = [[f64; 4]; 4];

pub const IDENTITY: Mat = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

pub fn mat_mul(a: &Mat, b: &Mat) -> Mat {
    let mut r = [[0.0; 4]; 4];
    for (c, col) in r.iter_mut().enumerate() {
        for (row, cell) in col.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[k][row] * b[c][k]).sum();
        }
    }
    r
}

/// How much `m` scales a volume: the size of the determinant of its linear part (1 for a rigid
/// placement).
pub fn mat_volume_scale(m: &Mat) -> f64 {
    let c = |i: usize| m.get(i).map_or(Vec3::ZERO, |col| Vec3::new(col[0], col[1], col[2]));
    c(0).dot(c(1).cross(c(2))).abs()
}

/// Why an assembly too large to walk whole is refused.
pub(crate) fn too_many_placements() -> crate::DocError {
    crate::DocError::Invalid(format!("the assembly is deeper than {MAX_PLACEMENT_DEPTH} levels or has more than {MAX_PLACEMENTS} placements"))
}

pub fn apply_point(m: &Mat, p: Vec3) -> Vec3 {
    Vec3::new(
        m[0][0] * p.x + m[1][0] * p.y + m[2][0] * p.z + m[3][0],
        m[0][1] * p.x + m[1][1] * p.y + m[2][1] * p.z + m[3][1],
        m[0][2] * p.x + m[1][2] * p.y + m[2][2] * p.z + m[3][2],
    )
}

pub fn apply_vector(m: &Mat, v: Vec3) -> Vec3 {
    Vec3::new(
        m[0][0] * v.x + m[1][0] * v.y + m[2][0] * v.z,
        m[0][1] * v.x + m[1][1] * v.y + m[2][1] * v.z,
        m[0][2] * v.x + m[1][2] * v.y + m[2][2] * v.z,
    )
}

/// `p` carried by `m`: `None` when `m` flattens it.
pub fn map_plane(m: &Mat, p: &Plane) -> Option<Plane> {
    Plane::new(apply_point(m, p.origin), apply_vector(m, p.x), apply_vector(m, p.y))
}

pub fn is_identity(m: &Mat) -> bool {
    m.iter().flatten().zip(IDENTITY.iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-12)
}

/// Inverse of an affine transform.
pub fn mat_inverse(m: &Mat) -> Option<Mat> {
    let a = |r: usize, c: usize| m[c][r];
    let det = a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1)) - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0))
        + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0));
    if det.abs() < 1e-14 || !det.is_finite() {
        return None;
    }
    let inv = |r: usize, c: usize| -> f64 {
        // Cofactor of (c, r) / det.
        let (r0, r1) = match c {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        let (c0, c1) = match r {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        let minor = a(r0, c0) * a(r1, c1) - a(r0, c1) * a(r1, c0);
        let sign = if (r + c).is_multiple_of(2) { 1.0 } else { -1.0 };
        sign * minor / det
    };
    let mut out = IDENTITY;
    for (c, col) in out.iter_mut().enumerate().take(3) {
        for (r, cell) in col.iter_mut().enumerate().take(3) {
            *cell = inv(r, c);
        }
    }
    let t = Vec3::new(m[3][0], m[3][1], m[3][2]);
    let it = apply_vector(&out, t);
    out[3] = [-it.x, -it.y, -it.z, 1.0];
    Some(out)
}

/// Whether `m` is a placement an occurrence may have: finite, a rotation (orthonormal axes, not
/// mirrored) and a translation within 1e9 mm, its last column `0, 0, 0, 1`. Every placement is
/// checked against this where it enters the document (a design file, a command, an import).
pub fn is_placement(m: &Mat) -> bool {
    let axis = |i: usize| Vec3::new(m[i][0], m[i][1], m[i][2]);
    let (x, y, z) = (axis(0), axis(1), axis(2));
    m.iter().flatten().all(|v| v.is_finite())
        && m[3][..3].iter().all(|v| v.abs() <= 1e9)
        && [x, y, z].iter().all(|v| (v.dot(*v) - 1.0).abs() < 1e-6)
        && x.dot(y).abs() < 1e-6
        && x.dot(z).abs() < 1e-6
        && y.dot(z).abs() < 1e-6
        && x.cross(y).dot(z) > 0.0
        && [m[0][3], m[1][3], m[2][3]].iter().all(|v| v.abs() < 1e-9)
        && (m[3][3] - 1.0).abs() < 1e-9
}

/// Capture a sketch's foreign construction plane in its author's frame. A new choice replaces
/// any previous capture; root sketches and references inside their own component stay live.
pub fn capture_sketch_plane_placement(doc: &Document, plane: &mut PlaneRef, author: u64, frame: &Mat) -> Result<()> {
    let mut current = plane;
    for _ in 0..=64 {
        match current {
            PlaneRef::Construction { feature, placement } => {
                let source = doc.feature(feature.0).ok_or_else(|| DocError::Unknown(format!("construction plane {feature}")))?;
                if !matches!(source.kind, FeatureKind::ConstructionPlane { .. }) {
                    return Err(DocError::Invalid(format!("feature {feature} is not a construction plane")));
                }
                if author == 0 || source.component == author {
                    *placement = None;
                    return Ok(());
                }
                let source_frame = doc.component_transform(source.component);
                if !is_placement(frame) || !is_placement(&source_frame) {
                    return Err(DocError::Invalid("invalid construction plane capture frame".into()));
                }
                let inverse = mat_inverse(frame).ok_or_else(|| DocError::Invalid("sketch author placement cannot be inverted".into()))?;
                let captured = mat_mul(&inverse, &source_frame);
                if !is_placement(&captured) {
                    return Err(DocError::Invalid("invalid captured construction plane placement".into()));
                }
                *placement = Some(captured);
                return Ok(());
            }
            PlaneRef::Offset { base, .. } | PlaneRef::Flipped { base, .. } | PlaneRef::AtAngle { base, .. } | PlaneRef::ThroughPoint { base, .. } => {
                current = base
            }
            _ => return Ok(()),
        }
    }
    Err(DocError::Invalid("construction plane nesting exceeds 64".into()))
}

/// Re-express a captured construction plane when its author or source definition changes
/// coordinate frames. Occurrence motion leaves the captured relationship alone.
pub fn reframe_plane_placements(plane: &mut PlaneRef, author: &Mat, source: impl Fn(u64) -> Mat) -> Result<()> {
    let mut current = plane;
    for _ in 0..=64 {
        match current {
            PlaneRef::Construction { feature, placement } => {
                if let Some(saved) = placement {
                    let from = source(feature.0);
                    if !is_placement(author) || !is_placement(&from) || !is_placement(saved) {
                        return Err(DocError::Invalid("invalid construction plane frame change".into()));
                    }
                    let inverse = mat_inverse(&from).ok_or_else(|| DocError::Invalid("construction plane source frame cannot be inverted".into()))?;
                    let next = mat_mul(&mat_mul(author, saved), &inverse);
                    if !is_placement(&next) {
                        return Err(DocError::Invalid("invalid reframed construction plane placement".into()));
                    }
                    *saved = next;
                }
                return Ok(());
            }
            PlaneRef::Offset { base, .. } | PlaneRef::Flipped { base, .. } | PlaneRef::AtAngle { base, .. } | PlaneRef::ThroughPoint { base, .. } => {
                current = base
            }
            _ => return Ok(()),
        }
    }
    Err(DocError::Invalid("construction plane nesting exceeds 64".into()))
}

/// A rigid transform: rotation by `angle` about `axis` through `origin`, then a translation.
pub fn rigid(translate: Vec3, origin: Vec3, axis: Vec3, angle: f64) -> Mat {
    let k = axis.normalized().unwrap_or(Vec3::Z);
    let (s, c) = angle.sin_cos();
    let t = 1.0 - c;
    let r = [
        [t * k.x * k.x + c, t * k.x * k.y + s * k.z, t * k.x * k.z - s * k.y],
        [t * k.x * k.y - s * k.z, t * k.y * k.y + c, t * k.y * k.z + s * k.x],
        [t * k.x * k.z + s * k.y, t * k.y * k.z - s * k.x, t * k.z * k.z + c],
    ];
    let mut m = IDENTITY;
    for (mc, rc) in m.iter_mut().zip(r) {
        mc[..3].copy_from_slice(&rc);
    }
    let ro = apply_vector(&m, origin);
    let tr = origin - ro + translate;
    m[3] = [tr.x, tr.y, tr.z, 1.0];
    m
}

/// Which coordinate frame an occurrence move reads its translation and rotation in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveSpace {
    #[default]
    Local,
    World,
}

impl MoveSpace {
    pub fn is_local(&self) -> bool {
        *self == MoveSpace::Local
    }
}

/// A placement of a component inside a parent component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Occurrence {
    pub id: u64,
    pub component: u64,
    /// The component it sits in (0 = the root).
    #[serde(default)]
    pub parent: u64,
    pub name: String,
    #[serde(default = "identity")]
    pub transform: Mat,
    /// Baseline transform before timeline `CapturePosition` features are applied.
    /// `None` in legacy designs and before any `CapturePosition` applies to this occurrence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_transform: Option<Mat>,
    /// Grounded occurrences don't move.
    #[serde(default)]
    pub grounded: bool,
    /// How this occurrence counts in the bill of materials, in place of its component's type
    /// (`None`: as its component).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bom_type: Option<BomType>,
    /// Moves whose translation was typed as expressions over the design's parameters, which the
    /// occurrence follows when they change (oldest first; none in older designs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expressed_moves: Vec<ExpressedMove>,
}

impl Occurrence {
    /// Keep `base_transform` consistent when a direct `capture: true` command or joint solve
    /// moves this occurrence from `old_resolved` to `new_resolved`.
    pub fn adjust_base_for_direct_move(&mut self, old_resolved: &Mat, new_resolved: &Mat) {
        if let Some(base) = self.base_transform {
            self.base_transform =
                mat_inverse(old_resolved).map(|inv| mat_mul(&base, &mat_mul(&inv, new_resolved))).filter(is_placement).or(Some(*new_resolved));
        }
    }
}

fn identity() -> Mat {
    IDENTITY
}

/// How a component counts in the bill of materials of an assembly it is placed in. It is a part
/// property (the `bom_type` key of the component's [`Metadata`](crate::Metadata)); a component
/// without one is [`BomType::Normal`]. One occurrence can count as another type
/// ([`Occurrence::bom_type`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BomType {
    /// A row of its own; every occurrence adds to its quantity.
    #[default]
    Normal,
    /// No row of its own: what it holds counts as if placed directly in its parent.
    Phantom,
    /// Shown, but counted nowhere, and nothing inside it is counted either.
    Reference,
    /// Bought: one row, with nothing inside it counted or listed.
    Purchased,
    /// Made as one item that doesn't come apart (a welded frame): one row in the assembly it is
    /// placed in. What it holds is listed in its own bill of materials, and its purchased parts
    /// also in the parts-only list of the assembly it is placed in.
    Inseparable,
}

impl BomType {
    /// Every type, in the order commands and documentation list them.
    pub const ALL: [BomType; 5] = [BomType::Normal, BomType::Phantom, BomType::Reference, BomType::Purchased, BomType::Inseparable];

    /// The word the design file and commands use.
    pub fn as_str(self) -> &'static str {
        match self {
            BomType::Normal => "normal",
            BomType::Phantom => "phantom",
            BomType::Reference => "reference",
            BomType::Purchased => "purchased",
            BomType::Inseparable => "inseparable",
        }
    }

    /// The type a word names, if it names one.
    pub fn parse(word: &str) -> Option<BomType> {
        BomType::ALL.into_iter().find(|t| t.as_str() == word)
    }
}

/// How far an occurrence's expressed moves take it after a parameter change
/// ([`Document::expressed_move_shifts`]).
#[derive(Clone, Debug, PartialEq)]
pub struct ExpressedShift {
    pub occurrence: u64,
    /// In the occurrence's parent frame (mm).
    pub shift: Vec3,
    /// What each of its expressed moves works out to now, in order.
    pub at: Vec<ExpressedAt>,
}

/// What an [`ExpressedMove`]'s expressions work out to: the translation and the turn's origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExpressedAt {
    pub translate: [f64; 3],
    pub origin: [f64; 3],
}

/// How many expressed moves one occurrence keeps.
pub const MAX_EXPRESSED_MOVES: usize = 64;

/// A move of an occurrence whose translation was typed as expressions (`occurrence.move` with
/// `translate: ["0 mm", "dy", 0]`, #8192). The occurrence keeps it and, whenever the design
/// changes, moves on by the difference between what the expressions work out to now and what they
/// worked out to when last applied: a parameter change moves it as the move would have, and
/// every other move of it (a drag, another move) stays.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpressedMove {
    /// Per axis, the length expression typed (`None` for a number, which never changes).
    pub translate: [Option<String>; 3],
    /// What the translation worked out to when it was last applied (mm, along `frame`'s axes).
    pub at: [f64; 3],
    /// Takes the translation into the occurrence's parent frame: the identity when it was typed
    /// in that frame (`space: local`), the parent's world rotation undone when it was typed in
    /// world axes (`space: world`).
    #[serde(default = "identity", skip_serializing_if = "is_identity")]
    pub frame: Mat,
    /// Typed in world axes (`space: world`): `frame` is worked out again when the occurrence
    /// moves to another parent. Older designs read as local.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub world: bool,
    /// Per axis, the expression typed for the point the move turns about (`origin`), `None` for a
    /// number. A move without a turn has no use for it and keeps none.
    #[serde(default, skip_serializing_if = "origin_is_numbers")]
    pub origin: [Option<String>; 3],
    /// What `origin` worked out to when it was last applied (mm, along `frame`'s axes).
    #[serde(default, skip_serializing_if = "is_zero3")]
    pub origin_at: [f64; 3],
    /// The move's turn (a rotation matrix, along the axes it was typed in), which makes the point
    /// it turns about move the occurrence by the part of it the turn leaves behind.
    #[serde(default = "identity", skip_serializing_if = "is_identity")]
    pub rotation: Mat,
    /// Made with `move_grounded`: it moves the occurrence on a parameter change even while the
    /// occurrence is grounded. Without it a grounded occurrence stays put.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub grounded_ok: bool,
}

fn origin_is_numbers(o: &[Option<String>; 3]) -> bool {
    o.iter().all(Option::is_none)
}

fn is_zero3(v: &[f64; 3]) -> bool {
    *v == [0.0; 3]
}

impl ExpressedMove {
    /// What the expressions give now, or why they can't: an unknown name, a wrong unit, or a
    /// length too large.
    pub fn eval(&self, params: &crate::joints::Params) -> Result<ExpressedAt> {
        let one = |what: &str, exprs: &[Option<String>; 3], from: [f64; 3]| -> Result<[f64; 3]> {
            let mut out = from;
            for (slot, e) in out.iter_mut().zip(exprs) {
                let Some(e) = e else { continue };
                let v = params.eval(e, crate::expr::Kind::Length).map_err(|err| DocError::Invalid(format!("{what} `{e}`: {err}")))?;
                if !v.is_finite() || v.abs() > 1e9 {
                    return Err(DocError::Invalid(format!("{what} `{e}` gives {v}")));
                }
                *slot = v;
            }
            Ok(out)
        };
        Ok(ExpressedAt { translate: one("translate", &self.translate, self.at)?, origin: one("origin", &self.origin, self.origin_at)? })
    }

    /// What the move was last applied with.
    pub fn applied(&self) -> ExpressedAt {
        ExpressedAt { translate: self.at, origin: self.origin_at }
    }

    /// How far the move takes the occurrence in its parent's frame when its expressions work out
    /// to `now` instead of what they did: the translation's change, plus the change of the point
    /// the turn is about taken round (`origin - R origin`).
    pub fn shift(&self, now: &ExpressedAt) -> Vec3 {
        let d = |a: [f64; 3], b: [f64; 3]| Vec3::new(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
        let o = d(now.origin, self.origin_at);
        // The primitive refuses only a number that is not finite: `now.origin` is bounded by `eval`
        // and `rotation` is a rigid matrix the commands checked, so for a stored move it does not
        // fail. Were it to, no extra move (the turn's own part is gone) is the safe reading.
        let kept = solidcraft_kernel::affine::pivot_displacement(&self.rotation, [o.x, o.y, o.z]).map_or(Vec3::ZERO, |k| Vec3::new(k[0], k[1], k[2]));
        apply_vector(&self.frame, d(now.translate, self.at) + kept)
    }

    /// The expressions it keeps, with what each is for (`translate y`, `origin x`).
    pub fn exprs(&self) -> Vec<(String, &str)> {
        let mut out = Vec::new();
        for (what, list) in [("translate", &self.translate), ("origin", &self.origin)] {
            for (axis, e) in ["x", "y", "z"].into_iter().zip(list) {
                if let Some(e) = e.as_deref() {
                    out.push((format!("{what} {axis}"), e));
                }
            }
        }
        out
    }

    /// Every expression it keeps, to change.
    pub fn exprs_mut(&mut self) -> impl Iterator<Item = &mut String> {
        self.translate.iter_mut().chain(self.origin.iter_mut()).flatten()
    }
}

/// How deep and how many placements [`Document::placements`] walks.
pub const MAX_PLACEMENT_DEPTH: usize = 64;
pub const MAX_PLACEMENTS: usize = 100_000;

/// `p` moved by `m` (as [`FeatureKind::transform_geometry`] moves a feature's planes).
pub fn map_plane_ref(m: &Mat, p: &mut PlaneRef) {
    match p {
        PlaneRef::Custom { plane } => *plane = map_plane(m, plane).unwrap_or(*plane),
        PlaneRef::Placed { origin, x_dir, y_dir } => {
            map_point_expr(m, origin);
            *x_dir = apply_vector(m, *x_dir);
            *y_dir = apply_vector(m, *y_dir);
        }
        PlaneRef::Face { plane, at, .. } => {
            *plane = map_plane(m, plane).unwrap_or(*plane);
            *at = apply_point(m, *at);
        }
        PlaneRef::Offset { base, .. } | PlaneRef::Flipped { base, .. } => map_plane_ref(m, base),
        PlaneRef::ThroughPoint { base, point } => {
            map_plane_ref(m, base);
            map_geo_ref(m, point);
        }
        PlaneRef::AtAngle { base, axis_origin, axis_dir, line, pivot, .. } => {
            map_plane_ref(m, base);
            map_point_expr(m, axis_origin);
            *axis_dir = apply_vector(m, *axis_dir);
            if let Some(g) = line {
                map_geo_ref(m, g);
            }
            if let Some(g) = pivot {
                map_geo_ref(m, g);
            }
        }
        // Named origin planes, construction planes and placement datums are the component's own.
        PlaneRef::Origin { .. } | PlaneRef::Datum { .. } | PlaneRef::Construction { .. } => {}
        PlaneRef::Tangent { face, at, reference, .. } => {
            *at = apply_point(m, *at);
            map_geo_ref(m, face);
            if let Some(r) = reference {
                map_geo_ref(m, r);
            }
        }
        PlaneRef::Midplane { .. }
        | PlaneRef::TwoEdges { .. }
        | PlaneRef::AxisPoint { .. }
        | PlaneRef::ThreePoints { .. }
        | PlaneRef::AlongPath { .. }
        | PlaneRef::Perpendicular { .. }
        | PlaneRef::SquareToLine { .. } => {
            for r in p.refs_mut() {
                map_geo_ref(m, r);
            }
        }
    }
}

fn map_geo_ref(m: &Mat, g: &mut crate::construct::GeoRef) {
    g.map_points(&mut |p: &mut Vec3| *p = apply_point(m, *p), &mut |pl: &mut PlaneRef| map_plane_ref(m, pl));
}

/// Map a point given as expressions by `m`: numbers stay numbers; a coordinate that is an
/// expression stays one (`m · (x, y, z) + t`, written out), so it still follows its parameters.
pub fn map_point_expr(m: &Mat, p: &mut crate::PointExpr) {
    map_exprs(m, p, " mm");
}

/// [`map_point_expr`] for coordinates read with `unit` after a bare number (" mm" for lengths, ""
/// for the unitless components of a direction).
///
/// A length's constant under 1e-12 mm is rounding left by the matrix and is dropped. A
/// direction has no scale of its own (it is normalized when evaluated), so its constant is
/// dropped only when it is that small next to the numbers it came from: an axis whose only
/// number is 1e-20 keeps it (#18822).
fn map_exprs(m: &Mat, p: &mut crate::PointExpr, unit: &str) {
    let num = |e: &crate::Expr| e.trim().parse::<f64>().ok().filter(|v| v.is_finite());
    if let [Some(x), Some(y), Some(z)] = [num(&p[0]), num(&p[1]), num(&p[2])] {
        *p = crate::point_expr(apply_point(m, Vec3::new(x, y, z)));
        return;
    }
    let direction = unit.is_empty();
    let scale = if direction { p.iter().filter_map(num).fold(0.0_f64, |a, v| a.max(v.abs())) } else { 1.0 };
    let old = p.clone();
    for (row, out) in p.iter_mut().enumerate() {
        let mut constant = m[3][row];
        let mut terms: Vec<String> = Vec::new();
        for (col, e) in old.iter().enumerate() {
            let c = m[col][row];
            match num(e) {
                // A direction's numbers count at any size; a length's below the cutoff are noise.
                Some(v) if direction || c.abs() >= 1e-12 => constant += c * v,
                Some(_) => {}
                None if c.abs() < 1e-12 => {}
                None if (c - 1.0).abs() < 1e-12 => terms.push(format!("({e})")),
                None => terms.push(format!("{c} * ({e})")),
            }
        }
        let kept = constant != 0.0 && constant.abs() >= 1e-12 * scale;
        // A direction's number is written short (`1e-20`, not 20 zeros), and its rounding
        // noise is 0 whether or not an expression is left beside it.
        let text = |v: f64| if direction { short_number(if kept { v } else { 0.0 }) } else { format!("{v}") };
        let mut e = terms.join(" + ");
        if terms.is_empty() {
            e = format!("{}{unit}", text(constant));
        } else if kept {
            e = format!("{e} {} {}{unit}", if constant < 0.0 { '-' } else { '+' }, text(constant.abs()));
        }
        *out = e.into();
    }
}

/// `v` as an expression reads it back exactly: plainly when that is short, else with an
/// exponent (`1e-300`).
fn short_number(v: f64) -> String {
    let plain = format!("{v}");
    if plain.len() <= 24 { plain } else { format!("{v:e}") }
}

/// Map a displacement given as length expressions, retaining its parameter dependencies.
/// Unlike a point, a displacement is unaffected by the frame's translation.
pub fn map_vector_expr(m: &Mat, v: &mut crate::PointExpr) {
    let mut rotation = *m;
    rotation[3] = [0.0, 0.0, 0.0, 1.0];
    map_point_expr(&rotation, v);
}

/// Map a direction given as unitless expressions (a primitive's axis) by `m`'s rotation, keeping
/// its parameter dependencies; the frame's translation doesn't move it.
pub fn map_dir_expr(m: &Mat, d: &mut crate::DirExpr) {
    let mut rotation = *m;
    rotation[3] = [0.0, 0.0, 0.0, 1.0];
    map_exprs(&rotation, d, "");
}

impl FeatureKind {
    /// Map the feature's world geometry (points, directions, explicit planes) by `m`.
    pub fn transform_geometry(&mut self, m: &Mat) {
        if matches!(self, FeatureKind::CapturePosition { .. }) {
            return;
        }
        use crate::feature_geometry::Geo;
        use crate::walk::Walk;
        self.walk_geometry_mut(&mut |g| match g {
            Geo::Pick(p) | Geo::Pos(p) => *p = apply_point(m, *p),
            Geo::Dir(v) => *v = apply_vector(m, *v),
            Geo::Plane(p) => map_plane_ref(m, p),
            Geo::Construct(r) => map_geo_ref(m, r),
            Geo::Placement(e) => map_point_expr(m, e),
            Geo::DirExpr(d) => map_dir_expr(m, d),
            // An Emboss cylinder is picked in its component's frame (`EmbossFace`).
            Geo::Pinned(_) | Geo::Profiles(..) => {}
        });
    }
}

/// [`Document::component_transform`] with each component's first occurrence found by `first`
/// (an index, where many transforms are wanted at once).
pub(crate) fn transform_through<'a>(component: u64, first: impl Fn(u64) -> Option<&'a Occurrence>) -> Mat {
    let mut m = IDENTITY;
    let mut c = component;
    for _ in 0..1000 {
        if c == 0 {
            break;
        }
        let Some(o) = first(c) else { break };
        m = mat_mul(&o.transform, &m);
        c = o.parent;
    }
    m
}

impl Document {
    /// The occurrences of inactive component patterns. They remain in the design so their
    /// identity and assembly references survive suppression and history rollback.
    pub fn inactive_pattern_occurrences(&self) -> std::collections::BTreeSet<u64> {
        if self.occurrences.is_empty() {
            return std::collections::BTreeSet::new();
        }
        let marker = self.marker.unwrap_or(self.features.len());
        self.features
            .iter()
            .enumerate()
            .filter(|(index, feature)| *index >= marker || feature.suppressed)
            .filter_map(|(_, feature)| match &feature.kind {
                crate::FeatureKind::Pattern { occurrences, .. } => Some(occurrences),
                _ => None,
            })
            .flatten()
            .copied()
            .collect()
    }

    /// Active component-pattern copies mapped to their seed occurrence id.
    /// Each copy's placement is owned by its active `FeatureKind::Pattern` feature relative to
    /// the first placement of its component (`reconcile_component_patterns`).
    pub fn pattern_occurrence_seeds(&self) -> std::collections::BTreeMap<u64, u64> {
        if self.occurrences.is_empty() {
            return std::collections::BTreeMap::new();
        }
        let marker = self.marker.unwrap_or(self.features.len());
        let mut by_id = std::collections::HashMap::with_capacity(self.occurrences.len());
        let mut first_of = std::collections::HashMap::new();
        for o in &self.occurrences {
            by_id.insert(o.id, o.component);
            first_of.entry(o.component).or_insert(o.id);
        }
        let mut out = std::collections::BTreeMap::new();
        for (index, feature) in self.features.iter().enumerate() {
            if index >= marker || feature.suppressed {
                continue;
            }
            let crate::FeatureKind::Pattern { components, occurrences, .. } = &feature.kind else {
                continue;
            };
            if components.is_empty() {
                continue;
            }
            for &copy_id in occurrences {
                let Some(&comp) = by_id.get(&copy_id) else { continue };
                let Some(&seed_id) = first_of.get(&comp) else { continue };
                if seed_id != copy_id {
                    out.entry(copy_id).or_insert(seed_id);
                }
            }
        }
        out
    }

    /// Occurrences available at the current history position, in document order.
    pub fn active_occurrences(&self) -> impl Iterator<Item = &Occurrence> {
        let inactive = self.inactive_pattern_occurrences();
        self.occurrences.iter().filter(move |o| !inactive.contains(&o.id))
    }

    /// The first occurrence of a component (its placement for authoring).
    pub fn occurrence_of(&self, component: u64) -> Option<&crate::Occurrence> {
        self.occurrences.iter().find(|o| o.component == component)
    }

    /// The bill of materials type each occurrence counts as, in the order of `occurrences`: its
    /// own ([`Occurrence::bom_type`]), else its component's. Components are looked up once.
    pub fn occurrence_bom_types(&self) -> Vec<BomType> {
        let types: std::collections::HashMap<u64, BomType> =
            self.components.iter().map(|c| (c.id, c.metadata.bom_type())).filter(|(_, t)| *t != BomType::Normal).collect();
        self.occurrences.iter().map(|o| o.bom_type.or_else(|| types.get(&o.component).copied()).unwrap_or_default()).collect()
    }

    /// The occurrences that count as reference ([`Document::occurrence_bom_types`]). A body placed
    /// through any of them is a reference part: the bill of materials counts nothing inside a
    /// reference, and a drawing draws all of it as reference.
    pub fn reference_occurrences(&self) -> std::collections::HashSet<u64> {
        let types = self.occurrences.iter().zip(self.occurrence_bom_types());
        types.filter(|(_, bom_type)| *bom_type == BomType::Reference).map(|(o, _)| o.id).collect()
    }

    /// World transform of a component's frame (through its first occurrence and its parents').
    pub fn component_transform(&self, component: u64) -> Mat {
        transform_through(component, |c| self.occurrence_of(c))
    }

    /// Ancestor chain of `component` from itself up to root (bounded by [`MAX_PLACEMENT_DEPTH`]).
    fn component_ancestors(&self, mut component: u64) -> Vec<u64> {
        let mut chain = vec![component];
        for _ in 0..MAX_PLACEMENT_DEPTH {
            let Some(occ) = self.occurrence_of(component) else { break };
            if chain.contains(&occ.parent) {
                break;
            }
            component = occ.parent;
            chain.push(component);
        }
        chain
    }

    /// Lowest common ancestor of two components (`0` / root when they share no closer ancestor).
    pub fn common_ancestor(&self, a: u64, b: u64) -> u64 {
        if a == 0 || b == 0 {
            return 0;
        }
        if a == b {
            return a;
        }
        let ancestors_a = self.component_ancestors(a);
        for c in self.component_ancestors(b) {
            if ancestors_a.contains(&c) {
                return c;
            }
        }
        0
    }

    /// Rigid transform of `component` relative to `ancestor` (`IDENTITY` when `component == ancestor`
    /// or `ancestor` is not above `component`).
    pub fn component_transform_in(&self, mut component: u64, ancestor: u64) -> Mat {
        let mut m = IDENTITY;
        for _ in 0..MAX_PLACEMENT_DEPTH {
            if component == ancestor {
                break;
            }
            let Some(occ) = self.occurrence_of(component) else { break };
            m = mat_mul(&occ.transform, &m);
            component = occ.parent;
        }
        m
    }

    /// One placement: the component the occurrence `path` from the root leads to (the root for
    /// an empty path) and its world transform. `None` when the path does not currently place it.
    pub fn path_placement(&self, path: &[u64]) -> Option<(u64, Mat)> {
        if !path.is_empty() {
            let inactive = self.inactive_pattern_occurrences();
            if path.iter().any(|id| inactive.contains(id)) {
                return None;
            }
        }
        self.stored_path_placement(path)
    }

    /// A stored placement, including a temporarily inactive pattern copy, for references
    /// that should survive history rollback.
    pub fn stored_path_placement(&self, path: &[u64]) -> Option<(u64, Mat)> {
        if path.len() > MAX_PLACEMENT_DEPTH + 1 {
            return None;
        }
        let (mut m, mut c) = (IDENTITY, 0);
        for id in path {
            let o = self.occurrences.iter().find(|o| o.id == *id && o.parent == c)?;
            m = mat_mul(&m, &o.transform);
            c = o.component;
        }
        Some((c, m))
    }

    /// World transform of one placement of `component`: the occurrence `path` from the root
    /// (empty for the root itself). `None` when the path no longer leads to that component.
    pub fn path_transform(&self, component: u64, path: &[u64]) -> Option<Mat> {
        self.path_placement(path).and_then(|(c, m)| (c == component).then_some(m))
    }

    /// Every placement of each component: (component, occurrence path ids, world transform).
    /// A branch stops past [`MAX_PLACEMENT_DEPTH`] levels, the walk past [`MAX_PLACEMENTS`] placements.
    pub fn placements(&self) -> Vec<(u64, Vec<u64>, Mat)> {
        crate::occurrence_tree::OccurrenceTree::new(self).placements_with(|o| o.transform)
    }

    /// [`Document::placements`] with some occurrences' transforms (in their parents) replaced:
    /// moves not captured yet, or a joint drag's frame, drawn without copying the design.
    pub fn placements_moved(&self, moves: &std::collections::BTreeMap<u64, Mat>) -> Vec<(u64, Vec<u64>, Mat)> {
        crate::occurrence_tree::OccurrenceTree::new(self).placements_with(|o| moves.get(&o.id).copied().unwrap_or(o.transform))
    }

    /// Every placement inside `component`, `component` itself first: (component, occurrence
    /// path ids from `component`, transform into `component`'s frame). Limited as
    /// [`Document::placements`] is.
    pub fn placements_within(&self, component: u64) -> Vec<(u64, Vec<u64>, Mat)> {
        crate::occurrence_tree::OccurrenceTree::new(self).placements_from(component, |o| o.transform)
    }

    /// [`Document::placements_within`], or `None` when the walk stopped at a limit before it
    /// reached every placement.
    pub fn all_placements_within(&self, component: u64) -> Option<Vec<(u64, Vec<u64>, Mat)>> {
        let (placements, complete) = crate::occurrence_tree::OccurrenceTree::new(self).walk_from(component, |o| o.transform);
        complete.then_some(placements)
    }

    /// Whether [`Document::placements`] reaches every placement. Its walk stops at
    /// [`MAX_PLACEMENTS`], and a branch past [`MAX_PLACEMENT_DEPTH`] levels, so a placement it
    /// holds may have occurrences it never walked; the walk is whole when none does.
    pub fn placements_complete(&self) -> bool {
        let mut children = std::collections::HashMap::<u64, usize>::new();
        for o in self.active_occurrences() {
            *children.entry(o.parent).or_default() += 1;
        }
        let walk = self.placements();
        let reached = walk.iter().fold(0usize, |n, (c, _, _)| n.saturating_add(children.get(c).copied().unwrap_or(0)));
        walk.len() == reached.saturating_add(1)
    }

    /// The placement `path` of `component` at `world`, and every placement below it: (component,
    /// occurrence path ids, world transform), the start first. Unlike [`Self::placements`], which
    /// stops a branch where the display does, a larger assembly is refused, never walked in part:
    /// for totals that must cover all of it (physical properties).
    pub fn placements_below(&self, component: u64, path: &[u64], world: Mat) -> crate::Result<Vec<(u64, Vec<u64>, Mat)>> {
        let mut children: std::collections::HashMap<u64, Vec<&Occurrence>> = std::collections::HashMap::new();
        for o in self.active_occurrences() {
            children.entry(o.parent).or_default().push(o);
        }
        let mut out = vec![(component, path.to_vec(), world)];
        let mut stack = vec![(component, path.to_vec(), world)];
        while let Some((parent, path, world)) = stack.pop() {
            for o in children.get(&parent).into_iter().flatten() {
                if path.len() >= MAX_PLACEMENT_DEPTH || out.len() > MAX_PLACEMENTS {
                    return Err(too_many_placements());
                }
                let mut child = path.clone();
                child.push(o.id);
                let w = mat_mul(&world, &o.transform);
                out.push((o.component, child.clone(), w));
                stack.push((o.component, child, w));
            }
        }
        Ok(out)
    }

    /// Every body as the world shows it: its world name ("Body1", or "Body1 (Occ:2)" for a
    /// further instance), its component, the occurrence path, and the transform from the frame
    /// its geometry is in to the world.
    pub fn body_frames(&self, st: &crate::ModelState) -> Vec<(String, u64, Vec<u64>, Mat)> {
        self.body_frames_with_source(st).into_iter().map(|(world, _, component, path, matrix)| (world, component, path, matrix)).collect()
    }

    /// Body frames with explicit definition names; callers must not infer them from labels.
    pub fn body_frames_with_source(&self, st: &crate::ModelState) -> Vec<(String, String, u64, Vec<u64>, Mat)> {
        let mut out = Vec::new();
        let owners = self.model_body_components(st);
        let names = WorldNames::new(self, st);
        for (comp, path, m) in self.placements() {
            for (b, _) in st.bodies.iter().zip(&owners).filter(|(_, owner)| **owner == comp) {
                let name = names.name(&b.name, comp, &path);
                let w = match self.model_body_offset(b) {
                    Some(o) => mat_mul(&m, o),
                    None => m,
                };
                out.push((name, b.definition_name().to_string(), comp, path.clone(), w));
            }
        }
        out
    }

    /// Computed placement name. Source names retain priority; ambiguous labels use full IDs.
    /// Resolves label ambiguity with bounded dynamic counts instead of expanding all placements.
    /// To name many placed bodies, make one [`WorldNames`] and ask it for each.
    pub fn world_body_name(&self, st: &crate::ModelState, source: &str, component: u64, path: &[u64]) -> String {
        // One name: scanning the design once is cheaper than indexing it first.
        placed_name(&Scan { doc: self, st }, source, component, path)
    }

    /// The occurrence path of a component's first placement.
    pub fn first_path(&self, component: u64) -> Vec<u64> {
        let mut path = Vec::new();
        let mut c = component;
        for _ in 0..1000 {
            if c == 0 {
                break;
            }
            let Some(o) = self.occurrence_of(c) else { break };
            path.insert(0, o.id);
            c = o.parent;
        }
        path
    }

    /// Is `a` inside `b` (or `b` itself)?
    pub fn component_within(&self, a: u64, b: u64) -> bool {
        created_within(a, b, |c| self.components.iter().find(|x| x.id == c).map(|x| x.parent))
    }

    /// The virtual components that hold something, by id: a feature (a sketch, a body's, a moved
    /// body's) or a placed component. A virtual component holds nothing ([`crate::metadata::VIRTUAL`]):
    /// the engine refuses any change that puts something in one.
    pub fn virtual_holding(&self) -> Vec<u64> {
        let virtuals: std::collections::HashSet<u64> = self.components.iter().filter(|c| c.metadata.is_virtual()).map(|c| c.id).collect();
        if virtuals.is_empty() {
            return Vec::new();
        }
        let mut held: Vec<u64> =
            self.features.iter().map(|f| f.component).chain(self.occurrences.iter().map(|o| o.parent)).filter(|id| virtuals.contains(id)).collect();
        held.sort_unstable();
        held.dedup();
        held
    }

    /// Is `c` (or one of its ancestors) marked out of the History bar (#10393)? The root (0)
    /// never is: its own features always show.
    pub fn component_history_hidden(&self, mut c: u64) -> bool {
        for _ in 0..1000 {
            match self.components.iter().find(|x| x.id == c) {
                Some(x) if x.history_hidden => return true,
                Some(x) if x.parent != 0 => c = x.parent,
                _ => return false,
            }
        }
        false
    }

    /// Would putting a placement of `component` into `parent` put it inside itself: is `parent`
    /// the component, or inside it, created in it or placed in it at any depth? Placements can sit
    /// apart from where their component was created ([`Self::component_within`] follows only
    /// that), so both are followed.
    pub fn inside_component(&self, parent: u64, component: u64) -> bool {
        if self.component_within(parent, component) {
            return true;
        }
        // Only a component that has an occurrence can be placed in another (the root has none in
        // a sound design).
        if self.occurrences.iter().all(|o| o.component != parent) {
            return false;
        }
        let tree = crate::occurrence_tree::OccurrenceTree::new(self);
        let tree = &tree;
        placed_within(parent, component, move |c| tree.children(c).iter().map(|o| o.component))
    }

    /// The occurrences whose expressed moves ([`ExpressedMove`]) work out to something else than
    /// when they were last applied (a parameter changed), with how far each moves in its parent's
    /// frame. A move whose expressions don't work out (a parameter turned into an angle) leaves the
    /// occurrence where it is until they do again; so does one that would take it out of range.
    pub fn expressed_move_shifts(&self) -> Vec<ExpressedShift> {
        if self.occurrences.iter().all(|o| o.expressed_moves.is_empty()) {
            return Vec::new();
        }
        let params = crate::joints::Params::new(self);
        let mut out = Vec::new();
        for o in self.occurrences.iter().filter(|o| !o.expressed_moves.is_empty()) {
            let mut shift = Vec3::ZERO;
            let mut at = Vec::with_capacity(o.expressed_moves.len());
            for m in &o.expressed_moves {
                let now = m.eval(&params).unwrap_or_else(|_| m.applied());
                // A grounded occurrence stays put, except for the moves made with `move_grounded`.
                // The others keep up with the parameters all the same, so that ungrounding it
                // later doesn't jump by everything that changed meanwhile.
                if !o.grounded || m.grounded_ok {
                    shift += m.shift(&now);
                }
                at.push(now);
            }
            if shift == Vec3::ZERO {
                if o.expressed_moves.iter().zip(&at).any(|(m, now)| m.applied() != *now) {
                    out.push(ExpressedShift { occurrence: o.id, shift: Vec3::ZERO, at });
                }
                continue;
            }
            let moved = Vec3::new(o.transform[3][0], o.transform[3][1], o.transform[3][2]) + shift;
            if shift == Vec3::ZERO || !moved.is_finite() || [moved.x, moved.y, moved.z].iter().any(|x| x.abs() > 1e9) {
                continue;
            }
            out.push(ExpressedShift { occurrence: o.id, shift, at });
        }
        out
    }

    /// Move the occurrences as [`Document::expressed_move_shifts`] worked out.
    pub fn apply_expressed_shifts(&mut self, shifts: &[ExpressedShift]) {
        for s in shifts {
            let Some(o) = self.occurrences.iter_mut().find(|o| o.id == s.occurrence) else { continue };
            for (i, v) in [s.shift.x, s.shift.y, s.shift.z].into_iter().enumerate() {
                if let Some(t) = o.transform[3].get_mut(i) {
                    *t += v;
                }
            }
            for (m, at) in o.expressed_moves.iter_mut().zip(&s.at) {
                (m.at, m.origin_at) = (at.translate, at.origin);
            }
        }
    }

    /// Add an occurrence of an existing component into `parent`, placed by `transform` (which
    /// must pass [`is_placement`]).
    pub fn add_occurrence(&mut self, component: u64, parent: u64, transform: Mat) -> Result<u64> {
        if component == 0 || !self.components.iter().any(|c| c.id == component) {
            return Err(DocError::Unknown(format!("component {component}")));
        }
        if self.inside_component(parent, component) {
            return Err(DocError::Invalid("a component can't contain itself".into()));
        }
        if self.occurrences.len() >= MAX_OCCURRENCES {
            return Err(DocError::Invalid("too many occurrences".into()));
        }
        if !is_placement(&transform) {
            return Err(DocError::Invalid("an occurrence's placement must be a rigid motion (no scaling or mirroring) within 1e9 mm".into()));
        }
        let id = self.next_id.max(1);
        self.next_id = id.checked_add(1).ok_or_else(|| DocError::Invalid("too many items in the design".into()))?;
        let cname = self.components.iter().find(|c| c.id == component).map(|c| c.name.clone()).unwrap_or_default();
        let next = self.occurrences.iter().filter(|o| o.component == component).count() + 1;
        // The numbers the names `{cname}:{k}` take, from one look through the names. No number
        // asked is more than one past the count of occurrences, as there are no more names.
        let mut taken = vec![false; self.occurrences.len() + 2];
        for o in &self.occurrences {
            let k = o.name.strip_prefix(cname.as_str()).and_then(|rest| rest.strip_prefix(':')).and_then(decimal);
            if let Some(slot) = k.and_then(|k| taken.get_mut(k)) {
                *slot = true;
            }
        }
        let k = occurrence_number(next, 1, |k| taken.get(k).is_some_and(|t| *t));
        self.occurrences.push(crate::Occurrence {
            id,
            component,
            parent,
            name: format!("{cname}:{k}"),
            transform,
            base_transform: None,
            grounded: false,
            expressed_moves: Vec::new(),
            bom_type: None,
        });
        Ok(id)
    }

    /// Give each component that has no occurrence one, in the component it was created in, at
    /// the identity. Older designs may lack them. This is what calling [`Self::add_occurrence`]
    /// for each in turn does, with the same names, ids and errors, but what each call would look
    /// for in the whole design (the component, what is placed in each component, the names taken
    /// and how many occurrences the component has) is gathered once and kept up to date. Looking
    /// through the whole design for each component, and through the names for each number
    /// tried until it met that name, took hours on a design of 10 000 components and 90 000 occurrences.
    pub(crate) fn place_unplaced_components(&mut self) -> Result<()> {
        use std::collections::{HashMap, HashSet};
        let mut counts: HashMap<u64, usize> = HashMap::new();
        for o in &self.occurrences {
            *counts.entry(o.component).or_default() += 1;
        }
        let unplaced: Vec<(u64, u64)> = self.components.iter().filter(|c| !counts.contains_key(&c.id)).map(|c| (c.id, c.parent)).collect();
        if unplaced.is_empty() {
            return Ok(());
        }
        let (components, occurrences, next_id) = (&self.components, &mut self.occurrences, &mut self.next_id);
        // The first component with each id, which a search of them finds.
        let mut by_id: HashMap<u64, &crate::Component> = HashMap::new();
        for c in components {
            by_id.entry(c.id).or_insert(c);
        }
        let mut children: HashMap<u64, Vec<u64>> = HashMap::new();
        let mut names: HashSet<String> = HashSet::new();
        for o in occurrences.iter() {
            children.entry(o.parent).or_default().push(o.component);
            names.insert(o.name.clone());
        }
        // Per component name, a number below which every `{name}:{k}` is taken. Names are only
        // added here, so it only grows, and each search for a free number starts there.
        let mut free_from: HashMap<String, usize> = HashMap::new();
        for (component, parent) in unplaced {
            let Some(c) = by_id.get(&component).filter(|_| component != 0) else {
                return Err(DocError::Unknown(format!("component {component}")));
            };
            // As in `inside_component`, only a component that has an occurrence can be placed in
            // another.
            let placed = &children;
            if created_within(parent, component, |x| by_id.get(&x).map(|x| x.parent))
                || (counts.contains_key(&parent) && placed_within(parent, component, move |x| placed.get(&x).into_iter().flatten().copied()))
            {
                return Err(DocError::Invalid("a component can't contain itself".into()));
            }
            if occurrences.len() >= MAX_OCCURRENCES {
                return Err(DocError::Invalid("too many occurrences".into()));
            }
            // The identity is a rigid motion, so the placement needs no check.
            let id = (*next_id).max(1);
            *next_id = id.checked_add(1).ok_or_else(|| DocError::Invalid("too many items in the design".into()))?;
            let count = counts.entry(component).or_default();
            let next = *count + 1;
            *count = next;
            let from = free_from.get(c.name.as_str()).copied().unwrap_or(1);
            let k = occurrence_number(next, from, |k| names.contains(&format!("{}:{k}", c.name)));
            if k != next {
                free_from.insert(c.name.clone(), k + 1);
            }
            let name = format!("{}:{k}", c.name);
            names.insert(name.clone());
            children.entry(parent).or_default().push(component);
            occurrences.push(crate::Occurrence {
                id,
                component,
                parent,
                name,
                transform: IDENTITY,
                base_transform: None,
                grounded: false,
                expressed_moves: Vec::new(),
                bom_type: None,
            });
        }
        Ok(())
    }
}

/// The most occurrences a design holds.
const MAX_OCCURRENCES: usize = 100_000;

/// Is `a` inside `b` (or `b` itself), following each component up to the one it was created in?
/// `parent_of` gives that parent, `None` for an unknown component. The walk stops after 1000
/// steps.
fn created_within(a: u64, b: u64, parent_of: impl Fn(u64) -> Option<u64>) -> bool {
    let mut c = a;
    for _ in 0..1000 {
        if c == b {
            return true;
        }
        if c == 0 {
            return false;
        }
        match parent_of(c) {
            Some(parent) => c = parent,
            None => return false,
        }
    }
    false
}

/// Is `parent` placed in `component` at any depth? `children` gives the components of the
/// occurrences placed in a component, in document order. The walk looks inside at most
/// [`MAX_PLACEMENTS`] components.
fn placed_within<I: IntoIterator<Item = u64>>(parent: u64, component: u64, children: impl Fn(u64) -> I) -> bool {
    let mut stack = vec![component];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(c) = stack.pop() {
        if !seen.insert(c) || seen.len() > MAX_PLACEMENTS {
            continue;
        }
        for child in children(c) {
            if child == parent {
                return true;
            }
            stack.push(child);
        }
    }
    false
}

/// The number `k` in the name `{component name}:{k}` of a new occurrence of a component that has
/// `next - 1` occurrences. It is `next`, or, when an occurrence already has that name (one before
/// it was deleted, or of another component of that name), the first number none has (#9738).
/// `taken` tells whether an occurrence has the name with a number, and every number below `from`
/// must be taken.
fn occurrence_number(next: usize, from: usize, taken: impl Fn(usize) -> bool) -> usize {
    std::iter::once(next).chain(from..).find(|&k| !taken(k)).unwrap_or(next)
}

/// The number `s` spells as `format!` writes one (digits without a leading zero), if it does.
fn decimal(s: &str) -> Option<usize> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0')) {
        return None;
    }
    s.parse().ok()
}

impl Document {
    /// Whether this document uses timeline `CapturePosition` features, occurrence `base_transform`s,
    /// or joint `base_values`.
    pub fn has_capture_position_timeline(&self) -> bool {
        self.occurrences.iter().any(|o| o.base_transform.is_some())
            || self.assembly.joints.iter().any(|j| j.base_values.is_some())
            || self.features.iter().any(|f| matches!(f.kind, FeatureKind::CapturePosition { .. }))
    }

    /// Compute the new local transforms for every occurrence moved by one `CapturePosition` step
    /// without mutating `self` (atomic: if any move fails, nothing changes).
    pub fn compute_capture_position_step(
        &self,
        vals: &std::collections::BTreeMap<String, crate::expr::Value>,
        moves: &[crate::CapturedOccurrenceMove],
    ) -> Result<Vec<(usize, Mat)>> {
        if moves.len() > MAX_PLACEMENTS {
            return Err(DocError::Invalid("too many captured occurrence moves".into()));
        }
        let mut transforms: Vec<Mat> = self.occurrences.iter().map(|o| o.transform).collect();
        let mut by_id = std::collections::HashMap::with_capacity(self.occurrences.len());
        let mut first_of = std::collections::HashMap::with_capacity(self.occurrences.len());
        for (idx, occ) in self.occurrences.iter().enumerate() {
            by_id.entry(occ.id).or_insert(idx);
            first_of.entry(occ.component).or_insert(idx);
        }
        let mut changed: std::collections::BTreeMap<usize, Mat> = std::collections::BTreeMap::new();
        for m in moves {
            let delta = m.eval_delta(vals)?;
            let Some(&idx) = by_id.get(&m.occurrence) else { continue };
            let Some(occ) = self.occurrences.get(idx) else { continue };
            let cur = transforms.get(idx).copied().unwrap_or(IDENTITY);
            let local_delta = if m.space == MoveSpace::Local || occ.parent == 0 {
                delta
            } else {
                let parent_world = m
                    .path
                    .as_deref()
                    .and_then(|path| path_parent_world(&self.occurrences, &transforms, path, occ.id, occ.parent))
                    .unwrap_or_else(|| transform_through_slice(occ.parent, &self.occurrences, &transforms, &first_of));
                let inv = mat_inverse(&parent_world).ok_or_else(|| DocError::Invalid("parent placement is singular".into()))?;
                mat_mul(&mat_mul(&inv, &delta), &parent_world)
            };
            let next = mat_mul(&local_delta, &cur);
            if !is_placement(&next) {
                return Err(DocError::Invalid("captured move placement is out of range".into()));
            }
            if let Some(slot) = transforms.get_mut(idx) {
                *slot = next;
            }
            changed.insert(idx, next);
        }
        Ok(changed.into_iter().collect())
    }

    /// Apply one `CapturePosition` step to `self.occurrences` atomically.
    pub fn apply_capture_position_step(
        &mut self,
        vals: &std::collections::BTreeMap<String, crate::expr::Value>,
        moves: &[crate::CapturedOccurrenceMove],
    ) -> Result<()> {
        let updates = self.compute_capture_position_step(vals, moves)?;
        for (idx, next) in updates {
            if let Some(occ) = self.occurrences.get_mut(idx) {
                occ.transform = next;
            }
        }
        Ok(())
    }

    /// Replay `base_transform` plus active (`!suppressed && i < marker`) `CapturePosition` steps in
    /// timeline order and return `(resolved_occurrence_transforms, resolved_joints)`.
    pub fn replay_occurrence_transforms(
        &self,
    ) -> Result<(std::collections::BTreeMap<u64, Mat>, std::collections::BTreeMap<u64, crate::CapturedJoint>)> {
        let (vals, _) = self.param_values();
        let mut working = self.clone();
        for o in &mut working.occurrences {
            if let Some(base) = o.base_transform {
                o.transform = base;
            }
        }
        let mut joints = std::collections::BTreeMap::new();
        let marker = working.marker.unwrap_or(usize::MAX);
        for (i, f) in self.features.iter().enumerate() {
            if i >= marker || f.suppressed {
                continue;
            }
            if let FeatureKind::CapturePosition { moves, joints: step_joints } = &f.kind {
                working.apply_capture_position_step(&vals, moves)?;
                for cj in step_joints {
                    let resolved = eval_captured_joint(&working, &vals, cj)?;
                    joints.insert(resolved.joint, resolved);
                }
            }
        }
        let occs = working.occurrences.into_iter().map(|o| (o.id, o.transform)).collect();
        Ok((occs, joints))
    }
}

fn transform_through_slice(component: u64, occurrences: &[Occurrence], transforms: &[Mat], first_of: &std::collections::HashMap<u64, usize>) -> Mat {
    let mut m = IDENTITY;
    let mut c = component;
    for _ in 0..MAX_PLACEMENT_DEPTH {
        if c == 0 {
            break;
        }
        let Some(&idx) = first_of.get(&c) else { break };
        let Some(o) = occurrences.get(idx) else { break };
        let Some(t) = transforms.get(idx) else { break };
        m = mat_mul(t, &m);
        c = o.parent;
    }
    m
}

fn path_parent_world(occurrences: &[Occurrence], transforms: &[Mat], path: &[u64], occ_id: u64, occ_parent: u64) -> Option<Mat> {
    if path.len() > MAX_PLACEMENT_DEPTH + 1 {
        return None;
    }
    let (&last, prefix) = path.split_last()?;
    if last != occ_id {
        return None;
    }
    let (mut m, mut c) = (IDENTITY, 0);
    for id in prefix {
        let (idx, o) = occurrences.iter().enumerate().find(|(_, o)| o.id == *id && o.parent == c)?;
        let t = transforms.get(idx)?;
        m = mat_mul(&m, t);
        c = o.component;
    }
    (c == occ_parent).then_some(m)
}

impl crate::CapturedOccurrenceMove {
    /// Evaluate this move's rigid delta matrix against `vals`.
    pub fn eval_delta(&self, vals: &std::collections::BTreeMap<String, crate::expr::Value>) -> Result<Mat> {
        let t = crate::eval::point(vals, &self.translate)?;
        if let Some(rot) = self.rotation {
            if !is_placement(&rot) || rot[3][..3].iter().any(|v| v.abs() >= 1e-9) {
                return Err(DocError::Invalid("invalid captured move rotation".into()));
            }
            let mut m = rot;
            m[3] = [t.x, t.y, t.z, 1.0];
            if !is_placement(&m) {
                return Err(DocError::Invalid("captured move placement is out of range".into()));
            }
            return Ok(m);
        }
        let origin = match &self.origin {
            Some(o) => crate::eval::point(vals, o)?,
            None => Vec3::ZERO,
        };
        let angle = match &self.angle {
            Some(a) => crate::eval::val(vals, a, crate::expr::Kind::Angle)?,
            None => 0.0,
        };
        if !angle.is_finite() {
            return Err(DocError::Invalid("captured move angle must be finite".into()));
        }
        let axis = match self.rotate_axis {
            Some([x, y, z]) => {
                let v = Vec3::new(x, y, z);
                if !v.is_finite() || (angle != 0.0 && v.x * v.x + v.y * v.y + v.z * v.z == 0.0) {
                    return Err(DocError::Invalid("captured move rotation axis must be finite and nonzero".into()));
                }
                v
            }
            None => Vec3::Z,
        };
        let m = rigid(t, origin, axis, angle);
        if !is_placement(&m) {
            return Err(DocError::Invalid("captured move placement is out of range".into()));
        }
        Ok(m)
    }
}

/// Evaluate any parameter expressions on a [`crate::CapturedJoint`] against `vals`, updating its
/// resolved DOF values while leaving non-expressed DOFs unchanged.
pub fn eval_captured_joint(
    doc: &Document,
    vals: &std::collections::BTreeMap<String, crate::expr::Value>,
    cj: &crate::CapturedJoint,
) -> Result<crate::CapturedJoint> {
    let mut out = cj.clone();
    if !cj.exprs.is_empty()
        && let Some(joint) = doc.assembly.joints.iter().find(|j| j.id == cj.joint)
    {
        let dofs = joint.kind.dofs();
        for (i, e) in cj.exprs.iter().enumerate().take(dofs.len()) {
            let Some(e) = e.as_deref() else { continue };
            let kind = if joint.kind.is_angle(i) { crate::expr::Kind::Angle } else { crate::expr::Kind::Length };
            let v = crate::eval::val(vals, e, kind)?;
            if !v.is_finite() || v.abs() >= 1e9 {
                return Err(DocError::Invalid(format!("captured joint {} expression `{e}` is out of range", joint.name)));
            }
            if out.values.len() <= i {
                out.values.resize(i + 1, 0.0);
            }
            if let Some(slot) = out.values.get_mut(i) {
                *slot = v;
            }
        }
    }
    if out.values.iter().any(|v| !v.is_finite() || v.abs() >= 1e9) {
        return Err(DocError::Invalid(format!("captured joint {} value is out of range", cj.joint)));
    }
    Ok(out)
}

/// The names of a design's placed bodies ([`Document::world_body_name`]), with what naming one
/// needs from the whole design worked out once: the occurrences by id, by name and by
/// component, the model's body names, and whether any name could alias a label or fallback key.
/// Naming each body of an assembly is then as cheap as its occurrence path, not a scan of the
/// whole design.
pub struct WorldNames<'a> {
    /// The first occurrence with each id.
    by_id: std::collections::HashMap<u64, &'a crate::Occurrence>,
    /// The occurrences with each name, in document order.
    by_name: std::collections::HashMap<&'a str, Vec<&'a crate::Occurrence>>,
    /// Each component's first occurrence ([`Document::occurrence_of`]).
    first_of: std::collections::HashMap<u64, &'a crate::Occurrence>,
    bodies: std::collections::HashSet<&'a str>,
    /// Some occurrence or body name has a slash or the fallback key's marker.
    reserved: bool,
}

impl<'a> WorldNames<'a> {
    pub fn new(doc: &'a Document, st: &'a crate::ModelState) -> WorldNames<'a> {
        let mut by_id = std::collections::HashMap::with_capacity(doc.occurrences.len());
        let mut by_name: std::collections::HashMap<&str, Vec<&crate::Occurrence>> = std::collections::HashMap::new();
        let mut first_of = std::collections::HashMap::new();
        for o in &doc.occurrences {
            by_id.entry(o.id).or_insert(o);
            by_name.entry(o.name.as_str()).or_default().push(o);
            first_of.entry(o.component).or_insert(o);
        }
        let reserved =
            doc.occurrences.iter().any(|o| o.name.contains('/') || o.name.contains("@occ[")) || st.bodies.iter().any(|b| b.name.contains("@occ["));
        let bodies = st.bodies.iter().map(|b| b.name.as_str()).collect();
        WorldNames { by_id, by_name, first_of, bodies, reserved }
    }

    /// The world name of the body `source` of `component` placed through `path`.
    pub fn name(&self, source: &str, component: u64, path: &[u64]) -> String {
        placed_name(self, source, component, path)
    }
}

/// What naming a placed body looks up in the design: by scanning it ([`Scan`], for one name)
/// or through its indexes ([`WorldNames`], for many).
trait NameLookup {
    fn occurrence(&self, id: u64) -> Option<&crate::Occurrence>;
    fn each_named(&self, name: &str, f: &mut dyn FnMut(&crate::Occurrence));
    fn first_of(&self, component: u64) -> Option<&crate::Occurrence>;
    fn reserved(&self) -> bool;
    fn has_body(&self, name: &str) -> bool;
}

/// Lookups by scanning the design.
struct Scan<'a> {
    doc: &'a Document,
    st: &'a crate::ModelState,
}

impl NameLookup for Scan<'_> {
    fn occurrence(&self, id: u64) -> Option<&crate::Occurrence> {
        self.doc.occurrences.iter().find(|o| o.id == id)
    }
    fn each_named(&self, name: &str, f: &mut dyn FnMut(&crate::Occurrence)) {
        self.doc.occurrences.iter().filter(|o| o.name == name).for_each(f);
    }
    fn first_of(&self, component: u64) -> Option<&crate::Occurrence> {
        self.doc.occurrence_of(component)
    }
    fn reserved(&self) -> bool {
        self.doc.occurrences.iter().any(|o| o.name.contains('/') || o.name.contains("@occ["))
            || self.st.bodies.iter().any(|b| b.name.contains("@occ["))
    }
    fn has_body(&self, name: &str) -> bool {
        self.st.body(name).is_some()
    }
}

impl NameLookup for WorldNames<'_> {
    fn occurrence(&self, id: u64) -> Option<&crate::Occurrence> {
        self.by_id.get(&id).copied()
    }
    fn each_named(&self, name: &str, f: &mut dyn FnMut(&crate::Occurrence)) {
        self.by_name.get(name).into_iter().flatten().copied().for_each(f);
    }
    fn first_of(&self, component: u64) -> Option<&crate::Occurrence> {
        self.first_of.get(&component).copied()
    }
    fn reserved(&self) -> bool {
        self.reserved
    }
    fn has_body(&self, name: &str) -> bool {
        self.bodies.contains(name)
    }
}

/// The world name of the body `source` of `component` placed through `path` (see
/// [`Document::world_body_name`]).
fn placed_name(l: &impl NameLookup, source: &str, component: u64, path: &[u64]) -> String {
    // The occurrence path of the component's first placement ([`Document::first_path`]).
    let mut first = Vec::new();
    let mut c = component;
    for _ in 0..1000 {
        if c == 0 {
            break;
        }
        let Some(o) = l.first_of(c) else { break };
        first.insert(0, o.id);
        c = o.parent;
    }
    if path == first {
        return source.to_string();
    }
    let names: Vec<&str> = path.iter().filter_map(|id| l.occurrence(*id)).map(|o| o.name.as_str()).collect();
    let label = names.join("/");
    let candidate = format!("{source} ({label})");
    let mut counts = std::collections::BTreeMap::from([(0, 1usize)]);
    for name in &names {
        let mut next = std::collections::BTreeMap::<u64, usize>::new();
        l.each_named(name, &mut |o| {
            if let Some(count) = counts.get(&o.parent) {
                let n = next.entry(o.component).or_default();
                *n = (*n + count).min(2);
            }
        });
        counts = next;
    }
    let ambiguous = counts.get(&component).copied().unwrap_or(0) > 1;
    // Slash and reserved-marker names could alias concatenated labels or fallback keys.
    if !ambiguous && !l.reserved() && !l.has_body(&candidate) {
        return candidate;
    }
    let ids = path.iter().map(u64::to_string).collect::<Vec<_>>().join("/");
    let encoded = serde_json::to_string(source).unwrap_or_else(|_| source.to_string());
    let mut key = format!("{encoded}@occ[{ids}]");
    while l.has_body(&key) {
        key.push('#');
    }
    key
}
