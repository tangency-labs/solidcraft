//! Evaluation of Emboss, Rib/Web, Replace Face, Align and Remove (features built from the
//! kernel's extrude, boolean, offset and transform operations).

use super::*;
use crate::EmbossFace;
use crate::apply_point;
use crate::sheet;
use solidcore_record::{Loop2, Seg2};
use solidcraft_geom::{Aabb3, Ring};

pub(super) fn eval(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState, warning: &mut Option<String>) -> Result<()> {
    match &f.kind {
        FeatureKind::Emboss { sketch, profiles, depth, deboss, targets, face } => {
            let ss = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?;
            let regions = select_profiles(ss, profiles)?;
            let d = val(vals, depth, Kind::Length)?;
            if !(d > 0.0) {
                return Err(DocError::Invalid("the emboss depth must be positive".into()));
            }
            if let Some(face) = face {
                if !targets.is_empty() && (targets.len() != 1 || targets.first() != Some(&face.body)) {
                    return Err(DocError::Invalid("cylinder emboss must keep its selected target body".into()));
                }
                let i =
                    st.bodies.iter().position(|b| b.name == face.body).ok_or_else(|| DocError::Unknown(format!("emboss body `{}`", face.body)))?;
                let mb = st.bodies.get(i).cloned().ok_or_else(|| DocError::Invalid("emboss body".into()))?;
                let body_frame = face.body_frame(doc, &mb)?;
                let inverse = crate::mat_inverse(&body_frame).ok_or_else(|| DocError::Invalid("invalid cylinder body frame".into()))?;
                let sketch_owner = doc.feature(ss.feature).map(|f| f.component).unwrap_or(0);
                let author_frame = EmbossFace::component_frame(doc, f.component, &face.author_path)?;
                let sketch_frame = face.sketch_frame(doc, f.component, sketch_owner, &mb)?;
                let sketch_transform = crate::mat_mul(&inverse, &sketch_frame);
                let plane = Plane::new(
                    crate::apply_point(&sketch_transform, ss.plane.origin),
                    crate::apply_vector(&sketch_transform, ss.plane.x),
                    crate::apply_vector(&sketch_transform, ss.plane.y),
                )
                .ok_or_else(|| DocError::Invalid("invalid cylinder sketch frame".into()))?;
                let mut at = crate::apply_point(&crate::mat_mul(&inverse, &author_frame), face.at.0);
                let name = f
                    .face_names
                    .first()
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| DocError::Invalid("selected cylinder face has no persistent identity".into()))?;
                {
                    let mut scoped = ModelState::default();
                    scoped.bodies.push(mb.clone());
                    let hit = crate::name_lookup::locate_face(&scoped, name, at).ok_or_else(|| DocError::reselect(name.as_str()))?;
                    at = kernel::cylinder_emboss_anchor(&mb.body, hit.face, hit.point)?;
                }
                let result = kernel::emboss_cylinder_at(&mb.body, at, &plane, &regions, if *deboss { -d } else { d })?;
                if let Some(slot) = st.bodies.get_mut(i) {
                    *slot = ModelBody::new(mb.name, result, mb.feature);
                }
                return Ok(());
            }
            let (a, b) = if *deboss { (-d, 0.0) } else { (0.0, d) };
            let tools = kernel::extrude(&ss.plane, &regions, a, b)?;
            if st.bodies.is_empty() {
                return Err(DocError::Invalid("there is no body to emboss".into()));
            }
            // #10940: an emboss or deboss whose tool lies clear of the body (a sketch on the far
            // side of the material, a depth that stops short) used to report success and change
            // nothing. Say so: the body is compared before and after, as the command's reply does.
            let before: Vec<Body> = st.bodies.iter().map(|b| b.body.clone()).collect();
            apply_op(doc, st, f, tools, if *deboss { Operation::Cut } else { Operation::Join }, &crate::plain(targets))?;
            let unchanged = st.bodies.len() == before.len() && st.bodies.iter().zip(&before).all(|(b, old)| old.same(&b.body));
            if unchanged {
                let what = if *deboss { "deboss" } else { "emboss" };
                append_warning(
                    warning,
                    format!("the {what} changed nothing; check where its profile lies, which side of the face the sketch is on, and the depth"),
                );
            }
            Ok(())
        }
        FeatureKind::Rib { .. } => {
            let tools = rib_tools(vals, st, f, None)?;
            apply_op(doc, st, f, tools, Operation::Join, &[])
        }
        FeatureKind::ReplaceFace { faces, target, body } => {
            let tp = doc.resolve_plane_in(vals, target, 0, Some(st))?;
            let tn = tp.normal();
            let i = body_at(st, body, &crate::plain(faces))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            // Each face moved is a step of the body's lineage, as Offset Face's is: without it no face of
            // the result could take a sketch's attachment.
            let mut b = mb.traced();
            for p in faces {
                let n = planar_normal_at(&b.body, p.0)
                    .ok_or_else(|| DocError::Invalid("not supported yet: replacing a face that is not planar".into()))?;
                if n.dot(tn).abs() < 1.0 - 1e-9 {
                    return Err(DocError::Invalid("not supported yet: a target that is not parallel to the face".into()));
                }
                let d = (tp.origin - p.0).dot(n);
                if d.abs() > 1e-9 {
                    b = Traced::step(kernel::offset_faces(&b.body, &[p.0], d)?, &[&b]);
                }
            }
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::traced_new(mb.name, b, mb.feature);
            }
            Ok(())
        }
        FeatureKind::Align { bodies, from, to, from_normal, to_normal, flip, from_plane, to_plane, axes, from_frame, to_frame, angle } => {
            // A plane on either side is worked out now, so the bodies follow it when it moves.
            let (mut from, mut from_normal) = (from.0, from_normal.and_then(|n| n.normalized()));
            if let Some(p) = from_plane {
                let pl = doc.resolve_plane_in(vals, p, 0, Some(&*st))?;
                (from, from_normal) = (pl.origin, Some(pl.normal()));
            }
            // So is a frame (#6257): its origin and z, found again from the geometry.
            if let Some(fr) = from_frame {
                let (o, z) = fr.resolve(doc, st)?;
                (from, from_normal) = (o, Some(z));
            }
            let target = match to_frame {
                Some(_) => None,
                None => to_plane.as_ref().map(|p| doc.resolve_plane_in(vals, p, 0, Some(&*st))).transpose()?,
            };
            let (to, to_normal) = match (to_frame, &target) {
                (Some(fr), _) => fr.resolve(doc, st).map(|(o, z)| (o, Some(z)))?,
                (None, Some(pl)) => (pl.origin, Some(pl.normal())),
                (None, None) => (to.0, to_normal.and_then(|n| n.normalized())),
            };
            let mut m = translation(Vec3::ZERO);
            if let (Some(a), Some(b)) = (from_normal, to_normal) {
                // Turn `a` onto −b (faces meet) or b (flipped), about the `from` point.
                let want = if *flip { b } else { b * -1.0 };
                let axis = a.cross(want);
                let ang = axis.len().atan2(a.dot(want));
                let axis = axis.normalized().unwrap_or_else(|| a.any_perp());
                if ang.abs() > 1e-12 {
                    m = rotation(from, axis, ang);
                }
            }
            // Onto a plane the point goes straight along the plane's normal (nothing slides
            // across it); onto a point it lands on it.
            let d = match (&target, to_normal) {
                (Some(_), Some(n)) => n * (to - from).dot(n),
                _ => to - from,
            };
            // Point-to-point alignment can keep only the chosen world X/Y/Z directions,
            // stored in the bodies' frame. A missing list means all three (the full offset).
            let d = match axes {
                None => d,
                Some(dirs) if dirs.is_empty() => {
                    return Err(DocError::Invalid("the align axes enable none of X, Y and Z".into()));
                }
                Some(dirs) => {
                    let mut kept = Vec3::ZERO;
                    let mut used = false;
                    for a in dirs {
                        if let Some(a) = a.0.normalized() {
                            kept += a * d.dot(a);
                            used = true;
                        }
                    }
                    if !used {
                        return Err(DocError::Invalid("the align axes have no direction".into()));
                    }
                    kept
                }
            };
            m[3][0] += d.x;
            m[3][1] += d.y;
            m[3][2] += d.z;
            // Then the turn about the target's direction, through where `from` landed.
            if let Some(e) = angle {
                let a = val(vals, e, Kind::Angle)?;
                if !a.is_finite() {
                    return Err(DocError::Invalid("the Align angle is not a finite number".into()));
                }
                let n = to_normal.ok_or_else(|| {
                    DocError::Invalid("an Align angle turns about the target's direction: move onto a frame, a face or a plane".into())
                })?;
                if a.abs() > 1e-12 {
                    m = crate::mat_mul(&rotation(apply_point(&m, from), n, a), &m);
                }
            }
            if bodies.is_empty() {
                return Err(DocError::Invalid("no bodies to align".into()));
            }
            for n in bodies {
                let i = st.bodies.iter().position(|b| &b.name == n).ok_or_else(|| DocError::Unknown(format!("body `{n}`")))?;
                if let Some(mb) = st.bodies.get_mut(i) {
                    let nb = kernel::transform_matrix(&mb.body, m)?;
                    *mb = ModelBody::new(mb.name.clone(), nb, mb.feature);
                }
            }
            Ok(())
        }
        FeatureKind::Remove { bodies } => {
            if bodies.is_empty() {
                return Err(DocError::Invalid("no bodies to remove".into()));
            }
            for n in bodies {
                if st.body(n).is_none() {
                    return Err(DocError::Unknown(format!("body `{n}`")));
                }
            }
            st.bodies.retain(|b| !bodies.iter().any(|t| *t == b.name));
            Ok(())
        }
        _ => Err(DocError::Invalid(format!("{} is not evaluated here", f.name))),
    }
}

/// The walls of rib `f` (one per curve for a web), each run until it meets the bodies. `at`
/// moves the rib's sketch first: a pattern's or mirror's copy of the rib is built again where
/// the copy is, against the bodies there (#4593), as a hole is drilled again at each copy.
pub(super) fn rib_tools(vals: &BTreeMap<String, Value>, st: &ModelState, f: &Feature, at: Option<&crate::Mat>) -> Result<Vec<Body>> {
    let FeatureKind::Rib { sketch, curves, thickness, depth, flip, web, normal } = &f.kind else {
        return Err(DocError::Invalid(format!("{} is not a rib", f.name)));
    };
    let mut ss = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?.clone();
    if let Some(m) = at {
        let p = ss.plane;
        ss.plane = Plane::new(apply_point(m, p.origin), crate::apply_vector(m, p.x), crate::apply_vector(m, p.y))
            .ok_or_else(|| DocError::Invalid("the rib's sketch plane at the copy".into()))?;
    }
    let t = val(vals, thickness, Kind::Length)?;
    if !(t > 0.0) {
        return Err(DocError::Invalid("the rib thickness must be positive".into()));
    }
    let depth = match depth {
        Some(e) => Some(val(vals, e, Kind::Length)?)
            .filter(|d| *d > 0.0)
            .ok_or_else(|| DocError::Invalid("the rib depth must be positive".into()))
            .map(Some)?,
        None => None,
    };
    let groups: Vec<Vec<String>> = if *web { curves.iter().map(|c| vec![c.clone()]).collect() } else { vec![curves.clone()] };
    if groups.is_empty() || curves.is_empty() || curves.len() > 1000 {
        return Err(DocError::Invalid("a rib needs 1…1000 sketch curves".into()));
    }
    let mut tools = Vec::new();
    for g in &groups {
        tools.push(if *normal { rib_tool_normal(st, &ss, g, t, depth, *flip)? } else { rib_tool(st, &ss, g, t, depth, *flip)? });
    }
    Ok(tools)
}

/// Unit outward normal of the planar face through `p`: of the planar faces whose plane runs
/// through it, the one nearest it. A face elsewhere in such a plane (the wall of a notch, the end
/// of a rib, in line with a rim) is no pick of `p`, though its plane runs through it as exactly.
fn planar_normal_at(b: &Body, p: Vec3) -> Option<Vec3> {
    let tol = (b.size() * 2e-3).max(1e-3);
    let faces = b.faces(tol).ok()?;
    let mesh = b.mesh(&solidcraft_kernel::MeshTolerance::chord(tol)).ok()?;
    let mut near = vec![f64::INFINITY; faces.len()];
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        if let (Some([u, v, w]), Some(d)) = (mesh.tri(t), near.get_mut(*f as usize)) {
            *d = d.min(crate::appearance::closest_on_triangle(p, u, v, w).dist(p));
        }
    }
    faces
        .iter()
        .filter_map(|f| f.plane_normal.map(|n| (n, ((p - f.centroid).dot(n)).abs(), near.get(f.index).copied().unwrap_or(f64::INFINITY))))
        .filter(|(_, d, _)| *d < tol * 5.0)
        .min_by(|a, b| a.2.total_cmp(&b.2).then(a.1.total_cmp(&b.1)))
        .map(|(n, _, _)| n)
}

/// The curves of a chain as one polyline in sketch coordinates, in order.
fn chain_points(ss: &SolvedSketch, ids: &[String]) -> Result<Vec<Vec2>> {
    let mut segs: Vec<Seg2> = Vec::new();
    for id in ids {
        let ci = ss.sketch.curve_index(id).ok_or_else(|| DocError::Unknown(format!("curve `{id}`")))?;
        segs.extend(ss.sketch.segs(ci));
    }
    let Some(first) = segs.first().copied() else { return Err(DocError::Invalid("no curves".into())) };
    // Orient the first segment toward the second, then chain.
    let mut chain = vec![first];
    if let Some(n) = segs.get(1)
        && (first.start().dist(n.start()) < 1e-6 || first.start().dist(n.end()) < 1e-6)
    {
        chain[0] = first.reversed();
    }
    for s in segs.iter().skip(1) {
        let prev = chain.last().map(Seg2::end).unwrap_or_default();
        let s = if s.end().dist(prev) < s.start().dist(prev) { s.reversed() } else { *s };
        if s.start().dist(prev) > 1e-5 {
            return Err(DocError::Invalid("the rib curves are not connected end to end".into()));
        }
        chain.push(s);
    }
    let mut pts = vec![chain[0].start()];
    for s in &chain {
        let n = if matches!(s, Seg2::Line { .. }) { 1 } else { 24 };
        for k in 1..=n {
            pts.push(s.point_at(k as f64 / n as f64));
        }
    }
    Ok(pts)
}

