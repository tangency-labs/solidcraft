//! Identity of stationary contact solutions, independent of modelling tolerance.
//!
//! Not upstream. The search keeps its gap and normal tests. Only merging repeated
//! candidates is proved here: equal parameters, or the same certified stationary root.
//! Krawczyk's operator is the kernel's shared rounded operator (Rump, Verification
//! methods, 2010, section 13). A root enclosure and its larger uniqueness domain are
//! distinct objects; overlap alone never establishes identity.
//!
//! Deviations: eight inflation steps and eight refinements, with cancellation. Unresolved
//! identity is refused. Charts without a continuous first derivative or an exact
//! enclosure keep the existing boundary and singular-contact search.

use super::{stationary_system, tp_evaluate, TpEval};
use crate::blend::fillet_section::certificate::{knot_cuts, nurbs_partials_over_box};
use crate::blend::fillet_section::interval::{round_up, Ivl};
use crate::blend::fillet_section::series::{v_add, v_scale, V3};
use crate::blend::fillet_section::station::Nurbs;
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::krawczyk::{contraction, inverse_of_columns4, k_operator, KRAWCZYK_STEPS};
use crate::math::Point2;
use crate::surface::plane::Plane;
use crate::surface::surface::exact_pieces::exact_nurbs_surface;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::Kernel;

const OP: &str = "intersect::adv_surface_intersector::contact_identity";
const REFINEMENTS: usize = 8;
type Box4 = [Ivl; 4];

fn unproved() -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason: "the identity of stationary contact candidates cannot be certified",
    }
}

/// An accepted candidate with its original parameter representative.
pub(super) struct Contact {
    pub(super) uv: [Point2; 2],
    root: Option<Root>,
}

impl Contact {
    pub(super) fn new(uv: [Point2; 2]) -> Self {
        Self { uv, root: None }
    }
}

/// Proof data is built only when unequal accepted parameter pairs need comparing.
#[derive(Default)]
pub(super) struct Comparator(Option<Pair>);

impl Comparator {
    pub(super) fn new(k: &Kernel, surfaces: [&Surface; 2], domains: [Extent2d; 2]) -> Result<Self> {
        Ok(Self(Pair::new(k, surfaces, domains)?))
    }

    pub(super) fn supports(&self, uv: [Point2; 2]) -> bool {
        self.0.as_ref().is_some_and(|pair| {
            uv.into_iter().zip(pair.domains).all(|(p, d)| {
                p.x > d.min().x && p.x < d.max().x && p.y > d.min().y && p.y < d.max().y
            })
        })
    }

    pub(super) fn same(
        &mut self,
        k: &Kernel,
        surfaces: [&Surface; 2],
        old: &mut Contact,
        new: &mut Contact,
    ) -> Result<bool> {
        if old.uv == new.uv {
            return Ok(true);
        }
        let pair = self.0.as_ref().ok_or_else(unproved)?;
        if old.root.is_none() {
            old.root = Some(pair.certify(k, surfaces, old.uv)?.ok_or_else(unproved)?);
        }
        if new.root.is_none() {
            new.root = Some(pair.certify(k, surfaces, new.uv)?.ok_or_else(unproved)?);
        }
        let a = old.root.as_mut().ok_or_else(unproved)?;
        let b = new.root.as_mut().ok_or_else(unproved)?;
        for step in 0..=REFINEMENTS {
            k.check_cancelled(OP)?;
            if contains(a.unique, b.enclosure) || contains(b.unique, a.enclosure) {
                return Ok(true);
            }
            if separated(pair.position(a.enclosure)?, pair.position(b.enclosure)?) {
                return Ok(false);
            }
            let hull = core::array::from_fn(|i| {
                let x = a.enclosure.get(i).copied().unwrap_or(Ivl::whole());
                let y = b.enclosure.get(i).copied().unwrap_or(Ivl::whole());
                Ivl::new(x.lo.min(y.lo), x.hi.max(y.hi))
            });
            let (_, j) = pair.system(hull)?;
            if contraction(&a.inverse, &j).1 < 1.0 {
                return Ok(true);
            }
            if step < REFINEMENTS {
                a.refine(pair)?;
                b.refine(pair)?;
            }
        }
        Err(unproved())
    }
}

fn contains(outer: Box4, inner: Box4) -> bool {
    inner
        .iter()
        .zip(outer)
        .all(|(x, y)| x.lo >= y.lo && x.hi <= y.hi)
}

