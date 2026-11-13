//! WR-635 / WR-636 (#960; the SolidCraft fork's tangency-labs/smlib@eddf0ed and bba9914,
//! branch `replay/1.0.1`): with `fillet_cliff_overflow`, the removal region of a cliff
//! overflow also closes
//!
//! - (WR-635) where an Nx1 corner of mixed convexity ends the blend on a face of the sheet
//!   on the end wall's surface extended (that face's lamina edges pass onto the next wall;
//!   the chain on the top ends at the rim's vertex), and on planar B-spline walls (a loft's
//!   sides), which continue as their planes;
//! - (WR-636) where the sheet's boundary on a face is a closed chain round a loop of that
//!   face whose every edge is blended (a pocket's rim chamfered all round).
//!
//! Without the flag the same blends are refused (`NoRoom`), as before; with it but without
//! these two changes they were refused by the construction (the comments below).
//!
//! The expected volumes are closed forms: the body less the convex region the blend
//! removes, clipped to the body.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::FilletErrorType;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_box;
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn kernel(cliff: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_cliff_overflow = cliff;
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

/// The edge from `a` to `b` (either way).
fn edge_between(k: &Kernel, b: &Brep, a: Point3, q: Point3) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let s = c.evaluate_point(k, ivl.min()).unwrap();
            let t = c.evaluate_point(k, ivl.max()).unwrap();
            (s.distance(a) < 1e-9 && t.distance(q) < 1e-9)
                || (s.distance(q) < 1e-9 && t.distance(a) < 1e-9)
        })
        .expect("edge")
}

/// The outcome of a blend: the error code and comment of a refusal, or the volume of the
/// result (a manifold solid).
#[derive(Debug)]
enum Outcome {
    Built(f64),
    Refused(i32, #[allow(dead_code)] String),
}

/// Chamfers (`chamfer`) or rounds the edges between the point pairs `edges` of the body
/// `make` builds, by `r`.
fn blend(
    cliff: bool,
    make: fn(&mut Kernel) -> Brep,
    edges: &[(Point3, Point3)],
    r: f64,
    chamfer: bool,
) -> Outcome {
    let mut k = kernel(cliff);
    let mut b = make(&mut k);
    let ids: Vec<EdgeId> = edges
        .iter()
        .map(|&(a, q)| edge_between(&k, &b, a, q))
        .collect();
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &ids, r, u32::from(!chamfer), 1, 1.0)
        .unwrap();
    let result = session.do_fillet(&mut k);
    let info = session.error_info().clone();
    drop(session);
    match result {
        Ok(()) => {
            assert!(
                is_manifold_solid(&b).unwrap(),
                "r {r}: not a manifold solid"
            );
            Outcome::Built(volume(&k, &mut b))
        }
        Err(_) => Outcome::Refused(info.code, info.comment),
    }
}

fn assert_builds(outcome: &Outcome, expected: f64, rel: f64) {
    let Outcome::Built(v) = outcome else {
        panic!("expected a build at {expected}, got {outcome:?}");
    };
    assert!(
        (v - expected).abs() <= rel * expected,
        "volume {v} against {expected} (rel {})",
        (v - expected).abs() / expected
    );
}

fn assert_no_room(outcome: &Outcome) {
    let Outcome::Refused(code, _) = outcome else {
        panic!("expected a refusal, got {outcome:?}");
    };
    assert_eq!(*code, FilletErrorType::NoRoom as i32, "{outcome:?}");
}

// ---- WR-636: a pocket's rim chamfered all round --------------------------------------------

/// A 60 x 40 x 10 plate with a 30 x 20 pocket 5 deep at its middle (perimeter 100).
fn pocketed_plate(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::ZERO, 60.0, 40.0, 10.0).unwrap();
    let pocket = create_box(k, p(15.0, 10.0, 5.0), 30.0, 20.0, 6.0).unwrap();
    boolean_difference(k, plate, pocket).unwrap()
}

/// The pocket's four rim edges.
fn pocket_rim() -> Vec<(Point3, Point3)> {
    let c = [
        p(15.0, 10.0, 10.0),
        p(45.0, 10.0, 10.0),
        p(45.0, 30.0, 10.0),
        p(15.0, 30.0, 10.0),
    ];
    (0..4).map(|i| (c[i], c[(i + 1) % 4])).collect()
}

/// The same plate 20 thick with the pocket 15 deep: deep enough for every chamfer below.
fn deep_pocketed_plate(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::ZERO, 60.0, 40.0, 20.0).unwrap();
    let pocket = create_box(k, p(15.0, 10.0, 5.0), 30.0, 20.0, 16.0).unwrap();
    boolean_difference(k, plate, pocket).unwrap()
}

