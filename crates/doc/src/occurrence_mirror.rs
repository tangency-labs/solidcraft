//! Mirrored occurrences: a placement kept at the mirror image of another one across a plane.
//!
//! A placement is a rigid move and a reflection is not one, so an occurrence cannot hold the
//! reflection itself. It holds the reflection across the mirror plane followed by a reflection
//! of the part in a plane of its own frame (`local`): two reflections make a rigid move. When
//! the part is symmetric about that plane of its own, the occurrence shows exactly the mirror
//! image of the source. An opposite-hand part is a component whose bodies are the source's
//! reflected in that same plane of its own: placed this way it is the mirror image of any part.
//!
//! The relation is kept in the design ([`crate::joints::Assembly::mirrors`]) and the engine
//! places the mirrored occurrence again whenever the design changes, so it follows its source
//! and the plane.

use serde::{Deserialize, Serialize};
use solidcore_record::{Plane, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{Body, PointClass};

use crate::{DocError, Document, Mat, ModelState, PlaneRef, Result};

/// The most mirror relations a design keeps.
pub const MAX_MIRRORS: usize = 10_000;

/// An occurrence placed as the mirror image of another one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OccurrenceMirror {
    /// The occurrence this relation places.
    pub occurrence: u64,
    /// The occurrence it mirrors.
    pub source: u64,
    /// The mirror plane, in the frame of component `frame`.
    pub plane: PlaneRef,
    /// The component whose frame `plane` is in (0: the design's own).
    #[serde(default)]
    pub frame: u64,
    /// The reflection in the part's own frame that follows the mirror (a reflection in one of
    /// its mid-planes), so that the placement is rigid.
    pub local: Mat,
}

/// The reflection across `pl`, as a column-major matrix.
pub fn reflection(pl: &Plane) -> Mat {
    let n = pl.normal();
    let d = pl.origin.dot(n);
    let c = [n.x, n.y, n.z];
    let mut m = crate::IDENTITY;
    for (j, col) in m.iter_mut().enumerate().take(3) {
        for (i, cell) in col.iter_mut().enumerate().take(3) {
            *cell = if i == j { 1.0 } else { 0.0 } - 2.0 * c[i] * c[j];
        }
    }
    m[3] = [2.0 * d * n.x, 2.0 * d * n.y, 2.0 * d * n.z, 1.0];
    m
}

/// Whether `m` is a reflection in a plane: orthonormal, turned inside out, finite and in range.
pub fn is_reflection(m: &Mat) -> bool {
    let axis = |i: usize| Vec3::new(m[i][0], m[i][1], m[i][2]);
    let (x, y, z) = (axis(0), axis(1), axis(2));
    m.iter().flatten().all(|v| v.is_finite())
        && m[3][..3].iter().all(|v| v.abs() <= 1e9)
        && [x, y, z].iter().all(|v| (v.dot(*v) - 1.0).abs() < 1e-6)
        && x.dot(y).abs() < 1e-6
        && x.dot(z).abs() < 1e-6
        && y.dot(z).abs() < 1e-6
        && x.cross(y).dot(z) < 0.0
        && [m[0][3], m[1][3], m[2][3]].iter().all(|v| v.abs() < 1e-9)
        && (m[3][3] - 1.0).abs() < 1e-9
}

/// The mirror plane of `m` in world coordinates.
pub fn world_plane(doc: &Document, st: Option<&ModelState>, plane: &PlaneRef, frame: u64) -> Result<Plane> {
    let (vals, _) = doc.param_values();
    let pl = doc.resolve_plane_in(&vals, plane, 0, frame, st)?;
    let f = doc.component_transform(frame);
    Plane::new(crate::apply_point(&f, pl.origin), crate::apply_vector(&f, pl.x), crate::apply_vector(&f, pl.y))
        .ok_or_else(|| DocError::Invalid("the mirror plane is degenerate".into()))
}

