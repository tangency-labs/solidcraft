//! Faces on a surface the kernel's evaluator refuses on one circle: the planner leaves such a
//! face out whole and names the point, wherever it meets it (a node, a middle the sag check
//! needs, a node of a seam cut, or a point a size rule probes between the nodes it places).
//! Nothing is substituted for the point, and no point is skipped.

use solidcore::eval::{EvalCurve, EvalSurface};
use solidcore_convert as kernel;
use solidcore_record::Vec3;

use crate::vec::VecExt as _;

use super::size::{FaceSize, Rules, face_size};
use super::{NoValue, plan_or_fill, seam, uv};

/// An exact cylinder of radius 3 about z, its profile `u` in [0, 1] running from z = 0 to
/// z = 40 (`z = 40 u`), `v` the angle: a rational cubic whose weight has a double root at
/// `hole` (0.5 or 0.25), where both its numerator and weight vanish. The evaluator has no point
/// on the circle u = `hole` (0 / 0) and the cylinder's own everywhere else. The weights are
/// exact in binary, so the root is exactly zero there.
pub(crate) fn cylinder_without(hole: f64) -> EvalSurface {
    // Homogeneous control points (w x, w y, w z, w): the weight is (1 - u / hole)² scaled, and
    // w z is 40 u times it, in the cubic Bernstein basis.
    let ctrl = if hole == 0.5 {
        // Weight 3 (1 - 2u)²: Bernstein (3, -1, -1, 3); w z = 40 u · weight: (0, 40, -80, 120).
        vec![
            [9.0, 0.0, 0.0, 3.0],
            [-3.0, 0.0, 40.0, -1.0],
            [-3.0, 0.0, -80.0, -1.0],
            [9.0, 0.0, 120.0, 3.0],
        ]
    } else {
        assert_eq!(hole, 0.25);
        // Weight 3 (1 - 4u)²: Bernstein (3, -5, 3, 27); w z = 40 u · weight: (0, 40, -240, 1080).
        vec![
            [9.0, 0.0, 0.0, 3.0],
            [-15.0, 0.0, 40.0, -5.0],
            [9.0, 0.0, -240.0, 3.0],
            [81.0, 0.0, 1080.0, 27.0],
        ]
    };
    let s = EvalSurface::Revolved {
        profile: EvalCurve::new(3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], ctrl),
        origin: solidcore::Vec3::ZERO,
        axis: solidcore::Vec3::Z,
        transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        swap: false,
    };
    assert!(
        s.derivs(hole, 0.3).is_none(),
        "no point on the circle u = {hole}"
    );
    for u in [0.0, 0.125, 0.375, 0.5, 0.625, 0.75, 1.0]
        .into_iter()
        .filter(|u| *u != hole)
    {
        let p = point(&s, [u, 0.3]);
        let want = Vec3::new(3.0 * 0.3f64.cos(), 3.0 * 0.3f64.sin(), 40.0 * u);
        assert!(p.dist(want) < 1e-9, "the cylinder at u = {u}: {p:?}");
    }
    s
}

/// The size the planner works the cylinder's faces out at: side `h`, its speeds, no sample
/// left out.
fn size(h: f64) -> FaceSize {
    FaceSize {
        h,
        speed: [40.0, 3.0],
        unevaluated: Vec::new(),
    }
}

fn point(s: &EvalSurface, q: [f64; 2]) -> Vec3 {
    kernel::record_vec3(s.derivs(q[0], q[1]).unwrap().0)
}

/// A quarter turn of that cylinder, u in [0, `top`] and v in [0, pi/2], as a loop
/// counter-clockwise in (u, v): its straight sides (u = 0 to `top` at v = 0 and v = pi/2) one
/// segment each, its arcs (u = 0 and u = `top`) `n` segments.
fn quarter(s: &EvalSurface, n: u32, top: f64) -> seam::Loop {
    let quarter = std::f64::consts::FRAC_PI_2;
    let at = |k: u32| quarter * f64::from(k) / f64::from(n);
    let mut uv: Vec<[f64; 2]> = vec![[0.0, 0.0]];
    uv.extend((0..=n).map(|k| [top, at(k)]));
    // Down the arc at u = 0, from v = pi/2 to just above the start.
    uv.extend((1..=n).rev().map(|k| [0.0, at(k)]));
    let pts = uv.iter().map(|q| point(s, *q)).collect();
    seam::Loop { uv, pts }
}

