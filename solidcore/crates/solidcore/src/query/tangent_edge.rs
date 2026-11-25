//! Whether the two faces of an edge meet tangentially all along it, proved from the faces'
//! NURBS forms and the edge uses' pcurves.
//!
//! Upstream: none (not in SMLib; SMLib's `IsTangentSector` samples five fixed stations,
//! [`crate::topology::edgeuse::p2::is_tangent_sector`]).
//!
//! | Rust                    | Upstream |
//! |-------------------------|----------|
//! | [`tangent_all_along`]   | (none)   |
//! | [`Tangency`]            | (none)   |
//! | [`g2_all_along`], [`G2`] | (none)   |
//! | `bernstein_roots` (private) | (none) |
//! | [`tube_regularity`], [`TubeRegularity`] | (none) |
//! | [`side_along`], [`SideAlong`] | (none; SMLib's `EvaluateBinormal` gives one point's) |
//!
//! **The polynomials.** Each use of the edge carries its pcurve `(u(t), v(t)) = (X, Y) / W`
//! on the edge's own parameter `t`, and its face's surface is its NURBS form `A / w` in the
//! form's parameters (every analytic surface is one; a blend is taken as its net). On a piece
//! of `t` where the pcurve stays within one of its knot spans and one knot cell of its net,
//! these are polynomials, of degree `r` and `(p, q)`. The net's natural normal times `w^4` is
//! `N_w = (A_u w - A w_u) x (A_v w - A w_v)`, a polynomial in `(u, v)` of total degree
//! `m = 4p + 4q - 2` (`2p + 2q - 2` where the weights are one value, `w` then constant; 0 for
//! a plane); along the pcurve `W^m N_w` is a polynomial in `t` of degree `m r`. With each use's
//! `F = s W^m N_w`, `s` the sign turning it into the face use's normal, `P = F_a x F_c` and
//! `Q = F_a . F_c` are polynomials in `t` of degree at most the sum `D` of the uses', and the
//! angle between the face use normals is `atan2(|P|, Q)` (`w`, `W` positive).
//!
//! **The values.** `F` at a point `t_j` is evaluated in rounded interval arithmetic
//! ([`Ivl`]): the pcurve's homogeneous `(X, Y, W)` by its B-spline basis, `(u, v)` as a box,
//! the net's homogeneous point and partials over that box (the hull over the knot spans the
//! box meets), then `N_w` and `W^m`. So `P(t_j)` and `Q(t_j)` lie in the intervals computed.
//!
//! **The decision.** On each piece, at `D + 1` Chebyshev points `t_j`, a polynomial of degree
//! `D` is its interpolant, `P(t) = sum_j P(t_j) L_j(t)`: for every unit `e`,
//! `|e . P(t)| <= max_j |P(t_j)| sum_j |L_j(t)|`, so `|P(t)| <= L M` between the points, `M`
//! the largest `|P(t_j)|` enclosed and `L` the Lebesgue constant of the points, under
//! `1 + (2/pi) ln(D + 1)` (Rivlin, *An Introduction to the Approximation of Functions*, 1.4;
//! rounded up). Since the `L_j` sum to 1, `Q(t) >= c - L r` for `c` the middle and `r` the
//! half range of the hull of the `Q(t_j)`. Where that is positive, the faces' angle is at most
//! `atan(L M / (c - L r))` all over the piece. That is never under `atan(L tan)` of the angle
//! itself, however short the piece, so a second bound is taken from the same points and the
//! smaller of the two kept. For any `u`, `E = u . P` and `R = P - E u` are polynomials of degree
//! `D`, and `|P|^2 = (2 - |u|^2) E^2 + |R|^2` (as `u . R = (1 - |u|^2) E`). `|R|` is at most
//! `L` times its largest, as `|P|` is. For any `s >= 0`, `G = s Q - E` is a polynomial of degree
//! `D`, at least some `G_lo` over the piece, so with `Q_lo = c - L r > 0` of `Q`,
//! `E = s Q - G <= (s + max(0, -G_lo) / Q_lo) Q`, and `-E <= max(0, -E_lo) / Q_lo Q`. So
//! `|E| <= k Q`, `k` the larger, `|R| <= k_R Q`, and the angle is at most
//! `atan(sqrt((2 - |u|^2) k^2 + k_R^2))`. `u`, along `P` at the middle point, and `s`, the largest
//! `E / Q` at the points, are chosen, not proved (any are sound). `R` is of the order of the
//! piece's width and enters squared, and what `P` and `Q` share cancels in `G`: where the
//! angle is constant, or largest inside the piece, the bound is over it by the order of the
//! width squared. Where `c - L r` is not positive (a right angle or more somewhere
//! on the piece, or an enclosure too wide to tell), it is bounded from `P`'s part along a fixed
//! direction: for any `e`, `E = e . P` is a polynomial of degree `D` too, and `E <= |e| |P|`.
//! Where `E`'s lower end over the piece, `c - L r` of its values, is some `E_lo > 0`, take any
//! `s >= 0`: `G = Q + s E` is a polynomial of degree `D`, at least some `G_lo` over the piece, and
//! `Q = G - s E >= -(max(0, -G_lo) / E_lo + s) E >= -(max(0, -G_lo) / E_lo + s) |e| |P| = -k |P|`.
//! So the angle is at most `atan2(1, -k)`, as `atan2(|P|, Q)` falls as `Q` grows. `e` and `s` are
//! chosen, not proved (any are sound): `e` is the enclosure's middle of `P` at the middle point
//! and `s` the largest `-Q / E` at the points. `P` and `Q` vary together along an edge (the
//! nets' weights, the pcurves' speed), and in `G` that cancels: where the angle is constant, or
//! largest inside the piece, the bound is over it by the order of the piece's width squared, where
//! bounds of `|P|` and `Q` each on its own would be over by the order of its width. Then:
//! - **not tangent** where at one point the angle's lower end, `atan2` of `|P|`'s lower end and
//!   `Q`'s upper end, less the piece's margin, exceeds the angle asked: a witness;
//! - **tangent** where on every piece the bound plus the piece's margin is within it (exactly
//!   parallel nets, every `P(t_j)` exactly 0 and every `Q(t_j)` positive, give 0);
//! - otherwise the piece is halved, at most `MAX_SPLITS` times, and a piece still neither is
//!   [`Tangency::Undecided`].
//!
//! **Rounded points.** The points `t_j` are the exact Chebyshev points `t*_j` rounded: in the
//! piece's `[-1, 1]` scale `|t_j - t*_j| <= delta` (`node_error`: the angle
//! `(2j + 1) pi / 2n` within `gamma_4 pi`, its cosine within 2 ulps of `f64::cos` (the count
//! taken for the platform's `cos`), and `m + h c` within `gamma_3 (|m| / h + 1)`). By Markov's
//! inequality `|P'| <= D^2 ||P||` on `[-1, 1]`, so `|e . P(t*_j)| <= M + delta D^2 ||P||` for
//! each unit `e`, and the interpolant at the exact points gives
//! `||P|| <= L (M + delta D^2 ||P||)`, that is `||P|| <= L M / (1 - L delta D^2)`, used in
//! place of `L M` (refused, the piece undecided, where `L delta D^2 >= 1`). Likewise
//! `Q(t) >= c - L (r + delta D^2 ||Q||)` with `||Q|| <= L A / (1 - L delta D^2)`, `A` the
//! largest `|Q(t_j)|` enclosed, and each `E` and `G` the same (`poly_range`), `R` as `P`.
//!
//! **The pieces.** The edge is cut at the pcurves' knots (exact values: a pcurve's polynomial
//! changes exactly there). A piece is taken as one polynomial only where both pcurves' `(u, v)`
//! enclosures over the whole piece meet no interior knot line of their nets: that is proved,
//! not found by a root to rounding. A piece whose enclosure meets one is cut, at most
//! `MAX_CELL_DEPTH` times, and meanwhile bounded directly. Where a line's crossing is found
//! (bisection on the pcurve's point between the piece's ends; at the nearer end where the line
//! lies in the enclosure's rounding there) within `SLIVER` (a sixteenth) of the piece of one
//! of its ends, as a line through a pcurve's knot is, that share of the piece is cut off that
//! end, so the rest is one cell at once and the end part shrinks sixteen times per cut, not
//! twice; otherwise the piece is halved. The crossing is a guess: every part is proved one cell or
//! not as any piece is. Bounded directly: the enclosures of `P` and `Q` over
//! the whole piece (the hull over the cells it meets) give an angle of at most
//! `atan(|P|_hi / Q_lo)` on it, or `atan2(|P|_lo, Q_lo)` where `Q_lo <= 0 < |P|_lo` (`atan2(y, x)`
//! falls as `x` grows, and for `x <= 0` as `y` grows; where `Q > 0` the angle is under `pi / 2`,
//! which that is not), and its middle point is a witness as above. So a piece around
//! a crossing, or along a knot line, is decided by its enclosure and never by the
//! interpolation, which holds only on one cell.
//!
//! **The net's domain.** The certificate is about the surface the face's evaluator evaluates.
//! Past its net's domain that evaluator clamps the point to the domain (SMLib's practice: the
//! B-spline surface's `evaluate_simple` unless its form is polynomial or out-of-bounds
//! evaluation is on, the cone's and a blend's the same), so a pcurve's box is clamped as it
//! does; a box straddling an end is not one cell (the clamp bends `F` there) and is bounded
//! over its box. At a point, a clamp that moves the face's point more than the edge's
//! tolerance from the net's at the pcurve's own parameters (the net continued past its knots)
//! means the pcurve is not on its face (a periodic face's pcurve a period off): refused,
//! naming it. A round's pcurve 1e-16 past its net's end, as fillets make them, moves it by
//! that much. A plane is exempt (its net continued is the same plane), and so is a face whose
//! evaluator continues its net. Over a whole piece, where the pcurve's weight is not proved
//! positive (its enclosure loose, not its value), the box is its control points' hull.
//!
//! **The cost.** Each piece between the pcurves' knots is decided on a budget of `MAX_POINTS`
//! evaluations; a piece past it is [`Tangency::Undecided`], not an error.
//!
//! **Platforms.** The points use the platform's `cos`, the bounds its `ln`, `atan`, `atan2` and
//! `acos`, each taken within 2 ulps: the verdicts are sound on any platform where that holds
//! and may differ in the last bit between platforms.
//!
//! **The sense.** The two uses are an edge use and its radial partner, the face uses on the
//! two sides of one region. Their face use normals (the surface's natural normal turned by the
//! face use's orientation, as [`evaluate_binormal`] gives it) agree where the faces meet
//! smoothly and are opposite where they fold back on each other (a cusp). The sign `s` is read
//! once per use, at the first point where its `N_w` is proved non-zero, from
//! [`evaluate_binormal`] (a point where it gives no normal is skipped). Any cosine above 0
//! fixes the sign; 60 degrees (`SAME_SURFACE_COS`) also refuses a net that is not the face's
//! surface at all (a face whose evaluator and net were built apart), where the two normals
//! would disagree by more than any rounding.
//!
//! **A blend's net.** A procedural blend's face is decided on its net, the polynomial the proof
//! needs, and its definition is carried as a margin over each piece: the pcurve's `(u, v)` over
//! the piece is enclosed as a box, the definition's `S_u x S_v` over it by
//! [`definition_partials_over_box`] and the net's `N_w` over it as above, and the angle between
//! any two vectors of those enclosures, from `cos >= (D . B)_lo / (|D|_hi |B|_hi)`, is the
//! margin (pi where that is not positive). The margin is the sum of both uses'. A face that is
//! not a blend has margin 0. The angle between the two enclosed normals is bounded twice and
//! the smaller taken: by `acos` of the cosine's lower end, and, the angle being proved acute
//! and both lengths positive, by `asin` of the sine's upper end, `|D x B|_hi / (|D|_lo |B|_lo)`
//! (rounded up, clamped at 1). For a small angle `t`, `1 - cos t` is about `t^2 / 2`, so an
//! enclosure of width `w` in the cosine costs about `sqrt(2 w)` in the angle; the sine is about
//! `t`, so the same width costs about `w`. Over the small boxes the margin is asked on, the
//! sine's bound is the one that decides (on `circle_root`'s round it fell with the box as
//! `sqrt` of its width by the cosine alone). Two ways it can fail are kept apart: too loose
//! (the cosine not proved over a wide box; a halved piece may prove it, so the piece is
//! halved) and not
//! enclosed (the definition's enclosure fails over the box: `gn_def_face_of` refuses the
//! definition, or `definition_partials_over_box` proves nothing there). An enclosure can fail
//! on a box and hold on its halves, so a not-enclosed part is halved too, but only
//! `MAX_UNENCLOSED_SPLITS` times; past that it is undecided. Either way nothing is decided
//! without a bounded margin.

//!
//! **A rounded offset's tube ([`tube_regularity`]).** A rounded offset caps a convex edge
//! `c(t)` with the tube of radius `d` about it: the points `c + d u` for `u` on the arc
//! between the two face use normals turned to the offset's side (#13685). That sweep is
//! regular where `1 - d (u . k) > 0`, `k = (c'' - (c'' . c') c' / |c'|^2) / |c'|^2` the
//! curvature vector (no Frenet frame: `k` is defined, and small, where the edge is straight),
//! and folds over itself past it. On a piece of `t`, `c'` and `c''` are enclosed from the
//! edge's NURBS form (`curve_derivs_over`) and the normals from the uses' `F` (as above), and
//! `max over the arc of u . k` is bounded twice: from below by the arc's two ends (their
//! enclosures' lower ends), or by `|k_p|` (`k_p` the projection of `k` on the arc's plane)
//! where `k_p`'s direction is proved inside the arc; from above by `|k|` unless `k_p`'s
//! direction is proved outside the arc, and then by the two ends. A piece whose lower bound
//! times `d` is at least 1 is a fold; one whose upper bound times `d` is below 1 is regular;
//! any other is halved, at most `MAX_TUBE_HALVINGS` times and to at most `MAX_TUBE_PIECES`
//! pieces an edge, and is undecided past them. A piece whose normals or speed are not proved
//! non-zero has no bound and is halved likewise. Where the normals are within rounding of one
//! line (`1 - g^2`, `g = u_a . u_c`, not proved positive), the arc is short only if `g` is proved
//! positive: then its ends plus `|k| sqrt(1 - g^2)` bound it from above; else (the normals near
//! opposite, the arc near a half circle) only `|k|` does. The normals are the face uses' on
//! the side of the edge use the offset builds from (their mates' turned round, when the edge's
//! first use is on the other side). A blend's net only approximates its definition: where a
//! use carries a margin `m` (the angle within which its definition's normal lies of its net's,
//! as above), every point of the definition's arc lies within `2 m / (cos(theta / 2) - m)` of
//! the net's arc (`cos(theta / 2) = sqrt((1 + g) / 2)`, the arcs matched by their chords'
//! points), so both bounds widen by `|k|` times that; a piece whose margin is too loose is
//! halved, and one whose definition is not enclosed is undecided. A fold names the largest radius at which the
//! tube is proved not to fold (only the tube: other defects can appear at or near it):
//! with `L` the largest lower bound over the pieces proved to fold, no radius past `U = 1 / L`
//! is regular (the tube folds there on that piece), and `U <= d`; the same decision is run at
//! `U (1 - 2^-j)` for `j` = 8 down to 1, and the first proved regular is named, none if none is.
//!
//! **G2: the curvature across ([`g2_all_along`]).** Each use's normal curvature across the
//! edge (in the tangent plane, square to the edge) is the shape operator's trace less the
//! normal curvature along the pcurve's tangent, `k = 2H - II(T) / I(T)`, so no direction across
//! is chosen. With `E, F, G` and `T = S_u u' + S_v v'`, and `|N|^2 = EG - F^2`,
//! `k = n . V / (|N|^2 |T|^2)`, `n` the unit normal, where
//! `V = (E S_vv - 2F S_uv + G S_uu) |T|^2 - (S_uu u'^2 + 2 S_uv u'v' + S_vv v'^2) |N|^2`.
//! In the net's homogeneous terms (`S = A / w`): `w^2 S_u = A_u w - A w_u`,
//! `w^3 S_uu = w^2 A_uu - w (2 w_u A_u + w_uu A) + 2 w_u^2 A`, `w^3 S_uv = w^2 A_uv -
//! w (w_u A_v + w_v A_u + w_uv A) + 2 w_u w_v A` (one constant weight `c`: `c S_u = A_u`,
//! `c S_uu = A_uu`); the pcurve's `u' = (X'W - XW') / W^2`. Written in those (starred) terms,
//! `k = n . V~` with `V~ = w V* / (|N*|^2 |T*|^2)`, `T* = S*_u U* + S*_v V'*` and
//! `U* = X'W - XW'`, `V'* = Y'W - YW'`. Along the pcurve, `P^ = W^k w V*` and
//! `Q^ = W^k |N*|^2 |T*|^2`, `k = 6 d`, are polynomials in `t` of one degree
//! `6 d r + 4 r - 4` (`g2_degrees`), with `d` the degree of `S*_u` in `(u, v)`: `2p + 2q - 1`
//! for a rational net, `p + q - 1` for one weight; `V~ = P^ / Q^`. A plane's `V~` is 0.
//!
//! **Tangency first.** G2 is a smooth edge's: [`g2_all_along`] first decides the tangency at
//! the angle asked ([`tangent_all_along`]). A crease is [`G2::NotTangent`] at that cost, and
//! an edge whose tangency is undecided is never [`G2::Continuous`]: it is [`G2::Jumps`] or
//! [`G2::Undecided`]. Its jump is still bounded as below, since a part whose bound is within
//! `max_jump` provably holds no witness and is dropped: only the parts that may still jump
//! are halved.
//!
//! **The jump.** Both curvatures are read against use a's face use normal `n_a = F_a / |F_a|`
//! (`F` as above): `k_b = n_b . V~_b` differs from `n_a . V~_b` by at most
//! `|n_a - n_b| |V~_b| <= theta |V~_b|`, `theta` the angle between the normals, bounded over a
//! piece from the same points' `P` and `Q` as [`tangent_all_along`] does. So
//! `k_a - k_b = J / G +- theta |P^_b| / Q^_b` with the polynomial
//! `J = F_a . (P^_a Q^_b - P^_b Q^_a)` and `G = |F_a| Q^_a Q^_b`. On a piece, at the `D + 1`
//! Chebyshev points (`D` the degree of `J`, the largest of the piece's polynomials), `|J|`,
//! `|P^_b|` and the curvatures' numerators are bounded as `P` is above, and `|F_a|^2`, `Q^_a`
//! and `Q^_b` from below as `Q` is (each factor of `G` on its own). Then, with the scale
//! `max(|k_a|, |k_b|, 1 / L)` (`L` the edge's measured length, a stated scale, not a bound):
//! - **continuous** where the jump's bound (plus a blend's margin) is within `max_jump` of the
//!   scale's lower bound on every piece;
//! - **jumps** where at one point the jump, each use's curvature against its own normal and
//!   evaluated directly, exceeds `max_jump` of the scale's upper bound there (less the margin):
//!   a witness (at a point on a knot line, on each closed cell's polynomial, the face from
//!   that side);
//! - otherwise halved, as above, and [`G2::Undecided`] past the splits or the budget. A point
//!   where `|N|^2`, `|T|^2` or `cos theta` is not bounded away from 0 (a pole, an apex, a
//!   crease) leaves its piece undecided. A piece across a knot line is bounded over its
//!   box cell by cell: each pair of the two uses' cells it meets, each cell's polynomial
//!   continued over the box (the point at each `t` is in one of each), the largest jump
//!   taken. A blend's margin is `|k_def - k_net|` over the piece's box, its definition's
//!   partials to second order by [`definition_partials_over_box`]; where they are not
//!   enclosed the piece is undecided.
//!
//! Deviations: no upstream counterpart. `bernstein_roots` (used by `sweep::stations`)
//! recurses at most `MAX_DEPTH` (48) deep and `MAX_SUBDIVISIONS` (4096) times per call; a piece
//! is cut at most `MAX_CELL_DEPTH` (48) times to find its cells (halved, or a sixteenth cut
//! off an end a knot line runs through) and `MAX_SPLITS` (12) times to bound it, on an explicit stack; each piece
//! between the pcurves' knots evaluates at most `MAX_POINTS` (one million) points of a cross product of degree at most `MAX_DEGREE` (4096).

use crate::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
use crate::blend::fillet_section::definition_box::{definition_partials_over_box, gn_def_face_of};
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::station::{as_b_spline, span, Nurbs};
use crate::error::{Result, SolidError};
use crate::geom::interval_vector::{cross, dot, norm, square};
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::nurbs::b_spline_surface::BSplineSurfaceForm;
use crate::nurbs::crv_basic::Curve as NurbCurve;
use crate::nurbs::math::{gamma, CPoint, NOW, UNIT};
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::{edge, Brep, EdgeId, EdgeuseId};

const OP: &str = "query::tangent_edge";

/// Bisection steps that place a knot line's crossing on a part ([`Use::crossing`]).
const CROSSING_STEPS: usize = 60;

/// Most points the decision evaluates on one piece between the pcurves' knots.
const MAX_POINTS: usize = 1_000_000;
/// Points the decision evaluates between polls of the kernel's cancel token (the mass
/// enclosure's stride: every 64 cells); the first point of a decision polls too.
const POLL_POINTS: usize = 64;
/// Largest degree of a piece's cross product the decision takes.
const MAX_DEGREE: usize = 4096;
/// Halvings of one piece the decision makes before leaving it undecided.
const MAX_SPLITS: u32 = 12;
/// Halvings of a part over which a blend's definition is not enclosed, before leaving it
/// undecided: a work budget, not a tolerance (past it the part is undecided, never decided).
/// The definition's enclosure over a box can fail on a box and hold on its halves (on
/// `circle_root`'s round, parts that mended did so within two halvings); four leaves room and
/// caps a part at 16 such pieces.
const MAX_UNENCLOSED_SPLITS: u32 = 4;
/// Halvings of a piece that meets a knot line, to find the cells it lies in: a crossing's
/// piece is then 2^-48 of a pcurve span, decided by its enclosure.
const MAX_CELL_DEPTH: u32 = 48;
/// Subdivisions of one Bernstein polynomial while isolating its roots: its pieces are then
/// 2^-48 of a pcurve span, where a root left unisolated (a pcurve touching a knot line) ends a
/// piece there.
pub(crate) const MAX_DEPTH: u32 = 48;
/// Subdivisions one root isolation may make: a root that touches its knot line (a double
/// root, the only kind that keeps two sign changes as the pieces shrink) costs one a level.
pub(crate) const MAX_SUBDIVISIONS: usize = 4096;
/// Halvings of one piece [`tube_regularity`] makes before leaving it undecided: a piece is
/// then 2^-16 of one between the pcurves' knots. A work budget (#13685).
pub(crate) const MAX_TUBE_HALVINGS: u32 = 16;
/// Pieces [`tube_regularity`] decides on one edge: past it the rest are undecided. Over the
/// offset goldens and the kernel shell cases (13685-tubeprobe-4) a decided edge took at most
/// 2119 pieces; the two edges that reached this budget were left undecided near the limit,
/// on bodies whose check fails. A work budget, not a tolerance.
pub(crate) const MAX_TUBE_PIECES: usize = 4096;
/// The least cosine between a face use's normal from the face's evaluator and from its net
/// at one point: below it the net is not the face's surface and the face is refused.
const SAME_SURFACE_COS: f64 = 0.5;

fn unsupported(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason,
    }
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// An interval vector.
type I3 = [Ivl; 3];

fn scale(a: I3, s: Ivl) -> I3 {
    a.map(|x| x * s)
}

/// The knot spans `k` (`u[k] < u[k + 1]`, `p <= k < n`) of a knot vector of degree `p` with
/// `n` control points whose interval meets `[lo, hi]`; past the domain, its end span.
fn spans_meeting(u: &[f64], p: usize, n: usize, lo: f64, hi: f64) -> Vec<usize> {
    // the knots ascend, so the spans meeting `[lo, hi]` are those from the last with
    // `u[k + 1] < lo` (exclusive) to the last with `u[k] <= hi`: found by bisection, then each
    // tested as before (a NaN end finds none)
    let first = u.partition_point(|&x| x < lo).saturating_sub(1).max(p);
    let last = u.partition_point(|&x| x <= hi).min(n);
    let mut out: Vec<usize> = (first..last)
        .filter(|&k| match (u.get(k), u.get(k + 1)) {
            (Some(&a), Some(&b)) => a < b && a <= hi && b >= lo,
            _ => false,
        })
        .collect();
    if out.is_empty() {
        out.push(span(u, p, n, 0.5 * (lo + hi)));
    }
    out
}

