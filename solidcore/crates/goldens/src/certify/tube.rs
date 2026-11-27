//! Certificates for the curves of a surface/surface intersection (tests only): a tube
//! through the curves of an answer that run into one another holds one component of the
//! true intersection, the surfaces cross along it, and disjoint tubes hold different
//! components. A curve counts as in a tube when the zeros refined from its points are
//! (the tube proves the component, not how near the answer's curve lies to it).
//!
//! The unknowns are the parameters of each surface the kernel evaluates parametrically:
//! a rational B-spline net (every kind it evaluates through its NURB) or a plane's affine
//! map, enclosed over a box in rounded intervals ([`Ivl`], every operation rounding
//! outward). A cone or cylinder, which the kernel evaluates in its own analytic form,
//! enters by its implicit equation instead, a polynomial in the point: with the frame
//! `M = [X Y Z]` as stored, `a = adj(M) (x - o)` and `D = det(M)`,
//! `F = a_x^2 + a_y^2 - (D r0 + tan(angle) a_z)^2` and the height `a_z / D` (the
//! tangent within 1e-15 relative of the C library's, ARGUED as in `tay::wide`). The
//! equations are `E = S1 - S2` (3 in 4 unknowns) or `E = F(S)` (1 in 2).
//!
//! A tube is a chain of boxes `T x X` along one curve: `t` one unknown, `x` the others.
//! In each box the parametric Krawczyk test, `K = m - Y E(T, m) + (I - Y E_x(T, X)) (X -
//! m)` strictly inside `X`, proves for every `t` in `T` exactly one zero `x(t)` in `X`,
//! and every matrix of `E_x` over the box invertible. The zeros form an arc (a graph with
//! a closed graph in a compact box is continuous) along which the surfaces cross (`E_x`
//! of full rank: the tangent planes span space). Consecutive boxes share the arc: a
//! box's arc point at a zero it was made over, enclosed by the same test with `T` a
//! point, lies in the next box ([`joined`]), and the arc point at each end of every
//! box's `T` lies strictly inside another box of the tube ([`covered`]). A tube closes
//! (its last box joins its first) or ends where its arc leaves the domains: the end box's
//! outer arc point lies past them, its inner one inside. A box may reach past the domains
//! only in coordinates that are monotone along its arc (an unknown with a signed slope,
//! a cone's height with `dh/dt` of one sign), so the arc's part inside is one piece; the
//! arc points at both ends of every other box lie inside. A cone's seam (`a_y = 0`,
//! `a_x > 0`, the domain's u ends) is a domain end like any other. So inside a tube the
//! zero set is one arc, open and closed in the zero set within the domains: a component.
//! Tubes disjoint pairwise hold distinct components. An [`Exit`] names the domain end an
//! open tube's ends lie on as the answer's curve ends there; the proof is only that the
//! arc leaves the domains.
//!
//! The intersector splits a curve where a net is not smooth (a knot line of a degree-1
//! profile); curves that share an end are one chain and get one tube, which crosses the
//! knot line with two boxes, each on its side's polynomial continued ([`across`]).
//!
//! [`no_zero`] proves that a box about a point holds no zero (a point an old answer
//! returned), and [`on_tube`] that a point lies in a tube's box, where the only zero is
//! the tube's.

use solidcore::blend::fillet_section::interval::Ivl;
use solidcore::blend::fillet_section::station::as_b_spline;
use solidcore::blend::fillet_section::tay::wide;
use solidcore::curve::curve::Curve;
use solidcore::geom::extent2d::Extent2d;
use solidcore::kernel::Kernel;
use solidcore::math::Point2;
use solidcore::nurbs::b_spline_surface::p1::SurfParamType;
use solidcore::nurbs::math::NOW;
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::surface::surface::Surface;

type V3 = [Ivl; 3];

fn pt(x: f64) -> Ivl {
    Ivl::from(x)
}

