//! #2371: the G^n section round (`fixes.fillet_gn_section_generator`, the curvature
//! fillet) polls the kernel's [`CancelToken`] (#2136) in the loops it spends its time in:
//! the stations, the certificate's spans, pieces and Krawczyk steps, and the skin.
//!
//! - A token that never fires leaves every result bit for bit as without one (the
//!   volume, every vertex and the faces' points), with the flag on and off.
//! - A token fired before the call, or from another thread while the boss root's
//!   order-2 round certifies its spans (4 min uncancelled in the difftest profile, a
//!   227-span certificate), turns the call into `Err(Cancelled)`, never `Ok`, and the call
//!   returns soon after the cancel.
//!   The cancel is returned by the section's own check point: before #2371 the
//!   solver's trace absorbed it (`c4_trace` passed only `Unported` through), so the
//!   round ran on for minutes to the corners' check.
//!
//! Expected values: the uncancelled results are compared with themselves (no token
//! against a token that never fires), and were checked against main's bit for bit when
//! this test was written (see #2373).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::error::SolidError;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::vector2d::Point2;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::face::p2 as face;
use solidcore::topology::{vertex, EdgeId, FaceId};
use solidcore::{Brep, CancelToken, Kernel, Result};

/// The G^n section's flag and the one its plane rails need (as `fork_ea59e9c_section_orders`).
fn gn_kernel(gn: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_gn_section_generator = gn;
    k.fixes.fillet_surface_plane_curve_on_plane_equation = true;
    k
}

fn box_40_30_20(k: &mut Kernel) -> Result<Brep> {
    create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0)
}

fn boss_on_plate(k: &mut Kernel) -> Result<Brep> {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0)?;
    let boss = create_cylinder(k, Point3::new(30.0, 20.0, 10.0), 8.0, 10.0)?;
    boolean_union(k, plate, boss)
}

