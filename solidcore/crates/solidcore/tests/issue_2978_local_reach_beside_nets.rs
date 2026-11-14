//! The local operation's reach beside bare B-spline nets: planes moved beside the walls of
//! `Solids/extrusion.smb`, two of which are extrusions with no analytic label (of a cubic
//! B-spline and of a rational curve of lines and an arc; the reader makes its other faces
//! planes and cylinders), build sound bodies of their closed-form volumes instead of being
//! refused as `ReachUncertified`; so does the base of `Solids/pyramid.smb` moved up,
//! beside sides stored as bare B-spline nets; and
//! the planes moved over the curved nets of `Solids/nurb_fillet.smb` (the cases whose
//! extension can leave a body that is no manifold solid) are built sound or refused with
//! the input left as it came in, never returned unsound. A surface of revolution with no
//! analytic label (a Bézier profile turned 90°) moved out beside its caps and half-planes
//! builds the sound body of its Pappus volume, and a half-plane moved past the turn's end,
//! which the executive does not turn on, is refused.
//!
//! Each case but the revolution's is a goldens case of `topology.brep_local_operation`, made as its runner makes
//! it: a face's surface replaced by the plane through the point at the middle of its UV
//! domain, moved `offset` along the surface's normal there, the normal tilted by `tilt`.
//!
//! The solids are the upstream test files, read when the pinned upstream checkout is
//! present (the tests are skipped otherwise, as in CI's test job).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::Notifications;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::mass::integrator::properties::{brep_volume_default, face_area, face_centroid};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::knots::Side;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::local_operation::local_operation;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::face::p2 as face_ops;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::FaceId;
use solidcore::{Brep, EntityRef, Kernel};

/// The upstream test file `Solids/<name>`, or `None` without the upstream checkout.
fn solid(k: &mut Kernel, name: &str) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Solids")
        .join(name);
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        eprintln!("unavailable upstream test file: {}", path.display());
        return None;
    };
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = |_: EntityRef| ObjectRef {
        key: ObjectKey(0),
        type_code: 0,
    };
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    Some(b)
}

/// The plane the goldens runner gives face `index` (`GetFaces` order): through the point
/// at the middle of its UV domain moved `offset` along the normal there, the normal plus
/// `tilt`: the face, and the plane's origin and normal.
fn plane_for(
    k: &Kernel,
    b: &Brep,
    index: usize,
    offset: f64,
    tilt: [f64; 3],
) -> (FaceId, (Point3, Vec3)) {
    let face = bp1::faces(b, None).unwrap()[index];
    let surface = face_ops::surface(b, face).unwrap().unwrap();
    let uv = b.face(face).unwrap().uv_domain.evaluate(0.5, 0.5);
    let p = surface.evaluate_point(k, uv).unwrap();
    let n = surface
        .evaluate_normal(k, uv, Side::Left, Side::Left)
        .unwrap();
    let origin = Point3::new(p.x + n.x * offset, p.y + n.y * offset, p.z + n.z * offset);
    let normal = Vec3::new(n.x + tilt[0], n.y + tilt[1], n.z + tilt[2]);
    (face, (origin, normal))
}

/// `b`'s faces `moves` (index, offset, tilt) given their planes by a local operation; the
/// planes' origins and normals.
fn moved(
    k: &mut Kernel,
    b: &mut Brep,
    moves: &[(usize, f64, [f64; 3])],
) -> solidcore::Result<Vec<(Point3, Vec3)>> {
    let (faces, planes): (Vec<FaceId>, Vec<(Point3, Vec3)>) = moves
        .iter()
        .map(|&(i, offset, tilt)| plane_for(k, b, i, offset, tilt))
        .unzip();
    let surfaces = planes
        .iter()
        .map(|&(o, n)| Surface::from(Plane::from_normal(o, n)))
        .collect();
    let mut notifications = Notifications::new(b);
    local_operation(k, b, &faces, surfaces, &mut notifications.context())?;
    Ok(planes)
}