/// Where relation `m` places its occurrence, within that occurrence's parent: the source's
/// world placement reflected across the plane, then reflected in the part's own plane.
pub fn mirrored_transform(doc: &Document, st: Option<&ModelState>, m: &OccurrenceMirror) -> Result<Mat> {
    let lost = |id: u64| DocError::Invalid(format!("occurrence {id} of a mirror no longer exists"));
    let target = doc.occurrences.iter().find(|o| o.id == m.occurrence).ok_or_else(|| lost(m.occurrence))?;
    if !doc.occurrences.iter().any(|o| o.id == m.source) {
        return Err(lost(m.source));
    }
    if !is_reflection(&m.local) {
        return Err(DocError::Invalid("a mirror's own reflection is not a reflection".into()));
    }
    let s = reflection(&world_plane(doc, st, &m.plane, m.frame)?);
    let source = crate::joints::occurrence_world(doc, m.source);
    let world = crate::mat_mul(&crate::mat_mul(&s, &source), &m.local);
    let parent = doc.component_transform(target.parent);
    let inverse = crate::mat_inverse(&parent).ok_or_else(|| DocError::Invalid("the mirrored occurrence's parent is singular".into()))?;
    let local = crate::mat_mul(&inverse, &world);
    if !crate::valid_transfer_matrix(&local) {
        return Err(DocError::Invalid("the mirrored placement is not a rigid move".into()));
    }
    Ok(local)
}

/// The occurrences whose placement the design's mirror relations change, with where they go,
/// and a message for each relation that cannot be followed now. A relation whose occurrence or
/// source is gone is listed in the third value, to be dropped.
#[allow(clippy::type_complexity)]
pub fn follow(doc: &Document, st: Option<&ModelState>) -> (Vec<(u64, Mat)>, Vec<String>, Vec<usize>) {
    let (mut moves, mut notes, mut gone) = (Vec::new(), Vec::new(), Vec::new());
    let exists = |id: u64| doc.occurrences.iter().any(|o| o.id == id);
    for (k, m) in doc.assembly.mirrors.iter().enumerate().take(MAX_MIRRORS) {
        if !exists(m.occurrence) || !exists(m.source) || m.occurrence == m.source {
            gone.push(k);
            continue;
        }
        match mirrored_transform(doc, st, m) {
            Ok(t) => {
                let now = doc.occurrences.iter().find(|o| o.id == m.occurrence).map(|o| o.transform);
                let same = now.is_some_and(|n| n.iter().flatten().zip(t.iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-12));
                if !same {
                    moves.push((m.occurrence, t));
                }
            }
            Err(e) => notes.push(e.to_string()),
        }
    }
    (moves, notes, gone)
}

/// What a component shows, decided on the exact B-rep: its bodies and, through its occurrences,
/// those of the components inside it, each with where it stands in the component's own frame.
#[derive(Default)]
pub struct ComponentShape<'a> {
    parts: Vec<ShapePart<'a>>,
}

struct ShapePart<'a> {
    body: &'a Body,
    /// The body's frame to the component's.
    at: Mat,
    /// The component's frame to the body's.
    back: Mat,
}

/// The most bodies a component's shape gathers.
const MAX_SHAPE_BODIES: usize = 10_000;
/// The most points [`symmetric_about`] reflects (vertices and points along edges).
const MAX_SAMPLES: usize = 256;

impl<'a> ComponentShape<'a> {
    /// The shape of `parts`: bodies, each with its frame to the component's. A body placed by a
    /// singular matrix is left out.
    pub fn new(parts: impl IntoIterator<Item = (&'a Body, Mat)>) -> Self {
        let parts = parts
            .into_iter()
            .take(MAX_SHAPE_BODIES)
            .filter_map(|(body, at)| crate::mat_inverse(&at).map(|back| ShapePart { body, at, back }))
            .collect();
        Self { parts }
    }

    /// Whether the component shows nothing.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// The box of the shape in the component's frame: each body's exact box, placed.
    fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut out: Option<(Vec3, Vec3)> = None;
        for part in &self.parts {
            let b = part.body.exact_bounds().unwrap_or_else(|_| part.body.outline_bounds());
            if !(b.min.is_finite() && b.max.is_finite()) || b.min.x > b.max.x || b.min.y > b.max.y || b.min.z > b.max.z {
                continue;
            }
            for k in 0..8 {
                let corner = Vec3::new(
                    if k & 1 == 0 { b.min.x } else { b.max.x },
                    if k & 2 == 0 { b.min.y } else { b.max.y },
                    if k & 4 == 0 { b.min.z } else { b.max.z },
                );
                let p = crate::apply_point(&part.at, corner);
                out = Some(out.map_or((p, p), |(lo, hi)| (lo.min(p), hi.max(p))));
            }
        }
        out
    }

