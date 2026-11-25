//! Faces with a pole of higher order (no normal at the point, `S_u × S_v` and its first-order
//! limit vanishing there), built exactly as records, each with the neighbour that shares its
//! edge through the pole.
//!
//! Every body is an open shell of two faces on one surface kind, scaled by `L` (mm):
//!
//! 1. `tangent`: the profile `r = z² / L` (a quadratic Bézier from the axis, tangent to it)
//!    turned about z. At the apex `S_s` is `L e_z`, `S_θ`, `S_sθ` vanish and
//!    `S_s × S_θ = L² (−s² cos θ, −s² sin θ, 2 s³)`: the first coefficient that does not vanish
//!    is `N_2 = −L² e_r(θ)`. Limit normal along any direction into the face from `(0, θ)`:
//!    `−e_r(θ) = (−cos θ, −sin θ, 0)`.
//! 2. `collapsed`: the cubic Bézier patch `S(u, v) = L (u³, u³ v, u)`, its row u = 0 collapsed
//!    to the origin, `S_v = S_uv = S_uuv = 0` there and `S_uuuv = 6 L e_y`.
//!    `S_u × S_v = L² (−u³, 0, 3 u⁵)`: `N_3 = −L² e_x`, limit `(−1, 0, 0)` from every point of the
//!    row along every direction with a positive u component. The neighbour is
//!    `L (u³, u³ (v − 1), u)`. [`Kind::Quartic`] is the same of degree four, `L (u⁴, u⁴ v, u)`:
//!    `S_u × S_v = L² (−u⁴, 0, 4 u⁷)`, a pole of order four whose `N_0` to `N_3` all vanish.
//! 3. `cone`: the straight profile `r = z` parameterised as `(L s², L s²)` (a quadratic Bézier
//!    whose first two control points coincide at the apex) turned about z: a 45° cone whose
//!    apex is of third order. `S_s = 2 L s (e_r + e_z)`, `S_θ = L s² e_θ`,
//!    `S_s × S_θ = 2 L² s³ (−e_r + e_z)`: `N_3 = 2 L² (−e_r(θ) + e_z)`, limit
//!    `(−cos θ, −sin θ, 1) / √2`, a different normal along every meridian.
//!
//! The revolutions' faces are quarter turns θ ∈ [0, π/2] (the face) and [π/2, π] (its
//! neighbour), sharing the meridian θ = π/2 and the apex, held as the record holds a revolution:
//! rational splines of degree (2, 2), v the quarter arc's rational parameter, θ = [`angle`]`(v)`
//! (θ' > 0, so the orders and signs above hold with `S_v = θ' S_θ`). The patches share the curve
//! `L (u³, 0, u)`.
//!
//! [`closed_cone`] is the cone of 3. on one spline closed along v (the whole turn, four quarter
//! arcs, v in [0, 1]), cut into two half faces along the meridians at v = 1/4 and 3/4: face 0
//! runs from v = 3/4 across the interval's end to 1/4 (its rim in two edges meeting there),
//! face 1 over [1/4, 3/4]. θ = [`closed_angle`].

use solidcore_record::brep::{
    Brep, Curve, Curve2, Edge, EdgeUseRef, Face, Loop, OrientedEdge, OrientedFace, Region, Shell,
    Surface, Vertex,
};

/// The bodies' scale (mm).
pub const L: f64 = 10.0;
const HALF_PI: f64 = std::f64::consts::FRAC_PI_2;

/// Which test face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Tangent,
    Collapsed,
    Cone,
    /// [`Kind::Collapsed`] of degree four: a pole of order four.
    Quartic,
}

impl Kind {
    /// The poles of order two and three, which have a limit normal.
    pub const LIMIT: [Kind; 3] = [Kind::Tangent, Kind::Collapsed, Kind::Cone];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Tangent => "face1",
            Kind::Collapsed => "face2",
            Kind::Cone => "face3",
            Kind::Quartic => "order4",
        }
    }

    /// The exact unit limit normal at the pole of the face, approached along the parameter
    /// direction `dir` from the pole point `(u, v)` (natural normal, `S_u × S_v`).
    /// Along every direction with a positive u component the limit is the same: `N_k` at the
    /// pole point does not depend on the direction (`D^m S_v = 0` for m < k − 1 and `S_u`
    /// only enters through its value), so it is a function of the pole point alone.
    pub fn limit(self, face: usize, at: [f64; 2]) -> [f64; 3] {
        match self {
            Kind::Tangent => {
                let th = angle(face, at[1]);
                [-th.cos(), -th.sin(), 0.0]
            }
            Kind::Collapsed | Kind::Quartic => [-1.0, 0.0, 0.0],
            Kind::Cone => {
                let th = angle(face, at[1]);
                let r = std::f64::consts::FRAC_1_SQRT_2;
                [-th.cos() * r, -th.sin() * r, r]
            }
        }
    }

    /// The pole face (0) and its neighbour (1).
    pub fn body(self) -> Brep {
        match self {
            Kind::Tangent => revolution([[0.0, 0.0, 0.0], [0.0, 0.0, 0.5 * L], [L, 0.0, L]]),
            Kind::Cone => revolution([[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [L, 0.0, L]]),
            Kind::Collapsed => patches(3),
            Kind::Quartic => patches(4),
        }
    }
}

