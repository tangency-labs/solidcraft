//! Tests of the cell grid planner: the lattices, snapping, the three clip modes, alignment of
//! two fields and hostile input.

#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp
)]

use crate::math::det;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_6, PI};

use crate::math::Vec2;
use crate::prism::profile::{Loop2, Seg2};

use crate::cell_grid::lattice::{Lattice, CIRCLE_SIDES};
use crate::cell_grid::region::{
    clip_halfplane, poly_area, Bounds, Convex, Loop, Prim, Region, EPS,
};
use crate::cell_grid::snap::{piece_hits, simple, snap_loop as snap_loop_by};
use crate::cell_grid::vec::{ring_next, ring_prev, Vec2Ext};
use crate::cell_grid::*;
use crate::prism::profile::cross2;

/// A vector turned a quarter turn counter-clockwise (tests only).
trait Perp {
    fn perp(self) -> Vec2;
}
impl Perp for Vec2 {
    fn perp(self) -> Vec2 {
        Vec2::new(-self.y, self.x)
    }
}

const S: f64 = 5.0;
const W: f64 = 1.2;
const P: f64 = S + W;
const SQRT3: f64 = 1.732_050_807_568_877_2;
const H: f64 = P * SQRT3 / 2.0;
const R: f64 = S / SQRT3;
const CELL_AREA: f64 = SQRT3 / 2.0 * S * S;

