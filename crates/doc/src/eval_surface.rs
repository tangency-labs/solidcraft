//! Evaluation of the surface features: Patch, Stitch, Thicken, Trim, Extend, Surface Extrude,
//! Revolve, Sweep and Loft, Offset, Ruled, Untrim, Unstitch and Reverse Normal (kernel `surfaces`,
//! `surface_offset`, `surface_ruled`, `surface_untrim`).

use super::*;
use crate::{RuledCorners, RuledMode, SheetSection};
use solidcore_record::Seg2;

/// Qualify the portable result, which can differ from the native body checked
/// before export. A bad edge lift must leave the source and timeline intact.
fn qualify_refined_thicken(body: &Body) -> Result<()> {
    if body.is_surface() || body.is_mesh() {
        return Err(DocError::Invalid("Thicken did not produce a closed solid from the refined Form".into()));
    }
    let validity = kernel::validity(body);
    if let Err(error) = validity.record {
        return Err(DocError::Invalid(format!("Thicken produced an invalid solid from the refined Form ({error})")));
    }
    match validity.smlib {
        Some(kernel::SmlibCheck::Valid) => Ok(()),
        Some(kernel::SmlibCheck::Invalid(error)) => {
            Err(DocError::Invalid(format!("Thicken produced an invalid solid from the refined Form ({error})")))
        }
        Some(kernel::SmlibCheck::NotSupported(error)) => {
            Err(DocError::Invalid(format!("Thicken could not validate the refined Form solid ({error})")))
        }
        None => Err(DocError::Invalid("Thicken could not validate the refined Form solid".into())),
    }
}

/// Choose the half-wall construction only when the direct symmetric output
/// fails the portable and native checks used during timeline evaluation.
pub fn refined_symmetric_uses_half_walls(body: &Body, thickness: f64) -> Result<bool> {
    if !body.is_surface() {
        return Err(DocError::Invalid("Thicken requires an open refined Form surface".into()));
    }
    let direct = kernel::thicken(body, thickness, true)?;
    Ok(qualify_refined_thicken(&direct).is_err())
}

fn trace_half_walls(source: &Traced, thickness: f64) -> Result<Traced> {
    let (wall, side_history) = kernel::thicken_opposite_sides(&source.body, thickness)?;
    qualify_refined_thicken(&wall)?;
    let lineage = source.lineage.as_ref().and_then(|input| crate::lineage::Lineage::from_history(side_history, &[input, input]));
    Ok(Traced { body: wall, lineage, recipe: None })
}

/// Sew two saved Form producers while retaining both editable sources.
pub(super) fn eval_refined_weld(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    feature: &Feature,
    state: &mut ModelState,
    history: &[FeatureResult],
) -> Result<()> {
    let invalid = |why: &str| DocError::Invalid(why.into());
    let FeatureKind::Stitch { bodies, tolerance, form_seam: Some(seam) } = &feature.kind else {
        return Err(invalid("expected a qualified refined Weld"));
    };
    let [first_name, second_name] = bodies.as_slice() else {
        return Err(invalid("refined Weld needs exactly two source bodies"));
    };
    if first_name == second_name || val(vals, tolerance, Kind::Length)? != 0.0 {
        return Err(invalid("refined Weld requires distinct source bodies and its fixed sewing tolerance"));
    }
    let prior = &*state;
    let source = |id: crate::FeatureId, name: &crate::BodyName| {
        let producer = doc
            .feature(id.0)
            .filter(|source| source.component == feature.component)
            .ok_or_else(|| invalid("refined Weld sources must be in the same component as the seam"))?;
        let FeatureKind::Form { tolerance, operation: Operation::NewBody, .. } = &producer.kind else {
            return Err(invalid("refined Weld requires unchanged standalone Form producers"));
        };
        let result = history
            .iter()
            .find(|r| r.id == id.0 && !r.skipped && !r.stale && r.error.is_none())
            .ok_or_else(|| invalid("refined Weld source is unavailable before the seam"))?;
        let mut bodies = result.state.bodies.iter().filter(|body| body.feature == id.0);
        let original = bodies.next().filter(|_| bodies.next().is_none()).ok_or_else(|| invalid("refined Weld source produced an ambiguous body"))?;
        let current = prior.bodies.iter().find(|body| body.name == name.0).ok_or_else(|| invalid("refined Weld source body is missing"))?;
        if current.feature != id.0
            || current.component != feature.component
            || original.component != feature.component
            || current.source_name.is_some()
            || current.transferred
            || current.source_transform != original.source_transform
            || !current.body.same(&original.body)
        {
            return Err(invalid("refined Weld source body changed between its Form feature and the seam"));
        }
        Ok((current, val(vals, tolerance, Kind::Length)?))
    };
    let [first, second] = seam.sources();
    let (a, a_tolerance) = source(first, first_name)?;
    let (b, b_tolerance) = source(second, second_name)?;
    let prepared = crate::forms::prepare_refined_weld(doc, seam, [a_tolerance, b_tolerance])?;
    if !prepared.matches_operands([&a.body, &b.body]) {
        return Err(invalid("refined Weld qualified records do not match the evaluated Form bodies"));
    }
    let name = a.name.clone();
    let traced = Traced::step(prepared.body, &[&a.traced(), &b.traced()]);
    if traced.lineage.is_none() {
        return Err(invalid("refined Weld could not retain both source body lineages"));
    }
    let result = a.remade(traced);
    let slot = state.bodies.iter_mut().find(|body| body.name == name).ok_or_else(|| invalid("missing first refined Weld body"))?;
    *slot = result;
    state.bodies.retain(|body| body.name != second_name.0);
    Ok(())
}

