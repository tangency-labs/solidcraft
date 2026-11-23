//! Hypoid gears: an offset spiral pinion driving a conical ring gear, as in a car's final drive.
//!
//! # Method
//!
//! **Pitch cones** ([`design`]) are worked out from first principles, in the terms of ISO 23509
//! and Gleason's hypoid calculation (Litvin & Fuentes, *Gear Geometry and Applied Theory*, ch. 21;
//! Stadtfeld, *Handbook of Bevel and Hypoid Gears*):
//!
//! - The ring gear's axis is z; the pinion's axis is square to the common perpendicular (y) at
//!   the shaft angle Σ, `offset` (E) away. The ring gear's outer pitch diameter and face width
//!   give its mean pitch radius r_m2 = (d_e2 − b2·sin δ2) / 2.
//! - The two pitch cones touch at the mean point P with a common tangent plane: P is on the
//!   gear's mean pitch circle, the gear cone's normal there is the pinion cone's (reversed), and
//!   that normal lies in the pinion's axial plane through P. That fixes the pinion axis' height
//!   (the crossing point) for a given angular position of P.
//! - The two cone generatrices through P make the offset angle ε′ in the pitch plane, so the
//!   tooth line through P has spiral angle β1 against the pinion's generatrix and β2 = β1 − ε′
//!   against the gear's: the pinion's spiral angle is the larger one.
//! - The normal pitches agree at P: r_m1·cos β1 / z1 = r_m2·cos β2 / z2. Solved for P's angular
//!   position this makes the pinion larger than a bevel pinion of the same ratio by
//!   cos β2 / cos β1 (its "enlargement"), and makes the two pitch surfaces slide on each other
//!   along the tooth line only.
//! - One more condition is needed to settle the cones. ISO 23509 Method 1 iterates the wheel
//!   pitch angle until the limit radius of lengthwise curvature ρ_lim equals the tooth line's
//!   curvature (the face-milling cutter's radius). The tooth line here is a logarithmic spiral
//!   (constant spiral angle), whose curvature radius in the developed pitch cone is
//!   R_m2 / sin β2, so δ2 is solved for ρ_lim = R_m2 / sin β2, starting from the bevel value
//!   tan δ2 = u·sin Σ / (1 + u·cos Σ) (it lands within a degree or two of it; a real face-milled
//!   set with a small cutter has a flatter wheel cone, e.g. 61° instead of 72° at 15:45).
//!   Given δ2 and r_m2, the rest reproduces a published ISO 23509 Method 1 set (MITcalc's
//!   hypoid example) to 1e-4° (test `matches_a_published_iso_23509_set`).
//! - Depths are Gleason's: mean working depth 2·m_mn, gear addendum c·h with
//!   c = 0.21 + 0.29 / u², the pinion's the rest (long addendum pinion), clearance from the basic
//!   rack. The limit pressure angle (ISO 23509) is taken off one flank's pressure angle and added
//!   to the other's, so the two sides of the pinion's teeth work alike.
//!
//! **The ring gear** is the simple member, as a Formate (non-generated) gear is: straight-sided
//! teeth in the normal section at the mean point, every other slice of a tooth being that one
//! scaled about the gear's pitch apex and turned along a logarithmic spiral (constant spiral
//! angle β2; a face-milled gear's circular arc is not followed). Root and face cones share the
//! pitch apex (standard depth taper). The teeth start and end on the front (toe) and back (heel)
//! cones, square to the pitch cone at the inner and outer cone distances, at every depth.
//!
//! **The pinion** is generated from the ring gear, as a real hypoid pinion is cut to run with its
//! gear: it is what is left of its blank after the ring gear (its teeth carried on past both
//! ends of its face, its tips and roots raised by the clearance) has turned through it at the
//! ratio z1 : z2. Each pinion section is found point by point: on a circle about the pinion's
//! axis, the part no ring gear tooth ever reaches is the pinion's tooth. The pinion's flanks are
//! therefore exactly conjugate to the gear's, its root fillets are the paths of the gear's tip
//! corners and its tip is the gear's root cone seen from the pinion, a clearance away.
//!
//! Both members are lofts through planar sections square to their own axes (19 or more for the
//! ring gear; 9 exact pinion sections, lofted through as many between as the kernel needs: with
//! the kernel's cubic loft through them the flanks stay within a few microns of the true ones at
//! module 5; the tests check the pair as the lofts describe it).
//!
//! **Toe and heel.** A plane square to the axis can't end a tooth: on the ring gear's flat cone
//! (70–80° at 90° shafts) it meets the tips and the roots tens of millimetres apart, and a ring
//! gear ended on such planes kept long thin blades of root inside its toe. Each member's loft
//! therefore runs on past both ends with whole teeth in every section, and is intersected with
//! its blank, a solid of revolution bounded by the front and back cones (`Step::Revolve`,
//! `Op::Intersect`). Each member has its own face width: the ring gear's is `face_width`
//! (ISO 23509's b2, which sizes the set); the pinion's covers the ring gear's face with a tenth
//! of b2 more at each end, or is `face_width2`, moved along its cone by `face_offset2`.

use std::f64::consts::{PI, TAU};

use crate::axes::{add, cross, dot, len, mul, num, sub};
use crate::geom::{Outline, Region, Seg, V2, wrap_period};
use crate::plan::{Body, Frame, Op, Plan, PreviewGear, Spec, Step, Unit};

type V3 = [f64; 3];

fn unit(a: V3) -> V3 {
    let l = len(a);
    if l > 1e-300 { mul(a, 1.0 / l) } else { [0.0, 0.0, 1.0] }
}

/// A hypoid set's pitch cones, spiral angles and proportions, in a frame with the ring gear's
/// axis along z through the origin and its mean pitch circle on z = 0 (left-hand pinion; a
/// right-hand set is this one mirrored in y).
#[derive(Clone, Debug)]
pub struct Design {
    pub z1: f64,
    pub z2: f64,
    /// Shaft angle Σ and offset E.
    pub sigma: f64,
    pub offset: f64,
    /// Pitch cone angles δ1, δ2.
    pub delta1: f64,
    pub delta2: f64,
    /// Mean spiral angles (magnitudes) β1 > β2 and the offset angle in the pitch plane ε′.
    pub beta1: f64,
    pub beta2: f64,
    pub eps_pitch: f64,
    /// Where the mean point lies round each axis: the angle from the plane through the axis
    /// parallel to the other axis to the axial plane through P (gear, pinion).
    pub eps_gear: f64,
    pub eta_pinion: f64,
    /// Mean pitch radii, mean cone distances, the gear's outer cone distance and pitch radius,
    /// its face width.
    pub rm1: f64,
    pub rm2: f64,
    pub cone_m1: f64,
    pub cone_m2: f64,
    pub cone_e2: f64,
    pub re2: f64,
    pub b2: f64,
    /// Mean normal module, mean addenda and dedenda, clearance.
    pub mmn: f64,
    pub ha1: f64,
    pub hf1: f64,
    pub ha2: f64,
    pub hf2: f64,
    pub clearance: f64,
    /// The limit pressure angle (ISO 23509), signed.
    pub alpha_lim: f64,
    /// Which side of the gear's centre the pinion's axis is (y = −side·E).
    pub side: f64,
    /// The crossing point's height on the gear axis.
    pub h: f64,
    /// The mean point, the gear cone's outward normal there, the pinion's axis direction (towards
    /// its apex), the crossing point on the pinion's axis, the two pitch apexes and the tooth
    /// line's direction at P (towards the apexes).
    pub p: V3,
    pub n2: V3,
    pub a1: V3,
    pub o1: V3,
    pub apex1: V3,
    pub apex2: V3,
    pub tooth: V3,
    pub warnings: Vec<String>,
}

/// One trial position of the mean point.
#[derive(Clone, Copy, Debug)]
struct Trial {
    phi: f64,
    h: f64,
    p: V3,
    n2: V3,
    g1: V3,
    g2: V3,
    a1: V3,
    o1: V3,
    sd1: f64,
    rm1: f64,
    eps: f64,
    res: f64,
}

struct Setup {
    z1: f64,
    z2: f64,
    sigma: f64,
    rm2: f64,
    d2: f64,
    beta1: f64,
}

