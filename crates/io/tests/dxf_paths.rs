//! Public path conversion and DXF readback acceptance, using original analytic fixtures.

use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI, TAU};

use solidcraft_geom::{Loop2, MAX_BEZIER_CTRL, Seg2, Vec2, bezier};
use solidcraft_io::{DxfEntity, DxfPath, DxfPathStyle, DxfShape, DxfVertex, Geom2, IoError, path_shapes, read_dxf, write_dxf_drawing};

// Accepted fixtures span at most 40 mm. Legacy entities keep twelve decimal places;
// bulged polylines may retain more. These bounds allow roundoff in the independent
// inverse-bulge calculation and reader, not fitting error or an output-format requirement.
const POSITION_EPS: f64 = 1e-8;
const BULGE_EPS: f64 = 1e-11;

fn point_near(actual: Vec2, expected: Vec2) {
    assert!(actual.dist(expected) < POSITION_EPS, "actual {actual:?}, expected {expected:?}");
}

fn number_near(actual: f64, expected: f64, tolerance: f64) {
    assert!((actual - expected).abs() < tolerance, "actual {actual}, expected {expected}, tolerance {tolerance}");
}

fn styles() -> [DxfPathStyle; 3] {
    [DxfPathStyle::Exact, DxfPathStyle::Polyline, DxfPathStyle::Arcs { tolerance_mm: 0.01 }]
}

fn drawing(shapes: &[DxfShape]) -> String {
    let entities: Vec<_> = shapes.iter().cloned().map(|shape| DxfEntity { layer: "0".into(), shape }).collect();
    write_dxf_drawing(&[], &entities).unwrap()
}

type RawEntity<'a> = (&'a str, Vec<(i32, &'a str)>);

fn raw_entities(text: &str) -> Vec<RawEntity<'_>> {
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len() % 2, 0);
    let pairs: Vec<_> = lines.chunks_exact(2).map(|p| (p[0].trim().parse::<i32>().unwrap(), p[1].trim())).collect();
    let start = pairs.iter().position(|p| *p == (2, "ENTITIES")).unwrap();
    let mut entities: Vec<RawEntity<'_>> = Vec::new();
    for &(code, value) in &pairs[start + 1..] {
        match (code, value) {
            (0, "ENDSEC") => return entities,
            (0, kind) => entities.push((kind, Vec::new())),
            pair => entities.last_mut().unwrap().1.push(pair),
        }
    }
    panic!("missing ENTITIES terminator");
}

fn scalar(entity: &RawEntity<'_>, code: i32) -> f64 {
    let values: Vec<_> = entity.1.iter().filter(|p| p.0 == code).map(|p| p.1.parse::<f64>().unwrap()).collect();
    assert_eq!(values.len(), 1, "group {code}: {entity:?}");
    values[0]
}

// Read group 42 beside its owning vertex, rather than collecting all bulges separately:
// omitting a zero group must not shift the next nonzero bulge to a different edge.
fn raw_polyline(entity: &RawEntity<'_>) -> (Vec<DxfVertex>, bool) {
    assert_eq!(entity.0, "LWPOLYLINE");
    let mut vertices: Vec<DxfVertex> = Vec::new();
    for &(code, value) in &entity.1 {
        match code {
            10 => vertices.push(DxfVertex { at: Vec2::new(value.parse().unwrap(), f64::NAN), bulge: 0.0 }),
            20 => vertices.last_mut().unwrap().at.y = value.parse().unwrap(),
            42 => vertices.last_mut().unwrap().bulge = value.parse().unwrap(),
            _ => {}
        }
    }
    assert_eq!(scalar(entity, 90), vertices.len() as f64);
    assert!(vertices.iter().all(|v| v.at.is_finite() && v.bulge.is_finite()));
    let flags = scalar(entity, 70) as u32;
    (vertices, flags & 1 != 0)
}

fn one_polyline(shapes: &[DxfShape], closed: bool) -> &[DxfVertex] {
    let [DxfShape::BulgedPolyline { vertices, closed: actual }] = shapes else { panic!("{shapes:?}") };
    assert_eq!(*actual, closed);
    vertices
}

fn assert_serialized_vertices(text: &str, expected: &[DxfVertex], closed: bool) -> Vec<DxfVertex> {
    let entities = raw_entities(text);
    assert_eq!(entities.len(), 1);
    let (vertices, actual_closed) = raw_polyline(&entities[0]);
    assert_eq!(actual_closed, closed);
    assert_eq!(vertices.len(), expected.len());
    for (actual, expected) in vertices.iter().zip(expected) {
        point_near(actual.at, expected.at);
        number_near(actual.bulge, expected.bulge, BULGE_EPS);
    }
    vertices
}

// Algebraic inverse from chord and sagitta ratio; deliberately does not use the reader's
// atan/sin/cos construction, the converter, or Seg2::fit_arcs as an oracle.
fn assert_arc_support(a: &DxfVertex, b: &DxfVertex, center: Vec2, radius: f64) {
    assert!(a.bulge != 0.0);
    let chord = b.at - a.at;
    let square = a.bulge * a.bulge;
    let recovered_center = (a.at + b.at) * 0.5 + chord.perp() * ((1.0 - square) / (4.0 * a.bulge));
    let recovered_radius = chord.len() * (1.0 + square) / (4.0 * a.bulge.abs());
    point_near(recovered_center, center);
    number_near(recovered_radius, radius, POSITION_EPS);
}