fn separated(a: V3<Ivl>, b: V3<Ivl>) -> bool {
    [(a.x, b.x), (a.y, b.y), (a.z, b.z)]
        .iter()
        .any(|(x, y)| x.hi < y.lo || y.hi < x.lo)
}

enum Bounds {
    Plane(Plane),
    Net(Nurbs),
}

impl Bounds {
    fn of(k: &Kernel, mut surface: &Surface, domain: Extent2d) -> Result<Option<Self>> {
        for _ in 0..32 {
            match surface.kind() {
                SurfaceKind::CurveBounded(b) => surface = b.basis(),
                SurfaceKind::Plane(p) => return Ok(Some(Self::Plane(p.clone()))),
                _ => {
                    let Some(s) = exact_nurbs_surface(k, surface)? else {
                        return Ok(None);
                    };
                    let Ok(n) = Nurbs::of(&s) else {
                        return Ok(None);
                    };
                    if knot_cuts(&n.u, n.p, n.nu, domain.min().x, domain.max().x, 1).is_ok()
                        && knot_cuts(&n.v, n.q, n.nv, domain.min().y, domain.max().y, 1).is_ok()
                    {
                        return Ok(Some(Self::Net(n)));
                    }
                    return Ok(None);
                }
            }
        }
        Ok(None)
    }

    fn at(&self, u: Ivl, v: Ivl) -> Result<TpEval<Ivl>> {
        match self {
            Self::Net(n) => {
                // F is continuous on a C1 net. Its Jacobian uses both one-sided
                // second derivatives, so the mean-value enclosure holds piecewise.
                let p = nurbs_partials_over_box(n, u.lo, u.hi, v.lo, v.hi, 2, 1)?;
                Ok(TpEval {
                    p: p.at(0, 0),
                    pu: p.at(1, 0),
                    pv: p.at(0, 1),
                    puu: p.at(2, 0),
                    puv: p.at(1, 1),
                    pvv: p.at(0, 2),
                })
            }
            Self::Plane(p) => {
                let pu = v_scale(
                    V3::from_vec3(p.position().x_axis()),
                    Ivl::from(p.uv_scale().x),
                );
                let pv = v_scale(
                    V3::from_vec3(p.position().y_axis()),
                    Ivl::from(p.uv_scale().y),
                );
                let point = v_add(
                    v_add(V3::from_vec3(p.position().origin()), v_scale(pu, u)),
                    v_scale(pv, v),
                );
                let zero = V3::from_vec3(crate::math::Vec3::ZERO);
                Ok(TpEval {
                    p: point,
                    pu,
                    pv,
                    puu: zero,
                    puv: zero,
                    pvv: zero,
                })
            }
        }
    }
}

struct Pair {
    surfaces: [Bounds; 2],
    offset: [f64; 4],
    scale: [f64; 4],
    domains: [Extent2d; 2],
}

impl Pair {
    fn new(k: &Kernel, surfaces: [&Surface; 2], domains: [Extent2d; 2]) -> Result<Option<Self>> {
        let [a, b] = domains;
        let [sa, sb] = surfaces;
        let scale = [a.size().x, a.size().y, b.size().x, b.size().y];
        if !scale.iter().all(|x| x.is_finite() && *x > 0.0) {
            return Ok(None);
        }
        let (Some(sa), Some(sb)) = (Bounds::of(k, sa, a)?, Bounds::of(k, sb, b)?) else {
            return Ok(None);
        };
        Ok(Some(Self {
            surfaces: [sa, sb],
            offset: [a.min().x, a.min().y, b.min().x, b.min().y],
            scale,
            domains,
        }))
    }

    fn parameters(&self, x: Box4) -> Box4 {
        core::array::from_fn(
            |i| match (self.offset.get(i), self.scale.get(i), x.get(i)) {
                (Some(&o), Some(&s), Some(&v)) => Ivl::from(o) + Ivl::from(s) * v,
                _ => Ivl::whole(),
            },
        )
    }

    fn jets(&self, x: Box4) -> Result<[TpEval<Ivl>; 2]> {
        let [u1, v1, u2, v2] = self.parameters(x);
        for ([u, v], d) in [[u1, v1], [u2, v2]].into_iter().zip(self.domains) {
            if !(u.lo >= d.min().x && u.hi <= d.max().x && v.lo >= d.min().y && v.hi <= d.max().y) {
                return Err(unproved());
            }
        }
        Ok([self.surfaces[0].at(u1, v1)?, self.surfaces[1].at(u2, v2)?])
    }