/// The face of `b` whose surface's point at the middle of its domain is at height `z`, and
/// whose normal there is along z (a cap of the extrusion).
fn cap_at(k: &Kernel, b: &Brep, z: f64) -> FaceId {
    let caps: Vec<FaceId> = bp1::faces(b, None)
        .unwrap()
        .into_iter()
        .filter(|&f| {
            let s = face_ops::surface(b, f).unwrap().unwrap();
            let uv = b.face(f).unwrap().uv_domain.evaluate(0.5, 0.5);
            let p = s.evaluate_point(k, uv).unwrap();
            let n = s.evaluate_normal(k, uv, Side::Left, Side::Left).unwrap();
            (p.z - z).abs() < 1e-9 && n.x.abs() < 1e-9 && n.y.abs() < 1e-9
        })
        .collect();
    assert_eq!(caps.len(), 1, "{caps:?}");
    caps[0]
}

/// The integrators' accuracy, and the agreement asked of two integrals at it.
const ACCURACY: f64 = 1e-6;
/// The accuracy the revolution's volumes are integrated at (against Pappus).
const TIGHT: f64 = 1e-10;
const AGREE: f64 = 1e-5;

/// The volume of the manifold solid `b`, integrated at [`ACCURACY`].
fn volume(k: &mut Kernel, b: &mut Brep) -> solidcore::Result<f64> {
    brep_volume_default(k, b, ACCURACY)
}

/// Walls of the extrusion (z from 0 to 10) moved onto planes, the caps kept: the result
/// is the prism over its bottom cap, ten times the cap's area, and sound, and each new wall
/// stands on its plane (at least four of the body's vertices on it, two at each cap).
fn walls_moved_build_their_prism(moves: &[(usize, f64, [f64; 3])]) {
    let mut k = Kernel::default();
    let Some(mut b) = solid(&mut k, "extrusion.smb") else {
        return;
    };
    let faces = bp1::faces(&b, None).unwrap().len();
    let planes = moved(&mut k, &mut b, moves).unwrap();
    for (o, n) in planes {
        let n = n * (1.0 / n.length());
        let on: Vec<f64> = b
            .vertex_list()
            .iter()
            .map(|&v| solidcore::topology::vertex::point(&b, v).unwrap())
            .filter(|p| (*p - o).dot(n).abs() <= b.tolerance())
            .map(|p| p.z)
            .collect();
        let at = |z: f64| {
            on.iter()
                .filter(|&&h| (h - z).abs() <= b.tolerance())
                .count()
        };
        assert!(at(0.0) >= 2 && at(10.0) >= 2, "{on:?}");
    }
    assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
    assert_eq!(bp1::faces(&b, None).unwrap().len(), faces);
    let v = volume(&mut k, &mut b).unwrap();
    let bottom = cap_at(&k, &b, 0.0);
    let want = 10.0 * face_area(&k, &mut b, bottom, ACCURACY).unwrap();
    assert!((v - want).abs() <= AGREE * want, "{v} vs {want}");
}

/// gen0000: the quarter cylinder wall (face 4) given its tangent plane moved out 1.14, and
/// the cubic wall (face 2) its tangent plane at its middle. Each new plane meets the caps,
/// the flat wall beside them and the rational wall (face 5, an extrusion), whose ruled
/// extension past its end the moved planes reach.
#[test]
fn two_walls_given_planes_out_build_the_prism_of_their_profile() {
    walls_moved_build_their_prism(&[(4, 1.143_604_136_567_873_7, [0.0; 3]), (2, 0.0, [0.0; 3])]);
}

/// gen0021: the same two walls, both moved out (0.20 and 0.55).
#[test]
fn two_walls_moved_out_build_the_prism_of_their_profile() {
    walls_moved_build_their_prism(&[
        (4, 0.202_101_138_781_204_4, [0.0; 3]),
        (2, 0.548_365_023_570_791_1, [0.0; 3]),
    ]);
}

/// gen0048: the cubic wall (face 2) given its tangent plane at its middle.
#[test]
fn a_cubic_wall_given_its_tangent_plane_builds_the_prism_of_its_profile() {
    walls_moved_build_their_prism(&[(2, 0.0, [0.0; 3])]);
}