fn assert_readback(text: &str, expected: &[Seg2]) {
    let actual = read_dxf(text).unwrap();
    assert_eq!(actual.len(), expected.len(), "{actual:?}");
    for (actual, expected) in actual.iter().zip(expected) {
        match (actual, expected) {
            (Geom2::Line(a, b), Seg2::Line { a: ea, b: eb }) => {
                point_near(*a, *ea);
                point_near(*b, *eb);
            }
            (Geom2::Arc { c, a, b }, Seg2::Arc { center, radius, sweep, .. }) => {
                point_near(*c, *center);
                number_near(a.dist(*c), *radius, POSITION_EPS);
                number_near(b.dist(*c), *radius, POSITION_EPS);
                // Geom2 normalizes clockwise arcs to counter-clockwise endpoint order.
                let (ea, eb) = if *sweep > 0.0 { (expected.start(), expected.end()) } else { (expected.end(), expected.start()) };
                point_near(*a, ea);
                point_near(*b, eb);
            }
            _ => panic!("read {actual:?}, expected {expected:?}"),
        }
    }
}

fn assert_arc_style(shapes: &[DxfShape], segment: &Seg2) {
    assert!(!shapes.is_empty(), "nonempty curve was discarded");
    assert!(shapes.iter().all(|s| matches!(s, DxfShape::Line(..) | DxfShape::Arc { .. } | DxfShape::Circle { .. })));
    let ends: Vec<_> = shapes
        .iter()
        .flat_map(|shape| match shape {
            DxfShape::Line(a, b) => vec![*a, *b],
            DxfShape::Arc { c, r, a0, a1 } => vec![*c + Vec2::from_angle(a0.to_radians()) * *r, *c + Vec2::from_angle(a1.to_radians()) * *r],
            _ => Vec::new(),
        })
        .collect();
    for endpoint in [segment.start(), segment.end()] {
        assert!(ends.iter().any(|p| p.dist(endpoint) < POSITION_EPS), "missing endpoint {endpoint:?}: {shapes:?}");
    }
}

#[test]
fn primitive_paths_dispatch_by_style_without_losing_arc_direction() {
    let line = Seg2::Line { a: Vec2::new(2.0, -3.0), b: Vec2::new(11.0, 4.0) };
    for style in styles() {
        let out = path_shapes(DxfPath::Open(&[line]), style).unwrap();
        match style {
            DxfPathStyle::Polyline => {
                let vertices = one_polyline(&out, false);
                assert_eq!(vertices.len(), 2);
                assert!(vertices.iter().all(|v| v.bulge == 0.0));
            }
            _ => assert_eq!(out, vec![DxfShape::Line(line.start(), line.end())]),
        }
        assert_readback(&drawing(&out), &[line]);
        for sweep in [-FRAC_PI_2, FRAC_PI_2] {
            let arc = Seg2::Arc { center: Vec2::new(3.0, -5.0), radius: 7.0, start: 0.3, sweep };
            let out = path_shapes(DxfPath::Open(&[arc]), style).unwrap();
            match style {
                DxfPathStyle::Polyline => {
                    let vertices = one_polyline(&out, false);
                    assert_eq!(vertices.len(), 2);
                    number_near(vertices[0].bulge, sweep.signum() * (2.0_f64.sqrt() - 1.0), BULGE_EPS);
                    assert_eq!(vertices[1].bulge, 0.0);
                }
                _ => assert!(matches!(out.as_slice(), [DxfShape::Arc { .. }])),
            }
            assert_readback(&drawing(&out), &[arc]);
        }
    }
}

#[test]
fn conic_styles_keep_exact_ellipses_and_rational_splines_unless_fitting_is_requested() {
    let (a, apex, b) = (Vec2::new(12.0, 0.0), Vec2::new(12.0, 5.0), Vec2::new(0.0, 5.0));
    for w in [FRAC_1_SQRT_2, 1.0, 1.75] {
        let conic = Seg2::Conic { a, apex, b, w };
        for style in styles() {
            let out = path_shapes(DxfPath::Open(&[conic]), style).unwrap();
            if matches!(style, DxfPathStyle::Arcs { .. }) {
                assert_arc_style(&out, &conic);
            } else if w < 1.0 {
                assert!(matches!(out.as_slice(), [DxfShape::Ellipse { .. }]));
            } else {
                let [DxfShape::Spline { degree, ctrl, weights, .. }] = out.as_slice() else { panic!("{out:?}") };
                assert_eq!(*degree, 2);
                assert_eq!(ctrl, &[a, apex, b]);
                let expected_weights = (w != 1.0).then(|| vec![1.0, w, 1.0]);
                assert_eq!(weights, &expected_weights);
            }
        }
    }
}

#[test]
fn degrees_one_through_nine_use_their_actual_public_curve_representation() {
    for degree in 1..=9 {
        let points: Vec<_> = (0..=degree).map(|i| Vec2::new(2.0 * i as f64, 0.08 * (i * (degree - i)) as f64)).collect();
        let segment = bezier(&points).unwrap();
        for style in styles() {
            let out = path_shapes(DxfPath::Open(&[segment]), style).unwrap();
            if matches!(style, DxfPathStyle::Arcs { .. }) {
                assert_arc_style(&out, &segment);
            } else if degree == 1 {
                if matches!(style, DxfPathStyle::Polyline) {
                    assert_eq!(one_polyline(&out, false).len(), 2);
                } else {
                    assert_eq!(out, vec![DxfShape::Line(points[0], points[1])]);
                }
            } else {
                let [DxfShape::Spline { degree: actual, ctrl, knots, .. }] = out.as_slice() else { panic!("{out:?}") };
                assert_eq!(usize::from(*actual), degree);
                assert_eq!(ctrl, &points);
                assert_eq!(knots.len(), 2 * (degree + 1));
                assert!(knots[..=degree].iter().all(|k| *k == 0.0));
                assert!(knots[degree + 1..].iter().all(|k| *k == 1.0));
            }
        }
    }
}

