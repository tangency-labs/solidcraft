//! Writing ISO 10303-21: a DATA section built entity by entity, and [`crate::geom`] curves and
//! surfaces as ISO 10303-42 entities. Identical geometric entities are written once and shared,
//! as the reader shares them; topological ones are distinct ([`Writer::add_distinct`]).
//! [`topology`] writes a schema 4 record's solids.

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::geom::{
    Associated, Axis1, BSplineCurve, BSplineSurface, Curve, CurveForm, KnotSpec, Logical, P3, Placement, PreferredRep, Surface, SurfaceCurveKind,
    SurfaceForm, Trim, TrimPreference,
};
use crate::p21::Param;

pub mod topology;

/// A real as Part 21 writes it: always with a decimal point, exponent as `E`.
pub fn real(x: f64) -> String {
    if !x.is_finite() {
        return "0.".into();
    }
    let s = format!("{x:?}");
    match s.split_once('e') {
        Some((m, e)) => {
            let m = if m.contains('.') { m.to_string() } else { format!("{m}.") };
            format!("{m}E{e}")
        }
        None if s.contains('.') => s,
        None => format!("{s}."),
    }
}

/// A string literal: quotes doubled, non-ASCII as `\X2\` UTF-16 runs.
pub fn string(s: &str) -> String {
    let mut o = String::from("'");
    for c in s.chars() {
        match c {
            '\'' => o.push_str("''"),
            '\\' => o.push_str("\\\\"),
            c if c.is_ascii() && !c.is_ascii_control() => o.push(c),
            c if c.is_ascii_control() => {}
            c => {
                o.push_str("\\X2\\");
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    let _ = write!(o, "{u:04X}");
                }
                o.push_str("\\X0\\");
            }
        }
    }
    o.push('\'');
    o
}

fn param(p: &Param, o: &mut String) {
    match p {
        Param::Int(i) => {
            let _ = write!(o, "{i}");
        }
        Param::Real(x) => o.push_str(&real(*x)),
        Param::Str(s) => o.push_str(&string(s)),
        Param::Enum(e) => {
            let _ = write!(o, ".{e}.");
        }
        Param::Ref(r) => {
            let _ = write!(o, "#{r}");
        }
        Param::List(v) => {
            o.push('(');
            params(v, o);
            o.push(')');
        }
        Param::Typed(name, v) => {
            o.push_str(name);
            o.push('(');
            params(v, o);
            o.push(')');
        }
        Param::Binary(b) => {
            let _ = write!(o, "\"{b}\"");
        }
        Param::Null => o.push('$'),
        Param::Derived => o.push('*'),
    }
}

fn params(v: &[Param], o: &mut String) {
    for (i, p) in v.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        param(p, o);
    }
}

/// A DATA section under construction. Entities are numbered from 1 in the order they are added;
/// adding an entity identical to an earlier one returns the earlier one's id.
#[derive(Default)]
pub struct Writer {
    lines: Vec<String>,
    shared: HashMap<String, u64>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, body: String) -> u64 {
        if let Some(id) = self.shared.get(&body) {
            return *id;
        }
        let id = self.push_distinct(&body);
        self.shared.insert(body, id);
        id
    }

    fn push_distinct(&mut self, body: &str) -> u64 {
        let id = self.lines.len() as u64 + 1;
        self.lines.push(format!("#{id}={body};"));
        id
    }

    fn simple(name: &str, p: &[Param]) -> String {
        let mut body = format!("{name}(");
        params(p, &mut body);
        body.push(')');
        body
    }

    /// A simple entity instance, shared with an identical earlier one.
    pub fn add(&mut self, name: &str, p: &[Param]) -> u64 {
        self.push(Self::simple(name, p))
    }

    /// A simple entity instance of its own, even when an identical one exists: an entity whose
    /// identity matters (two vertices at one point are two vertices; a product is one product).
    pub fn add_distinct(&mut self, name: &str, p: &[Param]) -> u64 {
        self.push_distinct(&Self::simple(name, p))
    }

    /// A complex entity instance; its records are written in alphabetical order, as Part 21 asks.
    pub fn add_complex(&mut self, records: &[(&str, Vec<Param>)]) -> u64 {
        let mut sorted: Vec<&(&str, Vec<Param>)> = records.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        let mut body = String::from("(");
        for (name, p) in sorted {
            body.push_str(name);
            body.push('(');
            params(p, &mut body);
            body.push(')');
        }
        body.push(')');
        self.push(body)
    }

    /// Entities added so far.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The DATA section's entity lines.
    pub fn data(&self) -> String {
        let mut o = String::new();
        for l in &self.lines {
            o.push_str(l);
            o.push('\n');
        }
        o
    }

    /// A whole exchange file around the entities written so far.
    pub fn file(&self, name: &str, schema: &str) -> String {
        format!(
            "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME({},'',(''),(''),'SolidCraft','SolidCraft','');\nFILE_SCHEMA(({}));\nENDSEC;\nDATA;\n{}ENDSEC;\nEND-ISO-10303-21;\n",
            string(name),
            string(schema),
            self.data()
        )
    }
}

