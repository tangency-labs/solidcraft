//! Maps the AP203/AP214 B-rep subset of a parsed file to the neutral [`Model`].
//!
//! Written from the ISO 10303-42 (geometry and topology) and ISO 10303-41 (units)
//! entity definitions as used by AP203 and AP214. Supported: `MANIFOLD_SOLID_BREP`,
//! `BREP_WITH_VOIDS`, `FACETED_BREP`, `SHELL_BASED_SURFACE_MODEL`; closed, open and
//! oriented shells; `ADVANCED_FACE`, `FACE_SURFACE`, `FACE` (planar polygon) and
//! `ORIENTED_FACE`; edge, vertex and poly loops; the curves and surfaces of
//! [`crate::model`]; `SURFACE_CURVE`, `SEAM_CURVE`, `INTERSECTION_CURVE` and
//! `TRIMMED_CURVE` as edge geometry (their 3D / basis curve); SI and conversion-based
//! length and plane-angle units. Anything else reachable from a body makes that body
//! [`Skipped`] with `unsupported: <ENTITY>`; it never panics.
//!
//! Every reference is checked to exist, reference chains are bounded by
//! [`Limits::max_ref_depth`], and no allocation is sized from a value in the file.

use std::collections::BTreeMap;
use std::fmt;

use crate::geom::{self, build_frame, conic_angle, dist, unit};
use crate::model::{
    BSplineCurve, BSplineSurface, Body, BodyKind, Bound, Curve, Diagnostic, Edge, Face, Frame,
    Loop, Model, OrientedEdge, Shell, Skipped, Surface, Vertex, P3,
};
use crate::part21::{Instance, Limits, Param, Record, StepFile};

/// Why a body could not be mapped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapError {
    /// An entity outside the supported subset.
    Unsupported(String),
    /// Malformed data (dangling reference, wrong parameter, bad B-spline, ...).
    Invalid(String),
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapError::Unsupported(e) => write!(f, "unsupported: {e}"),
            MapError::Invalid(e) => write!(f, "invalid: {e}"),
        }
    }
}

type R<T> = Result<T, MapError>;

fn invalid<T>(msg: impl Into<String>) -> R<T> {
    Err(MapError::Invalid(msg.into()))
}

/// Units of one representation context.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Units {
    /// Millimetres per length unit.
    mm: f64,
    /// Degrees per plane-angle unit.
    deg: f64,
    /// Distance uncertainty in mm.
    uncertainty: Option<f64>,
}

impl Default for Units {
    fn default() -> Self {
        Self {
            mm: 1.0,
            deg: 180.0 / std::f64::consts::PI,
            uncertainty: None,
        }
    }
}

const BODY_NAMES: [(&str, BodyKind); 4] = [
    ("MANIFOLD_SOLID_BREP", BodyKind::ManifoldSolid),
    ("BREP_WITH_VOIDS", BodyKind::SolidWithVoids),
    ("FACETED_BREP", BodyKind::Faceted),
    ("SHELL_BASED_SURFACE_MODEL", BodyKind::ShellModel),
];

struct Mapper<'a> {
    file: &'a StepFile,
    limits: Limits,
}

/// Per-body state.
struct Ctx {
    u: Units,
    vertices: Vec<Vertex>,
    edges: Vec<Edge>,
    faces: Vec<Face>,
    vmap: BTreeMap<u64, usize>,
    emap: BTreeMap<u64, usize>,
    /// Edges made from poly-loop segments, by their point ids (low, high).
    pmap: BTreeMap<(u64, u64), usize>,
}

