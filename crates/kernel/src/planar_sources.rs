//! Exact oriented planar support lineage through a trim and an inside shell.
//! No tessellated face names, centroids, or nearest-plane choices are used.
use super::{Body, FaceSource};
use crate::{KernelError, Result, guard};
use solidcraft_geom::brep::{self, Surface};
use solidcraft_geom::{Plane, Vec3};
use std::sync::Arc;

/// The oriented plane each face of `body` lies on (in face order), from its record: the
/// surface's plane, its normal turned with the face; none for a face on any other surface.
pub(super) fn actual_planes(body: &Body) -> Result<Vec<Option<Plane>>> {
    let record = crate::native::brep_record(body)?;
    let mut forward = vec![true; record.faces.len()];
    for shell in &record.shells {
        if let brep::Shell::Faces { faces, .. } = shell {
            for o in faces {
                if let Some(f) = forward.get_mut(o.face as usize) {
                    *f = o.forward;
                }
            }
        }
    }
    let v = |p: [f64; 3]| Vec3::new(p[0], p[1], p[2]);
    Ok(record
        .faces
        .iter()
        .zip(forward)
        .map(|(face, forward)| {
            let (o, u, w) = match face.surface {
                Surface::Plane { origin, u_axis, v_axis } => (v(origin), v(u_axis), v(v_axis)),
                Surface::PlaneThrough { origin, u_point, v_point } => (v(origin), v(u_point) - v(origin), v(v_point) - v(origin)),
                _ => return None,
            };
            if !o.is_finite() || !u.is_finite() || !w.is_finite() || u.cross(w).len() <= u.len() * w.len() * 1e-8 {
                return None;
            }
            // A reversed face faces against its surface's normal.
            if forward { Plane::new(o, u, w) } else { Plane::new(o, w, u) }
        })
        .collect())
}

/// How far apart the normals' x components of two planes `same_support` accepts can be, with
/// room to spare: their unit normals are within about 1e-12 of each other.
const NORMAL_WINDOW: f64 = 1e-9;

/// The entries of `sorted` (ordered by their plane's normal x) whose normal x is within
/// [`NORMAL_WINDOW`] of `nx`: the only ones `same_support` can accept for a plane with that normal.
fn normal_window<T>(sorted: &[T], plane: impl Fn(&T) -> &Plane, nx: f64) -> &[T] {
    let first = sorted.partition_point(|t| plane(t).normal().x < nx - NORMAL_WINDOW);
    let last = sorted.partition_point(|t| plane(t).normal().x <= nx + NORMAL_WINDOW);
    sorted.get(first..last.max(first)).unwrap_or_default()
}

pub(super) fn same_support(a: &Plane, b: &Plane) -> bool {
    let error = 4096. * f64::EPSILON * (1. + a.origin.len() + b.origin.len());
    a.normal().cross(b.normal()).len() <= 4096. * f64::EPSILON
        && a.normal().dot(b.normal()) > 0.
        && (a.origin - b.origin).dot(a.normal()).abs() <= error
}

fn base_plane(source: &FaceSource, actual: &Plane) -> Option<Plane> {
    match source {
        FaceSource::CertifiedExtrudeSide { plane: Some(p), .. }
        | FaceSource::CertifiedExtrudeCap { plane: p, .. }
        | FaceSource::Box { producer: Some(_), plane: p, .. }
        | FaceSource::CylinderCap { plane: p, .. }
        | FaceSource::ShellPlanar { producer: Some(_), plane: p, .. } => Some(*p),
        FaceSource::Imported { producer: Some(_), payload_sha256: Some(_), .. } => Some(*actual),
        _ => None,
    }
}

pub(super) fn certified_plane(source: &FaceSource) -> Option<Plane> {
    match source {
        FaceSource::CertifiedExtrudeSide { plane: Some(p), .. }
        | FaceSource::CertifiedExtrudeCap { plane: p, .. }
        | FaceSource::Box { producer: Some(_), plane: p, .. }
        | FaceSource::CylinderCap { plane: p, .. }
        | FaceSource::ShellPlanar { producer: Some(_), plane: p, .. } => Some(*p),
        _ => None,
    }
}

fn boolean_source(source: &FaceSource, constructed: bool) -> bool {
    matches!(source, FaceSource::ShellPlanar { producer: Some(_), .. }) || (constructed && certified_plane(source).is_some())
}

