//! #2405 (from tangency-labs/solidcraft#10850 and tangency-labs/smlib#97): follow-ups of
//! the generic blend's corner work, each checked against a closed form.
//!
//! - The exact round classes' cylinder (`CalcLineSpineFilletGeom`) is built on its exact
//!   spine, the line the offsets meet in, and keeps that frame. The fork's 1.06e-12 on the
//!   oblique Ball came from the solved stations it took the centres from (the far one
//!   2e-12 off the spine here), and the analytic recognition of the patch
//!   (`CopyAndAddAnalytics` → `IsNurbSurfaceCone`, the end circles' `IsArc`) then refitted
//!   the frame from the net (7e-15 in the axis's direction, 2e-13 at C).
//! - A plane meets a torus in its exact circles with the plane first too (WR-721).
//! - A face made from exact pcurves keeps them (WR-722): the draft walls and `CopyFaces`
//!   pass them on, and `MakeFaceWithCurves`'s keep step matches them on a face of
//!   opposite orientation.
//! - A drop from a guess does not take a stalled local solve for the foot (WR-56's
//!   tight tolerance, the fork's eaa998a), shown on a case that reaches it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::math::{Point3, Vec3, BIG_DOUBLE};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

/// Every fix but `geometric_g2_continuity`, with or without `fillet_exact_round_classes`.
fn all_fixes(exact_classes: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity"
            && (exact_classes || name != "fillet_exact_round_classes")
        {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
}

fn unit(v: Vec3) -> Vec3 {
    v / v.length()
}

/// The edge of `b` nearest `at` (within 0.1).
fn pick(k: &Kernel, b: &Brep, at: Vec3) -> EdgeId {
    let mut best: Option<(EdgeId, f64)> = None;
    for e in get_edges(b).unwrap() {
        let Some(c) = edge::curve(b, e).unwrap() else {
            continue;
        };
        let ivl = edge::interval(b, e).unwrap();
        let d = c
            .drop_point(
                k,
                &ivl,
                at,
                None,
                BIG_DOUBLE,
                None,
                SolverOperation::Minimize,
            )
            .unwrap();
        if d.success && d.distance < 0.1 && best.is_none_or(|(_, bd)| d.distance < bd) {
            best = Some((e, d.distance));
        }
    }
    best.unwrap().0
}

/// Rounds the edges at `at` at radius `r` together.
fn round(k: &mut Kernel, b: &mut Brep, at: &[Vec3], r: f64) {
    let edges: Vec<EdgeId> = at.iter().map(|&p| pick(k, b, p)).collect();
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    let radii = vec![r; edges.len()];
    {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius(k, &edges, &radii, &xs, 1, 1.0)
            .unwrap();
        s.do_fillet(k).unwrap();
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
}

/// The prism swept `height` along `dir` from the closed polygon `pts`.
fn prism(k: &mut Kernel, pts: &[Point3], dir: Vec3, height: f64) -> Brep {
    let sides: Vec<Curve> = (0..pts.len())
        .map(|i| Curve::from(create_line_segment(pts[i], pts[(i + 1) % pts.len()]).unwrap()))
        .collect();
    let refs: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
    create_linear_sweep(k, &refs, dir, height, true).unwrap()
}

/// The box 400 wide on the far side (along `n`) of the plane through `at` with normal `n`.
fn half_space(k: &mut Kernel, at: Vec3, n: Vec3) -> Brep {
    let u = unit(n.cross(if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y }));
    let v = n.cross(u);
    let h = 200.0;
    let c = [
        at + (u + v) * h,
        at + (v - u) * h,
        at - (u + v) * h,
        at + (u - v) * h,
    ];
    prism(k, &c, n, 400.0)
}

/// The fork's capi `ball oblique_corner` (as `wr697_ball_corner`): three planes through
/// the origin, none square to another, with outward normals `n`, their edges `e` 20 long
/// and cut square at their far ends; the three edges at the origin rounded r 2. Returns
/// the rounded body, `n`, `e` and the corner ball's centre C (`n_i . C = -r`).
fn oblique_corner(k: &mut Kernel, rad: f64) -> (Brep, [Vec3; 3], [Vec3; 3], Vec3) {
    let len = 20.0;
    let n = [
        unit(Vec3::new(1.0, 0.2, 0.0)),
        unit(Vec3::new(0.0, 1.0, 0.2)),
        unit(Vec3::new(0.2, 0.0, 1.0)),
    ];
    let mut e = [Vec3::ZERO; 3];
    for i in 0..3 {
        e[i] = unit(n[i].cross(n[(i + 1) % 3]));
        if e[i].dot(n[(i + 2) % 3]) > 0.0 {
            e[i] = -e[i];
        }
    }
    let det = n[0].dot(n[1].cross(n[2]));
    let c = (n[1].cross(n[2]) + n[2].cross(n[0]) + n[0].cross(n[1])) * (-rad / det);
    let mut body = create_box(k, Point3::new(-40.0, -40.0, -40.0), 80.0, 80.0, 80.0).unwrap();
    for i in 0..3 {
        let hs = half_space(k, Vec3::ZERO, n[i]);
        body = boolean_difference(k, body, hs).unwrap();
        let hs = half_space(k, e[i] * len, e[i]);
        body = boolean_difference(k, body, hs).unwrap();
    }
    let points: Vec<Vec3> = e.iter().map(|&ei| ei * (len / 2.0)).collect();
    round(k, &mut body, &points, rad);
    (body, n, e, c)
}

/// The three rounds' cones of the oblique corner, each with the edge it rounds: the cone
/// whose axis is nearest that edge's direction.
fn round_cones(b: &Brep, e: &[Vec3; 3]) -> Vec<(usize, solidcore::surface::cone::Cone)> {
    let mut out = Vec::new();
    for f in get_faces(b).unwrap() {
        let s = face2::surface(b, f).unwrap().unwrap();
        if let SurfaceKind::Cone(cone) = s.kind() {
            let z = cone.rev().position().z_axis();
            let i = (0..3)
                .min_by(|&a, &bb| {
                    z.cross(e[a])
                        .length()
                        .partial_cmp(&z.cross(e[bb]).length())
                        .unwrap()
                })
                .unwrap();
            out.push((i, (**cone).clone()));
        }
    }
    out
}

/// Item 1: each round of the oblique corner is the exact cylinder of its spine. The ball
/// of radius r touching planes i and i + 1 rolls along their edge with its centre on the
/// offsets' line through C along `e_i = n_i x n_(i+1) / |n_i x n_(i+1)|`, so the round's
/// axis is that line and its radius r.
#[test]
fn the_exact_rounds_cylinder_is_on_its_exact_spine() {
    let mut k = all_fixes(true);
    let rad = 2.0;
    let (b, _n, e, c) = oblique_corner(&mut k, rad);
    let cones = round_cones(&b, &e);
    assert_eq!(cones.len(), 3, "three rounds, each a cone");
    let mut worst = (0.0f64, 0.0f64, 0.0f64);
    for (i, cone) in &cones {
        let frame = cone.rev().position();
        let z = frame.z_axis();
        // the axis's distance from C, its direction's from e_i, the radius's from r
        let off = (c - frame.origin()).cross(z).length();
        let dir = z.cross(e[*i]).length();
        let r = (cone.bot_radius() - rad).abs();
        eprintln!(
            "round {i}: axis {off:.3e} from C, {dir:.3e} from e; radius {r:.3e} off; semi-angle {}",
            cone.semi_angle_deg()
        );
        assert_eq!(cone.semi_angle_deg(), 0.0);
        worst = (worst.0.max(off), worst.1.max(dir), worst.2.max(r));
    }
    // to rounding: the direction to 1e-15, the axis's distance from C and the radius to
    // 1e-15 of the part's size (the frame's origin is the spine's far end, 28 from C,
    // whose coordinates round at 3.6e-15)
    let size = 48.66;
    assert!(
        worst.0 <= 1e-15 * size && worst.1 <= 1e-15 && worst.2 <= 1e-15 * size,
        "{worst:?}"
    );
}

/// The edges of `b` lying on the plane where `plane_dist` is zero (within 1e-9 at 33
/// points): whether each is a circle (an ellipse kind) and its worst distance from the
/// circles `circle_dist` measures.
fn plane_section_edges(
    k: &Kernel,
    b: &Brep,
    plane_dist: &dyn Fn(Vec3) -> f64,
    circle_dist: &dyn Fn(Vec3) -> f64,
) -> Vec<(bool, f64)> {
    use solidcore::curve::curve::CurveKind;
    let mut out = Vec::new();
    for e in get_edges(b).unwrap() {
        let c = edge::curve(b, e).unwrap().unwrap();
        let ivl = edge::interval(b, e).unwrap();
        let pts: Vec<Vec3> = (0..=32)
            .map(|t| {
                c.evaluate_point(k, ivl.min() + (ivl.max() - ivl.min()) * f64::from(t) / 32.0)
                    .unwrap()
            })
            .collect();
        if pts.iter().all(|&p| plane_dist(p).abs() <= 1e-9) {
            let off = pts.iter().map(|&p| circle_dist(p)).fold(0.0, f64::max);
            out.push((matches!(c.kind(), CurveKind::Ellipse(_)), off));
        }
    }
    out
}

/// A torus R 10, r 3 about the z axis at the origin met by a box in either order: the box's
/// face z = 0 (square to the axis, through the centre) in the circles of radii 7 and 13,
/// its face x = 0 (through the axis) in the meridians about (0, +-10, 0) of radius 3.
/// Returns the section edges on that face for each order (box first, then torus first).
fn torus_box_sections(k: &mut Kernel, through_axis: bool) -> [Vec<(bool, f64)>; 2] {
    use solidcore::boolean::api_brep::boolean_intersection;
    use solidcore::primitive::api_primitives::create_torus;
    let plane: fn(Vec3) -> f64 = if through_axis { |p| p.x } else { |p| p.z };
    let circles: fn(Vec3) -> f64 = if through_axis {
        |p| (((p.y.abs() - 10.0).powi(2) + p.z * p.z).sqrt() - 3.0).abs()
    } else {
        |p| {
            let rho = (p.x * p.x + p.y * p.y).sqrt();
            (rho - 7.0).abs().min((rho - 13.0).abs())
        }
    };
    [true, false].map(|box_first| {
        let t = create_torus(k, Point3::new(0.0, 0.0, 0.0), 10.0, 3.0).unwrap();
        let bx = if through_axis {
            create_box(k, Point3::new(0.0, -20.0, -20.0), 20.0, 40.0, 40.0)
        } else {
            create_box(k, Point3::new(-20.0, -20.0, -20.0), 40.0, 40.0, 20.0)
        }
        .unwrap();
        let r = if box_first {
            boolean_intersection(k, bx, t)
        } else {
            boolean_intersection(k, t, bx)
        }
        .unwrap();
        plane_section_edges(k, &r, &plane, &circles)
    })
}

/// Item 2 (WR-721): a plane meets a torus in its exact circles in either order, where the
/// plane is square to the axis or through it, and each is a circle. Upstream's plane first
/// has no torus case and marches the section: 1.36e-6 off the circles, 3.46e-6 off the
/// meridians.
#[test]
fn a_plane_meets_a_torus_in_its_exact_circles_in_either_order() {
    let mut k = Kernel::default();
    for through_axis in [false, true] {
        for (order, edges) in torus_box_sections(&mut k, through_axis).iter().enumerate() {
            eprintln!("through axis {through_axis}, order {order}: {edges:?}");
            assert_eq!(edges.len(), 2, "two circles");
            for &(circle, off) in edges {
                assert!(circle && off <= 1e-14, "{edges:?}");
            }
        }
    }
}

/// The UV trim curves stored on the edge uses of face `f`'s upward face use in `b` (`None`
/// for a use without one), each as: how far it is from the nearest side of the face's
/// domain (the largest difference of the coordinate across that side at 17 points: 0.0
/// on an iso line exactly), and the worst distance of its points lifted onto the surface
/// from the edge's curve.
fn stored_pcurves(k: &Kernel, b: &Brep, f: solidcore::topology::FaceId) -> Vec<Option<(f64, f64)>> {
    use solidcore::topology::edgeuse::p1::{
        faceuse_edgeuses, uv_trim_curve as edgeuse_uv_trim_curve,
    };
    let s = face2::surface(b, f).unwrap().unwrap();
    let dom = face2::uv_domain(b, f).unwrap();
    let fu = face2::upward_faceuse(b, f).unwrap().unwrap();
    let mut out = Vec::new();
    for eu in faceuse_edgeuses(b, fu, false, None).unwrap() {
        let Some(c) = edgeuse_uv_trim_curve(b, eu).unwrap().0.cloned() else {
            out.push(None);
            continue;
        };
        let e = b.edgeuse(eu).unwrap().edge.unwrap();
        let curve = edge::curve(b, e).unwrap().unwrap();
        let eivl = edge::interval(b, e).unwrap();
        let ivl = c.natural_interval();
        let pts: Vec<_> = (0..=16)
            .map(|t| {
                c.evaluate_point(k, ivl.min() + (ivl.max() - ivl.min()) * f64::from(t) / 16.0)
                    .unwrap()
            })
            .collect();
        let dev = |f: &dyn Fn(&Point3) -> f64| pts.iter().map(f).fold(0.0f64, f64::max);
        let iso = [
            dev(&|p| (p.x - dom.min().x).abs()),
            dev(&|p| (p.x - dom.max().x).abs()),
            dev(&|p| (p.y - dom.min().y).abs()),
            dev(&|p| (p.y - dom.max().y).abs()),
        ]
        .into_iter()
        .fold(f64::INFINITY, f64::min);
        let mut off = 0.0f64;
        for p in &pts {
            let q = s
                .evaluate_point(k, solidcore::math::Point2::new(p.x, p.y))
                .unwrap();
            let d = curve
                .drop_point(
                    k,
                    &eivl,
                    q,
                    None,
                    BIG_DOUBLE,
                    None,
                    SolverOperation::Minimize,
                )
                .unwrap();
            off = off.max(d.distance);
        }
        out.push(Some((iso, off)));
    }
    out
}

/// Item 5 (WR-722): with `keep_exact_pcurves`, a draft extrusion's cone wall keeps the
/// exact seam pcurves its construction made. The half disc of radius 20 (its diameter on
/// the x axis), drafted 10 degrees in over a height of 10 with the curvature-keeping
/// corners, has its arc's wall on the cone `rho = 20 - z tan 10`, and every edge of that
/// face is on it in closed form (to 1e-13). With the flag each of the face's four pcurves
/// lifts onto its edge to 1e-12. Without it (upstream) the walls' `MakeFaceWithCurves`
/// and `CopyFaces` drop them and the seams' are projected again, 2.37e-6 off their edges.
#[test]
fn a_draft_cone_wall_keeps_its_exact_seam_pcurves() {
    use solidcore::curve::ellipse::Ellipse;
    use solidcore::curve::line::Line;
    use solidcore::sweep::api_brep::{
        create_taper_extrude_with_corner, OffsetCornerType, TaperExtrude,
    };
    let (tan, cos) = (10.0f64.to_radians().tan(), 10.0f64.to_radians().cos());
    for keep in [true, false] {
        let mut k = Kernel::default();
        k.fixes.keep_exact_pcurves = keep;
        let p = |x: f64, y: f64| Vec3::new(x, y, 0.0);
        let line = Line::create_line_segment(3, p(-20.0, 0.0), p(20.0, 0.0), None).unwrap();
        let arc =
            Ellipse::circle_from_3_points(&k, p(20.0, 0.0), p(0.0, 20.0), p(-20.0, 0.0), 3, false)
                .unwrap();
        let curves = [Curve::from(line), Curve::from(arc)];
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        let made = create_taper_extrude_with_corner(
            &mut k,
            &refs,
            10.0,
            10.0,
            3,
            OffsetCornerType::CurveExtension,
        )
        .unwrap();
        let TaperExtrude::Solid(b) = made else {
            panic!("{made:?}")
        };
        let cones: Vec<_> = get_faces(&b)
            .unwrap()
            .into_iter()
            .filter(|&f| {
                matches!(
                    face2::surface(&b, f).unwrap().unwrap().kind(),
                    SurfaceKind::Cone(_)
                )
            })
            .collect();
        assert_eq!(cones.len(), 1);
        let wall = cones[0];
        // its edges on the cone in closed form
        let mut on_cone = 0.0f64;
        for e in solidcore::topology::topology_traverser::face_edges(&b, wall).unwrap() {
            let c = edge::curve(&b, e).unwrap().unwrap();
            let ivl = edge::interval(&b, e).unwrap();
            for i in 0..=32 {
                let q = c
                    .evaluate_point(
                        &k,
                        ivl.min() + (ivl.max() - ivl.min()) * f64::from(i) / 32.0,
                    )
                    .unwrap();
                let rho = (q.x * q.x + q.y * q.y).sqrt();
                on_cone = on_cone.max(((rho - (20.0 - q.z * tan)) * cos).abs());
            }
        }
        assert!(on_cone <= 1e-13, "{on_cone:e}");
        let pcurves = stored_pcurves(&k, &b, wall);
        eprintln!("keep {keep}: {pcurves:?}");
        assert_eq!(pcurves.len(), 4);
        let worst = pcurves.iter().map(|q| q.unwrap().1).fold(0.0f64, f64::max);
        if keep {
            assert!(worst <= 1e-12, "{worst:e}");
        } else {
            // the projected seams (the behaviour the flag replaces)
            assert!(
                (worst - 2.371_850_463_194_937e-6).abs() <= 1e-12,
                "{worst:e}"
            );
        }
    }
}

/// Item 5 (WR-722), the keep step on its own: a face `MakeFaceWithCurves` makes with its
/// surface's orientation opposite, from the four sides of part of a cylinder's domain
/// (r 4, h 10) given clockwise in `(u, v)` with their exact iso lines (`pbOptCrvOnSurf`).
/// With the flag every edge use keeps its iso line: exactly on its side of the domain,
/// on its edge to 1e-12. Without it (upstream) the orientations are matched against the
/// upward face use's edge uses, which run the other way on this face: `sm_FindBestUVCurve`'s
/// tangent test refuses every given curve and each is projected again, off its iso line.
#[test]
fn an_opposite_face_keeps_the_pcurves_it_is_given() {
    use solidcore::geom::extent2d::Extent2d;
    use solidcore::geom::tol::USE_DEFAULT;
    use solidcore::nurbs::b_spline_surface::SurfParamType;
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::primitive::api_primitives::create_cylinder_surface;
    use solidcore::surface::surface::Surface;
    use solidcore::topology::brep::p2::ArenaNotifyState;
    use solidcore::topology::brep::p3::new_brep;
    use solidcore::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
    use solidcore::topology::brep_cache::{arena_identity, CacheObject};
    for keep in [true, false] {
        let mut k = Kernel::default();
        k.fixes.keep_exact_pcurves = keep;
        let surface = Surface::from(
            create_cylinder_surface(&k, Point3::new(1.0, 2.0, 3.0), 4.0, 10.0).unwrap(),
        );
        let nat = surface.natural_uv_domain_with_nurb(&k).unwrap();
        let domain = Extent2d::from_bounds(
            nat.min().x + 0.1 * nat.x_length(),
            nat.min().y + 0.2 * nat.y_length(),
            nat.min().x + 0.6 * nat.x_length(),
            nat.min().y + 0.9 * nat.y_length(),
        );
        // the counter-clockwise loop, each UV line parameterised as its curve, then
        // reversed: clockwise, each side's orientation flipped
        let natural = surface
            .create_natural_uv_trim_curves(&k, &domain, SurfParamType::U, false)
            .unwrap();
        let mut curves_3d = Vec::new();
        let mut uv_curves = Vec::new();
        let mut orientations = Vec::new();
        for ((c, mut uv), o) in natural
            .curves_3d
            .into_iter()
            .zip(natural.uv_curves)
            .zip(natural.orients)
            .rev()
        {
            uv.edit_parameterization(&c.natural_interval()).unwrap();
            curves_3d.push(Some(c));
            uv_curves.push(Curve::from(uv));
            orientations.push(if o == OrientType::Same {
                OrientType::Opposite
            } else {
                OrientType::Same
            });
        }
        let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
        let f = {
            let mut state = ArenaNotifyState::new(&b);
            let mut env = state.env(None);
            let this_face = |face| arena_identity(CacheObject::Face(face));
            let input = FaceCurvesInput {
                loops: vec![4],
                curves_3d: Some(curves_3d),
                uv_curves: Some(uv_curves),
                orientations,
                loop_points: Vec::new(),
            };
            make_face_with_curves(
                &mut k,
                &mut b,
                None,
                input,
                surface,
                &domain,
                OrientType::Opposite,
                Some(true),
                &this_face,
                &mut env,
            )
            .unwrap()
            .face
            .unwrap()
        };
        let pcurves = stored_pcurves(&k, &b, f);
        eprintln!("keep {keep}: {pcurves:?}");
        assert_eq!(pcurves.len(), 4);
        for q in &pcurves {
            let (iso, off) = q.unwrap();
            assert!(off <= 1e-12, "{pcurves:?}");
            if keep {
                assert!(iso == 0.0, "{pcurves:?}");
            }
        }
        if !keep {
            assert!(pcurves.iter().any(|q| q.unwrap().0 > 0.0), "{pcurves:?}");
        }
    }
}

/// Item 6: `SmSurface::DropPoint` from a guess takes a stalled local solve for the foot
/// without `good_drop_result_distances_in_order` (WR-56's fork eaa998a). On a wavy bicubic
/// (an 8 x 8 net over 10 x 10, its heights alternating +-1), the point 3 below
/// `S(0.1, 0.8)` along the normal, dropped from the guess (0.3, 0.9): `LocalPointSolve_1`
/// stops without converging (unable to go on) 1e-8 off a stationary point that is not the
/// foot, its gap's components along the tangents summing to 7.7e-9. That is under
/// upstream's 1e-4, so upstream returns it, at 2.99999944 from the point. With the flag
/// only a foot within the local solve's own tolerance `EFF_ZERO (1 + max|P|)` is taken, so
/// the drop goes on to the global solve and returns the foot, 4.5e-5 nearer (its gap
/// 1.4e-8 off the normal, the global solve's own convergence): no point of a 201 x 201
/// sampling of the surface is nearer.
#[test]
fn a_drop_from_a_guess_does_not_take_a_stalled_local_solve() {
    use solidcore::geom::extent2d::Extent2d;
    use solidcore::geom::local_solve_nd::TerminationReason;
    use solidcore::math::{Point2, EFF_ZERO};
    use solidcore::nurbs::b_spline_surface::BSplineSurface;
    use solidcore::nurbs::math::{CPoint, NOW};
    use solidcore::surface::surface::p4::NoObject;
    use solidcore::surface::surface::p5::DropPointOptions;
    use solidcore::surface::surface::Surface;
    let net: Vec<Vec<CPoint>> = (0..8u32)
        .map(|i| {
            (0..8u32)
                .map(|j| {
                    let z = if (i + j) % 2 == 0 { 1.0 } else { -1.0 };
                    CPoint::new(10.0 * f64::from(i) / 7.0, 10.0 * f64::from(j) / 7.0, z, NOW)
                })
                .collect()
        })
        .collect();
    let knots = [0.0, 0.0, 0.0, 0.0, 0.2, 0.4, 0.6, 0.8, 1.0, 1.0, 1.0, 1.0];
    let s =
        Surface::from(BSplineSurface::from_data(7, 7, 3, 3, 11, 11, &net, &knots, &knots).unwrap());
    let dom = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
    let k = Kernel::default();
    let (q, su, sv) = s
        .evaluate_1st_derivatives(&k, Point2::new(0.1, 0.8), true, true)
        .unwrap();
    let n = unit(su.cross(sv));
    let p = q - 3.0 * n;
    let guess = Point2::new(0.3, 0.9);
    let tight = EFF_ZERO * (1.0 + p.max_dimension());
    // |Fu| + |Fv|, the gap's components along the unit tangents at `uv`
    let gap_along = |uv: Point2| {
        let (sp, su, sv) = s.evaluate_1st_derivatives(&k, uv, true, true).unwrap();
        let gap = p - sp;
        gap.dot(unit(su)).abs() + gap.dot(unit(sv)).abs()
    };

    // the local solve stalls
    let local = s
        .local_point_solve_1::<NoObject>(&k, &dom, SolverOperation::Minimize, p, guess, None)
        .unwrap();
    assert!(local.found && local.reason == TerminationReason::UnableToConverge);
    let stalled = Point2::new(
        local.solution.start.parameters[0],
        local.solution.start.parameters[1],
    );
    let along = gap_along(stalled);
    eprintln!(
        "stalled at {stalled:?} after {}, |Fu| + |Fv| {along:e}",
        local.iterations
    );
    assert!(along > 100.0 * tight && along < 1e-4, "{along:e}");

    let drop = |flag: bool| {
        let mut k = Kernel::default();
        k.fixes.good_drop_result_distances_in_order = flag;
        let opts = DropPointOptions {
            guess_uv: Some(guess),
            ..DropPointOptions::default()
        };
        let d = s.drop_point::<NoObject>(&k, p, &dom, &opts).unwrap();
        assert!(d.success);
        d
    };
    // upstream: the stalled result is the foot
    let up = drop(false);
    eprintln!("upstream {:?} at {}", up.result_uv, up.gap);
    assert_eq!(up.result_uv, stalled);
    // with the flag: the global solve's foot, nearer
    let fixed = drop(true);
    eprintln!("with the flag {:?} at {}", fixed.result_uv, fixed.gap);
    assert!(
        gap_along(fixed.result_uv) < 1e-7,
        "{:e}",
        gap_along(fixed.result_uv)
    );
    assert!(up.gap - fixed.gap > 4e-5, "{} {}", up.gap, fixed.gap);
    let mut nearest = f64::INFINITY;
    for i in 0..=200 {
        for j in 0..=200 {
            let uv = Point2::new(f64::from(i) / 200.0, f64::from(j) / 200.0);
            nearest = nearest.min(s.evaluate_point(&k, uv).unwrap().distance(p));
        }
    }
    assert!(nearest >= fixed.gap - 1e-12, "{nearest} {}", fixed.gap);
}
