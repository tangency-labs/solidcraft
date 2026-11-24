//! Pattern on Face: where copies stand on a face and how each is placed, through
//! `solidcore_ops::face_pattern` (the recipe) and the kernel's `solidcore::face_pattern` (every
//! point, direction and length). This crate computes nothing of its own here: it guards the
//! calls and words the refusals.
//!
//! - [`face_stations`] and [`face_random_stations`]: the stations a grid or a random placement
//!   puts on a face, inside it and clear of its boundary, each with the face's frame there.
//! - [`face_layout_stations`]: the stations any layout ([`FaceLayout`]) puts on a set of faces
//!   of one body, taken as one region: besides the grid and random ones, rows a spacing apart,
//!   a Fibonacci spiral, an even fill and a geodesic sphere, all spaced by distance on the
//!   surface so they stay even where its parameters crowd (a sphere's poles).
//! - [`face_edge_points`] and [`face_edge_points_across`]: points along a face's edges, or a set
//!   of faces' outline, for clearances measured from them.
//! - [`PointDistance`]: exact distances from points to a face or a whole body.
//! - [`face_placement`], [`attraction`] and [`ramp`]: the matrix that stands a copy on a station,
//!   and how much an attractor changes it by its distance.

pub use solidcore_ops::face_pattern::{
    EdgeFrame as FaceEdgeFrame, Falloff, MAX_EDGE_POINTS, MAX_FACES, MAX_GEODESIC_FREQUENCY, MAX_STATION_COUNT, Noise, Station as FaceStation,
    attraction, ramp,
};

use solidcore_ops::face_pattern as ops;
use solidcore_ops::query::Query;
use solidcore_record::Vec3;

use crate::body::Body;
use crate::{KernelError, Result, guard};

/// A face of a B-rep body, checked: a mesh body has no faces.
fn faces_of(body: &Body) -> Result<()> {
    if body.is_mesh() {
        return Err(KernelError::Invalid("pattern on a face: a mesh body has no faces; use Convert Mesh first".into()));
    }
    Ok(())
}

fn refused(e: solidcore_ops::Error) -> KernelError {
    KernelError::Invalid(format!("pattern on a face: {e}"))
}

/// The points a grid of `count_u` by `count_v` cells puts on face `face` of `body`, one at each
/// cell's centre. The grid covers the face's extent in its surface parameters (u, v), so on a
/// cylinder `count_u` runs round it and `count_v` along it. `angle` (radians) turns the grid
/// about the extent's centre, measured in the extent's own unit square; `staggered` moves every
/// other column half a cell along. Points the face's trims leave out (holes, the parts of its
/// extent it does not cover) are dropped, as are points nearer than `margin` to its boundary.
///
/// # Errors
/// [`KernelError::Invalid`] for a count out of range, an angle or margin that is not a number or
/// a negative margin, a mesh body, or a face the body lacks or the kernel cannot read.
pub fn face_stations(body: &Body, face: usize, count_u: usize, count_v: usize, angle: f64, staggered: bool, margin: f64) -> Result<Vec<FaceStation>> {
    if !(1..=MAX_STATION_COUNT).contains(&count_u) || !(1..=MAX_STATION_COUNT).contains(&count_v) {
        return Err(KernelError::Invalid(format!("pattern on a face: 1…{MAX_STATION_COUNT} cells each way")));
    }
    if !(angle.is_finite() && margin.is_finite() && margin >= 0.0) {
        return Err(KernelError::Invalid("pattern on a face: the angle and the margin must be numbers, the margin not negative".into()));
    }
    faces_of(body)?;
    guard("pattern on a face", || {
        let grid = ops::Grid { count_u, count_v, angle, staggered };
        ops::grid_stations(&Query::new(body.record()), face, &grid, margin).map_err(refused)
    })
}