fn v3(p: [f64; 3]) -> V3 {
    [pt(p[0]), pt(p[1]), pt(p[2])]
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(s: Ivl, a: V3) -> V3 {
    [s * a[0], s * a[1], s * a[2]]
}

fn dot(a: V3, b: V3) -> Ivl {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn mid3(a: V3) -> [f64; 3] {
    [a[0].mid(), a[1].mid(), a[2].mid()]
}

fn inside(a: Ivl, b: Ivl) -> bool {
    a.lo > b.lo && a.hi < b.hi
}

fn within(a: Ivl, b: Ivl) -> bool {
    a.lo >= b.lo && a.hi <= b.hi
}

/// The strict sign of `a`, `None` where it holds 0.
fn sign(a: Ivl) -> Option<i32> {
    if a.lo > 0.0 {
        Some(1)
    } else if a.hi < 0.0 {
        Some(-1)
    } else {
        None
    }
}

/// A surface's point and first partials over a box.
#[derive(Clone, Copy, Debug)]
struct Jet {
    s: V3,
    su: V3,
    sv: V3,
}

fn hull(a: Jet, b: Jet) -> Jet {
    let h = |x: V3, y: V3| [x[0].hull(y[0]), x[1].hull(y[1]), x[2].hull(y[2])];
    Jet {
        s: h(a.s, b.s),
        su: h(a.su, b.su),
        sv: h(a.sv, b.sv),
    }
}

/// A rational B-spline net: degrees, control point counts, full knots, homogeneous
/// points `(x w, y w, z w, w)` row by row (u-major).
#[derive(Debug)]
struct Net {
    p: usize,
    q: usize,
    nu: usize,
    nv: usize,
    u: Vec<f64>,
    v: Vec<f64>,
    pw: Vec<[f64; 4]>,
    /// A net of degree 1 with two rows in one direction, as it is evaluated (see
    /// [`Ruled`]).
    ruled: Option<Ruled>,
}

/// A net of degree 1 with two control rows `P0`, `P1` in direction `dir` (an extrusion's,
/// its rows at `C -+ L d`, `L` up to ~1e9): its points `M + mu D` with `M = (P0 + P1) / 2`,
/// `D = (P1 - P0) / 2` per control point and `mu = (2 t - t0 - t1) / (t1 - t0)` along the
/// rows. The same rational function as the net's; summed this way the rows' cancellation
/// happens once per control point, where it is exact (Sterbenz: `P0 + P1` of nearly
/// opposite numbers), instead of in every point (a 1e-8 to 1e-5 wide enclosure).
#[derive(Debug)]
struct Ruled {
    dir: usize,
    t: [f64; 2],
    mid: Vec<[Ivl; 4]>,
    half: Vec<[Ivl; 4]>,
}

fn full_knots(knots: &[f64], mult: &[usize]) -> Vec<f64> {
    knots
        .iter()
        .zip(mult)
        .flat_map(|(&k, &m)| std::iter::repeat_n(k, m))
        .collect()
}

/// The span `k` (`U[k] <= x < U[k + 1]`, `p <= k < n`) of `x`, the end spans continued.
fn span(u: &[f64], p: usize, n: usize, x: f64) -> usize {
    let mut k = p;
    while k + 1 < n && x >= u[k + 1] {
        k += 1;
    }
    k
}

/// `[a.lo, a.hi]` cut at the knots strictly inside it, each part with its span.
fn pieces(u: &[f64], p: usize, n: usize, a: Ivl) -> Vec<(Ivl, usize)> {
    let mut cuts = vec![a.lo];
    for &x in &u[p + 1..n] {
        if x > a.lo && x < a.hi && cuts.last() != Some(&x) {
            cuts.push(x);
        }
    }
    cuts.push(a.hi);
    cuts.windows(2)
        .map(|w| (Ivl::new(w[0], w[1]), span(u, p, n, 0.5 * (w[0] + w[1]))))
        .collect()
}

/// `x^k` over `x`, a box's offset from its middle (holding 0): within `+-|x|^k` (an even
/// power not below 0), the bound an outward-rounded product and kept above the subnormal
/// range, where the products' enclosures would give up.
fn ipow(x: Ivl, k: usize) -> Ivl {
    if k == 0 {
        return pt(1.0);
    }
    let m = x.mag();
    if m == 0.0 {
        return pt(0.0);
    }
    let b = (0..k)
        .fold(pt(1.0), |p, _| p * pt(m))
        .hi
        .max(f64::MIN_POSITIVE);
    if k.is_multiple_of(2) {
        Ivl::new(0.0, b)
    } else {
        Ivl::new(-b, b)
    }
}

/// The `p + 1` basis functions of span `k` as polynomials in `d = x - c` (coefficients of
/// `d^0 .. d^p`, enclosed): Cox-de Boor on polynomials. Summing a net against these at a
/// point, then evaluating over a box, keeps the nets' cancellation exact where a direct
/// enclosure over the box loses it (the basis functions' widths times the control
/// points).
fn taylor_basis(u: &[f64], p: usize, k: usize, c: f64) -> Vec<Vec<Ivl>> {
    let mut n: Vec<Vec<Ivl>> = vec![vec![pt(1.0)]];
    for j in 1..=p {
        let mut next = vec![vec![pt(0.0); j + 1]; j + 1];
        for (r, nr) in n.iter().enumerate() {
            let i = k + 1 + r - j;
            let (a, b) = (u[i], u[i + j]);
            if b > a {
                let inv = pt(1.0) / (pt(b) - pt(a));
                for (e, &coef) in nr.iter().enumerate() {
                    let w = coef * inv;
                    // (b - x) w = (b - c) w - d w;  (x - a) w = (c - a) w + d w
                    next[r][e] = next[r][e] + (pt(b) - pt(c)) * w;
                    next[r][e + 1] = next[r][e + 1] - w;
                    next[r + 1][e] = next[r + 1][e] + (pt(c) - pt(a)) * w;
                    next[r + 1][e + 1] = next[r + 1][e + 1] + w;
                }
            }
        }
        n = next;
    }
    n
}

/// The rational point and partials from homogeneous `a` and its partials.
fn rational(a: [Ivl; 4], au: [Ivl; 4], av: [Ivl; 4]) -> Jet {
    let w = a[3];
    let s = [a[0] / w, a[1] / w, a[2] / w];
    let part = |d: [Ivl; 4]| {
        [
            (d[0] - s[0] * d[3]) / w,
            (d[1] - s[1] * d[3]) / w,
            (d[2] - s[2] * d[3]) / w,
        ]
    };
    Jet {
        s,
        su: part(au),
        sv: part(av),
    }
}

impl Net {
    /// The net of `s`'s NURB form, as the kernel evaluates `s`.
    ///
    /// # Panics
    ///
    /// When `s` has no NURB form or a weight is not positive.
    fn of(k: &Kernel, s: &Surface) -> Self {
        let b = as_b_spline(k, s)
            .expect("a NURB form")
            .expect("a surface with a NURB");
        let (ku, mu) = b.knots(SurfParamType::U, None).expect("u knots");
        let (kv, mv) = b.knots(SurfParamType::V, None).expect("v knots");
        let (nu, nv, cps) = b.control_points().expect("control points");
        let pw = cps
            .iter()
            .map(|c| {
                let w = if c.w == NOW { 1.0 } else { c.w };
                assert!(w > 0.0, "a weight {w}");
                [c.x, c.y, c.z, w]
            })
            .collect::<Vec<_>>();
        let mut net = Self {
            p: b.degree(SurfParamType::U),
            q: b.degree(SurfParamType::V),
            nu,
            nv,
            u: full_knots(&ku, &mu),
            v: full_knots(&kv, &mv),
            pw,
            ruled: None,
        };
        net.ruled = net.ruled();
        assert_eq!(net.u.len(), nu + net.p + 1);
        assert_eq!(net.v.len(), nv + net.q + 1);
        assert_eq!(net.pw.len(), nu * nv);
        net
    }

    fn ruled(&self) -> Option<Ruled> {
        let (dir, rows, t) = if self.q == 1 && self.nv == 2 {
            let rows = (0..self.nu)
                .map(|i| (self.pw[i * 2], self.pw[i * 2 + 1]))
                .collect::<Vec<_>>();
            (1, rows, [self.v[1], self.v[2]])
        } else if self.p == 1 && self.nu == 2 {
            let rows = (0..self.nv)
                .map(|j| (self.pw[j], self.pw[self.nv + j]))
                .collect();
            (0, rows, [self.u[1], self.u[2]])
        } else {
            return None;
        };
        let half = pt(0.5);
        let mid = rows
            .iter()
            .map(|(a, b)| [0, 1, 2, 3].map(|l| half * (pt(a[l]) + pt(b[l]))))
            .collect();
        let diff = rows
            .iter()
            .map(|(a, b)| [0, 1, 2, 3].map(|l| half * (pt(b[l]) - pt(a[l]))))
            .collect();
        Some(Ruled {
            dir,
            t,
            mid,
            half: diff,
        })
    }

    /// [`Self::jet`] of a ruled net ([`Ruled`]), in Taylor form along the curve about the
    /// box's middle: `A = sum_k (M_k + mu_c H_k) d^k + mu' dl sum_k H_k d^k`.
    fn ruled_jet(&self, r: &Ruled, a: Ivl, b: Ivl, force: [Option<usize>; 2]) -> Jet {
        // `c` runs along the curve (the other direction), `l` across the rows
        let (c, l, kc, pc, nc, knots) = if r.dir == 1 {
            (a, b, force[0], self.p, self.nu, &self.u)
        } else {
            (b, a, force[1], self.q, self.nv, &self.v)
        };
        let width = pt(r.t[1]) - pt(r.t[0]);
        let lc = l.mid();
        let mu_c = (pt(2.0) * pt(lc) - pt(r.t[0]) - pt(r.t[1])) / width;
        let dmu = pt(2.0) / width;
        let dl = if l.lo == l.hi { pt(0.0) } else { l - pt(lc) };
        let on = kc.map_or_else(|| pieces(knots, pc, nc, c), |k| vec![(c, k)]);
        let mut out: Option<Jet> = None;
        for (x, k) in on {
            let cc = x.mid();
            let tb = taylor_basis(knots, pc, k, cc);
            let d = if x.lo == x.hi { pt(0.0) } else { x - pt(cc) };
            let pd: Vec<Ivl> = (0..=pc).map(|e| ipow(d, e)).collect();
            let mut a0 = [pt(0.0); 4];
            let mut ac = [pt(0.0); 4];
            let mut al = [pt(0.0); 4];
            for e in 0..=pc {
                for q in 0..4 {
                    let (mut m, mut h) = (pt(0.0), pt(0.0));
                    for (i, t) in tb.iter().enumerate() {
                        m = m + r.mid[k - pc + i][q] * t[e];
                        h = h + r.half[k - pc + i][q] * t[e];
                    }
                    let ce = m + mu_c * h;
                    a0[q] = a0[q] + (ce + dmu * dl * h) * pd[e];
                    al[q] = al[q] + dmu * h * pd[e];
                    if e > 0 {
                        ac[q] = ac[q] + pt(e as f64) * (ce + dmu * dl * h) * pd[e - 1];
                    }
                }
            }
            let jt = rational(a0, ac, al);
            let j = if r.dir == 1 {
                jt
            } else {
                Jet {
                    s: jt.s,
                    su: jt.sv,
                    sv: jt.su,
                }
            };
            out = Some(out.map_or(j, |o| hull(o, j)));
        }
        out.expect("a box")
    }

    /// The jet over `a x b`: the hull over the polynomial pieces the box lies on, or in a
    /// direction with a forced span, that span's polynomial continued over the whole box.
    fn jet(&self, a: Ivl, b: Ivl, force: [Option<usize>; 2]) -> Jet {
        if let Some(r) = &self.ruled {
            return self.ruled_jet(r, a, b, force);
        }
        let on = |u: &[f64], p: usize, n: usize, x: Ivl, f: Option<usize>| {
            f.map_or_else(|| pieces(u, p, n, x), |k| vec![(x, k)])
        };
        let mut out: Option<Jet> = None;
        for &(pa, ku) in &on(&self.u, self.p, self.nu, a, force[0]) {
            for &(pb, kv) in &on(&self.v, self.q, self.nv, b, force[1]) {
                let j = self.piece(ku, kv, pa, pb);
                out = Some(out.map_or(j, |h| hull(h, j)));
            }
        }
        out.expect("a box")
    }

    /// One polynomial piece's jet over `a x b`, in Taylor form about the box's middle.
    fn piece(&self, ku: usize, kv: usize, a: Ivl, b: Ivl) -> Jet {
        let (cu, cv) = (a.mid(), b.mid());
        let (tu, tv) = (
            taylor_basis(&self.u, self.p, ku, cu),
            taylor_basis(&self.v, self.q, kv, cv),
        );
        let off = |x: Ivl, c: f64| if x.lo == x.hi { pt(0.0) } else { x - pt(c) };
        let (du, dv) = (off(a, cu), off(b, cv));
        let pu: Vec<Ivl> = (0..=self.p).map(|e| ipow(du, e)).collect();
        let pv: Vec<Ivl> = (0..=self.q).map(|e| ipow(dv, e)).collect();
        let mut h = [[pt(0.0); 4]; 3];
        for e in 0..=self.p {
            for f in 0..=self.q {
                let mut sum = [pt(0.0); 4];
                for (i, ti) in tu.iter().enumerate() {
                    for (j, tj) in tv.iter().enumerate() {
                        let c = self.pw[(ku - self.p + i) * self.nv + kv - self.q + j];
                        for (a, &cq) in sum.iter_mut().zip(&c) {
                            *a = *a + pt(cq) * ti[e] * tj[f];
                        }
                    }
                }
                for (q, &al) in sum.iter().enumerate() {
                    h[0][q] = h[0][q] + al * pu[e] * pv[f];
                    if e > 0 {
                        h[1][q] = h[1][q] + pt(e as f64) * al * pu[e - 1] * pv[f];
                    }
                    if f > 0 {
                        h[2][q] = h[2][q] + pt(f as f64) * al * pu[e] * pv[f - 1];
                    }
                }
            }
        }
        rational(h[0], h[1], h[2])
    }
}

/// A cone or cylinder by its implicit equation (module docs).
#[derive(Debug)]
struct ConeForm {
    o: V3,
    rows: [V3; 3],
    det: Ivl,
    r0: Ivl,
    tan: Ivl,
    /// The heights of the domain: the inner and the outer enclosure of its ends.
    inner: Ivl,
    outer: Ivl,
}

impl ConeForm {
    /// The implicit form of `c` over the domain `d`.
    ///
    /// # Panics
    ///
    /// When `d`'s ends cannot be converted to the cone's own parameters, `d` is short of a
    /// full turn, its seam is not at angle 0 or the frame is not right-handed.
    fn of(k: &Kernel, c: &solidcore::surface::cone::Cone, d: &Extent2d) -> Self {
        let canonical = c.canonical();
        let f = &canonical.origin;
        let (o, x, y, z) = (f.origin(), f.x_axis(), f.y_axis(), f.z_axis());
        let [x, y, z] = [x, y, z].map(|a| v3([a.x, a.y, a.z]));
        let rows = [cross(y, z), cross(z, x), cross(x, y)];
        let t = canonical.semi_angle_deg.to_radians().tan();
        // the domain's ends in STEP parameters (degrees, height); each end within a few
        // roundings of its conversion, ARGUED
        let at = |uv: Point2| {
            c.convert_uv_from_nurbs_to_step(k, uv)
                .expect("a cone's STEP parameters")
        };
        let (a, b) = (at(d.min()), at(d.max()));
        let turn = (b.x - a.x).abs();
        assert!(
            turn >= 360.0 - 1e-9,
            "a cone's domain short of a full turn: {turn} degrees"
        );
        // the seam (the domain's u ends) at angle 0: the half-plane a_y = 0, a_x > 0
        assert!(
            a.x.min(b.x) == 0.0,
            "a cone's seam at {} degrees",
            a.x.min(b.x)
        );
        let (h0, h1) = (a.y.min(b.y), a.y.max(b.y));
        let e = |h: f64| 64.0 * f64::EPSILON * (1.0 + h.abs());
        let det = dot(x, rows[0]);
        // a right-handed frame: a_x, a_y keep the angle's signs
        assert!(det.lo > 0.0, "a cone's frame not right-handed");
        Self {
            o: v3([o.x, o.y, o.z]),
            rows,
            det,
            r0: pt(canonical.radius),
            tan: wide(t, t),
            inner: Ivl::new(h0 + e(h0), h1 - e(h1)),
            outer: Ivl::new(h0 - e(h0), h1 + e(h1)),
        }
    }

    /// `F` and its gradient at the points `x`.
    fn value_grad(&self, x: V3) -> (Ivl, V3) {
        let d = sub(x, self.o);
        let a = [
            dot(self.rows[0], d),
            dot(self.rows[1], d),
            dot(self.rows[2], d),
        ];
        let r = self.det * self.r0 + self.tan * a[2];
        let two = pt(2.0);
        let f = a[0] * a[0] + a[1] * a[1] - r * r;
        let g = add(
            add(
                scale(two * a[0], self.rows[0]),
                scale(two * a[1], self.rows[1]),
            ),
            scale(pt(-2.0) * r * self.tan, self.rows[2]),
        );
        (f, g)
    }

    fn height(&self, x: V3) -> Ivl {
        dot(self.rows[2], sub(x, self.o)) / self.det
    }

    fn height_grad(&self) -> V3 {
        scale(pt(1.0) / self.det, self.rows[2])
    }
}

/// A surface as the certificate sees it.
#[derive(Debug)]
enum Side {
    Net(Net),
    /// `o + u du + v dv` (the plane's `uv_scale` folded into its axes).
    Plane {
        o: V3,
        du: V3,
        dv: V3,
    },
    Cone(ConeForm),
}

impl Side {
    fn of(k: &Kernel, s: &Surface, d: &Extent2d) -> Self {
        match s.kind_with_nurb(k).expect("a surface kind") {
            SurfaceKind::Plane(p) => {
                let (f, sc) = (p.position(), p.uv_scale());
                let (o, x, y) = (f.origin(), f.x_axis(), f.y_axis());
                Self::Plane {
                    o: v3([o.x, o.y, o.z]),
                    du: scale(pt(sc.x), v3([x.x, x.y, x.z])),
                    dv: scale(pt(sc.y), v3([y.x, y.y, y.z])),
                }
            }
            SurfaceKind::Cone(c) => Self::Cone(ConeForm::of(k, c, d)),
            _ => Self::Net(Net::of(k, s)),
        }
    }

    fn jet(&self, a: Ivl, b: Ivl, force: [Option<usize>; 2]) -> Jet {
        match self {
            Self::Net(n) => n.jet(a, b, force),
            Self::Plane { o, du, dv } => Jet {
                s: add(*o, add(scale(a, *du), scale(b, *dv))),
                su: *du,
                sv: *dv,
            },
            Self::Cone(_) => unreachable!("a cone has no parameters here"),
        }
    }
}

/// The intersection's equations over the unknowns: both surfaces' parameters, or the
/// parametric one's where the other is a cone.
#[derive(Debug)]
pub(crate) struct System {
    sides: [Side; 2],
    /// The parametric surfaces (indices into `sides`), in the unknowns' order.
    param: Vec<usize>,
    /// The cone, if one is.
    cone: Option<usize>,
    /// Each unknown's domain.
    dom: Vec<Ivl>,
}

/// One evaluation over a box: the equations, their Jacobian (rows) and, with a cone,
/// its height and the height's partials.
struct Eval {
    e: Vec<Ivl>,
    j: Vec<Vec<Ivl>>,
    h: Option<(Ivl, Vec<Ivl>)>,
    /// With a cone: `a_y`, `a_x` (its frame's, times `det M`) and `a_y`'s partials; the
    /// seam is `a_y = 0, a_x > 0`.
    seam: Option<(Ivl, Ivl, Vec<Ivl>)>,
    jets: Vec<Jet>,
}

impl System {
    /// The equations of `s[0]` over `d[0]` against `s[1]` over `d[1]`.
    ///
    /// # Panics
    ///
    /// When both surfaces are cones, or a surface cannot be read (see [`Net::of`],
    /// [`ConeForm::of`]).
    pub(crate) fn new(k: &Kernel, s: [&Surface; 2], d: [&Extent2d; 2]) -> Self {
        let sides = [Side::of(k, s[0], d[0]), Side::of(k, s[1], d[1])];
        let cone = sides.iter().position(|x| matches!(x, Side::Cone(_)));
        let param: Vec<usize> = (0..2).filter(|&i| Some(i) != cone).collect();
        assert!(!param.is_empty(), "two cones");
        let mut dom = Vec::new();
        for &i in &param {
            let (u, v) = (d[i].u_interval(), d[i].v_interval());
            dom.push(Ivl::new(u.min(), u.max()));
            dom.push(Ivl::new(v.min(), v.max()));
        }
        Self {
            sides,
            param,
            cone,
            dom,
        }
    }

    fn eval(&self, y: &[Ivl]) -> Eval {
        self.eval_on(y, &[])
    }

    /// [`Self::eval`] with each unknown's net span forced where `force` names one.
    fn eval_on(&self, y: &[Ivl], force: &[Option<usize>]) -> Eval {
        let f = |i: usize| force.get(i).copied().flatten();
        let jets: Vec<Jet> = self
            .param
            .iter()
            .enumerate()
            .map(|(i, &s)| self.sides[s].jet(y[2 * i], y[2 * i + 1], [f(2 * i), f(2 * i + 1)]))
            .collect();
        if let Some(c) = self.cone() {
            let j = jets[0];
            let (v, g) = c.value_grad(j.s);
            let hg = c.height_grad();
            let d = sub(j.s, c.o);
            let ry = c.rows[1];
            return Eval {
                e: vec![v],
                j: vec![vec![dot(g, j.su), dot(g, j.sv)]],
                h: Some((c.height(j.s), vec![dot(hg, j.su), dot(hg, j.sv)])),
                seam: Some((
                    dot(ry, d),
                    dot(c.rows[0], d),
                    vec![dot(ry, j.su), dot(ry, j.sv)],
                )),
                jets,
            };
        }
        let (a, b) = (jets[0], jets[1]);
        let e = sub(a.s, b.s).to_vec();
        let j = (0..3)
            .map(|r| vec![a.su[r], a.sv[r], pt(0.0) - b.su[r], pt(0.0) - b.sv[r]])
            .collect();
        Eval {
            e,
            j,
            h: None,
            seam: None,
            jets,
        }
    }

    fn eval_at(&self, y: &[f64]) -> Eval {
        self.eval(&y.iter().map(|&x| pt(x)).collect::<Vec<_>>())
    }

    fn cone(&self) -> Option<&ConeForm> {
        self.cone.and_then(|c| match &self.sides[c] {
            Side::Cone(f) => Some(f),
            _ => None,
        })
    }

    /// The 3D speed of each unknown at a point: `|dS/dy_i|`.
    fn speeds(ev: &Eval) -> Vec<f64> {
        let n = |a: V3| {
            let m = mid3(a);
            (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt()
        };
        ev.jets.iter().flat_map(|j| [n(j.su), n(j.sv)]).collect()
    }
}

/// The inverse of the `n x n` matrix `a` (rows), `None` where singular.
fn inverse(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let n = a.len();
    let mut m: Vec<Vec<f64>> = a
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut r = r.clone();
            r.extend((0..n).map(|j| f64::from(u8::from(i == j))));
            r
        })
        .collect();
    for c in 0..n {
        let p = (c..n).max_by(|&i, &j| m[i][c].abs().total_cmp(&m[j][c].abs()))?;
        if m[p][c] == 0.0 || !m[p][c].is_finite() {
            return None;
        }
        m.swap(c, p);
        let d = m[c][c];
        for x in &mut m[c] {
            *x /= d;
        }
        let pivot = m[c].clone();
        for (r, row) in m.iter_mut().enumerate() {
            if r != c {
                let f = row[c];
                for (x, &pv) in row.iter_mut().zip(&pivot) {
                    *x -= f * pv;
                }
            }
        }
    }
    Some(m.into_iter().map(|r| r[n..].to_vec()).collect())
}

/// The tangent of the zero set at `y`: the Jacobian's null vector (signed minors).
fn tangent(j: &[Vec<Ivl>]) -> Vec<f64> {
    let jm: Vec<Vec<f64>> = j
        .iter()
        .map(|r| r.iter().map(|x| x.mid()).collect())
        .collect();
    if jm.len() == 1 {
        return vec![-jm[0][1], jm[0][0]];
    }
    (0..4)
        .map(|i| {
            let c: Vec<usize> = (0..4).filter(|&c| c != i).collect();
            let m = |r: usize, k: usize| jm[r][c[k]];
            let det = m(0, 0) * (m(1, 1) * m(2, 2) - m(1, 2) * m(2, 1))
                - m(0, 1) * (m(1, 0) * m(2, 2) - m(1, 2) * m(2, 0))
                + m(0, 2) * (m(1, 0) * m(2, 1) - m(1, 1) * m(2, 0));
            if i.is_multiple_of(2) {
                det
            } else {
                -det
            }
        })
        .collect()
}

/// The unknowns by their share of the tangent's 3D speed at `y`, largest first: the
/// first is the one the zero set is best a graph over.
fn by_share(sys: &System, y: &[f64]) -> Vec<usize> {
    let ev = sys.eval_at(y);
    let tg = tangent(&ev.j);
    let sp = System::speeds(&ev);
    let mut o: Vec<usize> = (0..tg.len()).collect();
    o.sort_by(|&x, &y| (tg[y].abs() * sp[y]).total_cmp(&(tg[x].abs() * sp[x])));
    o
}

/// The unknown the zero set is best a graph over at `y` ([`by_share`]).
fn choose_t(sys: &System, y: &[f64]) -> usize {
    by_share(sys, y).first().copied().unwrap_or(0)
}

/// One more equation for [`newton`]: a cone's height, or its `a_y` (the seam), at a value.
#[derive(Clone, Copy, Debug)]
enum Target {
    Height(f64),
    Seam(f64),
}

/// Newton on `E = 0` with unknown `ti` held at `y[ti]` (and with a `target`, one more
/// equation); `None` where it does not converge. The zeros it finds are guesses only:
/// the boxes prove.
fn newton(sys: &System, y: &[f64], ti: Option<usize>, target: Option<Target>) -> Option<Vec<f64>> {
    let mut y = y.to_vec();
    let mut last = f64::INFINITY;
    for _ in 0..60 {
        let ev = sys.eval_at(&y);
        let mut rows: Vec<Vec<f64>> =
            ev.j.iter()
                .map(|r| r.iter().map(|x| x.mid()).collect())
                .collect();
        let mut rhs: Vec<f64> = ev.e.iter().map(|x| x.mid()).collect();
        match (target, &ev.h, &ev.seam) {
            (Some(Target::Height(v)), Some((h, hg)), _) => {
                rows.push(hg.iter().map(|x| x.mid()).collect());
                rhs.push(h.mid() - v);
            }
            (Some(Target::Seam(v)), _, Some((ay, _, g))) => {
                rows.push(g.iter().map(|x| x.mid()).collect());
                rhs.push(ay.mid() - v);
            }
            _ => {}
        }
        let free: Vec<usize> = (0..y.len()).filter(|&i| Some(i) != ti).collect();
        let a: Vec<Vec<f64>> = rows
            .iter()
            .map(|r| free.iter().map(|&c| r[c]).collect())
            .collect();
        if a.len() != free.len() {
            return None;
        }
        let inv = inverse(&a)?;
        let mut step = 0.0f64;
        for (r, &c) in free.iter().enumerate() {
            let d: f64 = inv[r].iter().zip(&rhs).map(|(a, b)| a * b).sum();
            y[c] -= d;
            step = step.max(d.abs() / (1.0 + y[c].abs()));
        }
        if !y.iter().all(|x| x.is_finite()) {
            return None;
        }
        // converged: the step at rounding, or no longer shrinking (a net far from the
        // origin, an extrusion's over an unbounded domain, is evaluated to ~1e-8 only)
        if step < 1e-15 || (step < 1e-7 && step > 0.25 * last) {
            return Some(y);
        }
        last = step;
    }
    (last < 1e-7).then_some(y)
}

/// One certified box: unknown `ti` over `t`, the others over `x`.
#[derive(Clone, Debug)]
struct KBox {
    ti: usize,
    t: Ivl,
    x: Vec<Ivl>,
    /// The direction the tube runs in `t` (+1 or -1).
    dir: f64,
    /// Each unknown's net span, where the box takes one piece continued (across a knot
    /// line, [`across`]).
    force: Vec<Option<usize>>,
    /// `t` at the zeros the box was made over (its arc's points there are shared with
    /// the neighbours).
    at: [f64; 2],
    /// The ends of `T` on a knot line ([`across`]): the arc crosses there into the other
    /// box of the pair, no other box need hold them.
    on_face: [bool; 2],
    /// An end box through a cone's seam: the sign of `a_y` on its inner side (its arc's
    /// part inside the domain lies there).
    seam_side: Option<i32>,
    /// With a cone: the sign of `a_y` along the box's arc, where it has one.
    ay_sign: Option<i32>,
}

impl KBox {
    fn full(&self) -> Vec<Ivl> {
        let mut y = self.x.clone();
        y.insert(self.ti, self.t);
        y
    }
}

/// The parametric Krawczyk test on `t x x`: the enclosure `K`, strictly inside `x`, or
/// `None`.
fn krawczyk(
    sys: &System,
    ti: usize,
    t: Ivl,
    x: &[Ivl],
    force: &[Option<usize>],
) -> Option<Vec<Ivl>> {
    let k = krawczyk_k(sys, ti, t, x, force)?;
    k.iter().zip(x).all(|(&a, &b)| inside(a, b)).then_some(k)
}

/// Krawczyk's operator on `t x x` (not checked against `x`).
fn krawczyk_k(
    sys: &System,
    ti: usize,
    t: Ivl,
    x: &[Ivl],
    force: &[Option<usize>],
) -> Option<Vec<Ivl>> {
    let m: Vec<f64> = x.iter().map(|a| a.mid()).collect();
    // E(T, m) in its mean-value form about T's middle: E(t0, m) + E_t(T, m) (T - t0) (a
    // direct enclosure over T loses the cancellation in the nets' sums)
    let t0 = t.mid();
    let at = |t: Ivl| {
        let mut y: Vec<Ivl> = m.iter().map(|&a| pt(a)).collect();
        y.insert(ti, t);
        sys.eval_on(&y, force)
    };
    let (e0, et) = (at(pt(t0)), at(t));
    let centre_e: Vec<Ivl> =
        e0.e.iter()
            .zip(&et.j)
            .map(|(&e, r)| e + r[ti] * (t - pt(t0)))
            .collect();
    let mut y = x.to_vec();
    y.insert(ti, t);
    let over = sys.eval_on(&y, force);
    let jx: Vec<Vec<Ivl>> = over
        .j
        .iter()
        .map(|r| (0..r.len()).filter(|&c| c != ti).map(|c| r[c]).collect())
        .collect();
    let mids: Vec<Vec<f64>> = jx
        .iter()
        .map(|r| r.iter().map(|a| a.mid()).collect())
        .collect();
    let inv = inverse(&mids)?;
    let n = x.len();
    let mut k = Vec::with_capacity(n);
    for (i, yi) in inv.iter().enumerate() {
        let mut s = pt(m[i]);
        for (&yij, &e) in yi.iter().zip(&centre_e) {
            s = s - pt(yij) * e;
        }
        for l in 0..n {
            let mut c = pt(f64::from(u8::from(i == l)));
            for (&yij, row) in yi.iter().zip(&jx) {
                c = c - pt(yij) * row[l];
            }
            s = s + c * (x[l] - pt(m[l]));
        }
        k.push(s);
    }
    Some(k)
}

/// The arc point of box `b` at `t = at` (enclosed, all unknowns): Krawczyk's test on the
/// point `t`, then `X <- X & K(X)` while it shrinks (the zero lies in every such `X`).
fn point_at(sys: &System, b: &KBox, at: f64) -> Option<Vec<Ivl>> {
    let mut x = krawczyk(sys, b.ti, pt(at), &b.x, &b.force)?;
    for _ in 0..12 {
        let Some(k) = krawczyk_k(sys, b.ti, pt(at), &x, &b.force) else {
            break;
        };
        let next: Vec<Ivl> = x
            .iter()
            .zip(&k)
            .map(|(a, c)| Ivl::new(a.lo.max(c.lo), a.hi.min(c.hi)))
            .collect();
        if next.iter().any(|v| v.lo > v.hi) {
            break;
        }
        let shrunk = next
            .iter()
            .zip(&x)
            .any(|(n, o)| n.hi - n.lo < 0.9 * (o.hi - o.lo));
        x = next;
        if !shrunk {
            break;
        }
    }
    x.insert(b.ti, pt(at));
    Some(x)
}

/// A box over the arc from `ya` to `yb` (zeros), or `None`.
fn segment(sys: &System, ya: &[f64], yb: &[f64], ti: usize) -> Option<KBox> {
    segment_with(sys, ya, yb, ti, [true, true], &vec![None; ya.len()])
}

/// [`segment`] with `T` extended past `ya` and `yb` (`ext`) and the nets' spans `force`d.
fn segment_with(
    sys: &System,
    ya: &[f64],
    yb: &[f64],
    ti: usize,
    ext: [bool; 2],
    force: &[Option<usize>],
) -> Option<KBox> {
    let (ta, tb) = (ya[ti], yb[ti]);
    let dt = tb - ta;
    if dt == 0.0 {
        return None;
    }
    let over = 0.2 * dt.abs();
    let ea = if ext[0] { over } else { 0.0 };
    let eb = if ext[1] { over } else { 0.0 };
    let t = if dt > 0.0 {
        Ivl::new(ta - ea, tb + eb)
    } else {
        Ivl::new(tb - eb, ta + ea)
    };
    for grow in [0.6, 2.5] {
        let mut x: Vec<Ivl> = (0..ya.len())
            .filter(|&c| c != ti)
            .map(|c| {
                let (a, b) = (ya[c], yb[c]);
                let m = grow * ((b - a).abs() + 1e-9 * dt.abs()) + 1e-13 * (1.0 + a.abs());
                Ivl::new(a.min(b) - m, a.max(b) + m)
            })
            .collect();
        // epsilon-inflation: where K leaves X, X becomes their hull, widened by a tenth
        for _ in 0..6 {
            let Some(k) = krawczyk_k(sys, ti, t, &x, force) else {
                break;
            };
            if k.iter().zip(&x).all(|(&a, &b)| inside(a, b)) {
                return Some(KBox {
                    ti,
                    t,
                    x,
                    dir: dt.signum(),
                    force: force.to_vec(),
                    at: [ta, tb],
                    on_face: [false; 2],
                    seam_side: None,
                    ay_sign: None,
                });
            }
            if k.iter().any(|v| !v.is_finite()) {
                break;
            }
            x = k
                .iter()
                .zip(&x)
                .map(|(kk, xx)| {
                    let h = kk.hull(*xx);
                    let w = 0.1 * (h.hi - h.lo) + 1e-15 * (1.0 + h.mag());
                    Ivl::new(h.lo - w, h.hi + w)
                })
                .collect();
        }
    }
    None
}

/// The one knot line of unknown `c`'s net strictly between `a` and `b`: its value and the
/// spans on either side, `None` for none or more than one (or a plane's unknown).
fn knot_between(sys: &System, c: usize, a: f64, b: f64) -> Option<(f64, usize, usize)> {
    let Side::Net(n) = &sys.sides[*sys.param.get(c / 2)?] else {
        return None;
    };
    let (u, p, cnt) = if c.is_multiple_of(2) {
        (&n.u, n.p, n.nu)
    } else {
        (&n.v, n.q, n.nv)
    };
    let (lo, hi) = (a.min(b), a.max(b));
    let mut inner: Vec<f64> = u[p + 1..cnt]
        .iter()
        .copied()
        .filter(|&x| x > lo && x < hi)
        .collect();
    inner.dedup();
    let [kappa] = inner[..] else { return None };
    Some((
        kappa,
        span(u, p, cnt, 0.5 * (lo + kappa)),
        span(u, p, cnt, kappa),
    ))
}

/// The knots that bound span `k` of unknown `c`'s net.
fn span_ends(sys: &System, c: usize, k: usize) -> Option<(f64, f64)> {
    let Side::Net(n) = &sys.sides[*sys.param.get(c / 2)?] else {
        return None;
    };
    let u = if c.is_multiple_of(2) { &n.u } else { &n.v };
    Some((*u.get(k)?, *u.get(k + 1)?))
}

/// Two boxes across the knot line `c = kappa` between the zeros `a` and `b`: `t = c`, each
/// on its side's polynomial piece continued, meeting on the line, where the arc point of
/// each lies strictly inside the other's `X` (the system is continuous across the line,
/// so both see the one zero there).
fn across(sys: &System, a: &[f64], b: &[f64], c: usize) -> Option<[KBox; 2]> {
    let (kappa, left, right) = knot_between(sys, c, a[c], b[c])?;
    let mut m: Vec<f64> = a.iter().zip(b).map(|(x, y)| 0.5 * (x + y)).collect();
    m[c] = kappa;
    let yk = newton(sys, &m, Some(c), None)?;
    let force = |k: usize| {
        let mut f = vec![None; a.len()];
        f[c] = Some(k);
        f
    };
    let (fa, fb) = if a[c] < kappa {
        (force(left), force(right))
    } else {
        (force(right), force(left))
    };
    let mut ba = segment_with(sys, a, &yk, c, [true, false], &fa)?;
    let mut bb = segment_with(sys, &yk, b, c, [false, true], &fb)?;
    // each box's polynomial is its surface's only over its span
    let on_span = |bx: &KBox, k: Option<usize>| {
        k.and_then(|k| span_ends(sys, c, k))
            .is_some_and(|(lo, hi)| bx.t.lo >= lo && bx.t.hi <= hi)
    };
    if !on_span(&ba, fa[c]) || !on_span(&bb, fb[c]) {
        return None;
    }
    // the end of each T at kappa
    ba.on_face = [ba.t.lo == kappa, ba.t.hi == kappa];
    bb.on_face = [bb.t.lo == kappa, bb.t.hi == kappa];
    let pa = point_at(sys, &ba, kappa)?;
    let pb = point_at(sys, &bb, kappa)?;
    let strict = |p: &[Ivl], bx: &KBox| {
        (0..p.len())
            .filter(|&i| i != c)
            .zip(&bx.x)
            .all(|(i, &x)| inside(p[i], x))
    };
    (strict(&pa, &bb) && strict(&pb, &ba)).then_some([ba, bb])
}

/// The end of box `b` facing the tube's next box (`ahead`) or its previous one.
fn facing(b: &KBox, ahead: bool) -> f64 {
    if (b.dir > 0.0) == ahead {
        b.t.hi
    } else {
        b.t.lo
    }
}

fn strictly_in(p: &[Ivl], b: &[Ivl]) -> bool {
    p.iter().zip(b).all(|(&a, &c)| inside(a, c))
}

/// Whether consecutive boxes share their arc: `a`'s arc point at its last zero lies in
/// `b` (a zero there, so `b`'s one for that value of its `t`), or `b`'s at its first in
/// `a`.
fn joined(sys: &System, a: &KBox, b: &KBox) -> bool {
    let within_all = |p: &[Ivl], bx: &KBox| p.iter().zip(&bx.full()).all(|(&x, &y)| within(x, y));
    point_at(sys, a, a.at[1]).is_some_and(|p| within_all(&p, b))
        || point_at(sys, b, b.at[0]).is_some_and(|p| within_all(&p, a))
}

/// Whether every end of every box's arc lies strictly inside another box of the tube (a
/// neighbour up to eight along), but for the tube's own ends (`open`) and the ends on a
/// knot line: then the zeros in the tube are open in the zeros (an arc point on a box's
/// face goes on inside another box), so the tube holds one component. `Err` names the
/// first end that does not.
fn covered(sys: &System, boxes: &[KBox], open: bool) -> Result<(), String> {
    let n = boxes.len();
    for (k, b) in boxes.iter().enumerate() {
        for (side, t) in [(0, b.t.lo), (1, b.t.hi)] {
            if b.on_face[side] {
                continue;
            }
            // the tube's ends: the first box's end facing back, the last's facing on
            let back = (side == 1) != (b.dir > 0.0);
            if open && ((k == 0 && back) || (k + 1 == n && !back)) {
                continue;
            }
            let p = point_at(sys, b, t).ok_or_else(|| format!("box {k}: no arc point at {t}"))?;
            let mut near = (1..=8isize).flat_map(|d| [d, -d]).filter_map(|d| {
                let j = k as isize + d;
                if open {
                    usize::try_from(j).ok().filter(|&j| j < n)
                } else {
                    usize::try_from(j.rem_euclid(n as isize))
                        .ok()
                        .filter(|&j| j != k)
                }
            });
            if !near.any(|j| strictly_in(&p, &boxes[j].full())) {
                return Err(format!(
                    "box {k} of {n}: its arc's end at t = {t} lies in no other box"
                ));
            }
        }
    }
    Ok(())
}

/// Whether box `b` meets the domains as a tube needs: inside them, or every coordinate
/// (and a cone's height) it does not hold inside them monotone along the arc in it, so the
/// arc's part inside is one piece. An inner box's arc points at both ends of `T` lie
/// inside; an end box's (`outer`: the end of `T` towards the outside) there past the
/// domains, at the other end inside.
fn domain_ok(sys: &System, b: &KBox, outer: Option<f64>) -> bool {
    let full = b.full();
    let cone = sys.cone();
    let ev = |y: &[Ivl]| sys.eval_on(y, &b.force);
    let height = |y: &[Ivl]| ev(y).h.map(|(h, _)| h);
    let seam = |y: &[Ivl]| ev(y).seam.map(|(ay, ax, _)| (ay, ax));
    let leaks: Vec<usize> = (0..full.len())
        .filter(|&i| !within(full[i], sys.dom[i]))
        .collect();
    let h_leaks = cone.is_some_and(|c| height(&full).is_some_and(|h| !within(h, c.inner)));
    // the box meets the seam: a_y holds 0 where a_x may be positive
    let s_box = seam(&full);
    let s_leaks = s_box.is_some_and(|(ay, ax)| ay.lo <= 0.0 && ay.hi >= 0.0 && ax.hi > 0.0);
    if leaks.is_empty() && !h_leaks && !s_leaks && outer.is_none() {
        return true;
    }
    let Some((d, dh)) = slopes(sys, b) else {
        return false;
    };
    if leaks.iter().any(|&i| sign(d[i]).is_none()) || (h_leaks && dh.and_then(sign).is_none()) {
        return false;
    }
    if s_leaks {
        // crossing only where a_x > 0, a_y monotone along the arc
        let ds = ev(&full)
            .seam
            .map(|(_, _, g)| g.iter().zip(&d).fold(pt(0.0), |s, (&g, &v)| s + g * v));
        if !s_box.is_some_and(|(_, ax)| ax.lo > 0.0) || ds.and_then(sign).is_none() {
            return false;
        }
    }
    let inside_at = |p: &[Ivl]| {
        p.iter().zip(&sys.dom).all(|(&a, &d)| within(a, d))
            && cone.is_none_or(|c| height(p).is_some_and(|h| within(h, c.inner)))
    };
    let past_at = |p: &[Ivl]| {
        p.iter()
            .zip(&sys.dom)
            .any(|(a, d)| a.hi < d.lo || a.lo > d.hi)
            || cone
                .is_some_and(|c| height(p).is_some_and(|h| h.hi < c.outer.lo || h.lo > c.outer.hi))
    };
    let side = |p: &[Ivl]| seam(p).and_then(|(ay, _)| sign(ay));
    let at = |t: f64| point_at(sys, b, t);
    if let Some(o) = outer {
        let inner = if o == b.t.lo { b.t.hi } else { b.t.lo };
        let (Some(po), Some(pi)) = (at(o), at(inner)) else {
            return false;
        };
        let through_seam =
            s_leaks && side(&pi).is_some() && side(&po).is_some() && side(&pi) != side(&po);
        let in_ok = inside_at(&pi) && (!s_leaks || side(&pi).is_some());
        return in_ok && (past_at(&po) || through_seam);
    }
    let (Some(p0), Some(p1)) = (at(b.t.lo), at(b.t.hi)) else {
        return false;
    };
    // a monotone a_y of one strict sign at both ends never reaches the seam
    inside_at(&p0)
        && inside_at(&p1)
        && (!s_leaks || (side(&p0).is_some() && side(&p0) == side(&p1)))
}

/// For an end box (`outer` its end of `T` outside) across a cone's seam: the sign of
/// `a_y` at its inner arc point.
fn seam_side(sys: &System, b: &KBox, outer: Option<f64>) -> Option<i32> {
    let o = outer?;
    let (ay, ax, _) = sys.eval_on(&b.full(), &b.force).seam?;
    if !(ay.lo <= 0.0 && ay.hi >= 0.0 && ax.lo > 0.0) {
        return None;
    }
    let inner = if o == b.t.lo { b.t.hi } else { b.t.lo };
    let p = point_at(sys, b, inner)?;
    sign(sys.eval_on(&p, &b.force).seam?.0)
}

/// How a tube's end leaves the domains.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Exit {
    /// Through an unknown's domain end.
    Param(usize, f64),
    /// Through the cone's height window, at that height.
    Height(f64),
    /// Through the cone's seam.
    Seam,
}