/// The non-zero basis functions `N_{k-p..=k, p}` over `x` in span `k` and their first
/// derivatives, enclosed (the NURBS book's A2.2 for degrees `p - 1` and `p`;
/// `N' = p (N_{i,p-1} / (U_{i+p} - U_i) - N_{i+1,p-1} / (U_{i+p+1} - U_{i+1}))`).
fn basis(knots: &[f64], k: usize, p: usize, x: Ivl) -> Option<(Vec<Ivl>, Vec<Ivl>)> {
    let u = |i: usize| knots.get(i).copied().map(Ivl::from);
    let mut n = vec![Ivl::from(1.0)];
    let mut lower = Vec::new();
    for j in 1..=p {
        if j == p {
            lower.clone_from(&n);
        }
        let mut next = vec![Ivl::from(0.0); j + 1];
        let mut saved = Ivl::from(0.0);
        for r in 0..j {
            let denom = u(k + 1 + r)? - u((k + 1 + r).checked_sub(j)?)?;
            if !denom.is_positive() {
                return None;
            }
            let temp = *n.get(r)? / denom;
            let right = u(k + 1 + r)? - x;
            let left = x - u((k + 1 + r).checked_sub(j)?)?;
            *next.get_mut(r)? = saved + right * temp;
            saved = left * temp;
        }
        *next.get_mut(j)? = saved;
        n = next;
    }
    let mut d = vec![Ivl::from(0.0); p + 1];
    if p > 0 {
        let pf = Ivl::from(f64::from(u32::try_from(p).ok()?));
        for (j, dj) in d.iter_mut().enumerate() {
            let i = (k + j).checked_sub(p)?;
            let mut v = Ivl::from(0.0);
            if j >= 1 {
                let denom = u(i + p)? - u(i)?;
                if !denom.is_positive() {
                    return None;
                }
                v = v + *lower.get(j - 1)? / denom;
            }
            if j < p {
                let denom = u(i + p + 1)? - u(i + 1)?;
                if !denom.is_positive() {
                    return None;
                }
                v = v - *lower.get(j)? / denom;
            }
            *dj = pf * v;
        }
    }
    Some((n, d))
}

/// One use of the edge: its face's net, its pcurve, the degrees of its normal along a piece,
/// the knot lines of its net and its sense.
struct Use {
    eu: EdgeuseId,
    /// The face's net.
    net: Nurbs,
    /// Whether the net's weights are not all one value.
    rational: bool,
    /// The pcurve's NURBS form (its Bezier spans give the pieces' ends).
    nurb: NurbCurve,
    /// The pcurve's Bezier spans: each one's knots `[a, b]` and homogeneous control points
    /// `(x w, y w, w)`, and the bound on their rounding (`Use::sub_span_box`).
    pc_spans: Vec<BezSpan>,
    /// The pcurve's knots, degree and homogeneous control points `(x w, y w, w)`.
    pc_knots: Vec<f64>,
    pc_degree: usize,
    pc_points: Vec<[f64; 3]>,
    /// The degree `m` of the normal in `(u, v)`.
    uv_degree: usize,
    /// The degree of the normal along a piece, `m r`.
    degree: usize,
    /// The net's distinct knots in u and v over its domain: a polynomial cell's sides (none
    /// for a plane, whose normal is constant).
    lines: [Vec<f64>; 2],
    /// The sign `s` turning `N_w` into the face use's normal, once read.
    sign: Option<f64>,
    /// Whether the face is a plane: its net, extended past its domain, is the same plane, so
    /// a pcurve in the plane's own parameters (past the net's patch) is on it.
    plane: bool,
    /// The edge's tolerance: how far the face's point may lie from the pcurve's (`Use::uv_box`).
    tolerance: f64,
    /// Whether the face's evaluator clamps a point past its net's domain to it (`Use::uv_box`).
    clamps: bool,
    /// A procedural blend's surface, whose definition the net is measured against.
    definition: Option<Surface>,
}

/// The distinct knots of a knot vector of degree `p` strictly inside its domain
/// `[u[p], u[len - p - 1]]`: the lines between its polynomial cells (its domain's ends bound
/// the face's net, [`Use::uv`] refuses a pcurve past them).
fn knot_lines(u: &[f64], p: usize) -> Vec<f64> {
    let end = u.len().saturating_sub(p + 1);
    let mut out: Vec<f64> = u.get(p..=end).map(<[f64]>::to_vec).unwrap_or_default();
    out.dedup();
    // the domain's two ends are not lines between cells: drop the first and the last
    out.pop();
    out.into_iter().skip(1).collect()
}

/// The domain `[u[p], u[len - p - 1]]` of a knot vector of degree `p`.
fn domain(u: &[f64], p: usize) -> Option<(f64, f64)> {
    let end = u.len().checked_sub(p + 1)?;
    Some((*u.get(p)?, *u.get(end)?))
}

impl Use {
    fn new(k: &Kernel, b: &Brep, eu: EdgeuseId) -> Result<Self> {
        let f = eu1::face(b, eu)?.ok_or_else(|| invalid("an edge use without face"))?;
        let surface = b
            .face(f)?
            .surface
            .as_ref()
            .ok_or_else(|| invalid("a face without surface"))?;
        let plane = matches!(surface.kind(), SurfaceKind::Plane(_));
        let e = eu1::edge(b, eu)?.ok_or_else(|| invalid("an edge use without edge"))?;
        let tolerance = edge::tolerance(b, e)?;
        let definition = matches!(surface.kind(), SurfaceKind::ProceduralBlend(_))
            .then(|| surface.clone_for_evaluation())
            .transpose()?;
        let bs = as_b_spline(k, surface)?
            .ok_or_else(|| unsupported("a face whose surface has no NURBS form"))?;
        // a net in power-basis form holds its polynomial's coefficients, not control points
        // (`b_spline_surface_n_lib`'s evaluators switch to the power basis for it): this
        // certificate reads control points, so such a face is refused, named
        if bs.form() == BSplineSurfaceForm::Polynomial {
            return Err(unsupported("a face in power-basis (polynomial) form"));
        }
        let net = Nurbs::of(&bs)?;
        // the B-spline surface evaluator clamps a point past its domain unless out-of-bounds
        // evaluation is enabled (`b_spline_surface_n_lib`'s `evaluate_simple`; a polynomial
        // form, which does not clamp, is refused above); an analytic face's and a blend's
        // evaluators clamp to their net's domain the same way (the cone's `evaluate_step`,
        // `ProceduralBlend::evaluate`)
        let clamps = !bs.is_out_of_bounds_enabled();
        let pcurve = eu1::uv_trim_curve(b, eu)?
            .0
            .ok_or_else(|| unsupported("an edge use without a pcurve"))?;
        let base = pcurve
            .b_spline_base()
            .ok_or_else(|| unsupported("a pcurve with no NURBS form"))?;
        let r = base.degree()?;
        let (unique, mult) = base.knots(None)?;
        let mut knots = Vec::new();
        for (&t, &m) in unique.iter().zip(&mult) {
            knots.extend(std::iter::repeat_n(t, m));
        }
        // the control polygon's weights: none for a non-rational pcurve, else one each
        let (points, weights) = base.control_polygon()?;
        if !weights.is_empty() && weights.len() != points.len() {
            return Err(invalid(
                "a pcurve's weights do not match its control points",
            ));
        }
        let mut pw = Vec::new();
        let mut pc_points = Vec::new();
        for (i, p) in points.iter().enumerate() {
            if weights.is_empty() {
                pw.push(CPoint::new(p.x, p.y, 0.0, NOW));
                pc_points.push([p.x, p.y, 1.0]);
                continue;
            }
            let Some(&w) = weights.get(i) else {
                return Err(invalid(
                    "a pcurve's weights do not match its control points",
                ));
            };
            let positive = w > 0.0;
            if !positive {
                return Err(invalid("a pcurve's weight that is not positive"));
            }
            pw.push(CPoint::new(p.x * w, p.y * w, 0.0, w));
            pc_points.push([p.x * w, p.y * w, w]);
        }
        let nurb = NurbCurve::new(pw, r, knots.clone())?;
        // the Bezier spans by knot insertion (`decompose_bez`, Boehm's: every point a chain of
        // convex combinations of the control points, at most r for each of a span's two ends,
        // each a quotient, a difference and two products and a sum), so each coordinate is
        // within gamma_(12 r + 1) of the largest control point's same coordinate of its exact
        // value (per coordinate: X and Y can be far larger than W)
        let mut rounding = [0.0_f64; 3];
        for c in &pc_points {
            for (e, x) in rounding.iter_mut().zip(c) {
                *e = e.max(x.abs());
            }
        }
        let rounding = rounding.map(|m| gamma(12 * r + 1) * m);
        let mut pc_spans = Vec::new();
        for seg in nurb.decompose_bez()? {
            let (Some(&a), Some(&b)) = (seg.knots().first(), seg.knots().last()) else {
                continue;
            };
            let points = seg
                .cpts()
                .iter()
                .map(|c| [c.x, c.y, if c.w == NOW { 1.0 } else { c.w }])
                .collect();
            pc_spans.push(BezSpan {
                a,
                b,
                points,
                rounding,
            });
        }
        let w: Vec<f64> = net.pw.chunks(4).filter_map(|c| c.get(3).copied()).collect();
        if w.len() != net.nu * net.nv {
            return Err(invalid("a net's weights do not match its control points"));
        }
        let first = w
            .first()
            .copied()
            .ok_or_else(|| invalid("a net without control points"))?;
        let rational = w.iter().any(|&x| x != first);
        let (uv_degree, lines) = if plane {
            (0, [Vec::new(), Vec::new()])
        } else if rational {
            (
                (4 * net.p + 4 * net.q).saturating_sub(2),
                [knot_lines(&net.u, net.p), knot_lines(&net.v, net.q)],
            )
        } else {
            (
                (2 * net.p + 2 * net.q).saturating_sub(2),
                [knot_lines(&net.u, net.p), knot_lines(&net.v, net.q)],
            )
        };
        Ok(Self {
            eu,
            net,
            rational,
            nurb,
            pc_spans,
            pc_knots: knots,
            pc_degree: r,
            pc_points,
            uv_degree,
            degree: uv_degree.saturating_mul(r),
            lines,
            sign: None,
            plane,
            tolerance,
            clamps,
            definition,
        })
    }

    /// The pcurve's homogeneous `(X, Y, W)` over `t`, enclosed.
    fn pcurve(&self, t: Ivl) -> Result<[Ivl; 3]> {
        let (r, n) = (self.pc_degree, self.pc_points.len());
        let mut out: Option<[Ivl; 3]> = None;
        for kt in spans_meeting(&self.pc_knots, r, n, t.lo, t.hi) {
            let (nb, _) = basis(&self.pc_knots, kt, r, t)
                .ok_or_else(|| invalid("a pcurve's degenerate knots"))?;
            let mut h = [Ivl::from(0.0); 3];
            for (i, bi) in nb.iter().enumerate() {
                let c = (kt + i)
                    .checked_sub(r)
                    .and_then(|j| self.pc_points.get(j))
                    .ok_or_else(|| invalid("a pcurve's control point"))?;
                for (hx, cx) in h.iter_mut().zip(c) {
                    *hx = *hx + *bi * Ivl::from(*cx);
                }
            }
            out = Some(match out {
                None => h,
                Some(o) => [o[0].hull(h[0]), o[1].hull(h[1]), o[2].hull(h[2])],
            });
        }
        out.ok_or_else(|| invalid("a pcurve without a span"))
    }

    /// The box of the pcurve's control points that are active over `t` (its points over `t`
    /// lie in their hull, the weights being positive): `(u, v)`, each `x / w` enclosed.
    fn pcurve_hull(&self, t: Ivl) -> Option<(Ivl, Ivl)> {
        let (r, n) = (self.pc_degree, self.pc_points.len());
        let mut out: Option<(Ivl, Ivl)> = None;
        for kt in spans_meeting(&self.pc_knots, r, n, t.lo, t.hi) {
            for j in kt.checked_sub(r)?..=kt {
                let [x, y, w] = *self.pc_points.get(j)?;
                // a weight of exactly 1 (a polynomial pcurve) leaves the point exact; the
                // interval quotient would widen it by an ulp, and a pcurve on its net's end to
                // the last bit would then reach past it (and be taken as straddling the end)
                let (u, v) = if w == 1.0 {
                    (Ivl::from(x), Ivl::from(y))
                } else {
                    let w = Ivl::from(w);
                    (Ivl::from(x) / w, Ivl::from(y) / w)
                };
                out = Some(match out {
                    None => (u, v),
                    Some((a, b)) => (a.hull(u), b.hull(v)),
                });
            }
        }
        out
    }

    /// The box of the pcurve over `t`: the hull of the Bezier control points of each span it
    /// meets, cut to `t` by de Casteljau in interval arithmetic (the spans' points widened by
    /// their rounding), each `X / W` where every weight is proved positive; `None` otherwise.
    fn sub_span_box(&self, t: Ivl) -> Option<(Ivl, Ivl)> {
        let mut out: Option<(Ivl, Ivl)> = None;
        for span in &self.pc_spans {
            let (lo, hi) = (t.lo.max(span.a), t.hi.min(span.b));
            // a piece ending at a span's start meets that span only there, at a point the
            // span before also holds
            let start_only = hi == span.a && t.lo < t.hi;
            if lo > hi || start_only {
                continue;
            }
            let (u, v) = sub_span_hull(span, lo, hi)?;
            out = Some(match out {
                None => (u, v),
                Some((a, b)) => (a.hull(u), b.hull(v)),
            });
        }
        out
    }

    /// The `(u, v)` box of the pcurve over `t` as the face evaluates it (clamped to the net's
    /// domain, module docs), its `W`, and whether the box straddles an end of the domain (the
    /// clamp then bends `F` inside the piece); `None` where neither the pcurve's weight is
    /// proved positive over `t` nor its control points give a box (the piece is then halved).
    ///
    /// # Errors
    ///
    /// At a point, where the face's point at the clamped parameters is not proved within the
    /// edge's tolerance of the net's at the pcurve's (a pcurve leaving its face's net).
    fn uv_box(&self, t: Ivl) -> Result<Option<UvBox>> {
        let [x, y, w] = self.pcurve(t)?;
        // over a piece, the evaluation's enclosure loses the basis' partition of unity (a
        // pcurve along a line u = c encloses u over [0, 2c] on a wide piece); the hull of the
        // active control points keeps it, and shrinks less as the piece does: the box is the
        // two's intersection, both holding the true points
        let meet = |a: Ivl, b: Ivl| {
            let (lo, hi) = (a.lo.max(b.lo), a.hi.min(b.hi));
            if lo <= hi {
                Ivl::new(lo, hi)
            } else {
                a
            }
        };
        // and the hull of the pcurve cut to `t` (de Casteljau on each span it meets): it
        // shrinks with the piece, where the active control points' hull is the span's
        let hull = match (self.pcurve_hull(t), self.sub_span_box(t)) {
            (Some((hu, hv)), Some((su, sv))) => Some((meet(hu, su), meet(hv, sv))),
            (h, None) | (None, h) => h,
        };
        let (u, v) = match (w.is_positive(), hull) {
            (true, Some((hu, hv))) => (meet(x / w, hu), meet(y / w, hv)),
            (true, None) => (x / w, y / w),
            (false, Some(h)) => h,
            (false, None) => return Ok(None),
        };
        if self.plane || !self.clamps {
            // a plane's net continued is the same plane, whatever the face's evaluator does past
            // it; an evaluator that does not clamp continues the net, as `n_w` does
            return Ok(Some(UvBox {
                u,
                v,
                w,
                straddles: false,
            }));
        }
        // the face's evaluator clamps a point past the net's domain to it (SMLib's practice),
        // so the certificate is about the clamped surface
        let n = &self.net;
        let (cu, su) = clamp(u, domain(&n.u, n.p));
        let (cv, sv) = clamp(v, domain(&n.v, n.q));
        let moved = (cu.lo, cu.hi, cv.lo, cv.hi) != (u.lo, u.hi, v.lo, v.hi);
        if moved && t.lo == t.hi {
            // at a point: the clamped face point within the edge's tolerance of the net's at
            // the pcurve's parameters (the net continued past its knots), or the pcurve is not
            // on its face there (a periodic face's pcurve a period off)
            let off = norm(sub3(self.point(u, v)?, self.point(cu, cv)?)).hi;
            let within = off <= self.tolerance;
            if !within {
                return Err(unsupported("a pcurve leaving its face's net"));
            }
        }
        Ok(Some(UvBox {
            u: cu,
            v: cv,
            w,
            straddles: su || sv,
        }))
    }

    /// The net's point over the box `u x v` (the hull over the knot cells it meets, each cell's
    /// polynomial continued over the box), enclosed.
    fn point(&self, u: Ivl, v: Ivl) -> Result<I3> {
        let n = &self.net;
        let refuse = || invalid("a net's degenerate knots");
        let mut out: Option<I3> = None;
        for ku in spans_meeting(&n.u, n.p, n.nu, u.lo, u.hi) {
            for kv in spans_meeting(&n.v, n.q, n.nv, v.lo, v.hi) {
                let (bu, _) = basis(&n.u, ku, n.p, u).ok_or_else(refuse)?;
                let (bv, _) = basis(&n.v, kv, n.q, v).ok_or_else(refuse)?;
                let mut h = [Ivl::from(0.0); 4];
                for (a, fu) in bu.iter().enumerate() {
                    for (c, fv) in bv.iter().enumerate() {
                        let i = (ku + a).checked_sub(n.p).ok_or_else(refuse)?;
                        let j = (kv + c).checked_sub(n.q).ok_or_else(refuse)?;
                        let at = 4 * (i * n.nv + j);
                        let cp =
                            n.pw.get(at..at + 4)
                                .ok_or_else(|| invalid("a net's control point"))?;
                        for (hl, x) in h.iter_mut().zip(cp) {
                            *hl = *hl + *fu * *fv * Ivl::from(*x);
                        }
                    }
                }
                let [x, y, z, w] = h;
                let p = [x / w, y / w, z / w];
                out = Some(match out {
                    None => p,
                    Some(o) => [o[0].hull(p[0]), o[1].hull(p[1]), o[2].hull(p[2])],
                });
            }
        }
        out.ok_or_else(|| invalid("a net without a cell"))
    }

    /// The net's `N_w` over the box `u x v` (the hull over the knot cells it meets), enclosed.
    ///
    /// Each cell is evaluated on its control points moved by `-c w` (`c` its first control
    /// point's place): `N_w` is the same for the net moved by any constant `c`, since
    /// `(A - c w)_u w - (A - c w) w_u = A_u w - A w_u`, while the enclosure of a sum of basis
    /// intervals times points grows with the points' size, which a net far from the origin
    /// makes large against the cell's own extent. The moved points are enclosed in interval
    /// arithmetic, so the identity holds for the points as given.
    fn n_w_direct(&self, u: Ivl, v: Ivl) -> Result<I3> {
        let n = &self.net;
        let refuse = || invalid("a net's degenerate knots");
        let mut out: Option<I3> = None;
        for ku in spans_meeting(&n.u, n.p, n.nu, u.lo, u.hi) {
            for kv in spans_meeting(&n.v, n.q, n.nv, v.lo, v.hi) {
                let (bu, du) = basis(&n.u, ku, n.p, u).ok_or_else(refuse)?;
                let (bv, dv) = basis(&n.v, kv, n.q, v).ok_or_else(refuse)?;
                // the cell's centre for the move: its first control point, projected
                let (i0, j0) = (
                    ku.checked_sub(n.p).ok_or_else(refuse)?,
                    kv.checked_sub(n.q).ok_or_else(refuse)?,
                );
                let first =
                    n.pw.get(4 * (i0 * n.nv + j0)..4 * (i0 * n.nv + j0) + 4)
                        .ok_or_else(|| invalid("a net's control point"))?;
                let centre = match *first {
                    [x, y, z, w] if w > 0.0 => [x / w, y / w, z / w],
                    _ => [0.0; 3],
                };
                let zero = Ivl::from(0.0);
                let (mut h, mut hu, mut hv) = ([zero; 4], [zero; 4], [zero; 4]);
                for (a, (fu, fdu)) in bu.iter().zip(&du).enumerate() {
                    for (c, (fv, fdv)) in bv.iter().zip(&dv).enumerate() {
                        let i = (ku + a).checked_sub(n.p).ok_or_else(refuse)?;
                        let j = (kv + c).checked_sub(n.q).ok_or_else(refuse)?;
                        let at = 4 * (i * n.nv + j);
                        let cp =
                            n.pw.get(at..at + 4)
                                .ok_or_else(|| invalid("a net's control point"))?;
                        let (f, f_u, f_v) = (*fu * *fv, *fdu * *fv, *fu * *fdv);
                        let wt = Ivl::from(cp.get(3).copied().unwrap_or(1.0));
                        for (l, x) in cp.iter().enumerate() {
                            // x - c w for the three coordinates, w itself kept
                            let x = match centre.get(l) {
                                Some(&c) => Ivl::from(*x) - Ivl::from(c) * wt,
                                None => Ivl::from(*x),
                            };
                            if let (Some(hl), Some(hul), Some(hvl)) =
                                (h.get_mut(l), hu.get_mut(l), hv.get_mut(l))
                            {
                                *hl = *hl + f * x;
                                *hul = *hul + f_u * x;
                                *hvl = *hvl + f_v * x;
                            }
                        }
                    }
                }
                let [ax, ay, az, w] = h;
                let [aux, auy, auz, wu] = hu;
                let [avx, avy, avz, wv] = hv;
                let n_w = if self.rational {
                    let tu = [aux * w - ax * wu, auy * w - ay * wu, auz * w - az * wu];
                    let tv = [avx * w - ax * wv, avy * w - ay * wv, avz * w - az * wv];
                    cross(tu, tv)
                } else {
                    // the weights one constant c: A_u x A_v is c^2 S_u x S_v, a positive
                    // multiple of N_w, and of degree 2p + 2q - 2
                    cross([aux, auy, auz], [avx, avy, avz])
                };
                out = Some(match out {
                    None => n_w,
                    Some(o) => [o[0].hull(n_w[0]), o[1].hull(n_w[1]), o[2].hull(n_w[2])],
                });
            }
        }
        out.ok_or_else(|| invalid("a net without a cell"))
    }

    /// The net's `N_w` over the box `u x v`, enclosed: [`Use::n_w_direct`], and, over a box of
    /// one knot cell, also the centred form (the mean value theorem on the cell's polynomial),
    /// `N_w(m) + N_w,u(box) (u - m_u) + N_w,v(box) (v - m_v)`, the two intersected. The direct
    /// enclosure's width grows with the box as fast as the interval basis' does (each basis
    /// interval times a point, the dependency between them lost); the centred form's grows as
    /// the partials' true size times the box, its own looseness only multiplied by the box's
    /// half widths. With `T_u = A_u w - A w_u` and `T_v = A_v w - A w_v`, `N_w = T_u x T_v`,
    /// `N_w,u = T_u,u x T_v + T_u x T_v,u` and `N_w,v` alike, where
    /// `T_u,u = A_uu w - A w_uu`, `T_u,v = A_uv w + A_u w_v - A_v w_u - A w_uv`,
    /// `T_v,u = A_uv w + A_v w_u - A_u w_v - A w_uv` and `T_v,v = A_vv w - A w_vv`; for
    /// constant weights `N_w = A_u x A_v`, `N_w,u = A_uu x A_v + A_u x A_uv` and `N_w,v` alike.
    fn n_w(&self, u: Ivl, v: Ivl) -> Result<I3> {
        let direct = self.n_w_direct(u, v)?;
        let n = &self.net;
        let (us, vs) = (
            spans_meeting(&n.u, n.p, n.nu, u.lo, u.hi),
            spans_meeting(&n.v, n.q, n.nv, v.lo, v.hi),
        );
        let point = u.lo == u.hi && v.lo == v.hi;
        let ([ku], [kv]) = (us.as_slice(), vs.as_slice()) else {
            return Ok(direct);
        };
        if point {
            return Ok(direct);
        }
        let (mu, mv) = (u.mid(), v.mid());
        let at_m = self.n_w_direct(Ivl::from(mu), Ivl::from(mv))?;
        let [a, au, av, auu, auv, avv] = self.second(u, v, Some((*ku, *kv)))?;
        let xyz = |h: [Ivl; 4]| [h[0], h[1], h[2]];
        let (du, dv) = if self.rational {
            let (w, wu, wv, wuu, wuv, wvv) = (a[3], au[3], av[3], auu[3], auv[3], avv[3]);
            let lin = |p: [Ivl; 4], s: Ivl, q: [Ivl; 4], t: Ivl| {
                let (p, q) = (xyz(p), xyz(q));
                [
                    p[0] * s - q[0] * t,
                    p[1] * s - q[1] * t,
                    p[2] * s - q[2] * t,
                ]
            };
            let add = |x: I3, y: I3| [x[0] + y[0], x[1] + y[1], x[2] + y[2]];
            let tu = lin(au, w, a, wu);
            let tv = lin(av, w, a, wv);
            let tuu = lin(auu, w, a, wuu);
            let tvv = lin(avv, w, a, wvv);
            let tuv = add(lin(auv, w, a, wuv), lin(au, wv, av, wu));
            let tvu = add(lin(auv, w, a, wuv), lin(av, wu, au, wv));
            (
                add(cross(tuu, tv), cross(tu, tvu)),
                add(cross(tuv, tv), cross(tu, tvv)),
            )
        } else {
            let add = |x: I3, y: I3| [x[0] + y[0], x[1] + y[1], x[2] + y[2]];
            (
                add(cross(xyz(auu), xyz(av)), cross(xyz(au), xyz(auv))),
                add(cross(xyz(auv), xyz(av)), cross(xyz(au), xyz(avv))),
            )
        };
        let (hu, hv) = (u - Ivl::from(mu), v - Ivl::from(mv));
        let centred = [
            at_m[0] + du[0] * hu + dv[0] * hv,
            at_m[1] + du[1] * hu + dv[1] * hv,
            at_m[2] + du[2] * hu + dv[2] * hv,
        ];
        let meet = |a: Ivl, b: Ivl| {
            let (lo, hi) = (a.lo.max(b.lo), a.hi.min(b.hi));
            if lo <= hi {
                Ivl::new(lo, hi)
            } else {
                a
            }
        };
        Ok([
            meet(direct[0], centred[0]),
            meet(direct[1], centred[1]),
            meet(direct[2], centred[2]),
        ])
    }

