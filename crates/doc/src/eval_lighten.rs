//! Pockets cut from sketch profiles, following Onshape's Lighten (FeatureScript 1260).
//!
//! Each profile is inset by half the wall thickness, so two profiles that share an edge leave
//! that thickness between them. With trim on, the part's own outline on the same plane is inset
//! by the full thickness and the pockets stay inside it. That inset is the net of the script's
//! two offsets: `shellAndEncloseWithConcaveFillet` moves every face in by `wall/2 + toolRadius`,
//! then `expandWithFillet` moves every face except the caps back out by `toolRadius`.
//!
//! The fillets are the script's, at the radius `opModifyFillet` would leave. There is no
//! enclose or change-radius operation, so they are cut on this already-offset solid:
//! concave edges at `wall/2` (the shell), and convex edges that do not bound a cap at the
//! tool radius (the expand). Cap rims stay sharp, as the script's `noEditFaces` requires.

use solidcraft_geom::{Loop2, Mesh, Region2, Seg2, Vec3};
use solidcraft_sketch::inset_region;

use super::*;

const OVERLAP: f64 = 0.05;
const PLANE_GAP: f64 = 1e-2;
const JOIN_TOL: f64 = 1e-4;

pub(super) fn eval(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState, warning: &mut Option<String>) -> Result<()> {
    let FeatureKind::Lighten { sketch, profiles, thickness, tool_radius, depth, through_all, to, trim, targets } = &f.kind else {
        return Err(DocError::Invalid("not a lighten feature".into()));
    };
    let ss = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?;
    let ss = sketch_in_feature_frame(doc, ss, f.component)?;
    let profiles = select_profiles(&ss, profiles)?;
    let thickness = val(vals, thickness, Kind::Length)?;
    if !(thickness > 0.0) {
        return Err(DocError::Invalid("the wall thickness must be positive".into()));
    }
    let tool_radius = val(vals, tool_radius, Kind::Length)?;
    if !(tool_radius >= 0.0) {
        return Err(DocError::Invalid("the tool radius must be zero or positive".into()));
    }
    let target_names = crate::plain(targets);
    let (_, names) = boolean_scope(st, f, &target_names);
    if names.is_empty() {
        return Err(DocError::Invalid("there is no body to cut".into()));
    }
    let (outlines, face_sign) = coincident_faces(st, &names, &ss.plane, *trim)?;
    if *trim {
        if outlines.is_empty() {
            return Err(DocError::Invalid("the profiles are not on a face of the parts".into()));
        }
        if !regions_overlap(&profiles, &outlines)? {
            return Err(DocError::Invalid("the profiles are not on a face of the parts".into()));
        }
    }
    let sign = match face_sign {
        Some(sign) => sign,
        None => probe_sign(&ss.plane, &profiles, st, &names)
            .ok_or_else(|| DocError::Invalid("cannot tell which way to cut: the profiles are not on a face of the parts".into()))?,
    };
    let mut pockets = Vec::with_capacity(profiles.len());
    let mut skipped = 0;
    for profile in &profiles {
        // A sliver at a node circle can be narrower than the wall. Leave it out. The bays
        // around it still cut, which is the part Onshape lightens.
        match inset_region(profile, thickness * 0.5) {
            Ok(region) => pockets.push(region),
            Err(_) => skipped += 1,
        }
    }
    if pockets.is_empty() {
        return Err(DocError::Invalid("the offset is larger than the profile allows".into()));
    }
    if *trim {
        let stock = inset_regions(&union_regions(outlines)?, thickness)?;
        if stock.is_empty() {
            return Err(DocError::Invalid("trimming to the part outline failed. Turn trim off.".into()));
        }
        let mut clipped = Vec::new();
        for pocket in &pockets {
            match kernel::planar_regions_boolean(std::slice::from_ref(pocket), &stock, BoolOp::Intersect, 1e-4) {
                Ok(hit) if hit.iter().any(|region| region.area() > 1e-6) => clipped.extend(hit),
                _ => skipped += 1,
            }
        }
        if clipped.is_empty() {
            return Err(DocError::Invalid("trimming to the part outline failed. Turn trim off.".into()));
        }
        pockets = clipped;
    }
    pockets = union_keeping(pockets, &mut skipped)?;
    if skipped > 0 {
        let note = if skipped == 1 {
            "1 profile was too small for the wall and was left out".to_string()
        } else {
            format!("{skipped} profiles were too small for the wall and were left out")
        };
        super::append_warning(warning, note);
    }
    let (start, end) = extent(vals, &ss.plane, st, &names, depth, *through_all, *to, sign)?;
    let mut tools = kernel::extrude(&ss.plane, &pockets, start, end)?;
    let axis = ss.plane.normal();
    for tool in &mut tools {
        *tool = shell_and_enclose_with_concave_fillet(tool, thickness * 0.5)?;
        *tool = expand_with_fillet(tool, tool_radius, axis)?;
    }
    apply_op(doc, st, f, tools, Operation::Cut, &names)
}

