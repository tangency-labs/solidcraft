//! #18325: every golden element the exact iso-line drop moves is CLOSER to the exact value
//! of the ideal pipeline than main's, EQUAL to main's, or EQUAL-at-rounding: within the
//! a-priori bound of the kernel's own float evaluation, or (19 elements, MEASURED below)
//! within a measured bound of main's. No element is FARTHER, except as a MEASURED one:
//! two of those (a point solve's uv, 1 ulp) are farther from the exact iso line than
//! main's and are accepted on that measured bound alone.
//!
//! The fix makes `drop_curve` return the iso line `(f, t)` of a B-spline surface, with
//! ends exactly `(f, t0)` and `(f, t1)`, when the curve is that iso line, decided before the
//! station loop so the drop quality is measured on that exact line; and it seeds the
//! edge/face gap of a `CrvOnSurf` on the other surface itself at its own uv, the exact
//! foot (`heal/gap.rs` `own_foot`). Together with the edge's 3D iso curve that
//! `crv_trim`'s left-knot insertion now computes with `affine2` (#17896, the
//! `keep_operand_uv_trims` retirement), they move 2805 elements of 333 rows of
//! `topology.edgeuse_p2` (60), `topology.edgeuse_p3` (2673) and `topology.face_p3` (72),
//! and the 498 elements of 39 rows of six other units below.
//! `data/issue_18325_moved_rows.tsv` lists them (`E unit case key i main new class`, main's
//! and the new golden's values and the pinned class), main's whole vector of every row (`M`,
//! the bits) and main's old golden line (`O`), with the stations the drop quality maxima are
//! taken over (`DC`/`DU` the edge's 3D curve or the UV line of its `CrvOnSurf`, `DI` the
//! `drop_iso_curve` stations, `DG` the `surface_curve_max_distance_between` stations, as the
//! kernel evaluated them), the blade hub face's surface (`S`) and main's Gauss-Kronrod
//! decision tapes of the replayed mass faces (`T unit case face len hex`).
//!
//! The test replays every listed case through the goldens runner. Each row's vector must
//! match the new golden bit for bit; every index not listed must equal main's (`M`) bit for
//! bit; and main's block triple, recomputed from `M` as `golden.rs` encodes it, must equal
//! the old golden (`O`). The data lines are tied to the replayed model: `DC`/`DU` must be
//! the edge's curve (or its `CrvOnSurf`'s UV curve) and `S` the face's surface, bit for bit.
//! Then every element gets its certificate and class:
//!
//! * The reference `r` is the value of the ideal pipeline, the exact iso line `(f, t)`:
//!   `f` the face's domain side, `t` the parameter the op evaluates. Which side is `f`
//!   comes from main's values (main's fixed coordinate strictly nearer it than the other
//!   side) and main's trim interval must equal the new one bit for bit; a normal uv's
//!   direction comes from main. Nothing in a reference comes from the new output. Trim
//!   control points and samples, uv points and derivatives have exact double references.
//!   Surface values at `(f, t)` (normal, binormal, uv vector, `CrvOnSurf` point and
//!   derivative, lifted samples) are enclosed in narrow outward-rounded intervals of the
//!   exact rational value.
//! * CLOSER: `|new − r| < |main − r|`, decided exactly (dyadic arithmetic) for a double
//!   reference, or `r`'s whole enclosure on new's side of the midpoint of new and main.
//! * EQUAL: `new == main`, or the two at the same exact distance from `r`.
//! * EQUAL-at-rounding: `new` lies in the a-priori enclosure `E` of the kernel's own float
//!   result, so `|new − r| <= B = sup_{x in E} |x − r|`, with `B = L e_in + γ_k M`; for an
//!   enclosed reference `R` must lie in `E` too (a reference wider than the kernel's
//!   rounding decides nothing):
//!   - a degree-1 trim evaluation: `B = 64 ε max(|f|, |t0|, |t1|)` (`nurbs_crv.rs`
//!     `crv_point_error`);
//!   - a surface evaluation at the record's own uv: the exact homogeneous sums widened by
//!     `γ_K Σ|terms|`, `K = 7p + 7q + (p+1)(q+1) + 10`, counted from the evaluator
//!     (`srf_nurb.rs` `first_derivs_span`, `univariate_first`): `6p` per direction for the
//!     basis and its derivative, `2` plus `(p+1)(q+1)` for the tensor sum, `p + q + 6` for
//!     the rational branch; then one rounding per operation of the rational quotient; a
//!     cross product `γ_2` per component, a dot product of `n` terms `γ_n`, a normalisation
//!     `γ_3` (dot), `u` (square root) and `γ_2` (division), the uv-vector solve `γ_2` per
//!     2×2 minor and `u` per division;
//!   - a `CrvOnSurf` point or derivative at `(f, t)`: `L e_in + γ_K M` with `L` the hull
//!     bound of the surface's first (second, for a derivative) derivative net, `e_in` the
//!     trim evaluation's `64 ε M` plus the parameter's own `γ_3 (|lo| + |hi|)`, `M` the net's
//!     largest coordinate;
//!   - a drop quality value (a maximum over stations): every station is on the exact iso
//!     line, an iso station at `(f, param)` bit for bit, a gap station at curve parameter
//!     `s = t` bit for bit and at the degree-1 trim's float uv, within the a-priori
//!     `64 ε max(|f|, |t0|, |t1|)` of `(f, t)` (checked per station; no solver residual
//!     enters). Every station's `|dist|`, `dev`, `line_param` and `drop_line_dist` is
//!     enclosed from the station's own inputs through the float surface, normal and
//!     difference chain (each dumped value checked inside its enclosure); the reported value
//!     is the maximum, or the kernel's `sqrt(d² + v²)` with one rounding per operation. The
//!     reference is the exact maximum over the same stations of the ideal pipeline: `0` at
//!     every station when the edge's curve is the face's boundary row at `f` (a clamped end,
//!     bit for bit) or its `CrvOnSurf` UV curve is the exact iso line; otherwise at `(f, t)`
//!     the exact split of `C(s) − S(f, t)` along and across the normal, at a gap station at
//!     the curve's closest approach to the normal line, `s*` certified by interval Newton
//!     (unique in a box of a few hundred ulps, `h'` enclosed with the hull of the curve's
//!     second-derivative control net over the box's span), the values taken at a double
//!     near `s*` and bounded by the mean value theorem and the convexity of `dev²`
//!     ([`gap_ref`]). At rounding needs `R ⊆ E` as well as `new ∈ E` (checked on the
//!     maximum), so `|new − r| <= width(E)`, the kernel's rounding alone.
//!   - the cached edge/face gap of a goc case (a `CrvOnSurf` on its own face, checked on
//!     the model) and `face_p3`'s mean and maximum curve/surface gaps (every edge curve of
//!     the face its boundary row along a domain side, checked on the model): the reference
//!     is `0`, and the new values are `0` exactly.
//!   - a point or tangent of the edge's 3D iso B-spline curve (`create_iso_parametric_curve`:
//!     `extract_iso_crv`, `crv_trim`, `end_at_insertion_point`) at `t`: the reference is the
//!     exact surface point `S(f, t)` (tangent `S_v` or `S_u`, negated for an `Opposite` edge
//!     use); the curve's control points are within `γ_{14(dx+2d)} M` of the exact ones, so a
//!     point is within `γ_{14(dx+2d)+7d+3} M` and a tangent within
//!     `(2d/h) γ_{14(dx+2d)+7d+6} M` ([`label_iso_tangent`]); a rational net is enclosed
//!     through the kernel's own quotient chain ([`label_iso_tangent_rational`]).
//! * MEASURED (19 elements; not certified): `|new − main|` is within a measured bound, the
//!   edge curve's a-priori move ([`iso_curve_move`]) carried through a step whose
//!   sensitivity is sampled, not enclosed: a point solve's uv (over the surface's smallest
//!   singular value at `(f, t)`, [`solved_uv_bound`], 2 elements, both 1 ulp farther from
//!   the exact iso line than main's: the exact reference classes them FARTHER, and the
//!   measured bound is used only then), `check_set`'s maximum
//!   edge / UV-curve distance (a sampled search, 9) and a drop quality with no dumped
//!   stations (8).
//! * A binormal row's vertex/face-gap-cached flag (`1 → 0`): proven `0` on the replayed
//!   model (`prove_corner_flag`): the edge's curve and its loop neighbour's at the shared
//!   vertex are `create_iso_parametric_curve` of two domain sides of different coordinates,
//!   so the vertex's ideal uv is their corner, where the ideal trims meet exactly; only
//!   `sm_FixUVTrimCurves` computes that gap on this path, and only for trims that miss each
//!   other by more than `EFF_ZERO (1 + |uv|)`.
//! * FARTHER: none of the above, a defect. The pinned classes hold none.
//!
//! The rows are those of a fresh bless of main and of the fix, diffed bit for bit, in
//! these three units, including elements that move by less than the goldens' tolerance;
//! all of them are re-blessed. The same diff moves 39 rows of six other units, all within
//! the goldens' tolerance (their block comparison). Every one of those moves comes from
//! the exact drop ends: with the gap seed reverted the fresh bless of all nine units is the
//! same bit for bit. All of them are certified here and re-blessed:
//!
//! * `topology.vertexuse` (2 rows): the uv point, tangents and sector angle at a vertex use
//!   of a face whose edges are all domain sides' iso curves (checked on the replayed model,
//!   each vertex on sides of both coordinates): the corner main's point is nearest, the
//!   axis vector main's tangent is nearest, and the exact multiple of 90° between them
//!   (`label_vertexuse`).
//! * `topology.brep_validation` (1 row): the samples of the edge uses' UV trims and of the
//!   edges' `CrvOnSurf` curves over copies of them, against the exact iso line (side and
//!   direction those main's samples single out) and the exact surface point on it
//!   (`label_validation`).
//! * `query.face_p2` (2 rows): a face-class query uv and a vertex-class gap at a point of
//!   an edge use's trim, the edge a domain side's iso curve on the replayed model
//!   (`label_query`).
//!
//! * `blend.fillet_solver_p2` (5 rows): the rail / side edge-use intersection, a
//!   `LocalSolveNd` fixed point of affine equations (two offset planes, a degree-1 trim): the
//!   root enclosed from the exact residual at the new point, the kernel's own residual there
//!   within its evaluation's rounding, and `|new − x*|` bounded by `|J⁻¹|` times it
//!   (`label_rail_eu`).
//!
//! * `mass.integrator` (27 rows) and `boolean.api_brep` (1 row, two areas): the kernel's
//!   nested adaptive Gauss-Kronrod integration of a face's properties along its trims, the
//!   trims the face's domain sides (every end a domain corner bit for bit). The integrator
//!   is replayed in floats (bit for bit the row's values, recording each piece's
//!   accept/split decision: the partition), then over the same partition in
//!   outward-rounded intervals ([`mass_replay`]): `R` exact, `E` the enclosure of the
//!   kernel's float result (every recorded float intermediate checked inside it). The same
//!   float replay run against main's kernel reproduced main's row (`M`) bit for bit, and
//!   its tape is committed (`T`, one line per replayed face). Two certificates:
//!   - `mass.integrator` (`label_mass`): the new run's tape must equal main's decision for
//!     decision, so main and the fix integrate over one shared partition, and `R` is the
//!     exact quadrature of that shared partition on the exact sides (the kernel's rule
//!     computed exactly: its nodes, weights and partition). `R` is not claimed to be the
//!     value an exact pipeline would reach: in exact arithmetic some accept/split
//!     decisions of three `issue1435_multispan_*` rows straddle the threshold (the
//!     estimate's enclosure contains it), and such a pipeline could partition
//!     differently. Those rows are certified against the kernel's rule on the shared
//!     partition.
//!   - `boolean.api_brep` `more0138` (`label_api_areas`): its new partition is not main's
//!     (face 0 has one decision fewer), so every decision of the new run is checked in
//!     exact arithmetic against the kernel's rule (`|K − G| <= accuracy · max(|K|, scale,
//!     EFF_ZERO)` per component, `K` and `G` enclosed; [`kr_settle`]). An accepted piece
//!     must be settled; a split piece the exact rule could accept adds the hull of both
//!     branches. `R` then holds the exact pipeline's value.
//!
//!   Each moved element has `R ⊆ E` and `new ∈ E` (at rounding), or new is closer; main's
//!   value (`M`) is asserted in the same `E` or farther than new. `E`'s absolute width
//!   tracks `γ · M` (the sums' magnitudes), not `|value|`: its largest width relative to
//!   `|new|` is on near-cancelling components (`|value|` ~1e-7 against O(1) siblings),
//!   where the absolute width matches the O(1) entries'.
//!
//! Run: `cargo test --profile goldens -p goldens --test issue_18325_moved_rows -- --nocapture`
//! (prints the table: element classes per unit and kind, the largest `|new − r| / B` of the
//! at-rounding elements, and the row classes).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::print_stderr,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::suboptimal_flops,
    missing_docs
)] // test

use std::collections::BTreeMap;

use goldens::case::repo_root;
use goldens::golden::encode;
use goldens::units::blend_fillet_solver_p2::rail_eu_scene;
use goldens::units::query_face_p2::scene as query_scene;
use goldens::units::topology_brep_validation::scene as val_scene;
use goldens::units::topology_edgeuse_p2::scene as scene_p2;
use goldens::units::topology_edgeuse_p3::scene;
use goldens::units::topology_face_p3::scene as face_scene;
use goldens::units::topology_vertexuse::scene as vu_scene;
use goldens::{registry, CaseContext, Output};
use serde_json::Value;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::extent2d::Extent2d;
use solidcore::kernel::Kernel;
use solidcore::math::Point2;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, ControlPointForm};
use solidcore::nurbs::b_spline_surface::p1::SurfParamType;
use solidcore::nurbs::b_spline_surface::p2::IsoCurveOptions;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{vertexuse, Brep, EdgeuseId};

const DATA: &str = include_str!("data/issue_18325_moved_rows.tsv");

const UNITS: &[&str] = &[
    "blend.fillet_solver_p2",
    "boolean.api_brep",
    "mass.integrator",
    "query.face_p2",
    "topology.brep_validation",
    "topology.edgeuse_p2",
    "topology.edgeuse_p3",
    "topology.face_p3",
    "topology.vertexuse",
];

/// Units whose committed main goldens may hold values older than main's output (passing,
/// so a bless kept them): there `M` is checked to pass `O`, not to equal it.
const STALE_GOLDEN_UNITS: &[&str] = &["boolean.api_brep", "mass.integrator"];

const BLADE: &str = "own_reader_blade_hub_create_eu36_rebuild";

// ---- exact dyadic comparison ----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    /// `|new − ref| < |main − ref|` (or `<=` with the two different).
    Closer,
    /// `new == main`, or the two equally far from an exact reference.
    Equal,
    /// Neither nearer: `new` lies in the a-priori enclosure `E` of the kernel's own float
    /// result, so `|new − ref| <= B = sup_{x in E} |x − ref|` (see the module docs).
    AtRounding,
    /// MEASURED, not certified: no exact reference is in reach (a numerical search's or
    /// solver's output), and `|new − main|` is within a stated bound of how far the
    /// search's certified input moved ([`Cert::Measured`]).
    Measured,
    /// None of the above: a defect.
    Farther,
}

/// An exact dyadic rational `m 2^e`, wide enough for the differences of two doubles of
/// nearby magnitude.
#[derive(Debug, Clone, Copy)]
struct Dy {
    m: i128,
    e: i32,
}

impl Dy {
    fn f(x: f64) -> Dy {
        assert!(x.is_finite(), "{x}");
        if x == 0.0 {
            return Dy { m: 0, e: 0 };
        }
        let bits = x.to_bits();
        let sign = if bits >> 63 == 1 { -1 } else { 1 };
        let exp = ((bits >> 52) & 0x7ff) as i32;
        let frac = i128::from(bits & ((1 << 52) - 1));
        let (m, e) = if exp == 0 {
            (frac, -1074)
        } else {
            (frac | (1 << 52), exp - 1075)
        };
        Dy { m: sign * m, e }.norm()
    }
    fn norm(self) -> Dy {
        if self.m == 0 {
            return Dy { m: 0, e: 0 };
        }
        let z = self.m.trailing_zeros();
        Dy {
            m: self.m >> z,
            e: self.e + z as i32,
        }
    }
    /// `|a − b|` against `|c − d|` exactly (all four doubles), or `None` when the exponents
    /// are too far apart for `i128` (then the caller falls back to intervals).
    fn cmp_dist(a: f64, b: f64, c: f64, d: f64) -> Option<std::cmp::Ordering> {
        let xs = [Dy::f(a), Dy::f(b), Dy::f(c), Dy::f(d)];
        let e = xs
            .iter()
            .filter(|x| x.m != 0)
            .map(|x| x.e)
            .min()
            .unwrap_or(0);
        let mut ms = [0i128; 4];
        for (k, x) in xs.iter().enumerate() {
            if x.m == 0 {
                continue;
            }
            let s = u32::try_from(x.e - e).ok()?;
            if s >= 120 || x.m.unsigned_abs().leading_zeros() <= s + 2 {
                return None;
            }
            ms[k] = x.m << s;
        }
        let l = ms[0].checked_sub(ms[1])?.checked_abs()?;
        let r = ms[2].checked_sub(ms[3])?.checked_abs()?;
        Some(l.cmp(&r))
    }
}

// ---- outward-rounded intervals --------------------------------------------------------------

/// A closed interval of doubles; every operation rounds to nearest and then widens one ulp
/// each way, so it holds the exact result for all members of its operands.
#[derive(Debug, Clone, Copy)]
struct Iv {
    lo: f64,
    hi: f64,
}

const U: f64 = 1.0 / 9_007_199_254_740_992.0; // 2^-53, the unit roundoff

impl Iv {
    fn p(x: f64) -> Iv {
        Iv { lo: x, hi: x }
    }
    fn out(lo: f64, hi: f64) -> Iv {
        Iv {
            lo: lo.next_down(),
            hi: hi.next_up(),
        }
    }
    fn hull(a: Iv, b: Iv) -> Iv {
        Iv {
            lo: a.lo.min(b.lo),
            hi: a.hi.max(b.hi),
        }
    }
    fn add(self, o: Iv) -> Iv {
        Iv::out(self.lo + o.lo, self.hi + o.hi)
    }
    fn sub(self, o: Iv) -> Iv {
        Iv::out(self.lo - o.hi, self.hi - o.lo)
    }
    fn neg(self) -> Iv {
        Iv {
            lo: -self.hi,
            hi: -self.lo,
        }
    }
    fn mul(self, o: Iv) -> Iv {
        let c = [
            self.lo * o.lo,
            self.lo * o.hi,
            self.hi * o.lo,
            self.hi * o.hi,
        ];
        Iv::out(
            c.iter().copied().fold(f64::INFINITY, f64::min),
            c.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        )
    }
    fn div(self, o: Iv) -> Iv {
        assert!(
            o.lo > 0.0 || o.hi < 0.0,
            "division by an interval holding 0: {o:?}"
        );
        let c = [
            self.lo / o.lo,
            self.lo / o.hi,
            self.hi / o.lo,
            self.hi / o.hi,
        ];
        Iv::out(
            c.iter().copied().fold(f64::INFINITY, f64::min),
            c.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        )
    }
    fn sqrt(self) -> Iv {
        let lo = self.lo.max(0.0);
        assert!(self.hi >= 0.0, "sqrt of {self:?}");
        Iv::out(lo.sqrt(), self.hi.sqrt()).clamp0()
    }
    fn clamp0(self) -> Iv {
        Iv {
            lo: self.lo.max(0.0),
            hi: self.hi,
        }
    }
    fn abs(self) -> Iv {
        if self.lo >= 0.0 {
            self
        } else if self.hi <= 0.0 {
            self.neg()
        } else {
            Iv {
                lo: 0.0,
                hi: (-self.lo).max(self.hi),
            }
        }
    }
    /// The largest magnitude (exact: no rounding).
    fn mag(self) -> f64 {
        self.lo.abs().max(self.hi.abs())
    }
    /// Widened by `r >= 0` each way.
    fn widen(self, r: f64) -> Iv {
        assert!(r >= 0.0);
        Iv::out(self.lo - r, self.hi + r)
    }
    /// One kernel rounding chain of relative size `g`: widened by `g |x|`.
    fn rel(self, g: f64) -> Iv {
        self.widen(up_mul(g, self.mag()))
    }
    fn contains(self, x: f64) -> bool {
        self.lo <= x && x <= self.hi
    }
}

/// `a b` rounded up (`a, b >= 0`).
fn up_mul(a: f64, b: f64) -> f64 {
    Iv::p(a).mul(Iv::p(b)).hi
}
/// `a + b` rounded up (`a, b >= 0`).
fn up_add(a: f64, b: f64) -> f64 {
    Iv::p(a).add(Iv::p(b)).hi
}

/// `γ_n = n u / (1 − n u)`, rounded up: the relative error bound of a chain of `n` roundings.
fn gam(n: usize) -> f64 {
    let nu = Iv::p(n as f64).mul(Iv::p(U));
    nu.div(Iv::p(1.0).sub(nu)).hi
}

type V3 = [Iv; 3];

// The kernel-chain helpers below come in two modes over the same interval expression:
// `round` widens every step by its float rounding (an enclosure of the kernel's float result
// for inputs in the operand boxes); without it they are the exact operations. Interval
// operations are inclusion-monotone, so the exact chain over the exact inputs' enclosures
// lies inside the rounded chain over boxes holding them: the reference `R` inside `E`.

/// `x` widened by `g |x|` when `round`.
fn rnd(x: Iv, g: f64, round: bool) -> Iv {
    if round {
        x.rel(g)
    } else {
        x
    }
}

/// A dot product of `n` terms; rounded: the exact sum widened by `γ_n Σ |terms|`.
fn rdot(a: &V3, b: &V3, n: usize, round: bool) -> Iv {
    let mut s = Iv::p(0.0);
    let mut m = 0.0;
    for (x, y) in a.iter().zip(b) {
        let t = x.mul(*y);
        s = s.add(t);
        m = up_add(m, t.mag());
    }
    if round {
        s.widen(up_mul(gam(n), m))
    } else {
        s
    }
}

/// `x − y`; rounded: each operand a product, `γ_2 (|x| + |y|)`.
fn rsub2(x: Iv, y: Iv, round: bool) -> Iv {
    let d = x.sub(y);
    if round {
        d.widen(up_mul(gam(2), up_add(x.mag(), y.mag())))
    } else {
        d
    }
}

/// A cross product; rounded: each component's two products and difference, `γ_2`.
fn rcross(a: &V3, b: &V3, round: bool) -> V3 {
    let comp = |i: usize, j: usize| rsub2(a[i].mul(b[j]), a[j].mul(b[i]), round);
    [comp(1, 2), comp(2, 0), comp(0, 1)]
}

/// A normalisation `c / |c|`; rounded: the dot (`γ_3`), the square root (`u`), the division
/// (`γ_2`, which also covers a reciprocal-and-multiply).
fn runit(c: &V3, round: bool) -> V3 {
    let q = rdot(c, c, 3, round);
    let ln = rnd(q.sqrt(), U, round);
    std::array::from_fn(|i| rnd(c[i].div(ln), gam(2), round))
}

fn cross(a: &V3, b: &V3) -> V3 {
    [
        a[1].mul(b[2]).sub(a[2].mul(b[1])),
        a[2].mul(b[0]).sub(a[0].mul(b[2])),
        a[0].mul(b[1]).sub(a[1].mul(b[0])),
    ]
}

fn dot(a: &V3, b: &V3) -> Iv {
    a[0].mul(b[0]).add(a[1].mul(b[1])).add(a[2].mul(b[2]))
}

fn sub3(a: &V3, b: &V3) -> V3 {
    [a[0].sub(b[0]), a[1].sub(b[1]), a[2].sub(b[2])]
}

// ---- B-spline bases, surfaces and curves ----------------------------------------------------

/// The degree-`p` basis functions over `knots` at the double `t`, their first derivatives
/// and the sums `|terms|` of each derivative (enclosures of the exact values). The span is
/// right-continuous (the last nonempty one at the end, the first before the start), as the
/// kernel's.
fn basis(knots: &[f64], p: usize, tt: Iv) -> (Vec<Iv>, Vec<Iv>, Vec<Iv>) {
    let n = knots.len() - p - 1;
    let t = tt.lo;
    let span = (p..n)
        .rev()
        .find(|&i| knots[i] <= t && t < knots[i + 1])
        .unwrap_or(if t < knots[p] { p } else { n - 1 });
    // the whole interval in that span's polynomial piece
    assert!(
        tt.hi == tt.lo || tt.hi <= knots[span + 1] || span == n - 1,
        "parameter {tt:?} straddles a knot"
    );
    let zero = Iv::p(0.0);
    let k = |i: usize| Iv::p(knots[i]);
    let mut nb: Vec<Iv> = (0..knots.len() - 1)
        .map(|i| if i == span { Iv::p(1.0) } else { zero })
        .collect();
    let mut lower = vec![zero; knots.len()];
    for d in 1..=p {
        lower.clone_from(&nb);
        let mut next = vec![zero; knots.len() - 1 - d];
        for (i, slot) in next.iter_mut().enumerate() {
            let mut acc = zero;
            if knots[i + d] != knots[i] {
                acc = acc.add(tt.sub(k(i)).div(k(i + d).sub(k(i))).mul(nb[i]));
            }
            if knots[i + d + 1] != knots[i + 1] {
                acc = acc.add(
                    k(i + d + 1)
                        .sub(tt)
                        .div(k(i + d + 1).sub(k(i + 1)))
                        .mul(nb[i + 1]),
                );
            }
            *slot = acc;
        }
        nb = next;
    }
    let pf = Iv::p(p as f64);
    let mut der = Vec::with_capacity(n);
    let mut dab = Vec::with_capacity(n);
    for i in 0..n {
        let mut a = zero;
        let mut b = zero;
        if p > 0 {
            if knots[i + p] != knots[i] {
                let x = pf.mul(lower[i]).div(k(i + p).sub(k(i)));
                a = a.add(x);
                b = b.add(x.abs());
            }
            if knots[i + p + 1] != knots[i + 1] {
                let x = pf.mul(lower[i + 1]).div(k(i + p + 1).sub(k(i + 1)));
                a = a.sub(x);
                b = b.add(x.abs());
            }
        }
        der.push(a);
        dab.push(b);
    }
    nb.truncate(n);
    (nb, der, dab)
}

fn num(v: &Value) -> f64 {
    v.as_f64().expect("number")
}