impl<'a> Mapper<'a> {
    fn inst(&self, id: u64) -> R<&'a Instance> {
        self.file
            .entities
            .get(&id)
            .map_or_else(|| invalid(format!("dangling reference #{id}")), Ok)
    }

    /// The simple record of `id`, any name.
    fn simple(&self, id: u64) -> R<&'a Record> {
        match self.inst(id)? {
            Instance::Simple(r) => Ok(r),
            Instance::Complex(_) => Err(MapError::Unsupported(format!(
                "complex instance #{id} ({})",
                self.inst(id)?.type_name()
            ))),
        }
    }

    fn check_depth(&self, depth: usize) -> R<()> {
        if depth > self.limits.max_ref_depth {
            return invalid("reference chain too deep (cycle?)");
        }
        Ok(())
    }

    fn point(&self, p: &Param, s: f64) -> R<P3> {
        let id = ref_of(p)?;
        let r = self.simple(id)?;
        if r.name != "CARTESIAN_POINT" {
            return Err(MapError::Unsupported(r.name.clone()));
        }
        let c = list(param(r, 1)?)?;
        let x = num_at(c, 0)?;
        let y = num_at(c, 1)?;
        let z = if c.len() > 2 { num_at(c, 2)? } else { 0.0 };
        Ok([x * s, y * s, z * s])
    }

    fn direction(&self, p: &Param) -> R<P3> {
        let r = self.simple(ref_of(p)?)?;
        if r.name != "DIRECTION" {
            return Err(MapError::Unsupported(r.name.clone()));
        }
        let c = list(param(r, 1)?)?;
        let z = if c.len() > 2 { num_at(c, 2)? } else { 0.0 };
        Ok([num_at(c, 0)?, num_at(c, 1)?, z])
    }

    /// `VECTOR(name, orientation, magnitude)` as (unit direction, magnitude in mm).
    fn vector(&self, p: &Param, s: f64) -> R<(P3, f64)> {
        let r = self.simple(ref_of(p)?)?;
        if r.name != "VECTOR" {
            return Err(MapError::Unsupported(r.name.clone()));
        }
        let d =
            unit(self.direction(param(r, 1)?)?).map_or_else(|| invalid("zero direction"), Ok)?;
        let m = num(param(r, 2)?)? * s;
        Ok((d, m))
    }

    fn placement(&self, p: &Param, s: f64) -> R<Frame> {
        let r = self.simple(ref_of(p)?)?;
        if r.name != "AXIS2_PLACEMENT_3D" {
            return Err(MapError::Unsupported(r.name.clone()));
        }
        let o = self.point(param(r, 1)?, s)?;
        let axis = match param(r, 2)? {
            Param::Null => None,
            q => Some(self.direction(q)?),
        };
        let rd = match r.params.get(3) {
            None | Some(Param::Null) => None,
            Some(q) => Some(self.direction(q)?),
        };
        build_frame(o, axis, rd).map_or_else(|| invalid("degenerate AXIS2_PLACEMENT_3D"), Ok)
    }

    // ---- units ---------------------------------------------------------------------

    /// (kind, factor): kind `L` length (mm per unit), `A` plane angle (degrees per
    /// unit), or other.
    fn unit_factor(&self, id: u64, depth: usize) -> R<(char, f64)> {
        self.check_depth(depth)?;
        let inst = self.inst(id)?;
        let kind = if inst.record("LENGTH_UNIT").is_some() {
            'L'
        } else if inst.record("PLANE_ANGLE_UNIT").is_some() {
            'A'
        } else {
            return Ok(('?', 1.0));
        };
        if let Some(si) = inst.record("SI_UNIT") {
            let prefix = match si.params.first() {
                Some(Param::Enum(e)) => Some(e.as_str()),
                _ => None,
            };
            let f = match prefix {
                None => 1.0,
                Some("EXA") => 1e18,
                Some("PETA") => 1e15,
                Some("TERA") => 1e12,
                Some("GIGA") => 1e9,
                Some("MEGA") => 1e6,
                Some("KILO") => 1e3,
                Some("HECTO") => 1e2,
                Some("DECA") => 1e1,
                Some("DECI") => 1e-1,
                Some("CENTI") => 1e-2,
                Some("MILLI") => 1e-3,
                Some("MICRO") => 1e-6,
                Some("NANO") => 1e-9,
                Some("PICO") => 1e-12,
                Some(p) => return Err(MapError::Unsupported(format!("SI prefix {p}"))),
            };
            return Ok(match kind {
                'L' => ('L', f * 1000.0),
                _ => ('A', f * 180.0 / std::f64::consts::PI),
            });
        }
        if let Some(cb) = inst.record("CONVERSION_BASED_UNIT") {
            let m = self.inst(ref_of(param(cb, 1)?)?)?;
            let mr = match m {
                Instance::Simple(r) => r,
                Instance::Complex(rs) => rs
                    .iter()
                    .find(|r| r.name == "MEASURE_WITH_UNIT")
                    .map_or_else(|| invalid("conversion factor"), Ok)?,
            };
            let v = num(param(mr, 0)?)?;
            let (k2, f2) = self.unit_factor(ref_of(param(mr, 1)?)?, depth + 1)?;
            if k2 != kind {
                return invalid("conversion unit of another kind");
            }
            return Ok((kind, v * f2));
        }
        Err(MapError::Unsupported(format!(
            "unit #{id} ({})",
            inst.type_name()
        )))
    }

    fn context_units(&self, id: u64) -> R<Units> {
        let ctx = self.inst(id)?;
        let mut u = Units::default();
        if let Some(g) = ctx.record("GLOBAL_UNIT_ASSIGNED_CONTEXT") {
            for p in list(param(g, 0)?)? {
                match self.unit_factor(ref_of(p)?, 0)? {
                    ('L', f) => u.mm = f,
                    ('A', f) => u.deg = f,
                    _ => {}
                }
            }
        }
        if let Some(g) = ctx.record("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT") {
            for p in list(param(g, 0)?)? {
                let r = self.simple(ref_of(p)?)?;
                if r.name != "UNCERTAINTY_MEASURE_WITH_UNIT" {
                    continue;
                }
                if let (Ok(v), Ok(uid)) = (num(param(r, 0)?), ref_of(param(r, 1)?)) {
                    if let ('L', f) = self.unit_factor(uid, 0)? {
                        u.uncertainty = Some(v * f);
                    }
                }
            }
        }
        if !(u.mm.is_finite() && u.mm > 0.0 && u.deg.is_finite() && u.deg > 0.0) {
            return invalid("bad unit factor");
        }
        Ok(u)
    }

    /// Representation item id -> context id, from every representation's items.
    fn item_contexts(&self) -> BTreeMap<u64, u64> {
        let mut out = BTreeMap::new();
        for inst in self.file.entities.values() {
            let rec = match inst {
                Instance::Simple(r) if r.name.ends_with("REPRESENTATION") => r,
                Instance::Complex(rs) => match rs.iter().find(|r| r.name == "REPRESENTATION") {
                    Some(r) => r,
                    None => continue,
                },
                Instance::Simple(_) => continue,
            };
            let (Some(Param::List(items)), Some(Param::Ref(ctx))) =
                (rec.params.get(1), rec.params.get(2))
            else {
                continue;
            };
            for it in items {
                if let Param::Ref(i) = it {
                    out.entry(*i).or_insert(*ctx);
                }
            }
        }
        out
    }

    // ---- curves --------------------------------------------------------------------

    /// An edge geometry: the curve and whether its direction is reversed relative to
    /// the referenced entity's own parameterisation (a `TRIMMED_CURVE` with
    /// `sense_agreement = .F.`).
    fn edge_curve_geom(&self, id: u64, c: &Ctx, depth: usize) -> R<(Curve, bool)> {
        self.check_depth(depth)?;
        let inst = self.inst(id)?;
        if let Instance::Simple(r) = inst {
            match r.name.as_str() {
                "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE" | "BOUNDED_SURFACE_CURVE" => {
                    return self.edge_curve_geom(ref_of(param(r, 1)?)?, c, depth + 1);
                }
                "TRIMMED_CURVE" => {
                    let (cv, rev) = self.edge_curve_geom(ref_of(param(r, 1)?)?, c, depth + 1)?;
                    let sense = boolean(param(r, 4)?)?;
                    return Ok((cv, rev ^ !sense));
                }
                _ => {}
            }
        }
        Ok((self.curve(id, c, depth)?, false))
    }

    fn curve(&self, id: u64, c: &Ctx, depth: usize) -> R<Curve> {
        self.check_depth(depth)?;
        let s = c.u.mm;
        let inst = self.inst(id)?;
        match inst {
            Instance::Simple(r) => match r.name.as_str() {
                "LINE" => {
                    let origin = self.point(param(r, 1)?, s)?;
                    let (dir, magnitude) = self.vector(param(r, 2)?, s)?;
                    if !(magnitude.is_finite() && magnitude > 0.0) {
                        return invalid("LINE with a zero vector");
                    }
                    Ok(Curve::Line {
                        origin,
                        dir,
                        magnitude,
                    })
                }
                "CIRCLE" => {
                    let frame = self.placement(param(r, 1)?, s)?;
                    let radius = positive(num(param(r, 2)?)? * s, "CIRCLE radius")?;
                    Ok(Curve::Circle { frame, radius })
                }
                "ELLIPSE" => {
                    let frame = self.placement(param(r, 1)?, s)?;
                    let semi1 = positive(num(param(r, 2)?)? * s, "ELLIPSE semi axis")?;
                    let semi2 = positive(num(param(r, 3)?)? * s, "ELLIPSE semi axis")?;
                    Ok(Curve::Ellipse {
                        frame,
                        semi1,
                        semi2,
                    })
                }
                "POLYLINE" => {
                    let pts = list(param(r, 1)?)?;
                    if pts.len() < 2 {
                        return invalid("POLYLINE with fewer than 2 points");
                    }
                    let mut points = Vec::new();
                    for p in pts {
                        points.push(self.point(p, s)?);
                    }
                    let n = points.len();
                    let knots: Vec<f64> = (0..n).map(|i| i as f64).collect();
                    let mut mults = vec![1u32; n];
                    if let Some(m) = mults.first_mut() {
                        *m = 2;
                    }
                    if let Some(m) = mults.last_mut() {
                        *m = 2;
                    }
                    Ok(Curve::BSpline(BSplineCurve {
                        degree: 1,
                        points,
                        weights: None,
                        knots,
                        mults,
                    }))
                }
                "B_SPLINE_CURVE_WITH_KNOTS" => {
                    let b = self.bspline_curve_base(r.params.as_slice(), 1, s)?;
                    let tail = r.params.get(6..).unwrap_or_default();
                    self.finish_bspline_curve(b, tail, None)
                }
                "QUASI_UNIFORM_CURVE" | "UNIFORM_CURVE" | "BEZIER_CURVE" => {
                    let b = self.bspline_curve_base(r.params.as_slice(), 1, s)?;
                    implied_knots_curve(b, &r.name, None)
                }
                // As a swept curve: the basis / 3D curve. The trimmed portion only
                // narrows the swept surface, which the face's loops bound anyway.
                "TRIMMED_CURVE" | "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE" => {
                    self.curve(ref_of(param(r, 1)?)?, c, depth + 1)
                }
                other => Err(MapError::Unsupported(other.to_string())),
            },
            Instance::Complex(rs) => {
                let base = rs
                    .iter()
                    .find(|r| r.name == "B_SPLINE_CURVE")
                    .map_or_else(|| Err(MapError::Unsupported(inst.type_name())), Ok)?;
                let b = self.bspline_curve_base(base.params.as_slice(), 0, s)?;
                let weights = match rs.iter().find(|r| r.name == "RATIONAL_B_SPLINE_CURVE") {
                    Some(w) => Some(num_list(list(param(w, 0)?)?)?),
                    None => None,
                };
                if let Some(k) = rs.iter().find(|r| r.name == "B_SPLINE_CURVE_WITH_KNOTS") {
                    self.finish_bspline_curve(b, k.params.as_slice(), weights)
                } else if let Some(k) = rs.iter().find(|r| {
                    matches!(
                        r.name.as_str(),
                        "QUASI_UNIFORM_CURVE" | "UNIFORM_CURVE" | "BEZIER_CURVE"
                    )
                }) {
                    implied_knots_curve(b, &k.name, weights)
                } else {
                    Err(MapError::Unsupported(inst.type_name()))
                }
            }
        }
    }

    /// (degree, points) from `B_SPLINE_CURVE` attributes starting at `at` (after the
    /// name when the record is a simple instance).
    fn bspline_curve_base(&self, ps: &[Param], at: usize, s: f64) -> R<(u32, Vec<P3>)> {
        let degree = degree_of(ps.get(at))?;
        let pts = list(
            ps.get(at + 1)
                .map_or_else(|| invalid("missing points"), Ok)?,
        )?;
        let mut points = Vec::new();
        for p in pts {
            points.push(self.point(p, s)?);
        }
        Ok((degree, points))
    }

    /// Completes a B-spline from `B_SPLINE_CURVE_WITH_KNOTS`'s own attributes
    /// (multiplicities, knots, knot spec).
    #[allow(clippy::unused_self)]
    fn finish_bspline_curve(
        &self,
        (degree, points): (u32, Vec<P3>),
        ps: &[Param],
        weights: Option<Vec<f64>>,
    ) -> R<Curve> {
        let mults = mult_list(list(
            ps.first().map_or_else(|| invalid("missing mults"), Ok)?,
        )?)?;
        let knots = num_list(list(
            ps.get(1).map_or_else(|| invalid("missing knots"), Ok)?,
        )?)?;
        let b = BSplineCurve {
            degree,
            points,
            weights,
            knots,
            mults,
        };
        check_bspline_curve(&b)?;
        Ok(Curve::BSpline(b))
    }

    // ---- surfaces ------------------------------------------------------------------

    fn surface(&self, id: u64, c: &Ctx, depth: usize) -> R<Surface> {
        self.check_depth(depth)?;
        let s = c.u.mm;
        let inst = self.inst(id)?;
        match inst {
            Instance::Simple(r) => match r.name.as_str() {
                "PLANE" => Ok(Surface::Plane {
                    frame: self.placement(param(r, 1)?, s)?,
                }),
                "CYLINDRICAL_SURFACE" => Ok(Surface::Cylinder {
                    frame: self.placement(param(r, 1)?, s)?,
                    radius: positive(num(param(r, 2)?)? * s, "cylinder radius")?,
                }),
                "CONICAL_SURFACE" => {
                    let frame = self.placement(param(r, 1)?, s)?;
                    let radius = num(param(r, 2)?)? * s;
                    if !(radius.is_finite() && radius >= 0.0) {
                        return invalid("cone radius");
                    }
                    let a = num(param(r, 3)?)? * c.u.deg;
                    if !(a > 0.0 && a < 90.0) {
                        return invalid(format!("cone semi angle {a} degrees"));
                    }
                    Ok(Surface::Cone {
                        frame,
                        radius,
                        semi_angle_deg: a,
                    })
                }
                "SPHERICAL_SURFACE" => Ok(Surface::Sphere {
                    frame: self.placement(param(r, 1)?, s)?,
                    radius: positive(num(param(r, 2)?)? * s, "sphere radius")?,
                }),
                "TOROIDAL_SURFACE" => Ok(Surface::Torus {
                    frame: self.placement(param(r, 1)?, s)?,
                    major: positive(num(param(r, 2)?)? * s, "torus major radius")?,
                    minor: positive(num(param(r, 3)?)? * s, "torus minor radius")?,
                }),
                "SURFACE_OF_REVOLUTION" => {
                    let curve = self.curve(ref_of(param(r, 1)?)?, c, depth + 1)?;
                    let ax = self.simple(ref_of(param(r, 2)?)?)?;
                    if ax.name != "AXIS1_PLACEMENT" {
                        return Err(MapError::Unsupported(ax.name.clone()));
                    }
                    let axis_origin = self.point(param(ax, 1)?, s)?;
                    let axis_dir = match ax.params.get(2) {
                        None | Some(Param::Null) => [0.0, 0.0, 1.0],
                        Some(p) => self.direction(p)?,
                    };
                    let axis_dir = unit(axis_dir).map_or_else(|| invalid("zero axis"), Ok)?;
                    Ok(Surface::Revolution {
                        curve: Box::new(curve),
                        axis_origin,
                        axis_dir,
                    })
                }
                "SURFACE_OF_LINEAR_EXTRUSION" => {
                    let curve = self.curve(ref_of(param(r, 1)?)?, c, depth + 1)?;
                    let (dir, magnitude) = self.vector(param(r, 2)?, s)?;
                    if !(magnitude.is_finite() && magnitude > 0.0) {
                        return invalid("extrusion with a zero vector");
                    }
                    Ok(Surface::Extrusion {
                        curve: Box::new(curve),
                        dir,
                        magnitude,
                    })
                }
                "B_SPLINE_SURFACE_WITH_KNOTS" => {
                    let (ud, vd, pts) = self.bspline_surface_base(r.params.as_slice(), 1, s)?;
                    let tail = r.params.get(8..).unwrap_or_default();
                    finish_bspline_surface(ud, vd, pts, tail, None)
                }
                "QUASI_UNIFORM_SURFACE" | "UNIFORM_SURFACE" | "BEZIER_SURFACE" => {
                    let (ud, vd, pts) = self.bspline_surface_base(r.params.as_slice(), 1, s)?;
                    implied_knots_surface(ud, vd, pts, &r.name, None)
                }
                other => Err(MapError::Unsupported(other.to_string())),
            },
            Instance::Complex(rs) => {
                let base = rs
                    .iter()
                    .find(|r| r.name == "B_SPLINE_SURFACE")
                    .map_or_else(|| Err(MapError::Unsupported(inst.type_name())), Ok)?;
                let (ud, vd, pts) = self.bspline_surface_base(base.params.as_slice(), 0, s)?;
                let weights = match rs.iter().find(|r| r.name == "RATIONAL_B_SPLINE_SURFACE") {
                    Some(w) => {
                        let mut rows = Vec::new();
                        for row in list(param(w, 0)?)? {
                            rows.push(num_list(list(row)?)?);
                        }
                        Some(rows)
                    }
                    None => None,
                };
                if let Some(k) = rs.iter().find(|r| r.name == "B_SPLINE_SURFACE_WITH_KNOTS") {
                    return finish_bspline_surface(ud, vd, pts, k.params.as_slice(), weights);
                }
                let k = rs
                    .iter()
                    .find(|r| {
                        matches!(
                            r.name.as_str(),
                            "QUASI_UNIFORM_SURFACE" | "UNIFORM_SURFACE" | "BEZIER_SURFACE"
                        )
                    })
                    .map_or_else(|| Err(MapError::Unsupported(inst.type_name())), Ok)?;
                implied_knots_surface(ud, vd, pts, &k.name, weights)
            }
        }
    }

    fn bspline_surface_base(&self, ps: &[Param], at: usize, s: f64) -> R<(u32, u32, Vec<Vec<P3>>)> {
        let ud = degree_of(ps.get(at))?;
        let vd = degree_of(ps.get(at + 1))?;
        let rows = list(
            ps.get(at + 2)
                .map_or_else(|| invalid("missing points"), Ok)?,
        )?;
        let mut pts = Vec::new();
        for row in rows {
            let mut out = Vec::new();
            for p in list(row)? {
                out.push(self.point(p, s)?);
            }
            pts.push(out);
        }
        Ok((ud, vd, pts))
    }

    // ---- topology ------------------------------------------------------------------

    fn vertex(&self, id: u64, c: &mut Ctx) -> R<usize> {
        if let Some(&i) = c.vmap.get(&id) {
            return Ok(i);
        }
        let r = self.simple(id)?;
        if r.name != "VERTEX_POINT" {
            return Err(MapError::Unsupported(r.name.clone()));
        }
        let point = self.point(param(r, 1)?, c.u.mm)?;
        let i = c.vertices.len();
        c.vertices.push(Vertex { step_id: id, point });
        c.vmap.insert(id, i);
        Ok(i)
    }

    fn edge(&self, id: u64, c: &mut Ctx) -> R<usize> {
        if let Some(&i) = c.emap.get(&id) {
            return Ok(i);
        }
        let r = self.simple(id)?;
        if r.name != "EDGE_CURVE" {
            return Err(MapError::Unsupported(r.name.clone()));
        }
        let start = self.vertex(ref_of(param(r, 1)?)?, c)?;
        let end = self.vertex(ref_of(param(r, 2)?)?, c)?;
        let (curve, reversed) = self.edge_curve_geom(ref_of(param(r, 3)?)?, c, 0)?;
        let same_sense = boolean(param(r, 4)?)? ^ reversed;
        let ps = c.vertices.get(start).map(|v| v.point);
        let pe = c.vertices.get(end).map(|v| v.point);
        let (Some(ps), Some(pe)) = (ps, pe) else {
            return invalid("edge vertex");
        };
        let (t_start, t_end) = edge_params(&curve, ps, pe, start == end, same_sense)?;
        let i = c.edges.len();
        c.edges.push(Edge {
            step_id: id,
            start,
            end,
            curve,
            same_sense,
            t_start,
            t_end,
        });
        c.emap.insert(id, i);
        Ok(i)
    }

    /// `ORIENTED_EDGE` (possibly of another oriented edge) -> (edge, forward).
    fn oriented_edge(&self, id: u64, c: &mut Ctx, depth: usize) -> R<OrientedEdge> {
        self.check_depth(depth)?;
        let r = self.simple(id)?;
        match r.name.as_str() {
            "ORIENTED_EDGE" => {
                let inner = ref_of(param(r, 3)?)?;
                let fwd = boolean(param(r, 4)?)?;
                let e = self.oriented_edge(inner, c, depth + 1)?;
                Ok(OrientedEdge {
                    edge: e.edge,
                    forward: e.forward == fwd,
                })
            }
            "EDGE_CURVE" => Ok(OrientedEdge {
                edge: self.edge(id, c)?,
                forward: true,
            }),
            other => Err(MapError::Unsupported(other.to_string())),
        }
    }

    /// A poly-loop point as a vertex (keyed by the point's id).
    fn poly_vertex(&self, p: &Param, c: &mut Ctx) -> R<(u64, usize)> {
        let id = ref_of(p)?;
        if let Some(&i) = c.vmap.get(&id) {
            return Ok((id, i));
        }
        let point = self.point(p, c.u.mm)?;
        let i = c.vertices.len();
        c.vertices.push(Vertex { step_id: id, point });
        c.vmap.insert(id, i);
        Ok((id, i))
    }

    fn poly_loop(&self, r: &Record, c: &mut Ctx) -> R<Vec<OrientedEdge>> {
        let pts = list(param(r, 1)?)?;
        if pts.len() < 3 {
            return invalid("POLY_LOOP with fewer than 3 points");
        }
        let mut vs = Vec::new();
        for p in pts {
            vs.push(self.poly_vertex(p, c)?);
        }
        let mut out = Vec::new();
        for (k, &(a_id, a)) in vs.iter().enumerate() {
            let &(b_id, b) = vs
                .get((k + 1) % vs.len())
                .map_or_else(|| invalid("poly"), Ok)?;
            if a_id == b_id {
                continue;
            }
            let key = (a_id.min(b_id), a_id.max(b_id));
            let edge = if let Some(&e) = c.pmap.get(&key) {
                e
            } else {
                let (s, e) = if a_id < b_id { (a, b) } else { (b, a) };
                let ps = c.vertices.get(s).map(|v| v.point);
                let pe = c.vertices.get(e).map(|v| v.point);
                let (Some(ps), Some(pe)) = (ps, pe) else {
                    return invalid("poly vertex");
                };
                let len = dist(ps, pe);
                let dir =
                    unit(geom::sub(pe, ps)).map_or_else(|| invalid("zero-length poly edge"), Ok)?;
                let i = c.edges.len();
                c.edges.push(Edge {
                    step_id: 0,
                    start: s,
                    end: e,
                    curve: Curve::Line {
                        origin: ps,
                        dir,
                        magnitude: len,
                    },
                    same_sense: true,
                    t_start: 0.0,
                    t_end: 1.0,
                });
                c.pmap.insert(key, i);
                i
            };
            out.push(OrientedEdge {
                edge,
                forward: a_id < b_id,
            });
        }
        Ok(out)
    }

    fn face_bound(&self, id: u64, c: &mut Ctx) -> R<(Loop, Option<Vec<P3>>)> {
        let r = self.simple(id)?;
        let outer = match r.name.as_str() {
            "FACE_OUTER_BOUND" => true,
            "FACE_BOUND" => false,
            other => return Err(MapError::Unsupported(other.to_string())),
        };
        let orientation = boolean(param(r, 2)?)?;
        let lid = ref_of(param(r, 1)?)?;
        let lr = self.simple(lid)?;
        let mut poly = None;
        let bound = match lr.name.as_str() {
            "EDGE_LOOP" => {
                let mut es = Vec::new();
                for p in list(param(lr, 1)?)? {
                    es.push(self.oriented_edge(ref_of(p)?, c, 0)?);
                }
                if es.is_empty() {
                    return invalid("empty EDGE_LOOP");
                }
                Bound::Edges(es)
            }
            "VERTEX_LOOP" => Bound::Vertex(self.vertex(ref_of(param(lr, 1)?)?, c)?),
            "POLY_LOOP" => {
                let mut pts = Vec::new();
                for p in list(param(lr, 1)?)? {
                    pts.push(self.point(p, c.u.mm)?);
                }
                poly = Some(pts);
                Bound::Edges(self.poly_loop(lr, c)?)
            }
            other => return Err(MapError::Unsupported(other.to_string())),
        };
        Ok((
            Loop {
                outer,
                orientation,
                bound,
            },
            poly,
        ))
    }

    fn face(&self, id: u64, c: &mut Ctx, depth: usize) -> R<usize> {
        self.check_depth(depth)?;
        let r = self.simple(id)?;
        match r.name.as_str() {
            "ORIENTED_FACE" => {
                let inner = self.face(ref_of(param(r, 2)?)?, c, depth + 1)?;
                if !boolean(param(r, 3)?)? {
                    // Face rows are not memoized, so this freshly mapped inner row
                    // is not yet referenced by a shell. Reverse it in place rather
                    // than appending a clone and leaving an orphaned inner face.
                    let f = c.faces.get_mut(inner).map_or_else(|| invalid("face"), Ok)?;
                    f.step_id = id;
                    f.same_sense = !f.same_sense;
                    for l in &mut f.loops {
                        l.orientation = !l.orientation;
                    }
                    return Ok(inner);
                }
                Ok(inner)
            }
            "ADVANCED_FACE" | "FACE_SURFACE" | "FACE" => {
                let mut loops = Vec::new();
                let mut first_poly = None;
                for p in list(param(r, 1)?)? {
                    let (l, poly) = self.face_bound(ref_of(p)?, c)?;
                    if first_poly.is_none() {
                        first_poly = poly;
                    }
                    loops.push(l);
                }
                let (surface, same_sense) = if r.name == "FACE" {
                    let pts = first_poly.map_or_else(
                        || Err(MapError::Unsupported("FACE without geometry".into())),
                        Ok,
                    )?;
                    (polygon_plane(&pts)?, true)
                } else {
                    (
                        self.surface(ref_of(param(r, 2)?)?, c, 0)?,
                        boolean(param(r, 3)?)?,
                    )
                };
                c.faces.push(Face {
                    step_id: id,
                    surface,
                    same_sense,
                    loops,
                });
                Ok(c.faces.len() - 1)
            }
            other => Err(MapError::Unsupported(other.to_string())),
        }
    }

    fn shell(&self, id: u64, c: &mut Ctx, depth: usize) -> R<Shell> {
        self.check_depth(depth)?;
        let r = self.simple(id)?;
        match r.name.as_str() {
            "CLOSED_SHELL" | "OPEN_SHELL" => {
                let mut faces = Vec::new();
                for p in list(param(r, 1)?)? {
                    faces.push(self.face(ref_of(p)?, c, 0)?);
                }
                Ok(Shell {
                    step_id: id,
                    closed: r.name == "CLOSED_SHELL",
                    orientation: true,
                    faces,
                })
            }
            "ORIENTED_CLOSED_SHELL" | "ORIENTED_OPEN_SHELL" => {
                let mut sh = self.shell(ref_of(param(r, 2)?)?, c, depth + 1)?;
                if !boolean(param(r, 3)?)? {
                    sh.orientation = !sh.orientation;
                }
                sh.step_id = id;
                Ok(sh)
            }
            other => Err(MapError::Unsupported(other.to_string())),
        }
    }

    fn body(&self, id: u64, kind: BodyKind, r: &Record, u: Units) -> R<Body> {
        let mut c = Ctx {
            u,
            vertices: Vec::new(),
            edges: Vec::new(),
            faces: Vec::new(),
            vmap: BTreeMap::new(),
            emap: BTreeMap::new(),
            pmap: BTreeMap::new(),
        };
        let mut shells = Vec::new();
        match kind {
            BodyKind::ManifoldSolid | BodyKind::Faceted => {
                shells.push(self.shell(ref_of(param(r, 1)?)?, &mut c, 0)?);
            }
            BodyKind::SolidWithVoids => {
                shells.push(self.shell(ref_of(param(r, 1)?)?, &mut c, 0)?);
                for p in list(param(r, 2)?)? {
                    shells.push(self.shell(ref_of(p)?, &mut c, 0)?);
                }
            }
            BodyKind::ShellModel => {
                for p in list(param(r, 1)?)? {
                    shells.push(self.shell(ref_of(p)?, &mut c, 0)?);
                }
            }
        }
        Ok(Body {
            step_id: id,
            name: match r.params.first() {
                Some(Param::Str(name)) if !name.is_empty() => Some(name.clone()),
                _ => None,
            },
            kind,
            unit_mm: u.mm,
            uncertainty_mm: u.uncertainty,
            shells,
            faces: c.faces,
            edges: c.edges,
            vertices: c.vertices,
        })
    }
}