pub(super) fn descriptor_cost(source: &FaceSource) -> Option<usize> {
    let mut cost = 256usize;
    match source {
        FaceSource::CertifiedExtrudeCap { outer, holes, .. } => {
            if holes.len() > 4096 {
                return None;
            }
            cost = cost.checked_add(holes.len().checked_mul(64)?)?;
            let mut entries = 4096usize;
            for id in outer.iter().chain(holes.iter().flatten()) {
                entries = entries.checked_sub(1)?;
                if id.len() > 128 {
                    return None;
                }
                cost = cost.checked_add(id.len().checked_add(64)?)?;
            }
        }
        FaceSource::CertifiedExtrudeSide { curve, .. } => {
            if curve.len() > 128 {
                return None;
            }
            cost = cost.checked_add(curve.len())?;
        }
        FaceSource::Imported { payload_sha256: Some(digest), .. } => {
            if digest.len() > 64 {
                return None;
            }
            cost = cost.checked_add(digest.len())?;
        }
        FaceSource::ShellPlanar { source, .. } if !matches!(source.as_ref(), FaceSource::ShellPlanar { .. }) => {
            cost = cost.checked_add(descriptor_cost(source)?)?;
        }
        FaceSource::ExtrudeCap { region, .. } => {
            if region.holes.len() > 8192 {
                return None;
            }
            cost = cost.checked_add(region.holes.len().checked_mul(64)?)?;
            let count = region.holes.iter().try_fold(region.outer.segs.len(), |sum, h| sum.checked_add(h.segs.len()))?;
            if count > 8192 {
                return None;
            }
            cost = cost.checked_add(count.checked_mul(128)?)?;
        }
        FaceSource::ExtrudeSide { .. } | FaceSource::Imported { .. } | FaceSource::Box { .. } | FaceSource::CylinderCap { .. } => {}
        _ => return None,
    }
    Some(cost)
}

impl Body {
    /// Preserve uniquely certified unchanged planar construction supports.
    pub fn with_preserved_planar_sources(self, input: &Body) -> Result<Self> {
        self.map_planar_sources(&[input], None, None, &[], false)
    }

    /// Generate distinct outer/inner roles from the exact original supports of
    /// an inside shell. Generated rims and curved walls are intentionally absent.
    pub(crate) fn with_shell_planar_sources(self, input: &Body, thickness: f64, open: &[Vec3]) -> Result<Self> {
        self.map_planar_sources(&[input], Some(thickness), None, open, false)
    }

    /// Retain exact shell roles, and construction roles for Cut, only on unchanged
    /// supports without a competing coplanar tool. No imported or fitted role is inferred.
    pub(crate) fn with_preserved_boolean_planar_sources(self, input: &Body, tool: &Body, constructed: bool) -> Result<Self> {
        let self_ = self.inherit_regularization_protection(input).inherit_regularization_protection(tool);
        if !input.face_sources().is_some_and(|sources| sources.iter().flatten().any(|s| boolean_source(s, constructed))) {
            return Ok(self_);
        }
        self_.map_planar_sources(&[input], None, Some(tool), &[], constructed)
    }

    /// Retain exact construction and shell roles through a Union from both operands, only on
    /// unchanged supports. Coplanar supports of the two operands carry distinct identities, so
    /// a face merged from both stays without a role.
    pub(crate) fn with_preserved_union_planar_sources(self, a: &Body, b: &Body) -> Result<Self> {
        let self_ = self.inherit_regularization_protection(a).inherit_regularization_protection(b);
        if ![a, b].iter().any(|input| input.face_sources().is_some_and(|sources| sources.iter().flatten().any(|s| boolean_source(s, true)))) {
            return Ok(self_);
        }
        self_.map_planar_sources(&[a, b], None, None, &[], true)
    }