    /// `W^m N_w` at `t` (unsigned), enclosed; `None` where the pcurve's weight is not proved
    /// positive over `t` ([`Use::uv_box`]).
    fn raw_box(&self, t: Ivl) -> Result<Option<I3>> {
        let Some(UvBox { u, v, w, .. }) = self.uv_box(t)? else {
            return Ok(None);
        };
        let n_w = self.n_w(u, v)?;
        let mut wm = Ivl::from(1.0);
        for _ in 0..self.uv_degree {
            wm = wm * w;
        }
        Ok(Some(scale(n_w, wm)))
    }

    /// `W^m N_w` at `t` (unsigned), enclosed.
    fn raw(&self, t: Ivl) -> Result<I3> {
        self.raw_box(t)?
            .ok_or_else(|| invalid("a pcurve's weight not proved positive"))
    }

    /// `F = s W^m N_w` over `t`, enclosed; `None` as [`Use::raw_box`].
    fn f_box(&self, t: Ivl) -> Result<Option<I3>> {
        let s = self
            .sign
            .ok_or_else(|| invalid("a use whose sense is not read"))?;
        Ok(self.raw_box(t)?.map(|r| scale(r, Ivl::from(s))))
    }

    /// `F = s W^m N_w` at `t`, enclosed.
    fn f(&self, t: Ivl) -> Result<I3> {
        self.f_box(t)?
            .ok_or_else(|| invalid("a pcurve's weight not proved positive"))
    }

    /// Reads the sign `s` from the face's evaluator at `t`, where `N_w` is proved non-zero;
    /// whether it was read.
    fn read_sign(&mut self, k: &Kernel, b: &mut Brep, t: f64) -> Result<bool> {
        let raw = self.raw(Ivl::from(t))?;
        if !norm(raw).is_positive() {
            return Ok(false);
        }
        let mid = Vec3::new(raw[0].mid(), raw[1].mid(), raw[2].mid());
        let options = BinormalOptions::default();
        let at = evaluate_binormal(k, b, self.eu, t, false, &options)?;
        let fu = at.face_normal;
        let len = fu.length() * mid.length();
        let has_normal = len > 0.0;
        if !has_normal {
            // the evaluator has no normal here: read at the next point
            return Ok(false);
        }
        let c = mid.dot(fu) / len;
        let same_surface = c.abs() >= SAME_SURFACE_COS;
        if !same_surface {
            return Err(unsupported(
                "a face whose evaluator's normal and its NURBS form's disagree",
            ));
        }
        self.sign = Some(c.signum());
        Ok(true)
    }

    /// The angle within which the face's own normal lies of its net's over the piece
    /// `[s0, s1]` (module docs): 0 for a face that is not a blend; [`Margin::Loose`] where its
    /// enclosures are too wide to prove it (a smaller piece may), [`Margin::NotEnclosed`] where
    /// the definition is not enclosed over the piece at all (no smaller piece helps).
    fn margin(&self, k: &Kernel, s0: f64, s1: f64) -> Result<Margin> {
        let Some(def) = &self.definition else {
            return Ok(Margin::Bounded(0.0));
        };
        let Some(UvBox { u, v, .. }) = self.uv_box(Ivl::new(s0, s1))? else {
            // not bounded over the whole piece: a smaller piece may be
            return Ok(Margin::Loose);
        };
        let n = &self.net;
        let ku = span(&n.u, n.p, n.nu, u.mid());
        let kv = span(&n.v, n.q, n.nv, v.mid());
        let not_enclosed = |e: SolidError| {
            if e.is_unported_or_cancelled() {
                Err(e)
            } else {
                Ok(Margin::NotEnclosed)
            }
        };
        let mut d = match gn_def_face_of(k, def) {
            Ok(d) => d,
            Err(e) => return not_enclosed(e),
        };
        let parts =
            match definition_partials_over_box(k, &mut d, n, ku, kv, u.lo, u.hi, v.lo, v.hi, 1) {
                Ok(p) => p,
                Err(e) => return not_enclosed(e),
            };
        let (su, sv) = (parts.at(1, 0), parts.at(0, 1));
        let def_n = cross([su.x, su.y, su.z], [sv.x, sv.y, sv.z]);
        let net_n = self.n_w(u, v)?;
        Ok(angle_between(def_n, net_n).map_or(Margin::Loose, Margin::Bounded))
    }

    /// Where in `(s0, s1)` the pcurve meets a knot line of the net its box over the part
    /// crosses: the first such line's crossing, found by bisection on the pcurve's point (the
    /// middle of its enclosure) between the part's ends, where they lie on its two sides;
    /// `None` where no line is crossed between the ends. A guess only: the parts it cuts are
    /// each proved one cell (or not) as any part is.
    fn crossing(&self, s0: f64, s1: f64) -> Result<Option<f64>> {
        let Some(UvBox { u, v, .. }) = self.uv_box(Ivl::new(s0, s1))? else {
            return Ok(None);
        };
        let at = |t: f64| -> Result<Option<(f64, f64)>> {
            Ok(self
                .uv_box(Ivl::from(t))?
                .map(|UvBox { u, v, .. }| (u.mid(), v.mid())))
        };
        let (Some(start), Some(end)) = (at(s0)?, at(s1)?) else {
            return Ok(None);
        };
        let [lu, lv] = &self.lines;
        for (lines, along, first_coord) in [(lu, u, true), (lv, v, false)] {
            let coord = |p: (f64, f64)| if first_coord { p.0 } else { p.1 };
            // the lines ascend: the first strictly inside the box, by bisection
            let first = lines.partition_point(|&x| x <= along.lo);
            if let Some(&l) = lines.get(first).filter(|&&l| l < along.hi) {
                let side = |p: (f64, f64)| coord(p) < l;
                if side(start) == side(end) {
                    // the line within the box but not between the ends' points: in the box's
                    // rounding at an end (a line through the pcurve's knot), taken at the
                    // nearer end
                    let (ds, de) = ((coord(start) - l).abs(), (coord(end) - l).abs());
                    return Ok(Some(if ds <= de { s0 } else { s1 }));
                }
                let (mut lo, mut hi) = (s0, s1);
                for _ in 0..CROSSING_STEPS {
                    let mid = 0.5 * (lo + hi);
                    if !(mid > lo && mid < hi) {
                        break;
                    }
                    let Some(m) = at(mid)? else {
                        return Ok(None);
                    };
                    if side(m) == side(start) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                return Ok(Some(0.5 * (lo + hi)));
            }
        }
        Ok(None)
    }

    /// Whether the pcurve over `t` stays in one knot cell of the net (its `(u, v)` box meets
    /// no interior knot line), so `F` is one polynomial there; a plane's net is one cell.
    fn one_cell(&self, t: Ivl) -> Result<bool> {
        if self.lines.iter().all(Vec::is_empty) {
            return Ok(true);
        }
        let Some(UvBox {
            u, v, straddles, ..
        }) = self.uv_box(t)?
        else {
            return Ok(false);
        };
        let [lu, lv] = &self.lines;
        Ok(!straddles && !meets(lu, u) && !meets(lv, v))
    }
}

/// One Bezier span of a pcurve: its knots, its homogeneous control points and the bound on
/// their rounding.
struct BezSpan {
    a: f64,
    b: f64,
    points: Vec<[f64; 3]>,
    /// The bound on each coordinate's rounding (`X`, `Y`, `W`).
    rounding: [f64; 3],
}

/// The `(u, v)` box of `span` over `[lo, hi]` (within its knots): its control points cut to
/// `[lo, hi]` by two de Casteljau splits in interval arithmetic (at `l1 = (hi - a) / (b - a)`,
/// the left part kept, then at `l0 / l1`, the right part kept), their hull, each `X / W`;
/// `None` where a weight is not proved positive or the cut is degenerate (`l1` holding 0).
fn sub_span_hull(span: &BezSpan, lo: f64, hi: f64) -> Option<(Ivl, Ivl)> {
    let (a, b) = (Ivl::from(span.a), Ivl::from(span.b));
    let unit = |x: Ivl| Ivl::new(x.lo.max(0.0), x.hi.min(1.0));
    let l0 = unit((Ivl::from(lo) - a) / (b - a));
    let l1 = unit((Ivl::from(hi) - a) / (b - a));
    // the points as stored, cut exactly in intervals; their rounding `e` is added after: a
    // cut's points are convex combinations of the span's (weights at least 0, summing to 1,
    // for l in [0, 1]), so they move by at most `e` when the span's points do. Carried through
    // the cut, `e` would grow at every level of `p + l (q - p)`
    let [ex, ey, ew] = span.rounding;
    let mut pts: Vec<[Ivl; 3]> = span.points.iter().map(|p| p.map(Ivl::from)).collect();
    if hi == span.a {
        // the span's start alone: its first control point
        pts.truncate(1);
    } else {
        // keep the part [0, l1]: the left column of the triangle at l1
        pts = split(&pts, l1, true);
        // then [l0 / l1, 1] of that: the right column at l0 / l1
        let positive = l1.lo > 0.0;
        if !positive {
            return None;
        }
        pts = split(&pts, unit(l0 / l1), false);
    }
    let mut out: Option<(Ivl, Ivl)> = None;
    let widen = |x: Ivl, e: f64| {
        Ivl::new(
            crate::blend::fillet_section::interval::round_down(x.lo - e),
            crate::blend::fillet_section::interval::round_up(x.hi + e),
        )
    };
    for [x, y, w] in pts {
        let (x, y, w) = (widen(x, ex), widen(y, ey), widen(w, ew));
        if !w.is_positive() {
            return None;
        }
        let (u, v) = (x / w, y / w);
        out = Some(match out {
            None => (u, v),
            Some((p, q)) => (p.hull(u), q.hull(v)),
        });
    }
    out
}

/// The control points of a Bezier curve's part `[0, l]` (`left`) or `[l, 1]`, by de
/// Casteljau's triangle at `l`, in interval arithmetic.
fn split(pts: &[[Ivl; 3]], l: Ivl, left: bool) -> Vec<[Ivl; 3]> {
    let mut row: Vec<[Ivl; 3]> = pts.to_vec();
    let mut firsts = Vec::new();
    let mut lasts = Vec::new();
    while let (Some(&f), Some(&g)) = (row.first(), row.last()) {
        firsts.push(f);
        lasts.push(g);
        row = row
            .windows(2)
            .filter_map(|w| match w {
                // p + l (q - p): l's width then costs |q - p|, where (1 - l) p + l q would cost
                // |p| + |q| (the points far from the origin, the difference small)
                [p, q] => Some([
                    p[0] + l * (q[0] - p[0]),
                    p[1] + l * (q[1] - p[1]),
                    p[2] + l * (q[2] - p[2]),
                ]),
                _ => None,
            })
            .collect();
    }
    if left {
        firsts
    } else {
        lasts.reverse();
        lasts
    }
}

/// The pcurve's box over a piece or at a point, as [`Use::uv_box`] gives it.
#[derive(Clone, Copy)]
struct UvBox {
    u: Ivl,
    v: Ivl,
    w: Ivl,
    straddles: bool,
}

/// The enclosure `x` clamped to the domain `d` as the face's evaluator clamps a parameter,
/// and whether `x` straddles an end of it (some of it inside, some past).
fn clamp(x: Ivl, d: Option<(f64, f64)>) -> (Ivl, bool) {
    let Some((lo, hi)) = d else {
        return (x, false);
    };
    let at = |y: f64| y.max(lo).min(hi);
    let straddles = (x.lo < lo && x.hi > lo) || (x.lo < hi && x.hi > hi);
    (Ivl::new(at(x.lo), at(x.hi)), straddles)
}

fn sub3(a: I3, b: I3) -> I3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Whether the enclosure `x` meets one of the knot `lines` (closed: a line at its end meets).
fn meets(lines: &[f64], x: Ivl) -> bool {
    lines.iter().any(|&l| l >= x.lo && l <= x.hi)
}

/// The value at `s` of the polynomial with Bernstein coefficients `c` on `[0, 1]` (de
/// Casteljau); `None` for no coefficients.
fn bernstein_value(c: &[f64], s: f64) -> Option<f64> {
    let mut b = c.to_vec();
    while b.len() > 1 {
        b = b
            .windows(2)
            .map(|w| {
                if let [x, y] = w {
                    x * (1.0 - s) + y * s
                } else {
                    0.0
                }
            })
            .collect();
    }
    b.first().copied()
}

/// The Bernstein coefficients of the polynomial `c` on `[0, 1/2]` and `[1/2, 1]`.
fn bernstein_halves(c: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut b = c.to_vec();
    while let (Some(&f), Some(&l)) = (b.first(), b.last()) {
        left.push(f);
        right.push(l);
        b = b
            .windows(2)
            .map(|w| if let [x, y] = w { 0.5 * (x + y) } else { 0.0 })
            .collect();
    }
    right.reverse();
    (left, right)
}

/// The roots in `[lo, hi]` of the polynomial with Bernstein coefficients `c` there, as places
/// in `[0, 1]`: none where the coefficients keep one sign, exactly one where they change sign
/// once (found by bisection to rounding), otherwise those of each half. Out of depth or work,
/// the piece's two ends stand for the roots it holds, and the result is `false`: the roots were
/// not all isolated (a root of even multiplicity, or roots closer than the depth resolves).
/// Crate-visible for `sweep::stations`' rail crossings.
pub(crate) fn bernstein_roots(
    c: &[f64],
    lo: f64,
    hi: f64,
    depth: u32,
    work: &mut usize,
    out: &mut Vec<f64>,
) -> bool {
    let (Some(&first), Some(&last)) = (c.first(), c.last()) else {
        return true;
    };
    if first == 0.0 {
        out.push(lo);
    }
    if last == 0.0 {
        out.push(hi);
    }
    let signs: Vec<bool> = c.iter().filter(|x| **x != 0.0).map(|x| *x > 0.0).collect();
    let changes = signs
        .windows(2)
        .filter(|w| matches!(w, [x, y] if x != y))
        .count();
    if changes == 0 {
        return true;
    }
    if changes == 1 && first != 0.0 && last != 0.0 {
        let (mut a, mut b) = (0.0, 1.0);
        for _ in 0..64 {
            let m = 0.5 * (a + b);
            if !(m > a && m < b) {
                break;
            }
            let Some(value) = bernstein_value(c, m) else {
                break;
            };
            if (value > 0.0) == (first > 0.0) {
                a = m;
            } else {
                b = m;
            }
        }
        out.push(lo + (hi - lo) * 0.5 * (a + b));
        return true;
    }
    if depth == 0 || *work == 0 {
        out.extend([lo, hi]);
        return false;
    }
    *work -= 1;
    let mid = 0.5 * (lo + hi);
    let (l, r) = bernstein_halves(c);
    let left = bernstein_roots(&l, lo, mid, depth - 1, work, out);
    let right = bernstein_roots(&r, mid, hi, depth - 1, work, out);
    left && right
}

/// The two uses' values at one point: `P` and `Q` (module docs), enclosed, and a lower end of
/// the angle between the face use normals where both are proved non-zero.
struct Node {
    p: I3,
    q: Ivl,
    angle_lo: Option<f64>,
}

/// An edge between two face uses, ready to decide.
struct Pair {
    a: Use,
    c: Use,
    /// `D + 1`.
    nodes: usize,
    /// The Lebesgue constant of `D + 1` Chebyshev points, rounded up.
    lebesgue: f64,
    /// The pieces' ends.
    breaks: Vec<f64>,
    /// Points evaluated so far.
    points: usize,
}

/// A piece of the edge being decided: its ends, how often it was halved to find the knot
/// cells it lies in, and how often after that to bound it.
#[derive(Clone, Copy)]
struct Part {
    s0: f64,
    s1: f64,
    depth: u32,
    splits: u32,
    /// Halvings made while the definition was not enclosed over the part.
    unenclosed: u32,
}

/// The largest angle between any vector of the enclosure `d` and any of `b`, where it is
/// proved under a right angle; `None` where it is not. Two bounds of the one angle, the
/// smaller taken: `acos` of the cosine's lower end, and, the angle being acute, `asin` of the
/// sine's upper end, `|d x b|_hi / (|d|_lo |b|_lo)`. The cosine form loses the square root of
/// the enclosures' width near 0 (`1 - cos t ~ t^2 / 2`), the sine form only the width itself,
/// so a margin over a small box is the sine's.
fn angle_between(d: I3, b: I3) -> Option<f64> {
    let (nd, nb) = (norm(d), norm(b));
    let cos = dot(d, b) / (nd * nb);
    let acute = cos.lo > 0.0 && cos.lo <= 1.0;
    if !acute {
        return None;
    }
    // acos and asin within 2 ulps (the platform's): 8 u of the angle covers each
    let by_cos = cos.lo.acos() * (1.0 + 8.0 * UNIT);
    // the sine form only where both lengths are proved positive; its argument is the upper
    // end of an outward-rounded quotient (so rounded up), clamped at 1 (asin 1 = pi / 2, no
    // tighter than the cosine's acute bound, which is then kept)
    let lengths = nd.lo * nb.lo > 0.0;
    if !lengths {
        return Some(by_cos);
    }
    let sin = (norm(cross(d, b)) / (nd * nb)).hi.min(1.0);
    let by_sin = sin.asin() * (1.0 + 8.0 * UNIT);
    Some(by_sin.min(by_cos))
}

/// The bound on the angle `atan2(|P|, Q)` where `Q` is not proved positive, from lower ends
/// `p_lo` of `|P|` and `q_lo <= 0` of `Q` (module docs): `atan2(p_lo, q_lo)`, over `pi / 2`.
/// `None` where `|P|` is not bounded away from 0 (a fold, or not known to be none).
fn obtuse_bound(p_lo: f64, q_lo: f64) -> Option<f64> {
    let bounded = p_lo > 0.0 && p_lo.is_finite() && q_lo <= 0.0 && q_lo.is_finite();
    // atan2 within 2 ulps (the platform's)
    bounded.then(|| p_lo.atan2(q_lo) * (1.0 + 8.0 * UNIT))
}

/// The bound on the angle between the nets' normals from enclosures of `P` and `Q` over a
/// whole piece: `atan(|P|_hi / Q_lo)`, or where `Q` is not bounded away from 0
/// [`obtuse_bound`]; `None` where that gives none.
fn box_bound(p: I3, q: Ivl) -> Option<f64> {
    if !q.is_positive() {
        return obtuse_bound(norm(p).lo, q.lo);
    }
    let ratio = norm(p) / q;
    // atan within 2 ulps (the platform's)
    Some(ratio.hi.atan() * (1.0 + 8.0 * UNIT))
}

/// The pieces between consecutive `breaks` of some length.
fn pieces(breaks: &[f64]) -> Vec<(f64, f64)> {
    breaks
        .windows(2)
        .filter_map(|w| match *w {
            [s0, s1] if s1 > s0 => Some((s0, s1)),
            _ => None,
        })
        .collect()
}

/// The share of a part cut off the end a knot line runs through. Not smaller: a part much
/// wider than its distance from the line can leave a blend's definition unenclosed over it
/// (`a_procedural_round_s_edges_carry_its_margin` stays undecided at 1e-6), and at a sixteenth
/// each cut gains four halvings.
const SLIVER: f64 = 1.0 / 16.0;

/// Where to cut `[s0, s1]` for a knot line's crossing at `at` within `SLIVER` of the part of
/// one of its ends (a line through the pcurve's knot, met only in the box's rounding there):
/// `SLIVER` of the part from that end, leaving the rest clear of the line. `None` for a
/// crossing farther inside, which is halved towards as before (a cut on the line itself would
/// put the line at the parts' ends, where a blend's definition may not be enclosed).
fn crossing_cut(s0: f64, s1: f64, at: f64) -> Option<f64> {
    let d = SLIVER * (s1 - s0);
    let cut = if at - s0 < d {
        s0 + d
    } else if s1 - at < d {
        s1 - d
    } else {
        return None;
    };
    (s0 < cut && cut < s1).then_some(cut)
}

/// An upper bound of the Lebesgue constant of `n` Chebyshev points, `1 + (2/pi) ln n`
/// (module docs), rounded up.
fn lebesgue(n: usize) -> f64 {
    let bound = 1.0 + 2.0 / std::f64::consts::PI * (n as f64).ln();
    // ln within 2 ulps (4 u, the platform's), the quotient, product and sum: gamma_4 more,
    // under the 16 u taken
    bound * (1.0 + 16.0 * UNIT)
}

/// A bound `delta` on how far the rounded Chebyshev points of `[s0, s1]` lie from the exact
/// ones, in the piece's `[-1, 1]` scale (module docs).
fn node_error(s0: f64, s1: f64) -> f64 {
    let (m, h) = (0.5 * (s0 + s1), 0.5 * (s1 - s0));
    // the angle (2j + 1) pi / 2n: pi's own rounding, a product and a quotient (the integers
    // are exact), gamma_4 of pi
    let angle = gamma(4) * std::f64::consts::PI;
    // its cosine, within 2 ulps of a value at most 1 (the platform's `cos`): 4 u
    let cosine = 4.0 * UNIT;
    // m + h c: m and h each one rounding (the halving is exact), the product and the sum,
    // gamma_3 of |m| + h in the piece's scale h
    let mapped = gamma(3) * (m.abs() / h + 1.0);
    // the quotient |m| / h and the two sums above: gamma_4 more
    (angle + cosine + mapped) * (1.0 + gamma(4))
}

/// The bound on the angle all over a piece where `Q` is not bounded away from 0 from below, from
/// its enclosed values at its rounded Chebyshev points (module docs): `atan2(1, -k)` with
/// `Q >= -k |P|` proved all over the piece from `P`'s part along one fixed direction. `None`
/// where that part is not bounded away from 0 (a fold, or `P` turning or shrinking too much over
/// the piece).
fn obtuse_piece_bound(nodes: &[Node], lebesgue: f64, eta: f64) -> Option<f64> {
    let e = nodes
        .get(nodes.len() / 2)?
        .p
        .map(|x| Ivl::from(Ivl::mid(x)));
    let along: Vec<Ivl> = nodes.iter().map(|n| dot(n.p, e)).collect();
    let e_lo = Ivl::from(poly_range(&along, lebesgue, eta)?.lo);
    if !e_lo.is_positive() {
        return None;
    }
    // E is positive at every point: its floor over the piece is
    let s = nodes
        .iter()
        .zip(&along)
        .map(|(n, a)| -n.q.mid() / a.mid())
        .fold(0.0, f64::max);
    let g: Vec<Ivl> = nodes
        .iter()
        .zip(&along)
        .map(|(n, &a)| n.q + Ivl::from(s) * a)
        .collect();
    let under = Ivl::from((-poly_range(&g, lebesgue, eta)?.lo).max(0.0));
    let k = (Ivl::from(s) + under / e_lo) * norm(e);
    obtuse_bound(1.0, -k.hi)
}

/// The bound on the angle between the nets' normals all over a piece from its enclosed values
/// at its rounded Chebyshev points (module docs): `atan(||P|| / Q_floor)`, 0 where every
/// `P(t_j)` is exactly 0 and every `Q(t_j)` positive at exact points, or where `Q` is not
/// bounded away from 0 [`obtuse_piece_bound`]; `None` where that gives none or the points'
/// rounding is too large for the piece (`eta`, the `delta D^2` of the module docs, with
/// `L eta >= 1`).
fn piece_bound(nodes: &[Node], lebesgue: f64, eta: f64) -> Option<f64> {
    let big_m = nodes.iter().map(|n| norm(n.p).hi).fold(0.0, f64::max);
    let q_lo = nodes.iter().map(|n| n.q.lo).fold(f64::INFINITY, f64::min);
    let q_hi = nodes
        .iter()
        .map(|n| n.q.hi)
        .fold(f64::NEG_INFINITY, f64::max);
    let q_abs = nodes.iter().map(|n| n.q.mag()).fold(0.0, f64::max);
    if big_m == 0.0 && q_lo > 0.0 && eta == 0.0 {
        return Some(0.0);
    }
    let lam = Ivl::from(lebesgue);
    let one = Ivl::from(1.0);
    let room = one - lam * Ivl::from(eta);
    if !room.is_positive() {
        return None;
    }
    let p_norm = lam * Ivl::from(big_m) / room;
    let q_norm = lam * Ivl::from(q_abs) / room;
    let (lo, hi) = (Ivl::from(q_lo), Ivl::from(q_hi));
    let half = Ivl::from(0.5);
    let floor = (lo + hi) * half - lam * ((hi - lo) * half + Ivl::from(eta) * q_norm);
    if !floor.is_positive() {
        return obtuse_piece_bound(nodes, lebesgue, eta);
    }
    let ratio = p_norm / floor;
    // atan within 2 ulps (the platform's)
    Some(ratio.hi.atan() * (1.0 + 8.0 * UNIT))
}

/// A bound on the angle all over a piece where `Q` is bounded away from 0, from its enclosed
/// values at its rounded Chebyshev points, that goes to the angle as the piece gets short
/// (module docs): `P` split along one fixed direction `u`, `|E| <= k Q` and `|R| <= k_R Q`
/// proved all over the piece. `None` where `Q` is not bounded away from 0 or `P` is 0 at the
/// middle point (no direction).
fn acute_piece_bound(nodes: &[Node], lebesgue: f64, eta: f64) -> Option<f64> {
    let q: Vec<Ivl> = nodes.iter().map(|n| n.q).collect();
    let q_lo = Ivl::from(poly_range(&q, lebesgue, eta)?.lo);
    if !q_lo.is_positive() {
        return None;
    }
    let middle = nodes.get(nodes.len() / 2)?.p.map(Ivl::mid);
    let length = norm(middle.map(Ivl::from)).mid();
    if !(length > 0.0 && length.is_finite()) {
        return None;
    }
    let u = middle.map(|x| Ivl::from(x / length));
    let along: Vec<Ivl> = nodes.iter().map(|n| dot(n.p, u)).collect();
    // Q is positive at every point: its floor over the piece is
    let s = along
        .iter()
        .zip(&q)
        .map(|(a, q)| a.mid() / q.mid())
        .fold(0.0, f64::max);
    let g: Vec<Ivl> = along
        .iter()
        .zip(&q)
        .map(|(&a, &q)| Ivl::from(s) * q - a)
        .collect();
    let under = |x: Ivl| Ivl::from((-x.lo).max(0.0)) / q_lo;
    let with = Ivl::from(s) + under(poly_range(&g, lebesgue, eta)?);
    let against = under(poly_range(&along, lebesgue, eta)?);
    let k = Ivl::from(with.hi.max(against.hi));
    let rest = nodes
        .iter()
        .zip(&along)
        .map(|(n, &a)| sub3(n.p, scale(u, a)));
    let k_rest = Ivl::from(vec_norm_bound(rest, lebesgue, eta)?) / q_lo;
    let share = Ivl::from((Ivl::from(2.0) - dot(u, u)).hi.max(0.0));
    let tan = (share * square(k) + square(k_rest)).sqrt();
    // atan within 2 ulps (the platform's)
    tan.hi
        .is_finite()
        .then(|| tan.hi.atan() * (1.0 + 8.0 * UNIT))
}

/// The bound the decision takes on the angle all over a piece: [`piece_bound`], or
/// [`acute_piece_bound`] where that is smaller (each is sound on its own).
fn part_bound(nodes: &[Node], lebesgue: f64, eta: f64) -> Option<f64> {
    let bound = piece_bound(nodes, lebesgue, eta)?;
    Some(acute_piece_bound(nodes, lebesgue, eta).map_or(bound, |b| b.min(bound)))
}

/// The Chebyshev points of `[s0, s1]`, `n` of them.
fn chebyshev(s0: f64, s1: f64, n: usize) -> impl Iterator<Item = f64> {
    let (m, h) = (0.5 * (s0 + s1), 0.5 * (s1 - s0));
    (0..n).map(move |j| {
        let angle = (2 * j + 1) as f64 * std::f64::consts::PI / (2 * n) as f64;
        m + h * angle.cos()
    })
}

impl Pair {
    /// The pair of edge `e`, its signs read; `None` for an edge not between two face uses.
    fn of(k: &Kernel, b: &mut Brep, e: EdgeId) -> Result<Option<Self>> {
        if !edge::is_manifold(b, e)? || eu1::edge_is_lamina(b, e)? {
            return Ok(None);
        }
        let eu0 = eu1::primary_edgeuse(b, e)?.ok_or_else(|| invalid("an edge without edge use"))?;
        let Some(eu_r) = eu1::radial(b, eu0)? else {
            return Ok(None);
        };
        let a = Use::new(k, b, eu0)?;
        let c = Use::new(k, b, eu_r)?;
        let degree = a.degree.saturating_add(c.degree);
        if degree > MAX_DEGREE {
            return Err(unsupported(
                "an edge whose faces' normals are of too high a degree along it",
            ));
        }
        let ivl = edge::interval(b, e)?;
        let (t0, t1) = (ivl.min(), ivl.max());
        let mut breaks = vec![t0, t1];
        for u in [&a, &c] {
            breaks.extend(u.nurb.knots().iter().copied().filter(|&t| t > t0 && t < t1));
        }
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        let nodes = degree + 1;
        let mut pair = Self {
            a,
            c,
            nodes,
            lebesgue: lebesgue(nodes),
            breaks,
            points: 0,
        };
        // the signs, at the first points where each use's N_w is proved non-zero
        'signs: for (s0, s1) in pieces(&pair.breaks) {
            for t in chebyshev(s0, s1, nodes) {
                if pair.a.sign.is_none() {
                    pair.a.read_sign(k, b, t)?;
                }
                if pair.c.sign.is_none() {
                    pair.c.read_sign(k, b, t)?;
                }
                if pair.a.sign.is_some() && pair.c.sign.is_some() {
                    break 'signs;
                }
            }
        }
        if pair.a.sign.is_none() || pair.c.sign.is_none() {
            return Err(invalid(
                "an edge whose faces' normals are not proved non-zero anywhere along it",
            ));
        }
        Ok(Some(pair))
    }

