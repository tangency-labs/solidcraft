//! Runner for unit `topology.brep_constructor`. C++:
//! `solidcore_ffi::units::topology_brep_constructor` (upstream `SmBrepConstructor`). Rust:
//! `solidcore::topology::brep_constructor` through [`crate::constructor`].
//!
//! A case is a constructor script ([`step_import::script`]): the calls an importer makes,
//! over a table of curves and surfaces. Both sides run it and report every call's status,
//! where upstream crashes on a null dereference (the port's constructor
//! [`solidcore::SolidError::InvalidInput`]), and the B-rep built: its whole graph in
//! traversal order (regions, shells, face uses, loop uses, the CCW edge use rings with
//! mates, radials and vertices, the B-rep's face/edge/vertex lists, each edge's use list)
//! and its values (edge intervals and end points, vertex points, face domains,
//! tolerances).
//!
//! Built-in cases: solids from prisms (cubes, polygons, boxes with square and polygonal
//! holes through them), cylinders (seam edges), a vertex-loop sphere, a box with a void,
//! open shells, a non-manifold fan of three faces on one edge, the hand-written STEP
//! tetrahedron and disk of `step-import`, each with limits from vertex points or curve
//! parameters, reversed faces, and as separate edges stitched by `EndShell(TRUE)`; faces
//! bounded by lifted UV lines and B-splines; and malformed call sequences (calls out of
//! state, null curves, decreasing limits, missing curves). Generated cases: random shapes,
//! scales, offsets and options, with a random mutation of the call sequence (an op removed,
//! duplicated, swapped or reoriented) in some.

// The shape builders index the vectors they have just built.
#![allow(clippy::indexing_slicing)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore_ffi::units::topology_brep_constructor as ffi;
use step_import::model::{
    BSplineCurve, Body, BodyKind, Bound, Curve, Edge, Face, Frame, Loop, OrientedEdge, Shell,
    Surface, Vertex, P3,
};
use step_import::script::{
    self, FromBodyOptions, Node, Op, Orient, RegionRef, Script, ScriptCurve,
};

use crate::constructor::{self, blocks, Deferred};
use crate::runner::default_cases;
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};

const UNIT: &str = "topology.brep_constructor";

/// The runner.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    /// The encoded script.
    script: Vec<f64>,
}

fn harness(m: impl Into<String>) -> HarnessError {
    HarnessError::new(m.into())
}

/// The comparable output of a run's blocks.
fn output(out: &[f64]) -> Result<Output, HarnessError> {
    let bl = blocks(out).map_err(harness)?;
    let mut geometry = Vec::new();
    let mut ops = Vec::new();
    let mut state = Vec::new();
    let mut topology = Vec::new();
    let mut values = Vec::new();
    let mut crash = -1i64;
    for (tag, v) in bl {
        let ints = |v: &[f64]| v.iter().map(|&x| x as i64).collect::<Vec<i64>>();
        match tag {
            1 => geometry = ints(&v),
            2 => ops.extend(ints(&v)),
            3 => crash = v.first().map_or(-1, |&x| x as i64),
            6 => state = ints(&v),
            4 => topology = ints(&v),
            5 => values = v,
            other => return Err(harness(format!("unexpected block {other}"))),
        }
    }
    Ok(Output::new()
        .ints("geometry", geometry)
        .ints("ops", ops)
        .int("crash_at_op", crash)
        .ints("state", state)
        .ints("topology", topology)
        .values("values", values))
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        UNIT
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        match ffi::run(&i.script, 0, 1e-6) {
            Ok(out) => output(&out),
            Err(e) => Err(harness(e.to_string())),
        }
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let s = script::decode(&i.script).map_err(harness)?;
        let mut notes = Vec::new();
        let (out, deferred) = constructor::run(&s, 0, 1e-6, &mut notes);
        match deferred {
            Some(Deferred(unit, symbol)) => Ok(Output::deferred(unit, symbol)),
            None => output(&out),
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let shape = random_shape(rng);
        let options = FromBodyOptions {
            parameter_limits: rng.chance(0.3),
            stitch: rng.chance(0.2),
            natural_domains: rng.chance(0.2),
            separate_edges: false,
        };
        let mut s = script::from_body(&shape, options);
        if rng.chance(0.4) {
            mutate(&mut s, rng);
        }
        inputs(&s)
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut v = default_cases(self, ctx)?;
        for (index, (name, s)) in builtin().into_iter().enumerate() {
            v.push(Case {
                unit: UNIT.to_string(),
                name,
                source: crate::CaseSource::Fixture {
                    path: "built-in: topology.brep_constructor cases".to_string(),
                    index,
                    upstream_ref: Some(
                        "prog_test smTopoTest.cpp my_test_seam_uv_redistribution_at_poles (SmBrepConstructor call pattern: shared edges, StartVertexOfLoop, SetLimits from points)"
                            .to_string(),
                    ),
                },
                inputs: serde_json::to_value(Inputs {
                    script: script::encode(&s),
                })
                .map_err(|e| harness(e.to_string()))?,
            });
        }
        Ok(v)
    }
}

fn inputs(s: &Script) -> Option<Value> {
    let flat = script::encode(s);
    script::decode(&flat).ok()?;
    serde_json::to_value(Inputs { script: flat }).ok()
}

// ---- shapes ---------------------------------------------------------------------------------

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: P3, b: P3) -> P3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: P3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn unit(a: P3) -> P3 {
    let n = norm(a);
    [a[0] / n, a[1] / n, a[2] / n]
}

fn neg(a: P3) -> P3 {
    [-a[0], -a[1], -a[2]]
}

