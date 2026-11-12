//! Screw definitions reconstructed from raw coefficients and certified against their nets.
//! Serialized bounds are never evidence: both the section and the complete net are
//! re-enclosed from the stored numbers before the surface is returned: a saved net on its
//! regenerated net's basis by their control nets span by span, any other by Bezier cells.
//! Deviations: no upstream counterpart. No recursion; counts use the screw-net budgets.

use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::recertify::{
    mapped_clamped_net_distance, same_basis, same_basis_net_distance,
};
use crate::error::{Result, SolidError};
use crate::eval::geometry::sweep as r;
use crate::math::Vec3;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::math::gamma;
use crate::surface::procedural_blend::{convert, record as raw};
use crate::surface::surface::Surface;
use crate::sweep::screw::{screw_net, MAX_LAW, MAX_NET_POINTS, MAX_STATIONS};

use super::offset_profile::OffsetProfile;
use super::{Motion, ProceduralSweep, RadialLaw, ScrewMotion, SweepMap};

const OP: &str = "surface::procedural_sweep::record";

fn bad(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

fn raw_curve(r: &r::NurbsCurveRecord) -> raw::NurbsCurveRecord {
    raw::NurbsCurveRecord {
        dim: r.dim,
        points_3d: r.points_3d,
        degree: r.degree,
        knots: r.knots.clone(),
        points: r.points.clone(),
        weights: r.weights.clone(),
    }
}

fn curve(r: &r::NurbsCurveRecord) -> Result<BSplineCurve> {
    if r.points.len() > MAX_NET_POINTS || r.knots.len() > MAX_NET_POINTS + 17 || r.degree > 16 {
        return Err(bad("a saved screw profile exceeds its resource budget"));
    }
    let raw = raw_curve(r);
    raw.validate("profile")
        .map_err(|_| bad("invalid saved screw profile"))?;
    convert::curve_of(&raw)
}

fn curve_record(c: &BSplineCurve) -> Result<r::NurbsCurveRecord> {
    let raw = convert::curve_record(c)?;
    Ok(r::NurbsCurveRecord {
        dim: raw.dim,
        points_3d: raw.points_3d,
        degree: raw.degree,
        knots: raw.knots,
        points: raw.points,
        weights: raw.weights,
    })
}

fn net(r: &r::NurbsSurfaceRecord) -> Result<BSplineSurface> {
    if r.points.len() > MAX_NET_POINTS
        || r.u_knots.len() > MAX_NET_POINTS + 17
        || r.v_knots.len() > MAX_NET_POINTS + 17
        || r.u_degree > 16
        || r.v_degree > 16
    {
        return Err(bad("a saved screw net exceeds its resource budget"));
    }
    let raw = raw::NurbsSurfaceRecord {
        u_degree: r.u_degree,
        v_degree: r.v_degree,
        u_count: r.u_count,
        v_count: r.v_count,
        u_knots: r.u_knots.clone(),
        v_knots: r.v_knots.clone(),
        points: r.points.clone(),
        weights: r.weights.clone(),
    };
    raw.validate("net")
        .map_err(|_| bad("invalid saved screw net"))?;
    convert::surface_of(&raw)
}

fn net_record(net: &BSplineSurface) -> Result<r::NurbsSurfaceRecord> {
    let raw = convert::surface_record(net)?;
    Ok(r::NurbsSurfaceRecord {
        u_degree: raw.u_degree,
        v_degree: raw.v_degree,
        u_count: raw.u_count,
        v_count: raw.v_count,
        u_knots: raw.u_knots,
        v_knots: raw.v_knots,
        points: raw.points,
        weights: raw.weights,
    })
}

impl ProceduralSweep {
    /// Certifies an existing screw net against this exact definition and attaches
    /// the definition to the returned surface. This is the same bounded
    /// reconstruction and interval proof used when restoring a saved record.
    /// Call before face construction so subsequent reversals and parameter edits
    /// carry the definition map together with the net.
    ///
    /// # Errors
    /// Refuses non-screw definitions, malformed or oversized nets, unsupported
    /// sections and any net relationship not established by the certificate.
    pub fn certify_on_net(&self, net: &BSplineSurface) -> Result<Surface> {
        Self::from_record(&self.to_record(net)?)
    }

    /// Exports this screw definition and its actual net, retaining every raw coefficient.
    ///
    /// # Errors
    /// A general-path definition has no screw record; malformed internal data is refused.
    pub fn to_record(&self, net: &BSplineSurface) -> Result<r::ProceduralSweepRecord> {
        let Motion::Screw(motion) = &self.motion else {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "a general-path sweep has no screw-definition record",
            });
        };
        let law = motion.law().map(|law| r::RadialLawRecord {
            direction: law.direction().to_array(),
            angles: law.angles().to_vec(),
            offsets: law.offsets().to_vec(),
        });
        let offset = self
            .offset
            .as_ref()
            .map(|offset| -> Result<_> {
                Ok(r::ScrewOffsetRecord {
                    source: curve_record(&offset.source)?,
                    signed_distance: offset.distance,
                    approximation_bound: offset.bound,
                })
            })
            .transpose()?;
        Ok(r::ProceduralSweepRecord {
            definition: r::SweepDefinitionRecord::Screw {
                profile: curve_record(&self.profile)?,
                motion: r::ScrewMotionRecord {
                    origin: motion.origin().to_array(),
                    axis: motion.axis().to_array(),
                    pitch: motion.pitch(),
                    law,
                },
                offset,
            },
            net: net_record(net)?,
            stations: self.stations.clone(),
            half: self.half,
            bound: self.bound,
            map_a: self.map.a,
            map_b: self.map.b,
            swapped: self.map.swapped,
        })
    }

    /// Reconstructs a saved screw surface and re-certifies its actual net against its
    /// exact definition. A claimed bound smaller than the newly established bound is
    /// replaced by the conservative certified bound; it never authorises net evaluation.
    ///
    /// # Errors
    /// Refuses malformed data, unsupported offset sources, singular maps, oversized
    /// records and nets whose relation to the definition cannot be certified.
    pub fn from_record(record: &r::ProceduralSweepRecord) -> Result<Surface> {
        if !record.bound.is_finite()
            || record.bound <= 0.0
            || !record.half.is_finite()
            || record.half <= 0.0
            || record.stations.len() as f64 > MAX_STATIONS + 1.0
            || !record
                .map_a
                .iter()
                .chain(&record.map_b)
                .all(|x| x.is_finite())
            || record.map_b.contains(&0.0)
        {
            return Err(bad("invalid saved screw metadata"));
        }
        let r::SweepDefinitionRecord::Screw {
            profile,
            motion,
            offset,
        } = &record.definition;
        let profile = curve(profile)?;
        let law = motion
            .law
            .as_ref()
            .map(|law| {
                if law.angles.len() > MAX_LAW || law.offsets.len() > MAX_LAW {
                    return Err(bad("saved radial law exceeds its budget"));
                }
                RadialLaw::new(
                    Vec3::from(law.direction),
                    law.angles.clone(),
                    law.offsets.clone(),
                )
            })
            .transpose()?;
        let saved_axis = Vec3::from(motion.axis);
        // The writer stores the already-normalized axis. Re-normalizing it on
        // read changes valid coefficients by an ulp, so preserve its bits after
        // proving the stored vector fits the normalization-rounding invariant.
        // Three squares, two sums, sqrt, reciprocal and three component products,
        // with outward-rounded recomputation of the squared norm, fit gamma_32.
        // Inputs outside this bound are not certified outputs of that normalizer.
        let q = Ivl::from(saved_axis.x) * Ivl::from(saved_axis.x)
            + Ivl::from(saved_axis.y) * Ivl::from(saved_axis.y)
            + Ivl::from(saved_axis.z) * Ivl::from(saved_axis.z);
        let g = gamma(32);
        let limit = Ivl::checked(g, g).hi;
        if !q.lo.is_finite() || !q.hi.is_finite() || (q - Ivl::from(1.0)).mag() > limit {
            return Err(bad("saved screw axis is not a certified normalized axis"));
        }
        let mut motion =
            ScrewMotion::new(Vec3::from(motion.origin), saved_axis, motion.pitch, law)?;
        motion.axis = saved_axis;
        let half = record
            .stations
            .windows(2)
            .fold(0.0_f64, |largest, pair| match pair {
                [a, b] => largest.max((b - a) * 0.5),
                _ => largest,
            });
        let mut definition = Self::create(&profile, motion.clone(), &record.stations, half)?;
        if let Some(offset) = offset {
            if !offset.signed_distance.is_finite()
                || !offset.approximation_bound.is_finite()
                || offset.approximation_bound < 0.0
                || motion.law().is_some()
            {
                return Err(bad("invalid saved screw offset source"));
            }
            let mut source = OffsetProfile {
                source: curve(&offset.source)?,
                distance: offset.signed_distance,
                bound: 0.0,
            };
            source.bound = source.certify(&profile, &motion)?;
            definition.invalidate_runtime_allowance();
            definition.offset = Some(source);
        }
        definition.invalidate_runtime_allowance();
        definition.map = SweepMap {
            a: record.map_a,
            b: record.map_b,
            swapped: record.swapped,
        };
        let net = net(&record.net)?;
        if definition.offset.is_some() {
            // Offset walls may have refined net cuts without changing the original
            // screw stations. Rebuild the certificate from the actual saved net;
            // a claimed record bound is never an operand in this proof.
            let pieces = super::refined_net::saved_reference_pieces(&definition, &net)?;
            definition.bound = super::refined_net::certify_refined(&definition, &net, pieces)?;
        } else {
            let expected = screw_net(&profile, &motion, &record.stations)?;
            // A wall's saved net is on its regenerated net's own basis (the same knots,
            // read through an identity map, transposed when swapped) unless it was cut or
            // reparametrized. On one basis their control nets bound their distance span
            // by span; any other net is refined to Bezier cells against the regenerated one.
            let (a, b, swapped) = (record.map_a, record.map_b, record.swapped);
            let discrepancy = if same_basis(&net, &expected, a, b, swapped)? {
                same_basis_net_distance(&net, &expected, swapped)?
            } else {
                mapped_clamped_net_distance(&net, &expected, a, b, swapped)?
            };
            // `create` stored `certify()` as the bound, and the certificate reads the
            // profile, motion, stations and half angle alone (not the map set since, and
            // there is no offset here): the same value, not derived twice
            let certified = (Ivl::from(definition.bound) + Ivl::from(discrepancy)).hi;
            if !certified.is_finite() {
                return Err(bad("saved screw net error is not certified finite"));
            }
            definition.bound = record.bound.max(certified);
        }
        let mut result = Surface::from(net);
        result.set_sweep_definition(definition);
        Ok(result)
    }
}
