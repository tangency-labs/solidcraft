use super::*;
use solidcore_record::Vec2;
use solidcraft_geom::drawing_view::{NamedView, ProjectedSide, Scale, ViewFrame, basis_looking, projected_basis};

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

// ---- An auxiliary view: folded from its parent, square to a line or a face ----

/// The parent folded toward `w` in third angle: the kernel's `fold` the tests below were written for.
fn fold(f: &Plane, w: [f64; 2]) -> Result<Plane> {
    auxiliary_basis(f, Vec2::new(w[0], w[1]), Projection::Third)
}

/// `p` in `frame`'s coordinates, restated with the test's own products.
fn seen(frame: &Plane, p: Vec3) -> (f64, f64) {
    let d = p - frame.origin;
    (d.dot(frame.x), d.dot(frame.y))
}

/// A 50 mm cube's corners from the origin.
fn cube() -> Vec<Vec3> {
    let s = 50.0;
    (0..8).map(|i| Vec3::new(f64::from(i & 1) * s, f64::from((i >> 1) & 1) * s, f64::from((i >> 2) & 1) * s)).collect()
}

/// A view leaning off every axis, as `basis_looking` makes it.
fn leaning() -> Plane {
    basis_looking(Vec3::new(0.3, 1.0, -0.2), Vec3::new(0.1, 0.2, 1.0), Vec3::new(5.0, -7.0, 2.0)).unwrap()
}

/// Toward each of its paper axes the fold is the frame turned a quarter turn about its other axis,
/// as the same doubles (`==`, so a zero's sign aside): toward +x its x becomes `-n` and its y stays,
/// toward -x `n` and y, toward +y x and `-n`, toward -y x and `n`, with `n = x × y`; a direction's
/// size changes nothing, up to 1e300.
#[test]
fn fold_toward_each_side_is_the_frame_turned_a_quarter_turn() {
    for f in [front(), tilted(), turned()] {
        let n = f.x.cross(f.y);
        for (w, want_x, want_y) in [([1.0, 0.0], -n, f.y), ([-1.0, 0.0], n, f.y), ([0.0, 1.0], f.x, -n), ([0.0, -1.0], f.x, n)] {
            for size in [1.0, 3.0, 1e300] {
                let got = fold(&f, w.map(|c| c * size)).unwrap();
                assert_eq!(got.origin, f.origin);
                assert_eq!((got.x, got.y), (want_x, want_y), "{w:?} x{size}");
            }
        }
    }
}

/// Toward each paper axis an auxiliary view is that side's projected view, in either angle, from
/// every named view and a leaning one, as the same doubles with no -0: folded right of the front
/// view it is the right view.
#[test]
fn an_auxiliary_view_toward_a_side_is_its_projected_view() {
    use ProjectedSide::{Above, Below, Left, Right};
    for parent in NamedView::ALL.map(NamedView::basis).into_iter().chain([leaning()]) {
        for projection in [Projection::Third, Projection::First] {
            for (side, toward) in
                [(Right, Vec2::new(1.0, 0.0)), (Left, Vec2::new(-1.0, 0.0)), (Above, Vec2::new(0.0, 1.0)), (Below, Vec2::new(0.0, -1.0))]
            {
                let view = auxiliary_basis(&parent, toward, projection).unwrap();
                assert_eq!(view, projected_basis(&parent, side, projection).unwrap(), "{parent:?} {side:?} {projection:?}");
                assert!(!negative_zero(view.x) && !negative_zero(view.y), "{parent:?} {side:?} {projection:?}: {view:?}");
            }
        }
    }
    assert_eq!(fold(&front(), [1.0, 0.0]).unwrap(), NamedView::Right.basis());
}