/// gen0041: the top cap (face 1) moved down 0.16 and tilted. The walls keep their surfaces
/// and meet it along their rulings (the two extrusions as nets); the body is the solid over the
/// bottom cap under the tilted plane, whose volume is the cap's area times the plane's
/// height over the cap's centroid (the height is linear), and sound.
#[test]
fn a_top_moved_down_and_tilted_holds_its_height_over_the_centroid() {
    let mut k = Kernel::default();
    let Some(mut b) = solid(&mut k, "extrusion.smb") else {
        return;
    };
    let bottom = cap_at(&k, &b, 0.0);
    let (area, centroid) = face_centroid(&k, &mut b, bottom, ACCURACY).unwrap();
    let tilt = [
        -0.095_716_453_310_687_6,
        0.014_193_158_954_830_198,
        -0.155_603_183_282_545_88,
    ];
    let planes = moved(&mut k, &mut b, &[(1, -0.163_195_178_504_810_47, tilt)]).unwrap();
    assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
    // the plane's height over the centroid: (x − o)·n = 0 solved for z
    let (o, n) = planes[0];
    let height = o.z - ((centroid.x - o.x) * n.x + (centroid.y - o.y) * n.y) / n.z;
    let want = area * height;
    let v = volume(&mut k, &mut b).unwrap();
    assert!((v - want).abs() <= AGREE * want, "{v} vs {want}");
}

/// gen0076: the base of `Solids/pyramid.smb` (a plane) moved up 1.31 toward the apex. Two
/// of the sides are nets off their planes by the rounding of their stored points (single
/// precision: 1.9e-6); the base meets them on their nets. What is left is the pyramid above
/// the new base, similar to the old one: the volume times `((h − d) / h)³`, `h` the apex's
/// height over the old base, `d` the move.
#[test]
fn a_pyramid_base_moved_up_leaves_the_similar_pyramid_above_it() {
    let mut k = Kernel::default();
    let Some(mut b) = solid(&mut k, "pyramid.smb") else {
        return;
    };
    let before = volume(&mut k, &mut b).unwrap();
    let h = b
        .vertex_list()
        .iter()
        .map(|&v| solidcore::topology::vertex::point(&b, v).unwrap().z)
        .fold(f64::NEG_INFINITY, f64::max);
    let planes = moved(&mut k, &mut b, &[(0, 1.308_567_886_431_627_2, [0.0; 3])]).unwrap();
    assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
    let d = planes[0].0.z;
    assert!(d > 1.3 && d < 1.31, "{d}");
    let want = before * ((h - d) / h).powi(3);
    let v = volume(&mut k, &mut b).unwrap();
    assert!((v - want).abs() <= AGREE * want, "{v} vs {want}");
}

/// The cases of `Solids/nurb_fillet.smb` that can leave a non-manifold body once the plane
/// beside its curved nets has a reach: built only as a manifold solid of positive volume,
/// or refused with the input as it came in (its volume, faces and edges).
fn sound_or_refused_unchanged(moves: &[(usize, f64, [f64; 3])]) {
    let mut k = Kernel::default();
    let Some(mut b) = solid(&mut k, "nurb_fillet.smb") else {
        return;
    };
    let before = (
        volume(&mut k, &mut b).unwrap(),
        bp1::faces(&b, None).unwrap().len(),
        b.edge_list().len(),
    );
    match moved(&mut k, &mut b, moves) {
        Ok(_) => {
            assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
            assert!(volume(&mut k, &mut b).unwrap() > 0.0);
        }
        Err(e) => {
            // a refusal of the local operation, not an internal failure rolled back
            assert_eq!(e.kind(), solidcore::error::ErrorKind::Unsupported, "{e}");
            assert!(
                matches!(
                    e.failure().map(|f| &f.reason),
                    Some(solidcore::error::Reason::Offset(_))
                ),
                "not an offset refusal: {e}"
            );
            let after = (
                volume(&mut k, &mut b).unwrap(),
                bp1::faces(&b, None).unwrap().len(),
                b.edge_list().len(),
            );
            assert_eq!(after, before, "{e}");
        }
    }
}

/// `nurb_fillet_face0_same` (and its `attributed_` twin): face 0 given its tangent plane.
#[test]
fn a_curved_net_given_its_tangent_plane_is_sound_or_refused_unchanged() {
    sound_or_refused_unchanged(&[(0, 0.0, [0.0; 3])]);
}

/// `nurb_fillet_face4_same` (and its `attributed_` twin): face 4 given its tangent plane.
#[test]
fn another_curved_net_given_its_tangent_plane_is_sound_or_refused_unchanged() {
    sound_or_refused_unchanged(&[(4, 0.0, [0.0; 3])]);
}

