#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::*;

fn plane() -> Plane {
    Plane::new(Vec3::ZERO, Vec3::X, Vec3::Y).unwrap()
}

fn view(scale: f64, pixel_scale: f64) -> PixelCurveView {
    PixelCurveView {
        vp: Mat4([[scale, 0.0, 0.0, 0.0], [0.0, scale, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]),
        width_px: 1400.0 * pixel_scale,
        height_px: 900.0 * pixel_scale,
    }
}

fn dense_error(s: &Seg2, plane: &Plane, v: &PixelCurveView, path: &SampledPath) -> f64 {
    let segments: Vec<_> =
        path.parts.iter().flat_map(|part| part.windows(2)).map(|w| (screen(v.vp.apply(w[0]), v), screen(v.vp.apply(w[1]), v))).collect();
    (0..=12_000)
        .filter_map(|i| {
            let q = v.vp.apply(plane.to_world(s.point_at(i as f64 / 12_000.0)));
            (distances(q, v, true).iter().all(|x| *x >= 0.0))
                .then(|| segments.iter().map(|(a, b)| distance_segment(screen(q, v), *a, *b)).fold(f64::INFINITY, f64::min))
        })
        .fold(0.0, f64::max)
}

#[test]
fn exact_arcs_ellipses_rational_and_looping_control_hulls_bound_pixels() {
    let mut degree7 = [Vec2::ZERO; solidcraft_geom::MAX_BEZIER_CTRL];
    for (i, p) in degree7.iter_mut().enumerate() {
        *p = Vec2::new(i as f64 / 4.0 - 0.875, if i % 2 == 0 { -0.8 } else { 0.8 });
    }
    let curves = [
        Seg2::Arc { center: Vec2::ZERO, radius: 1.0, start: 0.27, sweep: -5.8 },
        Seg2::Conic { a: Vec2::new(0.8, 0.0), apex: Vec2::new(0.8, 0.025), b: Vec2::new(0.0, 0.025), w: 0.5_f64.sqrt() },
        Seg2::Conic { a: Vec2::new(-0.8, -0.4), apex: Vec2::new(0.0, 0.8), b: Vec2::new(0.8, -0.4), w: 0.06 },
        Seg2::Cubic { p0: Vec2::new(-0.8, 0.0), p1: Vec2::new(0.9, 0.9), p2: Vec2::new(-0.9, -0.9), p3: Vec2::new(0.8, 0.0) },
        Seg2::Cubic { p0: Vec2::new(0.0, -0.4), p1: Vec2::new(1.0, 0.9), p2: Vec2::new(-1.0, 0.9), p3: Vec2::new(0.0, -0.4) },
        Seg2::Bezier { n: 7, p: degree7 },
    ];
    for scale in [0.003, 0.03, 0.3, 1.3] {
        for pixels in [1.0, 1.5, 2.0] {
            for s in curves {
                let v = view(scale, pixels);
                let path = sample_segment(&s, &plane(), &v, &mut SamplingBudget::default());
                assert_eq!(path.quality, SamplingQuality::Complete, "{s:?}");
                assert!(dense_error(&s, &plane(), &v, &path) <= 0.501, "{s:?}");
                assert!(path.vertex_count < 1024);
            }
        }
    }
}

#[test]
fn fit_circle_zoom_budget_and_closed_unclipped_fill() {
    let s = Seg2::Arc { center: Vec2::ZERO, radius: 1.0, start: 0.0, sweep: std::f64::consts::TAU };
    let small = sample_segment(&s, &plane(), &view(0.003, 1.0), &mut SamplingBudget::default());
    let fitted = sample_segment(&s, &plane(), &view(219.0 / 700.0, 1.0), &mut SamplingBudget::default());
    assert!(fitted.vertex_count > small.vertex_count);
    assert!(dense_error(&s, &plane(), &view(219.0 / 700.0, 1.0), &fitted) < 0.5);
    let fill = sample_segment_unclipped(&s, &plane(), &view(3.0, 2.0), &mut SamplingBudget::default());
    assert_eq!(fill.quality, SamplingQuality::Complete);
    assert_eq!(fill.parts.len(), 1);
    let part = &fill.parts[0];
    assert!(part.first().unwrap().dist(*part.last().unwrap()) < 1e-12);
    let limited = sample_segment_unclipped(&s, &plane(), &view(1e20, 1.0), &mut SamplingBudget::new(64, 1000));
    assert_eq!(limited.quality, SamplingQuality::BudgetLimited);
    assert!(limited.vertex_count <= 64);
    assert!(limited.parts.iter().flatten().all(|p| [p.x, p.y, p.z].iter().all(|v| v.is_finite())));
}

#[test]
fn perspective_tilt_exact_s_curve_and_clipping_do_not_bridge_gaps() {
    let p = Plane::new(Vec3::new(0.0, 0.0, -2.0), Vec3::new(0.8, 0.0, 0.6), Vec3::Y).unwrap();
    let v = PixelCurveView {
        vp: Mat4([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, -1.01, -1.0], [0.0, 0.0, -0.101, 0.0]]),
        width_px: 900.0,
        height_px: 720.0,
    };
    let s = Seg2::Cubic { p0: Vec2::new(-0.9, 0.0), p1: Vec2::new(0.9, 1.0), p2: Vec2::new(-0.9, -1.0), p3: Vec2::new(0.9, 0.0) };
    let path = sample_segment(&s, &p, &v, &mut SamplingBudget::default());
    assert_eq!(path.quality, SamplingQuality::Complete);
    assert!(dense_error(&s, &p, &v, &path) < 0.501);
    let crossing = Plane::new(Vec3::new(0.0, 0.0, -0.15), Vec3::new(0.6, 0.0, 0.8), Vec3::Y).unwrap();
    let s = Seg2::Arc { center: Vec2::ZERO, radius: 0.2, start: std::f64::consts::PI, sweep: std::f64::consts::TAU };
    let path = sample_segment_unclipped(&s, &crossing, &v, &mut SamplingBudget::default());
    assert!(!path.parts.is_empty());
    assert_eq!(path.quality, SamplingQuality::BudgetLimited);
    for point in path.parts.iter().flatten() {
        let q = v.vp.apply(*point);
        assert!(q[3] > 0.0 && q[2] >= -1e-10);
    }
    assert!(path.parts.len() >= 2);
}

