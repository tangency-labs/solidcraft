//! The field path: where one patch has an analytic surface S, the intersection
//! is the zero set of g = `f_S`(P(s, t)) on the other patch P's own parameters,
//! and it is found there, in two unknowns, instead of by subdividing both.
//!
//! Each Bézier piece of P gets g as a Bernstein polynomial (`field::compose`),
//! and its square is cut into cells:
//!  - g of one sign on a cell: no intersection there (a proof by coefficients);
//!  - ∂g/∂t of one sign: on every line s = const g has at most one zero, so the
//!    zero set in the cell is a graph t(s) over intervals of s, ends on the
//!    cell's edges, found from the edges' roots; each of its points is the root
//!    of g(s, ·), bracketed, so a trace cannot jump to another branch (and alike
//!    with s and t swapped);
//!  - neither: the cell may hold a critical point of g (∇g = 0), where the
//!    surfaces touch if g is zero there. When the Hessian's determinant has one
//!    sign over the cell and ∇g winds once around its boundary, the cell holds
//!    exactly one, nondegenerate (`critical`): an extremum is a touch point or
//!    proves the cell clear, a saddle a crossing of two branches (`contact`).
//!    Where g is convex along one parameter's lines, or its third or fourth
//!    derivative along them has one sign, the curve of its extrema along them
//!    is a tangent curve, two branches, or none, as proved on its chords
//!    (`valley`, `contact_curve`). Otherwise the cell is halved, until smaller
//!    than the resolution, where nothing proved: refused.
//!
//! Points of the other patch Q come from inverting each point onto Q
//! (`invert::Side`), and Q's domain clips the curves.

use crate::coincide::{sample_points, Spacing};
use crate::field::{casteljau, elevate1, roots, split1, Bern, Field};
use crate::intersect::{CurvePoint, Param};
use crate::invert::Side;
use crate::patch::Patch;
use crate::piece::{box_of, boxes_apart, cross, dot, norm, sub, Piece, V3};
use crate::Error;

/// What the field path takes from the run.
pub(crate) struct Settings {
    pub resolution: f64,
    /// The gap below which surfaces apart at an extremum of g still touch there.
    pub contact: f64,
    pub spacing: Spacing,
    /// Contact curves of third and fourth order may be traced (`contact_curve`):
    /// they read g's third and fourth derivatives, which a local field bounds
    /// no error of.
    pub contacts: bool,
}

/// Which patch is parametric and which surface implicit: (A parametric, S of
/// B) or (B parametric, S of A), or None when neither has an implicit form.
/// The implicit side is the one with a collapsed edge (a pole stays out of
/// the parameters traced), then the one of lower degree, then B.
fn roles<'f>(
    a: &Patch,
    fa: Option<Field<'f>>,
    b: &Patch,
    fb: Option<Field<'f>>,
) -> Option<(bool, Field<'f>)> {
    match (fa, fb) {
        (None, None) => None,
        (None, Some(sb)) => Some((true, sb)),
        (Some(sa), None) => Some((false, sa)),
        (Some(sa), Some(sb)) => {
            let rank = |patch: &Patch, s: &Field| (!patch.has_collapsed_edge(), s.degree());
            Some(if rank(a, &sa) < rank(b, &sb) {
                (false, sa)
            } else {
                (true, sb)
            })
        }
    }
}

/// The intersection of patches a and b by the field path: Some(arcs, oriented
/// along nA × nB, isolated touch points, and the points where branches cross)
/// when it applies and resolves, None when it does not (neither patch has an
/// implicit form, or a cell it cannot resolve), the caller then subdividing
/// both patches as before. `fa` and `fb` are the patches' implicit forms.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
/// `pa` and `pb` are the patches' Bézier pieces.
pub(crate) fn trace_pair(
    a: &Patch,
    ia: usize,
    fa: Option<Field>,
    pa: &[Piece],
    b: &Patch,
    ib: usize,
    fb: Option<Field>,
    pb: &[Piece],
    settings: &Settings,
) -> Result<Option<(Vec<Vec<CurvePoint>>, Vec<CurvePoint>, Vec<V3>)>, Error> {
    let Some((p_is_a, field)) = roles(a, fa, b, fb) else {
        return Ok(None);
    };
    let surf = &field;
    let (q, iq, p_pieces, q_pieces) = if p_is_a {
        (b, ib, pa, pb)
    } else {
        (a, ia, pb, pa)
    };
    let side = Side::with_pieces(q, iq, q_pieces.to_vec())?;
    // Only pieces whose boxes meet the other patch's are composed: a 31 x 31
    // span plane against a sphere (ssi-corpus c9n_many_spans_plane_sphere) has
    // 961 pieces and a circle crossing a few dozen.
    let q_box = box_of(&q.poles);
    let pad = 1e-9
        * norm([
            q_box[3] - q_box[0],
            q_box[4] - q_box[1],
            q_box[5] - q_box[2],
        ]);
    let mut arcs = Vec::new();
    let mut points = Vec::new();
    let mut vertices = Vec::new();
    for piece in p_pieces {
        if boxes_apart(piece.bounds(), &q_box, pad) {
            continue;
        }
        let Some((g, scale)) = surf.compose(piece) else {
            return Ok(None);
        };
        let (g, poles) = divide_poles(piece, g, scale);
        let Some(found) = cells(piece, g, scale, surf, settings, None) else {
            return Ok(None);
        };
        let ctx = Emit {
            piece,
            side: &side,
            p_is_a,
            spacing: &settings.spacing,
            clip: None,
            local: None,
        };
        let (more, touches, met) = emit_found(&ctx, &found, surf, poles);
        arcs.extend(more);
        points.extend(touches);
        vertices.extend(met);
    }
    Ok(Some((arcs, points, vertices)))
}

/// A piece's arcs, touch points and vertices from its cells.
#[allow(clippy::type_complexity)]
fn emit_found(
    ctx: &Emit,
    found: &Found,
    surf: &Field,
    poles: Vec<(V3, (f64, f64))>,
) -> (Vec<Vec<CurvePoint>>, Vec<CurvePoint>, Vec<V3>) {
    let piece = ctx.piece;
    let (mut arcs, mut points, mut vertices) = (Vec::new(), Vec::new(), Vec::new());
    for sweep in &found.sweeps {
        arcs.extend(ctx.emit(sweep, true));
    }
    for sweep in &found.tangents {
        arcs.extend(ctx.emit(sweep, false));
    }
    points.extend(
        found
            .touches
            .iter()
            .filter_map(|&(s, t)| ctx.contact_point(s, t, surf)),
    );
    vertices.extend(
        found
            .meets
            .iter()
            .filter_map(|&(s, t)| ctx.contact_point(s, t, surf))
            .map(|q| q.p),
    );
    // A pole on the other surface where arcs end: they meet there (two
    // branches crossing at the pole leave its edge at different s). One no
    // arc ends at touches the surface there alone: an isolated point, when
    // it is on the other patch (two shells touching at their poles, the
    // field zero to second order along the edge and both factors divided
    // out, ssi-corpus y23_fig1g_three_points).
    let near = 1e-9
        * norm(sub(
            [piece.bounds()[3], piece.bounds()[4], piece.bounds()[5]],
            [piece.bounds()[0], piece.bounds()[1], piece.bounds()[2]],
        ));
    for (pole, (s, t)) in poles {
        if arcs.iter().any(|arc: &Vec<CurvePoint>| {
            [arc.first(), arc.last()]
                .into_iter()
                .flatten()
                .any(|end| norm(sub(end.p, pole)) <= near)
        }) {
            vertices.push(pole);
        } else if let Some(q) = ctx.contact_point(s, t, surf) {
            points.push(q);
        }
    }
    for (x, ends) in &found.crossings {
        let Some(center) = ctx.contact_point(x.0, x.1, surf) else {
            continue;
        };
        let segments = ctx.crossing(center, ends);
        if !segments.is_empty() {
            vertices.push(center.p);
        }
        arcs.extend(segments);
    }
    (arcs, points, vertices)
}

/// The order of a local field's polynomial.
const LOCAL_ORDER: usize = 6;

