//! The least scale of a corner's setbacks at which its G1 fill is certified convex (the
//! fork's `SmBlendSetback::CertifiedScale`, `sm_SbModelRounds`; tangency-labs/smlib
//! 250cb3f, f1d81f4; smlib#104).
//!
//! The corner's rounds are made from its geometry: edge i's round, of radius r between its
//! faces f and g (normals n out of the material), has its ball's centre C at
//! `V + t a + s r (n_f + n_g) / (1 + n_f . n_g)` at t along the edge from the vertex V
//! (`s = -1` at a convex edge, the ball in the material; `+1` at a concave one), its section
//! the arc from `C - s r n_f` to `C - s r n_g` about C, the rational quadratic through the
//! edge's point with the weight cos of half the arc, `(1 + n_f . n_g) / 2` under the root. A
//! round runs from its end section away from the vertex, linear along.
//!
//! Deviations: the fork's status, `rdScale`, `*plSolves` and `*ppReason` are one return
//! value ([`CertifiedScale`]) or a [`Refusal`]; the solves of a refused search are not
//! returned.

use super::graph::faces_common_point;
use super::{el, fill_hole_probe, smax, smin, Edge, Face, FillStats, Probe, Refusal, Round};
use crate::error::Result;
use crate::geom::vector3d::sqrt;
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::nurbs::b_spline_curve::KnotType;
use crate::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData, SurfParamType,
};
use crate::surface::surface::Surface;

/// What [`certified_scale`] finds.
#[derive(Debug, Clone, PartialEq)]
pub struct CertifiedScale {
    /// `rdScale`: the least scale that certifies (to `1e-2`, on a grid of `1e-3`), 1 where
    /// the setbacks themselves do; 0 where none up to 2 does.
    pub scale: f64,
    /// `*plSolves`: the quadratic programmes solved.
    pub solves: usize,
    /// `*ppReason`: why none certifies, with a scale of 0.
    pub reason: Option<&'static str>,
}

/// `sm_SbModelRounds`: the corner's constant circular rounds, one per edge, each ending at
/// its setback (module docs).
fn model_rounds(
    faces: &[Face],
    edges: &[Edge],
    setbacks: &[f64],
) -> core::result::Result<Vec<BSplineSurface>, Refusal> {
    let n = edges.len();
    if n < 3 || faces.len() != n || setbacks.len() != n {
        return Err(Refusal::invalid(
            "SmBlendSetback: a corner needs three or more edges, as many faces and setbacks",
        ));
    }
    // the vertex: the faces' planes' common point (least squares)
    let v = faces_common_point(faces).ok_or_else(|| {
        Refusal::invalid("SmBlendSetback: the corner's faces meet in no one point")
    })?;
    let mut surfaces = Vec::with_capacity(n);
    for (i, edge) in edges.iter().enumerate() {
        let [f, g] = edge.faces;
        let setback = el(setbacks, i)?;
        if f >= n || g >= n || !(edge.radius > 0.0) || !(setback > 0.0) {
            return Err(Refusal::invalid(
                "SmBlendSetback: an edge needs a positive radius, a positive setback and two of the faces given",
            ));
        }
        let nf = el(faces, f)?.normal;
        let ng = el(faces, g)?.normal;
        // convex where the other edge on face g runs into face f's material
        let mut side = 0.0;
        for (j, other) in edges.iter().enumerate() {
            if j != i && (other.faces[0] == g || other.faces[1] == g) {
                side = other.away.dot(nf);
            }
        }
        if side == 0.0 {
            return Err(Refusal::invalid(
                "SmBlendSetback: an edge's face holds no other edge of the corner",
            ));
        }
        let s = if side < 0.0 { -1.0 } else { 1.0 };
        let r = edge.radius;
        let c = nf.dot(ng);
        if !(1.0 + c > 1.0e-9) {
            return Err(Refusal::invalid(
                "SmBlendSetback: an edge's faces fold onto each other",
            ));
        }
        let sw = (nf + ng) * (s * r / (1.0 + c));
        let w = sqrt(0.5 * (1.0 + c));
        let len = 2.0 * r;
        let mut pts: Vec<Point3> = Vec::with_capacity(6);
        let mut wts: Vec<f64> = Vec::with_capacity(6);
        for k in 0..2 {
            let se = v + edge.away * (setback + f64::from(k) * len);
            let sc = se + sw;
            pts.push(sc - nf * (s * r));
            pts.push(se);
            pts.push(sc - ng * (s * r));
            wts.push(1.0);
            wts.push(w);
            wts.push(1.0);
        }
        let um = [2, 2];
        let vm = [3, 3];
        let uk = [setback, setback + len];
        let vk = [0.0, 1.0];
        let surface = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 1,
            v_degree: 2,
            control_points: &pts,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &um,
            v_multiplicities: &vm,
            u_knots: &uk,
            v_knots: &vk,
            knot_type: KnotType::Unspecified,
            weights: Some(&wts),
            uv_domain: None,
        })?;
        surfaces.push(surface);
    }
    Ok(surfaces)
}

