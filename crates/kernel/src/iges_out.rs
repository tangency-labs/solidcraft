//! IGES 5.3 export, written from the published specification: each body's record as manifold
//! solid B-reps (186, one per material region, with its voids) and open shells (514 form 2),
//! with their faces, loops, edge and vertex lists, on analytic surfaces (planes, cylinders,
//! cones, spheres, tori), surfaces of revolution and rational B-splines, with body names (406
//! name properties) and colours (314). Geometry is the record's, exact, in the same STEP form
//! the STEP export writes ([`solidcraft_step::write::topology`]); IGES loops run
//! counter-clockwise about their surface's normal.

use std::collections::HashMap;
use std::fmt::Write as _;

use solidcraft_geom::brep::{Brep, Loop, Shell};
use solidcraft_step::geom::{Curve, Placement, Surface};
use solidcraft_step::write::topology;

use crate::{ExportBody, KernelError, Result, StepHeader};

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// A real in IGES free format (always with a decimal point).
fn real(x: f64) -> String {
    let x = if x.is_finite() { x } else { 0.0 };
    let s = format!("{x:?}");
    let s = s.replace('e', "E");
    if s.contains('.') {
        s
    } else if let Some((m, e)) = s.split_once('E') {
        format!("{m}.E{e}")
    } else {
        format!("{s}.")
    }
}

/// A Hollerith string.
fn holl(s: &str) -> String {
    let clean: String = s.chars().map(|c| if c.is_ascii() && !c.is_ascii_control() { c } else { '_' }).collect();
    format!("{}H{clean}", clean.len())
}

struct Ent {
    typ: i64,
    form: i64,
    params: Vec<String>,
    xform: i64,
    color: i64,
    label: String,
    dependent: bool,
}

#[derive(Default)]
struct W {
    ents: Vec<Ent>,
    colors: HashMap<[u32; 3], i64>,
    /// The largest coordinate written (the global section's maximum coordinate).
    max_coord: f64,
}

impl W {
    fn add(&mut self, typ: i64, form: i64, params: Vec<String>) -> i64 {
        self.ents.push(Ent { typ, form, params, xform: 0, color: 0, label: String::new(), dependent: true });
        2 * self.ents.len() as i64 - 1
    }
    fn get_mut(&mut self, de: i64) -> Option<&mut Ent> {
        usize::try_from((de - 1) / 2).ok().and_then(|i| self.ents.get_mut(i))
    }
    /// Coordinates of a model point, noted for the maximum coordinate.
    fn coords(&mut self, p: V3) -> [String; 3] {
        self.max_coord = p.iter().fold(self.max_coord, |m, x| if x.is_finite() { m.max(x.abs()) } else { m });
        p.map(real)
    }
    fn point(&mut self, p: V3) -> i64 {
        let c = self.coords(p);
        self.add(116, 0, c.to_vec())
    }
    fn direction(&mut self, d: V3) -> i64 {
        self.add(123, 0, vec![real(d[0]), real(d[1]), real(d[2])])
    }
    /// Transformation (124) taking a frame's local coordinates to the model.
    fn frame(&mut self, o: V3, x: V3, z: V3) -> i64 {
        let y = cross(z, x);
        let rows: Vec<String> = (0..3).flat_map(|i| [real(x[i]), real(y[i]), real(z[i]), real(o[i])]).collect();
        self.add(124, 0, rows)
    }
    fn color(&mut self, c: [f32; 3]) -> i64 {
        let key = c.map(f32::to_bits);
        if let Some(d) = self.colors.get(&key) {
            return *d;
        }
        let d = self.add(314, 0, vec![real(c[0] as f64 * 100.0), real(c[1] as f64 * 100.0), real(c[2] as f64 * 100.0), holl("")]);
        if let Some(e) = self.get_mut(d) {
            e.dependent = false;
        }
        self.colors.insert(key, d);
        d
    }
}

fn failed(what: impl std::fmt::Display) -> KernelError {
    KernelError::Failed(format!("IGES export: {what}"))
}

