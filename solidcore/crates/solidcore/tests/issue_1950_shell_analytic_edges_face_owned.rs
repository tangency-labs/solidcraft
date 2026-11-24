//! Regression test for tangency-labs/solidcore#1950: WR-545's
//! `make_edges_on_analytics_exact` (the fork's tangency-labs/smlib@cd85d34; once the
//! `shell_edges_on_analytics_exact` flag, now the end of every shell) on face-owned surfaces. `MakeEdgesOnAnalyticsExact`
//! intersects the surfaces of the two faces of each marched edge. Upstream those are the
//! faces' own surfaces, whose caches resolve through their faces. Before #1963 (tdamsma)
//! the port intersected them without the B-rep's face services and stopped on `Unported`
//! (a trimmed cache of a face-owned surface, `TrimSrfCache::build_tree_with_face`, with
//! SolidCraft's fixes; the face handle of a surface's cache with the defaults). #1963
//! tests shell/566 and shell/1525; this is the third case of #1950. SolidCraft's plate: the profile
//! (0, 0)-(42.5, 0)-(42.5, 20)-(0, 20) closed by a half circle r 10 on the x < 0 side,
//! extruded 27.5, less a blind hole r 5 along x through the arc side, shelled 2 inward
//! open at x = 42.5. SolidCraft's rig gives V 10400.2131 without the flag; with it the
//! fork's C++ fails its plate with 1001 (#1950), which this reconstruction (the
//! `issue_1795` operand) does not reproduce: the port shells it, within 1e-7 of its
//! volume without the flag.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;
#[path = "support/solidcraft_fixes.rs"]
mod solidcraft_fixes;

#[path = "support/volume_leak.rs"]
mod volume_leak;
use offset_fixture::{planar_faces_at, shell, volume};
use solidcore::boolean::api_brep as boolean;
use solidcore::curve::curve::Curve;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::UpstreamFixes;
use solidcore::primitive::api_curves as curves;
use solidcore::primitive::api_primitives as primitives;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::sweep::api_brep as sweeps;
use solidcore::topology::proto_topology::p1::api_transform::{api_rotate, TransformObject};
use solidcore::{Brep, Kernel};
use solidcraft_fixes::solidcraft_fixes;

/// SolidCraft's volume of the shelled plate (without the flag).
const RIG_VOLUME: f64 = 10400.2131;

/// The shelled plate's closed form: the plate (profile area 850 + 50 pi, height 27.5) less
/// the blind hole (r 5, from the arc r 10 to x = 7.5) less the cavity (the profile inset 2,
/// 680 + 32 pi, height 23.5, open at x = 42.5) with the hole's offset in it (r 7, from the
/// arc r 8 to x = 9.5, cut by the cavity's y = 18 wall). The hole and the offset hole are
/// one-dimensional integrals of a disk's band area, taken by tanh-sinh quadrature on their
/// smooth pieces to 1e-12 (notes only).
const CLOSED_FORM: f64 = 10400.212373846494;

/// The plate (as `issue_1795_shell_hole_through_arc.rs` builds the #1528 operand).
fn drilled(k: &mut Kernel) -> Brep {
    let p = |x: f64, y: f64| Vec3::new(x, y, 0.0);
    let cs: Vec<Curve> = vec![
        Curve::from(curves::create_line_segment(p(0.0, 0.0), p(42.5, 0.0)).unwrap()),
        Curve::from(curves::create_line_segment(p(42.5, 0.0), p(42.5, 20.0)).unwrap()),
        Curve::from(curves::create_line_segment(p(42.5, 20.0), p(0.0, 20.0)).unwrap()),
        Curve::from(curves::create_arc(p(0.0, 10.0), 10.0, 90.0, 270.0).unwrap()),
    ];
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    let body = sweeps::create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 27.5, true).unwrap();
    // a cylinder r 5 along z from (-10, 12.5, 12.5), turned onto +x about its base
    let mut drill =
        primitives::create_cylinder(k, Point3::new(-10.0, 12.5, 12.5), 5.0, 17.5).unwrap();
    api_rotate(
        k,
        Some(TransformObject::Brep(&mut drill)),
        Point3::new(-10.0, 12.5, 12.5),
        Vec3::new(0.0, 1.0, 0.0),
        90.0,
    )
    .unwrap();
    let mut b = boolean::boolean_difference(k, body, drill).unwrap();
    b.refresh_cached_topology();
    b
}

