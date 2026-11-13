use super::*;
use solidcore_record::Vec2;
use solidcraft_geom::drawing_view::{NamedView, Scale, ViewFrame, basis_looking};

const EPS: f64 = 1e-12;

/// The front view: paper x along +X, paper y along +Z, its normal `X × Z = -Y` toward the viewer.
fn front() -> Plane {
    NamedView::Front.basis()
}

/// x along (0.6, 0.8, 0) and y along +z: exactly unit and square, so the closed forms are exact.
fn tilted() -> Plane {
    Plane { origin: Vec3::new(1.0, 2.0, 3.0), x: Vec3::new(0.6, 0.8, 0.0), y: Vec3::Z }
}

/// A frame turned off every axis, its axes exact thirds.
fn turned() -> Plane {
    Plane { origin: Vec3::new(5.0, -7.0, 2.0), x: Vec3::new(2.0 / 3.0, 2.0 / 3.0, 1.0 / 3.0), y: Vec3::new(2.0 / 3.0, -1.0 / 3.0, -2.0 / 3.0) }
}

/// Whether `r` is refused with words that contain `size`.
fn says<T: std::fmt::Debug>(r: Result<T>, size: &str) -> bool {
    r.is_err_and(|e| e.to_string().contains(size))
}

/// Whether any component of `v` is -0.
fn negative_zero(v: Vec3) -> bool {
    [v.x, v.y, v.z].iter().any(|c| *c == 0.0 && c.is_sign_negative())
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).len() < EPS
}

/// SolidCraft's isometric view from the world's upper corner `(cx, cy, 1)` as geom's named views
/// compute it (`iso` in `crates/geom/src/drawing_view.rs`), restated so the comparison is to the bit.
fn iso_restated(cx: f64, cy: f64) -> Plane {
    let toward = Vec3::new(-cx, -cy, -1.0) * (1.0 / 3f64.sqrt());
    let x = Vec3::new(-cy, cx, 0.0) * (1.0 / 2f64.sqrt());
    Plane { origin: Vec3::ZERO, x, y: x.cross(toward) }
}

/// The front view's four upper corners give the four named isometric views as the same doubles
/// (`==`, so a zero's sign aside), through the front view's origin; a corner's size changes nothing,
/// up to 1e300. Geom's named views give the same doubles, so a view named `iso_front_right` and the
/// pictorial view above right of the front view are one basis.
#[test]
fn axonometric_from_the_front_views_corners_is_the_named_isometric_views() {
    let o = Plane { origin: Vec3::new(1.0, -2.0, 3.0), ..front() };
    // (the corner along the front view's x, y and normal; the world corner (cx, cy, 1) it is; the named view)
    for (k, (cx, cy), named) in [
        ([1.0, 1.0, 1.0], (1.0, -1.0), NamedView::IsoFrontRight),
        ([-1.0, 1.0, 1.0], (-1.0, -1.0), NamedView::IsoFrontLeft),
        ([1.0, 1.0, -1.0], (1.0, 1.0), NamedView::IsoBackRight),
        ([-1.0, 1.0, -1.0], (-1.0, 1.0), NamedView::IsoBackLeft),
    ] {
        let want = iso_restated(cx, cy);
        assert_eq!(named.basis(), want, "{named:?}");
        for size in [1.0, 2.0, 1e300] {
            let got = axonometric(&o, k.map(|c| c * size)).unwrap();
            assert_eq!(got.origin, o.origin);
            assert_eq!((got.x, got.y), (want.x, want.y), "{k:?} x{size}");
        }
    }
}

/// A corner straight along a paper axis or the normal gives the principal view on that side, as the
/// same doubles: in front, the front view itself; along +x the right view, along -x the left view,
/// behind the back view.
#[test]
fn axonometric_from_a_side_is_the_principal_view_on_that_side() {
    let f = front();
    for (k, x, y) in [
        ([0.0, 0.0, 1.0], Vec3::X, Vec3::Z),
        ([1.0, 0.0, 0.0], Vec3::Y, Vec3::Z),
        ([-1.0, 0.0, 0.0], -Vec3::Y, Vec3::Z),
        ([0.0, 0.0, -1.0], -Vec3::X, Vec3::Z),
    ] {
        let got = axonometric(&f, k).unwrap();
        assert_eq!((got.x, got.y), (x, y), "{k:?}");
    }
}