/// `fc0004`: faces 16 and 0 given tilted planes, 16's moved in 1.21.
#[test]
fn two_curved_nets_given_tilted_planes_are_sound_or_refused_unchanged() {
    sound_or_refused_unchanged(&[
        (
            16,
            -1.212_618_021_690_697_2,
            [
                -0.227_499_561_725_195_12,
                -0.119_235_175_885_283_43,
                0.212_040_256_859_765_76,
            ],
        ),
        (
            0,
            0.0,
            [
                0.195_822_407_983_586_08,
                -0.180_816_552_666_528_46,
                -0.250_316_266_099_896_8,
            ],
        ),
    ]);
}

/// Sam-47515301's quarter revolution on #13722, as SolidCraft built it: the profile
/// `r = B(15, 21, 20, 15)`, `z = 90 t` (a cubic Bézier over `t` in [0, 1]) closed along the
/// axis and by the caps `z = 0` and `z = 90`, turned 90° about z. Its five faces: the
/// surface of revolution (no analytic label: a net), the two caps and the two half-planes
/// at either end of the turn.
const REVOLVED_QUARTER: &[u8] = include_bytes!("data/issue_13722_revolved_bezier_quarter.smb");

/// [`REVOLVED_QUARTER`], read as SolidCraft reads it.
fn revolved_quarter(k: &mut Kernel) -> Brep {
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &solidcore::topology::loopuse::notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = solidcore::topology::api_files::read_brep_from_file(
        k,
        Some(REVOLVED_QUARTER.to_vec()),
        false,
        false,
        &mut env,
    )
    .unwrap();
    b.refresh_cached_topology();
    b
}

/// The faces of `b` whose surface is a surface of revolution.
fn revolutions(b: &Brep) -> Vec<FaceId> {
    bp1::faces(b, None)
        .unwrap()
        .into_iter()
        .filter(|&f| {
            matches!(
                face_ops::surface(b, f).unwrap().unwrap().kind(),
                solidcore::surface::surface::SurfaceKind::SurfOfRevolution(_)
            )
        })
        .collect()
}

