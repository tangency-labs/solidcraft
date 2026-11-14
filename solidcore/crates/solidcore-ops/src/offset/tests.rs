//! Shells, face offsets, thickening and sheet offsets against closed forms (the cases of
//! SolidCraft's `core/offset_tests.rs`, on record face indices through the body's `Order`).

use std::f64::consts::PI;

use solidcore::geom::extent3d::Extent3d;
use solidcore::kernel::Kernel;
use solidcore::query::api_queries::face_bounding_box;

use super::{offset_faces, offset_sheet, shell, thicken};
use crate::session::{self, run};
use crate::{make_box, make_cylinder, make_sphere, Body, Error, Order};

fn all_fixes() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
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

/// The record order of `b`: record face `i` is the kernel face `order.faces[i]`.
fn order(b: &Body) -> Order {
    b.to_record().unwrap().1
}

/// The record index (through `order`) of the face of `b` whose tight box `pick` accepts.
fn face(b: &Body, order: &Order, pick: impl Fn(&Extent3d) -> bool) -> usize {
    let k = Kernel::default();
    let brep = b.lock();
    order
        .faces
        .iter()
        .position(|&f| pick(&face_bounding_box(&k, &brep, f, true).unwrap()))
        .unwrap()
}

/// The block's top, `z = 10`, by its record index.
fn top(b: &Body, order: &Order) -> usize {
    face(b, order, |bx| {
        (bx.min().z - 10.0).abs() < 1e-9 && (bx.max().z - 10.0).abs() < 1e-9
    })
}

#[test]
fn a_block_shells_to_its_closed_forms() {
    all_fixes();
    // inward 2, open at the top: abc - (a - 4)(b - 4)(c - 2)
    let b = block();
    let o = order(&b);
    shell(&b, &o, &[top(&b, &o)], -2.0).unwrap();
    assert!(rel(volume(&b), ABC - 26.0 * 16.0 * 8.0) < 1e-9);
    // closed: (a - 4)(b - 4)(c - 4) hollow
    let b = block();
    shell(&b, &order(&b), &[], -2.0).unwrap();
    assert!(rel(volume(&b), ABC - 26.0 * 16.0 * 6.0) < 1e-9);
    // outward 1, the convex edges extended: (a + 2t)(b + 2t)(c + t) - abc
    let b = block();
    let o = order(&b);
    shell(&b, &o, &[top(&b, &o)], 1.0).unwrap();
    assert!(rel(volume(&b), 32.0 * 22.0 * 11.0 - ABC) < 1e-9);
}

#[test]
fn a_cylinder_shells_open_at_its_top() {
    all_fixes();
    // r 5, h 10, t 1: pi r^2 h - pi (r - t)^2 (h - t)
    let c = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
    let o = order(&c);
    let at = face(&c, &o, |bx| {
        (bx.min().z - 10.0).abs() < 1e-9 && (bx.max().z - 10.0).abs() < 1e-9
    });
    shell(&c, &o, &[at], -1.0).unwrap();
    assert!(rel(volume(&c), PI * 25.0 * 10.0 - PI * 16.0 * 9.0) < 1e-9);
}