#[test]
fn invalid_weights_matrices_and_shared_frame_exhaustion_are_explicit() {
    let bad = Seg2::Conic { a: Vec2::ZERO, apex: Vec2::X, b: Vec2::Y, w: -1.0 };
    assert_eq!(sample_segment(&bad, &plane(), &view(1.0, 1.0), &mut SamplingBudget::default()).quality, SamplingQuality::Invalid);
    let line = Seg2::Line { a: Vec2::ZERO, b: Vec2::X };
    let mut budget = SamplingBudget::new(2, 2);
    assert_eq!(sample_segment(&line, &plane(), &view(0.2, 1.0), &mut budget).quality, SamplingQuality::Complete);
    assert_eq!(sample_segment(&line, &plane(), &view(0.2, 1.0), &mut budget).quality, SamplingQuality::BudgetLimited);
    let mut bad = view(1.0, 1.0);
    bad.vp.0[0][0] = f64::NAN;
    assert_eq!(sample_segment(&line, &plane(), &bad, &mut SamplingBudget::default()).quality, SamplingQuality::Invalid);
    // The projected coordinates themselves are small, but huge VP summands cancel.
    // Do not certify accuracy from the small result alone.
    let far = Plane::new(Vec3::new(1e15, 0.0, 0.0), Vec3::X, Vec3::Y).unwrap();
    let mut cancellation = view(0.2, 1.0);
    cancellation.vp.0[3][0] = -2e14;
    let path = sample_segment_unclipped(&line, &far, &cancellation, &mut SamplingBudget::new(16, 100));
    assert_eq!(path.quality, SamplingQuality::BudgetLimited);
    assert!(path.vertex_count <= 16);
    let plane_cancellation = Plane::new(Vec3::new(1e9, 0.0, 0.0), Vec3::X, Vec3::Y).unwrap();
    let local_circle = Seg2::Arc { center: Vec2::new(-1e9 + 1.0, 0.0), radius: 1.0, start: 0.0, sweep: std::f64::consts::TAU };
    let path = sample_segment_unclipped(&local_circle, &plane_cancellation, &view(1e9, 1.0), &mut SamplingBudget::new(16, 100));
    assert_eq!(path.quality, SamplingQuality::BudgetLimited);
    assert!(path.vertex_count <= 16);
}