fn s() -> Param {
    Param::Str(String::new())
}
fn r(id: u64) -> Param {
    Param::Ref(id)
}
fn f(x: f64) -> Param {
    Param::Real(x)
}
fn e(name: &str) -> Param {
    Param::Enum(name.into())
}
fn list(v: Vec<Param>) -> Param {
    Param::List(v)
}
fn reals(v: &[f64]) -> Param {
    list(v.iter().map(|x| f(*x)).collect())
}
fn ints(v: &[usize]) -> Param {
    list(v.iter().map(|x| Param::Int(*x as i64)).collect())
}
fn logical(l: Logical) -> Param {
    e(match l {
        Logical::True => "T",
        Logical::False => "F",
        Logical::Unknown => "U",
    })
}
fn boolean(b: bool) -> Param {
    e(if b { "T" } else { "F" })
}

fn curve_form(f: CurveForm) -> Param {
    e(match f {
        CurveForm::PolylineForm => "POLYLINE_FORM",
        CurveForm::CircularArc => "CIRCULAR_ARC",
        CurveForm::EllipticArc => "ELLIPTIC_ARC",
        CurveForm::ParabolicArc => "PARABOLIC_ARC",
        CurveForm::HyperbolicArc => "HYPERBOLIC_ARC",
        CurveForm::Unspecified => "UNSPECIFIED",
    })
}

fn surface_form(f: SurfaceForm) -> Param {
    e(match f {
        SurfaceForm::PlaneSurf => "PLANE_SURF",
        SurfaceForm::CylindricalSurf => "CYLINDRICAL_SURF",
        SurfaceForm::ConicalSurf => "CONICAL_SURF",
        SurfaceForm::SphericalSurf => "SPHERICAL_SURF",
        SurfaceForm::ToroidalSurf => "TOROIDAL_SURF",
        SurfaceForm::SurfOfRevolution => "SURF_OF_REVOLUTION",
        SurfaceForm::RuledSurf => "RULED_SURF",
        SurfaceForm::GeneralisedCone => "GENERALISED_CONE",
        SurfaceForm::QuadricSurf => "QUADRIC_SURF",
        SurfaceForm::SurfOfLinearExtrusion => "SURF_OF_LINEAR_EXTRUSION",
        SurfaceForm::Unspecified => "UNSPECIFIED",
    })
}

fn knot_spec(k: KnotSpec) -> Param {
    e(match k {
        KnotSpec::UniformKnots => "UNIFORM_KNOTS",
        KnotSpec::QuasiUniformKnots => "QUASI_UNIFORM_KNOTS",
        KnotSpec::PiecewiseBezierKnots => "PIECEWISE_BEZIER_KNOTS",
        KnotSpec::Unspecified => "UNSPECIFIED",
    })
}