fn frame(origin: P3, x: P3, y: P3) -> Frame {
    let (x, y) = (unit(x), unit(y));
    Frame {
        origin,
        x,
        y,
        z: cross(x, y),
    }
}

fn plane(origin: P3, x: P3, y: P3) -> Surface {
    Surface::Plane {
        frame: frame(origin, x, y),
    }
}

/// A line edge from vertex `a` (at `p`) to vertex `b` (at `q`), parametrised by length.
fn line_edge(a: usize, b: usize, p: P3, q: P3) -> Edge {
    Edge {
        step_id: 0,
        start: a,
        end: b,
        curve: Curve::Line {
            origin: p,
            dir: unit(sub(q, p)),
            magnitude: 1.0,
        },
        same_sense: true,
        t_start: 0.0,
        t_end: norm(sub(q, p)),
    }
}

fn oe(edge: usize, forward: bool) -> OrientedEdge {
    OrientedEdge { edge, forward }
}

fn edge_loop(outer: bool, es: Vec<OrientedEdge>) -> Loop {
    Loop {
        outer,
        orientation: true,
        bound: Bound::Edges(es),
    }
}

fn face(surface: Surface, loops: Vec<Loop>) -> Face {
    Face {
        step_id: 0,
        surface,
        same_sense: true,
        loops,
    }
}

fn body(kind: BodyKind, faces: Vec<Face>, edges: Vec<Edge>, vertices: Vec<P3>, closed: bool) -> Body {
    let n = faces.len();
    Body {
        step_id: 0,
        name: None,
        kind,
        unit_mm: 1.0,
        uncertainty_mm: None,
        shells: vec![Shell {
            step_id: 0,
            closed,
            orientation: true,
            faces: (0..n).collect(),
        }],
        faces,
        edges,
        vertices: vertices
            .into_iter()
            .map(|point| Vertex { step_id: 0, point })
            .collect(),
    }
}

