//! Shells, face offsets, thickening and sheet offsets against closed forms (the cases of
//! SolidCraft's `core/offset_tests.rs`, on face indices).

use std::f64::consts::PI;

use solidcore::geom::extent3d::Extent3d;
use solidcore::kernel::Kernel;
use solidcore::query::api_queries::face_bounding_box;

use super::{offset_faces, offset_sheet, shell, thicken};
use crate::session::{self, run};
use crate::{make_box, make_cylinder, make_sphere, Body, Error};

fn all_fixes() {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

/// The 30 x 20 x 10 block at the origin.
fn block() -> Body {
    make_box([0.0; 3], [30.0, 20.0, 10.0]).unwrap()
}

const ABC: f64 = 6000.0;

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1.0)
}

fn volume(b: &Body) -> f64 {
    b.volume(1e-8).unwrap()
}

/// The index of the face of `b` whose tight box `pick` accepts.
fn face(b: &Body, pick: impl Fn(&Extent3d) -> bool) -> usize {
    let k = Kernel::default();
    let brep = b.lock();
    let faces = brep.face_list().to_vec();
    faces
        .iter()
        .position(|&f| pick(&face_bounding_box(&k, &brep, f, true).unwrap()))
        .unwrap()
}

/// The block's top, `z = 10`.
fn top(b: &Body) -> usize {
    face(b, |bx| {
        (bx.min().z - 10.0).abs() < 1e-9 && (bx.max().z - 10.0).abs() < 1e-9
    })
}

#[test]
fn a_block_shells_to_its_closed_forms() {
    all_fixes();
    // inward 2, open at the top: abc - (a - 4)(b - 4)(c - 2)
    let b = block();
    shell(&b, &[top(&b)], -2.0).unwrap();
    assert!(rel(volume(&b), ABC - 26.0 * 16.0 * 8.0) < 1e-9);
    // closed: (a - 4)(b - 4)(c - 4) hollow
    let b = block();
    shell(&b, &[], -2.0).unwrap();
    assert!(rel(volume(&b), ABC - 26.0 * 16.0 * 6.0) < 1e-9);
    // outward 1, the convex edges extended: (a + 2t)(b + 2t)(c + t) - abc
    let b = block();
    shell(&b, &[top(&b)], 1.0).unwrap();
    assert!(rel(volume(&b), 32.0 * 22.0 * 11.0 - ABC) < 1e-9);
}

#[test]
fn a_cylinder_shells_open_at_its_top() {
    all_fixes();
    // r 5, h 10, t 1: pi r^2 h - pi (r - t)^2 (h - t)
    let c = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
    let at = face(&c, |bx| {
        (bx.min().z - 10.0).abs() < 1e-9 && (bx.max().z - 10.0).abs() < 1e-9
    });
    shell(&c, &[at], -1.0).unwrap();
    assert!(rel(volume(&c), PI * 25.0 * 10.0 - PI * 16.0 * 9.0) < 1e-9);
}

#[test]
fn a_wall_with_no_room_is_refused_before_the_kernel() {
    all_fixes();
    // a wall of 15 inside a block 20 wide leaves no cavity
    let b = block();
    assert_eq!(shell(&b, &[top(&b)], -15.0), Err(Error::NoRoom));
    // the top moved in 10, as deep as the block: nothing behind it
    let b = block();
    assert_eq!(offset_faces(&b, &[top(&b)], -10.0), Err(Error::NoRoom));
}

#[test]
fn the_top_moves_out() {
    all_fixes();
    let b = block();
    offset_faces(&b, &[top(&b)], 2.0).unwrap();
    assert!(rel(volume(&b), ABC + 2.0 * 600.0) < 1e-9);
}

#[test]
fn hostile_arguments_are_refused() {
    all_fixes();
    let b = block();
    for t in [0.0, f64::NAN, 2e6] {
        assert_eq!(shell(&b, &[], t), Err(Error::Argument));
        assert_eq!(offset_faces(&b, &[0], t), Err(Error::Argument));
        assert_eq!(thicken(&b, t, false), Err(Error::Argument));
        assert_eq!(offset_sheet(&b, t), Err(Error::Argument));
    }
    assert_eq!(
        shell(&b, &[6], -1.0),
        Err(Error::Argument),
        "an index past the faces"
    );
    assert_eq!(
        shell(&b, &[1, 1], -1.0),
        Err(Error::Argument),
        "an index twice"
    );
    assert_eq!(offset_faces(&b, &[], 1.0), Err(Error::Argument), "no faces");
    assert_eq!(
        thicken(&b, 2.0, false),
        Err(Error::Argument),
        "a solid is not thickened"
    );
    assert_eq!(
        volume(&b),
        volume(&block()),
        "refused before the kernel, the body is as it was"
    );
}

/// The 30 x 20 plate in z = 0 (the x axis swept along y).
fn plate() -> Body {
    run("plate", |k| {
        let line = solidcore::curve::line::Line::create_line_segment(
            3,
            solidcore::Vec3::ZERO,
            solidcore::Vec3::new(30.0, 0.0, 0.0),
            None,
        )?;
        let c = solidcore::curve::curve::Curve::from(line);
        solidcore::sweep::api_brep::create_linear_sweep(
            k,
            &[Some(&c)],
            solidcore::Vec3::new(0.0, 1.0, 0.0),
            20.0,
            false,
        )
    })
    .map(Body::new)
    .unwrap()
}

