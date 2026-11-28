//! One signed mesh-volume accumulation, also used by reconstruction.
//!
//! The rounded value retains reconstruction's original arithmetic and input order.
//! A requested enclosure uses the existing outward-rounded interval primitives,
//! including coordinate subtraction, so no sign follows from a guessed tolerance.

use crate::{blend::fillet_section::interval::Ivl, math::Vec3};

pub(crate) struct Volume {
    origin: Vec3,
    six: f64,
    enclosure: Option<Ivl>,
}

impl Volume {
    pub(crate) fn new(origin: Vec3) -> Self {
        Self {
            origin,
            six: 0.,
            enclosure: None,
        }
    }

    pub(crate) fn certified(origin: Vec3) -> Self {
        Self {
            enclosure: Some(Ivl::from(0.)),
            ..Self::new(origin)
        }
    }

    pub(crate) fn add(&mut self, [a, b, c]: [Vec3; 3]) {
        self.six += (a - self.origin).dot((b - self.origin).cross(c - self.origin));
        if let Some(sum) = &mut self.enclosure {
            let relative = |p: Vec3| {
                [
                    Ivl::from(p.x).sub_ulp(Ivl::from(self.origin.x)),
                    Ivl::from(p.y).sub_ulp(Ivl::from(self.origin.y)),
                    Ivl::from(p.z).sub_ulp(Ivl::from(self.origin.z)),
                ]
            };
            let [ax, ay, az] = relative(a);
            let [bx, by, bz] = relative(b);
            let [cx, cy, cz] = relative(c);
            let x = by.mul_ulp(cz).sub_ulp(bz.mul_ulp(cy));
            let y = bz.mul_ulp(cx).sub_ulp(bx.mul_ulp(cz));
            let z = bx.mul_ulp(cy).sub_ulp(by.mul_ulp(cx));
            *sum = sum.add_ulp(ax.mul_ulp(x).add_ulp(ay.mul_ulp(y)).add_ulp(az.mul_ulp(z)));
        }
    }

    pub(crate) fn value(&self) -> f64 {
        self.six / 6.
    }

    pub(crate) fn sign(&self) -> Option<core::cmp::Ordering> {
        let bound = self.enclosure?;
        if !bound.is_finite() {
            None
        } else if bound.lo > 0. {
            Some(core::cmp::Ordering::Greater)
        } else if bound.hi < 0. {
            Some(core::cmp::Ordering::Less)
        } else {
            None
        }
    }
}
