//! WR-651 (tangency-labs/smlib@326de73), the second half of #1799's trimmed-face tight box
//! (`Kernel::fixes.face_tight_box_searches_trim`, WR-588): the search divides an undecided
//! cell at the surface's knot inside it, not only at its middle.
//!
//! A rational patch is read monotone only within one knot span. Halved at middles alone, a
//! cell over a knot off the dyadic points of the face's domain never comes to lie within one
//! span, so along the trim it stays undecided down to the depth limit and counts with its
//! surface's top: the box overshoots. SolidCraft's case: an M10x1.5 thread cut from a shaft
//! of radius 5 and length 12 boxed to z 12.46875; its screw walls have a knot every eighth
//! of a turn.
//!
//! Here: the 270-degree cylinder of radius 1 about z (three rational quadratic quarter
//! spans, knots at u = 1/3 and 2/3), z = v over [0, 1], trimmed to z <= 0.3. Its box is
//! (-1, -1, 0) to (1, 1, 0.3).
//!
//! The first half, WR-588 itself, through an operation (#1954): a barrel with two bulges
//! (a cubic B-spline profile revolved a whole turn), its lower half cut away on one side by a
//! boolean difference, then the body's tight box. The face's surface peaks at the cut-away
//! bulge, outside the face; the face's own extreme is the upper bulge, inside it.
//!
//! #13753: a cell is set aside only on a bound that holds it, not on its solve's top. The
//! chamfered peg's Mixed corner face dips beside its seam where the solve over its whole
//! domain finds no point.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::FRAC_1_SQRT_2;

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::primitive::api_curves::{create_circle, create_curve, create_line_segment};
use solidcore::primitive::api_primitives::create_box;
use solidcore::primitive::api_trim_ops::{
    trim_project_parallel, trim_surface_with_3d_curves, TrimObject,
};
use solidcore::query::api_queries::brep_bounding_box;
use solidcore::query::face::p3::calculate_tight_bounding_box;
use solidcore::surface::surface::Surface;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::topology::brep::p1::faces;
use solidcore::topology::brep::p10::TrimType;
use solidcore::topology::brep::sheet::sheet_on;
use solidcore::Kernel;

/// The arc's knots: three quarter spans, double interior knots at 1/3 and 2/3.
fn arc_knots() -> Vec<f64> {
    let (a, b) = (1.0 / 3.0, 2.0 / 3.0);
    vec![0.0, 0.0, 0.0, a, a, b, b, 1.0, 1.0, 1.0]
}

/// The arc's control points `(x, y)` and weights, from angle 0 to 270 degrees.
fn arc_net() -> Vec<(f64, f64, f64)> {
    let s = FRAC_1_SQRT_2;
    vec![
        (1.0, 0.0, 1.0),
        (1.0, 1.0, s),
        (0.0, 1.0, 1.0),
        (-1.0, 1.0, s),
        (-1.0, 0.0, 1.0),
        (-1.0, -1.0, s),
        (0.0, -1.0, 1.0),
    ]
}

/// The arc at height `z`, reversed when `backwards` (the knots are symmetric).
fn arc(z: f64, backwards: bool) -> Curve {
    let mut net = arc_net();
    if backwards {
        net.reverse();
    }
    let pw = net
        .into_iter()
        .map(|(x, y, w)| CPoint::new(x * w, y * w, z * w, w))
        .collect();
    let nurb = NurbCurve::new(pw, 2, arc_knots()).unwrap();
    Curve::from(BSplineCurve::from_nurb(nurb, 3, false))
}

/// The cylinder: the arc along u, z = v linear over [0, 1].
fn cylinder() -> Surface {
    let net: Vec<Vec<CPoint>> = arc_net()
        .into_iter()
        .map(|(x, y, w)| {
            [0.0, 1.0]
                .iter()
                .map(|&z| CPoint::new(x * w, y * w, z * w, w))
                .collect()
        })
        .collect();
    let nurb = NurbSurface::new(net, 2, 1, arc_knots(), vec![0.0, 0.0, 1.0, 1.0]).unwrap();
    Surface::from(BSplineSurface::from_nurb(nurb, false))
}

fn line(from: Point3, to: Point3) -> Curve {
    let interval = Extent1d::from_min_max(0.0, 1.0);
    Curve::from(Line::create_line_segment(3, from, to, Some(&interval)).unwrap())
}