#[test]
fn a_pocket_rim_chamfer_past_the_floor_is_subtracted() {
    // The fork's bba9914: the rim loop chamfered d = 5.5 (and 6): the legs on the 5 high
    // walls run past the floor, and the sheet's boundary on the top is a closed chain round
    // the rim. Before WR-636: "a cliff overflow's fillet boundary closes on one face".
    //
    // The region a chamfer removes lies within d of the rim, on the material's side of
    // both the top and the walls, so it is the same whatever the pocket's depth: it must
    // equal what the same chamfer removes from a pocket 15 deep, which needs no overflow.
    // (The fork's closed form, 50 d^2 + (4/3) d^3, is the chamfers mitred at the corners,
    // the fork's 4f06e72, which the port does not have: the corners here are upstream's
    // corner patches; the comparison holds either way.) The two builds' corner patches
    // differ by about 1e-7 of the removed volume without any overflow (d = 3), hence the
    // 1e-6.
    for d in [3.0, 5.5, 6.0] {
        let deep_rim: Vec<(Point3, Point3)> = pocket_rim()
            .into_iter()
            .map(|(a, b)| (p(a.x, a.y, 20.0), p(b.x, b.y, 20.0)))
            .collect();
        let Outcome::Built(deep) = blend(true, deep_pocketed_plate, &deep_rim, d, true) else {
            panic!("the deep pocket's chamfer {d} must build");
        };
        let removed = 48_000.0 - 9_000.0 - deep;
        assert_builds(
            &blend(true, pocketed_plate, &pocket_rim(), d, true),
            24_000.0 - 3_000.0 - removed,
            1e-6 * removed / 21_000.0,
        );
    }
    assert_no_room(&blend(false, pocketed_plate, &pocket_rim(), 5.5, true));
}

// ---- WR-635: a sloped basin's front rim chamfered past the slab's front ----------------------

/// The closed polyline through `pts` as line curves.
fn polyline(pts: &[Point3]) -> Vec<Curve> {
    pts.iter()
        .zip(pts.iter().cycle().skip(1))
        .map(|(a, b)| {
            Curve::from(
                create_line_segment(Vec3::new(a.x, a.y, a.z), Vec3::new(b.x, b.y, b.z)).unwrap(),
            )
        })
        .collect()
}

/// A 100 x 60 x 30 slab (x 0..100, y 0..60, z 0..30) whose top face's plane runs on 10
/// past its front edge: a 100 x 70 x 30 box less its front 10, so the top keeps the box's
/// plane and domain (a SolidCraft sketch's plane is larger than its face; the corner's
/// edge on the top's plane past the front lies in that domain).
fn slab(k: &mut Kernel) -> Brep {
    let big = create_box(k, p(0.0, -10.0, 0.0), 100.0, 70.0, 30.0).unwrap();
    let front = create_box(k, p(-1.0, -11.0, -1.0), 102.0, 11.0, 32.0).unwrap();
    boolean_difference(k, big, front).unwrap()
}

/// A sloped basin: the slab less a degree-1 loft (its walls planar B-spline faces) between
/// rectangles at z = 10 and z = 40. At height z its front wall is at
/// `y = rim - (z - 30) / 2`, its end walls at `x = 20 - lean (z - 30)` and
/// `80 + lean (z - 30)`, its back wall at `50 + lean (z - 30)`.
#[derive(Clone, Copy)]
struct Basin {
    rim: f64,
    lean: f64,
}

impl Basin {
    fn front(self, z: f64) -> f64 {
        self.rim - (z - 30.0) / 2.0
    }

    fn ends(self, z: f64) -> (f64, f64) {
        (20.0 - self.lean * (z - 30.0), 80.0 + self.lean * (z - 30.0))
    }

    fn back(self, z: f64) -> f64 {
        50.0 + self.lean * (z - 30.0)
    }

    fn build(self, k: &mut Kernel) -> Brep {
        let slab = slab(k);
        let rect = |z: f64| {
            let (x0, x1) = self.ends(z);
            let (y0, y1) = (self.front(z), self.back(z));
            polyline(&[p(x0, y0, z), p(x1, y0, z), p(x1, y1, z), p(x0, y1, z)])
        };
        let (lo, hi) = (rect(10.0), rect(40.0));
        let refs: Vec<Option<&Curve>> = lo.iter().chain(hi.iter()).map(Some).collect();
        let loft = create_skin_primitive(k, &[4, 4], &refs, 1, true)
            .unwrap()
            .brep;
        boolean_difference(k, slab, loft).unwrap()
    }