/// The record's surface parameters' frame axes, as IGES's analytic surfaces take them.
fn axes(p: &Placement) -> (V3, V3, V3) {
    (p.origin, p.axis, p.ref_direction)
}

/// A B-spline (126) of degree `degree` on full `knots`, over `[t0, t1]`.
fn bspline_curve(w: &mut W, degree: usize, knots: &[f64], weights: &[f64], points: &[V3], [t0, t1]: [f64; 2]) -> i64 {
    let rational = weights.iter().any(|x| (x - 1.0).abs() > 1e-12);
    let mut p = vec![
        points.len().saturating_sub(1).to_string(),
        degree.to_string(),
        "0".into(),
        "0".into(),
        if rational { "0" } else { "1" }.into(),
        "0".into(),
    ];
    p.extend(knots.iter().map(|x| real(*x)));
    p.extend(weights.iter().map(|x| real(*x)));
    for q in points {
        p.extend(w.coords(*q));
    }
    p.extend([real(t0), real(t1), real(0.0), real(0.0), real(0.0)]);
    w.add(126, 0, p)
}

/// Full knots of a STEP B-spline (each repeated by its multiplicity).
fn full_knots(knots: &[f64], mults: &[usize]) -> Vec<f64> {
    knots.iter().zip(mults).flat_map(|(k, m)| std::iter::repeat_n(*k, *m)).collect()
}

/// The curve over `[t0, t1]` as an IGES entity running from `a` to `b` (the edge's vertices at
/// its interval's ends); `closed` when they are one vertex. The DE pointer.
fn curve(w: &mut W, c: &Curve, interval: [f64; 2], a: V3, b: V3, closed: bool) -> Result<i64> {
    match c {
        Curve::Line { .. } => {
            let (ca, cb) = (w.coords(a), w.coords(b));
            Ok(w.add(110, 0, ca.into_iter().chain(cb).collect()))
        }
        Curve::Circle { position, .. } | Curve::Ellipse { position, .. } => {
            let (o, z, x) = axes(position);
            let y = cross(z, x);
            let local = |q: V3| {
                let v = sub(q, o);
                [dot(v, x), dot(v, y)]
            };
            let (la, lb) = (local(a), if closed { local(a) } else { local(b) });
            let tr = w.frame(o, x, z);
            let de = match c {
                Curve::Ellipse { semi_axis_1: ra, semi_axis_2: rb, .. } => {
                    // A x² + C y² + F = 0 with F = -1: A = 1/ra², C = 1/rb².
                    w.add(
                        104,
                        1,
                        vec![
                            real(1.0 / (ra * ra)),
                            real(0.0),
                            real(1.0 / (rb * rb)),
                            real(0.0),
                            real(0.0),
                            real(-1.0),
                            real(0.0),
                            real(la[0]),
                            real(la[1]),
                            real(lb[0]),
                            real(lb[1]),
                        ],
                    )
                }
                _ => w.add(100, 0, vec![real(0.0), real(0.0), real(0.0), real(la[0]), real(la[1]), real(lb[0]), real(lb[1])]),
            };
            if let Some(ent) = w.get_mut(de) {
                ent.xform = tr;
            }
            Ok(de)
        }
        Curve::BSpline(s) => {
            let weights = s.weights.clone().unwrap_or_else(|| vec![1.0; s.points.len()]);
            Ok(bspline_curve(w, s.degree, &full_knots(&s.knots, &s.multiplicities), &weights, &s.points, interval))
        }
        // Segment i on [i, i + 1]: a degree-1 B-spline on knots 0, 0, 1, …, n − 1, n − 1.
        Curve::Polyline(points) => {
            let n = points.len();
            let knots: Vec<f64> = std::iter::once(0.0).chain((0..n).map(|i| i as f64)).chain(std::iter::once(n.saturating_sub(1) as f64)).collect();
            Ok(bspline_curve(w, 1, &knots, &vec![1.0; n], points, interval))
        }
        _ => Err(failed("a curve of this kind is not written yet")),
    }
}