    /// The values at `t`.
    fn node(&mut self, t: f64) -> Result<Node> {
        self.points += 1;
        let (fa, fc) = (self.a.f(Ivl::from(t))?, self.c.f(Ivl::from(t))?);
        let p = cross(fa, fc);
        let q = dot(fa, fc);
        let defined = norm(fa).is_positive() && norm(fc).is_positive();
        let angle_lo = defined.then(|| {
            // atan2(y, x) grows with y >= 0 and falls with x: its lower end at (|P|_lo, Q_hi)
            let a = norm(p).lo.atan2(q.hi);
            // atan2 within 2 ulps (the platform's)
            a - 8.0 * UNIT * a.abs()
        });
        Ok(Node { p, q, angle_lo })
    }

    /// The margin over the piece `[s0, s1]`: both uses'.
    fn margin(&self, k: &Kernel, s0: f64, s1: f64) -> Result<Margin> {
        let (a, c) = (self.a.margin(k, s0, s1)?, self.c.margin(k, s0, s1)?);
        let (Margin::Bounded(a), Margin::Bounded(c)) = (a, c) else {
            // not enclosed on either side decides it; else too loose on either
            return Ok(if a == Margin::NotEnclosed || c == Margin::NotEnclosed {
                Margin::NotEnclosed
            } else {
                Margin::Loose
            });
        };
        let m = a + c;
        // the sum: one rounding
        Ok(Margin::Bounded(m * (1.0 + gamma(1))))
    }
}

/// A piece's margin ([`Use::margin`]): bounded, too loose to prove over the piece (a smaller
/// piece may prove it), or not enclosed (the definition's enclosure fails there; no smaller
/// piece helps, and the piece is undecided at once).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Margin {
    Bounded(f64),
    Loose,
    NotEnclosed,
}

impl Margin {
    /// The margin as an angle: pi where it is not bounded.
    fn angle(self) -> f64 {
        match self {
            Margin::Bounded(m) => m,
            Margin::Loose | Margin::NotEnclosed => std::f64::consts::PI,
        }
    }
}

/// The face use normal of `eu` at its edge's parameter `t`, enclosed (`F = s W^m N_w`, module
/// docs: a positive multiple of the unit normal), its sense read there from the face's
/// evaluator; `None` where the normal is not proved non-zero or the evaluator gives none.
///
/// # Errors
///
/// As [`tangent_all_along`] for the use's face and pcurve.
pub(crate) fn face_use_normal(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    t: f64,
) -> Result<Option<[Ivl; 3]>> {
    let mut u = Use::new(k, b, eu)?;
    if !u.read_sign(k, b, t)? {
        return Ok(None);
    }
    Ok(Some(u.f(Ivl::from(t))?))
}

/// The answer of [`tangent_all_along`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tangency {
    /// The faces meet within the angle asked all along the edge.
    Tangent,
    /// They part by more than the angle asked somewhere along it.
    NotTangent,
    /// A piece of the edge the bound cannot place on either side of the angle asked, even
    /// halved (module docs), past its budget of points, or where a blend's definition is not
    /// enclosed: `angle` is the bound there (pi where neither `P` nor `Q` is bounded away from
    /// 0, or past the budget) and `margin` the piece's margin (pi where it is not bounded).
    Undecided {
        /// The bound on the angle between the nets' normals there (radians).
        angle: f64,
        /// The margin there (radians).
        margin: f64,
        /// The middle of the piece left undecided (the edge's parameter).
        at: f64,
    },
}

/// Whether edge `e`'s two face uses meet with normals within `max_angle` (radians) of each
/// other all along it: a smooth edge (a seam a face meets itself across is one). An edge
/// whose faces are tangent at one end only is a crease everywhere else, which is not smooth;
/// nor is a cusp, its faces folded back on each other. `None` for an edge not between two
/// face uses. See the module documentation. At `max_angle` 0 only exactly parallel nets are
/// [`Tangency::Tangent`]: off a plane the enclosure of `P` is never exactly 0.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an angle not finite in `[0, pi)` or an edge whose faces'
/// normals are not proved non-zero anywhere along it; [`SolidError::Unsupported`] for a face
/// with no NURBS form or in power-basis form, an edge use with no pcurve in NURBS form, a
/// pcurve past its net's domain, or a degree past `MAX_DEGREE`; [`SolidError::Cancelled`]
/// when the kernel's cancel token is cancelled (polled every `POLL_POINTS` points); those of the
/// evaluations.
pub fn tangent_all_along(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    max_angle: f64,
) -> Result<Option<Tangency>> {
    decide(k, b, e, max_angle, &mut Counts::default())
}

/// What [`decide`] counts as it goes (for tests that pin where an edge stays undecided).
#[derive(Debug, Default)]
struct Counts {
    /// Parts left undecided because a blend's definition is not enclosed over them, past
    /// `MAX_UNENCLOSED_SPLITS` halvings.
    not_enclosed: usize,
    /// Points evaluated on the pieces between the pcurves' knots that hold no witness.
    points: usize,
}

/// Counts one point against [`POLL_POINTS`], polling the kernel's cancel token (bounded work
/// polls it: definitions-and-certificates.md) when the stride is full.
fn poll(k: &Kernel, since: &mut usize) -> Result<()> {
    if *since >= POLL_POINTS {
        *since = 0;
        k.check_cancelled(OP)?;
    }
    *since += 1;
    Ok(())
}

/// [`tangent_all_along`], counting into `counts`.
fn decide(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    max_angle: f64,
    counts: &mut Counts,
) -> Result<Option<Tangency>> {
    if !(0.0..std::f64::consts::PI).contains(&max_angle) {
        return Err(invalid("a tangency angle in [0, pi)"));
    }
    let Some(mut pair) = Pair::of(k, b, e)? else {
        return Ok(None);
    };
    let mut undecided: Option<Tangency> = None;
    // points since the cancel token was last polled; full, so the first point polls
    let mut since_poll = POLL_POINTS;
    let mut note = |angle: Option<f64>, margin: f64, at: f64| {
        if undecided.is_none() {
            undecided = Some(Tangency::Undecided {
                angle: angle.unwrap_or(std::f64::consts::PI),
                margin,
                at,
            });
        }
    };
    for (r0, r1) in pieces(&pair.breaks) {
        // each piece between the pcurves' knots on its own budget of points; past it, the
        // piece is undecided, not an error
        pair.points = 0;
        let mut stack: Vec<Part> = vec![Part {
            unenclosed: 0,
            s0: r0,
            s1: r1,
            depth: 0,
            splits: 0,
        }];
        while let Some(part) = stack.pop() {
            if pair.points > MAX_POINTS {
                note(None, 0.0, 0.5 * (r0 + r1));
                break;
            }
            let Part { s0, s1, .. } = part;
            let kind = pair.margin(k, s0, s1)?;
            let margin = kind.angle();
            let t = Ivl::new(s0, s1);
            let one_cell = pair.a.one_cell(t)? && pair.c.one_cell(t)?;
            let decided = if one_cell {
                let mut nodes = Vec::new();
                for t in chebyshev(s0, s1, pair.nodes) {
                    poll(k, &mut since_poll)?;
                    nodes.push(pair.node(t)?);
                }
                // a witness
                if nodes
                    .iter()
                    .filter_map(|n| n.angle_lo)
                    .any(|angle| angle - margin > max_angle)
                {
                    return Ok(Some(Tangency::NotTangent));
                }
                let degree = (pair.nodes - 1) as f64;
                // D^2 exact (D <= MAX_DEGREE), the product one rounding: gamma_2 covers it
                let eta = node_error(s0, s1) * degree * degree * (1.0 + gamma(2));
                part_bound(&nodes, pair.lebesgue, eta)
            } else {
                // the piece meets a knot line: F is not one polynomial on it. Its enclosure
                // over the whole piece (the hull over the cells it meets) bounds it directly
                poll(k, &mut since_poll)?;
                let mid = pair.node(0.5 * (s0 + s1))?;
                if mid.angle_lo.is_some_and(|angle| angle - margin > max_angle) {
                    return Ok(Some(Tangency::NotTangent));
                }
                pair.points += 1;
                match (pair.a.f_box(t)?, pair.c.f_box(t)?) {
                    (Some(fa), Some(fc)) => box_bound(cross(fa, fc), dot(fa, fc)),
                    _ => None,
                }
            };
            match decided {
                Some(angle) if angle + margin <= max_angle => {}
                // the definition is not enclosed over the part (its enclosures fail): halved
                // within its own budget, as an enclosure over a box can fail and hold on its
                // halves; past it, undecided (exit: never Tangent)
                bound if kind == Margin::NotEnclosed => {
                    let mid = 0.5 * (s0 + s1);
                    if part.unenclosed < MAX_UNENCLOSED_SPLITS && mid > s0 && mid < s1 {
                        let unenclosed = part.unenclosed + 1;
                        let (depth, splits) = (part.depth, part.splits);
                        stack.push(Part {
                            s0: mid,
                            s1,
                            depth,
                            splits,
                            unenclosed,
                        });
                        stack.push(Part {
                            s0,
                            s1: mid,
                            depth,
                            splits,
                            unenclosed,
                        });
                    } else {
                        counts.not_enclosed += 1;
                        note(bound, margin, 0.5 * (s0 + s1));
                    }
                }
                bound => {
                    let mid = 0.5 * (s0 + s1);
                    let can_split = if one_cell {
                        part.splits < MAX_SPLITS
                    } else {
                        part.depth < MAX_CELL_DEPTH
                    };
                    // a part whose box meets a knot line only in its rounding at an end (a line
                    // through the pcurve's knot): a sixteenth cut off that end, so the rest is one
                    // cell at once and the end part shrinks four halvings' worth per cut; any
                    // other part halved as before
                    let cut = if one_cell || !can_split {
                        None
                    } else {
                        match pair.a.crossing(s0, s1)? {
                            Some(at) => Some(at),
                            None => pair.c.crossing(s0, s1)?,
                        }
                        .and_then(|at| crossing_cut(s0, s1, at))
                    };
                    if let Some(cut) = cut {
                        let (depth, splits, unenclosed) =
                            (part.depth + 1, part.splits, part.unenclosed);
                        for (lo, hi) in [(cut, s1), (s0, cut)] {
                            stack.push(Part {
                                s0: lo,
                                s1: hi,
                                depth,
                                splits,
                                unenclosed,
                            });
                        }
                    } else if can_split && mid > s0 && mid < s1 {
                        let (depth, splits) = if one_cell {
                            (part.depth, part.splits + 1)
                        } else {
                            (part.depth + 1, part.splits)
                        };
                        let unenclosed = part.unenclosed;
                        stack.push(Part {
                            s0: mid,
                            s1,
                            depth,
                            splits,
                            unenclosed,
                        });
                        stack.push(Part {
                            s0,
                            s1: mid,
                            depth,
                            splits,
                            unenclosed,
                        });
                    } else {
                        note(bound, margin, mid);
                    }
                }
            }
        }
        counts.points = counts.points.saturating_add(pair.points);
    }
    Ok(Some(undecided.unwrap_or(Tangency::Tangent)))
}

/// The non-zero basis functions `N_{k-p..=k, p}` over `x` in span `k` and their first and
/// second derivatives, enclosed: [`basis`]'s, and the second from the first derivatives of
/// degree `p - 1`, `N'' = p (N'_{i,p-1} / (U_{i+p} - U_i) - N'_{i+1,p-1} / (U_{i+p+1} - U_{i+1}))`
/// (0 for `p < 2`).
fn basis2(knots: &[f64], k: usize, p: usize, x: Ivl) -> Option<[Vec<Ivl>; 3]> {
    let (n, d) = basis(knots, k, p, x)?;
    let mut d2 = vec![Ivl::from(0.0); p + 1];
    if p >= 2 {
        let u = |i: usize| knots.get(i).copied().map(Ivl::from);
        let (_, lower) = basis(knots, k, p - 1, x)?;
        let pf = Ivl::from(f64::from(u32::try_from(p).ok()?));
        for (j, dj) in d2.iter_mut().enumerate() {
            let i = (k + j).checked_sub(p)?;
            let mut v = Ivl::from(0.0);
            if j >= 1 {
                let denom = u(i + p)? - u(i)?;
                if !denom.is_positive() {
                    return None;
                }
                v = v + *lower.get(j - 1)? / denom;
            }
            if j < p {
                let denom = u(i + p + 1)? - u(i + 1)?;
                if !denom.is_positive() {
                    return None;
                }
                v = v - *lower.get(j)? / denom;
            }
            *dj = pf * v;
        }
    }
    Some([n, d, d2])
}

/// The degrees along a piece of one use's `F` and of its curvature's `P^` and `Q^` (module
/// docs, G2), for a net of degrees `(p, q)` (`rational`: weights not all one value), a pcurve
/// of degree `r`, and a plane (0, 0).
fn g2_degrees(p: usize, q: usize, r: usize, rational: bool, plane: bool) -> (usize, usize) {
    if plane {
        return (0, 0);
    }
    let pq = p + q;
    // the degree d of S*_u in (u, v)
    let d = if rational {
        (2 * pq).saturating_sub(1)
    } else {
        pq.saturating_sub(1)
    };
    let f = (2 * d).saturating_mul(r);
    // 6 d r + 4 r - 4
    let bend = (6 * d)
        .saturating_mul(r)
        .saturating_add(4 * r)
        .saturating_sub(4);
    (f, bend)
}

/// One use's values for the curvature across at a point or over a box (module docs, G2): `F`,
/// the face use normal times a positive factor, and `P^` and `Q^`, with `V~ = P^ / Q^`.
#[derive(Clone, Copy)]
struct Bend {
    f: I3,
    p: I3,
    q: Ivl,
}

/// The net's homogeneous point `A` and its partials `A_u, A_v, A_uu, A_uv, A_vv` over the
/// box `u x v` (each the hull over the knot cells it meets).
type Second = [[Ivl; 4]; 6];

/// `S*_u, S*_v, S*_uu, S*_uv, S*_vv` and `w` (module docs, G2).
struct Starred {
    s: [I3; 5],
    w: Ivl,
}

fn add3(a: I3, b: I3) -> I3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// The curvature across's `N*`, `w V*` and `|N*|^2 |T*|^2` from the starred partials and the
/// pcurve's `U*`, `V'*` (module docs, G2), enclosed.
fn across(st: &Starred, du: Ivl, dv: Ivl) -> (I3, I3, Ivl) {
    let [su, sv, suu, suv, svv] = st.s;
    let (e, f, g) = (dot(su, su), dot(su, sv), dot(sv, sv));
    let n = cross(su, sv);
    let t = add3(scale(su, du), scale(sv, dv));
    let (n2, t2) = (dot(n, n), dot(t, t));
    let two = Ivl::from(2.0);
    let shape = add3(
        add3(scale(svv, e), scale(suv, Ivl::from(-2.0) * f)),
        scale(suu, g),
    );
    let along = add3(
        add3(scale(suu, du * du), scale(suv, two * du * dv)),
        scale(svv, dv * dv),
    );
    let v = sub3(scale(shape, t2), scale(along, n2));
    (n, scale(v, st.w), n2 * t2)
}

impl Use {
    /// The pcurve's homogeneous `(X, Y, W)` over `t` and their first derivatives, enclosed.
    fn pcurve_ders(&self, t: Ivl) -> Result<([Ivl; 3], [Ivl; 3])> {
        let (r, n) = (self.pc_degree, self.pc_points.len());
        let mut out: Option<([Ivl; 3], [Ivl; 3])> = None;
        for kt in spans_meeting(&self.pc_knots, r, n, t.lo, t.hi) {
            let (nb, db) = basis(&self.pc_knots, kt, r, t)
                .ok_or_else(|| invalid("a pcurve's degenerate knots"))?;
            let zero = Ivl::from(0.0);
            let (mut h, mut hd) = ([zero; 3], [zero; 3]);
            for (i, (bi, di)) in nb.iter().zip(&db).enumerate() {
                let c = (kt + i)
                    .checked_sub(r)
                    .and_then(|j| self.pc_points.get(j))
                    .ok_or_else(|| invalid("a pcurve's control point"))?;
                for ((hx, hdx), cx) in h.iter_mut().zip(hd.iter_mut()).zip(c) {
                    *hx = *hx + *bi * Ivl::from(*cx);
                    *hdx = *hdx + *di * Ivl::from(*cx);
                }
            }
            let hull =
                |a: [Ivl; 3], b: [Ivl; 3]| [a[0].hull(b[0]), a[1].hull(b[1]), a[2].hull(b[2])];
            out = Some(match out {
                None => (h, hd),
                Some((o, od)) => (hull(o, h), hull(od, hd)),
            });
        }
        out.ok_or_else(|| invalid("a pcurve without a span"))
    }

    /// The net's `A` and its partials to second order over the box `u x v` ([`Second`]): the
    /// hull over the knot cells it meets, or, given `cell`, that cell's polynomial continued
    /// over the whole box.
    fn second(&self, u: Ivl, v: Ivl, cell: Option<(usize, usize)>) -> Result<Second> {
        let n = &self.net;
        let refuse = || invalid("a net's degenerate knots");
        let mut out: Option<Second> = None;
        let (us, vs) = match cell {
            Some((ku, kv)) => (vec![ku], vec![kv]),
            None => (
                spans_meeting(&n.u, n.p, n.nu, u.lo, u.hi),
                spans_meeting(&n.v, n.q, n.nv, v.lo, v.hi),
            ),
        };
        for &ku in &us {
            for &kv in &vs {
                let [bu, du, ddu] = basis2(&n.u, ku, n.p, u).ok_or_else(refuse)?;
                let [bv, dv, ddv] = basis2(&n.v, kv, n.q, v).ok_or_else(refuse)?;
                let mut h = [[Ivl::from(0.0); 4]; 6];
                for (a, ((fu, fdu), fddu)) in bu.iter().zip(&du).zip(&ddu).enumerate() {
                    for (c, ((fv, fdv), fddv)) in bv.iter().zip(&dv).zip(&ddv).enumerate() {
                        let i = (ku + a).checked_sub(n.p).ok_or_else(refuse)?;
                        let j = (kv + c).checked_sub(n.q).ok_or_else(refuse)?;
                        let at = 4 * (i * n.nv + j);
                        let cp =
                            n.pw.get(at..at + 4)
                                .ok_or_else(|| invalid("a net's control point"))?;
                        let fs = [
                            *fu * *fv,
                            *fdu * *fv,
                            *fu * *fdv,
                            *fddu * *fv,
                            *fdu * *fdv,
                            *fu * *fddv,
                        ];
                        for (hk, fk) in h.iter_mut().zip(fs) {
                            for (hl, x) in hk.iter_mut().zip(cp) {
                                *hl = *hl + fk * Ivl::from(*x);
                            }
                        }
                    }
                }
                out = Some(match out {
                    None => h,
                    Some(mut o) => {
                        for (ok, hk) in o.iter_mut().zip(h) {
                            for (ol, hl) in ok.iter_mut().zip(hk) {
                                *ol = ol.hull(hl);
                            }
                        }
                        o
                    }
                });
            }
        }
        out.ok_or_else(|| invalid("a net without a cell"))
    }

