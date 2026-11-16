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

/// The homogeneous point of a B-spline at `t` (de Boor), `knots` full; beyond its ends the
/// end spans' polynomials.
fn de_boor(p: usize, knots: &[f64], control: &[[f64; 4]], t: f64) -> [f64; 4] {
    let n = control.len();
    // the span: knots[s] <= t < knots[s + 1], clamped to the curve's ends
    let mut s = p;
    while s + 1 < n && t >= knots[s + 1] {
        s += 1;
    }
    let mut d: Vec<[f64; 4]> = (0..=p).map(|j| control[s - p + j]).collect();
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
    d[p]
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
                let control: Vec<[f64; 4]> = points
                    .iter()
                    .zip(weights)
                    .map(|(q, w)| [q[0] * w, q[1] * w, q[2] * w, *w])
                    .collect();
                let h = de_boor(*degree, knots, &control, t);
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
    /// Rows along u of homogeneous control points; full knots.
    BSpline {
        u_degree: usize,
        v_degree: usize,
        rows: Vec<Vec<[f64; 4]>>,
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
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
            IsoSurface::BSpline {
                u_degree,
                v_degree,
                rows,
                u_knots,
                v_knots,
            } => {
                let column: Vec<[f64; 4]> = rows
                    .iter()
                    .map(|row| de_boor(*v_degree, v_knots, row, v))
                    .collect();
                let h = de_boor(*u_degree, u_knots, &column, u);
                [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
            }
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
        // a B-spline surface, simple or complex (rational)
        let bspline = if let Some([du, dv, net, ..]) = e.record("B_SPLINE_SURFACE") {
            let k = e.record("B_SPLINE_SURFACE_WITH_KNOTS").unwrap();
            Some((du, dv, net, &k[0], &k[1], &k[2], &k[3]))
        } else if e.name() == "B_SPLINE_SURFACE_WITH_KNOTS" {
            let b = e.params();
            Some((&b[1], &b[2], &b[3], &b[8], &b[9], &b[10], &b[11]))
        } else {
            None
        };
        if let Some((du, dv, net, um, vm, uk, vk)) = bspline {
            let weights: Option<Vec<Vec<f64>>> = e.record("RATIONAL_B_SPLINE_SURFACE").map(|w| {
                list(&w[0])
                    .iter()
                    .map(|row| list(row).iter().map(real).collect())
                    .collect()
            });
            let rows = list(net)
                .iter()
                .enumerate()
                .map(|(i, row)| {
                    list(row)
                        .iter()
                        .enumerate()
                        .map(|(j, q)| {
                            let q = self.point(rid(q));
                            let w = weights.as_ref().map_or(1.0, |w| w[i][j]);
                            [q[0] * w, q[1] * w, q[2] * w, w]
                        })
                        .collect()
                })
                .collect();
            return IsoSurface::BSpline {
                u_degree: du.as_i64().unwrap() as usize,
                v_degree: dv.as_i64().unwrap() as usize,
                rows,
                u_knots: full_knots(list(um), list(uk)),
                v_knots: full_knots(list(vm), list(vk)),
            };
        }
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
    /// On a void's shell (turned over by its `ORIENTED_CLOSED_SHELL`).
    void: bool,
    /// Its shell's place in the solid: 0 the outer, then the voids.
    shell: usize,
    /// Its loops alone don't say which side the face is (a seam loop round a closed
    /// surface traces its whole (u, v) box either way): the side is the one `same_sense`
    /// gives, as ISO readers settle it.
    settled_by_sense: bool,
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

/// A loop of a curved face in its surface's (u, v): its uses' pcurves end to end, each
/// along its use. The joins follow the edges: consecutive uses meet at their shared
/// vertex, so each pcurve starts where the last ended, or the two ends lift to that one
/// point across a singular line of the surface (a pole), which the loop then runs along.
/// A seam edge carries two pcurves on the surface; where a loop uses it twice each use
/// takes a different one. A wrap across a periodic seam is not a join: a straight
/// (u, v) step whose points lift to different points is refused. Where the edges leave
/// two rings (a seam loop round a closed surface: the same box traced either way), the
/// one turning as `same_sense` says is taken, and the second value says so.
fn ring(
    file: &File,
    surface: &IsoSurface,
    surface_id: u64,
    face: u64,
    uses: &[(u64, P, P, bool, bool)],
    same_sense: bool,
) -> (Vec<[f64; 2]>, bool) {
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
    // the points (u, v) lifts to along a straight step from a to b: one point, or not
    let degenerate = |a: [f64; 2], b: [f64; 2]| {
        let p0 = surface.at(a[0], a[1]);
        let scale = 1.0 + norm(p0);
        (1..=16).all(|i| {
            let t = f64::from(i) / 16.0;
            let q = surface.at(a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]));
            norm(sub(q, p0)) <= 1e-7 * scale
        })
    };
    let choices: usize = options.iter().map(Vec::len).product();
    let mut rings = Vec::new();
    for mut pick in 0..choices {
        let chosen: Vec<usize> = options
            .iter()
            .map(|o| {
                let c = pick % o.len();
                pick /= o.len();
                c
            })
            .collect();
        // a seam used twice takes both its pcurves
        let reuses = (0..uses.len()).any(|i| {
            (i + 1..uses.len())
                .any(|j| uses[i].0 == uses[j].0 && options[i].len() > 1 && chosen[i] == chosen[j])
        });
        if reuses {
            continue;
        }
        let joined = (0..uses.len()).all(|i| {
            let end = *options[i][chosen[i]].last().unwrap();
            let j = (i + 1) % uses.len();
            let next = options[j][chosen[j]][0];
            (end[0] - next[0]).hypot(end[1] - next[1]) < 1e-7 || degenerate(end, next)
        });
        if joined {
            let r: Vec<[f64; 2]> = (0..uses.len())
                .flat_map(|i| options[i][chosen[i]].iter().copied())
                .collect();
            rings.push(r);
        }
    }
    assert!(
        !rings.is_empty(),
        "face #{face}: its loop's pcurves do not join along its edges"
    );
    if rings.len() == 1 {
        return (rings.remove(0), false);
    }
    let area = |r: &[[f64; 2]]| {
        (0..r.len())
            .map(|i| {
                let (a, b) = (r[i], r[(i + 1) % r.len()]);
                0.5 * (a[0] * b[1] - b[0] * a[1])
            })
            .sum::<f64>()
    };
    let i = rings
        .iter()
        .position(|r| (area(r) > 0.0) == same_sense)
        .unwrap_or_else(|| panic!("face #{face}: no ring of its loop turns as same_sense says"));
    (rings.remove(i), true)
}

fn measure(text: &str) -> Vec<FaceMeasure> {
    let file = File {
        x: p21::parse(text).expect("the file parses"),
    };
    let solids: Vec<u64> = file
        .x
        .entities
        .iter()
        .filter(|(_, e)| matches!(e.name(), "MANIFOLD_SOLID_BREP" | "BREP_WITH_VOIDS"))
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(solids.len(), 1, "one solid");
    let solid = file.e(solids[0]);
    // the solid's shells, each with the sign its faces count with: the outer CLOSED_SHELL,
    // and each void's ORIENTED_CLOSED_SHELL, which turns its closed shell over when .F.
    let mut shells = vec![(rid(&solid.params()[1]), 1.0)];
    if solid.name() == "BREP_WITH_VOIDS" {
        for void in list(&solid.params()[2]) {
            let oriented = file.params(rid(void), "ORIENTED_CLOSED_SHELL");
            let sign = if flag(&oriented[3]) { 1.0 } else { -1.0 };
            shells.push((rid(&oriented[2]), sign));
        }
    }
    let faces: Vec<(u64, f64, usize)> = shells
        .iter()
        .enumerate()
        .flat_map(|(index, &(shell, sign))| {
            list(&file.params(shell, "CLOSED_SHELL")[1])
                .iter()
                .map(move |face| (rid(face), sign, index))
                .collect::<Vec<_>>()
        })
        .collect();
    let mut out = Vec::new();
    for (face, shell_sign, shell) in faces {
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
        let mut settled_by_sense = false;
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
            // the loops in (u, v), joined by their edges ([`ring`])
            let mut settled = false;
            let mut rings: Vec<Vec<[f64; 2]>> = Vec::new();
            for uses in &loops {
                let (r, by_sense) = ring(&file, &surface, surface_id, face, uses, same_sense);
                settled |= by_sense;
                rings.push(r);
            }
            settled_by_sense |= settled;
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
            void: shell_sign < 0.0,
            shell,
            settled_by_sense,
            by_loops: shell_sign * by_loops / 3.0,
            by_sense: shell_sign * by_sense / 3.0,
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
            "{name}: face #{} {}{} same_sense {} by loops {:.6} by same_sense {:.6}{}",
            f.id,
            f.kind,
            if f.void { " (void)" } else { "" },
            f.same_sense,
            f.by_loops,
            f.by_sense,
            if f.settled_by_sense {
                " (side settled by same_sense)"
            } else {
                ""
            }
        );
    }
    let by_loops: f64 = faces.iter().map(|f| f.by_loops).sum();
    let by_sense: f64 = faces.iter().map(|f| f.by_sense).sum();
    let scale = expected.abs();
    // every face counts: off the origin no face's p·n integrates to 0, so a face turned
    // over changes the volume
    let silent: Vec<_> = faces
        .iter()
        .filter(|f| f.by_sense.abs() <= 1e-6 * scale)
        .collect();
    assert!(
        silent.is_empty(),
        "{name}: faces that add nothing: {silent:?}"
    );
    // each shell bounds a volume of its own the right way round: the outer shell's is
    // positive, each void's negative once its ORIENTED_CLOSED_SHELL turns it over
    let shells = faces.iter().map(|f| f.shell).max().map_or(0, |m| m + 1);
    for shell in 0..shells {
        let of = |by: fn(&FaceMeasure) -> f64| {
            faces
                .iter()
                .filter(|f| f.shell == shell)
                .map(by)
                .sum::<f64>()
        };
        let (loops, sense) = (of(|f| f.by_loops), of(|f| f.by_sense));
        assert!(
            (shell == 0) == (loops > 0.0) && (shell == 0) == (sense > 0.0),
            "{name}: shell {shell} ({}) bounds {loops} by loops, {sense} by same_sense",
            if shell == 0 { "outer" } else { "void" }
        );
    }
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
    let brep = api::create_box(&mut kernel, Point3::new(3.0, 4.0, 5.0), 10.0, 20.0, 30.0).unwrap();
    assert_right_side_out("box", &brep, 6000.0, 1e-12);
}

