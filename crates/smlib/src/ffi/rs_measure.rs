//! `smlib_brep_mass_properties`, `smlib_brep_area`, `smlib_brep_bounds` and `smlib_brep_check` on
//! solidcore.
use std::fmt::Write;

use solidcore::geom::assert_array::AssertArray;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{MassPropertyBehavior, PropertyAttributes, brep_properties, face_centroid};
use solidcore::math::{Point3, Vec3};
use solidcore::query::brep::{BoundingBoxOptions, calculate_bounding_box};
use solidcore::topology::Brep;
use solidcore::topology::brep::validation::validate_pointers;
use solidcore::topology::edge::max_up_dim_gap_3d;

use super::rs::{body, keys, status, with_reader};
use super::{ERR_ARG, ERR_BUFFER};

/// SMLib clamps the accuracy of its volume integrals to this range.
fn accuracy(relative: f64) -> Result<f64, i32> {
    if relative.is_finite() { Ok(relative.clamp(1e-8, 1e-1)) } else { Err(ERR_ARG) }
}

fn with_attributes<R>(f: impl FnOnce(&PropertyAttributes<'_>) -> R) -> R {
    let store = AttributeStore::default();
    f(&PropertyAttributes { store: &store, key_of: &|_| ObjectKey(0) })
}

fn properties(k: &Kernel, b: &mut Brep, acc: f64, origin: Point3) -> Result<solidcore::mass::integrator::properties::PreciseProperties, i32> {
    with_attributes(|a| brep_properties(k, b, acc, origin, MassPropertyBehavior::default(), a)).map_err(|e| status(&e))
}

/// Whether `b` holds material: a solid region (the infinite one and voids hold none).
fn has_solid(b: &Brep) -> bool {
    let infinite = b.infinite_region();
    b.regions().iter().any(|(id, r)| Some(id) != infinite && r.is_void == 0)
}

/// A body with no material (a sheet, an open shell) measures as its faces, each counted once: their
/// total area and its centroid. The solid integral skips them, as both uses of a face face the void.
fn faces_measure(k: &Kernel, b: &mut Brep, acc: f64) -> Result<(f64, Point3), i32> {
    let (mut area, mut moment) = (0.0, Vec3::new(0.0, 0.0, 0.0));
    for f in b.face_list().to_vec() {
        let (a, c) = face_centroid(k, b, f, acc).map_err(|e| status(&e))?;
        area += a;
        moment += Vec3::new(c.x, c.y, c.z) * a;
    }
    if area <= 0.0 {
        return Ok((0.0, Point3::new(0.0, 0.0, 0.0)));
    }
    Ok((area, Point3::new(moment.x / area, moment.y / area, moment.z / area)))
}

pub fn mass_properties(h: &super::Handle, relative_accuracy: f64) -> Result<[f64; 11], i32> {
    let acc = accuracy(relative_accuracy)?;
    with_reader(|k| {
        let mut b = body(h);
        if !has_solid(&b) {
            let (area, c) = faces_measure(k, &mut b, acc)?;
            return Ok([area, 0.0, c.x, c.y, c.z, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        }
        // The first moments about the box's midpoint give the centroid; the second moments are then
        // taken about it, so a body far from the origin loses nothing to cancellation.
        let mid = calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default()).map_err(|e| status(&e))?.mid();
        let first = properties(k, &mut b, acc, mid)?;
        let centroid = match (first.volume > 0.0, first.moments.get(4)) {
            (true, Some(m)) => mid + *m / first.mass,
            _ => mid,
        };
        // One pass about the box's midpoint, then the parallel-axis shift to the centroid (as the C
        // API does), instead of a second integral about the centroid.
        let (Some(products), Some(axial)) = (first.moments.get(6), first.moments.get(7)) else { return Err(super::ERR_EXCEPTION) };
        let d = match (first.volume > 0.0, first.moments.get(4)) {
            (true, Some(m)) => *m / first.mass,
            _ => Vec3::new(0.0, 0.0, 0.0),
        };
        let m = first.mass;
        // unconfirmed: moments[6] is the products yz, zx, xy as smlib.h gives them (signed integrals
        // about the midpoint), and moments[7] the axial moments Ixx, Iyy, Izz.
        Ok([
            first.area,
            first.volume,
            centroid.x,
            centroid.y,
            centroid.z,
            axial.x - m * (d.y * d.y + d.z * d.z),
            axial.y - m * (d.z * d.z + d.x * d.x),
            axial.z - m * (d.x * d.x + d.y * d.y),
            products.x - m * d.y * d.z,
            products.y - m * d.z * d.x,
            products.z - m * d.x * d.y,
        ])
    })
}

pub fn area(h: &super::Handle, relative_accuracy: f64) -> Result<f64, i32> {
    let acc = accuracy(relative_accuracy)?;
    with_reader(|k| {
        let mut b = body(h);
        if !has_solid(&b) {
            return faces_measure(k, &mut b, acc).map(|(area, _)| area);
        }
        // `brep_area` sums the faces at a coarser accuracy than asked (6126.0027 against 6126.1057 for
        // a cylinder at 1e-8); the properties integral counts a sheet's faces once too, and honours it.
        let mid = calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default()).map_err(|e| status(&e))?.mid();
        properties(k, &mut b, acc, mid).map(|p| p.area)
    })
}

pub fn bounds(h: &super::Handle) -> Result<([f64; 3], [f64; 3]), i32> {
    with_reader(|k| {
        let opts = BoundingBoxOptions { tight: true, expand_by_zone_tol_3d: false, ..BoundingBoxOptions::default() };
        let e = calculate_bounding_box(k, &mut *body(h), opts).map_err(|e| status(&e))?;
        Ok(([0, 1, 2].map(|i| e.min_at(i)), [0, 1, 2].map(|i| e.max_at(i))))
    })
}

/// What each [`crate::CheckLevel`] runs: `SmBrep::ValidatePointers` at SMLib's test level 0, 1 and
/// 4 (`SM_LEVEL_0`, `_1`, `_GIVEN`). solidcore has no port of `AssertValid` over the topology
/// graph, so no level runs more than the pointer and topology tests.
fn test_level(level: i32) -> Result<u8, i32> {
    match level {
        0 => Ok(0),
        1 => Ok(1),
        2 => Ok(4),
        _ => Err(ERR_ARG),
    }
}

pub fn check(h: &super::Handle, level: i32, buf: &mut [u8]) -> Result<(usize, i64), (i32, usize)> {
    let level = test_level(level).map_err(|s| (s, 0))?;
    with_reader(|k| {
        let mut reports = AssertArray::new();
        let mut b = body(h);
        let keys = keys(h, &b);
        validate_pointers(k, &mut b, Some(&mut reports), level, false, &*keys).map_err(|e| (status(&e), 0))?;
        let mut text = String::new();
        let mut failures = 0i64;
        for r in reports.reports().iter().filter(|r| !r.ok) {
            failures += 1;
            let _ = writeln!(text, "SmBrep::Test #{} {}: {}", r.test_index, r.name, r.message.as_deref().unwrap_or(""));
        }
        if text.len() > buf.len() {
            return Err((ERR_BUFFER, text.len()));
        }
        buf[..text.len()].copy_from_slice(text.as_bytes());
        Ok((text.len(), failures))
    })
}

/// How far edge `edge` (an index in the body's edge order) lies from the faces it bounds, over their
/// own domains: the largest edge/face gap of its uses (`GetMaxUpDimGap3d`), SMLib's own. `face` is
/// the face asked about; the caller's sheets hold one.
pub fn edge_face_distance(h: &super::Handle, edge: i64, face: i64) -> Result<f64, i32> {
    let (Ok(edge), Ok(face)) = (usize::try_from(edge), usize::try_from(face)) else { return Err(ERR_ARG) };
    with_reader(|k| {
        let mut b = body(h);
        if face >= b.face_list().len() {
            return Err(ERR_ARG);
        }
        let e = *b.edge_list().get(edge).ok_or(ERR_ARG)?;
        let gap = max_up_dim_gap_3d(k, &mut b, e, false, None).map_err(|e| status(&e))?;
        Ok(gap.max.map_or(0.0, |g| g.length()))
    })
}

/// The volume of `b` to a thousandth, or `None` where the integral does not converge (the check
/// that asks for it then has nothing to compare).
pub(super) fn volume_of(k: &Kernel, b: &mut Brep) -> Option<f64> {
    properties(k, b, 1e-3, Point3::new(0.0, 0.0, 0.0)).ok().map(|p| p.volume)
}
