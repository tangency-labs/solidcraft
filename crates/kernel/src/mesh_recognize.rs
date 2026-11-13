//! Recognizing the analytic surfaces a mesh stands for: the geometry half of mesh-to-CAD.
//!
//! A scan arrives as triangles and carries no idea of a face. This module groups the triangles
//! into patches and names each patch -- a plane, a cylinder, a cone, a sphere, a torus, or
//! freeform where nothing analytic fits -- so a design can be rebuilt from what the part is,
//! with ordinary sketch and feature commands, instead of being frozen as a lump of triangles.
//!
//! A caller may propose the grouping through `groups`, one group number per triangle, as a neural
//! segmenter produces. The proposal only decides which triangles are tried together. What a patch
//! *is* is never taken on trust: every surface is fitted by least squares and then certified
//! against the mesh, each of the patch's vertices within `tolerance` of the surface. A patch that
//! will not certify is split and tried again, and one that still will not is reported as freeform
//! rather than guessed at. A wrong proposal therefore costs recognition, never correctness.
//!
//! Between surfaces that all certify, the simplest wins: a plane before a cylinder or a sphere,
//! those before a cone, a cone before a torus. A more complex surface takes the patch only when it
//! more than halves the deviation, which is what tells a band of a large cylinder (flat to within
//! a scan's noise, and reported flat) from one that is genuinely round.

use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::meshbody::TriMesh;
use crate::{Body, KernelError, Result};

#[path = "mesh_recognize_fit.rs"]
mod fits;

use fits::{fit_cone, fit_cylinder, fit_plane, fit_sphere, fit_torus};

/// Triangles one recognition can take. Reading a scan is the point of this, so the limit is high;
/// the work is linear in the triangles and every loop below is bounded.
const MAX_TRIANGLES: usize = 2_000_000;
/// Patches reported. A part with more is reported as far as this, largest first.
const MAX_PATCHES: usize = 4096;
/// How often a patch that will not certify may be split and tried again.
const MAX_REFINE: u32 = 3;
/// How often a patch that will not certify may be parted by how sharply it turns. Each split
/// leaves both halves strictly smaller than the patch, so this only bounds how finely a surface
/// nothing names is taken apart before it is reported as freeform.
const MAX_SPLITS: u32 = 4;
/// Bins across each face of the cube the normals are sorted into, to ask how much of a patch
/// shares one triangle's own direction. 48 is a bin about four degrees wide, narrower than the
/// facet-to-facet step of a round that any sane tolerance would call round.
const DIRECTIONS: usize = 48;
/// How much more area one side of a split has to share a direction with than the other.
const SHARED_GAP: f64 = 4.;
/// The widest angle between neighbouring triangle normals that still grows one patch (radians,
/// about 20 degrees). Wide enough to carry a tessellated round face in one piece, narrow enough to
/// stop at a crease; a patch it grows too far is split by the refinement above.
const GROW_ANGLE: f64 = 0.35;
/// Triangle visits across all the growing and splitting, before recognition gives up.
const MAX_WORK: usize = 64_000_000;

fn invalid(message: &str) -> KernelError {
    KernelError::Invalid(format!("mesh recognition: {message}"))
}

/// A surface a patch of the mesh was certified as. Lengths are mm, angles radians, directions unit
/// vectors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Surface {
    /// A flat face through `origin` (the patch's centroid) facing `normal`.
    Plane {
        origin: Vec3,
        normal: Vec3,
    },
    /// A circular cylinder: `axis` through `origin`, the point on the axis nearest the patch's
    /// centroid.
    Cylinder {
        origin: Vec3,
        axis: Vec3,
        radius: f64,
    },
    /// A circular cone. `axis` points away from `apex`, the way the radius widens.
    Cone {
        apex: Vec3,
        axis: Vec3,
        half_angle: f64,
    },
    Sphere {
        centre: Vec3,
        radius: f64,
    },
    /// A torus: the `major` circle lies in the plane through `centre` square to `axis`, and the
    /// tube's radius is `minor`. This is what a fillet or a rounded edge comes back as.
    Torus {
        centre: Vec3,
        axis: Vec3,
        major: f64,
        minor: f64,
    },
    /// Nothing analytic certified: a sculpted face, or a part of a scan too noisy to name.
    Freeform,
}

impl Surface {
    /// How many numbers the surface is pinned down by, for choosing the simplest fit that
    /// certifies. Freeform is last of all.
    fn parameters(&self) -> u32 {
        match self {
            Surface::Plane { .. } => 1,
            Surface::Cylinder { .. } | Surface::Sphere { .. } => 2,
            Surface::Cone { .. } => 3,
            Surface::Torus { .. } => 4,
            Surface::Freeform => u32::MAX,
        }
    }

    /// The surface's own axis, where it has one.
    pub fn axis(&self) -> Option<Vec3> {
        match *self {
            Surface::Plane { normal, .. } => Some(normal),
            Surface::Cylinder { axis, .. } | Surface::Cone { axis, .. } | Surface::Torus { axis, .. } => Some(axis),
            Surface::Sphere { .. } | Surface::Freeform => None,
        }
    }

    /// The surface's own origin: what [`Patch::along`] is measured from.
    fn origin(&self) -> Option<Vec3> {
        match *self {
            Surface::Plane { origin, .. } | Surface::Cylinder { origin, .. } => Some(origin),
            Surface::Cone { apex, .. } => Some(apex),
            Surface::Torus { centre, .. } => Some(centre),
            Surface::Sphere { .. } | Surface::Freeform => None,
        }
    }

    /// The surface's own outward direction at a point on or near it: along a plane's normal, away
    /// from a cylinder's axis or a sphere's centre, away from the centre of a torus's tube, and
    /// out of the material of a solid cone. Comparing it with a triangle's own normal says which
    /// side of the surface the material is on.
    fn outward_at(&self, p: Vec3) -> Option<Vec3> {
        let radial = |origin: Vec3, axis: Vec3| (p - origin - axis * (p - origin).dot(axis)).normalized();
        match *self {
            Surface::Plane { normal, .. } => Some(normal),
            Surface::Cylinder { origin, axis, .. } => radial(origin, axis),
            Surface::Cone { apex, axis, half_angle } => {
                radial(apex, axis).and_then(|r| (r * half_angle.cos() - axis * half_angle.sin()).normalized())
            }
            Surface::Sphere { centre, .. } => (p - centre).normalized(),
            Surface::Torus { centre, axis, major, .. } => radial(centre, axis).and_then(|r| (p - (centre + r * major)).normalized()),
            Surface::Freeform => None,
        }
    }

