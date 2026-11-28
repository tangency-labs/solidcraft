//! The B-spline a curve is held as, for the kinds that are one (`IsKindOf(SmBSplineCurve_TYPE)`).

use std::borrow::Cow;

use super::{Curve, CurveKind};
use crate::kernel::Kernel;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::Result;

impl Curve {
    /// The `SmBSplineCurve` part of a curve that is one (`IsKindOf(SmBSplineCurve_TYPE)`), with
    /// its NURB (`GetOrCreateGwNurbPointer`): a B-spline as it is; a line, an ellipse, a hyperbola
    /// or a parabola with its NURB, made on a copy when it has none yet. `None` for the other
    /// kinds.
    ///
    /// # Errors
    /// The kernel's error making the NURB of a line or conic.
    pub fn bspline_part(&self, k: &Kernel) -> Result<Option<Cow<'_, BSplineCurve>>> {
        Ok(Some(match self.kind() {
            CurveKind::BSpline(b) => Cow::Borrowed(b),
            CurveKind::Line(l) => l.base_with_nurb()?,
            CurveKind::Ellipse(e) if e.base().gw_nurb().is_none() => {
                let mut made = e.clone();
                made.make_nurb(k)?;
                Cow::Owned(made.base().clone_exact())
            }
            CurveKind::Ellipse(e) => Cow::Borrowed(e.base()),
            CurveKind::Hyperbola(h) if h.base().gw_nurb().is_none() => {
                let mut made = h.clone();
                made.make_nurb(None)?;
                Cow::Owned(made.base().clone_exact())
            }
            CurveKind::Hyperbola(h) => Cow::Borrowed(h.base()),
            CurveKind::Parabola(p) if p.base().gw_nurb().is_none() => {
                let mut made = p.clone();
                made.make_nurb(None)?;
                Cow::Owned(made.base().clone_exact())
            }
            CurveKind::Parabola(p) => Cow::Borrowed(p.base()),
            _ => return Ok(None),
        }))
    }
}
