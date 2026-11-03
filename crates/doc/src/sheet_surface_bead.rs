//! Formed beads (`sheet.bead`) on a sheet made from surfaces (#6748): a stiffening bead pressed
//! into one planar panel along a straight centreline, kept on the panel as a form: its
//! centreline in the panel's chart, its section, and the face it stands out of. Across, it is a
//! trapezoid with filleted corners; along, each end ramps up from the face over the end length.
//! The sheet keeps its thickness: the skin that stands out (the raised skin) and the skin under
//! it are concentric, and both meet the panel's faces on one rectangle, the footprint. The
//! folded solid leaves the footprint open in its panel's faces and sews the bead's faces in
//! (see [`crate::sheet_surface_fold::solid`]). In the flat the blank stays flat: the flat
//! pattern lists the bead, and the DXF draws it.
//!
//! In a bead's own frame ξ runs across (0 on the centreline), η along it (0 and `len` at the
//! ends' virtual sharps on the face) and h out of the face it stands out of (the other face is at
//! h = −t). Each skin is the lower of two profiles, P across (in ξ) and Q along (in η); where
//! they meet, at the footprint's four corners, the creases are traced as cubic Bézier chains.

use std::f64::consts::{FRAC_PI_2, PI};

use serde::{Deserialize, Serialize};
use solidcraft_develop::{Fold, Surface};
use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel::{EdgeSpec, FaceSpec, SurfSpec};

use crate::sheet_surface::Piece;
use crate::{DocError, Result, SurfaceSheet};

/// The narrowest a bead's crown, and the shortest its crown line, may be (mm).
const MIN_CROWN: f64 = 1e-3;
/// How far a traced crease may stand off the two faces it joins (mm), as a bend's edges may.
const CREASE_TOL: f64 = 1e-7;
/// Most cubic pieces in one crease.
const MAX_CREASE_PIECES: usize = 512;
/// Most beads on one sheet.
pub const MAX_BEADS: usize = 200;
/// How far an extruded skin's surface runs past its face, so the boundary lies well inside it.
const PAD: f64 = 1.0;

/// A bead on a sheet made from surfaces, in its panel's chart (see the module notes).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceBead {
    pub feature: u64,
    /// The panel it is pressed into.
    pub panel: usize,
    /// The centreline's ends in the panel's chart: where the end ramps' virtual sharps meet the
    /// face.
    pub a: Vec2,
    pub b: Vec2,
    /// It stands out of the panel's far face (the one a thickness into the material), else out
    /// of its near face (the sheet's surface).
    pub far: bool,
    /// From the face it stands out of to its crown on that side.
    pub depth: f64,
    /// Across, between the flanks' virtual sharps on that face.
    pub width: f64,
    /// The flanks' angle to the face (radians).
    pub angle: f64,
    /// The inner radius of the bends at the base and at the crown.
    pub base_radius: f64,
    pub crown_radius: f64,
    /// Along the centreline, from an end's virtual sharp on the face to the virtual sharp where
    /// the ramp reaches the crown.
    pub end_length: f64,
}

/// One piece of a profile in (u, h): u from the footprint's edge inward, h out of the face the
/// bead stands out of.
#[derive(Clone, Copy, Debug)]
enum Seg1 {
    Line {
        u0: f64,
        h0: f64,
        u1: f64,
        h1: f64,
    },
    /// Centre (cu, ch), radius r, from angle a0 to a1.
    Arc {
        cu: f64,
        ch: f64,
        r: f64,
        a0: f64,
        a1: f64,
    },
}

impl Seg1 {
    fn at(&self, s: f64) -> (f64, f64) {
        match *self {
            Seg1::Line { u0, h0, u1, h1 } => (u0 + (u1 - u0) * s, h0 + (h1 - h0) * s),
            Seg1::Arc { cu, ch, r, a0, a1 } => {
                let a = a0 + (a1 - a0) * s;
                (cu + r * a.cos(), ch + r * a.sin())
            }
        }
    }

    /// The angle of an arc at height h, on the side of the circle the arc runs on.
    fn angle_at(ch_r: (f64, f64), a0: f64, a1: f64, h: f64) -> f64 {
        let (ch, r) = ch_r;
        let x = ((h - ch) / r).clamp(-1.0, 1.0);
        if ((a0 + a1) / 2.0).cos() >= 0.0 { x.asin() } else { PI - x.asin() }
    }

    /// u at height h.
    fn u_at(&self, h: f64) -> f64 {
        match *self {
            Seg1::Line { u0, h0, u1, h1 } => u0 + (h - h0) * (u1 - u0) / (h1 - h0),
            Seg1::Arc { cu, ch, r, a0, a1 } => cu + r * Self::angle_at((ch, r), a0, a1, h).cos(),
        }
    }