    /// What the surface is called in a reply.
    pub fn name(&self) -> &'static str {
        match self {
            Surface::Plane { .. } => "plane",
            Surface::Cylinder { .. } => "cylinder",
            Surface::Cone { .. } => "cone",
            Surface::Sphere { .. } => "sphere",
            Surface::Torus { .. } => "torus",
            Surface::Freeform => "freeform",
        }
    }
}

/// One group of triangles and the surface it was certified as.
#[derive(Debug, Clone)]
pub struct Patch {
    pub surface: Surface,
    /// How many triangles the patch holds.
    pub triangles: usize,
    /// The patch's area (mm^2).
    pub area: f64,
    /// The greatest distance from the fitted surface over the patch's vertices (mm). Zero for a
    /// freeform patch, which was not fitted.
    pub deviation: f64,
    /// Where the patch sits along its surface's axis, measured from the surface's own origin, for
    /// the surfaces that have an axis.
    pub along: Option<[f64; 2]>,
    /// Whether the patch's triangles face the way the surface's own outward direction points:
    /// away from a cylinder's axis, a sphere's centre or a torus's tube. True is a shaft or a
    /// boss, false a bore. `None` for a plane, whose `normal` is already turned to face the way
    /// its triangles do, for freeform, and where the mesh does not say.
    pub outward: Option<bool>,
    /// The box the patch's vertices lie in, as `[lowest, highest]` in world axes. How far the
    /// patch reaches along any other direction follows from it only as an upper bound.
    pub bounds: Option<[Vec3; 2]>,
}

/// A direction several patches share: the candidate for an extrusion or a revolve.
#[derive(Debug, Clone, Copy)]
pub struct AxisCluster {
    pub direction: Vec3,
    /// Area of the round patches turning about this direction, and of the planes parallel to it:
    /// an extrusion's walls.
    pub along: f64,
    /// Area of the planes square to it: an extrusion's end caps.
    pub across: f64,
}

/// What the mesh was recognized as.
#[derive(Debug, Clone)]
pub struct Recognition {
    /// Triangles after welding.
    pub triangles: usize,
    /// Vertices after welding.
    pub vertices: usize,
    /// Whether the mesh is closed and consistently oriented (two triangles per edge). An open mesh
    /// is still recognized; only a closed one can become a solid without repair.
    pub closed: bool,
    /// The mesh's total area (mm^2).
    pub area: f64,
    /// The tolerance every patch was certified within (mm).
    pub tolerance: f64,
    /// The patches, largest area first.
    pub patches: Vec<Patch>,
    /// Shared directions, most area first.
    pub axes: Vec<AxisCluster>,
}

impl Recognition {
    /// The area no analytic surface certified over, as a share of the whole.
    pub fn freeform_share(&self) -> f64 {
        if !(self.area > 0.) {
            return 0.;
        }
        let freeform: f64 = self.patches.iter().filter(|p| p.surface == Surface::Freeform).map(|p| p.area).sum();
        freeform / self.area
    }
}

/// Recognize the analytic surfaces in a mesh body, certifying each within `tolerance` mm.
///
/// `groups` optionally proposes the grouping, one number per triangle in the body's own triangle
/// order (a neural segmenter's output, or a caller's own). Triangles of one group that do not
/// touch are still separate patches, and a group whose fit will not certify is split further: the
/// proposal steers the search and never decides the geometry.
pub fn recognize_mesh(body: &Body, tolerance: f64, groups: Option<&[u32]>) -> Result<Recognition> {
    crate::guard("recognize mesh", || {
        if !tolerance.is_finite() || !(1e-6..=1.).contains(&tolerance) {
            return Err(invalid("tolerance must be between 0.000001 and 1 mm"));
        }
        let input = body.mesh.as_ref().ok_or_else(|| invalid("select a mesh body"))?;
        if let Some(groups) = groups
            && groups.len() != input.triangles.len()
        {
            return Err(invalid(&format!(
                "the proposed grouping has {} entries for {} triangles; it needs one per triangle",
                groups.len(),
                input.triangles.len()
            )));
        }
        let welded = crate::mesh_convert::weld(input, tolerance, MAX_TRIANGLES)?;
        let mesh = welded.mesh.as_ref().ok_or_else(|| invalid("empty mesh"))?;
        // Welding drops the triangles of no area and renumbers the rest, so a proposal made for
        // the body's own triangles is re-read in the order welding kept them.
        let kept = kept_groups(input, mesh, groups);
        recognize_welded(mesh, tolerance, kept.as_deref())
    })
}

/// The proposed group of each triangle that survived welding, in the welded mesh's order; `None`
/// when the two cannot be lined up, so a proposal is dropped rather than misapplied.
fn kept_groups(input: &TriMesh, welded: &TriMesh, groups: Option<&[u32]>) -> Option<Vec<u32>> {
    let groups = groups?;
    let mut out = Vec::with_capacity(welded.triangles.len());
    for (i, t) in input.triangles.iter().enumerate() {
        let (Some(a), Some(b), Some(c)) =
            (input.positions.get(t[0] as usize), input.positions.get(t[1] as usize), input.positions.get(t[2] as usize))
        else {
            return None;
        };
        if (*b - *a).cross(*c - *a).normalized().is_none() {
            continue;
        }
        out.push(*groups.get(i)?);
    }
    (out.len() == welded.triangles.len()).then_some(out)
}

/// Triangle neighbours across shared edges, as one flat table.
struct Adjacency {
    /// Where each triangle's neighbours start in `items`, with a final end.
    offsets: Vec<u32>,
    items: Vec<u32>,
}

impl Adjacency {
    fn of(&self, triangle: usize) -> &[u32] {
        let (Some(&from), Some(&to)) = (self.offsets.get(triangle), self.offsets.get(triangle + 1)) else {
            return &[];
        };
        self.items.get(from as usize..to as usize).unwrap_or(&[])
    }
}

