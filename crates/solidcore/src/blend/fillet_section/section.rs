//! The G^n section generator: the blend surface of the order-n section between the rails,
//! on stations at the knots of the rails and the centre line (and where a foot crosses a
//! face's knot line), certified span by span, and the section's order of contact with a
//! face at a rail.
//!
//! Fork: `SmGnSectionFSG` (`SM_GN_MAX_ORDER`, `SM_GN_DEFAULT_THUMBWEIGHT`, the constructor,
//! `IsValid`, `CreateSurface`, `CreateSurfaceFromCurves`, `Build`, `ContactOrderAt`,
//! `GetOrder`, `GetThumbweight`, `GetCertifiedBound`, `GetCertifiedSpans`) in
//! `source/SMLib/inc/SmFilletSolver.h` and `source/SMLib/src/SmFilletSection.cpp`
//! (tangency-labs/smlib@11ea2d9); see [`super`] for the table.

use super::certificate::{gn_certify_span, NurbsCurve};
use super::form::{gn_form, hermite_knots, hermite_points};
use super::series::Trunc;
use super::station::{gn_solve_station, smax, surface_partials, vec3, GnStation, Nurbs};
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, BIG_DOUBLE, PI};
use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_surface::p1::CanonicalSurfaceData;
use crate::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use crate::nurbs::knots::Side;
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "blend::fillet_section::GnSectionFsg";

/// `SM_GN_MAX_ORDER`: the highest order of a G^n section.
pub const GN_MAX_ORDER: usize = 4;
/// `SM_GN_THUMBWEIGHT_EQUAL_AREA` (the fork's 6148773 = d01d7b0's 1947a45): a value outside
/// the thumbweights' range that a generator resolves per blend to the w at which its
/// section, on the same rails, takes off the area the ball's circle does
/// ([`GnSectionFsg::equal_area_thumbweight`]).
pub const GN_THUMBWEIGHT_EQUAL_AREA: f64 = -2.0;
/// `SM_GN_DEFAULT_THUMBWEIGHT`: the equal area one, [`GN_THUMBWEIGHT_EQUAL_AREA`] (0, the
/// section with no thumbweight, before 6148773).
pub const GN_DEFAULT_THUMBWEIGHT: f64 = GN_THUMBWEIGHT_EQUAL_AREA;

/// `SmGnSectionFSG`: a blend's cross-section of contact order n with its faces along the
/// rails, `n = 2..=GN_MAX_ORDER`, with thumbweight w. It needs a ball of one radius
/// (constant offsets).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GnSectionFsg {
    /// `m_lOrder`: n.
    pub order: usize,
    /// `m_dThumbweight`: w, `-1 < w < 1`, or [`GN_THUMBWEIGHT_EQUAL_AREA`] until it is
    /// resolved (by `build` and [`GnSectionFsg::contact_order_at`], which write it here as
    /// the fork's `mutable` member is written).
    pub thumbweight: f64,
    /// The session generator this one is a solver's copy of (`None` for a generator of
    /// its own): the fork's solvers share one generator, so a thumbweight one resolves
    /// is every sharer's (the session copies it back; not a fork field).
    pub shared: Option<u32>,
    /// `m_dCertifiedBound`: the last surface built: its certified distance from the blend
    /// (-1: none yet).
    pub certified_bound: f64,
    /// `m_lCertifiedSpans`: and its number of spans along the rails.
    pub certified_spans: usize,
}