#[test]
fn whole_circle_and_ellipse_dispatch_through_the_same_public_path_entry() {
    let (center, u, v) = (Vec2::new(4.0, -2.0), Vec2::new(12.0, 0.0), Vec2::new(0.0, 5.0));
    let quarter = |u, v| Seg2::Conic { a: center + u, apex: center + u + v, b: center + v, w: FRAC_1_SQRT_2 };
    let ellipse = Loop2 { segs: vec![quarter(u, v), quarter(v, -u), quarter(-u, -v), quarter(-v, u)] };
    for style in styles() {
        let circle = Loop2::circle(center, 6.0);
        let out = path_shapes(DxfPath::Closed(&circle), style).unwrap();
        assert_eq!(out, vec![DxfShape::Circle { c: center, r: 6.0 }]);
        for sweep in [-TAU, TAU] {
            let arc = Seg2::Arc { center, radius: 6.0, start: 0.4, sweep };
            assert_eq!(path_shapes(DxfPath::Open(&[arc]), style).unwrap(), out);
        }
        let out = path_shapes(DxfPath::Closed(&ellipse), style).unwrap();
        if matches!(style, DxfPathStyle::Arcs { .. }) {
            assert!(!out.is_empty());
            assert!(out.iter().all(|s| matches!(s, DxfShape::Line(..) | DxfShape::Arc { .. } | DxfShape::Circle { .. })));
        } else {
            assert!(matches!(out.as_slice(), [DxfShape::Ellipse { .. }]));
        }
    }
}

#[test]
fn half_turn_boundary_splits_only_larger_arcs_and_keeps_signed_bulges() {
    let (center, radius, start) = (Vec2::new(12.0, -7.0), 9.0, 0.37);
    for magnitude in [PI - 1e-6, PI, PI + 1e-6, 1.5 * PI] {
        for sign in [-1.0, 1.0] {
            let sweep = sign * magnitude;
            let arc = Seg2::Arc { center, radius, start, sweep };
            let out = path_shapes(DxfPath::Open(&[arc]), DxfPathStyle::Polyline).unwrap();
            let vertices = one_polyline(&out, false);
            let count = if magnitude > PI { 2 } else { 1 };
            assert_eq!(vertices.len(), count + 1);
            let expected: Vec<_> = (0..=count)
                .map(|i| {
                    let angle = start + sweep * i as f64 / count as f64;
                    DxfVertex {
                        at: center + Vec2::new(angle.cos(), angle.sin()) * radius,
                        bulge: if i == count { 0.0 } else { (sweep / (4.0 * count as f64)).tan() },
                    }
                })
                .collect();
            for (actual, expected) in vertices.iter().zip(&expected) {
                point_near(actual.at, expected.at);
                number_near(actual.bulge, expected.bulge, BULGE_EPS);
            }
            let text = drawing(&out);
            let raw = assert_serialized_vertices(&text, &expected, false);
            for edge in raw.windows(2) {
                assert_arc_support(&edge[0], &edge[1], center, radius);
            }
            let pieces: Vec<_> = (0..count)
                .map(|i| Seg2::Arc { center, radius, start: start + sweep * i as f64 / count as f64, sweep: sweep / count as f64 })
                .collect();
            assert_readback(&text, &pieces);
        }
    }
}

#[test]
fn open_runs_preserve_zero_bulges_between_arcs_and_have_no_closing_edge() {
    let first = Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 4.0, start: FRAC_PI_2, sweep: -FRAC_PI_2 };
    let second = Seg2::Arc { center: Vec2::new(9.0, 0.0), radius: 3.0, start: PI, sweep: FRAC_PI_2 };
    let path = [first, Seg2::Line { a: first.end(), b: second.start() }, second];
    let q = 2.0_f64.sqrt() - 1.0;
    for reverse in [false, true] {
        let segments = if reverse { path.iter().rev().map(Seg2::reversed).collect::<Vec<_>>() } else { path.to_vec() };
        let out = path_shapes(DxfPath::Open(&segments), DxfPathStyle::Polyline).unwrap();
        let vertices = one_polyline(&out, false);
        assert_eq!(vertices.len(), 4);
        let bulges = [-q, 0.0, q, 0.0];
        let expected: Vec<_> =
            (0..4).map(|i| DxfVertex { at: if i < 3 { segments[i].start() } else { segments[2].end() }, bulge: bulges[i] }).collect();
        let text = drawing(&out);
        let raw = assert_serialized_vertices(&text, &expected, false);
        for (i, segment) in segments.iter().enumerate() {
            if let Seg2::Arc { center, radius, .. } = segment {
                assert_arc_support(&raw[i], &raw[i + 1], *center, *radius);
            }
        }
        assert_readback(&text, &segments);
    }
}

#[test]
fn a_closed_last_arc_owns_the_final_bulge_and_reversal_moves_it_to_its_new_edge() {
    let arc = Seg2::Arc { center: Vec2::ZERO, radius: 4.0, start: 0.0, sweep: FRAC_PI_2 };
    let (corner1, corner2) = (Vec2::new(-3.0, 4.0), Vec2::new(-3.0, 0.0));
    let loop_ = Loop2 {
        segs: vec![Seg2::Line { a: arc.end(), b: corner1 }, Seg2::Line { a: corner1, b: corner2 }, Seg2::Line { a: corner2, b: arc.start() }, arc],
    };
    for (path, bulges) in [(loop_.clone(), [0.0, 0.0, 0.0, 2.0_f64.sqrt() - 1.0]), (loop_.reversed(), [1.0 - 2.0_f64.sqrt(), 0.0, 0.0, 0.0])] {
        let out = path_shapes(DxfPath::Closed(&path), DxfPathStyle::Polyline).unwrap();
        let vertices = one_polyline(&out, true);
        assert_eq!(vertices.len(), 4, "closed paths must not duplicate the first vertex");
        let expected: Vec<_> = path.segs.iter().zip(bulges).map(|(s, bulge)| DxfVertex { at: s.start(), bulge }).collect();
        let text = drawing(&out);
        let raw = assert_serialized_vertices(&text, &expected, true);
        for (i, segment) in path.segs.iter().enumerate() {
            if let Seg2::Arc { center, radius, .. } = segment {
                assert_arc_support(&raw[i], &raw[(i + 1) % raw.len()], *center, *radius);
            }
        }
        assert_readback(&text, &path.segs);
    }
}