/// Neighbours across shared edges, and whether the mesh is closed and consistently oriented.
fn adjacency(mesh: &TriMesh) -> Result<(Adjacency, bool)> {
    let count = mesh.triangles.len();
    let mut edges: Vec<(u64, u32, i8)> = Vec::with_capacity(count * 3);
    for (i, t) in mesh.triangles.iter().enumerate() {
        let i = u32::try_from(i).map_err(|_| invalid("too many triangles"))?;
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (lo, hi) = (a.min(b), a.max(b));
            edges.push(((u64::from(lo) << 32) | u64::from(hi), i, if a < b { 1 } else { -1 }));
        }
    }
    edges.sort_unstable();
    let mut closed = true;
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    let mut run = 0usize;
    while run < edges.len() {
        let mut end = run + 1;
        while edges.get(end).is_some_and(|e| edges.get(run).is_some_and(|r| e.0 == r.0)) {
            end += 1;
        }
        match (edges.get(run), edges.get(run + 1), end - run) {
            (Some(a), Some(b), 2) => {
                if a.2 + b.2 != 0 {
                    closed = false;
                }
                pairs.push((a.1, b.1));
                pairs.push((b.1, a.1));
            }
            _ => closed = false,
        }
        run = end;
    }
    let mut offsets = vec![0u32; count + 1];
    for &(from, _) in &pairs {
        if let Some(slot) = offsets.get_mut(from as usize + 1) {
            *slot += 1;
        }
    }
    for i in 0..count {
        let previous = *offsets.get(i).unwrap_or(&0);
        if let Some(slot) = offsets.get_mut(i + 1) {
            *slot += previous;
        }
    }
    let mut cursor = offsets.clone();
    let mut items = vec![0u32; pairs.len()];
    for &(from, to) in &pairs {
        let Some(at) = cursor.get_mut(from as usize) else { continue };
        if let Some(slot) = items.get_mut(*at as usize) {
            *slot = to;
        }
        *at += 1;
    }
    Ok((Adjacency { offsets, items }, closed))
}

/// Per-call marks on triangles and vertices, so neither has to be cleared between patches.
struct Marks {
    triangle: Vec<u32>,
    vertex: Vec<u32>,
    now: u32,
}

impl Marks {
    fn new(triangles: usize, vertices: usize) -> Marks {
        Marks { triangle: vec![0; triangles], vertex: vec![0; vertices], now: 0 }
    }

    fn next(&mut self) -> u32 {
        self.now += 1;
        self.now
    }
}

fn recognize_welded(mesh: &TriMesh, tolerance: f64, groups: Option<&[u32]>) -> Result<Recognition> {
    let (adjacent, closed) = adjacency(mesh)?;
    let mut normals = Vec::with_capacity(mesh.triangles.len());
    let mut areas = Vec::with_capacity(mesh.triangles.len());
    let mut total = 0.;
    for t in &mesh.triangles {
        let (Some(a), Some(b), Some(c)) = (mesh.positions.get(t[0] as usize), mesh.positions.get(t[1] as usize), mesh.positions.get(t[2] as usize))
        else {
            return Err(invalid("triangle refers to a missing vertex"));
        };
        let cross = (*b - *a).cross(*c - *a);
        // Welding dropped the triangles of no area, so every one here has a normal.
        let Some(n) = cross.normalized() else { return Err(invalid("degenerate triangle")) };
        normals.push(n);
        areas.push(cross.len() * 0.5);
        total += cross.len() * 0.5;
    }
    let bounds = mesh.positions.iter().fold(solidcraft_geom::Aabb3::EMPTY, |mut b, p| {
        b.add(*p);
        b
    });
    let span = bounds.size().len().max(tolerance);
    let facets = Facets { mesh, normals: &normals, areas: &areas };
    let mut marks = Marks::new(mesh.triangles.len(), mesh.positions.len());
    let mut queue: Vec<Bucket> = buckets(mesh, groups)?;
    let mut found: Vec<Found> = Vec::new();
    // A patch that no surface named as it stands, kept whole: the torus pass below needs it whole,
    // because splitting a fillet into bands turns it into a row of little cylinders.
    let mut whole: Vec<(u32, Vec<u32>)> = Vec::new();
    let mut work = 0usize;
    while let Some(Bucket { members, angle, depth, splits, origin }) = queue.pop() {
        work = work.saturating_add(members.len());
        if work > MAX_WORK {
            return Err(invalid("recognition exceeds its work budget"));
        }
        if found.len().saturating_add(queue.len()) > MAX_PATCHES * 4 {
            return Err(invalid(&format!(
                "at a tolerance of {tolerance} mm the mesh breaks into more than {} patches. A noisy or finely faceted mesh needs a \
                 tolerance that covers its own roughness, or no surface certifies anywhere and every facet ends up a patch of its own; \
                 a scan usually wants tenths of a millimetre or more",
                MAX_PATCHES * 4
            )));
        }
        for component in components(&members, &adjacent, &normals, angle, &mut marks) {
            let points = points_of(&component, mesh, &mut marks)?;
            if let Some((surface, deviation)) = fit(&points, &component, &normals, tolerance, span) {
                found.push(Found { patch: patch_of(surface, deviation, &component, &points.vertices, &facets), origin });
                continue;
            }
            let origin = match origin {
                Some(origin) => origin,
                None => {
                    let origin = u32::try_from(whole.len()).map_err(|_| invalid("too many patches"))?;
                    whole.push((origin, component.clone()));
                    origin
                }
            };
            // A round that meets a flat face tangentially -- a fillet, or the corner of a rounded
            // plate -- grows into one patch with that face, because their normals agree exactly
            // where they meet, and no single surface is both. How much of the patch shares each
            // triangle's own direction parts them, so that is tried before narrowing the angle.
            if splits < MAX_SPLITS
                && let Some((flatter, sharper)) = split_by_direction(&component, &normals, &areas)
            {
                queue.push(Bucket { members: flatter, angle, depth, splits: splits + 1, origin: Some(origin) });
                queue.push(Bucket { members: sharper, angle, depth, splits: splits + 1, origin: Some(origin) });
            } else if depth < MAX_REFINE {
                queue.push(Bucket { members: component, angle: angle * 0.5, depth: depth + 1, splits, origin: Some(origin) });
            } else {
                found.push(Found { patch: patch_of(Surface::Freeform, 0., &component, &points.vertices, &facets), origin: Some(origin) });
            }
        }
    }
    // A fillet or a rounded edge is a torus, and a torus cannot be found from its own patch alone:
    // it turns about the axis of whatever it runs around. So each patch that nothing named is
    // tried once more, whole, against the axes the named patches found; where one certifies it
    // replaces the pieces the splitting left behind.
    let candidates = axis_candidates(&found);
    for (origin, component) in whole {
        let points = points_of(&component, mesh, &mut marks)?;
        let Some((surface, deviation)) = candidates
            .iter()
            .filter_map(|&axis| fit_torus(&points.vertices, &points.samples, axis, tolerance, span))
            .min_by(|a, b| a.1.total_cmp(&b.1))
        else {
            continue;
        };
        if deviation > tolerance {
            continue;
        }
        found.retain(|f| f.origin != Some(origin));
        found.push(Found { patch: patch_of(surface, deviation, &component, &points.vertices, &facets), origin: Some(origin) });
    }
    let mut patches: Vec<Patch> = found.into_iter().map(|f| f.patch).collect();
    patches.sort_by(|a, b| b.area.total_cmp(&a.area));
    patches.truncate(MAX_PATCHES);
    let axes = clusters(&patches);
    Ok(Recognition { triangles: mesh.triangles.len(), vertices: mesh.positions.len(), closed, area: total, tolerance, patches, axes })
}