/// A rib wall: the band swept from the curves across the plane by `depth` (or to the body),
/// `t` thick about the sketch plane.
fn rib_tool(st: &ModelState, ss: &SolvedSketch, ids: &[String], t: f64, depth: Option<f64>, flip: bool) -> Result<Body> {
    let pts = chain_points(ss, ids)?;
    let (Some(a), Some(b)) = (pts.first().copied(), pts.last().copied()) else { return Err(DocError::Invalid("empty rib curve".into())) };
    let chord = (b - a).normalized().ok_or_else(|| DocError::Invalid("a closed or zero-length rib curve".into()))?;
    let mid = pts.get(pts.len() / 2).copied().unwrap_or(a);
    // Toward the material: the side of the curve where the bodies are (unless flipped).
    let mut n = chord.perp();
    let reach = st.bodies.iter().map(|b| b.mesh().bounds()).fold(0.0f64, |acc, bb| acc.max((bb.max - bb.min).len()));
    let probe = |side: Vec2| {
        let mut hits = 0;
        for k in 1..=20 {
            let q = ss.plane.to_world(mid + side * (reach * k as f64 / 20.0));
            if st.bodies.iter().any(|b| b.mesh().contains(q)) {
                hits += 1;
            }
        }
        hits
    };
    if probe(n * -1.0) > probe(n) {
        n = n * -1.0;
    }
    if flip {
        n = n * -1.0;
    }
    // From each point of the curves, walk across the plane until the body begins (or `depth`);
    // walls that reach the body go a little into it so they merge with it.
    let inside = |q: Vec2| {
        let w = ss.plane.to_world(q);
        st.bodies.iter().any(|b| b.mesh().contains(w))
    };
    let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    let samples = ((len / (t.min(len) * 0.25).max(len / 400.0)).ceil() as usize).clamp(2, 400);
    let along: Vec<Vec2> = (0..=samples).map(|k| point_on_polyline(&pts, len * k as f64 / samples as f64)).collect();
    let limit = depth.unwrap_or(reach * 2.0 + 1.0);
    let step = (limit.min(reach * 2.0 + 1.0) / 400.0).max(1e-3);
    let overlap = (len / samples as f64 * 2.0).max(step * 2.0).min(t);
    let mut far = Vec::with_capacity(along.len());
    for q in &along {
        let mut hit = None;
        let mut k = 1;
        while (k as f64) * step <= limit + step && k < 100_000 {
            if inside(*q + n * (k as f64 * step)) {
                // Refine between the last outside and the first inside sample.
                let (mut lo, mut hi) = ((k - 1) as f64 * step, k as f64 * step);
                for _ in 0..30 {
                    let m = 0.5 * (lo + hi);
                    if inside(*q + n * m) { hi = m } else { lo = m }
                }
                hit = Some(hi);
                break;
            }
            k += 1;
        }
        let h = match (hit, depth) {
            (Some(h), Some(d)) if h >= d => d,
            (Some(h), _) => h + overlap,
            (None, Some(d)) => d,
            (None, None) => return Err(DocError::Invalid("the rib curves do not face a body (give a depth)".into())),
        };
        far.push(*q + n * h);
    }
    let mut poly = along.clone();
    poly.extend(far.into_iter().rev());
    let mut lp = Loop2::polygon(&poly);
    if lp.signed_area() < 0.0 {
        lp = lp.reversed();
    }
    let region = Region2 { outer: lp, holes: Vec::new() };
    let tools = kernel::extrude(&ss.plane, &[region], -t / 2.0, t / 2.0)?;
    tools.into_iter().next().ok_or_else(|| DocError::Invalid("the rib made no solid".into()))
}

/// A rib wall along the sketch normal: the curves widened to `t` in the sketch plane, run from
/// the sketch toward the body by `depth` (or until the whole wall reaches the body).
fn rib_tool_normal(st: &ModelState, ss: &SolvedSketch, ids: &[String], t: f64, depth: Option<f64>, flip: bool) -> Result<Body> {
    let pts = chain_points(ss, ids)?;
    let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    if !(len > 1e-9) {
        return Err(DocError::Invalid("a zero-length rib curve".into()));
    }
    let samples = ((len / (t * 0.5).max(len / 400.0)).ceil() as usize).clamp(2, 400);
    let along: Vec<Vec3> = (0..=samples).map(|k| ss.plane.to_world(point_on_polyline(&pts, len * k as f64 / samples as f64))).collect();
    let normal = ss.plane.normal();
    let reach = st.bodies.iter().map(|b| b.mesh().bounds()).fold(0.0f64, |acc, bb| acc.max((bb.max - bb.min).len()));
    let inside = |q: Vec3| st.bodies.iter().any(|b| b.mesh().contains(q));
    // Where the body begins from each point of the curves, along `sign` × the sketch normal.
    let limit = reach * 2.0 + 1.0;
    let step = (limit / 400.0).max(1e-3);
    let hits = |sign: f64| -> Vec<f64> {
        let mut out = Vec::new();
        for q in &along {
            let Some(k) = (1..=400).find(|k| inside(*q + normal * (sign * *k as f64 * step))) else { continue };
            let (mut lo, mut hi) = ((k - 1) as f64 * step, k as f64 * step);
            for _ in 0..30 {
                let m = 0.5 * (lo + hi);
                if inside(*q + normal * (sign * m)) { hi = m } else { lo = m }
            }
            out.push(hi);
        }
        out
    };
    // Toward the material: the side of the sketch where the curves meet the bodies (unless flipped).
    let (front, back) = (hits(1.0), hits(-1.0));
    let mut sign = if back.len() > front.len() { -1.0 } else { 1.0 };
    let mut reached = if sign > 0.0 { front } else { back };
    if flip {
        sign = -sign;
        reached = hits(sign);
    }
    let h = match depth {
        Some(d) => d,
        None => {
            let far = reached.iter().copied().fold(f64::NAN, f64::max);
            if !far.is_finite() {
                return Err(DocError::Invalid("the rib curves do not face a body along the sketch normal (give a depth)".into()));
            }
            far + (t * 0.5).max(step)
        }
    };
    // The curves widened to `t`: each point offset by half the thickness along the miter of its
    // neighbours, both sides joined into one band.
    let half = t / 2.0;
    let mut left = Vec::with_capacity(pts.len());
    let mut right = Vec::with_capacity(pts.len());
    for (i, p) in pts.iter().enumerate() {
        let prev = i.checked_sub(1).and_then(|j| pts.get(j)).and_then(|q| (*p - *q).normalized());
        let next = pts.get(i + 1).and_then(|q| (*q - *p).normalized());
        let (n, scale) = match (prev, next) {
            (Some(a), Some(b)) => {
                let m = (a.perp() + b.perp()).normalized().unwrap_or(a.perp());
                (m, 1.0 / m.dot(a.perp()).max(0.25))
            }
            (Some(a), None) | (None, Some(a)) => (a.perp(), 1.0),
            (None, None) => continue,
        };
        left.push(*p + n * (half * scale));
        right.push(*p - n * (half * scale));
    }
    left.extend(right.into_iter().rev());
    let mut lp = Loop2::polygon(&left);
    if lp.signed_area() < 0.0 {
        lp = lp.reversed();
    }
    let region = Region2 { outer: lp, holes: Vec::new() };
    let (a, b) = if sign > 0.0 { (0.0, h) } else { (-h, 0.0) };
    let tools = kernel::extrude(&ss.plane, &[region], a, b)?;
    tools.into_iter().next().ok_or_else(|| DocError::Invalid("the rib made no solid".into()))
}

/// The point `d` along a polyline.
fn point_on_polyline(pts: &[Vec2], d: f64) -> Vec2 {
    let mut left = d.max(0.0);
    for w in pts.windows(2) {
        let l = w[0].dist(w[1]);
        if left <= l && l > 0.0 {
            return w[0].lerp(w[1], left / l);
        }
        left -= l;
    }
    pts.last().copied().unwrap_or_default()
}

/// The points of a feature's picked edges: by name first; a name that is gone keeps its point,
/// with a warning.
fn edge_picks(f: &Feature, mb: &ModelBody, picks: &[crate::Position], warning: &mut Option<String>) -> Result<Vec<Vec3>> {
    let mut out = Vec::with_capacity(picks.len());
    for (i, p) in picks.iter().enumerate() {
        // A name that no longer finds its edge fails the feature, asking for the edge to be
        // picked again.
        let name = f.edge_names.get(i).filter(|n| !n.is_empty());
        out.push(match name {
            Some(n) => {
                let hit = crate::naming::locate_edge(mb, n, p.0).map_err(|_| DocError::reselect(n.as_str()))?;
                if let Some(note) = hit.note(n) {
                    append_warning(warning, note);
                }
                hit.point
            }
            None => p.0,
        });
    }
    Ok(out)
}

/// The free edge of a sheet made from surfaces that a feature's pick finds: the free edge where
/// the edge of its body its saved name (the model's, `<face A>|<face B>`, see
/// [`crate::sheet::sheet_edge_names`]) names lies now, however the sheet changed; without a name,
/// the edge at the picked point. A name that no longer finds a free edge fails the feature,
/// asking for the edge to be picked again. `st` holds the sheet's body as the pick found it.
fn surface_edge_pick(st: &ModelState, sk: &crate::SurfaceSheet, name: Option<&String>, p: Vec3) -> Result<crate::sheet_surface_features::FreeEdge> {
    match name.filter(|n| !n.is_empty()) {
        Some(n) => named_free_edge(st, sk, n, p).ok_or_else(|| DocError::reselect(n.as_str())),
        None => sk.free_edge_at(p)?.ok_or_else(|| DocError::Invalid("pick a free edge of the sheet".into())),
    }
}

/// The free edge of sheet `sk` along the edge of its body (as `st` has it) that the model's edge
/// name `name` finds nearest `near`. None where the name finds no edge of the body, or none of
/// the sheet's free edges lies there.
fn named_free_edge(st: &ModelState, sk: &crate::SurfaceSheet, name: &str, near: Vec3) -> Option<crate::sheet_surface_features::FreeEdge> {
    let hit = crate::naming::locate_edge(st.body(&sk.body)?, name, near).ok()?;
    sk.free_edge_at(hit.point).ok().flatten()
}

/// Rebuild a sheet body's solid from its model.
pub(super) fn replace_sheet_body(mb: &mut ModelBody, body: Body) {
    let mut next = ModelBody::new(mb.name.clone(), body, mb.feature);
    next.component = mb.component;
    next.transferred = mb.transferred;
    next.source_name = mb.source_name.clone();
    next.source_transform = mb.source_transform;
    next.imported = mb.imported.clone();
    *mb = next;
}

/// A rule's values as a sheet made from surfaces takes them.
fn convert_rule(rv: &crate::sheet::RuleValues) -> crate::ConvertRule {
    crate::ConvertRule {
        thickness: rv.thickness,
        k_factor: rv.k_factor,
        bend_radius: rv.bend_radius,
        gap: rv.gap,
        corner_relief: rv.corner_relief_shape,
        corner_relief_size: rv.corner_relief,
        relief_width: rv.relief_width,
        relief_depth: rv.relief_depth,
    }
}

/// Converts the body `body` to sheet metal from its surfaces (#6748), its `faces` as Convert or
/// Recognize chooses them, the faces at `engrave` engravings of Convert's panels: the body
/// becomes the folded sheet, and the model keeps the sheet. The sheet is the rule's: its
/// thickness and bends are, whatever the body's.
#[allow(clippy::too_many_arguments)]
fn convert_to_sheet(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &mut ModelState,
    warning: &mut Option<String>,
    body: &crate::BodyName,
    rule: &Option<String>,
    bends: &[crate::Position],
    faces: crate::SheetFaces,
    engrave: &[Vec3],
    direction: &str,
) -> Result<()> {
    let mb = st.body(body).cloned().ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
    let r = doc.sheet_rule(rule.as_deref());
    let values = |t: Option<f64>| -> Result<crate::ConvertRule> { Ok(convert_rule(&doc.rule_values_at(vals, &r, t)?)) };
    let picks = edge_picks(f, &mb, bends, warning)?;
    let placement = doc.component_transform(doc.model_body_component(&mb));
    let to_world = doc.model_body_offset(&mb).map(|offset| crate::mat_mul(&placement, offset)).unwrap_or(placement);
    let conversion = crate::convert_surfaces(&mb, &values, &picks, faces, engrave, direction == "outside", &to_world)?;
    for note in &conversion.notes {
        append_warning(warning, note.clone());
    }
    let (sheet, solid) = crate::SurfaceSheet::converted(&mb.name, &r.name, &conversion, f.id)?;
    // Recognizing, a shell's or a plate's recesses are the sheet's, from either face: read where
    // the body has its own thickness, and kept, each as deep from its face, at the rule's.
    // Converting, the body has none of its own: only the engravings the user picked are kept,
    // and the conversion has them already.
    let same = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(1.0);
    let as_ruled = |own: f64| same(own, sheet.thickness) && conversion.modelled_radii.iter().all(|(_, r)| same(*r, conversion.rule_radius));
    let (sheet, solid) = match conversion.own_thickness {
        None => (sheet, solid),
        Some(own) if as_ruled(own) => crate::sheet_surface_cuts::engraved(&sheet, &solid, &mb.body)?,
        Some(own) => {
            let mut as_modelled = sheet.as_modelled(own, &conversion.modelled_radii);
            let built = as_modelled.build()?;
            let (read, _) = crate::sheet_surface_cuts::engraved(&as_modelled, &built, &mb.body)?;
            let mut sheet = sheet;
            sheet.recesses = read.recesses;
            let solid = if sheet.recesses.is_empty() { solid } else { sheet.build()? };
            (sheet, solid)
        }
    };
    // Its faces named from the faces of the body its panels were (see `PanelMade::Converted`).
    let none = vec![None; mb.body.face_count()];
    let lineage = crate::lineage::sheet_lineage(Some(crate::lineage::SheetBefore::found(&mb, &none)), &sheet, &solid);
    if let Some(b) = st.bodies.iter_mut().find(|b| b.name == *body) {
        replace_traced_sheet_body(b, solid, lineage, &sheet.frames);
    }
    st.sheets.retain(|s| s.body != sheet.body);
    st.surface_sheets.retain(|s| s.body != sheet.body);
    st.surface_sheets.push(sheet);
    Ok(())
}

/// The sheet made from surfaces (#6748) a feature works on: the named body's, or with none named,
/// the last one `fits`.
fn surface_sheet_for(st: &ModelState, body: &Option<crate::BodyName>, fits: impl Fn(&crate::SurfaceSheet) -> bool) -> Option<usize> {
    match body {
        Some(n) => st.surface_sheets.iter().position(|s| &s.body == n),
        None => st.surface_sheets.iter().rposition(fits),
    }
}

/// The sheet made from surfaces (#6748) a feature on its edge at `pick` works on: the named
/// body's; else the one the edge's saved `name` finds an edge on (wherever the sheet has moved
/// it since the pick, see [`surface_edge_pick`]); else the one with a free edge at the pick.
fn surface_sheet_at(st: &ModelState, body: &Option<crate::BodyName>, name: Option<&str>, pick: Option<Vec3>) -> Option<usize> {
    if let Some(n) = body {
        return st.surface_sheets.iter().position(|s| &s.body == n);
    }
    let p = pick?;
    if let Some(n) = name.filter(|n| !n.is_empty())
        && let Some(i) = st.surface_sheets.iter().position(|s| named_free_edge(st, s, n, p).is_some())
    {
        return Some(i);
    }
    // A sheet that refuses the pick (a bend line, not a free edge) is the pick's, so its reason
    // is the feature's.
    st.surface_sheets.iter().position(|s| !matches!(s.free_edge_at(p), Ok(None)))
}

/// Why a feature found no sheet made from surfaces to work on: the named body is no sheet metal
/// body, or there is none.
fn no_sheet(body: &Option<crate::BodyName>) -> DocError {
    match body {
        Some(n) => DocError::Invalid(format!("`{n}` is not a sheet metal body")),
        None => DocError::Invalid("there is no sheet metal body".into()),
    }
}

/// Why a feature on a sheet's edge found no sheet made from surfaces to work on: as
/// [`no_sheet`] where it names a body or there is no sheet, else no sheet holds a free edge at
/// its pick.
fn no_free_edge(st: &ModelState, body: &Option<crate::BodyName>, pick: Option<Vec3>) -> DocError {
    if body.is_some() || st.surface_sheets.is_empty() {
        return no_sheet(body);
    }
    match pick {
        Some(p) => DocError::Invalid(format!("no free edge of the sheet lies at ({:.3}, {:.3}, {:.3}): pick one on the sheet", p.x, p.y, p.z)),
        None => DocError::Invalid("pick a free edge of the sheet".into()),
    }
}

