//! A body as one flat `f64` array: the plain C data the C++ shim reads, so both
//! kernels build from exactly the same numbers.
//!
//! Integers and flags are stored as `f64` (exact below 2^53). Layout (version 1):
//!
//! ```text
//! body    : 21 1 kind nv ne nf ns  vertex*nv  edge*ne  face*nf  shell*ns
//! vertex  : x y z
//! edge    : start end same_sense t_start t_end curve
//! curve   : 0 origin[3] dir[3] magnitude                       LINE
//!         | 1 frame radius                                     CIRCLE
//!         | 2 frame semi1 semi2                                ELLIPSE
//!         | 3 degree n rational nk  point[3]*n  weight*n (if rational)  knot*nk  mult*nk
//! frame   : origin[3] x[3] y[3] z[3]
//! face    : same_sense surface nl loop*nl
//! loop    : outer orientation 0 n (edge forward)*n             edge loop
//!         | outer orientation 1 vertex                         vertex loop
//! surface : 0 frame | 1 frame r | 2 frame r semi_angle_deg | 3 frame r | 4 frame R r
//!         | 5 curve axis_origin[3] axis_dir[3]                 revolution
//!         | 6 curve dir[3] magnitude                           extrusion
//!         | 7 ud vd nu nv rational nuk nvk  point[3]*nu*nv (U major)  weight*nu*nv
//!             uknot*nuk umult*nuk vknot*nvk vmult*nvk          B-spline
//! shell   : closed orientation n face*n
//! ```
//!
//! `kind` is 0 manifold solid, 1 solid with voids, 2 faceted, 3 shell model.

use crate::map::{check_bspline_curve, check_knots};
use crate::model::{
    BSplineCurve, BSplineSurface, Body, BodyKind, Bound, Curve, Edge, Face, Frame, Loop,
    OrientedEdge, Shell, Surface, Vertex, P3,
};

/// First word of an encoded body.
pub const MAGIC: f64 = 21.0;
/// Layout version.
pub const VERSION: f64 = 1.0;

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

fn frame(o: &mut Vec<f64>, f: &Frame) {
    for v in [f.origin, f.x, f.y, f.z] {
        o.extend_from_slice(&v);
    }
}

fn curve(o: &mut Vec<f64>, c: &Curve) {
    match c {
        Curve::Line {
            origin,
            dir,
            magnitude,
        } => {
            o.push(0.0);
            o.extend_from_slice(origin);
            o.extend_from_slice(dir);
            o.push(*magnitude);
        }
        Curve::Circle { frame: f, radius } => {
            o.push(1.0);
            frame(o, f);
            o.push(*radius);
        }
        Curve::Ellipse {
            frame: f,
            semi1,
            semi2,
        } => {
            o.push(2.0);
            frame(o, f);
            o.push(*semi1);
            o.push(*semi2);
        }
        Curve::BSpline(s) => {
            o.push(3.0);
            o.push(f64::from(s.degree));
            o.push(s.points.len() as f64);
            o.push(b(s.weights.is_some()));
            o.push(s.knots.len() as f64);
            for p in &s.points {
                o.extend_from_slice(p);
            }
            if let Some(w) = &s.weights {
                o.extend_from_slice(w);
            }
            o.extend_from_slice(&s.knots);
            o.extend(s.mults.iter().map(|&m| f64::from(m)));
        }
    }
}