    fn map_planar_sources(mut self, inputs: &[&Body], shell: Option<f64>, blocker: Option<&Body>, open: &[Vec3], constructed: bool) -> Result<Self> {
        // Every boolean (a Cut's tool, or a Union's second operand) leaves the result with
        // faces whose support no input role describes.
        let boolean = blocker.is_some() || inputs.len() > 1;
        for input in inputs {
            self.regularization_protected |= input.regularization_protected;
        }
        if inputs.iter().all(|input| input.face_sources().is_none()) {
            return Ok(self);
        }
        if inputs.len() > 2
            || inputs.iter().any(|input| input.face_count() > 16_384)
            || self.face_count() > 16_384
            || blocker.is_some_and(|b| b.face_count() > 16_384)
        {
            return Err(KernelError::Invalid("planar source face budget exceeded".into()));
        }
        if inputs.iter().any(|input| input.face_sources().is_some_and(|sources| sources.len() != input.face_count())) {
            return Err(KernelError::Invalid("planar source alignment mismatch".into()));
        }
        if shell.is_some_and(|t| !t.is_finite() || t <= 0.) {
            return Err(KernelError::Invalid("invalid shell source thickness".into()));
        }
        if open.len() > 256 {
            return Ok(self);
        }
        let opening_tolerance = if shell.is_some() { inputs.iter().map(|input| input.size()).fold(0., f64::max) * 1e-4 + 1e-6 } else { 0. };
        let mapped = guard("exact planar support lineage", || {
            let mut competing = Vec::new();
            for plane in blocker.map(actual_planes).transpose()?.into_iter().flatten().flatten() {
                // A tool face blocks a face on its plane facing either way.
                let opposing = Plane::new(plane.origin, plane.x, -plane.y).ok_or_else(|| KernelError::Invalid("invalid opposing plane".into()))?;
                competing.extend([plane, opposing]);
            }
            competing.sort_by(|a, b| a.normal().x.total_cmp(&b.normal().x));
            let mut candidates = Vec::new();
            let mut unknown_supports = Vec::new();
            let mut faces = Vec::new();
            for input in inputs {
                let sources = input.face_sources().unwrap_or_default();
                faces
                    .extend(actual_planes(input)?.into_iter().enumerate().map(|(index, plane)| (plane, sources.get(index).and_then(Option::as_ref))));
            }
            for (actual, source) in faces {
                let Some(actual) = actual else { continue };
                let Some(source) = source else {
                    unknown_supports.push(actual);
                    continue;
                };
                if (boolean && !boolean_source(source, constructed)) || (shell.is_some() && matches!(source, FaceSource::ShellPlanar { .. })) {
                    unknown_supports.push(actual);
                    continue;
                }
                let Some(plane) = base_plane(source, &actual) else {
                    unknown_supports.push(actual);
                    continue;
                };
                if !same_support(&plane, &actual) || (shell.is_some() && open.iter().any(|p| actual.height(*p).abs() <= opening_tolerance)) {
                    unknown_supports.push(actual);
                    continue;
                }
                let Some(cost) = descriptor_cost(source) else {
                    unknown_supports.push(actual);
                    continue;
                };
                candidates.push((plane, source, false, cost));
                if let Some(t) = shell {
                    let inner = Plane::new(plane.origin - plane.normal() * t, plane.x, -plane.y)
                        .ok_or_else(|| KernelError::Invalid("invalid shell inner source plane".into()))?;
                    candidates.push((inner, source, true, cost));
                }
            }
            // Planes that match must have (nearly) the same unit normal (`same_support`), so the
            // lists are sorted by the normal's x and each face looks only at its window of them.
            unknown_supports.sort_by(|a, b| a.normal().x.total_cmp(&b.normal().x));
            candidates.sort_by(|a, b| a.0.normal().x.total_cmp(&b.0.normal().x));
            let mut work = 2_000_000usize;
            let mut metadata = 2_000_000usize;
            let mut identity_bytes = 16_000_000usize;
            let mut result = Vec::new();
            for (index, actual) in actual_planes(&self)?.into_iter().enumerate() {
                let mut found = None;
                if let Some(actual) = actual {
                    let mut blocked = false;
                    let nx = actual.normal().x;
                    for plane in normal_window(&unknown_supports, |p| p, nx) {
                        work = work.checked_sub(1).ok_or_else(|| KernelError::Invalid("unknown planar source comparison budget exceeded".into()))?;
                        if same_support(plane, &actual) {
                            blocked = true;
                            break;
                        }
                    }
                    for plane in normal_window(&competing, |p| p, nx) {
                        work = work.checked_sub(1).ok_or_else(|| KernelError::Invalid("planar tool comparison budget exceeded".into()))?;
                        if same_support(plane, &actual) {
                            blocked = true;
                            break;
                        }
                    }
                    if blocked {
                        let existing = self
                            .face_sources()
                            .and_then(|sources| sources.get(index))
                            .and_then(|s| s.as_ref())
                            .filter(|s| (boolean || self.face_was_coalesced(index)) && !boolean_source(s, constructed));
                        let keep = existing.and_then(|s| {
                            descriptor_cost(s).and_then(|cost| {
                                metadata.checked_sub(cost).map(|remaining| {
                                    metadata = remaining;
                                    s.clone()
                                })
                            })
                        });
                        result.push(keep);
                        continue;
                    }
                    let mut chosen: Option<(Plane, &FaceSource, bool, usize)> = None;
                    for (plane, source, inner, cost) in normal_window(&candidates, |c| &c.0, nx) {
                        work = work.checked_sub(1).ok_or_else(|| KernelError::Invalid("planar source comparison budget exceeded".into()))?;
                        if same_support(plane, &actual) {
                            if let Some((previous_plane, previous_source, previous_inner, _)) = chosen {
                                let Some(remaining) = identity_bytes.checked_sub(*cost) else {
                                    chosen = None;
                                    break;
                                };
                                identity_bytes = remaining;
                                // Several current pieces of one exact source are one
                                // logical face; distinct source identities remain ambiguous.
                                if previous_plane != *plane || previous_source != *source || previous_inner != *inner {
                                    chosen = None;
                                    break;
                                }
                            } else {
                                chosen = Some((*plane, *source, *inner, *cost));
                            }
                        }
                    }
                    if let Some((plane, source, inner, cost)) = chosen
                        && let Some(remaining) = metadata.checked_sub(cost)
                    {
                        metadata = remaining;
                        found = Some(if shell.is_some() {
                            FaceSource::ShellPlanar { producer: None, source: Box::new(source.clone()), inner, plane }
                        } else {
                            source.clone()
                        });
                    }
                }
                if found.is_none() && (boolean || self.face_was_coalesced(index)) {
                    found = self
                        .face_sources()
                        .and_then(|sources| sources.get(index))
                        .and_then(|s| s.as_ref())
                        .filter(|s| !boolean_source(s, constructed))
                        .and_then(|s| {
                            descriptor_cost(s).and_then(|cost| {
                                metadata.checked_sub(cost).map(|remaining| {
                                    metadata = remaining;
                                    s.clone()
                                })
                            })
                        });
                }
                result.push(found);
            }
            Ok(result)
        })?;
        self.face_sources = Some(Arc::new(mapped));
        self.regularize_certified_planar_faces()
    }

