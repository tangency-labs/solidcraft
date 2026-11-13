//! A knife edge chamfered by its sizes (`FilletSession::prepare_chamfers`) at equal distances
//! `d`: the kernel rig's `fillets/851-chamfer-knife-edge-tip-*` (#2793). The faces fold back
//! on each other along the edge, so there is no wedge for a chamfer's definition. The chamfer
//! is the chord between the contacts of the ball of radius `d` that fits there, as
//! `tests/wr_611_steep_fillet_trims_its_face.rs` makes it through the linear section. Other
//! sizes have no such chord and stay refused. A distance past the one at which the chord
//! takes the whole part is refused naming that distance less the fit margin (#15250).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_executive::{FitKind, NoRoomReason};
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::surface::procedural_blend::chamfer::ChamferSize;
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::edge;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

const R: f64 = 21.0;
const TIP: f64 = 62.0 + 1.0 / 6.0;
const H: f64 = 10.0;

fn kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_knife_edge_rounds = true;
    k.fixes.fillet_trim_inside_by_best_dot = true;
    k
}

fn line(a: (f64, f64), b: (f64, f64)) -> Curve {
    Curve::from(create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap())
}

/// The part: a plate whose corner a circle of radius 21 about (TIP, -21) cuts, so its wall runs
/// out tangent into the back face y = 0 at the tip; extruded 10.
fn part(k: &mut Kernel) -> Brep {
    let curves = [
        line((0.0, 0.0), (TIP, 0.0)),
        Curve::from(create_arc(Vec3::new(TIP, -R, 0.0), R, 90.0, 180.0).unwrap()),
        line((TIP - R, -R), (0.0, -R)),
        line((0.0, -R), (0.0, 0.0)),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_taper_extrude(k, &refs, H, 0.0, 3, false).unwrap()
}

fn knife_edge(k: &Kernel, b: &Brep) -> EdgeId {
    let at_tip = |p: Point3| (p.x - TIP).abs() < 1e-6 && p.y.abs() < 1e-6;
    b.edge_list()
        .iter()
        .copied()
        .find(|&e| {
            let curve = edge::curve(b, e).unwrap().unwrap();
            let ivl = edge::interval(b, e).unwrap();
            let p = curve.evaluate_point(k, ivl.min()).unwrap();
            let q = curve.evaluate_point(k, ivl.max()).unwrap();
            at_tip(p) && at_tip(q) && (p.z - q.z).abs() > H / 2.0
        })
        .expect("the knife edge")
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-11, &attrs).unwrap()
}

/// What the chord takes off: the triangle back contact, tip, wall contact, less the wall's
/// circular segment, over the height.
fn removed(d: f64) -> f64 {
    let flat = TIP - (4.0 * R * d).sqrt();
    let wy = -R + (R - d) * R / (R + d);
    let triangle = 0.5 * ((TIP - flat) * wy).abs();
    let b = ((R - d) / (R + d)).acos();
    (triangle - 0.5 * R * R * (b - b.sin())) * H
}

#[test]
fn equal_distances_are_the_chord_between_the_balls_contacts() {
    let before = (TIP - R) * R * H + R * R * (1.0 - PI / 4.0) * H;
    // the rig's values: 9432.157649 at d 2, 9027.684392 at d 5
    assert!((before - removed(2.0) - 9_432.157_648_674_35).abs() < 1e-6);
    assert!((before - removed(5.0) - 9_027.684_392_451_565).abs() < 1e-6);
    for d in [0.5, 2.0, 5.0] {
        let mut k = kernel();
        let mut b = part(&mut k);
        let e = knife_edge(&k, &b);
        {
            let mut s = FilletSession::new(&mut k, &mut b).unwrap();
            s.prepare_chamfers(&k, &[e], &[ChamferSize::Distances([d, d])])
                .unwrap();
            s.do_fillet(&mut k).unwrap();
        }
        stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
        assert!(is_manifold_solid(&b).unwrap(), "d {d}: not a solid");
        let v = volume(&k, &mut b);
        let want = before - removed(d);
        assert!(
            (v - want).abs() <= 1e-9 * want,
            "d {d}: volume {v}, expected {want}"
        );
    }
}