/// The sheet made from surfaces a fold is on, and its line's ends where they are now: the sketch
/// line's, or the points given. The named body's sheet, or the first whose flat panel the line
/// lies on.
fn surface_fold_sheet(
    st: &ModelState,
    body: &Option<crate::BodyName>,
    sketch: &Option<crate::SketchId>,
    curve: &Option<String>,
    a: &crate::Position,
    b: &crate::Position,
) -> Option<(usize, Vec3, Vec3)> {
    let (a, b) = match (sketch, curve) {
        (Some(sk), Some(c)) => {
            let ss = st.sketch(sk.0)?;
            let ci = ss.sketch.curve_index(c)?;
            match ss.sketch.segs(ci).as_slice() {
                [Seg2::Line { a, b }] => (ss.plane.to_world(*a), ss.plane.to_world(*b)),
                _ => return None,
            }
        }
        _ => (a.0, b.0),
    };
    if let Some(n) = body {
        return st.surface_sheets.iter().position(|s| &s.body == n).map(|i| (i, a, b));
    }
    st.surface_sheets.iter().position(|s| s.fold_panel_at(a, b)).map(|i| (i, a, b))
}

impl Document {
    /// The side a fold keeps put (see [`crate::FoldSide`]), decided as the fold is made, on the
    /// sheet as `st` has it before the fold: by its `fixed` point, else as the sheet joins on
    /// either side ([`crate::SurfaceSheet::fold_side`]). None for a feature that is no fold on a
    /// sheet made from surfaces.
    pub fn fold_side_at(&self, st: &ModelState, kind: &FeatureKind) -> Result<Option<crate::FoldSide>> {
        let FeatureKind::SheetFold { sketch, curve, a, b, angle, radius, position, flip, fixed, body, .. } = kind else { return Ok(None) };
        let Some((i, a, b)) = surface_fold_sheet(st, body, sketch, curve, a, b) else { return Ok(None) };
        let Some(sk) = st.surface_sheets.get(i).filter(|sk| !sk.unfolded) else { return Ok(None) };
        let vals = self.evaluation_values();
        let ang = val(&vals, angle, Kind::Angle)?;
        let rr = match radius {
            Some(e) => val(&vals, e, Kind::Length)?,
            None => self.rule_values_at(&vals, &self.sheet_rule(Some(&sk.rule)), Some(sk.thickness))?.bend_radius,
        };
        sk.fold_side(a, b, ang, rr, position, *flip, fixed.map(Vec3::from)).map(Some)
    }

    /// This design with each fold that keeps no side that stays (a design saved before folds
    /// kept one) given the side it folds now, as `model`, this design's evaluation, has the sheet
    /// before it: from then on that side stays, wherever the line moves. None where no fold
    /// lacks it.
    pub fn with_fold_sides(&self, model: &Model) -> Option<Document> {
        let mut out: Option<Document> = None;
        for f in &self.features {
            let FeatureKind::SheetFold { stays: None, .. } = &f.kind else { continue };
            let Ok(Some(side)) = self.fold_side_at(&model.state_before(f.id), &f.kind) else { continue };
            let doc = out.get_or_insert_with(|| self.clone());
            if let Some(FeatureKind::SheetFold { stays, .. }) = doc.feature_mut(f.id).map(|x| &mut x.kind) {
                *stays = Some(side);
            }
        }
        out
    }
}

/// Builds a sheet made from surfaces again after a change to it, and puts its body in place, its
/// faces named from what the sheet says each was made for (see `lineage::sheet_lineage`).
fn rebuild_surface_sheet(st: &mut ModelState, i: usize) -> Result<()> {
    let mut sk = st.surface_sheets.get(i).cloned().ok_or_else(|| DocError::Invalid("sheet".into()))?;
    // What the body's faces were made for, as the sheet last built it.
    let prev = std::mem::take(&mut sk.parts);
    let solid = sk.build()?;
    let b = st.bodies.iter_mut().find(|b| b.name == sk.body).ok_or_else(|| DocError::Unknown(format!("body `{}`", sk.body)))?;
    let lineage = crate::lineage::SheetBefore::of(b, &prev).and_then(|before| crate::lineage::sheet_lineage(Some(before), &sk, &solid));
    replace_traced_sheet_body(b, solid, lineage, &sk.frames);
    if let Some(slot) = st.surface_sheets.get_mut(i) {
        *slot = sk;
    }
    Ok(())
}

/// [`replace_sheet_body`], the new body's faces named after `lineage` (none: from geometry), each
/// face taking the sketch frame `frames` gives it (see [`ModelBody::sketch_frame`]).
pub(super) fn replace_traced_sheet_body(mb: &mut ModelBody, body: Body, lineage: Option<crate::lineage::Lineage>, frames: &[Option<Plane>]) {
    replace_sheet_body(mb, body);
    mb.made = match lineage {
        Some(l) => crate::lineage::Made::Traced(std::sync::Arc::new(l)),
        None => crate::lineage::Made::Unknown,
    };
    mb.sketch_frames = Some(std::sync::Arc::new(frames.to_vec()));
}

/// A sheet's body as its first build made it, its faces named from what the sheet says each was
/// made for, each face taking the sketch frame the sheet gives it.
fn new_sheet_body(name: String, sheet: &crate::SurfaceSheet, solid: Body, feature: u64) -> ModelBody {
    let lineage = crate::lineage::sheet_lineage(None, sheet, &solid);
    let mut mb = ModelBody::new(name, solid, feature).with_lineage(lineage);
    mb.sketch_frames = Some(std::sync::Arc::new(sheet.frames.clone()));
    mb
}

/// A region mirrored in the sketch x axis (y → −y), exactly: free-form curves through their
/// control points.
fn mirror_y(r: &Region2) -> Region2 {
    let f = |p: Vec2| Vec2::new(p.x, -p.y);
    let m = |l: &Loop2| {
        let segs: Vec<Seg2> = l
            .segs
            .iter()
            .map(|s| match *s {
                Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center: f(center), radius, start: -start, sweep: -sweep },
                other => other.map_controls(f),
            })
            .collect();
        Loop2 { segs }.ccw()
    };
    Region2 { outer: m(&r.outer), holes: r.holes.iter().map(|h| m(h).reversed()).collect() }
}

