//! Measures and queries: mass properties, area, the exact box, where a point lies, the samples of
//! the edge/face gap, the manifold test and the solid count, and the distances and normals an
//! application's picks compare. Every number is the kernel's; ops converts and compares.

use solidcore::error::EntityRef;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::tol::{brep_zone_tol_3d, SmallTopoType};
use solidcore::intersect::point_class::PointClassType;
use solidcore::mass::integrator::properties::{
    brep_area_about, brep_mass_properties_estimated, region_material, AreaAbout,
    MassPropertyRequest, PropertyAttributes,
};
use solidcore::query::api_queries::{
    self as q, brep_bounding_box, brep_is_manifold_solid, brep_material_census,
    edge_face_gap_samples, face_normal_at_edge,
};
use solidcore::query::brep::BrepClassifier;
use solidcore::topology::face::p2::{self as face_p2, face_side_regions};
use solidcore::topology::{Brep, FaceId, FaceuseId, RegionId};
use solidcore::{Point3, Vec3};

use crate::body::Body;
use crate::error::status;
use crate::order::{self, Order};
use crate::session::{answer, with_body, Answer};
use crate::{Error, Result};

impl Answer for Vec<q::EdgeFaceGapSample> {}
impl Answer for Distance {}
impl Answer for solidcore::geom::extent3d::Extent3d {}

fn array(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn point(p: [f64; 3]) -> Result<Point3> {
    if !p.iter().all(|x| x.is_finite()) {
        return Err(Error::Argument);
    }
    Ok(Point3::new(p[0], p[1], p[2]))
}

/// Mass properties of a body at unit density (mm, mm², mm³, mm⁵), and what they are worth.
///
/// **Requested accuracy, estimated, not proved.** The kernel's integrator holds its error
/// ESTIMATE under `requested_accuracy`; that is not a bound, and nothing may treat it as one.
/// For a density ρ the mass is ρ·`volume` and the inertia scales by ρ.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MassPropertiesEstimate {
    /// The accuracy the integrator's error estimate was held to (not a proved bound).
    pub requested_accuracy: f64,
    /// The area of the faces between material and void (the solid's boundary) and of the
    /// sheet faces (void on both sides, or one material region on both sides), each once. A
    /// partition between two material regions is no part of it, so it differs from
    /// [`Body::area`], which counts every face once, partitions too.
    pub area: f64,
    /// The material regions' volume; 0 for a body with no material.
    pub volume: f64,
    /// The centre of mass; of the area for a body with no material.
    pub centroid: [f64; 3],
    /// Ixx, Iyy, Izz about axes through the centroid parallel to the world axes:
    /// ∫(y²+z²), ∫(z²+x²), ∫(x²+y²) dV.
    pub moments: [f64; 3],
    /// Iyz, Izx, Ixy about the centroid: ∫yz, ∫zx, ∫xy dV relative to it.
    pub products: [f64; 3],
}

impl MassPropertiesEstimate {
    /// The inertia tensor about the centroid (row-major), whose off-diagonal terms are the
    /// negated products.
    #[must_use]
    pub fn inertia_tensor(&self) -> [[f64; 3]; 3] {
        let [ixx, iyy, izz] = self.moments;
        let [iyz, izx, ixy] = self.products;
        [[ixx, -ixy, -izx], [-ixy, iyy, -iyz], [-izx, -iyz, izz]]
    }
}

/// Where a point lies against a body ([`Body::classify_point`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointClass {
    /// In a void region: outside the material, or in a cavity.
    Outside,
    /// In a material region.
    Inside,
    /// Within the body's tolerance of a face, an edge or a vertex.
    OnBoundary,
}

/// A distance with its proved rounding bound: the exact distance between the points it was
/// measured between is within `bound` of `value`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Distance {
    /// The distance.
    pub value: f64,
    /// The proved bound on its rounding.
    pub bound: f64,
}

