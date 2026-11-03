//! Reverse engineering: classify an imported dumb solid, rebuild the flat-plate
//! and turned-shaft cases.
//!
//! `reverse.classify` is read-only evidence: it measures one imported body and
//! reports whether it is a flat plate (rectangular outline, uniform thickness,
//! round through-holes) or a turned shaft (coaxial Z cylinder steps with flat
//! ends), with the numbers a rebuild needs. Anything else is a refusal with
//! the reason, never a guess. `reverse.rebuild_plate` / `reverse.rebuild_turned`
//! turn that evidence into a parametric tree so the part can be edited;
//! `dry_run` returns the recipe without building, which is the human/LLM
//! correction hook.

use super::CommandSpec;
use crate::params::{bad, num, string_list};
use crate::{Result, Session};
use serde_json::{Value, json};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("reverse.classify", "Classify Imported Part", classify).icon("inspect").noundo().params(
        "body?: name (default: the only body, else name one) → flat plate, bent sheet or turned shaft: \
         category, dimensions, holes/segments, reasons, unmatched faces",
    ),
    CommandSpec::new("reverse.rebuild_plate", "Rebuild Plate Parametrically", rebuild_plate).icon("box").params(
        "source?: body (default: the only body); holes?: keep detected holes (default true); \
         dry_run?: return the recipe without building (default false); name?: new body name \
         → the parametric rebuild (a box plus one hole feature per hole)",
    ),
    CommandSpec::new("reverse.rebuild_turned", "Rebuild Shaft Parametrically", rebuild_turned).icon("cylinder").params(
        "source?: body (default: the only body); \
         dry_run?: return the recipe without building (default false); name?: new body name \
         → the parametric rebuild (one cylinder feature per shaft step)",
    ),
    CommandSpec::new("reverse.rebuild_bent_sheet", "Rebuild Bent Sheet Parametrically", rebuild_bent_sheet).icon("flange").params(
        "source?: body (default: the only body); holes?: keep detected holes (default true); \
         dry_run?: return the recipe without building (default false); name?: new body name \
         → the parametric rebuild (sheet rule + base flange + edge flange + cutouts)",
    ),
];

/// Biggest gap the plate search tolerates, in multiples of `tolerance_mm`.
const COAXIAL_SLACK: f64 = 10.0;
/// How parallel two plate faces must be: |dot| above this.
const PARALLEL_DOT: f64 = 0.999;
/// How square a side face must be to the plate: |dot| below this.
const SQUARE_DOT: f64 = 0.001;
/// How perpendicular two leg normals must be to count as one bend.
const BENT_SQUARE: f64 = 0.02;
/// A sheet is thin: its thickness is at most this fraction of the part's
/// smallest extent. Keeps square plates and cubes out of the bent branch.
const SHEET_THIN_FRAC: f64 = 0.25;