#[test]
fn exact_curves_interrupt_closed_polyline_runs_without_inventing_closing_lines() {
    let (a, b, c, d, e) = (Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(8.0, 4.0), Vec2::new(8.0, 9.0), Vec2::new(0.0, 12.0));
    let lines = [(a, b), (c, d), (e, a)];
    let path = Loop2 {
        segs: vec![
            Seg2::Line { a, b },
            Seg2::Cubic { p0: b, p1: Vec2::new(6.0, 0.0), p2: Vec2::new(8.0, 2.0), p3: c },
            Seg2::Line { a: c, b: d },
            Seg2::Conic { a: d, apex: Vec2::new(8.0, 12.0), b: e, w: FRAC_1_SQRT_2 },
            Seg2::Line { a: e, b: a },
        ],
    };
    let out = path_shapes(DxfPath::Closed(&path), DxfPathStyle::Polyline).unwrap();
    assert_eq!(out.iter().filter(|s| matches!(s, DxfShape::Spline { degree: 3, .. })).count(), 1);
    assert_eq!(out.iter().filter(|s| matches!(s, DxfShape::Ellipse { .. })).count(), 1);
    let text = drawing(&out);
    let entities = raw_entities(&text);
    let mut edges = Vec::new();
    for entity in entities.iter().filter(|e| e.0 == "LWPOLYLINE") {
        let (vertices, closed) = raw_polyline(entity);
        assert!(!closed, "a run interrupted by an exact curve is open");
        assert!(vertices.iter().all(|v| v.bulge == 0.0));
        edges.extend(vertices.windows(2).map(|v| (v[0].at, v[1].at)));
    }
    // Prefix/suffix runs may be joined across the original seam. Either representation must
    // contain exactly the three authored line edges and no chord across a free-form curve.
    assert_eq!(edges.len(), lines.len());
    for (a, b) in lines {
        assert_eq!(edges.iter().filter(|(ea, eb)| ea.dist(a) < POSITION_EPS && eb.dist(b) < POSITION_EPS).count(), 1);
    }
    let read = read_dxf(&text).unwrap();
    assert_eq!(read.iter().filter(|s| matches!(s, Geom2::Line(..))).count(), 3);
}

fn invalid(path: &[Seg2], style: DxfPathStyle) -> String {
    match path_shapes(DxfPath::Open(path), style) {
        Err(IoError::Invalid(message)) => message,
        other => panic!("expected contextual invalid-path error, got {other:?}"),
    }
}

#[test]
fn options_are_validated_even_for_empty_and_line_only_paths() {
    let line = [Seg2::Line { a: Vec2::ZERO, b: Vec2::X }];
    for tolerance_mm in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, 0.5e-9] {
        let style = DxfPathStyle::Arcs { tolerance_mm };
        invalid(&[], style);
        invalid(&line, style);
    }
    for style in styles().into_iter().chain([DxfPathStyle::Arcs { tolerance_mm: 1e-9 }]) {
        assert!(path_shapes(DxfPath::Open(&[]), style).unwrap().is_empty());
        assert!(!path_shapes(DxfPath::Open(&line), style).unwrap().is_empty());
    }
}

#[test]
fn invalid_bezier_storage_is_refused_before_helpers_can_clamp_the_degree() {
    let mut p = [Vec2::ZERO; MAX_BEZIER_CTRL];
    for (i, point) in p.iter_mut().enumerate() {
        *point = Vec2::new(i as f64, (i % 3) as f64);
    }
    for n in [0, 1, 2, 3, 10, u8::MAX] {
        for style in styles() {
            invalid(&[Seg2::Bezier { n, p }], style);
        }
    }
}

#[test]
fn piece_errors_keep_original_input_indices_when_earlier_curves_can_expand() {
    let predecessors = [
        Seg2::Arc { center: Vec2::new(4.0, 2.0), radius: 3.0, start: 0.4, sweep: 1.5 * PI },
        Seg2::Cubic { p0: Vec2::ZERO, p1: Vec2::new(0.0, 8.0), p2: Vec2::new(7.0, -5.0), p3: Vec2::new(9.0, 1.0) },
    ];
    for predecessor in predecessors {
        let mut p = [Vec2::ZERO; MAX_BEZIER_CTRL];
        p[0] = predecessor.end();
        let path = [Seg2::Line { a: predecessor.start() - Vec2::X, b: predecessor.start() }, predecessor, Seg2::Bezier { n: 10, p }];
        for style in styles() {
            let message = invalid(&path, style);
            assert!(message.contains("piece 3 of 3"), "{message}");
            assert!(message.contains("degree 10"), "{message}");
        }
    }
}

#[test]
fn closed_intent_with_a_gap_preserves_pieces_as_open_runs_without_bridging() {
    // Closed is caller intent, not a topology certificate. The explicit gap from (1,0)
    // to (2,0) must remain absent from the output, even though the last point is the first.
    let path = Loop2 { segs: vec![Seg2::Line { a: Vec2::ZERO, b: Vec2::X }, Seg2::Line { a: Vec2::new(2.0, 0.0), b: Vec2::ZERO }] };
    for style in styles() {
        let out = path_shapes(DxfPath::Closed(&path), style).unwrap();
        let text = drawing(&out);
        if matches!(style, DxfPathStyle::Polyline) {
            assert_eq!(out.len(), 2);
            for (shape, segment) in out.iter().zip(&path.segs) {
                let vertices = one_polyline(std::slice::from_ref(shape), false);
                assert_eq!(vertices.len(), 2);
                point_near(vertices[0].at, segment.start());
                point_near(vertices[1].at, segment.end());
                assert!(vertices.iter().all(|v| v.bulge == 0.0));
            }
            for entity in raw_entities(&text) {
                assert!(!raw_polyline(&entity).1);
            }
        } else {
            assert_eq!(out, vec![DxfShape::Line(Vec2::ZERO, Vec2::X), DxfShape::Line(Vec2::new(2.0, 0.0), Vec2::ZERO)]);
        }
        assert_readback(&text, &path.segs);
    }
}

