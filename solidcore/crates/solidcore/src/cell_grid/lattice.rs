//! Cell lattices in their own frame.
//!
//! Every lattice has a cell size `s` (the cell's inscribed-circle diameter) and a web `w`, the
//! gap between neighbouring cells. Cell `(i, j)` has its centre at `center(i, j)`, worked out
//! from the integer indices and never by adding steps, so two fields with the same size, web,
//! angle and anchor get the very same centres.
//!
//! The tiling lattices (hexagon, square, triangle) have *lattice lines*: lines that cut every
//! cell they cross along a chord between two of its vertices, and pass through the web of every
//! other cell. Snapping moves field edges onto them. Lines run in the directions `k * step`
//! (`k` an integer); [`Lattice::family`] gives their offsets along the left normal of direction
//! `k * step`.
//!
//! The dot lattices (circles and regular polygons on a hexagonal or square grid of centres)
//! don't tile, have no lattice lines and never snap.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_3, FRAC_PI_4, FRAC_PI_6, PI, SQRT_2};

use crate::math::Vec2;

use super::region::{clip_halfplane, poly_area, Bounds};
use super::{CellGridError, CellShape, DotGrid, MAX_SIDES};

const SQRT3: f64 = 1.732_050_807_568_877_2;
/// Direction tolerance (radians) for an edge to count as running along a lattice line.
pub(crate) const ANGLE_TOL: f64 = 1e-7;
/// Circles are tested against their circumscribed polygon of this many sides: conservative by
/// 0.12% of the radius.
pub(crate) const CIRCLE_SIDES: usize = 64;
/// Most lattice cells a field may reach before it is refused.
pub const MAX_CELLS: usize = 100_000;

/// A regular polygon with inscribed radius `r_in` and `n` sides, counter-clockwise, centred on
/// the origin, with one edge horizontal at `y = -r_in` (a hexagon is then flat-top, a square
/// axis-aligned).
pub(crate) fn regular_polygon(r_in: f64, n: usize) -> Vec<Vec2> {
    let nf = n as f64;
    let rc = r_in / (PI / nf).cos();
    let a0 = -PI / 2.0 + PI / nf;
    (0..n)
        .map(|k| {
            let a = a0 + 2.0 * PI * k as f64 / nf;
            Vec2::new(rc * a.cos(), rc * a.sin())
        })
        .collect()
}

/// The distance between neighbouring centres of a grid of translated copies of `poly` that keeps
/// them at least `web` apart. Two copies a distance `d` apart along a unit direction `u` are
/// separated along `u` by `d` less the polygon's width along `u`, and a gap along one direction
/// is a gap in distance, so `d` is at least that width plus `web`: for each direction a
/// neighbour lies in (the grid's lines, and the diagonals of a square grid, a `sqrt 2` farther).
/// An odd-sided or vertex-first polygon is wider than its inscribed diameter, so its pitch is
/// larger than inscribed diameter plus web.
fn polygon_pitch(poly: &[Vec2], grid: DotGrid, web: f64) -> f64 {
    let width = |angle: f64| {
        let u = Vec2::new(angle.cos(), angle.sin());
        let (lo, hi) = poly.iter().fold((f64::MAX, f64::MIN), |(lo, hi), q| {
            let t = q.dot(u);
            (lo.min(t), hi.max(t))
        });
        hi - lo
    };
    let dirs: &[(f64, f64)] = match grid {
        // Neighbours at 90, 30 and -30 degrees (the lattice vectors are `p (0, 1)`, `p (sqrt 3 /
        // 2, 1/2)` and `p (sqrt 3 / 2, -1/2)`), all `p` away.
        DotGrid::Hexagonal => &[(FRAC_PI_2, 1.0), (FRAC_PI_6, 1.0), (-FRAC_PI_6, 1.0)],
        // Neighbours along both axes `p` away, and along both diagonals `p sqrt 2` away.
        DotGrid::Square => &[
            (0.0, 1.0),
            (FRAC_PI_2, 1.0),
            (FRAC_PI_4, SQRT_2),
            (-FRAC_PI_4, SQRT_2),
        ],
    };
    dirs.iter()
        .map(|&(a, reach)| (width(a) + web) / reach)
        .fold(0.0, f64::max)
}

/// How a piece of a cell is named by what it is, for the counts and for tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PieceKind {
    Whole,
    Pentagon,
    Half,
    Triangle,
    Part,
}

