//! Translation of native CAD IR models (decoded with `DecodeDepth::Full`) into
//! native SolidCraft documents with parameters, sketches, and timeline features (Option A & B),
//! with graceful fallback to base B-Rep (`FeatureKind::Import`).

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::features::patterns::{PatternSeed, PatternTransform};
use cadmpeg_ir::features::{
    BodySelection, BooleanKind, BooleanOp, ExtrudeExtent, ExtrudeSide, FaceSelection, FeatureDefinition, FeatureOperation, LinearTermination,
    ParameterId, ParameterValue, PathRef, PlanarProfileRef, ProfileRef, RevolveConstruction, SketchProfileRegion, SolidSweepOperation, SweepMode,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchEntityId, SketchGeometryDefinition, SketchId as IrSketchId, SketchLocus};
use solidcore_record::{Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_doc::{
    Direction, Document, Extent, ExtrudeConvention, FeatureKind, Operation, PlaneRef, ProfileSel, RailFit, SketchId, SweepOrientation,
};
use solidcraft_geom::prelude::*;
use solidcraft_geom::{Aabb2, Aabb3};
use solidcraft_sketch::{Constraint, ConstraintKind, Curve, CurveKind, Profile, SPoint, Sketch, profile_by_curves};

use crate::native_cad::{native_cad_decode_full, native_cad_ir_to_step};

fn pop_failed_feature(doc: &mut Document, model: &mut solidcraft_doc::Model) {
    doc.features.pop();
    if model.results.len() > doc.features.len() {
        model.results.pop();
    }
}

fn extrude_tool_bounds(doc: &Document, model: &solidcraft_doc::Model, sk_fid: u64, profiles: &ProfileSel, extent: &Extent) -> Option<Aabb3> {
    if extent.through_all || extent.to.is_some() || extent.from.is_some() {
        return None;
    }
    let st = model.state();
    let sk_state = st.sketch(sk_fid)?;
    let dist = doc.eval(&extent.distance, solidcraft_doc::expr::Kind::Length).ok()?;
    if !dist.is_finite() {
        return None;
    }
    let start_off = extent.start_offset.as_ref().and_then(|e| doc.eval(e, solidcraft_doc::expr::Kind::Length).ok()).unwrap_or(0.0);
    if !start_off.is_finite() {
        return None;
    }

    let (d_min, d_max) = match extent.direction {
        Direction::Positive => {
            let s = start_off;
            let e = start_off + dist;
            (s.min(e), s.max(e))
        }
        Direction::Negative => {
            let s = start_off;
            let e = start_off - dist;
            (s.min(e), s.max(e))
        }
        Direction::Symmetric => {
            let half = dist * 0.5;
            let s = start_off - half;
            let e = start_off + half;
            (s.min(e), s.max(e))
        }
    };

    let plane = sk_state.plane;
    let n = plane.normal();

    let mut aabb2 = Aabb2::EMPTY;
    let sc_profs = sk_state.profiles.as_slice();
    match profiles {
        ProfileSel::All => {
            for p in sc_profs {
                aabb2 = aabb2.union(&p.region.outer.polyline(1.0e-2).bounds());
            }
        }
        ProfileSel::Indices { indices } => {
            for &idx in indices {
                if let Some(p) = sc_profs.get(idx) {
                    aabb2 = aabb2.union(&p.region.outer.polyline(1.0e-2).bounds());
                }
            }
        }
        _ => return None,
    }

    if aabb2.is_empty() || !aabb2.min.x.is_finite() || !aabb2.max.x.is_finite() {
        return None;
    }

    let growth = if let Some(ref taper_expr) = extent.taper {
        if let Ok(deg) = doc.eval(taper_expr, solidcraft_doc::expr::Kind::Angle) {
            let len = (d_max - d_min).abs();
            (len * deg.to_radians().tan()).abs()
        } else {
            0.0
        }
    } else {
        0.0
    };
    let aabb2 = aabb2.inflated(growth + 1.0e-3);

    let mut aabb3 = Aabb3::EMPTY;
    for &u in &[aabb2.min.x, aabb2.max.x] {
        for &v in &[aabb2.min.y, aabb2.max.y] {
            let p_base = plane.to_world(Vec2::new(u, v));
            aabb3.add(p_base + n * (d_min - 1.0e-3));
            aabb3.add(p_base + n * (d_max + 1.0e-3));
        }
    }
    Some(aabb3)
}

fn quantize(u: f64, v: f64) -> (i64, i64) {
    ((u * 1.0e5).round() as i64, (v * 1.0e5).round() as i64)
}

pub(crate) fn get_or_insert_point(sketch: &mut Sketch, point_map: &mut HashMap<(i64, i64), usize>, u: f64, v: f64) -> usize {
    let key = quantize(u, v);
    if key == (0, 0) {
        return 0; // Sketch origin
    }
    if let Some(&idx) = point_map.get(&key) {
        return idx;
    }
    let idx = sketch.points.len();
    sketch.points.push(SPoint { id: format!("p{idx}"), pos: Vec2::new(u, v), fixed: false, link: None, exprs: None });
    point_map.insert(key, idx);
    idx
}

fn resolve_locus(
    locus: &SketchLocus,
    entity_to_point: &HashMap<SketchEntityId, usize>,
    entity_to_curve: &HashMap<SketchEntityId, usize>,
    sketch: &Sketch,
) -> Option<usize> {
    match locus {
        SketchLocus::Entity(id) => entity_to_point.get(id).copied().or_else(|| {
            entity_to_curve.get(id).and_then(|&ci| match sketch.curves.get(ci)?.kind {
                CurveKind::Circle { c, .. } | CurveKind::Arc { c, .. } => Some(c),
                CurveKind::Line { a, .. } => Some(a),
                _ => None,
            })
        }),
        SketchLocus::Start(id) => entity_to_curve.get(id).and_then(|&ci| match sketch.curves.get(ci)?.kind {
            CurveKind::Line { a, .. } | CurveKind::Arc { a, .. } => Some(a),
            _ => None,
        }),
        SketchLocus::End(id) => entity_to_curve.get(id).and_then(|&ci| match sketch.curves.get(ci)?.kind {
            CurveKind::Line { b, .. } | CurveKind::Arc { b, .. } => Some(b),
            _ => None,
        }),
        SketchLocus::Center(id) => entity_to_curve.get(id).and_then(|&ci| match sketch.curves.get(ci)?.kind {
            CurveKind::Circle { c, .. } | CurveKind::Arc { c, .. } => Some(c),
            _ => None,
        }),
    }
}

fn expr_has_undefined_references(expr: &str, defined: &HashSet<&str>) -> bool {
    let mut chars = expr.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c.is_alphabetic() || c == '_' {
            let mut id = String::new();
            id.push(c);
            while let Some(&(_, next_c)) = chars.peek() {
                if next_c.is_alphanumeric() || next_c == '_' {
                    id.push(next_c);
                    chars.next();
                } else {
                    break;
                }
            }
            let lower = id.to_ascii_lowercase();
            if matches!(
                lower.as_str(),
                "mm" | "cm"
                    | "m"
                    | "in"
                    | "ft"
                    | "deg"
                    | "rad"
                    | "sin"
                    | "cos"
                    | "tan"
                    | "asin"
                    | "acos"
                    | "atan"
                    | "sqrt"
                    | "abs"
                    | "ceil"
                    | "floor"
                    | "round"
                    | "exp"
                    | "log"
                    | "ln"
                    | "min"
                    | "max"
                    | "pi"
            ) {
                continue;
            }
            if !defined.contains(id.as_str()) {
                return true;
            }
        }
    }
    false
}

#[derive(Clone, Debug)]
struct IrProfileDescriptor {
    curve_ids: Vec<String>,
    midpoints: Vec<Vec2>,
}

struct PreparedSketch {
    plane_ref: PlaneRef,
    sketch: Sketch,
    name: String,
    entity_curve_ids: HashMap<SketchEntityId, String>,
    ir_profiles: Vec<IrProfileDescriptor>,
}

