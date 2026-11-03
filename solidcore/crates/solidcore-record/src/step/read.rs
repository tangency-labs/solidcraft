//! STEP curve entities (ISO 10303-42) to the plain data of [`super::geom`], from a parsed
//! [`Exchange`] (ISO 10303-21) in one representation context's [`Units`]. Surfaces, surface
//! curves and pcurves follow in the next slice (#2578, slice 2c).
//!
//! Input is hostile: every count, number and reference is checked, nesting is capped
//! ([`MAX_GEOM_DEPTH`]), counts are capped ([`MAX_CTRL`], [`MAX_DEGREE`]) and checked before
//! anything is expanded, numbers must be finite and below [`MAX_COORD`], nothing is
//! pre-allocated from a count in the file, shared entities are read once, and every error is a
//! message, never a panic.
//!
//! The only arithmetic is unit conversion (a length read times the length unit, an angle times
//! the plane angle unit) and the knot counting of [`BSplineCurve`]s given without knots.
//! Directions keep the ratios the file writes; checks that would need a geometric quantity are
//! the kernel's when it builds from this data (#2578, slice 4), and the checks kept here are on
//! the data alone: a direction that is exactly zero in every component, a vector of magnitude
//! zero, a radius, magnitude or weight of the wrong sign, a knot vector that does not fit its
//! control points.
//!
//! Origin: SolidCraft's `crates/step/src/read.rs` at `a5b71ca2` (MIT OR Apache-2.0), its
//! points, directions, placements, vectors and curves.
//!
//! Deviations from SolidCraft, each without a geometric computation in its place (the kernel
//! takes those over in slice 4):
//! - Directions are not normalised (was `unit`): a direction is refused when every ratio is
//!   exactly 0 (was: when its length is at most 1e-12), and is the ratios as written.
//! - A placement keeps `axis` and `ref_direction` as given, each optional (was: the reference
//!   direction made orthogonal to the axis, or `any_perpendicular` when it is absent or
//!   parallel; neither refused a file).
//! - A `VECTOR` keeps its orientation and magnitude and the space's unit factors (was: a unit
//!   direction and the scaled length); it is refused when its magnitude is negative or zero (was:
//!   negative, or a scaled length that is not positive).
//! - Uniform implied knots are counted in integers before conversion (`i - degree`; was the
//!   same difference in `f64`, exact for every count below 2^53).
//! - The depth bound is checked in the one function every curve read goes through.
//! - The cache is a `BTreeMap` (was a `HashMap`); nothing iterates it. Collections grow as they
//!   are read (no capacity from a count in the file). [`Units`] is `super::units::Units`.
//!   Rustfmt, clippy-driven rewrites that keep the result, and doc comments on every public
//!   item.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::geom::{
    BSplineCurve, Curve, CurveForm, KnotSpec, Logical, P3, Placement, Trim, TrimPreference, Vector,
};
use super::p21::{Entity, Exchange, Param};
use super::units::Units;

/// A result whose error is a message.
pub type R<T> = Result<T, String>;

/// Geometry nesting we follow (a trimmed curve of a trimmed curve of …). Every curve read
/// checks it, so a reference cycle ends here.
pub const MAX_GEOM_DEPTH: usize = 12;
/// Most control points in one B-spline (the whole net for a surface).
pub const MAX_CTRL: usize = 1_000_000;
/// Largest coordinate or length, in file units.
pub const MAX_COORD: f64 = 1e9;
/// Highest B-spline degree.
pub const MAX_DEGREE: usize = 32;

/// How a trim parameter scales from file units: a conic's angles by the angle unit, anything
/// else (B-spline knots, a line's parameter) not at all.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ParamKind {
    Angle,
    Plain,
}

/// The space an entity is read in: the unit factors of its coordinates and of its direction
/// ratios, and its dimension (2 for an `AXIS2_PLACEMENT_2D` read as a frame).
#[derive(Clone, Copy, Debug)]
struct Space {
    /// Factor per coordinate of a point (and of a vector's components).
    pts: [f64; 3],
    /// Factor per direction ratio: 1 in model space.
    dirs: [f64; 3],
    /// 3 for model space, 2 for a 2D placement.
    dim: usize,
}

impl Space {
    /// Model space in the length unit `l`.
    fn model(l: f64, dim: usize) -> Self {
        Space {
            pts: [l, l, l],
            dirs: [1.0; 3],
            dim,
        }
    }
}