/// The tight box of the cylinder trimmed to `0 <= z <= top`, with the trim search on or off.
fn trimmed_cylinder_box(top: f64, searches_trim: bool) -> Extent3d {
    let mut k = Kernel::default();
    k.fixes.face_tight_box_searches_trim = searches_trim;
    let curves = vec![
        arc(0.0, false),
        line(Point3::new(0.0, -1.0, 0.0), Point3::new(0.0, -1.0, top)),
        arc(top, true),
        line(Point3::new(1.0, 0.0, top), Point3::new(1.0, 0.0, 0.0)),
    ];
    let b = trim_surface_with_3d_curves(&mut k, cylinder(), &[4], curves).unwrap();
    let fs = faces(&b, None).unwrap();
    assert_eq!(fs.len(), 1);
    calculate_tight_bounding_box(&k, &b, fs[0]).unwrap()
}

fn assert_close(what: &str, got: f64, want: f64) {
    assert!((got - want).abs() <= 1e-9, "{what}: {got}, want {want}");
}

/// With the search on, the cells over the knots divide there and are read monotone: the box
/// is the face's, exactly. Halved at middles only, the cells over u = 1/3 along the trim at
/// z = 0.3 stay undecided to the depth limit and the box reaches above 0.3.
#[test]
fn trimmed_rational_cylinder_box_is_exact_with_the_search() {
    let bx = trimmed_cylinder_box(0.3, true);
    assert_close("z max", bx.max().z, 0.3);
    assert_close("z min", bx.min().z, 0.0);
    assert_close("x max", bx.max().x, 1.0);
    assert_close("x min", bx.min().x, -1.0);
    assert_close("y max", bx.max().y, 1.0);
    assert_close("y min", bx.min().y, -1.0);
}

/// Flag off (upstream): the surface's global solves classify outside the face, so the box
/// is the vertices' and edges', which here is the face's too.
#[test]
fn trimmed_rational_cylinder_box_without_the_search_is_the_edges() {
    let bx = trimmed_cylinder_box(0.3, false);
    assert_close("z max", bx.max().z, 0.3);
    assert_close("z min", bx.min().z, 0.0);
}

/// The barrel's outer profile in the plane y = 0: a cubic B-spline from (5, 0, 0) to
/// (5, 0, 10) with a tall bulge near z = 2 and a lower one near z = 8.
fn barrel_profile() -> Curve {
    let p = |x: f64, z: f64| Point3::new(x, 0.0, z);
    let points = [
        p(5.0, 0.0),
        p(5.0, 1.0),
        p(12.0, 2.0),
        p(5.0, 3.0),
        p(5.0, 4.5),
        p(5.0, 5.5),
        p(8.5, 8.0),
        p(5.0, 9.0),
        p(5.0, 10.0),
    ];
    Curve::from(create_curve(&points).unwrap())
}

/// The profile closed by lines round a bore of radius 1, revolved a whole turn about z.
fn barrel(k: &mut Kernel) -> solidcore::Brep {
    let p = |x: f64, z: f64| Point3::new(x, 0.0, z);
    let curves = [
        barrel_profile(),
        Curve::from(create_line_segment(p(5.0, 10.0), p(1.0, 10.0)).unwrap()),
        Curve::from(create_line_segment(p(1.0, 10.0), p(1.0, 0.0)).unwrap()),
        Curve::from(create_line_segment(p(1.0, 0.0), p(5.0, 0.0)).unwrap()),
    ];
    let refs: Vec<_> = curves.iter().map(Some).collect();
    create_rotational_sweep(k, &refs, Point3::new(0.0, 0.0, 0.0), Vec3::Z, 360.0, true).unwrap()
}

/// The profile's point at parameter `t`.
fn profile_at(k: &Kernel, c: &Curve, t: f64) -> Point3 {
    c.evaluate_point(k, t).unwrap()
}

