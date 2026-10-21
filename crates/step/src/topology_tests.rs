//! STEP topology into schema 4: small solids written here entity by entity (boxes, a cylinder
//! with its seam, a plate with a through hole, sheets, a void, faces and edges written against
//! their geometry's sense), each read, validated, spot-checked and its faces' sides checked
//! against the material; hostile files are errors, never panics. Expected values are analytic.

use std::fmt::Write as _;

use super::*;
use crate::p21;

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
    fn file(mut self, items: &[u64]) -> p21::Exchange {
        let l = self.add("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
        let a = self.add("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))");
        let s = self.add("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())");
        let u = self.add(format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#{l},'distance_accuracy_value','')"));
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
    s.brep.validate().unwrap();
    s.brep.check_geometry().unwrap();
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
            let (a, c) = (u.pcurve.eval(m).unwrap(), u.pcurve.eval(m + 1e-4 * (t1 - t0)).unwrap());
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
            assert!((e.interval[1] - e.interval[0] - TAU).abs() < 1e-12);
            assert_eq!(e.vertices[0], e.vertices[1]);
        }
    }
    faces_away(&s.brep, 0, |p| [1.0, 2.0, p[2].clamp(1.0, 9.0)]);
    // The side face spans one turn of the cylinder and its height.
    let f = s.brep.faces.iter().find(|f| matches!(f.surface, BSurface::Cylinder { .. })).unwrap();
    assert!((f.domain[0][1] - f.domain[0][0] - TAU).abs() < 1e-9 && f.domain[1][0] < 1e-6 && f.domain[1][1] > 10.0 - 1e-6, "{:?}", f.domain);
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
        assert!((e.interval[1] - e.interval[0] - PI).abs() < 1e-12, "{:?}", e.interval);
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
    // An edge across a closed spline's seam is refused, for now.
    let r = read_solids(&spline_sheet(true, true));
    assert_eq!(r.solids.len(), 0);
    assert!(r.failures[0].message.contains("across a closed spline's seam"), "{:?}", r.failures);
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
    // Unsupported curves.
    let hyperbola = Curve::Hyperbola { position, semi_axis: 1.0, semi_imag_axis: 1.0 };
    assert!(map_edge(&hyperbola, s, e, true, false, false, tol).unwrap_err().contains("not supported yet: HYPERBOLA"));
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
        ("an unsupported solid", Box::new(|t: &str| t.replace("MANIFOLD_SOLID_BREP", "FACETED_BREP"))),
    ];
    for (what, edit) in cases {
        let r = edited(edit);
        assert!(r.solids.is_empty() && r.failures.len() == 1, "{what}: {:?} {}", r.failures, r.solids.len());
        eprintln!("{what}: {}", r.failures[0].message);
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