/// How long each of the frame's axes shows on paper: in the isometric view all three at `sqrt(2/3)`
/// (a 10 mm cube's edges at 8.165 mm), in the standard dimetric view (ISO 5456-3, corner
/// `(1, 1, sqrt 7)`) the frame's x and y at `sqrt(8/9)` and its normal (the depth) at `sqrt(2/9)`,
/// half as long.
#[test]
fn axonometric_shows_the_axes_at_the_isometric_and_dimetric_ratios() {
    let f = front();
    let shown = |k: [f64; 3], e: Vec3| {
        let got = axonometric(&f, k).unwrap();
        e.dot(got.x).hypot(e.dot(got.y))
    };
    let depth = -Vec3::Y;
    for e in [Vec3::X, Vec3::Z, depth] {
        let iso = shown([1.0, 1.0, 1.0], e);
        assert!((iso - (2.0f64 / 3.0).sqrt()).abs() < 1e-15, "{e:?} {iso}");
        assert!((10.0 * iso - 8.164_965_809_277_26).abs() < 1e-12, "{e:?}");
    }
    let dimetric = [1.0, 1.0, 7f64.sqrt()];
    for e in [Vec3::X, Vec3::Z] {
        let s = shown(dimetric, e);
        assert!((s - (8.0f64 / 9.0).sqrt()).abs() < 1e-15, "{e:?} {s}");
    }
    let s = shown(dimetric, depth);
    assert!((s - (2.0f64 / 9.0).sqrt()).abs() < 1e-15, "{s}");
}

