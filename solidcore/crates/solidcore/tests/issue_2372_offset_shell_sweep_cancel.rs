//! #2372: offset, shell and sweep poll the kernel's [`CancelToken`] (#2136) in their own
//! loops (the offset executive per face, edge use, vertex and pass; the sweep per
//! repetition, vertex, edge and face), not only inside a boolean or SSI loop they reach.
//!
//! - A token that never fires leaves every result bit for bit as without one (the
//!   volume, every vertex and the faces' points).
//! - A token fired before the call turns it into `Err(Cancelled)`, never `Ok`.
//! - A token fired from another thread while the swept bar of #1476 (SolidCraft's
//!   shell/566) is shelled turns the call into `Err(Cancelled)` soon after the cancel.
//!
//! Expected values: the uncancelled results are compared with themselves (no token
//! against a token that never fires), and were checked against main's bit for bit when
//! this test was written (see #2375).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{api_shell, offset_full, planar_faces_at, volume};
use solidcore::curve::curve::p2::Curve;
use solidcore::error::SolidError;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector2d::Point2;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::sweep::api_brep::{create_linear_sweep, create_rotational_sweep};
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::face::p2 as face;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{vertex, FaceId};
use solidcore::{Brep, CancelToken, EntityRef, Kernel, Result};

/// `SmBrep::ReadFromFile` of #1476's operand (the rig's working copy of shell/566).
fn swept_bar(k: &mut Kernel) -> Brep {
    let bytes = include_bytes!("data/issue_1476_swept_bar.smd").to_vec();
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
        next_key: 1 << 40,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

/// The result's bits: its volume, every vertex, and each face's surface on a 5 x 5 grid
/// of its domain.
fn fingerprint(k: &Kernel, b: &mut Brep) -> Vec<u64> {
    b.refresh_cached_topology();
    let mut bits = vec![volume(k, b).unwrap().to_bits()];
    for &v in b.vertex_list() {
        let p = vertex::point(b, v).unwrap();
        bits.extend([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]);
    }
    let ids: Vec<FaceId> = b.faces().ids().collect();
    for f in ids {
        let Some(srf) = face::surface(b, f).unwrap() else {
            continue;
        };
        let dom = srf.natural_uv_domain_with_nurb(k).unwrap();
        for i in 0..5 {
            for j in 0..5 {
                let u = dom.min().x + (dom.max().x - dom.min().x) * f64::from(i) / 4.0;
                let v = dom.min().y + (dom.max().y - dom.min().y) * f64::from(j) / 4.0;
                let p = srf.evaluate_point(k, Point2::new(u, v)).unwrap();
                bits.extend([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]);
            }
        }
    }
    bits
}

/// An operation on a fresh kernel; `token` is set after the operand is built.
type Op = fn(&mut Kernel, Option<CancelToken>) -> Result<Brep>;

/// The shell of the swept bar (`SmApiShellBrepFull(b, -0.5, TRUE, TRUE, TRUE, [start
/// cap])`, as #1476 runs it).
fn shell_swept_bar(k: &mut Kernel, token: Option<CancelToken>) -> Result<Brep> {
    let mut part = swept_bar(k);
    let start = planar_faces_at(k, &part, 2, 0.0);
    with_token(k, token);
    api_shell(
        k,
        &mut part,
        ShellOptions {
            offset: OffsetOptions {
                distance: -0.5,
                extend_convex_edges: true,
                merge_results: true,
            },
            create_offset_solid: true,
            merge_sheet_side_walls: true,
        },
        &start,
    )
}

/// The box's offset 1 outward, extended (`SmApiOffsetBrepFull`).
fn offset_box(k: &mut Kernel, token: Option<CancelToken>) -> Result<Brep> {
    let mut part = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0)?;
    with_token(k, token);
    offset_full(k, &mut part, 1.0)
}

/// The cylinder's inset 0.5.
fn inset_cylinder(k: &mut Kernel, token: Option<CancelToken>) -> Result<Brep> {
    let mut part = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 5.0, 10.0)?;
    with_token(k, token);
    offset_full(k, &mut part, -0.5)
}

/// The square of side 4 in z = 0, as four line curves.
fn square() -> Vec<Curve> {
    let p = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
    (0..4)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % 4]);
            Curve::from(
                BSplineCurve::create_line_segment(
                    3,
                    Point3::new(a[0], a[1], 0.0),
                    Point3::new(b[0], b[1], 0.0),
                )
                .unwrap(),
            )
        })
        .collect()
}

/// The square swept 6 along z, capped.
fn linear_sweep(k: &mut Kernel, token: Option<CancelToken>) -> Result<Brep> {
    let curves = square();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    with_token(k, token);
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 6.0, true)
}

