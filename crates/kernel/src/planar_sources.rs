//! Exact oriented planar support lineage through a trim and an inside shell.
//! No tessellated face names, centroids, or nearest-plane choices are used.
use super::{Body, FaceSource, from_p3};
use crate::{KernelError, Result, guard};
use solidcraft_geom::{Plane, Vec3};
use std::sync::Arc;
use truck_modeling as mt;

pub(super) fn actual_plane(face: &mt::Face) -> Option<Plane> {
    let mt::Surface::Plane(p) = face.oriented_surface() else { return None };
    let u = p.u_axis();
    let v = p.v_axis();
    let u = Vec3::new(u.x, u.y, u.z);
    let v = Vec3::new(v.x, v.y, v.z);
    if !u.is_finite() || !v.is_finite() || u.cross(v).len() <= u.len() * v.len() * 1e-8 {
        return None;
    }
    Plane::new(from_p3(p.origin()), u, v)
}

/// A plane's signed distance from the world origin along its normal.
fn offset(plane: &Plane) -> f64 {
    plane.normal().dot(plane.origin)
}

/// How far apart the [`offset`]s of two planes `same_support` accepts can be, in units of
/// `EPSILON * (1 + |a.origin| + |b.origin|)`. Their origins' gap along `a`'s normal is within
/// 4096 of those units, and their unit normals within about 4096 * EPSILON of each other, which
/// moves the offset by at most that times `|b.origin|`; the rest is room for rounding.
const OFFSET_SLACK: f64 = 16384.;

/// Planes sorted by [`offset`], so a face looks only at the window that `same_support` can accept.
/// Sorting by normal alone put every face of an axis-aligned part (all normals along X, Y or Z)
/// into a few windows, so a cut with thousands of coplanar tool faces compared each result face
/// with all of them.
struct SupportIndex<T> {
    sorted: Vec<T>,
    /// The longest plane origin in `sorted`, for the window's width.
    reach: f64,
}

impl<T> SupportIndex<T> {
    fn new(mut items: Vec<T>, plane: impl Fn(&T) -> &Plane) -> Self {
        items.sort_by(|a, b| offset(plane(a)).total_cmp(&offset(plane(b))));
        let reach = items.iter().map(|t| plane(t).origin.len()).fold(0., f64::max);
        SupportIndex { sorted: items, reach }
    }