pub(super) fn eval(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState, warning: &mut Option<String>) -> Result<()> {
    match &f.kind {
        FeatureKind::Patch { sketch, profiles, profiles_3d, body, edges, crown, continuity, points } => {
            let mut made = Vec::new();
            if let Some(sk) = sketch {
                let ss = st.sketch(sk.0).ok_or_else(|| DocError::Unknown(format!("sketch {sk} (it must come earlier in the timeline)")))?;
                // An empty index list means the caller asked only for 3D loops (#741).
                if !matches!(profiles, ProfileSel::Indices { indices } if indices.is_empty()) {
                    let regions = solidcraft_sketch::merge_regions(&select_profiles(ss, profiles)?);
                    for r in &regions {
                        made.push(kernel::patch_region(&ss.plane, r)?);
                    }
                }
                for &i in profiles_3d {
                    let p = ss.profiles_3d.get(i).ok_or_else(|| DocError::Invalid(format!("sketch {sk} has no 3D profile {i}")))?;
                    made.push(kernel::patch_region(&p.plane, &p.region)?);
                }
            }
            if !edges.is_empty() {
                let i = body_at(st, body, &crate::plain(edges))?;
                let b = st.bodies.get(i).ok_or_else(|| DocError::Invalid("body".into()))?;
                // A design may carry an order the fill does not build yet: refused, not built lower.
                for g in std::iter::once(continuity.all).chain(continuity.edges.iter().map(|e| e.g)) {
                    g.built_by(kernel::Construction::Patch)?;
                }
                let spec = kernel::PatchSpec {
                    continuity: continuity.all,
                    edges: continuity.edges.iter().map(|e| (e.edge.0, e.g)).collect(),
                    points: crate::plain(points),
                    crown: *crown,
                };
                made.push(kernel::patch_edges_with(&b.body, &crate::plain(edges), &spec)?);
            }
            if made.is_empty() {
                return Err(DocError::Invalid("a patch needs a sketch region, a 3D sketch loop, or a loop of edges".into()));
            }
            for (k, b) in made.into_iter().enumerate() {
                let name = new_name(st, f, k);
                st.bodies.push(ModelBody::new(name, b, f.id));
            }
            Ok(())
        }
        FeatureKind::Stitch { bodies, tolerance, form_seam: None } => {
            let tol = val(vals, tolerance, Kind::Length)?;
            let idx: Vec<usize> = bodies
                .iter()
                .map(|n| st.bodies.iter().position(|b| &b.name == n).ok_or_else(|| DocError::Unknown(format!("body `{n}`"))))
                .collect::<Result<_>>()?;
            let Some(&first) = idx.first() else { return Err(DocError::Invalid("pick the surfaces to stitch".into())) };
            let refs: Vec<&kernel::Body> = idx.iter().filter_map(|i| st.bodies.get(*i).map(|b| &b.body)).collect();
            let sewn = kernel::stitch(&refs, tol)?;
            let Some(mb) = st.bodies.get(first).cloned() else { return Err(DocError::Invalid("body".into())) };
            if let Some(slot) = st.bodies.get_mut(first) {
                *slot = ModelBody::new(mb.name, sewn, mb.feature);
            }
            // The others are now part of the first.
            let gone: Vec<String> = idx.iter().skip(1).filter_map(|i| st.bodies.get(*i).map(|b| b.name.clone())).collect();
            st.bodies.retain(|b| !gone.contains(&b.name));
            Ok(())
        }
        FeatureKind::Thicken { bodies, thickness, symmetric, form_half_walls, form_planar_walls, operation, targets } => {
            let t = val(vals, thickness, Kind::Length)?;
            if form_half_walls.as_ref().is_some_and(|saved| saved.len() != bodies.len() || !*symmetric)
                || form_planar_walls.as_ref().is_some_and(|saved| saved.len() != bodies.len())
            {
                return Err(DocError::Invalid("Thicken has invalid saved source constructions".into()));
            }
            let saved_for = |choices: &Option<Vec<bool>>, index| choices.as_ref().and_then(|saved| saved.get(index)).copied().unwrap_or(false);
            let mut tools = Vec::new();
            for (index, n) in bodies.iter().enumerate() {
                let b = st.body(n).cloned().ok_or_else(|| DocError::Unknown(format!("body `{n}`")))?;
                let form = doc.feature(b.feature).and_then(|feature| match &feature.kind {
                    FeatureKind::Form { definition, .. } => Some(definition),
                    _ => None,
                });
                let refined_form = form.is_some_and(|definition| definition.hierarchy.is_some());
                if refined_form && !b.body.is_surface() {
                    return Err(DocError::Invalid("Thicken requires an open refined Form surface".into()));
                }
                let source = b.traced();
                let planar = saved_for(form_planar_walls, index);
                let halves = saved_for(form_half_walls, index);
                let traced = if planar {
                    if halves || form.is_none() {
                        return Err(DocError::Invalid("Thicken has invalid saved source constructions".into()));
                    }
                    let (body, history) = crate::forms::build_planar_wall(&source.body, t, *symmetric)?;
                    let lineage = source.lineage.as_ref().and_then(|input| crate::lineage::Lineage::from_history(history, &[input, input]));
                    Traced { body, lineage, recipe: None }
                } else if halves {
                    trace_half_walls(&source, t)?
                } else {
                    let thickened = kernel::thicken(&source.body, t, *symmetric)?;
                    if refined_form && let Err(original) = qualify_refined_thicken(&thickened) {
                        if !*symmetric || form_half_walls.is_some() {
                            return Err(original);
                        }
                        trace_half_walls(&source, t)
                            .map_err(|fallback| DocError::Invalid(format!("{original}; could not join the two half walls ({fallback})")))?
                    } else {
                        Traced::step(thickened, &[&source])
                    }
                };
                if (planar || refined_form) && traced.lineage.is_none() {
                    return Err(DocError::Invalid("Thicken could not retain exact history from the Form".into()));
                }
                tools.push(traced);
            }
            if tools.is_empty() {
                return Err(DocError::Invalid("pick the surfaces to thicken".into()));
            }
            apply_traced(doc, st, f, tools, *operation, &crate::plain(targets))
        }
        FeatureKind::SurfaceTrim { body, plane, tool, keep, keep_more, curves, with, keep_with, stitch, extend } => {
            let i = st.bodies.iter().position(|b| &b.name == body).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let picks: Vec<Vec3> = std::iter::once(keep.0).chain(keep_more.iter().map(|p| p.0)).collect();
            let keeps = if keep_more.is_empty() { kernel::TrimKeep::Side(keep.0) } else { kernel::TrimKeep::Regions(&picks) };
            if let Some(other) = with {
                return mutual_trim(st, i, other, keeps, &crate::plain(keep_with), *stitch, *extend);
            }
            let trimmed = match (tool, curves) {
                (Some(t), _) => {
                    let tb = st.body(&t.body).ok_or_else(|| DocError::Unknown(format!("body `{}`", t.body)))?;
                    let face = crate::appearance::face_index_at(tb, t.point.0)
                        .ok_or_else(|| DocError::Invalid(format!("no face of `{}` at the tool point", t.body)))?;
                    kernel::trim_keep(&mb.body, &kernel::SplitTool::Face { body: &tb.body, face }, keeps)?
                }
                (None, Some(c)) => {
                    let ss = st
                        .sketch(c.sketch.0)
                        .ok_or_else(|| DocError::Unknown(format!("sketch {} (it must come earlier in the timeline)", c.sketch)))?;
                    let ss = sketch_in_feature_frame(doc, ss, f.component)?;
                    let ids: Vec<String> = if c.curves.is_empty() {
                        ss.sketch.curves.iter().filter(|k| !k.construction).map(|k| k.id.clone()).collect()
                    } else {
                        c.curves.clone()
                    };
                    let chains = curve_chains(&ss, &ids)?;
                    if chains.is_empty() {
                        return Err(DocError::Invalid(format!("sketch `{}` has no curves to trim with", ss.name)));
                    }
                    kernel::trim_keep(
                        &mb.body,
                        &kernel::SplitTool::Curves { plane: ss.plane, curves: chains, along: c.direction.map(|d| d.0) },
                        keeps,
                    )?
                }
                (None, None) => {
                    kernel::trim_keep(&mb.body, &kernel::SplitTool::Plane(doc.resolve_plane_in(vals, plane, 0, f.component, Some(st))?), keeps)?
                }
            };
            let made = Traced::step(trimmed, &[&mb.traced()]);
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = mb.remade(made);
            }
            Ok(())
        }
        FeatureKind::SurfaceExtend { body, edges, distance, mode, up_to, neighbours } => {
            let i = st.bodies.iter().position(|b| &b.name == body).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let mode = match mode {
                crate::ExtendMode::Natural => kernel::ExtendMode::Natural,
                crate::ExtendMode::Tangent => kernel::ExtendMode::Tangent,
                crate::ExtendMode::Curvature => kernel::ExtendMode::Curvature,
            };
            let at = |limit| kernel::ExtendOptions { mode, limit, neighbours: *neighbours };
            let out = match up_to {
                None => {
                    kernel::extend_surface(&mb.body, &crate::plain(edges), &at(kernel::ExtendLimit::Distance(val(vals, distance, Kind::Length)?)))?
                }
                Some(crate::ExtendTo::Bounds) => kernel::extend_surface(&mb.body, &crate::plain(edges), &at(kernel::ExtendLimit::Bounds))?,
                Some(crate::ExtendTo::Plane(plane)) => {
                    let tool = kernel::SplitTool::Plane(doc.resolve_plane_in(vals, plane, 0, f.component, Some(st))?);
                    kernel::extend_surface(&mb.body, &crate::plain(edges), &at(kernel::ExtendLimit::UpTo(&tool)))?
                }
                Some(crate::ExtendTo::Face(t)) => {
                    let tb = st.body(&t.body).ok_or_else(|| DocError::Unknown(format!("body `{}`", t.body)))?;
                    let face = crate::appearance::face_index_at(tb, t.point.0)
                        .ok_or_else(|| DocError::Invalid(format!("no face of `{}` at the target point", t.body)))?;
                    let tool = kernel::SplitTool::Face { body: &tb.body, face };
                    kernel::extend_surface(&mb.body, &crate::plain(edges), &at(kernel::ExtendLimit::UpTo(&tool)))?
                }
            };
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::new(mb.name, out, mb.feature);
            }
            Ok(())
        }
        FeatureKind::SurfaceBlend { from, to, continuity, bulge, align, seam, stitch } => {
            let a = st.body(&from.body).ok_or_else(|| DocError::Unknown(format!("body `{}`", from.body)))?.body.clone();
            let b = st.body(&to.body).ok_or_else(|| DocError::Unknown(format!("body `{}`", to.body)))?.body.clone();
            if from.edges.is_empty() || to.edges.is_empty() {
                return Err(DocError::Invalid("a blend needs a chain of edges at each end".into()));
            }
            let (made, gap) = kernel::blend_between(
                &a,
                &crate::plain(&from.edges),
                &b,
                &crate::plain(&to.edges),
                *continuity,
                *bulge,
                align.map(|[p, q]| [p.0, q.0]),
                seam.map(|[p, q]| [p.0, q.0]),
                BLEND_STATIONS,
                *stitch,
            )?;
            if gap > 1e-9 {
                *warning = Some(format!("the blend's boundary lies up to {gap:.4} mm from the edges it was built on"));
            }
            let name = new_name(st, f, 0);
            // A stitched blend takes the place of both bodies it joins; otherwise it is a new
            // surface body of its own.
            if *stitch {
                let gone = [from.body.0.clone(), to.body.0.clone()];
                st.bodies.retain(|x| !gone.contains(&x.name));
            }
            st.bodies.push(ModelBody::new(name, made, f.id));
            Ok(())
        }
        FeatureKind::SurfaceBoundary { sketch, curves, orders, crown, samples } => {
            let ss = sketch_in_feature_frame(
                doc,
                st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?,
                f.component,
            )?;
            // Two curves are the two sides of a bridge. Three or four run round a loop: the Coons
            // patch of the loop, which lies in the sketch's plane with them, so every order holds.
            let body = if curves.len() >= 3 {
                if *crown != 0.0 {
                    return Err(DocError::Invalid(
                        "not supported yet: crown on a patch through three or four curves (it is the flat patch of its sides)".into(),
                    ));
                }
                loop_patch(&ss, curves)?
            } else {
                let boundaries = boundary_frames(&ss, curves, orders, *samples)?;
                kernel::boundary_patch(&boundaries, *crown, *samples)?
            };
            let name = new_name(st, f, 0);
            st.bodies.push(ModelBody::new(name, body, f.id));
            Ok(())
        }
        FeatureKind::SurfaceNetwork { u, v, operation } => {
            if !matches!(operation, crate::Operation::NewBody | crate::Operation::Join) {
                return Err(DocError::Invalid("a boundary surface is a new body or joins the body of its edges".into()));
            }
            let us = network_sources(doc, st, f, u, "U")?;
            let vs = network_sources(doc, st, f, v, "V")?;
            let made = kernel::boundary_surface(&network_curves(&us), &network_curves(&vs))?;
            if *operation == crate::Operation::Join {
                // Into the body of the first curve picked on a body's edges.
                let target = u
                    .iter()
                    .chain(v)
                    .find_map(|c| c.body.clone())
                    .ok_or_else(|| DocError::Invalid("join needs a curve on a body's edges: the surface is sewn into that body".into()))?;
                let i = st.bodies.iter().position(|b| b.name == target).ok_or_else(|| DocError::Unknown(format!("body `{target}`")))?;
                let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
                let joined = kernel::boundary_join(&mb.body, &made)?;
                if let Some(slot) = st.bodies.get_mut(i) {
                    *slot = ModelBody::new(mb.name, joined, mb.feature);
                }
                return Ok(());
            }
            let name = new_name(st, f, 0);
            st.bodies.push(ModelBody::new(name, made, f.id));
            Ok(())
        }
        FeatureKind::SurfaceMatch { body, faces, target, edges, order } => {
            let i = st.bodies.iter().position(|b| &b.name == body).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let tb = st.body(target).ok_or_else(|| DocError::Unknown(format!("body `{target}`")))?.body.clone();
            if edges.is_empty() {
                return Err(DocError::Invalid("a match needs a chain of the target's edges".into()));
            }
            // Picked by a point on the face; without picks, the body's first face.
            let picked: Vec<solidcore_record::Vec3> = if faces.is_empty() {
                let faces = mb.body.faces(mb.body.size() * 1e-2)?;
                vec![faces.first().ok_or_else(|| DocError::Invalid(format!("`{body}` has no face to match")))?.centroid]
            } else {
                crate::plain(faces)
            };
            let made = kernel::match_surface(&mb.body, &picked, &tb, &crate::plain(edges), *order)?;
            // A match rebuilds the face in place: the body keeps its name, and whatever meets it
            // downstream still finds it.
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::new(mb.name, made, mb.feature);
            }
            Ok(())
        }
        FeatureKind::SurfaceFair { body, faces, strength, iterations, keep_edges, tolerance } => {
            let tol = val(vals, tolerance, Kind::Length)?;
            let opts = kernel::FairOptions::new(*strength, *iterations as usize, *keep_edges, tol)?;
            let i = st.bodies.iter().position(|b| &b.name == body).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            // Picked by a point on the face; without picks, every face of the body.
            let mut picked: Vec<usize> = Vec::new();
            if faces.is_empty() {
                picked.extend(0..mb.body.face_count());
            } else {
                for p in faces {
                    let fi =
                        crate::appearance::face_index_at(&mb, p.0).ok_or_else(|| DocError::Invalid(format!("Fair: no face of {body} at {p:?}")))?;
                    if !picked.contains(&fi) {
                        picked.push(fi);
                    }
                }
            }
            if picked.is_empty() {
                return Err(DocError::Invalid("the body has no faces to fair".into()));
            }
            // The face keeps every edge it had: only the surface between them is faired. The
            // energy before and after goes into the feature's warning, so the reply can say how
            // much the face moved.
            let (faired, report) = kernel::fair_faces(&mb.body, &picked, &opts)?;
            *warning = Some(format!(
                "faired {} face(s): energy {:.6} → {:.6} in {} step(s), control points moved up to {:.4} mm",
                picked.len(),
                report.energy_before,
                report.energy_after,
                report.steps,
                report.movement
            ));
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::new(mb.name, faired, mb.feature);
            }
            Ok(())
        }
        FeatureKind::SurfaceExtrude { sketch, profiles, curves, extent } => {
            let ss = sketch_in_feature_frame(
                doc,
                st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?,
                f.component,
            )?;
            if extent.through_all || extent.taper.is_some() || extent.to.is_some() || extent.from.is_some() {
                return Err(DocError::Invalid("not supported yet: a surface extrude through all, to or from an object, or with a taper".into()));
            }
            let d = val(vals, &extent.distance, Kind::Length)?;
            let off = extent.start_offset.as_deref().map(|e| val(vals, e, Kind::Length)).transpose()?.unwrap_or(0.0);
            let (a, b) = extent_sides(vals, extent, d, extent.direction)?;
            let chains = sheet_chains(&ss, profiles.as_ref(), curves)?;
            let body = kernel::extrude_sheet(&ss.plane, &chains, a + off, b + off)?;
            push_sheet(st, f, crate::lineage::ChainRoles { sketch: &ss, chains: &chains }.tool(body));
            Ok(())
        }
        FeatureKind::SurfaceRevolve { sketch, profiles, curves, axis, angle, angle2, symmetric } => {
            let ss = sketch_in_feature_frame(
                doc,
                st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?,
                f.component,
            )?;
            let (o, d) = revolve_axis(
                Some(doc),
                f.component,
                vals,
                &ss,
                axis,
                &*super::axis_state(doc, st, f.component, axis)?,
                &|name| construction_axis_in(Some(doc), st, name, f.component),
                &|owner, curve| {
                    let solved =
                        st.sketch(owner).ok_or_else(|| DocError::Unknown(format!("sketch {owner} (it must come earlier in the timeline)")))?;
                    sketch_line_world(&sketch_in_feature_frame(doc, solved, f.component)?, curve)
                },
            )?;
            let a = val(vals, angle, Kind::Angle)?;
            let (start, sweep) = match (angle2, symmetric) {
                (Some(a2), _) => {
                    let a2 = val(vals, a2, Kind::Angle)?;
                    (-a2, a + a2)
                }
                (None, true) => (-a / 2.0, a),
                (None, false) => (0.0, a),
            };
            let chains = sheet_chains(&ss, profiles.as_ref(), curves)?;
            let body = kernel::revolve_sheet(&ss.plane, &chains, o, d, start, sweep)?;
            push_sheet(st, f, crate::lineage::ChainRoles { sketch: &ss, chains: &chains }.tool(body));
            Ok(())
        }
        FeatureKind::Unstitch { body, faces } => {
            let i = st.bodies.iter().position(|b| &b.name == body).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let (pieces, rest) = kernel::unstitch(&mb.body, &crate::plain(faces))?;
            let source = mb.traced();
            // The faces left keep the body's name; each face taken out is a new surface body.
            match rest {
                Some(rest) => {
                    if let Some(slot) = st.bodies.get_mut(i) {
                        *slot = mb.remade(source.then(rest));
                    }
                }
                None => {
                    st.bodies.remove(i);
                }
            }
            for (k, piece) in pieces.into_iter().enumerate() {
                let name = new_name(st, f, k);
                st.bodies.push(ModelBody::traced_new(name, source.then(piece), f.id));
            }
            Ok(())
        }
        FeatureKind::ReverseNormal { body, faces } => {
            let i = st.bodies.iter().position(|b| &b.name == body).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let turned = mb.traced().then(kernel::reverse_faces(&mb.body, &crate::plain(faces))?);
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = mb.remade(turned);
            }
            Ok(())
        }
        FeatureKind::SurfaceSweep {
            sketch,
            profiles,
            curves,
            path_sketch,
            path,
            path_body,
            path_edges,
            orientation,
            twist,
            scale,
            rails,
            rail_fit,
        } => {
            let ss = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?;
            // The path is a chain of a body's edges (#2216) or the curves of a sketch.
            if let Some(body) = path_body {
                let chains = sheet_chains(ss, profiles.as_ref(), curves)?;
                // The path runs on from the end nearest the middle of the curves.
                let pts: Vec<Vec2> = chains.iter().flatten().flat_map(|s| s.polyline(1e-2)).collect();
                let start = ss.plane.to_world(pts.iter().fold(Vec2::default(), |a, p| a + *p) * (1.0 / pts.len().max(1) as f64));
                let segs = crate::eval::edge_path_segments(st, body, path_edges, start, Some(&ss.plane))?;
                let path_start = segs.first().map(kernel::PathSeg::start).unwrap_or(start);
                let rails = crate::eval::rail_polylines(doc, st, rails, f.component, path_start)?;
                let ctl = crate::eval::sweep_control(vals, orientation, twist, scale, rails, *rail_fit)?;
                let body = kernel::sweep_sheet_with(&ss.plane, &chains, &segs, &ctl)?;
                let traced = crate::lineage::ChainRoles { sketch: ss, chains: &chains }.tool(body);
                push_sheet(st, f, traced);
                return Ok(());
            }
            let ps =
                st.sketch(path_sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {path_sketch} (it must come earlier in the timeline)")))?;
            // As Sweep: a 3D path sketch is followed in the feature's frame, the curves with it.
            let profile;
            let ss = if !ps.sketch.wires.is_empty() {
                profile = profile_in_frame(doc, ss, f.component)?;
                &profile
            } else {
                ss
            };
            let mapped = path_in_frame(doc, ps, f.component)?;
            let ps = mapped.as_ref();
            let chains = sheet_chains(ss, profiles.as_ref(), curves)?;
            // The path runs on from the end nearest the middle of the curves.
            let pts: Vec<Vec2> = chains.iter().flatten().flat_map(|s| s.polyline(1e-2)).collect();
            let start = ss.plane.to_world(pts.iter().fold(Vec2::default(), |a, p| a + *p) * (1.0 / pts.len().max(1) as f64));
            let body = match path_points(ps, path, start)? {
                Some(path) => {
                    let pts = path.points();
                    let rails = crate::eval::rail_polylines(doc, st, rails, f.component, pts.first().copied().unwrap_or(start))?;
                    let ctl = crate::eval::sweep_control(vals, orientation, twist, scale, rails, *rail_fit)?;
                    kernel::sweep_sheet_points_with(&ss.plane, &chains, &pts, &ctl)?
                }
                None => {
                    let segs = path_segments_at(ps, path, start, Some(&ss.plane))?;
                    let path_start = segs.first().map_or(start, kernel::PathSeg::start);
                    let rails = crate::eval::rail_polylines(doc, st, rails, f.component, path_start)?;
                    let ctl = crate::eval::sweep_control(vals, orientation, twist, scale, rails, *rail_fit)?;
                    kernel::sweep_sheet_with(&ss.plane, &chains, &segs, &ctl)?
                }
            };
            let traced = crate::lineage::ChainRoles { sketch: ss, chains: &chains }.tool(body);
            push_sheet(st, f, traced);
            Ok(())
        }
        FeatureKind::SurfaceLoft { sections, continuity, crown, rails, rail_fit } => {
            // A loft with edge chains for sections, or one that has to leave its ends a particular
            // way, is built through `loft_blend` from the frames of every section; the plain one
            // (sketch chains only, both ends G0) is the loft of old.
            if *continuity != [kernel::Continuity::G0, kernel::Continuity::G0] || sections.iter().any(|s| s.body.is_some()) {
                if !rails.is_empty() {
                    return Err(DocError::Invalid("not supported yet: guide rails on a loft with edge sections or end continuity".into()));
                }
                return loft_blend(st, f, sections, *continuity, *crown, doc);
            }
            let mut secs = Vec::with_capacity(sections.len());
            let mut sketches = Vec::with_capacity(sections.len());
            for sec in sections {
                let ss = sketch_in_feature_frame(
                    doc,
                    st.sketch(sec.sketch.0)
                        .ok_or_else(|| DocError::Unknown(format!("sketch {} (it must come earlier in the timeline)", sec.sketch)))?,
                    f.component,
                )?;
                let profiles = match (&sec.profiles, sec.curves.is_empty()) {
                    (None, true) => Some(&ProfileSel::All),
                    (p, _) => p.as_ref(),
                };
                let mut chains = sheet_chains(&ss, profiles, &sec.curves)?.into_iter();
                let (Some(chain), None) = (chains.next(), chains.next()) else {
                    return Err(DocError::Invalid(
                        "each loft section must be one chain: one profile without holes, or curves meeting end to end".into(),
                    ));
                };
                secs.push((ss.plane, chain));
                sketches.push(ss);
            }
            let body = if rails.is_empty() {
                kernel::loft_sheet(&secs)?
            } else {
                // The rails run from the end at the first section.
                let start = secs.first().and_then(|(p, c)| c.first().map(|seg| p.to_world(seg.start()))).unwrap_or_default();
                let rails = crate::eval::rail_polylines(doc, st, rails, f.component, start)?;
                kernel::loft_sheet_with_rails(&secs, &rails, crate::eval::kernel_rail_fit(*rail_fit))?
            };
            // Each wall named by the section curves it runs between, as a loft's sides are.
            let loops: Vec<solidcore_record::Loop2> = secs.iter().map(|(_, c)| solidcore_record::Loop2 { segs: c.clone() }).collect();
            let traced = crate::lineage::SectionRoles { sections: sketches.iter().zip(&loops).map(|(s, lp)| (s, vec![lp])).collect() }.tool(body);
            push_sheet(st, f, traced);
            Ok(())
        }
        FeatureKind::SurfaceOffset { faces, body, distance, chain, tolerance } => {
            let d = val(vals, distance, Kind::Length)?;
            let tol = match tolerance {
                Some(t) => val(vals, t, Kind::Length)?,
                None => 0.01,
            };
            let i = body_at(st, body, &crate::plain(faces))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let picks = if *chain { smooth_chain(&mb, &crate::plain(faces)) } else { crate::plain(faces) };
            let mut picked: Vec<usize> = Vec::new();
            for p in &picks {
                let fi =
                    crate::appearance::face_index_at(&mb, *p).ok_or_else(|| DocError::Invalid(format!("Offset: no face of {} at {p:?}", mb.name)))?;
                if !picked.contains(&fi) {
                    picked.push(fi);
                }
            }
            let made = Traced::step(kernel::offset_surface(&mb.body, &picked, d, tol)?, &[&mb.traced()]);
            let name = new_name(st, f, 0);
            st.bodies.push(ModelBody::traced_new(name, made, f.id));
            Ok(())
        }
        FeatureKind::SurfaceRuled { body, edges, distance, mode, angle, direction, flip, corners, face, far } => {
            let d = val(vals, distance, Kind::Length)?;
            let mode = match mode {
                RuledMode::Normal => kernel::RuledMode::Normal,
                RuledMode::Tangent => kernel::RuledMode::Tangent,
                RuledMode::Angle => {
                    let a = angle.as_ref().ok_or_else(|| DocError::Invalid("a ruled surface at an angle needs `angle`".into()))?;
                    kernel::RuledMode::Angle { angle: val(vals, a, Kind::Angle)?, reference: direction.map(Vec3::from) }
                }
                RuledMode::Direction => kernel::RuledMode::Direction(
                    direction.map(Vec3::from).ok_or_else(|| DocError::Invalid("a ruled surface along a direction needs `direction`".into()))?,
                ),
            };
            let corners = match corners {
                RuledCorners::Mitre => kernel::RuledCorners::Mitre,
                RuledCorners::Round => kernel::RuledCorners::Round,
            };
            let mb = st.body(body).cloned().ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            let r = kernel::Ruled { distance: d, mode, flip: *flip, corners, face: face.map(Vec3::from) };
            let handles: Vec<Option<Vec<Vec3>>> = far.iter().map(|h| h.as_ref().map(|pts| crate::plain(pts))).collect();
            let made = Traced::step(kernel::ruled_surface_with(&mb.body, &crate::plain(edges), &r, &handles)?, &[&mb.traced()]);
            let name = new_name(st, f, 0);
            st.bodies.push(ModelBody::traced_new(name, made, f.id));
            Ok(())
        }
        FeatureKind::SurfaceUntrim { face, holes, outer, in_place } => {
            let i = st.bodies.iter().position(|b| b.name == face.body).ok_or_else(|| DocError::Unknown(format!("body `{}`", face.body)))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let fi = crate::appearance::face_index_at(&mb, face.point.0)
                .ok_or_else(|| DocError::Invalid(format!("Untrim: no face of {} at {:?}", mb.name, face.point)))?;
            let which = match holes {
                None => kernel::UntrimHoles::All,
                Some(points) => kernel::UntrimHoles::At(&crate::plain(points)),
            };
            // A surface body's face is untrimmed where it is when asked; a solid's is copied out.
            let replace = *in_place && mb.body.is_surface();
            let made = mb.traced().then(kernel::untrim(&mb.body, fi, which, *outer, replace)?);
            if replace {
                if let Some(slot) = st.bodies.get_mut(i) {
                    *slot = mb.remade(made);
                }
            } else {
                let name = new_name(st, f, 0);
                st.bodies.push(ModelBody::traced_new(name, made, f.id));
            }
            Ok(())
        }
        _ => Err(DocError::Invalid("not a surface feature".into())),
    }
}

