//! WR-704 / WR-713 (`cone_iso_circle_at_axial_height` and
//! `cone_unbounded_parameterized_as_nurb` retired, part of #14246): every golden row the
//! retirement moves is the correct answer for the cone, by certificate or closed form, and
//! the old row is either as correct (EQUAL) or not (CLOSER). A row is FARTHER when the new
//! value fails its certificate and the old one passes; none is.
//!
//! Run (seconds):
//! `WR713_INVENTORY=<jsonl> WR713_OUTPUTS=<old.jsonl> cargo test --profile goldens -p goldens --test wr704_wr713_cone_moves -- --ignored --nocapture`
//! with `<jsonl>` the moved rows (`{"unit", "case", "old", "new"}`: the golden lines of the
//! parent and of this commit, the mechanical diff of the goldens files) and `<old.jsonl>`
//! the parent's full outputs of those cases (`wr704_wr713_outputs.rs` run on the parent;
//! the goldens keep only block sums of long arrays).
//!
//! # Authentication
//!
//! Each case is re-run here: its output must agree with the inventory's new line and with
//! the golden file's line, and the old outputs, with the old line's status fields, with the
//! inventory's old line, by the goldens' own comparison (`golden::diff`: exact statuses,
//! values within its relative tolerance; a test binary inlines differently from the runner,
//! so the last bits of long block sums may differ). So the values certified are the
//! goldens' values, as the goldens check them.
//!
//! # The certificates (one per op, applied to the new and to the old values alike)
//!
//! The cone `T` is the new cone's analytic data (placement, base radius, semi-angle). A
//! build row certifies its placement against the inputs' frame by closed form, and an
//! unbounded cone's semi-angle as the input's modulo 180 degrees (the same `tan`, the same
//! generator line). Two old rows fail the frame, with an input `Y` of `-1e-300 Y`, which
//! upstream unitizes to zero and replaces by `Z`.
//!
//! **Axes upstream cannot unitize** ([`cert_rescaled`], [`AXIS_SCALED_ROWS`]). Upstream's
//! definition unitizes both axis inputs, so the cone `(O, X, Y, r, s)` defines is the same
//! for `X -> λ X` and `Y -> μ Y` with `λ, μ > 0`; only floating point (`|v|^2`
//! underflowing or overflowing, or the unitize threshold) tells them apart. Such a row is
//! re-run with each axis scaled by the power of two that brings its largest coordinate into
//! `[1, 2)` (exact: `(0, -1e-300, 0)` by `2^996`, `(0, -1e300, 1)` by `2^-996`, `5e-13` by
//! `2^41`), an ordinary input upstream's own unitize reads. That run is certified with the
//! ordinary certificate, and the row's new value must equal it (bit for bit, else within the
//! goldens' comparison); the old value is certified against the rescaled inputs too. So the
//! frame these rows are held to follows from the definition's scale invariance, not from a
//! convention of this proof (PROVEN). The `5e-13` axis is the one a reader may call a
//! tolerance rather than an artefact; the same invariance decides it. The 32 rows:
//! * `surface.cone_p1` (2): `height_without_polar_tiny_axis_swap0_inside1` and
//!   `_swap1_inside1`;
//! * `surface.cone_p2` (16), every `nopolar_*` row (input `Y` of `(0, -1e-300, 0)`):
//!   `nopolar_bounded_{off_pos, off_trunc, rebuild, recognize, trim, y_trim_o0_full}`, the
//!   same six with `nopolar_bounded_apex_`, `nopolar_bounded_cone_{rebuild, recognize,
//!   trim}` and `nopolar_bounded_cone_x_trim_oneg_full`. That last one goes from one
//!   interval to none: on the cone the inputs define, the trim ellipse lies inside the
//!   solid nappe, away from the surface (certified by `cert_trim`), while main's cone
//!   (its `Y` replaced by `Z`) is another cone;
//! * `surface.cone_p3` (14): `rejected_tiny_axis_{0, 3, cylinders}_{cone, other}`,
//!   `rejected_below_unitize_threshold_axis` (`5e-13`), `fuzz_axis_overflow_zero_y` with
//!   `_swapped` and `_trimmed`, `fuzz_height_06674` (axis `(0, -1e300, 1)`),
//!   `fuzz_height_06289` (`1e-300`) and `missing_polar_height_plane_1_y` and `_2_y`
//!   (`-1e-300`).
//!
//! Every other state row checks the data bit-equal old and new (the semi-angle modulo
//! 180), so both sides answer for the same cone. `T` has the STEP domain `D`, itself certified by the
//! state certificate: `u` over
//! `[0, 360]`, the height interval on one nappe (it does not cross the apex height
//! `-r0 / tan(semi)`), an unbounded cone's from the apex to `SM_INFINITE_PARAMETER` on the
//! side of the base circle (`v = 0`, the cone's defining radius). Nothing is sampled.
//!
//! * **State** (`build`, `copy`, `adjust`, `transform`, the constructor queries): the NURB
//!   form lies on `T` (`implicit::lies_on`, every Bézier patch's Bernstein coefficients of
//!   `T`'s equation); the polar converter's circle and the generator lie on `T`
//!   (`implicit::curve_lies_on`), the circle in one plane across the axis; the domain as
//!   above; the apex, radii, heights and extents by closed form.
//! * **STEP evaluation** (`eval_step`): every derivative the evaluator writes against the
//!   closed form `O + R(v)(cos u X + sin u Y) + v Z`, at the clamped parameter.
//! * **Parameter maps and NURB evaluation** (`eval`, `eval_point`, `to_step`, `to_nurbs`):
//!   the STEP point of the STEP parameter and the form's point of the NURB parameter agree
//!   within the rounding `implicit::evaluation_ratio_of` derives (the bound of
//!   `implicit::evaluates_as`), and the STEP parameter is in `D`. Higher derivatives of
//!   `eval` are the certified form's (re-run, authenticated); the old ones are compared to
//!   them.
//! * **Iso circle** (`iso_circle`): `None` exactly when the point's axial height is more
//!   than `tol` outside `D`'s heights; else a closed curve on `T` within the map's rounding
//!   of the plane at the clamped height.
//! * **Iso curve** (`iso`): the curve lies on `T`.
//! * **Inversion**: inside exactly when the point is within `tol` of the patch (the
//!   distance in the meridian plane to the generator segment over `D`, closed form), and an
//!   inside answer's STEP point within `tol` of the point.
//! * **Positive point**: the apex's closed form and the side of its plane.
//! * **Refusals**: a refused construction is correct exactly when the inputs define no
//!   cone, decided in closed form from the input numbers ([`degenerate`]): an unbounded
//!   semi-angle of 90 degrees modulo 180 (no tangent: a plane) or not finite, a zero or
//!   non-finite axis (no direction, no axis `Z`), a cylinder of radius 0 (a line); a
//!   refused op is wrong where the op's certified answer exists.
//! * **Cone intersections** (`surface.cone_p3`): the closed-form answer for each pair
//!   (coaxial, parallel cylinders, cone and plane), with a witness section where two
//!   circles must cross.
//! * **Corner blends** (`nurbs.b_spline_surface_n_lib_p2`, regenerated with the kernel):
//!   G0. Each patch side equals its loop curve (or its half for an n-sided patch), by
//!   Bézier blossoms after degree elevation. G1 is the sup-norm angle per side between
//!   the patch normal and the neighbour's normal (the implicit gradient of the data
//!   cylinder, sphere, cone or torus), bounded by Bernstein forward-error bounds of
//!   `sin^2` per Bézier segment. That maximum sits at the patch corners, where the shared
//!   loop curves fix both normals, so it is corner-saturated: EQUAL there means the
//!   corners agree, not the side interiors. Some rows moved because the neighbour did
//!   (a bounded cylinder on non-unit axes, whose NURB main built with radius `r |X|`);
//!   the others moved because the refinement loop now compares copies side by side. Each
//!   copy is measured at the same stations (the union of the bare and every refined copy's
//!   knot stations), against side normals read once from the bare patch; a station counts
//!   only where its angle bound is certified (error below a right angle). A copy is taken
//!   only when, against every copy held so far, no side is worse, undecided or uncertified
//!   where the held copy certifies: "no worse" means its upper bound is at most the held
//!   lower bound plus the resolution `0.5` degrees and its lower bound not above the held
//!   upper bound. A side certainly above but within the resolution vetoes too, unless the
//!   copy's largest angle over all sides is certainly lower than the held copy's by more
//!   than the resolution. Those rows, by label "(tie rule)", are
//!   checked twice before G1 decides the class. First the refinement replays
//!   ([`replay_coons`]): every station's derivative and angle bounds enclose the exact
//!   values (enclosed here from the candidate's net and knots), every candidate is measured
//!   at the bare patch's stations, and an independent replay of the rule picks the logged
//!   copy, which the output holds bit for bit. Then main's patch is found among the
//!   candidates, and a side counts against a patch only with a station where it is
//!   certainly worse and none where it is certainly better
//!   ([`cert_coons_against_main`]). analytic064 (torus), whose refined copies are
//!   certainly worse along two sides, keeps main's bare patch, and analytic111 the 32-span
//!   copy main took; neither moves. [`coons_refinements_replay_with_honest_bounds`]
//!   replays them, analytic028 and `topology.brep_p1`'s `vase_rev_corner_blend_3` and
//!   `Endcap_corner_blend_3`.
//! * **Whole-solid corner blends** (`topology.brep_p1`, label "Coons replay, stations"):
//!   the neighbours have no implicit form, so only the replay and the comparison with
//!   main's candidate apply. `Endcap_corner_blend_3` moves: main took a refined copy whose
//!   bottom side reaches 1.1353 ± 2.6e-5 at a station where the bare patch is certified at
//!   1.0752 ± 3e-4, so every refined copy is certainly worse there and the bare patch is
//!   kept. Main's copy is also certainly nearer to tangent at other stations, so neither
//!   patch fails the pointwise comparison and the row is EQUAL, not CLOSER.
//! * **Extrusion and cone curves** (`surface.surf_of_extrusion_p2`): every Bézier segment
//!   lies within `tol` of the extrusion's planes (convex hull) and of the cone (a Bernstein
//!   bound of the implicit equation, by de Casteljau halving).
//!
//! The bounds are `γ_n` multiples of the magnitudes each formula combines (`n` above the
//! operation count of the paths measured, with room), stated at each use. Those constants
//! are not derived to the last factor, so a row whose old value fails its bound by less
//! than [`CLOSER_MARGIN`] (10 times) is classed EQUAL, not CLOSER. The report prints the
//! distribution of the old values' error-to-bound ratios over the CLOSER rows.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::many_single_char_names,
    clippy::float_cmp,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::type_complexity
)] // test

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use goldens::case::repo_root;
use goldens::units::{nurbs_b_spline_surface_n_lib_p2 as nlib, topology_brep_p1 as bp1};
use goldens::{golden, registry, CaseContext, Output};
use serde_json::Value;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::tol::is_zero;
use solidcore::kernel::Kernel;
use solidcore::math::det;
use solidcore::math::{Point2, Vec3, EFF_ZERO, INFINITE_PARAMETER, PI};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::b_spline_surface_n_lib::p2::{CoonsCandidateLog, CoonsTangencyLog};
use solidcore::nurbs::math::{gamma, CPoint, NOW};
use solidcore::nurbs::{crv_basic, srf_basic};
use solidcore::surface::cone::Cone;
use solidcore::surface::implicit::{curve_lies_on, evaluation_ratio_of, lies_on, Implicit};
use solidcore::surface::surface::Surface;

/// The goldens' default number of generated cases per unit.
const SEEDS: usize = 16;
/// `SM_SUCCESS`, `SM_ERR`.
const SUCCESS: f64 = 1000.0;
const SM_ERR: f64 = 1001.0;
/// `SM_ERR_OUTSIDE_OF_DOMAIN` and `SM_ERR_NOT_WITHIN_TOLERANCE`: since WR-662 (#18066) an
/// analytic inversion reports these where it reported `SM_ERR` for a point outside the
/// sweep or off the generator, so both are the "not inside" answer.
const OUTSIDE_OF_DOMAIN: f64 = 1020.0;
const NOT_WITHIN_TOLERANCE: f64 = 1021.0;

// ---- verdicts ---------------------------------------------------------------------------

/// A certificate's verdict on one side's values: `Err` names what fails and, where a
/// measured error stands against a bound, their ratio.
type Verdict = Result<(), Fail>;

#[derive(Debug, Clone)]
struct Fail {
    why: String,
    /// error / bound, where the failure is a value off by more than its bound
    ratio: Option<f64>,
}

fn fail<T>(why: impl Into<String>) -> Result<T, Fail> {
    Err(Fail {
        why: why.into(),
        ratio: None,
    })
}

/// `|err| <= bound`, else a failure carrying the ratio.
fn within(what: &str, err: f64, bound: f64) -> Verdict {
    if err <= bound {
        Ok(())
    } else {
        Err(Fail {
            why: format!("{what}: {err:e} > {bound:e}"),
            ratio: Some(if err.is_nan() {
                f64::INFINITY
            } else {
                err / bound
            }),
        })
    }
}

fn check(cond: bool, why: impl Into<String>) -> Verdict {
    if cond {
        Ok(())
    } else {
        fail(why)
    }
}

// ---- reading values -----------------------------------------------------------------------

struct Rd<'a> {
    v: &'a [f64],
    i: usize,
}

impl<'a> Rd<'a> {
    fn new(v: &'a [f64]) -> Self {
        Self { v, i: 0 }
    }
    fn f(&mut self) -> f64 {
        let x = self.v[self.i];
        self.i += 1;
        x
    }
    fn n(&mut self, k: usize) -> Vec<f64> {
        let x = self.v[self.i..self.i + k].to_vec();
        self.i += k;
        x
    }
    fn v3(&mut self) -> Vec3 {
        Vec3::new(self.f(), self.f(), self.f())
    }
    fn rest(&self) -> usize {
        self.v.len() - self.i
    }
}

/// A curve as `put_curve` writes it.
#[derive(Debug, Clone)]
struct CurveData {
    ivl: [f64; 2],
    nurb: Option<(usize, Vec<CPoint>, Vec<f64>)>,
}

fn read_curve(r: &mut Rd) -> Option<CurveData> {
    if r.f() == 0.0 {
        return None;
    }
    let _type = r.f();
    let ivl = [r.f(), r.f()];
    if r.f() == 0.0 {
        return Some(CurveData { ivl, nurb: None });
    }
    let deg = r.f() as usize;
    let n = r.f() as usize;
    let pts = (0..n)
        .map(|_| CPoint::new(r.f(), r.f(), r.f(), r.f()))
        .collect();
    let nk = r.f() as usize;
    let knots = r.n(nk);
    Some(CurveData {
        ivl,
        nurb: Some((deg, pts, knots)),
    })
}

impl CurveData {
    fn bspline(&self) -> Option<BSplineCurve> {
        let (p, pts, knots) = self.nurb.clone()?;
        let c = crv_basic::Curve::new(pts, p, knots).ok()?;
        Some(BSplineCurve::from_nurb(c, 3, true))
    }
    /// The Euclidean control points.
    fn points(&self) -> Vec<Vec3> {
        self.nurb.as_ref().map_or_else(Vec::new, |(_, pts, _)| {
            pts.iter()
                .map(|c| {
                    let w = if c.w == NOW { 1.0 } else { c.w };
                    Vec3::new(c.x, c.y, c.z) * (1.0 / w)
                })
                .collect()
        })
    }
}

/// A cone's state as `put_state` writes it.
#[derive(Debug, Clone)]
struct State {
    /// type, strings, kind flags, radius, angle, flags, placement: compared bit-exact
    identity: Vec<f64>,
    r0: f64,
    semi: f64,
    bounded: bool,
    origin: Vec3,
    x: Vec3,
    y: Vec3,
    dom: [f64; 4],
    radh: Vec<f64>,
    lin: Vec<f64>,
    maxdom: Vec<f64>,
    apex: Vec<f64>,
    height: Vec<f64>,
    polar: bool,
    angles: Vec<f64>,
    polar_curve: Option<CurveData>,
    gen: Option<CurveData>,
    form: Option<BSplineSurface>,
}

fn read_state(r: &mut Rd) -> State {
    let start = r.i;
    let _type = r.f();
    for _ in 0..2 {
        let n = r.f() as usize;
        r.n(n);
    }
    let _kinds = r.n(6);
    let r0 = r.f();
    let semi_at = r.i - start;
    let semi = r.f();
    let flags = r.n(5);
    let origin = r.v3();
    let x = r.v3();
    let y = r.v3();
    let mut identity = r.v[start..r.i].to_vec();
    // the angle modulo 180 degrees: `s` and `s -+ 180` are one cone (same `tan`, same
    // generator line); main keeps an unbounded cone's angle as given, the new code its
    // representative in (-90, 90)
    identity[semi_at] = semi_rep(semi);
    let d = r.n(4);
    let dom = [d[0], d[1], d[2], d[3]];
    let radh = r.n(5);
    let lin = r.n(3);
    let maxdom = r.n(4);
    let apex = r.n(6);
    let _canonical = r.n(7);
    let height = r.n(2);
    let polar = r.f() != 0.0;
    let na = r.f() as usize;
    let angles = if na > 0 { r.n(2) } else { Vec::new() };
    let polar_curve = read_curve(r);
    let gen = read_curve(r);
    let form = if r.f() == 0.0 {
        None
    } else {
        let p = r.f() as usize;
        let q = r.f() as usize;
        let n = r.f() as usize;
        let m = r.f() as usize;
        let net: Vec<Vec<CPoint>> = (0..n)
            .map(|_| {
                (0..m)
                    .map(|_| CPoint::new(r.f(), r.f(), r.f(), r.f()))
                    .collect()
            })
            .collect();
        let nu = r.f() as usize;
        let u = r.n(nu);
        let nv = r.f() as usize;
        let v = r.n(nv);
        let s = srf_basic::Surface::new(net, p, q, u, v).unwrap();
        Some(BSplineSurface::from_nurb(s, true))
    };
    State {
        identity,
        r0,
        semi,
        bounded: flags[4] != 0.0,
        origin,
        x,
        y,
        dom,
        radh,
        lin,
        maxdom,
        apex,
        height,
        polar,
        angles,
        polar_curve,
        gen,
        form,
    }
}

// ---- the cone ---------------------------------------------------------------------------

/// `tan` of an angle in degrees, as the cone takes it (`deg2rad(semi).tan()`).
fn tan_deg(deg: f64) -> f64 {
    det::tan(deg * PI / 180.0)
}

/// `|v|`'s largest coordinate.
fn mc(v: Vec3) -> f64 {
    v.x.abs().max(v.y.abs()).max(v.z.abs())
}

/// The analytic cone and its STEP domain.
#[derive(Debug, Clone, Copy)]
struct Analytic {
    o: Vec3,
    x: Vec3,
    y: Vec3,
    z: Vec3,
    r0: f64,
    semi: f64,
    t: f64,
    /// `u0, v0, u1, v1`
    dom: [f64; 4],
    oob: bool,
}

