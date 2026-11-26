//! solidcore's round-tube copes ([`TubeCopes`]): each the solidcore-ops call this crate's callers
//! made before the interface existed, its refusals kept whole.

use solidcore_ops::tube_cope as ops;
use solidcore_record::tube_cope::{Box3, Cope, Fit, Mate, Template, Terminal};
use solidcore_record::{Plane, Vec3};
use solidcraft_kernel_api::tube_cope::{CopeFailed, CopeRefusal, TemplateRefusal, TubeCopes};

use super::Solidcore;

fn failed(e: solidcore_ops::Error) -> CopeFailed {
    CopeFailed(e.to_string())
}

fn refusal(r: ops::Refusal) -> CopeRefusal {
    match r {
        ops::Refusal::OffAxis { distance } => CopeRefusal::OffAxis { distance },
        ops::Refusal::Smaller { mate_diameter, tube_diameter } => CopeRefusal::Smaller { mate_diameter, tube_diameter },
        ops::Refusal::Parallel { sine } => CopeRefusal::Parallel { sine },
        ops::Refusal::PastTerminal => CopeRefusal::PastTerminal,
        ops::Refusal::MateEnd => CopeRefusal::MateEnd,
        ops::Refusal::MateCope => CopeRefusal::MateCope,
        ops::Refusal::MeetsOther => CopeRefusal::MeetsOther,
    }
}

fn template_refusal(r: ops::TemplateRefusal) -> TemplateRefusal {
    match r {
        ops::TemplateRefusal::TooManySegments => TemplateRefusal::TooManySegments,
        ops::TemplateRefusal::OutsideDepth => TemplateRefusal::OutsideDepth,
        ops::TemplateRefusal::Cancelled => TemplateRefusal::Cancelled,
    }
}

impl TubeCopes for Solidcore {
    const MIN_SINE: f64 = ops::MIN_SINE;

    fn terminal_at(&self, plane: &Plane, length: f64, at_end: bool) -> Result<Terminal, CopeFailed> {
        ops::terminal_at(plane, length, at_end).map_err(failed)
    }

    fn terminal_ok(&self, t: &Terminal) -> bool {
        ops::terminal_ok(t)
    }

    fn mate_ok(&self, m: &Mate) -> bool {
        ops::mate_ok(m)
    }

    fn line_box(&self, a: Vec3, b: Vec3, pad: f64) -> Box3 {
        ops::line_box(a, b, pad)
    }

    fn arc_box(&self, a: Vec3, center: Vec3, pad: f64) -> Box3 {
        ops::arc_box(a, center, pad)
    }

    fn fit(&self, t: &Terminal, radius: f64, m: &Mate, others: &[Box3]) -> Result<Result<Fit, CopeRefusal>, CopeFailed> {
        Ok(ops::fit(t, radius, m, others).map_err(failed)?.map_err(refusal))
    }

    fn template(
        &self,
        cope: &Cope<'_>,
        seam: f64,
        tolerance: f64,
        max_segments: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Result<Template, TemplateRefusal>, CopeFailed> {
        Ok(ops::template(cope, seam, tolerance, max_segments, cancelled).map_err(failed)?.map_err(template_refusal))
    }

    fn ends_cross(&self, start: f64, end: f64, length: f64) -> bool {
        ops::ends_cross(start, end, length)
    }
}