/// Parameters of an edge's start and end vertices on its curve (see [`Edge`]).
fn edge_params(curve: &Curve, ps: P3, pe: P3, closed: bool, same_sense: bool) -> R<(f64, f64)> {
    let periodic = |ts: f64, te: f64| -> (f64, f64) {
        let mut te = te;
        if same_sense {
            if closed {
                te = ts + 360.0;
            }
            while te <= ts {
                te += 360.0;
            }
            while te > ts + 360.0 {
                te -= 360.0;
            }
        } else {
            if closed {
                te = ts - 360.0;
            }
            while te >= ts {
                te -= 360.0;
            }
            while te < ts - 360.0 {
                te += 360.0;
            }
        }
        // Keep both ends in [-360, 360] (SMLib's angular domain limits).
        let (lo, hi) = (ts.min(te), ts.max(te));
        if hi > 360.0 {
            (ts - 360.0, te - 360.0)
        } else if lo < -360.0 {
            (ts + 360.0, te + 360.0)
        } else {
            (ts, te)
        }
    };
    Ok(match curve {
        Curve::Line {
            origin,
            dir,
            magnitude,
        } => {
            if closed {
                return invalid("closed edge on a LINE");
            }
            let t = |p: P3| geom::dot(geom::sub(p, *origin), *dir) / magnitude;
            (t(ps), t(pe))
        }
        Curve::Circle { frame, radius } => periodic(
            conic_angle(frame, *radius, *radius, ps),
            conic_angle(frame, *radius, *radius, pe),
        ),
        Curve::Ellipse {
            frame,
            semi1,
            semi2,
        } => periodic(
            conic_angle(frame, *semi1, *semi2, ps),
            conic_angle(frame, *semi1, *semi2, pe),
        ),
        Curve::BSpline(b) => {
            let (lo, hi) = geom::bspline_range(b).map_or_else(|| invalid("B-spline range"), Ok)?;
            if closed {
                if same_sense {
                    (lo, hi)
                } else {
                    (hi, lo)
                }
            } else {
                let ts = geom::bspline_param(b, ps).map_or_else(|| invalid("B-spline eval"), Ok)?;
                let te = geom::bspline_param(b, pe).map_or_else(|| invalid("B-spline eval"), Ok)?;
                (ts, te)
            }
        }
    })
}

