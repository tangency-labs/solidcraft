//! Inspection: curvature combs and minimum radius on sketch curves, model edges and faces;
//! centre of mass of bodies.

use serde_json::{Value, json};
use solidcraft_geom::{Aabb3, Seg2, Vec2, Vec3};

use super::CommandSpec;
use crate::params::{bad, bool_, num, string_list, vec3};
use crate::{CombPick, CombView, EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("inspect.curvature_comb", "Curvature Comb Analysis", curvature_comb)
        .at("SKETCH", "INSPECT")
        .icon("curvature_comb")
        .noundo()
        .params(
            "curves?: [sketch curve ids] (sketch?: id|name, default active) | edges?: [[x,y,z] points on body edges: the edge whose \
             exact curve passes nearest] | faces?: [[x,y,z] points on faces] with count?: isoparametric curves each way (default 4; \
             combs along the face's iso grid); density?: teeth per curve, 2…2000 (default 40; on an edge or an iso curve exactly \
             that many, evenly spaced along its parameter); scale?: comb length per unit curvature (default auto: the longest \
             tooth a fifth of the combs' size); length?: a multiplier on the automatic scale, 0.01…100 (default 1; ignored with \
             `scale`); add?: true keeps the combs shown and adds these, every comb again at one scale (with nothing picked it \
             draws those shown again with this call's options, or turns the analysis on with none); clear?: true turns the \
             analysis off. Every comb of a call shares one scale, so tooth lengths compare across curves; the viewport draws \
             them. The curvature is exact: from each sketch curve, edge curve (line, circle, ellipse, B-spline, NURBS, \
             intersection curve) or face surface's own derivatives, never from the display mesh; a face's teeth are the \
             surface's normal curvature along each iso curve, along the surface normal. → combs: [{curve, teeth: [[foot, tip]], \
             envelope: [tip], max_curvature, min_radius}] (those shown, with `add`), scale",
        ),
    CommandSpec::new("inspect.minimum_radius", "Minimum Radius Analysis", minimum_radius)
        .at("SKETCH", "INSPECT")
        .icon("min_radius")
        .noundo()
        .params("curves?: [sketch curve ids] (sketch?) | edges?: [[x,y,z]…] | faces?: [[x,y,z]…] (default: every face of every body); the smallest radius of curvature of each and where it is"),
    CommandSpec::new("inspect.isocurve", "Isocurve Analysis", isocurves)
        .at("SKETCH", "INSPECT")
        .icon("iso_analysis")
        .noundo()
        .params(
            "faces: [[x,y,z]…] points on faces (1…100); count?: whole number of curves each way, 1…100 (default 8); \
             add?: true keeps the faces already shown; clear?: true turns the analysis off. → each face's isoparametric \
             curves, u = const and v = const of its surface, evenly spaced across it and cut to its trims, as points \
             (`curves`) and per face (`faces`); the viewport draws them",
        ),
    CommandSpec::new("inspect.center_of_mass", "Center of Mass", center_of_mass)
        .at("SKETCH", "INSPECT")
        .icon("center_of_mass")
        .noundo()
        .params("bodies?: [names] (default all): the centre of mass (uniform density) of each and of all together, from the exact mass properties `inspect.measure` takes → center, volume, bodies: [{body, center, volume}], skipped: [{body, reason}] (bodies that enclose no volume or could not be measured)"),
];

/// Curvature (signed, 1/mm) and unit normal of a segment at `t`.
fn seg_curvature(s: &Seg2, t: f64) -> (f64, Vec2) {
    let d1 = s.derivative(t);
    let h = 1e-5;
    let (ta, tb) = ((t - h).max(0.0), (t + h).min(1.0));
    let d2 = (s.derivative(tb) - s.derivative(ta)) / (tb - ta).max(1e-12);
    let l = d1.len().max(1e-300);
    let k = d1.cross(d2) / (l * l * l);
    (k, d1.perp() / l)
}

/// Teeth along a 2D curve: (foot, curvature, normal).
fn teeth_2d(segs: &[Seg2], density: usize) -> Vec<(Vec2, f64, Vec2)> {
    let per = density.div_ceil(segs.len().max(1)).max(2);
    let mut out = Vec::new();
    for s in segs {
        for i in 0..=per {
            let t = i as f64 / per as f64;
            let (k, n) = seg_curvature(s, t);
            out.push((s.point_at(t), k, n));
        }
    }
    out
}

/// Teeth from the kernel's exact comb: (foot, curvature, unit normal towards the centre of
/// curvature; zero where straight).
fn teeth_exact(teeth: &[crate::kernel::CombTooth]) -> Vec<(Vec3, f64, Vec3)> {
    teeth
        .iter()
        .map(|t| {
            let k = t.curvature.len();
            (t.at, k, if k > 0.0 { t.curvature * (1.0 / k) } else { Vec3::ZERO })
        })
        .collect()
}

/// The comb of the body edge whose exact curve passes nearest `at`, over the model's B-rep bodies
/// (mesh bodies have no curves): `density` exact teeth along it.
fn edge_teeth(st: &solidcraft_doc::ModelState, at: Vec3, density: usize) -> Result<Option<Vec<(Vec3, f64, Vec3)>>> {
    let mut best: Option<(f64, &solidcraft_doc::ModelBody, usize)> = None;
    for b in st.bodies.iter().filter(|b| !b.body.is_mesh()) {
        if let Some((d, edge)) = crate::kernel::comb_edge_near(&b.body, at)?
            && best.is_none_or(|x| d < x.0)
        {
            best = Some((d, b, edge));
        }
    }
    match best {
        Some((_, b, edge)) => Ok(Some(teeth_exact(&crate::kernel::edge_comb(&b.body, edge, density)?))),
        None => Ok(None),
    }
}

/// Most curves Curvature Comb Analysis shows at once.
const MAX_COMB_PICKS: usize = 1000;

