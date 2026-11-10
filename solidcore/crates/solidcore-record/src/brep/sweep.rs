//! Procedural sweep definitions and their display nets as plain data.
//!
//! These records preserve the producer's numbers. Structural validation belongs to the
//! portable B-rep check; the kernel certifies the relationship between a net and its definition.

use serde::{Deserialize, Serialize};

/// A definition curve, with its original polynomial or rational coefficients.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NurbsCurveRecord {
    /// Image dimension.
    pub dim: u32,
    /// Whether the stored coefficients include z.
    pub points_3d: bool,
    /// Polynomial degree.
    pub degree: u32,
    /// Full knot vector.
    pub knots: Vec<f64>,
    /// Weighted coordinates when rational, unweighted coordinates otherwise.
    pub points: Vec<[f64; 3]>,
    /// Rational weights, absent for a polynomial curve.
    pub weights: Option<Vec<f64>>,
}

/// The fitted surface net, with its original coefficients.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NurbsSurfaceRecord {
    /// Degree along u.
    pub u_degree: u32,
    /// Degree along v.
    pub v_degree: u32,
    /// Control-point count along u.
    pub u_count: u32,
    /// Control-point count along v.
    pub v_count: u32,
    /// Full u knot vector.
    pub u_knots: Vec<f64>,
    /// Full v knot vector.
    pub v_knots: Vec<f64>,
    /// Coefficients in u-major order, weighted when rational.
    pub points: Vec<[f64; 3]>,
    /// Rational weights, absent for a polynomial surface.
    pub weights: Option<Vec<f64>>,
}

/// A piecewise linear radial displacement, held constant beyond its endpoints.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadialLawRecord {
    /// Unit displacement direction.
    pub direction: [f64; 3],
    /// Strictly ascending angles in radians.
    pub angles: Vec<f64>,
    /// Displacement at each angle.
    pub offsets: Vec<f64>,
}

/// A screw motion about an oriented axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScrewMotionRecord {
    /// A point on the axis.
    pub origin: [f64; 3],
    /// Unit axis direction.
    pub axis: [f64; 3],
    /// Signed rise per full turn.
    pub pitch: f64,
    /// Optional radial displacement law.
    pub law: Option<RadialLawRecord>,
}

/// The exact source of an approximated normal-offset section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScrewOffsetRecord {
    /// Original affine profile, before the normal offset.
    pub source: NurbsCurveRecord,
    /// Signed normal-offset distance.
    pub signed_distance: f64,
    /// Certified section approximation error.
    pub approximation_bound: f64,
}

/// The construction defining a sweep; each motion has its own data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SweepDefinitionRecord {
    /// A profile carried by a screw motion.
    Screw {
        /// Profile used to construct the fitted net.
        profile: NurbsCurveRecord,
        /// Screw motion.
        motion: ScrewMotionRecord,
        /// Exact offset source, when the profile approximates an offset section.
        offset: Option<ScrewOffsetRecord>,
    },
}

/// A sweep's definition, fitted net and certified relationship, in the face's parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralSweepRecord {
    /// Exact defining construction.
    pub definition: SweepDefinitionRecord,
    /// Fitted display and intersection net.
    pub net: NurbsSurfaceRecord,
    /// Net stations along the turn, in radians.
    pub stations: Vec<f64>,
    /// Half angle of the net's spans.
    pub half: f64,
    /// Certified net-to-definition distance; the kernel verifies it on reconstruction.
    pub bound: f64,
    /// Affine parameter-map origins.
    pub map_a: [f64; 2],
    /// Affine parameter-map scales.
    pub map_b: [f64; 2],
    /// Whether the face's u and v exchange roles in the map.
    pub swapped: bool,
}

/// Dynamic definition data copied by a reconstructed curve referencing this owner.
pub(super) fn reference_cost(s: &ProceduralSweepRecord) -> super::Result<usize> {
    fn add(total: &mut usize, n: usize) -> super::Result<()> {
        *total = total.checked_add(n).ok_or(super::Invalid("item budget"))?;
        Ok(())
    }
    fn curve(total: &mut usize, c: &NurbsCurveRecord) -> super::Result<()> {
        add(total, c.knots.len())?;
        add(total, c.points.len())?;
        if let Some(weights) = &c.weights {
            add(total, weights.len())?;
        }
        Ok(())
    }
    let mut total = s.stations.len();
    let SweepDefinitionRecord::Screw {
        profile,
        motion,
        offset,
    } = &s.definition;
    curve(&mut total, profile)?;
    if let Some(law) = &motion.law {
        add(&mut total, law.angles.len())?;
        add(&mut total, law.offsets.len())?;
    }
    if let Some(offset) = offset {
        curve(&mut total, &offset.source)?;
    }
    Ok(total)
}

