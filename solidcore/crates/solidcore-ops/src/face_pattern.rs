//! Pattern on Face: the stations a pattern stands its copies on across a face of a B-rep record
//! and how each copy is placed there, from the kernel's [`solidcore::face_pattern`] primitive (which computes every point, direction
//! and length). The record reaches the kernel through a [`Query`], made once for any number of
//! calls; this module converts the kernel's answers to the record's points and types its refusals.
//!
//! - [`grid_stations`]: a grid across the face, one station at each cell's centre;
//! - [`random_stations`]: stations at random, even over the face's area, spread by a [`Noise`];
//! - [`edge_points`]: points along the face's edges, for clearances measured from them;
//! - [`spaced_stations`], [`spiral_stations`], [`fill_stations`] and [`geodesic_stations`]: the
//!   layouts spaced by distance on the surface (rows a spacing apart, a Fibonacci spiral, an even
//!   fill, a geodesic sphere), even where the parameters crowd, as at a sphere's poles;
//! - the `_across` forms of the first three, and every layout, take a set of faces as one region:
//!   the edges two of them share are not edges of it;
//! - [`placement`]: the matrix that stands a copy on a station;
//! - [`attraction`] and [`ramp`]: how much an attractor changes a copy, by its distance.

use solidcore::face_pattern as k;
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::Vec3;

use crate::error::status;
use crate::query::Query;
use crate::Result;

pub use solidcore::face_pattern::{
    attraction, ramp, Falloff, Fit, Grid, Noise, Random, Spacing, MAX_EDGE_POINTS, MAX_FACES,
    MAX_GEODESIC_FREQUENCY, MAX_STATION_COUNT,
};

/// A point on a face where a pattern puts a copy, with the face's frame there
/// ([`k::Station`]), in the record's points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Station {
    /// The point, on the face's surface.
    pub point: Vec3,
    /// The face's unit normal there, out of the material.
    pub normal: Vec3,
    /// A unit direction along the face, square to `normal`: the way its first parameter runs.
    pub along: Vec3,
    /// How far the point is from the face's boundary: its edges, not its seams.
    pub margin: f64,
}

fn station(s: &k::Station) -> Station {
    Station {
        point: record_vec3(s.point),
        normal: record_vec3(s.normal),
        along: record_vec3(s.along),
        margin: s.margin,
    }
}

/// The stations a grid puts on face `face` of `query`'s record, inside it and at least `margin`
/// from its boundary ([`k::grid_stations`]).
///
/// # Errors
/// [`crate::Error::Refused`] for a count, angle or margin out of range, or a face the record
/// lacks or whose surface or loops can't be read.
pub fn grid_stations(query: &Query, face: usize, grid: &Grid, margin: f64) -> Result<Vec<Station>> {
    let stations = k::grid_stations(query.kernel(), face, grid, margin).map_err(status)?;
    Ok(stations.iter().map(station).collect())
}

/// `random.count` stations at random on face `face` of `query`'s record, each inside it, at least
/// `margin` from its boundary and one `keep` takes ([`k::random_stations`]).
///
/// # Errors
/// [`crate::Error::Refused`] for a count or margin out of range, or a face the record lacks,
/// whose surface or loops can't be read or that has no area.
pub fn random_stations(
    query: &Query,
    face: usize,
    random: &Random,
    margin: f64,
    keep: &mut dyn FnMut(&Station) -> bool,
) -> Result<Vec<Station>> {
    let mut keep_kernel = |s: &k::Station| keep(&station(s));
    let stations = k::random_stations(query.kernel(), face, random, margin, &mut keep_kernel)
        .map_err(status)?;
    Ok(stations.iter().map(station).collect())
}

/// Points along each edge of face `face` (not its seams), at most `spacing` apart
/// ([`k::edge_points`]).
///
/// # Errors
/// [`crate::Error::Refused`] for a spacing not above zero, a face the record lacks, or an edge
/// whose curve can't be evaluated.
pub fn edge_points(query: &Query, face: usize, spacing: f64) -> Result<Vec<Vec<Vec3>>> {
    let lists = k::edge_points(query.kernel(), face, spacing).map_err(status)?;
    Ok(lists
        .into_iter()
        .map(|l| l.into_iter().map(record_vec3).collect())
        .collect())
}

fn stations(found: solidcore::Result<Vec<k::Station>>) -> Result<Vec<Station>> {
    Ok(found.map_err(status)?.iter().map(station).collect())
}

/// The stations a grid puts on each of `faces` ([`k::grid_stations_across`]), the margin measured
/// to the boundary of the set; with `fit`, from the fit in from the faces' edges to the same in
/// from the other side ([`Fit`]), else at the centres of its cells.
///
/// # Errors
/// As [`grid_stations`]'s, and for no face, more than [`MAX_FACES`] or a face listed twice.
pub fn grid_stations_across(
    query: &Query,
    faces: &[usize],
    grid: &Grid,
    fit: Option<&Fit>,
    margin: f64,
) -> Result<Vec<Station>> {
    stations(k::grid_stations_across(
        query.kernel(),
        faces,
        grid,
        fit,
        margin,
    ))
}