/// The face on [`cylinder_without`]`(0.5)`: whatever layout meshes it puts nodes or middles on
/// the circle u = 0.5. The face is refused with the point, not meshed around it, and not filled
/// leniently: no layout meshes a surface that is not there.
#[test]
fn a_face_whose_surface_refuses_an_inner_point_is_refused_whole() {
    let s = cylinder_without(0.5);
    let loops = [quarter(&s, 8, 1.0)];
    let rules = Rules::new(0.01, 40.0, None);
    let r = plan_or_fill(0, "revolution", &s, &loops, &rules, &size(1.0), 0.0, None);
    match r {
        Err(e) => assert!(
            e.to_string()
                .contains("the surface does not evaluate at [0.5"),
            "{e}"
        ),
        Ok((fp, stats)) => {
            panic!(
                "meshed with {} triangles, lenient {:?}",
                fp.triangles.len(),
                stats.lenient
            )
        }
    }
}

/// On [`cylinder_without`]`(0.25)` the straight direction needs no line inside [0, 1] (the
/// profile is straight), so no node lies on u = 0.25, and the sag check looks at middles
/// (u = 0.5) and centroids (u = 1/3, 2/3) only. The size rules' probes between the lines (the
/// quarter points of an interval, and the iso-lines across it) do reach u = 0.25: there the
/// surface refuses, and the division names the point instead of taking the interval as fine.
#[test]
fn the_division_names_a_point_between_its_lines_the_surface_does_not_evaluate_at() {
    let s = cylinder_without(0.25);
    let rules = Rules::new(0.01, 40.0, None);
    let quarter = (0.0, std::f64::consts::FRAC_PI_2);
    assert!(matches!(
        uv::divide(&s, 0, (0.0, 1.0), quarter, &rules, 50.0, 1000, &uv::everywhere),
        Err(uv::Unmeasured::NoValue(NoValue([u, _]))) if u == 0.25
    ));
    assert!(matches!(
        uv::divide(&s, 1, quarter, (0.0, 1.0), &rules, 50.0, 1000, &uv::everywhere),
        Err(uv::Unmeasured::NoValue(NoValue([u, _]))) if u == 0.25
    ));
    assert_eq!(
        uv::fine_between(&s, &rules, 50.0, [0.0, 0.1], [0.5, 0.1]),
        Err(NoValue([0.25, 0.1]))
    );
    // Off the circle the rules hold: the profile is straight, and 40 mm is within 50.
    assert_eq!(
        uv::fine_between(&s, &rules, 50.0, [0.5, 0.1], [1.0, 0.1]),
        Ok(true)
    );
}

/// The face on [`cylinder_without`]`(0.25)`, its arcs fine enough (16 segments on r = 3) that no
/// side across the face sags past the tolerance: its nodes stay off u = 0.25 and so do its
/// triangles' middles (see above). Taking a point the surface does not evaluate at as fine, the
/// planner meshed it as a strip of 32 triangles straight over the circle the surface does not
/// have. The size rules probe u = 0.25, and the face is refused there.
#[test]
fn a_face_whose_surface_refuses_a_point_between_its_nodes_is_refused_whole() {
    let s = cylinder_without(0.25);
    let loops = [quarter(&s, 16, 1.0)];
    let rules = Rules::new(0.01, 40.0, None);
    let r = plan_or_fill(0, "revolution", &s, &loops, &rules, &size(50.0), 0.0, None);
    match r {
        Err(e) => assert!(
            e.to_string()
                .contains("the surface does not evaluate at [0.25"),
            "{e}"
        ),
        Ok((fp, stats)) => {
            panic!(
                "meshed with {} triangles, lenient {:?}",
                fp.triangles.len(),
                stats.lenient
            )
        }
    }
}

