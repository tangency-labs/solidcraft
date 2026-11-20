//! Record-vector façade for exact tensor atlas preparation and qualification.
pub use super::Tensor;
pub use kernel::{atlas_domains, quarters, rectangle, restricted, singular_corner, trim, Trim};
use solidcore::geom::form_fill::rechart::chart as kernel;
use solidcore::Result;
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::Vec3;

/// Original ordered homogeneous point evaluation, without a control-net copy.
pub fn point<C>(
    degree: [usize; 2],
    knots: &[Vec<f64>; 2],
    count: [usize; 2],
    controls: &[C],
    uv: [f64; 2],
    components: impl Fn(&C) -> [f64; 4],
) -> Option<Vec3> {
    kernel::point(degree, knots, count, controls, uv, components).map(record_vec3)
}
/// One exact tensor and its kernel-owned closest-point start cache.
pub struct Source(kernel::Source);

/// Distinguishes a failed candidate build from a measured miss on a valid candidate.
pub enum FitError<E> {
    /// The tolerance or trim size is outside the supported range.
    InvalidInput,
    /// Source chart preparation or error measurement failed.
    Kernel(solidcore::SolidError),
    /// The supplied boundary-constrained builder failed.
    Build(E),
    /// Worst sampled distance after all permitted guide updates.
    Distance(f64),
}

impl Source {
    /// Validate the original source and prepare its reusable evaluator.
    pub fn new(tensor: Tensor) -> Result<Self> {
        kernel::Source::new(tensor).map(Self)
    }
    /// Fit the supplied trim to its exact boundary, adding independently measured
    /// source points as guides on each round. The builder returns the candidate
    /// and the corresponding evaluators in patch order.
    pub fn fit<T, E>(
        &self,
        wire: &[[f64; 2]],
        tolerance: f64,
        guide_limit: usize,
        mut build: impl FnMut(&[Vec3]) -> std::result::Result<(T, Vec<Self>), E>,
    ) -> std::result::Result<(T, f64), FitError<E>> {
        if !tolerance.is_finite()
            || tolerance <= 0.0
            || tolerance > 1.0
            || !(3..=16).contains(&wire.len())
        {
            return Err(FitError::InvalidInput);
        }
        let domain = trim(wire).map_err(FitError::Kernel)?.domain;
        let mut guides = self.guides(wire, domain).map_err(FitError::Kernel)?;
        let mut worst = f64::INFINITY;
        for _ in 0..3 {
            let (candidate, fitted) = build(&guides).map_err(FitError::Build)?;
            let (distance, misses) = self
                .measure(wire, domain, &fitted)
                .map_err(FitError::Kernel)?;
            if distance <= tolerance {
                return Ok((candidate, distance));
            }
            worst = distance;
            let next = append_guides(&guides, &misses, tolerance, guide_limit)
                .map_err(FitError::Kernel)?;
            if next.len() == guides.len() {
                break;
            }
            guides = next;
        }
        Err(FitError::Distance(worst))
    }

    /// Source's original parameter domain.
    pub fn domain(&self) -> [[f64; 2]; 2] {
        self.0.domain()
    }
    /// Point in the exact source chart.
    pub fn point(&self, uv: [f64; 2]) -> Result<Vec3> {
        self.0.point(uv).map(record_vec3)
    }
    /// Directed leave and normal vectors for a boundary station.
    pub fn frame(&self, uv: [f64; 2], tangent: Vec3) -> Option<[Vec3; 2]> {
        self.0
            .frame(uv, vec3(tangent))
            .map(|frame| frame.map(record_vec3))
    }
    /// Interior source guides in the original trim.
    pub fn guides(&self, wire: &[[f64; 2]], domain: [[f64; 2]; 2]) -> Result<Vec<Vec3>> {
        self.0
            .guides(wire, domain)
            .map(|points| points.into_iter().map(record_vec3).collect())
    }
    /// Bidirectional sampled atlas error, preserving worst-first source target order.
    pub fn measure(
        &self,
        wire: &[[f64; 2]],
        domain: [[f64; 2]; 2],
        fitted: &[Self],
    ) -> Result<(f64, Vec<(f64, Vec3)>)> {
        self.0
            .measure(wire, domain, fitted.iter().map(|source| &source.0))
            .map(|(gap, misses)| {
                (
                    gap,
                    misses
                        .into_iter()
                        .map(|(gap, point)| (gap, record_vec3(point)))
                        .collect(),
                )
            })
    }
}
/// Keep the existing guide limit and kernel distance-based target deduplication.
pub fn append_guides(
    guides: &[Vec3],
    misses: &[(f64, Vec3)],
    tolerance: f64,
    limit: usize,
) -> Result<Vec<Vec3>> {
    // The producer's measure has a bounded number of retained validation stations.
    // Preflight before copying either caller-controlled buffer.
    kernel::check_guide_sizes(guides.len(), misses.len(), limit)?;
    let mut output = guides.iter().copied().map(vec3).collect();
    let misses = misses
        .iter()
        .map(|&(gap, point)| (gap, vec3(point)))
        .collect::<Vec<_>>();
    kernel::append_guides(&mut output, &misses, tolerance, limit)?;
    Ok(output.into_iter().map(record_vec3).collect())
}
