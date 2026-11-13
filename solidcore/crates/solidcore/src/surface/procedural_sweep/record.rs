//! Screw definitions reconstructed from raw coefficients and certified against their nets.
//! Serialized bounds are never evidence: both the section and the complete net are
//! re-enclosed from the stored numbers before the surface is returned: a saved net on its
//! regenerated net's basis by their control nets span by span, any other by Bezier cells.
//! A face's record keeps its wall over the face's own turns alone ([`ProceduralSweep::to_record_over`]),
//! so a wall split into many faces is not written once per face in full.
//! Deviations: no upstream counterpart. No recursion; counts use the screw-net budgets.

use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::recertify::{
    mapped_clamped_net_distance, same_basis, same_basis_net_distance,
};
use crate::error::{Result, SolidError};
use crate::eval::geometry::sweep as r;
use crate::geom::extent2d::Extent2d;
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

    /// [`ProceduralSweep::to_record`] over the face box `uv` (in the net's own parameters):
    /// the same profile, motion and law, with only the stations whose spans the box's turns
    /// cross and the net's control points over those spans, and the bound certified for that
    /// piece. A face cut from a long wall then carries its own turns, not the whole wall's. A
    /// wall the piece would not be the same surface of in the same parameters (an offset
    /// wall, a remapped turn parameter, a net not on its stations) goes whole.
    ///
    /// # Errors
    /// Those of [`ProceduralSweep::to_record`]; a net whose counts do not hold together.
    pub fn to_record_over(
        &self,
        net: &BSplineSurface,
        uv: &Extent2d,
    ) -> Result<r::ProceduralSweepRecord> {
        restricted(self.to_record(net)?, uv)
    }

    /// Reconstructs a saved screw surface and re-certifies its actual net against its
    /// exact definition. A claimed bound smaller than the newly established bound is
    /// replaced by the conservative certified bound; it never authorises net evaluation.
    ///
    /// # Errors
    /// Refuses malformed data, unsupported offset sources, singular maps, oversized
    /// records and nets whose relation to the definition cannot be certified.
    pub fn from_record(record: &r::ProceduralSweepRecord) -> Result<Surface> {
        let (mut definition, net) = Self::proved(record)?;
        if definition.offset.is_none() {
            definition.bound = record.bound.max(definition.bound);
        }
        let mut result = Surface::from(net);
        result.set_sweep_definition(definition);
        Ok(result)
    }

    /// The definition and net a saved screw record holds, the definition's bound the
    /// certificate of the net against it alone, the record's claimed bound no operand.
    fn proved(record: &r::ProceduralSweepRecord) -> Result<(Self, BSplineSurface)> {
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
            definition.bound = certified;
        }
        Ok((definition, net))
    }
}