#[test]
fn a_plate_thickens_and_offsets() {
    all_fixes();
    let p = plate();
    thicken(&p, 2.0, false).unwrap();
    assert!(rel(volume(&p), 1200.0) < 1e-9);
    let p = plate();
    thicken(&p, 2.0, true).unwrap();
    assert!(rel(volume(&p), 1200.0) < 1e-9);
    assert_eq!(p.counts().unwrap()[0], 6);
}

#[test]
fn a_ball_offsets_as_a_sheet() {
    all_fixes();
    // r 10 moved out 2: the ball of radius 12
    let s = make_sphere([0.0; 3], 10.0).unwrap();
    offset_sheet(&s, 2.0).unwrap();
    assert!(rel(volume(&s), 4.0 / 3.0 * PI * 1728.0) < 1e-6);
    // moved in 10, to its centre: nothing is left
    let s = make_sphere([0.0; 3], 10.0).unwrap();
    assert_eq!(offset_sheet(&s, -10.0), Err(Error::Empty));
}

/// A Y of three walls meeting along one edge (a sheet, not manifold there): thickened, its
/// result is no closed solid. Refused as `NotSolid` (with the input's contract it passed, the
/// input being a sheet), the sheet as it came in.
#[test]
fn a_thickened_sheet_that_is_no_solid_is_refused() {
    all_fixes();
    let o = solidcore::Vec3::ZERO;
    let arms = [
        solidcore::Vec3::new(10.0, 0.0, 0.0),
        solidcore::Vec3::new(-5.0, 8.0, 0.0),
        solidcore::Vec3::new(-5.0, -8.0, 0.0),
    ];
    let y = run("y", |k| {
        let cs: Vec<solidcore::curve::curve::Curve> = arms
            .iter()
            .map(|&a| {
                solidcore::curve::line::Line::create_line_segment(3, o, a, None)
                    .map(solidcore::curve::curve::Curve::from)
            })
            .collect::<solidcore::Result<_>>()?;
        let refs: Vec<Option<&solidcore::curve::curve::Curve>> = cs.iter().map(Some).collect();
        solidcore::sweep::api_brep::create_linear_sweep(
            k,
            &refs,
            solidcore::Vec3::new(0.0, 0.0, 1.0),
            10.0,
            false,
        )
    })
    .map(Body::new)
    .unwrap();
    let before = y.to_smb(false).unwrap();
    assert_eq!(thicken(&y, 1.0, false), Err(Error::NotSolid));
    assert_eq!(y.to_smb(false).unwrap(), before);
}

/// The offset statuses as the operations' errors: walls that touch, a wall that folds (with
/// the largest that works), a vertex moved past an edge's other end; others as the kernel's.
#[test]
fn the_offset_refusals_are_typed() {
    use solidcore::error::{SM_ERR_OFFSET_EDGE_VANISHES, SM_ERR_OFFSET_WALLS_TOUCH};
    use solidcore::offset::construction::ShellRefusal;
    use solidcore::SolidError;
    let up = |code| SolidError::Upstream {
        operation: "test",
        code,
        entity: None,
    };
    assert_eq!(
        super::status(up(SM_ERR_OFFSET_WALLS_TOUCH)),
        Error::WallsTouch
    );
    assert!(matches!(
        super::status(up(SM_ERR_OFFSET_EDGE_VANISHES)),
        Error::Unsupported(_)
    ));
    assert_eq!(
        super::status(SolidError::ShellRefused {
            operation: "test",
            code: 1001,
            entity: None,
            refusal: ShellRefusal::Fold,
            size: Some(10.0),
        }),
        Error::WallFolds {
            largest: Some(10.0)
        }
    );
    assert_eq!(super::status(up(1001)), Error::Status(1001));
}

/// A face whose surface the kernel's room test cannot measure (an offset surface): the room
/// test's answer maps to `Unsupported`, not to a room or `NoRoom`.
#[test]
fn an_unmeasurable_face_is_unsupported() {
    all_fixes();
    let r = run("unmeasurable", |k| {
        let mut b = solidcore::primitive::api_primitives::create_box(
            k,
            solidcore::Vec3::ZERO,
            20.0,
            20.0,
            20.0,
        )?;
        let f = b.face_list()[0];
        let base = b
            .face(f)?
            .surface
            .clone()
            .ok_or(solidcore::SolidError::sm_err("test"))?;
        let off = solidcore::surface::offset_surface::OffsetSurface::new(k, 0.0, base, true, 0)?;
        b.face_mut(f)?.surface = Some(solidcore::surface::surface::Surface::from(off));
        super::room(k, &mut b, &[], 2.0)
    })
    .unwrap();
    assert!(matches!(r, Err(Error::Unsupported(_))), "{r:?}");
}

/// A block with every face flipped is manifold but faces inward: an inward shell and faces
/// moved in are refused (the room test would read "inward" outward), the body as it came in.
#[test]
fn an_inside_out_body_is_refused() {
    all_fixes();
    let b = make_box([0.0; 3], [20.0, 20.0, 20.0]).unwrap();
    {
        let mut br = b.lock();
        for f in br.face_list().to_vec() {
            solidcore::topology::face::p1::flip_face_orientation(&mut br, f).unwrap();
        }
    }
    let before = b.to_smb(false).unwrap();
    assert_eq!(shell(&b, &[], -15.0), Err(Error::Argument));
    assert_eq!(offset_faces(&b, &[0], -2.0), Err(Error::Argument));
    assert_eq!(b.to_smb(false).unwrap(), before);
}
