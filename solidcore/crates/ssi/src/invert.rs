//! A patch ready to have points of its surface inverted onto it: the
//! parameters of a point, found by Newton on the right Bézier piece, free to
//! run past the patch's boundary where the exact surface goes on. The piece
//! comes from a hint (the previous point along a curve) or, failing that, from
//! `ImplicitCAM`'s certified closest point.

use crate::geom3::V3 as Exact3;
use crate::surface3::NurbsSurf;

use crate::patch::Patch;
use crate::piece::{cross, dot, norm, sub, Piece, V3};
use crate::small::{solve_regularized, System};
use crate::Error;

/// A knot rectangle: [u0, u1] x [v0, v1].
type Rect = (f64, f64, f64, f64);

pub(crate) struct Side {
    pub index: usize,
    pieces: Vec<Piece>,
    nurbs: NurbsSurf,
    pub domain: [f64; 4],
    /// Whether the patch closes on itself in u and in v (a seam).
    pub closed: [bool; 2],
    /// The patch's size, for the inversion's own check.
    size: f64,
}

impl Side {
    /// The patch, with its Bézier pieces (made once by the caller, who has them).
    pub fn with_pieces(patch: &Patch, index: usize, pieces: Vec<Piece>) -> Result<Self, Error> {
        if pieces.is_empty() {
            return Err(Error::BadPatch(format!("patch {index} has no pieces")));
        }
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for piece in &pieces {
            let (b_lo, b_hi) = piece.bounds().split_at(3);
            for ((l, h), (&bl, &bh)) in lo.iter_mut().zip(&mut hi).zip(b_lo.iter().zip(b_hi)) {
                *l = l.min(bl);
                *h = h.max(bh);
            }
        }
        let size = norm(sub(hi, lo)).max(f64::MIN_POSITIVE);
        let domain = patch.domain().ok_or_else(|| {
            Error::BadPatch(format!(
                "patch {index}: its knots are too few for its poles"
            ))
        })?;
        Ok(Side {
            index,
            pieces,
            nurbs: patch.nurbs()?,
            domain,
            closed: patch.closed(),
            size,
        })
    }

    /// The point at patch parameters (u, v), and the normal Su × Sv there.
    pub fn at(&self, u: f64, v: f64) -> (V3, V3) {
        let Some(piece) = self.piece_at(u, v) else {
            return ([f64::NAN; 3], [f64::NAN; 3]);
        };
        let e = piece.evaluate(
            (u - piece.u0) / (piece.u1 - piece.u0),
            (v - piece.v0) / (piece.v1 - piece.v0),
        );
        (e.point, cross(e.ds, e.dt))
    }

    /// The piece holding (u, v) clamped to the domain, else the first piece
    /// (there is always one).
    fn piece_at(&self, u: f64, v: f64) -> Option<&Piece> {
        let d = self.domain;
        let (u, v) = (u.clamp(d[0], d[1]), v.clamp(d[2], d[3]));
        self.pieces
            .iter()
            .find(|p| p.u0 <= u && u <= p.u1 && p.v0 <= v && v <= p.v1)
            .or_else(|| self.pieces.first())
    }

    /// Gauss-Newton for p from (u, v): the parameters it converges to, and
    /// whether they reproduce p (to 1e-9 of the patch). Newton runs on one
    /// piece and may leave it; inside the domain that is the piece's extension,
    /// not its neighbour, and a rational arc's extension parameterizes its
    /// circle differently, so (u, v) would name another point of the patch
    /// (4.5e-4 off near a sphere's pole, ssi-corpus `c1_sphere_cyl_two_loops`).
    /// It is run again on the piece holding where it landed until the two
    /// agree; only past the domain does it run on the last piece's extension.
    fn newton_from(&self, p: V3, u: f64, v: f64) -> Option<((f64, f64), bool)> {
        let (mut u, mut v) = (u, v);
        let mut result = None;
        for _ in 0..4 {
            let (uv, reproduces, piece) = self.newton_on(p, u, v)?;
            result = Some((uv, reproduces));
            let d = self.domain;
            let in_domain = (d[0]..=d[1]).contains(&uv.0) && (d[2]..=d[3]).contains(&uv.1);
            let slack = 1e-12;
            let in_piece = uv.0 >= piece.0 - slack
                && uv.0 <= piece.1 + slack
                && uv.1 >= piece.2 - slack
                && uv.1 <= piece.3 + slack;
            if !in_domain || in_piece {
                break;
            }
            (u, v) = uv;
        }
        result
    }