fn surface(o: &mut Vec<f64>, s: &Surface) {
    match s {
        Surface::Plane { frame: f } => {
            o.push(0.0);
            frame(o, f);
        }
        Surface::Cylinder { frame: f, radius } => {
            o.push(1.0);
            frame(o, f);
            o.push(*radius);
        }
        Surface::Cone {
            frame: f,
            radius,
            semi_angle_deg,
        } => {
            o.push(2.0);
            frame(o, f);
            o.push(*radius);
            o.push(*semi_angle_deg);
        }
        Surface::Sphere { frame: f, radius } => {
            o.push(3.0);
            frame(o, f);
            o.push(*radius);
        }
        Surface::Torus {
            frame: f,
            major,
            minor,
        } => {
            o.push(4.0);
            frame(o, f);
            o.push(*major);
            o.push(*minor);
        }
        Surface::Revolution {
            curve: c,
            axis_origin,
            axis_dir,
        } => {
            o.push(5.0);
            curve(o, c);
            o.extend_from_slice(axis_origin);
            o.extend_from_slice(axis_dir);
        }
        Surface::Extrusion {
            curve: c,
            dir,
            magnitude,
        } => {
            o.push(6.0);
            curve(o, c);
            o.extend_from_slice(dir);
            o.push(*magnitude);
        }
        Surface::BSpline(s) => {
            o.push(7.0);
            o.push(f64::from(s.u_degree));
            o.push(f64::from(s.v_degree));
            o.push(s.points.len() as f64);
            o.push(s.points.first().map_or(0, Vec::len) as f64);
            o.push(b(s.weights.is_some()));
            o.push(s.u_knots.len() as f64);
            o.push(s.v_knots.len() as f64);
            for row in &s.points {
                for p in row {
                    o.extend_from_slice(p);
                }
            }
            if let Some(w) = &s.weights {
                for row in w {
                    o.extend_from_slice(row);
                }
            }
            o.extend_from_slice(&s.u_knots);
            o.extend(s.u_mults.iter().map(|&m| f64::from(m)));
            o.extend_from_slice(&s.v_knots);
            o.extend(s.v_mults.iter().map(|&m| f64::from(m)));
        }
    }
}

/// Encodes a body (see the module docs for the layout).
pub fn encode(body: &Body) -> Vec<f64> {
    let mut o = vec![
        MAGIC,
        VERSION,
        match body.kind {
            BodyKind::ManifoldSolid => 0.0,
            BodyKind::SolidWithVoids => 1.0,
            BodyKind::Faceted => 2.0,
            BodyKind::ShellModel => 3.0,
        },
        body.vertices.len() as f64,
        body.edges.len() as f64,
        body.faces.len() as f64,
        body.shells.len() as f64,
    ];
    for v in &body.vertices {
        o.extend_from_slice(&v.point);
    }
    for e in &body.edges {
        o.push(e.start as f64);
        o.push(e.end as f64);
        o.push(b(e.same_sense));
        o.push(e.t_start);
        o.push(e.t_end);
        curve(&mut o, &e.curve);
    }
    for f in &body.faces {
        o.push(b(f.same_sense));
        surface(&mut o, &f.surface);
        o.push(f.loops.len() as f64);
        for l in &f.loops {
            o.push(b(l.outer));
            o.push(b(l.orientation));
            match &l.bound {
                Bound::Edges(es) => {
                    o.push(0.0);
                    o.push(es.len() as f64);
                    for e in es {
                        o.push(e.edge as f64);
                        o.push(b(e.forward));
                    }
                }
                Bound::Vertex(v) => {
                    o.push(1.0);
                    o.push(*v as f64);
                }
            }
        }
    }
    for s in &body.shells {
        o.push(b(s.closed));
        o.push(b(s.orientation));
        o.push(s.faces.len() as f64);
        o.extend(s.faces.iter().map(|&f| f as f64));
    }
    o
}