impl Analytic {
    fn of_cone(c: &Cone) -> Self {
        let pos = c.rev().position();
        let d = c.rev().step_uv_domain();
        Self {
            o: pos.origin(),
            x: pos.x_axis(),
            y: pos.y_axis(),
            z: pos.z_axis(),
            r0: c.base_radius(),
            semi: c.semi_angle_deg(),
            t: tan_deg(c.semi_angle_deg()),
            dom: [d.min().x, d.min().y, d.max().x, d.max().y],
            oob: c.rev().is_out_of_bounds_enabled(),
        }
    }
    fn of_state(s: &State) -> Self {
        Self {
            o: s.origin,
            x: s.x,
            y: s.y,
            z: s.x.cross(s.y),
            r0: s.r0,
            semi: s.semi,
            t: tan_deg(s.semi),
            dom: s.dom,
            oob: false,
        }
    }
    fn implicit(&self) -> Implicit {
        Implicit::Cone {
            origin: self.o,
            axis: self.z,
            radius: self.r0,
            tan_angle: self.t,
        }
    }
    fn cylinder(&self) -> bool {
        is_zero(self.semi)
    }
    /// The apex height `-r0 / tan(semi)` and its bound (`γ_4` of it: the angle's
    /// conversion, the tangent's correctly rounded-or-1-ulp result and the quotient).
    fn apex_h(&self) -> (f64, f64) {
        let h = -self.r0 / self.t;
        (h, gamma(8) * h.abs())
    }
    fn radius(&self, v: f64) -> f64 {
        self.r0 + self.t * v
    }
    /// The STEP point at `(u, v)` degrees and height, by closed form.
    fn point(&self, u: f64, v: f64) -> Vec3 {
        let a = u * PI / 180.0;
        self.o + self.radius(v) * (det::cos(a) * self.x + det::sin(a) * self.y) + v * self.z
    }
    /// The rounding of [`Self::point`] (and of any evaluator computing the same sum): per
    /// coordinate `γ_8` of the magnitudes summed, plus the angle's error (its conversion
    /// `γ_3 |a|` and the sine and cosine within 2 ulp) times the radius, as a length.
    fn point_bound(&self, u: f64, v: f64) -> f64 {
        let a = (u * PI / 180.0).abs();
        let r = self.r0.abs() + (self.t * v).abs();
        3.0_f64.sqrt()
            * (gamma(8) * (mc(self.o) + r + v.abs())
                + r * (gamma(3) * a + 4.0 * gamma(1))
                + v.abs() * self.t_bound())
    }
    /// The error of a tangent of the semi-angle as an evaluator computes it (its own
    /// rounding of the angle to radians, `γ_3 |θ|`, carried by `tan' = 1 + tan²`, and the
    /// tangent's own 2 ulp): two evaluators that convert the degrees in different orders
    /// differ by this (near 90 degrees `tan'` is 3283 at 89).
    fn t_bound(&self) -> f64 {
        let th = (self.semi * PI / 180.0).abs();
        (1.0 + self.t * self.t) * gamma(3) * th + gamma(2) * self.t.abs()
    }
    /// The parameter clamped to the domain unless out-of-bounds evaluation is on.
    fn clamp(&self, u: f64, v: f64) -> (f64, f64) {
        if self.oob {
            (u, v)
        } else {
            (
                u.clamp(self.dom[0], self.dom[2]),
                v.clamp(self.dom[1], self.dom[3]),
            )
        }
    }
    /// The distance from `p` to the patch `D` (full sweep), closed form in the meridian
    /// plane: to the generator segment `(R(v), v)`, `v` in `[v0, v1]`; and its bound.
    fn patch_distance(&self, p: Vec3) -> Option<(f64, f64)> {
        if self.dom[0] > 0.0 || self.dom[2] < 360.0 {
            return None;
        }
        let w = p - self.o;
        let z = w.dot(self.z);
        let rho = (w - z * self.z).length();
        let (v0, v1) = (self.dom[1], self.dom[3]);
        let (a0, a1) = (self.radius(v0), self.radius(v1));
        // the segment from (a0, v0) to (a1, v1)
        let (dx, dz) = (a1 - a0, v1 - v0);
        let len2 = dx * dx + dz * dz;
        let s = if len2 > 0.0 {
            (((rho - a0) * dx + (z - v0) * dz) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (cx, cz) = (a0 + s * dx, v0 + s * dz);
        let d = (det::powi(rho - cx, 2) + det::powi(z - cz, 2)).sqrt();
        let m = mc(self.o) + mc(p) + a0.abs() + a1.abs() + v0.abs() + v1.abs();
        Some((d, gamma(32) * m))
    }
}

/// The representative in `[-90, 90]` of a semi-angle modulo 180 degrees: `s` and `s -+
/// 180` have the same tangent, so the same `Rad(V) = r0 + tan(s) V` and the same generator
/// line (directions that are each other's negatives). `%` is IEEE `fmod` (exact) and the
/// `-+ 180` of a remainder past 90 is exact (Sterbenz).
fn semi_rep(s: f64) -> f64 {
    let r = s % 180.0;
    if r > 90.0 {
        r - 180.0
    } else if r < -90.0 {
        r + 180.0
    } else {
        r
    }
}

/// The semi-angle an unbounded cone's inputs give, if they make an unbounded cone.
fn unbounded_semi(cone: &Value) -> Option<f64> {
    let mode = &cone["mode"];
    let num = |k: &str| mode[k].as_f64();
    match mode["kind"].as_str()? {
        "cone_unbounded" => num("semi"),
        "canonical_semi" if mode["height"].is_null() => num("semi"),
        // `CreateCanonical` with no height makes the unbounded cone with `top` as its
        // semi-angle in degrees
        "canonical" if num("height").is_some_and(|h| h <= 0.0) => num("top"),
        _ => None,
    }
}

/// The inputs' reason, in closed form, that they define no cone, so that a construction
/// must refuse them (exact arithmetic on the input numbers, no rule of the code under
/// test):
/// - an axis that is not finite, or zero: it has no direction, so no axis `Z`;
/// - an unbounded cone's semi-angle of 90 degrees modulo 180: `tan` has no value there,
///   so `Rad(V) = r0 + tan(semi) V` is undefined; the generator perpendicular to the axis
///   sweeps a plane, every point at height 0, which the cone's height parameter `v` cannot
///   parameterise. NaN and infinite angles have no remainder modulo 180;
/// - a cylinder of radius 0: the sweep is the axis line, `dS/du = 0` everywhere.
fn degenerate(cone: &Value) -> Option<String> {
    let v3 = |k: &str| {
        let a = &cone[k];
        Vec3::new(
            a[0].as_f64().unwrap(),
            a[1].as_f64().unwrap(),
            a[2].as_f64().unwrap(),
        )
    };
    let mode = &cone["mode"];
    let num = |k: &str| mode[k].as_f64();
    let kind = mode["kind"].as_str().unwrap();
    // the canonical modes take a placement: the axes the constructor sees are the held
    // ones (`Axis2Placement::new` of the inputs, as the unit makes it)
    let held = matches!(kind, "canonical_semi" | "canonical" | "cyl_canonical");
    let axis = |k: &str| {
        if held {
            let p = Axis2Placement::new(v3("origin"), v3("x"), v3("y"));
            if k == "x" {
                p.x_axis()
            } else {
                p.y_axis()
            }
        } else {
            v3(k)
        }
    };
    // an unbounded cone's semi-angle must not be 90 modulo 180
    if unbounded_semi(cone).is_some_and(|s| !(semi_rep(s).abs() < 90.0)) {
        return Some(
            "a semi-angle of 90 degrees modulo 180 (or not finite): no tangent, a plane".into(),
        );
    }
    let (x, y) = (axis("x"), axis("y"));
    let finite = |a: Vec3| a.x.is_finite() && a.y.is_finite() && a.z.is_finite();
    if !finite(x) || !finite(y) {
        return Some("an axis that is not finite".into());
    }
    let zero = |a: Vec3| a.x == 0.0 && a.y == 0.0 && a.z == 0.0;
    if zero(x) || zero(y) {
        return Some("a zero axis: no direction, no Z = X x Y".into());
    }
    let zero_cylinder = match kind {
        "cyl_canonical" | "cyl_bounded" | "cyl_unbounded" => num("radius") == Some(0.0),
        "cone_unbounded" => num("base") == Some(0.0) && num("semi").is_some_and(is_zero),
        "canonical_semi" => num("bot") == Some(0.0) && num("semi").is_some_and(is_zero),
        "canonical" | "cone_bounded" => num("bot") == Some(0.0) && num("top") == Some(0.0),
        _ => false,
    };
    zero_cylinder.then(|| "a cylinder of radius 0: the axis line, dS/du = 0".into())
}

// ---- certificates ---------------------------------------------------------------------------

/// The state certificate (module docs).
fn cert_state(s: &State) -> Verdict {
    cert_state_with(s, true, true)
}

/// [`cert_state`]; with `apex_end` false an unbounded cone's finite end need not be at
/// its apex (an offset's: the offset of the input's apex circle, which its caller checks).
/// With `exact_form` false the NURB form is not checked here: a rebuild keeps the
/// caller's net, which its contract accepts within a tolerance of a cone; the caller
/// bounds its distance ([`moved_net_distance`]).
fn cert_state_with(s: &State, apex_end: bool, exact_form: bool) -> Verdict {
    let a = Analytic::of_state(s);
    let t_cone = a.implicit();
    let [u0, v0, u1, v1] = s.dom;
    check(u0 == 0.0 && u1 == 360.0 || s.bounded, "sweep not [0, 360]")?;
    // the form
    let Some(form) = &s.form else {
        return fail("no NURB form");
    };
    if exact_form {
        check(
            lies_on(form, &t_cone).unwrap_or(false),
            "the NURB form is off the cone",
        )?;
    }
    // the generator
    let gen = s.gen.as_ref().and_then(CurveData::bspline);
    let Some(gen) = gen else {
        return fail("no generator NURB");
    };
    check(
        curve_lies_on(&gen, &t_cone).unwrap_or(false),
        "the generator is off the cone",
    )?;
    // the polar converter's circle: on the cone, in one plane across the axis
    check(s.polar, "no polar conversion")?;
    let Some(pc) = &s.polar_curve else {
        return fail("no polar curve");
    };
    let Some(pcb) = pc.bspline() else {
        return fail("no polar curve NURB");
    };
    // (without `exact_form` the caller bounds the polar circle's distance:
    // [`circle_distance`])
    if exact_form {
        check(
            curve_lies_on(&pcb, &t_cone).unwrap_or(false),
            "the polar circle is off the cone",
        )?;
        let hs: Vec<f64> = pc.points().iter().map(|p| (*p - a.o).dot(a.z)).collect();
        let (lo, hi) = hs
            .iter()
            .fold((f64::MAX, f64::MIN), |(l, h), x| (l.min(*x), h.max(*x)));
        let reach = pc.points().iter().fold(0.0_f64, |m, p| m.max(mc(*p)));
        // (the spread of two points' computed heights, each within `γ_16` of its size)
        within(
            "polar circle's plane",
            hi - lo,
            2.0 * gamma(16) * (reach + mc(a.o)),
        )?;
    }
    check(
        s.angles.len() == 2 && s.angles[0] == u0 && s.angles[1] == u1,
        "polar angles are not the sweep",
    )?;
    // the domain: on one nappe; an unbounded cone's from the apex to the infinite end on
    // the base circle's side
    let inf = INFINITE_PARAMETER;
    if a.cylinder() {
        if !s.bounded {
            check(v0 == -inf && v1 == inf, "unbounded cylinder's heights")?;
        }
    } else {
        let (h, b) = a.apex_h();
        // (a bounded cone's heights are its inputs' `[0, height]`: radii of opposite signs
        // ask for the patch across the apex)
        check(
            s.bounded || (v0 >= h - b && v1 >= h - b) || (v0 <= h + b && v1 <= h + b),
            "the heights cross the apex",
        )?;
        if !s.bounded {
            check(
                v0 <= 0.0 && 0.0 <= v1,
                "the base circle is not in the domain",
            )?;
            // the apex end, at the infinite parameter when the apex is beyond it (or at
            // it: a cone recognised from a NURB that runs from the infinite end of an
            // unbounded cone up to its apex has its origin at the far end, the apex at
            // `SM_INFINITE_PARAMETER` above; then the infinite end is the apex end)
            check(v0 == -inf || v1 == inf, "no infinite end")?;
            if h.abs() > inf + b {
                let apex_end = if v0 == -inf { v1 } else { v0 };
                check(
                    apex_end.abs() == inf,
                    "an apex beyond the infinite parameter",
                )?;
            } else if apex_end {
                let hc = h.clamp(-inf, inf);
                within("apex end", (v0 - hc).abs().min((v1 - hc).abs()), b)?;
            }
        }
        // apex
        check(s.apex[0] == SUCCESS && s.apex[1] == 0.0, "apex status")?;
        let pt = Vec3::new(s.apex[2], s.apex[3], s.apex[4]);
        let want = a.o + h * a.z;
        within(
            "apex point",
            (pt - want).length(),
            3.0_f64.sqrt() * gamma(8) * (mc(a.o) + h.abs()),
        )?;
        // (the parameter is the generator line's inversion of the apex: with the apex off
        // the generator's ends, as for an offset's, the line's points are known to the
        // rounding of its ends' heights)
        let span = if apex_end {
            0.0
        } else {
            v0.abs().max(v1.abs())
        };
        within(
            "apex parameter",
            (s.apex[5] - h).abs(),
            b + gamma(16) * (mc(a.o) + h.abs() + span),
        )?;
        // (a bounded patch across the apex keeps its own heights)
        let (lo, hi) = (v0.min(v1), v0.max(v1));
        // (an apex within its own rounding `b` of an end is at that end)
        let across = s.apex[5] > lo + EFF_ZERO.max(b) && s.apex[5] < hi - EFF_ZERO.max(b);
        check(
            s.maxdom[0] == 0.0
                && s.maxdom[2] == 360.0
                && if across {
                    s.bounded && s.maxdom[1] == lo && s.maxdom[3] == hi
                } else {
                    // limited at the apex: at its parameter, or at a domain end within
                    // the apex height's rounding `b` of it (the apex is then that end)
                    (s.maxdom[1] - s.apex[5]).abs() <= b || (s.maxdom[3] - s.apex[5]).abs() <= b
                },
            "max analytic domain not limited at the apex",
        )?;
    }
    // radii, heights, extent: closed forms of the domain
    let rb = |v: f64| gamma(4) * (s.r0.abs() + (a.t * v).abs());
    within("bottom radius", (s.radh[0] - a.radius(v0)).abs(), rb(v0))?;
    within("top radius", (s.radh[1] - a.radius(v1)).abs(), rb(v1))?;
    check(s.radh[2] == v0 && s.radh[3] == v1, "bottom/top heights")?;
    check(s.lin == [v0, v1, 1.0], "linear extent")?;
    // height: the form's corner points' axial span, the domain's
    let reach = form_reach(form);
    within(
        "height",
        (s.height[1] - (v1 - v0)).abs(),
        gamma(64) * (reach + v0.abs() + v1.abs() + mc(a.o)),
    )?;
    Ok(())
}

fn form_reach(form: &BSplineSurface) -> f64 {
    let (_, _, cps) = form.control_points().unwrap();
    cps.iter().fold(0.0_f64, |m, c| {
        let w = if c.w == NOW { 1.0 } else { c.w };
        m.max(mc(Vec3::new(c.x, c.y, c.z) * (1.0 / w)))
    })
}

/// The new cone, as the case builds it, with its form.
struct Ctx {
    k: Kernel,
    a: Analytic,
    surface: Surface,
    form: BSplineSurface,
}

impl Ctx {
    /// `evaluation_ratio_of` at the STEP parameter `step` and the form's point `f`.
    fn ratio(&self, step: (f64, f64), f: Vec3) -> Verdict {
        let r = evaluation_ratio_of(
            &self.k,
            &self.surface,
            &self.form,
            Point2::new(step.0, step.1),
            f,
        )
        .map_err(|e| Fail {
            why: format!("evaluation_ratio_of: {e}"),
            ratio: None,
        })?;
        let Some(r) = r else {
            return fail("no evaluation bound for the cone");
        };
        within("STEP point vs form point (ratio to bound)", r, 1.0)
    }
    /// The form's point at `uv` clamped to its domain (the form's patch: past its ends
    /// the B-spline extrapolates its end spans, which name no point of the cone).
    fn form_point(&self, uv: (f64, f64)) -> Vec3 {
        let d = self.form.natural_uv_domain();
        let uv = (
            uv.0.clamp(d.min().x, d.max().x),
            uv.1.clamp(d.min().y, d.max().y),
        );
        self.form.evaluate_point(Point2::new(uv.0, uv.1)).unwrap()
    }
    /// The readings of a STEP parameter on `D`'s patch: clamped to `D` (as the STEP
    /// form evaluates), and on a full sweep also with the angle reduced modulo 360.
    fn step_readings(&self, u: f64, v: f64) -> Vec<(f64, f64)> {
        let [u0, v0, u1, v1] = self.a.dom;
        let vc = v.clamp(v0, v1);
        let mut r = vec![(u.clamp(u0, u1), vc)];
        if u0 == 0.0 && u1 == 360.0 {
            r.push((u.rem_euclid(360.0), vc));
        }
        r
    }
    /// The height `v` (clamped to `D`) of a STEP parameter against the form's point
    /// `f`, in the meridian plane: `f`'s axial height is `v` and its distance from the
    /// axis `|R(v)|`, within the point's and the form's roundings. Certifies a moved
    /// height where the angle did not move.
    fn meridian(&self, step: (f64, f64), f: Vec3) -> Verdict {
        let [_, v0, _, v1] = self.a.dom;
        let v = step.1.clamp(v0.min(v1), v0.max(v1));
        let w = f - self.a.o;
        let h = w.dot(self.a.z);
        let rho = (w - h * self.a.z).length();
        let b = self.a.point_bound(step.0, v) + gamma(32) * (form_reach(&self.form) + mc(self.a.o));
        within("form point's height vs the STEP height", (h - v).abs(), b)?;
        within(
            "form point's radius vs R(v)",
            (rho - self.a.radius(v).abs()).abs(),
            b,
        )
    }
    /// The STEP parameter `(u, v)` lies in `D` (to the rounding of a conversion).
    fn in_domain(&self, u: f64, v: f64) -> Verdict {
        let [u0, v0, u1, v1] = self.a.dom;
        let bu = gamma(16) * 360.0;
        let bv = gamma(16) * (v0.abs() + v1.abs());
        check(
            u >= u0 - bu && u <= u1 + bu && v >= v0 - bv && v <= v1 + bv,
            format!("STEP parameter ({u}, {v}) outside the domain"),
        )
    }
}

/// The STEP derivatives `evaluate_step` writes at `(u, v)` (u-major, `(hu+1)(hv+1)`),
/// closed form, and their bounds; `None` where the evaluator writes nothing (kept 0).
fn step_derivatives(
    a: &Analytic,
    u: f64,
    v: f64,
    hu: usize,
    hv: usize,
) -> Vec<Option<(Vec3, f64)>> {
    let (u, v) = a.clamp(u, v);
    let th = u * PI / 180.0;
    let c = PI / 180.0;
    let (cs, sn) = (det::cos(th), det::sin(th));
    let e = [
        cs * a.x + sn * a.y,
        -sn * a.x + cs * a.y,
        -cs * a.x - sn * a.y,
        sn * a.x - cs * a.y,
    ];
    let r = a.radius(v);
    let (v0, v1) = (a.dom[1], a.dom[3]);
    // the evaluator's slope (R(v1) - R(v0)) / (v1 - v0): the tangent within the radii's
    // rounding over the length, and the quotient's
    let b_rv = gamma(8) * (a.radius(v1).abs() + a.radius(v0).abs()) / (v1 - v0).abs()
        + gamma(4) * a.t.abs()
        + a.t_bound() * (v1.abs() + v0.abs()) / (v1 - v0).abs();
    let mh = hu.min(hv);
    let mut out = vec![Some((Vec3::ZERO, 0.0)); (hu + 1) * (hv + 1)];
    let idx = |i: usize, j: usize| i * (hv + 1) + j;
    let angle_err = gamma(3) * th.abs() + 4.0 * gamma(1);
    let rad = r.abs();
    // the radius's own error: the tangent's times the height
    let b_r = v.abs() * a.t_bound();
    out[0] = Some((a.point(u, v), a.point_bound(u, v)));
    for i in 1..=hu.min(3) {
        let ci = det::powi(c, i as i32);
        let b = 3.0_f64.sqrt() * ci * (gamma(8) * rad + rad * angle_err + b_r) * (1.0 + gamma(4));
        out[idx(i, 0)] = Some((r * ci * e[i], b));
    }
    if hv >= 1 {
        out[idx(0, 1)] = Some((
            a.t * e[0] + a.z,
            3.0_f64.sqrt() * (b_rv + gamma(8) * (a.t.abs() + 1.0) + a.t.abs() * angle_err),
        ));
    }
    let cross = |i: usize| {
        let ci = det::powi(c, i as i32);
        Some((
            a.t * ci * e[i],
            3.0_f64.sqrt() * ci * (b_rv + a.t.abs() * (gamma(8) + angle_err)) * (1.0 + gamma(4)),
        ))
    };
    if hu >= 1 && hv >= 1 && mh >= 2 {
        out[idx(1, 1)] = cross(1);
    }
    if hu >= 2 && hv >= 1 && mh >= 3 {
        out[idx(2, 1)] = cross(2);
    }
    // the evaluator writes zeros for (1, 2), (0, 2), (0, 3) and leaves the rest 0: all
    // exact zeros, the closed form's too for the v-derivatives of order 2 and up; the
    // entries it skips (upstream's unwritten slots) are 0 as well
    let written_cross =
        |i: usize, j: usize| (i == 1 && j == 1 && mh >= 2) || (i == 2 && j == 1 && mh >= 3);
    for i in 1..=hu {
        for j in 1..=hv {
            if !written_cross(i, j) {
                out[idx(i, j)] = None;
            }
        }
    }
    for i in 4..=hu {
        out[idx(i, 0)] = None;
    }
    out
}

fn cert_eval_step(a: &Analytic, op: &Value, out: &[f64]) -> Verdict {
    let (u, v) = (op["u"].as_f64().unwrap(), op["v"].as_f64().unwrap());
    let point = op["point"].as_bool().unwrap_or(false);
    let (hu, hv) = if point {
        (0, 0)
    } else {
        (
            op["hu"].as_u64().unwrap() as usize,
            op["hv"].as_u64().unwrap() as usize,
        )
    };
    let want = step_derivatives(a, u, v, hu, hv);
    check(out.len() == 3 * want.len(), "derivative count")?;
    for (n, w) in want.iter().enumerate() {
        let got = Vec3::new(out[3 * n], out[3 * n + 1], out[3 * n + 2]);
        match w {
            Some((p, b)) => within(&format!("derivative {n}"), (got - *p).length(), *b)?,
            None => check(got == Vec3::ZERO, format!("unwritten derivative {n} not 0"))?,
        }
    }
    Ok(())
}

/// The iso circle certificate: `None` exactly outside the heights by more than `tol`;
/// else a closed curve on the cone in the plane at the clamped height.
fn cert_iso_circle(a: &Analytic, p: Vec3, tol: f64, out: &[f64]) -> Verdict {
    let h = (p - a.o).dot(a.z);
    let bh = gamma(8) * (mc(p) + mc(a.o));
    // (a bounded cone of negative height keeps its heights reversed)
    let (v0, v1) = (a.dom[1].min(a.dom[3]), a.dom[1].max(a.dom[3]));
    let outside = h < v0 - tol - bh || h > v1 + tol + bh;
    let inside = h >= v0 - tol + bh && h <= v1 + tol - bh;
    let mut r = Rd::new(out);
    let curve = read_curve(&mut r);
    check(r.rest() == 0, "trailing values")?;
    match curve {
        None => check(!inside, "no circle where the height is within the domain"),
        Some(c) => {
            check(!outside, "a circle where the height is outside the domain")?;
            let hc = h.clamp(v0, v1);
            let Some(b) = c.bspline() else {
                return fail("circle without NURB");
            };
            check(
                curve_lies_on(&b, &a.implicit()).unwrap_or(false),
                "circle off the cone",
            )?;
            let pts = c.points();
            let reach = pts.iter().fold(0.0_f64, |m, q| m.max(mc(*q)));
            // the height maps onto the form's linear parameter and back through the
            // iso extraction: the map's rounding over the heights, the extraction's
            // over the net's reach
            let delta =
                gamma(64) * (v0.abs().min(1e300) + v1.abs() + h.abs() + reach + mc(a.o)) + bh;
            for q in &pts {
                within(
                    "circle off its height",
                    ((*q - a.o).dot(a.z) - hc).abs(),
                    delta,
                )?;
            }
            if a.dom[0] == 0.0 && a.dom[2] == 360.0 {
                let (f, l) = (pts[0], pts[pts.len() - 1]);
                within(
                    "circle not closed",
                    (f - l).length(),
                    gamma(64) * (reach + mc(a.o)),
                )?;
            }
            let _ = c.ivl;
            Ok(())
        }
    }
}

/// The distance from `p` to the inversion's patch and its bound: `D` and, for a cone, its
/// reflection through the apex. The cone's STEP surface is the double cone (STEP's
/// `conical_surface`, of radius `r0 + v tan(semi)` for every `v`, negative past the apex),
/// and its inversion (`STEPInversion` through `GetPositivePoint`, on every cone alike)
/// mirrors a point on the far nappe onto `D`, inverts it there, and mirrors the height
/// back: its patch is `D` and its reflection, the far nappe's points at heights `2h - v`.
fn inversion_patch_distance(a: &Analytic, p: Vec3) -> Option<(f64, f64)> {
    let (d, b) = a.patch_distance(p)?;
    if a.cylinder() {
        return Some((d, b));
    }
    let (h, bh) = a.apex_h();
    let apex = a.o + h * a.z;
    let m = 2.0 * apex - p;
    // the reflection's rounding: the apex point's (its height's bound along a unit axis,
    // and the sum's) and the difference's
    let bm = 2.0 * (bh + 3.0_f64.sqrt() * gamma(4) * (mc(a.o) + h.abs()))
        + 3.0_f64.sqrt() * gamma(2) * (2.0 * mc(apex) + mc(p));
    let (d2, b2) = a.patch_distance(m)?;
    Some((d.min(d2), b.max(b2 + bm)))
}

/// The inversion certificate on `(inside, uv)`.
fn cert_inversion(a: &Analytic, p: Vec3, tol: f64, answer: Option<(bool, f64, f64)>) -> Verdict {
    let Some((d, b)) = inversion_patch_distance(a, p) else {
        return fail("partial sweep: no closed-form distance");
    };
    match answer {
        None => fail("inversion refused with an error"),
        Some((true, u, v)) => {
            within(
                "inside, but the point is farther than tol from the patch",
                d,
                tol + b,
            )?;
            let [u0, v0, u1, v1] = a.dom;
            let bv = gamma(16) * (v0.abs() + v1.abs());
            let in_d = |v: f64, bv: f64| v >= v0 - bv && v <= v1 + bv;
            // a height on the far nappe is the reflection of one in `D`
            let (h, bh) = a.apex_h();
            let mirrored = !a.cylinder()
                && in_d(
                    2.0 * h - v,
                    bv + 2.0 * bh + gamma(2) * (2.0 * h.abs() + v.abs()),
                );
            check(
                u >= u0 - gamma(16) * 360.0
                    && u <= u1 + gamma(16) * 360.0
                    && (in_d(v, bv) || mirrored),
                format!("inside at ({u}, {v}), outside the domain and its reflection"),
            )?;
            within(
                "inside, but the STEP point is farther than tol",
                (a.point(u, v) - p).length(),
                tol + b + a.point_bound(u, v),
            )
        }
        Some((false, _, _)) => check(
            d >= tol - b,
            format!("not inside, but the point is {d:e} from the patch (tol {tol:e})"),
        ),
    }
}

fn cert_positive(a: &Analytic, p: Vec3, out: &[f64]) -> Verdict {
    check(out.len() == 5, "positive layout")?;
    let negated = out[0] != 0.0;
    let got = Vec3::new(out[1], out[2], out[3]);
    let param = out[4];
    if a.cylinder() {
        check(
            !negated && got == p && param == 0.0,
            "cylinder's positive point",
        )?;
        return Ok(());
    }
    let (h, b) = a.apex_h();
    let apex = a.o + h * a.z;
    let s = (p - apex).dot(a.z);
    let bs = gamma(8) * (mc(p) + mc(a.o) + h.abs()) + b;
    let far = (a.semi > 0.0 && s < 0.0) || (a.semi < 0.0 && s > 0.0);
    if s.abs() > bs {
        check(negated == far, "wrong side of the apex plane")?;
    }
    let want = if negated { 2.0 * apex - p } else { p };
    within(
        "positive point",
        (got - want).length(),
        3.0_f64.sqrt() * gamma(8) * (mc(p) + mc(a.o) + h.abs()) + 2.0 * b,
    )?;
    within(
        "apex parameter",
        (param - h).abs(),
        b + gamma(16) * (mc(a.o) + h.abs()),
    )
}

// ---- rows ---------------------------------------------------------------------------------

struct Row {
    unit: String,
    case: String,
    old: Value,
    new: Value,
}

fn inventory() -> Vec<Row> {
    let path = std::env::var("WR713_INVENTORY").expect("WR713_INVENTORY=<jsonl>");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| {
            let v: Value = serde_json::from_str(l).unwrap();
            Row {
                unit: v["unit"].as_str().unwrap().to_owned(),
                case: v["case"].as_str().unwrap().to_owned(),
                old: v["old"].clone(),
                new: v["new"].clone(),
            }
        })
        .collect()
}

type Values = BTreeMap<String, Vec<f64>>;

fn old_outputs() -> BTreeMap<(String, String), Values> {
    let path = std::env::var("WR713_OUTPUTS").expect("WR713_OUTPUTS=<jsonl>");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| {
            let v: Value = serde_json::from_str(l).unwrap();
            let values = v["values"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, xs)| {
                    let xs = xs
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|h| {
                            f64::from_bits(u64::from_str_radix(h.as_str().unwrap(), 16).unwrap())
                        })
                        .collect();
                    (k.clone(), xs)
                })
                .collect();
            (
                (
                    v["unit"].as_str().unwrap().to_owned(),
                    v["case"].as_str().unwrap().to_owned(),
                ),
                values,
            )
        })
        .collect()
}