/// A surface builder's body, added to the model with what its faces came from.
fn push_sheet(st: &mut ModelState, f: &Feature, traced: crate::lineage::Traced) {
    let name = new_name(st, f, 0);
    st.bodies.push(ModelBody::traced_new(name, traced, f.id));
}

/// The most curves one direction of a network may list (the kernel's own cap).
const MAX_NETWORK_CURVES: usize = solidcraft_geom::network::MAX_NETWORK_CURVES;

/// A loop of three or four sketch curves as one patch: four give the Coons patch of opposite pairs
/// (the first and third one way, the second and fourth the other); three collapse the side
/// opposite the first curve to the corner where the other two meet.
fn loop_patch(ss: &SolvedSketch, curves: &[String]) -> Result<kernel::Body> {
    let mut chains: Vec<Vec<Seg2>> = Vec::with_capacity(curves.len());
    for id in curves {
        let ci = ss.sketch.curve_index(id).ok_or_else(|| DocError::Unknown(format!("sketch curve `{id}`")))?;
        let segs = ss.sketch.exact_segs(ci).unwrap_or_else(|| ss.sketch.segs(ci));
        if segs.is_empty() {
            return Err(DocError::Invalid(format!("curve `{id}` has no geometry to build on")));
        }
        chains.push(segs);
    }
    let side = |k: usize| -> Result<kernel::BoundaryCurve<'static>> {
        let (Some(id), Some(segs)) = (curves.get(k), chains.get(k)) else { return Err(DocError::Invalid("a boundary curve is missing".into())) };
        Ok(kernel::BoundaryCurve {
            label: format!("curve `{id}`"),
            source: kernel::CurveSource::Sketch { plane: ss.plane, segs: segs.clone() },
            continuity: kernel::Continuity::G0,
        })
    };
    match curves.len() {
        4 => Ok(kernel::boundary_surface(&[side(0)?, side(2)?], &[side(1)?, side(3)?])?),
        3 => {
            // The corner the second and third curves share, whichever way each was drawn.
            let ends = |k: usize| -> Option<[Vec2; 2]> {
                let c = chains.get(k)?;
                Some([c.first()?.start(), c.last()?.end()])
            };
            let (Some(b), Some(c)) = (ends(1), ends(2)) else { return Err(DocError::Invalid("a boundary curve is missing".into())) };
            let mut best = (f64::INFINITY, Vec2::new(0.0, 0.0));
            for p in b {
                for q in c {
                    if p.dist(q) < best.0 {
                        best = (p.dist(q), (p + q) * 0.5);
                    }
                }
            }
            let apex = kernel::BoundaryCurve {
                label: "the corner opposite the first curve".into(),
                source: kernel::CurveSource::Point(ss.plane.to_world(best.1)),
                continuity: kernel::Continuity::G0,
            };
            Ok(kernel::boundary_surface(&[side(0)?, apex], &[side(1)?, side(2)?])?)
        }
        _ => Err(DocError::Invalid("a loop patch takes three or four curves".into())),
    }
}