#[test]
fn a_union_and_a_cut_are_written_right_side_out() {
    let mut kernel = Kernel::default();
    let cube = |k: &mut Kernel, at: f64| {
        api::create_box(k, Point3::new(at, at, at), 10.0, 10.0, 10.0).unwrap()
    };
    let (a, b) = (cube(&mut kernel, 3.0), cube(&mut kernel, 8.0));
    let union = boolean_union(&mut kernel, a, b).unwrap();
    assert_right_side_out("union", &union, 1875.0, 1e-12);
    let (a, b) = (cube(&mut kernel, 3.0), cube(&mut kernel, 8.0));
    let cut = boolean_difference(&mut kernel, a, b).unwrap();
    assert_right_side_out("cut", &cut, 875.0, 1e-12);
}

#[test]
fn a_cylinder_is_written_right_side_out() {
    let mut kernel = Kernel::default();
    let brep = api::create_cylinder(&mut kernel, Point3::new(1.0, -2.0, 3.0), 5.0, 10.0).unwrap();
    assert_right_side_out("cylinder", &brep, 250.0 * std::f64::consts::PI, 1e-9);
}

/// A sphere, a cone (a frustum) and a torus: their faces' loops run round closed surfaces,
/// with seams and (the sphere's) poles.
#[test]
fn a_sphere_a_cone_and_a_torus_are_written_right_side_out() {
    use std::f64::consts::PI;
    let mut kernel = Kernel::default();
    let at = Point3::new(3.0, 4.0, 5.0);
    let sphere = api::create_sphere(&mut kernel, at, 5.0).unwrap();
    assert_right_side_out("sphere", &sphere, 500.0 * PI / 3.0, 1e-9);
    let cone = api::create_cone(&mut kernel, at, 3.0, 1.0, 4.0).unwrap();
    assert_right_side_out("cone", &cone, 4.0 * PI / 3.0 * 13.0, 1e-9);
    let torus = api::create_torus(&mut kernel, at, 5.0, 1.5).unwrap();
    assert_right_side_out("torus", &torus, 2.0 * PI * PI * 5.0 * 2.25, 1e-9);
}

