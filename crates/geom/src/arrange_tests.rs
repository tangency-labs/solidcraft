use std::sync::atomic::AtomicBool;

use solidcore_record::Vec2;

use super::*;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn poly(pts: &[(f64, f64)]) -> Shape2 {
    Shape2 { outer: pts.iter().map(|&(x, y)| v(x, y)).collect(), holes: Vec::new() }
}

fn item(shape: Shape2, quantity: usize, rotations_deg: &[f64]) -> ArrangeItem {
    ArrangeItem { shape, quantity, rotations_deg: rotations_deg.to_vec() }
}

fn ngon(n: usize, r: f64) -> Shape2 {
    let pts: Vec<(f64, f64)> = (0..n).map(|i| (i as f64 / n as f64 * std::f64::consts::TAU).sin_cos()).map(|(s, c)| (r * c, r * s)).collect();
    poly(&pts)
}

fn frame(outer: f64, inner: f64) -> Shape2 {
    let a = (outer - inner) / 2.0;
    Shape2 {
        outer: vec![v(0.0, 0.0), v(outer, 0.0), v(outer, outer), v(0.0, outer)],
        holes: vec![vec![v(a, a), v(a, a + inner), v(a + inner, a + inner), v(a + inner, a)]],
    }
}

fn comb(teeth: usize) -> Shape2 {
    // A spine 10 high with teeth 8 wide and 30 high every 20, gaps of 12 between them.
    let w = teeth as f64 * 20.0;
    let mut pts = vec![(0.0, 0.0), (w, 0.0), (w, 10.0)];
    for i in (0..teeth).rev() {
        let x = i as f64 * 20.0;
        pts.extend([(x + 14.0, 10.0), (x + 14.0, 40.0), (x + 6.0, 40.0), (x + 6.0, 10.0)]);
    }
    pts.push((0.0, 10.0));
    poly(&pts)
}

// ---------- an independent checker, written from scratch so it shares no code with the packer ----------

fn placed_rings(items: &[ArrangeItem], p: &Placement2) -> Vec<Vec<(f64, f64)>> {
    let Some(it) = items.get(p.item) else { return Vec::new() };
    let (s, c) = p.angle.sin_cos();
    std::iter::once(&it.shape.outer)
        .chain(&it.shape.holes)
        .map(|r| r.iter().map(|q| (q.x * c - q.y * s + p.shift.x, q.x * s + q.y * c + p.shift.y)).collect())
        .collect()
}

fn ring_edges(r: &[(f64, f64)]) -> Vec<((f64, f64), (f64, f64))> {
    (0..r.len()).map(|i| (r[i], r[(i + 1) % r.len()])).collect()
}

fn point_seg(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l = dx * dx + dy * dy;
    let t = if l == 0.0 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l).clamp(0.0, 1.0) };
    ((p.0 - a.0 - t * dx).powi(2) + (p.1 - a.1 - t * dy).powi(2)).sqrt()
}

fn orient(p: (f64, f64), q: (f64, f64), r: (f64, f64)) -> f64 {
    (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
}

/// Whether a-b and c-d cross, each end more than `eps` off the other's line.
fn proper_cross(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64), eps: f64) -> bool {
    let len = |p: (f64, f64), q: (f64, f64)| ((q.0 - p.0).powi(2) + (q.1 - p.1).powi(2)).sqrt().max(f64::MIN_POSITIVE);
    let (o1, o2) = (orient(a, b, c) / len(a, b), orient(a, b, d) / len(a, b));
    let (o3, o4) = (orient(c, d, a) / len(c, d), orient(c, d, b) / len(c, d));
    ((o1 > eps && o2 < -eps) || (o1 < -eps && o2 > eps)) && ((o3 > eps && o4 < -eps) || (o3 < -eps && o4 > eps))
}

fn seg_seg(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> f64 {
    if proper_cross(a, b, c, d, 0.0) {
        return 0.0;
    }
    point_seg(a, c, d).min(point_seg(b, c, d)).min(point_seg(c, a, b)).min(point_seg(d, a, b))
}

fn in_poly(p: (f64, f64), rings: &[Vec<(f64, f64)>]) -> bool {
    let mut inside = false;
    for r in rings {
        for (a, b) in ring_edges(r) {
            if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0) {
                inside = !inside;
            }
        }
    }
    inside
}

fn signed_area(r: &[(f64, f64)]) -> f64 {
    let o = r.first().copied().unwrap_or((0.0, 0.0));
    ring_edges(r).iter().map(|(a, b)| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)).sum::<f64>() / 2.0
}