/// A network curve resolved in the feature's frame, owning what it is built from.
enum NetSource {
    Sketch(solidcore_record::Plane, Vec<Seg2>),
    Points(Vec<Vec3>),
    Edges(kernel::Body, Vec<Vec3>),
    Point(Vec3),
}

/// The curves of one direction of a network, resolved: each one sketch chain, 3D sketch chain,
/// edge chain or point, labelled `U1`, `U2`, … (with `dir` "U") for messages.
fn network_sources(
    doc: &Document,
    st: &ModelState,
    f: &Feature,
    list: &[crate::NetworkCurve],
    dir: &str,
) -> Result<Vec<(String, NetSource, kernel::Continuity)>> {
    if list.len() > MAX_NETWORK_CURVES {
        return Err(DocError::Invalid(format!("at most {MAX_NETWORK_CURVES} curves each way")));
    }
    let mut out = Vec::with_capacity(list.len());
    for (k, c) in list.iter().enumerate() {
        let label = format!("{dir}{}", k + 1);
        let given = [c.sketch.is_some() || !c.curves.is_empty(), c.body.is_some() || !c.edges.is_empty(), c.point.is_some()];
        if given.iter().filter(|g| **g).count() != 1 {
            return Err(DocError::Invalid(format!("{label} must be sketch curves, a body's edges or a point")));
        }
        let source = if let Some(p) = c.point {
            NetSource::Point(p.0)
        } else if let Some(name) = &c.body {
            if c.edges.is_empty() {
                return Err(DocError::Invalid(format!("{label} needs points on the edges of {name}")));
            }
            let b = st.body(name).ok_or_else(|| DocError::Unknown(format!("body `{name}`")))?;
            NetSource::Edges(b.body.clone(), crate::plain(&c.edges))
        } else {
            let sk = c.sketch.ok_or_else(|| DocError::Invalid(format!("{label} needs the sketch its curves are in")))?;
            if c.curves.is_empty() || c.curves.len() > MAX_CHAIN_CURVES {
                return Err(DocError::Invalid(format!("{label} needs 1 to {MAX_CHAIN_CURVES} curves of its sketch")));
            }
            let raw = st.sketch(sk.0).ok_or_else(|| DocError::Unknown(format!("sketch {sk} (it must come earlier in the timeline)")))?;
            // 3D sketch curves are followed through their points; planar ones exactly.
            let mapped = path_in_frame(doc, raw, f.component)?;
            match path_points(&mapped, &c.curves, Vec3::new(f64::NAN, f64::NAN, f64::NAN))? {
                Some(path) => NetSource::Points(path.points()),
                None => {
                    let ss = sketch_in_feature_frame(doc, raw, f.component)?;
                    let mut chains = curve_chains(&ss, &c.curves)?.into_iter();
                    let (Some(chain), None) = (chains.next(), chains.next()) else {
                        return Err(DocError::Invalid(format!("{label}: its curves must meet end to end in one chain")));
                    };
                    NetSource::Sketch(ss.plane, chain)
                }
            }
        };
        out.push((label, source, c.continuity));
    }
    Ok(out)
}