/// Reads an encoded body back, validating everything the C++ shim relies on: the
/// header, every count against the data left, every index, flags, finite numbers and
/// the B-spline invariants the importer guarantees. Nothing is allocated from a count
/// before the data for it has been read. STEP ids are not encoded (they come back 0).
pub fn decode(data: &[f64]) -> Result<Body, String> {
    let mut r = Rd { d: data, i: 0 };
    if r.num()? != MAGIC || r.num()? != VERSION {
        return Err("bad magic or version".into());
    }
    let kind = match r.index(4)? {
        0 => BodyKind::ManifoldSolid,
        1 => BodyKind::SolidWithVoids,
        2 => BodyKind::Faceted,
        _ => BodyKind::ShellModel,
    };
    let nv = r.count(3)?;
    let ne = r.count(6)?;
    let nf = r.count(3)?;
    let ns = r.count(3)?;
    let mut vertices = Vec::new();
    for _ in 0..nv {
        vertices.push(Vertex {
            step_id: 0,
            point: r.p3()?,
        });
    }
    let mut edges = Vec::new();
    for _ in 0..ne {
        let start = r.index(nv)?;
        let end = r.index(nv)?;
        let same_sense = r.flag()?;
        let t_start = r.num()?;
        let t_end = r.num()?;
        let curve = r.curve()?;
        edges.push(Edge {
            step_id: 0,
            start,
            end,
            curve,
            same_sense,
            t_start,
            t_end,
        });
    }
    let mut faces = Vec::new();
    for _ in 0..nf {
        let same_sense = r.flag()?;
        let surface = r.surface()?;
        let nl = r.count(3)?;
        let mut loops = Vec::new();
        for _ in 0..nl {
            let outer = r.flag()?;
            let orientation = r.flag()?;
            let bound = if r.flag()? {
                Bound::Vertex(r.index(nv)?)
            } else {
                let n = r.count(2)?;
                let mut es = Vec::new();
                for _ in 0..n {
                    es.push(OrientedEdge {
                        edge: r.index(ne)?,
                        forward: r.flag()?,
                    });
                }
                Bound::Edges(es)
            };
            loops.push(Loop {
                outer,
                orientation,
                bound,
            });
        }
        faces.push(Face {
            step_id: 0,
            surface,
            same_sense,
            loops,
        });
    }
    let mut shells = Vec::new();
    for _ in 0..ns {
        let closed = r.flag()?;
        let orientation = r.flag()?;
        let n = r.count(1)?;
        let mut fs = Vec::new();
        for _ in 0..n {
            fs.push(r.index(nf)?);
        }
        shells.push(Shell {
            step_id: 0,
            closed,
            orientation,
            faces: fs,
        });
    }
    if r.i != data.len() {
        return Err("trailing data".into());
    }
    Ok(Body {
        step_id: 0,
        name: None,
        kind,
        unit_mm: 1.0,
        uncertainty_mm: None,
        shells,
        faces,
        edges,
        vertices,
    })
}

struct Rd<'a> {
    d: &'a [f64],
    i: usize,
}