/// A revolution's profile written over `range` of its parameter.
fn profile(w: &mut W, c: &Curve, range: [f64; 2]) -> Result<i64> {
    let at = |t: f64| c.point(t).ok_or_else(|| failed(format!("the profile has no point at {t}")));
    let (a, b) = (at(range[0])?, at(range[1])?);
    curve(w, c, range, a, b, false)
}

/// A face's surface; `domain` is the face's parameter box in the surface's (STEP) parameters.
fn surface(w: &mut W, s: &Surface, domain: [[f64; 2]; 2]) -> Result<i64> {
    let analytic = |w: &mut W, typ: i64, p: &Placement, extra: &[f64]| -> Result<i64> {
        let (o, z, x) = axes(p);
        let (po, pz, px) = (w.point(o), w.direction(z), w.direction(x));
        let mut v = vec![po.to_string()];
        let e = |i: usize| real(extra.get(i).copied().unwrap_or(0.0));
        match typ {
            190 => v.extend([pz.to_string(), px.to_string()]),
            192 => v.extend([pz.to_string(), e(0), px.to_string()]),
            194 => v.extend([pz.to_string(), e(0), real(extra.get(1).copied().unwrap_or(0.0).to_degrees()), px.to_string()]),
            196 => v.extend([e(0), pz.to_string(), px.to_string()]),
            _ => v.extend([pz.to_string(), e(0), e(1), px.to_string()]),
        }
        Ok(w.add(typ, 1, v))
    };
    match s {
        Surface::Plane { position } => analytic(w, 190, position, &[]),
        Surface::Cylinder { position, radius } => analytic(w, 192, position, &[*radius]),
        Surface::Cone { position, radius, semi_angle } => analytic(w, 194, position, &[*radius, *semi_angle]),
        Surface::Sphere { position, radius } => analytic(w, 196, position, &[*radius]),
        Surface::Torus { position, major_radius, minor_radius } => analytic(w, 198, position, &[*major_radius, *minor_radius]),
        Surface::Revolution { profile: c, axis } => {
            // STEP's v is the profile's parameter: the generatrix over the face's.
            let generatrix = profile(w, c, domain[1])?;
            let (o, d) = (axis.origin, axis.direction);
            let (co, cd) = (w.coords(o), w.coords([o[0] + d[0], o[1] + d[1], o[2] + d[2]]));
            let line = w.add(110, 0, co.into_iter().chain(cd).collect());
            Ok(w.add(120, 0, vec![line.to_string(), generatrix.to_string(), real(0.0), real(std::f64::consts::TAU)]))
        }
        Surface::BSpline(b) => {
            let nu = b.points.len();
            let nv = b.points.first().map_or(0, Vec::len);
            if nu == 0 || nv == 0 || b.points.iter().any(|r| r.len() != nv) {
                return Err(failed("a ragged B-spline surface"));
            }
            let (ku, kv) = (full_knots(&b.u_knots, &b.u_multiplicities), full_knots(&b.v_knots, &b.v_multiplicities));
            let wt = |i: usize, j: usize| b.weights.as_ref().and_then(|w| w.get(i)?.get(j).copied()).unwrap_or(1.0);
            let rational = (0..nu).any(|i| (0..nv).any(|j| (wt(i, j) - 1.0).abs() > 1e-12));
            let mut v = vec![
                (nu - 1).to_string(),
                (nv - 1).to_string(),
                b.u_degree.to_string(),
                b.v_degree.to_string(),
                "0".into(),
                "0".into(),
                if rational { "0" } else { "1" }.into(),
                "0".into(),
                "0".into(),
            ];
            v.extend(ku.iter().map(|x| real(*x)));
            v.extend(kv.iter().map(|x| real(*x)));
            // The first index runs fastest.
            for j in 0..nv {
                for i in 0..nu {
                    v.push(real(wt(i, j)));
                }
            }
            for j in 0..nv {
                for i in 0..nu {
                    let q = b.points.get(i).and_then(|r| r.get(j)).copied().unwrap_or([0.0; 3]);
                    v.extend(w.coords(q));
                }
            }
            let range = |k: &[f64], d: usize, n: usize| (k.get(d).copied().unwrap_or(0.0), k.get(n).copied().unwrap_or(1.0));
            let ((u0, u1), (v0, v1)) = (range(&ku, b.u_degree, nu), range(&kv, b.v_degree, nv));
            v.extend([real(u0), real(u1), real(v0), real(v1)]);
            Ok(w.add(128, 0, v))
        }
        _ => Err(failed("a surface of this kind is not written yet")),
    }
}

