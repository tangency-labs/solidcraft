//! Joint origins on named faces: a face found by its persistent name gives its own plane or
//! cylinder (axis, radius, extent) directly, instead of searching again for the face nearest a
//! point (on a thin plate the face nearest a point on a hole's wall is often the plate's face).
//! A round origin's end is kept by the name of the face across the rim at that end, so it is the
//! same end after the design changes; an x reference keeps the origin's turn about its axis.

use std::collections::BTreeMap;

use solidcraft_geom::Vec3;
use solidcraft_kernel::CylinderFace;

use super::{Snap, XRef, component_bodies};
use crate::history_names::Match;
use crate::{DocError, Document, IDENTITY, Mat, ModelBody, ModelState, Result, apply_point, apply_vector, mat_inverse};

/// Rim points sampled to find the face across a cylinder's rim.
const RIM_SAMPLES: usize = 16;

/// A face found by its persistent name: its body, its index there, the point of it nearest the
/// pick (the component's coordinates) and the body's offset (a body moved in from another
/// component keeps its geometry where it was made; the offset places it).
pub(super) struct NamedFace<'a> {
    pub body: &'a ModelBody,
    pub face: usize,
    pub point: Vec3,
    pub offset: Mat,
}

fn gone(name: &str) -> DocError {
    DocError::Invalid(format!("joint origin: the face `{name}` no longer exists (edit the joint to pick again)"))
}

/// The face called `name` nearest `pick` (an error when it is gone: a joint must not quietly
/// attach elsewhere).
pub(super) fn named_face<'a>(doc: &Document, st: &'a ModelState, pick: Vec3, name: &str) -> Result<NamedFace<'a>> {
    let hit = crate::name_lookup::locate_face(st, name, pick).ok_or_else(|| gone(name))?;
    let body = st.bodies.get(hit.body).ok_or_else(|| gone(name))?;
    let Some(offset) = doc.model_body_offset(body).copied() else {
        return Ok(NamedFace { body, face: hit.face, point: hit.point, offset: IDENTITY });
    };
    let inv = mat_inverse(&offset).ok_or_else(|| gone(name))?;
    let (face, at, _) = crate::naming::face_on_body(body, name, apply_point(&inv, pick)).ok_or_else(|| gone(name))?;
    Ok(NamedFace { body, face, point: apply_point(&offset, at), offset })
}

impl NamedFace<'_> {
    /// A point of the component's coordinates in the body's own.
    fn local(&self, p: Vec3) -> Vec3 {
        if self.offset == IDENTITY {
            return p;
        }
        mat_inverse(&self.offset).map_or(p, |inv| apply_point(&inv, p))
    }

    /// The face as a cylinder, in the body's own coordinates (`None`: not a cylinder).
    fn own_cylinder(&self) -> Option<CylinderFace> {
        solidcraft_kernel::cylinder_face(&self.body.body, &self.body.mesh(), self.face)
    }

    /// The face as a cylinder, in the component's coordinates.
    pub fn cylinder(&self) -> Option<CylinderFace> {
        self.own_cylinder().map(|c| placed(&self.offset, c))
    }

    /// The face's centre and outward normal (the component's coordinates), when it is planar.
    pub fn plane(&self) -> Result<Option<(Vec3, Vec3)>> {
        let faces = self.body.joint_faces()?;
        Ok(faces.get(self.face).and_then(|f| f.plane_normal.map(|n| (apply_point(&self.offset, f.centroid), apply_vector(&self.offset, n)))))
    }
}

/// A cylinder of a body's own coordinates placed by the body's (rigid) offset.
pub(super) fn placed(m: &Mat, c: CylinderFace) -> CylinderFace {
    CylinderFace { axis_point: apply_point(m, c.axis_point), axis: apply_vector(m, *c.axis).unit().unwrap_or(c.axis), ..c }
}