/// A group of triangles waiting to be fitted, and how it got here.
struct Bucket {
    members: Vec<u32>,
    /// The angle neighbouring normals may differ by and still grow one patch.
    angle: f64,
    /// How many times this group has already been narrowed by angle.
    depth: u32,
    /// How many times it has already been split by how sharply it curves.
    splits: u32,
    /// The whole patch this group was split out of, where it was split at all.
    origin: Option<u32>,
}

/// A fitted patch, with the whole patch it came out of.
struct Found {
    patch: Patch,
    origin: Option<u32>,
}

/// The groups of triangles to try together: the proposal's, or the whole mesh as one.
fn buckets(mesh: &TriMesh, groups: Option<&[u32]>) -> Result<Vec<Bucket>> {
    let count = u32::try_from(mesh.triangles.len()).map_err(|_| invalid("too many triangles"))?;
    let whole = |members: Vec<u32>| Bucket { members, angle: GROW_ANGLE, depth: 0, splits: 0, origin: None };
    let Some(groups) = groups else { return Ok(vec![whole((0..count).collect())]) };
    let mut order: Vec<(u32, u32)> = groups.iter().copied().zip(0..count).collect();
    order.sort_unstable();
    let mut out = Vec::new();
    let mut run = 0usize;
    while run < order.len() {
        let mut end = run + 1;
        while order.get(end).is_some_and(|e| order.get(run).is_some_and(|r| e.0 == r.0)) {
            end += 1;
        }
        out.push(whole(order.get(run..end).unwrap_or(&[]).iter().map(|&(_, t)| t).collect()));
        run = end;
    }
    Ok(out)
}

fn patch_of(surface: Surface, deviation: f64, component: &[u32], vertices: &[Vec3], facets: &Facets) -> Patch {
    let area = component.iter().filter_map(|&t| facets.areas.get(t as usize)).sum();
    // Which side of the surface the material is on. A plane is turned to face the way its own
    // triangles do, so a wall's normal always points out of the part; the surfaces that enclose
    // something say it in `outward` instead, because turning their axis over would change what
    // `along` is measured from.
    let facing = facing(surface, component, facets);
    let surface = match (surface, facing) {
        (Surface::Plane { origin, normal }, Some(facing)) if facing < 0. => Surface::Plane { origin, normal: -normal },
        (surface, _) => surface,
    };
    let outward = match surface {
        Surface::Plane { .. } | Surface::Freeform => None,
        _ => facing.map(|f| f > 0.),
    };
    let along = surface.axis().zip(surface.origin()).and_then(|(axis, origin)| {
        let (lo, hi) =
            vertices.iter().map(|p| (*p - origin).dot(axis)).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| (lo.min(t), hi.max(t)));
        (lo.is_finite() && hi.is_finite()).then_some([lo, hi])
    });
    let bounds = vertices.iter().fold(None, |box_: Option<[Vec3; 2]>, p| {
        Some(match box_ {
            None => [*p, *p],
            Some([lo, hi]) => [Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z)), Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z))],
        })
    });
    Patch { surface, triangles: component.len(), area, deviation, along, outward, bounds }
}

/// The triangles of the mesh, with each one's normal and area: what says which side of a fitted
/// surface the material is on.
struct Facets<'a> {
    mesh: &'a TriMesh,
    normals: &'a [Vec3],
    areas: &'a [f64],
}

/// How much of the patch's area faces the way its surface's own outward direction points, less
/// how much faces the other way. `None` where the surface has no outward direction, or where the
/// two sides cancel exactly and the mesh therefore does not say.
fn facing(surface: Surface, component: &[u32], facets: &Facets) -> Option<f64> {
    let mut sum = 0.;
    let mut said = false;
    for &t in component {
        let (Some(tri), Some(&normal), Some(&area)) =
            (facets.mesh.triangles.get(t as usize), facets.normals.get(t as usize), facets.areas.get(t as usize))
        else {
            continue;
        };
        let (Some(a), Some(b), Some(c)) =
            (facets.mesh.positions.get(tri[0] as usize), facets.mesh.positions.get(tri[1] as usize), facets.mesh.positions.get(tri[2] as usize))
        else {
            continue;
        };
        let Some(out) = surface.outward_at((*a + *b + *c) / 3.) else { continue };
        said = true;
        sum += area * normal.dot(out);
    }
    (said && sum != 0.).then_some(sum)
}

