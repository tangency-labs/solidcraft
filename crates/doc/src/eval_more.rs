//! Evaluation of Emboss, Rib/Web, Replace Face, Align and Remove (features built from the
//! kernel's extrude, boolean, offset and transform operations).

use super::*;
use crate::EmbossFace;
use crate::apply_point;
use crate::sheet;
use solidcore_record::{Loop2, Seg2};
use solidcraft_develop::prelude::*;
use solidcraft_geom::{Aabb3, ArcMap, Ring};

pub(super) fn eval(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState, warning: &mut Option<String>) -> Result<()> {
    match &f.kind {
        FeatureKind::Emboss { sketch, profiles, depth, deboss, targets, face, offset_x, offset_y, angle } => {
            let ss = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?;
            // The profiles are placed before anything is built from them (#20386), so the wrap, the
            // projection and the plain emboss all get the offsets and the angle the same way.
            let given = |e: &Option<crate::Expr>, kind: Kind| e.as_deref().map_or(Ok(0.0), |e| val(vals, e, kind));
            let shift = Vec2::new(given(offset_x, Kind::Length)?, given(offset_y, Kind::Length)?);
            let regions = emboss_placement(select_profiles(ss, profiles)?, shift, given(angle, Kind::Angle)?)?;
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
                let at = crate::apply_point(&crate::mat_mul(&inverse, &author_frame), face.at.0);
                let name = f
                    .face_names
                    .first()
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| DocError::Invalid("selected emboss face has no persistent identity".into()))?;
                let hit = {
                    let mut scoped = ModelState::default();
                    scoped.bodies.push(mb.clone());
                    crate::name_lookup::locate_face(&scoped, name, at).ok_or_else(|| DocError::reselect(name.as_str()))?
                };
                let signed = if *deboss { -d } else { d };
                // A sketch tangent to a cylinder wall, outside or a bore, takes the wrap (whose
                // refusals stand): a complete wall, or any other wall face bounded by two arcs and
                // two lines (#12029); a sketch off the wall and every other face are projected.
                let result = match kernel::cylinder_emboss_anchor(&mb.body, hit.face, hit.point) {
                    Ok(at) if kernel::infer_cylinder_emboss(&mb.body, &plane, &regions, signed)?.is_some() => {
                        kernel::emboss_cylinder_at(&mb.body, at, &plane, &regions, signed)?
                    }
                    _ => {
                        let at = kernel::emboss_anchor(&mb.body, hit.face, hit.point)?;
                        kernel::emboss_project_face(&mb.body, hit.face, at, &plane, &regions, signed)?
                    }
                };
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
            // The rib is built against the bodies where its sketch is drawn: a body of another,
            // moved component is met where it is shown (#17655), whether it takes part or only
            // stops the wall.
            let shown = targets_as_shown(st, f, &["*".to_string()])?;
            let tools = rib_tools(vals, &shown, f, None)?;
            apply_op(doc, st, f, tools, Operation::Join, &[])
        }
        FeatureKind::ReplaceFace { faces, target, body } => {
            let tp = doc.resolve_plane_in(vals, target, 0, f.component, Some(st))?;
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
                *slot = mb.remade(b);
            }
            Ok(())
        }
        FeatureKind::Align { bodies, from, to, from_normal, to_normal, flip, from_plane, to_plane, axes, from_frame, to_frame, angle } => {
            // A plane on either side is worked out now, so the bodies follow it when it moves.
            let (mut from, mut from_normal) = (from.0, from_normal.and_then(|n| n.normalized()));
            if let Some(p) = from_plane {
                let pl = doc.resolve_plane_in(vals, p, 0, f.component, Some(&*st))?;
                (from, from_normal) = (pl.origin, Some(pl.normal()));
            }
            // So is a frame (#6257): its origin and z, found again from the geometry.
            if let Some(fr) = from_frame {
                let (o, z) = fr.resolve(doc, st)?;
                (from, from_normal) = (o, Some(z));
            }
            let target = match to_frame {
                Some(_) => None,
                None => to_plane.as_ref().map(|p| doc.resolve_plane_in(vals, p, 0, f.component, Some(&*st))).transpose()?,
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
                    // A rigid alignment keeps each face's identity. Carry the transform's
                    // exact history, as Move does, instead of naming the result by overlap
                    // with the body still at its old position (#20288).
                    *mb = mb.remade(Traced::step(nb, &[&mb.traced()]));
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
    let FeatureKind::Rib { sketch, curves, thickness, depth, flip, web, normal, extend } = &f.kind else {
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
        tools.push(if *normal { rib_tool_normal(st, &ss, g, t, depth, *flip)? } else { rib_tool(st, &ss, g, t, depth, *flip, *extend)? });
    }
    Ok(tools)
}

/// Unit outward normal of the planar face through `p`: of the planar faces whose plane runs
/// through it, the one nearest it. A face elsewhere in such a plane (the wall of a notch, the end
/// of a rib, in line with a rim) is no pick of `p`, though its plane runs through it as exactly.
pub(super) fn planar_normal_at(b: &Body, p: Vec3) -> Option<Vec3> {
    if b.is_mesh() {
        return None;
    }
    let tol = (b.size() * 2e-3).max(1e-3);
    let rec = b.record();
    let mut best: Option<(f64, f64, Vec3)> = None;

    for (i, face) in rec.faces.iter().enumerate() {
        let Some(plane) = b.planar_face(i) else { continue };
        let h = plane.height(p).abs();
        if h > tol * 5.0 {
            continue;
        }
        let local_p = plane.to_local(p);
        let mut loops: Vec<Ring<Vec2>> = Vec::new();
        let mut min_edge_dist = f64::INFINITY;

        for lp in &face.loops {
            let solidcore_record::brep::Loop::Edges { edges } = lp else { continue };
            let mut pts = Vec::new();
            for u in edges {
                let Some(edge) = rec.edges.get(u.edge as usize) else { continue };
                let Ok(eval) = solidcraft_geom::brep::eval::CurveEval::new(edge.parameters()) else { continue };
                let [t0, t1] = edge.interval;
                let n_samples = match &eval {
                    solidcraft_geom::brep::eval::CurveEval::Line { .. } => 1,
                    _ => 8,
                };
                for k in 0..n_samples {
                    let frac0 = k as f64 / n_samples as f64;
                    let frac1 = (k + 1) as f64 / n_samples as f64;
                    let t_a = if u.forward { t0 + (t1 - t0) * frac0 } else { t1 - (t1 - t0) * frac0 };
                    let t_b = if u.forward { t0 + (t1 - t0) * frac1 } else { t1 - (t1 - t0) * frac1 };
                    if let (Some(a), Some(b)) = (eval.point(t_a), eval.point(t_b)) {
                        let seg_dist = p.dist_to_segment(a, b);
                        min_edge_dist = min_edge_dist.min(seg_dist);
                        if pts.is_empty() || pts.last().is_some_and(|last: &Vec2| last.dist(plane.to_local(a)) > 1e-9) {
                            pts.push(plane.to_local(a));
                        }
                    }
                }
            }
            if pts.len() >= 3 {
                loops.push(Ring::new(pts));
            }
        }

        let is_inside = if let Some(outer) = loops.iter().max_by(|a, b| a.signed_area().abs().total_cmp(&b.signed_area().abs())) {
            let inside_outer = outer.contains(local_p);
            let inside_hole = loops.iter().filter(|lp| !std::ptr::eq(*lp, outer)).any(|hole| hole.contains(local_p));
            inside_outer && !inside_hole
        } else {
            false
        };

        let face_dist = if is_inside { 0.0 } else { min_edge_dist };
        let candidate = (face_dist, h, plane.normal());
        if best.as_ref().is_none_or(|(bd, bh, _)| face_dist < *bd || ((face_dist - *bd).abs() < 1e-6 && h < *bh)) {
            best = Some(candidate);
        }
    }

    best.filter(|(d, ..)| *d < tol * 5.0).map(|(_, _, n)| n)
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

/// A contour flange's profile, drawn on the sketch curves `ids` end to end in that order: each
/// curve's exact pieces (a line's or an arc's; see `Sketch::exact_segs`), each curve run the way
/// the profile goes, the first toward the second and each next one on from where the one before
/// ends, as [`chain_points`] reads a chain; and for each piece the sketch curve it was drawn on
/// and its key: the curve's id, with the piece's place on the curve as drawn (`s3.2`, from 1)
/// where the curve is drawn in several. A free-form curve (an ellipse, a conic, a spline) is
/// refused: its panel's flat has no closed form (#15743).
fn contour_profile(ss: &SolvedSketch, ids: &[String]) -> Result<(Vec<Seg2>, Vec<(String, String)>)> {
    let mut curves: Vec<(&String, Vec<Seg2>)> = Vec::with_capacity(ids.len());
    for id in ids {
        let ci = ss.sketch.curve_index(id).ok_or_else(|| DocError::Unknown(format!("curve `{id}`")))?;
        if ss.sketch.curves.get(ci).is_some_and(|c| c.kind.is_freeform()) {
            return Err(crate::sheet_surface_convert::free_form_contour(id));
        }
        let pieces = ss.sketch.exact_segs(ci).unwrap_or_else(|| ss.sketch.segs(ci));
        if pieces.is_empty() {
            return Err(DocError::Invalid(format!("the curve `{id}` in a contour flange has no length")));
        }
        curves.push((id, pieces));
    }
    let ends = |pieces: &[Seg2]| pieces.first().map(Seg2::start).zip(pieces.last().map(Seg2::end));
    let (mut segs, mut drawn): (Vec<Seg2>, Vec<(String, String)>) = (Vec::new(), Vec::new());
    for (k, (id, pieces)) in curves.iter().enumerate() {
        let Some((start, end)) = ends(pieces.as_slice()) else { continue };
        let back = match segs.last().map(Seg2::end) {
            Some(at) => end.dist(at) < start.dist(at),
            None => curves.get(k + 1).and_then(|(_, next)| ends(next.as_slice())).is_some_and(|(s, e)| start.dist(s) < 1e-6 || start.dist(e) < 1e-6),
        };
        let n = pieces.len();
        let key = |i: usize| ((*id).clone(), if n == 1 { (*id).clone() } else { format!("{id}.{}", i + 1) });
        let run: Vec<(Seg2, (String, String))> = if back {
            pieces.iter().enumerate().rev().map(|(i, s)| (s.reversed(), key(i))).collect()
        } else {
            pieces.iter().enumerate().map(|(i, s)| (*s, key(i))).collect()
        };
        if let (Some(at), Some((first, _))) = (segs.last().map(Seg2::end), run.first())
            && first.start().dist(at) > 1e-5
        {
            return Err(DocError::Invalid("the contour flange's curves are not connected end to end".into()));
        }
        for (s, key) in run {
            segs.push(s);
            drawn.push(key);
        }
    }
    Ok((segs, drawn))
}

/// A rib wall: the band swept from the curves across the plane by `depth` (or to the body),
/// `t` thick about the sketch plane.
fn rib_tool(st: &ModelState, ss: &SolvedSketch, ids: &[String], t: f64, depth: Option<f64>, flip: bool, extend: bool) -> Result<Body> {
    let mut pts = chain_points(ss, ids)?;
    let meshes: Vec<_> = st.bodies.iter().map(|b| b.mesh()).collect();
    let reach = meshes.iter().map(|m| m.bounds()).fold(0.0f64, |acc, bb| acc.max((bb.max - bb.min).len()));
    if extend {
        extend_rib_ends(st, ss, &mut pts, t, reach);
    }
    let (Some(a), Some(b)) = (pts.first().copied(), pts.last().copied()) else { return Err(DocError::Invalid("empty rib curve".into())) };
    let chord = (b - a).normalized().ok_or_else(|| DocError::Invalid("a closed or zero-length rib curve".into()))?;
    let tol = 1e-7 * (1.0 + reach);
    let normal = ss.plane.normal();
    // Whether `q`, moved `off` along the sketch normal (across the wall's thickness), is in a body.
    let inside = |q: Vec2, off: f64| {
        let w = ss.plane.to_world(q) + normal * off;
        meshes.iter().any(|m| m.contains(w))
    };
    // Where the body begins and ends again from `q` along `d` (a unit direction in the plane),
    // up to `limit`: the ray's crossings of the bodies' surfaces, with the first beyond `limit`,
    // each span between two tested at its middle. Fixed steps passed over thin walls or landed
    // on their faces, so whether a rib found the floor of a long tray hung on its length (#16874).
    // `off` moves the ray along the sketch normal, to one of the wall's faces or between them.
    let entry = |q: Vec2, d: Vec2, limit: f64, off: f64| -> Option<(f64, f64)> {
        let (o, dir) = (ss.plane.to_world(q) + normal * off, ss.plane.dir_to_world(d));
        let mut cuts = Vec::new();
        for m in &meshes {
            let mut from = 0.0;
            while cuts.len() < 4096 {
                let Some((s, _)) = m.raycast(o + dir * from, dir) else { break };
                from += s;
                cuts.push(from);
                if !(from <= limit) {
                    break;
                }
                from += tol;
            }
        }
        ray_entry(cuts, tol, |s| inside(q + d * s, off)).filter(|(h, _)| *h <= limit)
    };
    let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    let samples = ((len / (t.min(len) * 0.25).max(len / 400.0)).ceil() as usize).clamp(2, 400);
    let along: Vec<Vec2> = (0..=samples).map(|k| point_on_polyline(&pts, len * k as f64 / samples as f64)).collect();
    // Toward the material: the side of the curve whose points reach a body across the plane
    // (the nearer body when both sides do), unless flipped.
    let mut n = chord.perp();
    let probe = |side: Vec2| {
        let (mut reached, mut nearest) = (0usize, f64::INFINITY);
        for q in along.iter().step_by((along.len() / 24).max(1)) {
            if let Some((h, _)) = entry(*q, side, reach * 2.0 + 1.0, 0.0) {
                reached += 1;
                nearest = nearest.min(h);
            }
        }
        (reached, nearest)
    };
    let (ahead, behind) = (probe(n), probe(n * -1.0));
    if behind.0 > ahead.0 || (behind.0 == ahead.0 && behind.1 < ahead.1) {
        n = n * -1.0;
    }
    if flip {
        n = n * -1.0;
    }
    // From each point of the curves, across the plane until the body begins (or `depth`);
    // walls that reach the body go a little into it so they merge with it, never more than
    // half through the material there, so a rib on a floor thinner than the overlap does not
    // come out below it (#16874).
    let limit = depth.unwrap_or(reach * 2.0 + 1.0);
    let step = (limit.min(reach * 2.0 + 1.0) / 400.0).max(1e-3);
    let overlap = (len / samples as f64 * 2.0).max(step * 2.0).min(t);
    // How far the wall runs across the plane from curve point `q`. The wall's end is flat across
    // its thickness, so it is found on both of the wall's faces and between them, not only on the
    // sketch plane: a face that curves across the thickness (a cylinder standing across the rib,
    // a round) is further away at the wall's faces than at its middle, and an end found on the
    // middle alone left a sliver of air at the faces, wider the thicker the rib (#18261). The end
    // goes as far as the furthest of these reaches the body, and on into the material, never
    // beyond where the nearest leaves it again; where no flat end can do both (a thin curved wall),
    // it is found on the sketch plane as before.
    let across = |q: Vec2| -> Result<f64> {
        let centre = entry(q, n, limit, 0.0);
        let hits: Vec<(f64, f64)> = [-0.5, -0.25, 0.25, 0.5].iter().filter_map(|k| entry(q, n, limit, k * t)).chain(centre).collect();
        let Some(nearest) = centre.or_else(|| hits.iter().copied().min_by(|a, b| a.0.total_cmp(&b.0))) else {
            return depth.ok_or_else(|| DocError::Invalid("the rib curves do not face a body (give a depth)".into()));
        };
        let far = hits.iter().map(|(h, _)| *h).fold(f64::NEG_INFINITY, f64::max);
        let leaves = hits.iter().map(|(_, out)| *out).fold(f64::INFINITY, f64::min);
        if let Some(d) = depth
            && far >= d
        {
            return Ok(d);
        }
        if far < leaves {
            Ok(far + overlap.min(0.5 * (leaves - far)))
        } else {
            let (h, out) = nearest;
            Ok(h + overlap.min(0.5 * (out - h)))
        }
    };
    // An end of the curves often sits exactly on a wall (a rib from wall to wall), where the
    // body's inside is ambiguous; search from just inside the curve there, so the end does not
    // read the wall it touches as the body ahead and the end face stays square.
    let nudge = len / samples as f64 * 0.01;
    let mut wall: Vec<(Vec2, f64)> = Vec::with_capacity(along.len());
    for (i, q) in along.iter().enumerate() {
        let from = if i == 0 {
            point_on_polyline(&pts, nudge)
        } else if i == samples {
            point_on_polyline(&pts, len - nudge)
        } else {
            *q
        };
        let h = across(from)?;
        // A run-on end meets the wall it reaches between two samples, where the run across the
        // plane jumps (from the far side to straight into that wall): find the jump closely, so
        // the outline does not cut a wedge off the corner there. Looked for wherever the run
        // changes more steeply than 45° between samples, whatever the thickness: samples are
        // about a quarter of the thickness apart, so a jump of under `t` is still a corner.
        if extend
            && let Some(&(q0, h0)) = wall.last()
            && (h - h0).abs() > q0.dist(*q).max(tol)
        {
            let (mut lo, mut hi, mut h_lo, mut h_hi) = (0.0, 1.0, h0, h);
            for _ in 0..12 {
                let m = 0.5 * (lo + hi);
                // A probe that faces no body (a gap between the samples) keeps the bracket.
                let Ok(hm) = across(q0.lerp(*q, m)) else { break };
                if (hm - h0).abs() < (hm - h).abs() {
                    (lo, h_lo) = (m, hm);
                } else {
                    (hi, h_hi) = (m, hm);
                }
            }
            // A step only where the run really jumps; a steep but smooth face keeps its slope.
            // One point of the curve for both sides of the jump: a step straight across, not two
            // curve points a hair apart, which a thin rib's outline could not close (#16874's tray).
            if (h_hi - h_lo).abs() > 0.5 * (h - h0).abs() {
                let at = q0.lerp(*q, 0.5 * (lo + hi));
                wall.push((at, h_lo));
                wall.push((at, h_hi));
            }
        }
        wall.push((*q, h));
    }
    let mut along: Vec<Vec2> = wall.iter().map(|(q, _)| *q).collect();
    along.dedup_by(|b, a| b.dist(*a) <= tol);
    let far: Vec<Vec2> = wall.iter().map(|(q, h)| *q + n * *h).collect();
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

/// Run a rib curve's open ends (`pts`, in sketch coordinates) on in their own direction until they
/// meet the bodies, and on until the wall's two faces, `t` apart about the sketch plane, are in
/// the material too, so a curved face is reached across the whole thickness (#13019). Decided on
/// the bodies' exact faces. An end already on or in the material, or facing none within `reach`,
/// stays where it is.
fn extend_rib_ends(st: &ModelState, ss: &SolvedSketch, pts: &mut Vec<Vec2>, t: f64, reach: f64) {
    let normal = ss.plane.normal();
    // A point the kernel cannot place counts as outside: the end then stays or runs on no further.
    let solid = |q: Vec2, off: f64| {
        let w = ss.plane.to_world(q) + normal * off;
        st.bodies.iter().any(|b| matches!(b.body.classify_point(w), Ok(c) if c != kernel::PointClass::Outside))
    };
    let step = (reach / 200.0).max(1e-3);
    for at_start in [true, false] {
        let n = pts.len();
        let (end, inner) = if at_start { (0, 1) } else { (n.saturating_sub(1), n.saturating_sub(2)) };
        let (Some(p), Some(q)) = (pts.get(end).copied(), pts.get(inner).copied()) else { continue };
        let Some(dir) = (p - q).normalized() else { continue };
        if solid(p, 0.0) {
            continue;
        }
        let Some(k) = (1..=200).map(f64::from).find(|k| solid(p + dir * (k * step), 0.0)) else { continue };
        // Refine between the last outside and the first inside step.
        let (mut lo, mut hi) = ((k - 1.0) * step, k * step);
        for _ in 0..30 {
            let m = 0.5 * (lo + hi);
            if solid(p + dir * m, 0.0) { hi = m } else { lo = m }
        }
        // On until both faces of the wall are in the material (a face curving away across the
        // thickness), at most the thickness further.
        let mut h = hi + t * 0.05;
        for i in 0..=20 {
            let d = hi + t * f64::from(i) / 20.0;
            let c = p + dir * d;
            if solid(c, t * 0.5) && solid(c, -t * 0.5) {
                h = d + t * 0.05;
                break;
            }
            h = d;
        }
        let tip = p + dir * h;
        if at_start {
            pts.insert(0, tip);
        } else {
            pts.push(tip);
        }
    }
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
    let meshes: Vec<_> = st.bodies.iter().map(|b| b.mesh()).collect();
    let reach = meshes.iter().map(|m| m.bounds()).fold(0.0f64, |acc, bb| acc.max((bb.max - bb.min).len()));
    let tol = 1e-7 * (1.0 + reach);
    let inside = |q: Vec3| meshes.iter().any(|m| m.contains(q));
    // Where the body begins from each point of the curves, along `sign` × the sketch normal:
    // the ray's crossings of the bodies' surfaces, each span between two tested at its middle,
    // so no thin wall is stepped over (#16874).
    let limit = reach * 2.0 + 1.0;
    let step = (limit / 400.0).max(1e-3);
    let hits = |sign: f64| -> Vec<(f64, f64)> {
        let dir = normal * sign;
        let mut out = Vec::new();
        for q in &along {
            let mut cuts = Vec::new();
            for m in &meshes {
                let mut from = 0.0;
                while cuts.len() < 4096 {
                    let Some((s, _)) = m.raycast(*q + dir * from, dir) else { break };
                    from += s;
                    cuts.push(from);
                    if !(from <= limit) {
                        break;
                    }
                    from += tol;
                }
            }
            if let Some(h) = ray_entry(cuts, tol, |s| inside(*q + dir * s)).filter(|(h, _)| *h <= limit) {
                out.push(h);
            }
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
            // The farthest entry, and into its material at most half way through it, so the
            // wall does not come out beyond a plate thinner than the overlap (#16874).
            let Some((far, out)) = reached.iter().copied().max_by(|a, b| a.0.total_cmp(&b.0)) else {
                return Err(DocError::Invalid("the rib curves do not face a body along the sketch normal (give a depth)".into()));
            };
            far + (t * 0.5).max(step).min(0.5 * (out - far))
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

/// Where the material begins and ends along a ray, from where the ray crosses the bodies'
/// surfaces (`cuts`, distances from its start, in any order) and whether the point at a
/// distance lies inside: the start and end of the first span between crossings whose middle is
/// inside, starting at 0 when the ray starts in the material. Crossings within `tol` of each
/// other are one; a span is only tested at its middle, clear of the faces at its ends.
fn ray_entry(mut cuts: Vec<f64>, tol: f64, inside: impl Fn(f64) -> bool) -> Option<(f64, f64)> {
    cuts.retain(|c| c.is_finite() && *c > 0.0);
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|c, prev| *c - *prev <= tol);
    let mut start = 0.0;
    for c in cuts {
        if c - start > tol && inside(0.5 * (start + c)) {
            return Some((start, c));
        }
        start = c;
    }
    None
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
/// the sheet's free edges lies there. The free edge is read at the named edge's middle, on its
/// exact curve, then at the point the name found: a pick at an end of the edge (a corner of the
/// panel) is as near the edge beside it, and read there it could take that one, whatever the name
/// said. The middle is read only where the body's edge of that index passes through the point the
/// name found (the mesh's edges are the B-rep's, in its order).
fn named_free_edge(st: &ModelState, sk: &crate::SurfaceSheet, name: &str, near: Vec3) -> Option<crate::sheet_surface_features::FreeEdge> {
    let body = st.body(&sk.body)?;
    let hit = crate::naming::locate_edge(body, name, near).ok()?;
    let reach = crate::name_lookup::pick_reach(body);
    let on_it = solidcraft_kernel::edge_distance(&body.body, hit.edge, hit.point).ok().flatten().is_some_and(|d| d <= reach);
    on_it
        .then(|| solidcraft_kernel::edge_middle(&body.body, hit.edge).ok().flatten())
        .flatten()
        .and_then(|mid| sk.free_edge_at(mid).ok().flatten())
        .or_else(|| sk.free_edge_at(hit.point).ok().flatten())
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
        bend_relief: rv.bend_relief_shape,
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
            sheet.countersinks = read.countersinks;
            let solid = if sheet.recesses.is_empty() && sheet.countersinks.is_empty() { solid } else { sheet.build()? };
            (sheet, solid)
        }
    };
    // Its faces named from the faces of the body its panels were (see `PanelMade::Converted`).
    let none = vec![None; mb.body.face_count()];
    let lineage = crate::lineage::sheet_lineage(Some(crate::lineage::SheetBefore::found(&mb, &none)), &sheet, &solid);
    if let Some(b) = st.bodies.iter_mut().find(|b| b.name == *body) {
        replace_traced_sheet_body(b, solid, lineage, &sheet.frames);
    }
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

/// The sheet made from surfaces an edge flange on `edges` works on: the one [`surface_sheet_at`]
/// finds at its first pick; else, where no body is named and the first edge is split into
/// segments, the one with a curved edge at that pick, so that the split is refused for what it
/// is (a curved edge is no free edge, by which `surface_sheet_at` finds a sheet).
fn surface_flange_sheet(st: &ModelState, body: &Option<crate::BodyName>, f: &Feature, edges: &[crate::FlangeEdge]) -> Option<usize> {
    let first = edges.first();
    surface_sheet_at(st, body, f.edge_names.first().map(String::as_str), first.map(|e| e.point.0)).or_else(|| {
        let p = first.filter(|e| body.is_none() && !e.segments.is_empty())?.point.0;
        st.surface_sheets.iter().position(|s| s.curved_edge_at(p))
    })
}

/// The stretches the `k`th edge `e` of an edge flange bends in, each `(from, to, height, angle,
/// flip)`: `from` and `to` fractions of the stretch of the edge the sheet still has, in order
/// along it. An edge with no segments is one stretch, the whole edge, at `height` and `angle` (its
/// own, else the feature's) to the feature's side `flip`. A split edge makes one stretch for each
/// run of its segments that share a height, an angle and a side (a segment's own, else the
/// edge's; a segment's `flip` replaces the feature's outright): the same bend carries on.
///
/// The runs keep this invariant: the first starts at the edge's start (0), the last ends at its
/// end (1), and each starts where the one before it ends, so they are sorted, contiguous and
/// non-overlapping, and every stretch the segments give is covered. `sheet.flange` accepts
/// segments that meet within [`crate::sheet::SEGMENT_JOIN_EPS`], and a junction it accepts is shut
/// here, on the numbers given, before anything is built: a stretch starts where the edge is
/// covered so far, and one narrower than that tolerance, or one ending behind the covered end, is
/// no bend of its own (its angle and height do not build) but goes to the run before it. A list
/// that does not tile the edge (a design file's: the command refuses it) is refused, naming the
/// segment, as is a segment's angle outside 0…180° or a height below `least` for its angle (the
/// least height the feature's bend takes); a list of such narrow stretches only is the whole edge.
fn segment_runs(
    k: usize,
    e: &crate::FlangeEdge,
    vals: &BTreeMap<String, Value>,
    (height, angle, flip): (f64, f64, bool),
    least: &dyn Fn(f64) -> f64,
) -> Result<Vec<(f64, f64, f64, f64, bool)>> {
    use crate::sheet::{SEGMENT_JOIN_EPS, WHOLE_EDGE};
    let (edge_height, edge_angle) = (height, angle);
    if e.segments.is_empty() {
        return Ok(vec![(WHOLE_EDGE.0, WHOLE_EDGE.1, edge_height, edge_angle, flip)]);
    }
    let mut runs: Vec<(f64, f64, f64, f64, bool)> = Vec::new();
    // The end of the edge the runs made so far cover. A run starts here, never before it, so the
    // runs are sorted, contiguous and non-overlapping by construction.
    let mut covered = WHOLE_EDGE.0;
    // The `to` of the segment before, as stored: the segments must tile the edge, and a stored
    // list that does not is refused rather than built past the edge or with part of it bare.
    let mut prev_to = WHOLE_EDGE.0;
    for (j, seg) in e.segments.iter().enumerate() {
        if !(0.0..=1.0).contains(&seg.from) {
            return Err(DocError::Invalid(format!("edges[{k}].segments[{j}].from must be a fraction from 0 to 1")));
        }
        if !(0.0..=1.0).contains(&seg.to) {
            return Err(DocError::Invalid(format!("edges[{k}].segments[{j}].to must be a fraction from 0 to 1")));
        }
        if j == 0 && seg.from > SEGMENT_JOIN_EPS {
            return Err(DocError::Invalid(format!("edges[{k}].segments[0].from must be 0: the segments cover the whole edge")));
        }
        if j > 0 && (seg.from - prev_to).abs() > SEGMENT_JOIN_EPS {
            return Err(DocError::Invalid(format!(
                "edges[{k}].segments[{j}].from must equal the to of the segment before it (the segments must meet, with no gap or overlap)"
            )));
        }
        prev_to = seg.to;
        let height = match &seg.height {
            Some(x) => val(vals, x, Kind::Length)?,
            None => edge_height,
        };
        let angle = match &seg.angle {
            Some(x) => val(vals, x, Kind::Angle)?,
            None => edge_angle,
        };
        // A segment's own angle and height are checked here, where its index is known, so a
        // refusal names the segment the caller wrote.
        if !(angle > 1e-6 && angle <= std::f64::consts::PI - 1e-6) {
            return Err(DocError::Invalid(format!("edges[{k}].segments[{j}]: the flange angle must be between 0 and 180°")));
        }
        let at_least = least(angle);
        if !(height >= at_least - 1e-9) {
            return Err(DocError::Invalid(format!("edges[{k}].segments[{j}]: the flange height must be at least {at_least:.3} mm for this bend")));
        }
        let side = seg.flip.unwrap_or(flip);
        // A junction the command accepts is shut here, whether the two stretches match or not:
        // the stretch starts where the edge is covered, so no gap or overlap of the junction
        // tolerance reaches the sheet. The 1e-12 comparison below only says whether they are one
        // bend.
        let start = seg.from.max(covered);
        let start = if start <= covered + SEGMENT_JOIN_EPS { covered } else { start };
        if seg.to - start <= SEGMENT_JOIN_EPS {
            // No bend of its own: narrower than the junction tolerance, or ending behind the
            // covered end. The run before it takes it, its end moving on to the stretch's end but
            // never back, so the edge stays covered; one taken before any run is covered by the
            // run that follows, which starts at the edge's start.
            covered = covered.max(seg.to);
            if let Some(last) = runs.last_mut() {
                last.1 = covered;
            }
            continue;
        }
        let same_bend = runs.last().is_some_and(|last| (last.2 - height).abs() < 1e-12 && (last.3 - angle).abs() < 1e-12 && last.4 == side);
        if same_bend {
            // The same bend carries on: the junction closes, and the run takes the stretch's end.
            if let Some(last) = runs.last_mut() {
                last.1 = seg.to;
            }
        } else {
            // Another bend starts where the run before it ends; the first starts at the edge's
            // start however far the stretches taken before it have covered.
            let start = if runs.is_empty() { WHOLE_EDGE.0 } else { start };
            runs.push((start, seg.to, height, angle, side));
        }
        covered = seg.to;
    }
    if !runs.is_empty() && prev_to < WHOLE_EDGE.1 - SEGMENT_JOIN_EPS {
        // A stored list that stops short of the edge's end leaves part of the edge without a
        // flange: refused, as the command refuses it.
        let n = e.segments.len() - 1;
        return Err(DocError::Invalid(format!("edges[{k}].segments[{n}].to must be 1: the segments cover the whole edge")));
    }
    if runs.is_empty() {
        // A stored list of narrow stretches only (`sheet.flange` cannot write one): the edge
        // still builds whole, as the feature's own bend.
        runs.push((WHOLE_EDGE.0, WHOLE_EDGE.1, edge_height, edge_angle, flip));
    }
    // A last `to` within the junction tolerance of the edge's end is its end.
    if let Some(last) = runs.last_mut()
        && (last.1 - WHOLE_EDGE.1).abs() <= SEGMENT_JOIN_EPS
    {
        last.1 = WHOLE_EDGE.1;
    }
    Ok(runs)
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

fn relative_transform(doc: &Document, from: u64, to: u64) -> Option<crate::Mat> {
    if from == to {
        return Some(crate::IDENTITY);
    }
    let m_from = doc.component_transform(from);
    let m_to = doc.component_transform(to);
    let inv_to = crate::mat_inverse(&m_to)?;
    Some(crate::mat_mul(&inv_to, &m_from))
}

/// The sheet made from surfaces a fold is on, and its line's ends in that sheet's component frame.
/// The named body's sheet, or the first whose flat panel the line lies on.
fn surface_fold_sheet(
    doc: &Document,
    st: &ModelState,
    body: &Option<crate::BodyName>,
    sketch: &Option<crate::SketchId>,
    curve: &Option<String>,
    a: &crate::Position,
    b: &crate::Position,
    author_comp: u64,
) -> Option<(usize, Vec3, Vec3)> {
    let (source_a, source_b, source_comp) = match (sketch, curve) {
        (Some(sk), Some(c)) => {
            let ss = st.sketch(sk.0)?;
            let ci = ss.sketch.curve_index(c)?;
            let comp = doc.feature(sk.0).map(|f| f.component).unwrap_or(author_comp);
            match ss.sketch.segs(ci).as_slice() {
                [Seg2::Line { a, b }] => (ss.plane.to_world(*a), ss.plane.to_world(*b), comp),
                _ => return None,
            }
        }
        _ => (a.0, b.0, author_comp),
    };
    if let Some(n) = body {
        let i = st.surface_sheets.iter().position(|s| &s.body == n)?;
        let s = &st.surface_sheets[i];
        let sheet_comp = st.body(&s.body).map(|b| doc.model_body_component(b)).unwrap_or(author_comp);
        let m = relative_transform(doc, source_comp, sheet_comp).unwrap_or(crate::IDENTITY);
        return Some((i, apply_point(&m, source_a), apply_point(&m, source_b)));
    }
    st.surface_sheets.iter().enumerate().find_map(|(i, s)| {
        let sheet_comp = st.body(&s.body).map(|b| doc.model_body_component(b)).unwrap_or(author_comp);
        let m = relative_transform(doc, source_comp, sheet_comp).unwrap_or(crate::IDENTITY);
        let (sheet_a, sheet_b) = (apply_point(&m, source_a), apply_point(&m, source_b));
        if s.fold_panel_at(sheet_a, sheet_b) { Some((i, sheet_a, sheet_b)) } else { None }
    })
}

impl Document {
    /// The side a fold keeps put (see [`crate::FoldSide`]), decided as the fold is made, on the
    /// sheet as `st` has it before the fold: by its `fixed` point, else as the sheet joins on
    /// either side ([`crate::SurfaceSheet::fold_side`]). None for a feature that is no fold on a
    /// sheet made from surfaces.
    pub fn fold_side_at(&self, st: &ModelState, kind: &FeatureKind, component: u64) -> Result<Option<crate::FoldSide>> {
        let FeatureKind::SheetFold { sketch, curve, a, b, angle, radius, position, flip, fixed, body, .. } = kind else { return Ok(None) };
        let Some((i, a, b)) = surface_fold_sheet(self, st, body, sketch, curve, a, b, component) else { return Ok(None) };
        let Some(sk) = st.surface_sheets.get(i).filter(|sk| !sk.unfolded) else { return Ok(None) };
        let vals = self.evaluation_values();
        let ang = val(&vals, angle, Kind::Angle)?;
        let rr = match radius {
            Some(e) => val(&vals, e, Kind::Length)?,
            None => self.rule_values_at(&vals, &self.sheet_rule(Some(&sk.rule)), Some(sk.thickness))?.bend_radius,
        };
        let sheet_comp = st.body(&sk.body).map(|b| self.model_body_component(b)).unwrap_or(component);
        let m = relative_transform(self, component, sheet_comp).unwrap_or(crate::IDENTITY);
        let fixed_sheet = fixed.map(|p| apply_point(&m, p.0));
        sk.fold_side(a, b, ang, rr, position, *flip, fixed_sheet).map(Some)
    }

    /// Whether a fold's `fixed` point touches no material of the sheet made from surfaces the
    /// fold is on, as `st` has it before the fold (see `SurfaceSheet::touches`): such a
    /// point picks no side, and the `sheet.fold` command refuses it (#263). False for a fold with
    /// no fixed point, or one on no folded sheet (which fails as it is evaluated).
    pub fn fold_fixed_off_sheet(&self, st: &ModelState, kind: &FeatureKind, component: u64) -> bool {
        let FeatureKind::SheetFold { sketch, curve, a, b, fixed: Some(p), body, .. } = kind else { return false };
        let Some((i, _, _)) = surface_fold_sheet(self, st, body, sketch, curve, a, b, component) else { return false };
        let Some(sk) = st.surface_sheets.get(i).filter(|sk| !sk.unfolded) else { return false };
        let sheet_comp = st.body(&sk.body).map(|b| self.model_body_component(b)).unwrap_or(component);
        let m = relative_transform(self, component, sheet_comp).unwrap_or(crate::IDENTITY);
        let p_sheet = apply_point(&m, p.0);
        !sk.touches(p_sheet, (sk.thickness * 1e-3).max(1e-6))
    }

    /// This design with each fold that keeps no side that stays (a design saved before folds
    /// kept one) given the side it folds now, as `model`, this design's evaluation, has the sheet
    /// before it: from then on that side stays, wherever the line moves. None where no fold
    /// lacks it.
    pub fn with_fold_sides(&self, model: &Model) -> Option<Document> {
        let mut out: Option<Document> = None;
        for f in &self.features {
            let FeatureKind::SheetFold { stays: None, .. } = &f.kind else { continue };
            let Ok(Some(side)) = self.fold_side_at(&model.state_before(f.id), &f.kind, f.component) else { continue };
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

/// A segment mirrored in the sketch x axis (y → −y), exactly: an arc turns the other way,
/// free-form curves through their control points.
fn mirrored_y(s: &Seg2) -> Seg2 {
    let f = |p: Vec2| Vec2::new(p.x, -p.y);
    match *s {
        Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center: f(center), radius, start: -start, sweep: -sweep },
        other => other.map_controls(f),
    }
}

/// A region mirrored in the sketch x axis (y → −y), exactly: free-form curves through their
/// control points.
fn mirror_y(r: &Region2) -> Region2 {
    let f = |p: Vec2| Vec2::new(p.x, -p.y);
    let m = |l: &Loop2| {
        // -0.0: a start of +0.0 mirrors to -0.0, as `-start` does.
        let segs: Vec<Seg2> = l.segs.iter().map(|s| s.mapped(f, ArcMap::turn(-0.0, true))).collect();
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
            // The sketch's lines, in the sheet's plane: the rims of the panel it makes are named after them.
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
        FeatureKind::SheetContour { sketch, curves, distance, radius, rule, orientation, symmetric, flip, reverse } => {
            let ss = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?.clone();
            // The profile as the sketch draws it, exactly, end to end, and the sketch curve each
            // of its pieces was drawn on with the piece's key, the curves put in chain order first
            // (main's #15364).
            let (segs, drawn) = contour_profile(&ss, &ss.chain_order(curves)?)?;
            let w = val(vals, distance, Kind::Length)?;
            if !(w > 0.0) {
                return Err(DocError::Invalid("the contour flange distance must be positive".into()));
            }
            let r = doc.sheet_rule(rule.as_deref());
            let mut rv = doc.rule_values(vals, &r)?;
            if let Some(radius) = radius {
                rv.bend_radius = val(vals, radius, Kind::Length)?;
                if !(rv.bend_radius >= 0.0 && rv.bend_radius < 1e4) {
                    return Err(DocError::Invalid("the contour flange bend radius must be non-negative".into()));
                }
            }
            let rule = convert_rule(&rv);
            // Where the sketched profile lies across the sheet's thickness, as its depth into the
            // material from the face the sheet's surfaces are (`side1`): on that face, through the
            // middle (`center`) or on the other face (`side2`).
            let depth = match orientation.as_deref() {
                None | Some("side1") => 0.0,
                Some("center") => rule.thickness / 2.0,
                Some("side2") => rule.thickness,
                Some(_) => return Err(DocError::Invalid("contour flange orientation must be center, side1 or side2".into())),
            };
            // `reverse` (Other side) mirrors the sketch, taking the width to the other side of
            // its plane, with an orientation saved or not.
            let plane = if *reverse { Plane { origin: ss.plane.origin, x: ss.plane.x, y: ss.plane.y * -1.0 } } else { ss.plane };
            let segs: Vec<Seg2> = if *reverse { segs.iter().map(mirrored_y).collect() } else { segs };
            // Mirroring the sketch swaps the material side too. A sheet made from surfaces (#6748):
            // each piece pushed out along the normal is a panel of it.
            let name = new_name(st, f, 0);
            let (sheet, solid) =
                crate::SurfaceSheet::contour(&name, &r.name, &rule, &plane, &segs, w, depth, *symmetric, *flip != *reverse, f.id, &drawn)?;
            st.bodies.push(new_sheet_body(name, &sheet, solid, f.id));
            st.surface_sheets.push(sheet);
            Ok(())
        }
        FeatureKind::SheetCornerJoin { body, corner } => {
            let mut candidates = st.surface_sheets.iter().enumerate().filter(|(_, sheet)| match body {
                Some(name) => &sheet.body == name,
                None => st.body(&sheet.body).is_some_and(|built| corner.joint_pair(|source| sheet.bend_joint_for_edge_name(source, built)).is_ok()),
            });
            let Some((i, before)) = candidates.next() else {
                return Err(DocError::reselect(body.as_ref().map(|name| name.to_string()).unwrap_or_else(|| corner.covering.to_string())));
            };
            if candidates.next().is_some() {
                return Err(DocError::Invalid("select a sheet body; these corner sources match more than one sheet".into()));
            }
            let mut next = before.clone();
            let built = st.body(&before.body).ok_or_else(|| DocError::reselect(&before.body))?;
            corner.apply(&mut next, |source| before.bend_joint_for_edge_name(source, built), |expr| val(vals, expr, Kind::Length))?;
            *st.surface_sheets.get_mut(i).ok_or_else(|| DocError::Invalid("sheet".into()))? = next;
            rebuild_surface_sheet(st, i)
        }
        FeatureKind::SheetFlange {
            edges,
            height,
            angle,
            radius,
            position,
            flip,
            body,
            height_from,
            corner_relief_shape,
            corner_relief,
            bend_relief_shape,
            corners,
        } if surface_flange_sheet(st, body, f, edges).is_some() => {
            if corners.len() > 2000 {
                return Err(DocError::Invalid("at most 2000 flange corner choices are supported".into()));
            }
            // The sheet with a free edge at the first pick; else, for an edge split into
            // segments, the one with a curved edge there, where the split is refused for what it
            // is (see `surface_flange_sheet`).
            let at_free = surface_sheet_at(st, body, f.edge_names.first().map(String::as_str), edges.first().map(|e| e.point.0));
            let i = at_free.or_else(|| surface_flange_sheet(st, body, f, edges)).ok_or_else(|| DocError::Invalid("sheet".into()))?;
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
            // Each edge's own height and angle, else the feature's, and the stretches it bends in:
            // the whole edge, or each run of its segments (see `segment_runs`). Every segment is
            // checked before any edge is looked for, so a refusal names the segment written.
            let t = sk.sheet.thickness;
            let least = |angle: f64| from.before_leg(angle, rr, t);
            let mut runs_of = Vec::with_capacity(edges.len());
            for (k, e) in edges.iter().enumerate() {
                let height = match &e.height {
                    Some(x) => val(vals, x, Kind::Length)?,
                    None => h,
                };
                let angle = match &e.angle {
                    Some(x) => val(vals, x, Kind::Angle)?,
                    None => a,
                };
                runs_of.push(segment_runs(k, e, vals, (height, angle, *flip), &least)?);
            }
            // Every edge is found on the sheet as it was before any of them.
            let mut sk = sk.clone();
            let before = sk.clone();
            let mut picks = Vec::with_capacity(edges.len());
            for (k, (e, runs)) in edges.iter().zip(runs_of).enumerate() {
                let edge = match surface_edge_pick(st, &before, f.edge_names.get(k), e.point.0) {
                    Ok(edge) => edge,
                    // A split edge picked on a curved edge: its stretches' fractions would follow
                    // no straight line.
                    Err(_) if runs.len() > 1 && before.curved_edge_at(e.point.0) => {
                        return Err(DocError::Invalid("not supported yet: segments on a curved edge (pick a straight edge of the sheet)".into()));
                    }
                    // The sheet was found only by a curved edge at the first pick: no free edge
                    // lies there.
                    Err(_) if at_free.is_none() => return Err(no_free_edge(st, body, edges.first().map(|e| e.point.0))),
                    Err(why) => return Err(why),
                };
                picks.push((edge, runs));
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
            // The bend relief these flanges ask for in place of the rule's, at their bends' free ends.
            let own_bend = match bend_relief_shape {
                Some(name) => Some(
                    sheet::BendReliefShape::parse(name)
                        .ok_or_else(|| DocError::Invalid(format!("unknown bend relief shape `{name}` (rectangular, obround or tear)")))?,
                ),
                None => None,
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
            let mut square = Vec::with_capacity(picks.len());
            for (k, (edge, runs)) in picks.into_iter().enumerate() {
                // One bend for each run: a split edge's butt where its runs meet.
                let (joints, sides) = sk.flanges_on(edge, &runs, rr, position, from).map_err(named(k))?;
                square.push(sides);
                if let Some((relief, size)) = own {
                    for &joint in &joints {
                        sk.corner_reliefs.push((joint, relief, size, order));
                    }
                }
                if let Some(shape) = own_bend {
                    sk.bend_reliefs.extend(joints.iter().map(|&joint| (joint, shape)));
                }
                made.push(joints);
            }
            let mut open = Vec::new();
            for (k, joints) in made.iter().enumerate() {
                open.extend(sk.close_flange_corners(joints).map_err(named(k))?);
            }
            // A side laid on square no other bend met carries on the edge beside it again, now
            // that every corner of the feature is closed (#19833).
            for (k, sides) in square.iter().enumerate() {
                sk.slant_open_sides(sides).map_err(named(k))?;
            }
            // The corners on the outside of a bend that stay open (#20720): each pair of bends is
            // met once, so each corner once.
            for at in open {
                append_warning(warning, format!("the flanges at ({:.3}, {:.3}, {:.3}) {}", at.x, at.y, at.z, crate::OUTSIDE_BEND_OPEN));
            }
            let made: Vec<usize> = made.into_iter().flatten().collect();
            sk.clear_of_neighbours(&made)?;
            tag_panels(&mut sk, Some(&before), n0, f.id, PanelsBy::Bends(&[]));
            if !corners.is_empty() {
                let prev = std::mem::take(&mut sk.parts);
                let solid = sk.build()?;
                let mut built = st.body(&before.body).cloned().ok_or_else(|| DocError::reselect(&before.body))?;
                let depth = built.names.depth().unwrap_or(0).saturating_add(1);
                let lineage =
                    crate::lineage::SheetBefore::of(&built, &prev).and_then(|previous| crate::lineage::sheet_lineage(Some(previous), &sk, &solid));
                replace_traced_sheet_body(&mut built, solid, lineage.clone(), &sk.frames);
                built.names = crate::naming::NameCell::traced(
                    Some(std::sync::Arc::new(crate::naming::NamingOrigin {
                        feature: f.clone(),
                        before: std::sync::Arc::new(st.clone()),
                        depth,
                        mats: Vec::new(),
                    })),
                    lineage.map(std::sync::Arc::new),
                );
                let sources = sk.clone();
                for corner in corners {
                    let joints = corner.joint_pair(|source| sources.bend_joint_for_edge_name(source, &built))?;
                    if !joints.iter().any(|joint| made.contains(joint)) {
                        return Err(DocError::Invalid("a flange corner choice must include one of this feature's bends".into()));
                    }
                    corner.apply(&mut sk, |source| sources.bend_joint_for_edge_name(source, &built), |expr| val(vals, expr, Kind::Length))?;
                }
                sk.parts = prev;
            }
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
            // An outline that reaches none of the sheet's panels holds no holes. A recompute keeps the
            // field, so an edit that moves the outline off the sheet still builds: it says so (#19631).
            if outline.as_ref().is_some_and(Vec::is_empty) {
                append_warning(warning, "the perforation's outline does not overlap the sheet, so it holds no holes".into());
            }
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
                found.push(before.edge_left_by_cuts(surface_edge_pick(st, &before, f.edge_names.get(k), p.0)?, p.0)?);
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
            if surface_fold_sheet(doc, st, body, sketch, curve, a, b, f.component).is_some() =>
        {
            let (i, a, b) = surface_fold_sheet(doc, st, body, sketch, curve, a, b, f.component).ok_or_else(|| DocError::Invalid("sheet".into()))?;
            let sk = st.surface_sheets.get(i).ok_or_else(|| DocError::Invalid("sheet".into()))?;
            if sk.unfolded {
                return Err(DocError::Invalid("refold the sheet before folding it".into()));
            }
            let ang = val(vals, angle, Kind::Angle)?;
            let rr = match radius {
                Some(e) => val(vals, e, Kind::Length)?,
                None => doc.rule_values_at(vals, &doc.sheet_rule(Some(&sk.rule)), Some(sk.thickness))?.bend_radius,
            };
            // The side that stays is the fold's own, decided when it was made; a fold saved
            // before it kept one decides it as it was made (and keeps it once opened), by its
            // fixed point where it has one, wherever that point lies now.
            let stays = match stays {
                Some(side) => *side,
                None => {
                    let sheet_comp = st.body(&sk.body).map(|b| doc.model_body_component(b)).unwrap_or(f.component);
                    let m = relative_transform(doc, f.component, sheet_comp).unwrap_or(crate::IDENTITY);
                    let fixed_sheet = fixed.map(|p| apply_point(&m, p.0));
                    sk.fold_side(a, b, ang, rr, position, *flip, fixed_sheet)?
                }
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
        FeatureKind::SheetTab { sketch, profiles, body } => {
            let ss = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?.clone();
            let regions = solidcraft_sketch::merge_regions(&select_profiles(&ss, profiles)?);
            // The named sheet, else the one sheet with a face that takes the profile: which is never
            // a matter of the sheets' order, so a tab two sheets would take is refused.
            let candidates: Vec<usize> = match body {
                Some(n) => vec![
                    st.surface_sheets
                        .iter()
                        .position(|s| &s.body == n)
                        .ok_or_else(|| DocError::Invalid(format!("`{n}` is not a sheet metal body")))?,
                ],
                None => (0..st.surface_sheets.len()).collect(),
            };
            let mut first_error = None;
            let mut took = Vec::new();
            for i in candidates {
                let Some(mut sk) = st.surface_sheets.get(i).cloned() else { continue };
                match sk.add_tab(&ss.plane, &regions) {
                    Ok(_) => took.push((i, sk)),
                    Err(e) => {
                        first_error.get_or_insert(e);
                    }
                }
            }
            match <[(usize, crate::SurfaceSheet); 1]>::try_from(took) {
                Ok([(i, sk)]) => {
                    if let Some(slot) = st.surface_sheets.get_mut(i) {
                        *slot = sk;
                    }
                    rebuild_surface_sheet(st, i)
                }
                Err(took) if !took.is_empty() => Err(DocError::Invalid(sheet::TAB_SEVERAL.into())),
                Err(_) => Err(first_error.unwrap_or_else(|| DocError::Invalid(sheet::TAB_OFF_SHEET.into()))),
            }
        }
        FeatureKind::SheetBead {
            sketch,
            curves,
            shape,
            depth,
            width,
            angle,
            crown_width,
            base_radius,
            crown_radius,
            end_length,
            margin,
            flip,
            body,
        } => {
            if !matches!(shape.as_deref(), None | Some("trapezoid")) {
                return Err(DocError::Invalid(format!(
                    "not supported yet: a bead of shape {:?} (only \"trapezoid\")",
                    shape.as_deref().unwrap_or_default()
                )));
            }
            let ss = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?.clone();
            if curves.is_empty() || curves.len() > crate::sheet_surface_bead::MAX_BEADS {
                return Err(DocError::Invalid(format!("a bead feature needs 1…{} centrelines", crate::sheet_surface_bead::MAX_BEADS)));
            }
            let mut lines = Vec::with_capacity(curves.len());
            for id in curves {
                let ci = ss.sketch.curve_index(id).ok_or_else(|| DocError::Unknown(format!("curve `{id}`")))?;
                match ss.sketch.segs(ci).as_slice() {
                    [Seg2::Line { a, b }] => lines.push((id, ss.plane.to_world(*a), ss.plane.to_world(*b))),
                    _ => return Err(DocError::Invalid(format!("not supported yet: a bead along `{id}`, which is not a straight line"))),
                }
            }
            // A bead is a form on a planar panel of a sheet: the named sheet's, else the one sheet
            // with a planar panel the first centreline lies on. Which is never a matter of the
            // sheets' order, so a centreline two sheets would take is refused.
            let Some(&(_, a0, b0)) = lines.first() else {
                return Err(DocError::Invalid("a bead feature needs a centreline".into()));
            };
            // A line of the sketch on sheet `s`, read from the sketch's component into the sheet's
            // (its body's), as a fold's line is.
            let source = doc.feature(sketch.0).map(|x| x.component).unwrap_or(f.component);
            let on = |s: &crate::SurfaceSheet, a: Vec3, b: Vec3| {
                let comp = st.body(&s.body).map(|m| doc.model_body_component(m)).unwrap_or(source);
                let m = relative_transform(doc, source, comp).unwrap_or(crate::IDENTITY);
                let (origin, normal) = (crate::apply_point(&m, ss.plane.origin), crate::apply_vector(&m, ss.plane.normal()));
                s.bead_line(origin, normal, crate::apply_point(&m, a), crate::apply_point(&m, b))
            };
            let i = match body {
                Some(n) => st
                    .surface_sheets
                    .iter()
                    .position(|s| &s.body == n)
                    .ok_or_else(|| DocError::Invalid(format!("`{n}` is not a sheet metal body")))?,
                None => {
                    let on: Vec<usize> =
                        (0..st.surface_sheets.len()).filter(|&i| st.surface_sheets.get(i).is_some_and(|s| on(s, a0, b0).is_ok())).collect();
                    match on.as_slice() {
                        [i] => *i,
                        [] => return Err(DocError::Invalid("a bead's centreline must lie on a planar panel of a sheet".into())),
                        _ => return Err(DocError::Invalid("a bead's centreline lies on more than one sheet: name the sheet with `body`".into())),
                    }
                }
            };
            let sk = st.surface_sheets.get(i).ok_or_else(|| DocError::Invalid("sheet".into()))?;
            if sk.unfolded {
                return Err(DocError::Invalid("refold the sheet before adding beads".into()));
            }
            let rv = doc.rule_values_at(vals, &doc.sheet_rule(Some(&sk.rule)), Some(sk.thickness))?;
            let t = sk.sheet.thickness;
            let d = val(vals, depth, Kind::Length)?;
            let w = val(vals, width, Kind::Length)?;
            let a = match (angle, crown_width) {
                (Some(_), Some(_)) => return Err(DocError::Invalid("a bead takes its flanks' angle or its crown width, not both".into())),
                (Some(e), None) => val(vals, e, Kind::Angle)?,
                (None, Some(e)) => {
                    let c = val(vals, e, Kind::Length)?;
                    solidcraft_develop::bead_flank_angle(d, w, c)
                        .map_err(|_| DocError::Invalid(format!("a bead's crown width ({c}) must be at least 0 and less than its width ({w})")))?
                }
                (None, None) => std::f64::consts::FRAC_PI_4,
            };
            let or = |e: &Option<crate::Expr>, default: f64| -> Result<f64> {
                match e {
                    Some(e) => val(vals, e, Kind::Length),
                    None => Ok(default),
                }
            };
            let (rb, rc) = (or(base_radius, rv.bend_radius)?, or(crown_radius, rv.bend_radius)?);
            let (el, m) = (or(end_length, 3.0 * d)?, or(margin, 3.0 * t)?);
            if sk.beads.len().saturating_add(lines.len()) > crate::sheet_surface_bead::MAX_BEADS {
                return Err(DocError::Invalid(format!("a sheet takes at most {} beads", crate::sheet_surface_bead::MAX_BEADS)));
            }
            let mut sk = sk.clone();
            let mut notes = Vec::new();
            for (curve, pa, pb) in lines {
                let (panel, qa, qb, far) = on(&sk, pa, pb)?;
                let bead = crate::SurfaceBead {
                    feature: f.id,
                    curve: curve.clone(),
                    panel,
                    a: qa,
                    b: qb,
                    // Out of the face it is sketched on; with `flip`, out of the other one.
                    far: far != *flip,
                    depth: d,
                    width: w,
                    angle: a,
                    base_radius: rb,
                    crown_radius: rc,
                    end_length: el,
                };
                sk.check_new_bead(&bead, m, &mut notes)?;
                sk.beads.push(bead);
            }
            notes.dedup();
            if !notes.is_empty() {
                append_warning(warning, notes.join("; "));
            }
            if let Some(slot) = st.surface_sheets.get_mut(i) {
                *slot = sk;
            }
            rebuild_surface_sheet(st, i)
        }
        FeatureKind::SheetUnfold { body, refold, fixed } => {
            let i = surface_sheet_for(st, body, |s| s.unfolded == *refold).ok_or_else(|| match body {
                Some(_) => no_sheet(body),
                None => DocError::Invalid(if *refold { "no unfolded sheet to refold".into() } else { "no sheet metal body to unfold".into() }),
            })?;
            if let Some(sk) = st.surface_sheets.get_mut(i) {
                // The panel at `fixed` stays where it is: the flat lies in its plane from now on.
                if let (Some(at), false) = (fixed, *refold) {
                    sk.flat = sk.flat_at(at.0).ok_or_else(|| DocError::Invalid("the fixed point lies on no flat panel of the sheet".into()))?;
                }
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
            for s in [lo, hi] {
                // The footprint carried along the normal onto the level `s`, an affine map: its box
                // there holds it (see [`crate::sheet_surface_cuts::loop_box_in`]).
                let onto = |q: Vec2| {
                    let w = plane.to_world(q);
                    w + n * (s - (w - cut.plane.origin).dot(n))
                };
                b = b.union(&crate::sheet_surface_cuts::loop_box_in(&region.outer, onto));
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
                let (a, b) = (crate::sheet_surface_cuts::loop_box(&r.outer), crate::sheet_surface_cuts::loop_box(&mapped.outer));
                !(a.max.x < b.min.x || b.max.x < a.min.x || a.max.y < b.min.y || b.max.y < a.min.y)
            };
            if out.recesses.iter().any(|r| r.panel == p && near(&r.region))
                || out.countersinks.iter().any(|c| c.panel == p && near(&Region2 { outer: c.outer_loop(), holes: Vec::new() }))
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
                if out.bores.iter().any(|b| b.panel == p) {
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
/// (#6748) where the sheet can't keep it:
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
pub(super) fn surface_hole_cut(f: &Feature, st: &mut ModelState, tools: &[super::HoleTool]) -> Result<Vec<String>> {
    if tools.is_empty() || tools.len() > 10_000 {
        return Ok(Vec::new());
    }
    let mut drills = Vec::with_capacity(tools.len());
    for tool in tools {
        // A drill meets a panel by what the tool holds whole, and reaches all its cylinder may.
        let Some(PrimitivePrism::Cylinder { base, axis, radius, height, reach, part }) = tool.prism else { return Ok(Vec::new()) };
        let role = tool.point_role.as_ref().map(|point| format!("{point}:{part}"));
        drills.push((base, axis, radius, height, drill_box(base, axis, radius, reach), role));
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
        for &(base, axis, radius, height, reach, ref role) in reaching {
            let mut found: Option<(usize, bool, Vec3, Vec3, (f64, f64), (f64, f64))> = None;
            for (p, frame, own) in &panels {
                let inv = frame.inverse();
                let (b, a) = (inv.point(base), inv.vector(axis));
                let Some(a) = a.normalized() else { return Ok(Vec::new()) };
                let across = a.z.abs() >= 1.0 - 1e-9;
                let mut span = (height.min(0.0), height.max(0.0));
                // How far a bore along the panel runs on past each end of `span`: a hair where the
                // panel's material ends there, so its end is no face of the panel's rim; none where
                // the drill ends inside the material (a blind hole's floor), which is as deep as drilled.
                let mut past = (0.0, 0.0);
                let meets = if across {
                    let (z0, z1) = (b.z.min(b.z + a.z * height), b.z.max(b.z + a.z * height));
                    let through = z0 <= tol && z1 >= t - tol && {
                        let circle = Region2 { outer: Loop2::circle(Vec2::new(b.x, b.y), radius), holes: Vec::new() };
                        crate::sheet_surface::holds_clear(own, &circle)
                    };
                    if own.contains(Vec2::new(b.x, b.y)) && !through {
                        return Ok(Vec::new());
                    }
                    through
                } else if a.z.abs() <= 1e-9 {
                    // Along the panel: its line through the panel's material, inside its thickness.
                    // A drill clear of the panel's slab meets nothing of it: one square across
                    // the base, beside a flange standing on it, runs along the flange's plane.
                    let (b2, a2) = (Vec2::new(b.x, b.y), Vec2::new(a.x, a.y));
                    let clear = b.z - radius > t + tol || b.z + radius < -tol;
                    let material = crate::sheet_surface_convert::extent_across(own, (b2, a2));
                    let inside = material.map(|(s0, s1)| (s0.max(span.0), s1.min(span.1)));
                    let through = !clear && inside.is_some_and(|(u0, u1)| u0 < u1);
                    if let (Some(inside), Some((s0, s1))) = (inside.filter(|_| through), material) {
                        let hair = t * 1e-2;
                        past = (if inside.0 <= s0 + tol { hair } else { 0.0 }, if inside.1 >= s1 - tol { hair } else { 0.0 });
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
                    found = Some((*p, across, b, a, span, past));
                }
            }
            let Some((p, across, b, a, (u0, u1), (p0, p1))) = found else { return Ok(Vec::new()) };
            if boxes.iter().any(|(k, bx)| *k != Some(p) && !boxes_apart(&reach, &Some(*bx))) {
                return Ok(Vec::new());
            }
            // What the hole takes from its panel, in the panel's chart: its circle across it, or
            // the band its bore runs along inside it (as far as the bore runs, a hair past an end
            // where the material ends). It goes the general way where that meets a pocket on the
            // panel or a cut through the sheet, read exactly; clear of them, it is cut here.
            let footprint = if across {
                Loop2::circle(Vec2::new(b.x, b.y), radius)
            } else {
                let (o, d) = (Vec2::new(b.x, b.y), Vec2::new(a.x, a.y));
                let Some(d) = d.normalized() else { return Ok(Vec::new()) };
                let side = Vec2::new(-d.y, d.x) * radius;
                let (s, e) = (o + d * (u0 - p0), o + d * (u1 + p1));
                Loop2::polygon(&[s - side, e - side, e + side, s + side])
            };
            if out.recesses.iter().any(|r| r.panel == p && !crate::sheet_surface::loop_clear_of(&footprint, &r.region))
                || out
                    .countersinks
                    .iter()
                    .any(|c| c.panel == p && !crate::sheet_surface::loop_clear_of(&footprint, &Region2 { outer: c.outer_loop(), holes: Vec::new() }))
            {
                return Ok(Vec::new());
            }
            if !out.sheet.cuts.is_empty() {
                // The cuts are drawn in the chart of a panel each reaches, and may reach this one
                // across a joint: both are read in the flat, where the development places them.
                let Ok(dev) = sk.develop() else { return Ok(Vec::new()) };
                let Some(Some(here)) = dev.placements.get(p) else { return Ok(Vec::new()) };
                let laid = here.region(&Region2 { outer: footprint, holes: Vec::new() }).outer;
                for c in &out.sheet.cuts {
                    let Some(Some(there)) = dev.placements.get(c.panel) else { return Ok(Vec::new()) };
                    if !crate::sheet_surface::loop_clear_of(&laid, &there.region(&c.region)) {
                        return Ok(Vec::new());
                    }
                }
            }
            let Some(panel) = out.sheet.panels.get_mut(p) else { return Ok(Vec::new()) };
            if across {
                let circle = Region2 { outer: Loop2::circle(Vec2::new(b.x, b.y), radius), holes: Vec::new() };
                if !crate::sheet_surface::holds_clear(&panel.region, &circle) {
                    return Ok(Vec::new());
                }
                if let Some(role) = role {
                    out.holes.push(crate::sheet_surface_convert::HoleRecord {
                        panel: panel.name.clone(),
                        feature: f.id,
                        segs: circle.outer.segs.iter().map(|seg| (*seg, String::new(), Some(*seg))).collect(),
                        drill_role: Some(format!("{role}:side:0")),
                    });
                }
                panel.region.holes.push(circle.outer.ccw().reversed());
            } else {
                // Through the panel's material only, a hair past it where it runs out of it.
                let source = role.as_ref().map(|role| crate::sheet::BoreSource { feature: f.id, role: role.clone() });
                out.bores.push(crate::sheet::SheetBore { panel: p, base: b + a * (u0 - p0), axis: a, radius, height: u1 - u0 + p0 + p1, source });
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
    /// A cylinder: base centre, unit axis, radius and signed height, which the tool holds whole.
    /// The tool lies within the cylinder of the same base `reach` long: longer than `height` where
    /// material above the hole's entry face was left out of the tool (see `hole_run_on`).
    Cylinder { base: Vec3, axis: Vec3, radius: f64, height: f64, reach: f64, part: &'static str },
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
/// An extrude cut whose profile passes a hair off a corner of a sheet made from surfaces it cuts
/// (#15346), nearer it than the body's working tolerance but not through it, is refused. The
/// kernel's booleans and sew take two points that near for one (the record's tolerance, 10⁻⁶ of
/// the solid's size and at least 10⁻⁵ mm, as `sew` in crates/kernel/src/sew.rs sets it), so the
/// cut would build another part than the one drawn, or none; moving the cut onto the corner would
/// change what was drawn. Read exactly in the sketch plane: each corner the extrude runs past, its
/// distance from the profile's lines and arcs (other curves are not read here). "Through it" is
/// within the model's linear resolution, `LINEAR_TOL` (10⁻⁶ mm).
pub(super) fn surface_corner_graze_refused(f: &Feature, st: &ModelState, targets: &[String], c: &ExtrudeConstructor) -> Result<()> {
    let FeatureKind::Extrude { sketch, profiles, .. } = &f.kind else { return Ok(()) };
    let Some(ss) = st.sketch(sketch.0) else { return Ok(()) };
    if c.plane != ss.plane {
        return Ok(());
    }
    let regions = select_profiles(ss, profiles)?;
    let plane = ss.plane;
    let n = plane.normal();
    let (lo, hi) = (c.interval.0.min(c.interval.1), c.interval.0.max(c.interval.1));
    for sk in &st.surface_sheets {
        let Some(mb) = st.body(&sk.body) else { continue };
        if !takes_part(f, targets, mb) {
            continue;
        }
        let reach = (mb.body.size() * 1e-6).max(1e-5);
        for v in &mb.body.record().vertices {
            let p = Vec3::new(v.point[0], v.point[1], v.point[2]);
            let h = (p - plane.origin).dot(n);
            if h < lo - reach || h > hi + reach {
                continue;
            }
            let q = plane.to_local(p);
            let near = regions
                .iter()
                .flat_map(|r| std::iter::once(&r.outer).chain(&r.holes))
                .flat_map(|l| &l.segs)
                .filter_map(|s| match *s {
                    Seg2::Line { a, b } => Some(crate::sheet_surface_cuts::to_segment(q, a, b)),
                    Seg2::Arc { center, radius, start, sweep } => {
                        Some(crate::sheet_surface_cuts::to_arc(q, (center, radius, start, sweep), [s.start(), s.end()]))
                    }
                    _ => None,
                })
                .fold(f64::INFINITY, f64::min);
            // On the corner: within the model's linear resolution (a folded panel's corner lies
            // where its fold puts it, to a rounding far below that).
            if near > solidcraft_geom::LINEAR_TOL && near < reach {
                return Err(DocError::Invalid(format!(
                    "the cut passes {near:.1e} mm from a corner of `{}` at ({:.4}, {:.4}, {:.4}): nearer than the model's tolerance ({reach:.1e} mm) \
                     but not through it, so it can't be built as drawn. Draw the cut through the corner, or at least {reach:.1e} mm from it",
                    sk.body, p.x, p.y, p.z
                )));
            }
        }
    }
    Ok(())
}

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
    #[cfg(test)]
    cost_tests::count(|c| c.tight_boxes += 1);
    let b = body.exact_bounds().ok()?;
    (!b.is_empty() && b.min.is_finite() && b.max.is_finite()).then(|| b.inflated(2.0 * body.record().tolerance + 1e-6))
}

/// Whether every tool of `reach` lies inside `mb`: its box is inside the box of the
/// body's mesh, which holds the body's points (a mesh's vertices are on the surface) and
/// is nearly free.
///
/// A tool inside the body is not apart from it, so the kernel's tight box of the body
/// cannot show the tool apart of it and need not be measured: measuring it costs the
/// kernel minutes on a body with many faces (#18821: 347 s for the cup of the report,
/// against 30 s for the cut the box was measured for). The cut is made all the same,
/// exactly as the tight box would have left it.
pub(super) fn tools_inside(mb: &ModelBody, reach: &[Option<Aabb3>]) -> bool {
    let b = mb.mesh_bounds();
    if b.is_empty() || !b.min.is_finite() || !b.max.is_finite() {
        return false;
    }
    let b = b.inflated(2.0 * mb.body.record().tolerance + 1e-6);
    reach.iter().flatten().all(|tool| b.contains(tool.min) && b.contains(tool.max))
}

/// Whether some tool of `reach` meets the box of `mb`'s vertices, grown as [`exact_box`] grows
/// the exact box. Every vertex lies in the body's exact box, so such a tool meets the exact box
/// too and the exact box cannot show it apart of the body: it need not be measured. A tool cut
/// through a wall of a rounded spline body reaches out of its mesh box ([`tools_inside`]), and
/// measuring that body's exact box took the kernel over a minute for each of its holes.
pub(super) fn tools_meet_vertices(mb: &ModelBody, reach: &[Option<Aabb3>]) -> bool {
    let b: Aabb3 = mb.body.record().vertices.iter().map(|v| Vec3::new(v.point[0], v.point[1], v.point[2])).collect();
    if b.is_empty() || !b.min.is_finite() || !b.max.is_finite() {
        return false;
    }
    let b = b.inflated(2.0 * mb.body.record().tolerance + 1e-6);
    reach.iter().flatten().any(|tool| b.meets(tool))
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

/// Whether [`kernel::cylinder_face_at`] is known to find no face of `mb` at `p`. It takes a face
/// only when its mesh of the body, made to a chord tolerance of `(size * 1e-3).max(1e-3)`, comes
/// within a twentieth of the body's size of the point. That mesh lies within its tolerance of
/// the exact faces, so of the body's exact box ([`exact_box`]): a box farther from the point
/// than a twentieth of the size and twice that tolerance holds no face for it. Working that out
/// from the exact box spares meshing every body in the design for each tap marking (#10227). A
/// mesh body, and a body whose box is unknown, are searched as before.
pub(super) fn beyond_cylinder_reach(mb: &ModelBody, p: Vec3) -> bool {
    if mb.body.is_mesh() {
        return false;
    }
    let Some(b) = exact_box(&mb.body) else { return false };
    let size = mb.body.size();
    let reach = size * 0.05 + 2.0 * (size * 1e-3).max(1e-3);
    let off = |at: f64, lo: f64, hi: f64| (lo - at).max(at - hi).max(0.0);
    let (dx, dy, dz) = (off(p.x, b.min.x, b.max.x), off(p.y, b.min.y, b.max.y), off(p.z, b.min.z, b.max.z));
    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
    dist.is_finite() && reach.is_finite() && dist > reach
}

/// Whether two boxes are known to be apart (an unknown box is never apart).
pub(super) fn boxes_apart(a: &Option<Aabb3>, b: &Option<Aabb3>) -> bool {
    let (Some(a), Some(b)) = (a, b) else { return false };
    a.max.x < b.min.x || b.max.x < a.min.x || a.max.y < b.min.y || b.max.y < a.min.y || a.max.z < b.min.z || b.max.z < a.min.z
}

/// Whether `tools` may cover `body`: the body's exact box lies inside the box around the tools'
/// ([`exact_box`]), grown by twice the body's own tolerance, as it does for a body inside its
/// tools. A cut that leaves nothing of a body they cannot cover is a failed boolean, not an empty
/// result (#15700). Unknown is covered, and so is a mesh body or a body cut by a mesh, as such a
/// cut always was.
pub(super) fn tools_may_cover(body: &Body, tools: &[Traced]) -> bool {
    if body.is_mesh() || tools.iter().any(|t| t.body.is_mesh()) {
        return true;
    }
    let mut hull = Aabb3::EMPTY;
    for t in tools {
        let Some(b) = exact_box(&t.body) else { return true };
        hull = hull.union(&b);
    }
    let hull = hull.inflated(2.0 * body.record().tolerance);
    match body.exact_bounds() {
        Ok(b) if !b.is_empty() && b.min.is_finite() && b.max.is_finite() => hull.contains(b.min) && hull.contains(b.max),
        _ => true,
    }
}

/// The refusal of a cut that left nothing of `body` although its tools cannot cover it
/// ([`tools_may_cover`]): the body is kept instead of vanishing without a word (#15700).
pub(super) fn failed_cut(body: &str) -> DocError {
    DocError::Invalid(format!("the cut failed: it would remove all of `{body}`, which the tools do not cover"))
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
    if b.body.nearest_face(p).is_none_or(|(distance, _)| distance > tolerance) {
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
                    *slot = mb.remade(nb);
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
/// (the sheet as it was) that its bend lies along, by what made that rim's edge (`F15/F12/side.l2`: a
/// flange on the line `l2` of the panel the base flange `F12` made; `F17/F15/F12/side.l2/far`: a
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A flipped base's sketch region mirrors in the sketch's x axis exactly: its arc stays an arc
    /// and bulges the mirrored way, and the outline keeps its sense.
    #[test]
    fn a_flipped_region_mirrors_its_arcs_exactly() {
        let c = Vec2::new(40.0, 30.0);
        let r = Vec2::new(10.0, 0.0);
        let half =
            Loop2 { segs: vec![Seg2::Arc { center: c, radius: 10.0, start: 0.0, sweep: std::f64::consts::PI }, Seg2::Line { a: c - r, b: c + r }] };
        let mirrored = mirror_y(&Region2 { outer: half, holes: vec![] }).outer;
        let arc = mirrored.segs.iter().find(|s| matches!(s, Seg2::Arc { .. })).unwrap();
        assert!(
            mirrored.is_ccw()
                && matches!(arc, Seg2::Arc { center, radius, .. } if (*center - Vec2::new(40.0, -30.0)).len() < 1e-12 && *radius == 10.0)
        );
        assert!((arc.mid() - Vec2::new(40.0, -40.0)).len() < 1e-9, "{:?}", arc.mid());
    }

    /// An Emboss's placement (#20386) turns the picked regions about the middle of their bounds
    /// and then shifts them along the sketch's own axes. A hole goes with its outline, an arc
    /// stays an arc, a shift alone is exact, and with nothing asked the regions come back as
    /// they are.
    #[test]
    fn an_emboss_placement_turns_about_the_middle_of_the_bounds_then_shifts() {
        let square =
            |x: f64, y: f64| Loop2::polygon(&[Vec2::new(x, y), Vec2::new(x + 10.0, y), Vec2::new(x + 10.0, y + 10.0), Vec2::new(x, y + 10.0)]);
        // Two squares over x 0..30 and y 0..10, the first with a round hole: the middle is (15, 5).
        let regions = vec![
            Region2 { outer: square(0.0, 0.0), holes: vec![Loop2::circle(Vec2::new(5.0, 5.0), 2.0)] },
            Region2 { outer: square(20.0, 0.0), holes: vec![] },
        ];
        assert_eq!(emboss_placement(regions.clone(), Vec2::ZERO, 0.0).unwrap(), regions);
        // A quarter turn about (15, 5) takes (x, y) to (20 - y, x - 10); then 100 along x.
        let placed = emboss_placement(regions.clone(), Vec2::new(100.0, 0.0), std::f64::consts::FRAC_PI_2).unwrap();
        let want = |p: Vec2| Vec2::new(120.0 - p.y, p.x - 10.0);
        assert_eq!(placed.len(), 2);
        for (was, now) in regions.iter().zip(&placed) {
            assert_eq!(was.holes.len(), now.holes.len());
            for (a, b) in was.loops().zip(now.loops()) {
                assert_eq!(a.segs.len(), b.segs.len());
                for (s, t) in a.segs.iter().zip(&b.segs) {
                    assert_eq!(std::mem::discriminant(s), std::mem::discriminant(t), "{s:?} became {t:?}");
                    for (p, q) in [(s.start(), t.start()), (s.mid(), t.mid()), (s.end(), t.end())] {
                        assert!((want(p) - q).len() < 1e-12, "{p:?} went to {q:?}");
                    }
                }
            }
        }
        // A whole turn more is the same placement.
        let again = emboss_placement(regions.clone(), Vec2::new(100.0, 0.0), 2.5 * std::f64::consts::PI).unwrap();
        assert!((again[1].outer.segs[0].start() - placed[1].outer.segs[0].start()).len() < 1e-9);
        let shifted = emboss_placement(regions.clone(), Vec2::new(3.0, -4.0), 0.0).unwrap();
        assert_eq!(shifted[1].outer.segs[0].start(), Vec2::new(23.0, -4.0));
        assert!(
            matches!(shifted[0].holes[0].segs[0], Seg2::Arc { center, radius, start, .. } if center == Vec2::new(8.0, 1.0) && radius == 2.0 && start == 0.0)
        );
        assert!(emboss_placement(regions.clone(), Vec2::new(f64::NAN, 0.0), 0.0).is_err());
        assert!(emboss_placement(regions, Vec2::ZERO, f64::INFINITY).is_err());
    }
}