/// The profile's greatest radius (x) where z lies in `lo..hi`, from the curve itself: the
/// best of 4096 samples, then a golden-section search round it.
fn profile_max_x(lo: f64, hi: f64) -> f64 {
    let k = Kernel::default();
    let c = barrel_profile();
    let ivl = c.natural_interval();
    let n = 4096;
    let step = (ivl.max() - ivl.min()) / f64::from(n);
    let x_at = |t: f64| {
        let q = profile_at(&k, &c, t);
        if q.z > lo && q.z < hi {
            q.x
        } else {
            f64::NEG_INFINITY
        }
    };
    let best = (0..=n)
        .map(|i| ivl.min() + step * f64::from(i))
        .max_by(|a, b| x_at(*a).total_cmp(&x_at(*b)))
        .unwrap();
    let (mut a, mut b) = (best - step, best + step);
    let g = (5.0_f64.sqrt() - 1.0) / 2.0;
    for _ in 0..200 {
        let (c1, c2) = (b - g * (b - a), a + g * (b - a));
        if x_at(c1) < x_at(c2) {
            a = c1;
        } else {
            b = c2;
        }
    }
    x_at((a + b) / 2.0)
}

/// The profile's radius (x) at height `z` (z rises along the profile), by bisection.
fn profile_x_at_z(z: f64) -> f64 {
    let k = Kernel::default();
    let c = barrel_profile();
    let ivl = c.natural_interval();
    let (mut a, mut b) = (ivl.min(), ivl.max());
    for _ in 0..200 {
        let m = (a + b) / 2.0;
        if profile_at(&k, &c, m).z < z {
            a = m;
        } else {
            b = m;
        }
    }
    profile_at(&k, &c, (a + b) / 2.0).x
}

/// WR-588 at operation level (#1954): the barrel's tight box is its surface's either way.
/// Cut by a boolean difference of the box over y >= 0, z <= 5, the tall bulge stays on the
/// face only on the side y < 0. The face's +y extreme is the lower bulge's crest, inside the
/// face. Off (pinned upstream), the global solve finds the cut-away bulge, classifies it
/// outside the face and drops it: the body's +y side is the cut edge round the waist at
/// z = 5. On (the fork), the trim search finds the crest. The other five sides agree.
#[test]
fn wr_588_barrel_cut_below_its_waist_is_boxed_to_its_upper_bulge() {
    let tall = profile_max_x(0.0, 5.0);
    let low = profile_max_x(5.0, 10.0);
    let waist = profile_x_at_z(5.0);
    assert!(
        waist < low - 1.0 && low < tall - 1.0,
        "{waist} {low} {tall}"
    );
    for searches_trim in [false, true] {
        let mut k = Kernel::default();
        k.fixes.face_tight_box_searches_trim = searches_trim;
        let whole = barrel(&mut k);
        // control: uncut, the surface's global extremes are the face's
        let bx = brep_bounding_box(&k, &whole, true).unwrap();
        for (what, got, want) in [
            ("x max", bx.max().x, tall),
            ("x min", bx.min().x, -tall),
            ("y max", bx.max().y, tall),
            ("y min", bx.min().y, -tall),
            ("z max", bx.max().z, 10.0),
            ("z min", bx.min().z, 0.0),
        ] {
            assert!(
                (got - want).abs() <= 1e-9,
                "{searches_trim}: whole {what}: {got}, want {want}"
            );
        }

        let cut = create_box(&mut k, Point3::new(-20.0, 0.0, -1.0), 40.0, 20.0, 6.0).unwrap();
        let body = boolean_difference(&mut k, whole, cut).unwrap();
        assert_eq!(faces(&body, None).unwrap().len(), 7, "{searches_trim}");
        let bx = brep_bounding_box(&k, &body, true).unwrap();
        let want_y = if searches_trim { low } else { waist };
        assert!(
            (bx.max().y - want_y).abs() <= 1e-9,
            "{searches_trim}: y max {}, want {want_y}",
            bx.max().y
        );
        for (what, got, want) in [
            ("x max", bx.max().x, tall),
            ("x min", bx.min().x, -tall),
            ("y min", bx.min().y, -tall),
            ("z max", bx.max().z, 10.0),
            ("z min", bx.min().z, 0.0),
        ] {
            assert!(
                (got - want).abs() <= 1e-9,
                "{searches_trim}: cut {what}: {got}, want {want}"
            );
        }
    }
}

/// Clamped uniform cubic knots for `n` control points over [0, 1].
fn uniform_knots(n: usize) -> Vec<f64> {
    let spans = n - 3;
    let mut knots = vec![0.0; 4];
    knots.extend((1..spans).map(|i| i as f64 / spans as f64));
    knots.extend([1.0; 4]);
    knots
}