#[test]
fn other_sizes_stay_refused() {
    for size in [
        ChamferSize::Distances([2.0, 3.0]),
        ChamferSize::DistanceAngle {
            distance: 2.0,
            angle: 0.5,
        },
        ChamferSize::Chord(2.0),
    ] {
        let mut k = kernel();
        let mut b = part(&mut k);
        let e = knife_edge(&k, &b);
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        assert!(
            s.prepare_chamfers(&k, &[e], &[size]).is_err(),
            "{size:?} was accepted on a knife edge"
        );
    }
}

/// Every fix, as SolidCraft runs the kernel: a blend past its faces is trimmed at its creases.
fn all_fixes() -> Kernel {
    Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    }
}

/// The chamfer's largest distance: the part's far bottom corner (0, -R) lies at x = -TIP,
/// y = -R from the tip, and the chord through it, `d = R (TIP / (2R - R))² = TIP² / R`, takes
/// all the part (every other point of it is reached sooner: the far top corner at
/// `TIP² / 4R`, the arc's foot at `R`).
const KEEPS_NOTHING: f64 = TIP * TIP / R;

/// What the chord of distance `d` keeps, past 46 (where its flat contact leaves the top face)
/// and short of `KEEPS_NOTHING`: the triangle at the far bottom corner cut off by the chord
/// through `A = (0, -2R)` from the tip and the flat contact `(-2s, 0)`, `s = sqrt(R d)`. The
/// chord meets the far side `x = -TIP` at `y = R TIP / s - 2R` and the bottom `y = -R` at
/// `x = -s`, over the height.
fn kept(d: f64) -> f64 {
    let s = (R * d).sqrt();
    0.5 * (R * TIP / s - R) * (TIP - s) * H
}

#[derive(Debug)]
struct Refused {
    reason: Option<NoRoomReason>,
    kind: Option<FitKind>,
    on_edge: bool,
    distance: Option<f64>,
}

/// The knife edge chamfered at equal distances `d` with every fix: the part's volume when it
/// builds, else the refusal.
fn chamfer(d: f64) -> Result<f64, Refused> {
    let mut k = all_fixes();
    let mut b = part(&mut k);
    let e = knife_edge(&k, &b);
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_chamfers(&k, &[e], &[ChamferSize::Distances([d, d])])
            .unwrap();
        if s.do_fillet(&mut k).is_err() {
            let fit = s.fit_size();
            return Err(Refused {
                reason: s.error_info().no_room_reason(),
                kind: fit.map(|f| f.kind),
                on_edge: fit.is_some_and(|f| f.edge == Some(e)),
                distance: fit.and_then(|f| s.chamfer_fit(f)),
            });
        }
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    assert!(is_manifold_solid(&b).unwrap(), "d {d}: not a solid");
    Ok(volume(&k, &mut b))
}

/// Chamfered 200, wider than the part: refused as an overflow that keeps nothing (not as
/// corners taking the edge: the knife edge's ends have no corners), naming the distance at
/// which the chord takes all the part, `TIP² / R`, less the fit margin (a thousand edge
/// tolerances of 1e-5), as a radius fit on the knife edge, which is its distance.
#[test]
fn a_chamfer_wider_than_the_part_names_where_it_keeps_nothing() {
    let refused = chamfer(200.0).expect_err("refused");
    assert_eq!(
        refused.reason,
        Some(NoRoomReason::OverflowKeepsNothing),
        "{refused:?}"
    );
    assert_ne!(refused.reason, Some(NoRoomReason::CornersTakeWholeEdge));
    assert_eq!(refused.kind, Some(FitKind::Radius), "{refused:?}");
    assert!(refused.on_edge, "{refused:?}");
    let d = refused.distance.expect("a distance");
    let closed = KEEPS_NOTHING - 0.01;
    assert!((d - closed).abs() <= 1e-9 * closed, "{d} vs {closed}");
    assert!((KEEPS_NOTHING - 184.033_068_783_068_8).abs() < 1e-9);
}