pub(super) fn sheet_eval(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &mut ModelState,
    warning: &mut Option<String>,
) -> Result<()> {
    use crate::sheet;
    match &f.kind {
        FeatureKind::SheetBase { sketch, profiles, rule, flip } => {
            let ss = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?.clone();
            let regions = solidcraft_sketch::merge_regions(&select_profiles(&ss, profiles)?);
            let r = doc.sheet_rule(rule.as_deref());
            let rule = convert_rule(&doc.rule_values(vals, &r)?);
            // The sketch's lines, in the sheet's plane: the base's rims are named after them.
            let mut curves: Vec<(String, Vec2, Vec2)> = Vec::new();
            for (ci, c) in ss.sketch.curves.iter().enumerate() {
                for s in ss.sketch.segs(ci) {
                    if let Seg2::Line { a, b } = s {
                        let f = |p: Vec2| if *flip { Vec2::new(p.x, -p.y) } else { p };
                        curves.push((c.id.clone(), f(a), f(b)));
                    }
                }
            }
            for (k, region) in regions.into_iter().enumerate() {
                let (plane, region) = if *flip {
                    (Plane { origin: ss.plane.origin, x: ss.plane.x, y: ss.plane.y * -1.0 }, mirror_y(&region))
                } else {
                    (ss.plane, region)
                };
                let name = new_name(st, f, k);
                // A sheet made from surfaces (#6748): one flat panel, the profile, every later
                // feature building on it.
                let region = Region2 { outer: region.outer.ccw(), holes: region.holes.iter().map(|h| h.ccw().reversed()).collect() };
                let (sheet, solid) = crate::SurfaceSheet::base(&name, &r.name, &rule, plane, region, &curves, f.id)?;
                st.bodies.push(new_sheet_body(name, &sheet, solid, f.id));
                st.surface_sheets.push(sheet);
            }
            Ok(())
        }
        FeatureKind::SheetContour { sketch, curves, distance, rule, flip, reverse } => {
            let ss = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?.clone();
            for id in curves {
                let ci = ss.sketch.curve_index(id).ok_or_else(|| DocError::Unknown(format!("curve `{id}`")))?;
                if !ss.sketch.segs(ci).iter().all(|s| matches!(s, Seg2::Line { .. })) {
                    return Err(DocError::Invalid("not supported yet: arcs in a contour flange (use lines; the bends are made for you)".into()));
                }
            }
            let pts = chain_points(&ss, curves)?;
            // The sketch line each line of the chain was drawn on, and its key: the line, with its
            // place on it where it was drawn in several.
            let lines = drawn_lines(&ss, curves);
            let w = val(vals, distance, Kind::Length)?;
            if !(w > 0.0) {
                return Err(DocError::Invalid("the contour flange distance must be positive".into()));
            }
            let r = doc.sheet_rule(rule.as_deref());
            let rule = convert_rule(&doc.rule_values(vals, &r)?);
            let plane = if *reverse { Plane { origin: ss.plane.origin, x: ss.plane.x, y: ss.plane.y * -1.0 } } else { ss.plane };
            let pts: Vec<Vec2> = if *reverse { pts.iter().map(|p| Vec2::new(p.x, -p.y)).collect() } else { pts };
            // Mirroring the sketch swaps the material side too. A sheet made from surfaces (#6748).
            let name = new_name(st, f, 0);
            let (sheet, solid) = crate::SurfaceSheet::contour(&name, &r.name, &rule, &plane, &pts, w, *flip != *reverse, f.id, &lines)?;
            st.bodies.push(new_sheet_body(name, &sheet, solid, f.id));
            st.surface_sheets.push(sheet);
            Ok(())
        }
        FeatureKind::SheetFlange { edges, height, angle, radius, position, flip, body, height_from, corner_relief_shape, corner_relief }
            if surface_sheet_at(st, body, f.edge_names.first().map(String::as_str), edges.first().map(|e| e.point.0)).is_some() =>
        {
            let i = surface_sheet_at(st, body, f.edge_names.first().map(String::as_str), edges.first().map(|e| e.point.0))
                .ok_or_else(|| DocError::Invalid("sheet".into()))?;
            let (h, a) = (val(vals, height, Kind::Length)?, val(vals, angle, Kind::Angle)?);
            let sk = st.surface_sheets.get(i).ok_or_else(|| DocError::Invalid("sheet".into()))?;
            if sk.unfolded {
                return Err(DocError::Invalid("refold the sheet before adding flanges".into()));
            }
            let rv = doc.rule_values_at(vals, &doc.sheet_rule(Some(&sk.rule)), Some(sk.thickness))?;
            let rr = match radius {
                Some(e) => val(vals, e, Kind::Length)?,
                None => rv.bend_radius,
            };
            let from = match height_from {
                Some(name) => sheet::HeightFrom::parse(name)
                    .ok_or_else(|| DocError::Invalid(format!("unknown flange height datum `{name}` (outer, inner or tangent)")))?,
                None => sheet::HeightFrom::Outer,
            };
            // Every edge is found on the sheet as it was before any of them.
            let mut sk = sk.clone();
            let before = sk.clone();
            let mut picks = Vec::with_capacity(edges.len());
            for (k, e) in edges.iter().enumerate() {
                let height = match &e.height {
                    Some(x) => val(vals, x, Kind::Length)?,
                    None => h,
                };
                let angle = match &e.angle {
                    Some(x) => val(vals, x, Kind::Angle)?,
                    None => a,
                };
                let edge = surface_edge_pick(st, &before, f.edge_names.get(k), e.point.0)?;
                picks.push((edge, height, angle));
            }
            // The corner relief these flanges ask for in place of the rule's, at the corners
            // their bends make.
            let own =
                match (corner_relief_shape, corner_relief) {
                    (None, None) => None,
                    (shape, size) => Some((
                        match shape {
                            Some(name) => crate::sheet_surface_convert::corner_relief(sheet::ReliefShape::parse(name).ok_or_else(|| {
                                DocError::Invalid(format!("unknown corner relief shape `{name}` (notch, round, rectangular or tear)"))
                            })?),
                            None => sk.reliefs.corner,
                        },
                        match size {
                            Some(e) => val(vals, e, Kind::Length)?,
                            None => sk.reliefs.corner_size,
                        },
                    )),
                };
            let n0 = sk.sheet.panels.len();
            let several = picks.len() > 1;
            // Each edge's error names it, among several.
            let named = |k: usize| {
                move |e: DocError| match e {
                    DocError::Invalid(m) if several => DocError::Invalid(format!("edge {}: {m}", k + 1)),
                    e => e,
                }
            };
            // Every flange is added before any corner is closed: a flange on a wall's end edge
            // is there when the flange beside the wall closes the corner the three bends make.
            // The feature's place in the timeline: where two features' reliefs meet at a corner,
            // the later one's is cut.
            let order = doc.feature_index(f.id).unwrap_or(usize::MAX);
            let mut made = Vec::with_capacity(picks.len());
            for (k, (edge, height, angle)) in picks.into_iter().enumerate() {
                let joint = sk.flange_on(edge, height, angle, rr, position, from, *flip).map_err(named(k))?;
                if let Some((relief, size)) = own {
                    sk.corner_reliefs.push((joint, relief, size, order));
                }
                made.push(joint);
            }
            for (k, joint) in made.iter().enumerate() {
                sk.close_flange_corners(std::slice::from_ref(joint)).map_err(named(k))?;
            }
            sk.clear_of_neighbours(&made)?;
            tag_panels(&mut sk, Some(&before), n0, f.id, PanelsBy::Bends(&[]));
            if let Some(slot) = st.surface_sheets.get_mut(i) {
                *slot = sk;
            }
            rebuild_surface_sheet(st, i)?;
            for note in st.surface_sheets.get(i).map(|s| s.notes.clone()).unwrap_or_default() {
                append_warning(warning, note);
            }
            Ok(())
        }
        FeatureKind::SheetFlange { edges, body, .. } => Err(no_free_edge(st, body, edges.first().map(|e| e.point.0))),
        FeatureKind::SheetPath { edge, sketch, curves, radius, body } if surface_sheet_at(st, body, None, Some(edge.0)).is_some() => {
            let ss = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?.clone();
            if curves.is_empty() || curves.len() > sheet::MAX_PATH_LINES {
                return Err(DocError::Invalid(format!("a path flange's path must have 1…{} lines", sheet::MAX_PATH_LINES)));
            }
            for id in curves {
                let ci = ss.sketch.curve_index(id).ok_or_else(|| DocError::Unknown(format!("curve `{id}`")))?;
                if !ss.sketch.segs(ci).iter().all(|s| matches!(s, Seg2::Line { .. })) {
                    return Err(DocError::Invalid("not supported yet: arcs in a path flange's path (use lines; the bends are made for you)".into()));
                }
            }
            let order = ss.chain_order(curves)?;
            let path: Vec<Vec3> = chain_points(&ss, &order)?.into_iter().map(|p| ss.plane.to_world(p)).collect();
            // The path's lines, in its order: each panel past the first is keyed by its own.
            let lines = drawn_lines(&ss, &order);
            let i = surface_sheet_at(st, body, None, Some(edge.0)).ok_or_else(|| DocError::Invalid("sheet".into()))?;
            let sk = st.surface_sheets.get(i).ok_or_else(|| DocError::Invalid("sheet".into()))?;
            if sk.unfolded {
                return Err(DocError::Invalid("refold the sheet before adding flanges".into()));
            }
            let rr = match radius {
                Some(e) => val(vals, e, Kind::Length)?,
                None => doc.rule_values_at(vals, &doc.sheet_rule(Some(&sk.rule)), Some(sk.thickness))?.bend_radius,
            };
            if let Some(sk) = st.surface_sheets.get_mut(i) {
                let before = sk.clone();
                let n0 = sk.sheet.panels.len();
                sk.add_path_flange(edge.0, &path, rr)?;
                tag_panels(sk, Some(&before), n0, f.id, PanelsBy::Bends(&lines));
            }
            rebuild_surface_sheet(st, i)
        }
        FeatureKind::SheetPath { edge, body, .. } => Err(no_free_edge(st, body, Some(edge.0))),
        FeatureKind::SheetPerforate { diameter, pitch, margin, pattern, body, sketch, profiles } => {
            // Without a body name the field goes on the newest sheet.
            let i = match body {
                Some(n) => st.surface_sheets.iter().position(|s| &s.body == n),
                None => st.surface_sheets.len().checked_sub(1),
            }
            .ok_or_else(|| no_sheet(body))?;
            let d = val(vals, diameter, Kind::Length)?;
            let pitch = val(vals, pitch, Kind::Length)?;
            let margin = match margin {
                Some(e) => val(vals, e, Kind::Length)?,
                None => d,
            };
            let square = match pattern.as_deref() {
                None | Some("staggered") => false,
                Some("square") => true,
                Some(o) => return Err(DocError::Invalid(format!("perforation pattern {o:?}: use \"staggered\" or \"square\""))),
            };
            if !(d > 0.0 && pitch > d && margin >= 0.0) {
                return Err(DocError::Invalid(format!(
                    "perforation needs diameter > 0, pitch > diameter and margin >= 0 (diameter {d}, pitch {pitch}, margin {margin})"
                )));
            }
            let outline = match sketch {
                Some(sk) => {
                    let ss = st.sketch(sk.0).ok_or_else(|| DocError::Unknown(format!("sketch {sk}")))?;
                    let regions = select_profiles(ss, profiles.as_ref().unwrap_or(&ProfileSel::All))?;
                    let sh = st.surface_sheets.get(i).ok_or_else(|| DocError::Invalid("sheet".into()))?;
                    Some(sh.perforation_outline(&ss.plane, &regions)?)
                }
                None => None,
            };
            let sh = st.surface_sheets.get_mut(i).ok_or_else(|| DocError::Invalid("sheet".into()))?;
            sh.perforations.push(crate::sheet::SheetPerforation { feature: f.id, diameter: d, pitch, margin, square, from_right: false, outline });
            // It keeps clear of the bends the sheet has now; a bend made later runs through it.
            let joints = sh.sheet.joints.len();
            sh.perforation_joints.push((f.id, joints));
            Ok(())
        }
        FeatureKind::SheetHem { edges, length, gap, flip, body }
            if surface_sheet_at(st, body, f.edge_names.first().map(String::as_str), edges.first().map(|p| p.0)).is_some() =>
        {
            let i = surface_sheet_at(st, body, f.edge_names.first().map(String::as_str), edges.first().map(|p| p.0))
                .ok_or_else(|| DocError::Invalid("sheet".into()))?;
            let l = val(vals, length, Kind::Length)?;
            let mut sk = st.surface_sheets.get(i).cloned().ok_or_else(|| DocError::Invalid("sheet".into()))?;
            if sk.unfolded {
                return Err(DocError::Invalid("refold the sheet before adding hems".into()));
            }
            let g = match gap {
                Some(e) => val(vals, e, Kind::Length)?,
                None => doc.rule_values_at(vals, &doc.sheet_rule(Some(&sk.rule)), Some(sk.thickness))?.hem_gap,
            };
            let before = sk.clone();
            let mut found = Vec::with_capacity(edges.len());
            for (k, p) in edges.iter().enumerate() {
                found.push(surface_edge_pick(st, &before, f.edge_names.get(k), p.0)?);
            }
            let n0 = sk.sheet.panels.len();
            for edge in found {
                sk.add_hem_on(edge, l, g, *flip)?;
            }
            tag_panels(&mut sk, Some(&before), n0, f.id, PanelsBy::Bends(&[]));
            if let Some(slot) = st.surface_sheets.get_mut(i) {
                *slot = sk;
            }
            rebuild_surface_sheet(st, i)
        }
        FeatureKind::SheetHem { edges, body, .. } => Err(no_free_edge(st, body, edges.first().map(|p| p.0))),
        FeatureKind::SheetFold { sketch, curve, a, b, angle, radius, position, flip, fixed, stays, body }
            if surface_fold_sheet(st, body, sketch, curve, a, b).is_some() =>
        {
            let (i, a, b) = surface_fold_sheet(st, body, sketch, curve, a, b).ok_or_else(|| DocError::Invalid("sheet".into()))?;
            let sk = st.surface_sheets.get(i).ok_or_else(|| DocError::Invalid("sheet".into()))?;
            if sk.unfolded {
                return Err(DocError::Invalid("refold the sheet before folding it".into()));
            }
            let ang = val(vals, angle, Kind::Angle)?;
            let rr = match radius {
                Some(e) => val(vals, e, Kind::Length)?,
                None => doc.rule_values_at(vals, &doc.sheet_rule(Some(&sk.rule)), Some(sk.thickness))?.bend_radius,
            };
            // A fixed point that touches no material at all still keeps its fold recomputing (the
            // side it was made with stays); it says so (#371).
            let t = sk.thickness;
            if let Some(p) = fixed
                && !sk.touches(p.0, (t * 1e-3).max(1e-6))
            {
                let kept = if stays.is_some() { "the side the fold was made with stays" } else { "the side of the fold line it lies on stays" };
                *warning = Some(format!("{} at ({:.3}, {:.3}, {:.3}); {kept}", sheet::FOLD_FIXED_OFF_SHEET, p.x, p.y, p.z));
            }
            // The side that stays is the fold's own, decided when it was made; a fold saved
            // before it kept one decides it as it was made (and keeps it once opened).
            let stays = match stays {
                Some(side) => *side,
                None => sk.fold_side(a, b, ang, rr, position, *flip, fixed.map(Vec3::from))?,
            };
            if let Some(sk) = st.surface_sheets.get_mut(i) {
                let before = sk.clone();
                let n0 = sk.sheet.panels.len();
                sk.add_fold(a, b, ang, rr, position, *flip, stays)?;
                tag_panels(sk, Some(&before), n0, f.id, PanelsBy::Fold([a, b]));
            }
            rebuild_surface_sheet(st, i)
        }
        FeatureKind::SheetFold { sketch, curve, body, .. } => {
            // No sheet takes the fold: its sketch line is gone or no line, the named body is no
            // sheet, there is none, or the line lies on no flat face of one.
            if let (Some(sk), Some(c)) = (sketch, curve) {
                let ss = st.sketch(sk.0).ok_or_else(|| DocError::Unknown(format!("sketch {sk}")))?;
                let ci = ss.sketch.curve_index(c).ok_or_else(|| DocError::Unknown(format!("curve `{c}`")))?;
                if !matches!(ss.sketch.segs(ci).as_slice(), [Seg2::Line { .. }]) {
                    return Err(DocError::Invalid("the fold line must be a sketch line".into()));
                }
            }
            Err(match body {
                None if !st.surface_sheets.is_empty() => DocError::Invalid("the fold line must lie on a flat face of the sheet".into()),
                _ => no_sheet(body),
            })
        }
        FeatureKind::SheetUnfold { body, refold } => {
            let i = surface_sheet_for(st, body, |s| s.unfolded == *refold).ok_or_else(|| match body {
                Some(_) => no_sheet(body),
                None => DocError::Invalid(if *refold { "no unfolded sheet to refold".into() } else { "no sheet metal body to unfold".into() }),
            })?;
            if let Some(sk) = st.surface_sheets.get_mut(i) {
                sk.unfolded = !*refold;
            }
            rebuild_surface_sheet(st, i)
        }
        // Convert (#6748): the body becomes the folded sheet, and the model keeps the sheet.
        // Convert takes every face of the body but the skipped ones, or the faces picked and
        // every face a picked bend joins to them, however thin the body: each is a panel, and
        // the faces picked to engrave are engravings of the panels round them. Recognize alone
        // reads a body that is sheet metal already, one side of it.
        FeatureKind::SheetSurfaceConvert { body, rule, bends, skip, engrave, direction } => {
            let skip: Vec<Vec3> = skip.iter().map(|p| p.0).collect();
            let engrave: Vec<Vec3> = engrave.iter().map(|p| p.0).collect();
            convert_to_sheet(doc, vals, f, st, warning, body, rule, bends, crate::SheetFaces::AllBut(&skip), &engrave, direction)
        }
        FeatureKind::SheetConvert { body, face, faces, rule, bends, engrave, direction } => {
            let picked: Vec<Vec3> = std::iter::once(face.0).chain(faces.iter().map(|p| p.0)).collect();
            let engrave: Vec<Vec3> = engrave.iter().map(|p| p.0).collect();
            convert_to_sheet(doc, vals, f, st, warning, body, rule, bends, crate::SheetFaces::Picked(&picked), &engrave, direction)
        }
        FeatureKind::SheetRecognize { body, face, skip, bends, rule } => {
            let skip: Vec<Vec3> = skip.iter().map(|p| p.0).collect();
            let faces = crate::SheetFaces::Sheet { face: face.as_ref().map(|p| p.0), skip: &skip };
            convert_to_sheet(doc, vals, f, st, warning, body, rule, bends, faces, &[], "")
        }
        _ => Err(DocError::Invalid(format!("{} is not a sheet metal feature", f.name))),
    }
}

/// The tool's depth along the sketch normal (from `source`), the roundoff of that depth and the
/// tool's current footprint where it differs from the profile: from its current unmodified
/// extrusion caps, including placement, else from its exact record proving it a uniform prism
/// along the normal (a To Object tool trimmed by the body it runs to, whose boolean leaves no
/// extrusion faces: its section is its footprint). Tessellation can drift in thickness at
/// distant coordinates and cannot certify a slab.
fn sheet_tool_span(body: &Body, source: &Plane) -> Option<(f64, f64, f64, Option<(Plane, Region2)>)> {
    extrusion_tool_span(body, source).or_else(|| {
        let prism = kernel::certified_prism_section(body, source).ok().flatten()?;
        (prism.tolerance <= 1e-7).then_some((prism.lo, prism.hi, prism.tolerance, Some((*source, prism.region))))
    })
}

/// [`sheet_tool_span`] from the tool's extrusion caps.
fn extrusion_tool_span(body: &Body, source: &Plane) -> Option<(f64, f64, f64, Option<(Plane, Region2)>)> {
    use kernel::FaceSource;
    #[derive(PartialEq)]
    enum Signature<'a> {
        Raw(&'a Region2),
        Certified(&'a [String], &'a [Vec<String>]),
    }
    let sources = body.face_sources()?;
    if sources.len() != body.face_count() || sources.len() > 16_384 {
        return None;
    }
    let n = source.normal();
    let mut family = None;
    let mut caps = [None, None];
    let mut projection_roundoff: f64 = 0.0;
    let mut footprint = None;
    for (index, entry) in sources.iter().enumerate() {
        let (group, cap) = match entry.as_ref()? {
            FaceSource::ExtrudeSide { .. } => (None, None),
            FaceSource::ExtrudeCap { high, region, plane } => (None, Some((*high, *plane, Signature::Raw(region)))),
            FaceSource::CertifiedExtrudeSide { producer, sketch, .. } => (Some((*producer, *sketch)), None),
            FaceSource::CertifiedExtrudeCap { producer, sketch, high, outer, holes, plane } => {
                (Some((*producer, *sketch)), Some((*high, *plane, Signature::Certified(outer, holes))))
            }
            _ => return None,
        };
        match family {
            None => family = Some(group),
            Some(previous) if previous != group => return None,
            _ => {}
        }
        if let Some((high, plane, signature)) = cap {
            if high && let Signature::Raw(region) = &signature {
                footprint = Some((plane, (*region).clone()));
            }
            let valid = |p: &Plane| {
                p.origin.is_finite()
                    && p.x.is_finite()
                    && p.y.is_finite()
                    && (p.x.len2() - 1.0).abs() <= 1e-10
                    && (p.y.len2() - 1.0).abs() <= 1e-10
                    && p.x.dot(p.y).abs() <= 1e-10
                    && p.normal().cross(n).len() <= 1e-10
            };
            let current = body.planar_face(index)?;
            if !valid(&plane) || !valid(&current) {
                return None;
            }
            // Bound floating point projection separately from model geometry tolerance.
            let roundoff = 128.0 * f64::EPSILON * (plane.origin.len() + current.origin.len() + source.origin.len()).max(1.0);
            // The cap's current point lies off the recorded plane by the noise of evaluating
            // its edges, which its fitted tilt shows across the distance between the two points
            // (8e-12 over 2.4 mm on a 45° flange's drill, #1255; none on axis-aligned caps).
            let noise = current.normal().cross(n).len() * (current.origin - plane.origin).len();
            if roundoff > 1e-7 || (current.origin - plane.origin).dot(n).abs() > roundoff + noise {
                return None;
            }
            projection_roundoff = projection_roundoff.max(roundoff);
            let distance = (plane.origin - source.origin).dot(n);
            if !distance.is_finite() || caps[usize::from(high)].is_some() {
                return None;
            }
            caps[usize::from(high)] = Some((distance, signature));
        }
    }
    let [Some((low, low_signature)), Some((high, high_signature))] = caps else { return None };
    (low_signature == high_signature && low != high).then_some((low.min(high), low.max(high), projection_roundoff, footprint))
}

/// What an untapered extrude cut (or intersect) machines into a sheet.
struct ExtrudeCut {
    plane: Plane,
    regions: Vec<Region2>,
    /// The outlines of the sketch's text: the sheet remembers where its letters went.
    text: Vec<Seg2>,
    operation: Operation,
}

/// The sketch plane, regions and text of `f`'s extrude; None for any other feature, a tapered
/// extrude or a missing sketch.
fn extrude_cut(st: &ModelState, f: &Feature) -> Result<Option<ExtrudeCut>> {
    let FeatureKind::Extrude { sketch, profiles, operation, extent, .. } = &f.kind else { return Ok(None) };
    if extent.taper.is_some() {
        return Ok(None);
    }
    let Some(ss) = st.sketch(sketch.0) else { return Ok(None) };
    let regions = solidcraft_sketch::merge_regions(&select_profiles(ss, profiles)?);
    let text: Vec<Seg2> = ss
        .sketch
        .links
        .iter()
        .filter(|l| matches!(l.source, solidcraft_sketch::LinkSource::Text { .. }))
        .flat_map(|l| ss.sketch.link_curves(&l.id))
        .flat_map(|ci| ss.sketch.exact_segs(ci).unwrap_or_else(|| ss.sketch.segs(ci)))
        .collect();
    Ok(Some(ExtrudeCut { plane: ss.plane, regions, text, operation: *operation }))
}