fn size(rings: &[Vec<(f64, f64)>]) -> f64 {
    let (mut lo, mut hi) = ((f64::INFINITY, f64::INFINITY), (f64::NEG_INFINITY, f64::NEG_INFINITY));
    for &(x, y) in rings.iter().flatten() {
        lo = (lo.0.min(x), lo.1.min(y));
        hi = (hi.0.max(x), hi.1.max(y));
    }
    ((hi.0 - lo.0).powi(2) + (hi.1 - lo.1).powi(2)).sqrt()
}

/// Points a hair off every edge on the material side (`out`: the other side), at several places
/// along it and near each corner, however short the edge.
fn edge_samples(rings: &[Vec<(f64, f64)>], out: bool) -> Vec<(f64, f64)> {
    let off = 1e-7 * size(rings);
    let mut pts = Vec::new();
    for (k, r) in rings.iter().enumerate() {
        let left = ((k == 0) == (signed_area(r) > 0.0)) != out;
        for (a, b) in ring_edges(r) {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l = (dx * dx + dy * dy).sqrt();
            if l == 0.0 {
                continue;
            }
            let (nx, ny) = if left { (-dy / l, dx / l) } else { (dy / l, -dx / l) };
            for f in [0.02, 0.1, 0.3, 0.5, 0.7, 0.9, 0.98] {
                pts.push((a.0 + f * dx + off * nx, a.1 + f * dy + off * ny));
            }
        }
    }
    pts
}

/// Least gap between two placed regions (0 when they touch); negative when they overlap.
fn gap(a: &[Vec<(f64, f64)>], b: &[Vec<(f64, f64)>]) -> f64 {
    let eps = 1e-9 * size(a).max(size(b));
    let mut d = f64::INFINITY;
    for ra in a {
        for (p, q) in ring_edges(ra) {
            for rb in b {
                for (c, e) in ring_edges(rb) {
                    if proper_cross(p, q, c, e, eps) {
                        return -1.0;
                    }
                    d = d.min(seg_seg(p, q, c, e));
                }
            }
        }
    }
    if edge_samples(a, false).iter().any(|&p| in_poly(p, b)) || edge_samples(b, false).iter().any(|&p| in_poly(p, a)) {
        return -1.0;
    }
    d
}

/// Asserts the layout keeps every rule: parts inside the sheet by the margin (and off its holes
/// and notches), apart by the spacing, the counts add up. Everything is measured from the sheet's
/// first corner so a sheet far from the origin keeps its precision.
fn check(items: &[ArrangeItem], sheet: &Shape2, opts: &ArrangeOptions, r: &ArrangeResult) {
    let o = sheet.outer.first().map_or((0.0, 0.0), |p| (p.x, p.y));
    let local = |rings: Vec<Vec<(f64, f64)>>| -> Vec<Vec<(f64, f64)>> {
        rings.into_iter().map(|r| r.into_iter().map(|(x, y)| (x - o.0, y - o.1)).collect()).collect()
    };
    let sheet_rings = local(std::iter::once(&sheet.outer).chain(&sheet.holes).map(|r| r.iter().map(|q| (q.x, q.y)).collect()).collect());
    let tol = 1e-6;
    let parts: Vec<Vec<Vec<(f64, f64)>>> = r.placed.iter().map(|p| local(placed_rings(items, p))).collect();
    let off_sheet = edge_samples(&sheet_rings, true);
    for (i, part) in parts.iter().enumerate() {
        assert!(edge_samples(part, false).iter().all(|&p| in_poly(p, &sheet_rings)), "part {i} leaves the sheet");
        assert!(!off_sheet.iter().any(|&p| in_poly(p, part)), "part {i} covers a notch or hole of the sheet");
        let mut edge = f64::INFINITY;
        for rp in part {
            for (a, b) in ring_edges(rp) {
                for rs in &sheet_rings {
                    for (c, d) in ring_edges(rs) {
                        assert!(!proper_cross(a, b, c, d, 1e-9 * size(&sheet_rings)), "part {i} crosses the sheet's edge");
                        edge = edge.min(seg_seg(a, b, c, d));
                    }
                }
            }
        }
        assert!(edge >= opts.margin - tol, "part {i} is {edge} from the sheet edge, margin {}", opts.margin);
        for (j, other) in parts.iter().enumerate().skip(i + 1) {
            let g = gap(part, other);
            assert!(g >= opts.spacing - tol, "parts {i} and {j} are {g} apart, spacing {}", opts.spacing);
            if !opts.part_in_part || opts.method == ArrangeMethod::Rectangular {
                let (pa, pb) = (&part[..1], &other[..1]);
                assert!(gap(pa, pb) >= 0.0, "parts {i} and {j} nest");
            }
        }
    }
    for (k, it) in items.iter().enumerate() {
        let n = r.placed.iter().filter(|p| p.item == k).count();
        assert_eq!(n + r.unplaced[k], it.quantity, "item {k} count");
    }
}