fn pipe_on_plate(k: &mut Kernel) -> Result<Brep> {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0)?;
    let mut pipe = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 10.0, 60.0)?;
    let pl = Axis2Placement::new(
        Point3::new(0.0, 20.0, 16.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    solidcore::topology::brep::p5::transform(k, &mut pipe, &pl, None, false)?;
    boolean_union(k, plate, pipe)
}

const BOSS_ROOT: [f64; 3] = [
    30.0 + 4.0 * std::f64::consts::SQRT_2,
    20.0 + 4.0 * std::f64::consts::SQRT_2,
    10.0,
];

fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (0.1f64, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap_or_else(|| panic!("no edge near {p:?}"))
}

/// The result's bits: its volume, every vertex, and each face's surface on a 5 x 5 grid
/// of its domain.
fn fingerprint(k: &Kernel, b: &mut Brep) -> Vec<u64> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let mut bits = vec![brep_volume(k, b, 1e-7, &attrs).unwrap().to_bits()];
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

type Build = fn(&mut Kernel) -> Result<Brep>;

/// A blend's `fillet_edges` arguments: cross-section type, continuity, thumbweight.
type Blend = (u32, u32, f64);

/// The order-2 and order-3 G^n section rounds (`SM_FSG_BLEND_CURVE`).
const GN2: Blend = (2, 2, 0.0);
const GN3: Blend = (2, 3, 0.0);
/// The circular round.
const ROUND: Blend = (1, 1, 1.0);

/// The case's model, built without a token, and its blend's result with `token`.
fn round(
    gn: bool,
    build: Build,
    at: [f64; 3],
    r: f64,
    blend: Blend,
    token: Option<CancelToken>,
) -> (Kernel, Brep, Result<()>) {
    let mut k = gn_kernel(gn);
    let mut b = build(&mut k).unwrap();
    let e = pick(&k, &b, at);
    if let Some(t) = token {
        k.set_cancel_token(t);
    }
    let (xsect, continuity, w) = blend;
    let made = fillet_edges(&mut k, &mut b, &[e], r, xsect, continuity, w);
    (k, b, made)
}

fn is_cancelled(r: &Result<()>) -> bool {
    matches!(r, Err(SolidError::Cancelled { .. }))
}

/// Without a token and with one that never fires: the same status, and for a built blend
/// the same result, bit for bit.
fn assert_unfired_is_identical(gn: bool, build: Build, at: [f64; 3], r: f64, blend: Blend) {
    let (k0, mut b0, made0) = round(gn, build, at, r, blend, None);
    let (k1, mut b1, made1) = round(gn, build, at, r, blend, Some(CancelToken::new()));
    assert_eq!(made0, made1, "the statuses differ");
    if std::env::var_os("ISSUE_2371_PRINT").is_some() {
        eprintln!("ISSUE_2371 gn={gn} at={at:?} blend={blend:?} status={made0:?}");
    }
    if made0.is_err() {
        // the blend curve at continuity 2 without the G^n generator: refused alike
        assert!(!gn, "the G^n round failed: {made0:?}");
        return;
    }
    let (f0, f1) = (fingerprint(&k0, &mut b0), fingerprint(&k1, &mut b1));
    if std::env::var_os("ISSUE_2371_PRINT").is_some() {
        eprintln!("ISSUE_2371 gn={gn} at={at:?} blend={blend:?} bits={f0:x?}");
    }
    assert_eq!(f0, f1, "an unfired token changed the result");
}

#[test]
fn an_unfired_token_leaves_the_box_edge_rounds_bit_for_bit() {
    let edge = [20.0, 30.0, 20.0];
    for gn in [true, false] {
        for blend in [GN2, GN3, ROUND] {
            assert_unfired_is_identical(gn, box_40_30_20, edge, 3.0, blend);
        }
    }
}

#[test]
fn an_unfired_token_leaves_the_pipe_root_rounds_bit_for_bit() {
    let edge = [30.0, 28.0, 10.0];
    for gn in [true, false] {
        for blend in [GN2, ROUND] {
            assert_unfired_is_identical(gn, pipe_on_plate, edge, 2.0, blend);
        }
    }
}

#[test]
fn a_token_fired_before_the_call_cancels_the_gn_round() {
    for gn in [true, false] {
        let token = CancelToken::new();
        token.cancel();
        let (_, _, made) = round(gn, box_40_30_20, [20.0, 30.0, 20.0], 3.0, GN2, Some(token));
        assert!(is_cancelled(&made), "gn={gn}: {made:?}");
    }
}

/// The boss root's order-2 round, cancelled from another thread `delay` into the call:
/// `Err(Cancelled)`: the time from the cancel to the return, and the check point's
/// operation.
#[cfg(not(target_arch = "wasm32"))]
fn cancel_boss_root_after(delay: std::time::Duration) -> (std::time::Duration, &'static str) {
    use std::time::Instant;
    let mut k = gn_kernel(true);
    let mut b = boss_on_plate(&mut k).unwrap();
    let e = pick(&k, &b, BOSS_ROOT);
    let token = CancelToken::new();
    k.set_cancel_token(token.clone());
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(delay);
        token.cancel();
        Instant::now()
    });
    let made = fillet_edges(&mut k, &mut b, &[e], 2.0, 2, 2, 0.0);
    let returned = Instant::now();
    let cancelled_at = canceller.join().unwrap();
    let Err(SolidError::Cancelled { operation }) = made else {
        panic!("the cancelled round returned {made:?} (after {delay:?})");
    };
    (returned.saturating_duration_since(cancelled_at), operation)
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_token_fired_from_another_thread_stops_the_boss_root_round_soon() {
    for ms in [200, 1500, 4000, 10000] {
        let (stop, at) = cancel_boss_root_after(std::time::Duration::from_millis(ms));
        eprintln!("ISSUE_2371 cancelled {ms} ms in: returned {stop:?} after the cancel, at {at}");
        assert!(
            stop < std::time::Duration::from_secs(5),
            "returned {stop:?} after the cancel"
        );
        // past the set-up the round is building its sections (4 min in the difftest
        // profile): the check point there ends it, not one in a later loop after a
        // status-ignoring helper absorbed the cancel
        if ms >= 1500 {
            assert!(
                at.starts_with("blend::fillet_section::"),
                "cancelled {ms} ms in, at {at}"
            );
        }
    }
}
