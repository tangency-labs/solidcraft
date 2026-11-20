//! Portable linkage jets; polynomial targets and derivatives are computed by the kernel.
use solidcore::geom::form_fill::linkage as kernel;
use solidcore_convert::vec3;
use solidcore_record::Vec3;

pub use kernel::{Target, MAX_CONTINUITY};
/// A surface-height jet in the directed tangent frame, through order four.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Jet {
    /// Highest retained geometric continuity order.
    pub order: usize,
    /// Binary height forms of orders two through four.
    pub forms: [[f64; MAX_CONTINUITY + 1]; MAX_CONTINUITY - 1],
}
impl Jet {
    fn kernel(&self) -> kernel::Jet {
        kernel::Jet {
            order: self.order,
            forms: self.forms,
        }
    }
    /// Evaluate a binary height form in its original directed frame.
    pub fn form(&self, m: usize, a: [f64; 2]) -> Option<f64> {
        self.kernel().form(m, a)
    }
    /// Whether all retained coefficients are finite.
    pub fn is_finite(&self) -> bool {
        self.kernel().is_finite()
    }
    /// Linkage targets for the supplied tangent-plane curve derivatives.
    pub fn targets(&self, x: &[[f64; 2]]) -> Option<Vec<f64>> {
        self.kernel().targets(x)
    }
    /// Linkage targets with exact forward-differentiated polynomial gradients.
    pub fn linearised(&self, x: &[[f64; 2]]) -> Option<Vec<Target>> {
        self.kernel().linearised(x)
    }
}
impl From<kernel::Jet> for Jet {
    fn from(jet: kernel::Jet) -> Self {
        Self {
            order: jet.order,
            forms: jet.forms,
        }
    }
}
/// Transfer exact partial data to the kernel's height-jet construction.
pub fn from_partials(
    d: &[[Vec3; MAX_CONTINUITY + 1]; MAX_CONTINUITY + 1],
    normal: Vec3,
    leave: Vec3,
    order: usize,
) -> Option<Jet> {
    let d = d.map(|row| row.map(vec3));
    kernel::from_partials(&d, vec3(normal), vec3(leave), order).map(Jet::from)
}
