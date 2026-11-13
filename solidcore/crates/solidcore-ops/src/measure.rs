//! Measures and queries: mass properties, area, the exact box, where a point lies, the samples of
//! the edge/face gap, the manifold test and the solid count, the distances and normals an
//! application's picks compare, and each face's area, centroid and plane normal and each edge's
//! length and middle. Every number is the kernel's; ops converts and compares.

use solidcore::error::EntityRef;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::Shape;
use solidcore::geom::tol::{brep_zone_tol_3d, SmallTopoType};
use solidcore::intersect::point_class::PointClassType;
use solidcore::mass::integrator::properties::{
    brep_area_about, brep_mass_properties_estimated, edge_length, face_area, face_centroid,
    region_material, AreaAbout, MassPropertyRequest, PropertyAttributes,
};
use solidcore::query::api_queries::{
    self as q, brep_bounding_box, brep_is_manifold_solid, brep_material_census,
    edge_face_gap_samples, face_normal_at_edge,
};
use solidcore::query::brep::{BrepClassifier, BrepQuery, LiveBrepObjects};
use solidcore::topology::brep::outward_solid_failure;
use solidcore::topology::face::p2::{self as face_p2, face_side_regions};
use solidcore::topology::{edge, face};
use solidcore::topology::{Brep, BrepId, FaceId, FaceuseId, RegionId};
use solidcore::{Kernel, Point3, Vec3};

use crate::blend::middle;
use crate::body::Body;
use crate::error::status;
use crate::order::{self, Order};
use crate::session::{answer, measure_body, with_body, Answer};
use crate::{Error, Result};

impl Answer for Vec<q::EdgeFaceGapSample> {}
impl Answer for Distance {}
impl Answer for solidcore::geom::extent3d::Extent3d {}
impl Answer for [f64; 3] {}
impl Answer for Vec<Option<f64>> {}
impl Answer for Vec<Vec<Option<f64>>> {}
impl Answer for Vec<Result<PointClass>> {}

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
    /// The inertia tensor about the centroid (row-major), the kernel's: whose off-diagonal
    /// terms are the negated products.
    pub tensor: [[f64; 3]; 3],
}