    /// The tangent at parameter s (any length), the way s runs.
    fn tangent(&self, s: f64) -> (f64, f64) {
        match *self {
            Seg1::Line { u0, h0, u1, h1 } => (u1 - u0, h1 - h0),
            Seg1::Arc { r, a0, a1, .. } => {
                let a = a0 + (a1 - a0) * s;
                let k = r * (a1 - a0);
                (-a.sin() * k, a.cos() * k)
            }
        }
    }
}

/// One skin's profile: rising from the footprint's edge (u = 0, h = `base`) to the crown
/// (u = `top_u`, h = `top`): the base bend, the flank and the crown bend.
#[derive(Clone, Debug)]
struct Profile {
    segs: [Seg1; 3],
    base: f64,
    top: f64,
    top_u: f64,
}

impl Profile {
    /// A skin of a bead rising `depth` at `angle`, with inner radii `rb` (base) and `rc`
    /// (crown), in a sheet `t` thick: the raised one (its face at h = 0) or the one under it.
    fn new(angle: f64, depth: f64, rb: f64, rc: f64, t: f64, raised: bool) -> Profile {
        let ro = rc + t;
        let half = (angle / 2.0).tan();
        let uc = rb * half + depth / angle.tan() + ro * half;
        let (r_base, r_crown, base, top) = if raised { (rb, ro, 0.0, depth) } else { (rb + t, rc, -t, depth - t) };
        let (s, c) = angle.sin_cos();
        let (bu, bh) = (r_base * s, rb - r_base * c);
        let (cu, chh) = (uc - r_crown * s, depth - ro + r_crown * c);
        Profile {
            segs: [
                Seg1::Arc { cu: 0.0, ch: rb, r: r_base, a0: -FRAC_PI_2, a1: -FRAC_PI_2 + angle },
                Seg1::Line { u0: bu, h0: bh, u1: cu, h1: chh },
                Seg1::Arc { cu: uc, ch: depth - ro, r: r_crown, a0: FRAC_PI_2 + angle, a1: FRAC_PI_2 },
            ],
            base,
            top,
            top_u: uc,
        }
    }

    /// The heights where the pieces meet, from the base to the top.
    fn breaks(&self) -> [f64; 4] {
        [self.base, self.segs[1].at(0.0).1, self.segs[2].at(0.0).1, self.top]
    }

    /// The piece holding the heights from h up (the upper one at a joint).
    fn piece(&self, h: f64) -> usize {
        let b = self.breaks();
        if h < b[1] - 1e-12 {
            0
        } else if h < b[2] - 1e-12 {
            1
        } else {
            2
        }
    }
}

/// A bead's numbers, worked out for a sheet `t` thick.
#[derive(Clone, Debug)]
pub(crate) struct BeadGeom {
    /// Along: the centreline's length; across and along unit vectors in the panel's chart.
    pub len: f64,
    pub ex: Vec2,
    pub ey: Vec2,
    /// Half the footprint's width, and how far the footprint runs past each end.
    pub xf: f64,
    pub sq: f64,
    /// Across and along, raised skin then the one under it.
    p: [Profile; 2],
    q: [Profile; 2],
}