/// The Delaunay sag check evaluates every triangle's corners and middle: a surface that refuses
/// a middle stops it with that point, instead of taking the triangle as on the surface.
#[test]
fn the_delaunay_sag_check_names_a_middle_the_surface_does_not_evaluate_at() {
    use super::cdt::{Fill, Sag, triangulate};
    let b = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    // A plane with no point inside the open square.
    let eval = |u: f64, v: f64| {
        let inside = u > 0.0 && u < 1.0 && v > 0.0 && v < 1.0;
        if inside {
            Err(NoValue([u, v]))
        } else {
            Ok((Vec3::new(u, v, 0.0), Some(Vec3::Z)))
        }
    };
    let sag = Sag {
        eval: &eval,
        tol: 1e-3,
        h: 10.0,
        turn: None,
        scale: [1.0, 1.0],
    };
    let fill = Fill {
        seeds: &[],
        refine: Some((sag, 100)),
        shape: false,
        lenient: false,
    };
    match triangulate(&b, std::slice::from_ref(&(0..4)), fill) {
        Err(NoValue([u, v])) => assert!(u > 0.0 && u < 1.0 && v > 0.0 && v < 1.0, "{u} {v}"),
        other => panic!("{other:?}"),
    }
}

/// The size samples cover the surface's whole parameter box, which reaches past a trimmed face.
/// On [`cylinder_without`]`(0.5)` the samples on u = 0.5 (the box's middle row) do not
/// evaluate: they measure nothing, and they refuse only a face they lie inside. A face trimmed
/// to u in [0, 0.25] meshes; the whole face, over u = 0.5, is refused at such a sample.
#[test]
fn a_size_sample_the_surface_refuses_refuses_only_a_face_it_lies_in() {
    let s = cylinder_without(0.5);
    let rules = Rules::new(0.01, 40.0, None);
    let size = face_size(&s, &solidcore::eval::uv::domain(&s), &rules);
    assert!(!size.unevaluated.is_empty(), "{size:?}");
    assert!(
        size.unevaluated.iter().all(|q| q[0] == 0.5),
        "{:?}",
        size.unevaluated
    );
    // A face beside the circle: the samples on it lie outside, and it meshes.
    let low = [quarter(&s, 16, 0.25)];
    match plan_or_fill(0, "revolution", &s, &low, &rules, &size, 0.0, None) {
        Ok((fp, stats)) => assert!(
            !fp.triangles.is_empty() && stats.lenient.is_empty(),
            "{stats:?}"
        ),
        Err(e) => panic!("{e}"),
    }
    // The face over it: refused at the first sample inside it, before any layout.
    let whole = [quarter(&s, 16, 1.0)];
    let inside = size
        .unevaluated
        .iter()
        .find(|q| q[1] <= std::f64::consts::FRAC_PI_2)
        .copied();
    let Some(inside) = inside else {
        panic!("a sample inside the quarter turn: {size:?}")
    };
    match plan_or_fill(0, "revolution", &s, &whole, &rules, &size, 0.0, None) {
        Err(e) => assert!(
            e.to_string()
                .contains(&format!("does not evaluate at {inside:?}")),
            "{e}"
        ),
        Ok((fp, _)) => panic!("meshed with {} triangles", fp.triangles.len()),
    }
}