impl From<q::Distance> for Distance {
    fn from(d: q::Distance) -> Self {
        Distance {
            value: d.value,
            bound: d.bound,
        }
    }
}

/// One sample of the edge/face gap ([`Body::edge_face_gap_samples`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GapSample {
    /// The edge curve's parameter.
    pub edge_parameter: f64,
    /// The curve's point there.
    pub edge_point: [f64; 3],
    /// The face surface's (u, v) the kernel's drop found.
    pub face_uv: [f64; 2],
    /// The surface's point there.
    pub face_point: [f64; 3],
    /// The distance between the two points as evaluated, with its proved rounding bound (the face
    /// point is the evaluator's, rounded: alone it bounds nothing about the surface).
    pub distance: Distance,
    /// A proved upper bound on the edge point's distance to the surface, from the enclosure of
    /// the exact surface point at `face_uv`; `None` where the kernel does not enclose the
    /// surface (the sample is then a sample only). Not the nearest point's distance: the drop
    /// may not have found the nearest point.
    pub surface_bound: Option<f64>,
}

/// The point of an edge nearest a point ([`Body::edge_closest_point`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgePoint {
    /// The point on the edge.
    pub point: [f64; 3],
    /// The edge curve's parameter there.
    pub parameter: f64,
    /// The kernel solve's distance (not certified: a local solve's).
    pub distance: f64,
}

/// A unit normal with its proved bound: the exact unit normal is within `bound` of `value`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Normal {
    /// The unit normal.
    pub value: [f64; 3],
    /// The proved bound on its distance from the exact one.
    pub bound: f64,
}

/// The distance between two points, with its proved rounding bound
/// (`query::api_queries::point_distance`).
///
/// # Errors
/// [`Error::Argument`] for a coordinate that is not finite; the kernel's refusal of a distance
/// that overflows.
pub fn point_distance(a: [f64; 3], b: [f64; 3]) -> Result<Distance> {
    let (a, b) = (point(a)?, point(b)?);
    q::point_distance(a, b).map(Distance::from).map_err(status)
}

impl Body {
    /// Mass properties at `relative_accuracy` (the kernel's rule: finite and positive, clamped
    /// to [1e-8, 1e-1]): the sum over the body's material regions, so several lumps, or lumps
    /// touching along edges or at points, measure too, with the area of sheet faces counted
    /// ([`MassPropertiesEstimate::area`]). A body with no material (a sheet, an open shell)
    /// has volume 0 and no inertia, its faces' area and the centroid of that area.
    /// **Estimated, not proved** ([`MassPropertiesEstimate`]).
    ///
    /// # Errors
    /// [`Error::Status`] `SM_ERR_INVALID_INPUT` for an accuracy the kernel refuses, no mass or no
    /// area, or a result that is not finite; [`Error::Status`] `SM_ERR_NOT_CONVERGING` when the
    /// integration does not converge (the kernel's `NotConverged`); [`Error::Degenerate`] for
    /// an integrand that is not finite; [`Error::Unsupported`] for a span the integrator took
    /// no step over, or a body with a partition while the session's
    /// `brep_area_between_solid_and_void` fix is off (its area would count the partition from
    /// both sides); [`Error::UnsureRegion`] for a region the kernel is unsure is material.
    pub fn mass_properties(&self, relative_accuracy: f64) -> Result<MassPropertiesEstimate> {
        with_body("mass_properties", self, |k, b| {
            let store = AttributeStore::default();
            let attrs = PropertyAttributes {
                store: &store,
                key_of: &|_| ObjectKey(0),
            };
            let r = match brep_mass_properties_estimated(
                k,
                b,
                relative_accuracy,
                &MassPropertyRequest::centroidal(),
                &attrs,
            ) {
                Ok(r) => r,
                Err(e) => return Ok(Err(status(e))),
            };
            Ok(Ok(MassPropertiesEstimate {
                requested_accuracy: r.requested_accuracy,
                area: r.area,
                volume: r.volume,
                centroid: array(r.centroid),
                moments: array(r.inertia),
                products: array(r.products),
            }))
        })?
    }