fn nums(v: &Value) -> Vec<f64> {
    v.as_array().expect("array").iter().map(num).collect()
}

/// A B-spline surface of a case's input (`bspline`: homogeneous control net `net[i][j] =
/// [w x, w y, w z, w]`, `w = −MAX` for a non-rational net; `i` along `u`).
struct Srf {
    net: Vec<Vec<[f64; 4]>>,
    rational: bool,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

/// `S`, `S_u`, `S_v`.
type Eval = (V3, V3, V3);

impl Srf {
    fn from_input(inputs: &Value) -> Srf {
        Srf::from_bspline(&inputs["brep"]["surface"]["bspline"])
    }

    /// A `bspline` surface input (`net`, `p`, `q`, `u`, `v`).
    fn from_bspline(b: &Value) -> Srf {
        let net: Vec<Vec<[f64; 4]>> = b["net"]
            .as_array()
            .expect("net")
            .iter()
            .map(|row| {
                row.as_array()
                    .expect("row")
                    .iter()
                    .map(|cp| {
                        let c = nums(cp);
                        [c[0], c[1], c[2], c[3]]
                    })
                    .collect()
            })
            .collect();
        let rational = net.iter().flatten().any(|cp| cp[3] > -1e300);
        Srf {
            net,
            rational,
            p: b["p"].as_u64().expect("p") as usize,
            q: b["q"].as_u64().expect("q") as usize,
            u: nums(&b["u"]),
            v: nums(&b["v"]),
        }
    }

    /// The homogeneous sums at `(u, v)` and their `|terms|` sums: `(A, A_u, A_v)` and
    /// `(|A|, |A_u|, |A_v|)`.
    fn sums(&self, u: Iv, v: Iv) -> ([[Iv; 4]; 3], [[f64; 4]; 3]) {
        let (nu, du, dua) = basis(&self.u, self.p, u);
        let (nv, dv, dva) = basis(&self.v, self.q, v);
        let mut a = [[Iv::p(0.0); 4]; 3];
        let mut m = [[0.0f64; 4]; 3];
        for (i, row) in self.net.iter().enumerate() {
            for (j, cp) in row.iter().enumerate() {
                for c in 0..4 {
                    let x = if c == 3 && !self.rational { 1.0 } else { cp[c] };
                    let xi = Iv::p(x);
                    let ax = Iv::p(x.abs());
                    let terms = [
                        (nu[i].mul(nv[j]), nu[i].mul(nv[j]).abs()),
                        (du[i].mul(nv[j]), dua[i].mul(nv[j].abs())),
                        (nu[i].mul(dv[j]), nu[i].abs().mul(dva[j])),
                    ];
                    for (d, (w, wa)) in terms.iter().enumerate() {
                        a[d][c] = a[d][c].add(w.mul(xi));
                        m[d][c] = up_add(m[d][c], wa.mul(ax).hi);
                    }
                }
            }
        }
        (a, m)
    }

    /// The exact `S, S_u, S_v` anywhere in `(u, v)` (narrow intervals), enclosed.
    fn exact(&self, u: Iv, v: Iv) -> Eval {
        let (a, _) = self.sums(u, v);
        self.quotient(a, false)
    }

    /// An enclosure of the kernel's float `S, S_u, S_v` at the doubles `(u, v)`: the exact
    /// sums widened by `γ_K Σ|terms|`, then the rational quotient one rounding per
    /// operation. `K = 7p + 7q + (p+1)(q+1) + 10`, counted on the chain the values go
    /// through (`Surface::first_derivs_span` in `nurbs/srf_nurb.rs`; `eval` the same
    /// recurrences without the derivative):
    /// - `univariate_first`, per direction: each level of the basis triangle rounds
    ///   `left`/`right` (1), their sum, the divisor (1 more), the quotient `temp` (1), the
    ///   product `right · temp` (1) and the sum `saved + …` (1) on nonnegative terms: `6p`
    ///   relative to the terms. The first derivative's terms: the divisor of level `p`
    ///   (2), the reciprocal (1), the product (1), the sum (1) and the factor `p` (1) on
    ///   the level `p − 1` values (`6(p − 1)`): `6p` relative to `Σ|terms|` (`dua`).
    /// - the tensor sum: `alpha = a · b` (1), `alpha · P` (1), and the running sum of the
    ///   `(p+1)(q+1)` terms (`(p+1)(q+1)`).
    /// - the rational branch (only the blade hub's net, every weight exactly `1`): the
    ///   denominator `d00` and its derivatives (the inner and outer weight sums, `q + 1`
    ///   and `p + 1`, and the product, 1), the reciprocal `inv_w00` (1), the products by
    ///   the weight and by `inv_w00` (2) and the derivative recurrence's `c · r` product and
    ///   difference (2): `p + q + 6` more, `7p + 7q + (p+1)(q+1) + 10` in all (a
    ///   non-rational net needs `6p + 6q + (p+1)(q+1) + 2` of it).
    ///
    /// Every dumped value is checked inside its enclosure, so an undercount shows as a
    /// failed containment, never as a wrong class.
    fn kernel(&self, u: f64, v: f64) -> Eval {
        self.kernel_iv(Iv::p(u), Iv::p(v))
    }

    /// [`Srf::kernel`] for every `(u, v)` in a parameter box (one polynomial piece).
    fn kernel_iv(&self, u: Iv, v: Iv) -> Eval {
        let (mut a, m) = self.sums(u, v);
        let g = gam(srf_k(self.p, self.q));
        for d in 0..3 {
            for c in 0..4 {
                a[d][c] = a[d][c].widen(up_mul(g, m[d][c]));
            }
        }
        self.quotient(a, true)
    }

    /// The kernel's float evaluation enclosed over a box (`round`), or the exact one. A box
    /// that meets interior knots is split there and the pieces' enclosures joined (the
    /// piece `[k, hi]` evaluates `k` on the span to its right, as the kernel does; the piece
    /// `[lo, k]` adds the left span's limit, which only widens the enclosure).
    fn eval(&self, u: Iv, v: Iv, round: bool) -> Eval {
        let pieces = |kn: &[f64], x: Iv| -> Vec<Iv> {
            let mut cuts: Vec<f64> = kn
                .iter()
                .copied()
                .filter(|&k| x.lo < k && k <= x.hi)
                .collect();
            cuts.dedup();
            let mut out = Vec::new();
            let mut lo = x.lo;
            for k in cuts {
                out.push(Iv { lo, hi: k });
                lo = k;
            }
            out.push(Iv { lo, hi: x.hi });
            out
        };
        let mut acc: Option<Eval> = None;
        for pu in pieces(&self.u, u) {
            for pv in pieces(&self.v, v) {
                let e = if round {
                    self.kernel_iv(pu, pv)
                } else {
                    self.exact(pu, pv)
                };
                acc = Some(match acc {
                    None => e,
                    Some(a) => {
                        let h = |x: &V3, y: &V3| -> V3 {
                            std::array::from_fn(|q| Iv::hull(x[q], y[q]))
                        };
                        (h(&a.0, &e.0), h(&a.1, &e.1), h(&a.2, &e.2))
                    }
                });
            }
        }
        acc.expect("a piece")
    }