/// The face's parameter box mapped onto its STEP surface's parameters (a linear map: the box
/// of the mapped corners).
fn mapped_domain(domain: [[f64; 2]; 2], map: topology::Map) -> [[f64; 2]; 2] {
    let [[u0, u1], [v0, v1]] = domain;
    let corners = [[u0, v0], [u1, v0], [u0, v1], [u1, v1]].map(|c| map.apply(c));
    let span = |d: usize| corners.iter().fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], c| [lo.min(c[d]), hi.max(c[d])]);
    [span(0), span(1)]
}

/// The parameters of a shell (514): its faces, with their vertex list, edge list and loops
/// written first.
fn shell(w: &mut W, b: &Brep, si: u32, paint: &HashMap<usize, [f32; 3]>) -> Result<Vec<String>> {
    let Some(Shell::Faces { faces, .. }) = b.shells.get(si as usize) else { return Err(failed(format!("shell {si} is not a face shell"))) };
    // Vertices and edges of the shell, in first-use order.
    let (mut vidx, mut verts): (HashMap<u32, usize>, Vec<V3>) = (HashMap::new(), Vec::new());
    let (mut eidx, mut edges): (HashMap<u32, usize>, Vec<u32>) = (HashMap::new(), Vec::new());
    let mut face_data = Vec::with_capacity(faces.len());
    for of in faces {
        let face = b.faces.get(of.face as usize).ok_or_else(|| failed(format!("face {}: no such face", of.face)))?;
        let (s, map) = topology::surface(face.parameters()).map_err(|e| failed(format!("face {}: {e}", of.face)))?;
        let keeps = map.keeps_normal();
        let outer = topology::outer_loop(b, face);
        let order: Vec<usize> = outer.into_iter().chain((0..face.loops.len()).filter(|l| Some(*l) != outer)).collect();
        let mut loops: Vec<Vec<(usize, bool)>> = Vec::new();
        for li in order {
            let Some(Loop::Edges { edges: uses }) = face.loops.get(li) else { continue };
            let mut l = Vec::with_capacity(uses.len());
            for u in uses {
                let k = match eidx.get(&u.edge) {
                    Some(k) => *k,
                    None => {
                        let e = b.edges.get(u.edge as usize).ok_or_else(|| failed(format!("edge {}: no such edge", u.edge)))?;
                        for v in e.vertices {
                            if let std::collections::hash_map::Entry::Vacant(e) = vidx.entry(v) {
                                let p = b.vertices.get(v as usize).ok_or_else(|| failed(format!("vertex {v}: no such vertex")))?.point;
                                e.insert(verts.len());
                                verts.push(p);
                            }
                        }
                        eidx.insert(u.edge, edges.len());
                        edges.push(u.edge);
                        edges.len() - 1
                    }
                };
                l.push((k, u.forward));
            }
            // Record loops run counter-clockwise about the natural normal; IGES's about the
            // surface's.
            if !keeps {
                l.reverse();
                for x in &mut l {
                    x.1 = !x.1;
                }
            }
            loops.push(l);
        }
        face_data.push((of.face, s, mapped_domain(face.domain, map), of.forward == keeps, outer.is_some(), loops));
    }
    let mut vp = vec![verts.len().to_string()];
    for v in &verts {
        vp.extend(w.coords(*v));
    }
    let vlist = w.add(502, 1, vp);
    // Edges run along their curves, from the record edge's start (interval min) to its end.
    let mut ep = vec![edges.len().to_string()];
    for &e in &edges {
        let edge = b.edges.get(e as usize).ok_or_else(|| failed(format!("edge {e}: no such edge")))?;
        let c = topology::curve(edge.parameters()).map_err(|m| failed(format!("edge {e}: {m}")))?;
        let [sv, tv] = edge.vertices.map(|v| vidx.get(&v).copied().unwrap_or(0));
        let (pa, pb) = (verts.get(sv).copied().unwrap_or([0.0; 3]), verts.get(tv).copied().unwrap_or([0.0; 3]));
        let cd = curve(w, &c, edge.interval, pa, pb, sv == tv)?;
        ep.extend([cd.to_string(), vlist.to_string(), (sv + 1).to_string(), vlist.to_string(), (tv + 1).to_string()]);
    }
    let elist = w.add(504, 1, ep);
    let mut sp = vec![face_data.len().to_string()];
    for (f, s, domain, agrees, has_outer, loops) in face_data {
        let sd = surface(w, &s, domain)?;
        let mut lids = Vec::with_capacity(loops.len());
        for l in loops {
            let mut lp = vec![l.len().to_string()];
            for (k, along) in l {
                lp.extend(["0".into(), elist.to_string(), (k + 1).to_string(), if along { "1" } else { "0" }.into(), "0".into()]);
            }
            lids.push(w.add(508, 1, lp).to_string());
        }
        let mut fp = vec![sd.to_string(), lids.len().to_string(), if has_outer { "1" } else { "0" }.into()];
        fp.extend(lids);
        let fd = w.add(510, 1, fp);
        if let Some(c) = paint.get(&(f as usize)) {
            let cd = w.color(*c);
            if let Some(e) = w.get_mut(fd) {
                e.color = -cd;
            }
        }
        sp.extend([fd.to_string(), if agrees { "1" } else { "0" }.into()]);
    }
    Ok(sp)
}