/// Whether body `b` takes part in feature `f`, as `apply_traced_with` picks the bodies a boolean
/// changes: the feature's own `targets`, else its explicit participants (`*` for all),
/// else the bodies of its component and what it made itself. A sheet made from surfaces is one
/// body among them: a cut aimed at another component's body, or at bodies it names, leaves it be.
fn takes_part(f: &Feature, targets: &[String], b: &ModelBody) -> bool {
    let targets: &[String] = if targets.is_empty() { &f.participants } else { targets };
    let all = targets.len() == 1 && targets.first().is_some_and(|t| t == "*");
    all || if targets.is_empty() { b.component == f.component || b.feature == f.id } else { targets.contains(&b.name) }
}

/// What an extrude cut takes from the sheets made from surfaces it reaches, of the bodies it takes
/// part in (see [`takes_part`]), read straight into their outlines (#6748): each tool a prism
/// along a flat panel's normal, through its whole thickness, its footprint clear inside the
/// panel's material and its box apart from the rest of the sheet. That is what the cut takes in
/// space (see `sheet_surface_cuts`), without cutting
/// a body the sheet builds again. What lies inside a footprint (a letter's counter) stays, a
/// loose piece. A footprint through the panel's outline takes what it covers of the panel, the
/// curves of both kept as drawn; a piece it parts from the panel stays, a loose piece. The
/// sheets' bodies cut; none, and nothing changed, for any other cut, which the generic path
/// takes.
pub(super) fn surface_prism_cut(
    f: &Feature,
    st: &mut ModelState,
    tools: &[Body],
    targets: &[String],
    constructor: Option<&ExtrudeConstructor>,
) -> Result<Vec<String>> {
    let Some(cut) = extrude_cut(st, f)? else { return Ok(Vec::new()) };
    if cut.operation != Operation::Cut || tools.is_empty() || tools.len() > 10_000 {
        return Ok(Vec::new());
    }
    let n = cut.plane.normal();
    // The tools' span along the sketch's normal, one for all of them, and each footprint: the
    // tools' own, else the sketch's profiles.
    // The extrusion's own interval where it was built on the sketch's plane.
    let mut span: Option<(f64, f64)> =
        constructor.filter(|c| c.plane == cut.plane).map(|c| (c.interval.0.min(c.interval.1), c.interval.0.max(c.interval.1)));
    let mut footprints: Option<Vec<(Plane, Region2)>> = if span.is_some() { None } else { Some(Vec::with_capacity(tools.len())) };
    let read_tools = footprints.is_some();
    for tool in tools.iter().filter(|_| read_tools) {
        let Some((lo, hi, _, footprint)) = sheet_tool_span(tool, &cut.plane) else { return Ok(Vec::new()) };
        if span.is_some_and(|(a, b)| (a - lo).abs() > 1e-7 || (b - hi).abs() > 1e-7) {
            return Ok(Vec::new());
        }
        span = Some((lo, hi));
        match (footprint, footprints.as_mut()) {
            (Some(fp), Some(list)) => list.push(fp),
            _ => footprints = None,
        }
    }
    let Some((lo, hi)) = span else { return Ok(Vec::new()) };
    let footprints = footprints.unwrap_or_else(|| cut.regions.iter().map(|r| (cut.plane, r.clone())).collect());
    // Each footprint's prism's box.
    let spans: Vec<(f64, f64, Plane, Region2, Option<Aabb3>)> = footprints
        .into_iter()
        .map(|(plane, region)| {
            let mut b = Aabb3::EMPTY;
            for q in &region.outer.polyline(1e-3) {
                let w = plane.to_world(*q);
                let off = (w - cut.plane.origin).dot(n);
                for s in [lo, hi] {
                    b.add(w + n * (s - off));
                }
            }
            (lo, hi, plane, region, Some(b.inflated(1e-6)))
        })
        .collect();
    // The cut's sketch: the curves its holes' walls are named after (see `SurfaceSheet::name_hole`).
    let cut_sketch = match &f.kind {
        FeatureKind::Extrude { sketch, .. } => st.sketch(sketch.0),
        _ => None,
    };
    let mut plans: Vec<(usize, crate::SurfaceSheet)> = Vec::new();
    for (i, sk) in st.surface_sheets.iter().enumerate() {
        let Some(mb) = st.body(&sk.body) else { continue };
        if !takes_part(f, targets, mb) {
            continue;
        }
        let body_box = exact_box(&mb.body);
        let reaching: Vec<_> = spans.iter().filter(|s| !boxes_apart(&s.4, &body_box)).collect();
        if reaching.is_empty() {
            continue;
        }
        let Some((panels, boxes)) = sk.pieces_in_space() else { return Ok(Vec::new()) };
        let t = sk.thickness;
        let tol = 1e-7;
        let mut out = sk.clone();
        for (lo, hi, plane, region, tool_box) in reaching {
            // The panel the tool goes square through.
            let mut found = None;
            for (p, frame, own) in &panels {
                if frame.z.cross(n).len() > 1e-9 {
                    continue;
                }
                let s0 = (frame.shift - cut.plane.origin).dot(n);
                let s1 = s0 + t * frame.z.dot(n);
                if !(*lo <= s0.min(s1) + tol && *hi >= s0.max(s1) - tol) {
                    continue;
                }
                let inv = frame.inverse();
                let chart = |q: Vec2| {
                    let w = inv.point(plane.to_world(q));
                    Vec2::new(w.x, w.y)
                };
                let mapped = crate::sheet_surface_convert::rigid_region(region, chart);
                let outline = Region2 { outer: mapped.outer.clone(), holes: Vec::new() };
                // Clear inside the panel's material, a hole of it; else through its outline or a
                // hole of it where it covers some of that material, and beside the panel where
                // it covers none.
                let clear = crate::sheet_surface::holds_clear(own, &outline);
                if !clear {
                    match kernel::planar_regions_boolean(std::slice::from_ref(own), std::slice::from_ref(&outline), kernel::BoolOp::Intersect, tol) {
                        Ok(on) if on.iter().map(Region2::area).sum::<f64>() > 1e-9 => {}
                        Ok(_) => continue,
                        // The boolean failing leaves the cut to the body's own, the 3D path, as
                        // each `Ok(Vec::new())` here does. That is the conservative choice, not an
                        // equivalent one: the body's cut brings the panel's curves back as splines
                        // fitted to them, which is what this path exists to avoid.
                        Err(_) => return Ok(Vec::new()),
                    }
                }
                if found.is_some() {
                    return Ok(Vec::new());
                }
                found = Some((*p, mapped, *frame, clear));
            }
            let Some((p, mapped, frame, clear)) = found else { return Ok(Vec::new()) };
            // Nothing else of the sheet in its way, and nothing the panel keeps there already.
            if boxes.iter().any(|(k, b)| *k != Some(p) && !boxes_apart(tool_box, &Some(*b))) {
                return Ok(Vec::new());
            }
            let near = |r: &Region2| {
                let (a, b) = (r.outer.polyline(1e-3).bounds(), mapped.outer.polyline(1e-3).bounds());
                !(a.max.x < b.min.x || b.max.x < a.min.x || a.max.y < b.min.y || b.max.y < a.min.y)
            };
            if out.recesses.iter().any(|r| r.panel == p && near(&r.region))
                || out.loose.iter().any(|l| l.panel == p && near(&l.region))
                || !out.sheet.cuts.is_empty()
            {
                return Ok(Vec::new());
            }
            if !clear {
                // Through the panel's outline: the panel less the footprint, by the planar
                // boolean, keeps the panel's curves and the tool's as drawn, where the body's cut
                // brings both back as splines fitted to them. A piece it parts from the panel
                // stays, a loose piece, as the body's cut reads it; pieces that each hold a joint
                // are left to the body's cut, which keeps the panel whole. A bore runs in from the
                // panel's edge, which the cut moves: the body's cut reads what it leaves of one.
                if out.bores.iter().any(|b| b.panel == Some(p)) {
                    return Ok(Vec::new());
                }
                let Ok((kept, parted)) = crate::sheet_surface_cuts::left_of_panel(&out, p, std::slice::from_ref(&mapped), tol) else {
                    return Ok(Vec::new());
                };
                let Some(panel) = out.sheet.panels.get_mut(p) else { return Ok(Vec::new()) };
                panel.region = kept;
                out.loose.extend(parted.into_iter().map(|region| crate::Loose { panel: p, region }));
                continue;
            }
            let Some(panel) = out.sheet.panels.get_mut(p) else { return Ok(Vec::new()) };
            if !crate::sheet_surface::holds_clear(&panel.region, &Region2 { outer: mapped.outer.clone(), holes: Vec::new() }) {
                return Ok(Vec::new());
            }
            let hole = mapped.outer.ccw().reversed();
            panel.region.holes.push(hole.clone());
            let holder = panel.name.clone();
            out.loose.extend(mapped.holes.iter().map(|h| crate::Loose { panel: p, region: Region2 { outer: h.ccw(), holes: Vec::new() } }));
            // Its walls are named after the curves of the cut's profiles they lie on, read in the
            // sketch's plane, which is parallel to the panel (the cut goes square through both),
            // not the tool's footprint, which lies on its cap when it cuts through all or both ways.
            if let Some(ss) = cut_sketch {
                let inv = frame.inverse();
                let chart = |q: Vec2| {
                    let w = inv.point(cut.plane.to_world(q));
                    Vec2::new(w.x, w.y)
                };
                out.name_hole(&holder, &hole, f.id, ss, &cut.regions, &chart);
            }
        }
        plans.push((i, out));
    }
    let letters = cut_letters(st, f);
    let mut handled = Vec::with_capacity(plans.len());
    for (i, mut sk) in plans {
        if let Some(flat) = letters.as_ref().and_then(|(plane, regions)| sk.flat_of(plane, regions)) {
            sk.labels.extend(flat);
        }
        handled.push(sk.body.clone());
        if let Some(slot) = st.surface_sheets.get_mut(i) {
            *slot = sk;
        }
        rebuild_surface_sheet(st, i)?;
    }
    Ok(handled)
}

/// Refuses a tapered extrude cut (or intersect) that changes a sheet made from surfaces it targets
/// (#6748) where the sheet can't keep it, as the flat model refuses it:
///
/// - a cut that stops inside a flat panel's thickness: a cap of the extrude (an end of the
///   interval `constructor` says it was drawn over) lies strictly between the faces of a flat
///   panel parallel to its plane, in a panel a tool reaches by their exact boxes. Its walls are
///   not square to the sheet, and read back only its floor would show: the panel would keep a
///   pocket at its floor's size;
/// - an intersect: what it keeps has walls that are not square to the sheet.
///
/// Any other tapered cut goes on to the reader as an untapered one does (see
/// `sheet_surface_cuts`): one through the sheet has walls from face to face, and the reader cuts
/// it as its shadow, square through as a shop cuts it, as it cuts a hole at a slant. A cut beside
/// the sheet changes nothing. Where the extrude's interval isn't known, a cut is refused wherever
/// it changes the sheet.
pub(super) fn surface_taper_refused(
    st: &ModelState,
    tools: &[Body],
    constructor: Option<&ExtrudeConstructor>,
    targets: &[String],
    op: Operation,
) -> Result<()> {
    let all = targets.len() == 1 && targets.first().is_some_and(|t| t == "*");
    let reach: Vec<Option<Aabb3>> = tools.iter().map(exact_box).collect();
    for sk in st.surface_sheets.iter().filter(|sk| all || targets.is_empty() || targets.contains(&sk.body)) {
        let Some(mb) = st.body(&sk.body) else { continue };
        if op == Operation::Cut && constructor.is_some_and(|c| !stops_inside(sk, &reach, &c.plane, c.interval)) {
            continue;
        }
        super::sheet_boolean_unchanged(&mb.body, tools, op)?;
    }
    Ok(())
}

/// Whether an extrude drawn on `plane` over `interval` (along the plane's normal) stops inside a
/// flat panel of `sk` that a tool reaches (`reach`: the tools' boxes): a cap of it strictly between
/// the panel's faces, parallel to them. Past a rounding of the numbers its depths come from, a cap
/// within a few units in the last place of a face is on it (see [`surface_thin_layer_refused`]).
/// Where the sheet's pieces can't be placed, it is taken to stop inside one.
fn stops_inside(sk: &crate::SurfaceSheet, reach: &[Option<Aabb3>], plane: &Plane, interval: (f64, f64)) -> bool {
    let Some((panels, boxes)) = sk.pieces_in_space() else { return true };
    let n = plane.normal();
    let t = sk.thickness;
    panels.iter().any(|(p, frame, _)| {
        if frame.z.cross(n).len() > 1e-9 {
            return false;
        }
        // The panel's faces along the normal.
        let s0 = (frame.shift - plane.origin).dot(n);
        let s1 = s0 + t * frame.z.dot(n);
        let (p0, p1) = (s0.min(s1), s0.max(s1));
        let ulps = 4.0 * f64::EPSILON * (frame.shift.len() + plane.origin.len() + interval.0.abs() + interval.1.abs() + t);
        [interval.0, interval.1].into_iter().any(|cap| cap - p0 > ulps && p1 - cap > ulps)
            && boxes.iter().any(|(k, b)| *k == Some(*p) && reach.iter().any(|r| !boxes_apart(r, &Some(*b))))
    })
}

