//! The reported revolved vase chamfer (solidcore#2905), captured before its operation.
//!
//! The meridian's circle has centre (-16,33), radius sqrt(1690), between z=30 and
//! z=60. The upper line is r=15+0.15*(z-60). Equal setbacks 2.995 determine the
//! ball offsets; the chamfer joins the two contact points by a straight segment.
//! Its revolution is a cone. Volume below is an independent meridian integral.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::f64::consts::PI;

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::geom::attribute::AttributeStore;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::brep_volume_default;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::procedural_blend::chamfer::ChamferSize;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::loopuse;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel, Vec3};

fn input() -> (Kernel, Brep) {
    input_from_bytes(include_bytes!("data/issue_2905_revolved_vase.smb"))
}

fn input_from_bytes(bytes: &[u8]) -> (Kernel, Brep) {
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    k.fixes.fillet_refuse_rounds_breaking_through = false;
    k.fixes.fillet_round_ends_on_outline = false;
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &loopuse::notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let bytes = bytes.to_vec();
    let b = read_brep_from_file(&mut k, Some(bytes), true, false, &mut env).unwrap();
    (k, b)
}

fn arc_volume(a: f64, b: f64) -> f64 {
    let primitive = |z: f64| {
        let t = z - 33.0;
        let j = 0.5 * (t * (1690.0 - t * t).sqrt() + 1690.0 * (t / 1690.0_f64.sqrt()).asin());
        PI * (1946.0 * t - t * t * t / 3.0 - 32.0 * j)
    };
    primitive(b) - primitive(a)
}

fn base_volume() -> f64 {
    let bottom = PI * 30.0 / 3.0 * (20.0 * 20.0 + 20.0 * 25.0 + 25.0 * 25.0);
    let top = PI * 20.0 / 3.0 * (15.0 * 15.0 + 15.0 * 18.0 + 18.0 * 18.0);
    bottom + arc_volume(30.0, 60.0) + top
}

fn chamfer_contacts() -> [[f64; 2]; 2] {
    // Independently solve the offset circle/cone meridians, selecting the contact
    // on both finite faces. Neither a kernel result nor the old cached volume.
    let d = 2.995;
    let rho = 1690.0_f64.sqrt();
    let alpha =
        ((27.0 / rho) * (0.15 / 1.0225_f64.sqrt()) - (31.0 / rho) / 1.0225_f64.sqrt()).acos();
    let r = d * (1.0 - alpha.cos()) / alpha.sin();
    let n = 1.0225_f64.sqrt();
    // Offset upper line: x - 0.15*z = 6 + r*sqrt(1.0225).
    let c = 6.0 + r * n;
    let aa = 1.0225;
    let bb = 0.3 * (c + 16.0) - 66.0;
    let cc = (c + 16.0) * (c + 16.0) + 1089.0 - (rho + r) * (rho + r);
    let z = (-bb + (bb * bb - 4.0 * aa * cc).sqrt()) / (2.0 * aa);
    let x = c + 0.15 * z;
    let ra = -16.0 + (x + 16.0) * rho / (rho + r);
    let za = 33.0 + (z - 33.0) * rho / (rho + r);
    let rb = x - r / n;
    let zb = z + 0.15 * r / n;
    [[ra, za], [rb, zb]]
}

fn expected_volume() -> f64 {
    let [[ra, za], [rb, zb]] = chamfer_contacts();
    let frustum = |h: f64, a: f64, b: f64| PI * h / 3.0 * (a * a + a * b + b * b);
    base_volume() - arc_volume(za, 60.0) - frustum(zb - 60.0, 15.0, rb) + frustum(zb - za, ra, rb)
}

fn chamfer_is_the_meridian_cone(k: &Kernel, b: &Brep) {
    let [[ra, za], [rb, zb]] = chamfer_contacts();
    // Check the definition, independently of its certified display approximation.
    let _exact = solidcore::surface::procedural_blend::NetScope::exact(k);
    let mut band_faces = 0;
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for id in b.face_list() {
        let surface = b.face(*id).unwrap().surface.as_ref().unwrap();
        let domain = solidcore::topology::face::uv_domain(b, *id).unwrap();
        let uv = |u: f64, v: f64| {
            solidcore::geom::vector2d::Point2::new(
                domain.min().x + (domain.max().x - domain.min().x) * u,
                domain.min().y + (domain.max().y - domain.min().y) * v,
            )
        };
        let midpoint = surface.evaluate_point(k, uv(0.5, 0.5)).unwrap();
        // Locate the band by its height. This also accepts an analytic cone and
        // rectangular angular patches of the same band.
        if !(midpoint.z > za && midpoint.z < zb) {
            continue;
        }
        band_faces += 1;
        for u in [0.0, 0.25, 0.5, 0.75, 1.0] {
            for v in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let point = surface.evaluate_point(k, uv(u, v)).unwrap();
                let radius = (point.x * point.x + point.y * point.y).sqrt();
                let expected = ra + (rb - ra) * (point.z - za) / (zb - za);
                assert!(
                    (radius - expected).abs() <= 1e-9 * radius.abs().max(expected.abs()),
                    "chamfer radius {radius} != cone radius {expected} at {point:?}"
                );
                low = low.min(point.z);
                high = high.max(point.z);
            }
        }
    }
    assert!(band_faces > 0, "the chamfer band is missing");
    assert!(
        (low - za).abs() <= 1e-9 * za.abs(),
        "lower contact {low} != {za}"
    );
    assert!(
        (high - zb).abs() <= 1e-9 * zb.abs(),
        "upper contact {high} != {zb}"
    );
}

fn valid_volume(k: &mut Kernel, b: &mut Brep, expected: f64) {
    assert!(is_manifold_solid(b).unwrap());
    let check = check_brep(k, b, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "{:?}", check.lines());
    let volume = brep_volume_default(k, b, 1e-10).unwrap();
    assert!(
        (volume - expected).abs() <= 1e-9 * expected.abs(),
        "{volume} != {expected}"
    );
}

#[test]
fn captured_vase_has_its_meridian_volume() {
    let (mut k, mut b) = input();
    assert_eq!(
        (
            b.face_list().len(),
            b.edge_list().len(),
            b.vertex_list().len()
        ),
        (5, 7, 4)
    );
    valid_volume(&mut k, &mut b, base_volume());
}

#[test]
fn closed_vase_chamfer_has_its_meridian_geometry_and_volume() {
    let (mut k, mut b) = input();
    let point = Vec3::new(-15.0, 0.0, 60.0);
    let picked = b
        .edge_list()
        .iter()
        .copied()
        .map(|e| {
            (
                e,
                edge_closest_point(&k, &b, e, point)
                    .unwrap()
                    .closest
                    .distance,
            )
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    assert!(picked.1 < 1e-8);
    let edges =
        solidcore::blend::blend_refusal::tangent_chain(&k, &mut b, &[picked.0], 0.5 * PI / 180.0)
            .unwrap();
    assert_eq!(edges.len(), 1);
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.set_swept_turns(false);
        s.prepare_chamfers_along(&k, &edges, &[ChamferSize::Distances([2.995, 2.995])], &[])
            .unwrap();
        s.create_corners(&k, None).unwrap();
        for corner in s.fillet_corners().unwrap() {
            s.set_corner_bevel(corner, true).unwrap();
        }
        s.set_overflow_first(false);
        s.set_classify_only(false);
        s.do_filleting(&mut k, None).unwrap();
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    b.refresh_cached_topology();
    chamfer_is_the_meridian_cone(&k, &b);
    valid_volume(&mut k, &mut b, expected_volume());
}