/// The old side's `Output`: the old values and the old line's other fields.
fn old_output(new: &Output, line: &Value, values: &Values) -> Output {
    let mut o = new.clone();
    o.values = values.clone();
    o.error = line["error"].as_str().map(str::to_owned);
    o.deferred = None;
    assert!(line["deferred"].is_null());
    o.ints = line["ints"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, xs)| {
            let xs = xs
                .as_array()
                .unwrap_or_else(|| panic!("blocked ints {k}"))
                .iter()
                .map(|x| x.as_i64().unwrap())
                .collect();
            (k.clone(), xs)
        })
        .collect();
    o
}

fn first(vals: &Values, key: &str) -> Option<f64> {
    vals.get(key).and_then(|v| v.first().copied())
}

fn v3(v: &Value) -> Vec3 {
    Vec3::new(
        v[0].as_f64().unwrap(),
        v[1].as_f64().unwrap(),
        v[2].as_f64().unwrap(),
    )
}

/// One side's verdict for a `surface.cone_p1` or common-cone row. `surface_api`: the row
/// runs through `Surface` (`surface.surface_p2`'s common cases): inversion records
/// `[u, v, loc]` after `SUCCESS`, or an error status.
fn cert_cone_row(
    inputs: &Value,
    vals: &Values,
    ctx: Option<&Ctx>,
    surface_api: bool,
    same_angle: bool,
) -> Verdict {
    let build = first(vals, "build").unwrap();
    let cone_in = &inputs["cone"];
    if build != SUCCESS {
        return match degenerate(cone_in) {
            Some(_) => Ok(()),
            None => fail(format!(
                "construction refused ({build}) of a cone the inputs define"
            )),
        };
    }
    if let Some(why) = degenerate(cone_in) {
        return fail(format!("built a cone of degenerate inputs: {why}"));
    }
    let Some(ctx) = ctx else {
        return fail("no new cone to certify against");
    };
    let a = &ctx.a;
    let op = inputs["op"].as_str().unwrap();
    let status = first(vals, "status");
    let out: &[f64] = vals.get("out").map_or(&[], Vec::as_slice);
    // the ops whose answer always exists here: a refusal is wrong
    let ok = |s: Option<f64>| s == Some(SUCCESS);
    match op {
        "build" => {
            let mut r = Rd::new(out);
            let s = read_state(&mut r);
            check(r.rest() == 0, "trailing values")?;
            cert_input_frame(cone_in, &s)?;
            if let Some(semi) = unbounded_semi(cone_in) {
                // the angle is the input's modulo 180, in (-90, 90)
                check(
                    s.semi == semi_rep(semi),
                    format!("semi-angle {} is not {semi} modulo 180", s.semi),
                )?;
            }
            cert_state(&s)
        }
        "copy" | "adjust" | "transform" => {
            check(ok(status), format!("{op} refused ({status:?})"))?;
            let mut r = Rd::new(out);
            if op == "copy" {
                check(r.f() == 1.0, "copy not equal to its source")?;
            }
            let s = read_state(&mut r);
            check(r.rest() == 0, "trailing values")?;
            cert_state(&s)
        }
        "eval_step" => {
            let order = |k: &str| inputs[k].as_u64().unwrap_or(0);
            if !ok(status)
                && !inputs["point"].as_bool().unwrap_or(false)
                && (order("hu") > 3 || order("hv") > 3)
            {
                // the STEP form evaluates derivatives up to order 3 only: a refusal of
                // higher ones is its answer
                return Ok(());
            }
            check(ok(status), format!("eval_step refused ({status:?})"))?;
            cert_eval_step(a, inputs, out)
        }
        "eval_point" | "eval" => {
            check(ok(status), format!("{op} refused ({status:?})"))?;
            let uv = (inputs["u"].as_f64().unwrap(), inputs["v"].as_f64().unwrap());
            let p = Vec3::new(out[0], out[1], out[2]);
            let Ok(step) = ctx
                .surface
                .convert_uv_from_nurbs_to_step(&ctx.k, Point2::new(uv.0, uv.1))
            else {
                // a cone without polar curve has no map to its STEP form: the point is
                // the form's at `uv` (evaluation's definition), and the form lies on `T`
                check(
                    lies_on(&ctx.form, &a.implicit()).unwrap_or(false),
                    "the NURB form is off the cone",
                )?;
                let f = ctx.form.evaluate_point(Point2::new(uv.0, uv.1)).unwrap();
                return within(
                    "point vs the form's point",
                    (p - f).length(),
                    gamma(16) * form_reach(&ctx.form),
                );
            };
            ctx.ratio((step.x, step.y), p)
        }
        "to_step" => {
            check(ok(status), format!("to_step refused ({status:?})"))?;
            let uv = (inputs["u"].as_f64().unwrap(), inputs["v"].as_f64().unwrap());
            let (su, sv) = (out[0], out[1]);
            let d = ctx.form.natural_uv_domain();
            let inside =
                (d.min().x..=d.max().x).contains(&uv.0) && (d.min().y..=d.max().y).contains(&uv.1);
            if inside {
                ctx.in_domain(su, sv)?;
                let f = ctx.form_point(uv);
                let r = ctx.ratio((su, sv), f);
                if r.is_err() && same_angle {
                    // the angle is the polar converter's, unmoved (bit-equal old and
                    // new): the moved height is certified in the meridian plane
                    return ctx.meridian((su, sv), f);
                }
                return r;
            }
            // a parameter outside the form's domain: the maps extend past their ends, and
            // the answer's reading on `D` (clamped, as the STEP form evaluates) is the
            // form's point at the clamped parameter
            let f = ctx.form_point(uv);
            let mut verdict = fail("no reading");
            for s in ctx.step_readings(su, sv) {
                verdict = ctx.ratio(s, f);
                if verdict.is_ok() {
                    break;
                }
            }
            if verdict.is_err() && same_angle {
                // the angle is the polar converter's, unmoved (bit-equal old and new):
                // the moved height is certified in the meridian plane
                return ctx.meridian((su, sv), f);
            }
            verdict
        }
        "to_nurbs" => {
            check(ok(status), format!("to_nurbs refused ({status:?})"))?;
            let f = ctx.form_point((out[0], out[1]));
            let mut verdict = fail("no reading");
            for s in ctx.step_readings(inputs["u"].as_f64().unwrap(), inputs["v"].as_f64().unwrap())
            {
                verdict = ctx.ratio(s, f);
                if verdict.is_ok() {
                    break;
                }
            }
            verdict
        }
        "iso" => {
            if !ok(status) {
                // a refusal is right exactly when the requested curve has no point: the
                // iso value outside the form's domain (and the requested one), or the
                // requested range across it disjoint from the form's
                let d = ctx.form.natural_uv_domain();
                let along_u = inputs["param"].as_u64() == Some(0);
                let iso = inputs["iso"].as_f64().unwrap();
                let (p_lo, p_hi, q_lo, q_hi) = if along_u {
                    (d.min().x, d.max().x, d.min().y, d.max().y)
                } else {
                    (d.min().y, d.max().y, d.min().x, d.max().x)
                };
                let (mut p_lo, mut p_hi, mut q_lo, mut q_hi) = (p_lo, p_hi, q_lo, q_hi);
                if let Some(r) = inputs["dom"].as_array() {
                    let r: Vec<f64> = r.iter().map(|x| x.as_f64().unwrap()).collect();
                    let (rp, rq) = if along_u {
                        ((r[0], r[2]), (r[1], r[3]))
                    } else {
                        ((r[1], r[3]), (r[0], r[2]))
                    };
                    p_lo = p_lo.max(rp.0.min(rp.1));
                    p_hi = p_hi.min(rp.0.max(rp.1));
                    q_lo = q_lo.max(rq.0.min(rq.1));
                    q_hi = q_hi.min(rq.0.max(rq.1));
                }
                return check(
                    iso < p_lo || iso > p_hi || q_lo >= q_hi,
                    format!("iso refused ({status:?}) where the curve exists"),
                );
            }
            let mut r = Rd::new(&out[1..]);
            let c = read_curve(&mut r);
            let _uv = read_curve(&mut r);
            check(r.rest() == 0, "trailing values")?;
            let Some(b) = c.as_ref().and_then(CurveData::bspline) else {
                return fail("iso curve without NURB");
            };
            check(
                curve_lies_on(&b, &a.implicit()).unwrap_or(false),
                "iso curve off the cone",
            )
        }
        "iso_circle" => {
            check(ok(status), format!("iso_circle refused ({status:?})"))?;
            cert_iso_circle(a, v3(&inputs["p"]), inputs["tol"].as_f64().unwrap(), out)
        }
        "inversion" => {
            let p = v3(&inputs["p"]);
            let tol = inputs["tol"].as_f64().unwrap();
            let answer = if surface_api {
                match status {
                    Some(SUCCESS) => Some((true, out[0], out[1])),
                    Some(SM_ERR | OUTSIDE_OF_DOMAIN | NOT_WITHIN_TOLERANCE) => {
                        Some((false, f64::NAN, f64::NAN))
                    }
                    _ => None,
                }
            } else {
                match status {
                    Some(SUCCESS) => Some((true, out[0], out[1])),
                    Some(SM_ERR) if !out.is_empty() => Some((false, out[0], out[1])),
                    // `SM_ERR` without outputs: the rotated point is on no generator, the
                    // same "not inside" answer (the surface API reports it as `SM_ERR`);
                    // the distance decides whether it is right
                    Some(SM_ERR | OUTSIDE_OF_DOMAIN | NOT_WITHIN_TOLERANCE) => {
                        Some((false, f64::NAN, f64::NAN))
                    }
                    _ => None,
                }
            };
            cert_inversion(a, p, tol, answer)
        }
        "positive" => {
            check(ok(status), format!("positive refused ({status:?})"))?;
            cert_positive(a, v3(&inputs["p"]), out)
        }
        _ => fail(format!("no certificate for op {op}")),
    }
}

/// The new cone a cone_p1-shaped `inputs` builds, with its form; `None` when it does not
/// build.
fn ctx_of(inputs: &Value) -> Option<Ctx> {
    let k = Kernel::default();
    let cone = goldens::units::surface_cone_p1::case_cone(&k, inputs)
        .ok()?
        .ok()?;
    let form = cone.rev().base().clone();
    form.gw_nurb()?;
    Some(Ctx {
        a: Analytic::of_cone(&cone),
        surface: Surface::from(cone),
        form,
        k,
    })
}

/// The class of a moved row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    Closer,
    Equal,
    Farther,
}

#[test]
#[ignore = "needs WR713_INVENTORY and WR713_OUTPUTS; see the module docs"]
fn moved_cone_rows_are_proven() {
    let rows = inventory();
    let old_out = old_outputs();
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    let mut wanted: BTreeMap<String, BTreeMap<String, &Row>> = BTreeMap::new();
    for r in &rows {
        assert!(
            wanted
                .entry(r.unit.clone())
                .or_default()
                .insert(r.case.clone(), r)
                .is_none(),
            "row twice: {} {}",
            r.unit,
            r.case
        );
    }
    let mut seen = BTreeSet::new();
    let mut counts: BTreeMap<(String, String, Class), usize> = BTreeMap::new();
    let mut failures = Vec::new();
    let mut closest: BTreeMap<String, (f64, String)> = BTreeMap::new();
    let mut ratios: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut axis_rows: Vec<String> = Vec::new();
    let mut report = String::new();
    for unit in registry() {
        let Some(cases) = wanted.get(unit.id()) else {
            continue;
        };
        let gold = golden::read(&golden::default_dir(), unit.id()).unwrap();
        for case in unit.cases(&ctx).unwrap() {
            let Some(row) = cases.get(&case.name) else {
                continue;
            };
            seen.insert((unit.id().to_owned(), case.name.clone()));
            let new = unit.run_rust(&case).unwrap();
            let floors = unit.abs_floors();
            let enc = golden::encode(&case.name, &new);
            let d = golden::diff(&row.new, &enc, floors);
            assert!(
                d.is_none(),
                "{} {}: re-run is not the inventory's new line: {d:?}",
                unit.id(),
                case.name
            );
            let g = gold.get(&case.name).expect("a golden line");
            assert!(
                golden::diff(g, &enc, floors).is_none(),
                "{} {}: not the golden line",
                unit.id(),
                case.name
            );
            let ov = &old_out[&(unit.id().to_owned(), case.name.clone())];
            let old = old_output(&new, &row.old, ov);
            let d = golden::diff(&row.old, &golden::encode(&case.name, &old), floors);
            assert!(
                d.is_none(),
                "{} {}: old outputs are not the inventory's old line: {d:?}",
                unit.id(),
                case.name
            );
            let logs = coons_logs(unit.id(), &case, &new);
            let (op, vn, vo) = if is_axis_scaled_row(unit.id(), &case.name) {
                let (op, vn, vo, bits) = cert_rescaled(unit.as_ref(), &case, &new, &old);
                axis_rows.push(format!(
                    "{} {} ({})",
                    unit.id(),
                    case.name,
                    if bits {
                        "bit for bit"
                    } else {
                        "within the goldens' tolerance"
                    }
                ));
                (op, vn, vo)
            } else {
                certify(unit.id(), &case.inputs, &new, &old, &logs)
            };
            if std::env::var("WR713_SHOW").is_ok_and(|w| w.split(',').any(|w| w == case.name)) {
                println!(
                    "SHOW {} {}\n  new {:?} {:?}\n  old {:?} {:?}",
                    unit.id(),
                    case.name,
                    new.error,
                    new.values,
                    old.error,
                    old.values
                );
            }
            // an old value off by less than `CLOSER_MARGIN` times its bound fails only by
            // the bound's constant (`γ_n` with an `n` stated, not derived for the old
            // path): EQUAL, not CLOSER (review r3 M6)
            let class = match (&vn, &vo) {
                (Err(_), _) => Class::Farther,
                (Ok(()), Ok(())) => Class::Equal,
                (Ok(()), Err(f)) if f.ratio.is_some_and(|r| r < CLOSER_MARGIN) => Class::Equal,
                (Ok(()), Err(_)) => Class::Closer,
            };
            if let (Ok(()), Err(f)) = (&vn, &vo) {
                if let Some(r) = f.ratio {
                    ratios.entry(op.clone()).or_default().push(r);
                }
            }
            if let Err(f) = &vn {
                let o = match &vo {
                    Ok(()) => "old passes".to_owned(),
                    Err(g) => format!("old fails: {}", g.why),
                };
                failures.push(format!(
                    "{} {} [{op}]: new fails: {} || {o}",
                    unit.id(),
                    case.name,
                    f.why
                ));
            }
            if let (Ok(()), Err(f), Class::Closer) = (&vn, &vo, class) {
                let r = f.ratio.unwrap_or(f64::INFINITY);
                let e = closest
                    .entry(op.clone())
                    .or_insert((f64::INFINITY, String::new()));
                if r < e.0 {
                    *e = (r, format!("{} ({})", case.name, f.why));
                }
            }
            *counts.entry((unit.id().to_owned(), op, class)).or_default() += 1;
        }
    }
    for r in &rows {
        assert!(
            seen.contains(&(r.unit.clone(), r.case.clone())),
            "{} {}: no such case",
            r.unit,
            r.case
        );
    }
    for ((u, op, c), n) in &counts {
        writeln!(report, "{n:5}  {u:32} {op:24} {c:?}").unwrap();
    }
    writeln!(report, "closest CLOSER rows (old error / bound):").unwrap();
    for (op, (r, w)) in &closest {
        writeln!(report, "  {op:24} {r:.3e}  {w}").unwrap();
    }
    writeln!(
        report,
        "old error / bound where the old value fails by a bound (EQUAL below {CLOSER_MARGIN}):"
    )
    .unwrap();
    let edges = [1.0, 10.0, 1e2, 1e4, 1e8, f64::INFINITY];
    for (op, rs) in &ratios {
        let mut line = format!("  {op:24}");
        let mut lo = 0.0;
        for hi in edges {
            let n = rs.iter().filter(|r| **r >= lo && **r < hi).count();
            write!(line, " [{lo:.0e},{hi:.0e}):{n}").unwrap();
            lo = hi;
        }
        let n = rs.iter().filter(|r| !r.is_finite()).count();
        write!(line, " inf/nan:{n}").unwrap();
        writeln!(report, "{line}").unwrap();
    }
    writeln!(
        report,
        "axis rows certified on inputs rescaled by powers of two ({}):",
        axis_rows.len()
    )
    .unwrap();
    for r in &axis_rows {
        writeln!(report, "  {r}").unwrap();
    }
    assert_eq!(
        axis_rows.len(),
        AXIS_SCALED_ROWS.len(),
        "an axis row is missing"
    );
    println!("{report}");
    for f in &failures {
        println!("FARTHER {f}");
    }
    assert!(failures.is_empty(), "{} rows FARTHER", failures.len());
}

/// An old value off its certified answer by less than this many times the bound fails only
/// by the bound's constant: the `γ_n` bounds state an `n` above the operation count of the
/// path they were written for, with room, not derived for main's path. Such a row is
/// EQUAL, not CLOSER.
const CLOSER_MARGIN: f64 = 10.0;

/// The rows whose inputs hold an axis that upstream's unitize cannot read (a coordinate of
/// `1e-300` or `1e300`, or `5e-13`, below its threshold): certified on the same inputs with
/// each axis rescaled by a power of two ([`cert_rescaled`]).
const AXIS_SCALED_ROWS: [(&str, &str); 32] = [
    (
        "surface.cone_p1",
        "height_without_polar_tiny_axis_swap0_inside1",
    ),
    (
        "surface.cone_p1",
        "height_without_polar_tiny_axis_swap1_inside1",
    ),
    ("surface.cone_p2", "nopolar_bounded_off_pos"),
    ("surface.cone_p2", "nopolar_bounded_off_trunc"),
    ("surface.cone_p2", "nopolar_bounded_rebuild"),
    ("surface.cone_p2", "nopolar_bounded_recognize"),
    ("surface.cone_p2", "nopolar_bounded_trim"),
    ("surface.cone_p2", "nopolar_bounded_y_trim_o0_full"),
    ("surface.cone_p2", "nopolar_bounded_apex_off_pos"),
    ("surface.cone_p2", "nopolar_bounded_apex_off_trunc"),
    ("surface.cone_p2", "nopolar_bounded_apex_rebuild"),
    ("surface.cone_p2", "nopolar_bounded_apex_recognize"),
    ("surface.cone_p2", "nopolar_bounded_apex_trim"),
    ("surface.cone_p2", "nopolar_bounded_apex_y_trim_o0_full"),
    ("surface.cone_p2", "nopolar_bounded_cone_rebuild"),
    ("surface.cone_p2", "nopolar_bounded_cone_recognize"),
    ("surface.cone_p2", "nopolar_bounded_cone_trim"),
    ("surface.cone_p2", "nopolar_bounded_cone_x_trim_oneg_full"),
    ("surface.cone_p3", "rejected_tiny_axis_0_cone"),
    ("surface.cone_p3", "rejected_tiny_axis_0_other"),
    ("surface.cone_p3", "rejected_tiny_axis_3_cone"),
    ("surface.cone_p3", "rejected_tiny_axis_3_other"),
    ("surface.cone_p3", "rejected_tiny_axis_cylinders_cone"),
    ("surface.cone_p3", "rejected_tiny_axis_cylinders_other"),
    ("surface.cone_p3", "rejected_below_unitize_threshold_axis"),
    ("surface.cone_p3", "fuzz_axis_overflow_zero_y"),
    ("surface.cone_p3", "fuzz_axis_overflow_zero_y_swapped"),
    ("surface.cone_p3", "fuzz_axis_overflow_zero_y_trimmed"),
    ("surface.cone_p3", "fuzz_height_06674"),
    ("surface.cone_p3", "fuzz_height_06289"),
    ("surface.cone_p3", "missing_polar_height_plane_1_y"),
    ("surface.cone_p3", "missing_polar_height_plane_2_y"),
];

fn is_axis_scaled_row(unit: &str, case: &str) -> bool {
    AXIS_SCALED_ROWS
        .iter()
        .any(|(u, c)| *u == unit && *c == case)
}

/// The power of two that brings the largest absolute coordinate of `v` into `[1, 2)`;
/// `1` for a zero or non-finite vector. Each step is a multiplication by 2 or 1/2, exact
/// in the ranges reached (the coordinates stay normal or are zero).
fn pow2_scale(v: &[f64]) -> f64 {
    let m = v.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
    if m == 0.0 || !m.is_finite() {
        return 1.0;
    }
    let mut s = 1.0;
    while m * s < 1.0 {
        s *= 2.0;
    }
    while m * s >= 2.0 {
        s *= 0.5;
    }
    s
}

/// `v` times `s`, each product checked exact (it divides back to `v`).
fn scaled(v: &[f64], s: f64) -> Vec<f64> {
    v.iter()
        .map(|&x| {
            let y = x * s;
            assert!(y / s == x, "{x} * {s} is not exact");
            y
        })
        .collect()
}

/// The row's inputs with each cone or cylinder axis (`X` and `Y`) scaled by
/// [`pow2_scale`]: the JSON `x` and `y` of `cone_p1`/`cone_p2` and the entries 3..9 of each
/// 17-entry cone array of `cone_p3` (a 13-entry plane is left as it is). `None` where no
/// axis changes.
fn rescaled_inputs(unit: &str, inputs: &Value) -> Option<Value> {
    let mut out = inputs.clone();
    let mut changed = false;
    let fix = |arr: &mut Value| -> bool {
        let v: Vec<f64> = arr
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap())
            .collect();
        let s = pow2_scale(&v);
        if s != 1.0 {
            *arr = serde_json::json!(scaled(&v, s));
        }
        s != 1.0
    };
    match unit {
        "surface.cone_p1" | "surface.cone_p2" => {
            changed |= fix(&mut out["cone"]["x"]);
            changed |= fix(&mut out["cone"]["y"]);
        }
        "surface.cone_p3" => {
            for key in ["cone", "other"] {
                let a: Vec<f64> = out[key]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_f64().unwrap())
                    .collect();
                if a.len() != 17 {
                    continue;
                }
                let mut b = a.clone();
                for range in [3..6, 6..9] {
                    let s = pow2_scale(&a[range.clone()]);
                    if s != 1.0 {
                        changed = true;
                        b.splice(range.clone(), scaled(&a[range], s));
                    }
                }
                out[key] = serde_json::json!(b);
            }
        }
        _ => return None,
    }
    changed.then_some(out)
}

/// The certificate of an axis row (review r3 B1). The cone `(O, X, Y, r, s)` defines is
/// the same for `X -> λ X` and `Y -> μ Y` (`λ, μ > 0`): upstream's definition unitizes both
/// axes, and so does its scale-invariant reading, in exact arithmetic. Only the rounding
/// of `|v|^2` (its underflow or overflow, or the unitize threshold) tells the two apart.
/// Scaling by a power of two is exact, so the rescaled inputs ([`rescaled_inputs`]) are
/// the same cone given with ordinary axes, which upstream's own unitize reads. The row's
/// new value must equal the rescaled run's (bit for bit, else within the goldens'
/// comparison), and that run is certified with the ordinary certificate ([`certify`])
/// against the rescaled inputs; the old value is certified against them too. Returns the
/// op, the new and old verdicts, and whether the two runs agree bit for bit.
fn cert_rescaled(
    unit: &dyn goldens::UnitDiff,
    case: &goldens::case::Case,
    new: &Output,
    old: &Output,
) -> (String, Verdict, Verdict, bool) {
    let inputs = rescaled_inputs(unit.id(), &case.inputs)
        .unwrap_or_else(|| panic!("{}: no axis to rescale", case.name));
    let scaled_case = goldens::case::Case {
        inputs: inputs.clone(),
        ..case.clone()
    };
    let out = unit.run_rust(&scaled_case).unwrap();
    let (op, vr, vo) = certify(unit.id(), &inputs, &out, old, &[]);
    let enc = |o: &Output| golden::encode(&case.name, o);
    let bits = enc(new) == enc(&out)
        && new.values.iter().all(|(k, v)| {
            out.values.get(k).is_some_and(|w| {
                v.len() == w.len() && v.iter().zip(w).all(|(a, b)| a.to_bits() == b.to_bits())
            })
        })
        && new.values.len() == out.values.len()
        && new.error == out.error;
    let same = bits || golden::diff(&enc(&out), &enc(new), unit.abs_floors()).is_none();
    let vn = vr.and_then(|()| check(same, "the row's value is not the rescaled inputs' value"));
    (format!("{op} (axes rescaled)"), vn, vo, bits)
}