/// The combs of one pick, each as (id, teeth (foot, curvature, unit normal)): one for a curve or
/// an edge, one per iso curve for a face.
fn pick_teeth(s: &Session, pick: &CombPick, density: usize) -> Result<Vec<(String, Vec<(Vec3, f64, Vec3)>)>> {
    let cmd = "inspect.curvature_comb";
    match pick {
        CombPick::Curve { sketch, curve } => {
            let st = s.model.state();
            let ss = st.sketch(*sketch).ok_or_else(|| EngineError::Other("that sketch is not evaluated".into()))?;
            let ci = ss.sketch.curve_index(curve).ok_or_else(|| bad(cmd, format!("unknown curve `{curve}`")))?;
            let segs = ss.sketch.exact_segs(ci).unwrap_or_else(|| ss.sketch.segs(ci));
            let teeth = teeth_2d(&segs, density).into_iter().map(|(q, k, n)| (ss.plane.to_world(q), k, ss.plane.dir_to_world(n))).collect();
            Ok(vec![(curve.clone(), teeth)])
        }
        CombPick::Edge { at } => {
            let teeth = edge_teeth(&s.model.state(), *at, density)?.ok_or_else(|| bad(cmd, "no edge there"))?;
            Ok(vec![(format!("edge@{},{},{}", at.x, at.y, at.z), teeth)])
        }
        CombPick::Face { at, count } => {
            let world = s.world_state()?;
            let (body, face) = iso_face_at(s, &world, *at).ok_or_else(|| bad(cmd, "no face there"))?;
            let mut out = Vec::new();
            for (k, line) in body.iso_lines(face, *count)?.iter().enumerate() {
                out.push((format!("face@{},{},{}#{k}", at.x, at.y, at.z), teeth_exact(&crate::kernel::iso_comb(&body.body, face, line, density)?)));
            }
            Ok(out)
        }
    }
}

/// `inspect.curvature_comb`: the combs of the picked curves, edges and faces (with `add`, those
/// already shown too), all at one scale, kept so the viewport draws them. View state only.
fn curvature_comb(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.curvature_comb";
    if bool_(p, "clear").unwrap_or(false) {
        s.comb = None;
        s.revision += 1;
        return Ok(json!({"analysis": "curvature_comb", "cleared": true}));
    }
    let add = bool_(p, "add").unwrap_or(false);
    let shown = if add { s.comb.clone() } else { None };
    let density = num(p, "density")
        .filter(|d| d.is_finite())
        .map(|d| d.clamp(crate::kernel::MIN_COMB_TEETH as f64, crate::kernel::MAX_COMB_TEETH as f64) as usize)
        .unwrap_or(shown.as_ref().map_or(40, |v| v.density));
    let length = match p.get("length") {
        None | Some(Value::Null) => shown.as_ref().map_or(1.0, |v| v.length),
        Some(v) => v.as_f64().filter(|x| (0.01..=100.0).contains(x)).ok_or_else(|| bad(cmd, "`length` must be a multiplier 0.01…100"))?,
    };
    let fixed_scale = match p.get("scale") {
        None | Some(Value::Null) => shown.as_ref().and_then(|v| v.fixed_scale),
        Some(_) => num(p, "scale").filter(|x| x.is_finite() && *x > 0.0),
    };
    let mut picks: Vec<CombPick> = Vec::new();
    let curves = string_list(p, "curves");
    if !curves.is_empty() {
        let id = match p.get("sketch") {
            Some(Value::Number(n)) => n.as_u64(),
            Some(Value::String(x)) => s.doc.find_feature(x).map(|f| f.id),
            _ => s.active_sketch,
        }
        .ok_or_else(|| bad(cmd, "no such sketch (and none is active)"))?;
        picks.extend(curves.iter().take(MAX_COMB_PICKS).map(|c| CombPick::Curve { sketch: id, curve: c.clone() }));
    }
    if let Some(edges) = p.get("edges").and_then(Value::as_array) {
        for e in edges.iter().take(MAX_COMB_PICKS) {
            picks.push(CombPick::Edge { at: vec3(e).ok_or_else(|| bad(cmd, "`edges` must be points [x,y,z]"))? });
        }
    }
    if let Some(faces) = p.get("faces") {
        let faces = faces.as_array().filter(|a| !a.is_empty()).ok_or_else(|| bad(cmd, "`faces` must list points [x,y,z]"))?;
        let count = match p.get("count") {
            None | Some(Value::Null) => 4,
            Some(_) => {
                num(p, "count").filter(|c| c.is_finite()).map(|c| c.clamp(1.0, 50.0) as usize).ok_or_else(|| bad(cmd, "`count` must be a number"))?
            }
        };
        for v in faces.iter().take(100) {
            picks.push(CombPick::Face { at: vec3(v).ok_or_else(|| bad(cmd, "`faces` must list points [x,y,z]"))?, count });
        }
    }
    if picks.is_empty() && !add {
        return Err(bad(cmd, "give `curves`, `edges` or `faces`"));
    }
    let shown_picks = shown.map(|v| v.picks).unwrap_or_default();
    if shown_picks.len() + picks.len() > MAX_COMB_PICKS {
        return Err(bad(cmd, format!("at most {MAX_COMB_PICKS} curves can be combed at once")));
    }
    // The curves shown are combed again with these options; one gone with a model change since
    // (its sketch, edge or face no longer there) is dropped. A new pick must be there.
    let mut kept: Vec<CombPick> = Vec::new();
    let mut combs: Vec<(String, Vec<(Vec3, f64, Vec3)>)> = Vec::new();
    for pick in shown_picks {
        if let Ok(teeth) = pick_teeth(s, &pick, density) {
            combs.extend(teeth);
            kept.push(pick);
        }
    }
    for pick in picks {
        combs.extend(pick_teeth(s, &pick, density)?);
        kept.push(pick);
    }
    // One scale for all combs: the longest tooth is a fifth of the largest curve's size.
    let kmax = combs.iter().flat_map(|c| c.1.iter().map(|t| t.1.abs())).fold(0.0_f64, f64::max);
    let bounds: Aabb3 = combs.iter().flat_map(|c| c.1.iter().map(|t| t.0)).collect();
    let diag = bounds.diagonal();
    let auto = if kmax > 1e-12 && !bounds.is_empty() && diag.is_finite() { 0.2 * diag / kmax } else { 1.0 };
    let scale = fixed_scale.unwrap_or(auto * length);
    let mut drawn = Vec::with_capacity(combs.len());
    let out: Vec<Value> = combs
        .iter()
        .map(|(id, teeth)| {
            let lines: Vec<[Vec3; 2]> = teeth.iter().map(|(q, k, n)| [*q, *q - *n * (k * scale)]).collect();
            let kmax_c = teeth.iter().map(|t| t.1.abs()).fold(0.0_f64, f64::max);
            let reply: Vec<[[f64; 3]; 2]> = lines.iter().map(|[a, b]| [(*a).into(), (*b).into()]).collect();
            let value = json!({
                "curve": id,
                "envelope": reply.iter().map(|l| l[1]).collect::<Vec<_>>(),
                "teeth": reply,
                "max_curvature": kmax_c,
                "min_radius": if kmax_c > 1e-12 { Value::from(1.0 / kmax_c) } else { Value::Null },
            });
            drawn.push(lines);
            value
        })
        .collect();
    s.comb = Some(CombView { picks: kept, density, length, fixed_scale, scale, teeth: drawn });
    s.revision += 1;
    Ok(json!({"combs": out, "scale": scale}))
}