    /// The basin's volume less the region a chamfer `d` of its front rim edge removes, in
    /// closed form. The chamfer's rails are where a ball of radius d tangent to the top and
    /// the front wall touches them, `s = d / tan(alpha / 2)` from the edge (alpha the
    /// material's angle between the top's direction `(0, -1, 0)` and the wall's `(0, 1, -2) /
    /// sqrt 5`). At height z the region removed is the rectangle between the end walls'
    /// planes (the blend ends on the end walls' surfaces continued) and between the chamfer
    /// and the front wall, clipped to y >= 0: its area is piecewise quadratic in z, so
    /// Simpson's rule on each piece is exact. The basin's own section, `(x1 - x0) (y1 - y0)`,
    /// is quadratic too.
    fn chamfered_volume(self, d: f64) -> f64 {
        let simpson = |f: &dyn Fn(f64) -> f64, a: f64, b: f64| {
            (b - a) / 6.0 * (f(a) + 4.0 * f((a + b) / 2.0) + f(b))
        };
        let section = |z: f64| {
            let (x0, x1) = self.ends(z);
            (x1 - x0) * (self.back(z) - self.front(z))
        };
        let basin = 100.0 * 60.0 * 30.0 - simpson(&section, 10.0, 30.0);
        let wall = (1.0 / 5.0f64.sqrt(), -2.0 / 5.0f64.sqrt());
        let alpha = (-wall.0).acos();
        let s = d / (alpha / 2.0).tan();
        // the top rail (rim - s, 30), the wall rail (rim + s w.y, 30 + s w.z)
        let (ty, wy, wz) = (self.rim - s, self.rim + s * wall.0, 30.0 + s * wall.1);
        let chamfer_y = |z: f64| ty + (wy - ty) * (30.0 - z) / (30.0 - wz);
        let removed = |z: f64| {
            let (x0, x1) = self.ends(z);
            (x1 - x0) * (self.front(z) - chamfer_y(z).max(0.0))
        };
        // the chamfer crosses y = 0 at z0
        let z0 = 30.0 - (0.0 - ty) / (wy - ty) * (30.0 - wz);
        let cut = if ty < 0.0 && z0 > wz {
            simpson(&removed, wz, z0) + simpson(&removed, z0, 30.0)
        } else {
            simpson(&removed, wz, 30.0)
        };
        basin - cut
    }
}

#[test]
fn a_sloped_basin_rim_chamfer_past_the_slab_front_is_subtracted() {
    // solidcraft#1080's sloped basin (the fork's eddf0ed): the front rim edge chamfered d, its
    // leg on the top running off the rim over the slab's front face. Each end of the edge is
    // an Nx1 corner of mixed convexity (the end wall's top edge convex, the pocket's corner
    // edge concave): its end face, on the end wall's surface extended back over the rim,
    // lies on the end wall's surface and the front wall's both. Before WR-635: "a cliff
    // overflow's fillet boundary lies on the surfaces of two faces".
    let cases: [(Basin, f64); 4] = [
        (
            Basin {
                rim: 1.0,
                lean: 0.0,
            },
            2.0,
        ),
        (
            Basin {
                rim: 1.0,
                lean: 0.25,
            },
            2.0,
        ),
        (
            Basin {
                rim: 1.0,
                lean: 0.25,
            },
            3.0,
        ),
        (
            Basin {
                rim: 0.5,
                lean: 0.5,
            },
            4.0,
        ),
    ];
    for (basin, d) in cases {
        let edge = (p(20.0, basin.rim, 30.0), p(80.0, basin.rim, 30.0));
        let outcome = blend_body(true, basin, &[edge], d, true);
        assert_builds(&outcome, basin.chamfered_volume(d), 1e-8);
    }
    let basin = Basin {
        rim: 1.0,
        lean: 0.25,
    };
    let edge = (p(20.0, 1.0, 30.0), p(80.0, 1.0, 30.0));
    assert_no_room(&blend_body(false, basin, &[edge], 2.0, true));
}

#[test]
fn a_small_sloped_basin_rim_chamfer_is_unchanged() {
    // A chamfer that stays on the rim: no overflow, the same closed form.
    let basin = Basin {
        rim: 1.0,
        lean: 0.25,
    };
    let edge = (p(20.0, 1.0, 30.0), p(80.0, 1.0, 30.0));
    for cliff in [false, true] {
        let outcome = blend_body(cliff, basin, &[edge], 0.5, true);
        assert_builds(&outcome, basin.chamfered_volume(0.5), 1e-8);
    }
}

/// [`blend`] on a [`Basin`].
fn blend_body(
    cliff: bool,
    basin: Basin,
    edges: &[(Point3, Point3)],
    r: f64,
    chamfer: bool,
) -> Outcome {
    BASIN.with(|c| c.set(Some((basin.rim, basin.lean))));
    blend(
        cliff,
        |k| {
            let (rim, lean) = BASIN.with(std::cell::Cell::get).unwrap();
            Basin { rim, lean }.build(k)
        },
        edges,
        r,
        chamfer,
    )
}

thread_local!(static BASIN: std::cell::Cell<Option<(f64, f64)>> = const { std::cell::Cell::new(None) });