/// The corner whose model is filled (`CertifiedScale`'s arguments, for its `at` and
/// `model`).
struct Model<'a> {
    kernel: &'a Kernel,
    faces: &'a [Face],
    edges: &'a [Edge],
    setbacks: &'a [f64],
    most: Option<&'a [f64]>,
    size: f64,
}

impl Model<'_> {
    /// `at`: the setbacks at scale k, each k times its own, held at its most.
    fn at(&self, k: f64) -> Result<Vec<f64>> {
        let mut out = self.setbacks.to_vec();
        for (i, x) in out.iter_mut().enumerate() {
            *x *= k;
            if let Some(m) = self.most {
                *x = smin(*x, el(m, i)?);
            }
        }
        Ok(out)
    }

    /// `model`: the model's fill at scale k, its probe at degree 9 (`probe`), or the fill
    /// itself (taken certified: `true`).
    fn run(
        &self,
        k: f64,
        probe: Option<&mut Probe>,
        solves: &mut usize,
    ) -> core::result::Result<bool, Refusal> {
        let ks = self.at(k)?;
        let surfaces: Vec<Surface> = model_rounds(self.faces, self.edges, &ks)?
            .into_iter()
            .map(Surface::from)
            .collect();
        let mut rounds = Vec::with_capacity(surfaces.len());
        for (surface, &end) in surfaces.iter().zip(&ks) {
            rounds.push(Round {
                surface,
                along: SurfParamType::U,
                end,
                corner_above: false,
            });
        }
        let tol = 1.0e-9 * (1.0 + k * self.size);
        let mut as_fill = Probe {
            fill: true,
            ..Probe::default()
        };
        let is_probe = probe.is_some();
        let mut stats = FillStats::default();
        let result = fill_hole_probe(
            self.kernel,
            self.faces,
            &rounds,
            tol,
            probe.unwrap_or(&mut as_fill),
            &mut stats,
        );
        *solves += stats.solves;
        match result {
            Err(_) if !is_probe && stats.uncertified => Ok(false),
            Err(refusal) => Err(refusal),
            Ok(_) => Ok(!is_probe),
        }
    }
}

/// `margin`: the least uniform violation of the fill's sector conditions at scale k.
fn margin(
    model: &mut impl FnMut(f64, Option<&mut Probe>) -> core::result::Result<bool, Refusal>,
    k: f64,
) -> core::result::Result<f64, Refusal> {
    let mut probe = Probe {
        margin: true,
        ..Probe::default()
    };
    model(k, Some(&mut probe))?;
    Ok(probe.margin_value)
}