#[test]
fn nonfinite_controls_and_invalid_arcs_are_rejected_in_every_style() {
    let controls = [
        Seg2::Line { a: Vec2::ZERO, b: Vec2::new(f64::NAN, 2.0) },
        Seg2::Cubic { p0: Vec2::ZERO, p1: Vec2::new(1.0, f64::INFINITY), p2: Vec2::new(2.0, 1.0), p3: Vec2::new(3.0, 0.0) },
        Seg2::Conic { a: Vec2::ZERO, apex: Vec2::new(1.0, 2.0), b: Vec2::new(2.0, 0.0), w: f64::NAN },
        // Finite inputs may still overflow the derived arc endpoint or bounds.
        Seg2::Arc { center: Vec2::new(f64::MAX, 0.0), radius: f64::MAX, start: 0.0, sweep: FRAC_PI_2 },
    ];
    for style in styles() {
        for segment in controls {
            invalid(&[segment], style);
        }
        for radius in [0.0, -1.0, f64::INFINITY, f64::NAN] {
            invalid(&[Seg2::Arc { center: Vec2::ZERO, radius, start: 0.0, sweep: PI }], style);
        }
        // A one-ULP overrun is still more than one turn; the old near-TAU shortcut must not
        // silently turn it into a circle. Larger positive/negative overruns are also refused.
        for sweep in [0.0, f64::NAN, f64::INFINITY, f64::from_bits(TAU.to_bits() + 1), -f64::from_bits(TAU.to_bits() + 1), 1.1 * TAU, -1.1 * TAU] {
            invalid(&[Seg2::Arc { center: Vec2::ZERO, radius: 3.0, start: 0.0, sweep }], style);
        }
    }
}

#[test]
fn fitting_cannot_report_success_after_discarding_a_nonempty_piece() {
    // The existing fitter may return Some(empty) for a point-sized cubic. The public path
    // converter must refuse that result instead of silently dropping an authored piece.
    let at = Vec2::new(2.0, 3.0);
    let point_curve = Seg2::Cubic { p0: at, p1: at, p2: at, p3: at };
    invalid(&[point_curve], DxfPathStyle::Arcs { tolerance_mm: 0.01 });
}

#[test]
fn bulged_arc_extents_include_cardinal_points_between_its_vertices() {
    let arc = Seg2::Arc { center: Vec2::new(12.0, -7.0), radius: 9.0, start: 0.2, sweep: 1.5 * PI };
    let out = path_shapes(DxfPath::Open(&[arc]), DxfPathStyle::Polyline).unwrap();
    let text = drawing(&out);
    let lines: Vec<_> = text.lines().collect();
    let pairs: Vec<_> = lines.chunks_exact(2).map(|p| (p[0].trim().parse::<i32>().unwrap(), p[1].trim())).collect();
    let bound = |name: &str| {
        let start = pairs.iter().position(|p| *p == (9, name)).unwrap();
        let fields: Vec<_> = pairs[start + 1..].iter().take_while(|p| p.0 != 9 && p.0 != 0).copied().collect();
        let entity = (name, fields);
        Vec2::new(scalar(&entity, 10), scalar(&entity, 20))
    };
    // The arc crosses pi/2, pi and 3pi/2. None of the two-piece polyline's interior
    // vertices supplies all three extremes; the rightmost point is its original start.
    point_near(bound("$EXTMIN"), Vec2::new(3.0, -16.0));
    point_near(bound("$EXTMAX"), Vec2::new(12.0 + 9.0 * 0.2_f64.cos(), 2.0));
}

// Resource-boundary fixtures retain small exact-integer coordinates and do not serialize
// the large paths. Repeated nonzero pieces are intentional: conversion must preserve them,
// not deduplicate geometry to make a path fit a resource budget.
const PATH_ITEM_LIMIT: usize = 200_000;

fn alternating_lines(count: usize) -> Vec<Seg2> {
    (0..count).map(|i| if i & 1 == 0 { Seg2::Line { a: Vec2::ZERO, b: Vec2::X } } else { Seg2::Line { a: Vec2::X, b: Vec2::ZERO } }).collect()
}

fn assert_line_entities(shapes: &[DxfShape], segments: &[Seg2]) {
    assert_eq!(shapes.len(), segments.len());
    for (actual, expected) in shapes.iter().zip(segments) {
        let Seg2::Line { a, b } = expected else { panic!("line fixture expected") };
        assert_eq!(actual, &DxfShape::Line(*a, *b));
    }
}

#[test]
fn input_and_exact_entity_budgets_accept_the_boundary_without_truncation() {
    let mut path = alternating_lines(PATH_ITEM_LIMIT);
    {
        let shapes = path_shapes(DxfPath::Open(&path), DxfPathStyle::Exact).unwrap();
        // Exact conversion emits one entity per piece. This reaches the public entity
        // boundary as well as the independent input boundary; all geometry must remain.
        assert_eq!(shapes.len(), PATH_ITEM_LIMIT);
        assert_line_entities(&shapes, &path);
    }
    path.push(Seg2::Line { a: Vec2::ZERO, b: Vec2::X });
    for style in styles() {
        let message = invalid(&path, style);
        assert!(message.contains("input piece budget"), "{message}");
    }
}