/// Reads geometry from one exchange structure, in one context's units.
pub struct Reader<'a> {
    ex: &'a Exchange,
    units: Units,
    curves: BTreeMap<u64, Arc<Curve>>,
}

fn entity(ex: &Exchange, id: u64) -> R<&Entity> {
    ex.get(id).ok_or_else(|| format!("missing entity #{id}"))
}

fn reference(p: Option<&Param>, what: &str) -> R<u64> {
    p.and_then(Param::as_ref_id)
        .ok_or_else(|| format!("{what}: expected a reference"))
}

fn num(p: Option<&Param>, what: &str) -> R<f64> {
    let x = p
        .and_then(Param::as_f64)
        .ok_or_else(|| format!("{what}: expected a number"))?;
    if x.is_finite() && x.abs() < MAX_COORD {
        Ok(x)
    } else {
        Err(format!("{what}: {x} out of range"))
    }
}

fn logical(p: Option<&Param>, what: &str) -> R<Logical> {
    match p.and_then(Param::as_enum) {
        Some("T") => Ok(Logical::True),
        Some("F") => Ok(Logical::False),
        Some("U") => Ok(Logical::Unknown),
        _ => Err(format!("{what}: expected a logical")),
    }
}

fn boolean(p: Option<&Param>, what: &str) -> R<bool> {
    p.and_then(Param::as_bool)
        .ok_or_else(|| format!("{what}: expected a boolean"))
}

fn positive(x: f64, what: &str) -> R<f64> {
    if x > 0.0 {
        Ok(x)
    } else {
        Err(format!("{what} must be positive"))
    }
}

fn curve_form(p: Option<&Param>) -> CurveForm {
    match p.and_then(Param::as_enum) {
        Some("POLYLINE_FORM") => CurveForm::PolylineForm,
        Some("CIRCULAR_ARC") => CurveForm::CircularArc,
        Some("ELLIPTIC_ARC") => CurveForm::EllipticArc,
        Some("PARABOLIC_ARC") => CurveForm::ParabolicArc,
        Some("HYPERBOLIC_ARC") => CurveForm::HyperbolicArc,
        _ => CurveForm::Unspecified,
    }
}

fn knot_spec(p: Option<&Param>) -> KnotSpec {
    match p.and_then(Param::as_enum) {
        Some("UNIFORM_KNOTS") => KnotSpec::UniformKnots,
        Some("QUASI_UNIFORM_KNOTS") => KnotSpec::QuasiUniformKnots,
        Some("PIECEWISE_BEZIER_KNOTS") => KnotSpec::PiecewiseBezierKnots,
        _ => KnotSpec::Unspecified,
    }
}

/// Distinct knots and multiplicities, checked against `n_ctrl` points of `degree` before
/// anything is expanded (a file can claim multiplicities that would need gigabytes).
fn knots(
    mults: Option<&Param>,
    values: Option<&Param>,
    n_ctrl: usize,
    degree: usize,
) -> R<(Vec<f64>, Vec<usize>)> {
    let m: Vec<usize> = mults
        .and_then(Param::as_list)
        .ok_or("B-spline without knot multiplicities")?
        .iter()
        .map(|x| {
            x.as_i64()
                .filter(|v| *v >= 1 && *v as usize <= degree + 1)
                .map(|v| v as usize)
                .ok_or_else(|| "bad knot multiplicity".to_string())
        })
        .collect::<R<_>>()?;
    let raw: Vec<f64> = values
        .and_then(Param::as_list)
        .ok_or("B-spline without knots")?
        .iter()
        .map(|x| num(Some(x), "knot"))
        .collect::<R<_>>()?;
    if m.len() != raw.len() || raw.windows(2).any(|w| w.first() > w.get(1)) {
        return Err("knots must not decrease, one multiplicity each".into());
    }
    // Some writers list a knot twice; that is one knot with both multiplicities.
    let (mut k, mut mm): (Vec<f64>, Vec<usize>) = (Vec::new(), Vec::new());
    for (x, n) in raw.into_iter().zip(m) {
        match (k.last(), mm.last_mut()) {
            (Some(last), Some(lm)) if *last == x => *lm += n,
            _ => {
                k.push(x);
                mm.push(n);
            }
        }
    }
    let m = mm;
    if k.len() < 2 || m.iter().any(|v| *v > degree + 1) {
        return Err("bad knot vector".into());
    }
    if m.iter().try_fold(0usize, |a, v| a.checked_add(*v)) != Some(n_ctrl + degree + 1) {
        return Err("knot multiplicities do not match the control points".into());
    }
    Ok((k, m))
}