/// The sheet's control points each way (37 knot spans).
const SHEET_N: usize = 40;
/// The sheet's height: z = `SHEET_RISE` (y - `SHEET_FLAT`)^3, flat across y = `SHEET_FLAT`.
const SHEET_RISE: f64 = 0.05;
const SHEET_FLAT: f64 = 6.0;
/// The disk the sheet is trimmed to: centre (5, 4), radius 2, its top point (5, 6) where
/// the sheet is flat.
const DISK: (f64, f64, f64) = (5.0, 4.0, 2.0);

/// The 10 x 10 cubic sheet over u, v in [0, 1]: x = 10 u, y = 10 v and
/// z = `SHEET_RISE` (y - `SHEET_FLAT`)^3, exactly. Each control value is the polynomial's
/// blossom at the three knots after it (x and y the Greville abscissae, their blossoms).
fn sheet() -> Surface {
    let knots = uniform_knots(SHEET_N);
    let net: Vec<Vec<CPoint>> = (0..SHEET_N)
        .map(|i| {
            let x = 10.0 * (knots[i + 1] + knots[i + 2] + knots[i + 3]) / 3.0;
            (0..SHEET_N)
                .map(|j| {
                    let t = [knots[j + 1], knots[j + 2], knots[j + 3]].map(|t| 10.0 * t);
                    let y = (t[0] + t[1] + t[2]) / 3.0;
                    let z = SHEET_RISE
                        * (t[0] - SHEET_FLAT)
                        * (t[1] - SHEET_FLAT)
                        * (t[2] - SHEET_FLAT);
                    CPoint::new(x, y, z, 1.0)
                })
                .collect()
        })
        .collect();
    let nurb = NurbSurface::new(net, 3, 3, knots.clone(), knots).unwrap();
    Surface::from(BSplineSurface::from_nurb(nurb, false))
}

/// #14452: a G2 corner patch's tight box took 3-4 s a face (46 s for a box with eight): a
/// 129 x 131 rational bicubic net, its +x, +y or +z extreme along the trim, where the patch
/// meets a wall tangentially, the surface past the trim a little higher. The trim search's
/// cells along that trim were each solved over the whole surface, and a solve's control-net
/// guesses read every control point of the surface it is given: thousands of cells, each
/// reading all 16,899. Each cell of a B-spline face is now solved on its own subpatch, and a
/// cell whose subpatch's control points reach no higher than the box so far is skipped.
///
/// Here: a 40 x 40 cubic sheet z = 0.05 (y - 6)^3 trimmed to the disk about (5, 4) of radius
/// 2. The face's highest point is the disk's top (5, 6, 0), where the sheet is flat; past
/// the trim the sheet rises. The box is checked against the exact sheet, and the control
/// points the solves read are counted: 1,433,600 before (896 whole nets of 1,600), 40,562
/// now.
#[test]
fn tight_box_cells_are_solved_on_their_own_subpatch() {
    let mut k = Kernel::default();
    k.fixes.face_tight_box_searches_trim = true;
    let (cx, cy, r) = DISK;
    let circle = Curve::from(create_circle(Point3::new(cx, cy, 5.0), r).unwrap());
    let b = trim_project_parallel(
        &mut k,
        TrimObject::Surface(Box::new(sheet())),
        &circle,
        Vec3::new(0.0, 0.0, -1.0),
        Point3::new(cx, cy, 0.0),
        TrimType::KeepPoint,
    )
    .unwrap();
    let fs = faces(&b, None).unwrap();
    assert_eq!(fs.len(), 1);
    k.work_counters.reset();
    let bx = calculate_tight_bounding_box(&k, &b, fs[0]).unwrap();
    let read = k.work_counters.face_extreme_control_points();

    // The face is the sheet over the disk: x, y the disk's extents, z from
    // 0.05 (2 - 6)^3 = -3.2 at its bottom point to 0 at its top. The trim is the circle
    // projected within the trim tolerance (0.01), so x and y are held to that; z is flat at
    // the top, where a trim 0.01 past the circle rises 0.05 x 0.01^3 = 5e-8.
    let z = |y: f64| SHEET_RISE * (y - SHEET_FLAT).powi(3);
    for (what, got, want, tol) in [
        ("x min", bx.min().x, cx - r, 1e-3),
        ("x max", bx.max().x, cx + r, 1e-3),
        ("y min", bx.min().y, cy - r, 1e-3),
        ("y max", bx.max().y, cy + r, 1e-3),
        ("z min", bx.min().z, z(cy - r), 1e-3),
        ("z max", bx.max().z, z(cy + r), 5e-8),
    ] {
        assert!((got - want).abs() <= tol, "{what}: {got}, want {want}");
    }

    let net = (SHEET_N * SHEET_N) as u64;
    eprintln!("control points read {read} (whole net {net})");
    assert!(
        read <= 50 * net,
        "control points read {read}, whole net {net}"
    );
}