/// A zero past the domain end `exit`, from the zero `inner` inside.
fn outer_point(sys: &System, inner: &[f64], exit: Exit) -> Option<Vec<f64>> {
    [0.5, 0.2, 0.05].into_iter().find_map(|over| match exit {
        Exit::Param(c, e) => {
            let mut y = inner.to_vec();
            y[c] = e + (e - inner[c]) * over;
            newton(sys, &y, Some(c), None)
        }
        Exit::Height(e) => {
            let h0 = sys.eval_at(inner).h.map_or(f64::NAN, |(h, _)| h.mid());
            newton(sys, inner, None, Some(Target::Height(e + (e - h0) * over)))
        }
        Exit::Seam => {
            let a0 = sys
                .eval_at(inner)
                .seam
                .map_or(f64::NAN, |(a, _, _)| a.mid());
            newton(sys, inner, None, Some(Target::Seam(-a0 * over)))
        }
    })
}

/// The nearest domain end to `y` and its distance, relative to the domain's width.
fn nearest_exit(sys: &System, y: &[f64]) -> (Exit, f64) {
    let mut best = (Exit::Param(0, 0.0), f64::INFINITY);
    for (i, d) in sys.dom.iter().enumerate() {
        let w = (d.hi - d.lo).max(f64::MIN_POSITIVE);
        for e in [d.lo, d.hi] {
            let r = (y[i] - e).abs() / w;
            if r < best.1 {
                best = (Exit::Param(i, e), r);
            }
        }
    }
    let ev = sys.eval_at(y);
    if let Some((ay, ax, _)) = ev.seam {
        let (ay, ax) = (ay.mid(), ax.mid());
        if ax > 0.0 {
            let r = ay.atan2(ax).abs() / std::f64::consts::TAU;
            if r < best.1 {
                best = (Exit::Seam, r);
            }
        }
    }
    if let (Some(c), Some((h, _))) = (sys.cone(), ev.h) {
        let w = c.outer.hi - c.outer.lo;
        for e in [c.inner.lo, c.inner.hi] {
            let r = (h.mid() - e).abs() / w;
            if r < best.1 {
                best = (Exit::Height(e), r);
            }
        }
    }
    best
}