/// For 1000 seeded corners on three frames (the front view, a tilted one and one turned off every
/// axis), and the boundary shapes (a zero component, a -0, a corner below, sizes 1e300 and just past
/// the least that passes): the axes are unit and square to each other within 16 roundings, paper x
/// is square to the frame's y (horizontal), the view's normal `x × y` points at the corner, and the
/// origin is the frame's.
#[test]
fn axonometric_axes_are_orthonormal_and_look_from_the_corner() {
    // gamma_16 (Higham §3.1): 16 roundings.
    let tol = 16.0 * f64::EPSILON / 2.0 / (1.0 - 16.0 * f64::EPSILON / 2.0);
    let check = |f: &Plane, k: [f64; 3]| {
        let got = axonometric(f, k).unwrap_or_else(|e| panic!("{k:?}: {e}"));
        let m = k.iter().fold(0.0_f64, |m, c| m.max(c.abs()));
        let c = f.x * (k[0] / m) + f.y * (k[1] / m) + f.x.cross(f.y) * (k[2] / m);
        let c = c * (1.0 / c.len());
        assert!((got.x.len() - 1.0).abs() <= tol, "{k:?} {:?}", got.x);
        assert!((got.y.len() - 1.0).abs() <= tol, "{k:?} {:?}", got.y);
        assert!(got.x.dot(got.y).abs() <= tol, "{k:?}");
        assert!(got.x.dot(f.y).abs() <= tol, "{k:?}: paper x is horizontal");
        assert!((got.x.cross(got.y).dot(c) - 1.0).abs() <= tol, "{k:?}: looks from the corner");
        assert_eq!(got.origin, f.origin);
    };
    let mut seed = 0x0001_3044_u64;
    let mut next = move || {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (seed >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    };
    for f in &[front(), tilted(), turned()] {
        for _ in 0..1000 {
            check(f, [next(), next(), next()]);
        }
        for k in [
            [0.0, 1.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 0.0],
            [-0.0, 1.0, 1.0],
            [1.0, -0.0, -0.0],
            [1.0, -1.0, 1.0],
            [1e300, 1e300, -1e300],
            [1.000_000_1 * MIN_DIRECTION, 0.0, 0.0],
            [1.000_000_1 * MIN_DIRECTION, 1.0, 0.0],
        ] {
            check(f, k);
        }
    }
}

/// What gives no view is refused, each with its cause: a frame not finite or off orthonormal past
/// 1e-8; a corner not finite or of length at or below `MIN_DIRECTION` (a longer one fits); a corner
/// within `MIN_DIRECTION` radians of the frame's y, which has no horizontal (one further from y
/// fits). The thresholds decide exactly.
#[test]
fn axonometric_refuses_what_gives_no_view() {
    let f = front();
    let off = |dx: f64| Plane { x: Vec3::new(1.0 + dx, 0.0, 0.0), ..front() };
    assert!(axonometric(&off(0.5e-8), [1.0; 3]).is_ok());
    assert!(says(axonometric(&off(2e-8), [1.0; 3]), "not orthonormal within 1e-8"));
    for bad in [Plane { origin: Vec3::new(f64::NAN, 0.0, 0.0), ..front() }, Plane { y: Vec3::new(0.0, 0.0, f64::INFINITY), ..front() }] {
        assert!(says(axonometric(&bad, [1.0; 3]), "a frame not finite"));
    }
    for bad in [[f64::NAN, 1.0, 1.0], [1.0, f64::INFINITY, 1.0], [0.0; 3], [1e-300; 3], [MIN_DIRECTION, 0.0, 0.0]] {
        let r = axonometric(&f, bad);
        assert!(says(r.clone(), "a longer one fits") && says(r, "1e-9"), "{bad:?}");
    }
    assert!(axonometric(&f, [1.000_000_1 * MIN_DIRECTION, 0.0, 0.0]).is_ok());
    for up in [[0.0, 1.0, 0.0], [0.0, -3.0, 0.0], [1e-10, 1.0, -1e-10], [MIN_DIRECTION, 1.0, 0.0]] {
        assert!(says(axonometric(&f, up), "no horizontal"), "{up:?}");
    }
    assert!(says(axonometric(&f, [0.0, 1.0, MIN_DIRECTION]), "1e-9 rad"));
    assert!(axonometric(&f, [0.0, 1.0, 1.000_000_1 * MIN_DIRECTION]).is_ok());
}

/// The pictorial views of the principal side views on their upper diagonals in third angle, and of
/// the front view on its lower ones in first angle, are the named isometric views exactly, with no
/// -0: a diagonal shows what its two neighbours show, as the sides do. A parent given by its view
/// direction can carry a -0: looking along -X with Z up, paper x is `(0, 1, -0)`, and the view above
/// right of it has -0 in its paper x's Z until it is written out as 0.
#[test]
fn a_pictorial_view_of_a_side_view_is_a_named_iso_view() {
    use NamedView::{Back, Front, IsoBackLeft, IsoBackRight, IsoFrontLeft, IsoFrontRight, Left, Right};
    let (above_right, above_left, below_right, below_left) = ((1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0));
    let cases = [
        (Front, above_right, Projection::Third, IsoFrontRight),
        (Front, above_left, Projection::Third, IsoFrontLeft),
        (Right, above_right, Projection::Third, IsoBackRight),
        (Right, above_left, Projection::Third, IsoFrontRight),
        (Back, above_right, Projection::Third, IsoBackLeft),
        (Back, above_left, Projection::Third, IsoBackRight),
        (Left, above_right, Projection::Third, IsoFrontLeft),
        (Left, above_left, Projection::Third, IsoBackLeft),
        (Front, below_left, Projection::First, IsoFrontRight),
        (Front, below_right, Projection::First, IsoFrontLeft),
    ];
    for (parent, diagonal, projection, named) in cases {
        let view = pictorial_basis(&parent.basis(), diagonal, projection).unwrap();
        assert_eq!(view, named.basis(), "{parent:?} {diagonal:?} {projection:?}");
        assert!(!negative_zero(view.x) && !negative_zero(view.y), "{parent:?} {diagonal:?} {projection:?}: {view:?}");
    }
    let looking_along_minus_x = basis_looking(Vec3::new(-1.0, 0.0, 0.0), Vec3::Z, Vec3::ZERO).unwrap();
    assert!(negative_zero(looking_along_minus_x.x), "{looking_along_minus_x:?}");
    let view = pictorial_basis(&looking_along_minus_x, (1.0, 1.0), Projection::Third).unwrap();
    assert_eq!(view, IsoBackRight.basis());
    assert!(!negative_zero(view.x) && !negative_zero(view.y), "{view:?}");
}

/// From any view, on each diagonal in either angle: a basis through the parent's origin, its paper x
/// horizontal in the parent (square to its paper y), looking from the parent's corner
/// `(right, up, 1) / sqrt 3` in third angle and `(-right, -up, 1) / sqrt 3` in first. A parent that
/// isn't orthonormal or finite is refused.
#[test]
fn a_pictorial_view_looks_from_its_corner() {
    let leaning = basis_looking(Vec3::new(0.3, 1.0, -0.2), Vec3::new(0.1, 0.2, 1.0), Vec3::new(5.0, -7.0, 2.0)).unwrap();
    for parent in NamedView::ALL.map(NamedView::basis).into_iter().chain([leaning]) {
        for (projection, from) in [(Projection::Third, 1.0), (Projection::First, -1.0)] {
            for (right, up) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                let view = pictorial_basis(&parent, (right, up), projection).unwrap();
                validate_plane(&view).unwrap();
                assert_eq!(view.origin, parent.origin);
                let corner = (parent.x * (right * from) + parent.y * (up * from) + parent.normal()) * (1.0 / 3f64.sqrt());
                assert!(close(view.normal(), corner), "{parent:?} {right} {up} {projection:?}: {view:?}");
                assert!(view.x.dot(parent.y).abs() < EPS, "{parent:?} {right} {up} {projection:?}: paper x is horizontal");
            }
        }
    }
    let skewed = Plane { x: Vec3::new(1.0, 0.1, 0.0), ..front() };
    let not_finite = Plane { origin: Vec3::new(f64::NAN, 0.0, 0.0), ..front() };
    for parent in [skewed, not_finite] {
        assert!(says(pictorial_basis(&parent, (1.0, 1.0), Projection::Third), "a frame not finite or not orthonormal"));
    }
}