    fn quotient(&self, a: [[Iv; 4]; 3], round: bool) -> Eval {
        let r = |x: Iv| if round { x.rel(U) } else { x };
        if !self.rational {
            let f = |d: usize| [a[d][0], a[d][1], a[d][2]];
            return (f(0), f(1), f(2));
        }
        let w = a[0][3];
        let s: V3 = std::array::from_fn(|c| r(a[0][c].div(w)));
        let der = |d: usize| -> V3 {
            std::array::from_fn(|c| r(r(a[d][c].sub(r(a[d][3].mul(s[c])))).div(w)))
        };
        (s, der(1), der(2))
    }
}

/// The rounding count `K` of the kernel's surface evaluation of degrees `(p, q)`, value
/// and first derivatives ([`Srf::kernel`] counts it): `7p + 7q + (p+1)(q+1) + 10`.
fn srf_k(p: usize, q: usize) -> usize {
    7 * p + 7 * q + (p + 1) * (q + 1) + 10
}

/// A dumped B-spline curve (`deg`, the full knot vector, homogeneous control points
/// `[w x, w y, w z, w]`).
#[derive(Debug, Clone)]
struct Crv {
    p: usize,
    knots: Vec<f64>,
    cps: Vec<[f64; 4]>,
}

impl Crv {
    fn rational(&self) -> bool {
        self.cps.iter().any(|c| c[3] != 1.0)
    }
    /// The exact `C(s)` and `C'(s)` at the double `s`, enclosed.
    fn exact(&self, s: f64) -> (V3, V3) {
        let (n, d, _) = basis(&self.knots, self.p, Iv::p(s));
        let mut a = [Iv::p(0.0); 4];
        let mut ad = [Iv::p(0.0); 4];
        for (i, cp) in self.cps.iter().enumerate() {
            for q in 0..4 {
                a[q] = a[q].add(n[i].mul(Iv::p(cp[q])));
                ad[q] = ad[q].add(d[i].mul(Iv::p(cp[q])));
            }
        }
        let w = a[3];
        let c: V3 = std::array::from_fn(|q| a[q].div(w));
        let cd: V3 = std::array::from_fn(|q| ad[q].sub(ad[3].mul(c[q])).div(w));
        (c, cd)
    }
    /// An enclosure of the kernel's float `C(s)` (`crv_eval` in `nurbs/nurbs_crv.rs`, the
    /// path `BSplineCurve::evaluate_point` takes inside the interval; its ends return the
    /// end control points, which this holds too): the exact sums widened by
    /// `γ_K Σ|terms|`, `K = 7p + 2`, counted as [`Srf::kernel`]'s per direction:
    /// - `basis_eval_into`, each of the `p` levels: `left`/`right` (1), their sum `delta`
    ///   (1 more), the quotient (1), the product by `temp` (1) and the sum `saved + …` (1)
    ///   on nonnegative terms: `6p` relative to the terms;
    /// - the weighted sum: `alpha · Pw` (1) and the running sum of the `p + 1` terms
    ///   (`p + 1`);
    ///
    /// then `to_euclid`'s division by the weight, one rounding (`u`) of the quotient of the
    /// two enclosures.
    fn kernel(&self, s: f64) -> V3 {
        let (n, _, _) = basis(&self.knots, self.p, Iv::p(s));
        let mut a = [Iv::p(0.0); 4];
        let mut m = [0.0f64; 4];
        for (i, cp) in self.cps.iter().enumerate() {
            for q in 0..4 {
                a[q] = a[q].add(n[i].mul(Iv::p(cp[q])));
                m[q] = up_add(m[q], n[i].abs().mul(Iv::p(cp[q].abs())).hi);
            }
        }
        let g = gam(7 * self.p + 2);
        let a: [Iv; 4] = std::array::from_fn(|q| a[q].widen(up_mul(g, m[q])));
        if !self.rational() {
            return [a[0], a[1], a[2]];
        }
        std::array::from_fn(|q| a[q].div(a[3]).rel(U))
    }
    /// The nonempty knot span holding the interval `s` (`knots[j] <= s.lo < knots[j + 1]`,
    /// the last one at the end), asserting `s` does not straddle an interior knot.
    fn span(&self, s: Iv) -> usize {
        let n = self.cps.len();
        let j = (self.p..n)
            .rev()
            .find(|&i| self.knots[i] <= s.lo && s.lo < self.knots[i + 1])
            .unwrap_or(if s.lo < self.knots[self.p] {
                self.p
            } else {
                n - 1
            });
        assert!(
            s.hi <= self.knots[j + 1] || j == n - 1,
            "parameter box {s:?} straddles a knot"
        );
        j
    }
    /// `C(S)`, `C'(S)`, `C''(S)` over the box `S` and `w(S)`: `C''` from the hull of the
    /// homogeneous second-derivative control net over `S`'s span (the convex hull property:
    /// the second derivative of the homogeneous curve on a span is a B-spline over that
    /// span's `p − 1` second-derivative control points), `C'' = (A'' − 2 w' C' − w'' C) / w`.
    fn homog_iv(&self, s: Iv) -> (V3, V3, V3, Iv) {
        let (n, d, _) = basis(&self.knots, self.p, s);
        let mut a = [Iv::p(0.0); 4];
        let mut ad = [Iv::p(0.0); 4];
        for (i, cp) in self.cps.iter().enumerate() {
            for q in 0..4 {
                a[q] = a[q].add(n[i].mul(Iv::p(cp[q])));
                ad[q] = ad[q].add(d[i].mul(Iv::p(cp[q])));
            }
        }
        let w = a[3];
        let c: V3 = std::array::from_fn(|q| a[q].div(w));
        let cd: V3 = std::array::from_fn(|q| ad[q].sub(ad[3].mul(c[q])).div(w));
        let add = self.d2_hull(s);
        let cdd: V3 = std::array::from_fn(|q| {
            add[q]
                .sub(Iv::p(2.0).mul(ad[3]).mul(cd[q]))
                .sub(add[3].mul(c[q]))
                .div(w)
        });
        (c, cd, cdd, w)
    }
    /// The hull of the homogeneous second-derivative control points that act on `S`'s span:
    /// `Q_i = p (H_{i+1} − H_i) / (k_{i+p+1} − k_{i+1})`, `R_i = (p − 1) (Q_{i+1} − Q_i) /
    /// (k_{i+p+1} − k_{i+2})`, `i` in `j − p ..= j − 2` for the span `j` (every divisor
    /// there holds the span, so it is positive). `0` for degree 1.
    fn d2_hull(&self, s: Iv) -> [Iv; 4] {
        let p = self.p;
        if p < 2 {
            return [Iv::p(0.0); 4];
        }
        let j = self.span(s);
        let k = |i: usize| Iv::p(self.knots[i]);
        let h = |i: usize| self.cps[i].map(Iv::p);
        let pf = Iv::p(p as f64);
        let q = |i: usize| -> [Iv; 4] {
            let den = k(i + p + 1).sub(k(i + 1));
            let (a, b) = (h(i + 1), h(i));
            std::array::from_fn(|c| pf.mul(a[c].sub(b[c])).div(den))
        };
        let pm = Iv::p((p - 1) as f64);
        let mut hull: Option<[Iv; 4]> = None;
        for i in j - p..=j - 2 {
            let den = k(i + p + 1).sub(k(i + 2));
            let (a, b) = (q(i + 1), q(i));
            let r: [Iv; 4] = std::array::from_fn(|c| pm.mul(a[c].sub(b[c])).div(den));
            hull = Some(match hull {
                None => r,
                Some(x) => std::array::from_fn(|c| Iv::hull(x[c], r[c])),
            });
        }
        hull.expect("a second-derivative control point")
    }
}

/// Hull bounds of a non-rational surface: `max|P|`, `max|S_u|`, `max|S_v|` and the second
/// derivative nets' maxima per coordinate (the derivative nets of the control net), and the
/// shortest nonempty knot spans.
struct Bounds {
    m: [f64; 3],
    lu: [f64; 3],
    lv: [f64; 3],
    luu: [f64; 3],
    luv: [f64; 3],
    lvv: [f64; 3],
    hu: f64,
    hv: f64,
}

fn deriv_net(net: &[Vec<[Iv; 3]>], kn: &[f64], p: usize, along_u: bool) -> Vec<Vec<[Iv; 3]>> {
    let pf = Iv::p(p as f64);
    if along_u {
        (0..net.len() - 1)
            .map(|i| {
                (0..net[0].len())
                    .map(|j| {
                        if kn[i + p + 1] == kn[i + 1] {
                            // its basis function vanishes identically (degree 0)
                            return [Iv::p(0.0); 3];
                        }
                        let d = Iv::p(kn[i + p + 1]).sub(Iv::p(kn[i + 1]));
                        std::array::from_fn(|c| pf.mul(net[i + 1][j][c].sub(net[i][j][c])).div(d))
                    })
                    .collect()
            })
            .collect()
    } else {
        (0..net.len())
            .map(|i| {
                (0..net[0].len() - 1)
                    .map(|j| {
                        if kn[j + p + 1] == kn[j + 1] {
                            // its basis function vanishes identically (degree 0)
                            return [Iv::p(0.0); 3];
                        }
                        let d = Iv::p(kn[j + p + 1]).sub(Iv::p(kn[j + 1]));
                        std::array::from_fn(|c| pf.mul(net[i][j + 1][c].sub(net[i][j][c])).div(d))
                    })
                    .collect()
            })
            .collect()
    }
}

fn hull_max(net: &[Vec<[Iv; 3]>]) -> [f64; 3] {
    std::array::from_fn(|c| {
        net.iter()
            .flatten()
            .map(|cp| cp[c].mag())
            .fold(0.0, f64::max)
    })
}

impl Srf {
    fn bounds(&self) -> Bounds {
        assert!(!self.rational, "hull bounds are for non-rational nets");
        let net: Vec<Vec<[Iv; 3]>> = self
            .net
            .iter()
            .map(|r| {
                r.iter()
                    .map(|c| [Iv::p(c[0]), Iv::p(c[1]), Iv::p(c[2])])
                    .collect()
            })
            .collect();
        let pu = deriv_net(&net, &self.u, self.p, true);
        let pv = deriv_net(&net, &self.v, self.q, false);
        let ui = &self.u[1..self.u.len() - 1];
        let vi = &self.v[1..self.v.len() - 1];
        let hmin = |kn: &[f64]| {
            kn.windows(2)
                .filter(|w| w[1] > w[0])
                .map(|w| Iv::p(w[1]).sub(Iv::p(w[0])).lo)
                .fold(f64::INFINITY, f64::min)
        };
        Bounds {
            m: hull_max(&net),
            lu: hull_max(&pu),
            lv: hull_max(&pv),
            luu: hull_max(&deriv_net(&pu, ui, self.p - 1, true)),
            luv: hull_max(&deriv_net(&pu, &self.v, self.q, false)),
            lvv: hull_max(&deriv_net(&pv, vi, self.q - 1, false)),
            hu: hmin(&self.u),
            hv: hmin(&self.v),
        }
    }
}

/// An enclosure of the kernel's float `CrvOnSurf` point `S(trim(s))`, `trim` a degree-1 UV
/// line: the exact surface point at the exact line point, widened by the line evaluation's
/// error `64 ε max|cp|` (`crv_point_error` at degree 1, [`line_bound`]) through the hull
/// Lipschitz bounds, and by the surface rounding `γ_K max|P|`, `K` = [`srf_k`] (the
/// surface's `evaluate_point` runs [`Srf::kernel`]'s counted chain without the derivative;
/// a non-negative basis summing to 1 keeps `Σ|terms| <= max|P|`).
fn cos_point(srf: &Srf, b: &Bounds, trim: &Crv, s: f64) -> V3 {
    assert!(trim.p == 1 && trim.cps.len() == 2 && !trim.rational());
    let (t0, t1) = (trim.knots[0], trim.knots[trim.knots.len() - 1]);
    let a = Iv::p(s).sub(Iv::p(t0)).div(Iv::p(t1).sub(Iv::p(t0)));
    let uv: [Iv; 2] = std::array::from_fn(|q| {
        Iv::p(trim.cps[0][q]).add(a.mul(Iv::p(trim.cps[1][q]).sub(Iv::p(trim.cps[0][q]))))
    });
    let mx = trim
        .cps
        .iter()
        .flat_map(|c| [c[0].abs(), c[1].abs()])
        .fold(0.0, f64::max);
    let e = up_mul(64.0 * f64::EPSILON, mx);
    // the exact surface point over the line point's enclosure, widened through the hull
    // Lipschitz bounds by the evaluation error, and by the surface's rounding
    let (p, _, _) = srf.exact(uv[0], uv[1]);
    let g = gam(srf_k(srf.p, srf.q));
    std::array::from_fn(|q| {
        let r = up_add(
            up_add(up_mul(b.lu[q], e), up_mul(b.lv[q], e)),
            up_mul(g, b.m[q]),
        );
        p[q].widen(r)
    })
}

// ---- certificates ---------------------------------------------------------------------------

/// What an element is checked against.
#[derive(Debug, Clone)]
enum Cert {
    /// An exact reference (a double), and the a-priori bound `B` of the kernel's error for
    /// it where one applies (`|new − r| <= B` is then at rounding).
    Exact {
        r: f64,
        b: Option<f64>,
        what: &'static str,
    },
    /// A reference known to lie in `r`, and the enclosure `e` of the kernel's float result.
    Enc {
        r: Iv,
        e: Option<Iv>,
        what: &'static str,
    },
    /// MEASURED: the output of a numerical search or solve whose inputs moved at most a
    /// certified amount; `bound` is that move through the search's (measured, not
    /// certified) sensitivity, and `|new − main| <= bound` is checked.
    Measured { bound: f64, what: &'static str },
}

impl Cert {
    fn what(&self) -> &'static str {
        match self {
            Cert::Exact { what, .. } | Cert::Enc { what, .. } | Cert::Measured { what, .. } => what,
        }
    }
}

fn classify(new: f64, main: f64, c: &Cert) -> Class {
    if new.to_bits() == main.to_bits() {
        return Class::Equal;
    }
    match c {
        Cert::Exact { r, b, .. } => match Dy::cmp_dist(new, *r, main, *r) {
            Some(std::cmp::Ordering::Less) => Class::Closer,
            Some(std::cmp::Ordering::Equal) => Class::Equal,
            _ => match b {
                Some(b) if Iv::p(new).sub(Iv::p(*r)).abs().hi <= *b => Class::AtRounding,
                _ => Class::Farther,
            },
        },
        Cert::Enc { r, e, .. } => {
            // closer: every point of r is nearer to new than to main (r beyond the
            // midpoint, on new's side)
            let mid = Iv::p(new).add(Iv::p(main)).mul(Iv::p(0.5));
            let closer = if new > main {
                r.lo > mid.hi
            } else {
                r.hi < mid.lo
            };
            // at rounding: new is in the kernel's float enclosure `E`, and the reference's
            // enclosure lies inside `E` too, so `|new − ref| <= width(E)`, the kernel's own
            // rounding (a reference wider than `E` decides nothing)
            if closer {
                Class::Closer
            } else if e.is_some_and(|e| e.contains(new) && e.lo <= r.lo && r.hi <= e.hi) {
                Class::AtRounding
            } else {
                Class::Farther
            }
        }
        Cert::Measured { bound, .. } => {
            if Iv::p(new).sub(Iv::p(main)).abs().hi <= *bound {
                Class::Measured
            } else {
                Class::Farther
            }
        }
    }
}

/// A UV trim record of a golden row (`util::trim_curve`): `1, type, n, degree, m`, the
/// `n + 1` homogeneous control points, the `m + 1` knots, the interval and 3 samples
/// (`0, u, v, 0` or a failure status).
struct Trim {
    cp: usize,
    n: usize,
    deg: usize,
    ivl: (f64, f64),
    samples: [Option<usize>; 3],
    end: usize,
}

fn parse_trim(v: &[f64], i: usize) -> Option<Trim> {
    let g = |j: usize| v.get(j).copied();
    if g(i)? != 1.0 || g(i + 1)? != 14033.0 {
        return None;
    }
    let (n, deg, m) = (g(i + 2)?, g(i + 3)?, g(i + 4)?);
    if n < 0.0 || deg < 1.0 || m < 1.0 || n.fract() != 0.0 || deg.fract() != 0.0 {
        return None;
    }
    let (n, deg, m) = (n as usize, deg as usize, m as usize);
    let cp = i + 5;
    let mut j = cp + 4 * (n + 1) + m + 1;
    let ivl = (g(j)?, g(j + 1)?);
    j += 2;
    let mut samples = [None; 3];
    for s in &mut samples {
        if g(j)? == 0.0 {
            *s = Some(j + 1);
            j += 4;
        } else {
            j += 1;
        }
    }
    Some(Trim {
        cp,
        n,
        deg,
        ivl,
        samples,
        end: j,
    })
}

/// The iso line a row's UV trim is, when it is one: the fixed coordinate `c` (`0`: `u`), its
/// value `f` and the trim interval `[t0, t1]`.
#[derive(Debug, Clone, Copy)]
struct Iso {
    c: usize,
    f: f64,
    ivl: (f64, f64),
}

/// The bound of a degree-1 trim evaluation in the kernel (`nurbs_crv.rs`
/// `crv_point_error`): `64 ε max(|f|, |t0|, |t1|)`.
fn line_bound(iso: &Iso) -> f64 {
    let mx = iso.f.abs().max(iso.ivl.0.abs()).max(iso.ivl.1.abs());
    up_mul(64.0 * f64::EPSILON, mx)
}

/// Labels the elements of every UV trim record in `v` that is an iso line of the face's
/// surface with the exact references of the ideal pipeline: control points and samples
/// `(f, t)`, `f` a side of the face's domain `dom` and `t` the trim's own parameter.
///
/// The side is not taken on the new output's word: main's trim at the same place (`main`,
/// the committed golden's vector) has the same interval bit for bit (`t0`, `t1` are not
/// the fix's output), and both of main's fixed-coordinate control points are strictly
/// nearer `f` than the domain's other side.
fn label_trims(
    v: &[f64],
    main: &[f64],
    dom: [f64; 4],
    out: &mut BTreeMap<usize, Cert>,
) -> Option<Iso> {
    let mut first = None;
    let mut i = 0;
    while i < v.len() {
        let Some(tr) = parse_trim(v, i) else {
            i += 1;
            continue;
        };
        i = tr.end;
        if tr.n != 1 || tr.deg != 1 {
            continue;
        }
        let p = [(v[tr.cp], v[tr.cp + 1]), (v[tr.cp + 4], v[tr.cp + 5])];
        let (t0, t1) = tr.ivl;
        for c in 0..2 {
            let at = |k: usize| if c == 0 { p[k].0 } else { p[k].1 };
            let mv = |k: usize| if c == 0 { p[k].1 } else { p[k].0 };
            if at(0) != at(1) || mv(0) != t0 || mv(1) != t1 {
                continue;
            }
            let f = at(0);
            if f != dom[c] && f != dom[c + 2] {
                continue;
            }
            let other = if f == dom[c] { dom[c + 2] } else { dom[c] };
            let mt = parse_trim(main, tr.cp - 5).expect("main's trim record");
            assert_eq!(
                [mt.ivl.0, mt.ivl.1].map(f64::to_bits),
                [t0, t1].map(f64::to_bits),
                "main's trim interval"
            );
            for k in 0..2 {
                let m = main[tr.cp + 4 * k + c];
                assert!(
                    (m - f).abs() < (m - other).abs(),
                    "main's fixed coordinate {m} is not nearer the side {f} than {other}"
                );
            }
            let iso = Iso { c, f, ivl: tr.ivl };
            first.get_or_insert(iso);
            let b = line_bound(&iso);
            for (k, t) in [(0usize, t0), (1, t1)] {
                out.insert(
                    tr.cp + 4 * k + c,
                    Cert::Exact {
                        r: f,
                        b: None,
                        what: "trim cp fixed",
                    },
                );
                out.insert(
                    tr.cp + 4 * k + 1 - c,
                    Cert::Exact {
                        r: t,
                        b: None,
                        what: "trim cp param",
                    },
                );
            }
            for (q, frac) in [0.0, 0.5, 1.0].into_iter().enumerate() {
                let Some(j) = tr.samples[q] else { continue };
                let t = ev(t0, t1, frac);
                out.insert(
                    j + c,
                    Cert::Exact {
                        r: f,
                        b: Some(b),
                        what: "trim sample fixed",
                    },
                );
                out.insert(
                    j + 1 - c,
                    Cert::Exact {
                        r: t,
                        b: Some(b),
                        what: "trim sample param",
                    },
                );
            }
            break;
        }
    }
    first
}

/// `Extent1d::evaluate`: `lo + f (hi − lo)` (the ends exactly at `f = 0, 1`).
fn ev(lo: f64, hi: f64, f: f64) -> f64 {
    if f == 0.0 {
        lo
    } else if f == 1.0 {
        hi
    } else {
        lo + f * (hi - lo)
    }
}

fn pts(v: &[f64], i: usize) -> V3 {
    [Iv::p(v[i]), Iv::p(v[i + 1]), Iv::p(v[i + 2])]
}

fn sign_of(a: &V3, b: &V3) -> f64 {
    if dot(a, b).lo > 0.0 {
        1.0
    } else {
        assert!(dot(a, b).hi < 0.0, "orientation undecided");
        -1.0
    }
}

fn scale3(a: &V3, s: f64) -> V3 {
    std::array::from_fn(|q| a[q].mul(Iv::p(s)))
}

fn norm(a: &V3) -> Iv {
    dot(a, a).sqrt()
}

/// The a-priori bound of how far the edge's float iso curve can lie from its exact `S(f, ·)`
/// (Euclidean, [`label_iso_bspline`]'s `γ_K |M|`, with the degrees along and across the
/// side both bounded by the larger of the net's): twice that bounds main's and the new
/// curve's distance. A rational net's homogeneous error `δQ`, `δw` maps to the Euclidean
/// `(|δQ| + |P| |δw|) / w_min`, `|P| <= |M| / w_min`.
fn iso_curve_move(srf: &Srf) -> f64 {
    let d = srf.p.max(srf.q);
    let m: [f64; 4] = std::array::from_fn(|c| {
        srf.net
            .iter()
            .flatten()
            .map(|cp| cp[c].abs())
            .fold(0.0, f64::max)
    });
    let norm = Iv::p(m[0])
        .mul(Iv::p(m[0]))
        .add(Iv::p(m[1]).mul(Iv::p(m[1])))
        .add(Iv::p(m[2]).mul(Iv::p(m[2])))
        .sqrt()
        .hi;
    let g = gam(14 * 3 * d + 7 * d + 3);
    let one = if srf.rational {
        let wmin = srf
            .net
            .iter()
            .flatten()
            .map(|cp| cp[3])
            .fold(f64::INFINITY, f64::min);
        assert!(wmin > 0.0, "a rational net with a non-positive weight");
        let p = Iv::p(norm).div(Iv::p(wmin)).hi;
        Iv::p(up_mul(g, norm))
            .add(Iv::p(up_mul(p, up_mul(g, m[3]))))
            .div(Iv::p(wmin))
            .hi
    } else {
        up_mul(g, norm)
    };
    up_mul(2.0, one)
}

/// MEASURED: the bound of a point solve's uv move. Its input, the edge's 3D point, moved
/// at most [`iso_curve_move`]; the solve's uv moves by that over the surface's smallest
/// singular value `σ` at the exact `(f, t)` (the local inverse's gain, evaluated at one
/// point, not enclosed), plus `8 u` of the larger value. The `8 u` is a measured allowance,
/// not a bound on the solver: its stopping tolerance is far coarser, and only the observed
/// moves (1 ulp, after the same number of steps on both trees) justify it.
fn solved_uv_bound(srf: &Srf, uv: [f64; 2], new: f64, main: f64) -> f64 {
    let (_, su, sv) = srf.exact(Iv::p(uv[0]), Iv::p(uv[1]));
    let (a, b, c) = (
        dot(&su, &su).mid(),
        dot(&su, &sv).mid(),
        dot(&sv, &sv).mid(),
    );
    let h = (a - c) / 2.0;
    let lmin = (a + c) / 2.0 - (h * h + b * b).sqrt();
    assert!(lmin > 0.0, "a singular surface at the iso point");
    iso_curve_move(srf) / lmin.sqrt() + 8.0 * U * new.abs().max(main.abs())
}

/// `EvaluateBinormal` at a point of the iso line (record `[point, binormal, tangent,
/// normal, uv, uv_vector]`): the references at the exact `(f, t)` (`t` the edge interval's
/// `evaluate(frac)`, as the kernel's), the enclosures the same chain rounded as the kernel
/// rounds it over the box holding the row's own uv and `(f, t)` ([`label_binormal_at`]),
/// so the reference lies inside the enclosure.
fn label_binormal(
    v: &[f64],
    main: &[f64],
    srf: &Srf,
    iso: &Iso,
    frac: f64,
    out: &mut BTreeMap<usize, Cert>,
) {
    let n = v.len();
    let (lo, hi) = (v[n - 3], v[n - 2]);
    let t = ev(lo, hi, frac).clamp(lo, hi);
    label_binormal_at(v, main, 0, true, t, srf, iso, out);
}

/// The binormal record `[point, binormal, tangent, normal, uv]` starting at `v[base]`,
/// evaluated at the iso line's parameter `t`, with the uv vector after it when
/// `uv_vector`. The normal's orientation is main's (`main`, the same record on main).
#[allow(clippy::too_many_arguments)] // test
fn label_binormal_at(
    v: &[f64],
    main: &[f64],
    base: usize,
    uv_vector: bool,
    t: f64,
    srf: &Srf,
    iso: &Iso,
    out: &mut BTreeMap<usize, Cert>,
) {
    let v = &v[base..];
    let main = &main[base..];
    let mut uv = [t, t];
    uv[iso.c] = iso.f;
    let b = line_bound(iso);
    for (q, r) in uv.into_iter().enumerate() {
        let exact = Cert::Exact {
            r,
            b: Some(b),
            what: "bin uv",
        };
        // main's own uv off the iso line by more than the trim evaluation's bound: the uv
        // is a point solve's (the edge use evaluated in 3D only), not the trim's. The exact
        // reference still certifies the row when the new value is closer to it or at
        // rounding; only a row it cannot certify falls back to the measured bound.
        let solved = Iv::p(main[12 + q]).sub(Iv::p(r)).abs().hi > b;
        let c = if solved && classify(v[12 + q], main[12 + q], &exact) == Class::Farther {
            Cert::Measured {
                bound: solved_uv_bound(srf, uv, v[12 + q], main[12 + q]),
                what: "bin uv (point solve)",
            }
        } else {
            exact
        };
        out.insert(base + 12 + q, c);
    }
    // The chain `n = ±unit(S_u × S_v)`, `b = unit(n × T)`, `uv_vector` the 2×2 solve of
    // `[S_u S_v]ᵀ[S_u S_v] x = [S_u S_v]ᵀ b`, once exactly at `(f, t)` (the reference) and
    // once as the kernel rounds it over the box holding its own uv and `(f, t)` (the
    // enclosure). The tangent `T` is the edge's 3D curve tangent: an input the fix does not
    // move (main's and the new row's bit for bit), which both chains take as it is, or, for
    // a `CrvOnSurf` edge, the certified `cos derivative` (its reference in the exact chain,
    // its enclosure in the rounded one).
    let tangent: Vec<&Cert> = (0..3).filter_map(|q| out.get(&(base + 6 + q))).collect();
    let (rt, et) = if tangent.is_empty() {
        assert_eq!(
            bits(&v[6..9]),
            bits(&main[6..9]),
            "the binormal's tangent moved"
        );
        (pts(v, 6), pts(v, 6))
    } else {
        let enc = |q: usize| match tangent[q] {
            Cert::Enc { r, e: Some(e), .. } => (*r, *e),
            c => panic!("the tangent's certificate {c:?}"),
        };
        (
            std::array::from_fn(|q| enc(q).0),
            std::array::from_fn(|q| enc(q).1),
        )
    };
    let chain = |uvb: [Iv; 2], round: bool| -> (V3, V3, [Iv; 2]) {
        let t3 = if round { et } else { rt };
        let (_, su, sv) = srf.eval(uvb[0], uvb[1], round);
        let c = rcross(&su, &sv, round);
        let n = scale3(&runit(&c, round), sign_of(&pts(main, 9), &c));
        let bn = runit(&rcross(&n, &t3, round), round);
        let r = |x: Iv| rnd(x, U, round);
        let a00 = rdot(&su, &su, 3, round);
        let a01 = rdot(&su, &sv, 3, round);
        let a11 = rdot(&sv, &sv, 3, round);
        let b0 = rdot(&su, &bn, 3, round);
        let b1 = rdot(&sv, &bn, 3, round);
        let det = rsub2(a00.mul(a11), a01.mul(a01), round);
        let uvv = [
            r(rsub2(b0.mul(a11), b1.mul(a01), round).div(det)),
            r(rsub2(a00.mul(b1), a01.mul(b0), round).div(det)),
        ];
        (n, bn, uvv)
    };
    let exact = [Iv::p(uv[0]), Iv::p(uv[1])];
    let (rn, rb, ruv) = chain(exact, false);
    let boxed = [
        Iv::hull(exact[0], Iv::p(v[12])),
        Iv::hull(exact[1], Iv::p(v[13])),
    ];
    let (kn, kb, kuv) = chain(boxed, true);
    for q in 0..3 {
        out.insert(
            base + 9 + q,
            Cert::Enc {
                r: rn[q],
                e: Some(kn[q]),
                what: "bin normal",
            },
        );
        out.insert(
            base + 3 + q,
            Cert::Enc {
                r: rb[q],
                e: Some(kb[q]),
                what: "bin binormal",
            },
        );
    }
    if !uv_vector {
        return;
    }
    for q in 0..2 {
        out.insert(
            base + 14 + q,
            Cert::Enc {
                r: ruv[q],
                e: Some(kuv[q]),
                what: "bin uv_vector",
            },
        );
    }
}

/// The 3D point and derivative of an edge whose curve is `CrvOnSurf(trim, S)` (the
/// `cos_open` fixtures), at the parameter the op evaluates: the references `S(f, t*)` and
/// `±S_w(f, t*)` (the ideal trim is the iso line itself), the enclosures those values
/// widened by the uv error of the float trim evaluation (`64 ε M`) and of the parameter
/// (`γ_3 (|lo| + |hi|)`) through the hull bounds, and by the surface rounding.
fn label_cos(v: &[f64], inputs: &Value, srf: &Srf, iso: &Iso, out: &mut BTreeMap<usize, Cert>) {
    let n = v.len();
    let (lo, hi) = (v[n - 3], v[n - 2]);
    let b = srf.bounds();
    let g = gam(srf_k(srf.p, srf.q));
    let e_uv = line_bound(iso);
    let mv = 1 - iso.c;
    let binormal = inputs["op"] == "binormal";
    let (t, e_t, didx) = if binormal {
        let frac = num(&inputs["frac"]);
        let t = ev(lo, hi, frac).clamp(lo, hi);
        let e_t = if frac == 0.0 || frac == 1.0 {
            0.0
        } else {
            up_mul(gam(3), up_add(lo.abs(), hi.abs()))
        };
        (Iv::p(t), e_t, 6)
    } else {
        // orientation: the record's derivative against the surface's along the moving axis
        let mut mid = [Iv::p(iso.f); 2];
        mid[mv] = Iv::p(lo).add(Iv::p(hi)).mul(Iv::p(0.5));
        let (_, su0, sv0) = srf.exact(mid[0], mid[1]);
        let smv0 = if mv == 1 { sv0 } else { su0 };
        let opp = (v[3 + mv] < 0.0) != (smv0[mv].mid() < 0.0);
        let tt = if opp {
            1.0 - num(&inputs["t"])
        } else {
            num(&inputs["t"])
        };
        let t = Iv::p(lo).add(Iv::p(tt).mul(Iv::p(hi).sub(Iv::p(lo))));
        (t, up_mul(gam(3), up_add(lo.abs(), hi.abs())), 3)
    };
    let mut uv = [Iv::p(iso.f); 2];
    uv[mv] = t;
    let (s, su, sv) = srf.exact(uv[0], uv[1]);
    let smv = if mv == 1 { sv } else { su };
    let sg = sign_of(&pts(v, didx), &smv);
    let mut r = [e_uv; 2];
    r[mv] = up_add(e_t, e_uv);
    // the parameter's own enclosure width moves the reference too
    let wt = t.hi - t.lo;
    r[mv] = up_add(r[mv], wt);
    for q in 0..3 {
        let rad = up_add(
            up_add(up_mul(b.lu[q], r[0]), up_mul(b.lv[q], r[1])),
            up_mul(g, b.m[q]),
        );
        out.insert(
            q,
            Cert::Enc {
                r: s[q],
                e: Some(s[q].widen(rad)),
                what: "cos point",
            },
        );
        let (l0, l1, sc) = if mv == 1 {
            (
                b.luv[q],
                b.lvv[q],
                Iv::p(2.0 * srf.q as f64).div(Iv::p(b.hv)).hi,
            )
        } else {
            (
                b.luu[q],
                b.luv[q],
                Iv::p(2.0 * srf.p as f64).div(Iv::p(b.hu)).hi,
            )
        };
        let rad = up_add(
            up_add(up_mul(l0, r[0]), up_mul(l1, r[1])),
            up_mul(up_mul(g, b.m[q]), sc),
        );
        let tq = smv[q].mul(Iv::p(sg));
        out.insert(
            didx + q,
            Cert::Enc {
                r: tq,
                e: Some(tq.widen(rad)),
                what: "cos derivative",
            },
        );
    }
}

impl Iv {
    fn mid(self) -> f64 {
        0.5 * self.lo + 0.5 * self.hi
    }
}

// ---- drop quality maxima ----------------------------------------------------------------------

/// The stations behind a row's reported drop quality (the data file's `D*` lines): the 3D
/// curve the drop ran on, and the stations of `drop_iso_curve` (`p, u, v` and its
/// `drop_line_dist`, `line_param`) and of `CrvOnSurf::surface_curve_max_distance_between`
/// (`t, u, v, s` and its `dist`, `dev`), as the kernel evaluated them.
#[derive(Debug, Default)]
struct Drop {
    /// The 3D edge curve (`None`: a `CrvOnSurf` over [`Drop::other_uv`]).
    curve: Option<Crv>,
    other_uv: Option<Crv>,
    isos: Vec<[f64; 5]>,
    gaps: Vec<[f64; 6]>,
}

impl Drop {
    fn point(&self, srf: &Srf, b: Option<&Bounds>, s: f64) -> V3 {
        match (&self.curve, &self.other_uv, b) {
            (Some(c), _, _) => c.kernel(s),
            (None, Some(t), Some(b)) => cos_point(srf, b, t, s),
            _ => panic!("drop data without a curve"),
        }
    }
}

/// `|x|` of an enclosure.
fn absiv(x: Iv) -> Iv {
    x.abs()
}

fn imax(xs: &[Iv]) -> Iv {
    Iv {
        lo: xs.iter().map(|x| x.lo).fold(0.0, f64::max),
        hi: xs.iter().map(|x| x.hi).fold(0.0, f64::max),
    }
}

/// The kernel's `eval_dist_and_dev` from its surface point and normal at `uv` and the curve
/// point `c`: `(|dist|, dev)` enclosed.
fn station_enc(srf: &Srf, uv: [Iv; 2], c: &V3, round: bool) -> (Iv, Iv) {
    let r = |x: Iv| rnd(x, U, round);
    let (p, su, sv) = srf.eval(uv[0], uv[1], round);
    let n = runit(&rcross(&su, &sv, round), round);
    let gap: V3 = std::array::from_fn(|q| r(c[q].sub(p[q])));
    let d = rdot(&gap, &n, 3, round);
    let npt: V3 = std::array::from_fn(|q| r(r(d.mul(n[q])).add(p[q])));
    let diff: V3 = std::array::from_fn(|q| r(c[q].sub(npt[q])));
    let dev = r(rdot(&diff, &diff, 3, round).clamp0().sqrt());
    (absiv(d), dev)
}

/// `line_point_distance(S(uv), n(uv), c)`: `(|line_param|, drop_line_dist)` enclosed (`n`
/// unit, so `line_param = (c − S).n / |n|²` with `|n|²` within rounding of 1).
fn iso_enc(srf: &Srf, uv: [Iv; 2], c: &V3, round: bool) -> (Iv, Iv) {
    let r = |x: Iv| rnd(x, U, round);
    let (p, su, sv) = srf.eval(uv[0], uv[1], round);
    let n = runit(&rcross(&su, &sv, round), round);
    let gap: V3 = std::array::from_fn(|q| r(c[q].sub(p[q])));
    let lp = r(rdot(&gap, &n, 3, round).div(rdot(&n, &n, 3, round)));
    let pnt: V3 = std::array::from_fn(|q| r(r(lp.mul(n[q])).add(p[q])));
    let diff: V3 = std::array::from_fn(|q| r(pnt[q].sub(c[q])));
    let dld = r(rdot(&diff, &diff, 3, round).clamp0().sqrt());
    (absiv(lp), dld)
}

/// The closest approach `s*` of `curve` to the line `P + R nvec`, near `s0`: the root of
/// `h(s) = (w − (w.n̂)n̂).C'(s)`, `w = C(s) − P`, located by secant steps in doubles and
/// then certified by interval Newton: for a box `S` around the estimate `m`,
/// `N = m − h(m) / h'(S)` with `h' = |C'|² − (C'.n̂)² + (w − (w.n̂)n̂).C''` enclosed over `S`
/// ([`Crv::homog_iv`]); `N` strictly inside `S` proves `S` holds exactly one root, and that
/// it lies in `N`. The box is then contracted by further Newton steps (`S ∩ N`). Returns
/// the final box (a few hundred ulps at most: `h` is enclosed only to the rounding of its
/// coordinates).
fn closest_param(curve: &Crv, p: &V3, nvec: &V3, s0: f64) -> Iv {
    let nn = dot(nvec, nvec);
    let perp_of = |c: &V3| -> V3 {
        let w = sub3(c, p);
        let wn = dot(&w, nvec);
        std::array::from_fn(|q| w[q].sub(wn.mul(nvec[q]).div(nn)))
    };
    let h = |s: f64| {
        let (c, cd) = curve.exact(s);
        dot(&perp_of(&c), &cd)
    };
    let dh = |s: Iv| {
        let (c, cd, cdd, _) = curve.homog_iv(s);
        let cn = dot(&cd, nvec);
        dot(&cd, &cd)
            .sub(cn.mul(cn).div(nn))
            .add(dot(&perp_of(&c), &cdd))
    };
    let newton = |m: f64, s: Iv| Iv::p(m).sub(h(m).div(dh(s)));
    let (mut a, mut b) = (s0, s0 + 2f64.powi(-30) * s0.abs().max(1.0));
    let (mut fa, mut fb) = (h(a).mid(), h(b).mid());
    for _ in 0..80 {
        if fb == fa || fb == 0.0 {
            break;
        }
        let c = b - fb * (b - a) / (fb - fa);
        a = b;
        fa = fb;
        b = c;
        fb = h(b).mid();
        if (b - a).abs() <= 4.0 * f64::EPSILON * b.abs().max(1e-300) {
            break;
        }
    }
    let mut d = 4.0 * f64::EPSILON * b.abs().max(f64::MIN_POSITIVE);
    for _ in 0..60 {
        let s = Iv::out(b - d, b + d);
        let n = newton(b, s);
        if s.lo < n.lo && n.hi < s.hi {
            // `S` holds exactly one root, inside `N`: contract
            let mut x = n;
            for _ in 0..8 {
                let m = x.mid();
                let y = newton(m, x);
                let z = Iv {
                    lo: x.lo.max(y.lo),
                    hi: x.hi.min(y.hi),
                };
                assert!(z.lo <= z.hi, "Newton contraction left the root's box");
                if z.lo == x.lo && z.hi == x.hi {
                    break;
                }
                x = z;
            }
            return x;
        }
        d *= 2.0;
    }
    panic!("interval Newton did not certify a root near {b}");
}

/// The references of the ideal pipeline at the stations (the UV curve the exact iso line
/// `(f, ·)`): `(|line_param|, drop_line_dist)` at an iso station and `(|dist|, dev)` at a gap
/// station, enclosed; `0` for a `CrvOnSurf` edge (its ideal curve is `S(f, ·)` itself).
fn station_refs(srf: &Srf, iso: &Iso, d: &Drop, param: f64, gap: bool) -> (Iv, Iv) {
    let Some(curve) = &d.curve else {
        return (Iv::p(0.0), Iv::p(0.0));
    };
    let mut uv = [Iv::p(param); 2];
    uv[iso.c] = Iv::p(iso.f);
    if !gap {
        let (c, _) = curve.exact(param);
        return iso_enc(srf, uv, &c, false);
    }
    // the ideal pipeline's values at the certified `s*`
    let (p, su, sv) = srf.exact(uv[0], uv[1]);
    let s = closest_param(curve, &p, &cross(&su, &sv), param);
    gap_ref(srf, uv, curve, s)
}

/// The ideal gap station's `(|dist|, dev)` at the closest approach `s*` in the certified box
/// `S`. Evaluating `C` over `S` directly adds `|C'| w(S)` to every coordinate, so the
/// values come from a double `m` in `S` near `s*` instead. With `n` the exact unit normal at
/// the exact `uv`, `w(s) = C(s) − P`, `perp(s) = w − (w.n) n` and `h = perp.C'`:
/// - `h' = |C'|² − (C'.n)² + perp.C''` is positive on `S` ([`closest_param`] certified
///   it), so `|m − s*| <= x = |h(m)| / min h'(S)` (mean value theorem on `h`);
/// - `dist(s) = w.n`: `dist(s*) ∈ dist(m) + (C'(S).n) [−x, x]`;
/// - `dev(s)² = |perp|²` has derivative `2h` and second derivative `2h'`, so it is convex on
///   `S` with its minimum at `s*`: `dev(m)² − max h'(S) x² <= dev(s*)² <= dev(m)²`.
fn gap_ref(srf: &Srf, uv: [Iv; 2], curve: &Crv, s: Iv) -> (Iv, Iv) {
    let (p, su, sv) = srf.eval(uv[0], uv[1], false);
    let n = runit(&rcross(&su, &sv, false), false);
    let perp_at = |c: &V3| -> V3 {
        let w = sub3(c, &p);
        let wn = dot(&w, &n);
        std::array::from_fn(|q| w[q].sub(wn.mul(n[q])))
    };
    let h_at = |m: f64| {
        let (c, cd) = curve.exact(m);
        dot(&perp_at(&c), &cd)
    };
    let (cs, cds, cdds, _) = curve.homog_iv(s);
    let cn = dot(&cds, &n);
    let dh = dot(&cds, &cds)
        .sub(cn.mul(cn))
        .add(dot(&perp_at(&cs), &cdds));
    assert!(
        dh.lo > 0.0,
        "h' not positive over the certified box {s:?}: {dh:?}"
    );
    // a double in `S` near `s*`: Newton steps in doubles from the box's middle
    let mut m = s.mid();
    for _ in 0..4 {
        let next = m - h_at(m).mid() / dh.mid();
        if !s.contains(next) || next == m {
            break;
        }
        m = next;
    }
    let x = Iv::p(h_at(m).mag()).div(Iv::p(dh.lo)).hi;
    let ds = Iv { lo: -x, hi: x };
    let (cm, _) = curve.exact(m);
    let dist = dot(&sub3(&cm, &p), &n).add(cn.mul(ds));
    let pm = perp_at(&cm);
    let dev2m = dot(&pm, &pm);
    let dev2 = Iv {
        lo: dev2m.lo - up_mul(dh.hi, up_mul(x, x)),
        hi: dev2m.hi,
    };
    let dev2 = Iv::out(dev2.lo, dev2.hi);
    (absiv(dist), dev2.clamp0().sqrt())
}

/// `sqrt(d² + v²)` as the kernel rounds it (`round`) or exactly.
fn comb(d: Iv, v: Iv, round: bool) -> Iv {
    let r = |x: Iv| if round { x.rel(U) } else { x };
    let q = r(r(d.mul(d)).add(r(v.mul(v))));
    r(q.clamp0().sqrt())
}

/// Whether `curve` is the face's boundary curve along the iso line `iso`: `f` a clamped end
/// of the fixed direction's knots (`p + 1` equal knots), and the curve of the moving
/// direction's degree and knots with the net's boundary row as its control points, bit for
/// bit (a non-rational net's sentinel weight against weight `1`). Then
/// `C(s) = S(f, s)` exactly for every `s`: at a clamped end the fixed direction's basis is
/// exactly `1` on the boundary row and `0` elsewhere.
fn is_boundary(srf: &Srf, iso: &Iso, curve: &Crv) -> bool {
    let (fixed, deg, moving, mdeg) = if iso.c == 0 {
        (&srf.u, srf.p, &srf.v, srf.q)
    } else {
        (&srf.v, srf.q, &srf.u, srf.p)
    };
    let n = fixed.len();
    let rows = if iso.c == 0 {
        srf.net.len()
    } else {
        srf.net[0].len()
    };
    let at = if fixed[..=deg].iter().all(|&x| x == iso.f) {
        0
    } else if fixed[n - 1 - deg..].iter().all(|&x| x == iso.f) {
        rows - 1
    } else {
        return false;
    };
    let row: Vec<[f64; 4]> = if iso.c == 0 {
        srf.net[at].clone()
    } else {
        srf.net.iter().map(|r| r[at]).collect()
    };
    let cp_eq = |a: &[f64; 4], b: &[f64; 4]| {
        (0..3).all(|q| a[q].to_bits() == b[q].to_bits())
            && (a[3].to_bits() == b[3].to_bits() || (!srf.rational && b[3] == 1.0))
    };
    curve.p == mdeg
        && bits(&curve.knots) == bits(moving)
        && row.len() == curve.cps.len()
        && row.iter().zip(&curve.cps).all(|(a, b)| cp_eq(a, b))
}

fn bits(xs: &[f64]) -> Vec<u64> {
    xs.iter().map(|x| x.to_bits()).collect()
}

/// The certificates of a row's reported drop quality (`state[0]`, or `state[0..2]`), from
/// its stations. Every station's dumped value must lie in its enclosure (the data are the
/// kernel's), and every station must be on the exact iso line: an iso station's uv is
/// `(f, param)` bit for bit; a gap station's curve parameter is `s = t` bit for bit and its
/// uv is the degree-1 trim's float evaluation at `t`, within the a-priori [`line_bound`] of
/// `(f, t)`. The enclosures are then of the kernel's float evaluation at those inputs (no
/// solver residual enters). The references: `0` at every station when the edge's 3D curve is the face's
/// boundary row at `f` ([`is_boundary`]) or a `CrvOnSurf` over the exact iso line; else
/// the certified ideal values of [`station_refs`].
fn label_drop(case: &str, srf: &Srf, iso: &Iso, d: &Drop, out: &mut BTreeMap<usize, Cert>) {
    let b = d.curve.is_none().then(|| srf.bounds());
    let at = |param: f64| {
        let mut want = [param; 2];
        want[iso.c] = iso.f;
        want.map(f64::to_bits)
    };
    for st in &d.isos {
        assert_eq!(
            [st[1], st[2]].map(f64::to_bits),
            at(st[0]),
            "{case}: iso station {st:?} off the exact iso line"
        );
    }
    // a gap station's uv is the returned trim evaluated as a float B-spline (the
    // `CrvOnSurf`'s UV curve): within the trim evaluation's a-priori `line_bound` of the
    // exact `(f, t)`, the certified input error; its curve parameter is the station's own
    let lb = line_bound(iso);
    for st in &d.gaps {
        let mut want = [st[0]; 2];
        want[iso.c] = iso.f;
        assert!(
            [st[1], st[2]]
                .into_iter()
                .zip(want)
                .all(|(got, want)| Iv::p(got).sub(Iv::p(want)).abs().hi <= lb),
            "{case}: gap station {st:?} off the exact iso line"
        );
        assert_eq!(
            st[3].to_bits(),
            st[0].to_bits(),
            "{case}: gap station {st:?}: the curve parameter is not the station's"
        );
    }
    let zero = match (&d.curve, &d.other_uv) {
        (Some(c), _) => is_boundary(srf, iso, c),
        (None, Some(u)) => {
            let line = iso_curve(iso);
            assert!(
                u.p == 1
                    && bits(&u.knots) == bits(&line.knots)
                    && u.cps.iter().zip(&line.cps).all(|(a, b)| bits(a) == bits(b)),
                "{case}: the CrvOnSurf's UV curve is not the exact iso line"
            );
            true
        }
        _ => panic!("{case}: drop data without a curve"),
    };
    let refs = |param: f64, gap: bool| {
        if zero {
            (Iv::p(0.0), Iv::p(0.0))
        } else {
            station_refs(srf, iso, d, param, gap)
        }
    };
    let mut e = (Vec::new(), Vec::new());
    let mut r = (Vec::new(), Vec::new());
    // the dumped values, and whether the station is a gap station
    let mut x = (Vec::new(), Vec::new());
    for st in &d.isos {
        let [p, u, v, dld, lp] = *st;
        let c = d.point(srf, b.as_ref(), p);
        let (ed, ev) = iso_enc(srf, [Iv::p(u), Iv::p(v)], &c, true);
        assert!(
            ed.contains(lp.abs()) && ev.contains(dld),
            "{case}: iso station {st:?} outside {ed:?} {ev:?}"
        );
        let (rd, rv) = refs(p, false);
        e.0.push(ed);
        e.1.push(ev);
        r.0.push(rd);
        r.1.push(rv);
        x.0.push((lp.abs(), false));
        x.1.push((dld, false));
    }
    for st in &d.gaps {
        let [t, u, v, s, dist, dev] = *st;
        let c = d.point(srf, b.as_ref(), s);
        // the station's float uv and the exact `(f, t)` (within `line_bound` of it,
        // asserted above): the enclosure holds the kernel's value and the exact chain's
        let mut want = [Iv::p(t); 2];
        want[iso.c] = Iv::p(iso.f);
        let uv = [Iv::hull(Iv::p(u), want[0]), Iv::hull(Iv::p(v), want[1])];
        let (ed, ev) = station_enc(srf, uv, &c, true);
        assert!(
            ed.contains(dist) && ev.contains(dev),
            "{case}: gap station {st:?} outside {ed:?} {ev:?}"
        );
        let (rd, rv) = refs(t, true);
        e.0.push(ed);
        e.1.push(ev);
        r.0.push(rd);
        r.1.push(rv);
        x.0.push((dist, true));
        x.1.push((dev, true));
    }
    let (ed, ev, rd, rv) = (imax(&e.0), imax(&e.1), imax(&r.0), imax(&r.1));
    // whether only gap stations attain the reported maximum
    let by_gap = |xs: &[(f64, bool)]| {
        let mx = xs.iter().map(|y| y.0).fold(0.0, f64::max);
        mx > 0.0 && xs.iter().all(|y| y.0 != mx || y.1)
    };
    let (gd, gv) = (by_gap(&x.0), by_gap(&x.1));
    let tag = |g: bool, a: &'static str, b: &'static str| if g { b } else { a };
    if ["create_plain", "goc", "create_dev0"]
        .iter()
        .any(|k| case.contains(k))
    {
        out.insert(
            0,
            Cert::Enc {
                r: comb(rd, rv, false),
                e: Some(comb(ed, ev, true)),
                what: tag(
                    gd && gv,
                    "drop quality sqrt(dist² + dev²), max over stations",
                    "drop quality sqrt(dist² + dev²), max set at gap stations",
                ),
            },
        );
    } else {
        out.insert(
            0,
            Cert::Enc {
                r: rd,
                e: Some(ed),
                what: tag(
                    gd,
                    "drop distance, max over stations",
                    "drop distance, max set at a gap station",
                ),
            },
        );
        out.insert(
            1,
            Cert::Enc {
                r: rv,
                e: Some(ev),
                what: tag(
                    gv,
                    "drop deviation, max over stations",
                    "drop deviation, max set at a gap station",
                ),
            },
        );
    }
}

// ---- the data file ---------------------------------------------------------------------------

struct El {
    key: String,
    i: usize,
    main: f64,
    new: f64,
    want: String,
}

#[derive(Default)]
struct Data {
    /// `(unit, case)` -> moved elements.
    els: BTreeMap<(String, String), Vec<El>>,
    drops: BTreeMap<String, Drop>,
    /// `(unit, case)` -> main's whole vector of the row's moved key (`M`), bit for bit.
    mains: BTreeMap<(String, String), (String, Vec<f64>)>,
    /// `(unit, case)` -> main's committed golden of that key (`O`).
    olds: BTreeMap<(String, String), Value>,
    /// `(unit, case)` -> main's Gauss-Kronrod decision tape of every replayed face (`T`).
    tapes: BTreeMap<(String, String), Vec<Vec<bool>>>,
    /// The blade hub face's surface and domain.
    blade: Option<(Srf, [f64; 4])>,
}

fn f(s: &str) -> f64 {
    s.parse().unwrap_or_else(|_| panic!("number {s}"))
}

fn fs(s: &str) -> Vec<f64> {
    s.split(',').map(f).collect()
}

fn cps(s: &str) -> Vec<[f64; 4]> {
    s.split(';')
        .map(|c| {
            let c = fs(c);
            [c[0], c[1], c[2], c[3]]
        })
        .collect()
}

/// `deg|knots|cps`: degree, full knot vector, homogeneous control points.
fn crv(s: &str) -> Crv {
    let p: Vec<&str> = s.split('|').collect();
    Crv {
        p: p[0].parse().unwrap(),
        knots: fs(p[1]),
        cps: cps(p[2]),
    }
}

/// `p|q|u knots|v knots|net (rows along u joined by '/')|domain`.
fn srf(s: &str) -> (Srf, [f64; 4]) {
    let p: Vec<&str> = s.split('|').collect();
    let net: Vec<Vec<[f64; 4]>> = p[4].split('/').map(cps).collect();
    let rational = net.iter().flatten().any(|cp| cp[3] > -1e300);
    let d = fs(p[5]);
    let s = Srf {
        net,
        rational,
        p: p[0].parse().unwrap(),
        q: p[1].parse().unwrap(),
        u: fs(p[2]),
        v: fs(p[3]),
    };
    (s, [d[0], d[1], d[2], d[3]])
}

/// A `T` line's tape: `len` decisions packed most significant bit first (`1` accept, `0`
/// split) in hex, the padding bits `0`.
fn tape(len: usize, hex: &str) -> Vec<bool> {
    assert_eq!(hex.len(), len.div_ceil(8) * 2, "T length");
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    let bit = |i: usize| bytes[i / 8] >> (7 - i % 8) & 1 == 1;
    assert!((len..bytes.len() * 8).all(|i| !bit(i)), "T padding");
    (0..len).map(bit).collect()
}

fn data() -> Data {
    let mut d = Data::default();
    for line in DATA
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let c: Vec<&str> = line.split('\t').collect();
        match c[0] {
            "E" => d
                .els
                .entry((c[1].to_owned(), c[2].to_owned()))
                .or_default()
                .push(El {
                    key: c[3].to_owned(),
                    i: c[4].parse().unwrap(),
                    main: f(c[5]),
                    new: f(c[6]),
                    want: c[7].to_owned(),
                }),
            "DC" => d.drops.entry(c[1].to_owned()).or_default().curve = Some(crv(c[2])),
            "DU" => d.drops.entry(c[1].to_owned()).or_default().other_uv = Some(crv(c[2])),
            "DI" => {
                let x: Vec<f64> = c[2..7].iter().map(|s| f(s)).collect();
                d.drops
                    .entry(c[1].to_owned())
                    .or_default()
                    .isos
                    .push([x[0], x[1], x[2], x[3], x[4]]);
            }
            "DG" => {
                let x: Vec<f64> = c[2..8].iter().map(|s| f(s)).collect();
                d.drops
                    .entry(c[1].to_owned())
                    .or_default()
                    .gaps
                    .push([x[0], x[1], x[2], x[3], x[4], x[5]]);
            }
            "M" => {
                let bits = c[4]
                    .split(',')
                    .map(|b| f64::from_bits(u64::from_str_radix(b, 16).unwrap()))
                    .collect();
                d.mains
                    .insert((c[1].to_owned(), c[2].to_owned()), (c[3].to_owned(), bits));
            }
            "O" => {
                d.olds.insert(
                    (c[1].to_owned(), c[2].to_owned()),
                    serde_json::from_str(c[4]).unwrap(),
                );
            }
            "T" => {
                let faces = d
                    .tapes
                    .entry((c[1].to_owned(), c[2].to_owned()))
                    .or_default();
                assert_eq!(c[3].parse::<usize>().unwrap(), faces.len(), "T face order");
                faces.push(tape(c[4].parse().unwrap(), c[5]));
            }
            "S" => d.blade = Some(srf(c[2])),
            k => panic!("unknown line kind {k}"),
        }
    }
    d
}

