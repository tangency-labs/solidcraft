//! tangency-labs/solidcore#1438: `FilletSession::set_blend_curve_at_thirds` (the fork's
//! `SmFilletExecutive::SetBlendCurveAtThirds`, tangency-labs/smlib `fd3b6a7`). A G2
//! (`SM_FSG_BLEND_CURVE`, continuity 2) round r = 2 of one edge of a 30 x 20 x 10 box.
//! With the option each section is the quintic whose control points sit at 0, 1/3 and
//! 2/3 of the way from each contact to the corner, so the body loses exactly that
//! section's area to the corner times the edge length. Off (the default), the sections
//! are the pinned C++'s faired Hermite and the volume differs.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::{edge, vertex, Brep, EdgeId};
use solidcore::Kernel;

const R: f64 = 2.0;
const L: f64 = 30.0;

/// The box's top front edge, along X at y = 0, z = 10.
fn top_front_edge(b: &Brep) -> EdgeId {
    let found: Vec<_> = b
        .edges()
        .ids()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(b, e).unwrap();
            let (p, q) = (
                vertex::point(b, s.unwrap()).unwrap(),
                vertex::point(b, t.unwrap()).unwrap(),
            );
            p.y.abs() < 1e-9 && q.y.abs() < 1e-9 && p.z == 10.0 && q.z == 10.0
        })
        .collect();
    assert_eq!(found.len(), 1);
    found[0]
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// The box with its top front edge rounded G2, sections at thirds or not.
fn rounded(at_thirds: Option<bool>) -> f64 {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), L, 20.0, 10.0).unwrap();
    let e = top_front_edge(&b);
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        if let Some(on) = at_thirds {
            s.set_blend_curve_at_thirds(on);
        }
        s.prepare_per_edge_constant_radius(
            &mut k,
            &[e],
            &[R],
            &[FilletSurfaceGeneratorType::BlendCurve],
            2,
            1.0,
        )
        .unwrap();
        s.create_corners(&k, None).unwrap();
        s.do_filleting(&mut k, None).unwrap();
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    volume(&k, &mut b)
}

/// The area between the corner (the origin) and the quintic Bezier from (1, 0) to
/// (0, 1) with control points at thirds along each side: ½∮(x dy − y dx), which is zero
/// along the two sides through the corner, by 8-point Gauss–Legendre (exact for the
/// degree 9 integrand).
fn quintic_area_to_corner() -> f64 {
    let p = [
        (1.0, 0.0),
        (2.0 / 3.0, 0.0),
        (1.0 / 3.0, 0.0),
        (0.0, 1.0 / 3.0),
        (0.0, 2.0 / 3.0),
        (0.0, 1.0),
    ];
    let binom = [1.0, 5.0, 10.0, 10.0, 5.0, 1.0];
    let eval = |t: f64| {
        let (mut x, mut y, mut dx, mut dy) = (0.0, 0.0, 0.0, 0.0);
        for i in 0..6 {
            let b = binom[i] * t.powi(i as i32) * (1.0 - t).powi(5 - i as i32);
            let db = binom[i]
                * (if i > 0 {
                    i as f64 * t.powi(i as i32 - 1) * (1.0 - t).powi(5 - i as i32)
                } else {
                    0.0
                } - if i < 5 {
                    (5 - i) as f64 * t.powi(i as i32) * (1.0 - t).powi(4 - i as i32)
                } else {
                    0.0
                });
            x += b * p[i].0;
            y += b * p[i].1;
            dx += db * p[i].0;
            dy += db * p[i].1;
        }
        x * dy - y * dx
    };
    let nodes = [
        (0.183_434_642_495_649_8, 0.362_683_783_378_362_0),
        (0.525_532_409_916_329_0, 0.313_706_645_877_887_3),
        (0.796_666_477_413_626_7, 0.222_381_034_453_374_5),
        (0.960_289_856_497_536_3, 0.101_228_536_290_376_3),
    ];
    let mut integral = 0.0;
    for (x, w) in nodes {
        for s in [-x, x] {
            integral += w * eval(0.5 * (s + 1.0)) * 0.5;
        }
    }
    0.5 * integral
}

#[test]
fn g2_sections_at_thirds_remove_the_quintic_area() {
    let full = L * 20.0 * 10.0;
    let expected = full - quintic_area_to_corner() * R * R * L;
    let v = rounded(Some(true));
    assert!((v - expected).abs() < 1e-9 * full, "{v} vs {expected}");
}

#[test]
fn off_by_default_and_off_keeps_the_faired_hermite() {
    let full = L * 20.0 * 10.0;
    let at_thirds = full - quintic_area_to_corner() * R * R * L;
    let default = rounded(None);
    let off = rounded(Some(false));
    assert_eq!(default.to_bits(), off.to_bits());
    assert!((off - at_thirds).abs() > 1e-3, "{off} vs {at_thirds}");
}