/// Onshape `shellAndEncloseWithConcaveFillet`: concave edges become fillets of `offset`.
fn shell_and_enclose_with_concave_fillet(body: &Body, offset: f64) -> Result<Body> {
    fillet_corners(body, offset, false, false, Vec3::Z)
}

/// Onshape `expandWithFillet`: convex edges that do not bound a cap become fillets of `offset`.
fn expand_with_fillet(body: &Body, offset: f64, axis: Vec3) -> Result<Body> {
    fillet_corners(body, offset, true, true, axis)
}

fn fillet_corners(body: &Body, radius: f64, convex: bool, skip_caps: bool, axis: Vec3) -> Result<Body> {
    if !(radius > 1e-6) {
        return Ok(body.clone());
    }
    let mut points = corner_edges(body, convex, skip_caps, axis)?;
    if points.is_empty() {
        return Ok(body.clone());
    }
    // A circle split into two arcs extrudes to a smooth seam. The mesh can call that seam a
    // corner. Onshape only fillets edges that are really concave or convex, so a seam stays.
    let current = body.clone();
    while !points.is_empty() {
        match kernel::fillet(&current, &points, radius) {
            Ok(next) => return Ok(next),
            Err(kernel::KernelError::Invalid(msg)) => {
                let Some(at) = smooth_edge_at(&msg) else { return Err(kernel::KernelError::Invalid(msg).into()) };
                let before = points.len();
                points.retain(|point| point.dist(at) > 0.2);
                if points.len() == before {
                    let nearest = points.iter().enumerate().min_by(|(_, a), (_, b)| a.dist(at).total_cmp(&b.dist(at))).map(|(index, _)| index);
                    if let Some(index) = nearest {
                        points.remove(index);
                    }
                }
            }
            Err(other) => return Err(other.into()),
        }
    }
    Ok(current)
}

/// The point named by the kernel when an edge is a smooth seam, not a corner.
fn smooth_edge_at(msg: &str) -> Option<Vec3> {
    let rest = msg.split("the edge at [").nth(1)?;
    if !rest.contains("is smooth") {
        return None;
    }
    let inside = rest.split(']').next()?;
    let nums: Vec<f64> = inside.split(',').filter_map(|piece| piece.trim().parse().ok()).collect();
    (nums.len() == 3).then(|| Vec3::new(nums[0], nums[1], nums[2]))
}

/// Midpoints of the edges whose convexity is `convex`. With `skip_caps`, an edge that bounds a
/// face normal to `axis` is left out (a cap rim).
fn corner_edges(body: &Body, convex: bool, skip_caps: bool, axis: Vec3) -> Result<Vec<Vec3>> {
    // Fine enough that a small node circle on a large plate is not faceted into a false corner.
    let tol = (body.size() * 1e-4).clamp(1e-3, 5e-2);
    let mesh = body.tessellate(tol)?;
    let mut tris: Vec<Vec<usize>> = vec![Vec::new(); mesh.tri_face.iter().copied().max().map(|f| f as usize + 1).unwrap_or(0)];
    for (index, face) in mesh.tri_face.iter().enumerate() {
        if let Some(bucket) = tris.get_mut(*face as usize) {
            bucket.push(index);
        }
    }
    let mut points = Vec::new();
    for (edge, faces) in mesh.edges.iter().zip(&mesh.edge_faces) {
        let [fa, fb] = faces.as_slice() else { continue };
        let (Some(a), Some(b)) = (side_along(&mesh, &tris, *fa as usize, edge, tol), side_along(&mesh, &tris, *fb as usize, edge, tol)) else {
            continue;
        };
        if skip_caps && (caps(a.normal, axis) || caps(b.normal, axis)) {
            continue;
        }
        if turning(&a, &b) == Some(convex) {
            if let Some(mid) = polyline_mid(edge) {
                points.push(mid);
            }
        }
    }
    Ok(points)
}