/// The face across the rim at the `at` end (along the axis) of cylinder `c`, face `face` of `b`
/// (the body's own coordinates): of the body's other faces, the one nearest the rim's points.
fn rim_neighbour(b: &ModelBody, face: usize, c: &CylinderFace, at: f64) -> Option<usize> {
    let mesh = b.mesh();
    let slack = (c.end - c.start).abs() * 1e-3 + 1e-9;
    let mut rim: Vec<Vec3> = Vec::new();
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        if *f as usize != face {
            continue;
        }
        for k in t {
            if let Some(q) = mesh.positions.get(*k as usize)
                && ((*q - c.axis_point).dot(c.axis) - at).abs() <= slack
                && !rim.iter().any(|r| r.dist(*q) <= slack)
            {
                rim.push(*q);
            }
        }
    }
    // Evenly through the rim's points, at most RIM_SAMPLES of them.
    let step = rim.len().div_ceil(RIM_SAMPLES).max(1);
    let rim: Vec<Vec3> = rim.into_iter().step_by(step).collect();
    if rim.is_empty() {
        return None;
    }
    let mut near: BTreeMap<usize, Vec<f64>> = BTreeMap::new();
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        let f = *f as usize;
        if f == face {
            continue;
        }
        let Some([p, q, r]) = mesh.tri(t) else { continue };
        let slot = near.entry(f).or_insert_with(|| vec![f64::INFINITY; rim.len()]);
        for (d, x) in slot.iter_mut().zip(&rim) {
            *d = d.min(crate::naming::closest(*x, p, q, r).dist(*x));
        }
    }
    // The face that meets the whole rim (the other half of a cylinder split in two meets only
    // the rim's points at its seams).
    near.into_iter().map(|(f, d)| (d.iter().sum::<f64>(), f)).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, f)| f)
}

/// The ends of a cylinder: (place along the axis, which way out of the face).
fn ends(c: &CylinderFace) -> [(f64, f64); 2] {
    [(c.start, -1.0), (c.end, 1.0)]
}

/// The end of `c` nearer `pick` along the axis. On a tie, the end older builds took: the start
/// of the axis as the kernel turns it (toward +x, else +y, else +z), whichever way the face's
/// own surface runs.
fn nearer_end(c: &CylinderFace, pick: Vec3) -> (f64, f64) {
    let s = (pick - c.axis_point).dot(c.axis);
    let [start, end] = ends(c);
    let (to_start, to_end) = ((s - c.start).abs(), (c.end - s).abs());
    let reversed = c.axis.x + c.axis.y * 1e-3 + c.axis.z * 1e-6 < 0.0;
    if to_start < to_end || (to_start == to_end && !reversed) { start } else { end }
}

/// The end a named round face takes without an `end` (a design saved before origins named
/// it): the one nearer the pick (moved onto the face) along the axis, decided as those builds
/// decided it, by the kernel's reading of the cylinder at the pick, so a pick midway along the
/// wall takes the same end as it did then. `own` is the face's cylinder (the body's own
/// coordinates).
fn end_by_pick(nf: &NamedFace, own: &CylinderFace) -> (f64, f64) {
    let pick = nf.local(nf.point);
    let [start, end] = ends(own);
    match solidcraft_kernel::cylinder_face_at(&nf.body.body, pick).filter(|k| k.axis.dot(own.axis).abs() > 0.5) {
        Some(k) => {
            let s = (pick - k.axis_point).dot(k.axis);
            let out = if (s - k.start).abs() <= (k.end - s).abs() { -k.axis } else { k.axis };
            if out.dot(own.axis) < 0.0 { start } else { end }
        }
        None => nearer_end(own, pick),
    }
}

/// A round joint origin worked out: its cylinder, and the centre and outward axis of the end
/// it takes (the component's coordinates).
pub(super) struct Round {
    pub cylinder: CylinderFace,
    pub centre: Vec3,
    pub z: Vec3,
}

impl Round {
    fn at(c: CylinderFace, (at, dir): (f64, f64)) -> Round {
        Round { cylinder: c, centre: c.axis_point + c.axis * at, z: c.axis * dir }
    }
}

/// A circle-centre snap's cylinder found through its named face (`None`: the snap names no
/// face, or the face is not a cylinder any more; the caller looks by point, as older builds
/// did). The end is the one whose rim meets the face called `end`, else the one nearer `pick`.
pub(super) fn named_round(doc: &Document, st: &ModelState, pick: Vec3, face: &str, end: Option<&str>) -> Result<Option<Round>> {
    let nf = named_face(doc, st, pick, face)?;
    let Some(own) = nf.own_cylinder() else { return Ok(None) };
    let chosen = end.and_then(|name| end_named(nf.body, nf.face, &own, name)).unwrap_or_else(|| end_by_pick(&nf, &own));
    Ok(Some(Round::at(placed(&nf.offset, own), chosen)))
}