fn line2(t: [f64; 2], a: [f64; 2], b: [f64; 2]) -> Curve2 {
    Curve2::Spline {
        knots: vec![t[0], t[0], t[1], t[1]],
        points: vec![a, b],
    }
}

fn vertex(p: [f64; 3]) -> Vertex {
    Vertex {
        point: p,
        tolerance: 1e-7,
    }
}

fn edge(curve: Curve, interval: [f64; 2], vertices: [u32; 2]) -> Edge {
    Edge {
        curve,
        nurbs: None,
        interval,
        vertices,
        tolerance: 1e-7,
        uses: Vec::new(),
    }
}

/// The two faces' loops (edge, forward, pcurve) to a body: fills the edges' uses.
fn body(
    surfaces: [Surface; 2],
    domains: [[[f64; 2]; 2]; 2],
    loops: [Vec<(u32, bool, Curve2)>; 2],
    mut edges: Vec<Edge>,
    vertices: Vec<Vertex>,
) -> Brep {
    let mut faces = Vec::new();
    for (fi, ((surface, domain), lp)) in surfaces.into_iter().zip(domains).zip(loops).enumerate() {
        let mut oes = Vec::new();
        for (pos, (e, forward, pcurve)) in lp.into_iter().enumerate() {
            if let Some(edge) = edges.get_mut(e as usize) {
                edge.uses.push(EdgeUseRef {
                    face: fi as u32,
                    r#loop: 0,
                    position: pos as u32,
                    upper: !forward,
                });
            }
            oes.push(OrientedEdge {
                edge: e,
                forward,
                pcurve,
            });
        }
        faces.push(Face {
            surface,
            nurbs: None,
            domain,
            loops: vec![Loop::Edges { edges: oes }],
        });
    }
    Brep {
        version: 4,
        tolerance: 1e-6,
        regions: vec![Region {
            shells: vec![0],
            void: false,
        }],
        shells: vec![Shell::Faces {
            faces: vec![
                OrientedFace {
                    face: 0,
                    forward: true,
                },
                OrientedFace {
                    face: 1,
                    forward: true,
                },
            ],
            wires: Vec::new(),
        }],
        faces,
        edges,
        vertices,
    }
}

/// The quarter circle's rational quadratic weight in the middle: cos 45°.
const MID: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// The angle about z of the rational quarter arc of face `face` (0: θ in [0, π/2], 1: in
/// [π/2, π]) at its parameter `v`: `atan2(y, x)` of the arc `(1, 0), (1, 1), (0, 1)` with weights
/// `1, cos 45°, 1`, turned a quarter for the neighbour.
pub fn angle(face: usize, v: f64) -> f64 {
    let (b0, b1, b2) = ((1.0 - v) * (1.0 - v), 2.0 * v * (1.0 - v) * MID, v * v);
    let th = (b1 + b2).atan2(b0 + b1);
    if face == 0 { th } else { th + HALF_PI }
}