#[test]
fn open_and_closed_vertex_budgets_count_the_terminal_vertex_differently() {
    let path = Loop2 { segs: alternating_lines(PATH_ITEM_LIMIT) };
    {
        let shapes = path_shapes(DxfPath::Open(&path.segs[..PATH_ITEM_LIMIT - 1]), DxfPathStyle::Polyline).unwrap();
        let vertices = one_polyline(&shapes, false);
        assert_eq!(vertices.len(), PATH_ITEM_LIMIT);
        for (i, vertex) in vertices.iter().enumerate() {
            assert_eq!(vertex.at, if i & 1 == 0 { Vec2::ZERO } else { Vec2::X });
            assert_eq!(vertex.bulge, 0.0);
        }
    }
    {
        // The same even-length chain ends at its start. Closed is only caller intent;
        // these authored overlapping edges still all count, without a duplicate terminal.
        let shapes = path_shapes(DxfPath::Closed(&path), DxfPathStyle::Polyline).unwrap();
        let vertices = one_polyline(&shapes, true);
        assert_eq!(vertices.len(), PATH_ITEM_LIMIT);
        for (i, vertex) in vertices.iter().enumerate() {
            assert_eq!(vertex.at, if i & 1 == 0 { Vec2::ZERO } else { Vec2::X });
            assert_eq!(vertex.bulge, 0.0);
        }
    }
    // Still within the input limit, but its open representation needs 200,001 vertices.
    let message = invalid(&path.segs, DxfPathStyle::Polyline);
    assert!(message.contains("polyline vertex budget"), "{message}");
}

#[test]
fn vertex_budget_is_accumulated_across_disconnected_runs() {
    let line = Seg2::Line { a: Vec2::ZERO, b: Vec2::X };
    let mut path = vec![line; PATH_ITEM_LIMIT / 2];
    {
        let shapes = path_shapes(DxfPath::Open(&path), DxfPathStyle::Polyline).unwrap();
        assert_eq!(shapes.len(), PATH_ITEM_LIMIT / 2);
        for shape in &shapes {
            let vertices = one_polyline(std::slice::from_ref(shape), false);
            assert_eq!(vertices.len(), 2);
            assert_eq!(vertices[0].at, Vec2::ZERO);
            assert_eq!(vertices[1].at, Vec2::X);
            assert!(vertices.iter().all(|v| v.bulge == 0.0));
        }
    }
    path.push(line);
    // Only 100,001 inputs/entities; allowing each tiny run its own budget would miss this.
    let message = invalid(&path, DxfPathStyle::Polyline);
    assert!(message.contains("polyline vertex budget"), "{message}");
}

fn assert_budget_run_geometry(shapes: &[DxfShape], segments: &[Seg2]) -> usize {
    assert_eq!(shapes.len(), segments.len(), "every fixture piece is a separate run or exact arc");
    let mut vertex_count = 0;
    for (shape, segment) in shapes.iter().zip(segments) {
        match (shape, segment) {
            (DxfShape::Arc { c, r, a0, a1 }, Seg2::Arc { center, radius, .. }) => {
                // The fixture's 270-degree arcs may legally remain exact ARC entities.
                assert_eq!(c, center);
                assert_eq!(r, radius);
                assert_eq!((*a0, *a1), (0.0, 270.0));
            }
            (DxfShape::BulgedPolyline { vertices, closed }, Seg2::Arc { center, radius, .. }) => {
                assert!(!closed);
                assert_eq!(vertices.len(), 3);
                point_near(vertices[0].at, segment.start());
                point_near(vertices[2].at, segment.end());
                assert_eq!(vertices[2].bulge, 0.0);
                let mut sweep = 0.0;
                for edge in vertices.windows(2) {
                    assert!(edge[0].bulge > 0.0 && edge[0].bulge <= 1.0);
                    assert_arc_support(&edge[0], &edge[1], *center, *radius);
                    sweep += 4.0 * edge[0].bulge.atan();
                }
                number_near(sweep, 1.5 * PI, BULGE_EPS);
                vertex_count += vertices.len();
            }
            (DxfShape::BulgedPolyline { vertices, closed }, Seg2::Line { a, b }) => {
                assert!(!closed);
                assert_eq!(vertices.len(), 2);
                assert_eq!(vertices[0].at, *a);
                assert_eq!(vertices[1].at, *b);
                assert!(vertices.iter().all(|v| v.bulge == 0.0));
                vertex_count += vertices.len();
            }
            _ => panic!("fixture geometry lost: {segment:?} became {shape:?}"),
        }
    }
    vertex_count
}

#[test]
fn optional_arc_packing_preserves_support_and_never_exceeds_the_vertex_budget() {
    let arc = Seg2::Arc { center: Vec2::ZERO, radius: 4.0, start: 0.0, sweep: 1.5 * PI };
    // A packed disconnected 270-degree arc has two curved edges and three vertices.
    // 66,666 such runs plus an isolated line need at most 200,000 vertices, including
    // when every arc packs. Exact ARC fallback uses no polyline vertices and is permitted.
    let arc_count = PATH_ITEM_LIMIT / 3;
    let mut path = vec![arc; arc_count];
    path.push(Seg2::Line { a: Vec2::X, b: Vec2::new(2.0, 0.0) });
    {
        let shapes = path_shapes(DxfPath::Open(&path), DxfPathStyle::Polyline).unwrap();
        assert!(assert_budget_run_geometry(&shapes, &path) <= PATH_ITEM_LIMIT);
    }
    path.push(arc);
    assert!(path.len() < PATH_ITEM_LIMIT);
    // If the expanded representation exceeds the budget, refuse it. If optional exact
    // fallbacks keep it within the budget, success must preserve every original support.
    // This never calibrates expectations to which arcs the current packer happens to admit.
    match path_shapes(DxfPath::Open(&path), DxfPathStyle::Polyline) {
        Ok(shapes) => assert!(assert_budget_run_geometry(&shapes, &path) <= PATH_ITEM_LIMIT),
        Err(IoError::Invalid(message)) => assert!(message.contains("polyline vertex budget"), "{message}"),
        other => panic!("unexpected path failure: {other:?}"),
    }
}