/// The square revolved 270 degrees about the line x = -2 along y, capped.
fn rotational_sweep(k: &mut Kernel, token: Option<CancelToken>) -> Result<Brep> {
    let curves = square();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    with_token(k, token);
    create_rotational_sweep(
        k,
        &refs,
        Point3::new(-2.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        270.0,
        true,
    )
}

const OPS: [(&str, Op); 5] = [
    ("shell_swept_bar", shell_swept_bar),
    ("offset_box", offset_box),
    ("inset_cylinder", inset_cylinder),
    ("linear_sweep", linear_sweep),
    ("rotational_sweep", rotational_sweep),
];

/// The token an operation's kernel takes once its operand is built (the operand's own
/// construction runs uncancelled).
fn with_token(k: &mut Kernel, token: Option<CancelToken>) {
    if let Some(t) = token {
        k.set_cancel_token(t);
    }
}

fn run(op: Op, token: Option<CancelToken>) -> (Kernel, Result<Brep>) {
    let mut k = Kernel::default();
    let made = op(&mut k, token);
    (k, made)
}

#[test]
fn an_unfired_token_leaves_offset_shell_and_sweep_bit_for_bit() {
    for (name, op) in OPS {
        let (k0, made0) = run(op, None);
        let (k1, made1) = run(op, Some(CancelToken::new()));
        let (mut b0, mut b1) = (
            made0.unwrap_or_else(|e| panic!("{name}: {e:?}")),
            made1.unwrap_or_else(|e| panic!("{name} with a token: {e:?}")),
        );
        let (f0, f1) = (fingerprint(&k0, &mut b0), fingerprint(&k1, &mut b1));
        if std::env::var_os("ISSUE_2372_PRINT").is_some() {
            eprintln!("ISSUE_2372 {name} bits={f0:x?}");
        }
        assert_eq!(f0, f1, "{name}: an unfired token changed the result");
    }
}

#[test]
fn a_token_fired_before_the_call_cancels_offset_shell_and_sweep() {
    for (name, op) in OPS {
        let token = CancelToken::new();
        token.cancel();
        let (_, made) = run(op, Some(token));
        assert!(
            matches!(made, Err(SolidError::Cancelled { .. })),
            "{name}: {made:?}"
        );
    }
}

/// Every opt-in fix but `geometric_g2_continuity` (as `fork_ea59e9c_section_orders`'s
/// SolidCraft kernel): with them the shell takes #1476's slow path (two 38 000-sample
/// curve offsets, over 4 min in a debug build).
fn every_fix_but_g2() -> solidcore::kernel::UpstreamFixes {
    let mut f = solidcore::kernel::UpstreamFixes::default();
    for &name in solidcore::kernel::UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    f
}

/// The swept bar's shell, cancelled from another thread `delay` into the call:
/// `Err(Cancelled)`: the time from the cancel to the return, and the check point's
/// operation; `None` when the shell returned before the cancel (a faster build: in a
/// debug build the 60 s cancel lands in the bend walls' knot removals, 97 s each). The
/// canceller waits for the delay or the shell's return, whichever comes first, so a shell
/// that returns first ends the wait with it. The knot removal's and the sampling's own
/// checks are also tested directly in `nurbs::crv_offset`.
#[cfg(not(target_arch = "wasm32"))]
fn cancel_shell_after(
    every_fix: bool,
    delay: std::time::Duration,
) -> Option<(std::time::Duration, &'static str)> {
    use std::sync::mpsc::{channel, RecvTimeoutError};
    use std::time::Instant;
    let token = CancelToken::new();
    let canceller_token = token.clone();
    let (start_tx, start_rx) = channel::<()>();
    let (returned_tx, returned_rx) = channel::<()>();
    let canceller = std::thread::spawn(move || {
        start_rx.recv().unwrap();
        match returned_rx.recv_timeout(delay) {
            Err(RecvTimeoutError::Timeout) => {
                canceller_token.cancel();
                Some(Instant::now())
            }
            // the shell returned before the delay: nothing to cancel
            Ok(()) | Err(RecvTimeoutError::Disconnected) => None,
        }
    });
    let mut k = Kernel::default();
    if every_fix {
        k.fixes = every_fix_but_g2();
    }
    let mut part = swept_bar(&mut k);
    let start = planar_faces_at(&k, &part, 2, 0.0);
    k.set_cancel_token(token);
    start_tx.send(()).unwrap();
    let made = api_shell(
        &mut k,
        &mut part,
        ShellOptions {
            offset: OffsetOptions {
                distance: -0.5,
                extend_convex_edges: true,
                merge_results: true,
            },
            create_offset_solid: true,
            merge_sheet_side_walls: true,
        },
        &start,
    );
    let returned = Instant::now();
    // (the canceller may have timed out just before this: it then cancelled)
    let _ = returned_tx.send(());
    let cancelled_at = canceller.join().unwrap();
    let Some(cancelled_at) = cancelled_at.filter(|&at| at <= returned || made.is_err()) else {
        // a faster build finished the shell before the cancel: nothing to cancel
        assert!(made.is_ok(), "the uncancelled shell returned {made:?}");
        return None;
    };
    let Err(SolidError::Cancelled { operation }) = made else {
        panic!("the cancelled shell returned {made:?} (after {delay:?})");
    };
    Some((returned.saturating_duration_since(cancelled_at), operation))
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_token_fired_from_another_thread_stops_the_shell_soon() {
    for (every_fix, ms) in [
        (false, 100),
        (false, 500),
        (false, 1500),
        (true, 2000),
        (true, 60000),
    ] {
        let Some((stop, at)) = cancel_shell_after(every_fix, std::time::Duration::from_millis(ms))
        else {
            eprintln!("ISSUE_2372 every_fix={every_fix}: done before the cancel {ms} ms in");
            continue;
        };
        eprintln!(
            "ISSUE_2372 every_fix={every_fix} cancelled {ms} ms in: returned {stop:?} after \
             the cancel, at {at}"
        );
        assert!(
            stop < std::time::Duration::from_secs(5),
            "returned {stop:?} after the cancel"
        );
    }
}