fn v3(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array()?;
    if a.len() != 3 {
        return None;
    }
    let mut out = [0.0; 3];
    for (i, x) in a.iter().enumerate() {
        out[i] = x.as_f64()?;
        if !out[i].is_finite() {
            return None;
        }
    }
    Some(out)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The single body to work on: named, or the only one when the design holds one.
fn target_body(s: &mut Session, cmd: &str, p: &Value) -> Result<String> {
    let mut want = string_list(cmd, p, "bodies")?;
    want.extend(string_list(cmd, p, "body")?);
    let measured = s.execute("inspect.measure", &json!({}))?;
    let mut names: Vec<String> = measured
        .get("bodies")
        .and_then(Value::as_array)
        .map(|bs| bs.iter().filter_map(|b| b.get("name")?.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    names.retain(|n| want.is_empty() || want.contains(n));
    if names.is_empty() {
        return Err(bad(cmd, "no bodies to classify"));
    }
    if names.len() > 1 {
        return Err(bad(cmd, format!("name one body: {}", names.join(", "))));
    }
    Ok(names.remove(0))
}

fn tolerance_mm(p: &Value, cmd: &str) -> Result<f64> {
    let t = num(p, "tolerance_mm").unwrap_or(0.01);
    if !t.is_finite() || t < 1e-6 || t > 10.0 {
        return Err(bad(cmd, "`tolerance_mm` must be finite, 1e-6…10"));
    }
    Ok(t)
}

fn classify_body(s: &mut Session, body: &str, tol: f64) -> Result<Value> {
    let mut reasons: Vec<Value> = Vec::new();
    let mut unmatched: Vec<Value> = Vec::new();
    let refuse = |reasons: &[Value], unmatched: &[Value], why: &str| {
        let mut reasons = reasons.to_vec();
        reasons.push(json!(why));
        json!({"body": body, "supported": false, "category": "other",
               "reasons": reasons, "unmatched": unmatched})
    };

    let faces_v = s.execute("model.faces", &json!({"body": body}))?;
    let faces: Vec<&Value> = faces_v.get("faces").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    if faces.is_empty() {
        return Ok(refuse(&reasons, &unmatched, "the body has no faces"));
    }
    let measure = s.execute("inspect.measure", &json!({"bodies": [body]}))?;
    let measured = measure.get("bodies").and_then(Value::as_array).and_then(|a| a.first());
    let bbox_min = measured.and_then(|m| m.get("bbox")).and_then(|b| b.get("min")).and_then(v3);
    let bbox_max = measured.and_then(|m| m.get("bbox")).and_then(|b| b.get("max")).and_then(v3);
    let (Some(bmin), Some(bmax)) = (bbox_min, bbox_max) else {
        return Ok(refuse(&reasons, &unmatched, "the body has no bounding box"));
    };
    let size = [bmax[0] - bmin[0], bmax[1] - bmin[1], bmax[2] - bmin[2]];

    // A shaft is all cylinder walls plus flat ends; check it before the plate
    // branch so a plain rod is not mistaken for a plate with one big hole.
    if let Some(turned) = classify_turned(&faces, bmin, bmax, size, tol, body, &mut reasons) {
        return Ok(turned);
    }

    // A bent bracket is two perpendicular thin planar pairs plus one bend
    // cylinder pair; anything plate-like falls through to the plate branch.
    if let Some(bent) = classify_bent(&faces, bmin, bmax, size, tol, body, &mut reasons) {
        return Ok(bent);
    }

    // Plate faces: the two largest planar faces with opposite normals.
    let mut normals: Vec<([f64; 3], [f64; 3], f64, usize)> = Vec::new();
    for f in &faces {
        if let (Some(n), Some(pt)) = (f.get("plane_normal").and_then(v3), f.get("point").and_then(v3)) {
            let area = f.get("area").and_then(Value::as_f64).unwrap_or(0.0);
            let idx = f.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            normals.push((n, pt, area, idx));
        }
    }
    normals.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    let Some((n0, p0, _, _)) = normals.first().copied() else {
        return Ok(refuse(&reasons, &unmatched, "no planar faces"));
    };
    let mate = normals.iter().find(|(n, _, _, _)| dot(*n, n0) < -PARALLEL_DOT);
    let Some((n1, p1, _, _)) = mate.copied() else {
        return Ok(refuse(&reasons, &unmatched, "no opposing parallel face pair"));
    };
    let axis = if dot(n0, [0.0, 0.0, 1.0]).abs() > PARALLEL_DOT {
        [0.0, 0.0, 1.0]
    } else {
        return Ok(refuse(&reasons, &unmatched, "the plate is not Z-aligned"));
    };
    let _ = n1;
    let thickness = ((p1[0] - p0[0]) * axis[0] + (p1[1] - p0[1]) * axis[1] + (p1[2] - p0[2]) * axis[2]).abs();
    if thickness <= 0.0 || (thickness - size[2]).abs() > tol.max(size[2] * 0.001) {
        return Ok(refuse(&reasons, &unmatched, "plate faces disagree with the bounding box"));
    }
    let top_z = bmax[2];
    let mid_z = (bmin[2] + bmax[2]) / 2.0;
    reasons.push(json!(format!("two opposing planar faces {thickness:.3} mm apart, Z-aligned")));

    // Remaining faces: sides must be square, cylinders must be full-height holes.
    let mut holes: Vec<Value> = Vec::new();
    let mut bores: Vec<([f64; 3], [f64; 3], f64, [f64; 3])> = Vec::new();
    for f in &faces {
        let area = f.get("area").and_then(Value::as_f64).unwrap_or(0.0);
        let idx = f.get("index").and_then(Value::as_u64).unwrap_or(0);
        if let Some(n) = f.get("plane_normal").and_then(v3) {
            let dn = dot(n, axis).abs();
            let coplanar = f.get("point").and_then(v3).map(|pt| ((pt[2] - bmin[2]).abs(), (pt[2] - bmax[2]).abs()));
            let on_plate = coplanar.map(|(lo, hi)| lo.min(hi) < tol).unwrap_or(false);
            if dn > PARALLEL_DOT && on_plate {
                continue;
            }
            if dn < SQUARE_DOT {
                continue;
            }
            unmatched.push(json!({"index": idx, "area": area, "why": "non-plate planar face"}));
            continue;
        }
        if let Some(cyl) = f.get("cylinder") {
            let (Some(ax), Some(org), Some(r)) =
                (cyl.get("axis").and_then(v3), cyl.get("origin").and_then(v3), cyl.get("radius").and_then(Value::as_f64))
            else {
                unmatched.push(json!({"index": idx, "area": area, "why": "cylinder without axis"}));
                continue;
            };
            let alen = (ax[0] * ax[0] + ax[1] * ax[1] + ax[2] * ax[2]).sqrt();
            if alen <= 0.0 || r <= 0.0 {
                unmatched.push(json!({"index": idx, "area": area, "why": "degenerate cylinder"}));
                continue;
            }
            let ax = [ax[0] / alen, ax[1] / alen, ax[2] / alen];
            if (dot(ax, axis).abs()) < PARALLEL_DOT {
                unmatched.push(json!({"index": idx, "area": area, "why": "off-axis cylindrical face"}));
                continue;
            }
            let h = area / (2.0 * std::f64::consts::PI * r);
            let centroid = f.get("centroid").and_then(v3).unwrap_or(org);
            if (h - thickness).abs() > tol.max(thickness * 0.01) || (centroid[2] - mid_z).abs() > thickness * 0.05 + tol {
                unmatched.push(json!({"index": idx, "area": area, "why": "blind or partial cylindrical face"}));
                continue;
            }
            bores.push((ax, org, r, centroid));
            continue;
        }
        unmatched.push(json!({"index": idx, "area": area, "why": "non-planar, non-cylindrical face"}));
    }
    // One hole per axis line: entry and exit walls share it.
    let mut used = vec![false; bores.len()];
    for i in 0..bores.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let (ax, org, r, _) = bores[i];
        let mut members = 1;
        for j in (i + 1)..bores.len() {
            if used[j] || (bores[j].2 - r).abs() > tol {
                continue;
            }
            let w = sub(bores[j].1, org);
            let along = dot(w, ax);
            let d2 = dot(w, w) - along * along;
            if d2 <= (COAXIAL_SLACK * tol) * (COAXIAL_SLACK * tol) {
                used[j] = true;
                members += 1;
            }
        }
        let _ = members;
        // Project the bore centroid onto the top face for the drill point.
        let c = bores[i].3;
        let center = [c[0], c[1], top_z];
        holes.push(json!({"center": center, "diameter": 2.0 * r}));
    }
    if !unmatched.is_empty() {
        return Ok(refuse(&reasons, &unmatched, "faces the flat-plate recipe cannot explain (see unmatched)"));
    }
    reasons.push(json!(format!("{} through-hole(s)", holes.len())));
    Ok(json!({"body": body, "supported": true, "category": "flat_plate",
              "length": size[0], "width": size[1], "thickness": thickness,
              "corner": bmin, "top_z": top_z,
              "holes": holes, "reasons": reasons, "unmatched": unmatched}))
}

/// Stepped-shaft check: every face is a Z-parallel cylinder wall or a Z-normal
/// planar ring (flat ends and shoulders). Returns `Some` when the body has the
/// shaft signature — supported with the step list, or refused with the reason —
/// and `None` when it is not a shaft at all (the plate branch runs next).
#[allow(clippy::too_many_arguments)]
fn classify_turned(
    faces: &[&Value],
    bmin: [f64; 3],
    bmax: [f64; 3],
    size: [f64; 3],
    tol: f64,
    body: &str,
    reasons: &mut Vec<Value>,
) -> Option<Value> {
    let axis = [0.0, 0.0, 1.0];
    let refuse = |reasons: &mut Vec<Value>, why: &str| {
        reasons.push(json!(why));
        Some(json!({"body": body, "supported": false, "category": "other",
                    "reasons": reasons, "unmatched": []}))
    };
    // (radius, z0, z1, centre-x, centre-y) per wall, in face order.
    let mut walls: Vec<(f64, f64, f64, f64, f64)> = Vec::new();
    let mut ends = 0;
    for f in faces {
        if let Some(n) = f.get("plane_normal").and_then(v3) {
            if dot(n, axis).abs() < PARALLEL_DOT {
                return None; // a side face: not a shaft
            }
            ends += 1;
            continue;
        }
        if let Some(cyl) = f.get("cylinder") {
            let (Some(ax), Some(org), Some(r)) =
                (cyl.get("axis").and_then(v3), cyl.get("origin").and_then(v3), cyl.get("radius").and_then(Value::as_f64))
            else {
                return None;
            };
            let alen = (ax[0] * ax[0] + ax[1] * ax[1] + ax[2] * ax[2]).sqrt();
            let area = f.get("area").and_then(Value::as_f64).unwrap_or(0.0);
            if alen <= 0.0 || r <= 0.0 || area <= 0.0 || dot([ax[0] / alen, ax[1] / alen, ax[2] / alen], axis).abs() < PARALLEL_DOT {
                return None; // degenerate or off-axis wall: not a shaft
            }
            let h = area / (2.0 * std::f64::consts::PI * r);
            let c = f.get("centroid").and_then(v3).unwrap_or(org);
            walls.push((r, c[2] - h / 2.0, c[2] + h / 2.0, c[0], c[1]));
            continue;
        }
        return None; // cone, sphere, anything else: not a shaft
    }
    if walls.is_empty() {
        return None;
    }
    if ends < 2 {
        return refuse(reasons, "shaft walls without both flat end faces");
    }
    // All walls share one axis line.
    let (r0, _, _, cx, cy) = walls[0];
    let _ = r0;
    for (r, _, _, x, y) in &walls {
        let _ = r;
        let dx = x - cx;
        let dy = y - cy;
        let slack = COAXIAL_SLACK * tol;
        if dx * dx + dy * dy > slack * slack {
            return refuse(reasons, "cylinder walls are not coaxial");
        }
    }
    // Steps tile the full height with no gaps. Tessellated wall areas read a
    // touch short, so the coverage slack grows with the part, not just `tol`.
    let slack = tol.max(0.005 * size[2]);
    walls.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    if (walls[0].1 - bmin[2]).abs() > slack {
        return refuse(reasons, "lowest wall does not reach the bottom face");
    }
    if (walls[walls.len() - 1].2 - bmax[2]).abs() > slack {
        return refuse(reasons, "highest wall does not reach the top face");
    }
    for w in walls.windows(2) {
        if w[1].1 - w[0].2 > slack {
            return refuse(reasons, "gap between shaft steps the recipe cannot bridge");
        }
    }
    // Snap the outer bounds to the measured box and split shared joints, so the
    // rebuild volumes match instead of carrying tessellation error.
    walls[0].1 = bmin[2];
    let last = walls.len() - 1;
    walls[last].2 = bmax[2];
    for i in 0..last {
        let mid = (walls[i].2 + walls[i + 1].1) / 2.0;
        walls[i].2 = mid;
        walls[i + 1].1 = mid;
    }
    let segments: Vec<Value> = walls.iter().map(|(r, z0, z1, _, _)| json!({"radius": r, "z0": z0, "z1": z1})).collect();
    reasons.push(json!(format!("{} coaxial Z wall(s), flat ends, {:.3} mm tall", walls.len(), size[2])));
    Some(json!({"body": body, "supported": true, "category": "turned",
                "length": size[2], "center": [cx, cy], "bottom": bmin[2],
                "segments": segments, "reasons": reasons, "unmatched": []}))
}

/// Bent-sheet check: two perpendicular thin planar pairs (the legs) plus
/// exactly one bend cylinder pair. Returns `Some` when the body is sheet-like
/// — supported with legs, bend and holes, or refused with the reason — and
/// `None` when it is not sheet at all (plate-like input falls through).
#[allow(clippy::too_many_arguments)]
fn classify_bent(faces: &[&Value], bmin: [f64; 3], bmax: [f64; 3], size: [f64; 3], tol: f64, body: &str, reasons: &mut Vec<Value>) -> Option<Value> {
    let refuse = |reasons: &mut Vec<Value>, why: &str| {
        reasons.push(json!(why));
        Some(json!({"body": body, "supported": false, "category": "other",
                    "reasons": reasons, "unmatched": []}))
    };
    let center_all = [(bmin[0] + bmax[0]) / 2.0, (bmin[1] + bmax[1]) / 2.0, (bmin[2] + bmax[2]) / 2.0];
    // Planar faces by dominant axis; slanted ones can never belong to a bracket.
    let mut axis_faces: Vec<Vec<([f64; 3], [f64; 3], f64, u64)>> = vec![Vec::new(), Vec::new(), Vec::new()];
    let mut slanted = 0;
    for f in faces {
        if let (Some(n), Some(pt)) = (f.get("plane_normal").and_then(v3), f.get("point").and_then(v3)) {
            let area = f.get("area").and_then(Value::as_f64).unwrap_or(0.0);
            let idx = f.get("index").and_then(Value::as_u64).unwrap_or(0);
            let mut dom = 0;
            let mut best = n[0].abs();
            for a in 1..3 {
                if n[a].abs() > best {
                    best = n[a].abs();
                    dom = a;
                }
            }
            if best < PARALLEL_DOT {
                slanted += 1;
                continue;
            }
            axis_faces[dom].push((n, pt, area, idx));
        }
    }
    // Opposing pairs per orientation.
    // (axis, normal, point_a, point_b, gap)
    let mut pairs: Vec<(usize, [f64; 3], [f64; 3], [f64; 3], f64)> = Vec::new();
    let mut strips: Vec<([f64; 3], [f64; 3], f64, u64)> = Vec::new();
    for (ax, group) in axis_faces.iter().enumerate() {
        let mut used = vec![false; group.len()];
        for i in 0..group.len() {
            if used[i] {
                continue;
            }
            let mut mate = None;
            for j in (i + 1)..group.len() {
                if used[j] || dot(group[i].0, group[j].0) > -PARALLEL_DOT {
                    continue;
                }
                mate = Some(j);
                break;
            }
            if let Some(j) = mate {
                used[i] = true;
                used[j] = true;
                let gap = dot(sub(group[j].1, group[i].1), group[i].0).abs();
                pairs.push((ax, group[i].0, group[i].1, group[j].1, gap));
            }
        }
        for (i, g) in group.iter().enumerate() {
            if !used[i] {
                strips.push((g.0, g.1, g.2, g.3));
            }
        }
    }
    // Legs are the one gap value shared by exactly two pairs.
    let mut legs: Option<((usize, [f64; 3], [f64; 3], [f64; 3], f64), (usize, [f64; 3], [f64; 3], [f64; 3], f64))> = None;
    for i in 0..pairs.len() {
        for j in (i + 1)..pairs.len() {
            if (pairs[i].4 - pairs[j].4).abs() > tol {
                continue;
            }
            if pairs[i].0 == pairs[j].0 {
                continue; // stacked plates, not a bend
            }
            if legs.is_some() {
                return refuse(reasons, "more than two sheet flanges share one thickness");
            }
            legs = Some((pairs[i], pairs[j]));
        }
    }
    // Three or more pairs on one thickness is a tray, not a bracket.
    if legs.is_none() {
        let mut counts: Vec<(f64, usize)> = Vec::new();
        for p in &pairs {
            if let Some(c) = counts.iter_mut().find(|(g, _)| (g - p.4).abs() <= tol) {
                c.1 += 1;
            } else {
                counts.push((p.4, 1));
            }
        }
        if counts.iter().any(|(_, c)| *c > 2) {
            return refuse(reasons, "multi-flange tray: only single bends are supported");
        }
        return None; // not sheet at all: plate branch decides
    }
    let (pa, pb) = legs.unwrap();
    let t = (pa.4 + pb.4) / 2.0;
    if t <= 0.0 || t > SHEET_THIN_FRAC * size[0].min(size[1]).min(size[2]) {
        return None; // chunky, not sheet
    }
    if dot(pa.1, pb.1).abs() > BENT_SQUARE {
        return None; // legs are not perpendicular
    }
    if slanted > 0 {
        return refuse(reasons, "slanted planar faces a single bend cannot explain");
    }
    // Leg 1 is the horizontal (Z-normal) pair; up-bends only for now.
    let (leg1, leg2) = if pa.0 == 2 {
        (pa, pb)
    } else if pb.0 == 2 {
        (pb, pa)
    } else {
        return refuse(reasons, "no horizontal leg to build the base from");
    };
    let bbox_c = center_all;
    let outward = |n: [f64; 3], pt: [f64; 3]| dot(n, sub(pt, bbox_c)) > 0.0;
    let (n1o, pt1o) = if outward(leg1.1, leg1.2) { (leg1.1, leg1.2) } else { ([-leg1.1[0], -leg1.1[1], -leg1.1[2]], leg1.3) };
    let (n2o, pt2o) = if outward(leg2.1, leg2.2) { (leg2.1, leg2.2) } else { ([-leg2.1[0], -leg2.1[1], -leg2.1[2]], leg2.3) };
    if (pt1o[2] - bmin[2]).abs() > tol && (pt1o[2] - bmax[2]).abs() > tol {
        return refuse(reasons, "leg-1 outer face is not on the part boundary");
    }
    if (pt1o[2] - bmax[2]).abs() <= tol {
        return refuse(reasons, "downward bend: only upward bends are supported");
    }
    // Cylinders: tall ones are the bend, leg-normal ones through-holes.
    let mut bend_walls: Vec<([f64; 3], [f64; 3], f64, f64, [f64; 3])> = Vec::new(); // ax, org, r, h, centroid
    let mut hole_cyls: Vec<([f64; 3], [f64; 3], f64, [f64; 3], usize)> = Vec::new(); // ax, org, r, centroid, leg
    let leg_normals = [n1o, n2o];
    for f in faces {
        if let Some(cyl) = f.get("cylinder") {
            let area = f.get("area").and_then(Value::as_f64).unwrap_or(0.0);
            let (Some(ax), Some(org), Some(r)) =
                (cyl.get("axis").and_then(v3), cyl.get("origin").and_then(v3), cyl.get("radius").and_then(Value::as_f64))
            else {
                return refuse(reasons, "cylinder without axis");
            };
            let alen = (ax[0] * ax[0] + ax[1] * ax[1] + ax[2] * ax[2]).sqrt();
            if alen <= 0.0 || r <= 0.0 || area <= 0.0 {
                return refuse(reasons, "degenerate cylindrical face");
            }
            let ax = [ax[0] / alen, ax[1] / alen, ax[2] / alen];
            let h = area / (2.0 * std::f64::consts::PI * r);
            let c = f.get("centroid").and_then(v3).unwrap_or(org);
            let mut leg_hit = None;
            for (li, ln) in leg_normals.iter().enumerate() {
                if dot(ax, *ln).abs() > PARALLEL_DOT {
                    leg_hit = Some(li);
                    break;
                }
            }
            if let Some(li) = leg_hit {
                if (h - t).abs() > tol.max(0.01 * t) {
                    return refuse(reasons, "blind or partial cylindrical face");
                }
                hole_cyls.push((ax, org, r, c, li));
                continue;
            }
            if dot(ax, leg_normals[0]).abs() < BENT_SQUARE && dot(ax, leg_normals[1]).abs() < BENT_SQUARE {
                bend_walls.push((ax, org, r, h, c));
                continue;
            }
            return refuse(reasons, "cylindrical face the single-bend recipe cannot explain");
        } else if f.get("plane_normal").is_none() {
            return refuse(reasons, "non-planar, non-cylindrical face");
        }
    }
    if bend_walls.len() != 2 {
        return refuse(reasons, "a single bend needs exactly one inner and one outer wall");
    }
    let (ax0, org0, r0, h0, _) = bend_walls[0];
    let (_, org1, r1, h1, _) = bend_walls[1];
    let (r_in, r_out) = if r0 < r1 { (r0, r1) } else { (r1, r0) };
    if ((r_out - r_in) - t).abs() > tol {
        return refuse(reasons, "bend walls are not one thickness apart");
    }
    if dot(ax0, bend_walls[1].0).abs() < PARALLEL_DOT {
        return refuse(reasons, "bend walls are not coaxial");
    }
    let w = sub(org1, org0);
    let along = dot(w, ax0);
    let d2 = dot(w, w) - along * along;
    if d2 > (COAXIAL_SLACK * tol) * (COAXIAL_SLACK * tol) {
        return refuse(reasons, "bend walls are not coaxial");
    }
    if ax0[2].abs() > SQUARE_DOT {
        return refuse(reasons, "bend axis is not horizontal");
    }
    let axis_kind = if ax0[0].abs() > PARALLEL_DOT {
        0
    } else if ax0[1].abs() > PARALLEL_DOT {
        1
    } else {
        return refuse(reasons, "bend axis is not X/Y aligned");
    };
    let width = if axis_kind == 0 { size[0] } else { size[1] };
    if (h0 - width).abs() > tol.max(0.01 * width) || (h1 - width).abs() > tol.max(0.01 * width) {
        return refuse(reasons, "bend walls do not span the full sheet width");
    }
    // Side strips run along the axis, end strips sit square to a leg.
    for (n, pt, _area, idx) in &strips {
        if dot(*n, ax0).abs() > PARALLEL_DOT {
            continue; // side strip along the bend
        }
        let mut square = false;
        for ln in &leg_normals {
            if dot(*n, *ln).abs() < SQUARE_DOT {
                square = true;
                break;
            }
        }
        if square {
            continue; // end strip
        }
        let _ = pt;
        let _ = idx;
        return refuse(reasons, "planar face the single-bend recipe cannot explain");
    }
    // Leg dimensions and the bend edge point on the outer face.
    let mid = [(bmin[0] + bmax[0]) / 2.0, (bmin[1] + bmax[1]) / 2.0, (bmin[2] + bmax[2]) / 2.0];
    let s = dot(sub(mid, org0), ax0);
    let q = [org0[0] + ax0[0] * s, org0[1] + ax0[1] * s, org0[2] + ax0[2] * s];
    let d_out = dot(sub(pt1o, q), n1o);
    let edge = [q[0] + n1o[0] * d_out, q[1] + n1o[1] * d_out, q[2] + n1o[2] * d_out];
    let (leg1_len, edge_coord, min_c, max_c) =
        if axis_kind == 0 { (size[1], edge[1], bmin[1], bmax[1]) } else { (size[0], edge[0], bmin[0], bmax[0]) };
    let dir_sign = if (edge_coord - min_c).abs() <= (edge_coord - max_c).abs() { 1.0 } else { -1.0 };
    let leg2_len = size[2] - t;
    let bend_angle = dot(n1o, n2o).clamp(-1.0, 1.0).acos().to_degrees();
    // Holes per leg, grouped by axis line like plate bores.
    let mut holes: Vec<Value> = Vec::new();
    let mut used = vec![false; hole_cyls.len()];
    for i in 0..hole_cyls.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let (ax, org, r, _, li) = hole_cyls[i];
        let mut members = 0;
        for j in (i + 1)..hole_cyls.len() {
            if used[j] || hole_cyls[j].4 != li || (hole_cyls[j].2 - r).abs() > tol {
                continue;
            }
            let w = sub(hole_cyls[j].1, org);
            let along = dot(w, ax);
            let d2 = dot(w, w) - along * along;
            if d2 <= (COAXIAL_SLACK * tol) * (COAXIAL_SLACK * tol) {
                used[j] = true;
                members += 1;
            }
        }
        let _ = members;
        let c = hole_cyls[i].3;
        let (ln_o, pt_o) = if li == 0 { (n1o, pt1o) } else { (n2o, pt2o) };
        let d = dot(sub(pt_o, c), ln_o);
        let drilled = [c[0] + ln_o[0] * d, c[1] + ln_o[1] * d, c[2] + ln_o[2] * d];
        holes.push(json!({"leg": li, "center": drilled, "diameter": 2.0 * r}));
    }
    reasons.push(json!(format!("two {t:.3} mm legs at {bend_angle:.1}°, bend radius {r_in:.3} mm")));
    reasons.push(json!(format!("{} through-hole(s)", holes.len())));
    let (outer0, outer1) = (pt1o, pt2o);
    let _ = outer1;
    Some(json!({"body": body, "supported": true, "category": "bent_sheet",
              "thickness": t, "bend_radius": r_in, "bend_angle_deg": bend_angle,
              "bend_axis": ax0, "bend_center": q,
              "legs": [{"length": leg1_len, "width": width}, {"length": leg2_len, "width": width}],
              "bend_edge": edge, "edge_dir_sign": dir_sign, "axis_kind": axis_kind,
              "leg1_outer": [n1o, outer0],
              "holes": holes, "reasons": reasons, "unmatched": []}))
}

fn classify(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "reverse.classify";
    let tol = tolerance_mm(p, cmd)?;
    let body = target_body(s, cmd, p)?;
    classify_body(s, &body, tol)
}

fn rebuild_plate(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "reverse.rebuild_plate";
    let tol = tolerance_mm(p, cmd)?;
    let source = target_body(s, cmd, p)?;
    let keep_holes = p.get("holes").and_then(Value::as_bool).unwrap_or(true);
    let dry_run = p.get("dry_run").and_then(Value::as_bool).unwrap_or(false);
    let name = p.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("{source}_plate"));

    let found = classify_body(s, &source, tol)?;
    if !found.get("supported").and_then(Value::as_bool).unwrap_or(false) {
        let why = found.get("reasons").and_then(Value::as_array).and_then(|r| r.first()).cloned().unwrap_or(json!("unsupported shape"));
        return Err(crate::params::bad(cmd, format!("not supported yet: {why}")));
    }
    let get = |k: &str| found.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let corner = found.get("corner").and_then(v3).unwrap_or([0.0, 0.0, 0.0]);
    let mut recipe: Vec<Value> = vec![json!({"id": "solid.box", "params":
        {"length": get("length"), "width": get("width"), "height": get("thickness"),
         "corner": corner, "body_name": name}})];
    let mut holes: Vec<&Value> = found.get("holes").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    if !keep_holes {
        holes.clear();
    }
    for h in &holes {
        let center = h.get("center").and_then(v3).unwrap_or(corner);
        let diameter = h.get("diameter").and_then(Value::as_f64).unwrap_or(0.0);
        recipe.push(json!({"id": "solid.hole", "params":
            {"position": center, "diameter": diameter, "targets": [name]}}));
    }
    if dry_run {
        return Ok(json!({"dry_run": true, "recipe": recipe, "found": found}));
    }
    let mut built = 0;
    for step in &recipe {
        let id = step.get("id").and_then(Value::as_str).unwrap_or("");
        let params = step.get("params").cloned().unwrap_or(json!({}));
        s.execute(id, &params)?;
        built += 1;
    }
    Ok(json!({"body": name, "holes_built": holes.len(), "steps_built": built, "recipe": recipe}))
}