    /// The starred partials of the net over the box `u x v` (module docs, G2), on `cell` as
    /// [`Use::second`] takes it.
    fn starred(&self, u: Ivl, v: Ivl, cell: Option<(usize, usize)>) -> Result<Starred> {
        let [a, au, av, auu, auv, avv] = self.second(u, v, cell)?;
        let xyz = |h: [Ivl; 4]| [h[0], h[1], h[2]];
        let (w, wu, wv) = (a[3], au[3], av[3]);
        if !self.rational {
            // one weight c: c S_u = A_u, c S_uu = A_uu (module docs, G2)
            return Ok(Starred {
                s: [xyz(au), xyz(av), xyz(auu), xyz(auv), xyz(avv)],
                w,
            });
        }
        let (a3, au3, av3) = (xyz(a), xyz(au), xyz(av));
        let first = |d: I3, wd: Ivl| sub3(scale(d, w), scale(a3, wd));
        let two = Ivl::from(2.0);
        let ww = w * w;
        // w^3 S_xy = w^2 A_xy - w (w_x A_y + w_y A_x + w_xy A) + 2 w_x w_y A
        let second = |dxy: [Ivl; 4], wx: Ivl, wy: Ivl, ax: I3, ay: I3| {
            let wxy = dxy[3];
            let mid = add3(add3(scale(ay, wx), scale(ax, wy)), scale(a3, wxy));
            add3(
                sub3(scale(xyz(dxy), ww), scale(mid, w)),
                scale(a3, two * wx * wy),
            )
        };
        Ok(Starred {
            s: [
                first(au3, wu),
                first(av3, wv),
                second(auu, wu, wu, au3, au3),
                second(auv, wu, wv, au3, av3),
                second(avv, wv, wv, av3, av3),
            ],
            w,
        })
    }

    /// `F`, `P^` and `Q^` over `t` (module docs, G2), enclosed; `None` where the pcurve's
    /// box is not ([`Use::uv_box`]).
    fn bend(&self, t: Ivl) -> Result<Option<Bend>> {
        self.bend_on(t, None)
    }

    /// The knot cells of the net the pcurve's box over `t` meets (none where there is no box).
    fn cells(&self, t: Ivl) -> Result<Vec<(usize, usize)>> {
        let Some(UvBox { u, v, .. }) = self.uv_box(t)? else {
            return Ok(Vec::new());
        };
        let n = &self.net;
        let us = spans_meeting(&n.u, n.p, n.nu, u.lo, u.hi);
        let vs = spans_meeting(&n.v, n.q, n.nv, v.lo, v.hi);
        Ok(us
            .iter()
            .flat_map(|&ku| vs.iter().map(move |&kv| (ku, kv)))
            .collect())
    }

    /// [`Use::bend`] on one knot cell's polynomial continued over the box (`cell`), or on the
    /// hull over the cells it meets (`None`).
    fn bend_on(&self, t: Ivl, cell: Option<(usize, usize)>) -> Result<Option<Bend>> {
        let s = self
            .sign
            .ok_or_else(|| invalid("a use whose sense is not read"))?;
        let Some(UvBox { u, v, w: big_w, .. }) = self.uv_box(t)? else {
            return Ok(None);
        };
        let ([x, y, wc], [xd, yd, wd]) = self.pcurve_ders(t)?;
        let du = xd * wc - x * wd;
        let dv = yd * wc - y * wd;
        let st = self.starred(u, v, cell)?;
        let (n, wv, q) = across(&st, du, dv);
        let pow = |k: usize| {
            let mut out = Ivl::from(1.0);
            for _ in 0..k {
                out = out * big_w;
            }
            out
        };
        let f = scale(n, pow(self.uv_degree) * Ivl::from(s));
        if self.plane {
            // a plane bends no way: V~ = 0
            let zero = Ivl::from(0.0);
            return Ok(Some(Bend {
                f,
                p: [zero; 3],
                q: Ivl::from(1.0),
            }));
        }
        // W^(6 d), d = m / 2 the degree of S*_u
        let wk = pow(3 * self.uv_degree);
        Ok(Some(Bend {
            f,
            p: scale(wv, wk),
            q: q * wk,
        }))
    }

    /// The curvature across of the face's definition less that of its net, enclosed over the
    /// piece `[s0, s1]`'s box (module docs, G2): `Some(0)` for a face that is not a blend;
    /// `None` where it is not enclosed.
    fn bend_margin(&self, k: &Kernel, s0: f64, s1: f64) -> Result<Option<f64>> {
        let Some(def) = &self.definition else {
            return Ok(Some(0.0));
        };
        let t = Ivl::new(s0, s1);
        let Some(UvBox { u, v, .. }) = self.uv_box(t)? else {
            return Ok(None);
        };
        let n = &self.net;
        let ku = span(&n.u, n.p, n.nu, u.mid());
        let kv = span(&n.v, n.q, n.nv, v.mid());
        let keep = |e: SolidError| {
            if e.is_unported_or_cancelled() {
                Err(e)
            } else {
                Ok(None)
            }
        };
        let mut d = match gn_def_face_of(k, def) {
            Ok(d) => d,
            Err(e) => return keep(e),
        };
        let parts =
            match definition_partials_over_box(k, &mut d, n, ku, kv, u.lo, u.hi, v.lo, v.hi, 2) {
                Ok(p) => p,
                Err(e) => return keep(e),
            };
        let v3 = |a: usize, b: usize| {
            let x = parts.at(a, b);
            [x.x, x.y, x.z]
        };
        let def = Starred {
            s: [v3(1, 0), v3(0, 1), v3(2, 0), v3(1, 1), v3(0, 2)],
            w: Ivl::from(1.0),
        };
        let ([x, y, wc], [xd, yd, wd]) = self.pcurve_ders(t)?;
        let (du, dv) = (xd * wc - x * wd, yd * wc - y * wd);
        let natural = |st: &Starred| {
            let (nn, wv, q) = across(st, du, dv);
            dot(nn, wv) / (norm(nn) * q)
        };
        let gap = natural(&def) - natural(&self.starred(u, v, None)?);
        let bounded = gap.lo.is_finite() && gap.hi.is_finite();
        Ok(bounded.then_some(gap.mag()))
    }
}

/// The answer of [`g2_all_along`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum G2 {
    /// The faces' curvatures across the edge agree within the jump asked all along it, and
    /// the faces are tangent within the angle asked ([`tangent_all_along`]).
    Continuous,
    /// The faces are not tangent within the angle asked ([`Tangency::NotTangent`]): a crease,
    /// not G2.
    NotTangent,
    /// They differ by more somewhere: at the edge's parameter `at`, by `jump` (use a's less
    /// its partner's, each against its own face use normal, 1/length), the nets' jump there
    /// lying within `within` of it (its enclosure's half width, rounded up).
    Jumps {
        /// The edge's parameter there.
        at: f64,
        /// The jump there: its enclosure's middle.
        jump: f64,
        /// The enclosure's half width: the nets' jump lies in `[jump - within, jump + within]`.
        within: f64,
    },
    /// A piece the bound places on neither side, even halved, past its budget of points, at
    /// a degenerate point, or where a blend's definition is not enclosed: `at` the piece's
    /// middle and `bound` the bound on the jump there (infinite where there is none). Also an
    /// edge whose curvatures are proved to agree but whose tangency is undecided: `at` where
    /// [`tangent_all_along`] left it, `bound` infinite.
    Undecided {
        /// The edge's parameter at the piece's middle.
        at: f64,
        /// The bound on the jump there.
        bound: f64,
    },
}

/// [`G2::Jumps`] at `at` from the jump's enclosure there.
fn jumps(at: f64, jump: Ivl) -> G2 {
    let mid = jump.mid();
    // the half width's two differences rounded up, one more rounding for the larger
    let within = (jump.hi - mid).max(mid - jump.lo) * (1.0 + gamma(2));
    G2::Jumps {
        at,
        jump: mid,
        within,
    }
}

/// A G2 node: the values at one point (module docs, G2).
struct G2Node {
    /// `J`, `F_a . F_a`, `Q^_a`, `Q^_b`, `F_a . P^_a`, `F_b . F_b`, `F_b . P^_b`.
    j: Ivl,
    faa: Ivl,
    qa: Ivl,
    qb: Ivl,
    ka: Ivl,
    fbb: Ivl,
    kb: Ivl,
    /// `P^_b`.
    pb: I3,
    /// `F_a x F_b` and `F_a . F_b`, for the normals' angle.
    g1: Node,
    /// The jump and the larger curvature's size, each use against its own normal.
    jump: Ivl,
    larger: f64,
}

fn g2_node(a: &Bend, c: &Bend) -> G2Node {
    let j = dot(a.f, sub3(scale(a.p, c.q), scale(c.p, a.q)));
    let (faa, fbb) = (dot(a.f, a.f), dot(c.f, c.f));
    let (ka, kb) = (dot(a.f, a.p), dot(c.f, c.p));
    let kappa_a = ka / (faa.sqrt() * a.q);
    let kappa_b = kb / (fbb.sqrt() * c.q);
    G2Node {
        j,
        faa,
        qa: a.q,
        qb: c.q,
        ka,
        fbb,
        kb,
        pb: c.p,
        g1: Node {
            p: cross(a.f, c.f),
            q: dot(a.f, c.f),
            angle_lo: None,
        },
        jump: kappa_a - kappa_b,
        larger: kappa_a.mag().max(kappa_b.mag()),
    }
}

/// The range `[lo, hi]` of a polynomial of degree at most `D` over a piece from its values
/// enclosed at the piece's rounded Chebyshev points (module docs: the floor of `Q`, and its
/// ceiling the same way); `None` where `L eta >= 1`.
fn poly_range(values: &[Ivl], lebesgue: f64, eta: f64) -> Option<Ivl> {
    let lo = values.iter().map(|x| x.lo).fold(f64::INFINITY, f64::min);
    let hi = values
        .iter()
        .map(|x| x.hi)
        .fold(f64::NEG_INFINITY, f64::max);
    let abs = values.iter().map(|x| x.mag()).fold(0.0, f64::max);
    if !(lo.is_finite() && hi.is_finite()) {
        return None;
    }
    let lam = Ivl::from(lebesgue);
    let room = Ivl::from(1.0) - lam * Ivl::from(eta);
    if !room.is_positive() {
        return None;
    }
    let size = lam * Ivl::from(abs) / room;
    let (l, h) = (Ivl::from(lo), Ivl::from(hi));
    let half = Ivl::from(0.5);
    let mid = (l + h) * half;
    let reach = lam * ((h - l) * half + Ivl::from(eta) * size);
    Some(Ivl::new((mid - reach).lo, (mid + reach).hi))
}

/// The bound `L M / (1 - L eta)` on `|V(t)|` over a piece, `M` the largest `|V(t_j)|`
/// enclosed (module docs).
fn vec_norm_bound(values: impl Iterator<Item = I3>, lebesgue: f64, eta: f64) -> Option<f64> {
    let big_m = values.map(|v| norm(v).hi).fold(0.0, f64::max);
    let lam = Ivl::from(lebesgue);
    let room = Ivl::from(1.0) - lam * Ivl::from(eta);
    if !room.is_positive() || !big_m.is_finite() {
        return None;
    }
    Some((lam * Ivl::from(big_m) / room).hi)
}

/// What one piece of [`g2_all_along`] proves.
enum G2Piece {
    /// The bound on the jump over it and the scale's lower bound there.
    Bound { jump: f64, scale: f64 },
    /// Nothing bounded.
    Unbounded,
}

/// A piece's bound from its nodes (module docs, G2).
fn g2_bound(nodes: &[G2Node], lebesgue: f64, eta: f64, floor: f64) -> G2Piece {
    let range = |f: &dyn Fn(&G2Node) -> Ivl| {
        let v: Vec<Ivl> = nodes.iter().map(f).collect();
        poly_range(&v, lebesgue, eta)
    };
    let g1: Vec<Node> = nodes
        .iter()
        .map(|n| Node {
            p: n.g1.p,
            q: n.g1.q,
            angle_lo: None,
        })
        .collect();
    let (Some(j), Some(faa), Some(qa), Some(qb), Some(ka), Some(fbb), Some(kb)) = (
        range(&|n| n.j),
        range(&|n| n.faa),
        range(&|n| n.qa),
        range(&|n| n.qb),
        range(&|n| n.ka),
        range(&|n| n.fbb),
        range(&|n| n.kb),
    ) else {
        return G2Piece::Unbounded;
    };
    let (Some(theta), Some(pb)) = (
        piece_bound(&g1, lebesgue, eta),
        vec_norm_bound(nodes.iter().map(|n| n.pb), lebesgue, eta),
    ) else {
        return G2Piece::Unbounded;
    };
    let floors = [faa.lo, qa.lo, qb.lo, fbb.lo];
    if !floors.iter().all(|&x| x > 0.0) {
        return G2Piece::Unbounded;
    }
    let lower = |x: Ivl| Ivl::from(x.lo);
    let jmax = Ivl::from(j.mag());
    let bound = jmax / (lower(faa).sqrt() * lower(qa) * lower(qb))
        + Ivl::from(theta) * Ivl::from(pb) / lower(qb);
    // the larger curvature's lower bound: |F . P^| from below over |F| Q^ from above
    let upper = |x: Ivl| Ivl::from(x.hi);
    let kappa_a = Ivl::from(ka.mig()) / (upper(faa).sqrt() * upper(qa));
    let kappa_b = Ivl::from(kb.mig()) / (upper(fbb).sqrt() * upper(qb));
    let scale = floor.max(kappa_a.lo.max(0.0)).max(kappa_b.lo.max(0.0));
    G2Piece::Bound {
        jump: bound.hi,
        scale,
    }
}

/// A piece's bound across a knot line, directly over its box `t` (module docs, G2), adding
/// the points it evaluates to `pair`'s; unbounded where they would pass `max_points`.
fn g2_cells_bound(pair: &mut Pair, t: Ivl, floor: f64, max_points: usize) -> Result<G2Piece> {
    // each cell's polynomial continued over the box, every pair of the two uses'
    // cells: the point at each t lies in some cell of each, whose pair holds its
    // values (a hull of the cells' partials, mixed, would not shrink with the box
    // where the net is only C0 across a knot line, as a rational circle's is)
    let (ca, cc) = (pair.a.cells(t)?, pair.c.cells(t)?);
    let mut worst: Option<(f64, f64)> = None;
    let mut unbounded = ca.is_empty() || cc.is_empty();
    'cells: for &x in &ca {
        for &y in &cc {
            if pair.points >= max_points {
                unbounded = true;
                break 'cells;
            }
            pair.points += 1;
            let (Some(a), Some(c)) = (pair.a.bend_on(t, Some(x))?, pair.c.bend_on(t, Some(y))?)
            else {
                unbounded = true;
                break 'cells;
            };
            let n = g2_node(&a, &c);
            let kappa = |kx: Ivl, fxx: Ivl, qx: Ivl| {
                (Ivl::from(kx.mig()) / (Ivl::from(fxx.hi).sqrt() * Ivl::from(qx.hi))).lo
            };
            let scale = floor
                .max(kappa(n.ka, n.faa, n.qa).max(0.0))
                .max(kappa(n.kb, n.fbb, n.qb).max(0.0));
            let jump = n.jump.mag();
            if !jump.is_finite() {
                unbounded = true;
                break 'cells;
            }
            worst = Some(match worst {
                None => (jump, scale),
                Some((j, s)) => (j.max(jump), s.min(scale)),
            });
        }
    }
    Ok(match worst {
        Some((jump, scale)) if !unbounded => G2Piece::Bound { jump, scale },
        _ => G2Piece::Unbounded,
    })
}

/// Whether edge `e`'s two face uses bend alike across it all along it (G2): their normal
/// curvatures across the edge within `max_jump` of the larger, or of `1 / L` where both are
/// flatter (`L` the edge's measured length: a stated scale, not a bound), SolidCraft's
/// `same_bend` rule, and their normals within `max_angle` of each other all along it
/// ([`tangent_all_along`], decided first: a crease is [`G2::NotTangent`] at its cost). `None`
/// for an edge not between two face uses. See the module documentation (G2).
///
/// # Errors
///
/// As [`tangent_all_along`] for `max_angle`; [`SolidError::InvalidInput`] for a `max_jump` not
/// finite and non-negative, an edge whose
/// length is not positive, or whose faces' normals are not proved non-zero anywhere along it;
/// [`SolidError::Unsupported`] as [`tangent_all_along`], or for a curvature of a degree past
/// `MAX_DEGREE`; those of the evaluations.
pub fn g2_all_along(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    max_angle: f64,
    max_jump: f64,
) -> Result<Option<G2>> {
    Ok(g2_within(k, b, e, max_angle, max_jump, MAX_POINTS)?.0)
}

/// [`g2_all_along`] within `max_points` on each piece between the pcurves' knots (each point
/// checked against it before it is evaluated), and the points it evaluated.
fn g2_within(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    max_angle: f64,
    max_jump: f64,
    max_points: usize,
) -> Result<(Option<G2>, usize)> {
    if !(max_jump.is_finite() && max_jump >= 0.0) {
        return Err(invalid("a curvature jump finite and not negative"));
    }
    let length = crate::mass::integrator::properties::edge_length(k, b, e, 1e-6)?;
    if !(length.is_finite() && length > 0.0) {
        return Err(invalid("an edge of some length"));
    }
    let floor = 1.0 / length;
    // G2 is a smooth edge's: first the tangency, at `max_angle` ([`tangent_all_along`]); a
    // crease is not G2 (and its normals' angle would leave every piece's jump unbounded)
    let unproved_at = match tangent_all_along(k, b, e, max_angle)? {
        None => return Ok((None, 0)),
        Some(Tangency::NotTangent) => return Ok((Some(G2::NotTangent), 0)),
        Some(Tangency::Tangent) => None,
        Some(Tangency::Undecided { at, .. }) => Some(at),
    };
    let Some(mut pair) = Pair::of(k, b, e)? else {
        return Ok((None, 0));
    };
    let degrees = |u: &Use| g2_degrees(u.net.p, u.net.q, u.pc_degree, u.rational, u.plane);
    let ((fa, pa), (fc, pc)) = (degrees(&pair.a), degrees(&pair.c));
    // the largest degree of the piece's polynomials: J, F . F and F . P^ of each use, F_a x F_b
    let degree = [
        fa.saturating_add(pa).saturating_add(pc),
        fc.saturating_add(pc),
        fa.saturating_add(pa),
        fa.saturating_mul(2),
        fc.saturating_mul(2),
        fa.saturating_add(fc),
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    if degree > MAX_DEGREE {
        return Err(unsupported(
            "an edge whose faces' curvatures are of too high a degree along it",
        ));
    }
    let nodes_n = degree + 1;
    let lam = lebesgue(nodes_n);
    let mut undecided: Option<G2> = None;
    let mut note = |at: f64, bound: f64| {
        if undecided.is_none() {
            undecided = Some(G2::Undecided { at, bound });
        }
    };
    let mut total = 0_usize;
    for (r0, r1) in pieces(&pair.breaks) {
        total = total.saturating_add(pair.points);
        pair.points = 0;
        let mut stack: Vec<Part> = vec![Part {
            unenclosed: 0,
            s0: r0,
            s1: r1,
            depth: 0,
            splits: 0,
        }];
        while let Some(part) = stack.pop() {
            let Part { s0, s1, .. } = part;
            let mid = 0.5 * (s0 + s1);
            if pair.points >= max_points {
                note(mid, f64::INFINITY);
                break;
            }
            let margin = match (
                pair.a.bend_margin(k, s0, s1)?,
                pair.c.bend_margin(k, s0, s1)?,
            ) {
                (Some(a), Some(c)) => Some((a + c) * (1.0 + gamma(1))),
                _ => None,
            };
            let m = margin.unwrap_or(f64::INFINITY);
            let t = Ivl::new(s0, s1);
            let one_cell = pair.a.one_cell(t)? && pair.c.one_cell(t)?;
            let witness = |n: &G2Node| {
                let scale = floor.max(n.larger);
                n.jump.mig() - m > max_jump * scale * (1.0 + gamma(2))
            };
            let decided = if one_cell {
                let mut nodes = Vec::new();
                let mut missing = false;
                let mut spent = false;
                for t in chebyshev(s0, s1, nodes_n) {
                    if pair.points >= max_points {
                        spent = true;
                        break;
                    }
                    pair.points += 1;
                    let at = Ivl::from(t);
                    match (pair.a.bend(at)?, pair.c.bend(at)?) {
                        (Some(a), Some(c)) => {
                            let n = g2_node(&a, &c);
                            if witness(&n) {
                                let points = total.saturating_add(pair.points);
                                return Ok((Some(jumps(t, n.jump)), points));
                            }
                            nodes.push(n);
                        }
                        _ => missing = true,
                    }
                }
                if spent {
                    note(mid, f64::INFINITY);
                    break;
                }
                if missing {
                    G2Piece::Unbounded
                } else {
                    let d = degree as f64;
                    let eta = node_error(s0, s1) * d * d * (1.0 + gamma(2));
                    g2_bound(&nodes, lam, eta, floor)
                }
            } else {
                // across a knot line: bounded directly over the piece's box
                // a witness at the middle, cell by cell: a point on a knot line lies in the
                // closed cells on both sides, each cell's polynomial the face there from that
                // side (a hull of the two, mixed, proves nothing where the net is only C0)
                let at = Ivl::from(mid);
                let (wa, wc) = (pair.a.cells(at)?, pair.c.cells(at)?);
                let mut spent = false;
                'witness: for &x in &wa {
                    for &y in &wc {
                        if pair.points >= max_points {
                            spent = true;
                            break 'witness;
                        }
                        pair.points += 1;
                        if let (Some(a), Some(c)) =
                            (pair.a.bend_on(at, Some(x))?, pair.c.bend_on(at, Some(y))?)
                        {
                            let n = g2_node(&a, &c);
                            if witness(&n) {
                                let points = total.saturating_add(pair.points);
                                return Ok((Some(jumps(mid, n.jump)), points));
                            }
                        }
                    }
                }
                if spent {
                    note(mid, f64::INFINITY);
                    break;
                }
                g2_cells_bound(&mut pair, t, floor, max_points)?
            };
            let bound = match decided {
                G2Piece::Bound { jump, scale } => {
                    let sum = (jump + m) * (1.0 + gamma(1));
                    if sum <= max_jump * scale * (1.0 - gamma(2)) {
                        continue;
                    }
                    sum
                }
                G2Piece::Unbounded => f64::INFINITY,
            };
            let can_split = if one_cell {
                part.splits < MAX_SPLITS
            } else {
                part.depth < MAX_CELL_DEPTH
            };
            if can_split && mid > s0 && mid < s1 {
                let (depth, splits) = if one_cell {
                    (part.depth, part.splits + 1)
                } else {
                    (part.depth + 1, part.splits)
                };
                let unenclosed = part.unenclosed;
                stack.push(Part {
                    s0: mid,
                    s1,
                    depth,
                    splits,
                    unenclosed,
                });
                stack.push(Part {
                    s0,
                    s1: mid,
                    depth,
                    splits,
                    unenclosed,
                });
            } else {
                note(mid, bound);
            }
        }
    }
    let points = total.saturating_add(pair.points);
    // continuous curvature on an edge whose tangency is undecided is not G2 proved
    let verdict = match (undecided, unproved_at) {
        (Some(u), _) => u,
        (None, Some(at)) => G2::Undecided {
            at,
            bound: f64::INFINITY,
        },
        (None, None) => G2::Continuous,
    };
    Ok((Some(verdict), points))
}

/// The answer of [`tube_regularity`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TubeRegularity {
    /// The tube is proved regular all along the edge.
    Regular,
    /// It folds over itself somewhere along the edge: `largest` is the largest radius at which
    /// the tube is proved regular (module docs), `None` where none of the sizes tried is.
    Folds {
        /// The largest tube radius proved regular, when one is.
        largest: Option<f64>,
    },
    /// It is neither proved regular nor proved to fold: a piece is undecided past the budget,
    /// or the edge has no two face uses, or its curve no NURBS form, to decide it on.
    NotCertified,
}