/// Smallest radius along teeth: (radius, where), or None when straight.
fn min_of(teeth: &[(Vec3, f64, Vec3)]) -> Option<(f64, Vec3)> {
    teeth.iter().filter(|t| t.1.abs() > 1e-9).max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).map(|t| (1.0 / t.1.abs(), t.0))
}

/// The face of a body at `at` (nearest triangle): (body index, face index).
fn face_at(st: &solidcraft_doc::ModelState, at: Vec3) -> Option<(usize, u32)> {
    let mut best: Option<(f64, usize, u32)> = None;
    for (bi, b) in st.bodies.iter().enumerate() {
        let m = b.mesh();
        for (ti, t) in m.triangles.iter().enumerate() {
            let [a, bb, c] = t.map(|i| m.positions.get(i as usize).copied().unwrap_or_default());
            let d = at.dist(closest_on_triangle(at, a, bb, c));
            if best.is_none_or(|x| d < x.0) {
                best = Some((d, bi, m.tri_face.get(ti).copied().unwrap_or(0)));
            }
        }
    }
    best.filter(|b| b.0 < 1.0).map(|b| (b.1, b.2))
}

fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    // Project onto the plane, then clamp to the edges if outside.
    let n = (b - a).cross(c - a);
    let nn = n.dot(n);
    if nn < 1e-300 {
        return a;
    }
    let q = p - n * ((p - a).dot(n) / nn);
    let inside = |u: Vec3, v: Vec3| (v - u).cross(q - u).dot(n) >= 0.0;
    if inside(a, b) && inside(b, c) && inside(c, a) {
        return q;
    }
    let seg = |u: Vec3, v: Vec3| {
        let d = v - u;
        let t = if d.dot(d) > 1e-300 { ((p - u).dot(d) / d.dot(d)).clamp(0.0, 1.0) } else { 0.0 };
        u + d * t
    };
    [seg(a, b), seg(b, c), seg(c, a)].into_iter().min_by(|x, y| x.dist(p).total_cmp(&y.dist(p))).unwrap_or(a)
}

/// Smallest radius of curvature over a face of a mesh, from the turn of the vertex normals
/// along each triangle side: (radius, where).
fn face_min_radius(m: &solidcraft_geom::Mesh, face: u32) -> Option<(f64, Vec3)> {
    let mut best: Option<(f64, Vec3)> = None;
    for (ti, t) in m.triangles.iter().enumerate() {
        if m.tri_face.get(ti) != Some(&face) {
            continue;
        }
        for k in 0..3 {
            let (i, j) = (t[k] as usize, t[(k + 1) % 3] as usize);
            let (Some(pi), Some(pj), Some(ni), Some(nj)) = (m.positions.get(i), m.positions.get(j), m.normals.get(i), m.normals.get(j)) else {
                continue;
            };
            let d = *pj - *pi;
            let dd = d.dot(d);
            if dd < 1e-18 {
                continue;
            }
            let kappa = ((*nj - *ni).dot(d) / dd).abs();
            if kappa > 1e-6 && best.is_none_or(|b| 1.0 / kappa < b.0) {
                best = Some((1.0 / kappa, (*pi + *pj) * 0.5));
            }
        }
    }
    best
}

fn minimum_radius(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.minimum_radius";
    let st = s.model.state();
    let mut items: Vec<(String, Option<(f64, Vec3)>)> = Vec::new();
    let curves = string_list(p, "curves");
    if !curves.is_empty() {
        let id = match p.get("sketch") {
            Some(Value::Number(n)) => n.as_u64(),
            Some(Value::String(x)) => s.doc.find_feature(x).map(|f| f.id),
            _ => s.active_sketch,
        }
        .ok_or_else(|| bad(cmd, "no such sketch (and none is active)"))?;
        let ss = st.sketch(id).ok_or_else(|| EngineError::Other("that sketch is not evaluated".into()))?;
        for c in curves.iter().take(1000) {
            let ci = ss.sketch.curve_index(c).ok_or_else(|| bad(cmd, format!("unknown curve `{c}`")))?;
            let segs = ss.sketch.exact_segs(ci).unwrap_or_else(|| ss.sketch.segs(ci));
            let teeth: Vec<(Vec3, f64, Vec3)> =
                teeth_2d(&segs, 400).into_iter().map(|(q, k, n)| (ss.plane.to_world(q), k, ss.plane.dir_to_world(n))).collect();
            items.push((c.clone(), min_of(&teeth)));
        }
    }
    if let Some(edges) = p.get("edges").and_then(Value::as_array) {
        for e in edges.iter().take(1000) {
            let at = vec3(e).ok_or_else(|| bad(cmd, "`edges` must be points [x,y,z]"))?;
            let teeth = edge_teeth(&st, at, 400)?.ok_or_else(|| bad(cmd, "no edge there"))?;
            items.push((format!("edge@{},{},{}", at.x, at.y, at.z), min_of(&teeth)));
        }
    }
    let faces: Vec<Vec3> = match p.get("faces").and_then(Value::as_array) {
        Some(a) => a.iter().take(1000).map(|v| vec3(v).ok_or_else(|| bad(cmd, "`faces` must be points [x,y,z]"))).collect::<Result<_>>()?,
        None => Vec::new(),
    };
    for at in &faces {
        let (bi, f) = face_at(&st, *at).ok_or_else(|| bad(cmd, "no face there"))?;
        let m = st.bodies.get(bi).map(|b| b.mesh()).ok_or_else(|| bad(cmd, "body"))?;
        items.push((format!("face@{},{},{}", at.x, at.y, at.z), face_min_radius(&m, f)));
    }
    if items.is_empty() {
        // Every face of every body.
        for b in &st.bodies {
            let m = b.mesh();
            let n = m.tri_face.iter().copied().max().map(|x| x + 1).unwrap_or(0);
            for f in 0..n.min(100_000) {
                items.push((format!("{}:face{f}", b.name), face_min_radius(&m, f)));
            }
        }
    }
    let overall = items.iter().filter_map(|i| i.1).min_by(|a, b| a.0.total_cmp(&b.0));
    let fmt = |x: Option<(f64, Vec3)>| match x {
        Some((r, at)) => json!({"min_radius": r, "at": [at.x, at.y, at.z]}),
        None => json!({"min_radius": null}),
    };
    Ok(json!({
        "items": items.iter().map(|(id, x)| { let mut v = fmt(*x); v["item"] = json!(id); v }).collect::<Vec<_>>(),
        "min_radius": overall.map(|o| o.0),
        "at": overall.map(|o| [o.1.x, o.1.y, o.1.z]),
    }))
}