/// The ramp's height over x: z = `RAMP_BEND` x^2.
const RAMP_BEND: f64 = 0.02;
/// The ramp's length along x.
const RAMP_LENGTH: f64 = 10.0;
/// Where the ramp is cut off: it is trimmed to x in 0.5..`RAMP_STOP`, across all of y.
const RAMP_STOP: f64 = 6.3;

/// A ramp like a thread's screw wall: a rational quadratic B-spline with `spans` knot spans
/// along u (x from 0 to `RAMP_LENGTH`, uniform knots, every other weight 0.8) and one
/// linear span across v (y from 0 to 1); its control points' heights are `RAMP_BEND` x^2
/// of their x, so z rises along u and does not change across v.
fn ramp(spans: usize) -> Surface {
    let n = spans + 2;
    let mut knots = vec![0.0; 3];
    knots.extend((1..spans).map(|i| i as f64 / spans as f64));
    knots.extend([1.0; 3]);
    let net: Vec<Vec<CPoint>> = (0..n)
        .map(|i| {
            let w = if i % 2 == 1 { 0.8 } else { 1.0 };
            let x = RAMP_LENGTH * i as f64 / (n - 1) as f64;
            let z = RAMP_BEND * x * x;
            [0.0, 1.0]
                .iter()
                .map(|&y| CPoint::new(x * w, y * w, z * w, w))
                .collect()
        })
        .collect();
    let nurb = NurbSurface::new(net, 2, 1, knots, vec![0.0, 0.0, 1.0, 1.0]).unwrap();
    Surface::from(BSplineSurface::from_nurb(nurb, false))
}

/// The ramp's height where its x is `x`, from the surface itself (x rises along u and z
/// does not change across v): bisection on u.
fn ramp_z_at(spans: usize, x: f64) -> f64 {
    let k = Kernel::default();
    let s = ramp(spans);
    let at = |u: f64| s.evaluate_point(&k, Point2::new(u, 0.5)).unwrap();
    let (mut a, mut b) = (0.0, 1.0);
    for _ in 0..200 {
        let m = 0.5 * (a + b);
        if at(m).x < x {
            a = m;
        } else {
            b = m;
        }
    }
    at(0.5 * (a + b)).z
}

/// The ramp of `spans` spans trimmed to x in 0.5..`RAMP_STOP` (a rectangle projected down
/// onto it, its long sides on the ramp's own sides y = 0 and y = 1, as a screw wall's
/// trims run along its sides): its tight box and the control points the trim search's
/// solves read.
fn trimmed_ramp_box(spans: usize) -> (Extent3d, u64) {
    let mut k = Kernel::default();
    k.fixes.face_tight_box_searches_trim = true;
    let corners = [
        (0.5, 0.0),
        (RAMP_STOP, 0.0),
        (RAMP_STOP, 1.0),
        (0.5, 1.0),
        (0.5, 0.0),
    ];
    let pw = corners
        .iter()
        .map(|&(x, y)| CPoint::new(x, y, 5.0, 1.0))
        .collect();
    let knots = vec![0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0];
    let window = Curve::from(BSplineCurve::from_nurb(
        NurbCurve::new(pw, 1, knots).unwrap(),
        3,
        false,
    ));
    let b = trim_project_parallel(
        &mut k,
        TrimObject::Surface(Box::new(ramp(spans))),
        &window,
        Vec3::new(0.0, 0.0, -1.0),
        Point3::new(3.0, 0.5, 0.0),
        TrimType::KeepPoint,
    )
    .unwrap();
    let fs = faces(&b, None).unwrap();
    assert_eq!(fs.len(), 1);
    k.work_counters.reset();
    let bx = calculate_tight_bounding_box(&k, &b, fs[0]).unwrap();
    (bx, k.work_counters.face_extreme_control_points())
}