/// `count` points placed at random on face `face` of `body`, the same for the same `seed`, spread
/// as `noise` says, none nearer than `margin` to the face's boundary and each one `keep` takes (it
/// sees the station with its normal out of the body's material, and may say no to one a copy
/// there would not fit). Places are uniform over the face's area (not its parameters), inside its
/// trims. Fewer come back when the face has no room for more within the tries allowed.
///
/// # Errors
/// [`KernelError::Invalid`] for a count past [`MAX_STATION_COUNT`], a negative margin, a mesh
/// body, or a face the body lacks, the kernel cannot read or that has no area.
pub fn face_random_stations(
    body: &Body,
    face: usize,
    count: usize,
    seed: u64,
    noise: Noise,
    margin: f64,
    keep: &mut dyn FnMut(&FaceStation) -> bool,
) -> Result<Vec<FaceStation>> {
    if count > MAX_STATION_COUNT {
        return Err(KernelError::Invalid(format!("pattern on a face: at most {MAX_STATION_COUNT} random instances")));
    }
    if !(margin.is_finite() && margin >= 0.0) {
        return Err(KernelError::Invalid("pattern on a face: the margin must not be negative".into()));
    }
    faces_of(body)?;
    guard("pattern on a face", || {
        let random = ops::Random { count, seed, noise };
        ops::random_stations(&Query::new(body.record()), face, &random, margin, keep).map_err(refused)
    })
}

/// Points along each of face `face`'s edges (not its seams), one list for each edge from its
/// start to its end, on the edge's exact curve, the parameter halved until no two neighbours are
/// more than `spacing` apart (or [`MAX_EDGE_POINTS`] is reached).
///
/// # Errors
/// [`KernelError::Invalid`] for a spacing not above zero, a mesh body, or a face the body lacks
/// or an edge the kernel cannot evaluate.
pub fn face_edge_points(body: &Body, face: usize, spacing: f64) -> Result<Vec<Vec<Vec3>>> {
    if !(spacing.is_finite() && spacing > 0.0) {
        return Err(KernelError::Invalid("edge points: the spacing must be above zero".into()));
    }
    faces_of(body)?;
    guard("edge points", || {
        ops::edge_points(&Query::new(body.record()), face, spacing).map_err(|e| KernelError::Invalid(format!("edge points: {e}")))
    })
}

/// How a face pattern lays its stations out ([`face_layout_stations`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FaceLayout {
    /// A grid across each face's parameters: with `fit` (mm in from the edges its first and its
    /// second parameter run between), from that far in from one edge to that far in from the
    /// other, its outer stations on those lines; without, at the centres of its cells
    /// ([`face_stations`]).
    Grid { count_u: usize, count_v: usize, angle: f64, staggered: bool, fit: Option<[f64; 2]> },
    /// At random, even over the faces' area ([`face_random_stations`]).
    Random { count: usize, seed: u64, noise: Noise },
    /// Rows `row_spacing` apart along the surface, each with as many stations as fit its length at
    /// `spacing` apart; round a sphere, its circles of latitude. With `fit`, the outer rows and
    /// the outer stations of each open row stand that far in from the edges, as the grid's.
    Spacing { spacing: f64, row_spacing: f64, staggered: bool, fit: Option<[f64; 2]> },
    /// `count` stations on a Fibonacci spiral, even over the faces' area.
    Spiral { count: usize },
    /// `count` stations relaxed until evenly spread over the faces together (Lloyd's method).
    Fill { count: usize },
    /// The vertices of a geodesic sphere of `frequency` on faces of one sphere, turned by `turn`
    /// (radians) about its pole.
    Geodesic { frequency: usize, turn: f64 },
}