/// Offsets of the lattice lines of one direction: `period` apart, from each of the `phases`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Family {
    pub period: f64,
    phases: [f64; 2],
    n: usize,
}

impl Family {
    fn one(period: f64, a: f64) -> Family {
        Family {
            period,
            phases: [a, 0.0],
            n: 1,
        }
    }

    fn two(period: f64, a: f64, b: f64) -> Family {
        Family {
            period,
            phases: [a, b],
            n: 2,
        }
    }

    pub fn phases(&self) -> &[f64] {
        self.phases.get(..self.n).unwrap_or(&[])
    }

    fn negated(mut self) -> Family {
        for p in &mut self.phases {
            *p = -*p;
        }
        self
    }
}

enum Kind {
    Hex { h: f64, r: f64 },
    Square,
    Triangle { r: f64, rt: f64, l: f64, ht: f64 },
    Dot { grid: DotGrid, rc: f64 },
}

/// The outline of one kind of cell and what the chords cut it into.
struct KeyData {
    outline: Vec<Vec2>,
    tiles: Vec<Vec<Vec2>>,
    candidates: Vec<(PieceKind, Vec<Vec2>)>,
}

pub(crate) struct Lattice {
    kind: Kind,
    /// The cell's inscribed diameter.
    pub s: f64,
    /// The web between neighbouring cells.
    pub w: f64,
    /// The distance between neighbouring cell centres.
    pub p: f64,
    pub cell_area: f64,
    /// True when cells are circles: the outline is then the circumscribed polygon.
    pub round: bool,
    step: f64,
    ndir: usize,
    keys: Vec<KeyData>,
}

impl Lattice {
    pub fn new(
        shape: CellShape,
        size: f64,
        web: f64,
        grid: DotGrid,
        sides: u32,
    ) -> Result<Lattice, CellGridError> {
        let (s, w) = (size, web);
        let mut p = s + w;
        let r_in = s / 2.0;
        let (kind, step, ndir, cell_area, round, outlines): (
            Kind,
            f64,
            usize,
            f64,
            bool,
            Vec<Vec<Vec2>>,
        ) = match shape {
            CellShape::Hexagon => {
                let r = s / SQRT3;
                let hex = (0..6)
                    .map(|k| {
                        Vec2::new(
                            r * (f64::from(k) * FRAC_PI_3).cos(),
                            r * (f64::from(k) * FRAC_PI_3).sin(),
                        )
                    })
                    .collect();
                (
                    Kind::Hex {
                        h: p * SQRT3 / 2.0,
                        r,
                    },
                    FRAC_PI_6,
                    6,
                    SQRT3 / 2.0 * s * s,
                    false,
                    vec![hex],
                )
            }
            CellShape::Square => {
                let sq = vec![
                    Vec2::new(-r_in, -r_in),
                    Vec2::new(r_in, -r_in),
                    Vec2::new(r_in, r_in),
                    Vec2::new(-r_in, r_in),
                ];
                (Kind::Square, FRAC_PI_4, 4, s * s, false, vec![sq])
            }
            CellShape::Triangle => {
                let rt = r_in + w / 2.0;
                let up = vec![
                    Vec2::new(-SQRT3 * r_in, -r_in),
                    Vec2::new(SQRT3 * r_in, -r_in),
                    Vec2::new(0.0, 2.0 * r_in),
                ];
                let down = vec![
                    Vec2::new(SQRT3 * r_in, r_in),
                    Vec2::new(-SQRT3 * r_in, r_in),
                    Vec2::new(0.0, -2.0 * r_in),
                ];
                (
                    Kind::Triangle {
                        r: r_in,
                        rt,
                        l: 2.0 * SQRT3 * rt,
                        ht: 3.0 * rt,
                    },
                    FRAC_PI_3,
                    3,
                    3.0 * SQRT3 * r_in * r_in,
                    false,
                    vec![up, down],
                )
            }
            CellShape::Circle | CellShape::Polygon => {
                let (poly, area, round) = if shape == CellShape::Circle {
                    (regular_polygon(r_in, CIRCLE_SIDES), PI * r_in * r_in, true)
                } else {
                    if !(3..=MAX_SIDES).contains(&sides) {
                        return Err(CellGridError::Parameter(format!(
                            "a polygon cell has 3 to {MAX_SIDES} sides, not {sides}"
                        )));
                    }
                    let poly = regular_polygon(r_in, sides as usize);
                    let area = poly_area(&poly);
                    (poly, area, false)
                };
                let rc = poly.iter().map(|q| q.length()).fold(0.0, f64::max);
                if !round {
                    p = polygon_pitch(&poly, grid, w);
                }
                (
                    Kind::Dot { grid, rc },
                    FRAC_PI_6,
                    6,
                    area,
                    round,
                    vec![poly],
                )
            }
        };
        let mut lat = Lattice {
            kind,
            s,
            w,
            p,
            cell_area,
            round,
            step,
            ndir,
            keys: Vec::new(),
        };
        let mut keys = Vec::new();
        for outline in outlines {
            let chords = lat.chord_lines();
            let tiles = tiles_of(&outline, &chords);
            let candidates = candidates_of(&outline, &chords);
            keys.push(KeyData {
                outline,
                tiles,
                candidates,
            });
        }
        lat.keys = keys;
        Ok(lat)
    }