/// Dynamic owner data copied while building the cached exact evaluator.
pub(super) fn evaluator_cost(s: &ProceduralSweepRecord) -> super::Result<usize> {
    let mut n = reference_cost(s)?;
    for count in [
        s.net.u_knots.len(),
        s.net.v_knots.len(),
        s.net.points.len(),
        s.net.weights.as_ref().map_or(0, Vec::len),
    ] {
        n = n.checked_add(count).ok_or(super::Invalid("item budget"))?;
    }
    Ok(n)
}

/// Checks counts, finite numbers and resource budgets, without certifying geometry.
pub(super) fn check(s: &ProceduralSweepRecord, budget: &mut super::Budget) -> super::Result<()> {
    use super::{numbers, points, require};

    net(&s.net, budget)?;
    match &s.definition {
        SweepDefinitionRecord::Screw {
            profile,
            motion,
            offset,
        } => {
            curve(profile, budget)?;
            points(&[motion.origin, motion.axis], budget)?;
            numbers([motion.pitch])?;
            require(motion.pitch != 0.0, "zero screw pitch")?;
            if let Some(law) = &motion.law {
                points(&[law.direction], budget)?;
                budget.add(law.angles.len())?;
                budget.add(law.offsets.len())?;
                numbers(law.angles.iter().chain(&law.offsets).copied())?;
                require(
                    !law.angles.is_empty() && law.angles.len() == law.offsets.len(),
                    "screw radial-law counts",
                )?;
                require(
                    law.angles.windows(2).all(|w| matches!(w, [a, b] if a < b)),
                    "screw radial-law angles",
                )?;
            }
            if let Some(offset) = offset {
                curve(&offset.source, budget)?;
                numbers([offset.signed_distance, offset.approximation_bound])?;
                require(
                    offset.approximation_bound >= 0.0,
                    "negative screw section bound",
                )?;
            }
        }
    }
    budget.add(s.stations.len())?;
    numbers(s.stations.iter().copied())?;
    numbers([s.half, s.bound])?;
    numbers(s.map_a.into_iter().chain(s.map_b))?;
    require(s.half > 0.0 && s.bound >= 0.0, "sweep half angle or bound")?;
    require(s.map_b.iter().all(|b| *b != 0.0), "singular sweep map")?;
    require(
        s.stations.len() >= 2 && s.stations.windows(2).all(|w| matches!(w, [a, b] if a < b)),
        "sweep stations",
    )
}

fn curve(c: &NurbsCurveRecord, budget: &mut super::Budget) -> super::Result<()> {
    super::require(c.dim == 3 && c.points_3d, "sweep profile dimension")?;
    degree(c.degree, &c.knots, c.points.len())?;
    super::knots(&c.knots, c.points.len(), budget)?;
    super::points(&c.points, budget)?;
    check_weights(c.weights.as_deref(), c.points.len(), budget)
}

fn net(n: &NurbsSurfaceRecord, budget: &mut super::Budget) -> super::Result<()> {
    let u = usize::try_from(n.u_count).map_err(|_| super::Invalid("sweep net u count"))?;
    let v = usize::try_from(n.v_count).map_err(|_| super::Invalid("sweep net v count"))?;
    degree(n.u_degree, &n.u_knots, u)?;
    degree(n.v_degree, &n.v_knots, v)?;
    super::require(u.checked_mul(v) == Some(n.points.len()), "sweep net counts")?;
    super::knots(&n.u_knots, u, budget)?;
    super::knots(&n.v_knots, v, budget)?;
    super::points(&n.points, budget)?;
    check_weights(n.weights.as_deref(), n.points.len(), budget)
}

fn degree(degree: u32, knots: &[f64], count: usize) -> super::Result<()> {
    let degree = usize::try_from(degree).map_err(|_| super::Invalid("sweep degree"))?;
    let expected = count.checked_add(degree).and_then(|n| n.checked_add(1));
    super::require(
        expected == Some(knots.len()),
        "sweep knot and degree counts",
    )
}

fn check_weights(
    weights: Option<&[f64]>,
    count: usize,
    budget: &mut super::Budget,
) -> super::Result<()> {
    if let Some(weights) = weights {
        budget.add(weights.len())?;
        super::require(weights.len() == count, "sweep weight count")?;
        super::numbers(weights.iter().copied())?;
        super::require(weights.iter().all(|w| *w >= 1e-12), "sweep rational weight")?;
    }
    Ok(())
}