/// The quarter's volume by Pappus: `θ ∬ r dA` over the profile, by Green's theorem
/// `θ ∫ r²/2 dz` along its outer side (the axis `r = 0` and the caps `dz = 0` add nothing),
/// for the side moved `d` along its outward normal `(z', −r') / |c'|` and cut by the caps
/// (where it ends inside a cap, its tangent line on to the cap: the executive's ruled
/// extension of the moved surface past its profile's ends). Integrated in plain f64 by
/// 40-point Gauss–Legendre over 400 pieces, independently of the kernel; the cut parameters
/// by bisection. `d = 0` is the input's volume, `θ ∫ r² 45 dt` in closed form.
fn pappus(d: f64) -> f64 {
    const R: [f64; 4] = [15.0, 21.0, 20.0, 15.0];
    const Z: [f64; 4] = [0.0, 30.0, 60.0, 90.0];
    let theta = std::f64::consts::FRAC_PI_2;
    let bez = |c: [f64; 4], t: f64| {
        let s = 1.0 - t;
        c[0] * s * s * s + 3.0 * c[1] * t * s * s + 3.0 * c[2] * t * t * s + c[3] * t * t * t
    };
    let dbez = |c: [f64; 4], t: f64| {
        let s = 1.0 - t;
        3.0 * ((c[1] - c[0]) * s * s + 2.0 * (c[2] - c[1]) * t * s + (c[3] - c[2]) * t * t)
    };
    let ddr =
        |t: f64| 6.0 * ((R[2] - 2.0 * R[1] + R[0]) * (1.0 - t) + (R[3] - 2.0 * R[2] + R[1]) * t);
    // the moved side `(R, Z)(t)` and `Z'(t)` (z'' = 0: z is linear in t)
    let side = |t: f64| {
        let (rp, zp) = (dbez(R, t), dbez(Z, t));
        let l = rp.hypot(zp);
        (bez(R, t) + d * zp / l, bez(Z, t) - d * rp / l)
    };
    let side_dz = |t: f64| {
        let (rp, zp) = (dbez(R, t), dbez(Z, t));
        let l = rp.hypot(zp);
        let dl = rp * ddr(t) / l;
        zp - d * (ddr(t) * l - rp * dl) / (l * l)
    };
    let cut = |target: f64| {
        let (mut lo, mut hi) = (-0.2_f64, 1.2_f64);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if side(mid).1 < target {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    };
    // Gauss–Legendre nodes and weights (Newton on P_40)
    let n = 40;
    let mut nodes = Vec::new();
    for i in 1..=n {
        let mut x = (std::f64::consts::PI * (f64::from(i) - 0.25) / (f64::from(n) + 0.5)).cos();
        let mut dp = 1.0;
        for _ in 0..100 {
            let (mut p0, mut p1) = (1.0, x);
            for k in 2..=n {
                let k = f64::from(k);
                (p0, p1) = (p1, ((2.0 * k - 1.0) * x * p1 - (k - 1.0) * p0) / k);
            }
            dp = f64::from(n) * (x * p1 - p0) / (x * x - 1.0);
            let dx = p1 / dp;
            x -= dx;
            if dx.abs() < 1e-17 {
                break;
            }
        }
        nodes.push((x, 2.0 / ((1.0 - x * x) * dp * dp)));
    }
    let quad = |f: &dyn Fn(f64) -> f64, a: f64, b: f64| {
        let m = 400;
        let h = (b - a) / f64::from(m);
        (0..m)
            .map(|j| {
                let c = a + (f64::from(j) + 0.5) * h;
                nodes
                    .iter()
                    .map(|&(x, w)| w * f(c + 0.5 * h * x))
                    .sum::<f64>()
                    * 0.5
                    * h
            })
            .sum::<f64>()
    };
    let (t0, t1) = (cut(0.0), cut(90.0));
    let (a, b) = (t0.max(0.0), t1.min(1.0));
    let mut v = theta * quad(&|t| side(t).0.powi(2) / 2.0 * side_dz(t), a, b);
    // a tangent line on from an end of the side to the cap it ends inside: R linear in Z
    for (end, cap, inside) in [(0.0, 0.0, t0 < 0.0), (1.0, 90.0, t1 > 1.0)] {
        if inside {
            let (r0, z0) = side(end);
            let slope = dbez(R, end) / dbez(Z, end);
            let f = |z: f64| (r0 + (z - z0) * slope).powi(3) / (6.0 * slope);
            v += theta
                * if end == 0.0 {
                    f(z0) - f(cap)
                } else {
                    f(cap) - f(z0)
                };
        }
    }
    v
}

/// The revolution of [`REVOLVED_QUARTER`] moved out 1.5 (Sam's Offset Face): beside the caps
/// and the half-planes, which keep their surfaces, the moved surface of revolution is a net
/// with a certified reach (its sides round the axis taken on as the turn continued, only as
/// far as the executive's surfaces hold within the tolerance), not refused as
/// `ReachUncertified` as on main. The body is a sound manifold solid of the profile moved
/// out turned 90°: its volume is [`pappus`]'s within the moved surface's own construction
/// bound (its generator's offset is made within the B-rep's tolerance: the volume moves by
/// at most that times the moved face's area) and the integrator's accuracy.
fn revolution_moved_builds_its_pappus_volume(d: f64) {
    let mut k = Kernel::default();
    let mut b = revolved_quarter(&mut k);
    let before = brep_volume_default(&k, &mut b, TIGHT).unwrap();
    assert!(
        (before - pappus(0.0)).abs() <= 10.0 * TIGHT * before,
        "{before} vs {}",
        pappus(0.0)
    );
    let faces = revolutions(&b);
    assert_eq!(faces.len(), 1);
    let tolerance = b.tolerance();
    let mut exec = solidcore::offset::offset_faces::OffsetFacesExecutive::new();
    exec.offset_faces(&mut k, &mut b, &faces, d).unwrap();
    assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
    let check = solidcore::topology::brep::validation::check_brep(
        &mut k,
        &mut b,
        solidcore::topology::brep::validation::CheckLevel::Level2,
    )
    .unwrap();
    assert!(check.is_valid(), "{check:?}");
    assert_eq!(bp1::faces(&b, None).unwrap().len(), 5);
    let moved = revolutions(&b);
    assert_eq!(moved.len(), 1);
    let area = face_area(&k, &mut b, moved[0], ACCURACY).unwrap();
    let v = brep_volume_default(&k, &mut b, TIGHT).unwrap();
    let want = pappus(d);
    // the generator's offset is made within the tolerance, so the moved face is within it of
    // the exact offset and the volume within its area times it; and the integrator's
    // accuracy. An offset `δ` past the tolerance moves the volume by about `area δ`, over it.
    let bound = area * tolerance + 10.0 * TIGHT * want;
    assert!((v - want).abs() <= bound, "{v} vs {want} (bound {bound})");
}

/// Out 1.5, as on #13722: the moved side meets both caps on its own face.
#[test]
fn a_revolved_spline_moved_out_builds_its_pappus_volume() {
    revolution_moved_builds_its_pappus_volume(1.5);
}

/// A half-plane at an end of the turn moved out: the kept surface of revolution would have
/// to be turned on 1.5 past its side round the axis to meet it, which the executive does
/// not build. The meeting is proved on the turn continued but not within the tolerance of
/// the side, so the reach is refused (`ReachUncertified`), never a body short of the meeting.
#[test]
fn a_half_plane_moved_past_a_revolutions_turn_is_refused() {
    let mut k = Kernel::default();
    let mut b = revolved_quarter(&mut k);
    // the half-plane x = 0 (the turn's end at 90°)
    let end = bp1::faces(&b, None)
        .unwrap()
        .into_iter()
        .find(|&f| {
            let s = face_ops::surface(&b, f).unwrap().unwrap();
            let uv = b.face(f).unwrap().uv_domain.evaluate(0.5, 0.5);
            let p = s.evaluate_point(&k, uv).unwrap();
            let n = s.evaluate_normal(&k, uv, Side::Left, Side::Left).unwrap();
            p.x.abs() < 1e-9 && n.y.abs() < 1e-9 && n.z.abs() < 1e-9
        })
        .unwrap();
    let mut exec = solidcore::offset::offset_faces::OffsetFacesExecutive::new();
    let e = exec.offset_faces(&mut k, &mut b, &[end], 1.5).unwrap_err();
    assert!(
        matches!(
            e.failure().map(|f| &f.reason),
            Some(solidcore::error::Reason::Offset(
                solidcore::error::offset::OffsetFailure::ReachUncertified { .. }
            ))
        ),
        "{e:?}"
    );
}

/// A certified upper bound on the distance of `p` from the plane through `o` with unit
/// normal `n`: the f64 value plus the rounding of its dot product (`γ_3` of the terms' sizes,
/// and the subtraction's).
fn plane_gap(p: Point3, o: Point3, n: Vec3) -> f64 {
    let d = p - o;
    let size = (d.x * n.x).abs() + (d.y * n.y).abs() + (d.z * n.z).abs();
    let terms = (p.x.abs() + o.x.abs()) * n.x.abs()
        + (p.y.abs() + o.y.abs()) * n.y.abs()
        + (p.z.abs() + o.z.abs()) * n.z.abs();
    d.dot(n).abs() + 8.0 * f64::EPSILON * (size + terms)
}

/// The one face of `b` on the plane through `o` with normal `n` (its surface's point and
/// normal at the middle of its domain on that plane), and that every vertex of it and every
/// edge of it lie on the plane within the B-rep's tolerance: a vertex by [`plane_gap`], an
/// edge by every control point of its curve's NURBS form (its weights positive, so the
/// curve lies in their convex hull, as near the plane as the farthest of them).
fn plane_face_lies_on_its_plane(k: &Kernel, b: &Brep, o: Point3, n: Vec3) {
    use solidcore::curve::curve::CurveKind;
    let n = n * (1.0 / n.length());
    let tol = b.tolerance();
    let on: Vec<FaceId> = bp1::faces(b, None)
        .unwrap()
        .into_iter()
        .filter(|&f| {
            let s = face_ops::surface(b, f).unwrap().unwrap();
            let uv = b.face(f).unwrap().uv_domain.evaluate(0.5, 0.5);
            let p = s.evaluate_point(k, uv).unwrap();
            let m = s.evaluate_normal(k, uv, Side::Left, Side::Left).unwrap();
            plane_gap(p, o, n) <= tol && m.dot(n).abs() >= 1.0 - 1e-12
        })
        .collect();
    assert_eq!(on.len(), 1, "faces on the plane through {o:?}: {on:?}");
    let face = on[0];
    for v in solidcore::topology::face::p4::vertices(b, face, None, false, None).unwrap() {
        let p = solidcore::topology::vertex::point(b, v).unwrap();
        assert!(plane_gap(p, o, n) <= tol, "vertex {v:?} at {p:?}");
    }
    for e in solidcore::topology::face::p4::edges(b, face, None, false, None).unwrap() {
        let curve = solidcore::topology::edge::curve(b, e).unwrap().unwrap();
        let form = match curve.kind() {
            CurveKind::BSpline(c) => c.canonical(),
            CurveKind::Ellipse(c) => c.base().canonical(),
            CurveKind::Line(c) => c.base_with_nurb().unwrap().canonical(),
            other => panic!("edge {e:?}: a curve with no NURBS form: {other:?}"),
        }
        .unwrap();
        assert!(form.weights.iter().all(|&w| w > 0.0), "edge {e:?}");
        for &p in &form.control_points {
            assert!(plane_gap(p, o, n) <= tol, "edge {e:?}: control point {p:?}");
        }
    }
}

/// The cases of `Solids/revolution.smb` (four surfaces of revolution with no analytic label,
/// their profile pieces, between two caps) whose planes main refused as `ReachUncertified`
/// and that now have a certified reach beside the revolutions: `built`, a sound manifold
/// solid (`check_brep` level 2) with one face on each new plane, every vertex and every
/// edge of it certified on that plane ([`plane_face_lies_on_its_plane`]); otherwise refused
/// as an offset edge that vanishes, with the input left as it came in (its volume, faces
/// and edges).
fn revolution_built_sound_or_refused_unchanged(moves: &[(usize, f64, [f64; 3])], built: bool) {
    let mut k = Kernel::default();
    let Some(mut b) = solid(&mut k, "revolution.smb") else {
        return;
    };
    let before = (
        volume(&mut k, &mut b).unwrap(),
        bp1::faces(&b, None).unwrap().len(),
        b.edge_list().len(),
    );
    match moved(&mut k, &mut b, moves) {
        Ok(planes) => {
            assert!(built, "built where a refusal was expected");
            assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
            let check = solidcore::topology::brep::validation::check_brep(
                &mut k,
                &mut b,
                solidcore::topology::brep::validation::CheckLevel::Level2,
            )
            .unwrap();
            assert!(check.is_valid(), "{check:?}");
            assert!(volume(&mut k, &mut b).unwrap() > 0.0);
            for (o, n) in planes {
                plane_face_lies_on_its_plane(&k, &b, o, n);
            }
        }
        Err(e) => {
            assert!(!built, "refused: {e}");
            // (the one case refused: an offset edge vanishes, not an uncertified reach)
            assert!(e.is_offset_edge_vanishes(), "{e:?}");
            let after = (
                volume(&mut k, &mut b).unwrap(),
                bp1::faces(&b, None).unwrap().len(),
                b.edge_list().len(),
            );
            assert_eq!(after, before, "{e}");
        }
    }
}

/// `gen0009` of `topology.brep_local_operation`: three revolution faces given planes, one
/// tilted, built.
#[test]
fn three_revolution_faces_given_planes_build_a_sound_solid() {
    revolution_built_sound_or_refused_unchanged(
        &[
            (2, -0.448_550_426_245_376_55, [0.0; 3]),
            (0, 0.923_223_920_212_509_9, [0.0; 3]),
            (
                3,
                -0.796_674_521_950_179,
                [
                    -0.222_731_639_538_988_28,
                    -0.077_993_583_261_715_68,
                    -0.016_536_445_744_678_385,
                ],
            ),
        ],
        true,
    );
}

/// `gen0012`: one revolution face given two planes and another a tilted one, refused with
/// the input unchanged (past the reach, an edge vanishes: `SM_ERR_OFFSET_EDGE_VANISHES`).
#[test]
fn a_revolution_face_given_two_planes_is_refused_unchanged() {
    revolution_built_sound_or_refused_unchanged(
        &[
            (2, -0.536_542_413_140_470_6, [0.0; 3]),
            (2, -1.250_163_666_404_037_9, [0.0; 3]),
            (
                1,
                0.0,
                [
                    0.019_211_488_598_403_77,
                    0.247_784_794_496_732_8,
                    0.166_922_271_536_784_85,
                ],
            ),
        ],
        false,
    );
}

/// `gen0050`: two neighbouring revolution faces given planes, moved out, built.
#[test]
fn two_revolution_faces_given_planes_build_a_sound_solid() {
    revolution_built_sound_or_refused_unchanged(
        &[
            (0, 0.701_236_569_338_806_1, [0.0; 3]),
            (1, 1.302_451_597_675_177_4, [0.0; 3]),
        ],
        true,
    );
}