/// The op label and the new and old verdicts of one row.
/// A state row's identity (type, strings, kind flags, radius, angle, flags, placement) as
/// bits, where the side built and the op answered.
fn state_identity(inputs: &Value, o: &Output) -> Option<Vec<u64>> {
    let op = inputs["op"].as_str()?;
    if o.error.is_some() || first(&o.values, "build") != Some(SUCCESS) {
        return None;
    }
    // a build row's frame is certified against the inputs' frame instead
    // ([`cert_input_frame`])
    match op {
        "copy" | "adjust" | "transform" if first(&o.values, "status") == Some(SUCCESS) => {}
        _ => return None,
    }
    let mut r = Rd::new(o.values.get("out")?);
    if op == "copy" {
        r.f();
    }
    let s = read_state(&mut r);
    Some(s.identity.iter().map(|x| x.to_bits()).collect())
}

/// A built cone's placement has the directions the inputs give: `X` along the input `X`,
/// `Y` along the part of the input `Y` across it (Gram-Schmidt), each axis first scaled by
/// its largest coordinate so tiny and huge axes are directions. The canonical modes take
/// the axes `Axis2Placement::new` holds, as the unit passes them. The bound is the
/// rounding of the two normalisations, amplified by `1 / sin` of the inputs' angle.
fn cert_input_frame(cone: &Value, s: &State) -> Verdict {
    let held = matches!(
        cone["mode"]["kind"].as_str(),
        Some("canonical_semi" | "canonical" | "cyl_canonical")
    );
    let (x, y) = if held {
        let p = Axis2Placement::new(v3(&cone["origin"]), v3(&cone["x"]), v3(&cone["y"]));
        (p.x_axis(), p.y_axis())
    } else {
        (v3(&cone["x"]), v3(&cone["y"]))
    };
    let unit = |v: Vec3| {
        let w = v * (1.0 / mc(v));
        w * (1.0 / w.length())
    };
    let xd = unit(x);
    let yu = unit(y);
    let across = yu - yu.dot(xd) * xd;
    let sin = across.length();
    let yd = across * (1.0 / sin);
    let b = gamma(64) / sin;
    within(
        "placement X is not along the inputs' X",
        (unit(s.x) - xd).length(),
        b,
    )?;
    within(
        "placement Y is not along the inputs' Y across X",
        (unit(s.y) - yd).length(),
        b,
    )
}

/// The header's premise: where both sides answer a state row, the cone's analytic data are
/// bit-equal old and new, so both sides are certified against the same cone `T`.
fn assert_same_identity(inputs: &Value, new: &Output, old: &Output) {
    if let (Some(n), Some(o)) = (state_identity(inputs, new), state_identity(inputs, old)) {
        assert_eq!(n, o, "state identity moved: {inputs}");
    }
}

fn certify(
    unit: &str,
    inputs: &Value,
    new: &Output,
    old: &Output,
    logs: &[CoonsTangencyLog],
) -> (String, Verdict, Verdict) {
    match unit {
        "surface.cone_p1" => {
            let op = inputs["op"].as_str().unwrap_or("?").to_owned();
            assert_same_identity(inputs, new, old);
            let ctx = ctx_of(inputs);
            (
                op,
                side(inputs, new, ctx.as_ref(), false, same_angle(new, old)),
                side(inputs, old, ctx.as_ref(), false, same_angle(new, old)),
            )
        }
        "surface.surface_p2"
            if inputs.get("common_cone").is_some() && inputs.get("common_const_make").is_none() =>
        {
            let i = &inputs["common_cone"];
            let mut op = i["op"].as_str().unwrap_or("?").to_owned();
            if inputs.get("common_subdomain").is_some() {
                op.push_str(" (subdomain)");
            }
            assert_same_identity(i, new, old);
            let ctx = ctx_of(i);
            let sa = same_angle(new, old);
            (
                op,
                side(i, new, ctx.as_ref(), true, sa),
                side(i, old, ctx.as_ref(), true, sa),
            )
        }
        "surface.surface_p2" if inputs.get("common_const_make").is_some() => {
            let i = &inputs["common_cone"];
            let ctx = ctx_of(i);
            let side = |o: &Output| match (&o.error, &ctx) {
                (Some(e), _) => fail(format!("error {e}")),
                (None, None) => fail("no new cone to certify against"),
                (None, Some(c)) => cert_const_make(c, &o.values),
            };
            ("const_make".into(), side(new), side(old))
        }
        "surface.cone_p2" => {
            let op = format!("p2 {}", inputs["op"]["kind"].as_str().unwrap_or("?"));
            let side = |o: &Output| match &o.error {
                Some(e) => fail(format!("error {e}")),
                None => cert_p2_row(inputs, &o.values),
            };
            (op, side(new), side(old))
        }
        "intersect.api_intersectors" if inputs["op"] == "surfaces" => {
            let side = |o: &Output| match &o.error {
                Some(e) => fail(format!("error {e}")),
                None => cert_sphere_cone(inputs, &o.values),
            };
            ("sphere_cone".into(), side(new), side(old))
        }
        "primitive.api_primitives" if inputs["op"] == "cylinder_surface" => {
            // args: origin x, y, z, radius, height, inside-out. An origin with a NaN or
            // infinite coordinate places no cylinder: the right answer is a refusal, and
            // any refusal status is one (1001 `SM_ERR` old, 1007 `SM_ERR_INVALID_INPUT`
            // new, the more specific one).
            let finite = inputs["args"].as_array().map(|a| {
                a.iter()
                    .take(3)
                    .all(|x| x.as_f64().is_some_and(f64::is_finite))
            });
            let side = |o: &Output| {
                check(finite == Some(false), "the origin is finite")?;
                check(o.error.is_none(), "the runner failed")?;
                check(
                    first(&o.values, "rec").is_some_and(|r| r != SUCCESS),
                    "a cylinder was built",
                )
            };
            ("cylinder_surface".into(), side(new), side(old))
        }
        "surface.cone_p3" => {
            let side = |o: &Output| match &o.error {
                Some(e) => fail(format!("error {e}")),
                None => cert_p3_row(inputs, o),
            };
            ("p3 intersection".into(), side(new), side(old))
        }
        "nurbs.b_spline_surface_n_lib_p2" => {
            let side = |o: &Output| match &o.error {
                Some(e) => fail(format!("error {e}")),
                None => cert_corner_sides(inputs, o),
            };
            // the new side is also judged on why it moved: its inputs are points of the
            // true cylinder, where main's were not ([`cert_corner_inputs`]), or else the
            // refinement's rule chose another copy: it replays with honest bounds, and
            // the new copy is not certainly worse at the stations than main's
            // ([`cert_coons_against_main`]); G1 decides the class
            let (g1_new, g1_old) = cert_corner_g1(inputs, new, old);
            let (op, why) = match cert_corner_inputs(inputs) {
                Ok(()) => ("corner blend sides and G1", Ok(())),
                Err(_) => (
                    "corner blend sides and G1 (tie rule)",
                    cert_coons_against_main(logs, new, old).0,
                ),
            };
            let new_side = why.and_then(|()| side(new)).and(g1_new);
            (op.into(), new_side, side(old).and(g1_old))
        }
        "topology.brep_p1" if !logs.is_empty() => {
            let (vn, vo) = cert_coons_against_main(logs, new, old);
            ("corner blend (Coons replay, stations)".into(), vn, vo)
        }
        "surface.surf_of_extrusion_p2" => {
            let side = |o: &Output, other: &Output| match &o.error {
                Some(e) => fail(format!("error {e}")),
                None => cert_extrusion_cone(inputs, o, other),
            };
            (
                "extrusion/cone curves".into(),
                side(new, old),
                side(old, new),
            )
        }
        _ => (
            "unhandled".into(),
            fail("no certificate for this unit's rows"),
            fail("-"),
        ),
    }
}

// ---- nurbs.b_spline_surface_n_lib_p2: corner blends on a bounded cylinder ---------------

/// The blossom of the Bezier `b` at `ts` (`ts.len()` = its degree).
fn blossom(b: &[Vec3], ts: &[f64]) -> Vec3 {
    let mut w = b.to_vec();
    for (r, t) in ts.iter().enumerate() {
        for i in 0..w.len() - 1 - r {
            w[i] = (1.0 - t) * w[i] + *t * w[i + 1];
        }
    }
    w[0]
}

/// The Bezier `b` over `[s0, s1]` (its control points: blossoms of `s0` and `s1`).
fn bezier_piece(b: &[Vec3], s0: f64, s1: f64) -> Vec<Vec3> {
    let d = b.len() - 1;
    (0..=d)
        .map(|k| {
            let ts: Vec<f64> = (0..d).map(|i| if i < d - k { s0 } else { s1 }).collect();
            blossom(b, &ts)
        })
        .collect()
}

/// The unit roundoff.
const UNIT_RO: f64 = f64::EPSILON / 2.0;
/// The factor that covers an error bound's own rounding: each bound below is a sum of
/// at most 8 positive terms and products, computed with relative error under `γ_16`.
fn err_up(e: f64) -> f64 {
    e * (1.0 + gamma(16))
}

/// A computed value and a bound on its distance from the exact value (forward error
/// analysis: each operation adds the propagated input errors and its own rounding,
/// `u |result|`).
#[derive(Clone, Copy, Debug)]
struct Rv {
    v: f64,
    e: f64,
}

impl Rv {
    fn add(self, q: Self) -> Self {
        let v = self.v + q.v;
        Rv {
            v,
            e: err_up(self.e + q.e + UNIT_RO * v.abs()),
        }
    }
    fn sub(self, q: Self) -> Self {
        let v = self.v - q.v;
        Rv {
            v,
            e: err_up(self.e + q.e + UNIT_RO * v.abs()),
        }
    }
    fn mul(self, q: Self) -> Self {
        let v = self.v * q.v;
        Rv {
            v,
            e: err_up(self.v.abs() * q.e + q.v.abs() * self.e + self.e * q.e + UNIT_RO * v.abs()),
        }
    }
    /// times a positive weight computed within `γ_64` of its value
    fn weigh(self, w: f64) -> Self {
        let v = self.v * w;
        Rv {
            v,
            e: err_up(self.e * w * (1.0 + gamma(64)) + v.abs() * gamma(66)),
        }
    }
    fn half_sum(self, q: Self) -> Self {
        let v = (self.v + q.v) * 0.5;
        Rv {
            v,
            e: err_up((self.e + q.e) * 0.5 + UNIT_RO * v.abs()),
        }
    }
}

/// A vector of [`Rv`].
#[derive(Clone, Copy, Debug)]
struct R3 {
    x: Rv,
    y: Rv,
    z: Rv,
}

impl R3 {
    fn new(v: Vec3, e: f64) -> Self {
        R3 {
            x: Rv { v: v.x, e },
            y: Rv { v: v.y, e },
            z: Rv { v: v.z, e },
        }
    }
    fn map(self, q: Self, f: impl Fn(Rv, Rv) -> Rv) -> Self {
        R3 {
            x: f(self.x, q.x),
            y: f(self.y, q.y),
            z: f(self.z, q.z),
        }
    }
    fn add(self, q: Self) -> Self {
        self.map(q, Rv::add)
    }
    fn sub(self, q: Self) -> Self {
        self.map(q, Rv::sub)
    }
    fn times(self, c: Rv) -> Self {
        R3 {
            x: self.x.mul(c),
            y: self.y.mul(c),
            z: self.z.mul(c),
        }
    }
    fn weigh(self, w: f64) -> Self {
        R3 {
            x: self.x.weigh(w),
            y: self.y.weigh(w),
            z: self.z.weigh(w),
        }
    }
    fn dot(self, q: Self) -> Rv {
        self.x.mul(q.x).add(self.y.mul(q.y)).add(self.z.mul(q.z))
    }
    fn cross(self, q: Self) -> Self {
        R3 {
            x: self.y.mul(q.z).sub(self.z.mul(q.y)),
            y: self.z.mul(q.x).sub(self.x.mul(q.z)),
            z: self.x.mul(q.y).sub(self.y.mul(q.x)),
        }
    }
}

/// The Bernstein coefficients of the product of two Bernstein polynomials (degrees
/// `m`, `n`): `c_k = sum C(m, i) C(n, k - i) / C(m + n, k) f(a_i, b_{k-i})`, positive
/// weights (each computed in under 64 operations, so within `γ_64`).
fn bern_product<A: Copy, B: Copy, C: Copy>(
    a: &[A],
    b: &[B],
    f: impl Fn(A, B) -> C,
    weigh: impl Fn(C, f64) -> C,
    add: impl Fn(C, C) -> C,
) -> Vec<C> {
    let (m, n) = (a.len() - 1, b.len() - 1);
    let choose = |n: usize, k: usize| -> f64 {
        (0..k).fold(1.0, |c, i| c * (n - i) as f64 / (i + 1) as f64)
    };
    (0..=m + n)
        .map(|k| {
            (k.saturating_sub(n)..=m.min(k))
                .map(|i| {
                    let w = choose(m, i) * choose(n, k - i) / choose(m + n, k);
                    weigh(f(a[i], b[k - i]), w)
                })
                .reduce(&add)
                .unwrap()
        })
        .collect()
}

/// De Casteljau halves of Bernstein coefficients.
fn halves(c: &[Rv]) -> (Vec<Rv>, Vec<Rv>) {
    let mut w = c.to_vec();
    let (mut l, mut r) = (vec![w[0]], vec![w[w.len() - 1]]);
    while w.len() > 1 {
        w = w.windows(2).map(|p| p[0].half_sum(p[1])).collect();
        l.push(w[0]);
        r.push(w[w.len() - 1]);
    }
    r.reverse();
    (l, r)
}

/// The halving depth past which the G1 bound has not converged (a failure, not a bound).
const G1_DEPTH: usize = 60;

/// The analytic neighbour of a corner blend, as `analytic_of` in the unit builds it: `0`
/// the cone from radius `p0` at `origin` to `p1` at height `p2` along `Z`, `1` the
/// cylinder of radius `p0` about `Z`, `2` the sphere of radius `p0` about `origin`, `3`
/// the torus of major radius `p0` and minor `p1` about `Z`.
struct Neighbour {
    kind: u64,
    z: R3,
    p: [f64; 3],
}

impl Neighbour {
    fn of(a: &Value) -> Result<Self, Fail> {
        let z = v3(&a["x"]).cross(v3(&a["y"]));
        let z = z * (1.0 / z.length());
        let kind = a["kind"].as_u64().unwrap();
        let p = [0, 1, 2].map(|i| a["p"][i].as_f64().unwrap());
        check(kind <= 3, "no closed form: an unknown analytic kind")?;
        check(
            kind != 0 || p[2] > 0.0,
            "no closed form: an unbounded cone (height not above 0)",
        )?;
        Ok(Neighbour {
            kind,
            z: R3::new(z, gamma(16)),
            p,
        })
    }
}

/// The Bernstein coefficients of the neighbour's normal direction over a segment, from
/// those of `d = P - origin` (degree 1 in `d` for the cone, cylinder and sphere, an affine
/// map of each coefficient; degree 3 for the torus, by products): the gradient of the
/// implicit equation, halved or quartered. With `ρ = d - (d·Z) Z` and `h = d·Z`:
/// cylinder `|ρ|^2 - r^2`: `ρ`; sphere `|d|^2 - r^2`: `d`; cone `|ρ|^2 - (r0 + t h)^2`
/// with `t = (r1 - r0) / height`: `ρ - t (r0 + t h) Z`; torus `(|d|^2 + R^2 - r^2)^2 - 4
/// R^2 |ρ|^2`: `(|d|^2 + R^2 - r^2) d - 2 R^2 ρ`.
fn reference_normal(nb: &Neighbour, bp: &[R3]) -> Vec<R3> {
    let zr = nb.z;
    let exact = |v: f64| Rv { v, e: 0.0 };
    let rho = |d: &R3| d.sub(zr.times(d.dot(zr)));
    match nb.kind {
        1 => bp.iter().map(rho).collect(),
        2 => bp.to_vec(),
        0 => {
            let tv = (nb.p[1] - nb.p[0]) / nb.p[2];
            let t = Rv {
                v: tv,
                e: gamma(2) * tv.abs(),
            };
            let r0 = exact(nb.p[0]);
            bp.iter()
                .map(|d| rho(d).sub(zr.times(t.mul(r0.add(t.mul(d.dot(zr)))))))
                .collect()
        }
        _ => {
            let (big, small) = (exact(nb.p[0]), exact(nb.p[1]));
            let r2 = big.mul(big);
            let k = r2.sub(small.mul(small));
            let dd = bern_product(bp, bp, R3::dot, Rv::weigh, Rv::add);
            let k0: Vec<Rv> = dd.iter().map(|c| c.add(k)).collect();
            let kd = bern_product(&k0, bp, |a: Rv, b: R3| b.times(a), R3::weigh, R3::add);
            let rhos: Vec<R3> = bp.iter().map(rho).collect();
            let ones = [exact(1.0); 3];
            let rho3 = bern_product(&rhos, &ones, R3::times, R3::weigh, R3::add);
            let two_r2 = r2.add(r2);
            kd.iter()
                .zip(&rho3)
                .map(|(a, b)| a.sub(b.times(two_r2)))
                .collect()
        }
    }
}

/// The certified interval `[lo, hi]` of `max sin θ` over the patch sides that lie on a loop
/// curve with a neighbour, `θ` the angle between the patch normal `N = ∂S/∂u x ∂S/∂v`
/// and the neighbour's normal at the side point `P`, the gradient `R` of its implicit
/// equation ([`reference_normal`]: the data's cylinder, cone, sphere or torus about
/// `origin` and the unit `Z` of `X x Y`). On a side (clamped knots) the cross derivative
/// is a positive multiple of `Σ (P_1j - P_0j) N_j`, so on each Bézier segment `N`, `R`,
/// `f = |N x R|^2` and `g = |N|^2 |R|^2` are Bernstein polynomials and `sin^2 θ = f / g`.
/// Upper: `max f_k / min g_k` over a piece (the convex hull), halving pieces until the
/// bound is within `1e-9` of the best lower; lower: `f / g` at the pieces' ends (exact
/// values of the polynomials). Each value carries a forward error bound ([`Rv`]) with
/// the rounding of every operation charged, and of `Z` (`γ_16` a component from the exact
/// unit axis). No point is sampled: every value is a bound.
///
/// This is the sup-norm of the angle over each side, the G1 measure. Where it sits at a
/// patch corner it is fixed by the loop curves both runs share, so it is EQUAL there by
/// construction (corner-saturated), and says nothing about the rest of the side.
fn corner_g1(inputs: &Value, out: &Output) -> Result<(f64, f64), Fail> {
    let srfs = inputs["srfs"].as_array().unwrap();
    let a = srfs.iter().find_map(|s| s.get("analytic")).unwrap();
    let o = v3(&a["origin"]);
    let neighbour = Neighbour::of(a)?;
    let curves: Vec<Vec<Vec3>> = inputs["curves"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["cpts"].as_array().unwrap().iter().map(v3).collect())
        .collect();
    let size = curves.iter().flatten().fold(0.0f64, |m, p| m.max(mc(*p)));
    let bound = gamma(256) * size;
    let halves_of_loop = curves.len() > 4;
    let ends: Vec<(usize, Vec3, Vec3)> = (0..curves.len())
        .flat_map(|i| {
            let c = &curves[i];
            if halves_of_loop {
                let m = blossom(c, &[0.5; 3]);
                vec![(i, c[0], m), (i, m, c[3])]
            } else {
                vec![(i, c[0], c[3])]
            }
        })
        .collect();
    let count = out.values.get("patch_count").unwrap()[0] as usize;
    let (mut lo, mut hi) = (0.0f64, 0.0f64);
    let mut sides_met = 0;
    for pi in 0..count {
        let get = |k: &str| {
            out.values
                .get(&format!("patch{pi}_{k}"))
                .map_or(&[][..], Vec::as_slice)
        };
        let (cpts, dims, uk, vk) = (get("cpts"), get("dims"), get("u"), get("v"));
        let (nu, nv, p, q) = (
            dims[0] as usize + 1,
            dims[1] as usize + 1,
            dims[2] as usize,
            dims[3] as usize,
        );
        let clamped = |k: &[f64], d: usize| {
            k[..=d].iter().all(|t| *t == k[0])
                && k[k.len() - 1 - d..].iter().all(|t| *t == k[k.len() - 1])
        };
        check(
            clamped(uk, p) && clamped(vk, q),
            "no closed form: unclamped knots",
        )?;
        let pt = |i: usize, j: usize| {
            Vec3::new(
                cpts[4 * (i * nv + j)],
                cpts[4 * (i * nv + j) + 1],
                cpts[4 * (i * nv + j) + 2],
            )
        };
        let sides: [(Vec<Vec3>, Vec<Vec3>, usize, &[f64]); 4] = [
            (
                (0..nv).map(|j| pt(0, j)).collect(),
                (0..nv).map(|j| pt(1, j) - pt(0, j)).collect(),
                q,
                vk,
            ),
            (
                (0..nv).map(|j| pt(nu - 1, j)).collect(),
                (0..nv).map(|j| pt(nu - 1, j) - pt(nu - 2, j)).collect(),
                q,
                vk,
            ),
            (
                (0..nu).map(|i| pt(i, 0)).collect(),
                (0..nu).map(|i| pt(i, 1) - pt(i, 0)).collect(),
                p,
                uk,
            ),
            (
                (0..nu).map(|i| pt(i, nv - 1)).collect(),
                (0..nu).map(|i| pt(i, nv - 1) - pt(i, nv - 2)).collect(),
                p,
                uk,
            ),
        ];
        for (row, cross, deg, knots) in &sides {
            let (e0, e1) = (row[0], row[row.len() - 1]);
            let near = |x: Vec3, y: Vec3| (x - y).length() <= bound;
            let Some(&(ci, _, _)) = ends.iter().find(|(_, c0, c1)| {
                (near(e0, *c0) && near(e1, *c1)) || (near(e0, *c1) && near(e1, *c0))
            }) else {
                continue;
            };
            if srfs[ci].is_null() {
                continue;
            }
            sides_met += 1;
            // knot insertion: each inserted point is a convex combination of two, with
            // relative error under `γ_6` of the net's largest coordinate `m`, chained once
            // per insertion; a cross difference adds its own rounding (`2 m`)
            let m = (0..nu * nv).fold(0.0f64, |acc, k| acc.max(mc(pt(k / nv, k % nv))));
            let interior = {
                let (lo, hi) = (knots[*deg], knots[knots.len() - 1 - deg]);
                let mut ks: Vec<f64> = knots
                    .iter()
                    .copied()
                    .filter(|t| *t > lo && *t < hi)
                    .collect();
                ks.dedup();
                ks.len()
            };
            let gins = gamma(6 * deg * interior + 4);
            let segs = bezier_segments(*deg, row, knots);
            let csegs = bezier_segments(*deg, cross, knots);
            check(
                segs.len() == csegs.len(),
                "a side's segments not its knots'",
            )?;
            for (b, c) in segs.iter().zip(&csegs) {
                let bp: Vec<R3> = b
                    .iter()
                    .map(|p| R3::new(*p, gins * m).sub(R3::new(o, 0.0)))
                    .collect();
                let dp: Vec<R3> = c.iter().map(|p| R3::new(*p, gins * 2.0 * m)).collect();
                let t: Vec<R3> = bp.windows(2).map(|w| w[1].sub(w[0])).collect();
                let n = bern_product(&dp, &t, R3::cross, R3::weigh, R3::add);
                let r = reference_normal(&neighbour, &bp);
                let nr = bern_product(&n, &r, R3::cross, R3::weigh, R3::add);
                let f = bern_product(&nr, &nr, R3::dot, Rv::weigh, Rv::add);
                let nn = bern_product(&n, &n, R3::dot, Rv::weigh, Rv::add);
                let rr = bern_product(&r, &r, R3::dot, Rv::weigh, Rv::add);
                let gg = bern_product(&nn, &rr, Rv::mul, Rv::weigh, Rv::add);
                // |R| from below over the segment: the reference normal exists
                let rmin = rr.iter().map(|c| c.v - c.e).fold(f64::INFINITY, f64::min);
                check(
                    rmin > 0.0,
                    "no certificate: a side point where the neighbour's gradient vanishes",
                )?;
                let ratio_lo = |fc: Rv, gc: Rv| {
                    let num = fc.v - fc.e;
                    let den = gc.v + gc.e;
                    (num / den).max(0.0) * (1.0 - gamma(2))
                };
                let ratio_v = |fc: Rv, gc: Rv| fc.v / gc.v;
                let (fe, ge) = (f[f.len() - 1], gg[gg.len() - 1]);
                let mut low = ratio_lo(f[0], gg[0]).max(ratio_lo(fe, ge));
                // the same without the rounding charge: only to decide when to stop halving
                let mut low_v = ratio_v(f[0], gg[0]).max(ratio_v(fe, ge));
                let mut up = 0.0f64;
                let mut stack = vec![(f.clone(), gg.clone(), 0usize)];
                while let Some((fs, gs, depth)) = stack.pop() {
                    let (f0, f1, g0, g1) = (fs[0], fs[fs.len() - 1], gs[0], gs[gs.len() - 1]);
                    low = low.max(ratio_lo(f0, g0)).max(ratio_lo(f1, g1));
                    low_v = low_v.max(ratio_v(f0, g0)).max(ratio_v(f1, g1));
                    let fmax = fs.iter().map(|c| c.v + c.e).fold(0.0f64, f64::max);
                    let gmin = gs.iter().map(|c| c.v - c.e).fold(f64::INFINITY, f64::min);
                    let fmax_v = fs.iter().map(|c| c.v).fold(0.0f64, f64::max);
                    let gmin_v = gs.iter().map(|c| c.v).fold(f64::INFINITY, f64::min);
                    if gmin > 0.0 && fmax_v / gmin_v <= low_v * (1.0 + 1e-9) + 1e-30 {
                        // the hull is down to the best value: the charged bound holds here
                        up = up.max(fmax / gmin * (1.0 + gamma(2)));
                        continue;
                    }
                    check(
                        depth < G1_DEPTH,
                        "no certificate: the G1 bound did not converge",
                    )?;
                    let (fl, fr) = halves(&fs);
                    let (gl, gr) = halves(&gs);
                    stack.push((fl, gl, depth + 1));
                    stack.push((fr, gr, depth + 1));
                }
                lo = lo.max(low.sqrt());
                hi = hi.max(up.max(low).sqrt());
            }
        }
    }
    check(sides_met > 0, "no side with a neighbour")?;
    Ok((lo, hi))
}