    /// Where `p` (in the component's frame) lies against the shape: inside when inside a body,
    /// else on the boundary when on one, else outside. `None` when a body cannot tell.
    fn class(&self, p: Vec3) -> Option<PointClass> {
        let mut on = false;
        for part in &self.parts {
            match part.body.classify_point(crate::apply_point(&part.back, p)).ok()? {
                PointClass::Inside => return Some(PointClass::Inside),
                PointClass::OnBoundary => on = true,
                PointClass::Outside => {}
            }
        }
        Some(if on { PointClass::OnBoundary } else { PointClass::Outside })
    }

    /// Whether `p` lies within `tol` of the shape's boundary: on it, or with material on one
    /// side of it and none on another within `tol`.
    fn near_boundary(&self, p: Vec3, tol: f64) -> bool {
        let (mut inside, mut outside) = (false, false);
        let probes = [Vec3::ZERO, Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z];
        for d in probes {
            match self.class(p + d * tol) {
                Some(PointClass::OnBoundary) => return true,
                Some(PointClass::Inside) => inside = true,
                Some(PointClass::Outside) => outside = true,
                None => return false,
            }
            if inside && outside {
                return true;
            }
        }
        false
    }

    /// Points on the shape's boundary, in the component's frame: its bodies' vertices and points
    /// along their edges' curves (a sample of them on a big shape).
    fn samples(&self) -> Vec<Vec3> {
        let mut out = Vec::new();
        for part in &self.parts {
            let record = part.body.record();
            out.extend(record.vertices.iter().map(|v| {
                let [x, y, z] = v.point;
                crate::apply_point(&part.at, Vec3::new(x, y, z))
            }));
            for e in &record.edges {
                let Ok(curve) = solidcraft_geom::brep::eval::CurveEval::new(e.parameters()) else { continue };
                let [t0, t1] = e.interval;
                for f in [0.25, 0.5, 0.75] {
                    if let Some(p) = curve.point(t0 + (t1 - t0) * f) {
                        out.push(crate::apply_point(&part.at, p));
                    }
                }
            }
        }
        let step = (out.len() / MAX_SAMPLES).max(1);
        out.into_iter().step_by(step).filter(|p| p.is_finite()).collect()
    }
}

/// The shape a component shows (its bodies and, through its occurrences, those of the
/// components inside it), in its own frame.
pub fn component_shape<'a>(doc: &Document, st: &'a ModelState, component: u64) -> ComponentShape<'a> {
    let mut parts = Vec::new();
    let owners = doc.model_body_components(st);
    let mut stack = vec![(component, crate::IDENTITY, 0usize)];
    while let Some((c, at, depth)) = stack.pop() {
        if depth > 64 || parts.len() >= MAX_SHAPE_BODIES {
            break;
        }
        for (b, owner) in st.bodies.iter().zip(&owners) {
            if *owner != c {
                continue;
            }
            let m = doc.model_body_offset(b).map_or(at, |offset| crate::mat_mul(&at, offset));
            parts.push((&b.body, m));
        }
        for o in doc.occurrences.iter().filter(|o| o.parent == c && c != 0).take(10_000) {
            stack.push((o.component, crate::mat_mul(&at, &o.transform), depth + 1));
        }
    }
    ComponentShape::new(parts)
}

/// The plane of the part's own frame through the middle of its box, square to `axis` (0, 1,
/// 2: X, Y, Z), or `None` for a part with nothing to show.
pub fn mid_plane(shape: &ComponentShape, axis: usize) -> Option<Plane> {
    let b = shape.bounds()?;
    let c = (b.0 + b.1) * 0.5;
    let n = [Vec3::X, Vec3::Y, Vec3::Z].get(axis).copied()?;
    Plane::from_normal(c, n)
}

