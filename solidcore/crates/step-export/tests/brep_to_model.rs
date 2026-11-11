//! `brep_to_model` + `write` files read as ISO 10303-42 says: every face's normal points out of
//! the material and its loops run anticlockwise about that normal, so the solid's signed
//! volume is positive.
//!
//! The volume is computed here from the written file's entities alone (the Part 21 entity
//! graph, not our STEP reader): the divergence theorem, V = 1/3 ∮ p·n dA, face by face. A
//! planar face's ∮ p·n dA is p0·A where A = ½ Σ ∮ p × dp over its oriented loops (each edge
//! integrated along its 3D curve). A curved face's comes from its PCURVEs by Green's theorem
//! in (u, v), with `p·(S_u × S_v)` evaluated by the ISO formula of its surface entity. Each face
//! is measured twice: once by its loops' direction alone, once by its `same_sense` alone. The
//! two agree on a consistent face; both sum to the solid's volume.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::collections::BTreeMap;

use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::curve::curve::Curve;
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives as api;
use solidcore::sweep::api_brep::{create_linear_sweep, create_rotational_sweep};
use solidcore::topology::Brep;
use solidcore_record::step::p21::{self, Entity, Exchange, Param};
use step_export::brep_to_model::brep_to_model;

type P = [f64; 3];

fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(s: f64, a: P) -> P {
    [s * a[0], s * a[1], s * a[2]]
}
fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: P, b: P) -> P {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: P) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: P) -> P {
    mul(1.0 / norm(a), a)
}

/// Gauss–Legendre nodes and weights on [0, 1] (8 points).
fn gauss() -> [(f64, f64); 8] {
    let nodes = [
        (0.183_434_642_495_649_8, 0.362_683_783_378_362),
        (0.525_532_409_916_329, 0.313_706_645_877_887_3),
        (0.796_666_477_413_626_7, 0.222_381_034_453_374_5),
        (0.960_289_856_497_536_3, 0.101_228_536_290_376_3),
    ];
    let mut out = [(0.0, 0.0); 8];
    for (i, (x, w)) in nodes.iter().enumerate() {
        out[2 * i] = (0.5 - 0.5 * x, 0.5 * w);
        out[2 * i + 1] = (0.5 + 0.5 * x, 0.5 * w);
    }
    out
}

/// A curve of the file, by its ISO 10303-42 parameterisation.
#[derive(Clone, Debug)]
enum IsoCurve {
    Line {
        origin: P,
        step: P,
    },
    Circle {
        o: P,
        x: P,
        y: P,
        r: f64,
    },
    BSpline {
        degree: usize,
        points: Vec<P>,
        weights: Vec<f64>,
        knots: Vec<f64>,
    },
}

impl IsoCurve {
    fn at(&self, t: f64) -> P {
        match self {
            IsoCurve::Line { origin, step } => add(*origin, mul(t, *step)),
            IsoCurve::Circle { o, x, y, r } => {
                add(*o, add(mul(r * t.cos(), *x), mul(r * t.sin(), *y)))
            }
            IsoCurve::BSpline {
                degree,
                points,
                weights,
                knots,
            } => {
                let p = *degree;
                let n = points.len();
                // the span: knots[s] <= t < knots[s + 1], clamped to the curve's ends
                let mut s = p;
                while s + 1 < n && t >= knots[s + 1] {
                    s += 1;
                }
                let mut d: Vec<[f64; 4]> = (0..=p)
                    .map(|j| {
                        let i = s - p + j;
                        let w = weights[i];
                        let q = points[i];
                        [q[0] * w, q[1] * w, q[2] * w, w]
                    })
                    .collect();
                for r in 1..=p {
                    for j in (r..=p).rev() {
                        let i = s - p + j;
                        let a = (t - knots[i]) / (knots[i + p + 1 - r] - knots[i]);
                        let below = d[j - 1];
                        for (c, low) in d[j].iter_mut().zip(below) {
                            *c = (1.0 - a) * low + a * *c;
                        }
                    }
                }
                let h = d[p];
                [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
            }
        }
    }

    /// The parameter of a point on the curve.
    fn param_of(&self, q: P) -> f64 {
        match self {
            IsoCurve::Line { origin, step } => dot(sub(q, *origin), *step) / dot(*step, *step),
            IsoCurve::Circle { o, x, y, .. } => {
                let d = sub(q, *o);
                dot(d, *y).atan2(dot(d, *x))
            }
            IsoCurve::BSpline { knots, .. } => {
                let (lo, hi) = (knots[0], knots[knots.len() - 1]);
                let samples = 4096;
                let mut best = (f64::INFINITY, lo);
                for i in 0..=samples {
                    let t = lo + (hi - lo) * f64::from(i) / f64::from(samples);
                    let gap = norm(sub(self.at(t), q));
                    if gap < best.0 {
                        best = (gap, t);
                    }
                }
                // refine by bisection on the distance
                let mut step = (hi - lo) / f64::from(samples);
                let mut t = best.1;
                for _ in 0..60 {
                    step *= 0.5;
                    for c in [t - step, t + step] {
                        let c = c.clamp(lo, hi);
                        if norm(sub(self.at(c), q)) < norm(sub(self.at(t), q)) {
                            t = c;
                        }
                    }
                }
                t
            }
        }
    }
}

/// A surface of the file, by its ISO 10303-42 parameterisation (angles in radians).
#[derive(Clone, Debug)]
enum IsoSurface {
    Plane {
        o: P,
        z: P,
    },
    Cylinder {
        o: P,
        x: P,
        y: P,
        z: P,
        r: f64,
    },
    Cone {
        o: P,
        x: P,
        y: P,
        z: P,
        r: f64,
        tan: f64,
    },
    Sphere {
        o: P,
        x: P,
        y: P,
        z: P,
        r: f64,
    },
    Torus {
        o: P,
        x: P,
        y: P,
        z: P,
        major: f64,
        minor: f64,
    },
    Revolution {
        curve: IsoCurve,
        o: P,
        d: P,
    },
    Extrusion {
        curve: IsoCurve,
        step: P,
    },
}

impl IsoSurface {
    fn at(&self, u: f64, v: f64) -> P {
        let (su, cu) = u.sin_cos();
        let (sv, cv) = v.sin_cos();
        let around = |o: P, x: P, y: P, z: P, r: f64, h: f64| {
            add(add(o, mul(h, z)), add(mul(r * cu, x), mul(r * su, y)))
        };
        match self {
            IsoSurface::Plane { .. } => panic!("a plane is measured by its loops"),
            IsoSurface::Cylinder { o, x, y, z, r } => around(*o, *x, *y, *z, *r, v),
            IsoSurface::Cone { o, x, y, z, r, tan } => around(*o, *x, *y, *z, r + v * tan, v),
            IsoSurface::Sphere { o, x, y, z, r } => around(*o, *x, *y, *z, r * cv, r * sv),
            IsoSurface::Torus {
                o,
                x,
                y,
                z,
                major,
                minor,
            } => around(*o, *x, *y, *z, major + minor * cv, minor * sv),
            IsoSurface::Revolution { curve, o, d } => {
                // the generator's point at v, turned by u about the axis (right-handed)
                let q = sub(curve.at(v), *o);
                let along = dot(q, *d);
                let radial = sub(q, mul(along, *d));
                add(
                    add(*o, mul(along, *d)),
                    add(mul(cu, radial), mul(su, cross(*d, radial))),
                )
            }
            IsoSurface::Extrusion { curve, step } => add(curve.at(u), mul(v, *step)),
        }
    }