fn rebuild_turned(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "reverse.rebuild_turned";
    let tol = tolerance_mm(p, cmd)?;
    let source = target_body(s, cmd, p)?;
    let dry_run = p.get("dry_run").and_then(Value::as_bool).unwrap_or(false);
    let name = p.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("{source}_shaft"));

    let found = classify_body(s, &source, tol)?;
    if found.get("category").and_then(Value::as_str) != Some("turned") || !found.get("supported").and_then(Value::as_bool).unwrap_or(false) {
        let why = found.get("reasons").and_then(Value::as_array).and_then(|r| r.last()).cloned().unwrap_or(json!("unsupported shape"));
        return Err(crate::params::bad(cmd, format!("not supported yet: {why}")));
    }
    let center = found.get("center").and_then(v3).unwrap_or([0.0, 0.0, 0.0]);
    let segments: Vec<&Value> = found.get("segments").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    let mut recipe: Vec<Value> = Vec::new();
    for (i, seg) in segments.iter().enumerate() {
        let r = seg.get("radius").and_then(Value::as_f64).unwrap_or(0.0);
        let z0 = seg.get("z0").and_then(Value::as_f64).unwrap_or(0.0);
        let z1 = seg.get("z1").and_then(Value::as_f64).unwrap_or(0.0);
        let mut params = json!(
            {"radius": r, "height": z1 - z0, "base": [center[0], center[1], z0],
             "axis": "Z", "body_name": name});
        if i > 0 {
            // Same name alone does not fuse steps: join explicitly.
            params["operation"] = json!("join");
            params["targets"] = json!([name]);
        }
        recipe.push(json!({"id": "solid.cylinder", "params": params}));
    }
    if dry_run {
        return Ok(json!({"dry_run": true, "recipe": recipe, "found": found}));
    }
    let mut built = 0;
    for step in &recipe {
        let id = step.get("id").and_then(Value::as_str).unwrap_or("");
        let params = step.get("params").cloned().unwrap_or(json!({}));
        s.execute(id, &params)?;
        built += 1;
    }
    Ok(json!({"body": name, "segments_built": segments.len(), "steps_built": built, "recipe": recipe}))
}