/// The end of cylinder `own` (face `face` of `b`) whose rim meets the face called `name` better
/// than the other end's rim does: the very face, or one merged into it, else a piece of it whose
/// neighbours have changed (the face round a hole in a sheet is told apart from its other pieces
/// by the faces around it, and those change when the hole moves). `None` when neither rim, or
/// both, meet it as well.
fn end_named(b: &ModelBody, face: usize, own: &CylinderFace, name: &str) -> Option<(f64, f64)> {
    let names = crate::naming::names(b);
    let how = |at: f64| {
        let f = rim_neighbour(b, face, own, at)?;
        if crate::name_lookup::legacy_name(name).is_some() {
            return crate::name_lookup::face_has_name(b, f, name).then_some(Match::Exact);
        }
        crate::history_names::matches(&names, f, name)
    };
    let [start, end] = ends(own);
    match (how(start.0), how(end.0)) {
        (s, e) if s > e => Some(start),
        (s, e) if e > s => Some(end),
        _ => None,
    }
}

fn alignment_direction(doc: &Document, st: &ModelState, reference: &super::AlignmentRef, component: u64) -> Result<Vec3> {
    use super::AlignmentRef;
    let bad = |message: &str| DocError::Invalid(message.into());
    let owns = |feature| -> Result<()> {
        let f = doc.feature(feature).ok_or_else(|| bad("alignment feature no longer exists"))?;
        if f.component != component {
            return Err(bad("alignment reference is not on the origin's component"));
        }
        Ok(())
    };
    match reference {
        AlignmentRef::Edge { name } => {
            let mut found = None;
            for body in component_bodies(doc, st, component) {
                let Ok(hit) = crate::name_lookup::locate_edge(body, name, Vec3::ZERO) else { continue };
                if hit.split || !hit.verified {
                    return Err(bad("alignment edge identity is ambiguous; select the hole's circular rim, cylindrical face or a construction axis"));
                }
                let mesh = body.mesh();
                let points = mesh.edges.get(hit.edge).ok_or_else(|| bad("alignment edge no longer exists"))?;
                let direction = crate::construct::as_line(points)
                    .map(|(_, d)| d)
                    .or_else(|| crate::construct::as_circle(points).map(|(_, n, _)| n))
                    .ok_or_else(|| bad("alignment edge must remain straight or circular"))?;
                let direction = doc.model_body_offset(body).map_or(direction, |frame| apply_vector(frame, direction));
                if found.replace(direction).is_some() {
                    return Err(bad("alignment edge identity is ambiguous"));
                }
            }
            found.ok_or_else(|| bad("alignment edge no longer exists on the origin's component"))
        }
        AlignmentRef::Cylinder { name } => {
            let (owner, pick) = named_face_spot(doc, st, None, name).map_err(DocError::Invalid)?;
            if owner != component {
                return Err(bad("alignment face is not on the origin's component"));
            }
            let nf = named_face(doc, st, pick, name)?;
            nf.cylinder().map(|c| *c.axis).ok_or_else(|| bad("alignment reference must remain cylindrical"))
        }
        AlignmentRef::Construction { feature } => {
            owns(*feature)?;
            let construction = st.construct.iter().find(|c| c.feature == *feature).ok_or_else(|| bad("alignment construction no longer exists"))?;
            match &construction.geom {
                crate::construct::ConstructGeom::Axis { dir, .. } => Ok(*dir),
                crate::construct::ConstructGeom::Plane(plane) => Ok(plane.normal()),
                _ => Err(bad("alignment construction must be an axis or plane")),
            }
        }
        AlignmentRef::SketchLine { sketch, curve } => {
            owns(*sketch)?;
            let ss = st.sketch(*sketch).ok_or_else(|| bad("alignment sketch no longer exists"))?;
            let index = ss.sketch.curve_index(curve).ok_or_else(|| bad("alignment line no longer exists"))?;
            if !matches!(ss.sketch.curves[index].kind, solidcraft_sketch::CurveKind::Line { .. }) {
                return Err(bad("alignment sketch curve must remain a line"));
            }
            let points: Vec<_> = ss.sketch.polyline(index).iter().map(|p| ss.plane.to_world(*p)).collect();
            crate::construct::as_line(&points).map(|(_, d)| d).ok_or_else(|| bad("alignment line is degenerate"))
        }
        AlignmentRef::OriginAxis { axis } => match axis.as_str() {
            "X" => Ok(Vec3::X),
            "Y" => Ok(Vec3::Y),
            "Z" => Ok(Vec3::Z),
            _ => Err(bad("unknown alignment axis")),
        },
        AlignmentRef::OriginPlane { plane } => match plane.as_str() {
            "XY" => Ok(Vec3::Z),
            "XZ" => Ok(-Vec3::Y),
            "YZ" => Ok(Vec3::X),
            _ => Err(bad("unknown alignment plane")),
        },
    }
}

