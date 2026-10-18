//! FFI wrappers for unit `query.loop`.
//!
//! Shim: `harness/cpp/shim/query_loop.cpp`.
//! Upstream: `source/SMLib/src/SmLoop.cpp` (`SmLoop::ContainsUVPoint`) on a one-face
//! B-rep built with `SmBrepConstructor` (`source/SMLib/src/SmBrepData.cpp`).
//!
//! | Rust         | shim                     | upstream                  |
//! |--------------|--------------------------|---------------------------|
//! | [`contains`] | `sc_query_loop_contains` | `SmLoop::ContainsUVPoint` |
//!
//! Input layout (all `f64`): `face_tol vertex_loop u v tol3d`, the surface block, `n_edges`
//! then per edge `curve-block d0 d1 tol`, `n_edgeuses` then per edge use
//! `edge orient(1 = SAME, 2 = OPPOSITE) n p m cpts(x y w) knots` (a 2D B-spline UV trim
//! curve). Surface block: `0 O[3] X[3] Y[3] su sv d[4]` (`SmPlane`), `1 n m p q r s
//! net(x y z w) U V` (`SmBSplineSurface`), `2 gen P[3] A[3] swap`
//! (`SmSurfOfRevolution::CreateCanonical`, generator `0 a[3] b[3]` line or
//! `1 c[3] x[3] y[3] d0 d1 r` circle). Curve block: `0 a[3] b[3]` (`SmLine`),
//! `1 c[3] x[3] y[3] d0 d1 r` (`SmCircle`), `2 n p m cpts(x y z w) knots`
//! (`SmBSplineCurve`).
//!
//! Guards (inputs that are undefined upstream): NURB sizes must be consistent (degree
//! `1..=MAX_DEGREE`, `n >= p`, `m = n + p + 1`, non-decreasing knots with a non-empty
//! domain, positive weights or `NL_NOW`), every value finite; an edge is used at most twice (a third
//! use takes the non-manifold radial-sector path of the constructor); every edge use's
//! interval lies in its UV trim curve's domain (evaluations outside it fail and
//! `ContainsUVPoint` then reads uninitialised outputs); the first edge use's interval is
//! longer than `SM_EFF_ZERO` (otherwise the end of the walk is read before it is ever
//! written); a vertex loop has no edge uses. The shim itself rejects a point the surface
//! cannot evaluate (build status `5`).

use crate::validate::bad;
use crate::FfiError;

/// Largest degree accepted for NURB curves and surfaces.
pub const MAX_DEGREE: usize = 8;
/// Largest control point count per direction.
pub const MAX_POINTS: usize = 64;
/// Largest number of edges or edge uses.
pub const MAX_EDGES: usize = 64;
/// Upstream `NL_NOW`: the weight of a non-rational control point.
pub const NOW: f64 = -f64::MAX;
/// Upstream `SM_EFF_ZERO`.
const EFF_ZERO: f64 = 1.0e-12;

mod sys {
    extern "C" {
        pub fn sc_query_loop_contains(input: *const f64, n_in: i64, out: *mut f64) -> i32;
    }
}

/// A NURB curve: control points `[x, y, z, w]` (for a 2D curve `z` is ignored), degree
/// and knots.
#[derive(Debug, Clone, PartialEq)]
pub struct Nurb {
    /// Control points (`Pw`, homogeneous as stored).
    pub cpts: Vec<[f64; 4]>,
    /// Degree.
    pub p: usize,
    /// Knots (`m + 1 = n + p + 2` values).
    pub knots: Vec<f64>,
}

/// A 3D edge curve.
#[derive(Debug, Clone, PartialEq)]
pub enum Curve3 {
    /// `SmLine(a, b, 3)`.
    Line {
        /// Start point.
        a: [f64; 3],
        /// End point.
        b: [f64; 3],
    },
    /// `SmCircle(c, x, y, [d0, d1], r, 3)`.
    Circle {
        /// Centre.
        c: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// Domain.
        d: [f64; 2],
        /// Radius.
        r: f64,
    },
    /// `SmBSplineCurve(nurb, 3)`.
    BSpline(Nurb),
}