fn rebuild_bent_sheet(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "reverse.rebuild_bent_sheet";
    let tol = tolerance_mm(p, cmd)?;
    let source = target_body(s, cmd, p)?;
    let keep_holes = p.get("holes").and_then(Value::as_bool).unwrap_or(true);
    let dry_run = p.get("dry_run").and_then(Value::as_bool).unwrap_or(false);
    let name = p.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("{source}_bent"));

    let found = classify_body(s, &source, tol)?;
    if found.get("category").and_then(Value::as_str) != Some("bent_sheet") || !found.get("supported").and_then(Value::as_bool).unwrap_or(false) {
        let why = found.get("reasons").and_then(Value::as_array).and_then(|r| r.last()).cloned().unwrap_or(json!("unsupported shape"));
        return Err(bad(cmd, format!("not supported yet: {why}")));
    }
    let get = |k: &str| found.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let t = get("thickness");
    let r = get("bend_radius");
    let deg = get("bend_angle_deg");
    let legs: Vec<&Value> = found.get("legs").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    if legs.len() != 2 {
        return Err(bad(cmd, "bent classification without two legs"));
    }
    let gl = |i: usize, k: &str| legs[i].get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let axis_kind = found.get("axis_kind").and_then(Value::as_u64).unwrap_or(0);
    let dir_sign = found.get("edge_dir_sign").and_then(Value::as_f64).unwrap_or(1.0);
    let edge = found.get("bend_edge").and_then(v3).unwrap_or([0.0, 0.0, 0.0]);
    // Base rectangle on the leg-1 outer face, running away from the bend edge.
    let (ox, oy, oz, px, py) = if axis_kind == 0 {
        let x0 = edge[0] - gl(0, "width") / 2.0;
        let y0 = if dir_sign > 0.0 { edge[1] } else { edge[1] - gl(0, "length") };
        (x0, y0, edge[2], gl(0, "width"), gl(0, "length"))
    } else {
        let y0 = edge[1] - gl(0, "width") / 2.0;
        let x0 = if dir_sign > 0.0 { edge[0] } else { edge[0] - gl(0, "length") };
        (x0, y0, edge[2], gl(0, "length"), gl(0, "width"))
    };
    let mut recipe: Vec<Value> = vec![
        json!({"id": "sheet.manage_rules", "params":
            {"name": "REV_BENT", "thickness": format!("{t} mm"), "bend_radius": format!("{r} mm"),
             "k_factor": 0.5, "active": true}}),
        json!({"id": "sketch.create", "params": {"plane": {"origin": [ox, oy, oz]}, "name": "RevBase"}}),
        json!({"id": "sketch.rectangle.two_point", "params": {"p0": [0, 0], "p1": [px, py]}}),
        json!({"id": "sketch.finish", "params": {}}),
        json!({"id": "sheet.flange", "params": {"sketch": "RevBase"}}),
        json!({"id": "sheet.flange", "params":
            {"edges": [[edge[0], edge[1], edge[2] + t]], "height": gl(1, "length"), "angle": format!("{deg} deg")}}),
    ];
    let mut holes: Vec<&Value> = found.get("holes").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    if !keep_holes {
        holes.clear();
    }
    for h in &holes {
        let center = h.get("center").and_then(v3).unwrap_or(edge);
        let diameter = h.get("diameter").and_then(Value::as_f64).unwrap_or(0.0);
        recipe.push(json!({"id": "solid.hole", "params":
            {"position": center, "diameter": diameter, "targets": [name]}}));
    }
    if dry_run {
        return Ok(json!({"dry_run": true, "recipe": recipe, "found": found}));
    }
    let mut built = 0;
    for step in &recipe {
        let id = step.get("id").and_then(Value::as_str).unwrap_or("");
        let params = step.get("params").cloned().unwrap_or(json!({}));
        s.execute(id, &params)?;
        built += 1;
    }
    Ok(json!({"body": name, "bends_built": 1, "holes_built": holes.len(), "steps_built": built, "recipe": recipe}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(s: &mut Session, c: &str, p: Value) -> Value {
        s.execute(c, &p).unwrap_or_else(|e| panic!("{c}: {e}"))
    }

    fn plate_session() -> Session {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 5, "corner": [0, 0, 0], "body_name": "Src"}));
        run(&mut s, "solid.hole", json!({"position": [10, 10, 5], "diameter": 6, "targets": ["Src"]}));
        run(&mut s, "solid.hole", json!({"position": [50, 30, 5], "diameter": 8, "targets": ["Src"]}));
        s
    }

    fn round_trip() -> (Session, String) {
        let mut s = plate_session();
        let dir = std::env::temp_dir().join("reverse_round_trip");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.step");
        run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "format": "step"}));
        let mut r = Session::default();
        let _ = run(&mut r, "file.insert_step", json!({"path": path.to_string_lossy()}));
        (r, "plate".to_string())
    }

    #[test]
    fn flat_plate_round_trip() {
        let (mut s, _hint) = round_trip();
        // The import names bodies after the file; find the single solid body.
        let bodies: Vec<String> = s.execute("inspect.measure", &json!({})).unwrap()["bodies"]
            .as_array()
            .map(|a| a.iter().filter_map(|b| b.get("name")?.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        assert_eq!(bodies.len(), 1, "one imported body, got {bodies:?}");
        let found = run(&mut s, "reverse.classify", json!({"body": bodies[0]}));
        assert_eq!(found["supported"], true, "classify: {found}");
        assert_eq!(found["category"], "flat_plate");
        assert!((found["thickness"].as_f64().unwrap_or(0.0) - 5.0).abs() < 0.05, "thickness: {found}");
        assert_eq!(found["holes"].as_array().map(Vec::len), Some(2), "holes: {found}");
        let dry = run(&mut s, "reverse.rebuild_plate", json!({"source": bodies[0], "dry_run": true}));
        assert_eq!(dry["recipe"].as_array().map(Vec::len), Some(3), "recipe: {dry}");
        let done = run(&mut s, "reverse.rebuild_plate", json!({"source": bodies[0], "name": "Plate2"}));
        assert_eq!(done["holes_built"], 2, "built: {done}");
        let back = s.execute("inspect.measure", &json!({"bodies": ["Plate2"]})).unwrap();
        let b = &back["bodies"][0];
        let vol = b["volume_mm3"].as_f64().unwrap_or(0.0);
        let hole_vol = std::f64::consts::PI * (3.0 * 3.0 + 4.0 * 4.0) * 5.0;
        assert!((vol - (60.0 * 40.0 * 5.0 - hole_vol)).abs() < 1.0, "volume: {b}");
    }

    #[test]
    fn rod_is_turned() {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 20, "body_name": "Rod"}));
        let found = run(&mut s, "reverse.classify", json!({"body": "Rod"}));
        assert_eq!(found["supported"], true, "classify: {found}");
        assert_eq!(found["category"], "turned");
        assert_eq!(found["segments"].as_array().map(Vec::len), Some(1), "segments: {found}");
        let dry = run(&mut s, "reverse.rebuild_turned", json!({"source": "Rod", "dry_run": true}));
        assert_eq!(dry["recipe"].as_array().map(Vec::len), Some(1), "recipe: {dry}");
        let done = run(&mut s, "reverse.rebuild_turned", json!({"source": "Rod", "name": "Rod2"}));
        assert_eq!(done["segments_built"], 1, "built: {done}");
        let back = s.execute("inspect.measure", &json!({"bodies": ["Rod2"]})).unwrap();
        let vol = back["bodies"][0]["volume_mm3"].as_f64().unwrap_or(0.0);
        assert!((vol - std::f64::consts::PI * 100.0 * 20.0).abs() < 1.0, "volume: {back}");
    }

    #[test]
    fn stepped_shaft_round_trip() {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 10, "base": [0, 0, 0], "body_name": "Shaft"}));
        run(
            &mut s,
            "solid.cylinder",
            json!({"radius": 6, "height": 10, "base": [0, 0, 10], "body_name": "Shaft", "operation": "join", "targets": ["Shaft"]}),
        );
        let dir = std::env::temp_dir().join("reverse_shaft_trip");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shaft.step");
        run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "format": "step"}));
        let mut r = Session::default();
        let _ = run(&mut r, "file.insert_step", json!({"path": path.to_string_lossy()}));
        let mut bodies: Vec<String> = r.execute("inspect.measure", &json!({})).unwrap()["bodies"]
            .as_array()
            .map(|a| a.iter().filter_map(|b| b.get("name")?.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        assert!(!bodies.is_empty(), "imported nothing");
        if bodies.len() > 1 {
            // Face-touching steps may import as separate bodies; fuse them.
            let tools: Vec<Value> = bodies[1..].iter().map(|b| json!(b)).collect();
            run(&mut r, "solid.combine", json!({"target": bodies[0], "tools": tools, "touching": "one_body"}));
            bodies = r.execute("inspect.measure", &json!({})).unwrap()["bodies"]
                .as_array()
                .map(|a| a.iter().filter_map(|b| b.get("name")?.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
        }
        assert_eq!(bodies.len(), 1, "one fused body, got {bodies:?}");
        let found = run(&mut r, "reverse.classify", json!({"body": bodies[0]}));
        assert_eq!(found["supported"], true, "classify: {found}");
        assert_eq!(found["category"], "turned");
        assert_eq!(found["segments"].as_array().map(Vec::len), Some(2), "segments: {found}");
        let done = run(&mut r, "reverse.rebuild_turned", json!({"source": bodies[0], "name": "Shaft2"}));
        assert_eq!(done["segments_built"], 2, "built: {done}");
        // The shared joint re-splits at its measured midpoint: tessellation-scale
        // shift around the exact union volume.
        let back = r.execute("inspect.measure", &json!({})).unwrap();
        let vols: Vec<f64> = back["bodies"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|b| b.get("name").and_then(Value::as_str).unwrap_or("").starts_with("Shaft2"))
                    .filter_map(|b| b.get("volume_mm3").and_then(Value::as_f64))
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(vols.len(), 1, "one rebuilt body, got: {back}");
        let want = std::f64::consts::PI * (100.0 * 10.0 + 36.0 * 10.0);
        assert!((vols[0] - want).abs() < 15.0, "volume {}, want ~{want}: {back}", vols[0]);
    }

    #[test]
    fn non_shaft_refuses() {
        let mut s = Session::default();
        run(&mut s, "solid.sphere", json!({"radius": 10, "body_name": "Ball"}));
        let found = run(&mut s, "reverse.classify", json!({"body": "Ball"}));
        assert_eq!(found["supported"], false, "classify: {found}");
        let err = s.execute("reverse.rebuild_plate", &json!({"source": "Ball"}));
        assert!(err.is_err(), "plate rebuild must refuse");
        let err = s.execute("reverse.rebuild_turned", &json!({"source": "Ball"}));
        assert!(err.is_err(), "turned rebuild must refuse");
    }

    #[test]
    fn hostile_params_never_panic() {
        let mut s = Session::default();
        assert!(s.execute("reverse.classify", &json!({"tolerance_mm": -5.0})).is_err());
        assert!(s.execute("reverse.classify", &json!({"body": "Nope"})).is_err());
        assert!(s.execute("reverse.classify", &json!({})).is_err());
        assert!(s.execute("reverse.rebuild_plate", &json!({"source": "Nope"})).is_err());
        assert!(s.execute("reverse.rebuild_bent_sheet", &json!({"source": "Nope"})).is_err());
    }

    fn bent_session() -> Session {
        // Base 100x60 on the default rule + one 20 mm up-flange on the y=0 edge.
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sheet.flange", json!({"sketch": "Base"}));
        run(&mut s, "sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20}));
        s
    }

    #[test]
    fn bent_bracket_round_trip() {
        let mut s = bent_session();
        let vol0 = s.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap_or(0.0);
        let dir = std::env::temp_dir().join("reverse_bent_trip");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bracket.step");
        run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "format": "step"}));
        let mut r = Session::default();
        let _ = run(&mut r, "file.insert_step", json!({"path": path.to_string_lossy()}));
        let bodies: Vec<String> = r.execute("inspect.measure", &json!({})).unwrap()["bodies"]
            .as_array()
            .map(|a| a.iter().filter_map(|b| b.get("name")?.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        assert_eq!(bodies.len(), 1, "one imported body, got {bodies:?}");
        let found = run(&mut r, "reverse.classify", json!({"body": bodies[0]}));
        assert_eq!(found["supported"], true, "classify: {found}");
        assert_eq!(found["category"], "bent_sheet");
        assert!((found["thickness"].as_f64().unwrap_or(0.0) - 2.5).abs() < 0.05, "thickness: {found}");
        assert!((found["bend_angle_deg"].as_f64().unwrap_or(0.0) - 90.0).abs() < 1.0, "angle: {found}");
        assert_eq!(found["holes"].as_array().map(Vec::len), Some(0), "holes: {found}");
        let dry = run(&mut r, "reverse.rebuild_bent_sheet", json!({"source": bodies[0], "dry_run": true}));
        assert_eq!(dry["recipe"].as_array().map(Vec::len), Some(6), "recipe: {dry}");
        let done = run(&mut r, "reverse.rebuild_bent_sheet", json!({"source": bodies[0], "name": "Bent2"}));
        assert_eq!(done["bends_built"], 1, "built: {done}");
        let back = r.execute("inspect.measure", &json!({"bodies": ["Bent2"]})).unwrap();
        let vol = back["bodies"][0]["volume_mm3"].as_f64().unwrap_or(0.0);
        assert!((vol - vol0).abs() / vol0 < 0.02, "volume {vol} vs original {vol0}");
        let flat = r.execute("sheet.flat_pattern", &json!({})).unwrap();
        assert_eq!(flat["bends"].as_array().map(Vec::len), Some(1), "flat pattern: {flat}");
    }

    #[test]
    fn bent_bracket_with_holes() {
        let mut s = bent_session();
        let bname0: String = s.execute("inspect.measure", &json!({})).unwrap()["bodies"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|b| b.get("name")?.as_str())
            .map(str::to_string)
            .unwrap_or_default();
        assert!(!bname0.is_empty(), "built bracket has a body");
        run(&mut s, "solid.hole", json!({"position": [20, 20, 2.5], "diameter": 6, "targets": [bname0]}));
        // Find leg 2's outer face at runtime: planar, normal horizontal.
        let bnames: Vec<String> = s.execute("inspect.measure", &json!({})).unwrap()["bodies"]
            .as_array()
            .map(|a| a.iter().filter_map(|b| b.get("name")?.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        assert_eq!(bnames.len(), 1, "one built body, got {bnames:?}");
        let faces = s.execute("model.faces", &json!({"body": bnames[0]})).unwrap();
        let mut leg2_face: Option<Value> = None;
        for f in faces["faces"].as_array().cloned().unwrap_or_default() {
            let n = f.get("plane_normal").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            if n.len() == 3 {
                let nx = n[0].as_f64().unwrap_or(0.0);
                let ny = n[1].as_f64().unwrap_or(0.0);
                let nz = n[2].as_f64().unwrap_or(0.0);
                if nx.abs() < 0.01 && nz.abs() < 0.01 && ny.abs() > 0.999 && leg2_face.is_none() {
                    leg2_face = Some(f.clone());
                }
            }
        }
        let lf = leg2_face.expect("leg-2 outer face");
        let pt = lf.get("point").cloned().unwrap_or(json!([50, 0, 10]));
        let p = pt.as_array().cloned().unwrap_or_default();
        let px = p.first().and_then(Value::as_f64).unwrap_or(50.0);
        let py = p.get(1).and_then(Value::as_f64).unwrap_or(0.0);
        let pz = p.get(2).and_then(Value::as_f64).unwrap_or(10.0);
        run(&mut s, "solid.hole", json!({"position": [px, py, pz], "diameter": 8, "targets": [bname0]}));
        let vol0 = s.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap_or(0.0);
        let dir = std::env::temp_dir().join("reverse_bent_holes_trip");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bracket_holes.step");
        run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "format": "step"}));
        let mut r = Session::default();
        let _ = run(&mut r, "file.insert_step", json!({"path": path.to_string_lossy()}));
        let bodies: Vec<String> = r.execute("inspect.measure", &json!({})).unwrap()["bodies"]
            .as_array()
            .map(|a| a.iter().filter_map(|b| b.get("name")?.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        assert_eq!(bodies.len(), 1, "one imported body, got {bodies:?}");
        let found = run(&mut r, "reverse.classify", json!({"body": bodies[0]}));
        assert_eq!(found["supported"], true, "classify: {found}");
        assert_eq!(found["category"], "bent_sheet");
        let holes = found["holes"].as_array().cloned().unwrap_or_default();
        assert_eq!(holes.len(), 2, "holes: {found}");
        let mut legs: Vec<u64> = holes.iter().filter_map(|h| h.get("leg").and_then(Value::as_u64)).collect();
        legs.sort_unstable();
        assert_eq!(legs, vec![0, 1], "one hole per leg: {found}");
        let done = run(&mut r, "reverse.rebuild_bent_sheet", json!({"source": bodies[0], "name": "BentH2"}));
        assert_eq!(done["holes_built"], 2, "built: {done}");
        let back = r.execute("inspect.measure", &json!({"bodies": ["BentH2"]})).unwrap();
        let vol = back["bodies"][0]["volume_mm3"].as_f64().unwrap_or(0.0);
        assert!((vol - vol0).abs() / vol0 < 0.02, "volume {vol} vs original {vol0}");
    }

    #[test]
    fn plate_still_plate_and_tray_refuses() {
        // Flat plate with holes is untouched by the bent branch.
        let (mut s, _hint) = round_trip();
        let bodies: Vec<String> = s.execute("inspect.measure", &json!({})).unwrap()["bodies"]
            .as_array()
            .map(|a| a.iter().filter_map(|b| b.get("name")?.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let found = run(&mut s, "reverse.classify", json!({"body": bodies[0]}));
        assert_eq!(found["category"], "flat_plate", "plate still plate: {found}");
        // Two opposite edge flanges = tray: refuse, do not guess.
        let mut t = Session::default();
        run(&mut t, "sketch.create", json!({"plane": "XY", "name": "Base"}));
        run(&mut t, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
        run(&mut t, "sketch.finish", json!({}));
        run(&mut t, "sheet.flange", json!({"sketch": "Base"}));
        run(&mut t, "sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20}));
        run(&mut t, "sheet.flange", json!({"edges": [[50, 60, 2.5]], "height": 20}));
        let dir = std::env::temp_dir().join("reverse_tray_trip");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tray.step");
        run(&mut t, "file.export", json!({"path": path.to_string_lossy(), "format": "step"}));
        let mut r = Session::default();
        let _ = run(&mut r, "file.insert_step", json!({"path": path.to_string_lossy()}));
        let bodies: Vec<String> = r.execute("inspect.measure", &json!({})).unwrap()["bodies"]
            .as_array()
            .map(|a| a.iter().filter_map(|b| b.get("name")?.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        assert_eq!(bodies.len(), 1, "one imported tray, got {bodies:?}");
        let found = run(&mut r, "reverse.classify", json!({"body": bodies[0]}));
        assert_eq!(found["supported"], false, "tray refuses: {found}");
        let err = r.execute("reverse.rebuild_bent_sheet", &json!({"source": bodies[0]}));
        assert!(err.is_err(), "bent rebuild must refuse the tray");
    }
}
