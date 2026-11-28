//! A face's screw-sweep definition, the record's ([`r::SweepDefinition`]) and the kernel's
//! ([`ks::SweepRecord`]), both ways: the same fields, copied. The kernel reads a definition off
//! a face ([`solidcore::surface::procedural_sweep::record::face_sweep`]) and gives it back, its
//! bound certified again ([`solidcore::surface::procedural_sweep::record::adopt_sweeps`]).
//!
//! The profile is the record's 3D spline (non-rational: the stored points; rational: the stored
//! weighted points and their weights, as `[x w, y w, z w, w]`, which is how both hold it), so
//! nothing is divided or multiplied on the way.

use solidcore::surface::procedural_blend::record::NurbsCurveRecord;
use solidcore::surface::procedural_sweep::record as ks;
use solidcore_record::brep::sweep as r;
use solidcore_record::brep::Curve;

/// The kernel's definition as the record's; `None` for a profile that is not a 3D curve with
/// z, or whose weights are not one per control point.
#[must_use]
pub fn record_sweep(d: &ks::SweepRecord) -> Option<r::SweepDefinition> {
    let c = &d.profile;
    if c.dim != 3 || !c.points_3d {
        return None;
    }
    let profile = match &c.weights {
        None => Curve::Spline {
            knots: c.knots.clone(),
            points: c.points.clone(),
        },
        Some(w) if w.len() == c.points.len() => Curve::RationalSpline {
            knots: c.knots.clone(),
            homogeneous: c
                .points
                .iter()
                .zip(w)
                .map(|(p, w)| [p[0], p[1], p[2], *w])
                .collect(),
        },
        Some(_) => return None,
    };
    Some(r::SweepDefinition {
        profile,
        motion: r::SweepMotion::Screw {
            origin: d.origin,
            axis: d.axis,
            pitch: d.pitch,
            law: d.law.as_ref().map(|l| r::RadialLaw {
                direction: l.direction,
                angles: l.angles.clone(),
                offsets: l.offsets.clone(),
            }),
        },
        stations: d.stations.clone(),
        half_angles: d.halves.clone(),
        map: r::SweepMap {
            a: d.map.0,
            b: d.map.1,
            swapped: d.map.2,
        },
        bound: d.bound,
    })
}

/// The record's definition as the kernel's; `None` for a profile that is not a spline (which a
/// validated record never holds).
#[must_use]
pub fn sweep(d: &r::SweepDefinition) -> Option<ks::SweepRecord> {
    let (knots, points, weights) = match &d.profile {
        Curve::Spline { knots, points } => (knots, points.clone(), None),
        Curve::RationalSpline { knots, homogeneous } => (
            knots,
            homogeneous.iter().map(|h| [h[0], h[1], h[2]]).collect(),
            Some(homogeneous.iter().map(|h| h[3]).collect()),
        ),
        _ => return None,
    };
    let degree = knots.len().checked_sub(points.len() + 1)?;
    let r::SweepMotion::Screw {
        origin,
        axis,
        pitch,
        law,
    } = &d.motion;
    Some(ks::SweepRecord {
        profile: NurbsCurveRecord {
            dim: 3,
            points_3d: true,
            degree: u32::try_from(degree).ok()?,
            knots: knots.clone(),
            points,
            weights,
        },
        origin: *origin,
        axis: *axis,
        pitch: *pitch,
        law: law.as_ref().map(|l| ks::LawRecord {
            direction: l.direction,
            angles: l.angles.clone(),
            offsets: l.offsets.clone(),
        }),
        stations: d.stations.clone(),
        halves: d.half_angles.clone(),
        map: (d.map.a, d.map.b, d.map.swapped),
        bound: d.bound,
    })
}