/// A field whose edges all lie on lattice lines (anchor at the origin, angle 0): columns 0 and 4
/// are cut to 5/6 by their outer short chords, y = 0 and y = 4p pass through the centres of the
/// even columns.
const ALIGNED: (f64, f64, f64, f64) = (-R / 2.0, 0.0, 4.0 * H + R / 2.0, 4.0 * P);
/// What snapping that field gives: 11 whole cells and 12 partial ones (6 pentagons, 4 corner
/// quadrilaterals of 5/12 and 2 halves).
const ALIGNED_WHOLE: usize = 11;
const ALIGNED_PARTIAL: usize = 12;
const ALIGNED_AREA: f64 = CELL_AREA * (11.0 + 6.0 * 5.0 / 6.0 + 4.0 * 5.0 / 12.0 + 2.0 * 0.5);

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn polygon(pts: &[(f64, f64)]) -> Loop2 {
    let pts: Vec<Vec2> = pts.iter().map(|&(x, y)| v(x, y)).collect();
    let n = pts.len();
    Loop2 {
        segs: (0..n)
            .map(|k| Seg2::Line {
                a: pts[k],
                b: pts[(k + 1) % n],
            })
            .collect(),
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop2 {
    polygon(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
}

/// A circle as two half arcs, as a sketch draws one.
fn circle(cx: f64, cy: f64, r: f64) -> Loop2 {
    Loop2 {
        segs: vec![
            Seg2::Arc {
                center: v(cx, cy),
                radius: r,
                start: 0.0,
                sweep: PI,
            },
            Seg2::Arc {
                center: v(cx, cy),
                radius: r,
                start: PI,
                sweep: PI,
            },
        ],
    }
}

fn spec(shape: CellShape) -> CellGridSpec {
    CellGridSpec {
        shape,
        size: S,
        web: W,
        ..CellGridSpec::default()
    }
}

fn with_clip(shape: CellShape, clip: Clip) -> CellGridSpec {
    CellGridSpec {
        clip,
        ..spec(shape)
    }
}

fn run(field: &[Loop2], s: &CellGridSpec) -> CellGridPlan {
    plan(field, s).unwrap()
}

fn counts(p: &CellGridPlan) -> (usize, usize, usize) {
    (p.whole, p.partial, p.fallback)
}

fn lattice(shape: CellShape) -> Lattice {
    Lattice::new(shape, S, W, DotGrid::Hexagonal, 8).unwrap()
}

/// A loop of the lattice frame from corner points, in the order given.
fn frame_loop(pts: &[(f64, f64)]) -> Loop {
    let prims = (0..pts.len())
        .map(|k| {
            Prim::line(
                v(pts[k].0, pts[k].1),
                v(pts[(k + 1) % pts.len()].0, pts[(k + 1) % pts.len()].1),
            )
        })
        .collect();
    Loop::new(prims, None).unwrap()
}

fn frame_rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop {
    frame_loop(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
}

/// A small deterministic random source.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }

    fn int(&mut self, lo: usize, hi: usize) -> usize {
        lo + (self.next() * (hi - lo + 1) as f64) as usize % (hi - lo + 1)
    }

    /// `n` distinct values of `0..m`, in order.
    fn sample(&mut self, m: usize, n: usize) -> Vec<usize> {
        let mut all: Vec<usize> = (0..m).collect();
        for k in 0..n {
            let j = k + self.int(0, m - k - 1);
            all.swap(k, j);
        }
        let mut out = all[..n].to_vec();
        out.sort_unstable();
        out
    }
}

/// A convex polygon with edges at multiples of 360/`ndirs` degrees and random offsets.
fn random_polygon(rng: &mut Rng, ndirs: usize, size: f64, lo: usize, hi: usize) -> Vec<(f64, f64)> {
    let angles = loop {
        let count = rng.int(lo, hi);
        let a = rng.sample(ndirs, count);
        let gaps = (0..a.len()).map(|k| (a[(k + 1) % a.len()] + ndirs - a[k]) % ndirs);
        if 2 * gaps.max().unwrap() < ndirs {
            break a;
        }
    };
    let planes: Vec<((f64, f64), f64)> = angles
        .iter()
        .map(|&k| {
            let a = k as f64 * 2.0 * PI / ndirs as f64 + FRAC_PI_2;
            ((det::cos(a), det::sin(a)), rng.range(0.35, 0.5) * size)
        })
        .collect();
    // The planes cut out of a large box, so a plane that touches nothing adds no corner.
    let mut poly = vec![v(-1e4, -1e4), v(1e4, -1e4), v(1e4, 1e4), v(-1e4, 1e4)];
    for &((nx, ny), c) in &planes {
        poly = clip_halfplane(&poly, v(-nx, -ny), -c);
    }
    poly.iter().map(|q| (q.x, q.y)).collect()
}

/// Whether each edge of an opening lies on its cell's side or on one of its chords.
fn edges_on_cell_lines(lat: &Lattice, cell: &GridCell) -> bool {
    let c = lat.center(cell.i, cell.j);
    let key = lat.key(cell.i, cell.j);
    let outline = lat.outline(key);
    cell.loops.iter().all(|lp| {
        lp.segs.iter().all(|seg| {
            let Seg2::Line { a, b } = *seg else {
                return false;
            };
            let (la, lb) = (a - c, b - c);
            let on_chord = lat
                .chords(key)
                .iter()
                .any(|&(n, off)| (n.dot(la) - off).abs() < 1e-6 && (n.dot(lb) - off).abs() < 1e-6);
            let on_side = (0..outline.len()).any(|k| {
                let (p, q) = (outline[k], outline[(k + 1) % outline.len()]);
                let d = (q - p).normalized().unwrap();
                cross2(d, la - p).abs() < 1e-6 && cross2(d, lb - p).abs() < 1e-6
            });
            on_chord || on_side
        })
    })
}

fn vertices(cell: &GridCell) -> Vec<Vec2> {
    cell.loops
        .iter()
        .flat_map(|l| l.segs.iter().map(Seg2::start))
        .collect()
}

// --- the lattices ---

#[test]
fn a_hexagon_lattice_has_the_documented_dimensions() {
    let lat = lattice(CellShape::Hexagon);
    assert!((lat.p - 6.2).abs() < 1e-12);
    // Neighbours across a flat are p apart, so the flats are one web apart.
    let (a, b) = (lat.center(0, 0), lat.center(0, 1));
    assert!((b.y - a.y - S - W).abs() < 1e-12);
    let c = lat.center(1, 0);
    assert!((c.distance(a) - P).abs() < 1e-12);
    assert!((cell_center(&spec(CellShape::Hexagon), 1, 0).unwrap().x - H).abs() < 1e-12);
}

#[test]
fn a_short_diagonal_line_clears_the_next_column_by_the_web_times_root_three_over_two() {
    assert!((H - 1.5 * R - W * SQRT3 / 2.0).abs() < 1e-12);
}

#[test]
fn a_cell_size_or_web_that_is_zero_negative_or_not_a_number_is_refused() {
    for (size, web) in [
        (5.0, 0.0),
        (0.0, 1.0),
        (-5.0, 1.0),
        (5.0, -1.0),
        (f64::NAN, 1.0),
        (5.0, f64::NAN),
        (f64::INFINITY, 1.0),
        (5.0, f64::INFINITY),
        (1e-9, 1.0),
    ] {
        let s = CellGridSpec {
            size,
            web,
            ..CellGridSpec::default()
        };
        assert!(
            matches!(
                plan(&[rect(0.0, 0.0, 10.0, 10.0)], &s),
                Err(CellGridError::Parameter(_))
            ),
            "size {size} web {web}"
        );
    }
}

#[test]
fn an_angle_or_anchor_that_is_not_finite_is_refused() {
    for s in [
        CellGridSpec {
            angle: f64::NAN,
            ..CellGridSpec::default()
        },
        CellGridSpec {
            angle: f64::INFINITY,
            ..CellGridSpec::default()
        },
        CellGridSpec {
            anchor: v(f64::NAN, 0.0),
            ..CellGridSpec::default()
        },
        CellGridSpec {
            anchor: v(0.0, f64::INFINITY),
            ..CellGridSpec::default()
        },
        CellGridSpec {
            anchor: v(1e300, 0.0),
            ..CellGridSpec::default()
        },
    ] {
        assert!(plan(&[rect(0.0, 0.0, 10.0, 10.0)], &s).is_err());
        assert!(cell_center(&s, 0, 0).is_err());
    }
}

#[test]
fn a_polygon_cell_needs_three_to_sixty_four_sides() {
    for sides in [0, 1, 2, 65, u32::MAX] {
        let s = CellGridSpec {
            shape: CellShape::Polygon,
            sides,
            ..CellGridSpec::default()
        };
        assert!(
            plan(&[rect(0.0, 0.0, 10.0, 10.0)], &s).is_err(),
            "{sides} sides"
        );
    }
    for sides in [3, 5, 64] {
        let s = CellGridSpec {
            shape: CellShape::Polygon,
            sides,
            ..CellGridSpec::default()
        };
        assert!(
            plan(&[rect(0.0, 0.0, 30.0, 30.0)], &s).is_ok(),
            "{sides} sides"
        );
    }
}

#[test]
fn a_hexagon_cell_is_cut_into_24_faces_by_its_chords() {
    let lat = lattice(CellShape::Hexagon);
    let tiles = lat.tiles(0);
    assert_eq!(tiles.len(), 24);
    assert!((tiles.iter().map(|t| poly_area(t)).sum::<f64>() - CELL_AREA).abs() < 1e-9);
}

#[test]
fn a_hexagon_cell_has_a_whole_piece_and_six_each_of_pentagons_halves_and_triangles() {
    use crate::cell_grid::lattice::PieceKind::*;
    let lat = lattice(CellShape::Hexagon);
    let cands = lat.candidates(0);
    assert_eq!(cands.iter().filter(|c| c.0 == Whole).count(), 1);
    assert_eq!(cands.first().map(|c| c.0), Some(Whole));
    for (kind, frac) in [(Pentagon, 5.0 / 6.0), (Half, 0.5), (Triangle, 1.0 / 6.0)] {
        let of_kind: Vec<_> = cands.iter().filter(|c| c.0 == kind).collect();
        assert_eq!(of_kind.len(), 6);
        for (_, poly) in of_kind {
            assert!((poly_area(poly) / CELL_AREA - frac).abs() < 1e-9);
        }
    }
}

#[test]
fn every_lattice_line_meets_a_cell_along_a_chord_through_two_vertices_or_misses_it() {
    for shape in [CellShape::Hexagon, CellShape::Square, CellShape::Triangle] {
        let lat = lattice(shape);
        let cells: Vec<Vec<Vec2>> = (-4..=4)
            .flat_map(|i| (-4..=4).map(move |j| (i, j)))
            .map(|(i, j)| lat.cell(i, j))
            .collect();
        for k in 0..lat.direction_count() {
            let fam = lat.family(k);
            let nrm = lat.normal(k);
            for m in -4..=4 {
                for &ph in fam.phases() {
                    let c = ph + f64::from(m) * fam.period;
                    for cell in &cells {
                        let d: Vec<f64> = cell.iter().map(|q| nrm.dot(*q) - c).collect();
                        let (lo, hi) = (
                            d.iter().copied().fold(f64::INFINITY, f64::min),
                            d.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                        );
                        if lo < -1e-9 && hi > 1e-9 {
                            assert_eq!(
                                d.iter().filter(|x| x.abs() < 1e-9).count(),
                                2,
                                "{shape:?}: line k={k} c={c} cuts off a chord"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn the_chords_and_for_squares_and_triangles_the_sides_of_a_cell_are_lattice_lines() {
    for shape in [CellShape::Hexagon, CellShape::Square, CellShape::Triangle] {
        let lat = lattice(shape);
        for i in -2..=2 {
            for j in -2..=2 {
                let key = lat.key(i, j);
                let c = lat.center(i, j);
                let mut lines: Vec<(Vec2, f64)> = lat.chords(key);
                if shape != CellShape::Hexagon {
                    let o = lat.outline(key);
                    for k in 0..o.len() {
                        let (p, q) = (o[k], o[(k + 1) % o.len()]);
                        let d = (q - p).normalized().unwrap();
                        let nrm = d.perp();
                        lines.push((nrm, nrm.dot(p)));
                    }
                }
                for (nrm, off) in lines {
                    let k = lat.direction_index(det::atan2(-nrm.x, nrm.y)).unwrap();
                    assert!(
                        lat.on_line(k, off + nrm.dot(c), 1e-9),
                        "{shape:?} cell ({i}, {j}): a side or chord is off the lattice"
                    );
                }
            }
        }
    }
}

fn poly_gap(a: &[Vec2], b: &[Vec2]) -> f64 {
    let to_seg = |q: Vec2, u: Vec2, w: Vec2| {
        let d = w - u;
        let t = ((q - u).dot(d) / d.dot(d)).clamp(0.0, 1.0);
        q.distance(u + d * t)
    };
    let mut best = f64::INFINITY;
    for (p, q) in [(a, b), (b, a)] {
        for &pt in p {
            for k in 0..q.len() {
                best = best.min(to_seg(pt, q[k], q[(k + 1) % q.len()]));
            }
        }
    }
    best
}

#[test]
fn neighbouring_cells_of_every_tiling_are_exactly_one_web_apart() {
    for shape in [CellShape::Hexagon, CellShape::Square, CellShape::Triangle] {
        let lat = lattice(shape);
        let gaps = (-2..=2)
            .flat_map(|i| (-2..=2).map(move |j| (i, j)))
            .filter(|&ij| ij != (0, 0))
            .map(|(i, j)| poly_gap(&lat.cell(0, 0), &lat.cell(i, j)));
        let min = gaps.fold(f64::INFINITY, f64::min);
        assert!((min - W).abs() < 1e-9, "{shape:?}: {min}");
    }
}

#[test]
fn the_tiles_of_every_tiling_cell_cover_it() {
    for shape in [CellShape::Hexagon, CellShape::Square, CellShape::Triangle] {
        let lat = lattice(shape);
        for key in 0..lat.key_count() {
            assert!(
                (lat.tiles(key).iter().map(|t| poly_area(t)).sum::<f64>() - lat.cell_area).abs()
                    < 1e-9
            );
        }
    }
}

#[test]
fn a_square_cell_has_four_tiles_and_four_half_pieces() {
    use crate::cell_grid::lattice::PieceKind::*;
    let lat = lattice(CellShape::Square);
    assert_eq!(lat.tiles(0).len(), 4);
    assert_eq!(
        lat.candidates(0).iter().map(|c| c.0).collect::<Vec<_>>(),
        [Whole, Half, Half, Half, Half]
    );
}

#[test]
fn a_triangle_cell_is_only_ever_whole_and_alternates_up_and_down() {
    let lat = lattice(CellShape::Triangle);
    assert_eq!(lat.tiles(0).len(), 1);
    assert_eq!(lat.candidates(1).len(), 1);
    assert_eq!(
        (lat.key(0, 0), lat.key(1, 0), lat.key(0, 1), lat.key(-1, 0)),
        (0, 1, 1, 1)
    );
    assert!((lat.cell_area - 3.0 * SQRT3 * det::powi(S / 2.0, 2)).abs() < 1e-12);
    for key in 0..2 {
        // The inscribed circle of each cell has diameter S.
        let o = lat.outline(key);
        for k in 0..o.len() {
            let (p, q) = (o[k], o[(k + 1) % o.len()]);
            let dist = cross2((q - p).normalized().unwrap(), p).abs();
            assert!((dist - S / 2.0).abs() < 1e-12);
        }
    }
}

#[test]
fn circles_on_either_grid_are_a_web_apart_and_polygons_have_flats_a_web_apart_on_the_square_grid() {
    for grid in [DotGrid::Hexagonal, DotGrid::Square] {
        let lat = Lattice::new(CellShape::Circle, S, W, grid, 8).unwrap();
        let c0 = lat.center(0, 0);
        let nearest = (-2..=2)
            .flat_map(|i| (-2..=2).map(move |j| (i, j)))
            .filter(|&ij| ij != (0, 0))
            .map(|(i, j)| c0.distance(lat.center(i, j)))
            .fold(f64::INFINITY, f64::min);
        assert!((nearest - P).abs() < 1e-12, "{grid:?}");
        assert!((lat.cell_area - PI * det::powi(S / 2.0, 2)).abs() < 1e-12);
        assert!(!lat.snappable());
        assert_eq!(lat.outline(0).len(), CIRCLE_SIDES);
    }
    let oct = Lattice::new(CellShape::Polygon, S, W, DotGrid::Square, 8).unwrap();
    assert_eq!(oct.outline(0).len(), 8);
    assert!((poly_gap(&oct.cell(0, 0), &oct.cell(1, 0)) - W).abs() < 1e-9);
}

// --- plane geometry ---

#[test]
fn a_region_nests_fields_keep_outs_and_islands_by_depth() {
    let loops = [
        rect(0.0, 0.0, 10.0, 10.0),
        rect(2.0, 2.0, 8.0, 8.0),
        rect(4.0, 4.0, 6.0, 6.0),
    ]
    .map(|l| {
        let prims = l
            .segs
            .iter()
            .map(|s| Prim::line(s.start(), s.end()))
            .collect();
        Loop::new(prims, None).unwrap()
    });
    let reg = Region::from_loops(loops.to_vec(), 5.0);
    assert_eq!(reg.parts.len(), 2);
    assert!(reg.contains(v(1.0, 1.0)));
    assert!(!reg.contains(v(3.0, 3.0)));
    assert!(reg.contains(v(5.0, 5.0)));
    assert!(!reg.contains(v(11.0, 5.0)));
}

fn arc_loop(c: Vec2, r: f64) -> Loop {
    Loop::new(
        vec![Prim::arc(c, r, 0.0, PI), Prim::arc(c, r, PI, PI)],
        None,
    )
    .unwrap()
}

#[test]
fn a_circle_contains_the_points_inside_it() {
    let lp = arc_loop(v(0.0, 0.0), 2.0);
    for (p, inside) in [
        ((0.0, 0.0), true),
        ((1.9, 0.0), true),
        ((0.0, -1.99), true),
        ((1.5, 1.5), false),
        ((0.0, 2.01), false),
    ] {
        assert_eq!(lp.contains(v(p.0, p.1)), inside, "{p:?}");
    }
    assert!((lp.signed_area() - 4.0 * PI).abs() < 1e-9);
}

#[test]
fn a_circle_whose_centre_is_off_by_float_noise_still_contains_the_points_at_its_seam_height() {
    let r = 26.4;
    for cy in [
        0.0,
        1e-15,
        -1e-15,
        3.552713678800501e-15,
        -3.55e-15,
        1e-13,
        -1e-12,
        1e-12,
    ] {
        let lp = arc_loop(v(0.0, cy), r);
        for dy in [
            0.0, 1e-16, -1e-16, 1e-15, -1e-15, 7e-15, -7e-15, 1e-12, -1e-12,
        ] {
            for x in [-r + 1e-6, -20.0, 0.0, 20.0, r - 1e-6] {
                assert!(lp.contains(v(x, cy + dy)), "cy {cy:e} dy {dy:e} x {x}");
            }
            for x in [-r - 1e-6, r + 1e-6] {
                assert!(!lp.contains(v(x, cy + dy)));
            }
        }
    }
}

#[test]
fn the_joints_of_a_rounded_rectangle_are_welded_to_one_exact_point() {
    let (r, q, noise) = (3.0, FRAC_PI_2, 1e-9);
    let prims = vec![
        Prim::line(v(r, 0.0), v(20.0 - r, 0.0)),
        Prim::arc(v(20.0 - r, r), r, -q, q),
        Prim::line(v(20.0, r + noise), v(20.0, 10.0 - r)),
        Prim::arc(v(20.0 - r, 10.0 - r), r, 0.0, q),
        Prim::line(v(20.0 - r - noise, 10.0), v(r, 10.0)),
        Prim::arc(v(r, 10.0 - r), r, q, q),
        Prim::line(v(0.0, 10.0 - r), v(0.0, r - noise)),
        Prim::arc(v(r, r), r, PI, q),
    ];
    let lp = Loop::new(prims, None).unwrap();
    let n = lp.prims.len();
    for k in 0..n {
        assert_eq!(lp.prims[k].start(), lp.prims[ring_prev(k, n)].end());
    }
    for y in [
        r,
        r + 1e-15,
        r - 1e-15,
        10.0 - r,
        10.0 - r + 1e-15,
        10.0 - r - 1e-15,
    ] {
        assert!(lp.contains(v(10.0, y)) && lp.contains(v(0.5, y)));
        assert!(!lp.contains(v(-0.5, y)) && !lp.contains(v(20.5, y)));
    }
}

#[test]
fn an_arc_meets_a_convex_polygon_only_where_it_passes_through_it() {
    let arc = Prim::arc(v(0.0, 0.0), 2.0, 0.0, FRAC_PI_2);
    let sq = |x0: f64, y0: f64, x1: f64, y1: f64| {
        Convex::new(vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)]).unwrap()
    };
    assert!(arc.meets_convex(&sq(1.0, 1.0, 3.0, 3.0)));
    assert!(!arc.meets_convex(&sq(-3.0, -3.0, -1.0, -1.0)));
    assert!(!arc.meets_convex(&sq(-0.5, -0.5, 0.5, 0.5)));
}

// --- snapping ---

/// A loop snapped with no inset.
fn snap_loop(lp: &Loop, lat: &Lattice) -> Result<Loop, &'static str> {
    snap_loop_by(lp, lat, 0)
}

fn aligned_loop() -> Loop {
    frame_rect(ALIGNED.0, ALIGNED.1, ALIGNED.2, ALIGNED.3)
}

#[test]
fn a_loop_on_lattice_lines_stays_where_it_is() {
    let lat = lattice(CellShape::Hexagon);
    let snapped = snap_loop(&aligned_loop(), &lat).unwrap();
    let b = snapped.bounds();
    for (got, want) in [
        (b.x0, ALIGNED.0),
        (b.y0, ALIGNED.1),
        (b.x1, ALIGNED.2),
        (b.y1, ALIGNED.3),
    ] {
        assert!((got - want).abs() < 1e-9);
    }
}

#[test]
fn a_field_snaps_inwards_and_a_keep_out_snaps_outwards() {
    let lat = lattice(CellShape::Hexagon);
    let (x0, y0, x1, y1) = ALIGNED;
    let b = snap_loop(&frame_rect(x0 - 0.4, y0 - 0.4, x1 + 0.4, y1 + 0.4), &lat)
        .unwrap()
        .bounds();
    for (got, want) in [(b.x0, x0), (b.y0, y0), (b.x1, x1), (b.y1, y1)] {
        assert!((got - want).abs() < 1e-9);
    }
    // A keep-out (clockwise: the region is outside it) grows onto the next lines out.
    let hole = frame_rect(10.0, 10.0, 12.0, 11.0).oriented(false);
    let b = snap_loop(&hole, &lat).unwrap().bounds();
    assert!(b.x0 <= 10.0 && b.x1 >= 12.0);
    assert!((b.y0 - 9.3).abs() < 1e-9 && (b.y1 - 12.4).abs() < 1e-9);
}

#[test]
fn a_loop_with_an_arc_or_an_off_direction_edge_cannot_snap() {
    let lat = lattice(CellShape::Hexagon);
    assert_eq!(
        snap_loop(&arc_loop(v(0.0, 0.0), 3.0), &lat).err(),
        Some("has curves")
    );
    let tilted = frame_loop(&[(0.0, 0.0), (10.0, 1.0), (9.0, 11.0), (-1.0, 10.0)]);
    assert_eq!(
        snap_loop(&tilted, &lat).err(),
        Some("edge off the lattice directions")
    );
}

/// The issue's field: x from -45.5 to 47.5, y from -24.5 to 24.5, with a round keep-out of
/// diameter 14.2 centred at (-39.5, 0) that crosses the left edge and merges into the outer
/// loop (four lines and an arc). The hexagon lattice has size 10, web 2 and runs at 90 degrees,
/// so its vertical lattice lines are at x = 6k and the right edge should snap to x = 42.
fn notched_field_spec() -> CellGridSpec {
    CellGridSpec {
        shape: CellShape::Hexagon,
        size: 10.0,
        web: 2.0,
        angle: FRAC_PI_2,
        clip: Clip::Snap,
        ..CellGridSpec::default()
    }
}

fn notched_field() -> Loop2 {
    let (cx, r) = (-39.5, 7.1);
    let y = (r * r - 6.0 * 6.0_f64).sqrt();
    let a = det::atan2(y, -6.0);
    Loop2 {
        segs: vec![
            Seg2::Line {
                a: v(-45.5, -24.5),
                b: v(47.5, -24.5),
            },
            Seg2::Line {
                a: v(47.5, -24.5),
                b: v(47.5, 24.5),
            },
            Seg2::Line {
                a: v(47.5, 24.5),
                b: v(-45.5, 24.5),
            },
            Seg2::Line {
                a: v(-45.5, 24.5),
                b: v(-45.5, y),
            },
            Seg2::Arc {
                center: v(cx, 0.0),
                radius: r,
                start: a,
                sweep: -2.0 * a,
            },
            Seg2::Line {
                a: v(-45.5, -y),
                b: v(-45.5, -24.5),
            },
        ],
    }
}

#[test]
fn an_arc_in_the_outer_loop_does_not_stop_its_straight_edges_snapping() {
    let p = run(&[notched_field()], &notched_field_spec());
    let right = p
        .cells
        .iter()
        .flat_map(vertices)
        .map(|q| q.x)
        .fold(f64::NEG_INFINITY, f64::max);
    // The right edge snaps to the lattice line x = 42: no cell reaches past it. Unsnapped, the
    // whole cells flush against x = 47 were kept.
    assert!(right <= 42.0 + 1e-6, "rightmost cell wall at {right}");
    assert!(p
        .cells
        .iter()
        .flat_map(vertices)
        .any(|q| (q.x - 42.0).abs() < 1e-6));
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
}

#[test]
fn a_cubic_keep_out_does_not_stop_an_arc_field_snapping() {
    // The notched field has an exact arc; the keep-out inside it is a cubic loop, which is
    // flattened. Only the keep-out is followed as drawn: the field's straight edges still snap.
    let keep_out = Loop2 {
        segs: vec![
            Seg2::Cubic {
                p0: v(6.0, 0.0),
                p1: v(6.0, 6.0),
                p2: v(14.0, 6.0),
                p3: v(14.0, 0.0),
            },
            Seg2::Cubic {
                p0: v(14.0, 0.0),
                p1: v(14.0, -6.0),
                p2: v(6.0, -6.0),
                p3: v(6.0, 0.0),
            },
        ],
    };
    let p = run(&[notched_field(), keep_out], &notched_field_spec());
    let right = p
        .cells
        .iter()
        .flat_map(vertices)
        .map(|q| q.x)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(right <= 42.0 + 1e-6, "rightmost cell wall at {right}");
    assert!(
        p.warnings.iter().any(|w| w.contains("followed")),
        "{:?}",
        p.warnings
    );
}

#[test]
fn snapping_a_loop_with_an_arc_keeps_the_arc_and_moves_its_ends_onto_the_snapped_edges() {
    let lat = Lattice::new(CellShape::Hexagon, 10.0, 2.0, DotGrid::Hexagonal, 8).unwrap();
    let spec = notched_field_spec();
    let lp = lattice_loops(&[notched_field()], &spec, &mut Vec::new())
        .unwrap()
        .0;
    let snapped = snap_loop(&lp[0], &lat).unwrap();
    let arc = snapped
        .prims
        .iter()
        .find_map(|p| match *p {
            Prim::Arc { r, c, p0, p1, .. } => Some((r, c, p0, p1)),
            Prim::Line { .. } => None,
        })
        .unwrap();
    assert!((arc.0 - 7.1).abs() < 1e-9);
    // Both ends are still on the circle, and the ring stays closed.
    for end in [arc.2, arc.3] {
        assert!((end.distance(arc.1) - 7.1).abs() < 1e-9);
    }
    let n = snapped.prims.len();
    for k in 0..n {
        assert!(
            snapped.prims[k]
                .end()
                .distance(snapped.prims[(k + 1) % n].start())
                < 1e-9
        );
    }
}

/// Whether the straight piece `line` meets `arc` cut into chords of pi/16, as the first version
/// of the self-intersection test saw arcs.
fn line_meets_chords_of(arc: &Prim, line: &Prim) -> bool {
    let Prim::Arc {
        c, r, a0, sweep, ..
    } = *arc
    else {
        return false;
    };
    let n = (sweep.abs() / (PI / 16.0)).ceil().max(1.0) as usize;
    let pt = |k: usize| {
        let a = a0 + sweep * k as f64 / n as f64;
        c + v(r * det::cos(a), r * det::sin(a))
    };
    let side = |a: Vec2, b: Vec2, x: Vec2| cross2(b - a, x - a) > 0.0;
    (0..n).any(|k| {
        let (p0, p1) = (pt(k), pt(k + 1));
        let (q0, q1) = (line.start(), line.end());
        side(p0, p1, q0) != side(p0, p1, q1) && side(q0, q1, p0) != side(q0, q1, p1)
    })
}

#[test]
fn a_snapped_edge_that_cuts_an_arc_between_its_chord_samples_is_found_to_cross_it() {
    // An arc of radius 10 from 0 to pi/8, closed by two lines through the centre, and a short
    // stray edge across the arc at pi/32 that starts and ends 0.02 either side of it: the arc's
    // chords (at pi/16) lie 0.048 inside the circle, so they never reach the edge.
    let (c, r) = (v(0.0, 0.0), 10.0);
    let arc = Prim::arc(c, r, 0.0, PI / 8.0);
    let at = |radius: f64| v(radius * det::cos(PI / 32.0), radius * det::sin(PI / 32.0));
    let ring = [
        arc,
        Prim::line(arc.end(), c),
        Prim::line(c, arc.start()),
        Prim::line(at(r - 0.02), at(r + 0.02)),
    ];
    assert!(
        !line_meets_chords_of(&arc, &ring[3]),
        "the sampled test misses the crossing"
    );
    assert!(!simple(&ring));
    // The same edge stopping short of the arc does not cross it.
    let short = [
        ring[0],
        ring[1],
        ring[2],
        Prim::line(at(r - 0.04), at(r - 0.02)),
    ];
    // (The pieces are a sector and a stray edge, not one closed loop, so the stray is checked
    // against the arc directly.)
    assert!(piece_hits(&short[0], &short[3]).is_empty());
    // Two circles crossing between each other's samples.
    let other = Prim::arc(v(10.0, 0.0), r, PI * 0.5, PI);
    assert!(!simple(&[Prim::arc(c, r, -PI / 2.0, PI), other]));
    assert!(simple(&[
        Prim::arc(c, r, PI / 2.0, PI),
        Prim::arc(v(30.0, 0.0), r, PI / 2.0, PI)
    ]));
}

/// A field with a round keep-out well inside it: the outer edges and the keep-out can be clipped
/// by different modes.
fn keep_out_field() -> Vec<Loop2> {
    vec![rect(0.3, 0.2, 43.1, 40.7), circle(21.0, 20.0, 4.3)]
}

fn kind_count(p: &CellGridPlan, kind: CellKind) -> usize {
    p.cells.iter().filter(|c| c.kind == kind).count()
}

#[test]
fn a_keep_out_can_be_clipped_by_another_mode_than_the_field() {
    let field = keep_out_field();
    let snap = run(&field, &with_clip(CellShape::Hexagon, Clip::Snap));
    // The default: the keep-out follows `clip`.
    let same = CellGridSpec {
        keepout_clip: Some(Clip::Snap),
        ..with_clip(CellShape::Hexagon, Clip::Snap)
    };
    let same = run(&field, &same);
    assert_eq!(counts(&snap), counts(&same));
    assert!((snap.open_area - same.open_area).abs() < 1e-9);
    assert_eq!(kind_count(&snap, CellKind::Free), 0);
    // Free round the keep-out only: its cells are trimmed to the circle, the outer edge's are not.
    let mixed = CellGridSpec {
        keepout_clip: Some(Clip::Free),
        ..with_clip(CellShape::Hexagon, Clip::Snap)
    };
    let mixed = run(&field, &mixed);
    let free = kind_count(&mixed, CellKind::Free);
    assert!(
        free > 0 && free < mixed.cells.len() / 2,
        "{free} of {}",
        mixed.cells.len()
    );
    // Cells far from the keep-out are the same as with snap everywhere.
    let far = |p: &CellGridPlan| {
        let mut v: Vec<(i64, i64)> = p
            .cells
            .iter()
            .filter(|c| c.center.distance(v(21.0, 20.0)) > 4.3 + 2.0 * P)
            .map(|c| (c.i, c.j))
            .collect();
        v.sort_unstable();
        v
    };
    assert_eq!(far(&mixed), far(&snap));
    // Whole round the keep-out only: no cell touches the circle, and nothing is trimmed.
    let whole = CellGridSpec {
        keepout_clip: Some(Clip::Whole),
        ..with_clip(CellShape::Hexagon, Clip::Snap)
    };
    let whole = run(&field, &whole);
    assert_eq!(kind_count(&whole, CellKind::Free), 0);
    assert!(whole.open_area <= snap.open_area + 1e-9);
}

#[test]
fn an_edge_that_passes_through_a_vertex_of_the_loop_is_not_simple() {
    // (0,0) -> (4,4) passes through the vertex (2,2), where the two edges on either side of it
    // lie on opposite sides: the loop crosses itself there.
    let pts = [(0.0, 0.0), (4.0, 4.0), (0.0, 4.0), (2.0, 2.0), (3.0, 0.0)];
    let ring: Vec<Prim> = (0..pts.len())
        .map(|k| {
            let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
            Prim::line(v(a.0, a.1), v(b.0, b.1))
        })
        .collect();
    assert!(!simple(&ring));
    // A plain quadrilateral, whose neighbours only meet at their joints, is still simple.
    let square: Vec<Prim> = [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)]
        .iter()
        .zip([(4.0, 0.0), (4.0, 4.0), (0.0, 4.0), (0.0, 0.0)])
        .map(|(a, b)| Prim::line(v(a.0, a.1), v(b.0, b.1)))
        .collect();
    assert!(simple(&square));
}

#[test]
fn a_very_long_edge_across_an_arc_is_still_found_to_cross_it() {
    // The unit right semicircle, and a line from -1e9 to 1e9 through its centre: it meets the
    // arc's rim at (1, 0). A quadratic with the radius added to terms of 1e18 loses the radius.
    let arc = Prim::arc(v(0.0, 0.0), 1.0, -FRAC_PI_2, PI);
    let line = Prim::line(v(-1e9, 0.0), v(1e9, 0.0));
    assert!(!simple(&[arc, line]));
}

#[test]
fn a_circle_meets_a_very_long_segment_where_it_crosses_it() {
    use crate::cell_grid::region::circle_segment_points;
    let (pts, n) = circle_segment_points(v(0.0, 0.0), 1.0, v(-1e9, 0.0), v(1e9, 0.0));
    assert_eq!(n, 2);
    assert!(pts[0].distance(v(-1.0, 0.0)) < 1e-6 && pts[1].distance(v(1.0, 0.0)) < 1e-6);
    // A segment that stops short of the circle meets it nowhere; one that ends inside, once.
    assert_eq!(
        circle_segment_points(v(0.0, 0.0), 1.0, v(-1e9, 0.0), v(-2.0, 0.0)).1,
        0
    );
    assert_eq!(
        circle_segment_points(v(0.0, 0.0), 1.0, v(-1e9, 0.0), v(0.0, 0.0)).1,
        1
    );
}

#[test]
fn pieces_that_overlap_along_a_stretch_are_not_simple() {
    let ring = |extra: Prim| {
        [
            Prim::line(v(0.0, 0.0), v(3.0, 0.0)),
            Prim::line(v(3.0, 0.0), v(3.0, 3.0)),
            Prim::line(v(3.0, 3.0), v(0.0, 0.0)),
            extra,
        ]
    };
    // Only neighbours may meet, and only at their joint: a stray edge that touches the ring
    // at a point is refused.
    assert!(!simple(&ring(Prim::line(v(3.0, 0.0), v(3.5, 0.0)))));
    let apart = ring(Prim::line(v(3.5, 0.0), v(4.0, 0.0)));
    assert!(apart
        .iter()
        .take(3)
        .all(|p| piece_hits(p, &apart[3]).is_empty()));
    assert!(!simple(&ring(Prim::line(v(1.0, 0.0), v(2.0, 0.0)))));
    // Two arcs of one circle: end to end they only touch, overlapping they share a stretch.
    let (c, r) = (v(0.0, 0.0), 5.0);
    let touching = [Prim::arc(c, r, 0.0, PI), Prim::arc(c, r, PI, PI)];
    assert!(simple(&touching));
    let overlapping = [Prim::arc(c, r, 0.0, PI), Prim::arc(c, r, PI / 2.0, PI)];
    assert!(!simple(&overlapping));
    let backwards = [Prim::arc(c, r, 0.0, PI), Prim::arc(c, r, 1.5 * PI, -PI)];
    assert!(!simple(&backwards));
}

#[test]
fn a_snap_inset_moves_each_straight_field_edge_in_by_whole_lattice_lines() {
    let lat = lattice(CellShape::Hexagon);
    let field = frame_rect(0.3, 0.2, 60.1, 50.7);
    let b = |n| snap_loop_by(&field, &lat, n).unwrap().bounds();
    // The horizontal lattice lines are P / 2 apart (P is the cell pitch), at multiples of it: the
    // field's y edges 0.2 and 50.7 snap in to 3.1 and 49.6, and each inset moves each of them in
    // by exactly one more line. Worked out here from the pitch, not from the lattice code.
    let step = P / 2.0;
    for n in 0..4u32 {
        let got = b(n);
        let want = (step * (1.0 + f64::from(n)), step * (16.0 - f64::from(n)));
        assert!(
            (got.y0 - want.0).abs() < 1e-9,
            "inset {n}: y0 {} not {}",
            got.y0,
            want.0
        );
        assert!(
            (got.y1 - want.1).abs() < 1e-9,
            "inset {n}: y1 {} not {}",
            got.y1,
            want.1
        );
    }
    // The vertical edges move in too, by less than a cell column each step.
    for n in 0..3u32 {
        let (lo, hi) = (b(n), b(n + 1));
        assert!(lo.x0 < hi.x0 && hi.x0 - lo.x0 <= H + 1e-9, "{lo:?} {hi:?}");
        assert!(hi.x1 < lo.x1 && lo.x1 - hi.x1 <= H + 1e-9, "{lo:?} {hi:?}");
    }
    // A keep-out (clockwise) is not moved by an inset, whatever the field gets.
    let hole = frame_rect(10.0, 10.0, 12.0, 11.0).oriented(false);
    let plain = snap_loop(&hole, &lat).unwrap().bounds();
    let spec = CellGridSpec {
        snap_inset: 2,
        ..with_clip(CellShape::Hexagon, Clip::Snap)
    };
    let region = vec![rect(0.3, 0.2, 60.1, 50.7), circle(30.0, 25.0, 2.1)];
    let moved = run(&region, &spec);
    let still = run(&region, &with_clip(CellShape::Hexagon, Clip::Snap));
    assert!(moved.cells.len() < still.cells.len());
    assert!(moved.open_area < still.open_area);
    assert!(plain.x0 <= 10.0 && plain.x1 >= 12.0);
}

#[test]
fn a_snap_inset_needs_snap_and_a_sane_count() {
    let with = |clip, snap_inset| CellGridSpec {
        snap_inset,
        ..with_clip(CellShape::Hexagon, clip)
    };
    let field = [rect(0.3, 0.2, 60.1, 50.7)];
    assert!(plan(&field, &with(Clip::Snap, 1)).is_ok());
    assert!(plan(&field, &with(Clip::Free, 1)).is_err());
    assert!(plan(&field, &with(Clip::Whole, 1)).is_err());
    assert!(plan(&field, &with(Clip::Snap, MAX_SNAP_INSET + 1)).is_err());
    // An inset that eats the whole field keeps it as drawn, with a warning, and never panics.
    let eaten = plan(&field, &with(Clip::Snap, MAX_SNAP_INSET)).unwrap();
    assert!(!eaten.warnings.is_empty());
}

#[test]
fn a_step_between_two_edges_that_snap_onto_one_line_vanishes() {
    let lat = lattice(CellShape::Hexagon);
    let stepped = frame_loop(&[
        (-6.0, 0.0),
        (6.0, 0.0),
        (6.0, 8.0),
        (-6.3, 8.0),
        (-6.3, 4.0),
        (-6.0, 4.0),
    ])
    .oriented(false);
    let snapped = snap_loop(&stepped, &lat).unwrap();
    assert_eq!(snapped.prims.len(), 4);
    let plain = snap_loop(&frame_rect(-6.3, 0.0, 6.0, 8.0).oriented(false), &lat).unwrap();
    let (a, b) = (snapped.bounds(), plain.bounds());
    for (got, want) in [(a.x0, b.x0), (a.y0, b.y0), (a.x1, b.x1), (a.y1, b.y1)] {
        assert!((got - want).abs() < 1e-9);
    }
}

#[test]
fn a_loop_narrower_than_any_pair_of_lattice_lines_collapses_when_snapped() {
    let lat = lattice(CellShape::Hexagon);
    assert!(snap_loop(&frame_rect(0.5, 0.5, 20.0, 2.5), &lat).is_err());
}

// --- clipping: whole, snap, free ---

#[test]
fn a_field_on_lattice_lines_gets_the_documented_cells() {
    let (x0, y0, x1, y1) = ALIGNED;
    let p = run(&[rect(x0, y0, x1, y1)], &spec(CellShape::Hexagon));
    assert_eq!(counts(&p), (ALIGNED_WHOLE, ALIGNED_PARTIAL, 0));
    assert!((p.open_area - ALIGNED_AREA).abs() < 1e-6);
    assert_eq!(p.cells.len(), ALIGNED_WHOLE + ALIGNED_PARTIAL);
    assert!(p.warnings.is_empty());
}

#[test]
fn a_field_a_little_outside_the_lattice_lines_snaps_in_to_the_same_cells() {
    let (x0, y0, x1, y1) = ALIGNED;
    let p = run(
        &[rect(x0 - 0.5, y0 - 0.5, x1 + 0.5, y1 + 0.5)],
        &spec(CellShape::Hexagon),
    );
    assert_eq!(counts(&p), (ALIGNED_WHOLE, ALIGNED_PARTIAL, 0));
    assert!((p.open_area - ALIGNED_AREA).abs() < 1e-6);
}

#[test]
fn a_stepped_keep_out_that_snaps_to_a_rectangle_gets_the_cells_of_that_rectangle() {
    let field = rect(-30.0, -20.0, 30.0, 20.0);
    let stepped = polygon(&[
        (-6.0, 0.0),
        (6.0, 0.0),
        (6.0, 8.0),
        (-6.3, 8.0),
        (-6.3, 4.0),
        (-6.0, 4.0),
    ]);
    let a = run(&[field.clone(), stepped], &spec(CellShape::Hexagon));
    let b = run(
        &[field, rect(-6.3, 0.0, 6.0, 8.0)],
        &spec(CellShape::Hexagon),
    );
    assert!(a.warnings.is_empty());
    assert_eq!(counts(&a), counts(&b));
    assert_eq!(counts(&b), (43, 27, 0));
    assert!((a.open_area - b.open_area).abs() < 1e-9);
}

#[test]
fn a_stepped_field_edge_gets_the_cells_of_the_plain_rectangle() {
    let stepped = polygon(&[
        (0.0, 0.0),
        (40.0, 0.0),
        (40.0, 30.0),
        (0.2, 30.0),
        (0.2, 15.0),
        (0.0, 15.0),
    ]);
    let a = run(&[stepped], &spec(CellShape::Hexagon));
    let b = run(&[rect(0.0, 0.0, 40.0, 30.0)], &spec(CellShape::Hexagon));
    assert!(a.warnings.is_empty());
    assert_eq!(counts(&a), counts(&b));
    assert!((a.open_area - b.open_area).abs() < 1e-9);
}

#[test]
fn whole_keeps_only_the_cells_entirely_inside_the_field() {
    let (x0, y0, x1, y1) = ALIGNED;
    let p = run(
        &[rect(x0, y0, x1, y1)],
        &with_clip(CellShape::Hexagon, Clip::Whole),
    );
    assert_eq!(counts(&p), (ALIGNED_WHOLE, 0, 0));
}

#[test]
fn free_trims_the_edge_cells_and_adds_up_to_the_snapped_area_when_the_edges_are_on_the_lattice() {
    let (x0, y0, x1, y1) = ALIGNED;
    let p = run(
        &[rect(x0, y0, x1, y1)],
        &with_clip(CellShape::Hexagon, Clip::Free),
    );
    assert_eq!(p.whole, ALIGNED_WHOLE);
    assert!(p.partial >= ALIGNED_PARTIAL);
    assert!((p.open_area - ALIGNED_AREA).abs() < 1e-6, "{}", p.open_area);
    assert!(p
        .cells
        .iter()
        .filter(|c| c.kind == CellKind::Free)
        .all(|c| c.area > 0.0));
}

#[test]
fn a_round_keep_out_leaves_the_cells_around_it_as_single_chord_pieces() {
    let p = run(
        &[rect(-20.0, -20.0, 40.0, 40.0), circle(10.0, 10.0, 6.0)],
        &spec(CellShape::Hexagon),
    );
    assert!(p.fallback > 0);
    for c in p.cells.iter().filter(|c| c.kind == CellKind::Fallback) {
        assert!(
            [5.0 / 6.0, 0.5, 1.0 / 6.0]
                .iter()
                .any(|f| (c.area / CELL_AREA - f).abs() < 1e-9),
            "{}",
            c.area / CELL_AREA
        );
        // Nothing of it reaches into the keep-out.
        assert!(vertices(c)
            .iter()
            .all(|q| q.distance(v(10.0, 10.0)) >= 6.0 - 1e-6));
    }
    assert!(p.cells.iter().all(|c| {
        vertices(c)
            .iter()
            .all(|q| q.distance(v(10.0, 10.0)) >= 6.0 - 1e-6)
    }));
}

#[test]
fn a_circular_field_off_a_lattice_row_by_float_noise_keeps_that_row() {
    for row_y in [0.0, P / 2.0] {
        let reference = run(&[circle(0.0, row_y, 26.4)], &spec(CellShape::Hexagon));
        for d in [
            1e-15,
            -1e-15,
            3.552713678800501e-15,
            -3.552713678800501e-15,
            1e-13,
            -1e-13,
            1e-12,
            -1e-12,
        ] {
            let p = run(&[circle(0.0, row_y + d, 26.4)], &spec(CellShape::Hexagon));
            assert_eq!(
                counts(&p),
                counts(&reference),
                "row {row_y}, centre off by {d:e}"
            );
            assert!((p.open_area - reference.open_area).abs() < 1e-6);
        }
    }
}

#[test]
fn random_hexagon_fields_with_keep_outs_never_open_a_piece_outside_the_field_or_off_a_chord() {
    let mut rng = Rng(1234);
    let lat = lattice(CellShape::Hexagon);
    for trial in 0..25 {
        let outline = random_polygon(&mut rng, 12, 60.0, 4, 6);
        let field = Convex::new(outline.iter().map(|&(x, y)| v(x, y)).collect()).unwrap();
        let field = if poly_area(&field.pts) > 0.0 {
            field
        } else {
            Convex::new(field.pts.iter().rev().copied().collect()).unwrap()
        };
        let mut loops = vec![polygon(&outline)];
        let mut keep: Vec<(bool, [f64; 4])> = Vec::new();
        // Keep-outs in separate quadrants, so they never overlap.
        let quadrants = [(-14.0, -14.0), (2.0, -14.0), (-14.0, 2.0), (2.0, 2.0)];
        let count = rng.int(0, 2);
        for q in rng.sample(4, count) {
            let (qx, qy) = quadrants[q];
            if rng.next() < 0.5 {
                let (x0, y0) = (rng.range(qx, qx + 2.0), rng.range(qy, qy + 2.0));
                let (x1, y1) = (x0 + rng.range(3.0, 10.0), y0 + rng.range(3.0, 10.0));
                loops.push(rect(x0, y0, x1, y1));
                keep.push((false, [x0, y0, x1, y1]));
            } else {
                let r = rng.range(1.5, 5.5);
                loops.push(circle(qx + 6.0, qy + 6.0, r));
                keep.push((true, [qx + 6.0, qy + 6.0, r, 0.0]));
            }
        }
        let in_field = |q: Vec2| {
            field.contains(q, 1e-6)
                && keep.iter().all(|&(round, k)| {
                    if round {
                        q.distance(v(k[0], k[1])) >= k[2] - 1e-6
                    } else {
                        !(q.x > k[0] + 1e-6
                            && q.x < k[2] - 1e-6
                            && q.y > k[1] + 1e-6
                            && q.y < k[3] - 1e-6)
                    }
                })
        };
        let p = run(&loops, &spec(CellShape::Hexagon));
        for c in &p.cells {
            assert!(
                vertices(c).iter().all(|&q| in_field(q)),
                "trial {trial}: ({}, {}) {:?} leaves the field: bad {:?}; keep {:?}; field {:?}",
                c.i,
                c.j,
                c.kind,
                vertices(c)
                    .iter()
                    .filter(|&&q| !in_field(q))
                    .collect::<Vec<_>>(),
                keep,
                outline
            );
            assert!(
                edges_on_cell_lines(&lat, c),
                "trial {trial}: ({}, {}) has an off-chord edge",
                c.i,
                c.j
            );
        }
    }
}

// --- the other shapes ---

#[test]
fn whole_cells_of_every_polygonal_lattice_are_exactly_the_cells_with_every_corner_in_a_rectangle() {
    let mut rng = Rng(7);
    let cases = [
        (CellShape::Hexagon, DotGrid::Hexagonal, 8),
        (CellShape::Square, DotGrid::Hexagonal, 8),
        (CellShape::Triangle, DotGrid::Hexagonal, 8),
        (CellShape::Polygon, DotGrid::Hexagonal, 5),
        (CellShape::Polygon, DotGrid::Square, 8),
    ];
    for (shape, dot_grid, sides) in cases {
        let lat = Lattice::new(shape, S, W, dot_grid, sides).unwrap();
        let s = CellGridSpec {
            shape,
            dot_grid,
            sides,
            clip: Clip::Whole,
            ..spec(shape)
        };
        for trial in 0..6 {
            let (x0, y0) = (rng.range(-20.0, 0.0), rng.range(-20.0, 0.0));
            let (x1, y1) = (x0 + rng.range(15.0, 45.0), y0 + rng.range(15.0, 45.0));
            let mut got: Vec<(i64, i64)> = run(&[rect(x0, y0, x1, y1)], &s)
                .cells
                .iter()
                .map(|c| (c.i, c.j))
                .collect();
            got.sort_unstable();
            let mut expect: Vec<(i64, i64)> = (-30..=30)
                .flat_map(|i| (-30..=30).map(move |j| (i, j)))
                .filter(|&(i, j)| {
                    lat.cell(i, j).iter().all(|q| {
                        q.x >= x0 - 1e-9 && q.x <= x1 + 1e-9 && q.y >= y0 - 1e-9 && q.y <= y1 + 1e-9
                    })
                })
                .collect();
            expect.sort_unstable();
            assert_eq!(got, expect, "{shape:?} {dot_grid:?} trial {trial}");
        }
    }
}

#[test]
fn a_rectangle_on_the_cell_sides_of_a_square_grid_gets_every_cell_whole() {
    let a = S / 2.0;
    let p = run(
        &[rect(-a, -a, 3.0 * P + a, 2.0 * P + a)],
        &spec(CellShape::Square),
    );
    assert_eq!(counts(&p), (12, 0, 0));
}

#[test]
fn a_square_grid_field_edge_snaps_inwards_through_the_web_or_to_the_next_cell_in() {
    let a = S / 2.0;
    // An edge inside column 0 snaps to that column's far side, dropping it.
    let p = run(
        &[rect(-a + 0.3, -a - 0.4, 3.0 * P + a + 0.5, 2.0 * P + a)],
        &spec(CellShape::Square),
    );
    assert_eq!(counts(&p), (9, 0, 0));
    // An edge in the web snaps onto the side of the next cell in.
    let p = run(
        &[rect(-a - 0.5, -a, 3.0 * P + a + 0.5, 2.0 * P + a)],
        &spec(CellShape::Square),
    );
    assert_eq!(counts(&p), (12, 0, 0));
}

#[test]
fn a_diagonal_field_edge_halves_the_square_cells_it_runs_through() {
    let a = S / 2.0;
    let p = run(
        &[polygon(&[
            (-a, -a),
            (3.0 * P + a, -a),
            (3.0 * P + a, 3.0 * P + a),
        ])],
        &spec(CellShape::Square),
    );
    assert_eq!(counts(&p), (6, 4, 0));
    assert!((p.open_area - (6.0 * S * S + 4.0 * S * S / 2.0)).abs() < 1e-9);
}

#[test]
fn a_small_keep_out_on_a_square_grid_snaps_out_to_the_sides_of_its_cell() {
    let a = S / 2.0;
    let field = rect(-a, -a, 5.0 * P + a, 5.0 * P + a);
    let p = run(
        &[
            field,
            rect(2.0 * P - 0.2, 2.0 * P - 0.2, 2.0 * P + 0.2, 2.0 * P + 0.2),
        ],
        &spec(CellShape::Square),
    );
    assert_eq!(p.whole, 35);
}

#[test]
fn random_square_grid_fields_never_open_a_piece_outside_the_field_or_off_a_chord() {
    let mut rng = Rng(99);
    let lat = lattice(CellShape::Square);
    for trial in 0..20 {
        let outline = random_polygon(&mut rng, 8, 50.0, 3, 8);
        let mut field = Convex::new(outline.iter().map(|&(x, y)| v(x, y)).collect()).unwrap();
        if poly_area(&field.pts) < 0.0 {
            field = Convex::new(field.pts.iter().rev().copied().collect()).unwrap();
        }
        let p = run(&[polygon(&outline)], &spec(CellShape::Square));
        for c in &p.cells {
            assert!(
                vertices(c).iter().all(|&q| field.contains(q, 1e-6)),
                "trial {trial}: ({}, {}) leaves the field",
                c.i,
                c.j
            );
            assert!(
                edges_on_cell_lines(&lat, c),
                "trial {trial}: ({}, {}) is cut off a chord",
                c.i,
                c.j
            );
        }
    }
}

/// The triangle bounded by cell (0, 0)'s three sides, each moved out by `grow`.
fn big_triangle(lat: &Lattice, grow: f64) -> Loop2 {
    let mut lines = Vec::new();
    for k in [0, 2, 4] {
        let nrm = lat.normal(k);
        let off = lat
            .outline(0)
            .iter()
            .map(|q| nrm.dot(*q))
            .fold(f64::INFINITY, f64::min);
        lines.push((nrm, off - grow));
    }
    let pts: Vec<(f64, f64)> = (0..3)
        .map(|k| {
            let ((n1, c1), (n2, c2)) = (lines[(k + 2) % 3], lines[k]);
            let det = n1.x * n2.y - n1.y * n2.x;
            ((c1 * n2.y - c2 * n1.y) / det, (n1.x * c2 - n2.x * c1) / det)
        })
        .collect();
    polygon(&pts)
}

#[test]
fn a_triangle_field_the_size_of_one_triangle_cell_gets_that_cell_even_when_a_little_larger() {
    let lat = lattice(CellShape::Triangle);
    assert_eq!(
        counts(&run(&[big_triangle(&lat, 0.0)], &spec(CellShape::Triangle))),
        (1, 0, 0)
    );
    assert_eq!(
        counts(&run(&[big_triangle(&lat, 0.4)], &spec(CellShape::Triangle))),
        (1, 0, 0)
    );
}

#[test]
fn a_triangle_field_snaps_to_whole_rows_and_can_lose_nearly_a_row() {
    let lat = lattice(CellShape::Triangle);
    let ht = lat.row_height();
    // Each side moved out by one row: the sides are lattice lines again, round 4 tiling
    // triangles a side, so 16 cells (10 up, 6 down), all whole.
    let p = run(&[big_triangle(&lat, ht)], &spec(CellShape::Triangle));
    assert_eq!(counts(&p), (16, 0, 0));
    assert_eq!(p.cells.iter().map(|c| lat.key(c.i, c.j)).sum::<usize>(), 6);
    // Moved in by 0.4: horizontal lattice lines come in pairs a web apart, one pair a row, so each
    // side snaps in by nearly a row, back to cell (0, 0) alone. Whole keeps more, with a sawtooth edge.
    let shrunk = [big_triangle(&lat, ht - 0.4)];
    assert_eq!(counts(&run(&shrunk, &spec(CellShape::Triangle))), (1, 0, 0));
    assert!(run(&shrunk, &with_clip(CellShape::Triangle, Clip::Whole)).whole > 1);
}

#[test]
fn a_rectangle_does_not_snap_to_triangles_so_it_acts_as_whole() {
    let field = [rect(-10.0, -10.0, 30.0, 25.0)];
    let snap = run(&field, &spec(CellShape::Triangle));
    let whole = run(&field, &with_clip(CellShape::Triangle, Clip::Whole));
    assert_eq!(counts(&snap), counts(&whole));
}

#[test]
fn circles_are_counted_whole_on_either_grid_and_snap_acts_as_whole_for_them() {
    let (x0, y0, x1, y1) = (-11.3, -7.9, 40.6, 31.7);
    for dot_grid in [DotGrid::Hexagonal, DotGrid::Square] {
        let lat = Lattice::new(CellShape::Circle, S, W, dot_grid, 8).unwrap();
        let s = CellGridSpec {
            dot_grid,
            clip: Clip::Whole,
            ..spec(CellShape::Circle)
        };
        let p = run(&[rect(x0, y0, x1, y1)], &s);
        let mut expect = 0;
        for (i, j) in lat.cells_in(&region_bounds(x0, y0, x1, y1)).unwrap() {
            let c = lat.center(i, j);
            let margin = (c.x - x0).min(x1 - c.x).min(c.y - y0).min(y1 - c.y);
            assert!(
                (margin - S / 2.0).abs() > 0.01,
                "the test field is too close to a cell"
            );
            expect += usize::from(margin > S / 2.0);
        }
        assert_eq!(counts(&p), (expect, 0, 0), "{dot_grid:?}");
        assert!((p.open_area - expect as f64 * lat.cell_area).abs() < 1e-9);
        // Snap means whole for circles.
        assert_eq!(
            counts(&run(
                &[rect(x0, y0, x1, y1)],
                &CellGridSpec {
                    clip: Clip::Snap,
                    ..s
                }
            )),
            (expect, 0, 0)
        );
        let free = run(
            &[rect(x0, y0, x1, y1)],
            &CellGridSpec {
                clip: Clip::Free,
                ..s
            },
        );
        assert_eq!(free.whole, expect);
        assert!(free.partial > 0);
    }
}

fn region_bounds(x0: f64, y0: f64, x1: f64, y1: f64) -> Bounds {
    Bounds { x0, y0, x1, y1 }
}

#[test]
fn octagons_on_a_square_grid_inside_a_rectangle_are_all_whole_with_the_octagon_area() {
    let s = CellGridSpec {
        shape: CellShape::Polygon,
        dot_grid: DotGrid::Square,
        sides: 8,
        clip: Clip::Whole,
        ..spec(CellShape::Polygon)
    };
    let lat = Lattice::new(CellShape::Polygon, S, W, DotGrid::Square, 8).unwrap();
    let p = run(
        &[rect(
            -S / 2.0,
            -S / 2.0,
            3.0 * P + S / 2.0,
            2.0 * P + S / 2.0,
        )],
        &s,
    );
    assert_eq!(counts(&p), (12, 0, 0));
    assert!((p.open_area - 12.0 * lat.cell_area).abs() < 1e-9);
}

#[test]
fn a_whole_circle_cell_is_one_circle_loop_and_a_whole_hexagon_is_six_lines() {
    let c = run(
        &[rect(-4.0, -4.0, 4.0, 4.0)],
        &with_clip(CellShape::Circle, Clip::Whole),
    );
    let cell = c.cells.first().unwrap();
    assert!(
        matches!(cell.loops.first().unwrap().segs.as_slice(), [Seg2::Arc { radius, .. }] if (*radius - S / 2.0).abs() < 1e-12)
    );
    let h = run(
        &[rect(-4.0, -4.0, 4.0, 4.0)],
        &with_clip(CellShape::Hexagon, Clip::Whole),
    );
    assert_eq!(
        h.cells.first().unwrap().loops.first().unwrap().segs.len(),
        6
    );
}

// --- free clipping, exactly ---

fn hexagon_cell_area_left_of(x: f64) -> f64 {
    let lat = lattice(CellShape::Hexagon);
    poly_area(&clip_halfplane(&lat.cell(0, 0), v(-1.0, 0.0), -x))
}

#[test]
fn free_trims_a_hexagon_to_a_straight_edge_exactly() {
    let p = run(
        &[rect(-100.0, -100.0, 1.0, 100.0)],
        &with_clip(CellShape::Hexagon, Clip::Free),
    );
    let cell = p.cells.iter().find(|c| (c.i, c.j) == (0, 0)).unwrap();
    assert_eq!(cell.kind, CellKind::Free);
    assert!((cell.area - hexagon_cell_area_left_of(1.0)).abs() < 1e-9);
    assert_eq!(cell.loops.len(), 1);
}

#[test]
fn free_open_area_of_a_rectangle_matches_clipping_every_cell_to_it() {
    let (x0, y0, x1, y1) = (-8.13, -7.31, 31.27, 26.93);
    for (shape, sides) in [
        (CellShape::Hexagon, 8),
        (CellShape::Square, 8),
        (CellShape::Triangle, 8),
        (CellShape::Polygon, 6),
    ] {
        let s = CellGridSpec {
            shape,
            sides,
            clip: Clip::Free,
            ..spec(shape)
        };
        let lat = Lattice::new(shape, S, W, DotGrid::Hexagonal, sides).unwrap();
        let mut expect = 0.0;
        for i in -20..=20 {
            for j in -20..=20 {
                let mut poly = lat.cell(i, j);
                for (n, c) in [
                    (v(1.0, 0.0), x0),
                    (v(-1.0, 0.0), -x1),
                    (v(0.0, 1.0), y0),
                    (v(0.0, -1.0), -y1),
                ] {
                    poly = clip_halfplane(&poly, n, c);
                }
                if poly.len() >= 3 {
                    expect += poly_area(&poly);
                }
            }
        }
        let got = run(&[rect(x0, y0, x1, y1)], &s).open_area;
        assert!(
            (got - expect).abs() < 1e-6 * expect,
            "{shape:?}: {got} against {expect}"
        );
    }
}

#[test]
fn free_leaves_a_hole_where_a_keep_out_sits_inside_one_cell() {
    let p = run(
        &[rect(-40.0, -40.0, 40.0, 40.0), circle(0.5, 0.3, 1.0)],
        &with_clip(CellShape::Hexagon, Clip::Free),
    );
    let cell = p.cells.iter().find(|c| (c.i, c.j) == (0, 0)).unwrap();
    assert_eq!(cell.kind, CellKind::Free);
    assert_eq!(cell.loops.len(), 2);
    assert!((cell.area - (CELL_AREA - PI)).abs() < 1e-9);
    let areas: Vec<f64> = cell
        .loops
        .iter()
        .map(|l| l.signed_area().unwrap().value)
        .collect();
    assert!(areas.iter().any(|&a| a > 0.0) && areas.iter().any(|&a| a < 0.0));
}

#[test]
fn free_trims_a_circle_cell_to_a_straight_edge_with_the_exact_segment_area() {
    let (r, d) = (S / 2.0, 1.0);
    let p = run(
        &[rect(-100.0, -100.0, d, 100.0)],
        &with_clip(CellShape::Circle, Clip::Free),
    );
    let cell = p.cells.iter().find(|c| (c.i, c.j) == (0, 0)).unwrap();
    let expect = r * r * det::acos(-d / r) + d * (r * r - d * d).sqrt();
    assert!(
        (cell.area - expect).abs() < 1e-9,
        "{} against {expect}",
        cell.area
    );
    assert!(cell
        .loops
        .first()
        .unwrap()
        .segs
        .iter()
        .any(|s| matches!(s, Seg2::Arc { .. })));
}

#[test]
fn free_follows_a_round_field_with_arcs_and_matches_a_fine_polygon_of_it() {
    let (cx, cy, r) = (0.3, 0.7, 20.0);
    let p = run(
        &[circle(cx, cy, r)],
        &with_clip(CellShape::Hexagon, Clip::Free),
    );
    let lat = lattice(CellShape::Hexagon);
    let n = 2048;
    let disc: Vec<Vec2> = (0..n)
        .map(|k| {
            v(
                cx + r * det::cos(2.0 * PI * k as f64 / n as f64),
                cy + r * det::sin(2.0 * PI * k as f64 / n as f64),
            )
        })
        .collect();
    let mut expect = 0.0;
    for i in -12..=12 {
        for j in -12..=12 {
            let mut poly = lat.cell(i, j);
            for k in 0..n {
                let (a, b) = (disc[k], disc[ring_next(k, n)]);
                let nrm = (b - a).perp().normalized().unwrap();
                poly = clip_halfplane(&poly, nrm, nrm.dot(a));
                if poly.len() < 3 {
                    break;
                }
            }
            if poly.len() >= 3 {
                expect += poly_area(&poly);
            }
        }
    }
    assert!(
        (p.open_area - expect).abs() < 0.1,
        "{} against {expect}",
        p.open_area
    );
    let free: Vec<&GridCell> = p
        .cells
        .iter()
        .filter(|c| c.kind == CellKind::Free)
        .collect();
    assert!(!free.is_empty());
    assert!(free.iter().any(|c| {
        c.loops
            .iter()
            .any(|l| l.segs.iter().any(|s| matches!(s, Seg2::Arc { .. })))
    }));
    assert!(p.open_area < PI * r * r);
}

// --- two fields on one lattice ---

#[test]
fn two_fields_with_the_same_anchor_angle_and_size_share_one_lattice() {
    for (angle, anchor) in [
        (0.0, v(0.0, 0.0)),
        (0.0, v(3.7, -1.9)),
        (FRAC_PI_6, v(30.0, 20.0)),
        (0.4, v(-5.0, 8.0)),
    ] {
        for shape in [
            CellShape::Hexagon,
            CellShape::Square,
            CellShape::Triangle,
            CellShape::Circle,
        ] {
            let s = CellGridSpec {
                angle,
                anchor,
                ..with_clip(shape, Clip::Whole)
            };
            let a = run(&[rect(-30.0, -20.0, 10.0, 25.0)], &s);
            let b = run(&[circle(40.0, 15.0, 22.0)], &s);
            assert!(!a.cells.is_empty() && !b.cells.is_empty());
            for c in a.cells.iter().chain(&b.cells) {
                // The very same centre, bit for bit, whichever field it came from.
                assert_eq!(
                    c.center,
                    cell_center(&s, c.i, c.j).unwrap(),
                    "{shape:?} angle {angle}"
                );
            }
        }
    }
}

#[test]
fn a_grid_split_over_two_fields_has_every_cell_of_either_half_in_the_whole_field_too() {
    for angle in [0.0, FRAC_PI_6, 0.9] {
        let s = CellGridSpec {
            angle,
            anchor: v(3.0, 4.0),
            ..with_clip(CellShape::Hexagon, Clip::Whole)
        };
        // The whole field is a rotated rectangle; each half is one side of a cut across it.
        let rot = |x: f64, y: f64| {
            let d = v(x, y) - s.anchor;
            let (sn, cs) = det::sin_cos(angle);
            (
                s.anchor.x + cs * d.x - sn * d.y,
                s.anchor.y + sn * d.x + cs * d.y,
            )
        };
        let (x0, x1, y0, y1, cut) = (-20.0, 60.0, -10.0, 45.0, 17.3);
        let quad =
            |ya: f64, yb: f64| polygon(&[rot(x0, ya), rot(x1, ya), rot(x1, yb), rot(x0, yb)]);
        let whole = run(&[quad(y0, y1)], &s);
        let (lo, hi) = (run(&[quad(y0, cut)], &s), run(&[quad(cut, y1)], &s));
        let key = |c: &GridCell| (c.i, c.j);
        let all: std::collections::HashSet<_> = whole.cells.iter().map(key).collect();
        for c in lo.cells.iter().chain(&hi.cells) {
            assert!(
                all.contains(&key(c)),
                "angle {angle}: ({}, {}) is in a half but not the whole",
                c.i,
                c.j
            );
        }
        let halves: std::collections::HashSet<_> =
            lo.cells.iter().chain(&hi.cells).map(key).collect();
        for c in whole.cells.iter().filter(|c| !halves.contains(&key(c))) {
            // Only cells the cut runs through are missing from the halves.
            let local = (c.center - s.anchor).dot(v(-det::sin(angle), det::cos(angle)));
            assert!(
                (local - cut).abs() < S,
                "angle {angle}: ({}, {}) is missing away from the cut",
                c.i,
                c.j
            );
        }
    }
}

#[test]
fn snapped_halves_split_on_a_lattice_line_add_up_to_the_whole_field() {
    let (x0, y0, x1, y1) = ALIGNED;
    let split = 2.0 * P; // a long-diagonal line, through the centres of the even columns
    let s = spec(CellShape::Hexagon);
    let single = run(&[rect(x0, y0, x1, y1)], &s);
    let (lo, hi) = (
        run(&[rect(x0, y0, x1, split)], &s),
        run(&[rect(x0, split, x1, y1)], &s),
    );
    assert!((lo.open_area + hi.open_area - single.open_area).abs() < 1e-6);
    assert!((single.open_area - ALIGNED_AREA).abs() < 1e-6);
}

#[test]
fn moving_the_anchor_by_a_lattice_step_moves_every_cell_with_it() {
    let base = spec(CellShape::Hexagon);
    let step = cell_center(&base, 2, -1).unwrap();
    let moved = CellGridSpec {
        anchor: step,
        ..base
    };
    let field = [rect(-30.0, -20.0, 30.0, 20.0)];
    let shifted = [rect(
        -30.0 + step.x,
        -20.0 + step.y,
        30.0 + step.x,
        20.0 + step.y,
    )];
    let (a, b) = (run(&field, &base), run(&shifted, &moved));
    assert_eq!(counts(&a), counts(&b));
    assert!((a.open_area - b.open_area).abs() < 1e-6);
}

#[test]
fn moving_the_anchor_off_the_lattice_moves_where_the_cells_fall() {
    let field = [rect(-30.0, -20.0, 30.0, 20.0)];
    let a = run(&field, &with_clip(CellShape::Hexagon, Clip::Whole));
    let b = run(
        &field,
        &CellGridSpec {
            anchor: v(1.3, 0.9),
            ..with_clip(CellShape::Hexagon, Clip::Whole)
        },
    );
    let centres = |p: &CellGridPlan| {
        p.cells
            .iter()
            .map(|c| (c.center.x.to_bits(), c.center.y.to_bits()))
            .collect::<std::collections::HashSet<_>>()
    };
    assert_ne!(centres(&a), centres(&b));
}

#[test]
fn a_rotated_grid_over_a_rotated_field_gets_the_cells_of_the_unrotated_one() {
    let (x0, y0, x1, y1) = ALIGNED;
    for angle in [FRAC_PI_6, 0.7, -2.0] {
        let anchor = v(2.0, -3.0);
        let rot = |x: f64, y: f64| {
            let (sn, cs) = det::sin_cos(angle);
            (anchor.x + cs * x - sn * y, anchor.y + sn * x + cs * y)
        };
        let field = polygon(&[rot(x0, y0), rot(x1, y0), rot(x1, y1), rot(x0, y1)]);
        let p = run(
            &[field],
            &CellGridSpec {
                angle,
                anchor,
                ..spec(CellShape::Hexagon)
            },
        );
        assert_eq!(
            counts(&p),
            (ALIGNED_WHOLE, ALIGNED_PARTIAL, 0),
            "angle {angle}"
        );
        assert!((p.open_area - ALIGNED_AREA).abs() < 1e-6);
    }
}

// --- hostile input ---

#[test]
fn an_empty_field_has_no_cells() {
    let p = run(&[], &spec(CellShape::Hexagon));
    assert_eq!((p.cells.len(), p.open_area), (0, 0.0));
}

#[test]
fn a_field_too_small_to_hold_a_cell_has_no_cells_in_any_mode() {
    for clip in [Clip::Whole, Clip::Snap] {
        assert_eq!(
            run(
                &[rect(0.2, 0.2, 1.0, 1.0)],
                &with_clip(CellShape::Hexagon, clip)
            )
            .cells
            .len(),
            0,
            "{clip:?}"
        );
    }
}

#[test]
fn a_field_that_reaches_too_many_cells_is_refused_not_planned() {
    for shape in [
        CellShape::Hexagon,
        CellShape::Square,
        CellShape::Triangle,
        CellShape::Circle,
    ] {
        for size in [(1e5, 1e5), (1e9, 1e9)] {
            let r = plan(&[rect(0.0, 0.0, size.0, size.1)], &spec(shape));
            assert!(
                matches!(r, Err(CellGridError::TooManyCells { .. })),
                "{shape:?} {size:?}"
            );
        }
    }
    let tiny = CellGridSpec {
        size: MIN_SIZE,
        web: MIN_SIZE,
        ..CellGridSpec::default()
    };
    assert!(matches!(
        plan(&[rect(0.0, 0.0, 100.0, 100.0)], &tiny),
        Err(CellGridError::TooManyCells { .. })
    ));
}

#[test]
fn a_field_of_thirty_thousand_cells_is_planned() {
    let p = run(
        &[rect(0.0, 0.0, 1000.0, 1000.0)],
        &with_clip(CellShape::Hexagon, Clip::Snap),
    );
    assert!(p.cells.len() > 25_000 && p.cells.len() <= MAX_CELLS);
    assert!(p.open_area < 1_000_000.0);
}

#[test]
fn a_loop_with_no_segments_a_gap_or_a_coordinate_that_is_not_finite_is_refused() {
    let s = spec(CellShape::Hexagon);
    assert!(matches!(
        plan(&[Loop2 { segs: vec![] }], &s),
        Err(CellGridError::Field(_))
    ));
    let open = Loop2 {
        segs: vec![
            Seg2::Line {
                a: v(0.0, 0.0),
                b: v(10.0, 0.0),
            },
            Seg2::Line {
                a: v(10.0, 0.0),
                b: v(10.0, 10.0),
            },
        ],
    };
    assert!(matches!(plan(&[open], &s), Err(CellGridError::Field(_))));
    assert!(plan(&[rect(0.0, 0.0, f64::NAN, 10.0)], &s).is_err());
    assert!(plan(&[rect(0.0, 0.0, f64::INFINITY, 10.0)], &s).is_err());
    assert!(plan(&[rect(0.0, 0.0, 1e300, 10.0)], &s).is_err());
    let bad_arc = Loop2 {
        segs: vec![Seg2::Arc {
            center: v(0.0, 0.0),
            radius: -1.0,
            start: 0.0,
            sweep: 7.0,
        }],
    };
    assert!(plan(&[bad_arc], &s).is_err());
}

#[test]
fn degenerate_loops_are_ignored_and_never_crash_any_mode() {
    let sliver = rect(0.0, 0.0, 30.0, 0.0);
    let dot = polygon(&[(5.0, 5.0), (5.0, 5.0), (5.0, 5.0)]);
    for shape in [
        CellShape::Hexagon,
        CellShape::Square,
        CellShape::Triangle,
        CellShape::Circle,
        CellShape::Polygon,
    ] {
        for clip in [Clip::Whole, Clip::Snap, Clip::Free] {
            let s = with_clip(shape, clip);
            let _ = plan(
                &[sliver.clone(), dot.clone(), rect(0.0, 0.0, 30.0, 30.0)],
                &s,
            );
            let _ = plan(std::slice::from_ref(&sliver), &s);
        }
    }
}

#[test]
fn overlapping_keep_outs_and_self_crossing_fields_never_crash_any_mode() {
    let bowtie = polygon(&[(0.0, 0.0), (30.0, 30.0), (30.0, 0.0), (0.0, 30.0)]);
    let fields = [
        vec![bowtie],
        vec![
            rect(0.0, 0.0, 40.0, 40.0),
            circle(15.0, 15.0, 8.0),
            circle(20.0, 18.0, 8.0),
            rect(10.0, 10.0, 25.0, 25.0),
        ],
    ];
    for field in &fields {
        for shape in [
            CellShape::Hexagon,
            CellShape::Square,
            CellShape::Triangle,
            CellShape::Circle,
            CellShape::Polygon,
        ] {
            for clip in [Clip::Whole, Clip::Snap, Clip::Free] {
                let r = plan(field, &with_clip(shape, clip));
                assert!(r.is_ok(), "{shape:?} {clip:?}");
            }
        }
    }
}

#[test]
fn a_field_with_a_spline_edge_is_followed_closely_and_never_snapped() {
    let spline = Seg2::Cubic {
        p0: v(40.0, 0.0),
        p1: v(55.0, 12.0),
        p2: v(55.0, 28.0),
        p3: v(40.0, 40.0),
    };
    let field = Loop2 {
        segs: vec![
            Seg2::Line {
                a: v(0.0, 0.0),
                b: v(40.0, 0.0),
            },
            spline,
            Seg2::Line {
                a: v(40.0, 40.0),
                b: v(0.0, 40.0),
            },
            Seg2::Line {
                a: v(0.0, 40.0),
                b: v(0.0, 0.0),
            },
        ],
    };
    for clip in [Clip::Whole, Clip::Snap, Clip::Free] {
        let p = run(
            std::slice::from_ref(&field),
            &with_clip(CellShape::Hexagon, clip),
        );
        assert!(p.whole > 10, "{clip:?}");
        assert!(
            p.warnings.iter().any(|w| w.contains("followed")),
            "{clip:?}"
        );
        // The bulge is part of the field: cells reach past the chord x = 40.
        assert!(
            p.cells
                .iter()
                .any(|c| vertices(c).iter().any(|q| q.x > 40.0)),
            "{clip:?}"
        );
    }
    let whole = run(
        std::slice::from_ref(&field),
        &with_clip(CellShape::Hexagon, Clip::Whole),
    );
    let free = run(&[field], &with_clip(CellShape::Hexagon, Clip::Free));
    assert!(free.open_area >= whole.open_area);
}

#[test]
fn an_arc_field_with_a_negative_sweep_is_the_same_field_drawn_the_other_way() {
    let ccw = circle(5.0, 5.0, 18.0);
    let cw = Loop2 {
        segs: vec![
            Seg2::Arc {
                center: v(5.0, 5.0),
                radius: 18.0,
                start: 0.0,
                sweep: -PI,
            },
            Seg2::Arc {
                center: v(5.0, 5.0),
                radius: 18.0,
                start: -PI,
                sweep: -PI,
            },
        ],
    };
    for clip in [Clip::Whole, Clip::Free] {
        let s = with_clip(CellShape::Hexagon, clip);
        let (a, b) = (
            run(std::slice::from_ref(&ccw), &s),
            run(std::slice::from_ref(&cw), &s),
        );
        assert_eq!(counts(&a), counts(&b), "{clip:?}");
        assert!((a.open_area - b.open_area).abs() < 1e-6);
    }
}

#[test]
fn a_cell_grid_error_names_what_is_wrong() {
    let e = plan(
        &[],
        &CellGridSpec {
            size: 0.0,
            ..CellGridSpec::default()
        },
    )
    .unwrap_err();
    assert!(e.to_string().contains("cell size"));
}

#[test]
fn a_cells_regions_put_each_hole_in_the_outer_loop_that_holds_it() {
    // A small keep-out sits inside the cell at the anchor: that cell opens as its hexagon with
    // a hole, the others as one loop each.
    let p = run(
        &[rect(-20.0, -20.0, 40.0, 40.0), rect(-0.5, -0.5, 0.5, 0.5)],
        &with_clip(CellShape::Hexagon, Clip::Free),
    );
    let holed = p
        .cells
        .iter()
        .find(|c| c.center.x.abs() < 1e-9 && c.center.y.abs() < 1e-9)
        .unwrap();
    assert_eq!(holed.loops.len(), 2);
    let regions = holed.regions().unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].holes.len(), 1);
    let whole = p.cells.iter().find(|c| c.kind == CellKind::Whole).unwrap();
    let regions = whole.regions().unwrap();
    assert_eq!((regions.len(), regions[0].holes.len()), (1, 0));
    // Every cell's regions keep its loops.
    for c in &p.cells {
        let regions = c.regions().unwrap();
        let loops: usize = regions.iter().map(|r| 1 + r.holes.len()).sum();
        assert_eq!(loops, c.loops.len());
    }
}

#[test]
fn a_hole_of_an_island_belongs_to_the_island_not_to_the_outer_loop_around_it() {
    // An opening of four loops: the outer one, a hole in it, an island in the hole, and a hole
    // in the island. The last lies inside both outer loops.
    let reverse = |l: Loop2| {
        polygon(
            &l.segs
                .iter()
                .rev()
                .map(|s| (s.end().x, s.end().y))
                .collect::<Vec<_>>(),
        )
    };
    let cell = GridCell {
        i: 0,
        j: 0,
        kind: CellKind::Free,
        center: v(0.0, 0.0),
        loops: vec![
            rect(-10.0, -10.0, 10.0, 10.0),
            reverse(rect(-8.0, -8.0, 8.0, 8.0)),
            rect(-6.0, -6.0, 6.0, 6.0),
            reverse(rect(-1.0, -1.0, 1.0, 1.0)),
        ],
        area: 0.0,
    };
    let regions = cell.regions().unwrap();
    assert_eq!(regions.len(), 2);
    let sizes: Vec<(f64, usize)> = regions
        .iter()
        .map(|r| (r.outer.segs[0].start().x.abs(), r.holes.len()))
        .collect();
    assert!(
        sizes.contains(&(10.0, 1)) && sizes.contains(&(6.0, 1)),
        "{sizes:?}"
    );
}

#[test]
fn a_loop_of_many_pieces_is_counted_once() {
    // A 632-sided polygon: 632 pieces, not the 200,028 a running count of the loop's own prefix gave.
    let n = 632;
    let pts: Vec<(f64, f64)> = (0..n)
        .map(|k| {
            let a = 2.0 * PI * f64::from(k) / f64::from(n);
            (50.0 * det::cos(a), 50.0 * det::sin(a))
        })
        .collect();
    let p = run(
        &[polygon(&pts)],
        &with_clip(CellShape::Hexagon, Clip::Whole),
    );
    assert!(!p.cells.is_empty());
}

#[test]
fn a_curve_that_needs_more_than_the_piece_cap_is_refused() {
    // A cubic whose control points are far apart cannot be followed to within the tolerance in 4096 pieces.
    let big = 1.0e7;
    let field = Loop2 {
        segs: vec![
            Seg2::Cubic {
                p0: v(0.0, 0.0),
                p1: v(big, big),
                p2: v(-big, big),
                p3: v(10.0, 0.0),
            },
            Seg2::Line {
                a: v(10.0, 0.0),
                b: v(0.0, 0.0),
            },
        ],
    };
    let e = plan(&[field], &spec(CellShape::Hexagon))
        .unwrap_err()
        .to_string();
    assert!(e.contains("too complex"), "{e}");
}

#[test]
fn a_bezier_of_degree_two_is_a_curve_like_any_other() {
    let mut p = [v(0.0, 0.0); crate::prism::profile::MAX_BEZIER_CTRL];
    p[1] = v(10.0, 8.0);
    p[2] = v(20.0, 0.0);
    let field = Loop2 {
        segs: vec![
            Seg2::Bezier { n: 2, p },
            Seg2::Line {
                a: v(20.0, 0.0),
                b: v(0.0, 0.0),
            },
        ],
    };
    assert!(plan(&[field], &with_clip(CellShape::Circle, Clip::Free)).is_ok());
}

#[test]
fn polygon_cells_keep_the_web_to_every_neighbour_on_both_grids() {
    // The cells of a polygon are wider than their inscribed diameter (a triangle's circle is half
    // its corners' reach), so the pitch grows with the outline: no two cells come nearer than the web.
    for grid in [DotGrid::Square, DotGrid::Hexagonal] {
        for sides in [3u32, 5, 6, 8] {
            let s = CellGridSpec {
                shape: CellShape::Polygon,
                sides,
                dot_grid: grid,
                clip: Clip::Whole,
                ..spec(CellShape::Polygon)
            };
            let p = run(&[rect(-40.0, -40.0, 40.0, 40.0)], &s);
            let polys: Vec<Vec<Vec2>> = p
                .cells
                .iter()
                .map(|c| {
                    c.loops
                        .iter()
                        .flat_map(|l| l.segs.iter().map(Seg2::start))
                        .collect()
                })
                .collect();
            let edge_distance = |q: Vec2, a: Vec2, b: Vec2| {
                let d = b - a;
                let t = ((q - a).dot(d) / d.dot(d)).clamp(0.0, 1.0);
                q.distance(a + d * t)
            };
            for (n, a) in polys.iter().enumerate() {
                for b in polys.iter().skip(n + 1) {
                    for &q in a {
                        for m in 0..b.len() {
                            let gap = edge_distance(q, b[m], b[(m + 1) % b.len()]);
                            assert!(gap >= W - 1e-9, "{sides} sides on {grid:?}: {gap} < {W}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn a_small_loop_far_from_the_origin_keeps_its_area() {
    let far = 1.0e8;
    let lp = frame_rect(far, far, far + 1.0, far + 1.0);
    assert!(
        (lp.signed_area() - 1.0).abs() < 1e-6,
        "{}",
        lp.signed_area()
    );
}

#[test]
fn a_long_straight_boundary_of_many_collinear_pieces_snaps_to_the_same_cells() {
    let n = 20_000;
    let width = 600.0;
    let mut pts = vec![(0.0, 0.0)];
    pts.extend((1..n).map(|k| (width * f64::from(k) / f64::from(n), 0.0)));
    pts.extend([(width, 0.0), (width, 60.0), (0.0, 60.0)]);
    let many = run(&[polygon(&pts)], &with_clip(CellShape::Hexagon, Clip::Snap));
    let plain = run(
        &[rect(0.0, 0.0, width, 60.0)],
        &with_clip(CellShape::Hexagon, Clip::Snap),
    );
    assert_eq!(counts(&many), counts(&plain));
    assert!((many.open_area - plain.open_area).abs() < 1e-6);
}

#[test]
fn a_tiny_step_on_a_very_long_edge_is_not_merged_away() {
    // The vertex a hundredth of a millimetre off the line of a 50 km edge is a turn of under a
    // nanoradian each side, but it is still a corner.
    let ring = vec![
        (v(0.0, 0.0), v(2.5e7, 0.01)),
        (v(2.5e7, 0.01), v(5.0e7, 0.0)),
        (v(5.0e7, 0.0), v(5.0e7, 100.0)),
        (v(5.0e7, 100.0), v(0.0, 100.0)),
        (v(0.0, 100.0), v(0.0, 0.0)),
    ];
    assert_eq!(snap::merge_collinear(ring).len(), 5);
    let straight = vec![
        (v(0.0, 0.0), v(25.0, 0.0)),
        (v(25.0, 0.0), v(50.0, 0.0)),
        (v(50.0, 0.0), v(50.0, 10.0)),
        (v(50.0, 10.0), v(0.0, 10.0)),
        (v(0.0, 10.0), v(0.0, 0.0)),
    ];
    assert_eq!(snap::merge_collinear(straight).len(), 4);
}

#[test]
fn a_triangle_grid_works_at_the_extreme_indices() {
    let lat = lattice(CellShape::Triangle);
    // Neither the centre nor the key adds the indices.
    let _ = lat.center(i64::MAX, 1);
    assert!(lat.key(i64::MAX, 1) <= 1);
}

fn has(p: &CellGridPlan, i: i64, j: i64) -> bool {
    p.cells.iter().any(|c| c.i == i && c.j == j)
}

fn with_margin(shape: CellShape, margin: f64) -> CellGridSpec {
    CellGridSpec {
        size: 10.0,
        web: 2.0,
        margin,
        ..with_clip(shape, Clip::Whole)
    }
}

/// A circle of radius `r` about the origin as four rational quadratic arcs (exact).
fn conic_circle(r: f64) -> Loop2 {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let quarter = |a: (f64, f64), apex: (f64, f64), b: (f64, f64)| Seg2::Conic {
        a: v(a.0 * r, a.1 * r),
        apex: v(apex.0 * r, apex.1 * r),
        b: v(b.0 * r, b.1 * r),
        w,
    };
    Loop2 {
        segs: vec![
            quarter((1.0, 0.0), (1.0, 1.0), (0.0, 1.0)),
            quarter((0.0, 1.0), (-1.0, 1.0), (-1.0, 0.0)),
            quarter((-1.0, 0.0), (-1.0, -1.0), (0.0, -1.0)),
            quarter((0.0, -1.0), (1.0, -1.0), (1.0, 0.0)),
        ],
    }
}

#[test]
fn a_round_cell_is_tested_as_its_true_circle_not_the_polygon_round_it() {
    // The cell is a circle of radius 2.5 at the origin; the field's circle is 0.001 mm wider. The
    // 64-sided polygon round the cell reaches 2.503 and would cross the field's edge.
    let spec = CellGridSpec {
        size: 5.0,
        web: 1.0,
        ..with_clip(CellShape::Circle, Clip::Whole)
    };
    let p = run(&[circle(0.0, 0.0, 2.501)], &spec);
    assert!(
        has(&p, 0, 0) && p.cells.len() == 1,
        "{} cells",
        p.cells.len()
    );
    // The same field drawn as conics, measured on chords less their bound.
    // (its chords are within 0.001 of it, so it is kept from 0.002 wider than the cell on).
    let q = run(&[conic_circle(2.504)], &spec);
    assert!(
        has(&q, 0, 0) && q.cells.len() == 1,
        "{} cells",
        q.cells.len()
    );
    // A field narrower than the cell keeps nothing, on either.
    assert!(run(&[circle(0.0, 0.0, 2.499)], &spec).cells.is_empty());
    assert!(run(&[conic_circle(2.499)], &spec).cells.is_empty());
}

#[test]
fn an_arc_keep_out_drops_the_cells_it_reaches_and_not_the_ones_beside_it() {
    // Square cells of 10 mm on a 12 mm pitch: cell (1, 0) spans 7 to 17 in X, so a keep-out circle
    // of radius 6.99 about the origin comes 0.01 short of it and one of 7.01 crosses it.
    let field = |r: f64| [rect(-60.0, -60.0, 60.0, 60.0), circle(0.0, 0.0, r)];
    let beside = run(&field(6.99), &with_margin(CellShape::Square, 0.0));
    assert!(has(&beside, 1, 0) && has(&beside, 0, 1) && has(&beside, -1, 0) && has(&beside, 0, -1));
    assert!(!has(&beside, 0, 0));
    let over = run(&field(7.01), &with_margin(CellShape::Square, 0.0));
    assert!(!has(&over, 1, 0) && !has(&over, 0, 1) && !has(&over, -1, 0) && !has(&over, 0, -1));
}

#[test]
fn a_margin_drops_the_cells_nearer_than_it_and_zero_is_whole() {
    let field = [rect(-60.0, -60.0, 60.0, 60.0), circle(0.0, 0.0, 6.4)];
    let sized = CellGridSpec {
        size: 10.0,
        web: 2.0,
        ..with_clip(CellShape::Square, Clip::Whole)
    };
    let whole = run(&field, &sized);
    let zero = run(&field, &with_margin(CellShape::Square, 0.0));
    assert_eq!(zero, whole);
    // Cell (1, 0) is 0.6 from the keep-out circle.
    assert!(has(&zero, 1, 0));
    assert!(has(
        &run(&field, &with_margin(CellShape::Square, 0.5)),
        1,
        0
    ));
    let wide = run(&field, &with_margin(CellShape::Square, 0.7));
    assert!(!has(&wide, 1, 0) && !has(&wide, 0, 1));
    // The cells beside the field's own edge are measured too: the ring next to x = 60 is 7 from it.
    assert!(has(
        &run(&field, &with_margin(CellShape::Square, 6.9)),
        4,
        1
    ));
    assert!(!has(
        &run(&field, &with_margin(CellShape::Square, 7.1)),
        4,
        1
    ));
    // A margin too small to matter changes nothing where no cell lies that close to an edge.
    let tiny = run(&field, &with_margin(CellShape::Square, 1e-7));
    assert_eq!(tiny.cells.len(), zero.cells.len());
}

#[test]
fn a_margin_that_is_negative_not_a_number_or_huge_is_refused() {
    let field = [rect(-20.0, -20.0, 20.0, 20.0)];
    for m in [-1.0, f64::NAN, f64::INFINITY, 1e300, MAX_COORD * 2.0] {
        let e = plan(&field, &with_margin(CellShape::Square, m)).unwrap_err();
        assert!(matches!(e, CellGridError::Parameter(_)), "{m}: {e}");
    }
    // A margin needs whole cells: snapped and trimmed ones meet the edge by design.
    for clip in [Clip::Snap, Clip::Free] {
        let spec = CellGridSpec {
            margin: 1.0,
            ..with_clip(CellShape::Square, clip)
        };
        assert!(matches!(
            plan(&field, &spec),
            Err(CellGridError::Parameter(_))
        ));
    }
}

#[test]
fn a_spline_loop_does_not_stop_a_straight_keep_out_from_snapping_its_neighbours() {
    // The keep-out's edges lie off the lattice lines and snap outwards onto them. A field with a
    // spline in its outer loop must cut the cells beside the keep-out on those lines, as a field
    // of straight edges does, not keep them whole on the strength of the keep-out as drawn.
    let keep_out = rect(18.0, 18.0, 25.0, 25.0);
    let bulged = Loop2 {
        segs: vec![
            Seg2::Line {
                a: v(0.0, 0.0),
                b: v(40.0, 0.0),
            },
            Seg2::Cubic {
                p0: v(40.0, 0.0),
                p1: v(55.0, 12.0),
                p2: v(55.0, 28.0),
                p3: v(40.0, 40.0),
            },
            Seg2::Line {
                a: v(40.0, 40.0),
                b: v(0.0, 40.0),
            },
            Seg2::Line {
                a: v(0.0, 40.0),
                b: v(0.0, 0.0),
            },
        ],
    };
    let spec = with_clip(CellShape::Hexagon, Clip::Snap);
    let near = |p: &CellGridPlan| {
        let mut cells: Vec<(i64, i64, CellKind)> = p
            .cells
            .iter()
            .filter(|c| (c.center - v(21.5, 21.5)).length() < 11.0)
            .map(|c| (c.i, c.j, c.kind))
            .collect();
        cells.sort_by_key(|c| (c.0, c.1));
        cells
    };
    let curved = near(&run(&[bulged, keep_out.clone()], &spec));
    let straight = near(&run(&[rect(0.0, 0.0, 40.0, 40.0), keep_out], &spec));
    assert!(
        straight.iter().any(|c| c.2 == CellKind::Exact),
        "the keep-out cuts cells on lattice lines"
    );
    assert_eq!(curved, straight);
}

// --- exact distance: chords less their bound on curves, stable at large coordinates ---

/// The first quarter of the cubic `(1e6 (t - 0.002)(t - 0.006)(t + 0.0015), t)`, as a Bézier: it
/// dips to within 0.00036 of (-0.005, 0.002) in a basin about 0.004 wide in `t`, narrower than the
/// 1/64 of the curve between two samples.
fn dipping_cubic() -> Seg2 {
    const SPAN: f64 = 0.25;
    let x = |t: f64| 1e6 * (t - 0.002) * (t - 0.006) * (t + 0.0015);
    let dx = |t: f64| {
        1e6 * ((t - 0.006) * (t + 0.0015) + (t - 0.002) * (t + 0.0015) + (t - 0.002) * (t - 0.006))
    };
    // Its control points: the ends and a third of the end tangents (in the parameter 0..1).
    let (e0, e1) = (v(x(0.0), 0.0), v(x(SPAN), SPAN));
    let (t0, t1) = (v(SPAN * dx(0.0), SPAN), v(SPAN * dx(SPAN), SPAN));
    Seg2::Cubic {
        p0: e0,
        p1: e0 + t0 / 3.0,
        p2: e1 - t1 / 3.0,
        p3: e1,
    }
}

#[test]
fn a_curve_dipping_between_samples_drops_the_cell_it_reaches() {
    use crate::cell_grid::exact::{CellOutline, Edge, ExactField};
    let s = dipping_cubic();
    let q = v(-0.005, 0.002);
    let truth = (0..=2_000_000)
        .map(|i| s.point_at(f64::from(i) / 2_000_000.0).distance(q))
        .fold(f64::INFINITY, f64::min);
    assert!(truth < 0.0004 && truth > 0.0003, "{truth}");
    // 64 samples (and Newton from them) land far from the dip: the basin is missed, 0.017 away.
    let sampled = (0..=64)
        .map(|i| s.point_at(f64::from(i) / 64.0).distance(q))
        .fold(f64::INFINITY, f64::min);
    assert!(sampled > 0.01, "{sampled}");
    let edge = Edge::of(&s).unwrap();
    assert!(edge.within_point(q, truth * 1.01));
    let field = ExactField::new(vec![edge], 1.0);
    // Round cells about the point, of radius 0.0004 (the curve is inside it) and 0.0003 (a hair
    // clear, inside the chords' bound): both dropped, never kept.
    for r in [0.0004, 0.0003] {
        assert!(
            field.meets(&CellOutline::Circle { c: q, r }, 0.0),
            "radius {r}"
        );
    }
    // Far enough off, a round cell is kept; and a margin reaches the curve from where it stands.
    assert!(!field.meets(
        &CellOutline::Circle {
            c: v(-1.0, 0.002),
            r: 0.1
        },
        0.0
    ));
    assert!(field.meets(
        &CellOutline::Circle {
            c: v(-1.0, 0.002),
            r: 0.1
        },
        0.9
    ));
}

#[test]
fn a_curve_too_fine_to_follow_is_refused_not_guessed_at() {
    // A cubic that takes more than the allowed pieces to cut to within 0.001 mm.
    let wild = Loop2 {
        segs: vec![
            Seg2::Cubic {
                p0: v(0.0, 0.0),
                p1: v(0.0, 1e5),
                p2: v(1e5, 1e5),
                p3: v(1e5, 0.0),
            },
            Seg2::Line {
                a: v(1e5, 0.0),
                b: v(0.0, 0.0),
            },
        ],
    };
    let spec = with_clip(CellShape::Hexagon, Clip::Whole);
    match plan(&[wild], &spec) {
        Err(CellGridError::Field(m)) => assert!(m.contains("too complex"), "{m}"),
        other => panic!("{other:?}"),
    }
}

/// The distance from `p` to the segment `ab`, the long way.
fn dist_to_side(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let t = ((p - a).dot(d) / d.dot(d)).clamp(0.0, 1.0);
    p.distance(a + d * t)
}

#[test]
fn square_and_hexagon_cells_keep_their_margin_from_a_cubic_within_the_chords_bound() {
    use crate::cell_grid::exact::{CellOutline, Edge, ExactField};
    // An arch peaking at (13, 7.5); cells above it. A square with a flat side toward it, a hexagon
    // with a flat side, and one with a corner toward it: the segment against the curve each way.
    let arch = Seg2::Cubic {
        p0: v(0.0, 0.0),
        p1: v(8.0, 10.0),
        p2: v(18.0, 10.0),
        p3: v(26.0, 0.0),
    };
    let ngon = |c: Vec2, r: f64, n: usize, turn: f64| -> Vec<Vec2> {
        (0..n)
            .map(|k| {
                let a = turn + std::f64::consts::TAU * k as f64 / n as f64;
                c + v(det::cos(a), det::sin(a)) * r
            })
            .collect()
    };
    let cells = [
        ngon(v(13.0, 10.5), 2.0 * 2.0_f64.sqrt(), 4, FRAC_PI_6 * 1.5),
        ngon(v(13.0, 10.5), 2.0, 6, 0.0),
        ngon(v(13.0, 10.5), 2.0, 6, FRAC_PI_6 * 3.0),
        ngon(v(14.3, 10.5), 2.0, 6, FRAC_PI_6),
    ];
    let field = ExactField::new(vec![Edge::of(&arch).unwrap()], 1.0);
    for (k, pts) in cells.into_iter().enumerate() {
        let n = pts.len();
        // The true clearance, from a dense reference of the curve, not the code under test.
        let truth = (0..=400_000)
            .map(|i| arch.point_at(f64::from(i) / 400_000.0))
            .map(|p| {
                (0..n)
                    .map(|j| dist_to_side(p, pts[j], pts[(j + 1) % n]))
                    .fold(f64::INFINITY, f64::min)
            })
            .fold(f64::INFINITY, f64::min);
        assert!(truth > 0.5 && truth < 3.0, "cell {k}: {truth}");
        let convex = Convex::new(pts).unwrap();
        let outline = CellOutline::Polygon(&convex);
        // Two chord bounds (2 * 1e-3) and a little rounding under the clearance: kept.
        assert!(
            !field.meets(&outline, truth - 2.0e-3 - 1e-6),
            "cell {k}: kept at {}",
            truth - 2.0e-3 - 1e-6
        );
        // A hair over it: dropped, as it is for any margin beyond.
        assert!(field.meets(&outline, truth + 1e-4), "cell {k}: dropped");
        assert!(field.meets(&outline, truth + 0.5), "cell {k}: dropped far");
    }
}

#[test]
fn a_conic_and_a_segment_are_measured_on_the_curve_not_its_samples() {
    use crate::cell_grid::exact::Edge;
    // A quarter of the unit circle as a conic: the segment x + y = 2 is sqrt(2) - 1 from it.
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let edge = Edge::of(&Seg2::Conic {
        a: v(1.0, 0.0),
        apex: v(1.0, 1.0),
        b: v(0.0, 1.0),
        w,
    })
    .unwrap();
    let gap = 2.0_f64.sqrt() - 1.0;
    let (a, b) = (v(2.0, 0.0), v(0.0, 2.0));
    assert!(!edge.within_segment(a, b, gap - 2.1e-3));
    assert!(edge.within_segment(a, b, gap + 1e-6));
    assert!(edge.within_segment(v(0.5, 0.5), v(2.0, 2.0), 0.0));
}

#[test]
fn a_segment_far_from_the_origin_keeps_its_clearance_from_an_arc() {
    use crate::cell_grid::exact::Edge;
    let unit = |a: f64| v(det::cos(a), det::sin(a));
    let full = Edge::of(&Seg2::Arc {
        center: v(0.0, 0.0),
        radius: 1.0,
        start: 0.0,
        sweep: std::f64::consts::TAU,
    })
    .unwrap();
    let quarter = Edge::of(&Seg2::Arc {
        center: v(0.0, 0.0),
        radius: 1.0,
        start: 0.0,
        sweep: FRAC_PI_2,
    })
    .unwrap();
    // The segment 2 mm off the centre, 5e8 long, turned to several sides: 1 mm clear of the
    // circle, whatever the turn; and the same line moved to 0.5 mm crosses it.
    for degrees in [0.0, 30.0, 45.0, 60.0, 90.0, 135.0, 225.0] {
        let t = f64::to_radians(degrees);
        let (u, n) = (unit(t), unit(t + FRAC_PI_2));
        for (offset, clear) in [(2.0, true), (0.5, false)] {
            let (a, b) = (n * offset - u * 2.5e8, n * offset + u * 2.5e8);
            assert_eq!(full.within_segment(a, b, 0.9), !clear, "{degrees} {offset}");
            if clear {
                assert!(full.within_segment(a, b, 1.01), "{degrees} reaches 1 mm");
            }
        }
    }
    // Axis-aligned, the case of the report: exactly 1 mm clear, the segment from (-2.5e8, 2).
    let (a, b) = (v(-2.5e8, 2.0), v(2.5e8, 2.0));
    assert!(!full.within_segment(a, b, 0.999));
    assert!(full.within_segment(a, b, 1.001));
    // A quarter arc the line crosses outside of: clear by the arc's end, 0.5 mm.
    let (a, b) = (v(-2.5e8, -0.5), v(2.5e8, -0.5));
    assert!(!quarter.within_segment(a, b, 0.49));
    assert!(quarter.within_segment(a, b, 0.51));
}

#[test]
fn conic_chords_stay_within_the_chord_bound_of_the_curve_for_every_weight() {
    // `flatten` promises chords within 0.001 mm of the curve and the curve within it of the
    // chords; the keep-or-drop of a cell leans on it. Every dense sample of the curve (uniform in
    // the parameter, and crowding its ends and its middle, where a large weight turns) lies
    // within that of the chords, and every chord's midpoint within that of the curve.
    const TOL: f64 = 1e-3;
    const DENSE: usize = 20_000;
    let to_polyline = |pts: &[Vec2], p: Vec2| {
        pts.windows(2)
            .map(|w| dist_to_side(p, w[0], w[1]))
            .fold(f64::INFINITY, f64::min)
    };
    // Parameters as pairs `(s, t)` with `s + t = 1`, each held to full precision: near an end of
    // the curve of a large weight a rounded `t` alone moves the point by more than the tolerance.
    let mut ts: Vec<(f64, f64)> = (0..=DENSE)
        .map(|i| (1.0 - i as f64 / DENSE as f64, i as f64 / DENSE as f64))
        .collect();
    for k in 0..200 {
        let e = det::powf(10.0_f64, -f64::from(k) / 8.0) * 0.5;
        ts.extend([
            (1.0 - e, e),
            (e, 1.0 - e),
            (0.5 + e, 0.5 - e),
            (0.5 - e, 0.5 + e),
        ]);
    }
    ts.sort_by(|p, q| p.1.total_cmp(&q.1));
    // The point of the conic at `(s, t)`: the weighted mean of its controls, every term positive.
    let conic_at = |a: Vec2, apex: Vec2, b: Vec2, w: f64, (s, t): (f64, f64)| {
        let (ka, kb, kw) = (s * s, t * t, 2.0 * w * s * t);
        (a * ka + apex * kw + b * kb) / (ka + kw + kb)
    };
    for w in [1e-12, 1e-6, 0.05, 1.0, 300.0, 1e6, 1e12] {
        for apex in [v(5.0, 8.0), v(5.0, -3.0), v(9.0, 1.0), v(-40.0, 25.0)] {
            let seg = Seg2::Conic {
                a: v(0.0, 0.0),
                apex,
                b: v(10.0, 0.0),
                w,
            };
            // None of these needs more than a few hundred pieces: none is refused.
            let chords =
                flatten(&seg, TOL).unwrap_or_else(|| panic!("w {w} apex {apex:?} refused"));
            let dense: Vec<Vec2> = ts
                .iter()
                .map(|&st| conic_at(v(0.0, 0.0), apex, v(10.0, 0.0), w, st))
                .collect();
            let worst = dense
                .iter()
                .map(|&p| to_polyline(&chords, p))
                .fold(0.0, f64::max);
            assert!(
                worst <= TOL,
                "w {w} apex {apex:?}: a point of the curve is {worst} from the chords"
            );
            for pair in chords.windows(2) {
                let mid = (pair[0] + pair[1]) * 0.5;
                let near = to_polyline(&dense, mid);
                assert!(
                    near <= TOL,
                    "w {w} apex {apex:?}: a chord's midpoint is {near} from the curve"
                );
            }
        }
    }
    // A conic too large for 4096 chords within 0.001 mm is refused, not followed coarsely.
    let big = Seg2::Conic {
        a: v(0.0, 0.0),
        apex: v(5e5, 1e6),
        b: v(1e6, 0.0),
        w: 1.0,
    };
    assert!(flatten(&big, TOL).is_none());
    // And a weight that is not positive and finite is refused.
    for w in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let seg = Seg2::Conic {
            a: v(0.0, 0.0),
            apex: v(5.0, 8.0),
            b: v(10.0, 0.0),
            w,
        };
        assert!(flatten(&seg, TOL).is_none(), "w {w}");
    }
}

#[test]
fn a_conic_whose_chord_is_too_short_to_measure_stays_within_the_tolerance_of_its_chords() {
    // The squared length of the chord underflows to a subnormal: the height from it is wrong,
    // so the piece must be bounded by the apex's distance to the nearer end instead.
    const TOL: f64 = 1e-3;
    let (a, apex, b, w) = (
        v(0.0, 0.0),
        v(5.035e-162, 0.0010001),
        v(1.007e-161, 0.0),
        1e12,
    );
    let seg = Seg2::Conic { a, apex, b, w };
    let Some(chords) = flatten(&seg, TOL) else {
        return;
    };
    let conic_at = |t: f64| {
        let s = 1.0 - t;
        let (ka, kb, kw) = (s * s, t * t, 2.0 * w * s * t);
        (a * ka + apex * kw + b * kb) / (ka + kw + kb)
    };
    for i in 0..=20_000 {
        let p = conic_at(f64::from(i) / 20_000.0);
        let near = chords
            .windows(2)
            .map(|c| dist_to_side(p, c[0], c[1]))
            .fold(f64::INFINITY, f64::min);
        assert!(near <= TOL, "t {i}/20000: {near} from the chords");
    }
}

// --- sketched cells ---

/// An L of lines with a half-circle cap on its stem: the foot is `[0, 10] x [0, 4]`, the stem
/// `[0, 4] x [4, 10]`, and the arc from `(4, 10)` over `(2, 12)` to `(0, 10)` (centre `(2, 10)`,
/// radius 2). Area `40 + 24 + 2 pi`.
fn sk_l_arc() -> Loop2 {
    let line = |a: (f64, f64), b: (f64, f64)| Seg2::Line {
        a: v(a.0, a.1),
        b: v(b.0, b.1),
    };
    Loop2 {
        segs: vec![
            line((0.0, 0.0), (10.0, 0.0)),
            line((10.0, 0.0), (10.0, 4.0)),
            line((10.0, 4.0), (4.0, 4.0)),
            line((4.0, 4.0), (4.0, 10.0)),
            Seg2::Arc {
                center: v(2.0, 10.0),
                radius: 2.0,
                start: 0.0,
                sweep: PI,
            },
            line((0.0, 10.0), (0.0, 0.0)),
        ],
    }
}

const SK_L_AREA: f64 = 64.0 + 2.0 * PI;

fn sk_cell(profile: Loop2, origin: Option<Vec2>, px: f64, py: f64, stagger: f64) -> SketchCell {
    SketchCell {
        profile,
        origin,
        pitch_x: px,
        pitch_y: py,
        stagger,
        cell_angle: 0.0,
        flip: false,
    }
}

/// An L cell with its origin at the profile's zero.
fn sk_l(px: f64, py: f64, stagger: f64) -> SketchCell {
    sk_cell(sk_l_arc(), Some(v(0.0, 0.0)), px, py, stagger)
}

/// A square cell of side 10 on pitch 12 (a web of 2 apart), the origin at its centre, so with
/// `anchor` (5, 5) cell `(i, j)` covers `[12 i, 12 i + 10] x [12 j, 12 j + 10]`.
fn sk_square() -> SketchCell {
    sk_cell(rect(0.0, 0.0, 10.0, 10.0), None, 12.0, 12.0, 0.0)
}

fn sk_spec(clip: Clip) -> CellGridSpec {
    CellGridSpec {
        clip,
        web: 2.0,
        anchor: v(5.0, 5.0),
        ..CellGridSpec::default()
    }
}

fn sk_run(field: &[Loop2], cell: &SketchCell, spec: &CellGridSpec) -> CellGridPlan {
    plan_sketch(field, cell, spec).unwrap()
}

fn sk_close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} is not {b}");
}

fn sk_points(cell: &GridCell) -> Vec<Vec2> {
    cell.loops
        .iter()
        .flat_map(|l| l.segs.iter().map(Seg2::start))
        .collect()
}

#[test]
fn square_cells_on_a_square_lattice_are_kept_whole_or_trimmed_by_the_field() {
    // Cell (i, j) covers [12 i, 12 i + 10] in x and likewise in y; the field is [0, 100]^2.
    // Whole: 12 i >= 0 and 12 i + 10 <= 100 give i = 0..=7 (the last ends at 94, the next at 106),
    // 8 x 8 = 64 cells of area 100, the first column and row flush with the field's edges.
    let field = [rect(0.0, 0.0, 100.0, 100.0)];
    let whole = sk_run(&field, &sk_square(), &sk_spec(Clip::Whole));
    assert_eq!((whole.whole, whole.partial, whole.cells.len()), (64, 0, 64));
    sk_close(whole.open_area, 6400.0);
    // Free: the ninth column and row (i = 8, from x = 96) are cut to 4 wide, so 81 cells: 64 whole
    // and 8 + 8 + 1 = 17 trimmed. Each axis is covered 8 x 10 + 4 = 84, so the area is 84^2 = 7056.
    let free = sk_run(&field, &sk_square(), &sk_spec(Clip::Free));
    assert_eq!((free.whole, free.partial, free.cells.len()), (64, 17, 81));
    sk_close(free.open_area, 7056.0);
}

#[test]
fn a_keep_out_removes_the_cells_it_covers_whole_and_trims_those_it_cuts_free() {
    // Keep-out [40, 60]^2. The cells overlapping it are i, j in {3, 4}: [36, 46] and [48, 58]
    // (i = 2 ends at 34, i = 5 starts at 60 and only touches it, which keeps it). That is 4 cells.
    // Whole: 64 - 4 = 60 cells, area 6000.
    let field = [rect(0.0, 0.0, 100.0, 100.0), rect(40.0, 40.0, 60.0, 60.0)];
    let whole = sk_run(&field, &sk_square(), &sk_spec(Clip::Whole));
    assert_eq!((whole.whole, whole.partial), (60, 0));
    sk_close(whole.open_area, 6000.0);
    // Free: of the keep-out, the cells cover x in [40, 46] and [48, 58] (6 + 10 = 16 wide, the
    // same in y), 16 x 16 = 256 mm2, so the area is 7056 - 256 = 6800. Cell (4, 4) lies wholly
    // inside the keep-out and goes; (3, 3) keeps 100 - 36 = 64, (3, 4) and (4, 3) keep 100 - 60 = 40
    // each. So 80 cells: 60 whole and 17 + 3 = 20 trimmed (6000 + 656 + 144 = 6800).
    let free = sk_run(&field, &sk_square(), &sk_spec(Clip::Free));
    assert_eq!((free.whole, free.partial, free.cells.len()), (60, 20, 80));
    sk_close(free.open_area, 6800.0);
    assert!(!free.cells.iter().any(|c| (c.i, c.j) == (4, 4)));
    let cut = free.cells.iter().find(|c| (c.i, c.j) == (3, 3)).unwrap();
    sk_close(cut.area, 64.0);
}

#[test]
fn a_keep_out_may_follow_a_different_mode_than_the_field() {
    // Field mode whole, keep-out mode free: the cells at the field's edge (none cross it here, all
    // 64 fit) are as before, the four cells the keep-out cuts are trimmed, and (4, 4) goes.
    let field = [rect(0.0, 0.0, 100.0, 100.0), rect(40.0, 40.0, 60.0, 60.0)];
    let spec = CellGridSpec {
        keepout_clip: Some(Clip::Free),
        ..sk_spec(Clip::Whole)
    };
    let plan = sk_run(&field, &sk_square(), &spec);
    assert_eq!((plan.whole, plan.partial), (60, 3));
    sk_close(plan.open_area, 6000.0 + 144.0);
}

#[test]
fn a_curved_field_edge_touching_a_cell_drops_it_when_the_fields_mode_is_whole_whatever_the_keep_outs(
) {
    // A margin needs every mode whole, so the split of the exact check between the field's edges
    // and the keep-outs' shows only at margin 0, on a curve (a curve is measured exactly, a line
    // or arc may touch the cell). The field is [0, 94] x [0, 100] with its right side a cubic
    // bulging out from (94, 0) to (94, 100). Cell i covers [12 i, 12 i + 10], so cell (7, 0) has
    // its corner exactly on the curve's end (94, 0); no other cell is within 0.002 mm of the
    // curve (the next nearest is 6 mm from its end), and the cells i, j = 0..=7 all lie inside the
    // field: 64. The curve touches (7, 0), so a whole field drops it: 63, the same with the
    // keep-outs free (there are none; their mode is not the field's).
    let curved = Loop2 {
        segs: vec![
            Seg2::Line {
                a: v(0.0, 0.0),
                b: v(94.0, 0.0),
            },
            Seg2::Cubic {
                p0: v(94.0, 0.0),
                p1: v(109.0, 12.0),
                p2: v(109.0, 88.0),
                p3: v(94.0, 100.0),
            },
            Seg2::Line {
                a: v(94.0, 100.0),
                b: v(0.0, 100.0),
            },
            Seg2::Line {
                a: v(0.0, 100.0),
                b: v(0.0, 0.0),
            },
        ],
    };
    let kept = |clip: Clip, keepout_clip: Option<Clip>| {
        let spec = CellGridSpec {
            keepout_clip,
            ..sk_spec(clip)
        };
        let plan = sk_run(std::slice::from_ref(&curved), &sk_square(), &spec);
        plan.cells
            .iter()
            .any(|c| (c.i, c.j) == (7, 0) && c.kind == CellKind::Whole)
    };
    assert!(
        !kept(Clip::Whole, None),
        "a whole field drops the cell the curve touches"
    );
    assert!(
        !kept(Clip::Whole, Some(Clip::Free)),
        "so does one whose keep-outs are free"
    );
}

#[test]
fn a_notch_a_keep_out_takes_from_a_large_cell_drops_it_though_it_is_a_tiny_share_of_its_area() {
    // Cells of 1e5 x 1e5 mm (area 1e10 mm2) on a pitch with a web of 2 between them, from cell
    // (0, 0) at [0, 1e5]^2. A keep-out of 2 x 2 straddles the right side of cell (0, 0) at its
    // middle, so it takes a notch of 1 x 2 = 2 mm2 from it. A keep-out of 2 x 0.1 straddles the
    // right side of cell (1, 1) (at x = 200002) and takes a notch 1 mm deep and 0.1 mm wide,
    // 0.1 mm2: 1e-11 of the cell, though 1e6 times the EPS it is measured by. Each cell is cut by
    // a whole keep-out, so both go; (0, 1) and (1, 0) are whole.
    let side = 1e5;
    let field = [
        rect(-10.0, -10.0, 2.0 * side + 12.0, 2.0 * side + 12.0),
        rect(side - 1.0, 4.9e4, side + 1.0, 4.9e4 + 2.0),
        rect(2.0 * side + 1.0, 1.5e5, 2.0 * side + 3.0, 1.5e5 + 0.1),
    ];
    let cell = sk_cell(
        rect(0.0, 0.0, side, side),
        None,
        side + 2.0,
        side + 2.0,
        0.0,
    );
    let spec = CellGridSpec {
        anchor: v(side / 2.0, side / 2.0),
        ..sk_spec(Clip::Whole)
    };
    let plan = sk_run(&field, &cell, &spec);
    let mut kept: Vec<(i64, i64)> = plan.cells.iter().map(|c| (c.i, c.j)).collect();
    kept.sort_unstable();
    assert_eq!(kept, vec![(0, 1), (1, 0)]);
}

#[test]
fn a_clip_edge_with_its_middle_near_a_spike_of_the_cell_is_not_the_cells_own() {
    // A 1e5 square cell with a slot from its top to 5e-7 above y = 0, and its bottom at -1e-5. A
    // whole keep-out (-1, -1) to (100001, 0) cuts a strip 1e-5 deep off the bottom, leaving the
    // clip edge y = 0 from x = 0 to 1e5. Its middle (5e4, 0) is 5e-7 (under EPS) from the slot's
    // tip, but its ends are 1e-5 from the bottom side, so it is not on one piece of the outline:
    // the cell was cut and, the keep-out being whole, it goes. The pitch is so large that this is
    // the only cell in the field.
    let line = |a: (f64, f64), b: (f64, f64)| Seg2::Line {
        a: v(a.0, a.1),
        b: v(b.0, b.1),
    };
    let pts = [
        (0.0, -1e-5),
        (1e5, -1e-5),
        (1e5, 1e5),
        (50100.0, 1e5),
        (50100.0, 100.0),
        (50000.0, 5e-7),
        (49900.0, 100.0),
        (49900.0, 1e5),
        (0.0, 1e5),
    ];
    let profile = Loop2 {
        segs: (0..pts.len())
            .filter_map(|k| Some(line(*pts.get(k)?, *pts.get((k + 1) % pts.len())?)))
            .collect(),
    };
    let cell = sk_cell(profile, Some(v(0.0, 0.0)), 3e5, 3e5, 0.0);
    let spec = CellGridSpec {
        anchor: v(0.0, 0.0),
        ..sk_spec(Clip::Whole)
    };
    let field = [
        rect(-5.0, -5.0, 100010.0, 100010.0),
        rect(-1.0, -1.0, 100001.0, 0.0),
    ];
    let plan = sk_run(&field, &cell, &spec);
    assert!(plan.cells.is_empty(), "{:?}", plan.cells.len());
}

#[test]
fn a_clipped_piece_is_the_cells_own_only_on_one_outline_piece_of_its_kind() {
    use crate::cell_grid::sketch::unchanged;
    let one = |prims: Vec<Prim>, cell: &[Prim]| unchanged(&[(prims, 1.0)], cell, 1.0);
    // An arc of radius 1000 about the origin from angle 0, sweeping 0.002 rad (a chord of 2 mm).
    let t = 0.002;
    let arc = Prim::arc(v(0.0, 0.0), 1000.0, 0.0, t);
    let cell = [arc];
    assert!(one(vec![arc], &cell), "the arc itself");
    assert!(
        one(
            vec![Prim::arc(v(0.0, 0.0), 1000.0, t / 4.0, t / 2.0)],
            &cell
        ),
        "a part of it"
    );
    assert!(
        !one(vec![Prim::arc(v(0.0, 0.0), 1000.0, t, -t)], &cell),
        "the same circle turned the other way"
    );
    assert!(
        !one(vec![Prim::arc(v(0.0, 0.0), 1000.0, -t / 2.0, t)], &cell),
        "starting before the outline arc, though inside the tolerance of its end"
    );
    // A circle of radius 1001 through the same two ends, bulging the same way. The chord is
    // c = 2 * 1000 * sin(0.001) = 2 mm, so the sagitta c^2 / (8 r) is 4 / 8 * (1 / 1000 - 1 / 1001)
    // = 5e-7 mm less at the middle: the ends and the middle are all within EPS of the outline
    // arc, yet the circle is another, so it is a change.
    let (p, q) = (arc.start(), arc.end());
    let m = (p + q) * 0.5;
    let half = p.distance(q) * 0.5;
    let toward = m * (-1.0 / m.length());
    let c2 = m + toward * (1001.0_f64 * 1001.0 - half * half).sqrt();
    let (a, b) = (
        det::atan2(p.y - c2.y, p.x - c2.x),
        det::atan2(q.y - c2.y, q.x - c2.x),
    );
    let other = Prim::arc(c2, 1001.0, a, b - a);
    assert!(arc.distance(other.start()) <= EPS && arc.distance(other.end()) <= EPS);
    assert!(
        arc.distance(other.mid()) <= EPS,
        "{}",
        arc.distance(other.mid())
    );
    assert!(!one(vec![other], &cell), "another circle");
    // Directed containment: an outline arc of radius 10 from 0 to 270 degrees (counter-clockwise).
    // A piece over 0..90 or 180..270 lies on it, one starting at its start (0..270) does, but the
    // piece 270..360 on the same circle has both its ends on the outline (at 270 and, at 360, its
    // start) and runs outside it, so it is not the cell's own.
    let big = Prim::arc(v(0.0, 0.0), 10.0, 0.0, 3.0 * FRAC_PI_2);
    let outline = [big];
    let part = |from: f64, sweep: f64| Prim::arc(v(0.0, 0.0), 10.0, from, sweep);
    assert!(one(vec![part(0.0, FRAC_PI_2)], &outline), "0..90");
    assert!(one(vec![part(PI, FRAC_PI_2)], &outline), "180..270");
    assert!(
        one(vec![part(0.0, 3.0 * FRAC_PI_2)], &outline),
        "the whole arc"
    );
    assert!(
        one(vec![part(-1e-9, FRAC_PI_2)], &outline),
        "a rounding before the start"
    );
    assert!(
        !one(vec![part(3.0 * FRAC_PI_2, FRAC_PI_2)], &outline),
        "270..360 runs outside"
    );
    // Lines: on one segment, or across two, and a line against an arc.
    let square = [
        Prim::line(v(0.0, 0.0), v(10.0, 0.0)),
        Prim::line(v(10.0, 0.0), v(10.0, 10.0)),
    ];
    assert!(one(vec![Prim::line(v(2.0, 0.0), v(9.0, 1e-7))], &square));
    assert!(
        !one(vec![Prim::line(v(5.0, 0.0), v(10.0, 5.0))], &square),
        "a corner cut"
    );
    assert!(
        !one(vec![Prim::line(p, q)], &cell),
        "a chord is not its arc"
    );
    assert!(!one(vec![arc], &square), "an arc is not a line");
}

#[test]
fn the_neighbour_budget_leaves_out_the_cell_itself() {
    // A strip 0.5 wide and 999.8 tall on pitch 1 along the row and 1001 between rows, the web 0.5:
    // the reach is sqrt(499.9^2 + 0.25^2) = 499.9, so neighbours are looked for within
    // l = 2 * 499.9 + 0.5 = 1000.3 mm, which holds the columns -1000..=1000 of the cell's own row
    // (the next row is 1001 off): 2000 neighbours, exactly the limit, so it is planned: the
    // columns i = 0..=9 are whole in [0, 10] x [0, 1000].
    let strip =
        |height: f64, pitch_y: f64| sk_cell(rect(0.0, 0.0, 0.5, height), None, 1.0, pitch_y, 0.0);
    let spec = CellGridSpec {
        web: 0.5,
        anchor: v(0.25, 499.9),
        ..sk_spec(Clip::Whole)
    };
    let plan = sk_run(
        &[rect(0.0, 0.0, 10.0, 1000.0)],
        &strip(999.8, 1001.0),
        &spec,
    );
    assert_eq!((plan.whole, plan.partial), (10, 0));
    // 1000.8 tall (l = 1001.3: columns -1001..=1001, 2002 neighbours) on rows 2000 apart: refused.
    match plan_sketch(
        &[rect(0.0, 0.0, 10.0, 1000.0)],
        &strip(1000.8, 2000.0),
        &spec,
    ) {
        Err(CellGridError::Layout(m)) => assert!(m.contains("neighbouring cells"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn rows_a_pitch_apart_with_few_neighbours_in_reach_are_planned() {
    // A strip cell 2098 x 0.5 on rows 1 mm apart, each row shifted 2100 along (pitch_x 1e6, stagger
    // 0.0021). The reach is 1049 mm, so neighbours are looked for within 2100 mm: 2100 rows, but
    // a row holds a neighbour only where its shift of 2100 per row, less whole pitches of 1e6,
    // lands within that: a few of them. The layout is fine (neighbours 2 mm apart along the rows),
    // so it is planned and cell (0, 0) is whole in the field [0, 3000] x [0, 10].
    let cell = sk_cell(rect(0.0, 0.0, 2098.0, 0.5), None, 1e6, 1.0, 0.0021);
    let spec = CellGridSpec {
        anchor: v(1049.0, 0.25),
        ..sk_spec(Clip::Whole)
    };
    let plan = sk_run(&[rect(0.0, 0.0, 3000.0, 10.0)], &cell, &spec);
    assert!(plan
        .cells
        .iter()
        .any(|c| (c.i, c.j) == (0, 0) && c.kind == CellKind::Whole));
}

#[test]
fn neighbours_are_counted_in_the_disc_of_the_reach_not_in_the_square_round_it() {
    // A square of 10 with its origin 130 mm off, on a pitch of 12 (the web 2 apart): the reach is
    // the corner (10, 10) seen from the origin, sqrt(140^2 + 10^2) = 140.357, so neighbours are
    // looked for within l = 2 * 140.357 + 2 = 282.71 mm = 23.56 pitches. The square of that
    // holds 47 x 47 = 2209 offsets, more than the 2000 allowed; the disc holds about
    // pi * 23.56^2 = 1744. The layout is fine, so it is planned, and the cells of the field
    // [0, 100]^2: x in [135 + 12 i, 145 + 12 i] whole for i = -11..=-4 (8 columns) and
    // y in [5 + 12 j, 15 + 12 j] for j = 0..=7 (8 rows).
    let cell = sk_cell(
        rect(0.0, 0.0, 10.0, 10.0),
        Some(v(-130.0, 0.0)),
        12.0,
        12.0,
        0.0,
    );
    let plan = sk_run(
        &[rect(0.0, 0.0, 100.0, 100.0)],
        &cell,
        &sk_spec(Clip::Whole),
    );
    assert_eq!((plan.whole, plan.partial), (64, 0));
}

#[test]
fn a_very_shallow_arc_still_moves_the_default_origin_by_its_cap() {
    // A 4e4 x 1000 rectangle whose top side is an arc of radius 5e8 mm bulging up: its sweep is
    // 8e-5 rad (under 1e-4), its sagitta 0.4 mm. The cap has the area r^2 t^3 / 12 = 10667 mm2
    // (t the sweep) and its centroid lies 0.2 r (t / 2)^2 = 0.16 mm above the chord, so the
    // centroid of the cell is y = (4e7 * 500 + 10667 * 1000.16) / (4e7 + 10667) mm and x = 2e4.
    let (r, t) = (5e8_f64, 8e-5_f64);
    let arc = Seg2::Arc {
        center: v(2e4, 1000.0 - r * det::cos(t / 2.0)),
        radius: r,
        start: FRAC_PI_2 - t / 2.0,
        sweep: t,
    };
    let line = |a: Vec2, b: Vec2| Seg2::Line { a, b };
    let profile = Loop2 {
        segs: vec![
            line(v(0.0, 0.0), v(4e4, 0.0)),
            line(v(4e4, 0.0), arc.start()),
            arc,
            line(arc.end(), v(0.0, 0.0)),
        ],
    };
    let cap = r * r * t * t * t / 12.0;
    let cy = (4e7 * 500.0 + cap * (1000.0 + 0.2 * r * (t / 2.0) * (t / 2.0))) / (4e7 + cap);
    let cell = sk_cell(profile, None, 5e4, 1100.0, 0.0);
    let spec = CellGridSpec {
        anchor: v(0.0, 0.0),
        ..sk_spec(Clip::Whole)
    };
    let plan = sk_run(&[rect(-3e4, -2e3, 3e4, 2e3)], &cell, &spec);
    let first = plan.cells.iter().find(|c| (c.i, c.j) == (0, 0)).unwrap();
    let start = first.loops[0].segs[0].start();
    assert!((start.x + 2e4).abs() < 1e-5, "{}", start.x);
    assert!((start.y + cy).abs() < 1e-5, "{} vs {}", start.y, -cy);
}

#[test]
fn staggered_rows_shift_by_half_a_pitch_at_a_stagger_of_one_half() {
    // b = (6, 12): row j is shifted by 6 j, so its cells start at x = 12 i + 6 j (a cell is 10
    // wide, rows 12 apart, so rows do not touch: a gap of 2 in y). In [0, 100] a row needs the
    // start in [0, 90]: even rows start at 0, 12, ..., 84 (8 cells) and odd rows at 6, 18, ..., 90
    // (8 cells, the last flush with x = 100). Rows start at y = 12 j <= 90: j = 0..=7. 64 cells.
    let field = [rect(0.0, 0.0, 100.0, 100.0)];
    let plan = sk_run(
        &field,
        &sk_cell(rect(0.0, 0.0, 10.0, 10.0), None, 12.0, 12.0, 0.5),
        &sk_spec(Clip::Whole),
    );
    assert_eq!((plan.whole, plan.partial), (64, 0));
    sk_close(plan.open_area, 6400.0);
    let cell = |i: i64, j: i64| plan.cells.iter().find(|c| (c.i, c.j) == (i, j)).unwrap();
    // Cell (0, 1) is half a pitch along from cell (0, 0) and a row up.
    let (c0, c1) = (cell(0, 0).center, cell(0, 1).center);
    sk_close(c1.x - c0.x, 6.0);
    sk_close(c1.y - c0.y, 12.0);
}

#[test]
fn a_skewed_stagger_shifts_every_row_by_its_fraction_of_the_pitch_more() {
    // stagger 0.3 on pitch 12: row j is `3.6 j` along. Anchor (5.5, 5), so a cell starts at
    // x0 = 0.5 + 12 i + 3.6 j and fits in [0, 100] for 0 <= x0 <= 90. The smallest start of row j
    // is 0.5 + (3.6 j mod 12): 0.5, 4.1, 7.7, 11.3, 2.9, 6.5, 10.1, 1.7 for j = 0..=7, and a row
    // holds floor((90 - start) / 12) + 1 cells: 8, 8, 7, 7, 8, 7, 7, 8 = 60. Rows start at
    // y = 12 j <= 90: j = 0..=7.
    let field = [rect(0.0, 0.0, 100.0, 100.0)];
    let spec = CellGridSpec {
        anchor: v(5.5, 5.0),
        ..sk_spec(Clip::Whole)
    };
    let cell = sk_cell(rect(0.0, 0.0, 10.0, 10.0), None, 12.0, 12.0, 0.3);
    let plan = sk_run(&field, &cell, &spec);
    assert_eq!((plan.whole, plan.partial), (60, 0));
    sk_close(plan.open_area, 6000.0);
}

#[test]
fn an_l_with_an_arc_fills_a_square_and_a_staggered_lattice() {
    // Origin at the profile's zero; the cell is 10 wide and 12 tall (the arc's top is at y = 12),
    // pitch 14 x 16, so cell (i, j) covers [x0, x0 + 10] x [16 j, 16 j + 12] with x0 = 14 i + s j,
    // s the row shift, and the nearest neighbours are 4 mm away: a web of 2 is kept. Whole in
    // [0, 100]^2 needs 16 j + 12 <= 100: j = 0..=5, and 0 <= x0 <= 90.
    let field = [rect(0.0, 0.0, 100.0, 100.0)];
    let spec = CellGridSpec {
        anchor: v(0.0, 0.0),
        ..sk_spec(Clip::Whole)
    };
    // Square lattice (s = 0): x0 = 0, 14, ..., 84 (7 cells) in each of 6 rows: 42.
    let square = sk_run(&field, &sk_l(14.0, 16.0, 0.0), &spec);
    assert_eq!(square.whole, 42);
    sk_close(square.open_area, 42.0 * SK_L_AREA);
    // Staggered 0.5 (s = 7 j): even rows start at 0, 14, ..., 84 (7 cells), odd rows at 7, 21,
    // ..., 77 (6 cells; the next, 91, is past 90). Rows 0, 2, 4 give 21 and rows 1, 3, 5 give 18.
    let staggered = sk_run(&field, &sk_l(14.0, 16.0, 0.5), &spec);
    assert_eq!(staggered.whole, 39);
    sk_close(staggered.open_area, 39.0 * SK_L_AREA);
}

#[test]
fn the_default_origin_is_the_centroid_of_the_outline_with_its_arcs() {
    // Parts: the foot (area 40, centroid (5, 2)), the stem (24, (2, 7)) and the half disc of the
    // cap (2 pi, (2, 10 + 4 r / (3 pi)) with r = 2). The centroid is their area-weighted mean.
    let area = 64.0 + 2.0 * PI;
    let cx = (40.0 * 5.0 + 24.0 * 2.0 + 2.0 * PI * 2.0) / area;
    let cy = (40.0 * 2.0 + 24.0 * 7.0 + 2.0 * PI * (10.0 + 8.0 / (3.0 * PI))) / area;
    let cell = sk_cell(sk_l_arc(), None, 14.0, 16.0, 0.0);
    let spec = CellGridSpec {
        anchor: v(50.0, 50.0),
        ..sk_spec(Clip::Whole)
    };
    let plan = sk_run(&[rect(0.0, 0.0, 100.0, 100.0)], &cell, &spec);
    let first = plan.cells.iter().find(|c| (c.i, c.j) == (0, 0)).unwrap();
    // Cell (0, 0) sits with its origin on the anchor, so the profile's zero is the centroid back.
    sk_close(first.center.x, 50.0);
    let start = first.loops[0].segs[0].start();
    sk_close(start.x, 50.0 - cx);
    sk_close(start.y, 50.0 - cy);
}

#[test]
fn a_cell_turned_on_its_own_stays_a_cell_about_its_origin() {
    // Squares of side 10 turned 30 degrees about their centres, pitch 20, anchor (50, 50): cell
    // (i, j) is centred at (50 + 20 i, 50 + 20 j). A turned square reaches 5 (cos 30 + sin 30) =
    // 6.83 each way, so centres 10, 30, 50, 70, 90 (i = -2..=2) fit in [0, 100]: 5 x 5 = 25 cells.
    let mut cell = sk_cell(rect(0.0, 0.0, 10.0, 10.0), None, 20.0, 20.0, 0.0);
    cell.cell_angle = PI / 6.0;
    let spec = CellGridSpec {
        anchor: v(50.0, 50.0),
        ..sk_spec(Clip::Whole)
    };
    let plan = sk_run(&[rect(0.0, 0.0, 100.0, 100.0)], &cell, &spec);
    assert_eq!((plan.whole, plan.partial), (25, 0));
    sk_close(plan.open_area, 2500.0);
    // Each opening is the square turned: its corners are 5 sqrt 2 from the centre and its sides
    // are turned by 30 degrees.
    let c = plan.cells.iter().find(|c| (c.i, c.j) == (0, 0)).unwrap();
    for p in sk_points(c) {
        sk_close(p.distance(c.center), 5.0 * 2.0_f64.sqrt());
    }
    let first = &c.loops[0].segs[0];
    let d = first.end() - first.start();
    let turn = det::atan2(d.y, d.x).rem_euclid(FRAC_PI_2);
    sk_close(turn, PI / 6.0);
}

#[test]
fn a_grid_angle_turns_the_lattice_about_the_anchor() {
    // The same square lattice turned a quarter turn about the anchor is the same set of cells:
    // cell (i, j) lands where cell (-j, i) was.
    let cell = sk_square();
    let a = CellGridSpec {
        anchor: v(50.0, 50.0),
        ..sk_spec(Clip::Whole)
    };
    let b = CellGridSpec {
        angle: FRAC_PI_2,
        ..a
    };
    for (i, j) in [(0, 0), (1, 0), (2, 3), (-3, 1)] {
        let p = sketch_cell_center(&b, &cell, i, j).unwrap();
        let q = sketch_cell_center(&a, &cell, -j, i).unwrap();
        sk_close(p.x, q.x);
        sk_close(p.y, q.y);
    }
}

#[test]
fn a_cells_centre_follows_the_pitch_the_stagger_the_angle_and_the_anchor() {
    // Cell (2, 3) on pitch 14 x 16 with stagger 0.5 is at 2 a + 3 b = (28 + 21, 48) = (49, 48)
    // from the anchor (10, 20): (59, 68); turned a quarter about the anchor, (49, 48) becomes
    // (-48, 49), so (-38, 69).
    let cell = sk_l(14.0, 16.0, 0.5);
    let spec = CellGridSpec {
        anchor: v(10.0, 20.0),
        ..sk_spec(Clip::Whole)
    };
    let p = sketch_cell_center(&spec, &cell, 2, 3).unwrap();
    sk_close(p.x, 59.0);
    sk_close(p.y, 68.0);
    let turned = CellGridSpec {
        angle: FRAC_PI_2,
        ..spec
    };
    let p = sketch_cell_center(&turned, &cell, 2, 3).unwrap();
    sk_close(p.x, -38.0);
    sk_close(p.y, 69.0);
}

#[test]
fn flip_mirrors_the_cells_with_i_plus_j_odd_and_lets_them_interlock() {
    // With the L's foot 10 wide, a mirrored neighbour at pitch_x 22 spans [12, 22] against the
    // cell's [0, 10]: 2 apart, the web. (Mirroring is in the vertical axis through the origin, so
    // an odd cell spans [x - 10, x] about its origin x.)
    let mut cell = sk_l(22.0, 16.0, 0.0);
    cell.flip = true;
    let spec = CellGridSpec {
        anchor: v(0.0, 0.0),
        ..sk_spec(Clip::Whole)
    };
    let field = [rect(-50.0, -50.0, 150.0, 150.0)];
    let plan = sk_run(&field, &cell, &spec);
    let x_range = |i: i64, j: i64| {
        let c = plan.cells.iter().find(|c| (c.i, c.j) == (i, j)).unwrap();
        let pts = sk_points(c);
        let lo = pts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
        let hi = pts.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
        (lo, hi)
    };
    // (0, 0) and (1, 1) are the cell as drawn (i + j even); (1, 0) and (0, 1) are mirrored.
    assert_eq!(x_range(0, 0), (0.0, 10.0));
    let (lo, hi) = x_range(1, 0);
    sk_close(lo, 12.0);
    sk_close(hi, 22.0);
    let (lo, hi) = x_range(0, 1);
    sk_close(lo, -10.0);
    sk_close(hi, 0.0);
    let (lo, hi) = x_range(1, 1);
    sk_close(lo, 22.0);
    sk_close(hi, 32.0);
    // A mirrored loop is still counter-clockwise.
    for c in &plan.cells {
        for l in &c.loops {
            assert!(l.signed_area().unwrap().value > 0.0);
        }
    }
    // The same pitch without the flip puts the cells 22 apart in a row, so the flip changed them.
    let mut plain = cell.clone();
    plain.flip = false;
    let plain = sk_run(&field, &plain, &spec);
    let c = plain.cells.iter().find(|c| (c.i, c.j) == (1, 0)).unwrap();
    assert!(sk_points(c).iter().all(|p| p.x >= 22.0 - 1e-9));
}

#[test]
fn a_cell_that_overlaps_its_neighbour_is_refused_with_the_pitch_named() {
    let spec = sk_spec(Clip::Whole);
    let field = [rect(0.0, 0.0, 100.0, 100.0)];
    // A foot 10 wide on pitch 8 overlaps the next cell's foot.
    let e = plan_sketch(&field, &sk_l(8.0, 16.0, 0.0), &spec).unwrap_err();
    assert!(matches!(e, CellGridError::Layout(_)), "{e}");
    assert!(e.to_string().contains("overlaps"), "{e}");
    assert!(e.to_string().contains("pitch_x"), "{e}");
    // A cell of area 70 cannot fit a 5 x 5 pitch.
    let e = plan_sketch(&field, &sk_l(5.0, 5.0, 0.0), &spec).unwrap_err();
    assert!(matches!(e, CellGridError::Layout(_)), "{e}");
    // Mirrored cells on pitch_x 19 overlap: the neighbour spans [9, 19] against [0, 10].
    let mut flipped = sk_l(19.0, 16.0, 0.0);
    flipped.flip = true;
    assert!(matches!(
        plan_sketch(&field, &flipped, &spec),
        Err(CellGridError::Layout(_))
    ));
    // A stagger that slides a cell onto the row below overlaps it: pitch_y 6 puts the next row's
    // cells inside the L's stem.
    assert!(matches!(
        plan_sketch(&field, &sk_l(14.0, 6.0, 0.5), &spec),
        Err(CellGridError::Layout(_))
    ));
    // A square 10 tall on a row pitch of 9 overlaps the row above, though its area fits the pitch.
    assert!(matches!(
        plan_sketch(
            &field,
            &sk_cell(rect(0.0, 0.0, 10.0, 10.0), None, 12.0, 9.0, 0.0),
            &spec
        ),
        Err(CellGridError::Layout(_))
    ));
}

#[test]
fn a_web_wider_than_the_gap_is_refused_and_one_within_it_is_kept() {
    // The L's nearest neighbours are 4 mm away on pitch 14 x 16 (14 - 10 along a row, 16 - 12 up a
    // column), so a web of 3.9 is kept and 4.5 is not.
    let field = [rect(0.0, 0.0, 100.0, 100.0)];
    let with_web = |web: f64| CellGridSpec {
        web,
        anchor: v(0.0, 0.0),
        ..sk_spec(Clip::Whole)
    };
    assert!(plan_sketch(&field, &sk_l(14.0, 16.0, 0.0), &with_web(3.9)).is_ok());
    let e = plan_sketch(&field, &sk_l(14.0, 16.0, 0.0), &with_web(4.5)).unwrap_err();
    assert!(matches!(e, CellGridError::Layout(_)), "{e}");
    let msg = e.to_string();
    assert!(msg.contains("closer than the web of 4.5"), "{msg}");
    assert!(msg.contains("pitch_x"), "{msg}");
    // Cells that share an edge exactly are not overlapping: with a web of 0 they are accepted.
    let tiled = sk_cell(rect(0.0, 0.0, 10.0, 10.0), None, 10.0, 10.0, 0.0);
    let plan = plan_sketch(
        &field,
        &tiled,
        &CellGridSpec {
            web: 0.0,
            anchor: v(5.0, 5.0),
            ..sk_spec(Clip::Whole)
        },
    )
    .unwrap();
    assert_eq!(plan.whole, 100);
}

#[test]
fn snap_is_refused_for_a_sketched_cell() {
    let field = [rect(0.0, 0.0, 100.0, 100.0)];
    let cell = sk_square();
    for spec in [
        sk_spec(Clip::Snap),
        CellGridSpec {
            keepout_clip: Some(Clip::Snap),
            ..sk_spec(Clip::Free)
        },
    ] {
        let e = plan_sketch(&field, &cell, &spec).unwrap_err();
        assert!(matches!(e, CellGridError::Parameter(_)), "{e}");
        assert!(
            e.to_string()
                .contains("snap needs the cell's own chords; use whole or free"),
            "{e}"
        );
    }
}

#[test]
fn a_margin_keeps_whole_sketched_cells_that_far_from_every_edge() {
    // Margin 3: cell i covers [12 i, 12 i + 10], so it needs 12 i >= 3 and 12 i + 10 <= 97:
    // i = 1..=7, 7 x 7 = 49 cells (i = 0 touches the field's edge, i = 8 is cut).
    let spec = CellGridSpec {
        margin: 3.0,
        ..sk_spec(Clip::Whole)
    };
    let plan = sk_run(&[rect(0.0, 0.0, 100.0, 100.0)], &sk_square(), &spec);
    assert_eq!((plan.whole, plan.partial), (49, 0));
    // Around a keep-out as well: [40, 60]^2 with margin 3 drops the cells within 3 of it:
    // i = 2 ends at 34 (6 away, kept), 5 starts at 60 (touching, dropped), 3 and 4 overlap.
    let around = sk_run(
        &[rect(0.0, 0.0, 100.0, 100.0), rect(40.0, 40.0, 60.0, 60.0)],
        &sk_square(),
        &spec,
    );
    // Columns i, rows j in 1..=7; cells dropped have i in {3, 4, 5} and j in {3, 4, 5}
    // within reach: x in [36, 46], [48, 58], [60, 70] with y in the same three: 9 cells.
    assert_eq!(around.whole, 49 - 9);
}

#[test]
fn cells_round_a_circular_field_are_decided_on_the_arc() {
    // A circle of radius 30 about (50, 50), squares centred on (50 + 12 i, 50 + 12 j), half side
    // 5. A cell's farthest corner is sqrt((12 |i| + 5)^2 + (12 |j| + 5)^2) from the centre:
    // 24.04 for |i| = |j| = 1 (kept), 29.43 for (2, 0) (kept), 33.6 for (2, 1) (dropped). The
    // kept cells are |i|, |j| <= 1 (9) and the four (+-2, 0), (0, +-2): 13.
    let field = [circle(50.0, 50.0, 30.0)];
    let spec = CellGridSpec {
        anchor: v(50.0, 50.0),
        ..sk_spec(Clip::Whole)
    };
    let whole = sk_run(&field, &sk_square(), &spec);
    assert_eq!((whole.whole, whole.partial), (13, 0));
    let free = sk_run(
        &field,
        &sk_square(),
        &CellGridSpec {
            clip: Clip::Free,
            ..spec
        },
    );
    assert_eq!(free.whole, 13);
    assert!(free.partial > 0);
    assert!(free.open_area > whole.open_area);
    // The trimmed cells stay inside the circle: the openings cannot add up to more than its area.
    assert!(free.open_area < PI * 30.0 * 30.0);
}

#[test]
fn free_trims_a_cell_that_is_not_convex_to_the_field_exactly() {
    // Row j = 6 of the L cells on pitch 14 x 16 starts at y = 96, so the foot (4 tall) tops out
    // exactly at the field's edge y = 100 and the stem and cap lie above it. Each of the cells
    // i = 0..=6 (x0 = 14 i, up to 84 + 10 = 94) is cut to its foot: 10 x 4 = 40; the cell i = 7
    // starts at x = 98, so its foot is cut to 2 wide: 2 x 4 = 8. That is 7 x 40 + 8 = 288.
    let spec = CellGridSpec {
        anchor: v(0.0, 0.0),
        ..sk_spec(Clip::Free)
    };
    let plan = sk_run(
        &[rect(0.0, 0.0, 100.0, 100.0)],
        &sk_l(14.0, 16.0, 0.0),
        &spec,
    );
    let row: Vec<&GridCell> = plan.cells.iter().filter(|c| c.j == 6).collect();
    assert_eq!(row.len(), 8);
    let area: f64 = row.iter().map(|c| c.area).sum();
    sk_close(area, 288.0);
    assert!(row.iter().all(|c| c.kind == CellKind::Free));
    // Whole drops them all.
    let whole = sk_run(
        &[rect(0.0, 0.0, 100.0, 100.0)],
        &sk_l(14.0, 16.0, 0.0),
        &CellGridSpec {
            clip: Clip::Whole,
            ..spec
        },
    );
    assert!(whole.cells.iter().all(|c| c.j <= 5));
}

#[test]
fn two_fields_with_the_same_sketched_grid_line_up() {
    // The cells are worked out from their indices, so a field shifted along the lattice gets the
    // very same cells where the two overlap.
    let cell = sk_l(14.0, 16.0, 0.3);
    let spec = CellGridSpec {
        anchor: v(3.0, 4.0),
        angle: 0.2,
        ..sk_spec(Clip::Free)
    };
    let a = sk_run(&[rect(0.0, 0.0, 100.0, 100.0)], &cell, &spec);
    let b = sk_run(&[rect(50.0, 0.0, 150.0, 100.0)], &cell, &spec);
    let again = sk_run(&[rect(0.0, 0.0, 100.0, 100.0)], &cell, &spec);
    assert_eq!(a, again);
    let mut shared = 0;
    for x in &a.cells {
        if let Some(y) = b.cells.iter().find(|y| (y.i, y.j) == (x.i, x.j)) {
            if x.kind == CellKind::Whole && y.kind == CellKind::Whole {
                assert_eq!(x.center, y.center);
                assert_eq!(x.loops, y.loops);
                shared += 1;
            }
        }
    }
    assert!(shared > 0);
}

#[test]
fn hostile_sketched_cells_are_refused_and_never_panic() {
    let field = [rect(0.0, 0.0, 100.0, 100.0)];
    let spec = sk_spec(Clip::Whole);
    let nan = f64::NAN;
    let inf = f64::INFINITY;
    let mut cases: Vec<(&str, SketchCell, CellGridSpec)> = Vec::new();
    let mut add = |name: &'static str, f: &dyn Fn(&mut SketchCell, &mut CellGridSpec)| {
        let (mut c, mut s) = (sk_square(), spec);
        f(&mut c, &mut s);
        cases.push((name, c, s));
    };
    add("pitch_x NaN", &|c, _| c.pitch_x = nan);
    add("pitch_y NaN", &|c, _| c.pitch_y = nan);
    add("pitch_x zero", &|c, _| c.pitch_x = 0.0);
    add("pitch_y zero", &|c, _| c.pitch_y = 0.0);
    add("pitch negative", &|c, _| c.pitch_x = -12.0);
    add("pitch infinite", &|c, _| c.pitch_y = inf);
    add("pitch huge", &|c, _| c.pitch_x = 1e12);
    add("stagger NaN", &|c, _| c.stagger = nan);
    add("stagger above 1", &|c, _| c.stagger = 1.5);
    add("stagger below 0", &|c, _| c.stagger = -0.1);
    add("cell angle NaN", &|c, _| c.cell_angle = nan);
    add("cell angle huge", &|c, _| c.cell_angle = 1e30);
    add("origin NaN", &|c, _| c.origin = Some(v(nan, 0.0)));
    add("origin huge", &|c, _| c.origin = Some(v(1e12, 0.0)));
    add("web NaN", &|_, s| s.web = nan);
    add("web negative", &|_, s| s.web = -1.0);
    add("web huge", &|_, s| s.web = 1e12);
    add("grid angle NaN", &|_, s| s.angle = nan);
    add("anchor infinite", &|_, s| s.anchor = v(inf, 0.0));
    add("margin negative", &|_, s| s.margin = -1.0);
    add("empty profile", &|c, _| {
        c.profile = Loop2 { segs: Vec::new() }
    });
    add("open profile", &|c, _| {
        c.profile = Loop2 {
            segs: vec![
                Seg2::Line {
                    a: v(0.0, 0.0),
                    b: v(10.0, 0.0),
                },
                Seg2::Line {
                    a: v(10.0, 0.0),
                    b: v(10.0, 10.0),
                },
                Seg2::Line {
                    a: v(10.0, 10.0),
                    b: v(0.0, 10.0),
                },
            ],
        }
    });
    add("bow tie", &|c, _| {
        c.profile = polygon(&[(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)]);
    });
    add("zero area", &|c, _| {
        c.profile = polygon(&[(0.0, 0.0), (10.0, 0.0)]);
    });
    add("profile NaN", &|c, _| {
        c.profile = polygon(&[(0.0, 0.0), (nan, 0.0), (10.0, 10.0)]);
    });
    add("profile coordinate huge", &|c, _| {
        c.profile = rect(0.0, 0.0, 1e10, 1e10);
    });
    add("profile reaches too far", &|c, _| {
        c.profile = rect(0.0, 0.0, 3e6, 3e6);
    });
    add("profile with a spline", &|c, _| {
        c.profile = Loop2 {
            segs: vec![
                Seg2::Cubic {
                    p0: v(0.0, 0.0),
                    p1: v(5.0, 5.0),
                    p2: v(8.0, 5.0),
                    p3: v(10.0, 0.0),
                },
                Seg2::Line {
                    a: v(10.0, 0.0),
                    b: v(0.0, 0.0),
                },
            ],
        }
    });
    add("profile too many segments", &|c, _| {
        let pts: Vec<(f64, f64)> = (0..300)
            .map(|k| {
                let a = f64::from(k) / 300.0 * 2.0 * PI;
                (5.0 * det::cos(a), 5.0 * det::sin(a))
            })
            .collect();
        c.profile = polygon(&pts);
    });
    add("arc with no radius", &|c, _| {
        c.profile = Loop2 {
            segs: vec![Seg2::Arc {
                center: v(0.0, 0.0),
                radius: 0.0,
                start: 0.0,
                sweep: PI,
            }],
        }
    });
    for (name, cell, spec) in &cases {
        assert!(
            plan_sketch(&field, cell, spec).is_err(),
            "{name} was not refused"
        );
    }
    // A tiny pitch against a big field is too many cells, not a hang; a huge but legal pitch is
    // fine; an open field is refused as the other shapes' fields are.
    let big = [rect(0.0, 0.0, 1e6, 1e6)];
    let tiny = sk_cell(rect(0.0, 0.0, 0.01, 0.01), None, 0.02, 0.02, 0.0);
    assert!(matches!(
        plan_sketch(&big, &tiny, &CellGridSpec { web: 0.0, ..spec }),
        Err(CellGridError::TooManyCells { .. })
    ));
    let sparse = sk_cell(rect(0.0, 0.0, 10.0, 10.0), None, 1e8, 1e8, 0.5);
    assert!(plan_sketch(&field, &sparse, &spec).is_ok());
    let open = [Loop2 {
        segs: vec![Seg2::Line {
            a: v(0.0, 0.0),
            b: v(10.0, 0.0),
        }],
    }];
    assert!(matches!(
        plan_sketch(&open, &sk_square(), &spec),
        Err(CellGridError::Field(_))
    ));
}