struct Side {
    normal: Vec3,
    into: Vec3,
}

fn caps(normal: Vec3, axis: Vec3) -> bool {
    normal.dot(axis).abs() > 1.0 - 1e-3
}

/// `Some(true)` when the edge is convex. Tangent joins and knife edges are `None`.
fn turning(a: &Side, b: &Side) -> Option<bool> {
    const CLEAR: f64 = 0.05;
    // A split circle's seam: the two faces share a tangent plane. Mesh facets sit a few degrees
    // off the true normal, so this is wider than the kernel's exact test.
    if a.normal.dot(b.normal) > 0.98 {
        return None;
    }
    if a.into.dot(b.into) > 1.0 - CLEAR || a.into.dot(b.into) < -1.0 + CLEAR {
        return None;
    }
    let toward_b = a.normal.dot(b.into);
    let toward_a = b.normal.dot(a.into);
    if toward_b < -CLEAR && toward_a < -CLEAR {
        Some(true)
    } else if toward_b > CLEAR && toward_a > CLEAR {
        Some(false)
    } else {
        None
    }
}

fn side_along(mesh: &Mesh, tris: &[Vec<usize>], face: usize, edge: &[Vec3], tol: f64) -> Option<Side> {
    for &index in tris.get(face)? {
        let tri = mesh.triangles.get(index)?;
        let pts = [
            mesh.positions.get(tri[0] as usize).copied()?,
            mesh.positions.get(tri[1] as usize).copied()?,
            mesh.positions.get(tri[2] as usize).copied()?,
        ];
        let normal = (pts[1] - pts[0]).cross(pts[2] - pts[0]).normalized()?;
        for i in 0..3 {
            let a = pts[i];
            let b = pts[(i + 1) % 3];
            if dist_to_polyline(a, edge) > tol || dist_to_polyline(b, edge) > tol {
                continue;
            }
            let dir = (b - a).normalized()?;
            return Some(Side { normal, into: normal.cross(dir).normalized()? });
        }
    }
    None
}

fn dist_to_polyline(p: Vec3, edge: &[Vec3]) -> f64 {
    edge.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::MAX, f64::min)
}

fn polyline_mid(pts: &[Vec3]) -> Option<Vec3> {
    if pts.len() < 2 {
        return None;
    }
    let total: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut left = total * 0.5;
    for w in pts.windows(2) {
        let span = w[0].dist(w[1]);
        if left <= span {
            let t = if span > 1e-12 { left / span } else { 0.0 };
            return Some(w[0] + (w[1] - w[0]) * t);
        }
        left -= span;
    }
    pts.last().copied()
}

fn extent(
    vals: &BTreeMap<String, Value>,
    plane: &Plane,
    st: &ModelState,
    names: &[String],
    depth: &crate::Expr,
    through_all: bool,
    to: Option<crate::FacePick>,
    sign: f64,
) -> Result<(f64, f64)> {
    if let Some(to) = to {
        let height = plane.height(to.0);
        if !(height.is_finite() && height * sign > OVERLAP) {
            return Err(DocError::Invalid("the end is not ahead of the profiles".into()));
        }
        return Ok((-sign * OVERLAP, height));
    }
    if through_all {
        let far = through_all_distance(st, plane, names)?;
        return Ok((-far, far));
    }
    let depth = val(vals, depth, Kind::Length)?;
    if !(depth > 1e-6) {
        return Err(DocError::Invalid("the depth must be positive".into()));
    }
    Ok((-sign * OVERLAP, sign * depth))
}

fn inset_regions(regions: &[Region2], distance: f64) -> Result<Vec<Region2>> {
    regions.iter().map(|region| Ok(inset_region(region, distance)?)).collect()
}