    /// The entries whose plane `same_support` can accept for `actual` (and possibly a few more).
    fn window(&self, plane: impl Fn(&T) -> &Plane, actual: &Plane) -> &[T] {
        let d = offset(actual);
        let tolerance = OFFSET_SLACK * f64::EPSILON * (1. + actual.origin.len() + self.reach);
        let first = self.sorted.partition_point(|t| offset(plane(t)) < d - tolerance);
        let last = self.sorted.partition_point(|t| offset(plane(t)) <= d + tolerance);
        self.sorted.get(first..last.max(first)).unwrap_or_default()
    }
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
            || inputs.iter().any(|input| input.solid.face_iter().count() > 16_384)
            || self.solid.face_iter().count() > 16_384
            || blocker.is_some_and(|b| b.solid.face_iter().count() > 16_384)
        {
            return Err(KernelError::Invalid("planar source face budget exceeded".into()));
        }
        if inputs.iter().any(|input| input.face_sources().is_some_and(|sources| sources.len() != input.solid.face_iter().count())) {
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
            for plane in blocker.into_iter().flat_map(|b| b.solid.face_iter()).filter_map(actual_plane) {
                // A tool face blocks a face on its plane facing either way.
                let opposing = Plane::new(plane.origin, plane.x, -plane.y).ok_or_else(|| KernelError::Invalid("invalid opposing plane".into()))?;
                competing.extend([plane, opposing]);
            }
            let mut candidates = Vec::new();
            let mut unknown_supports = Vec::new();
            let faces = inputs.iter().flat_map(|input| {
                let sources = input.face_sources().unwrap_or_default();
                input.solid.face_iter().enumerate().map(move |(index, face)| (face, sources.get(index).and_then(Option::as_ref)))
            });
            for (face, source) in faces {
                let Some(actual) = actual_plane(face) else { continue };
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
            // Planes that match lie (nearly) on one support (`same_support`), so the lists are
            // indexed by offset and each face looks only at its window of them.
            let unknown_supports = SupportIndex::new(unknown_supports, |p| p);
            let competing = SupportIndex::new(competing, |p| p);
            let candidates = SupportIndex::new(candidates, |c| &c.0);
            let mut work = 2_000_000usize;
            let mut metadata = 2_000_000usize;
            let mut identity_bytes = 16_000_000usize;
            let mut result = Vec::new();
            for (index, face) in self.solid.face_iter().enumerate() {
                let mut found = None;
                if let Some(actual) = actual_plane(face) {
                    let mut blocked = false;
                    for plane in unknown_supports.window(|p| p, &actual) {
                        work = work.checked_sub(1).ok_or_else(|| KernelError::Invalid("unknown planar source comparison budget exceeded".into()))?;
                        if same_support(plane, &actual) {
                            blocked = true;
                            break;
                        }
                    }
                    for plane in competing.window(|p| p, &actual) {
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
                    for (plane, source, inner, cost) in candidates.window(|c| &c.0, &actual) {
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

    #[test]
    fn support_index_window_holds_every_plane_same_support_accepts() {
        // Axis-aligned planes on a 2.54 grid both ways, as a grooved and perforated plate has,
        // some with origins far out in their plane, plus near copies that `same_support` still
        // accepts: nudged along the normal, tilted by a few ulps, or both.
        let axes = [(Vec3::X, Vec3::Y), (Vec3::Y, Vec3::Z), (Vec3::Z, Vec3::X)];
        let mut planes = Vec::new();
        for k in 0..120 {
            let t = k as f64 * 2.54 - 150.;
            for (x, y) in axes {
                let n = x.cross(y);
                let along = x * (if k % 3 == 0 { 1.0e3 } else { k as f64 });
                let origin = n * t + along;
                for (x, y) in [(x, y), (x, -y)] {
                    planes.push(Plane::new(origin, x, y).unwrap());
                    let nudge = n * (1024. * f64::EPSILON * (1. + origin.len()));
                    planes.push(Plane::new(origin + nudge, x, y).unwrap());
                    planes.push(Plane::new(origin - nudge, x + n * (1000. * f64::EPSILON), y).unwrap());
                }
            }
        }
        let index = SupportIndex::new(planes.clone(), |p| p);
        let mut accepted = 0;
        for actual in &planes {
            let window = index.window(|p| p, actual);
            for plane in &planes {
                if same_support(plane, actual) {
                    accepted += 1;
                    assert!(window.contains(plane), "{plane:?} accepted for {actual:?} but outside its window");
                }
            }
            // Only a plane's own near copies share its offset (plus the odd other axis's plane
            // at the same offset), not the hundreds that share its normal.
            assert!(window.len() <= 12, "window of {} for {actual:?}", window.len());
        }
        assert!(accepted > planes.len(), "the near copies should match each other");
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
            let raw = cup.tessellate(0.01).unwrap();
            let display = cup.display_mesh(0.01).unwrap();
            let cylindrical: Vec<_> = cup
                .solid
                .face_iter()
                .enumerate()
                .filter_map(|(i, f)| {
                    let crate::FaceSurface::Cylinder { radius, .. } = crate::face_surface(&cup, i)? else { return None };
                    let lo = f.vertex_iter().map(|v| v.point().z).fold(f64::INFINITY, f64::min);
                    let hi = f.vertex_iter().map(|v| v.point().z).fold(f64::NEG_INFINITY, f64::max);
                    Some((i, radius, lo, hi))
                })
                .collect();
            eprintln!("roundedShell width{width} thickness{thickness} raw{} cylindrical roles{cylindrical:?}", cup.solid.face_iter().count());
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
                    let face = cup.solid.face_iter().nth(matching[0]).unwrap();
                    let z: Vec<_> = face.vertex_iter().map(|v| v.point().z).collect();
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
            assert!((cup.tessellate(1e-4).unwrap().measure().volume - expected).abs() < 0.2);
            let imported = crate::step_import(&crate::step_export(&[&cup], "regularized rounded shell").unwrap()).unwrap();
            assert!(imported.warnings.is_empty());
            let copy = &imported.bodies[0].body;
            assert_eq!(copy.face_count(), 19);
            // STEP may additionally coalesce cylindrical corner patches. The
            // certified planar wall topology itself must survive unchanged.
            for (axis, coordinate) in [(Vec3::X, width / 2.), (-Vec3::X, width / 2.), (Vec3::Y, 30.), (-Vec3::Y, 30.)] {
                let walls: Vec<_> = copy
                    .solid
                    .face_iter()
                    .filter(|f| actual_plane(f).is_some_and(|p| p.normal().dot(axis) > 0.999 && (p.origin.dot(axis) - coordinate).abs() < 1e-8))
                    .collect();
                assert_eq!(walls.len(), 1);
                let z: Vec<_> = walls[0].vertex_iter().map(|v| v.point().z).collect();
                assert!(z.iter().any(|z| z.abs() < 1e-8));
                assert!(z.iter().any(|z| (*z - 30.).abs() < 1e-8));
            }
            let copy_mesh = copy.tessellate(1e-4).unwrap();
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
        assert_eq!(cup.solid.face_iter().count(), 11);
        let mesh = cup.tessellate(0.01).unwrap();
        assert!((mesh.measure().volume - (40. * 30. * 20. - 36. * 26. * 18.)).abs() < 1e-6);
        assert!(mesh.contains(Vec3::new(20., 1., 1.)));
        assert!(mesh.contains(Vec3::new(20., 1., 10.)));
        assert!(!mesh.contains(Vec3::new(20., 15., 10.)));
        let imported = crate::step_import(&crate::step_export(&[&cup], "regularized shell").unwrap()).unwrap();
        assert!(imported.warnings.is_empty());
        let copy = &imported.bodies[0].body;
        assert_eq!(copy.solid.face_iter().count(), 11);
        assert!((copy.tessellate(0.01).unwrap().measure().volume - mesh.measure().volume).abs() < 1e-6);
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
        for (i, face) in joined.solid.face_iter().enumerate() {
            let plane = actual_plane(face).unwrap();
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
        let mut shells = a.solid.boundaries().to_vec();
        shells.extend(b.solid.boundaries().iter().cloned());
        let mut combined = Body::new(mt::Solid::try_new(shells).unwrap()).unwrap();
        combined.face_sources = Some(Arc::new(a.face_sources().unwrap().iter().chain(b.face_sources().unwrap()).cloned().collect()));
        let result = a.clone().with_preserved_planar_sources(&combined).unwrap();
        for (i, face) in result.solid.face_iter().enumerate() {
            if actual_plane(face).unwrap().normal().dot(Vec3::Z).abs() > 0.999 {
                assert!(result.face_sources().unwrap()[i].is_none());
            }
        }
        let mut mixed = combined.clone();
        mixed.face_sources =
            Some(Arc::new(a.face_sources().unwrap().iter().cloned().chain(std::iter::repeat_n(None, b.solid.face_iter().count())).collect()));
        let guarded = a.clone().with_preserved_planar_sources(&mixed).unwrap();
        for (i, face) in guarded.solid.face_iter().enumerate() {
            if actual_plane(face).unwrap().normal().dot(Vec3::Z).abs() > 0.999 {
                assert!(guarded.face_sources().unwrap()[i].is_none(), "uncertified competing support must not gain identity");
            }
        }
        let cup = shell(&a, &[Vec3::new(5., 5., 10.)], 1.).unwrap().with_shell_certificate(3).unwrap();
        let tool = box_solid(Vec3::new(2., 2., 1.), Vec3::new(4., 4., 3.)).unwrap();
        let preserved = cup.clone().with_preserved_boolean_planar_sources(&cup, &tool, false).unwrap();
        for (i, face) in preserved.solid.face_iter().enumerate() {
            let Some(p) = actual_plane(face) else { continue };
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
        let raw = pocket.tessellate(0.01).unwrap();
        let display = pocket.display_mesh(0.01).unwrap();
        let sources = pocket.face_sources().unwrap();
        assert_eq!(pocket.solid.face_iter().count(), 11, "four outer walls, four inner walls, rim, floor, bottom");
        let sides: Vec<_> =
            sources.iter().enumerate().filter_map(|(i, s)| matches!(s, Some(FaceSource::CertifiedExtrudeSide { .. })).then_some(i)).collect();
        assert_eq!(sides.len(), 4);
        for i in sides {
            let face = pocket.solid.face_iter().nth(i).unwrap();
            let z: Vec<_> = face.vertex_iter().map(|v| v.point().z).collect();
            assert!(z.iter().any(|z| z.abs() < 1e-9));
            assert!(z.iter().any(|z| (*z - 10.).abs() < 1e-9));
            assert!(!face.edge_iter().any(|e| (e.front().point().z - 1.).abs() < 1e-9 && (e.back().point().z - 1.).abs() < 1e-9));
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
        assert_eq!(copy.solid.face_iter().count(), 11);
        let copy_mesh = copy.tessellate(0.01).unwrap();
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