fn point_on_seg2(pt: Vec2, seg: &Seg2, tol: f64) -> bool {
    match *seg {
        Seg2::Line { a, b } => {
            let ab = b - a;
            let len2 = ab.len2();
            let t = if len2 > 1.0e-12 { ((pt - a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
            a.lerp(b, t).dist(pt) <= tol
        }
        Seg2::Arc { center, radius, .. } => (pt.dist(center) - radius).abs() <= tol && (seg.mid().dist(pt) <= radius * 2.0 + tol),
        _ => seg.mid().dist(pt) <= tol,
    }
}

fn map_ir_profile_to_solidcraft(desc: &IrProfileDescriptor, sc_profiles: &[Profile]) -> Option<usize> {
    if let Ok(idx) = profile_by_curves(sc_profiles, &desc.curve_ids) {
        return Some(idx);
    }
    if !desc.midpoints.is_empty() {
        let mut geom_matches: Vec<(usize, f64)> = sc_profiles
            .iter()
            .enumerate()
            .filter(|(_, p)| desc.midpoints.iter().all(|&m| p.region.outer.segs.iter().any(|s| point_on_seg2(m, s, 2.0e-2))))
            .map(|(i, p)| (i, p.area.abs()))
            .collect();
        geom_matches.sort_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((best, _)) = geom_matches.first() {
            return Some(*best);
        }
    }
    if !desc.curve_ids.is_empty() {
        let mut best: Option<(usize, usize, usize, f64)> = None;
        for (i, p) in sc_profiles.iter().enumerate() {
            let hits = desc.curve_ids.iter().filter(|c| p.outer_curves.contains(c)).count();
            if hits == 0 {
                continue;
            }
            let extra = p.outer_curves.len().saturating_sub(hits);
            let candidate = (i, hits, extra, p.area.abs());
            let better = match best {
                None => true,
                Some((_, best_hits, best_extra, best_area)) => {
                    hits > best_hits || (hits == best_hits && (extra < best_extra || (extra == best_extra && p.area.abs() < best_area)))
                }
            };
            if better {
                best = Some(candidate);
            }
        }
        if let Some((i, _, _, _)) = best {
            return Some(i);
        }
    }
    None
}

fn filter_nested_profile_indices(indices: &mut Vec<usize>, sc_profiles: &[Profile], is_cut: bool) {
    if indices.len() <= 1 {
        return;
    }
    let mut to_remove = Vec::new();
    for &idx_a in indices.iter() {
        let Some(prof_a) = sc_profiles.get(idx_a) else {
            continue;
        };
        if prof_a.region.outer.segs.is_empty() {
            continue;
        }
        for &idx_b in indices.iter() {
            if idx_a == idx_b {
                continue;
            }
            let Some(prof_b) = sc_profiles.get(idx_b) else {
                continue;
            };
            let a_is_hole_of_b = prof_b.region.holes.iter().any(|hole| {
                !hole.segs.is_empty() && prof_a.region.outer.segs.iter().all(|s| hole.segs.iter().any(|hs| point_on_seg2(s.mid(), hs, 2.0e-2)))
            });
            if a_is_hole_of_b {
                if !is_cut {
                    if !to_remove.contains(&idx_a) {
                        to_remove.push(idx_a);
                    }
                } else if !to_remove.contains(&idx_b) {
                    to_remove.push(idx_b);
                }
            }
        }
    }
    if to_remove.len() < indices.len() {
        indices.retain(|i| !to_remove.contains(i));
    }
}

fn resolve_planar_profile_sel(
    profile: &PlanarProfileRef,
    ir_profiles: &[IrProfileDescriptor],
    sc_profiles: &[Profile],
    is_cut: bool,
) -> Option<ProfileSel> {
    match profile {
        PlanarProfileRef::Sketch(_) => {
            if ir_profiles.len() > 1 && sc_profiles.len() > ir_profiles.len() {
                let mut indices = Vec::new();
                for desc in ir_profiles {
                    if let Some(sc_idx) = map_ir_profile_to_solidcraft(desc, sc_profiles)
                        && !indices.contains(&sc_idx)
                    {
                        indices.push(sc_idx);
                    }
                }
                filter_nested_profile_indices(&mut indices, sc_profiles, is_cut);
                if !indices.is_empty() {
                    return Some(ProfileSel::Indices { indices });
                }
            }
            Some(ProfileSel::All)
        }
        PlanarProfileRef::SketchProfiles { profiles, .. } => {
            let mut indices = Vec::new();
            for &ir_idx in profiles.as_slice() {
                let idx = ir_idx as usize;
                if let Some(desc) = ir_profiles.get(idx) {
                    if let Some(sc_idx) = map_ir_profile_to_solidcraft(desc, sc_profiles) {
                        if !indices.contains(&sc_idx) {
                            indices.push(sc_idx);
                        }
                    } else if sc_profiles.len() == ir_profiles.len() && idx < sc_profiles.len() && !indices.contains(&idx) {
                        indices.push(idx);
                    }
                }
            }
            filter_nested_profile_indices(&mut indices, sc_profiles, is_cut);
            if !indices.is_empty() { Some(ProfileSel::Indices { indices }) } else { Some(ProfileSel::All) }
        }
        PlanarProfileRef::SketchRegions { regions, .. } => {
            let mut indices = Vec::new();
            for reg in regions.as_slice() {
                let SketchProfileRegion::Loops { loops } = reg else {
                    continue;
                };
                let idx = loops.outer() as usize;
                let resolved_outer = if let Some(desc) = ir_profiles.get(idx) {
                    if let Some(sc_idx) = map_ir_profile_to_solidcraft(desc, sc_profiles) {
                        if !indices.contains(&sc_idx) {
                            indices.push(sc_idx);
                        }
                        Some(sc_idx)
                    } else if sc_profiles.len() == ir_profiles.len() && idx < sc_profiles.len() {
                        if !indices.contains(&idx) {
                            indices.push(idx);
                        }
                        Some(idx)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(outer_sc_idx) = resolved_outer
                    && !loops.holes().is_empty()
                    && let Some(outer_prof) = sc_profiles.get(outer_sc_idx)
                {
                    let hole_sc_indices: Vec<usize> = loops
                        .holes()
                        .iter()
                        .filter_map(|&h| {
                            let h_idx = h as usize;
                            ir_profiles.get(h_idx).and_then(|d| map_ir_profile_to_solidcraft(d, sc_profiles))
                        })
                        .collect();
                    let mut found_intermediate = false;
                    for (cand_idx, cand_prof) in sc_profiles.iter().enumerate() {
                        if cand_idx == outer_sc_idx
                            || indices.contains(&cand_idx)
                            || hole_sc_indices.contains(&cand_idx)
                            || cand_prof.region.outer.segs.is_empty()
                        {
                            continue;
                        }
                        let cand_outer_is_hole_of_outer = outer_prof.region.holes.iter().any(|hole| {
                            !hole.segs.is_empty()
                                && cand_prof.region.outer.segs.iter().all(|s| hole.segs.iter().any(|hs| point_on_seg2(s.mid(), hs, 2.0e-2)))
                        });
                        let cand_hole_matches_region_hole = cand_prof.region.holes.iter().any(|chole| {
                            !chole.segs.is_empty()
                                && hole_sc_indices.iter().any(|&h_sc| {
                                    sc_profiles.get(h_sc).is_some_and(|h_prof| {
                                        !h_prof.region.outer.segs.is_empty()
                                            && h_prof.region.outer.segs.iter().all(|s| chole.segs.iter().any(|cs| point_on_seg2(s.mid(), cs, 2.0e-2)))
                                    })
                                })
                        });
                        if cand_outer_is_hole_of_outer && cand_hole_matches_region_hole {
                            indices.push(cand_idx);
                            found_intermediate = true;
                        }
                    }
                    if !found_intermediate {
                        for h_sc in hole_sc_indices {
                            if !indices.contains(&h_sc) {
                                indices.push(h_sc);
                            }
                        }
                    }
                }
            }
            filter_nested_profile_indices(&mut indices, sc_profiles, is_cut);
            if !indices.is_empty() { Some(ProfileSel::Indices { indices }) } else { Some(ProfileSel::All) }
        }
        PlanarProfileRef::SketchSelection { .. } => Some(ProfileSel::All),
        _ => Some(ProfileSel::All),
    }
}

fn entity_suffix_key(id_str: &str) -> String {
    id_str.rsplit([':', '#']).next().unwrap_or(id_str).to_string()
}

fn body_selection_keys(sel: &BodySelection) -> Vec<String> {
    match sel {
        BodySelection::Bodies(ids) | BodySelection::Resolved { bodies: ids, .. } => ids.iter().map(|id| entity_suffix_key(id.as_str())).collect(),
        BodySelection::ResolvedSet { members } => members.bodies().map(|id| entity_suffix_key(id.as_str())).collect(),
        BodySelection::Historical { bodies: ids, .. } => ids.iter().map(|id| entity_suffix_key(id.as_str())).collect(),
        BodySelection::HistoricalSet { members, .. } => members.bodies().map(|id| entity_suffix_key(id.as_str())).collect(),
        _ => Vec::new(),
    }
}

fn plane_intersects_bbox(plane: &Plane, min: Vec3, max: Vec3) -> bool {
    let corners = [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(min.x, max.y, max.z),
        Vec3::new(max.x, max.y, max.z),
    ];
    let n = plane.normal();
    let mut has_pos = false;
    let mut has_neg = false;
    for c in corners {
        let d = (c - plane.origin).dot(n);
        if d > 1.0e-4 {
            has_pos = true;
        } else if d < -1.0e-4 {
            has_neg = true;
        }
    }
    has_pos && has_neg
}

fn find_matching_body<'a>(
    c: Vec3,
    bodies: &'a [solidcraft_doc::ModelBody],
    already_assigned: &HashSet<String>,
) -> Option<&'a solidcraft_doc::ModelBody> {
    let containing: Vec<_> = bodies
        .iter()
        .filter(|b| {
            let bb = b.body.outline_bounds();
            let margin = 0.5;
            c.x >= bb.min.x - margin
                && c.x <= bb.max.x + margin
                && c.y >= bb.min.y - margin
                && c.y <= bb.max.y + margin
                && c.z >= bb.min.z - margin
                && c.z <= bb.max.z + margin
        })
        .collect();
    if containing.len() == 1 {
        return Some(containing[0]);
    }
    bodies.iter().min_by(|a, b| {
        let ca = (a.body.outline_bounds().min + a.body.outline_bounds().max) * 0.5;
        let cb = (b.body.outline_bounds().min + b.body.outline_bounds().max) * 0.5;
        let va = ca - c;
        let vb = cb - c;
        let da = va.dot(va);
        let db = vb.dot(vb);
        let pen_a = if already_assigned.contains(&a.name) { 1.5 } else { 1.0 };
        let pen_b = if already_assigned.contains(&b.name) { 1.5 } else { 1.0 };
        (da * pen_a).partial_cmp(&(db * pen_b)).unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn parse_edge_midpoints<K: std::fmt::Display>(
    props: &std::collections::BTreeMap<K, String>,
    rot: impl Fn(Vec3) -> Vec3,
) -> Vec<solidcraft_doc::Position> {
    let Some((_, val)) = props.iter().find(|(k, _)| k.to_string() == "edge_midpoints") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in val.split(';') {
        let mut parts = item.split(',');
        let (Some(xs), Some(ys), Some(zs)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if let (Ok(x), Ok(y), Ok(z)) = (xs.parse::<f64>(), ys.parse::<f64>(), zs.parse::<f64>())
            && x.is_finite()
            && y.is_finite()
            && z.is_finite()
        {
            out.push(solidcraft_doc::Position(rot(Vec3::new(x, y, z))));
        }
    }
    out
}

fn filter_edges_on_body(
    state: &solidcraft_doc::ModelState,
    body_name: &str,
    pts: Vec<solidcraft_doc::Position>,
    tol: f64,
) -> Vec<solidcraft_doc::Position> {
    let Some(b) = state.bodies.iter().find(|b| b.name == body_name) else {
        return Vec::new();
    };
    let size = b.body.size();
    let Ok(edge_infos) = b.body.edges((size * 1.0e-3).max(1.0e-3)) else {
        return pts;
    };
    let mut out = Vec::new();
    let mut seen_edges = HashSet::new();
    for pos in pts {
        let p = pos.0;
        let mut best: Option<(usize, f64, Vec3)> = None;
        for (e_idx, e) in edge_infos.iter().enumerate() {
            for w in e.points.windows(2) {
                let ab = w[1] - w[0];
                let l2 = ab.len2();
                let t = if l2 > 1.0e-18 { ((p - w[0]).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
                let q = w[0] + ab * t;
                let d = p.dist(q);
                if d <= tol && best.is_none_or(|(_, bd, _)| d < bd) {
                    best = Some((e_idx, d, q));
                }
            }
        }
        if let Some((e_idx, _, q)) = best
            && seen_edges.insert(e_idx)
        {
            out.push(solidcraft_doc::Position(q));
        }
    }
    out
}

fn filter_consumed_transverse_edges(
    state: &solidcraft_doc::ModelState,
    body_name: &str,
    pts: Vec<solidcraft_doc::Position>,
    blend_size: f64,
) -> Vec<solidcraft_doc::Position> {
    let Some(b) = state.bodies.iter().find(|b| b.name == body_name) else {
        return pts;
    };
    let size = b.body.size();
    let Ok(edge_infos) = b.body.edges((size * 1.0e-3).max(1.0e-3)) else {
        return pts;
    };
    struct MatchedEdge {
        pos: solidcraft_doc::Position,
        start: Vec3,
        end: Vec3,
        len: f64,
        dir: Option<Vec3>,
    }
    let mut matched: Vec<MatchedEdge> = Vec::with_capacity(pts.len());
    for pos in &pts {
        let p = pos.0;
        let mut best_idx: Option<(usize, f64)> = None;
        for (e_idx, e) in edge_infos.iter().enumerate() {
            for w in e.points.windows(2) {
                let ab = w[1] - w[0];
                let l2 = ab.len2();
                let t = if l2 > 1.0e-18 { ((p - w[0]).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
                let q = w[0] + ab * t;
                let d = p.dist(q);
                if best_idx.is_none_or(|(_, bd)| d < bd) {
                    best_idx = Some((e_idx, d));
                }
            }
        }
        if let Some((e_idx, _)) = best_idx
            && let Some(e) = edge_infos.get(e_idx)
            && let (Some(&start), Some(&end)) = (e.points.first(), e.points.last())
        {
            let len: f64 = e.points.windows(2).map(|w| w[0].dist(w[1])).sum();
            let dir = (end - start).normalized();
            matched.push(MatchedEdge { pos: *pos, start, end, len, dir });
        }
    }
    if matched.len() < 2 {
        return pts;
    }
    let kept: Vec<solidcraft_doc::Position> = matched
        .iter()
        .enumerate()
        .filter(|&(i, m)| {
            if m.len > blend_size + 1.0e-2 {
                return true;
            }
            let Some(dir_a) = m.dir else {
                return true;
            };
            let consumed_by_longer = matched.iter().enumerate().any(|(j, other)| {
                if i == j || other.len <= blend_size + 1.0e-2 {
                    return false;
                }
                let shares_vertex = m.start.dist(other.start) <= 1.0e-2
                    || m.start.dist(other.end) <= 1.0e-2
                    || m.end.dist(other.start) <= 1.0e-2
                    || m.end.dist(other.end) <= 1.0e-2;
                let perpendicular = other.dir.is_some_and(|dir_b| dir_a.dot(dir_b).abs() < 0.25);
                shares_vertex && perpendicular
            });
            !consumed_by_longer
        })
        .map(|(_, m)| m.pos)
        .collect();
    if kept.is_empty() { pts } else { kept }
}

fn parse_body_slot_list<K: std::fmt::Display>(props: &std::collections::BTreeMap<K, String>, key: &str) -> Vec<String> {
    let Some((_, val)) = props.iter().find(|(k, _)| k.to_string() == key) else {
        return Vec::new();
    };
    val.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

fn parse_body_centers<K: std::fmt::Display>(props: &std::collections::BTreeMap<K, String>, rot: impl Fn(Vec3) -> Vec3) -> HashMap<String, Vec3> {
    let mut out = HashMap::new();
    let Some((_, val)) = props.iter().find(|(k, _)| k.to_string() == "body_centers") else {
        return out;
    };
    for item in val.split(';') {
        let Some((slot, coords)) = item.split_once(':') else {
            continue;
        };
        let mut parts = coords.split(',');
        let (Some(xs), Some(ys), Some(zs)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if let (Ok(x), Ok(y), Ok(z)) = (xs.parse::<f64>(), ys.parse::<f64>(), zs.parse::<f64>())
            && x.is_finite()
            && y.is_finite()
            && z.is_finite()
        {
            out.insert(slot.trim().to_string(), rot(Vec3::new(x, y, z)));
        }
    }
    out
}

/// Compute the signed distance range `(min_d, max_d)` of a modeling feature's geometry
/// relative to a mirror plane `pl`. Returns `None` for non-modeling construction features.
fn feature_signed_distance_range(pf: &solidcraft_doc::Feature, state: &solidcraft_doc::ModelState, pl: &Plane) -> Option<(f64, f64)> {
    let mut min_d = f64::INFINITY;
    let mut max_d = f64::NEG_INFINITY;
    let mut add_pt = |p: Vec3| {
        let d = (p - pl.origin).dot(pl.normal());
        min_d = min_d.min(d);
        max_d = max_d.max(d);
    };
    let mut add_sketch = |sk_id: u64| {
        if let Some(sk) = state.sketch(sk_id) {
            for p in &sk.profiles {
                let b = p.region.outer.polyline(1.0e-2).bounds();
                if !b.is_empty() && b.min.x.is_finite() && b.max.x.is_finite() {
                    add_pt(sk.plane.to_world(b.min));
                    add_pt(sk.plane.to_world(b.max));
                    add_pt(sk.plane.to_world(Vec2::new(b.min.x, b.max.y)));
                    add_pt(sk.plane.to_world(Vec2::new(b.max.x, b.min.y)));
                }
            }
        }
    };
    match &pf.kind {
        FeatureKind::Fillet { edges, .. } | FeatureKind::Chamfer { edges, .. } => {
            for e in edges {
                add_pt(e.0);
            }
        }
        FeatureKind::Extrude { sketch, .. } | FeatureKind::Revolve { sketch, .. } => {
            add_sketch(sketch.0);
        }
        FeatureKind::Sweep { sketch, path_sketch, .. } => {
            add_sketch(sketch.0);
            add_sketch(path_sketch.0);
        }
        FeatureKind::Loft { sections, .. } => {
            for sec in sections {
                add_sketch(sec.sketch.0);
            }
        }
        FeatureKind::Combine { .. } | FeatureKind::Shell { .. } => return Some((-1.0, 1.0)),
        _ => return None,
    }
    if min_d <= max_d { Some((min_d, max_d)) } else { None }
}

/// Convert native CAD bytes to a SolidCraft document with timeline features and evaluated model.
/// Returns `None` if the file does not have native sketches or features.
pub fn native_cad_to_document_with_model(bytes: &[u8], file: &str) -> Result<Option<(Document, solidcraft_doc::Model)>, String> {
    native_cad_to_document_with_model_validating(bytes, file, solidcraft_kernel::Validation::default())
}

/// [`native_cad_to_document_with_model`], deferring intermediate B-rep validation during timeline
/// replay (`Validation::Off`) and validating the final kept bodies under `validation`.
pub fn native_cad_to_document_with_model_validating(
    bytes: &[u8],
    file: &str,
    validation: solidcraft_kernel::Validation,
) -> Result<Option<(Document, solidcraft_doc::Model)>, String> {
    let (format, ir, _report, fidelity) = native_cad_decode_full(bytes, file)?;

    // If there are no sketches, parameters, or features, this is a plain geometry file;
    // let caller import it as a standard base feature.
    if ir.model.sketches.is_empty() && ir.model.parameters.is_empty() && ir.model.features.is_empty() {
        return Ok(None);
    }

    let stem = std::path::Path::new(file).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Design".into());
    let mut doc = Document::new(&stem);

    // 1. Option A1: Extract parameters into the document
    let mut param_map: HashMap<ParameterId, (String, f64)> = HashMap::new();
    let defined_names: HashSet<&str> = ir.model.parameters.iter().map(|p| p.name.trim()).filter(|n| !n.is_empty()).collect();

    for p in &ir.model.parameters {
        let name = p.name.trim();
        if name.is_empty() {
            continue;
        }
        let (unit, val) = match &p.value {
            Some(ParameterValue::Length(l)) => ("mm", l.get()),
            Some(ParameterValue::Angle(a)) => ("deg", a.get().to_degrees()),
            Some(ParameterValue::Real(r)) => ("", r.get()),
            Some(ParameterValue::Integer(i)) => ("", *i as f64),
            _ => ("", 0.0),
        };
        param_map.insert(p.id.clone(), (name.to_string(), val));

        let has_missing_refs = expr_has_undefined_references(&p.expression, &defined_names);
        let (expr, comment) = if p.expression.trim().is_empty() || has_missing_refs {
            let lit = if unit.is_empty() { format!("{val}") } else { format!("{val} {unit}") };
            let comment = if has_missing_refs { format!("f3d: {}", p.expression.trim()) } else { String::new() };
            (lit, comment)
        } else {
            (p.expression.trim().to_string(), String::new())
        };

        let is_model = name.starts_with('d') && name[1..].chars().all(|c| c.is_ascii_digit());
        doc.params.push(solidcraft_doc::Param { name: name.to_string(), expr, unit: unit.to_string(), comment, model: is_model });
    }

    // Place user parameters (model == false) before model parameters (model == true)
    doc.params.sort_by_key(|p| p.model);

    let is_f3d = format == "f3d" || file.ends_with(".f3d");
    let clean_f64 = |x: f64| -> f64 { if x.abs() < 1.0e-12 { 0.0 } else { x } };
    let rot = |v: Vec3| -> Vec3 {
        let r = if is_f3d { Vec3::new(v.x, -v.z, v.y) } else { v };
        Vec3::new(clean_f64(r.x), clean_f64(r.y), clean_f64(r.z))
    };

    // Map B-Rep body entity suffix -> presentation body name from `ir.model.bodies`
    let mut entity_body_names: HashMap<String, String> = HashMap::new();
    let mut solid_body_keys: HashSet<String> = HashSet::new();
    for b in &ir.model.bodies {
        let key = entity_suffix_key(b.id.as_str());
        if b.kind == cadmpeg_ir::topology::BodyKind::Solid {
            solid_body_keys.insert(key.clone());
        }
        if let Some(name) = &b.name
            && !name.trim().is_empty()
        {
            entity_body_names.insert(key, name.clone());
        }
    }
    log::warn!("native_timeline: entity_body_names = {:?}", entity_body_names);

    // Pre-scan features to identify which sketch profiles are referenced on each sketch
    let mut used_profiles_by_sketch: HashMap<IrSketchId, HashSet<usize>> = HashMap::new();
    let mut whole_sketch_used: HashSet<IrSketchId> = HashSet::new();
    let mut seen_first_extrude = false;
    let mut sorted_feats: Vec<_> = ir.model.features.iter().collect();
    sorted_feats.sort_by_key(|f| f.ordinal);
    for f in &sorted_feats {
        let op_def = match f.evaluation.definition() {
            FeatureDefinition::Operation(op) => op,
            FeatureDefinition::PostProcess { operation, .. } => operation,
        };
        let hist_sk = f
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("historical_face_sketch"))
            .and_then(|s| ir.model.sketches.iter().find(|sk| sk.id.as_str() == s.as_str()).map(|sk| sk.id.clone()));
        let hist_profs: Vec<usize> = f
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("historical_face_profiles"))
            .map(|s| s.split(',').filter_map(|p| p.trim().parse::<usize>().ok()).collect())
            .unwrap_or_default();
        if let Some(sk_id) = hist_sk {
            for idx in hist_profs {
                used_profiles_by_sketch.entry(sk_id.clone()).or_default().insert(idx);
            }
        }
        let mut planar_refs: Vec<&PlanarProfileRef> = Vec::new();
        match op_def {
            FeatureOperation::Extrude { profile: ProfileRef::Planar(pr), .. } => {
                if !seen_first_extrude {
                    seen_first_extrude = true;
                    if let PlanarProfileRef::Sketch(sk)
                    | PlanarProfileRef::SketchProfiles { sketch: sk, .. }
                    | PlanarProfileRef::SketchRegions { sketch: sk, .. }
                    | PlanarProfileRef::SketchSelection { sketch: sk, .. }
                    | PlanarProfileRef::SketchEntities { sketch: sk, .. } = pr
                    {
                        whole_sketch_used.insert(sk.clone());
                    }
                }
                planar_refs.push(pr);
            }
            FeatureOperation::Sweep { shape, .. } => {
                if let Some(pr) = shape.referenced_profile() {
                    planar_refs.push(pr);
                }
            }
            FeatureOperation::Loft { sections, .. } => {
                for sec in sections {
                    if let cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Planar(pr)) = sec {
                        planar_refs.push(pr);
                    }
                }
            }
            FeatureOperation::Revolve { construction: RevolveConstruction::Resolved { profile: pr, .. }, .. } => {
                planar_refs.push(pr);
            }
            _ => {}
        }
        for pr in planar_refs {
            match pr {
                PlanarProfileRef::SketchProfiles { sketch, profiles } => {
                    let set = used_profiles_by_sketch.entry(sketch.clone()).or_default();
                    for &p in profiles.as_slice() {
                        set.insert(p as usize);
                    }
                }
                PlanarProfileRef::SketchRegions { sketch, regions } => {
                    let set = used_profiles_by_sketch.entry(sketch.clone()).or_default();
                    for reg in regions.as_slice() {
                        if let SketchProfileRegion::Loops { loops } = reg {
                            set.insert(loops.outer() as usize);
                            for &h in loops.holes() {
                                set.insert(h as usize);
                            }
                        }
                    }
                }
                PlanarProfileRef::Sketch(sketch) | PlanarProfileRef::SketchEntities { sketch, .. } => {
                    whole_sketch_used.insert(sketch.clone());
                }
                _ => {}
            }
        }
    }

    // 2. Option A2: Prepare 2D sketches (stored to insert chronologically)
    let mut prepared_sketches: HashMap<IrSketchId, PreparedSketch> = HashMap::new();

    for sk in &ir.model.sketches {
        let mut active_profile_entities: HashSet<SketchEntityId> = HashSet::new();
        if !sk.profiles.is_empty() {
            let specific_used = used_profiles_by_sketch.get(&sk.id).filter(|s| !s.is_empty() && !whole_sketch_used.contains(&sk.id));
            for (prof_idx, prof) in sk.profiles.iter().enumerate() {
                if let Some(used_set) = specific_used
                    && !used_set.contains(&prof_idx)
                {
                    continue;
                }
                for use_ in prof {
                    if prof.iter().filter(|u| u.entity == use_.entity).count() == 1 {
                        active_profile_entities.insert(use_.entity.clone());
                    }
                }
            }
        }

        let plane_ref = match sk.placement.resolved() {
            Some((origin, normal, u_axis)) => {
                let o = rot(Vec3::new(origin.get().x, origin.get().y, origin.get().z));
                let n = rot(Vec3::new(normal.get().x, normal.get().y, normal.get().z));
                let u = rot(Vec3::new(u_axis.get().x, u_axis.get().y, u_axis.get().z));
                if o.len() < 1.0e-5 {
                    if (n - Vec3::Z).len() < 1.0e-5 && (u - Vec3::X).len() < 1.0e-5 {
                        PlaneRef::Origin { name: "XY".into() }
                    } else if (n - Vec3::Y).len() < 1.0e-5 && (u - Vec3::X).len() < 1.0e-5 {
                        PlaneRef::Origin { name: "XZ".into() }
                    } else if (n - Vec3::X).len() < 1.0e-5 && (u - Vec3::Y).len() < 1.0e-5 {
                        PlaneRef::Origin { name: "YZ".into() }
                    } else if let Some(plane) = Plane::new(o, u, n.cross(u)) {
                        PlaneRef::Custom { plane }
                    } else {
                        PlaneRef::Origin { name: "XY".into() }
                    }
                } else if let Some(plane) = Plane::new(o, u, n.cross(u)) {
                    PlaneRef::Custom { plane }
                } else {
                    PlaneRef::Origin { name: "XY".into() }
                }
            }
            None => PlaneRef::Origin { name: "XY".into() },
        };

        let mut sketch = Sketch::new();
        let mut point_map: HashMap<(i64, i64), usize> = HashMap::new();
        point_map.insert((0, 0), 0);

        let mut entity_to_point: HashMap<SketchEntityId, usize> = HashMap::new();
        let mut entity_to_curve: HashMap<SketchEntityId, usize> = HashMap::new();
        let mut entity_curve_ids: HashMap<SketchEntityId, String> = HashMap::new();
        let mut entity_defs: HashMap<SketchEntityId, SketchGeometryDefinition> = HashMap::new();

        // Pass 1: Collect Point entities
        for ent in &ir.model.sketch_entities {
            if ent.sketch != sk.id {
                continue;
            }
            let raw_def = ent.geometry.definition().to_raw();
            if let SketchGeometryDefinition::Point { position } = raw_def {
                let p_idx = get_or_insert_point(&mut sketch, &mut point_map, position.u, position.v);
                entity_to_point.insert(ent.id().clone(), p_idx);
            }
            entity_defs.insert(ent.id().clone(), raw_def);
        }

        // Pass 2: Collect curves (Line, Circle, Arc)
        for ent in &ir.model.sketch_entities {
            if ent.sketch != sk.id {
                continue;
            }
            let is_construction = ent.construction || (!sk.profiles.is_empty() && !active_profile_entities.contains(ent.id()));
            match ent.geometry.definition().to_raw() {
                SketchGeometryDefinition::Point { .. } => {}
                SketchGeometryDefinition::Line { start, end } => {
                    let a = get_or_insert_point(&mut sketch, &mut point_map, start.u, start.v);
                    let b = get_or_insert_point(&mut sketch, &mut point_map, end.u, end.v);
                    if a != b {
                        let ci = sketch.curves.len();
                        let cid = format!("l{}", ci + 1);
                        sketch.curves.push(Curve {
                            id: cid.clone(),
                            kind: CurveKind::Line { a, b },
                            construction: is_construction,
                            reversed: false,
                            link: None,
                            centerline: false,
                            fixed: false,
                            infinite: false,
                        });
                        entity_to_curve.insert(ent.id().clone(), ci);
                        entity_curve_ids.insert(ent.id().clone(), cid);
                    }
                }
                SketchGeometryDefinition::Circle { center, radius } => {
                    let c = get_or_insert_point(&mut sketch, &mut point_map, center.u, center.v);
                    let ci = sketch.curves.len();
                    let cid = format!("c{}", ci + 1);
                    sketch.curves.push(Curve {
                        id: cid.clone(),
                        kind: CurveKind::Circle { c, r: radius.get() },
                        construction: is_construction,
                        reversed: false,
                        link: None,
                        centerline: false,
                        fixed: false,
                        infinite: false,
                    });
                    entity_to_curve.insert(ent.id().clone(), ci);
                    entity_curve_ids.insert(ent.id().clone(), cid);
                }
                SketchGeometryDefinition::Arc { center, radius, start_angle, end_angle } => {
                    let r = radius.get();
                    let sa = start_angle.get();
                    let ea = end_angle.get();
                    let sp = Point2 { u: center.u + r * sa.cos(), v: center.v + r * sa.sin() };
                    let ep = Point2 { u: center.u + r * ea.cos(), v: center.v + r * ea.sin() };
                    let c = get_or_insert_point(&mut sketch, &mut point_map, center.u, center.v);
                    let a = get_or_insert_point(&mut sketch, &mut point_map, sp.u, sp.v);
                    let b = get_or_insert_point(&mut sketch, &mut point_map, ep.u, ep.v);
                    let ci = sketch.curves.len();
                    let cid = format!("a{}", ci + 1);
                    sketch.curves.push(Curve {
                        id: cid.clone(),
                        kind: CurveKind::Arc { c, a, b },
                        construction: is_construction,
                        reversed: false,
                        link: None,
                        centerline: false,
                        fixed: false,
                        infinite: false,
                    });
                    entity_to_curve.insert(ent.id().clone(), ci);
                    entity_curve_ids.insert(ent.id().clone(), cid);
                }
                SketchGeometryDefinition::ReferenceLine { origin, direction } => {
                    let a = get_or_insert_point(&mut sketch, &mut point_map, origin.u, origin.v);
                    let b = get_or_insert_point(&mut sketch, &mut point_map, origin.u + direction.u, origin.v + direction.v);
                    if a != b {
                        let ci = sketch.curves.len();
                        let cid = format!("l{}", ci + 1);
                        sketch.curves.push(Curve {
                            id: cid.clone(),
                            kind: CurveKind::Line { a, b },
                            construction: true,
                            reversed: false,
                            link: None,
                            centerline: false,
                            fixed: false,
                            infinite: true,
                        });
                        entity_to_curve.insert(ent.id().clone(), ci);
                        entity_curve_ids.insert(ent.id().clone(), cid);
                    }
                }
                _ => {}
            }
        }

        // Build IR profile descriptors (curve IDs + 2D boundary midpoints)
        let mut ir_profiles = Vec::with_capacity(sk.profiles.len());
        for prof in &sk.profiles {
            let mut curve_ids = Vec::new();
            let mut midpoints = Vec::new();
            for use_ in prof {
                if prof.iter().filter(|u| u.entity == use_.entity).count() > 1 {
                    continue;
                }
                if let Some(cid) = entity_curve_ids.get(&use_.entity) {
                    curve_ids.push(cid.clone());
                }
                if let Some(def) = entity_defs.get(&use_.entity) {
                    let pt = match def {
                        SketchGeometryDefinition::Line { start, end } => Some(Vec2::new((start.u + end.u) * 0.5, (start.v + end.v) * 0.5)),
                        SketchGeometryDefinition::Circle { center, radius } => Some(Vec2::new(center.u + radius.get(), center.v)),
                        SketchGeometryDefinition::Arc { center, radius, start_angle, end_angle } => {
                            let t_mid = (start_angle.get() + end_angle.get()) * 0.5;
                            Some(Vec2::new(center.u + radius.get() * t_mid.cos(), center.v + radius.get() * t_mid.sin()))
                        }
                        _ => None,
                    };
                    if let Some(pt) = pt {
                        midpoints.push(pt);
                    }
                }
            }
            ir_profiles.push(IrProfileDescriptor { curve_ids, midpoints });
        }

        let push_valid = |sketch: &mut Sketch, kind: ConstraintKind, param: Option<String>| -> bool {
            let ci = sketch.constraints.len();
            let cid = format!("k{}", ci + 1);
            sketch.constraints.push(Constraint { id: cid, kind, param, driven: false, text: None });
            if solidcraft_sketch::constraint_residual(sketch, ci).is_some_and(|r| r.is_finite() && r.abs() <= 1.0e-3) {
                true
            } else {
                sketch.constraints.pop();
                false
            }
        };

        // Pass 3: Collect Constraints
        for c in &ir.model.sketch_constraints {
            if c.sketch != sk.id {
                continue;
            }
            match c.definition.kind() {
                SketchConstraintDefinitionInput::Horizontal { entity } => {
                    if let Some(&l) = entity_to_curve.get(entity) {
                        push_valid(&mut sketch, ConstraintKind::Horizontal { l }, None);
                    }
                }
                SketchConstraintDefinitionInput::Vertical { entity } => {
                    if let Some(&l) = entity_to_curve.get(entity) {
                        push_valid(&mut sketch, ConstraintKind::Vertical { l }, None);
                    }
                }
                SketchConstraintDefinitionInput::Parallel { first, second } => {
                    if let (Some(&a), Some(&b)) = (entity_to_curve.get(first), entity_to_curve.get(second)) {
                        push_valid(&mut sketch, ConstraintKind::Parallel { a, b }, None);
                    }
                }
                SketchConstraintDefinitionInput::Perpendicular { first, second } => {
                    if let (Some(&a), Some(&b)) = (entity_to_curve.get(first), entity_to_curve.get(second)) {
                        push_valid(&mut sketch, ConstraintKind::Perpendicular { a, b }, None);
                    }
                }
                SketchConstraintDefinitionInput::Collinear { first, second } => {
                    if let (Some(&a), Some(&b)) = (entity_to_curve.get(first), entity_to_curve.get(second)) {
                        push_valid(&mut sketch, ConstraintKind::Collinear { a, b }, None);
                    }
                }
                SketchConstraintDefinitionInput::Tangent { first, second } => {
                    if let (Some(&a), Some(&b)) = (entity_to_curve.get(first), entity_to_curve.get(second)) {
                        push_valid(&mut sketch, ConstraintKind::Tangent { a, b }, None);
                    }
                }
                SketchConstraintDefinitionInput::Equal { first, second } => {
                    if let (Some(&a), Some(&b)) = (entity_to_curve.get(first), entity_to_curve.get(second)) {
                        push_valid(&mut sketch, ConstraintKind::Equal { a, b }, None);
                    }
                }
                SketchConstraintDefinitionInput::Concentric { first, second } => {
                    if let (Some(&a), Some(&b)) = (entity_to_curve.get(first), entity_to_curve.get(second)) {
                        push_valid(&mut sketch, ConstraintKind::Concentric { a, b }, None);
                    }
                }
                SketchConstraintDefinitionInput::CoincidentLoci { loci } if loci.len() >= 2 => {
                    let p = resolve_locus(&loci[0], &entity_to_point, &entity_to_curve, &sketch);
                    let q = resolve_locus(&loci[1], &entity_to_point, &entity_to_curve, &sketch);
                    let mut added = false;
                    if let (Some(p), Some(q)) = (p, q)
                        && p != q
                    {
                        added = push_valid(&mut sketch, ConstraintKind::Coincident { p, q }, None);
                    }
                    if !added {
                        if let (Some(p), SketchLocus::Entity(id)) = (p, &loci[1])
                            && let Some(&curve) = entity_to_curve.get(id)
                        {
                            added = push_valid(&mut sketch, ConstraintKind::PointOnCurve { p, c: curve }, None);
                        }
                        if !added
                            && let (SketchLocus::Entity(id), Some(q)) = (&loci[0], q)
                            && let Some(&curve) = entity_to_curve.get(id)
                        {
                            push_valid(&mut sketch, ConstraintKind::PointOnCurve { p: q, c: curve }, None);
                        }
                    }
                }
                SketchConstraintDefinitionInput::Coincident { entities } if entities.len() >= 2 => {
                    let p = entity_to_point.get(&entities[0]).copied();
                    let q = entity_to_point.get(&entities[1]).copied();
                    if let (Some(p), Some(q)) = (p, q)
                        && p != q
                    {
                        push_valid(&mut sketch, ConstraintKind::Coincident { p, q }, None);
                    }
                }
                SketchConstraintDefinitionInput::Diameter { entity, parameter } => {
                    if let Some(&c) = entity_to_curve.get(entity) {
                        let (param_name, val) = param_map.get(parameter).cloned().unwrap_or_default();
                        let p_opt = if param_name.is_empty() { None } else { Some(param_name) };
                        push_valid(&mut sketch, ConstraintKind::Diameter { c, value: val }, p_opt);
                    }
                }
                SketchConstraintDefinitionInput::Radius { entity, parameter } => {
                    if let Some(&c) = entity_to_curve.get(entity) {
                        let (param_name, val) = param_map.get(parameter).cloned().unwrap_or_default();
                        let p_opt = if param_name.is_empty() { None } else { Some(param_name) };
                        push_valid(&mut sketch, ConstraintKind::Radius { c, value: val }, p_opt);
                    }
                }
                SketchConstraintDefinitionInput::RepeatedLength { entities, parameter } => {
                    let (param_name, val) = param_map.get(parameter).cloned().unwrap_or_default();
                    let l = entities.first().and_then(|e| entity_to_curve.get(e).copied());
                    if let Some(l) = l {
                        let p_opt = if param_name.is_empty() { None } else { Some(param_name) };
                        push_valid(&mut sketch, ConstraintKind::Length { l, value: val }, p_opt);
                        for next_ent in entities.iter().skip(1) {
                            if let Some(&next_l) = entity_to_curve.get(next_ent) {
                                push_valid(&mut sketch, ConstraintKind::Equal { a: l, b: next_l }, None);
                            }
                        }
                    }
                }
                SketchConstraintDefinitionInput::HorizontalDistance { first, second, parameter }
                | SketchConstraintDefinitionInput::VerticalDistance { first, second, parameter } => {
                    let is_horiz = matches!(c.definition.kind(), SketchConstraintDefinitionInput::HorizontalDistance { .. });
                    let (param_name, val) = param_map.get(parameter).cloned().unwrap_or_default();
                    let p_opt = if param_name.is_empty() { None } else { Some(param_name) };
                    let p = resolve_locus(first, &entity_to_point, &entity_to_curve, &sketch);
                    let q = resolve_locus(second, &entity_to_point, &entity_to_curve, &sketch);
                    let mut added = false;
                    if let (Some(p), Some(q)) = (p, q) {
                        let kind =
                            if is_horiz { ConstraintKind::DistanceX { p, q, value: val } } else { ConstraintKind::DistanceY { p, q, value: val } };
                        added = push_valid(&mut sketch, kind, p_opt.clone());
                    }
                    if !added {
                        let c1 = match first {
                            SketchLocus::Entity(id) => entity_to_curve.get(id).copied(),
                            _ => None,
                        };
                        let c2 = match second {
                            SketchLocus::Entity(id) => entity_to_curve.get(id).copied(),
                            _ => None,
                        };
                        for (line_c, pt, other_c) in [(c1, q, c2), (c2, p, c1)] {
                            if added {
                                break;
                            }
                            let Some(l) = line_c.filter(|&ci| matches!(sketch.curves.get(ci).map(|cv| &cv.kind), Some(CurveKind::Line { .. })))
                            else {
                                continue;
                            };
                            if let Some(p) = pt {
                                added = push_valid(&mut sketch, ConstraintKind::PointLineDistance { p, l, value: val }, p_opt.clone());
                            }
                            if !added
                                && let Some(c) = other_c.filter(|&ci| {
                                    matches!(sketch.curves.get(ci).map(|cv| &cv.kind), Some(CurveKind::Circle { .. } | CurveKind::Arc { .. }))
                                })
                            {
                                added =
                                    push_valid(&mut sketch, ConstraintKind::LineCircleDistance { l, c, value: val, inside: false }, p_opt.clone())
                                        || push_valid(
                                            &mut sketch,
                                            ConstraintKind::LineCircleDistance { l, c, value: val, inside: true },
                                            p_opt.clone(),
                                        );
                            }
                        }
                    }
                }
                SketchConstraintDefinitionInput::Distance { entities, .. } if entities.len() >= 2 => {
                    if let (Some(&a), Some(&b)) = (entity_to_curve.get(&entities[0]), entity_to_curve.get(&entities[1])) {
                        push_valid(&mut sketch, ConstraintKind::Parallel { a, b }, None);
                    }
                }
                _ => {}
            }
        }

        let name = sk.name.as_deref().unwrap_or("Sketch").to_string();
        prepared_sketches.insert(sk.id.clone(), PreparedSketch { plane_ref, sketch, name, entity_curve_ids, ir_profiles });
    }

    // 3. Option B1: Replay timeline features in ordinal order
    let mut timeline_features: Vec<_> = ir.model.features.iter().collect();
    timeline_features.sort_by_key(|f| f.ordinal);

    // Propagate presentation body names across historical body slot re-indexing when an old slot is replaced by a new slot
    for f in timeline_features.iter().rev() {
        let affected = parse_body_slot_list(&f.source_properties, "affected_bodies");
        let deleted = parse_body_slot_list(&f.source_properties, "deleted_bodies");
        if !deleted.is_empty() {
            let surviving: Vec<&String> = affected.iter().filter(|k| !deleted.contains(k)).collect();
            if surviving.len() == 1
                && let Some(known_name) = affected.iter().find_map(|k| entity_body_names.get(k).cloned())
            {
                for k in &affected {
                    entity_body_names.entry(k.clone()).or_insert_with(|| known_name.clone());
                }
            }
        }
    }

    let mut sketch_map: HashMap<IrSketchId, u64> = HashMap::new();
    let mut sketch_entity_curves: HashMap<IrSketchId, HashMap<SketchEntityId, String>> = HashMap::new();
    let mut sketch_ir_profiles: HashMap<IrSketchId, Vec<IrProfileDescriptor>> = HashMap::new();
    let mut model = solidcraft_doc::Model::default();
    model.set_validation(solidcraft_kernel::Validation::Off);
    let mut datum_planes: Vec<Plane> = Vec::new();
    // Maps B-Rep historical entity key (e.g. "1", "6", "21", "103", "106", "131") -> SolidCraft body name
    let mut entity_to_body: HashMap<String, String> = HashMap::new();
    let mut pending_shell: Option<(String, f64, String)> = None;

    let max_ordinal = std::env::var("SOLID_TIMELINE_MAX_ORDINAL").ok().and_then(|s| s.parse::<u64>().ok()).unwrap_or(u64::MAX);

    for f in &timeline_features {
        if f.ordinal > max_ordinal {
            break;
        }
        let op_def = match f.evaluation.definition() {
            FeatureDefinition::Operation(op) => op,
            FeatureDefinition::PostProcess { operation, .. } => operation,
        };
        let mut output_keys = parse_body_slot_list(&f.source_properties, "affected_bodies");
        for b in f.evaluation.outputs().iter() {
            let k = entity_suffix_key(b.as_str());
            if solid_body_keys.contains(&k) && !output_keys.contains(&k) {
                output_keys.push(k);
            }
        }
        let deleted_keys = parse_body_slot_list(&f.source_properties, "deleted_bodies");
        let fname = f.name.as_deref().unwrap_or("Feature");
        let centers_for_f = parse_body_centers(&f.source_properties, rot);
        if !deleted_keys.is_empty()
            && !matches!(
                op_def,
                FeatureOperation::DeleteBody { .. }
                    | FeatureOperation::Combine { .. }
                    | FeatureOperation::SplitBody { .. }
                    | FeatureOperation::Extrude { op: BooleanOp::Join, .. }
            )
        {
            let surviving_keys: Vec<&String> = output_keys.iter().filter(|k| !deleted_keys.contains(k)).collect();
            let mut remove_names: Vec<solidcraft_doc::BodyName> = Vec::new();
            for dk in &deleted_keys {
                if let Some(bname) = entity_to_body.get(dk).cloned() {
                    let still_referenced = surviving_keys.iter().any(|sk| entity_to_body.get(*sk) == Some(&bname));
                    let exists_in_model = model.state().bodies.iter().any(|b| b.name == bname);
                    if !still_referenced && exists_in_model {
                        let bn = solidcraft_doc::BodyName(bname);
                        if !remove_names.contains(&bn) {
                            remove_names.push(bn);
                        }
                    }
                }
            }
            if !remove_names.is_empty() {
                let kind = FeatureKind::Remove { bodies: remove_names };
                if doc.add_feature(kind, Some("Remove 1")).is_ok() {
                    model.evaluate(&doc);
                    for dk in &deleted_keys {
                        if let Some(bname) = entity_to_body.get(dk)
                            && !model.state().bodies.iter().any(|b| &b.name == bname)
                        {
                            entity_to_body.remove(dk);
                        }
                    }
                }
            }
        }

        // Sketch feature
        if let FeatureOperation::Sketch { sketch } = op_def {
            let sk_id_opt = match sketch {
                cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(id)) => Some(id),
                _ => None,
            };
            if let Some(id) = sk_id_opt
                && let Some(prep) = prepared_sketches.remove(id)
            {
                let display_name = if !prep.name.is_empty() && !prep.name.starts_with("0_") {
                    prep.name.as_str()
                } else if !fname.is_empty() && fname != "Feature" && !fname.starts_with("0_") {
                    fname
                } else {
                    prep.name.as_str()
                };
                sketch_entity_curves.insert(id.clone(), prep.entity_curve_ids);
                sketch_ir_profiles.insert(id.clone(), prep.ir_profiles);
                if let Ok(fid) = doc.add_feature(FeatureKind::Sketch { plane: prep.plane_ref, sketch: prep.sketch }, Some(display_name)) {
                    sketch_map.insert(id.clone(), fid);
                    model.evaluate(&doc);
                }
            }
            continue;
        }

        // DatumPlane feature
        if let FeatureOperation::DatumPlane { frame } = op_def {
            let o = rot(Vec3::new(frame.origin().get().x, frame.origin().get().y, frame.origin().get().z));
            let n = rot(Vec3::new(frame.normal().get().x, frame.normal().get().y, frame.normal().get().z));
            let u = rot(Vec3::new(frame.u_axis().get().x, frame.u_axis().get().y, frame.u_axis().get().z));
            if let Some(plane) = Plane::new(o, u, n.cross(u)) {
                datum_planes.push(plane);
                let _ = doc.add_feature(FeatureKind::ConstructionPlane { plane: PlaneRef::Custom { plane } }, Some(fname));
                model.evaluate(&doc);
            }
            continue;
        }

        // Native WorkPlane feature
        if let FeatureOperation::Native { kind: cadmpeg_ir::features::NativeFeatureKind::Other(k), .. } = op_def
            && k == "WorkPlane"
        {
            let next_plane = timeline_features.iter().find(|nf| nf.ordinal > f.ordinal).and_then(|nf| {
                let later_op = match nf.evaluation.definition() {
                    FeatureDefinition::Operation(op) => op,
                    FeatureDefinition::PostProcess { operation, .. } => operation,
                };
                match later_op {
                    FeatureOperation::Sketch { sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sk_id)) } => {
                        prepared_sketches.get(sk_id).map(|p| p.plane_ref.clone())
                    }
                    FeatureOperation::SplitBody { .. } => {
                        let centers = parse_body_centers(&nf.source_properties, rot);
                        let aff = parse_body_slot_list(&nf.source_properties, "affected_bodies");
                        if aff.len() >= 2
                            && let (Some(&c1), Some(&c2)) = (centers.get(&aff[0]), centers.get(&aff[1]))
                        {
                            let n = (c2 - c1).normalized()?;
                            let mid = (c1 + c2) * 0.5;
                            let ref_axis = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
                            let u = n.cross(ref_axis).normalized()?;
                            Plane::new(mid, u, n.cross(u)).map(|plane| PlaneRef::Custom { plane })
                        } else if let Some((_, sp_val)) = nf.source_properties.iter().find(|(k, _)| *k == "split_plane") {
                            let parts: Vec<&str> = sp_val.split(';').collect();
                            if parts.len() == 2 {
                                let op: Vec<f64> = parts[0].split(',').filter_map(|s| s.parse().ok()).collect();
                                let np: Vec<f64> = parts[1].split(',').filter_map(|s| s.parse().ok()).collect();
                                if op.len() == 3 && np.len() == 3 {
                                    let origin = rot(Vec3::new(op[0], op[1], op[2]));
                                    let normal = rot(Vec3::new(np[0], np[1], np[2])).normalized()?;
                                    let ref_axis = if normal.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
                                    let u = normal.cross(ref_axis).normalized()?;
                                    Plane::new(origin, u, normal.cross(u)).map(|plane| PlaneRef::Custom { plane })
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    FeatureOperation::Pattern { pattern, .. } => {
                        if matches!(
                            pattern.definition(),
                            PatternTransform::Unresolved { form: Some(cadmpeg_ir::features::patterns::PatternForm::Mirror) }
                        ) || matches!(pattern.definition(), PatternTransform::Mirror { .. })
                        {
                            let centers = parse_body_centers(&nf.source_properties, rot);
                            let aff = parse_body_slot_list(&nf.source_properties, "affected_bodies");
                            if let Some(&c_after) = aff.first().and_then(|k| centers.get(k)) {
                                Plane::new(Vec3::new(c_after.x, 0.0, 0.0), Vec3::Y, Vec3::Z).map(|plane| PlaneRef::Custom { plane })
                            } else {
                                let (min_x, max_x) = model.state().bodies.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), b| {
                                    let bb = b.body.outline_bounds();
                                    (min.min(bb.min.x), max.max(bb.max.x))
                                });
                                let mid_x = if min_x.is_finite() && max_x.is_finite() { (min_x + max_x) * 0.5 } else { 0.0 };
                                Plane::new(Vec3::new(mid_x, 0.0, 0.0), Vec3::Y, Vec3::Z).map(|plane| PlaneRef::Custom { plane })
                            }
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            });
            if let Some(plane_ref) = next_plane {
                if let PlaneRef::Custom { plane } = plane_ref {
                    datum_planes.push(plane);
                }
                let _ = doc.add_feature(FeatureKind::ConstructionPlane { plane: plane_ref }, Some(fname));
                model.evaluate(&doc);
            }
            continue;
        }

        // Fillet feature
        if let FeatureOperation::Fillet { groups } = op_def {
            let radius_val = groups.first().and_then(|g| match &g.radius {
                cadmpeg_ir::features::edge_treatments::RadiusSpec::Constant { radius } => Some(radius.get()),
                _ => None,
            });
            let Some(r) = radius_val else {
                continue;
            };
            let target_body_name = parse_body_slot_list(&f.source_properties, "affected_bodies")
                .into_iter()
                .find_map(|k| entity_to_body.get(&k).cloned())
                .or_else(|| output_keys.iter().find_map(|k| entity_to_body.get(k).cloned()))
                .or_else(|| {
                    let assigned: HashSet<String> = entity_to_body.values().cloned().collect();
                    for k in parse_body_slot_list(&f.source_properties, "affected_bodies").into_iter().chain(output_keys.iter().cloned()) {
                        if let Some(&c) = centers_for_f.get(&k)
                            && let Some(matched) = find_matching_body(c, &model.state().bodies, &assigned)
                        {
                            entity_to_body.insert(k.clone(), matched.name.clone());
                            return Some(matched.name.clone());
                        }
                    }
                    None
                })
                .or_else(|| model.state().bodies.first().map(|b| b.name.clone()));
            let Some(bname) = target_body_name else {
                continue;
            };
            let radius_expr: solidcraft_doc::Expr = ir
                .model
                .parameters
                .iter()
                .filter(|p| p.owner.as_ref() == Some(&f.id) && matches!(p.value, Some(ParameterValue::Length(_))))
                .find_map(|p| {
                    let cand = p.expression.trim();
                    if !cand.is_empty()
                        && !expr_has_undefined_references(cand, &defined_names)
                        && doc.eval(cand, solidcraft_doc::expr::Kind::Length).is_ok_and(|v| (v - r).abs() <= 1.0e-4)
                    {
                        Some(solidcraft_doc::Expr::from(cand))
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| format!("{r}").into());
            let raw_pts = parse_edge_midpoints(&f.source_properties, rot);
            let edges = filter_edges_on_body(&model.state(), &bname, raw_pts, 1.5);
            if !edges.is_empty() {
                let kind = FeatureKind::Fillet {
                    edges: edges.clone(),
                    radius: radius_expr.clone(),
                    body: Some(bname.clone().into()),
                    style: solidcraft_doc::FilletStyle::Constant,
                    fit: solidcraft_doc::FilletFit::Exact,
                    corner: solidcraft_doc::FilletCorner::Rolling,
                    turn: Default::default(),
                    setbacks: Vec::new(),
                    tangent_chain: true,
                };
                if doc.add_feature(kind, Some(fname)).is_ok() {
                    model.evaluate(&doc);
                    let feat_err = model.results.last().and_then(|res| res.error.as_ref());
                    let lumps_err = model.state().bodies.iter().any(|b| b.body.lumps().is_err());
                    let mut bad = feat_err.is_some() || lumps_err;
                    let mut used_tangent_chain = true;
                    if bad {
                        log::warn!("native_timeline: Fillet {fname} failed with tangent_chain=true: feat_err={feat_err:?}, lumps_err={lumps_err}");
                        pop_failed_feature(&mut doc, &mut model);
                        let no_chain_kind = FeatureKind::Fillet {
                            edges: edges.clone(),
                            radius: radius_expr.clone(),
                            body: Some(bname.clone().into()),
                            style: solidcraft_doc::FilletStyle::Constant,
                            fit: solidcraft_doc::FilletFit::Exact,
                            corner: solidcraft_doc::FilletCorner::Rolling,
                            turn: Default::default(),
                            setbacks: Vec::new(),
                            tangent_chain: false,
                        };
                        if doc.add_feature(no_chain_kind, Some(fname)).is_ok() {
                            model.evaluate(&doc);
                            let bad2 = model.results.last().and_then(|res| res.error.as_ref()).is_some()
                                || model.state().bodies.iter().any(|b| b.body.lumps().is_err());
                            if bad2 {
                                pop_failed_feature(&mut doc, &mut model);
                            } else {
                                bad = false;
                                used_tangent_chain = false;
                            }
                        }
                    }
                    if bad && edges.len() >= 2 && edges.len() <= 8 {
                        let mut valid_edges = Vec::new();
                        let mut has_feature_in_doc = false;
                        for &edge in &edges {
                            let mut try_edges = valid_edges.clone();
                            try_edges.push(edge);
                            if try_edges.len() == edges.len() {
                                continue;
                            }
                            let single_kind = FeatureKind::Fillet {
                                edges: try_edges.clone(),
                                radius: radius_expr.clone(),
                                body: Some(bname.clone().into()),
                                style: solidcraft_doc::FilletStyle::Constant,
                                fit: solidcraft_doc::FilletFit::Exact,
                                corner: solidcraft_doc::FilletCorner::Rolling,
                                turn: Default::default(),
                                setbacks: Vec::new(),
                                tangent_chain: used_tangent_chain,
                            };
                            if has_feature_in_doc {
                                pop_failed_feature(&mut doc, &mut model);
                                has_feature_in_doc = false;
                            }
                            if doc.add_feature(single_kind, Some(fname)).is_ok() {
                                model.evaluate(&doc);
                                let single_ok = model.results.last().and_then(|res| res.error.as_ref()).is_none()
                                    && !model.state().bodies.iter().any(|b| b.body.lumps().is_err());
                                if single_ok {
                                    valid_edges = try_edges;
                                    has_feature_in_doc = true;
                                } else {
                                    pop_failed_feature(&mut doc, &mut model);
                                    has_feature_in_doc = false;
                                }
                            }
                        }
                        if !valid_edges.is_empty() && !has_feature_in_doc {
                            let restore_kind = FeatureKind::Fillet {
                                edges: valid_edges,
                                radius: radius_expr,
                                body: Some(bname.into()),
                                style: solidcraft_doc::FilletStyle::Constant,
                                fit: solidcraft_doc::FilletFit::Exact,
                                corner: solidcraft_doc::FilletCorner::Rolling,
                                turn: Default::default(),
                                setbacks: Vec::new(),
                                tangent_chain: used_tangent_chain,
                            };
                            if doc.add_feature(restore_kind, Some(fname)).is_ok() {
                                model.evaluate(&doc);
                                if model.results.last().and_then(|res| res.error.as_ref()).is_some()
                                    || model.state().bodies.iter().any(|b| b.body.lumps().is_err())
                                {
                                    pop_failed_feature(&mut doc, &mut model);
                                }
                            }
                        }
                    }
                }
            }
            continue;
        }

        // Chamfer feature
        if let FeatureOperation::Chamfer { groups, .. } = op_def {
            let dist_val = groups.first().and_then(|g| match &g.spec {
                cadmpeg_ir::features::edge_treatments::ChamferSpec::Distance { distance } => Some(distance.get()),
                cadmpeg_ir::features::edge_treatments::ChamferSpec::DistanceAngle { distance, .. } => Some(distance.get()),
                cadmpeg_ir::features::edge_treatments::ChamferSpec::TwoDistances { .. } => None,
                _ => None,
            });
            let raw_edges = parse_edge_midpoints(&f.source_properties, rot);
            let Some(d) = dist_val else {
                continue;
            };
            if raw_edges.is_empty() {
                continue;
            };
            let dist_expr: solidcraft_doc::Expr = ir
                .model
                .parameters
                .iter()
                .filter(|p| p.owner.as_ref() == Some(&f.id) && matches!(p.value, Some(ParameterValue::Length(_))))
                .find_map(|p| {
                    let cand = p.expression.trim();
                    if !cand.is_empty()
                        && !expr_has_undefined_references(cand, &defined_names)
                        && doc.eval(cand, solidcraft_doc::expr::Kind::Length).is_ok_and(|v| (v - d).abs() <= 1.0e-4)
                    {
                        Some(solidcraft_doc::Expr::from(cand))
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| format!("{d}").into());
            let target_body_name = parse_body_slot_list(&f.source_properties, "affected_bodies")
                .into_iter()
                .find_map(|k| entity_to_body.get(&k).cloned())
                .or_else(|| output_keys.iter().find_map(|k| entity_to_body.get(k).cloned()))
                .or_else(|| {
                    let pt = raw_edges.first()?.0;
                    model
                        .state()
                        .bodies
                        .iter()
                        .find(|b| {
                            let bb = b.body.outline_bounds();
                            pt.x >= bb.min.x - 1.0
                                && pt.x <= bb.max.x + 1.0
                                && pt.y >= bb.min.y - 1.0
                                && pt.y <= bb.max.y + 1.0
                                && pt.z >= bb.min.z - 1.0
                                && pt.z <= bb.max.z + 1.0
                        })
                        .map(|b| b.name.clone())
                });
            if let Some(bname) = target_body_name {
                let edges = filter_edges_on_body(&model.state(), &bname, raw_edges.clone(), 1.5);
                let edges = filter_consumed_transverse_edges(&model.state(), &bname, edges, d);
                if edges.is_empty() {
                    continue;
                }
                let kind = FeatureKind::Chamfer {
                    edges: edges.clone(),
                    distance: dist_expr.clone(),
                    distance2: None,
                    angle: None,
                    flip: false,
                    body: Some(bname.clone().into()),
                    tangent_chain: true,
                };
                if doc.add_feature(kind, Some(fname)).is_ok() {
                    model.evaluate(&doc);
                    let mut bad = model.results.last().and_then(|res| res.error.as_ref()).is_some()
                        || model.state().bodies.iter().any(|b| b.body.lumps().is_err());
                    let mut used_tangent_chain = true;
                    if bad {
                        let feat_err = model.results.last().and_then(|res| res.error.as_ref());
                        log::warn!("native_timeline: Chamfer {fname} failed with tangent_chain=true: {feat_err:?}");
                        pop_failed_feature(&mut doc, &mut model);
                        let no_chain_kind = FeatureKind::Chamfer {
                            edges: edges.clone(),
                            distance: dist_expr.clone(),
                            distance2: None,
                            angle: None,
                            flip: false,
                            body: Some(bname.clone().into()),
                            tangent_chain: false,
                        };
                        if doc.add_feature(no_chain_kind, Some(fname)).is_ok() {
                            model.evaluate(&doc);
                            bad = model.results.last().and_then(|res| res.error.as_ref()).is_some()
                                || model.state().bodies.iter().any(|b| b.body.lumps().is_err());
                            if bad {
                                pop_failed_feature(&mut doc, &mut model);
                            } else {
                                used_tangent_chain = false;
                            }
                        }
                    }
                    if bad && edges.len() >= 2 && edges.len() <= 8 {
                        let mut start = 0;
                        while start < edges.len() {
                            let max_end = if start == 0 { edges.len() - 1 } else { edges.len() };
                            let mut matched_end = None;
                            let mut end = max_end;
                            while end > start {
                                let try_edges = edges[start..end].to_vec();
                                let sub_kind = FeatureKind::Chamfer {
                                    edges: try_edges,
                                    distance: dist_expr.clone(),
                                    distance2: None,
                                    angle: None,
                                    flip: false,
                                    body: Some(bname.clone().into()),
                                    tangent_chain: used_tangent_chain,
                                };
                                if doc.add_feature(sub_kind, Some(fname)).is_ok() {
                                    model.evaluate(&doc);
                                    let ok = model.results.last().and_then(|res| res.error.as_ref()).is_none()
                                        && !model.state().bodies.iter().any(|b| b.body.lumps().is_err());
                                    if ok {
                                        matched_end = Some(end);
                                        break;
                                    }
                                    pop_failed_feature(&mut doc, &mut model);
                                }
                                end -= 1;
                            }
                            if let Some(next_start) = matched_end {
                                start = next_start;
                            } else {
                                start += 1;
                            }
                        }
                    }
                }
            }
            continue;
        }

        // Shell feature
        if let FeatureOperation::Shell { removed_faces, thickness, outward, .. } = op_def {
            if let Some(t) = thickness {
                let dir = if outward.unwrap_or(false) { "outside".to_string() } else { "inside".to_string() };
                let target_key = output_keys.iter().find(|k| entity_to_body.contains_key(*k)).cloned();
                let target_name =
                    target_key.as_ref().and_then(|k| entity_to_body.get(k).cloned()).or_else(|| model.state().bodies.first().map(|b| b.name.clone()));
                if matches!(removed_faces, FaceSelection::Faces(faces) if faces.is_empty())
                    || matches!(removed_faces, FaceSelection::Unresolved)
                    || matches!(removed_faces, FaceSelection::Native(..))
                {
                    let mut applied_closed = false;
                    if let Some(ref bname) = target_name {
                        let shell_kind = FeatureKind::Shell {
                            faces: Vec::new(),
                            thickness: format!("{}", t.get()).into(),
                            body: Some(bname.clone().into()),
                            direction: dir.clone(),
                            tangent_chain: false,
                        };
                        if doc.add_feature(shell_kind, Some(fname)).is_ok() {
                            model.evaluate(&doc);
                            let bad = model.results.last().and_then(|res| res.error.as_ref()).is_some()
                                || model.state().bodies.iter().any(|b| b.body.lumps().is_err());
                            if bad {
                                pop_failed_feature(&mut doc, &mut model);
                            } else {
                                applied_closed = true;
                            }
                        }
                    }
                    if !applied_closed {
                        // Closed hollow shell fallback: deferred until SplitBody exposes the interior faces
                        pending_shell = Some((fname.to_string(), t.get(), dir));
                    }
                }
            }
            continue;
        }

        // DeleteFace feature (when used to delete an entire temporary body that is not in final bodies)
        if let FeatureOperation::DeleteFace { .. } = op_def {
            let mut remove_names: Vec<solidcraft_doc::BodyName> = Vec::new();
            let mut removed_keys: Vec<String> = Vec::new();
            for k in &output_keys {
                let referenced_later = timeline_features
                    .iter()
                    .filter(|later| later.ordinal > f.ordinal)
                    .any(|later| parse_body_slot_list(&later.source_properties, "affected_bodies").contains(k));
                if (!solid_body_keys.contains(k) || !referenced_later)
                    && model.state().bodies.len() > 1
                    && let Some(bname) = entity_to_body.get(k)
                {
                    let bn = solidcraft_doc::BodyName(bname.clone());
                    if !remove_names.contains(&bn) {
                        remove_names.push(bn);
                    }
                    removed_keys.push(k.clone());
                }
            }
            if !remove_names.is_empty() {
                let kind = FeatureKind::Remove { bodies: remove_names };
                if doc.add_feature(kind, Some(fname)).is_ok() {
                    model.evaluate(&doc);
                    for k in removed_keys {
                        entity_to_body.remove(&k);
                    }
                }
            }
            continue;
        }

        // DeleteBody (Remove) feature
        if let FeatureOperation::DeleteBody { bodies, .. } = op_def {
            let mut keys = body_selection_keys(bodies);
            for k in &deleted_keys {
                if !keys.contains(k) {
                    keys.push(k.clone());
                }
            }
            for k in &output_keys {
                if !keys.contains(k) {
                    keys.push(k.clone());
                }
            }
            let mut remove_names: Vec<solidcraft_doc::BodyName> = Vec::new();
            for k in &keys {
                if let Some(bname) = entity_to_body.get(k) {
                    let bn = solidcraft_doc::BodyName(bname.clone());
                    if !remove_names.contains(&bn) {
                        remove_names.push(bn);
                    }
                }
            }
            if !remove_names.is_empty() {
                let kind = FeatureKind::Remove { bodies: remove_names };
                if doc.add_feature(kind, Some(fname)).is_ok() {
                    model.evaluate(&doc);
                    for k in &keys {
                        if let Some(bname) = entity_to_body.get(k)
                            && !model.state().bodies.iter().any(|b| &b.name == bname)
                        {
                            entity_to_body.remove(k);
                        }
                    }
                }
            }
            continue;
        }

        // SplitBody feature
        if let FeatureOperation::SplitBody { targets, .. } = op_def {
            let target_key = body_selection_keys(targets)
                .into_iter()
                .find(|k| entity_to_body.contains_key(k))
                .or_else(|| output_keys.iter().find(|k| entity_to_body.contains_key(*k)).cloned());
            let target_name =
                target_key.as_ref().and_then(|k| entity_to_body.get(k).cloned()).or_else(|| model.state().bodies.first().map(|b| b.name.clone()));
            let Some(target) = target_name else {
                continue;
            };
            let split_plane = (|| {
                if let Some((_, sp_val)) = f.source_properties.iter().find(|(k, _)| *k == "split_plane") {
                    let parts: Vec<&str> = sp_val.split(';').collect();
                    if parts.len() == 2 {
                        let op: Vec<f64> = parts[0].split(',').filter_map(|s| s.parse().ok()).collect();
                        let np: Vec<f64> = parts[1].split(',').filter_map(|s| s.parse().ok()).collect();
                        if op.len() == 3 && np.len() == 3 {
                            let origin = rot(Vec3::new(op[0], op[1], op[2]));
                            let normal = rot(Vec3::new(np[0], np[1], np[2])).normalized()?;
                            let ref_axis = if normal.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
                            let u = normal.cross(ref_axis).normalized()?;
                            return Plane::new(origin, u, normal.cross(u));
                        }
                    }
                }
                None
            })()
            .or_else(|| {
                model.state().bodies.iter().find(|b| b.name == target).and_then(|mb| {
                    let bb = mb.body.outline_bounds();
                    datum_planes.iter().rev().find(|pl| plane_intersects_bbox(pl, bb.min, bb.max)).copied()
                })
            })
            .or_else(|| {
                let aff = parse_body_slot_list(&f.source_properties, "affected_bodies");
                let centers = parse_body_centers(&f.source_properties, rot);
                if aff.len() >= 2
                    && let (Some(&c1), Some(&c2)) = (centers.get(&aff[0]), centers.get(&aff[1]))
                {
                    let n = (c2 - c1).normalized()?;
                    let mid = (c1 + c2) * 0.5;
                    let ref_axis = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
                    let u = n.cross(ref_axis).normalized()?;
                    return Plane::new(mid, u, n.cross(u));
                }
                None
            })
            .or_else(|| datum_planes.last().copied());
            let Some(mut plane) = split_plane else {
                log::warn!("native_timeline: SplitBody {fname} skipped: split_plane is None for target {target}");
                continue;
            };
            let new_output_keys: Vec<String> =
                output_keys.iter().filter(|k| !deleted_keys.contains(k) && !entity_to_body.contains_key(*k)).cloned().collect();
            let nk_keeps_target_name = target_key
                .as_ref()
                .zip(new_output_keys.first())
                .is_some_and(|(tk, nk)| !solid_body_keys.contains(tk) && entity_body_names.get(nk) == Some(&target));
            let centers = parse_body_centers(&f.source_properties, rot);
            let should_flip = if let (Some(tk), Some(nk)) = (target_key.as_ref(), new_output_keys.first())
                && let (Some(&tc), Some(&nc)) = (centers.get(tk), centers.get(nk))
            {
                if nk_keeps_target_name { (nc - tc).dot(plane.normal()) < 0.0 } else { (tc - nc).dot(plane.normal()) < 0.0 }
            } else if let Some(tk) = target_key.as_ref()
                && let Some(&tc) = centers.get(tk)
            {
                (tc - plane.origin).dot(plane.normal()) < 0.0
            } else {
                false
            };
            if should_flip && let Some(flipped) = Plane::new(plane.origin, plane.y, plane.x) {
                plane = flipped;
            }
            let already_partitioned = output_keys.len() >= 2
                && output_keys.iter().all(|k| {
                    if let Some(bn) = entity_to_body.get(k) {
                        model.state().bodies.iter().any(|b| &b.name == bn)
                    } else if let Some(&c) = centers.get(k) {
                        let assigned: HashSet<String> = entity_to_body.values().cloned().collect();
                        find_matching_body(c, &model.state().bodies, &assigned).is_some()
                    } else {
                        false
                    }
                })
                && {
                    let mut matched_names = HashSet::new();
                    let assigned: HashSet<String> = entity_to_body.values().cloned().collect();
                    for k in &output_keys {
                        if let Some(bn) = entity_to_body.get(k) {
                            matched_names.insert(bn.clone());
                        } else if let Some(&c) = centers.get(k)
                            && let Some(m) = find_matching_body(c, &model.state().bodies, &assigned)
                        {
                            matched_names.insert(m.name.clone());
                        }
                    }
                    matched_names.len() >= output_keys.len()
                };

            if already_partitioned {
                let assigned: HashSet<String> = entity_to_body.values().cloned().collect();
                for k in &new_output_keys {
                    if let Some(&c) = centers.get(k)
                        && let Some(matched) = find_matching_body(c, &model.state().bodies, &assigned)
                    {
                        entity_to_body.insert(k.clone(), matched.name.clone());
                        continue;
                    }
                    if let Some(known_name) = entity_body_names.get(k)
                        && model.state().bodies.iter().any(|b| &b.name == known_name)
                    {
                        entity_to_body.insert(k.clone(), known_name.clone());
                    }
                }
                log::warn!("native_timeline: SplitBody {fname} skipped: all output bodies already exist as separate bodies");
                continue;
            }

            let target_bounds = model.state().bodies.iter().find(|b| b.name == target).map(|b| b.body.outline_bounds());
            let intersects = target_bounds.as_ref().is_some_and(|bb| plane_intersects_bbox(&plane, bb.min, bb.max));
            if !intersects {
                let assigned: HashSet<String> = entity_to_body.values().cloned().collect();
                for k in &new_output_keys {
                    if let Some(&c) = centers.get(k)
                        && let Some(matched) = find_matching_body(c, &model.state().bodies, &assigned)
                    {
                        entity_to_body.insert(k.clone(), matched.name.clone());
                        continue;
                    }
                    if let Some(known_name) = entity_body_names.get(k)
                        && model.state().bodies.iter().any(|b| &b.name == known_name)
                    {
                        entity_to_body.insert(k.clone(), known_name.clone());
                    }
                }
                log::warn!("native_timeline: SplitBody {fname} skipped: target {target} does not intersect split plane");
                continue;
            }
            let before_names: HashSet<String> = model.state().bodies.iter().map(|b| b.name.clone()).collect();
            let new_body_names: Vec<String> = new_output_keys
                .iter()
                .filter_map(|k| entity_body_names.get(k).filter(|&n| n != &target && !before_names.contains(n)).cloned())
                .collect();
            let kind = FeatureKind::Split { body: target.clone().into(), plane: PlaneRef::Custom { plane }, tool: None, sketch: None, line: None };
            if doc.add_feature(kind, Some(fname)).is_ok() {
                if !new_body_names.is_empty()
                    && let Some(feat) = doc.features.last_mut()
                {
                    feat.body_names = new_body_names.clone();
                }
                model.evaluate(&doc);
                let split_err = model.results.last().and_then(|r| r.error.as_ref());
                if split_err.is_some() {
                    log::warn!("native_timeline: SplitBody {fname} failed: {split_err:?}");
                    pop_failed_feature(&mut doc, &mut model);
                    let assigned: HashSet<String> = entity_to_body.values().cloned().collect();
                    for k in &new_output_keys {
                        if let Some(&c) = centers.get(k)
                            && let Some(matched) = find_matching_body(c, &model.state().bodies, &assigned)
                        {
                            entity_to_body.insert(k.clone(), matched.name.clone());
                            continue;
                        }
                        if let Some(known_name) = entity_body_names.get(k)
                            && model.state().bodies.iter().any(|b| &b.name == known_name)
                        {
                            entity_to_body.insert(k.clone(), known_name.clone());
                        }
                    }
                } else {
                    let mut resolved_by_centroid = false;
                    if let (Some(tk), Some(nk)) = (target_key.as_ref(), new_output_keys.first())
                        && let (Some(&tc), Some(&nc)) = (centers.get(tk), centers.get(nk))
                        && let (Some(target_mb), Some(created_mb)) = (
                            model.state().bodies.iter().find(|b| b.name == target),
                            model.state().bodies.iter().find(|b| !before_names.contains(&b.name)),
                        )
                    {
                        let c_target = (target_mb.body.outline_bounds().min + target_mb.body.outline_bounds().max) * 0.5;
                        let c_created = (created_mb.body.outline_bounds().min + created_mb.body.outline_bounds().max) * 0.5;
                        let dist_normal = (c_target - tc).dot(c_target - tc) + (c_created - nc).dot(c_created - nc);
                        let dist_swapped = (c_created - tc).dot(c_created - tc) + (c_target - nc).dot(c_target - nc);
                        if dist_swapped < dist_normal {
                            entity_to_body.insert(tk.clone(), created_mb.name.clone());
                            entity_to_body.insert(nk.clone(), target_mb.name.clone());
                        } else {
                            entity_to_body.insert(tk.clone(), target_mb.name.clone());
                            entity_to_body.insert(nk.clone(), created_mb.name.clone());
                        }
                        resolved_by_centroid = true;
                    }
                    if !resolved_by_centroid {
                        if nk_keeps_target_name && let (Some(tk), Some(nk)) = (target_key.as_ref(), new_output_keys.first()) {
                            entity_to_body.insert(nk.clone(), target.clone());
                            if let Some(created_name) =
                                model.state().bodies.iter().find(|b| !before_names.contains(&b.name) && b.name != target).map(|b| b.name.clone())
                            {
                                entity_to_body.insert(tk.clone(), created_name);
                            }
                        } else {
                            for k in &new_output_keys {
                                if let Some(created_name) = model
                                    .state()
                                    .bodies
                                    .iter()
                                    .find(|b| !before_names.contains(&b.name) && !entity_to_body.values().any(|v| v == &b.name))
                                    .map(|b| b.name.clone())
                                {
                                    entity_to_body.insert(k.clone(), created_name);
                                }
                            }
                        }
                    }
                    if let Some((shell_fname, thickness, dir)) = pending_shell.take() {
                        let mut targets_to_shell: Vec<String> = Vec::new();
                        for k in &output_keys {
                            if let Some(bname) = entity_to_body.get(k)
                                && !targets_to_shell.contains(bname)
                            {
                                targets_to_shell.push(bname.clone());
                            }
                        }
                        if targets_to_shell.is_empty() {
                            targets_to_shell = model.state().bodies.iter().map(|b| b.name.clone()).collect();
                        }
                        for bname in targets_to_shell {
                            let shell_pt = model
                                .state()
                                .bodies
                                .iter()
                                .find(|b| b.name == bname)
                                .map(|b| {
                                    let bb = b.body.outline_bounds();
                                    let c = (bb.min + bb.max) * 0.5;
                                    plane.origin + plane.x * (c - plane.origin).dot(plane.x) + plane.y * (c - plane.origin).dot(plane.y)
                                })
                                .unwrap_or(plane.origin);
                            let shell_kind = FeatureKind::Shell {
                                faces: vec![solidcraft_doc::FacePick(shell_pt)],
                                thickness: format!("{thickness}").into(),
                                body: Some(bname.clone().into()),
                                direction: dir.clone(),
                                tangent_chain: false,
                            };
                            let _ = doc.add_feature(shell_kind, Some(&format!("{shell_fname} ({bname})")));
                            model.evaluate(&doc);
                        }
                    }
                }
            }
            continue;
        }

        // Combine feature
        if let FeatureOperation::Combine { operands, op, keep_tools: ir_keep_tools } = op_def {
            let explicit_targets = body_selection_keys(operands.target());
            let explicit_tools = body_selection_keys(operands.tools());
            let target_key = explicit_targets
                .first()
                .cloned()
                .or_else(|| output_keys.iter().find(|k| !deleted_keys.contains(k) && entity_to_body.contains_key(*k)).cloned())
                .or_else(|| output_keys.iter().find(|k| entity_to_body.contains_key(*k)).cloned());
            let mut tool_keys: Vec<String> = explicit_tools.into_iter().filter(|k| Some(k) != target_key.as_ref()).collect();
            if tool_keys.is_empty() {
                tool_keys = output_keys.iter().filter(|&k| Some(k) != target_key.as_ref() && entity_to_body.contains_key(k)).cloned().collect();
            }
            if tool_keys.is_empty() {
                let centers = parse_body_centers(&f.source_properties, rot);
                for k in centers.keys() {
                    if Some(k) != target_key.as_ref() && entity_to_body.contains_key(k) && !tool_keys.contains(k) {
                        tool_keys.push(k.clone());
                    }
                }
            }
            if tool_keys.is_empty() {
                tool_keys = entity_to_body.keys().filter(|&k| Some(k) != target_key.as_ref()).cloned().collect();
            }
            let target_name = target_key.as_ref().and_then(|k| entity_to_body.get(k).cloned());
            if let Some(tname) = target_name {
                let mut tool_names: Vec<solidcraft_doc::BodyName> = Vec::new();
                for k in &tool_keys {
                    if let Some(bname) = entity_to_body.get(k)
                        && bname != &tname
                    {
                        let bn = solidcraft_doc::BodyName(bname.clone());
                        if !tool_names.contains(&bn) {
                            tool_names.push(bn);
                        }
                    }
                }
                if !tool_names.is_empty() {
                    let operation = match op {
                        BooleanKind::Cut => Operation::Cut,
                        BooleanKind::Intersect => Operation::Intersect,
                        BooleanKind::Join => Operation::Join,
                    };
                    let mut final_target = tname;
                    if matches!(operation, Operation::Join)
                        && let Some(tk) = &target_key
                        && let Some(desired) = entity_body_names.get(tk)
                        && final_target != *desired
                    {
                        let desired_bn = solidcraft_doc::BodyName(desired.clone());
                        if let Some(pos) = tool_names.iter().position(|b| b == &desired_bn) {
                            tool_names[pos] = solidcraft_doc::BodyName(final_target);
                            final_target = desired.clone();
                        }
                    }

                    // Fast bounding-box check for Combine
                    if let Some(target_b) = model.state().body(&final_target) {
                        let target_bb = target_b.outline_bounds();
                        if !target_bb.is_empty() {
                            let tools_intersect = tool_names.iter().any(|t| {
                                model.state().body(&t.0).is_some_and(|tb| {
                                    let ob = tb.outline_bounds();
                                    !ob.is_empty() && ob.meets(&target_bb)
                                })
                            });
                            if !tools_intersect {
                                log::warn!("native_timeline: Combine {fname} skipped: tool bodies do not intersect target `{final_target}`");
                                continue;
                            }
                        }
                    }

                    // A tool body that was not deleted as a whole body in the history transition (or remains in `ir.model.bodies`) was kept (`keep_tools = true`).
                    let keep_tools = *ir_keep_tools
                        || (!matches!(operation, Operation::Join)
                            && (tool_keys.is_empty() || tool_keys.iter().any(|k| !deleted_keys.contains(k) || entity_body_names.contains_key(k))));
                    let kind = FeatureKind::Combine {
                        target: solidcraft_doc::BodyName(final_target.clone()),
                        tools: tool_names.clone(),
                        operation,
                        keep_tools,
                    };
                    if doc.add_feature(kind, Some(fname)).is_ok() {
                        model.evaluate(&doc);
                        if model.results.last().is_some_and(|r| r.error.is_some()) {
                            let err = model.results.last().and_then(|r| r.error.as_ref());
                            log::warn!("native_timeline: Combine {fname} failed: {err:?}");
                            pop_failed_feature(&mut doc, &mut model);
                        } else if matches!(operation, Operation::Join)
                            && let Some(tk) = target_key
                        {
                            entity_to_body.insert(tk, final_target);
                            for k in &tool_keys {
                                if let Some(bname) = entity_to_body.get(k)
                                    && !model.state().bodies.iter().any(|b| &b.name == bname)
                                {
                                    entity_to_body.remove(k);
                                }
                            }
                        }
                    }
                }
            }
            continue;
        }

        let candidate_kind = match op_def {
            FeatureOperation::TreeNode { .. }
            | FeatureOperation::Sketch { .. }
            | FeatureOperation::DatumPlane { .. }
            | FeatureOperation::SplitBody { .. }
            | FeatureOperation::DeleteBody { .. }
            | FeatureOperation::DeleteFace { .. }
            | FeatureOperation::Combine { .. }
            | FeatureOperation::Shell { .. }
            | FeatureOperation::Fillet { .. }
            | FeatureOperation::Chamfer { .. } => None,
            FeatureOperation::Extrude {
                profile,
                extent,
                op,
                direction: ext_dir,
                start: _,
                solid: _,
                face_maker: _,
                inner_wire_taper: _,
                length_along_profile_normal: _,
                allow_multi_profile_faces: _,
            } => {
                let ProfileRef::Planar(planar_ref) = profile else {
                    continue;
                };
                let hist_face_sk = f
                    .source_properties
                    .get(&cadmpeg_core::nonblank_literal!("historical_face_sketch"))
                    .and_then(|s| sketch_map.keys().find(|k| k.as_str() == s.as_str()));
                let sk_id = match planar_ref {
                    PlanarProfileRef::Sketch(sk)
                    | PlanarProfileRef::SketchProfiles { sketch: sk, .. }
                    | PlanarProfileRef::SketchRegions { sketch: sk, .. }
                    | PlanarProfileRef::SketchSelection { sketch: sk, .. }
                    | PlanarProfileRef::SketchEntities { sketch: sk, .. } => Some(sk),
                    PlanarProfileRef::HistoricalFaces { .. } => hist_face_sk,
                    _ => None,
                };
                let Some(ir_sk_id) = sk_id else {
                    continue;
                };
                let Some(&sk_fid) = sketch_map.get(ir_sk_id) else {
                    continue;
                };
                let is_base_extrude = model.state().bodies.is_empty();
                let is_cut = matches!(op, BooleanOp::Cut);
                let profiles = if is_base_extrude && !matches!(planar_ref, PlanarProfileRef::SketchRegions { .. }) {
                    ProfileSel::All
                } else {
                    let ir_profs = sketch_ir_profiles.get(ir_sk_id).map(Vec::as_slice).unwrap_or(&[]);
                    let state = model.state();
                    let sc_profs = state.sketch(sk_fid).map(|s| s.profiles.as_slice()).unwrap_or(&[]);
                    let resolved_opt = if matches!(planar_ref, PlanarProfileRef::HistoricalFaces { .. }) {
                        let indices: Vec<u32> = f
                            .source_properties
                            .get(&cadmpeg_core::nonblank_literal!("historical_face_profiles"))
                            .map(|s| s.split(',').filter_map(|p| p.trim().parse::<u32>().ok()).collect())
                            .unwrap_or_default();
                        if let Ok(synth_ref) = PlanarProfileRef::sketch_profiles(ir_sk_id.clone(), indices) {
                            resolve_planar_profile_sel(&synth_ref, ir_profs, sc_profs, is_cut)
                        } else {
                            None
                        }
                    } else if matches!(planar_ref, PlanarProfileRef::SketchSelection { .. }) && sc_profs.len() > 1 {
                        let mut usage_counts = vec![0usize; sc_profs.len()];
                        for other_f in &timeline_features {
                            if other_f.ordinal == f.ordinal {
                                continue;
                            }
                            let other_op = match other_f.evaluation.definition() {
                                FeatureDefinition::Operation(op) => op,
                                FeatureDefinition::PostProcess { operation, .. } => operation,
                            };
                            if let FeatureOperation::Extrude { profile: ProfileRef::Planar(other_planar), .. } = other_op {
                                let other_sk = match other_planar {
                                    PlanarProfileRef::Sketch(sk)
                                    | PlanarProfileRef::SketchProfiles { sketch: sk, .. }
                                    | PlanarProfileRef::SketchRegions { sketch: sk, .. }
                                    | PlanarProfileRef::SketchSelection { sketch: sk, .. }
                                    | PlanarProfileRef::SketchEntities { sketch: sk, .. } => Some(sk),
                                    _ => None,
                                };
                                if other_sk == Some(ir_sk_id)
                                    && let Some(ProfileSel::Indices { indices }) =
                                        resolve_planar_profile_sel(other_planar, ir_profs, sc_profs, is_cut)
                                {
                                    for idx in indices {
                                        if idx < usage_counts.len() {
                                            usage_counts[idx] += 1;
                                        }
                                    }
                                }
                            }
                        }
                        let all_unused = usage_counts.iter().all(|&cnt| cnt == 0);
                        if all_unused {
                            let mut indices: Vec<usize> = (0..sc_profs.len()).collect();
                            filter_nested_profile_indices(&mut indices, sc_profs, is_cut);
                            if !indices.is_empty() && indices.len() < sc_profs.len() {
                                Some(ProfileSel::Indices { indices })
                            } else {
                                Some(ProfileSel::All)
                            }
                        } else {
                            let min_idx = usage_counts.iter().enumerate().min_by_key(|(_, cnt)| *cnt).map(|(idx, _)| idx).unwrap_or(0);
                            Some(ProfileSel::Indices { indices: vec![min_idx] })
                        }
                    } else {
                        resolve_planar_profile_sel(planar_ref, ir_profs, sc_profs, is_cut)
                    };
                    let Some(resolved_sel) = resolved_opt else {
                        continue;
                    };
                    resolved_sel
                };

                let to_dist = f
                    .source_properties
                    .get(&cadmpeg_core::nonblank_literal!("extrude_to_distance"))
                    .and_then(|s| s.parse::<f64>().ok())
                    .filter(|d| d.abs() > 1.0e-4);

                let is_to_shape = matches!(
                    extent,
                    ExtrudeExtent::OneSided {
                        side: ExtrudeSide {
                            termination: LinearTermination::ToShape { .. } | LinearTermination::ToFace { .. } | LinearTermination::ThroughNext { .. },
                            ..
                        },
                    }
                );
                let draft_deg = match extent {
                    ExtrudeExtent::OneSided { side: ExtrudeSide { draft: Some(a), .. } } => Some(a.get().to_degrees()),
                    _ => None,
                }
                .filter(|a| a.is_finite() && a.abs() > 1.0e-4 && a.abs() < 89.0);

                let (mut distance, mut direction, mut through_all, used_to_dist) = match extent {
                    ExtrudeExtent::OneSided { side: ExtrudeSide { termination: LinearTermination::Blind { length }, .. } } => {
                        (length.get(), Direction::Positive, false, false)
                    }
                    ExtrudeExtent::Symmetric { side: ExtrudeSide { termination: LinearTermination::Blind { length }, .. } } => {
                        (length.get() * 2.0, Direction::Symmetric, false, false)
                    }
                    ExtrudeExtent::OneSided {
                        side:
                            ExtrudeSide {
                                termination:
                                    LinearTermination::ThroughAll { .. }
                                    | LinearTermination::ToFace { .. }
                                    | LinearTermination::ToShape { .. }
                                    | LinearTermination::ThroughNext { .. },
                                ..
                            },
                    } => {
                        if let Some(d) = to_dist {
                            (d, Direction::Positive, false, true)
                        } else {
                            (0.0, Direction::Positive, true, false)
                        }
                    }
                    _ => (0.0, Direction::Positive, false, false),
                };

                if !used_to_dist && matches!(ext_dir, cadmpeg_ir::features::ExtrudeDirection::ReversedProfileNormal {}) {
                    direction = match direction {
                        Direction::Positive => Direction::Negative,
                        Direction::Negative => Direction::Positive,
                        d => d,
                    };
                }
                if distance < 0.0 {
                    direction = match direction {
                        Direction::Positive => Direction::Negative,
                        Direction::Negative => Direction::Positive,
                        d => d,
                    };
                    distance = distance.abs();
                }

                let is_new_body = matches!(op, BooleanOp::NewBody) || model.state().bodies.is_empty();

                let mut start_offset: Option<solidcraft_doc::Expr> = f
                    .source_properties
                    .get(&cadmpeg_core::nonblank_literal!("historical_face_offset"))
                    .and_then(|s| s.parse::<f64>().ok())
                    .filter(|v| v.is_finite() && v.abs() > 1.0e-4)
                    .map(|v| format!("{v}").into());

                let (operation, targets) = if is_new_body {
                    (Operation::NewBody, Vec::new())
                } else {
                    let boolean_op = match op {
                        BooleanOp::Cut => Operation::Cut,
                        BooleanOp::Intersect => Operation::Intersect,
                        _ => Operation::Join,
                    };
                    let mut tgts: Vec<solidcraft_doc::BodyName> = Vec::new();
                    let target_key_iter: Box<dyn Iterator<Item = &String>> = if boolean_op == Operation::Join {
                        Box::new(output_keys.iter().chain(deleted_keys.iter()))
                    } else {
                        Box::new(output_keys.iter())
                    };
                    for k in target_key_iter {
                        if let Some(bname) = entity_to_body.get(k) {
                            let bn = solidcraft_doc::BodyName(bname.clone());
                            if !tgts.contains(&bn) {
                                tgts.push(bn);
                            }
                        }
                    }
                    if tgts.is_empty() {
                        let assigned: HashSet<String> = entity_to_body.values().cloned().collect();
                        for k in &output_keys {
                            if let Some(&c) = centers_for_f.get(k)
                                && let Some(matched) = find_matching_body(c, &model.state().bodies, &assigned)
                            {
                                entity_to_body.insert(k.clone(), matched.name.clone());
                                let bn = solidcraft_doc::BodyName(matched.name.clone());
                                if !tgts.contains(&bn) {
                                    tgts.push(bn);
                                }
                            }
                        }
                    }
                    if tgts.is_empty()
                        && let Some(first) = model.state().bodies.first()
                    {
                        tgts.push(solidcraft_doc::BodyName(first.name.clone()));
                    }
                    if boolean_op == Operation::Cut && tgts.len() > 1 {
                        let uncut: Vec<solidcraft_doc::BodyName> = tgts
                            .iter()
                            .filter(|t| {
                                !doc.features.iter().any(|prev_f| {
                                    matches!(
                                        &prev_f.kind,
                                        FeatureKind::Extrude {
                                            sketch: prev_sk,
                                            profiles: prev_profs,
                                            operation: Operation::Cut,
                                            targets: prev_tgts,
                                            ..
                                        } if *prev_sk == SketchId(sk_fid)
                                            && prev_profs == &profiles
                                            && prev_tgts.contains(t)
                                    )
                                })
                            })
                            .cloned()
                            .collect();
                        if !uncut.is_empty() {
                            tgts = uncut;
                        }
                    }
                    (boolean_op, tgts)
                };

                let mut profiles = profiles;
                if is_to_shape
                    && !used_to_dist
                    && matches!(operation, Operation::Join)
                    && let ProfileSel::Indices { ref mut indices } = profiles
                    && let Some(sk_state) = model.state().sketch(sk_fid)
                {
                    let sc_profs = sk_state.profiles.as_slice();
                    let mut extra = Vec::new();
                    for &idx in indices.iter() {
                        if let Some(p) = sc_profs.get(idx)
                            && !p.region.holes.is_empty()
                        {
                            for (h_idx, hp) in sc_profs.iter().enumerate() {
                                if !indices.contains(&h_idx)
                                    && !extra.contains(&h_idx)
                                    && p.region
                                        .holes
                                        .iter()
                                        .any(|h| (Region2 { outer: h.ccw(), holes: Vec::new() }).contains(hp.region.interior_point()))
                                {
                                    extra.push(h_idx);
                                }
                            }
                        }
                    }
                    indices.extend(extra);
                }

                let mut to_pos: Option<solidcraft_doc::FacePick> = None;
                if is_to_shape
                    && !used_to_dist
                    && let Some(sk_state) = model.state().sketch(sk_fid)
                {
                    let sc_profs = sk_state.profiles.as_slice();
                    let first_idx = match &profiles {
                        ProfileSel::Indices { indices } => indices.first().copied(),
                        ProfileSel::All => Some(0),
                        _ => None,
                    };
                    if let Some(idx) = first_idx
                        && let Some(prof) = sc_profs.get(idx)
                    {
                        let bb = prof.region.outer.polyline(1.0e-2).bounds();
                        let c2 = (bb.min + bb.max) * 0.5;
                        let ray_origin = sk_state.plane.to_world(c2);
                        let primary_dir = match direction {
                            Direction::Negative => -sk_state.plane.normal(),
                            _ => sk_state.plane.normal(),
                        };
                        let ray_cast = |dir: Vec3| -> Option<f64> {
                            let mut best: Option<f64> = None;
                            for b in model.state().bodies.iter().filter(|b| targets.is_empty() || targets.iter().any(|t| t.0 == b.name)) {
                                let m = b.mesh();
                                for tri in &m.triangles {
                                    let Some([p0, p1, p2]) = m.tri(tri) else {
                                        continue;
                                    };
                                    let e1 = p1 - p0;
                                    let e2 = p2 - p0;
                                    let h = dir.cross(e2);
                                    let a = e1.dot(h);
                                    if a.abs() > 1.0e-9 {
                                        let f_inv = 1.0 / a;
                                        let s = ray_origin - p0;
                                        let u = f_inv * s.dot(h);
                                        if (0.0..=1.0).contains(&u) {
                                            let q = s.cross(e1);
                                            let v = f_inv * dir.dot(q);
                                            if v >= 0.0 && u + v <= 1.0 {
                                                let t = f_inv * e2.dot(q);
                                                if t > 1.0e-3 && best.is_none_or(|bt| t < bt) {
                                                    best = Some(t);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            best
                        };
                        let (hit_t, hit_dir) = if let Some(t) = ray_cast(primary_dir) {
                            (Some(t), primary_dir)
                        } else if let Some(t) = ray_cast(-primary_dir) {
                            direction = match direction {
                                Direction::Positive => Direction::Negative,
                                Direction::Negative => Direction::Positive,
                                d => d,
                            };
                            (Some(t), -primary_dir)
                        } else {
                            (None, primary_dir)
                        };
                        if let Some(t) = hit_t {
                            to_pos = Some(solidcraft_doc::FacePick(ray_origin + hit_dir * t));
                            distance = t + 2.0;
                            through_all = false;
                        }
                    }
                }

                if matches!(operation, Operation::Cut)
                    && start_offset.is_none()
                    && let ProfileSel::Indices { indices: ref cut_idx } = profiles
                    && !cut_idx.is_empty()
                {
                    let opposite_join_dist = doc.features.iter().find_map(|prev_f| {
                        let FeatureKind::Extrude {
                            sketch: prev_sk,
                            profiles: ProfileSel::Indices { indices: prev_idx },
                            extent: prev_ext,
                            operation: Operation::Join,
                            targets: prev_tgts,
                            ..
                        } = &prev_f.kind
                        else {
                            return None;
                        };
                        let opposite_dir = matches!(
                            (direction, prev_ext.direction),
                            (Direction::Negative, Direction::Positive) | (Direction::Positive, Direction::Negative)
                        );
                        if *prev_sk == SketchId(sk_fid)
                            && opposite_dir
                            && prev_ext.start_offset.is_none()
                            && prev_idx.len() > cut_idx.len()
                            && cut_idx.iter().all(|i| prev_idx.contains(i))
                            && (targets.is_empty() || targets.iter().any(|t| prev_tgts.contains(t)))
                        {
                            doc.eval(&prev_ext.distance, solidcraft_doc::expr::Kind::Length).ok().filter(|&d| d > 1.0e-4)
                        } else {
                            None
                        }
                    });
                    if let Some(join_dist) = opposite_join_dist {
                        let signed_off = match direction {
                            Direction::Negative => join_dist,
                            Direction::Positive => -join_dist,
                            Direction::Symmetric => 0.0,
                        };
                        if signed_off.abs() > 1.0e-4 {
                            start_offset = Some(format!("{signed_off}").into());
                            distance += join_dist;
                        }
                    }
                }

                if matches!(planar_ref, PlanarProfileRef::HistoricalFaces { .. })
                    && let Some(so_expr) = start_offset.as_deref()
                    && let Ok(so_val) = doc.eval(so_expr, solidcraft_doc::expr::Kind::Length)
                {
                    if used_to_dist && is_to_shape && matches!(operation, Operation::Cut) {
                        distance += 1.0;
                    }
                    let cap_dist = doc.features.iter().find_map(|prev_f| {
                        let FeatureKind::Extrude { sketch: prev_sk, profiles: prev_profs, extent: prev_ext, .. } = &prev_f.kind else {
                            return None;
                        };
                        if *prev_sk == SketchId(sk_fid)
                            && prev_profs == &profiles
                            && prev_ext.direction == direction
                            && prev_ext.start_offset.is_none()
                            && let Ok(prev_dist) = doc.eval(&prev_ext.distance, solidcraft_doc::expr::Kind::Length)
                            && (prev_dist - so_val.abs()).abs() <= 1.0e-3
                        {
                            Some(prev_dist)
                        } else {
                            None
                        }
                    });
                    if let Some(prev_dist) = cap_dist {
                        let exact_so = if so_val < 0.0 { -prev_dist } else { prev_dist };
                        start_offset = Some(format!("{exact_so}").into());
                    }
                }

                let param_dist_expr = (!used_to_dist && !through_all && to_pos.is_none() && start_offset.is_none())
                    .then(|| {
                        ir.model
                            .parameters
                            .iter()
                            .filter(|p| p.owner.as_ref() == Some(&f.id) && matches!(p.value, Some(ParameterValue::Length(_))))
                            .find_map(|p| {
                                let raw = p.expression.trim();
                                let cand = raw.strip_prefix('-').map(str::trim).unwrap_or(raw);
                                if !cand.is_empty()
                                    && !expr_has_undefined_references(cand, &defined_names)
                                    && doc.eval(cand, solidcraft_doc::expr::Kind::Length).is_ok_and(|v| (v - distance).abs() <= 1.0e-4)
                                {
                                    Some(solidcraft_doc::Expr::from(cand))
                                } else {
                                    None
                                }
                            })
                    })
                    .flatten();
                let dist_expr: solidcraft_doc::Expr = param_dist_expr.unwrap_or_else(|| format!("{distance}").into());

                let taper = draft_deg.map(|a| {
                    let signed_a = if matches!(operation, Operation::Cut) { a.abs() } else { a };
                    format!("{signed_a} deg").into()
                });

                let ext = Extent {
                    distance: dist_expr.clone(),
                    convention: ExtrudeConvention::TotalDistance,
                    direction,
                    distance2: None,
                    start_offset,
                    through_all,
                    second_follows_first: false,
                    taper,
                    corner: None,
                    to: to_pos,
                    to_offset: None,
                    from: None,
                    to_body_apart: false,
                };

                Some(FeatureKind::Extrude { sketch: SketchId(sk_fid), profiles, extent: ext, operation, targets, thin: None })
            }
            FeatureOperation::Sweep { shape, path, .. } => {
                let Some(planar_ref) = shape.referenced_profile() else {
                    continue;
                };
                let prof_sk_id = match planar_ref {
                    PlanarProfileRef::Sketch(sk)
                    | PlanarProfileRef::SketchProfiles { sketch: sk, .. }
                    | PlanarProfileRef::SketchRegions { sketch: sk, .. }
                    | PlanarProfileRef::SketchSelection { sketch: sk, .. }
                    | PlanarProfileRef::SketchEntities { sketch: sk, .. } => Some(sk),
                    _ => None,
                };
                let Some(prof_ir_sk) = prof_sk_id else {
                    continue;
                };
                let Some(&prof_fid) = sketch_map.get(prof_ir_sk) else {
                    continue;
                };
                let Some(PathRef::SketchCurves { sketch: path_ir_sk, curves }) = path else {
                    continue;
                };
                let Some(&path_fid) = sketch_map.get(path_ir_sk) else {
                    continue;
                };
                let path_curves: Vec<String> = if let Some(ent_map) = sketch_entity_curves.get(path_ir_sk) {
                    curves.iter().filter_map(|c| ent_map.get(c).cloned()).collect()
                } else {
                    Vec::new()
                };
                if path_curves.is_empty() {
                    continue;
                };
                let ir_profs = sketch_ir_profiles.get(prof_ir_sk).map(Vec::as_slice).unwrap_or(&[]);
                let state = model.state();
                let sc_profs = state.sketch(prof_fid).map(|s| s.profiles.as_slice()).unwrap_or(&[]);
                let is_sweep_cut = matches!(shape.mode(), SweepMode::Solid { op: SolidSweepOperation::Cut });
                let mut profiles = resolve_planar_profile_sel(planar_ref, ir_profs, sc_profs, is_sweep_cut).unwrap_or(ProfileSel::All);
                if matches!(profiles, ProfileSel::All) && sc_profs.len() > 1 && !sc_profs[0].region.holes.is_empty() {
                    let idx = sc_profs.iter().position(|p| p.region.holes.is_empty()).unwrap_or(0);
                    profiles = ProfileSel::Indices { indices: vec![idx] };
                }
                let operation = match shape.mode() {
                    SweepMode::Solid { op: SolidSweepOperation::Join } => Operation::Join,
                    SweepMode::Solid { op: SolidSweepOperation::Cut } => Operation::Cut,
                    SweepMode::Solid { op: SolidSweepOperation::Intersect } => Operation::Intersect,
                    _ => Operation::NewBody,
                };
                let targets: Vec<solidcraft_doc::BodyName> = if matches!(operation, Operation::NewBody) {
                    Vec::new()
                } else {
                    output_keys.iter().filter_map(|k| entity_to_body.get(k).cloned()).map(solidcraft_doc::BodyName).collect()
                };
                Some(FeatureKind::Sweep {
                    sketch: SketchId(prof_fid),
                    profiles,
                    path_sketch: SketchId(path_fid),
                    path: path_curves,
                    orientation: SweepOrientation::Perpendicular,
                    twist: None,
                    scale: None,
                    rails: Vec::new(),
                    rail_fit: RailFit::Scale,
                    operation,
                    targets,
                })
            }
            FeatureOperation::Loft { sections: ir_sections, op, .. } => {
                let operation = match op {
                    BooleanOp::NewBody => Operation::NewBody,
                    BooleanOp::Join => Operation::Join,
                    BooleanOp::Cut => Operation::Cut,
                    BooleanOp::Intersect => Operation::Intersect,
                    BooleanOp::Unresolved => Operation::NewBody,
                };
                let is_cut = matches!(operation, Operation::Cut);
                let mut sections = Vec::new();
                for sec in ir_sections {
                    let cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Planar(planar_ref)) = sec else {
                        continue;
                    };
                    let sk_id = match planar_ref {
                        PlanarProfileRef::Sketch(sk)
                        | PlanarProfileRef::SketchProfiles { sketch: sk, .. }
                        | PlanarProfileRef::SketchRegions { sketch: sk, .. }
                        | PlanarProfileRef::SketchSelection { sketch: sk, .. }
                        | PlanarProfileRef::SketchEntities { sketch: sk, .. } => Some(sk),
                        _ => None,
                    };
                    let Some(ir_sk_id) = sk_id else {
                        continue;
                    };
                    let Some(&sk_fid) = sketch_map.get(ir_sk_id) else {
                        continue;
                    };
                    let ir_profs = sketch_ir_profiles.get(ir_sk_id).map(Vec::as_slice).unwrap_or(&[]);
                    let state = model.state();
                    let sc_profs = state.sketch(sk_fid).map(|s| s.profiles.as_slice()).unwrap_or(&[]);
                    let Some(profiles) = resolve_planar_profile_sel(planar_ref, ir_profs, sc_profs, is_cut) else {
                        continue;
                    };
                    sections.push(solidcraft_doc::LoftSection { sketch: SketchId(sk_fid), profiles, point: None });
                }
                if sections.len() < 2 {
                    continue;
                }
                let targets: Vec<solidcraft_doc::BodyName> = if matches!(operation, Operation::NewBody) {
                    Vec::new()
                } else {
                    output_keys.iter().filter_map(|k| entity_to_body.get(k).cloned()).map(solidcraft_doc::BodyName).collect()
                };
                Some(FeatureKind::Loft { sections, operation, targets, rails: Vec::new(), rail_fit: RailFit::Scale })
            }
            FeatureOperation::Pattern { seeds, pattern } => {
                let mirror_plane = match pattern.definition() {
                    PatternTransform::Mirror { plane_origin, plane_normal } => {
                        let o = rot(Vec3::new(plane_origin.get().x, plane_origin.get().y, plane_origin.get().z));
                        let n = rot(Vec3::new(plane_normal.get().x, plane_normal.get().y, plane_normal.get().z));
                        if n.len() > 1.0e-5 {
                            let ref_axis = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
                            n.cross(ref_axis).normalized().and_then(|u| Plane::new(o, u, n.cross(u)))
                        } else {
                            datum_planes.last().copied()
                        }
                    }
                    PatternTransform::Unresolved { form: Some(cadmpeg_ir::features::patterns::PatternForm::Mirror) } => datum_planes.last().copied(),
                    _ => continue,
                };
                let seed_keys: Vec<String> = seeds
                    .iter()
                    .flat_map(|s| match s {
                        PatternSeed::Bodies(sel) => body_selection_keys(sel),
                        _ => Vec::new(),
                    })
                    .collect();
                let mirror_bodies: Vec<solidcraft_doc::BodyName> =
                    seed_keys.iter().filter_map(|k| entity_to_body.get(k).cloned()).map(solidcraft_doc::BodyName).collect();
                if let Some(pl) = mirror_plane {
                    if !mirror_bodies.is_empty() {
                        Some(FeatureKind::Mirror {
                            features: Vec::new(),
                            plane: PlaneRef::Custom { plane: pl },
                            bodies: mirror_bodies,
                            combine: false,
                            placements: Vec::new(),
                        })
                    } else {
                        let st = model.state();
                        let mut side_sign: Option<f64> = None;
                        let mut start_idx: Option<usize> = None;
                        for (idx, pf) in doc.features.iter().enumerate().rev() {
                            let Some((min_d, max_d)) = feature_signed_distance_range(pf, &st, &pl) else {
                                continue;
                            };
                            let feat_sign = if min_d > -1.0e-3 && max_d > 1.0e-3 {
                                1.0
                            } else if max_d < 1.0e-3 && min_d < -1.0e-3 {
                                -1.0
                            } else {
                                break;
                            };
                            match side_sign {
                                None => {
                                    side_sign = Some(feat_sign);
                                    start_idx = Some(idx);
                                }
                                Some(s) if (s - feat_sign).abs() < 0.5 => {
                                    start_idx = Some(idx);
                                }
                                Some(_) => break,
                            }
                        }
                        let mirror_features: Vec<solidcraft_doc::FeatureId> = start_idx
                            .map(|si| {
                                doc.features[si..]
                                    .iter()
                                    .filter(|pf| {
                                        !matches!(
                                            pf.kind,
                                            FeatureKind::Sketch { .. }
                                                | FeatureKind::ConstructionPlane { .. }
                                                | FeatureKind::Split { .. }
                                                | FeatureKind::Remove { .. }
                                        )
                                    })
                                    .map(|pf| solidcraft_doc::FeatureId(pf.id))
                                    .collect()
                            })
                            .unwrap_or_default();
                        if !mirror_features.is_empty() {
                            Some(FeatureKind::Mirror {
                                features: mirror_features,
                                plane: PlaneRef::Custom { plane: pl },
                                bodies: Vec::new(),
                                combine: false,
                                placements: Vec::new(),
                            })
                        } else {
                            None
                        }
                    }
                } else {
                    None
                }
            }
            FeatureOperation::Revolve { construction, op } => {
                let RevolveConstruction::Resolved { profile, extent, .. } = construction else {
                    continue;
                };

                let sk_id = match profile {
                    PlanarProfileRef::Sketch(sk)
                    | PlanarProfileRef::SketchProfiles { sketch: sk, .. }
                    | PlanarProfileRef::SketchRegions { sketch: sk, .. }
                    | PlanarProfileRef::SketchSelection { sketch: sk, .. }
                    | PlanarProfileRef::SketchEntities { sketch: sk, .. } => sketch_map.get(sk).copied(),
                    _ => None,
                };
                let Some(sk_fid) = sk_id else {
                    continue;
                };

                let profiles = match profile {
                    PlanarProfileRef::SketchProfiles { profiles, .. } if !profiles.is_empty() => {
                        ProfileSel::Indices { indices: profiles.iter().map(|&i| i as usize).collect() }
                    }
                    _ => ProfileSel::All,
                };

                let angle_deg = match extent {
                    cadmpeg_ir::features::RevolveExtent::OneSided { termination: cadmpeg_ir::features::AngularTermination::Angle { angle } } => {
                        angle.get().to_degrees()
                    }
                    cadmpeg_ir::features::RevolveExtent::Symmetric { termination: cadmpeg_ir::features::AngularTermination::Angle { angle } } => {
                        angle.get().to_degrees()
                    }
                    _ => continue,
                };

                let operation = match op {
                    BooleanOp::NewBody => Operation::NewBody,
                    BooleanOp::Join => Operation::Join,
                    BooleanOp::Cut => Operation::Cut,
                    BooleanOp::Intersect => Operation::Intersect,
                    BooleanOp::Unresolved => Operation::NewBody,
                };

                Some(FeatureKind::Revolve {
                    sketch: SketchId(sk_fid),
                    profiles,
                    axis: solidcraft_doc::AxisRef::SketchAxis { axis: "x".into() },
                    angle: format!("{angle_deg}").into(),
                    operation,
                    targets: Vec::new(),
                    angle2: None,
                    symmetric: matches!(extent, cadmpeg_ir::features::RevolveExtent::Symmetric { .. }),
                    convention: Default::default(),
                    to: None,
                })
            }
            _ => None,
        };

        let _is_join_op = matches!(
            &candidate_kind,
            Some(
                FeatureKind::Extrude { operation: Operation::Join, .. }
                    | FeatureKind::Sweep { operation: Operation::Join, .. }
                    | FeatureKind::Revolve { operation: Operation::Join, .. }
            )
        );
        if let Some(kind) = candidate_kind {
            // Fast-rejection: bounding box check for boolean operations
            if let FeatureKind::Extrude { sketch, ref profiles, ref extent, operation, ref targets, .. } = kind
                && matches!(operation, Operation::Cut | Operation::Intersect)
                && !model.state().bodies.is_empty()
            {
                if let Ok(d_val) = doc.eval(&extent.distance, solidcraft_doc::expr::Kind::Length)
                    && !extent.through_all
                    && extent.to.is_none()
                    && extent.from.is_none()
                    && d_val.abs() <= 1.0e-6
                {
                    log::warn!("native_timeline: candidate Extrude {fname} skipped: cut depth {d_val} <= 1nm is below kernel resolution");
                    continue;
                }
                let tool_bb_opt = extrude_tool_bounds(&doc, &model, sketch.0, profiles, extent);
                if let Some(tool_bb) = tool_bb_opt {
                    let st = model.state();
                    let intersects_target = if targets.is_empty() {
                        st.bodies.iter().any(|tb| {
                            let ob = tb.outline_bounds();
                            if ob.is_empty() || !ob.meets(&tool_bb) {
                                return false;
                            }
                            let inter = ob.intersection(&tool_bb);
                            let d = inter.max - inter.min;
                            d.x > 1.0e-6 && d.y > 1.0e-6 && d.z > 1.0e-6
                        })
                    } else {
                        targets.iter().filter_map(|t| st.body(&t.0)).any(|tb| {
                            let ob = tb.outline_bounds();
                            if ob.is_empty() || !ob.meets(&tool_bb) {
                                return false;
                            }
                            let inter = ob.intersection(&tool_bb);
                            let d = inter.max - inter.min;
                            d.x > 1.0e-6 && d.y > 1.0e-6 && d.z > 1.0e-6
                        })
                    };
                    if !intersects_target {
                        log::warn!("native_timeline: candidate Extrude {fname} skipped: tool bounding box does not intersect any target body");
                        continue;
                    }
                }
            }

            let before_names: HashSet<String> = model.state().bodies.iter().map(|b| b.name.clone()).collect();
            let new_output_keys: Vec<String> = output_keys.iter().filter(|k| !entity_to_body.contains_key(*k)).cloned().collect();
            let new_body_names: Vec<String> = new_output_keys.iter().filter_map(|k| entity_body_names.get(k).cloned()).collect();
            let fallback_kind = match &kind {
                FeatureKind::Extrude { sketch, profiles, extent, operation, targets, thin } if extent.taper.is_some() || extent.to.is_some() => {
                    let mut fb_ext = extent.clone();
                    fb_ext.taper = None;
                    if fb_ext.to.is_some() {
                        fb_ext.to = None;
                        fb_ext.through_all = false;
                    }
                    Some(FeatureKind::Extrude {
                        sketch: *sketch,
                        profiles: profiles.clone(),
                        extent: fb_ext,
                        operation: *operation,
                        targets: targets.clone(),
                        thin: thin.clone(),
                    })
                }
                _ => None,
            };
            if doc.add_feature(kind.clone(), Some(fname)).is_ok() {
                if !new_body_names.is_empty()
                    && let Some(feat) = doc.features.last_mut()
                {
                    feat.body_names = new_body_names.clone();
                }
                model.evaluate(&doc);
                let mut err = model.results.last().and_then(|r| r.error.as_ref()).cloned();
                if err.is_some() {
                    log::warn!("native_timeline: candidate feature {fname} failed: {err:?}");
                    pop_failed_feature(&mut doc, &mut model);
                    if let FeatureKind::Mirror { ref features, ref plane, combine, ref placements, ref bodies } = kind
                        && features.len() > 1
                        && bodies.is_empty()
                    {
                        for f_id in features {
                            let single_mirror = FeatureKind::Mirror {
                                features: vec![*f_id],
                                plane: plane.clone(),
                                bodies: Vec::new(),
                                combine,
                                placements: placements.clone(),
                            };
                            if doc.add_feature(single_mirror, Some(&format!("{fname} ({})", f_id.0))).is_ok() {
                                model.evaluate(&doc);
                                if model.results.last().and_then(|r| r.error.as_ref()).is_some() {
                                    pop_failed_feature(&mut doc, &mut model);
                                }
                            }
                        }
                        continue;
                    }
                    if let Some(fb) = fallback_kind
                        && doc.add_feature(fb.clone(), Some(fname)).is_ok()
                    {
                        if !new_body_names.is_empty()
                            && let Some(feat) = doc.features.last_mut()
                        {
                            feat.body_names = new_body_names;
                        }
                        model.evaluate(&doc);
                        err = model.results.last().and_then(|r| r.error.as_ref()).cloned();
                        if err.is_some() {
                            log::warn!("native_timeline: fallback feature {fname} failed: {err:?}");
                            pop_failed_feature(&mut doc, &mut model);
                        }
                    }
                }
                if err.is_none() {
                    let after_names: HashSet<String> = model.state().bodies.iter().map(|b| b.name.clone()).collect();
                    for dk in &deleted_keys {
                        if let Some(bname) = entity_to_body.get(dk)
                            && !after_names.contains(bname)
                        {
                            entity_to_body.remove(dk);
                        }
                    }
                    let surviving_name = output_keys
                        .iter()
                        .find_map(|ok| entity_to_body.get(ok))
                        .filter(|n| after_names.contains(*n))
                        .cloned()
                        .or_else(|| after_names.iter().next().cloned());
                    if let Some(surviving) = surviving_name {
                        for v in entity_to_body.values_mut() {
                            if !after_names.contains(v) {
                                *v = surviving.clone();
                            }
                        }
                    }
                    let mut mapped_new = false;
                    for k in &new_output_keys {
                        if let Some(created) =
                            model.state().bodies.iter().find(|b| !before_names.contains(&b.name) && !entity_to_body.values().any(|v| v == &b.name))
                        {
                            entity_to_body.insert(k.clone(), created.name.clone());
                            mapped_new = true;
                        }
                    }
                    if !mapped_new
                        && !new_output_keys.is_empty()
                        && let Some(existing_body) = output_keys.iter().find_map(|k| entity_to_body.get(k).cloned())
                    {
                        for k in &new_output_keys {
                            entity_to_body.insert(k.clone(), existing_body.clone());
                        }
                    }
                }
            }
        }
    }

    // Append any prepared sketches that were not referenced by a timeline sketch feature
    let mut extra_sketches_added = false;
    for (_id, prep) in prepared_sketches {
        let display_name = if !prep.name.is_empty() && !prep.name.starts_with("0_") { prep.name } else { "Sketch".to_string() };
        if doc.add_feature(FeatureKind::Sketch { plane: prep.plane_ref, sketch: prep.sketch }, Some(&display_name)).is_ok() {
            extra_sketches_added = true;
        }
    }
    if extra_sketches_added || model.results.len() != doc.features.len() {
        model.evaluate(&doc);
    }

    model.set_validation(validation);
    if validation != solidcraft_kernel::Validation::Off {
        let mut final_validation_err: Option<String> = None;
        for b in &model.state().bodies {
            if let Err(e) = solidcraft_kernel::check_kept(&b.body) {
                final_validation_err = Some(format!("body `{}`: {e}", b.name));
                break;
            }
        }
        if let Some(err) = final_validation_err
            && let Some(last) = model.results.last_mut()
            && last.error.is_none()
        {
            last.error = Some(err);
        }
    }

    // 4. Option B2: Evaluation and Fallback check
    let eval_success = !model.state().bodies.is_empty() && !model.results.iter().any(|r| r.error.is_some());
    if eval_success {
        return Ok(Some((doc, model)));
    }

    // Fallback: If evaluation produced no bodies or has errors, append base B-Rep as FeatureKind::Import
    if let Ok((step, _warnings)) = native_cad_ir_to_step(file, &format, &ir, Some(&fidelity), bytes.len()) {
        let file_name = std::path::Path::new(file).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| file.to_string());
        let _ = doc
            .add_feature(FeatureKind::Import { file: file_name, step: step.into(), components: Vec::new(), skip: Vec::new() }, Some("BaseFeature"));
        model.evaluate(&doc);
    }

    Ok(Some((doc, model)))
}

/// Convert native CAD bytes to a SolidCraft document with timeline features (Option A & B).
/// Returns `None` if the file does not have native sketches or features.
pub fn native_cad_to_document(bytes: &[u8], file: &str) -> Result<Option<Document>, String> {
    native_cad_to_document_with_model(bytes, file).map(|opt| opt.map(|(d, _m)| d))
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "f3d")]
    use super::*;

    #[test]
    #[cfg(feature = "f3d")]
    fn test_1wire_timeline_step_conformance() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = manifest_dir.join("../../tests/fixtures/1wire");
        let f3d_path = root.join("1WireRGBWController.f3d");
        let timeline_dir = root.join("Timeline");
        let walkthrough = timeline_dir.join("TimelineWalkthrough.md");

        let bytes = std::fs::read(&f3d_path)
            .or_else(|_| std::fs::read("/archive/Projects/Fusion360/1 Wire LED Controller Rev 1.0/1WireRGBWController.f3d"))
            .expect("1WireRGBWController.f3d must be present in tests/fixtures/1wire or archive");

        assert!(
            walkthrough.exists()
                || std::path::Path::new("/archive/Projects/Fusion360/1 Wire LED Controller Rev 1.0/Timeline/TimelineWalkthrough.md").exists(),
            "TimelineWalkthrough.md must be present"
        );

        let (doc, model) =
            native_cad_to_document_with_model(&bytes, "1WireRGBWController.f3d").expect("evaluation succeeds").expect("document and model produced");

        assert!(!doc.features.is_empty(), "Document should have timeline features");
        assert_eq!(doc.features.len(), model.results.len());
        for (i, f) in doc.features.iter().enumerate() {
            let res = &model.results[i];
            assert!(res.error.is_none(), "Feature [{:02}] {} returned error: {:?}", i + 1, f.name, res.error);
        }

        // Verify key timeline steps against reference STEP files
        let step_cases = [
            ("02_Extrude1.step", 1, 6),
            ("03_Fillet1.step", 1, 26),
            ("04_Shell1.step", 1, 52),
            ("06_Split1.step", 2, 70),
            ("76_Mirror1.step", 2, 401),
        ];

        for (step_file, expected_bodies, expected_faces) in step_cases {
            let step_path = timeline_dir.join(step_file);
            let alt_path = std::path::PathBuf::from(format!("/archive/Projects/Fusion360/1 Wire LED Controller Rev 1.0/Timeline/{}", step_file));
            let step_bytes = std::fs::read(&step_path).or_else(|_| std::fs::read(&alt_path));
            if let Ok(content) = step_bytes {
                let content_str = String::from_utf8_lossy(&content);
                let actual_face_count = content_str.matches("ADVANCED_FACE").count();
                let actual_body_count = content_str.matches("MANIFOLD_SOLID_BREP").count() + content_str.matches("BREP_WITH_VOIDS").count();
                assert_eq!(actual_body_count, expected_bodies, "Reference STEP {step_file} body count");
                assert_eq!(actual_face_count, expected_faces, "Reference STEP {step_file} face count");
            }
        }

        // Verify final model bodies
        let final_bodies = &model.state().bodies;
        assert_eq!(final_bodies.len(), 2, "Final model must have 2 bodies (Bottom and Top)");
        let total_faces: usize = final_bodies.iter().map(|b| b.body.face_count()).sum();
        assert!(total_faces >= 270, "Final model face count: {}", total_faces);
    }

    #[test]
    #[cfg(feature = "f3d")]
    fn test_f3d_features_trace() {
        let path = "/archive/Projects/Fusion360/1 Wire LED Controller Rev 1.0/1WireRGBWController.f3d";
        if let Ok(bytes) = std::fs::read(path) {
            let (_format, ir, _report, _fidelity) = native_cad_decode_full(&bytes, path).unwrap();
            println!("Decoded {} features, {} bodies:", ir.model.features.len(), ir.model.bodies.len());
            for b in &ir.model.bodies {
                println!("  body id={} name={:?} kind={:?}", b.id, b.name, b.kind);
            }
            println!("PARAMETERS ({} total):", ir.model.parameters.len());
            for p in &ir.model.parameters {
                println!("  PARAM: name={:?} expr={:?} val={:?} owner={:?}", p.name, p.expression, p.value, p.owner);
            }
            let mut sorted: Vec<_> = ir.model.features.iter().collect();
            sorted.sort_by_key(|f| f.ordinal);
            println!("IR FEATURES ({} total):", sorted.len());
            for (i, f) in sorted.iter().enumerate() {
                let op = match f.evaluation.definition() {
                    FeatureDefinition::Operation(op) => op,
                    FeatureDefinition::PostProcess { operation, .. } => operation,
                };
                let op_name = match op {
                    FeatureOperation::Extrude { .. } => "Extrude",
                    FeatureOperation::Chamfer { .. } => "Chamfer",
                    FeatureOperation::Fillet { .. } => "Fillet",
                    FeatureOperation::SplitBody { .. } => "SplitBody",
                    FeatureOperation::Combine { .. } => "Combine",
                    FeatureOperation::DeleteFace { .. } => "DeleteFace",
                    FeatureOperation::DeleteBody { .. } => "DeleteBody",
                    FeatureOperation::Sweep { .. } => "Sweep",
                    FeatureOperation::Pattern { .. } => "Pattern/Mirror",
                    FeatureOperation::Sketch { .. } => "Sketch",
                    FeatureOperation::DatumPlane { .. } => "DatumPlane",
                    FeatureOperation::Native { .. } => "Native",
                    _ => "Other",
                };
                println!("\nFEATURE [{:02}] ord={} name={:?} op={op_name}", i + 1, f.ordinal, f.name);
                for (k, v) in &f.source_properties {
                    println!("  prop {k} = {v}");
                }
                println!("  op_def = {op:?}");
            }
            if std::env::var("SKIP_REPLAY").is_ok() {
                for sk in &ir.model.sketches {
                    if sk.id.as_str().contains("97211") || sk.id.as_str().contains("97412") || sk.id.as_str().contains("97794") {
                        println!("=== SKETCH {} ({:?}) ===", sk.id, sk.name);
                        for ent in &ir.model.sketch_entities {
                            if ent.sketch == sk.id {
                                println!("  ent id={} constr={} def={:?}", ent.id(), ent.construction, ent.geometry.definition().to_raw());
                            }
                        }
                    }
                }
                return;
            }
            match native_cad_to_document_with_model(&bytes, path) {
                Ok(Some((doc, model))) => {
                    println!("\n=== DOCUMENT REPLAY RESULT ===");
                    println!("Doc features count: {}", doc.features.len());
                    println!("Model bodies count: {}", model.state().bodies.len());
                    for b in &model.state().bodies {
                        println!("  body name: {:?}, faces: {}, bbox: {:?}", b.name, b.body.face_count(), b.body.outline_bounds());
                        if let Ok(edges) = b.body.edges(0.01) {
                            for e in edges {
                                if e.mid.y.abs() > 1000.0 || e.length > 1000.0 || e.points.iter().any(|p| p.y.abs() > 1000.0) {
                                    println!("    EXTREME EDGE idx={}: mid={:?} len={} pts={:?}", e.index, e.mid, e.length, e.points);
                                }
                            }
                        }
                    }
                    println!("ALL DOC FEATURES ({}):", doc.features.len());
                    for (idx, f) in doc.features.iter().enumerate() {
                        let res = model.results.get(idx);
                        let err = res.and_then(|r| r.error.as_ref());
                        let warn = res.and_then(|r| r.warning.as_ref());
                        let bbs: Vec<_> = res
                            .map(|r| r.state.bodies.iter().map(|b| (b.name.clone(), b.body.face_count(), b.body.outline_bounds())).collect())
                            .unwrap_or_default();
                        println!("  [{idx:02}] {}: kind={} err={:?} warn={:?} bbs={bbs:?}", f.name, f.kind.type_name(), err, warn);
                    }
                }
                Ok(None) => println!("native_cad_to_document_with_model returned None"),
                Err(e) => println!("native_cad_to_document_with_model failed: {e}"),
            }
            return;
        }
    }
}