    /// Whether field edges can move onto lattice lines (the tilings).
    pub fn snappable(&self) -> bool {
        !matches!(self.kind, Kind::Dot { .. })
    }

    #[cfg(test)]
    /// The pitch between rows of the triangle tiling, `3 * rt` (zero for the other lattices).
    pub fn row_height(&self) -> f64 {
        match self.kind {
            Kind::Triangle { ht, .. } => ht,
            _ => 0.0,
        }
    }

    pub fn center(&self, i: i64, j: i64) -> Vec2 {
        let (fi, fj) = (i as f64, j as f64);
        match self.kind {
            Kind::Hex { h, .. } => Vec2::new(fi * h, fi * self.p / 2.0 + fj * self.p),
            Kind::Triangle { rt, l, ht, .. } => Vec2::new(
                fi * l / 2.0,
                fj * ht + if ((i ^ j) & 1) == 1 { rt } else { 0.0 },
            ),
            Kind::Square
            | Kind::Dot {
                grid: DotGrid::Square,
                ..
            } => Vec2::new(fi * self.p, fj * self.p),
            Kind::Dot {
                grid: DotGrid::Hexagonal,
                ..
            } => Vec2::new(fi * self.p * SQRT3 / 2.0, fi * self.p / 2.0 + fj * self.p),
        }
    }

    /// Which outline cell `(i, j)` uses (cells differ only by orientation, as triangles do).
    pub fn key(&self, i: i64, j: i64) -> usize {
        match self.kind {
            Kind::Triangle { .. } => ((i ^ j) & 1) as usize,
            _ => 0,
        }
    }

    #[cfg(test)]
    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    /// The cell's polygon (counter-clockwise) at the origin.
    pub fn outline(&self, key: usize) -> &[Vec2] {
        self.keys.get(key).map_or(&[], |k| k.outline.as_slice())
    }

    #[cfg(test)]
    pub fn cell(&self, i: i64, j: i64) -> Vec<Vec2> {
        let c = self.center(i, j);
        self.outline(self.key(i, j))
            .iter()
            .map(|&q| q + c)
            .collect()
    }

    /// The faces the chords cut a cell into.
    pub fn tiles(&self, key: usize) -> &[Vec<Vec2>] {
        self.keys.get(key).map_or(&[], |k| k.tiles.as_slice())
    }

    /// Single-chord pieces of a cell, the whole cell first and then the largest first.
    pub fn candidates(&self, key: usize) -> &[(PieceKind, Vec<Vec2>)] {
        self.keys.get(key).map_or(&[], |k| k.candidates.as_slice())
    }

    /// The lines through two non-adjacent vertices of a cell, as `(normal, offset)`.
    #[cfg(test)]
    pub fn chords(&self, _key: usize) -> Vec<(Vec2, f64)> {
        self.chord_lines()
    }

    fn chord_lines(&self) -> Vec<(Vec2, f64)> {
        match self.kind {
            Kind::Hex { r, .. } => {
                let mut out = Vec::new();
                for k in 0..6 {
                    let nrm = self.normal(k);
                    if k % 2 == 0 {
                        out.push((nrm, 0.0));
                    } else {
                        out.push((nrm, r / 2.0));
                        out.push((nrm, -r / 2.0));
                    }
                }
                out
            }
            Kind::Square => vec![(self.normal(1), 0.0), (self.normal(3), 0.0)],
            _ => Vec::new(),
        }
    }