/// Writes geometry into a [`Writer`]. Lengths are written in millimetres and angles in radians:
/// the representation context the caller writes must say so.
pub struct GeomWriter<'w> {
    pub w: &'w mut Writer,
    /// The 2D parameter-space context pcurves refer to, once written.
    pcurve_context: Option<u64>,
}

impl<'w> GeomWriter<'w> {
    pub fn new(w: &'w mut Writer) -> Self {
        GeomWriter { w, pcurve_context: None }
    }

    fn coords(p: P3, dim: usize) -> Param {
        reals(p.get(..dim).unwrap_or(&p))
    }

    pub fn point(&mut self, p: P3) -> u64 {
        self.point_in(p, 3)
    }

    fn point_in(&mut self, p: P3, dim: usize) -> u64 {
        self.w.add("CARTESIAN_POINT", &[s(), Self::coords(p, dim)])
    }

    fn direction_in(&mut self, d: P3, dim: usize) -> u64 {
        self.w.add("DIRECTION", &[s(), Self::coords(d, dim)])
    }

    pub fn direction(&mut self, d: P3) -> u64 {
        self.direction_in(d, 3)
    }

    fn vector_in(&mut self, d: P3, magnitude: f64, dim: usize) -> u64 {
        let d = self.direction_in(d, dim);
        self.w.add("VECTOR", &[s(), r(d), f(magnitude)])
    }

    fn placement_in(&mut self, p: &Placement, dim: usize) -> u64 {
        let o = self.point_in(p.origin, dim);
        let x = self.direction_in(p.ref_direction, dim);
        if dim == 2 {
            self.w.add("AXIS2_PLACEMENT_2D", &[s(), r(o), r(x)])
        } else {
            let z = self.direction_in(p.axis, dim);
            self.w.add("AXIS2_PLACEMENT_3D", &[s(), r(o), r(z), r(x)])
        }
    }

    pub fn placement(&mut self, p: &Placement) -> u64 {
        self.placement_in(p, 3)
    }

    pub fn axis1(&mut self, a: &Axis1) -> u64 {
        let o = self.point(a.origin);
        let d = self.direction(a.direction);
        self.w.add("AXIS1_PLACEMENT", &[s(), r(o), r(d)])
    }

    /// A 3D curve.
    pub fn curve(&mut self, c: &Curve) -> u64 {
        self.curve_in(c, 3)
    }