impl Rd<'_> {
    fn num(&mut self) -> Result<f64, String> {
        let x = *self.d.get(self.i).ok_or("truncated")?;
        self.i += 1;
        if x.is_finite() {
            Ok(x)
        } else {
            Err("non-finite number".into())
        }
    }

    /// An integer in `[0, n)`.
    fn index(&mut self, n: usize) -> Result<usize, String> {
        let x = self.num()?;
        if x >= 0.0 && x.fract() == 0.0 && x < n as f64 {
            Ok(x as usize)
        } else {
            Err(format!("index {x} out of [0, {n})"))
        }
    }

    /// A count of items at least `min_words` long each, checked against the data left.
    fn count(&mut self, min_words: usize) -> Result<usize, String> {
        let left = self.d.len().saturating_sub(self.i);
        self.index(left / min_words.max(1) + 1)
    }

    fn flag(&mut self) -> Result<bool, String> {
        Ok(self.index(2)? == 1)
    }

    fn p3(&mut self) -> Result<P3, String> {
        Ok([self.num()?, self.num()?, self.num()?])
    }

    fn unit(&mut self) -> Result<P3, String> {
        let v = self.p3()?;
        let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if (n - 1.0).abs() > 1e-9 {
            return Err("direction not unit".into());
        }
        Ok(v)
    }

    fn positive(&mut self) -> Result<f64, String> {
        let x = self.num()?;
        if x > 0.0 {
            Ok(x)
        } else {
            Err("not positive".into())
        }
    }

    fn frame(&mut self) -> Result<Frame, String> {
        Ok(Frame {
            origin: self.p3()?,
            x: self.unit()?,
            y: self.unit()?,
            z: self.unit()?,
        })
    }

    fn knots(&mut self, nk: usize) -> Result<(Vec<f64>, Vec<u32>), String> {
        let mut knots = Vec::new();
        for _ in 0..nk {
            knots.push(self.num()?);
        }
        let mut mults = Vec::new();
        for _ in 0..nk {
            mults.push(self.index(34)? as u32);
        }
        Ok((knots, mults))
    }

    fn curve(&mut self) -> Result<Curve, String> {
        Ok(match self.index(4)? {
            0 => Curve::Line {
                origin: self.p3()?,
                dir: self.unit()?,
                magnitude: self.positive()?,
            },
            1 => Curve::Circle {
                frame: self.frame()?,
                radius: self.positive()?,
            },
            2 => Curve::Ellipse {
                frame: self.frame()?,
                semi1: self.positive()?,
                semi2: self.positive()?,
            },
            _ => {
                let degree = self.index(33)? as u32;
                let n = self.count(3)?;
                let rational = self.flag()?;
                let nk = self.count(2)?;
                let mut points = Vec::new();
                for _ in 0..n {
                    points.push(self.p3()?);
                }
                let weights = if rational {
                    let mut w = Vec::new();
                    for _ in 0..n {
                        w.push(self.num()?);
                    }
                    Some(w)
                } else {
                    None
                };
                let (knots, mults) = self.knots(nk)?;
                if degree == 0 {
                    return Err("degree 0".into());
                }
                let b = BSplineCurve {
                    degree,
                    points,
                    weights,
                    knots,
                    mults,
                };
                check_bspline_curve(&b).map_err(|e| e.to_string())?;
                Curve::BSpline(b)
            }
        })
    }

    fn surface(&mut self) -> Result<Surface, String> {
        Ok(match self.index(8)? {
            0 => Surface::Plane {
                frame: self.frame()?,
            },
            1 => Surface::Cylinder {
                frame: self.frame()?,
                radius: self.positive()?,
            },
            2 => {
                let frame = self.frame()?;
                let radius = self.num()?;
                let semi_angle_deg = self.num()?;
                if radius < 0.0 || !(semi_angle_deg > 0.0 && semi_angle_deg < 90.0) {
                    return Err("bad cone".into());
                }
                Surface::Cone {
                    frame,
                    radius,
                    semi_angle_deg,
                }
            }
            3 => Surface::Sphere {
                frame: self.frame()?,
                radius: self.positive()?,
            },
            4 => Surface::Torus {
                frame: self.frame()?,
                major: self.positive()?,
                minor: self.positive()?,
            },
            5 => Surface::Revolution {
                curve: Box::new(self.curve()?),
                axis_origin: self.p3()?,
                axis_dir: self.unit()?,
            },
            6 => Surface::Extrusion {
                curve: Box::new(self.curve()?),
                dir: self.unit()?,
                magnitude: self.positive()?,
            },
            _ => self.bspline_surface()?,
        })
    }

    fn bspline_surface(&mut self) -> Result<Surface, String> {
        let ud = self.index(33)?;
        let vd = self.index(33)?;
        let nu = self.count(3)?;
        let nv = self.count(3)?;
        let rational = self.flag()?;
        let nuk = self.count(2)?;
        let nvk = self.count(2)?;
        if ud == 0 || vd == 0 {
            return Err("degree 0".into());
        }
        let total = nu.checked_mul(nv).ok_or("net too large")?;
        if total > self.d.len().saturating_sub(self.i) / 3 {
            return Err("net larger than the data".into());
        }
        let mut points = Vec::new();
        for _ in 0..nu {
            let mut row = Vec::new();
            for _ in 0..nv {
                row.push(self.p3()?);
            }
            points.push(row);
        }
        let weights = if rational {
            let mut rows = Vec::new();
            for _ in 0..nu {
                let mut row = Vec::new();
                for _ in 0..nv {
                    row.push(self.positive()?);
                }
                rows.push(row);
            }
            Some(rows)
        } else {
            None
        };
        let (u_knots, u_mults) = self.knots(nuk)?;
        let (v_knots, v_mults) = self.knots(nvk)?;
        let fits = |k: &[f64], m: &[u32], n: usize, d: usize| {
            crate::geom::expand_knots(k, m, n + d + 1).is_some()
        };
        if nu < ud + 1
            || nv < vd + 1
            || !fits(&u_knots, &u_mults, nu, ud)
            || !fits(&v_knots, &v_mults, nv, vd)
        {
            return Err("B-spline surface knots do not fit the net".into());
        }
        check_knots(&u_knots, &u_mults, ud).map_err(|e| e.to_string())?;
        check_knots(&v_knots, &v_mults, vd).map_err(|e| e.to_string())?;
        Ok(Surface::BSpline(BSplineSurface {
            u_degree: ud as u32,
            v_degree: vd as u32,
            points,
            weights,
            u_knots,
            u_mults,
            v_knots,
            v_mults,
        }))
    }
}