/// Knots of a B-spline given by its type alone (`BEZIER_`, `UNIFORM_`, `QUASI_UNIFORM_CURVE`):
/// the knot values are whole numbers and the multiplicities counts, laid out from the number of
/// control points and the degree (integer arithmetic only).
fn implied_knots(spec: KnotSpec, n_ctrl: usize, degree: usize) -> R<(Vec<f64>, Vec<usize>)> {
    if n_ctrl <= degree {
        return Err("too few control points".into());
    }
    match spec {
        KnotSpec::PiecewiseBezierKnots => {
            // Segments of `degree`, full interior multiplicity.
            let segs = (n_ctrl - 1) / degree;
            if segs * degree + 1 != n_ctrl {
                return Err("Bezier control point count".into());
            }
            let k = (0..=segs).map(|s| s as f64).collect();
            let mut m = vec![degree; segs + 1];
            if let Some(f) = m.first_mut() {
                *f = degree + 1;
            }
            if let Some(l) = m.last_mut() {
                *l = degree + 1;
            }
            Ok((k, m))
        }
        KnotSpec::UniformKnots => {
            let n = n_ctrl + degree + 1;
            // n ≤ MAX_CTRL + MAX_DEGREE + 1, so the counts fit in i64 and convert exactly.
            let k = (0..n).map(|i| (i as i64 - degree as i64) as f64).collect();
            Ok((k, vec![1; n]))
        }
        KnotSpec::QuasiUniformKnots | KnotSpec::Unspecified => {
            // Quasi-uniform: clamped ends, uniform interior.
            let inner = n_ctrl - degree;
            let k = (0..=inner).map(|i| i as f64).collect();
            let mut m = vec![1; inner + 1];
            if let Some(f) = m.first_mut() {
                *f = degree + 1;
            }
            if let Some(l) = m.last_mut() {
                *l = degree + 1;
            }
            Ok((k, m))
        }
    }
}

fn weights(p: Option<&Param>, n: usize) -> R<Vec<f64>> {
    let w: Vec<f64> = p
        .and_then(Param::as_list)
        .ok_or("rational B-spline without weights")?
        .iter()
        .map(|w| {
            num(Some(w), "weight").and_then(|w| {
                if w > 0.0 {
                    Ok(w)
                } else {
                    Err("non-positive weight".into())
                }
            })
        })
        .collect::<R<_>>()?;
    if w.len() == n {
        Ok(w)
    } else {
        Err("weights do not match the control points".into())
    }
}

fn degree(p: Option<&Param>) -> R<usize> {
    p.and_then(Param::as_i64)
        .filter(|d| (1..=MAX_DEGREE as i64).contains(d))
        .map(|d| d as usize)
        .ok_or_else(|| "bad B-spline degree".into())
}

/// The knot type a B-spline entity's (sub)type implies when it lists no knots.
fn spec_of_type(e: &Entity) -> KnotSpec {
    let has = |n: &str| e.name() == n || e.has(n);
    if has("BEZIER_CURVE") || has("BEZIER_SURFACE") {
        KnotSpec::PiecewiseBezierKnots
    } else if has("UNIFORM_CURVE") || has("UNIFORM_SURFACE") {
        KnotSpec::UniformKnots
    } else {
        KnotSpec::QuasiUniformKnots
    }
}

/// The parameters of a simple or complex B-spline entity, as the simple form lays them out:
/// the name first, then the `B_SPLINE_*` attributes, then the `*_WITH_KNOTS` ones.
fn bspline_params(e: &Entity, base: &str, with_knots: &str) -> R<(Vec<Param>, Option<Vec<Param>>)> {
    if e.is_complex() {
        let b = e
            .record(base)
            .ok_or_else(|| format!("complex entity without {base}"))?;
        let mut p = vec![Param::Str(String::new())];
        p.extend(b.iter().cloned());
        Ok((p, e.record(with_knots).map(<[Param]>::to_vec)))
    } else {
        let p = e.params();
        let n = if base == "B_SPLINE_CURVE" { 6 } else { 8 };
        let k =
            (e.name() == with_knots).then(|| p.get(n..).map(<[Param]>::to_vec).unwrap_or_default());
        Ok((p.to_vec(), k))
    }
}

impl<'a> Reader<'a> {
    /// A reader of `ex` whose lengths and angles are in `units`.
    pub fn new(ex: &'a Exchange, units: Units) -> Self {
        Reader {
            ex,
            units,
            curves: BTreeMap::new(),
        }
    }

