//! STEP topology into schema 4: small solids written here entity by entity (boxes, a cylinder
//! with its seam, a plate with a through hole, sheets, a void, faces and edges written against
//! their geometry's sense), each read, validated, spot-checked and its faces' sides checked
//! against the material; hostile files are errors, never panics. Expected values are analytic.

use std::fmt::Write as _;

use super::*;
use crate::assembly::{OPEN_DOMAIN_SPAN, face_domain, onto_form, onto_knots, seams};
use crate::p21;
use solidcore_ops::pcurve::TrimFace;
use solidcore_record::brep::{Curve2, EdgeUseRef, Face, Loop, Region, Shell};

type V = [f64; 3];

fn real(x: f64) -> String {
    format!("{x:.17E}")
}
fn sub3(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn len3(a: V) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn neg(a: V) -> V {
    [-a[0], -a[1], -a[2]]
}

/// Writes a STEP data section one entity at a time.
#[derive(Default)]
struct W {
    s: String,
    n: u64,
}

impl W {
    fn add(&mut self, body: impl AsRef<str>) -> u64 {
        self.n += 1;
        let _ = writeln!(self.s, "#{}={};", self.n, body.as_ref());
        self.n
    }
    fn pt(&mut self, p: V) -> u64 {
        self.add(format!("CARTESIAN_POINT('',({},{},{}))", real(p[0]), real(p[1]), real(p[2])))
    }
    fn dir(&mut self, d: V) -> u64 {
        self.add(format!("DIRECTION('',({},{},{}))", real(d[0]), real(d[1]), real(d[2])))
    }
    fn axis(&mut self, o: V, z: V, x: V) -> u64 {
        let (o, z, x) = (self.pt(o), self.dir(z), self.dir(x));
        self.add(format!("AXIS2_PLACEMENT_3D('',#{o},#{z},#{x})"))
    }
    fn vertex(&mut self, p: V) -> u64 {
        let p = self.pt(p);
        self.add(format!("VERTEX_POINT('',#{p})"))
    }
    /// A line edge from `a` to `b`; with `same_sense` false its LINE runs from b to a.
    fn line_edge(&mut self, (va, pa): (u64, V), (vb, pb): (u64, V), same_sense: bool, magnitude: f64) -> u64 {
        let (o, d) = if same_sense { (pa, sub3(pb, pa)) } else { (pb, sub3(pa, pb)) };
        let (o, d) = (self.pt(o), self.dir(d));
        let v = self.add(format!("VECTOR('',#{d},{})", real(magnitude)));
        let l = self.add(format!("LINE('',#{o},#{v})"));
        self.add(format!("EDGE_CURVE('',#{va},#{vb},#{l},{})", b(same_sense)))
    }
    fn circle(&mut self, c: V, z: V, x: V, r: f64) -> u64 {
        let a = self.axis(c, z, x);
        self.add(format!("CIRCLE('',#{a},{})", real(r)))
    }
    fn oe(&mut self, edge: u64, orientation: bool) -> u64 {
        self.add(format!("ORIENTED_EDGE('',*,*,#{edge},{})", b(orientation)))
    }
    fn bound(&mut self, oes: &[u64], outer: bool, orientation: bool) -> u64 {
        let l = self.add(format!("EDGE_LOOP('',({}))", list(oes)));
        self.add(format!("{}('',#{l},{})", if outer { "FACE_OUTER_BOUND" } else { "FACE_BOUND" }, b(orientation)))
    }
    fn plane(&mut self, o: V, z: V, x: V) -> u64 {
        let a = self.axis(o, z, x);
        self.add(format!("PLANE('',#{a})"))
    }
    fn face(&mut self, bounds: &[u64], surface: u64, same_sense: bool) -> u64 {
        self.add(format!("ADVANCED_FACE('',({}),#{surface},{})", list(bounds), b(same_sense)))
    }
    /// The file: millimetres, radians, an uncertainty of 1e-7 mm, every item in one
    /// representation.
    fn file(self, items: &[u64]) -> p21::Exchange {
        self.file_with_uncertainty(items, 1e-7)
    }

    /// [`W::file`] with the file's uncertainty `uncertainty` (mm).
    fn file_with_uncertainty(mut self, items: &[u64], uncertainty: f64) -> p21::Exchange {
        let l = self.add("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
        let a = self.add("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))");
        let s = self.add("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())");
        let u = self.add(format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({}),#{l},'distance_accuracy_value','')", real(uncertainty)));
        let c = self.add(format!(
            "(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{u}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{l},#{a},#{s}))REPRESENTATION_CONTEXT('',''))"
        ));
        self.add(format!("ADVANCED_BREP_SHAPE_REPRESENTATION('',({}),#{c})", list(items)));
        parse(&self.s)
    }
}

fn parse(data: &str) -> p21::Exchange {
    let t = format!("ISO-10303-21;\nHEADER;FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));ENDSEC;\nDATA;\n{data}ENDSEC;\nEND-ISO-10303-21;\n");
    match p21::parse(&t) {
        Ok(ex) => ex,
        Err(e) => panic!("{e}\n{t}"),
    }
}

fn b(x: bool) -> &'static str {
    if x { ".T." } else { ".F." }
}
fn list(ids: &[u64]) -> String {
    ids.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(",")
}

/// How a box is written.
#[derive(Clone, Copy, Default)]
struct BoxStyle {
    /// Every other face on a plane facing into the box, with same_sense .F.
    flip_faces: bool,
    /// Every other edge with same_sense .F. (its line running from its end to its start).
    flip_edges: bool,
    /// Every face's bound written the other way round with orientation .F.
    flip_bounds: bool,
    /// One face's same_sense written wrong (its plane facing in, same_sense .T.).
    wrong_face: bool,
    /// One edge's same_sense written wrong (its line running from start to end, same_sense .F.).
    wrong_edge: bool,
    /// Every edge a second face uses is written again for that face, as an EDGE_CURVE of its own
    /// on the same two vertices: on the first one's LINE (`Some(0.0)`), or on a LINE of its own
    /// whose origin lies the given distance off.
    twin_edges: Option<f64>,
    /// The first edge starts at a second VERTEX_POINT at its first corner's point, not at the
    /// corner's own vertex, moved by the given distance along x (0 for the same point).
    dup_vertex: Option<f64>,
}

/// The box from `lo` to `hi`: its faces (outward normals) and its top and bottom faces'
/// extra inner bounds' places (written by `inner`, given the face's axis sign).
fn box_faces(w: &mut W, lo: V, hi: V, style: BoxStyle, inner: &mut dyn FnMut(&mut W, f64) -> Vec<u64>) -> Vec<u64> {
    let corner = |i: usize| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
    let vs: Vec<(u64, V)> = (0..8).map(|i| (w.vertex(corner(i)), corner(i))).collect();
    let mut edges: std::collections::HashMap<(usize, usize), u64> = Default::default();
    let mut faces = Vec::new();
    let mut count = 0;
    for a in 0..3 {
        for side in [false, true] {
            let (bx, cx) = ((a + 1) % 3, (a + 2) % 3);
            let bit = |axis: usize| 1usize << axis;
            let base = if side { bit(a) } else { 0 };
            let mut ring = [base, base | bit(bx), base | bit(bx) | bit(cx), base | bit(cx)];
            if !side {
                ring.reverse();
            }
            let mut oes = Vec::new();
            for k in 0..4 {
                let (p, q) = (ring[k], ring[(k + 1) % 4]);
                let key = (p.min(q), p.max(q));
                let e = match edges.get(&key) {
                    Some(e) if style.twin_edges.is_some() => {
                        let line = w.s.lines().find(|l| l.starts_with(&format!("#{e}="))).map(str::to_string).unwrap_or_default();
                        let body = line.split_once('=').map(|(_, b)| b.trim_end_matches(';').to_string()).unwrap_or_default();
                        match style.twin_edges {
                            Some(off) if off != 0.0 => {
                                let (va, pa) = vs[key.0];
                                let (vb, pb) = vs[key.1];
                                w.line_edge((va, [pa[0] + off, pa[1], pa[2]]), (vb, pb), true, 1.0)
                            }
                            _ => w.add(body),
                        }
                    }
                    Some(e) => *e,
                    None => {
                        let n = edges.len();
                        let flip = style.flip_edges && n % 2 == 1;
                        let e = if style.wrong_edge && n == 3 {
                            // Its line runs from start to end, but it says it runs against it.
                            let (va, pa) = vs[key.0];
                            let (vb, pb) = vs[key.1];
                            let (o, d) = (w.pt(pa), w.dir(sub3(pb, pa)));
                            let v = w.add(format!("VECTOR('',#{d},1.)"));
                            let l = w.add(format!("LINE('',#{o},#{v})"));
                            w.add(format!("EDGE_CURVE('',#{va},#{vb},#{l},.F.)"))
                        } else if let Some(off) = style.dup_vertex.filter(|_| n == 0) {
                            let at = [vs[key.0].1[0] + off, vs[key.0].1[1], vs[key.0].1[2]];
                            let twin = (w.vertex(at), at);
                            w.line_edge(twin, vs[key.1], !flip, 1.0)
                        } else {
                            w.line_edge(vs[key.0], vs[key.1], !flip, if n.is_multiple_of(3) { 1.0 } else { 2.5 })
                        };
                        edges.insert(key, e);
                        e
                    }
                };
                oes.push(w.oe(e, p == key.0));
            }
            let mut normal = [0.0; 3];
            normal[a] = if side { 1.0 } else { -1.0 };
            let mut x = [0.0; 3];
            x[bx] = 1.0;
            let center = {
                let mut c = [0.5 * (lo[0] + hi[0]), 0.5 * (lo[1] + hi[1]), 0.5 * (lo[2] + hi[2])];
                c[a] = if side { hi[a] } else { lo[a] };
                c
            };
            let mut bounds = if style.flip_bounds {
                let rev: Vec<u64> = oes
                    .iter()
                    .rev()
                    .map(|&oe| {
                        // The same edges, the other way round.
                        let line = w.s.lines().find(|l| l.starts_with(&format!("#{oe}="))).map(str::to_string).unwrap_or_default();
                        let flipped = if line.ends_with(".T.);") { line.replace(".T.);", ".F.);") } else { line.replace(".F.);", ".T.);") };
                        let body = flipped.split_once('=').map(|(_, b)| b.trim_end_matches(';').to_string()).unwrap_or_default();
                        w.add(body)
                    })
                    .collect();
                vec![w.bound(&rev, true, false)]
            } else {
                vec![w.bound(&oes, true, true)]
            };
            if a == 2 {
                bounds.extend(inner(w, normal[2]));
            }
            let flip = style.flip_faces && count % 2 == 1;
            let wrong = style.wrong_face && count == 4;
            let plane = w.plane(center, if flip || wrong { neg(normal) } else { normal }, x);
            faces.push(w.face(&bounds, plane, !flip));
            count += 1;
        }
    }
    faces
}

fn closed_shell(w: &mut W, faces: &[u64]) -> u64 {
    w.add(format!("CLOSED_SHELL('',({}))", list(faces)))
}

fn one_solid(ex: &p21::Exchange) -> Solid {
    let r = read_solids(ex);
    assert!(r.failures.is_empty(), "{:?}", r.failures);
    assert_eq!(r.solids.len(), 1);
    let s = r.solids.into_iter().next().unwrap();
    solidcore_ops::record_check::validate(&s.brep).unwrap();
    solidcore_ops::record_check::agreement(&s.brep).unwrap();
    s
}

fn counts(b: &Brep) -> [usize; 5] {
    [b.regions.len(), b.shells.len(), b.faces.len(), b.edges.len(), b.vertices.len()]
}

/// Each oriented face of each shell: a point inside it and its normal there in that shell.
fn sides(b: &Brep) -> Vec<(usize, V, V)> {
    let mut out = Vec::new();
    for (si, s) in b.shells.iter().enumerate() {
        let Shell::Faces { faces, .. } = s else { continue };
        for of in faces {
            let f = &b.faces[of.face as usize];
            let eval = SurfaceEval::new(&f.surface).unwrap();
            // Inside the outer loop: its first use's middle, nudged to its left.
            let Loop::Edges { edges } = &f.loops[0] else { panic!("outer vertex loop") };
            let u = &edges[0];
            let [t0, t1] = u.pcurve.interval().unwrap();
            let m = 0.5 * (t0 + t1);
            let (a, c) = (solidcore_ops::pcurve::eval(&u.pcurve, m).unwrap(), solidcore_ops::pcurve::eval(&u.pcurve, m + 1e-4 * (t1 - t0)).unwrap());
            let along = if u.forward { [c[0] - a[0], c[1] - a[1]] } else { [a[0] - c[0], a[1] - c[1]] };
            let n = (along[0].hypot(along[1])).max(1e-300);
            let left = [-along[1] / n, along[0] / n];
            let uv = [a[0] + 1e-3 * left[0], a[1] + 1e-3 * left[1]];
            let (p, su, sv) = eval.derivs(uv[0], uv[1]).unwrap();
            let mut nn = su.cross(sv);
            if !of.forward {
                nn = nn * -1.0;
            }
            out.push((si, [p.x, p.y, p.z], [nn.x, nn.y, nn.z]));
        }
    }
    out
}

/// Every face of shell `shell` faces away from `inside(p)`.
#[track_caller]
fn faces_away(b: &Brep, shell: usize, inside: impl Fn(V) -> V) {
    for (s, p, n) in sides(b) {
        if s == shell {
            let i = inside(p);
            let d = sub3(p, i);
            assert!(d[0] * n[0] + d[1] * n[1] + d[2] * n[2] > 0.0, "shell {shell}: face at {p:?} has normal {n:?}");
        }
    }
}

fn cube(style: BoxStyle) -> p21::Exchange {
    let mut w = W::default();
    let faces = box_faces(&mut w, [0.0; 3], [10.0, 20.0, 30.0], style, &mut |_, _| Vec::new());
    let shell = closed_shell(&mut w, &faces);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('cube',#{shell})"));
    w.file(&[solid])
}

#[test]
fn a_cube_is_two_regions_six_faces_twelve_edges_eight_vertices() {
    for style in [
        BoxStyle::default(),
        BoxStyle { flip_faces: true, ..Default::default() },
        BoxStyle { flip_edges: true, ..Default::default() },
        BoxStyle { flip_bounds: true, ..Default::default() },
        BoxStyle { flip_faces: true, flip_edges: true, flip_bounds: true, ..Default::default() },
    ] {
        let s = one_solid(&cube(style));
        assert_eq!(s.name, "cube");
        assert!(!s.sheet);
        assert!(s.notes.is_empty(), "{:?}", s.notes);
        assert_eq!(counts(&s.brep), [2, 1, 6, 12, 8]);
        assert_eq!(s.brep.regions[0].shells, [0]);
        assert_eq!(s.brep.regions[1].shells, [0]);
        assert!((s.brep.tolerance - 1e-7).abs() < 1e-20);
        // Same_sense .F. faces are the reversed sides of their planes.
        let reversed = match &s.brep.shells[0] {
            Shell::Faces { faces, .. } => faces.iter().filter(|f| !f.forward).count(),
            _ => panic!(),
        };
        assert_eq!(reversed, if style.flip_faces { 3 } else { 0 });
        faces_away(&s.brep, 0, |_| [5.0, 10.0, 15.0]);
        // Every edge is a line on [0, length / magnitude], used twice, once each way.
        for e in &s.brep.edges {
            assert_eq!(e.uses.len(), 2);
            let BCurve::Line { start, end } = &e.curve else { panic!("{:?}", e.curve) };
            let [a, z] = e.vertices.map(|v| s.brep.vertices[v as usize].point);
            assert!(len3(sub3(*start, a)) < 1e-12, "{start:?} {a:?}");
            let m = len3(sub3(*end, *start));
            assert!((e.interval[1] * m - len3(sub3(z, a))).abs() < 1e-9 && e.interval[0] == 0.0);
            // Once each way about the faces' normals in the shell.
            let Shell::Faces { faces, .. } = &s.brep.shells[0] else { panic!() };
            let side = |u: &EdgeUseRef| u.upper == faces.iter().find(|f| f.face == u.face).unwrap().forward;
            assert_ne!(side(&e.uses[0]), side(&e.uses[1]));
        }
    }
}

/// A cube whose first edge starts at a second vertex at its corner's point (tangency-labs/solidcraft#17141):
/// the faces meeting there close by position, and the corner is one vertex.
#[test]
fn a_corner_written_as_two_vertices_at_one_point_is_one_vertex() {
    let s = one_solid(&cube(BoxStyle { dup_vertex: Some(0.0), ..Default::default() }));
    assert!(!s.sheet);
    assert_eq!(counts(&s.brep), [2, 1, 6, 12, 8]);
    faces_away(&s.brep, 0, |_| [5.0, 10.0, 15.0]);
}

/// Two vertices that really are apart (1 mm, against tolerances of 1e-7 mm) are never one: the
/// loop stays open and the solid is refused naming it, as before vertices could join.
#[test]
fn a_loop_whose_ends_are_apart_is_still_refused() {
    let r = read_solids(&cube(BoxStyle { dup_vertex: Some(1.0), ..Default::default() }));
    assert!(r.solids.is_empty(), "{}", r.solids.len());
    assert_eq!(r.failures.len(), 1);
    assert!(r.failures[0].message.contains("is not closed") || r.failures[0].message.contains("is not connected"), "{}", r.failures[0].message);
}

/// A builder over `ex` at its 1e-7 mm uncertainty, for reading single bounds and edges.
fn with_builder<T>(ex: &p21::Exchange, f: impl FnOnce(&mut Builder<'_, '_>) -> T) -> T {
    let mut reader = crate::read::Reader::new(ex, crate::read::Units::default());
    let mut budget = MAX_FILE_FACES;
    let mut b = Builder::new(ex, &mut reader, 1e-7, &mut budget, None);
    f(&mut b)
}

/// Two vertices 1.5e-7 mm apart (within their two 1e-7 mm tolerances) that are the two ends of
/// one LINE edge are never joined, even when a loop read before that edge meets them: the
/// loop is refused and the edge, read later, stays a line between two vertices.
#[test]
fn the_two_ends_of_an_edge_read_later_are_never_joined() {
    let mut w = W::default();
    let (x, p, q) = ([0.0, 5.0, 0.0], [0.0; 3], [1.5e-7, 0.0, 0.0]);
    let (vx, vp, vq) = ((w.vertex(x), x), (w.vertex(p), p), (w.vertex(q), q));
    let short = w.line_edge(vp, vq, true, 1.0);
    let (xp, qx) = (w.line_edge(vx, vp, true, 1.0), w.line_edge(vq, vx, true, 1.0));
    let (a, z) = (w.oe(xp, true), w.oe(qx, true));
    let bound = w.bound(&[a, z], true, true);
    let ex = w.file(&[bound]);
    with_builder(&ex, |b| {
        let mut edges = 0;
        let err = b.bound(bound, &mut edges).err().unwrap_or_default();
        assert!(err.contains("is not connected"), "{err}");
        let (e, _) = b.edge(short).unwrap();
        let [s, t] = b.asm.edges[e as usize].vertices;
        assert_ne!(s, t);
    });
}

/// A loop X→B, C→C (a circle), D→X with B, C and D at one point: every join resolves the
/// vertex the last one left, so B, C and D are one vertex and the loop's edges meet there.
#[test]
fn a_loop_through_three_vertices_at_one_point_joins_all_three() {
    let mut w = W::default();
    let (x, c) = ([0.0, 5.0, 0.0], [0.0; 3]);
    let vx = (w.vertex(x), x);
    let (vb, vc, vd) = ((w.vertex(c), c), w.vertex(c), (w.vertex(c), c));
    let xb = w.line_edge(vx, vb, true, 1.0);
    let circle = w.circle([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 1.0);
    let cc = w.add(format!("EDGE_CURVE('',#{vc},#{vc},#{circle},.T.)"));
    let dx = w.line_edge(vd, vx, true, 1.0);
    let oes = [w.oe(xb, true), w.oe(cc, true), w.oe(dx, true)];
    let bound = w.bound(&oes, true, true);
    let ex = w.file(&[bound]);
    with_builder(&ex, |b| {
        let mut edges = 0;
        let (Bound::Edges(uses), _) = b.bound(bound, &mut edges).unwrap() else { panic!("an edge loop") };
        b.join_vertices().unwrap();
        let ends: Vec<[u32; 2]> = uses
            .iter()
            .map(|&(e, fwd)| {
                let [s, t] = b.asm.edges[e as usize].vertices;
                if fwd { [s, t] } else { [t, s] }
            })
            .collect();
        for (k, e) in ends.iter().enumerate() {
            assert_eq!(e[1], ends[(k + 1) % ends.len()][0], "{ends:?}");
        }
        assert_eq!(b.asm.edges.iter().flat_map(|e| e.vertices).collect::<HashSet<_>>().len(), 2);
    });
}

#[test]
fn faces_and_edges_written_against_their_sense_are_read_by_their_loops_and_vertices() {
    let s = one_solid(&cube(BoxStyle { wrong_face: true, wrong_edge: true, ..Default::default() }));
    assert_eq!(counts(&s.brep), [2, 1, 6, 12, 8]);
    assert!(s.notes.iter().any(|n| n.contains("1 faces turned over")), "{:?}", s.notes);
    assert!(s.notes.iter().any(|n| n.contains("1 edges run against")), "{:?}", s.notes);
    faces_away(&s.brep, 0, |_| [5.0, 10.0, 15.0]);
}

/// The cylinder of radius `r` from z = z0 to z1 about (cx, cy): its vertices on the seam at
/// angle 0, its circles (counter-clockwise about +z) and its seam line (upwards).
struct Cyl {
    circles: [u64; 2],
    seam: u64,
    surface: u64,
}

fn cylinder(w: &mut W, c: [f64; 2], r: f64, z: [f64; 2]) -> Cyl {
    let p = |h: f64| [c[0] + r, c[1], h];
    let (v0, v1) = (w.vertex(p(z[0])), w.vertex(p(z[1])));
    let circle = |w: &mut W, v: u64, h: f64| {
        let k = w.circle([c[0], c[1], h], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], r);
        w.add(format!("EDGE_CURVE('',#{v},#{v},#{k},.T.)"))
    };
    let circles = [circle(w, v0, z[0]), circle(w, v1, z[1])];
    let seam = w.line_edge((v0, p(z[0])), (v1, p(z[1])), true, 1.0);
    let a = w.axis([c[0], c[1], 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let surface = w.add(format!("CYLINDRICAL_SURFACE('',#{a},{})", real(r)));
    Cyl { circles, seam, surface }
}

/// The cylinder's side as a face whose normal points out of the cylinder (`outward`) or into it.
fn side(w: &mut W, c: &Cyl, outward: bool) -> u64 {
    let [b, t] = c.circles;
    // Counter-clockwise in (angle, height): about the outward normal.
    let ring = [(b, true), (c.seam, true), (t, false), (c.seam, false)];
    let oes: Vec<u64> =
        if outward { ring.iter().map(|&(e, o)| w.oe(e, o)).collect() } else { ring.iter().rev().map(|&(e, o)| w.oe(e, !o)).collect() };
    let bound = w.bound(&oes, true, true);
    w.face(&[bound], c.surface, outward)
}

/// A quarter round of radius 0.05 mm, 1 mm long, in a file of 0.1 mm uncertainty (a
/// connector's pin edges): the cylinder is thinner than the tolerance, but it has no pole. Read
/// as collapsed within the tolerance everywhere, every point of its boundary was on a pole, its
/// box bounded nothing, and the face was refused as "a face whose boundary does not lie on its
/// surface".
#[test]
fn a_round_thinner_than_the_tolerance_reads() {
    let r = 0.05;
    let mut w = W::default();
    let (p0, p1, p2, p3) = ([r, 0.0, 0.0], [r, 1.0, 0.0], [0.0, 1.0, r], [0.0, 0.0, r]);
    let (v0, v1, v2, v3) = (w.vertex(p0), w.vertex(p1), w.vertex(p2), w.vertex(p3));
    // The cylinder's frame: axis -y, angle 0 on +x, a quarter turn on +z.
    let (axis, x) = ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0]);
    let along = w.line_edge((v0, p0), (v1, p1), true, 1.0);
    let k1 = w.circle([0.0, 1.0, 0.0], axis, x, r);
    let far = w.add(format!("EDGE_CURVE('',#{v1},#{v2},#{k1},.T.)"));
    let back = w.line_edge((v2, p2), (v3, p3), true, 1.0);
    let k0 = w.circle([0.0; 3], axis, x, r);
    let near = w.add(format!("EDGE_CURVE('',#{v0},#{v3},#{k0},.T.)"));
    let a = w.axis([0.0; 3], axis, x);
    let cylinder = w.add(format!("CYLINDRICAL_SURFACE('',#{a},{})", real(r)));
    let uses = [w.oe(along, true), w.oe(far, true), w.oe(back, true), w.oe(near, false)];
    let bound = w.bound(&uses, true, true);
    let face = w.face(&[bound], cylinder, true);
    let open = w.add(format!("OPEN_SHELL('',(#{face}))"));
    let model = w.add(format!("SHELL_BASED_SURFACE_MODEL('pin round',(#{open}))"));
    let s = one_solid(&w.file_with_uncertainty(&[model], 0.1));
    assert!(s.sheet);
    assert_eq!(counts(&s.brep), [1, 1, 1, 4, 4]);
    // The face's box: a quarter turn by the 1 mm length.
    let d = s.brep.faces[0].domain;
    assert!((d[1][1] - d[1][0] - 1.0).abs() < 1e-9 || (d[0][1] - d[0][0] - 1.0).abs() < 1e-9, "{d:?}");
}

#[test]
fn a_cylinder_has_a_seam_used_twice_by_its_side() {
    let mut w = W::default();
    let c = cylinder(&mut w, [1.0, 2.0], 5.0, [0.0, 10.0]);
    let side_face = side(&mut w, &c, true);
    let ob = w.oe(c.circles[0], false);
    let bottom_bound = w.bound(&[ob], true, true);
    let bottom_plane = w.plane([1.0, 2.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let bottom = w.face(&[bottom_bound], bottom_plane, false);
    let ot = w.oe(c.circles[1], true);
    let top_bound = w.bound(&[ot], true, true);
    let top_plane = w.plane([1.0, 2.0, 10.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let top = w.face(&[top_bound], top_plane, true);
    let shell = closed_shell(&mut w, &[bottom, side_face, top]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('',#{shell})"));
    let s = one_solid(&w.file(&[solid]));
    assert_eq!(counts(&s.brep), [2, 1, 3, 3, 2]);
    let seam = s.brep.edges.iter().find(|e| matches!(e.curve, BCurve::Line { .. })).unwrap();
    assert_eq!(seam.uses.len(), 2);
    assert_eq!(seam.uses[0].face, seam.uses[1].face);
    for e in &s.brep.edges {
        if matches!(e.curve, BCurve::Circle { .. }) {
            // On its NURBS form: a whole turn in angle, its interval in the form's parameter.
            let [a0, a1] = solidcore_ops::label::edge_curve_interval(e).unwrap();
            assert!(e.nurbs.is_some() && (a1 - a0 - TAU).abs() < 1e-12);
            assert_eq!(e.vertices[0], e.vertices[1]);
        }
    }
    faces_away(&s.brep, 0, |p| [1.0, 2.0, p[2].clamp(1.0, 9.0)]);
    // The side face spans one turn of the cylinder (four quarters of its NURBS form) and its height.
    let f = s.brep.faces.iter().find(|f| matches!(f.surface, BSurface::Cylinder { .. })).unwrap();
    assert!(
        f.nurbs.is_some() && (f.domain[0][1] - f.domain[0][0] - 4.0).abs() < 1e-9 && f.domain[1][0] < 1e-6 && f.domain[1][1] > 10.0 - 1e-6,
        "{:?}",
        f.domain
    );
}

#[test]
fn a_ball_bounded_by_its_seam_alone() {
    for same_sense in [true, false] {
        let mut w = W::default();
        let (n, south) = ([0.0, 0.0, 5.0], [0.0, 0.0, -5.0]);
        let (vn, vs) = (w.vertex(n), w.vertex(south));
        // The meridian through +x: angle 0 at the north pole, π at the south.
        let k = w.circle([0.0; 3], if same_sense { [0.0, 1.0, 0.0] } else { [0.0, -1.0, 0.0] }, [0.0, 0.0, 1.0], 5.0);
        let e = w.add(format!("EDGE_CURVE('',#{vn},#{vs},#{k},{})", b(same_sense)));
        let (o1, o2) = (w.oe(e, true), w.oe(e, false));
        let bound = w.bound(&[o1, o2], true, true);
        let a = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let sphere = w.add(format!("SPHERICAL_SURFACE('',#{a},5.)"));
        let face = w.face(&[bound], sphere, true);
        let shell = closed_shell(&mut w, &[face]);
        let solid = w.add(format!("MANIFOLD_SOLID_BREP('ball',#{shell})"));
        let s = one_solid(&w.file(&[solid]));
        assert_eq!(counts(&s.brep), [2, 1, 1, 1, 2]);
        let e = &s.brep.edges[0];
        let [a0, a1] = solidcore_ops::label::edge_curve_interval(e).unwrap();
        assert!((a1 - a0 - PI).abs() < 1e-12, "{:?}", e.interval);
        faces_away(&s.brep, 0, |_| [0.0; 3]);
    }
}

#[test]
fn a_cone_reaches_its_apex_through_a_vertex_loop() {
    let mut w = W::default();
    let v = w.vertex([5.0, 0.0, 0.0]);
    let apex = w.vertex([0.0, 0.0, 10.0]);
    let k = w.circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 5.0);
    let e = w.add(format!("EDGE_CURVE('',#{v},#{v},#{k},.T.)"));
    // The cone widens down -z from its apex at z = 10 to radius 5 at z = 0.
    let a = w.axis([0.0; 3], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
    let cone = w.add(format!("CONICAL_SURFACE('',#{a},5.,{})", real(0.5f64.atan())));
    let o = w.oe(e, true);
    let outer = w.bound(&[o], true, true);
    let vl = w.add(format!("VERTEX_LOOP('',#{apex})"));
    let point = w.add(format!("FACE_BOUND('',#{vl},.T.)"));
    let side = w.face(&[outer, point], cone, true);
    let o = w.oe(e, false);
    let base_bound = w.bound(&[o], true, true);
    let plane = w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let base = w.face(&[base_bound], plane, false);
    let shell = closed_shell(&mut w, &[side, base]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('cone',#{shell})"));
    let s = one_solid(&w.file(&[solid]));
    // A seam from the base circle's vertex to the apex replaces the vertex loop.
    assert_eq!(counts(&s.brep), [2, 1, 2, 2, 2]);
    assert!(s.notes.iter().any(|n| n.contains("1 seam edges added")), "{:?}", s.notes);
    let f = &s.brep.faces[0];
    assert_eq!(f.loops.len(), 1);
    assert!((f.domain[1][0] + 10.0).abs() < 1e-9, "{:?}", f.domain);
    faces_away(&s.brep, 0, |p| [0.0, 0.0, p[2].clamp(1.0, 2.0)]);
}

/// The same cone written with its seam: the base circle, then the seam out to the apex and back.
/// The loop turns once round the axis and closes at the apex, so it has its seam; reading it as
/// a loop without one gave the face a second seam, a sliver out and back the face's boundary
/// crossed (#5193: a turned pin's drill point after a STEP round trip did not tessellate).
#[test]
fn a_cone_with_its_seam_to_the_apex_gets_no_other() {
    let mut w = W::default();
    let (p, top) = ([5.0, 0.0, 0.0], [0.0, 0.0, 10.0]);
    let (v, apex) = (w.vertex(p), w.vertex(top));
    let k = w.circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 5.0);
    let e = w.add(format!("EDGE_CURVE('',#{v},#{v},#{k},.T.)"));
    let seam = w.line_edge((v, p), (apex, top), true, len3(sub3(top, p)));
    let a = w.axis([0.0; 3], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
    let cone = w.add(format!("CONICAL_SURFACE('',#{a},5.,{})", real(0.5f64.atan())));
    let uses = [w.oe(e, true), w.oe(seam, true), w.oe(seam, false)];
    let outer = w.bound(&uses, true, true);
    let side = w.face(&[outer], cone, true);
    let o = w.oe(e, false);
    let base_bound = w.bound(&[o], true, true);
    let plane = w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let base = w.face(&[base_bound], plane, false);
    let shell = closed_shell(&mut w, &[side, base]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('cone',#{shell})"));
    let s = one_solid(&w.file(&[solid]));
    assert_eq!(counts(&s.brep), [2, 1, 2, 2, 2]);
    assert!(!s.notes.iter().any(|n| n.contains("seam edges added")), "{:?}", s.notes);
    faces_away(&s.brep, 0, |p| [0.0, 0.0, p[2].clamp(1.0, 2.0)]);
}

/// A cone point (radius 3 at z = 0, half-angle atan ½, its apex at z = −6) closed by a disk,
/// every bound a plain FACE_BOUND, the file's uncertainty 1e-5 mm: its apex VERTEX_LOOP's point
/// written 1e-6 mm off the apex, across the axis or along it (within the uncertainty, as a
/// writer's rounding leaves it). The vertex is the cone's pole: the seam from the rim runs up the
/// ruling to the apex itself, the vertex's tolerance covering its gap, and the point reads closed.
#[test]
fn a_cone_point_whose_apex_vertex_is_a_little_off_its_apex_reads_closed() {
    for off in [[6e-7, 8e-7, 0.0], [0.0, 0.0, 1e-6], [0.0, 0.0, -1e-6]] {
        let mut w = W::default();
        let v = w.vertex([3.0, 0.0, 0.0]);
        let k = w.circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 3.0);
        let rim = w.add(format!("EDGE_CURVE('',#{v},#{v},#{k},.T.)"));
        let apex = w.vertex([off[0], off[1], -6.0 + off[2]]);
        let point = w.add(format!("VERTEX_LOOP('',#{apex})"));
        let point = w.add(format!("FACE_BOUND('',#{point},.T.)"));
        let a = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let cone = w.add(format!("CONICAL_SURFACE('',#{a},3.,{})", real(0.5f64.atan())));
        // The cone's normal points out and down: the face lies below its rim, which runs −u.
        let x = w.oe(rim, false);
        let bx = w.bound(&[x], false, true);
        let side = w.face(&[point, bx], cone, true);
        let plane = w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let x = w.oe(rim, true);
        let bx = w.bound(&[x], false, true);
        let disk = w.face(&[bx], plane, true);
        let shell = closed_shell(&mut w, &[side, disk]);
        let solid = w.add(format!("MANIFOLD_SOLID_BREP('point',#{shell})"));
        let s = one_solid(&w.file_with_uncertainty(&[solid], 1e-5));
        assert_eq!(counts(&s.brep), [2, 1, 2, 2, 2], "{off:?}");
        faces_away(&s.brep, 0, |p| [0.0, 0.0, p[2].clamp(-4.0, -1.0)]);
    }
}

/// A tube (radius 5 outside, 3 inside, from z = 0 to 4) whose cylinders carry no seam edges:
/// each side is bounded by two circles whose vertices sit at different angles.
#[test]
fn bands_and_caps_without_seams_get_one() {
    let mut w = W::default();
    let ring = |w: &mut W, r: f64, z: f64, at: f64| {
        let v = w.vertex([r * at.cos(), r * at.sin(), z]);
        let k = w.circle([0.0, 0.0, z], [0.0, 0.0, 1.0], [at.cos(), at.sin(), 0.0], r);
        w.add(format!("EDGE_CURVE('',#{v},#{v},#{k},.T.)"))
    };
    let (ob, ot, ib, it) = (ring(&mut w, 5.0, 0.0, 0.0), ring(&mut w, 5.0, 4.0, 1.0), ring(&mut w, 3.0, 0.0, 2.0), ring(&mut w, 3.0, 4.0, 3.0));
    let a = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let (outer_cyl, inner_cyl) = (w.add(format!("CYLINDRICAL_SURFACE('',#{a},5.)")), w.add(format!("CYLINDRICAL_SURFACE('',#{a},3.)")));
    // Outside: normal out; its bottom circle runs +u, its top −u.
    let (x, y) = (w.oe(ob, true), w.oe(ot, false));
    let (bx, by) = (w.bound(&[x], true, true), w.bound(&[y], false, true));
    let outside = w.face(&[bx, by], outer_cyl, true);
    // Inside: normal towards the axis (same_sense .F.): about it, the bottom runs −u.
    let (x, y) = (w.oe(ib, false), w.oe(it, true));
    let (bx, by) = (w.bound(&[x], true, true), w.bound(&[y], false, true));
    let inside = w.face(&[bx, by], inner_cyl, false);
    // Annuli: the bottom faces −z, the top +z.
    let plane_b = w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let (x, y) = (w.oe(ob, false), w.oe(ib, true));
    let (bx, by) = (w.bound(&[x], true, true), w.bound(&[y], false, true));
    let bottom = w.face(&[bx, by], plane_b, false);
    let plane_t = w.plane([0.0, 0.0, 4.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let (x, y) = (w.oe(ot, true), w.oe(it, false));
    let (bx, by) = (w.bound(&[x], true, true), w.bound(&[y], false, true));
    let top = w.face(&[bx, by], plane_t, true);
    let shell = closed_shell(&mut w, &[outside, inside, bottom, top]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('tube',#{shell})"));
    let s = one_solid(&w.file(&[solid]));
    // Each band's top circle split where the seam from its bottom vertex meets it.
    assert_eq!(counts(&s.brep), [2, 1, 4, 4 + 2 + 2, 4 + 2]);
    assert!(s.notes.iter().any(|n| n.contains("2 seam edges added")), "{:?}", s.notes);
    faces_away(&s.brep, 0, |p| {
        let r = p[0].hypot(p[1]);
        let k = 4.0 / r.max(1e-9);
        [p[0] * k, p[1] * k, p[2].clamp(1.0, 3.0)]
    });

    // A dome: a hemisphere bounded by its equator alone, closed by a disk.
    let mut w = W::default();
    let e = ring(&mut w, 5.0, 0.0, 0.5);
    let a = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let sphere = w.add(format!("SPHERICAL_SURFACE('',#{a},5.)"));
    let x = w.oe(e, true);
    let bx = w.bound(&[x], true, true);
    let dome = w.face(&[bx], sphere, true);
    let plane = w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let x = w.oe(e, false);
    let bx = w.bound(&[x], true, true);
    let disk = w.face(&[bx], plane, false);
    let shell = closed_shell(&mut w, &[dome, disk]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('dome',#{shell})"));
    let s = one_solid(&w.file(&[solid]));
    // A meridian from the equator's vertex to a new vertex at the north pole.
    assert_eq!(counts(&s.brep), [2, 1, 2, 2, 2]);
    let pole = s.brep.vertices.iter().find(|v| (v.point[2] - 5.0).abs() < 1e-9);
    assert!(pole.is_some(), "{:?}", s.brep.vertices);
    faces_away(&s.brep, 0, |p| [0.0, 0.0, p[2].clamp(1.0, 2.0)]);
}

/// A cylinder (r 5, h 10) with a hemispherical dome on top (#17151), its sphere's axis along -y
/// or along +z. Along -y the dome's rim, the top circle, is a meridian of the sphere through both
/// its poles: one edge passing through both, split at each so that the loop runs along each pole
/// at a vertex. Split at the first pole alone, the rest of the rim crossed the second inside an
/// edge, and the dome came in turned over, the solid closed with the wrong volume.
#[test]
fn a_dome_whose_rim_runs_through_both_poles_of_its_sphere() {
    let (r, h) = (5.0, 10.0);
    // pi r^2 h for the cylinder, 2/3 pi r^3 for the dome; its area 2 pi r h + pi r^2 + 2 pi r^2.
    let volume = PI * r * r * h + 2.0 / 3.0 * PI * r * r * r;
    let area = 2.0 * PI * r * h + 3.0 * PI * r * r;
    for axis in [[0.0, -1.0, 0.0], [0.0, 0.0, 1.0]] {
        let mut w = W::default();
        let (vb, vt) = (w.vertex([r, 0.0, 0.0]), w.vertex([r, 0.0, h]));
        let kb = w.circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], r);
        let kt = w.circle([0.0, 0.0, h], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], r);
        let bottom = w.add(format!("EDGE_CURVE('',#{vb},#{vb},#{kb},.T.)"));
        let top = w.add(format!("EDGE_CURVE('',#{vt},#{vt},#{kt},.T.)"));
        let side = w.line_edge((vb, [r, 0.0, 0.0]), (vt, [r, 0.0, h]), true, 1.0);
        let a = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let cyl = w.add(format!("CYLINDRICAL_SURFACE('',#{a},{})", real(r)));
        let uses = [w.oe(bottom, true), w.oe(side, true), w.oe(top, false), w.oe(side, false)];
        let bx = w.bound(&uses, true, true);
        let wall = w.face(&[bx], cyl, true);
        let plane = w.plane([0.0; 3], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
        let x = w.oe(bottom, false);
        let bx = w.bound(&[x], true, true);
        let base = w.face(&[bx], plane, true);
        let a = w.axis([0.0, 0.0, h], axis, [1.0, 0.0, 0.0]);
        let sphere = w.add(format!("SPHERICAL_SURFACE('',#{a},{})", real(r)));
        let x = w.oe(top, true);
        let bx = w.bound(&[x], true, true);
        let dome = w.face(&[bx], sphere, true);
        let shell = closed_shell(&mut w, &[wall, base, dome]);
        let solid = w.add(format!("MANIFOLD_SOLID_BREP('cylinder',#{shell})"));
        let s = one_solid(&w.file(&[solid]));
        assert!(!s.notes.iter().any(|n| n.contains("turned over")), "axis {axis:?}: {:?}", s.notes);
        if axis[2] == 0.0 {
            assert!(s.notes.iter().any(|n| n.contains("2 edges split where they pass through a face's pole")), "{:?}", s.notes);
        }
        faces_away(&s.brep, 0, |_| [0.0, 0.0, 0.75 * h]);
        let (body, _) = solidcore_ops::body::Body::from_record(&s.brep, false).unwrap();
        assert!(body.is_manifold_solid().unwrap());
        let mass = body.mass_properties(1e-6).unwrap();
        assert!((mass.volume - volume).abs() < 1e-3, "axis {axis:?}: {mass:?}, {volume} expected");
        assert!((mass.area - area).abs() < 1e-3, "axis {axis:?}: {mass:?}, {area} expected");
    }
}

/// A rod along x capped at its far end by a half sphere whose sphere has another axis (#19570).
/// With the sphere's axis z and its reference direction x, the cap is centred on the sphere's
/// seam and its rim runs through both poles. The cap must read with the file's sense, so the
/// solid has the volume of the rod plus the half sphere, whichever axis the sphere has.
#[test]
fn a_half_sphere_cap_centred_on_its_spheres_seam_keeps_its_sense() {
    let (r, h) = (1.0, 25.0);
    let volume = PI * r * r * h + 2.0 / 3.0 * PI * r * r * r;
    let area = 2.0 * PI * r * h + 3.0 * PI * r * r;
    let x = [1.0, 0.0, 0.0];
    for (axis, reference) in [([0.0, 0.0, 1.0], x), ([0.0, 1.0, 0.0], x), ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]), (x, [0.0, 0.0, 1.0])] {
        let mut w = W::default();
        let (vb, vt) = (w.vertex([0.0, 0.0, r]), w.vertex([h, 0.0, r]));
        let kb = w.circle([0.0; 3], x, [0.0, 0.0, 1.0], r);
        let kt = w.circle([h, 0.0, 0.0], x, [0.0, 0.0, 1.0], r);
        let bottom = w.add(format!("EDGE_CURVE('',#{vb},#{vb},#{kb},.T.)"));
        let top = w.add(format!("EDGE_CURVE('',#{vt},#{vt},#{kt},.T.)"));
        let side = w.line_edge((vb, [0.0, 0.0, r]), (vt, [h, 0.0, r]), true, 1.0);
        let a = w.axis([0.0; 3], x, [0.0, 0.0, 1.0]);
        let cyl = w.add(format!("CYLINDRICAL_SURFACE('',#{a},{})", real(r)));
        let uses = [w.oe(bottom, true), w.oe(side, true), w.oe(top, false), w.oe(side, false)];
        let bx = w.bound(&uses, true, true);
        let wall = w.face(&[bx], cyl, true);
        let plane = w.plane([0.0; 3], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let o = w.oe(bottom, false);
        let bx = w.bound(&[o], true, true);
        let base = w.face(&[bx], plane, true);
        let a = w.axis([h, 0.0, 0.0], axis, reference);
        let sphere = w.add(format!("SPHERICAL_SURFACE('',#{a},{})", real(r)));
        let o = w.oe(top, true);
        let bx = w.bound(&[o], true, true);
        let cap = w.face(&[bx], sphere, true);
        let shell = closed_shell(&mut w, &[wall, base, cap]);
        let solid = w.add(format!("MANIFOLD_SOLID_BREP('rod',#{shell})"));
        let s = one_solid(&w.file(&[solid]));
        assert!(!s.notes.iter().any(|n| n.contains("turned over")), "axis {axis:?} ref {reference:?}: {:?}", s.notes);
        let (body, _) = solidcore_ops::body::Body::from_record(&s.brep, false).unwrap();
        assert!(body.is_manifold_solid().unwrap());
        let mass = body.mass_properties(1e-6).unwrap();
        assert!((mass.volume - volume).abs() < 1e-3, "axis {axis:?} ref {reference:?}: {mass:?}, {volume} expected");
        assert!((mass.area - area).abs() < 1e-3, "axis {axis:?} ref {reference:?}: {mass:?}, {area} expected");
    }
}

/// An edge from vertex `va` counterclockwise about +z to vertex `vb` on the circle of radius `r`
/// round the z axis at height `z`.
fn arc_about_z(w: &mut W, (va, vb): (u64, u64), r: f64, z: f64) -> u64 {
    let k = w.circle([0.0, 0.0, z], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], r);
    w.add(format!("EDGE_CURVE('',#{va},#{vb},#{k},.T.)"))
}

/// A countersink's cone between a full circle at its rim and a loop that comes up to within
/// 0.05 mm of it below its only vertex and drops 0.5 mm elsewhere (a chamfer's edge broken across
/// it), in a file of 0.1 mm uncertainty. The seam from the rim's vertex straight down to the loop
/// would be 0.07 mm long, shorter than its own tolerance (an edge smaller than its tube): the face
/// was left out of the part with "a seam of no length". The seam goes where the band is wider.
#[test]
fn a_band_narrower_than_its_tolerance_at_a_vertex_gets_its_seam_where_it_is_wider() {
    for same_sense in [false, true] {
        let mut w = W::default();
        // A 45 degree cone opening up +z, radius 1 at z = 0: radius 1 + z at height z.
        let r = |z: f64| 1.0 + z;
        let at = |z: f64, angle: f64| [r(z) * angle.cos(), r(z) * angle.sin(), z];
        let (lo, hi, rim) = (0.5, 0.95, 1.0);
        let t = PI / 3.0;
        let v_rim = w.vertex(at(rim, 0.0));
        let rim_edge = arc_about_z(&mut w, (v_rim, v_rim), r(rim), rim);
        // The loop below: high from -60 to 60 degrees, low the rest of the way round.
        let (p_hi0, p_hi1, p_lo1, p_lo0) = (at(hi, -t), at(hi, t), at(lo, t), at(lo, -t));
        let (h0, h1, l1, l0) = (w.vertex(p_hi0), w.vertex(p_hi1), w.vertex(p_lo1), w.vertex(p_lo0));
        let high = arc_about_z(&mut w, (h0, h1), r(hi), hi);
        let down = w.line_edge((h1, p_hi1), (l1, p_lo1), true, len3(sub3(p_lo1, p_hi1)));
        let low = arc_about_z(&mut w, (l1, l0), r(lo), lo);
        let up = w.line_edge((l0, p_lo0), (h0, p_hi0), true, len3(sub3(p_hi0, p_lo0)));
        let a = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let cone = w.add(format!("CONICAL_SURFACE('',#{a},1.,{})", real(PI / 4.0)));
        // The face on the material's side of the cone: towards the axis for a countersink.
        let below: Vec<u64> = [(high, true), (down, true), (low, true), (up, true)].iter().map(|&(e, o)| w.oe(e, o == same_sense)).collect();
        let below = if same_sense { below } else { below.into_iter().rev().collect() };
        let ob = w.bound(&below, true, true);
        let o = w.oe(rim_edge, !same_sense);
        let rb = w.bound(&[o], false, true);
        let face = w.face(&[ob, rb], cone, same_sense);
        let open = w.add(format!("OPEN_SHELL('',(#{face}))"));
        let model = w.add(format!("SHELL_BASED_SURFACE_MODEL('countersink',(#{open}))"));
        let s = one_solid(&w.file_with_uncertainty(&[model], 0.1));
        assert!(s.sheet);
        assert!(s.notes.iter().any(|n| n.contains("1 seam edges added")), "{:?}", s.notes);
        // Every edge is longer than its tolerance. The loop's vertices at ±60 degrees end its
        // rulings, where a seam would run along an edge: the seam goes in the middle of the low
        // stretch, at 180 degrees where the band is 0.45 mm high, the low arc and the rim split
        // there.
        for e in &s.brep.edges {
            let [a, b] = e.vertices.map(|v| s.brep.vertices[v as usize].point);
            assert!(a == b || len3(sub3(a, b)) > e.tolerance, "an edge {:?} shorter than its tolerance {}", [a, b], e.tolerance);
        }
        assert_eq!(counts(&s.brep), [1, 1, 1, 5 + 2 + 1, 5 + 2], "same_sense {same_sense}");
        let seam = s.brep.edges.iter().find(|e| {
            let [a, b] = e.vertices.map(|v| s.brep.vertices[v as usize].point);
            (a[2] - b[2]).abs() > 0.475
        });
        let [a, b] = seam.unwrap().vertices.map(|v| s.brep.vertices[v as usize].point);
        assert!(a[1].atan2(a[0]).abs() > 2.0 * t && b[1].atan2(b[0]).abs() > 2.0 * t, "seam at {a:?} {b:?}");
    }
}

/// A cylinder band between a rim on the cylinder and one the file has 0.2 um outside it (a rim
/// written to less than the file's stated 0.01 um), their vertices half a turn apart. The seam
/// from the lower vertex up its ruling ends on the upper rim where that is split, 0.2 um off the
/// cylinder, so its middle is 0.1 um off: the face was left out with "a seam off its surface".
/// A seam is as far off as its ends are, and its tolerance says so.
#[test]
fn a_seam_to_a_rim_a_little_off_its_cylinder_is_no_farther_off_than_the_rim() {
    let mut w = W::default();
    let (r, off) = (10.0, 2e-4);
    let v_low = w.vertex([r, 0.0, 0.0]);
    let low = arc_about_z(&mut w, (v_low, v_low), r, 0.0);
    let v_high = w.vertex([-(r + off), 0.0, 5.0]);
    let k = w.circle([0.0, 0.0, 5.0], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], r + off);
    let high = w.add(format!("EDGE_CURVE('',#{v_high},#{v_high},#{k},.T.)"));
    let a = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let cylinder = w.add(format!("CYLINDRICAL_SURFACE('',#{a},{})", real(r)));
    let (x, y) = (w.oe(low, true), w.oe(high, false));
    let (bx, by) = (w.bound(&[x], true, true), w.bound(&[y], false, true));
    let face = w.face(&[bx, by], cylinder, true);
    let open = w.add(format!("OPEN_SHELL('',(#{face}))"));
    let model = w.add(format!("SHELL_BASED_SURFACE_MODEL('band',(#{open}))"));
    let s = one_solid(&w.file_with_uncertainty(&[model], 1e-5));
    assert!(s.notes.iter().any(|n| n.contains("1 seam edges added")), "{:?}", s.notes);
    // The seam: the one edge from the lower rim's height to the upper's (the halves of the
    // upper rim, split where the seam ends, also join two vertices), its tolerance covering
    // its gap.
    let seams: Vec<_> = s
        .brep
        .edges
        .iter()
        .filter(|e| {
            let [a, b] = e.vertices.map(|v| s.brep.vertices[v as usize].point);
            (a[2] - b[2]).abs() > 4.0
        })
        .collect();
    let [seam] = seams.as_slice() else { panic!("{} edges between the rims", seams.len()) };
    assert!(seam.tolerance >= 0.5 * off && seam.tolerance < 10.0 * off, "{}", seam.tolerance);
}

/// A spherical band (a dome with a ring of holes round a centre one) between a circle at its foot
/// and one round its top, both with their vertex at angle 0, and a hole straddling that angle
/// between them. The meridian through both vertices crosses the hole, so a seam there cut it in
/// two and the face was lost ("an edge lies 5.9 mm off the face's surface"). The seam goes where
/// no other loop of the face reaches: both rims split there.
#[test]
fn a_band_whose_vertices_line_up_across_a_hole_gets_its_seam_clear_of_it() {
    let radius = 5.0;
    let ring = |w: &mut W, z: f64| {
        let r = (radius * radius - z * z).sqrt();
        let v = w.vertex([r, 0.0, z]);
        let k = w.circle([0.0, 0.0, z], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], r);
        w.add(format!("EDGE_CURVE('',#{v},#{v},#{k},.T.)"))
    };
    let mut w = W::default();
    let (foot, top) = (ring(&mut w, 0.0), ring(&mut w, 4.0));
    // The hole: a window from 10 degrees west of +x to 10 east, between latitudes 20 and 40,
    // bounded by two parallels and two meridians.
    let (west, east, south, north) = (-PI / 18.0, PI / 18.0, PI / 9.0, 2.0 * PI / 9.0);
    let on = |lat: f64, lon: f64| [radius * lat.cos() * lon.cos(), radius * lat.cos() * lon.sin(), radius * lat.sin()];
    let (sw, se, ne, nw) = (w.vertex(on(south, west)), w.vertex(on(south, east)), w.vertex(on(north, east)), w.vertex(on(north, west)));
    let s_edge = arc_about_z(&mut w, (sw, se), radius * south.cos(), radius * south.sin());
    let n_edge = arc_about_z(&mut w, (nw, ne), radius * north.cos(), radius * north.sin());
    // A meridian with its angle the latitude.
    let meridian = |w: &mut W, lon: f64, (a, b): (u64, u64)| {
        let k = w.circle([0.0; 3], [lon.sin(), -lon.cos(), 0.0], [lon.cos(), lon.sin(), 0.0], radius);
        w.add(format!("EDGE_CURVE('',#{a},#{b},#{k},.T.)"))
    };
    let (w_edge, e_edge) = (meridian(&mut w, west, (sw, nw)), meridian(&mut w, east, (se, ne)));
    let a = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let sphere = w.add(format!("SPHERICAL_SURFACE('',#{a},{})", real(radius)));
    // Outward: the foot runs +u, the top -u, the hole clockwise seen from outside (north up its
    // west side, east along its north side, back south and west).
    let (f, t) = (w.oe(foot, true), w.oe(top, false));
    let hole = [(w_edge, true), (n_edge, true), (e_edge, false), (s_edge, false)].map(|(e, o)| w.oe(e, o));
    let bounds = [w.bound(&[f], true, true), w.bound(&[t], false, true), w.bound(&hole, false, true)];
    let face = w.face(&bounds, sphere, true);
    let open = w.add(format!("OPEN_SHELL('',(#{face}))"));
    let model = w.add(format!("SHELL_BASED_SURFACE_MODEL('dome',(#{open}))"));
    let s = one_solid(&w.file(&[model]));
    assert!(s.sheet);
    assert!(s.notes.iter().any(|n| n.contains("1 seam edges added")), "{:?}", s.notes);
    // The foot and the top split in two at the seam's angle, the hole whole: two loops, the
    // merged band and the hole.
    assert_eq!(counts(&s.brep), [1, 1, 1, 2 + 2 + 1 + 4, 2 + 2 + 4]);
    assert_eq!(s.brep.faces[0].loops.len(), 2);
    // The seam: the one edge from the foot to the top, clear of the hole's angles.
    let seam = s.brep.edges.iter().find(|e| {
        let [a, b] = e.vertices.map(|v| s.brep.vertices[v as usize].point);
        (a[2].abs() < 1e-9 && (b[2] - 4.0).abs() < 1e-9) || (b[2].abs() < 1e-9 && (a[2] - 4.0).abs() < 1e-9)
    });
    let seam = seam.unwrap_or_else(|| panic!("no seam from the foot to the top: {:?}", s.brep.vertices));
    let foot_end = seam.vertices.map(|v| s.brep.vertices[v as usize].point).into_iter().find(|p| p[2].abs() < 1e-9).unwrap();
    let angle = foot_end[1].atan2(foot_end[0]);
    assert!(angle.abs() > east, "the seam at {angle} rad crosses the hole");
}

/// A drill point's cone to its tip whose edge turns back partway round (a second drill point and
/// a flat cutting it): the loop goes once round, but over 350° to 360° it runs forward along the
/// rim, back a ring nearer the tip and forward again a ring nearer still, joined by rulings. A
/// seam at a vertex among those angles left the loop, followed from it, 370° wide, and the face
/// was taken for a thread's flank ("a loop that winds -1.0 turns about its axis and does not
/// close"). Its seam to the tip starts where the loop passes only once, the rim split there.
#[test]
fn a_cone_to_its_tip_whose_edge_turns_back_gets_its_seam_where_the_edge_passes_once() {
    let (deg, apex) = (PI / 180.0, -2.5);
    // The cone r = 2.5 + z about +z, its tip at z = -2.5.
    let at = |z: f64, a: f64| [(2.5 + z) * a.cos(), (2.5 + z) * a.sin(), z];
    let mut w = W::default();
    let corners = [(0.0, 20.0), (0.0, 0.0), (-0.5, 0.0), (-0.5, -10.0), (-1.0, -10.0), (-1.0, 20.0)].map(|(z, a)| (z, a * deg));
    let [a, b, c, d, e, f] = corners.map(|(z, t)| (w.vertex(at(z, t)), at(z, t)));
    // The arcs, each running +u: the rim from 20° round to 360°, the rings from -10°.
    let rim = arc_about_z(&mut w, (a.0, b.0), 2.5, 0.0);
    let near = arc_about_z(&mut w, (d.0, c.0), 2.0, -0.5);
    let nearer = arc_about_z(&mut w, (e.0, f.0), 1.5, -1.0);
    let ruling = |w: &mut W, p: (u64, V), q: (u64, V)| w.line_edge(p, q, true, len3(sub3(q.1, p.1)));
    let (af, ed, cb) = (ruling(&mut w, a, f), ruling(&mut w, e, d), ruling(&mut w, c, b));
    // The face lies between the loop and the tip (lower v): the loop runs -u overall, written
    // from a vertex where it turns back (-10° on the near ring).
    let uses = [(near, true), (cb, true), (rim, false), (af, true), (nearer, false), (ed, true)].map(|(x, o)| w.oe(x, o));
    let outer = w.bound(&uses, true, true);
    let tip = w.vertex([0.0, 0.0, apex]);
    let tip_loop = w.add(format!("VERTEX_LOOP('',#{tip})"));
    let tip_bound = w.add(format!("FACE_BOUND('',#{tip_loop},.T.)"));
    let axis = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let cone = w.add(format!("CONICAL_SURFACE('',#{axis},2.5,{})", real(PI / 4.0)));
    let face = w.face(&[outer, tip_bound], cone, true);
    let open = w.add(format!("OPEN_SHELL('',(#{face}))"));
    let model = w.add(format!("SHELL_BASED_SURFACE_MODEL('drill point',(#{open}))"));
    let s = one_solid(&w.file(&[model]));
    assert!(s.sheet);
    assert_eq!(s.brep.faces.len(), 1);
    assert!(s.notes.iter().any(|n| n.contains("1 seam edges added")), "{:?}", s.notes);
    // The seam: the edge to the tip, from the rim where only it passes (between 20° and 350°).
    let seam = s.brep.edges.iter().find(|x| x.vertices.iter().any(|v| len3(sub3(s.brep.vertices[*v as usize].point, [0.0, 0.0, apex])) < 1e-9));
    let seam = seam.unwrap_or_else(|| panic!("no seam to the tip: {:?}", s.brep.vertices));
    let foot = seam.vertices.map(|v| s.brep.vertices[v as usize].point).into_iter().find(|p| p[2].abs() < 1e-9).unwrap();
    let angle = foot[1].atan2(foot[0]).rem_euclid(2.0 * PI);
    assert!(angle > 20.0 * deg && angle < 350.0 * deg, "the seam at {angle} rad starts where the edge turns back");
}

/// A piece of a thread's edge that comes within its pad of a ruling's angle without crossing it
/// (a near miss) may meet the ruling anywhere over its hull's heights, not only at the end nearer
/// in angle (`wound_band_seam` asks `seams::meets` of every piece of every loop): here both ends
/// are below the ruling's vertex and the middle rises above it, so a seam up the ruling is blocked
/// from the vertex on. Read at its nearer end, the piece lies behind the vertex and is no matter
/// (#16930).
#[test]
fn a_near_miss_piece_blocks_a_seam_over_its_whole_height_range() {
    let tol = 1e-3;
    let frame = seams::Perpendicular::about(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
    // An arch on a cylinder of radius 10, 100 to 400 um short of the ruling at angle 0 (the pad
    // reaches tol / r = 100 um): its ends at height -2, its middle's control points at 6.
    let at = |angle: f64, z: f64| [10.0 * angle.cos(), 10.0 * angle.sin(), z];
    let arch = BCurve::Spline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        points: vec![at(-4e-5, -2.0), at(-3e-5, 6.0), at(-2e-5, 6.0), at(-1e-5, -2.0)],
    };
    let form = Some(CurveEval::new(&arch).unwrap());
    let pieces = frame.pieces(&arch, [0.0, 1.0], tol);
    let [piece] = pieces.as_slice() else { panic!("{} pieces", pieces.len()) };
    let height = |p: Vec3| p.z;
    let reach = |along: &dyn Fn(f64) -> f64| match seams::meets(&frame, &form, piece, 0.0, height, along, tol).unwrap() {
        seams::Meets::Hull(reach) => reach,
        seams::Meets::Clear => panic!("the ruling passes within the piece's pad"),
        seams::Meets::Once { d, .. } => panic!("a crossing at {d}: the angle never reaches the ruling's"),
    };
    // From a vertex at height 0 up: the hull's lowest point is behind it, so the piece is on the
    // seam from the vertex.
    assert_eq!(reach(&|h| h), Some(0.0));
    // From a vertex at height -3 up: the whole hull is ahead, its lowest point 1 mm along.
    assert_eq!(reach(&|h| h + 3.0), Some(1.0));
    // From a vertex at height 7: up, the hull is behind it; down, its lowest point is 1 mm along.
    assert_eq!(reach(&|h| h - 7.0), None);
    assert_eq!(reach(&|h| 7.0 - h), Some(1.0));
}

/// A rational quadratic spline on the cylinder of radius `r` about the z axis, from angle `from`
/// at height `z` round the axis counterclockwise, a quarter turn and `rise` up for each of its
/// `pieces`: seen along the axis each piece is an exact quarter circle (weights 1, cos 45°, 1),
/// and its control points' heights are evenly spaced, so the piece is symmetric about its
/// middle and the area under it is that under a straight helix. The curve, and its two ends.
fn rising_quarters(w: &mut W, r: f64, from: f64, z: f64, rise: f64, pieces: usize) -> (u64, [V; 2]) {
    use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_4, SQRT_2};
    let at = |i: usize| {
        let (angle, radius) = (from + i as f64 * FRAC_PI_4, if i.is_multiple_of(2) { r } else { r * SQRT_2 });
        [radius * angle.cos(), radius * angle.sin(), z + 0.5 * rise * i as f64]
    };
    let points: Vec<u64> = (0..=2 * pieces).map(|i| w.pt(at(i))).collect();
    let weights: Vec<String> = (0..points.len()).map(|i| real(if i.is_multiple_of(2) { 1.0 } else { FRAC_1_SQRT_2 })).collect();
    let multiplicities: Vec<&str> = (0..=pieces).map(|i| if i == 0 || i == pieces { "3" } else { "2" }).collect();
    let knots: Vec<String> = (0..=pieces).map(|i| real(i as f64)).collect();
    let curve = w.add(format!(
        "(BOUNDED_CURVE()B_SPLINE_CURVE(2,({}),.UNSPECIFIED.,.F.,.F.)B_SPLINE_CURVE_WITH_KNOTS(({}),({}),.UNSPECIFIED.)CURVE()GEOMETRIC_REPRESENTATION_ITEM()RATIONAL_B_SPLINE_CURVE(({}))REPRESENTATION_ITEM(''))",
        list(&points),
        multiplicities.join(","),
        knots.join(","),
        weights.join(",")
    ));
    (curve, [at(0), at(2 * pieces)])
}

/// A cylinder strip between two rational-quadratic helices, 24 turns of pitch 1.25 mm
/// and 0.5 mm apart. The context's 0.1 mm uncertainty is much coarser than the 0.001 mm
/// pcurve construction tolerance. Counting at the source tolerance can miss the splits
/// needed by the tighter fit; count and construct at the same tolerance instead.
#[test]
fn a_long_wound_rational_edge_with_coarse_accuracy_is_split_for_its_pcurve_fit() {
    use crate::assembly::MAX_SPLINE_POINTS;
    let mut w = W::default();
    let (radius, rise, pieces) = (4.2, 1.25 / 4.0, 96);
    let (low_curve, [low_start, low_end]) = rising_quarters(&mut w, radius, 0.0, 0.0, rise, pieces);
    let (high_curve, [high_start, high_end]) = rising_quarters(&mut w, radius, 0.0, 0.5, rise, pieces);
    let (ls, le, hs, he) = (w.vertex(low_start), w.vertex(low_end), w.vertex(high_start), w.vertex(high_end));
    let low = w.add(format!("EDGE_CURVE('',#{ls},#{le},#{low_curve},.T.)"));
    let high = w.add(format!("EDGE_CURVE('',#{hs},#{he},#{high_curve},.T.)"));
    let start = w.line_edge((ls, low_start), (hs, high_start), true, 1.0);
    let end = w.line_edge((le, low_end), (he, high_end), true, 1.0);
    let axis = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let cylinder = w.add(format!("CYLINDRICAL_SURFACE('',#{axis},{})", real(radius)));
    let uses = [(low, true), (end, true), (high, false), (start, false)].map(|(e, forward)| w.oe(e, forward));
    let bound = w.bound(&uses, true, true);
    let face = w.face(&[bound], cylinder, true);
    let shell = w.add(format!("OPEN_SHELL('',(#{face}))"));
    let model = w.add(format!("SHELL_BASED_SURFACE_MODEL('wound strip',(#{shell}))"));
    // one_solid asserts that import has no refusals and validates the complete record.
    let s = one_solid(&w.file_with_uncertainty(&[model], 0.1));
    assert!(s.sheet);
    assert_eq!(s.brep.tolerance, 0.1);
    assert_eq!(s.brep.faces.len(), 1);
    assert!(s.notes.iter().any(|n| n.contains("edges of faces that wind round their axis split")), "{:?}", s.notes);
    assert!(s.brep.edges.len() > 4, "the long helices need splitting at their construction tolerance");
    assert!(s.brep.edges.iter().any(|e| matches!(e.curve, BCurve::RationalSpline { .. })));
    let mut pcurves = 0;
    for l in &s.brep.faces[0].loops {
        let Loop::Edges { edges } = l else { panic!("edge loop") };
        for u in edges {
            let points = match &u.pcurve {
                Curve2::Spline { points, .. } => points.len(),
                Curve2::RationalSpline { homogeneous, .. } => homogeneous.len(),
            };
            assert!(points <= MAX_SPLINE_POINTS, "edge {}: {points} pcurve control points", u.edge);
            pcurves += 1;
        }
    }
    assert_eq!(pcurves, s.brep.edges.len(), "every split edge keeps its pcurve");
}

/// A thread's crest in a bore: one cylinder face (radius 5) from a circle at z = 0 up to a rim at
/// z = 8, less the thread's groove, 1 high, which starts at a wall along the axis from z = 2 to 3
/// and winds a turn and a half up (4 a turn) to run out through the rim. The face's second loop
/// goes along the rim, down the groove's lower helix, up the wall and up its upper helix. A seam
/// can start from the circle's vertex alone, at 0° (the ruling from the rim's runs into the
/// groove), and its ruling meets the lower helix half a turn up it, near z = 4, where two of the
/// helix's pieces join. With that joint 0.001 rad to either side of 0° (0.005 mm, in a file of
/// 0.01 mm uncertainty) the helix crosses the ruling on one piece, and the piece across the joint
/// comes within the tolerance of the ruling without crossing it. That piece was taken for a
/// second meeting, anywhere over its heights, and the face was left out ("a band winding round
/// its axis whose rulings from its vertices meet another loop first"). It is the helix going on
/// from the seam's end. The groove is 1 high over the upper helix's turn and a quarter, and half
/// that on average over the last quarter turn under the rim, so the face's area is
/// 5 (2π · 8 − 5π/2 − π/4) = 66.25π.
#[test]
fn a_seam_that_ends_beside_a_joint_of_a_helixs_pieces_is_not_taken_to_meet_the_next_piece() {
    let (r, top, uncertainty) = (5.0, 8.0, 0.01);
    for off in [-1e-3, 1e-3] {
        let mut w = W::default();
        let start = [r, 0.0, 0.0];
        let v = w.vertex(start);
        let circle = arc_about_z(&mut w, (v, v), r, 0.0);
        // The helices from the wall at 180° + off: the lower one from z = 2 a turn and a half up
        // to the rim, the upper one from z = 3 a turn and a quarter.
        let (low_curve, [low_foot, low_top]) = rising_quarters(&mut w, r, PI + off, 2.0, 1.0, 6);
        let (high_curve, [high_foot, high_top]) = rising_quarters(&mut w, r, PI + off, 3.0, 1.0, 5);
        let (lf, hf, lt, ht) = (w.vertex(low_foot), w.vertex(high_foot), w.vertex(low_top), w.vertex(high_top));
        let low = w.add(format!("EDGE_CURVE('',#{lf},#{lt},#{low_curve},.T.)"));
        let high = w.add(format!("EDGE_CURVE('',#{hf},#{ht},#{high_curve},.T.)"));
        let wall = w.line_edge((lf, low_foot), (hf, high_foot), true, 1.0);
        // The rim over the crest: the three quarters of a turn from the lower helix's end on.
        let rim = arc_about_z(&mut w, (lt, ht), r, top);
        let a = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let cylinder = w.add(format!("CYLINDRICAL_SURFACE('',#{a},{})", real(r)));
        let below = w.oe(circle, true);
        let above = [(rim, false), (low, false), (wall, true), (high, true)].map(|(e, o)| w.oe(e, o));
        let bounds = [w.bound(&[below], true, true), w.bound(&above, false, true)];
        let face = w.face(&bounds, cylinder, true);
        let open = w.add(format!("OPEN_SHELL('',(#{face}))"));
        let model = w.add(format!("SHELL_BASED_SURFACE_MODEL('crest',(#{open}))"));
        let s = one_solid(&w.file_with_uncertainty(&[model], uncertainty));
        assert!(s.sheet);
        assert_eq!(s.brep.faces.len(), 1, "off {off}: {:?}", s.notes);
        assert!(s.notes.iter().any(|n| n.contains("1 seam edges added")), "off {off}: {:?}", s.notes);
        // The seam: up the ruling from the circle's vertex to the lower helix, which is at
        // 4 - off / 2 there (it climbs half a unit a radian where its pieces join).
        let seam = s.brep.edges.iter().find_map(|e| {
            let [p, q] = e.vertices.map(|v| s.brep.vertices[v as usize].point);
            (len3(sub3(p, start)) < 1e-9 && q[2] > 1.0).then_some(q).or_else(|| (len3(sub3(q, start)) < 1e-9 && p[2] > 1.0).then_some(p))
        });
        let end = seam.unwrap_or_else(|| panic!("off {off}: no seam from the circle's vertex: {:?}", s.brep.vertices));
        assert!(len3(sub3(end, [r, 0.0, 4.0 - 0.5 * off])) < 1e-5, "off {off}: the seam ends at {end:?}");
        let (body, _) = solidcore_ops::body::Body::from_record(&s.brep, false).unwrap();
        let mass = body.mass_properties(1e-6).unwrap();
        let area = 66.25 * PI;
        assert!((mass.area - area).abs() < 1e-3 * area, "off {off}: area {} for {area}", mass.area);
    }
}

/// A seam's angle test (`seams::angular_sign`): a ruling's angular speed is 0, and rounding
/// leaves its one coefficient a few 1e-17 either way; that is not one way. An arc's is.
#[test]
fn a_ruling_is_not_certified_one_way_by_its_rounding() {
    let ruling = [[0.1, 0.7], [0.3, 2.1]];
    let raw = ruling[0][0] * (ruling[1][1] - ruling[0][1]) - ruling[0][1] * (ruling[1][0] - ruling[0][0]);
    assert_ne!(raw, 0.0, "the case needs a rounded coefficient");
    assert_eq!(seams::angular_sign(&ruling), None);
    // A quarter circle's rational quadratic, homogeneous: counterclockwise.
    let w = std::f64::consts::FRAC_1_SQRT_2;
    assert_eq!(seams::angular_sign(&[[1.0, 0.0], [w, w], [0.0, 1.0]]), Some(1));
    assert_eq!(seams::angular_sign(&[[0.0, 1.0], [w, w], [1.0, 0.0]]), Some(-1));
}

/// The arcs a seam must keep clear of are widened by their rounding and by the tolerance at their
/// radius: a vertex a rounding past a ruling's angle, or a loop within the tolerance of the seam,
/// is not clear.
#[test]
fn blocked_arcs_keep_a_seam_off_by_the_tolerance() {
    let frame = seams::Perpendicular::about(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
    let tol = 1e-3;
    // A ruling at angle 0, radius 1.
    let ruling = BCurve::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![[1.0, 0.0, 0.0], [1.0, 0.0, 1.0]] };
    let blocked = seams::Blocked { arcs: frame.pieces(&ruling, [0.0, 1.0], tol).iter().map(seams::Piece::blocked).collect() };
    assert!(!blocked.clear(1e-12), "a rounding off the ruling");
    assert!(!blocked.clear(0.5 * tol), "within the tolerance of the ruling");
    assert!(blocked.clear(2.0 * tol));
    // A hole's arc of radius 2 from 0.1 to 0.3 rad round the axis: clear only beyond tol / 2.
    let arc = BCurve::Circle { frame: Frame { origin: [0.0; 3], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] }, radius: 2.0 };
    let blocked = seams::Blocked { arcs: frame.pieces(&arc, [0.1, 0.3], tol).iter().map(seams::Piece::blocked).collect() };
    assert!(!blocked.clear(0.3 + 0.25 * tol));
    assert!(blocked.clear(0.3 + 0.75 * tol / 2.0 + 0.5 * tol));
}

/// A closed rational cubic spline of two pieces, each an exact half circle of radius 1 about the
/// axis (a cylinder's rim as some exporters write it): a piece reaching half a turn has no arc of
/// its own, and its whole edge was taken to reach every angle, so a band between two such rims
/// had no angle left for its seam and the face was left out ("a band whose other loops leave its
/// seam no angle"). Halved, each part has its arc.
#[test]
fn a_piece_half_way_round_the_axis_is_halved_for_its_arcs() {
    let frame = seams::Perpendicular::about(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
    // (1, 0), (1, 2), (-1, 2), (-1, 0) with weights 1, 1/3, 1/3, 1, and the same turned half way.
    let w = 1.0 / 3.0;
    let half = |s: f64| [[s, 0.0, 0.0, 1.0], [s * w, s * 2.0 * w, 0.0, w], [-s * w, s * 2.0 * w, 0.0, w], [-s, 0.0, 0.0, 1.0]];
    let (upper, lower) = (half(1.0), half(-1.0));
    let rim = BCurve::RationalSpline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0],
        homogeneous: upper.iter().chain(&lower[1..]).copied().collect(),
    };
    let pieces = frame.pieces(&rim, [0.0, 2.0], 1e-6);
    assert_eq!(pieces.len(), 4, "each half halved once");
    let mut turn = 0.0;
    for p in &pieces {
        let [a, b] = p.blocked();
        assert!(b - a < PI, "an arc of {} rad", b - a);
        turn += b - a;
    }
    // The four arcs cover the turn between them (each a little wider than its quarter).
    assert!((TAU..TAU + 0.1).contains(&turn), "{turn}");
}

/// A control point that is not a number gives no arc (the whole turn is blocked), never one
/// that leaves it out.
#[test]
fn an_arc_of_points_not_finite_is_none() {
    let frame = seams::Perpendicular::about(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
    let points = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(f64::NAN, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)];
    assert!(frame.span(&points, 0.0).is_none());
    assert!(frame.span(&[Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)], 0.0).is_some());
}

/// A half torus between its rims (a fillet round a bore, `torus_ring` of the step-conformance
/// set): its face winds around the axis between two circles about it, and gets a seam round the
/// tube on the face's side, closing the solid with the annulus.
#[test]
fn a_half_torus_between_its_rims_gets_a_seam_round_the_tube() {
    let mut w = W::default();
    let ring = |w: &mut W, r: f64| {
        let v = w.vertex([r, 0.0, 0.0]);
        let k = w.circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], r);
        w.add(format!("EDGE_CURVE('',#{v},#{v},#{k},.T.)"))
    };
    let (inner, outer) = (ring(&mut w, 15.0), ring(&mut w, 25.0));
    let a = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let torus = w.add(format!("TOROIDAL_SURFACE('',#{a},20.,5.)"));
    let (x, y) = (w.oe(outer, true), w.oe(inner, false));
    let (bx, by) = (w.bound(&[x], true, true), w.bound(&[y], false, true));
    let tube = w.face(&[bx, by], torus, true);
    let plane = w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let (x, y) = (w.oe(outer, false), w.oe(inner, true));
    let (bx, by) = (w.bound(&[x], true, true), w.bound(&[y], false, true));
    let annulus = w.face(&[bx, by], plane, false);
    let shell = closed_shell(&mut w, &[tube, annulus]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('ring',#{shell})"));
    let s = one_solid(&w.file(&[solid]));
    // The two rims and one seam, a tube half circle from the outer rim's vertex to the inner's.
    assert_eq!(counts(&s.brep), [2, 1, 2, 3, 2]);
    assert!(s.notes.iter().any(|n| n.contains("1 seam edges added")), "{:?}", s.notes);
    let seam = s.brep.edges.iter().find(|e| matches!(e.curve, solidcore_record::brep::Curve::Circle { radius, .. } if (radius - 5.0).abs() < 1e-9));
    let seam = seam.expect("a tube circle seam");
    // On the face's side: up the tube, over its top (z > 0), from angle 0 to π.
    let [t0, t1] = solidcore_ops::label::edge_curve_interval(seam).unwrap();
    assert!((t1 - t0 - std::f64::consts::PI).abs() < 1e-9, "{t0} {t1}");
    faces_away(&s.brep, 0, |p| {
        let r = p[0].hypot(p[1]).max(1e-9);
        [20.0 * p[0] / r, 20.0 * p[1] / r, 1.0]
    });
}

/// A band round a spindle torus (`major` under `minor`: its tube crossing its axis, as the round
/// end of a pin), read as its tube circle revolved: between its widest circle (its vertex at
/// angle `at`) and a circle below it (its vertex at 0.3 rad), closed by a disk on each. Read,
/// validated, its faces pointing out, and its one seam (the one edge that is not a circle) on
/// the quarter line at the revolution's profile (angle 0: y = 0, x > 0), its loops split there.
fn spindle_torus_band(major: f64, minor: f64, at: f64) {
    let low = -std::f64::consts::FRAC_PI_3;
    let (r_low, z_low) = (major + minor * low.cos(), minor * low.sin());
    let mut w = W::default();
    let ring = |w: &mut W, r: f64, z: f64, at: f64| {
        let x = [at.cos(), at.sin(), 0.0];
        let v = w.vertex([r * x[0], r * x[1], z]);
        let k = w.circle([0.0, 0.0, z], [0.0, 0.0, 1.0], x, r);
        w.add(format!("EDGE_CURVE('',#{v},#{v},#{k},.T.)"))
    };
    let (equator, lower) = (ring(&mut w, major + minor, 0.0, at), ring(&mut w, r_low, z_low, 0.3));
    let a = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let torus = w.add(format!("TOROIDAL_SURFACE('',#{a},{},{})", real(major), real(minor)));
    // The band lies below the equator (running −u) and above the lower circle (running +u).
    let (x, y) = (w.oe(equator, false), w.oe(lower, true));
    let (bx, by) = (w.bound(&[x], true, true), w.bound(&[y], false, true));
    let band = w.face(&[bx, by], torus, true);
    let top = w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let x = w.oe(equator, true);
    let bx = w.bound(&[x], true, true);
    let top = w.face(&[bx], top, true);
    let bottom = w.plane([0.0, 0.0, z_low], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
    let x = w.oe(lower, false);
    let bx = w.bound(&[x], true, true);
    let bottom = w.face(&[bx], bottom, true);
    let shell = closed_shell(&mut w, &[band, top, bottom]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('pin end',#{shell})"));
    let s = one_solid(&w.file(&[solid]));
    assert_eq!(s.brep.faces.len(), 3);
    assert!(s.notes.iter().any(|n| n.contains("1 seam edges added")), "{:?}", s.notes);
    let seams: Vec<_> = s.brep.edges.iter().filter(|e| !matches!(e.curve, solidcore_record::brep::Curve::Circle { .. })).collect();
    assert_eq!(seams.len(), 1, "{:?}", s.brep.edges.iter().map(|e| &e.curve).collect::<Vec<_>>());
    for p in seams[0].vertices.map(|v| s.brep.vertices[v as usize].point) {
        assert!(p[1].abs() < 1e-9 * (major + minor) && p[0] > 0.0, "{p:?}");
    }
    faces_away(&s.brep, 0, |_| [0.0, 0.0, 0.5 * z_low]);
}

/// The pin end's radii, both circles' vertices off the quarter lines (at 0.5 and 0.3 rad): the
/// seam lies on the quarter line at angle 0, both circles split there. A seam at a vertex left
/// its second use past the end of the form, which starts on a quarter line.
#[test]
fn a_band_round_a_spindle_torus_gets_its_seam_on_a_quarter_line() {
    spindle_torus_band(0.84455, 1.905, 0.5);
}

/// A widest circle of radius 100 with its vertex 5e-9 rad (5e-7 mm, past the file's 1e-7 mm)
/// round from the quarter line: the vertex is off the line, and the circle is split on it that
/// close to its end, the piece left longer than the tolerance.
#[test]
fn a_spindle_torus_band_splits_its_circle_just_short_of_a_vertex_near_the_quarter_line() {
    spindle_torus_band(40.0, 60.0, 5e-9);
}

/// A spike on a disk: the part of a torus whose tube reaches its axis (major radius `major`,
/// minor radius `minor` no smaller, its centre `c`) from the circle a tube radius below `c` up
/// to the nearest point where the tube meets the axis, closed by the disk in the circle. Read,
/// validated, its faces pointing out; returns the solid and that point, the surface's pole.
fn torus_spike(c: V, major: f64, minor: f64) -> (Solid, V) {
    let (z, x) = ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let below = [c[0], c[1], c[2] - minor];
    let pole = [c[0], c[1], c[2] - (minor * minor - major * major).max(0.0).sqrt()];
    let mut w = W::default();
    let v = w.vertex([c[0] + major, c[1], c[2] - minor]);
    let k = w.circle(below, z, x, major);
    let rim = w.add(format!("EDGE_CURVE('',#{v},#{v},#{k},.T.)"));
    let apex = w.vertex(pole);
    let point = w.add(format!("VERTEX_LOOP('',#{apex})"));
    let point = w.add(format!("FACE_BOUND('',#{point},.T.)"));
    let a = w.axis(c, z, x);
    let torus = w.add(format!("TOROIDAL_SURFACE('',#{a},{},{})", real(major), real(minor)));
    // The torus's normal points out of its tube, into the spike: the face is its other side.
    let o = w.oe(rim, true);
    let bound = w.bound(&[o], false, true);
    let side = w.face(&[point, bound], torus, false);
    let plane = w.plane(below, z, x);
    let o = w.oe(rim, false);
    let bound = w.bound(&[o], false, true);
    let disk = w.face(&[bound], plane, false);
    let shell = closed_shell(&mut w, &[side, disk]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('spike',#{shell})"));
    let s = one_solid(&w.file(&[solid]));
    assert_eq!(counts(&s.brep), [2, 1, 2, 2, 2], "{c:?}");
    // The spike is thin at its rim: the material lies under a point of its face and over a
    // point of its disk.
    let floor = c[2] - minor;
    faces_away(&s.brep, 0, |p| if p[2] > floor + 1e-8 * minor { [p[0], p[1], floor] } else { [p[0], p[1], floor + 1e-8 * minor] });
    (s, pole)
}

/// The end of the form of [`torus_spike`]'s face that lies nearer `pole`: its parameter along
/// the tube, whether it is the form's own end (its first or last knot), and how far the form
/// is from `pole` there, at its farthest round the axis.
fn spike_form_end(s: &Solid, pole: V) -> (f64, bool, f64) {
    let face = s.brep.faces.iter().find(|f| f.nurbs.is_some()).unwrap();
    let Some(form @ BSurface::RationalSpline { v_knots, .. }) = &face.nurbs else { panic!("{:?}", face.nurbs) };
    let eval = SurfaceEval::new(form).unwrap();
    let [[u0, u1], [v0, v1]] = face.domain;
    let first = eval.point([u0, v0]).unwrap().dist(v3(pole)) < eval.point([u0, v1]).unwrap().dist(v3(pole));
    let (end, knot) = if first { (v0, v_knots[0]) } else { (v1, v_knots[v_knots.len() - 1]) };
    let off = (0..=8).map(|i| eval.point([u0 + (u1 - u0) * f64::from(i) / 8.0, end]).unwrap().dist(v3(pole))).fold(0.0, f64::max);
    (end, end == knot, off)
}

/// A horn torus has its major radius equal to its minor radius `r`: its tube touches its axis
/// in one point, the torus's centre. A face that reaches that point is the tube's circle
/// revolved about the axis from where the circle touches it, so its form ends on that point,
/// the surface's pole. Whether the circle touches is the torus's two radii being equal. Worked
/// out from the circle's centre, with the torus off the origin, it came out a few units in the
/// last place either side: read as a miss, the face was refused ("a face around an open
/// surface of revolution with one bound"); read as a crossing, its form ended 5e-8 of a quarter
/// short of the pole and the kernel's check refused the body. The solid has the volume under
/// the tube's quarter arc, π r³ (5/3 − π/2).
#[test]
fn a_horn_torus_face_to_its_pole_has_a_form_that_ends_on_the_pole() {
    let r: f64 = 1.3;
    for c in [[0.0; 3], [137.3, 23.7, 17.9], [44.58, -23.18, 17.91], [3.3, 3.3, 3.3], [1e3, 1e3, 1e3], [0.0, 0.0, 1e6]] {
        let (s, pole) = torus_spike(c, r, r);
        assert_eq!(pole, c);
        let (_, on_end, off) = spike_form_end(&s, pole);
        assert!(on_end, "{c:?}: the form ends on the pole");
        assert!(off <= 1e-12 * (1.0 + v3(c).len()), "{c:?}: {off}");
        let (body, _) = solidcore_ops::body::Body::from_record(&s.brep, false).unwrap();
        assert!(body.is_manifold_solid().unwrap(), "{c:?}");
        let report = body.check(solidcore_ops::check::CheckLevel::All).unwrap();
        assert!(report.is_valid(), "{c:?}: {report}");
        assert_eq!(body.check_solid(), Ok(()), "{c:?}");
        let volume = PI * r.powi(3) * (5.0 / 3.0 - 0.5 * PI);
        let mass = body.mass_properties(1e-6).unwrap();
        assert!((mass.volume - volume).abs() < 1e-5 * volume, "{c:?}: {} vs {volume}", mass.volume);
    }
}

/// A spindle torus whose tube crosses its axis by 1e-9 (its minor radius that much over its
/// major radius) is no horn torus, however far from the origin it sits: its face's form ends
/// where the tube crosses the axis, 5.1e-5 below the torus's centre, not on the centre.
#[test]
fn a_spindle_torus_a_hair_from_a_horn_ends_its_form_where_its_tube_crosses_the_axis() {
    let (major, minor): (f64, f64) = (1.3, 1.3 + 1e-9);
    for c in [[0.0; 3], [137.3, 23.7, 17.9], [0.0, 0.0, 1e4]] {
        let (s, pole) = torus_spike(c, major, minor);
        assert!((c[2] - pole[2] - 5.1e-5).abs() < 1e-6, "{pole:?}");
        let (_, on_end, off) = spike_form_end(&s, pole);
        assert!(on_end, "{c:?}: the form ends on the pole");
        assert!(off <= 1e-9 * (1.0 + v3(c).len()), "{c:?}: {off}");
    }
}

/// A quarter elbow: a torus face winding round its tube between two tube circles (at u = 0 and
/// u = π/2), closed by the two disks they bound. The tube face gets a seam round the axis at a
/// vertex's angle round the tube, as a band round the axis gets one round the tube.
#[test]
fn a_quarter_elbow_gets_a_seam_round_the_axis() {
    let mut w = W::default();
    // The tube circle at u = 0 (in the plane y = 0, about (20, 0, 0)) and at u = π/2 (x = 0).
    let ring = |w: &mut W, c: V, n: V, x: V| {
        let v = w.vertex([c[0] + 5.0 * x[0], c[1] + 5.0 * x[1], c[2] + 5.0 * x[2]]);
        let k = w.circle(c, n, x, 5.0);
        w.add(format!("EDGE_CURVE('',#{v},#{v},#{k},.T.)"))
    };
    // Each circle runs with the torus's v about its normal −(the tube's direction there).
    let start = ring(&mut w, [20.0, 0.0, 0.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]);
    let end = ring(&mut w, [0.0, 20.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let a = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let torus = w.add(format!("TOROIDAL_SURFACE('',#{a},20.,5.)"));
    // The face lies to the left of its loops in (u, v): from u = 0 it runs −v, from π/2 +v.
    let (x, y) = (w.oe(start, false), w.oe(end, true));
    let (bx, by) = (w.bound(&[x], true, true), w.bound(&[y], false, true));
    let tube = w.face(&[bx, by], torus, true);
    // The end disks, facing out of the elbow: −y at its start, −x at its end.
    let p0 = w.plane([20.0, 0.0, 0.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]);
    let x = w.oe(start, true);
    let bx = w.bound(&[x], true, true);
    let d0 = w.face(&[bx], p0, true);
    let p1 = w.plane([0.0, 20.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let x = w.oe(end, false);
    let bx = w.bound(&[x], true, true);
    let d1 = w.face(&[bx], p1, true);
    let shell = closed_shell(&mut w, &[tube, d0, d1]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('elbow',#{shell})"));
    let r = read_solids(&w.file(&[solid]));
    assert!(r.failures.is_empty(), "{:?}", r.failures);
    let s = r.solids.into_iter().next().unwrap();
    solidcore_ops::record_check::validate(&s.brep).unwrap();
    solidcore_ops::record_check::agreement(&s.brep).unwrap();
    assert!(s.notes.iter().any(|n| n.contains("1 seam edges added")), "{:?}", s.notes);
    // The seam: a circle about the axis (centred on it), a quarter turn long.
    let seam = s.brep.edges.iter().find(
        |e| matches!(&e.curve, solidcore_record::brep::Curve::Circle { frame, .. } if frame.origin[0].abs() < 1e-9 && frame.origin[1].abs() < 1e-9),
    );
    let [t0, t1] = solidcore_ops::label::edge_curve_interval(seam.expect("a seam about the axis")).unwrap();
    assert!((t1 - t0 - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{t0} {t1}");
}

/// A tube of radius 1.8 swept down the z axis from z = 15, round a corner of radius 2.5 at the
/// origin, and on along `leg` in the xy plane, written as a sweep writes it: a cylinder, the bend
/// on a torus, a cylinder and two end disks, each round face with one seam. The seams line up
/// with the first leg's seam, at x = −1.8, so the torus's seam round its axis lies at the tube
/// angle of that point, a quarter line only when `leg` runs along an axis.
fn swept_tube(leg: [f64; 2]) -> (p21::Exchange, f64, f64) {
    let (r, big) = (1.8, 2.5);
    let length = (leg[0] * leg[0] + leg[1] * leg[1]).sqrt();
    let d = [leg[0] / length, leg[1] / length, 0.0];
    let at = |o: V, k: f64, v: V| [o[0] + k * v[0], o[1] + k * v[1], o[2] + k * v[2]];
    let dot = |a: V, b: V| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: V, b: V| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let down = [0.0, 0.0, -1.0];
    let (top, corner, start, end) = ([0.0, 0.0, 15.0], [0.0, 0.0, big], [big * d[0], big * d[1], 0.0], [leg[0], leg[1], 0.0]);
    // The torus: centred where the bend's arc is, its axis normal to the bend's plane, its
    // longitude zero at the first leg's end.
    let centre = at(corner, big, d);
    let axis = [d[1], -d[0], 0.0];
    // The seam vertices: on the first leg's seam, and the same point turned a quarter round the
    // torus's axis.
    let p0 = [-r, 0.0, big];
    let seam_centre = at(centre, dot(sub3(p0, centre), axis), axis);
    let seam_radius = len3(sub3(p0, seam_centre));
    let x = sub3(p0, seam_centre).map(|c| c / seam_radius);
    let p1 = at(seam_centre, seam_radius, cross(axis, x));
    let out = sub3(p1, start).map(|c| c / r);
    let (pt, pe) = ([-r, 0.0, 15.0], at(p1, length - big, d));
    let mut w = W::default();
    let (vt, v0, v1, ve) = (w.vertex(pt), w.vertex(p0), w.vertex(p1), w.vertex(pe));
    let ring = |w: &mut W, v: u64, c: V, n: V, x: V| {
        let k = w.circle(c, n, x, r);
        w.add(format!("EDGE_CURVE('',#{v},#{v},#{k},.T.)"))
    };
    let top_ring = ring(&mut w, vt, top, down, [-1.0, 0.0, 0.0]);
    let corner_ring = ring(&mut w, v0, corner, down, [-1.0, 0.0, 0.0]);
    let start_ring = ring(&mut w, v1, start, neg(d), down);
    let end_ring = ring(&mut w, ve, end, d, out);
    let seam_top = w.line_edge((vt, pt), (v0, p0), true, 1.0);
    let k = w.circle(seam_centre, axis, x, seam_radius);
    let seam_bend = w.add(format!("EDGE_CURVE('',#{v0},#{v1},#{k},.T.)"));
    let seam_end = w.line_edge((v1, p1), (ve, pe), true, 1.0);
    let mut faces = Vec::new();
    let mut side = |w: &mut W, surface: u64, uses: [(u64, bool); 4]| {
        let uses: Vec<u64> = uses.into_iter().map(|(e, o)| w.oe(e, o)).collect();
        let bound = w.bound(&uses, false, true);
        faces.push(w.face(&[bound], surface, true));
    };
    let a = w.axis(top, down, [-1.0, 0.0, 0.0]);
    let first = w.add(format!("CYLINDRICAL_SURFACE('',#{a},{})", real(r)));
    side(&mut w, first, [(seam_top, true), (corner_ring, false), (seam_top, false), (top_ring, true)]);
    let a = w.axis(centre, axis, neg(d));
    let torus = w.add(format!("TOROIDAL_SURFACE('',#{a},{},{})", real(big), real(r)));
    side(&mut w, torus, [(seam_bend, true), (start_ring, true), (seam_bend, false), (corner_ring, true)]);
    let a = w.axis(start, d, out);
    let second = w.add(format!("CYLINDRICAL_SURFACE('',#{a},{})", real(r)));
    side(&mut w, second, [(seam_end, true), (end_ring, false), (seam_end, false), (start_ring, false)]);
    for (o, n, x, ring, sense) in [(top, down, [-1.0, 0.0, 0.0], top_ring, false), (end, d, axis, end_ring, true)] {
        let plane = w.plane(o, n, x);
        let x = w.oe(ring, true);
        let bound = w.bound(&[x], false, sense);
        faces.push(w.face(&[bound], plane, sense));
    }
    let shell = closed_shell(&mut w, &faces);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('tube',#{shell})"));
    // Straight runs of 15 − 2.5 and |leg| − 2.5, and a quarter of a torus.
    let run = 15.0 - big + length - big;
    let pi = std::f64::consts::PI;
    let volume = pi * r * r * (run + big * pi / 2.0);
    let area = 2.0 * pi * r * run + pi * pi * big * r + 2.0 * pi * r * r;
    (w.file(&[solid]), volume, area)
}

/// A swept tube's bend reads with every edge on its torus whatever the direction of its second
/// leg: along −y its torus's seam round the axis lies on a quarter line of the tube; along the
/// legs off the axes (the seam in each quarter of the tube) and 1e-6 off −y it does not; 1e-8 off
/// −y it does, to within the file's uncertainty. The bend stays a torus, and the quarter grid of
/// its tube runs through the seam (#18778).
#[test]
fn a_tube_swept_round_a_corner_reads_closed_whatever_its_second_legs_direction() {
    let legs = [[0.0, -11.24], [-3.75, -10.6], [3.75, -10.6], [-3.75, 10.6], [3.75, 10.6], [1e-8, -11.24], [1e-6, -11.24]];
    for leg in legs {
        let (file, volume, area) = swept_tube(leg);
        let r = read_solids(&file);
        assert!(r.failures.is_empty(), "{leg:?}: {:?}", r.failures);
        let s = r.solids.into_iter().next().unwrap();
        assert!(!s.sheet, "{leg:?}: {:?}", s.notes);
        solidcore_ops::record_check::validate(&s.brep).unwrap();
        solidcore_ops::record_check::agreement(&s.brep).unwrap();
        // The file's 5 faces, 7 edges (3 seams, 4 rings) and 4 vertices, inside and outside.
        assert_eq!(counts(&s.brep), [2, 1, 5, 7, 4], "{leg:?}: {:?}", s.notes);
        // One torus, the tube angle of the seam vertex (−1.8, 0, 2.5) on it a whole number of
        // quarters from its tube start, to within the angle the file's uncertainty (1e-7 mm)
        // makes round the tube (the start a face within it of a quarter line keeps).
        let tori: Vec<_> = s
            .brep
            .faces
            .iter()
            .filter_map(|f| match &f.surface {
                BSurface::Torus { frame, major_radius, tube_start, .. } => Some((frame.clone(), *major_radius, *tube_start)),
                _ => None,
            })
            .collect();
        let [(frame, major, tube_start)] = tori.as_slice() else { panic!("{leg:?}: {} tori", tori.len()) };
        let (x, y) = (Vec3::from(frame.x_axis), Vec3::from(frame.y_axis));
        let z = x.cross(y);
        let d = Vec3::from([-1.8, 0.0, 2.5]) - Vec3::from(frame.origin);
        let h = d.dot(z);
        let angle = h.atan2((d - z * h).len() - major);
        let off = angle - tube_start;
        let off = off - (off / std::f64::consts::FRAC_PI_2).round() * std::f64::consts::FRAC_PI_2;
        assert!(off.abs() <= 1e-7 / 1.8, "{leg:?}: seam at {angle}, tube start {tube_start}");
        let (body, _) = solidcore_ops::body::Body::from_record(&s.brep, false).unwrap();
        assert!(body.is_manifold_solid().unwrap());
        let mass = body.mass_properties(1e-9).unwrap();
        assert!((mass.volume - volume).abs() < 1e-6 * volume, "{leg:?}: {mass:?}, {volume} expected");
        assert!((mass.area - area).abs() < 1e-6 * area, "{leg:?}: {mass:?}, {area} expected");
    }
}

/// A ball whose one face is bounded by two vertex loops, at its poles: the whole sphere, closed
/// by a meridian seam from pole to pole.
#[test]
fn a_ball_bounded_by_vertex_loops_at_its_poles() {
    let mut w = W::default();
    let a = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let sphere = w.add(format!("SPHERICAL_SURFACE('',#{a},5.)"));
    let (n, s_) = (w.vertex([0.0, 0.0, 5.0]), w.vertex([0.0, 0.0, -5.0]));
    let (ln, ls) = (w.add(format!("VERTEX_LOOP('',#{n})")), w.add(format!("VERTEX_LOOP('',#{s_})")));
    let (bn, bs) = (w.add(format!("FACE_OUTER_BOUND('',#{ln},.T.)")), w.add(format!("FACE_BOUND('',#{ls},.T.)")));
    let face = w.face(&[bn, bs], sphere, true);
    let shell = closed_shell(&mut w, &[face]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('ball',#{shell})"));
    let s = one_solid(&w.file(&[solid]));
    assert_eq!(counts(&s.brep), [2, 1, 1, 1, 2]);
    faces_away(&s.brep, 0, |_| [0.0; 3]);
}

/// A disk bounded by a closed quadratic B-spline written unclamped (uniform knots from −2 to 6,
/// its first two points repeated at the end, as cadmpeg's N04 recipes and periodic writers do):
/// the curve is clamped to its domain [0, 4] exactly, so its pcurve on the plane is the closed
/// form and the face reads.
#[test]
fn a_face_bounded_by_an_unclamped_periodic_spline_reads() {
    let mut w = W::default();
    let corners = [[-10.0, -10.0, 0.0], [10.0, -10.0, 0.0], [10.0, 10.0, 0.0], [-10.0, 10.0, 0.0]];
    let p: Vec<u64> = corners.iter().map(|c| w.pt(*c)).collect();
    let curve = w.add(format!(
        "B_SPLINE_CURVE_WITH_KNOTS('',2,(#{},#{},#{},#{},#{},#{}),.UNSPECIFIED.,.T.,.U.,(1,1,1,1,1,1,1,1,1),(-2.,-1.,0.,1.,2.,3.,4.,5.,6.),.UNSPECIFIED.)",
        p[0], p[1], p[2], p[3], p[0], p[1]
    ));
    // The curve at its domain's start: the middle of its first two corners.
    let v = w.vertex([0.0, -10.0, 0.0]);
    let e = w.add(format!("EDGE_CURVE('',#{v},#{v},#{curve},.T.)"));
    let oe = w.oe(e, true);
    let bound = w.bound(&[oe], true, true);
    let plane = w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let face = w.face(&[bound], plane, true);
    let shell = w.add(format!("OPEN_SHELL('',(#{face}))"));
    let model = w.add(format!("SHELL_BASED_SURFACE_MODEL('disk',(#{shell}))"));
    let s = one_solid(&w.file(&[model]));
    let edge = s.brep.edges.first().unwrap();
    let solidcore_record::brep::Curve::Spline { knots, .. } = edge.parameters() else { panic!("{:?}", edge.curve) };
    assert_eq!((knots[..3].to_vec(), knots[knots.len() - 3..].to_vec()), (vec![0.0; 3], vec![4.0; 3]));
}

/// A FACETED_BREP: a tetrahedron of four FACE_SURFACEs on planes, each bounded by a POLY_LOOP of
/// three of its four points. Each point is one vertex and each side one edge, shared by the two
/// faces it bounds, so the shell closes.
#[test]
fn a_faceted_brep_of_poly_loops_is_a_closed_solid() {
    let mut w = W::default();
    let p = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]].map(|q| w.pt(q));
    // Each face's points counter-clockwise seen from outside.
    let tri = |w: &mut W, i: [usize; 3], o: V, z: V, x: V| {
        let l = w.add(format!("POLY_LOOP('',(#{},#{},#{}))", p[i[0]], p[i[1]], p[i[2]]));
        let b = w.add(format!("FACE_OUTER_BOUND('',#{l},.T.)"));
        let s = w.plane(o, z, x);
        w.add(format!("FACE_SURFACE('',(#{b}),#{s},.T.)"))
    };
    let k = 1.0 / 3f64.sqrt();
    let faces = [
        tri(&mut w, [0, 2, 1], [0.0; 3], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]),
        tri(&mut w, [0, 1, 3], [0.0; 3], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]),
        tri(&mut w, [0, 3, 2], [0.0; 3], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        tri(&mut w, [1, 2, 3], [10.0, 0.0, 0.0], [k, k, k], [-k * 3f64.sqrt() / 2f64.sqrt(), k * 3f64.sqrt() / 2f64.sqrt(), 0.0]),
    ];
    let shell = closed_shell(&mut w, &faces);
    let solid = w.add(format!("FACETED_BREP('tetra',#{shell})"));
    let s = one_solid(&w.file(&[solid]));
    assert_eq!(counts(&s.brep), [2, 1, 4, 6, 4]);
    faces_away(&s.brep, 0, |_| [2.0, 2.0, 2.0]);
}

/// A 20 × 20 × 5 plate with a hole of radius 3 through it; with `marked`, its square bounds are
/// FACE_OUTER_BOUNDs, otherwise every bound is a FACE_BOUND and the hole is listed first.
fn plate(marked: bool) -> p21::Exchange {
    let mut w = W::default();
    let c = cylinder(&mut w, [10.0, 10.0], 3.0, [0.0, 5.0]);
    let mut inner = |w: &mut W, up: f64| {
        // About the face's outward normal the hole runs clockwise.
        let oe = if up > 0.0 { w.oe(c.circles[1], false) } else { w.oe(c.circles[0], true) };
        vec![w.bound(&[oe], false, true)]
    };
    let mut faces = box_faces(&mut w, [0.0; 3], [20.0, 20.0, 5.0], BoxStyle::default(), &mut inner);
    if !marked {
        let text = w.s.replace("FACE_OUTER_BOUND", "FACE_BOUND");
        w.s = text;
        // The hole first.
        let mut lines: Vec<String> = w.s.lines().map(str::to_string).collect();
        for l in &mut lines {
            if l.contains("ADVANCED_FACE") && l.matches('#').count() == 4 {
                let (head, rest) = l.split_once("',(").unwrap();
                let (bounds, tail) = rest.split_once(')').unwrap();
                let parts: Vec<&str> = bounds.split(',').rev().collect();
                *l = format!("{head}',({}){tail}", parts.join(","));
            }
        }
        w.s = lines.join("\n") + "\n";
    }
    faces.push(side(&mut w, &c, false));
    let shell = closed_shell(&mut w, &faces);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('plate',#{shell})"));
    w.file(&[solid])
}

#[test]
fn a_plate_with_a_through_hole() {
    for marked in [true, false] {
        let s = one_solid(&plate(marked));
        assert_eq!(counts(&s.brep), [2, 1, 7, 15, 10], "marked {marked}");
        let holed: Vec<&Face> = s.brep.faces.iter().filter(|f| f.loops.len() == 2).collect();
        assert_eq!(holed.len(), 2);
        for f in holed {
            let Loop::Edges { edges } = &f.loops[0] else { panic!() };
            assert_eq!(edges.len(), 4, "the square is the outer loop");
        }
        faces_away(&s.brep, 0, |p| {
            let r = ((p[0] - 10.0).powi(2) + (p[1] - 10.0).powi(2)).sqrt();
            if (r - 3.0).abs() < 1e-6 {
                // On the hole: material lies away from the axis.
                [10.0 + (p[0] - 10.0) * 2.0, 10.0 + (p[1] - 10.0) * 2.0, p[2]]
            } else {
                [p[0].clamp(0.5, 19.5), p[1].clamp(0.5, 19.5), 2.5]
            }
        });
    }
}

/// A plate 10 mm deep and 5 mm thick whose side at x = 10 leans out to x = 15 at its bottom,
/// with a through hole of radius 2 about (8, 5): the hole's top rim touches the top face's edge
/// x = 10 at the one point (10, 5, 5), where the rim's vertex is a vertex of that edge too, and
/// the leaning side runs clear of the hole below it. The top face is written with the hole as a
/// bound of its own, as a boolean writes it.
fn plate_with_a_hole_touching_an_edge() -> p21::Exchange {
    let mut w = W::default();
    let at = |w: &mut W, p: V| (w.vertex(p), p);
    let [a0, b0, c0, d0] = [[0.0, 0.0, 0.0], [15.0, 0.0, 0.0], [15.0, 10.0, 0.0], [0.0, 10.0, 0.0]].map(|p| at(&mut w, p));
    let [a, b, c, d] = [[0.0, 0.0, 5.0], [10.0, 0.0, 5.0], [10.0, 10.0, 5.0], [0.0, 10.0, 5.0]].map(|p| at(&mut w, p));
    // Where the rim touches the edge, and the bottom rim's vertex under it.
    let (touch, under) = (at(&mut w, [10.0, 5.0, 5.0]), at(&mut w, [10.0, 5.0, 0.0]));
    let line = |w: &mut W, p: (u64, V), q: (u64, V)| w.line_edge(p, q, true, 1.0);
    let bottom = [line(&mut w, a0, b0), line(&mut w, b0, c0), line(&mut w, c0, d0), line(&mut w, d0, a0)];
    let top = [line(&mut w, a, b), line(&mut w, b, touch), line(&mut w, touch, c), line(&mut w, c, d), line(&mut w, d, a)];
    let up = [line(&mut w, a0, a), line(&mut w, b0, b), line(&mut w, c0, c), line(&mut w, d0, d)];
    let rim = |w: &mut W, v: u64, h: f64| {
        let k = w.circle([8.0, 5.0, h], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 2.0);
        w.add(format!("EDGE_CURVE('',#{v},#{v},#{k},.T.)"))
    };
    let rims = [rim(&mut w, under.0, 0.0), rim(&mut w, touch.0, 5.0)];
    let seam = line(&mut w, under, touch);
    let axis = w.axis([8.0, 5.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let surface = w.add(format!("CYLINDRICAL_SURFACE('',#{axis},2.)"));
    let wall = side(&mut w, &Cyl { circles: rims, seam, surface }, false);
    // Each face's outer bound counter-clockwise about its outward normal, then its hole.
    let face = |w: &mut W, uses: &[(u64, bool)], hole: Option<(u64, bool)>, o: V, z: V, x: V| {
        let oes: Vec<u64> = uses.iter().map(|&(e, forward)| w.oe(e, forward)).collect();
        let mut bounds = vec![w.bound(&oes, false, true)];
        if let Some((e, forward)) = hole {
            let oe = w.oe(e, forward);
            bounds.push(w.bound(&[oe], false, true));
        }
        let plane = w.plane(o, z, x);
        w.face(&bounds, plane, true)
    };
    let lean = 0.5_f64.sqrt();
    let faces = [
        face(
            &mut w,
            &[(top[0], true), (top[1], true), (top[2], true), (top[3], true), (top[4], true)],
            Some((rims[1], false)),
            a.1,
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
        ),
        face(
            &mut w,
            &[(bottom[3], false), (bottom[2], false), (bottom[1], false), (bottom[0], false)],
            Some((rims[0], true)),
            a0.1,
            [0.0, 0.0, -1.0],
            [1.0, 0.0, 0.0],
        ),
        face(&mut w, &[(bottom[0], true), (up[1], true), (top[0], false), (up[0], false)], None, a0.1, [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]),
        face(&mut w, &[(up[3], true), (top[3], false), (up[2], false), (bottom[2], true)], None, d0.1, [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
        face(&mut w, &[(up[0], true), (top[4], false), (up[3], false), (bottom[3], true)], None, a0.1, [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        face(
            &mut w,
            &[(bottom[1], true), (up[2], true), (top[2], false), (top[1], false), (up[1], false)],
            None,
            b0.1,
            [lean, 0.0, lean],
            [0.0, 1.0, 0.0],
        ),
        wall,
    ];
    let shell = closed_shell(&mut w, &faces);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('plate',#{shell})"));
    w.file(&[solid])
}

/// A hole whose rim touches its face's outer edge at one vertex (tangency-labs/solidcraft#19165:
/// a part from OpenCascade's booleans) is no bound of its own: the kernel takes no face whose
/// loops share a vertex (its face tests 32, 38 and 39 refused the body, and every feature on it
/// after). The face's one loop runs along the outer edge to the vertex, round the rim and on
/// along the edge, and bounds the same region: the solid has the faces, edges, vertices and
/// volume it is written with, and the kernel's check passes it.
#[test]
fn a_hole_that_touches_its_faces_outer_edge_at_one_vertex_is_in_the_outer_loop() {
    let s = one_solid(&plate_with_a_hole_touching_an_edge());
    assert!(!s.sheet);
    assert_eq!(counts(&s.brep), [2, 1, 7, 16, 10]);
    let b = &s.brep;
    let (body, _) = solidcore_ops::body::Body::from_record(b, false).unwrap();
    assert!(body.is_manifold_solid().unwrap());
    let report = body.check(solidcore_ops::check::CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{report}");
    assert_eq!(body.counts().unwrap(), [7, 16, 10]);
    // The leaning prism less the hole: 10 x (10 + 15) / 2 x 5 - pi 2^2 x 5.
    let volume = 625.0 - 20.0 * PI;
    let mass = body.mass_properties(1e-9).unwrap();
    assert!((mass.volume - volume).abs() < 1e-8 * volume, "{} vs {volume}", mass.volume);
    let touch = b.vertices.iter().position(|v| v.point == [10.0, 5.0, 5.0]).unwrap() as u32;
    // The top face: the five edges of its outline and the rim between the two that meet at the
    // vertex, the rim run clockwise about the face's normal as a hole is.
    let rim = b.edges.iter().position(|e| matches!(e.curve, BCurve::Circle { .. }) && e.vertices == [touch; 2]).unwrap() as u32;
    let tops: Vec<&Face> =
        b.faces.iter().filter(|f| matches!(f.surface, BSurface::Plane { .. }) && f.loops.iter().any(|l| uses_edge(l, rim))).collect();
    let [top] = tops.as_slice() else { panic!("{} plane faces on the rim", tops.len()) };
    let [Loop::Edges { edges }] = top.loops.as_slice() else { panic!("{} loops", top.loops.len()) };
    assert_eq!(edges.len(), 6);
    let starts: Vec<u32> = edges.iter().map(|u| b.edges[u.edge as usize].vertices[usize::from(!u.forward)]).collect();
    assert_eq!(starts.iter().filter(|v| **v == touch).count(), 2, "through the vertex twice: {starts:?}");
    let at = edges.iter().position(|u| u.edge == rim).unwrap();
    let (before, after) = (&edges[(at + 5) % 6], &edges[(at + 1) % 6]);
    for u in [before, after] {
        assert!(matches!(b.edges[u.edge as usize].curve, BCurve::Line { .. }));
    }
    assert_eq!(starts[at], touch);
    assert_eq!(starts[(at + 1) % 6], touch);
    // The bottom face keeps its hole as a loop of its own: nothing touches there.
    assert_eq!(b.faces.iter().filter(|f| f.loops.len() == 2).count(), 1);
    faces_away(b, 0, |p| {
        let r = (p[0] - 8.0).hypot(p[1] - 5.0);
        if (r - 2.0).abs() < 1e-6 {
            [8.0 + (p[0] - 8.0) * 2.0, 5.0 + (p[1] - 5.0) * 2.0, p[2]]
        } else {
            [p[0].clamp(0.5, 5.0), p[1].clamp(0.5, 9.5), 2.5]
        }
    });
    assert!(s.notes.iter().any(|n| n.contains("made one loop")), "{:?}", s.notes);
}

/// Whether loop `l` runs along edge `e`.
fn uses_edge(l: &Loop, e: u32) -> bool {
    matches!(l, Loop::Edges { edges } if edges.iter().any(|u| u.edge == e))
}

/// A hole that touches its face's outer loop at two vertices cuts the face in two, which is no
/// one loop: a square sheet whose hole reaches from the middle of one side to the middle of the
/// opposite one keeps its two loops as written, and the reader's notes name the face and where
/// its loops touch.
#[test]
fn a_hole_that_touches_its_faces_outer_loop_at_two_vertices_is_left_and_named() {
    let mut w = W::default();
    let at = |w: &mut W, p: V| (w.vertex(p), p);
    let ring = |w: &mut W, corners: &[(u64, V)]| -> Vec<u64> {
        let n = corners.len();
        (0..n)
            .map(|k| {
                let e = w.line_edge(corners[k], corners[(k + 1) % n], true, 1.0);
                w.oe(e, true)
            })
            .collect()
    };
    let (right, left) = (at(&mut w, [10.0, 5.0, 0.0]), at(&mut w, [0.0, 5.0, 0.0]));
    let square =
        [at(&mut w, [0.0, 0.0, 0.0]), at(&mut w, [10.0, 0.0, 0.0]), right, at(&mut w, [10.0, 10.0, 0.0]), at(&mut w, [0.0, 10.0, 0.0]), left];
    // Clockwise about +z, as a hole runs.
    let diamond = [right, at(&mut w, [5.0, 3.0, 0.0]), left, at(&mut w, [5.0, 7.0, 0.0])];
    let (outer, hole) = (ring(&mut w, &square), ring(&mut w, &diamond));
    let bounds = [w.bound(&outer, true, true), w.bound(&hole, false, true)];
    let plane = w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let face = w.face(&bounds, plane, true);
    let open = w.add(format!("OPEN_SHELL('',(#{face}))"));
    let model = w.add(format!("SHELL_BASED_SURFACE_MODEL('sheet',(#{open}))"));
    let s = one_solid(&w.file(&[model]));
    assert!(s.sheet);
    assert_eq!(counts(&s.brep), [1, 1, 1, 10, 8]);
    assert_eq!(s.brep.faces[0].loops.len(), 2);
    let named = format!("face #{face}: its loops touch at (10, 5, 0) mm and are left as written: ");
    assert!(s.notes.iter().any(|n| n.starts_with(&named)), "{:?}", s.notes);
    assert!(!s.notes.iter().any(|n| n.contains("made one loop")), "{:?}", s.notes);
}

/// [`cylinder`] with each rim circle from its vertex to a second VERTEX_POINT at the same point,
/// as OpenCascade's booleans can write a hole's rim (tangency-labs/solidcraft#18784).
fn cylinder_with_twin_rim_ends(w: &mut W, c: [f64; 2], r: f64, z: [f64; 2]) -> Cyl {
    let p = |h: f64| [c[0] + r, c[1], h];
    let (v0, v1) = (w.vertex(p(z[0])), w.vertex(p(z[1])));
    let circle = |w: &mut W, v: u64, h: f64| {
        let k = w.circle([c[0], c[1], h], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], r);
        let end = w.vertex(p(h));
        w.add(format!("EDGE_CURVE('',#{v},#{end},#{k},.T.)"))
    };
    let circles = [circle(w, v0, z[0]), circle(w, v1, z[1])];
    let seam = w.line_edge((v0, p(z[0])), (v1, p(z[1])), true, 1.0);
    let a = w.axis([c[0], c[1], 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let surface = w.add(format!("CYLINDRICAL_SURFACE('',#{a},{})", real(r)));
    Cyl { circles, seam, surface }
}

/// A closed cylinder: its two caps (each bounded by one rim) and its side (rims and seam).
fn closed_cylinder(twin_rim_ends: bool) -> p21::Exchange {
    let mut w = W::default();
    let c = if twin_rim_ends {
        cylinder_with_twin_rim_ends(&mut w, [1.0, 2.0], 5.0, [0.0, 10.0])
    } else {
        cylinder(&mut w, [1.0, 2.0], 5.0, [0.0, 10.0])
    };
    let side_face = side(&mut w, &c, true);
    let ob = w.oe(c.circles[0], false);
    let bottom_bound = w.bound(&[ob], true, true);
    let bottom_plane = w.plane([1.0, 2.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let bottom = w.face(&[bottom_bound], bottom_plane, false);
    let ot = w.oe(c.circles[1], true);
    let top_bound = w.bound(&[ot], true, true);
    let top_plane = w.plane([1.0, 2.0, 10.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let top = w.face(&[top_bound], top_plane, true);
    let shell = closed_shell(&mut w, &[bottom, side_face, top]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('',#{shell})"));
    w.file(&[solid])
}

/// A cylinder whose rims are full circles each from one VERTEX_POINT to a second one at the same
/// point (tangency-labs/solidcraft#18784): each rim is a full turn whose two ends are one vertex,
/// so its caps' one-edge loops and its side's loop close, and it reads as the same closed solid
/// as the cylinder whose rims have one vertex each.
#[test]
fn a_full_circle_edge_between_two_vertices_at_one_point_closes_its_loop() {
    let (one, two) = (one_solid(&closed_cylinder(false)), one_solid(&closed_cylinder(true)));
    assert!(!two.sheet);
    assert_eq!(counts(&two.brep), [2, 1, 3, 3, 2]);
    assert_eq!(counts(&one.brep), counts(&two.brep));
    faces_away(&two.brep, 0, |p| [1.0, 2.0, p[2].clamp(1.0, 9.0)]);
    for e in two.brep.edges.iter().filter(|e| matches!(e.curve, BCurve::Circle { .. })) {
        let [a0, a1] = solidcore_ops::label::edge_curve_interval(e).unwrap();
        assert!((a1 - a0 - TAU).abs() < 1e-12, "a full turn: {a0} {a1}");
        assert_eq!(e.vertices[0], e.vertices[1]);
    }
}

/// A unit square sheet bounded by one closed edge on a closed polygon spline round it (its seam
/// at (1, 0.5)), from the vertex there to itself, or with `twin_ends` to a second VERTEX_POINT at
/// the same point.
fn spline_outline(twin_ends: bool) -> p21::Exchange {
    let mut w = W::default();
    let start = [1.0, 0.5, 0.0];
    let v = w.vertex(start);
    let end = if twin_ends { w.vertex(start) } else { v };
    let pts: Vec<u64> = [start, [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], start].iter().map(|p| w.pt(*p)).collect();
    let c =
        w.add(format!("B_SPLINE_CURVE_WITH_KNOTS('',1,({}),.POLYLINE_FORM.,.T.,.F.,(2,1,1,1,1,2),(0.,1.,2.,3.,4.,5.),.UNSPECIFIED.)", list(&pts)));
    let e = w.add(format!("EDGE_CURVE('',#{v},#{end},#{c},.T.)"));
    let oe = w.oe(e, true);
    let bound = w.bound(&[oe], true, true);
    let plane = w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let face = w.face(&[bound], plane, true);
    let shell = w.add(format!("OPEN_SHELL('',(#{face}))"));
    let model = w.add(format!("SHELL_BASED_SURFACE_MODEL('',(#{shell}))"));
    w.file(&[model])
}

/// A closed spline edge written with two vertices at one point is read as the same edge written
/// with one: the whole spline, not an empty piece of it between one point and itself.
#[test]
fn a_closed_spline_edge_between_two_vertices_at_one_point_is_its_one_vertex_edge() {
    let (one, two) = (one_solid(&spline_outline(false)), one_solid(&spline_outline(true)));
    assert_eq!(counts(&two.brep), [1, 1, 1, 1, 1]);
    assert_eq!(counts(&one.brep), counts(&two.brep));
    let (a, b) = (&one.brep.edges[0], &two.brep.edges[0]);
    assert_eq!(a.curve, b.curve);
    assert_eq!(a.interval, b.interval);
    assert_eq!(b.vertices[0], b.vertices[1]);
}

/// A LINE edge between two vertices at one point is refused as the LINE edge with one vertex is:
/// joining its ends never makes an edge of no length.
#[test]
fn a_line_edge_between_two_vertices_at_one_point_is_refused_as_a_closed_edge() {
    for twin_ends in [false, true] {
        let mut w = W::default();
        let p = [1.0, 2.0, 3.0];
        let v = w.vertex(p);
        let end = if twin_ends { w.vertex(p) } else { v };
        let (o, d) = (w.pt(p), w.dir([1.0, 0.0, 0.0]));
        let vec = w.add(format!("VECTOR('',#{d},1.)"));
        let line = w.add(format!("LINE('',#{o},#{vec})"));
        let e = w.add(format!("EDGE_CURVE('',#{v},#{end},#{line},.T.)"));
        let ex = w.file(&[e]);
        with_builder(&ex, |b| {
            let err = b.edge(e).err().unwrap_or_default();
            assert!(err.contains("a closed edge on a line"), "twin ends {twin_ends}: {err}");
        });
    }
}

/// Two vertices at one point that are the two ends of a circle edge not read yet meet: a loop
/// read before that edge joins them, as the edge itself would.
#[test]
fn the_two_ends_at_one_point_of_an_edge_read_later_meet() {
    let mut w = W::default();
    let p = [3.0, 0.0, 0.0];
    let (v0, v1) = (w.vertex(p), w.vertex(p));
    let k = w.circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 3.0);
    let circle = w.add(format!("EDGE_CURVE('',#{v0},#{v1},#{k},.T.)"));
    let ex = w.file(&[circle]);
    with_builder(&ex, |b| {
        let (a, z) = (b.vertex(v0).unwrap(), b.vertex(v1).unwrap());
        assert!(b.meet(a, z).unwrap());
        let (e, _) = b.edge(circle).unwrap();
        let [s, t] = b.asm.edges[e as usize].vertices;
        assert_eq!(s, t);
    });
}

/// A circle edge from A to a second vertex C at A's point, read after an earlier loop joined A
/// with B, where B and C are the two ends of a LINE 1.5e-7 mm long (within their 1e-7 mm
/// tolerances): the circle's two ends meet as any two vertices do, so they are not joined across
/// the LINE, and the LINE keeps its two vertices.
#[test]
fn a_two_vertex_circle_never_joins_the_two_ends_of_a_line_read_after_it() {
    let mut w = W::default();
    let (a, c, bp) = ([0.0; 3], [0.0; 3], [1.5e-7, 0.0, 0.0]);
    let (va, vb, vc) = (w.vertex(a), w.vertex(bp), w.vertex(c));
    let k = w.circle([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 1.0);
    let circle = w.add(format!("EDGE_CURVE('',#{va},#{vc},#{k},.T.)"));
    let line = w.line_edge((vb, bp), (vc, c), true, 1.0);
    let ex = w.file(&[circle, line]);
    with_builder(&ex, |b| {
        let (ia, ib) = (b.vertex(va).unwrap(), b.vertex(vb).unwrap());
        // An earlier loop meets A and B: they are apart by less than their tolerances.
        assert!(b.meet(ib, ia).unwrap());
        let _ = b.edge(circle);
        let (e, _) = b.edge(line).unwrap();
        b.join_vertices().unwrap();
        let [s, t] = b.asm.edges[e as usize].vertices;
        assert_ne!(s, t, "the LINE keeps its two vertices");
        let ic = b.vertex(vc).unwrap();
        assert_ne!(b.find(ib), b.find(ic));
    });
}

#[test]
fn a_closed_shell_beside_open_shells_reads_each_open_shell_as_a_sheet() {
    // A closed shell beside two open ones: the closed shell is a solid body and each open shell a
    // surface body of its own.
    let mut w = W::default();
    let faces = box_faces(&mut w, [0.0; 3], [1.0; 3], BoxStyle::default(), &mut |_, _| Vec::new());
    let closed = closed_shell(&mut w, &faces);
    let mut faces = box_faces(&mut w, [2.0; 3], [3.0; 3], BoxStyle::default(), &mut |_, _| Vec::new());
    faces.pop();
    let open = w.add(format!("OPEN_SHELL('',({}))", list(&faces)));
    let single = box_faces(&mut w, [5.0; 3], [6.0; 3], BoxStyle::default(), &mut |_, _| Vec::new());
    let one = w.add(format!("OPEN_SHELL('',(#{}))", single[0]));
    let model = w.add(format!("SHELL_BASED_SURFACE_MODEL('sheet',(#{closed},#{open},#{one}))"));
    let r = read_solids(&w.file(&[model]));
    assert!(r.failures.is_empty(), "{:?}", r.failures);
    assert_eq!(r.solids.len(), 3);
    assert!(!r.solids[0].sheet);
    assert_eq!(counts(&r.solids[0].brep), [2, 1, 6, 12, 8]);
    assert!(r.solids[1].sheet && r.solids[2].sheet);
    assert_eq!(counts(&r.solids[1].brep), [1, 1, 5, 12, 8]);
    assert_eq!(counts(&r.solids[2].brep), [1, 1, 1, 4, 4]);
}

/// A box written with every edge twice (one EDGE_CURVE for each face that uses it, on the same
/// LINE) reads as the box with 12 edges: the twins are one edge each, the solid closes, and the
/// reader says so.
#[test]
fn edges_written_twice_on_one_curve_record_are_one_edge() {
    let mut w = W::default();
    let style = BoxStyle { twin_edges: Some(0.0), ..BoxStyle::default() };
    let faces = box_faces(&mut w, [0.0; 3], [1.0; 3], style, &mut |_, _| Vec::new());
    assert_eq!(w.s.matches("EDGE_CURVE(").count(), 24, "every edge is written twice");
    let shell = closed_shell(&mut w, &faces);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('twins',#{shell})"));
    let s = one_solid(&w.file(&[solid]));
    assert!(!s.sheet);
    assert_eq!(counts(&s.brep), [2, 1, 6, 12, 8]);
    assert!(s.notes.iter().any(|n| n.contains("12 edges the file writes twice")), "{:?}", s.notes);
}

/// Two edges on the same vertices whose curves are different records are not merged, however
/// near they lie: nothing is decided by how close two curves come (here a LINE of its own, its
/// origin 1e-9 off).
#[test]
fn edges_written_twice_on_different_curve_records_are_not_merged() {
    let mut w = W::default();
    let style = BoxStyle { twin_edges: Some(1e-9), ..BoxStyle::default() };
    let faces = box_faces(&mut w, [0.0; 3], [1.0; 3], style, &mut |_, _| Vec::new());
    let shell = closed_shell(&mut w, &faces);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('apart',#{shell})"));
    let r = read_solids(&w.file(&[solid]));
    assert!(
        !r.solids.iter().any(|s| s.notes.iter().any(|n| n.contains("written twice"))),
        "{:?}",
        r.solids.iter().map(|s| &s.notes).collect::<Vec<_>>()
    );
}

#[test]
fn a_surface_model_of_open_shells_is_a_sheet() {
    let mut w = W::default();
    let mut faces = box_faces(&mut w, [0.0; 3], [1.0, 1.0, 1.0], BoxStyle::default(), &mut |_, _| Vec::new());
    faces.pop();
    let open = w.add(format!("OPEN_SHELL('',({}))", list(&faces)));
    let single = box_faces(&mut w, [5.0; 3], [6.0; 3], BoxStyle::default(), &mut |_, _| Vec::new());
    let one = w.add(format!("OPEN_SHELL('',(#{}))", single[0]));
    let model = w.add(format!("SHELL_BASED_SURFACE_MODEL('sheet',(#{open},#{one}))"));
    let s = one_solid(&w.file(&[model]));
    assert!(s.sheet);
    assert_eq!(counts(&s.brep), [1, 2, 6, 12 + 4, 8 + 4]);
    assert_eq!(s.brep.regions[0].shells, [0, 1]);
    let once = s.brep.edges.iter().filter(|e| e.uses.len() == 1).count();
    assert_eq!(once, 4 + 4);

    // A closed shell in a surface model encloses a region of its own.
    let mut w = W::default();
    let faces = box_faces(&mut w, [0.0; 3], [1.0; 3], BoxStyle::default(), &mut |_, _| Vec::new());
    let shell = closed_shell(&mut w, &faces);
    let model = w.add(format!("SHELL_BASED_SURFACE_MODEL('',(#{shell}))"));
    let s = one_solid(&w.file(&[model]));
    assert!(!s.sheet);
    assert_eq!(counts(&s.brep), [2, 1, 6, 12, 8]);
}

/// A solid with a void read with a face of its outer shell left out ([`read_solid_without`],
/// as the import does where that face cannot be made): the outer shell is a sheet and the void,
/// still closed, bounds its cavity, a region of its own. Its one region with the sheet made the
/// record refuse it ("a closed shell bounds two regions and encloses one"), and the whole solid
/// was lost.
#[test]
fn a_solid_with_a_void_and_a_face_left_out_keeps_its_cavity() {
    struct NoTrims;
    impl Trims for NoTrims {
        fn trims(&self, _: &TrimFace<'_>) -> std::result::Result<Vec<Vec<Curve2>>, String> {
            Err("no trims".into())
        }
    }
    let mut w = W::default();
    let faces = box_faces(&mut w, [0.0; 3], [10.0; 3], BoxStyle::default(), &mut |_, _| Vec::new());
    let outer = closed_shell(&mut w, &faces);
    let inner = box_faces(&mut w, [4.0; 3], [6.0; 3], BoxStyle { flip_faces: true, ..Default::default() }, &mut |_, _| Vec::new());
    let inner = closed_shell(&mut w, &inner);
    let void = w.add(format!("ORIENTED_CLOSED_SHELL('',*,#{inner},.F.)"));
    let solid = w.add(format!("BREP_WITH_VOIDS('',#{outer},(#{void}))"));
    let ex = w.file(&[solid]);
    let skip: HashSet<u64> = [faces[0]].into_iter().collect();
    let s = read_solid_without(&ex, solid, units::Context::default(), &NoTrims, &skip).unwrap();
    solidcore_ops::record_check::validate(&s.brep).unwrap();
    assert!(s.sheet);
    assert_eq!(counts(&s.brep)[..3], [2, 2, 11]);
    assert_eq!(s.brep.regions[0], Region { shells: vec![0, 1], void: false });
    assert_eq!(s.brep.regions[1], Region { shells: vec![1], void: true });
}

#[test]
fn a_solid_with_a_void() {
    let mut w = W::default();
    let outer = box_faces(&mut w, [0.0; 3], [10.0; 3], BoxStyle::default(), &mut |_, _| Vec::new());
    let outer = closed_shell(&mut w, &outer);
    let inner = box_faces(&mut w, [4.0; 3], [6.0; 3], BoxStyle { flip_faces: true, ..Default::default() }, &mut |_, _| Vec::new());
    let inner = closed_shell(&mut w, &inner);
    let void = w.add(format!("ORIENTED_CLOSED_SHELL('',*,#{inner},.F.)"));
    let solid = w.add(format!("BREP_WITH_VOIDS('',#{outer},(#{void}))"));
    let s = one_solid(&w.file(&[solid]));
    assert_eq!(counts(&s.brep), [3, 2, 12, 24, 16]);
    assert_eq!(s.brep.regions[0], Region { shells: vec![0], void: false });
    assert_eq!(s.brep.regions[1], Region { shells: vec![0, 1], void: false });
    assert_eq!(s.brep.regions[2], Region { shells: vec![1], void: true });
    faces_away(&s.brep, 0, |_| [5.0; 3]);
    // The void's faces face into the cavity, out of the material around it.
    for (shell, p, n) in sides(&s.brep) {
        if shell == 1 {
            let d = sub3([5.0; 3], p);
            assert!(d[0] * n[0] + d[1] * n[1] + d[2] * n[2] > 0.0, "{p:?} {n:?}");
        }
    }
}

/// A unit square sheet on z = 0 whose edge from (1, 0) to (1, 1) is a B-spline (quadratic, its
/// middle point off centre so its parameter is not proportional to length), written along or
/// against its curve.
fn spline_sheet(same_sense: bool, closed_basis: bool) -> p21::Exchange {
    let mut w = W::default();
    let ps = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
    let vs: Vec<(u64, V)> = ps.iter().map(|p| (w.vertex(*p), *p)).collect();
    let e0 = w.line_edge(vs[0], vs[1], true, 1.0);
    let e1 = if closed_basis {
        // A closed polygon spline with its seam halfway up x = 1: the edge from (1, 0) to
        // (1, 1) along it crosses the seam.
        let pts: Vec<u64> =
            [[1.0, 0.5, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.5, 0.0]].iter().map(|p| w.pt(*p)).collect();
        let c = w
            .add(format!("B_SPLINE_CURVE_WITH_KNOTS('',1,({}),.POLYLINE_FORM.,.T.,.F.,(2,1,1,1,1,2),(0.,1.,2.,3.,4.,5.),.UNSPECIFIED.)", list(&pts)));
        w.add(format!("EDGE_CURVE('',#{},#{},#{c},.T.)", vs[1].0, vs[2].0))
    } else {
        let order: [V; 3] =
            if same_sense { [[1.0, 0.0, 0.0], [1.0, 0.2, 0.0], [1.0, 1.0, 0.0]] } else { [[1.0, 1.0, 0.0], [1.0, 0.8, 0.0], [1.0, 0.0, 0.0]] };
        let pts: Vec<u64> = order.iter().map(|p| w.pt(*p)).collect();
        let c = w.add(format!("B_SPLINE_CURVE_WITH_KNOTS('',2,({}),.UNSPECIFIED.,.F.,.F.,(3,3),(2.,5.),.UNSPECIFIED.)", list(&pts)));
        let sc = w.add(format!("SURFACE_CURVE('',#{c},(),.CURVE_3D.)"));
        w.add(format!("EDGE_CURVE('',#{},#{},#{sc},{})", vs[1].0, vs[2].0, b(same_sense)))
    };
    let e2 = w.line_edge(vs[2], vs[3], false, 1.0);
    let e3 = w.line_edge(vs[3], vs[0], true, 1.0);
    let oes: Vec<u64> = [e0, e1, e2, e3].iter().map(|e| w.oe(*e, true)).collect();
    let bound = w.bound(&oes, true, true);
    let plane = w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let face = w.face(&[bound], plane, true);
    let shell = w.add(format!("OPEN_SHELL('',(#{face}))"));
    let model = w.add(format!("SHELL_BASED_SURFACE_MODEL('',(#{shell}))"));
    w.file(&[model])
}

#[test]
fn spline_edges_get_their_vertices_parameters() {
    for same_sense in [true, false] {
        let s = one_solid(&spline_sheet(same_sense, false));
        assert_eq!(counts(&s.brep), [1, 1, 1, 4, 4]);
        let e = s.brep.edges.iter().find(|e| matches!(e.curve, BCurve::Spline { .. })).unwrap();
        assert_eq!(e.interval, [2.0, 5.0]);
        let BCurve::Spline { knots, .. } = &e.curve else { panic!() };
        assert_eq!(knots, &[2.0, 2.0, 2.0, 5.0, 5.0, 5.0]);
        let [a, z] = e.vertices.map(|v| s.brep.vertices[v as usize].point);
        assert_eq!((a, z), if same_sense { ([1.0, 0.0, 0.0], [1.0, 1.0, 0.0]) } else { ([1.0, 1.0, 0.0], [1.0, 0.0, 0.0]) });
    }
    // This closed polygon spline's right edge crosses its original seam.
    let s = one_solid(&spline_sheet(true, true));
    assert_eq!(counts(&s.brep), [1, 1, 1, 4, 4]);
    let edge = s.brep.edges.iter().find(|e| matches!(e.curve, BCurve::Spline { .. })).unwrap();
    let eval = CurveEval::new(&edge.curve).unwrap();
    let [a, b] = edge.interval;
    for i in 0..=16 {
        let p = eval.point(a + (b - a) * f64::from(i) / 16.0).unwrap();
        assert!((p.x - 1.0).abs() < 1e-12 && p.z.abs() < 1e-12, "{p:?}");
        assert!((p.y - f64::from(i) / 16.0).abs() < 1e-12, "{p:?}");
    }
}

/// The issue's exact rational circles have their seam at +X and edge ends at ±Y.
/// Both caps and one side used to be discarded when their edges crossed that seam.
#[test]
fn a_cylinder_with_edges_across_closed_spline_seams_is_solid() {
    let ex = p21::parse(include_str!("../tests/fixtures/closed-spline-cylinder.step")).unwrap();
    let s = one_solid(&ex);
    assert_eq!(counts(&s.brep), [2, 1, 4, 6, 4]);
    assert!(!s.sheet);
    let (body, _) = solidcore_ops::body::Body::from_record(&s.brep, false).unwrap();
    assert!(body.is_manifold_solid().unwrap());
    let mass = body.mass_properties(1e-6).unwrap();
    assert!((mass.volume - 2.0 * PI).abs() < 1e-3, "{mass:?}");
    assert!((mass.area - 6.0 * PI).abs() < 1e-3, "{mass:?}");
}

#[test]
fn closed_spline_seam_mapping_preserves_sense_and_geometry() {
    let points = vec![[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.], [0., 0., 0.]];
    let knots = vec![0., 0., 1., 2., 3., 4., 4.];
    let polynomial = BCurve::Spline { knots: knots.clone(), points: points.clone() };
    let rational = BCurve::RationalSpline {
        knots,
        homogeneous: points.iter().zip([2., 3., 4., 2., 1.]).map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, w]).collect(),
    };
    for curve in [polynomial, rational, BCurve::Polyline { points }] {
        let original = CurveEval::new(&curve).unwrap();
        let lo = original.point(3.25).unwrap();
        let hi = original.point(0.75).unwrap();
        let array = |p: Vec3| [p.x, p.y, p.z];
        for sense in [true, false] {
            let (ps, pe) = if sense { (array(lo), array(hi)) } else { (array(hi), array(lo)) };
            let mapped = open_or_closed(curve.clone(), ps, pe, sense, false, 1e-7).unwrap();
            assert_eq!(mapped.with_step, sense);
            assert!(!mapped.sense_mismatch);
            assert_eq!(mapped.interval, [3.25, 4.75], "retain exact sampled parameters");
            let rotated = CurveEval::new(&mapped.curve).unwrap();
            for i in 0..=32 {
                let t = 3.25 + 1.5 * f64::from(i) / 32.;
                let old_t = if t >= 4. { t - 4. } else { t };
                assert!(rotated.point(t).unwrap().dist(original.point(old_t).unwrap()) < 1e-12, "{t}");
            }
        }
        assert!(open_or_closed(curve.clone(), array(lo), array(lo), true, false, 1e-7).is_err());
    }
}

#[test]
fn vertex_parameters_on_lines_conics_and_splines() {
    let tol = 1e-7;
    // A line written with magnitude 2: the record's line spans one magnitude, the interval is
    // in its parameter.
    let line = Curve::Line { origin: [1.0, 0.0, 0.0], direction: [0.0, 1.0, 0.0], magnitude: 2.0 };
    let m = map_edge(&line, [1.0, 7.0, 0.0], [1.0, 3.0, 0.0], true, false, false, tol).unwrap();
    assert_eq!(m.interval, [0.0, 2.0]);
    assert!(!m.with_step && m.sense_mismatch);
    assert_eq!(m.curve, BCurve::Line { start: [1.0, 3.0, 0.0], end: [1.0, 5.0, 0.0] });
    assert!(map_edge(&line, [1.0, 7.0, 0.0], [1.0, 7.0, 0.0], true, true, true, tol).is_err());
    // A circle, either way round, and a full turn.
    let position = Placement { origin: [0.0; 3], axis: [0.0, 0.0, 1.0], ref_direction: [1.0, 0.0, 0.0] };
    let circle = Curve::Circle { position, radius: 2.0 };
    let (s, e) = ([0.0, 2.0, 0.0], [-2.0, 0.0, 0.0]);
    let m = map_edge(&circle, s, e, true, false, false, tol).unwrap();
    assert!((m.interval[0] - FRAC_PI_2).abs() < 1e-15 && (m.interval[1] - PI).abs() < 1e-15 && m.with_step);
    let m = map_edge(&circle, s, e, false, false, false, tol).unwrap();
    assert!((m.interval[0] - PI).abs() < 1e-15 && (m.interval[1] - (TAU + FRAC_PI_2)).abs() < 1e-14 && !m.with_step, "{:?}", m.interval);
    let m = map_edge(&circle, e, e, false, true, true, tol).unwrap();
    assert!((m.interval[1] - m.interval[0] - TAU).abs() < 1e-15);
    // An ellipse's parameter is not the polar angle.
    let ellipse = Curve::Ellipse { position, semi_axis_1: 4.0, semi_axis_2: 1.0 };
    let t: f64 = 0.7;
    let p = [4.0 * t.cos(), t.sin(), 0.0];
    let m = map_edge(&ellipse, [4.0, 0.0, 0.0], p, true, false, false, tol).unwrap();
    assert!(m.interval[0].abs() < 1e-15 && (m.interval[1] - t).abs() < 1e-14, "{:?}", m.interval);
    // A hyperbola edge: rational quadratic pieces on the STEP parameters of its ends, through
    // the hyperbola's points; a parabola: one Bézier whose parameter is STEP's.
    let (a, b) = (3.0, 2.0);
    let hyperbola = Curve::Hyperbola { position, semi_axis: a, semi_imag_axis: b };
    let at = |t: f64| [a * t.cosh(), b * t.sinh(), 0.0];
    let m = map_edge(&hyperbola, at(1.5), at(-2.5), true, false, false, tol).unwrap();
    assert_eq!(m.interval, [-2.5, 1.5]);
    assert!(!m.with_step && m.sense_mismatch);
    let c = CurveEval::new(&m.curve).unwrap();
    for k in 0..=40 {
        let p = c.point(-2.5 + 4.0 * f64::from(k) / 40.0).unwrap();
        assert!(((p.x / a).powi(2) - (p.y / b).powi(2) - 1.0).abs() < 1e-12 && p.x > 0.0, "{p:?}");
    }
    for t in [-2.5, -0.5, 0.5, 1.5] {
        assert!(c.point(t).unwrap().dist(v3(at(t))) < 1e-12, "{t}");
    }
    let parabola = Curve::Parabola { position, focal_dist: 0.5 };
    let at = |t: f64| [0.5 * t * t, t, 0.0];
    let m = map_edge(&parabola, at(-1.0), at(3.0), true, false, false, tol).unwrap();
    assert_eq!(m.interval, [-1.0, 3.0]);
    let c = CurveEval::new(&m.curve).unwrap();
    for k in 0..=8 {
        let t = -1.0 + 0.5 * f64::from(k);
        assert!(c.point(t).unwrap().dist(v3(at(t))) < 1e-12, "{t}");
    }
    // Unsupported curves.
    let offset = Curve::OffsetCurve3d {
        basis: std::sync::Arc::new(line),
        distance: 1.0,
        self_intersect: crate::geom::Logical::False,
        ref_direction: [0.0, 0.0, 1.0],
    };
    assert!(map_edge(&offset, s, e, true, false, false, tol).unwrap_err().contains("not supported yet: OFFSET_CURVE_3D"));
}

/// A rational quadratic circle of radius 2 in quarters, from (2, 0), running on for `quarters`
/// quarter turns (more than four: a full turn and then some, as a record keeps a turn unrolled).
fn quarter_circle_spline(quarters: usize) -> Curve {
    let corners = [[2.0, 0.0], [2.0, 2.0], [0.0, 2.0], [-2.0, 2.0], [-2.0, 0.0], [-2.0, -2.0], [0.0, -2.0], [2.0, -2.0]];
    let points: Vec<P3> = (0..=2 * quarters).map(|i| corners[i % 8]).map(|[x, y]| [x, y, 0.0]).collect();
    let weights: Vec<f64> = (0..points.len()).map(|i| if i % 2 == 0 { 1.0 } else { std::f64::consts::FRAC_1_SQRT_2 }).collect();
    let mut multiplicities = vec![3];
    multiplicities.extend(std::iter::repeat_n(2, quarters - 1));
    multiplicities.push(3);
    Curve::BSpline(Box::new(crate::geom::BSplineCurve {
        degree: 2,
        points,
        weights: Some(weights),
        knots: (0..=quarters).map(|k| k as f64).collect(),
        multiplicities,
        form: crate::geom::CurveForm::Unspecified,
        closed: crate::geom::Logical::False,
        self_intersect: crate::geom::Logical::Unknown,
        knot_spec: crate::geom::KnotSpec::Unspecified,
    }))
}

/// The seam of the last wall of #21379's swept tube, as its file writes it: a rational cubic
/// (weights 1 up to rounding) 48 mm long, about 700 mm from the origin. Three of its points lie
/// on a circle of radius 8.6 km, and the rest of it lies microns off that circle: it is no
/// circle, and read as one it lay about 4 µm off its wall, which was then started anew a sliver
/// off its seam. A
/// rational quadratic quarter circle of radius 500 at the same distance is still its circle. Both
/// stay so moved 50 m off: the bound is at the spline's own size, wherever it lies. A spline with
/// a point that is not a number is no circle.
#[test]
fn a_nearly_straight_rational_spline_far_from_the_origin_is_not_a_circle() {
    let moved = |b: &crate::geom::BSplineCurve, by: f64| crate::geom::BSplineCurve {
        points: b.points.iter().map(|p| [p[0] + by, p[1], p[2]]).collect(),
        ..b.clone()
    };
    let seam = crate::geom::BSplineCurve {
        degree: 3,
        points: vec![
            [681.260037996487, 187.44692670357483, 52.85495796514432],
            [680.9158873817419, 191.38065431391036, 54.27139725475507],
            [680.2537941324069, 198.9485961621376, 56.9964316584237],
            [679.3160225935635, 209.66758248669714, 60.856078497380125],
            [678.5127273075279, 218.84917330359616, 64.16212726839862],
            [677.8622207424097, 226.28433846458321, 66.83933124789313],
            [677.4874017361843, 230.568439785698, 68.3819177764027],
            [677.3222395706246, 232.45620871116463, 69.06165148653872],
        ],
        weights: Some(vec![1.0, 1.0, 1.0000000000000002, 0.9999999999999993, 1.0000000000000002, 0.9999999999999998, 1.0, 1.0]),
        knots: vec![3.0, 3.262194489040001, 3.5044250097276883, 3.7144522474345854, 3.8741747496344305, 4.0],
        multiplicities: vec![4, 1, 1, 1, 1, 4],
        form: crate::geom::CurveForm::Unspecified,
        closed: crate::geom::Logical::False,
        self_intersect: crate::geom::Logical::Unknown,
        knot_spec: crate::geom::KnotSpec::Unspecified,
    };
    let (r, o) = (500.0, [700.0, 200.0, 50.0]);
    let arc = crate::geom::BSplineCurve {
        degree: 2,
        points: vec![[o[0] + r, o[1], o[2]], [o[0] + r, o[1] + r, o[2]], [o[0], o[1] + r, o[2]]],
        weights: Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
        knots: vec![0.0, 1.0],
        multiplicities: vec![3, 3],
        form: crate::geom::CurveForm::Unspecified,
        closed: crate::geom::Logical::False,
        self_intersect: crate::geom::Logical::Unknown,
        knot_spec: crate::geom::KnotSpec::Unspecified,
    };
    for by in [0.0, 5e4] {
        assert!(swept::spline_circle(&moved(&seam, by)).is_none(), "seam moved {by}");
        let Some((Curve::Circle { position, radius }, swept)) = swept::spline_circle(&moved(&arc, by)) else {
            panic!("a quarter circle moved {by} is a circle")
        };
        assert!((radius - r).abs() <= 1e-9 * r, "moved {by}: radius {radius}");
        assert!((swept - std::f64::consts::FRAC_PI_2).abs() <= 1e-9, "moved {by}: swept {swept}");
        let centre = [o[0] + by, o[1], o[2]];
        assert!(position.origin.iter().zip(centre).all(|(a, b)| (a - b).abs() <= 1e-9 * r), "moved {by}: {:?}", position.origin);
    }
    let mut broken = arc.clone();
    broken.points[1][2] = f64::NAN;
    assert!(swept::spline_circle(&broken).is_none());
}

/// A circle is accepted on the whole of the spline, not at sample points: a quarter circle with
/// knots 0.1, 0.101, 0.102 and 0.103 inserted is still its circle, and with the one control
/// point whose basis function lives on [0.1, 0.103] moved 1 mm out of the circle, or out of its
/// plane, it is not. That point is zero at every one of the 65 samples k/64 (6/64 < 0.1 and
/// 0.103 < 7/64), so the moved spline matches the circle at all of them and departs from it
/// only between two of them.
#[test]
fn a_spline_that_departs_from_its_circle_between_the_samples_is_not_a_circle() {
    let (r, o) = (500.0, [700.0, 200.0, 50.0]);
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let mut knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let mut h: Vec<[f64; 4]> = vec![[o[0] + r, o[1], o[2], 1.0], [(o[0] + r) * w, (o[1] + r) * w, o[2] * w, w], [o[0], o[1] + r, o[2], 1.0]];
    // Boehm's insertion of one knot into the degree 2 spline, on homogeneous points.
    for t in [0.1, 0.101, 0.102, 0.103] {
        let k = knots.iter().rposition(|u| *u <= t).unwrap();
        let mut next = Vec::with_capacity(h.len() + 1);
        for i in 0..=h.len() {
            next.push(if i + 2 <= k {
                h[i]
            } else if i > k {
                h[i - 1]
            } else {
                let a = (t - knots[i]) / (knots[i + 2] - knots[i]);
                std::array::from_fn(|c| a * h[i][c] + (1.0 - a) * h[i - 1][c])
            });
        }
        h = next;
        knots.insert(k + 1, t);
    }
    let spline = |h: &[[f64; 4]]| crate::geom::BSplineCurve {
        degree: 2,
        points: h.iter().map(|q| [q[0] / q[3], q[1] / q[3], q[2] / q[3]]).collect(),
        weights: Some(h.iter().map(|q| q[3]).collect()),
        knots: vec![0.0, 0.1, 0.101, 0.102, 0.103, 1.0],
        multiplicities: vec![3, 1, 1, 1, 1, 3],
        form: crate::geom::CurveForm::Unspecified,
        closed: crate::geom::Logical::False,
        self_intersect: crate::geom::Logical::Unknown,
        knot_spec: crate::geom::KnotSpec::Unspecified,
    };
    let circle = spline(&h);
    let Some((Curve::Circle { radius, .. }, _)) = swept::spline_circle(&circle) else { panic!("the quarter circle with knots inserted is a circle") };
    assert!((radius - r).abs() <= 1e-9 * r, "radius {radius}");
    // Point 3's basis function is the one on knots 0.1 to 0.103.
    assert_eq!(&knots[3..=6], &[0.1, 0.101, 0.102, 0.103]);
    let q = h[3];
    let p = [q[0] / q[3], q[1] / q[3], q[2] / q[3]];
    let out = [p[0] - o[0], p[1] - o[1]];
    let reach = out[0].hypot(out[1]);
    for (name, by) in [("out of the circle", [out[0] / reach, out[1] / reach, 0.0]), ("out of its plane", [0.0, 0.0, 1.0])] {
        let mut moved = h.clone();
        moved[3] = [(p[0] + by[0]) * q[3], (p[1] + by[1]) * q[3], (p[2] + by[2]) * q[3], q[3]];
        let moved = spline(&moved);
        for k in 0..=64 {
            let s = f64::from(k) / 64.0;
            let (a, b) = (circle.point(s).unwrap(), moved.point(s).unwrap());
            assert!(a.iter().zip(b).all(|(a, b)| (a - b).abs() <= 1e-12 * r), "{name}: sample {k} moved");
        }
        let (a, b) = (circle.point(0.1015).unwrap(), moved.point(0.1015).unwrap());
        assert!(a.iter().zip(b).map(|(a, b)| (a - b) * (a - b)).sum::<f64>() > 0.25, "{name}: the spline departs between the samples");
        assert!(swept::spline_circle(&moved).is_none(), "a spline moved {name} between the samples is a circle");
    }
}

/// A full circle written unclamped, as some writers give a closed rational circle: nine points
/// of the square's corners and edge midpoints, knots −0.25, 0 (twice) … 1 (twice), 1.25. Over its
/// domain [0, 1] a degree 2 spline does not use its outermost knots, so it is the circle the
/// clamped knots give, and it reads as that circle, a full turn. 0.49.0 refused it, as the proof
/// on its control points took only clamped knots, and a revolved round on it lost its face
/// (tangency-labs/solidcore#3401).
#[test]
fn a_circle_written_with_unclamped_knots_is_a_circle() {
    let (r, o) = (2.0, [-3.0, -132.0, 152.7]);
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let corners = [(1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (-1.0, 1.0), (-1.0, 0.0), (-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (1.0, 0.0)];
    let circle = crate::geom::BSplineCurve {
        degree: 2,
        points: corners.iter().map(|(x, z)| [o[0] + r * x, o[1], o[2] + r * z]).collect(),
        weights: Some((0..9).map(|i| if i % 2 == 1 { w } else { 1.0 }).collect()),
        knots: vec![-0.25, 0.0, 0.25, 0.5, 0.75, 1.0, 1.25],
        multiplicities: vec![1, 2, 2, 2, 2, 2, 1],
        form: crate::geom::CurveForm::Unspecified,
        closed: crate::geom::Logical::True,
        self_intersect: crate::geom::Logical::False,
        knot_spec: crate::geom::KnotSpec::Unspecified,
    };
    let Some((Curve::Circle { position, radius }, swept)) = swept::spline_circle(&circle) else { panic!("the unclamped circle is a circle") };
    assert!((radius - r).abs() <= 1e-9 * r, "radius {radius}");
    assert!((swept.abs() - std::f64::consts::TAU).abs() <= 1e-9, "swept {swept}");
    assert!(position.origin.iter().zip(o).all(|(a, b)| (a - b).abs() <= 1e-9 * r), "{:?}", position.origin);
    // Off its circle it is still refused: the point at its far side moved 1 mm out.
    let mut off = circle.clone();
    off.points[4][0] -= 1.0;
    assert!(swept::spline_circle(&off).is_none(), "a spline off its circle is a circle");
}

#[test]
fn a_closed_edge_on_a_spline_that_runs_on_past_a_full_turn_is_the_turn_between_its_passes() {
    // Five quarters from (2, 0): the curve passes (0, 2) at 1 and again at 5, its end. A
    // record's turn whose seam moved is written so, and read back the edge is that turn.
    let tol = 1e-7;
    let five = quarter_circle_spline(5);
    let m = map_edge(&five, [0.0, 2.0, 0.0], [0.0, 2.0, 0.0], true, true, true, tol).unwrap();
    assert!((m.interval[0] - 1.0).abs() < 1e-9 && (m.interval[1] - 5.0).abs() < 1e-9, "{:?}", m.interval);
    assert!(m.with_step && !m.sense_mismatch);
    // Passing (2, 0) at 0 and 4: the same, the first pass at the curve's start.
    let m = map_edge(&five, [2.0, 0.0, 0.0], [2.0, 0.0, 0.0], true, true, true, tol).unwrap();
    assert!(m.interval[0].abs() < 1e-9 && (m.interval[1] - 4.0).abs() < 1e-9, "{:?}", m.interval);
    // Three quarters pass each point once: no loop there to be the edge.
    let three = quarter_circle_spline(3);
    assert!(map_edge(&three, [0.0, 2.0, 0.0], [0.0, 2.0, 0.0], true, true, true, tol).unwrap_err().contains("closed edge on an open curve"));
    // Nine quarters pass (0, 2) three times: which turn is the edge is not known.
    let nine = quarter_circle_spline(9);
    assert!(map_edge(&nine, [0.0, 2.0, 0.0], [0.0, 2.0, 0.0], true, true, true, tol).unwrap_err().contains("closed edge on an open curve"));
}

use std::f64::consts::{FRAC_PI_2, PI};

/// The file's solids: `cube` with `edit` applied to its text.
fn edited(edit: impl Fn(&str) -> String) -> Solids {
    let mut w = W::default();
    let faces = box_faces(&mut w, [0.0; 3], [1.0; 3], BoxStyle::default(), &mut |_, _| Vec::new());
    let shell = closed_shell(&mut w, &faces);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('',#{shell})"));
    let mut extra = W { s: String::new(), n: w.n };
    extra.add(format!("ADVANCED_BREP_SHAPE_REPRESENTATION('',(#{solid}),#{})", w.n + 100));
    let text = edit(&(w.s + &extra.s));
    read_solids(&parse(&text))
}

/// The cube with its first edge's LINE replaced by a CURVE_REPLICA of a line 7 mm lower, moved up
/// by a transformation operator (what FreeCAD writes for a repeated edge).
fn replica_edge(t: &str) -> String {
    let at = t.find("=LINE('',#").expect("a line");
    let (_, line_id) = t[..at].rsplit_once('#').expect("an id");
    let tail = &t[at + "=LINE('',#".len()..];
    let (point, tail) = tail.split_once(",#").expect("a vector");
    let (vector, _) = tail.split_once(')').expect("the end");
    let pt = t.split_once(&format!("#{point}=CARTESIAN_POINT('',(")).expect("the point").1;
    let xyz: Vec<f64> = pt.split_once(')').unwrap().0.split(',').map(|v| v.parse().unwrap()).collect();
    let replaced = t.replacen(&format!("#{line_id}=LINE('',#{point},#{vector})"), &format!("#{line_id}=CURVE_REPLICA('',#9003,#9004)"), 1);
    format!(
        "{replaced}#9001=CARTESIAN_POINT('',({},{},{}));#9002=CARTESIAN_POINT('',(0.,0.,7.));#9003=LINE('',#9001,#{vector});#9004=CARTESIAN_TRANSFORMATION_OPERATOR_3D('','',$,$,#9002,$,$);",
        real(xyz[0]),
        real(xyz[1]),
        real(xyz[2] - 7.0)
    )
}

#[test]
fn an_edge_on_a_curve_replica_is_the_moved_curve() {
    let plain = edited(str::to_string);
    let moved = edited(|t| {
        let edited = replica_edge(t);
        assert!(edited.contains("CURVE_REPLICA"), "the edit did not apply");
        edited
    });
    assert!(moved.failures.is_empty(), "{:?}", moved.failures);
    assert_eq!(moved.solids.len(), 1);
    let (a, b) = (&plain.solids[0].brep, &moved.solids[0].brep);
    solidcore_ops::record_check::validate(b).unwrap();
    assert_eq!(counts(a), counts(b));
    // The replica's edge is the plain cube's edge: same curve, same interval.
    assert_eq!(a.edges[0].curve, b.edges[0].curve);
    assert_eq!(a.edges[0].interval, b.edges[0].interval);
}

#[test]
fn hostile_topology_is_an_error_never_a_panic() {
    // Sanity: unedited, the cube reads.
    assert_eq!(edited(str::to_string).solids.len(), 1);
    let cases: Vec<(&str, Box<dyn Fn(&str) -> String>)> = vec![
        ("missing shell", Box::new(|t: &str| t.replace("MANIFOLD_SOLID_BREP('',#", "MANIFOLD_SOLID_BREP('',#9999"))),
        ("missing face", Box::new(|t: &str| t.replacen("CLOSED_SHELL('',(#", "CLOSED_SHELL('',(#9999,#", 1))),
        (
            "a face twice",
            Box::new(|t: &str| {
                let (head, rest) = t.split_once("CLOSED_SHELL('',(#").unwrap();
                let first: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                format!("{head}CLOSED_SHELL('',(#{first},#{rest}")
            }),
        ),
        (
            "a face missing",
            Box::new(|t: &str| {
                let (head, rest) = t.split_once("CLOSED_SHELL('',(#").unwrap();
                let (_, rest) = rest.split_once(',').unwrap();
                format!("{head}CLOSED_SHELL('',({rest}")
            }),
        ),
        (
            "a cycle of oriented faces",
            Box::new(|t: &str| format!("{t}#9000=ORIENTED_FACE('',*,#9000,.T.);").replacen("CLOSED_SHELL('',(#", "CLOSED_SHELL('',(#9000,#", 1)),
        ),
        (
            "a cycle of oriented edges",
            Box::new(|t: &str| format!("{t}#9000=ORIENTED_EDGE('',*,*,#9000,.T.);").replacen("EDGE_LOOP('',(#", "EDGE_LOOP('',(#9000,#", 1)),
        ),
        (
            "an open loop",
            Box::new(|t: &str| {
                let (head, rest) = t.split_once("EDGE_LOOP('',(#").unwrap();
                let (_, rest) = rest.split_once(',').unwrap();
                format!("{head}EDGE_LOOP('',({rest}")
            }),
        ),
        ("an unsupported vertex", Box::new(|t: &str| t.replacen("=VERTEX_POINT(", "=VERTEX_X(", 1))),
        ("an unsupported edge", Box::new(|t: &str| t.replacen("=EDGE_CURVE(", "=SUBEDGE(", 1))),
        (
            "a vertex that is a direction",
            Box::new(|t: &str| {
                let (head, rest) = t.split_once("VERTEX_POINT('',#").unwrap();
                let (_, rest) = rest.split_once(')').unwrap();
                let d: String = t.split_once("=DIRECTION").unwrap().0.rsplit('#').next().unwrap().to_string();
                format!("{head}VERTEX_POINT('',#{d}){rest}")
            }),
        ),
        ("a face on a hyperbola's plane", Box::new(|t: &str| t.replacen("PLANE('',", "HYPERBOLA('',", 1))),
        (
            "a huge loop",
            Box::new(|t: &str| {
                let (head, rest) = t.split_once("EDGE_LOOP('',(#").unwrap();
                let first: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                let many = format!("#{first},").repeat(MAX_LOOP + 1);
                format!("{head}EDGE_LOOP('',({many}#{rest}")
            }),
        ),
        (
            "a degenerate torus",
            Box::new(|t: &str| {
                let (head, rest) = t.split_once("PLANE('',#").unwrap();
                let a: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                let (_, rest) = rest.split_once(')').unwrap();
                format!("{head}DEGENERATE_TOROIDAL_SURFACE('',#{a},1.,2.,.T.){rest}")
            }),
        ),
        (
            "a non-boolean same_sense",
            Box::new(|t: &str| {
                let (head, rest) = t.split_once("ADVANCED_FACE(").unwrap();
                format!("{head}ADVANCED_FACE({}", rest.replacen(",.T.);", ",.U.);", 1))
            }),
        ),
        ("an unsupported loop", Box::new(|t: &str| t.replacen("EDGE_LOOP(", "POLY_LOOP(", 1))),
    ];
    for (what, edit) in cases {
        let r = edited(edit);
        assert!(r.solids.is_empty() && r.failures.len() == 1, "{what}: {:?} {}", r.failures, r.solids.len());
        eprintln!("{what}: {}", r.failures[0].message);
    }
}

/// The step-conformance parts (SolidCraft's tests/step-conformance, CC0): every one reads, its
/// faces facing out of its material; the NURBS cylinder, dome and torus ring, whose faces wind
/// around a spline closed on itself in u without a seam edge, get one along its boundary column.
/// The parts live in SolidCraft's tree, where solidcore is vendored; outside it this has nothing
/// to read.
#[test]
fn step_conformance_parts() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/step-conformance");
    if !dir.is_dir() {
        return;
    }
    let mut paths: Vec<_> =
        std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "step")).collect();
    paths.sort();
    assert_eq!(paths.len(), 17);
    for path in paths {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let r = read_solids(&p21::parse(&std::fs::read_to_string(&path).unwrap()).unwrap());
        assert!(r.failures.is_empty() && r.solids.len() == 1, "{name}: {:?}", r.failures);
        let b = &r.solids[0].brep;
        solidcore_ops::record_check::validate(b).unwrap();
        solidcore_ops::record_check::agreement(b).unwrap();
        assert_eq!(b.regions.len(), 2, "{name}");
    }
}

/// Every solid of every STEP file in `SOLIDCRAFT_STEP_CORPUS` (the tangency-labs/models files)
/// read into schema 4: solids read and refused (by reason), faces, edges, time. Run with
/// `--ignored --nocapture`.
#[test]
#[ignore = "needs SOLIDCRAFT_STEP_CORPUS"]
fn corpus_solids_read_into_schema_4() {
    let Ok(dir) = std::env::var("SOLIDCRAFT_STEP_CORPUS") else { return };
    let mut paths: Vec<_> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths.iter().filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("step") || e.eq_ignore_ascii_case("stp"))) {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let t0 = std::time::Instant::now();
        let ex = p21::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let parsed = t0.elapsed();
        let r = read_solids(&ex);
        let read = t0.elapsed() - parsed;
        let faces: usize = r.solids.iter().map(|s| s.brep.faces.len()).sum();
        let edges: usize = r.solids.iter().map(|s| s.brep.edges.len()).sum();
        let sheets = r.solids.iter().filter(|s| s.sheet).count();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        println!(
            "{name}: {} solids read ({sheets} sheets), {} refused; {faces} faces, {edges} edges; parse {:.0} ms, read {:.0} ms",
            r.solids.len(),
            r.failures.len(),
            parsed.as_secs_f64() * 1e3,
            read.as_secs_f64() * 1e3
        );
        let mut reasons: std::collections::BTreeMap<String, usize> = Default::default();
        for f in &r.failures {
            // Group by the message without entity numbers.
            let key: String = f.message.split('#').map(|s| s.trim_start_matches(|c: char| c.is_ascii_digit())).collect::<Vec<_>>().join("#");
            *reasons.entry(key).or_default() += 1;
        }
        for (m, n) in &reasons {
            println!("    {n} × {m}");
        }
        let mut notes: std::collections::BTreeMap<String, usize> = Default::default();
        for s in &r.solids {
            for n in &s.notes {
                let key: String = n.chars().skip_while(|c| c.is_ascii_digit()).collect();
                *notes.entry(key).or_default() += 1;
            }
        }
        for (m, n) in &notes {
            println!("    note in {n} solids: …{m}");
        }
    }
}

/// One STEP surface (entity #10 of `data`, after the common placements) sampled over a box of
/// its own parameters: the mapped record passes through every sample, and its natural normal is
/// STEP's (or its opposite where the mapping says so).
#[track_caller]
fn same_surface(data: &str, us: (f64, f64), vs: (f64, f64)) -> BSurface {
    const BASE: &str = "#1=CARTESIAN_POINT('',(0.,0.,0.));#2=DIRECTION('',(0.,0.,1.));#3=DIRECTION('',(1.,0.,0.));\
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);#5=DIRECTION('',(0.,1.,0.));#6=AXIS1_PLACEMENT('',#1,#2);";
    let ex = parse(&format!("{BASE}{data}\n"));
    let s = crate::read::Reader::new(&ex, crate::read::Units::default()).surface(10).unwrap();
    let n = 8;
    let grid: Vec<(f64, f64)> = (0..=n)
        .flat_map(|i| (0..=n).map(move |j| (us.0 + (us.1 - us.0) * i as f64 / n as f64, vs.0 + (vs.1 - vs.0) * j as f64 / n as f64)))
        .collect();
    let ext = swept::Extent { points: grid.iter().map(|&(u, v)| v3(s.point(u, v).unwrap())).collect(), seam: Vec::new(), walks: Vec::new() };
    let (record, reversed) = swept::map_surface(&s, &ext, DEFAULT_TOLERANCE).unwrap();
    let _ = solidcore_ops::label::surface_natural(&record);
    let eval = SurfaceEval::new(&record).unwrap();
    for &(u, v) in &grid {
        let p = v3(s.point(u, v).unwrap());
        let q = eval.project(p, None, f64::INFINITY).unwrap_or_else(|| panic!("{data}: no parameters at ({u}, {v})"));
        let (at, su, sv) = eval.derivs(q[0], q[1]).unwrap();
        assert!(at.dist(p) < 1e-8, "{data}: ({u}, {v}) {p:?} is {} off", at.dist(p));
        let mine = su.cross(sv).normalized().unwrap();
        let theirs = v3(s.normal(u, v).unwrap());
        let agree = mine.dot(theirs) * if reversed { -1.0 } else { 1.0 };
        assert!(agree > 0.999, "{data}: ({u}, {v}) normal {mine:?} vs {theirs:?} (reversed {reversed})");
    }
    record
}

#[test]
fn swept_and_offset_surfaces_are_the_files_own() {
    // Revolutions: a slanted line (a cone's frustum), a ball's half circle, an ellipse off the
    // axis, a B-spline profile; a circle in a plane through the axis is the ring torus it is.
    let line = "#7=CARTESIAN_POINT('',(5.,0.,0.));#8=DIRECTION('',(-2.,0.,4.));#9=VECTOR('',#8,4.47213595499958);#11=LINE('',#7,#9);";
    let r = same_surface(&format!("{line}#10=SURFACE_OF_REVOLUTION('',#11,#6);"), (0.0, 6.0), (0.0, 1.0));
    assert!(matches!(r, BSurface::PlacedRevolution { forward: false, .. }));
    // A circle across the axis covers the ball twice, once each way round: its trims say which
    // half the surface is.
    for (half, vs) in [
        ("(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(3.14159265358979))", (0.2, 3.0)),
        ("(PARAMETER_VALUE(3.14159265358979)),(PARAMETER_VALUE(0.))", (3.3, 6.0)),
    ] {
        let ball = format!(
            "#7=AXIS2_PLACEMENT_3D('',#1,#5,#2);#12=CIRCLE('',#7,5.);#11=TRIMMED_CURVE('',#12,{half},.T.,.PARAMETER.);#10=SURFACE_OF_REVOLUTION('',#11,#6);"
        );
        assert!(matches!(same_surface(&ball, (0.0, 6.0), vs), BSurface::PlacedRevolution { .. }));
    }
    let tilted =
        "#7=CARTESIAN_POINT('',(10.,0.,1.));#8=AXIS2_PLACEMENT_3D('',#7,#5,#3);#11=ELLIPSE('',#8,3.,2.);#10=SURFACE_OF_REVOLUTION('',#11,#6);";
    same_surface(tilted, (-1.0, 2.0), (-1.0, 1.5));
    let spline = "#7=CARTESIAN_POINT('',(4.,0.,0.));#8=CARTESIAN_POINT('',(6.,0.,2.));#9=CARTESIAN_POINT('',(5.,0.,5.));\
#11=B_SPLINE_CURVE_WITH_KNOTS('',2,(#7,#8,#9),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.);#10=SURFACE_OF_REVOLUTION('',#11,#6);";
    same_surface(spline, (0.0, 6.0), (0.05, 0.95));
    for axis in ["#5", "#12"] {
        let torus = format!(
            "#12=DIRECTION('',(0.,-1.,0.));#7=CARTESIAN_POINT('',(10.,0.,0.));#8=AXIS2_PLACEMENT_3D('',#7,{axis},#3);#11=CIRCLE('',#8,2.);#10=SURFACE_OF_REVOLUTION('',#11,#6);"
        );
        assert!(matches!(same_surface(&torus, (0.0, 6.0), (0.0, 6.0)), BSurface::Torus { .. }));
    }
    // Extrusions: an ellipse askew, a line (a plane strip), a B-spline, a parabola, a hyperbola;
    // a circle along its axis either way is a cylinder.
    let askew = "#7=ELLIPSE('',#4,9.,4.);#8=DIRECTION('',(0.5,0.25,1.));#9=VECTOR('',#8,2.);#10=SURFACE_OF_LINEAR_EXTRUSION('',#7,#9);";
    assert!(matches!(same_surface(askew, (-1.0, 1.0), (0.0, 5.0)), BSurface::RationalSpline { .. }));
    let strip = "#7=VECTOR('',#3,2.);#8=LINE('',#1,#7);#9=VECTOR('',#5,1.);#10=SURFACE_OF_LINEAR_EXTRUSION('',#8,#9);";
    assert!(matches!(same_surface(strip, (-1.0, 3.0), (0.0, 5.0)), BSurface::Spline { .. }));
    let spline = "#7=CARTESIAN_POINT('',(4.,0.,0.));#8=CARTESIAN_POINT('',(6.,1.,0.));#9=CARTESIAN_POINT('',(5.,3.,0.));\
#11=B_SPLINE_CURVE_WITH_KNOTS('',2,(#7,#8,#9),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.);#12=VECTOR('',#2,3.);#10=SURFACE_OF_LINEAR_EXTRUSION('',#11,#12);";
    same_surface(spline, (0.05, 0.95), (-1.0, 1.0));
    let parabola = "#7=PARABOLA('',#4,0.5);#8=VECTOR('',#2,1.);#10=SURFACE_OF_LINEAR_EXTRUSION('',#7,#8);";
    same_surface(parabola, (-2.0, 3.0), (0.0, 4.0));
    let hyperbola = "#7=HYPERBOLA('',#4,3.,2.);#8=VECTOR('',#2,1.);#10=SURFACE_OF_LINEAR_EXTRUSION('',#7,#8);";
    same_surface(hyperbola, (-2.0, 1.5), (0.0, 4.0));
    for (dir, reversed) in [("#2", false), ("#7", true)] {
        let tube = format!("#7=DIRECTION('',(0.,0.,-1.));#8=CIRCLE('',#4,5.);#9=VECTOR('',{dir},3.);#10=SURFACE_OF_LINEAR_EXTRUSION('',#8,#9);");
        assert!(matches!(same_surface(&tube, (0.0, 6.0), (0.0, 2.0)), BSurface::Cylinder { radius: 5.0, .. }));
        // Offset along STEP's normal: outward one way, inward the other.
        let off = tube.replace("#10=SURFACE", "#11=SURFACE").replace("#10=", "#11=") + "#10=OFFSET_SURFACE('',#11,1.,.F.);";
        let want = if reversed { 4.0 } else { 6.0 };
        assert!(matches!(same_surface(&off, (0.0, 6.0), (0.0, 2.0)), BSurface::Cylinder { radius, .. } if (radius - want).abs() < 1e-12), "{dir}");
    }
    // Offsets of each analytic kind, nested ones too.
    for (basis, d) in [
        ("PLANE('',#4)", 2.0),
        ("CYLINDRICAL_SURFACE('',#4,5.)", -2.0),
        ("CONICAL_SURFACE('',#4,5.,0.3)", 1.5),
        ("CONICAL_SURFACE('',#4,5.,-0.3)", -1.0),
        ("SPHERICAL_SURFACE('',#4,5.)", 1.0),
        ("TOROIDAL_SURFACE('',#4,10.,2.)", 1.0),
    ] {
        let one = format!("#11={basis};#10=OFFSET_SURFACE('',#11,{},.F.);", real(d));
        same_surface(&one, (0.1, 2.0), (0.1, 1.2));
        let two = format!("#12={basis};#11=OFFSET_SURFACE('',#12,{},.F.);#10=OFFSET_SURFACE('',#11,0.5,.U.);", real(d));
        same_surface(&two, (0.1, 2.0), (0.1, 1.2));
    }
    // Refused: offsets that collapse or of other kinds, an extrusion in its profile's plane.
    let ext = swept::Extent { points: vec![Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 1.0)], seam: Vec::new(), walks: Vec::new() };
    for (data, why) in [
        ("#11=CYLINDRICAL_SURFACE('',#4,5.);#10=OFFSET_SURFACE('',#11,-5.,.F.);", "collapses"),
        ("#11=TOROIDAL_SURFACE('',#4,10.,2.);#10=OFFSET_SURFACE('',#11,9.,.F.);", "meets its axis"),
        ("#11=CYLINDRICAL_SURFACE('',#4,5.);#12=OFFSET_SURFACE('',#11,-6.,.F.);#10=OFFSET_SURFACE('',#12,6.,.F.);", "collapses"),
        ("#7=ELLIPSE('',#4,9.,4.);#8=VECTOR('',#3,1.);#10=SURFACE_OF_LINEAR_EXTRUSION('',#7,#8);", "profile's plane"),
        (
            "#7=ELLIPSE('',#4,9.,4.);#8=VECTOR('',#2,1.);#11=SURFACE_OF_LINEAR_EXTRUSION('',#7,#8);#10=OFFSET_SURFACE('',#11,1.,.F.);",
            "not supported yet",
        ),
    ] {
        let exch = parse(&format!(
            "#1=CARTESIAN_POINT('',(0.,0.,0.));#2=DIRECTION('',(0.,0.,1.));#3=DIRECTION('',(1.,0.,0.));#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);{data}\n"
        ));
        let s = crate::read::Reader::new(&exch, crate::read::Units::default()).surface(10).unwrap();
        let e = swept::map_surface(&s, &ext, DEFAULT_TOLERANCE).unwrap_err();
        assert!(e.contains(why), "{data}: {e}");
    }
}

/// A frustum (radius 5 at z = 0 to radius 3 at z = 4) whose side is a SURFACE_OF_REVOLUTION of
/// its slanted profile line, with the profile as a seam edge or without one, about +z or −z
/// (STEP's normal ∂angle × ∂profile then points in, and the face says same_sense .F.).
fn frustum(seam: bool, down: bool) -> p21::Exchange {
    let mut w = W::default();
    let (pb, pt) = ([5.0, 0.0, 0.0], [3.0, 0.0, 4.0]);
    let (vb, vt) = (w.vertex(pb), w.vertex(pt));
    let kb = w.circle([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 5.0);
    let kt = w.circle([0.0, 0.0, 4.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 3.0);
    let cb = w.add(format!("EDGE_CURVE('',#{vb},#{vb},#{kb},.T.)"));
    let ct = w.add(format!("EDGE_CURVE('',#{vt},#{vt},#{kt},.T.)"));
    let (o, d) = (w.pt(pb), w.dir([-2.0, 0.0, 4.0]));
    let vec = w.add(format!("VECTOR('',#{d},{})", real(20f64.sqrt())));
    let profile = w.add(format!("LINE('',#{o},#{vec})"));
    let origin = w.pt([0.0; 3]);
    let z = w.dir([0.0, 0.0, if down { -1.0 } else { 1.0 }]);
    let axis = w.add(format!("AXIS1_PLACEMENT('',#{origin},#{z})"));
    let surface = w.add(format!("SURFACE_OF_REVOLUTION('',#{profile},#{axis})"));
    let bounds = if seam {
        let s = w.add(format!("EDGE_CURVE('',#{vb},#{vt},#{profile},.T.)"));
        let ring = [(cb, true), (s, true), (ct, false), (s, false)];
        let oes: Vec<u64> = ring.iter().map(|&(e, o)| w.oe(e, o)).collect();
        vec![w.bound(&oes, true, true)]
    } else {
        let (x, y) = (w.oe(cb, true), w.oe(ct, false));
        vec![w.bound(&[x], true, true), w.bound(&[y], false, true)]
    };
    let side = w.face(&bounds, surface, !down);
    let x = w.oe(cb, false);
    let bb = w.bound(&[x], true, true);
    let plane = w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let bottom = w.face(&[bb], plane, false);
    let x = w.oe(ct, true);
    let tb = w.bound(&[x], true, true);
    let plane = w.plane([0.0, 0.0, 4.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let top = w.face(&[tb], plane, true);
    let shell = closed_shell(&mut w, &[side, bottom, top]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('frustum',#{shell})"));
    w.file(&[solid])
}

#[test]
fn surfaces_of_revolution_read_as_placed_revolutions() {
    for seam in [true, false] {
        for down in [false, true] {
            let s = one_solid(&frustum(seam, down));
            assert_eq!(counts(&s.brep), [2, 1, 3, 3, 2], "seam {seam} down {down}");
            assert!(s.brep.faces.iter().any(|f| matches!(f.surface, BSurface::PlacedRevolution { forward: false, .. })));
            assert_eq!(s.notes.iter().any(|n| n.contains("seam edges added")), !seam, "{:?}", s.notes);
            faces_away(&s.brep, 0, |p| [0.0, 0.0, p[2].clamp(1.0, 3.0)]);
        }
    }

    // A ball: a half circle (trimmed from the north pole to the south through +x) revolved,
    // bounded by its seam meridian alone. STEP's normal there points in: same_sense .F.
    let mut w = W::default();
    let (vn, vs) = (w.vertex([0.0, 0.0, 5.0]), w.vertex([0.0, 0.0, -5.0]));
    let k = w.circle([0.0; 3], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 5.0);
    let half = w.add(format!("TRIMMED_CURVE('',#{k},(PARAMETER_VALUE(0.)),(PARAMETER_VALUE({})),.T.,.PARAMETER.)", real(PI)));
    let e = w.add(format!("EDGE_CURVE('',#{vn},#{vs},#{k},.T.)"));
    let (o1, o2) = (w.oe(e, true), w.oe(e, false));
    let bound = w.bound(&[o1, o2], true, true);
    let (origin, z) = (w.pt([0.0; 3]), w.dir([0.0, 0.0, 1.0]));
    let axis = w.add(format!("AXIS1_PLACEMENT('',#{origin},#{z})"));
    let surface = w.add(format!("SURFACE_OF_REVOLUTION('',#{half},#{axis})"));
    let face = w.face(&[bound], surface, false);
    let shell = closed_shell(&mut w, &[face]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('ball',#{shell})"));
    let s = one_solid(&w.file(&[solid]));
    assert_eq!(counts(&s.brep), [2, 1, 1, 1, 2]);
    faces_away(&s.brep, 0, |_| [0.0; 3]);
}

#[test]
fn surfaces_of_extrusion_read_as_splines_or_cylinders() {
    // A half elliptic prism: the ellipse (6 by 3) from −y round +x to +y, extruded 5 up, cut
    // by the plane x = 0.
    let mut w = W::default();
    let p = |y: f64, z: f64| [0.0, y, z];
    let v: Vec<(u64, V)> = [p(-3.0, 0.0), p(3.0, 0.0), p(-3.0, 5.0), p(3.0, 5.0)].iter().map(|q| (w.vertex(*q), *q)).collect();
    let ellipse = |w: &mut W, z: f64| {
        let a = w.axis([0.0, 0.0, z], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        w.add(format!("ELLIPSE('',#{a},6.,3.)"))
    };
    let (eb, et) = (ellipse(&mut w, 0.0), ellipse(&mut w, 5.0));
    let arc_b = w.add(format!("EDGE_CURVE('',#{},#{},#{eb},.T.)", v[0].0, v[1].0));
    let arc_t = w.add(format!("EDGE_CURVE('',#{},#{},#{et},.T.)", v[2].0, v[3].0));
    let up_m = w.line_edge(v[0], v[2], true, 1.0);
    let up_p = w.line_edge(v[1], v[3], true, 1.0);
    let low = w.line_edge(v[1], v[0], true, 1.0);
    let high = w.line_edge(v[2], v[3], true, 1.0);
    let z = w.dir([0.0, 0.0, 1.0]);
    let vec = w.add(format!("VECTOR('',#{z},2.)"));
    let surface = w.add(format!("SURFACE_OF_LINEAR_EXTRUSION('',#{eb},#{vec})"));
    let face = |w: &mut W, ring: &[(u64, bool)], surface: u64, sense: bool| {
        let oes: Vec<u64> = ring.iter().map(|&(e, o)| w.oe(e, o)).collect();
        let b = w.bound(&oes, true, true);
        w.face(&[b], surface, sense)
    };
    let curved = face(&mut w, &[(arc_b, true), (up_p, true), (arc_t, false), (up_m, false)], surface, true);
    let flat_plane = w.plane([0.0; 3], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let flat = face(&mut w, &[(low, true), (up_m, true), (high, true), (up_p, false)], flat_plane, true);
    let bottom_plane = w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let bottom = face(&mut w, &[(arc_b, false), (low, false)], bottom_plane, false);
    let top_plane = w.plane([0.0, 0.0, 5.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let top = face(&mut w, &[(arc_t, true), (high, false)], top_plane, true);
    let shell = closed_shell(&mut w, &[curved, flat, bottom, top]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('half',#{shell})"));
    let s = one_solid(&w.file(&[solid]));
    assert_eq!(counts(&s.brep), [2, 1, 4, 6, 4]);
    assert!(s.brep.faces.iter().any(|f| matches!(f.surface, BSurface::RationalSpline { .. })));
    faces_away(&s.brep, 0, |q| [q[0].clamp(0.5, 2.0), q[1].clamp(-1.0, 1.0), q[2].clamp(1.0, 4.0)]);

    // A cylinder whose side is its bottom circle extruded up (or down, STEP's normal then
    // pointing in), or a narrower cylinder offset out by 1.
    for variant in ["up", "down", "offset"] {
        let mut w = W::default();
        let c = cylinder(&mut w, [1.0, 2.0], 5.0, [0.0, 10.0]);
        let side_face = side(&mut w, &c, true);
        let bottom_bound = {
            let ob = w.oe(c.circles[0], false);
            w.bound(&[ob], true, true)
        };
        let bottom_plane = w.plane([1.0, 2.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let bottom = w.face(&[bottom_bound], bottom_plane, false);
        let top_bound = {
            let ot = w.oe(c.circles[1], true);
            w.bound(&[ot], true, true)
        };
        let top_plane = w.plane([1.0, 2.0, 10.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let top = w.face(&[top_bound], top_plane, true);
        let k = w.circle([1.0, 2.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 5.0);
        let d = w.dir([0.0, 0.0, if variant == "down" { -1.0 } else { 1.0 }]);
        let vec = w.add(format!("VECTOR('',#{d},1.)"));
        let a = w.axis([1.0, 2.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let narrow = w.add(format!("CYLINDRICAL_SURFACE('',#{a},4.)"));
        let shell = closed_shell(&mut w, &[bottom, side_face, top]);
        let solid = w.add(format!("MANIFOLD_SOLID_BREP('',#{shell})"));
        let old = format!("#{}=CYLINDRICAL_SURFACE", c.surface);
        let text = w.s.lines().map(|l| {
            if l.starts_with(&old) {
                match variant {
                    "offset" => format!("#{}=OFFSET_SURFACE('',#{narrow},1.,.F.);", c.surface),
                    _ => format!("#{}=SURFACE_OF_LINEAR_EXTRUSION('',#{k},#{vec});", c.surface),
                }
            } else if variant == "down" && l.starts_with(&format!("#{side_face}=")) {
                l.replace(",.T.);", ",.F.);")
            } else {
                l.to_string()
            }
        });
        w.s = text.collect::<Vec<_>>().join("\n") + "\n";
        assert!(!w.s.contains(&old) && w.s.contains(if variant == "offset" { "OFFSET_SURFACE" } else { "LINEAR_EXTRUSION" }));
        assert_eq!(w.s.contains(&format!("#{side_face}=ADVANCED_FACE('',(#{}),#{},.F.)", side_face - 1, c.surface)), variant == "down");
        let s = one_solid(&w.file(&[solid]));
        assert_eq!(counts(&s.brep), [2, 1, 3, 3, 2], "{variant}");
        assert!(s.brep.faces.iter().any(|f| matches!(f.surface, BSurface::Cylinder { radius, .. } if (radius - 5.0).abs() < 1e-12)), "{variant}");
        faces_away(&s.brep, 0, |q| [1.0, 2.0, q[2].clamp(1.0, 9.0)]);
    }
}

#[test]
fn parabola_and_hyperbola_edges_bound_faces() {
    for kind in ["PARABOLA('',#{a},0.5)", "HYPERBOLA('',#{a},1.,1.)"] {
        let mut w = W::default();
        let at = |t: f64| if kind.starts_with("PARABOLA") { [0.5 * t * t, t, 0.0] } else { [t.cosh(), t.sinh(), 0.0] };
        let (p1, p2) = (at(-1.5), at(1.5));
        let (v1, v2) = (w.vertex(p1), w.vertex(p2));
        let a = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let conic = w.add(kind.replace("{a}", &a.to_string()));
        let e1 = w.add(format!("EDGE_CURVE('',#{v1},#{v2},#{conic},.T.)"));
        let e2 = w.line_edge((v2, p2), (v1, p1), true, 1.0);
        let (o1, o2) = (w.oe(e1, true), w.oe(e2, true));
        let bound = w.bound(&[o1, o2], true, true);
        let plane = w.plane([0.0; 3], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
        let face = w.face(&[bound], plane, true);
        let shell = w.add(format!("OPEN_SHELL('',(#{face}))"));
        let model = w.add(format!("SHELL_BASED_SURFACE_MODEL('',(#{shell}))"));
        let s = one_solid(&w.file(&[model]));
        assert_eq!(counts(&s.brep), [1, 1, 1, 2, 2], "{kind}");
        let e = s.brep.edges.iter().find(|e| !matches!(e.curve, BCurve::Line { .. })).unwrap();
        assert_eq!(e.interval, [-1.5, 1.5]);
        let c = CurveEval::new(&e.curve).unwrap();
        for k in 0..=12 {
            let p = c.point(-1.5 + 0.25 * f64::from(k)).unwrap();
            let on = if kind.starts_with("PARABOLA") { p.y * p.y - 2.0 * p.x } else { p.x * p.x - p.y * p.y - 1.0 };
            assert!(on.abs() < 1e-12, "{kind}: {p:?}");
        }
    }
}

/// SolidCraft's own export of a cross-bored rod whose bore rim is a B-spline of 1 399 control
/// points running back and forth across the bore (tests/step-regressions in SolidCraft, #4598):
/// the rim's faces have no closed-form pcurves there, and the refusal names the face rather
/// than build the folded rim. Ported from step-brep's `tests/issue_2583_topology_first.rs` with
/// its fixture; step-brep built the rim as written.
#[test]
fn rod_doubled_edge_names_the_face_of_its_folded_rim() {
    let text = include_str!("../tests/fixtures/rod_doubled_edge.step");
    let r = read_solids(&p21::parse(text).unwrap());
    assert!(r.solids.is_empty());
    let why = &r.failures.first().expect("a refusal").message;
    assert!(super::named_face(why).is_some(), "{why}");
}

/// A sliver face's parameter box is `OPEN_DOMAIN_SPAN` across even against a bound of its
/// surface's natural range, and pcurves wholly outside that range stay an empty box (#12757).
#[test]
fn face_domain_sliver_at_a_natural_bound() {
    use std::f64::consts::FRAC_PI_2;
    let ball = BSurface::Sphere { frame: Frame { origin: [0.0; 3], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] }, radius: 5.0 };
    let loop_at = |v0: f64, v1: f64| vec![vec![Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![[0.1, v0], [0.3, v1]] }]];
    for (v0, v1, want) in [
        (-FRAC_PI_2, -FRAC_PI_2 + 1e-6, [-FRAC_PI_2, -FRAC_PI_2 + OPEN_DOMAIN_SPAN]),
        (FRAC_PI_2 - 1e-6, FRAC_PI_2, [FRAC_PI_2 - OPEN_DOMAIN_SPAN, FRAC_PI_2]),
    ] {
        let d = face_domain(&ball, &loop_at(v0, v1)).unwrap();
        assert!((d[1][0] - want[0]).abs() < 1e-12 && (d[1][1] - want[1]).abs() < 1e-12, "{v0} {v1}: {d:?}");
    }
    let mid = face_domain(&ball, &loop_at(0.0, 1e-6)).unwrap();
    assert!((mid[1][1] - mid[1][0] - OPEN_DOMAIN_SPAN).abs() < 1e-12 && mid[1][0] < 0.0 && mid[1][1] > 1e-6, "{mid:?}");
    assert!(face_domain(&ball, &loop_at(FRAC_PI_2 + 1e-4, FRAC_PI_2 + 2e-4)).is_err());
}

/// A prism over a closed periodic cubic (an octagon's uniform B-spline, written unclamped with
/// v_closed .T., its first three control points repeated at the end, as exporters write a closed
/// spline) from z = 0 to 4: its side is a ruled spline closed in v, bounded by the two closed
/// B-spline curves without a seam edge, closed by two planar caps. The unclamped knots are
/// clamped on reading (the same surface: its points at the domain's ends are the curves'), and
/// the side gets a seam along its boundary row from the bottom curve's vertex to the top's.
#[test]
fn a_spline_closed_in_v_without_a_seam_gets_one() {
    let octagon: Vec<V> = (0..8).map(|k| f64::from(k) * std::f64::consts::FRAC_PI_4).map(|t| [5.0 * t.cos(), 5.0 * t.sin(), 0.0]).collect();
    let wrapped: Vec<V> = octagon.iter().chain(octagon.iter().take(3)).copied().collect();
    let knots = (0..15).map(|k| real(f64::from(k))).collect::<Vec<_>>().join(",");
    let ones = vec!["1"; 15].join(",");
    let mut w = W::default();
    let at = |p: V, z: f64| [p[0], p[1], z];
    let curve = |w: &mut W, z: f64| {
        let pts: Vec<u64> = wrapped.iter().map(|p| w.pt(at(*p, z))).collect();
        w.add(format!("B_SPLINE_CURVE_WITH_KNOTS('',3,({}),.UNSPECIFIED.,.T.,.F.,({ones}),({knots}),.UNSPECIFIED.)", list(&pts)))
    };
    // The uniform cubic at its domain's start (t = 3): (P0 + 4 P1 + P2) / 6.
    let start = |z: f64| {
        let [a, b, c] = [octagon[0], octagon[1], octagon[2]];
        [(a[0] + 4.0 * b[0] + c[0]) / 6.0, (a[1] + 4.0 * b[1] + c[1]) / 6.0, z]
    };
    let edge = |w: &mut W, z: f64| {
        let k = curve(w, z);
        let v = w.vertex(start(z));
        w.add(format!("EDGE_CURVE('',#{v},#{v},#{k},.T.)"))
    };
    let (bottom, top) = (edge(&mut w, 0.0), edge(&mut w, 4.0));
    let rows: Vec<String> = [0.0, 4.0]
        .iter()
        .map(|z| {
            let pts: Vec<u64> = wrapped.iter().map(|p| w.pt(at(*p, *z))).collect();
            format!("({})", list(&pts))
        })
        .collect();
    let surface = w.add(format!(
        "B_SPLINE_SURFACE_WITH_KNOTS('',1,3,({}),.RULED_SURF.,.F.,.T.,.F.,(2,2),({ones}),(0.,1.),({knots}),.UNSPECIFIED.)",
        rows.join(",")
    ));
    // The side's normal (along z, then round counter-clockwise) points in: same_sense .F.;
    // outward, the bottom curve runs counter-clockwise about +z, the top clockwise.
    let (x, y) = (w.oe(bottom, true), w.oe(top, false));
    let (bx, by) = (w.bound(&[x], true, true), w.bound(&[y], false, true));
    let side = w.face(&[bx, by], surface, false);
    let plane_b = w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let x = w.oe(bottom, false);
    let bx = w.bound(&[x], true, true);
    let cap_b = w.face(&[bx], plane_b, false);
    let plane_t = w.plane([0.0, 0.0, 4.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let x = w.oe(top, true);
    let bx = w.bound(&[x], true, true);
    let cap_t = w.face(&[bx], plane_t, true);
    let shell = closed_shell(&mut w, &[side, cap_b, cap_t]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('prism',#{shell})"));
    let s = one_solid(&w.file(&[solid]));
    assert_eq!(counts(&s.brep), [2, 1, 3, 3, 2]);
    assert!(s.notes.iter().any(|n| n.contains("1 seam edges added")), "{:?}", s.notes);
    // Clamped: the side's knots repeat their ends four times.
    let side = s.brep.faces.iter().find_map(|f| match &f.surface {
        BSurface::Spline { v_knots, .. } => Some(v_knots.clone()),
        _ => None,
    });
    let v_knots = side.expect("the side's spline");
    assert!(v_knots.iter().take(4).all(|k| *k == 3.0) && v_knots.iter().rev().take(4).all(|k| *k == 11.0), "{v_knots:?}");
    faces_away(&s.brep, 0, |p| [0.0, 0.0, p[2].clamp(1.0, 3.0)]);
}

/// A cone as a B-spline surface closed in v whose first u row collapses to its apex, bounded by
/// its base curve alone (a closed B-spline without a seam edge) and a planar cap: the face covers
/// the apex, a pole at the start of its u range, which lies on the side of lower u of a loop
/// running +v. It gets one seam along its boundary row, from the base curve's vertex to a new
/// vertex at the apex.
#[test]
fn a_spline_cone_closed_in_v_gets_a_seam_to_its_apex() {
    let octagon: Vec<V> = (0..8).map(|k| f64::from(k) * std::f64::consts::FRAC_PI_4).map(|t| [5.0 * t.cos(), 5.0 * t.sin(), 0.0]).collect();
    let wrapped: Vec<V> = octagon.iter().chain(octagon.iter().take(3)).copied().collect();
    let knots = (0..15).map(|k| real(f64::from(k))).collect::<Vec<_>>().join(",");
    let ones = vec!["1"; 15].join(",");
    let mut w = W::default();
    let pts: Vec<u64> = wrapped.iter().map(|p| w.pt(*p)).collect();
    let base = w.add(format!("B_SPLINE_CURVE_WITH_KNOTS('',3,({}),.UNSPECIFIED.,.T.,.F.,({ones}),({knots}),.UNSPECIFIED.)", list(&pts)));
    // The uniform cubic at its domain's start (t = 3): (P0 + 4 P1 + P2) / 6.
    let [a, b, c] = [octagon[0], octagon[1], octagon[2]];
    let v = w.vertex([(a[0] + 4.0 * b[0] + c[0]) / 6.0, (a[1] + 4.0 * b[1] + c[1]) / 6.0, 0.0]);
    let edge = w.add(format!("EDGE_CURVE('',#{v},#{v},#{base},.T.)"));
    let apex: Vec<u64> = wrapped.iter().map(|_| w.pt([0.0, 0.0, 4.0])).collect();
    let rim: Vec<u64> = wrapped.iter().map(|p| w.pt(*p)).collect();
    let surface = w.add(format!(
        "B_SPLINE_SURFACE_WITH_KNOTS('',1,3,(({}),({})),.UNSPECIFIED.,.F.,.T.,.F.,(2,2),({ones}),(0.,1.),({knots}),.UNSPECIFIED.)",
        list(&apex),
        list(&rim)
    ));
    // From the apex (u = 0) down to the rim, then round counter-clockwise: the normal points
    // out, and outward the base curve runs counter-clockwise about +z.
    let x = w.oe(edge, true);
    let bx = w.bound(&[x], true, true);
    let side = w.face(&[bx], surface, true);
    let plane = w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let x = w.oe(edge, false);
    let bx = w.bound(&[x], true, true);
    let cap = w.face(&[bx], plane, false);
    let shell = closed_shell(&mut w, &[side, cap]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('cone',#{shell})"));
    let s = one_solid(&w.file(&[solid]));
    assert_eq!(counts(&s.brep), [2, 1, 2, 2, 2]);
    assert!(s.notes.iter().any(|n| n.contains("1 seam edges added")), "{:?}", s.notes);
    assert!(
        s.brep.vertices.iter().any(|v| {
            let d = sub3(v.point, [0.0, 0.0, 4.0]);
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() <= 1e-9
        }),
        "no vertex at the apex"
    );
    faces_away(&s.brep, 0, |_| [0.0, 0.0, 1.0]);
}

/// A prism whose side is a B-spline surface closed around u (its first and last control columns
/// the same), extruded from a closed quintic on 121 points of a wavy outline (a radius of 5 ± 2 mm,
/// thirty waves round), as a fit-point spline is written; the side's loop has its seam,
/// a line at the knots' start used both ways. The turns the loop makes round u were counted from
/// each point's parameters found from the last point's alone, which settled on another wave
/// millimetres away and counted a turn the loop does not make: the side was refused as "a face
/// around an open surface of revolution with one bound" and the solid read open (#16723).
#[test]
fn a_wavy_spline_prism_with_its_seam_gets_no_other() {
    let n = 121;
    let outline: Vec<V> = (0..n)
        .map(|k| {
            // The last point is the first, exactly.
            let t = std::f64::consts::TAU * f64::from(k % (n - 1)) / f64::from(n - 1);
            let r = 5.0 + 2.0 * (30.0 * t).sin();
            [r * t.cos(), r * t.sin(), 0.0]
        })
        .collect();
    // Clamped quintic knots: 0 and 1 six times, evenly between.
    let mults: Vec<&str> = (0..n - 4).map(|k| if k == 0 || k == n - 5 { "6" } else { "1" }).collect();
    let knots: Vec<String> = (0..n - 4).map(|k| real(f64::from(k) / f64::from(n - 5))).collect();
    let (mults, knots) = (mults.join(","), knots.join(","));
    let at = |p: V, z: f64| [p[0], p[1], z];
    let mut w = W::default();
    let curve = |w: &mut W, z: f64| {
        let pts: Vec<u64> = outline.iter().map(|p| w.pt(at(*p, z))).collect();
        w.add(format!("B_SPLINE_CURVE_WITH_KNOTS('',5,({}),.UNSPECIFIED.,.T.,.F.,({mults}),({knots}),.UNSPECIFIED.)", list(&pts)))
    };
    let (v0, v1) = (w.vertex(at(outline[0], 0.0)), w.vertex(at(outline[0], 5.0)));
    let (kb, kt) = (curve(&mut w, 0.0), curve(&mut w, 5.0));
    let bottom = w.add(format!("EDGE_CURVE('',#{v0},#{v0},#{kb},.T.)"));
    let top = w.add(format!("EDGE_CURVE('',#{v1},#{v1},#{kt},.T.)"));
    let seam = w.line_edge((v0, at(outline[0], 0.0)), (v1, at(outline[0], 5.0)), true, 5.0);
    let rows: Vec<String> = outline
        .iter()
        .map(|p| {
            let (a, b) = (w.pt(at(*p, 0.0)), w.pt(at(*p, 5.0)));
            format!("(#{a},#{b})")
        })
        .collect();
    let surface = w.add(format!(
        "B_SPLINE_SURFACE_WITH_KNOTS('',5,1,({}),.UNSPECIFIED.,.T.,.F.,.F.,({mults}),(2,2),({knots}),(0.,5.),.UNSPECIFIED.)",
        rows.join(",")
    ));
    // Along the outline (counter-clockwise about +z) and up: the side's normal points out. Its
    // loop runs round the bottom, up the seam, back round the top and down the seam.
    let uses = [w.oe(bottom, true), w.oe(seam, true), w.oe(top, false), w.oe(seam, false)];
    let bx = w.bound(&uses, true, true);
    let side = w.face(&[bx], surface, true);
    let plane_b = w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let x = w.oe(bottom, false);
    let bx = w.bound(&[x], true, true);
    let cap_b = w.face(&[bx], plane_b, false);
    let plane_t = w.plane([0.0, 0.0, 5.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let x = w.oe(top, true);
    let bx = w.bound(&[x], true, true);
    let cap_t = w.face(&[bx], plane_t, true);
    let shell = closed_shell(&mut w, &[side, cap_b, cap_t]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('wavy prism',#{shell})"));
    let s = one_solid(&w.file_with_uncertainty(&[solid], 0.01));
    assert_eq!(counts(&s.brep), [2, 1, 3, 3, 2]);
    assert!(!s.notes.iter().any(|n| n.contains("seam edges added")), "{:?}", s.notes);
    faces_away(&s.brep, 0, |p| [0.0, 0.0, p[2].clamp(1.0, 4.0)]);
}

/// A loop's samples each within the tolerance of two parts of a surface periodic in u: the one
/// the loop runs on, and another between 0.4 and 0.6 of a period away, nearer for the middle
/// half of the loop. Taking the nearer at each sample jumps about +0.45 of a period out and +0.45 back
/// (the offset now past half a period, wrapped the other way) and counts a turn the loop does
/// not make. The count keeps to the part the loop is on: no turn for a loop running along v,
/// one for a loop running once round u.
#[test]
fn a_loop_within_the_tolerance_of_two_parts_of_a_surface_keeps_its_turns() {
    let n = 20;
    let reach = 0.01;
    for (turns, expected) in [(0.0, [0, 0]), (1.0, [1, 0])] {
        let mut t = seams::Turns::new([Some(1.0), None]);
        for k in 0..=n {
            let s = f64::from(k) / f64::from(n);
            let on = [(turns * s).rem_euclid(1.0), s];
            let other = [(on[0] + 0.4 + 0.2 * s).rem_euclid(1.0), s];
            let nearer = (5..15).contains(&k);
            assert!(t.step(&[(on, 0.5 * reach), (other, if nearer { 0.4 } else { 0.6 } * reach)], reach));
        }
        t.close();
        assert_eq!(t.count(), expected, "{turns} turns");
    }
}

#[test]
fn pcurves_past_a_quarter_line_the_form_ends_on_are_put_on_the_form() {
    // A cylinder face from 2e-5 rad before the frame's x axis to 0.5 rad (its straight edge 0.2 um
    // off the quarter line, under the 1e-3 mm tolerance): the form starts on the quarter line,
    // and the edge's pcurve, on the angle the edge has, lies past the form's range.
    let r = 10.0;
    let label = BSurface::Cylinder { frame: Frame { origin: [1.0, 2.0, 3.0], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] }, radius: r };
    let (eps, tol) = (2e-5, 1e-3);
    let domain = [[-eps, 0.5], [0.0, 5.0]];
    let (f, d) = crate::kernel::form::surface_within(&label, domain, tol).unwrap();
    assert_eq!(d[0][0], 0.0, "the form starts on the quarter line: {d:?}");
    let past = crate::kernel::form::param(-eps);
    assert!(past < 0.0 && past > -eps, "{past}");
    let line = |u: f64| Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![[u, 0.0], [u, 5.0]] };
    // An arc along the bottom from the edge's end, as a rational segment (weights 1, 0.9, 1).
    let w = 0.9;
    let arc =
        Curve2::RationalSpline { knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], homogeneous: vec![[past, 0.0, 1.0], [0.1 * w, 0.0, w], [0.3, 0.0, 1.0]] };
    let far = crate::kernel::form::param(-0.01);
    let mut pcs = vec![vec![line(past), arc], vec![line(far)]];
    assert_eq!(onto_form(&f, &label, domain, tol, &mut pcs), 3, "the line's two ends and the arc's start");
    assert_eq!(pcs[0][0], line(0.0));
    match &pcs[0][1] {
        Curve2::RationalSpline { homogeneous, .. } => assert_eq!(homogeneous[0], [0.0, 0.0, 1.0]),
        c => panic!("{c:?}"),
    }
    // A point further out than the tolerance's angle is left for the check to refuse.
    assert_eq!(pcs[1][0], line(far));
    // The moved pcurve is on the form within the tolerance of where the edge is.
    let (on_form, on_label) = (SurfaceEval::new(&f).unwrap(), SurfaceEval::new(&label).unwrap());
    for v in [0.0, 2.5, 5.0] {
        let gap = on_form.point([0.0, v]).unwrap().dist(on_label.point([-eps, v]).unwrap());
        assert!(gap <= tol && (gap - r * eps).abs() < 1e-9, "{v}: {gap}");
    }
}

#[test]
fn pcurves_a_few_microns_past_the_height_a_cone_form_ends_at_are_put_on_the_form() {
    // A cone face of half angle 60 degrees over heights 1 to 4: its form ends where the domain
    // does. A line pcurve fitted 0.4 tol below the bottom (a height that moves the surface by
    // 0.8 tol along the cone's line) is put on it; one 0.6 tol above the top (1.2 tol) is not.
    let half_angle = std::f64::consts::FRAC_PI_3;
    let label = BSurface::Cone {
        frame: Frame { origin: [1.0, 2.0, 3.0], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] },
        radius_at_origin: 5.0,
        half_angle,
    };
    let tol = 1e-3;
    let domain = [[0.2, 1.0], [1.0, 4.0]];
    let (f, _) = crate::kernel::form::surface_within(&label, domain, tol).unwrap();
    let ranges = solidcore_ops::label::surface_natural(&f);
    assert!(matches!(ranges.get(1), Some(Natural::Range(a, b)) if *a == 1.0 && *b == 4.0), "{ranges:?}");
    let u = crate::kernel::form::param(0.5);
    let line = |v0: f64, v1: f64| Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![[u, v0], [u, v1]] };
    let mut pcs = vec![vec![line(1.0 - 0.4 * tol, 2.0)], vec![line(3.0, 4.0 + 0.6 * tol)]];
    assert_eq!(onto_form(&f, &label, domain, tol, &mut pcs), 1, "only the end within the tolerance moves");
    assert_eq!(pcs[0][0], line(1.0, 2.0));
    assert_eq!(pcs[1][0], line(3.0, 4.0 + 0.6 * tol));
    // The move is the surface's distance between the two heights, within the tolerance.
    let on = SurfaceEval::new(&f).unwrap();
    let gap = on.point([u, 1.0]).unwrap().dist(on.point([u, 1.0 - 0.4 * tol]).unwrap());
    assert!(gap <= tol && (gap - 0.8 * tol).abs() < 1e-9, "{gap}");
}

/// A kernel making the missing pcurves as SMLib makes some: each a straight line in (u, v)
/// between the true parameters of its edge's ends on the face's NURBS form, with no regard for
/// the form's range (a cylinder face; the angle unwrapped to the turn the face's domain is on).
struct Projected(std::sync::atomic::AtomicUsize);

impl Trims for Projected {
    fn trims(&self, face: &TrimFace<'_>) -> std::result::Result<Vec<Vec<Curve2>>, String> {
        let BSurface::Cylinder { frame, .. } = face.surface else { return Err("not a cylinder".into()) };
        let (o, x, y) = (Vec3::from(frame.origin), Vec3::from(frame.x_axis), Vec3::from(frame.y_axis));
        let z = x.cross(y);
        let centre = 0.5 * (face.domain[0][0] + face.domain[0][1]);
        let at = |p: Vec3| -> [f64; 2] {
            let d = p - o;
            // A turn is four quarters of form parameter.
            let mut a = d.dot(y).atan2(d.dot(x));
            while crate::kernel::form::param(a) < centre - 2.0 {
                a += TAU;
            }
            while crate::kernel::form::param(a) > centre + 2.0 {
                a -= TAU;
            }
            [crate::kernel::form::param(a), d.dot(z)]
        };
        let mut out = Vec::new();
        for l in &face.loops {
            let mut pcs = Vec::new();
            for u in l {
                if let Some(pc) = &u.pcurve {
                    pcs.push(pc.clone());
                    continue;
                }
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let e = face.edges.get(u.edge as usize).ok_or("edge")?;
                let eval = CurveEval::new(e.nurbs.unwrap_or(e.curve)).map_err(|m| m.to_string())?;
                let [t0, t1] = e.interval;
                // The one curved edge written here ([`Run::Bulge`]): u = 0.8 t (1 - t), v = 1 - t.
                if matches!(e.curve, BCurve::Spline { .. }) {
                    pcs.push(Curve2::Spline { knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], points: vec![[0.0, 1.0], [0.4, 0.5], [0.0, 0.0]] });
                    continue;
                }
                let (a, b) = (eval.point(t0).ok_or("start")?, eval.point(t1).ok_or("end")?);
                pcs.push(Curve2::Spline { knots: vec![t0, t0, t1, t1], points: vec![at(a), at(b)] });
            }
            out.push(pcs);
        }
        Ok(out)
    }
}

/// A quarter-cylinder wedge (radius 0.2 mm, height 5 mm, 0 to 100 degrees about +z) whose straight
/// edge runs from 1.9 um before the x axis to 1.9 um past it, 0.6 um outside the cylinder, in a
/// file of uncertainty 2e-3 mm: the face's extent ends within that of the x axis, so its form
/// starts there, while the skewed edge starts before it. Off the form cut short at the x axis by
/// more than half the tolerance, the edge takes its closed form on the form not cut short (its
/// kernel is not asked), and that pcurve is put on the form's range: every pcurve of the record
/// lies in its face's domain, as SMLib's check asks (`SmBrep::ValidatePointers` tests 70 and 71,
/// "Edge endpoints within face domain").
#[test]
fn a_skewed_edge_past_the_quarter_line_a_face_form_ends_on_stays_in_the_face_domain() {
    let (r, h, q): (f64, f64, f64) = (0.2, 5.0, 100f64.to_radians());
    // The edge 1.9 um either side of the x axis at the rim, and 0.6 um outside the cylinder: more
    // than half the tolerance off any line on the form cut short at the x axis.
    let (eps, rl) = (1.9e-3 / r, r + 0.6e-3);
    let mut w = W::default();
    let v = |w: &mut W, p: V| (w.vertex(p), p);
    let (p0, p1) = ([rl * eps.cos(), -rl * eps.sin(), 0.0], [rl * eps.cos(), rl * eps.sin(), h]);
    let nq = [-q.sin(), q.cos(), 0.0];
    // The axis edge: where the planes through the axis' foot, (p0, p1) and the 100 degree side
    // meet, up to the top.
    let np = Vec3::from(p0).cross(Vec3::from(p1));
    let line = np.cross(Vec3::from(nq));
    let a1 = [line.x * h / line.z, line.y * h / line.z, h];
    let (qb, qt) = ([r * q.cos(), r * q.sin(), 0.0], [r * q.cos(), r * q.sin(), h]);
    let (a0, a1, p0, p1, q0, q1) = (v(&mut w, [0.0; 3]), v(&mut w, a1), v(&mut w, p0), v(&mut w, p1), v(&mut w, qb), v(&mut w, qt));
    let edge = |w: &mut W, a: (u64, V), b: (u64, V)| w.line_edge(a, b, true, len3(sub3(b.1, a.1)));
    let (axis, e_p, e_q) = (edge(&mut w, a0, a1), edge(&mut w, p0, p1), edge(&mut w, q0, q1));
    let (r_pb, r_qb, r_pt, r_qt) = (edge(&mut w, a0, p0), edge(&mut w, a0, q0), edge(&mut w, a1, p1), edge(&mut w, a1, q1));
    let arc = |w: &mut W, a: (u64, V), b: (u64, V), z: f64| {
        let c = w.circle([0.0, 0.0, z], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], r);
        w.add(format!("EDGE_CURVE('',#{},#{},#{c},.T.)", a.0, b.0))
    };
    let (arc_b, arc_t) = (arc(&mut w, p0, q0, 0.0), arc(&mut w, p1, q1, h));
    let ax = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let cyl = w.add(format!("CYLINDRICAL_SURFACE('',#{ax},{})", real(r)));
    let face = |w: &mut W, uses: &[(u64, bool)], surface: u64, same_sense: bool| {
        let oes: Vec<u64> = uses.iter().map(|&(e, f)| w.oe(e, f)).collect();
        let b = w.bound(&oes, true, true);
        w.face(&[b], surface, same_sense)
    };
    let (bottom, top) = (w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]), w.plane([0.0, 0.0, h], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]));
    let side_q = w.plane([0.0; 3], nq, [q.cos(), q.sin(), 0.0]);
    let side_p = w.plane([0.0; 3], [np.x, np.y, np.z], p0.1);
    let faces = [
        face(&mut w, &[(e_q, true), (arc_t, false), (e_p, false), (arc_b, true)], cyl, true),
        face(&mut w, &[(r_qb, true), (arc_b, false), (r_pb, false)], bottom, false),
        face(&mut w, &[(r_pt, true), (arc_t, true), (r_qt, false)], top, true),
        face(&mut w, &[(axis, true), (r_qt, true), (e_q, false), (r_qb, false)], side_q, true),
        face(&mut w, &[(r_pb, true), (e_p, true), (r_pt, false), (axis, false)], side_p, true),
    ];
    let shell = closed_shell(&mut w, &faces);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('wedge',#{shell})"));
    let kernel = Projected(std::sync::atomic::AtomicUsize::new(0));
    let read = read_solids_with(&w.file_with_uncertainty(&[solid], 2e-3), &kernel);
    assert!(read.failures.is_empty(), "{:?}", read.failures);
    assert_eq!(kernel.0.load(std::sync::atomic::Ordering::Relaxed), 0, "the skewed edge's pcurve is its closed form");
    let b = &read.solids[0].brep;
    assert_eq!(counts(b), [2, 1, 5, 9, 6]);
    for (i, f) in b.faces.iter().enumerate() {
        let d = f.domain;
        for l in &f.loops {
            let Loop::Edges { edges } = l else { continue };
            for u in edges {
                let [t0, t1] = u.pcurve.interval().unwrap();
                for t in [t0, t1] {
                    let p = solidcore_ops::pcurve::eval(&u.pcurve, t).unwrap();
                    let out = (d[0][0] - p[0]).max(p[0] - d[0][1]).max(d[1][0] - p[1]).max(p[1] - d[1][1]);
                    assert!(out <= 1e-9, "face {i}, edge {}: {p:?} lies {out:e} outside the domain {d:?}", u.edge);
                }
            }
        }
    }
    solidcore_ops::record_check::validate(b).unwrap();
    solidcore_ops::record_check::agreement(b).unwrap();
}

/// A kernel making the missing pcurves of a torus face on its NURBS form: each a straight line
/// in (u, v) between the true form parameters of its edge's ends (a closed edge a turn round),
/// with no regard for the form's range, as SMLib's would be. It counts the uses it is asked for.
struct TubeLines(std::sync::atomic::AtomicUsize);

impl Trims for TubeLines {
    fn trims(&self, face: &TrimFace<'_>) -> std::result::Result<Vec<Vec<Curve2>>, String> {
        let BSurface::Torus { frame, major_radius, .. } = face.surface else { return Err("not a torus".into()) };
        let (o, x, y) = (Vec3::from(frame.origin), Vec3::from(frame.x_axis), Vec3::from(frame.y_axis));
        let z = x.cross(y);
        let at = |p: Vec3| -> [f64; 2] {
            let d = p - o;
            let (dx, dy, dz) = (d.dot(x), d.dot(y), d.dot(z));
            let tube = dz.atan2(dx.hypot(dy) - major_radius).rem_euclid(TAU);
            [crate::kernel::form::param(dy.atan2(dx).rem_euclid(TAU)), crate::kernel::form::param(tube)]
        };
        let mut out = Vec::new();
        for l in &face.loops {
            let mut pcs = Vec::new();
            for u in l {
                if let Some(pc) = &u.pcurve {
                    pcs.push(pc.clone());
                    continue;
                }
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let e = face.edges.get(u.edge as usize).ok_or("edge")?;
                let eval = CurveEval::new(e.nurbs.unwrap_or(e.curve)).map_err(|m| m.to_string())?;
                let [t0, t1] = e.interval;
                let (a, mut b) = (at(eval.point(t0).ok_or("start")?), at(eval.point(t1).ok_or("end")?));
                if e.vertices[0] == e.vertices[1] {
                    b[0] = a[0] + 4.0;
                }
                pcs.push(Curve2::Spline { knots: vec![t0, t0, t1, t1], points: vec![a, b] });
            }
            out.push(pcs);
        }
        Ok(out)
    }
}

/// A cup (a torus of radii 4 and 1 mm about +z): its wall a quarter of the tube from its top
/// circle (tube angle 90 degrees, radius 4 mm at z = 1 mm) round to a circle 2e-3 rad past its
/// inner equator, its seam a tube circle edge in the plane y = 0, closed by a disc on each
/// circle, in a file of uncertainty 0.01 mm. Every edge lies on the torus exactly. The wall's
/// boundary ends 2e-3 rad (2e-3 mm along the tube) past the quarter line, within the file's
/// uncertainty, so its form ends there. The bottom circle and the seam's end there lie
/// that far off the form: the gap is measured and covered by their pcurve tolerances, so the
/// two have their closed forms on the form (a parallel, and a line along the tube) and the kernel
/// is asked for none. Without that, at the fitting tolerance of 1e-3 mm, the circle had none and
/// the seam's closed form ran past the form's knot range, where its pole query on the rational
/// net was refused ("off the surface's range ... rational net").
#[test]
fn a_torus_face_running_a_hair_past_the_quarter_line_its_form_ends_on_keeps_its_closed_form_pcurves() {
    let (major, minor, past): (f64, f64, f64) = (4.0, 1.0, 2e-3);
    let mut w = W::default();
    let (rb, zb) = (major - minor * past.cos(), -minor * past.sin());
    let (v_bottom, v_top) = (w.vertex([rb, 0.0, zb]), w.vertex([major, 0.0, minor]));
    let ring = |w: &mut W, v: u64, r: f64, z: f64| {
        let k = w.circle([0.0, 0.0, z], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], r);
        w.add(format!("EDGE_CURVE('',#{v},#{v},#{k},.T.)"))
    };
    let (bottom, top) = (ring(&mut w, v_bottom, rb, zb), ring(&mut w, v_top, major, minor));
    // The seam: the tube circle at angle 0 about the axis, from the bottom vertex over the inner
    // equator to the top one (its own angle from pi - past to 3 pi / 2 about +y).
    let k = w.circle([major, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], minor);
    let seam = w.add(format!("EDGE_CURVE('',#{v_bottom},#{v_top},#{k},.T.)"));
    let a = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let torus = w.add(format!("TOROIDAL_SURFACE('',#{a},{},{})", real(major), real(minor)));
    let oes = [w.oe(bottom, true), w.oe(seam, true), w.oe(top, false), w.oe(seam, false)];
    let wall = w.bound(&oes, true, true);
    let wall = w.face(&[wall], torus, false);
    let disc = |w: &mut W, edge: u64, z: f64, up: bool| {
        let plane = w.plane([0.0, 0.0, z], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let oe = w.oe(edge, up);
        let b = w.bound(&[oe], true, true);
        w.face(&[b], plane, up)
    };
    let (floor, lid) = (disc(&mut w, bottom, zb, false), disc(&mut w, top, minor, true));
    let shell = closed_shell(&mut w, &[wall, floor, lid]);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('cup',#{shell})"));
    let kernel = TubeLines(std::sync::atomic::AtomicUsize::new(0));
    let read = read_solids_with(&w.file_with_uncertainty(&[solid], 1e-2), &kernel);
    assert!(read.failures.is_empty(), "{:?}", read.failures);
    assert_eq!(kernel.0.load(std::sync::atomic::Ordering::Relaxed), 0, "every pcurve of the wall is a closed form");
    let b = &read.solids[0].brep;
    assert_eq!(counts(b), [2, 1, 3, 3, 2]);
    // The wall's form ends on the quarter line at the inner equator (form parameter 2 along the
    // tube), and every pcurve lies in its face's domain.
    let wall = b.faces.iter().find(|f| f.loops.iter().any(|l| matches!(l, Loop::Edges { edges } if edges.len() == 4))).unwrap();
    assert!((wall.domain[1][1] - 2.0).abs() <= 1e-12, "{:?}", wall.domain);
    for (i, f) in b.faces.iter().enumerate() {
        let d = f.domain;
        for l in &f.loops {
            let Loop::Edges { edges } = l else { continue };
            for u in edges {
                let [t0, t1] = u.pcurve.interval().unwrap();
                for t in [t0, t1] {
                    let p = solidcore_ops::pcurve::eval(&u.pcurve, t).unwrap();
                    let out = (d[0][0] - p[0]).max(p[0] - d[0][1]).max(d[1][0] - p[1]).max(p[1] - d[1][1]);
                    assert!(out <= 1e-9, "face {i}, edge {}: {p:?} lies {out:e} outside the domain {d:?}", u.edge);
                }
            }
        }
    }
    solidcore_ops::record_check::validate(b).unwrap();
    solidcore_ops::record_check::agreement(b).unwrap();
    let (body, _) = solidcore_ops::body::Body::from_record(b, false).unwrap();
    body.check_solid().unwrap();
}

/// A wedge of a cylinder (radius 10 mm, 5 mm high) from -120 to 180 degrees about +z, in a file
/// of uncertainty 0.02 mm, whose corner at the top of its 180 degree side lies 0.008 mm outside
/// the cylinder, 1e-4 mm below the top and 1e-4 rad round past the side. The cylinder face is
/// its NURBS form, a turn from -180 degrees, and ends on the form's seam at that side; the drop
/// of the corner onto it stops on the face's bound and crosses the seam, a turn from the
/// corner's trims. The corner's point on the face is the one on the trims' side: the trims
/// closed onto it at the corner (the 1e-4 mm gap there) stay in the face's domain
/// (`SmBrep::ValidatePointers` tests 70 to 72, "Edge endpoints within face domain"), the corner
/// classifies on the face (`SmFace` test 12), and the kernel's check of the healed body finds
/// only the corner's gap off the cylinder.
#[test]
fn a_corner_off_a_face_where_it_ends_on_its_seam_is_on_the_face_and_its_trims_in_its_domain() {
    let (r, h): (f64, f64) = (10.0, 5.0);
    let (from, to) = ((-120f64).to_radians(), 180f64.to_radians());
    let (off, below, past) = (0.008, 1e-4, 1e-4);
    let mut w = W::default();
    let v = |w: &mut W, p: V| (w.vertex(p), p);
    let on = |a: f64, rr: f64, z: f64| [rr * a.cos(), rr * a.sin(), z];
    let (a0, a1) = (v(&mut w, [0.0; 3]), v(&mut w, [0.0, 0.0, h]));
    let (p0, p1) = (v(&mut w, on(from, r, 0.0)), v(&mut w, on(from, r, h)));
    let (q0, q1) = (v(&mut w, on(to, r, 0.0)), v(&mut w, on(to + past, r + off, h - below)));
    let edge = |w: &mut W, a: (u64, V), b: (u64, V)| w.line_edge(a, b, true, len3(sub3(b.1, a.1)));
    let (axis, e_p, e_q) = (edge(&mut w, a0, a1), edge(&mut w, p0, p1), edge(&mut w, q0, q1));
    let (r_pb, r_qb, r_pt, r_qt) = (edge(&mut w, a0, p0), edge(&mut w, a0, q0), edge(&mut w, a1, p1), edge(&mut w, a1, q1));
    let arc = |w: &mut W, a: (u64, V), b: (u64, V), z: f64| {
        let c = w.circle([0.0, 0.0, z], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], r);
        w.add(format!("EDGE_CURVE('',#{},#{},#{c},.T.)", a.0, b.0))
    };
    let (arc_b, arc_t) = (arc(&mut w, p0, q0, 0.0), arc(&mut w, p1, q1, h));
    let ax = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let cyl = w.add(format!("CYLINDRICAL_SURFACE('',#{ax},{})", real(r)));
    let face = |w: &mut W, uses: &[(u64, bool)], surface: u64, same_sense: bool| {
        let oes: Vec<u64> = uses.iter().map(|&(e, f)| w.oe(e, f)).collect();
        let b = w.bound(&oes, true, true);
        w.face(&[b], surface, same_sense)
    };
    let (bottom, top) = (w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]), w.plane([0.0, 0.0, h], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]));
    let side_q = w.plane([0.0; 3], [-to.sin(), to.cos(), 0.0], [to.cos(), to.sin(), 0.0]);
    let side_p = w.plane([0.0; 3], [from.sin(), -from.cos(), 0.0], [from.cos(), from.sin(), 0.0]);
    let faces = [
        face(&mut w, &[(e_q, true), (arc_t, false), (e_p, false), (arc_b, true)], cyl, true),
        face(&mut w, &[(r_qb, true), (arc_b, false), (r_pb, false)], bottom, false),
        face(&mut w, &[(r_pt, true), (arc_t, true), (r_qt, false)], top, true),
        face(&mut w, &[(axis, true), (r_qt, true), (e_q, false), (r_qb, false)], side_q, true),
        face(&mut w, &[(r_pb, true), (e_p, true), (r_pt, false), (axis, false)], side_p, true),
    ];
    let shell = closed_shell(&mut w, &faces);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('wedge',#{shell})"));
    let kernel = Projected(std::sync::atomic::AtomicUsize::new(0));
    let read = read_solids_with(&w.file_with_uncertainty(&[solid], 2e-2), &kernel);
    assert!(read.failures.is_empty(), "{:?}", read.failures);
    let b = &read.solids[0].brep;
    assert_eq!(counts(b), [2, 1, 5, 9, 6]);
    let (body, _) = solidcore_ops::body::Body::from_record(b, false).unwrap();
    let report = body.check(solidcore_ops::check::CheckLevel::All).unwrap();
    for line in report.text.lines() {
        assert!(line.contains("Bad Gap: Vertex/Face gap"), "{report}");
    }
    body.check_solid().unwrap();
}

/// A disc (radius 30 mm, 18 mm high) with a flat cut off its side, its cylinder face going 330
/// degrees about +z from 30.02 degrees to 360.02, in a file of uncertainty 0.02 mm (as Onshape
/// writes): the face ends 0.0105 mm past its frame's x axis, within the uncertainty, so its form
/// ends on that quarter line and its frame is not turned. Its two rim arcs, labelled on that
/// frame, run those 0.02 degrees into a fifth quarter of the grid. Each has a form that is its
/// arc (cut to it), not five whole quarters, a quarter past one turn, which the kernel refused
/// ("a conic's NURBS form does not run anticlockwise round it at most once"): the record goes
/// into the kernel, and the body has the disc's volume less the segment's.
#[test]
fn a_rim_arc_that_ends_within_the_tolerance_past_a_quarter_line_has_a_form_of_one_turn() {
    let (r, h): (f64, f64) = (30.0, 18.0);
    let (from, to) = (30.02_f64.to_radians(), 360.02_f64.to_radians());
    let on_rim = |a: f64, z: f64| [r * a.cos(), r * a.sin(), z];
    let mut w = W::default();
    let v = |w: &mut W, p: V| (w.vertex(p), p);
    let (a0, a1, b0, b1) = (v(&mut w, on_rim(from, 0.0)), v(&mut w, on_rim(from, h)), v(&mut w, on_rim(to, 0.0)), v(&mut w, on_rim(to, h)));
    let edge = |w: &mut W, a: (u64, V), b: (u64, V)| w.line_edge(a, b, true, len3(sub3(b.1, a.1)));
    let (up_a, up_b, chord_b, chord_t) = (edge(&mut w, a0, a1), edge(&mut w, b0, b1), edge(&mut w, b0, a0), edge(&mut w, a1, b1));
    let arc = |w: &mut W, a: (u64, V), b: (u64, V), z: f64| {
        let c = w.circle([0.0, 0.0, z], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], r);
        w.add(format!("EDGE_CURVE('',#{},#{},#{c},.T.)", a.0, b.0))
    };
    let (arc_b, arc_t) = (arc(&mut w, a0, b0, 0.0), arc(&mut w, a1, b1, h));
    let ax = w.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let cyl = w.add(format!("CYLINDRICAL_SURFACE('',#{ax},{})", real(r)));
    let face = |w: &mut W, uses: &[(u64, bool)], surface: u64, same_sense: bool| {
        let oes: Vec<u64> = uses.iter().map(|&(e, f)| w.oe(e, f)).collect();
        let b = w.bound(&oes, true, true);
        w.face(&[b], surface, same_sense)
    };
    let (bottom, top) = (w.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]), w.plane([0.0, 0.0, h], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]));
    // The flat: through the two rulings, its normal out along the middle of the cut.
    let middle = 0.5 * (from + to - TAU);
    let flat = w.plane(b0.1, [middle.cos(), middle.sin(), 0.0], [-middle.sin(), middle.cos(), 0.0]);
    let faces = [
        face(&mut w, &[(arc_b, true), (up_b, true), (arc_t, false), (up_a, false)], cyl, true),
        face(&mut w, &[(chord_b, true), (up_a, true), (chord_t, true), (up_b, false)], flat, true),
        face(&mut w, &[(arc_b, false), (chord_b, false)], bottom, false),
        face(&mut w, &[(arc_t, true), (chord_t, false)], top, true),
    ];
    let shell = closed_shell(&mut w, &faces);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('disc',#{shell})"));
    // The rulings at the face's end lie past its form's range: their pcurves are the kernel's.
    let kernel = Projected(std::sync::atomic::AtomicUsize::new(0));
    let read = read_solids_with(&w.file_with_uncertainty(&[solid], 2e-2), &kernel);
    assert!(read.failures.is_empty(), "{:?}", read.failures);
    let b = &read.solids[0].brep;
    assert_eq!(counts(b), [2, 1, 4, 6, 4]);
    let mut arcs = 0;
    for e in &b.edges {
        let (BCurve::Circle { .. }, Some(BCurve::RationalSpline { knots, .. })) = (&e.curve, &e.nurbs) else { continue };
        arcs += 1;
        let (first, last) = (knots[0], knots[knots.len() - 1]);
        assert_eq!([first, last], e.interval, "the form is the arc");
        let sweep = solidcore::eval::form::angle(last) - solidcore::eval::form::angle(first);
        assert!((sweep - (to - from)).abs() < 1e-12, "{sweep}");
    }
    assert_eq!(arcs, 2);
    let (body, _) = solidcore_ops::body::Body::from_record(b, false).unwrap();
    assert!(body.is_manifold_solid().unwrap());
    let cut = TAU - (to - from);
    let volume = h * r * r * (PI - 0.5 * (cut - cut.sin()));
    let mass = body.mass_properties(1e-6).unwrap();
    assert!((mass.volume - volume).abs() < 1e-5 * volume, "{} vs {volume}", mass.volume);
}

/// A spline face's pcurve a little past its patch's knot range is put on the range when that
/// moves the surface within its edge's tolerance, measured along the iso-line it runs on. The
/// RailCore V6 hotend's face #1310 is a whole bicubic patch whose edge along its first v row, of
/// tolerance 5.7e-4 mm, gets a closed-form pcurve from u = -4.1e-4 to 1.00097; its pole test
/// refused the end past the range ("OffRangeBeyondReach"), and the face was left out. Here the
/// row v = 0 is a straight line 0.3 mm long (the surface moves 0.3 mm per unit of u there), the
/// row v = 1 three times as long: bounded over the whole patch the move would exceed the
/// tolerance, along its own row it does not.
#[test]
fn a_pcurve_a_little_past_a_spline_patchs_knot_range_is_put_on_it() {
    let bezier = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let points: Vec<Vec<V>> = (0..4)
        .map(|i| {
            let x = f64::from(i) / 3.0;
            (0..4)
                .map(|j| {
                    let s = f64::from(j) / 3.0;
                    // The row's length grows from 0.3 at v = 0 to 0.9 at v = 1; a bulge inside.
                    let z = if (1..=2).contains(&i) && (1..=2).contains(&j) { 0.5 } else { 0.0 };
                    [0.3 * (1.0 + 2.0 * s) * x, 10.0 * s, z]
                })
                .collect()
        })
        .collect();
    let patch = BSurface::Spline { u_knots: bezier.clone(), v_knots: bezier, points };
    let row = |a: f64, b: f64| Curve2::Spline { knots: vec![-1.0, -1.0, 0.0, 0.0], points: vec![[a, 0.0], [b, 0.0]] };
    let (before, after) = (-4.1e-4, 1.0 + 9.7e-4);
    let tolerance = 5.7e-4;
    let mut pcs = vec![vec![row(after, before)]];
    assert_eq!(onto_knots(&patch, &[vec![tolerance]], &mut pcs), 1);
    assert_eq!(pcs[0][0], row(1.0, 0.0));
    // The same ends with a tolerance under 0.3 x 9.7e-4 mm stay where they are.
    let mut pcs = vec![vec![row(after, before)]];
    assert_eq!(onto_knots(&patch, &[vec![2e-4]], &mut pcs), 0);
    assert_eq!(pcs[0][0], row(after, before));
    // Measured along its own row the move is 0.3 x 9.7e-4 mm, within the tolerance (bounded
    // over the whole patch, where the row v = 1 moves 0.9 mm per unit of u, it would not be).
    let (_, moved) = SurfaceEval::new(&patch).unwrap().pcurve_onto_knots(&row(after, before), tolerance).unwrap().unwrap();
    assert!((0.3 * 9.7e-4..0.302 * 9.7e-4).contains(&moved), "{moved}");
    // A pcurve whose row also leaves the range across, where the surface moves 10 mm per unit of
    // v, is left as it is.
    let off = Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![[before, -1e-3], [0.5, -1e-3]] };
    let mut pcs = vec![vec![off.clone()]];
    assert_eq!(onto_knots(&patch, &[vec![tolerance]], &mut pcs), 0);
    assert_eq!(pcs[0][0], off);
}

/// A closed spline edge whose end vertex sits a few µm off the curve's seam (its start and end
/// point) projects a hair past the start: it is at the seam, and the edge runs to the curve's
/// end, not refused as crossing the seam.
#[test]
fn an_edge_to_a_vertex_a_hair_off_a_closed_splines_seam_ends_at_the_seam() {
    // A closed square of side 10 as a degree-1 spline, seam at the origin.
    let square = || BCurve::Spline {
        knots: vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
        points: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 0.0]],
    };
    let mid = [10.0, 5.0, 0.0];
    // 5 µm along the first side from the seam: within the 1e-3 mm tolerance of it.
    let off = [5e-6, 0.0, 0.0];
    let tol = 1e-3;
    let m = open_or_closed(square(), mid, off, true, false, tol).unwrap();
    assert!(m.with_step);
    assert!((m.interval[0] - 1.5).abs() < 1e-9 && m.interval[1] == 4.0, "{:?}", m.interval);
    // Starting there, against the curve's sense: from the seam's end round to the middle.
    let m = open_or_closed(square(), off, mid, false, false, tol).unwrap();
    assert!((m.interval[0] - 1.5).abs() < 1e-9 && m.interval[1] == 4.0, "{:?}", m.interval);
    // Farther off than the tolerance it is a vertex of the curve like any other: the edge from
    // the middle to it, the curve's way, crosses the seam, so the curve restarts at the middle
    // and the edge runs from there round to that vertex.
    let far = [5e-3, 0.0, 0.0];
    let m = open_or_closed(square(), mid, far, true, false, tol).unwrap();
    let eval = CurveEval::new(&m.curve).unwrap();
    let at = |t: f64, q: [f64; 3]| eval.point(t).unwrap().dist(v3(q)) < 1e-9;
    assert!(at(m.interval[0], mid) && at(m.interval[1], far), "{:?}", m.interval);
}

/// A closed edge (one vertex at both ends) on a closed spline whose vertex is half way round
/// the curve, not at its seam (a thread's end loop as some exporters write it): the edge is the
/// whole curve started at its vertex, so both its ends are the vertex. It was read from the
/// curve's own seam, its vertex 5 mm off both ends, and the face refused (#16930).
#[test]
fn a_closed_edge_whose_vertex_is_off_its_closed_splines_seam_starts_there() {
    let square = || BCurve::Spline {
        knots: vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
        points: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 0.0]],
    };
    let mid = [10.0, 5.0, 0.0];
    let m = open_or_closed(square(), mid, mid, true, true, 1e-3).unwrap();
    assert!((m.interval[1] - m.interval[0] - 4.0).abs() < 1e-12, "{:?}", m.interval);
    let eval = CurveEval::new(&m.curve).unwrap();
    for t in m.interval {
        assert!(eval.point(t).unwrap().dist(v3(mid)) < 1e-12, "{t}");
    }
    // Half way round from the vertex: the opposite side's middle, the curve's shape kept.
    assert!(eval.point(m.interval[0] + 2.0).unwrap().dist(v3([0.0, 5.0, 0.0])) < 1e-12);
    // A vertex at the seam keeps the curve as it is.
    let m = open_or_closed(square(), [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], true, true, 1e-3).unwrap();
    assert_eq!(m.interval, [0.0, 4.0]);
}

/// A spline surface whose first and last control columns lie within the tolerance of each other
/// is made closed (both become their mean, the surface moving by half the gap at most), so it is
/// periodic; one closed exactly, one whose ends are farther apart, or one with a weight that
/// differs at its ends is left as it is.
#[test]
fn a_spline_surface_whose_ends_meet_within_the_tolerance_is_closed() {
    // A square tube of side 10 and height 5: degree 1 round it (u) and along it (v).
    let tube = |gap: f64| BSurface::Spline {
        u_knots: vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        points: [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [gap, 0.0]].iter().map(|&[x, y]| vec![[x, y, 0.0], [x, y, 5.0]]).collect(),
    };
    let tol = 1e-3;
    let periodic = |s: &BSurface| SurfaceEval::new(s).unwrap().domain().period;
    assert_eq!(periodic(&tube(1e-5)), [None, None]);
    let closed = closed_within(&tube(1e-5), tol).expect("closed within the tolerance");
    assert_eq!(periodic(&closed), [Some(4.0), None]);
    let BSurface::Spline { points, .. } = &closed else { panic!("a spline") };
    assert_eq!(points[0], points[4]);
    assert!((points[0][0][0] - 5e-6).abs() < 1e-15, "the mean of the two columns");
    assert!(closed_within(&tube(0.0), tol).is_none(), "already closed");
    assert!(closed_within(&tube(1e-2), tol).is_none(), "farther apart than the tolerance");
    let BSurface::Spline { u_knots, v_knots, points } = tube(1e-5) else { panic!("a spline") };
    let mut homogeneous: Vec<Vec<[f64; 4]>> = points.iter().map(|r| r.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect()).collect();
    homogeneous[4] = homogeneous[4].iter().map(|h| [h[0] * 2.0, h[1] * 2.0, h[2] * 2.0, 2.0]).collect();
    assert!(closed_within(&BSurface::RationalSpline { u_knots, v_knots, homogeneous }, tol).is_none(), "weights differ at the ends");
}

/// A fan narrowing to a point at v = 1, degree 2 along u and 1 along v, whose last control
/// column is the apex `(0, 0, 10)` with the k-th of its three points moved by `off[k]` along x.
fn fan(off: [f64; 3]) -> BSurface {
    let base = [[-5.0, 0.0, 0.0], [0.0, -5.0, 0.0], [5.0, 0.0, 0.0]];
    BSurface::Spline {
        u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        points: base.iter().zip(off).map(|(&p, o)| vec![p, [o, 0.0, 10.0]]).collect(),
    }
}

/// A spline side whose control points all lie within the tolerance of their centroid, but are not
/// one point, is made an exact pole: its points become their centroid, and the surface moves by
/// at most the farthest any of them moved. A side already one point (or one to rounding), one
/// spread wider than the tolerance, one on knots not clamped at that end, and a surface no wider than ten tolerances
/// anywhere are left as they are. A rational side keeps its weights.
#[test]
fn a_spline_side_that_closes_to_a_point_within_the_tolerance_is_made_an_exact_pole() {
    let tol = 1e-3;
    let off = [2e-4, -1e-4, 5e-5];
    let near = fan(off);
    let (pole, sides) = poles_within(&near, tol, &[]).expect("a side within the tolerance of a point");
    assert_eq!(sides, 1);
    let (BSurface::Spline { points, .. }, BSurface::Spline { points: was, .. }) = (&pole, &near) else { panic!("splines") };
    let apex = [(off[0] + off[1] + off[2]) / 3.0, 0.0, 10.0];
    assert!(points.iter().all(|row| row[1] == apex), "{points:?}");
    assert!(points.iter().zip(was).all(|(row, was)| row[0] == was[0]), "the other side stays");
    // The farthest control point moved by |2e-4 - 5e-5| = 1.5e-4: the surface by no more.
    let farthest = off.iter().map(|o| (o - apex[0]).abs()).fold(0.0, f64::max);
    let (before, after) = (SurfaceEval::new(&near).unwrap(), SurfaceEval::new(&pole).unwrap());
    let mut moved = 0.0_f64;
    for i in 0..=20 {
        for j in 0..=20 {
            let uv = [f64::from(i) / 20.0, f64::from(j) / 20.0];
            moved = moved.max(before.point(uv).unwrap().dist(after.point(uv).unwrap()));
        }
    }
    assert!(moved <= farthest * (1.0 + 1e-12), "moved {moved}, farthest control point {farthest}");
    // One point to the rounding of the surface's evaluation (its control points are one exactly).
    let top: Vec<_> = (0..=20).map(|i| after.point([f64::from(i) / 20.0, 1.0]).unwrap()).collect();
    assert!(top.iter().all(|p| p.dist(Vec3::new(apex[0], apex[1], apex[2])) < 1e-12), "the side is one point: {top:?}");
    // With the face's vertex at the apex, the pole is the vertex, not the centroid: the side's
    // farthest point (2e-4 off it) is within the tolerance. A vertex farther from the side than
    // the tolerance does not move it there.
    let vertex = Vec3::new(0.0, 0.0, 10.0);
    let (at_vertex, _) = poles_within(&near, tol, &[Vec3::new(5.0, 0.0, 0.0), vertex]).expect("a pole at the vertex");
    let BSurface::Spline { points, .. } = &at_vertex else { panic!("a spline") };
    assert!(points.iter().all(|row| row[1] == [0.0, 0.0, 10.0]), "{points:?}");
    let (off_vertex, _) = poles_within(&near, tol, &[Vec3::new(0.0, 0.0, 10.01)]).expect("a pole at the centroid");
    let BSurface::Spline { points, .. } = &off_vertex else { panic!("a spline") };
    assert!(points.iter().all(|row| row[1] == apex), "{points:?}");

    assert!(poles_within(&fan([0.0; 3]), tol, &[]).is_none(), "already a pole");
    // One ulp apart (as a rational sphere's pole row reads back from a file): a pole already.
    let BSurface::Spline { u_knots, v_knots, mut points } = fan([0.0; 3]) else { panic!("a spline") };
    points[1][1][2] = f64::from_bits(10.0_f64.to_bits() + 1);
    assert!(poles_within(&BSurface::Spline { u_knots, v_knots, points }, tol, &[]).is_none(), "one to rounding");
    assert!(poles_within(&fan([2e-3, 0.0, 0.0]), tol, &[]).is_none(), "farther from a point than the tolerance");
    let BSurface::Spline { u_knots, points, .. } = fan(off) else { panic!("a spline") };
    let unclamped = BSurface::Spline { u_knots, v_knots: vec![0.0, 0.0, 1.0, 2.0], points };
    assert!(poles_within(&unclamped, tol, &[]).is_none(), "the last column is not the surface's side");
    let sliver = BSurface::Spline {
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![vec![[0.0, 0.0, 0.0], [0.0, 0.0, 10.0]], vec![[5e-3, 0.0, 0.0], [5e-4, 0.0, 10.0]]],
    };
    assert!(poles_within(&sliver, tol, &[]).is_none(), "a sliver is thin, not collapsed");

    let BSurface::Spline { u_knots, v_knots, points } = fan(off) else { panic!("a spline") };
    let weights = [1.0, 0.5, 2.0];
    let homogeneous = points.iter().zip(weights).map(|(row, w)| row.iter().map(|p| [p[0] * w, p[1] * w, p[2] * w, w]).collect()).collect();
    let (rational, _) = poles_within(&BSurface::RationalSpline { u_knots, v_knots, homogeneous }, tol, &[]).expect("a rational side");
    let BSurface::RationalSpline { homogeneous, .. } = rational else { panic!("rational") };
    for (row, w) in homogeneous.iter().zip(weights) {
        assert_eq!(row[1][3], w, "the weight stays");
        assert!((row[1][0] / w - apex[0]).abs() < 1e-18 && row[1][2] / w == 10.0, "{row:?}");
    }
}

/// A tetrahedron A B C D whose face A B D (on y = 0) is a degree 1 spline whose side at v = 1
/// should be the vertex D, but whose control point over A lies 2e-4 mm off it, in a file of
/// uncertainty 1e-3 mm. That side is made an exact pole when the face is read (noted), so the
/// kernel's check takes the solid: no boundary is only close to singular (its surface test 5,
/// "Bad Surf Pole Approximation", which every operation on the body ran into).
#[test]
fn a_spline_face_whose_side_closes_to_its_vertex_within_the_tolerance_reads_with_an_exact_pole() {
    let (a, bp, c, d) = ([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]);
    let mut w = W::default();
    let (va, vb, vc, vd) = (w.vertex(a), w.vertex(bp), w.vertex(c), w.vertex(d));
    let edge = |w: &mut W, (vp, p): (u64, V), (vq, q): (u64, V)| w.line_edge((vp, p), (vq, q), true, len3(sub3(q, p)));
    let ab = edge(&mut w, (va, a), (vb, bp));
    let ac = edge(&mut w, (va, a), (vc, c));
    let ad = edge(&mut w, (va, a), (vd, d));
    let bc = edge(&mut w, (vb, bp), (vc, c));
    let bd = edge(&mut w, (vb, bp), (vd, d));
    let cd = edge(&mut w, (vc, c), (vd, d));
    let face = |w: &mut W, uses: &[(u64, bool)], surface: u64| {
        let oes: Vec<u64> = uses.iter().map(|&(e, o)| w.oe(e, o)).collect();
        let bound = w.bound(&oes, true, true);
        w.face(&[bound], surface, true)
    };
    let z0 = w.plane(a, [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
    let x0 = w.plane(a, [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let r3 = 1.0 / 3.0_f64.sqrt();
    let slope = w.plane(bp, [r3, r3, r3], [-std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2, 0.0]);
    let (p00, p01, p10, p11) = (w.pt(a), w.pt([2e-4, 0.0, 10.0]), w.pt(bp), w.pt(d));
    let spline = w.add(format!(
        "B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#{p00},#{p01}),(#{p10},#{p11})),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.)"
    ));
    let faces = [
        face(&mut w, &[(ac, true), (bc, false), (ab, false)], z0),
        face(&mut w, &[(ad, true), (cd, false), (ac, false)], x0),
        face(&mut w, &[(bc, true), (cd, true), (bd, false)], slope),
        face(&mut w, &[(ab, true), (bd, true), (ad, false)], spline),
    ];
    let shell = closed_shell(&mut w, &faces);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('',#{shell})"));
    let s = one_solid(&w.file_with_uncertainty(&[solid], 1e-3));
    assert!(s.notes.iter().any(|n| n.contains("1 sides of spline surfaces that close to a point")), "{:?}", s.notes);
    let (body, _) = solidcore_ops::body::Body::from_record(&s.brep, false).unwrap();
    let check = body.check(solidcore_ops::CheckLevel::All).unwrap();
    assert!(!check.text.contains("Pole Approximation"), "{check}");
    assert!(check.is_valid(), "{check}");
    let volume = body.mass_properties(1e-6).unwrap().volume;
    assert!((volume - 1000.0 / 6.0).abs() < 1e-3, "{volume}");
}

/// The range a closed profile is built over for a face whose loops are walked: a loop round the
/// whole profile (a band round an extruded ellipse, its samples an eighth of a turn apart) takes
/// the whole turn from its first point; one along part of it the part it sweeps, however coarse
/// its samples.
#[test]
fn a_loop_round_a_closed_profile_takes_the_whole_turn() {
    let eighths: Vec<f64> = (0..=8).map(|k| f64::from(k) * TAU / 8.0).collect();
    match swept::span_for_tests(&eighths, &[], std::slice::from_ref(&eighths)) {
        swept::SpanForTests::Turn(start) => assert!(start.abs() < 1e-12, "{start}"),
        other => panic!("{other:?}"),
    }
    // Half a turn and back at another height (a half band): the half it sweeps.
    let half: Vec<f64> = (0..=4).map(|k| f64::from(k) * TAU / 8.0).chain((0..=4).rev().map(|k| f64::from(k) * TAU / 8.0)).collect();
    match swept::span_for_tests(&half, &[], std::slice::from_ref(&half)) {
        // From where the gap it leaves ends, a whole number of turns on.
        swept::SpanForTests::Open(inner) => {
            assert!(inner.0.rem_euclid(TAU) < 1e-12 && (inner.1 - inner.0 - 0.5 * TAU).abs() < 1e-12, "{inner:?}");
        }
        other => panic!("{other:?}"),
    }
}

/// Only end control lines that are the surface's ends are made one: on knots not clamped at its
/// ends the first and last control columns are not the surface's edges, and the surface stays
/// as it is however close they are.
#[test]
fn a_spline_surface_on_unclamped_knots_is_not_closed() {
    let points: Vec<Vec<V>> =
        [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [1e-5, 0.0]].iter().map(|&[x, y]| vec![[x, y, 0.0], [x, y, 5.0]]).collect();
    let unclamped = BSurface::Spline { u_knots: vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0], v_knots: vec![0.0, 0.0, 1.0, 1.0], points };
    assert!(closed_within(&unclamped, 1e-3).is_none());
}

/// A cubic along x whose third control point steps back `back` mm against the polygon's way
/// (and a little off it, `off` along y), its ends at x = 0 and 10.
fn wiggle(back: f64, off: f64) -> BCurve {
    BCurve::Spline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
        points: vec![[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [4.0 - back, off, 0.0], [7.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
    }
}

/// A control polygon whose step turns back on the one before it by no more than the tolerance
/// has that step's inner point moved onto its neighbour: the curve moves by at most the step, its
/// ends stay, and the polygon no longer turns back. A step back longer than the tolerance, a
/// polygon that does not turn back, and a step that turns less than the check's 170 degrees are
/// left as they are. A rational point keeps its weight.
#[test]
fn a_control_polygon_that_turns_back_within_the_tolerance_is_unfolded() {
    let tol = 1e-3;
    let near = wiggle(5e-4, 0.0);
    let (unfolded, moved) = unfolded_curve(&near, tol).expect("a step back within the tolerance");
    assert_eq!(moved, 1);
    let BCurve::Spline { points, .. } = &unfolded else { panic!("a spline") };
    assert_eq!(points[2], [4.0, 0.0, 0.0], "onto its neighbour");
    assert_eq!((points[0], points[4]), ([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]), "the ends stay");
    let (before, after) = (CurveEval::new(&near).unwrap(), CurveEval::new(&unfolded).unwrap());
    let moved_by = (0..=200).map(|k| f64::from(k) / 200.0).map(|t| before.point(t).unwrap().dist(after.point(t).unwrap())).fold(0.0, f64::max);
    assert!(moved_by <= 5e-4 * (1.0 + 1e-12), "moved {moved_by}");
    assert!(unfolded_curve(&unfolded, tol).is_none(), "nothing left to unfold");

    assert!(unfolded_curve(&wiggle(2e-3, 0.0), tol).is_none(), "farther back than the tolerance");
    assert!(unfolded_curve(&wiggle(-5e-4, 0.0), tol).is_none(), "no step back");
    // 5e-4 back and 2e-4 aside turns about 158 degrees: under the check's 170.
    assert!(unfolded_curve(&wiggle(5e-4, 2e-4), tol).is_none(), "turns less than 170 degrees");

    let BCurve::Spline { knots, points } = wiggle(5e-4, 0.0) else { panic!("a spline") };
    let w = [1.0, 0.5, 2.0, 1.0, 1.0];
    let homogeneous = points.iter().zip(w).map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, w]).collect();
    let (rational, _) = unfolded_curve(&BCurve::RationalSpline { knots, homogeneous }, tol).expect("a rational step back");
    let BCurve::RationalSpline { homogeneous, .. } = rational else { panic!("rational") };
    assert_eq!(homogeneous[2], [8.0, 0.0, 0.0, 2.0], "onto its neighbour, keeping its weight");
}

/// A spline surface's control rows and columns are unfolded as a curve's polygon is: a row that
/// steps back within the tolerance has that point moved onto its neighbour, the surface moving
/// by at most the step.
#[test]
fn a_control_net_that_turns_back_within_the_tolerance_is_unfolded() {
    let tol = 1e-3;
    let row = |y: f64, back: f64| vec![[0.0, y, 0.0], [4.0, y, 0.0], [4.0 - back, y, 0.0], [7.0, y, 0.0], [10.0, y, 0.0]];
    let net = BSurface::Spline {
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
        points: vec![row(0.0, 0.0), row(5.0, 5e-4)],
    };
    let (unfolded, moved) = unfolded_surface(&net, tol).expect("a row that steps back");
    assert_eq!(moved, 1);
    let BSurface::Spline { points, .. } = &unfolded else { panic!("a spline") };
    assert_eq!(points[1][2], [4.0, 5.0, 0.0]);
    assert_eq!(points[0], row(0.0, 0.0), "the other row stays");
    let (before, after) = (SurfaceEval::new(&net).unwrap(), SurfaceEval::new(&unfolded).unwrap());
    let mut moved_by = 0.0_f64;
    for i in 0..=10 {
        for j in 0..=100 {
            let uv = [f64::from(i) / 10.0, f64::from(j) / 100.0];
            moved_by = moved_by.max(before.point(uv).unwrap().dist(after.point(uv).unwrap()));
        }
    }
    assert!(moved_by <= 5e-4 * (1.0 + 1e-12), "moved {moved_by}");
    assert!(unfolded_surface(&unfolded, tol).is_none());
}

/// A polygon whose run back reaches its last point keeps that end: the run's points from the
/// end of the last step forward are moved onto it, each within the tolerance of where it was,
/// and nothing that turns back is left. Moved one point at a time, the run left a step back of
/// 0.8 µm behind (review of the first version).
#[test]
fn a_run_back_to_a_polygons_last_point_is_moved_onto_it() {
    let tol = 1e-3;
    let points = vec![[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [3.9996, 0.0, 0.0], [3.9992, 0.0, 0.0]];
    let curve = BCurve::Spline { knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], points: points.clone() };
    let (unfolded, moved) = unfolded_curve(&curve, tol).expect("a run back to the end");
    let BCurve::Spline { points: now, .. } = &unfolded else { panic!("a spline") };
    assert_eq!(now, &vec![[0.0, 0.0, 0.0], [3.9992, 0.0, 0.0], [3.9992, 0.0, 0.0], [3.9992, 0.0, 0.0]]);
    assert_eq!(moved, 2);
    assert!(now.iter().zip(&points).all(|(a, b)| (a[0] - b[0]).abs() <= tol), "{now:?}");
    assert!(unfolded_curve(&unfolded, tol).is_none(), "nothing left that turns back");
    // A run back farther than the tolerance from where its points were is left as it is.
    let far = BCurve::Spline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        points: vec![[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [3.9994, 0.0, 0.0], [3.9988, 0.0, 0.0], [3.9985, 0.0, 0.0]],
    };
    let BCurve::Spline { points: far_points, .. } = &far else { panic!("a spline") };
    if let Some((c, _)) = unfolded_curve(&far, tol) {
        let BCurve::Spline { points: now, .. } = &c else { panic!("a spline") };
        assert!(now.iter().zip(far_points).all(|(a, b)| (a[0] - b[0]).abs() <= tol), "{now:?}");
    }
}

/// A control point a row and its column both move stays within the tolerance of where the file
/// put it: every move is bounded against the original net. The rows' pass moves the centre point
/// to 3.9994 and the columns' pass would carry it on to 3.9988, 1.2 µm from 4.0 (review of the
/// first version).
#[test]
fn a_control_point_moved_by_its_row_and_its_column_stays_within_the_tolerance() {
    let tol = 1e-3;
    let points = vec![
        vec![[0.0, -1.0, 0.0], [3.9988, 0.0, 0.0], [5.0, -1.0, 0.0]],
        vec![[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [3.9994, 0.0, 0.0]],
        vec![[0.0, 1.0, 0.0], [3.9989, 0.0, 0.0], [5.0, 1.0, 0.0]],
    ];
    let bezier = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let net = BSurface::Spline { u_knots: bezier.clone(), v_knots: bezier, points: points.clone() };
    let (unfolded, _) = unfolded_surface(&net, tol).expect("a row that steps back to its end");
    let BSurface::Spline { points: now, .. } = &unfolded else { panic!("a spline") };
    for (row, was) in now.iter().zip(&points) {
        for (p, q) in row.iter().zip(was) {
            let d = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
            assert!(d <= tol, "a point moved {d}: {now:?}");
        }
    }
}

/// A cubic from x = 0 to 10 whose first control step runs `run` back along -x (and `aside` along
/// +y) before the polygon goes forward to x = 4, 7 and 10.
fn run_out(run: f64, aside: f64, second: f64) -> BCurve {
    BCurve::Spline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
        points: vec![[0.0, 0.0, 0.0], [-run, aside, 0.0], [second, 0.0, 0.0], [7.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
    }
}

/// A polygon that runs out of its first point the wrong way, by no more than the tolerance, and
/// that the next step turns back on past the first point keeps its first point and has the run
/// turned the way it goes: the point mirrored through the first point. The curve keeps its speed
/// at its start, now pointing the way the curve goes, moves by at most twice the run, and nothing
/// that turns back is left. A run whose mirror image lies farther than the tolerance from it, a
/// step back that does not carry the polygon past its first point by more than the run, and a
/// polygon that does not turn back are left as they are. A rational point keeps its weight.
#[test]
fn a_run_out_of_a_polygons_first_point_that_the_next_step_turns_back_past_is_turned_the_way_it_goes() {
    let tol = 1e-3;
    let near = run_out(2e-4, 1e-9, 4.0);
    let (unfolded, moved) = unfolded_curve(&near, tol).expect("a run out within the tolerance");
    assert_eq!(moved, 1);
    let BCurve::Spline { points, .. } = &unfolded else { panic!("a spline") };
    assert_eq!(points[1], [2e-4, -1e-9, 0.0], "its mirror image through the first point");
    assert_eq!((points[0], points[4]), ([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]), "the ends stay");
    let (before, after) = (CurveEval::new(&near).unwrap(), CurveEval::new(&unfolded).unwrap());
    let moved_by = (0..=200).map(|k| f64::from(k) / 200.0).map(|t| before.point(t).unwrap().dist(after.point(t).unwrap())).fold(0.0, f64::max);
    assert!(moved_by <= 4e-4 * (1.0 + 1e-9), "moved {moved_by}");
    let ((_, d0), (_, d1)) = (before.derivs(0.0).unwrap(), after.derivs(0.0).unwrap());
    let origin = Vec3::new(0.0, 0.0, 0.0);
    assert!((d0.dist(origin) - d1.dist(origin)).abs() <= 1e-12 * d0.dist(origin), "the same speed at the start");
    assert!(d0.x < 0.0 && d1.x > 0.0, "now leaving the way the curve goes: {d0:?} {d1:?}");
    assert!(unfolded_curve(&unfolded, tol).is_none(), "nothing left that turns back");

    assert!(unfolded_curve(&run_out(6e-4, 0.0, 4.0), tol).is_none(), "its mirror image farther than the tolerance");
    // 2e-4 out, then back to 1e-4 past the first point: no farther than the run.
    assert!(unfolded_curve(&run_out(2e-4, 0.0, 1e-4), tol).is_none(), "not carried past the first point by more than the run");
    assert!(unfolded_curve(&run_out(-2e-4, 0.0, 4.0), tol).is_none(), "no step back");

    let BCurve::Spline { knots, points } = run_out(2e-4, 0.0, 4.0) else { panic!("a spline") };
    let w = [1.0, 0.5, 2.0, 1.0, 1.0];
    let homogeneous = points.iter().zip(w).map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, w]).collect();
    let (rational, _) = unfolded_curve(&BCurve::RationalSpline { knots, homogeneous }, tol).expect("a rational run out");
    let BCurve::RationalSpline { homogeneous, .. } = rational else { panic!("rational") };
    assert_eq!(homogeneous[1], [1e-4, 0.0, 0.0, 0.5], "mirrored, keeping its weight");
}

/// A 10 × 20 × 30 mm block, in a file of uncertainty 0.01 mm, whose edge from (0, 0, 30) to
/// (0, 20, 30) is a cubic spline that first runs 1 um the wrong way (and 1e-9 mm down) before
/// going on to its end, all inside its first Bezier piece: it leaves its start vertex, turns back
/// within a micron and passes the vertex again. Its pcurves on its two plane faces are its
/// control polygon's images, so they turn back too, and the kernel's curve check failed the edge
/// curve and both pcurves on a radius of curvature of 1e-16 (BSplineCurve tests 13 and 14,
/// "Extremely small radius of curvature") and a speed of 1e-5 at the turn (tests 11 and 12).
/// Read, the run out is turned the way the edge goes, and every curve passes.
#[test]
fn a_spline_edge_that_runs_out_of_its_start_vertex_the_wrong_way_reads_without_a_turn_back() {
    use solidcore::curve::curve::CurveKind;
    use solidcore::geom::assert_array::{AssertArray, ObjectKey};
    use solidcore::topology::edgeuse::p1 as eu1;
    let mut w = W::default();
    let faces = box_faces(&mut w, [0.0; 3], [10.0, 20.0, 30.0], BoxStyle::default(), &mut |_, _| Vec::new());
    // The first edge box_faces writes is that from corner 4 to corner 6, on its first LINE.
    let (a, z) = ([0.0, 0.0, 30.0], [0.0, 20.0, 30.0]);
    let line = w.s.lines().find(|l| l.contains("=LINE(")).map(str::to_string).unwrap();
    let id = line.split_once('=').unwrap().0.to_string();
    // Two Bezier pieces along the edge: the first, 1e-4 of the parameter, holds the run out and
    // the turn back.
    let polygon = [a, [0.0, -1e-3, 30.0 - 1e-9], [0.0, 2e-3, 30.0], [0.0, 4e-3, 30.0], [0.0, 8.0, 30.0], [0.0, 14.0, 30.0], z];
    let ids: Vec<u64> = polygon.iter().map(|&p| w.pt(p)).collect();
    let spline = format!("{id}=B_SPLINE_CURVE_WITH_KNOTS('',3,({}),.UNSPECIFIED.,.F.,.F.,(4,3,4),(0.,1.E-4,1.),.UNSPECIFIED.);", list(&ids));
    w.s = w.s.replace(&line, &spline);
    let shell = closed_shell(&mut w, &faces);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('block',#{shell})"));
    let read = read_solids(&w.file_with_uncertainty(&[solid], 1e-2));
    assert!(read.failures.is_empty(), "{:?}", read.failures);
    let s = &read.solids[0];
    assert_eq!(counts(&s.brep), [2, 1, 6, 12, 8]);
    let (body, _) = solidcore_ops::body::Body::from_record(&s.brep, false).unwrap();
    // The kernel's check of each spline: the edge curves, and the pcurves on their faces' surfaces.
    let k = solidcore::kernel::Kernel::default();
    let mut failed = Vec::new();
    {
        let b = body.kernel();
        let mut check = |c: &solidcore::curve::curve::Curve, on: Option<&solidcore::surface::surface::Surface>| {
            let CurveKind::BSpline(x) = c.kind() else { return };
            let mut list = AssertArray::new();
            x.assert_valid_on(&k, Some(&mut list), false, ObjectKey(1), on).unwrap();
            failed.extend(list.reports().iter().filter(|r| !r.ok).map(|r| r.format_log_message()));
        };
        for (_, e) in b.edges().iter() {
            check(e.curve.as_ref().unwrap(), None);
        }
        for (eu, _) in b.edgeuses().iter() {
            let (Some(trim), _) = eu1::uv_trim_curve(&b, eu).unwrap() else { continue };
            let f = eu1::face(&b, eu).unwrap().unwrap();
            check(trim, b.face(f).unwrap().surface.as_ref());
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
    assert_eq!(body.check_solid(), Ok(()));
    let mass = body.mass_properties(1e-6).unwrap();
    assert!((mass.volume - 6000.0).abs() < 1e-6 * 6000.0, "{}", mass.volume);
    assert!(s.notes.iter().any(|n| n.starts_with("1 control points of spline curves")), "{:?}", s.notes);
    let edge = s.brep.edges.iter().find(|e| matches!(e.curve, BCurve::Spline { .. })).unwrap();
    let BCurve::Spline { points, .. } = &edge.curve else { panic!("a spline") };
    let mirrored = [0.0, 1e-3, 30.0 + 1e-9];
    assert!(points[1].iter().zip(mirrored).all(|(p, q)| (p - q).abs() <= 1e-12), "its run out mirrored through its start: {:?}", points[1]);
}

/// A rational surface's end lines are made one only while the surface moves by at most the
/// tolerance: half the gap, and half the weights' difference times the control net's size over
/// its least weight. Far from the origin (1e6 mm) as near it: a weight difference just inside
/// that bound is closed, and the surface then lies within the tolerance of where it was; one just
/// outside is left as it is.
#[test]
fn a_rational_surface_is_closed_only_while_its_weights_move_it_within_the_tolerance() {
    let (tol, gap, far) = (1e-3, 1e-5, 1e6);
    let corners = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [gap, 0.0]];
    let size = (10.0_f64 * 10.0 + 10.0 * 10.0 + 5.0 * 5.0).sqrt();
    // 0.5 gap + 0.5 δ size = tol.
    let bound = (2.0 * tol - gap) / size;
    let tube = |delta: f64| {
        let homogeneous = corners
            .iter()
            .enumerate()
            .map(|(i, &[x, y])| {
                let w = if i == 4 { 1.0 + delta } else { 1.0 };
                vec![[(far + x) * w, (far + y) * w, 0.0, w], [(far + x) * w, (far + y) * w, 5.0 * w, w]]
            })
            .collect();
        BSurface::RationalSpline { u_knots: vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0], v_knots: vec![0.0, 0.0, 1.0, 1.0], homogeneous }
    };
    let inside = tube(0.99 * bound);
    let closed = closed_within(&inside, tol).expect("inside the bound");
    let (before, after) = (SurfaceEval::new(&inside).unwrap(), SurfaceEval::new(&closed).unwrap());
    let mut moved = 0.0_f64;
    for i in 0..=40 {
        for j in 0..=4 {
            let uv = [4.0 * f64::from(i) / 40.0, f64::from(j) / 4.0];
            moved = moved.max(before.point(uv).unwrap().dist(after.point(uv).unwrap()));
        }
    }
    assert!(moved <= tol, "moved {moved}");
    assert!(closed_within(&tube(1.01 * bound), tol).is_none(), "outside the bound");
}

/// An edge a few µm long across a closed spline's seam, both its vertices within the tolerance of
/// the seam, is refused: taking both its ends to the seam would make it the whole curve.
#[test]
fn a_short_edge_across_a_closed_splines_seam_is_refused_not_made_the_whole_curve() {
    // A closed square of side 10, its last side split 1 µm before the seam (a short last knot
    // span, so a point there projects onto it rather than onto the seam's start).
    let square = || BCurve::Spline {
        knots: vec![0.0, 0.0, 1.0, 2.0, 3.0, 3.9999, 4.0, 4.0],
        points: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0], [0.0, 1e-3, 0.0], [0.0, 0.0, 0.0]],
    };
    // From 0.5 µm before the seam to 0.5 µm after it, the curve's way: across the seam.
    match open_or_closed(square(), [0.0, 5e-4, 0.0], [5e-4, 0.0, 0.0], true, false, 1e-3) {
        Ok(m) => panic!("{:?}", m.interval),
        Err(e) => assert!(e.contains("short edge across a closed spline's seam"), "{e}"),
    }
}

/// Arcs whose last one reaches past a turn from the first start cover the start of the sweep:
/// a gap the sweep found there before is covered (a near-full band's coarse samples, one walk
/// round most of the profile and past its start, another just after the start).
#[test]
fn a_walk_past_the_start_covers_the_gap_found_before_it() {
    let short = vec![0.0, 0.5, 1.0, 0.5, 0.0];
    // From 1.5 round to a turn and 1.2 and back, in steps under half a turn.
    let n = 8;
    let out: Vec<f64> = (0..=n).map(|k| 1.5 + (TAU - 0.3) * f64::from(k) / f64::from(n)).collect();
    let long: Vec<f64> = out.iter().chain(out.iter().rev()).copied().collect();
    match swept::span_for_tests(&[], &[], &[short, long]) {
        // The only gap is (1.2, 1.5).
        swept::SpanForTests::Open(inner) => {
            assert!((inner.0.rem_euclid(TAU) - 1.5).abs() < 1e-12 && (inner.1 - inner.0 - (TAU - 0.3)).abs() < 1e-12, "{inner:?}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn restarted_curve_errors_keep_their_step_messages() {
    let line = BCurve::Line { start: [0.; 3], end: [1.; 3] };
    assert_eq!(crate::kernel::restart_curve(&line, 0.5).unwrap_err(), "only splines and polylines can be restarted");
    let closed = BCurve::Polyline { points: vec![[0.; 3], [1.; 3], [0.; 3]] };
    assert_eq!(crate::kernel::restart_curve(&closed, 0.).unwrap_err(), "spline restart outside the open domain");
    let open = BCurve::Polyline { points: vec![[0.; 3], [1.; 3], [2.; 3]] };
    assert_eq!(crate::kernel::restart_curve(&open, 0.5).unwrap_err(), "spline seam endpoints do not coincide");
}

/// The unit box's top is the spline S(u,v) = (u, v + 0.016 u(1-u), 1). Its
/// horizontal edges have quadratic pcurves v = y - 0.016 u(1-u). A straight pcurve
/// between their ends misses the edge by 0.004 mm, inside half a 0.01 mm source uncertainty
/// but outside half the importer's default precision. Supply the exact quadratic when asked.
struct ParabolicTrims(std::sync::atomic::AtomicUsize);

impl Trims for ParabolicTrims {
    fn trims(&self, face: &TrimFace<'_>) -> std::result::Result<Vec<Vec<Curve2>>, String> {
        assert!(face.tolerance <= DEFAULT_TOLERANCE);
        let mut out = Vec::new();
        for l in &face.loops {
            let mut pcs = Vec::new();
            for u in l {
                if let Some(pc) = &u.pcurve {
                    pcs.push(pc.clone());
                    continue;
                }
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let e = &face.edges[u.edge as usize];
                assert!(e.tolerance <= DEFAULT_TOLERANCE);
                let curve = CurveEval::new(e.nurbs.unwrap_or(e.curve)).unwrap();
                let [a, z] = e.interval;
                let inverse = |t| {
                    let p = curve.point(t).unwrap();
                    [p.x, p.y - 0.016 * p.x * (1.0 - p.x)]
                };
                let (p, m, q) = (inverse(a), inverse(0.5 * (a + z)), inverse(z));
                // A quadratic Bezier's middle coefficient is 2 C(1/2) - (C(0)+C(1))/2.
                pcs.push(Curve2::Spline { knots: vec![a, a, a, z, z, z], points: vec![p, [0, 1].map(|d| 2.0 * m[d] - 0.5 * (p[d] + q[d])), q] });
            }
            out.push(pcs);
        }
        Ok(out)
    }
}

#[test]
fn a_spline_face_imported_with_coarse_distance_accuracy_keeps_its_boundary() {
    for uncertainty in [0.01, 0.005] {
        let mut w = W::default();
        let faces = box_faces(&mut w, [0.0; 3], [1.0; 3], BoxStyle::default(), &mut |_, _| Vec::new());
        let top = w.s.lines().rfind(|l| l.contains("=PLANE(")).unwrap().to_string();
        let id = top.split_once('=').unwrap().0;
        let rows: Vec<String> = [(0.0, 0.0), (0.5, 0.008), (1.0, 0.0)]
            .into_iter()
            .map(|(x, bump)| {
                let (a, z) = (w.pt([x, -0.016 + bump, 1.0]), w.pt([x, 1.0 + bump, 1.0]));
                format!("(#{a},#{z})")
            })
            .collect();
        let spline = format!(
            "{id}=B_SPLINE_SURFACE_WITH_KNOTS('',2,1,({}),.UNSPECIFIED.,.F.,.F.,.F.,(3,3),(2,2),(0.,1.),(-0.016,1.),.UNSPECIFIED.);",
            rows.join(",")
        );
        w.s = w.s.replace(&top, &spline);
        let shell = closed_shell(&mut w, &faces);
        let solid = w.add(format!("MANIFOLD_SOLID_BREP('spline box',#{shell})"));
        let trims = ParabolicTrims(std::sync::atomic::AtomicUsize::new(0));
        let read = read_solids_with(&w.file_with_uncertainty(&[solid], uncertainty), &trims);
        assert!(read.failures.is_empty(), "{:?}", read.failures);
        assert_eq!(read.solids.len(), 1);
        let s = &read.solids[0];
        assert!(!s.sheet);
        assert_eq!(counts(&s.brep), [2, 1, 6, 12, 8]);
        assert_eq!(s.brep.tolerance, uncertainty, "source tolerance is retained for accepting data");
        assert!(trims.0.load(std::sync::atomic::Ordering::Relaxed) >= 2, "the horizontal boundaries need quadratic pcurves");
        let f = s.brep.faces.iter().find(|f| matches!(f.surface, BSurface::Spline { .. })).unwrap();
        let surface = SurfaceEval::new(f.parameters()).unwrap();
        for l in &f.loops {
            let Loop::Edges { edges } = l else { panic!("edge loop") };
            for u in edges {
                let e = &s.brep.edges[u.edge as usize];
                let curve = CurveEval::new(e.parameters()).unwrap();
                for k in 0..=16 {
                    let t = e.interval[0] + (e.interval[1] - e.interval[0]) * f64::from(k) / 16.0;
                    let q = solidcore_ops::pcurve::eval(&u.pcurve, t).unwrap();
                    let gap = surface.point(q).unwrap().dist(curve.point(t).unwrap());
                    assert!(gap < 1e-9, "accuracy {uncertainty}: the exact boundary lifts with gap {gap}");
                }
            }
        }
    }
}

/// A 20 × 30 × 10 mm block (mm, uncertainty 1e-6 mm) whose 12 edges are straight cubic Béziers
/// on the knots (0, `span`), as Siemens NX writes them (`span` 1.020781457822e-7), and whose edge
/// from (0, 0, 0) to (20, 0, 0) is split `split` mm from its start: a micro edge and the rest,
/// both on that edge's curve, in its place in both its faces' loops.
fn bezier_block(span: f64, split: f64) -> p21::Exchange {
    let mut w = W::default();
    let size = [20.0, 30.0, 10.0];
    let corner = |i: usize| -> V {
        let c = [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0], [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]][i];
        [f64::from(c[0]) * size[0], f64::from(c[1]) * size[1], f64::from(c[2]) * size[2]]
    };
    let vs: Vec<u64> = (0..8).map(|i| w.vertex(corner(i))).collect();
    let cut = w.vertex([split, 0.0, 0.0]);
    let pairs = [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)];
    let mut edges = Vec::new();
    for (k, &(a, z)) in pairs.iter().enumerate() {
        let (p, q) = (corner(a), corner(z));
        let pts: Vec<u64> = (0..4)
            .map(|j| {
                let t = f64::from(j) / 3.0;
                w.pt([p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1]), p[2] + t * (q[2] - p[2])])
            })
            .collect();
        let c = w.add(format!(
            "B_SPLINE_CURVE_WITH_KNOTS('',3,({}),.UNSPECIFIED.,.F.,.F.,(4,4),(0.,{}),.PIECEWISE_BEZIER_KNOTS.)",
            list(&pts),
            real(span)
        ));
        if k == 0 {
            edges.push(w.add(format!("EDGE_CURVE('',#{},#{cut},#{c},.T.)", vs[0])));
            let rest = w.add(format!("EDGE_CURVE('',#{cut},#{},#{c},.T.)", vs[1]));
            edges.push(rest);
        } else {
            edges.push(w.add(format!("EDGE_CURVE('',#{},#{},#{c},.T.)", vs[a], vs[z])));
        }
    }
    // `edges[0]` is the micro edge and `edges[1]` the rest of that edge; the others follow.
    let e = |k: usize| if k == 0 { 0 } else { k + 1 };
    // Loops counter-clockwise seen from outside: (edge index into `pairs`, or 12 for the rest of
    // edge 0; forward), then the plane's origin and normal.
    let faces: [(&[(usize, bool)], V, V); 6] = [
        (&[(3, false), (2, false), (1, false), (12, false), (0, false)], [0.0; 3], [0.0, 0.0, -1.0]),
        (&[(4, true), (5, true), (6, true), (7, true)], [0.0, 0.0, 10.0], [0.0, 0.0, 1.0]),
        (&[(0, true), (12, true), (9, true), (4, false), (8, false)], [0.0; 3], [0.0, -1.0, 0.0]),
        (&[(1, true), (10, true), (5, false), (9, false)], [20.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        (&[(2, true), (11, true), (6, false), (10, false)], [20.0, 30.0, 0.0], [0.0, 1.0, 0.0]),
        (&[(3, true), (8, true), (7, false), (11, false)], [0.0, 30.0, 0.0], [-1.0, 0.0, 0.0]),
    ];
    let mut ids = Vec::new();
    for (lp, o, n) in faces {
        let oes: Vec<u64> = lp.iter().map(|&(k, fwd)| w.oe(edges[if k == 12 { 1 } else { e(k) }], fwd)).collect();
        let bound = w.bound(&oes, true, true);
        let x = if n[0] == 0.0 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let plane = w.plane(o, n, x);
        ids.push(w.face(&[bound], plane, true));
    }
    let shell = closed_shell(&mut w, &ids);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('block',#{shell})"));
    w.file_with_uncertainty(&[solid], 1e-6)
}

/// A micro edge cut from the start of a straight cubic Bézier whose knots span 1.02e-7 (Siemens
/// NX writes both) lies on [0, 6.5e-15] of the file's parameter at 1.283e-6 mm: no more than the
/// kernel's knot resolution (1e-12), so its trims' end knots were one and the kernel's check
/// failed the block (and with a 1.26e-3 mm micro edge, [0, 6.4e-12], only 6.4 times it). Such a
/// curve's parameter is rescaled where it is read, exactly, and every one of these blocks is the
/// closed 6000 mm³ block the kernel's check passes, as with the knots (0, 20).
#[test]
fn a_micro_edge_on_a_bezier_with_a_tiny_knot_span_reads_as_the_block() {
    for span in [20.0, 1.020781457822e-7] {
        for split in [1.283e-6, 1.26e-3] {
            let case = format!("knots (0, {span}), micro edge {split} mm");
            let s = one_solid(&bezier_block(span, split));
            assert!(!s.sheet, "{case}");
            // The record as read, without the kernel's healer (which takes the split vertex off
            // the straight edge): the kernel's check of the reader's own trims.
            let (body, _) = solidcore_ops::body::Body::from_record(&s.brep, true).unwrap_or_else(|e| panic!("{case}: {e}"));
            assert!(body.is_manifold_solid().unwrap(), "{case}");
            let report = body.check(solidcore_ops::check::CheckLevel::All).unwrap();
            assert!(report.is_valid(), "{case}: {report}");
            assert_eq!(body.counts().unwrap(), [6, 13, 9], "{case}");
            let mass = body.mass_properties(1e-9).unwrap();
            assert!((mass.volume - 6000.0).abs() <= 1e-9 * 6000.0, "{case}: volume {}", mass.volume);
            // Each edge's curve is its cubic with the file's control points, on the file's knots
            // (0, 20) as written, and on (0, 1.02e-7) scaled by the least power of two at or above
            // the curve's speed, its length L over the span (3 |P_{i+1} − P_i| / span, exact
            // thirds); every edge then spans more than the knot resolution.
            for e in &s.brep.edges {
                let solidcore_record::brep::Curve::Spline { knots, points } = &e.curve else { panic!("{case}: {:?}", e.curve) };
                let (Some(p0), Some(p3)) = (points.first(), points.last()) else { panic!("{case}") };
                let length = ((p3[0] - p0[0]).powi(2) + (p3[1] - p0[1]).powi(2) + (p3[2] - p0[2]).powi(2)).sqrt();
                let end = *knots.last().unwrap();
                assert_eq!(&knots[..4], &[0.0; 4], "{case}");
                assert!(knots[4..].iter().all(|k| *k == end), "{case}: {knots:?}");
                if span == 20.0 {
                    assert_eq!(end, 20.0, "{case}");
                } else {
                    let scale = end / span;
                    assert_eq!(scale, 2f64.powi(scale.log2().round() as i32), "{case}: {knots:?}");
                    let speed = length / span;
                    assert!(scale >= speed && scale < 2.0 * speed * (1.0 + 1e-12), "{case}: {scale} for {speed}");
                }
                assert!(e.interval[1] - e.interval[0] > 1e-12, "{case}: {:?}", e.interval);
            }
        }
    }
}

#[test]
fn a_small_cylinder_imported_with_coarse_distance_accuracy_keeps_its_cylinder() {
    for uncertainty in [0.01, 0.005] {
        let mut w = W::default();
        let (r, h) = (0.05, 0.1);
        let c = cylinder(&mut w, [0.0; 2], r, [0.0, h]);
        let wall = side(&mut w, &c, true);
        let cap = |w: &mut W, k: usize| {
            let plane = w.plane([0.0, 0.0, k as f64 * h], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
            let oe = w.oe(c.circles[k], k == 1);
            let bound = w.bound(&[oe], true, true);
            w.face(&[bound], plane, k == 1)
        };
        let (bottom, top) = (cap(&mut w, 0), cap(&mut w, 1));
        let shell = closed_shell(&mut w, &[bottom, wall, top]);
        let solid = w.add(format!("MANIFOLD_SOLID_BREP('small cylinder',#{shell})"));
        let s = one_solid(&w.file_with_uncertainty(&[solid], uncertainty));
        assert!(!s.sheet);
        assert_eq!(counts(&s.brep), [2, 1, 3, 3, 2]);
        assert_eq!(s.brep.tolerance, uncertainty);
        assert!(s.brep.faces.iter().any(|f| matches!(f.surface, BSurface::Cylinder { .. })));
    }
}

/// A 2 um edge/plane gap inside a 10 um declaration still needs a 6 um allowance
/// (SURFACE_GAP_MARGIN = 3), even though fitting new curves asks for only 1 um.
#[test]
fn a_measured_gap_smaller_than_source_uncertainty_keeps_its_pcurve_allowance() {
    use crate::assembly::{Assembly, Bound, Kind};
    let mut a = Assembly::new(0.01, None);
    let points = [[0.0, 0.0, 0.002], [1.0, 0.0, 0.002], [1.0, 1.0, 0.002], [0.0, 1.0, 0.002]];
    let vs: Vec<u32> = points.iter().map(|&p| a.add_vertex(p).unwrap()).collect();
    let mut edges = Vec::new();
    for k in 0..4 {
        let next = (k + 1) % 4;
        edges.push(a.add_edge(k as u64, BCurve::Line { start: points[k], end: points[next] }, [0.0, 1.0], [vs[k], vs[next]]).unwrap());
    }
    let plane = BSurface::Plane { origin: [0.0; 3], u_axis: [1.0, 0.0, 0.0], v_axis: [0.0, 1.0, 0.0] };
    let f = a.add_face(1, plane, vec![Bound::Edges(edges)], Some(0), true).unwrap();
    let s = a.finish(1, "offset boundary".into(), vec![vec![(f, true)]], Kind::Model).unwrap();
    assert!(s.sheet);
    assert_eq!(s.brep.tolerance, 0.01);
    let f = &s.brep.faces[0];
    let surface = SurfaceEval::new(f.parameters()).unwrap();
    let Loop::Edges { edges } = &f.loops[0] else { panic!("edge loop") };
    for u in edges {
        let e = &s.brep.edges[u.edge as usize];
        let curve = CurveEval::new(e.parameters()).unwrap();
        let q = solidcore_ops::pcurve::eval(&u.pcurve, 0.5).unwrap();
        let gap = surface.point(q).unwrap().dist(curve.point(0.5).unwrap());
        assert!((gap - 0.002).abs() < 1e-12, "the source geometry keeps its real gap");
        assert_eq!(e.tolerance, 0.01);
    }
}

/// A cubic rational spline with 101 control points 10 mm apart along a 1000 mm line, weights
/// alternating 1 and 0.9, on uniform knots over [0, 1]: its speed is about 1000, so a 5e-8 mm edge
/// spans 5e-11 of its parameter, which the kernel resolves, and its knots stay the file's bit for
/// bit. Its local bound is `3 · 1.9 · 50 / 0.9 / (3 / 98)`, about 1.0e4 (the box of the 6 points
/// each derivative term spans, 50 mm); the whole curve's box (1000 mm) would give 2.1e5, past the
/// threshold (5e4), and a 2^18 rescale it does not need. On [0, 1e-7] it is rescaled, exactly.
#[test]
fn a_rational_curve_that_resolves_its_edges_keeps_its_knots() {
    let n = 101;
    let homogeneous: Vec<[f64; 4]> = (0..n)
        .map(|i| {
            let w = if i % 2 == 0 { 1.0 } else { 0.9 };
            [w * 10.0 * f64::from(i), 0.0, 0.0, w]
        })
        .collect();
    let knots = |end: f64| -> Vec<f64> {
        let mut k = vec![0.0; 4];
        k.extend((1..98).map(|j| end * f64::from(j) / 98.0));
        k.extend([end; 4]);
        k
    };
    let curve = BCurve::RationalSpline { knots: knots(1.0), homogeneous: homogeneous.clone() };
    assert_eq!(super::resolvable(curve.clone()).unwrap(), curve);
    let fast = knots(1e-7);
    let BCurve::RationalSpline { knots: scaled, homogeneous: same } =
        super::resolvable(BCurve::RationalSpline { knots: fast.clone(), homogeneous: homogeneous.clone() }).unwrap()
    else {
        panic!("not a rational spline")
    };
    assert_eq!(same, homogeneous);
    let scale = scaled.last().unwrap() / 1e-7;
    assert_eq!(scale, 2f64.powi(scale.log2().round() as i32), "{scale}");
    assert!(scaled.iter().zip(&fast).all(|(s, f)| *s == f * scale), "{scaled:?}");
}

/// A kernel that counts the pcurves it is asked for and makes none.
struct Counting(std::sync::atomic::AtomicUsize);

impl Trims for Counting {
    fn trims(&self, _: &TrimFace<'_>) -> std::result::Result<Vec<Vec<Curve2>>, String> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Err("no trims".into())
    }
}

/// A 10 x 10 x 5 block whose top is a bilinear patch twisted 1 mm at one corner (so 525 mm3 under it) (not a plane),
/// its four top edges the straight lines between the block's own corners, and the patch's control
/// points `off` mm above them: the edges lie `off` mm off the patch's boundary.
fn block_with_a_twisted_patch(off: f64) -> p21::Exchange {
    block_under_a_patch([off; 4], [false; 4], false)
}

/// [`block_with_a_twisted_patch`] with the patch's corner `k` (the top edges' ends, in the order
/// (0, 0), (10, 0), (10, 10), (0, 10)) `offs[k]` mm above the block's, the top edge `i` (corner
/// `i` to `i + 1`) written from `i + 1` to `i` where `reversed[i]` (the patch's loop then runs
/// against it, its use `forward` false), and the patch's weights all 2 where `rational` (the same
/// surface, not a polynomial net). The patch is on (u, v) = (x / 10, y / 10): edge 0 is v = 0,
/// edge 1 is u = 1, edge 2 is v = 1 and edge 3 is u = 0.
fn block_under_a_patch(offs: [f64; 4], reversed: [bool; 4], rational: bool) -> p21::Exchange {
    let mut w = W::default();
    let top = [5.0, 5.0, 6.0, 5.0];
    let bottom: Vec<(u64, V)> =
        [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]].iter().map(|p| (w.vertex([p[0], p[1], 0.0]), [p[0], p[1], 0.0])).collect();
    let high: Vec<(u64, V)> =
        [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]].iter().zip(top).map(|(p, z)| (w.vertex([p[0], p[1], z]), [p[0], p[1], z])).collect();
    let edge = |w: &mut W, a: (u64, V), b: (u64, V)| w.line_edge(a, b, true, len3(sub3(b.1, a.1)));
    let low: Vec<u64> = (0..4).map(|i| edge(&mut w, bottom[i], bottom[(i + 1) % 4])).collect();
    let up: Vec<u64> = (0..4).map(|i| edge(&mut w, bottom[i], high[i])).collect();
    let rim: Vec<u64> =
        (0..4).map(|i| if reversed[i] { edge(&mut w, high[(i + 1) % 4], high[i]) } else { edge(&mut w, high[i], high[(i + 1) % 4]) }).collect();
    let face = |w: &mut W, uses: &[(u64, bool)], surface: u64| {
        let oes: Vec<u64> = uses.iter().map(|&(e, o)| w.oe(e, o)).collect();
        let bound = w.bound(&oes, true, true);
        w.face(&[bound], surface, true)
    };
    let floor = w.plane([0.0; 3], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
    let walls = [
        w.plane([0.0; 3], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]),
        w.plane([10.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        w.plane([0.0, 10.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]),
        w.plane([0.0; 3], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ];
    let mut faces = vec![face(&mut w, &[(low[3], false), (low[2], false), (low[1], false), (low[0], false)], floor)];
    for i in 0..4 {
        // Along the bottom edge, up one corner, back along the top edge, down the other.
        faces.push(face(&mut w, &[(low[i], true), (up[(i + 1) % 4], true), (rim[i], reversed[i]), (up[i], false)], walls[i]));
    }
    // The patch's control points (i along u, that is x; j along v, that is y), `offs` above.
    let mut at = |k: usize| w.pt([high[k].1[0], high[k].1[1], high[k].1[2] + offs[k]]);
    let (p00, p01, p10, p11) = (at(0), at(3), at(1), at(2));
    let patch = if rational {
        w.add(format!(
            "(BOUNDED_SURFACE()B_SPLINE_SURFACE(1,1,((#{p00},#{p01}),(#{p10},#{p11})),.UNSPECIFIED.,.F.,.F.,.F.)\
             B_SPLINE_SURFACE_WITH_KNOTS((2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.)GEOMETRIC_REPRESENTATION_ITEM()\
             RATIONAL_B_SPLINE_SURFACE(((2.,2.),(2.,2.)))REPRESENTATION_ITEM('')SURFACE())"
        ))
    } else {
        w.add(format!(
            "B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#{p00},#{p01}),(#{p10},#{p11})),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.)"
        ))
    };
    faces.push(face(&mut w, &[(rim[0], !reversed[0]), (rim[1], !reversed[1]), (rim[2], !reversed[2]), (rim[3], !reversed[3])], patch));
    let shell = closed_shell(&mut w, &faces);
    let solid = w.add(format!("MANIFOLD_SOLID_BREP('',#{shell})"));
    w.file_with_uncertainty(&[solid], 1e-5)
}

/// A whole spline patch whose four edges lie within a few micrometres of its boundary curves
/// (0, 20 and 75 micrometres, in a file that states 10) gets its pcurves from the closed form the
/// proved gap allows, with no call to the kernel, and the edges' tolerances cover the gap; a patch
/// whose edges stand 0.5 mm off it (past what a tolerance may be raised to) goes to the kernel.
#[test]
fn a_whole_spline_patch_with_edges_off_its_boundary_by_micrometres_needs_no_kernel() {
    for off in [0.0, 2e-5, 7.5e-5] {
        let ex = block_with_a_twisted_patch(off);
        let kernel = Counting(Default::default());
        let r = read_solids_with(&ex, &kernel);
        assert!(r.failures.is_empty(), "{off}: {:?}", r.failures);
        assert_eq!(kernel.0.load(std::sync::atomic::Ordering::Relaxed), 0, "{off}: the kernel was asked for pcurves");
        // And with no kernel at all.
        let s = one_solid(&ex);
        assert_eq!(counts(&s.brep), [2, 1, 6, 12, 8], "{off}");
        // The top edges' tolerance covers the gap (with the margin that covers an analytic one).
        if off > 0.0 {
            let raised = s.brep.edges.iter().filter(|e| e.tolerance >= 3.0 * off).count();
            assert_eq!(raised, 4, "{off}: only the patch's four edges are raised");
        }
        let (body, _) = solidcore_ops::body::Body::from_record(&s.brep, false).unwrap();
        let volume = body.mass_properties(1e-6).unwrap().volume;
        assert!((volume - 525.0).abs() < 0.05, "{off}: {volume}");
    }
    let ex = block_with_a_twisted_patch(0.5);
    let kernel = Counting(Default::default());
    let _ = read_solids_with(&ex, &kernel);
    assert!(kernel.0.load(std::sync::atomic::Ordering::Relaxed) > 0, "past the cap, the kernel is asked");
}

/// A kernel standing in for SMLib on [`block_under_a_patch`]'s patch, which is on (u, v) =
/// (x / 10, y / 10): each pcurve it is asked for is the straight line between those of its edge's
/// ends, and it counts them.
struct OnPatch(std::sync::atomic::AtomicUsize);

impl Trims for OnPatch {
    fn trims(&self, face: &TrimFace<'_>) -> std::result::Result<Vec<Vec<Curve2>>, String> {
        let mut out = Vec::new();
        for l in &face.loops {
            let mut pcs = Vec::new();
            for u in l {
                if let Some(pc) = &u.pcurve {
                    pcs.push(pc.clone());
                    continue;
                }
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let e = face.edges.get(u.edge as usize).ok_or("edge")?;
                let eval = CurveEval::new(e.nurbs.unwrap_or(e.curve)).map_err(|m| m.to_string())?;
                let [t0, t1] = e.interval;
                // The one curved edge written here ([`Run::Bulge`]): u = 0.8 t (1 - t), v = 1 - t.
                if matches!(e.curve, BCurve::Spline { .. }) {
                    pcs.push(Curve2::Spline { knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], points: vec![[0.0, 1.0], [0.4, 0.5], [0.0, 0.0]] });
                    continue;
                }
                let at = |t: f64| eval.point(t).map(|p| [p.x / 10.0, p.y / 10.0]).ok_or("a point");
                pcs.push(Curve2::Spline { knots: vec![t0, t0, t1, t1], points: vec![at(t0)?, at(t1)?] });
            }
            out.push(pcs);
        }
        Ok(out)
    }
}

/// The patch face's loop of [`block_under_a_patch`], each use's edge, whether it runs forward and
/// the two ends of its pcurve; each pcurve is the straight line between the (u, v) of its edge's
/// ends, along one of the patch's four boundaries (u or v 0 or 1). Returns the boundaries, as
/// (the constant coordinate, 0 for u and 1 for v, and its value), and how many uses run against their edge.
fn patch_lines(b: &Brep) -> (Vec<(usize, f64)>, usize) {
    let face = b.faces.iter().find(|f| matches!(f.surface, BSurface::Spline { .. } | BSurface::RationalSpline { .. })).expect("the patch face");
    let Loop::Edges { edges } = &face.loops[0] else { panic!("an edge loop") };
    assert_eq!(edges.len(), 4);
    let (mut sides, mut against) = (Vec::new(), 0);
    for u in edges {
        let Curve2::Spline { knots, points } = &u.pcurve else { panic!("{:?}", u.pcurve) };
        let e = &b.edges[u.edge as usize];
        let uv = |v: u32| [b.vertices[v as usize].point[0] / 10.0, b.vertices[v as usize].point[1] / 10.0];
        // Between its edge's ends: exactly where the proof gave the line, or (where the tolerance
        // already took it) where the closed form projects them, a few parts in 10⁶ off.
        for (got, want) in points.iter().zip([uv(e.vertices[0]), uv(e.vertices[1])]) {
            assert!(got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-5), "a straight line between its edge's ends: {points:?}");
        }
        assert_eq!(knots, &vec![e.interval[0], e.interval[0], e.interval[1], e.interval[1]]);
        // The coordinate that does not change along it: v where u runs, else u.
        let fixed = usize::from((points[0][0] - points[1][0]).abs() > 0.5);
        assert!((points[0][fixed] - points[1][fixed]).abs() < 1e-5, "constant along its side");
        let value = points[0][fixed].round();
        assert!((points[0][fixed] - value).abs() < 1e-5 && (value == 0.0 || value == 1.0), "on a boundary: {value}");
        sides.push((fixed, value));
        against += usize::from(!u.forward);
    }
    sides.sort_by(|a, c| a.partial_cmp(c).unwrap());
    (sides, against)
}

/// Every one of a whole patch's four boundaries gets its straight line with no call to the kernel:
/// edge 0 is v = 0, edge 1 u = 1 (the u maximum), edge 2 v = 1 (the v maximum) and edge 3 u = 0,
/// whichever way each edge is written against the patch's loop (a use that runs against its edge
/// has the pcurve of the edge's own direction, `forward` false).
#[test]
fn every_boundary_of_a_whole_patch_gets_its_line_whichever_way_its_edge_runs() {
    for reversed in [[false; 4], [true; 4], [true, false, true, false], [false, true, false, true], [false, false, true, true]] {
        let ex = block_under_a_patch([2e-5; 4], reversed, false);
        let kernel = OnPatch(Default::default());
        let r = read_solids_with(&ex, &kernel);
        assert!(r.failures.is_empty(), "{reversed:?}: {:?}", r.failures);
        assert_eq!(kernel.0.load(std::sync::atomic::Ordering::Relaxed), 0, "{reversed:?}: the kernel was asked for pcurves");
        let s = one_solid(&ex);
        assert_eq!(counts(&s.brep), [2, 1, 6, 12, 8], "{reversed:?}");
        let (sides, against) = patch_lines(&s.brep);
        // u = 0, u = 1, v = 0, v = 1.
        assert_eq!(sides, [(0, 0.0), (0, 1.0), (1, 0.0), (1, 1.0)], "{reversed:?}: one use on each boundary");
        // A use runs against its edge where the file wrote the edge the other way round.
        let wrong = reversed.iter().filter(|r| **r).count();
        assert!(against == wrong || against == 4 - wrong, "{reversed:?}: {against} uses against their edges");
        let raised = s.brep.edges.iter().filter(|e| e.tolerance >= 3.0 * 2e-5).count();
        assert_eq!(raised, 4, "{reversed:?}: the patch's four edges are covered");
    }
}

/// A face whose adjacent edges lie on boundaries with different gaps (0, 20 and 75 micrometres at
/// the corners) raises each edge to three times its own gap and each vertex to the largest of the
/// edges that meet it, covering the shared corner of two raised edges by the larger; the solid is
/// closed, the kernel is asked for nothing, and the block's other vertices are not touched.
#[test]
fn vertices_shared_by_raised_boundary_edges_hold_the_larger_tolerance() {
    let offs = [0.0, 2e-5, 7.5e-5, 0.0];
    let ex = block_under_a_patch(offs, [false; 4], false);
    let kernel = OnPatch(Default::default());
    let r = read_solids_with(&ex, &kernel);
    assert!(r.failures.is_empty(), "{:?}", r.failures);
    assert_eq!(kernel.0.load(std::sync::atomic::Ordering::Relaxed), 0);
    let s = one_solid(&ex);
    assert_eq!(counts(&s.brep), [2, 1, 6, 12, 8]);
    let b = &s.brep;
    let tolerance_at = |p: V| b.vertices.iter().find(|v| v.point == p).map(|v| v.tolerance).expect("a vertex");
    // The edge from corner i to i + 1 stands off the patch by the larger of its two corners'.
    let gap = |i: usize| offs[i].max(offs[(i + 1) % 4]);
    let corners = [[0.0, 0.0, 5.0], [10.0, 0.0, 5.0], [10.0, 10.0, 6.0], [0.0, 10.0, 5.0]];
    for (k, c) in corners.iter().enumerate() {
        let wanted = 3.0 * gap(k).max(gap((k + 3) % 4));
        let got = tolerance_at(*c);
        assert!(got >= wanted * (1.0 - 1e-9) && got <= wanted * 1.01, "corner {k}: {got} for edges standing off by {wanted}");
    }
    for p in [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0]] {
        assert!(tolerance_at(p) < 2e-5, "{p:?}: {}", tolerance_at(p));
    }
    // The edge between two corners that stand 0 off is not raised past the file's.
    let flat = b.edges.iter().filter(|e| e.tolerance < 2e-5).count();
    assert!(flat >= 9, "{flat}: the edges not on the patch's boundaries keep the file's tolerance, as the edge between the 0 corners");
    let (body, _) = solidcore_ops::body::Body::from_record(b, false).unwrap();
    let volume = body.mass_properties(1e-6).unwrap().volume;
    assert!((volume - 525.0).abs() < 0.05, "{volume}");
}

/// One edge of a [`patch_sheet`]'s loop: the line between two points on the patch, or the curved
/// edge from (0, 10, 5) to (0, 0, 5) that bulges 2 mm in x: the patch's own curve u = 0.8 t (1 - t),
/// v = 1 - t, exactly (a cubic).
enum Run {
    Line(V, V),
    Bulge,
}

/// A sheet of one face on the twisted patch of [`block_under_a_patch`], (x, y, z) = (10 u, 10 v,
/// 5 + u v) with its control points `off` mm above, whose loops (the first the outer one) are `loops`.
fn patch_sheet(off: f64, loops: &[Vec<Run>]) -> p21::Exchange {
    let mut w = W::default();
    let mut vertices: Vec<(V, u64)> = Vec::new();
    let mut vertex = |w: &mut W, p: V| -> (u64, V) {
        if let Some((_, id)) = vertices.iter().find(|(q, _)| *q == p) {
            return (*id, p);
        }
        let id = w.vertex(p);
        vertices.push((p, id));
        (id, p)
    };
    let mut bounds = Vec::new();
    for (i, l) in loops.iter().enumerate() {
        let mut oes = Vec::new();
        for run in l {
            let e = match run {
                Run::Line(a, b) => {
                    let (va, vb) = (vertex(&mut w, *a), vertex(&mut w, *b));
                    w.line_edge(va, vb, true, len3(sub3(*b, *a)))
                }
                Run::Bulge => {
                    let (va, vb) = (vertex(&mut w, [0.0, 10.0, 5.0]), vertex(&mut w, [0.0, 0.0, 5.0]));
                    let poles = [[0.0, 10.0, 5.0], [8.0 / 3.0, 20.0 / 3.0, 5.0 + 0.8 / 3.0], [8.0 / 3.0, 10.0 / 3.0, 5.0], [0.0, 0.0, 5.0]];
                    let ids: Vec<u64> = poles.iter().map(|p| w.pt(*p)).collect();
                    let c = w.add(format!("B_SPLINE_CURVE_WITH_KNOTS('',3,({}),.UNSPECIFIED.,.F.,.F.,(4,4),(0.,1.),.UNSPECIFIED.)", list(&ids)));
                    w.add(format!("EDGE_CURVE('',#{},#{},#{c},.T.)", va.0, vb.0))
                }
            };
            oes.push(w.oe(e, true));
        }
        bounds.push(w.bound(&oes, i == 0, true));
    }
    let mut at = |x: f64, y: f64, z: f64| w.pt([x, y, z + off]);
    let (p00, p01, p10, p11) = (at(0.0, 0.0, 5.0), at(0.0, 10.0, 5.0), at(10.0, 0.0, 5.0), at(10.0, 10.0, 6.0));
    let patch = w.add(format!(
        "B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#{p00},#{p01}),(#{p10},#{p11})),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.)"
    ));
    let face = w.face(&bounds, patch, true);
    let open = w.add(format!("OPEN_SHELL('',(#{face}))"));
    let model = w.add(format!("SHELL_BASED_SURFACE_MODEL('patch',(#{open}))"));
    w.file_with_uncertainty(&[model], 1e-5)
}

/// The four corners of [`patch_sheet`]'s patch, counter-clockwise in (u, v).
const PATCH_CORNERS: [V; 4] = [[0.0, 0.0, 5.0], [10.0, 0.0, 5.0], [10.0, 10.0, 6.0], [0.0, 10.0, 5.0]];

/// The patch's four sides, each a line between two corners.
fn patch_sides() -> Vec<Run> {
    (0..4).map(|k| Run::Line(PATCH_CORNERS[k], PATCH_CORNERS[(k + 1) % 4])).collect()
}

/// A sheet read with the stand-in kernel: its brep, and the number of pcurves the kernel made. How
/// many edges the proof of a boundary raised to three times their gap (to `off`'s 3 times, to a
/// hair): the raise the importer makes for the proof, where the kernel's own measure is 1.5 times.
fn read_sheet(ex: &p21::Exchange, off: f64) -> (Brep, usize, usize) {
    let kernel = OnPatch(Default::default());
    let r = read_solids_with(ex, &kernel);
    assert!(r.failures.is_empty(), "{:?}", r.failures);
    let b = r.solids.into_iter().next().expect("a sheet").brep;
    let raised = b.edges.iter().filter(|e| e.tolerance >= 3.0 * off && e.tolerance <= 3.0 * off * 1.001).count();
    (b, kernel.0.load(std::sync::atomic::Ordering::Relaxed), raised)
}

/// A face is raised for the proof only as a whole patch: exactly one loop of four uses on the
/// patch's four sides. The whole patch is (no kernel, its four edges covered); three sides of it
/// with a fourth that bulges 2 mm into the patch is not, and neither is a face of its four sides
/// with a hole, or a face of two sides and a curve across: the kernel is asked and no edge is
/// raised to the proved three times its gap (what the sides alone would be, one by one, as the
/// proof of an edge was taken before it was asked of the face).
#[test]
fn a_boundary_gap_is_covered_only_where_the_face_is_the_whole_patch() {
    let off = 2e-5;
    let (b, calls, raised) = read_sheet(&patch_sheet(off, &[patch_sides()]), off);
    assert_eq!((calls, raised, b.edges.len()), (0, 4, 4), "the whole patch: no kernel, four edges covered");

    // Three sides and a fourth 2 mm inside the patch at its middle.
    let mut trimmed: Vec<Run> = patch_sides().into_iter().take(3).collect();
    trimmed.push(Run::Bulge);
    let (b, calls, raised) = read_sheet(&patch_sheet(off, &[trimmed]), off);
    assert!(calls > 0, "the kernel makes the pcurves of a trimmed face");
    assert_eq!(raised, 0, "no edge of a trimmed face is raised for a proof: {:?}", b.edges.iter().map(|e| e.tolerance).collect::<Vec<_>>());

    // The four sides and a hole of straight lines on the patch, clockwise about its middle.
    let hole = [[4.0, 4.0, 5.16], [4.0, 6.0, 5.24], [6.0, 6.0, 5.36], [6.0, 4.0, 5.24]];
    let inner = (0..4).map(|k| Run::Line(hole[k], hole[(k + 1) % 4])).collect();
    let (_, calls, raised) = read_sheet(&patch_sheet(off, &[patch_sides(), inner]), off);
    assert!(calls > 0);
    assert_eq!(raised, 0, "a face with a hole is not the whole patch");

    // Two sides whole and a diagonal across: three uses.
    let triangle = vec![
        Run::Line(PATCH_CORNERS[0], PATCH_CORNERS[1]),
        Run::Line(PATCH_CORNERS[1], PATCH_CORNERS[2]),
        Run::Line(PATCH_CORNERS[2], PATCH_CORNERS[0]),
    ];
    let (_, calls, raised) = read_sheet(&patch_sheet(off, &[triangle]), off);
    assert!(calls > 0);
    assert_eq!(raised, 0, "three uses are no whole patch");
}

/// A face that does not qualify is read as it was: a patch whose edges stand 0.5 mm off its
/// boundary (past what a tolerance may be raised to) takes its pcurves from the kernel, exactly as
/// the kernel makes them, and its edges' tolerance is the one the measured gap of those pcurves
/// gives (1.5 times it), with no raise for a proof that did not hold. A rational patch (weights
/// 2, the same surface) is no polynomial net: its pcurves are the kernel's too, with the tolerance
/// their measured gap gives; and with no kernel neither is read.
#[test]
fn a_patch_that_does_not_qualify_is_read_as_before() {
    let far = block_with_a_twisted_patch(0.5);
    let kernel = OnPatch(Default::default());
    let r = read_solids_with(&far, &kernel);
    assert!(r.failures.is_empty(), "{:?}", r.failures);
    assert_eq!(kernel.0.load(std::sync::atomic::Ordering::Relaxed), 4, "the kernel made the patch's four pcurves");
    let s = &r.solids[0];
    let (sides, _) = patch_lines(&s.brep);
    assert_eq!(sides.len(), 4);
    let top: Vec<f64> = s.brep.edges.iter().filter(|e| e.tolerance > 1e-4).map(|e| e.tolerance).collect();
    assert_eq!(top.len(), 4);
    assert!(top.iter().all(|t| (t - 0.75).abs() < 1e-9), "{top:?}: 1.5 times the 0.5 mm the pcurves are measured off");
    assert!(!read_solids(&far).failures.is_empty(), "no kernel, no pcurves");

    // A rational patch, as a polynomial one (the same surface) 20 micrometres off the edges: no
    // closed form takes the line at the file's tolerance, and nothing proves it.
    let rational = block_under_a_patch([2e-5; 4], [false; 4], true);
    let kernel = OnPatch(Default::default());
    let r = read_solids_with(&rational, &kernel);
    assert!(r.failures.is_empty(), "{:?}", r.failures);
    assert_eq!(kernel.0.load(std::sync::atomic::Ordering::Relaxed), 4, "a rational patch goes to the kernel");
    let s = &r.solids[0];
    assert_eq!(patch_lines(&s.brep).0.len(), 4);
    let top: Vec<f64> = s.brep.edges.iter().filter(|e| e.tolerance > 2e-5).map(|e| e.tolerance).collect();
    assert_eq!(top.len(), 4);
    assert!(top.iter().all(|t| (t - 3e-5).abs() < 1e-8), "{top:?}: 1.5 times the 20 micrometres, not the 3 times a proof covers");
    assert!(!read_solids(&rational).failures.is_empty(), "no kernel, no pcurves");
    // The same gap on a polynomial patch is proved: no kernel, and 3 times the gap.
    let proved = block_under_a_patch([2e-5; 4], [false; 4], false);
    let kernel = OnPatch(Default::default());
    assert!(read_solids_with(&proved, &kernel).failures.is_empty());
    assert_eq!(kernel.0.load(std::sync::atomic::Ordering::Relaxed), 0);
}

/// A pcurve kernel that must not be asked: it counts its calls and refuses them.
struct RefusedTrims(std::sync::atomic::AtomicUsize);

impl Trims for RefusedTrims {
    fn trims(&self, _: &TrimFace<'_>) -> std::result::Result<Vec<Vec<Curve2>>, String> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Err("the pcurve kernel was asked".into())
    }
}

/// A 20 mm cube whose 12 edges and 8 corners are rounded with conics (rho 0.4, radius 3 mm): 6
/// planes, 12 rational quadratic strips and 8 corner patches (a biquadratic net with one side
/// collapsed to a point). Its 48 edges are curves of their own, the conic ones 30 nm off the
/// boundary of the patch they bound, and the file carries no pcurves. Every face is read in
/// closed form: the planes' own, and the strips' and corners' as the straight lines along their
/// boundary curves, with the kernel never asked. (Before rational patches had the proof, 20
/// of the 26 faces went to the kernel, 1.4 to 4 s in all.)
#[test]
fn a_cube_with_conic_rounded_edges_and_corners_is_read_without_the_pcurve_kernel() {
    let ex = p21::parse(include_str!("../tests/fixtures/rounded-cube-conic-corners.step")).unwrap();
    let trims = RefusedTrims(std::sync::atomic::AtomicUsize::new(0));
    let read = read_solids_with(&ex, &trims);
    assert!(read.failures.is_empty(), "{:?}", read.failures);
    assert_eq!(read.solids.len(), 1);
    let s = &read.solids[0];
    assert!(!s.sheet);
    assert_eq!(s.brep.faces.len(), 26);
    assert_eq!(s.brep.edges.len(), 48);
    assert_eq!(trims.0.load(std::sync::atomic::Ordering::Relaxed), 0, "the kernel was asked");
    // The edges the proof covered hold their gap: raised past 30 nm, and still within a micrometre.
    let raised = s.brep.edges.iter().filter(|e| e.tolerance > 3e-5).count();
    assert!(raised >= 24, "{raised} edges raised");
    assert!(s.brep.edges.iter().all(|e| e.tolerance < 1e-3));
}

/// A 40 x 10 mm block whose top is one ruled spline patch (cubic along, 40 poles), the two long
/// edges of which are separate splines refitted through points of the patch's own boundary at a
/// warped pace (`make_ruled_patch_fixture.py --poles 40 --period 20 --edge-warp 0.006`): they lie
/// on the boundary to a few micrometres but are 0.16 mm from it at equal parameter, so no
/// straight line in (u, v) is their pcurve. The reader gives each the line along the boundary at
/// the edge's own pace, with the pcurve kernel never asked, and holds the edges' tolerance to the
/// geometric gap, not the parametric one.
#[test]
fn an_edge_refitted_at_its_own_pace_along_a_patch_boundary_is_read_without_the_pcurve_kernel() {
    let ex = p21::parse(include_str!("../tests/fixtures/ruled-spline-patch-reparametrised.step")).unwrap();
    let trims = RefusedTrims(std::sync::atomic::AtomicUsize::new(0));
    let read = read_solids_with(&ex, &trims);
    assert!(read.failures.is_empty(), "{:?}", read.failures);
    assert_eq!(read.solids.len(), 1);
    let s = &read.solids[0];
    assert!(!s.sheet);
    assert_eq!(s.brep.faces.len(), 6);
    assert_eq!(trims.0.load(std::sync::atomic::Ordering::Relaxed), 0, "the kernel was asked");
    // Covered by the gap the proof found (a few micrometres), nowhere near the 0.16 mm at equal parameter.
    let widest = s.brep.edges.iter().map(|e| e.tolerance).fold(0.0_f64, f64::max);
    assert!(widest < 1e-3, "an edge tolerance of {widest} mm");
}

/// `text` with the reference direction of every conical surface's placement turned about its axis by `angle`.
fn cone_frames_turned(text: &str, angle: f64) -> String {
    let refs = |s: &str| -> Vec<u64> {
        s.split_once('=')
            .map_or("", |(_, r)| r)
            .split('#')
            .skip(1)
            .filter_map(|t| t.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok())
            .collect()
    };
    let lines: std::collections::HashMap<u64, &str> = text
        .lines()
        .filter_map(|l| {
            let (id, _) = l.strip_prefix('#')?.split_once('=')?;
            Some((id.trim().parse().ok()?, l))
        })
        .collect();
    let numbers = |l: &str| -> Vec<f64> {
        l.rsplit_once(",(").map_or(vec![], |(_, r)| r.trim_end_matches([')', ';']).split(',').filter_map(|x| x.trim().parse().ok()).collect())
    };
    let mut out = text.to_string();
    for l in lines.values().filter(|l| l.contains("=CONICAL_SURFACE(")) {
        let placement = refs(l)[0];
        let [_, z, x] = refs(lines[&placement])[..] else { panic!("placement {placement}") };
        let (zd, xd) = (numbers(lines[&z]), numbers(lines[&x]));
        let y = [zd[1] * xd[2] - zd[2] * xd[1], zd[2] * xd[0] - zd[0] * xd[2], zd[0] * xd[1] - zd[1] * xd[0]];
        let t: [f64; 3] = std::array::from_fn(|i| xd[i] * angle.cos() + y[i] * angle.sin());
        out = out.replace(lines[&x], &format!("#{x}=DIRECTION('',({:?},{:?},{:?}));", t[0], t[1], t[2]));
    }
    out
}

/// solidcore#3253: a whole-turn cone face whose seam lies a hair (well within the face's
/// tolerance, far beyond rounding) beside a quarter line kept its form starting on that
/// line, a quarter away from the seam, and the block read back without its stepped hole.
#[test]
fn whole_turn_cone_faces_close_on_their_seam_beside_a_quarter_line() {
    let text = include_str!("../tests/fixtures/stepped_tapered_hole.step");
    let slope = 4.0_f64.to_radians().tan();
    let frustum = |r: f64, h: f64| std::f64::consts::PI * h * (r * r + r * (r + h * slope) + (r + h * slope).powi(2)) / 3.0;
    let exact = 9.0 * 24.0 * 18.0 - frustum(1.35, 5.0) - frustum(2.65, 3.5);
    let wrong: Vec<String> = [0.0, 1e-12, -1e-12, 1e-7, 1e-6, 1e-5, -1e-6, -1e-5]
        .into_iter()
        .filter_map(|angle| {
            let ex = p21::parse(&cone_frames_turned(text, angle)).unwrap();
            let s = one_solid(&ex);
            let volume = solidcore_ops::body::Body::from_record(&s.brep, false)
                .map_err(|e| format!("{e:?}"))
                .and_then(|(body, _)| body.mass_properties(1e-6).map_err(|e| format!("{e:?}")))
                .map(|m| m.volume);
            match volume {
                Ok(v) if (v - exact).abs() < 1e-6 * exact => None,
                other => Some(format!("frames turned {angle:e}: {other:?} vs {exact}")),
            }
        })
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");
}