/// The plate shelled 2 inward, open at x = 42.5, under `fixes`: a manifold solid and its
/// volume.
/// `was`: the volume before `brep_volume` closed tolerant edges' gaps (#16735), the move
/// checked within the old integrator's leak bound on the body.
fn shelled_volume(fixes: &UpstreamFixes, was: f64) -> f64 {
    let mut k = Kernel {
        fixes: *fixes,
        ..Kernel::default()
    };
    let b = drilled(&mut k);
    let open = planar_faces_at(&k, &b, 0, 42.5);
    assert_eq!(open.len(), 1, "the x = 42.5 face");
    let mut r = shell(&mut k, b, -2.0, &open).unwrap();
    r.refresh_cached_topology();
    assert_eq!(r.face_list().len(), 16);
    assert!(brep_is_manifold_solid(&r).unwrap());
    let v = volume(&k, &mut r).unwrap();
    volume_leak::assert_moved_by_the_old_leak(&k, &mut r, was, v, 1e-6);
    v
}

fn check(v: f64, want: f64) {
    assert!(
        (v - RIG_VOLUME).abs() <= 1e-6 * RIG_VOLUME,
        "{v} vs the rig's"
    );
    assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    // within the volume's accuracy (1e-6) of the closed form
    assert!(
        (v - CLOSED_FORM).abs() <= 1e-6 * CLOSED_FORM,
        "{v} vs {CLOSED_FORM}"
    );
}

/// With SolidCraft's fixes (the reported configuration; the trimmed cache stub before).
/// The shell runs `make_edges_on_analytics_exact` through the face-owned surfaces; "with
/// the flag" below is that path, the only one since the flag was retired (WR-545), and
/// "without" the marched edges it replaced. Pinned since #2890 (precise properties integrate from the trims' base): against the
/// closed form, without the flag +4.26e-4 (it was 10400.212787697543, +4.14e-4), with it
/// +4.26e-4, within the volume's accuracy. With the flag it was 10400.212354107192
/// (-1.97e-5) before the certified offset became the default (#13827): the shell's
/// non-analytic offsets are certified nets, not `N_SrfOffset`'s fits, and the flag now
/// moves the volume by 1.5e-12 relative. The shell still runs the flag through the
/// face-owned surfaces. Since `brep_volume` closes tolerant edges' gaps (#16735), +6.1e-5
/// from the closed form with and without the flag (it was 10400.212799740975 and
/// 10400.212799756418, +4.26e-4: the old integrator's leak, each move checked on the body).
#[test]
fn shelled_plate_with_solidcraft_fixes() {
    let mut fixes = solidcraft_fixes();
    // Off, as in the list this test copied before: with it the extruded half circle is a
    // cylinder rather than a surface of extrusion, so the operand is not the one pinned below
    // (its shell measures 10400.213233570486, +8.6e-4 against the closed form, 8.3e-8
    // relative, within the volume's 1e-6).
    fixes.linear_sweep_arc_is_cylinder = false;
    check(
        shelled_volume(&fixes, 10400.212799756418),
        10400.212434834219,
    );
}

/// With the kernel's defaults ("with the flag" is the only path now, as above). Pinned
/// since #2890: against the closed form, without the flag
/// +4.84e-4 (it was 10400.212793536877, +4.20e-4), with it +4.80e-4, each within the
/// volume's accuracy. With the flag it was 10400.212413698822 (+3.99e-5) before the
/// certified offset became the default (#13827), as above; the flag now moves the volume
/// by 3.1e-10 relative. Since `brep_volume` closes tolerant edges' gaps (#16735), +5.8e-5
/// and +6.4e-5 (it was 10400.212857439197 and 10400.212854198868), each move checked
/// within the old integrator's leak bound on the body ([`shelled_volume`]).
#[test]
fn shelled_plate_with_the_defaults() {
    check(
        shelled_volume(&UpstreamFixes::default(), 10400.212854198868),
        10400.212438244882,
    );
}