/// The quadratic profile `ctrl` (in the xz half plane, from the apex at the origin to
/// `(L, 0, L)`) turned about z, as the rational spline of degree (2, 2) the record holds a
/// revolution as: faces θ ∈ [0, π/2] and [π/2, π], v the arc's rational parameter.
fn revolution(ctrl: [[f64; 3]; 3]) -> Brep {
    // The quarter arcs' control points (unit radius, weights 1, cos 45°, 1) from θ = 0 and π/2.
    let arcs: [[[f64; 3]; 3]; 2] = [
        [[1.0, 0.0, 1.0], [1.0, 1.0, MID], [0.0, 1.0, 1.0]],
        [[0.0, 1.0, 1.0], [-1.0, 1.0, MID], [-1.0, 0.0, 1.0]],
    ];
    let surface = |arc: &[[f64; 3]; 3]| Surface::RationalSpline {
        u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        homogeneous: ctrl
            .iter()
            .map(|p| {
                arc.iter()
                    .map(|a| [a[2] * p[0] * a[0], a[2] * p[0] * a[1], a[2] * p[2], a[2]])
                    .collect()
            })
            .collect(),
    };
    // The meridian at the arc's control point `a` (a corner: weight 1, on an axis).
    let meridian3 = |a: [f64; 3]| Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        points: ctrl
            .iter()
            .map(|p| [p[0] * a[0], p[0] * a[1], p[2]])
            .collect(),
    };
    let rim = |arc: &[[f64; 3]; 3]| Curve::RationalSpline {
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        homogeneous: arc
            .iter()
            .map(|a| [a[2] * L * a[0], a[2] * L * a[1], a[2] * L, a[2]])
            .collect(),
    };
    // Vertices: the apex, then the rim at θ = 0, π/2, π.
    let vertices = vec![
        vertex([0.0, 0.0, 0.0]),
        vertex([L, 0.0, L]),
        vertex([0.0, L, L]),
        vertex([-L, 0.0, L]),
    ];
    // Edges in canonical order: the meridian at 0, the first rim arc, the meridian at π/2
    // (shared), the second rim arc, the meridian at π.
    let edges = vec![
        edge(meridian3(arcs[0][0]), [0.0, 1.0], [0, 1]),
        edge(rim(&arcs[0]), [0.0, 1.0], [1, 2]),
        edge(meridian3(arcs[0][2]), [0.0, 1.0], [0, 2]),
        edge(rim(&arcs[1]), [0.0, 1.0], [2, 3]),
        edge(meridian3(arcs[1][2]), [0.0, 1.0], [0, 3]),
    ];
    let meridian = |v: f64| line2([0.0, 1.0], [0.0, v], [1.0, v]);
    let rim2 = line2([0.0, 1.0], [1.0, 0.0], [1.0, 1.0]);
    let loops = [
        vec![
            (0, true, meridian(0.0)),
            (1, true, rim2.clone()),
            (2, false, meridian(1.0)),
        ],
        vec![
            (2, true, meridian(0.0)),
            (3, true, rim2),
            (4, false, meridian(1.0)),
        ],
    ];
    body(
        [surface(&arcs[0]), surface(&arcs[1])],
        [[[0.0, 1.0], [0.0, 1.0]]; 2],
        loops,
        edges,
        vertices,
    )
}

/// `L (uⁿ, uⁿ (v + dv), u)` as a Bézier of degree (n, 1).
fn patch(n: usize, dv: f64) -> Surface {
    let points = (0..=n)
        .map(|i| {
            let power = if i == n { 1.0 } else { 0.0 };
            let line = i as f64 / n as f64;
            (0..2)
                .map(|j| [L * power, L * power * (f64::from(j) + dv), L * line])
                .collect()
        })
        .collect();
    Surface::Spline {
        u_knots: [vec![0.0; n + 1], vec![1.0; n + 1]].concat(),
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        points,
    }
}

/// The patch of degree `n` and its neighbour below it in y, sharing the curve
/// `L (uⁿ, 0, u)`.
fn patches(n: usize) -> Brep {
    let cubic = |y: f64| Curve::Spline {
        knots: [vec![0.0; n + 1], vec![1.0; n + 1]].concat(),
        points: (0..=n)
            .map(|i| {
                let line = L * i as f64 / n as f64;
                if i == n {
                    [L, L * y, L]
                } else {
                    [0.0, 0.0, line]
                }
            })
            .collect(),
    };
    let vertices = vec![
        vertex([0.0, 0.0, 0.0]),
        vertex([L, 0.0, L]),
        vertex([L, L, L]),
        vertex([L, -L, L]),
    ];
    // Edges in canonical order: the shared curve, the patch's side u = 1, its row v = 1, the
    // neighbour's row v = 0, its side u = 1.
    let edges = vec![
        edge(cubic(0.0), [0.0, 1.0], [0, 1]),
        edge(
            Curve::Line {
                start: [L, 0.0, L],
                end: [L, L, L],
            },
            [0.0, 1.0],
            [1, 2],
        ),
        edge(cubic(1.0), [0.0, 1.0], [0, 2]),
        edge(cubic(-1.0), [0.0, 1.0], [0, 3]),
        edge(
            Curve::Line {
                start: [L, -L, L],
                end: [L, 0.0, L],
            },
            [0.0, 1.0],
            [3, 1],
        ),
    ];
    let loops = [
        vec![
            (0, true, line2([0.0, 1.0], [0.0, 0.0], [1.0, 0.0])),
            (1, true, line2([0.0, 1.0], [1.0, 0.0], [1.0, 1.0])),
            (2, false, line2([0.0, 1.0], [0.0, 1.0], [1.0, 1.0])),
        ],
        vec![
            (3, true, line2([0.0, 1.0], [0.0, 0.0], [1.0, 0.0])),
            (4, true, line2([0.0, 1.0], [1.0, 0.0], [1.0, 1.0])),
            (0, false, line2([0.0, 1.0], [0.0, 1.0], [1.0, 1.0])),
        ],
    ];
    body(
        [patch(n, 0.0), patch(n, -1.0)],
        [[[0.0, 1.0], [0.0, 1.0]]; 2],
        loops,
        edges,
        vertices,
    )
}