// ---- the labels the prototypes left to the driver --------------------------------------------

/// `NormalizedEvaluate` in parameter space on an iso trim: the point `(f, s)` at the
/// parameter the op evaluates (`s` from the normalised `t` over the trim interval as the
/// kernel maps it, `1 − t` when the edge use runs against the trim), the derivative the unit
/// vector along the moving axis.
fn label_norm_uv(main: &[f64], inputs: &Value, iso: &Iso, out: &mut BTreeMap<usize, Cert>) {
    let c = iso.c;
    let (t0, t1) = iso.ivl;
    // the direction from main's derivative (not the fix's output): nonzero, along the
    // moving axis
    let d = main[3 + 1 - c];
    assert!(d != 0.0 && d.abs() > 0.5, "main's derivative {d}");
    let opp = (d < 0.0) == (t1 > t0);
    let t = num(&inputs["t"]);
    let tt = if opp { 1.0 - t } else { t };
    let mut pt = [ev(t0, t1, tt); 2];
    pt[c] = iso.f;
    let b = line_bound(iso);
    for (q, r) in pt.into_iter().enumerate() {
        out.insert(
            q,
            Cert::Exact {
                r,
                b: Some(b),
                what: "norm uv point",
            },
        );
    }
    if inputs["want_deriv"] == true {
        let mut dd = [0.0; 2];
        dd[1 - c] = if d < 0.0 { -1.0 } else { 1.0 };
        for (q, r) in dd.into_iter().enumerate() {
            out.insert(
                3 + q,
                Cert::Exact {
                    r,
                    b: None,
                    what: "norm uv deriv",
                },
            );
        }
    }
}

/// The degree-1 UV trim `iso` is, as a curve.
fn iso_curve(iso: &Iso) -> Crv {
    let (t0, t1) = iso.ivl;
    let mut a = [t0, t0, 0.0, 1.0];
    let mut b = [t1, t1, 0.0, 1.0];
    a[iso.c] = iso.f;
    b[iso.c] = iso.f;
    Crv {
        p: 1,
        knots: vec![t0, t0, t1, t1],
        cps: vec![a, b],
    }
}

/// The cached edge/face gap of a `CrvOnSurf` edge (`get_or_create` records it at 0): the
/// edge's curve is a curve on the face's own surface (checked on the replayed model,
/// [`Model::cos_on_face`]), so every point of it is on the face: the reference is `0`.
fn label_goc(out: &mut BTreeMap<usize, Cert>) {
    out.insert(
        0,
        Cert::Exact {
            r: 0.0,
            b: None,
            what: "cached edge/face gap (a CrvOnSurf on its face)",
        },
    );
}

// ---- the replayed model ----------------------------------------------------------------------

/// A row's B-rep and edge use, built from the case as the unit builds them.
struct Model {
    brep: Brep,
    eu: EdgeuseId,
}

fn bspline_surface(s: &Surface) -> &BSplineSurface {
    match s.kind() {
        SurfaceKind::BSpline(b) => b,
        _ => panic!("not a B-spline surface"),
    }
}

/// A B-spline curve as the data lines hold it: full knot vector, homogeneous points.
fn crv_of(c: &BSplineCurve) -> Crv {
    let (kv, km) = c.knots(None).unwrap();
    let knots = kv
        .iter()
        .zip(&km)
        .flat_map(|(x, m)| std::iter::repeat_n(*x, *m))
        .collect();
    let cps = (0..c.number_control_points().unwrap())
        .map(|i| {
            let (q, w) = c
                .control_point(ControlPointForm::HomogeneousRational, i)
                .unwrap();
            [q.x, q.y, q.z, w]
        })
        .collect();
    Crv {
        p: c.degree().unwrap(),
        knots,
        cps,
    }
}

fn same_crv(a: &Crv, b: &Crv) -> bool {
    a.p == b.p
        && bits(&a.knots) == bits(&b.knots)
        && a.cps.len() == b.cps.len()
        && a.cps.iter().zip(&b.cps).all(|(x, y)| bits(x) == bits(y))
}

fn same_nurb(a: &BSplineSurface, b: &BSplineSurface) -> bool {
    let (Some(a), Some(b)) = (a.gw_nurb(), b.gw_nurb()) else {
        return false;
    };
    let cp = |c: &Vec<Vec<solidcore::nurbs::math::CPoint>>| -> Vec<u64> {
        c.iter()
            .flatten()
            .flat_map(|p| [p.x, p.y, p.z, p.w])
            .map(f64::to_bits)
            .collect()
    };
    a.degrees() == b.degrees()
        && bits(a.u_knots()) == bits(b.u_knots())
        && bits(a.v_knots()) == bits(b.v_knots())
        && cp(&a.cpts()) == cp(&b.cpts())
}

impl Model {
    fn of(case: &goldens::Case) -> Model {
        let (brep, eu) = scene(case).unwrap().expect("the case builds its edge use");
        Model { brep, eu }
    }
    fn curve(&self, eu: EdgeuseId) -> &Curve {
        let e = eu1::edge(&self.brep, eu).unwrap().expect("an edge");
        self.brep.edge(e).unwrap().curve.as_ref().expect("a curve")
    }
    fn surface(&self) -> &BSplineSurface {
        let f = eu1::face(&self.brep, self.eu).unwrap().expect("a face");
        bspline_surface(
            self.brep
                .face(f)
                .unwrap()
                .surface
                .as_ref()
                .expect("a surface"),
        )
    }
    /// The edge's 3D curve, a B-spline, as a data line holds it.
    fn bspline_curve(&self) -> Crv {
        match self.curve(self.eu).kind() {
            CurveKind::BSpline(b) => crv_of(b),
            _ => panic!("the edge curve is not a B-spline"),
        }
    }
    /// The UV curve of the edge's curve when it is a curve on the face's own surface (the
    /// same B-spline, bit for bit).
    fn cos_on_face(&self) -> Option<Crv> {
        let CurveKind::CrvOnSurf(c) = self.curve(self.eu).kind() else {
            return None;
        };
        if !same_nurb(bspline_surface(c.surface()), self.surface()) {
            return None;
        }
        match c.uv_curve().kind() {
            CurveKind::BSpline(b) => Some(crv_of(b)),
            _ => None,
        }
    }
    /// The domain side `(fixed coordinate, value)` whose `create_iso_parametric_curve` the
    /// edge curve of `eu` is, bit for bit, if any.
    fn iso_side(&self, k: &Kernel, eu: EdgeuseId, dom: [f64; 4]) -> Option<(usize, f64)> {
        let CurveKind::BSpline(b) = self.curve(eu).kind() else {
            return None;
        };
        let have = crv_of(b);
        let s = self.surface();
        let domain =
            Extent2d::from_points(Point2::new(dom[0], dom[1]), Point2::new(dom[2], dom[3]));
        for (c, param) in [(0, SurfParamType::U), (1, SurfParamType::V)] {
            for side in [dom[c], dom[c + 2]] {
                let opts = IsoCurveOptions {
                    domain: Some(domain),
                    uv_iso_curve: false,
                };
                let Ok(iso) = s.create_iso_parametric_curve(k, param, side, opts) else {
                    continue;
                };
                if same_crv(&crv_of(&iso.curve), &have) {
                    return Some((c, side));
                }
            }
        }
        None
    }
}

/// A binormal row's vertex/face-gap-cached flag (`state[52]`): proven `0` for the ideal
/// pipeline from the model. The edge use's curve and that of its loop neighbour at its
/// vertex are `create_iso_parametric_curve` of two different domain sides (bit for bit),
/// (two sides of different coordinates), and the vertex is the two edges' common vertex in
/// the topology: the only common point of the two sides' uv lines is the domain corner
/// they span, so the vertex's ideal uv is that corner. The ideal trims are
/// the exact iso lines of those sides, which meet exactly at the corner. The joint's uv gap
/// is `0 <= EFF_ZERO (1 + |uv|)`, so `sm_FixUVTrimCurves` never computes the vertex/face
/// gap.
fn prove_corner_flag(m: &Model, dom: [f64; 4]) {
    let k = Kernel::default();
    let vu = eu1::vertexuse(&m.brep, m.eu)
        .unwrap()
        .expect("a vertex use");
    let v = vertexuse::vertex(&m.brep, vu).unwrap().expect("a vertex");
    let (c0, f0) = m
        .iso_side(&k, m.eu, dom)
        .expect("the edge is a domain side's iso curve");
    let has = |eu: EdgeuseId| {
        let e = eu1::edge(&m.brep, eu).unwrap().expect("an edge");
        m.brep.edge(e).unwrap().vertices.contains(&v)
    };
    assert!(has(m.eu), "the vertex use's vertex is not the edge's");
    let mut met = false;
    for nb in [
        eu1::cw_edgeuse(&m.brep, m.eu),
        eu1::ccw_edgeuse(&m.brep, m.eu),
    ] {
        let nb = nb.unwrap().expect("a loop neighbour");
        if !has(nb) {
            continue;
        }
        let (c1, f1) = m
            .iso_side(&k, nb, dom)
            .expect("the neighbour is a domain side's iso curve");
        assert_ne!(c0, c1, "the two sides meet at a corner");
        let mut corner = [0.0; 2];
        corner[c0] = f0;
        corner[c1] = f1;
        assert!(
            [dom[0], dom[2]].contains(&corner[0]) && [dom[1], dom[3]].contains(&corner[1]),
            "corner {corner:?} of {dom:?}"
        );
        met = true;
    }
    assert!(met, "no loop neighbour meets the edge at its vertex");
}

/// The blade hub edge (a file B-rep): its 3D curve `C` is the face's `v = f` boundary
/// curve itself (same degree, knots and homogeneous control points: checked), so the ideal
/// drop's distance and deviation are `0`; the lifted samples' references are `S(t, f)`.
fn label_blade(v: &[f64], srf: &Srf, iso: &Iso, curve: &Crv, out: &mut BTreeMap<usize, Cert>) {
    assert_eq!(iso.c, 1, "the blade trim is a v iso line");
    let j = if iso.f == srf.v[0] {
        0
    } else {
        assert_eq!(iso.f, srf.v[srf.v.len() - 1]);
        srf.net[0].len() - 1
    };
    assert!(
        srf.v[..=srf.q].iter().all(|&x| x == srf.v[0]),
        "v knots not clamped"
    );
    assert!(srf.v[srf.v.len() - 1 - srf.q..]
        .iter()
        .all(|&x| x == srf.v[srf.v.len() - 1]));
    assert_eq!(
        (srf.p, &srf.u),
        (curve.p, &curve.knots),
        "the boundary is not C"
    );
    for (i, row) in srf.net.iter().enumerate() {
        assert_eq!(
            row[j].map(f64::to_bits),
            curve.cps[i].map(f64::to_bits),
            "net row {i}"
        );
    }
    out.insert(
        0,
        Cert::Exact {
            r: 0.0,
            b: None,
            what: "drop distance (C is the iso)",
        },
    );
    out.insert(
        1,
        Cert::Exact {
            r: 0.0,
            b: None,
            what: "drop deviation (C is the iso)",
        },
    );
    let b = line_bound(iso);
    // `put_created_lift`: `5, type`, then per sample `0, t, C(t), uv, S(uv)`
    for k in 0..5 {
        let base = 35 + 10 * k;
        assert_eq!(v[base], 0.0, "lift sample {k}");
        let t = v[base + 1];
        let mut uv = [Iv::p(t); 2];
        uv[iso.c] = Iv::p(iso.f);
        out.insert(
            base + 5 + iso.c,
            Cert::Exact {
                r: iso.f,
                b: Some(b),
                what: "lift uv fixed",
            },
        );
        out.insert(
            base + 6 - iso.c,
            Cert::Exact {
                r: t,
                b: Some(b),
                what: "lift uv param",
            },
        );
        let (s, _, _) = srf.exact(uv[0], uv[1]);
        let (e, _, _) = srf.kernel(v[base + 5], v[base + 6]);
        for q in 0..3 {
            out.insert(
                base + 7 + q,
                Cert::Enc {
                    r: s[q],
                    e: Some(e[q]),
                    what: "lift point",
                },
            );
        }
    }
}

// ---- the driver --------------------------------------------------------------------------------