/// The generator of a surface of revolution.
#[derive(Debug, Clone, PartialEq)]
pub enum Gen {
    /// `SmLine(a, b, 3)` with its NURB made.
    Line {
        /// Start point.
        a: [f64; 3],
        /// End point.
        b: [f64; 3],
    },
    /// `SmCircle(c, x, y, [d0, d1], r, 3)`.
    Circle {
        /// Centre.
        c: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// Domain.
        d: [f64; 2],
        /// Radius.
        r: f64,
    },
}

/// The face's surface.
#[derive(Debug, Clone, PartialEq)]
pub enum Surf {
    /// `SmPlane(o, x, y, scale, SmExtent2d(d0, d1, d2, d3))`.
    Plane {
        /// Origin.
        o: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// UV scale.
        scale: [f64; 2],
        /// Domain `[umin, vmin, umax, vmax]`.
        domain: [f64; 4],
    },
    /// `SmBSplineSurface(nurb)`: net rows in u.
    BSpline {
        /// Control net `[x, y, z, w]`, `(n + 1)` rows of `(m + 1)`.
        net: Vec<Vec<[f64; 4]>>,
        /// Degree in u.
        p: usize,
        /// Degree in v.
        q: usize,
        /// U knots.
        u: Vec<f64>,
        /// V knots.
        v: Vec<f64>,
    },
    /// `SmSurfOfRevolution::CreateCanonical(gen, point, axis, swap)`.
    Revolution {
        /// Generator.
        gen: Gen,
        /// Axis point.
        point: [f64; 3],
        /// Axis direction.
        axis: [f64; 3],
        /// Swap u and v.
        swap: bool,
    },
}

/// An edge: 3D curve, interval and zone tolerance.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// The curve.
    pub curve: Curve3,
    /// Interval `[min, max]` (`SetLimits` parameters).
    pub interval: [f64; 2],
    /// Zone tolerance (`SetTolerance(t, FALSE, FALSE)`).
    pub tol: f64,
}

/// An edge use of the loop, in loop order.
#[derive(Debug, Clone, PartialEq)]
pub struct Edgeuse {
    /// Index into [`Case::edges`].
    pub edge: usize,
    /// `SM_OT_SAME` (else `SM_OT_OPPOSITE`).
    pub same: bool,
    /// The 2D B-spline UV trim curve.
    pub trim: Nurb,
}

/// One `ContainsUVPoint` call.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    /// The face zone tolerance (`SetTolerance(t, FALSE, FALSE)`).
    pub face_tol: f64,
    /// A single-vertex loop (no edge uses).
    pub vertex_loop: bool,
    /// The point.
    pub uv: [f64; 2],
    /// `dTol3d` (unused upstream).
    pub tol3d: f64,
    /// The surface.
    pub surface: Surf,
    /// The edges.
    pub edges: Vec<Edge>,
    /// The edge uses in loop order.
    pub edgeuses: Vec<Edgeuse>,
}

/// The shim's answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Answer {
    /// `0` built and classified; else the failing build step (`1` surface, `3` a
    /// constructor step, `4` the realised loop differs from the request, `5` the point
    /// cannot be evaluated).
    pub build: i32,
    /// `SmPointObjectContainmentType`.
    pub containment: i32,
    /// Index of the on-edge, `-1` for none.
    pub on_edge: i64,
    /// The edge parameter.
    pub edge_param: f64,
    /// `SmOrientType`.
    pub orientation: i32,
}

fn finite(v: &[f64], what: &str) -> Result<(), FfiError> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(bad(format!("{what}: non-finite value")))
    }
}