/// The kernel's curves over resolved sources (borrowing their bodies).
fn network_curves(sources: &[(String, NetSource, kernel::Continuity)]) -> Vec<kernel::BoundaryCurve<'_>> {
    sources
        .iter()
        .map(|(label, src, g)| kernel::BoundaryCurve {
            label: label.clone(),
            source: match src {
                NetSource::Sketch(plane, segs) => kernel::CurveSource::Sketch { plane: *plane, segs: segs.clone() },
                NetSource::Points(pts) => kernel::CurveSource::Points(pts.clone()),
                NetSource::Edges(body, at) => kernel::CurveSource::Edges { body, at: at.clone() },
                NetSource::Point(p) => kernel::CurveSource::Point(*p),
            },
            continuity: *g,
        })
        .collect()
}

/// The boundaries `surface.boundary` builds its patch from: each picked curve's own stations in
/// the sketch's frame, every one of them leaving the boundary square into the loop — the direction
/// from the curve towards the middle of the loop, taken across its tangent — over the plane the
/// curve lies in (so a chain of planar curves makes a flat patch, and a network that is not
/// coplanar is followed as the plane through the curve's own tangent). `orders` sets how much of
/// that plane each side repeats.
fn boundary_frames(
    ss: &SolvedSketch,
    curves: &[String],
    orders: &[solidcraft_kernel::Continuity],
    samples: usize,
) -> Result<Vec<solidcraft_kernel::Boundary>> {
    use solidcraft_kernel::{Boundary, Frame};
    // The stations along each curve, evenly in its own arclength, so every side carries the same
    // number of frames and the patch's net is square.
    let mut sides: Vec<Vec<solidcore_record::Vec3>> = Vec::with_capacity(curves.len());
    for id in curves {
        let ci = ss.sketch.curve_index(id).ok_or_else(|| DocError::Unknown(format!("sketch curve `{id}`")))?;
        let pieces = ss.sketch.exact_pieces(ci);
        if pieces.is_empty() {
            return Err(DocError::Invalid(format!("curve `{id}` has no exact geometry to build on")));
        }
        let lengths: Vec<f64> = pieces.iter().map(Seg2::length).collect();
        let total: f64 = lengths.iter().sum();
        if !total.is_finite() || total <= 1e-9 {
            return Err(DocError::Invalid(format!("curve `{id}` has no length")));
        }
        let mut pts = Vec::with_capacity(samples);
        let (mut used, mut at) = (0.0, 0usize);
        for k in 0..samples {
            let want = total * k as f64 / (samples.saturating_sub(1)).max(1) as f64;
            while at + 1 < pieces.len() && used + lengths[at] < want {
                used += lengths[at];
                at += 1;
            }
            let (Some(piece), Some(len)) = (pieces.get(at), lengths.get(at)) else { return Err(DocError::Invalid("boundary piece".into())) };
            let t = ((want - used) / len.max(1e-12)).clamp(0.0, 1.0);
            pts.push(ss.plane.to_world(piece.point_at(t)));
        }
        sides.push(pts);
    }
    // The middle of the loop, so every side leaves towards the inside of the patch.
    let count = sides.iter().map(Vec::len).sum::<usize>().max(1) as f64;
    let centre = sides.iter().flatten().fold(solidcore_record::Vec3::ZERO, |a, p| a + *p) * (1.0 / count);
    let mut out = Vec::with_capacity(sides.len());
    for (side, pts) in sides.iter().enumerate() {
        let mut frames = Vec::with_capacity(pts.len());
        for (i, p) in pts.iter().enumerate() {
            // The tangent from the stations on either side, one-sided at the ends (where the side
            // only has one neighbour).
            let before = if i == 0 { *p } else { pts[i - 1] };
            let after = if i + 1 == pts.len() { *p } else { pts[i + 1] };
            let tangent = (after - before).normalized().ok_or_else(|| DocError::Invalid("a boundary curve has a stationary station".into()))?;
            let across = centre - *p;
            let leave = (across - tangent * across.dot(tangent))
                .normalized()
                .ok_or_else(|| DocError::Invalid("a boundary runs through the middle of the loop: no patch can leave it".into()))?;
            frames.push(Frame { p: *p, leave, bend: Vec3::ZERO, bend_change: Vec3::ZERO });
        }
        out.push(Boundary { frames, order: orders.get(side).copied().unwrap_or(solidcraft_kernel::Continuity::G1) });
    }
    Ok(out)
}