impl MassPropertiesEstimate {
    /// The inertia tensor about the centroid (row-major), whose off-diagonal terms are the
    /// negated products.
    #[must_use]
    pub fn inertia_tensor(&self) -> [[f64; 3]; 3] {
        self.tensor
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

/// Where the kernel's classification put a point ([`locate_points`]), before any rule for a
/// region it is unsure of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Located {
    /// Within the body's zone tolerance of a face, an edge or a vertex.
    OnBoundary,
    /// In `region`: material (`Some(true)`), void (`Some(false)`), or a region the kernel is
    /// unsure of (`None`, its `IsVoid` `UNSURE`).
    InRegion {
        /// The region the point lies in.
        region: RegionId,
        /// Whether the region is material, `None` when the kernel is unsure.
        material: Option<bool>,
    },
}

/// Why [`locate_points`] could not place a point.
#[derive(Clone, Debug, PartialEq)]
pub enum LocateFailure {
    /// A coordinate is not finite; the point was not classified.
    NotFinite,
    /// The kernel's classification refused the point.
    Classify(solidcore::SolidError),
    /// The region the point lies in could not be read.
    Region(solidcore::SolidError),
    /// The classification succeeded with a class other than a vertex, an edge, a face or a
    /// region (`SM_ERR` upstream).
    Unclassified,
}

/// Where each of `points` lies against `b`, in order: `SmBrep::Point3DClassify` at the body's
/// own zone tolerance (`SmTol::GetZoneTol3d`), with boundary intersections, so a point within
/// that tolerance of a vertex, an edge or a face is on the boundary and never decided by a ray.
/// The tolerance and one `BrepClassifier` (its private copy of `b`, which a solve may give UV
/// trim curves, and its search structures) are made once for all the points. The kernel's own
/// answer, before any rule for a region it is unsure of: [`Body::classify_points`] refuses such
/// a region, a caller holding a raw B-rep (SolidCraft's smlib) chooses its own rule.
#[must_use]
pub fn locate_points(
    k: &Kernel,
    b: &Brep,
    points: &[[f64; 3]],
) -> Vec<std::result::Result<Located, LocateFailure>> {
    let tol = brep_zone_tol_3d(k, b, SmallTopoType::NormalSize);
    let mut classifier = BrepClassifier::of_brep(b);
    points
        .iter()
        .map(|&p| locate(k, &mut classifier, tol, p))
        .collect()
}

/// One point of [`locate_points`] through `classifier` at zone tolerance `tol`.
fn locate(
    k: &Kernel,
    classifier: &mut BrepClassifier<'_, '_>,
    tol: f64,
    p: [f64; 3],
) -> std::result::Result<Located, LocateFailure> {
    let p = point(p).map_err(|_| LocateFailure::NotFinite)?;
    let pc = classifier
        .point_3d_classify(k, p, tol, true)
        .map_err(LocateFailure::Classify)?;
    match (pc.point_class(), pc.object()) {
        (PointClassType::Vertex | PointClassType::Edge | PointClassType::Face, _) => {
            Ok(Located::OnBoundary)
        }
        (PointClassType::Region, Some(EntityRef::Region(region))) => {
            match region_material(classifier.brep(), region) {
                Ok(m) => Ok(Located::InRegion {
                    region,
                    material: Some(m),
                }),
                Err(solidcore::SolidError::UnsureRegion { .. }) => Ok(Located::InRegion {
                    region,
                    material: None,
                }),
                Err(e) => Err(LocateFailure::Region(e)),
            }
        }
        _ => Err(LocateFailure::Unclassified),
    }
}

/// The only answer of a [`locate_points`] call on one point.
fn one(
    mut v: Vec<std::result::Result<Located, LocateFailure>>,
) -> std::result::Result<Located, LocateFailure> {
    v.pop().unwrap_or(Err(LocateFailure::Unclassified))
}

/// [`Body::classify_point`]'s class of a point [`locate_points`] placed: a region the kernel
/// is unsure of is refused by name, never read as outside or inside.
fn point_class(l: std::result::Result<Located, LocateFailure>) -> Result<PointClass> {
    match l {
        Ok(Located::OnBoundary) => Ok(PointClass::OnBoundary),
        Ok(Located::InRegion {
            material: Some(true),
            ..
        }) => Ok(PointClass::Inside),
        Ok(Located::InRegion {
            material: Some(false),
            ..
        }) => Ok(PointClass::Outside),
        Ok(Located::InRegion {
            region,
            material: None,
        }) => Err(Error::UnsureRegion {
            region: region.index(),
        }),
        Err(LocateFailure::NotFinite) => Err(Error::Argument),
        Err(LocateFailure::Classify(e) | LocateFailure::Region(e)) => Err(status(e)),
        // Point3DClassify reports only the four classes above on success: SM_ERR.
        Err(LocateFailure::Unclassified) => Err(Error::Status(solidcore::error::SM_ERR)),
    }
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

/// A face's area and the centroid of that area ([`Body::face_centroid`]), from one
/// integration. **Estimated, not proved** ([`MassPropertiesEstimate`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceCentroid {
    /// The face's area.
    pub area: f64,
    /// The centroid of its area.
    pub centroid: [f64; 3],
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
    /// [`Error::Refused`], in the kernel's words, for an accuracy it refuses, no mass or no area,
    /// or a result that is not finite; [`Error::Status`] `SM_ERR_NOT_CONVERGING` when the
    /// integration does not converge (the kernel's `NotConverged`); [`Error::Degenerate`] for
    /// an integrand that is not finite; [`Error::Unsupported`] for a span the integrator took
    /// no step over; [`Error::UnsureRegion`] for a region the kernel is unsure is material.
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
                tensor: r.inertia_tensor(),
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
    /// [`Error::Refused`] for an accuracy the kernel refuses, in its words; [`Error::Status`]
    /// `SM_ERR_INVALID_INPUT` for an area that is not finite; the kernel's error.
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
        point(p)?;
        with_body("classify_point", self, |k, b| {
            Ok(point_class(one(locate_points(k, b, &[p]))))
        })?
    }

    /// [`Body::classify_point`] for each of `points`, in order, through one classification of
    /// the body ([`locate_points`]): its zone tolerance, its private copy and its search
    /// structures are made once for all of them, not once a point (on a large body they cost
    /// far more than a point's own solve). Each point's class, or its own error.
    ///
    /// # Errors
    /// The kernel's error from the call itself; each point's own error is in its place, as
    /// [`Body::classify_point`] gives it.
    pub fn classify_points(&self, points: &[[f64; 3]]) -> Result<Vec<Result<PointClass>>> {
        if points.iter().all(|&p| point(p).is_err()) {
            return Ok(points.iter().map(|_| Err(Error::Argument)).collect());
        }
        with_body("classify_points", self, |k, b| {
            Ok(locate_points(k, b, points)
                .into_iter()
                .map(point_class)
                .collect())
        })
    }