/// An independent, named, coloured entity: its name as a name property (406 form 15).
fn top_level(w: &mut W, typ: i64, form: i64, mut params: Vec<String>, name: &str, color: Option<[f32; 3]>) -> i64 {
    let np = w.add(406, 15, vec!["1".into(), holl(name)]);
    params.extend(["0".into(), "1".into(), np.to_string()]);
    let d = w.add(typ, form, params);
    let c = color.map(|c| w.color(c));
    if let Some(ent) = w.get_mut(d) {
        ent.dependent = false;
        ent.label = name.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
        if let Some(c) = c {
            ent.color = -c;
        }
    }
    d
}

/// IGES text (5.3, millimetres) for bodies.
pub fn iges_export_bodies(bodies: &[ExportBody], header: &StepHeader) -> Result<String> {
    let mut w = W { max_coord: 1.0, ..W::default() };
    for eb in bodies {
        eb.body.require_brep("IGES export")?;
        let b = eb.body.record();
        let paint: HashMap<usize, [f32; 3]> = eb.body.paint().map(|p| p.faces.iter().map(|f| (f.face, f.color)).collect()).unwrap_or_default();
        let items = topology::layout(b).map_err(failed)?;
        // A closed shell is form 1, an open one form 2.
        for solid in &items.solids {
            let params = shell(&mut w, b, solid.outer, &paint)?;
            let outer = w.add(514, 1, params);
            let mut p = vec![outer.to_string(), "1".into(), solid.voids.len().to_string()];
            for &v in &solid.voids {
                let params = shell(&mut w, b, v, &paint)?;
                p.extend([w.add(514, 1, params).to_string(), "1".into()]);
            }
            top_level(&mut w, 186, 0, p, &eb.name, eb.color);
        }
        for &si in &items.sheets {
            let params = shell(&mut w, b, si, &paint)?;
            top_level(&mut w, 514, 2, params, &eb.name, eb.color);
        }
    }
    if w.ents.is_empty() {
        return Err(KernelError::Invalid("nothing to export".into()));
    }
    let max_coord = w.max_coord;
    Ok(layout(&w, header, max_coord))
}