    fn curve_in(&mut self, c: &Curve, dim: usize) -> u64 {
        match c {
            Curve::Line { origin, direction, magnitude } => {
                let p = self.point_in(*origin, dim);
                let v = self.vector_in(*direction, *magnitude, dim);
                self.w.add("LINE", &[s(), r(p), r(v)])
            }
            Curve::Circle { position, radius } => {
                let p = self.placement_in(position, dim);
                self.w.add("CIRCLE", &[s(), r(p), f(*radius)])
            }
            Curve::Ellipse { position, semi_axis_1, semi_axis_2 } => {
                let p = self.placement_in(position, dim);
                self.w.add("ELLIPSE", &[s(), r(p), f(*semi_axis_1), f(*semi_axis_2)])
            }
            Curve::Hyperbola { position, semi_axis, semi_imag_axis } => {
                let p = self.placement_in(position, dim);
                self.w.add("HYPERBOLA", &[s(), r(p), f(*semi_axis), f(*semi_imag_axis)])
            }
            Curve::Parabola { position, focal_dist } => {
                let p = self.placement_in(position, dim);
                self.w.add("PARABOLA", &[s(), r(p), f(*focal_dist)])
            }
            Curve::BSpline(b) => self.bspline_curve(b, dim),
            Curve::Polyline(pts) => {
                let ids = pts.iter().map(|p| r(self.point_in(*p, dim))).collect();
                self.w.add("POLYLINE", &[s(), list(ids)])
            }
            Curve::Trimmed { basis, trim_1, trim_2, sense, preference } => {
                let b = self.curve_in(basis, dim);
                let t1 = self.trims(trim_1, dim);
                let t2 = self.trims(trim_2, dim);
                let pref = e(match preference {
                    TrimPreference::Cartesian => "CARTESIAN",
                    TrimPreference::Parameter => "PARAMETER",
                    TrimPreference::Unspecified => "UNSPECIFIED",
                });
                self.w.add("TRIMMED_CURVE", &[s(), r(b), t1, t2, boolean(*sense), pref])
            }
            Curve::SurfaceCurve { kind, curve_3d, associated, master } => {
                let c3 = self.curve_in(curve_3d, 3);
                let items = associated
                    .iter()
                    .map(|a| match a {
                        Associated::Pcurve(pc) => {
                            let surf = self.surface(&pc.surface);
                            let c2 = self.curve_in(&pc.curve, 2);
                            let ctx = self.pcurve_context();
                            let rep = self.w.add("DEFINITIONAL_REPRESENTATION", &[s(), list(vec![r(c2)]), r(ctx)]);
                            r(self.w.add("PCURVE", &[s(), r(surf), r(rep)]))
                        }
                        Associated::Surface(sf) => r(self.surface(sf)),
                    })
                    .collect();
                let name = match kind {
                    SurfaceCurveKind::Surface => "SURFACE_CURVE",
                    SurfaceCurveKind::Seam => "SEAM_CURVE",
                    SurfaceCurveKind::Intersection => "INTERSECTION_CURVE",
                    SurfaceCurveKind::BoundedSurface => "BOUNDED_SURFACE_CURVE",
                };
                let m = e(match master {
                    PreferredRep::Curve3d => "CURVE_3D",
                    PreferredRep::PcurveS1 => "PCURVE_S1",
                    PreferredRep::PcurveS2 => "PCURVE_S2",
                });
                self.w.add(name, &[s(), r(c3), list(items), m])
            }
            Curve::OffsetCurve3d { basis, distance, self_intersect, ref_direction } => {
                let b = self.curve_in(basis, 3);
                let d = self.direction(*ref_direction);
                self.w.add("OFFSET_CURVE_3D", &[s(), r(b), f(*distance), logical(*self_intersect), r(d)])
            }
        }
    }

    fn pcurve_context(&mut self) -> u64 {
        if let Some(c) = self.pcurve_context {
            return c;
        }
        let c = self.w.add_complex(&[
            ("GEOMETRIC_REPRESENTATION_CONTEXT", vec![Param::Int(2)]),
            ("PARAMETRIC_REPRESENTATION_CONTEXT", vec![]),
            ("REPRESENTATION_CONTEXT", vec![Param::Str("2D SPACE".into()), s()]),
        ]);
        self.pcurve_context = Some(c);
        c
    }

    fn trims(&mut self, t: &[Trim], dim: usize) -> Param {
        list(
            t.iter()
                .map(|t| match t {
                    Trim::Point(p) => r(self.point_in(*p, dim)),
                    Trim::Parameter(x) => Param::Typed("PARAMETER_VALUE".into(), vec![f(*x)]),
                })
                .collect(),
        )
    }

    fn bspline_curve(&mut self, b: &BSplineCurve, dim: usize) -> u64 {
        let ctrl = list(b.points.iter().map(|p| r(self.point_in(*p, dim))).collect());
        let common = vec![Param::Int(b.degree as i64), ctrl, curve_form(b.form), logical(b.closed), logical(b.self_intersect)];
        let knots = vec![ints(&b.multiplicities), reals(&b.knots), knot_spec(b.knot_spec)];
        match &b.weights {
            None => {
                let mut p = vec![s()];
                p.extend(common);
                p.extend(knots);
                self.w.add("B_SPLINE_CURVE_WITH_KNOTS", &p)
            }
            Some(w) => self.w.add_complex(&[
                ("BOUNDED_CURVE", vec![]),
                ("B_SPLINE_CURVE", common),
                ("B_SPLINE_CURVE_WITH_KNOTS", knots),
                ("CURVE", vec![]),
                ("GEOMETRIC_REPRESENTATION_ITEM", vec![]),
                ("RATIONAL_B_SPLINE_CURVE", vec![reals(w)]),
                ("REPRESENTATION_ITEM", vec![s()]),
            ]),
        }
    }