fn run(items: &[ArrangeItem], sheet: &Shape2, opts: &ArrangeOptions) -> ArrangeResult {
    let r = arrange(items, sheet, opts, None).unwrap();
    check(items, sheet, opts, &r);
    r
}

// ---------- layouts ----------

#[test]
fn squares_fill_a_sheet_in_rows() {
    let items = [item(Shape2::rectangle(10.0, 10.0), 12, &[])];
    let sheet = Shape2::rectangle(40.0, 30.0);
    let r = run(&items, &sheet, &ArrangeOptions::default());
    assert_eq!(r.placed.len(), 12);
    assert!((r.fill - 1.0).abs() < 1e-6, "fill {}", r.fill);
}

#[test]
fn interlocking_strips_cut_from_a_sheet_fill_it_again() {
    // A 300 x 200 sheet cut into six strips along staircase lines: every strip but the end ones
    // is concave on both sides, and only an exact fit puts all six back.
    let stairs = |x: f64| -> Vec<(f64, f64)> {
        (0..8)
            .flat_map(|k| {
                let (y, step) = (f64::from(k) * 25.0, if k % 2 == 0 { 0.0 } else { 15.0 });
                [(x + step, y), (x + step, y + 25.0)]
            })
            .collect()
    };
    let items: Vec<ArrangeItem> = (0..6)
        .map(|i| {
            let (x0, x1) = (f64::from(i) * 50.0, f64::from(i + 1) * 50.0);
            let right = if i == 5 { vec![(300.0, 0.0), (300.0, 200.0)] } else { stairs(x1) };
            let left = if i == 0 { vec![(0.0, 0.0), (0.0, 200.0)] } else { stairs(x0) };
            let ring: Vec<(f64, f64)> = right.into_iter().chain(left.into_iter().rev()).map(|(x, y)| (x - x0, y)).collect();
            item(poly(&ring), 1, &[0.0, 180.0])
        })
        .collect();
    let r = run(&items, &Shape2::rectangle(300.0, 200.0), &ArrangeOptions::default());
    assert_eq!(r.placed.len(), 6, "unplaced {:?}", r.unplaced);
    assert!((r.fill - 1.0).abs() < 1e-6, "fill {}", r.fill);
}

#[test]
fn a_tight_plate_takes_more_parts_once_the_search_stands_some_on_end() {
    // A build plate a hand layout fills well: three 95 x 66 cases stood on end in a row, the
    // 100 mm square and the round parts above them. The lowest spot for each case lies it flat,
    // which breaks the room up; only a search that holds turns gets back to the hand layout.
    let items = [
        item(Shape2::rectangle(100.0, 100.0), 1, &[0.0, 90.0]),
        item(Shape2::rectangle(95.0, 66.0), 3, &[0.0, 90.0]),
        item(ngon(48, 25.0), 1, &[0.0, 90.0]),
        item(ngon(48, 20.0), 5, &[0.0, 90.0]),
    ];
    let sheet = Shape2::rectangle(205.0, 200.0);
    let greedy = ArrangeOptions { spacing: 2.0, ..ArrangeOptions::default() };
    let first = run(&items, &sheet, &greedy).placed.len();
    let searched = run(&items, &sheet, &ArrangeOptions { improve: 300, ..greedy }).placed.len();
    assert!(searched >= 8 && searched > first, "greedy {first}, searched {searched}");
}

#[test]
fn spacing_and_margin_are_kept_and_cost_room() {
    let items = [item(Shape2::rectangle(10.0, 10.0), 12, &[])];
    let sheet = Shape2::rectangle(40.0, 30.0);
    let opts = ArrangeOptions { spacing: 2.0, margin: 1.0, ..Default::default() };
    let r = run(&items, &sheet, &opts);
    // 3 across (1 + 10 + 2 + 10 + 2 + 10 + 1 = 36) and 2 down.
    assert_eq!(r.placed.len(), 6);
    assert_eq!(r.unplaced, vec![6]);
}

