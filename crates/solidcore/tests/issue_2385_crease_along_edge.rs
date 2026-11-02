//! #2385 (WR-717): a cliff overflow's crease that runs along an edge of the face it crosses,
//! within the trim's tolerance, is that edge (`fixes.fillet_overflow_crease_along_edge`).
//!
//! A disc 13 in radius and 2 high (SolidCraft #190's) has its top edge rounded r a little
//! past its height. The round's section is the circle of radius r about (13 - r, 2 - r) in
//! (rho, z): its rail runs down the wall's continuation to z = 2 - r, and it crosses the
//! bottom face's plane at rho = 13 - r + sqrt(4r - 4), a gap g = r - sqrt(4r - 4) (about
//! (r - 2)^2 / 4) inside the bottom edge. Where g is within the trim's tolerance (4e-5
//! here), the fork's `TrimAtCreases` (tangency-labs/smlib 5e8c9c1) drops that crease as not
//! inside the face, keeps the sheet's piece past it, takes the bottom face off with the
//! rest and refuses at the sewing: r 2.0001 to 2.013 fail where r 2 and r 2.02 build.
//!
//! With the fix the crease is the bottom edge: the sheet is split along it and sewn to the
//! edge, which stands as the imprint. The body is the disc less the round above z = 0:
//!
//! ```text
//! V(r) = 2 pi 13^2 - 2 pi int_0^r (13 - r + u) min(2, r - sqrt(r^2 - u^2)) du
//! ```
//!
//! (Pappus over the round's section, as the 190 fixture's r 2.5 case). The bottom face keeps
//! its own edge at rho = 13, within g of the crease, so its ring of width g (area
//! 2 pi 13 g) is the body's within that edge's tolerance; the volume is held to V(r) within
//! twice that ring's area (the ring at most the body's height from any reference the
//! integration takes) plus the integration's 1e-6.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::query::api_queries::{brep_is_manifold_solid, edge_closest_point};
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

const PI: f64 = std::f64::consts::PI;

/// Every flag, with WR-717's set as `on`.
fn kernel(on: bool) -> Kernel {
    let mut fixes = UpstreamFixes::all();
    fixes.fillet_overflow_crease_along_edge = on;
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = fixes;
    k
}

fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (0.1_f64, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap_or_else(|| panic!("no edge near {p:?}"))
}

/// The disc with its top edge rounded `r` (`SmApiFilletEdges`, circular section).
fn rounded(on: bool, r: f64) -> (Kernel, solidcore::Result<Brep>) {
    let mut k = kernel(on);
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 13.0, 2.0).unwrap();
    let e = pick(&k, &b, [0.0, 13.0, 2.0]);
    let built = fillet_edges(&mut k, &mut b, &[e], r, 1, 1, 1.0).map(|()| b);
    (k, built)
}

fn volume_of(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// V(r) above, by Simpson's rule on 400000 intervals (to 1e-9 here).
fn closed_form(r: f64) -> f64 {
    let n = 400_000_u32;
    let h = r / f64::from(n);
    let g = |u: f64| (13.0 - r + u) * (r - (r * r - u * u).max(0.0).sqrt()).min(2.0);
    let mut s = g(0.0) + g(r);
    for i in 1..n {
        s += g(f64::from(i) * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    2.0 * PI * 169.0 - 2.0 * PI * s * h / 3.0
}

/// Across the band, the disc builds one closed solid of three faces (top, round, bottom),
/// at V(r) within the ring's bound.
#[test]
fn a_round_just_past_its_wall_builds() {
    for r in [2.00005, 2.0001, 2.0003, 2.001, 2.005, 2.01] {
        let (k, built) = rounded(true, r);
        let mut b = built.unwrap_or_else(|e| panic!("r {r}: {e}"));
        assert!(brep_is_manifold_solid(&b).unwrap(), "r {r}");
        assert_eq!(b.face_list().len(), 3, "r {r}");
        let v = volume_of(&k, &mut b);
        let want = closed_form(r);
        let gap = r - (4.0 * r - 4.0).sqrt();
        let bound = 2.0 * (2.0 * PI * 13.0 * gap) + 1e-6;
        assert!(
            (v - want).abs() <= bound,
            "r {r}: {v} vs {want} (bound {bound:e})"
        );
    }
}

/// Without the flag the band is refused, as the fork refuses it.
#[test]
fn without_the_flag_the_band_is_refused() {
    for r in [2.0001, 2.001, 2.01] {
        let (_, built) = rounded(false, r);
        assert!(built.is_err(), "r {r}");
    }
}

/// Where the crease lies inside the bottom face (and at r 2, where nothing overflows) the
/// flag changes nothing: the same volume to the bit, at V(r) to 1e-6.
#[test]
fn a_crease_inside_the_face_is_unchanged() {
    for r in [2.0, 2.02, 2.5] {
        let (k_on, on) = rounded(true, r);
        let (k_off, off) = rounded(false, r);
        let (mut on, mut off) = (on.unwrap(), off.unwrap());
        let (v_on, v_off) = (volume_of(&k_on, &mut on), volume_of(&k_off, &mut off));
        assert_eq!(v_on.to_bits(), v_off.to_bits(), "r {r}");
        assert!((v_on - closed_form(r)).abs() <= 1e-6, "r {r}: {v_on}");
    }
}