    /// Every face's area, each counted once (sheets, open shells and partitions between two
    /// material regions too: unlike [`MassPropertiesEstimate::area`], which leaves the
    /// partitions out), at `relative_accuracy` (the kernel's rule: finite and positive,
    /// clamped to [1e-8, 1e-1]), integrated about the world origin. **Estimated, not proved**
    /// ([`MassPropertiesEstimate`]).
    ///
    /// # Errors
    /// [`Error::Status`] `SM_ERR_INVALID_INPUT` for an accuracy the kernel refuses or an area
    /// that is not finite; the kernel's error.
    pub fn area(&self, relative_accuracy: f64) -> Result<f64> {
        let a = with_body("area", self, |k, b| {
            Ok(
                brep_area_about(k, b, relative_accuracy, AreaAbout::Point(Point3::ZERO))
                    .map_err(status),
            )
        })??;
        if !a.is_finite() {
            return Err(Error::Status(crate::error::INVALID_INPUT));
        }
        Ok(a)
    }

    /// The exact axis-aligned box, least corner then greatest (`SmBrep::CalculateTightBoundingBox`):
    /// the extrema of every vertex, edge and face on its exact curve or surface.
    ///
    /// # Errors
    /// The kernel's error; [`Error::Status`] `SM_ERR_INVALID_INPUT` for no box (not finite, or
    /// its corners out of order).
    pub fn bounds(&self) -> Result<([f64; 3], [f64; 3])> {
        let x = with_body("bounds", self, |k, b| brep_bounding_box(k, b, true))?;
        let (lo, hi) = (array(x.min()), array(x.max()));
        let finite = lo.iter().chain(&hi).all(|v| v.is_finite());
        if !finite || lo.iter().zip(&hi).any(|(a, b)| a > b) {
            return Err(Error::Status(crate::error::INVALID_INPUT));
        }
        Ok((lo, hi))
    }

    /// The samples of the kernel's edge/face gap for record edge `edge` and record face `face`
    /// (`order`): the edge's curve over its interval against the face's surface over that
    /// surface's own domain. Each sample gives its points, their distance and, where the kernel
    /// encloses the surface, a proved upper bound on the curve point's distance to it; the
    /// largest sample is NOT the edge's largest gap (the curve is not bounded between samples).
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the edges or faces; the kernel's error.
    pub fn edge_face_gap_samples(
        &self,
        order: &Order,
        edge: usize,
        face: usize,
    ) -> Result<Vec<GapSample>> {
        let (e, f) = (order::edge(order, edge)?, order::face(order, face)?);
        let samples = with_body("edge_face_gap_samples", self, |k, b| {
            edge_face_gap_samples(k, b, e, f)
        })?;
        Ok(samples
            .into_iter()
            .map(|s| GapSample {
                edge_parameter: s.edge_parameter,
                edge_point: array(s.edge_point),
                face_uv: [s.face_uv.x, s.face_uv.y],
                face_point: array(s.face_point),
                distance: s.distance.into(),
                surface_bound: s.surface_bound,
            })
            .collect())
    }