    /// The units the reader converts from.
    pub fn units(&self) -> Units {
        self.units
    }

    fn length(&self, p: Option<&Param>, what: &str) -> R<f64> {
        Ok(num(p, what)? * self.units.len)
    }

    /// A `CARTESIAN_POINT`, each coordinate times its factor in `sp`.
    fn point_in(&self, id: u64, sp: Space) -> R<P3> {
        let e = entity(self.ex, id)?;
        if e.name() != "CARTESIAN_POINT" {
            return Err(format!(
                "#{id}: expected CARTESIAN_POINT, found {}",
                e.name()
            ));
        }
        let c = e
            .params()
            .get(1)
            .and_then(Param::as_list)
            .ok_or("CARTESIAN_POINT without coordinates")?;
        if c.is_empty() || c.len() > sp.dim {
            return Err(format!("#{id}: point with {} coordinates", c.len()));
        }
        let g = |i: usize| -> R<f64> {
            if i < c.len() {
                num(c.get(i), "coordinate")
            } else {
                Ok(0.0)
            }
        };
        let [sx, sy, sz] = sp.pts;
        Ok([
            g(0)? * sx,
            g(1)? * sy,
            if sp.dim == 3 { g(2)? * sz } else { 0.0 },
        ])
    }

    /// A `CARTESIAN_POINT` in millimetres.
    pub fn point(&self, id: u64) -> R<P3> {
        self.point_in(id, Space::model(self.units.len, 3))
    }

    /// The ratios of a `DIRECTION`, each times `s`. Refused when every ratio is exactly zero:
    /// that needs no arithmetic. A direction too short to normalise is the kernel's to refuse.
    fn ratios(&self, id: u64, s: [f64; 3], dim: usize) -> R<P3> {
        let e = entity(self.ex, id)?;
        if e.name() != "DIRECTION" {
            return Err(format!("#{id}: expected DIRECTION, found {}", e.name()));
        }
        let c = e
            .params()
            .get(1)
            .and_then(Param::as_list)
            .ok_or("DIRECTION without ratios")?;
        if c.is_empty() || c.len() > dim {
            return Err(format!("#{id}: direction with {} ratios", c.len()));
        }
        let g = |i: usize| -> R<f64> {
            if i < c.len() {
                num(c.get(i), "direction ratio")
            } else {
                Ok(0.0)
            }
        };
        let [sx, sy, sz] = s;
        let d = [
            g(0)? * sx,
            g(1)? * sy,
            if dim == 3 { g(2)? * sz } else { 0.0 },
        ];
        if d == [0.0; 3] {
            return Err(format!("#{id}: zero direction"));
        }
        Ok(d)
    }

    /// A `DIRECTION` in model space: its ratios as written (not normalised).
    pub fn direction(&self, id: u64) -> R<P3> {
        self.ratios(id, [1.0; 3], 3)
    }

    /// `AXIS2_PLACEMENT_3D` in 3D, or `AXIS2_PLACEMENT_2D` (no axis) in 2D.
    fn placement_in(&self, id: u64, sp: Space) -> R<Placement> {
        let e = entity(self.ex, id)?;
        let p = e.params();
        let origin = self.point_in(reference(p.get(1), "placement location")?, sp)?;
        let (axis, rf) = match (e.name(), sp.dim) {
            ("AXIS2_PLACEMENT_3D", 3) => {
                let z = match p.get(2).and_then(Param::as_ref_id) {
                    Some(d) => Some(self.ratios(d, sp.dirs, sp.dim)?),
                    None => None,
                };
                (z, p.get(3).and_then(Param::as_ref_id))
            }
            ("AXIS2_PLACEMENT_2D", 2) => (None, p.get(2).and_then(Param::as_ref_id)),
            (n, _) => return Err(format!("#{id}: unexpected placement {n} in {}D", sp.dim)),
        };
        let ref_direction = match rf {
            Some(d) => Some(self.ratios(d, sp.dirs, sp.dim)?),
            None => None,
        };
        Ok(Placement {
            origin,
            axis,
            ref_direction,
        })
    }

    /// An `AXIS2_PLACEMENT_3D` in millimetres.
    pub fn placement(&self, id: u64) -> R<Placement> {
        self.placement_in(id, Space::model(self.units.len, 3))
    }

