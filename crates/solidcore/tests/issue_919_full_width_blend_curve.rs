//! tangency-labs/solidcore#919 (WR-581): a blend-curve (G2 or G1) round on an R10
//! cylinder's rim at the full width 10 fails in SMLib (`SmApiFilletEdges` returns
//! `SM_ERR`, 1001, and leaves the cylinder as it was; at 9.99 it builds). At the full
//! width the fillet's centre line is the single point at the dome's centre on the axis,
//! and `CalcTorusFilletGeom`, which builds the circular round's hemisphere there, takes
//! only circular sections, so the blend is marched along a centre line of no length and
//! `TrimRails` fails. The difftest fixtures
//! `issue919-cylinder-rim-edge{1,2}-blend-curve-g{1,2}-r10.0`
//! (`fixtures/blend.fillet_executive_p3/issue_919_920_full_width.json`) show the pinned
//! C++ failing the same way, bit-identical to the port.
//!
//! With the opt-in `Kernel::fixes.fillet_blend_curve_revolved_on_arc` the blend curve of
//! the section is revolved about the axis there, as the circular section is: the round
//! closes the top as a dome. Every blend whose centre line is not on the axis is marched
//! as before (r 9.99 builds bit-identically with the fix on and off).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::error::SM_ERR;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::vector2d::Point2;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge;
use solidcore::{Brep, Kernel, SolidError};

const R: f64 = 10.0;
const H: f64 = 20.0;

/// `SM_FSG_BLEND_CURVE`.
const BLEND_CURVE: u32 = 2;

/// The cylinder of radius 10 and height 20 with its top rim blended by `radius` at
/// `continuity` (1 G1, 2 G2), with the fix `on` or off: the API's result and the body.
fn blended(radius: f64, continuity: u32, on: bool) -> (Kernel, Result<(), SolidError>, Brep) {
    let mut k = Kernel::default();
    k.fixes.fillet_blend_curve_revolved_on_arc = on;
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), R, H).unwrap();
    let rim = *b
        .edge_list()
        .iter()
        .find(|&&e| {
            let c = edge::curve(&b, e).unwrap().unwrap();
            let ivl = edge::interval(&b, e).unwrap();
            let p = c.evaluate_point(&k, ivl.min()).unwrap();
            (p.z - H).abs() < 1e-6 && (p.x.hypot(p.y) - R).abs() < 1e-6
        })
        .unwrap();
    let result = fillet_edges(&mut k, &mut b, &[rim], radius, BLEND_CURVE, continuity, 1.0);
    (k, result, b)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The kinds of `b`'s face surfaces, as names.
fn kinds(b: &Brep) -> Vec<&'static str> {
    b.face_list()
        .iter()
        .map(
            |&f| match b.face(f).unwrap().surface.as_ref().unwrap().kind() {
                SurfaceKind::Plane(_) => "plane",
                SurfaceKind::Cone(_) => "cylinder",
                SurfaceKind::SurfOfRevolution(_) => "revolution",
                SurfaceKind::Sphere(_) => "sphere",
                SurfaceKind::BSpline(_) => "bspline",
                _ => "other",
            },
        )
        .collect()
}

/// The volume of the cylinder under the dome surface `dome`, integrated along its
/// meridian independently of the mass properties: the cylinder up to where the meridian
/// starts, then the discs `pi r(z)^2 dz` up the meridian (trapezoids over 20000 steps).
fn volume_under(k: &Kernel, dome: &Surface) -> f64 {
    let d = dome.natural_uv_domain_with_nurb(k).unwrap();
    let (lo, hi) = (d.min(), d.max());
    let at = |u: f64, v: f64| dome.evaluate_point(k, Point2::new(u, v)).unwrap();
    let radius = |p: Point3| p.x.hypot(p.y);
    let (mu, mv) = (0.5 * lo.x + 0.5 * hi.x, 0.5 * lo.y + 0.5 * hi.y);
    // the meridian runs from the rim (r 10) to the apex (r 0); a parallel keeps its radius
    let along_v = (radius(at(mu, lo.y)) - radius(at(mu, hi.y))).abs() > 1.0;
    let n: u32 = 20_000;
    let pts: Vec<Point3> = (0..=n)
        .map(|i| {
            let s = f64::from(i) / f64::from(n);
            if along_v {
                at(mu, lo.y + s * (hi.y - lo.y))
            } else {
                at(lo.x + s * (hi.x - lo.x), mv)
            }
        })
        .collect();
    let pi = std::f64::consts::PI;
    let z0 = pts.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
    let mut v = pi * R * R * z0;
    for w in pts.windows(2) {
        let (ra, rb) = (radius(w[0]), radius(w[1]));
        v += pi * 0.5 * (ra * ra + rb * rb) * (w[1].z - w[0].z).abs();
    }
    v
}

#[test]
fn issue_919_full_width_blend_curve_is_refused_without_the_fix() {
    for continuity in [2, 1] {
        let (_, result, b) = blended(R, continuity, false);
        assert!(
            matches!(result, Err(SolidError::Upstream { code, .. }) if code == SM_ERR),
            "G{continuity}: {result:?}"
        );
        // the cylinder as it was: its wall and two caps
        let mut got = kinds(&b);
        got.sort_unstable();
        assert_eq!(got, ["cylinder", "plane", "plane"], "G{continuity}");
    }
}

#[test]
fn issue_919_full_width_blend_curve_closes_the_top_with_the_fix() {
    for continuity in [2, 1] {
        let (mut k, result, mut b) = blended(R, continuity, true);
        result.unwrap_or_else(|e| panic!("G{continuity}: {e:?}"));
        assert!(
            is_manifold_solid(&b).unwrap(),
            "G{continuity}: a closed solid"
        );
        let mut got = kinds(&b);
        got.sort_unstable();
        assert_eq!(got, ["cylinder", "plane", "revolution"], "G{continuity}");
        let dome = b
            .face_list()
            .iter()
            .map(|&f| b.face(f).unwrap().surface.as_ref().unwrap())
            .find(|s| matches!(s.kind(), SurfaceKind::SurfOfRevolution(_)))
            .unwrap();
        let want = volume_under(&k, dome);
        let c = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert_eq!(c.failure_count(), 0, "G{continuity}: {:?}", c.lines());
        let v = volume(&k, &mut b);
        // the dome lies between the cone and the cylinder on the top 10
        let pi = std::f64::consts::PI;
        assert!(
            v > pi * R * R * (H - R) + pi * R * R * R / 3.0 && v < pi * R * R * H,
            "G{continuity}: {v}"
        );
        assert!(
            (v - want).abs() <= 1e-6 * want,
            "G{continuity}: volume {v}, integrated {want}"
        );
        eprintln!("issue_919 G{continuity}: volume {v}, integrated {want}");
    }
}

/// Below the full width the centre line is a circle, not a point on the axis: the blend
/// is marched with the fix on as with it off, bit for bit.
#[test]
fn issue_919_narrower_blend_curve_is_unchanged_by_the_fix() {
    for continuity in [2, 1] {
        let (k0, r0, mut b0) = blended(9.99, continuity, false);
        let (k1, r1, mut b1) = blended(9.99, continuity, true);
        r0.unwrap();
        r1.unwrap();
        assert_eq!(kinds(&b0), kinds(&b1), "G{continuity}");
        assert_eq!(b0.edge_list().len(), b1.edge_list().len(), "G{continuity}");
        let (v0, v1) = (volume(&k0, &mut b0), volume(&k1, &mut b1));
        assert_eq!(v0.to_bits(), v1.to_bits(), "G{continuity}: {v0} vs {v1}");
    }
}