    /// The `(i, j)` of every lattice cell whose bounding circle meets `b` (a superset is fine),
    /// or an error when there are more than [`MAX_CELLS`].
    pub fn cells_in(&self, b: &Bounds) -> Result<Vec<(i64, i64)>, CellGridError> {
        let too_many = |n: f64| CellGridError::TooManyCells {
            cells: if n.is_finite() && n < u64::MAX as f64 {
                n as u64
            } else {
                u64::MAX
            },
            max: MAX_CELLS,
        };
        // Index ranges as floats first, so a huge field is refused before any integer overflows.
        let range = |lo: f64, hi: f64| (lo.ceil(), hi.floor());
        let mut out: Vec<(i64, i64)> = Vec::new();
        let (x0, y0, x1, y1) = (b.x0, b.y0, b.x1, b.y1);
        match self.kind {
            Kind::Hex { h, r } => {
                let (i0, i1) = range((x0 - r) / h, (x1 + r) / h);
                let columns = i1 - i0 + 1.0;
                let rows = (y1 - y0 + self.s) / self.p + 2.0;
                if !(columns * rows <= MAX_CELLS as f64 * 4.0) {
                    return Err(too_many(columns * rows));
                }
                let mut i = i0;
                while i <= i1 {
                    let base = i * self.p / 2.0;
                    let (j0, j1) = range(
                        (y0 - self.s / 2.0 - base) / self.p,
                        (y1 + self.s / 2.0 - base) / self.p,
                    );
                    let mut j = j0;
                    while j <= j1 {
                        out.push((i as i64, j as i64));
                        if out.len() > MAX_CELLS {
                            return Err(too_many(out.len() as f64));
                        }
                        j += 1.0;
                    }
                    i += 1.0;
                }
            }
            Kind::Square
            | Kind::Dot {
                grid: DotGrid::Square,
                ..
            } => {
                let a = match self.kind {
                    Kind::Dot { rc, .. } => rc,
                    _ => self.s / 2.0,
                };
                let (i0, i1) = range((x0 - a) / self.p, (x1 + a) / self.p);
                let (j0, j1) = range((y0 - a) / self.p, (y1 + a) / self.p);
                let n = (i1 - i0 + 1.0).max(0.0) * (j1 - j0 + 1.0).max(0.0);
                if !(n <= MAX_CELLS as f64) {
                    return Err(too_many(n));
                }
                let mut i = i0;
                while i <= i1 {
                    let mut j = j0;
                    while j <= j1 {
                        out.push((i as i64, j as i64));
                        j += 1.0;
                    }
                    i += 1.0;
                }
            }
            Kind::Dot {
                grid: DotGrid::Hexagonal,
                rc,
            } => {
                let dx = self.p * SQRT3 / 2.0;
                let (i0, i1) = range((x0 - rc) / dx, (x1 + rc) / dx);
                let columns = i1 - i0 + 1.0;
                let rows = (y1 - y0 + 2.0 * rc) / self.p + 2.0;
                if !(columns * rows <= MAX_CELLS as f64 * 4.0) {
                    return Err(too_many(columns * rows));
                }
                let mut i = i0;
                while i <= i1 {
                    let base = i * self.p / 2.0;
                    let (j0, j1) = range((y0 - rc - base) / self.p, (y1 + rc - base) / self.p);
                    let mut j = j0;
                    while j <= j1 {
                        out.push((i as i64, j as i64));
                        if out.len() > MAX_CELLS {
                            return Err(too_many(out.len() as f64));
                        }
                        j += 1.0;
                    }
                    i += 1.0;
                }
            }
            Kind::Triangle { r, rt, l, ht } => {
                let rc = 2.0 * r;
                let (j0, j1) = range((y0 - rc - rt) / ht, (y1 + rc) / ht);
                let (i0, i1) = range((x0 - rc) / (l / 2.0), (x1 + rc) / (l / 2.0));
                let n = (j1 - j0 + 1.0).max(0.0) * (i1 - i0 + 1.0).max(0.0);
                if !(n <= MAX_CELLS as f64) {
                    return Err(too_many(n));
                }
                let mut j = j0;
                while j <= j1 {
                    let mut i = i0;
                    while i <= i1 {
                        out.push((i as i64, j as i64));
                        i += 1.0;
                    }
                    j += 1.0;
                }
            }
        }
        if out.len() > MAX_CELLS {
            return Err(too_many(out.len() as f64));
        }
        Ok(out)
    }

    // --- lattice lines ---

    /// How many line directions there are (`2 * ndir`: both signs of each).
    #[cfg(test)]
    pub fn direction_count(&self) -> usize {
        2 * self.ndir
    }