/// The distance named builds, keeping the sliver at the far bottom corner the chord cuts off;
/// so does 100, where the issue's part kept 611.83. 100's volume is the clip's to 1e-9.
///
/// The sliver (4.8e-6; legs 1.7e-3 and 5.7e-4) is bounded by the accuracy of the chamfer's
/// contacts. Its two legs lie on exact faces (the far side and the bottom), so the error
/// lies in the chord, and the chord is only as good as its ends. With every fix, the rails
/// are the feet of the stations that `GnSectionFsg::build` re-solves. Each station's Newton
/// stops at the residual `1e-9 * max(1, |m|)` (`fillet_section/section.rs`, `solve_tol`),
/// where `m` is the station's centre-line point, the ball's centre (TIP - 2s, -d, z):
/// |m| <= 194.6 here. The chord moves by at most that much, so the volume moves by at most
/// `tol * hypotenuse * H` = 3.5e-9 absolute (7e-4 of the sliver). The 3.2e-9 relative error
/// seen here (1.5e-14 absolute; contacts about 1e-12 off, near |x| = 124) is that solve's
/// noise, far inside its tolerance. It does not depend on the volume integrator's accuracy:
/// it is the same from 1e-6 to 1e-11. The sliver's volume goes as the square of what the
/// distance falls short of `TIP² / R`, so contact noise is amplified there; 100's check is the
/// well-conditioned one.
#[test]
fn the_distance_named_builds_the_sliver_the_chord_keeps() {
    let v = chamfer(100.0).unwrap_or_else(|r| panic!("d 100: {r:?}"));
    let want = kept(100.0);
    assert!(
        (v - want).abs() <= 1e-9 * want,
        "d 100: volume {v}, expected {want}"
    );
    let named = KEEPS_NOTHING - 0.01;
    let v = chamfer(named).unwrap_or_else(|r| panic!("d {named}: {r:?}"));
    let want = kept(named);
    let s = (R * named).sqrt();
    let (a, b) = (TIP - s, R * TIP / s - R);
    let hypotenuse = (a * a + b * b).sqrt();
    let centre = ((TIP - 2.0 * s).powi(2) + named * named + H * H).sqrt();
    let solve_tol = 1.0e-9 * centre.max(1.0);
    assert!(
        (v - want).abs() <= solve_tol * hypotenuse * H,
        "d {named}: volume {v}, expected {want}"
    );
    assert!((kept(100.0) - 611.83).abs() < 0.01);
}

/// Just past the distance at which nothing is kept (184.04 against 184.033) is refused
/// naming the same distance.
#[test]
fn a_chamfer_just_past_the_distance_named_is_refused() {
    let refused = chamfer(184.04).expect_err("refused");
    assert_eq!(refused.reason, Some(NoRoomReason::OverflowKeepsNothing));
    let d = refused.distance.expect("a distance");
    let closed = KEEPS_NOTHING - 0.01;
    assert!((d - closed).abs() <= 1e-9 * closed, "{d} vs {closed}");
}