impl BeadGeom {
    /// The bead's shape, or why it cannot be made (each message names what would fit).
    pub fn new(b: &SurfaceBead, t: f64) -> Result<BeadGeom> {
        let bad = |m: String| Err(DocError::Invalid(m));
        let d = b.b - b.a;
        let len = d.len();
        let finite = [b.depth, b.width, b.angle, b.base_radius, b.crown_radius, b.end_length, len, t].iter().all(|v| v.is_finite());
        if !finite || !(t > 0.0) {
            return bad("a bead needs finite sizes on a sheet of positive thickness".into());
        }
        if !(len > 1e-6) {
            return bad("a bead's centreline has no length".into());
        }
        if !(b.depth > 0.0 && b.width > 0.0 && b.end_length > 0.0) {
            return bad(format!(
                "a bead needs a depth, a width and an end length above 0 (depth {}, width {}, end length {})",
                b.depth, b.width, b.end_length
            ));
        }
        if !(b.angle > 1e-3 && b.angle < FRAC_PI_2 - 1e-3) {
            return bad(format!("a bead's flanks need an angle between 0° and 90° (got {:.3}°)", b.angle.to_degrees()));
        }
        if !(b.base_radius > 0.0 && b.crown_radius > 0.0) {
            return bad("not supported yet: a bead's radii must be more than 0".into());
        }
        let (alpha, beta) = (b.angle, (b.depth / b.end_length).atan());
        let (rb, rc) = (b.base_radius, b.crown_radius);
        let ro = rc + t;
        // The flanks and the end ramps have length only if the bends' rises leave room.
        let rise = |a: f64| (rb + ro) * (1.0 - a.cos());
        if b.depth <= rise(alpha) + 1e-6 {
            return bad(format!(
                "a bead {} deep is too shallow for its radii at {:.3}°: at least {:.4} deep, or smaller radii",
                b.depth,
                alpha.to_degrees(),
                rise(alpha) + 1e-6
            ));
        }
        if b.depth <= rise(beta) + 1e-6 {
            return bad(format!(
                "a bead {} deep is too shallow for its radii on its end ramps ({:.3}°): lengthen the end length or make the radii smaller",
                b.depth,
                beta.to_degrees()
            ));
        }
        let half = |a: f64| (a / 2.0).tan();
        let crown = b.width / 2.0 - b.depth / alpha.tan() - ro * half(alpha);
        if crown < MIN_CROWN / 2.0 {
            return bad(format!(
                "a bead {} wide leaves no crown at {:.3}°, {} deep: at least {:.4} wide",
                b.width,
                alpha.to_degrees(),
                b.depth,
                2.0 * (b.depth / alpha.tan() + ro * half(alpha)) + MIN_CROWN
            ));
        }
        let ramp = b.end_length + ro * half(beta);
        if len - 2.0 * ramp < MIN_CROWN {
            return bad(format!("a bead {len:.4} long is too short for its two end ramps: at least {:.4}", 2.0 * ramp + MIN_CROWN));
        }
        let ey = d * (1.0 / len);
        let ex = Vec2::new(ey.y, -ey.x);
        let p = [Profile::new(alpha, b.depth, rb, rc, t, true), Profile::new(alpha, b.depth, rb, rc, t, false)];
        let q = [Profile::new(beta, b.depth, rb, rc, t, true), Profile::new(beta, b.depth, rb, rc, t, false)];
        Ok(BeadGeom { len, ex, ey, xf: b.width / 2.0 + rb * half(alpha), sq: rb * half(beta), p, q })
    }

    /// The footprint's corners in the panel's chart, counter-clockwise.
    pub fn footprint(&self, b: &SurfaceBead) -> [Vec2; 4] {
        let at = |x: f64, y: f64| b.a + self.ex * x + self.ey * y;
        let (x, y0, y1) = (self.xf, -self.sq, self.len + self.sq);
        [at(-x, y0), at(x, y0), at(x, y1), at(-x, y1)]
    }

    /// A point of the panel's chart in the bead's frame: (across, along).
    pub fn local(&self, b: &SurfaceBead, p: Vec2) -> (f64, f64) {
        ((p - b.a).dot(self.ex), (p - b.a).dot(self.ey))
    }

    /// The crown's width and the length of its line between the ramps.
    pub fn crown(&self) -> (f64, f64) {
        (2.0 * (self.xf - self.p[0].top_u), self.len + 2.0 * self.sq - 2.0 * self.q[0].top_u)
    }
}

/// A crease, from where its P and Q meet low to where they meet high: a chain in the bead's frame
/// for the corner at the start of the left side, with its ends.
struct Crease {
    chain: Vec<Vec3>,
}

/// The point of the left-start crease (ξ, η, h) on the piece where P's piece `kp` meets Q's
/// piece `kq`, by a parameter that runs smoothly along it: the shared angle where two bends of one
/// radius and centre height meet (a mitre), else an arc's angle, else the height.
#[derive(Clone, Copy)]
enum Param {
    Mitre { r: f64, ch: f64, cp: f64, cq: f64 },
    PArc,
    QArc,
    Height,
}

fn crease_param(sp: &Seg1, sq: &Seg1) -> Param {
    match (sp, sq) {
        (Seg1::Arc { cu: cp, ch, r, .. }, Seg1::Arc { cu: cq, ch: chq, r: rq, .. }) if (r - rq).abs() < 1e-12 && (ch - chq).abs() < 1e-12 => {
            Param::Mitre { r: *r, ch: *ch, cp: *cp, cq: *cq }
        }
        (_, Seg1::Arc { .. }) => Param::QArc,
        (Seg1::Arc { .. }, _) => Param::PArc,
        _ => Param::Height,
    }
}