/// The arc's derivatives along `t` over box `b`, enclosed: `dx/dt = -E_x^-1 E_t`, with
/// `Y E_x = I - R` and `|R| <= r < 1` (row sums), `dx/dt = -(z + R z + R^2 z + ...)` for
/// `z = Y E_t`, so within `r / (1 - r) max|z|` of `-z` (with `r < 1/2`, at most
/// `2 r max|z|`; `r` summed in floating point from enclosures' magnitudes, its rounding
/// inside the `1e-12` added); `t`'s own is 1. With a cone, the height's too.
fn slopes(sys: &System, b: &KBox) -> Option<(Vec<Ivl>, Option<Ivl>)> {
    let ev = sys.eval_on(&b.full(), &b.force);
    let n = b.x.len();
    let cols: Vec<usize> = (0..=n).filter(|&c| c != b.ti).collect();
    let jx: Vec<Vec<Ivl>> =
        ev.j.iter()
            .map(|r| cols.iter().map(|&c| r[c]).collect())
            .collect();
    let mids: Vec<Vec<f64>> = jx
        .iter()
        .map(|r| r.iter().map(|a| a.mid()).collect())
        .collect();
    let y = inverse(&mids)?;
    let z: Vec<Ivl> = y
        .iter()
        .map(|yi| {
            yi.iter()
                .zip(&ev.j)
                .fold(pt(0.0), |s, (&yij, row)| s + pt(yij) * row[b.ti])
        })
        .collect();
    let mut r = 0.0f64;
    for (i, yi) in y.iter().enumerate() {
        let mut row = 0.0;
        for l in 0..n {
            let yj = yi
                .iter()
                .zip(&jx)
                .fold(pt(0.0), |s, (&yij, jr)| s + pt(yij) * jr[l]);
            row += (pt(f64::from(u8::from(i == l))) - yj).mag();
        }
        r = r.max(row);
    }
    if r >= 0.5 {
        return None;
    }
    let zmax = z.iter().map(|a| a.mag()).fold(0.0f64, f64::max);
    let rad = 2.0 * r * zmax * (1.0 + 1e-12);
    let mut d: Vec<Ivl> = z
        .iter()
        .map(|&zi| pt(0.0) - zi + Ivl::new(-rad, rad))
        .collect();
    d.insert(b.ti, pt(1.0));
    let dh =
        ev.h.map(|(_, hg)| hg.iter().zip(&d).fold(pt(0.0), |s, (&g, &v)| s + g * v));
    Some((d, dh))
}