/// The limit read off the part's geometry, with no build: the knife edge between the top
/// plane and the wall's cylinder gives `TIP² / R`; an edge between two planes is no such
/// knife edge and gives none.
#[test]
fn the_limit_is_read_off_the_geometry() {
    use solidcore::blend::fillet_executive::session::fit_size::knife_chamfer_keeps_nothing;
    let mut k = all_fixes();
    let b = part(&mut k);
    let e = knife_edge(&k, &b);
    let limit = knife_chamfer_keeps_nothing(&k, &b, e)
        .unwrap()
        .expect("a limit");
    assert!(
        (limit - KEEPS_NOTHING).abs() <= 1e-9 * KEEPS_NOTHING,
        "{limit} vs {KEEPS_NOTHING}"
    );
    let corner = b
        .edge_list()
        .iter()
        .copied()
        .find(|&ei| {
            let faces = edge::faces(&b, ei, None).unwrap();
            faces.len() == 2
                && faces.iter().all(|&f| {
                    matches!(
                        solidcore::topology::face::p2::surface(&b, f)
                            .unwrap()
                            .unwrap()
                            .kind(),
                        solidcore::surface::surface::SurfaceKind::Plane(_)
                    )
                })
        })
        .expect("an edge between planes");
    assert_eq!(knife_chamfer_keeps_nothing(&k, &b, corner).unwrap(), None);
}

/// The wall split along its length into two faces of the same cylinder (its arc made of two
/// arcs): the edge between them is a line along the axis, not an arc of the circle, and the
/// split changes nothing about the chamfer, so the limit read off the geometry is still
/// `TIP² / R`.
#[test]
fn the_limit_holds_with_the_wall_split_along_its_length() {
    use solidcore::blend::fillet_executive::session::fit_size::knife_chamfer_keeps_nothing;
    let mut k = all_fixes();
    let curves = [
        line((0.0, 0.0), (TIP, 0.0)),
        Curve::from(create_arc(Vec3::new(TIP, -R, 0.0), R, 90.0, 135.0).unwrap()),
        Curve::from(create_arc(Vec3::new(TIP, -R, 0.0), R, 135.0, 180.0).unwrap()),
        line((TIP - R, -R), (0.0, -R)),
        line((0.0, -R), (0.0, 0.0)),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let b = create_taper_extrude(&mut k, &refs, H, 0.0, 3, false).unwrap();
    let e = knife_edge(&k, &b);
    let limit = knife_chamfer_keeps_nothing(&k, &b, e)
        .unwrap()
        .expect("a limit");
    assert!(
        (limit - KEEPS_NOTHING).abs() <= 1e-9 * KEEPS_NOTHING,
        "{limit} vs {KEEPS_NOTHING}"
    );
}

/// The fallback: the part with a spherical dimple in its bottom face, far from the knife
/// edge, is a body the limit is not read off (a sphere is neither a plane nor a cylinder
/// along the edge), so `knife_chamfer_keeps_nothing` gives none and the refusal names the
/// overflow's own bound, where the rails stay on their faces: the arc's radius less the
/// fit margin, as the part named before its limit was computed. The reason is still that
/// the overflow keeps nothing, not corners taking the edge (it has no corners).
#[test]
fn a_knife_edge_the_limit_is_not_read_off_names_its_faces_bound() {
    use solidcore::blend::fillet_executive::session::fit_size::knife_chamfer_keeps_nothing;
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::primitive::api_primitives::create_sphere;
    let mut k = all_fixes();
    let plain = part(&mut k);
    let dimple = create_sphere(&mut k, Point3::new(10.0, -10.0, 0.0), 3.0).unwrap();
    let mut b = boolean_difference(&mut k, plain, dimple).unwrap();
    let e = knife_edge(&k, &b);
    assert_eq!(knife_chamfer_keeps_nothing(&k, &b, e).unwrap(), None);
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_chamfers(&k, &[e], &[ChamferSize::Distances([200.0, 200.0])])
        .unwrap();
    assert!(s.do_fillet(&mut k).is_err(), "200 built");
    assert_eq!(
        s.error_info().no_room_reason(),
        Some(NoRoomReason::OverflowKeepsNothing)
    );
    let fit = s.fit_size().expect("a fit");
    assert_eq!(fit.kind, FitKind::Radius);
    assert_eq!(fit.edge, Some(e));
    let d = s.chamfer_fit(fit).expect("a distance");
    let closed = R - 0.01;
    assert!((d - closed).abs() <= 1e-9 * closed, "{d} vs {closed}");
}