impl BeadGeom {
    /// The crease of one skin between heights `h0` and `h1`, traced to [`CREASE_TOL`].
    fn crease(&self, skin: usize, h0: f64, h1: f64) -> Result<Crease> {
        let (p, q) = (&self.p[skin], &self.q[skin]);
        let mid = (h0 + h1) / 2.0;
        let (sp, sq) = (p.segs[p.piece(mid)], q.segs[q.piece(mid)]);
        let param = crease_param(&sp, &sq);
        let (xf, sqo) = (self.xf, self.sq);
        let point = move |s: f64| -> Vec3 {
            let (up, vq, h) = match param {
                Param::Mitre { r, ch, cp, cq } => (cp + r * s.cos(), cq + r * s.cos(), ch + r * s.sin()),
                Param::PArc => {
                    let Seg1::Arc { cu, ch, r, .. } = sp else { return Vec3::ZERO };
                    let h = ch + r * s.sin();
                    (cu + r * s.cos(), sq.u_at(h), h)
                }
                Param::QArc => {
                    let Seg1::Arc { cu, ch, r, .. } = sq else { return Vec3::ZERO };
                    let h = ch + r * s.sin();
                    (sp.u_at(h), cu + r * s.cos(), h)
                }
                Param::Height => (sp.u_at(s), sq.u_at(s), s),
            };
            Vec3::new(-xf + up, -sqo + vq, h)
        };
        let s_at = |h: f64| match (param, sp, sq) {
            (Param::Mitre { .. } | Param::PArc, Seg1::Arc { ch, r, a0, a1, .. }, _) => Seg1::angle_at((ch, r), a0, a1, h),
            (Param::QArc, _, Seg1::Arc { ch, r, a0, a1, .. }) => Seg1::angle_at((ch, r), a0, a1, h),
            _ => h,
        };
        let (s0, s1) = (s_at(h0), s_at(h1));
        let delta = (s1 - s0).abs().max(1e-12) * 1e-5;
        let deriv = |s: f64| (point(s + delta) - point(s - delta)) * (0.5 / delta);
        let cubic = |a: f64, b: f64| -> [Vec3; 4] {
            let (pa, pb) = (point(a), point(b));
            let k = (b - a) / 3.0;
            [pa, pa + deriv(a) * k, pb - deriv(b) * k, pb]
        };
        let bez = |c: &[Vec3; 4], u: f64| {
            let v = 1.0 - u;
            c[0] * (v * v * v) + c[1] * (3.0 * v * v * u) + c[2] * (3.0 * v * u * u) + c[3] * (u * u * u)
        };
        let mut n = 2usize;
        loop {
            let mut worst = 0.0f64;
            let pieces: Vec<[Vec3; 4]> =
                (0..n).map(|k| cubic(s0 + (s1 - s0) * k as f64 / n as f64, s0 + (s1 - s0) * (k + 1) as f64 / n as f64)).collect();
            for (k, c) in pieces.iter().enumerate() {
                for j in 1..4 {
                    let u = j as f64 / 4.0;
                    let s = s0 + (s1 - s0) * (k as f64 + u) / n as f64;
                    worst = worst.max(bez(c, u).dist(point(s)));
                }
            }
            if worst <= CREASE_TOL || n >= MAX_CREASE_PIECES {
                if !worst.is_finite() || worst > 1e-4 {
                    return Err(DocError::Invalid(format!("a bead's crease could not be traced (off by {worst:e} mm)")));
                }
                let mut chain = Vec::with_capacity(3 * n + 1);
                for (k, c) in pieces.iter().enumerate() {
                    chain.extend_from_slice(if k == 0 { &c[..] } else { &c[1..] });
                }
                return Ok(Crease { chain });
            }
            n = (n * 2).min(MAX_CREASE_PIECES);
        }
    }
}

/// Mirror a point of the left-start corner into corner `c` (0 left start, 1 right start, 2 left
/// end, 3 right end).
fn corner(p: Vec3, c: usize, len: f64) -> Vec3 {
    let x = if c % 2 == 1 { -p.x } else { p.x };
    let y = if c >= 2 { len - p.y } else { p.y };
    Vec3::new(x, y, p.z)
}

/// Reverse a loop of edges.
fn reversed_loop(l: Vec<EdgeSpec>) -> Vec<EdgeSpec> {
    l.into_iter().rev().map(|e| e.reversed()).collect()
}

/// The control points (or ends) of an edge, for a rough normal of its loop.
fn edge_points(e: &EdgeSpec) -> Vec<Vec3> {
    match e {
        EdgeSpec::Line { a, .. } => vec![*a],
        EdgeSpec::Arc { a, mid, .. } => vec![*a, *mid],
        EdgeSpec::Chain { ctrl, .. } => ctrl.iter().take(ctrl.len().saturating_sub(1)).copied().collect(),
    }
}