/// `[lower, upper]` of `max over the arc of u . k` on the piece `s` of the edge, times the
/// sign: `None` where it has no bound (a speed or a normal not proved non-zero, or an
/// evaluation that failed). Module docs.
fn tube_bounds(
    cn: &NurbsCurve,
    pair: &Pair,
    sigma: Ivl,
    s: Ivl,
    margin: f64,
) -> Option<(f64, f64)> {
    let one = Ivl::from(1.0);
    let ders = curve_derivs_over(cn, s, 2, None).ok()?;
    let (d1, d2) = (ders.get(1)?, ders.get(2)?);
    let c1 = [d1.x, d1.y, d1.z];
    let c2 = [d2.x, d2.y, d2.z];
    let speed2 = dot(c1, c1);
    if !speed2.is_positive() {
        return None;
    }
    // k = (c'' - (c'' . c') c' / |c'|^2) / |c'|^2
    let along = dot(c2, c1) / speed2;
    let kv = scale(sub3(c2, scale(c1, along)), one / speed2);
    let fa = pair.a.f(s).ok()?;
    let fc = pair.c.f(s).ok()?;
    let (na, nc) = (norm(fa), norm(fc));
    if !na.is_positive() || !nc.is_positive() {
        return None;
    }
    let ua = scale(fa, sigma / na);
    let uc = scale(fc, sigma / nc);
    let (ea, ec) = (dot(ua, kv), dot(uc, kv));
    let g = dot(ua, uc);
    let det = one - g * g;
    let ends_lo = ea.lo.max(ec.lo);
    let ends_hi = ea.hi.max(ec.hi);
    let kmag = norm(kv);
    // a blend's definition arc lies within `widen / |k|` of its net's (module docs)
    let widen = if margin > 0.0 {
        let half = (one + g) * Ivl::from(0.5);
        if !half.is_positive() {
            return None;
        }
        let w = half.sqrt() - Ivl::from(margin);
        if !w.is_positive() {
            return None;
        }
        (Ivl::from(2.0 * margin) / w * Ivl::from(kmag.hi)).hi
    } else {
        0.0
    };
    let widened = |(lo, hi): (f64, f64)| {
        Some((
            (Ivl::from(lo) - Ivl::from(widen)).lo,
            (Ivl::from(hi) + Ivl::from(widen)).hi,
        ))
    };
    if !det.is_positive() {
        if g.lo > 0.0 {
            // the arc is within rounding of a point: its ends bound it, plus |k| for the rest
            let rest = Ivl::from(kmag.hi) * Ivl::from(det.hi.max(0.0)).sqrt();
            return widened((ends_lo, (Ivl::from(ends_hi.max(0.0)) + rest).hi));
        }
        // the normals near opposite: the arc is near a half circle, and may hold k's direction
        return widened((ends_lo, kmag.hi.max(ends_hi)));
    }
    // k_p = alpha u_a + beta u_c, its direction inside the arc where both are positive
    let alpha = (ea - g * ec) / det;
    let beta = (ec - g * ea) / det;
    if alpha.lo > 0.0 && beta.lo > 0.0 {
        let kp = Ivl::from((alpha * ea + beta * ec).lo.max(0.0)).sqrt();
        widened((kp.lo.max(ends_lo), kmag.hi))
    } else if alpha.hi < 0.0 || beta.hi < 0.0 {
        widened((ends_lo, ends_hi))
    } else {
        widened((ends_lo, kmag.hi))
    }
}

/// What one run of the tube decision found: a fold, and the largest lower bound of
/// `max over the arc of u . k` over the pieces proved to fold (so `1 / least` bounds the
/// largest regular radius from above); or regular; or undecided.
enum TubeRun {
    Regular,
    Folds { least: f64 },
    Undecided,
}

/// The tube decision at radius `d` (module docs). With `regular_only` it asks only whether the
/// tube is proved regular, and stops at the first piece that folds or is left undecided (then
/// [`TubeRun::Undecided`]): the sizing's runs need no more, and a radius past the limit is
/// then not decided over the whole edge.
fn tube_run(
    k: &Kernel,
    cn: &NurbsCurve,
    pair: &Pair,
    sigma: Ivl,
    d: f64,
    regular_only: bool,
) -> Result<TubeRun> {
    let di = Ivl::from(d);
    let (mut folds, mut undecided, mut count) = (false, false, 0usize);
    let mut least = f64::NEG_INFINITY;
    let mut stack: Vec<(f64, f64, u32)> = pieces(&pair.breaks)
        .into_iter()
        .map(|(a, z)| (a, z, 0))
        .collect();
    while let Some((a, z, depth)) = stack.pop() {
        if count % POLL_POINTS == 0 {
            k.check_cancelled(OP)?;
        }
        count += 1;
        // a blend's margin over the piece (module docs): none for other faces
        let margin = if pair.a.definition.is_some() || pair.c.definition.is_some() {
            match pair.margin(k, a, z)? {
                Margin::Bounded(m) => Some(m),
                Margin::Loose => None,
                Margin::NotEnclosed => {
                    if regular_only {
                        return Ok(TubeRun::Undecided);
                    }
                    undecided = true;
                    continue;
                }
            }
        } else {
            Some(0.0)
        };
        let bounds = margin.and_then(|m| tube_bounds(cn, pair, sigma, Ivl::new(a, z), m));
        match bounds {
            Some((lo, _)) if (di * Ivl::from(lo)).lo >= 1.0 => {
                if regular_only {
                    return Ok(TubeRun::Undecided);
                }
                folds = true;
                least = least.max(lo);
            }
            Some((_, hi)) if (di * Ivl::from(hi)).hi < 1.0 => {}
            _ => {
                let m = 0.5 * (a + z);
                if depth < MAX_TUBE_HALVINGS
                    && count + stack.len() + 2 <= MAX_TUBE_PIECES
                    && a < m
                    && m < z
                {
                    stack.push((m, z, depth + 1));
                    stack.push((a, m, depth + 1));
                } else if regular_only {
                    return Ok(TubeRun::Undecided);
                } else {
                    undecided = true;
                }
            }
        }
    }
    Ok(if folds {
        TubeRun::Folds { least }
    } else if undecided {
        TubeRun::Undecided
    } else {
        TubeRun::Regular
    })
}

/// Whether the tube of radius `d` a rounded offset caps the edge of `eu` with is regular: the
/// sweep of `c + d u`, `u` on the arc between the normals of the edge's two face uses on the
/// side of `eu`'s face use, turned against them unless `outward` (module docs, "A rounded
/// offset's tube"). A fold names the largest radius at which the tube is proved regular.
///
/// # Errors
///
/// A cancellation; those of reading the edge and its uses. An edge not between two face uses,
/// one whose curve has no NURBS form, or whose uses' normals cannot be read, is
/// [`TubeRegularity::NotCertified`], not an error.
pub fn tube_regularity(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    d: f64,
    outward: bool,
) -> Result<TubeRegularity> {
    let Some(e) = eu1::edge(b, eu)? else {
        return Ok(TubeRegularity::NotCertified);
    };
    let pair = match Pair::of(k, b, e) {
        Ok(Some(p)) => p,
        Err(err) if err.is_unported_or_cancelled() => return Err(err),
        Ok(None) | Err(_) => return Ok(TubeRegularity::NotCertified),
    };
    // the pair's uses are the edge's first use and its radial, on one side of the faces: the
    // normals are turned round where `eu` is on the other side (its mate is one of them)
    let side = if eu == pair.a.eu || eu == pair.c.eu {
        1.0
    } else {
        match eu1::mate(b, eu)? {
            Some(m) if m == pair.a.eu || m == pair.c.eu => -1.0,
            _ => return Ok(TubeRegularity::NotCertified),
        }
    };
    let Some(cn) = edge_nurbs(b, e)? else {
        return Ok(TubeRegularity::NotCertified);
    };
    let sigma = Ivl::from(if outward { side } else { -side });
    let least = match tube_run(k, &cn, &pair, sigma, d, false)? {
        TubeRun::Regular => return Ok(TubeRegularity::Regular),
        TubeRun::Undecided => return Ok(TubeRegularity::NotCertified),
        TubeRun::Folds { least } => least,
    };
    // the largest radius named: `upper = 1 / least` bounds every regular radius from above (a
    // piece where `max u . k >= least` folds past it), so the first of `upper (1 - 2^-j)`,
    // j = 8 down to 1, the decision proves regular
    let upper = 1.0 / least;
    let mut largest = None;
    if upper.is_finite() && upper > 0.0 {
        for j in (1..=8).rev() {
            let r = upper * (1.0 - (-f64::from(j)).exp2());
            if r < d && matches!(tube_run(k, &cn, &pair, sigma, r, true)?, TubeRun::Regular) {
                largest = Some(r);
                break;
            }
        }
    }
    Ok(TubeRegularity::Folds { largest })
}

/// Edge `e`'s curve as NURBS data (a line, a conic or a B-spline curve); `None` for a curve with
/// no NURBS form (or none whose data reads).
///
/// # Errors
///
/// A cancellation or an unported routine while reading it; those of reading the edge.
fn edge_nurbs(b: &Brep, e: EdgeId) -> Result<Option<NurbsCurve>> {
    use crate::curve::curve::CurveKind;
    let Some(curve) = edge::curve(b, e)? else {
        return Ok(None);
    };
    let line;
    let base = match curve.kind() {
        CurveKind::BSpline(c) => c,
        CurveKind::Ellipse(c) => c.base(),
        CurveKind::Parabola(c) => c.base(),
        CurveKind::Hyperbola(c) => c.base(),
        CurveKind::Line(c) => match c.base_with_nurb() {
            Ok(l) => {
                line = l;
                &*line
            }
            Err(err) if err.is_unported_or_cancelled() => return Err(err),
            Err(_) => return Ok(None),
        },
        _ => return Ok(None),
    };
    Ok(NurbsCurve::of(base).ok())
}

/// `H = A' W - A W'` over `s` within one knot span of the curve (the span of its middle),
/// enclosed: `W^2` times the curve's derivative, so the same direction, and a polynomial in the
/// curve's parameter on the span. `None` where the basis is not enclosed.
fn curve_h(cn: &NurbsCurve, s: Ivl) -> Option<I3> {
    let p = cn.p;
    let k = span(&cn.u, p, cn.n, s.mid());
    let (n, dn) = basis(&cn.u, k, p, s)?;
    let zero = Ivl::from(0.0);
    let (mut a, mut da, mut w, mut dw) = ([zero; 3], [zero; 3], zero, zero);
    for j in 0..=p {
        let i = (k + j).checked_sub(p)?;
        let c = cn.pw.get(4 * i..4 * i + 4)?;
        let (nj, dj) = (*n.get(j)?, *dn.get(j)?);
        for ((ax, dax), &cx) in a.iter_mut().zip(da.iter_mut()).zip(c) {
            *ax = *ax + nj * Ivl::from(cx);
            *dax = *dax + dj * Ivl::from(cx);
        }
        let cw = Ivl::from(*c.get(3)?);
        w = w + nj * cw;
        dw = dw + dj * cw;
    }
    let [x, y, z] = a;
    let [dx, dy, dz] = da;
    Some([dx * w - x * dw, dy * w - y * dw, dz * w - z * dw])
}

/// Halvings of one piece [`side_along`] makes before leaving it undecided: a piece is then
/// 2^-24 of one between the knots. A work budget.
const MAX_SIDE_HALVINGS: u32 = 24;

/// Pieces [`side_along`] decides on one edge use before leaving it undecided. A work budget,
/// not a tolerance.
const MAX_SIDE_PIECES: usize = 4096;

/// The answer of [`side_along`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SideAlong {
    /// All along the edge, the binormal into the face has at most the part asked along the
    /// direction.
    Within,
    /// The part is more at `at` (the edge's parameter): a point where it is proved, or the
    /// middle of a piece all over which it is.
    RunsOn {
        /// Where it runs on.
        at: f64,
    },
    /// A piece around `at` is not placed on either side, even halved, or the budget ran out.
    Undecided {
        /// The middle of the piece.
        at: f64,
    },
}