    /// Whether the body is one closed manifold solid (`SmApiBrepIsManifoldSolid`).
    ///
    /// # Errors
    /// The kernel's error.
    pub fn is_manifold_solid(&self) -> Result<bool> {
        with_body("is_manifold_solid", self, |_, b| brep_is_manifold_solid(b))
    }

    /// Whether the body is a closed solid facing out, as an operation's result must be when its
    /// input is one (one closed manifold solid, valid, its volume proved positive): `Ok` where
    /// it is, else [`Error::NotSolidBecause`] with the first test it fails
    /// (`solidcore::topology::brep::outward_solid_failure`). The tests run on a copy, so the
    /// body is unchanged.
    ///
    /// # Errors
    /// [`Error::NotSolidBecause`]; the kernel's error from the tests.
    pub fn check_solid(&self) -> Result<()> {
        measure_body("check_solid", self, |k, b| {
            Ok(match outward_solid_failure(k, b)? {
                None => Ok(()),
                Some(test) => Err(Error::NotSolidBecause(test)),
            })
        })?
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

    /// The distance from each of `points` to record face `face` as it is trimmed
    /// (`SmApiFaceClosestPoint`), or `None` for a point the kernel's solve answers with a status
    /// (no point found). The face's shape is made once for all the points.
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the faces or a coordinate that is not finite; any
    /// other kernel error.
    pub fn face_distances(
        &self,
        order: &Order,
        face: usize,
        points: &[[f64; 3]],
    ) -> Result<Vec<Option<f64>>> {
        let f = order::face(order, face)?;
        let points = points
            .iter()
            .map(|p| point(*p))
            .collect::<Result<Vec<_>>>()?;
        with_body("face_distances", self, |k, b| {
            let id = BrepId::from_index(0);
            let Some(shape) = answer(Shape::new(k, &(id, &*b), &[], &[(id, f)], &[], &[]))? else {
                return Ok(points.iter().map(|_| None).collect());
            };
            let query = BrepQuery::of_brep(b);
            let identity = query.effective_identity();
            let objects = LiveBrepObjects::new(b, &identity, None)?;
            points
                .iter()
                .map(|p| {
                    let c = answer(q::face_closest_point_in_shape(
                        k,
                        &shape,
                        &objects,
                        (id, f),
                        *p,
                    ))?;
                    Ok(c.map(|c| c.distance))
                })
                .collect()
        })
    }

    /// The distance from each of `points` to every record face as it is trimmed, face by face in
    /// record order: row `f` holds one entry per point, `None` where the kernel's solve answers
    /// with a status, and `None` where no point of face `f` comes within `reach` of that point.
    ///
    /// The kernel skips a face no point comes within `reach` of, because a face lies inside its
    /// own box: such a point is further than `reach` from the face. A caller that keeps only
    /// distances within `reach` therefore reads what [`Body::face_distances`] gives face by face,
    /// including each point's nearest face whenever that distance is within `reach`.
    ///
    /// The whole pass is one kernel call, where [`Body::face_distances`] is one call per face.
    ///
    /// # Errors
    /// [`Error::Argument`] for a coordinate that is not finite; the kernel's refusal of a `reach`
    /// that is not finite and at least zero; any other kernel error.
    pub fn face_distances_within(
        &self,
        order: &Order,
        points: &[[f64; 3]],
        reach: f64,
    ) -> Result<Vec<Vec<Option<f64>>>> {
        let faces = (0..order.faces.len())
            .map(|f| order::face(order, f))
            .collect::<Result<Vec<_>>>()?;
        let points = points
            .iter()
            .map(|p| point(*p))
            .collect::<Result<Vec<_>>>()?;
        with_body("face_distances_within", self, |k, b| {
            q::faces_closest_distances_within(k, b, BrepId::from_index(0), &faces, &points, reach)
        })
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
            if edge::edgeuse_of_face(b, e, Some(f))?.is_none() {
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

    /// The area of record face `face` (`SmApiFaceComputeArea`) at `relative_accuracy`, as the
    /// kernel's contract takes it (ops passes it on as given). **Estimated, not proved**
    /// ([`MassPropertiesEstimate`]).
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the faces; [`Error::Refused`] for an accuracy the
    /// kernel refuses, in its words; [`Error::Status`] `SM_ERR_INVALID_INPUT` for an area that
    /// is not finite; the kernel's error.
    pub fn face_area(&self, order: &Order, face: usize, relative_accuracy: f64) -> Result<f64> {
        let f = order::face(order, face)?;
        let a = with_body("face_area", self, |k, b| {
            Ok(face_area(k, b, f, relative_accuracy).map_err(status))
        })??;
        if !a.is_finite() {
            return Err(Error::Status(crate::error::INVALID_INPUT));
        }
        Ok(a)
    }

    /// The area of record face `face` and the centroid of that area
    /// (`SmApiFaceComputeCentroid`, one integration of the area and its first moments) at
    /// `relative_accuracy`, as the kernel's contract takes it (ops passes it on as given).
    /// **Estimated, not proved** ([`MassPropertiesEstimate`]).
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the faces; [`Error::Refused`] for an accuracy the
    /// kernel refuses or a face of no area, in its words; [`Error::Status`]
    /// `SM_ERR_INVALID_INPUT` for a result that is not finite; the kernel's error.
    pub fn face_centroid(
        &self,
        order: &Order,
        face: usize,
        relative_accuracy: f64,
    ) -> Result<FaceCentroid> {
        let f = order::face(order, face)?;
        let c = with_body("face_centroid", self, |k, b| {
            Ok(face_centroid(k, b, f, relative_accuracy)
                .map(|(area, centroid)| FaceCentroid {
                    area,
                    centroid: array(centroid),
                })
                .map_err(status))
        })??;
        if !c.area.is_finite() || !c.centroid.iter().all(|x| x.is_finite()) {
            return Err(Error::Status(crate::error::INVALID_INPUT));
        }
        Ok(c)
    }

    /// The outward unit normal of record face `face` if the face is planar, with its proved
    /// bound; `None` for a face that is not. Planar is the kernel's test of the face's surface
    /// at the face's own tolerance (`SmSurface::IsPlanar`: a plane always is; another surface
    /// when its control net's box is that flat). The normal is the one
    /// [`Body::face_normal_at_edge`] gives at the first edge the kernel lists for the face: the
    /// sense of its use that faces out of its material, or for a sheet its upward use.
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the faces; [`Error::Unsupported`] for a planar face
    /// with no edge, or one with material on both sides (no side of it faces out);
    /// [`Error::UnsureRegion`] for a side the kernel is unsure is material; the kernel's error
    /// (a face without a surface, a refused certification).
    pub fn face_plane_normal(&self, order: &Order, face: usize) -> Result<Option<Normal>> {
        let f = order::face(order, face)?;
        let n = with_body("face_plane_normal", self, |k, b| {
            let planar = {
                let surface = face::surface(b, f)?
                    .ok_or_else(|| solidcore::SolidError::null_pointer("ops::face_plane_normal"))?;
                surface.is_planar(k, face::tolerance(b, f)?)?.is_planar
            };
            if !planar {
                return Ok(Ok(None));
            }
            let Some(&e) = face::edges(b, f, None, false, None)?.first() else {
                return Ok(Err(Error::Unsupported("a planar face with no edge")));
            };
            let fu = match outer_faceuse(b, f)? {
                Ok(fu) => fu,
                Err(e) => return Ok(Err(e)),
            };
            face_normal_at_edge(k, b, e, f, fu).map(|n| Ok(Some(n)))
        })??;
        Ok(n.map(|n| Normal {
            value: array(n.normal.value),
            bound: n.normal.bound,
        }))
    }

    /// The length of record edge `edge` (`SmApiEdgeComputeLength`) at `relative_accuracy`, as
    /// the kernel's contract takes it (ops passes it on as given).
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the edges; [`Error::Status`]
    /// `SM_ERR_INVALID_INPUT` for a length that is not finite; the kernel's error (an edge
    /// without a curve, an accuracy it refuses).
    pub fn edge_length(&self, order: &Order, edge: usize, relative_accuracy: f64) -> Result<f64> {
        let e = order::edge(order, edge)?;
        let l = with_body("edge_length", self, |k, b| {
            Ok(edge_length(k, b, e, relative_accuracy).map_err(status))
        })??;
        if !l.is_finite() {
            return Err(Error::Status(crate::error::INVALID_INPUT));
        }
        Ok(l)
    }

    /// The point of record edge `edge`'s curve at the middle of the edge's parameter interval
    /// (the parameter ops chooses; the point is the kernel's evaluation of the curve there).
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the edges; [`Error::Status`]
    /// `SM_ERR_INVALID_INPUT` for a point that is not finite; the kernel's error (an edge
    /// without a curve, an evaluation it refuses).
    pub fn edge_middle(&self, order: &Order, edge: usize) -> Result<[f64; 3]> {
        let e = order::edge(order, edge)?;
        let p = with_body("edge_middle", self, |k, b| {
            let interval = edge::interval(b, e)?;
            let curve = edge::curve(b, e)?
                .ok_or_else(|| solidcore::SolidError::null_pointer("ops::edge_middle"))?;
            curve
                .evaluate_point(k, middle(interval.min(), interval.max()))
                .map(array)
        })?;
        if !p.iter().all(|x| x.is_finite()) {
            return Err(Error::Status(crate::error::INVALID_INPUT));
        }
        Ok(p)
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
        session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
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

    /// A face's distance is to the face as trimmed, not to its whole surface: from above the
    /// middle of a 1 x 2 x 3 box's top, the top is 7 away and each side face reaches only to the
    /// top's edge.
    #[test]
    fn a_face_distance_is_to_the_trimmed_face() {
        all_fixes();
        let b = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
        let index = kernel_order(&b);
        let above = [0.5, 1.0, 10.0];
        let mut got: Vec<f64> = (0..index.faces.len())
            .map(|f| b.face_distances(&index, f, &[above]).unwrap()[0].unwrap())
            .collect();
        got.sort_by(f64::total_cmp);
        // squared: 7 x 7, then 0.5 or 1 across with 7 up, then 10 x 10
        let want = [49.0, 49.25, 49.25, 50.0, 50.0, 100.0];
        for (g, w) in got.iter().zip(want) {
            assert!((g * g - w).abs() <= 1e-9 * w, "{got:?}");
        }
        assert!(b
            .face_distances(&index, index.faces.len(), &[above])
            .is_err());
        assert!(b
            .face_distances(&index, 0, &[[f64::NAN, 0.0, 0.0]])
            .is_err());
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

    /// A batch reads an unsure region as the one-point call does: `classify_points` refuses that
    /// point by name and classifies the others; `locate_points` hands the kernel's answer on as
    /// it is, for a caller with a rule of its own.
    #[test]
    fn a_batch_refuses_an_unsure_region_point_by_point() {
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
            inner[0]
        };
        let points = [
            [0.5, 1.0, 1.5],
            [5.0, 1.0, 1.5],
            [1.0, 1.0, 1.5],
            [f64::NAN, 0.0, 0.0],
        ];
        let unsure = Error::UnsureRegion {
            region: inner.index(),
        };
        assert_eq!(
            b.classify_points(&points).unwrap(),
            vec![
                Err(unsure.clone()),
                Ok(PointClass::Outside),
                Ok(PointClass::OnBoundary),
                Err(Error::Argument)
            ]
        );
        assert_eq!(b.classify_point(points[0]), Err(unsure));
        let located = session::run("locate", |k| Ok(Some(locate_points(k, &b.lock(), &points))))
            .unwrap()
            .unwrap();
        assert_eq!(
            located[0],
            Ok(Located::InRegion {
                region: inner,
                material: None
            })
        );
        assert!(matches!(
            located[1],
            Ok(Located::InRegion {
                material: Some(false),
                ..
            })
        ));
        assert_eq!(located[2], Ok(Located::OnBoundary));
        assert_eq!(located[3], Err(LocateFailure::NotFinite));
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
                        face::p1::swap_uv(k, &mut g, f)?;
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

    /// A box's faces and edges against their closed forms: each face's area is the product of
    /// the box's other two sizes, its centroid the middle of those two and 0 or the size along
    /// its own axis, its normal that axis, pointing away from the box; each edge's length is
    /// one size, its middle half way along it.
    #[test]
    fn a_box_faces_and_edges_measure_their_closed_forms() {
        all_fixes();
        let size = [1.0, 2.0, 3.0];
        let b = make_box([0.0, 0.0, 0.0], size).unwrap();
        let o = kernel_order(&b);
        assert_eq!(o.faces.len(), 6);
        let mut total = 0.0;
        for f in 0..o.faces.len() {
            let a = b.face_area(&o, f, 1e-8).unwrap();
            let c = b.face_centroid(&o, f, 1e-8).unwrap();
            assert!((c.area - a).abs() <= 1e-12 * a, "{a} {c:?}");
            let n = b
                .face_plane_normal(&o, f)
                .unwrap()
                .expect("a box face is planar");
            let axis = n.value.iter().position(|x| x.abs() > 0.5).unwrap();
            assert!(
                (n.value[axis].abs() - 1.0).abs() <= n.bound + 1e-15,
                "{n:?}"
            );
            let [u, v] = [(axis + 1) % 3, (axis + 2) % 3];
            assert!(
                n.value[u].abs() <= n.bound && n.value[v].abs() <= n.bound,
                "{n:?}"
            );
            assert!((a - size[u] * size[v]).abs() < 1e-12, "face {f}: {a}");
            assert!((c.centroid[u] - 0.5 * size[u]).abs() < 1e-12, "{c:?}");
            assert!((c.centroid[v] - 0.5 * size[v]).abs() < 1e-12, "{c:?}");
            let at = c.centroid[axis];
            let far = (at - size[axis]).abs() < 1e-12;
            assert!(far || at.abs() < 1e-12, "{c:?}");
            // outward: along +axis on the far face, -axis on the near one
            assert_eq!(n.value[axis] > 0.0, far, "face {f}: {n:?} {c:?}");
            total += a;
        }
        assert!((total - 22.0).abs() < 1e-12, "{total}");
        let mut lengths = [0usize; 3];
        for e in 0..o.edges.len() {
            let l = b.edge_length(&o, e, 1e-8).unwrap();
            let axis = size.iter().position(|s| (l - s).abs() < 1e-12).unwrap();
            lengths[axis] += 1;
            let m = b.edge_middle(&o, e).unwrap();
            assert!(
                (m[axis] - 0.5 * size[axis]).abs() < 1e-12,
                "edge {e}: {m:?}"
            );
            for i in [(axis + 1) % 3, (axis + 2) % 3] {
                assert!(
                    m[i].abs() < 1e-12 || (m[i] - size[i]).abs() < 1e-12,
                    "{m:?}"
                );
            }
        }
        assert_eq!(lengths, [4, 4, 4]);
    }

    /// A cylinder r 2, h 5 and a ball r 3: the side's area 2πrh and centroid on the axis at mid
    /// height, a cap's area πr² and its normal ∓z, the rims' length 2πr with their middles on
    /// the rim circles, a seam's length h; the ball's area 4πr², not planar.
    #[test]
    fn curved_faces_and_edges_measure_their_closed_forms() {
        use std::f64::consts::PI;
        all_fixes();
        let (r, h) = (2.0, 5.0);
        let b = crate::make_cylinder([0.0, 0.0, 0.0], r, h).unwrap();
        let o = kernel_order(&b);
        let rel = |x: f64, y: f64| (x - y).abs() <= 1e-7 * y.abs().max(1.0);
        let (mut side, mut caps, mut sides) = (0.0, 0.0, 0);
        for f in 0..o.faces.len() {
            let c = b.face_centroid(&o, f, 1e-8).unwrap();
            assert!(rel(b.face_area(&o, f, 1e-8).unwrap(), c.area), "{c:?}");
            if let Some(n) = b.face_plane_normal(&o, f).unwrap() {
                // a cap: on the axis at z = 0 (normal -z) or z = h (normal +z)
                assert!(rel(c.area, PI * r * r), "{c:?}");
                assert!(
                    c.centroid[0].abs() < 1e-9 && c.centroid[1].abs() < 1e-9,
                    "{c:?}"
                );
                let top = (c.centroid[2] - h).abs() < 1e-9;
                assert!(top || c.centroid[2].abs() < 1e-9, "{c:?}");
                let want = if top { 1.0 } else { -1.0 };
                assert!((n.value[2] - want).abs() <= n.bound + 1e-15, "{n:?}");
                caps += c.area;
            } else {
                sides += 1;
                side += c.area;
                if o.faces.len() == 3 {
                    assert!(
                        c.centroid[0].abs() < 1e-7 && c.centroid[1].abs() < 1e-7,
                        "{c:?}"
                    );
                    assert!(rel(c.centroid[2], 0.5 * h), "{c:?}");
                }
            }
        }
        assert!(sides >= 1);
        assert!(rel(side, 2.0 * PI * r * h), "{side}");
        assert!(rel(caps, 2.0 * PI * r * r), "{caps}");
        for e in 0..o.edges.len() {
            let l = b.edge_length(&o, e, 1e-8).unwrap();
            let m = b.edge_middle(&o, e).unwrap();
            // every edge of the cylinder lies on its side: r from the axis
            assert!(rel(m[0] * m[0] + m[1] * m[1], r * r), "edge {e}: {m:?}");
            if rel(l, h) {
                assert!(rel(m[2], 0.5 * h), "seam {e}: {m:?}");
            } else {
                assert!(l <= 2.0 * PI * r * (1.0 + 1e-7), "edge {e}: {l}");
                assert!(
                    m[2].abs() < 1e-9 || (m[2] - h).abs() < 1e-9,
                    "rim {e}: {m:?}"
                );
            }
        }
        let ball = crate::make_sphere([0.0, 0.0, 0.0], 3.0).unwrap();
        let o = kernel_order(&ball);
        let mut area = 0.0;
        for f in 0..o.faces.len() {
            assert_eq!(ball.face_plane_normal(&o, f).unwrap(), None);
            area += ball.face_area(&o, f, 1e-8).unwrap();
        }
        assert!(rel(area, 36.0 * PI), "{area}");
    }

    /// The partition of two merged boxes: its plane normal refused (no side faces out), as
    /// [`Body::face_normal_at_edge`] refuses it, while the outer box's faces have theirs.
    #[test]
    fn a_partition_has_no_plane_normal() {
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
        let (mut refused, mut normals) = (0, 0);
        for f in 0..o.faces.len() {
            match a.face_plane_normal(&o, f) {
                Err(Error::Unsupported(_)) => refused += 1,
                Ok(Some(_)) => normals += 1,
                other => panic!("face {f}: {other:?}"),
            }
        }
        assert_eq!((refused, normals), (6, 6));
    }

    /// Indices past the lists are the caller's error, not the kernel's.
    #[test]
    fn face_and_edge_measures_refuse_an_index_past_the_lists() {
        all_fixes();
        let b = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
        let o = kernel_order(&b);
        assert_eq!(b.face_area(&o, 6, 1e-8), Err(Error::Argument));
        assert_eq!(b.face_centroid(&o, 6, 1e-8), Err(Error::Argument));
        assert_eq!(b.face_plane_normal(&o, 6), Err(Error::Argument));
        assert_eq!(b.edge_length(&o, 12, 1e-8), Err(Error::Argument));
        assert_eq!(b.edge_middle(&o, 12), Err(Error::Argument));
    }

    /// An L-block (the boxes [0,2]×[0,1]×[0,1] and [0,1]×[1,2]×[0,1]): volume 3, centroid
    /// (5/6, 5/6, 1/2), ∫xy dV about it 7/4 − 3·(5/6)² = −1/3 and the other products 0, so
    /// its tensor's xy terms are +1/3 (the negated product) and the rest of it off the diagonal 0.
    #[test]
    fn an_l_block_tensor_holds_its_negated_product() {
        all_fixes();
        let a = make_box([0.0, 0.0, 0.0], [2.0, 1.0, 1.0]).unwrap();
        let c = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 1.0]).unwrap();
        let l = crate::boolean::boolean(a, c, crate::BooleanOp::Union)
            .unwrap()
            .unwrap();
        let m = l.mass_properties(1e-8).unwrap();
        assert!((m.volume - 3.0).abs() < 1e-9, "{m:?}");
        assert!((m.products[2] + 1.0 / 3.0).abs() < 1e-8, "{m:?}");
        let t = m.inertia_tensor();
        assert!(
            (t[0][1] - 1.0 / 3.0).abs() < 1e-8 && (t[1][0] - 1.0 / 3.0).abs() < 1e-8,
            "{t:?}"
        );
        for (i, j) in [(0, 2), (2, 0), (1, 2), (2, 1)] {
            assert!(t[i][j].abs() < 1e-8, "{t:?}");
        }
        for (i, row) in t.iter().enumerate() {
            assert_eq!(row[i], m.moments[i], "{t:?}");
        }
    }
}