    /// A placement in 3D space (an assembly's or a mapped item's): `AXIS2_PLACEMENT_3D`, or an
    /// `AXIS2_PLACEMENT_2D` as the placement in the XY plane it describes.
    pub fn frame(&self, id: u64) -> R<Placement> {
        let dim = if entity(self.ex, id)?.name() == "AXIS2_PLACEMENT_2D" {
            2
        } else {
            3
        };
        self.placement_in(id, Space::model(self.units.len, dim))
    }

    /// A `VECTOR`: its orientation's ratios and its magnitude as written, with the factors of
    /// `sp`. Refused when the magnitude is negative or zero.
    fn vector_in(&self, id: u64, sp: Space) -> R<Vector> {
        let e = entity(self.ex, id)?;
        if e.name() != "VECTOR" {
            return Err(format!("#{id}: expected VECTOR, found {}", e.name()));
        }
        let p = e.params();
        let orientation =
            self.ratios(reference(p.get(1), "vector orientation")?, [1.0; 3], sp.dim)?;
        let magnitude = num(p.get(2), "vector magnitude")?;
        if magnitude < 0.0 {
            return Err("negative vector magnitude".into());
        }
        if magnitude == 0.0 {
            return Err("zero vector".into());
        }
        Ok(Vector {
            orientation,
            magnitude,
            scale: sp.pts,
        })
    }

    /// A 3D curve entity.
    pub fn curve(&mut self, id: u64) -> R<Arc<Curve>> {
        self.curve_in(id, 0)
    }

    fn curve_in(&mut self, id: u64, depth: usize) -> R<Arc<Curve>> {
        if let Some(c) = self.curves.get(&id) {
            return Ok(c.clone());
        }
        let c = Arc::new(self.read_curve(id, depth)?);
        self.curves.insert(id, c.clone());
        Ok(c)
    }

    fn factor(&self, k: ParamKind) -> f64 {
        match k {
            ParamKind::Angle => self.units.ang,
            ParamKind::Plain => 1.0,
        }
    }

    fn read_curve(&mut self, id: u64, depth: usize) -> R<Curve> {
        if depth > MAX_GEOM_DEPTH {
            return Err("curve definitions nested too deeply".into());
        }
        let sp = Space::model(self.units.len, 3);
        let e = entity(self.ex, id)?;
        if e.is_complex() {
            return self.bspline_curve(e, sp);
        }
        let p = e.params();
        match e.name() {
            "LINE" => {
                let origin = self.point_in(reference(p.get(1), "line point")?, sp)?;
                let vector = self.vector_in(reference(p.get(2), "line vector")?, sp)?;
                Ok(Curve::Line { origin, vector })
            }
            "CIRCLE" => {
                let k = self.units.len;
                let position = self.placement_in(reference(p.get(1), "circle placement")?, sp)?;
                Ok(Curve::Circle {
                    position,
                    radius: positive(num(p.get(2), "radius")? * k, "circle radius")?,
                })
            }
            "ELLIPSE" => {
                let k = self.units.len;
                let position = self.placement_in(reference(p.get(1), "ellipse placement")?, sp)?;
                Ok(Curve::Ellipse {
                    position,
                    semi_axis_1: positive(num(p.get(2), "semi axis")? * k, "ellipse axis")?,
                    semi_axis_2: positive(num(p.get(3), "semi axis")? * k, "ellipse axis")?,
                })
            }
            "HYPERBOLA" => {
                let k = self.units.len;
                let position =
                    self.placement_in(reference(p.get(1), "hyperbola placement")?, sp)?;
                Ok(Curve::Hyperbola {
                    position,
                    semi_axis: positive(num(p.get(2), "semi axis")? * k, "hyperbola axis")?,
                    semi_imag_axis: positive(
                        num(p.get(3), "semi imaginary axis")? * k,
                        "hyperbola axis",
                    )?,
                })
            }
            "PARABOLA" => {
                let k = self.units.len;
                let position = self.placement_in(reference(p.get(1), "parabola placement")?, sp)?;
                let focal_dist = num(p.get(2), "focal distance")? * k;
                if focal_dist == 0.0 {
                    return Err("parabola focal distance must not be zero".into());
                }
                Ok(Curve::Parabola {
                    position,
                    focal_dist,
                })
            }
            "B_SPLINE_CURVE_WITH_KNOTS"
            | "BEZIER_CURVE"
            | "UNIFORM_CURVE"
            | "QUASI_UNIFORM_CURVE" => self.bspline_curve(e, sp),
            "POLYLINE" => {
                let ids = p
                    .get(1)
                    .and_then(Param::as_list)
                    .ok_or("POLYLINE without points")?;
                if ids.len() < 2 || ids.len() > MAX_CTRL {
                    return Err("polyline point count".into());
                }
                let pts = ids
                    .iter()
                    .map(|c| self.point_in(reference(Some(c), "polyline point")?, sp))
                    .collect::<R<_>>()?;
                Ok(Curve::Polyline(pts))
            }
            "TRIMMED_CURVE" => {
                let b = reference(p.get(1), "trimmed curve basis")?;
                let basis = self.curve_in(b, depth + 1)?;
                // Conic parameters are angles; every other basis curve's are plain numbers.
                let kind = match &*basis {
                    Curve::Circle { .. } | Curve::Ellipse { .. } => ParamKind::Angle,
                    _ => ParamKind::Plain,
                };
                let trim_1 = self.trims(p.get(2), kind, sp)?;
                let trim_2 = self.trims(p.get(3), kind, sp)?;
                let sense = boolean(p.get(4), "trimmed curve sense")?;
                let preference = match p.get(5).and_then(Param::as_enum) {
                    Some("CARTESIAN") => TrimPreference::Cartesian,
                    Some("PARAMETER") => TrimPreference::Parameter,
                    _ => TrimPreference::Unspecified,
                };
                Ok(Curve::Trimmed {
                    basis,
                    trim_1,
                    trim_2,
                    sense,
                    preference,
                })
            }
            "OFFSET_CURVE_3D" => {
                let basis = self.curve_in(reference(p.get(1), "offset curve basis")?, depth + 1)?;
                let distance = self.length(p.get(2), "offset distance")?;
                let self_intersect = logical(p.get(3), "offset curve self_intersect")?;
                let ref_direction =
                    self.direction(reference(p.get(4), "offset curve direction")?)?;
                Ok(Curve::OffsetCurve3d {
                    basis,
                    distance,
                    self_intersect,
                    ref_direction,
                })
            }
            n => Err(format!("not supported yet: curve {n}")),
        }
    }