/// The paper lengths of the model's edges along X, Y and Z from the origin, in the pictorial view
/// above right of the front view at 1:1.
fn edges_in_iso(lengths: [f64; 3]) -> [f64; 3] {
    let frame = iso_frame();
    let o = frame.to_paper(Vec3::ZERO);
    let [x, y, z] = lengths;
    [Vec3::new(x, 0.0, 0.0), Vec3::new(0.0, y, 0.0), Vec3::new(0.0, 0.0, z)].map(|p| (frame.to_paper(p) - o).len())
}

/// The pictorial view above right of the front view, at 1:1, centred on the origin.
fn iso_frame() -> ViewFrame {
    let basis = pictorial_basis(&front(), (1.0, 1.0), Projection::Third).unwrap();
    ViewFrame::new(basis, Scale::new(1.0, 1.0).unwrap(), Vec2::ZERO, Vec2::ZERO).unwrap()
}

/// A 10 mm cube in true isometric at 1:1 shows each edge at 10 x sqrt(2/3) = 8.165 mm.
#[test]
fn a_10_mm_cube_in_true_isometric_has_8_165_mm_edges() {
    for edge in edges_in_iso([10.0; 3]) {
        assert!((edge - 10.0 * (2.0f64 / 3.0).sqrt()).abs() < EPS, "{edge}");
        assert!((edge - 8.16496580927726).abs() < EPS, "{edge}");
    }
}

/// A 100 x 60 x 20 block in the isometric view at 1:1: each edge at sqrt(2/3) of its length, and its
/// outline 160 / sqrt 2 wide and 200 / sqrt 6 high.
#[test]
fn a_100_by_60_by_20_block_shows_its_edges_at_the_isometric_ratio() {
    let sizes = [100.0, 60.0, 20.0];
    let edges = edges_in_iso(sizes);
    for ((edge, size), want) in edges.iter().zip(sizes).zip([81.6496580927726, 48.98979485566356, 16.32993161855452]) {
        assert!((edge - want).abs() < EPS, "{edge} {want}");
        assert!((edge / size - 0.816496580927726).abs() < EPS, "{edge} {size}");
    }
    let frame = iso_frame();
    let paper: Vec<Vec2> = (0..8)
        .map(|i| Vec3::new(f64::from(i & 1) * 100.0, f64::from((i >> 1) & 1) * 60.0, f64::from((i >> 2) & 1) * 20.0))
        .map(|p| frame.to_paper(p))
        .collect();
    let span = |f: fn(&Vec2) -> f64| paper.iter().map(f).fold(f64::NEG_INFINITY, f64::max) - paper.iter().map(f).fold(f64::INFINITY, f64::min);
    let (width, height) = (span(|p| p.x), span(|p| p.y));
    assert!((width - 113.13708498984761).abs() < EPS && (height - 81.64965809277261).abs() < EPS, "{width} {height}");
}