/// The angle about z of the closed whole-turn arc of [`closed_cone`] at its parameter `v` (any
/// real: one turn per unit), each quarter `[i/4, (i + 1)/4]` a rational quarter arc from `iπ/2`.
pub fn closed_angle(v: f64) -> f64 {
    let turns = v.floor();
    let w = 4.0 * (v - turns);
    let i = w.floor().min(3.0);
    angle(0, w - i) + i * HALF_PI + turns * std::f64::consts::TAU
}

/// The 45° cone of [`Kind::Cone`] on one spline closed along v (module doc), in two half faces,
/// the first across the interval's end.
pub fn closed_cone() -> Brep {
    let ctrl = [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [L, 0.0, L]];
    // The whole turn's control points (unit radius) and weights, from θ = 0.
    let circle: [[f64; 3]; 9] = [
        [1.0, 0.0, 1.0],
        [1.0, 1.0, MID],
        [0.0, 1.0, 1.0],
        [-1.0, 1.0, MID],
        [-1.0, 0.0, 1.0],
        [-1.0, -1.0, MID],
        [0.0, -1.0, 1.0],
        [1.0, -1.0, MID],
        [1.0, 0.0, 1.0],
    ];
    let surface = Surface::RationalSpline {
        u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        v_knots: vec![
            0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
        ],
        homogeneous: ctrl
            .iter()
            .map(|p| {
                circle
                    .iter()
                    .map(|a| [a[2] * p[0] * a[0], a[2] * p[0] * a[1], a[2] * p[2], a[2]])
                    .collect()
            })
            .collect(),
    };
    let meridian3 = |a: [f64; 3]| Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        points: ctrl
            .iter()
            .map(|p| [p[0] * a[0], p[0] * a[1], p[2]])
            .collect(),
    };
    // The rim from the circle's control point `from` (a corner, at θ = from · π/4) over
    // `quarters` quarter turns.
    let rim = |from: usize, quarters: usize| Curve::RationalSpline {
        knots: [
            vec![0.0; 3],
            (1..quarters)
                .flat_map(|k| [k as f64 / quarters as f64; 2])
                .collect(),
            vec![1.0; 3],
        ]
        .concat(),
        homogeneous: (0..=2 * quarters)
            .map(|k| circle[(from + k) % 8])
            .map(|a| [a[2] * L * a[0], a[2] * L * a[1], a[2] * L, a[2]])
            .collect(),
    };
    // The apex, then the rim at θ = 3π/2, 0 (the interval's end) and π/2.
    let vertices = vec![
        vertex([0.0, 0.0, 0.0]),
        vertex([0.0, -L, L]),
        vertex([L, 0.0, L]),
        vertex([0.0, L, L]),
    ];
    // Canonical order: the meridian at 3π/2, the rim to the interval's end and on from its
    // start, the meridian at π/2, the rim from π/2 to 3π/2.
    let edges = vec![
        edge(meridian3(circle[6]), [0.0, 1.0], [0, 1]),
        edge(rim(6, 1), [0.0, 1.0], [1, 2]),
        edge(rim(0, 1), [0.0, 1.0], [2, 3]),
        edge(meridian3(circle[2]), [0.0, 1.0], [0, 3]),
        edge(rim(2, 2), [0.0, 1.0], [3, 1]),
    ];
    let meridian = |v: f64| line2([0.0, 1.0], [0.0, v], [1.0, v]);
    let rim2 = |a: f64, b: f64| line2([0.0, 1.0], [1.0, a], [1.0, b]);
    // Face 0's loop crosses the interval's end between its two rim edges: its pcurves run to
    // v = 1 and on from v = 0, the same points of the closed surface.
    let loops = [
        vec![
            (0, true, meridian(0.75)),
            (1, true, rim2(0.75, 1.0)),
            (2, true, rim2(0.0, 0.25)),
            (3, false, meridian(0.25)),
        ],
        vec![
            (3, true, meridian(0.25)),
            (4, true, rim2(0.25, 0.75)),
            (0, false, meridian(0.75)),
        ],
    ];
    body(
        [surface.clone(), surface],
        [[[0.0, 1.0], [0.0, 1.0]], [[0.0, 1.0], [0.25, 0.75]]],
        loops,
        edges,
        vertices,
    )
}