#[test]
fn a_part_turns_to_fit() {
    let items = [item(Shape2::rectangle(30.0, 10.0), 1, &[0.0, 90.0])];
    let sheet = Shape2::rectangle(12.0, 40.0);
    let r = run(&items, &sheet, &ArrangeOptions::default());
    assert_eq!(r.placed.len(), 1);
    assert!((r.placed[0].angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    // Without the rotation it does not fit.
    let fixed = [item(Shape2::rectangle(30.0, 10.0), 1, &[])];
    assert_eq!(run(&fixed, &sheet, &ArrangeOptions::default()).unplaced, vec![1]);
}

#[test]
fn triangles_touching_at_zero_spacing_never_overlap() {
    let tri = poly(&[(0.0, 0.0), (10.0, 0.0), (5.0, 8.0)]);
    let items = [item(tri, 40, &[0.0, 180.0])];
    let r = run(&items, &Shape2::rectangle(60.0, 50.0), &ArrangeOptions::default());
    // Turned triangles nest point to base, so more fit than their boxes would.
    assert!(r.placed.len() > 30, "only {}", r.placed.len());
}

#[test]
fn combs_interlock_when_they_may_turn() {
    let items = [item(comb(5), 2, &[0.0, 180.0])];
    let sheet = Shape2::rectangle(110.0, 60.0);
    let r = run(&items, &sheet, &ArrangeOptions::default());
    // Stacked boxes need 80 high; interlocked teeth need only 50.
    assert_eq!(r.placed.len(), 2);
    assert!(r.used.max.y - r.used.min.y < 55.0, "used {:?}", r.used);
}

#[test]
fn a_small_part_goes_in_a_frames_hole_only_when_allowed() {
    let items = [item(frame(50.0, 30.0), 1, &[]), item(Shape2::rectangle(20.0, 20.0), 1, &[])];
    let sheet = Shape2::rectangle(55.0, 55.0);
    let r = run(&items, &sheet, &ArrangeOptions { spacing: 1.0, ..Default::default() });
    assert_eq!(r.unplaced, vec![0, 0]);
    let off = ArrangeOptions { spacing: 1.0, part_in_part: false, ..Default::default() };
    assert_eq!(run(&items, &sheet, &off).unplaced, vec![0, 1]);
}

#[test]
fn rectangular_method_packs_boxes_and_never_nests() {
    let items = [item(frame(50.0, 30.0), 1, &[]), item(Shape2::rectangle(20.0, 20.0), 1, &[])];
    let opts = ArrangeOptions { method: ArrangeMethod::Rectangular, ..Default::default() };
    let r = run(&items, &Shape2::rectangle(55.0, 55.0), &opts);
    assert_eq!(r.unplaced, vec![0, 1]);
    let tri = poly(&[(0.0, 0.0), (10.0, 0.0), (5.0, 8.0)]);
    let r = run(&[item(tri, 30, &[0.0, 180.0])], &Shape2::rectangle(60.0, 50.0), &opts);
    assert_eq!(r.placed.len(), 30);
}

#[test]
fn a_sheet_with_a_hole_and_an_odd_outline_is_respected() {
    let sheet = Shape2 {
        outer: vec![v(0.0, 0.0), v(100.0, 0.0), v(100.0, 60.0), v(50.0, 100.0), v(0.0, 60.0)],
        holes: vec![vec![v(40.0, 20.0), v(60.0, 20.0), v(60.0, 40.0), v(40.0, 40.0)]],
    };
    let items = [item(ngon(7, 6.0), 30, &[0.0, 90.0]), item(Shape2::rectangle(8.0, 3.0), 20, &[0.0, 45.0, 90.0])];
    let r = run(&items, &sheet, &ArrangeOptions { spacing: 0.5, margin: 1.0, ..Default::default() });
    assert!(r.placed.len() > 30, "only {}", r.placed.len());
}

#[test]
fn mixed_concave_parts_at_large_and_small_scale() {
    for scale in [1e-3, 1.0, 1e4] {
        let s = |sh: Shape2| Shape2 {
            outer: sh.outer.iter().map(|p| v(p.x * scale, p.y * scale)).collect(),
            holes: sh.holes.iter().map(|h| h.iter().map(|p| v(p.x * scale, p.y * scale)).collect()).collect(),
        };
        let items = [
            item(s(comb(3)), 3, &[0.0, 90.0, 180.0, 270.0]),
            item(s(poly(&[(0.0, 0.0), (30.0, 0.0), (30.0, 10.0), (10.0, 10.0), (10.0, 30.0), (0.0, 30.0)])), 4, &[0.0, 90.0, 180.0, 270.0]),
            item(s(ngon(12, 5.0)), 6, &[]),
        ];
        let opts = ArrangeOptions { spacing: 0.5 * scale, margin: 0.25 * scale, ..Default::default() };
        let r = arrange(&items, &s(Shape2::rectangle(150.0, 120.0)), &opts, None).unwrap();
        // The checker's tolerance is absolute; scale the layout back before checking.
        let back = |x: f64| x / scale;
        let unscaled =
            ArrangeResult { placed: r.placed.iter().map(|p| Placement2 { shift: v(back(p.shift.x), back(p.shift.y)), ..*p }).collect(), ..r.clone() };
        let plain = [
            item(comb(3), 3, &[]),
            item(poly(&[(0.0, 0.0), (30.0, 0.0), (30.0, 10.0), (10.0, 10.0), (10.0, 30.0), (0.0, 30.0)]), 4, &[]),
            item(ngon(12, 5.0), 6, &[]),
        ];
        check(&plain, &Shape2::rectangle(150.0, 120.0), &ArrangeOptions { spacing: 0.5, margin: 0.25, ..Default::default() }, &unscaled);
        assert_eq!(r.unplaced, vec![0, 0, 0], "scale {scale}");
    }
}

#[test]
fn too_many_parts_are_counted_not_dropped() {
    let items = [item(Shape2::rectangle(10.0, 10.0), 7, &[]), item(ngon(6, 3.0), 50, &[])];
    let r = run(&items, &Shape2::rectangle(25.0, 25.0), &ArrangeOptions::default());
    assert_eq!(r.placed.len() + r.unplaced.iter().sum::<usize>(), 57);
    assert!(r.unplaced[0] >= 3);
}

#[test]
fn a_part_bigger_than_the_sheet_is_left_out() {
    let items = [item(Shape2::rectangle(100.0, 100.0), 2, &[0.0, 90.0]), item(Shape2::rectangle(5.0, 5.0), 1, &[])];
    let r = run(&items, &Shape2::rectangle(50.0, 50.0), &ArrangeOptions::default());
    assert_eq!(r.unplaced, vec![2, 0]);
}

#[test]
fn the_same_input_gives_the_same_layout() {
    let items = [item(comb(4), 3, &[0.0, 180.0]), item(ngon(5, 7.0), 8, &[0.0, 36.0]), item(Shape2::rectangle(12.0, 4.0), 6, &[0.0, 90.0])];
    let opts = ArrangeOptions { spacing: 1.0, improve: 20, seed: 7, ..Default::default() };
    let sheet = Shape2::rectangle(120.0, 80.0);
    let a = run(&items, &sheet, &opts);
    let b = run(&items, &sheet, &opts);
    assert_eq!(a, b);
}

#[test]
fn the_order_search_never_makes_it_worse() {
    let items = [item(comb(4), 3, &[0.0, 180.0]), item(ngon(5, 7.0), 10, &[0.0, 36.0]), item(Shape2::rectangle(25.0, 6.0), 6, &[0.0, 90.0])];
    let sheet = Shape2::rectangle(90.0, 70.0);
    let first = run(&items, &sheet, &ArrangeOptions::default());
    let searched = run(&items, &sheet, &ArrangeOptions { improve: 30, ..Default::default() });
    let placed_area = |r: &ArrangeResult| r.fill;
    assert!(placed_area(&searched) >= placed_area(&first) - 1e-9);
}

#[test]
fn nothing_to_place_is_an_empty_layout() {
    let r = run(&[], &Shape2::rectangle(10.0, 10.0), &ArrangeOptions::default());
    assert!(r.placed.is_empty() && r.used.is_empty() && r.fill == 0.0);
    let r = run(&[item(Shape2::rectangle(1.0, 1.0), 0, &[])], &Shape2::rectangle(10.0, 10.0), &ArrangeOptions::default());
    assert_eq!(r.unplaced, vec![0]);
}

#[test]
fn a_cancelled_run_stops() {
    let cancel = AtomicBool::new(true);
    let items = [item(Shape2::rectangle(1.0, 1.0), 5, &[])];
    assert_eq!(arrange(&items, &Shape2::rectangle(10.0, 10.0), &ArrangeOptions::default(), Some(&cancel)), Err(ArrangeError::Cancelled));
}

#[test]
fn closing_corner_and_repeated_corners_are_accepted() {
    let sq = poly(&[(0.0, 0.0), (5.0, 0.0), (5.0, 0.0), (5.0, 5.0), (0.0, 5.0), (0.0, 0.0)]);
    let r = run(&[item(sq, 4, &[])], &Shape2::rectangle(10.0, 10.0), &ArrangeOptions::default());
    assert_eq!(r.placed.len(), 4);
}

#[test]
fn a_notch_in_the_sheet_edge_is_never_covered() {
    // A notch 4 wide and 2 deep in the top edge leaves room for one full-height part, right of it.
    let sheet = poly(&[(-10.0, 0.0), (30.0, 0.0), (30.0, 5.0), (7.0, 5.0), (7.0, 3.0), (3.0, 3.0), (3.0, 5.0), (-10.0, 5.0)]);
    let r = run(&[item(Shape2::rectangle(20.0, 5.0), 2, &[])], &sheet, &ArrangeOptions::default());
    assert_eq!(r.placed.len(), 1);
    assert!(r.placed[0].shift.x >= 7.0 - 1e-9, "{:?}", r.placed[0]);
}

#[test]
fn finely_tessellated_parts_keep_every_rule() {
    // Edges far shorter than the part: a flattened circle at the corner limit, beside a notch.
    let sheet = poly(&[(0.0, 0.0), (6.0, 0.0), (6.0, 3.0), (3.0, 3.0), (3.0, 6.0), (0.0, 6.0)]);
    let items = [item(ngon(MAX_CORNERS, 1.0), 2, &[]), item(Shape2::rectangle(1.0, 1.0), 6, &[])];
    let r = run(&items, &sheet, &ArrangeOptions::default());
    assert_eq!(r.unplaced, vec![0, 0]);
}

#[test]
fn sharp_corners_pack_flush() {
    // A 30-degree wedge: turned copies pair into boxes along their long edges.
    let wedge = poly(&[(0.0, 0.0), (10.0, 0.0), (0.0, 10.0 / 3f64.sqrt())]);
    let r = run(&[item(wedge, 8, &[0.0, 180.0])], &Shape2::rectangle(20.0, 11.6), &ArrangeOptions::default());
    assert_eq!(r.placed.len(), 8);
}

#[test]
fn a_part_slides_past_a_corner_it_only_touches() {
    // The second square drops beside the first, its side along the first's, down to the floor.
    let r = run(&[item(Shape2::rectangle(5.0, 5.0), 2, &[])], &Shape2::rectangle(10.0, 50.0), &ArrangeOptions::default());
    assert_eq!(r.placed.len(), 2);
    assert!(r.used.max.y < 5.0 + 1e-9, "used {:?}", r.used);
}

#[test]
fn quarter_turns_are_exact_and_pack_flush() {
    let r = run(&[item(Shape2::rectangle(10.0, 20.0), 6, &[90.0])], &Shape2::rectangle(40.0, 30.0), &ArrangeOptions::default());
    assert_eq!(r.placed.len(), 6);
    assert!((r.fill - 1.0).abs() < 1e-9, "fill {}", r.fill);
}

#[test]
fn sheets_that_are_not_upright_rectangles_still_fill() {
    let diamond = poly(&[(50.0, 0.0), (100.0, 50.0), (50.0, 100.0), (0.0, 50.0)]);
    let r = run(&[item(Shape2::rectangle(10.0, 10.0), 30, &[])], &diamond, &ArrangeOptions::default());
    assert!(r.placed.len() >= 15, "only {}", r.placed.len());
    let (s, c) = 2f64.to_radians().sin_cos();
    let tilted =
        Shape2 { outer: Shape2::rectangle(40.0, 30.0).outer.iter().map(|p| v(p.x * c - p.y * s, p.x * s + p.y * c)).collect(), holes: Vec::new() };
    let r = run(&[item(Shape2::rectangle(10.0, 10.0), 12, &[])], &tilted, &ArrangeOptions::default());
    assert!(r.placed.len() >= 6, "only {}", r.placed.len());
}

#[test]
fn a_sheet_far_from_the_origin_loses_no_precision() {
    let far = Shape2 { outer: Shape2::rectangle(10.0, 10.0).outer.iter().map(|p| v(p.x + 1e8, p.y + 1e8)).collect(), holes: Vec::new() };
    let r = run(&[item(Shape2::rectangle(1.0, 1.0), 100, &[])], &far, &ArrangeOptions::default());
    assert_eq!(r.placed.len(), 100);
}

#[test]
fn the_order_search_finds_a_better_order() {
    // Largest first puts the 6 x 6 in and shuts out the four 5 x 5s, which alone fill the sheet.
    let items = [item(Shape2::rectangle(6.0, 6.0), 1, &[]), item(Shape2::rectangle(5.0, 5.0), 4, &[])];
    let sheet = Shape2::rectangle(10.0, 10.0);
    let first = run(&items, &sheet, &ArrangeOptions::default());
    assert_eq!(first.unplaced, vec![0, 4]);
    let searched = run(&items, &sheet, &ArrangeOptions { improve: 20, ..Default::default() });
    assert_eq!(searched.unplaced, vec![1, 0]);
}

#[test]
fn a_run_cancelled_midway_stops_soon() {
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let worker = std::thread::spawn(move || {
        let items = [item(ngon(64, 1.0), 3_000, &[0.0, 90.0])];
        let started = std::time::Instant::now();
        let r = arrange(&items, &Shape2::rectangle(200.0, 200.0), &ArrangeOptions::default(), Some(&flag));
        (r, started.elapsed())
    });
    std::thread::sleep(std::time::Duration::from_millis(50));
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    let (r, took) = worker.join().unwrap();
    assert_eq!(r, Err(ArrangeError::Cancelled));
    assert!(took < std::time::Duration::from_secs(10), "took {took:?}");
}

// ---------- hostile input ----------

#[test]
fn hostile_input_is_an_error_never_a_panic() {
    let sheet = Shape2::rectangle(100.0, 100.0);
    let ok = || item(Shape2::rectangle(5.0, 5.0), 1, &[]);
    let opts = ArrangeOptions::default();
    let err = |items: &[ArrangeItem], sheet: &Shape2, opts: &ArrangeOptions| arrange(items, sheet, opts, None).unwrap_err();

    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300] {
        let shape = poly(&[(0.0, 0.0), (bad, 0.0), (5.0, 5.0)]);
        assert!(matches!(err(&[item(shape.clone(), 1, &[])], &sheet, &opts), ArrangeError::NotFinite { .. }), "{bad}");
        assert!(matches!(err(&[ok()], &shape, &opts), ArrangeError::NotFinite { .. }), "{bad}");
        for (spacing, margin) in [(bad, 0.0), (0.0, bad)] {
            assert!(matches!(err(&[ok()], &sheet, &ArrangeOptions { spacing, margin, ..Default::default() }), ArrangeError::BadDistance { .. }));
        }
        if !bad.is_finite() {
            assert_eq!(err(&[item(Shape2::rectangle(5.0, 5.0), 1, &[0.0, bad])], &sheet, &opts), ArrangeError::BadRotation { item: 0 });
        }
    }
    assert!(matches!(
        err(&[ok()], &sheet, &ArrangeOptions { spacing: -1.0, ..Default::default() }),
        ArrangeError::BadDistance { what: "spacing", .. }
    ));
    assert!(matches!(err(&[item(poly(&[(0.0, 0.0), (1.0, 1.0)]), 1, &[])], &sheet, &opts), ArrangeError::TooFewCorners { .. }));
    assert!(matches!(err(&[item(Shape2::default(), 1, &[])], &sheet, &opts), ArrangeError::TooFewCorners { .. }));
    assert!(matches!(err(&[item(poly(&[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)]), 1, &[])], &sheet, &opts), ArrangeError::NoArea { .. }));
    assert!(matches!(
        err(&[item(poly(&[(0.0, 0.0), (10.0, 0.0), (0.0, 10.0), (5.0, 10.0)]), 1, &[])], &sheet, &opts),
        ArrangeError::SelfCrossing { .. }
    ));
    let hole_outside = Shape2 { outer: Shape2::rectangle(5.0, 5.0).outer, holes: vec![vec![v(10.0, 10.0), v(11.0, 10.0), v(11.0, 11.0)]] };
    assert!(matches!(err(&[item(hole_outside, 1, &[])], &sheet, &opts), ArrangeError::HoleOutside { .. }));
    let hole_crossing = Shape2 { outer: Shape2::rectangle(5.0, 5.0).outer, holes: vec![vec![v(1.0, 1.0), v(8.0, 1.0), v(8.0, 2.0), v(1.0, 2.0)]] };
    assert!(matches!(err(&[item(hole_crossing, 1, &[])], &sheet, &opts), ArrangeError::SelfCrossing { .. } | ArrangeError::HoleOutside { .. }));
    assert_eq!(err(&[item(Shape2::rectangle(1.0, 1.0), MAX_COPIES + 1, &[])], &sheet, &opts), ArrangeError::TooManyCopies);
    assert_eq!(
        err(&[item(Shape2::rectangle(1.0, 1.0), usize::MAX, &[]), item(Shape2::rectangle(1.0, 1.0), usize::MAX, &[])], &sheet, &opts),
        ArrangeError::TooManyCopies
    );
    let many: Vec<f64> = (0..=MAX_ROTATIONS).map(|i| i as f64).collect();
    assert_eq!(err(&[item(Shape2::rectangle(1.0, 1.0), 1, &many)], &sheet, &opts), ArrangeError::TooManyRotations { item: 0 });
    let big = ngon(MAX_CORNERS + 1, 10.0);
    assert!(matches!(err(&[item(big, 1, &[])], &sheet, &opts), ArrangeError::TooManyCorners { .. }));
    assert_eq!(err(&[ok()], &sheet, &ArrangeOptions { improve: MAX_IMPROVE + 1, ..Default::default() }), ArrangeError::TooManyTries);
    let figure_eight = poly(&[(0.0, 0.0), (10.0, 0.0), (5.0, 5.0), (2.0, 8.0), (8.0, 8.0), (5.0, 5.0)]);
    assert!(matches!(err(&[item(figure_eight, 1, &[])], &sheet, &opts), ArrangeError::SelfCrossing { .. }));
    let square = |x: f64, w: f64| vec![v(x, x), v(x + w, x), v(x + w, x + w), v(x, x + w)];
    let nested = Shape2 { outer: Shape2::rectangle(20.0, 20.0).outer, holes: vec![square(2.0, 16.0), square(5.0, 4.0)] };
    assert!(matches!(err(&[item(nested, 1, &[])], &sheet, &opts), ArrangeError::HolesNested { .. }));
    let twice = Shape2 { outer: Shape2::rectangle(20.0, 20.0).outer, holes: vec![square(5.0, 4.0), square(5.0, 4.0)] };
    assert!(matches!(err(&[item(twice, 1, &[])], &sheet, &opts), ArrangeError::SelfCrossing { .. }));
    let all_turns: Vec<f64> = (0..MAX_ROTATIONS).map(|i| i as f64).collect();
    assert_eq!(err(&[item(ngon(MAX_CORNERS, 10.0), 1, &all_turns)], &sheet, &opts), ArrangeError::TooMuchWork);
    // A part listed with no copies costs nothing, however big.
    assert!(arrange(&[item(ngon(MAX_CORNERS, 10.0), 0, &all_turns)], &sheet, &opts, None).is_ok());
}