    /// Gauss-Newton for p on the piece holding (u, v), with that piece's knot rectangle.
    fn newton_on(&self, p: V3, u: f64, v: f64) -> Option<((f64, f64), bool, Rect)> {
        let piece = self.piece_at(u, v)?;
        let (du, dv) = (piece.u1 - piece.u0, piece.v1 - piece.v0);
        let (mut s, mut t) = ((u - piece.u0) / du, (v - piece.v0) / dv);
        for _ in 0..40 {
            let e = piece.evaluate(s, t);
            let damping = 1e-24 * (dot(e.ds, e.ds) + dot(e.dt, e.dt));
            let step = solve_regularized(&System::columns2(e.ds, e.dt, sub(p, e.point)), damping)?;
            (s, t) = (s + step[0], t + step[1]);
            if !(s.is_finite() && t.is_finite()) || s.abs() > 3.0 || t.abs() > 3.0 {
                return None;
            }
            if step[0].abs().max(step[1].abs()) < 1e-15 {
                break;
            }
        }
        let reproduces = norm(sub(piece.evaluate(s, t).point, p)) <= 1e-9 * self.size;
        Some((
            (piece.u0 + s * du, piece.v0 + t * dv),
            reproduces,
            (piece.u0, piece.u1, piece.v0, piece.v1),
        ))
    }

    /// The parameters of a point of the patch's surface: Newton from `hint` when
    /// that reproduces the point, else from the certified closest point.
    pub fn invert(&self, p: V3, hint: Option<(f64, f64)>) -> Option<(f64, f64)> {
        if let Some((u, v)) = hint {
            if let Some((uv, true)) = self.newton_from(p, u, v) {
                return Some(uv);
            }
        }
        let hit = self
            .nurbs
            .closest(Exact3::new(p[0], p[1], p[2]), f64::INFINITY)?;
        // Newton from the certified hit runs on past the boundary where it must.
        // Where it does not reproduce the point the parameters are unknown: not
        // the hit's, which a point outside the patch has clamped to its boundary,
        // and which would read as inside it (spurious curves on two overlapping
        // sphere patches, ssi-corpus c6_spheres_overlap).
        match self.newton_from(p, hit.u, hit.v) {
            Some((uv, true)) => Some(uv),
            _ => None,
        }
    }

    /// `invert` along a curve, warm-started from the previous point's
    /// parameters. A warm start runs on past a closed patch's seam (longitude
    /// 360.1 on a sphere whose seam the curve crosses, ssi-corpus
    /// `c9n_many_spans_plane_sphere`), where the point is outside by its
    /// parameters and inside the patch: the certified search finds the chart
    /// on the seam's other side. Only a closed patch has such a seam.
    pub fn place(&self, p: V3, hint: Option<(f64, f64)>) -> Option<(f64, f64)> {
        let uv = self.invert(p, hint)?;
        if hint.is_some() && self.closed.iter().any(|&c| c) && self.margin(uv, 0.0) < 0.0 {
            if let Some(fresh) = self.invert(p, None) {
                if self.margin(fresh, 0.0) > self.margin(uv, 0.0) {
                    return Some(fresh);
                }
            }
        }
        Some(uv)
    }

    /// Parameters a little outside the domain clamped into it, when the
    /// clamped ones name the same point p (to the inversion's own 1e-9 of the
    /// patch): on a collapsed edge any u names the pole.
    pub fn clamped(&self, p: V3, (u, v): (f64, f64)) -> Option<(f64, f64)> {
        let d = self.domain;
        let uv = (u.clamp(d[0], d[1]), v.clamp(d[2], d[3]));
        (norm(sub(self.at(uv.0, uv.1).0, p)) <= 1e-9 * self.size).then_some(uv)
    }

    /// The parameters' margin inside the domain: positive inside, zero on its
    /// boundary, negative outside, in units of the domain's own size; less
    /// `shrink`, which moves the boundary in.
    pub fn margin(&self, (u, v): (f64, f64), shrink: f64) -> f64 {
        let d = self.domain;
        let (su, sv) = (d[1] - d[0], d[3] - d[2]);
        ((u - d[0]) / su)
            .min((d[1] - u) / su)
            .min((v - d[2]) / sv)
            .min((d[3] - v) / sv)
            - shrink
    }

    /// The four edges, counterclockwise in (u, v): each a map from t in [0, 1] to parameters.
    pub fn edge(&self, e: usize, t: f64) -> (f64, f64) {
        let d = self.domain;
        let lerp = |a: f64, b: f64| a + t * (b - a);
        match e {
            0 => (lerp(d[0], d[1]), d[2]),
            1 => (d[1], lerp(d[2], d[3])),
            2 => (lerp(d[1], d[0]), d[3]),
            _ => (d[0], lerp(d[3], d[2])),
        }
    }
}