    /// The natural normal `S_u × S_v` (not unit), by central differences.
    fn normal(&self, u: f64, v: f64) -> P {
        let h = 1e-6;
        let su = mul(0.5 / h, sub(self.at(u + h, v), self.at(u - h, v)));
        let sv = mul(0.5 / h, sub(self.at(u, v + h), self.at(u, v - h)));
        cross(su, sv)
    }
}

/// The file's entity graph.
struct File {
    x: Exchange,
}

fn real(p: &Param) -> f64 {
    p.as_f64().unwrap_or_else(|| panic!("not a real: {p:?}"))
}
fn rid(p: &Param) -> u64 {
    p.as_ref_id()
        .unwrap_or_else(|| panic!("not a reference: {p:?}"))
}
fn list(p: &Param) -> &[Param] {
    p.as_list().unwrap_or_else(|| panic!("not a list: {p:?}"))
}
fn flag(p: &Param) -> bool {
    p.as_bool()
        .unwrap_or_else(|| panic!("not a boolean: {p:?}"))
}

/// The knots of a B-spline, each repeated by its multiplicity.
fn full_knots(mults: &[Param], knots: &[Param]) -> Vec<f64> {
    let mut out = Vec::new();
    for (m, k) in mults.iter().zip(knots) {
        for _ in 0..m.as_i64().unwrap() {
            out.push(real(k));
        }
    }
    out
}

impl File {
    fn e(&self, id: u64) -> &Entity {
        self.x
            .get(id)
            .unwrap_or_else(|| panic!("#{id} is not in the file"))
    }

    fn params(&self, id: u64, name: &str) -> &[Param] {
        let e = self.e(id);
        assert_eq!(e.name(), name, "#{id}");
        e.params()
    }

    fn point(&self, id: u64) -> P {
        let coords = list(&self.params(id, "CARTESIAN_POINT")[1]);
        [real(&coords[0]), real(&coords[1]), real(&coords[2])]
    }

    fn point2(&self, id: u64) -> [f64; 2] {
        let coords = list(&self.params(id, "CARTESIAN_POINT")[1]);
        [real(&coords[0]), real(&coords[1])]
    }

    fn direction(&self, id: u64) -> P {
        let r = list(&self.params(id, "DIRECTION")[1]);
        unit([real(&r[0]), real(&r[1]), real(&r[2])])
    }

    fn vector(&self, id: u64) -> P {
        let p = self.params(id, "VECTOR");
        mul(real(&p[2]), self.direction(rid(&p[1])))
    }