/// Checks a knot vector for `n + 1` points of degree `p`; returns its domain.
fn knots_ok(knots: &[f64], n_pts: usize, p: usize, what: &str) -> Result<[f64; 2], FfiError> {
    if !(1..=MAX_DEGREE).contains(&p) || n_pts < p + 1 || n_pts > MAX_POINTS {
        return Err(bad(format!("{what}: degree {p} with {n_pts} points")));
    }
    if knots.len() != n_pts + p + 1 {
        return Err(bad(format!("{what}: {} knots", knots.len())));
    }
    finite(knots, what)?;
    if knots.windows(2).any(|w| w.get(1) < w.first()) {
        return Err(bad(format!("{what}: decreasing knots")));
    }
    let lo = knots.get(p).copied().unwrap_or(0.0);
    let hi = knots.get(n_pts).copied().unwrap_or(0.0);
    if hi <= lo {
        return Err(bad(format!("{what}: empty domain")));
    }
    Ok([lo, hi])
}

fn weights_ok(pts: &[[f64; 4]], what: &str) -> Result<(), FfiError> {
    for c in pts {
        finite(c, what)?;
        // `NL_NOW` (-DBL_MAX) marks a non-rational point
        if c[3] <= 0.0 && c[3] != NOW {
            return Err(bad(format!("{what}: non-positive weight")));
        }
    }
    Ok(())
}

/// Checks a NURB curve; returns its domain.
fn nurb_ok(c: &Nurb, what: &str) -> Result<[f64; 2], FfiError> {
    let d = knots_ok(&c.knots, c.cpts.len(), c.p, what)?;
    weights_ok(&c.cpts, what)?;
    Ok(d)
}

fn push_nurb(o: &mut Vec<f64>, c: &Nurb, dim: usize) {
    let n = c.cpts.len().saturating_sub(1);
    let m = c.knots.len().saturating_sub(1);
    o.extend([n as f64, c.p as f64, m as f64]);
    for p in &c.cpts {
        if dim == 3 {
            o.extend_from_slice(p);
        } else {
            o.extend([p[0], p[1], p[3]]);
        }
    }
    o.extend_from_slice(&c.knots);
}

fn push_circle(o: &mut Vec<f64>, c: &[f64; 3], x: &[f64; 3], y: &[f64; 3], d: &[f64; 2], r: f64) {
    o.extend_from_slice(c);
    o.extend_from_slice(x);
    o.extend_from_slice(y);
    o.extend([d[0], d[1], r]);
}

fn push_surface(o: &mut Vec<f64>, s: &Surf) -> Result<(), FfiError> {
    match s {
        Surf::Plane {
            o: org,
            x,
            y,
            scale,
            domain,
        } => {
            for v in [&org[..], x, y, scale, domain] {
                finite(v, "plane")?;
            }
            o.push(0.0);
            o.extend_from_slice(org);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend_from_slice(scale);
            o.extend_from_slice(domain);
        }
        Surf::BSpline { net, p, q, u, v } => {
            let rows = net.len();
            let cols = net.first().map_or(0, Vec::len);
            if net.iter().any(|r| r.len() != cols) {
                return Err(bad("surface: ragged net"));
            }
            knots_ok(u, rows, *p, "surface u")?;
            knots_ok(v, cols, *q, "surface v")?;
            for r in net {
                weights_ok(r, "surface")?;
            }
            o.push(1.0);
            o.extend([
                (rows - 1) as f64,
                (cols - 1) as f64,
                *p as f64,
                *q as f64,
                (u.len() - 1) as f64,
                (v.len() - 1) as f64,
            ]);
            for r in net {
                for c in r {
                    o.extend_from_slice(c);
                }
            }
            o.extend_from_slice(u);
            o.extend_from_slice(v);
        }
        Surf::Revolution {
            gen,
            point,
            axis,
            swap,
        } => {
            o.push(2.0);
            match gen {
                Gen::Line { a, b } => {
                    finite(a, "generator")?;
                    finite(b, "generator")?;
                    o.push(0.0);
                    o.extend_from_slice(a);
                    o.extend_from_slice(b);
                }
                Gen::Circle { c, x, y, d, r } => {
                    for v in [&c[..], x, y, d, &[*r]] {
                        finite(v, "generator")?;
                    }
                    o.push(1.0);
                    push_circle(o, c, x, y, d, *r);
                }
            }
            finite(point, "axis point")?;
            finite(axis, "axis")?;
            o.extend_from_slice(point);
            o.extend_from_slice(axis);
            o.push(if *swap { 1.0 } else { 0.0 });
        }
    }
    Ok(())
}