/// Whether the face of edge use `eu` runs on along `along` across the edge anywhere: the part
/// along `along` of the unit binormal into the face (normal to the edge, in the face) is at
/// most `max_part` all along the edge, proved over the whole edge interval, not at points.
///
/// The binormal is `sigma (F x H)` made unit: `F` the face use's normal along the pcurve,
/// enclosed over a piece of the edge as [`tangent_all_along`] encloses it (module docs: the net's
/// homogeneous point and partials over the pcurve's box, in rounded interval arithmetic), `H`
/// the edge curve's derivative times `W^2` enclosed from its NURBS form (`curve_h`), and
/// `sigma` the sign that turns it into the binormal [`evaluate_binormal`] gives, read once where
/// the product is proved non-zero (a cosine under `SAME_SURFACE_COS` is refused, as for the
/// face's normal). On a piece on one cell of the face's net, `sigma F . (H x along)` is a
/// polynomial in the edge's parameter, bounded from its values at the piece's Chebyshev points
/// as [`tangent_all_along`] bounds `Q` (module docs), and the piece is placed where that bound
/// is within `max_part` times the floor of `|F x H| |along|` (a point past it runs on: a witness).
/// Else `P = sigma (F x H) . along / (|F x H| |along|)` is enclosed over the piece: an
/// upper end within `max_part` places the piece, a lower end past it is a piece all of which runs
/// on ([`SideAlong::RunsOn`]), and any other piece is halved, at most `MAX_SIDE_HALVINGS` times
/// and to at most `MAX_SIDE_PIECES` pieces, past which it is left undecided and the rest are
/// still decided: a piece that runs on decides the edge, else one left undecided makes it
/// [`SideAlong::Undecided`]. A piece whose product is not proved non-zero is halved the same way.
/// The part's numerator is taken as `F . (H x along)`, the same triple product, so that zeros
/// of `H x along` stay exact.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an `along` that is not finite or has no length, a `max_part`
/// not finite in `(-1, 1)`, or a face use whose normal or binormal is not proved non-zero
/// anywhere along the edge; [`SolidError::Unsupported`] for a procedural blend's or sweep's face
/// (its net only approximates it), an edge whose curve has no NURBS form, and as [`tangent_all_along`]
/// for the face and its pcurve; [`SolidError::Cancelled`] when the kernel's cancel token is
/// cancelled; those of the evaluations.
pub fn side_along(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    along: Vec3,
    max_part: f64,
) -> Result<SideAlong> {
    let finite = [along.x, along.y, along.z].iter().all(|c| c.is_finite());
    let part_ok = max_part.is_finite() && max_part > -1.0 && max_part < 1.0;
    if !finite || !part_ok {
        return Err(invalid("a direction and a part that are finite"));
    }
    let d = [Ivl::from(along.x), Ivl::from(along.y), Ivl::from(along.z)];
    let nd = norm(d);
    if !nd.is_positive() {
        return Err(invalid("a direction of no length"));
    }
    let e = eu1::edge(b, eu)?.ok_or_else(|| invalid("an edge use without edge"))?;
    let mut u = Use::new(k, b, eu)?;
    // a procedural surface's net only approximates it (the definitions contract; the kinds
    // `exact_nurbs_surface` turns down): the side is read on the net alone, so such a face is
    // refused, not decided on the net
    let procedural = {
        let f = eu1::face(b, eu)?.ok_or_else(|| invalid("an edge use without face"))?;
        b.face(f)?.surface.as_ref().is_some_and(|s| {
            s.procedural_sweep().is_some() || matches!(s.kind(), SurfaceKind::ProceduralBlend(_))
        })
    };
    if procedural || u.definition.is_some() {
        return Err(unsupported(
            "the side of an edge on a procedural blend or sweep (its net only approximates it)",
        ));
    }
    let cn =
        edge_nurbs(b, e)?.ok_or_else(|| unsupported("an edge whose curve has no NURBS form"))?;
    let ivl = edge::interval(b, e)?;
    let (t0, t1) = (ivl.min(), ivl.max());
    let mut breaks = vec![t0, t1];
    breaks.extend(u.nurb.knots().iter().copied().filter(|&t| t > t0 && t < t1));
    breaks.extend(cn.u.iter().copied().filter(|&t| t > t0 && t < t1));
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    // `F` and `H = W^2 C'` over `s` (`None` where either is not enclosed)
    let factors = |u: &Use, s: Ivl| -> Result<Option<(I3, I3)>> {
        let Some(f) = u.f_box(s)? else {
            return Ok(None);
        };
        Ok(curve_h(&cn, s).map(|h| (f, h)))
    };
    // the degree of `F . (H x d)` along a piece on one cell: `F`'s plus `H`'s (`W^2 C'` is
    // `A' W - A W'`, of degree `2p - 2`, or `p - 1` where the weights are one value)
    let rational = {
        let w: Vec<f64> = cn.pw.chunks(4).filter_map(|c| c.get(3).copied()).collect();
        w.first()
            .is_some_and(|&first| w.iter().any(|&x| x != first))
    };
    let h_degree = if rational {
        (2 * cn.p).saturating_sub(2)
    } else {
        cn.p.saturating_sub(1)
    };
    let degree = u.degree.saturating_add(h_degree);
    let nodes = degree + 1;
    let lebesgue_n = lebesgue(nodes);
    // the face use's sense, then sigma, each at the first point where it can be read
    let mut sigma = None;
    'read: for (s0, s1) in pieces(&breaks) {
        for t in chebyshev(s0, s1, 4) {
            if u.sign.is_none() && !u.read_sign(k, b, t)? {
                continue;
            }
            let Some(g) = factors(&u, Ivl::from(t))?.map(|(f, c1)| cross(f, c1)) else {
                continue;
            };
            if !norm(g).is_positive() {
                continue;
            }
            let mid = Vec3::new(g[0].mid(), g[1].mid(), g[2].mid());
            let binormal =
                evaluate_binormal(k, b, eu, t, true, &BinormalOptions::default())?.binormal;
            let len = mid.length() * binormal.length();
            if !(len > 0.0) {
                continue;
            }
            let c = mid.dot(binormal) / len;
            if c.abs() < SAME_SURFACE_COS {
                return Err(unsupported(
                    "an edge use whose binormal and its NURBS forms' disagree",
                ));
            }
            sigma = Some(Ivl::from(c.signum()));
            break 'read;
        }
    }
    let Some(sigma) = sigma else {
        return Err(invalid(
            "an edge use whose binormal is not proved non-zero anywhere along it",
        ));
    };
    let mut stack: Vec<(f64, f64, u32)> = pieces(&breaks)
        .into_iter()
        .map(|(a, z)| (a, z, 0))
        .collect();
    let mut count = 0usize;
    let mut undecided: Option<f64> = None;
    while let Some((a, z, depth)) = stack.pop() {
        if count % POLL_POINTS == 0 {
            k.check_cancelled(OP)?;
        }
        count += 1;
        let at = 0.5 * (a + z);
        // the budget counts every piece, the knots' own too
        if count > MAX_SIDE_PIECES {
            return Ok(SideAlong::Undecided { at });
        }
        let piece = Ivl::new(a, z);
        // on one cell, `sigma F . (H x d)` is a polynomial of degree `degree` in `t`: bounded from
        // its values at the piece's Chebyshev points (module docs, `poly_range`), which are
        // narrow where it is 0, against the floor of `|F x H| |d|` over the piece
        if degree <= MAX_DEGREE && u.one_cell(piece)? {
            let mut values = Vec::with_capacity(nodes);
            let mut witness = None;
            for t in chebyshev(a, z, nodes) {
                let Some((f, h)) = factors(&u, Ivl::from(t))? else {
                    values.clear();
                    break;
                };
                let n = sigma * dot(f, cross(h, d));
                let den = norm(cross(f, h)) * nd;
                if den.is_positive() && (n / den).lo > max_part {
                    witness = Some(t);
                    break;
                }
                values.push(n);
            }
            if let Some(at) = witness {
                return Ok(SideAlong::RunsOn { at });
            }
            let d2 = (degree as f64) * (degree as f64);
            let eta = node_error(a, z) * d2 * (1.0 + gamma(2));
            let floor = factors(&u, piece)?.map(|(f, h)| (norm(cross(f, h)) * nd).lo);
            let placed = match (poly_range(&values, lebesgue_n, eta), floor) {
                (Some(range), Some(floor)) => {
                    !values.is_empty()
                        && floor > 0.0
                        && range.hi <= (Ivl::from(max_part) * Ivl::from(floor)).lo
                }
                _ => false,
            };
            if placed {
                continue;
            }
        }
        // the part as F . (H x d) / (|F x H| |d|), the triple product of (F x H) . d: where d
        // and H have exact zeros (a direction along an axis, a curve in a coordinate plane) the
        // zeros of H x d stay exact, where (F x H) . d would carry F's widths into them
        let part = match factors(&u, piece)? {
            Some((f, h)) => {
                let ng = norm(cross(f, h));
                ng.is_positive()
                    .then(|| sigma * dot(f, cross(h, d)) / (ng * nd))
            }
            None => None,
        };
        match part {
            Some(p) if p.hi <= max_part => {}
            Some(p) if p.lo > max_part => return Ok(SideAlong::RunsOn { at }),
            _ => {
                if depth < MAX_SIDE_HALVINGS
                    && count + stack.len() + 2 <= MAX_SIDE_PIECES
                    && a < at
                    && at < z
                {
                    stack.push((at, z, depth + 1));
                    stack.push((a, at, depth + 1));
                } else {
                    // a piece around a point where the part crosses `max_part` is never placed:
                    // go on, as another piece may run on all over
                    undecided.get_or_insert(at);
                }
            }
        }
    }
    if let Some(at) = undecided {
        return Ok(SideAlong::Undecided { at });
    }
    Ok(SideAlong::Within)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box's edge, as a test's plain body.
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn box_edge(k: &mut Kernel) -> (Brep, EdgeId) {
        use crate::math::Point3;
        use crate::primitive::api_primitives::create_box;
        use crate::query::api_queries::get_edges;
        let b = create_box(k, Point3::new(0.0, 0.0, 0.0), 2.0, 3.0, 4.0).unwrap();
        let e = *get_edges(&b).unwrap().first().expect("an edge");
        (b, e)
    }

    /// The decision polls the kernel's cancel token: cancelled, it returns `Cancelled`
    /// (a box's square edge, `NotTangent` otherwise). Between planes no inner call polls (the
    /// margin is 0 at once), so this is the decision loop's own poll.
    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_cancelled_decision_returns_cancelled() {
        let mut k = Kernel::default();
        let (mut b, e) = box_edge(&mut k);
        assert_eq!(
            tangent_all_along(&k, &mut b, e, 0.01).unwrap(),
            Some(Tangency::NotTangent)
        );
        let token = crate::kernel::CancelToken::new();
        token.cancel();
        k.set_cancel_token(token);
        let r = tangent_all_along(&k, &mut b, e, 0.01);
        assert!(matches!(r, Err(SolidError::Cancelled { .. })), "{r:?}");
    }

    /// The procedural round's root edges, at kernel level, with the token already cancelled:
    /// `Cancelled` on each.
    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_cancelled_procedural_round_decision_returns_cancelled() {
        let mut k = Kernel::default();
        let (mut b, edges) = circle_root_round(&mut k);
        let token = crate::kernel::CancelToken::new();
        token.cancel();
        k.set_cancel_token(token);
        assert!(!edges.is_empty());
        for e in edges {
            let r = tangent_all_along(&k, &mut b, e, 0.5_f64.to_radians());
            assert!(
                matches!(r, Err(SolidError::Cancelled { .. })),
                "{e:?}: {r:?}"
            );
        }
    }

    /// A face in power-basis form (its net holds coefficients, not control points) is
    /// refused by name, not read as a control net.
    #[test]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn a_face_in_power_basis_form_is_refused() {
        let mut k = Kernel::default();
        let (mut b, e) = box_edge(&mut k);
        let eu = *edge::edgeuses(&b, e, None, None)
            .unwrap()
            .first()
            .expect("a use");
        let f = eu1::face(&b, eu).unwrap().expect("its face");
        let surface = b.face(f).unwrap().surface.as_ref().expect("a surface");
        let mut bs = as_b_spline(&k, surface)
            .unwrap()
            .expect("a NURBS form")
            .into_owned();
        bs.form = BSplineSurfaceForm::Polynomial;
        b.face_mut(f).unwrap().surface = Some(Surface::from(bs));
        let err = tangent_all_along(&k, &mut b, e, 0.01).unwrap_err();
        assert!(format!("{err:?}").contains("power-basis"), "{err:?}");
    }

    /// `wr698`'s `circle_root`: a plate with a pipe lying along it 4 degrees off x, sunk into it,
    /// the root rounded r 2 by the ball's exact circle (an enclosed definition); the round's
    /// edges along the root (not its ends, cross-sections at the plate's sides).
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn circle_root_round(k: &mut Kernel) -> (Brep, Vec<EdgeId>) {
        use crate::blend::fillet_executive::api::fillet_edges;
        use crate::boolean::api_brep::boolean_union;
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::math::Point3;
        use crate::primitive::api_primitives::{create_box, create_cylinder};
        use crate::query::api_queries::{edge_closest_point, get_edges};
        use crate::topology::face;
        let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
        let mut pipe = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 10.0, 60.0).unwrap();
        let phi = 4.0_f64.to_radians();
        let place = Axis2Placement::new(
            Point3::new(0.0, 20.0, 16.0),
            Vec3::new(-phi.sin(), 0.0, -phi.cos()),
            Vec3::new(0.0, 1.0, 0.0),
        );
        crate::topology::brep::p5::transform(k, &mut pipe, &place, None, false).unwrap();
        let mut b = boolean_union(k, plate, pipe).unwrap();
        let q = Point3::new(30.0, 29.2, 10.0);
        let root = get_edges(&b)
            .unwrap()
            .into_iter()
            .filter_map(|e| {
                edge_closest_point(k, &b, e, q)
                    .ok()
                    .map(|c| (e, c.closest.distance))
            })
            .filter(|&(_, d)| d <= 1.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .expect("the root edge")
            .0;
        fillet_edges(k, &mut b, &[root], 2.0, 1, 1, 1.0).unwrap();
        let rounds: Vec<_> = b
            .faces()
            .ids()
            .filter(|&f| {
                face::p2::surface(&b, f)
                    .unwrap()
                    .is_some_and(|s| s.procedural_blend().is_some())
            })
            .collect();
        assert!(!rounds.is_empty(), "the round is a definition");
        // the round's edges along the root, which runs along x (not its ends, cross-sections
        // at the plate's sides, which meet those square)
        let mut edges = Vec::new();
        for &f in &rounds {
            for e in face::p4::edges(&b, f, None, false, None).unwrap() {
                let t = edge::interval(&b, e).unwrap();
                let curve = edge::curve(&b, e).unwrap().expect("an edge curve");
                let x = |t: f64| curve.evaluate_point(k, t).unwrap().x;
                if (x(t.max()) - x(t.min())).abs() > 1.0 && !edges.contains(&e) {
                    edges.push(e);
                }
            }
        }
        (b, edges)
    }

    /// `wr698`'s `circle_root` (a pipe's root on a plate rounded r 2 by the ball's exact
    /// circle, a procedural blend whose definition is enclosed): on its edges along the root,
    /// each piece's margin is bounded, halved where it is too loose, and both edges are
    /// Tangent. Where the pipe's foot crosses its net's C1 quarter knot (u = 15.708,
    /// multiplicity 2, near `s` = 87.22) the definition's feet box is taken cell by cell (the
    /// C1-knot follow-up to tangency-labs/solidcraft#14284, a use of order 1), also where the
    /// foot sits on the knot just outside the guesses' box and the one solve's contraction
    /// reaches the line (tangency-labs/solidcraft#15183, edge 46 at `s` = 87.2234). Before
    /// them, both edges were Undecided (pi, pi) at 87.04 with 3 parts not enclosed (#15005).
    #[test]
    #[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    fn a_procedural_round_s_edges_carry_its_margin() {
        let mut k = Kernel::default();
        let (mut b, edges) = circle_root_round(&mut k);
        let half = 0.5_f64.to_radians();
        // every edge measured before anything is asserted, so a failure names them all
        let mut seen = Vec::new();
        for e in edges {
            let Some(pair) = Pair::of(&k, &mut b, e).unwrap() else {
                continue;
            };
            if pair.a.definition.is_none() && pair.c.definition.is_none() {
                continue;
            }
            let mut left = Vec::new();
            // a piece's margin, halved where it is too loose; the definition is enclosed on
            // every piece, the pipe's quarter knot included
            let mut stack: Vec<(f64, f64, u32)> = pieces(&pair.breaks)
                .into_iter()
                .map(|(a, b)| (a, b, 0))
                .collect();
            while let Some((s0, s1, depth)) = stack.pop() {
                let kind = pair.margin(&k, s0, s1).unwrap();
                // the diagnosis, built only where a piece fails (it costs as much as a margin)
                let why = || [&pair.a, &pair.c].map(|u| margin_why(&k, u, s0, s1));
                match kind {
                    Margin::Bounded(m) if m < half => assert!(m > 0.0, "{e:?} [{s0}, {s1}]: {m}"),
                    Margin::NotEnclosed if depth >= MAX_UNENCLOSED_SPLITS => {
                        left.push((s0, s1, why()));
                    }
                    // halved as the decision halves a piece (to MAX_CELL_DEPTH on the box path;
                    // the margin falls with the part's width)
                    _ if depth < MAX_CELL_DEPTH => {
                        let mid = 0.5 * (s0 + s1);
                        stack.push((s0, mid, depth + 1));
                        stack.push((mid, s1, depth + 1));
                    }
                    other => panic!("{e:?} [{s0}, {s1}]: {other:?}: {:?}", why()),
                }
            }
            let mut counts = Counts::default();
            let verdict = decide(&k, &mut b, e, half, &mut counts).unwrap();
            seen.push((e, verdict, counts.not_enclosed, left));
        }
        eprintln!("circle_root edges: {seen:?}");
        let all = seen.iter().all(|(_, verdict, not_enclosed, left)| {
            *verdict == Some(Tangency::Tangent) && *not_enclosed == 0 && left.is_empty()
        });
        assert!(seen.len() == 2 && all, "{seen:?}");
    }

    /// Which step of [`Use::margin`] leaves a piece's margin unbounded (a test's diagnosis).
    fn margin_why(k: &Kernel, u: &Use, s0: f64, s1: f64) -> String {
        let Some(def) = &u.definition else {
            return "no definition".to_owned();
        };
        let bx = match u.uv_box(Ivl::new(s0, s1)) {
            Ok(Some(bx)) => bx,
            other => return format!("uv_box {:?}", other.map(|o| o.is_some())),
        };
        let n = &u.net;
        let at = format!(
            "box u [{}, {}] v [{}, {}] straddles {} dom {:?} {:?}",
            bx.u.lo,
            bx.u.hi,
            bx.v.lo,
            bx.v.hi,
            bx.straddles,
            domain(&n.u, n.p),
            domain(&n.v, n.q)
        );
        let ku = span(&n.u, n.p, n.nu, bx.u.mid());
        let kv = span(&n.v, n.q, n.nv, bx.v.mid());
        let mut d = match gn_def_face_of(k, def) {
            Ok(d) => d,
            Err(e) => return format!("{at}: gn_def_face_of {e:?}"),
        };
        let parts = match definition_partials_over_box(
            k, &mut d, n, ku, kv, bx.u.lo, bx.u.hi, bx.v.lo, bx.v.hi, 1,
        ) {
            Ok(p) => p,
            Err(e) => return format!("{at}: partials {e:?}"),
        };
        let (su, sv) = (parts.at(1, 0), parts.at(0, 1));
        let def_n = cross([su.x, su.y, su.z], [sv.x, sv.y, sv.z]);
        match u.n_w(bx.u, bx.v) {
            Ok(net_n) => {
                let cos = dot(def_n, net_n) / (norm(def_n) * norm(net_n));
                format!(
                    "{at}: cos [{}, {}], |def_n| [{}, {}], |net_n| [{}, {}]",
                    cos.lo,
                    cos.hi,
                    norm(def_n).lo,
                    norm(def_n).hi,
                    norm(net_n).lo,
                    norm(net_n).hi
                )
            }
            Err(e) => format!("{at}: n_w {e:?}"),
        }
    }

    /// A pcurve along `u = s` (linear, and the same line as a cubic): its box over
    /// `[s0, s1]` holds `[s0, s1]` and lies within it widened by exactly the bounds the code
    /// takes: the spans' rounding (`gamma_(12 n + 1)` of the largest control point's each
    /// coordinate, as
    /// `Use::new` widens them) and the interval arithmetic's own. Each interval operation
    /// widens its result by `2.3e-16 |x|` (`interval::round_up`). A split's level per
    /// coordinate, `p + l (q - p)`, is a difference, a product and a sum: at most 4
    /// widenings of `M` (the difference up to `2 M`). The parameter's own width, from its
    /// differences of values up to `M` over the span's length `b - a`, costs at most
    /// `|q - p| <= b - a` times that: 5 widenings of `M` for `l1`, 11 for `l0 / l1`. So at
    /// most 15 per level, 30 over the two splits' `2 n` levels, as much again for `W`, and
    /// the quotient `X / W` once: under the `108 n` widenings of `M` taken. At the span's
    /// start, middle and end, and on
    /// a span far from 0 (`[89.88234, 89.88559]`, `circle_root`'s round), where the
    /// evaluation's box is the whole span (the partition of unity lost).
    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_line_pcurve_s_box_is_its_piece() {
        let line = |a: f64, b: f64, n: usize| {
            let size = a.abs().max(b.abs());
            BezSpan {
                a,
                b,
                points: (0..=n)
                    .map(|i| {
                        let f = i as f64 / n as f64;
                        [a + (b - a) * f, 0.0, 1.0]
                    })
                    .collect(),
                rounding: [gamma(12 * n + 1) * size, 0.0, gamma(12 * n + 1)],
            }
        };
        for (span, n) in [
            (line(0.0, 1.0, 1), 1),
            (line(0.0, 1.0, 3), 3),
            (line(89.88234, 89.88559, 1), 1),
        ] {
            let (a, b) = (span.a, span.b);
            let size = a.abs().max(b.abs()).max(1.0);
            #[allow(clippy::cast_precision_loss)]
            let slack =
                span.rounding[0] + size * span.rounding[2] + 108.0 * f64::from(n) * 2.3e-16 * size;
            let w = b - a;
            for (f0, f1) in [(0.0, 0.1), (0.45, 0.55), (0.9, 1.0)] {
                let (s0, s1) = (a + w * f0, a + w * f1);
                let (u, v) = sub_span_hull(&span, s0, s1).unwrap();
                assert!(u.lo <= s0 && s1 <= u.hi, "[{s0}, {s1}] not in {u:?}");
                let within = s0 - slack <= u.lo && u.hi <= s1 + slack;
                assert!(within, "{u:?} past [{s0}, {s1}] +- {slack}");
                assert!(
                    v.lo <= 0.0 && 0.0 <= v.hi && v.hi - v.lo <= 2.0 * slack,
                    "{v:?}"
                );
            }
        }
    }

    /// Two unit vectors `eps` apart, each enclosed to `delta`: the cosine form bounds the
    /// angle by about `sqrt(eps^2 + 4 delta)`, the sine form by about `eps + 2 delta`; both hold
    /// the true angle, and the sine's is far the smaller when `delta` is small.
    #[test]
    fn the_sine_form_bounds_a_small_angle_linearly() {
        let (eps, delta) = (1e-3_f64, 1e-9_f64);
        let w = |x: f64| Ivl::new(x - delta, x + delta);
        let d = [w(1.0), w(0.0), w(0.0)];
        let b = [w(eps.cos()), w(eps.sin()), w(0.0)];
        let got = angle_between(d, b).expect("acute");
        assert!(got >= eps, "{got} below the true angle {eps}");
        assert!(got <= eps + 4.0 * delta + 1e-12, "{got}");
        let cos = dot(d, b) / (norm(d) * norm(b));
        let by_cos = cos.lo.acos();
        assert!(by_cos > got, "the cosine form {by_cos} is the looser");
        assert!(angle_between([w(1.0), w(0.0), w(0.0)], [w(-1.0), w(0.0), w(0.0)]).is_none());
    }

    #[test]
    fn second_derivatives_of_a_bernstein_basis() {
        // each end's step per interval operation (derived below)
        const RHO: f64 = 2.3e-16 + f64::EPSILON / 2.0;
        // N = (1 - x)^2, 2 x (1 - x), x^2: N'' = 2, -4, 2 everywhere
        let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let [n, d, d2] = basis2(&knots, 2, 2, Ivl::from(0.25)).unwrap();
        // The width, derived. Each interval operation rounds its float result (within `u |x|`,
        // u = 2^-53) and then moves each end out by `2.3e-16 |end|` (interval.rs's `round_up`;
        // its 1e-300 is far below), so each end lies within `RHO |x|` of the exact value,
        // `RHO = 2.3e-16 + u`. Over an expression, the counts add through a product (both
        // factors' errors) and through a sum take the larger branch plus one, so each end is
        // within `g(k) A`, `g(k) = k RHO / (1 - k RHO)`, with `A` the expression evaluated on
        // magnitudes (every difference a sum). `basis`'s level is at most 6 operations (the knot
        // difference, the quotient, `u - x` or `x - u`, the product, the sum): a value of degree
        // `p`, `6 p`; a derivative takes the degree `p - 1` values and 5 more (the knot
        // difference, the quotient, the sum to 0, the difference, the factor `p`):
        // `6 (p - 1) + 5`; the second derivative takes the degree `p - 1` derivatives and 5
        // more: `6 (p - 2) + 10`. Here (p = 2, x = 1/4, Bernstein) `A` is the value itself for
        // `N` (every term non-negative), `p (N_{j-1,1} + N_{j,1})` for `N'` (1.5, 2, 0.5) and
        // `p (|N'_{j-1,1}| + |N'_{j,1}|)` for `N''` (2, 4, 2).
        let g = |k: f64| k * RHO / (1.0 - k * RHO);
        let held = |x: Ivl, want: f64, k: f64, a: f64| {
            x.lo <= want && want <= x.hi && x.hi - x.lo <= 2.0 * g(k) * a
        };
        let (kv, kd, kdd) = (12.0, 11.0, 10.0);
        let ok_n = held(n[0], 0.5625, kv, 0.5625)
            && held(n[1], 0.375, kv, 0.375)
            && held(n[2], 0.0625, kv, 0.0625);
        assert!(ok_n, "{n:?}");
        let ok_d =
            held(d[0], -1.5, kd, 1.5) && held(d[1], 1.0, kd, 2.0) && held(d[2], 0.5, kd, 0.5);
        assert!(ok_d, "{d:?}");
        let ok_dd =
            held(d2[0], 2.0, kdd, 2.0) && held(d2[1], -4.0, kdd, 4.0) && held(d2[2], 2.0, kdd, 2.0);
        assert!(ok_dd, "{d2:?}");
    }

    fn i3(v: [f64; 3]) -> I3 {
        v.map(Ivl::from)
    }

    #[test]
    fn bernstein_roots_are_isolated_and_exact() {
        // (s - 0.25)(s - 0.75) = s^2 - s + 3/16, in Bernstein form on [0, 1]:
        // 3/16, 3/16 - 1/2, 3/16
        let c = [0.1875, 0.1875 - 0.5, 0.1875];
        let mut roots = Vec::new();
        let mut work = MAX_SUBDIVISIONS;
        assert!(bernstein_roots(
            &c, 0.0, 1.0, MAX_DEPTH, &mut work, &mut roots
        ));
        roots.sort_by(f64::total_cmp);
        let want = [0.25, 0.75];
        assert_eq!(roots.len(), want.len(), "{roots:?}");
        assert!(
            roots.iter().zip(want).all(|(r, w)| (r - w).abs() < 1e-15),
            "{roots:?}"
        );
        let mut none = Vec::new();
        assert!(bernstein_roots(
            &[1.0, 0.5, 2.0],
            0.0,
            1.0,
            MAX_DEPTH,
            &mut work,
            &mut none
        ));
        assert!(none.is_empty());
        // (s - 1/2)^2 = s^2 - s + 1/4: a double root, never isolated by a sign change; its
        // coefficients halve to exact zeros at 1/2, both halves' ends there
        let mut touch = Vec::new();
        bernstein_roots(
            &[0.25, -0.25, 0.25],
            0.0,
            1.0,
            MAX_DEPTH,
            &mut work,
            &mut touch,
        );
        assert!(touch.iter().all(|&x| x == 0.5), "{touch:?}");
    }

    /// `P(t) = -1 + (8/3) t^2` along one direction, `Q = 1`: at the 3 Chebyshev points of
    /// `[-1, 1]` (0 and +-sqrt(3)/2) `|P|` is 1, an angle of 45 degrees, but at `t = 1` it is
    /// 5/3, 59.04 degrees. Decided at 50 degrees from the points alone this was "tangent"; the
    /// bound `atan(L)`, `L` = 1.70, is 59.5 degrees, over 50, so it is not.
    #[test]
    fn a_piece_s_bound_covers_the_angle_between_its_points() {
        let n = 3;
        let nodes: Vec<Node> = chebyshev(-1.0, 1.0, n)
            .map(|t| {
                let p = -1.0 + 8.0 / 3.0 * t * t;
                Node {
                    p: i3([p, 0.0, 0.0]),
                    q: Ivl::from(1.0),
                    angle_lo: Some(p.abs().atan2(1.0)),
                }
            })
            .collect();
        let at_points = nodes.iter().filter_map(|n| n.angle_lo).fold(0.0, f64::max);
        assert!(
            (at_points - std::f64::consts::FRAC_PI_4).abs() < 1e-12,
            "{at_points}"
        );
        let between = (5.0_f64 / 3.0).atan();
        let bound = piece_bound(&nodes, lebesgue(n), 0.0).unwrap();
        assert!(bound >= between, "{bound} under {between}");
        assert!(bound > 50.0_f64.to_radians(), "{bound}");
        // and no more than atan(L): M = 1, Q = 1 (the constant cannot grow unseen)
        // atan(L) with the bound's own factors: L's 16 u (atan grows by at most its argument's
        // relative change), the atan's 8 u and the interval quotient's outward rounding, under
        // 32 u; 64 u takes it
        let most = lebesgue(n).atan() * (1.0 + 64.0 * UNIT);
        assert!(bound <= most, "{bound} over {most}");
    }

    /// The points' rounding: `P = 1` and `Q = 1` at 3 points with `eta > 0` give
    /// `||P|| <= L / (1 - L eta)`, `||Q|| <= L / (1 - L eta)`, `Q >= 1 - L eta ||Q||`, so the
    /// bound `atan(||P|| / Q_floor)`; none where `L eta >= 1`.
    #[test]
    fn the_points_rounding_widens_the_bound() {
        let n = 3;
        let lam = lebesgue(n);
        let nodes: Vec<Node> = (0..n)
            .map(|_| Node {
                p: i3([1.0, 0.0, 0.0]),
                q: Ivl::from(1.0),
                angle_lo: Some(std::f64::consts::FRAC_PI_4),
            })
            .collect();
        let eta = 0.05;
        let room = 1.0 - lam * eta;
        let (p_norm, q_norm) = (lam / room, lam / room);
        let want = (p_norm / (1.0 - lam * eta * q_norm)).atan();
        let got = piece_bound(&nodes, lam, eta).unwrap();
        assert!(
            got >= want && got <= want * (1.0 + 1e-12),
            "{got} vs {want}"
        );
        assert!(got > piece_bound(&nodes, lam, 0.0).unwrap());
        assert_eq!(piece_bound(&nodes, lam, 1.0 / lam), None);
    }

    /// `Q` varying: `Q(t) = 1 + t^2 / 2` at the 3 Chebyshev points (1, 1.375, 1.375), `P = 0.1`:
    /// the floor is `c - L r` with `c = 1.1875`, `r = 0.1875`.
    #[test]
    fn the_floor_of_q_is_its_middle_less_l_half_ranges() {
        let n = 3;
        let lam = lebesgue(n);
        let nodes: Vec<Node> = chebyshev(-1.0, 1.0, n)
            .map(|t| Node {
                p: i3([0.1, 0.0, 0.0]),
                q: Ivl::from(1.0 + 0.5 * t * t),
                angle_lo: Some(0.1),
            })
            .collect();
        let floor = 1.1875 - lam * 0.1875;
        let want = (lam * 0.1 / floor).atan();
        let got = piece_bound(&nodes, lam, 0.0).unwrap();
        assert!((got - want).abs() <= 1e-12 * want, "{got} vs {want}");
    }

    /// `n` nodes of one value each, `P = (p, 0, 0)` and `Q = q`.
    fn constant_nodes(n: usize, p: f64, q: f64) -> Vec<Node> {
        (0..n)
            .map(|_| Node {
                p: i3([p, 0.0, 0.0]),
                q: Ivl::from(q),
                angle_lo: None,
            })
            .collect()
    }

    /// The obtuse bound against closed forms: `(1, -1)` is 3 pi / 4 and `(sqrt 3, -1)` is
    /// 2 pi / 3, each reached from above and within 16 u of it. It is never under a right
    /// angle: at `Q = 0` it is over the `f64` under pi / 2 (which is less than pi / 2), so no
    /// angle asked under a right angle is ever met by it. None where `|P|` is not bounded away
    /// from 0 (`atan2(0, 0)` is 0, and a fold's angle is pi) or an end is not finite.
    #[test]
    #[allow(clippy::unwrap_used)]
    fn the_obtuse_bound_is_the_angle_s_upper_end() {
        use std::f64::consts::{FRAC_PI_2, PI};
        for (p, q, want) in [
            (1.0, -1.0, 0.75 * PI),
            (3.0_f64.sqrt(), -1.0, 2.0 * PI / 3.0),
            (1.0, 0.0, FRAC_PI_2),
        ] {
            let got = obtuse_bound(p, q).unwrap();
            assert!(got > want, "{got} not over {want}");
            assert!(got <= want * (1.0 + 16.0 * UNIT), "{got} against {want}");
        }
        for (p, q) in [(1.0, 0.0), (1.0, -0.0), (1e300, -1e-300), (1.0, -1e-17)] {
            assert!(obtuse_bound(p, q).unwrap() > FRAC_PI_2, "{p} {q}");
        }
        for (p, q) in [
            (0.0, 0.0),
            (0.0, -0.0),
            (0.0, -1.0),
            (-1.0, -1.0),
            (f64::INFINITY, -1.0),
            (1.0, f64::NEG_INFINITY),
            (f64::NAN, -1.0),
            (1.0, f64::NAN),
            // a positive Q is the acute bound's, from |P|'s upper end
            (1.0, 1e-300),
        ] {
            assert_eq!(obtuse_bound(p, q), None, "{p} {q}");
        }
    }

    /// Over a box, the obtuse bound is from `|P|`'s and `Q`'s lower ends: `|P|` in [1, 2] and
    /// `Q` in [-1, 1/2] reach atan2(1, -1) = 3 pi / 4 (not atan2(2, -1), nor atan2(1, 1/2)).
    /// None where the box of `P` holds 0.
    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_box_s_obtuse_bound_is_at_its_lower_ends() {
        let zero = Ivl::from(0.0);
        let q = Ivl::new(-1.0, 0.5);
        let got = box_bound([Ivl::new(1.0, 2.0), zero, zero], q).unwrap();
        let want = 0.75 * std::f64::consts::PI;
        // |P|'s square, sum and root each rounded outward: under 64 u with the bound's own
        assert!(
            got > want && got <= want * (1.0 + 64.0 * UNIT),
            "{got} against {want}"
        );
        assert_eq!(box_bound([Ivl::new(-1.0, 2.0), zero, zero], q), None);
    }

    /// `P` and `Q` constant and in proportion: `G` is 0, so the bound is their angle, 3 pi / 4,
    /// whatever their size (`e`'s length is divided out) and the points' rounding `eta`, as long
    /// as `E` is bounded away from 0 (`1 - L^2 eta / (1 - L eta) > 0`).
    #[test]
    #[allow(clippy::unwrap_used)]
    fn p_and_q_in_proportion_are_bounded_by_their_angle() {
        let n = 3;
        let lam = lebesgue(n);
        let want = 0.75 * std::f64::consts::PI;
        for eta in [0.0, 0.05] {
            for size in [1.0, 4.0, 1e-3] {
                let got = piece_bound(&constant_nodes(n, size, -size), lam, eta).unwrap();
                assert!(
                    got >= want && got <= want * (1.0 + 1e-12),
                    "{size} {eta}: {got} vs {want}"
                );
            }
        }
        // L^2 eta >= 1 - L eta, then L eta >= 1
        for eta in [1.0 / (lam * lam), 1.0 / lam] {
            let got = piece_bound(&constant_nodes(n, 1.0, -1.0), lam, eta);
            assert_eq!(got, None, "{eta}");
        }
    }

    /// `P = 1` and `Q` -1, -1 and -1/2 at 3 points: `s` is 1 and `G` 0, 0 and 1/2, so
    /// `G >= 1/4 - L (1/4 + eta L (1/2) / (1 - L eta))` and `E >= 1 - L^2 eta / (1 - L eta)`:
    /// the points' rounding widens both.
    #[test]
    #[allow(clippy::unwrap_used)]
    fn the_points_rounding_widens_the_obtuse_bound() {
        let n = 3;
        let lam = lebesgue(n);
        let bound = |eta: f64| {
            let mut nodes = constant_nodes(n, 1.0, -1.0);
            nodes.last_mut().unwrap().q = Ivl::from(-0.5);
            piece_bound(&nodes, lam, eta).unwrap()
        };
        for eta in [0.0, 0.05] {
            let room = 1.0 - lam * eta;
            let under = lam * (0.25 + eta * lam * 0.5 / room) - 0.25;
            let e_lo = 1.0 - lam * (eta * lam / room);
            let want = 1.0_f64.atan2(-(1.0 + under / e_lo));
            let got = bound(eta);
            assert!(
                got >= want && got <= want * (1.0 + 1e-12),
                "{eta}: {got} vs {want}"
            );
        }
        assert!(bound(0.05) > bound(0.0));
    }

    /// `P(t) = 3 - (8/3) t^2` along one direction, `Q = -1`: at the 3 Chebyshev points of
    /// `[-1, 1]` `|P|` is 3, 1 and 1, angles of at most 135 degrees, but at `t = 1` it is 1/3,
    /// 161.57 degrees. The floor is `2 - L` (`L` = 1.70), 163.3 degrees: never under the angle
    /// between the points, and no more than that floor's.
    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_piece_s_obtuse_bound_covers_the_angle_between_its_points() {
        let n = 3;
        let lam = lebesgue(n);
        let nodes: Vec<Node> = chebyshev(-1.0, 1.0, n)
            .map(|t| Node {
                p: i3([0.0, 3.0 - 8.0 / 3.0 * t * t, 0.0]),
                q: Ivl::from(-1.0),
                angle_lo: None,
            })
            .collect();
        let between = (1.0_f64 / 3.0).atan2(-1.0);
        assert!(between > 0.75 * std::f64::consts::PI + 0.4);
        let bound = piece_bound(&nodes, lam, 0.0).unwrap();
        assert!(bound >= between, "{bound} under {between}");
        let floor = (2.0 - lam).atan2(-1.0);
        assert!((bound - floor).abs() <= 1e-12, "{bound} against {floor}");
    }

    /// `P = (1 - t^2, 2 t, 0)` turns nearly all the way round over `[-8, 8]`, `|P| = 1 + t^2`
    /// and `Q = -(1 + t^2)`: no one direction bounds `|P|` away from 0 there, so nothing is
    /// proved (the piece is halved); over a short piece of it the bound is the angle, 3 pi / 4.
    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_p_that_turns_round_is_bounded_on_short_pieces_only() {
        let n = 3;
        let lam = lebesgue(n);
        let nodes = |s0: f64, s1: f64| -> Vec<Node> {
            chebyshev(s0, s1, n)
                .map(|t| Node {
                    p: i3([1.0 - t * t, 2.0 * t, 0.0]),
                    q: Ivl::from(-1.0 - t * t),
                    angle_lo: None,
                })
                .collect()
        };
        assert_eq!(piece_bound(&nodes(-8.0, 8.0), lam, 0.0), None);
        let want = 0.75 * std::f64::consts::PI;
        for (s0, s1) in [(-0.001, 0.001), (3.0, 3.001), (-1.001, -1.0)] {
            let got = piece_bound(&nodes(s0, s1), lam, 0.0).unwrap();
            assert!(got >= want && got <= want + 0.01, "[{s0}, {s1}]: {got}");
        }
    }

    /// Random `P` and `Q` of degree up to 6 on random pieces, their values enclosed at the
    /// rounded points and `eta` as the decision takes it: wherever a bound is given, no point
    /// of 129 across the piece, its ends among them, has an angle proved over it (each value
    /// enclosed, the angle's lower end taken). Many bounds are obtuse, and at many of those
    /// the angle between the points is over every one at them.
    #[test]
    #[allow(clippy::unwrap_used, clippy::indexing_slicing)]
    fn no_point_of_a_piece_is_over_its_bound() {
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut unit = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            // in [-1, 1)
            (seed >> 11) as f64 / (1_u64 << 52) as f64 - 1.0
        };
        let value = |c: &[f64], t: f64| {
            c.iter()
                .rev()
                .fold(Ivl::from(0.0), |v, &x| v * Ivl::from(t) + Ivl::from(x))
        };
        let angle_lo = |p: I3, q: Ivl| {
            let a = norm(p).lo.atan2(q.hi);
            a - 8.0 * UNIT * a.abs()
        };
        let (mut obtuse, mut between, mut acute, mut tighter) = (0, 0, 0, 0);
        for case in 0..20_000_usize {
            let degree = case % 7;
            // the constant terms of size about 1, the rest smaller or not
            let size = [0.05, 0.3, 1.0][case % 3];
            let mut coeffs = || -> Vec<f64> {
                (0..=degree)
                    .map(|i| if i == 0 { unit() } else { size * unit() })
                    .collect()
            };
            let (p, q) = ([coeffs(), coeffs(), coeffs()], coeffs());
            let (x, y) = (unit(), unit());
            let (s0, s1) = (x.min(y), x.max(y));
            if s1 - s0 < 1e-3 {
                continue;
            }
            let at = |t: f64| ([0, 1, 2].map(|i| value(&p[i], t)), value(&q, t));
            let n = degree + 1;
            let nodes: Vec<Node> = chebyshev(s0, s1, n)
                .map(|t| {
                    let (p, q) = at(t);
                    Node {
                        p,
                        q,
                        angle_lo: Some(angle_lo(p, q)),
                    }
                })
                .collect();
            let d = degree as f64;
            let eta = node_error(s0, s1) * d * d * (1.0 + gamma(2));
            let Some(bound) = piece_bound(&nodes, lebesgue(n), eta) else {
                continue;
            };
            let worst = (0..=128)
                .map(|i| s0 + (s1 - s0) * f64::from(i) / 128.0)
                .map(|t| {
                    let (p, q) = at(t.min(s1));
                    angle_lo(p, q)
                })
                .fold(0.0, f64::max);
            let part = part_bound(&nodes, lebesgue(n), eta).unwrap();
            assert!(
                worst <= part && part <= bound,
                "case {case}: {worst} over {part} or {bound} on [{s0}, {s1}]"
            );
            tighter += usize::from(part < bound);
            let at_nodes = nodes.iter().filter_map(|n| n.angle_lo).fold(0.0, f64::max);
            if bound > std::f64::consts::FRAC_PI_2 {
                obtuse += 1;
                between += usize::from(worst > at_nodes);
            } else {
                acute += 1;
            }
        }
        eprintln!("{obtuse} obtuse, {between} of them between the points, {acute} acute, {tighter} of them tighter");
        // not vacuous
        assert!(
            obtuse > 1000 && between > 500 && acute > 1000 && tighter > 1000,
            "{obtuse} obtuse, {between} between the points, {acute} acute, {tighter} tighter"
        );
    }

    /// The points a decision over a right angle takes, against the budget of `MAX_POINTS` a
    /// piece: a cylinder's side and caps, square all along their circles, and a frustum's side
    /// and base, acos(-3/5) (126.87 degrees) apart all along theirs. Each is decided `Tangent` a
    /// hair over its angle in a small share of one piece's budget; without the obtuse bound no
    /// part was ever bounded, and every part was halved until the budget of each piece was spent.
    #[test]
    #[allow(clippy::unwrap_used)]
    fn an_edge_past_square_is_decided_in_few_points() {
        use crate::math::Point3;
        use crate::primitive::api_primitives::{create_cone, create_cylinder};
        use crate::query::api_queries::get_edges;
        use std::f64::consts::FRAC_PI_2;
        let mut k = Kernel::default();
        let o = Point3::new(0.0, 0.0, 0.0);
        let cylinder = create_cylinder(&mut k, o, 3.0, 7.0).unwrap();
        let frustum = create_cone(&mut k, o, 5.0, 2.0, 4.0).unwrap();
        let mut seen = 0;
        for (mut b, want, overs) in [
            (cylinder, FRAC_PI_2, [1e-9, 1e-6, 1e-3]),
            (frustum, (-0.6_f64).acos(), [1e-6, 1e-4, 1e-2]),
        ] {
            for e in get_edges(&b).unwrap() {
                let below = tangent_all_along(&k, &mut b, e, want - 1e-9).unwrap();
                // the seam (smooth) and the frustum's top (53.13 degrees) are within it
                if below != Some(Tangency::NotTangent) {
                    continue;
                }
                seen += 1;
                for over in overs {
                    let mut counts = Counts::default();
                    let got = decide(&k, &mut b, e, want + over, &mut counts).unwrap();
                    eprintln!("{want} + {over:e}: {got:?}, {} points", counts.points);
                    assert_eq!(got, Some(Tangency::Tangent), "{e:?} {want} {over:e}");
                    let most = if over < 1e-5 { 10 } else { 100 };
                    assert!(
                        counts.points * most <= MAX_POINTS,
                        "{e:?} {want} {over:e}: {} points",
                        counts.points
                    );
                }
            }
        }
        // the cylinder's two circles and the frustum's base
        assert_eq!(seen, 3);
    }

    /// `P` and `Q` constant and in proportion (`P = Q / 2`): `G` and `R` are 0, so the bound is
    /// their angle, atan(1/2), whatever the points' number and rounding `eta`, where the bound
    /// of `|P|` on its own is atan(L / 2) at the least. Never over it.
    #[test]
    #[allow(clippy::unwrap_used)]
    fn p_and_q_in_proportion_are_bounded_by_their_acute_angle() {
        let want = 0.5_f64.atan();
        for n in [2, 3, 11] {
            let lam = lebesgue(n);
            for eta in [0.0, 0.01] {
                let nodes = constant_nodes(n, 0.5, 1.0);
                let got = part_bound(&nodes, lam, eta).unwrap();
                assert!(
                    got >= want && got <= want * (1.0 + 1e-12),
                    "{n} {eta}: {got} vs {want}"
                );
                let alone = piece_bound(&nodes, lam, eta).unwrap();
                assert!(alone >= (0.5 * lam).atan(), "{n} {eta}: {alone}");
            }
        }
    }

    /// `P = (1, a t, 0)`, `a = 1/10`, and `Q = 1` at the 3 Chebyshev points of `[-1, 1]` (0 and
    /// +-sqrt(3)/2): along `u = (1, 0, 0)` `E` is 1 and `R = (0, a t, 0)`, at most
    /// `L a sqrt(3)/2 / (1 - L eta)` long with the points' rounding `eta`, and
    /// `Q >= 1 - L^2 eta / (1 - L eta)`, so the bound is atan(sqrt(1 + (that over this)^2)), over
    /// the angle at `t = 1`, atan(sqrt(1 + a^2)).
    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_part_s_bound_counts_the_turning_of_p() {
        let (n, a) = (3, 0.1);
        let lam = lebesgue(n);
        let nodes: Vec<Node> = chebyshev(-1.0, 1.0, n)
            .map(|t| Node {
                p: i3([1.0, a * t, 0.0]),
                q: Ivl::from(1.0),
                angle_lo: None,
            })
            .collect();
        for eta in [0.0, 0.01] {
            let room = 1.0 - lam * eta;
            let r = lam * a * 0.75_f64.sqrt() / room / (1.0 - lam * eta * lam / room);
            let want = (1.0 + r * r).sqrt().atan();
            let got = part_bound(&nodes, lam, eta).unwrap();
            assert!((got - want).abs() <= 1e-12, "{eta}: {got} vs {want}");
            assert!(got >= (1.0 + a * a).sqrt().atan());
        }
    }

    /// `P(t) = 1/10 - t^2` along one direction and `Q = 1`: at the middle point `P` is 1/10, at
    /// the other two -0.65, so `E` is mostly against its direction: its size is bounded from its
    /// lower end, `L` half ranges and the points' rounding, `L eta L 0.65 / (1 - L eta)`, under
    /// its middle, over the 9/10 at `t = 1` (42 degrees; the points' own largest is 33).
    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_part_s_bound_counts_p_against_its_direction() {
        let n = 3;
        let lam = lebesgue(n);
        let nodes: Vec<Node> = chebyshev(-1.0, 1.0, n)
            .map(|t| Node {
                p: i3([0.1 - t * t, 0.0, 0.0]),
                q: Ivl::from(1.0),
                angle_lo: None,
            })
            .collect();
        for eta in [0.0, 0.01] {
            let room = 1.0 - lam * eta;
            let against = 0.275 + lam * (0.375 + eta * lam * 0.65 / room);
            let want = (against / (1.0 - lam * eta * lam / room)).atan();
            let got = part_bound(&nodes, lam, eta).unwrap();
            assert!((got - want).abs() <= 1e-12, "{eta}: {got} vs {want}");
            assert!(got >= 0.9_f64.atan(), "{eta}: {got}");
        }
    }

    /// `P = 1/2` and `Q` 1, 1 and 2 at 3 points: `s` is 1/2 and `G = Q/2 - 1/2` 0, 0 and 1/2, so
    /// `G >= 1/4 - L (1/4 + eta L (1/2) / (1 - L eta))`, and
    /// `Q >= 3/2 - L (1/2 + eta L 2 / (1 - L eta))`: the points' rounding widens both.
    #[test]
    #[allow(clippy::unwrap_used)]
    fn the_points_rounding_widens_a_part_s_bound() {
        let n = 3;
        let lam = lebesgue(n);
        let bound = |eta: f64| {
            let mut nodes = constant_nodes(n, 0.5, 1.0);
            nodes.last_mut().unwrap().q = Ivl::from(2.0);
            part_bound(&nodes, lam, eta).unwrap()
        };
        for eta in [0.0, 0.01] {
            let room = 1.0 - lam * eta;
            let under = lam * (0.25 + eta * lam * 0.5 / room) - 0.25;
            let q_lo = 1.5 - lam * (0.5 + eta * lam * 2.0 / room);
            let want = (0.5 + under / q_lo).atan();
            let got = bound(eta);
            assert!(
                got >= want && got <= want * (1.0 + 1e-12),
                "{eta}: {got} vs {want}"
            );
        }
        assert!(bound(0.01) > bound(0.0));
    }

    /// The bound a part takes is `piece_bound`'s or a tighter one, and gives none where that
    /// does: an obtuse piece, and one `P` is exactly 0 on.
    #[test]
    fn a_part_s_bound_is_the_piece_s_where_there_is_no_tighter() {
        let lam = lebesgue(3);
        for (p, q) in [(1.0, -1.0), (0.0, 1.0), (0.0, -1.0), (1.0, 0.0)] {
            let nodes = constant_nodes(3, p, q);
            assert_eq!(
                part_bound(&nodes, lam, 0.0),
                piece_bound(&nodes, lam, 0.0),
                "{p} {q}"
            );
        }
    }

    /// The points an acute decision takes on a frustum's top circle, its side and top the same
    /// angle apart all along it, against the budget of `MAX_POINTS` a piece: nearly flat ones
    /// at 0.25 to 0.499 degrees asked half a degree, and a steep one at acos(3/5) (53.13
    /// degrees) asked a little over it. With `|P|` bounded on its own no part's bound ever went
    /// under atan(L tan) of the angle, `L` = 2.53 for its 11 points, however short: each was
    /// halved `MAX_SPLITS` times over and left undecided.
    #[test]
    #[allow(clippy::unwrap_used)]
    fn an_edge_under_the_angle_asked_is_decided_in_few_points() {
        use crate::math::Point3;
        use crate::primitive::api_primitives::create_cone;
        use crate::query::api_queries::{edge_closest_point, get_edges};
        let half = 0.5_f64.to_radians();
        let steep = 0.6_f64.acos();
        let mut k = Kernel::default();
        // (the true angle, the angle asked)
        let mut rows: Vec<(f64, f64)> = [0.25_f64, 0.3, 0.4, 0.49, 0.499]
            .map(|deg| (deg.to_radians(), half))
            .to_vec();
        rows.extend([1e-1, 1e-2, 1e-4].map(|over| (steep, steep + over)));
        for (angle, asked) in rows {
            let h = 3.0 * angle.tan();
            let o = Point3::new(0.0, 0.0, 0.0);
            let mut b = create_cone(&mut k, o, 5.0, 2.0, h).unwrap();
            let top = Point3::new(0.0, 2.0, h);
            let mut found = 0;
            for e in get_edges(&b).unwrap() {
                let near = edge_closest_point(&k, &b, e, top).unwrap();
                if near.closest.distance > 1e-9 {
                    continue;
                }
                found += 1;
                let mut counts = Counts::default();
                let got = decide(&k, &mut b, e, asked, &mut counts).unwrap();
                eprintln!("{angle} asked {asked}: {got:?}, {} points", counts.points);
                assert_eq!(got, Some(Tangency::Tangent), "{angle} asked {asked}");
                assert!(
                    counts.points * 10 <= MAX_POINTS,
                    "{angle} asked {asked}: {} points",
                    counts.points
                );
                // and the angle itself is never under it
                let under = angle * (1.0 - 1e-9);
                assert_eq!(
                    tangent_all_along(&k, &mut b, e, under).unwrap(),
                    Some(Tangency::NotTangent),
                    "{angle}"
                );
            }
            assert_eq!(found, 1, "{angle}");
        }
    }

    /// A pcurve running `1e-15` off the knot line `0.5` of a degree-1 net: its box, however
    /// small, either meets the line (the piece is halved, then bounded over the box) or lies
    /// wholly on one side of it, where `F` is one polynomial; never a cell taken as one that
    /// is two. Past the domain `[0, 2 pi]` a box is clamped as the face's evaluator clamps it:
    /// a box wholly past goes to the end, one straddling the end is clamped and marked (the
    /// clamp bends `F` there), one inside is kept.
    #[test]
    fn a_box_beside_a_knot_line_or_past_the_domain() {
        let lines = knot_lines(&[0.0, 0.0, 0.5, 1.0, 1.0], 1);
        assert_eq!(lines, vec![0.5]);
        let off = 0.5 + 1e-15;
        let point = Ivl::from(off);
        assert!(
            !meets(&lines, point),
            "a point beside the line is in one cell"
        );
        let across = Ivl::from(0.5 - 1e-15).hull(point);
        assert!(meets(&lines, across), "a box across the line is two cells");
        assert!(meets(&lines, Ivl::from(0.5)), "a box at the line meets it");
        let tau = std::f64::consts::TAU;
        let d = Some((0.0, tau));
        let (c, s) = clamp(Ivl::from(tau + 0.1).hull(Ivl::from(tau + 0.2)), d);
        assert_eq!((c.lo, c.hi, s), (tau, tau, false));
        let (c, s) = clamp(Ivl::from(-0.2).hull(Ivl::from(-0.1)), d);
        assert_eq!((c.lo, c.hi, s), (0.0, 0.0, false));
        let (c, s) = clamp(Ivl::from(tau - 0.1).hull(Ivl::from(tau + 0.1)), d);
        assert_eq!((c.lo, c.hi, s), (tau - 0.1, tau, true));
        let (c, s) = clamp(Ivl::from(1.0).hull(Ivl::from(2.0)), d);
        assert_eq!((c.lo, c.hi, s), (1.0, 2.0, false));
        let (c, s) = clamp(Ivl::from(-5.0), None);
        assert_eq!((c.lo, s), (-5.0, false));
    }

    #[test]
    fn knot_lines_are_the_domain_s_interior_knots() {
        assert_eq!(
            knot_lines(&[0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0], 2),
            vec![0.5]
        );
        assert!(knot_lines(&[0.0, 0.0, 1.0, 1.0], 1).is_empty());
    }
}