    /// `k` in `0..2*ndir` for an edge direction within [`ANGLE_TOL`] of `k * step`, else `None`.
    pub fn direction_index(&self, angle: f64) -> Option<usize> {
        let k = (angle / self.step).round();
        if !(k.is_finite() && (angle - k * self.step).abs() <= ANGLE_TOL) {
            return None;
        }
        Some((k as i64).checked_rem_euclid(2 * self.ndir as i64)? as usize)
    }

    /// The left normal of direction `k * step`.
    pub fn normal(&self, k: usize) -> Vec2 {
        let a = k as f64 * self.step;
        Vec2::new(-a.sin(), a.cos())
    }

    fn base_family(&self, k: usize) -> Family {
        match self.kind {
            Kind::Hex { h, r } => {
                if k % 2 == 0 {
                    Family::one(self.p / 2.0, 0.0)
                } else {
                    Family::two(h, r / 2.0, -r / 2.0)
                }
            }
            Kind::Square => {
                if k % 2 == 0 {
                    Family::two(self.p, self.s / 2.0, -self.s / 2.0)
                } else {
                    Family::one(self.p / SQRT_2, 0.0)
                }
            }
            Kind::Triangle { rt, ht, .. } => {
                let t = if k == 1 { rt } else { -rt };
                Family::two(ht, t - self.w / 2.0, t + self.w / 2.0)
            }
            Kind::Dot { .. } => Family::one(self.p, 0.0),
        }
    }

    /// The offsets of the lattice lines along `normal(k)`, for any `k`.
    pub fn family(&self, k: usize) -> Family {
        let k = k.checked_rem(2 * self.ndir).unwrap_or(0);
        let fam = self.base_family(k.checked_rem(self.ndir).unwrap_or(0));
        // The opposite direction has the negated normal.
        if k >= self.ndir {
            fam.negated()
        } else {
            fam
        }
    }

    /// The smallest lattice-line offset at least `c - tol` for direction `k`.
    pub fn snap_up(&self, k: usize, c: f64, tol: f64) -> f64 {
        let fam = self.family(k);
        fam.phases()
            .iter()
            .map(|&ph| ph + fam.period * ((c - ph - tol) / fam.period).ceil())
            .fold(f64::INFINITY, f64::min)
    }

    pub fn on_line(&self, k: usize, c: f64, tol: f64) -> bool {
        let fam = self.family(k);
        fam.phases()
            .iter()
            .any(|&ph| (ph + ((c - ph) / fam.period).round() * fam.period - c).abs() <= tol)
    }
}

/// The convex faces the chords cut `outline` into.
fn tiles_of(outline: &[Vec2], chords: &[(Vec2, f64)]) -> Vec<Vec<Vec2>> {
    let area = poly_area(outline);
    let mut faces = vec![outline.to_vec()];
    for &(nrm, c) in chords {
        let mut next = Vec::new();
        for f in &faces {
            for part in [clip_halfplane(f, nrm, c), clip_halfplane(f, -nrm, -c)] {
                if part.len() >= 3 && poly_area(&part) > 1e-12 * area {
                    next.push(part);
                }
            }
        }
        faces = next;
    }
    faces
}

fn candidates_of(outline: &[Vec2], chords: &[(Vec2, f64)]) -> Vec<(PieceKind, Vec<Vec2>)> {
    let area = poly_area(outline);
    let mut pieces = Vec::new();
    for &(nrm, c) in chords {
        for part in [
            clip_halfplane(outline, nrm, c),
            clip_halfplane(outline, -nrm, -c),
        ] {
            if part.len() >= 3 {
                pieces.push(part);
            }
        }
    }
    // Largest first; the sort is stable, so equal pieces keep their chord order.
    pieces.sort_by(|a, b| poly_area(b).total_cmp(&poly_area(a)));
    let mut out = vec![(PieceKind::Whole, outline.to_vec())];
    for q in pieces {
        let frac = poly_area(&q) / area;
        let kind = [
            (PieceKind::Pentagon, 5.0 / 6.0),
            (PieceKind::Half, 0.5),
            (PieceKind::Triangle, 1.0 / 6.0),
        ]
        .into_iter()
        .find(|&(_, f)| (frac - f).abs() < 1e-9)
        .map_or(PieceKind::Part, |(k, _)| k);
        out.push((kind, q));
    }
    out
}