fn union_regions(mut regions: Vec<Region2>) -> Result<Vec<Region2>> {
    let mut skipped = 0;
    let out = union_keeping(regions.split_off(0), &mut skipped)?;
    if skipped > 0 {
        return Err(DocError::Invalid("the offset is larger than the profile allows".into()));
    }
    Ok(out)
}

/// Union what can be unioned. A sliver whose offset is not a line or a circular arc is left out.
fn union_keeping(regions: Vec<Region2>, skipped: &mut usize) -> Result<Vec<Region2>> {
    let mut acc: Vec<Region2> = Vec::new();
    for region in regions {
        if region.area() <= 1e-8 || !boolean_curves(&region) {
            *skipped += 1;
            continue;
        }
        if acc.is_empty() {
            acc.push(region);
            continue;
        }
        match kernel::planar_regions_boolean(&acc, std::slice::from_ref(&region), BoolOp::Union, 1e-4) {
            Ok(next) => acc = next,
            Err(_) => *skipped += 1,
        }
    }
    if acc.is_empty() {
        return Err(DocError::Invalid("the offset is larger than the profile allows".into()));
    }
    Ok(acc)
}

fn boolean_curves(region: &Region2) -> bool {
    let ok = |seg: &Seg2| match *seg {
        Seg2::Line { a, b } => a.dist(b) > 1e-9,
        Seg2::Arc { radius, sweep, .. } => radius > 1e-9 && sweep.abs() > 1e-9 && sweep.abs() <= std::f64::consts::TAU + 1e-6,
        _ => false,
    };
    region.outer.segs.iter().all(ok) && region.holes.iter().all(|hole| hole.segs.iter().all(ok))
}

fn regions_overlap(profiles: &[Region2], outlines: &[Region2]) -> Result<bool> {
    let hit = kernel::planar_regions_boolean(profiles, outlines, BoolOp::Intersect, 1e-4)?;
    Ok(hit.iter().any(|region| region.area() > 1e-6))
}

/// Face outlines that lie on `plane`, and the sign of the cut (positive along the plane normal).
/// Faces whose normals disagree are refused. With trim off, an outline that is not lines and
/// arcs still supplies its direction.
fn coincident_faces(st: &ModelState, names: &[String], plane: &Plane, trim: bool) -> Result<(Vec<Region2>, Option<f64>)> {
    let mut regions = Vec::new();
    let mut sign: Option<f64> = None;
    for body in st.bodies.iter().filter(|body| names.iter().any(|name| name == &body.name)) {
        let mesh = body.mesh();
        let count = body.body.face_count().min(10_000);
        for index in 0..count {
            let Some(face) = body.body.face_plane(index).or_else(|| body.body.planar_face(index)) else { continue };
            if !same_plane(plane, &face) {
                continue;
            }
            let here = if plane.normal().dot(face.normal()) >= 0.0 { -1.0 } else { 1.0 };
            match sign {
                None => sign = Some(here),
                Some(previous) if (previous - here).abs() > 1e-9 => {
                    return Err(DocError::Invalid("coplanar faces point different ways".into()));
                }
                Some(_) => {}
            }
            if !trim {
                continue;
            }
            let edges: Vec<usize> =
                mesh.face_edges(index as u32).into_iter().filter(|edge| !mesh.seams.get(*edge).copied().unwrap_or(false)).collect();
            if edges.is_empty() {
                return Err(DocError::Invalid("the part outline is not a closed loop of lines and arcs".into()));
            }
            let projected = body.body.planar_edges(plane, &edges);
            let mut segs = Vec::new();
            for part in &projected {
                let Some(part) = part else {
                    return Err(DocError::Invalid("the part outline is not lines and arcs".into()));
                };
                segs.extend(part.iter().copied());
            }
            regions.extend(regions_from_loops(loops_from_segments(&segs)?)?);
        }
    }
    Ok((regions, sign))
}

fn same_plane(sketch: &Plane, face: &Plane) -> bool {
    sketch.normal().dot(face.normal()).abs() > 1.0 - 1e-6 && sketch.height(face.origin).abs() < PLANE_GAP
}