fn certificates(
    unit: &str,
    case: &goldens::Case,
    status: i64,
    v: &[f64],
    main: &[f64],
    d: &Data,
) -> BTreeMap<usize, Cert> {
    let (name, inputs) = (case.name.as_str(), &case.inputs);
    let mut out = BTreeMap::new();
    if unit == "blend.fillet_solver_p2" {
        label_rail_eu(case, v, &mut out);
        return out;
    }
    if unit == "mass.integrator" || unit == "boolean.api_brep" {
        let tapes = d
            .tapes
            .get(&(unit.to_owned(), name.to_owned()))
            .unwrap_or_else(|| panic!("{name}: no T lines (main's partition)"));
        if unit == "mass.integrator" {
            label_mass(case, v, main, tapes, &mut out);
        } else {
            label_api_areas(case, v, main, tapes, &mut out);
        }
        return out;
    }
    let dom = if let Some(x) = inputs["brep"].get("dom") {
        let a = nums(x);
        [a[0], a[1], a[2], a[3]]
    } else {
        assert_eq!(name, BLADE, "a row without a domain");
        d.blade.as_ref().expect("S line").1
    };
    match unit {
        "topology.vertexuse" => {
            label_vertexuse(case, main, dom, &mut out);
            return out;
        }
        "topology.brep_validation" => {
            label_validation(case, v, main, dom, &mut out);
            return out;
        }
        "query.face_p2" => {
            label_query(case, v, main, dom, &mut out);
            return out;
        }
        _ => {}
    }
    let iso = label_trims(v, main, dom, &mut out);
    if unit == "topology.face_p3" {
        // `uv_trim`'s parameter-space joint gaps: the mean over the joints off the pole, the
        // maximum including the pole, whose parameter gap is the whole side, 1
        out.insert(
            6,
            Cert::Exact {
                r: 0.0,
                b: None,
                what: "mean joint uv gap",
            },
        );
        out.insert(
            7,
            Cert::Exact {
                r: 1.0,
                b: None,
                what: "max joint uv gap (the pole)",
            },
        );
        if v[7] != main[7] {
            // on the replayed model: the face's surface has a pole (a domain side whose
            // boundary row of control points is one point, bit for bit), and every such
            // side's parameter extent is exactly 1
            let (b, f) = face_scene(case).unwrap().expect("the case builds its face");
            let srf = Srf::from_input(inputs);
            assert!(
                srf.is(bspline_surface(
                    b.face(f).unwrap().surface.as_ref().expect("a surface")
                )),
                "{name}: the input net is not the face's surface"
            );
            let (nu, nv) = (srf.net.len(), srf.net[0].len());
            let mut extents = Vec::new();
            for (c, row) in [(0, 0), (0, nu - 1), (1, 0), (1, nv - 1)] {
                let pts: Vec<[f64; 4]> = if c == 0 {
                    srf.net[row].clone()
                } else {
                    srf.net.iter().map(|r| r[row]).collect()
                };
                if pts.iter().all(|x| bits(x) == bits(&pts[0])) {
                    let (lo, hi) = (dom[1 - c], dom[3 - c]);
                    // `hi − lo` and its rounding error (Knuth's TwoSum, exact): `0` makes
                    // the double the exact extent
                    let d = hi - lo;
                    let bb = d - hi;
                    let err = (hi - (d - bb)) + (-lo - bb);
                    assert!(
                        err == 0.0,
                        "{name}: the pole side's extent {hi} − {lo} is not a double"
                    );
                    extents.push(d);
                }
            }
            assert!(
                !extents.is_empty() && extents.iter().all(|&d| d == 1.0),
                "{name}: the pole sides' parameter extents {extents:?} are not exactly 1"
            );
        }
        // the mean and maximum curve/surface gaps (the drop distances of the edges'
        // curves to the face): every edge curve of the face is the face's boundary curve
        // along a side of its domain, bit for bit (`is_boundary`, checked on the replayed
        // model), so every point of it is on the face: the reference is `0`
        if v[2] != main[2] || v[3] != main[3] {
            let (b, f) = face_scene(case).unwrap().expect("the case builds its face");
            let srf = Srf::from_input(inputs);
            assert!(
                srf.is(bspline_surface(
                    b.face(f).unwrap().surface.as_ref().expect("a surface")
                )),
                "{name}: the input net is not the face's surface"
            );
            let mut n = 0;
            for (eu, _) in b.edgeuses().iter() {
                if eu1::face(&b, eu).unwrap() != Some(f) {
                    continue;
                }
                let e = eu1::edge(&b, eu).unwrap().expect("an edge");
                let c = b.edge(e).unwrap().curve.as_ref().expect("an edge curve");
                let CurveKind::BSpline(c) = c.kind() else {
                    panic!("{name}: the edge curve of {eu:?} is not a B-spline");
                };
                let c = crv_of(c);
                let on = [(0, dom[0]), (0, dom[2]), (1, dom[1]), (1, dom[3])]
                    .into_iter()
                    .any(|(c0, f0)| {
                        let iso = Iso {
                            c: c0,
                            f: f0,
                            ivl: (0.0, 0.0),
                        };
                        is_boundary(&srf, &iso, &c)
                    });
                assert!(
                    on,
                    "{name}: the edge curve of {eu:?} is not a boundary curve"
                );
                n += 1;
            }
            assert!(n > 0, "{name}: no edge curves");
            for (i, what) in [(2, "mean curve/surface gap"), (3, "max curve/surface gap")] {
                out.insert(
                    i,
                    Cert::Exact {
                        r: 0.0,
                        b: None,
                        what,
                    },
                );
            }
        }
        return out;
    }
    if unit == "topology.edgeuse_p3" && inputs["op"] == "binormal" && status == 0 && v[16] == 1.0 {
        // `put_eu_state` after the 16 binormal values: `1`, the trim record, the owner, the
        // three drop-fail values, then whether the vertex use's vertex/face gap is cached:
        // proven `0` on the model (`prove_corner_flag`)
        if let Some(tr) = parse_trim(v, 17) {
            if v[tr.end + 4] != main[tr.end + 4] {
                prove_corner_flag(&Model::of(case), dom);
                out.insert(
                    tr.end + 4,
                    Cert::Exact {
                        r: 0.0,
                        b: None,
                        what: "vertex/face gap cached (proven: corner of two iso sides)",
                    },
                );
            }
        }
    }
    if unit == "topology.edgeuse_p2" && inputs["op"] == "step_off" {
        // `[code, point, binormal, tangent, normal, uv]` of `EvaluateBinormalStepOff` at
        // `frac` of the edge: the edge is a domain side's iso curve on the replayed model
        // (bit for bit), the side main's (main's fixed uv strictly nearer it), and `t` the
        // edge interval's `evaluate(frac)`, the op's own parameter
        let (b, eu) = scene_p2(case)
            .unwrap()
            .expect("the case builds its edge use");
        let m = Model { brep: b, eu };
        let k = Kernel::default();
        let (c, f) = m
            .iso_side(&k, eu, dom)
            .expect("the edge is a domain side's iso curve");
        let other = if f == dom[c] { dom[c + 2] } else { dom[c] };
        assert!(
            (main[13 + c] - f).abs() < (main[13 + c] - other).abs(),
            "{name}: main's uv is not nearer the side"
        );
        let srf = Srf::from_input(inputs);
        assert!(
            srf.is(m.surface()),
            "{name}: the input net is not the face's surface"
        );
        let e = eu1::edge(&m.brep, eu).unwrap().expect("an edge");
        let ivl = m.brep.edge(e).unwrap().interval;
        let t = ivl.evaluate(num(&inputs["frac"]));
        let iso = Iso {
            c,
            f,
            ivl: (ivl.min(), ivl.max()),
        };
        // #17896: the edge's own iso curve moves at rounding (exact shared coordinates in
        // its knot insertions), and its tangent with it
        let tangent = bits(&v[7..10]) != bits(&main[7..10]);
        if tangent || bits(&v[1..4]) != bits(&main[1..4]) {
            let opposite = eu1::orientation(&m.brep, eu).unwrap() == OrientType::Opposite;
            label_iso_tangent(
                &m.bspline_curve(),
                &srf,
                &iso,
                t,
                opposite,
                tangent,
                1,
                &mut out,
            );
        }
        label_binormal_at(v, main, 1, false, t, &srf, &iso, &mut out);
        return out;
    }
    if unit == "topology.edgeuse_p2" && inputs["op"] == "check_set" && v[0] != main[0] {
        // `check_and_set_uv_trim_curve`'s `max_dist_to_surf`: `curve_max_distance_between`
        // the edge's iso curve and the `CrvOnSurf` over the given UV curve, a sampled
        // search. The exact maximum distance is 1-Lipschitz in the edge curve, which moved
        // at most `iso_curve_move`; the search's own sensitivity is MEASURED (plus `8 u` of
        // the value for its last rounding).
        let srf = Srf::from_input(inputs);
        out.insert(
            0,
            Cert::Measured {
                bound: iso_curve_move(&srf) + 8.0 * U * v[0].abs().max(main[0].abs()),
                what: "check_set max edge / UV curve distance (a sampled search)",
            },
        );
    }
    let (Some(iso), 0) = (iso, status) else {
        return out;
    };
    if unit != "topology.edgeuse_p3" {
        return out;
    }
    if name == BLADE {
        let (s, _) = d.blade.as_ref().unwrap();
        let curve = d.drops[name].curve.as_ref().unwrap();
        let m = Model::of(case);
        // the data lines are the replayed model's: the edge's curve and the face's surface
        assert!(
            same_crv(&m.bspline_curve(), curve),
            "{name}: DC is not the edge curve"
        );
        assert!(s.is(m.surface()), "{name}: S is not the face surface");
        label_blade(v, s, &iso, curve, &mut out);
        return out;
    }
    let srf = Srf::from_input(inputs);
    let cos = name.starts_with("cos_open");
    match inputs["op"].as_str().unwrap() {
        "binormal" => {
            // a `CrvOnSurf` edge's tangent is certified first: the binormal chain takes it
            if cos {
                label_cos(v, inputs, &srf, &iso, &mut out);
            } else if bits(&v[6..9]) != bits(&main[6..9]) || bits(&v[0..3]) != bits(&main[0..3]) {
                // #17896: the edge's own iso curve moves at rounding, and its tangent
                let m = Model::of(case);
                let k = Kernel::default();
                assert_eq!(
                    m.iso_side(&k, m.eu, dom),
                    Some((iso.c, iso.f)),
                    "{name}: the edge is not its side's iso curve"
                );
                let n = v.len();
                let t = ev(v[n - 3], v[n - 2], num(&inputs["frac"])).clamp(v[n - 3], v[n - 2]);
                let opposite = eu1::orientation(&m.brep, m.eu).unwrap() == OrientType::Opposite;
                let tangent = bits(&v[6..9]) != bits(&main[6..9]);
                label_iso_tangent(
                    &m.bspline_curve(),
                    &srf,
                    &iso,
                    t,
                    opposite,
                    tangent,
                    0,
                    &mut out,
                );
            }
            label_binormal(v, main, &srf, &iso, num(&inputs["frac"]), &mut out);
        }
        "normalized" => {
            if inputs["parameter_space"] == true {
                label_norm_uv(main, inputs, &iso, &mut out);
            } else if cos {
                label_cos(v, inputs, &srf, &iso, &mut out);
            }
        }
        _ => {
            if name.contains("_goc_") {
                let m = Model::of(case);
                let uv = m
                    .cos_on_face()
                    .expect("a CrvOnSurf on the face's own surface");
                assert!(
                    same_crv(&uv, &iso_curve(&iso)),
                    "{name}: the edge's UV curve is not the iso line"
                );
                label_goc(&mut out);
            }
            if !d.drops.contains_key(name) {
                // no drop stations were dumped (the drop runs outside the iso station
                // loop): the drop quality, a maximum over the drop's own samples of the
                // distance from the edge curve, is 1-Lipschitz in that curve, which moved at
                // most `iso_curve_move`; the samples' sensitivity is MEASURED
                for i in 0..2 {
                    // only where no certificate (the `goc` labels') holds the key already
                    if v[i] != main[i] && (i == 0 || name.contains("getcreate")) {
                        out.entry(i).or_insert_with(|| Cert::Measured {
                            bound: iso_curve_move(&srf) + 8.0 * U * v[i].abs().max(main[i].abs()),
                            what: "drop quality, no station dump (sampled)",
                        });
                    }
                }
            }
            if let Some(dr) = d.drops.get(name) {
                let m = Model::of(case);
                assert!(
                    srf.is(m.surface()),
                    "{name}: the input net is not the face surface"
                );
                match (&dr.curve, &dr.other_uv) {
                    (Some(c), _) => assert!(
                        same_crv(&m.bspline_curve(), c),
                        "{name}: DC is not the edge curve"
                    ),
                    (None, Some(u)) => assert!(
                        same_crv(&m.cos_on_face().expect("a CrvOnSurf on the face"), u),
                        "{name}: DU is not the edge's UV curve"
                    ),
                    _ => panic!("{name}: drop data without a curve"),
                }
                label_drop(name, &srf, &iso, dr, &mut out);
            }
        }
    }
    out
}

// ---- the rows of the other units that move with the drop ends -------------------------------

/// The iso side `(fixed coordinate, value)` of every edge use of the model's face, by edge
/// use: each must be a domain side's `create_iso_parametric_curve`, bit for bit.
fn face_sides(m: &Model, dom: [f64; 4]) -> Vec<(EdgeuseId, usize, f64)> {
    let k = Kernel::default();
    let f = eu1::face(&m.brep, m.eu).unwrap().expect("a face");
    let mut sides = Vec::new();
    for (eu, _) in m.brep.edgeuses().iter() {
        if eu1::face(&m.brep, eu).unwrap() != Some(f) {
            continue;
        }
        let (c, side) = m
            .iso_side(&k, eu, dom)
            .unwrap_or_else(|| panic!("the edge of {eu:?} is not a domain side's iso curve"));
        sides.push((eu, c, side));
    }
    assert!(!sides.is_empty(), "no edge uses on the face");
    sides
}

/// Every vertex of the face is a domain corner in the ideal pipeline: two of its edges are
/// iso curves of sides of different coordinates (`face_sides`), whose uv lines meet only at
/// the corner they span.
fn prove_corner_vertices(m: &Model, dom: [f64; 4]) {
    let sides = face_sides(m, dom);
    for (v, _) in m.brep.vertices().iter() {
        let on: Vec<(usize, f64)> = sides
            .iter()
            .filter(|(eu, _, _)| {
                let e = eu1::edge(&m.brep, *eu).unwrap().expect("an edge");
                m.brep.edge(e).unwrap().vertices.contains(&v)
            })
            .map(|&(_, c, f)| (c, f))
            .collect();
        assert!(
            on.iter().any(|a| on.iter().any(|b| a.0 != b.0)),
            "vertex {v:?} is not on sides of both coordinates: {on:?}"
        );
    }
}

/// The candidate of `cands` whose prediction `dist` of main's values is nearest: strictly
/// nearer than `1e-9`, every other candidate farther than `1e-6`.
fn pick<T: Copy + std::fmt::Debug>(cands: &[T], dist: impl Fn(T) -> f64, what: &str) -> T {
    let mut d: Vec<(f64, T)> = cands.iter().map(|&c| (dist(c), c)).collect();
    d.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(
        d[0].0 < 1e-9 && d.get(1).is_none_or(|x| x.0 > 1e-6),
        "{what}: main's values do not single out a candidate: {d:?}"
    );
    d[0].1
}

/// `topology.vertexuse` (`state`): the face's edges are iso curves of its domain's sides
/// (on the replayed model), so each vertex's ideal uv is the corner main's point is nearest,
/// the trims' ideal unit tangents at it are axis vectors (the cross component `0`, the
/// moving one `±1`, the sign and axis main's), and the sector angle between two of them
/// the exact multiple of 90° their quadrants give.
fn label_vertexuse(
    case: &goldens::Case,
    main: &[f64],
    dom: [f64; 4],
    out: &mut BTreeMap<usize, Cert>,
) {
    let inputs = &case.inputs;
    let b = vu_scene(case).unwrap().expect("the case builds its B-rep");
    let eu = b.edgeuses().iter().next().expect("an edge use").0;
    let m = Model { brep: b, eu };
    let srf = Srf::from_input(inputs);
    assert!(
        srf.is(m.surface()),
        "the input net is not the face's surface"
    );
    prove_corner_vertices(&m, dom);
    let corners = [
        [dom[0], dom[1]],
        [dom[2], dom[1]],
        [dom[0], dom[3]],
        [dom[2], dom[3]],
    ];
    let corner = |i: usize, out: &mut BTreeMap<usize, Cert>| {
        let c = pick(
            &corners,
            |c| (main[i] - c[0]).abs().max((main[i + 1] - c[1]).abs()),
            "vertex corner",
        );
        for (q, &r) in c.iter().enumerate() {
            out.insert(
                i + q,
                Cert::Exact {
                    r,
                    b: None,
                    what: "vertex uv (domain corner)",
                },
            );
        }
    };
    match inputs["op"].as_str().unwrap() {
        "uv_point" => corner(1, out),
        "uv_sector" => {
            corner(6, out);
            corner(8, out);
            let axes = [[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0]];
            let mut quad = [0usize; 2];
            for (k, i) in [10usize, 12].into_iter().enumerate() {
                let q = pick(
                    &[0usize, 1, 2, 3],
                    |q| {
                        (main[i] - axes[q][0])
                            .abs()
                            .max((main[i + 1] - axes[q][1]).abs())
                    },
                    "uv tangent axis",
                );
                quad[k] = q;
                for (c, &r) in axes[q].iter().enumerate() {
                    out.insert(
                        i + c,
                        Cert::Exact {
                            r,
                            b: None,
                            what: "vertex uv tangent (axis)",
                        },
                    );
                }
            }
            // the counter-clockwise angle from the start tangent to the end one
            let r = 90.0 * ((quad[1] + 4 - quad[0]) % 4) as f64;
            assert!(
                (main[14] - r).abs() < 1e-9,
                "main's sector angle {}",
                main[14]
            );
            out.insert(
                14,
                Cert::Exact {
                    r,
                    b: None,
                    what: "sector angle (axis tangents)",
                },
            );
        }
        _ => {}
    }
}

/// A degree-1 trim from `a` to `b` over `[lo, hi]`.
fn line(a: [f64; 2], b: [f64; 2], lo: f64, hi: f64) -> Crv {
    Crv {
        p: 1,
        knots: vec![lo, lo, hi, hi],
        cps: vec![[a[0], a[1], 0.0, 1.0], [b[0], b[1], 0.0, 1.0]],
    }
}

/// The exact point of `line` at `s`, enclosed.
fn line_at(l: &Crv, s: f64) -> [Iv; 2] {
    let (t0, t1) = (l.knots[0], l.knots[3]);
    let a = Iv::p(s).sub(Iv::p(t0)).div(Iv::p(t1).sub(Iv::p(t0)));
    std::array::from_fn(|q| {
        Iv::p(l.cps[0][q]).add(a.mul(Iv::p(l.cps[1][q]).sub(Iv::p(l.cps[0][q]))))
    })
}

/// Checks that `curve` is laid out as the input surface's iso curve along the side of
/// fixed coordinate `c` over `[lo, hi]` (`create_iso_parametric_curve`, not reversed): a
/// non-rational B-spline of the surface's degree along the side, its knots the surface's
/// along the side clamped to `[lo, hi]`, bit for bit. Returns the degrees `(d, d_x)` along
/// and across the side.
fn iso_bspline_degrees(srf: &Srf, c: usize, curve: &Crv, (lo, hi): (f64, f64)) -> (usize, usize) {
    let (d, kn, dx) = if c == 0 {
        (srf.q, &srf.v, srf.p)
    } else {
        (srf.p, &srf.u, srf.q)
    };
    assert!(
        (srf.rational || !curve.rational()) && curve.p == d,
        "the edge curve is not a B-spline of the surface's degree, rational as it is"
    );
    let mut want = vec![lo; d + 1];
    want.extend(kn.iter().copied().filter(|&x| lo < x && x < hi));
    want.extend(vec![hi; d + 1]);
    assert_eq!(
        bits(&curve.knots),
        bits(&want),
        "the edge curve's knots are not the surface's clamped to the domain"
    );
    (d, dx)
}

/// The tangent `[base + 6, base + 9)` of a binormal record whose edge curve is the input
/// surface's iso curve along `iso` (`create_iso_parametric_curve`, checked by the caller
/// with [`Model::iso_side`], and laid out as [`iso_bspline_degrees`] checks), evaluated at
/// `t`. In exact arithmetic that curve is `S(f, ·)` along the side, so the reference is the
/// exact `S_v(f, t)` (`S_u` for a side of fixed `v`). `E` is it widened by the a-priori
/// bound of the kernel's float derivative: the control points' error `γ_{14 (d_x + 2 d)} M`
/// ([`label_iso_bspline`]) and the derivative basis's rounding `γ_{7 d + 6} M`, each
/// through `Σ|N'_j| <= 2 d / h`, `h` the curve's shortest non-empty knot span. The record's
/// tangent is the edge use's: negated (exactly) for an `Opposite` edge use
/// (`evaluate_binormal`), so are the reference and `E` (`opposite`).
///
/// A rational net goes to [`label_iso_tangent_rational`].
///
/// The point `[base, base + 3)` is labelled too: the reference the exact `S(f, t)`, `E` it
/// widened by [`label_iso_bspline`]'s `γ_{14 (d_x + 2 d) + 7 d + 3} M` (the point row of
/// the same `crv_derivs`). The tangent only when `tangent` (it moved): the binormal chain
/// takes an unmoved tangent as it is.
#[allow(clippy::too_many_arguments)] // test
fn label_iso_tangent(
    curve: &Crv,
    srf: &Srf,
    iso: &Iso,
    t: f64,
    opposite: bool,
    tangent: bool,
    base: usize,
    out: &mut BTreeMap<usize, Cert>,
) {
    if srf.rational {
        label_iso_tangent_rational(curve, srf, iso, t, opposite, tangent, base, out);
        return;
    }
    let bounds = srf.bounds();
    let (d, dx) = iso_bspline_degrees(srf, iso.c, curve, iso.ivl);
    let h = curve
        .knots
        .windows(2)
        .map(|w| Iv::p(w[1]).sub(Iv::p(w[0])).lo)
        .filter(|&x| x > 0.0)
        .fold(f64::INFINITY, f64::min);
    assert!(h.is_finite() && h > 0.0, "the iso curve has no knot span");
    let lip = Iv::p(2.0 * d as f64).div(Iv::p(h)).hi;
    let g = gam(14 * (dx + 2 * d) + 7 * d + 6);
    let mut uv = [Iv::p(t), Iv::p(t)];
    uv[iso.c] = Iv::p(iso.f);
    let (p, su, sv) = srf.exact(uv[0], uv[1]);
    let gp = gam(14 * (dx + 2 * d) + 7 * d + 3);
    for (q, (&pq, &mq)) in p.iter().zip(&bounds.m).enumerate() {
        out.insert(
            base + q,
            Cert::Enc {
                r: pq,
                e: Some(pq.widen(up_mul(gp, mq))),
                what: "iso B-spline point",
            },
        );
    }
    if !tangent {
        return;
    }
    let r = if iso.c == 0 { sv } else { su };
    let r = r.map(|x| if opposite { x.neg() } else { x });
    for (q, (&rq, &mq)) in r.iter().zip(&bounds.m).enumerate() {
        out.insert(
            base + 6 + q,
            Cert::Enc {
                r: rq,
                e: Some(rq.widen(up_mul(lip, up_mul(g, mq)))),
                what: "iso B-spline tangent",
            },
        );
    }
}

/// [`label_iso_tangent`] for a rational net. The kernel's homogeneous control points `Q̃_j`
/// of the edge curve are within `γ_{14 (d_x + 2 d)} M_c` per coordinate `c` (weight
/// included) of the exact iso curve's `Q_j` (the a-priori bound of [`label_iso_bspline`]:
/// the same `affine2` levels on all four coordinates, `M_c` the net's largest homogeneous
/// coordinate). `E` encloses, over those boxes around the kernel's `Q̃_j`, the kernel's
/// float rational tangent operation for operation (`crv_derivs` through
/// `rational_basis_derivs_into`): the basis and its derivative widened as [`Srf::kernel`]
/// counts them (`γ_{6 d}`), `d0 = Σ w_j N_j` and `d1 = Σ w_j N'_j` (one rounding per
/// product and sum), `R_j = w_j N_j / d0`, `R'_j = (w_j N'_j − d1 R_j) / d0`, the
/// Euclidean points `Q_j (1 / w_j)` (`to_euclid`) and the running sums `Σ R'_j P_j`. Each
/// rounding widening also holds the exact operation, so `E` holds the exact tangent of
/// the exact curve too: the reference, the exact `S_v(f, t)` (`S_u` for a side of fixed
/// `v`). The point likewise: `Σ R_j P_j` over the same boxes, the reference `S(f, t)`.
#[allow(clippy::too_many_arguments)] // test
fn label_iso_tangent_rational(
    curve: &Crv,
    srf: &Srf,
    iso: &Iso,
    t: f64,
    opposite: bool,
    tangent: bool,
    base: usize,
    out: &mut BTreeMap<usize, Cert>,
) {
    let (d, dx) = iso_bspline_degrees(srf, iso.c, curve, iso.ivl);
    let g = gam(14 * (dx + 2 * d));
    let m: [f64; 4] = std::array::from_fn(|c| {
        srf.net
            .iter()
            .flatten()
            .map(|cp| cp[c].abs())
            .fold(0.0, f64::max)
    });
    let q: Vec<[Iv; 4]> = curve
        .cps
        .iter()
        .map(|cp| std::array::from_fn(|c| Iv::p(cp[c]).widen(up_mul(g, m[c]))))
        .collect();
    let r = |x: Iv| rnd(x, U, true);
    let (n0, n1, n1a) = basis(&curve.knots, d, Iv::p(t));
    let gb = gam(6 * d);
    let n0: Vec<Iv> = n0.iter().map(|x| x.rel(gb)).collect();
    let n1: Vec<Iv> = n1
        .iter()
        .zip(&n1a)
        .map(|(x, a)| x.widen(up_mul(gb, a.hi)))
        .collect();
    let zero = Iv::p(0.0);
    let (mut d0, mut d1) = (zero, zero);
    for ((qj, a), b) in q.iter().zip(&n0).zip(&n1) {
        d0 = r(d0.add(r(qj[3].mul(*a))));
        d1 = r(d1.add(r(qj[3].mul(*b))));
    }
    let mut e = [zero; 3];
    let mut e0 = [zero; 3];
    for ((qj, a), b) in q.iter().zip(&n0).zip(&n1) {
        let rj = r(r(qj[3].mul(*a)).div(d0));
        let rdj = r(r(r(qj[3].mul(*b)).sub(r(d1.mul(rj)))).div(d0));
        let rw = r(Iv::p(1.0).div(qj[3]));
        for c in 0..3 {
            let pc = r(qj[c].mul(rw));
            e[c] = r(e[c].add(r(rdj.mul(pc))));
            e0[c] = r(e0[c].add(r(rj.mul(pc))));
        }
    }
    let mut uv = [Iv::p(t), Iv::p(t)];
    uv[iso.c] = Iv::p(iso.f);
    let (p, su, sv) = srf.exact(uv[0], uv[1]);
    for c in 0..3 {
        out.insert(
            base + c,
            Cert::Enc {
                r: p[c],
                e: Some(e0[c]),
                what: "rational iso B-spline point",
            },
        );
    }
    if !tangent {
        return;
    }
    let rf = if iso.c == 0 { sv } else { su };
    let rf = rf.map(|x| if opposite { x.neg() } else { x });
    let e = e.map(|x| if opposite { x.neg() } else { x });
    for c in 0..3 {
        out.insert(
            base + 6 + c,
            Cert::Enc {
                r: rf[c],
                e: Some(e[c]),
                what: "rational iso B-spline tangent",
            },
        );
    }
}

/// The samples of a 3D edge curve that is the input surface's iso curve along the domain
/// side `cand` over `[lo, hi]` (`create_iso_parametric_curve`: `extract_iso_crv` at the
/// side, `crv_trim` to the domain, `end_at_insertion_point`). Checked on the replayed
/// model: a non-rational B-spline of the surface's degree along the side, its knots the
/// surface's along the side clamped to `[lo, hi]` (not reversed), bit for bit. In exact
/// arithmetic each of those steps keeps the curve's points, so the ideal curve is
/// `S(f, s)` and the reference is the exact surface point there (`ideal`).
///
/// `E` is that reference widened by the a-priori bound `γ_K M` of the kernel's float chain,
/// `M` the net's largest coordinate (every control point of the chain is a convex
/// combination of net points). Each knot-insertion level is `CPoint::affine2`'s
/// `α a + (1 − α) b` with `α = (t − k_i) / (k_j − k_i)` in `[0, 1]`: `α` within `γ_3`,
/// `1 − α` one more rounding, two products and a sum, at most `γ_10 M` new error per level,
/// and the inherited error grows by at most `1 + γ_4` (`affine2` only replaces a shared
/// coordinate by its exact value). A control point of the trimmed curve passes at most
/// `d_x + 2 d` levels (`d_x` the degree across the side, extraction; `d` along it, the
/// trim's left then right insertion; the end insertion `d_x + d`), so `γ_{14 (d_x + 2 d)}`.
/// The evaluation (`crv_eval`, or a clamped end's control point as is) adds the basis
/// (`6 d`), the sum of `d + 1` terms and `2` more: `K = 14 (d_x + 2 d) + 7 d + 3`.
#[allow(clippy::too_many_arguments)]
fn label_iso_bspline(
    srf: &Srf,
    bounds: &Bounds,
    curve: &Crv,
    (c, _, rev): (usize, f64, bool),
    (lo, hi): (f64, f64),
    at: usize,
    out: &mut BTreeMap<usize, Cert>,
    ideal: impl Fn(f64) -> V3,
) {
    assert!(!rev, "a reversed iso B-spline edge curve");
    let (d, dx) = iso_bspline_degrees(srf, c, curve, (lo, hi));
    let g = gam(14 * (dx + 2 * d) + 7 * d + 3);
    for i in 0..5u8 {
        let s = ev(lo, hi, f64::from(i) / 4.0);
        let r = ideal(s);
        for (q, (&rq, &mq)) in r.iter().zip(&bounds.m).enumerate() {
            out.insert(
                at + 2 + 3 * usize::from(i) + q,
                Cert::Enc {
                    r: rq,
                    e: Some(rq.widen(up_mul(g, mq))),
                    what: "iso B-spline sample",
                },
            );
        }
    }
}