/// `SmBlendSetback::CertifiedScale`: the least scale k of the setbacks `setbacks` (one per
/// edge, the edges and faces as for [`super::distances`], every face a plane through the
/// corner's vertex, every edge a straight line there), each held at its most (`most`), at
/// which the G1 fill of the corner's constant circular rounds (each round's end section
/// square to its edge at its setback) is certified convex ([`super::fill_hole`]), to `1e-2`:
/// 1 where the setbacks themselves are; else the certifying end of a bracket `1e-2` wide,
/// found from the margin by which the fill's conditions fail (at 1, 1.02, their secant's
/// root, then the parabola's), up to 2; 0, with its reason, where none up to 2 is.
///
/// # Errors
/// [`Refusal`] with `SM_ERR_INVALID_INPUT` and its reason where the corner is not one of
/// planes and straight edges, or `most` is not one per edge; a failing fill's refusal.
#[allow(clippy::too_many_lines)] // the fork's one function
pub fn certified_scale(
    kernel: &Kernel,
    faces: &[Face],
    edges: &[Edge],
    setbacks: &[f64],
    most: Option<&[f64]>,
) -> core::result::Result<CertifiedScale, Refusal> {
    let mut solves = 0;
    let mut size = 0.0;
    for &x in setbacks {
        size = smax(size, x.abs());
    }
    if most.is_some_and(|m| m.len() != setbacks.len()) {
        return Err(Refusal::invalid(
            "SmBlendSetback: one most setback per edge",
        ));
    }
    let ctx = Model {
        kernel,
        faces,
        edges,
        setbacks,
        most,
        size,
    };
    let mut model = |k: f64, probe: Option<&mut Probe>| ctx.run(k, probe, &mut solves);
    // The search, from the margin m(k) by which the fill's sector conditions fail at degree 9
    // (m <= 0: met, and the conditions only grow easier with k): m at 1 and 1.02, then at
    // their secant's root, then the root of the parabola through the three; the certifying
    // end checked a half percent above it, the failing end the bracket's 1e-2 below.
    let m1 = margin(&mut model, 1.0)?;
    if m1 <= 0.0 && model(1.0, None)? {
        return Ok(CertifiedScale {
            scale: 1.0,
            solves,
            reason: None,
        });
    }
    let mut sk = vec![1.0];
    let mut sm = vec![m1];
    let mut lo = 1.0; // the largest scale known to fail
    let mut predict = 0.0;
    for step in 0..3 {
        let mut k;
        if step == 0 {
            k = 1.02;
        } else if step == 1 {
            let slope = (el(&sm, 1)? - el(&sm, 0)?) / (el(&sk, 1)? - el(&sk, 0)?);
            k = if slope < 0.0 {
                el(&sk, 1)? - el(&sm, 1)? / slope
            } else {
                2.0
            };
        } else {
            // the parabola through the three points, its root past the last
            let (k0, k1, k2) = (el(&sk, 0)?, el(&sk, 1)?, el(&sk, 2)?);
            let (m0, mm1, m2) = (el(&sm, 0)?, el(&sm, 1)?, el(&sm, 2)?);
            let d01 = (mm1 - m0) / (k1 - k0);
            let d12 = (m2 - mm1) / (k2 - k1);
            let a = (d12 - d01) / (k2 - k0);
            let b = d12 - a * (k1 + k2);
            let c = m2 - k2 * (a * k2 + b);
            let mut root = 0.0;
            if a.abs() > 1.0e-12 {
                let disc = b * b - 4.0 * a * c;
                if disc >= 0.0 {
                    let r1 = (-b - sqrt(disc)) / (2.0 * a);
                    let r2 = (-b + sqrt(disc)) / (2.0 * a);
                    // the root nearest past the last scale that fails
                    let (ra, rb) = (smin(r1, r2), smax(r1, r2));
                    root = if ra > lo { ra } else { rb };
                }
            }
            if !(root > lo) {
                root = if d12 < 0.0 { k2 - m2 / d12 } else { 2.0 };
            }
            predict = root;
            break;
        }
        k = smin(smax(k, lo * 1.001), 2.0);
        let m = margin(&mut model, k)?;
        sk.push(k);
        sm.push(m);
        if m > 0.0 {
            lo = smax(lo, k);
        }
        if step == 1 {
            predict = k;
        }
    }
    // the certifying end: a half percent past the prediction (and past the last failing
    // scale), stepping up by 1e-2 while it does not certify, up to 2
    let mut hi = smax(predict * 1.005, lo * 1.01);
    loop {
        if hi > 2.0 {
            hi = 2.0;
        }
        if model(hi, None)? {
            break;
        }
        lo = hi;
        if hi >= 2.0 {
            return Ok(CertifiedScale {
                scale: 0.0,
                solves,
                reason: Some(
                    "SmBlendSetback: no G1 fill is certified convex at the corner's setbacks up to twice them",
                ),
            });
        }
        hi *= 1.01;
    }
    // the failing end within 1e-2 below it
    while hi > lo * 1.01 {
        let k = hi / 1.01;
        let m = margin(&mut model, k)?;
        if m > 0.0 {
            break;
        }
        if !model(k, None)? {
            break;
        }
        hi = k;
    }
    // on a grid of 1e-3, up (certifying still: the conditions only grow easier with k), so
    // that one corner described two ways (its faces and edges in another order) gets one
    // scale
    Ok(CertifiedScale {
        scale: (hi * 1000.0).ceil() / 1000.0,
        solves,
        reason: None,
    })
}