/// How a tube ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Ends {
    /// Its last box joins its first.
    Closed,
    /// Its arc leaves the domains at its start and at its end.
    Open([Exit; 2]),
}

/// A certified tube along a chain of curves.
#[derive(Debug)]
pub(crate) struct Tube {
    boxes: Vec<KBox>,
    /// How it ends.
    pub(crate) ends: Ends,
}

/// The zeros along a curve at `m + 1` evenly spaced fractions of its interval: each
/// surface's parameters from its UV curve at that fraction, refined (`None` where Newton
/// does not converge).
fn samples(
    k: &Kernel,
    sys: &System,
    c: AnswerCurve<'_>,
    m: usize,
) -> Result<Vec<Option<Vec<f64>>>, String> {
    (0..=m)
        .map(|j| {
            let y = uv_point(k, sys, c, j as f64 / m as f64)?;
            Ok(newton(sys, &y, Some(choose_t(sys, &y)), None))
        })
        .collect()
}

/// A curve of an answer as the certificate takes it: its 3D curve and its UV curve on
/// each surface where the answer has one. A plane's parameters can come from the 3D
/// curve instead (mapped back exactly); a cone has none.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AnswerCurve<'a> {
    pub(crate) curve: &'a Curve,
    pub(crate) uv: [Option<&'a Curve>; 2],
}

/// The unknowns at the fraction `f` of a curve's interval, as its UV curves give them, or
/// for a plane without one, its 3D curve mapped back (the UV curves and the 3D curve run
/// over their intervals alike; the zeros are refined from these guesses).
///
/// # Panics
///
/// When a curve cannot be evaluated.
fn uv_point(k: &Kernel, sys: &System, c: AnswerCurve<'_>, f: f64) -> Result<Vec<f64>, String> {
    let at = |crv: &Curve| {
        let i = crv.natural_interval();
        crv.evaluate_point(k, i.min() + f * (i.max() - i.min()))
            .expect("a point of the curve")
    };
    let mut y = Vec::with_capacity(4);
    for &s in &sys.param {
        match (c.uv[s], &sys.sides[s]) {
            (Some(u), _) => {
                let p = at(u);
                y.extend([p.x, p.y]);
            }
            (None, Side::Plane { o, du, dv }) => {
                let p = at(c.curve);
                let r = [p.x - o[0].mid(), p.y - o[1].mid(), p.z - o[2].mid()];
                let (du, dv) = (mid3(*du), mid3(*dv));
                let d = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
                // the normal equations of r = u du + v dv
                let (a, b, e) = (d(du, du), d(du, dv), d(dv, dv));
                let (ru, rv) = (d(du, r), d(dv, r));
                let det = a * e - b * b;
                y.extend([(ru * e - rv * b) / det, (rv * a - ru * b) / det]);
            }
            (None, _) => return Err(format!("no UV curve on surface {s}")),
        }
    }
    Ok(y)
}

/// A piece of the arc between two zeros to be covered, and whether either zero lies past
/// the domains (an open chain's ends: the box over it is an end box).
#[derive(Clone, Debug)]
struct Seg {
    a: Vec<f64>,
    b: Vec<f64>,
    depth: u32,
    outer: [bool; 2],
}

impl Seg {
    /// The end of `bx`'s `T` towards the outside, for a segment at a chain's end.
    fn outer_end(&self, bx: &KBox) -> Option<f64> {
        if self.outer[0] {
            Some(facing(bx, false))
        } else if self.outer[1] {
            Some(facing(bx, true))
        } else {
            None
        }
    }
}

/// Whether the zero `y` lies past the domains (an unknown, or a cone's height).
fn past(sys: &System, y: &[f64]) -> bool {
    y.iter().zip(&sys.dom).any(|(x, d)| *x < d.lo || *x > d.hi)
        || sys.cone().is_some_and(|c| {
            sys.eval_at(y)
                .h
                .is_some_and(|(h, _)| h.mid() < c.inner.lo || h.mid() > c.inner.hi)
        })
}

