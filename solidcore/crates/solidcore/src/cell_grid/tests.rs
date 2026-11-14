//! Tests of the cell grid planner: the lattices, snapping, the three clip modes, alignment of
//! two fields and hostile input.

#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp
)]

use std::f64::consts::{FRAC_PI_2, FRAC_PI_6, PI};

use crate::math::Vec2;
use crate::prism::profile::{Loop2, Seg2};

use crate::cell_grid::lattice::{Lattice, CIRCLE_SIDES};
use crate::cell_grid::region::{clip_halfplane, poly_area, Bounds, Convex, Loop, Prim, Region};
use crate::cell_grid::snap::snap_loop;
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
            ((a.cos(), a.sin()), rng.range(0.35, 0.5) * size)
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
                    let k = lat.direction_index((-nrm.x).atan2(nrm.y)).unwrap();
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
    assert!((lat.cell_area - 3.0 * SQRT3 * (S / 2.0).powi(2)).abs() < 1e-12);
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
        assert!((lat.cell_area - PI * (S / 2.0).powi(2)).abs() < 1e-12);
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
    let expect = r * r * (-d / r).acos() + d * (r * r - d * d).sqrt();
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
                cx + r * (2.0 * PI * k as f64 / n as f64).cos(),
                cy + r * (2.0 * PI * k as f64 / n as f64).sin(),
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
            let (sn, cs) = angle.sin_cos();
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
            let local = (c.center - s.anchor).dot(v(-angle.sin(), angle.cos()));
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
            let (sn, cs) = angle.sin_cos();
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
            (50.0 * a.cos(), 50.0 * a.sin())
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
                c + v(a.cos(), a.sin()) * r
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
    let unit = |a: f64| v(a.cos(), a.sin());
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
        let e = 10.0_f64.powf(-f64::from(k) / 8.0) * 0.5;
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