impl Setup {
    fn a1(&self) -> V3 {
        [self.sigma.sin(), 0.0, self.sigma.cos()]
    }
    /// The crossing point's height that puts the common normal in the pinion's axial plane.
    fn height(&self, phi: f64, e: f64) -> Option<f64> {
        let (sd, cd) = self.d2.sin_cos();
        let (sp, cp) = phi.sin_cos();
        let (ss, cs) = self.sigma.sin_cos();
        let den = ss * cd * sp;
        if den.abs() < 1e-12 {
            return None;
        }
        let h = (cd * cs * e * cp - sd * ss * (self.rm2 * sp + e)) / den;
        h.is_finite().then_some(h)
    }
    /// The pitch cones with the mean point at angle `phi` and the pinion's axis through
    /// (0, −e, h): `e` is signed.
    fn trial(&self, phi: f64, h: f64, e: f64) -> Option<Trial> {
        let (sd, cd) = self.d2.sin_cos();
        let (sp, cp) = phi.sin_cos();
        let p = [self.rm2 * cp, self.rm2 * sp, 0.0];
        let n2 = [cd * cp, cd * sp, sd];
        let g2 = [-sd * cp, -sd * sp, cd];
        let a1 = self.a1();
        let o1 = [0.0, -e, h];
        let sd1 = -dot(n2, a1);
        if !(1e-3..0.999).contains(&sd1) {
            return None;
        }
        let cd1 = (1.0 - sd1 * sd1).sqrt();
        let n1 = mul(n2, -1.0);
        let g1 = mul(sub(a1, mul(n1, sd1)), 1.0 / cd1);
        let rel = sub(p, o1);
        let radial = sub(rel, mul(a1, dot(rel, a1)));
        let rm1 = len(radial);
        // The pinion sits on the gear cone's outer side.
        if rm1 <= 1e-9 || dot(radial, n2) >= 0.0 {
            return None;
        }
        let eps = dot(cross(g2, g1), n2).atan2(dot(g2, g1));
        let res = rm1 * self.beta1.cos() * self.z2 - self.rm2 * (self.beta1 - eps).cos() * self.z1;
        Some(Trial { phi, h, p, n2, g1, g2, a1, o1, sd1, rm1, eps, res })
    }
    fn at(&self, phi: f64, e: f64) -> Option<Trial> {
        self.trial(phi, self.height(phi, e)?, e)
    }
    /// The mean point for offset `e` (≥ 0) with a left-hand pinion: the root nearest the
    /// zero-offset position with ε′ > 0.
    fn solve(&self, e: f64) -> Option<(Trial, f64)> {
        if e < 1e-3 {
            let h = self.rm2 / self.d2.tan();
            return self.trial(PI, h, 0.0).map(|t| (t, 1.0));
        }
        let mut best: Option<(Trial, f64)> = None;
        let scale = self.rm2 * self.z1.max(self.z2);
        for side in [1.0, -1.0] {
            let es = side * e;
            for dir in [1.0, -1.0] {
                // Offsets from φ = π, spaced evenly in their logarithm.
                let n = 1500;
                let mut prev: Option<Trial> = None;
                for i in 0..=n {
                    let d = 10f64.powf(-7.0 + (7.0 + (PI / 2.0 - 1e-3).log10()) * i as f64 / n as f64);
                    let phi = PI + dir * d;
                    let cur = self.at(phi, es);
                    if let (Some(a), Some(b)) = (prev, cur)
                        && a.res.signum() != b.res.signum()
                    {
                        let (mut lo, mut hi) = (a, b);
                        let mut ok = true;
                        for _ in 0..80 {
                            let mid = 0.5 * (lo.phi + hi.phi);
                            match self.at(mid, es) {
                                Some(m) if m.res.signum() == lo.res.signum() => lo = m,
                                Some(m) => hi = m,
                                None => {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                        let r = if lo.res.abs() < hi.res.abs() { lo } else { hi };
                        if ok && r.res.abs() < 1e-7 * scale && r.eps > 0.0 && r.eps < self.beta1 + 1.0 {
                            let better = best.as_ref().is_none_or(|(b, _)| (r.phi - PI).abs() < (b.phi - PI).abs());
                            if better {
                                best = Some((r, side));
                            }
                        }
                    }
                    prev = cur;
                }
            }
        }
        best
    }
}

/// The least mean spiral angle of the ring gear (degrees). The offset takes the offset angle off
/// the pinion's spiral angle to give the ring gear's; below this its teeth barely spiral (or wind
/// the other way, the pinion's hand), the pinion's teeth come to a point and ISO 23509's limit
/// curvature can no longer be met, so a larger offset is refused.
pub const LEAST_RING_SPIRAL: f64 = 10.0;

/// The hypoid set a spec describes. An offset that leaves the ring gear's spiral angle under
/// [`LEAST_RING_SPIRAL`] is refused, naming the largest that works for these sizes.
pub fn design(s: &Spec) -> Result<Design, String> {
    let d = design_iso(s)?;
    let least = LEAST_RING_SPIRAL.to_radians();
    if s.offset < 1e-3 || d.beta2 >= least {
        return Ok(d);
    }
    // The ring gear's spiral angle falls as the offset grows: halve towards the largest offset
    // that keeps it, and whose pinion still has teeth (the offset that is named works).
    let fits = |e: f64| {
        let t = Spec { offset: e, ..s.clone() };
        design_iso(&t).is_ok_and(|d| d.beta2 >= least && work_out_design(&t, d).map_or_else(|e| e.contains("twist too fast to loft"), |_| true))
    };
    let (mut lo, mut hi) = (0.0, s.offset);
    for _ in 0..14 {
        let mid = 0.5 * (lo + hi);
        if fits(mid) { lo = mid } else { hi = mid }
    }
    // (Near the edge the pinion's teeth may give out unevenly: the offset named is one that works.)
    let mut most = (lo * 100.0).floor() / 100.0;
    for _ in 0..30 {
        if most <= 0.0 || fits(most) {
            break;
        }
        most = ((most - 0.01 * s.offset.max(1.0)) * 100.0).floor() / 100.0;
    }
    Err(if most > 0.0 {
        format!(
            "offset too large: use at most {} mm with these sizes ({} mm asked): the ring gear's spiral angle would be {}°, under the {}° it needs (a larger pinion spiral angle allows more)",
            num(most),
            num(s.offset),
            num(d.beta2.to_degrees()),
            num(LEAST_RING_SPIRAL)
        )
    } else if s.spiral_angle < LEAST_RING_SPIRAL + 1.0 {
        format!(
            "a pinion spiral angle of {}° leaves no room for an offset: any offset takes the ring gear's spiral angle under {}°; raise the spiral angle, or use offset 0 (a spiral bevel pair)",
            num(s.spiral_angle),
            num(LEAST_RING_SPIRAL)
        )
    } else {
        format!(
            "offset too large: no offset works with these sizes ({} mm asked; the ring gear's spiral angle would be {}°, under the {}° it needs): use offset 0 (a spiral bevel pair) or a larger pinion spiral angle",
            num(s.offset),
            num(d.beta2.to_degrees()),
            num(LEAST_RING_SPIRAL)
        )
    })
}

/// The pitch cones work out and leave the ring gear's spiral angle at
/// [`LEAST_RING_SPIRAL`] or more: [`design`] takes the spec (quick to ask: no search for the
/// largest offset).
pub(crate) fn ring_spirals(s: &Spec) -> bool {
    design_iso(s).is_ok_and(|d| s.offset < 1e-3 || d.beta2 >= LEAST_RING_SPIRAL.to_radians())
}

/// [`design`] without the ring gear spiral angle's limit.
fn design_iso(s: &Spec) -> Result<Design, String> {
    let first = design_with(s, None)?;
    if s.offset < 1e-3 {
        return Ok(first);
    }
    // ISO 23509 Method 1 settles the wheel pitch angle by making the limit radius of curvature
    // equal the tooth line's own lengthwise curvature (the cutter radius, for a face-milled
    // gear). Here the tooth line is a logarithmic spiral, whose radius of curvature in the
    // developed pitch cone is R_m2 / sin β2.
    let de2 = s.module * f64::from(s.teeth2);
    let miss = |d2: f64| -> Option<(f64, Design)> {
        let b2 = s.face_width.min(0.45 * de2 / 2.0 / d2.sin());
        let d = design_with(s, Some((d2, (de2 - b2 * d2.sin()) / 2.0))).ok()?;
        let r = rho_lim(&d);
        let own = d.cone_m2 / d.beta2.sin().abs().max(1e-6);
        (r.is_finite() && r > 0.0).then_some((r - own, d))
    };
    // From the bevel estimate a degree at a time towards the crossing (the limit radius grows
    // with the wheel pitch angle, the spiral's own falls), then halving.
    let Some((f0, _)) = miss(first.delta2) else { return Ok(first) };
    let dir = if f0 > 0.0 { -1.0 } else { 1.0 };
    let step = 1f64.to_radians();
    let (mut lo, mut f_lo) = (first.delta2, f0);
    let mut hi = f64::NAN;
    for k in 1..=30 {
        let x = first.delta2 + dir * step * f64::from(k);
        if !(10.5f64.to_radians()..84.5f64.to_radians()).contains(&x) {
            break;
        }
        let Some((f, _)) = miss(x) else { break };
        if f.signum() != f0.signum() {
            hi = x;
            break;
        }
        (lo, f_lo) = (x, f);
    }
    if hi.is_nan() {
        return Ok(first);
    }
    for _ in 0..26 {
        let mid = 0.5 * (lo + hi);
        match miss(mid) {
            Some((f, _)) if f.signum() == f_lo.signum() => (lo, f_lo) = (mid, f),
            Some(_) => hi = mid,
            None => return Ok(first),
        }
    }
    Ok(miss(0.5 * (lo + hi)).map_or(first, |(_, d)| d))
}

/// ISO 23509's limit radius of lengthwise curvature at the mean point: the tooth line curvature
/// at which the limit pressure angle makes both flanks' contact alike.
pub(crate) fn rho_lim(d: &Design) -> f64 {
    let (r1, r2) = (d.cone_m1, d.cone_m2);
    let a = d.alpha_lim;
    (d.beta1.tan() - d.beta2.tan())
        / a.cos()
        / (-a.tan() * (1.0 / (r1 * d.delta1.tan()) + 1.0 / (r2 * d.delta2.tan())) + 1.0 / (r1 * d.beta1.cos()) - 1.0 / (r2 * d.beta2.cos()))
}

/// [`design`], with the ring gear's pitch cone angle and mean pitch radius given instead of
/// worked out (`fix`: δ2 in radians, r_m2 in mm), as when checking against a published set.
fn design_with(s: &Spec, fix: Option<(f64, f64)>) -> Result<Design, String> {
    let (z1, z2) = (f64::from(s.teeth), f64::from(s.teeth2));
    if s.teeth > s.teeth2 {
        return Err("the pinion (teeth) can't have more teeth than the ring gear (mating teeth)".into());
    }
    if !(0.0..=60.0).contains(&s.spiral_angle) {
        return Err("the pinion's spiral angle must be between 0° and 60°".into());
    }
    if !(s.offset.is_finite() && s.offset >= 0.0) {
        return Err("the offset must not be negative (the hand picks the side)".into());
    }
    let mut warnings = Vec::new();
    let sigma = s.shaft_angle.to_radians();
    let u = z2 / z1;
    let d2 = fix.map_or_else(|| (u * sigma.sin()).atan2(1.0 + u * sigma.cos()), |f| f.0);
    if !(10f64.to_radians()..=85f64.to_radians()).contains(&d2) {
        // The shaft angles that keep the ring gear's pitch cone between 10° and 85°.
        let cone = |sg: f64| (u * sg.sin()).atan2(1.0 + u * sg.cos());
        let edge = |want: f64| {
            let (mut lo, mut hi) = (1e-3, PI - 1e-3);
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if cone(mid) < want { lo = mid } else { hi = mid }
            }
            lo.to_degrees()
        };
        let (least, most) = (edge(10f64.to_radians()).ceil(), edge(85f64.to_radians()).floor());
        return Err(if d2 > 85f64.to_radians() {
            format!(
                "shaft angle too large: use at most {most}° with these teeth (the ring gear would be a crown gear, pitch cone {}°)",
                num(d2.to_degrees())
            )
        } else {
            format!("shaft angle too small: use at least {least}° with these teeth (ring gear pitch cone {}°)", num(d2.to_degrees()))
        });
    }
    let de2 = s.module * z2;
    let re2 = de2 / 2.0;
    let cone_e2 = re2 / d2.sin();
    let b2 = s.face_width.min(0.45 * cone_e2);
    if s.face_width > cone_e2 * 0.34 {
        warnings.push(format!("the face width is over a third of the ring gear's cone distance ({} mm), which makes weak toes", num(cone_e2 / 3.0)));
    }
    let rm2 = fix.map_or((de2 - b2 * d2.sin()) / 2.0, |f| f.1);
    let setup = Setup { z1, z2, sigma, rm2, d2, beta1: s.spiral_angle.to_radians() };
    let e = s.offset;
    let Some((t, side)) = setup.solve(e) else {
        // The largest offset that still works.
        let (mut lo, mut hi) = (0.0, e);
        if setup.solve(0.0).is_none() {
            return Err("no pitch cones fit this shaft angle and these tooth counts".into());
        }
        for _ in 0..14 {
            let mid = 0.5 * (lo + hi);
            if setup.solve(mid).is_some() { lo = mid } else { hi = mid }
        }
        return Err(format!(
            "offset too large: use at most {} mm here ({} mm asked; a larger ring gear or spiral angle allows more)",
            num((lo * 100.0).floor() / 100.0),
            num(e)
        ));
    };
    let beta1 = setup.beta1;
    let beta2 = beta1 - t.eps;
    let delta1 = t.sd1.asin();
    let cone_m1 = t.rm1 / t.sd1;
    let cone_m2 = rm2 / d2.sin();
    let mmn = 2.0 * rm2 * beta2.cos() / z2;
    let hmw = 2.0 * s.rack.addendum * mmn;
    let c_ham = 0.21 + 0.29 / (u * u);
    let clearance = (s.rack.dedendum - s.rack.addendum).max(0.05) * mmn;
    let ha2 = c_ham * hmw;
    let ha1 = hmw - ha2;
    let (hf1, hf2) = (ha2 + clearance, ha1 + clearance);
    // The tooth line through P: β2 from the gear's generatrix, β1 from the pinion's.
    let tooth = add(mul(t.g2, beta2.cos()), mul(cross(t.n2, t.g2), -beta2.sin()));
    let apex1 = add(t.p, mul(t.g1, cone_m1));
    let apex2 = add(t.p, mul(t.g2, cone_m2));
    // Offset angles in each member's plane of rotation: from the plane through its axis parallel
    // to the other's axis, to its axial plane through P.
    let eps_gear = (t.phi - PI).abs().min(PI);
    let foot = add(t.o1, mul(t.a1, dot(sub(t.p, t.o1), t.a1)));
    let radial = unit(sub(t.p, foot));
    let down = unit(sub([0.0, 0.0, -1.0], mul(t.a1, -t.a1[2])));
    let eta_pinion = dot(radial, down).clamp(-1.0, 1.0).acos();
    // ISO 23509's limit pressure angle.
    let (td1, td2) = (delta1.tan(), d2.tan());
    let alpha_lim = (-(td1 * td2 / t.eps.cos()) * (cone_m1 * beta1.sin() - cone_m2 * beta2.sin()) / (cone_m1 * td1 + cone_m2 * td2)).atan();
    // (A ring gear spiral angle under `LEAST_RING_SPIRAL` is refused by `design`.)
    if e > 0.25 * de2 {
        warnings.push("an offset over a quarter of the ring gear's diameter is more than hypoid sets usually use".into());
    }
    if beta1.to_degrees() > 55.0 {
        warnings.push("pinion spiral angles over 55° make high axial thrust".into());
    }
    Ok(Design {
        z1,
        z2,
        sigma,
        offset: e,
        delta1,
        delta2: d2,
        beta1,
        beta2,
        eps_pitch: t.eps,
        eps_gear,
        eta_pinion,
        rm1: t.rm1,
        rm2,
        cone_m1,
        cone_m2,
        cone_e2: cone_m2 + b2 / 2.0,
        re2,
        b2,
        mmn,
        ha1,
        hf1,
        ha2,
        hf2,
        clearance,
        alpha_lim,
        side,
        h: t.h,
        p: t.p,
        n2: t.n2,
        a1: t.a1,
        o1: t.o1,
        apex1,
        apex2,
        tooth,
        warnings,
    })
}

impl Design {
    /// The pinion's cone generatrix at P, towards its apex.
    pub fn g1(&self) -> V3 {
        unit(sub(self.apex1, self.p))
    }
    pub fn g2(&self) -> V3 {
        unit(sub(self.apex2, self.p))
    }
}

fn wanted(s: &Spec, member: u8) -> bool {
    s.only == 0 || s.only == member
}

fn name(s: &Spec, base: &str) -> String {
    if s.name.trim().is_empty() { base.to_string() } else { format!("{} {base}", s.name.trim()) }
}

/// Where fit points go along a tooth's side (0 to 1): closer together towards both ends, where a
/// fit point spline (which ends without curvature) strays most from the curve it is given.
fn spread(x: f64) -> f64 {
    0.2 * x + 0.8 * 0.5 * (1.0 - (PI * x).cos())
}

/// The ring gear's teeth, in the design frame (axis z, mean pitch circle on z = 0).
///
/// A point of the gear is named by the cone through the pitch apex it lies on and how far down
/// that cone it is: `t`, the cone's height above the pitch cone where it crosses the mean back
/// cone (−dedendum at the root cone, +addendum at the face cone), and `mu`, its distance from the
/// apex against the mean back cone's (1 at the mean point, larger towards the heel). The slice
/// of a tooth at `mu` is the mean slice scaled by `mu` and turned by `-k·ln mu`.
#[derive(Clone, Debug)]
pub(crate) struct Form {
    teeth: u32,
    pitch: f64,
    sd: f64,
    cd: f64,
    /// The pitch apex's height and the mean pitch radius.
    hap: f64,
    rm: f64,
    ha: f64,
    hf: f64,
    /// tan β2 / sin δ2: a tooth's turn per unit of ln(cone distance).
    k: f64,
    /// A tooth centre's angle at the mean point.
    psi: f64,
    /// Half the normal tooth thickness at the pitch cone, and the tangents of the pressure angles
    /// of the flank at the lower angle and of the one at the higher angle.
    half: f64,
    tan_lo: f64,
    tan_hi: f64,
    /// cos β2 · r_m2: normal lengths at the mean point per radian round the axis.
    lat: f64,
    /// The toe and the heel: the front and back cones, square to the pitch cone, at these
    /// `mu` (the teeth run from one to the other at every depth).
    mu_i: f64,
    mu_e: f64,
    /// The outside diameter's radius (the tips at the heel), and the planes the toothed loft
    /// runs between: a little past the root at the heel and the tips at the toe, so every
    /// section is whole teeth and the cones trim them ([`Form::blank`]).
    r_out: f64,
    z_lo: f64,
    z_hi: f64,
    /// Fit points up each side of a tooth section.
    side: usize,
}

/// Fit points up each side of a ring gear tooth section, and of a pinion tooth section.
const GEAR_SIDE: usize = 7;
const PINION_SIDE: usize = 8;
/// Sections along each member's axis: the ring gear's; the pinion's worked out exactly (its
/// lofted sections are more, see [`LOFT_COUNTS`]).
const GEAR_SECTIONS: usize = 19;
const PINION_SECTIONS: usize = 9;
/// The most curves the kernel's loft takes, all its sections together (SMLib's `MAX_CURVES`).
const KERNEL_CURVES: usize = 16_384;
/// How many sections the pinion may be lofted through (the kernel takes at most 50): the fewest
/// the kernel lines up tooth to tooth ([`slips`]), each between the exact ones interpolated.
const LOFT_COUNTS: [usize; 7] = [9, 13, 17, 25, 33, 41, 49];

/// The start of each segment of an outline as the kernel's loft splits it (a fit point spline:
/// one cubic per span).
pub(crate) fn corners(o: &Outline) -> Vec<V2> {
    let mut out = Vec::new();
    for s in &o.segs {
        match s {
            Seg::Line(a, _) => out.push(*a),
            Seg::Arc { a, b, ccw, .. } => out.push(if *ccw { *a } else { *b }),
            Seg::Spline(p) => out.extend_from_slice(&p[..p.len() - 1]),
            Seg::Circle(c, r) => out.push(*c + V2::new(*r, 0.0)),
        }
    }
    out
}

/// How the kernel's loft lines its sections up (`loft_numbered_loops`): each section is
/// restarted at the segment whose corners best match the section before (the largest sum of dot
/// products of their directions from the centroid, here the axis), the first matched with the
/// last. Returns each section's restart, in the order given. Sections of `teeth`-fold symmetric
/// outlines are only tried within one tooth (a whole tooth further fits the same).
pub(crate) fn restarts(sections: &[Outline], teeth: usize) -> Vec<usize> {
    let rings: Vec<Vec<V2>> = sections.iter().map(corners).collect();
    let m = rings[0].len();
    let per = if teeth > 0 && m.is_multiple_of(teeth) { m / teeth } else { m };
    let mut dirs: Vec<V2> = rings[rings.len() - 1].clone();
    let mut out = Vec::with_capacity(rings.len());
    for r in &rings {
        if r.len() != m {
            out.push(usize::MAX);
            continue;
        }
        let fit = |k: usize| -> f64 { dirs.iter().enumerate().map(|(i, d)| r[(i + k) % m].dot(*d)).sum() };
        let base = out.last().copied().filter(|k| *k != usize::MAX).unwrap_or(0);
        let mut best = (f64::NEG_INFINITY, base);
        for j in 0..per {
            // Round the previous restart, so a tie keeps it.
            let k = (base + j + m - per / 2) % m;
            let v = fit(k);
            if v > best.0 + 1e-9 * v.abs() {
                best = (v, k);
            }
        }
        out.push(best.1);
        dirs = (0..m).map(|i| r[(i + best.1) % m]).collect();
    }
    out
}

/// Whether the kernel would pair a tooth's segments in one section with others than the same
/// tooth's same segments in the next (in either order of the sections): the loft between them
/// would then cut across the flanks.
pub(crate) fn slips(sections: &[Outline], teeth: usize) -> bool {
    let m = corners(&sections[0]).len();
    let per = if teeth > 0 && m.is_multiple_of(teeth) { m / teeth } else { m };
    let bad = |k: &[usize]| k.contains(&usize::MAX) || k.windows(2).any(|w| !(w[1] + m - w[0]).is_multiple_of(per));
    let rev: Vec<Outline> = sections.iter().rev().cloned().collect();
    bad(&restarts(sections, teeth)) || bad(&restarts(&rev, teeth))
}

/// For `n` ≥ 4 evenly spaced stations, the matrix taking values to a cubic spline's second
/// derivatives (per unit spacing) with not-a-knot ends: rows M[0] − 2 M[1] + M[2] = 0 (and the
/// same at the far end) and M[i−1] + 4 M[i] + M[i+1] = 6 (y[i−1] − 2 y[i] + y[i+1]).
// The elimination indexes rows and columns of one matrix at once, which reads plainest as indices.
#[allow(clippy::needless_range_loop)]
pub(crate) fn spline_matrix(n: usize) -> Vec<Vec<f64>> {
    let mut out = vec![vec![0.0f64; n]; n];
    for j in 0..n {
        let mut a = vec![vec![0.0f64; n + 1]; n];
        a[0][0] = 1.0;
        a[0][1] = -2.0;
        a[0][2] = 1.0;
        a[n - 1][n - 3] = 1.0;
        a[n - 1][n - 2] = -2.0;
        a[n - 1][n - 1] = 1.0;
        let y = |i: usize| if i == j { 1.0 } else { 0.0 };
        for i in 1..n - 1 {
            a[i][i - 1] = 1.0;
            a[i][i] = 4.0;
            a[i][i + 1] = 1.0;
            a[i][n] = 6.0 * (y(i - 1) - 2.0 * y(i) + y(i + 1));
        }
        // Gauss-Jordan with partial pivoting.
        for c in 0..n {
            let p = (c..n).max_by(|x, y| a[*x][c].abs().total_cmp(&a[*y][c].abs())).unwrap_or(c);
            a.swap(c, p);
            for r in 0..n {
                if r != c && a[r][c] != 0.0 {
                    let f = a[r][c] / a[c][c];
                    for k in c..=n {
                        a[r][k] -= f * a[c][k];
                    }
                }
            }
        }
        for i in 0..n {
            out[i][j] = a[i][n] / a[i][i];
        }
    }
    out
}

/// The cubic spline (not-a-knot) through `y` at evenly spaced stations, at `u` (0 to
/// `y.len() − 1`), `m` being [`spline_matrix`] of their number.
fn spline_at(m: &[Vec<f64>], y: &[f64], u: f64) -> f64 {
    let n = y.len();
    let k = (u.floor().max(0.0) as usize).min(n - 2);
    let t = u - k as f64;
    let a = 1.0 - t;
    let dd = |i: usize| -> f64 { m[i].iter().zip(y).map(|(w, v)| w * v).sum() };
    a * y[k] + t * y[k + 1] + ((a * a * a - a) * dd(k) + (t * t * t - t) * dd(k + 1)) / 6.0
}

/// `n` sections through the exact ones (evenly spaced along the axis, the ends kept): each fit
/// point's radius and side angles on a cubic spline through the exact sections'.
fn dense(zeta: &[f64], teeth: &[Teeth], n: usize) -> (Vec<f64>, Vec<Teeth>) {
    let e = zeta.len();
    let length = zeta[e - 1];
    let m = spline_matrix(e.max(4));
    let mut zs = Vec::with_capacity(n);
    let mut out = Vec::with_capacity(n);
    for j in 0..n {
        let u = (e - 1) as f64 * j as f64 / (n - 1) as f64;
        let k = u.round() as usize;
        zs.push(length * j as f64 / (n - 1) as f64);
        if (u - k as f64).abs() < 1e-9 {
            let t = &teeth[k];
            out.push(Teeth { r: t.r.clone(), lo: t.lo.clone(), hi: t.hi.clone(), pointed: t.pointed, undercut: t.undercut });
            continue;
        }
        let at = |f: &dyn Fn(&Teeth) -> &Vec<f64>, i: usize| spline_at(&m, &teeth.iter().map(|t| f(t)[i]).collect::<Vec<_>>(), u);
        let side = teeth[0].r.len();
        out.push(Teeth {
            r: (0..side).map(|i| at(&|t| &t.r, i)).collect(),
            lo: (0..side).map(|i| at(&|t| &t.lo, i)).collect(),
            hi: (0..side).map(|i| at(&|t| &t.hi, i)).collect(),
            pointed: false,
            undercut: false,
        });
    }
    (zs, out)
}

impl Form {
    fn new(s: &Spec, d: &Design) -> Result<Form, String> {
        let (sd, cd) = d.delta2.sin_cos();
        let hap = d.cone_m2 * cd;
        let (ha, hf, c) = (d.ha2, d.hf2, d.clearance);
        if (ha + c) * sd > 0.5 * hap || hf * cd > 0.5 * d.rm2 {
            return Err(format!(
                "teeth too tall: use a smaller module or more teeth (ring gear pitch cone {}°, mean cone distance {} mm)",
                num(d.delta2.to_degrees()),
                num(d.cone_m2)
            ));
        }
        // The limit pressure angle comes off the gear's convex flank (the drive side) and goes on
        // the concave one.
        let alpha = s.pressure_angle.to_radians();
        let lim = d.alpha_lim.clamp(-alpha + 6f64.to_radians(), alpha - 6f64.to_radians());
        let (convex, concave) = ((alpha + lim).tan(), (alpha - lim).tan());
        let (tan_hi, tan_lo) = if d.beta2 >= 0.0 { (convex, concave) } else { (concave, convex) };
        // Thickness: half the pitch, less where the slot would close at the root (the pinion's
        // top land is about the slot's bottom width), never so little that the gear's own top
        // land goes.
        let pn = PI * d.mmn;
        let most = pn - hf * (tan_lo + tan_hi) - 0.4 * d.mmn;
        let least = ha * (tan_lo + tan_hi) + 0.3 * d.mmn;
        if most < least {
            return Err(format!(
                "teeth come to a point: use a smaller pressure angle or a stub tooth form ({}° is too much here)",
                num(s.pressure_angle)
            ));
        }
        let thick = (pn / 2.0).clamp(least, most) - s.backlash.max(0.0);
        if thick < 0.6 * least {
            return Err("the backlash leaves no ring gear tooth: use less".into());
        }
        let mu_e = d.cone_e2 / d.cone_m2;
        let mu_i = ((d.cone_m2 - d.b2 / 2.0) / d.cone_m2).max(0.1);
        let mut f = Form {
            teeth: s.teeth2,
            pitch: TAU / d.z2,
            sd,
            cd,
            hap,
            rm: d.rm2,
            ha,
            hf,
            k: d.beta2.tan() / sd,
            psi: d.p[1].atan2(d.p[0]),
            half: thick / 2.0,
            tan_lo,
            tan_hi,
            lat: d.beta2.cos() * d.rm2,
            mu_i,
            mu_e,
            r_out: mu_e * (d.rm2 + ha * cd),
            z_lo: 0.0,
            z_hi: 0.0,
            side: GEAR_SIDE,
        };
        // The loft runs a little past both cones (half a module along the pitch cone), so the
        // cones cut through whole teeth and never meet the loft's own ends.
        let past = 0.5 * d.mmn / d.cone_m2;
        f.z_hi = f.height(ha, (mu_i - past).max(0.05));
        f.z_lo = f.height(-hf, mu_e + past);
        if !(mu_e > mu_i && f.z_hi - f.z_lo > 1e-6 * d.rm2) {
            return Err("the face width leaves no ring gear tooth".into());
        }
        Ok(f)
    }
    /// (t, mu) of the point at radius `rho` and height `z`; `None` at or above the apex.
    fn coords(&self, rho: f64, z: f64) -> Option<(f64, f64)> {
        let up = self.hap - z;
        if up <= 1e-9 * self.hap {
            return None;
        }
        let t = (self.hap * rho - self.rm * up) / (self.cd * up + self.sd * rho);
        let q = self.hap - t * self.sd;
        (q > 1e-9 * self.hap).then(|| (t, up / q))
    }
    /// The radius of the cone `t` on the plane `z`.
    fn radius(&self, t: f64, z: f64) -> f64 {
        (self.hap - z) * (self.rm + t * self.cd) / (self.hap - t * self.sd)
    }
    fn height(&self, t: f64, mu: f64) -> f64 {
        self.hap - mu * (self.hap - t * self.sd)
    }
    /// A tooth centre's angle at `mu`.
    fn centre(&self, mu: f64) -> f64 {
        self.psi - self.k * mu.ln()
    }
    /// A tooth's reach round the axis below and above its centre, on the cone `t`.
    fn widths(&self, t: f64) -> (f64, f64) {
        ((self.half - t * self.tan_lo) / self.lat, (self.half - t * self.tan_hi) / self.lat)
    }
    /// The teeth's top on the plane `z`: the face cone (the back cone trims them at the heel).
    fn tip(&self, _z: f64) -> f64 {
        self.ha
    }
    /// The point at height `t` over the pitch cone and `mu` along it, as (radius, z).
    fn point(&self, t: f64, mu: f64) -> V2 {
        V2::new(mu * (self.rm + t * self.cd), self.height(t, mu))
    }
    /// The height `t` at which the cone square to the pitch cone at `mu` (a back or front cone)
    /// crosses the plane `z`.
    fn across(&self, mu: f64, z: f64) -> f64 {
        (z - self.hap + mu * self.hap) / (mu * self.sd)
    }
    /// The gear's blank in its body frame (radius, height above the back face at `back`): the
    /// front cone at the toe from above the tips to under the roots, then flat to the axis;
    /// down the axis past the back face; the back cone at the heel up to above the tips; a cone
    /// over the tips between. The toothed loft is intersected with it, so its teeth end on the
    /// toe and heel cones at every depth.
    fn blank(&self, back: f64) -> Vec<V2> {
        let depth = self.ha + self.hf;
        let above = self.ha + 0.5 * depth;
        let under = -self.hf - 0.25 * depth;
        let low = back - 1.0;
        let at = |q: V2| V2::new(q.x, q.y - back);
        // (On a steep cone the front and back cones near the axis fast: they stop a little in
        // from the roots and the blank goes on square to the axis.)
        let t_at = |mu: f64, r: f64| (r / mu - self.rm) / self.cd;
        let toe_least = 0.3 * self.point(-self.hf, self.mu_i).x;
        let f1 = self.point(under.max(t_at(self.mu_i, toe_least)), self.mu_i);
        let heel_least = 0.3 * self.point(-self.hf, self.mu_e).x;
        let t_low = self.across(self.mu_e, low);
        let mut out = vec![at(V2::new(0.0, f1.y)), at(f1), at(self.point(above, self.mu_i)), at(self.point(above, self.mu_e))];
        if self.point(t_low, self.mu_e).x >= heel_least {
            out.push(at(self.point(t_low, self.mu_e)));
        } else {
            let q = self.point(t_at(self.mu_e, heel_least), self.mu_e);
            out.push(at(q));
            out.push(at(V2::new(q.x, low)));
        }
        out.push(at(V2::new(0.0, low)));
        out
    }
    /// The plane up to which the back web runs (between the root at the heel and the loft's
    /// first section) and its radius (between the back cone and the roots there).
    fn web(&self) -> (f64, f64) {
        let z = 0.5 * (self.z_lo + self.height(-self.hf, self.mu_e));
        let bc = self.point(self.across(self.mu_e, z), self.mu_e).x;
        (z, 0.5 * (bc + self.radius(-self.hf, z)))
    }
    /// The section on the plane `z`: radii up a tooth's sides and the two sides' angles.
    fn sides(&self, z: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let top = self.tip(z);
        let mut out = (Vec::new(), Vec::new(), Vec::new());
        for i in 0..self.side {
            let t = -self.hf + (top + self.hf) * spread(i as f64 / (self.side - 1) as f64);
            let mu = (self.hap - z) / (self.hap - t * self.sd);
            let c = self.centre(mu);
            let (wl, wh) = self.widths(t);
            out.0.push(self.radius(t, z));
            out.1.push(c - wl);
            out.2.push(c + wh);
        }
        out
    }
    fn section(&self, z: f64) -> Outline {
        let (r, lo, hi) = self.sides(z);
        teeth_outline(&r, &lo, &hi, self.teeth)
    }
}

/// The closed outline of `n` teeth, each with sides through (`rad[i]`, `lo[i]`) and
/// (`rad[i]`, `hi[i]`) in polar coordinates from the root up: like
/// [`crate::involute::teeth_outline`], but the two sides are their own curves, so teeth may lean
/// and curl.
fn teeth_outline(rad: &[f64], lo: &[f64], hi: &[f64], n: u32) -> Outline {
    let pitch = TAU / f64::from(n);
    let last = rad.len() - 1;
    let mut segs = Vec::with_capacity(4 * n as usize);
    for k in 0..n {
        let a = pitch * f64::from(k);
        segs.push(Seg::Spline((0..=last).map(|i| V2::polar(rad[i], lo[i] + a)).collect()));
        segs.push(Seg::Arc { c: V2::ZERO, a: V2::polar(rad[last], lo[last] + a), b: V2::polar(rad[last], hi[last] + a), ccw: true });
        segs.push(Seg::Spline((0..=last).rev().map(|i| V2::polar(rad[i], hi[i] + a)).collect()));
        segs.push(Seg::Arc { c: V2::ZERO, a: V2::polar(rad[0], hi[0] + a), b: V2::polar(rad[0], lo[0] + a + pitch), ccw: true });
    }
    crate::involute::join_ends(&mut segs);
    Outline { segs }
}

/// Whether the kernel can sketch a tooth's sides: fit points apart, no sharp turn between
/// chords, the tooth and the space both open.
fn sound(rad: &[f64], lo: &[f64], hi: &[f64], pitch: f64) -> bool {
    let n = rad.len();
    let scale = rad[n - 1];
    let ok = |ang: &[f64]| -> bool {
        let p: Vec<V2> = (0..n).map(|i| V2::polar(rad[i], ang[i])).collect();
        p.windows(2).all(|w| w[0].dist(w[1]) > 2e-5 * scale)
            && p.windows(3).all(|w| {
                let (u, v) = (w[1] - w[0], w[2] - w[1]);
                u.cross(v).atan2(u.dot(v)).abs() < 50f64.to_radians()
            })
    };
    rad.iter().chain(lo).chain(hi).all(|v| v.is_finite())
        && rad[0] > 0.0
        && rad.windows(2).all(|w| w[1] > w[0])
        && (0..n).all(|i| hi[i] - lo[i] > 1e-5 && pitch - (hi[i] - lo[i]) > 1e-5)
        && rad[n - 1] * (hi[n - 1] - lo[n - 1]) > 1e-5 * scale
        && rad[0] * (pitch - (hi[0] - lo[0])) > 1e-5 * scale
        && ok(lo)
        && ok(hi)
}

/// Which of the generating gear's solids of revolution a point is tested against.
#[derive(Clone, Copy, PartialEq)]
enum Blank {
    /// Everything under the teeth's tips.
    Tips,
    /// Everything under the teeth's roots.
    Roots,
}

/// One station on a circle about the pinion's axis: everything about the generating gear there
/// that doesn't depend on how far the pair has turned.
#[derive(Clone, Copy)]
struct Station {
    /// Depths inside the outside limit, under the roots and under the tips.
    clip: f64,
    root: f64,
    tip: f64,
    /// Normal length per radian and half the tooth's width round the axis.
    scale: f64,
    half: f64,
    /// The station's angle from the middle of the nearest tooth when the pinion point passing it
    /// is the one at angle 0.
    base: f64,
}

/// A pinion section's teeth: radii from the root up, and one tooth's two sides.
struct Teeth {
    r: Vec<f64>,
    lo: Vec<f64>,
    hi: Vec<f64>,
    pointed: bool,
    undercut: bool,
}

/// Generates the pinion from the ring gear.
///
/// The pinion turns by `t` about its frame's z while the gear turns by `−t·z1/z2` about its own.
/// The generating gear is the ring gear with its teeth carried on beyond the face at both ends,
/// its tips and roots raised by the clearance.
struct Carve<'a> {
    f: &'a Form,
    /// The pinion's frame in the design frame.
    op: V3,
    xp: V3,
    yp: V3,
    zp: V3,
    /// z1 / z2.
    ratio: f64,
    clearance: f64,
    /// The generating gear's outside radius.
    r_ext: f64,
    pitch: f64,
    /// The direction of the mean point round the pinion's axis, and the farthest a pinion tooth
    /// could reach from it.
    a_mesh: f64,
    reach: f64,
    /// The smallest top land (a length).
    land: f64,
}

impl Carve<'_> {
    fn at(&self, r: f64, a: f64, zeta: f64) -> V3 {
        let (s, c) = a.sin_cos();
        [0, 1, 2].map(|i| self.op[i] + self.zp[i] * zeta + r * (self.xp[i] * c + self.yp[i] * s))
    }
    fn blank(&self, x: V3, which: Blank) -> f64 {
        let rho = x[0].hypot(x[1]);
        let Some((t, mu)) = self.f.coords(rho, x[2]) else { return -1e9 };
        let top = if which == Blank::Tips { self.f.ha + self.clearance } else { -self.f.hf + self.clearance };
        (self.r_ext - rho).min((top - t) * mu)
    }
    /// The radius at which the ray at world angle `a` on the plane `zeta` enters a blank.
    fn enter(&self, zeta: f64, a: f64, which: Blank) -> Option<f64> {
        const STEPS: usize = 40;
        let f = |r: f64| self.blank(self.at(r, a, zeta), which);
        if f(0.0) >= 0.0 {
            return Some(0.0);
        }
        let step = self.reach / STEPS as f64;
        let k = (1..=STEPS).find(|k| f(step * *k as f64) >= 0.0)?;
        let (mut lo, mut hi) = (step * (k - 1) as f64, step * k as f64);
        for _ in 0..26 {
            let mid = 0.5 * (lo + hi);
            if f(mid) >= 0.0 { hi = mid } else { lo = mid }
        }
        Some(lo)
    }
    /// The largest circle on the plane `zeta` that stays out of a blank, and where it touches.
    fn clear(&self, zeta: f64, which: Blank) -> Option<(f64, f64)> {
        const HALF: i32 = 18;
        let step = 1.5 / f64::from(HALF);
        let at = |a: f64| self.enter(zeta, a, which).unwrap_or(f64::INFINITY);
        let (mut best, mut ra) = (f64::INFINITY, self.a_mesh);
        for k in -HALF..=HALF {
            let a = self.a_mesh + step * f64::from(k);
            let r = at(a);
            if r < best {
                (best, ra) = (r, a);
            }
        }
        if !best.is_finite() {
            return None;
        }
        // Golden section about the best ray.
        let g = 0.618_033_988_749_895;
        let (mut lo, mut hi) = (ra - step, ra + step);
        let (mut x1, mut x2) = (hi - g * (hi - lo), lo + g * (hi - lo));
        let (mut f1, mut f2) = (at(x1), at(x2));
        for _ in 0..22 {
            if f1 < f2 {
                hi = x2;
                (x2, f2) = (x1, f1);
                x1 = hi - g * (hi - lo);
                f1 = at(x1);
            } else {
                lo = x1;
                (x1, f1) = (x2, f2);
                x2 = lo + g * (hi - lo);
                f2 = at(x2);
            }
        }
        let (r, a) = if f1 < f2 { (f1, x1) } else { (f2, x2) };
        if r < best { Some((r, a)) } else { Some((best, ra)) }
    }
    /// The stretch of the circle (`r`, `zeta`) about `a` that is under the generating gear's tips.
    fn window(&self, r: f64, zeta: f64, a: f64) -> (f64, f64) {
        let inside = |a: f64| self.blank(self.at(r, a, zeta), Blank::Tips) >= 0.0;
        let step = 0.03;
        let run = |dir: f64| -> f64 {
            let mut k = 1.0;
            while k * step < 1.9 && inside(a + dir * k * step) {
                k += 1.0;
            }
            a + dir * (k + 1.0) * step
        };
        (run(-1.0), run(1.0))
    }
    /// The stations on the circle (`r`, `zeta`) across the window.
    fn stations(&self, r: f64, zeta: f64, win: (f64, f64)) -> Vec<Station> {
        const N: usize = 128;
        (0..=N)
            .map(|k| {
                let a = win.0 + (win.1 - win.0) * k as f64 / N as f64;
                let x = self.at(r, a, zeta);
                let rho = x[0].hypot(x[1]);
                let Some((t, mu)) = self.f.coords(rho, x[2]) else {
                    return Station { clip: -1e9, root: 0.0, tip: 0.0, scale: 0.0, half: 0.0, base: 0.0 };
                };
                let (w_lo, w_hi) = self.f.widths(t);
                Station {
                    clip: self.r_ext - rho,
                    root: (-self.f.hf + self.clearance - t) * mu,
                    tip: (self.f.ha + self.clearance - t) * mu,
                    scale: mu * self.f.lat,
                    half: 0.5 * (w_lo + w_hi),
                    // Pinion point θ passes world angle a at t = a − θ, when the gear has turned
                    // back by t·ratio. (A tooth's middle is off its centre line where its two
                    // flanks have different pressure angles.)
                    base: x[1].atan2(x[0]) + a * self.ratio - self.f.centre(mu) - 0.5 * (w_hi - w_lo),
                }
            })
            .collect()
    }
    /// The deepest the pinion point at angle `theta` on the stations' circle ever is inside the
    /// generating gear (negative: it is never reached, the pinion keeps it).
    fn depth(&self, st: &[Station], theta: f64) -> f64 {
        let at = |s: &Station| -> f64 {
            let d = wrap_period(s.base - theta * self.ratio, self.f.pitch);
            let lat = s.scale * (s.half - d.abs());
            s.clip.min(s.root.max(s.tip.min(lat)))
        };
        let (mut best, mut kb) = (f64::NEG_INFINITY, 0);
        let mut v = [0.0f64; 3];
        for (k, s) in st.iter().enumerate() {
            let d = at(s);
            if d > best {
                (best, kb) = (d, k);
            }
        }
        if kb > 0 && kb + 1 < st.len() {
            v[0] = at(&st[kb - 1]);
            v[1] = best;
            v[2] = at(&st[kb + 1]);
            // The parabola through the three stations about the deepest.
            let bend = v[0] - 2.0 * v[1] + v[2];
            if bend < -1e-300 {
                let off = (0.5 * (v[0] - v[2]) / bend).clamp(-1.0, 1.0);
                best = v[1] - 0.25 * (v[0] - v[2]) * off;
            }
        }
        best
    }
    /// Where the depth crosses zero between `free` (negative) and `cut` (not): the last angle
    /// still free.
    fn edge(&self, st: &[Station], mut free: f64, mut cut: f64) -> f64 {
        let (mut ff, mut fc) = (self.depth(st, free), self.depth(st, cut));
        for _ in 0..60 {
            if (cut - free).abs() < 1e-10 {
                break;
            }
            // Regula falsi with the Illinois correction, bisection when it stalls.
            let mut x = free + (cut - free) * (-ff / (fc - ff)).clamp(0.02, 0.98);
            if !x.is_finite() {
                x = 0.5 * (free + cut);
            }
            let fx = self.depth(st, x);
            if fx < 0.0 {
                (free, ff) = (x, fx);
                fc *= 0.5;
            } else {
                (cut, fc) = (x, fx);
                ff *= 0.5;
            }
        }
        free
    }
    /// The tooth about `centre` on a circle: its two sides, if any of it is left.
    fn tooth(&self, st: &[Station], centre: f64) -> Option<(f64, f64)> {
        if self.depth(st, centre) >= 0.0 {
            return None;
        }
        let side = |dir: f64| -> Option<f64> {
            // The space's middle is half a pitch off; look nearer if that is free too.
            for k in [12, 11, 10, 9, 8, 6, 4] {
                let cut = centre + dir * self.pitch * f64::from(k) / 24.0;
                if self.depth(st, cut) >= 0.0 {
                    return Some(self.edge(st, centre, cut));
                }
            }
            None
        };
        Some((side(-1.0)?, side(1.0)?))
    }
    /// The pinion's teeth on the plane `zeta`: the tooth nearest `near` (any, if `None`).
    fn section(&self, zeta: f64, near: Option<f64>) -> Result<Teeth, String> {
        let gone = || "no pinion tooth left: use a smaller offset, a larger spiral angle or fewer, larger teeth".to_string();
        let (r_root, a_root) = self.clear(zeta, Blank::Tips).ok_or_else(gone)?;
        let (r_tip, _) = self.clear(zeta, Blank::Roots).ok_or_else(gone)?;
        let depth = r_tip - r_root;
        if !(r_root > 0.05 * r_tip && depth > 1e-6 * r_tip) {
            return Err(gone());
        }
        let mut top = r_tip - 0.004 * depth;
        let win = self.window(top, zeta, a_root);
        // The root circle: a little above the deepest the gear's tips reach, where the space has
        // some width.
        let mut found = None;
        for lift in [0.04, 0.08, 0.14] {
            let r0 = r_root + lift * depth;
            let st = self.stations(r0, zeta, win);
            const SCAN: usize = 36;
            let (mut worst, mut ts) = (f64::NEG_INFINITY, 0.0);
            for k in 0..SCAN {
                let th = self.pitch * k as f64 / SCAN as f64;
                let d = self.depth(&st, th);
                if d > worst {
                    (worst, ts) = (d, th);
                }
            }
            if worst > 0.0 {
                found = Some((r0, st, ts));
                break;
            }
        }
        let (r0, st0, space) = found.ok_or_else(gone)?;
        let mut centre = space + self.pitch / 2.0;
        if self.depth(&st0, centre) >= 0.0 {
            // An uneven space: the freest angle instead.
            let (mut best, mut tb) = (0.0, centre);
            for k in 0..48 {
                let th = space + self.pitch * k as f64 / 48.0;
                let d = self.depth(&st0, th);
                if d < best {
                    (best, tb) = (d, th);
                }
            }
            centre = tb;
        }
        let (lo0, hi0) = self.tooth(&st0, centre).ok_or_else(gone)?;
        // The same tooth as the last section's.
        let turn = near.map_or(0.0, |n| ((n - 0.5 * (lo0 + hi0)) / self.pitch).round() * self.pitch);
        let (lo0, hi0) = (lo0 + turn, hi0 + turn);
        let mut pointed = false;
        for _ in 0..3 {
            let mut t = Teeth { r: vec![r0], lo: vec![lo0], hi: vec![hi0], pointed, undercut: false };
            let mut short = None;
            for i in 1..PINION_SIDE {
                let x = i as f64 / (PINION_SIDE - 1) as f64;
                let r = r0 + (top - r0) * spread(x);
                let c = 0.5 * (t.lo[i - 1] + t.hi[i - 1]);
                match self.tooth(&self.stations(r, zeta, win), c) {
                    Some((lo, hi)) if r * (hi - lo) >= self.land => {
                        t.r.push(r);
                        t.lo.push(lo);
                        t.hi.push(hi);
                    }
                    _ => {
                        short = Some((t.r[i - 1], r, c));
                        break;
                    }
                }
            }
            let Some((mut ok, mut bad, c)) = short else {
                let thick: Vec<f64> = (0..PINION_SIDE).map(|i| t.r[i] * (t.hi[i] - t.lo[i])).collect();
                t.undercut = thick.windows(2).any(|w| w[1] > w[0] + 1e-4 * depth);
                return Ok(t);
            };
            // Pointed: the tip comes down to where the top land is still wide enough.
            pointed = true;
            for _ in 0..14 {
                let mid = 0.5 * (ok + bad);
                match self.tooth(&self.stations(mid, zeta, win), c) {
                    Some((lo, hi)) if mid * (hi - lo) >= self.land => ok = mid,
                    _ => bad = mid,
                }
            }
            if ok - r0 < 0.3 * depth {
                return Err(gone());
            }
            top = ok;
        }
        Err(gone())
    }
}

