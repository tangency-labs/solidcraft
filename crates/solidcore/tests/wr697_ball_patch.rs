//! WR-697 follow-up (the fork's tangency-labs/smlib@f8e6709, smlib#79 step 4c; and
//! efb8007's `SmBallPatch::IsOnSphere`): the blend corner's Ball makes its face an
//! `SmBallPatch`, the rational patch carrying the sphere it is exactly on as its
//! definition (`SurfaceKind::BallPatch`, `SURF_BASE_TYPE + 42`). Its offset is the net
//! scaled about the centre, exact on the concentric sphere; it collapses where the radius
//! would not stay positive; it moves rigidly or by a uniform scale only; and the file
//! writes the centre and radius, then the net, and reads them back only under
//! `fillet_blend_corner`.
//!
//! The box corner is the fork's capi `ball box_corner` case (5e8c9c1); the offsets follow
//! the fork's capi shell case 5eb43a1 (a box rounded r 3, shelled 2 in: each corner face
//! on the sphere of radius 3 about its ball's centre and its offset on the concentric one
//! of radius 1), checked here in closed form on the patch itself.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::error::SolidError;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::math::{Point3, Vec3, BIG_DOUBLE};
use solidcore::nurbs::b_spline_curve::p2::ControlPointForm;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::surface::ball_patch::{BallPatch, BALL_PATCH_TYPE};
use solidcore::surface::surface::p2::{SurfaceKind, B_SPLINE_SURFACE_TYPE};
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::database_io::{DatabaseReader, DatabaseWriter, FileType};
use solidcore::topology::face::p2 as face2;
use solidcore::topology::geometry_write::write_surface;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

/// Every fix but `geometric_g2_continuity`, with or without `fillet_blend_corner`.
fn all_fixes(blend: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" && (blend || name != "fillet_blend_corner") {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
}