/// G1 old against new ([`corner_g1`]): the new blend is FARTHER when its certified
/// deviation is above the old one's, CLOSER when below, and EQUAL when the intervals
/// meet (they are then the same value to the certificate's width, `1e-9` relative plus
/// the rounding terms).
fn cert_corner_g1(inputs: &Value, new: &Output, old: &Output) -> (Verdict, Verdict) {
    let (n, o) = match (corner_g1(inputs, new), corner_g1(inputs, old)) {
        (Ok(n), Ok(o)) => (n, o),
        (Err(e), _) => return (Err(e), Ok(())),
        (Ok(_), Err(e)) => return (Ok(()), Err(e)),
    };
    if n.0 > o.1 {
        (
            fail(format!(
                "G1 deviation sin {:e} above the old {:e}",
                n.0, o.1
            )),
            Ok(()),
        )
    } else if n.1 < o.0 {
        (
            Ok(()),
            fail(format!(
                "G1 deviation sin {:e} above the new {:e}",
                o.0, n.1
            )),
        )
    } else {
        (Ok(()), Ok(()))
    }
}

/// Why a moved corner-blend row moved: its neighbour is a bounded cylinder
/// (`new_cylinder_bounded`) on perpendicular axes that are not unit. Main built its
/// generator, and so its NURB, at `origin + r X` with the axis as given, `r |X|` from the
/// axis, while the cylinder's data and STEP form have radius `r`: main blended against a
/// neighbour whose NURB was another cylinder. The loop curves are the same on both sides
/// (they are STEP points; the old patches' sides meet them too, [`cert_corner_sides`]).
/// This certifies both halves: `|X|` is not 1 (beyond its rounding, so main's NURB was
/// off by `r ||X| - 1|`), and every curve end (a point the neighbour evaluated) is at
/// distance `r` from the axis through `origin` along `X x Y`, within the rounding of the
/// evaluation and of this check (`γ_256` of the magnitudes, as in [`cert_corner_sides`]).
/// Tangency to the neighbour is certified separately, old against new ([`corner_g1`]).
fn cert_corner_inputs(inputs: &Value) -> Verdict {
    let srfs = inputs["srfs"].as_array().unwrap();
    let Some(a) = srfs.iter().find_map(|s| s.get("analytic")) else {
        return fail("no closed form: no analytic neighbour");
    };
    check(
        a["kind"].as_u64() == Some(1),
        "no closed form: not a bounded cylinder",
    )?;
    let (o, x, y) = (v3(&a["origin"]), v3(&a["x"]), v3(&a["y"]));
    let r = a["p"][0].as_f64().unwrap();
    let xl = x.length();
    check(
        (xl - 1.0).abs() > gamma(8),
        "a unit X: main's cylinder NURB had the data's radius",
    )?;
    let z = x.cross(y);
    let z = z * (1.0 / z.length());
    for c in inputs["curves"].as_array().unwrap() {
        let cp = c["cpts"].as_array().unwrap();
        for p in [cp.first().unwrap(), cp.last().unwrap()] {
            let d = v3(p) - o;
            let radial = (d - d.dot(z) * z).length();
            let bound = gamma(256) * (mc(o) + mc(v3(p)) + r);
            within(
                "a curve end off the cylinder of the data",
                (radial - r).abs(),
                bound,
            )?;
        }
    }
    Ok(())
}

/// A generated corner blend on an analytic neighbour (`analytic<i>`): the loop's curves
/// are cubic Beziers, and the blend's patches have them as sides (G0, the part of the
/// blend's contract the inputs allow: the curves' control points are points of the
/// neighbour, so the curves leave it, and its normal, by up to 17° here; neither run
/// reaches the 0.5° tangency target, which is best effort: `make_coons_patch_tangent_to
/// _sides` keeps its nearest attempt, unchanged code on both sides). Each patch side is
/// cut into Bezier segments ([`bezier_segments`]); a side is on the loop where its ends are
/// a curve's `C(s0)`, `C(s1)` (`[s0, s1]` the whole curve, or a half for the n-sided
/// blend's split), and then each segment's control points are those of the curve over the
/// segment's share of `[s0, s1]` ([`bezier_piece`], raised to the side's degree), to
/// rounding: the side is the curve there exactly. Every curve piece must be a side.
fn cert_corner_sides(inputs: &Value, out: &Output) -> Verdict {
    let status = out.values.get("status").and_then(|s| s.first().copied());
    check(status == Some(0.0), format!("status {status:?}"))?;
    let v3a = |p: &Value| {
        Vec3::new(
            p[0].as_f64().unwrap(),
            p[1].as_f64().unwrap(),
            p[2].as_f64().unwrap(),
        )
    };
    let mut curves = Vec::new();
    for c in inputs["curves"].as_array().unwrap() {
        check(
            c["p"].as_i64() == Some(3) && c["cpts"].as_array().unwrap().len() == 4,
            "no closed form: not a Bezier",
        )?;
        curves.push(
            c["cpts"]
                .as_array()
                .unwrap()
                .iter()
                .map(v3a)
                .collect::<Vec<Vec3>>(),
        );
    }
    let size = curves.iter().flatten().fold(0.0f64, |m, p| m.max(mc(*p)));
    let bound = gamma(256) * size;
    let halves = curves.len() > 4;
    let pieces: Vec<(usize, f64, f64)> = (0..curves.len())
        .flat_map(|i| {
            if halves {
                vec![(i, 0.0, 0.5), (i, 0.5, 1.0)]
            } else {
                vec![(i, 0.0, 1.0)]
            }
        })
        .collect();
    let mut met = vec![0usize; pieces.len()];
    let count = out
        .values
        .get("patch_count")
        .and_then(|c| c.first().copied())
        .unwrap_or(0.0) as usize;
    let mut worst = 0.0f64;
    for pi in 0..count {
        let get = |k: &str| {
            out.values
                .get(&format!("patch{pi}_{k}"))
                .map_or(&[][..], Vec::as_slice)
        };
        let (cpts, dims, uk, vk) = (get("cpts"), get("dims"), get("u"), get("v"));
        let (nu, nv, p, q) = (
            dims[0] as usize + 1,
            dims[1] as usize + 1,
            dims[2] as usize,
            dims[3] as usize,
        );
        check(cpts.len() == 4 * nu * nv, "a net not its dims'")?;
        check(
            cpts.chunks(4).all(|c| c[3] == NOW),
            "no closed form: a rational patch",
        )?;
        let pt = |i: usize, j: usize| {
            Vec3::new(
                cpts[4 * (i * nv + j)],
                cpts[4 * (i * nv + j) + 1],
                cpts[4 * (i * nv + j) + 2],
            )
        };
        let sides: [(Vec<Vec3>, usize, &[f64]); 4] = [
            ((0..nv).map(|j| pt(0, j)).collect(), q, vk),
            ((0..nv).map(|j| pt(nu - 1, j)).collect(), q, vk),
            ((0..nu).map(|i| pt(i, 0)).collect(), p, uk),
            ((0..nu).map(|i| pt(i, nv - 1)).collect(), p, uk),
        ];
        for (row, deg, knots) in &sides {
            let (a, b) = (row[0], row[row.len() - 1]);
            let found = pieces.iter().enumerate().find_map(|(k, &(ci, s0, s1))| {
                let c = &curves[ci];
                let (c0, c1) = (blossom(c, &[s0; 3]), blossom(c, &[s1; 3]));
                if (a - c0).length().max((b - c1).length()) <= bound {
                    Some((k, ci, s0, s1))
                } else if (a - c1).length().max((b - c0).length()) <= bound {
                    Some((k, ci, s1, s0))
                } else {
                    None
                }
            });
            let Some((k, ci, s0, s1)) = found else {
                continue;
            };
            met[k] += 1;
            let (lo, hi) = (knots[*deg], knots[knots.len() - 1 - deg]);
            let mut ks: Vec<f64> = knots
                .iter()
                .copied()
                .filter(|t| *t >= lo && *t <= hi)
                .collect();
            ks.dedup();
            let segs = bezier_segments(*deg, row, knots);
            check(
                segs.len() + 1 == ks.len(),
                "a side's segments not its knots'",
            )?;
            for (seg, w) in segs.iter().zip(ks.windows(2)) {
                check(seg.len() >= 4, "no closed form: a side below cubic")?;
                let at = |t: f64| s0 + (s1 - s0) * (t - lo) / (hi - lo);
                let mut want = bezier_piece(&curves[ci], at(w[0]), at(w[1]));
                // raised to the side's degree (`Q_i = i/(d+1) P_{i-1} + (1 - i/(d+1)) P_i`)
                while want.len() < seg.len() {
                    let d1 = want.len() as f64;
                    let mut up = vec![want[0]];
                    for i in 1..want.len() {
                        let a = i as f64 / d1;
                        up.push(a * want[i - 1] + (1.0 - a) * want[i]);
                    }
                    up.push(want[want.len() - 1]);
                    want = up;
                }
                for (x, y) in seg.iter().zip(&want) {
                    worst = worst.max((*x - *y).length());
                }
            }
        }
    }
    check(
        met.iter().all(|m| *m == 1),
        format!("loop pieces met {met:?} times, not once each"),
    )?;
    within("a patch side off its loop curve", worst, bound)
}

// ---- surface.surf_of_extrusion_p2: a polyline extrusion against an unbounded cone ------

/// The Bezier segments of the clamped B-spline `(p, cps, knots)` (Boehm's insertion of
/// every interior knot to multiplicity `p`; the curve is unchanged, the new points are
/// convex combinations of the old, rounding `γ_4` of their size a step).
fn bezier_segments(p: usize, cps: &[Vec3], knots: &[f64]) -> Vec<Vec<Vec3>> {
    let mut pts = cps.to_vec();
    let mut u = knots.to_vec();
    let (lo, hi) = (u[p], u[u.len() - 1 - p]);
    let mut distinct: Vec<f64> = u.iter().copied().filter(|t| *t > lo && *t < hi).collect();
    distinct.dedup();
    for t in distinct {
        loop {
            let s = u.iter().filter(|x| **x == t).count();
            if s >= p {
                break;
            }
            let k = u.iter().rposition(|x| *x <= t).unwrap();
            let mut q = Vec::with_capacity(pts.len() + 1);
            for i in 0..=pts.len() {
                if i + p <= k {
                    q.push(pts[i]);
                } else if i + s > k {
                    q.push(pts[i - 1]);
                } else {
                    let a = (t - u[i]) / (u[i + p] - u[i]);
                    q.push((1.0 - a) * pts[i - 1] + a * pts[i]);
                }
            }
            u.insert(k + 1, t);
            pts = q;
        }
    }
    let n = (pts.len() - 1) / p;
    (0..n).map(|j| pts[j * p..=j * p + p].to_vec()).collect()
}

/// The Bernstein coefficients of the product of two degree-`m` Bernstein polynomials.
fn bern_mul(a: &[f64], b: &[f64]) -> Vec<f64> {
    let m = a.len() - 1;
    let binom =
        |n: usize, k: usize| (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64);
    let mut c = vec![0.0; 2 * m + 1];
    for (i, ai) in a.iter().enumerate() {
        for (j, bj) in b.iter().enumerate() {
            c[i + j] += binom(m, i) * binom(m, j) / binom(2 * m, i + j) * ai * bj;
        }
    }
    c
}

/// The largest `|f|` over `[0, 1]` of the Bernstein polynomial `c`, bounded by its
/// coefficients (the convex hull property), halving by de Casteljau where the bound is
/// above `want`, `depth` times at most.
fn bern_bound(c: &[f64], want: f64, depth: u32) -> f64 {
    let b = c.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    if b <= want || depth == 0 {
        return b;
    }
    let n = c.len();
    let mut w = c.to_vec();
    let (mut left, mut right) = (vec![0.0; n], vec![0.0; n]);
    for r in 0..n {
        left[r] = w[0];
        right[n - 1 - r] = w[n - 1 - r];
        for i in 0..n - 1 - r {
            w[i] = 0.5 * (w[i] + w[i + 1]);
        }
    }
    bern_bound(&left, want, depth - 1).max(bern_bound(&right, want, depth - 1))
}

/// The curves of an extrusion intersection record (`put_tsect` after the cone's status
/// and the intersection's): `(degree, control points, knots)` of each NURB, non-rational.
fn extrusion_curves(out: &Output) -> Result<Vec<(usize, Vec<Vec3>, Vec<f64>)>, Fail> {
    let v = out.values.get("out").map_or(&[][..], Vec::as_slice);
    check(
        v.len() > 3 && v[0] == 0.0 && v[1] == 0.0,
        "a failed cone or intersection",
    )?;
    check(v[2] == 0.0, "more intersections to run")?;
    let mut r = Rd::new(&v[3..]);
    let n = r.f() as usize;
    let mut out = Vec::new();
    for _ in 0..n {
        check(r.f() == 1.0, "a missing curve")?;
        let _ty = r.f();
        let _ivl = r.n(2);
        check(r.f() == 1.0, "a curve with no NURB")?;
        let deg = r.f() as usize;
        let ncp = r.f() as usize;
        let mut cps = Vec::new();
        for _ in 0..ncp {
            let p = r.v3();
            check(r.f() == NOW, "a rational curve")?;
            cps.push(p);
        }
        let nk = r.f() as usize;
        out.push((deg, cps, r.n(nk)));
    }
    Ok(out)
}

/// An unbounded cone (`kind: semi`, no height) against the extrusion of a degree-1
/// B-spline generator (a polyline): each strip is a plane through a polyline segment
/// along the extrusion vector, so each curve of the intersection lies on one such plane
/// and on the cone. Each curve is cut into Bezier segments; a segment is within the
/// largest distance of its control points from the plane (convex hull), and, with `q =
/// P − o`, `h = q·z` and `R(h) = bot + h tan(semi)`, `f = q·q − h² − R²` is a degree-6
/// Bernstein polynomial whose coefficients bound it; a point at radius `ρ` from the axis
/// `|ρ − R| cos(semi) = |f| cos(semi) / (ρ + R)` from the cone, where `ρ + R ≥ R_min +
/// √(R_min² − |f|)`, `R_min` the least radius over the segment's heights (positive: the
/// segment is on the cone's nappe). Every curve
/// must be within the intersection's tolerance of both, and the curves' ends those of
/// the `other` run's curves (the same branches).
fn cert_extrusion_cone(inputs: &Value, out: &Output, other: &Output) -> Verdict {
    check(
        inputs["op"].as_str() == Some("global") && inputs["target"].as_str() == Some("cone"),
        "no closed form: op",
    )?;
    let c = &inputs["cone"];
    check(
        c["cone"]["kind"].as_str() == Some("semi") && c["cone"]["height"].is_null(),
        "no closed form: not an unbounded semi-angle cone",
    )?;
    check(
        !c["cone"]["inside"].as_bool().unwrap_or(false),
        "no closed form: inside out",
    )?;
    let v = |x: &Value| {
        let x = x.as_array().unwrap();
        Vec3::new(
            x[0].as_f64().unwrap(),
            x[1].as_f64().unwrap(),
            x[2].as_f64().unwrap(),
        )
    };
    let (o, x, y) = (v(&c["o"]), v(&c["x"]), v(&c["y"]));
    let z = x.cross(y);
    let z = z * (1.0 / z.length());
    let bot = c["cone"]["bot"].as_f64().unwrap();
    let t = det::tan(c["cone"]["semi"].as_f64().unwrap().to_radians());
    let tol = inputs["tol"].as_f64().unwrap();
    let g = &inputs["surf"]["gen"];
    check(
        g["kind"].as_str() == Some("b_spline") && g["p"].as_i64() == Some(1),
        "no closed form: generator",
    )?;
    let gp: Vec<Vec3> = g["cpts"].as_array().unwrap().iter().map(v).collect();
    let ext = v(&inputs["surf"]["build"]["vec"]);
    let curves = extrusion_curves(out)?;
    let theirs = extrusion_curves(other)?;
    check(!curves.is_empty(), "no curves")?;
    let mut worst = (0.0f64, 0.0f64);
    for (deg, cps, knots) in &curves {
        let segs = bezier_segments(*deg, cps, knots);
        let size = cps.iter().fold(mc(o), |m, p| m.max(mc(*p))) + bot.abs();
        let rnd = gamma(64) * size;
        // the strip plane nearest the whole curve's control points
        let plane = gp
            .windows(2)
            .map(|w| {
                let n = (w[1] - w[0]).cross(ext);
                let n = n * (1.0 / n.length());
                cps.iter()
                    .fold(0.0f64, |m, p| m.max((*p - w[0]).dot(n).abs()))
            })
            .fold(f64::INFINITY, f64::min);
        worst.0 = worst.0.max(plane + rnd);
        for s in &segs {
            let q: Vec<Vec3> = s.iter().map(|p| *p - o).collect();
            let h: Vec<f64> = q.iter().map(|w| w.dot(z)).collect();
            let rr: Vec<f64> = h.iter().map(|hh| bot + t * hh).collect();
            let (hmin, hmax) = h
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| {
                    (a.min(*x), b.max(*x))
                });
            let rmin = if t >= 0.0 {
                bot + t * hmin
            } else {
                bot + t * hmax
            };
            check(rmin > 0.0, "a segment reaching the apex")?;
            let comp = |f: fn(Vec3) -> f64| -> Vec<f64> { q.iter().map(|w| f(*w)).collect() };
            let (qx, qy, qz) = (comp(|w| w.x), comp(|w| w.y), comp(|w| w.z));
            let mut f = bern_mul(&qx, &qx);
            for (a, b) in [(&qy, &qy), (&qz, &qz)] {
                for (fi, ci) in f.iter_mut().zip(bern_mul(a, b)) {
                    *fi += ci;
                }
            }
            for (a, b) in [(&h, &h), (&rr, &rr)] {
                for (fi, ci) in f.iter_mut().zip(bern_mul(a, b)) {
                    *fi -= ci;
                }
            }
            let qm = q.iter().fold(0.0f64, |m, w| m.max(w.length()));
            let rm = rr.iter().fold(0.0f64, |m, x| m.max(x.abs()));
            let e = gamma(64) * (3.0 * qm * qm + rm * rm) + 2.0 * rnd * (qm + rm);
            // ρ² = f + R² ≥ R_min² − |f|, so ρ + R ≥ R_min + √(R_min² − |f|); the distance
            // from the cone in the meridian plane is |ρ − R| cos(semi)
            let cos = det::cos(c["cone"]["semi"].as_f64().unwrap().to_radians());
            let fb = bern_bound(&f, (tol * 1.9 * rmin / cos - e).max(0.0), 16) + e;
            let den = rmin + (rmin * rmin - fb).max(0.0).sqrt();
            worst.1 = worst.1.max(fb / den * cos * (1.0 + gamma(8)));
        }
    }
    within("a curve off its strip's plane", worst.0, tol)?;
    within("a curve off the cone", worst.1, tol)?;
    // the same branches: the same number of curves, each one's ends those of one of theirs
    check(
        curves.len() == theirs.len(),
        format!(
            "{} curves where the other run has {}",
            curves.len(),
            theirs.len()
        ),
    )?;
    let ends = |c: &(usize, Vec<Vec3>, Vec<f64>)| [c.1[0], c.1[c.1.len() - 1]];
    for c in &curves {
        let [a, b] = ends(c);
        let d = theirs
            .iter()
            .map(|t| {
                let [p, q] = ends(t);
                ((a - p).length().max((b - q).length())).min((a - q).length().max((b - p).length()))
            })
            .fold(f64::INFINITY, f64::min);
        within("a curve's ends not the other run's", d, 2.0 * tol)?;
    }
    Ok(())
}

// ---- surface.cone_p3 ------------------------------------------------------------------------

/// A `surface_cone_p3` cone record (`build`): origin, x, y, bottom and top radius, height,
/// start and end angle, swap, inside out, cylinder; the patch is `domains`' fractions of
/// the NURB domain (`domain`).
#[derive(Debug, Clone, Copy)]
struct P3Cone {
    o: Vec3,
    x: Vec3,
    y: Vec3,
    z: Vec3,
    bot: f64,
    top: f64,
    h: f64,
    /// the patch's heights along `z`
    hs: [f64; 2],
}

impl P3Cone {
    /// The cone of record `r` and its patch for the domain fractions `f`; `None` where
    /// the fractions' map to heights and angles is not certain here: a trimmed height
    /// range of an inside-out or swapped cone, or a trimmed angle range (the caller
    /// checks angles it needs against [`Self::angles_covered`]).
    fn of(r: &[f64], f: &[f64]) -> Option<(Self, [f64; 2])> {
        let v = |i: usize| Vec3::new(r[i], r[i + 1], r[i + 2]);
        let (o, x, y) = (v(0), v(3), v(6));
        // the axis of `x × y` scaled first: a coordinate of `1e300` overflows the product
        let sc = |w: Vec3| w * (1.0 / mc(w));
        let z = sc(x).cross(sc(y));
        let z = z * (1.0 / z.length());
        let xu = x * (1.0 / x.length());
        let yu = z.cross(xu);
        let cyl = r[16] != 0.0;
        let (bot, h) = (r[9], r[11]);
        let top = if cyl { bot } else { r[10] };
        let (swap, inside) = (r[14] != 0.0, r[15] != 0.0);
        let (lin, ang) = if swap {
            ([f[0], f[2]], [f[1], f[3]])
        } else {
            ([f[1], f[3]], [f[0], f[2]])
        };
        // inside out, the NURB may run the heights reversed: fractions symmetric about ½
        // name the same heights either way
        let symmetric = lin[0] + lin[1] == 1.0;
        if lin != [0.0, 1.0] && (swap || (inside && !symmetric)) {
            return None;
        }
        if ang != [0.0, 1.0] && (swap || inside) {
            return None;
        }
        if r[12] != 0.0 || r[13] != 360.0 {
            return None;
        }
        let hs = [h * lin[0], h * lin[1]];
        Some((
            P3Cone {
                o,
                x: xu,
                y: yu,
                z,
                bot,
                top,
                h,
                hs,
            },
            ang,
        ))
    }
    fn radius(&self, v: f64) -> f64 {
        self.bot + (self.top - self.bot) * v / self.h
    }
    fn cylinder(&self) -> bool {
        self.bot == self.top
    }
}

/// The angles (degrees from `x` towards `y`) the angular fractions `ang` cover for
/// certain: the 9-point circle's knots `0, ¼, ½, ¾, 1` are at `0°, 90°, 180°, 270°,
/// 360°`, so `[⌈4 f₀⌉ 90°, ⌊4 f₁⌋ 90°]`.
fn angles_covered(ang: [f64; 2]) -> [f64; 2] {
    [(4.0 * ang[0]).ceil() * 90.0, (4.0 * ang[1]).floor() * 90.0]
}

