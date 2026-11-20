//! Pattern on Face: the stations a pattern stands its copies on across a face of a B-rep record
//! and how each copy is placed there, from the kernel's [`solidcore::face_pattern`] primitive (which computes every point, direction
//! and length). The record reaches the kernel through a [`Query`], made once for any number of
//! calls; this module converts the kernel's answers to the record's points and types its refusals.
//!
//! - [`grid_stations`]: a grid across the face, one station at each cell's centre;
//! - [`random_stations`]: stations at random, even over the face's area, spread by a [`Noise`];
//! - [`edge_points`]: points along the face's edges, for clearances measured from them;
//! - [`placement`]: the matrix that stands a copy on a station;
//! - [`attraction`] and [`ramp`]: how much an attractor changes a copy, by its distance.

use solidcore::face_pattern as k;
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::Vec3;

use crate::error::status;
use crate::query::Query;
use crate::Result;

pub use solidcore::face_pattern::{
    attraction, ramp, Falloff, Grid, Noise, Random, MAX_EDGE_POINTS, MAX_STATION_COUNT,
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