fn probe_sign(plane: &Plane, profiles: &[Region2], st: &ModelState, names: &[String]) -> Option<f64> {
    let normal = plane.normal();
    let mut plus = 0;
    let mut minus = 0;
    for profile in profiles {
        let at = plane.to_world(profile.interior_point());
        for body in st.bodies.iter().filter(|body| names.iter().any(|name| name == &body.name)) {
            if body.body.contains(at + normal * 0.2) {
                plus += 1;
            }
            if body.body.contains(at - normal * 0.2) {
                minus += 1;
            }
        }
    }
    match (plus > 0, minus > 0) {
        (true, false) => Some(1.0),
        (false, true) => Some(-1.0),
        _ => None,
    }
}

fn loops_from_segments(segs: &[Seg2]) -> Result<Vec<Loop2>> {
    let mut atoms: Vec<Seg2> = segs
        .iter()
        .copied()
        .filter(|seg| match seg {
            Seg2::Line { .. } => seg.length() > JOIN_TOL,
            Seg2::Arc { sweep, radius, .. } => sweep.abs() > MIN_SWEEP && *radius > JOIN_TOL,
            Seg2::Cubic { .. } | Seg2::Conic { .. } | Seg2::Bezier { .. } => true,
        })
        .collect();
    if atoms.iter().any(|seg| !matches!(seg, Seg2::Line { .. } | Seg2::Arc { .. })) {
        return Err(DocError::Invalid("the part outline is not lines and arcs".into()));
    }
    if atoms.is_empty() || atoms.len() > 4_096 {
        return Err(DocError::Invalid("the part outline is not a closed loop of lines and arcs".into()));
    }
    let mut used = vec![false; atoms.len()];
    let mut loops = Vec::new();
    for start_at in 0..atoms.len() {
        if used[start_at] {
            continue;
        }
        if atoms[start_at].start().dist(atoms[start_at].end()) <= JOIN_TOL {
            used[start_at] = true;
            loops.push(Loop2 { segs: vec![atoms[start_at]] });
            continue;
        }
        let origin = atoms[start_at].start();
        let mut chain = vec![atoms[start_at]];
        used[start_at] = true;
        let mut cursor = atoms[start_at].end();
        let mut closed = false;
        for _ in 0..atoms.len() {
            if cursor.dist(origin) <= JOIN_TOL {
                closed = true;
                break;
            }
            let mut found: Option<usize> = None;
            for (index, seg) in atoms.iter().enumerate() {
                if used[index] {
                    continue;
                }
                if seg.start().dist(cursor) <= JOIN_TOL || seg.end().dist(cursor) <= JOIN_TOL {
                    if found.is_some() {
                        return Err(DocError::Invalid("the part outline has a branched boundary".into()));
                    }
                    found = Some(index);
                }
            }
            let Some(index) = found else { break };
            used[index] = true;
            let mut seg = atoms[index];
            if seg.start().dist(cursor) > JOIN_TOL {
                seg = seg.reversed();
            }
            cursor = seg.end();
            chain.push(seg);
            atoms[index] = seg;
        }
        if !closed {
            return Err(DocError::Invalid("the part outline is not a closed loop of lines and arcs".into()));
        }
        loops.push(Loop2 { segs: chain });
    }
    Ok(loops)
}

const MIN_SWEEP: f64 = 1e-6;

fn regions_from_loops(mut loops: Vec<Loop2>) -> Result<Vec<Region2>> {
    loops.retain(|loop2| loop2.signed_area().abs() > 1e-6);
    let mut order: Vec<usize> = (0..loops.len()).collect();
    order.sort_by(|&a, &b| loops[b].signed_area().abs().total_cmp(&loops[a].signed_area().abs()));
    let mut used = vec![false; loops.len()];
    let mut regions = Vec::new();
    for index in order {
        if used[index] {
            continue;
        }
        used[index] = true;
        let outer = loops[index].ccw();
        let mut holes = Vec::new();
        for (other, loop2) in loops.iter().enumerate() {
            if used[other] {
                continue;
            }
            if outer.contains(loop2.centroid()) {
                used[other] = true;
                holes.push(if loop2.is_ccw() { loop2.reversed() } else { loop2.clone() });
            }
        }
        regions.push(Region2 { outer, holes });
    }
    Ok(regions)
}