/// The plane through a polygon (Newell's normal), X along its first side.
fn polygon_plane(pts: &[P3]) -> R<Surface> {
    let mut n = [0.0; 3];
    for (k, a) in pts.iter().enumerate() {
        let b = pts.get((k + 1) % pts.len()).copied().unwrap_or(*a);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let o = *pts.first().map_or_else(|| invalid("empty polygon"), Ok)?;
    let x = pts.get(1).map(|p| geom::sub(*p, o));
    let frame = build_frame(o, Some(n), x).map_or_else(|| invalid("degenerate polygon"), Ok)?;
    Ok(Surface::Plane { frame })
}

pub(crate) fn check_bspline_curve(b: &BSplineCurve) -> R<()> {
    let n = b.points.len();
    let p = b.degree as usize;
    if n < p + 1 {
        return invalid("B-spline with too few control points");
    }
    if geom::expand_knots(&b.knots, &b.mults, n + p + 1).is_none() {
        return invalid("B-spline knot multiplicities do not match the control points");
    }
    check_knots(&b.knots, &b.mults, p)?;
    if let Some(w) = &b.weights {
        if w.len() != n || w.iter().any(|x| !(x.is_finite() && *x > 0.0)) {
            return invalid("B-spline weights");
        }
    }
    Ok(())
}

pub(crate) fn check_knots(knots: &[f64], mults: &[u32], degree: usize) -> R<()> {
    if knots
        .windows(2)
        .any(|w| matches!(w, [a, b] if a.partial_cmp(b) != Some(std::cmp::Ordering::Less)))
    {
        return invalid("knots not increasing");
    }
    if mults.iter().any(|&m| m == 0 || m as usize > degree + 1) {
        return invalid("knot multiplicity out of range");
    }
    Ok(())
}

fn implied_knots_curve(
    (degree, points): (u32, Vec<P3>),
    form: &str,
    weights: Option<Vec<f64>>,
) -> R<Curve> {
    let (knots, mults) = implied_knots(form, degree, points.len())?;
    let b = BSplineCurve {
        degree,
        points,
        weights,
        knots,
        mults,
    };
    check_bspline_curve(&b)?;
    Ok(Curve::BSpline(b))
}

/// ISO 10303-42 knots implied by `UNIFORM_*`, `QUASI_UNIFORM_*` and `BEZIER_*` forms
/// for `n` control points of degree `d`.
fn implied_knots(form: &str, d: u32, n: usize) -> R<(Vec<f64>, Vec<u32>)> {
    let dd = d as usize;
    if n < dd + 1 {
        return invalid("too few control points");
    }
    Ok(if form.starts_with("UNIFORM") {
        // knots -d .. n, all simple: n + d + 1 of them
        let k: Vec<f64> = (0..=(n + dd)).map(|i| i as f64 - f64::from(d)).collect();
        let m = vec![1; k.len()];
        (k, m)
    } else if form.starts_with("QUASI_UNIFORM") {
        let count = n - dd + 1;
        let k: Vec<f64> = (0..count).map(|i| i as f64).collect();
        let mut m = vec![1; count];
        if let Some(x) = m.first_mut() {
            *x = d + 1;
        }
        if let Some(x) = m.last_mut() {
            *x = d + 1;
        }
        (k, m)
    } else {
        // BEZIER: segments of d points sharing ends
        if dd == 0 || !(n - 1).is_multiple_of(dd) {
            return invalid("Bezier control point count");
        }
        let segs = (n - 1) / dd;
        let k: Vec<f64> = (0..=segs).map(|i| i as f64).collect();
        let mut m = vec![d; segs + 1];
        if let Some(x) = m.first_mut() {
            *x = d + 1;
        }
        if let Some(x) = m.last_mut() {
            *x = d + 1;
        }
        (k, m)
    })
}

fn implied_knots_surface(
    ud: u32,
    vd: u32,
    points: Vec<Vec<P3>>,
    form: &str,
    weights: Option<Vec<Vec<f64>>>,
) -> R<Surface> {
    let nu = points.len();
    let nv = points.first().map_or(0, Vec::len);
    let (uk, um) = implied_knots(form, ud, nu)?;
    let (vk, vm) = implied_knots(form, vd, nv)?;
    let tail = [
        knot_params(&um),
        knot_params(&vm),
        real_params(&uk),
        real_params(&vk),
    ];
    finish_bspline_surface(ud, vd, points, &tail, weights)
}

fn knot_params(m: &[u32]) -> Param {
    Param::List(m.iter().map(|&x| Param::Int(i64::from(x))).collect())
}

fn real_params(k: &[f64]) -> Param {
    Param::List(k.iter().map(|&x| Param::Real(x)).collect())
}

fn finish_bspline_surface(
    ud: u32,
    vd: u32,
    points: Vec<Vec<P3>>,
    ps: &[Param],
    weights: Option<Vec<Vec<f64>>>,
) -> R<Surface> {
    let u_mults = mult_list(list(
        ps.first().map_or_else(|| invalid("missing mults"), Ok)?,
    )?)?;
    let v_mults = mult_list(list(
        ps.get(1).map_or_else(|| invalid("missing mults"), Ok)?,
    )?)?;
    let u_knots = num_list(list(
        ps.get(2).map_or_else(|| invalid("missing knots"), Ok)?,
    )?)?;
    let v_knots = num_list(list(
        ps.get(3).map_or_else(|| invalid("missing knots"), Ok)?,
    )?)?;
    let nu = points.len();
    let nv = points.first().map_or(0, Vec::len);
    if nu < ud as usize + 1 || nv < vd as usize + 1 || points.iter().any(|r| r.len() != nv) {
        return invalid("B-spline surface control net");
    }
    if geom::expand_knots(&u_knots, &u_mults, nu + ud as usize + 1).is_none()
        || geom::expand_knots(&v_knots, &v_mults, nv + vd as usize + 1).is_none()
    {
        return invalid("B-spline surface knot multiplicities do not match the net");
    }
    check_knots(&u_knots, &u_mults, ud as usize)?;
    check_knots(&v_knots, &v_mults, vd as usize)?;
    if let Some(w) = &weights {
        if w.len() != nu
            || w.iter()
                .any(|r| r.len() != nv || r.iter().any(|x| !(x.is_finite() && *x > 0.0)))
        {
            return invalid("B-spline surface weights");
        }
    }
    Ok(Surface::BSpline(BSplineSurface {
        u_degree: ud,
        v_degree: vd,
        points,
        weights,
        u_knots,
        u_mults,
        v_knots,
        v_mults,
    }))
}

fn param(r: &Record, i: usize) -> R<&Param> {
    r.params
        .get(i)
        .map_or_else(|| invalid(format!("{} has no parameter {i}", r.name)), Ok)
}

fn ref_of(p: &Param) -> R<u64> {
    p.as_ref_id()
        .map_or_else(|| invalid(format!("expected a reference, found {p:?}")), Ok)
}

fn list(p: &Param) -> R<&[Param]> {
    p.as_list()
        .map_or_else(|| invalid(format!("expected a list, found {p:?}")), Ok)
}

fn num(p: &Param) -> R<f64> {
    match p.as_f64() {
        Some(x) if x.is_finite() => Ok(x),
        _ => invalid(format!("expected a number, found {p:?}")),
    }
}

fn num_at(l: &[Param], i: usize) -> R<f64> {
    num(l
        .get(i)
        .map_or_else(|| invalid("short coordinate list"), Ok)?)
}

fn num_list(l: &[Param]) -> R<Vec<f64>> {
    l.iter().map(num).collect()
}

fn mult_list(l: &[Param]) -> R<Vec<u32>> {
    l.iter()
        .map(|p| match p {
            Param::Int(i) if (1..=33).contains(i) => Ok(*i as u32),
            _ => invalid(format!("bad knot multiplicity {p:?}")),
        })
        .collect()
}

fn degree_of(p: Option<&Param>) -> R<u32> {
    match p {
        Some(Param::Int(d)) if (1..=32).contains(d) => Ok(*d as u32),
        _ => Err(MapError::Unsupported(format!("B-spline degree {p:?}"))),
    }
}

fn boolean(p: &Param) -> R<bool> {
    match p.as_enum() {
        Some("T") => Ok(true),
        Some("F") => Ok(false),
        _ => invalid(format!("expected .T. or .F., found {p:?}")),
    }
}

fn positive(x: f64, what: &str) -> R<f64> {
    if x.is_finite() && x > 0.0 {
        Ok(x)
    } else {
        invalid(format!("{what} {x}"))
    }
}

/// Maps every B-rep body of `file`.
/// Report references that point at no entity in the data section.
fn collect_dangling_references(file: &StepFile, diagnostics: &mut Vec<Diagnostic>) {
    fn visit(param: &Param, missing: &mut Vec<u64>) {
        match param {
            Param::Ref(id) => missing.push(*id),
            Param::List(items) => {
                for item in items {
                    visit(item, missing);
                }
            }
            Param::Typed(_, value) => visit(value, missing),
            _ => {}
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for (id, inst) in &file.entities {
        let records: &[Record] = match inst {
            Instance::Simple(record) => std::slice::from_ref(record),
            Instance::Complex(records) => records,
        };
        for record in records {
            for param in &record.params {
                let mut missing = Vec::new();
                visit(param, &mut missing);
                for target in missing {
                    if !file.entities.contains_key(&target) && seen.insert((*id, target)) {
                        diagnostics.push(Diagnostic {
                            step_id: Some(*id),
                            message: format!("dangling reference #{target}"),
                        });
                    }
                }
            }
        }
    }
}

/// Maps every B-rep body of `file`.
pub fn map_file(file: &StepFile, limits: &Limits) -> Model {
    let m = Mapper {
        file,
        limits: *limits,
    };
    let contexts = m.item_contexts();
    // Fallback units when an item is in no representation: those of the file's only
    // context with a unit assignment, if there is exactly one kind.
    let mut all_units: Vec<Units> = Vec::new();
    for (id, inst) in &file.entities {
        if inst.record("GLOBAL_UNIT_ASSIGNED_CONTEXT").is_some() {
            if let Ok(u) = m.context_units(*id) {
                if !all_units.contains(&u) {
                    all_units.push(u);
                }
            }
        }
    }
    let fallback = match all_units.as_slice() {
        [u] => *u,
        _ => Units::default(),
    };
    let mut model = Model {
        header: file.header.clone(),
        schemas: file.schemas.clone(),
        ..Model::default()
    };
    if all_units.is_empty() {
        model.diagnostics.push(Diagnostic {
            step_id: None,
            message: "no GLOBAL_UNIT_ASSIGNED_CONTEXT; assuming millimetres/degrees".into(),
        });
    } else if all_units.len() > 1 {
        model.diagnostics.push(Diagnostic {
            step_id: None,
            message: format!(
                "{} distinct unit contexts; using the default fallback for bodies without an explicit context",
                all_units.len()
            ),
        });
    }
    for (id, inst) in &file.entities {
        let rec = match inst {
            Instance::Simple(r) => r,
            Instance::Complex(_) => {
                if inst.record("MAPPED_ITEM").is_some() {
                    model.assembly_instances += 1;
                }
                continue;
            }
        };
        if rec.name == "MAPPED_ITEM" || rec.name == "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION" {
            model.assembly_instances += 1;
            continue;
        }
        let Some(&(name, kind)) = BODY_NAMES.iter().find(|(n, _)| *n == rec.name) else {
            continue;
        };
        let units = if let Some(ctx) = contexts.get(id) {
            m.context_units(*ctx)
        } else {
            model.diagnostics.push(Diagnostic {
                step_id: Some(*id),
                message: "body has no representation context; using the file unit fallback".into(),
            });
            Ok(fallback)
        };
        match units.and_then(|u| m.body(*id, kind, rec, u)) {
            Ok(b) => model.bodies.push(b),
            Err(e) => model.skipped.push(Skipped {
                step_id: *id,
                kind: name.to_string(),
                reason: e.to_string(),
            }),
        }
    }
    if model.assembly_instances > 0 {
        model.diagnostics.push(Diagnostic {
            step_id: None,
            message: format!(
                "{} assembly instance(s) are counted but not applied; bodies are in their defining representation coordinates",
                model.assembly_instances
            ),
        });
    }
    collect_dangling_references(file, &mut model.diagnostics);
    model
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part21::parse;

    fn model(data: &str) -> Model {
        let text = format!("ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));\nENDSEC;\nDATA;\n{data}\nENDSEC;\nEND-ISO-10303-21;\n");
        let f = parse(text.as_bytes(), &Limits::default()).unwrap();
        map_file(&f, &Limits::default())
    }

    /// Millimetre units and the representation context that assigns them to the
    /// `SHELL_BASED_SURFACE_MODEL` #5 of the oriented-face fixtures.
    const ORIENTED_FACE_CONTEXT: &str = "#20=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\
         #21=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#20)) REPRESENTATION_CONTEXT('',''));\
         #22=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#5),#21);";

    /// An ordinary planar `FACE` #6 on the non-collinear triangle #9/#10/#11,
    /// with its only bound `#7` a `.T.` `FACE_OUTER_BOUND`.
    const ORIENTED_FACE_TRIANGLE: &str = "#6=FACE('',(#7));\
         #7=FACE_OUTER_BOUND('',#8,.T.);\
         #8=POLY_LOOP('',(#9,#10,#11));\
         #9=CARTESIAN_POINT('',(0.,0.,0.));\
         #10=CARTESIAN_POINT('',(3.,0.,0.));\
         #11=CARTESIAN_POINT('',(0.,4.,0.));";

    fn oriented_face_model(body: &str) -> Model {
        model(&format!(
            "{ORIENTED_FACE_TRIANGLE}{body}{ORIENTED_FACE_CONTEXT}"
        ))
    }

    /// Two `ORIENTED_FACE`s over the same base `FACE` #6, each in its own open
    /// shell; `shells` is the item list of `SHELL_BASED_SURFACE_MODEL` #5.
    fn oriented_face_mixed_model(shells: &str) -> Model {
        oriented_face_model(&format!(
            "#3=ORIENTED_FACE('',*,#6,.T.);\
             #4=ORIENTED_FACE('',*,#6,.F.);\
             #5=SHELL_BASED_SURFACE_MODEL('',({shells}));\
             #13=OPEN_SHELL('',(#3));\
             #14=OPEN_SHELL('',(#4));"
        ))
    }

    /// A fixture is complete only if it maps to exactly one millimetre body
    /// without skips or diagnostics.
    fn assert_single_oriented_face_body(m: &Model) -> &Body {
        assert_eq!(m.bodies.len(), 1);
        assert!(m.skipped.is_empty(), "skipped: {:?}", m.skipped);
        assert!(m.diagnostics.is_empty(), "diagnostics: {:?}", m.diagnostics);
        let b = &m.bodies[0];
        assert_eq!(b.unit_mm, 1.0);
        b
    }

    #[test]
    fn implied_knot_forms() {
        let (k, m) = implied_knots("QUASI_UNIFORM_CURVE", 3, 6).unwrap();
        assert_eq!(k, vec![0.0, 1.0, 2.0, 3.0]);
        assert_eq!(m, vec![4, 1, 1, 4]);
        let (k, m) = implied_knots("BEZIER_CURVE", 3, 7).unwrap();
        assert_eq!(k, vec![0.0, 1.0, 2.0]);
        assert_eq!(m, vec![4, 3, 4]);
        let (k, _) = implied_knots("UNIFORM_CURVE", 2, 4).unwrap();
        assert_eq!(k.len(), 7);
        assert!(implied_knots("BEZIER_CURVE", 3, 6).is_err());
    }

    #[test]
    fn diagnostics_report_missing_and_multiple_unit_contexts() {
        let m = model("#1=CARTESIAN_POINT('',(0.,0.,0.));");
        assert!(m
            .diagnostics
            .iter()
            .any(|d| d.message.contains("no GLOBAL_UNIT")));

        let m = model(
            "#1=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#10)) REPRESENTATION_CONTEXT('',''));             #2=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#11)) REPRESENTATION_CONTEXT('',''));             #10=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));             #11=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.CENTI.,.METRE.));",
        );
        assert!(m
            .diagnostics
            .iter()
            .any(|d| d.message.contains("distinct unit contexts")));
    }

    #[test]
    fn diagnostics_report_unapplied_assembly_instances() {
        let m = model("#1=MAPPED_ITEM('',#2,#3);");
        assert_eq!(m.assembly_instances, 1);
        assert!(m
            .diagnostics
            .iter()
            .any(|d| d.message.contains("assembly instance")));
    }

    #[test]
    fn body_name_and_missing_context_are_reported() {
        let m = model(
            "#1=MANIFOLD_SOLID_BREP('named',#2);\
             #2=CLOSED_SHELL('',(#3));\
             #3=ADVANCED_FACE('',(),#4,.T.);\
             #4=PLANE('',#5);\
             #5=AXIS2_PLACEMENT_3D('',#6,$,$);\
             #6=CARTESIAN_POINT('',(0.,0.,0.));",
        );
        assert_eq!(m.bodies.len(), 1);
        assert_eq!(m.bodies[0].name.as_deref(), Some("named"));
        assert!(m
            .diagnostics
            .iter()
            .any(|d| d.message.contains("no representation context")));
    }

    #[test]
    fn dangling_references_outside_bodies_are_reported() {
        let m = model("#1=PRODUCT('p','p','',(#9));");
        assert!(m
            .diagnostics
            .iter()
            .any(|d| d.step_id == Some(1) && d.message.contains("dangling reference #9")));
    }

    #[test]
    fn dangling_and_unsupported() {
        let m = model("#1=MANIFOLD_SOLID_BREP('',#2);");
        assert_eq!(m.bodies.len(), 0);
        assert_eq!(m.skipped.len(), 1);
        assert!(m.skipped[0].reason.contains("dangling"));
        let m = model(
            "#1=MANIFOLD_SOLID_BREP('',#2);#2=CLOSED_SHELL('',(#3));\
             #3=ADVANCED_FACE('',(),#4,.T.);#4=OFFSET_SURFACE('',#5,1.,.F.);",
        );
        assert_eq!(m.skipped[0].reason, "unsupported: OFFSET_SURFACE");
        // a reference cycle stops at the depth bound
        let m = model("#1=MANIFOLD_SOLID_BREP('',#2);#2=ORIENTED_CLOSED_SHELL('',*,#2,.F.);");
        assert!(m.skipped[0].reason.contains("too deep"));
    }

    #[test]
    fn oriented_face_reverse_maps_wrapper_in_place() {
        let m = oriented_face_model(
            "#3=ORIENTED_FACE('',*,#6,.F.);\
             #5=SHELL_BASED_SURFACE_MODEL('',(#13));\
             #13=OPEN_SHELL('',(#3));",
        );
        let b = assert_single_oriented_face_body(&m);
        assert_eq!(b.kind, BodyKind::ShellModel);
        assert_eq!(b.faces.len(), 1);
        assert_eq!(b.shells.len(), 1);
        assert_eq!(b.shells[0].faces, vec![0]);
        let f = &b.faces[0];
        assert_eq!(f.step_id, 3);
        assert!(!f.same_sense);
        assert_eq!(f.loops.len(), 1);
        assert!(f.loops[0].outer);
        assert!(!f.loops[0].orientation);
        assert_eq!(b.edges.len(), 3);
        assert_eq!(b.vertices.len(), 3);
    }

    #[test]
    fn oriented_face_positive_control_keeps_inner_face() {
        let m = oriented_face_model(
            "#3=ORIENTED_FACE('',*,#6,.T.);\
             #5=SHELL_BASED_SURFACE_MODEL('',(#13));\
             #13=OPEN_SHELL('',(#3));",
        );
        let b = assert_single_oriented_face_body(&m);
        assert_eq!(b.faces.len(), 1);
        assert_eq!(b.shells.len(), 1);
        assert_eq!(b.shells[0].faces, vec![0]);
        let f = &b.faces[0];
        assert_eq!(f.step_id, 6);
        assert!(f.same_sense);
        assert_eq!(f.loops.len(), 1);
        assert!(f.loops[0].outer);
        assert!(f.loops[0].orientation);
        assert_eq!(b.edges.len(), 3);
        assert_eq!(b.vertices.len(), 3);
    }

    #[test]
    fn oriented_face_mixed_positive_then_reverse_shares_base_face() {
        let m = oriented_face_mixed_model("#13,#14");
        let b = assert_single_oriented_face_body(&m);
        assert_eq!(b.faces.len(), 2);
        assert_eq!(b.shells.len(), 2);
        assert_eq!(b.shells[0].step_id, 13);
        assert_eq!(b.shells[0].faces, vec![0]);
        assert_eq!(b.shells[1].step_id, 14);
        assert_eq!(b.shells[1].faces, vec![1]);
        // The `.T.` wrapper row is mapped first and must not be mutated by the
        // later `.F.` reverse.
        assert_eq!(b.faces[0].step_id, 6);
        assert!(b.faces[0].same_sense);
        assert!(b.faces[0].loops[0].orientation);
        assert_eq!(b.faces[1].step_id, 4);
        assert!(!b.faces[1].same_sense);
        assert!(!b.faces[1].loops[0].orientation);
        // Both wrappers describe the same bounds and geometry.
        assert!(b.faces[0].loops[0].outer);
        assert_eq!(b.faces[0].loops[0].bound, b.faces[1].loops[0].bound);
        assert_eq!(b.faces[0].surface, b.faces[1].surface);
        // POLY_LOOP vertex/edge caching is shared between the two uses.
        assert_eq!(b.edges.len(), 3);
        assert_eq!(b.vertices.len(), 3);
    }

    #[test]
    fn oriented_face_mixed_reverse_then_positive_shares_base_face() {
        let m = oriented_face_mixed_model("#14,#13");
        let b = assert_single_oriented_face_body(&m);
        assert_eq!(b.faces.len(), 2);
        assert_eq!(b.shells.len(), 2);
        assert_eq!(b.shells[0].step_id, 14);
        assert_eq!(b.shells[0].faces, vec![0]);
        assert_eq!(b.shells[1].step_id, 13);
        assert_eq!(b.shells[1].faces, vec![1]);
        assert_eq!(b.faces[0].step_id, 4);
        assert!(!b.faces[0].same_sense);
        assert!(!b.faces[0].loops[0].orientation);
        // The later `.T.` wrapper maps a fresh positive row.
        assert_eq!(b.faces[1].step_id, 6);
        assert!(b.faces[1].same_sense);
        assert!(b.faces[1].loops[0].orientation);
        assert!(b.faces[0].loops[0].outer);
        assert_eq!(b.faces[0].loops[0].bound, b.faces[1].loops[0].bound);
        assert_eq!(b.faces[0].surface, b.faces[1].surface);
        assert_eq!(b.edges.len(), 3);
        assert_eq!(b.vertices.len(), 3);
    }
}