/// A prism of height `h` over polygons in the XY plane (`polys[0]` counter-clockwise
/// seen from +Z, the holes clockwise), scaled by `scale` and moved by `offset`; the
/// faces have outward normals. Without `top` the top face is left out (an open shell).
fn prism(polys: &[Vec<[f64; 2]>], h: f64, scale: f64, offset: P3, top: bool) -> Body {
    let at = |p: [f64; 2], z: f64| {
        [
            p[0] * scale + offset[0],
            p[1] * scale + offset[1],
            z * scale + offset[2],
        ]
    };
    let mut vertices = Vec::new();
    let mut edges = Vec::new();
    // per polygon: (first bottom vertex, first bottom edge, first top edge, first vertical edge, n)
    let mut idx = Vec::new();
    for poly in polys {
        let n = poly.len();
        let v0 = vertices.len();
        for p in poly {
            vertices.push(at(*p, 0.0));
        }
        for p in poly {
            vertices.push(at(*p, h));
        }
        let (bv, tv) = (|j: usize| v0 + j % n, |j: usize| v0 + n + j % n);
        let e0 = edges.len();
        for j in 0..n {
            edges.push(line_edge(bv(j), bv(j + 1), vertices[bv(j)], vertices[bv(j + 1)]));
        }
        for j in 0..n {
            edges.push(line_edge(tv(j), tv(j + 1), vertices[tv(j)], vertices[tv(j + 1)]));
        }
        for j in 0..n {
            edges.push(line_edge(bv(j), tv(j), vertices[bv(j)], vertices[tv(j)]));
        }
        idx.push((e0, n));
    }
    let mut faces = Vec::new();
    // bottom, normal -Z: every polygon reversed
    let mut loops = Vec::new();
    for (i, &(e0, n)) in idx.iter().enumerate() {
        loops.push(edge_loop(i == 0, (0..n).rev().map(|j| oe(e0 + j, false)).collect()));
    }
    faces.push(face(
        plane(at([0.0, 0.0], 0.0), [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
        loops,
    ));
    if top {
        let loops = idx
            .iter()
            .enumerate()
            .map(|(i, &(e0, n))| edge_loop(i == 0, (0..n).map(|j| oe(e0 + n + j, true)).collect()))
            .collect();
        faces.push(face(
            plane(at([0.0, 0.0], h), [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            loops,
        ));
    }
    for &(e0, n) in &idx {
        for j in 0..n {
            let (b, t, v) = (e0 + j, e0 + n + j, |j: usize| e0 + 2 * n + j % n);
            let p = edges[b].clone();
            let Curve::Line { origin, dir, .. } = p.curve else {
                continue;
            };
            faces.push(face(
                plane(origin, dir, [0.0, 0.0, 1.0]),
                vec![edge_loop(
                    true,
                    vec![oe(b, true), oe(v(j + 1), true), oe(t, false), oe(v(j), false)],
                )],
            ));
        }
    }
    let kind = if top {
        BodyKind::ManifoldSolid
    } else {
        BodyKind::ShellModel
    };
    body(kind, faces, edges, vertices, top)
}

fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn ngon(n: usize, r: f64, phase: f64) -> Vec<[f64; 2]> {
    (0..n)
        .map(|i| {
            let a = phase + i as f64 * std::f64::consts::TAU / n as f64;
            [r * a.cos(), r * a.sin()]
        })
        .collect()
}

/// A cylinder of radius `r` and height `h` about the Z axis through `offset`: two
/// circular edges, one seam line, two planar caps and the cylindrical face.
fn cylinder(r: f64, h: f64, offset: P3) -> Body {
    let o = offset;
    let p0 = [o[0] + r, o[1], o[2]];
    let p1 = [o[0] + r, o[1], o[2] + h];
    let circle = |z: f64| Curve::Circle {
        frame: frame([o[0], o[1], o[2] + z], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        radius: r,
    };
    let edges = vec![
        Edge {
            step_id: 0,
            start: 0,
            end: 0,
            curve: circle(0.0),
            same_sense: true,
            t_start: 0.0,
            t_end: 360.0,
        },
        Edge {
            step_id: 0,
            start: 1,
            end: 1,
            curve: circle(h),
            same_sense: true,
            t_start: 0.0,
            t_end: 360.0,
        },
        line_edge(0, 1, p0, p1),
    ];
    let faces = vec![
        face(
            plane(o, [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
            vec![edge_loop(true, vec![oe(0, false)])],
        ),
        face(
            plane([o[0], o[1], o[2] + h], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            vec![edge_loop(true, vec![oe(1, true)])],
        ),
        face(
            Surface::Cylinder {
                frame: frame(o, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
                radius: r,
            },
            vec![edge_loop(
                true,
                vec![oe(0, true), oe(2, true), oe(1, false), oe(2, false)],
            )],
        ),
    ];
    body(BodyKind::ManifoldSolid, faces, edges, vec![p0, p1], true)
}

/// A sphere bounded by a single vertex loop at its north pole.
fn sphere(r: f64, offset: P3) -> Body {
    let pole = [offset[0], offset[1], offset[2] + r];
    let faces = vec![Face {
        step_id: 0,
        surface: Surface::Sphere {
            frame: frame(offset, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            radius: r,
        },
        same_sense: true,
        loops: vec![Loop {
            outer: true,
            orientation: true,
            bound: Bound::Vertex(0),
        }],
    }];
    body(BodyKind::ManifoldSolid, faces, Vec::new(), vec![pole], true)
}

/// `outer` with `inner`'s shell appended as a void (a `BREP_WITH_VOIDS`).
fn with_void(mut outer: Body, inner: Body) -> Body {
    let (nf, ne, nv) = (outer.faces.len(), outer.edges.len(), outer.vertices.len());
    outer.vertices.extend(inner.vertices);
    for mut e in inner.edges {
        e.start += nv;
        e.end += nv;
        outer.edges.push(e);
    }
    for mut f in inner.faces {
        for l in &mut f.loops {
            match &mut l.bound {
                Bound::Edges(es) => {
                    for e in es {
                        e.edge += ne;
                    }
                }
                Bound::Vertex(v) => *v += nv,
            }
        }
        outer.faces.push(f);
    }
    let n = outer.faces.len() - nf;
    outer.shells.push(Shell {
        step_id: 0,
        closed: true,
        orientation: false,
        faces: (nf..nf + n).collect(),
    });
    outer.kind = BodyKind::SolidWithVoids;
    outer
}

/// Three square sheets on the Z axis edge (an open, non-manifold shell).
fn fan(angles: &[f64]) -> Body {
    let mut vertices = vec![[0.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    let mut edges = vec![line_edge(0, 1, vertices[0], vertices[1])];
    let mut faces = Vec::new();
    for &a in angles {
        let d = [a.to_radians().cos(), a.to_radians().sin(), 0.0];
        let (v2, v3) = (vertices.len(), vertices.len() + 1);
        vertices.push(d);
        vertices.push([d[0], d[1], 1.0]);
        let e = edges.len();
        edges.push(line_edge(0, v2, vertices[0], vertices[v2]));
        edges.push(line_edge(v2, v3, vertices[v2], vertices[v3]));
        edges.push(line_edge(1, v3, vertices[1], vertices[v3]));
        faces.push(face(
            plane([0.0, 0.0, 0.0], d, [0.0, 0.0, 1.0]),
            vec![edge_loop(
                true,
                vec![oe(e, true), oe(e + 1, true), oe(e + 2, false), oe(0, false)],
            )],
        ));
    }
    body(BodyKind::ShellModel, faces, edges, vertices, false)
}

/// A cone frustum about the Z axis: bottom radius `r0`, height `h`, semi-angle `deg`
/// (the radius grows along +Z), with circular edges and a seam line.
fn cone(r0: f64, h: f64, deg: f64) -> Body {
    let r1 = r0 + h * deg.to_radians().tan();
    let mut b = cylinder(r0, h, [0.0; 3]);
    if let Some(e) = b.edges.get_mut(1) {
        if let Curve::Circle { radius, .. } = &mut e.curve {
            *radius = r1;
        }
    }
    let (p0, p1) = ([r0, 0.0, 0.0], [r1, 0.0, h]);
    b.vertices[1].point = p1;
    b.edges[2] = line_edge(0, 1, p0, p1);
    b.faces[2].surface = Surface::Cone {
        frame: frame([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        radius: r0,
        semi_angle_deg: deg,
    };
    b
}

/// An open shell of one planar face bounded by one closed ellipse.
fn ellipse_disk(a: f64, c: f64) -> Body {
    let edges = vec![Edge {
        step_id: 0,
        start: 0,
        end: 0,
        curve: Curve::Ellipse {
            frame: frame([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            semi1: a,
            semi2: c,
        },
        same_sense: true,
        t_start: 0.0,
        t_end: 360.0,
    }];
    let faces = vec![face(
        plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        vec![edge_loop(true, vec![oe(0, true)])],
    )];
    body(BodyKind::ShellModel, faces, edges, vec![[a, 0.0, 0.0]], false)
}

fn cubic(points: Vec<P3>) -> Curve {
    Curve::BSpline(BSplineCurve {
        degree: 3,
        points,
        weights: None,
        knots: vec![0.0, 1.0],
        mults: vec![4, 4],
    })
}

fn cubic_edge(a: usize, b: usize, points: Vec<P3>) -> Edge {
    Edge {
        step_id: 0,
        start: a,
        end: b,
        curve: cubic(points),
        same_sense: true,
        t_start: 0.0,
        t_end: 1.0,
    }
}

/// A planar face bounded by a cubic B-spline edge and three lines (an open shell);
/// with `bump`, a bicubic B-spline patch bounded by its four boundary curves.
fn bspline_face(bump: bool) -> Body {
    let z = |i: usize, j: usize| {
        if bump && (1..3).contains(&i) && (1..3).contains(&j) {
            0.4
        } else if bump {
            0.1 * (i as f64) - 0.05 * (j as f64)
        } else {
            0.0
        }
    };
    let net: Vec<Vec<P3>> = (0..4)
        .map(|i| {
            (0..4)
                .map(|j| {
                    let (x, y) = (i as f64 * 2.0 / 3.0, j as f64 / 3.0);
                    let y = if !bump && j == 0 { [0.0, -0.5, 0.5, 0.0][i] } else { y };
                    [x, y, z(i, j)]
                })
                .collect()
        })
        .collect();
    let row = |j: usize| (0..4).map(|i| net[i][j]).collect::<Vec<P3>>();
    let col = |i: usize| net[i].clone();
    let vertices = vec![net[0][0], net[3][0], net[3][3], net[0][3]];
    let (edges, surface) = if bump {
        (
            vec![
                cubic_edge(0, 1, row(0)),
                cubic_edge(1, 2, col(3)),
                cubic_edge(3, 2, row(3)),
                cubic_edge(0, 3, col(0)),
            ],
            Surface::BSpline(step_import::model::BSplineSurface {
                u_degree: 3,
                v_degree: 3,
                points: net.clone(),
                weights: None,
                u_knots: vec![0.0, 1.0],
                u_mults: vec![4, 4],
                v_knots: vec![0.0, 1.0],
                v_mults: vec![4, 4],
            }),
        )
    } else {
        (
            vec![
                cubic_edge(0, 1, row(0)),
                line_edge(1, 2, vertices[1], vertices[2]),
                line_edge(3, 2, vertices[3], vertices[2]),
                line_edge(0, 3, vertices[0], vertices[3]),
            ],
            plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        )
    };
    let faces = vec![face(
        surface,
        vec![edge_loop(
            true,
            vec![oe(0, true), oe(1, true), oe(2, false), oe(3, false)],
        )],
    )];
    body(BodyKind::ShellModel, faces, edges, vertices, false)
}

/// Reverses face `fi`'s plane (its frame's Y axis flipped, `same_sense` false): the face
/// normal and loops stay, the surface normal turns.
fn reverse_plane(b: &mut Body, fi: usize) {
    if let Some(f) = b.faces.get_mut(fi) {
        if let Surface::Plane { frame: fr } = &mut f.surface {
            fr.y = neg(fr.y);
            fr.z = neg(fr.z);
            f.same_sense = !f.same_sense;
        }
    }
}

const TETRA_STEP: &str = include_str!("topology_brep_constructor_tetra.stp");

fn step_bodies() -> Vec<Body> {
    step_import::import(TETRA_STEP.as_bytes(), &step_import::Limits::default())
        .map(|m| m.bodies)
        .unwrap_or_default()
}

fn shapes() -> Vec<(String, Body)> {
    let o = [0.0, 0.0, 0.0];
    let mut v = vec![
        ("cube".to_string(), prism(&[square(0.0, 0.0, 1.0, 1.0)], 1.0, 1.0, o, true)),
        (
            "box_offset".to_string(),
            prism(&[square(-2.0, 0.5, 3.0, 2.0)], 0.7, 1.0, [10.0, -5.0, 3.0], true),
        ),
        ("triangle_prism".to_string(), prism(&[ngon(3, 1.0, 0.3)], 2.0, 1.0, o, true)),
        ("hexagon_prism".to_string(), prism(&[ngon(6, 2.0, 0.0)], 0.5, 1.0, o, true)),
        (
            "box_square_hole".to_string(),
            prism(
                &[square(0.0, 0.0, 4.0, 4.0), {
                    let mut h = square(1.0, 1.0, 3.0, 3.0);
                    h.reverse();
                    h
                }],
                1.0,
                1.0,
                o,
                true,
            ),
        ),
        (
            "box_two_holes".to_string(),
            prism(
                &[
                    square(0.0, 0.0, 6.0, 3.0),
                    {
                        let mut h = square(1.0, 1.0, 2.0, 2.0);
                        h.reverse();
                        h
                    },
                    {
                        let mut h = ngon(5, 0.6, 0.1)
                            .into_iter()
                            .map(|p| [p[0] + 4.0, p[1] + 1.5])
                            .collect::<Vec<_>>();
                        h.reverse();
                        h
                    },
                ],
                2.0,
                1.0,
                o,
                true,
            ),
        ),
        (
            "l_shape".to_string(),
            prism(
                &[vec![
                    [0.0, 0.0],
                    [3.0, 0.0],
                    [3.0, 1.0],
                    [1.0, 1.0],
                    [1.0, 3.0],
                    [0.0, 3.0],
                ]],
                1.0,
                1.0,
                o,
                true,
            ),
        ),
        ("cube_tiny".to_string(), prism(&[square(0.0, 0.0, 1.0, 1.0)], 1.0, 1e-4, o, true)),
        ("cube_huge".to_string(), prism(&[square(0.0, 0.0, 1.0, 1.0)], 1.0, 1e4, o, true)),
        (
            "cube_far".to_string(),
            prism(&[square(0.0, 0.0, 1.0, 1.0)], 1.0, 1.0, [1e5, -2e5, 3e4], true),
        ),
        ("open_box".to_string(), prism(&[square(0.0, 0.0, 2.0, 1.0)], 1.0, 1.0, o, false)),
        ("cylinder".to_string(), cylinder(1.0, 2.0, o)),
        ("cylinder_offset".to_string(), cylinder(0.25, 0.1, [3.0, 4.0, -1.0])),
        ("sphere_vertex_loop".to_string(), sphere(2.0, o)),
        (
            "box_with_void".to_string(),
            with_void(
                prism(&[square(0.0, 0.0, 4.0, 4.0)], 4.0, 1.0, o, true),
                prism(&[square(1.0, 1.0, 2.0, 2.0)], 1.0, 1.0, [0.0, 0.0, 1.5], true),
            ),
        ),
        ("fan_three".to_string(), fan(&[0.0, 120.0, 240.0])),
        ("fan_four".to_string(), fan(&[0.0, 90.0, 180.0, 270.0])),
        ("fan_two".to_string(), fan(&[0.0, 90.0])),
        ("cone_frustum".to_string(), cone(1.0, 1.5, 20.0)),
        ("ellipse_disk".to_string(), ellipse_disk(2.0, 0.75)),
        ("bspline_edge_face".to_string(), bspline_face(false)),
        ("bspline_patch".to_string(), bspline_face(true)),
    ];
    let mut rev = prism(&[square(0.0, 0.0, 1.0, 1.0)], 1.0, 1.0, o, true);
    for fi in [0, 2, 5] {
        reverse_plane(&mut rev, fi);
    }
    v.push(("cube_reversed_faces".to_string(), rev));
    let mut rev = prism(
        &[square(0.0, 0.0, 4.0, 4.0), {
            let mut h = square(1.0, 1.0, 3.0, 3.0);
            h.reverse();
            h
        }],
        1.0,
        1.0,
        o,
        true,
    );
    reverse_plane(&mut rev, 1);
    v.push(("box_hole_reversed_top".to_string(), rev));
    let mut inside_out = prism(&[square(0.0, 0.0, 1.0, 1.0)], 1.0, 1.0, o, true);
    inside_out.shells[0].orientation = false;
    v.push(("cube_shell_reversed".to_string(), inside_out));
    for (i, b) in step_bodies().into_iter().enumerate() {
        v.push((format!("step_tetra_file_body{i}"), b));
    }
    v
}

fn random_shape(rng: &mut Rng) -> Body {
    let scale = 10f64.powf(rng.unit() * 6.0 - 3.0);
    let p = rng.point3(-100.0, 100.0);
    let offset = [p[0] * scale, p[1] * scale, p[2] * scale];
    let mut b = match rng.below(7) {
        0 => {
            let w = 0.2 + rng.unit() * 3.0;
            let d = 0.2 + rng.unit() * 3.0;
            prism(&[square(0.0, 0.0, w, d)], 0.1 + rng.unit(), scale, offset, rng.chance(0.8))
        }
        1 => {
            let n = 3 + rng.below(6) as usize;
            prism(&[ngon(n, 1.0, rng.unit())], 0.1 + rng.unit() * 2.0, scale, offset, true)
        }
        2 => {
            let mut h = square(0.3, 0.3, 0.7, 0.6);
            h.reverse();
            prism(&[square(0.0, 0.0, 1.0, 1.0), h], 0.5, scale, offset, true)
        }
        3 => cylinder(scale * (0.1 + rng.unit()), scale * (0.1 + rng.unit() * 3.0), offset),
        4 => sphere(scale * (0.1 + rng.unit()), offset),
        5 => {
            let a = rng.unit() * 90.0;
            fan(&[a, a + 100.0 + rng.unit() * 20.0, a + 230.0])
        }
        _ => with_void(
            prism(&[square(0.0, 0.0, 3.0, 3.0)], 3.0, scale, offset, true),
            prism(&[square(1.0, 1.0, 2.0, 2.0)], 1.0, scale, [offset[0], offset[1], offset[2] + scale], true),
        ),
    };
    let n = b.faces.len();
    for fi in 0..n {
        if rng.chance(0.2) {
            reverse_plane(&mut b, fi);
        }
    }
    b
}

/// One random change of the call sequence (kept only when the script stays valid).
fn mutate(s: &mut Script, rng: &mut Rng) {
    let n = s.ops.len();
    if n == 0 {
        return;
    }
    let i = rng.below(n as u64) as usize;
    let mut t = s.clone();
    match rng.below(7) {
        0 => {
            t.ops.remove(i);
        }
        1 => {
            let op = t.ops[i].clone();
            t.ops.insert(i, op);
        }
        2 if i + 1 < n => t.ops.swap(i, i + 1),
        3 => {
            let o = match rng.below(3) {
                0 => Orient::Unknown,
                1 => Orient::Same,
                _ => Orient::Opposite,
            };
            match &mut t.ops[i] {
                Op::StartEdge { orient, .. } => *orient = o,
                Op::StartLoop(x) | Op::StartFace(x) => *x = o,
                _ => {}
            }
        }
        4 => {
            if let Op::SetLimits(a, b) = &t.ops[i] {
                let (a, b) = (*a, *b);
                t.ops[i] = Op::SetLimits(b, a);
            }
        }
        5 => t.ops.insert(i, Op::SwapRegions),
        _ => {
            if let Op::EndShell { stitch } = &mut t.ops[i] {
                *stitch = !*stitch;
            }
        }
    }
    if script::decode(&script::encode(&t)).is_ok() {
        *s = t;
    }
}

// ---- hand-written scripts -----------------------------------------------------------------

fn line3(p: P3, q: P3) -> ScriptCurve {
    let e = line_edge(0, 0, p, q);
    ScriptCurve {
        dim: 3,
        bounds: Some((e.t_start, e.t_end)),
        curve: e.curve,
    }
}

fn line2(p: [f64; 2], q: [f64; 2]) -> ScriptCurve {
    let d = [q[0] - p[0], q[1] - p[1], 0.0];
    let len = norm(d);
    ScriptCurve {
        dim: 2,
        bounds: Some((0.0, len)),
        curve: Curve::Line {
            origin: [p[0], p[1], 0.0],
            dir: unit(d),
            magnitude: 1.0,
        },
    }
}

/// A planar square face (the XY plane) built from four UV curves that are lifted to
/// make the edges; `bspline` makes the first one a cubic B-spline.
fn uv_square(bspline: bool, limits_by_point: bool) -> Script {
    let mut s = Script {
        surfaces: vec![plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])],
        ..Script::default()
    };
    let corners = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    s.ops.extend([
        Op::StartBrep,
        Op::StartShell,
        Op::StartFace(Orient::Same),
        Op::SetFaceSurface {
            surface: 0,
            natural: false,
            domain: [-1.0, -1.0, 2.0, 2.0],
        },
        Op::StartLoop(Orient::Same),
    ]);
    for j in 0..4 {
        let (p, q) = (corners[j], corners[(j + 1) % 4]);
        let c = if bspline && j == 0 {
            ScriptCurve {
                dim: 2,
                bounds: None,
                curve: Curve::BSpline(BSplineCurve {
                    degree: 3,
                    points: vec![
                        [0.0, 0.0, 0.0],
                        [0.3, -0.2, 0.0],
                        [0.7, 0.2, 0.0],
                        [1.0, 0.0, 0.0],
                    ],
                    weights: None,
                    knots: vec![0.0, 1.0],
                    mults: vec![4, 4],
                }),
            }
        } else {
            line2(p, q)
        };
        let idx = s.curves.len();
        s.curves.push(c);
        s.ops.push(Op::StartEdge {
            orient: Orient::Same,
            edge: None,
            start: None,
            end: None,
        });
        s.ops.push(Op::SetEdgeCurve {
            curve: Some(idx),
            uv: true,
        });
        if limits_by_point {
            s.ops.push(Op::SetLimits(
                Node::Point([p[0], p[1], 0.0]),
                Node::Point([q[0], q[1], 0.0]),
            ));
        } else if !(bspline && j == 0) {
            s.ops.push(Op::SetLimits(
                Node::Parameter(0.0),
                Node::Parameter(norm([q[0] - p[0], q[1] - p[1], 0.0])),
            ));
        } else {
            s.ops.push(Op::SetLimits(Node::Parameter(0.0), Node::Parameter(1.0)));
        }
        s.ops.push(Op::EndEdge);
    }
    s.ops.extend([
        Op::EndLoop,
        Op::EndFace,
        Op::EndShell { stitch: false },
        Op::EndBrep,
    ]);
    s
}

/// A one-face open shell (a unit square in the XY plane) as the calls `ops` make it,
/// for the malformed variants: the edges are made with 3D lines.
fn square_face_ops(curves: &mut Vec<ScriptCurve>) -> Vec<Op> {
    let pts = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let mut ops = vec![
        Op::StartBrep,
        Op::StartShell,
        Op::StartFace(Orient::Same),
        Op::SetFaceSurface {
            surface: 0,
            natural: true,
            domain: [0.0; 4],
        },
        Op::StartLoop(Orient::Same),
    ];
    for j in 0..4 {
        let (p, q) = (pts[j], pts[(j + 1) % 4]);
        let c = curves.len();
        curves.push(line3(p, q));
        ops.push(Op::StartEdge {
            orient: Orient::Same,
            edge: None,
            start: None,
            end: None,
        });
        ops.push(Op::SetEdgeCurve {
            curve: Some(c),
            uv: false,
        });
        ops.push(Op::SetLimits(Node::Point(p), Node::Point(q)));
        ops.push(Op::EndEdge);
    }
    ops.extend([
        Op::EndLoop,
        Op::EndFace,
        Op::EndShell { stitch: false },
        Op::EndBrep,
    ]);
    ops
}

fn square_script(edit: impl FnOnce(&mut Vec<Op>)) -> Script {
    let mut curves = Vec::new();
    let mut ops = square_face_ops(&mut curves);
    edit(&mut ops);
    Script {
        curves,
        surfaces: vec![plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])],
        ops,
    }
}

fn position(ops: &[Op], f: impl Fn(&Op) -> bool) -> usize {
    ops.iter().position(f).unwrap_or(0)
}

fn malformed() -> Vec<(String, Script)> {
    let only = |ops: Vec<Op>| Script {
        ops,
        ..Script::default()
    };
    let mut v = vec![
        ("empty".to_string(), Script::default()),
        ("end_brep_without_start".to_string(), only(vec![Op::EndBrep])),
        (
            "start_brep_twice".to_string(),
            only(vec![Op::StartBrep, Op::StartBrep, Op::EndBrep]),
        ),
        (
            "calls_without_brep".to_string(),
            only(vec![
                Op::StartRegion {
                    solid: true,
                    outer: RegionRef::Null,
                },
                Op::EndRegion,
                Op::StartShell,
                Op::EndShell { stitch: false },
                Op::StartFace(Orient::Same),
                Op::EndFace,
                Op::StartLoop(Orient::Same),
                Op::EndLoop,
                Op::SingleVertexLoop([0.0; 3]),
                Op::StartEdge {
                    orient: Orient::Same,
                    edge: None,
                    start: None,
                    end: None,
                },
                Op::SetLimits(Node::Parameter(0.0), Node::Parameter(1.0)),
                Op::SetEdgeCurve {
                    curve: None,
                    uv: true,
                },
                Op::EndEdge,
                Op::EndVertex,
                Op::SwapRegions,
            ]),
        ),
        (
            "start_vertex_without_brep".to_string(),
            only(vec![Op::StartVertex([1.0, 2.0, 3.0])]),
        ),
        (
            "start_vertex_of_loop_without_brep".to_string(),
            only(vec![Op::StartVertexOfLoop([1.0, 2.0, 3.0])]),
        ),
        (
            "state_errors_in_brep".to_string(),
            only(vec![
                Op::StartBrep,
                Op::StartFace(Orient::Same),
                Op::StartLoop(Orient::Same),
                Op::EndRegion,
                Op::EndFace,
                Op::EndShell { stitch: false },
                Op::StartRegion {
                    solid: true,
                    outer: RegionRef::Null,
                },
                Op::StartRegion {
                    solid: false,
                    outer: RegionRef::Infinite,
                },
                Op::StartShell,
                Op::StartShell,
                Op::EndRegion,
                Op::EndBrep,
                Op::StartFace(Orient::Opposite),
                Op::EndShell { stitch: false },
                Op::StartLoop(Orient::Same),
                Op::EndFace,
                Op::SingleVertexLoop([1.0, 1.0, 1.0]),
                Op::EndLoop,
                Op::EndFace,
                Op::EndShell { stitch: false },
                Op::EndRegion,
                Op::StartVertex([4.0, 5.0, 6.0]),
                Op::EndVertex,
                Op::StartVertexOfLoop([0.0, 0.0, 0.0]),
                Op::EndBrep,
            ]),
        ),
        (
            "set_face_surface_without_face".to_string(),
            Script {
                surfaces: vec![plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])],
                ops: vec![
                    Op::StartBrep,
                    Op::SetFaceSurface {
                        surface: 0,
                        natural: true,
                        domain: [0.0; 4],
                    },
                ],
                ..Script::default()
            },
        ),
        (
            "regions_swapped_and_nested".to_string(),
            only(vec![
                Op::StartBrep,
                Op::StartRegion {
                    solid: true,
                    outer: RegionRef::Null,
                },
                Op::StartShell,
                Op::EndShell { stitch: false },
                Op::EndRegion,
                Op::StartRegion {
                    solid: true,
                    outer: RegionRef::Index(0),
                },
                Op::SwapRegions,
                Op::StartShell,
                Op::EndShell { stitch: false },
                Op::SwapRegions,
                Op::EndRegion,
                Op::StartRegion {
                    solid: false,
                    outer: RegionRef::Infinite,
                },
                Op::StartShell,
                Op::EndShell { stitch: true },
                Op::EndRegion,
                Op::EndBrep,
            ]),
        ),
    ];
    v.push(("square_face".to_string(), square_script(|_| {})));
    v.push((
        "square_no_curve_on_edge".to_string(),
        square_script(|ops| {
            let i = position(ops, |o| matches!(o, Op::SetEdgeCurve { .. }));
            ops.remove(i);
        }),
    ));
    v.push((
        "square_null_3d_curve".to_string(),
        square_script(|ops| {
            let i = position(ops, |o| matches!(o, Op::SetEdgeCurve { .. }));
            ops[i] = Op::SetEdgeCurve {
                curve: None,
                uv: false,
            };
        }),
    ));
    v.push((
        "square_null_uv_curve".to_string(),
        square_script(|ops| {
            let i = position(ops, |o| matches!(o, Op::SetEdgeCurve { .. }));
            ops.insert(
                i,
                Op::SetEdgeCurve {
                    curve: None,
                    uv: true,
                },
            );
        }),
    ));
    v.push((
        "square_curve_set_twice".to_string(),
        square_script(|ops| {
            let i = position(ops, |o| matches!(o, Op::SetEdgeCurve { .. }));
            if let Op::SetEdgeCurve { curve: Some(c), .. } = ops[i].clone() {
                // the next edge's curve on this edge: SM_ERR, and the next edge has none
                ops.insert(
                    i + 1,
                    Op::SetEdgeCurve {
                        curve: Some(c + 1),
                        uv: false,
                    },
                );
                let j = position(ops, |o| {
                    matches!(o, Op::SetEdgeCurve { curve: Some(x), .. } if *x == c + 1)
                });
                let k = ops
                    .iter()
                    .enumerate()
                    .skip(j + 1)
                    .find(|(_, o)| matches!(o, Op::SetEdgeCurve { curve: Some(x), .. } if *x == c + 1))
                    .map_or(0, |(k, _)| k);
                if k > 0 {
                    ops.remove(k);
                }
            }
        }),
    ));
    v.push((
        "square_decreasing_limits".to_string(),
        square_script(|ops| {
            let i = position(ops, |o| matches!(o, Op::SetLimits(..)));
            ops[i] = Op::SetLimits(Node::Parameter(0.75), Node::Parameter(0.25));
        }),
    ));
    v.push((
        "square_limits_parameter_beyond".to_string(),
        square_script(|ops| {
            let i = position(ops, |o| matches!(o, Op::SetLimits(..)));
            ops[i] = Op::SetLimits(Node::Parameter(-0.5), Node::Parameter(1.5));
        }),
    ));
    v.push((
        "square_limits_uv_and_empty_nodes".to_string(),
        square_script(|ops| {
            let i = position(ops, |o| matches!(o, Op::SetLimits(..)));
            ops[i] = Op::SetLimits(Node::Uv([0.2, 0.0]), Node::Empty);
        }),
    ));
    v.push((
        "square_limits_off_curve_points".to_string(),
        square_script(|ops| {
            let i = position(ops, |o| matches!(o, Op::SetLimits(..)));
            ops[i] = Op::SetLimits(Node::Point([0.3, 5.0, -2.0]), Node::Point([7.0, 0.0, 0.0]));
        }),
    ));
    v.push((
        "square_without_end_loop".to_string(),
        square_script(|ops| {
            let i = position(ops, |o| matches!(o, Op::EndLoop));
            ops.remove(i);
        }),
    ));
    v.push((
        "square_without_end_face".to_string(),
        square_script(|ops| {
            let i = position(ops, |o| matches!(o, Op::EndFace));
            ops.remove(i);
        }),
    ));
    v.push((
        "square_edges_opposite_in_loop".to_string(),
        square_script(|ops| {
            for o in ops.iter_mut() {
                if let Op::StartEdge { orient, .. } = o {
                    *orient = Orient::Opposite;
                }
            }
        }),
    ));
    v.push((
        "square_loop_opposite".to_string(),
        square_script(|ops| {
            let i = position(ops, |o| matches!(o, Op::StartLoop(_)));
            ops[i] = Op::StartLoop(Orient::Opposite);
        }),
    ));
    v.push((
        "square_orientation_unknown".to_string(),
        square_script(|ops| {
            for o in ops.iter_mut() {
                match o {
                    Op::StartEdge { orient, .. } => *orient = Orient::Unknown,
                    Op::StartLoop(x) | Op::StartFace(x) => *x = Orient::Unknown,
                    _ => {}
                }
            }
        }),
    ));
    v.push((
        "square_set_limits_after_end_loop".to_string(),
        square_script(|ops| {
            let i = position(ops, |o| matches!(o, Op::EndLoop));
            ops.insert(i + 1, Op::SetLimits(Node::Parameter(0.0), Node::Parameter(1.0)));
            ops.insert(i + 2, Op::EndEdge);
            ops.insert(
                i + 3,
                Op::SetEdgeCurve {
                    curve: None,
                    uv: false,
                },
            );
        }),
    ));
    v.push((
        "square_stitched".to_string(),
        square_script(|ops| {
            for o in ops.iter_mut() {
                if let Op::EndShell { stitch } = o {
                    *stitch = true;
                }
            }
        }),
    ));
    v.push((
        "square_with_vertex_loop_hole".to_string(),
        square_script(|ops| {
            let i = position(ops, |o| matches!(o, Op::EndLoop));
            ops.insert(i + 1, Op::SingleVertexLoop([0.5, 0.5, 0.0]));
        }),
    ));
    for (name, off) in [
        ("square_limits_half_tolerance_off", 0.5e-5),
        ("square_limits_fifty_tolerances_off", 50.0e-5),
        ("square_limits_far_off", 1.0),
    ] {
        v.push((
            name.to_string(),
            square_script(|ops| {
                let i = position(ops, |o| matches!(o, Op::SetLimits(..)));
                ops[i] = Op::SetLimits(
                    Node::Point([0.0, -off, off]),
                    Node::Point([1.0, off, -off]),
                );
            }),
        ));
    }
    v.push((
        "uv_flag_on_3d_circle".to_string(),
        Script {
            curves: vec![ScriptCurve {
                dim: 3,
                bounds: Some((0.0, 360.0)),
                curve: Curve::Circle {
                    frame: frame([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
                    radius: 0.5,
                },
            }],
            surfaces: vec![plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])],
            ops: vec![
                Op::StartBrep,
                Op::StartShell,
                Op::StartFace(Orient::Same),
                Op::SetFaceSurface {
                    surface: 0,
                    natural: true,
                    domain: [0.0; 4],
                },
                Op::StartLoop(Orient::Same),
                Op::StartEdge {
                    orient: Orient::Same,
                    edge: None,
                    start: None,
                    end: None,
                },
                Op::SetEdgeCurve {
                    curve: Some(0),
                    uv: true,
                },
                Op::SetLimits(Node::Parameter(0.0), Node::Parameter(360.0)),
                Op::EndEdge,
                Op::EndLoop,
                Op::EndFace,
                Op::EndShell { stitch: false },
                Op::EndBrep,
            ],
        },
    ));
    v.push(("uv_square_lines_param".to_string(), uv_square(false, false)));
    v.push(("uv_square_lines_points".to_string(), uv_square(false, true)));
    v.push(("uv_square_bspline_param".to_string(), uv_square(true, false)));
    v.push(("uv_square_bspline_points".to_string(), uv_square(true, true)));
    v
}

fn builtin() -> Vec<(String, Script)> {
    let mut v = Vec::new();
    let variants = [
        ("points", FromBodyOptions::default()),
        (
            "params",
            FromBodyOptions {
                parameter_limits: true,
                ..FromBodyOptions::default()
            },
        ),
        (
            "separate_stitched",
            FromBodyOptions {
                stitch: true,
                separate_edges: true,
                ..FromBodyOptions::default()
            },
        ),
        (
            "natural_domains",
            FromBodyOptions {
                natural_domains: true,
                ..FromBodyOptions::default()
            },
        ),
        (
            "shared_stitched",
            FromBodyOptions {
                stitch: true,
                ..FromBodyOptions::default()
            },
        ),
    ];
    for (name, b) in shapes() {
        for (vn, opts) in variants {
            v.push((format!("{name}_{vn}"), script::from_body(&b, opts)));
        }
    }
    v.extend(malformed());
    v
}