/// The file: start, global, directory and parameter sections and terminator.
fn layout(w: &W, header: &StepHeader, max_coord: f64) -> String {
    let mut out = String::new();
    let line = |out: &mut String, data: &str, sect: char, seq: usize| {
        let _ = writeln!(out, "{data:<72}{sect}{seq:>7}");
    };
    line(&mut out, "SolidCraft IGES export", 'S', 1);
    let stamp = {
        let t = header.time_stamp.replace(['-', ':', 'T'], "");
        let t: String = t.chars().filter(char::is_ascii_digit).collect();
        if t.len() >= 14 { format!("{}.{}", &t[..8], &t[8..14]) } else { "20000101.000000".to_string() }
    };
    let g = [
        "1H,".to_string(),
        "1H;".to_string(),
        holl("SolidCraft"),
        holl(&header.file_name),
        holl("SolidCraft"),
        holl("SolidCraft IGES 5.3"),
        "32".into(),
        "38".into(),
        "6".into(),
        "308".into(),
        "15".into(),
        holl(&header.file_name),
        real(1.0),
        "2".into(),
        holl("MM"),
        "1".into(),
        real(1.0),
        holl(&stamp),
        real(1e-6),
        real(max_coord),
        holl(&header.author),
        holl(&header.organization),
        "11".into(),
        "0".into(),
        holl(&stamp),
    ];
    let mut gseq = 0;
    for chunk in wrap(&g.iter().map(String::as_str).collect::<Vec<_>>(), ',', ';', 72) {
        gseq += 1;
        line(&mut out, &chunk, 'G', gseq);
    }
    // Parameter lines per entity.
    let mut pdata: Vec<Vec<String>> = Vec::with_capacity(w.ents.len());
    for e in &w.ents {
        let mut fields = vec![e.typ.to_string()];
        fields.extend(e.params.iter().cloned());
        pdata.push(wrap(&fields.iter().map(String::as_str).collect::<Vec<_>>(), ',', ';', 64));
    }
    let mut pstart = Vec::with_capacity(w.ents.len());
    let mut next = 1;
    for p in &pdata {
        pstart.push(next);
        next += p.len();
    }
    let f8 = |x: i64| format!("{x:>8}");
    for (k, e) in w.ents.iter().enumerate() {
        let status = if e.dependent { "00010000" } else { "00000000" };
        let a = format!("{}{}{}{}{}{}{}{}{status}", f8(e.typ), f8(pstart[k] as i64), f8(0), f8(0), f8(0), f8(0), f8(e.xform), f8(0));
        let b = format!("{}{}{}{}{}{}{}{:>8}{}", f8(e.typ), f8(0), f8(e.color), f8(pdata[k].len() as i64), f8(e.form), f8(0), f8(0), e.label, f8(0));
        line(&mut out, &a, 'D', 2 * k + 1);
        line(&mut out, &b, 'D', 2 * k + 2);
    }
    let mut pseq = 0;
    for (k, p) in pdata.iter().enumerate() {
        for l in p {
            pseq += 1;
            line(&mut out, &format!("{l:<64}{:>8}", 2 * k + 1), 'P', pseq);
        }
    }
    line(&mut out, &format!("S{:>7}G{:>7}D{:>7}P{:>7}", 1, gseq, 2 * w.ents.len(), pseq), 'T', 1);
    out
}

/// Fields joined by `delim`, ended by `end`, broken into lines of at most `width` columns
/// between fields (a longer field is split, as strings may be).
fn wrap(fields: &[&str], delim: char, end: char, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for (i, f) in fields.iter().enumerate() {
        let sep = if i + 1 == fields.len() { end } else { delim };
        let item = format!("{f}{sep}");
        if cur.len() + item.len() > width && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
        }
        if item.len() > width {
            let chars: Vec<char> = item.chars().collect();
            for c in chars.chunks(width) {
                let piece: String = c.iter().collect();
                if piece.len() == width {
                    lines.push(piece);
                } else {
                    cur = piece;
                }
            }
            continue;
        }
        cur.push_str(&item);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}