/// Part a patch in two by how much of it shares each triangle's own direction: the triangles whose
/// normal a lot of the patch has, and the triangles whose normal is nearly their own. `None` where
/// that is much the same everywhere, which is what one surface looks like, so there is nothing here
/// to part.
///
/// This is what tells a flat face from the round it runs into. Every facet of a flat face has the
/// same normal, so each of them shares its direction with the whole face; a round turns, so each
/// of its facets shares its direction with one band of itself and no more. Growing by the angle
/// between neighbours cannot part the two, because a fillet is tangent to the face it meets and
/// their normals agree exactly where they touch -- and narrowing that angle reaches the mesh's own
/// facet-to-facet step first, leaving a row of slivers instead of a face and a round. Directions
/// are counted in bins, so this costs one pass over the patch and is blind to how finely, or how
/// unevenly, either part of it happens to be triangulated.
fn split_by_direction(component: &[u32], normals: &[Vec3], areas: &[f64]) -> Option<(Vec<u32>, Vec<u32>)> {
    if component.len() < 4 {
        return None;
    }
    let mut shared = vec![0f64; DIRECTIONS * DIRECTIONS * 6];
    for &t in component {
        let (Some(&n), Some(&area)) = (normals.get(t as usize), areas.get(t as usize)) else { continue };
        if let Some(slot) = direction(n).and_then(|at| shared.get_mut(at)) {
            *slot += area;
        }
    }
    let mut peers: Vec<(f64, u32)> = Vec::with_capacity(component.len());
    for &t in component {
        let Some(&n) = normals.get(t as usize) else { continue };
        let Some(&area) = direction(n).and_then(|at| shared.get(at)) else { continue };
        peers.push((area, t));
    }
    if peers.len() < 4 {
        return None;
    }
    peers.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut split = None;
    let mut widest = SHARED_GAP;
    for i in 0..peers.len().saturating_sub(1) {
        let (Some(&(low, _)), Some(&(high, _))) = (peers.get(i), peers.get(i + 1)) else { continue };
        let gap = if low > 0. { high / low } else { f64::INFINITY };
        if gap >= widest {
            (widest, split) = (gap, Some(i + 1));
        }
    }
    let at = split?;
    let (curved, flat) = (peers.get(..at)?, peers.get(at..)?);
    if curved.is_empty() || flat.is_empty() {
        return None;
    }
    let mut curved: Vec<u32> = curved.iter().map(|&(_, t)| t).collect();
    let mut flat: Vec<u32> = flat.iter().map(|&(_, t)| t).collect();
    curved.sort_unstable();
    flat.sort_unstable();
    Some((flat, curved))
}

/// Which bin of directions a unit normal falls in: the face of the cube it points through, and a
/// square of bins across that face.
fn direction(n: Vec3) -> Option<usize> {
    let (face, u, v) = if n.x.abs() >= n.y.abs() && n.x.abs() >= n.z.abs() {
        (usize::from(n.x < 0.), n.y / n.x.abs(), n.z / n.x.abs())
    } else if n.y.abs() >= n.z.abs() {
        (2 + usize::from(n.y < 0.), n.x / n.y.abs(), n.z / n.y.abs())
    } else {
        (4 + usize::from(n.z < 0.), n.x / n.z.abs(), n.y / n.z.abs())
    };
    if !u.is_finite() || !v.is_finite() {
        return None;
    }
    let bin = |t: f64| ((t.clamp(-1., 1.) + 1.) * 0.5 * DIRECTIONS as f64) as usize;
    let (u, v) = (bin(u).min(DIRECTIONS - 1), bin(v).min(DIRECTIONS - 1));
    Some((face * DIRECTIONS + u) * DIRECTIONS + v)
}

/// Split `members` into groups that are joined across shared edges and whose neighbouring normals
/// stay within `angle`.
fn components(members: &[u32], adjacent: &Adjacency, normals: &[Vec3], angle: f64, marks: &mut Marks) -> Vec<Vec<u32>> {
    let member = marks.next();
    for &t in members {
        if let Some(slot) = marks.triangle.get_mut(t as usize) {
            *slot = member;
        }
    }
    let limit = angle.cos();
    let taken = marks.next();
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for &seed in members {
        if marks.triangle.get(seed as usize) != Some(&member) {
            continue;
        }
        if let Some(slot) = marks.triangle.get_mut(seed as usize) {
            *slot = taken;
        }
        stack.push(seed);
        let mut group = Vec::new();
        while let Some(t) = stack.pop() {
            group.push(t);
            let Some(&n) = normals.get(t as usize) else { continue };
            for &next in adjacent.of(t as usize) {
                if marks.triangle.get(next as usize) != Some(&member) {
                    continue;
                }
                if normals.get(next as usize).is_some_and(|m| n.dot(*m) >= limit) {
                    if let Some(slot) = marks.triangle.get_mut(next as usize) {
                        *slot = taken;
                    }
                    stack.push(next);
                }
            }
        }
        group.sort_unstable();
        out.push(group);
    }
    out
}

/// The points a patch is fitted from and measured over.
struct Points {
    /// The patch distinct vertices, which lie on the surface the mesh came from.
    vertices: Vec<Vec3>,
    /// The vertices and the middle of each triangle: what a fit is measured over. A surface that
    /// runs through every vertex of a coarse patch and misses the middle of its facets is not the
    /// surface that patch is.
    samples: Vec<Vec3>,
}

fn points_of(component: &[u32], mesh: &TriMesh, marks: &mut Marks) -> Result<Points> {
    let seen = marks.next();
    let mut vertices = Vec::new();
    let mut middles = Vec::with_capacity(component.len());
    for &t in component {
        let Some(t) = mesh.triangles.get(t as usize) else { continue };
        let mut corners = Vec::with_capacity(3);
        for &i in t {
            let p = *mesh.positions.get(i as usize).ok_or_else(|| invalid("triangle refers to a missing vertex"))?;
            corners.push(p);
            if marks.vertex.get(i as usize) == Some(&seen) {
                continue;
            }
            if let Some(slot) = marks.vertex.get_mut(i as usize) {
                *slot = seen;
            }
            vertices.push(p);
        }
        if let [a, b, c] = corners.as_slice() {
            middles.push((*a + *b + *c) / 3.);
        }
    }
    let mut samples = vertices.clone();
    samples.append(&mut middles);
    Ok(Points { vertices, samples })
}

/// The surface a patch certifies as, and how far the mesh strays from it. The simplest surface
/// that certifies wins, unless a more complex one more than halves the deviation.
fn fit(patch: &Points, component: &[u32], normals: &[Vec3], tolerance: f64, span: f64) -> Option<(Surface, f64)> {
    let Points { vertices, samples } = patch;
    if vertices.len() < 3 {
        return None;
    }
    let facing: Vec<Vec3> = component.iter().filter_map(|&t| normals.get(t as usize).copied()).collect();
    let mut best: Option<(Surface, f64)> = None;
    let tried = [
        fit_plane(vertices, samples, &facing),
        fit_cylinder(vertices, samples, &facing, span),
        fit_sphere(vertices, samples, span),
        fit_cone(vertices, samples, &facing, span),
    ];
    for candidate in tried.into_iter().flatten() {
        if candidate.1 > tolerance {
            continue;
        }
        best = Some(match best {
            None => candidate,
            Some(chosen) => pick(chosen, candidate),
        });
    }
    best
}