/// A loop's area vector (Newell): its normal, as long as the area it bounds.
fn newell(l: &[EdgeSpec]) -> Vec3 {
    let pts: Vec<Vec3> = l.iter().flat_map(edge_points).collect();
    let mut n = Vec3::ZERO;
    for (p, q) in pts.iter().zip(pts.iter().cycle().skip(1)) {
        n += Vec3::new((p.y - q.y) * (p.z + q.z), (p.z - q.z) * (p.x + q.x), (p.x - q.x) * (p.y + q.y));
    }
    n * 0.5
}

/// A face of a bead, in its frame: its loop (the right way round about `normal`), its surface
/// and a point on it.
struct LocalFace {
    edges: Vec<EdgeSpec>,
    surface: SurfSpec,
    probe: (Vec3, Vec3),
}

/// One skin's faces in the bead's frame (0: raised, 1: under it).
fn skin_faces(g: &BeadGeom, skin: usize) -> Result<Vec<LocalFace>> {
    let (p, q) = (&g.p[skin], &g.q[skin]);
    let outward = if skin == 0 { 1.0 } else { -1.0 };
    // The heights where either profile changes piece, from the base to the top.
    let mut hs: Vec<f64> = p.breaks().iter().chain(q.breaks().iter()).copied().collect();
    hs.sort_by(|a, b| a.total_cmp(b));
    hs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let n = hs.len();
    if n < 2 {
        return Err(DocError::Invalid("a bead has no height".into()));
    }
    // The left-start crease's pieces and its points at those heights; the base and the top
    // exactly at the footprint's and the crown's corners.
    let creases = (0..n - 1).map(|j| g.crease(skin, hs[j], hs[j + 1])).collect::<Result<Vec<_>>>()?;
    let mut verts: Vec<Vec3> = Vec::with_capacity(n);
    verts.push(Vec3::new(-g.xf, -g.sq, p.base));
    verts.extend(creases.iter().skip(1).map(|c| c.chain.first().copied().unwrap_or(Vec3::ZERO)));
    verts.push(Vec3::new(-g.xf + p.top_u, -g.sq + q.top_u, p.top));
    let vert = |c: usize, j: usize| corner(verts[j], c, g.len);
    // Corner c's crease piece j, from height j to j + 1, its ends exactly on the vertices.
    let crease = |c: usize, j: usize| -> EdgeSpec {
        let mut ctrl: Vec<Vec3> = creases[j].chain.iter().map(|p| corner(*p, c, g.len)).collect();
        if let Some(first) = ctrl.first_mut() {
            *first = vert(c, j);
        }
        if let Some(last) = ctrl.last_mut() {
            *last = vert(c, j + 1);
        }
        EdgeSpec::Chain { degree: 3, ctrl: ctrl.into() }
    };
    let index = |h: f64| hs.iter().position(|x| (x - h).abs() < 1e-9).unwrap_or(0);
    let line = |a: Vec3, b: Vec3| EdgeSpec::Line { a, b };
    let mut faces = Vec::new();
    let mut push = |mut edges: Vec<EdgeSpec>, surface: SurfSpec, probe: (Vec3, Vec3)| {
        if newell(&edges).dot(probe.1) < 0.0 {
            edges = reversed_loop(edges);
        }
        faces.push(LocalFace { edges, surface, probe });
    };
    let pb = p.breaks();
    let qb = q.breaks();
    for k in 0..3 {
        // Across: the piece of P on each side, between creases at the start and at the end.
        let (j0, j1) = (index(pb[k]), index(pb[k + 1]));
        for (side, (cs, ce)) in [(0usize, 2usize), (1, 3)].into_iter().enumerate() {
            let mut edges = vec![line(vert(cs, j0), vert(ce, j0))];
            edges.extend((j0..j1).map(|j| crease(ce, j)));
            edges.push(line(vert(ce, j1), vert(cs, j1)));
            edges.extend((j0..j1).rev().map(|j| crease(cs, j).reversed()));
            let sign = if side == 0 { 1.0 } else { -1.0 };
            let seg = p.segs[k];
            let x = |u: f64| sign * (-g.xf + u);
            let surface = match seg {
                Seg1::Line { .. } => SurfSpec::Plane,
                Seg1::Arc { .. } => {
                    let pt = |s: f64| {
                        let (u, h) = seg.at(s);
                        Vec3::new(x(u), -g.sq - PAD, h)
                    };
                    SurfSpec::Extruded { a: pt(0.0), b: pt(1.0), mid: pt(0.5), dir: Vec3::new(0.0, g.len + 2.0 * g.sq + 2.0 * PAD, 0.0) }
                }
            };
            let (u, h) = seg.at(0.5);
            let (du, dh) = seg.tangent(0.5);
            let normal = Vec3::new(-dh * sign, 0.0, du) * outward;
            push(edges, surface, (Vec3::new(x(u), g.len / 2.0, h), normal));
        }
        // Along: the piece of Q at each end, between the left and the right crease.
        let (j0, j1) = (index(qb[k]), index(qb[k + 1]));
        for (end, (cl, cr)) in [(0usize, 1usize), (2, 3)].into_iter().enumerate() {
            let mut edges = vec![line(vert(cl, j0), vert(cr, j0))];
            edges.extend((j0..j1).map(|j| crease(cr, j)));
            edges.push(line(vert(cr, j1), vert(cl, j1)));
            edges.extend((j0..j1).rev().map(|j| crease(cl, j).reversed()));
            let sign = if end == 0 { 1.0 } else { -1.0 };
            let seg = q.segs[k];
            let y = |v: f64| if end == 0 { -g.sq + v } else { g.len + g.sq - v };
            let surface = match seg {
                Seg1::Line { .. } => SurfSpec::Plane,
                Seg1::Arc { .. } => {
                    let pt = |s: f64| {
                        let (v, h) = seg.at(s);
                        Vec3::new(-g.xf - PAD, y(v), h)
                    };
                    SurfSpec::Extruded { a: pt(0.0), b: pt(1.0), mid: pt(0.5), dir: Vec3::new(2.0 * g.xf + 2.0 * PAD, 0.0, 0.0) }
                }
            };
            let (v, h) = seg.at(0.5);
            let (dv, dh) = seg.tangent(0.5);
            let normal = Vec3::new(0.0, -dh * sign, dv) * outward;
            push(edges, surface, (Vec3::new(0.0, y(v), h), normal));
        }
    }
    // The crown: a rectangle at the top.
    let top = n - 1;
    let edges =
        vec![line(vert(0, top), vert(1, top)), line(vert(1, top), vert(3, top)), line(vert(3, top), vert(2, top)), line(vert(2, top), vert(0, top))];
    push(edges, SurfSpec::Plane, (Vec3::new(0.0, g.len / 2.0, p.top), Vec3::new(0.0, 0.0, outward)));
    Ok(faces)
}