/// Isocurves each way when the call does not say (`count`), and most faces one call may pick.
const DEFAULT_ISO_COUNT: usize = 8;
const MAX_ISO_FACES: usize = 100;

/// The visible model body whose face is nearest `at` (within its pick tolerance), and that face.
fn iso_face_at<'a>(s: &Session, st: &'a solidcraft_doc::ModelState, at: Vec3) -> Option<(&'a solidcraft_doc::ModelBody, usize)> {
    st.bodies
        .iter()
        .filter(|b| s.body_visible(&b.name))
        .filter_map(|b| {
            let (d, f) = solidcraft_doc::appearance::nearest_face(b, at)?;
            (d <= solidcraft_doc::appearance::pick_tolerance(b)).then_some((d, b, f))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, b, f)| (b, f))
}

/// `inspect.isocurve`: the picked faces' isoparametric curves (`count` each way, exact and cut to
/// each face's trims, [`solidcraft_doc::ModelBody::iso_lines`]), kept so the viewport draws them.
/// View state only, like the other analyses: the design and its undo history are untouched.
fn isocurves(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.isocurve";
    if bool_(p, "clear").unwrap_or(false) {
        s.isocurves.clear();
        s.revision += 1;
        return Ok(json!({"analysis": "isocurve", "cleared": true}));
    }
    let max = crate::kernel::MAX_ISO_COUNT;
    let count = match p.get("count") {
        None | Some(Value::Null) => DEFAULT_ISO_COUNT,
        Some(v) => v
            .as_f64()
            .filter(|c| c.fract() == 0.0 && (1.0..=max as f64).contains(c))
            .map(|c| c as usize)
            .ok_or_else(|| bad(cmd, format!("`count` must be a whole number of curves each way, 1…{max}")))?,
    };
    let faces = p.get("faces").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`faces` must list points [x,y,z] on faces"))?;
    if faces.is_empty() || faces.len() > MAX_ISO_FACES {
        return Err(bad(cmd, format!("`faces` must list 1…{MAX_ISO_FACES} points [x,y,z] on faces")));
    }
    let st = s.world_state()?;
    let (mut picks, mut curves, mut shown) = (Vec::new(), Vec::new(), Vec::new());
    for v in faces {
        let at = vec3(v).ok_or_else(|| bad(cmd, "`faces` must list points [x,y,z] on faces"))?;
        let (body, face) = iso_face_at(s, &st, at).ok_or_else(|| bad(cmd, "no face there"))?;
        let lines = body.iso_lines(face, count)?;
        let along_u = lines.iter().filter(|l| l.along_u).count();
        shown.push(json!({"body": body.name, "face": face, "count": count, "u_curves": along_u, "v_curves": lines.len() - along_u}));
        curves.extend(lines.iter().map(|l| l.points.iter().map(|q| [q.x, q.y, q.z]).collect::<Vec<_>>()));
        picks.push((face, crate::IsoPick { body: body.name.clone(), at, count }));
    }
    if bool_(p, "add").unwrap_or(false) {
        // A face picked again shows its new count, once.
        let again = |k: &crate::IsoPick| {
            picks
                .iter()
                .any(|(f, n)| n.body == k.body && st.body(&k.body).and_then(|b| solidcraft_doc::appearance::face_index_at(b, k.at)) == Some(*f))
        };
        s.isocurves.retain(|k| !again(k));
    } else {
        s.isocurves.clear();
    }
    s.isocurves.extend(picks.into_iter().map(|(_, k)| k));
    s.revision += 1;
    Ok(json!({"curves": curves, "faces": shown}))
}

impl Session {
    /// The curves Isocurve Analysis shows, for the viewport to draw: those of each face in
    /// [`Session::isocurves`] of a visible body, found again from its picked point (worked out
    /// once per face and count, [`solidcraft_doc::ModelBody::iso_lines`]). A pick whose body or
    /// face is gone shows nothing.
    pub fn isocurve_lines(&self) -> Vec<std::sync::Arc<[crate::kernel::IsoLine]>> {
        if self.isocurves.is_empty() {
            return Vec::new();
        }
        let Ok(st) = self.world_state() else { return Vec::new() };
        self.isocurves
            .iter()
            .filter_map(|k| {
                let b = st.body(&k.body).filter(|b| self.body_visible(&b.name))?;
                let face = solidcraft_doc::appearance::face_index_at(b, k.at)?;
                b.iso_lines(face, k.count).ok()
            })
            .collect()
    }
}