#[test]
fn odd_but_valid_input_still_lays_out() {
    let sheet = Shape2::rectangle(100.0, 100.0);
    // Huge and negative angles, repeats, clockwise rings, a sheet far from the origin.
    let cw = poly(&[(0.0, 0.0), (0.0, 8.0), (3.0, 8.0), (3.0, 0.0)]);
    let items = [item(cw, 10, &[-90.0, 270.0, 1e6, 720.0, 0.0])];
    run(&items, &sheet, &ArrangeOptions::default());
    let far = Shape2 { outer: sheet.outer.iter().map(|p| v(p.x + 1e8, p.y - 1e8)).collect(), holes: Vec::new() };
    let r = run(&[item(Shape2::rectangle(10.0, 10.0), 4, &[])], &far, &ArrangeOptions::default());
    assert_eq!(r.placed.len(), 4);
    // A spacing larger than the sheet places one part at most.
    let r = run(&[item(Shape2::rectangle(1.0, 1.0), 5, &[])], &sheet, &ArrangeOptions { spacing: 1e6, ..Default::default() });
    assert_eq!(r.placed.len(), 1);
    // A margin larger than half the sheet places nothing.
    let r = run(&[item(Shape2::rectangle(1.0, 1.0), 5, &[])], &sheet, &ArrangeOptions { margin: 60.0, ..Default::default() });
    assert!(r.placed.is_empty());
    // Tiny slivers next to big parts.
    let sliver = poly(&[(0.0, 0.0), (50.0, 0.0), (50.0, 1e-3)]);
    run(&[item(sliver, 20, &[0.0, 180.0]), item(Shape2::rectangle(40.0, 40.0), 2, &[])], &sheet, &ArrangeOptions::default());
}

