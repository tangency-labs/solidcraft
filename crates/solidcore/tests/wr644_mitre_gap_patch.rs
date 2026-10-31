//! WR-644 (the fork's tangency-labs/smlib@2223108): with `fillet_mitre_gap_patch`, two rounds
//! mitred at a face's concave corner (`fillet_mitre_concave_face_corners`, WR-642) whose walls
//! lean differently, so that their rails meet the corner edge at two places, fill the gap past
//! the corner edge with a patch on the side wall's continuation. The fork's case: a sloped
//! basin's rim rounded r 0.5 (SolidCraft's `1080-fillet-sloped-basin-1-mm-rim-r0-5-4-edges`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
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

fn kernel(gap: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_cliff_overflow = true;
    k.fixes.fillet_mitre_concave_face_corners = true;
    k.fixes.fillet_mitre_gap_patch = gap;
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

fn edge_between(k: &Kernel, b: &Brep, a: Point3, q: Point3) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let s = c.evaluate_point(k, ivl.min()).unwrap();
            let t = c.evaluate_point(k, ivl.max()).unwrap();
            (s.distance(a) < 1e-7 && t.distance(q) < 1e-7)
                || (s.distance(q) < 1e-7 && t.distance(a) < 1e-7)
        })
        .expect("edge")
}

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

/// A basin lofted (degree 1) from `top` (x0, x1, y0, y1) at z = 30 to `bottom` at z = 10,
/// cut from a 100 x 60 x 30 slab; the loft runs on linearly to z = 40. With `wide_top`
/// the slab is a 100 x 70 x 30 box less its front 10, so the top's plane runs on 10 past
/// its front edge (a SolidCraft sketch's plane is larger than its face, as in the WR-635
/// tests).
#[derive(Clone, Copy)]
struct Basin {
    top: [f64; 4],
    bottom: [f64; 4],
    wide_top: bool,
}

impl Basin {
    fn at(self, z: f64) -> [f64; 4] {
        let t = (z - 10.0) / 20.0;
        core::array::from_fn(|i| self.bottom[i] + (self.top[i] - self.bottom[i]) * t)
    }

    fn build(self, k: &mut Kernel) -> Brep {
        let slab = if self.wide_top {
            let big = create_box(k, p(0.0, -10.0, 0.0), 100.0, 70.0, 30.0).unwrap();
            let front = create_box(k, p(-1.0, -11.0, -1.0), 102.0, 11.0, 32.0).unwrap();
            boolean_difference(k, big, front).unwrap()
        } else {
            create_box(k, Point3::ZERO, 100.0, 60.0, 30.0).unwrap()
        };
        let rect = |z: f64| {
            let [x0, x1, y0, y1] = self.at(z);
            polyline(&[p(x0, y0, z), p(x1, y0, z), p(x1, y1, z), p(x0, y1, z)])
        };
        let (lo, hi) = (rect(10.0), rect(40.0));
        let refs: Vec<Option<&Curve>> = lo.iter().chain(hi.iter()).map(Some).collect();
        let loft = create_skin_primitive(k, &[4, 4], &refs, 1, true)
            .unwrap()
            .brep;
        boolean_difference(k, slab, loft).unwrap()
    }

    /// The rim edges: front, right, back, left.
    fn rim(self) -> [(Point3, Point3); 4] {
        let [x0, x1, y0, y1] = self.top;
        let c = [
            p(x0, y0, 30.0),
            p(x1, y0, 30.0),
            p(x1, y1, 30.0),
            p(x0, y1, 30.0),
        ];
        [(c[0], c[1]), (c[1], c[2]), (c[2], c[3]), (c[3], c[0])]
    }
}

/// The fork's basin: 20..80 x 1..50 at the top, 30..70 x 20..40 at z = 10.
const FORK_BASIN: Basin = Basin {
    top: [20.0, 80.0, 1.0, 50.0],
    bottom: [30.0, 70.0, 20.0, 40.0],
    wide_top: false,
};

/// SolidCraft's `1080-fillet-sloped-basin-4-mm-rim-*` basin: the fork's with a 4 mm rim.
const RIM4_BASIN: Basin = Basin {
    top: [20.0, 80.0, 4.0, 50.0],
    ..FORK_BASIN
};

/// SolidCraft's `1080-fillet-equal-slope-basin-1-mm-rim-*` basin: every wall at 1 in 2.
const EQUAL_SLOPE_BASIN: Basin = Basin {
    bottom: [30.0, 70.0, 11.0, 40.0],
    ..FORK_BASIN
};