fn center_of_mass(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.center_of_mass";
    let names = string_list(p, "bodies");
    let st = s.model.state();
    let mut out = Vec::new();
    let (mut vol, mut moment) = (0.0, Vec3::ZERO);
    let mut skipped = Vec::new();
    // Each body's exact mass properties, as `inspect.measure` takes them: a body that encloses no
    // volume (a surface body, an open mesh) or that cannot be measured has no centre of mass.
    for b in st.bodies.iter().filter(|b| names.is_empty() || names.contains(&b.name)) {
        if !super::inspect::encloses_volume(&b.body) {
            skipped.push(json!({"body": b.name, "reason": "it encloses no volume"}));
            continue;
        }
        let m = match b.measure() {
            Ok(m) if m.volume.is_finite() && m.centroid.is_finite() => m,
            Ok(_) => {
                skipped.push(json!({"body": b.name, "reason": "its measure is not finite"}));
                continue;
            }
            Err(e) => {
                skipped.push(json!({"body": b.name, "reason": e.to_string()}));
                continue;
            }
        };
        vol += m.volume;
        moment += m.centroid * m.volume;
        out.push(json!({"body": b.name, "center": [m.centroid.x, m.centroid.y, m.centroid.z], "volume": m.volume}));
    }
    if out.is_empty() {
        return Err(bad(
            cmd,
            match skipped.first() {
                Some(why) => {
                    format!("no body to measure: `{}`: {}", why["body"].as_str().unwrap_or_default(), why["reason"].as_str().unwrap_or_default())
                }
                None if names.is_empty() => "there are no bodies".to_string(),
                None => "no such bodies".to_string(),
            },
        ));
    }
    let c = if vol.abs() > 1e-300 { moment / vol } else { Vec3::ZERO };
    Ok(json!({"center": [c.x, c.y, c.z], "volume": vol, "bodies": out, "skipped": skipped}))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::Session;

    #[test]
    fn curvature_combs() {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        let c = s.execute("sketch.circle.center", &json!({"center": [0, 0], "radius": 5})).unwrap()["curves"][0].clone();
        let l = s.execute("sketch.line", &json!({"points": [[0, 20], [10, 20]]})).unwrap()["curves"][0].clone();
        let r = s.execute("inspect.curvature_comb", &json!({"curves": [c, l], "density": 20})).unwrap();
        let circle = &r["combs"][0];
        assert!((circle["min_radius"].as_f64().unwrap() - 5.0).abs() < 1e-3, "{circle}");
        assert!(r["combs"][1]["min_radius"].is_null(), "a line is straight");
        // Circle teeth stand out on the convex side, away from the centre.
        let t = &circle["teeth"][3];
        let (a, b) = ((t[0][0].as_f64().unwrap(), t[0][1].as_f64().unwrap()), (t[1][0].as_f64().unwrap(), t[1][1].as_f64().unwrap()));
        assert!(b.0 * b.0 + b.1 * b.1 > a.0 * a.0 + a.1 * a.1);
        s.execute("sketch.finish", &json!({})).unwrap();
        s.execute("solid.cylinder", &json!({"base": [50, 0, 0], "radius": 8, "height": 10})).unwrap();
        let r = s.execute("inspect.curvature_comb", &json!({"edges": [[58, 0, 10]]})).unwrap();
        assert!((r["combs"][0]["min_radius"].as_f64().unwrap() - 8.0).abs() < 1e-9, "{r}");
        assert!(s.execute("inspect.curvature_comb", &json!({})).is_err());
        // Combs along the side face's isocurves: rings of radius 8 and straight lines along it.
        let r = s.execute("inspect.curvature_comb", &json!({"faces": [[58, 0, 5]], "count": 3})).unwrap();
        let combs = r["combs"].as_array().unwrap();
        assert!(combs.len() >= 6, "{}", combs.len());
        let rings = combs.iter().filter(|c| c["min_radius"].as_f64().is_some_and(|m| (m - 8.0).abs() < 0.5)).count();
        let straight = combs.iter().filter(|c| c["min_radius"].is_null() || c["min_radius"].as_f64().is_some_and(|m| m > 1e3)).count();
        assert!(rings >= 3 && straight >= 3, "{rings} rings, {straight} straight: {r}");
        assert!(s.execute("inspect.curvature_comb", &json!({"faces": []})).is_err());
        assert!(s.execute("inspect.curvature_comb", &json!({"faces": [[58, 0, 5]], "count": "x"})).is_err());
    }

    /// The combs shown are kept in the session: `add` adds to them and draws them all again at
    /// one scale (an R5 circle's teeth twice an R10 circle's), with nothing picked it takes new
    /// options, a sketch curve keeps its own sketch, and `clear` turns the analysis off.
    #[test]
    fn added_combs_share_one_scale_and_clear_turns_them_off() {
        use solidcraft_geom::Vec3;
        let mut s = Session::default();
        s.execute("solid.cylinder", &json!({"radius": 5, "height": 10})).unwrap();
        s.execute("solid.cylinder", &json!({"base": [40, 0, 0], "radius": 10, "height": 10})).unwrap();
        let one = s.execute("inspect.curvature_comb", &json!({"edges": [[0, 5, 10]], "density": 8})).unwrap();
        let both = s.execute("inspect.curvature_comb", &json!({"edges": [[40, 10, 10]], "add": true})).unwrap();
        let shown = s.comb.as_ref().unwrap();
        assert_eq!((shown.picks.len(), shown.teeth.len(), shown.density), (2, 2, 8), "the first call's density is kept");
        assert_eq!(both["combs"].as_array().unwrap().len(), 2);
        let tooth = |c: &[[Vec3; 2]]| c[0][0].dist(c[0][1]);
        assert!((tooth(&shown.teeth[0]) / tooth(&shown.teeth[1]) - 2.0).abs() < 1e-9);
        assert_ne!(one["scale"], both["scale"], "the scale is the pair's, not the first comb's");
        let longer = s.execute("inspect.curvature_comb", &json!({"add": true, "length": 2})).unwrap();
        assert!((longer["scale"].as_f64().unwrap() - 2.0 * both["scale"].as_f64().unwrap()).abs() < 1e-9);
        assert_eq!(s.comb.as_ref().unwrap().picks.len(), 2);
        // A sketch curve keeps its sketch after another becomes active.
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        let c = s.execute("sketch.circle.center", &json!({"center": [0, 30], "radius": 5})).unwrap()["curves"][0].clone();
        s.execute("inspect.curvature_comb", &json!({"curves": [c], "add": true})).unwrap();
        s.execute("sketch.finish", &json!({})).unwrap();
        s.execute("sketch.create", &json!({"plane": "XZ"})).unwrap();
        let r = s.execute("inspect.curvature_comb", &json!({"add": true})).unwrap();
        assert_eq!(r["combs"].as_array().unwrap().len(), 3, "{r}");
        s.execute("inspect.curvature_comb", &json!({"clear": true})).unwrap();
        assert!(s.comb.is_none());
        let on = s.execute("inspect.curvature_comb", &json!({"add": true})).unwrap();
        assert!(on["combs"].as_array().unwrap().is_empty() && s.comb.as_ref().is_some_and(|v| v.picks.is_empty()));
    }

    /// Each tooth of a comb reply as (foot, curvature): the tooth's length over the scale.
    fn tooth_curvatures(r: &Value, comb: usize) -> Vec<([f64; 3], f64)> {
        let scale = r["scale"].as_f64().unwrap();
        r["combs"][comb]["teeth"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                let [a, b] = [0, 1].map(|i| [0, 1, 2].map(|k| t[i][k].as_f64().unwrap()));
                (a, (0..3).map(|k| (b[k] - a[k]).powi(2)).sum::<f64>().sqrt() / scale)
            })
            .collect()
    }

    /// An edge's comb is its exact curve's: on a cylinder's circular edge of radius R every tooth
    /// reads 1/R to 1e-9, at any display quality (the comb never reads the mesh), the teeth stand
    /// out of the circle (away from its centre), and `density` is the number of teeth.
    #[test]
    fn an_edge_comb_is_exact_and_has_density_teeth() {
        use solidcraft_doc::display_quality::{DisplayQuality, set_current};
        let mut s = Session::default();
        s.execute("solid.cylinder", &json!({"base": [50, 0, 0], "radius": 0.75, "height": 40})).unwrap();
        let mut feet = Vec::new();
        for quality in [DisplayQuality::Coarse, DisplayQuality::Ultra] {
            set_current(quality);
            let r = s.execute("inspect.curvature_comb", &json!({"edges": [[50.75, 0, 40]], "density": 7})).unwrap();
            let teeth = tooth_curvatures(&r, 0);
            assert_eq!(teeth.len(), 7, "{r}");
            for (foot, k) in &teeth {
                assert!((k - 1.0 / 0.75).abs() < 1e-9, "{k} at {foot:?}");
                assert!(((foot[0] - 50.0).hypot(foot[1]) - 0.75).abs() < 1e-9 && (foot[2] - 40.0).abs() < 1e-9, "{foot:?}");
            }
            let tip = &r["combs"][0]["envelope"][2];
            assert!((tip[0].as_f64().unwrap() - 50.0).hypot(tip[1].as_f64().unwrap()) > 0.75, "outside the circle");
            assert!((r["combs"][0]["max_curvature"].as_f64().unwrap() - 1.0 / 0.75).abs() < 1e-9);
            feet.push(r["combs"][0]["teeth"].clone());
        }
        set_current(DisplayQuality::Normal);
        assert_eq!(feet[0], feet[1], "the same comb at any display quality");
        let r = s.execute("inspect.curvature_comb", &json!({"edges": [[50.75, 0, 0]], "density": 123})).unwrap();
        assert_eq!(r["combs"][0]["teeth"].as_array().unwrap().len(), 123);
        for density in [json!("x"), json!(-5), json!(1e300)] {
            assert!(s.execute("inspect.curvature_comb", &json!({"edges": [[50.75, 0, 0]], "density": density})).is_ok(), "{density}: clamped");
        }
    }

    /// An ellipse's curvature, a b / (b² cos² t + a² sin² t)^{3/2} at angle t, is a/b² at the ends of
    /// its major axis and b/a² at those of its minor one: the comb of an extruded ellipse's edge
    /// reads it at every tooth, the ends included.
    #[test]
    fn an_ellipse_edge_comb_matches_the_closed_form() {
        let (a, b) = (10.0_f64, 4.0_f64);
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        s.execute("sketch.ellipse", &json!({"center": [0, 0], "major": [a, 0], "minor_radius": b})).unwrap();
        s.execute("sketch.finish", &json!({})).unwrap();
        s.execute("solid.extrude", &json!({"distance": 5})).unwrap();
        let r = s.execute("inspect.curvature_comb", &json!({"edges": [[0, b, 5]], "density": 40})).unwrap();
        let exact = |p: [f64; 3]| {
            let (c, sn) = (p[0] / a, p[1] / b);
            a * b / (b * b * c * c + a * a * sn * sn).powf(1.5)
        };
        let teeth = tooth_curvatures(&r, 0);
        assert!(teeth.len() >= 20, "{r}");
        for (foot, k) in &teeth {
            assert!(((foot[0] / a).hypot(foot[1] / b) - 1.0).abs() < 1e-9, "on the ellipse: {foot:?}");
            assert!((k - exact(*foot)).abs() < 1e-9 * exact(*foot), "{k} vs {} at {foot:?}", exact(*foot));
        }
        // The vertices: the teeth at the ends of the axes, wherever the edge's parameter puts them.
        let at = |q: [f64; 2]| teeth.iter().find(|(f, _)| (f[0] - q[0]).hypot(f[1] - q[1]) < 1e-9).map(|t| t.1);
        let major = [[a, 0.0], [-a, 0.0]].into_iter().filter_map(at).collect::<Vec<_>>();
        let minor = [[0.0, b], [0.0, -b]].into_iter().filter_map(at).collect::<Vec<_>>();
        assert!(!major.is_empty() && !minor.is_empty(), "teeth at the vertices: {teeth:?}");
        assert!(major.iter().all(|k| (k - a / (b * b)).abs() < 1e-9), "{major:?}");
        assert!(minor.iter().all(|k| (k - b / (a * a)).abs() < 1e-12), "{minor:?}");
        assert!((r["combs"][0]["max_curvature"].as_f64().unwrap() - a / (b * b)).abs() < 1e-9, "{r}");
    }

    /// A face comb follows the surface: on a cylinder's side the iso curves round it read 1/R at
    /// every tooth and those along it 0, the teeth along the surface normal; `scale` sets the teeth's
    /// length per unit curvature and `length` multiplies the automatic one.
    #[test]
    fn a_face_comb_is_the_normal_curvature_and_scale_sets_its_length() {
        let mut s = Session::default();
        s.execute("solid.cylinder", &json!({"radius": 6, "height": 30})).unwrap();
        let r = s.execute("inspect.curvature_comb", &json!({"faces": [[6, 0, 15]], "count": 3, "density": 9})).unwrap();
        let combs = r["combs"].as_array().unwrap();
        let (mut rings, mut lines) = (0, 0);
        for (i, comb) in combs.iter().enumerate() {
            let teeth = tooth_curvatures(&r, i);
            assert_eq!(teeth.len(), 9, "{comb}");
            if teeth.iter().all(|(_, k)| *k == 0.0) {
                lines += 1;
                assert!(comb["min_radius"].is_null());
            } else {
                rings += 1;
                assert!(teeth.iter().all(|(_, k)| (k - 1.0 / 6.0).abs() < 1e-9), "{teeth:?}");
                // Out of the surface, along its normal (radially, at the tooth's height).
                for t in comb["teeth"].as_array().unwrap() {
                    let q = |i: usize, k: usize| t[i][k].as_f64().unwrap();
                    assert!((q(1, 2) - q(0, 2)).abs() < 1e-9 && q(1, 0).hypot(q(1, 1)) > 6.0 + 1e-6, "{t}");
                }
            }
        }
        assert_eq!((rings, lines), (3, 3), "{r}");
        // Tooth length is curvature × scale.
        let tooth = |r: &Value| tooth_curvatures(r, 0)[0].1 * r["scale"].as_f64().unwrap();
        let auto = s.execute("inspect.curvature_comb", &json!({"faces": [[6, 0, 15]], "count": 3})).unwrap();
        let fixed = s.execute("inspect.curvature_comb", &json!({"faces": [[6, 0, 15]], "count": 3, "scale": 12})).unwrap();
        assert!((tooth(&fixed) - 2.0).abs() < 1e-9, "12 × 1/6: {}", tooth(&fixed));
        let longer = s.execute("inspect.curvature_comb", &json!({"faces": [[6, 0, 15]], "count": 3, "length": 3})).unwrap();
        assert!((longer["scale"].as_f64().unwrap() - 3.0 * auto["scale"].as_f64().unwrap()).abs() < 1e-9);
        assert!((tooth(&longer) - 3.0 * tooth(&auto)).abs() < 1e-9);
        for bad in [json!(0), json!(-1), json!(1000), json!("2")] {
            assert!(s.execute("inspect.curvature_comb", &json!({"faces": [[6, 0, 15]], "length": bad})).is_err(), "{bad}");
        }
    }

    #[test]
    fn minimum_radius_and_center_of_mass() {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        let e = s.execute("sketch.ellipse", &json!({"center": [0, 0], "major": [10, 0], "minor_radius": 4})).unwrap()["curves"][0].clone();
        // An ellipse's tightest bend is at the major axis ends: b²/a.
        let r = s.execute("inspect.minimum_radius", &json!({"curves": [e]})).unwrap();
        assert!((r["min_radius"].as_f64().unwrap() - 1.6).abs() < 1e-3, "{r}");
        assert!((r["at"][0].as_f64().unwrap().abs() - 10.0).abs() < 0.1, "{r}");
        s.execute("sketch.finish", &json!({})).unwrap();
        s.execute("solid.cylinder", &json!({"base": [50, 0, 0], "radius": 8, "height": 10})).unwrap();
        // The cylinder's side face: radius 8 (the mesh estimate is close).
        let r = s.execute("inspect.minimum_radius", &json!({"faces": [[58, 0, 5]]})).unwrap();
        assert!((r["min_radius"].as_f64().unwrap() - 8.0).abs() < 0.4, "{r}");
        let r = s.execute("inspect.minimum_radius", &json!({})).unwrap();
        assert!((r["min_radius"].as_f64().unwrap() - 8.0).abs() < 0.4, "{r}");
        // Centre of mass: a box beside the cylinder.
        s.execute("solid.box", &json!({"corner": [0, 0, 0], "length": 10, "width": 10, "height": 10})).unwrap();
        let r = s.execute("inspect.center_of_mass", &json!({})).unwrap();
        let (vb, vc) = (1000.0, std::f64::consts::PI * 640.0);
        let want = (5.0 * vb + 50.0 * vc) / (vb + vc);
        // From the exact faces, not the display mesh, whose chords cut the cylinder short.
        assert!((r["center"][0].as_f64().unwrap() - want).abs() < 1e-6 * want, "{r}");
        assert!((r["center"][2].as_f64().unwrap() - 5.0).abs() < 1e-9, "{r}");
        assert!((r["bodies"][0]["volume"].as_f64().unwrap() - vc).abs() < 1e-6 * vc, "{r}");
        assert!((r["volume"].as_f64().unwrap() - (vb + vc)).abs() < 1e-6 * (vb + vc), "{r}");
        assert!(s.execute("inspect.center_of_mass", &json!({"bodies": ["nope"]})).is_err());
        // Isocurves on the cylinder's side: rings around it and lines along it.
        let r = s.execute("inspect.isocurve", &json!({"faces": [[58, 0, 5]], "count": 4})).unwrap();
        let curves = r["curves"].as_array().unwrap();
        let ring =
            |c: &Value| c.as_array().unwrap().iter().all(|q| ((q[0].as_f64().unwrap() - 50.0).hypot(q[1].as_f64().unwrap()) - 8.0).abs() < 1e-6);
        assert!(curves.len() >= 8, "{}", curves.len());
        assert!(curves.iter().all(ring), "every curve lies on the cylinder");
        let flat = |c: &Value| {
            let z: Vec<f64> = c.as_array().unwrap().iter().map(|q| q[2].as_f64().unwrap()).collect();
            z.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - z.iter().cloned().fold(f64::INFINITY, f64::min) < 1e-6
        };
        assert!(curves.iter().filter(|c| flat(c)).count() >= 4, "rings at constant height");
        assert!(s.execute("inspect.isocurve", &json!({"faces": []})).is_err());
    }

    /// Points of a reply's `curves`.
    fn curve_points(r: &Value) -> Vec<Vec<[f64; 3]>> {
        r["curves"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_array().unwrap().iter().map(|q| [0, 1, 2].map(|k| q[k].as_f64().unwrap())).collect())
            .collect()
    }

    /// Isocurve Analysis draws a face's exact isoparametric curves, `count` each way, cut to its
    /// trims: on a cylinder's side they lie on the cylinder and the lines along it stop where they
    /// meet a hole through it; the view state keeps the face, and hostile counts are refused.
    #[test]
    fn isocurves_are_exact_and_stop_at_a_hole() {
        let mut s = Session::default();
        s.execute("solid.cylinder", &json!({"radius": 10, "height": 40})).unwrap();
        // Untrimmed but for its ends: `count` curves each way, each one piece.
        let r = s.execute("inspect.isocurve", &json!({"faces": [[10, 0, 20]], "count": 5})).unwrap();
        let face = &r["faces"][0];
        assert_eq!((face["u_curves"].as_u64(), face["v_curves"].as_u64()), (Some(5), Some(5)), "{face}");
        assert_eq!(s.isocurves.len(), 1);
        assert_eq!(s.isocurve_lines().iter().map(|l| l.len()).sum::<usize>(), 10);
        // A hole of radius 3 through it along X, at half height.
        s.execute("solid.cylinder", &json!({"radius": 3, "height": 30, "base": [-15, 0, 20], "axis": "X", "operation": "cut"})).unwrap();
        let r = s.execute("inspect.isocurve", &json!({"faces": [[0, 10, 20]], "count": 9})).unwrap();
        let curves = curve_points(&r);
        assert!(!curves.is_empty(), "{r}");
        let from_hole = |q: &[f64; 3]| q[1].hypot(q[2] - 20.0) - 3.0;
        for c in &curves {
            for q in c {
                assert!((q[0].hypot(q[1]) - 10.0).abs() < 1e-6, "on the cylinder: {q:?}");
                assert!(from_hole(q) > -1e-3, "not in the hole: {q:?}");
                assert!((-1e-6..=40.0 + 1e-6).contains(&q[2]), "within its ends: {q:?}");
            }
        }
        // The lines along the axis that cross the hole stop at its rim: their ends there lie on
        // the hole's edge.
        let mut at_rim = 0;
        for c in &curves {
            for q in [c.first().unwrap(), c.last().unwrap()] {
                if q[2] > 1e-3 && q[2] < 40.0 - 1e-3 && from_hole(q) < 0.5 {
                    assert!(from_hole(q).abs() < 1e-3, "an end on the hole's edge: {q:?} is {} from it", from_hole(q));
                    at_rim += 1;
                }
            }
        }
        assert!(at_rim >= 4, "the lines crossing the hole stop at it ({at_rim} ends there)");
        // Picking the face again replaces it; `add` keeps the others; `clear` ends it.
        s.execute("inspect.isocurve", &json!({"faces": [[0, -10, 5]], "count": 3, "add": true})).unwrap();
        let shown = s.isocurves.len();
        s.execute("inspect.isocurve", &json!({"faces": [[0, -10, 5]], "count": 4, "add": true})).unwrap();
        assert_eq!(s.isocurves.len(), shown, "the same face again replaces its curves");
        assert_eq!(s.isocurves.last().map(|k| k.count), Some(4));
        let before = s.doc.clone();
        s.execute("inspect.isocurve", &json!({"clear": true})).unwrap();
        assert!(s.isocurves.is_empty() && s.isocurve_lines().is_empty());
        assert!(std::sync::Arc::ptr_eq(&before, &s.doc), "view state only: the design is untouched");
        // Hostile counts and picks.
        for count in [json!(0), json!(-3), json!(2.5), json!(101), json!(1e300), json!("8"), json!([4])] {
            assert!(s.execute("inspect.isocurve", &json!({"faces": [[0, 10, 20]], "count": count})).is_err(), "count {count}");
        }
        assert!(s.execute("inspect.isocurve", &json!({"faces": [[500, 0, 0]]})).is_err(), "no face there");
        assert!(s.execute("inspect.isocurve", &json!({"faces": [["x", 0, 0]]})).is_err());
        assert!(s.execute("inspect.isocurve", &json!({"faces": vec![[0, 10, 20]; 101]})).is_err());
        assert!(s.isocurves.is_empty(), "a refused call shows nothing");
    }

    /// A sphere's face is closed by its seam and poles: its curves are the circles round it and
    /// the meridians, each meridian from pole to pole.
    #[test]
    fn isocurves_of_a_sphere_meet_at_its_poles() {
        let mut s = Session::default();
        s.execute("solid.sphere", &json!({"radius": 10})).unwrap();
        let r = s.execute("inspect.isocurve", &json!({"faces": [[10, 0, 0]], "count": 5})).unwrap();
        let face = &r["faces"][0];
        assert_eq!((face["u_curves"].as_u64(), face["v_curves"].as_u64()), (Some(5), Some(5)), "{face}");
        let curves = curve_points(&r);
        let len = |q: &[f64; 3]| q[0].hypot(q[1]).hypot(q[2]);
        assert!(curves.iter().flatten().all(|q| (len(q) - 10.0).abs() < 1e-6), "on the sphere");
        let pole = |q: &[f64; 3]| q[0].hypot(q[1]) < 1e-6 && (q[2].abs() - 10.0).abs() < 1e-6;
        let meridians = curves.iter().filter(|c| pole(c.first().unwrap()) && pole(c.last().unwrap())).count();
        assert_eq!(meridians, 5, "the meridians run from pole to pole");
    }

    /// On a trimmed B-spline face (a spline profile's extruded side, with a hole through it: an
    /// inner loop) the isocurves are the surface's own rows and columns and stay inside the face's
    /// loops.
    #[test]
    fn isocurves_of_a_trimmed_spline_face_stay_inside_it() {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        s.execute("sketch.spline.fit_point", &json!({"points": [[0, 0], [20, 8], [40, 0]]})).unwrap();
        s.execute("sketch.line", &json!({"points": [[40, 0], [0, 0]]})).unwrap();
        s.execute("sketch.finish", &json!({})).unwrap();
        s.execute("solid.extrude", &json!({"distance": 30})).unwrap();
        s.execute("solid.cylinder", &json!({"radius": 3, "height": 30, "base": [28, -10, 15], "axis": "Y", "operation": "cut"})).unwrap();
        let r = s.execute("inspect.isocurve", &json!({"faces": [[28, 6.7, 5]], "count": 12})).unwrap();
        let face = &r["faces"][0];
        assert!(face["u_curves"].as_u64().unwrap() >= 12 && face["v_curves"].as_u64().unwrap() >= 12, "{face}");
        let st = s.model.state();
        let body = &st.bodies[0];
        let mesh = body.mesh();
        let tol = body.display_tol();
        let spline = face["face"].as_u64().unwrap() as u32;
        for c in curve_points(&r) {
            for q in &c {
                let p = solidcraft_geom::Vec3::new(q[0], q[1], q[2]);
                // On that face (within the display mesh's sag), within its extent, clear of the hole.
                let d = mesh
                    .triangles
                    .iter()
                    .zip(&mesh.tri_face)
                    .filter(|(_, f)| **f == spline)
                    .filter_map(|(t, _)| mesh.tri(t))
                    .map(|[a, b, c]| super::closest_on_triangle(p, a, b, c).dist(p))
                    .fold(f64::INFINITY, f64::min);
                assert!(d < 2.0 * tol, "on the spline face: {q:?} is {d} from it");
                assert!((-1e-6..=30.0 + 1e-6).contains(&q[2]) && q[1] > -1e-6, "{q:?}");
                assert!((q[0] - 28.0).hypot(q[2] - 15.0) > 3.0 - 1e-3, "not in the hole: {q:?}");
            }
        }
    }
}