/// The plane z = 0, x = u, y = v over [0, 1]², as a rational bicubic whose weight
/// (u − ½)² + (v − ½)² has its one root at (½, ½), where its numerators vanish too: the
/// evaluator has no point there (0 / 0) and the plane's own everywhere else. All its homogeneous
/// coordinates are whole numbers (36 times the weight's and numerators' Bernstein
/// coefficients), so the root is exactly zero.
fn plane_without_its_middle() -> EvalSurface {
    // 36 (t − ½)², 36 t (t − ½)², 3 t and 12 (t − ½)² in the cubic Bernstein basis.
    let a36 = [9.0, -3.0, -3.0, 9.0];
    let ta36 = [0.0, 3.0, -6.0, 9.0];
    let t3 = [0.0, 1.0, 2.0, 3.0];
    let a12 = [3.0, -1.0, -1.0, 3.0];
    let mut ctrl = Vec::new();
    for j in 0..4 {
        for i in 0..4 {
            // w u = u (u − ½)² + u (v − ½)², w v = (u − ½)² v + v (v − ½)².
            let x = ta36[i] + t3[i] * a12[j];
            let y = a12[i] * t3[j] + ta36[j];
            ctrl.push([x, y, 0.0, a36[i] + a36[j]]);
        }
    }
    let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let s = EvalSurface::nurbs([3, 3], [knots.clone(), knots], [4, 4], ctrl);
    assert!(s.derivs(0.5, 0.5).is_none(), "no point at the middle");
    for q in [[0.3, 0.7], [0.5, 0.25], [0.9, 0.5], [0.0, 1.0]] {
        let p = point(&s, q);
        assert!(p.dist(Vec3::new(q[0], q[1], 0.0)) < 1e-12, "{q:?}: {p:?}");
    }
    s
}

/// A loop through the corners `corners` (counter-clockwise in (u, v)), each side in pieces of
/// at most 0.1, on the surface `s`.
fn polygon(s: &EvalSurface, corners: &[[f64; 2]]) -> seam::Loop {
    let mut uv = Vec::new();
    for (k, a) in corners.iter().enumerate() {
        let b = corners[(k + 1) % corners.len()];
        let n = ((b[0] - a[0]).hypot(b[1] - a[1]) / 0.1).ceil().max(1.0) as u32;
        uv.extend((0..n).map(|i| {
            let t = f64::from(i) / f64::from(n);
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
        }));
    }
    let pts = uv.iter().map(|q| point(s, *q)).collect();
    seam::Loop { uv, pts }
}

/// The face's divisions run iso-lines across its whole parameter box, which reaches past a
/// trimmed face. On [`plane_without_its_middle`], a C-shaped face whose notch holds the middle
/// (½, ½) has that point in its box but not in itself: the point measures nothing, and the face
/// meshes (the division refused it there). A square face over the middle is refused at it.
#[test]
fn a_point_in_a_faces_notch_the_surface_does_not_evaluate_at_does_not_refuse_it() {
    let s = plane_without_its_middle();
    let rules = Rules::new(0.01, 1.0, None);
    let size = face_size(&s, &solidcore::eval::uv::domain(&s), &rules);
    let notched = [polygon(
        &s,
        &[
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 0.4],
            [0.4, 0.4],
            [0.4, 0.6],
            [1.0, 0.6],
            [1.0, 1.0],
            [0.0, 1.0],
        ],
    )];
    match plan_or_fill(0, "spline", &s, &notched, &rules, &size, 0.0, None) {
        Ok((fp, stats)) => assert!(
            !fp.triangles.is_empty() && stats.lenient.is_empty(),
            "{stats:?}"
        ),
        Err(e) => panic!("{e}"),
    }
    let square = [polygon(
        &s,
        &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    )];
    match plan_or_fill(0, "spline", &s, &square, &rules, &size, 0.0, None) {
        Err(e) => assert!(
            e.to_string()
                .contains("the surface does not evaluate at [0.5, 0.5]"),
            "{e}"
        ),
        Ok((fp, _)) => panic!("meshed with {} triangles", fp.triangles.len()),
    }
}

/// A plan whose face has a vertex where the kernel's evaluator has no point (on
/// [`cylinder_without`]`(0.5)`, at u = 0.5): the evaluation refuses that face whole, naming the
/// vertex, and keeps the other face. Nothing stands in for the point.
#[test]
fn evaluating_a_vertex_the_surface_has_no_point_at_refuses_its_face() {
    use crate::{FacePlan, RefusedFace, TessPlan};
    let s = cylinder_without(0.5);
    let face = |face: u32, middle: [f64; 2]| FacePlan {
        face,
        surface: Some(0),
        uv: vec![[0.0, 0.0], [1.0, 0.0], middle],
        triangles: vec![[0, 1, 2]],
        ..FacePlan::default()
    };
    let plan = TessPlan {
        surfaces: vec![s],
        faces: vec![face(0, [0.25, 1.0]), face(1, [0.5, 1.0])],
        ..TessPlan::default()
    };
    let (m, refused) = plan.evaluate();
    assert_eq!(m.tri_face, vec![0]);
    assert_eq!(m.positions.len(), 3);
    assert_eq!(refused.len(), 1, "{refused:?}");
    let RefusedFace { face, reason } = &refused[0];
    assert_eq!(*face, 1);
    assert!(reason.starts_with("vertex 2 at [0.5, 1.0]: "), "{reason}");
}