    /// origin, x, y, z of an `AXIS2_PLACEMENT_3D`.
    fn frame(&self, id: u64) -> (P, P, P, P) {
        let p = self.params(id, "AXIS2_PLACEMENT_3D");
        let o = self.point(rid(&p[1]));
        let z = self.direction(rid(&p[2]));
        let x0 = self.direction(rid(&p[3]));
        let x = unit(sub(x0, mul(dot(x0, z), z)));
        (o, x, cross(z, x), z)
    }

    fn vertex(&self, id: u64) -> P {
        self.point(rid(&self.params(id, "VERTEX_POINT")[1]))
    }

    fn curve(&self, id: u64) -> IsoCurve {
        let e = self.e(id);
        if let Some(p) = e.record("B_SPLINE_CURVE") {
            // the complex (rational) form
            let points: Vec<P> = list(&p[1]).iter().map(|q| self.point(rid(q))).collect();
            let k = e.record("B_SPLINE_CURVE_WITH_KNOTS").unwrap();
            let weights = e.record("RATIONAL_B_SPLINE_CURVE").map_or_else(
                || vec![1.0; points.len()],
                |w| list(&w[0]).iter().map(real).collect(),
            );
            return IsoCurve::BSpline {
                degree: p[0].as_i64().unwrap() as usize,
                knots: full_knots(list(&k[0]), list(&k[1])),
                points,
                weights,
            };
        }
        let p = e.params();
        match e.name() {
            "LINE" => IsoCurve::Line {
                origin: self.point(rid(&p[1])),
                step: self.vector(rid(&p[2])),
            },
            "CIRCLE" => {
                let (o, x, y, _) = self.frame(rid(&p[1]));
                IsoCurve::Circle {
                    o,
                    x,
                    y,
                    r: real(&p[2]),
                }
            }
            "B_SPLINE_CURVE_WITH_KNOTS" => {
                let points: Vec<P> = list(&p[2]).iter().map(|q| self.point(rid(q))).collect();
                IsoCurve::BSpline {
                    degree: p[1].as_i64().unwrap() as usize,
                    knots: full_knots(list(&p[6]), list(&p[7])),
                    weights: vec![1.0; points.len()],
                    points,
                }
            }
            "SURFACE_CURVE" | "SEAM_CURVE" => self.curve(rid(&p[1])),
            other => panic!("#{id}: curve {other}"),
        }
    }

    fn surface(&self, id: u64) -> IsoSurface {
        let e = self.e(id);
        let p = e.params();
        match e.name() {
            "PLANE" => {
                let (o, _, _, z) = self.frame(rid(&p[1]));
                IsoSurface::Plane { o, z }
            }
            "CYLINDRICAL_SURFACE" => {
                let (o, x, y, z) = self.frame(rid(&p[1]));
                IsoSurface::Cylinder {
                    o,
                    x,
                    y,
                    z,
                    r: real(&p[2]),
                }
            }
            "CONICAL_SURFACE" => {
                let (o, x, y, z) = self.frame(rid(&p[1]));
                IsoSurface::Cone {
                    o,
                    x,
                    y,
                    z,
                    r: real(&p[2]),
                    tan: real(&p[3]).tan(),
                }
            }
            "SPHERICAL_SURFACE" => {
                let (o, x, y, z) = self.frame(rid(&p[1]));
                IsoSurface::Sphere {
                    o,
                    x,
                    y,
                    z,
                    r: real(&p[2]),
                }
            }
            "TOROIDAL_SURFACE" => {
                let (o, x, y, z) = self.frame(rid(&p[1]));
                IsoSurface::Torus {
                    o,
                    x,
                    y,
                    z,
                    major: real(&p[2]),
                    minor: real(&p[3]),
                }
            }
            "SURFACE_OF_REVOLUTION" => {
                let axis = self.params(rid(&p[2]), "AXIS1_PLACEMENT");
                IsoSurface::Revolution {
                    curve: self.curve(rid(&p[1])),
                    o: self.point(rid(&axis[1])),
                    d: self.direction(rid(&axis[2])),
                }
            }
            "SURFACE_OF_LINEAR_EXTRUSION" => IsoSurface::Extrusion {
                curve: self.curve(rid(&p[1])),
                step: self.vector(rid(&p[2])),
            },
            other => panic!("#{id}: surface {other}"),
        }
    }