/// `topology.brep_validation` (`curve_samples`): `[lo, hi]` and 5 samples `[x, y, z]` at
/// `evaluate(i/4)` per present curve, the edges' curves and then the edge uses' UV trims
/// (in arena order, on the replayed model). Each edge curve here is a `CrvOnSurf` on the
/// input surface over a copy of its edge use's trim. The ideal trim is a domain side's
/// exact iso line over the curve's domain `[lo, hi]` (main's, bit for bit, and the side's
/// moving extent): the side and direction main's samples single out (`pick`). A trim
/// sample's references are the line's exact point, at rounding within the line evaluation
/// bound; a `CrvOnSurf` sample's the exact surface point there, at rounding within the
/// enclosure of the kernel's evaluation through the float line point (`cos_point`).
fn label_validation(
    case: &goldens::Case,
    v: &[f64],
    main: &[f64],
    dom: [f64; 4],
    out: &mut BTreeMap<usize, Cert>,
) {
    let srf = Srf::from_input(&case.inputs);
    let bounds = srf.bounds();
    let b = val_scene(case).unwrap().expect("the case builds its B-rep");
    let curves: Vec<(bool, &Curve)> = b
        .edges()
        .iter()
        .filter_map(|(_, e)| e.curve.as_ref().map(|c| (true, c)))
        .chain(
            b.edgeuses()
                .iter()
                .filter_map(|(_, eu)| eu.uv_trim_curve.as_ref().map(|c| (false, c))),
        )
        .collect();
    assert_eq!(17 * curves.len(), v.len(), "every curve has 5 samples");
    // the ideal lines: (fixed coordinate, side, reversed)
    let mut cands = Vec::new();
    for c in 0..2 {
        for side in [dom[c], dom[c + 2]] {
            for rev in [false, true] {
                cands.push((c, side, rev));
            }
        }
    }
    for (k, (edge, curve)) in curves.iter().enumerate() {
        let at = 17 * k;
        if (at..at + 17).all(|i| v[i].to_bits() == main[i].to_bits()) {
            continue;
        }
        let (lo, hi) = (main[at], main[at + 1]);
        assert_eq!(bits(&v[at..at + 2]), bits(&[lo, hi]), "the curve's domain");
        if *edge {
            match curve.kind() {
                CurveKind::CrvOnSurf(cs) => assert!(
                    srf.is(bspline_surface(cs.surface())),
                    "the CrvOnSurf's surface is not the input surface"
                ),
                CurveKind::BSpline(_) => {}
                _ => panic!("a moved edge curve that is neither a CrvOnSurf nor a B-spline"),
            }
        }
        let line_of = |(c, side, rev): (usize, f64, bool)| {
            let mut a = [side; 2];
            let mut z = [side; 2];
            a[1 - c] = if rev { hi } else { lo };
            z[1 - c] = if rev { lo } else { hi };
            line(a, z, lo, hi)
        };
        let ideal = |cand, s: f64| -> V3 {
            let uv = line_at(&line_of(cand), s);
            if *edge {
                srf.exact(uv[0], uv[1]).0
            } else {
                [uv[0], uv[1], Iv::p(0.0)]
            }
        };
        let samples: Vec<f64> = (0..5).map(|i| ev(lo, hi, f64::from(i) / 4.0)).collect();
        let fits = cands
            .iter()
            .filter(|&&(c, _, _)| [dom[1 - c], dom[3 - c]] == [lo, hi])
            .copied()
            .collect::<Vec<_>>();
        let cand = pick(
            &fits,
            |cand| {
                samples
                    .iter()
                    .enumerate()
                    .flat_map(|(i, &s)| {
                        let p = ideal(cand, s);
                        (0..3).map(move |q| (main[at + 2 + 3 * i + q] - p[q].mid()).abs())
                    })
                    .fold(0.0, f64::max)
            },
            "the curve's ideal line",
        );
        if let (true, CurveKind::BSpline(cb)) = (*edge, curve.kind()) {
            label_iso_bspline(&srf, &bounds, &crv_of(cb), cand, (lo, hi), at, out, |s| {
                ideal(cand, s)
            });
            continue;
        }
        let l = line_of(cand);
        // the bounds below are of the kernel evaluating this degree-1 line: the curve's UV
        // curve on the replayed model is it, bit for bit
        let uvc = match curve.kind() {
            CurveKind::CrvOnSurf(cs) => cs.uv_curve(),
            _ => curve,
        };
        let CurveKind::BSpline(uvb) = uvc.kind() else {
            panic!("a moved curve's UV curve that is not a B-spline");
        };
        assert!(
            same_crv(&crv_of(uvb), &l),
            "the moved curve's UV curve is not its ideal line, bit for bit"
        );
        let (c, side, rev) = cand;
        let lb = line_bound(&Iso {
            c,
            f: side,
            ivl: (lo, hi),
        });
        for (i, &s) in samples.iter().enumerate() {
            let j = at + 2 + 3 * i;
            if *edge {
                let r = ideal(cand, s);
                let e = cos_point(&srf, &bounds, &l, s);
                for q in 0..3 {
                    out.insert(
                        j + q,
                        Cert::Enc {
                            r: r[q],
                            e: Some(e[q]),
                            what: "CrvOnSurf sample",
                        },
                    );
                }
                continue;
            }
            out.insert(
                j + c,
                Cert::Exact {
                    r: side,
                    b: Some(lb),
                    what: "uv trim sample fixed",
                },
            );
            out.insert(
                j + 2,
                Cert::Exact {
                    r: 0.0,
                    b: None,
                    what: "uv trim sample z",
                },
            );
            if rev {
                let r = line_at(&l, s)[1 - c];
                out.insert(
                    j + 1 - c,
                    Cert::Enc {
                        r,
                        e: Some(r.widen(lb)),
                        what: "uv trim sample param",
                    },
                );
            } else {
                out.insert(
                    j + 1 - c,
                    Cert::Exact {
                        r: s,
                        b: Some(lb),
                        what: "uv trim sample param",
                    },
                );
            }
        }
    }
}

/// `query.face_p2` point classifications at a point of an edge use's trim (`at.mode`
/// `trim`; `pc` = `[class, kind, index, gap, tol, pre_snap, t, u, v, set]`). The edge use's
/// edge curve is a domain side's iso curve on the replayed model, so its ideal trim is the
/// exact line `(f, t)` over the edge's interval, `t = t0 + a (t1 − t0)` as the harness
/// computes it over the trim's interval (the new trim's interval is the edge's, bit for
/// bit, so the replay's `t` is the ideal one).
///
/// * Face class (`102`): the query uv is `(f, t) + (du, dv)` exactly; the kernel's is the
///   float line point (within the line bound) plus the offset, rounded once.
/// * Vertex class (`104`, `d = 0`): the gap is `|V − S(f, t)|` to the class's vertex `V`
///   (main's and the new vertex the same); the kernel's is the float distance from `V` to
///   the float surface point at the float line point (`cos_point`), within `γ_6`.
fn label_query(
    case: &goldens::Case,
    v: &[f64],
    main: &[f64],
    dom: [f64; 4],
    out: &mut BTreeMap<usize, Cert>,
) {
    let inputs = &case.inputs;
    let (b, _, eu) = query_scene(case)
        .unwrap()
        .expect("the case builds its scene");
    let m = Model { brep: b, eu };
    let srf = Srf::from_input(inputs);
    assert!(
        srf.is(m.surface()),
        "the input net is not the face's surface"
    );
    let k = Kernel::default();
    let (c, f) = m
        .iso_side(&k, eu, dom)
        .expect("the edge is a domain side's iso curve");
    let e = eu1::edge(&m.brep, eu).unwrap().expect("an edge");
    let ivl = m.brep.edge(e).unwrap().interval;
    let (t0, t1) = (ivl.min(), ivl.max());
    let (trim, _) = eu1::uv_trim_curve(&m.brep, eu).unwrap();
    let trim = trim.expect("a uv trim");
    let ti = trim.natural_interval();
    assert_eq!(
        bits(&[ti.min(), ti.max()]),
        bits(&[t0, t1]),
        "the trim's interval"
    );
    // the ideal trim, the exact line `(f, t)`; the bounds below are of the kernel
    // evaluating it, so the replayed trim (and a `CrvOnSurf` edge curve's UV curve) is it,
    // bit for bit
    let mut a = [f; 2];
    let mut z = [f; 2];
    a[1 - c] = t0;
    z[1 - c] = t1;
    let l = line(a, z, t0, t1);
    let is_l =
        |cv: &Curve| matches!(cv.kind(), CurveKind::BSpline(bc) if same_crv(&crv_of(bc), &l));
    assert!(is_l(trim), "the replayed trim is not the exact line");
    if let Some(CurveKind::CrvOnSurf(cs)) = m.brep.edge(e).unwrap().curve.as_ref().map(Curve::kind)
    {
        assert!(
            is_l(cs.uv_curve()),
            "the CrvOnSurf's UV curve is not the exact line"
        );
    }
    let iso = Iso {
        c,
        f,
        ivl: (t0, t1),
    };
    let lb = line_bound(&iso);
    let at = &inputs["at"];
    assert_eq!(v[0], main[0], "the class");
    match v[0] {
        102.0 => {
            // the fixed coordinate `f`, the moving one the line's `t` itself
            let t = t0 + num(&at["a"]) * (t1 - t0);
            for (q, x, what) in [
                (c, f, "query uv fixed coordinate"),
                (1 - c, t, "query uv moving coordinate"),
            ] {
                let d = num(&at[if q == 0 { "du" } else { "dv" }]);
                out.insert(
                    7 + q,
                    Cert::Enc {
                        r: Iv::p(x).add(Iv::p(d)),
                        e: Some(Iv::p(x).widen(lb).add(Iv::p(d)).rel(U)),
                        what,
                    },
                );
            }
        }
        104.0 => {
            assert_eq!([num(&at["du"]), num(&at["dv"])], [0.0, 0.0], "no offset");
            assert_eq!(bits(&v[1..3]), bits(&main[1..3]), "the class's vertex");
            let (_, vx) = m
                .brep
                .vertices()
                .iter()
                .find(|(id, _)| f64::from(id.index()) == v[2])
                .expect("the class's vertex");
            let p = [Iv::p(vx.point.x), Iv::p(vx.point.y), Iv::p(vx.point.z)];
            let t = t0 + num(&at["a"]) * (t1 - t0);
            let uv = line_at(&l, t);
            let r = norm(&sub3(&p, &srf.exact(uv[0], uv[1]).0));
            let q = cos_point(&srf, &srf.bounds(), &l, t);
            let e = norm(&sub3(&p, &q)).rel(gam(6));
            out.insert(
                3,
                Cert::Enc {
                    r,
                    e: Some(e),
                    what: "vertex class gap",
                },
            );
        }
        _ => {}
    }
}

// ---- blend.fillet_solver_p2: the rail / side edge-use solve ---------------------------------

/// The inverse of a square interval matrix by Gauss–Jordan elimination (every pivot an
/// interval without `0`, chosen by the largest smallest magnitude in its column), an
/// enclosure of the inverse of every matrix in the box.
fn iv_inverse(a: &[Vec<Iv>]) -> Vec<Vec<Iv>> {
    let n = a.len();
    let mut m: Vec<Vec<Iv>> = a
        .iter()
        .enumerate()
        .map(|(i, row)| {
            assert_eq!(row.len(), n, "a square matrix");
            let mut r = row.clone();
            r.extend((0..n).map(|j| Iv::p(if i == j { 1.0 } else { 0.0 })));
            r
        })
        .collect();
    let mig = |x: Iv| {
        if x.lo > 0.0 {
            x.lo
        } else if x.hi < 0.0 {
            -x.hi
        } else {
            0.0
        }
    };
    for c in 0..n {
        let p = (c..n)
            .max_by(|&i, &j| mig(m[i][c]).total_cmp(&mig(m[j][c])))
            .expect("a row");
        assert!(mig(m[p][c]) > 0.0, "a singular Jacobian column {c}");
        m.swap(c, p);
        let piv = m[c][c];
        let row: Vec<Iv> = m[c].iter().map(|x| x.div(piv)).collect();
        for (i, r) in m.iter_mut().enumerate() {
            if i == c {
                continue;
            }
            let f = r[c];
            for (x, y) in r.iter_mut().zip(&row) {
                *x = x.sub(f.mul(*y));
            }
        }
        m[c] = row;
    }
    m.into_iter().map(|r| r[n..].to_vec()).collect()
}

/// The offset point `S(u, v) + d N(u, v)` of a degree-(1, 1) net over the uv box, `N` the unit
/// normal `S_u × S_v / |S_u × S_v|`: exact (`round` false) or the kernel's float enclosed
/// (`round`: [`Srf::kernel`], the cross product and normalisation chains, the product by `d`
/// and the sum, one rounding each).
fn offset_point(s: &Srf, d: f64, u: f64, v: f64, round: bool) -> V3 {
    let (p, su, sv) = if round {
        s.kernel(u, v)
    } else {
        s.exact(Iv::p(u), Iv::p(v))
    };
    let n = runit(&rcross(&su, &sv, round), round);
    std::array::from_fn(|c| rnd(p[c].add(rnd(Iv::p(d).mul(n[c]), U, round)), U, round))
}

/// `blend.fillet_solver_p2` `rail_eu`: the edge parameter `t`, the rail's and the other
/// surface's uv and the extra unknowns where the rail of two offset degree-(1, 1) planes
/// meets the side edge use's UV trim (`rail_edgeuse_intersect`: `LocalSolveNd` on
/// `RailUvCurveIntersectEnfo`). The fix moves only the guess (`uv_trim(guess)`, the exact
/// iso line now), so the solve lands on another float fixed point of the same equations.
///
/// The certificate, on the replayed solve ([`rail_eu_scene`]): both nets are planar
/// parallelograms of integer points (checked: degree 1, integer coordinates, no twist) and
/// the trim a non-rational degree-1 segment, so `F(x)` (`trim(t) − uv_rail`,
/// `P_rail(uv_rail) − P_other(uv_other)`, `x_k − (target + k)`) is affine with the constant
/// Jacobian `J` and the root `x* = x − J⁻¹ F(x)` for any `x`; `x*` is enclosed (`R`) at the
/// new point from the exact `F(new)`. The kernel's float `F` at `new` is enclosed (`Ê`, the
/// value chains of [`Srf::kernel`] and [`Crv::kernel`] and one rounding per difference) and
/// the kernel's own residual there (the solver's accepted point, read from the replay) must
/// lie in `Ê` and within `b = sup|Ê − F(new)|` of `0`: the solve stopped at a residual of
/// the evaluation's rounding, not at its tolerance. Then `|F(new)| <= |F̂(new)| + b` and
/// `|new − x*| <= |J⁻¹| (|F̂(new)| + b)` per unknown, all rounding: `E` is `R` widened by it.
/// Every unknown also lies strictly inside the solve's box (no clamp or boundary hit).
fn label_rail_eu(case: &goldens::Case, v: &[f64], out: &mut BTreeMap<usize, Cert>) {
    let inputs = &case.inputs;
    assert_eq!(inputs["op"], "rail_eu", "{}: a rail_eu case", case.name);
    let extra = inputs["cfg"]["extra"].as_u64().expect("extra") as usize;
    let target = num(&inputs["cfg"]["target"]);
    let rail = inputs["rail"].as_u64().expect("rail") as usize;
    assert!(rail < 2);
    let surfs = &inputs["surfs"];
    assert!(
        surfs["has"]
            .as_array()
            .expect("has")
            .iter()
            .all(|h| h == true),
        "both offset surfaces"
    );
    let srf_of = |i: usize| -> (Srf, f64) {
        let s = Srf::from_bspline(&surfs["base"][i]["bspline"]);
        assert!(
            s.p == 1 && s.q == 1 && !s.rational && s.net.len() == 2 && s.net[0].len() == 2,
            "a bilinear net"
        );
        for c in 0..3 {
            let x = |i: usize, j: usize| s.net[i][j][c];
            assert!(
                s.net
                    .iter()
                    .flatten()
                    .all(|p| p[c].fract() == 0.0 && p[c].abs() < 1e6),
                "integer points"
            );
            assert_eq!(
                x(0, 0) + x(1, 1),
                x(0, 1) + x(1, 0),
                "a planar parallelogram"
            );
        }
        (s, num(&surfs["off"][i]))
    };
    let (rs, rd) = srf_of(rail);
    let (os, od) = srf_of(1 - rail);
    // the unknowns: t, the rail's uv, the other's uv, the extras; output `flat` holds `t` at
    // 5, the first surface's uv at 6, the second's at 8, the extras from 10, `t` again after
    let (ir, io) = if rail == 0 { (6, 8) } else { (8, 6) };
    let mut idx = vec![5, ir, ir + 1, io, io + 1];
    idx.extend((0..extra).map(|k| 10 + k));
    let n = idx.len();
    assert_eq!(v[4], 1.0, "{}: found", case.name);
    assert_eq!(
        v[10 + extra].to_bits(),
        v[5].to_bits(),
        "curve_parameter is t"
    );
    let x: Vec<f64> = idx.iter().map(|&i| v[i]).collect();
    let sc = rail_eu_scene(case, std::slice::from_ref(&x))
        .unwrap()
        .expect("the case builds its solve");
    let CurveKind::BSpline(tb) = sc.trim.kind() else {
        panic!("{}: a B-spline trim", case.name);
    };
    let trim = crv_of(tb);
    assert!(
        trim.p == 1 && trim.cps.len() == 2 && !trim.rational(),
        "a degree-1 segment trim"
    );
    // no clamp: every unknown strictly inside the solve's box
    let inside = |x: f64, lo: f64, hi: f64| lo < x && x < hi;
    assert!(
        inside(x[0], sc.interval.min(), sc.interval.max()),
        "t inside"
    );
    for (k, d) in sc.domains.iter().enumerate() {
        for c in 0..2 {
            let (lo, hi) = if c == 0 {
                (d.min().x, d.max().x)
            } else {
                (d.min().y, d.max().y)
            };
            assert!(inside(x[1 + 2 * k + c], lo, hi), "uv inside its domain");
        }
    }
    assert!(x[5..].iter().all(|&e| inside(e, -100.0, 100.0)));
    // F at new: exact, and the kernel's float enclosed
    let rows = |round: bool| -> Vec<Iv> {
        let (c, _) = trim.exact(x[0]);
        let c = if round { trim.kernel(x[0]) } else { c };
        let pr = offset_point(&rs, rd, x[1], x[2], round);
        let po = offset_point(&os, od, x[3], x[4], round);
        let mut f = vec![
            rnd(c[0].sub(Iv::p(x[1])), U, round),
            rnd(c[1].sub(Iv::p(x[2])), U, round),
        ];
        f.extend((0..3).map(|q| rnd(pr[q].sub(po[q]), U, round)));
        f.extend((0..extra).map(|k| {
            let tk = rnd(Iv::p(target).add(Iv::p(k as f64)), U, round);
            rnd(Iv::p(x[5 + k]).sub(tk), U, round)
        }));
        f
    };
    let (fx, fk) = (rows(false), rows(true));
    let kern = &sc.residuals[0];
    let mut beta = Vec::new();
    for j in 0..n {
        assert!(
            fk[j].contains(kern[j]),
            "{}: the kernel's F[{j}] {} outside {:?}",
            case.name,
            kern[j],
            fk[j]
        );
        let b = fk[j].sub(fx[j]).abs().hi;
        assert!(
            kern[j].abs() <= b,
            "{}: the solve stopped at F[{j}] = {}, above its rounding {b}",
            case.name,
            kern[j]
        );
        beta.push(up_add(kern[j].abs(), b));
    }
    // the constant Jacobian (exact)
    let (_, cd) = trim.exact(x[0]);
    let (_, ru, rv) = rs.exact(Iv::p(x[1]), Iv::p(x[2]));
    let (_, ou, ov) = os.exact(Iv::p(x[3]), Iv::p(x[4]));
    let z = Iv::p(0.0);
    let mut jm = vec![vec![z; n]; n];
    jm[0][0] = cd[0];
    jm[0][1] = Iv::p(-1.0);
    jm[1][0] = cd[1];
    jm[1][2] = Iv::p(-1.0);
    for q in 0..3 {
        jm[2 + q][1] = ru[q];
        jm[2 + q][2] = rv[q];
        jm[2 + q][3] = ou[q].neg();
        jm[2 + q][4] = ov[q].neg();
    }
    for k in 0..extra {
        jm[5 + k][5 + k] = Iv::p(1.0);
    }
    let inv = iv_inverse(&jm);
    for (i, &at) in idx.iter().enumerate() {
        let mut step = Iv::p(0.0);
        let mut bound = 0.0;
        for j in 0..n {
            step = step.add(inv[i][j].mul(fx[j]));
            bound = up_add(bound, up_mul(inv[i][j].mag(), beta[j]));
        }
        let r = Iv::p(x[i]).sub(step);
        let cert = Cert::Enc {
            r,
            e: Some(r.widen(bound)),
            what: "rail_eu",
        };
        if i == 0 {
            out.insert(10 + extra, cert.clone());
        }
        out.insert(at, cert);
    }
}

impl Srf {
    /// Whether this net is the B-spline `s`, bit for bit (degrees, knots, points).
    fn is(&self, s: &BSplineSurface) -> bool {
        let n = s.gw_nurb().expect("a nurb");
        let net: Vec<u64> = n
            .cpts()
            .iter()
            .flatten()
            .flat_map(|p| [p.x, p.y, p.z, p.w])
            .map(f64::to_bits)
            .collect();
        let ours: Vec<u64> = self
            .net
            .iter()
            .flatten()
            .flatten()
            .map(|x| x.to_bits())
            .collect();
        n.degrees() == (self.p, self.q)
            && bits(n.u_knots()) == bits(&self.u)
            && bits(n.v_knots()) == bits(&self.v)
            && net == ours
    }
}

// ---- mass.integrator: the Gauss-Kronrod partition, replayed in intervals ----------------------