/// A basin: a 100 x 60 x 30 slab less a ruled pocket (60 x 46 at the top, 4 from the front, 40 x
/// 20 at its floor 20 down), shelled open at its bottom, each wall moved in by t with sharp
/// edges. The cavity is the slab shrunk by t (open at z = 0) less the pocket grown by t, each
/// sloped side moved out along its normal; the volume, slab - pocket - cavity, is a cubic in z
/// integrated by hand. From t ~ 2.70 the front side's offset passes the front wall's below the
/// 4 mm rim, and the rim stays solid: the cavity's front stops at y = t (from
/// z = (28 - (1 + sqrt 1.64) t) / 0.8 up).
#[test]
#[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
fn a_basin_shells_past_its_rim_to_its_closed_form() {
    session::set_thread_fixes(
        solidcore::kernel::UpstreamFixes::NAMES
            .iter()
            .copied()
            .filter(|n| {
                !matches!(
                    *n,
                    "fillet_refuse_rounds_breaking_through" | "fillet_round_ends_on_outline"
                )
            }),
    )
    .unwrap();
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64, z: f64| {
        let p = [[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]];
        (0..4)
            .map(|i| crate::sweep::Curve::Line {
                start: p[i],
                end: p[(i + 1) % 4],
            })
            .collect::<Vec<_>>()
    };
    let pocket = crate::sweep::loft(
        &[
            rect(20.0, 4.0, 80.0, 50.0, 30.0),
            rect(30.0, 20.0, 70.0, 40.0, 10.0),
        ],
        1,
        true,
    )
    .unwrap();
    let slab = make_box([0.0; 3], [100.0, 60.0, 30.0]).unwrap();
    let basin = crate::boolean::boolean(slab, pocket, crate::BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    // The pocket grown by t at height z: x from 35 - z/2 - t sqrt5/2 to 65 + z/2 + t sqrt5/2, y
    // from 28 - 0.8 z - t sqrt1.64 (or y = t, where that is less) to 35 + z/2 + t sqrt5/2, for
    // z = 10 - t ... 30 - t; its sides stay inside the shrunk slab in x and at the back for t <= 6.
    let (r5, r164) = (5f64.sqrt(), 1.64f64.sqrt());
    let integral = |c: [f64; 3], lo: f64, hi: f64| {
        c[0] * (hi - lo) + c[1] * (hi * hi - lo * lo) / 2.0 + c[2] * (hi.powi(3) - lo.powi(3)) / 3.0
    };
    // (w0 + z)(h0 + h1 z) as coefficients in z.
    let product = |w0: f64, h0: f64, h1: f64| [w0 * h0, w0 * h1 + h0, h1];
    let grown = |t: f64| {
        let (lo, hi, w0) = (10.0 - t, 30.0 - t, 30.0 + r5 * t);
        let full = product(w0, 7.0 + t * (r5 / 2.0 + r164), 1.3);
        let z0 = ((28.0 - r164 * t - t) / 0.8).clamp(lo, hi);
        integral(full, lo, z0) + integral(product(w0, 35.0 + (r5 / 2.0 - 1.0) * t, 0.5), z0, hi)
    };
    let closed = |t: f64| {
        100.0 * 60.0 * 30.0 - grown(0.0) - (100.0 - 2.0 * t) * (60.0 - 2.0 * t) * (30.0 - t)
            + grown(t)
    };
    assert!(rel(volume(&basin), 180_000.0 - grown(0.0)) < 1e-9);
    for (t, want) in [
        (1.0, 17_105.624_041_5),
        (4.0, 63_560.249_228_4),
        (6.0, 90_113.920_210_1),
    ] {
        assert!(rel(closed(t), want) < 1e-12, "the closed form at {t}");
        let b = basin.copy().unwrap();
        let o = order(&b);
        let bottom = face(&b, &o, |bx| {
            bx.min().z.abs() < 1e-9 && bx.max().z.abs() < 1e-9
        });
        shell(&b, &o, &[bottom], -t).unwrap_or_else(|e| panic!("t {t}: {e:?}"));
        assert!(
            rel(volume(&b), want) < 1e-6,
            "t {t}: {} vs {want}",
            volume(&b)
        );
        let report = b.check(crate::CheckLevel::All).unwrap();
        assert!(report.is_valid(), "t {t}: {}", report.text);
    }
}

#[test]
fn a_wall_with_no_room_is_refused_before_the_kernel() {
    all_fixes();
    // a wall of 15 inside a block 20 wide leaves no cavity
    let b = block();
    let o = order(&b);
    assert_eq!(shell(&b, &o, &[top(&b, &o)], -15.0), Err(Error::NoRoom));
    // the top moved in 10, as deep as the block: nothing behind it
    let b = block();
    let o = order(&b);
    assert_eq!(
        offset_faces(&b, &o, &[top(&b, &o)], -10.0),
        Err(Error::NoRoom)
    );
}

#[test]
fn the_top_moves_out() {
    all_fixes();
    let b = block();
    let o = order(&b);
    offset_faces(&b, &o, &[top(&b, &o)], 2.0).unwrap();
    assert!(rel(volume(&b), ABC + 2.0 * 600.0) < 1e-9);
}

#[test]
fn hostile_arguments_are_refused() {
    all_fixes();
    let b = block();
    let o = order(&b);
    for t in [0.0, f64::NAN, 2e6] {
        assert_eq!(shell(&b, &o, &[], t), Err(Error::Argument));
        assert_eq!(offset_faces(&b, &o, &[0], t), Err(Error::Argument));
        assert_eq!(thicken(&b, t, false), Err(Error::Argument));
        assert_eq!(offset_sheet(&b, t), Err(Error::Argument));
    }
    assert_eq!(
        shell(&b, &o, &[6], -1.0),
        Err(Error::Argument),
        "an index past the faces"
    );
    assert_eq!(
        shell(&b, &o, &[1, 1], -1.0),
        Err(Error::Argument),
        "an index twice"
    );
    assert_eq!(
        offset_faces(&b, &o, &[], 1.0),
        Err(Error::Argument),
        "no faces"
    );
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
    use solidcore::error::offset::OffsetFailure;
    use solidcore::error::Failure;
    use solidcore::error::{SM_ERR_OFFSET_EDGE_VANISHES, SM_ERR_OFFSET_WALLS_TOUCH};
    use solidcore::{FaceId, SolidError};
    let none = |_: FaceId| -> Option<usize> { None };
    let up = |code| SolidError::Upstream {
        operation: "test",
        code,
        entity: None,
    };
    assert_eq!(
        super::status(up(SM_ERR_OFFSET_WALLS_TOUCH), &none),
        Error::WallsTouch
    );
    assert_eq!(
        super::status(up(SM_ERR_OFFSET_EDGE_VANISHES), &none),
        Error::OffsetRefused {
            face: None,
            why: crate::OffsetRefusal::EdgeVanishes,
        }
    );
    // the kernel's typed offset refusals (solidcore#2615)
    let refused = |o| Failure::new("test", o).into_error();
    assert_eq!(
        super::status(
            refused(OffsetFailure::Folds {
                face: FaceId::from_index(3),
                distance: -11.0,
                largest: 10.0,
            }),
            &none,
        ),
        Error::WallFolds {
            largest: Some(10.0)
        }
    );
    assert_eq!(
        super::status(
            refused(OffsetFailure::WallsTouch {
                face: None,
                contact: solidcore::error::offset::WallContact::Touch,
            }),
            &none,
        ),
        Error::WallsTouch
    );
    assert_eq!(
        super::status(
            refused(OffsetFailure::EdgeVanishes {
                edge: None,
                face: None,
            }),
            &none,
        ),
        Error::OffsetRefused {
            face: None,
            why: crate::OffsetRefusal::EdgeVanishes,
        }
    );
    // an unresolved crossing says how it crosses (#14783), no longer a bare status
    assert_eq!(
        super::status(
            refused(OffsetFailure::LoopsCross {
                face: None,
                crossing: solidcore::error::offset::LoopCrossing::AtLoopVertex,
            }),
            &none,
        ),
        Error::OffsetRefused {
            face: None,
            why: crate::OffsetRefusal::LoopsCross {
                crossing: solidcore::error::offset::LoopCrossing::AtLoopVertex,
            },
        }
    );
    assert_eq!(super::status(up(1001), &none), Error::Status(1001));
}

/// Each of the kernel's other offset refusals as [`Error::OffsetRefused`] (#14783): its reason,
/// and its face by the index the call's lookup gives, `None` for a face it does not find or a
/// refusal that names none. Never the bare status they were before, and never the kernel's text
/// in place of a reason (#15035): a routine's own failure keeps its kind and the kernel's phrase.
#[test]
fn every_other_offset_refusal_says_why_and_where() {
    use crate::OffsetRefusal as R;
    use solidcore::error::offset::{LoopCrossing, OffsetFailure as O, Unsoundness};
    use solidcore::error::ErrorKind;
    use solidcore::error::Failure;
    use solidcore::FaceId;
    let named = FaceId::from_index(3);
    let other = FaceId::from_index(4);
    // the call's lookup: face 3 is record face 7, face 4 is not the body's
    let index = |f: FaceId| -> Option<usize> { (f == named).then_some(7) };
    let refused = |o: O| super::status(Failure::new("test", o).into_error(), &index);
    let cases = [
        // (the kernel's fold kind, but no fold: its own reason, not `WallFolds`)
        (
            O::NotCertified {
                face: named,
                distance: -2.0,
            },
            Some(7),
            R::NotCertified { distance: -2.0 },
        ),
        (
            O::Collapses {
                face: named,
                distance: -5.0,
                radius: 5.0,
            },
            Some(7),
            R::Collapses {
                distance: -5.0,
                radius: 5.0,
            },
        ),
        (
            O::LoopsCross {
                face: Some(named),
                crossing: LoopCrossing::SharesEdge,
            },
            Some(7),
            R::LoopsCross {
                crossing: LoopCrossing::SharesEdge,
            },
        ),
        (
            O::DistanceUnresolved {
                distance: 1e-9,
                resolution: 1e-6,
            },
            None,
            R::DistanceUnresolved {
                distance: 1e-9,
                resolution: 1e-6,
            },
        ),
        (
            O::ToleranceUnresolved { tolerance: 1e-13 },
            None,
            R::ToleranceUnresolved { tolerance: 1e-13 },
        ),
        (O::SeamOpened { face: named }, Some(7), R::SeamOpened),
        (O::ExactCurveUncut, None, R::ExactCurveUncut),
        (O::FacesApart { face: other }, None, R::FacesApart),
        (
            O::ReachUncertified { face: named },
            Some(7),
            R::ReachUncertified,
        ),
        (
            O::ReachUncertified { face: other },
            None,
            R::ReachUncertified,
        ),
        (
            O::LoopGapUncertified { face: Some(named) },
            Some(7),
            R::LoopGapUncertified,
        ),
        (
            O::LoopGapUncertified { face: None },
            None,
            R::LoopGapUncertified,
        ),
        (
            O::Unsound {
                what: Unsoundness::Check {
                    failures: 2,
                    first: Some((1, 4)),
                },
            },
            None,
            R::Unsound {
                what: Unsoundness::Check {
                    failures: 2,
                    first: Some((1, 4)),
                },
            },
        ),
        (
            O::Unsound {
                what: Unsoundness::VolumeUndecided,
            },
            None,
            R::Unsound {
                what: Unsoundness::VolumeUndecided,
            },
        ),
        // a routine's own failure, so far only a phrase: its class, whether it is the kernel's
        // fault, and the kernel's words, as it said them when it arrived as text
        (
            O::Failed("the face has no surface"),
            None,
            R::Phrase {
                class: ErrorKind::Failed,
                defect: false,
                phrase: "the face has no surface".to_owned(),
            },
        ),
        (
            O::Missing("the face's surface"),
            None,
            R::Phrase {
                class: ErrorKind::Missing,
                defect: false,
                phrase: "the face's surface is missing".to_owned(),
            },
        ),
        (
            O::InvalidValue("the face has no outer face use"),
            None,
            R::Phrase {
                class: ErrorKind::InvalidInput,
                defect: false,
                phrase: "invalid input: the face has no outer face use".to_owned(),
            },
        ),
        (
            O::Inconsistent("an offset edge has no curve"),
            None,
            R::Phrase {
                class: ErrorKind::Failed,
                defect: true,
                phrase: "a consistency check failed: an offset edge has no curve".to_owned(),
            },
        ),
    ];
    for (o, face, why) in cases {
        let got = refused(o);
        assert_eq!(got, Error::OffsetRefused { face, why }, "{o:?}");
        assert!(!got.to_string().contains("status"), "{got}");
        // only a phrase arrives as one: every reason the kernel names is named here too
        let Error::OffsetRefused { why, .. } = &got else {
            panic!("not an offset refusal: {o:?}");
        };
        assert_eq!(matches!(why, R::Phrase { .. }), o.is_phrase(), "{o:?}");
    }
    // the face the kernel sets on a failure whose reason names none (a draft's face without a
    // surface) is the one said
    let what = "the face has no surface or no outer face use";
    let at = Failure::new("test", O::InvalidValue(what))
        .at(solidcore::EntityRef::Face(named))
        .into_error();
    assert_eq!(
        super::status(at, &index),
        Error::OffsetRefused {
            face: Some(7),
            why: R::Phrase {
                class: ErrorKind::InvalidInput,
                defect: false,
                phrase: format!("invalid input: {what}"),
            },
        }
    );
}

/// A reason ops does not name yet (one the kernel adds later) is said by its class and in the
/// kernel's words, never a bare status: [`super::unnamed`], the arm every unnamed reason takes,
/// keeps the kernel's class, whether it is a defect, and its own statement, and the error says
/// it.
#[test]
fn an_unnamed_offset_refusal_keeps_the_kernels_words() {
    use solidcore::error::offset::OffsetFailure;
    let o = OffsetFailure::ExactCurveUncut;
    let why = super::unnamed(&o);
    assert_eq!(
        why,
        crate::OffsetRefusal::Phrase {
            class: o.kind(),
            defect: o.is_defect(),
            phrase: o.to_string(),
        }
    );
    assert!(!o.to_string().is_empty());
    // one whose class has an error of its own (a new reason of the walls-touch, fold or
    // vanishing-edge kind) keeps taking it, as it did before it had a reason
    assert_eq!(
        super::by_kind(solidcore::error::ErrorKind::OffsetWallsTouch),
        Some(Error::WallsTouch)
    );
    assert_eq!(
        super::by_kind(solidcore::error::ErrorKind::OffsetFolds),
        Some(Error::WallFolds { largest: None })
    );
    assert_eq!(
        super::by_kind(solidcore::error::ErrorKind::OffsetEdgeVanishes),
        Some(Error::OffsetRefused {
            face: None,
            why: crate::OffsetRefusal::EdgeVanishes,
        })
    );
    assert_eq!(super::by_kind(o.kind()), None);
    let e = Error::OffsetRefused { face: Some(2), why };
    assert_eq!(
        e.to_string(),
        format!("the offset is refused at face 2: {o}")
    );
}

/// `refusal` is what the offset operations give for a kernel error, with any face unnamed: the
/// same routing as `status`, which agrees with it on every kind of error (an offset refusal is
/// typed, in words from its fields; an error with no offset reason is the kernel's status).
#[test]
fn a_kernel_offset_refusal_is_typed_for_a_caller_without_an_order() {
    use solidcore::error::offset::{LoopCrossing, OffsetFailure, Unsoundness};
    use solidcore::error::Failure;
    let face = solidcore::FaceId::from_index(4);
    let failed = |o| Failure::new("offset::offset_executive::crossing_loops", o).into_error();
    let crossing = LoopCrossing::ThreeAtOnePoint;
    let e = super::refusal(&failed(OffsetFailure::LoopsCross {
        face: Some(face),
        crossing,
    }));
    assert_eq!(
        e,
        Some(Error::OffsetRefused {
            face: None,
            why: crate::OffsetRefusal::LoopsCross { crossing }
        })
    );
    assert_eq!(
        e.map(|e| e.to_string()).as_deref(),
        Some("the offset is refused: its offset's loops cross in a way the offset cannot resolve: three loops cross at one point")
    );
    let what = Unsoundness::NotManifoldSolid;
    assert_eq!(
        super::refusal(&failed(OffsetFailure::Unsound { what })).map(|e| e.to_string()),
        Some(format!("the offset is refused: {what}"))
    );
    assert_eq!(
        what.to_string(),
        "the body built is not a manifold solid, as its input was"
    );
    assert_eq!(
        super::refusal(&failed(OffsetFailure::Folds {
            face,
            distance: 2.0,
            largest: 1.0
        })),
        Some(Error::WallFolds { largest: Some(1.0) })
    );
    assert_eq!(super::refusal(&solidcore::SolidError::sm_err("x")), None);
    // `status` and `refusal` agree: `status` is the refusal, else the kernel's status
    let errors = [
        failed(OffsetFailure::Failed("x")),
        failed(OffsetFailure::InvalidValue("x")),
        failed(OffsetFailure::Folds {
            face,
            distance: 2.0,
            largest: 1.0,
        }),
        failed(OffsetFailure::NotCertified {
            face,
            distance: -1.0,
        }),
        failed(OffsetFailure::Collapses {
            face,
            distance: -2.0,
            radius: 1.5,
        }),
        failed(OffsetFailure::LoopsCross {
            face: Some(face),
            crossing,
        }),
        failed(OffsetFailure::SeamOpened { face }),
        failed(OffsetFailure::Unsound { what }),
        failed(OffsetFailure::ExactCurveUncut),
        solidcore::SolidError::sm_err("x"),
        solidcore::SolidError::Upstream {
            operation: "offset::x",
            code: solidcore::error::SM_ERR_OFFSET_WALLS_TOUCH,
            entity: None,
        },
    ];
    for e in errors {
        let expected = super::refusal(&e).unwrap_or_else(|| crate::error::status(e.clone()));
        assert_eq!(super::status(e.clone(), &|_| None), expected, "{e:?}");
    }
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
    let o = order(&b);
    let before = b.to_smb(false).unwrap();
    assert_eq!(shell(&b, &o, &[], -15.0), Err(Error::Argument));
    assert_eq!(offset_faces(&b, &o, &[0], -2.0), Err(Error::Argument));
    assert_eq!(b.to_smb(false).unwrap(), before);
}

/// Drafts against closed forms (SolidCraft's `draft_*` cases, on record face indices), the oblique
/// neutral plane's, and the refusals.
mod drafts {
    use super::{all_fixes, block, face, order, rel, top, volume, ABC};
    use crate::offset::{draft, mark_moved, moved_faces};
    use crate::{make_box, make_cone, make_cylinder, Error, Order};
    use solidcore::geom::attribute::{ids, Attribute, Behavior};
    use solidcore::EntityRef;
    use std::f64::consts::PI;

    const Z: [f64; 3] = [0.0, 0.0, 1.0];

    // The closed forms' angles and tangents as numbers (ops computes no trigonometry, its
    // tests included): each the double nearest its expression.
    /// 5 degrees, radians.
    const FIVE: f64 = 0.087_266_462_599_716_47;
    /// tan 5 degrees.
    const TAN_FIVE: f64 = 0.087_488_663_525_924_01;
    /// 10 degrees, radians.
    const TEN: f64 = 0.174_532_925_199_432_95;
    /// tan 10 degrees.
    const TAN_TEN: f64 = 0.176_326_980_708_464_98;
    /// tan 0.1.
    const TAN_TENTH: f64 = 0.100_334_672_085_450_55;
    /// tan(phi), sin(phi) = sin(5 degrees) / 0.8: the turn about a hinge at sin(alpha) = 0.8
    /// to the pull that leaves a face at 5 degrees to it.
    const TAN_OBLIQUE: f64 = 0.109_597_020_255_368_76;
    /// tan(atan(-0.1) - 5 degrees).
    const TAN_CONE: f64 = -0.189_143_454_329_318_9;

    fn five() -> f64 {
        FIVE
    }

    /// The record index (through `o`) of the block's side flat at `x = at` (`axis` 0) or
    /// `y = at` (1).
    fn side(b: &crate::Body, o: &Order, axis: usize, at: f64) -> usize {
        face(b, o, |bx| {
            let c = |p: solidcore::Vec3| [p.x, p.y][axis];
            (c(bx.min()) - at).abs() < 1e-9 && (c(bx.max()) - at).abs() < 1e-9
        })
    }

    /// The record index (through `o`) of the face of `b` that is no plane: its tight box is
    /// not flat on any axis.
    fn curved(b: &crate::Body, o: &Order) -> usize {
        face(b, o, |bx| {
            let (lo, hi) = (bx.min(), bx.max());
            hi.x - lo.x > 1e-6 && hi.y - lo.y > 1e-6 && hi.z - lo.z > 1e-6
        })
    }

    /// The four sides drafted 5 degrees about the bottom, pulled up, leaning in: the section
    /// at height z is (a - 2 z s)(b - 2 z s), s = tan 5 degrees, so
    /// V = abh - (a + b) h^2 s + (4/3) h^3 s^2.
    #[test]
    fn the_sides_of_a_block() {
        all_fixes();
        let b = block();
        let o = order(&b);
        let sides = [
            side(&b, &o, 0, 0.0),
            side(&b, &o, 0, 30.0),
            side(&b, &o, 1, 0.0),
            side(&b, &o, 1, 20.0),
        ];
        draft(&b, &o, &sides, [0.0; 3], Z, Z, five()).unwrap();
        let (a, w, h, s) = (30.0, 20.0, 10.0, TAN_FIVE);
        let v = a * w * h - (a + w) * h * h * s + 4.0 / 3.0 * h * h * h * s * s;
        assert!(rel(volume(&b), v) < 1e-9, "{} {v}", volume(&b));
        assert_eq!(b.counts().unwrap()[0], 6);
    }

    /// The front (y = 0) alone: a wedge of a h^2 tan(5 degrees) / 2 comes off; a negative angle
    /// puts it on; hinged at the top, leaning in going up, the face leans out below the hinge
    /// and the wedge goes on.
    #[test]
    fn one_side_in_out_and_hinged_at_the_top() {
        all_fixes();
        let wedge = 30.0 * 100.0 * TAN_FIVE / 2.0;
        for (origin, angle, v) in [
            ([0.0; 3], five(), ABC - wedge),
            ([0.0; 3], -five(), ABC + wedge),
            ([0.0, 0.0, 10.0], five(), ABC + wedge),
        ] {
            let b = block();
            let o = order(&b);
            draft(&b, &o, &[side(&b, &o, 1, 0.0)], origin, Z, Z, angle).unwrap();
            assert!(
                rel(volume(&b), v) < 1e-9,
                "{origin:?} {angle}: {}",
                volume(&b)
            );
        }
    }

    /// The rig's two drafts (booleans/cov-draft-two-adjacent-faces-meet-at-a-corner, and the
    /// draft step of fillets/4989-chamfer-top-edge-of-drafted-face): the 40 x 30 x 20 block's
    /// front, and its front and right, drafted 10 degrees about its base. The section at height
    /// z is (40 - z t)(30 - z t), t = tan 10 degrees, for both, (40)(30 - z t) for the front;
    /// six faces either way.
    #[test]
    fn the_rigs_drafts() {
        all_fixes();
        let (ten, t) = (TEN, TAN_TEN);
        let both = 24000.0 - 70.0 * t * 200.0 + t * t * 8000.0 / 3.0;
        let front = 24000.0 - 0.5 * 20.0 * (20.0 * t) * 40.0;
        for (right, v) in [(true, both), (false, front)] {
            let b = make_box([0.0; 3], [40.0, 30.0, 20.0]).unwrap();
            let o = order(&b);
            let mut faces = vec![side(&b, &o, 1, 0.0)];
            if right {
                faces.push(side(&b, &o, 0, 40.0));
            }
            draft(&b, &o, &faces, [0.0; 3], Z, Z, ten).unwrap();
            let got = volume(&b);
            println!("rig draft (right {right}): volume {got:.17e}, closed form {v:.17e}");
            assert!(rel(got, v) < 1e-12, "{got} {v}");
            assert_eq!(b.counts().unwrap()[0], 6);
        }
    }

    /// The neutral plane oblique to the pull, normal N = (0, 0.6, 0.8) through the origin: the
    /// side x = 30 turns about the line where it meets it (x = 30, 0.6 y + 0.8 z = 0), which is
    /// at sin(alpha) = 0.8 to the pull, by phi with sin(phi) = sin(5 degrees) / 0.8, so that it
    /// ends at exactly 5 degrees to the pull: the plane x = 30 - tan(phi) (0.6 y + 0.8 z), so
    /// what comes off is tan(phi) times the integral of 0.6 y + 0.8 z over the side,
    /// 1200 + 800.
    #[test]
    fn an_oblique_neutral_plane_hinges_on_its_trace() {
        all_fixes();
        let b = block();
        let o = order(&b);
        draft(
            &b,
            &o,
            &[side(&b, &o, 0, 30.0)],
            [0.0; 3],
            [0.0, 0.6, 0.8],
            Z,
            five(),
        )
        .unwrap();
        let v = ABC - 2000.0 * TAN_OBLIQUE;
        assert!(rel(volume(&b), v) < 1e-9, "{} {v}", volume(&b));
        // a draft steeper than the hinge allows (sin 10 degrees > 0.141) is a typed refusal
        let b = block();
        let o = order(&b);
        assert!(matches!(
            draft(
                &b,
                &o,
                &[side(&b, &o, 0, 30.0)],
                [0.0; 3],
                [0.0, 0.99, 0.141],
                Z,
                TEN
            ),
            Err(Error::Unsupported(_))
        ));
    }

    /// A cylinder along the pull, R 5, h 10, drafted 0.1 about its bottom, leaning in: the
    /// frustum of radii R and R - h tan 0.1.
    #[test]
    fn a_cylinder_drafts_to_a_frustum() {
        all_fixes();
        let c = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
        let o = order(&c);
        draft(&c, &o, &[curved(&c, &o)], [0.0; 3], Z, Z, 0.1).unwrap();
        let (big, h) = (5.0, 10.0);
        let small = big - h * TAN_TENTH;
        let v = PI * h * (big * big + big * small + small * small) / 3.0;
        assert!(rel(volume(&c), v) < 1e-9, "{} {v}", volume(&c));
    }

    /// A cone r 10 at its base to 8 at its top, 20 tall, drafted 5 degrees about its base turns
    /// its generators 5 degrees more: r 10 + 20 tan(atan(-0.1) - 5 degrees) at the top.
    #[test]
    fn a_cone_about_the_pull_turns_its_generators() {
        all_fixes();
        let c = make_cone([0.0; 3], 10.0, 8.0, 20.0).unwrap();
        let o = order(&c);
        draft(&c, &o, &[curved(&c, &o)], [0.0; 3], Z, Z, five()).unwrap();
        let (big, h) = (10.0, 20.0);
        let small = big + h * TAN_CONE;
        let v = PI * h * (big * big + big * small + small * small) / 3.0;
        assert!(rel(volume(&c), v) < 1e-9, "{} {v}", volume(&c));
    }

    /// Refused before the kernel or by its draft, the body as it came in: no neutral normal, no
    /// angle, a right angle, an origin not finite or past 1e9, no faces, an index past the faces
    /// or twice, the pull in the neutral plane (Argument); the top across the pull (Unsupported).
    #[test]
    fn hostile_drafts_are_refused() {
        all_fixes();
        let b = block();
        let o = order(&b);
        let before = b.to_smb(false).unwrap();
        let t = top(&b, &o);
        let s = side(&b, &o, 1, 0.0);
        for (faces, at, n, p, angle) in [
            (vec![s], [0.0; 3], [0.0; 3], Z, 0.1),
            (vec![s], [0.0; 3], Z, [0.0; 3], 0.1),
            (vec![s], [0.0; 3], Z, Z, 0.0),
            (vec![s], [0.0; 3], Z, Z, f64::NAN),
            (vec![s], [0.0; 3], Z, Z, PI / 2.0),
            (vec![s], [0.0; 3], Z, Z, -1.569_796_326_794_896_6),
            (vec![s], [f64::NAN; 3], Z, Z, 0.1),
            (vec![s], [0.0, 2e9, 0.0], Z, Z, 0.1),
            (vec![], [0.0; 3], Z, Z, 0.1),
            (vec![6], [0.0; 3], Z, Z, 0.1),
            (vec![s, s], [0.0; 3], Z, Z, 0.1),
            (vec![s], [0.0; 3], [1.0, 0.0, 0.0], Z, 0.1),
        ] {
            assert_eq!(
                draft(&b, &o, &faces, at, n, p, angle),
                Err(Error::Argument),
                "{faces:?} {at:?} {n:?} {p:?} {angle}"
            );
        }
        assert!(matches!(
            draft(&b, &o, &[t], [0.0; 3], Z, Z, 0.1),
            Err(Error::Unsupported(_))
        ));
        assert_eq!(b.to_smb(false).unwrap(), before);
        // just under the largest angle passes the argument check
        let other = block();
        assert_ne!(
            draft(
                &other,
                &order(&other),
                &[s],
                [0.0; 3],
                Z,
                Z,
                1.569_796_326_794_896
            ),
            Err(Error::Argument)
        );
    }

    /// A body facing in is no solid to draft: refused, as it came in.
    #[test]
    fn an_inside_out_body_is_not_drafted() {
        all_fixes();
        let b = make_box([0.0; 3], [20.0, 20.0, 20.0]).unwrap();
        {
            let mut br = b.lock();
            for f in br.face_list().to_vec() {
                solidcore::topology::face::p1::flip_face_orientation(&mut br, f).unwrap();
            }
        }
        let o = order(&b);
        let before = b.to_smb(false).unwrap();
        assert_eq!(
            draft(&b, &o, &[0], [0.0; 3], Z, Z, 0.1),
            Err(Error::Argument)
        );
        assert_eq!(b.to_smb(false).unwrap(), before);
    }

    /// The moved marks live inside the call: a face marked reads as moved once and the mark is
    /// gone; an unmarked body reads as not moved (the draft's `NoEffect`); a drafted body keeps
    /// no draft mark, and a mark the body already carried (the kernel's `ids::MARK`) stays.
    #[test]
    fn the_moved_marks_live_inside_the_call() {
        all_fixes();
        let b = block();
        {
            let mut br = b.lock();
            assert!(!moved_faces(&mut br).unwrap(), "no face marked");
            let f = br.face_list()[0];
            mark_moved(&mut br, &[f]).unwrap();
            assert!(moved_faces(&mut br).unwrap(), "a face marked");
            assert!(!moved_faces(&mut br).unwrap(), "the mark removed");
        }
        let marks = |b: &crate::Body| -> Vec<Option<i64>> {
            let br = b.lock();
            br.face_list()
                .iter()
                .flat_map(|&f| {
                    br.entity_attributes(EntityRef::Face(f))
                        .into_iter()
                        .filter(|(_, a)| [ids::MARK, ids::DRAFT_MOVED].contains(&a.attribute_id()))
                        .map(|(_, a)| a.long_value())
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        let o = order(&b);
        let s = side(&b, &o, 1, 0.0);
        {
            let mut br = b.lock();
            let f = o.faces[s];
            br.add_attribute(
                EntityRef::Face(f),
                Attribute::long(ids::MARK, 7, Behavior::Copy),
            )
            .unwrap();
        }
        draft(&b, &o, &[s], [0.0; 3], Z, Z, five()).unwrap();
        assert_eq!(marks(&b), vec![Some(7)]);
        let c = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
        let o = order(&c);
        draft(&c, &o, &[curved(&c, &o)], [0.0; 3], Z, Z, 0.1).unwrap();
        assert!(marks(&c).is_empty());
    }

    /// 3 degrees, radians.
    const THREE: f64 = 0.052_359_877_559_829_89;
    /// tan 3 degrees.
    const TAN_THREE: f64 = 0.052_407_779_283_041_21;

    /// A boss r 10 about (20, 20), z 10 to 20, drawn as two faces: a circle extruded as two
    /// half arcs, so its wall is split along the rulings through (10, 20) and (30, 20) and its
    /// top has a vertex on each, where the two wall faces and the top meet.
    fn split_boss(on_block: bool) -> crate::Body {
        use crate::prism::extrude_planar_regions;
        use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2};
        let half = |start: f64| Seg2::Arc {
            center: Vec2::new(20.0, 20.0),
            radius: 10.0,
            start,
            sweep: PI,
        };
        let disc = Region2 {
            outer: Loop2 {
                segs: vec![half(0.0), half(PI)],
            },
            holes: vec![],
        };
        let boss = extrude_planar_regions(&Plane::XY, &[disc], 10.0, 20.0).unwrap();
        if !on_block {
            return boss;
        }
        let block = make_box([0.0; 3], [40.0, 40.0, 10.0]).unwrap();
        crate::boolean::boolean(block, boss, crate::BooleanOp::Union)
            .unwrap()
            .unwrap()
    }

    /// The top of a split boss (alone, and standing on a 40 x 40 x 10 block) drafted 3 degrees
    /// about the line x = 25 on it, pulled along x, leaning in going along x: the top turns
    /// to z = 20 - (x - 25) tan 3 degrees, 5 tan 3 degrees above z = 20 at the axis, so the boss
    /// gains pi r^2 5 tan 3 degrees; leaning out it loses as much. The top's vertices on the
    /// split rulings meet the turned top where the rulings cross it (#3005 refused them: two
    /// faces of one cylinder at a vertex where a plane moves). Valid at every check level.
    ///
    /// The split wall's halves are extended past their rulings, so the top's new edges are
    /// fitted, within the B-rep's tolerance (2e-5) of the ellipse: the volume is held to that
    /// tolerance over the top's area (under 315). The same boss drawn as one face with a seam
    /// keeps an exact ellipse and lands on the closed form to rounding.
    #[test]
    fn the_top_of_a_boss_split_along_its_rulings() {
        all_fixes();
        let x = [1.0, 0.0, 0.0];
        let band = 2e-5 * 315.0;
        for (split, on_block) in [(true, false), (true, true), (false, false)] {
            for (angle, sign) in [(THREE, 1.0), (-THREE, -1.0)] {
                let b = if split {
                    split_boss(on_block)
                } else {
                    make_cylinder([20.0, 20.0, 10.0], 10.0, 10.0).unwrap()
                };
                let before = volume(&b);
                let o = order(&b);
                let top = face(&b, &o, |bx| {
                    (bx.min().z - 20.0).abs() < 1e-9 && (bx.max().z - 20.0).abs() < 1e-9
                });
                draft(&b, &o, &[top], [25.0, 0.0, 0.0], x, x, angle).unwrap();
                let want = before + sign * PI * 10.0 * 10.0 * 5.0 * TAN_THREE;
                let got = volume(&b);
                if split {
                    assert!(
                        (got - want).abs() < band,
                        "{on_block} {angle}: {got} {want}"
                    );
                } else {
                    assert!(rel(got, want) < 1e-9, "seam {angle}: {got} {want}");
                }
                let report = b.check(crate::CheckLevel::All).unwrap();
                assert_eq!(
                    report.failures, 0,
                    "{split} {on_block} {angle}: {}",
                    report.text
                );
            }
        }
    }

    /// A cylinder of radius 10, height 20, split at z = 10 and drafted 3 degrees
    /// on each side away from that circle becomes two joined frusta. With
    /// r = 10 - 10 tan(3 degrees), each has volume pi*10*(100 + 10r + r²)/3.
    /// The coaxial positive-nappe surfaces meet only at their neutral circle.
    #[test]
    fn a_cylinder_parted_both_ways_is_closed_with_the_two_frusta_volume() {
        all_fixes();
        let c = make_cylinder([0.0; 3], 10.0, 20.0).unwrap();
        let o = order(&c);
        let parting = crate::split::Tool {
            surface: solidcore_record::brep::Surface::Plane {
                origin: [0.0, 0.0, 10.0],
                u_axis: [1.0, 0.0, 0.0],
                v_axis: [0.0, 1.0, 0.0],
            },
            domain: [[-20.0, 20.0], [-20.0, 20.0]],
        };
        let (parted, _) = crate::split::split_faces(&c, &o, &[curved(&c, &o)], &[parting])
            .unwrap()
            .unwrap();
        // the record index (through `o`) of the one face of `b` through `p`
        let through = |b: &crate::Body, o: &Order, p: [f64; 3]| {
            let hits: Vec<usize> = (0..o.faces.len())
                .filter(|&i| b.face_distances(o, i, &[p]).unwrap()[0].is_some_and(|d| d < 1e-9))
                .collect();
            assert_eq!(hits.len(), 1, "faces through {p:?}: {hits:?}");
            hits[0]
        };
        let small = 10.0 - 10.0 * TAN_THREE;
        let frustum = PI * 10.0 * (100.0 + 10.0 * small + small * small) / 3.0;
        for (step, (at, pull)) in [([10.0, 0.0, 15.0], Z), ([10.0, 0.0, 5.0], [0.0, 0.0, -1.0])]
            .into_iter()
            .enumerate()
        {
            let o = order(&parted);
            let half = through(&parted, &o, at);
            draft(&parted, &o, &[half], [0.0, 0.0, 10.0], Z, pull, THREE).unwrap();
            let expected = if step == 0 {
                1000.0 * PI + frustum
            } else {
                2.0 * frustum
            };
            let measured = volume(&parted);
            assert!(
                rel(measured, expected) < 1e-9,
                "step {step}: {measured} vs {expected}"
            );
            assert!(parted.is_manifold_solid().unwrap());
            parted.check_solid().unwrap();
            let report = parted.check(crate::CheckLevel::All).unwrap();
            assert_eq!(report.failures, 0, "step {step}: {}", report.text);
            println!(
                "parted cylinder step={step}: measured={measured:.12}, expected={expected:.12}"
            );
        }
    }
}