/// The x axis an x reference gives a frame at `o` with axis `z` (`None`: no reference).
pub(super) fn x_axis(doc: &Document, st: &ModelState, x: Option<&XRef>, o: Vec3, z: Vec3, occurrence: u64) -> Result<Option<Vec3>> {
    Ok(match x {
        None => None,
        Some(XRef::Dir(d)) => Some(*d),
        Some(XRef::Reference { reference, reverse }) => {
            let direction = alignment_direction(doc, st, reference, super::component_of(doc, occurrence)?)?;
            let z = z.normalized().ok_or_else(|| DocError::Invalid("alignment origin has no axis".into()))?;
            let x = (direction - z * direction.dot(z))
                .normalized()
                .ok_or_else(|| DocError::Invalid("alignment direction is parallel to the joint axis".into()))?;
            Some(if *reverse { -x } else { x })
        }
        Some(XRef::Planar { planar_face, reverse }) => {
            // locate_one refuses ambiguous split names; never choose a nearest replacement.
            let (on, pick) = named_face_spot(doc, st, None, planar_face).map_err(DocError::Invalid)?;
            if on != super::component_of(doc, occurrence)? {
                return Err(DocError::Invalid("alignment face is not on the origin's component".into()));
            }
            let nf = named_face(doc, st, pick, planar_face)?;
            let (_, n) = nf.plane()?.ok_or_else(|| DocError::Invalid("alignment reference must remain planar".into()))?;
            let z = z.normalized().ok_or_else(|| DocError::Invalid("alignment origin has no axis".into()))?;
            let x = (n - z * n.dot(z))
                .normalized()
                .ok_or_else(|| DocError::Invalid("alignment normal is parallel to the joint axis; pick another face".into()))?;
            Some(if *reverse { -x } else { x })
        }
        Some(XRef::Face { face, pick }) => {
            let nf = named_face(doc, st, pick.unwrap_or(o), face)?;
            if let Some((_, n)) = nf.plane()? {
                Some(n)
            } else if let Some(c) = nf.cylinder() {
                let v = c.axis_point - o;
                Some(v - c.axis * v.dot(c.axis))
            } else {
                Some(nf.point - o)
            }
        }
    })
}

/// The persistent name of the face of a component's bodies at `pick` (the component's
/// coordinates), measured to each face's triangles and, among the faces as near as the mesh
/// lets them be told apart, to the face's own surface (a point on a thin plate's hole wall lies
/// as near the facets of the plate's face round the rim). `round` prefers a cylinder on a tie
/// (a pick on a hole's rim names its wall). `None`: no named face there.
pub fn face_name_at(doc: &Document, st: &ModelState, component: u64, pick: Vec3, round: bool) -> Option<String> {
    let mut best: Option<(f64, &ModelBody, usize)> = None;
    for b in component_bodies(doc, st, component) {
        if b.body.is_mesh() {
            continue;
        }
        let offset = doc.model_body_offset(b).copied().unwrap_or(IDENTITY);
        let p = apply_point(&mat_inverse(&offset)?, pick);
        let mesh = b.mesh();
        let tol = (b.body.size() * 1e-3).max(1e-3).max(b.display_tol());
        let mut near: BTreeMap<usize, f64> = BTreeMap::new();
        for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
            let Some([x, y, z]) = mesh.tri(t) else { continue };
            let d = crate::naming::closest(p, x, y, z).dist(p);
            near.entry(*f as usize).and_modify(|n| *n = n.min(d)).or_insert(d);
        }
        let nearest = near.values().copied().fold(f64::INFINITY, f64::min);
        if nearest > tol * 5.0 {
            continue;
        }
        for (f, d) in near.into_iter().filter(|(_, d)| *d <= nearest + tol) {
            let (off, cylinder) = match solidcraft_kernel::face_surface(&b.body, f) {
                Some(solidcraft_kernel::FaceSurface::Plane { normal, point }) => (d.min((p - point).dot(normal).abs()), false),
                Some(solidcraft_kernel::FaceSurface::Cylinder { .. }) => {
                    (solidcraft_kernel::cylinder_face(&b.body, &mesh, f).map_or(d, |c| d.min(c.off(p))), true)
                }
                _ => (d, false),
            };
            let score = if round && cylinder { off - tol } else { off };
            if best.as_ref().is_none_or(|(s, ..)| score < *s) {
                best = Some((score, b, f));
            }
        }
    }
    let (_, b, f) = best?;
    crate::naming::face_names(b).get(f).filter(|n| !n.is_empty()).cloned()
}