// The integrator's rules (`mass/integrator/properties.rs`, private there): the 7-point
// Gauss-Kronrod rule of the trim pieces and the 15-point rule of the iso-u lines, centre node
// first, then each node at `±x`, with its Kronrod weight (the Gauss weights decide only
// acceptance, which the replay takes from the kernel's own decisions).
const KR7_NODES: [f64; 4] = [
    0.0,
    0.434_243_749_346_802_558_002_071_502_844_628,
    0.774_596_669_241_483_377_035_853_079_956_480,
    0.960_491_268_708_020_283_423_507_092_629_080,
];
const KR7_WEIGHTS: [f64; 4] = [
    0.450_916_538_658_474_142_345_110_087_045_571,
    0.401_397_414_775_962_222_905_051_818_618_432,
    0.268_488_089_868_333_440_728_569_280_666_710,
    0.104_656_226_026_467_265_193_823_857_192_073,
];
const GAUSS3_WEIGHTS: [f64; 4] = [8.0 / 9.0, 0.0, 5.0 / 9.0, 0.0];
const KR15_NODES: [f64; 8] = [
    0.0,
    0.207_784_955_007_898_467_600_689_403_773_245,
    0.405_845_151_377_397_166_906_606_412_076_961,
    0.586_087_235_467_691_130_294_144_845_693_013,
    0.741_531_185_599_394_439_863_864_773_280_788,
    0.864_864_423_359_769_072_789_712_788_640_926,
    0.949_107_912_342_758_524_526_189_684_047_851,
    0.991_455_371_120_812_639_206_854_697_526_329,
];
const KR15_WEIGHTS: [f64; 8] = [
    0.209_482_141_084_727_828_012_999_174_891_714,
    0.204_432_940_075_298_892_414_161_999_234_649,
    0.190_350_578_064_785_409_913_256_402_421_014,
    0.169_004_726_639_267_902_826_583_426_598_550,
    0.140_653_259_715_525_918_745_189_590_510_238,
    0.104_790_010_322_250_183_839_876_322_541_518,
    0.063_092_092_629_978_553_290_700_663_189_204,
    0.022_935_322_010_529_224_963_732_008_058_970,
];
const GAUSS7_WEIGHTS: [f64; 8] = [
    0.417_959_183_673_469_387_755_102_040_816_327,
    0.0,
    0.381_830_050_505_118_944_950_369_775_488_975,
    0.0,
    0.279_705_391_489_276_667_901_467_771_423_780,
    0.0,
    0.129_484_966_168_869_693_270_611_432_679_082,
    0.0,
];
type Rule = (&'static [f64], &'static [f64], &'static [f64]);
const KR7: Rule = (&KR7_NODES, &KR7_WEIGHTS, &GAUSS3_WEIGHTS);
const KR15: Rule = (&KR15_NODES, &KR15_WEIGHTS, &GAUSS7_WEIGHTS);

/// `kronrod_integrate_with` in floats, operation for operation, appending each piece's
/// accept (`true`) or halve decision to `tape` in the kernel's order (a piece's decision
/// after its nodes', and so after its nodes' inner integrals').
#[allow(clippy::too_many_arguments)]
fn kr_float<F>(
    (nodes, kw, gw): Rule,
    a: f64,
    b: f64,
    accuracy: f64,
    scale: &[f64],
    y: &mut [f64],
    tape: &mut Vec<bool>,
    mut f: F,
) where
    F: FnMut(f64, &mut [f64], &mut Vec<bool>),
{
    use solidcore::math::EFF_ZERO;
    let n = y.len();
    let mut stack = vec![(a, b, 0_u32)];
    while let Some((lo, hi, depth)) = stack.pop() {
        let mid = 0.5 * (lo + hi);
        let half = 0.5 * (hi - lo);
        let mut kronrod = vec![0.0; n];
        let mut gauss = vec![0.0; n];
        let mut centre = vec![0.0; n];
        f(mid, &mut centre, tape);
        for (i, &x) in nodes.iter().enumerate().skip(1) {
            for side in [-1.0, 1.0] {
                let mut dy = vec![0.0; n];
                f(mid + side * x * half, &mut dy, tape);
                for (((kv, gv), d), c) in kronrod.iter_mut().zip(&mut gauss).zip(&dy).zip(&centre) {
                    let off = *d - *c;
                    *kv += kw[i] * off;
                    *gv += gw[i] * off;
                }
            }
        }
        let mut accepted = true;
        for (i, ((kv, gv), c)) in kronrod.iter_mut().zip(&mut gauss).zip(&centre).enumerate() {
            *kv = half * (2.0 * *c + *kv);
            *gv = half * (2.0 * *c + *gv);
            let s = kv
                .abs()
                .max(scale.get(i).copied().unwrap_or(0.0))
                .max(EFF_ZERO);
            let estimate = (*kv - *gv).abs();
            if estimate.is_nan() || estimate > accuracy * s {
                accepted = false;
            }
        }
        tape.push(accepted);
        if accepted {
            for (t, p) in y.iter_mut().zip(&kronrod) {
                *t += *p;
            }
            continue;
        }
        assert!(
            depth < 48,
            "the replay bisects past the kernel's depth bound"
        );
        stack.push((mid, hi, depth + 1));
        stack.push((lo, mid, depth + 1));
    }
}

/// [`kr_float`] over intervals, the decisions read from `tape` at `pos`: every operation
/// enclosed (outward) and, `round`, widened by its own float rounding (`u |result|`), so the
/// result encloses the kernel's float sum wherever the integrand's enclosures hold its float
/// values (`round`), or the exact sum of the same partition (not `round`). `0.5 x` and `2 x`
/// are exact; `side · x` is an exact negation.
#[allow(clippy::too_many_arguments)]
fn kr_iv<F>(
    (nodes, kw, _): Rule,
    a: Iv,
    b: Iv,
    round: bool,
    y: &mut [Iv],
    tape: &[bool],
    pos: &mut usize,
    mut f: F,
) where
    F: FnMut(Iv, &mut [Iv], &mut usize),
{
    let r = |x: Iv| rnd(x, U, round);
    let n = y.len();
    let mut stack = vec![(a, b)];
    while let Some((lo, hi)) = stack.pop() {
        let mid = r(lo.add(hi)).mul(Iv::p(0.5));
        let half = r(hi.sub(lo)).mul(Iv::p(0.5));
        let mut kronrod = vec![Iv::p(0.0); n];
        let mut centre = vec![Iv::p(0.0); n];
        f(mid, &mut centre, pos);
        for (i, &x) in nodes.iter().enumerate().skip(1) {
            for side in [-1.0, 1.0] {
                let mut dy = vec![Iv::p(0.0); n];
                let at = r(mid.add(r(Iv::p(side * x).mul(half))));
                f(at, &mut dy, pos);
                for ((kv, d), c) in kronrod.iter_mut().zip(&dy).zip(&centre) {
                    let off = r(d.sub(*c));
                    *kv = r(kv.add(r(Iv::p(kw[i]).mul(off))));
                }
            }
        }
        let accepted = tape[*pos];
        *pos += 1;
        if accepted {
            for ((t, kv), c) in y.iter_mut().zip(&kronrod).zip(&centre) {
                let part = r(half.mul(r(Iv::p(2.0).mul(*c).add(*kv))));
                *t = r(t.add(part));
            }
            continue;
        }
        stack.push((mid, hi));
        stack.push((lo, mid));
    }
}

/// The kernel's accept rule for the exact decision check of [`kr_settle`]: the rule's
/// `accuracy` (the outer and the inner integrals both run at `accuracy / 10`), the
/// per-component `scale`, and the count of pieces the check left unsettled.
struct Settle {
    accuracy: f64,
    scale: Vec<f64>,
    unsettled: std::cell::Cell<usize>,
}

/// [`kr_iv`] in exact arithmetic (not `round`), every tape decision checked against the
/// kernel's rule on the exact values: a piece is accepted when every component has
/// `|K − G| <= accuracy · max(|K|, scale, EFF_ZERO)`. With `K`, `G` enclosed:
/// - an accepted piece must be settled (no component can exceed its threshold), else the
///   exact pipeline could split it and its branch is not on the tape: the test fails;
/// - a split piece settled as a split adds its halves; one the exact rule could accept
///   (unsettled, or settled the other way) adds the hull of both branches, its halves'
///   sum and its own `K`, so `R` holds the exact pipeline's value whichever way it goes.
///
/// The same nodes, sums and order as [`kr_iv`] (halves left then right, the tape's order);
/// the recursion is bounded by the kernel's depth (48), asserted.
#[allow(clippy::too_many_arguments)]
fn kr_settle<F>(
    rule: Rule,
    lo: Iv,
    hi: Iv,
    settle: &Settle,
    y: &mut [Iv],
    tape: &[bool],
    pos: &mut usize,
    f: &mut F,
    depth: u32,
) where
    F: FnMut(Iv, &mut [Iv], &mut usize),
{
    use solidcore::math::EFF_ZERO;
    assert!(
        depth <= 48,
        "the replay bisects past the kernel's depth bound"
    );
    let (nodes, kw, gw) = rule;
    let n = y.len();
    let zero = Iv::p(0.0);
    let mid = lo.add(hi).mul(Iv::p(0.5));
    let half = hi.sub(lo).mul(Iv::p(0.5));
    let mut kronrod = vec![zero; n];
    let mut gauss = vec![zero; n];
    let mut centre = vec![zero; n];
    f(mid, &mut centre, pos);
    for (i, &x) in nodes.iter().enumerate().skip(1) {
        for side in [-1.0, 1.0] {
            let mut dy = vec![zero; n];
            let at = mid.add(Iv::p(side * x).mul(half));
            f(at, &mut dy, pos);
            for (((kv, gv), d), c) in kronrod.iter_mut().zip(&mut gauss).zip(&dy).zip(&centre) {
                let off = d.sub(*c);
                *kv = kv.add(Iv::p(kw[i]).mul(off));
                *gv = gv.add(Iv::p(gw[i]).mul(off));
            }
        }
    }
    let whole: Vec<Iv> = kronrod
        .iter()
        .zip(&centre)
        .map(|(kv, c)| half.mul(Iv::p(2.0).mul(*c).add(*kv)))
        .collect();
    // can the exact rule accept (no component surely over), can it split (one possibly over)
    let mut can_accept = true;
    let mut can_split = false;
    for (i, (k, gv)) in whole.iter().zip(&gauss).enumerate() {
        let g = half.mul(Iv::p(2.0).mul(centre[i]).add(*gv));
        let est = k.sub(g).abs();
        let ka = k.abs();
        let sc = settle.scale.get(i).copied().unwrap_or(0.0).max(EFF_ZERO);
        let thr = Iv {
            lo: ka.lo.max(sc),
            hi: ka.hi.max(sc),
        }
        .mul(Iv::p(settle.accuracy));
        if est.lo > thr.hi {
            can_accept = false;
        }
        if est.hi > thr.lo {
            can_split = true;
        }
    }
    let accepted = tape[*pos];
    *pos += 1;
    let part = if accepted {
        assert!(
            !can_split,
            "an accepted piece the exact rule could split (its branch is not on the tape)"
        );
        whole
    } else {
        let mut halves = vec![zero; n];
        kr_settle(rule, lo, mid, settle, &mut halves, tape, pos, f, depth + 1);
        kr_settle(rule, mid, hi, settle, &mut halves, tape, pos, f, depth + 1);
        if can_accept {
            settle.unsettled.set(settle.unsettled.get() + 1);
            halves
                .iter()
                .zip(&whole)
                .map(|(h, w)| Iv::hull(*h, *w))
                .collect()
        } else {
            halves
        }
    };
    for (t, p) in y.iter_mut().zip(&part) {
        *t = t.add(*p);
    }
}

/// [`kr_settle`] for the exact pass (not `round`) when `settle` is given, else [`kr_iv`].
#[allow(clippy::too_many_arguments)]
fn kr_run<F>(
    rule: Rule,
    a: Iv,
    b: Iv,
    round: bool,
    settle: Option<&Settle>,
    y: &mut [Iv],
    tape: &[bool],
    pos: &mut usize,
    mut f: F,
) where
    F: FnMut(Iv, &mut [Iv], &mut usize),
{
    match settle {
        Some(s) if !round => kr_settle(rule, a, b, s, y, tape, pos, &mut f, 0),
        _ => kr_iv(rule, a, b, round, y, tape, pos, f),
    }
}

/// The kernel's float `S`, `S_u`, `S_v` of a rational net at `(u, v)` enclosed operation for
/// operation as `first_derivs_span` (`nurbs/srf_nurb.rs`) computes them (`round`), or the
/// exact values (not `round`): the basis values and first derivatives from `univariate_first`
/// (enclosed as [`Srf::kernel`] counts them, `γ_{6p}` of `Σ|terms|`), the Euclidean points
/// `P (1/w)`, the weight sums `d00`, `d01`, `d10` (one rounding per product and per sum), the
/// reciprocal `1/d00`, and per control point the rational basis recurrence `r00 = w a0 b0 /
/// d00`, `r01 = (w a0 b1 − d01 r00) / d00`, `r10 = (w a1 b0 − d10 r00) / d00` and the running
/// sums `s += r P`.
fn srf_kernel_eval(s: &Srf, u: Iv, v: Iv, round: bool) -> Eval {
    assert!(s.rational, "the rational path");
    let r = |x: Iv| rnd(x, U, round);
    let (nu, du, dua) = basis(&s.u, s.p, u);
    let (nv, dv, dva) = basis(&s.v, s.q, v);
    let widen = |x: &[Iv], m: Option<&[Iv]>, p: usize| -> Vec<Iv> {
        x.iter()
            .enumerate()
            .map(|(i, xi)| {
                if !round {
                    return *xi;
                }
                let g = gam(6 * p);
                match m {
                    None => xi.rel(g),
                    Some(m) => xi.widen(up_mul(g, m[i].hi)),
                }
            })
            .collect()
    };
    let (a0s, a1s) = (widen(&nu, None, s.p), widen(&du, Some(&dua), s.p));
    let (b0s, b1s) = (widen(&nv, None, s.q), widen(&dv, Some(&dva), s.q));
    let zero = Iv::p(0.0);
    let w = |i: usize, j: usize| Iv::p(s.net[i][j][3]);
    let pe = |i: usize, j: usize| -> V3 {
        let rw = r(Iv::p(1.0).div(w(i, j)));
        std::array::from_fn(|c| r(Iv::p(s.net[i][j][c]).mul(rw)))
    };
    let (ni, nj) = (s.net.len(), s.net[0].len());
    // d[k][l]: Σ_i (Σ_j w_ij b_l,j) a_k,i
    let mut d = [[zero; 2]; 2];
    for (l, bl) in [&b0s, &b1s].into_iter().enumerate() {
        let tu: Vec<Iv> = (0..ni)
            .map(|i| {
                let mut t = zero;
                for (j, b) in bl.iter().enumerate().take(nj) {
                    t = r(t.add(r(w(i, j).mul(*b))));
                }
                t
            })
            .collect();
        for (kk, ak) in [&a0s, &a1s].into_iter().enumerate() {
            let mut x = zero;
            for (t, a) in tu.iter().zip(ak) {
                x = r(x.add(r(t.mul(*a))));
            }
            d[kk][l] = x;
        }
    }
    let (d00, d01, d10) = (d[0][0], d[0][1], d[1][0]);
    let inv = r(Iv::p(1.0).div(d00));
    let mut sum = [[zero; 3]; 3];
    for i in 0..ni {
        for j in 0..nj {
            let (a0, a1, b0, b1) = (a0s[i], a1s[i], b0s[j], b1s[j]);
            let wij = w(i, j);
            let r00 = r(r(r(wij.mul(a0)).mul(b0)).mul(inv));
            let r01 = r(r(r(r(wij.mul(a0)).mul(b1)).sub(r(d01.mul(r00)))).mul(inv));
            let v10 = r(r(r(wij.mul(a1)).mul(b0)).sub(r(d10.mul(r00))));
            let r10 = r(v10.mul(inv));
            let p = pe(i, j);
            for (slot, rr) in sum.iter_mut().zip([r00, r10, r01]) {
                for c in 0..3 {
                    slot[c] = r(slot[c].add(r(rr.mul(p[c]))));
                }
            }
        }
    }
    (sum[0], sum[1], sum[2])
}

/// The iso-u integrand (`OdeIsoCurveLength::evaluate`, thickness 0) at `(u, v)` about
/// `origin`, operation for operation: `g[c]` is the kernel's `dydx[c + 1]`.
fn iso_iv(s: &Srf, lay: &Layout, origin: [f64; 3], u: Iv, v: Iv, round: bool) -> Vec<Iv> {
    use solidcore::math::EFF_ZERO_SQ;
    let r = |x: Iv| rnd(x, U, round);
    let (pt, su, sv) = srf_kernel_eval(s, u, v, round);
    let x: V3 = std::array::from_fn(|c| r(pt[c].sub(Iv::p(origin[c]))));
    let cr = |i: usize, j: usize| r(r(su[i].mul(sv[j])).sub(r(su[j].mul(sv[i]))));
    let norm = [cr(1, 2), cr(2, 0), cr(0, 1)];
    let sq = |a: &V3| r(r(r(a[0].mul(a[0])).add(r(a[1].mul(a[1])))).add(r(a[2].mul(a[2]))));
    let area = r(sq(&norm).sqrt());
    let mut g = vec![Iv::p(0.0); lay.n];
    assert!(sq(&su).lo > 0.0, "|S_u| > 0 is undecided");
    g[0] = area;
    if lay.area != 0 {
        g[lay.area - 1] = area;
    }
    let m = |a: Iv, b: Iv| r(a.mul(b));
    if let Some(i) = lay.moments[0] {
        for c in 0..3 {
            g[i + c] = m(x[c], area);
        }
    }
    if let Some(i) = lay.moments[1] {
        let xx = m(x[0], x[0]);
        let yy = m(x[1], x[1]);
        let zz = m(x[2], x[2]);
        g[i] = m(xx, area);
        g[i + 1] = m(yy, area);
        g[i + 2] = m(zz, area);
        g[i + 3] = m(m(x[1], x[2]), area);
        g[i + 4] = m(m(x[2], x[0]), area);
        g[i + 5] = m(m(x[0], x[1]), area);
        g[i + 6] = m(r(yy.add(zz)), area);
        g[i + 7] = m(r(xx.add(zz)), area);
        g[i + 8] = m(r(xx.add(yy)), area);
    }
    if lay.volume == 0 && lay.moments[4].is_none() && lay.moments[5].is_none() {
        return g;
    }
    let lsq = sq(&norm);
    assert!(
        lsq.lo > EFF_ZERO_SQ,
        "the normal's unitisation is undecided"
    );
    let lsq1 = lsq.sub(Iv::p(1.0)).abs();
    assert!(lsq1.lo > EFF_ZERO_SQ, "the normal's unit test is undecided");
    let len = r(lsq.sqrt());
    let nz = r(norm[2].div(len));
    let vd = m(m(area, x[2]), nz);
    if lay.volume != 0 {
        g[lay.volume - 1] = vd;
    }
    let half = |a: Iv| a.mul(Iv::p(0.5));
    if let Some(i) = lay.moments[4] {
        g[i] = m(x[0], vd);
        g[i + 1] = m(x[1], vd);
        g[i + 2] = half(m(x[2], vd));
    }
    if let Some(i) = lay.moments[5] {
        let xx = m(x[0], x[0]);
        let yy = m(x[1], x[1]);
        let zz3 = r(m(x[2], x[2]).div(Iv::p(3.0)));
        g[i] = m(xx, vd);
        g[i + 1] = m(yy, vd);
        g[i + 2] = m(zz3, vd);
        g[i + 3] = half(m(m(x[1], x[2]), vd));
        g[i + 4] = half(m(m(x[2], x[0]), vd));
        g[i + 5] = m(m(x[0], x[1]), vd);
        g[i + 6] = m(r(yy.add(zz3)), vd);
        g[i + 7] = m(r(xx.add(zz3)), vd);
        g[i + 8] = m(r(xx.add(yy)), vd);
    }
    g
}

/// `PrecisePropertyLayout` with 0-based component slots of `dydx[1..]` (`g`): the control
/// component is `g[0]`.
struct Layout {
    n: usize,
    area: usize,
    volume: usize,
    moments: [Option<usize>; 8],
}

impl Layout {
    fn of(l: &solidcore::topology::face::p2::PrecisePropertyLayout) -> Layout {
        Layout {
            n: l.num_components,
            area: l.area,
            volume: l.volume,
            moments: l.moments.map(|m| if m == 0 { None } else { Some(m - 1) }),
        }
    }
}

/// One straight trim of the rectangle face (a degree-1 B-spline from `c0` to `c1` over
/// `[t0, t1]`), as `create_oriented_trim_boundary` returns it: exact corners of the domain.
struct Side {
    curve: Curve,
    c0: [f64; 2],
    c1: [f64; 2],
    t: [f64; 2],
}

impl Side {
    /// `C(t)` and `C'(t)` over `t`, enclosed: exact (not `round`), or the kernel's float
    /// `evaluate` (`round`: the exact values widened by `γ_16 Σ|terms|`). `16` over-counts
    /// the degree-1 chain of `BSplineCurve::evaluate` (`crv_derivs_n` in
    /// `nurbs/nurbs_crv.rs`: `basis_derivs_into`, then `Σ b P` on the Euclidean points),
    /// counted relative to the terms as [`Crv::kernel`] counts:
    /// - the value, `9`: the basis level `5` (`left`/`right` 1, their sum `den` 1, the
    ///   quotient `ll / den` 1, the product by `ndu` 1, the sum `saved + …` 1), the
    ///   weighted sum `3` (the product `b · P` 1, the running sum of the `p + 1 = 2` terms
    ///   2), the Euclidean point `P = Pw / w` `1` (here `w = 1`, exact, counted anyway);
    /// - the derivative, `10`: the stored divisor `den` `2` (`left`/`right` and their sum),
    ///   the quotient `a / den` 1, the product by `ndu` 1, the sum of the two terms 1, the
    ///   factor `p` 1, then the weighted sum `3` and the Euclidean point `1`.
    ///
    /// Every trim value the kernel computed is also checked inside this enclosure.
    fn eval(&self, t: Iv, round: bool) -> ([Iv; 2], [Iv; 2]) {
        let (t0, t1) = (Iv::p(self.t[0]), Iv::p(self.t[1]));
        let dt = t1.sub(t0);
        let n1 = t.sub(t0).div(dt);
        let n0 = t1.sub(t).div(dt);
        let mut p = [Iv::p(0.0); 2];
        let mut d = [Iv::p(0.0); 2];
        for c in 0..2 {
            let (a, b) = (Iv::p(self.c0[c]), Iv::p(self.c1[c]));
            p[c] = n0.mul(a).add(n1.mul(b));
            d[c] = b.sub(a).div(dt);
            if round {
                let g = gam(16);
                let mp = up_add(n0.mul(a).mag(), n1.mul(b).mag());
                let md = Iv::p(self.c0[c].abs())
                    .add(Iv::p(self.c1[c].abs()))
                    .div(dt)
                    .hi;
                p[c] = p[c].widen(up_mul(g, mp));
                d[c] = d[c].widen(up_mul(g, md));
            }
        }
        (p, d)
    }
}

/// The kernel's float result of `boundary_properties` (unbanded, thickness 0) over the
/// rectangle face, replayed, with the decision tape (the partition); and, `intervals`, the
/// same partition in intervals, `(R, E)`: `R` the exact sums of that partition over the
/// exact trims, `E` the enclosure of the kernel's float sums (empty without `intervals`).
/// Each output as `[area, volume, moments (8 × 3)]`.
struct MassReplay {
    float: Vec<f64>,
    r: Vec<Iv>,
    e: Vec<Iv>,
    tape: Vec<bool>,
    /// With `settle`: the split pieces whose exact decision is not settled as a split
    /// (their `R` the hull of both branches, [`kr_settle`]).
    unsettled: usize,
}

#[allow(clippy::too_many_arguments)]
fn mass_replay(
    k: &Kernel,
    surface: &Surface,
    sides: &[Side],
    face_box: &solidcore::geom::extent3d::Extent3d,
    min_v: f64,
    same: bool,
    accuracy: f64,
    origin: [f64; 3],
    estimated_area: f64,
    flags: u64,
    intervals: bool,
    settle: bool,
) -> MassReplay {
    use solidcore::geom::extent3d::Extent3d;
    use solidcore::math::{Vec3, EFF_ZERO, EFF_ZERO_SQ};
    use solidcore::nurbs::b_spline_curve::p2::BoundingBoxRequest;
    use solidcore::topology::face::p2::{
        surface_knots_inside, trim_curve_breaks, OdeIsoCurveLength, PrecisePropertyLayout,
    };
    const POWERS: [u32; 8] = [1, 2, 2, 2, 2, 3, 3, 3];
    let accuracy = if accuracy < EFF_ZERO {
        EFF_ZERO_SQ
    } else {
        accuracy
    };
    let layout = PrecisePropertyLayout::new(flags);
    let lay = Layout::of(&layout);
    let o3 = Vec3::new(origin[0], origin[1], origin[2]);
    let mut scale = vec![solidcore::math::BIG_DOUBLE; layout.num_components];
    let origin_distance = face_box.min_distance(&Extent3d::from_point(o3));
    let sign = if same { -1.0 } else { 1.0 };
    // the base: the trims' lowest v (trims_v_range), no lower than the domain's
    let req = BoundingBoxRequest {
        normal: true,
        pseudo: false,
        polar: false,
    };
    let mut base = solidcore::math::BIG_DOUBLE;
    let mut top = -solidcore::math::BIG_DOUBLE;
    for s in sides {
        let bb = s
            .curve
            .calculate_bounding_box(k, &s.curve.natural_interval(), req, true)
            .unwrap()
            .normal
            .unwrap();
        base = base.min(bb.min().y);
        top = top.max(bb.max().y);
    }
    // (trims_v_range: no lower than the domain's lowest v)
    if base < min_v {
        base = min_v;
    }
    let top = if top > base { top } else { base };
    let lines = surface_knots_inside(k, surface, SurfParamType::V, base, top).unwrap();
    assert!(lines.len() < 12, "a banded face");
    let area_scale = estimated_area / 10.0;
    scale[0] = area_scale;
    let length = {
        let a = 10.0 * estimated_area.sqrt();
        if a > origin_distance {
            a
        } else {
            origin_distance
        }
    };
    if layout.volume != 0 {
        scale[layout.volume - 1] = area_scale * length;
    }
    for (&index, &power) in layout.moments.iter().zip(POWERS.iter()) {
        if index == 0 {
            continue;
        }
        let ms = area_scale * length.powf(f64::from(power));
        for cc in 0..3 {
            scale[index - 1 + cc] = ms;
        }
    }
    let iso_acc = accuracy / 10.0;
    let n = layout.num_components;
    let srf = Srf::of(nurb_of(k, surface));
    // which trims integrate: the vertical sides have `du/dt = 0` (no integral), the bottom
    // `v = base` a zero-width one; the top's iso lines run from `base` up to it
    for s in sides {
        let vertical = s.c0[0] == s.c1[0];
        let horizontal = s.c0[1] == s.c1[1];
        assert!(vertical != horizontal, "a trim that is not axis-parallel");
        if horizontal {
            assert!(s.c0[1] >= base, "a trim below the base");
        }
    }

    // ---- floats, recording the tape ----
    let mut tape = Vec::new();
    let mut vals: Vec<f64> = Vec::new();
    let mut y = vec![0.0; n];
    let mut breaks_of = Vec::new();
    for s in sides {
        let trim = &s.curve;
        let vertical = s.c0[0] == s.c1[0];
        let flat = !vertical && s.c0[1] == base;
        let breaks = trim_curve_breaks(k, trim, surface, &[]).unwrap();
        let mut trim_y = vec![0.0; n];
        for w in breaks.windows(2) {
            let (a, b) = (w[0], w[1]);
            if b.partial_cmp(&a) != Some(std::cmp::Ordering::Greater) {
                continue;
            }
            let mut span_y = vec![0.0; n];
            kr_float(
                KR7,
                a,
                b,
                accuracy / 10.0,
                &scale,
                &mut span_y,
                &mut tape,
                |t, dy, tape| {
                    let pv = trim.evaluate(k, t, 1, true, true).unwrap();
                    let (p0, p1) = (pv[0], pv[1]);
                    dy.fill(0.0);
                    if p1.x == 0.0 {
                        assert!(vertical, "a zero du/dt off a vertical side");
                        return;
                    }
                    assert!(!vertical, "a vertical side's float du/dt is not 0");
                    assert!(p0.y >= base, "a trim point below the base");
                    if p0.y == base {
                        assert!(flat, "a trim point on the base off the bottom");
                        return;
                    }
                    assert!(!flat, "the bottom's float point is off the base");
                    vals.extend([p0.x, p0.y, p1.x]);
                    let eval = OdeIsoCurveLength::new(k, surface, p0.x, o3, 0.0, &layout);
                    let mut scratch = vec![0.0; n + 1];
                    let mut v0 = base;
                    let knots =
                        surface_knots_inside(k, surface, SurfParamType::V, base, p0.y).unwrap();
                    for v1 in knots.into_iter().chain(std::iter::once(p0.y)) {
                        kr_float(KR15, v0, v1, iso_acc, &scale, dy, tape, |v, g, _| {
                            eval.evaluate(v, &mut scratch).unwrap();
                            for (d, x) in g.iter_mut().zip(scratch.iter().skip(1)) {
                                *d = *x;
                            }
                            vals.extend_from_slice(g);
                        });
                        v0 = v1;
                    }
                    for d in dy.iter_mut() {
                        *d = -(*d * p1.x);
                    }
                },
            );
            for (t, p) in trim_y.iter_mut().zip(&span_y) {
                *t += *p;
            }
        }
        for (t, p) in y.iter_mut().zip(&trim_y) {
            *t += *p;
        }
        breaks_of.push(breaks);
    }
    let assemble = |y: &[f64]| -> Vec<f64> {
        let value = |i: usize| if i == 0 { 0.0 } else { y[i - 1] };
        let mut out = vec![0.0; 26];
        if layout.area != 0 {
            out[0] = 0.0 + value(layout.area);
        }
        if layout.volume != 0 {
            out[1] = 0.0 + value(layout.volume) * sign;
        }
        for (kk, &index) in layout.moments.iter().enumerate() {
            if index == 0 {
                continue;
            }
            let ms = if kk < 4 { 1.0 } else { sign };
            for c in 0..3 {
                out[2 + 3 * kk + c] = 0.0 + value(index + c) * ms;
            }
        }
        out
    };
    let float = assemble(&y);
    if !intervals {
        return MassReplay {
            float,
            r: Vec::new(),
            e: Vec::new(),
            tape,
            unsettled: 0,
        };
    }

    // ---- intervals over the same partition ----
    let settler = Settle {
        accuracy: iso_acc,
        scale: scale.clone(),
        unsettled: std::cell::Cell::new(0),
    };
    let settle = settle.then_some(&settler);
    let interval = |round: bool| -> Vec<Iv> {
        let r = |x: Iv| rnd(x, U, round);
        let mut pos = 0usize;
        let mut vcount = 0usize;
        let vc = &mut vcount;
        let mut y = vec![Iv::p(0.0); n];
        for (s, breaks) in sides.iter().zip(&breaks_of) {
            let vertical = s.c0[0] == s.c1[0];
            let flat = !vertical && s.c0[1] == base;
            let mut trim_y = vec![Iv::p(0.0); n];
            for w in breaks.windows(2) {
                let (a, b) = (w[0], w[1]);
                if b.partial_cmp(&a) != Some(std::cmp::Ordering::Greater) {
                    continue;
                }
                let mut span_y = vec![Iv::p(0.0); n];
                kr_run(
                    KR7,
                    Iv::p(a),
                    Iv::p(b),
                    round,
                    settle,
                    &mut span_y,
                    &tape,
                    &mut pos,
                    |t, dy, pos| {
                        dy.fill(Iv::p(0.0));
                        if vertical || flat {
                            return;
                        }
                        let (mut p0, p1) = s.eval(t, round);
                        if round {
                            for (c, x) in [p0[0], p0[1], p1[0]].iter().enumerate() {
                                assert!(
                                    x.contains(vals[*vc + c]),
                                    "a trim value outside E: {x:?} {}",
                                    vals[*vc + c]
                                );
                            }
                        }
                        // the kernel's float v of the trim point
                        let fy = vals[*vc + 1];
                        *vc += 3;
                        // A side of constant v (the sides are the domain's: of constant u, whose
                        // pieces add nothing, or of constant v): the exact trim point's v is that
                        // constant, `R`'s top bit for bit; `E` keeps the evaluation's enclosure.
                        if !round {
                            p0[1] = Iv::p(s.c0[1]);
                        }
                        // the top's iso line: base up to the trim point, split at the v knots
                        // strictly between, the kernel's (from its float v): the exact top's, and
                        // at most the top itself as well (a float v an ulp past a knot the side
                        // lies on), whose piece is `[top, top]` in `R`, adding exactly nothing
                        let knots =
                            surface_knots_inside(k, surface, SurfParamType::V, base, fy).unwrap();
                        let exact =
                            surface_knots_inside(k, surface, SurfParamType::V, base, s.c0[1])
                                .unwrap();
                        assert!(
                            knots.len() >= exact.len()
                                && knots[..exact.len()] == exact[..]
                                && knots[exact.len()..].iter().all(|&x| x == s.c0[1]),
                            "the kernel's knots below the trim point {knots:?} are not the exact top's {exact:?}"
                        );
                        let mut v0 = Iv::p(base);
                        let mut ends: Vec<Iv> = knots.iter().map(|&x| Iv::p(x)).collect();
                        ends.push(p0[1]);
                        for v1 in ends {
                            kr_run(KR15, v0, v1, round, settle, dy, &tape, pos, |v, g, _| {
                                let iso = iso_iv(&srf, &lay, origin, p0[0], v, round);
                                if round {
                                    for (c, x) in iso.iter().enumerate() {
                                        assert!(
                                            x.contains(vals[*vc + c]),
                                            "an integrand value outside E: [{c}] {x:?} {}",
                                            vals[*vc + c]
                                        );
                                    }
                                }
                                *vc += iso.len();
                                g.copy_from_slice(&iso);
                            });
                            v0 = v1;
                        }
                        for d in dy.iter_mut() {
                            *d = r(d.mul(p1[0])).neg();
                        }
                    },
                );
                for (t, p) in trim_y.iter_mut().zip(&span_y) {
                    *t = r(t.add(*p));
                }
            }
            for (t, p) in y.iter_mut().zip(&trim_y) {
                *t = r(t.add(*p));
            }
        }
        assert_eq!(pos, tape.len(), "the tape is not consumed");
        assert_eq!(*vc, vals.len(), "the values are not consumed");
        let value = |i: usize| if i == 0 { Iv::p(0.0) } else { y[i - 1] };
        let mut out = vec![Iv::p(0.0); 26];
        if layout.area != 0 {
            out[0] = value(layout.area);
        }
        if layout.volume != 0 {
            out[1] = value(layout.volume).mul(Iv::p(sign));
        }
        for (kk, &index) in layout.moments.iter().enumerate() {
            if index == 0 {
                continue;
            }
            let ms = if kk < 4 { 1.0 } else { sign };
            for c in 0..3 {
                out[2 + 3 * kk + c] = value(index + c).mul(Iv::p(ms));
            }
        }
        out
    };
    let (r, e) = (interval(false), interval(true));
    MassReplay {
        float,
        r,
        e,
        tape,
        unsettled: settler.unsettled.get(),
    }
}

/// The B-spline the kernel evaluates a surface by (`evaluate_simple_first_into`): its own,
/// or an analytic kind's NURB (`analytic_base_with_nurb`).
fn nurb_of<'a>(k: &Kernel, s: &'a Surface) -> &'a BSplineSurface {
    match s.kind_with_nurb(k).unwrap() {
        SurfaceKind::BSpline(b) => b,
        SurfaceKind::Cone(c) => c.rev().base(),
        SurfaceKind::Sphere(c) => c.base().base(),
        SurfaceKind::Torus(c) => c.base().base(),
        SurfaceKind::SurfOfRevolution(c) => c.base(),
        SurfaceKind::SurfOfExtrusion(c) => c.base(),
        other => panic!("a surface the replay does not evaluate: {other:?}"),
    }
}