/// What a bead does to the folded solid: the footprint its panel's cell leaves open (in the
/// flat, clockwise as a cell's holes run) and the bead's faces that close it, in space.
pub(crate) struct Form {
    pub panel: usize,
    pub open: Vec<Seg2>,
    pub faces: Vec<FaceSpec>,
}

/// The folded solid's forms, one for each of the sheet's beads, after each is checked against
/// what the development leaves of its panel.
pub(crate) fn forms(sk: &SurfaceSheet, pieces: &[(Piece, Region2)], fold: &Fold) -> Result<Vec<Form>> {
    let t = sk.sheet.thickness;
    let mut out = Vec::with_capacity(sk.beads.len());
    for b in &sk.beads {
        sk.check_bead(b, pieces, fold, None, &mut Vec::new())?;
        let g = BeadGeom::new(b, t)?;
        let fp = g.footprint(b);
        let flat = |q: Vec2| fold.flat_point(b.panel, q).ok_or_else(|| DocError::Invalid("a bead's panel has no place in the flat".into()));
        let corners = [flat(fp[0])?, flat(fp[3])?, flat(fp[2])?, flat(fp[1])?];
        let open = (0..4).map(|i| Seg2::Line { a: corners[i], b: corners[(i + 1) % 4] }).collect();
        // A point of the bead's frame (ξ, η, h) in space. Depth runs into the material from the
        // panel's surface: out of the far face is t + h, out of the near face −h, with ξ turned
        // so that the frame keeps its hand.
        let w = |p: Vec3| -> Result<Vec3> {
            let x = if b.far { p.x } else { -p.x };
            let q = b.a + g.ex * x + g.ey * p.y;
            let depth = if b.far { t + p.z } else { -p.z };
            fold.surface_point(b.panel, q, depth)
                .filter(|v| v.is_finite())
                .ok_or_else(|| DocError::Invalid("a bead's panel has no place on the folded sheet".into()))
        };
        let edge = |e: &EdgeSpec| -> Result<EdgeSpec> {
            Ok(match e {
                EdgeSpec::Line { a, b: e1 } => EdgeSpec::Line { a: w(*a)?, b: w(*e1)? },
                EdgeSpec::Arc { a, b: e1, mid } => EdgeSpec::Arc { a: w(*a)?, b: w(*e1)?, mid: w(*mid)? },
                EdgeSpec::Chain { degree, ctrl } => EdgeSpec::Chain { degree: *degree, ctrl: ctrl.iter().map(|p| w(*p)).collect::<Result<_>>()? },
            })
        };
        let mut faces = Vec::new();
        for skin in 0..2 {
            for f in skin_faces(&g, skin)? {
                let edges: Vec<EdgeSpec> = f.edges.iter().map(&edge).collect::<Result<_>>()?;
                let surface = match f.surface {
                    SurfSpec::Extruded { a, b: e1, mid, dir } => SurfSpec::Extruded { a: w(a)?, b: w(e1)?, mid: w(mid)?, dir: w(a + dir)? - w(a)? },
                    s => s,
                };
                let (p, n) = f.probe;
                faces.push(FaceSpec { loops: vec![edges], surface, probe: Some((w(p)?, w(p + n)? - w(p)?)) });
            }
        }
        out.push(Form { panel: b.panel, open, faces });
    }
    Ok(out)
}