    pub fn with_shell_certificate(mut self, producer: u64) -> Result<Self> {
        if producer == 0 {
            return Err(KernelError::Invalid("invalid shell source producer".into()));
        }
        if let Some(sources) = &self.face_sources {
            let mut sources = sources.as_ref().clone();
            for source in sources.iter_mut().flatten() {
                if let FaceSource::ShellPlanar { producer: id, .. } = source {
                    *id = Some(producer);
                }
            }
            self.face_sources = Some(Arc::new(sources));
        }
        self.regularize_certified_planar_faces()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{box_solid, fillet, shell, transform_matrix};

    /// The corner points of face `i` of `b`, from its record.
    fn face_points(b: &Body, i: usize) -> Vec<Vec3> {
        let record = crate::native::brep_record(b).unwrap();
        let mut out = Vec::new();
        for l in &record.faces[i].loops {
            match l {
                brep::Loop::Edges { edges } => {
                    for e in edges {
                        for v in record.edges[e.edge as usize].vertices {
                            let p = record.vertices[v as usize].point;
                            out.push(Vec3::new(p[0], p[1], p[2]));
                        }
                    }
                }
                brep::Loop::Vertex { vertex } => {
                    let p = record.vertices[*vertex as usize].point;
                    out.push(Vec3::new(p[0], p[1], p[2]));
                }
            }
        }
        out
    }

    /// The edges of face `i` of `b` as their end points, from its record.
    fn face_edges(b: &Body, i: usize) -> Vec<(Vec3, Vec3)> {
        let record = crate::native::brep_record(b).unwrap();
        let at = |v: u32| {
            let p = record.vertices[v as usize].point;
            Vec3::new(p[0], p[1], p[2])
        };
        record.faces[i]
            .loops
            .iter()
            .flat_map(|l| match l {
                brep::Loop::Edges { edges } => edges.iter().map(|e| record.edges[e.edge as usize].vertices).map(|[a, b]| (at(a), at(b))).collect(),
                brep::Loop::Vertex { .. } => Vec::new(),
            })
            .collect()
    }

    /// One body holding `a` and `b` side by side (their records' shells together, `b` wholly
    /// beyond `a` along x), each face's source that of the face of its own operand on the same
    /// plane (none from `b` when `b_sources` is false).
    fn side_by_side(a: &Body, b: &Body, b_sources: bool) -> Body {
        let (ra, rb) = (crate::native::brep_record(a).unwrap(), crate::native::brep_record(b).unwrap());
        let mut r = ra.clone();
        let (nf, ne, nv, ns) = (r.faces.len() as u32, r.edges.len() as u32, r.vertices.len() as u32, r.shells.len() as u32);
        for mut face in rb.faces.clone() {
            for l in &mut face.loops {
                match l {
                    brep::Loop::Edges { edges } => edges.iter_mut().for_each(|e| e.edge += ne),
                    brep::Loop::Vertex { vertex } => *vertex += nv,
                }
            }
            r.faces.push(face);
        }
        for mut e in rb.edges.clone() {
            e.vertices = e.vertices.map(|v| v + nv);
            e.uses.iter_mut().for_each(|u| u.face += nf);
            r.edges.push(e);
        }
        r.vertices.extend(rb.vertices.iter().cloned());
        for mut sh in rb.shells.clone() {
            if let brep::Shell::Faces { faces, wires } = &mut sh {
                faces.iter_mut().for_each(|o| o.face += nf);
                wires.iter_mut().for_each(|w| *w += ne);
            }
            r.shells.push(sh);
        }
        for mut region in rb.regions.clone().into_iter().filter(|g| !g.void) {
            region.shells.iter_mut().for_each(|s| *s += ns);
            r.regions.push(region);
        }
        let mut combined = crate::native::body_from_smlib(&solidcraft_smlib::Brep::from_record(&r, true).unwrap()).unwrap();
        let planes = |x: &Body| actual_planes(x).unwrap();
        let (pa, pb, pc) = (planes(a), planes(b), planes(&combined));
        let a_right = a.outline_bounds().max.x;
        let sources: Vec<Option<FaceSource>> = pc
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let p = (*p)?;
                let find = |of: &Body, ps: &[Option<Plane>]| {
                    ps.iter().position(|q| q.is_some_and(|q| same_support(&q, &p))).and_then(|i| of.face_sources()?[i].clone())
                };
                let in_a = face_points(&combined, i).iter().all(|q| q.x <= a_right + 1e-9);
                if in_a {
                    find(a, &pa)
                } else if b_sources {
                    find(b, &pb)
                } else {
                    None
                }
            })
            .collect();
        combined.face_sources = Some(Arc::new(sources));
        combined
    }

    #[test]
    fn rounded_shell_planar_roles_are_exact_distinct_and_transformable() {
        for (width, thickness) in [(80., 2.), (100., 3.)] {
            let box_body = box_solid(Vec3::new(-width / 2., -30., 0.), Vec3::new(width / 2., 30., 30.)).unwrap().with_box_certificate(7).unwrap();
            let rounded = fillet(
                &box_body,
                &[
                    Vec3::new(width / 2., 30., 15.),
                    Vec3::new(-width / 2., 30., 15.),
                    Vec3::new(-width / 2., -30., 15.),
                    Vec3::new(width / 2., -30., 15.),
                ],
                6.,
            )
            .unwrap()
            .with_preserved_planar_sources(&box_body)
            .unwrap();
            assert_eq!(rounded.face_sources().unwrap().iter().flatten().count(), 6);
            let cup = shell(&rounded, &[Vec3::new(0., 0., 30.)], thickness).unwrap().with_shell_certificate(9).unwrap();
            let sources = cup.face_sources().unwrap();
            assert!(
                !sources
                    .iter()
                    .flatten()
                    .any(|s| matches!(s,FaceSource::ShellPlanar {source, ..} if matches!(source.as_ref(),FaceSource::Box {axis:2,high:true,..})))
            );
            let right = sources.iter().position(|s| matches!(s, Some(FaceSource::ShellPlanar { inner:false, source, .. }) if matches!(source.as_ref(),FaceSource::Box {axis:0,high:true,..}))).unwrap();
            let floor = sources.iter().position(|s| matches!(s, Some(FaceSource::ShellPlanar { inner:true, source, .. }) if matches!(source.as_ref(),FaceSource::Box {axis:2,high:false,..}))).unwrap();
            assert!((cup.face_plane(right).unwrap().origin.x - width / 2.).abs() < 1e-9);
            let floor_plane = cup.face_plane(floor).unwrap();
            assert!((floor_plane.origin.z - thickness).abs() < 1e-9);
            assert!(floor_plane.normal().dot(Vec3::Z) > 0.999999);
            for (i, s) in sources.iter().enumerate() {
                if matches!(s, Some(FaceSource::ExtrudeSide { coalesced: true, plane: None, .. })) {
                    assert!(cup.face_plane(i).is_none(), "loss witness is not source authority");
                } else if s.is_some() {
                    assert!(same_support(&cup.face_plane(i).unwrap(), &cup.planar_face(i).unwrap()));
                }
            }
            let moved = transform_matrix(&cup, [[0., 1., 0., 0.], [-1., 0., 0., 0.], [0., 0., 1., 0.], [100., 200., 300., 1.]]).unwrap();
            assert!((moved.face_plane(floor).unwrap().origin.z - (300. + thickness)).abs() < 1e-9);
            assert!(matches!(moved.face_sources().unwrap().get(right), Some(Some(FaceSource::ShellPlanar { producer: Some(9), inner: false, .. }))));
            assert!(moved.face_plane(right).unwrap().normal().dot(Vec3::Y) > 0.999999);
            let raw = cup.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
            let display = cup.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
            let cylindrical: Vec<_> = (0..cup.face_count())
                .filter_map(|i| {
                    let crate::FaceSurface::Cylinder { radius, .. } = crate::face_surface(&cup, i)? else { return None };
                    let z: Vec<f64> = face_points(&cup, i).iter().map(|p| p.z).collect();
                    let lo = z.iter().copied().fold(f64::INFINITY, f64::min);
                    let hi = z.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    Some((i, radius, lo, hi))
                })
                .collect();
            eprintln!("roundedShell width{width} thickness{thickness} raw{} cylindrical roles{cylindrical:?}", cup.face_count());
            let mut corner_joins = 0;
            for (edge, adjacent) in raw.edge_faces.iter().enumerate() {
                let [a, b] = adjacent.as_slice() else { continue };
                if raw.edges[edge].iter().all(|p| (p.z - thickness).abs() < 1e-8)
                    && cylindrical.iter().any(|(i, r, _, _)| *i == *a as usize && (*r - 6.).abs() < 1e-6)
                    && cylindrical.iter().any(|(i, r, _, _)| *i == *b as usize && (*r - 6.).abs() < 1e-6)
                {
                    corner_joins += 1;
                    assert!(display.seams[edge], "existing cylindrical corner joins must not draw a floor-height arc");
                }
            }
            assert_eq!(corner_joins, 0, "no physical lower corner strip remains");
            assert_eq!(cup.face_count(), 19);
            let outer: Vec<_> = cylindrical.iter().filter(|(_, r, _, _)| (*r - 6.).abs() < 1e-6).collect();
            assert_eq!(outer.len(), 4);
            assert!(outer.iter().all(|(i, _, _, _)| cup.face_was_coalesced(*i) && moved.face_was_coalesced(*i)));
            assert!(outer.iter().all(|(_, _, lo, hi)| lo.abs() < 1e-9 && (*hi - 30.).abs() < 1e-9));

            for axis in 0..2 {
                for high in [false, true] {
                    let matching: Vec<_> = sources.iter().enumerate().filter_map(|(i,s)| matches!(s,Some(FaceSource::ShellPlanar {inner:false,source,..}) if matches!(source.as_ref(),FaceSource::Box {axis:a,high:h,..} if *a==axis && *h==high)).then_some(i)).collect();
                    assert_eq!(matching.len(), 1, "each straight exterior wall is one actual face");
                    let z: Vec<_> = face_points(&cup, matching[0]).iter().map(|p| p.z).collect();
                    assert!(z.iter().any(|z| z.abs() < 1e-9));
                    assert!(z.iter().any(|z| (*z - 30.).abs() < 1e-9));
                }
            }
            assert!(display.contains(Vec3::new(width / 2. - thickness / 2., 0., thickness / 2.)));
            assert!(display.contains(Vec3::new(width / 2. - thickness / 2., 0., 15.)));
            assert!(!display.contains(Vec3::new(0., 0., 15.)));
            // Different meshers (the display plan and the reference), both within 0.01 mm.
            let (rv, dv) = (raw.measure().volume, display.measure().volume);
            assert!((rv - dv).abs() <= raw.measure().area * 0.01, "display volume {dv} vs {rv}");
            let expected = (width * 60. - (4. - std::f64::consts::PI) * 36.) * 30.
                - ((width - 2. * thickness) * (60. - 2. * thickness) - (4. - std::f64::consts::PI) * (6. - thickness).powi(2)) * (30. - thickness);
            assert!((cup.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume - expected).abs() < 0.2);
            let imported = crate::step_import(&crate::step_export(&[&cup], "regularized rounded shell").unwrap()).unwrap();
            assert!(imported.warnings.is_empty());
            let copy = &imported.bodies[0].body;
            assert_eq!(copy.face_count(), 19);
            // STEP may additionally coalesce cylindrical corner patches. The
            // certified planar wall topology itself must survive unchanged.
            for (axis, coordinate) in [(Vec3::X, width / 2.), (-Vec3::X, width / 2.), (Vec3::Y, 30.), (-Vec3::Y, 30.)] {
                let walls: Vec<usize> = actual_planes(copy)
                    .unwrap()
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.is_some_and(|p| p.normal().dot(axis) > 0.999 && (p.origin.dot(axis) - coordinate).abs() < 1e-8))
                    .map(|(i, _)| i)
                    .collect();
                assert_eq!(walls.len(), 1);
                let z: Vec<_> = face_points(copy, walls[0]).iter().map(|p| p.z).collect();
                assert!(z.iter().any(|z| z.abs() < 1e-8));
                assert!(z.iter().any(|z| (*z - 30.).abs() < 1e-8));
            }
            let copy_mesh = copy.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap();
            assert!((copy_mesh.measure().volume - expected).abs() < 0.2);
            assert!(copy_mesh.contains(Vec3::new(width / 2. - thickness / 2., 0., 15.)));
            assert!(!copy_mesh.contains(Vec3::new(0., 0., 15.)));
        }
    }
    #[test]
    fn simple_shell_has_one_full_height_face_per_outer_wall_and_exports_that_topology() {
        let source = box_solid(Vec3::ZERO, Vec3::new(40., 30., 20.)).unwrap().with_box_certificate(1).unwrap();
        let pending = shell(&source, &[Vec3::new(20., 15., 20.)], 2.).unwrap();
        let cup = pending.clone().with_shell_certificate(2).unwrap();
        assert_eq!(cup.face_count(), 11);
        let mesh = cup.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
        assert!((mesh.measure().volume - (40. * 30. * 20. - 36. * 26. * 18.)).abs() < 1e-6);
        assert!(mesh.contains(Vec3::new(20., 1., 1.)));
        assert!(mesh.contains(Vec3::new(20., 1., 10.)));
        assert!(!mesh.contains(Vec3::new(20., 15., 10.)));
        let imported = crate::step_import(&crate::step_export(&[&cup], "regularized shell").unwrap()).unwrap();
        assert!(imported.warnings.is_empty());
        let copy = &imported.bodies[0].body;
        assert_eq!(copy.face_count(), 11);
        assert!((copy.mesh(&crate::MeshTolerance::chord(0.01)).unwrap().measure().volume - mesh.measure().volume).abs() < 1e-6);
    }
    #[test]
    fn union_keeps_unchanged_supports_of_both_operands_and_drops_shared_ones() {
        use crate::{BoolOp, boolean};
        let a = box_solid(Vec3::ZERO, Vec3::new(20., 20., 10.)).unwrap().with_box_certificate(1).unwrap();
        // A taller box overlapping one side: the bottoms share z = 0, the tops do not.
        let b = box_solid(Vec3::new(10., 5., 0.), Vec3::new(30., 15., 15.)).unwrap().with_box_certificate(2).unwrap();
        let joined = boolean(&a, &b, BoolOp::Union).unwrap().unwrap();
        let sources = joined.face_sources().unwrap();
        let mut seen = Vec::new();
        for (i, plane) in actual_planes(&joined).unwrap().into_iter().enumerate() {
            let plane = plane.unwrap();
            let source = sources[i].as_ref();
            let height = plane.origin.z;
            if plane.normal().z > 0.999 && (height - 10.).abs() < 1e-9 {
                assert!(matches!(source, Some(FaceSource::Box { producer: Some(1), axis: 2, high: true, .. })), "{source:?}");
                seen.push("a top");
            } else if plane.normal().z > 0.999 && (height - 15.).abs() < 1e-9 {
                assert!(matches!(source, Some(FaceSource::Box { producer: Some(2), axis: 2, high: true, .. })), "{source:?}");
                seen.push("b top");
            } else if plane.normal().z < -0.999 {
                assert!(source.is_none(), "a bottom merged from both operands has no single identity: {source:?}");
                seen.push("bottom");
            }
        }
        for want in ["a top", "b top", "bottom"] {
            assert!(seen.contains(&want), "{want} missing from {seen:?}");
        }
    }
    #[test]
    fn coincident_competing_source_planes_and_tool_planes_stay_unknown() {
        let a = box_solid(Vec3::ZERO, Vec3::new(10., 10., 10.)).unwrap().with_box_certificate(1).unwrap();
        let b = box_solid(Vec3::new(20., 0., 0.), Vec3::new(30., 10., 10.)).unwrap().with_box_certificate(2).unwrap();
        let combined = side_by_side(&a, &b, true);
        let result = a.clone().with_preserved_planar_sources(&combined).unwrap();
        for (i, p) in actual_planes(&result).unwrap().into_iter().enumerate() {
            if p.unwrap().normal().dot(Vec3::Z).abs() > 0.999 {
                assert!(result.face_sources().unwrap()[i].is_none());
            }
        }
        let mixed = side_by_side(&a, &b, false);
        let guarded = a.clone().with_preserved_planar_sources(&mixed).unwrap();
        for (i, p) in actual_planes(&guarded).unwrap().into_iter().enumerate() {
            if p.unwrap().normal().dot(Vec3::Z).abs() > 0.999 {
                assert!(guarded.face_sources().unwrap()[i].is_none(), "uncertified competing support must not gain identity");
            }
        }
        let cup = shell(&a, &[Vec3::new(5., 5., 10.)], 1.).unwrap().with_shell_certificate(3).unwrap();
        let tool = box_solid(Vec3::new(2., 2., 1.), Vec3::new(4., 4., 3.)).unwrap();
        let preserved = cup.clone().with_preserved_boolean_planar_sources(&cup, &tool, false).unwrap();
        for (i, p) in actual_planes(&preserved).unwrap().into_iter().enumerate() {
            let Some(p) = p else { continue };
            if p.normal().dot(Vec3::Z) > 0.999 && (p.origin.z - 1.).abs() < 1e-9 {
                assert!(preserved.face_sources().unwrap()[i].is_none());
            }
        }
    }

    #[test]
    fn pocket_cut_removes_internal_wall_faces_in_actual_topology() {
        use solidcraft_geom::{Loop2, Region2, Vec2};
        let region = Region2 {
            outer: Loop2::polygon(&[Vec2::new(-30., -27.), Vec2::new(30., -27.), Vec2::new(30., 27.), Vec2::new(-30., 27.)]),
            holes: vec![],
        };
        let original = crate::extrude(&Plane::XY, std::slice::from_ref(&region), 0., 10.).unwrap().pop().unwrap();
        let curves: Vec<_> = original
            .face_sources()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, source)| matches!(source, Some(FaceSource::ExtrudeSide { .. })).then(|| format!("l{i}")))
            .collect();
        let original = original.with_extrude_certificates(2, 1, &curves).unwrap();
        let (x, y) = (24.73317426966596, 21.733174269665962);
        let cutter = box_solid(Vec3::new(-x, -y, 1.), Vec3::new(x, y, 10.)).unwrap();
        let pocket = crate::boolean(&original, &cutter, crate::BoolOp::Cut).unwrap().unwrap();
        let raw = pocket.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
        let display = pocket.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
        let sources = pocket.face_sources().unwrap();
        assert_eq!(pocket.face_count(), 11, "four outer walls, four inner walls, rim, floor, bottom");
        let sides: Vec<_> =
            sources.iter().enumerate().filter_map(|(i, s)| matches!(s, Some(FaceSource::CertifiedExtrudeSide { .. })).then_some(i)).collect();
        assert_eq!(sides.len(), 4);
        for i in sides {
            let z: Vec<_> = face_points(&pocket, i).iter().map(|p| p.z).collect();
            assert!(z.iter().any(|z| z.abs() < 1e-9));
            assert!(z.iter().any(|z| (*z - 10.).abs() < 1e-9));
            assert!(!face_edges(&pocket, i).iter().any(|(a, b)| (a.z - 1.).abs() < 1e-9 && (b.z - 1.).abs() < 1e-9));
        }
        assert!(display.contains(Vec3::new(0., 0., 0.5)));
        assert!(!display.contains(Vec3::new(0., 0., 5.)));
        assert!(display.contains(Vec3::new(28., 0., 5.)));
        assert!((display.measure().volume - (60. * 54. * 10. - 4. * x * y * 9.)).abs() < 1e-7);
        // Different triangulations of the same polyhedron: equal up to rounding.
        assert!((raw.measure().volume - display.measure().volume).abs() < 1e-9 * raw.measure().volume);
        let imported = crate::step_import(&crate::step_export(&[&pocket], "regularized pocket").unwrap()).unwrap();
        assert!(imported.warnings.is_empty());
        let copy = &imported.bodies[0].body;
        assert_eq!(copy.face_count(), 11);
        let copy_mesh = copy.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
        assert!((copy_mesh.measure().volume - raw.measure().volume).abs() < 1e-6);
        assert!(copy_mesh.contains(Vec3::new(0., 0., 0.5)));
        assert!(!copy_mesh.contains(Vec3::new(0., 0., 5.)));
        let competing = box_solid(Vec3::new(30., -1., 0.), Vec3::new(31., 1., 2.)).unwrap();
        let guarded = pocket.clone().with_preserved_boolean_planar_sources(&pocket, &competing, true).unwrap();
        for (index, source) in guarded.face_sources().unwrap().iter().enumerate() {
            if pocket.face_plane(index).is_some_and(|p| p.normal().dot(Vec3::X) > 0.999 && (p.origin.x - 30.).abs() < 1e-9) {
                assert!(source.is_none(), "competing tool plane cannot retain source identity");
            }
        }
    }
}
