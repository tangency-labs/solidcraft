//! Measures and queries: mass properties, area, the exact box, where a point lies, the samples of
//! the edge/face gap, the manifold test and the solid count, the distances and normals an
//! application's picks compare, and each face's area, centroid and plane normal and each edge's
//! length and middle. Every number is the kernel's; ops converts and compares.

use solidcore::curve::from_parameters;
use solidcore::error::EntityRef;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::object::Shape;
use solidcore::geom::tol::{brep_zone_tol_3d, SmallTopoType};
use solidcore::intersect::point_class::{PointClassType, PointClassification};
use solidcore::mass::integrator::properties::{
    brep_area_about, brep_area_about_prepared, brep_mass_properties_estimated,
    brep_mass_properties_estimated_prepared, edge_length, face_area, face_area_prepared,
    face_centroid, face_centroid_prepared, region_material, AreaAbout, EstimatedMassProperties,
    MassPropertyRequest, PropertyAttributes,
};
use solidcore::query::api_queries::{
    self as q, brep_bounding_box, brep_is_manifold_solid, brep_material_census,
    edge_face_gap_samples, face_normal_at_edge,
};
use solidcore::query::brep::{BrepClassifier, BrepQuery, LiveBrepObjects, Ray};
use solidcore::query::face::p1 as face_p1;
use solidcore::topology::brep::outward_solid_failure;
use solidcore::topology::brep::p3::{
    brep_copy_mapped, merge_brep, notify_serial, topology_ref, with_kernel_callbacks,
};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p2::{self as face_p2, face_side_regions};
use solidcore::topology::proto_topology::p1::CopyBrepMap;
use solidcore::topology::{edge, face};
use solidcore::topology::{Brep, BrepId, EdgeId, FaceId, FaceuseId, RegionId};
use solidcore::{Kernel, Point3, Vec3};

use crate::blend::middle;
use crate::body::Body;
use crate::error::status;
use crate::order::{self, Order};
use crate::session::{answer, measure_body, read_body, read_else_edit, run_on, with_body, Answer};
use crate::split::copies_of;
use crate::{Error, Result};

impl Answer for Vec<q::EdgeFaceGapSample> {}
impl Answer for Distance {}
impl Answer for Extent3d {}
impl Answer for [f64; 3] {}
impl Answer for Vec<[f64; 3]> {}
impl Answer for Vec<Option<f64>> {}
impl Answer for Vec<Vec<Option<f64>>> {}
impl Answer for Vec<Result<PointClass>> {}
impl Answer for Vec<f64> {}

/// A kernel measure that has a read-only form, under one lock of the body
/// ([`read_else_edit`]): `read`, which leaves the body (and its proof) as it is; where the
/// body lacks what the read needs (a UV trim curve, a trimmed cache's tree), which the kernel's
/// read form finds before any work and answers `PreparationRequired`, `make` as [`with_body`]
/// runs it, which makes them on the body as before. The two forms give the same answer where
/// the body has them; the kernel's error is the measure's refusal ([`status`]) either way.
///
/// # Errors
/// As [`with_body`].
fn read_else_make<T>(
    call: &'static str,
    body: &Body,
    read: impl FnOnce(&mut Kernel, &Brep) -> solidcore::Result<T>,
    make: impl FnOnce(&mut Kernel, &mut Brep) -> solidcore::Result<T>,
) -> Result<Result<T>> {
    read_else_edit(
        call,
        body,
        |k, b| match read(k, b) {
            Err(solidcore::SolidError::PreparationRequired { .. }) => Ok(None),
            r => Ok(Some(r.map_err(status))),
        },
        |k, b| Ok(make(k, b).map_err(status)),
    )
}

/// Unit density over every material region, with no per-entity mass attribute (the attribute
/// store is empty): the request [`Body::mass_properties`] measures.
fn centroidal_properties(
    measure: impl FnOnce(&PropertyAttributes<'_>) -> solidcore::Result<EstimatedMassProperties>,
) -> solidcore::Result<EstimatedMassProperties> {
    let store = AttributeStore::default();
    measure(&PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    })
}

/// A planar face square to a direction ([`Body::face_square_level`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SquareFaceLevel {
    /// The signed distance from the origin to the face's plane, along the direction.
    pub level: f64,
    /// The face's own tolerance: levels closer than this are the same level on the face.
    pub tolerance: f64,
}

fn array(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn point(p: [f64; 3]) -> Result<Point3> {
    if !p.iter().all(|x| x.is_finite()) {
        return Err(Error::Argument);
    }
    Ok(Point3::new(p[0], p[1], p[2]))
}

fn point2(p: [f64; 2]) -> Result<solidcore::Point2> {
    if !p.iter().all(|x| x.is_finite()) {
        return Err(Error::Argument);
    }
    Ok(solidcore::Point2::new(p[0], p[1]))
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

/// Which face supplies normals in [`Body::classify_points_with_normals`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointNormalFace {
    /// A record face index.
    Index(usize),
    /// A point on the face; the first containing face in record order is chosen.
    Point([f64; 3]),
}

/// A point's material class and, on the boundary, its named-by-index face and normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClassifiedPoint {
    /// Inside a material region.
    Inside,
    /// Outside material, including holes and cavities.
    Outside,
    /// On a trimmed face within the body's modelling tolerance.
    On {
        /// Face index in the supplied record order.
        face: usize,
        /// Unit normal facing out of material.
        normal: Normal,
    },
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
    located(classifier.brep(), &pc)
}

/// Read a classification against the body that supplied its region ids.
fn located(
    b: &Brep,
    pc: &PointClassification<EntityRef>,
) -> std::result::Result<Located, LocateFailure> {
    match (pc.point_class(), pc.object()) {
        (PointClassType::Vertex | PointClassType::Edge | PointClassType::Face, _) => {
            Ok(Located::OnBoundary)
        }
        (PointClassType::Region, Some(EntityRef::Region(region))) => {
            match region_material(b, region) {
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

/// The point of an edge nearest a ray ([`Body::edge_closest_to_ray`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeRayPoint {
    /// The point on the edge.
    pub point: [f64; 3],
    /// The edge curve's parameter there.
    pub parameter: f64,
    /// The ray's point nearest it.
    pub on_ray: [f64; 3],
    /// The kernel solve's distance between the two (not certified: a solve's).
    pub distance: f64,
}

/// A point where a ray meets a face ([`Body::face_hits_by_ray`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceHit {
    /// The record face.
    pub face: usize,
    /// The point on the ray where it meets the face.
    pub point: [f64; 3],
    /// The distance from the ray's origin to the point, along the ray.
    pub distance: f64,
}

/// The first face a ray meets ([`Body::face_hit_by_ray`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceRayHit {
    /// The record face.
    pub face: usize,
    /// The point where the ray meets it, on the face's surface.
    pub point: [f64; 3],
    /// The face's (u, v) there, as [`crate::face_surface::FaceSurface`] evaluates them.
    pub uv: [f64; 2],
    /// The distance from the ray's origin to the point, along the ray.
    pub distance: f64,
}

/// An edge's curve over the edge's own interval, copied out of its body ([`Body::edge_curve`]) to
/// be measured against faces and edges of any body ([`Body::closest_to_curve`]).
#[derive(Debug)]
pub struct EdgeCurve {
    curve: solidcore::curve::curve::Curve,
    interval: solidcore::geom::extent1d::Extent1d,
}

impl Answer for EdgeCurve {}
impl Answer for MembersClosest {}

/// The closest points of selected exact geometry ([`Body::closest_to_curve`],
/// [`Body::closest_to_point`], [`Body::closest_faces`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MembersClosest {
    /// The kernel solve's distance between the two points (not certified: a solve's).
    pub distance: f64,
    /// On the first selection: its curve, point or trimmed faces.
    pub from: [f64; 3],
    /// On the faces (as they are trimmed) or edges (over their intervals).
    pub to: [f64; 3],
}

/// The entity a ray meets first ([`Body::fire_ray`]), by its index in the body's record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RayEntity {
    /// Record face `i`.
    Face(usize),
    /// Record edge `i`.
    Edge(usize),
    /// Record vertex `i`.
    Vertex(usize),
}

/// Where a ray first meets a body ([`Body::fire_ray`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    /// The face, edge or vertex it meets.
    pub entity: RayEntity,
    /// The distance from the ray's origin along its unit direction: the hit point is
    /// `origin + distance · direction / |direction|`.
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

/// A 2D point set's centroid and mean distance from that centroid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointSetSummary {
    /// The arithmetic mean of the input points.
    pub centroid: [f64; 2],
    /// The arithmetic mean of each point's distance from `centroid`.
    pub mean_distance: f64,
}

/// The arithmetic centroid and mean radial distance of a non-empty point set.
pub fn centroid_and_mean_radius(points: &[[f64; 2]]) -> Result<PointSetSummary> {
    let points = points
        .iter()
        .copied()
        .map(point2)
        .collect::<Result<Vec<_>>>()?;
    let summary = solidcore::math::centroid_and_mean_radius(&points).map_err(status)?;
    Ok(PointSetSummary {
        centroid: summary.centroid.to_array(),
        mean_distance: summary.mean_distance,
    })
}

/// The vector from point `from` to point `to`.
pub fn point_displacement(from: [f64; 2], to: [f64; 2]) -> Result<[f64; 2]> {
    let (from, to) = (point2(from)?, point2(to)?);
    q::point_displacement(from, to)
        .map(solidcore::Vec2::to_array)
        .map_err(status)
}