impl Srf {
    /// A kernel B-spline surface's net (homogeneous points, as the kernel stores them).
    fn of(s: &BSplineSurface) -> Srf {
        let n = s.gw_nurb().expect("a nurb");
        let (p, q) = n.degrees();
        let net: Vec<Vec<[f64; 4]>> = n
            .cpts()
            .iter()
            .map(|row| row.iter().map(|c| [c.x, c.y, c.z, c.w]).collect())
            .collect();
        let rational = net.iter().flatten().any(|cp| cp[3] > -1e300);
        Srf {
            net,
            rational,
            p,
            q,
            u: n.u_knots().to_vec(),
            v: n.v_knots().to_vec(),
        }
    }
}

/// A face's precise properties as `face_properties` integrates them, replayed
/// ([`mass_replay`]) with the kernel's own preparation in its order: the face's box (the
/// estimated area `d²/100` and, `origin` `None`, the box middle as the origin), the oriented
/// trims, the box again (the scale's origin distance). `exact`: the trims must be the face's
/// domain sides, every end a corner of `uv_domain` bit for bit (the exact sides), and the
/// partition is replayed in intervals too; otherwise (the main-side tape dump, whose trims
/// need not be exact) the floats and the tape only. Returns the replay and the origin.
#[allow(clippy::too_many_arguments)]
fn face_replay(
    k: &Kernel,
    b: &mut Brep,
    f: solidcore::topology::FaceId,
    same: bool,
    origin: Option<[f64; 3]>,
    flags: u64,
    accuracy: f64,
    exact: bool,
    settle: bool,
) -> (MassReplay, [f64; 3]) {
    use solidcore::geom::attribute::ObjectKey;
    use solidcore::geom::object::ObjectRef;
    use solidcore::EntityRef;
    let bbox = solidcore::query::face::p1::calculate_bounding_box(k, &*b, f, false).unwrap();
    let key_of = |e: EntityRef| ObjectRef {
        type_code: 0,
        key: ObjectKey(match e {
            EntityRef::Edgeuse(eu) => u64::from(eu.index()),
            _ => 0,
        }),
    };
    let trims = solidcore::topology::face::p1::create_oriented_trim_boundary(
        k, b, f, true, true, None, &key_of,
    )
    .unwrap()
    .curves;
    let fbox = solidcore::query::face::p1::calculate_bounding_box(k, &*b, f, false).unwrap();
    let dom = b.face(f).unwrap().uv_domain;
    let (us, vs) = ([dom.min().x, dom.max().x], [dom.min().y, dom.max().y]);
    let sides: Vec<Side> = trims
        .into_iter()
        .map(|c| {
            let CurveKind::BSpline(bs) = c.kind() else {
                panic!("a trim that is not a B-spline")
            };
            let cr = crv_of(bs);
            assert!(cr.p == 1 && cr.cps.len() == 2, "a trim that is not a line");
            assert!(cr.cps.iter().all(|q| q[3] == 1.0 && q[2] == 0.0));
            // the exact sides: every end a corner of the domain, bit for bit
            for q in &cr.cps {
                assert!(
                    !exact || (us.contains(&q[0]) && vs.contains(&q[1])),
                    "a trim end off the domain's corners: {q:?}"
                );
            }
            let t = [cr.knots[1], cr.knots[2]];
            assert!(cr.knots[0] == t[0] && cr.knots[3] == t[1]);
            Side {
                c0: [cr.cps[0][0], cr.cps[0][1]],
                c1: [cr.cps[1][0], cr.cps[1][1]],
                t,
                curve: c,
            }
        })
        .collect();
    let surface = b.face(f).unwrap().surface.clone().expect("a surface");
    let size = bbox.size();
    let d = size.x.max(size.y).max(size.z);
    let est = d * d / 100.0;
    let mid = bbox.mid();
    let origin = origin.unwrap_or([mid.x, mid.y, mid.z]);
    let m = mass_replay(
        k, &surface, &sides, &fbox, vs[0], same, accuracy, origin, est, flags, exact, settle,
    );
    (m, origin)
}

/// The certificates of the moved elements `i` (`v[i] != main[i]`) of a row whose replayed
/// float values are `fl` and interval values `(rr, ee)`: `Enc { r: R, e: E }`, asserting the
/// replay is the row's vector bit for bit, and that main's value lies in the same `E` or the
/// new value is closer.
fn label_replayed(
    name: &str,
    fl: &[f64],
    rr: &[Iv],
    ee: &[Iv],
    v: &[f64],
    main: &[f64],
    out: &mut BTreeMap<usize, Cert>,
) {
    assert_eq!(
        bits(fl),
        bits(v),
        "{name}: the replay is not the row's vector"
    );
    let mut moved = 0;
    let mut main_in_e = 0;
    let mut width = 0.0f64;
    for (i, (r, e)) in rr.iter().zip(ee).enumerate() {
        if v[i].to_bits() == main[i].to_bits() {
            continue;
        }
        width = width.max((e.hi - e.lo) / v[i].abs());
        let c = Cert::Enc {
            r: *r,
            e: Some(*e),
            what: "Gauss-Kronrod partition replay",
        };
        let main_in = e.contains(main[i]);
        assert!(
            main_in || classify(v[i], main[i], &c) == Class::Closer,
            "{name} [{i}]: main {:?} is outside E {e:?} and the new {:?} is not closer",
            main[i],
            v[i]
        );
        moved += 1;
        main_in_e += usize::from(main_in);
        out.insert(i, c);
    }
    eprintln!(
        "replayed {name}: {moved} moved, main in E at {main_in_e}, largest width(E) / |new| {width:.1e}"
    );
}

/// A replayed `mass.integrator` or `boolean.api_brep` row: the row's outputs from the replayed
/// properties in floats; with `exact`, their `R` and `E` (empty otherwise); and the decision
/// tape (the partition) of every replayed face, in the order replayed.
struct RowReplay {
    float: Vec<f64>,
    r: Vec<Iv>,
    e: Vec<Iv>,
    tapes: Vec<Vec<bool>>,
    /// Per replayed face, with the exact decision check ([`kr_settle`]): the split pieces
    /// whose `R` is the hull of both branches.
    unsettled: Vec<usize>,
}

/// A `mass.integrator` face row (`issue1435_multispan_*`: the rational kind-5 face over a
/// rectangle of its domain) replayed ([`face_replay`]), the query's own outputs as the
/// runner computes them from the properties.
fn mass_row_replay(case: &goldens::Case, exact: bool) -> RowReplay {
    use goldens::units::mass_integrator::face_scene;
    use solidcore::topology::face::p2::{PPF_AREA, PPF_AREA_FIRST_MOMENTS};
    let (k, mut b, f, p, query) = face_scene(case).unwrap().expect("a face case");
    let (same, origin, flags) = match query {
        0 => (p[14] == 0.0, Some([p[9], p[10], p[11]]), p[15] as u64),
        2 | 4 => (true, None, PPF_AREA),
        3 => (true, None, PPF_AREA | PPF_AREA_FIRST_MOMENTS),
        _ => panic!("query {query}"),
    };
    assert!(query != 0 || p[13] == 0.0, "a thick face");
    let (m, origin) = face_replay(&k, &mut b, f, same, origin, flags, p[12], exact, false);
    let a = m.float[0];
    // the query's outputs from the properties
    let float = match query {
        0 => m.float.clone(),
        2 | 4 => vec![a.abs()],
        3 => {
            // `origin + m / area`
            let mut fo = vec![a.abs()];
            for (c, &oc) in origin.iter().enumerate() {
                fo.push(oc + m.float[2 + c] / a);
            }
            fo
        }
        _ => unreachable!(),
    };
    let (r, e) = if exact {
        match query {
            0 => (m.r.clone(), m.e.clone()),
            2 | 4 => (vec![m.r[0].abs()], vec![m.e[0].abs()]),
            3 => {
                let c = |round: bool, ar: &[Iv]| -> Vec<Iv> {
                    let mut o = vec![ar[0].abs()];
                    for (c, &oc) in origin.iter().enumerate() {
                        let q = rnd(ar[2 + c].div(ar[0]), U, round);
                        o.push(rnd(Iv::p(oc).add(q), U, round));
                    }
                    o
                };
                (c(false, &m.r), c(true, &m.e))
            }
            _ => unreachable!(),
        }
    } else {
        (Vec::new(), Vec::new())
    };
    RowReplay {
        float,
        r,
        e,
        tapes: vec![m.tape],
        unsettled: Vec::new(),
    }
}

/// A `boolean.api_brep` areas row (`more0138`, a rotational sweep) replayed: the swept
/// B-rep's face areas as the runner takes them (`face_area` of each face in order, at
/// `AREA_ACCURACY`, then sorted), with every face whose area is `v[i]` at a `moved` index
/// replayed ([`face_replay`]) in its place in that order; the others the kernel's.
fn api_row_replay(case: &goldens::Case, moved: &[usize], v: &[f64], exact: bool) -> RowReplay {
    use goldens::units::boolean_api_brep::rotational_sweep_scene;
    use solidcore::mass::integrator::properties::face_area;
    use solidcore::topology::face::p2::PPF_AREA;
    const AREA_ACCURACY: f64 = 1e-4;
    // which faces: the kernel's areas in the runner's order
    let (k, mut b) = rotational_sweep_scene(case)
        .unwrap()
        .expect("a swept B-rep");
    let faces = solidcore::topology::brep::p1::faces(&b, None).unwrap();
    let kernel: Vec<f64> = faces
        .iter()
        .map(|&f| face_area(&k, &mut b, f, AREA_ACCURACY).unwrap())
        .collect();
    let target: Vec<usize> = (0..faces.len())
        .filter(|&j| moved.iter().any(|&i| kernel[j].to_bits() == v[i].to_bits()))
        .collect();
    assert_eq!(
        target.len(),
        moved.len(),
        "{}: the moved areas' faces",
        case.name
    );
    // again, the moved faces replayed
    let (k, mut b) = rotational_sweep_scene(case)
        .unwrap()
        .expect("a swept B-rep");
    let faces = solidcore::topology::brep::p1::faces(&b, None).unwrap();
    let mut rows: Vec<(f64, Iv, Iv)> = Vec::new();
    let mut tapes = Vec::new();
    let mut unsettled = Vec::new();
    for (j, &f) in faces.iter().enumerate() {
        if target.contains(&j) {
            let (m, _) = face_replay(
                &k,
                &mut b,
                f,
                true,
                None,
                PPF_AREA,
                AREA_ACCURACY,
                exact,
                true,
            );
            unsettled.push(m.unsettled);
            let a = m.float[0].abs();
            let (r, e) = if exact {
                (m.r[0].abs(), m.e[0].abs())
            } else {
                (Iv::p(a), Iv::p(a))
            };
            rows.push((a, r, e));
            tapes.push(m.tape);
        } else {
            let a = face_area(&k, &mut b, f, AREA_ACCURACY).unwrap();
            rows.push((a, Iv::p(a), Iv::p(a)));
        }
    }
    rows.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut float = vec![faces.len() as f64];
    let mut r = vec![Iv::p(float[0])];
    let mut e = r.clone();
    for (a, ra, ea) in rows {
        float.push(a);
        r.push(ra);
        e.push(ea);
    }
    if !exact {
        r.clear();
        e.clear();
    }
    RowReplay {
        float,
        r,
        e,
        tapes,
        unsettled,
    }
}

/// The partition is shared: every replayed face's decision tape equals main's (`T`, from the
/// same replay run against main's kernel, whose floats reproduced main's row `M` bit for
/// bit), decision for decision. `R` is then the exact quadrature of that one partition.
fn assert_same_partition(name: &str, new: &[Vec<bool>], main: &[Vec<bool>]) {
    assert_eq!(new.len(), main.len(), "{name}: the replayed faces");
    for (j, (n, m)) in new.iter().zip(main).enumerate() {
        assert!(
            n == m,
            "{name}: face {j}'s partition ({} decisions) is not main's ({} decisions)",
            n.len(),
            m.len()
        );
    }
}

/// The certificate of a moved `mass.integrator` face row: the integrator's partition replayed
/// in floats, bit for bit the row's vector, its decision tape main's (`T`), and in intervals
/// over that shared partition (the exact trims, the domain's sides: the reference `R`, the
/// exact quadrature of the partition; and the enclosure `E` of the kernel's float result).
fn label_mass(
    case: &goldens::Case,
    v: &[f64],
    main: &[f64],
    tapes: &[Vec<bool>],
    out: &mut BTreeMap<usize, Cert>,
) {
    let rep = mass_row_replay(case, true);
    assert_same_partition(&case.name, &rep.tapes, tapes);
    label_replayed(&case.name, &rep.float, &rep.r, &rep.e, v, main, out);
}

/// The certificate of a moved `boolean.api_brep` areas row ([`api_row_replay`]). Its new
/// partition is not main's (`T`; printed per face), so `R` does not rest on a shared
/// partition: the exact pass checks every decision of the new run against the kernel's
/// rule in exact arithmetic and takes the hull of both branches wherever the exact rule
/// could accept a piece the float run split ([`kr_settle`]); an accepted piece the exact
/// rule could split fails the test. `R` then holds the exact pipeline's value of each
/// replayed face's quadrature.
fn label_api_areas(
    case: &goldens::Case,
    v: &[f64],
    main: &[f64],
    tapes: &[Vec<bool>],
    out: &mut BTreeMap<usize, Cert>,
) {
    let moved: Vec<usize> = (1..v.len())
        .filter(|&i| v[i].to_bits() != main[i].to_bits())
        .collect();
    let rep = api_row_replay(case, &moved, v, true);
    assert_eq!(
        rep.tapes.len(),
        tapes.len(),
        "{}: the replayed faces",
        case.name
    );
    for (j, ((n, m), u)) in rep.tapes.iter().zip(tapes).zip(&rep.unsettled).enumerate() {
        eprintln!(
            "settled {}: face {j}: {} decisions (main {}, {}), {u} split piece(s) hulled",
            case.name,
            n.len(),
            m.len(),
            if n == m {
                "the same"
            } else {
                "a different partition"
            }
        );
    }
    label_replayed(&case.name, &rep.float, &rep.r, &rep.e, v, main, out);
}

/// `|new − r| / B` of an at-rounding element.
fn ratio(new: f64, c: &Cert) -> f64 {
    match c {
        Cert::Exact { r, b: Some(b), .. } => Iv::p(new).sub(Iv::p(*r)).abs().hi / b,
        Cert::Enc { r, e: Some(e), .. } => {
            let d = Iv::p(new).sub(*r).abs().hi;
            let b = e.sub(*r).abs().hi;
            d / b
        }
        _ => f64::NAN,
    }
}

#[test]
fn moved_elements_are_closer_equal_or_at_rounding() {
    let d = data();
    let ctx = CaseContext {
        root: repo_root(),
        seeds: 16,
    };
    let filter = std::env::var("ISSUE_18325_FILTER").ok();
    let mut bad = Vec::new();
    let mut pins = Vec::new();
    let mut tally: BTreeMap<(String, &'static str, Class), (usize, f64)> = BTreeMap::new();
    let mut rows: BTreeMap<(String, Class), usize> = BTreeMap::new();
    let mut seen = 0usize;
    let mut stale = 0usize;
    for runner in registry() {
        if !UNITS.contains(&runner.id()) {
            continue;
        }
        let unit = runner.id().to_owned();
        let cases: BTreeMap<String, goldens::Case> = runner
            .cases(&ctx)
            .unwrap()
            .into_iter()
            .map(|c| (c.name.clone(), c))
            .collect();
        for ((u, name), els) in d.els.range((unit.clone(), String::new())..) {
            if *u != unit {
                break;
            }
            if filter.as_ref().is_some_and(|x| !name.contains(x.as_str())) {
                continue;
            }
            let case = cases
                .get(name)
                .unwrap_or_else(|| panic!("{unit}: no case {name}"));
            let out = runner.run_rust(case).unwrap();
            let key = &els[0].key;
            assert!(els.iter().all(|e| e.key == *key), "{name}: one key per row");
            let v = &out.values[key];
            let status = out
                .ints
                .get("status")
                .and_then(|s| s.first().copied())
                .unwrap_or(0);
            // completeness: main's whole vector (`M`) is the committed main golden's (`O`,
            // re-encoded as `golden::encode` does), and the replay differs from it at the
            // listed elements only
            let (mkey, main) = &d.mains[&(unit.clone(), name.clone())];
            assert_eq!(mkey, key, "{name}: M key");
            let mine = Output {
                values: BTreeMap::from([(key.clone(), main.clone())]),
                ..Output::default()
            };
            let enc = encode(name, &mine)["values"][key].clone();
            let old = &d.olds[&(unit.clone(), name.clone())];
            if enc != *old {
                // A committed golden older than main's output and within the goldens'
                // tolerance of it (a bless keeps a passing value): main's output passes
                // main's golden, the check CI runs on main, and no tighter tie exists.
                assert!(
                    STALE_GOLDEN_UNITS.contains(&unit.as_str()),
                    "{unit}/{name}: M is not main's committed golden"
                );
                let wrap = |x: &Value| serde_json::json!({ "values": { key.as_str(): x } });
                assert!(
                    goldens::golden::diff(&wrap(old), &wrap(&enc), runner.abs_floors()).is_none(),
                    "{unit}/{name}: M does not pass main's committed golden"
                );
                stale += 1;
            }
            assert_eq!(v.len(), main.len(), "{unit}/{name}: the vector's length");
            let listed: BTreeMap<usize, &El> = els.iter().map(|e| (e.i, e)).collect();
            for (i, (x, m)) in v.iter().zip(main).enumerate() {
                match listed.get(&i) {
                    Some(e) => assert_eq!(
                        m.to_bits(),
                        e.main.to_bits(),
                        "{unit}/{name} {key}[{i}]: E's main is not M's"
                    ),
                    None => {
                        if x.to_bits() != m.to_bits() {
                            bad.push(format!(
                                "{unit}/{name} {key}[{i}]: unlisted, moved {m:?} -> {x:?}"
                            ));
                        }
                    }
                }
            }
            let certs = certificates(&unit, case, status, v, main, &d);
            let mut worst = Class::Equal;
            for e in els {
                seen += 1;
                let got = v[e.i];
                if got.to_bits() != e.new.to_bits() {
                    bad.push(format!(
                        "{unit}/{name} {key}[{}]: replays {got:?}, the golden holds {:?}",
                        e.i, e.new
                    ));
                }
                let Some(c) = certs.get(&e.i) else {
                    bad.push(format!("{unit}/{name} {key}[{}]: no certificate", e.i));
                    continue;
                };
                let class = classify(e.new, e.main, c);
                let slot = tally
                    .entry((unit.clone(), c.what(), class))
                    .or_insert((0, 0.0));
                slot.0 += 1;
                if class == Class::AtRounding {
                    slot.1 = slot.1.max(ratio(e.new, c));
                }
                worst = match (worst, class) {
                    (_, Class::Farther) | (Class::Farther, _) => Class::Farther,
                    (_, Class::Measured) | (Class::Measured, _) => Class::Measured,
                    (_, Class::AtRounding) | (Class::AtRounding, _) => Class::AtRounding,
                    (_, Class::Closer) | (Class::Closer, _) => Class::Closer,
                    _ => Class::Equal,
                };
                let name_of = format!("{class:?}");
                if e.want == "?" {
                    pins.push(format!("{unit}\t{name}\t{key}\t{}\t{name_of}", e.i));
                } else if e.want != name_of {
                    bad.push(format!(
                        "{unit}/{name} {key}[{}] {:?} -> {:?}: {name_of}, pinned {} ({c:?})",
                        e.i, e.main, e.new, e.want
                    ));
                }
                if class == Class::Farther {
                    bad.push(format!(
                        "{unit}/{name} {key}[{}] {:?} -> {:?}: FARTHER ({c:?})",
                        e.i, e.main, e.new
                    ));
                }
            }
            *rows.entry((unit.clone(), worst)).or_default() += 1;
        }
    }
    eprintln!("element classes (unit, kind, class: count, max |new − r| / B):");
    for ((u, what, class), (n, r)) in &tally {
        if *class == Class::AtRounding {
            eprintln!("  {u}\t{what}\t{class:?}\t{n}\t{r:.3}");
        } else {
            eprintln!("  {u}\t{what}\t{class:?}\t{n}");
        }
    }
    eprintln!("row classes (the worst element's):");
    for ((u, class), n) in &rows {
        eprintln!("  {u}\t{class:?}\t{n}");
    }
    if let Ok(path) = std::env::var("ISSUE_18325_PINS") {
        std::fs::write(path, pins.join("\n") + "\n").unwrap();
    }
    if filter.is_none() {
        eprintln!("main goldens older than main's output (passing): {stale} rows");
        assert_eq!(seen, d.els.values().map(Vec::len).sum::<usize>());
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    assert!(
        pins.is_empty(),
        "{} elements without a pinned class",
        pins.len()
    );
}