/// What the drills of hole feature `f` take from the sheets made from surfaces they reach, of the
/// bodies the hole cuts (see [`takes_part`]) (#6748): a drill square across a flat panel, through
/// its thickness and clear inside its material, is a round hole in its outline; one along a flat
/// panel, inside its thickness, a bore of it (see [`crate::sheet::SheetBore`]). Each drill must
/// meet one panel so and nothing else of the sheet. The sheets' bodies drilled; none, and nothing
/// changed, for any other hole, which the generic path takes.
pub(super) fn surface_hole_cut(f: &Feature, st: &mut ModelState, tools: &[(Traced, Option<PrimitivePrism>)]) -> Result<Vec<String>> {
    if tools.is_empty() || tools.len() > 10_000 {
        return Ok(Vec::new());
    }
    let mut drills = Vec::with_capacity(tools.len());
    for (_, prism) in tools {
        let Some(PrimitivePrism::Cylinder { base, axis, radius, height }) = *prism else { return Ok(Vec::new()) };
        drills.push((base, axis, radius, height, drill_box(base, axis, radius, height)));
    }
    let mut plans: Vec<(usize, crate::SurfaceSheet)> = Vec::new();
    for (i, sk) in st.surface_sheets.iter().enumerate() {
        let Some(mb) = st.body(&sk.body) else { continue };
        // A hole has no targets of its own: its participants, else its component's bodies.
        if !takes_part(f, &[], mb) {
            continue;
        }
        let body_box = exact_box(&mb.body);
        let reaching: Vec<_> = drills.iter().filter(|d| !boxes_apart(&d.4, &body_box)).collect();
        if reaching.is_empty() {
            continue;
        }
        let Some((panels, boxes)) = sk.pieces_in_space() else { return Ok(Vec::new()) };
        let t = sk.thickness;
        let tol = 1e-7;
        let mut out = sk.clone();
        for &&(base, axis, radius, height, reach) in &reaching {
            let mut found: Option<(usize, bool, Vec3, Vec3, (f64, f64))> = None;
            for (p, frame, own) in &panels {
                let inv = frame.inverse();
                let (b, a) = (inv.point(base), inv.vector(axis));
                let Some(a) = a.normalized() else { return Ok(Vec::new()) };
                let across = a.z.abs() >= 1.0 - 1e-9;
                let mut span = (height.min(0.0), height.max(0.0));
                let meets = if across {
                    let (z0, z1) = (b.z.min(b.z + a.z * height), b.z.max(b.z + a.z * height));
                    let circle = Region2 { outer: Loop2::circle(Vec2::new(b.x, b.y), radius), holes: Vec::new() };
                    if own.contains(Vec2::new(b.x, b.y)) && !(z0 <= tol && z1 >= t - tol && crate::sheet_surface::holds_clear(own, &circle)) {
                        return Ok(Vec::new());
                    }
                    z0 <= tol && z1 >= t - tol && crate::sheet_surface::holds_clear(own, &circle)
                } else if a.z.abs() <= 1e-9 {
                    // Along the panel: its line through the panel's material, inside its thickness.
                    // A drill clear of the panel's slab meets nothing of it: one square across
                    // the base, beside a flange standing on it, runs along the flange's plane.
                    let (b2, a2) = (Vec2::new(b.x, b.y), Vec2::new(a.x, a.y));
                    let clear = b.z - radius > t + tol || b.z + radius < -tol;
                    let inside = crate::sheet_surface_convert::extent_across(own, (b2, a2)).map(|(s0, s1)| (s0.max(span.0), s1.min(span.1)));
                    let through = !clear && inside.is_some_and(|(u0, u1)| u0 < u1);
                    if let Some(inside) = inside.filter(|_| through) {
                        span = inside;
                    }
                    if through && !(b.z - radius > tol && b.z + radius < t - tol) {
                        return Ok(Vec::new());
                    }
                    through
                } else {
                    false
                };
                if meets {
                    if found.is_some() {
                        return Ok(Vec::new());
                    }
                    found = Some((*p, across, b, a, span));
                }
            }
            let Some((p, across, b, a, (u0, u1))) = found else { return Ok(Vec::new()) };
            if boxes.iter().any(|(k, bx)| *k != Some(p) && !boxes_apart(&reach, &Some(*bx))) {
                return Ok(Vec::new());
            }
            if out.recesses.iter().any(|r| r.panel == p) || !out.sheet.cuts.is_empty() {
                return Ok(Vec::new());
            }
            let Some(panel) = out.sheet.panels.get_mut(p) else { return Ok(Vec::new()) };
            if across {
                let circle = Region2 { outer: Loop2::circle(Vec2::new(b.x, b.y), radius), holes: Vec::new() };
                if !crate::sheet_surface::holds_clear(&panel.region, &circle) {
                    return Ok(Vec::new());
                }
                panel.region.holes.push(circle.outer.ccw().reversed());
            } else {
                // Through the panel's material only, a hair past it at either end.
                let past = t * 1e-2;
                out.bores.push(crate::sheet::SheetBore { panel: Some(p), base: b + a * (u0 - past), axis: a, radius, height: u1 - u0 + 2.0 * past });
            }
        }
        plans.push((i, out));
    }
    let mut handled = Vec::with_capacity(plans.len());
    for (i, sk) in plans {
        handled.push(sk.body.clone());
        if let Some(slot) = st.surface_sheets.get_mut(i) {
            *slot = sk;
        }
        rebuild_surface_sheet(st, i)?;
    }
    Ok(handled)
}

/// An untapered extrude join one sheet thick in the plane of a flat panel of a sheet made from
/// surfaces (#6748), folded or flat, of the bodies the join takes part in (see [`takes_part`]),
/// that touches that panel's material and no other piece of the sheet: its footprint becomes part
/// of the panel's outline, so the flat, its DXF and a refold keep it, and it follows its sketch as
/// a cut there does. The sheets' bodies it joined; none, and nothing changed, for any other join,
/// which the generic path takes.
pub(super) fn surface_sheet_join(
    f: &Feature,
    st: &mut ModelState,
    targets: &[String],
    constructor: Option<&ExtrudeConstructor>,
) -> Result<Vec<String>> {
    let FeatureKind::Extrude { sketch, profiles, extent, .. } = &f.kind else { return Ok(Vec::new()) };
    if extent.taper.is_some() {
        return Ok(Vec::new());
    }
    let Some(c) = constructor else { return Ok(Vec::new()) };
    let Some(ss) = st.sketch(sketch.0) else { return Ok(Vec::new()) };
    if c.plane != ss.plane {
        return Ok(Vec::new());
    }
    let regions = solidcraft_sketch::merge_regions(&select_profiles(ss, profiles)?);
    let plane = ss.plane;
    let n = plane.normal();
    let (lo, hi) = (c.interval.0.min(c.interval.1), c.interval.0.max(c.interval.1));
    for (i, sk) in st.surface_sheets.iter().enumerate() {
        if !st.body(&sk.body).is_some_and(|mb| takes_part(f, targets, mb)) {
            continue;
        }
        let Some((panels, _)) = sk.pieces_in_space() else { continue };
        let Ok(dev) = sk.develop() else { continue };
        let Ok(pieces) = crate::sheet_surface::pieces(&dev, &mut sheet::FlatWork::new()) else { continue };
        let t = sk.thickness;
        let tol = 1e-7;
        // Each region into the one panel it is one sheet thick in, touches and joins, apart from
        // every other piece in the flat.
        let mut out = sk.clone();
        let mut placed = 0usize;
        for region in &regions {
            let mut found = None;
            for (p, frame, own) in &panels {
                if frame.z.cross(n).len() > 1e-9 {
                    continue;
                }
                let s0 = (frame.shift - plane.origin).dot(n);
                let s1 = s0 + t * frame.z.dot(n);
                if (lo - s0.min(s1)).abs() > tol || (hi - s0.max(s1)).abs() > tol {
                    continue;
                }
                let inv = frame.inverse();
                let chart = |q: Vec2| {
                    let w = inv.point(plane.to_world(q));
                    Vec2::new(w.x, w.y)
                };
                let mapped = crate::sheet_surface_convert::rigid_region(region, chart);
                let Ok(joined) =
                    kernel::planar_regions_boolean(std::slice::from_ref(own), std::slice::from_ref(&mapped), kernel::BoolOp::Union, 1e-7)
                else {
                    continue;
                };
                if joined.len() != 1 {
                    continue;
                }
                let Some(Some(at)) = dev.placements.get(*p) else { continue };
                let flat = at.region(&mapped);
                let others: Vec<Region2> =
                    pieces.iter().filter(|(k, _)| !matches!(k, crate::sheet_surface::Piece::Panel(q) if q == p)).map(|(_, r)| r.clone()).collect();
                let overlap = kernel::planar_regions_boolean(std::slice::from_ref(&flat), &others, kernel::BoolOp::Intersect, 1e-7)
                    .map(|r| r.iter().map(Region2::area).sum::<f64>());
                if !overlap.is_ok_and(|a| a <= 1e-9) {
                    return Ok(Vec::new());
                }
                found = Some((*p, mapped));
                break;
            }
            let Some((p, mapped)) = found else { break };
            let Some(panel) = out.sheet.panels.get_mut(p) else { return Ok(Vec::new()) };
            let grown =
                kernel::planar_regions_boolean(std::slice::from_ref(&panel.region), std::slice::from_ref(&mapped), kernel::BoolOp::Union, 1e-7)?;
            let Ok([one]) = <[Region2; 1]>::try_from(grown) else { return Ok(Vec::new()) };
            panel.region = one;
            placed += 1;
        }
        if placed == 0 {
            continue;
        }
        if placed < regions.len() {
            return Ok(Vec::new());
        }
        // The flat grows by what was joined, or the sheet can't hold it (a tab beside a bend
        // that its zone's trim would take back).
        let area = |rs: &[Region2]| rs.iter().map(Region2::area).sum::<f64>();
        let mut built = out.clone();
        built.build()?;
        let added = regions.iter().map(Region2::area).sum::<f64>();
        if ((area(&built.regions) - area(&sk.regions)) - added).abs() > 1e-6 * added.max(1.0) {
            return Ok(Vec::new());
        }
        let body = out.body.clone();
        if let Some(slot) = st.surface_sheets.get_mut(i) {
            *slot = out;
        }
        rebuild_surface_sheet(st, i)?;
        return Ok(vec![body]);
    }
    Ok(Vec::new())
}

/// The letters `f` cut, where it is an extrude cut from a sketch with text: the sketch's plane
/// and the regions of the cut that are letters, with their counters.
pub(super) fn cut_letters(st: &ModelState, f: &Feature) -> Option<(Plane, Vec<Region2>)> {
    let cut = extrude_cut(st, f).ok().flatten()?;
    if cut.operation != Operation::Cut || cut.text.is_empty() || cut.text.len() > MAX_TEXT_CURVES {
        return None;
    }
    let letters: Vec<Region2> = cut.regions.iter().filter(|r| sheet::loop_lies_on(&r.outer, &cut.text)).take(MAX_LABELS).cloned().collect();
    (!letters.is_empty()).then_some((cut.plane, letters))
}

/// [`exact_box`] of a hole's drill, the cylinder from `base` along `axis`; None (never apart) for
/// a drill that makes no cylinder.
fn drill_box(base: Vec3, axis: Vec3, radius: f64, height: f64) -> Option<Aabb3> {
    let axis = axis.normalized()?;
    if ![base.x, base.y, base.z, radius, height].iter().all(|v| v.is_finite()) || !(radius > 0.0) || height == 0.0 {
        return None;
    }
    exact_box(&kernel::cylinder(base, axis, radius, height).ok()?)
}

/// The largest size a cone takes (mm): beyond it the tessellation and booleans lose precision.
const MAX_CONE_SIZE: f64 = 1e7;

/// A cone or truncated cone (#5641): the base circle of `radius` centred on `base`, the top circle
/// of `top_radius` `height` along `axis` (a negative height goes the other way). A top radius of
/// 0 (or below a micrometre) makes a full cone ending in an apex. Built, as the torus is, by
/// revolving its half section, a trapezoid (a triangle for a full cone) with an edge on the axis.
/// Its faces are named `bottom`, `side` and `top` ([`crate::lineage::cone_roles`]).
pub(super) fn cone(base: Vec3, axis: Vec3, radius: f64, top_radius: f64, height: f64) -> Result<Traced> {
    let in_range = |v: f64| v.is_finite() && v.abs() <= MAX_CONE_SIZE;
    if !(in_range(base.x) && in_range(base.y) && in_range(base.z)) {
        return Err(DocError::Invalid("the cone's base is out of range".into()));
    }
    if !(in_range(radius) && in_range(top_radius) && in_range(height)) {
        return Err(DocError::Invalid(format!("cone sizes must be finite and within {MAX_CONE_SIZE} mm")));
    }
    if !(radius > 1e-6) {
        return Err(DocError::Invalid("the cone's radius must be positive".into()));
    }
    if top_radius < 0.0 {
        return Err(DocError::Invalid("the cone's top radius must be 0 or more".into()));
    }
    if !(height.abs() > 1e-6) {
        return Err(DocError::Invalid("the cone's height must not be zero".into()));
    }
    let dir = axis.normalized().ok_or_else(|| DocError::Invalid("the cone's axis has no direction".into()))?;
    let (dir, h) = if height < 0.0 { (-dir, -height) } else { (dir, height) };
    // The half section in the XZ plane, the axis along +Z through the origin, revolved there
    // (where an apex on the axis comes out exact; built in a slanted frame directly, its side
    // surface measured too large) and then moved into place.
    let section = if top_radius > 1e-6 {
        vec![Vec2::ZERO, Vec2::new(radius, 0.0), Vec2::new(top_radius, h), Vec2::new(0.0, h)]
    } else {
        vec![Vec2::ZERO, Vec2::new(radius, 0.0), Vec2::new(0.0, h)]
    };
    let plane = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
    let region = Region2 { outer: Loop2::polygon(&section), holes: vec![] };
    let mut bodies = kernel::revolve(&plane, &[region], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU)?;
    let body = match (bodies.pop(), bodies.is_empty()) {
        (Some(b), true) => b,
        _ => return Err(DocError::Invalid("the cone could not be built".into())),
    };
    // Read off the revolve's own history; moving the body keeps its faces' order.
    let roles = crate::lineage::cone_roles(&body);
    if (dir - Vec3::Z).len() < 1e-12 && base.len() == 0.0 {
        return Ok(Traced::made(body, roles));
    }
    // A rotation taking +Z onto the axis (x and y square to it, right-handed), then the base.
    let x = Plane::from_normal(base, dir).ok_or_else(|| DocError::Invalid("the cone's axis has no direction".into()))?.x;
    let y = dir.cross(x);
    let m = [[x.x, x.y, x.z, 0.0], [y.x, y.y, y.z, 0.0], [dir.x, dir.y, dir.z, 0.0], [base.x, base.y, base.z, 1.0]];
    Ok(Traced::made(kernel::transform_matrix(&body, m)?, roles))
}

/// A hole tool's exact geometry, for [`surface_hole_cut`].
#[derive(Clone, Copy, Debug)]
pub(super) enum PrimitivePrism {
    /// A cylinder: base centre, unit axis, radius and signed height.
    Cylinder { base: Vec3, axis: Vec3, radius: f64, height: f64 },
}

/// The most outline segments of a sketch's text a cut tells letters by, and the most letters it
/// tells (see [`crate::SurfaceSheet::labels`]).
const MAX_TEXT_CURVES: usize = 20_000;
const MAX_LABELS: usize = 1000;

/// Refuses an extrude drawn on `plane` over `interval` (along the plane's normal) that would leave,
/// in a flat panel it reaches (`reach`: the tools' boxes) of a sheet made from surfaces, a layer
/// thinner than the extrusion resolution: the material it keeps or takes, or what it leaves
/// beside either face. Past a rounding of the numbers its depths come from, a cap within a few
/// units in the last place of a face is on it. The boolean doesn't resolve such a layer, so the
/// sheet would keep what the cut took or lose what it kept; the extrude's own interval shows it.
pub(super) fn surface_thin_layer_refused(
    st: &ModelState,
    reach: &[Option<Aabb3>],
    plane: &Plane,
    interval: (f64, f64),
    targets: &[String],
) -> Result<()> {
    let n = plane.normal();
    let (lo, hi) = (interval.0.min(interval.1), interval.0.max(interval.1));
    let all = targets.len() == 1 && targets.first().is_some_and(|t| t == "*");
    for sk in st.surface_sheets.iter().filter(|sk| all || targets.is_empty() || targets.contains(&sk.body)) {
        let Some((panels, boxes)) = sk.pieces_in_space() else { continue };
        let t = sk.thickness;
        for (p, frame, _) in &panels {
            if frame.z.cross(n).len() > 1e-9 {
                continue;
            }
            // The panel's faces along the normal, and the part of them the extrude spans.
            let s0 = (frame.shift - plane.origin).dot(n);
            let s1 = s0 + t * frame.z.dot(n);
            let (p0, p1) = (s0.min(s1), s0.max(s1));
            let (z0, z1) = (lo.max(p0), hi.min(p1));
            if z1 <= z0 {
                continue;
            }
            let ulps = 4.0 * f64::EPSILON * (frame.shift.len() + plane.origin.len() + lo.abs() + hi.abs() + t);
            if ![z0 - p0, p1 - z1, z1 - z0].into_iter().any(|d| d > ulps && d < 1e-6) {
                continue;
            }
            if boxes.iter().any(|(k, b)| *k == Some(*p) && reach.iter().any(|r| !boxes_apart(r, &Some(*b)))) {
                return Err(DocError::Invalid(
                    "sheet machining depth is below the supported 0.000001 mm extrusion resolution; increase the layer depth".into(),
                ));
            }
        }
    }
    Ok(())
}