    /// A surface.
    pub fn surface(&mut self, sf: &Surface) -> u64 {
        match sf {
            Surface::Plane { position } => {
                let p = self.placement(position);
                self.w.add("PLANE", &[s(), r(p)])
            }
            Surface::Cylinder { position, radius } => {
                let p = self.placement(position);
                self.w.add("CYLINDRICAL_SURFACE", &[s(), r(p), f(*radius)])
            }
            Surface::Cone { position, radius, semi_angle } => {
                let p = self.placement(position);
                self.w.add("CONICAL_SURFACE", &[s(), r(p), f(*radius), f(*semi_angle)])
            }
            Surface::Sphere { position, radius } => {
                let p = self.placement(position);
                self.w.add("SPHERICAL_SURFACE", &[s(), r(p), f(*radius)])
            }
            Surface::Torus { position, major_radius, minor_radius } => {
                let p = self.placement(position);
                self.w.add("TOROIDAL_SURFACE", &[s(), r(p), f(*major_radius), f(*minor_radius)])
            }
            Surface::DegenerateTorus { position, major_radius, minor_radius, select_outer } => {
                let p = self.placement(position);
                self.w.add("DEGENERATE_TOROIDAL_SURFACE", &[s(), r(p), f(*major_radius), f(*minor_radius), boolean(*select_outer)])
            }
            Surface::BSpline(b) => self.bspline_surface(b),
            Surface::Revolution { profile, axis } => {
                let c = self.curve(profile);
                let a = self.axis1(axis);
                self.w.add("SURFACE_OF_REVOLUTION", &[s(), r(c), r(a)])
            }
            Surface::Extrusion { profile, direction, magnitude } => {
                let c = self.curve(profile);
                let v = self.vector_in(*direction, *magnitude, 3);
                self.w.add("SURFACE_OF_LINEAR_EXTRUSION", &[s(), r(c), r(v)])
            }
            Surface::Offset { basis, distance, self_intersect } => {
                let b = self.surface(basis);
                self.w.add("OFFSET_SURFACE", &[s(), r(b), f(*distance), logical(*self_intersect)])
            }
            Surface::RectangularTrimmed { basis, u1, u2, v1, v2, u_sense, v_sense } => {
                let b = self.surface(basis);
                self.w.add("RECTANGULAR_TRIMMED_SURFACE", &[s(), r(b), f(*u1), f(*u2), f(*v1), f(*v2), boolean(*u_sense), boolean(*v_sense)])
            }
        }
    }

    fn bspline_surface(&mut self, b: &BSplineSurface) -> u64 {
        let ctrl = list(b.points.iter().map(|row| list(row.iter().map(|p| r(self.point(*p))).collect())).collect());
        let common = vec![
            Param::Int(b.u_degree as i64),
            Param::Int(b.v_degree as i64),
            ctrl,
            surface_form(b.form),
            logical(b.u_closed),
            logical(b.v_closed),
            logical(b.self_intersect),
        ];
        let knots = vec![ints(&b.u_multiplicities), ints(&b.v_multiplicities), reals(&b.u_knots), reals(&b.v_knots), knot_spec(b.knot_spec)];
        match &b.weights {
            None => {
                let mut p = vec![s()];
                p.extend(common);
                p.extend(knots);
                self.w.add("B_SPLINE_SURFACE_WITH_KNOTS", &p)
            }
            Some(w) => self.w.add_complex(&[
                ("BOUNDED_SURFACE", vec![]),
                ("B_SPLINE_SURFACE", common),
                ("B_SPLINE_SURFACE_WITH_KNOTS", knots),
                ("GEOMETRIC_REPRESENTATION_ITEM", vec![]),
                ("RATIONAL_B_SPLINE_SURFACE", vec![list(w.iter().map(|row| reals(row)).collect())]),
                ("REPRESENTATION_ITEM", vec![s()]),
                ("SURFACE", vec![]),
            ]),
        }
    }
}