    /// The (u, v) polylines of an `EDGE_CURVE`'s `PCURVE`s on `surface`, each running from the
    /// edge's start to its end.
    fn pcurves(&self, edge: u64, surface: u64) -> Vec<Vec<[f64; 2]>> {
        let geometry = self.e(rid(&self.params(edge, "EDGE_CURVE")[3]));
        if !matches!(geometry.name(), "SURFACE_CURVE" | "SEAM_CURVE") {
            return Vec::new();
        }
        list(&geometry.params()[2])
            .iter()
            .filter_map(|pc| {
                let pc = self.params(rid(pc), "PCURVE");
                (rid(&pc[1]) == surface).then(|| {
                    let rep = self.params(rid(&pc[2]), "DEFINITIONAL_REPRESENTATION");
                    let curve = self.params(rid(&list(&rep[1])[0]), "B_SPLINE_CURVE_WITH_KNOTS");
                    assert_eq!(curve[1].as_i64(), Some(1), "pcurves are polylines");
                    list(&curve[2])
                        .iter()
                        .map(|q| self.point2(rid(q)))
                        .collect()
                })
            })
            .collect()
    }
}

/// One face's ∮ p·n dA (the file's), measured by its loops alone and by its `same_sense`
/// alone.
#[derive(Debug)]
struct FaceMeasure {
    id: u64,
    kind: String,
    same_sense: bool,
    by_loops: f64,
    by_sense: f64,
}

/// ½ ∮ p × dp along an edge from `a` to `b` on `curve`, the edge running with the curve
/// when `along`.
fn half_moment(curve: &IsoCurve, a: P, b: P, along: bool) -> P {
    if let IsoCurve::Line { .. } = curve {
        return mul(0.5, cross(a, b));
    }
    let t0 = curve.param_of(a);
    let mut t1 = curve.param_of(b);
    if let IsoCurve::Circle { .. } = curve {
        let tau = std::f64::consts::TAU;
        if along {
            while t1 <= t0 + 1e-12 {
                t1 += tau;
            }
        } else {
            while t1 >= t0 - 1e-12 {
                t1 -= tau;
            }
        }
    } else if norm(sub(a, b)) < 1e-9 {
        panic!("a closed B-spline edge");
    }
    let pieces = 256;
    let mut sum = [0.0; 3];
    for i in 0..pieces {
        let lo = t0 + (t1 - t0) * f64::from(i) / f64::from(pieces);
        let hi = t0 + (t1 - t0) * f64::from(i + 1) / f64::from(pieces);
        for (x, w) in gauss() {
            let t = lo + (hi - lo) * x;
            let h = 1e-6 * (1.0 + t.abs());
            let d = mul(0.5 / h, sub(curve.at(t + h), curve.at(t - h)));
            sum = add(sum, mul(0.5 * w * (hi - lo), cross(curve.at(t), d)));
        }
    }
    sum
}

fn measure(text: &str) -> Vec<FaceMeasure> {
    let file = File {
        x: p21::parse(text).expect("the file parses"),
    };
    let breps: Vec<u64> = file
        .x
        .entities
        .iter()
        .filter(|(_, e)| e.name() == "MANIFOLD_SOLID_BREP")
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(breps.len(), 1, "one solid");
    let shell = rid(&file.params(breps[0], "MANIFOLD_SOLID_BREP")[1]);
    let faces = list(&file.params(shell, "CLOSED_SHELL")[1]).to_vec();
    let mut out = Vec::new();
    for face in faces {
        let face = rid(&face);
        let p = file.params(face, "ADVANCED_FACE");
        let surface_id = rid(&p[2]);
        let same_sense = flag(&p[3]);
        let sigma = if same_sense { 1.0 } else { -1.0 };
        let surface = file.surface(surface_id);
        // each loop as its oriented edge uses: (edge, start, end, along the edge curve)
        let mut loops = Vec::new();
        for bound in list(&p[1]) {
            let b = file.e(rid(bound));
            assert!(matches!(b.name(), "FACE_OUTER_BOUND" | "FACE_BOUND"));
            let loop_ = file.params(rid(&b.params()[1]), "EDGE_LOOP");
            let mut uses = Vec::new();
            for oe in list(&loop_[1]) {
                let oe = file.params(rid(oe), "ORIENTED_EDGE");
                let edge = rid(&oe[3]);
                let ec = file.params(edge, "EDGE_CURVE");
                let (sv, ev) = (file.vertex(rid(&ec[1])), file.vertex(rid(&ec[2])));
                let along = flag(&oe[4]);
                // the edge runs from its start vertex to its end; with its curve when
                // EDGE_CURVE.same_sense
                let with_curve = flag(&ec[4]);
                uses.push(if along {
                    (edge, sv, ev, with_curve, true)
                } else {
                    (edge, ev, sv, !with_curve, false)
                });
            }
            if !flag(&b.params()[2]) {
                uses.reverse();
                for u in &mut uses {
                    *u = (u.0, u.2, u.1, !u.3, !u.4);
                }
            }
            loops.push(uses);
        }
        let kind = file.e(surface_id).name().to_string();
        let (by_loops, by_sense) = if let IsoSurface::Plane { o, z } = surface {
            let mut area = [0.0; 3];
            for uses in &loops {
                for &(edge, a, b, with_curve, _) in uses {
                    let ec = file.params(edge, "EDGE_CURVE");
                    let curve = file.curve(rid(&ec[3]));
                    area = add(area, half_moment(&curve, a, b, with_curve));
                }
            }
            (dot(o, area), sigma * dot(o, z) * norm(area))
        } else {
            // the loops in (u, v): each use's pcurve, the seam's two in the order that joins
            let mut rings: Vec<Vec<[f64; 2]>> = Vec::new();
            for uses in &loops {
                let options: Vec<Vec<Vec<[f64; 2]>>> = uses
                    .iter()
                    .map(|&(edge, _, _, _, forward)| {
                        let mut curves = file.pcurves(edge, surface_id);
                        assert!(
                            !curves.is_empty(),
                            "face #{face}: edge #{edge} has no pcurve"
                        );
                        if !forward {
                            for c in &mut curves {
                                c.reverse();
                            }
                        }
                        curves
                    })
                    .collect();
                for (u, o) in uses.iter().zip(&options) {
                    for c in o {
                        eprintln!(
                            "DBG face #{face} edge #{} fwd {} {:?} -> {:?}",
                            u.0,
                            u.4,
                            c[0],
                            c.last().unwrap()
                        );
                    }
                }
                let choices: usize = options.iter().map(Vec::len).product();
                let mut best = (f64::INFINITY, Vec::new());
                for mut pick in 0..choices {
                    let chosen: Vec<&Vec<[f64; 2]>> = options
                        .iter()
                        .map(|o| {
                            let c = &o[pick % o.len()];
                            pick /= o.len();
                            c
                        })
                        .collect();
                    let gap: f64 = (0..chosen.len())
                        .map(|i| {
                            let end = chosen[i].last().unwrap();
                            let next = chosen[(i + 1) % chosen.len()][0];
                            (end[0] - next[0]).hypot(end[1] - next[1])
                        })
                        .sum();
                    if gap < best.0 {
                        best = (gap, chosen.into_iter().flatten().copied().collect());
                    }
                }
                assert!(
                    best.0 < 1e-6,
                    "face #{face}: its loop's pcurves do not join ({})",
                    best.0
                );
                rings.push(best.1);
            }
            // uv signed area (anticlockwise positive) and the Green line integral
            // −∮ H du, H(u, v) = ∫ from v_ref to v of p·(S_u × S_v) dt
            let v_ref = rings[0][0][1];
            let g = |u: f64, v: f64| dot(surface.at(u, v), surface.normal(u, v));
            let h = |u: f64, v: f64| {
                let pieces = 8;
                let mut sum = 0.0;
                for i in 0..pieces {
                    let lo = v_ref + (v - v_ref) * f64::from(i) / f64::from(pieces);
                    let hi = v_ref + (v - v_ref) * f64::from(i + 1) / f64::from(pieces);
                    for (x, w) in gauss() {
                        sum += w * (hi - lo) * g(u, lo + (hi - lo) * x);
                    }
                }
                sum
            };
            let (mut uv_area, mut green) = (0.0, 0.0);
            for ring in &rings {
                for i in 0..ring.len() {
                    let a = ring[i];
                    let b = ring[(i + 1) % ring.len()];
                    uv_area += 0.5 * (a[0] * b[1] - b[0] * a[1]);
                    if b[0] == a[0] {
                        continue;
                    }
                    for (x, w) in gauss() {
                        let u = a[0] + (b[0] - a[0]) * x;
                        let v = a[1] + (b[1] - a[1]) * x;
                        green -= w * (b[0] - a[0]) * h(u, v);
                    }
                }
            }
            (green, sigma * uv_area.signum() * green)
        };
        out.push(FaceMeasure {
            id: face,
            kind,
            same_sense,
            by_loops: by_loops / 3.0,
            by_sense: by_sense / 3.0,
        });
    }
    out
}

/// Exports `brep` and checks its file: every face consistent (its loops and its
/// `same_sense` measure the same), and the solid's volume `expected` both ways.
fn assert_right_side_out(name: &str, brep: &Brep, expected: f64, rel: f64) {
    let model = brep_to_model(brep).unwrap_or_else(|e| panic!("{name}: {e}"));
    let text = step_export::write(&model).unwrap_or_else(|e| panic!("{name}: {e}"));
    let faces = measure(&text);
    let kinds: BTreeMap<&str, usize> = faces.iter().fold(BTreeMap::new(), |mut m, f| {
        *m.entry(f.kind.as_str()).or_insert(0) += 1;
        m
    });
    eprintln!("{name}: {kinds:?}");
    for f in &faces {
        eprintln!(
            "{name}: face #{} {} same_sense {} by loops {:.6} by same_sense {:.6}",
            f.id, f.kind, f.same_sense, f.by_loops, f.by_sense
        );
    }
    let by_loops: f64 = faces.iter().map(|f| f.by_loops).sum();
    let by_sense: f64 = faces.iter().map(|f| f.by_sense).sum();
    let scale = expected.abs();
    let inconsistent: Vec<_> = faces
        .iter()
        .filter(|f| (f.by_loops - f.by_sense).abs() > rel * scale)
        .collect();
    assert!(
        inconsistent.is_empty(),
        "{name}: faces whose loops and same_sense disagree: {inconsistent:?}"
    );
    assert!(
        (by_loops - expected).abs() <= rel * scale && (by_sense - expected).abs() <= rel * scale,
        "{name}: ISO signed volume {by_loops} by loops, {by_sense} by same_sense; expected {expected}"
    );
}

#[test]
fn a_box_is_written_right_side_out() {
    let mut kernel = Kernel::default();
    let brep = api::create_box(&mut kernel, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
    assert_right_side_out("box", &brep, 6000.0, 1e-12);
}

#[test]
fn a_union_and_a_cut_are_written_right_side_out() {
    let mut kernel = Kernel::default();
    let cube = |k: &mut Kernel, at: f64| {
        api::create_box(k, Point3::new(at, at, at), 10.0, 10.0, 10.0).unwrap()
    };
    let (a, b) = (cube(&mut kernel, 0.0), cube(&mut kernel, 5.0));
    let union = boolean_union(&mut kernel, a, b).unwrap();
    assert_right_side_out("union", &union, 1875.0, 1e-12);
    let (a, b) = (cube(&mut kernel, 0.0), cube(&mut kernel, 5.0));
    let cut = boolean_difference(&mut kernel, a, b).unwrap();
    assert_right_side_out("cut", &cut, 875.0, 1e-12);
}

#[test]
fn a_cylinder_is_written_right_side_out() {
    let mut kernel = Kernel::default();
    let brep = api::create_cylinder(&mut kernel, Point3::new(1.0, -2.0, 3.0), 5.0, 10.0).unwrap();
    assert_right_side_out("cylinder", &brep, 250.0 * std::f64::consts::PI, 1e-9);
}

/// A quadratic Bezier in the plane `z = 0` or `y = 0`.
fn bezier(points: [P; 3]) -> Curve {
    let control: Vec<Point3> = points.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect();
    Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 2,
            control_points: &control,
            form: BSplineCurveForm::default(),
            knot_multiplicities: &[3, 3],
            knots: &[0.0, 1.0],
            knot_type: KnotType::default(),
            weights: None,
            trim: None,
        })
        .unwrap(),
    )
}