/// `whole` restricted to the stations `t_i0 <= .. <= t_i1` whose spans hold the face box
/// `uv`'s turns, `i0` the last station at or below the box and `i1` the first at or above it,
/// and its net to the control points over them. The record is kept whole where that is not
/// the same wall in the same parameters: an offset wall (its net is refined independently of
/// its stations), a map that does not take the net's turn parameter to the definition's as
/// the identity, or turn knots that are not the stations (triple at the ends, double
/// between, degree 2).
///
/// **Why it is the same surface.** The wall's definition on a span reads only that span's two
/// stations ([`ProceduralSweep::turn_at`]: its middle and width) and the motion and law, which
/// are absolute in the turn angle; so on the kept spans the restricted definition is the
/// whole one. On the net, a station is a knot of multiplicity 2 in degree 2: there the net
/// interpolates its control point, and the spans on either side read disjoint control points
/// but that one. The control points `2 i0 ..= 2 i1` with the knots
/// `t_i0 (3), t_i0+1 (2), .., t_i1 (3)` are then the same piecewise rational map over
/// `[t_i0, t_i1]`, every number copied, in the same parameters: every point, pcurve and name
/// on the face reads as it did. The box lies within `[t_i0, t_i1]` by the choice of `i0` and
/// `i1`, compared on the knots themselves.
///
/// **The bound.** Not the whole wall's, inherited: the piece's own certificate
/// ([`ProceduralSweep::proved`], the proof [`ProceduralSweep::from_record`] reads it back
/// with), the definition on the kept stations certified ([`ProceduralSweep::certify`]) plus
/// the kept net's enclosed distance from the net regenerated on those stations, so the bound
/// is sound whatever the whole wall's was. Against the whole wall's certificate: each term of
/// `certify` (the largest half angle, the law's largest change per span, the floor's largest
/// turn) is a largest over the stations, here over fewer of them, so none is larger; the kept
/// net is the whole net's own coefficients on the kept spans, and the regenerated one differs
/// from the whole regenerated net's only at the piece's last station, where the regeneration
/// places the end of the motion as its cap rather than turning the point (a rounding of the
/// point, which the floor's smaller largest turn exceeds). The regression test checks every
/// piece's certificate against its whole wall's.
fn restricted(whole: r::ProceduralSweepRecord, uv: &Extent2d) -> Result<r::ProceduralSweepRecord> {
    let r::SweepDefinitionRecord::Screw { offset, .. } = &whole.definition;
    let identity = whole.map_a.get(1) == Some(&0.0) && whole.map_b.get(1) == Some(&1.0);
    if offset.is_some() || !identity {
        return Ok(whole);
    }
    let n = &whole.net;
    let (u_degree, v_degree) = (n.u_degree, n.v_degree);
    let (degree, knots, [lo, hi]) = if whole.swapped {
        (n.u_degree, &n.u_knots, [uv.u_min(), uv.u_max()])
    } else {
        (n.v_degree, &n.v_knots, [uv.v_min(), uv.v_max()])
    };
    let st = &whole.stations;
    let last = st.len().saturating_sub(1);
    let pattern = |stations: &[f64]| -> Vec<f64> {
        let end = stations.len().saturating_sub(1);
        stations
            .iter()
            .enumerate()
            .flat_map(|(i, &t)| std::iter::repeat_n(t, if i == 0 || i == end { 3 } else { 2 }))
            .collect()
    };
    let on_stations = |knots: &[f64]| {
        let expected = pattern(st);
        knots.len() == expected.len()
            && knots
                .iter()
                .zip(&expected)
                .all(|(a, b)| a.to_bits() == b.to_bits())
    };
    if degree != 2 || last == 0 || !on_stations(knots) {
        return Ok(whole);
    }
    let i0 = st
        .partition_point(|&t| t <= lo)
        .saturating_sub(1)
        .min(last - 1);
    let i1 = st.partition_point(|&t| t < hi).max(i0 + 1).min(last);
    if i0 == 0 && i1 == last {
        return Ok(whole);
    }
    let stations = st
        .get(i0..=i1)
        .ok_or_else(|| bad("a face's turns are past its wall's stations"))?
        .to_vec();
    let rows = usize::try_from(n.u_count).map_err(|_| bad("a screw net's count"))?;
    let columns = usize::try_from(n.v_count).map_err(|_| bad("a screw net's count"))?;
    let kept = |k: usize| k >= 2 * i0 && k <= 2 * i1;
    let cell = |at: usize| {
        let (i, j) = (at.checked_div(columns)?, at.checked_rem(columns)?);
        Some(if whole.swapped { kept(i) } else { kept(j) })
    };
    if n.points.len() != rows.saturating_mul(columns)
        || n.weights
            .as_ref()
            .is_some_and(|w| w.len() != n.points.len())
    {
        return Err(bad("a screw net's points are not its counts"));
    }
    let pick = |at: usize| cell(at).unwrap_or(false);
    let points = n
        .points
        .iter()
        .enumerate()
        .filter(|&(at, _)| pick(at))
        .map(|(_, p)| *p)
        .collect();
    let weights = n.weights.as_ref().map(|w| {
        w.iter()
            .enumerate()
            .filter(|&(at, _)| pick(at))
            .map(|(_, w)| *w)
            .collect()
    });
    let count = u32::try_from(2 * (i1 - i0) + 1).map_err(|_| bad("a screw net's count"))?;
    let knots = pattern(&stations);
    let (u_count, v_count, u_knots, v_knots) = if whole.swapped {
        (count, n.v_count, knots, n.v_knots.clone())
    } else {
        (n.u_count, count, n.u_knots.clone(), knots)
    };
    let half = stations
        .windows(2)
        .fold(0.0_f64, |largest, pair| match pair {
            [a, b] => largest.max((b - a) * 0.5),
            _ => largest,
        });
    let mut part = r::ProceduralSweepRecord {
        net: r::NurbsSurfaceRecord {
            u_degree,
            v_degree,
            u_count,
            v_count,
            u_knots,
            v_knots,
            points,
            weights,
        },
        stations,
        half,
        ..whole
    };
    part.bound = ProceduralSweep::proved(&part)?.0.bound;
    Ok(part)
}