/// Of two fits that both certify, the one to report.
fn pick(chosen: (Surface, f64), other: (Surface, f64)) -> (Surface, f64) {
    let (simpler, complex) = if chosen.0.parameters() <= other.0.parameters() { (chosen, other) } else { (other, chosen) };
    if complex.1 * 2. < simpler.1 { complex } else { simpler }
}

/// Axis lines a torus could turn about: the axes of the round patches already named, most area
/// first. A fillet runs around something, and that something is in this list.
const MAX_AXIS_CANDIDATES: usize = 32;

fn axis_candidates(found: &[Found]) -> Vec<(Vec3, Vec3)> {
    let mut axes: Vec<(f64, Vec3, Vec3)> = found
        .iter()
        .filter_map(|f| match f.patch.surface {
            Surface::Cylinder { origin, axis, .. } => Some((f.patch.area, origin, axis)),
            Surface::Cone { apex, axis, .. } => Some((f.patch.area, apex, axis)),
            Surface::Torus { centre, axis, .. } => Some((f.patch.area, centre, axis)),
            Surface::Plane { .. } | Surface::Sphere { .. } | Surface::Freeform => None,
        })
        .collect();
    axes.sort_by(|a, b| b.0.total_cmp(&a.0));
    axes.truncate(MAX_AXIS_CANDIDATES);
    axes.into_iter().map(|(_, point, axis)| (point, axis)).collect()
}

/// How nearly parallel two directions count as parallel, and how nearly square as square: one
/// degree, which a tessellation's own error stays well inside.
const PARALLEL: f64 = 0.999_847;
const SQUARE: f64 = 0.017_453;
/// Directions reported.
const MAX_AXES: usize = 8;

/// The directions the patches share, most area first: where an extrusion or a revolve would be.
fn clusters(patches: &[Patch]) -> Vec<AxisCluster> {
    let mut out: Vec<AxisCluster> = Vec::new();
    for patch in patches {
        if out.len() >= MAX_AXES {
            break;
        }
        let Some(direction) = patch.surface.axis().and_then(canonical) else { continue };
        if out.iter().any(|c| c.direction.dot(direction).abs() >= PARALLEL) {
            continue;
        }
        out.push(AxisCluster { direction, along: 0., across: 0. });
    }
    for cluster in &mut out {
        for patch in patches {
            let Some(axis) = patch.surface.axis() else { continue };
            let aligned = axis.dot(cluster.direction).abs();
            match patch.surface {
                // A plane square to the direction is an end cap; one parallel to it is a wall.
                Surface::Plane { .. } if aligned >= PARALLEL => cluster.across += patch.area,
                Surface::Plane { .. } if aligned <= SQUARE => cluster.along += patch.area,
                Surface::Plane { .. } => {}
                _ if aligned >= PARALLEL => cluster.along += patch.area,
                _ => {}
            }
        }
    }
    out.sort_by(|a, b| (b.along + b.across).total_cmp(&(a.along + a.across)));
    out
}