/// The stations `layout` puts on `faces` of `body`, taken as one region: inside the faces and at
/// least `margin` from the region's boundary (the edges two of them share are not edges of it).
/// `keep` sees each random station as it is drawn, and may say no to one a copy there would not
/// fit; the other layouts' stations are all returned.
///
/// # Errors
/// [`KernelError::Invalid`] for a count, spacing, frequency, angle or margin out of range, a mesh
/// body, no face or more than [`MAX_FACES`], a face listed twice, one the body lacks or the
/// kernel cannot read, or a geodesic on faces not all of one sphere.
pub fn face_layout_stations(
    body: &Body,
    faces: &[usize],
    layout: &FaceLayout,
    margin: f64,
    keep: &mut dyn FnMut(&FaceStation) -> bool,
) -> Result<Vec<FaceStation>> {
    if faces.is_empty() || faces.len() > MAX_FACES {
        return Err(KernelError::Invalid(format!("pattern on a face: 1…{MAX_FACES} faces")));
    }
    if !(margin.is_finite() && margin >= 0.0) {
        return Err(KernelError::Invalid("pattern on a face: the margin must not be negative".into()));
    }
    let fitted = |fit: Option<[f64; 2]>| {
        if fit.is_some_and(|f| !f.iter().all(|x| x.is_finite() && *x >= 0.0)) {
            Err(KernelError::Invalid("pattern on a face: the fit to the edges must not be negative".into()))
        } else {
            Ok(())
        }
    };
    let counted = |count: usize| {
        if count > MAX_STATION_COUNT {
            Err(KernelError::Invalid(format!("pattern on a face: at most {MAX_STATION_COUNT} instances")))
        } else {
            Ok(())
        }
    };
    match *layout {
        FaceLayout::Grid { count_u, count_v, angle, fit, .. } => {
            fitted(fit)?;
            if !(1..=MAX_STATION_COUNT).contains(&count_u) || !(1..=MAX_STATION_COUNT).contains(&count_v) {
                return Err(KernelError::Invalid(format!("pattern on a face: 1…{MAX_STATION_COUNT} cells each way")));
            }
            if !angle.is_finite() {
                return Err(KernelError::Invalid("pattern on a face: the angle must be a number".into()));
            }
        }
        FaceLayout::Random { count, .. } | FaceLayout::Spiral { count } | FaceLayout::Fill { count } => counted(count)?,
        FaceLayout::Spacing { spacing, row_spacing, fit, .. } => {
            fitted(fit)?;
            if !(spacing.is_finite() && spacing > 0.0 && row_spacing.is_finite() && row_spacing > 0.0) {
                return Err(KernelError::Invalid("pattern on a face: the spacings must be above zero".into()));
            }
        }
        FaceLayout::Geodesic { frequency, turn } => {
            if !(1..=MAX_GEODESIC_FREQUENCY).contains(&frequency) {
                return Err(KernelError::Invalid(format!("pattern on a face: a geodesic's frequency is 1…{MAX_GEODESIC_FREQUENCY}")));
            }
            if !turn.is_finite() {
                return Err(KernelError::Invalid("pattern on a face: the angle must be a number".into()));
            }
        }
    }
    faces_of(body)?;
    guard("pattern on a face", || {
        let q = Query::new(body.record());
        match *layout {
            FaceLayout::Grid { count_u, count_v, angle, staggered, fit } => {
                let fit = fit.map(|[u, v]| ops::Fit { u, v });
                ops::grid_stations_across(&q, faces, &ops::Grid { count_u, count_v, angle, staggered }, fit.as_ref(), margin)
            }
            FaceLayout::Random { count, seed, noise } => ops::random_stations_across(&q, faces, &ops::Random { count, seed, noise }, margin, keep),
            FaceLayout::Spacing { spacing, row_spacing, staggered, fit } => {
                let fit = fit.map(|[u, v]| ops::Fit { u, v });
                ops::spaced_stations(&q, faces, &ops::Spacing { spacing, row_spacing, staggered }, fit.as_ref(), margin)
            }
            FaceLayout::Spiral { count } => ops::spiral_stations(&q, faces, count, margin),
            FaceLayout::Fill { count } => ops::fill_stations(&q, faces, count, margin),
            FaceLayout::Geodesic { frequency, turn } => ops::geodesic_stations(&q, faces, frequency, turn, margin),
        }
        .map_err(refused)
    })
}

/// Points along each edge of the outline of `faces` of `body` together (not their seams, nor the
/// edges two of them share), as [`face_edge_points`].
///
/// # Errors
/// [`KernelError::Invalid`] for a spacing not above zero, a mesh body, no face or more than
/// [`MAX_FACES`], a face listed twice or one the body lacks, or an edge the kernel cannot
/// evaluate.
pub fn face_edge_points_across(body: &Body, faces: &[usize], spacing: f64) -> Result<Vec<Vec<Vec3>>> {
    if !(spacing.is_finite() && spacing > 0.0) {
        return Err(KernelError::Invalid("edge points: the spacing must be above zero".into()));
    }
    faces_of(body)?;
    guard("edge points", || {
        ops::edge_points_across(&Query::new(body.record()), faces, spacing).map_err(|e| KernelError::Invalid(format!("edge points: {e}")))
    })
}