/// A 3D curve of a `surface_cone_p3` record (`put_curve`): type, and for a degree-1
/// two-point NURB its end points.
fn read_p3_curve(r: &mut Rd) -> Option<(f64, Option<[Vec3; 2]>, Option<[Vec3; 2]>)> {
    if r.f() == 0.0 {
        return None;
    }
    let ty = r.f();
    let _dim = r.f();
    let mut analytic = None;
    match ty as i64 {
        14010 => {
            let (p, d) = (r.v3(), r.v3());
            let ivl = r.n(2);
            let scale = r.f();
            let _inside = r.f();
            analytic = Some([p + (ivl[0] * scale) * d, p + (ivl[1] * scale) * d]);
        }
        14021 => {
            let _ = r.n(3 + 3 + 3 + 2 + 2 + 1);
        }
        _ => {}
    }
    let _kt = r.f();
    let _form = r.f();
    let mut seg = None;
    if r.f() != 0.0 {
        let n = r.f() as usize;
        let deg = r.f() as usize;
        let m = r.f() as usize;
        let cps: Vec<CPoint> = (0..=n)
            .map(|_| CPoint::new(r.f(), r.f(), r.f(), r.f()))
            .collect();
        let _knots = r.n(m + 1);
        if deg == 1 && cps.len() == 2 && cps.iter().all(|c| c.w == NOW || c.w == 1.0) {
            seg = Some([
                Vec3::new(cps[0].x, cps[0].y, cps[0].z),
                Vec3::new(cps[1].x, cps[1].y, cps[1].z),
            ]);
        }
    }
    let _nat = r.n(2);
    Some((ty, analytic, seg))
}

/// What the intersection of two `surface_cone_p3` patches is, by closed form.
enum P3Want {
    /// the segments (end points)
    Lines(Vec<[Vec3; 2]>),
    /// a point common to both patches, `margin` inside each (so the intersection is not
    /// empty)
    NonEmpty,
}

/// Two cylinders (or a cone and a cylinder) with parallel axes: in a plane across the
/// axes the sections are circles about points `d` apart; they meet where `|r₁ - r₂| ≤ d
/// ≤ r₁ + r₂`, two cylinders along the lines through the circles' common points, over the
/// patches' common heights (each height range is the record's fractions of `[0, H]`).
/// Cylinders whose axes cross at right angles (`x² + y² = r₁²`, `x² + z² = r₂²` about
/// the crossing): a common point has `y² − z² = r₁² − r₂²`, outside the range of
/// `y² − z²` over the patches' heights the intersection is empty.
fn p3_cones(
    a: &P3Cone,
    aang: [f64; 2],
    b: &P3Cone,
    bang: [f64; 2],
    tol: f64,
) -> Result<P3Want, Fail> {
    let c = a.z.dot(b.z);
    let sin = a.z.cross(b.z).length();
    let size =
        mc(a.o) + mc(b.o) + a.h + b.h + a.bot.abs() + a.top.abs() + b.bot.abs() + b.top.abs();
    let g = gamma(64) * size;
    // b's heights on a's axis
    let hb0 = (b.o - a.o).dot(a.z);
    let (b_lo, b_hi) = {
        let (e0, e1) = (hb0 + c * b.hs[0], hb0 + c * b.hs[1]);
        (e0.min(e1), e0.max(e1))
    };
    if sin <= gamma(8) {
        check(
            b.cylinder(),
            "no closed form: parallel axes, b not a cylinder",
        )?;
        let w = b.o - a.o;
        let wp = w - w.dot(a.z) * a.z;
        let d = wp.length();
        check(d > g, "no closed form: coaxial")?;
        let (lo, hi) = (a.hs[0].max(b_lo), a.hs[1].min(b_hi));
        let rb = b.bot.abs();
        if !a.cylinder() {
            // a witness: a section strictly inside the common heights (eighths of them)
            // whose circles cross, so both patches share its two crossing points
            check(hi - lo > 2.0 * g, "no closed form: no common heights")?;
            check(
                aang == [0.0, 1.0] && bang == [0.0, 1.0],
                "no closed form: trimmed angles",
            )?;
            let crosses = (1..8).any(|k| {
                let m = lo + f64::from(k) / 8.0 * (hi - lo);
                let ra = a.radius(m).abs();
                (ra - rb).abs() + g < d && d < ra + rb - g
            });
            check(crosses, "no closed form: no section's circles cross")?;
            return Ok(P3Want::NonEmpty);
        }
        let ra = a.bot.abs();
        if !((ra - rb).abs() < d - g && d + g < ra + rb) || hi < lo {
            check(
                d > ra + rb + tol + g || d < (ra - rb).abs() - tol - g || hi < lo - tol - g,
                "no closed form: tangent sections",
            )?;
            return Ok(P3Want::Lines(Vec::new()));
        }
        let e = wp * (1.0 / d);
        let t = a.z.cross(e);
        let p = (ra * ra - rb * rb + d * d) / (2.0 * d);
        let q = (ra * ra - p * p).sqrt();
        let mut out = Vec::new();
        for sg in [1.0, -1.0] {
            let base = a.o + p * e + (sg * q) * t;
            // its angles on each cylinder, against the angles the patches surely cover
            for (cy, ang) in [(a, aang), (b, bang)] {
                if ang != [0.0, 1.0] {
                    let r = base - cy.o;
                    let th = det::atan2(r.dot(cy.y), r.dot(cy.x))
                        .to_degrees()
                        .rem_euclid(360.0);
                    let [c0, c1] = angles_covered(ang);
                    check(
                        th > c0 + 1e-9 && th < c1 - 1e-9,
                        "no closed form: a line at a trimmed angle",
                    )?;
                }
            }
            out.push([base + lo * a.z, base + hi * a.z]);
        }
        return Ok(P3Want::Lines(out));
    }
    if c.abs() <= gamma(8) {
        check(
            a.cylinder() && b.cylinder(),
            "no closed form: crossing axes, not cylinders",
        )?;
        check(
            aang == [0.0, 1.0] && bang == [0.0, 1.0],
            "no closed form: trimmed angles",
        )?;
        // the crossing: b's axis meets a's
        let w = b.o - a.o;
        let along = w.dot(a.z);
        let k = b.o - along * a.z - a.o;
        let kb = k - k.dot(b.z) * b.z;
        check(kb.length() <= g, "no closed form: skew axes")?;
        let tb = -(k.dot(b.z)); // the crossing's height on b, from b.o
        let (ra, rb) = (a.bot.abs(), b.bot.abs());
        // z: heights on a about the crossing; y: on b about it
        let zs = [a.hs[0] - along, a.hs[1] - along];
        let ys = [b.hs[0] - tb, b.hs[1] - tb];
        let sq = |r: [f64; 2]| {
            let (l, h) = (r[0].min(r[1]), r[0].max(r[1]));
            let lo = if l <= 0.0 && h >= 0.0 {
                0.0
            } else {
                (l * l).min(h * h)
            };
            (lo, (l * l).max(h * h))
        };
        let (z2, y2) = (sq(zs), sq(ys));
        let (lo, hi) = (y2.0 - z2.1, y2.1 - z2.0);
        let k2 = ra * ra - rb * rb;
        let slack = 4.0 * (tol + g) * (size + 1.0);
        check(
            k2 < lo - slack || k2 > hi + slack,
            "no closed form: the ranges of y² − z² reach r₁² − r₂²",
        )?;
        return Ok(P3Want::Lines(Vec::new()));
    }
    fail("no closed form for these axes")
}

/// A cylinder and a plane parallel to its axis: lines where the plane meets the section
/// circle (`δ` its distance from the axis, `±√(r² − δ²)` along the plane), over the
/// patch's heights, inside the plane's rectangle.
fn p3_plane(a: &P3Cone, aang: [f64; 2], pl: &[f64], pf: &[f64], tol: f64) -> Result<P3Want, Fail> {
    check(a.cylinder(), "no closed form: a cone and a plane")?;
    check(aang == [0.0, 1.0], "no closed form: trimmed angles")?;
    check(pf == [0.0, 0.0, 1.0, 1.0], "no closed form: trimmed plane")?;
    let v = |i: usize| Vec3::new(pl[i], pl[i + 1], pl[i + 2]);
    let (po, px, py) = (v(0), v(3), v(6));
    let n = px.cross(py);
    let n = n * (1.0 / n.length());
    let size = mc(a.o) + mc(po) + a.h + a.bot.abs();
    let g = gamma(64) * size;
    check(
        n.dot(a.z).abs() <= gamma(8),
        "no closed form: the plane crosses the axis",
    )?;
    let r = a.bot.abs();
    let delta = (a.o - po).dot(n);
    if delta.abs() > r + tol + g {
        return Ok(P3Want::Lines(Vec::new()));
    }
    check(delta.abs() < r - tol - g, "no closed form: a tangent plane")?;
    let q = (r * r - delta * delta).sqrt();
    let t = n.cross(a.z);
    let mut out = Vec::new();
    for sg in [1.0, -1.0] {
        let base = a.o - delta * n + (sg * q) * t;
        let ends = [base + a.hs[0] * a.z, base + a.hs[1] * a.z];
        // inside the plane's rectangle (its axes as given, unit here)
        for e in ends {
            let w = e - po;
            let (u, vv) = (w.dot(px) / px.length(), w.dot(py) / py.length());
            check(
                u > pl[9] + g && u < pl[11] - g && vv > pl[10] + g && vv < pl[12] - g,
                "no closed form: a line leaves the plane's rectangle",
            )?;
        }
        out.push(ends);
    }
    Ok(P3Want::Lines(out))
}

/// A `surface.cone_p3` row (`surface_cone_p3::rust`): the intersection's 3D curves
/// against the closed form ([`p3_cones`], [`p3_plane`]). The record's lines are their
/// degree-1 NURBs (exactly the segments between their control points) and their
/// analytic lines over their intervals; each must be one wanted segment (either way)
/// within `γ_64` of the inputs' size, and every wanted segment met. A special case that
/// declines (`more`, no curves) leaves the answer to the general intersector: correct
/// for any intersection; no curves without `more` claims an empty one.
fn cert_p3_row(inputs: &Value, out: &Output) -> Verdict {
    let num = |v: &Value| -> Vec<f64> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap())
            .collect()
    };
    let (cone, other, dom) = (
        num(&inputs["cone"]),
        num(&inputs["other"]),
        num(&inputs["domains"]),
    );
    let op = inputs["op"].as_i64().unwrap();
    let tol = inputs["tolerance"].as_f64().unwrap_or(0.0);
    check(
        !inputs["same"].as_bool().unwrap_or(false),
        "no closed form: one surface",
    )?;
    let Some((a, aang)) = P3Cone::of(&cone, &dom[..4]) else {
        return fail("no closed form: the patch's fractions");
    };
    let want = if op == 0 || op == 3 {
        let Some((b, bang)) = P3Cone::of(&other, &dom[4..]) else {
            return fail("no closed form: the other patch's fractions");
        };
        p3_cones(&a, aang, &b, bang, tol)?
    } else {
        p3_plane(&a, aang, &other, &dom[4..], tol)?
    };
    let more = out
        .ints
        .get("status_and_more")
        .and_then(|v| v.get(1).copied())
        .unwrap_or(0);
    let geo: &[f64] = out.values.get("geometry").map_or(&[], Vec::as_slice);
    let mut r = Rd::new(geo);
    let n = r.f() as usize;
    let mut curves = Vec::new();
    for _ in 0..n {
        match read_p3_curve(&mut r) {
            Some(c) => curves.push(c),
            None => return fail("a missing 3D curve"),
        }
    }
    if more != 0 {
        check(
            curves.is_empty(),
            "curves with the general intersector still to run",
        )?;
        return Ok(());
    }
    match want {
        P3Want::NonEmpty => fail("no curves (none to follow) for a non-empty intersection"),
        P3Want::Lines(segs) => {
            check(
                curves.len() == segs.len(),
                format!("{} curves where {} lines are", curves.len(), segs.len()),
            )?;
            let size = mc(a.o)
                + mc(Vec3::new(other[0], other[1], other[2]))
                + a.h
                + a.bot.abs()
                + a.top.abs()
                + 1.0;
            let bound = gamma(64) * size;
            let mut met = vec![false; segs.len()];
            for (ty, analytic, seg) in curves {
                check(
                    ty == 14010.0,
                    format!("a curve of type {ty} where lines are"),
                )?;
                let (Some(an), Some(sg)) = (analytic, seg) else {
                    return fail("a line without its two-point NURB");
                };
                let dist = |x: [Vec3; 2], y: [Vec3; 2]| {
                    let f = (x[0] - y[0]).length().max((x[1] - y[1]).length());
                    let b = (x[0] - y[1]).length().max((x[1] - y[0]).length());
                    f.min(b)
                };
                within(
                    "a line's NURB against its analytic line",
                    dist(an, sg),
                    bound,
                )?;
                let Some((i, d)) = segs
                    .iter()
                    .enumerate()
                    .map(|(i, w)| (i, dist(*w, sg)))
                    .min_by(|x, y| x.1.total_cmp(&y.1))
                else {
                    return fail("no wanted line");
                };
                within("a line off the closed form's", d, bound)?;
                met[i] = true;
            }
            check(met.iter().all(|m| *m), "a wanted line not found")
        }
    }
}

/// A `common_const_make` row (`surface_cone_p1::const_make_rust`): the cone as its
/// constructor leaves it, its natural domain (making the NURB), the polar flag, a point,
/// the copy and its point, then the rotation `rot` and a point of the rotated cone. The
/// domains are the certified form's; each point is the STEP point of its NURB parameter
/// (`evaluation_ratio_of`), the rotated one the rotation of the first point.
fn cert_const_make(ctx: &Ctx, vals: &Values) -> Verdict {
    check(
        first(vals, "build") == Some(SUCCESS),
        "construction refused",
    )?;
    let out: &[f64] = vals.get("out").map_or(&[], Vec::as_slice);
    check(out.len() == 29, "const_make layout")?;
    let nd = ctx.form.natural_uv_domain();
    let dom = |o: &[f64]| o == [nd.min().x, nd.min().y, nd.max().x, nd.max().y];
    check(out[0] == SUCCESS, "const query refused")?;
    check(dom(&out[2..6]), "natural domain is not the form's")?;
    check(
        out[6] == 1.0 && out[7] == 1.0,
        "no NURB or no polar conversion after the query",
    )?;
    let point = |o: &[f64], s: f64, t: f64| -> Verdict {
        check(o[0] == SUCCESS, "evaluation refused")?;
        let q = nd.evaluate(s, t);
        let step = ctx
            .surface
            .convert_uv_from_nurbs_to_step(&ctx.k, q)
            .map_err(|e| Fail {
                why: format!("no STEP parameter: {e}"),
                ratio: None,
            })?;
        ctx.ratio((step.x, step.y), Vec3::new(o[1], o[2], o[3]))
    };
    point(&out[8..12], 0.3, 0.6)?;
    check(
        out[12] == SUCCESS && out[13] == 1.0 && out[14] == 1.0,
        "copy refused, without NURB or not equal",
    )?;
    point(&out[15..19], 0.7, 0.2)?;
    check(
        out[19] == SUCCESS && out[20] == 1.0,
        "transform refused or dropped the NURB",
    )?;
    check(
        dom(&out[21..25]),
        "rotated natural domain is not the form's",
    )?;
    check(out[25] == SUCCESS, "rotated evaluation refused")?;
    // `rot`: origin (1, 2, 3), X = (0, 1, 0), Y = (-1, 0, 0), Z = (0, 0, 1), a signed
    // permutation plus a translation. The rotated form's control points are the rotated
    // ones, each rounded once (|c'| <= reach + 3); its weights are kept, so at the same
    // parameter (t == d, checked) the rational point, a convex combination, moves by at
    // most that rounding; both evaluations round within gamma(64) of their reach.
    let p = Vec3::new(out[9], out[10], out[11]);
    let want = Vec3::new(1.0 - p.y, 2.0 + p.x, 3.0 + p.z);
    let got = Vec3::new(out[26], out[27], out[28]);
    let reach = form_reach(&ctx.form) + 3.0;
    within(
        "rotated point vs the rotated first point",
        (got - want).length(),
        3.0 * gamma(64) * reach,
    )
}

// ---- intersect.api_intersectors ---------------------------------------------------------------