fn push_curve(o: &mut Vec<f64>, c: &Curve3) -> Result<(), FfiError> {
    match c {
        Curve3::Line { a, b } => {
            finite(a, "line")?;
            finite(b, "line")?;
            o.push(0.0);
            o.extend_from_slice(a);
            o.extend_from_slice(b);
        }
        Curve3::Circle { c, x, y, d, r } => {
            for v in [&c[..], x, y, d, &[*r]] {
                finite(v, "circle")?;
            }
            o.push(1.0);
            push_circle(o, c, x, y, d, *r);
        }
        Curve3::BSpline(n) => {
            nurb_ok(n, "edge curve")?;
            o.push(2.0);
            push_nurb(o, n, 3);
        }
    }
    Ok(())
}

/// The flat shim input of `case` (validated).
fn flatten(case: &Case) -> Result<Vec<f64>, FfiError> {
    finite(&[case.face_tol, case.tol3d, case.uv[0], case.uv[1]], "case")?;
    if case.edges.len() > MAX_EDGES || case.edgeuses.len() > MAX_EDGES {
        return Err(bad("too many edges"));
    }
    if case.vertex_loop != case.edgeuses.is_empty() {
        return Err(bad("a vertex loop has no edge uses, an edge loop has some"));
    }
    let mut uses = vec![0usize; case.edges.len()];
    for (i, eu) in case.edgeuses.iter().enumerate() {
        let edge = case
            .edges
            .get(eu.edge)
            .ok_or_else(|| bad(format!("edge use {i}: edge {}", eu.edge)))?;
        if let Some(u) = uses.get_mut(eu.edge) {
            *u += 1;
            if *u > 2 {
                return Err(bad(format!("edge {} used more than twice", eu.edge)));
            }
        }
        let [lo, hi] = nurb_ok(&eu.trim, "trim")?;
        let [a, b] = edge.interval;
        finite(&[a, b, edge.tol], "edge")?;
        if a > b || a < lo || b > hi {
            return Err(bad(format!(
                "edge use {i}: interval outside its trim curve"
            )));
        }
        if i == 0 && (b - a).abs() <= EFF_ZERO {
            return Err(bad("first edge use: degenerate interval"));
        }
    }

    let mut o = vec![
        case.face_tol,
        if case.vertex_loop { 1.0 } else { 0.0 },
        case.uv[0],
        case.uv[1],
        case.tol3d,
    ];
    push_surface(&mut o, &case.surface)?;
    o.push(case.edges.len() as f64);
    for e in &case.edges {
        push_curve(&mut o, &e.curve)?;
        finite(&e.interval, "edge interval")?;
        o.extend([e.interval[0], e.interval[1], e.tol]);
    }
    o.push(case.edgeuses.len() as f64);
    for eu in &case.edgeuses {
        o.extend([eu.edge as f64, if eu.same { 1.0 } else { 2.0 }]);
        push_nurb(&mut o, &eu.trim, 2);
    }
    Ok(o)
}

/// Builds the face of `case` and calls upstream `SmLoop::ContainsUVPoint` on its loop.
pub fn contains(case: &Case) -> Result<Answer, FfiError> {
    let input = flatten(case)?;
    let n_in = i64::try_from(input.len()).map_err(|_| bad("input too long"))?;
    let mut out = [0.0f64; 5];
    // SAFETY: `input` is valid for `n_in` reads and laid out as documented (validated
    // above); `out` is valid for the 5 writes the shim makes.
    let st = unsafe { sys::sc_query_loop_contains(input.as_ptr(), n_in, out.as_mut_ptr()) };
    crate::check(st)?;
    let [build, cls, on, param, orient] = out;
    Ok(Answer {
        build: build as i32,
        containment: cls as i32,
        on_edge: on as i64,
        edge_param: param,
        orientation: orient as i32,
    })
}