/// The exact box of `body`, grown by twice its tolerance: bodies whose boxes are apart do not
/// meet. None when it can't be worked out.
pub(super) fn exact_box(body: &Body) -> Option<Aabb3> {
    let b = body.exact_bounds().ok()?;
    (!b.is_empty() && b.min.is_finite() && b.max.is_finite()).then(|| b.inflated(2.0 * body.record().tolerance + 1e-6))
}

/// Whether the line through `at` along `dir` is known to pass farther than `reach` from the
/// exact box of `mb` ([`exact_box`]) everywhere, and so from every point of the body. Unknown is
/// never.
pub(super) fn line_beyond_reach(mb: &ModelBody, at: Vec3, dir: Vec3, reach: f64) -> bool {
    let Some(b) = exact_box(&mb.body) else { return false };
    let b = b.inflated(reach);
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    for (a, d, min, max) in [(at.x, dir.x, b.min.x, b.max.x), (at.y, dir.y, b.min.y, b.max.y), (at.z, dir.z, b.min.z, b.max.z)] {
        if d == 0.0 {
            if a < min || a > max {
                return true;
            }
            continue;
        }
        let (t0, t1) = ((min - a) / d, (max - a) / d);
        lo = lo.max(t0.min(t1));
        hi = hi.min(t0.max(t1));
    }
    lo > hi
}

/// Whether two boxes are known to be apart (an unknown box is never apart).
pub(super) fn boxes_apart(a: &Option<Aabb3>, b: &Option<Aabb3>) -> bool {
    let (Some(a), Some(b)) = (a, b) else { return false };
    a.max.x < b.min.x || b.max.x < a.min.x || a.max.y < b.min.y || b.max.y < a.min.y || a.max.z < b.min.z || b.max.z < a.min.z
}

/// The face that [`FaceSlice::loops`], [`FaceSlice::outline`] and [`FaceSlice::recesses`] measure:
/// the mesh, the plane the outline's coordinates live in, the point `at` and normal `n` that levels
/// are measured from, and the tolerance. Callers build it once and pass it on unchanged.
/// (`n` stays a `Vec3` until #2675 lands `UnitVec3` upstream.)
struct FaceSlice<'a> {
    mesh: &'a Mesh,
    plane: Plane,
    at: Vec3,
    n: Vec3,
    tol: f64,
}

impl FaceSlice<'_> {
    /// Outline loops (in `plane` coordinates) of the mesh triangles lying in the plane at distance
    /// `level` from `at` along `n`: the edges used by one of them only, chained.
    fn loops(&self, level: f64) -> Vec<Ring<Vec2>> {
        self.outline(level).into_iter().map(|l| l.into_iter().map(|(p, _)| p).collect()).collect()
    }

    /// [`FaceSlice::loops`] with each point's B-rep face across the outline from it to the next
    /// point (the wall the outline runs along there): the face off the plane that meets both points.
    fn outline(&self, level: f64) -> Vec<Vec<(Vec2, Option<u32>)>> {
        let (mesh, plane, at, n, tol) = (self.mesh, &self.plane, self.at, self.n, self.tol);
        let on_plane = |t: &[u32; 3]| t.iter().all(|k| mesh.positions.get(*k as usize).is_some_and(|p| ((*p - at).dot(n) - level).abs() < tol));
        let on: Vec<[u32; 3]> = mesh.triangles.iter().copied().filter(on_plane).collect();
        let pos = |k: u32| mesh.positions.get(k as usize).copied().unwrap_or_default();
        let q = |p: Vec3| ((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64, (p.z * 1e6).round() as i64);
        // The faces off the plane at each point on it, by position to a millionth of a millimetre
        // either way (a neighbouring face's mesh may round a shared point differently).
        let mut touch: std::collections::HashMap<(i64, i64, i64), Vec<u32>> = Default::default();
        for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face).filter(|(t, _)| !on_plane(t)) {
            for p in t.iter().map(|k| pos(*k)).filter(|p| ((*p - at).dot(n) - level).abs() < tol) {
                let faces = touch.entry(q(p)).or_default();
                if !faces.contains(f) {
                    faces.push(*f);
                }
            }
        }
        let near = |p: Vec3| {
            let (x, y, z) = q(p);
            let cells = (-1..=1).flat_map(|i| (-1..=1).flat_map(move |j| (-1..=1).map(move |k| (x + i, y + j, z + k))));
            let mut faces: Vec<u32> = cells.filter_map(|c| touch.get(&c)).flatten().copied().collect();
            faces.sort_unstable();
            faces.dedup();
            faces
        };
        let across = |a: Vec3, b: Vec3| {
            let at_b = near(b);
            near(a).into_iter().find(|f| at_b.contains(f))
        };
        chain_loops(mesh, plane, &on, across)
    }
}

/// The loops (in `plane` coordinates) that bound the triangles `on` of the mesh, each point with
/// `across(it, next)`: the face the outline runs along from it to the next point.
fn chain_loops(
    mesh: &solidcraft_geom::Mesh,
    plane: &Plane,
    on: &[[u32; 3]],
    across: impl Fn(Vec3, Vec3) -> Option<u32>,
) -> Vec<Vec<(Vec2, Option<u32>)>> {
    // Boundary edges by position (meshes may repeat vertices per triangle).
    let pos = |k: u32| mesh.positions.get(k as usize).copied().unwrap_or_default();
    let q = |p: Vec3| ((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64, (p.z * 1e6).round() as i64);
    let key = |a: Vec3, b: Vec3| {
        let (ka, kb) = (q(a), q(b));
        (ka.min(kb), ka.max(kb))
    };
    let mut count: std::collections::HashMap<((i64, i64, i64), (i64, i64, i64)), i32> = Default::default();
    let edges: Vec<(Vec3, Vec3)> = on.iter().flat_map(|t| [(pos(t[0]), pos(t[1])), (pos(t[1]), pos(t[2])), (pos(t[2]), pos(t[0]))]).collect();
    for (a, b) in &edges {
        *count.entry(key(*a, *b)).or_insert(0) += 1;
    }
    let mut boundary: Vec<(Vec3, Vec3)> = edges.into_iter().filter(|(a, b)| count.get(&key(*a, *b)) == Some(&1)).collect();
    // Chain into loops (plane coordinates).
    let mut loops: Vec<Vec<(Vec2, Option<u32>)>> = Vec::new();
    for _ in 0..10_000 {
        let Some((s0, mut cur)) = boundary.pop() else { break };
        let mut lp = vec![(s0, across(s0, cur))];
        for _ in 0..boundary.len() + 1 {
            if q(cur) == q(s0) {
                break;
            }
            match boundary.iter().position(|(a, _)| q(*a) == q(cur)) {
                Some(k) => {
                    let b = boundary.remove(k).1;
                    lp.push((cur, across(cur, b)));
                    cur = b;
                }
                None => {
                    lp.push((cur, None));
                    break;
                }
            }
        }
        let raw: Vec<(Vec2, Option<u32>)> = lp.iter().map(|(p, f)| (plane.to_local(*p), *f)).collect();
        // Drop points in the middle of straight runs.
        let m = raw.len();
        let pts: Vec<(Vec2, Option<u32>)> = (0..m)
            .filter_map(|i| {
                let (p, c, n) = (raw.get((i + m - 1) % m)?.0, *raw.get(i)?, raw.get((i + 1) % m)?.0);
                ((c.0 - p).cross(n - c.0).abs() > 1e-9 * (c.0 - p).len().max(1e-9) * (n - c.0).len().max(1e-9)).then_some(c)
            })
            .collect();
        if pts.len() >= 3 {
            loops.push(pts);
        }
    }
    loops
}

/// The body a placed plastic feature joins: by name, else the one under the point.
fn body_under(st: &ModelState, body: &Option<crate::BodyName>, p: Vec3) -> Result<String> {
    let i = body_at(st, body, &[p])?;
    let b = st.bodies.get(i).ok_or_else(|| DocError::Invalid("there is no body".into()))?;
    let tolerance = (b.body.size() * 1e-5).max(1e-4);
    if crate::project::nearest_face(&b.mesh(), p).is_none_or(|(_, _, distance)| distance > tolerance) {
        return Err(DocError::Invalid("the plastic feature's placement is no longer on its body face".into()));
    }
    Ok(b.name.clone())
}

/// Axes with z along `z` (and x along `x` when given).
fn local_frame(z: Vec3, x: Option<Vec3>) -> Result<(Vec3, Vec3, Vec3)> {
    let z = z.normalized().ok_or_else(|| DocError::Invalid("direction".into()))?;
    let x = x.and_then(|x| (x - z * x.dot(z)).normalized()).unwrap_or_else(|| z.any_perp());
    let y = z.cross(x);
    Ok((x, y, z))
}

/// The smallest size a plastic feature's lengths may have, in millimetres. Thinner walls and
/// shorter arms than this are below what the solid booleans can resolve.
const PLASTIC_MIN_SIZE: f64 = 0.01;

/// `v`, the feature's `what`, when it is at least [`PLASTIC_MIN_SIZE`].
fn plastic_size(feature: &str, what: &str, v: f64) -> Result<f64> {
    if v.is_finite() && v >= PLASTIC_MIN_SIZE {
        Ok(v)
    } else {
        // Tiny values in exponent form: 1e-320 written out is hundreds of zeros.
        let got = if v != 0.0 && v.abs() < 1e-3 { format!("{v:e}") } else { format!("{v}") };
        Err(DocError::Invalid(format!("the {feature}'s {what} must be at least {PLASTIC_MIN_SIZE} mm (got {got} mm)")))
    }
}

pub(super) fn plastic_eval(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let len = |e: &str| val(vals, e, Kind::Length);
    // The plastic rule of the body a feature goes on gives its defaults (draft, clearance).
    let rule = |target: &str| -> Result<Option<crate::plastic::PlasticValues>> {
        doc.plastic_rule_for(target).map(|r| doc.plastic_values(vals, &r)).transpose()
    };
    // Each tool stands on the face as drawn: SMLib joins and cuts touching faces exactly.
    match &f.kind {
        FeatureKind::Boss {
            position,
            direction,
            diameter,
            height,
            hole_diameter,
            hole_depth,
            draft,
            fillet,
            ribs,
            rib_thickness,
            rib_length,
            rib_offset,
            body,
        } => {
            let target = body_under(st, body, position.0)?;
            let r0 = plastic_size("boss", "diameter", len(diameter)?)? / 2.0;
            let h = plastic_size("boss", "height", len(height)?)?;
            let dr = match draft {
                Some(e) => val(vals, e, Kind::Angle)?,
                None => rule(&target)?.map_or(0.0, |r| r.draft),
            };
            let rf = match fillet {
                Some(e) => len(e)?.max(0.0),
                None => 0.0,
            };
            let r_top = r0 - h * dr.tan();
            if r_top <= 0.0 {
                return Err(DocError::Invalid("the draft closes the boss before its top".into()));
            }
            let (x, _, z) = local_frame(direction.0, None)?;
            // The post: a (tapered) cylinder standing on the face.
            let base = Plane::new(position.0, x, z.cross(x)).ok_or_else(|| DocError::Invalid("boss frame".into()))?;
            let disc = Region2 { outer: Loop2::circle(Vec2::ZERO, r0), holes: Vec::new() };
            let post = if dr.abs() > 1e-12 {
                vec![kernel::extrude_tapered(&base, &disc, h, 1.0, -dr, kernel::DraftCorner::default())?]
            } else {
                kernel::extrude(&base, &[disc], 0.0, h)?
            };
            let mut tools: Vec<Traced> = post.into_iter().map(|b| crate::lineage::part_tool(b, "post")).collect();
            if let Some(n) = ribs {
                let n = val(vals, n, Kind::Unitless)?.round();
                if !(0.0..=64.0).contains(&n) {
                    return Err(DocError::Invalid("0…64 ribs".into()));
                }
                let t = match rib_thickness {
                    Some(e) => len(e)?,
                    None => r0 * 0.3,
                };
                let l = match rib_length {
                    Some(e) => len(e)?,
                    None => r0,
                };
                let off = match rib_offset {
                    Some(e) => len(e)?,
                    None => 0.0,
                };
                let hr = h - off;
                if n > 0.0 {
                    plastic_size("boss", "rib_thickness", t)?;
                    plastic_size("boss", "rib_length", l)?;
                    plastic_size("boss", "rib height (height less rib_offset)", hr)?;
                }
                for k in 0..n as usize {
                    let a = std::f64::consts::TAU * k as f64 / n;
                    let (u, v) = (x * a.cos() + z.cross(x) * a.sin(), z.cross(x) * a.cos() - x * a.sin());
                    let rp = Plane::new(position.0, u, v).ok_or_else(|| DocError::Invalid("rib frame".into()))?;
                    let rect = Region2 {
                        outer: Loop2::polygon(&[
                            Vec2::new(r0 * 0.5, -t / 2.0),
                            Vec2::new(r0 + l, -t / 2.0),
                            Vec2::new(r0 + l, t / 2.0),
                            Vec2::new(r0 * 0.5, t / 2.0),
                        ]),
                        holes: Vec::new(),
                    };
                    tools.extend(kernel::extrude(&rp, &[rect], 0.0, hr)?.into_iter().map(|b| crate::lineage::part_tool(b, &format!("rib{k}"))));
                }
            }
            apply_traced(doc, st, f, tools, Operation::Join, std::slice::from_ref(&target))?;
            // The root fillet: rounds the circle where the post meets the face.
            if rf > 1e-9 {
                let i = st.bodies.iter().position(|b| b.name == target).ok_or_else(|| DocError::Invalid("boss body".into()))?;
                let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("boss body".into())) };
                let mut warn = None;
                let edges = resolve_edges(&mb.body, &[position.0 + x * r0], &[], &mut warn)?;
                let nb = mb.traced().then(kernel::fillet(&mb.body, &edges, rf)?);
                if let Some(slot) = st.bodies.get_mut(i) {
                    *slot = ModelBody::traced_new(mb.name, nb, mb.feature);
                }
            }
            if let Some(hd) = hole_diameter {
                let rh = plastic_size("boss", "hole_diameter", len(hd)?)? / 2.0;
                let depth = match hole_depth {
                    Some(e) => plastic_size("boss", "hole_depth", len(e)?)?,
                    None => h,
                };
                if !(rh > 0.0 && rh < r_top && depth > 0.0) {
                    return Err(DocError::Invalid("the boss hole must fit inside the boss".into()));
                }
                let hp = Plane::new(position.0, x, z.cross(x)).ok_or_else(|| DocError::Invalid("hole frame".into()))?;
                let tool = kernel::extrude(&hp, &[Region2 { outer: Loop2::circle(Vec2::ZERO, rh), holes: Vec::new() }], h - depth, h)?;
                let tool = tool.into_iter().map(|b| crate::lineage::part_tool(b, "hole")).collect();
                apply_traced(doc, st, f, tool, Operation::Cut, std::slice::from_ref(&target))?;
            }
            Ok(())
        }
        FeatureKind::Lip { face, width, height, groove, gap, outside, body } => {
            let target = body_under(st, body, face.0)?;
            let mb = st.body(&target).cloned().ok_or_else(|| DocError::Invalid("body".into()))?;
            let n = planar_normal_at(&mb.body, face.0).ok_or_else(|| DocError::Invalid("pick the planar rim face".into()))?;
            let what = if *groove { "groove" } else { "lip" };
            let (w, h) = (plastic_size(what, "width", len(width)?)?, plastic_size(what, "height", len(height)?)?);
            let g = match gap {
                Some(e) => len(e)?,
                None if *groove => rule(&target)?.map_or(0.0, |r| r.clearance),
                None => 0.0,
            };
            if !(g >= 0.0) {
                return Err(DocError::Invalid("the groove's gap must not be negative".into()));
            }
            let plane = Plane::from_normal(face.0, n).ok_or_else(|| DocError::Invalid("rim plane".into()))?;
            let tol = (mb.body.size() * 1e-3).max(1e-3);
            let mesh = mb.body.mesh(&solidcraft_kernel::MeshTolerance::chord(tol))?;
            let slice = FaceSlice { mesh: &mesh, plane, at: face.0, n, tol };
            let mut loops = slice.loops(0.0);
            loops.sort_by(|a, b| b.signed_area().abs().total_cmp(&a.signed_area().abs()));
            let ccw = |l: &Ring<Vec2>| if l.signed_area() < 0.0 { l.iter().rev().copied().collect::<Ring<_>>() } else { l.clone() };
            // The inner edge is an opening (the cavity under a rim): not the foot of a boss or a
            // wall standing on the face, whose side the band would run against.
            let opening = |l: &Ring<Vec2>| {
                let eps = (tol * 4.0).max(1e-3);
                l.edges().filter_map(|(a, b)| Some(((*a + *b) * 0.5, (*b - *a).normalized()?))).take(3).any(|(m, t)| {
                    // Just over the face, inside the loop (to the left of a ccw loop's edges).
                    let q = plane.to_world(m + Vec2::new(-t.y, t.x) * eps) + n * eps;
                    !mesh.contains(q)
                })
            };
            let outer = loops.first().map(ccw);
            let openings: Vec<Ring<Vec2>> = loops.iter().skip(1).map(ccw).filter(|l| opening(l)).collect();
            // A rim runs round one opening, the cavity; smaller ones (a screw hole) may sit in it.
            // Openings alike in size (a plate's holes) leave no edge that is the rim's.
            let alike = match openings.as_slice() {
                [a, b, ..] => b.signed_area().abs() > 0.5 * a.signed_area().abs(),
                _ => false,
            };
            let (Some(_), Some(_), false) = (outer, openings.first(), alike) else {
                // A solid box top has no opening: Shell that face first, then pick the rim
                // around the cavity (#10437).
                let what = match (loops.len(), openings.len()) {
                    (0 | 1, _) => "Lip needs the rim of a shelled body: Shell a face open first, then pick the rim around that opening (a face with an outer edge and an inner hole)"
                        .to_string(),
                    (_, 0) => "Lip needs a rim around an opening: this face's inner edges are where other features stand on it, not a shell cavity. Shell a face open, then pick that rim"
                        .to_string(),
                    (_, k) => format!(
                        "Lip needs one clear opening as the rim: this face has {k} openings alike in size (for example a plate of holes). Shell one cavity, or pick a rim that rings a single opening"
                    ),
                };
                return Err(DocError::Invalid(what));
            };
            // A lip: the band raised from the rim's own faces, its walls continuing the walls below.
            if !*groove {
                let b = kernel::raise_band(&mb.body, face.0, w, h, *outside)?;
                let slot = st.bodies.iter_mut().find(|x| x.name == target).ok_or_else(|| DocError::Invalid("body".into()))?;
                *slot = ModelBody::traced_new(target.clone(), Traced::step(b, &[&mb.traced()]), slot.feature);
                return Ok(());
            }
            // A groove: the band along the chosen edge, inside the rim face, its gap included,
            // cut down from the face.
            let (plane, band) = kernel::rim_band(&mb.body, face.0, w + g, 0.0, *outside)?;
            let tool = kernel::extrude(&plane, &[band], -(h + g), 0.0)?.into_iter().map(|b| crate::lineage::part_tool(b, "groove")).collect();
            apply_traced(doc, st, f, tool, Operation::Cut, std::slice::from_ref(&target))
        }
        FeatureKind::SnapFit { position, direction, hook, length, thickness, width, catch_depth, catch_length, base_height, new_body, body } => {
            let target = body_under(st, body, position.0)?;
            let size = |what: &str, e: &str| plastic_size("snap fit", what, len(e)?);
            let (l, t, w) = (size("length", length)?, size("thickness", thickness)?, size("width", width)?);
            let (cd, cl) = (size("catch_depth", catch_depth)?, size("catch_length", catch_length)?);
            let bh = base_height.as_deref().map(|e| size("base_height", e)).transpose()?.unwrap_or(0.0);
            if !(cl < l) {
                return Err(DocError::Invalid("the snap fit's catch must be shorter than its arm".into()));
            }
            let (x, y, z) = local_frame(direction.0, Some(hook.0))?;
            // Side profile (x = toward the catch, y = up the arm), extruded across the width.
            // The base reaches `base_height` below the face, joined or a separate body.
            let y0 = -bh;
            let pts = [Vec2::new(-t, y0), Vec2::new(0.0, y0), Vec2::new(0.0, l - cl), Vec2::new(cd, l - cl), Vec2::new(0.0, l), Vec2::new(-t, l)];
            let o = position.0 - y * (w / 2.0);
            let pl = Plane::new(o, x, z).ok_or_else(|| DocError::Invalid("snap fit frame".into()))?;
            let mut lp = Loop2::polygon(&pts);
            if lp.signed_area() < 0.0 {
                lp = lp.reversed();
            }
            let sgn = if pl.normal().dot(y) >= 0.0 { 1.0 } else { -1.0 };
            let (lo, hi) = if sgn > 0.0 { (0.0, w) } else { (-w, 0.0) };
            let tool = kernel::extrude(&pl, &[Region2 { outer: lp, holes: Vec::new() }], lo, hi)?;
            let tool = tool.into_iter().map(|b| crate::lineage::part_tool(b, "arm")).collect();
            let op = if *new_body { Operation::NewBody } else { Operation::Join };
            apply_traced(doc, st, f, tool, op, std::slice::from_ref(&target))
        }
        FeatureKind::Rest { position, direction, along, width, length, height, draft, thickness, body } => {
            let target = body_under(st, body, position.0)?;
            let size = |what: &str, e: &str| plastic_size("rest", what, len(e)?);
            let (w, h) = (size("width", width)?, size("height", height)?);
            let l = length.as_deref().map(|e| size("length", e)).transpose()?;
            let dr = match draft {
                Some(e) => val(vals, e, Kind::Angle)?,
                None => rule(&target)?.map_or(0.0, |r| r.draft),
            };
            let t = thickness.as_deref().map(|e| size("thickness", e)).transpose()?;
            if t.is_some_and(|t| 2.0 * t >= w.min(l.unwrap_or(w))) {
                return Err(DocError::Invalid("the rest's wall must be thinner than half its width".into()));
            }
            if !(dr.abs() < 1.0) {
                return Err(DocError::Invalid("the rest's draft must be under 57°".into()));
            }
            let shrink = h * dr.tan();
            let closes = match t {
                Some(t) => 2.0 * shrink >= t,
                None => 2.0 * shrink >= w.min(l.unwrap_or(w)),
            };
            if closes {
                return Err(DocError::Invalid("the draft closes the rest before its top".into()));
            }
            let (x, y, _) = local_frame(direction.0, along.map(Vec3::from))?;
            // Sized at the face, where the tool starts.
            let shape = |grow: f64| match l {
                Some(l) => Loop2::polygon(&[
                    Vec2::new(-l / 2.0 - grow, -w / 2.0 - grow),
                    Vec2::new(l / 2.0 + grow, -w / 2.0 - grow),
                    Vec2::new(l / 2.0 + grow, w / 2.0 + grow),
                    Vec2::new(-l / 2.0 - grow, w / 2.0 + grow),
                ]),
                None => Loop2::circle(Vec2::ZERO, w / 2.0 + grow),
            };
            let holes = match t {
                Some(t) => vec![shape(-t).reversed()],
                None => Vec::new(),
            };
            let region = Region2 { outer: shape(0.0), holes };
            let base = Plane::new(position.0, x, y).ok_or_else(|| DocError::Invalid("rest frame".into()))?;
            let tool = if dr.abs() > 1e-12 {
                vec![kernel::extrude_tapered(&base, &region, h, 1.0, -dr, kernel::DraftCorner::default())?]
            } else {
                kernel::extrude(&base, &[region], 0.0, h)?
            };
            let tool = tool.into_iter().map(|b| crate::lineage::part_tool(b, "rest")).collect();
            apply_traced(doc, st, f, tool, Operation::Join, std::slice::from_ref(&target))
        }
        _ => Err(DocError::Invalid(format!("{} is not a plastic feature", f.name))),
    }
}