/// The flips' check of a side evaluates the surface at the middles of the triangles either side
/// of it: one the surface has no point at (on [`cylinder_without`]`(0.5)`, a kite of four nodes
/// off u = 0.5, one of whose triangles has its middle on it) stops the flips with that point,
/// instead of taking the side as fine.
#[test]
fn the_flips_name_a_middle_the_surface_does_not_evaluate_at() {
    let s = cylinder_without(0.5);
    // The middle of the first triangle (its corners' mean) is at u = 0.5.
    let uv = vec![[0.0, 0.0], [0.75, -0.5], [0.75, 0.5], [0.0, 1.0]];
    let fixed: Vec<Vec3> = uv.iter().map(|q| point(&s, *q)).collect();
    let mut tris = vec![[0, 1, 2], [0, 2, 3]];
    match super::sag::flip(
        &s,
        0.01,
        &mut super::sag::Poles::new(&s, 1e-9),
        &fixed,
        &uv,
        &mut tris,
    ) {
        Err(super::sag::Stop::NoValue(NoValue(q))) => assert_eq!(q[0], 0.5, "{q:?}"),
        other => panic!("{other:?}, flipped to {tris:?}"),
    }
}

/// The parabolic cylinder z = u² (x = u, y = v) over [0, 1]², as a rational bicubic-by-quartic
/// whose weight (u − ½)² + (v − ½)² has its one root at (½, ½), where its numerators vanish too:
/// no point there (0 / 0), the parabola's own everywhere else. Its homogeneous coordinates are
/// 72 times the Bernstein coefficients, all whole numbers, so the root is exactly zero.
fn parabola_without_its_middle() -> EvalSurface {
    let ctrl: Vec<[f64; 4]> = [
        [0, 0, 0, 36],
        [9, 0, 0, 18],
        [6, 0, 6, 12],
        [9, 0, 0, 18],
        [36, 0, 36, 36],
        [0, 12, 0, 12],
        [3, 6, 0, -6],
        [-6, 4, 2, -12],
        [-9, 6, -12, -6],
        [12, 12, 12, 12],
        [0, 0, 0, 12],
        [3, -12, 0, -6],
        [-6, -16, 2, -12],
        [-9, -12, -12, -6],
        [12, 0, 12, 12],
        [0, 36, 0, 36],
        [9, 18, 0, 18],
        [6, 12, 6, 12],
        [9, 18, 0, 18],
        [36, 36, 36, 36],
    ]
    .iter()
    .map(|c| c.map(f64::from))
    .collect();
    let s = EvalSurface::nurbs(
        [4, 3],
        [
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        ],
        [5, 4],
        ctrl,
    );
    assert!(s.derivs(0.5, 0.5).is_none(), "no point at the middle");
    for q in [[0.3, 0.7], [0.5, 0.25], [0.9, 0.5], [0.0, 1.0], [0.53, 0.5]] {
        let p = point(&s, q);
        assert!(
            p.dist(Vec3::new(q[0], q[1], q[0] * q[0])) < 1e-12,
            "{q:?}: {p:?}"
        );
    }
    s
}