/// The chain of boxes over consecutive zeros `ys` (an open chain's first and last past
/// the domains), each piece split where the test fails, across a knot line by [`across`];
/// a box that does not join the one before reopens that one.
fn chain(sys: &System, ys: &[Vec<f64>], open: bool) -> Result<Vec<KBox>, String> {
    let n = ys.len();
    let mut stack: Vec<Seg> = ys
        .windows(2)
        .enumerate()
        .rev()
        .map(|(k, w)| Seg {
            a: w[0].clone(),
            b: w[1].clone(),
            depth: 0,
            outer: [open && k == 0, open && k + 2 == n],
        })
        .collect();
    let mut boxes: Vec<(KBox, Seg)> = Vec::new();
    while let Some(sg) = stack.pop() {
        let mid: Vec<f64> = sg.a.iter().zip(&sg.b).map(|(x, y)| 0.5 * (x + y)).collect();
        // the best `t` first, then the others by the tangent's share
        let mut made: Vec<KBox> = by_share(sys, &mid)
            .into_iter()
            .find_map(|c| {
                let mut bx = segment(sys, &sg.a, &sg.b, c)?;
                let outer = sg.outer_end(&bx);
                if !domain_ok(sys, &bx, outer) {
                    return None;
                }
                bx.seam_side = seam_side(sys, &bx, outer);
                Some(bx)
            })
            .into_iter()
            .collect();
        if made.is_empty() && !sg.outer.contains(&true) {
            if let Some([x, y]) = (0..sg.a.len())
                .find_map(|c| across(sys, &sg.a, &sg.b, c))
                .filter(|[x, y]| domain_ok(sys, x, None) && domain_ok(sys, y, None))
            {
                made = vec![x, y];
            }
        }
        if let Some(head) = made.first() {
            if let Some((prev, psg)) = boxes.last() {
                if !joined(sys, prev, head) {
                    if psg.depth >= 16 && sg.depth >= 16 {
                        return Err(format!("no joined boxes at {:?}", sg.a));
                    }
                    // reopen the box before: both pieces again, the earlier one halved
                    let psg = reopen(&mut boxes).expect("a box before");
                    stack.push(Seg {
                        depth: sg.depth + 1,
                        ..sg
                    });
                    stack.extend(split(sys, &psg)?.into_iter().rev());
                    continue;
                }
            }
            let last = made.len() - 1;
            for (k, bx) in made.into_iter().enumerate() {
                let s = if k == last {
                    sg.clone()
                } else {
                    Seg {
                        outer: [false; 2],
                        ..sg.clone()
                    }
                };
                boxes.push((bx, s));
            }
            continue;
        }
        if sg.depth >= 16 {
            return Err(format!("no box over the arc at {:?}", sg.a));
        }
        stack.extend(split(sys, &sg)?.into_iter().rev());
    }
    let boxes: Vec<KBox> = boxes.into_iter().map(|(b, _)| b).collect();
    let mut boxes = bridged(sys, &boxes, open);
    for b in &mut boxes {
        // the side of the seam an end box's arc lies on inside the domain, else the strict
        // sign of `a_y` along the box's arc
        b.ay_sign = b.seam_side.or_else(|| arc_ay_sign(sys, b));
    }
    covered(sys, &boxes, open)?;
    Ok(boxes)
}

/// Takes the last box off the chain to cover its piece again, and returns the piece; a
/// knot line's pair goes whole ([`across`] makes it over one piece): [`covered`] takes
/// each end on the line as met by the pair's other box, so a box left without it would
/// leave that end unchecked.
fn reopen(boxes: &mut Vec<(KBox, Seg)>) -> Option<Seg> {
    let (b, sg) = boxes.pop()?;
    if b.on_face.contains(&true) {
        boxes.pop();
    }
    Some(sg)
}

/// The sign of a cone's `a_y` along box `b`'s arc, where it has one strict sign: over the
/// whole box, or at both ends of the arc with `a_y` monotone along it.
fn arc_ay_sign(sys: &System, b: &KBox) -> Option<i32> {
    let (ay, _, g) = sys.eval_on(&b.full(), &b.force).seam?;
    if let Some(s) = sign(ay) {
        return Some(s);
    }
    let (d, _) = slopes(sys, b)?;
    sign(g.iter().zip(&d).fold(pt(0.0), |s, (&g, &v)| s + g * v))?;
    let at = |t: f64| sign(sys.eval_on(&point_at(sys, b, t)?, &b.force).seam?.0);
    let (lo, hi) = (at(b.t.lo)?, at(b.t.hi)?);
    (lo == hi).then_some(lo)
}

/// The zero of box `b`'s arc at `t` (its enclosure's middle, refined).
fn zero_at(sys: &System, b: &KBox, t: f64) -> Option<Vec<f64>> {
    let p = point_at(sys, b, t)?;
    let y: Vec<f64> = p.iter().map(|v| v.mid()).collect();
    newton(sys, &y, Some(b.ti), None)
}

/// `boxes` with a bridge between each two consecutive ones (not a knot line's pair): a
/// box over the arc from the middle of one to the middle of the next, so that the ends at
/// the junction lie inside a box ([`covered`] checks it).
fn bridged(sys: &System, boxes: &[KBox], open: bool) -> Vec<KBox> {
    let n = boxes.len();
    let mut out = Vec::with_capacity(2 * n);
    for (k, b) in boxes.iter().enumerate() {
        out.push(b.clone());
        let next = if k + 1 < n {
            &boxes[k + 1]
        } else if open {
            continue;
        } else {
            &boxes[0]
        };
        if b.on_face.contains(&true) && next.on_face.contains(&true) {
            continue;
        }
        let made = [(0.5, 0.5), (0.75, 0.25), (0.9, 0.1)]
            .into_iter()
            .find_map(|(fa, fb)| {
                let ta = b.at[0] + fa * (b.at[1] - b.at[0]);
                let tb = next.at[0] + fb * (next.at[1] - next.at[0]);
                let (ya, yb) = (zero_at(sys, b, ta)?, zero_at(sys, next, tb)?);
                let mid: Vec<f64> = ya.iter().zip(&yb).map(|(x, y)| 0.5 * (x + y)).collect();
                segment(sys, &ya, &yb, choose_t(sys, &mid)).filter(|bx| domain_ok(sys, bx, None))
            });
        if let Some(bx) = made {
            out.push(bx);
        }
    }
    out
}

/// `sg` halved at a zero between its ends; a half between a zero past the domains and
/// another past them is dropped (the end box must straddle the domains' end).
fn split(sys: &System, sg: &Seg) -> Result<Vec<Seg>, String> {
    let mid: Vec<f64> = sg.a.iter().zip(&sg.b).map(|(x, y)| 0.5 * (x + y)).collect();
    let m = newton(sys, &mid, Some(choose_t(sys, &mid)), None)
        .ok_or_else(|| format!("no zero between {:?} and {:?}", sg.a, sg.b))?;
    let d = sg.depth + 1;
    let out = past(sys, &m);
    Ok(if sg.outer[0] && out {
        vec![Seg {
            a: m,
            b: sg.b.clone(),
            depth: d,
            outer: sg.outer,
        }]
    } else if sg.outer[1] && out {
        vec![Seg {
            a: sg.a.clone(),
            b: m,
            depth: d,
            outer: sg.outer,
        }]
    } else {
        vec![
            Seg {
                a: sg.a.clone(),
                b: m.clone(),
                depth: d,
                outer: [sg.outer[0], false],
            },
            Seg {
                a: m,
                b: sg.b.clone(),
                depth: d,
                outer: [false, sg.outer[1]],
            },
        ]
    })
}

/// The closed run of zeros `ys` opened where it crosses a cone's seam (`a_y` changes sign
/// where `a_x > 0`): it starts on one side and ends on the other. The zeros within 1e-6
/// radians of the seam are left out (their side is not known). `None` where it does not
/// cross.
fn opened_at_seam(sys: &System, ys: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    // a_y where a_x > 0
    let ay = |y: &[f64]| {
        let (ay, ax, _) = sys.eval_at(y).seam?;
        (ax.mid() > 0.0).then_some((ay.mid(), ax.mid()))
    };
    let mut kept: Vec<Vec<f64>> = ys
        .iter()
        .filter(|y| !ay(y).is_some_and(|(a, x)| a.abs() <= 1e-6 * x))
        .cloned()
        .collect();
    let n = kept.len();
    let j = (0..n).find(|&j| {
        let (a, b) = (ay(&kept[j]), ay(&kept[(j + 1) % n]));
        a.zip(b).is_some_and(|((a, _), (b, _))| a * b < 0.0)
    })?;
    kept.rotate_left(j + 1);
    Some(kept)
}

/// How a curve's end continues: out of the domains, or into another curve's end.
#[derive(Clone, Copy, Debug, PartialEq)]
enum End {
    Exit(Exit),
    Curve(usize, usize),
}

/// A certified component: the curves it holds (in order along it) and its tube.
#[derive(Debug)]
pub(crate) struct Component {
    pub(crate) curves: Vec<usize>,
    pub(crate) tube: Tube,
}