/// The pinion's frame x axis: square to its axis, towards the gear's +z where it can.
fn pinion_x(a1: V3) -> V3 {
    let z = [0.0, 0.0, 1.0];
    let x = sub(z, mul(a1, dot(z, a1)));
    if len(x) > 1e-6 { unit(x) } else { [1.0, 0.0, 0.0] }
}

/// The pinion's toe and heel: the cones square to its pitch cone at the cone distances `toe`
/// and `heel` from its pitch apex. Points are (radius, s) in its axial half plane, `s` along its
/// axis from the crossing point towards its apex.
///
/// The face it covers by default: the ring gear's face (its toe and heel pitch points taken
/// across to the pinion's pitch cone through the mean point) and a tenth of the ring gear's face
/// width more at each end, as ISO 23509 sizes the pinion's face to cover the wheel's with an
/// increment. `face_width2` sets its own width (about the same middle), `face_offset2` moves it
/// along the cone (towards the heel).
#[derive(Clone, Copy, Debug)]
pub(crate) struct PinionFace {
    pub toe: f64,
    pub heel: f64,
    s_apex: f64,
    sd: f64,
    cd: f64,
    /// Heights over the pitch cone: above every tip, about the tips at the toe, and under the
    /// roots at the toe and at the heel.
    above: f64,
    toe_tip: f64,
    under: f64,
    heel_under: f64,
    margin: f64,
}