/// An `ssi-sphere-cone-near-pole-*` row (`intersect_api_intersectors`, op `surfaces`): a
/// sphere of radius `R` about the origin and the cone of radius `b` at `z = 0` to `t` at
/// `z = H` (both on the Z axis, `H` about `R`, `t` small: the cone's tip near the
/// sphere's pole). In the meridian half-plane `(ρ, z)` the sphere is `ρ² + z² = R²` and
/// the cone's line `ρ = b - s z`, `s = (b - t) / H`, so the intersection is the circles at
/// the roots of `(1 + s²) z² - 2 b s z + b² - R² = 0` in `[0, H]` (closed form; the
/// stable quadratic formula, each root within `γ(32)` of the inputs' size). The recorded
/// curves (5 points each, the golden's record) must be circles about the axis, every
/// recorded point within `tol` of one of the exact circles in the meridian plane, and
/// every exact circle met by a curve. `tol = 1e-6 R`, a thousandth of the intersection's
/// own default tolerance (a thousandth of the surfaces' size, `SurfaceTracer::new`): a
/// side within it is a correct answer of the call.
fn cert_sphere_cone(inputs: &Value, vals: &Values) -> Verdict {
    let shape = |k: &str| &inputs[k]["shape"];
    let (sph, cone) = if shape("surface1")["type"] == "sphere" {
        (shape("surface1"), shape("surface2"))
    } else {
        (shape("surface2"), shape("surface1"))
    };
    let num = |v: &Value, k: &str| v[k].as_f64().unwrap();
    let axes = |v: &Value| {
        v["origin"] == serde_json::json!([0, 0, 0])
            && v["x"] == serde_json::json!([1, 0, 0])
            && v["y"] == serde_json::json!([0, 1, 0])
    };
    check(
        axes(sph) && axes(cone),
        "no closed form for these placements",
    )?;
    let r = num(sph, "radius");
    let (b, t, h) = (
        num(cone, "bot_radius"),
        num(cone, "top_radius"),
        num(cone, "height"),
    );
    let s = (b - t) / h;
    let (qa, qb, qc) = (1.0 + s * s, -2.0 * b * s, b * b - r * r);
    let disc = qb * qb - 4.0 * qa * qc;
    check(
        disc > 0.0,
        "no closed form: the meridian line misses the circle",
    )?;
    let q = -0.5 * (qb - disc.sqrt()); // qb < 0: no cancellation
    let size = b.abs() + r.abs() + h.abs();
    let root_err = gamma(32) * size;
    let tol = 1e-6 * r;
    let circles: Vec<(f64, f64)> = [q / qa, qc / q]
        .into_iter()
        .filter(|z| *z >= -tol && *z <= h + tol)
        .map(|z| (b - s * z, z))
        .collect();
    let rec: &[f64] = vals.get("record").map_or(&[], Vec::as_slice);
    check(first(vals, "status") == Some(SUCCESS), "the call failed")?;
    let mut rd = Rd::new(rec);
    let n = rd.f() as usize;
    let mut met = vec![false; circles.len()];
    for _ in 0..n {
        let _type = rd.f();
        let _ivl = rd.n(2);
        let mut pts = Vec::new();
        for _ in 0..5 {
            if rd.f() == 0.0 {
                pts.push(rd.v3());
            }
        }
        check(pts.len() == 5, "a curve point failed to evaluate")?;
        let on = |c: (f64, f64)| {
            pts.iter()
                .map(|p| det::hypot(det::hypot(p.x, p.y) - c.0, p.z - c.1))
                .fold(0.0_f64, f64::max)
        };
        let Some((i, d)) = circles
            .iter()
            .enumerate()
            .map(|(i, c)| (i, on(*c)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
        else {
            return fail("a curve where the closed form has none");
        };
        within("curve off the exact circle", d, tol + root_err)?;
        met[i] = true;
    }
    check(met.iter().all(|m| *m), "an exact circle not found")?;
    Ok(())
}

// ---- surface.cone_p2 ------------------------------------------------------------------------

/// A cone state as `surface_cone_p2::put_state` writes it, in [`State`]'s fields: that
/// layout has the radii but not the heights, linear extent or canonical data, which are
/// filled from the domain (their checks in [`cert_state`] hold by construction) and the
/// height (its one value).
fn read_state_p2(r: &mut Rd) -> State {
    let start = r.i;
    let _type = r.f();
    let _kinds = r.n(3);
    let r0 = r.f();
    let semi = r.f();
    let flags = r.n(5);
    let origin = r.v3();
    let x = r.v3();
    let y = r.v3();
    let identity = r.v[start..r.i].to_vec();
    let d = r.n(4);
    let dom = [d[0], d[1], d[2], d[3]];
    let rad = r.n(2);
    let maxdom = r.n(4);
    let apex = r.n(6);
    let height = r.f();
    let polar = r.f() != 0.0;
    let na = r.f() as usize;
    let angles = if na > 0 { r.n(2) } else { Vec::new() };
    let polar_curve = read_curve(r);
    let gen = read_curve(r);
    let form = if r.f() == 0.0 {
        None
    } else {
        let p = r.f() as usize;
        let q = r.f() as usize;
        let n = r.f() as usize;
        let m = r.f() as usize;
        let net: Vec<Vec<CPoint>> = (0..n)
            .map(|_| {
                (0..m)
                    .map(|_| CPoint::new(r.f(), r.f(), r.f(), r.f()))
                    .collect()
            })
            .collect();
        let nu = r.f() as usize;
        let u = r.n(nu);
        let nv = r.f() as usize;
        let v = r.n(nv);
        let s = srf_basic::Surface::new(net, p, q, u, v).unwrap();
        Some(BSplineSurface::from_nurb(s, true))
    };
    State {
        identity,
        r0,
        semi,
        bounded: flags[4] != 0.0,
        origin,
        x,
        y,
        dom,
        radh: vec![rad[0], rad[1], dom[1], dom[3]],
        lin: vec![dom[1], dom[3], 1.0],
        maxdom,
        apex,
        height: vec![0.0, height],
        polar,
        angles,
        polar_curve,
        gen,
        form,
    }
}

/// The cone the inputs define, by closed form: the canonical axes of the input axes taken
/// as directions (`Z` the unit `x × y`), the base radius and the semi-angle's tangent
/// (`(top - bot) / height` for a bounded cone), and the heights (`[0, height]` bounded).
fn input_cone(cone: &Value) -> Option<(Analytic, Option<[f64; 2]>)> {
    let v3 = |k: &str| {
        let a = &cone[k];
        Vec3::new(
            a[0].as_f64().unwrap(),
            a[1].as_f64().unwrap(),
            a[2].as_f64().unwrap(),
        )
    };
    let dir = |a: Vec3| a * (1.0 / mc(a));
    let x = dir(v3("x"));
    let z = x.cross(dir(v3("y")));
    let z = z * (1.0 / z.length());
    let x = x * (1.0 / x.length());
    let mode = &cone["mode"];
    let num = |k: &str| mode[k].as_f64().unwrap();
    let (r0, t, semi, heights) = match mode["kind"].as_str().unwrap() {
        "cone_unbounded" => (num("base"), tan_deg(num("semi")), num("semi"), None),
        // a cylinder (`SmCylinder`): the semi-angle 0, unbounded either way
        "cyl_unbounded" | "cyl_canonical" => (num("radius"), 0.0, 0.0, None),
        "cone_bounded" => {
            let (b, tp, h) = (num("bot"), num("top"), num("height"));
            let t = (tp - b) / h;
            (b, t, det::atan(t) * 180.0 / PI, Some([0.0, h]))
        }
        _ => return None,
    };
    let a = Analytic {
        o: v3("origin"),
        x,
        y: z.cross(x),
        z,
        r0,
        semi,
        t,
        dom: [0.0, 0.0, 360.0, 0.0],
        oob: false,
    };
    Some((a, heights))
}

/// The bound on the difference of two computations of a direction from the same axes
/// (normalisations and a cross product).
fn dir_bound() -> f64 {
    gamma(32)
}

/// `b` is the surface of `a` (the same axis line, and `|R_b| = |R_a|` along it): with
/// `s = ±1` the sense of `b`'s axis on `a`'s and `h` the axial height of `b`'s origin on
/// `a`, `R_b(v) = ±R_a(h + s v)`, i.e. `r0_b = ±(r0_a + t_a h)` and `t_b = ±s t_a`. Returns
/// `(h, s)`, mapping `b`'s heights to `a`'s.
fn same_cone(a: &Analytic, b: &Analytic, extra: f64) -> Result<(f64, f64), Fail> {
    let c = a.z.dot(b.z);
    let s = if c < 0.0 { -1.0 } else { 1.0 };
    within(
        "axis direction",
        (b.z - s * a.z).length(),
        2.0 * dir_bound() + extra,
    )?;
    let w = b.o - a.o;
    let h = w.dot(a.z);
    let off = (w - h * a.z).length();
    let m = mc(a.o) + mc(b.o);
    within(
        "axis line",
        off,
        gamma(32) * m + w.length() * dir_bound() + extra,
    )?;
    let r_at = a.r0 + a.t * h;
    let rb = gamma(32) * (a.r0.abs() + (a.t * h).abs() + b.r0.abs())
        + a.t.abs() * (gamma(32) * m + extra);
    let tb = a.t_bound() + b.t_bound() + gamma(4) * (a.t.abs() + b.t.abs()) + extra;
    let plus = (b.r0 - r_at).abs() <= rb && (b.t - s * a.t).abs() <= tb;
    let minus = (b.r0 + r_at).abs() <= rb && (b.t + s * a.t).abs() <= tb;
    if plus || minus {
        Ok((h, s))
    } else {
        fail(format!(
            "not the cone: r0 {} vs ±{r_at} (bound {rb:e}), tan {} vs ±{} (bound {tb:e})",
            b.r0,
            b.t,
            s * a.t,
        ))
    }
}

/// `b`'s heights `[v0, v1]` are `want`'s on `a` (by [`same_cone`]'s map), as sets.
fn same_heights(b: &Analytic, map: (f64, f64), want: [f64; 2], bound: f64) -> Verdict {
    let (h, s) = map;
    let e0 = h + s * b.dom[1];
    let e1 = h + s * b.dom[3];
    let (lo, hi) = (e0.min(e1), e0.max(e1));
    let (wl, wh) = (want[0].min(want[1]), want[0].max(want[1]));
    within(
        "height interval's ends",
        (lo - wl).abs().max((hi - wh).abs()),
        bound,
    )
}

/// A `surface.cone_p2` row (`surface_cone_p2::rust_op`): the answer for the cone the
/// inputs define (`input_cone`), certified by the state certificate and closed forms.
///
/// * `offset`: the offset by `d` along the surface's normal (`sign = ∓1` for `swap_uv`,
///   for inside out) is the cone of the same axis and tangent with `r0 + sign d / cos`,
///   the heights moved by `-sign d sin` (`(r0 + t v)` moved by `sign d cos`), an end that
///   would cross the apex truncated there.
/// * `recognize` and `rebuild` with no edit, or a rigid move of every control point, or a
///   homogeneous rescale: the (moved) input cone; `rebuild` after a move of the last row:
///   the state's own form lies on its cone (the certified state).
/// * `trim`: the unit's contract refuses a seam line parallel to the ellipse's plane.
fn cert_p2_row(inputs: &Value, vals: &Values) -> Verdict {
    let cone = &inputs["cone"];
    let build = first(vals, "build").unwrap();
    if build != SUCCESS {
        return match degenerate(cone) {
            Some(_) => Ok(()),
            None => fail(format!(
                "construction refused ({build}) of a cone the inputs define"
            )),
        };
    }
    let Some((a, heights)) = input_cone(cone) else {
        return fail("no closed form for this mode");
    };
    let op = &inputs["op"];
    let status = first(vals, "status");
    let out: &[f64] = vals.get("out").map_or(&[], Vec::as_slice);
    let mode = &cone["mode"];
    let swap = mode["swap"].as_bool().unwrap_or(false);
    let inside = mode["inside"].as_bool().unwrap_or(false);
    let state_tol =
        |r: &mut Rd, apex_end: bool, tol: Option<()>| -> Result<(State, Analytic), Fail> {
            let s = read_state_p2(r);
            check(r.rest() == 0, "trailing values")?;
            cert_state_with(&s, apex_end, tol.is_none())?;
            let b = Analytic::of_state(&s);
            Ok((s, b))
        };
    let state_with = |r: &mut Rd, apex_end: bool| state_tol(r, apex_end, None);
    let state = |r: &mut Rd| state_with(r, true);
    match op["kind"].as_str().unwrap() {
        "offset" => {
            let d = op["distance"].as_f64().unwrap();
            check(
                status == Some(SUCCESS),
                format!("offset refused ({status:?})"),
            )?;
            let mut r = Rd::new(out);
            check(r.f() == 1.0, "no offset surface")?;
            let (s, b) = state_with(&mut r, heights.is_some())?;
            let sign = (if swap { -1.0 } else { 1.0 }) * (if inside { -1.0 } else { 1.0 });
            let th = a.semi * PI / 180.0;
            let (sn, cs) = (det::sin(th), det::cos(th));
            let off_r = cs * sign * d;
            let off_z = -sn * sign * d;
            let mut want = a;
            want.r0 = a.r0 + a.t * -off_z + off_r;
            let mut e = gamma(16) * d.abs() * (1.0 + a.t.abs());
            // An unbounded cone runs from its apex `ha` to the infinite end. Offset, its apex
            // circle (radius 0) moves to radius `off_r` at `ha + off_z`; if that radius has
            // the opposite sign to the infinite side's, the offset crosses its own apex
            // there and is cut at it. The cut cone's origin is computed from the domain's
            // heights (`SM_INFINITE_PARAMETER` in size), so it is known to `gamma(8)` of
            // them.
            let inf = INFINITE_PARAMETER;
            let unb = (heights.is_none() && !a.cylinder()).then(|| {
                let (ha, _) = a.apex_h();
                let inf_side = if a.t * (inf - ha) > 0.0 { 1.0 } else { -1.0 };
                let r_inf = a.radius(ha + inf_side * 1.0);
                let cut = off_r != 0.0 && (off_r < 0.0) != (r_inf < 0.0);
                (ha, cut)
            });
            if let Some((ha, true)) = unb {
                e += gamma(8) * (inf + ha.abs());
            }
            let map = same_cone(&want, &b, e)?;
            if let Some((ha, cut)) = unb {
                // the finite end (the infinite end is `±SM_INFINITE_PARAMETER` exactly,
                // which the state certificate checks)
                let [_, v0, _, v1] = s.dom;
                let fin = if v0.abs() == inf { v1 } else { v0 };
                let (h, sg) = map;
                let got = h + sg * fin;
                let w = if cut { -want.r0 / want.t } else { ha + off_z };
                let reach = ha.abs() + d.abs() + mc(a.o) + mc(b.o) + fin.abs();
                within(
                    "offset's finite end",
                    (got - w).abs(),
                    gamma(64) * reach + e,
                )?;
            }
            if let Some([v0, v1]) = heights {
                // each end circle moved; one whose radius changes sign is cut at the apex
                let (r0e, r1e) = (a.radius(v0) + off_r, a.radius(v1) + off_r);
                let (mut w0, mut w1) = (v0 + off_z, v1 + off_z);
                if (r0e < 0.0) != (r1e < 0.0) && !a.cylinder() {
                    let apex = -want.r0 / want.t;
                    if r0e < 0.0 {
                        w0 = apex;
                    } else {
                        w1 = apex;
                    }
                }
                let reach = v0.abs() + v1.abs() + d.abs() + mc(a.o) + mc(b.o);
                same_heights(&b, map, [w0, w1], gamma(64) * reach + approx_tol(op))?;
            }
            Ok(())
        }
        "recognize" | "rebuild" => {
            let kind = op["kind"].as_str().unwrap();
            let edit = &op["edit"];
            let mut want = a;
            let rigid = match edit["kind"].as_str().unwrap() {
                "none" | "scale_last" => true,
                "move_all" => {
                    let ax = edit["axis"].as_array().unwrap();
                    let axis = [
                        ax[0].as_f64().unwrap(),
                        ax[1].as_f64().unwrap(),
                        ax[2].as_f64().unwrap(),
                    ];
                    let t = edit["t"].as_array().unwrap();
                    let t = Vec3::new(
                        t[0].as_f64().unwrap(),
                        t[1].as_f64().unwrap(),
                        t[2].as_f64().unwrap(),
                    );
                    let m = rotation(axis, edit["angle"].as_f64().unwrap());
                    let rot = |p: Vec3| {
                        Vec3::new(
                            m[0] * p.x + m[1] * p.y + m[2] * p.z,
                            m[3] * p.x + m[4] * p.y + m[5] * p.z,
                            m[6] * p.x + m[7] * p.y + m[8] * p.z,
                        )
                    };
                    want.o = rot(a.o) + t;
                    want.z = rot(a.z);
                    want.x = rot(a.x);
                    true
                }
                _ => false,
            };
            if kind == "recognize" {
                check(rigid, "recognition of an edit that is no rigid move")?;
                check(status == Some(1.0), "the cone's own NURB not recognised")?;
                let mut r = Rd::new(out);
                check(r.f() == 1.0, "no recognised cone")?;
                let (s, b) = state(&mut r)?;
                // the recognised placement is computed from the net's points: its origin
                // is known to their rounding, `gamma(64)` of their reach
                let reach = s.form.as_ref().map_or(0.0, form_reach);
                same_cone(&want, &b, gamma(64) * reach).map(|_| ())
            } else if edit["kind"] == "move_last" {
                // The last row moved rigidly by `t` and `angle` degrees: every control
                // point of the edited net is within `Δ = |t| + |angle| |p|` (radians, `p`
                // the point) of the input cone's, so the net is within `Δ` of that cone
                // (the convex hull: same weights). The rebuild's contract
                // (`RebuildSTEPFromNURBParameters`) accepts a net whose end circles are
                // circles and coaxial within its scaled zero, `1000 · 100 · SM_EFF_ZERO ·
                // (1 + |corner₀| + |corner₁|)` (the NURB domain's corner points, the net's
                // corner control points), and keeps the net: with `Δ` well inside that
                // tolerance a refusal is wrong, and the answer is right when its net is
                // within the tolerance of its cone (by [`moved_net_distance`]).
                let ax = edit["axis"].as_array().unwrap();
                let axis = [
                    ax[0].as_f64().unwrap(),
                    ax[1].as_f64().unwrap(),
                    ax[2].as_f64().unwrap(),
                ];
                let tv = edit["t"].as_array().unwrap();
                let tl = Vec3::new(
                    tv[0].as_f64().unwrap(),
                    tv[1].as_f64().unwrap(),
                    tv[2].as_f64().unwrap(),
                );
                let ang = (edit["angle"].as_f64().unwrap() * PI / 180.0).abs();
                let _ = axis;
                let scaled_zero = |s: &State| -> f64 {
                    let Some(form) = s.form.as_ref() else {
                        return 0.0;
                    };
                    let (_, _, cps) = form.control_points().unwrap();
                    let e = |c: &CPoint| {
                        let w = if c.w == NOW { 1.0 } else { c.w };
                        mc(Vec3::new(c.x, c.y, c.z) * (1.0 / w))
                    };
                    let (c0, c1) = (cps.first().map_or(0.0, e), cps.last().map_or(0.0, e));
                    1000.0 * 100.0 * EFF_ZERO * (1.0 + c0 + c1) * (1.0 - gamma(8))
                };
                let delta = |s: &State| {
                    let reach = s.form.as_ref().map_or(0.0, form_reach) * 3.0_f64.sqrt();
                    tl.length() + ang * reach
                };
                if status != Some(SUCCESS) {
                    // (the state after a refusal is the edited net's cone, its form the net)
                    let s = read_state_p2(&mut Rd::new(out));
                    let (d, z) = (delta(&s), scaled_zero(&s));
                    return if d <= z {
                        fail(format!("rebuild refused ({status:?}) of a net {d:e} from a cone, within its tolerance {z:e}"))
                    } else {
                        fail(format!(
                            "no certificate: the edit {d:e} is beyond the tolerance {z:e}"
                        ))
                    };
                }
                let mut r = Rd::new(out);
                let (s, b) = state_tol(&mut r, true, Some(()))?;
                let z = scaled_zero(&s);
                let d = delta(&s);
                within("the edit's size against the rebuild's tolerance", d, z)?;
                let dist = moved_net_distance(&a, &b, d);
                within("the kept net's distance from the rebuilt cone", dist, z)?;
                let pc = s
                    .polar_curve
                    .as_ref()
                    .map(CurveData::points)
                    .unwrap_or_default();
                let Some(cd) = circle_distance(&pc, &b) else {
                    return fail("the polar curve is no 9-point rational circle");
                };
                within("the polar circle's distance from the rebuilt cone", cd, z)
            } else {
                check(
                    status == Some(SUCCESS),
                    format!("rebuild refused ({status:?})"),
                )?;
                let mut r = Rd::new(out);
                let (_, b) = state(&mut r)?;
                if rigid {
                    let map = same_cone(&want, &b, gamma(16))?;
                    if let Some(hs) = heights {
                        let reach = hs[1].abs() + mc(a.o) + mc(b.o) + mc(want.o);
                        same_heights(&b, map, hs, gamma(64) * reach)?;
                    }
                }
                Ok(())
            }
        }
        "trim" => {
            // the unit's input contract: a seam line parallel to the ellipse's plane leaves
            // upstream's intersection unset; the refusal is the contract's answer
            let v = |k: &str| {
                let a = op[k].as_array().unwrap();
                Vec3::new(
                    a[0].as_f64().unwrap(),
                    a[1].as_f64().unwrap(),
                    a[2].as_f64().unwrap(),
                )
            };
            let n = v("x").cross(v("y"));
            let seam = a.t * a.x + a.z;
            let parallel = n.dot(seam).abs() <= gamma(16) * n.length() * seam.length();
            if status == Some(-1002.0) {
                return check(parallel, "trim refused with the seam crossing the plane");
            }
            check(
                !parallel,
                "trim answered with the seam in the plane's direction",
            )?;
            check(
                status == Some(SUCCESS),
                format!("trim refused ({status:?})"),
            )?;
            cert_trim(&a, heights, op, out)
        }
        k => fail(format!("no certificate for op {k}")),
    }
}

/// A bound on the distance from the rebuilt cone `c1` of a net within `delta` of the
/// unbounded input cone `c0`'s net (the net's heights on `c0` from its apex to the
/// infinite end). A point `P0` of `c0` at height `v` is `c(v) + R0(v) e`, `e` a unit
/// vector across `c0`'s axis; with `θ` the angle between the axes, `d⊥(v)` the distance of
/// `c(v)` from `c1`'s axis and `hc(v)` its height on `c1`, `P0`'s distance from `c1`'s
/// axis is within `d⊥ + |R0| (1 - cos θ)` of `|R0|` and its height within `|R0| sin θ` of
/// `hc`, so its radial distance from `c1` (at least its distance) is at most
/// `| |R0(v)| - |R1(hc(v))| | + d⊥(v) + |R0(v)| (1 - cos θ) + |t1| |R0(v)| sin θ`, the first
/// term at most `|R0(v) - σ R1(hc(v))|` for either fixed sign `σ`. Each term is then
/// convex in `v` (`|·|` of an affine map, `R0` of one sign from the apex on, a norm of an
/// affine map), so the largest is at an end of the heights: evaluated at both, plus the
/// computation's rounding, plus `delta`.
fn moved_net_distance(c0: &Analytic, c1: &Analytic, delta: f64) -> f64 {
    let inf = INFINITE_PARAMETER;
    let (ha, _) = c0.apex_h();
    let far = if c0.t * (inf - ha) > 0.0 { inf } else { -inf };
    let cos = c0.z.dot(c1.z).abs().min(1.0);
    let sin = c0.z.cross(c1.z).length();
    let hc_of = |v: f64| (c0.o + v * c0.z - c1.o).dot(c1.z);
    let sigma = if c0.radius(far) * c1.radius(hc_of(far)) < 0.0 {
        -1.0
    } else {
        1.0
    };
    let at = |v: f64| {
        let c = c0.o + v * c0.z;
        let w = c - c1.o;
        let hc = w.dot(c1.z);
        let dp = (w - hc * c1.z).length();
        let (r0s, r1s) = (c0.radius(v), c1.radius(hc));
        let (r0, r1) = (r0s.abs(), r1s.abs());
        let term = (r0s - sigma * r1s).abs() + dp + r0 * (1.0 - cos) + c1.t.abs() * r0 * sin;
        let size = mc(c) + mc(c1.o) + r0 + r1 + hc.abs() * (1.0 + c1.t.abs());
        term + gamma(64) * size
    };
    at(ha).max(at(far)) + delta
}

/// A bound on the distance from the cone `c1` of the 9-point rational circle (weights
/// `1, √½, ...`) with Euclidean control points `p`: with `C` the midpoint of `p₀, p₄`,
/// `u = p₀ - C`, `v = p₂ - C`, the control points of the circle of centre `C`, radius
/// `|u|`, in the plane of `u, v` are `C ± u ± v` (corners) and `C ± u`, `C ± v`, given
/// `|u| = |v|` and `u ⊥ v`; the curve is within the largest control point's departure
/// from those (a convex combination with the same weights) plus `| |u| - |v| |` and
/// `|u·v| / |u|` (the ellipse's from the circle) of that circle. The circle's points are
/// at distance `r` from `C` across its normal `n`: as in [`moved_net_distance`], within
/// `|r - σ R1(hc)| + d⊥ + r (1 - cos θ) + |t1| r sin θ` of `c1` radially (`θ` between `n`
/// and the axis), the smaller over `σ = ±1`, plus the rounding.
fn circle_distance(p: &[Vec3], c1: &Analytic) -> Option<f64> {
    if p.len() != 9 {
        return None;
    }
    let c = (p[0] + p[4]) * 0.5;
    let (u, v) = (p[0] - c, p[2] - c);
    let ideal = [
        c + u,
        c + u + v,
        c + v,
        c - u + v,
        c - u,
        c - u - v,
        c - v,
        c + u - v,
        c + u,
    ];
    let dep = p
        .iter()
        .zip(ideal.iter())
        .map(|(a, b)| (*a - *b).length())
        .fold(0.0_f64, f64::max);
    let r = u.length();
    if !(r > 0.0) {
        return None;
    }
    let shape = (r - v.length()).abs() + u.dot(v).abs() / r;
    let n = u.cross(v);
    let n = n * (1.0 / n.length());
    let cos = n.dot(c1.z).abs().min(1.0);
    let sin = n.cross(c1.z).length();
    let w = c - c1.o;
    let hc = w.dot(c1.z);
    let dp = (w - hc * c1.z).length();
    let r1 = c1.radius(hc);
    let radial = (r - r1).abs().min((r + r1).abs());
    let size = p.iter().fold(0.0_f64, |m, q| m.max(mc(*q))) + mc(c1.o) + r + r1.abs();
    Some(radial + dp + r * (1.0 - cos) + c1.t.abs() * r * sin + dep + shape + gamma(64) * size)
}

/// `approx_tol3d` of the op's `tol` (the offset's truncation test).
fn approx_tol(op: &Value) -> f64 {
    op["tol"].as_f64().unwrap_or(0.0).abs().max(1e-6)
}

/// The rotation by `angle` degrees about `axis` (`surface_cone_p2::rotation`, row-major:
/// the edit both sides were given).
fn rotation(axis: [f64; 3], angle: f64) -> [f64; 9] {
    let [ax, ay, az] = axis;
    let l = (ax * ax + ay * ay + az * az).sqrt();
    if l <= 0.0 {
        return [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    }
    let (x, y, z) = (ax / l, ay / l, az / l);
    let a = angle * PI / 180.0;
    let (s, c) = det::sin_cos(a);
    let t = 1.0 - c;
    [
        t * x * x + c,
        t * x * y - s * z,
        t * x * z + s * y,
        t * x * y + s * z,
        t * y * y + c,
        t * y * z - s * x,
        t * x * z - s * y,
        t * y * z + s * x,
        t * z * z + c,
    ]
}

/// A `trim` answer by closed form: an ellipse whose axial heights are all more than `tol`
/// outside the patch's is more than `tol` from every point of the patch, so it meets
/// neither trim circle nor a seam line: no interval.
fn cert_trim(a: &Analytic, heights: Option<[f64; 2]>, op: &Value, out: &[f64]) -> Verdict {
    let v = |k: &str| {
        let a = op[k].as_array().unwrap();
        Vec3::new(
            a[0].as_f64().unwrap(),
            a[1].as_f64().unwrap(),
            a[2].as_f64().unwrap(),
        )
    };
    let Some([h0, h1]) = heights else {
        return fail("no closed form for an unbounded cone's trim");
    };
    // the ellipse `c + rx cos(θ) X + ry sin(θ) Y` (X, Y its placement's canonical axes)
    // has axial heights `h + rx cos(θ) X·z + ry sin(θ) Y·z`, within `±e` of `h`,
    // `e = |(rx X·z, ry Y·z)|`; a plane across the axis has `e = 0`. (`e` is within a
    // few roundings; the slack `b` covers them.)
    let ux = v("x") * (1.0 / v("x").length());
    let nz = v("x").cross(v("y"));
    let uy = (nz * (1.0 / nz.length())).cross(ux);
    let (rx, ry) = (op["rx"].as_f64().unwrap(), op["ry"].as_f64().unwrap());
    let e = det::hypot(rx * ux.dot(a.z), ry * uy.dot(a.z));
    let h = (v("origin") - a.o).dot(a.z);
    let tol = op["tol"].as_f64().unwrap();
    let b = gamma(16) * (mc(v("origin")) + mc(a.o) + rx.abs() + ry.abs());
    let (lo, hi) = (h0.min(h1), h0.max(h1));
    let apart = h + e < lo - tol - b || h - e > hi + tol + b;
    // or the ellipse is inside the solid cone, away from its surface: the ellipse's
    // distance from the axis is at most `|c⊥| + |(rx |X⊥|, ry |Y⊥|)|` (`⊥` the part across
    // the axis; Cauchy-Schwarz on `rx cos(θ) X⊥ + ry sin(θ) Y⊥`), and on heights
    // `[h - e, h + e]` where the radius `R` keeps one sign it is at least the smaller end's
    // `|R|`. A point at distance `ρ < |R|` from the axis is inside the convex solid nappe,
    // `(|R| - ρ) cos(semi)` from its boundary, which holds the patch, its trim circles
    // and seam lines.
    let inside = || {
        let perp = |w: Vec3| w - w.dot(a.z) * a.z;
        let c = v("origin") - a.o;
        let rho = perp(c).length() + det::hypot(rx * perp(ux).length(), ry * perp(uy).length());
        let (r_lo, r_hi) = (a.radius(h - e), a.radius(h + e));
        let r_min = r_lo.abs().min(r_hi.abs());
        let margin = (r_min - rho) * det::cos(a.semi * PI / 180.0);
        (r_lo > 0.0) == (r_hi > 0.0) && r_lo != 0.0 && margin > tol + b * (1.0 + a.t.abs())
    };
    check(
        apart || inside(),
        "no closed form: the ellipse's heights reach the patch",
    )?;
    check(
        out == [0.0],
        format!("intervals {out:?} where the ellipse misses the patch"),
    )
}

fn side(
    inputs: &Value,
    out: &Output,
    ctx: Option<&Ctx>,
    surface_api: bool,
    same_angle: bool,
) -> Verdict {
    if let Some(e) = &out.error {
        return fail(format!("error {e}"));
    }
    cert_cone_row(inputs, &out.values, ctx, surface_api, same_angle)
}

/// Whether both sides answer the same STEP angle (the first output, bit-equal): the row
/// moved in its height only.
fn same_angle(new: &Output, old: &Output) -> bool {
    let a = |o: &Output| o.values.get("out").and_then(|v| v.first().copied());
    let st = |o: &Output| first(&o.values, "status");
    st(new) == Some(SUCCESS) && st(old) == Some(SUCCESS) && a(new).is_some() && a(new) == a(old)
}

// ---- Coons refinement replay (WR-713 tie rule) --------------------------------------------

impl Rv {
    fn exact(v: f64) -> Self {
        Rv { v, e: 0.0 }
    }
    /// `|A / B - a / b| <= (e_a + |a / b| e_b) / (|b| - e_b)`, unbounded where the divisor's
    /// enclosure holds zero.
    fn div(self, q: Self) -> Self {
        let v = self.v / q.v;
        let lo = q.v.abs() - q.e;
        if lo.is_nan() || lo <= 0.0 {
            return Rv {
                v,
                e: f64::INFINITY,
            };
        }
        Rv {
            v,
            e: err_up((self.e + v.abs() * q.e) / lo + UNIT_RO * v.abs()),
        }
    }
    /// `|sqrt X - sqrt x| <= min(e / sqrt x, sqrt e)` for `X >= 0` within `e` of `x`.
    fn sqrt(self) -> Self {
        let v = self.v.max(0.0).sqrt();
        let prop = if v > 0.0 {
            (self.e / v).min(self.e.sqrt())
        } else {
            self.e.sqrt()
        };
        Rv {
            v,
            e: err_up(prop + UNIT_RO * v),
        }
    }
}

/// The span `a` the evaluator takes at `t` (`KnotVector::find_span`, `Side::Left` inside:
/// `t_a <= t < t_(a+1)`; at or past either end the end span, moved inwards past spans
/// shorter than `1e-12`). Written here from the definition, not called.
fn eval_span(knots: &[f64], p: usize, t: f64) -> usize {
    let m = knots.len() - 1;
    if t >= knots[m - p] {
        let mut i = m - p - 1;
        while knots[i + 1] < knots[i] + 1e-12 && i > p {
            i -= 1;
        }
        return i;
    }
    if t <= knots[p] {
        let mut i = p;
        while knots[i + 1] < knots[i] + 1e-12 && i < m - p - 1 {
            i += 1;
        }
        return i;
    }
    (p..m - p)
        .find(|&a| knots[a] <= t && t < knots[a + 1])
        .unwrap()
}

/// Enclosures of the B-spline basis values `N_(a-p+k, p)(t)` and their first derivatives on
/// span `a`, by the Cox-de Boor recurrence and `N' = p (N_(i, p-1) / (t_(i+p) - t_i) -
/// N_(i+1, p-1) / (t_(i+p+1) - t_(i+1)))`, in [`Rv`] arithmetic from the exact knots.
fn basis_rv(knots: &[f64], p: usize, a: usize, t: f64) -> (Vec<Rv>, Vec<Rv>) {
    let kn = |i: usize| Rv::exact(knots[i]);
    let tt = Rv::exact(t);
    let mut n = vec![Rv::exact(1.0)];
    let mut lower = Vec::new();
    for d in 1..=p {
        if d == p {
            lower.clone_from(&n);
        }
        // n[k] is N_(a-(d-1)+k, d-1); next[k] is N_(a-d+k, d)
        let next: Vec<Rv> = (0..=d)
            .map(|k| {
                let i = a + k - d;
                let mut s = Rv::exact(0.0);
                if k >= 1 {
                    let w = tt.sub(kn(i)).div(kn(i + d).sub(kn(i)));
                    s = s.add(w.mul(n[k - 1]));
                }
                if k < d {
                    let w = kn(i + d + 1).sub(tt).div(kn(i + d + 1).sub(kn(i + 1)));
                    s = s.add(w.mul(n[k]));
                }
                s
            })
            .collect();
        n = next;
    }
    let pf = Rv::exact(p as f64);
    let d = (0..=p)
        .map(|k| {
            if p == 0 {
                return Rv::exact(0.0);
            }
            let i = a + k - p;
            let mut s = Rv::exact(0.0);
            if k >= 1 {
                s = s.add(pf.mul(lower[k - 1]).div(kn(i + p).sub(kn(i))));
            }
            if k < p {
                s = s.sub(pf.mul(lower[k]).div(kn(i + p + 1).sub(kn(i + 1))));
            }
            s
        })
        .collect();
    (n, d)
}

/// Enclosures of the exact first derivatives of a non-rational patch at `uv`, on the spans
/// the evaluator takes: `sum N'_i(u) N_j(v) P_ij` and `sum N_i(u) N'_j(v) P_ij`.
fn exact_derivatives(patch: &BSplineSurface, uv: Point2) -> Result<(R3, R3), Fail> {
    let Some(nurb) = patch.gw_nurb() else {
        return fail("no NURB form");
    };
    check(!patch.is_rational(), "a rational candidate")?;
    let (p, q) = (nurb.p(), nurb.q());
    let (uk, vk) = (nurb.u_knots(), nurb.v_knots());
    let (a, b) = (eval_span(uk, p, uv.x), eval_span(vk, q, uv.y));
    let (nu, du) = basis_rv(uk, p, a, uv.x);
    let (nv, dv) = basis_rv(vk, q, b, uv.y);
    let zero = R3::new(Vec3::new(0.0, 0.0, 0.0), 0.0);
    let (mut su, mut sv) = (zero, zero);
    for (k, (nuk, duk)) in nu.iter().zip(&du).enumerate() {
        for (l, (nvl, dvl)) in nv.iter().zip(&dv).enumerate() {
            let c = nurb.cpt(a - p + k, b - q + l).unwrap();
            let pt = R3::new(Vec3::new(c.x, c.y, c.z), 0.0);
            su = su.add(pt.times(duk.mul(*nvl)));
            sv = sv.add(pt.times(nuk.mul(*dvl)));
        }
    }
    Ok((su, sv))
}

/// `|computed - exact|` at most, for `exact` in the enclosure `enc`.
fn enclosed_distance(computed: Vec3, enc: R3) -> f64 {
    let c = |x: f64, r: Rv| (x - r.v).abs() + r.e;
    let (a, b, d) = (
        c(computed.x, enc.x),
        c(computed.y, enc.y),
        c(computed.z, enc.z),
    );
    (a * a + b * b + d * d).sqrt() * (1.0 + gamma(8))
}

/// What a replay of one refinement found.
#[derive(Debug, Default, Clone, Copy)]
struct Replay {
    /// stations whose bound was checked
    stations: usize,
    /// the largest checked derivative distance over its claimed bound
    deriv_ratio: f64,
    /// the largest half-width of an angle enclosure over the station's claimed bound
    angle_ratio: f64,
    /// candidates taken; refused for a side certainly worse than a held copy's, for a
    /// station a held copy certifies and the candidate does not, or for a comparison the
    /// bounds leave undecided
    taken: usize,
    worse: usize,
    uncertified: usize,
    undecided: usize,
}

/// The next double above `x` (the library's `next_up`, written again here).
fn up(x: f64) -> f64 {
    if x.is_nan() || x == f64::INFINITY {
        x
    } else if x == 0.0 {
        f64::from_bits(1)
    } else if x > 0.0 {
        f64::from_bits(x.to_bits() + 1)
    } else {
        f64::from_bits(x.to_bits() - 1)
    }
}

/// The next double below `x`.
fn down(x: f64) -> f64 {
    -up(-x)
}

/// A candidate's station angles per side (left, bottom, right, top), `None` for a side
/// with no station; a station is `Some((angle, err))` where it is certified (an angle,
/// and a bound below `PI / 2`).
type Sides = [Option<Vec<Option<(f64, f64)>>>; 4];

/// The [`Sides`] of a logged candidate, read from its stations.
fn candidate_sides(c: &CoonsCandidateLog) -> Sides {
    let mut sides: Sides = [None, None, None, None];
    for st in &c.stations {
        let at = st.angle.filter(|_| st.err < 0.5 * PI).map(|a| (a, st.err));
        sides[st.side].get_or_insert_with(Vec::new).push(at);
    }
    sides
}

/// The largest computed angle over every certified station (the loop's stop test).
fn sides_sup(s: &Sides) -> Option<f64> {
    s.iter()
        .flatten()
        .flatten()
        .flatten()
        .map(|&(a, _)| a)
        .fold(None, |m: Option<f64>, a| {
            Some(m.map_or(a, |m| if a > m { a } else { m }))
        })
}

/// The largest angle of one side of candidate `c` and of held copy `h`, at the same
/// stations, over those `h` certifies, each as `(max lo, max hi)` of the enclosures
/// `[angle - err, angle + err]` rounded outwards: `Ok(None)` where `h` certifies none,
/// `Err(())` where `c` lacks a station `h` certifies.
fn side_max(
    c: &[Option<(f64, f64)>],
    h: &[Option<(f64, f64)>],
) -> Result<Option<((f64, f64), (f64, f64))>, ()> {
    if c.len() != h.len() {
        return Err(());
    }
    let (mut cl, mut ch, mut hl, mut hh) = (
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    let mut any = false;
    for (sc, sh) in c.iter().zip(h) {
        let Some((ha, he)) = *sh else { continue };
        let Some((ca, ce)) = *sc else {
            return Err(());
        };
        cl = cl.max(down(ca - ce));
        ch = ch.max(up(ca + ce));
        hl = hl.max(down(ha - he));
        hh = hh.max(up(ha + he));
        any = true;
    }
    Ok(any.then_some(((cl, ch), (hl, hh))))
}

/// One side of a candidate `c` against a held copy `h` ([`side_max`]): where the
/// candidate's largest lower end is above the copy's largest upper end it is certainly
/// worse, `"rises"` if its largest upper end is still at most the copy's largest lower
/// end plus `res` (one step down), else `"worse"`; otherwise `"no worse"` where that
/// holds, `"uncertified"` where it lacks a station, else `"undecided"`.
fn side_verdict(c: &[Option<(f64, f64)>], h: &[Option<(f64, f64)>], res: f64) -> &'static str {
    let ((cl, ch), (hl, hh)) = match side_max(c, h) {
        Err(()) => return "uncertified",
        Ok(None) => return "no worse",
        Ok(Some(m)) => m,
    };
    let within = ch <= down(hl + res);
    if cl.is_nan() || hh.is_nan() {
        "worse"
    } else if cl > hh {
        if within {
            "rises"
        } else {
            "worse"
        }
    } else if within {
        "no worse"
    } else {
        "undecided"
    }
}

/// The verdict of candidate `c` against held copy `h`: the first of `"worse"`,
/// `"uncertified"`, `"undecided"` some side has; else, where a side `"rises"`, `"no
/// worse"` only if the candidate's largest upper end over every side both measure is at
/// most the copy's largest lower end less `res` (one step down), `"worse"` otherwise; else
/// `"no worse"`.
fn candidate_verdict(c: &Sides, h: &Sides, res: f64) -> &'static str {
    let mut found = Vec::new();
    let (mut c_hi, mut h_lo) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (cs, hs) in c.iter().zip(h) {
        found.push(match (cs, hs) {
            (_, None) => "no worse",
            (None, Some(hs)) if hs.iter().all(Option::is_none) => "no worse",
            (None, Some(_)) => "uncertified",
            (Some(cs), Some(hs)) => {
                if let Ok(Some(((_, ch), (hl, _)))) = side_max(cs, hs) {
                    c_hi = c_hi.max(ch);
                    h_lo = h_lo.max(hl);
                }
                side_verdict(cs, hs, res)
            }
        });
    }
    if let Some(v) = ["worse", "uncertified", "undecided"]
        .into_iter()
        .find(|v| found.contains(v))
    {
        return v;
    }
    if found.contains(&"rises") && !(h_lo.is_finite() && c_hi <= down(h_lo - res)) {
        return "worse";
    }
    "no worse"
}

/// Replays one `make_coons_patch_tangent_to_sides` run from its log and checks it.
///
/// * **Common stations.** Every candidate is measured at the same `(side, s)` stations as
///   the bare patch, bit for bit, so each comparison is of angles at the same points.
/// * **Honest bounds.** At every certified station (`err < PI / 2`), the exact first
///   derivatives of the candidate (enclosed here, [`exact_derivatives`]) are within the
///   claimed `(eu, ev)` of the computed ones, and the exact angle between the exact patch
///   normal `du x dv` and the logged side normal (its cosine enclosed in [`Rv`] arithmetic,
///   then `acos` of the ends, widened for the library's rounding) is inside
///   `[angle - err, angle + err]`. So each station's bound encloses the angle its own
///   arithmetic is meant to measure; nothing is taken from the kernel's bound arithmetic.
/// * **The rule.** From the bare patch, each refined copy (spans 8, 16, 32, 64, while the
///   last copy held reads above the target) is taken only where it is shown no worse
///   ([`candidate_verdict`]) than the bare patch and every copy taken before it, at the
///   log's resolution, which is `SM_CONTINUITY_ANGLE`; the replayed choice is the logged
///   one, and the loop stops where it did.
fn replay_coons(log: &CoonsTangencyLog) -> Result<Replay, Fail> {
    let mut r = Replay::default();
    check(
        log.resolution.to_bits() == (0.5 * (PI / 180.0)).to_bits(),
        format!("the resolution is {}", log.resolution),
    )?;
    let Some(bare) = log.candidates.first() else {
        return fail("no candidate");
    };
    let at = |c: &CoonsCandidateLog| -> Vec<(usize, u64)> {
        c.stations
            .iter()
            .map(|st| (st.side, st.s.to_bits()))
            .collect()
    };
    let bare_at = at(bare);
    for (ci, c) in log.candidates.iter().enumerate() {
        check(
            at(c) == bare_at,
            format!("candidate {ci} is not measured at the bare patch's stations"),
        )?;
        for st in &c.stations {
            check(st.side < 4, "a station on no side")?;
            if st.angle.is_none() || st.err >= 0.5 * PI {
                continue;
            }
            let (Some(angle), Some((_, m)), Some((eu, ev))) =
                (st.angle, st.neighbour, st.derivative_errors)
            else {
                return fail(format!(
                    "candidate {ci}: a counted station without its data"
                ));
            };
            let (dux, dvx) = exact_derivatives(&c.patch, st.uv)?;
            let (gu, gv) = (enclosed_distance(st.du, dux), enclosed_distance(st.dv, dvx));
            within(&format!("candidate {ci} du at {:?}", st.uv), gu, eu)?;
            within(&format!("candidate {ci} dv at {:?}", st.uv), gv, ev)?;
            r.deriv_ratio = r.deriv_ratio.max(gu / eu).max(gv / ev);
            // the angle in [0, PI / 2] between the lines of `du x dv` and `m`, as
            // `atan(|n x m| / |n . m|)`: well conditioned at every angle, unlike `acos`
            let nx = dux.cross(dvx);
            let mr = R3::new(m, 0.0);
            let y = nx.cross(mr);
            let y = y.dot(y).sqrt();
            let x = nx.dot(mr);
            let (ylo, yhi) = ((y.v - y.e).max(0.0), y.v + y.e);
            let (xlo, xhi) = ((x.v.abs() - x.e).max(0.0), x.v.abs() + x.e);
            let lo = det::atan(ylo / xhi) * (1.0 - 8.0 * UNIT_RO);
            let hi = if xlo > 0.0 {
                (det::atan(yhi / xlo) * (1.0 + 8.0 * UNIT_RO)).min(0.5 * PI * (1.0 + 4.0 * UNIT_RO))
            } else {
                0.5 * PI * (1.0 + 4.0 * UNIT_RO)
            };
            check(
                lo.is_finite() && hi.is_finite(),
                format!("candidate {ci}: no angle enclosure at {:?}", st.uv),
            )?;
            within(
                &format!("candidate {ci} angle at {:?} below its bound", st.uv),
                angle - lo,
                st.err,
            )?;
            within(
                &format!("candidate {ci} angle at {:?} above its bound", st.uv),
                hi - angle,
                st.err,
            )?;
            r.angle_ratio = r.angle_ratio.max((angle - lo).max(hi - angle) / st.err);
            r.stations += 1;
        }
        let want = if ci == 0 { 0 } else { 8 << (ci - 1) };
        check(
            c.spans == want,
            format!("candidate {ci} has {} spans", c.spans),
        )?;
    }
    let mut held: Vec<Sides> = vec![candidate_sides(bare)];
    let mut chosen = None;
    let mut next = 1;
    while next <= 4 && sides_sup(held.last().unwrap()).is_some_and(|m| m > log.target) {
        let Some(c) = log.candidates.get(next) else {
            return fail(format!(
                "the loop measured candidate {next}, the log has none"
            ));
        };
        let cs = candidate_sides(c);
        let verdict = held
            .iter()
            .map(|h| candidate_verdict(&cs, h, log.resolution))
            .find(|v| *v != "no worse")
            .unwrap_or("no worse");
        match verdict {
            "no worse" => {
                r.taken += 1;
                held.push(cs);
                chosen = Some(next);
            }
            "worse" => r.worse += 1,
            "uncertified" => r.uncertified += 1,
            _ => r.undecided += 1,
        }
        next += 1;
    }
    check(
        next == log.candidates.len(),
        format!(
            "the loop stops after {next} candidates, the log has {}",
            log.candidates.len()
        ),
    )?;
    check(
        chosen == log.chosen,
        format!("replayed choice {chosen:?}, logged {:?}", log.chosen),
    )?;
    Ok(r)
}

/// A candidate's values as the runners write a surface: control points `x y z w` row by
/// row, then the u and the v knots.
fn patch_values(c: &CoonsCandidateLog) -> Vec<f64> {
    let nurb = c.patch.gw_nurb().unwrap();
    let mut v = Vec::new();
    for row in nurb.cpts() {
        for p in row {
            v.extend([p.x, p.y, p.z, p.w]);
        }
    }
    v.extend_from_slice(nurb.u_knots());
    v.extend_from_slice(nurb.v_knots());
    v
}

/// Whether `hay` holds `needle` as a run, value by value within `rel` relatively.
fn holds_run(hay: &[f64], needle: &[f64], rel: f64) -> bool {
    let same =
        |a: f64, b: f64| a.to_bits() == b.to_bits() || (a - b).abs() <= rel * a.abs().max(b.abs());
    !needle.is_empty()
        && hay
            .windows(needle.len())
            .any(|w| w.iter().zip(needle).all(|(a, b)| same(*a, *b)))
}

/// An output's surfaces as one array: `out` (a B-rep's `put_surface` blocks) or each
/// `patch<j>_` of a corner blend, control points then knots.
fn surface_values(o: &Output) -> Vec<f64> {
    if let Some(out) = o.values.get("out") {
        return out.clone();
    }
    let n = o.values.get("patch_count").map_or(0, |c| c[0] as usize);
    let mut v = Vec::new();
    for j in 0..n {
        for k in ["cpts", "u", "v"] {
            v.extend_from_slice(&o.values[&format!("patch{j}_{k}")]);
        }
    }
    v
}

/// The index of the candidate whose patch `o` holds (`0` the bare patch), within `rel`.
fn candidate_in(log: &CoonsTangencyLog, o: &Output, rel: f64) -> Option<usize> {
    let hay = surface_values(o);
    log.candidates
        .iter()
        .position(|c| holds_run(&hay, &patch_values(c), rel))
}

/// The Coons logs of `case`'s run with the kernel's log on; the run's output must be
/// `new` bit for bit (keeping the log changes no result).
fn coons_logs(unit: &str, case: &goldens::case::Case, new: &Output) -> Vec<CoonsTangencyLog> {
    let mut k = Kernel::default();
    *k.coons_tangency_log.borrow_mut() = Some(Vec::new());
    let out = match unit {
        "nurbs.b_spline_surface_n_lib_p2" => nlib::run_case_on(&k, case).unwrap(),
        "topology.brep_p1" => bp1::run_case_on(&mut k, case).unwrap(),
        _ => return Vec::new(),
    };
    let bits = |o: &Output| -> Vec<(String, Vec<u64>)> {
        o.values
            .iter()
            .map(|(key, v)| (key.clone(), v.iter().map(|x| x.to_bits()).collect()))
            .collect()
    };
    assert_eq!(
        bits(&out),
        bits(new),
        "{}: the logged run differs",
        case.name
    );
    let logs = k.coons_tangency_log.borrow().clone().unwrap();
    logs
}

/// The replay certificate of a row whose corner patches went through the refinement: every
/// log replays ([`replay_coons`]), and `o` holds each log's chosen patch, bit for bit.
fn cert_coons_replay(logs: &[CoonsTangencyLog], o: &Output) -> Result<Replay, Fail> {
    check(!logs.is_empty(), "no refinement ran")?;
    let mut total = Replay::default();
    for (li, log) in logs.iter().enumerate() {
        let r = replay_coons(log).map_err(|f| Fail {
            why: format!("refinement {li}: {}", f.why),
            ..f
        })?;
        let want = log.chosen.unwrap_or(0);
        check(
            candidate_in(log, o, 0.0) == Some(want),
            format!("refinement {li}: the output does not hold candidate {want}"),
        )?;
        total.stations += r.stations;
        total.deriv_ratio = total.deriv_ratio.max(r.deriv_ratio);
        total.angle_ratio = total.angle_ratio.max(r.angle_ratio);
        total.taken += r.taken;
        total.worse += r.worse;
        total.uncertified += r.uncertified;
        total.undecided += r.undecided;
    }
    Ok(total)
}

/// The cases the replay is checked on outside the inventory: analytic028 (all sides tie:
/// the finest copy), analytic064 (two sides certainly worse on every copy: the bare
/// patch), analytic111 (three ties, then a certain loss at 64 spans: the 32-span copy, the
/// one main took), `vase_rev_corner_blend_3` (its sups tie within
/// the radian its left and right sides leave, but its bottom side, read to `3e-5`, is
/// certainly ten degrees worse on every copy: the bare patch) and `Endcap_corner_blend_3`
/// (likewise on its bottom side, 1.1353 against 1.0752 ± 3e-4: the bare patch, where
/// main took a refined copy).
#[test]
fn coons_refinements_replay_with_honest_bounds() {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    let want = [
        ("nurbs.b_spline_surface_n_lib_p2", "analytic028", Some(4)),
        ("nurbs.b_spline_surface_n_lib_p2", "analytic064", None),
        ("nurbs.b_spline_surface_n_lib_p2", "analytic111", Some(3)),
        ("topology.brep_p1", "vase_rev_corner_blend_3", None),
        ("topology.brep_p1", "Endcap_corner_blend_3", None),
    ];
    let mut seen = 0;
    for unit in registry() {
        for case in unit.cases(&ctx).unwrap() {
            let Some(&(_, _, chosen)) = want
                .iter()
                .find(|(u, c, _)| *u == unit.id() && *c == case.name)
            else {
                continue;
            };
            let new = unit.run_rust(&case).unwrap();
            let logs = coons_logs(unit.id(), &case, &new);
            assert_eq!(logs.len(), 1, "{}", case.name);
            let r = cert_coons_replay(&logs, &new)
                .unwrap_or_else(|f| panic!("{}: {}", case.name, f.why));
            println!("{} chosen {:?} {r:?}", case.name, logs[0].chosen);
            for c in &logs[0].candidates {
                let s = candidate_sides(c);
                let side_max = s.clone().map(|side| {
                    side.map(|v| {
                        v.iter()
                            .flatten()
                            .fold(
                                (f64::NEG_INFINITY, 0.0),
                                |m, &(a, e)| {
                                    if a > m.0 {
                                        (a, e)
                                    } else {
                                        m
                                    }
                                },
                            )
                    })
                });
                println!("  spans {} side maxima (angle, err) {side_max:?}", c.spans);
            }
            assert_eq!(logs[0].chosen, chosen, "{}", case.name);
            assert!(r.stations > 0);
            seen += 1;
        }
    }
    assert_eq!(seen, want.len());
}

/// The points where candidate `a` is certainly farther from tangent than candidate `b`:
/// the common stations `(side, s)` both certify where `a`'s enclosure of its exact angle
/// lies wholly above `b`'s. Each is a fact about the exact patches at that point (the
/// enclosures were shown honest by [`replay_coons`]), with nothing sampled in it.
fn witnesses(log: &CoonsTangencyLog, a: usize, b: usize) -> Vec<String> {
    let (ca, cb) = (&log.candidates[a], &log.candidates[b]);
    let mut out = Vec::new();
    for (x, y) in ca.stations.iter().zip(&cb.stations) {
        let (Some(ax), Some(ay)) = (x.angle, y.angle) else {
            continue;
        };
        if x.err >= 0.5 * PI || y.err >= 0.5 * PI {
            continue;
        }
        if down(ax - x.err) > up(ay + y.err) {
            out.push(format!(
                "side {} at s = {}: {ax} +- {:e} against {ay} +- {:e}",
                x.side, x.s, x.err, y.err
            ));
        }
    }
    out
}

/// A row whose corner patch moved because the refinement chose another copy: the new side
/// replays ([`cert_coons_replay`]); the old (main's) output must hold one of this run's
/// candidates (main refined the same patch the same way, and chose by a strict `<` of
/// uncertified maxima). The two choices are then compared point by point
/// ([`witnesses`]): a side fails where it is certainly farther from tangent than the
/// other at some common station and certainly nearer at none. Failing rests on a witness
/// point, a proof about the exact patches there. Passing is not a sup-norm bound: where
/// neither side has a witness, or each has one, both pass.
fn cert_coons_against_main(
    logs: &[CoonsTangencyLog],
    new: &Output,
    old: &Output,
) -> (Verdict, Verdict) {
    if let Err(f) = cert_coons_replay(logs, new) {
        return (Err(f), Ok(()));
    }
    let (mut vn, mut vo) = (Ok(()), Ok(()));
    for (li, log) in logs.iter().enumerate() {
        let i = log.chosen.unwrap_or(0);
        let Some(j) = candidate_in(log, old, 1e-12) else {
            return (
                fail(format!(
                    "refinement {li}: main's patch is none of this run's candidates"
                )),
                Ok(()),
            );
        };
        if i == j {
            continue;
        }
        let (wn, wo) = (witnesses(log, i, j), witnesses(log, j, i));
        let what = format!("refinement {li}: candidate {i} (new) against {j} (main)");
        if !wn.is_empty() && wo.is_empty() {
            vn = fail(format!(
                "{what}: the new patch is certainly farther from tangent at {} point(s), \
                 nearer at none; first {}",
                wn.len(),
                wn[0]
            ));
        } else if !wo.is_empty() && wn.is_empty() {
            vo = fail(format!(
                "{what}: main's patch is certainly farther from tangent at {} point(s), \
                 nearer at none; first {}",
                wo.len(),
                wo[0]
            ));
        }
    }
    (vn, vo)
}