/// A division whose iso-line runs through the middle (½, ½) of [`parabola_without_its_middle`],
/// which is not the face's (`mine` leaves out a disc of radius 0.05 round it): each interval with
/// an end there is measured from the nearest point inwards that is the face's, so the parabola
/// is divided to the tolerance on either side. The interval was passed over, and the division
/// stopped at [0, ½, 1], its halves sagging about 0.03 against a tolerance of 0.01.
#[test]
fn an_interval_ending_at_a_point_outside_the_face_is_measured_from_inside_it() {
    let s = parabola_without_its_middle();
    let rules = Rules::new(0.01, 1.0, None);
    let outside = |q: [f64; 2]| (q[0] - 0.5).hypot(q[1] - 0.5) < 0.05;
    let mine = |q: [f64; 2]| Ok(!outside(q));
    let Ok(lines) = uv::divide(&s, 0, (0.0, 1.0), (0.5, 0.5), &rules, 10.0, 1000, &mine) else {
        panic!("divides")
    };
    assert!(lines.len() >= 6, "{lines:?}");
    // Every interval clear of the left-out disc sags at most the tolerance from its chord.
    let on = |u: f64| Vec3::new(u, 0.5, u * u);
    for w in lines.windows(2) {
        let (a, b) = (w[0], w[1]);
        if b <= 0.45 || a >= 0.55 {
            let sag = (1..16)
                .map(|k| on(a + (b - a) * f64::from(k) / 16.0).dist_to_segment(on(a), on(b)))
                .fold(0.0, f64::max);
            assert!(
                sag <= 0.01 * (1.0 + 1e-9),
                "[{a}, {b}] sags {sag}: {lines:?}"
            );
        }
    }
}

/// A strip of [`cylinder_without`]`(0.5)` running 5000 turns round it (u in [0, 1]): the
/// division meets the circle u = 0.5 the surface has no point on, and the strip's loops span
/// more periods than a point is looked for in, so whether the point is the face's cannot be told.
/// The face is refused for that, as a size sample is, not as though the point were inside it.
#[test]
fn a_division_point_too_many_periods_away_to_place_refuses_the_face_for_that() {
    let s = cylinder_without(0.5);
    let loops = [long_strip(&s)];
    let rules = Rules::new(0.01, 40.0, None);
    match plan_or_fill(0, "revolution", &s, &loops, &rules, &size(1.0), 0.0, None) {
        Err(e) => assert!(e.to_string().contains("periods, more than the 4096"), "{e}"),
        Ok((fp, _)) => panic!("meshed with {} triangles", fp.triangles.len()),
    }
}

/// A strip of `s` from u = 0 to 1 running 5000 turns round v, in steps of a quarter turn so it
/// does not wind.
fn long_strip(s: &EvalSurface) -> seam::Loop {
    let tau = std::f64::consts::TAU;
    let (turns, steps) = (5000.0, 20_000u32);
    let up = (0..steps).map(|k| [1.0, turns * tau * f64::from(k) / f64::from(steps)]);
    let down = (0..steps).map(|k| [0.0, turns * tau * f64::from(steps - k) / f64::from(steps)]);
    let uv: Vec<[f64; 2]> = std::iter::once([0.0, 0.0]).chain(up).chain(down).collect();
    let pts = uv.iter().map(|q| point(s, *q)).collect();
    let l = seam::Loop { uv, pts };
    assert!(!seam::winds(std::slice::from_ref(&l), 1, tau));
    l
}

/// The same strip, with a size sample the surface does not evaluate at: whether the sample lies
/// in the face cannot be told past 4096 periods, and the face is refused for that, naming the
/// sample, before any layout.
#[test]
fn a_size_sample_too_many_periods_away_to_place_refuses_the_face_for_that() {
    let s = cylinder_without(0.5);
    let loops = [long_strip(&s)];
    let rules = Rules::new(0.01, 40.0, None);
    let size = FaceSize {
        unevaluated: vec![[0.5, 0.3]],
        ..size(1.0)
    };
    match plan_or_fill(0, "revolution", &s, &loops, &rules, &size, 0.0, None) {
        Err(e) => assert!(
            e.to_string().ends_with(
                "periods, more than the 4096 a point the surface does not evaluate at ([0.5, 0.3]) is looked for in"
            ),
            "{e}"
        ),
        Ok((fp, _)) => panic!("meshed with {} triangles", fp.triangles.len()),
    }
}