impl PinionFace {
    pub(crate) fn new(s: &Spec, d: &Design) -> Result<PinionFace, String> {
        let (g1, g2) = (d.g1(), d.g2());
        let cone = |x: V3| dot(sub(d.apex1, x), g1);
        let a = cone(sub(d.apex2, mul(g2, d.cone_e2)));
        let b = cone(sub(d.apex2, mul(g2, d.cone_m2 - d.b2 / 2.0)));
        let (toe0, heel0) = (a.min(b), a.max(b));
        if !(s.face_width2 >= 0.0 && s.face_width2.is_finite() && s.face_offset2.is_finite()) {
            return Err("the pinion's face width must not be negative".into());
        }
        let mid = 0.5 * (toe0 + heel0) + s.face_offset2;
        let half = if s.face_width2 > 0.0 { 0.5 * s.face_width2 } else { 0.5 * (heel0 - toe0) + 0.1 * d.b2 };
        let (toe, heel) = (mid - half, mid + half);
        if toe < 0.25 * d.cone_m1 {
            return Err(format!(
                "the pinion's face reaches too near its pitch apex: at most {} mm wide here (its toe {} mm from the apex)",
                num(2.0 * (mid - 0.25 * d.cone_m1).max(0.0)),
                num(toe)
            ));
        }
        if toe >= heel0 || heel <= toe0 || heel - toe < 0.1 * (heel0 - toe0) {
            return Err("the pinion's face misses the ring gear's: use a smaller face offset or a wider pinion face".into());
        }
        let (sd, cd) = d.delta1.sin_cos();
        let scale = heel / d.cone_m1;
        let depth = d.ha1 + d.hf1;
        let under = -(d.hf1 * toe / d.cone_m1 + 0.5 * d.mmn);
        let heel_under = -(d.hf1 * heel / d.cone_m1 + 0.5 * d.mmn);
        Ok(PinionFace {
            toe,
            heel,
            s_apex: dot(sub(d.apex1, d.o1), d.a1),
            sd,
            cd,
            above: 2.0 * depth * scale + d.mmn,
            toe_tip: 1.3 * (d.ha1 + d.clearance) * toe / d.cone_m1,
            under: under.max(-0.8 * toe * sd / cd),
            heel_under: heel_under.max(-0.8 * heel * sd / cd),
            margin: 0.25 * d.mmn,
        })
    }
    /// The point `h` over the pitch cone at cone distance `cone`.
    pub(crate) fn at(&self, cone: f64, h: f64) -> V2 {
        V2::new(cone * self.sd + h * self.cd, self.s_apex - cone * self.cd + h * self.sd)
    }
    /// Where the cone square to the pitch cone at `cone` is at radius `r`.
    pub(crate) fn s_on(&self, cone: f64, r: f64) -> f64 {
        self.at(cone, (r - cone * self.sd) / self.cd).y
    }
    /// The heel's flat back (square to the axis, under the roots, where the shank starts).
    pub(crate) fn back(&self) -> V2 {
        self.at(self.heel, self.heel_under)
    }
    /// The stretch of the axis the toothed loft takes (heel side first): from behind the heel's
    /// flat back to past the toe cone at the tips, so the cones cut through whole teeth.
    fn span(&self) -> (f64, f64) {
        (self.back().y - self.margin, self.at(self.toe, self.toe_tip).y + self.margin)
    }
    /// The pinion's blank (radius, s): flat from the axis to under the roots at the heel, the
    /// heel cone to above the tips, a line over the tips, the toe cone down to under the roots,
    /// then flat to the axis.
    fn blank(&self) -> Vec<V2> {
        let (high, low) = (self.back(), self.at(self.toe, self.under));
        vec![V2::new(0.0, high.y), high, self.at(self.heel, self.above), self.at(self.toe, self.above), low, V2::new(0.0, low.y)]
    }
}

/// The set, worked out: the ring gear's form, the pinion's frame and both members' sections.
pub(crate) struct Set {
    pub d: Design,
    form: Form,
    /// The pinion's frame (design frame) and whether its heel is at its z = 0; its toe and heel
    /// cones; its frame z is `s − s_min` (forward) or `s_max − s`.
    op: V3,
    xp: V3,
    zp: V3,
    heel_first: bool,
    face: PinionFace,
    forward: bool,
    s_min: f64,
    s_max: f64,
    /// Heights of the pinion's sections along its axis, and its teeth there.
    zeta: Vec<f64>,
    teeth: Vec<Teeth>,
    /// The sections the pinion is lofted through: the exact ones and more between.
    loft_z: Vec<f64>,
    loft: Vec<Teeth>,
    /// How many sections the ring gear is lofted through.
    gear_n: usize,
}

impl Set {
    /// The pinion frame's z of the axial position `s`, and back.
    fn z_of(&self, s: f64) -> f64 {
        if self.forward { s - self.s_min } else { self.s_max - s }
    }
    /// A pinion radius along the axis (the tips' or the roots', `pick`), straight between the
    /// exact sections.
    fn pinion_radius(&self, s: f64, pick: &dyn Fn(&Teeth) -> f64) -> f64 {
        let n = self.zeta.len();
        let z = self.z_of(s).clamp(self.zeta[0], self.zeta[n - 1]);
        let k = (0..n - 1).rfind(|k| self.zeta[*k] <= z).unwrap_or(0);
        let u = (z - self.zeta[k]) / (self.zeta[k + 1] - self.zeta[k]);
        pick(&self.teeth[k]) * (1.0 - u) + pick(&self.teeth[k + 1]) * u
    }
    /// Where the pinion's tips (or roots) meet the cone square to its pitch cone at `cone`: the
    /// radius there.
    fn pinion_on_cone(&self, cone: f64, pick: &dyn Fn(&Teeth) -> f64) -> f64 {
        // The crossing nearest the pitch radius, by a scan and halving (a fixed-point iteration
        // runs away on steep pinion cones).
        let r0 = self.face.at(cone, 0.0).x;
        let f = |r: f64| self.pinion_radius(self.face.s_on(cone, r), pick) - r;
        let n = 200;
        let at = |k: usize| r0 * (0.02 + 2.98 * k as f64 / n as f64);
        let mut best: Option<(f64, f64, f64)> = None;
        for k in 0..n {
            let (a, b) = (at(k), at(k + 1));
            if f(a).signum() != f(b).signum() && best.is_none_or(|x| (a - r0).abs() < (x.0 - r0).abs()) {
                best = Some((a, a, b));
            }
        }
        let Some((_, mut lo, mut hi)) = best else { return self.pinion_radius(self.face.at(cone, 0.0).y, pick) };
        let flo = f(lo).signum();
        for _ in 0..50 {
            let mid = 0.5 * (lo + hi);
            if f(mid).signum() == flo { lo = mid } else { hi = mid }
        }
        0.5 * (lo + hi)
    }
}

/// Works the set out in the design frame; when its teeth twist too fast for the kernel to loft
/// at the face width asked, at the widest face (in steps of a fifth) that lofts, with a warning.
pub(crate) fn work_out(s: &Spec) -> Result<Set, String> {
    let first = work_out_at(s);
    let Err(e) = &first else { return first };
    if !e.contains("twist too fast to loft") {
        return first;
    }
    let mut t = s.clone();
    for _ in 0..8 {
        t.face_width *= 0.8;
        if let Ok(mut set) = work_out_at(&t) {
            set.d.warnings.push(format!("face width cut to {} mm: wider teeth twist too fast to loft", num(set.d.b2)));
            return Ok(set);
        }
    }
    first
}

fn work_out_at(s: &Spec) -> Result<Set, String> {
    work_out_design(s, design(s)?)
}

/// [`work_out_at`] for a design already worked out (`design_iso`'s, when `design` checks an offset).
fn work_out_design(s: &Spec, d: Design) -> Result<Set, String> {
    let form = Form::new(s, &d)?;
    // The pinion is cut by the ring gear without its backlash, so the backlash stays between
    // the two (cut by the thinned gear, the pinion would grow into it).
    let cutter = if s.backlash > 0.0 { Form::new(&Spec { backlash: 0.0, ..s.clone() }, &d)? } else { form.clone() };
    // The pinion's toothed loft: through its face and past its toe and heel cones, which trim it.
    let face = PinionFace::new(s, &d)?;
    let (s_heel, s_toe) = face.span();
    let length = (s_toe - s_heel).abs();
    if !(length > 1e-6 * d.rm2) {
        return Err("the pinion's axis runs square to the ring gear's face: no pinion fits".into());
    }
    // The pinion's z axis: the way round that makes the gear turn by −t·z1/z2 when the pinion
    // turns by +t (the pitch surfaces then slide along the tooth line only).
    let ratio = d.z1 / d.z2;
    let across = cross(d.n2, d.tooth);
    let miss = |zp: V3| -> f64 {
        let v1 = cross(zp, sub(d.p, d.o1));
        let v2 = mul(cross([0.0, 0.0, 1.0], d.p), -ratio);
        dot(sub(v1, v2), across).abs()
    };
    let forward = miss(d.a1) <= miss(mul(d.a1, -1.0));
    let zp = if forward { d.a1 } else { mul(d.a1, -1.0) };
    let (s_min, s_max) = (s_heel.min(s_toe), s_heel.max(s_toe));
    let op = add(d.o1, mul(d.a1, if forward { s_min } else { s_max }));
    let heel_first = (s_heel < s_toe) == forward;
    let xp = pinion_x(zp);
    let yp = cross(zp, xp);
    let rel = sub(d.p, op);
    let carve = Carve {
        f: &cutter,
        op,
        xp,
        yp,
        zp,
        ratio,
        clearance: d.clearance,
        r_ext: form.r_out + d.b2 + 2.0 * d.rm1,
        pitch: TAU / d.z1,
        a_mesh: dot(rel, yp).atan2(dot(rel, xp)),
        reach: 3.0 * d.rm1 + 4.0 * (d.ha1 + d.hf1),
        land: 0.12 * d.mmn,
    };
    // How fast the tooth turns along the pinion's axis at the mean point, to follow one tooth
    // from section to section.
    let radial = unit(sub(rel, mul(zp, dot(rel, zp))));
    let round = cross(zp, radial);
    let lead = dot(d.tooth, zp);
    let slope = if lead.abs() > 1e-9 { dot(d.tooth, round) / (d.rm1 * lead) } else { 0.0 };
    let zeta: Vec<f64> = (0..PINION_SECTIONS).map(|j| length * j as f64 / (PINION_SECTIONS - 1) as f64).collect();
    let mut teeth: Vec<Teeth> = Vec::new();
    for (j, z) in zeta.iter().enumerate() {
        let mid = |t: &Teeth| 0.5 * (t.lo[0] + t.hi[0]);
        let near = match j {
            0 => None,
            1 => Some(mid(&teeth[0]) + slope * (zeta[1] - zeta[0])),
            _ => Some(2.0 * mid(&teeth[j - 1]) - mid(&teeth[j - 2])),
        };
        let t = carve.section(*z, near)?;
        if !sound(&t.r, &t.lo, &t.hi, carve.pitch) {
            return Err("pinion tooth can't be drawn: use a smaller offset or face width, or fewer, larger teeth".into());
        }
        teeth.push(t);
    }
    // The ring gear: GEAR_SECTIONS sections where the kernel takes that many curves, more if it
    // needs them to line the sections up tooth to tooth, and fewer fit points a flank (more
    // twist between sections taken) when that isn't enough.
    let mut form = form;
    let mut gear_n = None;
    'sides: for side in [GEAR_SIDE, 5, 4] {
        form.side = side;
        let most = (KERNEL_CURVES / (s.teeth2 as usize * 2 * side)).min(50);
        let first = GEAR_SECTIONS.min(most);
        if first < 4 {
            continue;
        }
        for n in std::iter::once(first)
            .chain(LOFT_COUNTS.into_iter().filter(|n| *n > first && *n <= most))
            .chain((most > 49).then_some(most).filter(|m| *m <= 50))
        {
            let mut gear = Vec::new();
            for j in 0..n {
                let z = form.z_lo + (form.z_hi - form.z_lo) * j as f64 / (n - 1) as f64;
                let (r, lo, hi) = form.sides(z);
                if !sound(&r, &lo, &hi, form.pitch) {
                    return Err("ring gear tooth can't be drawn: use a smaller face width or fewer, larger teeth".into());
                }
                gear.push(teeth_outline(&r, &lo, &hi, s.teeth2));
            }
            if cfg!(test) && std::env::var("HYPOID_TRACE").is_ok() {
                eprintln!("gear loft of {n} ({side} a side): restarts {:?}", restarts(&gear, s.teeth2 as usize));
            }
            if !slips(&gear, s.teeth2 as usize) {
                gear_n = Some(n);
                break 'sides;
            }
        }
    }
    // Teeth that turn too far between sections for the kernel's matching to follow still loft:
    // the engine lofts the sections as given, piece for piece (`kernel::loft_as_given`), each wall
    // on its own tooth. They keep the full flank and get as many sections as the kernel takes.
    let gear_n = match gear_n {
        Some(n) => n,
        None => {
            form.side = GEAR_SIDE;
            let most = (KERNEL_CURVES / (s.teeth2 as usize * 2 * GEAR_SIDE)).min(50);
            if most < 4 {
                return Err(format!("too many ring gear teeth to loft: use at most {}", KERNEL_CURVES / (4 * 2 * GEAR_SIDE)));
            }
            most
        }
    };
    // The pinion's teeth turn fast along its axis: the kernel would line its sections up
    // tooth to tooth only when they are close enough, so it gets as many as that takes.
    // (When none lines up, the most sections drawn: lofted as given, as the ring gear's.)
    let mut lofted = None;
    let mut densest = None;
    let most = KERNEL_CURVES / (s.teeth as usize * 2 * PINION_SIDE);
    for n in LOFT_COUNTS.into_iter().filter(|n| *n <= most) {
        let (z, t) = dense(&zeta, &teeth, n);
        let drawn = t.iter().all(|t| sound(&t.r, &t.lo, &t.hi, carve.pitch));
        if cfg!(test) && std::env::var("HYPOID_TRACE").is_ok() {
            let outlines: Vec<Outline> = t.iter().map(|t| teeth_outline(&t.r, &t.lo, &t.hi, s.teeth)).collect();
            eprintln!("pinion loft of {n}: drawn {drawn}, restarts {:?}", restarts(&outlines, s.teeth as usize));
        }
        if !drawn {
            continue;
        }
        let outlines: Vec<Outline> = t.iter().map(|t| teeth_outline(&t.r, &t.lo, &t.hi, s.teeth)).collect();
        if !slips(&outlines, s.teeth as usize) {
            lofted = Some((z, t));
            break;
        }
        densest = Some((z, t));
    }
    let Some((loft_z, loft)) = lofted.or(densest) else {
        return Err("pinion tooth can't be drawn at any number of sections: use a smaller offset or face width".into());
    };
    Ok(Set { d, form, op, xp, zp, heel_first, face, forward, s_min, s_max, zeta, teeth, loft_z, loft, gear_n })
}

/// A hypoid set's axes and pitch cones without its teeth, for a fast preview
/// ([`crate::simple::schematic`]): the design, where the bodies' frames go (as [`build`] places
/// them about `s.origin`, before the right-hand mirror and `s.placement`), the ring gear's
/// outside radius and the pinion's toothed length. The same steps as [`work_out`] up to its
/// sections, which are what take the time.
pub(crate) struct Layout {
    pub d: Design,
    /// The pinion's body frame (origin, x, z) and the gear's origin, world.
    pub pinion_origin: V3,
    pub pinion_x: V3,
    pub pinion_z: V3,
    pub gear_origin: V3,
    /// The gear's back face below the design frame's z = 0 (the body frame's z = 0), its teeth's
    /// top, and its outside radius.
    pub back: f64,
    pub z_hi: f64,
    pub r_out: f64,
    pub length: f64,
    /// The pinion's pitch cone at its heel and toe: (radius, frame z) each.
    pub pinion_heel: [f64; 2],
    pub pinion_toe: [f64; 2],
}

pub(crate) fn layout(s: &Spec) -> Result<Layout, String> {
    let d = design(s)?;
    let form = Form::new(s, &d)?;
    let face = PinionFace::new(s, &d)?;
    let (s_heel, s_toe) = face.span();
    let length = (s_toe - s_heel).abs();
    let ratio = d.z1 / d.z2;
    let across = cross(d.n2, d.tooth);
    let miss = |zp: V3| -> f64 {
        let v1 = cross(zp, sub(d.p, d.o1));
        let v2 = mul(cross([0.0, 0.0, 1.0], d.p), -ratio);
        dot(sub(v1, v2), across).abs()
    };
    let forward = miss(d.a1) <= miss(mul(d.a1, -1.0));
    let zp = if forward { d.a1 } else { mul(d.a1, -1.0) };
    let (s_min, s_max) = (s_heel.min(s_toe), s_heel.max(s_toe));
    let op = add(d.o1, mul(d.a1, if forward { s_min } else { s_max }));
    let web = if s.rim > 0.0 { s.rim } else { 0.06 * 2.0 * d.re2 };
    let back = form.z_lo - web;
    let world = |q: V3| -> V3 { [q[0] + s.origin[0], q[1] + s.origin[1], q[2] - back + s.origin[2]] };
    let z_of = |q: V2| [q.x, if forward { q.y - s_min } else { s_max - q.y }];
    let (pinion_heel, pinion_toe) = (z_of(face.at(face.heel, 0.0)), z_of(face.at(face.toe, 0.0)));
    Ok(Layout {
        pinion_origin: world(op),
        pinion_x: pinion_x(zp),
        pinion_z: zp,
        gear_origin: world([0.0, 0.0, back]),
        back,
        z_hi: form.z_hi,
        r_out: form.r_out,
        length,
        pinion_heel,
        pinion_toe,
        d,
    })
}