/// #14460: a thread's screw wall (a knot every eighth of a turn along u, one span across v)
/// took 1.2 s to box: its +z and -z extremes lie past the trim, and the cells along the
/// trim stay undecided until they lie within one span along u (the monotone test of a
/// rational patch). Each undecided cell was divided both ways, so v, which is one span,
/// was halved at every level too and the cells along the trim doubled each level: as many
/// cells as the wall has spans, each solved twice. A cell is now divided only the ways a
/// knot lies inside it while either way has one.
///
/// Here: a rational ramp, z = 0.02 x^2 over its control points, one span across, trimmed to
/// 0.5 <= x <= 6.3, its +z and -z (and +x, -x) extremes past the trim; with 16 and with 128
/// spans along u. Its box is the trimmed ramp's (z from the ramp's height at x = 0.5 to
/// that at x = 6.3, to the trim's projection tolerance). The control points the solves read
/// (each solve reads its cell's net): before, 2,848 and 37,152, 143 times the 128-span
/// ramp's net; now 728 and 4,872, 19 times it, the depth-0 solves over the whole net most
/// of it.
#[test]
fn a_wall_one_span_across_is_divided_only_along_its_spans() {
    let mut read = Vec::new();
    for spans in [16, 128] {
        let z = |x: f64| ramp_z_at(spans, x);
        let (bx, n) = trimmed_ramp_box(spans);
        for (what, got, want) in [
            ("x min", bx.min().x, 0.5),
            ("x max", bx.max().x, RAMP_STOP),
            ("y min", bx.min().y, 0.0),
            ("y max", bx.max().y, 1.0),
            ("z min", bx.min().z, z(0.5)),
            ("z max", bx.max().z, z(RAMP_STOP)),
        ] {
            assert!(
                (got - want).abs() <= 1e-3,
                "{spans} spans: {what}: {got}, want {want}"
            );
        }
        eprintln!("{spans} spans: control points read {n}");
        read.push((spans, n));
    }
    let [(few, read_few), (many, read_many)] = read[..] else {
        panic!("two ramps");
    };
    let net = 2 * (many as u64 + 2);
    assert!(
        read_many <= 30 * net,
        "{many} spans: control points read {read_many}, net {net}"
    );
    assert!(
        read_many * few as u64 <= read_few * many as u64,
        "control points read grow faster than the spans: {read_few} at {few}, {read_many} at {many}"
    );
}

/// The Mixed corner's chamfer face of the peg case
/// `fillets/report-peg-front-and-concave-chamfers-together-d2` (degrees 9 and 5, 91 x 6
/// control points), as the case built it on 2026-11-05: its record's homogeneous net in
/// `data/issue_13753_mixed_chamfer_face.json`. In the case it is a B-spline face open both
/// ways too, so its search takes the same path.
fn mixed_chamfer_face() -> Surface {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/issue_13753_mixed_chamfer_face.json"
    ))
    .unwrap();
    let data: serde_json::Value = serde_json::from_str(&text).unwrap();
    let floats = |v: &serde_json::Value| -> Vec<f64> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap())
            .collect()
    };
    let net: Vec<Vec<CPoint>> = data["homogeneous"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            row.as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    let h = floats(p);
                    CPoint::new(h[0], h[1], h[2], h[3])
                })
                .collect()
        })
        .collect();
    let (u, v) = (floats(&data["u_knots"]), floats(&data["v_knots"]));
    let (p, q) = (u.len() - net.len() - 1, v.len() - net[0].len() - 1);
    assert_eq!((p, q), (9, 5));
    let nurb = NurbSurface::new(net, p, q, u, v).unwrap();
    Surface::from(BSplineSurface::from_nurb(nurb, false))
}