/// The sketch line each line of a chain drawn on the sketch lines `ids` (in that order) was
/// drawn on, and the line's key: the sketch line's id, with the line's place on it (`l3.2`, from
/// 1) where that sketch line is drawn in several.
fn drawn_lines(ss: &SolvedSketch, ids: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for id in ids {
        let n = ss.sketch.curve_index(id).map_or(0, |ci| ss.sketch.segs(ci).len());
        for k in 0..n {
            out.push((id.clone(), if n == 1 { id.clone() } else { format!("{id}.{}", k + 1) }));
        }
    }
    out
}

/// What made the panels [`tag_panels`] keys.
#[derive(Clone, Copy)]
enum PanelsBy<'a> {
    /// Bends on rims of the sheet before the feature (flanges, hems), and on each other along a
    /// path flange's path, whose lines these are (see [`drawn_lines`]), in its order.
    Bends(&'a [(String, String)]),
    /// A fold along the line between these points (the body's coordinates), from the first to
    /// the second, parting panels of the sheet before it.
    Fold([Vec3; 2]),
}

/// Gives each panel a feature just added to a sheet made from surfaces (from the `from`th) its
/// key and what made it (see [`crate::sheet_surface_convert::PanelMade`]): the feature and what
/// the panel was made on, never its place among the panels, the picks or the joints, nor a
/// measure (#7990), so a panel keeps its key however the feature's picks are ordered, whatever
/// other panels come and go and whatever the sizes. A bend's panel is made on the rim of `before`
/// (the sheet as it was) that its bend lies along, by what made that rim's edge (`F15/F12/l2`: a
/// flange on the line `l2` of the panel the base flange `F12` made; `F17/F15/F12/l2/far`: a
/// return on that flange's far edge); a path flange's next panel on its path's line (`F18/l4`);
/// a fold's piece on its panel, numbered in the order the pieces start along the fold's line
/// (`F20/F12#1`). Panels that would share a key (bends on two pieces of one rim, or on rims
/// nothing says what made) are numbered in the order they stand along their bends' anchors. The
/// keys are never names: the model names the panels' faces (see `lineage::sheet_lineage`).
fn tag_panels(sk: &mut crate::SurfaceSheet, before: Option<&crate::SurfaceSheet>, from: usize, feature: u64, by: PanelsBy<'_>) {
    use crate::sheet_surface_convert::{PanelMade, SheetPart};
    use solidcraft_develop::JointKind;
    let n = sk.sheet.panels.len();
    // The joints the feature made: those past the ones the sheet had.
    let had = before.map_or(0, |b| b.sheet.joints.len());
    let made_at = |p: usize| sk.sheet.joints.iter().skip(had).find(|j| j.b.panel == p && j.a.panel != p).cloned();
    let mut keys: Vec<String> = sk.sheet.panels.iter().map(|p| p.name.clone()).collect();
    // The fold's line in the chart of the panel it folds: its pieces are ordered along it.
    let fold_way = |panel: usize| -> Option<Vec2> {
        let (PanelsBy::Fold([a, b]), Some(before)) = (by, before) else { return None };
        let dev = before.develop().ok()?;
        let place = crate::sheet_surface_convert::fold_onto(&before.sheet, &dev, &before.places).ok()?.places.get(panel).copied().flatten()?;
        let (qa, qb) = (place.inverse().point(a), place.inverse().point(b));
        Some(Vec2::new(qb.x - qa.x, qb.y - qa.y))
    };
    // Each new panel: what made it, its key, and where it stands, which only orders panels that
    // would share a key.
    let mut made: Vec<(usize, PanelMade, String, f64)> = Vec::new();
    for p in from..n {
        let Some(j) = made_at(p) else { continue };
        let on_key = keys.get(j.a.panel).cloned().unwrap_or_default();
        let stands = j.a.origin.dot(j.a.dir);
        match by {
            PanelsBy::Fold(_) => {
                // Along the fold's own line: the anchor runs across the zone's start, the piece
                // starting where its anchor does.
                let along = fold_way(j.a.panel).map_or(j.b.origin.dot(j.b.dir), |way| j.b.origin.dot(way));
                made.push((p, PanelMade::Fold { feature, from: on_key.clone(), place: 0 }, format!("F{feature}/{on_key}"), along));
            }
            PanelsBy::Bends(lines) if j.a.panel >= from => {
                // Along a path flange's path: its line at the panel's place along the path.
                let mut depth = 1;
                let mut at = j.a.panel;
                while depth <= n {
                    match made_at(at) {
                        Some(k) if k.a.panel >= from => {
                            at = k.a.panel;
                            depth += 1;
                        }
                        _ => break,
                    }
                }
                let Some((curve, line)) = lines.get(depth) else { continue };
                made.push((p, PanelMade::Chain { feature, curve: curve.clone() }, format!("F{feature}/{line}"), stands));
            }
            PanelsBy::Bends(_) => {
                // On a rim of the sheet as it was: a bend's anchor lies inside the edge it was
                // drawn on by its setback, within the radius and a thickness, whichever way the
                // bend was placed on the edge.
                let band = match &j.kind {
                    JointKind::Bend(bend) => 2.0 * (bend.radius + sk.thickness),
                    _ => 0.0,
                };
                let on = before.and_then(|b| b.rim_part_along(j.a.panel, j.a.origin, j.a.dir, band));
                let key = match &on {
                    Some(SheetPart::Rim { panel, edge }) => format!("F{feature}/{panel}/{}", edge.role().replace(':', ".")),
                    _ => format!("F{feature}/{on_key}/?"),
                };
                made.push((p, PanelMade::Bend { feature, on }, key, stands));
            }
        }
    }
    // Keys several panels would share, and a fold's pieces, numbered from 1 in the order they
    // stand: a rank, never the measure.
    let mut shared: std::collections::BTreeMap<String, Vec<(f64, usize)>> = std::collections::BTreeMap::new();
    for (k, (_, _, key, at)) in made.iter().enumerate() {
        shared.entry(key.clone()).or_default().push((*at, k));
    }
    for (key, mut members) in shared {
        let fold = matches!(by, PanelsBy::Fold(_));
        members.sort_by(|x, y| x.0.total_cmp(&y.0));
        for (rank, (_, k)) in members.iter().enumerate() {
            let Some((p, recipe, _, _)) = made.get_mut(*k) else { continue };
            let numbered = if fold || members.len() > 1 { format!("{key}#{}", rank + 1) } else { key.clone() };
            if let PanelMade::Fold { place, .. } = recipe {
                *place = rank + 1;
            }
            if let Some(slot) = keys.get_mut(*p) {
                *slot = numbered;
            }
        }
    }
    for (p, recipe, _, _) in made {
        let Some(key) = keys.get(p).cloned() else { continue };
        if let Some(panel) = sk.sheet.panels.get_mut(p) {
            panel.name.clone_from(&key);
        }
        sk.recipes.retain(|(k, _)| *k != key);
        sk.recipes.push((key, recipe));
    }
}