/// Axis-aligned bounding box `(min, max)` of a 2D polyline.
pub fn polyline_2d_bounds(points: &[[f64; 2]]) -> Result<Option<([f64; 2], [f64; 2])>> {
    let pts: Vec<_> = points
        .iter()
        .copied()
        .map(point2)
        .collect::<Result<Vec<_>>>()?;
    Ok(q::polyline_2d_bounds(&pts)
        .map_err(status)?
        .map(|(lo, hi)| (lo.to_array(), hi.to_array())))
}

/// Distance and closest point from `point` to a 2D polyline.
pub fn polyline_2d_closest_point(
    point: [f64; 2],
    points: &[[f64; 2]],
) -> Result<Option<(f64, [f64; 2])>> {
    let p = point2(point)?;
    let pts: Vec<_> = points
        .iter()
        .copied()
        .map(point2)
        .collect::<Result<Vec<_>>>()?;
    Ok(q::polyline_2d_closest_point(p, &pts)
        .map_err(status)?
        .map(|(d, q)| (d, q.to_array())))
}

/// Intersection point of two 2D segments within tolerance `tol`.
pub fn segment_2d_intersection(
    a1: [f64; 2],
    a2: [f64; 2],
    b1: [f64; 2],
    b2: [f64; 2],
    tol: f64,
) -> Result<Option<[f64; 2]>> {
    Ok(
        q::segment_2d_intersection(point2(a1)?, point2(a2)?, point2(b1)?, point2(b2)?, tol)
            .map_err(status)?
            .map(solidcore::Point2::to_array),
    )
}

/// All segment-segment intersections between two 2D polylines within tolerance `tol`.
pub fn polyline_2d_crossings(a: &[[f64; 2]], b: &[[f64; 2]], tol: f64) -> Result<Vec<[f64; 2]>> {
    let pa: Vec<_> = a.iter().copied().map(point2).collect::<Result<Vec<_>>>()?;
    let pb: Vec<_> = b.iter().copied().map(point2).collect::<Result<Vec<_>>>()?;
    Ok(q::polyline_2d_crossings(&pa, &pb, tol)
        .map_err(status)?
        .into_iter()
        .map(solidcore::Point2::to_array)
        .collect())
}

/// Analytic 2D tangent contact point between segment `[a, b]` and circle `(center, radius)`.
pub fn tangent_contact_line_circle_2d(
    a: [f64; 2],
    b: [f64; 2],
    center: [f64; 2],
    radius: f64,
    tol: f64,
) -> Result<Option<[f64; 2]>> {
    Ok(
        q::tangent_contact_line_circle_2d(point2(a)?, point2(b)?, point2(center)?, radius, tol)
            .map_err(status)?
            .map(solidcore::Point2::to_array),
    )
}

/// Analytic 2D tangent contact point between two circles `(c1, r1)` and `(c2, r2)`.
pub fn tangent_contact_circle_circle_2d(
    c1: [f64; 2],
    r1: f64,
    c2: [f64; 2],
    r2: f64,
    tol: f64,
) -> Result<Option<[f64; 2]>> {
    Ok(
        q::tangent_contact_circle_circle_2d(point2(c1)?, r1, point2(c2)?, r2, tol)
            .map_err(status)?
            .map(solidcore::Point2::to_array),
    )
}

/// The largest distance from `p` to a point of the box `[lo, hi]`: to its farthest corner
/// (`query::api_queries::box_max_distance_to_point`).
///
/// # Errors
/// [`Error::Argument`] for a coordinate that is not finite.
pub fn box_max_distance_to_point(lo: [f64; 3], hi: [f64; 3], p: [f64; 3]) -> Result<f64> {
    q::box_max_distance_to_point(point(lo)?, point(hi)?, point(p)?).map_err(status)
}