/// Builds the hypoid set into `p`.
pub fn build(s: &Spec, p: &mut Plan) -> Result<(), String> {
    let set = work_out(s)?;
    let (d, form) = (&set.d, &set.form);
    for w in &d.warnings {
        p.warn(w.clone());
    }
    let hand = if s.left_hand { "left" } else { "right" };
    let other = if s.left_hand { "right" } else { "left" };
    let n = set.zeta.len();
    let length = set.zeta[n - 1];
    // Every section has its teeth (said here, once), so the fallback below is never used.
    if set.teeth.iter().any(|t| t.r.is_empty()) {
        return Err("the pinion's teeth came out empty: no section to build".into());
    }
    let tip = |t: &Teeth| t.r.last().copied().unwrap_or(0.0);
    let root = |t: &Teeth| t.r[0];
    let face = set.face;
    // The gear's back face goes to the origin.
    let web = if s.rim > 0.0 { s.rim } else { 0.06 * 2.0 * d.re2 };
    let back = form.z_lo - web;
    p.fact("ratio", d.z2 / d.z1, Unit::None);
    p.fact("offset", d.offset, Unit::Mm);
    p.fact("shaft angle", d.sigma.to_degrees(), Unit::Deg);
    p.fact(format!("pinion mean spiral angle ({hand} hand)"), d.beta1.to_degrees(), Unit::Deg);
    p.fact(format!("gear mean spiral angle ({other} hand)"), d.beta2.to_degrees(), Unit::Deg);
    p.fact("offset angle in the pitch plane", d.eps_pitch.to_degrees(), Unit::Deg);
    p.fact("offset angle round the gear axis", d.eps_gear.to_degrees(), Unit::Deg);
    p.fact("offset angle round the pinion axis", d.eta_pinion.to_degrees(), Unit::Deg);
    p.fact("pinion pitch cone angle", d.delta1.to_degrees(), Unit::Deg);
    p.fact("gear pitch cone angle", d.delta2.to_degrees(), Unit::Deg);
    p.fact("gear face cone angle", (d.delta2 + (d.ha2 / d.cone_m2).atan()).to_degrees(), Unit::Deg);
    p.fact("gear root cone angle", (d.delta2 - (d.hf2 / d.cone_m2).atan()).to_degrees(), Unit::Deg);
    p.fact("pinion mean pitch diameter", 2.0 * d.rm1, Unit::Mm);
    p.fact("gear mean pitch diameter", 2.0 * d.rm2, Unit::Mm);
    p.fact("gear outer pitch diameter", 2.0 * d.re2, Unit::Mm);
    p.fact("gear outside diameter", 2.0 * form.r_out, Unit::Mm);
    let gear_inner = form.point(form.ha, form.mu_i).x;
    p.fact("gear inner tip diameter", 2.0 * gear_inner, Unit::Mm);
    let heel_root = set.pinion_on_cone(face.heel, &root);
    let toe_root = set.pinion_on_cone(face.toe, &root);
    p.fact("pinion heel tip diameter", 2.0 * set.pinion_on_cone(face.heel, &tip), Unit::Mm);
    p.fact("pinion heel root diameter", 2.0 * heel_root, Unit::Mm);
    p.fact("pinion toe tip diameter", 2.0 * set.pinion_on_cone(face.toe, &tip), Unit::Mm);
    p.fact("pinion toe root diameter", 2.0 * toe_root, Unit::Mm);
    p.fact("pinion mean cone distance", d.cone_m1, Unit::Mm);
    p.fact("gear mean cone distance", d.cone_m2, Unit::Mm);
    p.fact("gear outer cone distance", d.cone_e2, Unit::Mm);
    p.fact("gear face width", d.b2, Unit::Mm);
    p.fact("pinion face width", face.heel - face.toe, Unit::Mm);
    p.fact("pinion heel cone distance", face.heel, Unit::Mm);
    p.fact("pinion toe cone distance", face.toe, Unit::Mm);
    p.fact("pinion toothed length", (face.at(face.heel, 0.0).y - face.at(face.toe, 0.0).y).abs(), Unit::Mm);
    p.fact("pinion loft sections", set.loft_z.len() as f64, Unit::None);
    p.fact("mean normal module", d.mmn, Unit::None);
    p.fact("pinion mean addendum", d.ha1, Unit::Mm);
    p.fact("pinion mean dedendum", d.hf1, Unit::Mm);
    p.fact("gear mean addendum", d.ha2, Unit::Mm);
    p.fact("gear mean dedendum", d.hf2, Unit::Mm);
    p.fact("limit pressure angle", d.alpha_lim.to_degrees(), Unit::Deg);
    if d.offset >= 1e-3 {
        p.fact("limit radius of lengthwise curvature", rho_lim(d), Unit::Mm);
    }
    let (convex, concave) = if d.beta2 >= 0.0 { (form.tan_hi, form.tan_lo) } else { (form.tan_lo, form.tan_hi) };
    p.fact("gear convex flank pressure angle", convex.atan().to_degrees(), Unit::Deg);
    p.fact("gear concave flank pressure angle", concave.atan().to_degrees(), Unit::Deg);
    p.fact("pinion enlargement over a bevel pinion", d.rm1 * d.z2 / (d.rm2 * d.z1), Unit::None);
    p.fact("pinion axis above the gear's back face", d.h - back, Unit::Mm);
    p.fact("gear pitch apex above the gear's back face", d.apex2[2] - back, Unit::Mm);
    p.fact("pinion pitch apex beyond the crossing point", dot(sub(d.apex1, d.o1), d.a1), Unit::Mm);
    p.fact("gear pitch apex beyond the crossing point", d.apex2[2] - d.h, Unit::Mm);
    if set.teeth.iter().any(|t| t.pointed) {
        p.warn("pinion: the teeth come to a point; the tips are lowered to keep a top land");
    }
    if set.teeth.iter().any(|t| t.undercut) {
        p.warn("pinion: undercut teeth (the ring gear's tips cut into the flanks near the roots): fewer, larger teeth on the ring gear, a larger pressure angle or a smaller offset avoid it");
    }
    let twist = (0.5 * (set.teeth[n - 1].lo[0] + set.teeth[n - 1].hi[0]) - 0.5 * (set.teeth[0].lo[0] + set.teeth[0].hi[0])).abs();
    if twist / (n - 1) as f64 > 0.75 * TAU / d.z1 {
        p.warn(
            "pinion: the teeth wind more than three quarters of a pitch between sections; the solid follows them loosely (use a smaller face width)",
        );
    }
    // World placement.
    let world = |q: V3| -> V3 { [q[0] + s.origin[0], q[1] + s.origin[1], q[2] - back + s.origin[2]] };
    let circle = |r: f64| Region { outer: vec![Outline::circle(V2::ZERO, r)], holes: vec![] };
    let mut bodies = Vec::new();
    let pinion_sections: Vec<(Outline, f64)> =
        set.loft_z.iter().zip(&set.loft).map(|(z, t)| (teeth_outline(&t.r, &t.lo, &t.hi, s.teeth), *z)).collect();
    let gear_z: Vec<f64> = (0..set.gear_n).map(|j| form.z_lo + (form.z_hi - form.z_lo) * j as f64 / (set.gear_n - 1) as f64).collect();
    let gear_sections: Vec<(Outline, f64)> = gear_z.iter().map(|z| (form.section(*z), z - back)).collect();
    if wanted(s, 1) {
        // The teeth trimmed to the toe and heel cones, then a shank out of the heel cone, a
        // little into the body so the two overlap.
        let blank: Vec<V2> = face.blank().into_iter().map(|q| V2::new(q.x, set.z_of(q.y))).collect();
        let heel_r = heel_root.min(face.back().x) * 0.97;
        let shank_r = if s.hub_diameter > 0.0 { (s.hub_diameter / 2.0).min(heel_r) } else { 0.6 * heel_r };
        let toothed = (face.at(face.heel, 0.0).y - face.at(face.toe, 0.0).y).abs();
        let shank_l = if s.hub_length > 0.0 { s.hub_length } else { 0.8 * toothed };
        let lap = 0.5 * d.mmn;
        let z_sh = set.z_of(face.back().y);
        let z0 = if set.heel_first { z_sh - shank_l } else { z_sh - lap };
        let mut steps = vec![Step::Loft { sections: pinion_sections.clone(), op: Op::New }];
        steps.push(Step::Revolve { profile: blank, op: Op::Intersect });
        steps.push(Step::Prism { region: circle(shank_r), z0, height: shank_l + lap, twist: 0.0, op: Op::Join });
        if let Some(b) = s.bore.outline() {
            let nose = face.at(face.toe, face.under).x;
            let most = 0.8 * shank_r.min(toe_root).min(nose);
            if s.bore.reach() > most {
                p.refuse(format!(
                    "pinion: the bore (reaching Ø{}) leaves no wall in the shank; at most Ø{}",
                    num(2.0 * s.bore.reach()),
                    num(2.0 * most)
                ));
            }
            let (lo, hi) = (z0.min(0.0) - 1.0, (z0 + shank_l + lap).max(length) + 1.0);
            steps.push(Step::Prism { region: Region { outer: vec![b], holes: vec![] }, z0: lo, height: hi - lo, twist: 0.0, op: Op::Cut });
        }
        bodies.push(Body {
            name: name(s, &format!("Hypoid pinion z{}", s.teeth)),
            frame: Frame { origin: world(set.op), x: set.xp, z: set.zp },
            steps,
        });
    }
    if wanted(s, 2) {
        let mut steps = vec![Step::Loft { sections: gear_sections.clone(), op: Op::New }];
        // The back: a disc under the roots, into the loft; then the blank trims the teeth to
        // the toe and heel cones and the disc to the back cone.
        let (web_z, web_r) = form.web();
        steps.push(Step::Prism { region: circle(web_r), z0: 0.0, height: web_z - back, twist: 0.0, op: Op::Join });
        steps.push(Step::Revolve { profile: form.blank(back), op: Op::Intersect });
        if let Some(b) = s.bore2.outline() {
            // Through the flat faces only (inside the front face under the toe).
            let most = form.blank(back)[1].x * 0.95;
            if s.bore2.reach() > most {
                p.refuse(format!(
                    "gear 2: the bore (reaching Ø{}) cuts into the tooth roots; at most Ø{}",
                    num(2.0 * s.bore2.reach()),
                    num(2.0 * most)
                ));
            }
            steps.push(Step::Prism {
                region: Region { outer: vec![b], holes: vec![] },
                z0: -1.0,
                height: form.z_hi - back + 2.0,
                twist: 0.0,
                op: Op::Cut,
            });
        }
        bodies.push(Body {
            name: name(s, &format!("Hypoid gear z{}", s.teeth2)),
            frame: Frame { origin: world([0.0, 0.0, back]), x: [1.0, 0.0, 0.0], z: [0.0, 0.0, 1.0] },
            steps,
        });
    }
    // The 2D preview: both members' middle sections side by side, turning at the set's ratio (the
    // axes are skew, so no one plane shows the mesh itself).
    // (The sections through the mean point: the ring gear's plane z = 0, the pinion's exact
    // section nearest it.)
    let z_mean = set.z_of(dot(sub(d.p, d.o1), d.a1));
    let mid_p = (0..n).min_by(|a, b| (set.zeta[*a] - z_mean).abs().total_cmp(&(set.zeta[*b] - z_mean).abs())).unwrap_or(n / 2);
    let mut pinion_view = {
        let t = &set.teeth[mid_p];
        teeth_outline(&t.r, &t.lo, &t.hi, s.teeth)
    };
    let mut gear_view = form.section(0.0);
    let gear_reach = form.radius(form.ha, 0.0);
    let centre = V2::new(gear_reach + tip(&set.teeth[mid_p]) + 0.5 * d.mmn, 0.0);
    if !s.left_hand {
        // A right-hand pinion: the whole set mirrored in y (offset to the other side).
        for b in &mut bodies {
            mirror_body(b, s.origin[1]);
        }
        let my = |q: V2| V2::new(q.x, -q.y);
        pinion_view = pinion_view.map(&my, true);
        gear_view = gear_view.map(&my, true);
    }
    p.bodies.extend(bodies);
    p.preview.push(PreviewGear::turning("pinion", Region { outer: vec![pinion_view], holes: vec![] }, centre, 1.0, 0.0));
    p.preview.push(PreviewGear::turning(
        "gear",
        Region { outer: vec![gear_view], holes: s.bore2.outline().into_iter().collect() },
        V2::ZERO,
        -d.z1 / d.z2,
        form.radius(0.0, 0.0),
    ));
    p.preview_note = format!(
        "middle sections of the pinion and the ring gear, side by side at their speeds (the axes are {} mm apart at {}° and don't meet: {hand}-hand pinion)",
        num(d.offset),
        num(d.sigma.to_degrees())
    );
    Ok(())
}

fn mirror_region(r: &Region, f: &dyn Fn(V2) -> V2) -> Region {
    Region { outer: r.outer.iter().map(|o| o.map(f, true)).collect(), holes: r.holes.iter().map(|o| o.map(f, true)).collect() }
}

/// A body mirrored in the plane y = `y0` (world): the frame mirrored, its local y negated.
fn mirror_body(b: &mut Body, y0: f64) {
    let m = |v: V3| [v[0], -v[1], v[2]];
    let f = b.frame;
    b.frame = Frame { origin: [f.origin[0], 2.0 * y0 - f.origin[1], f.origin[2]], x: m(f.x), z: m(f.z) };
    for st in &mut b.steps {
        mirror_step(st);
    }
}