    /// Where `p` lies against this body (`SmBrep::Point3DClassify` at the body's own zone
    /// tolerance, with boundary intersections): on the boundary when within that tolerance of a
    /// face, an edge or a vertex (sheet faces too), else inside a material region or outside. A
    /// body with no material has no inside. The kernel's classification may add UV trim curves
    /// to the body.
    ///
    /// # Errors
    /// [`Error::Argument`] for a coordinate that is not finite; [`Error::UnsureRegion`] for a
    /// point in a region the kernel is unsure is material (never read as outside or inside,
    /// the rule of the mass properties too); the kernel's status.
    pub fn classify_point(&self, p: [f64; 3]) -> Result<PointClass> {
        let p = point(p)?;
        with_body("classify_point", self, |k, b| {
            let tol = brep_zone_tol_3d(k, b, SmallTopoType::NormalSize);
            let mut classifier = BrepClassifier::of_brep(b);
            let pc = match classifier.point_3d_classify(k, p, tol, true) {
                Ok(pc) => pc,
                Err(e) => return Ok(Err(status(e))),
            };
            Ok(match (pc.point_class(), pc.object()) {
                (PointClassType::Vertex | PointClassType::Edge | PointClassType::Face, _) => {
                    Ok(PointClass::OnBoundary)
                }
                (PointClassType::Region, Some(EntityRef::Region(r))) => {
                    Ok(if region_material(classifier.brep(), r)? {
                        PointClass::Inside
                    } else {
                        PointClass::Outside
                    })
                }
                // Point3DClassify reports only the four classes above on success: SM_ERR.
                _ => Err(Error::Status(solidcore::error::SM_ERR)),
            })
        })?
    }

    /// Whether the body is one closed manifold solid (`SmApiBrepIsManifoldSolid`).
    ///
    /// # Errors
    /// The kernel's error.
    pub fn is_manifold_solid(&self) -> Result<bool> {
        with_body("is_manifold_solid", self, |_, b| brep_is_manifold_solid(b))
    }

    /// How many solids the body has (`SmApiBrepMaterialCensus`, its voids dropped).
    ///
    /// # Errors
    /// The kernel's error.
    pub fn solid_count(&self) -> Result<usize> {
        with_body("solid_count", self, |_, b| {
            brep_material_census(b).map(|(solids, _voids)| solids)
        })
    }

    /// The point of record edge `edge` nearest `p` (`SmApiEdgeClosestPoint`), or `None` where the
    /// kernel's solve answers with a status (no point found).
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the edges or a coordinate that is not finite; any
    /// other kernel error.
    pub fn edge_closest_point(
        &self,
        order: &Order,
        edge: usize,
        p: [f64; 3],
    ) -> Result<Option<EdgePoint>> {
        let (e, p) = (order::edge(order, edge)?, point(p)?);
        let c = with_body("edge_closest_point", self, |k, b| {
            answer(q::edge_closest_point(k, b, e, p))
        })?;
        Ok(c.map(|c| EdgePoint {
            point: array(c.closest.point),
            parameter: c.parameter,
            distance: c.closest.distance,
        }))
    }

    /// The distance from record vertex `vertex`'s point to `p`, with its proved rounding bound.
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the vertices or a coordinate that is not finite; the
    /// kernel's refusal of a distance that overflows.
    pub fn vertex_distance(&self, order: &Order, vertex: usize, p: [f64; 3]) -> Result<Distance> {
        let (v, p) = (order::vertex(order, vertex)?, point(p)?);
        with_body("vertex_distance", self, |_, b| {
            q::vertex_distance(b, v, p).map(Distance::from)
        })
    }

    /// The outward unit normal of record face `face` at the middle of its record edge `edge`'s
    /// interval (the edge use's point on the face; `query::api_queries::face_normal_at_edge`),
    /// with its proved bound, in the sense of the face's use that faces out of its material
    /// (chosen by its two sides' regions, whatever way its surface is parameterised: at a
    /// cavity, into the cavity): or, for a face with material on neither side, of its upward
    /// face use (its surface normal's side). The sense is that use's orientation, proved.
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the edges or faces, or an edge that does not bound
    /// the face; [`Error::Unsupported`] for a face with material on both sides (a partition
    /// between two material regions, or a fin inside one): no side of it faces out;
    /// [`Error::UnsureRegion`] for a side the kernel is unsure is material;
    /// [`Error::Degenerate`] at a point where the surface has no normal (a pole, an apex); the
    /// kernel's error (a refused certification too).
    pub fn face_normal_at_edge(&self, order: &Order, edge: usize, face: usize) -> Result<Normal> {
        let (e, f) = (order::edge(order, edge)?, order::face(order, face)?);
        let n = with_body("face_normal_at_edge", self, |k, b| {
            if solidcore::topology::edge::edgeuse_of_face(b, e, Some(f))?.is_none() {
                return Ok(Err(Error::Argument));
            }
            let fu = match outer_faceuse(b, f)? {
                Ok(fu) => fu,
                Err(e) => return Ok(Err(e)),
            };
            face_normal_at_edge(k, b, e, f, fu).map(Ok)
        })??;
        Ok(Normal {
            value: array(n.normal.value),
            bound: n.normal.bound,
        })
    }
}