/// Mutual Trim: the body at `i` and `other` trimmed by each other (each keeping its picks), the
/// kept pieces sewn into the first when `stitch`; their faces named after the faces they are
/// pieces of.
fn mutual_trim(st: &mut ModelState, i: usize, other: &str, keeps: kernel::TrimKeep, keep_with: &[Vec3], stitch: bool, extend: bool) -> Result<()> {
    let j = st.bodies.iter().position(|b| b.name == other).ok_or_else(|| DocError::Unknown(format!("body `{other}`")))?;
    if i == j {
        return Err(DocError::Invalid("a surface can't trim itself: pick another surface".into()));
    }
    let (Some(a), Some(b)) = (st.bodies.get(i).cloned(), st.bodies.get(j).cloned()) else { return Err(DocError::Invalid("body".into())) };
    let keeps_with = match keep_with {
        [] => return Err(DocError::Invalid(format!("pick the side of `{other}` to keep"))),
        [p] => kernel::TrimKeep::Side(*p),
        ps => kernel::TrimKeep::Regions(ps),
    };
    let (ta, tb) = kernel::mutual_trim(&a.body, &b.body, keeps, keeps_with, extend)?;
    let (ta, tb) = (Traced::step(ta, &[&a.traced()]), Traced::step(tb, &[&b.traced()]));
    if stitch {
        let sewn = kernel::stitch(&[&ta.body, &tb.body], 0.0)?;
        if sewn.edge_count() >= ta.body.edge_count() + tb.body.edge_count() {
            return Err(DocError::Invalid("the trimmed surfaces don't meet along an edge: they can't be stitched".into()));
        }
        let sewn = Traced::step(sewn, &[&ta, &tb]);
        if let Some(slot) = st.bodies.get_mut(i) {
            *slot = ModelBody::traced_new(a.name, sewn, a.feature);
        }
        st.bodies.retain(|x| x.name != other);
    } else {
        if let Some(slot) = st.bodies.get_mut(i) {
            *slot = ModelBody::traced_new(a.name, ta, a.feature);
        }
        if let Some(slot) = st.bodies.get_mut(j) {
            *slot = ModelBody::traced_new(b.name, tb, b.feature);
        }
    }
    Ok(())
}