/// A step mirrored in its frame's plane y = 0.
fn mirror_step(st: &mut Step) {
    let my = |q: V2| V2::new(q.x, -q.y);
    match st {
        Step::Prism { region, twist, .. } => {
            *region = mirror_region(region, &my);
            *twist = -*twist;
        }
        Step::Taper { outline, twist, .. } | Step::RingTaper { outline, twist, .. } => {
            *outline = outline.map(&my, true);
            *twist = -*twist;
        }
        Step::Loft { sections, .. } => {
            for (o, _) in sections.iter_mut() {
                *o = o.map(&my, true);
            }
        }
        Step::Revolve { .. } => {}
        Step::Coil { left, .. } => *left = !*left,
        // Its frame mirrored like the body's (its local y negated), its own geometry with it.
        Step::Placed { frame, step } => {
            let m = |v: V3| [v[0], -v[1], v[2]];
            *frame = Frame { origin: m(frame.origin), x: m(frame.x), z: m(frame.z) };
            mirror_step(step);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Kind;

    fn spec() -> Spec {
        Spec::defaults_for(Kind::Hypoid)
    }

    /// The pitch cones touch at P and agree on the normal pitch; the pinion's apex is on its axis.
    fn consistent(d: &Design) {
        let g1 = d.g1();
        let g2 = d.g2();
        let n1 = mul(d.n2, -1.0);
        assert!(dot(g1, d.n2).abs() < 1e-9 && dot(g2, d.n2).abs() < 1e-9);
        assert!((dot(n1, d.a1) - d.delta1.sin()).abs() < 1e-9);
        // The pinion apex is on its axis and the gear apex on z.
        let rel = sub(d.apex1, d.o1);
        assert!(len(sub(rel, mul(d.a1, dot(rel, d.a1)))) < 1e-7, "pinion apex off its axis");
        assert!(d.apex2[0].abs() < 1e-9 && d.apex2[1].abs() < 1e-9);
        // Pitch radii from the cones.
        assert!((d.cone_m1 * d.delta1.sin() - d.rm1).abs() < 1e-9);
        assert!((d.cone_m2 * d.delta2.sin() - d.rm2).abs() < 1e-9);
        // Equal normal pitch: m_mn = 2 r cos β / z for both.
        let mn1 = 2.0 * d.rm1 * d.beta1.cos() / d.z1;
        let mn2 = 2.0 * d.rm2 * d.beta2.cos() / d.z2;
        assert!((mn1 - mn2).abs() < 1e-6 * mn2, "{mn1} {mn2}");
        // The angle between the generatrices is ε′ and β1 − β2 = ε′.
        let between = dot(g1, g2).clamp(-1.0, 1.0).acos();
        assert!((between - d.eps_pitch.abs()).abs() < 1e-9);
        assert!((d.beta1 - d.beta2 - d.eps_pitch).abs() < 1e-12);
        // The tooth line is in the pitch plane, β2 from the gear's generatrix and β1 from the
        // pinion's.
        assert!(dot(d.tooth, d.n2).abs() < 1e-9);
        assert!((dot(d.tooth, g2) - d.beta2.cos()).abs() < 1e-9 && (dot(d.tooth, g1) - d.beta1.cos()).abs() < 1e-9);
        // The pinion's axis is E from the gear's.
        assert!((d.o1[1].abs() - d.offset).abs() < 1e-9);
        // Turning at the ratio (one way round or the other), the pitch surfaces slide along the
        // tooth line only.
        let across = cross(d.n2, d.tooth);
        let slip = |way: f64| {
            let v1 = cross(mul(d.a1, way), sub(d.p, d.o1));
            let v2 = mul(cross([0.0, 0.0, 1.0], d.p), -d.z1 / d.z2);
            dot(sub(v1, v2), across).abs()
        };
        assert!(slip(1.0).min(slip(-1.0)) < 1e-6 * d.rm1, "{} {}", slip(1.0), slip(-1.0));
    }

    #[test]
    fn zero_offset_is_a_spiral_bevel_pair() {
        let mut s = spec();
        s.offset = 0.0;
        let d = design(&s).unwrap();
        consistent(&d);
        assert!(d.eps_pitch.abs() < 1e-9);
        assert!((d.beta1 - d.beta2).abs() < 1e-9);
        assert!(d.alpha_lim.abs() < 1e-9);
        // Cone angles of a 90° bevel pair, a common apex.
        assert!((d.delta1 + d.delta2 - PI / 2.0).abs() < 1e-9);
        assert!((d.delta2.tan() - 41.0 / 11.0).abs() < 1e-9);
        assert!(len(sub(d.apex1, d.apex2)) < 1e-6);
        assert!((d.rm1 / d.rm2 - 11.0 / 41.0).abs() < 1e-9);
    }

    /// The pitch cones of a published ISO 23509 Method 1 (Gleason, face milled) set: MITcalc's
    /// hypoid example, z 15:45, Σ 90°, a 17 mm, d_e2 99 mm, b2 15.3 mm, r_c0 35 mm. Given the
    /// wheel's pitch angle and mean radius the standard's iteration ended on (δ2 61.3634°,
    /// r_m2 42.3972 mm) and its pinion spiral angle (48.9797°), the tangent-plane and normal-pitch
    /// conditions here give back the rest.
    #[test]
    fn matches_a_published_iso_23509_set() {
        let mut s = spec();
        (s.teeth, s.teeth2, s.offset, s.face_width, s.spiral_angle) = (15, 45, 17.0, 15.3, 48.9797);
        s.module = 99.0 / 45.0;
        let d = design_with(&s, Some((61.3634f64.to_radians(), 42.3972))).unwrap();
        consistent(&d);
        let deg = |v: f64| v.to_degrees();
        eprintln!(
            "δ1 {:.4} β2 {:.4} ζmp {:.4} Rm1 {:.4} Rm2 {:.4} rm1 {:.4} αlim {:.4} ρlim {:.3} mmn {:.4}",
            deg(d.delta1),
            deg(d.beta2),
            deg(d.eps_pitch),
            d.cone_m1,
            d.cone_m2,
            d.rm1,
            deg(d.alpha_lim),
            rho_lim(&d),
            d.mmn
        );
        assert!((deg(d.delta1) - 26.9735).abs() < 0.01, "δ1 {}", deg(d.delta1));
        assert!((deg(d.beta2) - 27.7365).abs() < 0.01, "β2 {}", deg(d.beta2));
        assert!((deg(d.eps_pitch) - 21.2432).abs() < 0.01, "ζmp {}", deg(d.eps_pitch));
        assert!((d.cone_m1 - 42.0179).abs() < 0.01 && (d.cone_m2 - 48.3061).abs() < 0.01);
        assert!((d.rm1 - 19.0584).abs() < 0.01);
        assert!((d.mmn - 1.6678).abs() < 0.002);
        assert!((deg(d.alpha_lim).abs() - 4.7976).abs() < 0.01, "αlim {}", deg(d.alpha_lim));
    }

    /// The wheel pitch angle is the one at which ISO 23509's limit curvature is the tooth line's.
    #[test]
    fn the_wheel_pitch_angle_meets_the_limit_curvature() {
        for e in [10.0, 25.0, 35.0] {
            let mut s = spec();
            s.offset = e;
            let d = design(&s).unwrap();
            consistent(&d);
            let own = d.cone_m2 / d.beta2.sin();
            eprintln!(
                "E {e}: δ2 {:.3}° δ1 {:.3}° β2 {:.3}° ρlim {:.3} spiral ρ {:.3}",
                d.delta2.to_degrees(),
                d.delta1.to_degrees(),
                d.beta2.to_degrees(),
                rho_lim(&d),
                own
            );
            assert!((rho_lim(&d) / own - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    #[ignore]
    fn rho_lim_against_the_wheel_pitch_angle() {
        let s = spec();
        let d0 = design(&s).unwrap();
        for k in 0..12 {
            let d2 = d0.delta2 - (2.0 * k as f64).to_radians();
            let rm2 = (2.0 * d0.re2 - d0.b2 * d2.sin()) / 2.0;
            if let Ok(d) = design_with(&s, Some((d2, rm2))) {
                eprintln!(
                    "δ2 {:.2} δ1 {:.2} β2 {:.2} ρlim {:.2} log spiral ρ {:.2}",
                    d2.to_degrees(),
                    d.delta1.to_degrees(),
                    d.beta2.to_degrees(),
                    rho_lim(&d),
                    d.cone_m2 / d.beta2.sin()
                );
            }
        }
    }

    #[test]
    fn offset_raises_the_pinion_spiral_angle_and_enlarges_it() {
        let s = spec();
        let d = design(&s).unwrap();
        consistent(&d);
        assert!((d.offset - 25.0).abs() < 1e-9);
        assert!((2.0 * d.re2 - 200.0).abs() < 1e-9);
        assert!(d.beta1 > d.beta2 && d.eps_pitch > 0.0);
        // Offset hypoid pinion cones are flatter than the bevel's: δ1 + δ2 < Σ.
        assert!(d.delta1 + d.delta2 < PI / 2.0);
        // Typical car axle numbers: ε′ several degrees, pinion enlarged 20-40%.
        let enlarge = d.rm1 * d.z2 / (d.rm2 * d.z1);
        assert!((1.1..1.6).contains(&enlarge), "{enlarge}");
        assert!((enlarge - d.beta2.cos() / d.beta1.cos()).abs() < 1e-6);
        assert!((3f64.to_radians()..25f64.to_radians()).contains(&d.eps_pitch), "{}", d.eps_pitch.to_degrees());
        assert!(d.side.abs() == 1.0);
        // The two pitch apexes are apart.
        assert!(len(sub(d.apex1, d.apex2)) > 1.0);
        // A limit pressure angle of a few degrees.
        assert!((1.0..12.0).contains(&d.alpha_lim.abs().to_degrees()), "{}", d.alpha_lim.to_degrees());
    }

    #[test]
    fn larger_offsets_grow_the_spiral_difference() {
        let mut last = 0.0;
        for e in [5.0, 15.0, 25.0, 35.0] {
            let mut s = spec();
            s.offset = e;
            let d = design(&s).unwrap();
            consistent(&d);
            assert!(d.eps_pitch > last);
            last = d.eps_pitch;
        }
    }

    #[test]
    fn other_shaft_angles_solve() {
        for sigma in [70.0, 100.0] {
            let mut s = spec();
            s.shaft_angle = sigma;
            let d = design(&s).unwrap();
            consistent(&d);
        }
    }

    #[test]
    fn an_impossible_offset_is_refused_with_the_most_that_works() {
        let mut s = spec();
        s.offset = 400.0;
        let e = crate::plan(&s).unwrap_err();
        assert!(e.starts_with("offset too large: use at most"), "{e}");
        s.teeth = 50;
        assert!(crate::plan(&s).unwrap_err().contains("more teeth"));
    }

    /// An offset that turns the ring gear's spiral angle under `LEAST_RING_SPIRAL` (or the other
    /// way round) is refused, naming the largest offset that works; that one plans.
    #[test]
    fn an_offset_that_unwinds_the_ring_gear_is_refused_with_the_most_that_works() {
        let mut s = spec();
        s.offset = 80.0;
        let e = crate::plan(&s).unwrap_err();
        assert!(e.starts_with("offset too large: use at most") && e.contains("spiral angle would be -8."), "{e}");
        let most: f64 = e["offset too large: use at most ".len()..].split(' ').next().unwrap().parse().unwrap();
        assert!((20.0..80.0).contains(&most), "{most}");
        // The offset named works (the ring gear's centre hole aside, which is sized for 25 mm).
        let t = Spec { offset: most, bore2: Default::default(), ..s.clone() };
        let d = design(&t).unwrap();
        assert!(d.beta2 >= LEAST_RING_SPIRAL.to_radians(), "{}", d.beta2.to_degrees());
        let p = crate::plan(&t).unwrap_or_else(|e| panic!("{most} mm: {e}"));
        assert!(!p.bodies.is_empty());
        // A little more is refused the same way, or by the pinion's teeth.
        assert!(crate::plan(&Spec { offset: most + 1.0, ..t.clone() }).is_err());
        // The default set and every offset up to it are untouched.
        for e in [0.0, 10.0, 25.0, 40.0] {
            assert!(design(&Spec { offset: e, ..spec() }).is_ok(), "{e}");
        }
        // A pinion spiral angle under the limit leaves no offset at all.
        let e = design(&Spec { spiral_angle: 8.0, offset: 5.0, ..spec() }).unwrap_err();
        assert!(e.contains("leaves no room for an offset"), "{e}");
        assert!(design(&Spec { spiral_angle: 8.0, offset: 0.0, ..spec() }).is_ok());
    }

    // -----------------------------------------------------------------------------------------
    // A small 3D mesh checker: the bodies as their steps describe them.

    /// How a loft runs between its sections.
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Between {
        /// Straight lines between matching points.
        Straight,
        /// A cubic through four sections' matching points, as the kernel's loft through four or
        /// more sections is smooth.
        Smooth,
        /// One cubic spline through every section's matching points (C², not-a-knot ends), as
        /// the kernel skins a loft through four or more sections.
        Spline,
    }

    /// Matching polygons on planes up the body's z.
    struct Stack {
        z: Vec<f64>,
        rings: Vec<Vec<V2>>,
        teeth: usize,
        reach: f64,
        /// [`spline_matrix`] for these sections (evenly spaced).
        m: Vec<Vec<f64>>,
    }

    /// An outline's points, the same number for every outline of the same make-up.
    fn ring(o: &Outline) -> Vec<V2> {
        let mut out = Vec::new();
        for s in &o.segs {
            match s {
                Seg::Line(a, _) => out.push(*a),
                Seg::Arc { c, a, b, ccw } => {
                    let a0 = (*a - *c).angle();
                    let mut sweep = (*b - *c).angle() - a0;
                    if *ccw {
                        while sweep <= 0.0 {
                            sweep += TAU;
                        }
                    } else {
                        while sweep >= 0.0 {
                            sweep -= TAU;
                        }
                    }
                    for i in 0..8 {
                        out.push(*c + V2::polar(c.dist(*a), a0 + sweep * f64::from(i) / 8.0));
                    }
                }
                Seg::Spline(p) => {
                    let mut pts = Vec::new();
                    crate::geom::natural_spline(p, 4, &mut pts);
                    pts.pop();
                    out.extend(pts);
                }
                Seg::Circle(c, r) => out.extend((0..64).map(|i| *c + V2::polar(*r, TAU * f64::from(i) / 64.0))),
            }
        }
        out
    }

    impl Stack {
        fn new(z: Vec<f64>, rings: Vec<Vec<V2>>, teeth: usize) -> Stack {
            assert!(rings.iter().all(|r| r.len() == rings[0].len() && r.len() % teeth == 0));
            let reach = rings.iter().flatten().map(|p| p.len()).fold(0.0, f64::max);
            let m = if z.len() >= 4 { spline_matrix(z.len()) } else { Vec::new() };
            Stack { z, rings, teeth, reach, m }
        }
        /// The sections a height lies between, weighted.
        fn weights(&self, z: f64, how: Between) -> Option<Vec<(usize, f64)>> {
            let n = self.z.len();
            if z < self.z[0] || z > self.z[n - 1] {
                return None;
            }
            let k = (0..n - 1).rfind(|k| self.z[*k] <= z).unwrap_or(0);
            if how == Between::Straight || n < 4 {
                let u = (z - self.z[k]) / (self.z[k + 1] - self.z[k]);
                return Some(vec![(k, 1.0 - u), (k + 1, u)]);
            }
            if how == Between::Spline {
                let t = (z - self.z[k]) / (self.z[k + 1] - self.z[k]);
                let a = 1.0 - t;
                let (ca, ct) = ((a * a * a - a) / 6.0, (t * t * t - t) / 6.0);
                return Some(
                    (0..n)
                        .map(|j| {
                            let lin = if j == k {
                                a
                            } else if j == k + 1 {
                                t
                            } else {
                                0.0
                            };
                            (j, lin + ca * self.m[k][j] + ct * self.m[k + 1][j])
                        })
                        .filter(|(_, w)| w.abs() > 1e-12)
                        .collect(),
                );
            }
            let first = k.saturating_sub(1).min(n - 4);
            Some(
                (first..first + 4)
                    .map(|i| {
                        let mut w = 1.0;
                        for j in first..first + 4 {
                            if j != i {
                                w *= (z - self.z[j]) / (self.z[i] - self.z[j]);
                            }
                        }
                        (i, w)
                    })
                    .collect(),
            )
        }
        fn point(&self, w: &[(usize, f64)], i: usize) -> V2 {
            let mut p = V2::ZERO;
            for (l, k) in w {
                p = p + self.rings[*l][i] * *k;
            }
            p
        }
        /// How deep (x, y, z) is inside (negative: how far outside), measured in its plane.
        fn depth(&self, q: V2, z: f64, how: Between) -> Option<f64> {
            let w = self.weights(z, how)?;
            let total = self.rings[0].len();
            let per = total / self.teeth;
            // The teeth near the point's direction.
            let (first, count) = if self.teeth > 12 {
                let a0 = self.point(&w, 0).angle();
                let step = crate::geom::wrap(self.point(&w, per).angle() - a0);
                let j = (crate::geom::wrap(q.angle() - a0) / step).round() as i64;
                let span = 5i64;
                (((j - span).rem_euclid(self.teeth as i64)) as usize * per, (2 * span as usize + 1) * per)
            } else {
                (0, total)
            };
            let r = q.len();
            let (s, c) = (q.y / r.max(1e-300), q.x / r.max(1e-300));
            let mut inside = false;
            let mut best = f64::INFINITY;
            let mut a = self.point(&w, first % total);
            for i in 1..=count {
                let b = self.point(&w, (first + i) % total);
                // In coordinates with the point on +x.
                let (ay, by) = (-a.x * s + a.y * c, -b.x * s + b.y * c);
                if (ay > 0.0) != (by > 0.0) {
                    let (ax, bx) = (a.x * c + a.y * s, b.x * c + b.y * s);
                    if ax + (bx - ax) * ay / (ay - by) > r {
                        inside = !inside;
                    }
                }
                let e = b - a;
                let l2 = e.dot(e);
                let u = if l2 > 0.0 { ((q - a).dot(e) / l2).clamp(0.0, 1.0) } else { 0.0 };
                best = best.min(q.dist(a + e * u));
                a = b;
            }
            Some(if inside { best } else { -best })
        }
    }

    struct Solid {
        frame: Frame,
        parts: Vec<Stack>,
        /// The blanks the body is intersected with ((radius, z) polygons), and for each part
        /// the first of them that trims it (those made after it).
        trims: Vec<Vec<V2>>,
        trimmed_from: Vec<usize>,
    }

    /// How deep (radius, z) is inside a polygon (negative: outside).
    fn polygon_depth(poly: &[V2], q: V2) -> f64 {
        let n = poly.len();
        let mut inside = false;
        let mut best = f64::INFINITY;
        for i in 0..n {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (b.x - a.x) * (q.y - a.y) / (b.y - a.y) {
                inside = !inside;
            }
            let e = b - a;
            let u = ((q - a).dot(e) / e.dot(e).max(1e-300)).clamp(0.0, 1.0);
            best = best.min(q.dist(a + e * u));
        }
        if inside { best } else { -best }
    }

    impl Solid {
        /// The body's added material (its lofts, tapers and prisms that aren't cuts).
        fn of(b: &Body, teeth: usize) -> Solid {
            let mut parts = Vec::new();
            let mut trims = Vec::new();
            let mut trimmed_from = Vec::new();
            for st in &b.steps {
                trimmed_from.resize(parts.len(), trims.len());
                match st {
                    Step::Revolve { profile, op: Op::Intersect } => trims.push(profile.clone()),
                    // Point i of one section goes to point i of the next, as the kernel pairs them
                    // when it lines them up tooth to tooth (which a test checks).
                    Step::Loft { sections, op } if *op != Op::Cut => {
                        parts.push(Stack::new(sections.iter().map(|s| s.1).collect(), sections.iter().map(|s| ring(&s.0)).collect(), teeth))
                    }
                    Step::Taper { outline, z0, height, twist, scale, op } if *op != Op::Cut => {
                        let levels = 13;
                        let at = |k: usize| k as f64 / (levels - 1) as f64;
                        parts.push(Stack::new(
                            (0..levels).map(|k| z0 + height * at(k)).collect(),
                            (0..levels).map(|k| ring(&outline.scaled(1.0 + (scale - 1.0) * at(k)).rotated(twist * at(k)))).collect(),
                            teeth,
                        ));
                    }
                    Step::Prism { region, z0, height, twist, op } if *op != Op::Cut && *twist == 0.0 => {
                        let r = ring(&region.outer[0]);
                        parts.push(Stack::new(vec![*z0, z0 + height], vec![r.clone(), r], 1));
                    }
                    _ => {}
                }
            }
            trimmed_from.resize(parts.len(), trims.len());
            Solid { frame: b.frame, parts, trims, trimmed_from }
        }
        /// How deep a point is inside the blanks made after part `k` (infinite: none).
        fn trim(&self, k: usize, q: V2, z: f64) -> f64 {
            self.trims[self.trimmed_from[k]..].iter().map(|t| polygon_depth(t, V2::new(q.len(), z))).fold(f64::INFINITY, f64::min)
        }
        /// A world point in the body's own coordinates when the body has turned by `turn`.
        fn local(&self, x: V3, turn: f64) -> (V2, f64) {
            let d = sub(x, self.frame.origin);
            let q = V2::new(dot(d, self.frame.x), dot(d, self.frame.y()));
            (q.rot(-turn), dot(d, self.frame.z))
        }
        fn world(&self, q: V2, z: f64, turn: f64) -> V3 {
            let q = q.rot(turn);
            self.frame.world([q.x, q.y, z])
        }
        /// How deep a world point is inside the body, in the plane of its section (negative:
        /// outside); `None` when it is off the ends or well clear.
        fn flat(&self, q: V2, z: f64, how: Between) -> Option<f64> {
            let mut best: Option<f64> = None;
            for (k, p) in self.parts.iter().enumerate() {
                if q.len() > p.reach + 1.0 {
                    continue;
                }
                if let Some(d) = p.depth(q, z, how) {
                    // Not deeper than the nearer flat end, nor than the blanks trimming it.
                    let d = d.min(z - p.z[0]).min(p.z[p.z.len() - 1] - z).min(self.trim(k, q, z));
                    best = Some(best.map_or(d, |b: f64| b.max(d)));
                }
            }
            best
        }
        /// The same, corrected for the surface's slope along the axis (a true distance for a
        /// flat surface).
        fn deep(&self, q: V2, z: f64, how: Between) -> Option<f64> {
            let d = self.flat(q, z, how)?;
            let h = 0.02;
            let slope = match (self.flat(q, z - h, how), self.flat(q, z + h, how)) {
                (Some(a), Some(b)) => (b - a) / (2.0 * h),
                (Some(a), None) => (d - a) / h,
                (None, Some(b)) => (b - d) / h,
                _ => 0.0,
            };
            Some(d / (1.0 + slope * slope).sqrt())
        }
        /// Points all over the toothed part's surface, in the body's coordinates.
        fn skin(&self, levels: usize, how: Between) -> Vec<(V2, f64)> {
            let p = &self.parts[0];
            let (z0, z1) = (p.z[0], p.z[p.z.len() - 1]);
            let mut out = Vec::new();
            for k in 0..=levels {
                let z = z0 + (z1 - z0) * k as f64 / levels as f64;
                let w = p.weights(z, how).unwrap();
                // (Where the blank leaves the loft's surface.)
                out.extend((0..p.rings[0].len()).map(|i| (p.point(&w, i), z)).filter(|(q, z)| self.trim(0, *q, *z) >= -1e-9));
            }
            out
        }
    }

    /// The deepest any sampled surface point of one body is inside the other, and the smallest
    /// gap between them, with the pinion turned by `t1` and the gear by `t2`.
    fn probe(pinion: &Solid, gear: &Solid, skins: (&[(V2, f64)], &[(V2, f64)]), t1: f64, t2: f64, how: Between) -> (f64, f64) {
        let (mut pen, mut gap) = (0.0f64, f64::INFINITY);
        let mut one = |from: &Solid, skin: &[(V2, f64)], ta: f64, to: &Solid, tb: f64| {
            for (q, z) in skin {
                let (lq, lz) = to.local(from.world(*q, *z, ta), tb);
                let Some(d) = to.flat(lq, lz, how) else { continue };
                if d > pen || (d <= 0.0 && -d < gap) {
                    let d = to.deep(lq, lz, how).unwrap_or(d);
                    if d > 0.0 {
                        pen = pen.max(d);
                    } else {
                        gap = gap.min(-d);
                    }
                }
            }
        };
        one(pinion, skins.0, t1, gear, t2);
        one(gear, skins.1, t2, pinion, t1);
        (pen, gap)
    }

    struct Pair {
        pinion: Solid,
        gear: Solid,
        ratio: f64,
        pitch: f64,
        module: f64,
    }

    fn pair(s: &Spec) -> Pair {
        let p = crate::plan(s).unwrap_or_else(|e| panic!("{e}"));
        Pair {
            pinion: Solid::of(&p.bodies[0], s.teeth as usize),
            gear: Solid::of(&p.bodies[1], s.teeth2 as usize),
            ratio: f64::from(s.teeth) / f64::from(s.teeth2),
            pitch: TAU / f64::from(s.teeth),
            module: s.module,
        }
    }

    impl Pair {
        /// The worst penetration and the largest of the smallest gaps over `steps` positions
        /// through `pitches` tooth pitches, the gear `slip` (of a pinion pitch) out of step.
        fn run(&self, how: Between, pitches: f64, steps: usize, slip: f64) -> (f64, f64) {
            let skins = (self.pinion.skin(28, how), self.gear.skin(28, how));
            let (mut pen, mut gap) = (0.0f64, 0.0f64);
            for k in 0..steps {
                let t = self.pitch * pitches * k as f64 / steps as f64;
                let (p, g) = probe(&self.pinion, &self.gear, (&skins.0, &skins.1), t, -(t + slip * self.pitch) * self.ratio, how);
                pen = pen.max(p);
                gap = gap.max(g);
            }
            (pen, gap)
        }
    }

    /// Prints the set's facts, the pinion's sections and where the deepest overlap is (run with
    /// `--ignored --nocapture`).
    #[test]
    #[ignore]
    fn debug_dump() {
        let s = spec();
        let t0 = std::time::Instant::now();
        let set = work_out(&s).unwrap();
        eprintln!("work_out {:.2} ms", t0.elapsed().as_secs_f64() * 1e3);
        let p = crate::plan(&s).unwrap();
        for f in &p.facts {
            eprintln!("{:45} {:.4}", f.label, f.value);
        }
        eprintln!("warnings {:?}", p.warnings);
        let f = &set.form;
        eprintln!(
            "form z_lo {:.3} z_hi {:.3} r_out {:.3} hap {:.3} half {:.3} tan {:.3} {:.3}",
            f.z_lo, f.z_hi, f.r_out, f.hap, f.half, f.tan_lo, f.tan_hi
        );
        for (j, t) in set.teeth.iter().enumerate() {
            eprintln!("pinion section {j} zeta {:.2} pointed {} undercut {}", set.zeta[j], t.pointed, t.undercut);
            for i in 0..t.r.len() {
                eprintln!("   r {:8.3} lo {:8.4} hi {:8.4} thick {:7.3}", t.r[i], t.lo[i], t.hi[i], t.r[i] * (t.hi[i] - t.lo[i]));
            }
        }
        // How far the gear's loft strays from its true flanks between its sections.
        let gear = Solid::of(&p.bodies[1], s.teeth2 as usize);
        let pinion = Solid::of(&p.bodies[0], s.teeth as usize);
        let back = f.z_lo - 0.06 * 2.0 * set.d.re2;
        for how in [Between::Straight, Between::Smooth] {
            let (mut worst, mut wz, mut wt) = (0.0f64, 0.0, 0.0);
            let st = &gear.parts[0];
            for k in 0..60 {
                let zl = st.z[0] + (st.z[st.z.len() - 1] - st.z[0]) * (k as f64 + 0.5) / 60.0;
                let z = zl + back;
                let w = st.weights(zl, how).unwrap();
                let per = st.rings[0].len() / s.teeth2 as usize;
                let side = (GEAR_SIDE - 1) * 4;
                for i in (0..side).chain(side + 4..2 * side + 4) {
                    let q = st.point(&w, i);
                    let Some((t, mu)) = f.coords(q.len(), z) else { continue };
                    if t > f.tip(z) - 0.5 || t < -f.hf + 0.3 {
                        continue;
                    }
                    let d = wrap_period(q.angle() - f.centre(mu), f.pitch);
                    let (wl, wh) = f.widths(t);
                    let d = wrap_period(d - 0.5 * (wh - wl), f.pitch);
                    let e = (d.abs() - 0.5 * (wh + wl)).abs() * mu * f.lat;
                    if e > worst {
                        (worst, wz, wt) = (e, z, t);
                    }
                }
                let _ = per;
            }
            eprintln!("gear loft {how:?}: flanks stray {worst:.5} mm from the true ones (z {wz:.2}, t {wt:.2})");
        }
        // Where the deepest penetration is.
        let how = Between::Smooth;
        let skins = (pinion.skin(28, how), gear.skin(28, how));
        let ratio = 11.0 / 41.0;
        let mut worst = (0.0f64, String::new());
        for k in 0..16 {
            let t = TAU / 11.0 * k as f64 / 16.0;
            for (q, z) in &skins.0 {
                let (lq, lz) = gear.local(pinion.world(*q, *z, t), -t * ratio);
                if let Some(dd) = gear.flat(lq, lz, how)
                    && dd > worst.0
                {
                    let dd = gear.deep(lq, lz, how).unwrap();
                    if dd > worst.0 {
                        let c = f.coords(lq.len(), lz + back);
                        worst = (
                            dd,
                            format!(
                                "pinion point r {:.2} zeta {:.2} in gear at rho {:.2} z {:.2} (t, mu) {:?} tip {:.2} step {k}",
                                q.len(),
                                z,
                                lq.len(),
                                lz + back,
                                c,
                                f.tip(lz + back)
                            ),
                        );
                    }
                }
            }
            for (q, z) in &skins.1 {
                let (lq, lz) = pinion.local(gear.world(*q, *z, -t * ratio), t);
                if let Some(dd) = pinion.flat(lq, lz, how)
                    && dd > worst.0
                {
                    let dd = pinion.deep(lq, lz, how).unwrap();
                    if dd > worst.0 {
                        let c = f.coords(q.len(), z + back);
                        worst = (
                            dd,
                            format!(
                                "gear point rho {:.2} z {:.2} (t, mu) {:?} tip {:.2} in pinion at r {:.2} zeta {:.2} step {k}",
                                q.len(),
                                z + back,
                                c,
                                f.tip(z + back),
                                lq.len(),
                                lz
                            ),
                        );
                    }
                }
            }
        }
        eprintln!("deepest {:.5}: {}", worst.0, worst.1);
    }

    #[test]
    fn the_default_set_meshes_through_two_pitches() {
        for beta in [45.0, 50.0] {
            let mut s = spec();
            s.spiral_angle = beta;
            let set = pair(&s);
            let m = set.module;
            let t0 = std::time::Instant::now();
            let (pen, gap) = set.run(Between::Spline, 2.0, 32, 0.0);
            eprintln!(
                "hypoid β1 {beta}°: smooth loft: penetration {pen:.5} mm ({:.2e} module), largest least gap {gap:.5} mm, {:.1} s",
                pen / m,
                t0.elapsed().as_secs_f64()
            );
            assert!(pen <= 3e-3 * m, "penetration {pen}");
            assert!(gap <= 6e-3 * m, "gap {gap}");
            let (pen, gap) = set.run(Between::Straight, 2.0, 16, 0.0);
            eprintln!("hypoid β1 {beta}°: straight loft: penetration {pen:.5} mm ({:.2e} module), largest least gap {gap:.5} mm", pen / m);
            // Half a pitch out of step, the teeth run into each other.
            let (pen, _) = set.run(Between::Spline, 1.0, 6, 0.5);
            eprintln!("hypoid β1 {beta}°: half a pitch out of step: penetration {pen:.3} mm");
            assert!(pen > 0.1 * m, "half a pitch out of step only overlaps {pen}");
        }
    }

    /// The kernel's loft start alignment (`loft_numbered_loops`) searched over every restart, as
    /// the kernel does: each section restarted where its corners line up best, by the sum of dot
    /// products of directions from the centroid, with the section before (the first with the
    /// last, the "lead" of equal counts). Returns each section's shift.
    fn kernel_shifts(sections: &[(Outline, f64)]) -> Vec<usize> {
        let rings: Vec<Vec<V2>> = sections.iter().map(|(o, _)| corners(o)).collect();
        let m = rings[0].len();
        let mut dirs = rings[rings.len() - 1].clone();
        let mut out = Vec::new();
        for r in &rings {
            let fit = |k: usize| -> f64 { dirs.iter().enumerate().map(|(i, d)| r[(i + k) % m].dot(*d)).sum() };
            let k = (0..m).max_by(|a, b| fit(*a).total_cmp(&fit(*b))).unwrap();
            out.push(k);
            dirs = (0..m).map(|i| r[(i + k) % m]).collect();
        }
        out
    }

    /// [`kernel_shifts`] in the kernel's canonical order (`loft_reversed`: the sections taken
    /// last to first when the first-to-last step's largest world component is negative), each
    /// section's shift in the order given.
    fn loft_shifts(frame: &Frame, sections: &[(Outline, f64)]) -> Vec<usize> {
        let step = mul(frame.z, sections[sections.len() - 1].1 - sections[0].1);
        let big = step.into_iter().max_by(|a, b| a.abs().total_cmp(&b.abs())).unwrap_or(0.0);
        if big < 0.0 {
            let rev: Vec<(Outline, f64)> = sections.iter().rev().cloned().collect();
            let mut k = kernel_shifts(&rev);
            k.reverse();
            k
        } else {
            kernel_shifts(sections)
        }
    }

    /// An ASCII DXF of an outline (lines, arcs, fit point splines), as `sketch.insert_dxf` reads it.
    fn dxf(o: &Outline) -> String {
        let mut t = String::from("0\nSECTION\n2\nENTITIES\n");
        let f = |v: f64| format!("{v:.12}");
        for s in &o.segs {
            match s {
                Seg::Line(a, b) => t += &format!("0\nLINE\n10\n{}\n20\n{}\n11\n{}\n21\n{}\n", f(a.x), f(a.y), f(b.x), f(b.y)),
                Seg::Arc { c, a, b, ccw } => {
                    let (from, to) = if *ccw { (*a, *b) } else { (*b, *a) };
                    t += &format!(
                        "0\nARC\n10\n{}\n20\n{}\n40\n{}\n50\n{}\n51\n{}\n",
                        f(c.x),
                        f(c.y),
                        f(c.dist(from)),
                        f((from - *c).angle().to_degrees()),
                        f((to - *c).angle().to_degrees())
                    );
                }
                Seg::Spline(p) => {
                    t += "0\nSPLINE\n71\n3\n";
                    for q in p {
                        t += &format!("11\n{}\n21\n{}\n", f(q.x), f(q.y));
                    }
                }
                Seg::Circle(c, r) => t += &format!("0\nCIRCLE\n10\n{}\n20\n{}\n40\n{}\n", f(c.x), f(c.y), f(*r)),
            }
        }
        t + "0\nENDSEC\n0\nEOF\n"
    }

    /// A CLI script that builds a plan's bodies as `gear.create` does (sketches through
    /// `sketch.insert_dxf`, lofts and joined prisms; cuts left out), with the pinion turned by
    /// `turns[k]` pitches and the gear by the matching angle, an interference check after each.
    fn kernel_script(p: &Plan, s: &Spec, turns: &[f64]) -> String {
        let mut cmds: Vec<String> = Vec::new();
        let mut sketch = 0usize;
        let mut sk = |cmds: &mut Vec<String>, origin: V3, x: V3, y: V3, o: &Outline| -> usize {
            sketch += 1;
            cmds.push(format!(
                r#"{{"command":"sketch.create","params":{{"plane":{{"origin":{origin:?},"x_dir":{x:?},"y_dir":{y:?}}},"project_edges":false}}}}"#
            ));
            cmds.push(format!(r#"{{"command":"sketch.insert_dxf","params":{{"text":{}}}}}"#, serde_json_escape(&dxf(o))));
            cmds.push(r#"{"command":"sketch.finish","params":{}}"#.into());
            sketch
        };
        let mut features: Vec<String> = Vec::new();
        for b in &p.bodies {
            let f = b.frame;
            let (x, y) = (f.x, f.y());
            for (i, st) in b.steps.iter().enumerate() {
                match st {
                    Step::Loft { sections, op } => {
                        let ids: Vec<String> =
                            sections.iter().map(|(o, h)| format!(r#"{{"sketch":{}}}"#, sk(&mut cmds, f.world([0.0, 0.0, *h]), x, y, o))).collect();
                        assert!(*op == Op::New && i == 0);
                        features.push(format!(
                            r#"{{"command":"solid.loft","params":{{"sections":[{}],"operation":"new","body_name":"{}"}}}}"#,
                            ids.join(","),
                            b.name
                        ));
                    }
                    Step::Prism { region, z0, height, twist, op: Op::Join } if *twist == 0.0 && region.holes.is_empty() => {
                        let id = sk(&mut cmds, f.world([0.0, 0.0, *z0]), x, y, &region.outer[0]);
                        features.push(format!(
                            r#"{{"command":"solid.extrude","params":{{"sketch":{id},"distance":{height},"operation":"join","targets":["{}"]}}}}"#,
                            b.name
                        ));
                    }
                    // A trim, as `gear.create` makes it: the profile on the plane through the
                    // axis and x, turned about the sketch's y (the body's z).
                    Step::Revolve { profile, op: Op::Intersect } => {
                        let n = profile.len();
                        let o = Outline { segs: (0..n).map(|k| Seg::Line(profile[k], profile[(k + 1) % n])).collect() };
                        let id = sk(&mut cmds, f.origin, x, f.z, &o);
                        features.push(format!(
                            r#"{{"command":"solid.revolve","params":{{"sketch":{id},"axis":"y","operation":"intersect","targets":["{}"]}}}}"#,
                            b.name
                        ));
                    }
                    _ => {}
                }
            }
        }
        cmds.extend(features);
        let export = std::env::var("HYPOID_EXPORT").ok();
        let quality = std::env::var("HYPOID_QUALITY").unwrap_or_else(|_| "fine".into());
        let save = |cmds: &mut Vec<String>, tag: &str| {
            if let Some(dir) = &export {
                for (b, file) in p.bodies.iter().zip(["pinion", "gear"]) {
                    let path = std::path::Path::new(dir).join(format!("{file}{tag}.stl"));
                    cmds.push(format!(
                        r#"{{"command":"file.export","params":{{"path":{},"bodies":["{}"],"quality":"{quality}"}}}}"#,
                        serde_json_escape(&path.to_string_lossy()),
                        b.name
                    ));
                }
            } else {
                cmds.push(r#"{"command":"inspect.interference","params":{"budget_s":240}}"#.into());
            }
        };
        let turn = |cmds: &mut Vec<String>, b: &Body, a: f64| {
            cmds.push(format!(
                r#"{{"command":"solid.move","params":{{"bodies":["{}"],"axis":{:?},"origin":{:?},"angle":"{} deg"}}}}"#,
                b.name, b.frame.z, b.frame.origin, a
            ));
        };
        let ratio = f64::from(s.teeth) / f64::from(s.teeth2);
        let pitch = 360.0 / f64::from(s.teeth);
        let mut done = 0.0;
        for (k, t) in turns.iter().enumerate() {
            let step = (t - done) * pitch;
            done = *t;
            if step != 0.0 {
                for (b, a) in p.bodies.iter().zip([step, -step * ratio]) {
                    turn(&mut cmds, b, a);
                }
            }
            save(&mut cmds, &format!("_{k}"));
        }
        // Out of step: the pinion alone turned on by half a pitch.
        if std::env::var("HYPOID_SLIP").is_ok() {
            turn(&mut cmds, &p.bodies[0], 0.5 * pitch);
            save(&mut cmds, "_slip");
        }
        format!("[\n{}\n]\n", cmds.join(",\n"))
    }

    /// A body's tessellation, for point-in-solid and distance queries.
    struct Mesh {
        tris: Vec<[V3; 3]>,
        lo: V3,
        cell: f64,
        dims: [usize; 3],
        /// Triangles by 3D cell, and by (y, z) column for rays along +x.
        cells: Vec<Vec<u32>>,
        columns: Vec<Vec<u32>>,
    }

    impl Mesh {
        fn new(tris: Vec<[V3; 3]>, cell: f64) -> Mesh {
            let mut lo = [f64::MAX; 3];
            let mut hi = [f64::MIN; 3];
            for t in &tris {
                for v in t {
                    for i in 0..3 {
                        lo[i] = lo[i].min(v[i]);
                        hi[i] = hi[i].max(v[i]);
                    }
                }
            }
            let dims = [0, 1, 2].map(|i| (((hi[i] - lo[i]) / cell).ceil() as usize).max(1) + 1);
            let mut cells = vec![Vec::new(); dims[0] * dims[1] * dims[2]];
            let mut columns = vec![Vec::new(); dims[1] * dims[2]];
            for (k, t) in tris.iter().enumerate() {
                let c = |i: usize, f: fn(f64, f64) -> f64| ((f(f(t[0][i], t[1][i]), t[2][i]) - lo[i]) / cell) as usize;
                let (a, b) = ([0, 1, 2].map(|i| c(i, f64::min)), [0, 1, 2].map(|i| c(i, f64::max)));
                for z in a[2]..=b[2] {
                    for y in a[1]..=b[1] {
                        columns[y + dims[1] * z].push(k as u32);
                        for x in a[0]..=b[0] {
                            cells[x + dims[0] * (y + dims[1] * z)].push(k as u32);
                        }
                    }
                }
            }
            Mesh { tris, lo, cell, dims, cells, columns }
        }
        fn index(&self, p: V3) -> Option<[usize; 3]> {
            let mut out = [0; 3];
            for i in 0..3 {
                let c = (p[i] - self.lo[i]) / self.cell;
                if !(0.0..self.dims[i] as f64).contains(&c) {
                    return None;
                }
                out[i] = c as usize;
            }
            Some(out)
        }
        /// Whether `p` is inside (a ray along +x crosses the surface an odd number of times).
        fn inside(&self, p: V3) -> bool {
            let Some(c) = self.index(p) else { return false };
            let (py, pz) = (p[1] + 1.234_567e-7, p[2] + 2.345_678e-7);
            let mut n = 0;
            for &k in &self.columns[c[1] + self.dims[1] * c[2]] {
                let t = &self.tris[k as usize];
                // Barycentric in (y, z).
                let (ay, az, by, bz, cy, cz) = (t[0][1], t[0][2], t[1][1], t[1][2], t[2][1], t[2][2]);
                let d = (by - ay) * (cz - az) - (cy - ay) * (bz - az);
                if d.abs() < 1e-300 {
                    continue;
                }
                let u = ((py - ay) * (cz - az) - (cy - ay) * (pz - az)) / d;
                let v = ((by - ay) * (pz - az) - (py - ay) * (bz - az)) / d;
                if u < 0.0 || v < 0.0 || u + v > 1.0 {
                    continue;
                }
                let x = t[0][0] + u * (t[1][0] - t[0][0]) + v * (t[2][0] - t[0][0]);
                if x > p[0] {
                    n += 1;
                }
            }
            n % 2 == 1
        }
        /// The distance from `p` to the surface, if within one cell.
        fn distance(&self, p: V3) -> Option<f64> {
            let c = self.index(p)?;
            let mut best = f64::INFINITY;
            for z in c[2].saturating_sub(1)..(c[2] + 2).min(self.dims[2]) {
                for y in c[1].saturating_sub(1)..(c[1] + 2).min(self.dims[1]) {
                    for x in c[0].saturating_sub(1)..(c[0] + 2).min(self.dims[0]) {
                        for &k in &self.cells[x + self.dims[0] * (y + self.dims[1] * z)] {
                            best = best.min(point_triangle(p, &self.tris[k as usize]));
                        }
                    }
                }
            }
            (best <= self.cell).then_some(best)
        }
    }

    /// The distance from a point to a triangle.
    fn point_triangle(p: V3, t: &[V3; 3]) -> f64 {
        let (a, b, c) = (t[0], t[1], t[2]);
        let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
        let (d1, d2) = (dot(ab, ap), dot(ac, ap));
        if d1 <= 0.0 && d2 <= 0.0 {
            return len(ap);
        }
        let bp = sub(p, b);
        let (d3, d4) = (dot(ab, bp), dot(ac, bp));
        if d3 >= 0.0 && d4 <= d3 {
            return len(bp);
        }
        let vc = d1 * d4 - d3 * d2;
        if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
            let v = d1 / (d1 - d3);
            return len(sub(p, add(a, mul(ab, v))));
        }
        let cp = sub(p, c);
        let (d5, d6) = (dot(ab, cp), dot(ac, cp));
        if d6 >= 0.0 && d5 <= d6 {
            return len(cp);
        }
        let vb = d5 * d2 - d1 * d6;
        if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
            let w = d2 / (d2 - d6);
            return len(sub(p, add(a, mul(ac, w))));
        }
        let va = d3 * d6 - d5 * d4;
        if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
            let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
            return len(sub(p, add(b, mul(sub(c, b), w))));
        }
        let n = cross(ab, ac);
        dot(ap, n).abs() / len(n)
    }

    /// The kernel's bodies against each other, as their tessellations (`HYPOID_MESH`: a folder
    /// of `pinion_K.stl`, `gear_K.stl` for each mesh position K, and `_slip` for one out of
    /// step): the deepest any vertex of one is inside the other, and the least gap.
    #[test]
    #[ignore]
    fn kernel_mesh_interference() {
        let Ok(dir) = std::env::var("HYPOID_MESH") else { return };
        let mut tags: Vec<String> =
            (0..64).map(|k| format!("_{k}")).filter(|t| std::path::Path::new(&dir).join(format!("pinion{t}.stl")).exists()).collect();
        if std::path::Path::new(&dir).join("pinion_slip.stl").exists() {
            tags.push("_slip".into());
        }
        for tag in tags {
            let load = |n: &str| stl_tris(&std::path::Path::new(&dir).join(format!("{n}{tag}.stl")));
            let (pt, gt) = (load("pinion"), load("gear"));
            let (pm, gm) = (Mesh::new(pt.clone(), 0.6), Mesh::new(gt.clone(), 0.6));
            let verts = |tris: &[[V3; 3]]| {
                let mut v: Vec<V3> = tris.iter().flatten().copied().collect();
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                v.dedup();
                v
            };
            let (pv, gv) = (verts(&pt), verts(&gt));
            let mut pen = (0.0f64, [0.0; 3], "");
            let mut gap = f64::INFINITY;
            let mut contact = 0usize;
            for (vs, other, name) in [(&pv, &gm, "pinion in gear"), (&gv, &pm, "gear in pinion")] {
                for v in vs.iter() {
                    let Some(d) = other.distance(*v) else {
                        if other.inside(*v) {
                            pen = (f64::INFINITY, *v, name);
                        }
                        continue;
                    };
                    if other.inside(*v) {
                        if d > pen.0 {
                            pen = (d, *v, name);
                        }
                    } else {
                        gap = gap.min(d);
                        if d < 0.02 {
                            contact += 1;
                        }
                    }
                }
            }
            eprintln!(
                "position{tag}: deepest {:.5} mm ({} at {:?}), least gap {:.5} mm, {contact} vertices within 0.02 mm",
                pen.0, pen.2, pen.1, gap
            );
        }
    }

    fn serde_json_escape(t: &str) -> String {
        let mut o = String::from("\"");
        for c in t.chars() {
            match c {
                '"' => o += "\\\"",
                '\\' => o += "\\\\",
                '\n' => o += "\\n",
                c => o.push(c),
            }
        }
        o + "\""
    }

    /// Writes [`kernel_script`] for the set `HYPOID_SPEC` (`z1,z2,offset,beta,left,sigma`, ring
    /// gear Ø200) to `HYPOID_SCRIPT`, the pinion at `HYPOID_TURNS` (pitches, comma separated).
    #[test]
    #[ignore]
    fn write_kernel_script() {
        let Ok(out) = std::env::var("HYPOID_SCRIPT") else { return };
        let mut s = spec();
        if let Ok(v) = std::env::var("HYPOID_SPEC") {
            let f: Vec<&str> = v.split(',').collect();
            (s.teeth, s.teeth2, s.offset, s.spiral_angle, s.left_hand) =
                (f[0].parse().unwrap(), f[1].parse().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap(), f[4] == "1");
            if let Some(g) = f.get(5) {
                s.shaft_angle = g.parse().unwrap();
            }
            if let Some(g) = f.get(6) {
                s.backlash = g.parse().unwrap();
            }
            s.module = 200.0 / f64::from(s.teeth2);
        }
        let turns: Vec<f64> = std::env::var("HYPOID_TURNS").map(|v| v.split(',').map(|x| x.parse().unwrap()).collect()).unwrap_or_else(|_| vec![0.0]);
        let mut p = crate::plan(&s).unwrap();
        // Every k-th section of body `b` only (`b:k`), to try the kernel on fewer.
        if let Ok(v) = std::env::var("HYPOID_DECIMATE") {
            let f: Vec<usize> = v.split(':').map(|x| x.parse().unwrap()).collect();
            if let Step::Loft { sections, .. } = &mut p.bodies[f[0]].steps[0] {
                let n = sections.len();
                *sections = sections.iter().enumerate().filter(|(i, _)| i % f[1] == 0 || *i == n - 1).map(|(_, s)| s.clone()).collect();
            }
        }
        std::fs::write(&out, kernel_script(&p, &s, &turns)).unwrap();
        for f in &p.facts {
            if f.label.contains("loft") {
                eprintln!("{} {}", f.label, f.value);
            }
        }
    }

    /// The window's 3D check on the default set, and where its deepest overlap is (run with
    /// `--ignored --nocapture`).
    #[test]
    #[ignore]
    fn mesh3d_dump() {
        let s = spec();
        let p = crate::plan(&s).unwrap();
        let set = work_out(&s).unwrap();
        let back = set.form.z_lo - 0.06 * 2.0 * set.d.re2;
        let m = &p.meshes3d[0];
        let untrim = |b: &Body| Body { steps: b.steps.iter().filter(|st| !matches!(st, Step::Revolve { .. })).cloned().collect(), ..b.clone() };
        for (label, pb, gb) in [
            ("both trimmed", p.bodies[0].clone(), p.bodies[1].clone()),
            ("pinion untrimmed", untrim(&p.bodies[0]), p.bodies[1].clone()),
            ("gear untrimmed", p.bodies[0].clone(), untrim(&p.bodies[1])),
        ] {
            let (h, hz) = (s.module / 10.0, s.module / 3.0);
            let mut pair = crate::mesh3d::Pair::for_run(&pb, &gb, h, hz, m.ratio, -0.5 * m.period, 0.5 * m.period);
            let r = pair.run(m.ratio, 0.0, -0.5 * m.period, 0.5 * m.period, crate::mesh3d::CHECK_STEPS);
            let q = r.place;
            eprintln!("{label}: penetration {:.4} at ring (t, mu) {:?}", r.penetration, set.form.coords(q[0].hypot(q[1]), q[2] + back));
        }
        for (h, hz) in [(s.module / 10.0, s.module / 3.0), (s.module / 20.0, s.module / 8.0)] {
            let mut pair = crate::mesh3d::Pair::for_run(&p.bodies[0], &p.bodies[1], h, hz, m.ratio, -0.5 * m.period, 0.5 * m.period);
            let r = pair.run(m.ratio, 0.0, -0.5 * m.period, 0.5 * m.period, crate::mesh3d::CHECK_STEPS);
            let q = r.place;
            let (rho, z) = (q[0].hypot(q[1]), q[2] + back);
            eprintln!(
                "h {h:.3}: {r:?}
  deepest at ring (t, mu) {:?}",
                set.form.coords(rho, z)
            );
        }
    }

    /// Where a big set's planning time goes (run with `--ignored --nocapture`).
    #[test]
    #[ignore]
    fn timing_dump() {
        let mut s = spec();
        (s.teeth, s.teeth2, s.face_width) = (25, 100, 20.0);
        for _ in 0..3 {
            let t = std::time::Instant::now();
            let d = design(&s).unwrap();
            let t1 = t.elapsed().as_secs_f64() * 1e3;
            let set = work_out_design(&s, d).unwrap();
            let t2 = t.elapsed().as_secs_f64() * 1e3;
            let _ = crate::plan(&s).unwrap();
            let t3 = t.elapsed().as_secs_f64() * 1e3;
            eprintln!(
                "design {t1:.1} ms, work_out {:.1} ms, plan {:.1} ms; pinion loft {}, gear loft {}",
                t2 - t1,
                t3 - t2,
                set.loft_z.len(),
                set.gear_n
            );
        }
    }

    #[test]
    #[ignore]
    fn print_defaults_dump() {
        let mut s = spec();
        s.apply_print_defaults(0.4, 0.2);
        let d = design(&s).unwrap();
        let form = Form::new(&s, &d).unwrap();
        let _ = form;
        match work_out(&s) {
            Ok(set) => eprintln!("ok: {} loft sections", set.loft_z.len()),
            Err(e) => {
                eprintln!("{e}");
                // Which counts slip, which can't be drawn.
                let mut t = s.clone();
                t.rack = crate::involute::BasicRack::default();
                eprintln!("with the standard rack: {:?}", work_out(&t).map(|x| x.loft_z.len()));
            }
        }
    }

    #[test]
    #[ignore]
    fn shaft_angles_dump() {
        for sigma in [60.0, 70.0, 75.0, 90.0, 105.0, 120.0] {
            let mut s = spec();
            s.shaft_angle = sigma;
            match crate::plan(&s) {
                Ok(p) => {
                    let f = |l: &str| p.facts.iter().find(|f| f.label.starts_with(l)).map(|f| f.value).unwrap_or(f64::NAN);
                    let a = p.bodies[0].frame.z;
                    let angle = dot(a, [0.0, 0.0, 1.0]).clamp(-1.0, 1.0).acos().to_degrees();
                    eprintln!(
                        "Σ {sigma}: δ1 {:.2} δ2 {:.2} β2 {:.2} loft {} pinion axis to the gear's {:.2}° warnings {:?}",
                        f("pinion pitch cone"),
                        f("gear pitch cone"),
                        f("gear mean spiral"),
                        f("pinion loft"),
                        angle,
                        p.warnings
                    );
                }
                Err(e) => eprintln!("Σ {sigma}: {e}"),
            }
        }
    }

    /// The kernel lines up every loft's sections tooth to tooth (searched as the kernel does,
    /// over every restart, in its own order): the walls between sections run along the flanks.
    #[test]
    fn the_kernel_lines_the_loft_sections_up_tooth_to_tooth() {
        for (z1, z2, e, beta, sigma) in [
            (11, 41, 25.0, 50.0, 90.0),
            (11, 41, 25.0, 45.0, 90.0),
            (10, 43, 35.0, 50.0, 90.0),
            (13, 39, 12.0, 40.0, 90.0),
            (11, 41, 25.0, 50.0, 70.0),
        ] {
            let mut s = spec();
            (s.teeth, s.teeth2, s.offset, s.spiral_angle, s.shaft_angle) = (z1, z2, e, beta, sigma);
            s.module = 200.0 / f64::from(z2);
            for left in [true, false] {
                s.left_hand = left;
                let p = crate::plan(&s).unwrap_or_else(|e| panic!("{z1}:{z2} Σ {sigma}: {e}"));
                for (b, teeth) in p.bodies.iter().zip([z1, z2]) {
                    let Step::Loft { sections, .. } = &b.steps[0] else { panic!() };
                    let k = loft_shifts(&b.frame, sections);
                    let per = corners(&sections[0].0).len() / teeth as usize;
                    eprintln!("{}: Σ {sigma} {} sections, {per} corners a tooth, restarts {:?}", b.name, sections.len(), k);
                    assert!(
                        k.windows(2).all(|w| (w[1] + per * teeth as usize - w[0]).is_multiple_of(per)),
                        "{}: the kernel would slip a section: {k:?}",
                        b.name
                    );
                }
            }
        }
    }

    /// The triangles of a binary STL.
    fn stl_tris(path: &std::path::Path) -> Vec<[V3; 3]> {
        let b = std::fs::read(path).unwrap();
        let n = u32::from_le_bytes([b[80], b[81], b[82], b[83]]) as usize;
        assert_eq!(b.len(), 84 + 50 * n, "a binary STL");
        (0..n)
            .map(|k| {
                let o = 84 + 50 * k + 12;
                let p = |vi: usize| {
                    let f = |i: usize| {
                        let a = o + 12 * vi + 4 * i;
                        f64::from(f32::from_le_bytes([b[a], b[a + 1], b[a + 2], b[a + 3]]))
                    };
                    [f(0), f(1), f(2)]
                };
                [p(0), p(1), p(2)]
            })
            .collect()
    }

    /// The vertices of an STL (binary or ASCII), deduplicated.
    fn stl_points(path: &std::path::Path) -> Vec<V3> {
        let b = std::fs::read(path).unwrap();
        let mut out = Vec::new();
        let n = u32::from_le_bytes([b[80], b[81], b[82], b[83]]) as usize;
        if b.len() == 84 + 50 * n {
            for k in 0..n {
                let o = 84 + 50 * k + 12;
                for v in 0..3 {
                    let f = |i: usize| {
                        let a = o + 12 * v + 4 * i;
                        f64::from(f32::from_le_bytes([b[a], b[a + 1], b[a + 2], b[a + 3]]))
                    };
                    out.push([f(0), f(1), f(2)]);
                }
            }
        } else {
            for line in String::from_utf8_lossy(&b).lines() {
                if let Some(r) = line.trim().strip_prefix("vertex") {
                    let v: Vec<f64> = r.split_whitespace().map(|x| x.parse().unwrap()).collect();
                    out.push([v[0], v[1], v[2]]);
                }
            }
        }
        out.sort_by(|a, b| a.partial_cmp(b).unwrap());
        out.dedup();
        out
    }

    /// How far the kernel's surfaces (an STL's vertices, which lie on them) are from the bodies as
    /// these tests model them (`Between::Spline`, as the kernel skins a loft). `HYPOID_STL` names
    /// a folder with `pinion_0.stl` and `gear_0.stl`; `HYPOID_SPEC` optionally gives
    /// `z1,z2,offset,beta,left(1/0),sigma,backlash` (ring gear Ø200).
    #[test]
    #[ignore]
    fn kernel_surfaces_against_the_model() {
        let Ok(dir) = std::env::var("HYPOID_STL") else { return };
        let mut s = spec();
        if let Ok(v) = std::env::var("HYPOID_SPEC") {
            let f: Vec<&str> = v.split(',').collect();
            (s.teeth, s.teeth2, s.offset, s.spiral_angle, s.left_hand) =
                (f[0].parse().unwrap(), f[1].parse().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap(), f[4] == "1");
            if let Some(g) = f.get(5) {
                s.shaft_angle = g.parse().unwrap();
            }
            if let Some(g) = f.get(6) {
                s.backlash = g.parse().unwrap();
            }
            s.module = 200.0 / f64::from(s.teeth2);
        }
        let p = crate::plan(&s).unwrap();
        for (k, file) in [(0usize, "pinion_0.stl"), (1, "gear_0.stl")] {
            let z = if k == 0 { s.teeth } else { s.teeth2 } as usize;
            let solid = Solid::of(&p.bodies[k], z);
            let pts = stl_points(&std::path::Path::new(&dir).join(file));
            let st = &solid.parts[0];
            let (z0, z1) = (st.z[0], st.z[st.z.len() - 1]);
            let every = (pts.len() / 30_000).max(1);
            let mut errs: Vec<(f64, f64, f64)> = Vec::new();
            for x in pts.iter().step_by(every) {
                let (q, lz) = solid.local(*x, 0.0);
                // The toothed loft's side walls only, away from its end caps.
                if lz < z0 + 0.05 || lz > z1 - 0.05 {
                    continue;
                }
                if let Some(d) = st.depth(q, lz, Between::Spline) {
                    errs.push((d, q.len(), lz));
                }
            }
            errs.sort_by(|a, b| a.0.abs().partial_cmp(&b.0.abs()).unwrap());
            let n = errs.len();
            let pct = |f: f64| errs[((n as f64 - 1.0) * f) as usize].0.abs();
            let w = errs[n - 1];
            eprintln!(
                "{file} vs Spline: {n} vertices; |error| median {:.5} p90 {:.5} p99 {:.5} max {:.5} mm (signed {:.5}, at r {:.2}, z {:.2})",
                pct(0.5),
                pct(0.9),
                pct(0.99),
                w.0.abs(),
                w.0,
                w.1,
                w.2
            );
        }
    }

    /// Off 90°, the set still builds, its pinion's axis at the shaft angle to the ring gear's,
    /// lofted tooth to tooth; too far off, it is refused with the angle that works.
    #[test]
    fn shaft_angles_build_at_their_angle_or_are_refused() {
        for sigma in [60.0, 75.0, 90.0] {
            let mut s = spec();
            s.shaft_angle = sigma;
            let p = crate::plan(&s).unwrap_or_else(|e| panic!("Σ {sigma}: {e}"));
            let (pz, gz) = (p.bodies[0].frame.z, p.bodies[1].frame.z);
            let angle = dot(pz, gz).clamp(-1.0, 1.0).acos().to_degrees();
            assert!((angle - sigma).abs() < 1e-9 || (180.0 - angle - sigma).abs() < 1e-9, "Σ {sigma}: axes {angle}°");
            for (b, teeth) in p.bodies.iter().zip([s.teeth, s.teeth2]) {
                let Step::Loft { sections, .. } = &b.steps[0] else { panic!() };
                let k = loft_shifts(&b.frame, sections);
                let per = corners(&sections[0].0).len() / teeth as usize;
                assert!(k.windows(2).all(|w| (w[1] + per * teeth as usize - w[0]).is_multiple_of(per)), "Σ {sigma}: {} would slip", b.name);
            }
        }
        for sigma in [105.0, 120.0] {
            let mut s = spec();
            s.shaft_angle = sigma;
            let e = crate::plan(&s).unwrap_err();
            assert!(e.starts_with("shaft angle too large: use at most") && e.len() < 120, "{e}");
        }
    }

    /// The fraction of a circle about a body's axis (radius `r`, body height `z`) that is inside
    /// it, over `pitches` tooth pitches.
    fn fill(b: &Solid, teeth: usize, r: f64, z: f64, pitches: f64) -> f64 {
        let n = 720;
        let inside = (0..n)
            .filter(|k| {
                b.flat(V2::polar(r, TAU / teeth as f64 * pitches * (*k as f64 + 0.5) / n as f64), z, Between::Spline).is_some_and(|d| d > 0.0)
            })
            .count();
        inside as f64 / n as f64
    }

    /// Ring gear and pinion teeth as thick as their spaces are wide, give or take (the ring
    /// gear's teeth are thinner at the pitch cone, the long addendum pinion's thicker), at mid
    /// face and towards both ends: a ring gear cut on planes square to its flat cone once ended
    /// in long thin blades inside its toe.
    #[test]
    fn teeth_and_spaces_are_in_proportion_across_the_face() {
        let s = spec();
        let set = work_out(&s).unwrap();
        let p = crate::plan(&s).unwrap();
        let (pinion, gear) = (Solid::of(&p.bodies[0], s.teeth as usize), Solid::of(&p.bodies[1], s.teeth2 as usize));
        let f = &set.form;
        let back = f.z_lo - 0.06 * 2.0 * set.d.re2;
        for mu in [f.mu_i + 0.03, 1.0, f.mu_e - 0.03] {
            let q = f.point(0.0, mu);
            let k = fill(&gear, s.teeth2 as usize, q.x, q.y - back, 3.0);
            eprintln!("ring gear at mu {mu:.3}: teeth fill {k:.3} of the pitch circle");
            assert!((0.3..0.6).contains(&k), "ring gear at mu {mu}: {k}");
        }
        let face = set.face;
        for cone in [face.toe + 0.15 * (face.heel - face.toe), 0.5 * (face.toe + face.heel), face.heel - 0.15 * (face.heel - face.toe)] {
            let q = face.at(cone, 0.0);
            let k = fill(&pinion, s.teeth as usize, q.x, set.z_of(q.y), 2.0);
            eprintln!("pinion at cone distance {cone:.2}: teeth fill {k:.3} of the pitch circle");
            assert!((0.4..0.7).contains(&k), "pinion at {cone}: {k}");
        }
    }

    /// No tooth beyond the toe and heel cones of either member, teeth all the way between them.
    #[test]
    fn teeth_end_on_their_toe_and_heel_cones() {
        for s in [spec(), Spec { face_width2: 24.0, face_offset2: 3.0, ..spec() }, Spec { shaft_angle: 70.0, ..spec() }] {
            let set = work_out(&s).unwrap();
            let p = crate::plan(&s).unwrap();
            let (pinion, gear) = (Solid::of(&p.bodies[0], s.teeth as usize), Solid::of(&p.bodies[1], s.teeth2 as usize));
            let f = &set.form;
            let back = f.z_lo - 0.06 * 2.0 * set.d.re2;
            // The ring gear: along its cone, from under the toe to past the heel, between the
            // roots and the tips.
            let depth = f.ha + f.hf;
            for j in 0..=40 {
                let mu = 0.5 + 0.03 * f64::from(j);
                let any = [0.3, 0.6, 0.9].iter().any(|h| {
                    let q = f.point(-f.hf + h * depth, mu);
                    fill(&gear, s.teeth2 as usize, q.x, q.y - back, 1.0) > 0.0
                });
                let within = (f.mu_i + 0.005..=f.mu_e - 0.005).contains(&mu);
                if (f.mu_i - 0.005..=f.mu_e + 0.005).contains(&mu) && !within {
                    continue;
                }
                assert_eq!(any, within, "ring gear at mu {mu} ({} to {})", f.mu_i, f.mu_e);
            }
            // The pinion: along its pitch cone, the same.
            let face = set.face;
            let d = &set.d;
            for j in 0..=40 {
                let cone = face.toe - 8.0 + (face.heel - face.toe + 16.0) * f64::from(j) / 40.0;
                let any = [-0.6 * d.hf1, 0.0, 0.6 * d.ha1].iter().any(|h| {
                    let q = face.at(cone, *h * cone / d.cone_m1);
                    fill(&pinion, s.teeth as usize, q.x, set.z_of(q.y), 1.0) > 0.0
                });
                if (cone - face.toe).abs() < 0.3 || (cone - face.heel).abs() < 0.3 {
                    continue;
                }
                assert_eq!(any, (face.toe..=face.heel).contains(&cone), "pinion at cone distance {cone} ({} to {})", face.toe, face.heel);
            }
            let fact = |l: &str| p.facts.iter().find(|x| x.label == l).unwrap().value;
            assert!((fact("pinion face width") - (face.heel - face.toe)).abs() < 1e-9);
            if s.face_width2 > 0.0 {
                assert!((fact("pinion face width") - s.face_width2).abs() < 1e-9);
            }
        }
    }

    /// The ring gear's outside diameter is ISO 23509's d_ae2 = d_e2 + 2·h_ae2·cos δ2 (standard
    /// depth taper: h_ae2 = h_am2·R_e2/R_m2), measured on the body.
    #[test]
    fn the_ring_gear_outside_diameter_is_the_iso_one() {
        let s = spec();
        let set = work_out(&s).unwrap();
        let p = crate::plan(&s).unwrap();
        let gear = Solid::of(&p.bodies[1], s.teeth2 as usize);
        let d = &set.d;
        let f = &set.form;
        let iso = 2.0 * d.re2 + 2.0 * d.ha2 * d.cone_e2 / d.cone_m2 * d.delta2.cos();
        let fact = p.facts.iter().find(|x| x.label == "gear outside diameter").unwrap().value;
        assert!((fact - iso).abs() < 1e-9, "{fact} against {iso}");
        let back = f.z_lo - 0.06 * 2.0 * d.re2;
        let corner = f.point(f.ha, f.mu_e);
        let mut most = 0.0f64;
        for kz in -20..=20 {
            let z = corner.y - back + 0.05 * f64::from(kz);
            for k in 0..48 {
                let a = TAU / f64::from(s.teeth2) * f64::from(k) / 48.0;
                if let Some(r) = (0..80)
                    .map(|i| corner.x + 0.4 - 0.01 * f64::from(i))
                    .find(|r| gear.flat(V2::polar(*r, a), z, Between::Spline).is_some_and(|x| x > 0.0))
                {
                    most = most.max(r);
                }
            }
        }
        eprintln!("ring gear outside diameter {:.3} (ISO {iso:.3})", 2.0 * most);
        assert!((2.0 * most - iso).abs() < 0.1, "{} against {iso}", 2.0 * most);
    }

    /// The backlash stays between the members: the pinion is cut by the gear without it.
    #[test]
    fn backlash_leaves_a_gap() {
        let mut s = spec();
        s.backlash = 0.1;
        let set = pair(&s);
        let (pen, gap) = set.run(Between::Spline, 1.0, 8, 0.0);
        // The ring gear thinned by j: each flank about j/2 · cos α apart.
        let want = 0.5 * s.backlash * s.pressure_angle.to_radians().cos();
        eprintln!("backlash 0.1: penetration {pen:.5} mm, largest least gap {gap:.5} mm (about {want:.3} expected)");
        assert!(pen < 1e-3, "{pen}");
        assert!(gap > 0.3 * want && gap < 2.5 * want, "{gap}");
    }

    #[test]
    fn other_hands_offsets_and_counts_mesh() {
        for (z1, z2, e, beta, left) in [(11, 41, 25.0, 45.0, false), (10, 43, 35.0, 50.0, true), (13, 39, 12.0, 40.0, true)] {
            let mut s = spec();
            (s.teeth, s.teeth2, s.offset, s.spiral_angle, s.left_hand) = (z1, z2, e, beta, left);
            s.module = 200.0 / f64::from(z2);
            let set = pair(&s);
            let m = set.module;
            let (pen, gap) = set.run(Between::Spline, 2.0, 16, 0.0);
            eprintln!(
                "hypoid {z1}:{z2} E {e} β1 {beta}° left {left}: penetration {pen:.5} mm ({:.2e} module), largest least gap {gap:.5} mm",
                pen / m
            );
            assert!(pen <= 3e-3 * m, "penetration {pen}");
            assert!(gap <= 6e-3 * m, "gap {gap}");
            let (pen, _) = set.run(Between::Spline, 1.0, 4, 0.5);
            assert!(pen > 0.1 * m, "half a pitch out of step only overlaps {pen}");
        }
    }
}