/// Certifies the components of an answer's `curves` (module docs): curves are chained
/// where one's end is another's; each chain is one tube, closed or leaving the domains at
/// both ends. One result per chain; `Err` where its tube cannot be built, which proves
/// nothing either way (a curve without parameters on a surface, a curve that ends inside
/// both domains at no other curve's end, the surfaces tangent along it, a box test that
/// does not close).
///
/// # Panics
///
/// When a curve cannot be evaluated.
pub(crate) fn components(
    k: &Kernel,
    sys: &System,
    curves: &[AnswerCurve<'_>],
) -> Vec<Result<Component, String>> {
    const M: usize = 32;
    let all: Vec<Result<Vec<Option<Vec<f64>>>, String>> =
        curves.iter().map(|c| samples(k, sys, *c, M)).collect();
    // the ends as the curves give them (on a domain end or a knot line, where Newton need
    // not settle)
    let ends: Vec<Result<[Vec<f64>; 2], String>> = curves
        .iter()
        .map(|c| Ok([uv_point(k, sys, *c, 0.0)?, uv_point(k, sys, *c, 1.0)?]))
        .collect();
    let near = |a: &[f64], b: &[f64]| {
        a.iter()
            .zip(b)
            .all(|(x, y)| (x - y).abs() <= 1e-6 * (1.0 + x.abs()))
    };
    let link = |i: usize, e: usize| -> Result<End, String> {
        let y = &ends[i].as_ref().map_err(|m| format!("curve {i}: {m}"))?[e];
        let (exit, r) = nearest_exit(sys, y);
        if r <= 1e-6 {
            return Ok(End::Exit(exit));
        }
        for (j, ej) in ends.iter().enumerate() {
            let Ok(ej) = ej else { continue };
            for (f, yj) in ej.iter().enumerate() {
                if (j, f) != (i, e) && near(y, yj) {
                    return Ok(End::Curve(j, f));
                }
            }
        }
        Err(format!(
            "curve {i}'s end {e} at {y:?} is neither a domain end nor another curve's end"
        ))
    };
    let mut seen = vec![false; curves.len()];
    let mut out = Vec::new();
    for first in 0..curves.len() {
        if seen[first] {
            continue;
        }
        let mut one = || -> Result<Component, String> {
            // walk back from `first` to the chain's start
            let (mut i, mut e) = (first, 0);
            for _ in 0..curves.len() {
                match link(i, e)? {
                    End::Curve(j, f) if j != first => (i, e) = (j, 1 - f),
                    _ => break,
                }
            }
            // then forward, collecting the curves (each with the end it is entered from)
            let start = (i, e);
            let mut order = Vec::new();
            let mut closed = false;
            loop {
                order.push((i, e));
                seen[i] = true;
                match link(i, 1 - e)? {
                    End::Exit(_) => break,
                    End::Curve(j, f) if (j, f) == start => {
                        closed = true;
                        break;
                    }
                    End::Curve(j, f) => (i, e) = (j, f),
                }
                if order.len() > curves.len() {
                    return Err("the curves' ends do not chain".into());
                }
            }
            let mut ys = Vec::new();
            for &(i, e) in &order {
                let mut inner: Vec<Vec<f64>> =
                    all[i].as_ref().map_err(|m| format!("curve {i}: {m}"))?[1..M]
                        .iter()
                        .flatten()
                        .cloned()
                        .collect();
                if inner.len() < 4 {
                    return Err(format!("curve {i}: no zeros along it"));
                }
                if e == 1 {
                    inner.reverse();
                }
                ys.extend(inner);
            }
            let held: Vec<usize> = order.iter().map(|o| o.0).collect();
            let exits = if !closed {
                let (s1, e1) = *order.last().unwrap_or(&start);
                let exit = |x: End| match x {
                    End::Exit(x) => Ok(x),
                    End::Curve(..) => Err("a chain end that is not a domain end".to_string()),
                };
                [exit(link(start.0, start.1)?)?, exit(link(s1, 1 - e1)?)?]
            } else if let Some(opened) = opened_at_seam(sys, &ys) {
                // a closed curve across a cone's seam (a domain end): one arc from the
                // seam to the seam
                ys = opened;
                [Exit::Seam, Exit::Seam]
            } else {
                ys.push(ys[0].clone());
                let boxes = chain(sys, &ys, false)?;
                if !joined(sys, &boxes[boxes.len() - 1], &boxes[0]) {
                    return Err("the closed tube does not join".into());
                }
                let tube = Tube {
                    boxes,
                    ends: Ends::Closed,
                };
                return Ok(Component { curves: held, tube });
            };
            let o0 = outer_point(sys, &ys[0], exits[0])
                .ok_or_else(|| format!("no zero past {:?}", exits[0]))?;
            let o1 = outer_point(sys, &ys[ys.len() - 1], exits[1])
                .ok_or_else(|| format!("no zero past {:?}", exits[1]))?;
            let ys: Vec<Vec<f64>> = std::iter::once(o0)
                .chain(ys)
                .chain(std::iter::once(o1))
                .collect();
            let tube = Tube {
                boxes: chain(sys, &ys, true)?,
                ends: Ends::Open(exits),
            };
            Ok(Component { curves: held, tube })
        };
        out.push(one());
        seen[first] = true;
    }
    out
}

/// Whether two tubes are disjoint: every box of one apart from every box of the other in
/// some unknown, or its arc of the opposite strict sign of a cone's `a_y` (inside the
/// domain; an end box's arc reaches `a_y = 0` only on the seam, the domain's end, where
/// the two sides are a full turn apart).
pub(crate) fn disjoint(a: &Tube, b: &Tube) -> bool {
    a.boxes.iter().all(|x| {
        let fx = x.full();
        b.boxes.iter().all(|y| {
            let across_seam = matches!((x.ay_sign, y.ay_sign), (Some(p), Some(q)) if p != q);
            across_seam
                || fx
                    .iter()
                    .zip(&y.full())
                    .any(|(p, q)| p.hi < q.lo || q.hi < p.lo)
        })
    })
}

/// Whether the zero `y` lies in a box of `t` (where the only zeros are its arc's).
pub(crate) fn on_tube(t: &Tube, y: &[f64]) -> bool {
    t.boxes
        .iter()
        .any(|b| b.full().iter().zip(y).all(|(i, &x)| i.lo <= x && x <= i.hi))
}

/// The unknowns of a point given by its parameters on both surfaces.
pub(crate) fn unknowns(sys: &System, uv: [Point2; 2]) -> Vec<f64> {
    sys.param.iter().flat_map(|&s| [uv[s].x, uv[s].y]).collect()
}

/// A certified gap about `centre`: the box `centre +- radius` (within the domains) holds
/// no zero. Every part of a subdivision has an equation whose enclosure excludes 0 (or,
/// with a cone, a height past its domain); `Err` names the first part that does not at
/// the depth limit. Returns the parts' count.
pub(crate) fn no_zero(sys: &System, centre: &[f64], radius: &[f64]) -> Result<usize, String> {
    let clip = |c: f64, r: f64, d: Ivl| Ivl::new((c - r).max(d.lo), (c + r).min(d.hi));
    let first: Vec<Ivl> = centre
        .iter()
        .zip(radius)
        .zip(&sys.dom)
        .map(|((&c, &r), &d)| clip(c, r, d))
        .collect();
    let mut stack = vec![(first.clone(), 0u32)];
    let mut parts = 0;
    while let Some((y, depth)) = stack.pop() {
        let ev = sys.eval(&y);
        let empty = ev.e.iter().any(|e| e.lo > 0.0 || e.hi < 0.0)
            || sys
                .cone()
                .zip(ev.h.as_ref())
                .is_some_and(|(c, (h, _))| h.lo > c.outer.hi || h.hi < c.outer.lo);
        if empty {
            parts += 1;
            continue;
        }
        if depth >= 40 || parts > 4_000_000 {
            return Err(format!("a part that may hold a zero: {y:?}"));
        }
        // halve the widest unknown, relative to the first box
        let w = |i: usize| (y[i].hi - y[i].lo) / (first[i].hi - first[i].lo).max(f64::MIN_POSITIVE);
        let c = (0..y.len())
            .max_by(|&a, &b| w(a).total_cmp(&w(b)))
            .unwrap_or(0);
        let m = y[c].mid();
        let (mut lo, mut hi) = (y.clone(), y);
        lo[c] = Ivl::new(lo[c].lo, m);
        hi[c] = Ivl::new(m, hi[c].hi);
        stack.push((lo, depth + 1));
        stack.push((hi, depth + 1));
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::geom::vector2d::Vec2;
    use solidcore::math::{Point3, Vec3};
    use solidcore::nurbs::b_spline_curve::BSplineCurve;
    use solidcore::nurbs::b_spline_surface::BSplineSurface;
    use solidcore::nurbs::crv_basic::Curve as NlCurve;
    use solidcore::nurbs::math::CPoint;
    use solidcore::nurbs::srf_basic::Surface as NurbSurface;
    use solidcore::surface::cone::Cone;
    use solidcore::surface::plane::Plane;
    use solidcore::surface::surf_of_extrusion::SurfOfExtrusion;
    use solidcore::surface::surface::p2::{SurfaceIntersection, TsectCurveType};

    fn plane(origin: [f64; 3], x: [f64; 3], y: [f64; 3], half: [f64; 2]) -> Surface {
        let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
        let d = Extent2d::from_bounds(-half[0], -half[1], half[0], half[1]);
        let o = Point3::new(origin[0], origin[1], origin[2]);
        Surface::from(Plane::new(o, v(x), v(y), Vec2::new(1.0, 1.0), &d))
    }

    /// The cylinder of radius 1 about z from 0 to 4, its seam on +x.
    fn cylinder(k: &Kernel) -> Surface {
        let mut c = Cone::new_cylinder_bounded(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            1.0,
            0.0,
            360.0,
            4.0,
            false,
            false,
            false,
        );
        c.make_nurb(k).unwrap();
        Surface::from(c)
    }

    /// The polyline through `pts` (z = 0) extruded along z over `[0, 2]`: a net of degree
    /// 1 in both directions, not smooth across each inner point's knot line.
    fn extruded_polyline(k: &Kernel, pts: &[[f64; 2]]) -> Surface {
        let cps = pts
            .iter()
            .map(|p| CPoint::new(p[0], p[1], 0.0, NOW))
            .collect();
        let n = pts.len() - 1;
        let knots: Vec<f64> = std::iter::once(0.0)
            .chain((0..=n).map(|i| i as f64))
            .chain(std::iter::once(n as f64))
            .collect();
        let gen = Curve::from(BSplineCurve::from_nurb(
            NlCurve::new(cps, 1, knots).unwrap(),
            3,
            false,
        ));
        let origin = gen.evaluate_point(k, 0.0).unwrap();
        let d = Extent2d::from_bounds(0.0, 0.0, n as f64, 2.0);
        let mut s =
            SurfOfExtrusion::new(k, gen, origin, Vec3::new(0.0, 0.0, 1.0), d, false).unwrap();
        s.make_nurb(k).unwrap();
        Surface::from(s)
    }

    /// A bicubic dome over `[0, 3]^2` (z = 0 at its edges, 1.5 at the middle control
    /// points): a horizontal plane at a height below its top cuts it in one closed loop
    /// inside its domain.
    fn dome() -> Surface {
        let h: [f64; 4] = [0.0, 1.5, 1.5, 0.0];
        let net = (0..4)
            .map(|i| {
                (0..4)
                    .map(|j| CPoint::new(i as f64, j as f64, h[i].min(h[j]), NOW))
                    .collect()
            })
            .collect();
        let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let nurb = NurbSurface::new(net, 3, 3, knots.clone(), knots).unwrap();
        Surface::from(BSplineSurface::from_nurb(nurb, false))
    }

    /// The kernel's answer for `s[0]` against `s[1]` over their natural domains.
    fn answer(k: &Kernel, s: [&Surface; 2]) -> ([Extent2d; 2], SurfaceIntersection) {
        let d = [
            s[0].natural_uv_domain_with_nurb(k).unwrap(),
            s[1].natural_uv_domain_with_nurb(k).unwrap(),
        ];
        let r = s[0]
            .global_surface_intersect(k, &d[0], s[1], &d[1], [true, true], None, None)
            .unwrap();
        (d, r)
    }

    /// The answer's curves of type `ty`.
    fn of_type(r: &SurfaceIntersection, ty: TsectCurveType) -> Vec<AnswerCurve<'_>> {
        (0..r.curves_3d.len())
            .filter(|&i| r.curve_types[i] == ty)
            .map(|i| AnswerCurve {
                curve: &r.curves_3d[i],
                uv: [
                    r.surface1_uv_curves[i].as_ref(),
                    r.surface2_uv_curves[i].as_ref(),
                ],
            })
            .collect()
    }

    /// Asserts that `sys` evaluates each parametric surface as the kernel does, at two
    /// points of every curve (the certificate is about the surfaces the kernel has).
    fn assert_reads_as_kernel(
        k: &Kernel,
        sys: &System,
        s: [&Surface; 2],
        curves: &[AnswerCurve<'_>],
    ) {
        for c in curves {
            for &si in &sys.param {
                for f in [0.37, 0.61] {
                    let y = uv_point(k, sys, *c, f).unwrap();
                    let at = 2 * sys.param.iter().position(|&x| x == si).unwrap();
                    let p = Point2::new(y[at], y[at + 1]);
                    let q = s[si].evaluate_point(k, p).unwrap();
                    let m = mid3(sys.sides[si].jet(pt(p.x), pt(p.y), [None; 2]).s);
                    let d =
                        ((m[0] - q.x).powi(2) + (m[1] - q.y).powi(2) + (m[2] - q.z).powi(2)).sqrt();
                    assert!(
                        d <= 1e-12 * (1.0 + q.length()),
                        "side {si} at {p:?}: {m:?}, the kernel's {q:?}"
                    );
                }
            }
        }
    }

    /// The certified components of the answer's crossing curves, which must cover them.
    fn certified(k: &Kernel, s: [&Surface; 2]) -> (System, Vec<Component>) {
        let (d, r) = answer(k, s);
        let curves = of_type(&r, TsectCurveType::Crossing);
        assert_eq!(
            curves.len(),
            r.curves_3d.len(),
            "only crossing curves: {:?}",
            r.curve_types
        );
        let sys = System::new(k, s, [&d[0], &d[1]]);
        assert_reads_as_kernel(k, &sys, s, &curves);
        let found: Vec<Component> = components(k, &sys, &curves)
            .into_iter()
            .map(|c| c.unwrap_or_else(|e| panic!("not certified: {e}")))
            .collect();
        for c in &found {
            assert!(!disjoint(&c.tube, &c.tube), "a tube meets itself");
        }
        let mut held: Vec<usize> = found.iter().flat_map(|c| c.curves.clone()).collect();
        held.sort_unstable();
        assert_eq!(
            held,
            (0..curves.len()).collect::<Vec<_>>(),
            "each curve in one component"
        );
        (sys, found)
    }

    fn pairwise_disjoint(found: &[Component]) -> bool {
        found
            .iter()
            .enumerate()
            .all(|(i, a)| found[i + 1..].iter().all(|b| disjoint(&a.tube, &b.tube)))
    }

    /// Two squares crossing along the line `y = x / 2` in the first: one component, which
    /// leaves the first square through its two u ends.
    fn crossing_squares() -> [Surface; 2] {
        [
            plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0]),
            plane([0.0; 3], [1.0, 0.5, 0.0], [0.0, 0.0, 1.0], [2.0, 2.0]),
        ]
    }

    #[test]
    fn two_planes_cross_in_one_line() {
        let k = Kernel::default();
        let [a, b] = crossing_squares();
        let (_, found) = certified(&k, [&a, &b]);
        assert_eq!(found.len(), 1);
        let Ends::Open(ends) = found[0].tube.ends else {
            panic!("an open tube: {:?}", found[0].tube.ends);
        };
        let mut at: Vec<f64> = ends
            .iter()
            .map(|e| match e {
                Exit::Param(0, u) => *u,
                _ => panic!("an exit through the first square's u ends: {ends:?}"),
            })
            .collect();
        at.sort_by(f64::total_cmp);
        assert_eq!(at, [-1.0, 1.0]);
    }

    #[test]
    fn a_point_is_on_the_tube_or_has_no_zero_about_it() {
        let k = Kernel::default();
        let [a, b] = crossing_squares();
        let (sys, found) = certified(&k, [&a, &b]);
        let tube = &found[0].tube;
        let r = [0.05; 4];
        // (0.4, 0.2) on the first square is (0.2 sqrt 5, 0) on the second
        let on = unknowns(
            &sys,
            [Point2::new(0.4, 0.2), Point2::new(0.2 * 5f64.sqrt(), 0.0)],
        );
        assert!(on_tube(tube, &on));
        assert!(no_zero(&sys, &on, &r).is_err());
        // 0.3 off the line in the first square's v: no zero within 0.05
        let off = unknowns(
            &sys,
            [Point2::new(0.4, 0.5), Point2::new(0.2 * 5f64.sqrt(), 0.0)],
        );
        assert!(!on_tube(tube, &off));
        if let Err(e) = no_zero(&sys, &off, &r) {
            panic!("a zero near {off:?}: {e}");
        }
    }

    #[test]
    fn an_oblique_plane_cuts_a_cylinder_in_one_arc_from_seam_to_seam() {
        let k = Kernel::default();
        let c = cylinder(&k);
        // z = 2 - 0.3 x: the ellipse stays in the cylinder's heights, crosses its seam once
        let p = plane(
            [0.0, 0.0, 2.0],
            [1.0, 0.0, -0.3],
            [0.0, 1.0, 0.0],
            [3.0, 3.0],
        );
        let (_, found) = certified(&k, [&c, &p]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].tube.ends, Ends::Open([Exit::Seam, Exit::Seam]));
    }

    #[test]
    fn a_steep_plane_cuts_a_cylinder_in_two_arcs() {
        let k = Kernel::default();
        let c = cylinder(&k);
        // z = 2 - 3 x: the ellipse leaves the heights 0..4 at x = +-2/3, so two arcs (y > 0
        // and y < 0) run from the bottom to the top; the seam (x = 1, z = -1) is outside
        let p = plane(
            [0.0, 0.0, 2.0],
            [1.0, 0.0, -3.0],
            [0.0, 1.0, 0.0],
            [3.0, 3.0],
        );
        let (_, found) = certified(&k, [&p, &c]);
        assert_eq!(found.len(), 2);
        for f in &found {
            let Ends::Open(ends) = f.tube.ends else {
                panic!("an open tube: {:?}", f.tube.ends);
            };
            let mut h: Vec<f64> = ends
                .iter()
                .map(|e| match e {
                    Exit::Height(h) => h.round(),
                    _ => panic!("an exit through the heights: {ends:?}"),
                })
                .collect();
            h.sort_by(f64::total_cmp);
            assert_eq!(h, [0.0, 4.0]);
        }
        assert!(pairwise_disjoint(&found));
    }

    #[test]
    fn a_plane_along_a_cylinder_cuts_it_in_two_disjoint_lines() {
        let k = Kernel::default();
        let c = cylinder(&k);
        // x = -0.5: the lines at y = +-sqrt(3)/2, each from the bottom to the top
        let p = plane(
            [-0.5, 0.0, 2.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [3.0, 3.0],
        );
        let (_, found) = certified(&k, [&c, &p]);
        assert_eq!(found.len(), 2);
        for f in &found {
            let Ends::Open([a, b]) = f.tube.ends else {
                panic!("an open tube: {:?}", f.tube.ends);
            };
            let h = |e: Exit| match e {
                Exit::Height(h) => h.round(),
                _ => panic!("an exit through the heights: {:?}", f.tube.ends),
            };
            assert_eq!((h(a) - h(b)).abs(), 4.0, "{:?}", f.tube.ends);
        }
        assert!(pairwise_disjoint(&found));
    }

    #[test]
    fn a_plane_across_an_extruded_profile_cuts_it_in_three_disjoint_lines() {
        let k = Kernel::default();
        let e = extruded_polyline(&k, &[[0.0, 0.0], [1.0, 0.3], [2.0, -0.2], [3.0, 0.1]]);
        // y = 0.05 + (z - 1) / 50 crosses the profile three times (near x = 1/6, 1.5 and
        // 17/6): three lines from the extrusion's v = 0 to v = 2 (no cone: disjoint by the
        // boxes alone; tilted off z, where the intersector gives no UV curves)
        let p = plane(
            [0.0, 0.05, 1.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.02, 1.0],
            [5.0, 3.0],
        );
        let (d, r) = answer(&k, [&e, &p]);
        let (_, found) = certified(&k, [&e, &p]);
        assert_eq!(found.len(), 3, "{} curves", r.curves_3d.len());
        let v = d[0].v_interval();
        let (a, b) = (Exit::Param(1, v.min()), Exit::Param(1, v.max()));
        for f in &found {
            let ends = f.tube.ends;
            assert!(
                ends == Ends::Open([a, b]) || ends == Ends::Open([b, a]),
                "{ends:?}"
            );
        }
        assert!(pairwise_disjoint(&found));
    }

    #[test]
    fn a_plane_cuts_a_dome_in_a_closed_loop() {
        let k = Kernel::default();
        let s = dome();
        let p = plane(
            [1.5, 1.5, 0.5],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [3.0, 3.0],
        );
        let (_, found) = certified(&k, [&s, &p]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].tube.ends, Ends::Closed);
    }

    #[test]
    fn curves_split_at_a_profile_corner_are_one_component() {
        let k = Kernel::default();
        let e = extruded_polyline(&k, &[[0.0, 0.0], [1.0, 1.0], [2.0, -1.0], [3.0, 0.0]]);
        // z = 0.5 + x / 4 runs across the extrusion, through both corners' knot lines
        let p = plane(
            [0.0, 0.0, 0.5],
            [1.0, 0.0, 0.25],
            [0.0, 1.0, 0.0],
            [5.0, 3.0],
        );
        let (d, r) = answer(&k, [&e, &p]);
        assert!(
            r.curves_3d.len() > 1,
            "the intersector splits at the corners: {}",
            r.curves_3d.len()
        );
        let (_, found) = certified(&k, [&e, &p]);
        assert_eq!(
            found.len(),
            1,
            "one component of {} curves",
            r.curves_3d.len()
        );
        let u = d[0].u_interval();
        let (a, b) = (Exit::Param(0, u.min()), Exit::Param(0, u.max()));
        let ends = found[0].tube.ends;
        assert!(
            ends == Ends::Open([a, b]) || ends == Ends::Open([b, a]),
            "{ends:?}"
        );
    }

    /// The plane across the extruded profile with corners, and its zeros nearest the corner
    /// at u = 1 (the extrusion's u is unknown 0) on either side.
    fn zeros_by_a_corner(k: &Kernel) -> (System, Vec<f64>, Vec<f64>) {
        let e = extruded_polyline(k, &[[0.0, 0.0], [1.0, 1.0], [2.0, -1.0], [3.0, 0.0]]);
        let p = plane(
            [0.0, 0.0, 0.5],
            [1.0, 0.0, 0.25],
            [0.0, 1.0, 0.0],
            [5.0, 3.0],
        );
        let (d, r) = answer(k, [&e, &p]);
        let sys = System::new(k, [&e, &p], [&d[0], &d[1]]);
        let zeros: Vec<Vec<f64>> = of_type(&r, TsectCurveType::Crossing)
            .into_iter()
            .flat_map(|c| (1..10).map(move |j| (c, f64::from(j) / 10.0)))
            .filter_map(|(c, f)| {
                let y = uv_point(k, &sys, c, f).ok()?;
                newton(&sys, &y, Some(choose_t(&sys, &y)), None)
            })
            .collect();
        let near = |below: bool| {
            zeros
                .iter()
                .filter(|y| (y[0] < 1.0) == below && y[0] > 0.0 && y[0] < 2.0)
                .min_by(|a, b| (a[0] - 1.0).abs().total_cmp(&(b[0] - 1.0).abs()))
                .unwrap()
                .clone()
        };
        let (a, b) = (near(true), near(false));
        (sys, a, b)
    }

    #[test]
    fn two_boxes_cross_a_knot_line_each_on_its_own_piece() {
        let k = Kernel::default();
        let (sys, a, b) = zeros_by_a_corner(&k);
        let [x, y] = across(&sys, &a, &b, 0).expect("two boxes across the knot line");
        assert_eq!([x.t.hi, y.t.lo], [1.0, 1.0]);
        assert_eq!([x.on_face, y.on_face], [[false, true], [true, false]]);
        assert_ne!(x.force[0], y.force[0], "each box on its own span");
    }

    #[test]
    fn reopening_a_knot_line_pair_takes_both_boxes() {
        let k = Kernel::default();
        let (sys, a, b) = zeros_by_a_corner(&k);
        // a box before the pair, over the arc from a zero further down the first piece
        let mut c = a.clone();
        c[0] -= 0.5;
        let c = newton(&sys, &c, Some(0), None).expect("a zero before the corner");
        let before = segment(&sys, &c, &a, 0).expect("a box before the corner");
        let [x, y] = across(&sys, &a, &b, 0).expect("two boxes across the knot line");
        let sg = |a: &[f64], b: &[f64]| Seg {
            a: a.to_vec(),
            b: b.to_vec(),
            depth: 0,
            outer: [false; 2],
        };
        // as the chain holds them: the pair shares its piece
        let mut boxes = vec![(before, sg(&c, &a)), (x, sg(&a, &b)), (y, sg(&a, &b))];
        let piece = reopen(&mut boxes).expect("a box to reopen");
        assert_eq!([piece.a, piece.b], [a, b]);
        // the pair's first box would keep its end on the knot line without its partner,
        // and `covered` takes such an end as met
        let left: Vec<[bool; 2]> = boxes.iter().map(|(bx, _)| bx.on_face).collect();
        assert_eq!(left, [[false; 2]], "only the box before the pair is left");
    }

    #[test]
    fn a_tangent_line_is_not_certified() {
        let k = Kernel::default();
        let c = cylinder(&k);
        // y = 1 touches the cylinder along its line at 90 degrees
        let p = plane(
            [0.0, 1.0, 2.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [3.0, 3.0],
        );
        let (d, r) = answer(&k, [&c, &p]);
        assert_eq!(r.curve_types, [TsectCurveType::Tangent]);
        let curves = of_type(&r, TsectCurveType::Tangent);
        let sys = System::new(&k, [&c, &p], [&d[0], &d[1]]);
        let found = components(&k, &sys, &curves);
        assert_eq!(found.len(), 1);
        let Err(e) = &found[0] else {
            panic!("a tube along a tangent line");
        };
        // Newton finds no zero where the Jacobian is singular, or no box closes
        assert!(
            e.ends_with("no zeros along it") || e.starts_with("no box over the arc"),
            "{e}"
        );
    }
}