/// Rounds `edges` of `basin` by `r`, or chamfers them with legs `r` (`chamfer`): faces and volume, or the refusal.
fn blend(
    gap: bool,
    basin: Basin,
    edges: &[(Point3, Point3)],
    r: f64,
    chamfer: bool,
) -> Result<(usize, f64), String> {
    let mut k = kernel(gap);
    let mut b = basin.build(&mut k);
    let ids: Vec<EdgeId> = edges
        .iter()
        .map(|&(a, q)| edge_between(&k, &b, a, q))
        .collect();
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    if chamfer {
        // SolidCraft's chamfer: constant distance (the chord across the blend)
        session
            .prepare_per_edge_constant_distance(
                &mut k,
                &ids,
                &legs_to_chords(basin, r, ids.len()),
                &[FilletSurfaceGeneratorType::Linear],
                1,
                1.0,
            )
            .unwrap();
    } else {
        session
            .prepare_constant_radius_edges(&mut k, &ids, r, 1, 1, 1.0)
            .unwrap();
    }
    let result = session.do_fillet(&mut k);
    let info = session.error_info().clone();
    drop(session);
    if let Err(e) = result {
        return Err(format!("code {} {} ({e})", info.code, info.comment));
    }
    if !is_manifold_solid(&b).unwrap() {
        return Err("not a manifold solid".into());
    }
    b.refresh_cached_topology();
    Ok((b.faces().ids().count(), volume(&k, &mut b)))
}

/// The chord across a chamfer whose legs are `d` on each rim edge (front, right, back,
/// left): `2 d sin(alpha / 2)`, alpha the material's angle at the edge.
fn legs_to_chords(basin: Basin, d: f64, n: usize) -> Vec<f64> {
    let [x0, x1, y0, y1] = basin.top;
    let [bx0, bx1, by0, by1] = basin.bottom;
    let run = [by0 - y0, x1 - bx1, y1 - by1, bx0 - x0];
    run.iter()
        .take(n)
        .map(|&w| {
            let alpha = core::f64::consts::PI - (20.0f64).atan2(w);
            2.0 * d * (alpha / 2.0).sin()
        })
        .collect()
}

#[test]
fn sloped_basin_rim_rounds_fill_the_mitre_gap() {
    let rim = FORK_BASIN.rim();
    // the front and right rim edges: the fork 13 faces 144532.187340 against the closed
    // form 144532.187336
    let (n, v) = blend(true, FORK_BASIN, &rim[..2], 0.5, false).unwrap();
    assert_eq!(n, 13);
    assert!((v - 144_532.187_336).abs() < 1e-4, "two edges: {v}");
    // all four: the fork 15 faces 144530.427946 against 144530.427909
    let (n, v) = blend(true, FORK_BASIN, &rim, 0.5, false).unwrap();
    assert_eq!(n, 15);
    assert!((v - 144_530.427_909).abs() < 2e-4, "four edges: {v}");
}

#[test]
fn without_the_flag_the_mitre_splits_the_side_vertex_as_upstream() {
    let rim = FORK_BASIN.rim();
    // the split cuts the larger round by the side wall's own surface: 1001
    for edges in [&rim[..2], &rim[..]] {
        let r = blend(false, FORK_BASIN, edges, 0.5, false);
        assert!(r.as_ref().is_err_and(|e| e.starts_with("code 3 ")), "{r:?}");
    }
}

#[test]
fn sloped_basin_rim_chamfers_fill_the_mitre_gap() {
    // SolidCraft's 1080-fillet-sloped-basin-4-mm-rim-2-2-edges (5835): 145957.3269
    let rim = RIM4_BASIN.rim();
    let (_, v) = blend(true, RIM4_BASIN, &rim[..2], 2.0, true).unwrap();
    assert!((v - 145_957.326_9).abs() < 1e-3, "4 mm rim: {v}");
    assert!(blend(false, RIM4_BASIN, &rim[..2], 2.0, true).is_err());
    // 1080-fillet-sloped-basin-1-mm-rim-2-two-edges: 144372.0542, its front leg running off
    // the 1 mm rim over the slab's front (a cliff overflow)
    let basin = Basin {
        wide_top: true,
        ..FORK_BASIN
    };
    let rim = basin.rim();
    let (_, v) = blend(true, basin, &rim[..2], 2.0, true).unwrap();
    assert!((v - 144_372.054_2).abs() < 1e-3, "1 mm rim: {v}");
    assert!(blend(false, basin, &rim[..2], 2.0, true).is_err());
}

#[test]
fn an_equal_slope_basin_needs_no_gap() {
    // 1080-fillet-equal-slope-basin-1-mm-rim-loop-0-5: 140329.8156, the rails meeting the
    // corner edge at one place: the mitre alone (WR-642), with or without the gap patch
    let rim = EQUAL_SLOPE_BASIN.rim();
    for gap in [false, true] {
        let (n, v) = blend(gap, EQUAL_SLOPE_BASIN, &rim, 0.5, false).unwrap();
        assert_eq!(n, 15);
        assert!((v - 140_329.815_6).abs() < 1e-3, "equal slope: {v}");
    }
}