#[cfg(test)]
mod g2_tests {
    use super::*;

    #[test]
    fn g2_degrees_follow_the_module_docs() {
        // a bicubic polynomial net, a cubic pcurve: d = 5, F 2 d r = 30, P^ 6 d r + 4 r - 4 = 98
        assert_eq!(g2_degrees(3, 3, 3, false, false), (30, 98));
        // a rational net of degrees (2, 1), a line pcurve: d = 5, F 10, P^ 30
        assert_eq!(g2_degrees(2, 1, 1, true, false), (10, 30));
        // a plane bends no way
        assert_eq!(g2_degrees(1, 1, 3, false, true), (0, 0));
    }

    /// A cylinder `S = (R cos u, R sin u, v)` with the edge along v: across it (along u) its
    /// curvature against the natural (outward) normal is `-1 / R`.
    #[test]
    fn a_cylinder_bends_one_over_its_radius_across_a_ruling() {
        let (r, u) = (3.0_f64, 0.7_f64);
        let i3 = |v: [f64; 3]| v.map(Ivl::from);
        let st = Starred {
            s: [
                i3([-r * u.sin(), r * u.cos(), 0.0]),
                i3([0.0, 0.0, 1.0]),
                i3([-r * u.cos(), -r * u.sin(), 0.0]),
                i3([0.0; 3]),
                i3([0.0; 3]),
            ],
            w: Ivl::from(1.0),
        };
        let (n, wv, q) = across(&st, Ivl::from(0.0), Ivl::from(1.0));
        let kappa = dot(n, wv) / (norm(n) * q);
        assert!(kappa.lo <= -1.0 / r && -1.0 / r <= kappa.hi, "{kappa:?}");
    }

    /// The box and cylinder of the test below, and the edge along their tangent line.
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn wall_into_cylinder() -> (Kernel, Brep, EdgeId) {
        use crate::boolean::api_brep::boolean_union;
        use crate::math::Point3;
        use crate::primitive::api_primitives::{create_box, create_cylinder};
        use crate::query::api_queries::{edge_closest_point, get_edges};
        let mut k = Kernel::default();
        let block = create_box(&mut k, Point3::new(0.0, -5.0, 0.0), 20.0, 10.0, 4.0).unwrap();
        let drum = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 5.0, 4.0).unwrap();
        let b = boolean_union(&mut k, block, drum).unwrap();
        let q = Point3::new(0.0, -5.0, 2.0);
        let e = get_edges(&b)
            .unwrap()
            .into_iter()
            .filter_map(|e| {
                edge_closest_point(&k, &b, e, q)
                    .ok()
                    .map(|c| (e, c.closest.distance))
            })
            .filter(|&(_, d)| d <= 1e-6)
            .min_by(|a, c| a.1.total_cmp(&c.1))
            .expect("the tangent line's edge")
            .0;
        (k, b, e)
    }

    /// A 20 x 10 x 4 box from (0, -5, 0) united with a cylinder of radius 5, 4 high, about the
    /// z axis: the box's side y = -5 runs tangentially into the cylinder along the vertical
    /// line at (0, -5). The cylinder face is asserted exact (the cone class, a cylinder: its net
    /// the circle's exact rational form), so the jump is the closed form `1 / r`, `r` its
    /// radius, and the nets' certified enclosure at the witness holds it (`1 / r` itself
    /// rounded once: `u / r` more).
    #[test]
    #[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    fn a_wall_jumps_one_over_the_radius_into_its_tangent_cylinder() {
        use crate::query::edge::edge_faces;
        use crate::topology::face;
        let (k, mut b, e) = wall_into_cylinder();
        let mut planes = 0;
        let mut radius = None;
        for f in edge_faces(&b, e).unwrap() {
            match face::p2::surface(&b, f).unwrap().map(Surface::kind) {
                Some(SurfaceKind::Plane(_)) => planes += 1,
                Some(SurfaceKind::Cone(c)) => {
                    assert!(c.is_cylinder(), "the cone class face is a cylinder");
                    radius = Some(c.radius(0.0));
                }
                other => panic!("an unexpected face: {}", other.is_some()),
            }
        }
        let r = radius.expect("the cylinder");
        assert_eq!(planes, 1);
        let want = 1.0 / r;
        match g2_all_along(&k, &mut b, e, 0.5_f64.to_radians(), 0.5_f64.to_radians()).unwrap() {
            Some(G2::Jumps { jump, within, .. }) => {
                let off = (jump.abs() - want).abs();
                assert!(
                    off <= within + UNIT * want,
                    "{jump} +- {within} against {want}"
                );
            }
            other => panic!("{other:?}"),
        }
        // the scale is max(1 / r, 1 / L) = 1 / 4 (the edge 4 long): the jump 1 / 5 is within
        // twice it, so continuous where 2 is asked (as the jump's bound proves)
        assert_eq!(
            g2_all_along(&k, &mut b, e, 0.5_f64.to_radians(), 2.0).unwrap(),
            Some(G2::Continuous)
        );
    }

    /// The search keeps to its budget: none spent at a budget of 0 (each point is checked
    /// against it before it is evaluated), so undecided; at the budget the whole search took,
    /// per piece no more than that, the same verdict at the same count.
    #[test]
    #[allow(clippy::unwrap_used)]
    fn the_search_keeps_to_its_point_budget() {
        let (k, mut b, e) = wall_into_cylinder();
        let half = 0.5_f64.to_radians();
        let (full, n) = g2_within(&k, &mut b, e, half, 2.0, MAX_POINTS).unwrap();
        assert_eq!(full, Some(G2::Continuous));
        assert!(n > 0);
        let (none, zero) = g2_within(&k, &mut b, e, half, 2.0, 0).unwrap();
        assert_eq!(zero, 0);
        assert!(
            matches!(none, Some(G2::Undecided { bound, .. }) if bound.is_infinite()),
            "{none:?}"
        );
        assert_eq!(
            g2_within(&k, &mut b, e, half, 2.0, n).unwrap(),
            (Some(G2::Continuous), n)
        );
    }
}