/// The division along v of [`cylinder_without`]`(0.5)` (iso-lines across u, one at u = 0.5,
/// where the surface has no point), at a tolerance that leaves v in [0, 1] one interval, for a
/// face that leaves out v <= 0.05 and v >= 0.95: both ends of the interval at u = 0.5 are not
/// the face's, and the end at v = 0 steps inwards to v = 1/16, which is the face's and does not
/// evaluate. That is the face's refusal, naming the point; the steps were passed over, the
/// iso-line skipped, and the division never met the circle.
#[test]
fn a_step_inwards_the_surface_has_no_point_at_refuses_the_face() {
    let s = cylinder_without(0.5);
    let rules = Rules::new(1.0, 40.0, None);
    let mine = |q: [f64; 2]| Ok(q[1] > 0.05 && q[1] < 0.95);
    match uv::divide(&s, 1, (0.0, 1.0), (0.0, 1.0), &rules, 50.0, 1000, &mine) {
        Err(uv::Unmeasured::NoValue(NoValue(q))) => assert_eq!(q, [0.5, 0.0625]),
        other => panic!("{other:?}"),
    }
}

/// The sag a capped face reaches is measured where the surface evaluates: a triangle on
/// [`cylinder_without`]`(0.5)` whose middle lies on the circle the surface has no point on is
/// measured at its sides' middles only, and the face is not refused for a measure it never
/// needed (it was refused there); the middle is counted as unmeasured (it was dropped, and a
/// face over such a point could read as within its tolerance).
#[test]
fn the_worst_sag_is_measured_where_the_surface_evaluates() {
    let s = cylinder_without(0.5);
    let uv = [[0.0, 0.0], [0.75, -0.5], [0.75, 0.5]];
    let got = super::sag::worst(
        &s,
        &mut super::sag::Poles::new(&s, 1e-9),
        &[],
        &uv,
        &[[0, 1, 2]],
    );
    // Measured at its three sides' middles; its middle, unmeasured, is counted, so the face is
    // never taken as within its tolerance over it.
    assert!(
        got.as_ref()
            .is_ok_and(|(w, unmeasured)| w.is_finite() && *w > 0.0 && *unmeasured == 1),
        "{got:?}"
    );
}

/// A face whose surface's poles the kernel's classifier cannot tell: the plane x = u, y = v as a
/// bilinear net over u in [0, 2], whose last column (the span u in [1, 2], away from the face)
/// is not a number. The face, u in [0, 0.5], evaluates everywhere, but no point of it can be
/// classified (the net is not finite along u), so it is refused, named, with the classifier's
/// own reason; nothing takes "not a pole" for it.
#[test]
fn a_face_whose_poles_cannot_be_told_is_refused_with_the_classifiers_reason() {
    let nan = f64::NAN;
    let s = EvalSurface::nurbs(
        [1, 1],
        [vec![0.0, 0.0, 1.0, 2.0, 2.0], vec![0.0, 0.0, 1.0, 1.0]],
        [3, 2],
        vec![
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
            [nan, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [1.0, 1.0, 0.0, 1.0],
            [nan, 1.0, 0.0, 1.0],
        ],
    );
    let uv: Vec<[f64; 2]> = vec![[0.0, 0.0], [0.5, 0.0], [0.5, 1.0], [0.0, 1.0]];
    let pts: Vec<Vec3> = uv.iter().map(|q| point(&s, *q)).collect();
    let loops = [seam::Loop { uv, pts }];
    let rules = Rules::new(0.01, 2.0, None);
    let size = FaceSize {
        h: 1.0,
        speed: [1.0, 1.0],
        unevaluated: Vec::new(),
    };
    match plan_or_fill(3, "spline", &s, &loops, &rules, &size, 0.0, None) {
        Err(e) => {
            let why = e.to_string();
            assert!(
                why.contains("face 3 (spline): the surface's poles cannot be told at [0.0, 0.0]: pole classification: a control point is not finite"),
                "{why}"
            );
        }
        Ok((fp, _)) => panic!("meshed with {} triangles", fp.triangles.len()),
    }
}