/// A loft (degree 1) from a square of circumradius 2 to one of circumradius 1 turned by
/// 30 degrees, 6 above it: its four sides are bilinear B-spline surfaces. Its cross-section
/// area is quadratic in the height, so the prismatoid formula is exact:
/// V = h/6 (A0 + 4 Am + A1) = 20 + 4√3, the mid section a square of circumradius
/// |P + Q|/2 (|P + Q|² = 5 + 2√3).
#[test]
fn a_lofted_b_spline_solid_is_written_right_side_out() {
    use solidcore::sweep::api_surfaces::create_skin_primitive;
    let mut kernel = Kernel::default();
    let square = |r: f64, turn: f64, z: f64| -> Vec<Curve> {
        let corner = |i: usize| {
            let a = (45.0 + 90.0 * i as f64 + turn).to_radians();
            [3.0 + r * a.cos(), 4.0 + r * a.sin(), 5.0 + z]
        };
        (0..4)
            .map(|i| segment(corner(i), corner((i + 1) % 4)))
            .collect()
    };
    let curves: Vec<Curve> = square(2.0, 0.0, 0.0)
        .into_iter()
        .chain(square(1.0, 30.0, 6.0))
        .collect();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let skin = create_skin_primitive(&mut kernel, &[4, 4], &refs, 1, true).unwrap();
    assert_right_side_out("loft", &skin.brep, 20.0 + 4.0 * 3.0_f64.sqrt(), 1e-9);
}