/// #13753: the face on its own (a sheet over its whole domain) dips beside its side v = 1,
/// which lies in y = 0, to y = -7.2151e-4 at (0.6979, 0.9655). The solve over the whole
/// domain in -y gives (0, 1) at y = 0, the edges' value; a search that set the domain aside
/// on it boxed the face to y min 0. Its box holds the dip to the search's resolution, 1e-6;
/// its other sides are its edges' (x 32 to 40, y max 2, z 8 to 10).
#[test]
fn a_dip_the_whole_domain_solve_misses_is_inside_the_box() {
    let mut k = Kernel::default();
    k.fixes.face_tight_box_searches_trim = true;
    let face = mixed_chamfer_face();
    let dip = face
        .evaluate_point(
            &k,
            Point2::new(0.697_941_702_278_245_9, 0.965_501_350_901_047_9),
        )
        .unwrap();
    assert!(dip.y < -7.2e-4, "the dip: {dip:?}");
    let (b, f) = sheet_on(&mut k, face, 1e-6).unwrap();
    assert_eq!(
        (
            b.faces().len(),
            b.edges().len(),
            b.vertices().len(),
            b.shells().len()
        ),
        (1, 4, 4, 1)
    );
    let bx = calculate_tight_bounding_box(&k, &b, f).unwrap();
    assert!(
        (bx.min().y - dip.y).abs() <= 1e-6,
        "y min {}, the dip {}",
        bx.min().y,
        dip.y
    );
    assert_close("x min", bx.min().x, 32.0);
    assert_close("x max", bx.max().x, 40.0);
    assert_close("y max", bx.max().y, 2.0);
    assert_close("z min", bx.min().z, 8.0);
    assert_close("z max", bx.max().z, 10.0);
}

/// A quarter of a cylinder of radius 10 about z, from -30 to 60 degrees (one rational
/// quadratic span along u), z = 10 v over [0, 1]: its greatest x, 10, runs along the whole
/// line of angle 0 inside it, which no halving of u reaches.
fn quarter_cylinder() -> Surface {
    let (r, s) = (10.0, FRAC_1_SQRT_2);
    let at = |deg: f64, scale: f64| {
        let a = deg.to_radians();
        (scale * a.cos(), scale * a.sin())
    };
    let ((x0, y0), (x1, y1), (x2, y2)) = (at(-30.0, r), at(15.0, r / s), at(60.0, r));
    let arc = [(x0, y0, 1.0), (x1, y1, s), (x2, y2, 1.0)];
    let net: Vec<Vec<CPoint>> = arc
        .iter()
        .map(|&(x, y, w)| {
            [0.0, 10.0]
                .iter()
                .map(|&z| CPoint::new(x * w, y * w, z * w, w))
                .collect()
        })
        .collect();
    let nurb = NurbSurface::new(
        net,
        2,
        1,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
    )
    .unwrap();
    Surface::from(BSplineSurface::from_nurb(nurb, false))
}

/// #13753: a face whose greatest value runs along a line inside it (a cylinder's, in x).
/// Set aside only on their control points, the cells along that line are divided until
/// their hull comes within 1e-6 of it; halved along the line too, they doubled at every
/// depth, the search reached its cell limit, and the cells left on the stack counted with
/// hulls far above the face. Divided only across the line, the box is the face's (x max 10
/// to 1e-6) and the solves read few control points.
#[test]
fn a_ridge_along_a_face_is_divided_only_across_it() {
    let mut k = Kernel::default();
    k.fixes.face_tight_box_searches_trim = true;
    let (b, f) = sheet_on(&mut k, quarter_cylinder(), 1e-6).unwrap();
    assert_eq!(
        (
            b.faces().len(),
            b.edges().len(),
            b.vertices().len(),
            b.shells().len()
        ),
        (1, 4, 4, 1)
    );
    k.work_counters.reset();
    let bx = calculate_tight_bounding_box(&k, &b, f).unwrap();
    let read = k.work_counters.face_extreme_control_points();
    assert!((bx.max().x - 10.0).abs() <= 1e-6, "x max {}", bx.max().x);
    assert!((bx.min().x - 5.0).abs() <= 1e-6, "x min {}", bx.min().x);
    assert!((bx.min().y + 5.0).abs() <= 1e-6, "y min {}", bx.min().y);
    assert!(
        (bx.max().y - 10.0 * 60f64.to_radians().sin()).abs() <= 1e-6,
        "y max {}",
        bx.max().y
    );
    assert_close("z min", bx.min().z, 0.0);
    assert_close("z max", bx.max().z, 10.0);
    eprintln!("control points read {read}");
    assert!(read <= 2000, "control points read {read}");
}