/// What a surface builder sweeps: the boundary loops of the picked profiles, then the picked
/// curves chained end to end.
fn sheet_chains(ss: &SolvedSketch, profiles: Option<&ProfileSel>, curves: &[String]) -> Result<Vec<Vec<Seg2>>> {
    let mut chains: Vec<Vec<Seg2>> = Vec::new();
    if let Some(sel) = profiles {
        for r in solidcraft_sketch::merge_regions(&select_profiles(ss, sel)?) {
            chains.extend(std::iter::once(r.outer).chain(r.holes).map(|lp| lp.segs));
        }
    }
    chains.extend(curve_chains(ss, curves)?);
    if chains.is_empty() {
        return Err(DocError::Invalid("pick the profiles or curves".into()));
    }
    Ok(chains)
}

/// Most sketch curves one feature chains.
const MAX_CHAIN_CURVES: usize = 1000;
/// Stations a blend samples each of its two boundaries at: enough that the boundary it fits is
/// within a small part of a thousandth of the chain's length of the edges it was built on.
const BLEND_STATIONS: usize = 41;

/// The sketch curves `ids` (free-form ones exactly where they can be) grouped into chains of segments that meet end to end, each oriented
/// to run one way. Curves are joined greedily at their free ends: where three meet, one of them
/// starts a chain of its own.
pub(super) fn curve_chains(ss: &SolvedSketch, ids: &[String]) -> Result<Vec<Vec<Seg2>>> {
    if ids.len() > MAX_CHAIN_CURVES {
        return Err(DocError::Invalid(format!("at most {MAX_CHAIN_CURVES} curves")));
    }
    let mut pieces: std::collections::VecDeque<Vec<Seg2>> = ids
        .iter()
        .map(|id| {
            let ci = ss.sketch.curve_index(id).ok_or_else(|| DocError::Unknown(format!("curve `{id}`")))?;
            Ok(ss.sketch.exact_segs(ci).unwrap_or_else(|| ss.sketch.segs(ci)))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect();
    let reversed = |p: Vec<Seg2>| -> Vec<Seg2> { p.iter().rev().map(Seg2::reversed).collect() };
    let near = |p: Vec2, q: Vec2| p.dist(q) <= 1e-6;
    let mut chains = Vec::new();
    while let Some(mut chain) = pieces.pop_front() {
        while let (Some(head), Some(tail)) = (chain.first().map(Seg2::start), chain.last().map(Seg2::end)) {
            if near(head, tail) {
                break;
            }
            let ends = |p: &Vec<Seg2>| p.first().map(Seg2::start).zip(p.last().map(Seg2::end));
            let Some(i) = pieces.iter().position(|p| ends(p).is_some_and(|(s, e)| near(s, tail) || near(e, tail) || near(s, head) || near(e, head)))
            else {
                break;
            };
            let Some(p) = pieces.remove(i) else { break };
            let Some((s, e)) = ends(&p) else { continue };
            if near(s, tail) {
                chain.extend(p);
            } else if near(e, tail) {
                chain.extend(reversed(p));
            } else if near(e, head) {
                chain = p.into_iter().chain(chain).collect();
            } else {
                chain = reversed(p).into_iter().chain(chain).collect();
            }
        }
        chains.push(chain);
    }
    Ok(chains)
}

/// A multi-section blend: every section as frames, the body ones read off the face their edge
/// chain bounds, and the first and last leaving with `continuity` where the caller asks for it.
fn loft_blend(
    st: &mut ModelState,
    f: &Feature,
    sections: &[SheetSection],
    continuity: [kernel::Continuity; 2],
    crown: [f64; 2],
    doc: &Document,
) -> Result<()> {
    /// One section: a chain of sketch curves in its plane, or a body's edge chain.
    enum Piece {
        Sketch(solidcore_record::Plane, Vec<solidcore_record::Seg2>),
        Body(String, Vec<solidcore_record::Vec3>),
    }
    let mut pieces = Vec::with_capacity(sections.len());
    let mut centres = Vec::with_capacity(sections.len());
    for sec in sections {
        if let Some(name) = sec.body.as_deref() {
            if sec.edges.is_empty() {
                return Err(DocError::Invalid("a loft section picked from a body needs a chain of its edges".into()));
            }
            let n = sec.edges.len() as f64;
            centres.push(sec.edges.iter().fold(solidcore_record::Vec3::ZERO, |a, p| a + **p) * (1.0 / n));
            pieces.push(Piece::Body(name.to_string(), crate::plain(&sec.edges)));
            continue;
        }
        let ss = sketch_in_feature_frame(
            doc,
            st.sketch(sec.sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {} (it must come earlier in the timeline)", sec.sketch)))?,
            f.component,
        )?;
        let profiles = match (&sec.profiles, sec.curves.is_empty()) {
            (None, true) => Some(&ProfileSel::All),
            (p, _) => p.as_ref(),
        };
        let mut chains = sheet_chains(&ss, profiles, &sec.curves)?.into_iter();
        let (Some(chain), None) = (chains.next(), chains.next()) else {
            return Err(DocError::Invalid("each loft section must be one chain: one profile without holes, or curves meeting end to end".into()));
        };
        let pts: Vec<Vec2> = chain.iter().flat_map(|s| s.polyline(1e-2)).collect();
        let centre = pts.iter().fold(solidcore_record::Vec3::ZERO, |a, p| a + ss.plane.to_world(*p)) * (1.0 / pts.len().max(1) as f64);
        centres.push(centre);
        pieces.push(Piece::Sketch(ss.plane, chain));
    }
    // Each section leaves towards the one after it; the last one leaves back into the blend.
    let stations = BLEND_STATIONS;
    let last = pieces.len().saturating_sub(1);
    let mut frames = Vec::with_capacity(pieces.len());
    for (i, piece) in pieces.iter().enumerate() {
        let (from, to) = if i < last { (i, i + 1) } else { (last, last.saturating_sub(1)) };
        let leave = match (centres.get(from), centres.get(to)) {
            (Some(a), Some(b)) => *b - *a,
            _ => solidcore_record::Vec3::ZERO,
        };
        match piece {
            Piece::Body(name, edges) => {
                let b = st.body(name).ok_or_else(|| DocError::Unknown(format!("body `{name}`")))?.body.clone();
                frames.push(kernel::loft_blend::section_frames(&b, edges, leave, stations)?);
            }
            Piece::Sketch(plane, chain) => frames.push(chain_frames(plane, chain, leave, stations)),
        }
    }
    let body = kernel::loft_blend(&frames, continuity, crown, stations)?;
    let name = new_name(st, f, 0);
    st.bodies.push(ModelBody::new(name, body, f.id));
    Ok(())
}

/// The frames of one sketch chain, sampled by arclength at `stations` stations: a sketch has no
/// face behind it, so the surface leaves along the run with no curvature to repeat there.
fn chain_frames(
    plane: &solidcore_record::Plane,
    chain: &[solidcore_record::Seg2],
    leave: solidcore_record::Vec3,
    stations: usize,
) -> Vec<kernel::Frame> {
    let mut pts: Vec<Vec2> = Vec::new();
    for seg in chain {
        for p in seg.polyline(1e-2) {
            if pts.last().is_none_or(|l: &Vec2| (*l - p).len() > 1e-12) {
                pts.push(p);
            }
        }
    }
    if pts.len() < 2 || stations < 2 {
        return Vec::new();
    }
    let total: f64 = pts.windows(2).map(|w| (w[1] - w[0]).len()).sum();
    if !(total > 1e-12) {
        return Vec::new();
    }
    let step = total / (stations - 1) as f64;
    let (mut walked, mut k) = (0.0f64, 0usize);
    let mut out = Vec::with_capacity(stations);
    for i in 0..stations {
        let want = step * i as f64;
        while k + 1 < pts.len() && walked + (pts[k + 1] - pts[k]).len() < want {
            walked += (pts[k + 1] - pts[k]).len();
            k += 1;
        }
        let (Some(a), Some(b)) = (pts.get(k).copied(), pts.get(k + 1).copied()) else {
            if let Some(p) = pts.get(k).copied() {
                out.push(kernel::Frame {
                    p: plane.to_world(p),
                    leave,
                    bend: solidcore_record::Vec3::ZERO,
                    bend_change: solidcore_record::Vec3::ZERO,
                });
            }
            continue;
        };
        let span = (b - a).len();
        let t = if span > 1e-12 { ((want - walked) / span).clamp(0.0, 1.0) } else { 0.0 };
        let p = a * (1.0 - t) + b * t;
        out.push(kernel::Frame { p: plane.to_world(p), leave, bend: solidcore_record::Vec3::ZERO, bend_change: solidcore_record::Vec3::ZERO });
    }
    out
}