/// A circle-centre snap for the round end of a component's cylindrical face centred at
/// `centre` (the component's coordinates; a circle given by its centre, where no face lies),
/// with the face's name and its end's: `None` when no named cylinder ends there.
pub fn round_end_at(doc: &Document, st: &ModelState, component: u64, centre: Vec3) -> Option<Snap> {
    let mut best: Option<(f64, &ModelBody, usize, CylinderFace, f64)> = None;
    for b in component_bodies(doc, st, component) {
        if b.body.is_mesh() {
            continue;
        }
        let offset = doc.model_body_offset(b).copied().unwrap_or(IDENTITY);
        let p = apply_point(&mat_inverse(&offset)?, centre);
        let mesh = b.mesh();
        let tol = (b.body.size() * 1e-3).max(1e-3);
        for f in 0..b.body.face_count() {
            let Some(c) = solidcraft_kernel::cylinder_face(&b.body, &mesh, f) else { continue };
            let v = p - c.axis_point;
            let along = v.dot(c.axis);
            for (at, _) in ends(&c) {
                let off = (v - c.axis * along).len() + (along - at).abs();
                if off <= tol && best.as_ref().is_none_or(|(o, ..)| off < *o) {
                    best = Some((off, b, f, c, at));
                }
            }
        }
    }
    let (_, b, f, c, at) = best?;
    let names = crate::naming::face_names(b);
    let face = names.get(f).filter(|n| !n.is_empty()).cloned()?;
    let end = rim_neighbour(b, f, &c, at).and_then(|n| names.get(n)).filter(|n| !n.is_empty()).cloned();
    Some(Snap::CircleCenter { pick: centre, face: Some(face), end, x: None })
}

/// Name the end a named circle-centre snap takes now (the face across the rim there), so it
/// keeps that end when the design changes. Other snaps, and one whose end is named already, are
/// left as they are.
pub fn pin_end(doc: &Document, st: &ModelState, snap: &mut Snap) {
    let Snap::CircleCenter { pick, face: Some(face), end: end @ None, .. } = snap else { return };
    let Ok(nf) = named_face(doc, st, *pick, face) else { return };
    let Some(own) = nf.own_cylinder() else { return };
    let (at, _) = end_by_pick(&nf, &own);
    *end = rim_neighbour(nf.body, nf.face, &own, at).and_then(|n| crate::naming::face_names(nf.body).get(n).cloned()).filter(|n| !n.is_empty());
}

/// The names of the faces across the rims at the two ends of a named circle-centre snap's
/// cylinder (what its `end` can be); empty when the snap names no cylinder.
pub fn end_faces(doc: &Document, st: &ModelState, snap: &Snap) -> Vec<String> {
    let Snap::CircleCenter { pick, face: Some(face), .. } = snap else { return Vec::new() };
    let Ok(nf) = named_face(doc, st, *pick, face) else { return Vec::new() };
    let Some(own) = nf.own_cylinder() else { return Vec::new() };
    let names = crate::naming::face_names(nf.body);
    ends(&own)
        .iter()
        .filter_map(|(at, _)| rim_neighbour(nf.body, nf.face, &own, *at).and_then(|n| names.get(n).cloned()))
        .filter(|n| !n.is_empty())
        .collect()
}

/// The face `model.faces` calls `name` (on the body called `body`, or any body): the component
/// it belongs to and a point on it in that component's coordinates, for a joint origin given by
/// a face's name. `Err` says why there is no one such face.
pub fn named_face_spot(doc: &Document, st: &ModelState, body: Option<&str>, name: &str) -> std::result::Result<(u64, Vec3), String> {
    let hit = crate::name_lookup::locate_one(st, body, name)?;
    let b = st.bodies.get(hit.body).ok_or_else(|| format!("no face is named `{name}`"))?;
    let spot = crate::naming::face_spot(b, hit.face).ok_or_else(|| format!("face `{name}` has no surface to pick"))?;
    let offset = doc.model_body_offset(b).copied().unwrap_or(IDENTITY);
    Ok((doc.model_body_component(b), apply_point(&offset, spot.point)))
}