/// Whether two points meet: their distance is within `tol` (the rounded distance of
/// [`point_distance`], not more than `tol`).
///
/// # Errors
/// Those of [`point_distance`].
pub fn points_meet(a: [f64; 3], b: [f64; 3], tol: f64) -> Result<bool> {
    Ok(point_distance(a, b)?.value <= tol)
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
        let request = MassPropertyRequest::centroidal();
        let r = read_else_make(
            "mass_properties",
            self,
            |k, b| {
                centroidal_properties(|attrs| {
                    brep_mass_properties_estimated_prepared(
                        k,
                        b,
                        relative_accuracy,
                        &request,
                        attrs,
                    )
                })
            },
            |k, b| {
                centroidal_properties(|attrs| {
                    brep_mass_properties_estimated(k, b, relative_accuracy, &request, attrs)
                })
            },
        )??;
        Ok(MassPropertiesEstimate {
            requested_accuracy: r.requested_accuracy,
            area: r.area,
            volume: r.volume,
            centroid: array(r.centroid),
            moments: array(r.inertia),
            products: array(r.products),
            tensor: r.inertia_tensor(),
        })
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
        let about = AreaAbout::Point(Point3::ZERO);
        let a = read_else_make(
            "area",
            self,
            |k, b| brep_area_about_prepared(k, b, relative_accuracy, about),
            |k, b| brep_area_about(k, b, relative_accuracy, about),
        )??;
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
        let x = read_body("bounds", self, |k, b| brep_bounding_box(k, b, true))?;
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
        let samples = read_body("edge_face_gap_samples", self, |k, b| {
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
        read_body("classify_point", self, |k, b| {
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
        read_body("classify_points", self, |k, b| {
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
        read_body("is_manifold_solid", self, |_, b| brep_is_manifold_solid(b))
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
        read_body("solid_count", self, |_, b| {
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
        let c = read_body("edge_closest_point", self, |k, b| {
            answer(q::edge_closest_point(k, b, e, p))
        })?;
        Ok(c.map(|c| EdgePoint {
            point: array(c.closest.point),
            parameter: c.parameter,
            distance: c.closest.distance,
        }))
    }

    /// The point of record edge `edge` nearest the ray from `origin` along `direction` (the
    /// kernel's `query::edge::closest_point_to_ray`), with the ray's point nearest it: a pick
    /// through the view lands on the edge drawn under the pointer in any view, where the edge
    /// point nearest one point of the ray ([`Body::edge_closest_point`]) does so only when that
    /// point is the nearest. `None` where the kernel's solve finds no point.
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the edges or a coordinate that is not finite; the
    /// kernel's refusal of a direction of no length; any other kernel error.
    pub fn edge_closest_to_ray(
        &self,
        order: &Order,
        edge: usize,
        origin: [f64; 3],
        direction: [f64; 3],
    ) -> Result<Option<EdgeRayPoint>> {
        let (e, o, d) = (order::edge(order, edge)?, point(origin)?, point(direction)?);
        let c = read_body("edge_closest_to_ray", self, |k, b| {
            solidcore::query::edge::closest_point_to_ray(k, b, e, o, d)
        })?;
        Ok(c.map(|c| EdgeRayPoint {
            point: array(c.point),
            parameter: c.parameter,
            on_ray: array(c.on_ray),
            distance: c.distance,
        }))
    }

    /// The first face the ray from `origin` along `direction` meets (the kernel's
    /// `query::brep::BrepQuery::first_face_hit`): a pick on a sheet or a solid lands on the face
    /// drawn under the pointer, at the point there, with the face's (u, v) at it. A sheet is hit
    /// from either side; `None` where the ray meets no face. The kernel's solve may add UV trim
    /// curves to the body.
    ///
    /// # Errors
    /// [`Error::Argument`] for a coordinate that is not finite or a direction of no length;
    /// [`Error::Exception`] for a hit face `order` does not list (an order of another body); any
    /// other kernel error.
    pub fn face_hit_by_ray(
        &self,
        order: &Order,
        origin: [f64; 3],
        direction: [f64; 3],
    ) -> Result<Option<FaceRayHit>> {
        let ray = Ray::new(point(origin)?, point(direction)?).map_err(|_| Error::Argument)?;
        let hit = with_body("face_hit_by_ray", self, |k, b| {
            BrepQuery::of_brep(b).first_face_hit(k, b, ray)
        })?;
        let Some(hit) = hit else {
            return Ok(None);
        };
        let face = order
            .faces
            .iter()
            .position(|&f| f == hit.face)
            .ok_or(Error::Exception)?;
        Ok(Some(FaceRayHit {
            face,
            point: array(hit.point),
            uv: [hit.uv.x, hit.uv.y],
            distance: hit.distance,
        }))
    }

    /// Every point where the ray from `origin` along `direction` meets a face, nearest first
    /// (the kernel's `query::brep::face_hits`): the face, the point and the distance. A face the
    /// ray crosses twice gives two hits; a ray through an edge meets each face there at one
    /// distance; a stretch along a face is no hit. Empty where the ray meets no face. The
    /// kernel's solves may add UV trim curves to the body.
    ///
    /// # Errors
    /// [`Error::Argument`] for a coordinate that is not finite or a direction of no length;
    /// [`Error::Exception`] for a hit face `order` does not list (an order of another body); any
    /// other kernel error.
    pub fn face_hits_by_ray(
        &self,
        order: &Order,
        origin: [f64; 3],
        direction: [f64; 3],
    ) -> Result<Vec<FaceHit>> {
        let ray = Ray::new(point(origin)?, point(direction)?).map_err(|_| Error::Argument)?;
        let hits = with_body("face_hits_by_ray", self, |k, b| {
            solidcore::query::brep::face_hits(k, b, ray)
        })?;
        hits.into_iter()
            .map(|hit| {
                let face = order
                    .faces
                    .iter()
                    .position(|&f| f == hit.face)
                    .ok_or(Error::Exception)?;
                Ok(FaceHit {
                    face,
                    point: array(hit.point),
                    distance: hit.distance,
                })
            })
            .collect()
    }

    /// Record edge `edge`'s curve over the edge's own interval, copied out of the body to measure
    /// against faces and edges of this or another body ([`Body::closest_to_curve`]).
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the edges; the kernel's refusal of an edge without
    /// a curve; any other kernel error.
    pub fn edge_curve(&self, order: &Order, edge: usize) -> Result<EdgeCurve> {
        let e = order::edge(order, edge)?;
        read_body("edge_curve", self, |k, b| {
            let (curve, interval) = q::edge_curve_copy(k, b, e)?;
            Ok(EdgeCurve { curve, interval })
        })
    }

    /// The closest points of `curve` (over its edge's interval) and record faces `faces` (as
    /// they are trimmed, their edges and vertices included) and record edges `edges` (over their
    /// intervals): the kernel's minimum-distance solve on the exact curves and surfaces
    /// (`query::api_queries::closest_to_members`), never on a tessellation. `None` where the
    /// kernel's solve answers with a status (no points found).
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the faces or edges; any other kernel error.
    pub fn closest_to_curve(
        &self,
        order: &Order,
        faces: &[usize],
        edges: &[usize],
        curve: &EdgeCurve,
    ) -> Result<Option<MembersClosest>> {
        let probe = q::Probe::Curve {
            curve: &curve.curve,
            interval: curve.interval,
        };
        self.closest_to(order, faces, edges, probe, "closest_to_curve")
    }

    /// The closest points of `p` and record faces `faces` and edges `edges`, as
    /// [`Body::closest_to_curve`] measures them; `from` is `p`.
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the faces or edges or a coordinate that is not
    /// finite; any other kernel error.
    pub fn closest_to_point(
        &self,
        order: &Order,
        faces: &[usize],
        edges: &[usize],
        p: [f64; 3],
    ) -> Result<Option<MembersClosest>> {
        let probe = q::Probe::Point(point(p)?);
        self.closest_to(order, faces, edges, probe, "closest_to_point")
    }

    /// The closest points between selected trimmed faces of two bodies. The kernel solves
    /// both faces' interiors, edges and vertices on their exact geometry. The bodies may be
    /// the same body; their geometry and record orders are left unchanged.
    ///
    /// # Errors
    /// [`Error::Argument`] for an empty selection or an index past the faces; the kernel's
    /// error when copying, solving or evaluating either witness fails.
    pub fn closest_faces(
        &self,
        order: &Order,
        faces: &[usize],
        other: &Body,
        other_order: &Order,
        other_faces: &[usize],
    ) -> Result<MembersClosest> {
        if faces.is_empty() || other_faces.is_empty() {
            return Err(Error::Argument);
        }
        let first = faces
            .iter()
            .map(|f| order::face(order, *f))
            .collect::<Result<Vec<_>>>()?;
        let second = other_faces
            .iter()
            .map(|f| order::face(other_order, *f))
            .collect::<Result<Vec<_>>>()?;
        if self.is_poisoned() || other.is_poisoned() {
            return Err(Error::Internal(
                "closest_faces: a body was left unfinished by an earlier panic".into(),
            ));
        }
        run_on(
            "closest_faces",
            &[("first", self), ("second", other)],
            |k| {
                // A single measurement-only B-rep lets the existing live face services handle
                // both selections, even when the original bodies overlap. Each copy has its own
                // topology identities; compose the maps rather than assuming matching indices.
                let (mut combined, first_map) = {
                    let b = self.lock();
                    brep_copy_mapped(k, &b)?
                };
                let (mut second_body, second_map) = {
                    let b = other.lock();
                    brep_copy_mapped(k, &b)?
                };
                let mapped = |map: &CopyBrepMap<EntityRef>, faces: &[FaceId]| {
                    copies_of(map, faces, EntityRef::Face, |e| match e {
                        EntityRef::Face(f) => Some(f),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        solidcore::SolidError::invalid_input(
                            "closest_faces",
                            "a selected face has no copy",
                        )
                    })
                };
                let first = mapped(&first_map, &first)?;
                let second = mapped(&second_map, &second)?;
                let serial = notify_serial(&mut k.context, &mut second_body)?;
                let source_key = move |e| topology_ref(serial, e);
                let mut merged_map = CopyBrepMap::default();
                with_kernel_callbacks(k, &mut combined, |k, b, env| {
                    merge_brep(
                        k,
                        env,
                        b,
                        &second_body,
                        &source_key,
                        None,
                        Some(&mut merged_map),
                        false,
                    )
                })?;
                let second = mapped(&merged_map, &second)?;
                combined.refresh_cached_topology();
                let c = q::closest_between_faces(
                    k,
                    &mut combined,
                    BrepId::from_index(0),
                    &first,
                    &second,
                )?;
                Ok(MembersClosest {
                    distance: c.distance,
                    from: array(c.on_probe),
                    to: array(c.on_members),
                })
            },
        )
    }

    fn closest_to(
        &self,
        order: &Order,
        faces: &[usize],
        edges: &[usize],
        probe: q::Probe<'_>,
        call: &'static str,
    ) -> Result<Option<MembersClosest>> {
        let faces = faces
            .iter()
            .map(|f| order::face(order, *f))
            .collect::<Result<Vec<_>>>()?;
        let edges = edges
            .iter()
            .map(|e| order::edge(order, *e))
            .collect::<Result<Vec<_>>>()?;
        let c = with_body(call, self, |k, b| {
            answer(q::closest_to_members(
                k,
                b,
                BrepId::from_index(0),
                &faces,
                &edges,
                probe,
            ))
        })?;
        Ok(c.map(|c| MembersClosest {
            distance: c.distance,
            from: array(c.on_probe),
            to: array(c.on_members),
        }))
    }

    /// Where the ray from `origin` along `direction` first meets this body, on its exact faces,
    /// edges and vertices (the kernel's ray fire, `topology::brep::p2::ray_fire`, at the body's
    /// own zone tolerance): the entity it meets, by record index, and the distance along the
    /// unit ray. `None` where the ray meets nothing. At a tie the lowest-dimensional entity is
    /// the answer, so a ray through an edge names the edge, not one of its faces. The kernel's
    /// solve may add UV trim curves to the body, as a point classification does.
    ///
    /// [`Body::face_hit_by_ray`] answers the first face alone, with the point and its (u, v);
    /// this names the edge or vertex a ray passes through, as a pick needs. Both take the ray
    /// as [`Ray::new`] makes it.
    ///
    /// # Errors
    /// [`Error::Argument`] for a coordinate that is not finite, a direction of no length (both
    /// as [`Ray::new`] refuses them) or a hit the conversion's [`Order`] does not hold; the
    /// kernel's refusal of a ray that starts on the body (its nearest hit at the start); any
    /// other kernel error.
    pub fn fire_ray(
        &self,
        order: &Order,
        origin: [f64; 3],
        direction: [f64; 3],
    ) -> Result<Option<RayHit>> {
        /// Where kernel entity `id` stands in the record's `list`.
        fn at<T: PartialEq>(list: &[T], id: &T) -> Result<usize> {
            list.iter().position(|x| x == id).ok_or(Error::Argument)
        }
        let ray = Ray::new(point(origin)?, point(direction)?).map_err(|_| Error::Argument)?;
        let hit = with_body("fire_ray", self, |k, b| {
            let tol = brep_zone_tol_3d(k, b, SmallTopoType::NormalSize);
            let query = BrepQuery::of_brep(b);
            let identity = query.effective_identity();
            let cache = query.cache(k, b)?;
            let fired = solidcore::topology::brep::p2::ray_fire(
                k,
                b,
                identity.brep_id,
                cache,
                identity.keys,
                identity.owner_of,
                ray.origin(),
                ray.direction(),
                tol,
                None,
            )?;
            let pc = &fired.intersection;
            Ok(match (pc.point_class(), pc.object()) {
                (PointClassType::Unknown, _) | (_, None) => None,
                (_, Some(entity)) => Some((entity, pc.deviation())),
            })
        })?;
        let Some((entity, distance)) = hit else {
            return Ok(None);
        };
        let entity = match entity {
            EntityRef::Face(f) => RayEntity::Face(at(&order.faces, &f)?),
            EntityRef::Edge(e) => RayEntity::Edge(at(&order.edges, &e)?),
            EntityRef::Vertex(v) => RayEntity::Vertex(at(&order.vertices, &v)?),
            _ => return Err(Error::Argument),
        };
        Ok(Some(RayHit { entity, distance }))
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

    /// The distance from each of `points` to record face `face`'s surface untrimmed, where the
    /// point lies over it: the kernel's global solve for the surface's nearest point, a periodic
    /// seam wrapped, the surface by its definition (a B-spline over its domain, a plane,
    /// cylinder, cone, sphere or torus continued as that analytic surface;
    /// `query::face_surface_feet`). `None` for a point beside the surface, whose nearest point
    /// is held on a side of its domain. Where
    /// [`Body::face_distances`] reads a point past the face's edge as its distance to that edge,
    /// this reads its distance to the surface the face lies on.
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the faces or a coordinate that is not finite; any
    /// other kernel error.
    pub fn face_surface_gaps(
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
        read_body("face_surface_gaps", self, |k, b| {
            let feet = solidcore::query::face_surface_feet(k, b, f, &points)?;
            Ok(feet.into_iter().map(|f| f.map(|f| f.distance)).collect())
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

    /// The rim of record faces `faces`: the record edges with a face of the set on one side only,
    /// in record order. A seam (its face on both sides) and an edge between two faces of the set
    /// are not on it, whether the faces meet smoothly there or not. Empty when the faces close up
    /// on their own, as a whole sphere's do. [`Body::outline_cuts`] cuts with this rim.
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the faces or a face with no side that faces out;
    /// [`Error::Unsupported`] for a face with material on both sides; any other kernel error.
    pub fn rim_edges(&self, order: &Order, faces: &[usize]) -> Result<Vec<usize>> {
        let set = faces
            .iter()
            .map(|&i| order::face(order, i))
            .collect::<Result<Vec<_>>>()?;
        read_body("rim_edges", self, |_, b| {
            let rims = match rim_edges(b, &set)? {
                Ok(rims) => rims,
                Err(e) => return Ok(Err(e)),
            };
            let indices = (0..order.edges.len())
                .filter(|&i| order.edges.get(i).is_some_and(|e| rims.contains(e)));
            Ok(Ok(indices.collect()))
        })?
    }

    /// Where the rim of record faces `faces` cuts `outline`, a curve over `interval` (the record's
    /// parameter) that lies in the plane through `point` with normal `normal`: the outline's
    /// parameters, in order. The rim is [`Body::rim_edges`]: the edges with a face of the set on
    /// one side only, so not a seam or an edge between two faces of the set, smooth or not. A
    /// crossing of a rim edge with the plane (`edge_plane_crossings`; a stretch gives its two ends)
    /// is a cut when the kernel's closest point of the outline to it is within the edge's tolerance
    /// and inside `interval`; a crossing at a vertex already cut is not cut again. So an outline
    /// that is one line of the plane keeps only the crossings on that line, and two edges meeting
    /// at a vertex on it cut it once.
    ///
    /// A crossing inside an edge is cut where the outline meets the surface of the face across the
    /// edge (`edge_crossing_on_face`): the edge's curve holds the rim only to its tolerance (a
    /// traced curve's), the face's surface exactly. So the cut is exact when the outline lies on
    /// the set's own surface, as a contour lies on a sphere and a silhouette on a cone. A crossing
    /// at a vertex is the vertex's, and a stretch's ends are where the edge leaves the plane within
    /// its tolerance.
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the faces, a face with no side that faces out, a
    /// coordinate that is not finite or an outline the kernel can't hold; [`Error::Unsupported`]
    /// for a rim edge without exactly one face across it (a sheet's free edge, an edge of three
    /// faces); the kernel's refusal of a normal of no direction or an interval that doesn't
    /// increase, and of a cut its solve doesn't reach within the edge's tolerance
    /// ([`Error::Status`]); any other kernel error.
    pub fn outline_cuts(
        &self,
        order: &Order,
        faces: &[usize],
        outline: &solidcore_record::brep::Curve,
        interval: [f64; 2],
        point_on: [f64; 3],
        normal: [f64; 3],
    ) -> Result<Vec<f64>> {
        let set = faces
            .iter()
            .map(|&i| order::face(order, i))
            .collect::<Result<Vec<_>>>()?;
        let (on, normal) = (point(point_on)?, point(normal)?);
        let record = solidcore_convert::curve(outline).map_err(|_| Error::Argument)?;
        read_body("outline_cuts", self, |k, b| {
            let made = from_parameters::from_parameters(k, &record, interval)?;
            let rims = match rim_edges(b, &set)? {
                Ok(rims) => rims,
                Err(e) => return Ok(Err(e)),
            };
            let (mut cuts, mut cut_vertices) = (Vec::new(), Vec::new());
            for e in rims {
                let tolerance = b.edge(e)?.tolerance;
                for crossing in q::edge_plane_crossings(k, b, e, on, normal)? {
                    let (ends, stretch) = match crossing {
                        q::PlaneCrossing::Point(p) => (vec![p], false),
                        q::PlaneCrossing::Along { start, end } => (vec![start, end], true),
                    };
                    for end in ends {
                        if end.vertex.is_some_and(|v| cut_vertices.contains(&v)) {
                            continue;
                        }
                        // a cut only within the edge's tolerance of the outline (written to
                        // accept: a distance that isn't a number is no cut)
                        let near = q::curve_closest_point(k, &made.curve, end.point)?;
                        let within = near.closest.distance <= tolerance;
                        if !within {
                            continue;
                        }
                        // Inside the edge, its curve holds the rim only to its tolerance: the cut
                        // is where the outline meets the face across the edge. A vertex is its own
                        // point, and a stretch ends where the edge leaves the plane's tolerance.
                        let at = if stretch || end.vertex.is_some() {
                            near.parameter
                        } else {
                            let face = match across(b, e, &set)? {
                                Ok(face) => face,
                                Err(refusal) => return Ok(Err(refusal)),
                            };
                            q::edge_crossing_on_face(
                                k,
                                b,
                                e,
                                face,
                                &made.curve,
                                &made.interval,
                                near.parameter,
                            )?
                        };
                        let t = from_parameters::record_parameter(
                            &record,
                            interval,
                            at,
                            made.curve.evaluate_point(k, at)?,
                        );
                        // on its interval as the kernel classifies it: a cut a solve rounds just
                        // past an end is on that end
                        if let Some(t) = from_parameters::on_interval(interval, t) {
                            cuts.push(t);
                            cut_vertices.extend(end.vertex);
                        }
                    }
                }
            }
            cuts.sort_by(f64::total_cmp);
            Ok(Ok(cuts))
        })?
    }

    /// The distance from record vertex `vertex`'s point to `p`, with its proved rounding bound.
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the vertices or a coordinate that is not finite; the
    /// kernel's refusal of a distance that overflows.
    pub fn vertex_distance(&self, order: &Order, vertex: usize, p: [f64; 3]) -> Result<Distance> {
        let (v, p) = (order::vertex(order, vertex)?, point(p)?);
        read_body("vertex_distance", self, |_, b| {
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

    /// The outward unit normal at `p` on trimmed record face `face`, within the body's
    /// modelling tolerance, or `None` off that face. Uses the same tolerance as
    /// [`Body::classify_points`] and the material-side sense of [`Body::face_normal_at_edge`].
    ///
    /// # Errors
    /// [`Error::Argument`] for an invalid face index or non-finite point; those of
    /// [`Body::face_normal_at_edge`] for an undefined normal or material side.
    pub fn face_normal_at_point(
        &self,
        order: &Order,
        face: usize,
        p: [f64; 3],
    ) -> Result<Option<Normal>> {
        let (f, p) = (order::face(order, face)?, point(p)?);
        with_body("face_normal_at_point", self, |k, b| {
            let tol = brep_zone_tol_3d(k, b, SmallTopoType::NormalSize);
            Ok(face_normal_in_body(k, b, f, p, tol))
        })?
    }

    /// Classify all `points` and resolve boundary faces and outward normals in one
    /// rollback scope. A single mutable B-rep query classifies the whole batch without
    /// another body copy. Face boxes are built once and exclude distant faces before
    /// exact trimmed-face membership is tested, in record order.
    ///
    /// `face` selects one normal's face for all boundary points; without it the first
    /// containing face is chosen per point. Classes remain relative to the whole body.
    ///
    /// # Errors
    /// [`Error::Argument`] for non-finite coordinates, an invalid face index, or a
    /// point that has no containing selected face; [`Error::Unsupported`] when
    /// a point lies on a material/material partition (never merely when a candidate
    /// face is a partition); those of classification, face boxes and
    /// [`Body::face_normal_at_point`].
    pub fn classify_points_with_normals(
        &self,
        order: &Order,
        points: &[[f64; 3]],
        face: Option<PointNormalFace>,
    ) -> Result<Vec<ClassifiedPoint>> {
        for &p in points {
            point(p)?;
        }
        match face {
            Some(PointNormalFace::Index(i)) => {
                order::face(order, i)?;
            }
            Some(PointNormalFace::Point(p)) => {
                point(p)?;
            }
            None => {}
        }
        with_body("classify_points_with_normals", self, |k, b| {
            Ok(classify_with_normals(k, b, order, points, face))
        })?
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
        let a = read_else_make(
            "face_area",
            self,
            |k, b| face_area_prepared(k, b, f, relative_accuracy),
            |k, b| face_area(k, b, f, relative_accuracy),
        )??;
        if !a.is_finite() {
            return Err(Error::Status(crate::error::INVALID_INPUT));
        }
        Ok(a)
    }

    /// The point of record face `face`'s surface nearest `p`, when `p` lies on that face itself
    /// within `tolerance` (on its trimmed area, an edge or a vertex of it), else `None`: the
    /// kernel drops `p` onto the surface and classifies it against the face's own trim loops
    /// (`query::face::p1::point_on_face`), so a point on another face of the same surface is
    /// not on this one.
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the faces, a coordinate that is not finite or a
    /// tolerance that is not finite and positive; the kernel's error.
    pub fn point_on_face(
        &self,
        order: &Order,
        face: usize,
        p: [f64; 3],
        tolerance: f64,
    ) -> Result<Option<[f64; 3]>> {
        let f = order::face(order, face)?;
        let p = point(p)?;
        if !(tolerance.is_finite() && tolerance > 0.0) {
            return Err(Error::Argument);
        }
        with_body("point_on_face", self, |k, b| {
            face_p1::point_on_face(k, b, f, p, tolerance).map(|n| Ok(n.map(|n| [n.x, n.y, n.z])))
        })?
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
        let (area, centroid) = read_else_make(
            "face_centroid",
            self,
            |k, b| face_centroid_prepared(k, b, f, relative_accuracy),
            |k, b| face_centroid(k, b, f, relative_accuracy),
        )??;
        let c = FaceCentroid {
            area,
            centroid: array(centroid),
        };
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

    /// Where record face `face` lies along `direction`, when it is a plane square to it: the
    /// signed distance from `origin` to the face's plane measured along `direction` (a point of
    /// the face, its centroid, dotted with the unit `direction`), and the face's own tolerance
    /// (`SmFace::GetTolerance`), the distance within which the kernel takes a point for being on
    /// the face. `None` for a face that is not planar ([`Body::face_plane_normal`]) or whose
    /// normal is not parallel to `direction`: the normal's cross with the unit `direction` is
    /// longer than the normal's proved bound plus the kernel's model tolerance
    /// (`NL_MTOL`, `nurbs::math::MODEL_TOL`), the tilt below which the kernel takes two
    /// directions for the same.
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the faces, a `direction` or `origin` that is not
    /// finite, or input the kernel's query refuses (a `direction` of no length); the errors of
    /// [`Body::face_plane_normal`] and [`Body::face_centroid`].
    pub fn face_square_level(
        &self,
        order: &Order,
        face: usize,
        direction: [f64; 3],
        origin: [f64; 3],
    ) -> Result<Option<SquareFaceLevel>> {
        let f = order::face(order, face)?;
        let direction = point(direction)?;
        let origin = point(origin)?;
        let Some(n) = self.face_plane_normal(order, face)? else {
            return Ok(None);
        };
        let on_face = point(self.face_centroid(order, face, 1e-8)?.centroid)?;
        let Some(level) =
            q::face_square_level(point(n.value)?, n.bound, on_face, direction, origin).map_err(
                |e| match e {
                    solidcore::SolidError::InvalidInput { .. } => Error::Argument,
                    e => status(e),
                },
            )?
        else {
            return Ok(None);
        };
        let tolerance = read_body("face_tolerance", self, |_k, b| {
            Ok(Ok::<f64, Error>(face::tolerance(b, f)?))
        })??;
        Ok(Some(SquareFaceLevel { level, tolerance }))
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
        let l = read_body("edge_length", self, |k, b| {
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
        let p = read_body("edge_middle", self, |k, b| {
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

    /// `count` points of record edge `edge`'s curve inside the edge, never at its ends: the
    /// kernel's evaluation of the curve at the fractions k / (count + 1), k = 1 … count, of the
    /// edge's parameter interval. They are spaced evenly in parameter, not in length: on a
    /// line they are evenly spaced along it, on a rational arc or a spline they need not be. One
    /// point is [`Body::edge_middle`]'s. For a caller that checks what happened along an edge,
    /// not only at its middle: a blend that runs on past the end of its edge takes part of the
    /// next one. The body is only read, never copied for a rollback.
    ///
    /// # Errors
    /// [`Error::Argument`] for a `count` of 0 or past [`EDGE_POINTS_MAX`]; otherwise as
    /// [`Body::edge_middle`].
    pub fn edge_points(&self, order: &Order, edge: usize, count: usize) -> Result<Vec<[f64; 3]>> {
        let n = match u32::try_from(count) {
            Ok(n) if (1..=EDGE_POINTS_MAX).contains(&n) => n,
            _ => return Err(Error::Argument),
        };
        let e = order::edge(order, edge)?;
        let points = read_body("edge_points", self, |k, b| {
            let interval = edge::interval(b, e)?;
            let curve = edge::curve(b, e)?
                .ok_or_else(|| solidcore::SolidError::null_pointer("ops::edge_points"))?;
            let (lo, hi) = (interval.min(), interval.max());
            (1..=n)
                .map(|i| curve.evaluate_point(k, inside(lo, hi, i, n)).map(array))
                .collect::<solidcore::Result<Vec<_>>>()
        })?;
        if !points.iter().flatten().all(|x| x.is_finite()) {
            return Err(Error::Status(crate::error::INVALID_INPUT));
        }
        Ok(points)
    }
}

/// The most points [`Body::edge_points`] gives along one edge.
pub const EDGE_POINTS_MAX: u32 = 64;

/// The parameter `i / (n + 1)` of the way along `[lo, hi]`: the `i`-th of `n` sample positions
/// ops chooses inside the interval, as [`middle`] is the one position for `n` = 1.
#[allow(
    clippy::float_arithmetic,
    reason = "a sample parameter ops chooses, not a geometric quantity (docs/layers.md)"
)]
fn inside(lo: f64, hi: f64, i: u32, n: u32) -> f64 {
    lo + (hi - lo) * (f64::from(i) / (f64::from(n) + 1.0))
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
/// The rim of the faces `set` ([`Body::outline_cuts`]): each of their edges that lies once on the
/// set's outward uses ([`outer_faceuse`]), so with a face of the set on one side only, in the
/// faces' edge order. A seam lies twice on its face's use and an edge between two faces of the
/// set once on each, so neither is a rim; whether the faces meet smoothly there does not matter.
///
/// # Errors
/// [`Error::Argument`] or [`Error::Unsupported`] (inner) for a face with no side that faces out;
/// the kernel's errors (outer).
fn rim_edges(b: &Brep, set: &[FaceId]) -> solidcore::Result<Result<Vec<EdgeId>>> {
    let mut outward = Vec::with_capacity(set.len());
    for &f in set {
        match outer_faceuse(b, f)? {
            Ok(fu) => outward.push(fu),
            Err(e) => return Ok(Err(e)),
        }
    }
    let (mut seen, mut rims) = (Vec::new(), Vec::new());
    for &f in set {
        for e in face::edges(b, f, None, false, None)? {
            if seen.contains(&e) {
                continue;
            }
            seen.push(e);
            let mut on_set = 0;
            for u in edge::edgeuses(b, e, None, None)? {
                if eu1::faceuse(b, u)?.is_some_and(|fu| outward.contains(&fu)) {
                    on_set += 1;
                }
            }
            if on_set == 1 {
                rims.push(e);
            }
        }
    }
    Ok(Ok(rims))
}

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

/// Membership precedes material-side selection: an unrelated partition has no normal
/// to request and must not refuse a query on another face.
fn face_normal_in_body(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    p: Point3,
    tol: f64,
) -> Result<Option<Normal>> {
    let Some(uv) = q::face_parameters_at_point(k, b, f, p, tol).map_err(status)? else {
        return Ok(None);
    };
    let fu = outer_faceuse(b, f).map_err(status)??;
    let n = q::face_normal_at_uv(k, b, f, uv, fu).map_err(status)?;
    Ok(Some(Normal {
        value: array(n.value),
        bound: n.bound,
    }))
}

/// First exact containing face, after rejecting points outside the faces' kernel boxes.
fn find_point_normal(
    k: &Kernel,
    b: &mut Brep,
    candidates: &q::FacePointCandidates,
    p: Point3,
    tol: f64,
    selected: Option<usize>,
) -> Result<Option<(usize, Normal)>> {
    for (index, f) in candidates.at_point(p, tol, selected).map_err(status)? {
        if let Some(normal) = face_normal_in_body(k, b, f, p, tol)? {
            return Ok(Some((index, normal)));
        }
    }
    Ok(None)
}

/// The whole classification workflow, on the one body already protected by `with_body`.
fn classify_with_normals(
    k: &Kernel,
    b: &mut Brep,
    order: &Order,
    points: &[[f64; 3]],
    face: Option<PointNormalFace>,
) -> Result<Vec<ClassifiedPoint>> {
    let tol = brep_zone_tol_3d(k, b, SmallTopoType::NormalSize);
    let query = BrepQuery::of_brep(b);
    let mut classes = Vec::new();
    for &p in points {
        let pc = query
            .point_3d_classify(k, b, point(p)?, tol, true)
            .map_err(status)?;
        classes.push(point_class(located(b, &pc))?);
    }
    let needs_faces = matches!(face, Some(PointNormalFace::Point(_)))
        || classes.contains(&PointClass::OnBoundary);
    let candidates = if needs_faces {
        let selected = match face {
            Some(PointNormalFace::Index(i)) => Some(i),
            _ => None,
        };
        q::FacePointCandidates::new(k, b, &order.faces, selected).map_err(status)?
    } else {
        q::FacePointCandidates::default()
    };
    let selected = match face {
        Some(PointNormalFace::Index(i)) => Some(i),
        Some(PointNormalFace::Point(p)) => Some(
            find_point_normal(k, b, &candidates, point(p)?, tol, None)?
                .ok_or(Error::Argument)?
                .0,
        ),
        None => None,
    };
    points
        .iter()
        .zip(classes)
        .map(|(&p, class)| match class {
            PointClass::Inside => Ok(ClassifiedPoint::Inside),
            PointClass::Outside => Ok(ClassifiedPoint::Outside),
            PointClass::OnBoundary => {
                let (face, normal) =
                    find_point_normal(k, b, &candidates, point(p)?, tol, selected)?
                        .ok_or(Error::Argument)?;
                Ok(ClassifiedPoint::On { face, normal })
            }
        })
        .collect()
}

/// The face across rim edge `e` from the faces `set` ([`Body::outline_cuts`]): the one face of
/// the edge's uses that is not in the set.
///
/// # Errors
/// [`Error::Unsupported`] (inner) for an edge with no face across it (a sheet's free edge) or
/// more than one (an edge of three faces or more); the kernel's errors (outer).
fn across(b: &Brep, e: EdgeId, set: &[FaceId]) -> solidcore::Result<Result<FaceId>> {
    let mut faces = Vec::new();
    for u in edge::edgeuses(b, e, None, None)? {
        let face = match eu1::faceuse(b, u)? {
            Some(fu) => b.faceuse(fu)?.face,
            None => None,
        };
        if let Some(f) = face.filter(|f| !set.contains(f) && !faces.contains(f)) {
            faces.push(f);
        }
    }
    Ok(match faces.as_slice() {
        [f] => Ok(*f),
        _ => Err(Error::Unsupported(
            "a rim edge without exactly one face across it",
        )),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{make_box, session};
    use solidcore::surface::surface::SurfaceKind;

    fn all_fixes() {
        session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    }

    /// The selected walls of nested cylinders are 35 - 20 = 15 apart, even though their
    /// solids overlap. Preparing their common measurement B-rep must preserve both orders.
    #[test]
    fn closest_faces_of_nested_cylinders_are_their_radii_apart() {
        all_fixes();
        let a = crate::make_cylinder([0.0, 0.0, 0.0], 20.0, 5.0).unwrap();
        let b = crate::make_cylinder([0.0, 0.0, 0.0], 35.0, 5.0).unwrap();
        let (ao, bo) = (kernel_order(&a), kernel_order(&b));
        let af = faces_on(&a, &ao, |s| matches!(s, SurfaceKind::Cone(_)));
        let bf = faces_on(&b, &bo, |s| matches!(s, SurfaceKind::Cone(_)));
        assert!(!af.is_empty() && !bf.is_empty());
        for (a, ao, af, b, bo, bf, ra, rb) in [
            (&a, &ao, &af, &b, &bo, &bf, 20.0, 35.0),
            (&b, &bo, &bf, &a, &ao, &af, 35.0, 20.0),
        ] {
            let c = a.closest_faces(ao, af, b, bo, bf).unwrap();
            assert!((c.distance - 15.0).abs() <= 1e-9, "{c:?}");
            for (p, r) in [(c.from, ra), (c.to, rb)] {
                assert!(
                    (p[0] * p[0] + p[1] * p[1] - r * r).abs() <= 1e-9
                        && p[2] >= -1e-9
                        && p[2] <= 5.0 + 1e-9,
                    "{c:?}"
                );
            }
        }
        assert_eq!(
            a.closest_faces(&ao, &[], &b, &bo, &bf),
            Err(Error::Argument)
        );
        assert_eq!(
            a.closest_faces(&ao, &[usize::MAX], &b, &bo, &bf),
            Err(Error::Argument)
        );
        assert_eq!(kernel_order(&a).faces, ao.faces);
        assert_eq!(kernel_order(&b).faces, bo.faces);
    }

    /// Sphere centres (0, 0, 0) and (12, 16, 0) are 20 apart. Radii 5 and 8 leave
    /// a gap of 7, at (3, 4, 0) and (7.2, 9.6, 0), inside the surfaces, away from the seams.
    #[test]
    fn closest_faces_include_surface_interior_minima() {
        all_fixes();
        let a = crate::make_sphere([0.0, 0.0, 0.0], 5.0).unwrap();
        let b = crate::make_sphere([12.0, 16.0, 0.0], 8.0).unwrap();
        let (ao, bo) = (kernel_order(&a), kernel_order(&b));
        let af = faces_on(&a, &ao, |s| matches!(s, SurfaceKind::Sphere(_)));
        let bf = faces_on(&b, &bo, |s| matches!(s, SurfaceKind::Sphere(_)));
        let c = a.closest_faces(&ao, &af, &b, &bo, &bf).unwrap();
        assert!((c.distance - 7.0).abs() <= 1e-9, "{c:?}");
        assert!(
            close(&c.from, &[3.0, 4.0, 0.0]) && close(&c.to, &[7.2, 9.6, 0.0]),
            "{c:?}"
        );
    }

    /// Separated unit boxes: the selected top at z = 1 and bottom at z = 4 come nearest
    /// at corners (1, 1, 1) and (3, 4, 4), sqrt(2² + 3² + 3²) = sqrt(22) apart.
    #[test]
    fn closest_faces_keep_their_trimmed_boundaries() {
        all_fixes();
        let a = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
        let b = make_box([3.0, 4.0, 4.0], [1.0, 1.0, 1.0]).unwrap();
        let (ao, bo) = (kernel_order(&a), kernel_order(&b));
        let af = faces_at(&a, &ao, |p| (p[2] - 1.0).abs() < 1e-9);
        let bf = faces_at(&b, &bo, |p| (p[2] - 4.0).abs() < 1e-9);
        let c = a.closest_faces(&ao, &af, &b, &bo, &bf).unwrap();
        assert!((c.distance * c.distance - 22.0).abs() <= 1e-9, "{c:?}");
        assert_eq!(c.from, [1.0, 1.0, 1.0]);
        assert_eq!(c.to, [3.0, 4.0, 4.0]);
        let c = a.closest_faces(&ao, &af, &a, &ao, &af).unwrap();
        assert!(c.distance <= 1e-9, "{c:?}");
    }

    /// The order of a body made in the kernel with no record between: the kernel's own lists.
    pub(crate) fn kernel_order(b: &Body) -> Order {
        let k = b.lock();
        Order {
            split_edges: 0,
            faces: k.face_list().to_vec(),
            edges: k.edge_list().to_vec(),
            vertices: k.vertex_list().to_vec(),
        }
    }

    #[test]
    fn centroid_and_mean_radius_and_displacement_adapt_kernel_results() {
        let summary = centroid_and_mean_radius(&[[0.0, 0.0], [6.0, 0.0], [0.0, 6.0]]).unwrap();
        assert_eq!(summary.centroid, [2.0, 2.0]);
        // (2√2 + 4√5) / 3, evaluated independently.
        let expected = 3.924_233_011_581_783;
        assert!((summary.mean_distance - expected).abs() <= 2.0 * f64::EPSILON * expected);
        assert_eq!(
            point_displacement([2.0, 2.0], [5.0, 6.0]).unwrap(),
            [3.0, 4.0]
        );
        assert_eq!(
            centroid_and_mean_radius(&[[f64::NAN, 0.0]]),
            Err(Error::Argument)
        );
        assert_eq!(
            point_displacement([0.0, 0.0], [f64::INFINITY, 0.0]),
            Err(Error::Argument)
        );
        assert!(centroid_and_mean_radius(&[]).is_err());
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
        assert!(points_meet([1.0, 2.0, 3.0], [4.0, 6.0, 15.0], 13.0).unwrap());
        assert!(!points_meet([1.0, 2.0, 3.0], [4.0, 6.0, 15.0], 12.9).unwrap());
        assert!(points_meet([f64::NAN, 0.0, 0.0], [0.0; 3], 1.0).is_err());
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

    /// The cavity wall at x=3 faces +X into void; the outside wall at x=0 faces -X.
    /// This is independent of the surfaces' parameterisation and record face order.
    #[test]
    fn point_normals_face_out_of_material_in_record_order() {
        all_fixes();
        for turned in [false, true] {
            let b = cavity(turned);
            let mut order = kernel_order(&b);
            order.faces.reverse();
            for (p, want) in [([3., 5., 5.], [1., 0., 0.]), ([0., 5., 5.], [-1., 0., 0.])] {
                let normals: Vec<_> = (0..order.faces.len())
                    .filter_map(|f| b.face_normal_at_point(&order, f, p).unwrap())
                    .collect();
                assert_eq!(normals.len(), 1);
                for n in normals {
                    for (value, expected) in n.value.into_iter().zip(want) {
                        assert!((value - expected).abs() < 1e-12, "{n:?}");
                    }
                }
            }
            assert!(b
                .face_normal_at_point(&order, usize::MAX, [0., 0., 0.])
                .is_err());
            assert!(b
                .face_normal_at_point(&order, 0, [f64::NAN, 0., 0.])
                .is_err());
        }
    }

    /// A merged inner cube leaves six material/material partitions. Put them first in
    /// record order: off-partition points must still reach the two outer x faces,
    /// while a point actually on a partition still has no outward normal.
    #[test]
    fn batched_point_normals_skip_unrelated_partitions() {
        all_fixes();
        let b = make_box([0., 0., 0.], [10., 10., 10.]).unwrap();
        let inner = make_box([3., 3., 3.], [4., 4., 4.]).unwrap();
        session::run("merge", |k| {
            let mut g = b.lock();
            let outer = std::mem::replace(&mut *g, Brep::new());
            *g = solidcore::boolean::api_brep::boolean_merge(k, outer, inner.into_inner())?;
            Ok(())
        })
        .unwrap();
        let mut order = kernel_order(&b);
        let partitions: Vec<_> = {
            let g = b.lock();
            order
                .faces
                .iter()
                .copied()
                .filter(|&f| matches!(outer_faceuse(&g, f), Ok(Err(Error::Unsupported(_)))))
                .collect()
        };
        assert_eq!(partitions.len(), 6);
        order.faces.sort_by_key(|f| !partitions.contains(f));
        let mut refused = 0;
        for i in 0..6 {
            assert_eq!(
                b.face_normal_at_point(&order, i, [0., 5., 5.]).unwrap(),
                None
            );
            match b.face_normal_at_point(&order, i, [3., 5., 5.]) {
                Ok(None) => {}
                Err(Error::Unsupported(_)) => refused += 1,
                other => panic!("unexpected partition normal: {other:?}"),
            }
        }
        assert_eq!(refused, 1, "only the x=3 partition contains this point");

        let points = [[0., 5., 5.], [10., 5., 5.], [1., 1., 1.], [20., 5., 5.]];
        let results = b
            .classify_points_with_normals(&order, &points, None)
            .unwrap();
        #[cfg(feature = "profile")]
        {
            let many = vec![points[0]; 256];
            let (batch, calls) =
                session::profiled(|| b.classify_points_with_normals(&order, &many, None).unwrap());
            assert_eq!(batch.len(), many.len());
            assert_eq!(
                calls.iter().map(|c| c.call).collect::<Vec<_>>(),
                ["classify_points_with_normals"]
            );
        }
        assert_eq!(results.len(), points.len());
        for (result, want) in results.iter().zip([[-1., 0., 0.], [1., 0., 0.]]) {
            let ClassifiedPoint::On { face, normal } = result else {
                panic!("expected an exterior face: {result:?}");
            };
            assert!(*face >= 6, "the first six faces are internal partitions");
            for (value, expected) in normal.value.into_iter().zip(want) {
                assert!((value - expected).abs() < 1e-12, "{normal:?}");
            }
        }
        assert_eq!(results[2], ClassifiedPoint::Inside);
        assert_eq!(results[3], ClassifiedPoint::Outside);
        let ClassifiedPoint::On { face, .. } = results[0] else {
            panic!("the x=0 point is on the exterior");
        };
        for selected in [
            PointNormalFace::Index(face),
            PointNormalFace::Point([0., 4., 5.]),
        ] {
            assert_eq!(
                b.classify_points_with_normals(&order, &points[..1], Some(selected))
                    .unwrap(),
                results[..1]
            );
        }
        assert!(matches!(
            b.classify_points_with_normals(&order, &[[3., 5., 5.]], None),
            Err(Error::Unsupported(_))
        ));
        assert_eq!(
            b.classify_points_with_normals(&order, &points[..1], Some(PointNormalFace::Index(0))),
            Err(Error::Argument)
        );
        assert_eq!(
            b.classify_points_with_normals(&order, &[[f64::NAN, 0., 0.]], None),
            Err(Error::Argument)
        );
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

    /// A box's faces square to +Z: the top and the bottom, at their closed-form levels measured
    /// from an origin off the box; the four sides are not square to it, and nor is a direction
    /// tilted past the model tolerance; a sphere has no planar face.
    #[test]
    fn a_box_face_square_to_a_direction_is_found_at_its_level() {
        all_fixes();
        let b = make_box([0.0, 0.0, 1.0], [2.0, 3.0, 4.0]).unwrap();
        let o = kernel_order(&b);
        let mut levels = Vec::new();
        for f in 0..o.faces.len() {
            if let Some(s) = b
                .face_square_level(&o, f, [0.0, 0.0, 2.0], [5.0, 5.0, 0.5])
                .unwrap()
            {
                assert!(s.tolerance > 0.0 && s.tolerance < 1e-3, "{s:?}");
                levels.push(s.level);
            }
        }
        levels.sort_by(f64::total_cmp);
        assert_eq!(levels.len(), 2, "{levels:?}");
        assert!(
            (levels[0] - 0.5).abs() < 1e-9 && (levels[1] - 4.5).abs() < 1e-9,
            "{levels:?}"
        );
        for f in 0..o.faces.len() {
            assert_eq!(
                b.face_square_level(&o, f, [0.0, 0.01, 1.0], [0.0; 3])
                    .unwrap(),
                None
            );
        }
        let ball = crate::make_sphere([0.0, 0.0, 0.0], 3.0).unwrap();
        let ob = kernel_order(&ball);
        assert_eq!(
            ball.face_square_level(&ob, 0, [0.0, 0.0, 1.0], [0.0; 3])
                .unwrap(),
            None
        );
        assert!(matches!(
            b.face_square_level(&o, 0, [0.0; 3], [0.0; 3]),
            Err(Error::Argument)
        ));
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

    /// The faces of `b` (in its kernel order `o`) whose surface `is` picks.
    fn faces_on(b: &Body, o: &Order, is: fn(&SurfaceKind) -> bool) -> Vec<usize> {
        let brep = b.lock();
        (0..o.faces.len())
            .filter(|&i| is(face::surface(&brep, o.faces[i]).unwrap().unwrap().kind()))
            .collect()
    }

    /// The faces of `b` (in its kernel order `o`) whose area centroid `at` picks.
    fn faces_at(b: &Body, o: &Order, at: impl Fn([f64; 3]) -> bool) -> Vec<usize> {
        (0..o.faces.len())
            .filter(|&i| at(b.face_centroid(o, i, 1e-8).unwrap().centroid))
            .collect()
    }

    fn close(cuts: &[f64], want: &[f64]) -> bool {
        cuts.len() == want.len() && cuts.iter().zip(want).all(|(a, b)| (a - b).abs() <= 1e-9)
    }

    #[test]
    fn outline_cuts_of_a_sphere_with_a_hole_are_the_contour_s_closed_form_angles() {
        // a sphere of radius 5 less a cylinder of radius 1 about x = 3, y = 0: seen along y, its
        // contour is the circle x^2 + z^2 = 25 in y = 0, which the hole's rims cross where
        // x = 2 (z = +-sqrt 21) and x = 4 (z = +-3). The face set is the sphere's: its rim is the
        // hole's two rims; its seam, which lies on the contour, is on the sphere on both sides
        all_fixes();
        let sphere = crate::make_sphere([0.0, 0.0, 0.0], 5.0).unwrap();
        let hole = crate::make_cylinder([3.0, 0.0, -10.0], 1.0, 20.0).unwrap();
        let b = crate::boolean::boolean(sphere, hole, crate::BooleanOp::Subtract)
            .unwrap()
            .unwrap();
        let o = kernel_order(&b);
        let contour = solidcore_record::brep::Curve::Circle {
            frame: solidcore_record::brep::Frame {
                origin: [0.0, 0.0, 0.0],
                x_axis: [1.0, 0.0, 0.0],
                y_axis: [0.0, 0.0, 1.0],
            },
            radius: 5.0,
        };
        let turn = std::f64::consts::TAU;
        let cuts = b
            .outline_cuts(
                &o,
                &faces_on(&b, &o, |s| matches!(s, SurfaceKind::Sphere(_))),
                &contour,
                [0.0, turn],
                [0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
            )
            .unwrap();
        // atan2(3, 4) and atan2(sqrt 21, 2)
        let (near, far) = (0.643_501_108_793_284_4, 1.159_279_480_727_408_5);
        assert!(
            close(&cuts, &[near, far, turn - far, turn - near]),
            "{cuts:?}"
        );
    }

    #[test]
    fn outline_cuts_of_a_hole_seen_off_axis_are_exact_inside_its_rims() {
        // the sphere with the hole above, seen along (7, -24, 0) / 25: its contour is the circle of
        // radius 5 in the plane of the z axis and d = (24, 7, 0) / 25, at angle t from d towards
        // +z. A point s along d is on the hole where s^2 - 6 s (24 / 25) + 8 = 0, so at
        // s = (72 -+ sqrt 184) / 25, and the hole's rims cut the contour at acos(s / 5) and at
        // their mirrors. They cross the contour's plane at the hole's angles 73.4 and 139.1
        // degrees, away from its seam and the sphere's (in y = 0): inside the rim edges, whose
        // curves the boolean traced
        all_fixes();
        let sphere = crate::make_sphere([0.0, 0.0, 0.0], 5.0).unwrap();
        let hole = crate::make_cylinder([3.0, 0.0, -10.0], 1.0, 20.0).unwrap();
        let b = crate::boolean::boolean(sphere, hole, crate::BooleanOp::Subtract)
            .unwrap()
            .unwrap();
        let o = kernel_order(&b);
        let contour = solidcore_record::brep::Curve::Circle {
            frame: solidcore_record::brep::Frame {
                origin: [0.0, 0.0, 0.0],
                x_axis: [0.96, 0.28, 0.0],
                y_axis: [0.0, 0.0, 1.0],
            },
            radius: 5.0,
        };
        let turn = std::f64::consts::TAU;
        let cuts = b
            .outline_cuts(
                &o,
                &faces_on(&b, &o, |s| matches!(s, SurfaceKind::Sphere(_))),
                &contour,
                [0.0, turn],
                [0.0, 0.0, 0.0],
                [0.28, -0.96, 0.0],
            )
            .unwrap();
        // acos((72 + sqrt 184) / 125) and acos((72 - sqrt 184) / 125)
        let (near, far) = (0.816_855_004_760_949_2, 1.084_355_294_332_124_4);
        assert!(
            close(&cuts, &[near, far, turn - far, turn - near]),
            "{cuts:?}"
        );
    }

    #[test]
    fn outline_cuts_of_a_cut_cone_keep_only_its_own_silhouette() {
        // a frustum of base radius 4 and top radius 1, 6 high: its apex would be at z = 8, and its
        // right silhouette seen along y runs from the apex to (4, 0, 0), x = 4 - z / 2. Less the
        // part past x = 2, its cut edge (a hyperbola) crosses that silhouette at (2, 0, 4), a
        // half of the way; the top circle meets it at (1, 0, 6), a quarter of the way. The face set
        // is the cone's; its seam lies on that silhouette and is on the cone on both sides
        all_fixes();
        let silhouette = solidcore_record::brep::Curve::Line {
            start: [0.0, 0.0, 8.0],
            end: [4.0, 0.0, 0.0],
        };
        // less the box from x = from_x, 10 wide
        let cut = |from_x: f64| {
            let cone = crate::make_cone([0.0, 0.0, 0.0], 4.0, 1.0, 6.0).unwrap();
            let past = make_box([from_x, -10.0, -1.0], [10.0, 20.0, 10.0]).unwrap();
            let b = crate::boolean::boolean(cone, past, crate::BooleanOp::Subtract)
                .unwrap()
                .unwrap();
            let o = kernel_order(&b);
            b.outline_cuts(
                &o,
                &faces_on(&b, &o, |s| matches!(s, SurfaceKind::Cone(_))),
                &silhouette,
                [0.0, 1.0],
                [0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
            )
            .unwrap()
        };
        let cuts = cut(2.0);
        assert!(close(&cuts, &[0.25, 0.5]), "{cuts:?}");
        // less the part where x < -2 instead: its hyperbola crosses the mirror silhouette at
        // (-2, 0, 4), which is no cut of this one; the base circle's (4, 0, 0) is its end
        let other = cut(-12.0);
        assert!(close(&other, &[0.25, 1.0]), "{other:?}");
    }

    #[test]
    fn outline_cuts_at_a_vertex_cut_once() {
        // the unit box's front face, y = 0, and the line through (-1, 0, 0) and (2, 0, 0) in z = 0:
        // the face's bottom edge lies on it from (0, 0, 0) to (1, 0, 0), and its two upright edges
        // end at those corners; each corner is one cut, a third and two thirds along
        all_fixes();
        let b = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
        let o = kernel_order(&b);
        let line = solidcore_record::brep::Curve::Line {
            start: [-1.0, 0.0, 0.0],
            end: [2.0, 0.0, 0.0],
        };
        let cuts = b
            .outline_cuts(
                &o,
                &faces_at(&b, &o, |c| c[1].abs() <= 1e-9),
                &line,
                [0.0, 1.0],
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
            )
            .unwrap();
        assert!(close(&cuts, &[1.0 / 3.0, 2.0 / 3.0]), "{cuts:?}");
    }

    #[test]
    fn outline_cuts_keep_a_rim_where_the_faces_meet_smoothly() {
        // a hemisphere of radius 5 below z = 0 and a cylinder of radius 5 from z = 0 to 5 above it,
        // one revolved solid: they meet smoothly on the circle of radius 5 in z = 0. Seen along y,
        // the hemisphere's contour is the circle of radius 5 in y = 0, at angle t from -z towards
        // +x; that rim crosses it at (5, 0, 0) and (-5, 0, 0), t = pi / 2 and -pi / 2
        all_fixes();
        let arc = crate::sweep::Curve::Arc {
            center: [0.0; 3],
            axis: [0.0, 1.0, 0.0],
            reference: [1.0, 0.0, 0.0],
            radius: 5.0,
            start: 0.0,
            end: std::f64::consts::FRAC_PI_2,
        };
        let line = |start: [f64; 3], end: [f64; 3]| crate::sweep::Curve::Line { start, end };
        let profile = [
            arc,
            line([0.0, 0.0, -5.0], [0.0, 0.0, 5.0]),
            line([0.0, 0.0, 5.0], [5.0, 0.0, 5.0]),
            line([5.0, 0.0, 5.0], [5.0, 0.0, 0.0]),
        ];
        let turn = std::f64::consts::TAU;
        let b = crate::sweep::revolve(&profile, [0.0; 3], [0.0, 0.0, 1.0], turn, true).unwrap();
        let o = kernel_order(&b);
        let below = faces_at(&b, &o, |c| c[2] < 0.0);
        assert_eq!(below.len(), 1, "{below:?}");
        let contour = solidcore_record::brep::Curve::Circle {
            frame: solidcore_record::brep::Frame {
                origin: [0.0, 0.0, 0.0],
                x_axis: [0.0, 0.0, -1.0],
                y_axis: [1.0, 0.0, 0.0],
            },
            radius: 5.0,
        };
        let half = std::f64::consts::PI;
        let cuts = b
            .outline_cuts(
                &o,
                &below,
                &contour,
                [-half, half],
                [0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
            )
            .unwrap();
        let quarter = std::f64::consts::FRAC_PI_2;
        assert!(close(&cuts, &[-quarter, quarter]), "{cuts:?}");
    }

    #[test]
    fn outline_cuts_refuse_what_they_cannot_read() {
        all_fixes();
        let b = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
        let o = kernel_order(&b);
        let line = solidcore_record::brep::Curve::Line {
            start: [-1.0, 0.0, 0.0],
            end: [2.0, 0.0, 0.0],
        };
        let past = b.outline_cuts(
            &o,
            &[o.faces.len()],
            &line,
            [0.0, 1.0],
            [0.0; 3],
            [0.0, 0.0, 1.0],
        );
        assert!(matches!(past, Err(Error::Argument)), "{past:?}");
        for bad in [f64::NAN, f64::INFINITY] {
            let point = b.outline_cuts(
                &o,
                &[0],
                &line,
                [0.0, 1.0],
                [bad, 0.0, 0.0],
                [0.0, 0.0, 1.0],
            );
            assert!(matches!(point, Err(Error::Argument)), "{point:?}");
            let normal = b.outline_cuts(&o, &[0], &line, [0.0, 1.0], [0.0; 3], [0.0, bad, 1.0]);
            assert!(matches!(normal, Err(Error::Argument)), "{normal:?}");
        }
        // a normal of no direction is the kernel's refusal
        assert!(b
            .outline_cuts(&o, &[0], &line, [0.0, 1.0], [0.0; 3], [0.0; 3])
            .is_err());
    }

    #[test]
    fn rim_edges_are_the_edges_with_the_set_on_one_side() {
        // a box's top face: its four edges, in record order; all six faces, and a whole sphere's
        // faces (whose seam has the sphere on both sides), close up and have none
        all_fixes();
        let b = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
        let o = kernel_order(&b);
        let top = faces_at(&b, &o, |c| (c[2] - 1.0).abs() <= 1e-9);
        let on_top: Vec<usize> = (0..o.edges.len())
            .filter(|&i| (b.edge_middle(&o, i).unwrap()[2] - 1.0).abs() <= 1e-9)
            .collect();
        assert_eq!(on_top.len(), 4, "{on_top:?}");
        assert_eq!(b.rim_edges(&o, &top), Ok(on_top));
        let all: Vec<usize> = (0..o.faces.len()).collect();
        assert_eq!(b.rim_edges(&o, &all), Ok(Vec::new()));
        assert_eq!(b.rim_edges(&o, &[o.faces.len()]), Err(Error::Argument));
        let ball = crate::make_sphere([0.0, 0.0, 0.0], 5.0).unwrap();
        let ball_order = kernel_order(&ball);
        let whole = faces_on(&ball, &ball_order, |s| matches!(s, SurfaceKind::Sphere(_)));
        assert_eq!(ball.rim_edges(&ball_order, &whole), Ok(Vec::new()));
    }

    #[test]
    fn curve_arc_length_and_2d_polyline_ops_wrappers_round_trip() {
        let line = solidcore_record::brep::Curve::Line {
            start: [0.0, 0.0, 0.0],
            end: [6.0, 8.0, 0.0],
        };
        let len = crate::curve::arc_length(&line, None).unwrap();
        assert!((len - 10.0).abs() < 1e-9, "{len}");
        let mid = crate::curve::point_at_arc_length(&line, None, false, 5.0).unwrap();
        assert!((mid.point[0] - 3.0).abs() < 1e-9 && (mid.point[1] - 4.0).abs() < 1e-9);
        let from_end = crate::curve::point_at_arc_length(&line, None, true, 2.0).unwrap();
        assert!((from_end.point[0] - 4.8).abs() < 1e-9 && (from_end.point[1] - 6.4).abs() < 1e-9);

        let poly = solidcore_record::brep::Curve::Polyline {
            points: vec![[0.0, 0.0, 0.0], [3.0, 0.0, 0.0], [3.0, 4.0, 0.0]],
        };
        let (total, samples) =
            crate::curve::samples_at_fractions(&poly, None, &[0.0, 0.5, 1.0]).unwrap();
        assert!((total - 7.0).abs() < 1e-9);
        assert_eq!(samples.len(), 3);
        assert!(
            (samples[1].point[0] - 3.0).abs() < 1e-6 && (samples[1].point[1] - 0.5).abs() < 1e-6
        );

        let b = polyline_2d_bounds(&[[1.0, 2.0], [4.0, -1.0]])
            .unwrap()
            .unwrap();
        assert_eq!(b, ([1.0, -1.0], [4.0, 2.0]));
        let (d, p) = polyline_2d_closest_point([2.0, 3.0], &[[0.0, 0.0], [4.0, 0.0]])
            .unwrap()
            .unwrap();
        assert!((d - 3.0).abs() < 1e-9 && (p[0] - 2.0).abs() < 1e-9);
        let cross =
            polyline_2d_crossings(&[[0.0, 0.0], [10.0, 0.0]], &[[5.0, -2.0], [5.0, 2.0]], 1e-6)
                .unwrap();
        assert_eq!(cross.len(), 1);
        assert!((cross[0][0] - 5.0).abs() < 1e-9 && cross[0][1].abs() < 1e-9);
        let lc = tangent_contact_line_circle_2d([0.0, 0.0], [10.0, 0.0], [4.0, 3.0], 3.0, 1e-6)
            .unwrap()
            .unwrap();
        assert!((lc[0] - 4.0).abs() < 1e-9 && lc[1].abs() < 1e-9);
        let cc = tangent_contact_circle_circle_2d([0.0, 0.0], 2.0, [5.0, 0.0], 3.0, 1e-6)
            .unwrap()
            .unwrap();
        assert!((cc[0] - 2.0).abs() < 1e-9 && cc[1].abs() < 1e-9);
    }
}