#[test]
fn obstacles_stand_where_they_are_and_parts_keep_clear() {
    // A 30 x 30 obstacle in the middle of a 50 x 50 sheet leaves a 10 wide frame round it, which
    // takes 16 squares of 10 at no spacing, and none at a spacing of 1.
    let sheet = Shape2::rectangle(50.0, 50.0);
    let block = Shape2 { outer: vec![v(10.0, 10.0), v(40.0, 10.0), v(40.0, 40.0), v(10.0, 40.0)], holes: Vec::new() };
    let item = ArrangeItem { shape: Shape2::rectangle(10.0, 10.0), quantity: 20, rotations_deg: vec![0.0] };
    let opts = ArrangeOptions { part_in_part: false, ..ArrangeOptions::default() };
    let r = arrange_around(std::slice::from_ref(&item), &sheet, std::slice::from_ref(&block), &opts, &|| false).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r.placed.len(), 16, "{r:?}");
    for p in &r.placed {
        let (x, y) = (p.shift.x, p.shift.y);
        assert!(x + 10.0 <= 10.0 + 1e-9 || x >= 40.0 - 1e-9 || y + 10.0 <= 10.0 + 1e-9 || y >= 40.0 - 1e-9, "on the obstacle: {p:?}");
    }
    let spaced = ArrangeOptions { spacing: 1.0, ..opts.clone() };
    let r = arrange_around(std::slice::from_ref(&item), &sheet, std::slice::from_ref(&block), &spaced, &|| false).unwrap_or_else(|e| panic!("{e}"));
    assert!(r.placed.is_empty(), "{r:?}");
    // One far away is no obstacle; fill counts the parts only.
    let far = Shape2 { outer: block.outer.iter().map(|p| v(p.x + 500.0, p.y)).collect(), holes: Vec::new() };
    let r = arrange_around(std::slice::from_ref(&item), &sheet, &[far], &opts, &|| false).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r.placed.len(), 20, "{r:?}");
    assert!((r.fill - 0.8).abs() < 1e-9, "{r:?}");
}