    fn system(&self, x: Box4) -> Result<([Ivl; 4], [[Ivl; 4]; 4])> {
        let [a, b] = self.jets(x)?;
        let (f, mut j) = stationary_system(a, b);
        for row in &mut j {
            for (v, &s) in row.iter_mut().zip(&self.scale) {
                *v = *v * Ivl::from(s);
            }
        }
        Ok((f, j))
    }

    fn position(&self, x: Box4) -> Result<V3<Ivl>> {
        let [a, b] = self.jets(x)?;
        let p = v_scale(v_add(a.p, b.p), Ivl::from(0.5));
        if [p.x, p.y, p.z].iter().all(|v| v.is_finite()) {
            Ok(p)
        } else {
            Err(unproved())
        }
    }

    fn certify(
        &self,
        k: &Kernel,
        surfaces: [&Surface; 2],
        uv: [Point2; 2],
    ) -> Result<Option<Root>> {
        let raw = [uv[0].x, uv[0].y, uv[1].x, uv[1].y];
        let mid: [f64; 4] =
            core::array::from_fn(
                |i| match (raw.get(i), self.offset.get(i), self.scale.get(i)) {
                    (Some(&v), Some(&o), Some(&s)) => (v - o) / s,
                    _ => f64::NAN,
                },
            );
        let (_, j) = stationary_system(
            tp_evaluate(k, surfaces[0], uv[0])?,
            tp_evaluate(k, surfaces[1], uv[1])?,
        );
        let columns = core::array::from_fn(|c| {
            core::array::from_fn(|r| {
                j.get(r)
                    .and_then(|row| row.get(c))
                    .copied()
                    .unwrap_or(f64::NAN)
                    * self.scale.get(c).copied().unwrap_or(f64::NAN)
            })
        });
        let Some(inverse) = inverse_of_columns4(&columns) else {
            return Ok(None);
        };
        let (f, _) = self.system(mid.map(Ivl::from))?;
        let mut radii = inverse.map(|row| {
            row.iter()
                .zip(f)
                .fold(Ivl::from(0.0), |v, (&c, f)| v + Ivl::from(c) * f)
                .mag()
        });
        for (r, m) in radii.iter_mut().zip(mid) {
            *r = round_up(2.0 * *r + 32.0 * f64::EPSILON * (1.0 + m.abs()));
        }
        for _ in 0..KRAWCZYK_STEPS {
            k.check_cancelled(OP)?;
            let x = core::array::from_fn(|i| match (mid.get(i), radii.get(i)) {
                (Some(&m), Some(&r)) => Ivl::new(
                    (Ivl::from(m) - Ivl::from(r)).lo,
                    (Ivl::from(m) + Ivl::from(r)).hi,
                ),
                _ => Ivl::whole(),
            });
            let (_, j) = self.system(x)?;
            let (c, q) = contraction(&inverse, &j);
            let image = k_operator(&mid, &inverse, &f, &c, &x);
            if q < 1.0
                && image
                    .iter()
                    .zip(x)
                    .all(|(a, b)| a.is_finite() && a.lo > b.lo && a.hi < b.hi)
            {
                return Ok(Some(Root {
                    enclosure: image,
                    unique: x,
                    inverse,
                }));
            }
            for ((r, v), m) in radii.iter_mut().zip(image).zip(mid) {
                *r = round_up(1.5 * r.max((v.lo - m).abs().max((v.hi - m).abs())));
            }
        }
        Ok(None)
    }
}

struct Root {
    enclosure: Box4,
    unique: Box4,
    inverse: [[f64; 4]; 4],
}

impl Root {
    fn refine(&mut self, pair: &Pair) -> Result<()> {
        let mid = self.enclosure.map(Ivl::mid);
        let (_, j) = pair.system(self.enclosure)?;
        // Recentring keeps the residual's point inside the box used for the Jacobian.
        let (f, _) = pair.system(mid.map(Ivl::from))?;
        let (c, _) = contraction(&self.inverse, &j);
        let image = k_operator(&mid, &self.inverse, &f, &c, &self.enclosure);
        for (x, y) in self.enclosure.iter_mut().zip(image) {
            *x = Ivl::new(x.lo.max(y.lo), x.hi.min(y.hi));
            if !(x.is_finite() && x.lo <= x.hi) {
                return Err(unproved());
            }
        }
        Ok(())
    }
}