/// The faces' surfaces under a generator's rails and their uv curves.
#[derive(Clone, Copy, Debug)]
pub struct GnFaces<'a> {
    /// The faces' surfaces (`pSurf1`, `pSurf2`).
    pub surfaces: [&'a Surface; 2],
    /// The rails' uv curves on them (`pUV1`, `pUV2`).
    pub uv: [&'a BSplineCurve; 2],
}

/// The surface under an offset surface (`GetBaseSurface`), else the surface itself.
fn base_of(s: &Surface) -> &Surface {
    match s.kind() {
        SurfaceKind::OffsetSurface(o) => o.base_surface(),
        _ => s,
    }
}

/// `GetNaturalInterval` as `(min, max)`.
fn interval(c: &BSplineCurve) -> (f64, f64) {
    let i = c.natural_interval();
    (i.min(), i.max())
}

/// `EvaluateNormal(uv, TRUE, TRUE, sN)` then `Unitize` (both `SER`).
fn unit_normal(k: &Kernel, s: &Surface, uv: Point2) -> Result<Vec3> {
    s.evaluate_normal(k, uv, Side::Left, Side::Left)?.unitize()
}

/// A station's foot on face f at sigma^i eps^e.
fn foot(st: &GnStation, f: usize, i: usize, e: usize) -> Vec3 {
    let [a, b] = &st.f;
    let fs = if f == 0 { a } else { b };
    let [x, y, z] = &fs.a;
    Vec3::new(x.at(i, e), y.at(i, e), z.at(i, e))
}

/// A station's foot parameter q (u1, v1, u2, v2) at sigma^i eps^e.
fn param(st: &GnStation, q: usize, i: usize, e: usize) -> f64 {
    st.x.get(q).map_or(f64::NAN, |x| x.at(i, e))
}

/// `sm_GnSectionAreaIntegral`: the integral of `a b' - a' b` over [0, 1] for the order-n
/// section between two planes, where `S(t) = a(t) e1 + b(t) e2` with `e1`, `e2` the rails'
/// points from the edge: `a = f (1 - p1)`, `b = (1 - f) p2`. Twice the area the section
/// takes off, over `|e1 x e2|`; it does not depend on the planes' angle. Composite Simpson
/// on 2048 intervals.
fn gn_section_area_integral(n: usize, w: f64) -> f64 {
    const N: i32 = 2048;
    let n1 = (n + 1) as f64;
    let ab = |t: f64| -> (f64, f64, f64, f64) {
        let (a_, b_) = ((1.0 - t).powf(n1), t.powf(n1));
        let f = a_ / (a_ + b_);
        let dfa = -n1 * (1.0 - t).powf(n1 - 1.0);
        let dfb = n1 * t.powf(n1 - 1.0);
        let df = (dfa * b_ - a_ * dfb) / ((a_ + b_) * (a_ + b_));
        let (d1, d2) = (1.0 - w + w * t, 1.0 - w * t);
        let (p1, dp1) = (t / d1, (1.0 - w) / (d1 * d1));
        let (p2, dp2) = (t * (1.0 - w) / d2, (1.0 - w) / (d2 * d2));
        let a = f * (1.0 - p1);
        let da = df * (1.0 - p1) - f * dp1;
        let b = (1.0 - f) * p2;
        let db = -df * p2 + (1.0 - f) * dp2;
        (a, b, da, db)
    };
    let mut sum = 0.0;
    for i in 0..=N {
        let t = f64::from(i) / f64::from(N);
        let (a, b, da, db) = ab(t);
        let g = a * db - da * b;
        let c = if i == 0 || i == N {
            1.0
        } else if i % 2 != 0 {
            4.0
        } else {
            2.0
        };
        sum += c * g;
    }
    sum / (3.0 * f64::from(N))
}

impl GnSectionFsg {
    /// `SmGnSectionFSG(lOrder, dThumbweight)`.
    #[must_use]
    pub const fn new(order: usize, thumbweight: f64) -> Self {
        Self {
            order,
            thumbweight,
            shared: None,
            certified_bound: -1.0,
            certified_spans: 0,
        }
    }

    /// `IsValid`: whether an order and a thumbweight make a G^n section:
    /// `2 <= n <= GN_MAX_ORDER`, and `-1 < w < 1` (the section's weights `1 - w + w t` and
    /// `1 - w t` stay positive and its reparametrizations monotone) or the equal area
    /// default.
    #[must_use]
    pub fn is_valid(order: usize, thumbweight: f64) -> bool {
        (2..=GN_MAX_ORDER).contains(&order)
            && (thumbweight == GN_THUMBWEIGHT_EQUAL_AREA
                || (thumbweight > -1.0 && thumbweight < 1.0))
    }

    /// `EqualAreaThumbweight`: the w at which the order-n section between two planes
    /// meeting at the angle `alpha` (radians, the angle of the wedge the ball sits in,
    /// `0 < alpha < pi`) takes off, on the same rails, the area the ball's circle does:
    /// `r^2 (cot(alpha/2) - (pi - alpha)/2)`.
    ///
    /// With `e1`, `e2` the rails' points from the edge (`|e_i| = d = r cot(alpha/2)`), the
    /// section takes off `(1/2) d^2 sin(alpha) I(w)`, `I` the integral of `a b' - a' b`
    /// (`sm_GnSectionAreaIntegral`), so `I(w) = (cot(alpha/2) - (pi - alpha)/2) /
    /// (cot(alpha/2) cos^2(alpha/2))`. `I` falls as w rises, so the root is bisected on
    /// (-1, 1).
    ///
    /// # Errors
    ///
    /// `SM_ERR_INVALID_INPUT` for an order past 2..=4, an angle outside (0, pi), or a
    /// target past what the range reaches.
    pub fn equal_area_thumbweight(order: usize, alpha: f64) -> Result<f64> {
        const OP_EA: &str = "blend::fillet_section::GnSectionFsg::equal_area_thumbweight";
        if !((2..=GN_MAX_ORDER).contains(&order) && alpha > 0.0 && alpha < PI) {
            return Err(SolidError::invalid_input(OP_EA, "order or wedge angle"));
        }
        let half = 0.5 * alpha;
        let cot = half.cos() / half.sin();
        let target = (cot - 0.5 * (PI - alpha)) / (cot * half.cos() * half.cos());
        let mut lo = -1.0 + 1.0e-9;
        let mut hi = 1.0 - 1.0e-9;
        let i_lo = gn_section_area_integral(order, lo);
        let i_hi = gn_section_area_integral(order, hi);
        if !(target <= i_lo && target >= i_hi) {
            return Err(SolidError::invalid_input(
                OP_EA,
                "no thumbweight gives this wedge's section the ball's circle's area",
            ));
        }
        let mut it = 0;
        while it < 200 && hi - lo > 1.0e-15 {
            let mid = 0.5 * (lo + hi);
            if gn_section_area_integral(order, mid) > target {
                lo = mid;
            } else {
                hi = mid;
            }
            it += 1;
        }
        Ok(0.5 * (lo + hi))
    }

    /// `ResolveThumbweight`: the thumbweight used, the equal area one resolved once from
    /// the wedge at one station: the ball's centre `centre` and its contacts `rail1`,
    /// `rail2` give the wedge's angle `alpha = pi - (the angle between the radii)`. Exact
    /// for two planes; elsewhere the default of the station it is resolved at. A
    /// thumbweight given (or resolved before) is kept.
    ///
    /// # Errors
    ///
    /// `SM_ERR_INVALID_INPUT` for a radius of length 0; those of
    /// [`GnSectionFsg::equal_area_thumbweight`].
    pub fn resolve_thumbweight(
        &mut self,
        centre: Point3,
        rail1: Point3,
        rail2: Point3,
    ) -> Result<()> {
        if self.thumbweight != GN_THUMBWEIGHT_EQUAL_AREA {
            return Ok(());
        }
        let a = rail1 - centre;
        let b = rail2 - centre;
        let (la, lb) = (a.length(), b.length());
        if !(la > 0.0 && lb > 0.0) {
            return Err(SolidError::invalid_input(
                OP,
                "ResolveThumbweight: a radius of length 0",
            ));
        }
        let mut cos = a.dot(b) / (la * lb);
        cos = smax(-1.0, if 1.0 < cos { 1.0 } else { cos }); // smos_Max(-1, smos_Min(1, cos))
        let alpha = PI - cos.acos();
        let w = Self::equal_area_thumbweight(self.order, alpha)?;
        self.thumbweight = w;
        Ok(())
    }

    /// `CreateSurface`: the blend surface between the solver's rails. The solver's faces
    /// are its offset surfaces; the section is built on the faces under them (an offset
    /// surface's base, else the surface itself). The rails and their uv curves are rebuilt
    /// on the section's stations from the same exact feet, so that the surface's
    /// boundaries are the rails.
    ///
    /// # Errors
    ///
    /// Those of `Build`.
    pub fn create_surface(
        &mut self,
        k: &Kernel,
        approx_tol_3d: f64,
        center_line: &BSplineCurve,
        rails: [&mut BSplineCurve; 2],
        surfaces: [&Surface; 2],
        uvs: [&mut BSplineCurve; 2],
    ) -> Result<BSplineSurface> {
        let [s1, s2] = surfaces;
        let faces = [base_of(s1), base_of(s2)];
        self.build(k, center_line, rails, approx_tol_3d, faces, uvs, true)
    }

    /// `CreateSurfaceFromCurves`: the blend surface between two rails on the faces'
    /// surfaces, about the centre line. The rails and uv curves are not changed.
    ///
    /// # Errors
    ///
    /// Those of `Build`.
    pub fn create_surface_from_curves(
        &mut self,
        k: &Kernel,
        center_line: &BSplineCurve,
        rails: [&mut BSplineCurve; 2],
        approx_tol_3d: f64,
        faces: GnFaces<'_>,
    ) -> Result<BSplineSurface> {
        let [u1, u2] = faces.uv;
        let mut uv1 = u1.clone();
        let mut uv2 = u2.clone();
        self.build(
            k,
            center_line,
            rails,
            approx_tol_3d,
            faces.surfaces,
            [&mut uv1, &mut uv2],
            false,
        )
    }

    /// `Build`: the G^n blend surface between the rails, its stations the knots of the
    /// rails and the centre line. `replace_rails`: the rails and uv curves become the
    /// jets' own order-0 curves (the surface's boundaries). The surface's u is the rails'
    /// parameter, v the section's t, from the first rail (v = 0) to the second (v = 1).
    #[allow(clippy::too_many_arguments)]
    fn build(
        &mut self,
        k: &Kernel,
        center: &BSplineCurve,
        rails: [&mut BSplineCurve; 2],
        tol: f64,
        faces: [&Surface; 2],
        uvs: [&mut BSplineCurve; 2],
        replace_rails: bool,
    ) -> Result<BSplineSurface> {
        let n = self.order;
        if !Self::is_valid(n, self.thumbweight) {
            return Err(SolidError::invalid_input(
                OP,
                "the order must be 2 to 4 and the thumbweight in (-1, 1)",
            ));
        }
        let [rail1, rail2] = rails;
        let (c_min, c_max) = interval(center);
        let scale = smax(1.0, c_max - c_min);
        {
            // the default thumbweight resolved from the wedge at the middle station
            let mid_s = 0.5 * (c_min + c_max);
            let c = center.evaluate_point(mid_s)?;
            let r1 = rail1.evaluate_point(mid_s)?;
            let r2 = rail2.evaluate_point(mid_s)?;
            self.resolve_thumbweight(c, r1, r2)?;
        }

        // the stations: the unique knots of the rails and the centre line
        let mut ss: Vec<f64> = Vec::new();
        for c in [center, &*rail1, &*rail2] {
            let (lo, hi) = interval(c);
            if (lo - c_min).abs() > 1.0e-9 * scale || (hi - c_max).abs() > 1.0e-9 * scale {
                // "the rails and the centre line run over different intervals"
                return Err(SolidError::sm_err(OP));
            }
            ss.extend(c.knots(None)?.0);
        }
        ss.sort_by(f64::total_cmp);
        {
            let mut unique: Vec<f64> = Vec::new();
            for &s in &ss {
                if unique.last().is_none_or(|&b| s - b > 1.0e-9 * scale) {
                    unique.push(s);
                }
            }
            if let Some(f) = unique.first_mut() {
                *f = c_min;
            }
            if let Some(l) = unique.last_mut() {
                *l = c_max;
            }
            ss = unique;
        }
        let (Some(&s_first), true) = (ss.first(), ss.len() >= 2) else {
            // "the rails have no length"
            return Err(SolidError::sm_err(OP));
        };

        // the offsets and the sides, at the first station: the centre line there is the
        // ball's centre
        let mut r = [0.0_f64; 2];
        let mut side = [0.0_f64; 2];
        {
            let m = center.evaluate_point(s_first)?;
            let rl: [&BSplineCurve; 2] = [&*rail1, &*rail2];
            let rows = r
                .iter_mut()
                .zip(side.iter_mut())
                .zip(rl)
                .zip(uvs.iter())
                .zip(faces);
            for ((((rf, sf), rail), uv), face) in rows {
                let rho = rail.evaluate_point(s_first)?;
                let w = uv.evaluate_point(s_first)?;
                let nrm = unit_normal(k, face, Point2::new(w.x, w.y))?;
                let off = m - rho;
                *rf = off.length();
                *sf = if off.dot(nrm) >= 0.0 { 1.0 } else { -1.0 };
                let has_radius = *rf > tol;
                if !has_radius {
                    // "the ball has no radius at the rails' start"
                    return Err(SolidError::sm_err(OP));
                }
            }
        }

        // every station's feet and their series: values and s-derivatives (sigma order
        // 1), jets to order n
        let tr = Trunc::new(1, n)?;
        let mut stations: Vec<GnStation> = Vec::new();
        for &s in &ss {
            let w0 = uvs[0].evaluate_point(s)?;
            let w1 = uvs[1].evaluate_point(s)?;
            let seed = [w0.x, w0.y, w1.x, w1.y];
            // a constant ball: the station's centre on the centre line at the same offsets
            let m = center.evaluate_point(s)?;
            let solve_tol = 1.0e-9 * smax(1.0, m.length());
            let st = gn_solve_station(k, faces, center, s, seed, r, side, tr, solve_tol)?;
            for (f, &rf) in r.iter().enumerate() {
                let rho = foot(&st, f, 0, 0);
                if (m.distance(rho) - rf).abs() > smax(tol, 1.0e-6 * rf) {
                    return Err(SolidError::invalid_input(
                        OP,
                        "a section of order 2 and up is built for a ball of one radius only",
                    ));
                }
            }
            stations.push(st);
        }

        // the certificate
        let (stations, bound, spans) =
            self.certify(k, faces, center, stations, r, side, tr, tol)?;
        self.certified_bound = bound;
        self.certified_spans = spans;
        let ss: Vec<f64> = stations.iter().map(|st| st.s).collect();

        // the section's form
        let form = gn_form(n, self.thumbweight);
        let ld = form.w.len(); // 3n + 2 control points across
        let mut w_min = BIG_DOUBLE;
        let mut w_max = 0.0_f64;
        for &w in &form.w {
            w_min = if w_min < w { w_min } else { w }; // smos_Min
            w_max = smax(w_max, w);
        }
        let positive = w_min > 0.0;
        if !positive {
            return Err(SolidError::sm_err(OP));
        }
        let w_scale = 1.0 / crate::geom::vector3d::sqrt(w_min * w_max);

        // the jets' Hermite curves along s, all on the stations' knots
        let (u_knots, u_mult) = hermite_knots(&ss);
        let mut jets: [Vec<Vec<Vec3>>; 2] = [Vec::new(), Vec::new()];
        for (f, jf) in jets.iter_mut().enumerate() {
            for kk in 0..=n {
                let v: Vec<Vec3> = stations.iter().map(|st| foot(st, f, 0, kk)).collect();
                let d: Vec<Vec3> = stations.iter().map(|st| foot(st, f, 1, kk)).collect();
                jf.push(hermite_points(&ss, &v, &d));
            }
        }
        let jet = |f: usize, kk: usize, i: usize| -> Vec3 {
            jets.get(f)
                .and_then(|j| j.get(kk))
                .and_then(|j| j.get(i))
                .copied()
                .unwrap_or(Vec3::new(f64::NAN, f64::NAN, f64::NAN))
        };
        let lnu = jets[0].first().map_or(0, Vec::len);
        let mut cps: Vec<Point3> = Vec::new();
        let mut wts: Vec<f64> = Vec::new();
        for i in 0..lnu {
            for (j, &wj) in form.w.iter().enumerate() {
                let mut p = Vec3::new(0.0, 0.0, 0.0);
                for (f, gf) in form.g.iter().enumerate() {
                    for (kk, g) in gf.iter().enumerate().take(n + 1) {
                        p += jet(f, kk, i) * g.get(j).copied().unwrap_or(f64::NAN);
                    }
                }
                cps.push(p * (1.0 / wj));
                wts.push(wj * w_scale);
            }
        }
        let surface = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 3,
            v_degree: ld - 1,
            control_points: &cps,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &u_mult,
            v_multiplicities: &[ld, ld],
            u_knots: &u_knots,
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&wts),
            uv_domain: None,
        })?;

        if replace_rails {
            for (f, (rail, uv)) in [rail1, rail2].into_iter().zip(uvs).enumerate() {
                let rail_cps: Vec<Point3> = (0..lnu).map(|i| jet(f, 0, i)).collect();
                rail.set_canonical(&CanonicalData {
                    dim: 3,
                    degree: 3,
                    control_points: &rail_cps,
                    form: BSplineCurveForm::Unspecified,
                    knot_multiplicities: &u_mult,
                    knots: &u_knots,
                    knot_type: KnotType::Unspecified,
                    weights: None,
                    trim: None,
                })?;
                let v: Vec<Vec3> = stations
                    .iter()
                    .map(|st| Vec3::new(param(st, 2 * f, 0, 0), param(st, 2 * f + 1, 0, 0), 0.0))
                    .collect();
                let d: Vec<Vec3> = stations
                    .iter()
                    .map(|st| Vec3::new(param(st, 2 * f, 1, 0), param(st, 2 * f + 1, 1, 0), 0.0))
                    .collect();
                let uv_cps = hermite_points(&ss, &v, &d);
                let dim = uv.dim;
                uv.set_canonical(&CanonicalData {
                    dim,
                    degree: 3,
                    control_points: &uv_cps,
                    form: BSplineCurveForm::Unspecified,
                    knot_multiplicities: &u_mult,
                    knots: &u_knots,
                    knot_type: KnotType::Unspecified,
                    weights: None,
                    trim: None,
                })?;
            }
        }
        Ok(surface)
    }

    /// The certificate block of `Build`: a station wherever a foot crosses a knot line of
    /// its face, then each span's bound (`sm_GnCertifySpan`) at most a tenth of the
    /// tolerance, a span over it halved (a station solved at its middle) until none is.
    /// Returns the stations, the largest bound and the number of spans.
    #[allow(clippy::too_many_arguments)]
    fn certify(
        &self,
        k: &Kernel,
        faces: [&Surface; 2],
        center: &BSplineCurve,
        mut stations: Vec<GnStation>,
        r: [f64; 2],
        side: [f64; 2],
        tr: Trunc,
        tol: f64,
    ) -> Result<(Vec<GnStation>, f64, usize)> {
        let mut nurbs = Vec::new();
        for f in faces {
            let Some(bs) = super::station::as_b_spline(k, f)? else {
                // "a face that is not a NURBS surface: its blend of order 2 and up cannot
                // be certified"
                return Err(SolidError::sm_err(OP));
            };
            nurbs.push(Nurbs::of(&bs)?);
        }
        let (Some(n0), Some(n1)) = (nurbs.first(), nurbs.get(1)) else {
            return Err(SolidError::sm_err(OP));
        };
        let face_n = [n0, n1];
        let leader = NurbsCurve::of(center)?;
        let target = 0.1 * tol;
        let solve_tol = |s: f64| -> Result<f64> {
            let m = center.evaluate_point(s)?;
            Ok(1.0e-9 * smax(1.0, m.length()))
        };
        // a station wherever a foot crosses a knot line of its face: the face is only as
        // smooth there as the knot allows, and a span's enclosures are taken on one side
        for (f, nf) in face_n.iter().enumerate() {
            for c in 0..2 {
                let (kv, deg) = if c == 0 { (&nf.u, nf.p) } else { (&nf.v, nf.q) };
                let mut inner: Vec<f64> = Vec::new();
                let mut m = deg + 1;
                while m + deg + 1 < kv.len() {
                    let km = kv.get(m).copied().unwrap_or(f64::NAN);
                    if inner.last().is_none_or(|&b| km > b) {
                        inner.push(km);
                    }
                    m += 1;
                }
                let mut next: Vec<GnStation> = Vec::new();
                for pair in stations.windows(2) {
                    let [a, b] = pair else { continue };
                    next.push(*a);
                    for &dk in &inner {
                        let q = 2 * f + c;
                        let xa = param(a, q, 0, 0);
                        let xb = param(b, q, 0, 0);
                        let margin = 1.0e-9 * (1.0 + dk.abs());
                        let crosses = (xa < dk - margin && xb > dk + margin)
                            || (xa > dk + margin && xb < dk - margin);
                        if !crosses {
                            continue;
                        }
                        // the parameter where that foot is on the knot line: the secant,
                        // kept in its bracket
                        let (mut s0, mut s1, mut x0, mut x1) = (a.s, b.s, xa - dk, xb - dk);
                        let mut at: Option<GnStation> = None;
                        for _ in 0..60 {
                            let mut ds = s0 - x0 * (s1 - s0) / (x1 - x0);
                            let inside = ds > s0 && ds < s1;
                            if !inside {
                                ds = 0.5 * (s0 + s1);
                            }
                            let t = (ds - a.s) / (b.s - a.s);
                            let mut seed = [0.0_f64; 4];
                            for (q2, sq) in seed.iter_mut().enumerate() {
                                *sq = (1.0 - t) * param(a, q2, 0, 0) + t * param(b, q2, 0, 0);
                            }
                            let st = gn_solve_station(
                                k,
                                faces,
                                center,
                                ds,
                                seed,
                                r,
                                side,
                                tr,
                                solve_tol(ds)?,
                            )?;
                            let x = param(&st, q, 0, 0) - dk;
                            at = Some(st);
                            if x.abs() <= 1.0e-12 * (1.0 + dk.abs()) {
                                break;
                            }
                            if (x < 0.0) == (x0 < 0.0) {
                                s0 = ds;
                                x0 = x;
                            } else {
                                s1 = ds;
                                x1 = x;
                            }
                        }
                        if let Some(st) = at {
                            next.push(st);
                        }
                    }
                }
                if let Some(last) = stations.last() {
                    next.push(*last);
                }
                next.sort_by(|a, b| a.s.total_cmp(&b.s));
                stations = next;
            }
        }
        let mut bounds = vec![-1.0_f64; stations.len().saturating_sub(1)];
        loop {
            let mut all = true;
            for (j, bj) in bounds.iter_mut().enumerate() {
                if *bj < 0.0 {
                    let (Some(a), Some(b)) = (stations.get(j), stations.get(j + 1)) else {
                        return Err(SolidError::sm_err(OP));
                    };
                    *bj = gn_certify_span(
                        k, face_n, &leader, faces, center, a, b, r, side, self.order, target,
                    )?;
                }
                let within = *bj <= target;
                if !within {
                    all = false;
                }
            }
            if all {
                break;
            }
            if stations.len() > 4096 {
                // "the blend could not be certified within its tolerance"
                return Err(SolidError::sm_err(OP));
            }
            let mut next: Vec<GnStation> = Vec::new();
            let mut next_bounds: Vec<f64> = Vec::new();
            for (pair, &bj) in stations.windows(2).zip(&bounds) {
                let [a, b] = pair else { continue };
                next.push(*a);
                if bj <= target {
                    next_bounds.push(bj);
                    continue;
                }
                let mid = 0.5 * (a.s + b.s);
                let inside = mid > a.s && mid < b.s;
                if !inside {
                    return Err(SolidError::sm_err(OP));
                }
                let mut seed = [0.0_f64; 4];
                for (c, sc) in seed.iter_mut().enumerate() {
                    *sc = 0.5 * (param(a, c, 0, 0) + param(b, c, 0, 0));
                }
                let st =
                    gn_solve_station(k, faces, center, mid, seed, r, side, tr, solve_tol(mid)?)?;
                next.push(st);
                next_bounds.push(-1.0);
                next_bounds.push(-1.0);
            }
            if let Some(last) = stations.last() {
                next.push(*last);
            }
            stations = next;
            bounds = next_bounds;
        }
        let mut bound = 0.0_f64;
        for &b in &bounds {
            bound = smax(bound, b);
        }
        // (the fork's SM_ERR_MESSAGE report of the spans and the bound is not logged)
        let spans = bounds.len();
        Ok((stations, bound, spans))
    }

    /// `ContactOrderAt`: the order of contact k of this section with face `rail` (0 or 1)
    /// at the station of the centre line's parameter `s`: the power at which the section
    /// stands off the face, `c s^k`. The coefficient of `t^(n+1)` in the standoff along the
    /// face's normal `N_i` is `[(T_o(0) - rho_i) - c_i,n+1 (-1/(1 - w))^(n+1)] . N_i`:
    /// `k = n + 1` where it is not zero, `n + 2` where it is (two planes).
    ///
    /// # Errors
    ///
    /// `SM_ERR_INVALID_INPUT` for a rail past 1 or an invalid generator; those of the
    /// station solve and the evaluations.
    #[allow(clippy::too_many_arguments)]
    pub fn contact_order_at(
        &mut self,
        k: &Kernel,
        faces: [&Surface; 2],
        center: &BSplineCurve,
        s: f64,
        seed: [f64; 4],
        rail_points: [Point3; 2],
        rail: usize,
    ) -> Result<usize> {
        if rail > 1 || !Self::is_valid(self.order, self.thumbweight) {
            return Err(SolidError::invalid_input(OP, "ContactOrderAt"));
        }
        let n = self.order;
        let m = center.evaluate_point(s)?;
        let [rail_a, rail_b] = rail_points;
        self.resolve_thumbweight(m, rail_a, rail_b)?;
        let mut r = [0.0_f64; 2];
        let mut side = [0.0_f64; 2];
        let rows = r
            .iter_mut()
            .zip(side.iter_mut())
            .zip(faces)
            .zip(rail_points)
            .zip(seed.chunks_exact(2));
        for ((((rf, sf), face), rho), uv) in rows {
            let (Some(&u), Some(&v)) = (uv.first(), uv.get(1)) else {
                return Err(SolidError::sm_err(OP));
            };
            let nrm = unit_normal(k, face, Point2::new(u, v))?;
            let off = m - rho;
            *rf = off.length();
            *sf = if off.dot(nrm) >= 0.0 { 1.0 } else { -1.0 };
        }
        let tr = Trunc::new(0, n + 1)?;
        let at = gn_solve_station(
            k,
            faces,
            center,
            s,
            seed,
            r,
            side,
            tr,
            1.0e-9 * smax(1.0, m.length()),
        )?;
        let (i, o) = (rail, 1 - rail);
        let mut t_o = Vec3::new(0.0, 0.0, 0.0);
        for kk in 0..=n {
            t_o += foot(&at, o, 0, kk) * if kk % 2 == 0 { 1.0 } else { -1.0 };
        }
        let p = surface_partials(
            k,
            if i == 0 { faces[0] } else { faces[1] },
            Point2::new(param(&at, 2 * i, 0, 0), param(&at, 2 * i + 1, 0, 0)),
            1,
        )?;
        let ni = vec3(p.at(1, 0)).cross(vec3(p.at(0, 1))).unitize()?;
        let scale = (-1.0 / (1.0 - self.thumbweight)).powf((n + 1) as f64);
        let coef = ((t_o - foot(&at, i, 0, 0)) - foot(&at, i, 0, n + 1) * scale).dot(ni);
        let size = foot(&at, 0, 0, 0).distance(foot(&at, 1, 0, 0));
        Ok(if coef.abs() > 1.0e-9 * smax(1.0, size) {
            n + 1
        } else {
            n + 2
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::extent2d::Extent2d;
    use crate::math::Vec2;
    use crate::surface::plane::Plane;

    fn plane(x_axis: Vec3, y_axis: Vec3) -> Surface {
        Surface::from(Plane::new(
            Vec3::ZERO,
            x_axis,
            y_axis,
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(-10.0, -10.0, 10.0, 10.0),
        ))
    }

    /// `EqualAreaThumbweight` for orders 2 to 4 over wedges from 0.3 to 3 radians: the
    /// values fork d01d7b0 gives (built locally, `-ffp-contract=off`), bit for bit; the
    /// wedges too shut for any thumbweight are refused, as there. At a square corner the
    /// section takes off what the circle does, `(1 - pi/4) r^2` (closed form).
    #[test]
    fn equal_area_thumbweights_are_the_fork_s() {
        let fork: [(usize, f64, Option<f64>); 18] = [
            (2, 0.3, None),
            (2, 1.0, None),
            (2, core::f64::consts::FRAC_PI_2, Some(-0.52521240458756147)),
            (2, 2.0, Some(-0.29072725674815358)),
            (2, 2.5, Some(-0.15058804304328927)),
            (2, 3.0, Some(-0.0971637269709128)),
            (3, 0.3, None),
            (3, 1.0, None),
            (3, core::f64::consts::FRAC_PI_2, Some(-0.58094928730219753)),
            (3, 2.0, Some(-0.33852472453244753)),
            (3, 2.5, Some(-0.19361792885821855)),
            (3, 3.0, Some(-0.13837023281917249)),
            (4, 0.3, None),
            (4, 1.0, None),
            (4, core::f64::consts::FRAC_PI_2, Some(-0.62397886337619379)),
            (4, 2.0, Some(-0.37522338561877744)),
            (4, 2.5, Some(-0.22652960634299901)),
            (4, 3.0, Some(-0.16983693311599979)),
        ];
        for (n, alpha, want) in fork {
            let got = GnSectionFsg::equal_area_thumbweight(n, alpha);
            match want {
                Some(w) => assert_eq!(got.unwrap().to_bits(), w.to_bits(), "n {n} alpha {alpha}"),
                None => assert!(got.is_err(), "n {n} alpha {alpha}"),
            }
        }
        assert!(GnSectionFsg::equal_area_thumbweight(1, 1.0).is_err());
        assert!(GnSectionFsg::equal_area_thumbweight(2, 0.0).is_err());
        assert!(GnSectionFsg::equal_area_thumbweight(2, PI).is_err());

        // a square corner: half the integral is the area over r^2, 1 - pi/4
        for n in 2..=GN_MAX_ORDER {
            let w = GnSectionFsg::equal_area_thumbweight(n, core::f64::consts::FRAC_PI_2).unwrap();
            let area = 0.5 * gn_section_area_integral(n, w);
            assert!((area - (1.0 - PI / 4.0)).abs() < 1e-12, "n {n}: {area}");
        }

        // resolving: the wedge from a ball's centre and contacts, kept once resolved; a
        // thumbweight given is kept
        let mut g = GnSectionFsg::new(2, GN_DEFAULT_THUMBWEIGHT);
        let c = Point3::new(3.0, 0.0, 3.0);
        g.resolve_thumbweight(c, Point3::new(3.0, 0.0, 0.0), Point3::new(0.0, 0.0, 3.0))
            .unwrap();
        assert_eq!(
            g.thumbweight.to_bits(),
            (-0.52521240458756147_f64).to_bits()
        );
        g.resolve_thumbweight(c, Point3::new(3.0, 0.0, 0.0), Point3::new(1.0, 0.0, 1.0))
            .unwrap();
        assert_eq!(
            g.thumbweight.to_bits(),
            (-0.52521240458756147_f64).to_bits()
        );
        let mut g = GnSectionFsg::new(2, 0.3);
        g.resolve_thumbweight(c, c, c).unwrap();
        assert_eq!(g.thumbweight, 0.3);
        let mut g = GnSectionFsg::new(2, GN_THUMBWEIGHT_EQUAL_AREA);
        assert!(g
            .resolve_thumbweight(c, c, Point3::new(0.0, 0.0, 3.0))
            .is_err());
    }

    #[test]
    fn valid_orders_and_thumbweights() {
        assert!(GnSectionFsg::is_valid(2, 0.0));
        assert!(GnSectionFsg::is_valid(4, 0.999));
        assert!(!GnSectionFsg::is_valid(1, 0.0));
        assert!(!GnSectionFsg::is_valid(5, 0.0));
        assert!(!GnSectionFsg::is_valid(2, 1.0));
        // the fork's 6148773 (= 1947a45): (-1, 1) and the equal area default
        assert!(GnSectionFsg::is_valid(2, -0.1));
        assert!(GnSectionFsg::is_valid(2, -0.999));
        assert!(GnSectionFsg::is_valid(2, GN_THUMBWEIGHT_EQUAL_AREA));
        assert!(!GnSectionFsg::is_valid(2, -1.0));
        assert!(!GnSectionFsg::is_valid(2, -1.5));
        assert!(!GnSectionFsg::is_valid(2, f64::NAN));
        let g = GnSectionFsg::new(3, 0.25);
        assert_eq!(g.certified_bound, -1.0);
        assert_eq!(g.certified_spans, 0);
    }

    /// Two planes: the feet are linear in tau, so `c_i,n+1 = 0` and `T_o(0)` is the edge's
    /// point, on face i: the `t^(n+1)` coefficient vanishes and the contact is of order
    /// n + 2 on both faces (the fork's `ContactOrderAt` note), at a square corner and at
    /// 60 degrees, for every order and a thumbweight.
    #[test]
    fn two_planes_meet_the_section_at_order_n_plus_2() {
        let k = Kernel::default();
        let r = 1.5;
        let (c, s60) = (0.5_f64, 3.0_f64.sqrt() / 2.0);
        // face 1: z = 0 (normal +z); face 2: the plane through the y axis at angle a to it
        for (dir2, center) in [
            (Vec3::Z, Vec3::new(r, 0.0, r)),
            // the second face along (c, 0, s60): the ball's centre on the bisector at
            // distance r from both
            (
                Vec3::new(c, 0.0, s60),
                Vec3::new(r / (30.0_f64.to_radians()).tan(), 0.0, r),
            ),
        ] {
            let f1 = plane(Vec3::X, Vec3::Y);
            let f2 = plane(Vec3::Y, dir2);
            let leader =
                BSplineCurve::create_line_segment(3, center, center + Vec3::new(0.0, 4.0, 0.0))
                    .unwrap();
            let s = 0.5;
            let m = center + Vec3::new(0.0, 2.0, 0.0);
            let rho1 = Vec3::new(m.x, m.y, 0.0);
            // the foot on face 2 along its direction, at the centre's projection
            let t2 = m.x * dir2.x + m.z * dir2.z;
            let rho2 = Vec3::new(t2 * dir2.x, m.y, t2 * dir2.z);
            assert!(((m - rho1).length() - r).abs() < 1e-12);
            assert!(((m - rho2).length() - r).abs() < 1e-12);
            let seed = [rho1.x, rho1.y, m.y, t2];
            for n in 2..=GN_MAX_ORDER {
                for w in [0.0, 0.3] {
                    let mut g = GnSectionFsg::new(n, w);
                    for rail in 0..2 {
                        let order = g
                            .contact_order_at(&k, [&f1, &f2], &leader, s, seed, [rho1, rho2], rail)
                            .unwrap();
                        assert_eq!(order, n + 2, "n {n} w {w} rail {rail}");
                    }
                }
            }
        }
    }
}