fn pick(k: &Kernel, b: &Brep, at: [f64; 3]) -> EdgeId {
    let p = Point3::new(at[0], at[1], at[2]);
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
                p,
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

/// capi `ball box_corner`: a 40 x 30 x 20 box, the three edges at its corner (40, 30, 20)
/// rounded r 3 (the ball about (37, 27, 17)); every face's surface.
fn box_corner(blend: bool) -> (Kernel, Vec<Surface>) {
    let mut k = all_fixes(blend);
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let at = [[20.0, 30.0, 20.0], [40.0, 15.0, 20.0], [40.0, 30.0, 10.0]];
    let edges: Vec<EdgeId> = at.iter().map(|&p| pick(&k, &b, p)).collect();
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(&mut k, &edges, &[3.0; 3], &xs, 1, 1.0)
            .unwrap();
        s.do_fillet(&mut k).unwrap();
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let surfaces = get_faces(&b)
        .unwrap()
        .into_iter()
        .map(|f| face2::surface(&b, f).unwrap().unwrap().copy(&k).unwrap())
        .collect();
    (k, surfaces)
}

const CENTRE: Point3 = Point3::new(37.0, 27.0, 17.0);

/// The box corner's Ball face.
fn ball_face() -> (Kernel, Surface) {
    let (k, surfaces) = box_corner(true);
    let mut balls: Vec<Surface> = surfaces
        .into_iter()
        .filter(|s| matches!(s.kind(), SurfaceKind::BallPatch(_)))
        .collect();
    assert_eq!(balls.len(), 1, "one ball patch");
    (k, balls.remove(0))
}

fn patch(s: &Surface) -> &BallPatch {
    let SurfaceKind::BallPatch(p) = s.kind() else {
        panic!("a ball patch, got {:?}", s.kind());
    };
    p
}

/// The worst distance off the sphere about `c` of radius `r` over a 17 x 17 grid of the
/// surface's natural domain.
fn off_sphere(k: &Kernel, s: &Surface, c: Point3, r: f64) -> f64 {
    let dom = s.natural_uv_domain_with_nurb(k).unwrap();
    let mut worst = 0.0f64;
    for i in 0..=16 {
        for j in 0..=16 {
            let p = s
                .evaluate_point(k, dom.evaluate(f64::from(i) / 16.0, f64::from(j) / 16.0))
                .unwrap();
            worst = worst.max(((p - c).length() - r).abs());
        }
    }
    worst
}

/// The Ball's face is an `SmBallPatch` on the corner ball's sphere (closed form: centre
/// r in from each face, radius r), its net certified on it by `IsOnSphere` and not on a
/// sphere 1e-9 larger; the type is `SURF_BASE_TYPE + 42`, an `SmBSplineSurface` kind.
#[test]
fn the_ball_s_face_is_a_ball_patch_on_its_sphere() {
    let (k, s) = ball_face();
    let p = patch(&s);
    assert_eq!(p.centre(), CENTRE);
    assert_eq!(p.radius(), 3.0);
    assert_eq!(s.type_code(), BALL_PATCH_TYPE);
    assert_eq!(BALL_PATCH_TYPE, 15042);
    assert!(s.is_kind_of(BALL_PATCH_TYPE).unwrap());
    assert!(s.is_kind_of(B_SPLINE_SURFACE_TYPE).unwrap());
    assert!(off_sphere(&k, &s, CENTRE, 3.0) <= 1e-12);
    assert!(BallPatch::is_on_sphere(p.net(), CENTRE, 3.0, 1e-12).unwrap());
    assert!(!BallPatch::is_on_sphere(p.net(), CENTRE, 3.0 + 1e-9, 1e-12).unwrap());
    assert!(
        !BallPatch::is_on_sphere(p.net(), CENTRE + Vec3::new(1e-9, 0.0, 0.0), 3.0, 1e-12).unwrap()
    );
    // a net cut by inserted knots is still certified
    let mut cut = p.net().copy();
    let dom = cut.natural_uv_domain();
    cut.insert_knots(
        solidcore::nurbs::b_spline_surface::SurfParamType::U,
        &[dom.evaluate(0.3, 0.0).x, dom.evaluate(0.7, 0.0).x],
    )
    .unwrap();
    assert!(BallPatch::is_on_sphere(&cut, CENTRE, 3.0, 1e-12).unwrap());
    assert!(!BallPatch::is_on_sphere(&cut, CENTRE, 3.0 * (1.0 + 1e-9), 1e-12).unwrap());
}

/// Without the flag there is no Ball and no ball patch (the pinned corner).
#[test]
fn without_the_flag_there_is_no_ball_patch() {
    let (_, surfaces) = box_corner(false);
    assert!(surfaces
        .iter()
        .all(|s| !matches!(s.kind(), SurfaceKind::BallPatch(_))));
}

/// The offsets stay concentric (the fork's capi shell case 5eb43a1: radius 3, the inward
/// offset by 2 on radius 1): exact, a ball patch again, its net on the concentric sphere;
/// 3 in collapses, as the shell's thicker-than-the-ball offsets must.
#[test]
fn an_offset_stays_concentric() {
    let (k, s) = ball_face();
    // the convex corner's normal points away from the centre: an inward offset is negative
    for (d, want) in [(-2.0, 1.0), (2.0, 5.0), (-2.999, 0.001)] {
        let off = s.create_offset_surface(&k, d, 1e-6).unwrap().unwrap();
        let q = patch(&off);
        assert_eq!(q.centre(), CENTRE);
        assert!((q.radius() - want).abs() <= 1e-15, "{d}: {}", q.radius());
        assert!(
            off_sphere(&k, &off, CENTRE, want) <= 1e-12 * want.max(1.0),
            "{d}"
        );
        assert!(BallPatch::is_on_sphere(q.net(), CENTRE, want, 1e-12).unwrap());
        // the weights are unchanged
        assert_eq!(
            q.net().control_point_net().unwrap().weights,
            patch(&s).net().control_point_net().unwrap().weights
        );
        assert!(!s.offset_collapses(&k, d, 1e-6).unwrap());
    }
    for d in [-3.0, -3.5] {
        assert!(s.offset_collapses(&k, d, 1e-6).unwrap(), "{d}");
        assert!(matches!(
            s.create_offset_surface(&k, d, 1e-6),
            Err(SolidError::InvalidInput { .. })
        ));
    }
}

/// Transforms move the sphere with the net: rigid and uniformly scaled; a non-uniform
/// scale is refused.
#[test]
fn a_transform_moves_the_sphere() {
    let (k, s) = ball_face();
    let mut moved = s.copy(&k).unwrap();
    let place = Axis2Placement::new(
        Point3::new(1.0, -2.0, 0.5),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
    );
    moved
        .transform(&k, &place, Some(Vec3::new(2.0, 2.0, 2.0)))
        .unwrap();
    let q = patch(&moved);
    // scaled about the origin by 2, then x -> y, y -> -x, moved by (1, -2, 0.5)
    let want = Point3::new(1.0 - 54.0, -2.0 + 74.0, 0.5 + 34.0);
    assert!(q.centre().distance(want) <= 1e-12, "{:?}", q.centre());
    assert_eq!(q.radius(), 6.0);
    assert!(off_sphere(&k, &moved, q.centre(), 6.0) <= 1e-11);
    let mut bad = s.copy(&k).unwrap();
    assert!(bad
        .transform(
            &k,
            &Axis2Placement::default(),
            Some(Vec3::new(1.0, 2.0, 1.0))
        )
        .is_err());
}

/// The record round-trips bit for bit (binary; ASCII to its precision) under the flag,
/// is refused without it, and a hostile radius or centre is refused.
#[test]
fn a_file_round_trip_is_bit_for_bit() {
    let (k, s) = ball_face();
    let p = patch(&s);
    for ft in [FileType::Binary, FileType::Ascii] {
        let mut db = DatabaseWriter::open_for_write(ft);
        write_surface(&s, &mut db, 30).unwrap();
        let bytes = db.into_bytes();
        let mut r = DatabaseReader::new();
        r.open_for_read(bytes.clone(), ft);
        let back = Surface::read_from_db(&k, BALL_PATCH_TYPE, &mut r, 30).unwrap();
        let q = patch(&back);
        if ft == FileType::Binary {
            assert_eq!(q.centre().x.to_bits(), p.centre().x.to_bits());
            assert_eq!(q.centre().y.to_bits(), p.centre().y.to_bits());
            assert_eq!(q.centre().z.to_bits(), p.centre().z.to_bits());
            assert_eq!(q.radius().to_bits(), p.radius().to_bits());
            let (a, b) = (
                p.net().control_point_net().unwrap(),
                q.net().control_point_net().unwrap(),
            );
            assert_eq!(a.points.len(), b.points.len());
            for (x, y) in a.points.iter().zip(&b.points) {
                assert_eq!(
                    [x.x.to_bits(), x.y.to_bits(), x.z.to_bits()],
                    [y.x.to_bits(), y.y.to_bits(), y.z.to_bits()]
                );
            }
            for (x, y) in a.weights.iter().zip(&b.weights) {
                assert_eq!(x.to_bits(), y.to_bits());
            }
            assert!(back.approx_eq(&k, &s).unwrap());
            // written again, the same bytes
            let mut again = DatabaseWriter::open_for_write(ft);
            write_surface(&back, &mut again, 30).unwrap();
            assert_eq!(again.into_bytes(), bytes);
        } else {
            assert!(q.centre().distance(p.centre()) <= 1e-9);
            assert!((q.radius() - p.radius()).abs() <= 1e-9);
        }
        // without the flag the type is refused, as the pinned reader refuses it
        let off = Kernel::default();
        let mut r = DatabaseReader::new();
        r.open_for_read(bytes.clone(), ft);
        assert!(Surface::read_from_db(&off, BALL_PATCH_TYPE, &mut r, 30).is_err());
    }

    // hostile definitions: the record opens with the centre and the radius (4 doubles)
    let mut db = DatabaseWriter::open_for_write(FileType::Binary);
    write_surface(&s, &mut db, 30).unwrap();
    let good = db.into_bytes();
    for (at, value) in [
        (24, 0.0_f64),
        (24, -3.0),
        (24, f64::NAN),
        (24, f64::INFINITY),
        (0, f64::NAN),
        (16, f64::NEG_INFINITY),
    ] {
        let mut bad = good.clone();
        bad[at..at + 8].copy_from_slice(&value.to_le_bytes());
        let mut r = DatabaseReader::new();
        r.open_for_read(bad, FileType::Binary);
        assert!(matches!(
            Surface::read_from_db(&k, BALL_PATCH_TYPE, &mut r, 30),
            Err(SolidError::InvalidInput { .. })
        ));
    }
    // a truncated record
    for len in [0, 20, good.len() / 2, good.len() - 1] {
        let mut r = DatabaseReader::new();
        r.open_for_read(good[..len].to_vec(), FileType::Binary);
        assert!(Surface::read_from_db(&k, BALL_PATCH_TYPE, &mut r, 30).is_err());
    }
}

/// `SmBallPatch::Create` refuses a bad patch: a radius not finite and positive, a centre
/// not finite, a net that is not rational, a weight that is not positive.
#[test]
fn a_bad_patch_is_refused() {
    let (_, s) = ball_face();
    let net = patch(&s).net();
    let refused =
        |r: solidcore::Result<BallPatch>| matches!(r, Err(SolidError::InvalidInput { .. }));
    assert!(BallPatch::create(net, CENTRE, 3.0).is_ok());
    for radius in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(refused(BallPatch::create(net, CENTRE, radius)), "{radius}");
    }
    assert!(refused(BallPatch::create(
        net,
        Point3::new(f64::NAN, 0.0, 0.0),
        3.0
    )));
    // a non-rational net (the bilinear unit square)
    let flat = BSplineSurface::create_bilinear_surface(
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
    )
    .unwrap();
    assert!(!flat.is_rational());
    assert!(refused(BallPatch::create(&flat, CENTRE, 3.0)));
    assert!(!BallPatch::is_on_sphere(&flat, CENTRE, 3.0, 1e-12).unwrap());
    // a weight not positive
    let cpn = net.control_point_net().unwrap();
    for w in [0.0, -0.5] {
        let mut bad = net.copy();
        bad.set_control_point(
            ControlPointForm::EuclideanRational,
            1,
            1,
            cpn.points[cpn.v_count + 1],
            w,
        )
        .unwrap();
        assert!(refused(BallPatch::create(&bad, CENTRE, 3.0)), "{w}");
    }
}