fn segment(a: P, b: P) -> Curve {
    Curve::from(
        create_line_segment(Vec3::new(a[0], a[1], a[2]), Vec3::new(b[0], b[1], b[2])).unwrap(),
    )
}

/// A swept B-spline profile: its side is a surface of linear extrusion. The region under the
/// parabola from (0, 0) through (5, 5) to (10, 0) is 100/3; 6 tall, 200.
#[test]
fn an_extruded_spline_is_written_right_side_out() {
    let mut kernel = Kernel::default();
    let curves = [
        bezier([[0.0, 0.0, 0.0], [5.0, 10.0, 0.0], [10.0, 0.0, 0.0]]),
        segment([10.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let brep =
        create_linear_sweep(&mut kernel, &refs, Vec3::new(0.0, 0.0, 1.0), 6.0, true).unwrap();
    assert_right_side_out("extruded spline", &brep, 200.0, 1e-9);
}

/// A revolved profile with a B-spline side: its faces are a surface of revolution, a
/// cylinder and two annuli. The profile in y = 0: the Bezier (2, 0) (4, 2) (2, 4) in (x, z)
/// and the lines back by (1, 4) and (1, 0); by Pappus its volume is 2π · 12.4.
#[test]
fn a_revolved_spline_is_written_right_side_out() {
    let mut kernel = Kernel::default();
    let curves = [
        bezier([[2.0, 0.0, 0.0], [4.0, 0.0, 2.0], [2.0, 0.0, 4.0]]),
        segment([2.0, 0.0, 4.0], [1.0, 0.0, 4.0]),
        segment([1.0, 0.0, 4.0], [1.0, 0.0, 0.0]),
        segment([1.0, 0.0, 0.0], [2.0, 0.0, 0.0]),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let brep = create_rotational_sweep(
        &mut kernel,
        &refs,
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        360.0,
        true,
    )
    .unwrap();
    assert_right_side_out("revolved spline", &brep, 24.8 * std::f64::consts::PI, 1e-9);
}

/// The ISO 10303-42 surface of a model surface (the analytic and swept kinds the sheets
/// below use), as the writer writes it.
fn iso_of_model(surface: &solidcore_record::step::model::Surface) -> IsoSurface {
    use solidcore_record::step::model::{Curve as MC, Surface as MS};
    let curve = |c: &MC| match c {
        MC::Line {
            origin,
            dir,
            magnitude,
        } => IsoCurve::Line {
            origin: *origin,
            step: mul(*magnitude, *dir),
        },
        MC::Circle { frame, radius } => IsoCurve::Circle {
            o: frame.origin,
            x: frame.x,
            y: frame.y,
            r: *radius,
        },
        other => panic!("generator {other:?}"),
    };
    match surface {
        MS::Cylinder { frame, radius } => IsoSurface::Cylinder {
            o: frame.origin,
            x: frame.x,
            y: frame.y,
            z: frame.z,
            r: *radius,
        },
        MS::Cone {
            frame,
            radius,
            semi_angle_deg,
        } => IsoSurface::Cone {
            o: frame.origin,
            x: frame.x,
            y: frame.y,
            z: frame.z,
            r: *radius,
            tan: semi_angle_deg.to_radians().tan(),
        },
        MS::Sphere { frame, radius } => IsoSurface::Sphere {
            o: frame.origin,
            x: frame.x,
            y: frame.y,
            z: frame.z,
            r: *radius,
        },
        MS::Torus {
            frame,
            major,
            minor,
        } => IsoSurface::Torus {
            o: frame.origin,
            x: frame.x,
            y: frame.y,
            z: frame.z,
            major: *major,
            minor: *minor,
        },
        MS::Revolution {
            curve: c,
            axis_origin,
            axis_dir,
        } => IsoSurface::Revolution {
            curve: curve(c),
            o: *axis_origin,
            d: *axis_dir,
        },
        MS::Extrusion {
            curve: c,
            dir,
            magnitude,
        } => IsoSurface::Extrusion {
            curve: curve(c),
            step: mul(*magnitude, *dir),
        },
        other => panic!("surface {other:?}"),
    }
}

/// Whether the model's face `face` has loops running anticlockwise in its STEP (u, v)
/// (its pcurves, each along its edge, reversed where the loop runs against the edge):
/// the signed area of its rings.
fn uv_area(model: &solidcore_record::step::model::Model, face: usize) -> f64 {
    use solidcore_record::step::model::Bound;
    let body = &model.bodies[0];
    let face = &body.faces[face];
    let mut area = 0.0;
    for (loop_index, loop_) in face.loops.iter().enumerate() {
        let Bound::Edges(edges) = &loop_.bound else {
            panic!("a vertex loop")
        };
        let mut ring = Vec::new();
        for (use_index, oriented) in edges.iter().enumerate() {
            let curves = model
                .pcurves
                .get(&(face.step_id, loop_index, use_index))
                .unwrap_or_else(|| panic!("face {}: use {use_index} has no pcurve", face.step_id));
            let mut uv = curves[0].clone();
            if !oriented.forward {
                uv.reverse();
            }
            ring.extend(uv);
        }
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            area += 0.5 * (a[0] * b[1] - b[0] * a[1]);
        }
    }
    area
}

/// The kernel's swapped and inside-out parameterisations turn the NURB normal over against
/// the STEP surface's: each face's `same_sense` says so, its loops running anticlockwise
/// in STEP (u, v) exactly when it is true. One-face sheets, each edge use with its own UV
/// curve.
#[test]
fn same_sense_follows_swapped_and_inside_out_parameterisations() {
    use solidcore::curve::ellipse::p2::EllipseOptions;
    use solidcore::curve::ellipse::Ellipse;
    use solidcore::curve::line::Line;
    use solidcore::geom::extent1d::Extent1d;
    use solidcore::geom::extent2d::Extent2d;
    use solidcore::geom::global_solver::SolverOperation;
    use solidcore::geom::vector2d::Point2;
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::surface::cone::p1::Cone;
    use solidcore::surface::sphere::{Sphere, SphereOptions};
    use solidcore::surface::surf_of_extrusion::SurfOfExtrusion;
    use solidcore::surface::surf_of_revolution::SurfOfRevolution;
    use solidcore::surface::surface::Surface;
    use solidcore::surface::torus::{Torus, TorusOptions};
    use solidcore::topology::brep::p2::edgeuses;
    use solidcore::topology::brep::sheet::sheet_on;
    use solidcore::topology::edgeuse::p3::get_or_create_uv_trim_curve;
    let mut kernel = Kernel::default();
    // (without this opt-in fix the torus forgets that its generator runs backwards, and
    // says it is not inside out)
    kernel.fixes.torus_keeps_inside_out = true;
    let origin = Point3::new(0.0, 0.0, 0.0);
    let (x, y) = (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
    let mut surfaces: Vec<(String, Surface)> = Vec::new();
    for swap_uv in [false, true] {
        for inside_out in [false, true] {
            let tag = format!("swap {swap_uv} inside out {inside_out}");
            surfaces.push((
                format!("cone, {tag}"),
                Surface::from(Cone::new_bounded(
                    origin, x, y, 3.0, 1.0, 0.0, 270.0, 4.0, swap_uv, inside_out, true,
                )),
            ));
            surfaces.push((
                format!("cylinder, {tag}"),
                Surface::from(Cone::new_cylinder_bounded(
                    origin, x, y, 3.0, 0.0, 270.0, 4.0, swap_uv, inside_out, true,
                )),
            ));
            surfaces.push((
                format!("sphere, {tag}"),
                Surface::from(
                    Sphere::new(
                        &kernel,
                        origin,
                        x,
                        y,
                        Extent2d::from_bounds(0.0, -60.0, 270.0, 60.0),
                        2.0,
                        SphereOptions {
                            swap_uv,
                            inside_out,
                            circle_nurb: None,
                        },
                    )
                    .unwrap(),
                ),
            ));
            surfaces.push((
                format!("torus, {tag}"),
                Surface::from(
                    Torus::new(
                        &kernel,
                        origin,
                        x,
                        y,
                        Extent2d::from_bounds(0.0, 0.0, 270.0, 270.0),
                        5.0,
                        1.5,
                        TorusOptions {
                            swap_uv,
                            inside_out,
                            circle_nurb: None,
                        },
                    )
                    .unwrap(),
                ),
            ));
        }
        let mut line =
            Line::from_points(Point3::new(10.0, 0.0, 0.0), Point3::new(12.0, 0.0, 20.0), 3);
        line.make_nurb().unwrap();
        surfaces.push((
            format!("revolved line, swap {swap_uv}"),
            Surface::from(
                SurfOfRevolution::new(
                    &kernel,
                    Curve::from(line),
                    origin,
                    x,
                    y,
                    Extent2d::from_bounds(0.0, 0.0, 270.0, 1.0),
                    swap_uv,
                )
                .unwrap(),
            ),
        ));
        let arc = Ellipse::new_circle(
            &kernel,
            origin,
            x,
            y,
            Extent1d::from_min_max(0.0, 270.0),
            10.0,
            &EllipseOptions {
                dim: 3,
                ..EllipseOptions::default()
            },
        )
        .unwrap();
        surfaces.push((
            format!("extruded arc, swap {swap_uv}"),
            Surface::from(
                SurfOfExtrusion::new(
                    &kernel,
                    Curve::from(arc),
                    origin,
                    Vec3::new(0.0, 0.0, 20.0),
                    Extent2d::from_bounds(0.0, 0.0, 270.0, 1.0),
                    swap_uv,
                )
                .unwrap(),
            ),
        ));
    }
    let mut wrong = Vec::new();
    for (name, surface) in surfaces {
        let (mut brep, face_id) =
            sheet_on(&mut kernel, surface, 1e-6).unwrap_or_else(|e| panic!("{name}: {e}"));
        for edgeuse in edgeuses(&brep, None).unwrap() {
            get_or_create_uv_trim_curve(&kernel, &mut brep, edgeuse, false, false, true)
                .unwrap()
                .status
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        let model = brep_to_model(&brep).unwrap_or_else(|e| panic!("{name}: {e}"));
        let face = &model.bodies[0].faces[0];
        let iso = iso_of_model(&face.surface);
        // The written side, found geometrically: the kernel's normal (∂S/∂u × ∂S/∂v in its
        // NURB parameters, at the middle of its domain) on the side of the faceuse written
        // (the face's own: a sheet has no material region), against the ISO surface's
        // natural normal there.
        let surf = brep.face(face_id).unwrap().surface.clone().unwrap();
        let mid = surf.natural_uv_domain_with_nurb(&kernel).unwrap().mid();
        let at = |u: f64, v: f64| {
            let p = surf.evaluate_point(&kernel, Point2::new(u, v)).unwrap();
            [p.x, p.y, p.z]
        };
        let h = 1e-5;
        let p = at(mid.x, mid.y);
        let kernel_normal = cross(
            sub(at(mid.x + h, mid.y), at(mid.x - h, mid.y)),
            sub(at(mid.x, mid.y + h), at(mid.x, mid.y - h)),
        );
        let faceuse = brep.face(face_id).unwrap().faceuse.unwrap();
        let side = if brep.faceuse(faceuse).unwrap().orientation == OrientType::Same {
            1.0
        } else {
            -1.0
        };
        let away_from = |o: P, z: P| {
            let d = sub(p, o);
            sub(d, mul(dot(d, z), z))
        };
        let iso_normal = match &iso {
            IsoSurface::Cylinder { o, z, .. } | IsoSurface::Cone { o, z, .. } => away_from(*o, *z),
            IsoSurface::Sphere { o, .. } => sub(p, *o),
            IsoSurface::Torus { o, z, major, .. } => {
                sub(p, add(*o, mul(*major, unit(away_from(*o, *z)))))
            }
            IsoSurface::Revolution { .. } | IsoSurface::Extrusion { .. } => {
                let step = surf.convert_uv_from_nurbs_to_step(&kernel, mid).unwrap();
                let (u, v) = (step.x.to_radians(), step.y);
                assert!(norm(sub(iso.at(u, v), p)) < 1e-6, "{name}: STEP point");
                iso.normal(u, v)
            }
            IsoSurface::Plane { .. } => unreachable!(),
        };
        let expected = side * dot(kernel_normal, iso_normal) > 0.0;
        eprintln!(
            "{name}: same_sense {} geometric {expected}",
            face.same_sense
        );
        if face.same_sense != expected {
            wrong.push(format!("{name} (geometric)"));
        }
        // the pcurves are right: each point, lifted by the written surface's ISO formula,
        // lies on an edge of the sheet
        let edges: Vec<_> = brep
            .edge_list()
            .iter()
            .filter_map(|&e| {
                let edge = brep.edge(e).unwrap();
                Some((edge.curve.as_ref()?, edge.interval))
            })
            .collect();
        let mut worst = 0.0_f64;
        for &[u, v] in model.pcurves.values().flatten().flatten() {
            let [px, py, pz] = iso.at(u, v);
            let gap = edges
                .iter()
                .filter_map(|(curve, interval)| {
                    curve
                        .drop_point(
                            &kernel,
                            interval,
                            Point3::new(px, py, pz),
                            None,
                            1e-9,
                            None,
                            SolverOperation::Minimize,
                        )
                        .ok()
                        .filter(|dropped| dropped.success)
                        .map(|dropped| dropped.distance)
                })
                .fold(f64::INFINITY, f64::min);
            worst = worst.max(gap);
        }
        if worst >= 1e-4 {
            wrong.push(format!("{name} (pcurves lift {worst} off the edges)"));
            continue;
        }
        let area = uv_area(&model, 0);
        eprintln!("{name}: same_sense {} uv area {area}", face.same_sense);
        assert!(area.abs() > 1e-6, "{name}: no uv area");
        if (area > 0.0) != face.same_sense {
            wrong.push(format!("{name} (loops)"));
        }
    }
    assert!(wrong.is_empty(), "same_sense against the loops: {wrong:?}");
}