impl SurfaceSheet {
    /// A bead's footprint in the flat (counter-clockwise), its centreline's ends there, its
    /// crown's width and its length between the end ramps.
    pub fn bead_outline(&self, b: &SurfaceBead) -> Result<([Vec2; 4], [Vec2; 2], f64, f64)> {
        let g = BeadGeom::new(b, self.sheet.thickness)?;
        let (w, l) = g.crown();
        let dev = solidcraft_develop::develop(&self.sheet, 0).map_err(|e| DocError::Invalid(e.to_string()))?;
        let fold = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places)?;
        let fp = g.footprint(b);
        let flat = |q: Vec2| fold.flat_point(b.panel, q).ok_or_else(|| DocError::Invalid("a bead's panel has no place in the flat".into()));
        Ok(([flat(fp[0])?, flat(fp[1])?, flat(fp[2])?, flat(fp[3])?], [flat(b.a)?, flat(b.b)?], w, l))
    }

    /// Where a straight sketch line from `a` to `b` (in space) lies on the sheet: the planar
    /// panel whose face the sketch's `plane` lies on and that holds the line's middle, the line's
    /// ends in that panel's chart, and whether the plane is (nearer) its far face.
    pub fn bead_line(&self, plane: &Plane, a: Vec3, b: Vec3) -> Result<(usize, Vec2, Vec2, bool)> {
        let dev = solidcraft_develop::develop(&self.sheet, 0).map_err(|e| DocError::Invalid(e.to_string()))?;
        let fold = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places)?;
        let t = self.sheet.thickness;
        for (i, (panel, place)) in self.sheet.panels.iter().zip(&fold.places).enumerate() {
            let (Surface::Plane, Some(place)) = (&panel.surface, place) else { continue };
            if place.z.cross(plane.normal()).len() > 1e-6 {
                continue;
            }
            let inv = place.inverse();
            let depth = inv.point(plane.origin).z;
            let far = (depth - t).abs() < 1e-6;
            if !(depth.abs() < 1e-6 || far) {
                continue;
            }
            let (qa, qb) = (inv.point(a), inv.point(b));
            let (qa, qb) = (Vec2::new(qa.x, qa.y), Vec2::new(qb.x, qb.y));
            if panel.region.contains((qa + qb) * 0.5) {
                return Ok((i, qa, qb, far));
            }
        }
        Err(DocError::Invalid("a bead's centreline must lie on a face of a planar panel of the sheet".into()))
    }

    /// Checks a new bead `b` as [`SurfaceSheet::check_bead`] does, laying the sheet out first;
    /// `notes` gets what lies within `margin` of it, and how far it goes past the usual forming
    /// limits.
    pub(crate) fn check_new_bead(&self, b: &SurfaceBead, margin: f64, notes: &mut Vec<String>) -> Result<()> {
        let dev = solidcraft_develop::develop(&self.sheet, 0).map_err(|e| DocError::Invalid(e.to_string()))?;
        let pieces = crate::sheet_surface::pieces(&dev, &mut crate::sheet::FlatWork::new())?;
        let fold = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places)?;
        self.check_bead(b, &pieces, &fold, Some(margin), notes)
    }

    /// Whether bead `b` can be pressed: on a planar panel, its footprint inside what the
    /// development leaves of that panel, clear of the sheet's edges, its cuts, its bends'
    /// zones, its recesses and the other beads. With a `margin`, `notes` gets what lies closer
    /// than that, and how far the bead goes past the usual forming limits.
    pub(crate) fn check_bead(
        &self,
        b: &SurfaceBead,
        pieces: &[(Piece, Region2)],
        fold: &Fold,
        margin: Option<f64>,
        notes: &mut Vec<String>,
    ) -> Result<()> {
        let t = self.sheet.thickness;
        let panel = self.sheet.panels.get(b.panel).ok_or_else(|| DocError::Invalid("a bead's panel is not on the sheet".into()))?;
        if !matches!(panel.surface, Surface::Plane) {
            return Err(DocError::Invalid("not supported yet: a bead on a rolled or free-form panel".into()));
        }
        let g = BeadGeom::new(b, t)?;
        let fp = g.footprint(b);
        let centre = (fp[0] + fp[2]) * 0.5;
        // What lies in the flat, read into the panel's chart (a planar panel's chart and the flat
        // differ by a rigid move).
        let to_chart = |p: Vec2| fold.param(b.panel, p);
        let in_chart = |l: &Loop2| -> Vec<Vec2> { l.polyline(1e-3).into_vec().into_iter().filter_map(to_chart).collect() };
        let mut loops: Vec<(&str, Vec<Vec2>)> = Vec::new();
        for (piece, region) in pieces {
            match piece {
                Piece::Zone(_) | Piece::Tab(..) => loops.push(("a bend", in_chart(&region.outer))),
                Piece::Panel(i) if *i == b.panel => {
                    loops.push(("the sheet's edge", in_chart(&region.outer)));
                    loops.extend(region.holes.iter().map(|h| ("a cut-out", in_chart(h))));
                }
                Piece::Panel(_) => {}
            }
        }
        for r in self.recesses.iter().filter(|r| r.panel == b.panel) {
            loops.push(("a recess", r.region.outer.polyline(1e-3).into_vec()));
        }
        for o in self.beads.iter().filter(|o| *o != b && o.panel == b.panel) {
            if let Ok(og) = BeadGeom::new(o, t) {
                let ofp = og.footprint(o);
                loops.push(("another bead", vec![ofp[0], ofp[1], ofp[2], ofp[3], ofp[0]]));
            }
        }
        // In the bead's frame, a segment meets the footprint grown by `grow` when its clipped
        // part (Liang–Barsky) is not empty.
        let meets = |a: Vec2, c: Vec2, grow: f64| {
            let (ax, ay) = g.local(b, a);
            let (cx, cy) = g.local(b, c);
            let (dx, dy) = (cx - ax, cy - ay);
            let (mut t0, mut t1) = (0.0f64, 1.0f64);
            let (x0, x1, y0, y1) = (-g.xf - grow, g.xf + grow, -g.sq - grow, g.len + g.sq + grow);
            for (pk, qk) in [(-dx, ax - x0), (dx, x1 - ax), (-dy, ay - y0), (dy, y1 - ay)] {
                if pk.abs() < 1e-15 {
                    if qk < 0.0 {
                        return false;
                    }
                } else {
                    let r = qk / pk;
                    if pk < 0.0 {
                        t0 = t0.max(r);
                    } else {
                        t1 = t1.min(r);
                    }
                }
            }
            t0 <= t1
        };
        let hits = |grow: f64| -> Option<&str> {
            loops.iter().find_map(|(what, pts)| {
                let n = pts.len();
                (0..n).any(|k| meets(pts[k], pts[(k + 1) % n], grow)).then_some(*what)
            })
        };
        if let Some(what) = hits(0.0) {
            return Err(DocError::Invalid(format!("a bead's footprint meets {what}")));
        }
        // Clear of every loop, it lies wholly inside or outside each: inside its panel's material.
        let flat_centre = fold.flat_point(b.panel, centre).ok_or_else(|| DocError::Invalid("a bead's panel has no place in the flat".into()))?;
        let on_panel = pieces.iter().any(|(piece, region)| *piece == Piece::Panel(b.panel) && region.contains(flat_centre));
        if !on_panel {
            return Err(DocError::Invalid("a bead must lie on the material of one panel".into()));
        }
        if self.sheet.cuts.iter().any(|c| c.panel == b.panel && c.region.contains(centre)) {
            return Err(DocError::Invalid("a bead's footprint meets a cut-out".into()));
        }
        if let Some(m) = margin {
            if let Some(what) = hits(m) {
                notes.push(format!("a bead lies within {m} mm of {what}"));
            }
            if b.depth > b.width / 3.0 + 1e-9 {
                notes.push(format!("a bead {} deep is deeper than a third of its width ({:.4})", b.depth, b.width / 3.0));
            }
            if b.depth > 3.0 * t + 1e-9 {
                notes.push(format!("a bead {} deep is deeper than 3 × the sheet's thickness ({:.4})", b.depth, 3.0 * t));
            }
        }
        Ok(())
    }
}