/// Whether the part `shape` shows is its own mirror image in `pl`, decided on its exact faces
/// and edges: every reflected vertex and edge point (a sample of them on a big part) lies on its
/// boundary, and a grid of points through its box is in its material exactly where the
/// reflected points are.
pub fn symmetric_about(shape: &ComponentShape, pl: &Plane) -> bool {
    let Some((lo, hi)) = shape.bounds() else { return false };
    let size = (hi - lo).len();
    if !(size.is_finite() && size > 0.0) {
        return false;
    }
    let tol = (size * 1e-5).max(1e-6);
    let m = reflection(pl);
    let on_boundary = shape.samples().into_iter().all(|p| {
        let q = crate::apply_point(&m, p);
        shape.class(q) == Some(PointClass::OnBoundary) || shape.near_boundary(q, tol)
    });
    if !on_boundary {
        return false;
    }
    const STEPS: usize = 4;
    let at = |k: usize, lo: f64, hi: f64| lo + (hi - lo) * (k as f64 + 0.5) / STEPS as f64;
    (0..STEPS * STEPS * STEPS).all(|k| {
        let p = Vec3::new(at(k % STEPS, lo.x, hi.x), at(k / STEPS % STEPS, lo.y, hi.y), at(k / (STEPS * STEPS), lo.z, hi.z));
        match (shape.class(p), shape.class(crate::apply_point(&m, p))) {
            (Some(PointClass::OnBoundary), Some(_)) | (Some(_), Some(PointClass::OnBoundary)) => true,
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(lo: Vec3, hi: Vec3) -> Body {
        solidcraft_kernel::box_solid(lo, hi).unwrap()
    }

    fn shape(bodies: &[Body]) -> ComponentShape<'_> {
        ComponentShape::new(bodies.iter().map(|b| (b, crate::IDENTITY)))
    }

    #[test]
    fn a_reflection_in_a_plane_is_its_own_inverse_and_turns_inside_out() {
        let pl = Plane::from_normal(Vec3::new(5.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0)).unwrap();
        let m = reflection(&pl);
        assert!(is_reflection(&m));
        assert!(!is_reflection(&crate::IDENTITY));
        let twice = crate::mat_mul(&m, &m);
        assert!(crate::is_identity(&twice), "{twice:?}");
        // A point on the plane stays, a point off it goes to the other side.
        let on = pl.origin + pl.x * 3.0;
        assert!((crate::apply_point(&m, on) - on).len() < 1e-12);
        let off = pl.origin + pl.normal() * 2.0;
        assert!((crate::apply_point(&m, off) - (pl.origin - pl.normal() * 2.0)).len() < 1e-12);
    }

    #[test]
    fn a_block_is_symmetric_about_its_mid_planes_and_an_l_is_not() {
        let plate = [block(Vec3::new(0.0, 0.0, 0.0), Vec3::new(40.0, 30.0, 5.0))];
        let plate = shape(&plate);
        for axis in 0..3 {
            assert!(symmetric_about(&plate, &mid_plane(&plate, axis).unwrap()), "axis {axis}");
        }
        // Not about a plane off its middle.
        assert!(!symmetric_about(&plate, &Plane::from_normal(Vec3::new(10.0, 0.0, 0.0), Vec3::X).unwrap()));
        // An L: a base and an upright at one end. Symmetric across Y only.
        let l = [block(Vec3::new(0.0, 0.0, 0.0), Vec3::new(40.0, 30.0, 5.0)), block(Vec3::new(0.0, 0.0, 5.0), Vec3::new(5.0, 30.0, 35.0))];
        let l = shape(&l);
        assert!(!symmetric_about(&l, &mid_plane(&l, 0).unwrap()));
        assert!(symmetric_about(&l, &mid_plane(&l, 1).unwrap()));
        assert!(!symmetric_about(&l, &mid_plane(&l, 2).unwrap()));
        assert!(!symmetric_about(&ComponentShape::default(), &Plane::XY));
        assert!(mid_plane(&ComponentShape::default(), 0).is_none());
    }
}