/// The fork's capi shell case (5eb43a1): a 30 x 20 x 10 box with all twelve edges rounded
/// r 3, shelled 2 in (closed, thicker than the ball's wall). At each of the eight corners
/// one face on the sphere of radius 3 about its ball's centre (r in from each face) and
/// one, its offset, on the concentric sphere of radius 1, each face's surface over the
/// face (the box of its edges' UV curves: a shell's faces sit on surfaces extended past
/// them) on its sphere to 1e-12.
#[test]
fn the_shelled_box_keeps_its_corners_on_concentric_spheres() {
    use solidcore::offset::offset_executive::p2::{shell_brep_simple, OffsetOptions, ShellOptions};
    use solidcore::topology::edgeuse::p1 as eu1;
    use solidcore::topology::topology_traverser::face_edges;
    let (a, b, c, rad, inner) = (30.0, 20.0, 10.0, 3.0, 1.0);
    let mut k = all_fixes(true);
    let mut part = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), a, b, c).unwrap();
    let edges = get_edges(&part).unwrap();
    assert_eq!(edges.len(), 12);
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    {
        let mut s = FilletSession::new(&mut k, &mut part).unwrap();
        s.prepare_per_edge_constant_radius(&mut k, &edges, &[rad; 12], &xs, 1, 1.0)
            .unwrap();
        s.do_fillet(&mut k).unwrap();
    }
    stitch_and_orient(&mut k, &mut part, &StitchAndOrientOptions::default()).unwrap();
    let options = ShellOptions {
        offset: OffsetOptions {
            distance: -2.0,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid: true,
        merge_sheet_side_walls: false,
    };
    let shell = shell_brep_simple(&mut k, &part, options, &[]).unwrap();
    let mut found = [0usize; 2];
    let mut worst = 0.0f64;
    for f in get_faces(&shell).unwrap() {
        let s = face2::surface(&shell, f).unwrap().unwrap();
        let fe = face_edges(&shell, f).unwrap();
        let mut corners: Vec<Point3> = Vec::new();
        for &e in &fe {
            let curve = edge::curve(&shell, e).unwrap().unwrap();
            let ivl = edge::interval(&shell, e).unwrap();
            for t in [ivl.min(), ivl.max()] {
                let p = curve.evaluate_point(&k, t).unwrap();
                if !corners.iter().any(|q| q.distance(p) <= 1e-9) {
                    corners.push(p);
                }
            }
        }
        if corners.len() != 3 {
            continue;
        }
        for corner in 0..8 {
            let centre = Point3::new(
                if corner & 1 != 0 { a - rad } else { rad },
                if corner & 2 != 0 { b - rad } else { rad },
                if corner & 4 != 0 { c - rad } else { rad },
            );
            for (kk, want) in [rad, inner].into_iter().enumerate() {
                if !corners
                    .iter()
                    .all(|p| ((*p - centre).length() - want).abs() <= 1e-9)
                {
                    continue;
                }
                found[kk] += 1;
                // the box of the face's edges' UV curves
                let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
                for &e in &fe {
                    for eu in edge::edgeuses(&shell, e, None, None).unwrap() {
                        if eu1::face(&shell, eu).unwrap() != Some(f) {
                            continue;
                        }
                        let (Some(uv), _) = eu1::uv_trim_curve(&shell, eu).unwrap() else {
                            continue;
                        };
                        let ivl = uv.natural_interval();
                        for q in 0..=32 {
                            let p = uv
                                .evaluate_point(
                                    &k,
                                    ivl.min() + (ivl.max() - ivl.min()) * f64::from(q) / 32.0,
                                )
                                .unwrap();
                            lo = [lo[0].min(p.x), lo[1].min(p.y)];
                            hi = [hi[0].max(p.x), hi[1].max(p.y)];
                        }
                    }
                }
                for i in 0..=16 {
                    for j in 0..=16 {
                        let (u, v) = (f64::from(i) / 16.0, f64::from(j) / 16.0);
                        let uv = solidcore::math::Point2::new(
                            lo[0] + (hi[0] - lo[0]) * u,
                            lo[1] + (hi[1] - lo[1]) * v,
                        );
                        let p = s.evaluate_point(&k, uv).unwrap();
                        worst = worst.max(((p - centre).length() - want).abs());
                    }
                }
            }
        }
    }
    eprintln!("found {found:?}, surfaces off their spheres {worst:.3e}");
    assert_eq!(found, [8, 8]);
    assert!(worst <= 1e-12, "{worst:e}");
}