/// Points along each edge of the outline of `faces` of `body` together, as
/// [`face_edge_points_across`], each with the unit direction off the faces across the edge
/// (`out`, along the face, square to the edge) and the face's normal there: for telling whether a
/// copy standing on the faces reaches past their edges.
///
/// # Errors
/// As [`face_edge_points_across`]'s.
pub fn face_edge_frames_across(body: &Body, faces: &[usize], spacing: f64) -> Result<Vec<Vec<ops::EdgeFrame>>> {
    if !(spacing.is_finite() && spacing > 0.0) {
        return Err(KernelError::Invalid("edge points: the spacing must be above zero".into()));
    }
    faces_of(body)?;
    guard("edge points", || {
        ops::edge_frames_across(&Query::new(body.record()), faces, spacing).map_err(|e| KernelError::Invalid(format!("edge points: {e}")))
    })
}

/// Exact distances from points to a face, or to every face of a body, as the faces are trimmed:
/// the kernel's closest-point solve (`solidcore_ops::Body::face_distances`), never a mesh. Made
/// once for any number of points.
pub struct PointDistance<'a> {
    body: &'a Body,
    /// The face, or `None` for the whole body.
    face: Option<usize>,
}

/// How far past a body the closest-point solve of a whole body looks for its nearest face: far
/// past any model, so that every face is in reach.
const BODY_REACH: f64 = 1.0e12;

impl<'a> PointDistance<'a> {
    /// Distances to face `face` of `body`.
    ///
    /// # Errors
    /// [`KernelError::Invalid`] for a mesh body or a face the body lacks.
    pub fn to_face(body: &'a Body, face: usize) -> Result<PointDistance<'a>> {
        body.require_brep("a distance to a face")?;
        if face >= body.record().faces.len() {
            return Err(KernelError::Invalid(format!("the body has no face {face}")));
        }
        Ok(PointDistance { body, face: Some(face) })
    }

    /// Distances to the whole of `body`: its nearest face.
    ///
    /// # Errors
    /// [`KernelError::Invalid`] for a mesh body or a body with no faces.
    pub fn to_body(body: &'a Body) -> Result<PointDistance<'a>> {
        body.require_brep("a distance to a body")?;
        if body.record().faces.is_empty() {
            return Err(KernelError::Invalid("the body has no faces".into()));
        }
        Ok(PointDistance { body, face: None })
    }

    /// The distance from `at` to the nearest point of the face or body.
    ///
    /// # Errors
    /// [`KernelError::Invalid`] for a point that is not finite; [`KernelError::Failed`] where the
    /// kernel cannot take the body or no distance could be measured.
    pub fn measure(&self, at: Vec3) -> Result<f64> {
        if !at.is_finite() {
            return Err(KernelError::Invalid("the point must be finite".into()));
        }
        let failed = || KernelError::Failed("no distance to the face could be found".into());
        guard("a distance to a face", || {
            let form = self.body.ops_body().ok_or_else(|| KernelError::Failed("a distance to a face: the kernel cannot take this body".into()))?;
            let (ops, order) = &*form;
            let p = [[at.x, at.y, at.z]];
            let found = match self.face {
                Some(f) => ops
                    .face_distances(order, f, &p)
                    .map_err(|e| KernelError::Failed(format!("a distance to a face: {e}")))?
                    .into_iter()
                    .flatten()
                    .next(),
                None => ops
                    .face_distances_within(order, &p, BODY_REACH)
                    .map_err(|e| KernelError::Failed(format!("a distance to a body: {e}")))?
                    .into_iter()
                    .filter_map(|row| row.first().copied().flatten())
                    .min_by(f64::total_cmp),
            };
            found.filter(|d| d.is_finite()).ok_or_else(failed)
        })
    }
}

/// The placement (a column-major matrix: the images of x, y and z, then the translation) that
/// stands a copy on `station`: the copy's point `anchor` on the station, its +Z along the
/// station's normal (against it with `flip`) and moved out by `offset`, its X along the station's
/// direction turned by `spin` (radians) about that axis, all scaled by `scale`.
///
/// # Errors
/// [`KernelError::Invalid`] for a number that is not finite or a scale not above zero.
pub fn face_placement(station: &FaceStation, anchor: Vec3, flip: bool, scale: f64, offset: f64, spin: f64) -> Result<[[f64; 4]; 4]> {
    if !(scale.is_finite() && scale > 1e-6) {
        return Err(KernelError::Invalid(format!("attractors: a copy's scale came to {scale}; scales must stay above zero")));
    }
    guard("pattern on a face", || ops::placement(station, anchor, flip, scale, offset, spin).map_err(refused))
}