/// The field path on a pair of pieces where neither patch has an implicit
/// form: A's piece traced against B's on a local field (`taylor`), the
/// separation of A from B along A's normal at the piece's middle, as one
/// polynomial over the piece (grown by `GROW`, the square `critical` reads)
/// with proved bounds on its value's, gradient's and Hessian's errors. The
/// cells read the value's band as their scale, and the derivatives' bands at
/// each cell's width for the tests of derivatives' signs (`Cell::dscale`). One
/// polynomial over the piece, so its cells never disagree; the polynomial
/// proves, and every point emitted is placed on the surfaces themselves
/// (inverted onto B, clipped to B's piece: a neighboring pair owns the rest).
/// None when the field cannot be proved over the piece (too large: the pair
/// is halved instead) or a cell is unresolved.
#[allow(clippy::type_complexity)]
pub(crate) fn trace_local(
    a: &Piece,
    b: &Piece,
    side: &Side,
    settings: &Settings,
) -> Option<(Vec<Vec<CurvePoint>>, Vec<CurvePoint>, Vec<V3>)> {
    let dir = crate::taylor::normal(a, 0.5, 0.5);
    let start = [(0.5, 0.5), (0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
        .iter()
        .find_map(|&(u, v)| crate::taylor::separation(a, b, dir, (0.5, 0.5), (u, v, 0.0)))?;
    let grown = (-GROW, 1.0 + GROW, -GROW, 1.0 + GROW);
    let local = crate::taylor::local_field(a, b, dir, grown, start, LOCAL_ORDER)?;
    let (lo, hi) = (GROW / (1.0 + 2.0 * GROW), (1.0 + GROW) / (1.0 + 2.0 * GROW));
    let g = local.g.restrict(lo, hi, lo, hi);
    // The value's band as the cells' scale; the derivatives' at each cell's width.
    let scale = local.band / CLEAR;
    let field = Field::Local(b, dir);
    let local_settings = Settings {
        resolution: settings.resolution,
        contact: settings.contact,
        spacing: Spacing {
            max_step: settings.spacing.max_step,
            max_sag: settings.spacing.max_sag,
        },
        contacts: false,
    };
    let (g, poles) = divide_poles(a, g, scale);
    let found = cells(
        a,
        g,
        scale,
        &field,
        &local_settings,
        Some((local.gradient, local.hessian)),
    )?;
    let ctx = Emit {
        piece: a,
        side,
        p_is_a: true,
        spacing: &settings.spacing,
        clip: Some([b.u0, b.u1, b.v0, b.v1]),
        local: Some((b, dir)),
    };
    Some(emit_found(&ctx, &found, &field, poles))
}

/// g with the factor of each collapsed edge of the piece (a pole) divided out
/// where the pole lies on the surface, and those poles. There g is zero along
/// the whole edge, one point in space: every cell beside it is a tangency to
/// the cell test, and two branches crossing at the pole cross there in the
/// parameters too (ssi-corpus `y23_t3_ex3_torus_ellipsoid`, an ellipsoid's pole
/// on a torus). Divided out, the branches leave the edge apart. An edge that
/// is a curve on the surface is not divided: it is part of the zero set.
/// Each pole with the piece-local (s, t) of its edge's middle.
#[allow(clippy::type_complexity)]
fn divide_poles(piece: &Piece, mut g: Bern, scale: f64) -> (Bern, Vec<(V3, (f64, f64))>) {
    let mut poles = Vec::new();
    for e in 0..4 {
        let edge: Vec<V3> = piece
            .edge(e)
            .iter()
            .map(|h| [h[0] / h[3], h[1] / h[3], h[2] / h[3]])
            .collect();
        let collapsed = edge.windows(2).all(|w| match w {
            [a, b] => a
                .iter()
                .zip(b)
                .all(|(x, y)| (x - y).abs() <= 1e-12 * (1.0 + x.abs())),
            _ => true,
        });
        if !collapsed {
            continue;
        }
        let mut divided = false;
        while (if e < 2 { g.du } else { g.dv }) > 0
            && g.edge(e).iter().all(|x| x.abs() <= 1e-11 * scale)
        {
            g = g.divide_edge(e);
            divided = true;
        }
        if divided {
            let at = match e {
                0 => (0.0, 0.5),
                1 => (1.0, 0.5),
                2 => (0.5, 0.0),
                _ => (0.5, 1.0),
            };
            if let Some(&pole) = edge.first() {
                poles.push((pole, at));
            }
        }
    }
    (g, poles)
}

/// A cell of a piece's square [s0, s1] × [t0, t1], with g over it as a Bernstein polynomial on its own [0, 1]^2.
struct Cell {
    s0: f64,
    s1: f64,
    t0: f64,
    t1: f64,
    g: Bern,
    depth: u32,
    /// Proved free of critical points of g: halving resolves it, however small
    /// it must get (beside a crossing), so the resolution does not stop it.
    free: bool,
    /// The piece-local critical point the cell holds, already proved its only
    /// one and clear of the zero set (`Contact::Open`): its halves are not
    /// tested again, the ones without it being free.
    open: Option<(f64, f64)>,
    /// The scale of g's coefficients that its rounding bands are read in: the
    /// field's, times what dividing out zero edges multiplied the rounding by
    /// (`zero_edges`).
    scale: f64,
    /// The same for g's derivatives' coefficients, which the tests of their
    /// signs read (monotone, the Hessian's determinant, the gradient's turns,
    /// a valley's lines): `scale` for an implicit form's g, whose rounding is
    /// its coefficients'; for a local field, the proved bands of its gradient
    /// and Hessian at the cell's width (`cells`).
    dscale: f64,
    /// g has its zero edges divided out already: dividing it again would read
    /// its rows in the band the first division widened.
    divided: bool,
}

impl Cell {
    /// The piece-local (s, t) of cell-local (cs, ct).
    fn at(&self, cs: f64, ct: f64) -> (f64, f64) {
        (
            self.s0 + cs * (self.s1 - self.s0),
            self.t0 + ct * (self.t1 - self.t0),
        )
    }

    fn holds(&self, (s, t): (f64, f64)) -> bool {
        (self.s0..=self.s1).contains(&s) && (self.t0..=self.t1).contains(&t)
    }
}

/// A crossing's piece-local point and where its branches leave the box about it.
type Crossing = ((f64, f64), Vec<(f64, f64)>);

/// What a piece's cells come to.
#[derive(Default)]
struct Found {
    sweeps: Vec<Sweep>,
    /// Tangent curves: valleys of g on its zero level, each a sweep of g's
    /// derivative across them (`valley`).
    tangents: Vec<Sweep>,
    /// Piece-local (s, t) of touch points: extrema of g on its zero level.
    touches: Vec<(f64, f64)>,
    /// Crossings: a saddle of g on its zero level, and the piece-local points
    /// where its branches leave the small box about it.
    crossings: Vec<Crossing>,
    /// Piece-local (s, t) of points where two branches meet tangent to each other.
    meets: Vec<(f64, f64)>,
}

/// A cell's critical point of g, cell-local, with its index (+1 an extremum,
/// -1 a saddle) and the Hessian there, (`g_ss`, `g_st`, `g_tt`).
pub(crate) struct Critical {
    pub s: f64,
    pub t: f64,
    pub index: i32,
    pub hessian: (f64, f64, f64),
}

pub(crate) enum Inside {
    /// No critical point in the cell.
    Free,
    /// Exactly one, nondegenerate.
    One(Critical),
    /// Not proved either way.
    Unknown,
}

/// The critical points of g in a cell, when its coefficients prove how many.
/// The Hessian's determinant of one sign over the cell (Bernstein bounds) makes
/// every critical point nondegenerate, of index that sign; the turns ∇g makes
/// around the cell's boundary count them, as the sum of their indices
/// (Poincaré–Hopf, the degree of ∇g). Turns 0: none; turns equal to the sign:
/// one, found by Newton on ∇g. Otherwise unknown.
///
/// Where ∇g vanishes on the boundary (a critical point on a knot line or a
/// cell's edge, as in most symmetric models) the turns are undefined, and the
/// count is taken over the square grown by `GROW` on each side, g's polynomial
/// running on past the cell: its one critical point is the cell's when on the
/// closed cell, and the cell has none when it is outside.
///
/// Signs are read against rounding bands absolute in g's `scale` (the
/// magnitude of its terms), not relative to each polynomial: in a cell 1e-7
/// across on a tangent curve the determinant and ∇g are rounding throughout,
/// and relative bands read that as signs, a cell free of critical points
/// (ssi-corpus `c2n_quartic_circle`).
#[cfg(test)]
pub(crate) fn critical(g: &Bern, scale: f64) -> Inside {
    critical_claiming(g, scale, false)
}

/// `critical`, a critical point found past the closed cell claimed within its
/// position's uncertainty when `uncertain`: a local field's polynomial differs
/// from the field by its bands, and from its neighbor's, so one on the edge
/// between two cells may lie just outside each by its own polynomial, and
/// neither claimed it (a crossing on a knot line, ssi-corpus
/// `y23_fig1a_two_loops_cross`, at order 4). The uncertainty is the gradient's
/// band over the Hessian's smaller eigenvalue there; both cells may then claim
/// it, and duplicates are merged.
fn critical_claiming(g: &Bern, scale: f64, uncertain: bool) -> Inside {
    if let Ok(found) = critical_on(g, scale) {
        found
    } else {
        let (lo, hi) = (-GROW, 1.0 + GROW);
        match critical_on(&g.restrict(lo, hi, lo, hi), scale) {
            Ok(Inside::One(c)) => {
                let (s, t) = (lo + c.s * (hi - lo), lo + c.t * (hi - lo));
                let slack = if uncertain {
                    let (h11, h12, h22) = c.hessian;
                    let (mean, spread) = (
                        0.5 * (h11 + h22),
                        (0.25 * (h11 - h22) * (h11 - h22) + h12 * h12).sqrt(),
                    );
                    let weaker = (mean + spread).abs().min((mean - spread).abs());
                    (4.0 * 1e-13 * scale / weaker).clamp(1e-9, 0.5 * GROW)
                } else {
                    1e-9
                };
                if !(-slack..=1.0 + slack).contains(&s) || !(-slack..=1.0 + slack).contains(&t) {
                    return Inside::Free;
                }
                let (s, t) = (s.clamp(0.0, 1.0), t.clamp(0.0, 1.0));
                Inside::One(Critical {
                    s,
                    t,
                    index: c.index,
                    hessian: Hessian::of(g).at(s, t),
                })
            }
            Ok(found) => found,
            Err(()) => Inside::Unknown,
        }
    }
}

/// How far `critical` grows a cell whose boundary ∇g vanishes on: not a dyadic
/// fraction, so that it does not land on the next symmetric point.
const GROW: f64 = 1.0 / 24.0;

/// g's second derivatives as polynomials; one is zero where g is of degree 1 in its direction.
struct Hessian {
    ss: Option<Bern>,
    st: Bern,
    tt: Option<Bern>,
}

impl Hessian {
    fn of(g: &Bern) -> Hessian {
        let (gs, gt) = (g.derivative(0), g.derivative(1));
        Hessian {
            ss: (g.du >= 2).then(|| gs.derivative(0)),
            st: gs.derivative(1),
            tt: (g.dv >= 2).then(|| gt.derivative(1)),
        }
    }

    fn at(&self, s: f64, t: f64) -> (f64, f64, f64) {
        let value = |b: &Option<Bern>| b.as_ref().map_or(0.0, |b| b.at(s, t));
        (value(&self.ss), self.st.at(s, t), value(&self.tt))
    }

    /// The determinant `g_ss` `g_tt` - `g_st`^2 as a polynomial.
    fn det(&self) -> Bern {
        let square = self.st.mul(&self.st);
        match (&self.ss, &self.tt) {
            (Some(ss), Some(tt)) => Bern::sum(&[(1.0, &ss.mul(tt)), (-1.0, &square)]),
            _ => Bern::sum(&[(-1.0, &square)]),
        }
    }
}

/// `critical` on the cell's own boundary: Err where ∇g vanishes on it, or too nearly to tell.
fn critical_on(g: &Bern, scale: f64) -> Result<Inside, ()> {
    if g.du < 1 || g.dv < 1 {
        return Ok(Inside::Unknown);
    }
    let (gs, gt) = (g.derivative(0), g.derivative(1));
    let hessian = Hessian::of(g);
    // Second derivatives' rounding, and the determinant's from it.
    let band = 1e-12 * scale;
    let largest = hessian
        .ss
        .iter()
        .chain(hessian.tt.iter())
        .fold(hessian.st.max_abs(), |m, b| m.max(b.max_abs()));
    let floor = band * (2.0 * largest + band);
    // The determinant's signs at the corners and middle first: where they
    // differ it has no one sign, known before its polynomial, a product of
    // two of degree 30 or so on a free-form pair's field.
    let at = |s: f64, t: f64| {
        let (a, b, c) = hessian.at(s, t);
        a * c - b * b
    };
    let samples = [
        at(0.0, 0.0),
        at(1.0, 0.0),
        at(0.0, 1.0),
        at(1.0, 1.0),
        at(0.5, 0.5),
    ];
    if samples.iter().any(|&d| d > floor) && samples.iter().any(|&d| d < -floor) {
        return Ok(Inside::Unknown);
    }
    let det = hessian.det();
    if !det.one_sign((1e-12 * det.max_abs()).max(floor)) {
        return Ok(Inside::Unknown);
    }
    let sign = if det.c.first().is_some_and(|&x| x > 0.0) {
        1
    } else {
        -1
    };
    let turns = winding(&gs, &gt, 1e-13 * scale).ok_or(())?;
    if turns == 0 {
        return Ok(Inside::Free);
    }
    if turns != sign {
        return Ok(Inside::Unknown);
    }
    let newton = |mut s: f64, mut t: f64| -> Option<Critical> {
        for _ in 0..60 {
            let (a, b) = (gs.at(s, t), gt.at(s, t));
            let (h11, h12, h22) = hessian.at(s, t);
            let d = h11 * h22 - h12 * h12;
            let (ds, dt) = ((h22 * a - h12 * b) / d, (h11 * b - h12 * a) / d);
            (s, t) = (s - ds, t - dt);
            if !s.is_finite()
                || !t.is_finite()
                || !(-0.5..=1.5).contains(&s)
                || !(-0.5..=1.5).contains(&t)
            {
                return None;
            }
            if ds.abs().max(dt.abs()) <= 4.0 * f64::EPSILON {
                break;
            }
        }
        let slack = 1e-9;
        if !(-slack..=1.0 + slack).contains(&s) || !(-slack..=1.0 + slack).contains(&t) {
            return None;
        }
        let (s, t) = (s.clamp(0.0, 1.0), t.clamp(0.0, 1.0));
        Some(Critical {
            s,
            t,
            index: sign,
            hessian: hessian.at(s, t),
        })
    };
    Ok([
        (0.5, 0.5),
        (0.25, 0.25),
        (0.75, 0.25),
        (0.25, 0.75),
        (0.75, 0.75),
    ]
    .into_iter()
    .find_map(|(s, t)| newton(s, t))
    .map_or(Inside::Unknown, Inside::One))
}

/// The turns (`g_s`, `g_t`) makes around a cell's boundary, counterclockwise; None
/// where it vanishes on the boundary, or too nearly to tell: coefficients
/// within 1e-10 of the gradient's size, or within the rounding `floor`, count
/// as zero (a critical point on a cell's corner leaves rounding there, of
/// either sign, and taken for signs it gave 0 turns about a touch point,
/// ssi-corpus `c3n_four_points`).
fn winding(gs: &Bern, gt: &Bern, floor: f64) -> Option<i32> {
    let clear = (1e-10 * gs.max_abs().max(gt.max_abs())).max(floor);
    // Counterclockwise: t = 0 along s, s = 1 along t, t = 1 back along s, s = 0 back along t.
    let mut total = 0.0;
    for (e, way) in [(2, 1.0), (1, 1.0), (3, -1.0), (0, -1.0)] {
        total += way * turn(&gs.edge(e), &gt.edge(e), clear, 0)?;
    }
    let turns = total / std::f64::consts::TAU;
    let whole = turns.round();
    ((turns - whole).abs() < 1e-3).then_some(whole as i32)
}

/// The angle (u, v) turns through from x = 0 to 1, for one-variable Bernstein
/// polynomials u and v: halved until u or v has coefficients of one sign on
/// each part, where (u, v) keeps to a half-plane and the angle is the one
/// between its ends.
fn turn(u: &[f64], v: &[f64], clear: f64, depth: u32) -> Option<f64> {
    let strict = |c: &[f64]| c.iter().all(|&x| x > clear) || c.iter().all(|&x| x < -clear);
    if strict(u) || strict(v) {
        let (Some(&u0), Some(&v0), Some(&u1), Some(&v1)) =
            (u.first(), v.first(), u.last(), v.last())
        else {
            return None;
        };
        let (a, b) = ((u0, v0), (u1, v1));
        return Some((a.0 * b.1 - a.1 * b.0).atan2(a.0 * b.0 + a.1 * b.1));
    }
    if depth >= 40 {
        return None;
    }
    let ((u0, u1), (v0, v1)) = (split1(u), split1(v));
    Some(turn(&u0, &v0, clear, depth + 1)? + turn(&u1, &v1, clear, depth + 1)?)
}

/// What the surfaces do at a cell's one critical point x* of g.
enum Contact {
    /// They touch at x*, and nowhere else in the cell.
    Touch,
    /// They do not meet in the cell.
    Apart,
    /// Two branches of the intersection cross at x*.
    Crossing,
    /// The zero set keeps clear of x* by more than the resolution: halving resolves it.
    Open,
}

/// The contact at a cell's critical point. g(x*) and the Hessian H there give
/// the zero set's reach from x*, about sqrt(2 |g(x*)| / |λ|) along H's weaker
/// eigenvector, taken to 3D by the piece's derivatives. At an extremum whose
/// value lies away from zero (a minimum above it, a maximum below), H is
/// definite over the whole cell and that value bounds g there: the surfaces
/// are apart in the cell, or touch at x* when their gap there is within the
/// contact tolerance. An extremum within reach of the resolution is a touch
/// point (the loop about it is smaller than the resolution), a saddle a crossing.
fn contact(
    piece: &Piece,
    cell: &Cell,
    c: &Critical,
    surf: &Field,
    clear: f64,
    settings: &Settings,
) -> Contact {
    let (s, t) = cell.at(c.s, c.t);
    let e = piece.evaluate(s, t);
    let gap = norm(sub(e.point, surf.foot(e.point)));
    let value = cell.g.at(c.s, c.t);
    let (h11, h12, h22) = c.hessian;
    let (mean, spread) = (
        0.5 * (h11 + h22),
        (0.25 * (h11 - h22) * (h11 - h22) + h12 * h12).sqrt(),
    );
    let weaker = (mean + spread).abs().min((mean - spread).abs());
    let per_unit = norm(e.ds) * (cell.s1 - cell.s0) + norm(e.dt) * (cell.t1 - cell.t0);
    let reach = (2.0 * value.abs() / weaker).sqrt() * per_unit;
    // g zero to its rounding at x*: a contact to the input's own precision,
    // whatever the field's curvature makes of the reach (9.8e-13 against a
    // band of 2.1e-12, where a degree-6 form's flatness put the reach past the
    // resolution, ssi-corpus y23_fig1g_three_points).
    if value.abs() <= clear {
        return if c.index > 0 {
            Contact::Touch
        } else {
            Contact::Crossing
        };
    }
    if c.index > 0 {
        let apart = if h11 > 0.0 { value > 0.0 } else { value < 0.0 };
        if apart {
            return if gap <= settings.contact {
                Contact::Touch
            } else {
                Contact::Apart
            };
        }
        return if reach <= settings.resolution {
            Contact::Touch
        } else {
            Contact::Open
        };
    }
    if reach <= settings.resolution {
        Contact::Crossing
    } else {
        Contact::Open
    }
}

/// The rounding band of g's coefficients and its derivatives', in g's scale:
/// values within it are zero. Ten times the coefficients' rounding (a few
/// ulps of the scale); at 1e-12 a cylinder sunk 5e-13 into a plane read as
/// tangent along the knot between its two lines, and one line was lost
/// (ssi-corpus c7_sunk_cyl_1e-6).
const CLEAR: f64 = 1e-14;

/// A crossing at a cell's saddle x*: a box about it (the cell itself when
/// smaller), whose branches are joined to x* by segments from the box's edges;
/// the rest of the cell, free of critical points, cut into up to eight cells
/// about the box. The box is a few resolutions across, and wide enough that g
/// on its edges, of order |H| h^2 at half-width h, stands 1e4 times clear of
/// the rounding band its roots are found against: at a few resolutions it
/// was inside the band, and the box had no roots (ssi-corpus `c4_cone_apex_plane`).
fn crossing(
    piece: &Piece,
    cell: &Cell,
    c: &Critical,
    settings: &Settings,
    clear: f64,
    found: &mut Found,
    stack: &mut Vec<Cell>,
) {
    let e = piece.evaluate(cell.at(c.s, c.t).0, cell.at(c.s, c.t).1);
    let (ws, wt) = (cell.s1 - cell.s0, cell.t1 - cell.t0);
    let (h11, h12, h22) = c.hessian;
    let curvature = (h11 * h11 + 2.0 * h12 * h12 + h22 * h22).sqrt();
    let wide = 100.0 * (clear / curvature).sqrt();
    let (hs, ht) = (
        (2.0 * settings.resolution / (norm(e.ds) * ws)).max(wide),
        (2.0 * settings.resolution / (norm(e.dt) * wt)).max(wide),
    );
    let (bs0, bs1) = ((c.s - hs).max(0.0), (c.s + hs).min(1.0));
    let (bt0, bt1) = ((c.t - ht).max(0.0), (c.t + ht).min(1.0));
    let box_g = cell.g.restrict(bs0, bs1, bt0, bt1);
    let mut ends: Vec<(f64, f64)> = Vec::new();
    for edge in 0..4 {
        let Some(xs) = roots(&box_g.edge(edge), clear) else {
            continue;
        };
        for x in xs {
            let (bs, bt) = match edge {
                0 => (0.0, x),
                1 => (1.0, x),
                2 => (x, 0.0),
                _ => (x, 1.0),
            };
            let (ls, lt) = (bs0 + bs * (bs1 - bs0), bt0 + bt * (bt1 - bt0));
            // A box edge through x* itself (x* on the cell's edge) has a double
            // root there: not a branch's way out.
            if ((ls - c.s) / hs).powi(2) + ((lt - c.t) / ht).powi(2) < 0.25 {
                continue;
            }
            let q = cell.at(ls, lt);
            if !ends
                .iter()
                .any(|p| (p.0 - q.0).abs() + (p.1 - q.1).abs() <= 1e-12)
            {
                ends.push(q);
            }
        }
    }
    found.crossings.push((cell.at(c.s, c.t), ends));
    let (s_cuts, t_cuts) = ([0.0, bs0, bs1, 1.0], [0.0, bt0, bt1, 1.0]);
    for (i, s_pair) in s_cuts.windows(2).enumerate() {
        for (j, t_pair) in t_cuts.windows(2).enumerate() {
            let (&[a0, a1], &[b0, b1]) = (s_pair, t_pair) else {
                continue;
            };
            if (i == 1 && j == 1) || !(a1 > a0 && b1 > b0) {
                continue;
            }
            let ((s0, t0), (s1, t1)) = (cell.at(a0, b0), cell.at(a1, b1));
            stack.push(Cell {
                s0,
                s1,
                t0,
                t1,
                g: cell.g.restrict(a0, a1, b0, b1),
                depth: cell.depth + 1,
                free: true,
                open: None,
                scale: cell.scale,
                dscale: cell.dscale,
                divided: false,
            });
        }
    }
}

/// A tangent curve across a cell: where g is convex (or concave) along every
/// line of one parameter, `g_ss` of one sign on the cell, each line s -> g(s, t)
/// has one critical point m(t), the root of `g_s`, and G(t) = g(m(t), t) is its
/// minimum there (maximum, concave). The valley m(t) is swept like any arc,
/// as the zero set of `g_s`; when it crosses the cell from edge to edge and |G|
/// is proved small along all of it, every zero of g in the cell lies within
/// the resolution of the valley, sqrt(2 |G| / |`g_ss`|) along each line, and the
/// valley is the tangent curve. Where the valley leaves the cell by a side
/// (as a curved one does at every cell it cuts the corner of), the lines past
/// it have `g_s` of one sign: g is monotone along each, its one zero bracketed
/// by the line's ends, and those lines are swept as a monotone cell's are.
///
/// Proved on whole intervals, not at points: on a chord of the valley,
/// G <= g there (any point bounds the minimum), and by convexity
/// G >= g - `g_s`^2 / (2 min |`g_ss`|) there, both polynomials along the chord
/// bounded by their coefficients (`Bern::along`); a chord whose bounds do
/// not prove it is halved. Alike with s and t swapped.
///
/// Some(tangent curves, arcs, points where a split valley's two branches
/// meet) when proved; none of any when the valley is
/// the cell's high edge, left to the cell beyond (the ownership of
/// `sweep_cell`; a tangent ruling on a knot line, ssi-corpus
/// `c2_cyl_cyl_external`, otherwise halved the cell beside it to 2^20 cells
/// along the edge). None when neither direction proves one.
#[allow(clippy::type_complexity)]
fn valley(
    piece: &Piece,
    cell: &Cell,
    scale: f64,
    settings: &Settings,
) -> Option<(Vec<Sweep>, Vec<Sweep>, Vec<(f64, f64)>)> {
    for lines in [0, 1] {
        let d = cell.g.derivative(lines);
        let dd = d.derivative(lines);
        if !dd.one_sign(1e-14 * cell.dscale) {
            if !settings.contacts {
                continue;
            }
            if let Some(found) = contact_curve(piece, cell, lines, scale, settings) {
                return Some(found);
            }
            continue;
        }
        let dd_min = dd.c.iter().fold(f64::INFINITY, |m, x| m.min(x.abs()));
        // The band |G| <= limit keeps g's zeros within the resolution of the valley.
        let (cs, ct) = cell.at(0.5, 0.5);
        let e = piece.evaluate(cs, ct);
        let along = if lines == 0 {
            norm(e.ds) * (cell.s1 - cell.s0)
        } else {
            norm(e.dt) * (cell.t1 - cell.t0)
        };
        let limit = 0.5 * dd_min * (settings.resolution / along).powi(2);
        // g_s zero along the high edge, s = 1 (t = 1 for lines along t): the valley
        // is that edge, G there is g's edge polynomial, bounded by its coefficients.
        let high = if lines == 0 { 1 } else { 3 };
        let beyond = if lines == 0 {
            cell.s1 < 1.0 || piece.above[0]
        } else {
            cell.t1 < 1.0 || piece.above[1]
        };
        if beyond && roots(&d.edge(high), CLEAR * cell.dscale).is_none() {
            if cell.g.edge(high).iter().all(|x| x.abs() <= limit) {
                return Some((Vec::new(), Vec::new(), Vec::new()));
            }
            continue;
        }
        let mut valleys = Vec::new();
        let d_cell = Cell {
            s0: cell.s0,
            s1: cell.s1,
            t0: cell.t0,
            t1: cell.t1,
            g: d.clone(),
            depth: cell.depth,
            free: false,
            open: None,
            scale: cell.scale,
            dscale: cell.dscale,
            divided: false,
        };
        sweep_cell(piece, &d_cell, 1 - lines, CLEAR * cell.dscale, &mut valleys);
        if valleys.is_empty() {
            continue;
        }
        let sign = dd.c.first().map_or(f64::NAN, |x| x.signum());
        let local = |(s, t): (f64, f64)| {
            (
                (s - cell.s0) / (cell.s1 - cell.s0),
                (t - cell.t0) / (cell.t1 - cell.t0),
            )
        };
        // The bound on |G| over the chord from p to q (cell-local). Along each
        // line g lies between its value and slope at the chord with the
        // curvature at min |g_ss| and at max |g_ss| between the chord and the
        // line's minimum, so (for sign g convex)
        // g - g_s^2 / (2 min) <= sign G <= g - g_s^2 / (2 max) on the chord,
        // each one polynomial, g raised to the square's degree. The minimum is
        // within max |g_s| / min |g_ss| of the chord: g_ss is bounded on that
        // thin box about it, not on the cell, where it varied 5 times over.
        // (Bounded by g alone above and by the cell's g_ss, a curved valley
        // needed 15,000 chords, ssi-corpus c2n_quartic_circle.)
        let bound = |p: (f64, f64), q: (f64, f64)| {
            let a: Vec<f64> = cell.g.along(p, q).into_iter().map(|x| sign * x).collect();
            let b = d.along(p, q);
            let reach = b.iter().fold(0.0_f64, |m, x| m.max(x.abs())) / dd_min;
            let (line, other) = if lines == 0 {
                ((p.0, q.0), (p.1, q.1))
            } else {
                ((p.1, q.1), (p.0, q.0))
            };
            let (l0, l1) = (
                (line.0.min(line.1) - reach).max(0.0),
                (line.0.max(line.1) + reach).min(1.0),
            );
            let (o0, o1) = (other.0.min(other.1), other.0.max(other.1));
            let near = if lines == 0 {
                dd.restrict(l0, l1, o0, o1)
            } else {
                dd.restrict(o0, o1, l0, l1)
            };
            let (k_min, k_max) = near.c.iter().fold((f64::INFINITY, 0.0_f64), |(lo, hi), x| {
                (lo.min(x.abs()), hi.max(x.abs()))
            });
            let b = Bern {
                du: b.len() - 1,
                dv: 0,
                c: b,
            };
            let square = b.mul(&b);
            let degree = square.du.max(a.len() - 1);
            let (a, square) = (elevate1(&a, degree), elevate1(&square.c, degree));
            let below = |k: f64| a.iter().zip(&square).map(move |(x, y)| x - y / (2.0 * k));
            (
                below(k_min).fold(f64::INFINITY, f64::min),
                below(k_max).fold(f64::NEG_INFINITY, f64::max),
            )
        };
        // What the valley is, proved on chords from one over the whole valley,
        // halved where their bounds do not prove it, to a budget: past it the
        // cell is halved instead, its valley straighter (chords from the emitted
        // samples were 200 bounds a cell, most of 0.2 s on ssi-corpus
        // c2n_quartic_circle). The verdicts are tried as G at the valley's ends
        // and middle allows, before any chord's bound (a transversal cell's
        // valley lies far below zero; bounding its chords first made ssi-corpus
        // c1_sphere_cyl_two_loops 2.3 times slower):
        //  - Split: G at most zero (to rounding) all along and below the band
        //    somewhere: two zeros on each line, meeting where G reaches zero
        //    (two loops tangent at a point, G = -c x^4 about it, were merged
        //    into a tangent stretch where G was within the band, ssi-corpus
        //    y23_fig1e_loops_touching);
        //  - Tangent: G within the band all along;
        //  - Clear: G above it all along.
        let rounding = CLEAR * scale;
        let verdict = |sweep: &Sweep| -> Option<Valley> {
            let point = |x: f64| local(sweep.at(x));
            let (p0, p1) = (point(sweep.x0), point(sweep.x1));
            let middle = point(0.5 * (sweep.x0 + sweep.x1));
            let value = |(s, t): (f64, f64)| sign * cell.g.at(s, t);
            let values = [value(p0), value(middle), value(p1)];
            let prove = |holds: &dyn Fn((f64, f64)) -> bool| {
                let mut chords = vec![(sweep.x0, p0, sweep.x1, p1)];
                let mut budget = CHORDS;
                while let Some((x0, q0, x1, q1)) = chords.pop() {
                    if holds(bound(q0, q1)) {
                        continue;
                    }
                    budget -= 1;
                    if budget == 0 {
                        return false;
                    }
                    let xm = 0.5 * (x0 + x1);
                    let qm = point(xm);
                    chords.push((x0, q0, xm, qm));
                    chords.push((xm, qm, x1, q1));
                }
                true
            };
            if values.iter().all(|&v| v <= rounding)
                && values.iter().any(|&v| v < -limit)
                && prove(&|(_, high)| high <= rounding)
            {
                return Some(Valley::Split);
            }
            if values.iter().all(|&v| v.abs() <= limit)
                && prove(&|(low, high)| low >= -limit && high <= limit)
            {
                return Some(Valley::Tangent);
            }
            if values.iter().all(|&v| v > limit) && prove(&|(low, _)| low > limit) {
                return Some(Valley::Clear);
            }
            None
        };
        // Where a split valley's G reaches zero its two branches meet: the roots
        // of G' = sign g_x (g_s being zero there) between valley points where it
        // turns from rising to falling, each solved by bisection, and the
        // valley's ends where G rises to them (a crossing on a knot line is at
        // the end of the valleys on both sides of it, ssi-corpus
        // c4n_viviani_nurbs); kept where G is zero to rounding. Piece-local.
        let gx = cell.g.derivative(1 - lines);
        let meets = |sweep: &Sweep| -> Vec<(f64, (f64, f64))> {
            let point = |x: f64| local(sweep.at(x));
            let slope = |x: f64| {
                let (s, t) = point(x);
                sign * gx.at(s, t)
            };
            let n = 32;
            let xs: Vec<f64> = (0..=n)
                .map(|i| sweep.x0 + (sweep.x1 - sweep.x0) * f64::from(i) / f64::from(n))
                .collect();
            let mut out = Vec::new();
            for (x, rises) in [
                (sweep.x0, slope(sweep.x0) <= 0.0),
                (sweep.x1, slope(sweep.x1) >= 0.0),
            ] {
                let (s, t) = point(x);
                if rises && sign * cell.g.at(s, t) >= -rounding {
                    out.push((x, cell.at(s, t)));
                }
            }
            for w in xs.windows(2) {
                let &[mut a, mut b] = w else {
                    continue;
                };
                if !(slope(a) > 0.0 && slope(b) <= 0.0) {
                    continue;
                }
                for _ in 0..60 {
                    let m = 0.5 * (a + b);
                    if slope(m) > 0.0 {
                        a = m;
                    } else {
                        b = m;
                    }
                }
                let x = 0.5 * (a + b);
                let (s, t) = point(x);
                if sign * cell.g.at(s, t) >= -rounding {
                    out.push((x, cell.at(s, t)));
                }
            }
            out
        };
        let Some(verdicts) = valleys.iter().map(verdict).collect::<Option<Vec<Valley>>>() else {
            continue;
        };
        let mut tangents = Vec::new();
        let mut arcs = Vec::new();
        let mut met = Vec::new();
        for (valley, verdict) in valleys.iter().zip(verdicts) {
            match verdict {
                Valley::Tangent => tangents.push(valley.clone()),
                Valley::Clear => {}
                Valley::Split => {
                    // The side arcs end where the branches meet: below the
                    // valley the arc runs on one branch to the meeting and the
                    // other branch past it, where they cross there (ssi-corpus
                    // y23_t2_ex3_loop_two_curves, a loop crossing two curves).
                    let found = meets(valley);
                    let at: Vec<f64> = found.iter().map(|m| m.0).collect();
                    if !sweep_sides(cell, valley, &d, sign, CLEAR * scale, &at, &mut arcs) {
                        return None;
                    }
                    met.extend(found.into_iter().map(|m| m.1));
                }
            }
        }
        // The lines the valley does not cross: g monotone along each.
        let mut covered: Vec<(f64, f64)> = valleys.iter().map(|v| (v.x0, v.x1)).collect();
        covered.sort_by(|p, q| p.0.total_cmp(&q.0));
        let mut gaps = Vec::new();
        let mut from = 0.0;
        for (x0, x1) in covered {
            if x0 > from {
                gaps.push((from, x0));
            }
            from = f64::max(from, x1);
        }
        if from < 1.0 {
            gaps.push((from, 1.0));
        }
        for (x0, x1) in gaps {
            if !sweep_lines(cell, 1 - lines, x0, x1, CLEAR * scale, &mut arcs) {
                return None;
            }
        }
        return Some((tangents, arcs, met));
    }
    None
}

/// A contact curve of higher order across a cell: where g's k-th derivative
/// `d_k` along one parameter's lines has one sign (k = 3 or 4) and `g_ss` does
/// not, each line has at most k zeros, and the curve m where d_(k-1) vanishes
/// is a simple root on each line, swept like an arc. By Taylor's theorem about
/// m, with |`d_j`(m)| <= `B_j` for j <= k - 2 and |`d_k`| >= κ on the cell, g has no
/// zero farther than r from m along the line when
/// Σ `B_j` / (j! r^(k - j)) < κ / k!; with r the resolution the curve is then the
/// surfaces' contact curve to it (two sheets with third-order contact along a
/// line, crossing there with their normals parallel, ssi-corpus
/// `y23_t6_ex1_tangent_line`). `B_j` is proved on chords of the curve, not at
/// points: each `d_i` is bounded along the chord by its coefficients (`A_i`), the
/// curve is within δ = A_(k-1) / κ of the chord on each line, and so
/// |`d_j`(m)| <= Σ_(i = j..k-1) `A_i` δ^(i-j) / (i-j)! + `κ_max` δ^(k-j) / (k-j)!.
/// Only a curve that crosses every line of the cell is taken, or one along the
/// cell's high edge, left to the cell beyond.
#[allow(clippy::type_complexity)]
fn contact_curve(
    piece: &Piece,
    cell: &Cell,
    lines: usize,
    scale: f64,
    settings: &Settings,
) -> Option<(Vec<Sweep>, Vec<Sweep>, Vec<(f64, f64)>)> {
    let degree = if lines == 0 { cell.g.du } else { cell.g.dv };
    let mut ds = vec![cell.g.clone()];
    for _ in 0..4.min(degree) {
        let Some(next) = ds.last().map(|d| d.derivative(lines)) else {
            break;
        };
        ds.push(next);
    }
    let factorial = |n: usize| (1..=n).product::<usize>() as f64;
    let (cs, ct) = cell.at(0.5, 0.5);
    let e = piece.evaluate(cs, ct);
    let along = if lines == 0 {
        norm(e.ds) * (cell.s1 - cell.s0)
    } else {
        norm(e.dt) * (cell.t1 - cell.t0)
    };
    let r = settings.resolution / along;
    for k in 3..=4 {
        if degree < k {
            break;
        }
        let dk = ds.get(k)?;
        if !dk.one_sign(1e-14 * scale) {
            continue;
        }
        let (kappa, kappa_max) = dk.c.iter().fold((f64::INFINITY, 0.0_f64), |(lo, hi), x| {
            (lo.min(x.abs()), hi.max(x.abs()))
        });
        // Within the resolution by Taylor's theorem, or zero to the rounding of
        // each derivative's coefficients (CLEAR of the scale, times 2n for each
        // differencing): a contact to the input's own precision, where the
        // flatness puts no zero nearer than (rounding / κ)^(1/k) of it.
        let rounding = |j: usize| CLEAR * scale * (2.0 * degree as f64).powi(j as i32);
        // b holds k - 1 bounds, j = 0 to k - 2.
        let enough = |b: &[f64]| {
            b.iter().enumerate().all(|(j, &bj)| bj <= rounding(j))
                || b.iter()
                    .enumerate()
                    .map(|(j, &bj)| bj / (factorial(j) * r.powi((k - j) as i32)))
                    .sum::<f64>()
                    < kappa / factorial(k)
        };
        let largest = |c: &[f64]| c.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
        let curve = ds.get(k - 1)?;
        let high = if lines == 0 { 1 } else { 3 };
        let beyond = if lines == 0 {
            cell.s1 < 1.0 || piece.above[0]
        } else {
            cell.t1 < 1.0 || piece.above[1]
        };
        if beyond && roots(&curve.edge(high), CLEAR * scale).is_none() {
            let b: Vec<f64> = ds
                .iter()
                .take(k - 1)
                .map(|d| largest(&d.edge(high)))
                .collect();
            if enough(&b) {
                return Some((Vec::new(), Vec::new(), Vec::new()));
            }
            continue;
        }
        let mut sweeps = Vec::new();
        let curve_cell = Cell {
            s0: cell.s0,
            s1: cell.s1,
            t0: cell.t0,
            t1: cell.t1,
            g: curve.clone(),
            depth: cell.depth,
            free: false,
            open: None,
            scale: cell.scale,
            dscale: cell.dscale,
            divided: false,
        };
        sweep_cell(piece, &curve_cell, 1 - lines, CLEAR * scale, &mut sweeps);
        let [sweep] = sweeps.as_slice() else {
            continue;
        };
        if sweep.x0 != 0.0 || sweep.x1 != 1.0 {
            continue;
        }
        let local = |(s, t): (f64, f64)| {
            (
                (s - cell.s0) / (cell.s1 - cell.s0),
                (t - cell.t0) / (cell.t1 - cell.t0),
            )
        };
        let point = |x: f64| local(sweep.at(x));
        let chord = |p: (f64, f64), q: (f64, f64)| {
            // a holds k bounds, i = 0 to k - 1.
            let a: Vec<f64> = ds.iter().take(k).map(|d| largest(&d.along(p, q))).collect();
            let delta = a.last().map_or(f64::NAN, |&last| last / kappa);
            let b: Vec<f64> = (0..k - 1)
                .map(|j| {
                    a.iter()
                        .enumerate()
                        .skip(j)
                        .map(|(i, ai)| ai * delta.powi((i - j) as i32) / factorial(i - j))
                        .sum::<f64>()
                        + kappa_max * delta.powi((k - j) as i32) / factorial(k - j)
                })
                .collect();
            enough(&b)
        };
        // At the curve's ends and middle first, where it lies exactly (δ = 0).
        let (p0, p1, middle) = (point(0.0), point(1.0), point(0.5));
        if ![p0, middle, p1].iter().all(|&(s, t)| {
            enough(
                &ds.iter()
                    .take(k - 1)
                    .map(|d| d.at(s, t).abs())
                    .collect::<Vec<f64>>(),
            )
        }) {
            continue;
        }
        let mut chords = vec![(0.0, p0, 1.0, p1)];
        let mut budget = CHORDS;
        let mut proved = true;
        while let Some((x0, q0, x1, q1)) = chords.pop() {
            if chord(q0, q1) {
                continue;
            }
            budget -= 1;
            if budget == 0 {
                proved = false;
                break;
            }
            let xm = 0.5 * (x0 + x1);
            let qm = point(xm);
            chords.push((x0, q0, xm, qm));
            chords.push((xm, qm, x1, q1));
        }
        if proved {
            return Some((sweeps, Vec::new(), Vec::new()));
        }
    }
    None
}

/// The chords `valley` may halve a valley into before halving its cell instead.
const CHORDS: u32 = 64;

/// What a valley of g is, along every line it crosses: within the band of
/// zero (a tangent curve), below it (two zeros on each line, one on each
/// side), or above it (none).
#[derive(Clone, Copy)]
enum Valley {
    Tangent,
    Split,
    Clear,
}

/// The arcs on the two sides of a valley g dips below zero on (sign g convex
/// along the lines, its minimum m below zero): on each line, g is monotone
/// between the line's end and m, and has a zero there where the end's value
/// is of the other sign, bracketed by the end and m. Intervals end at the
/// roots of the line ends' edges, and at the valley's `meets`. False where an
/// edge vanishes throughout.
fn sweep_sides(
    cell: &Cell,
    valley: &Sweep,
    d: &Bern,
    sign: f64,
    clear: f64,
    meets: &[f64],
    out: &mut Vec<Sweep>,
) -> bool {
    let dir = valley.dir;
    let (low_edge, high_edge) = if dir == 0 {
        (cell.g.edge(2), cell.g.edge(3))
    } else {
        (cell.g.edge(0), cell.g.edge(1))
    };
    let value = |x: f64, y: f64| {
        if dir == 0 {
            cell.g.at(x, y)
        } else {
            cell.g.at(y, x)
        }
    };
    for (edge, end, below) in [(low_edge, 0.0, true), (high_edge, 1.0, false)] {
        let Some(found) = roots(&edge, clear) else {
            return false;
        };
        let mut breaks = vec![valley.x0, valley.x1];
        breaks.extend(
            found
                .into_iter()
                .chain(meets.iter().copied())
                .filter(|&x| x > valley.x0 && x < valley.x1),
        );
        breaks.sort_by(f64::total_cmp);
        for w in breaks.windows(2) {
            let &[x0, x1] = w else {
                continue;
            };
            if x1 > x0 && sign * value(0.5 * (x0 + x1), end) > 0.0 {
                let bracket = if below {
                    Bracket::Below(Box::new(valley.clone()))
                } else {
                    Bracket::Above(Box::new(valley.clone()))
                };
                out.push(Sweep {
                    cell_s: (cell.s0, cell.s1),
                    cell_t: (cell.t0, cell.t1),
                    g: cell.g.clone(),
                    dg: d.clone(),
                    dir,
                    x0,
                    x1,
                    bracket,
                });
            }
        }
    }
    true
}

/// The arcs of g over swept values [a, b] of a cell along whose lines g is
/// monotone there (its derivative along them of one sign on each line, not
/// over the cell): each line's one zero, where its ends differ in sign,
/// bracketed by them. Intervals end at the roots of the cell's two edges
/// across the lines. False where an edge vanishes throughout.
fn sweep_lines(cell: &Cell, dir: usize, a: f64, b: f64, clear: f64, out: &mut Vec<Sweep>) -> bool {
    let (low_edge, high_edge) = if dir == 0 {
        (cell.g.edge(2), cell.g.edge(3))
    } else {
        (cell.g.edge(0), cell.g.edge(1))
    };
    let (Some(low), Some(high)) = (roots(&low_edge, clear), roots(&high_edge, clear)) else {
        return false;
    };
    let mut breaks = vec![a, b];
    breaks.extend(low.into_iter().chain(high).filter(|&x| x > a && x < b));
    breaks.sort_by(f64::total_cmp);
    let value = |x: f64, y: f64| {
        if dir == 0 {
            cell.g.at(x, y)
        } else {
            cell.g.at(y, x)
        }
    };
    let dg = cell.g.derivative(1 - dir);
    for w in breaks.windows(2) {
        let &[x0, x1] = w else {
            continue;
        };
        let mid = 0.5 * (x0 + x1);
        if x1 > x0 && (value(mid, 0.0) > 0.0) != (value(mid, 1.0) > 0.0) {
            out.push(Sweep {
                cell_s: (cell.s0, cell.s1),
                cell_t: (cell.t0, cell.t1),
                g: cell.g.clone(),
                dg: dg.clone(),
                dir,
                x0,
                x1,
                bracket: Bracket::Whole,
            });
        }
    }
    true
}

/// One arc in a monotone cell: g's zero set over [x0, x1] of the swept
/// parameter (cell-local), its other coordinate solved for at each.
#[derive(Clone)]
struct Sweep {
    cell_s: (f64, f64),
    cell_t: (f64, f64),
    g: Bern,
    /// g's derivative along the solved coordinate, of one sign on the cell
    /// (on each side of the valley, for a bracket beside one).
    dg: Bern,
    /// 0: swept along s, solved for t (∂g/∂t of one sign); 1: the reverse.
    dir: usize,
    x0: f64,
    x1: f64,
    bracket: Bracket,
}

/// Where on each line the root is bracketed: across the cell, or on one side
/// of a valley (its own sweep, of g's derivative along the lines).
#[derive(Clone)]
enum Bracket {
    Whole,
    Below(Box<Sweep>),
    Above(Box<Sweep>),
}

impl Sweep {
    /// The piece-local (s, t) of the zero at swept value x (cell-local): the
    /// root in y of g, bracketed by [0, 1], by Newton steps kept inside the
    /// bracket (bisection where one would leave it), to the last bits.
    ///
    /// Recursion: a bracket's valley is one of `sweep_cell`'s sweeps, whose
    /// bracket is `Whole` (`sweep_sides` is given no other), so this nests once
    /// at most, whatever the input.
    fn at(&self, x: f64) -> (f64, f64) {
        // g and its derivative on the line x, once: each Newton step is then a
        // one-variable evaluation (bivariate ones of a degree-30 field were most
        // of a free-form pair's emission, ssi-corpus y23_fig1b_two_arcs_and_point).
        let (g_line, dg_line) = (self.g.line(self.dir, x), self.dg.line(self.dir, x));
        let at = |b: &[f64], y: f64| casteljau(b, y);
        let valley = |v: &Sweep| {
            let (s, t) = v.at(x);
            if self.dir == 0 {
                (t - self.cell_t.0) / (self.cell_t.1 - self.cell_t.0)
            } else {
                (s - self.cell_s.0) / (self.cell_s.1 - self.cell_s.0)
            }
        };
        let (mut lo, mut hi) = match &self.bracket {
            Bracket::Whole => (0.0_f64, 1.0_f64),
            Bracket::Below(v) => (0.0, valley(v)),
            Bracket::Above(v) => (valley(v), 1.0),
        };
        let (f_lo, f_hi) = (at(&g_line, lo), at(&g_line, hi));
        let y = if f_lo == 0.0 {
            lo
        } else if f_hi == 0.0 {
            hi
        } else if (f_lo > 0.0) == (f_hi > 0.0) {
            // No sign change at this x (an interval's end, by rounding): the nearer end.
            if f_lo.abs() <= f_hi.abs() {
                lo
            } else {
                hi
            }
        } else {
            let mut y = lo - f_lo * (hi - lo) / (f_hi - f_lo);
            for _ in 0..100 {
                let f = at(&g_line, y);
                if f == 0.0 {
                    break;
                }
                if (f > 0.0) == (f_lo > 0.0) {
                    lo = y;
                } else {
                    hi = y;
                }
                let step = f / at(&dg_line, y);
                // Converged before the bracket's update can move the point: a root
                // lands on the bracket's end, and the next point is then no longer
                // strictly inside it (returning the bracket's middle was a bug).
                if step.is_finite() && step.abs() <= 2.0 * f64::EPSILON * y.abs().max(1e-300) {
                    break;
                }
                let next = y - step;
                y = if next > lo && next < hi && step.is_finite() {
                    next
                } else {
                    0.5 * (lo + hi)
                };
                if (hi - lo) <= 4.0 * f64::EPSILON {
                    break;
                }
            }
            y
        };
        let (cs, ct) = if self.dir == 0 { (x, y) } else { (y, x) };
        (
            self.cell_s.0 + cs * (self.cell_s.1 - self.cell_s.0),
            self.cell_t.0 + ct * (self.cell_t.1 - self.cell_t.0),
        )
    }
}

/// The 3D size of a cell of a piece: its longer diagonal.
fn cell_size(piece: &Piece, c: &Cell) -> f64 {
    let p = |s: f64, t: f64| piece.evaluate(s, t).point;
    norm(sub(p(c.s1, c.t1), p(c.s0, c.t0))).max(norm(sub(p(c.s1, c.t0), p(c.s0, c.t1))))
}

/// What a piece's zero set comes to, or None when a cell cannot be resolved.
///
/// `derivative`: a local field's proved bands of its gradient and Hessian per
/// unit of the piece's parameters, from which each cell's `dscale` follows at
/// its width (a gradient's coefficients scale with it, a Hessian's with its
/// square); None for an implicit form's g, `dscale` then `scale`.
fn cells(
    piece: &Piece,
    g: Bern,
    scale: f64,
    surf: &Field,
    settings: &Settings,
    derivative: Option<(f64, f64)>,
) -> Option<Found> {
    let dscale_at = |width: f64, scale: f64| {
        derivative.map_or(scale, |(gradient, hessian)| {
            ((gradient * width).max(hessian * width * width) / 1e-14).max(scale)
        })
    };
    // An edge row this small is zero: the field's rounding, or the implicit
    // form's residual where that is larger (zero_edges).
    let exact = 2.0 * surf.residual() * scale;
    let mut found = Found::default();
    let mut stack = vec![Cell {
        s0: 0.0,
        s1: 1.0,
        t0: 0.0,
        t1: 1.0,
        g,
        depth: 0,
        free: false,
        open: None,
        scale,
        dscale: dscale_at(1.0, scale),
        divided: false,
    }];
    while let Some(cell) = stack.pop() {
        let (scale, clear) = (cell.scale, CLEAR * cell.scale);
        if cell.g.one_sign(clear) {
            continue;
        }
        let monotone = (0..2)
            .rev()
            .find(|&d| cell.g.derivative(d).one_sign(1e-14 * cell.dscale));
        if let Some(d) = monotone {
            // ∂g/∂t of one sign: sweep along s (dir 0); ∂g/∂s: sweep along t (dir 1).
            let dir = 1 - d;
            sweep_cell(piece, &cell, dir, clear, &mut found.sweeps);
            continue;
        }
        if let Some(divided) = zero_edges(piece, &cell, exact) {
            found.sweeps.extend(divided.crossings);
            found.tangents.extend(divided.tangents);
            found.meets.extend(divided.meets);
            stack.push(Cell {
                g: divided.g,
                scale: divided.scale,
                dscale: cell.dscale.max(divided.scale),
                divided: true,
                ..cell
            });
            continue;
        }
        let mut free = cell.free;
        let mut open = cell.open;
        if !free && open.is_none() {
            match critical_claiming(&cell.g, cell.dscale, matches!(surf, Field::Local(..))) {
                Inside::Free => free = true,
                Inside::One(c) => match contact(piece, &cell, &c, surf, clear, settings) {
                    Contact::Touch => {
                        found.touches.push(cell.at(c.s, c.t));
                        continue;
                    }
                    Contact::Apart => continue,
                    Contact::Crossing => {
                        crossing(piece, &cell, &c, settings, clear, &mut found, &mut stack);
                        continue;
                    }
                    Contact::Open => open = Some(cell.at(c.s, c.t)),
                },
                Inside::Unknown => {
                    if let Some((tangents, arcs, met)) = valley(piece, &cell, scale, settings) {
                        found.tangents.extend(tangents);
                        found.sweeps.extend(arcs);
                        found.meets.extend(met);
                        continue;
                    }
                }
            }
        }
        // A free cell may get far smaller than the resolution (beside a
        // crossing's box), but not without end: to depth 60 the rounding band
        // about a touch point split three cells a level (y23_fig1g).
        let floor = if free {
            settings.resolution / 64.0
        } else {
            settings.resolution
        };
        if cell.depth > if free { 60 } else { 40 } || cell_size(piece, &cell) < floor {
            // Zero to its rounding throughout, free of critical points, and
            // within the resolution of a touch point found: the rounding band
            // about that touch, where no zero set is told from the touch itself
            // (a disk 2e-6 across about one, ssi-corpus y23_fig1g_three_points).
            let center = piece
                .evaluate(cell.at(0.5, 0.5).0, cell.at(0.5, 0.5).1)
                .point;
            let near_touch = found.touches.iter().any(|&(s, t)| {
                norm(sub(piece.evaluate(s, t).point, center)) <= 2.0 * settings.resolution
            });
            if free && near_touch && cell.g.max_abs() <= clear {
                continue;
            }
            return None;
        }
        let (sm, tm) = (0.5 * (cell.s0 + cell.s1), 0.5 * (cell.t0 + cell.t1));
        let (left, right) = cell.g.split(0, 0.5);
        for (g, s0, s1) in [(left, cell.s0, sm), (right, sm, cell.s1)] {
            let (low, high) = g.split(1, 0.5);
            for (g, t0, t1) in [(high, tm, cell.t1), (low, cell.t0, tm)] {
                let mut half = Cell {
                    s0,
                    s1,
                    t0,
                    t1,
                    g,
                    depth: cell.depth + 1,
                    free,
                    open: None,
                    scale,
                    dscale: if derivative.is_some() {
                        dscale_at((s1 - s0).max(t1 - t0), scale)
                    } else {
                        cell.dscale
                    },
                    divided: false,
                };
                match open {
                    Some(x) if half.holds(x) => half.open = Some(x),
                    Some(_) => half.free = true,
                    None => {}
                }
                stack.push(half);
            }
        }
    }
    Some(found)
}

/// The deepest cells whose edges are tried as zero edges: lines 1/16 of the
/// piece apart. Deeper, a cell's rows are small because the cell is, beside
/// the zero set it is near, not because g vanishes on its edge; and every
/// division widens the band its halves read (a cell 2^-23 across divided
/// down to degree 25 from 36 x 54, its band 5e4, ssi-corpus
/// `y23_fig1d_two_quads`).
const ZERO_EDGE_DEPTH: u32 = 4;

/// The highest order of a zero edge: contact of fourth order, as
/// `contact_curve` traces at most.
const MAX_ORDER: usize = 4;

/// A cell's g with its zero edges divided out, and those edges' arcs.
struct Divided {
    g: Bern,
    scale: f64,
    /// Zero edges of order 1: crossing curves.
    crossings: Vec<Sweep>,
    /// Zero edges of higher order: tangent curves.
    tangents: Vec<Sweep>,
    /// Piece-local points where the rest of the zero set reaches a zero edge,
    /// and where two zero edges meet.
    meets: Vec<(f64, f64)>,
}

/// The cell's edges on which g vanishes, divided out: g = x^m h for the
/// factor x of edge e (`Bern::divide_edge`), the edge itself a branch of the
/// zero set and h's zero set the rest of g's. Where a branch lies on a line
/// the cells are split along (the tangent curves of symmetric models, on
/// s = 1/2 or t = 1/2) or on the patch's own edge, g is zero to its rounding
/// on whole cells beside it, and they are unresolved to every other test: a
/// cubic contact ending on a tube's crease, where the other tube's form is
/// singular (ssi-corpus `y23_t2_ex5_two_loops`), and a boundary meeting a
/// tangent curve (`y23_fig1d_two_quads`). An edge is zero to order m when its
/// first m rows of coefficients are (s^m divides g exactly when g's first m
/// rows from s = 0 vanish), each within the cell's rounding or `exact` (the
/// implicit form's residual) where that is larger. The rows are read on g
/// itself, every edge's before any is divided: dividing out one edge
/// multiplies the coefficients beside it by up to the degree, its neighbors'
/// rows with them (a triangle's two zero sides, the second read after the
/// first was divided, was not zero, ssi-corpus `y23_fig1c_boundary_triangle`).
/// Dividing m times in one direction multiplies a coefficient by up to C(n, m),
/// at the edge (by about 1 across the cell from it), its signal and its
/// rounding alike: h's coefficients are as clear of their rounding as g's
/// were, and h's band is the cell's, or the largest row dropped where that is
/// larger (the form's residual). A band times C(n, m) over the whole cell was
/// 1431 times too wide across it, past g's values (`y23_fig1d_two_quads`). A
/// cell that is rounding throughout has no zero edges, only rounding (the
/// band about a touch point).
///
/// The edges are emitted by the cell that owns them, as `sweep_cell` keeps
/// zero edges: its low edges, and its high ones on the patch's boundary;
/// crossing curves at order 1, tangent curves above. Where h has a root on a
/// zero edge (its ends included), or two zero edges meet, branches meet: a
/// vertex, and the edge's arc ends there.
fn zero_edges(piece: &Piece, cell: &Cell, exact: f64) -> Option<Divided> {
    if cell.divided || cell.depth > ZERO_EDGE_DEPTH {
        return None;
    }
    let original = CLEAR * cell.scale;
    let (du, dv) = (cell.g.du, cell.g.dv);
    // A composition of high degree rounds its coefficients by more than the
    // band read in its scale (a monomial's size, not the sum of the form's
    // terms): a few ulps of g's largest coefficient per degree is less than
    // that rounding, never more (a triangle's side at 1.2e-13 in a
    // degree-39 x 14 field of largest coefficient 3.2, the band 1e-14,
    // ssi-corpus y23_fig1c_boundary_triangle).
    let composed = 16.0 * f64::EPSILON * (du + dv) as f64 * cell.g.max_abs();
    let zero = original.max(exact).max(composed);
    // The k-th row of coefficients from edge e, at its largest.
    // A coefficient past the net counts as large: no row is taken for zero on it.
    let row = |e: usize, k: usize| -> f64 {
        let at = |i: usize, j: usize| {
            cell.g
                .c
                .get(i * (dv + 1) + j)
                .map_or(f64::INFINITY, |x| x.abs())
        };
        match e {
            0 => (0..=dv).map(|j| at(k, j)).fold(0.0, f64::max),
            1 => (0..=dv).map(|j| at(du - k, j)).fold(0.0, f64::max),
            2 => (0..=du).map(|i| at(i, k)).fold(0.0, f64::max),
            _ => (0..=du).map(|i| at(i, dv - k)).fold(0.0, f64::max),
        }
    };
    let mut order = [0usize; 4];
    let mut dropped = [0.0_f64; 4];
    for (e, (order_e, dropped_e)) in order.iter_mut().zip(&mut dropped).enumerate() {
        let degree = if e < 2 { du } else { dv };
        while *order_e < degree && row(e, *order_e) <= zero {
            *dropped_e = dropped_e.max(row(e, *order_e));
            *order_e += 1;
        }
        // A zero edge's rows give way to g's own beyond its order: past the
        // fourth order, or into rows hardly above the band, they fade into
        // rounding, the cell's rather than an edge's.
        if *order_e > 0
            && (*order_e > MAX_ORDER || *order_e >= degree || !(row(e, *order_e) > 4.0 * zero))
        {
            return None;
        }
    }
    if order == [0; 4] || order[0] + order[1] >= du.max(1) || order[2] + order[3] >= dv.max(1) {
        return None;
    }
    let mut g = cell.g.clone();
    let rounding = dropped.iter().fold(original, |r, &d| r.max(d));
    for (e, &order_e) in order.iter().enumerate() {
        for _ in 0..order_e {
            g = g.divide_edge(e);
        }
    }
    if g.max_abs() <= 64.0 * rounding {
        return None;
    }
    let owns = |e: usize| match e {
        1 => !(cell.s1 < 1.0 || piece.above[0]),
        3 => !(cell.t1 < 1.0 || piece.above[1]),
        _ => true,
    };
    let mut out = Divided {
        g: Bern {
            du: 0,
            dv: 0,
            c: Vec::new(),
        },
        scale: rounding / CLEAR,
        crossings: Vec::new(),
        tangents: Vec::new(),
        meets: Vec::new(),
    };
    for (e, &order_e) in order.iter().enumerate().filter(|&(_, &o)| o > 0) {
        let on_edge = |x: f64| match e {
            0 => cell.at(0.0, x),
            1 => cell.at(1.0, x),
            2 => cell.at(x, 0.0),
            _ => cell.at(x, 1.0),
        };
        // The rest of the zero set reaching the edge, at its ends too (a
        // triangle's crossing side meeting a zero side at the cell's corner,
        // ssi-corpus y23_fig1c_boundary_triangle).
        let reached: Vec<f64> = roots(&g.edge(e), rounding)
            .into_iter()
            .flatten()
            .map(|x| x.clamp(0.0, 1.0))
            .collect();
        out.meets.extend(reached.iter().map(|&x| on_edge(x)));
        let mut breaks = vec![0.0, 1.0];
        breaks.extend(reached);
        breaks.sort_by(f64::total_cmp);
        breaks.dedup_by(|x, y| (*x - *y).abs() <= 1e-12);
        if !owns(e) {
            continue;
        }
        for w in breaks.windows(2) {
            let &[x0, x1] = w else {
                continue;
            };
            if !(x1 > x0) {
                continue;
            }
            let sweep = edge_sweep(cell, e, x0, x1);
            if order_e == 1 {
                out.crossings.push(sweep);
            } else {
                out.tangents.push(sweep);
            }
        }
    }
    for (e, f, corner) in [
        (0, 2, (0.0, 0.0)),
        (0, 3, (0.0, 1.0)),
        (1, 2, (1.0, 0.0)),
        (1, 3, (1.0, 1.0)),
    ] {
        let zero_edge = |e: usize| order.get(e).is_some_and(|&o| o > 0);
        if zero_edge(e) && zero_edge(f) {
            out.meets.push(cell.at(corner.0, corner.1));
        }
    }
    out.g = g;
    Some(out)
}

/// The sweep of the cell's edge e from x0 to x1 along it (cell-local): the
/// root of the edge's own factor (s, 1 - s, t or 1 - t, as `Bern::divide_edge`
/// numbers them) on each line across it, the edge itself.
fn edge_sweep(cell: &Cell, e: usize, x0: f64, x1: f64) -> Sweep {
    let factor = if matches!(e, 0 | 2) {
        vec![0.0, 1.0]
    } else {
        vec![-1.0, 0.0]
    };
    // Edges s = 0, 1 are swept along t (dir 1), edges t = 0, 1 along s.
    let (g, dir) = if e < 2 {
        (
            Bern {
                du: 1,
                dv: 0,
                c: factor,
            },
            1,
        )
    } else {
        (
            Bern {
                du: 0,
                dv: 1,
                c: factor,
            },
            0,
        )
    };
    Sweep {
        cell_s: (cell.s0, cell.s1),
        cell_t: (cell.t0, cell.t1),
        g,
        dg: Bern {
            du: 0,
            dv: 0,
            c: vec![1.0],
        },
        dir,
        x0,
        x1,
        bracket: Bracket::Whole,
    }
}

/// The arcs of a monotone cell. Along s (dir 0) the zero set crosses each line
/// s = const at most once, between t = 0 and t = 1 where g's values there
/// differ in sign; its intervals end at the roots of the cell's bottom and top
/// edges and at the cell's sides. An edge on which g vanishes throughout is
/// itself the zero set: kept when it is the cell's low edge, left to the cell
/// beyond when it is the high edge and something lies beyond (its low edge).
fn sweep_cell(piece: &Piece, cell: &Cell, dir: usize, clear: f64, out: &mut Vec<Sweep>) {
    let dg = cell.g.derivative(1 - dir);
    let make = |x0: f64, x1: f64| Sweep {
        cell_s: (cell.s0, cell.s1),
        cell_t: (cell.t0, cell.t1),
        g: cell.g.clone(),
        dg: dg.clone(),
        dir,
        x0,
        x1,
        bracket: Bracket::Whole,
    };
    // The edges across the sweep: low (t = 0 or s = 0) and high.
    let (low_edge, high_edge) = if dir == 0 {
        (cell.g.edge(2), cell.g.edge(3))
    } else {
        (cell.g.edge(0), cell.g.edge(1))
    };
    let beyond_high = if dir == 0 {
        cell.t1 < 1.0 || piece.above[1]
    } else {
        cell.s1 < 1.0 || piece.above[0]
    };
    let (low_roots, high_roots) = (roots(&low_edge, clear), roots(&high_edge, clear));
    match (&low_roots, &high_roots) {
        (None, _) => {
            out.push(make(0.0, 1.0));
            return;
        }
        (_, None) => {
            if !beyond_high {
                out.push(make(0.0, 1.0));
            }
            return;
        }
        _ => {}
    }
    let mut breaks = vec![0.0, 1.0];
    breaks.extend(low_roots.into_iter().flatten());
    breaks.extend(high_roots.into_iter().flatten());
    breaks.sort_by(f64::total_cmp);
    breaks.dedup_by(|x, y| (*x - *y).abs() <= 1e-15);
    let value = |x: f64, y: f64| {
        if dir == 0 {
            cell.g.at(x, y)
        } else {
            cell.g.at(y, x)
        }
    };
    for w in breaks.windows(2) {
        let &[x0, x1] = w else {
            continue;
        };
        if !(x1 > x0) {
            continue;
        }
        let mid = 0.5 * (x0 + x1);
        if (value(mid, 0.0) > 0.0) != (value(mid, 1.0) > 0.0) {
            out.push(make(x0, x1));
        }
    }
}

/// Turning sweeps into curve points: the other patch's parameters, its domain's
/// clipping, and the orientation along nA × nB.
struct Emit<'e> {
    piece: &'e Piece,
    side: &'e Side,
    p_is_a: bool,
    spacing: &'e Spacing,
    /// A rectangle [u0, u1, v0, v1] of the other patch's parameters points must
    /// lie in, besides its domain: its piece, where a pair of pieces owns
    /// only its own share (`trace_local`).
    clip: Option<[f64; 4]>,
    /// A local field's other piece and direction: its zeros are the
    /// separation's only to its band, so each point is polished onto the
    /// intersection before it is placed (`taylor::polish`).
    local: Option<(&'e Piece, V3)>,
}

impl Emit<'_> {
    fn point(
        &self,
        sweep: &Sweep,
        x: f64,
        hint: Option<(f64, f64)>,
    ) -> Option<(CurvePoint, f64, (f64, f64))> {
        let (s, t) = self.placed(sweep.at(x));
        self.point_at(s, t, self.piece.evaluate(s, t).point, hint)
    }

    /// (s, t) polished onto the intersection for a local field, as it is otherwise.
    fn placed(&self, at: (f64, f64)) -> (f64, f64) {
        match self.local {
            Some((b, dir)) => crate::taylor::polish(self.piece, b, dir, at),
            None => at,
        }
    }

    /// The curve point at piece-local (s, t), whose 3D point is p: the other
    /// patch's parameters and the margin they have inside its domain.
    fn point_at(
        &self,
        s: f64,
        t: f64,
        p: V3,
        hint: Option<(f64, f64)>,
    ) -> Option<(CurvePoint, f64, (f64, f64))> {
        let piece = self.piece;
        if let Some((b, dir)) = self.local {
            return self.ray_point(s, t, p, b, dir);
        }
        let quv = self.side.place(p, hint)?;
        let mine = Param {
            patch: piece.patch,
            u: piece.u0 + s * (piece.u1 - piece.u0),
            v: piece.v0 + t * (piece.v1 - piece.v0),
        };
        let mut margin = self.side.margin(quv, 0.0);
        if let Some([u0, u1, v0, v1]) = self.clip {
            let d = self.side.domain;
            let (su, sv) = (d[1] - d[0], d[3] - d[2]);
            margin = margin
                .min((quv.0 - u0) / su)
                .min((u1 - quv.0) / su)
                .min((quv.1 - v0) / sv)
                .min((v1 - quv.1) / sv);
        }
        let theirs = Param {
            patch: self.side.index,
            u: quv.0,
            v: quv.1,
        };
        let (a, b) = if self.p_is_a {
            (mine, theirs)
        } else {
            (theirs, mine)
        };
        Some((CurvePoint { p, a, b }, margin, quv))
    }

    /// `point_at` for a local field: B's parameters where the line through p
    /// along the field's direction meets B's piece (inside that piece's square
    /// its polynomial is the patch), the point halfway to it, and the margin
    /// inside the piece's square and the patch's domain.
    fn ray_point(
        &self,
        s: f64,
        t: f64,
        p: V3,
        b: &Piece,
        dir: V3,
    ) -> Option<(CurvePoint, f64, (f64, f64))> {
        let (u, v, l) = crate::taylor::ray(b, dir, p, (0.5, 0.5, 0.0))?;
        let quv = (b.u0 + u * (b.u1 - b.u0), b.v0 + v * (b.v1 - b.v0));
        let d = self.side.domain;
        let (su, sv) = (d[1] - d[0], d[3] - d[2]);
        let margin = self
            .side
            .margin(quv, 0.0)
            .min((u.min(1.0 - u) * (b.u1 - b.u0) / su).min(v.min(1.0 - v) * (b.v1 - b.v0) / sv));
        let piece = self.piece;
        let mine = Param {
            patch: piece.patch,
            u: piece.u0 + s * (piece.u1 - piece.u0),
            v: piece.v0 + t * (piece.v1 - piece.v0),
        };
        let theirs = Param {
            patch: self.side.index,
            u: quv.0,
            v: quv.1,
        };
        let (a, bp) = if self.p_is_a {
            (mine, theirs)
        } else {
            (theirs, mine)
        };
        let half = 0.5 * l;
        let at = [
            p[0] + half * dir[0],
            p[1] + half * dir[1],
            p[2] + half * dir[2],
        ];
        Some((CurvePoint { p: at, a, b: bp }, margin, quv))
    }

    /// The sweep's arcs inside the other patch's domain; oriented along nA × nB
    /// when `orient`, else along the sweep (on a tangent curve nA × nB vanishes).
    fn emit(&self, sweep: &Sweep, orient: bool) -> Vec<Vec<CurvePoint>> {
        let span = sweep.x1 - sweep.x0;
        let x_of = |l: f64| sweep.x0 + l * span;
        // Sampled on the field's own zero set, smooth; each sample then placed.
        let curve = |l: f64| {
            let (s, t) = sweep.at(x_of(l));
            (self.piece.evaluate(s, t).point, (s, t))
        };
        let samples: Vec<(f64, V3, (f64, f64))> = sample_points(&curve, self.spacing)
            .into_iter()
            .map(|(l, p, at)| {
                let (s, t) = self.placed(at);
                if (s, t) == at {
                    (l, p, at)
                } else {
                    (l, self.piece.evaluate(s, t).point, (s, t))
                }
            })
            .collect();
        let mut runs = Vec::new();
        let mut run: Vec<CurvePoint> = Vec::new();
        let mut hint = None;
        let mut previous: Option<(f64, bool)> = None;
        for &(l, p, (s, t)) in &samples {
            let here = self.point_at(s, t, p, hint);
            let inside = here.as_ref().is_some_and(|(_, m, _)| *m >= -1e-12);
            if let Some((l_prev, was_in)) = previous {
                if was_in != inside {
                    // The domain's boundary between the two samples, by bisection.
                    let (mut lo, mut hi) = (l_prev, l);
                    for _ in 0..60 {
                        let mid = 0.5 * (lo + hi);
                        let in_mid = self
                            .point(sweep, x_of(mid), hint)
                            .is_some_and(|(_, m, _)| m >= 0.0);
                        if in_mid == was_in {
                            lo = mid;
                        } else {
                            hi = mid;
                        }
                    }
                    if let Some((q, _, _)) =
                        self.point(sweep, x_of(if was_in { lo } else { hi }), hint)
                    {
                        run.push(q);
                    }
                    if was_in {
                        runs.push(std::mem::take(&mut run));
                    }
                }
            }
            if let Some((q, _, quv)) = here {
                hint = Some(quv);
                if inside {
                    run.push(q);
                }
            }
            previous = Some((l, inside));
        }
        runs.push(run);
        runs.retain(|r| r.len() > 1);
        if orient {
            for arc in &mut runs {
                self.orient(arc);
            }
        }
        runs
    }

    /// The point where the surfaces touch at piece-local (s, t): halfway between
    /// the piece's point and its foot on the other surface, with the parameters
    /// of that foot on the other patch; None outside its domain.
    /// Parameters a little outside the domain that, clamped into it, name the
    /// same point are inside it: on a collapsed edge any u names the pole (a
    /// touch at the other patch's pole inverted to u = -1.4e-5, ssi-corpus
    /// `y23_fig1g_three_points`).
    ///
    /// The field's foot is a step along its gradient, and where an algebraic
    /// form is singular (its patch's sheet crossing another of its sheets,
    /// the gradient vanishing there) it lands far off: 9.4e-3 from a
    /// transversal point on such a double curve, and the crossing was lost
    /// (ssi-corpus `y23_t2_ex5_two_loops`). Where the foot is not on the other
    /// patch, the other patch's own closest point is tried.
    fn contact_point(&self, s: f64, t: f64, surf: &Field) -> Option<CurvePoint> {
        let p = self.piece.evaluate(s, t).point;
        self.contact_at(s, t, p, surf.foot(p)).or_else(|| {
            let uv = self.side.invert(p, None)?;
            self.contact_at(s, t, p, self.side.at(uv.0, uv.1).0)
        })
    }

    /// `contact_point` with the foot f of the piece's point p.
    fn contact_at(&self, s: f64, t: f64, p: V3, f: V3) -> Option<CurvePoint> {
        let (mut q, margin, quv) = self.point_at(s, t, f, None)?;
        q.p = [
            0.5 * (p[0] + f[0]),
            0.5 * (p[1] + f[1]),
            0.5 * (p[2] + f[2]),
        ];
        if margin >= -1e-12 {
            return Some(q);
        }
        let d = self.side.domain;
        let clamped = (quv.0.clamp(d[0], d[1]), quv.1.clamp(d[2], d[3]));
        let size = norm(sub(
            [
                self.piece.bounds()[3],
                self.piece.bounds()[4],
                self.piece.bounds()[5],
            ],
            [
                self.piece.bounds()[0],
                self.piece.bounds()[1],
                self.piece.bounds()[2],
            ],
        ));
        if norm(sub(self.side.at(clamped.0, clamped.1).0, f)) > 1e-9 * size {
            return None;
        }
        let theirs = Param {
            patch: self.side.index,
            u: clamped.0,
            v: clamped.1,
        };
        if self.p_is_a {
            q.b = theirs;
        } else {
            q.a = theirs;
        }
        Some(q)
    }

    /// A crossing's segments: from each point where a branch leaves the box
    /// about it to the crossing itself, oriented along nA × nB at the box's end
    /// (at the crossing nA × nB vanishes).
    fn crossing(&self, center: CurvePoint, ends: &[(f64, f64)]) -> Vec<Vec<CurvePoint>> {
        let mut out = Vec::new();
        for &end in ends {
            let (s, t) = self.placed(end);
            let Some((q, margin, quv)) = self.point_at(s, t, self.piece.evaluate(s, t).point, None)
            else {
                continue;
            };
            if margin < -1e-12 {
                continue;
            }
            let e = self.piece.evaluate(s, t);
            let (n_mine, n_theirs) = (cross(e.ds, e.dt), self.side.at(quv.0, quv.1).1);
            let tangent = if self.p_is_a {
                cross(n_mine, n_theirs)
            } else {
                cross(n_theirs, n_mine)
            };
            out.push(if dot(tangent, sub(center.p, q.p)) >= 0.0 {
                vec![q, center]
            } else {
                vec![center, q]
            });
        }
        out
    }

    /// Reversed when its direction runs against nA × nB.
    fn orient(&self, arc: &mut [CurvePoint]) {
        let i = arc.len() / 2;
        let (j, k) = (
            i.saturating_sub(1),
            (i + 1).min(arc.len().saturating_sub(1)),
        );
        let (Some(&q), Some(qj), Some(qk)) = (arc.get(i), arc.get(j), arc.get(k)) else {
            return;
        };
        let chord = sub(qk.p, qj.p);
        let (mine, theirs) = if self.p_is_a { (q.a, q.b) } else { (q.b, q.a) };
        let piece = self.piece;
        let e = piece.evaluate(
            (mine.u - piece.u0) / (piece.u1 - piece.u0),
            (mine.v - piece.v0) / (piece.v1 - piece.v0),
        );
        let n_mine = cross(e.ds, e.dt);
        let n_theirs = self.side.at(theirs.u, theirs.v).1;
        let tangent: V3 = if self.p_is_a {
            cross(n_mine, n_theirs)
        } else {
            cross(n_theirs, n_mine)
        };
        if dot(tangent, chord) < 0.0 {
            arc.reverse();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// g = s^2 + t^2 - 1/2 on the unit square, in Bernstein form of degree (2, 2).
    fn quarter_circle() -> Bern {
        let mut c = vec![0.0; 9];
        for i in 0..3 {
            for j in 0..3 {
                c[i * 3 + j] = [0.0, 0.0, 1.0][i] + [0.0, 0.0, 1.0][j] - 0.5;
            }
        }
        Bern { du: 2, dv: 2, c }
    }

    /// Σ p[i][j] s^i t^j, i, j up to 2, in Bernstein form of degree (2, 2).
    fn power(p: [[f64; 3]; 3]) -> Bern {
        // Bernstein coefficients of 1, x and x^2 in degree 2.
        let basis = [[1.0, 1.0, 1.0], [0.0, 0.5, 1.0], [0.0, 0.0, 1.0]];
        let mut c = vec![0.0; 9];
        for i in 0..3 {
            for j in 0..3 {
                for a in 0..3 {
                    for b in 0..3 {
                        c[a * 3 + b] += p[i][j] * basis[i][a] * basis[j][b];
                    }
                }
            }
        }
        Bern { du: 2, dv: 2, c }
    }

    /// k1 (s - s0)^2 + k2 (t - t0)^2 + k12 (s - s0)(t - t0) + value.
    fn quadric(s0: f64, t0: f64, k1: f64, k2: f64, k12: f64, value: f64) -> Bern {
        power([
            [
                k1 * s0 * s0 + k2 * t0 * t0 + k12 * s0 * t0 + value,
                -2.0 * k2 * t0 - k12 * s0,
                k2,
            ],
            [-2.0 * k1 * s0 - k12 * t0, k12, 0.0],
            [k1, 0.0, 0.0],
        ])
    }

    fn one(inside: Inside) -> Critical {
        match inside {
            Inside::One(c) => c,
            Inside::Free => panic!("free"),
            Inside::Unknown => panic!("unknown"),
        }
    }

    #[test]
    fn a_minimum_and_a_saddle_are_found_with_their_index() {
        let c = one(critical(&quadric(0.3, 0.65, 1.0, 2.0, 0.5, 0.0), 1.0));
        assert_eq!(c.index, 1);
        assert!(
            (c.s - 0.3).abs() < 1e-14 && (c.t - 0.65).abs() < 1e-14,
            "{} {}",
            c.s,
            c.t
        );
        let c = one(critical(&quadric(0.4, 0.55, 1.0, -1.5, 0.3, 0.0), 1.0));
        assert_eq!(c.index, -1);
        assert!(
            (c.s - 0.4).abs() < 1e-14 && (c.t - 0.55).abs() < 1e-14,
            "{} {}",
            c.s,
            c.t
        );
    }

    #[test]
    fn g_along_a_segment_and_raised_keeps_its_values() {
        let g = quadric(0.3, 0.65, 1.0, -2.0, 0.7, 0.25);
        for (p, q) in [
            ((0.2, 0.9), (0.7, 0.1)),
            ((0.9, 0.3), (0.1, 0.35)),
            ((0.5, 0.0), (0.5, 1.0)),
        ] {
            let line = g.along(p, q);
            let raised = elevate1(&line, line.len() + 3);
            for k in 0..=16 {
                let x = f64::from(k) / 16.0;
                let want = g.at(p.0 + x * (q.0 - p.0), p.1 + x * (q.1 - p.1));
                assert!(
                    (casteljau(&line, x) - want).abs() < 1e-14,
                    "{p:?} {q:?} {x}"
                );
                assert!(
                    (casteljau(&raised, x) - want).abs() < 1e-14,
                    "{p:?} {q:?} {x}"
                );
            }
        }
    }

    #[test]
    fn an_edge_factor_divides_out_exactly() {
        let h = quadric(0.3, 0.65, 1.0, -2.0, 0.7, 0.25);
        let factor = |du: usize, dv: usize, c: [f64; 2]| Bern {
            du,
            dv,
            c: c.to_vec(),
        };
        for (e, x) in [
            (0, factor(1, 0, [0.0, 1.0])),
            (1, factor(1, 0, [1.0, 0.0])),
            (2, factor(0, 1, [0.0, 1.0])),
            (3, factor(0, 1, [1.0, 0.0])),
        ] {
            let g = h.mul(&x);
            assert!(g.edge(e).iter().all(|&c| c == 0.0));
            let back = g.divide_edge(e);
            assert_eq!((back.du, back.dv), (h.du, h.dv));
            assert!(
                back.c.iter().zip(&h.c).all(|(a, b)| (a - b).abs() < 1e-15),
                "edge {e}"
            );
        }
    }

    /// A flat piece over the unit square, for the cell tests' edge rules.
    fn flat_piece() -> Piece {
        let c = vec![
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
            [1.0, 1.0, 0.0, 1.0],
        ];
        Piece::new(0, 0.0, 1.0, 0.0, 1.0, 1, 1, c, [false, false])
    }

    fn whole(g: Bern) -> Cell {
        Cell {
            s0: 0.0,
            s1: 1.0,
            t0: 0.0,
            t1: 1.0,
            g,
            depth: 0,
            free: false,
            open: None,
            scale: 1.0,
            dscale: 1.0,
            divided: false,
        }
    }

    /// g = s^2 h, h = t - 0.4 + s/2: the edge s = 0 is a tangent curve, split
    /// where h's own zero set reaches it, and h is what is left.
    #[test]
    fn a_zero_edge_is_divided_out_and_traced() {
        let h = Bern {
            du: 1,
            dv: 1,
            c: vec![-0.4, 0.6, 0.1, 1.1],
        };
        let s2 = Bern {
            du: 2,
            dv: 0,
            c: vec![0.0, 0.0, 1.0],
        };
        let divided = zero_edges(&flat_piece(), &whole(h.mul(&s2)), 0.0).expect("a zero edge");
        assert!(divided.crossings.is_empty());
        assert_eq!(divided.tangents.len(), 2);
        assert_eq!((divided.g.du, divided.g.dv), (1, 1));
        assert!(divided
            .g
            .c
            .iter()
            .zip(&h.c)
            .all(|(a, b)| (a - b).abs() < 1e-15));
        assert_eq!(divided.meets.len(), 1);
        let (ms, mt) = divided.meets[0];
        assert!(ms == 0.0 && (mt - 0.4).abs() < 1e-12, "{ms} {mt}");
        for sweep in &divided.tangents {
            let (s, t) = sweep.at(0.5 * (sweep.x0 + sweep.x1));
            assert!(s == 0.0 && t > 0.0 && t < 1.0, "{s} {t}");
        }
    }

    /// Coefficients of rounding throughout are no zero edge, only rounding.
    #[test]
    fn a_cell_of_rounding_has_no_zero_edges() {
        let g = Bern {
            du: 1,
            dv: 1,
            c: vec![1e-17, -2e-17, 3e-17, -1e-17],
        };
        assert!(zero_edges(&flat_piece(), &whole(g), 0.0).is_none());
    }

    #[test]
    fn a_cell_without_a_critical_point_is_free() {
        assert!(matches!(
            critical(&quadric(1.4, 0.5, 1.0, 1.0, 0.0, -0.1), 1.0),
            Inside::Free
        ));
    }

    /// On the cell's corner, ∇g vanishes on its boundary: the grown square finds it.
    #[test]
    fn a_critical_point_on_a_corner_is_the_cells() {
        let c = one(critical(&quadric(0.0, 1.0, 1.0, 3.0, -0.5, 0.0), 1.0));
        assert_eq!(c.index, 1);
        assert!(
            c.s.abs() < 1e-12 && (c.t - 1.0).abs() < 1e-12,
            "{} {}",
            c.s,
            c.t
        );
    }

    #[test]
    fn a_sweep_solves_every_point_to_a_root_of_g() {
        let g = quarter_circle();
        let dg = g.derivative(1);
        let sweep = Sweep {
            cell_s: (0.0, 1.0),
            cell_t: (0.0, 1.0),
            g,
            dg,
            dir: 0,
            x0: 0.0,
            x1: 0.5f64.sqrt(),
            bracket: Bracket::Whole,
        };
        for k in 0..=64 {
            let (s, t) = sweep.at(sweep.x1 * f64::from(k) / 64.0);
            assert!((s * s + t * t - 0.5).abs() < 1e-15, "s {s} t {t}");
        }
    }
}