/// For 1000 seeded directions on three frames, and the boundary shapes (a -0, just past the least
/// that passes, 1e300): the axes are unit and square within `gamma_16`, the view's normal is the
/// direction (it looks from the side it sits on), the origin is kept, and every point keeps its
/// coordinate along the fold line `f = (-w_y, w_x)`, so the two views line up across it.
#[test]
fn fold_keeps_the_fold_line_and_looks_from_its_side() {
    let tol = 16.0 * f64::EPSILON / 2.0 / (1.0 - 16.0 * f64::EPSILON / 2.0);
    let check = |f: &Plane, w: [f64; 2]| {
        let got = fold(f, w).unwrap_or_else(|e| panic!("{w:?}: {e}"));
        let m = w[0].abs().max(w[1].abs());
        let (wx, wy) = (w[0] / m, w[1] / m);
        let len = (wx * wx + wy * wy).sqrt();
        let (wx, wy) = (wx / len, wy / len);
        assert!((got.x.len() - 1.0).abs() <= tol, "{w:?} {:?}", got.x);
        assert!((got.y.len() - 1.0).abs() <= tol, "{w:?} {:?}", got.y);
        assert!(got.x.dot(got.y).abs() <= tol, "{w:?}");
        assert!((got.x.cross(got.y).dot(f.x * wx + f.y * wy) - 1.0).abs() <= tol, "{w:?}: looks from its side");
        assert_eq!(got.origin, f.origin);
        for p in [Vec3::new(10.0, -3.0, 4.0), Vec3::new(-25.0, 40.0, 7.5)] {
            let ((a, b), (c, d)) = (seen(f, p), seen(&got, p));
            assert!(((-wy * a + wx * b) - (-wy * c + wx * d)).abs() <= 100.0 * tol, "{w:?} {p:?}");
        }
    };
    let mut seed = 0x0001_3044_0003_u64;
    let mut next = move || {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (seed >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    };
    for f in [front(), tilted(), turned()] {
        for _ in 0..1000 {
            let w = [next(), next()];
            if w[0].abs().max(w[1].abs()) > 1e-6 {
                check(&f, w);
            }
        }
        for w in [[-0.0, 1.0], [1.0, -0.0], [0.6, -0.8], [1e300, -1e300], [1.000_000_1 * MIN_DIRECTION, 0.0]] {
            check(&f, w);
        }
    }
}

/// Toward any side, in either angle: a basis through the parent's origin, looking from the side it
/// sits on in third angle and from the side away from it in first, and every point of a cube keeps
/// its place along the fold line (square to the side), so the two views line up across it.
#[test]
fn an_auxiliary_view_looks_from_its_side_and_lines_up_along_its_fold() {
    for parent in [front(), leaning()] {
        for (projection, from) in [(Projection::Third, 1.0), (Projection::First, -1.0)] {
            for toward in [Vec2::new(0.6, 0.8), Vec2::new(-0.28, 0.96), Vec2::new(3.0, -4.0)] {
                let view = auxiliary_basis(&parent, toward, projection).unwrap();
                validate_plane(&view).unwrap();
                assert_eq!(view.origin, parent.origin);
                let w = toward * (1.0 / toward.len());
                assert!(close(view.normal(), (parent.x * w.x + parent.y * w.y) * from), "{toward:?} {projection:?}: {view:?}");
                let along = |v: Vec2| -w.y * v.x + w.x * v.y;
                for p in cube() {
                    let (shown, folded) = (along(parent.to_local(p)), along(view.to_local(p)));
                    assert!((shown - folded).abs() < EPS, "{toward:?} {projection:?} {p:?}: {shown} {folded}");
                }
            }
        }
    }
}

/// What gives no fold is refused with its cause: a parent off orthonormal past 1e-8 or not finite;
/// a side not finite or of length at or below `MIN_DIRECTION` (a longer one fits). The thresholds
/// decide exactly.
#[test]
fn fold_refuses_what_gives_no_view() {
    let off = |dx: f64| Plane { x: Vec3::new(1.0 + dx, 0.0, 0.0), ..front() };
    assert!(fold(&off(0.5e-8), [1.0, 0.0]).is_ok());
    assert!(says(fold(&off(2e-8), [1.0, 0.0]), "not orthonormal within 1e-8"));
    assert!(says(fold(&Plane { origin: Vec3::new(f64::NAN, 0.0, 0.0), ..front() }, [1.0, 0.0]), "not finite"));
    for bad in [[f64::NAN, 1.0], [1.0, f64::INFINITY], [0.0, 0.0], [1e-300, -1e-300], [MIN_DIRECTION, 0.0]] {
        let r = fold(&front(), bad);
        assert!(says(r.clone(), "a longer one fits") && says(r, "1e-9"), "{bad:?}");
    }
    assert!(fold(&front(), [1.000_000_1 * MIN_DIRECTION, 0.0]).is_ok());
}

/// A line's view: its side is square to the line's image, and the folded view shows the line at its
/// true length, on three frames, for lines at every slope to them (seeded), within 1e-12 of their
/// length.
#[test]
fn an_auxiliary_view_of_a_line_shows_its_true_length() {
    let mut seed = 0x0001_3044_0004_u64;
    let mut next = move || {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (seed >> 11) as f64 / (1u64 << 53) as f64 * 200.0 - 100.0
    };
    for f in [front(), tilted(), turned()] {
        for _ in 0..200 {
            let (start, end) = (Vec3::new(next(), next(), next()), Vec3::new(next(), next(), next()));
            let q = auxiliary_side(&f, FoldFor::Line { start, end }, FoldSide::Toward(Vec2::new(1.0, 1.0)))
                .unwrap_or_else(|e| panic!("{start:?} {end:?}: {e:?}"));
            let view = auxiliary_basis(&f, q, Projection::Third).unwrap();
            let ((a, b), (c, d)) = (seen(&view, start), seen(&view, end));
            let length = (end - start).len();
            assert!(((c - a).hypot(d - b) - length).abs() <= 1e-12 * length.max(1.0), "{start:?} {end:?}");
            let ((a, b), (c, d)) = (seen(&f, start), seen(&f, end));
            assert!((((c - a) * q.x + (d - b) * q.y) / length).abs() <= 1e-12, "square to the image");
            assert!((q.len() - 1.0).abs() <= 1e-15);
        }
    }
}

/// A face the front view sees edge-on (its slope, 30 degrees off level, is a line in it): the side
/// runs along its normal's image, and the folded view shows the face at its true shape, a 40 by 25
/// rectangle, its sides square.
#[test]
fn an_auxiliary_view_of_an_edge_on_face_shows_its_true_shape() {
    let f = front();
    let (s, c) = (0.5, 3f64.sqrt() / 2.0);
    // Down the slope in the front view's plane (X, Z), and across it along Y.
    let (down, across, corner) = (Vec3::new(c, 0.0, -s), Vec3::Y, Vec3::new(10.0, -5.0, 30.0));
    let q = auxiliary_side(&f, FoldFor::Face { point: corner, normal: Vec3::new(s, 0.0, c) }, FoldSide::Toward(Vec2::new(1.0, 1.0))).unwrap();
    assert!((q.x - s).abs() < 1e-15 && (q.y - c).abs() < 1e-15, "{q:?}");
    let view = auxiliary_basis(&f, q, Projection::Third).unwrap();
    let (o, a, b) = (seen(&view, corner), seen(&view, corner + down * 40.0), seen(&view, corner + across * 25.0));
    let side = |p: (f64, f64)| (p.0 - o.0).hypot(p.1 - o.1);
    assert!((side(a) - 40.0).abs() < EPS && (side(b) - 25.0).abs() < EPS);
    assert!(((a.0 - o.0) * (b.0 - o.0) + (a.1 - o.1) * (b.1 - o.1)).abs() < EPS);
}

/// The side: away from the middle (a vertical line at x = 10 of the front view goes right of a
/// middle at x = 0, left of one at x = 20), or the way `toward` points; a line exactly through the
/// middle goes right, or up when its fold line runs along x; no -0.
#[test]
fn an_auxiliary_side_goes_away_from_the_middle_or_toward_its_side() {
    let f = front();
    let line = |start: [f64; 3], end: [f64; 3]| FoldFor::Line { start: Vec3::from(start), end: Vec3::from(end) };
    let side = |what, side| auxiliary_side(&f, what, side).unwrap();
    let (away, toward) = (|x: f64, y: f64| FoldSide::AwayFrom(Vec2::new(x, y)), |x: f64, y: f64| FoldSide::Toward(Vec2::new(x, y)));
    let upright = line([10.0, 0.0, 0.0], [10.0, 3.0, 20.0]);
    assert_eq!(side(upright, away(0.0, 0.0)), Vec2::new(1.0, 0.0));
    assert_eq!(side(upright, away(20.0, 0.0)), Vec2::new(-1.0, 0.0));
    assert_eq!(side(upright, toward(0.3, 5.0)), Vec2::new(1.0, 0.0));
    assert_eq!(side(upright, toward(-0.3, -5.0)), Vec2::new(-1.0, 0.0));
    // The middle at (10, 7) of the front view's plane: the line passes through it.
    assert_eq!(side(upright, away(10.0, 7.0)), Vec2::new(1.0, 0.0));
    let level = line([0.0, 0.0, 4.0], [20.0, 0.0, 4.0]);
    let up = side(level, away(0.0, 0.0));
    assert!(up == Vec2::new(0.0, 1.0) && up.x.is_sign_positive(), "{up:?}");
    assert_eq!(side(level, away(0.0, 9.0)), Vec2::new(0.0, -1.0));
    assert_eq!(side(level, away(7.0, 4.0)), Vec2::new(0.0, 1.0));
    // A face: the side away from the middle along its normal's image.
    let wall = FoldFor::Face { point: Vec3::new(0.0, 0.0, 4.0), normal: Vec3::new(0.0, 0.0, -2.0) };
    assert_eq!(side(wall, away(0.0, 9.0)), Vec2::new(0.0, -1.0));
    assert_eq!(side(wall, away(0.0, 0.0)), Vec2::new(0.0, 1.0));
}

/// Each cause refused as itself: a parent off orthonormal; a point not finite or a line of no
/// length; a line the parent sees end-on (within `MIN_DIRECTION` radians of its normal, decided
/// exactly); a face it doesn't see edge-on, with its angle (past `EDGE_ON`); a side along the fold
/// line, or not finite.
#[test]
fn an_auxiliary_side_refuses_each_cause() {
    let f = front();
    let line = |end: [f64; 3]| FoldFor::Line { start: Vec3::ZERO, end: Vec3::from(end) };
    let face = |point: [f64; 3], normal: [f64; 3]| FoldFor::Face { point: Vec3::from(point), normal: Vec3::from(normal) };
    let right = FoldSide::Toward(Vec2::new(1.0, 0.0));
    let skewed = Plane { x: Vec3::new(1.0 + 2e-8, 0.0, 0.0), ..front() };
    assert_eq!(auxiliary_side(&skewed, line([0.0, 0.0, 1.0]), right), Err(FoldRefusal::Frame));
    for bad in [
        line([0.0; 3]),
        line([f64::NAN, 0.0, 1.0]),
        line([0.0, f64::INFINITY, 1.0]),
        FoldFor::Line { start: Vec3::new(-f64::MAX, 0.0, 0.0), end: Vec3::new(f64::MAX, 0.0, 0.0) },
        face([0.0; 3], [0.0; 3]),
        face([f64::NAN, 0.0, 0.0], [1.0, 0.0, 0.0]),
    ] {
        assert_eq!(auxiliary_side(&f, bad, right), Err(FoldRefusal::Input), "{bad:?}");
    }
    // The front view looks along -Y: a line along Y is a point in it.
    for end in [[0.0, 5.0, 0.0], [MIN_DIRECTION, -1.0, 0.0]] {
        assert_eq!(auxiliary_side(&f, line(end), right), Err(FoldRefusal::EndOn), "{end:?}");
    }
    // Just past end-on, its image runs along x, and so does its fold line: the side is across it.
    let up = FoldSide::Toward(Vec2::new(0.0, 1.0));
    assert_eq!(auxiliary_side(&f, line([1.000_000_1 * MIN_DIRECTION, -1.0, 0.0]), up), Ok(Vec2::new(0.0, 1.0)));
    assert_eq!(auxiliary_side(&f, line([1.000_000_1 * MIN_DIRECTION, -1.0, 0.0]), right), Err(FoldRefusal::AlongFoldLine));
    // A face at 45 degrees to the view's plane is refused, naming its angle.
    match auxiliary_side(&f, face([0.0; 3], [1.0, -1.0, 0.0]), right) {
        Err(FoldRefusal::NotEdgeOn { angle }) => assert!((angle - std::f64::consts::FRAC_PI_4).abs() < 1e-15, "{angle}"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(auxiliary_side(&f, face([0.0; 3], [1.0, -2.0 * EDGE_ON, 0.0]), right), Err(FoldRefusal::NotEdgeOn { .. })));
    assert!(auxiliary_side(&f, face([0.0; 3], [1.0, -0.5 * EDGE_ON, 0.0]), right).is_ok());
    // The upright line's fold line runs along the front view's y.
    let upright = line([0.0, 0.0, 1.0]);
    for along in [[0.0, 1.0], [0.0, -3.0], [MIN_DIRECTION, 1.0]] {
        assert_eq!(auxiliary_side(&f, upright, FoldSide::Toward(Vec2::from(along))), Err(FoldRefusal::AlongFoldLine), "{along:?}");
    }
    assert!(auxiliary_side(&f, upright, FoldSide::Toward(Vec2::new(1e-8, 1.0))).is_ok());
    for bad in [FoldSide::Toward(Vec2::new(f64::NAN, 1.0)), FoldSide::Toward(Vec2::ZERO), FoldSide::AwayFrom(Vec2::new(f64::INFINITY, 0.0))] {
        assert_eq!(auxiliary_side(&f, upright, bad), Err(FoldRefusal::Input), "{bad:?}");
    }
}