#[test]
fn only_mixed_paths_apply_the_aggregate_fitting_budget_to_retained_primitives() {
    const FITTING_LIMIT: usize = 16_384;
    const PER_CURVE_LIMIT: usize = 2_048;
    let style = DxfPathStyle::Arcs { tolerance_mm: 0.01 };
    let arc = Seg2::Arc { center: Vec2::new(2.0, 0.0), radius: 1.0, start: 0.0, sweep: FRAC_PI_2 };
    {
        let mut primitives = alternating_lines(FITTING_LIMIT);
        primitives.push(arc);
        let shapes = path_shapes(DxfPath::Open(&primitives), style).unwrap();
        assert_eq!(shapes.len(), FITTING_LIMIT + 1);
        assert_line_entities(&shapes[..FITTING_LIMIT], &primitives[..FITTING_LIMIT]);
        assert_arc_style(&shapes[FITTING_LIMIT..], &arc);
    }

    // A straight cubic has an exact, inexpensive fit. No assertion depends on how many
    // pieces the fitter chooses: any admitted fit has 1..=2,048 output pieces.
    let curve = Seg2::Cubic { p0: Vec2::ZERO, p1: Vec2::X, p2: Vec2::new(2.0, 0.0), p3: Vec2::new(3.0, 0.0) };
    let retained = FITTING_LIMIT - PER_CURVE_LIMIT;
    let mut mixed = alternating_lines(retained);
    mixed.push(curve);
    {
        let shapes = path_shapes(DxfPath::Open(&mixed), style).unwrap();
        assert!((retained + 1..=FITTING_LIMIT).contains(&shapes.len()));
        assert_line_entities(&shapes[..retained], &mixed[..retained]);
        assert_arc_style(&shapes[retained..], &curve);
    }
    assert_eq!(mixed.pop(), Some(curve));
    mixed.extend(alternating_lines(PER_CURVE_LIMIT));
    mixed.push(curve);
    // 16,384 retained primitives already exhaust the mixed-path budget. Every nonempty
    // fitted result exceeds it, regardless of the fitter's particular subdivision count.
    assert_eq!(mixed.len(), FITTING_LIMIT + 1);
    let message = invalid(&mixed, style);
    assert!(message.contains("path arc fitting budget"), "{message}");
}

#[test]
fn large_translation_cannot_turn_a_three_quarter_unit_radius_into_a_unit_radius() {
    let coordinate = (1_u64 << 52) as f64;
    let center = Vec2::new(coordinate, coordinate);
    let radius = 0.75;
    let arc = Seg2::Arc { center, radius, start: 0.0, sweep: FRAC_PI_2 };
    let shapes = path_shapes(DxfPath::Open(&[arc]), DxfPathStyle::Polyline).unwrap();
    let text = drawing(&shapes);
    let entities = raw_entities(&text);
    assert_eq!(entities.len(), 1);
    match entities[0].0 {
        "ARC" => {
            // ARC preserves the center, radius and angle directly, even when its derived
            // f64 endpoints round. This exact fallback is allowed when packing loses support.
            assert_eq!(scalar(&entities[0], 10), center.x);
            assert_eq!(scalar(&entities[0], 20), center.y);
            assert_eq!(scalar(&entities[0], 40), radius);
            assert_eq!(scalar(&entities[0], 50), 0.0);
            assert_eq!(scalar(&entities[0], 51), 90.0);
        }
        "LWPOLYLINE" => {
            let (vertices, closed) = raw_polyline(&entities[0]);
            assert!(!closed && vertices.len() >= 2);
            assert_eq!(vertices[0].at, arc.start());
            assert_eq!(vertices.last().unwrap().at, arc.end());
            assert_eq!(vertices.last().unwrap().bulge, 0.0);
            let mut sweep = 0.0;
            for edge in vertices.windows(2) {
                // Translate BEFORE reconstructing. At 2^52, adding a center correction in
                // world coordinates can erase it and make a wrong circle appear valid.
                let (a, b, bulge) = (edge[0].at - center, edge[1].at - center, edge[0].bulge);
                assert!(bulge > 0.0 && bulge <= 1.0);
                let chord = b - a;
                let square = bulge * bulge;
                let recovered_center = (a + b) * 0.5 + chord.perp() * ((1.0 - square) / (4.0 * bulge));
                let recovered_radius = chord.len() * (1.0 + square) / (4.0 * bulge);
                // Small local arithmetic gets an epsilon-scale allowance, never an ULP
                // allowance based on the huge translation (which could hide 0.25 mm loss).
                let support_epsilon = 64.0 * f64::EPSILON * radius;
                assert!(recovered_center.len() <= support_epsilon, "local center {recovered_center:?}");
                assert!((recovered_radius - radius).abs() <= support_epsilon, "radius {recovered_radius}, expected {radius}");
                sweep += 4.0 * bulge.atan();
            }
            number_near(sweep, FRAC_PI_2, 64.0 * f64::EPSILON);
        }
        kind => panic!("source arc support was lost or replaced: {kind}"),
    }
}

fn assert_serialized_arc_span(shapes: &[DxfShape], arc: &Seg2, span_error_degrees: f64) {
    let Seg2::Arc { center, radius, sweep, .. } = arc else { panic!("arc fixture expected") };
    assert!(matches!(shapes, [DxfShape::Arc { .. }]), "a partial arc must not become a circle or disappear: {shapes:?}");
    let text = drawing(shapes);
    let entities = raw_entities(&text);
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].0, "ARC");
    assert_eq!(scalar(&entities[0], 10), center.x);
    assert_eq!(scalar(&entities[0], 20), center.y);
    assert_eq!(scalar(&entities[0], 40), *radius);
    let (start, end) = (scalar(&entities[0], 50), scalar(&entities[0], 51));
    assert!(start.is_finite() && end.is_finite());
    // 360 and 0 are the same direction. Comparing just their stored numbers would
    // accept a degenerate ARC, even if they were distinct before serialization.
    let serialized_sweep = (end - start).rem_euclid(360.0);
    assert!(serialized_sweep > 0.0 && serialized_sweep < 360.0, "collapsed serialized ARC: {start} -> {end}");
    number_near(serialized_sweep, sweep.abs().to_degrees(), span_error_degrees);
}