/// The use of face `f` that faces out of its material, chosen by the materiality of the
/// regions on its two sides (`face_side_regions`, `region_material`), never by how its
/// surface is parameterised (a face use points into its shell's region):
/// - material on one side only: the use on the other side (a side with no region is void);
/// - material on neither side (a sheet): the upward use, its surface normal's side;
/// - material on both sides (a partition, a fin): refused, [`Error::Unsupported`];
/// - a face without its two uses: [`Error::Argument`].
///
/// # Errors
/// The kernel's `UnsureRegion` for a side it is unsure is material; `NotFound` for a dangling
/// id.
pub(crate) fn outer_faceuse(b: &Brep, f: FaceId) -> solidcore::Result<Result<FaceuseId>> {
    let (Some(x), Some(y)) = face_p2::faceuses(b, f)? else {
        return Ok(Err(Error::Argument));
    };
    let material = |r: Option<RegionId>| match r {
        Some(r) => region_material(b, r),
        None => Ok(false),
    };
    let [rx, ry] = face_side_regions(b, f)?;
    Ok(match (material(rx)?, material(ry)?) {
        (true, false) => Ok(y),
        // x on the void side of a material y; for a sheet, x is the upward use (its surface
        // normal's side)
        (false, _) => Ok(x),
        (true, true) => Err(Error::Unsupported(
            "a face with material on both sides has no side that faces out",
        )),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{make_box, session};

    fn all_fixes() {
        session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    }

    /// The order of a body made in the kernel with no record between: the kernel's own lists.
    pub(crate) fn kernel_order(b: &Body) -> Order {
        let k = b.lock();
        Order {
            faces: k.face_list().to_vec(),
            edges: k.edge_list().to_vec(),
            vertices: k.vertex_list().to_vec(),
        }
    }

    #[test]
    fn a_box_edge_gap_samples_distances_and_normals() {
        all_fixes();
        let b = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
        let index = kernel_order(&b);
        // every edge against every face: each sample's distance at most the box's diagonal (3.74), and an
        // edge on a face's plane sampled at 0
        let mut on = 0;
        for e in 0..index.edges.len() {
            for f in 0..index.faces.len() {
                let samples = b.edge_face_gap_samples(&index, e, f).unwrap();
                assert!(!samples.is_empty());
                for s in &samples {
                    assert!(
                        s.distance.value <= 3.75 && s.distance.bound < 1e-13,
                        "{s:?}"
                    );
                }
                on += usize::from(samples.iter().all(|s| s.distance.value <= 1e-12));
            }
        }
        // each edge lies on the planes of its two faces (and of no other)
        assert_eq!(on, 24);
        assert!(b
            .edge_face_gap_samples(&index, index.edges.len(), 0)
            .is_err());
        // the nearest point of each edge to a far point, and the vertex distances
        for e in 0..index.edges.len() {
            let c = b
                .edge_closest_point(&index, e, [0.5, 1.0, 10.0])
                .unwrap()
                .unwrap();
            assert!(c.distance >= 7.0 - 1e-12, "{c:?}");
        }
        for v in 0..index.vertices.len() {
            let d = b.vertex_distance(&index, v, [4.0, 6.0, 15.0]).unwrap();
            assert!(d.value >= 12.0 && d.bound < 1e-13, "{d:?}");
        }
        let d = point_distance([1.0, 2.0, 3.0], [4.0, 6.0, 15.0]).unwrap();
        assert_eq!(d.value, 13.0);
        // the outward normal of each face at each of its edges is an axis, pointing away from the
        // box's middle
        let mut normals = 0;
        for e in 0..index.edges.len() {
            for f in 0..index.faces.len() {
                let Ok(n) = b.face_normal_at_edge(&index, e, f) else {
                    continue;
                };
                let c = b
                    .edge_closest_point(&index, e, [0.5, 1.0, 1.5])
                    .unwrap()
                    .unwrap();
                let out = [c.point[0] - 0.5, c.point[1] - 1.0, c.point[2] - 1.5];
                let along: f64 = n.value.iter().zip(out).map(|(a, b)| a * b).sum();
                assert!(along > 0.0 && n.bound < 1e-12, "{n:?}");
                assert_eq!(
                    n.value.iter().filter(|v| v.abs() == 1.0).count(),
                    1,
                    "{n:?}"
                );
                normals += 1;
            }
        }
        assert_eq!(normals, 24);
    }

    /// Picks are record indices: the same face and edge named through an order that is not the
    /// kernel's give the same answer, and the kernel's index of another entity a different one.
    #[test]
    fn a_pick_names_the_record_s_entity() {
        all_fixes();
        let b = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
        let kernel = kernel_order(&b);
        let mut record = kernel.clone();
        record.faces.reverse();
        record.edges.reverse();
        let (nf, ne) = (record.faces.len(), record.edges.len());
        let mut same = 0;
        for e in 0..ne {
            for f in 0..nf {
                let k = b.face_normal_at_edge(&kernel, e, f);
                let r = b.face_normal_at_edge(&record, ne - 1 - e, nf - 1 - f);
                assert_eq!(k.is_ok(), r.is_ok());
                if let (Ok(k), Ok(r)) = (k, r) {
                    assert_eq!(k, r);
                    same += 1;
                }
            }
        }
        assert_eq!(same, 24);
        let first = b.vertex_distance(&kernel, 0, [9.0, 9.0, 9.0]).unwrap();
        let mut reversed = kernel.clone();
        reversed.vertices.reverse();
        let last = b.vertex_distance(&reversed, 0, [9.0, 9.0, 9.0]).unwrap();
        assert_ne!(first, last);
    }

    /// A region the kernel is unsure of is refused by name, for the point class as for the
    /// mass properties: never read as outside, nor as a box of volume 0.
    #[test]
    fn an_unsure_region_is_refused_by_name() {
        all_fixes();
        let b = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
        let inner = {
            let mut k = b.lock();
            let infinite = k.infinite_region();
            let inner: Vec<_> = solidcore::topology::brep::p1::regions(&k, None)
                .unwrap()
                .into_iter()
                .filter(|&r| Some(r) != infinite)
                .collect();
            assert_eq!(inner.len(), 1);
            k.region_mut(inner[0]).unwrap().set_is_void(2);
            inner[0].index()
        };
        let unsure = Error::UnsureRegion { region: inner };
        assert_eq!(b.classify_point([0.5, 1.0, 1.5]), Err(unsure.clone()));
        assert_eq!(b.mass_properties(1e-8), Err(unsure.clone()));
        // the volume, and the result contract's solid test (through an operation that
        // derives its expectation from its input), refuse it too: not 0, not NotSolid
        assert_eq!(b.volume(1e-8), Err(unsure.clone()));
        let order = kernel_order(&b);
        assert_eq!(
            crate::stitch::merge_faces(b, &order, &[]).map(|_| ()),
            Err(unsure)
        );
    }

    /// Two 10 mm cubes and the cube [3, 7]^3 inside the first, its faces turned
    /// (`SwapUV`: each surface's normal reversed, the face's other use made upward) so that
    /// they point into the cavity, as an import or `OrientTrimmedSurfaces` may leave them.
    fn cavity(turned: bool) -> Body {
        let b = make_box([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap();
        let c = make_box([3.0, 3.0, 3.0], [4.0, 4.0, 4.0]).unwrap();
        let b = crate::boolean::boolean(b, c, crate::boolean::BooleanOp::Subtract)
            .unwrap()
            .unwrap();
        if turned {
            session::run("turn", |k| {
                let mut g = b.lock();
                for f in g.face_list().to_vec() {
                    let x = solidcore::query::api_queries::face_bounding_box(k, &g, f, true)?;
                    if x.min().x > 2.0 && x.max().x < 8.0 {
                        solidcore::topology::face::p1::swap_uv(k, &mut g, f)?;
                    }
                }
                g.refresh_cached_topology();
                Ok(())
            })
            .unwrap();
        }
        b
    }

    /// The outward normal at a cavity's face points into the cavity (out of the material),
    /// whichever way the cavity's surfaces are parameterised; at the outer faces, away from
    /// the middle.
    #[test]
    fn a_cavity_s_normals_point_into_the_cavity_whatever_its_surfaces() {
        all_fixes();
        for turned in [false, true] {
            let b = cavity(turned);
            let o = kernel_order(&b);
            let (mut inner, mut outer) = (0, 0);
            for e in 0..o.edges.len() {
                for f in 0..o.faces.len() {
                    let Ok(n) = b.face_normal_at_edge(&o, e, f) else {
                        continue;
                    };
                    let p = b
                        .edge_closest_point(&o, e, [5.0, 5.0, 5.0])
                        .unwrap()
                        .unwrap()
                        .point;
                    let to_middle = [5.0 - p[0], 5.0 - p[1], 5.0 - p[2]];
                    let d: f64 = n.value.iter().zip(to_middle).map(|(a, b)| a * b).sum();
                    if p.iter().all(|x| (2.9..=7.1).contains(x)) {
                        assert!(
                            d > 0.0,
                            "turned {turned}: a cavity normal into the material"
                        );
                        inner += 1;
                    } else {
                        assert!(d < 0.0, "turned {turned}: an outer normal inwards");
                        outer += 1;
                    }
                }
            }
            assert_eq!((inner, outer), (24, 24), "turned {turned}");
        }
    }

    /// The kernel's `Degenerate` (a normal at a pole or an apex) and `UnsureRegion` are typed
    /// at the ops boundary, not a plain status.
    #[test]
    fn degenerate_and_unsure_are_typed() {
        use solidcore::SolidError;
        let d = SolidError::Degenerate {
            operation: "x",
            entity: None,
        };
        assert_eq!(status(d), Error::Degenerate);
        let b = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
        let r = solidcore::topology::brep::p1::regions(&b.lock(), None).unwrap()[0];
        let u = SolidError::UnsureRegion {
            operation: "x",
            region: r,
        };
        assert_eq!(status(u), Error::UnsureRegion { region: r.index() });
    }

    /// A face with material on both sides (the partition of two merged boxes) has no side
    /// that faces out: refused, not given a sense.
    #[test]
    fn a_partition_has_no_outward_normal() {
        all_fixes();
        let a = make_box([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap();
        let c = make_box([3.0, 3.0, 3.0], [4.0, 4.0, 4.0]).unwrap();
        session::run("merge", |k| {
            let mut g = a.lock();
            let x = std::mem::replace(&mut *g, Brep::new());
            *g = solidcore::boolean::api_brep::boolean_merge(k, x, c.into_inner())?;
            Ok(())
        })
        .unwrap();
        let o = kernel_order(&a);
        let mut refused = 0;
        for e in 0..o.edges.len() {
            for f in 0..o.faces.len() {
                if let Err(Error::Unsupported(_)) = a.face_normal_at_edge(&o, e, f) {
                    refused += 1;
                }
            }
        }
        // the inner cube's 6 faces at their 4 edges each
        assert_eq!(refused, 24);
    }
}