    fn trims(&self, p: Option<&Param>, kind: ParamKind, sp: Space) -> R<Vec<Trim>> {
        let items = p
            .and_then(Param::as_list)
            .ok_or("trimmed curve without trims")?;
        if items.is_empty() || items.len() > 2 {
            return Err("trimmed curve trim count".into());
        }
        items
            .iter()
            .map(|t| match t {
                Param::Ref(r) => Ok(Trim::Point(self.point_in(*r, sp)?)),
                Param::Typed(name, v) if name == "PARAMETER_VALUE" => Ok(Trim::Parameter(
                    num(v.first(), "trim parameter")? * self.factor(kind),
                )),
                Param::Real(_) | Param::Int(_) => Ok(Trim::Parameter(
                    num(Some(t), "trim parameter")? * self.factor(kind),
                )),
                _ => Err("bad trim".into()),
            })
            .collect()
    }

    fn bspline_curve(&mut self, e: &Entity, sp: Space) -> R<Curve> {
        let (base, with_knots) = bspline_params(e, "B_SPLINE_CURVE", "B_SPLINE_CURVE_WITH_KNOTS")?;
        let degree = degree(base.get(1))?;
        let ids = base
            .get(2)
            .and_then(Param::as_list)
            .ok_or("B-spline without control points")?;
        if ids.len() > MAX_CTRL {
            return Err("B-spline with too many control points".into());
        }
        let points: Vec<P3> = ids
            .iter()
            .map(|c| self.point_in(reference(Some(c), "control point")?, sp))
            .collect::<R<_>>()?;
        let (knots, multiplicities, knot_spec) = if let Some(k) = &with_knots {
            let (kv, m) = knots(k.first(), k.get(1), points.len(), degree)?;
            (kv, m, knot_spec(k.get(2)))
        } else {
            let spec = spec_of_type(e);
            let (kv, m) = implied_knots(spec, points.len(), degree)?;
            (kv, m, spec)
        };
        let weights = match e.record("RATIONAL_B_SPLINE_CURVE") {
            Some(r) => Some(weights(r.first(), points.len())?),
            None => None,
        };
        Ok(Curve::BSpline(Box::new(BSplineCurve {
            degree,
            points,
            weights,
            knots,
            multiplicities,
            form: curve_form(base.get(3)),
            closed: logical(base.get(4), "B-spline closed_curve").unwrap_or(Logical::Unknown),
            self_intersect: logical(base.get(5), "B-spline self_intersect")
                .unwrap_or(Logical::Unknown),
            knot_spec,
        })))
    }
}

#[cfg(test)]
mod tests;