fn preserved_arc_or_contextual_refusal(arc: Seg2, style: DxfPathStyle) {
    let Seg2::Arc { sweep, .. } = arc else { panic!("arc fixture expected") };
    match path_shapes(DxfPath::Open(&[arc]), style) {
        // A future precise writer may preserve these spans. Allow the rounding of unit
        // conversion/subtraction, not an absolute tolerance that can swallow a tiny arc.
        Ok(shapes) => assert_serialized_arc_span(&shapes, &arc, 2.0 * f64::EPSILON * sweep.abs().to_degrees()),
        Err(IoError::Invalid(message)) => {
            assert!(message.contains("piece 1 of 1"), "{message}");
            assert!(message.contains("arc"), "{message}");
        }
        other => panic!("unexpected arc failure: {other:?}"),
    }
}

#[test]
fn serialized_zero_crossing_arcs_keep_their_span_or_refuse_with_piece_context() {
    for style in [DxfPathStyle::Exact, DxfPathStyle::Arcs { tolerance_mm: 0.01 }] {
        for (start, sweep) in [(-1e-16, 1e-16), (0.0, -1e-16)] {
            preserved_arc_or_contextual_refusal(Seg2::Arc { center: Vec2::ZERO, radius: 1.0, start, sweep }, style);
        }
        // A small but ordinary arc remains supported. Two independently rounded degree
        // fields contribute at most 1e-12 degrees; subtraction adds at most a few ULPs.
        let writer_error = 1e-12 + 4.0 * f64::EPSILON * 360.0;
        for sweep in [-1e-6, 1e-6] {
            let arc = Seg2::Arc { center: Vec2::ZERO, radius: 1.0, start: 0.0, sweep };
            let shapes = path_shapes(DxfPath::Open(&[arc]), style).unwrap();
            assert_serialized_arc_span(&shapes, &arc, writer_error);
        }
    }
}

#[test]
fn an_arc_one_ulp_short_of_a_turn_cannot_serialize_as_a_circle_or_zero_span() {
    let almost_turn = f64::from_bits(TAU.to_bits() - 1);
    assert!(almost_turn < TAU);
    for style in [DxfPathStyle::Exact, DxfPathStyle::Arcs { tolerance_mm: 0.01 }] {
        for sweep in [almost_turn, -almost_turn] {
            preserved_arc_or_contextual_refusal(Seg2::Arc { center: Vec2::ZERO, radius: 1.0, start: 0.0, sweep }, style);
        }
    }
}

#[test]
fn cyclic_ellipse_starts_preserve_the_legacy_first_quarter_parameterization() {
    let vertices = [Vec2::new(15.0, 0.0), Vec2::new(0.0, 8.0), Vec2::new(-15.0, 0.0), Vec2::new(0.0, -8.0)];
    // The original full-loop writer keeps the first conic's frame instead of canonicalizing
    // its major-axis sign. For these four quarters the independent legacy signs are +--+.
    // These constants come from the analytic ellipse and legacy frame/decimal conventions;
    // no converter or recognizer is used to calculate expected values.
    let major_x = [15.0, -15.0, -15.0, 15.0];
    let serialized_major_x = ["15.0", "-15.0", "-15.0", "15.0"];
    for (first, (expected_major_x, expected_serialized)) in major_x.into_iter().zip(serialized_major_x).enumerate() {
        let path = Loop2 {
            segs: (0..4)
                .map(|offset| {
                    let (a, b) = (vertices[(first + offset) % 4], vertices[(first + offset + 1) % 4]);
                    Seg2::Conic { a, apex: a + b, b, w: FRAC_1_SQRT_2 }
                })
                .collect(),
        };
        let shapes = path_shapes(DxfPath::Closed(&path), DxfPathStyle::Exact).unwrap();
        let [DxfShape::Ellipse { c, major, ratio, t0, t1 }] = shapes.as_slice() else { panic!("{shapes:?}") };
        point_near(*c, Vec2::ZERO);
        point_near(*major, Vec2::new(expected_major_x, 0.0));
        number_near(*ratio, 8.0 / 15.0, 1e-12);
        assert_eq!((*t0, *t1), (0.0, TAU));
        let text = drawing(&shapes);
        let entities = raw_entities(&text);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].0, "ELLIPSE");
        let group = |code| entities[0].1.iter().filter(|(c, _)| *c == code).map(|(_, value)| *value).collect::<Vec<_>>();
        assert_eq!(group(11), vec![expected_serialized]);
        assert_eq!(group(21), vec!["0.0"]);
        assert_eq!(group(40), vec!["0.533333333333"]);
    }
}

#[test]
fn a_single_zero_length_line_in_a_closed_path_remains_a_writable_open_polyline() {
    let at = Vec2::new(2.0, -3.0);
    let path = Loop2 { segs: vec![Seg2::Line { a: at, b: at }] };
    let exact = path_shapes(DxfPath::Closed(&path), DxfPathStyle::Exact).unwrap();
    assert_eq!(exact, vec![DxfShape::Line(at, at)]);
    let exact_text = drawing(&exact);
    let exact_entities = raw_entities(&exact_text);
    assert_eq!(exact_entities.len(), 1);
    assert_eq!(exact_entities[0].0, "LINE");
    for (code, expected) in [(10, at.x), (20, at.y), (11, at.x), (21, at.y)] {
        assert_eq!(scalar(&exact_entities[0], code), expected);
    }

    let shapes = path_shapes(DxfPath::Closed(&path), DxfPathStyle::Polyline).unwrap();
    let expected = [DxfVertex { at, bulge: 0.0 }; 2];
    assert_eq!(one_polyline(&shapes, false), expected.as_slice());
    // Inspect the emitted entity: the current reader intentionally skips zero-length
    // polyline edges, so it cannot establish that this authored piece was written.
    let text = drawing(&shapes);
    let entities = raw_entities(&text);
    assert_eq!(entities.len(), 1);
    let (vertices, closed) = raw_polyline(&entities[0]);
    assert_eq!(vertices.as_slice(), expected.as_slice());
    assert!(!closed);
    assert_eq!(scalar(&entities[0], 70), 128.0);
}