/// A direction with its sign settled, so two opposite axes cluster as one.
fn canonical(v: Vec3) -> Option<Vec3> {
    let v = v.normalized()?;
    let flip = if v.x.abs() > 1e-9 {
        v.x < 0.
    } else if v.y.abs() > 1e-9 {
        v.y < 0.
    } else {
        v.z < 0.
    };
    Some(if flip { -v } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_body;
    use std::f64::consts::TAU;

    /// A closed box from the origin to `(x, y, z)`, wound outwards.
    fn box_mesh(x: f64, y: f64, z: f64) -> Body {
        let positions = [
            Vec3::new(0., 0., 0.),
            Vec3::new(x, 0., 0.),
            Vec3::new(x, y, 0.),
            Vec3::new(0., y, 0.),
            Vec3::new(0., 0., z),
            Vec3::new(x, 0., z),
            Vec3::new(x, y, z),
            Vec3::new(0., y, z),
        ];
        let triangles =
            [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
        mesh_body(&positions, &triangles).expect("box mesh")
    }

    /// A closed cylinder of `radius` and `height` about Z, in `sides` facets.
    fn cylinder_mesh(radius: f64, height: f64, sides: usize) -> Body {
        let mut positions = Vec::new();
        for i in 0..sides {
            let a = TAU * i as f64 / sides as f64;
            positions.push(Vec3::new(radius * a.cos(), radius * a.sin(), 0.));
            positions.push(Vec3::new(radius * a.cos(), radius * a.sin(), height));
        }
        let bottom = positions.len() as u32;
        positions.push(Vec3::new(0., 0., 0.));
        let top = positions.len() as u32;
        positions.push(Vec3::new(0., 0., height));
        let mut triangles = Vec::new();
        for i in 0..sides {
            let (b0, t0) = (2 * i as u32, 2 * i as u32 + 1);
            let next = ((i + 1) % sides) as u32;
            let (b1, t1) = (2 * next, 2 * next + 1);
            triangles.push([b0, b1, t1]);
            triangles.push([b0, t1, t0]);
            triangles.push([bottom, b1, b0]);
            triangles.push([top, t0, t1]);
        }
        mesh_body(&positions, &triangles).expect("cylinder mesh")
    }

    /// A closed cone of base `radius` and `height` about Z, apex up.
    fn cone_mesh(radius: f64, height: f64, sides: usize) -> Body {
        let mut positions: Vec<Vec3> = (0..sides)
            .map(|i| {
                let a = TAU * i as f64 / sides as f64;
                Vec3::new(radius * a.cos(), radius * a.sin(), 0.)
            })
            .collect();
        let apex = positions.len() as u32;
        positions.push(Vec3::new(0., 0., height));
        let centre = positions.len() as u32;
        positions.push(Vec3::new(0., 0., 0.));
        let mut triangles = Vec::new();
        for i in 0..sides {
            let (a, b) = (i as u32, ((i + 1) % sides) as u32);
            triangles.push([a, b, apex]);
            triangles.push([centre, b, a]);
        }
        mesh_body(&positions, &triangles).expect("cone mesh")
    }

    /// A sphere of `radius` about the origin, in `rings` by `sides` quads.
    fn sphere_mesh(radius: f64, rings: usize, sides: usize) -> Body {
        let mut positions = Vec::new();
        for r in 0..=rings {
            let phi = std::f64::consts::PI * r as f64 / rings as f64;
            for s in 0..sides {
                let theta = TAU * s as f64 / sides as f64;
                positions.push(Vec3::new(radius * phi.sin() * theta.cos(), radius * phi.sin() * theta.sin(), radius * phi.cos()));
            }
        }
        let mut triangles = Vec::new();
        for r in 0..rings {
            for s in 0..sides {
                let next = (s + 1) % sides;
                let (a, b) = ((r * sides + s) as u32, (r * sides + next) as u32);
                let (c, d) = (((r + 1) * sides + next) as u32, ((r + 1) * sides + s) as u32);
                triangles.push([a, c, b]);
                triangles.push([a, d, c]);
            }
        }
        mesh_body(&positions, &triangles).expect("sphere mesh")
    }

    /// A torus about Z: the tube of radius `minor` swept around a circle of radius `major`.
    fn torus_mesh(major: f64, minor: f64, around: usize, across: usize) -> (Vec<Vec3>, Vec<[u32; 3]>) {
        let mut positions = Vec::new();
        for i in 0..around {
            let u = TAU * i as f64 / around as f64;
            for j in 0..across {
                let v = TAU * j as f64 / across as f64;
                let r = major + minor * v.cos();
                positions.push(Vec3::new(r * u.cos(), r * u.sin(), minor * v.sin()));
            }
        }
        let mut triangles = Vec::new();
        for i in 0..around {
            for j in 0..across {
                let (i1, j1) = ((i + 1) % around, (j + 1) % across);
                let (a, b) = ((i * across + j) as u32, (i * across + j1) as u32);
                let (c, d) = ((i1 * across + j1) as u32, (i1 * across + j) as u32);
                triangles.push([a, b, c]);
                triangles.push([a, c, d]);
            }
        }
        (positions, triangles)
    }

    fn surfaces(r: &Recognition) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = r.patches.iter().map(|p| p.surface.name()).collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn a_box_is_six_planes_square_to_three_axes() {
        let r = recognize_mesh(&box_mesh(10., 20., 30.), 0.01, None).expect("recognize");
        assert!(r.closed, "a box mesh is closed");
        assert_eq!(surfaces(&r), ["plane"; 6], "six faces, each flat");
        assert!(r.patches.iter().all(|p| p.deviation < 1e-9), "a flat face strays from its plane by nothing: {:?}", r.patches);
        assert!((r.area - 2. * (10. * 20. + 10. * 30. + 20. * 30.)).abs() < 1e-9, "area {}", r.area);
        assert_eq!(r.freeform_share(), 0.);
        // Three directions, each with two caps and four walls.
        assert_eq!(r.axes.len(), 3, "{:?}", r.axes);
        for axis in &r.axes {
            assert!(axis.across > 0. && axis.along > 0., "{axis:?}");
            assert!((axis.along + axis.across - r.area).abs() < 1e-9, "every face is a wall or a cap: {axis:?}");
        }
    }

    #[test]
    fn a_cylinder_is_one_cylinder_between_two_discs() {
        let r = recognize_mesh(&cylinder_mesh(5., 12., 48), 0.05, None).expect("recognize");
        assert_eq!(surfaces(&r), ["cylinder", "plane", "plane"], "{:?}", r.patches);
        let round = r.patches.iter().find(|p| p.surface.name() == "cylinder").expect("a cylinder");
        let Surface::Cylinder { radius, axis, .. } = round.surface else { panic!("{:?}", round.surface) };
        // The facets sit inside the true radius by the sagitta, so the fitted radius is a little
        // under 5; it is the mesh that is faceted, not the fit that is wrong.
        assert!((radius - 5.).abs() < 0.02, "radius {radius}");
        assert!(axis.dot(Vec3::new(0., 0., 1.)).abs() > PARALLEL, "axis {axis:?}");
        assert_eq!(round.along.map(|a| ((a[1] - a[0]) * 1e6).round() / 1e6), Some(12.), "the patch spans the height");
    }

    #[test]
    fn a_cone_is_a_cone_on_a_disc() {
        let r = recognize_mesh(&cone_mesh(6., 8., 64), 0.05, None).expect("recognize");
        assert_eq!(surfaces(&r), ["cone", "plane"], "{:?}", r.patches);
        let side = r.patches.iter().find(|p| p.surface.name() == "cone").expect("a cone");
        let Surface::Cone { half_angle, apex, axis } = side.surface else { panic!("{:?}", side.surface) };
        // tan(half angle) = radius / height.
        assert!((half_angle.tan() - 6. / 8.).abs() < 0.02, "half angle {half_angle}");
        // The axis points the way the radius widens: down, from the apex at the top.
        assert!(axis.dot(Vec3::new(0., 0., -1.)) > PARALLEL, "axis {axis:?}");
        assert!((apex - Vec3::new(0., 0., 8.)).len() < 0.05, "apex {apex:?}");
    }

    #[test]
    fn a_sphere_is_a_sphere() {
        let r = recognize_mesh(&sphere_mesh(7., 24, 48), 0.05, None).expect("recognize");
        assert_eq!(surfaces(&r), ["sphere"], "{:?}", r.patches);
        let Some(Surface::Sphere { centre, radius }) = r.patches.first().map(|p| p.surface) else { panic!("{:?}", r.patches) };
        assert!((radius - 7.).abs() < 0.05, "radius {radius}");
        assert!(centre.len() < 0.05, "centre {centre:?}");
    }

    #[test]
    fn a_fillet_is_a_torus_about_the_axis_it_runs_around() {
        // A ring around a cylinder on the same axis: the cylinder gives the axis, which is the
        // only way a torus can be found at all.
        let (mut positions, mut triangles) = torus_mesh(10., 1.5, 48, 24);
        let cylinder = cylinder_mesh(10., 6., 48);
        let mesh = cylinder.mesh.as_ref().expect("a mesh body");
        let base = positions.len() as u32;
        positions.extend(mesh.positions.iter().map(|p| *p + Vec3::new(0., 0., 4.)));
        triangles.extend(mesh.triangles.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
        let body = mesh_body(&positions, &triangles).expect("ring and cylinder");
        let r = recognize_mesh(&body, 0.05, None).expect("recognize");
        let torus = r.patches.iter().find(|p| p.surface.name() == "torus").unwrap_or_else(|| panic!("a torus among {:?}", surfaces(&r)));
        let Surface::Torus { major, minor, axis, .. } = torus.surface else { panic!("{:?}", torus.surface) };
        assert!((major - 10.).abs() < 0.05, "major {major}");
        assert!((minor - 1.5).abs() < 0.05, "minor {minor}");
        assert!(axis.dot(Vec3::new(0., 0., 1.)).abs() > PARALLEL, "axis {axis:?}");
    }

    #[test]
    fn a_proposal_steers_the_search_and_never_decides_the_geometry() {
        let body = box_mesh(10., 20., 30.);
        let triangles = body.mesh.as_ref().expect("a mesh body").triangles.len();
        // One group for everything: growing still stops at the box's creases.
        let lumped = recognize_mesh(&body, 0.01, Some(&vec![7; triangles])).expect("one group");
        assert_eq!(surfaces(&lumped), ["plane"; 6]);
        // A group per triangle: twelve patches now, every one of them still certified flat.
        let split: Vec<u32> = (0..triangles as u32).collect();
        let apart = recognize_mesh(&body, 0.01, Some(&split)).expect("a group each");
        assert_eq!(apart.patches.len(), 12, "{:?}", surfaces(&apart));
        assert!(apart.patches.iter().all(|p| p.surface.name() == "plane" && p.deviation < 1e-9));
        // A proposal that cuts across the faces cannot invent a surface either.
        let crossed: Vec<u32> = (0..triangles as u32).map(|i| i % 3).collect();
        let crossed = recognize_mesh(&body, 0.01, Some(&crossed)).expect("across the faces");
        assert!(crossed.patches.iter().all(|p| p.surface.name() == "plane"), "{:?}", surfaces(&crossed));
        assert!((crossed.area - lumped.area).abs() < 1e-9, "the area is the mesh's, whatever the grouping");
    }

    #[test]
    fn a_grouping_of_the_wrong_length_is_refused_and_hostile_input_never_panics() {
        let body = box_mesh(1., 1., 1.);
        let wrong = recognize_mesh(&body, 0.01, Some(&[1, 2, 3])).expect_err("the wrong length");
        assert!(format!("{wrong}").contains("one per triangle"), "{wrong}");
        for tolerance in [0., -1., 2., f64::NAN, f64::INFINITY] {
            assert!(recognize_mesh(&body, tolerance, None).is_err(), "tolerance {tolerance} is not usable");
        }
        // Not a mesh body at all.
        let solid = crate::box_solid(Vec3::ZERO, Vec3::new(10., 10., 10.)).expect("a block");
        assert!(recognize_mesh(&solid, 0.01, None).is_err(), "a solid is not a mesh body");
    }

    #[test]
    fn a_bore_faces_inward_a_shaft_faces_outward_and_every_plane_faces_out_of_the_part() {
        let plate = crate::box_solid(Vec3::ZERO, Vec3::new(40., 30., 10.)).unwrap();
        let drill = crate::cylinder(Vec3::new(10., 15., -1.), Vec3::Z, 4., 12.).unwrap();
        let post = crate::cylinder(Vec3::new(30., 15., 10.), Vec3::Z, 6., 8.).unwrap();
        let bored = crate::boolean(&plate, &drill, crate::BoolOp::Cut).unwrap().unwrap();
        let part = crate::boolean(&bored, &post, crate::BoolOp::Union).unwrap().unwrap();
        let mesh = part.mesh(&crate::MeshTolerance::chord(0.005)).unwrap();
        let r = recognize_mesh(&mesh_body(&mesh.positions, &mesh.triangles).unwrap(), 0.05, None).unwrap();
        let cylinder = |radius: f64| {
            r.patches
                .iter()
                .find(|p| matches!(p.surface, Surface::Cylinder { radius: had, .. } if (had - radius).abs() < 0.01))
                .unwrap_or_else(|| panic!("no cylinder of radius {radius} among {:?}", surfaces(&r)))
        };
        // The hole's wall faces its own axis and the post's faces away from it, which is the whole
        // difference between a hole and a boss once the triangles are all that is left.
        assert_eq!(cylinder(4.).outward, Some(false));
        assert_eq!(cylinder(6.).outward, Some(true));
        // A plane's normal is turned to face the way its triangles do, so it points out of the
        // part: the bottom face of a part standing on the origin faces down.
        let bottom = r
            .patches
            .iter()
            .filter_map(|p| match p.surface {
                Surface::Plane { origin, normal } if origin.z.abs() < 0.01 => Some(normal),
                _ => None,
            })
            .next()
            .expect("a face on the origin plane");
        assert!(bottom.z < -0.99, "{bottom:?}");
        // Every patch knows the box it lies in, which is how a rebuild tells a wall that runs the
        // whole thickness from the side of a pocket.
        for patch in &r.patches {
            let [lo, hi] = patch.bounds.expect("every patch has bounds");
            assert!(lo.x <= hi.x && lo.y <= hi.y && lo.z <= hi.z, "{lo:?} {hi:?}");
            assert!(lo.z >= -0.01 && hi.z <= 18.01, "{lo:?} {hi:?}");
        }
    }

    #[test]
    fn a_round_tangent_to_the_flat_it_meets_is_still_found_as_a_round() {
        let plate = crate::box_solid(Vec3::ZERO, Vec3::new(40., 30., 10.)).unwrap();
        let edges = [Vec3::new(0., 0., 5.), Vec3::new(40., 0., 5.), Vec3::new(40., 30., 5.), Vec3::new(0., 30., 5.)];
        let rounded = crate::fillet(&plate, &edges, 5.).unwrap();
        let mesh = rounded.mesh(&crate::MeshTolerance::chord(0.005)).unwrap();
        let r = recognize_mesh(&mesh_body(&mesh.positions, &mesh.triangles).unwrap(), 0.05, None).unwrap();
        // A fillet's normals agree with the wall's exactly where they meet, so growing by angle
        // alone takes the lot as one patch and then leaves a row of slivers. Four corner rounds
        // and six flats is what the part is.
        let names = surfaces(&r);
        assert_eq!(names.iter().filter(|n| **n == "cylinder").count(), 4, "{names:?}");
        assert_eq!(names.iter().filter(|n| **n == "plane").count(), 6, "{names:?}");
        assert!(r.patches.iter().all(|p| p.deviation <= r.tolerance), "{:?}", r.patches.iter().map(|p| p.deviation).collect::<Vec<_>>());
    }
}