/// The cup and lid: a 100-cube hollowed to an open cup (walls and floor 10), joined with a
/// lid over its top 10, which seals an 80-cube cavity, 1000000 − 512000. On the default
/// kernel the union leaves the cavity's shell first in the region's list; the file's outer
/// shell is still the cube's.
#[test]
fn a_cavity_sealed_by_a_union_is_written_as_a_void() {
    for lid_first in [false, true] {
        let mut kernel = Kernel::default();
        let cube =
            api::create_box(&mut kernel, Point3::new(3.0, 4.0, 5.0), 100.0, 100.0, 100.0).unwrap();
        let pocket = api::create_box(
            &mut kernel,
            Point3::new(13.0, 14.0, 15.0),
            80.0,
            80.0,
            100.0,
        )
        .unwrap();
        let lid =
            api::create_box(&mut kernel, Point3::new(3.0, 4.0, 95.0), 100.0, 100.0, 10.0).unwrap();
        let cup = boolean_difference(&mut kernel, cube, pocket).unwrap();
        let (a, b) = if lid_first { (lid, cup) } else { (cup, lid) };
        let solid = boolean_union(&mut kernel, a, b).unwrap();
        let name = format!("cup and lid (lid first: {lid_first})");
        assert_right_side_out(&name, &solid, 488_000.0, 1e-12);
    }
}

/// A 10-cube with a 3-cube cavity: one `BREP_WITH_VOIDS`, the cavity a void (its shell
/// under an `ORIENTED_CLOSED_SHELL` turning it over), 1000 − 27.
#[test]
fn a_cavity_is_written_as_a_void() {
    let mut kernel = Kernel::default();
    let outer = api::create_box(&mut kernel, Point3::new(3.0, 4.0, 5.0), 10.0, 10.0, 10.0).unwrap();
    let cavity = api::create_box(&mut kernel, Point3::new(6.0, 7.0, 8.0), 3.0, 3.0, 3.0).unwrap();
    let solid = boolean_difference(&mut kernel, outer, cavity).unwrap();
    assert_right_side_out("cavity", &solid, 973.0, 1e-12);
}

/// A quadratic Bezier.
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
/// parabola from (0, 0) through (5, 5) to (10, 0) is 100/3; 6 tall, 200. (At z = 5, the
/// profile moved by (3, 4).)
#[test]
fn an_extruded_spline_is_written_right_side_out() {
    let mut kernel = Kernel::default();
    let curves = [
        bezier([[3.0, 4.0, 5.0], [8.0, 14.0, 5.0], [13.0, 4.0, 5.0]]),
        segment([13.0, 4.0, 5.0], [3.0, 4.0, 5.0]),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let brep =
        create_linear_sweep(&mut kernel, &refs, Vec3::new(0.0, 0.0, 1.0), 6.0, true).unwrap();
    assert_right_side_out("extruded spline", &brep, 200.0, 1e-9);
}

/// A revolved profile with a B-spline side: its faces are a surface of revolution, a
/// cylinder and two annuli. The profile in y = 0: the Bezier (2, 0) (4, 2) (2, 4) in (x, z)
/// and the lines back by (1, 4) and (1, 0); by Pappus its volume is 2π · 12.4. (All
/// moved by (3, 4, 5), its axis too.)
#[test]
fn a_revolved_spline_is_written_right_side_out() {
    let mut kernel = Kernel::default();
    let curves = [
        bezier([[5.0, 4.0, 5.0], [7.0, 4.0, 7.0], [5.0, 4.0, 9.0]]),
        segment([5.0, 4.0, 9.0], [4.0, 4.0, 9.0]),
        segment([4.0, 4.0, 9.0], [4.0, 4.0, 5.0]),
        segment([4.0, 4.0, 5.0], [5.0, 4.0, 5.0]),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let brep = create_rotational_sweep(
        &mut kernel,
        &refs,
        Point3::new(3.0, 4.0, 5.0),
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
            IsoSurface::Plane { .. } | IsoSurface::BSpline { .. } => unreachable!(),
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