// The sampler as it was before its control points were kept inline and each point's
// projection and clip distances were worked out once per node, kept to compare against.
fn reference_controls(segment: &Seg2, plane: &Plane) -> Option<(Vec<Vec<H>>, H)> {
    let mut terms: H = [0.0; 4];
    let mut h = |p: Vec2, w: f64| {
        for (r, (o, x, y)) in [(plane.origin.x, plane.x.x, plane.y.x), (plane.origin.y, plane.x.y, plane.y.y), (plane.origin.z, plane.x.z, plane.y.z)]
            .into_iter()
            .enumerate()
        {
            terms[r] = terms[r].max((o.abs() + x.abs() * p.x.abs() + y.abs() * p.y.abs()) * w.abs());
        }
        terms[3] = terms[3].max(w.abs());
        let p = plane.to_world(p);
        [p.x * w, p.y * w, p.z * w, w]
    };
    let spans = match *segment {
        Seg2::Line { a, b } => vec![vec![h(a, 1.0), h(b, 1.0)]],
        Seg2::Cubic { p0, p1, p2, p3 } => vec![vec![h(p0, 1.0), h(p1, 1.0), h(p2, 1.0), h(p3, 1.0)]],
        Seg2::Conic { a, apex, b, w } if w > 0.0 => vec![vec![h(a, 1.0), h(apex, w), h(b, 1.0)]],
        Seg2::Bezier { n, p } if (1..=solidcraft_geom::MAX_BEZIER_DEGREE as u8).contains(&n) => {
            vec![p[..=n as usize].iter().map(|p| h(*p, 1.0)).collect()]
        }
        Seg2::Arc { center, radius, start, sweep }
            if radius > 0.0 && start.is_finite() && sweep.is_finite() && sweep.abs() <= std::f64::consts::TAU + 1e-9 =>
        {
            let count = (sweep.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
            (0..count)
                .map(|i| {
                    let a = start + sweep * i as f64 / count as f64;
                    let b = start + sweep * (i + 1) as f64 / count as f64;
                    let mid = (a + b) * 0.5;
                    let w = ((b - a) * 0.5).cos();
                    let p = |t: f64, r: f64| center + Vec2::new(t.cos(), t.sin()) * r;
                    vec![h(p(a, radius), 1.0), h(p(mid, radius / w), w), h(p(b, radius), 1.0)]
                })
                .collect()
        }
        _ => return None,
    };
    spans.iter().flatten().all(|p| p.iter().all(|x| x.is_finite()) && p[3] > 0.0).then_some((spans, terms))
}

fn reference_halves(c: &[H]) -> (Vec<H>, Vec<H>) {
    let mut work = c.to_vec();
    let mut l = Vec::with_capacity(c.len());
    let mut r = Vec::with_capacity(c.len());
    while let (Some(a), Some(b)) = (work.first(), work.last()) {
        l.push(*a);
        r.push(*b);
        if work.len() == 1 {
            break;
        }
        work = work.windows(2).map(|p| mix(p[0], p[1], 0.5)).collect();
    }
    r.reverse();
    (l, r)
}

fn reference_sample(segment: &Seg2, plane: &Plane, view: &PixelCurveView, budget: &mut SamplingBudget, clip_xy: bool) -> SampledPath {
    let mut out = SampledPath { parts: Vec::new(), quality: SamplingQuality::Complete, vertex_count: 0 };
    if !view.width_px.is_finite()
        || !view.height_px.is_finite()
        || view.width_px <= 0.0
        || view.height_px <= 0.0
        || view.width_px > 1e7
        || view.height_px > 1e7
        || view.vp.0.iter().flatten().any(|x| !x.is_finite())
    {
        out.quality = SamplingQuality::Invalid;
        return out;
    }
    let Some((spans, source_terms)) = reference_controls(segment, plane) else {
        out.quality = SamplingQuality::Invalid;
        return out;
    };
    let mut stack: Vec<_> = spans
        .into_iter()
        .rev()
        .map(|c| {
            let magnitude: H = std::array::from_fn(|i| c.iter().map(|p| p[i].abs()).fold(source_terms[i], f64::max));
            let terms: H = std::array::from_fn(|r| (0..4).map(|i| view.vp.0[i][r].abs() * magnitude[i]).sum());
            (c, 0u8, terms)
        })
        .collect();
    let mut connected = false;
    let mut nodes = 0usize;
    while let Some((c, depth, terms)) = stack.pop() {
        if budget.remaining_nodes == 0 || nodes == 32_768 {
            out.quality = SamplingQuality::BudgetLimited;
            break;
        }
        budget.remaining_nodes -= 1;
        nodes += 1;
        let q: Vec<H> = c.iter().map(|h| project_h(view.vp, *h)).collect();
        if q.iter().flatten().any(|x| !x.is_finite()) {
            out.quality = SamplingQuality::Invalid;
            break;
        }
        let e: H = terms.map(|t| t * f64::EPSILON * (f64::from(depth) + 4.0) * 64.0);
        let clip_error = [
            e[0] + e[3] * (1.0 + 2.0 / view.width_px),
            e[0] + e[3] * (1.0 + 2.0 / view.width_px),
            e[1] + e[3] * (1.0 + 2.0 / view.height_px),
            e[1] + e[3] * (1.0 + 2.0 / view.height_px),
            e[2],
            e[2] + e[3],
            e[3],
        ];
        if (0..7).any(|i| q.iter().all(|p| distances(*p, view, clip_xy)[i] < -clip_error[i])) {
            connected = false;
            continue;
        }
        let positive = q.iter().all(|p| p[3] > 1e-12);
        let depth_safe = c.len() == 2 || (4..7).all(|i| q.iter().all(|p| distances(*p, view, clip_xy)[i] >= 0.0));
        let flat = positive
            && q.first().zip(q.last()).is_some_and(|(a, b)| {
                let (a, b) = (screen(*a, view), screen(*b, view));
                let magnitude = q.iter().map(|p| screen(*p, view).len()).fold(1.0, f64::max);
                // Keep floating-point roundoff inside the advertised bound.
                // Absolute summands also cover cancellation in VP multiplication.
                // Subdivision propagates roundoff from the original homogeneous hull.
                let projection_error = q
                    .iter()
                    .map(|p| {
                        let safe_w = p[3] - e[3];
                        if safe_w <= 0.0 {
                            return f64::INFINITY;
                        }
                        let x = (e[0] + (p[0] / p[3]).abs() * e[3]) / safe_w * view.width_px * 0.5;
                        let y = (e[1] + (p[1] / p[3]).abs() * e[3]) / safe_w * view.height_px * 0.5;
                        x.hypot(y)
                    })
                    .fold(0.0, f64::max);
                let allowance = projection_error + magnitude * f64::EPSILON * 128.0;
                allowance < PIXEL_ERROR && q.iter().all(|p| distance_segment(screen(*p, view), a, b) + allowance <= PIXEL_ERROR)
            });
        if !flat || !depth_safe {
            if depth == MAX_DEPTH || out.vertex_count >= MAX_VERTICES {
                out.quality = SamplingQuality::BudgetLimited;
                connected = false;
                continue;
            }
            let (l, r) = reference_halves(&c);
            stack.push((r, depth + 1, terms));
            stack.push((l, depth + 1, terms));
            continue;
        }
        let (Some(a), Some(b), Some(qa), Some(qb)) = (c.first(), c.last(), q.first(), q.last()) else { continue };
        let mut lo: f64 = 0.0;
        let mut hi: f64 = 1.0;
        for (a, b) in distances(*qa, view, clip_xy).into_iter().zip(distances(*qb, view, clip_xy)) {
            if a < 0.0 && b < 0.0 {
                hi = -1.0;
                break;
            }
            if a < 0.0 {
                lo = lo.max(a / (a - b));
            } else if b < 0.0 {
                hi = hi.min(a / (a - b));
            }
        }
        if lo > hi {
            connected = false;
            continue;
        }
        let (Some(a), Some(b)) = (world(mix(*a, *b, lo)), world(mix(*a, *b, hi))) else {
            out.quality = SamplingQuality::Invalid;
            break;
        };
        // Exact consecutive subdivision endpoints share their homogeneous value.
        let join = connected && lo == 0.0 && out.parts.last().and_then(|p| p.last()).is_some_and(|p| *p == a);
        let need = if join { 1 } else { 2 };
        if need > budget.remaining_vertices || out.vertex_count + need > MAX_VERTICES {
            out.quality = SamplingQuality::BudgetLimited;
            break;
        }
        budget.remaining_vertices -= need;
        out.vertex_count += need;
        if join {
            if let Some(part) = out.parts.last_mut() {
                part.push(b);
            }
        } else {
            out.parts.push(vec![a, b]);
        }
        connected = hi == 1.0;
    }
    out
}

fn same_path(a: &SampledPath, b: &SampledPath) -> bool {
    let bits = |p: &SampledPath| -> Vec<Vec<[u64; 3]>> {
        p.parts.iter().map(|part| part.iter().map(|v| [v.x, v.y, v.z].map(f64::to_bits)).collect()).collect()
    };
    a.quality == b.quality && a.vertex_count == b.vertex_count && bits(a) == bits(b)
}

/// The sampler gives the very same points, bit for bit, as before it was made quicker: every
/// kind of segment, on planes at a slant and far off, in orthographic and perspective views
/// zoomed from far out to far in, cut by the view's edges and the near plane, clipped and
/// unclipped, with ample and with nearly spent budgets.
#[test]
fn samples_the_same_points_bit_for_bit_as_the_reference() {
    let mut degree9 = [Vec2::ZERO; solidcraft_geom::MAX_BEZIER_CTRL];
    for (i, p) in degree9.iter_mut().enumerate().take(10) {
        let t = i as f64 / 9.0;
        *p = Vec2::new(t * 2.0 - 1.0, ((i % 3) as f64 - 1.0) * 0.8);
    }
    let segments = [
        Seg2::Line { a: Vec2::new(-0.7, -0.3), b: Vec2::new(0.9, 0.4) },
        Seg2::Line { a: Vec2::new(-50.0, 0.1), b: Vec2::new(50.0, 0.2) },
        Seg2::Cubic { p0: Vec2::new(-0.9, 0.0), p1: Vec2::new(0.9, 1.0), p2: Vec2::new(-0.9, -1.0), p3: Vec2::new(0.9, 0.0) },
        Seg2::Cubic { p0: Vec2::new(0.0, 0.0), p1: Vec2::new(1.0, 1.0), p2: Vec2::new(0.0, 1.0), p3: Vec2::new(1.0, 0.0) },
        Seg2::Conic { a: Vec2::new(-0.5, 0.0), apex: Vec2::new(0.0, 0.6), b: Vec2::new(0.5, 0.0), w: 0.3 },
        Seg2::Conic { a: Vec2::new(-0.5, 0.0), apex: Vec2::new(0.0, 0.6), b: Vec2::new(0.5, 0.0), w: 4.0 },
        Seg2::Bezier { n: 9, p: degree9 },
        Seg2::Bezier { n: 4, p: degree9 },
        Seg2::Arc { center: Vec2::ZERO, radius: 0.4, start: 0.3, sweep: std::f64::consts::TAU },
        Seg2::Arc { center: Vec2::new(0.2, -0.1), radius: 3.0, start: -1.0, sweep: -2.5 },
        Seg2::Arc { center: Vec2::ZERO, radius: 0.2, start: std::f64::consts::PI, sweep: 0.001 },
    ];
    let planes = [
        plane(),
        Plane::new(Vec3::new(0.0, 0.0, -2.0), Vec3::new(0.8, 0.0, 0.6), Vec3::Y).unwrap(),
        Plane::new(Vec3::new(0.0, 0.0, -0.15), Vec3::new(0.6, 0.0, 0.8), Vec3::Y).unwrap(),
        Plane::new(Vec3::new(1e6, -3e5, 2.0), Vec3::new(0.0, 0.6, 0.8), Vec3::new(1.0, 0.0, 0.0)).unwrap(),
    ];
    let perspective = |s: f64| PixelCurveView {
        vp: Mat4([[s, 0.0, 0.0, 0.0], [0.0, s, 0.0, 0.0], [0.0, 0.0, -1.01, -1.0], [0.0, 0.0, -0.101, 0.0]]),
        width_px: 900.0,
        height_px: 720.0,
    };
    let mut far = view(0.2, 1.0);
    far.vp.0[3][0] = -2e5;
    let views = [
        view(0.003, 1.0),
        view(0.31, 1.0),
        view(3.0, 2.0),
        view(400.0, 1.25),
        view(1e9, 1.0),
        far,
        perspective(1.0),
        perspective(7.5),
        perspective(0.05),
    ];
    let mut compared = 0;
    for segment in &segments {
        for plane in &planes {
            for view in &views {
                for clip in [true, false] {
                    for (vertices, nodes) in [(200_000, 400_000), (40, 1_000), (7, 9)] {
                        let (mut a, mut b) = (SamplingBudget::new(vertices, nodes), SamplingBudget::new(vertices, nodes));
                        let new = sample(segment, plane, view, &mut a, clip);
                        let old = reference_sample(segment, plane, view, &mut b, clip);
                        assert!(same_path(&new, &old), "{segment:?} on {plane:?} in {view:?}, clipped {clip}, budget {vertices}/{nodes}");
                        assert_eq!((a.remaining_vertices, a.remaining_nodes), (b.remaining_vertices, b.remaining_nodes));
                        compared += new.vertex_count;
                    }
                }
            }
        }
    }
    assert!(compared > 10_000, "the views sample in earnest: {compared} vertices");
}

/// Times the sampler against the reference on a few thousand cubic Béziers in a fitted view.
/// Run with `--ignored --nocapture` in a release build.
#[test]
#[ignore = "manual sampler measurement"]
fn sampler_cost_against_the_reference() {
    let segments: Vec<Seg2> = (0..4096)
        .map(|i| {
            let (x, y) = ((i % 64) as f64 * 3.0, (i / 64) as f64 * 3.0);
            Seg2::Cubic { p0: Vec2::new(x, y), p1: Vec2::new(x + 0.6, y - 0.5), p2: Vec2::new(x + 1.4, y - 0.5), p3: Vec2::new(x + 2.0, y) }
        })
        .collect();
    let v = view(2.0 / 200.0, 1.0);
    let time = |f: &dyn Fn(&Seg2, &mut SamplingBudget) -> SampledPath| {
        let mut t: Vec<f64> = (0..5)
            .map(|_| {
                let start = std::time::Instant::now();
                let mut budget = SamplingBudget::new(1_000_000, 2_000_000);
                let n: usize = segments.iter().map(|s| f(s, &mut budget).vertex_count).sum();
                std::hint::black_box(n);
                start.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        t.sort_by(f64::total_cmp);
        t[2]
    };
    for clip in [true, false] {
        let mut budget = SamplingBudget::new(1_000_000, 2_000_000);
        let vertices: usize = segments.iter().map(|s| sample(s, &plane(), &v, &mut budget, clip).vertex_count).sum();
        println!("SAMPLER clip={clip} segments={} nodes={} vertices={vertices}", segments.len(), 2_000_000 - budget.remaining_nodes);
        let (mut old, mut new) = (Vec::new(), Vec::new());
        for _ in 0..9 {
            old.push(time(&|s, b| reference_sample(s, &plane(), &v, b, clip)));
            new.push(time(&|s, b| sample(s, &plane(), &v, b, clip)));
        }
        old.sort_by(f64::total_cmp);
        new.sort_by(f64::total_cmp);
        println!("SAMPLER clip={clip} reference_ms={:.3} new_ms={:.3} ratio={:.3}", old[4], new[4], new[4] / old[4]);
    }
}