/// Stations at random over `faces` together ([`k::random_stations_across`]).
///
/// # Errors
/// As [`random_stations`]'s, and for no face, more than [`MAX_FACES`] or a face listed twice.
pub fn random_stations_across(
    query: &Query,
    faces: &[usize],
    random: &Random,
    margin: f64,
    keep: &mut dyn FnMut(&Station) -> bool,
) -> Result<Vec<Station>> {
    let mut keep_kernel = |s: &k::Station| keep(&station(s));
    stations(k::random_stations_across(
        query.kernel(),
        faces,
        random,
        margin,
        &mut keep_kernel,
    ))
}

/// Points along each edge of the boundary of `faces` together ([`k::edge_points_across`]).
///
/// # Errors
/// As [`edge_points`]'s, and for no face, more than [`MAX_FACES`] or a face listed twice.
pub fn edge_points_across(query: &Query, faces: &[usize], spacing: f64) -> Result<Vec<Vec<Vec3>>> {
    let lists = k::edge_points_across(query.kernel(), faces, spacing).map_err(status)?;
    Ok(lists
        .into_iter()
        .map(|l| l.into_iter().map(record_vec3).collect())
        .collect())
}

/// A point along the boundary of a set of faces, with which way is off them there
/// ([`k::EdgeFrame`]), in the record's points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeFrame {
    /// The point, on the edge's curve.
    pub point: Vec3,
    /// The unit direction off the faces across the edge, along the face, square to the edge.
    pub out: Vec3,
    /// The face's unit normal there, out of the material.
    pub normal: Vec3,
}

/// Points along each edge of the boundary of `faces` together, each with the direction off them
/// and the face's normal ([`k::edge_frames_across`]).
///
/// # Errors
/// As [`edge_points_across`]'s, and for an edge whose face can't be read along it.
pub fn edge_frames_across(
    query: &Query,
    faces: &[usize],
    spacing: f64,
) -> Result<Vec<Vec<EdgeFrame>>> {
    let lines = k::edge_frames_across(query.kernel(), faces, spacing).map_err(status)?;
    Ok(lines
        .into_iter()
        .map(|l| {
            l.into_iter()
                .map(|f| EdgeFrame {
                    point: record_vec3(f.point),
                    out: record_vec3(f.out),
                    normal: record_vec3(f.normal),
                })
                .collect()
        })
        .collect())
}

/// Rows a distance apart on `faces` ([`k::spaced_stations`]); with `fit`, from the fit in from
/// the faces' edges to the same in from the other side.
///
/// # Errors
/// [`crate::Error::Refused`] for a spacing not above zero or so small the faces would take more
/// than 16 000 places, a margin out of range, or faces the record lacks or can't read.
pub fn spaced_stations(
    query: &Query,
    faces: &[usize],
    spacing: &Spacing,
    fit: Option<&Fit>,
    margin: f64,
) -> Result<Vec<Station>> {
    stations(k::spaced_stations(
        query.kernel(),
        faces,
        spacing,
        fit,
        margin,
    ))
}

/// `count` stations on a Fibonacci spiral even over `faces`' area ([`k::spiral_stations`]).
///
/// # Errors
/// [`crate::Error::Refused`] for a count past [`MAX_STATION_COUNT`], a margin out of range, or
/// faces the record lacks, can't read or that have no area.
pub fn spiral_stations(
    query: &Query,
    faces: &[usize],
    count: usize,
    margin: f64,
) -> Result<Vec<Station>> {
    stations(k::spiral_stations(query.kernel(), faces, count, margin))
}

/// `count` stations spread evenly over `faces` together ([`k::fill_stations`]).
///
/// # Errors
/// As [`spiral_stations`]'s.
pub fn fill_stations(
    query: &Query,
    faces: &[usize],
    count: usize,
    margin: f64,
) -> Result<Vec<Station>> {
    stations(k::fill_stations(query.kernel(), faces, count, margin))
}

/// The vertices of a geodesic sphere of `frequency` on `faces` of one sphere, turned by `turn`
/// (radians) about its pole ([`k::geodesic_stations`]).
///
/// # Errors
/// [`crate::Error::Refused`] for a frequency of 0 or past [`MAX_GEODESIC_FREQUENCY`], a turn that
/// is not a number, a margin out of range, or faces the record lacks, can't read or that are not
/// all of one sphere.
pub fn geodesic_stations(
    query: &Query,
    faces: &[usize],
    frequency: usize,
    turn: f64,
    margin: f64,
) -> Result<Vec<Station>> {
    stations(k::geodesic_stations(
        query.kernel(),
        faces,
        frequency,
        turn,
        margin,
    ))
}

/// The placement that stands a copy on `station` ([`k::placement`]): a column-major 4 × 4 matrix,
/// the images of x, y and z, then the translation.
///
/// # Errors
/// [`crate::Error::Refused`] for an input that is not finite, a scale not above zero, or a
/// station whose normal and direction are not unit and square.
pub fn placement(
    station: &Station,
    anchor: Vec3,
    flip: bool,
    scale: f64,
    offset: f64,
    spin: f64,
) -> Result<[[f64; 4]; 4]> {
    let s = k::Station {
        point: vec3(station.point),
        normal: vec3(station.normal),
        along: vec3(station.along),
        margin: station.margin,
    };
    k::placement(&s, vec3(anchor), flip, scale, offset, spin).map_err(status)
}

#[cfg(test)]
#[path = "face_pattern_tests.rs"]
mod tests;
