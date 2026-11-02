//! tangency-labs/solidcore#2223: the R25 rim and an R2 corner arc of a tabbed plate rounded
//! r 0.5 together (two edges apart, sharing no vertex), WR-706
//! (`UpstreamFixes::fillet_section_end_trim_at_creases`).
//!
//! The plate is #2176's: an R25 disc with a tab (sides x = +-4, R2 corners, top y = 280/9 + 2),
//! extruded 2. Each end of the corner arc runs on tangentially into a straight top edge that
//! is not picked, so WR-674 (`fillet_round_ends_at_tangent_continuation`) ends that round at
//! its section through the vertex. The subtraction that builds WR-674's section ends builds
//! only a lone round both of whose ends are section ends, so with the rim picked too the
//! fillet was refused (`FilletErrorType::NoRoom`, 1001 in `cliff_overflow`). With WR-706 the
//! section-ended round is trimmed at its creases and sewn in with the rim's round
//! (`fillet_overflow_trim_at_creases`, WR-695).
//!
//! Expected values are closed form, not the fork's: at tangency-labs/smlib 5e8c9c1 the fork
//! leaves the corner arc unrounded (rim alone, corner alone and both remove 7.95867347,
//! -3.3e-9 and 7.95867350), so SMLib is known to differ here.
//!
//! **The corner arc's wedge (Pappus).** In each half-plane through the corner's vertical axis
//! (radius R = 2) the round removes the corner of the r x r square at the filleted edge
//! outside the ball's circle: the square less the quarter disc about the square's far corner,
//! of area A = r^2 - (pi/4) r^2 = (1 - pi/4) r^2. Its centroid lies off each wall by
//! c = (r^2 (r/2) - (pi r^2/4)(r - 4r/(3 pi))) / A = r (10 - 3 pi) / (3 (4 - pi)), so at
//! rho = R - c from the axis. The two section planes contain the axis, so the removed solid
//! is A revolved through theta = pi/2, and by Pappus V = A theta rho
//! = (1 - pi/4) r^2 (pi/2) (2 - c) = 0.159135839721916.
//!
//! **Both.** The rim's round ends on the tab's flat sides at y = sqrt(609) = 24.68, the corner
//! arc's round starts at y = 280/9 = 31.11, so the two removed regions are disjoint and the
//! fillet removes the rim's 7.958674274526857 (the rim alone, as #2176's test pins it) plus
//! the wedge: 8.117810114248773.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::curve::curve::Curve;
use solidcore::error::SolidError;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{Brep, EdgeId};
use std::f64::consts::PI;

const RADIUS: f64 = 0.5;
/// A point on the top rim arc.
const RIM: [f64; 3] = [0.0, -25.0, 2.0];
/// A point on the R2 corner arc at x > 0.
const CORNER: [f64; 3] = [3.414_213_56, 32.525_324_7, 2.0];
/// What the rim's round removes alone (#2176's `the_neighbours_keep_their_rounds`).
const RIM_REMOVED: f64 = 7.958_674_274_526_857;

/// Every opt-in fix on (as in the issue), with WR-706 as given.
fn kernel(wr706: bool) -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        *f.flag_mut(name).unwrap() = true;
    }
    f.fillet_section_end_trim_at_creases = wr706;
    k.fixes = f;
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The edge nearest `p`.
fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

fn line(a: (f64, f64), b: (f64, f64)) -> Curve {
    Curve::from(create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap())
}

fn arc(c: (f64, f64), r: f64, from: f64, to: f64) -> Curve {
    Curve::from(
        create_arc(
            Vec3::new(c.0, c.1, 0.0),
            r,
            from.to_degrees(),
            to.to_degrees(),
        )
        .unwrap(),
    )
}

/// R25 disc with a tab (sides x = +-4, R2 corners, top y = 280/9 + 2), 2 thick.
fn plate(k: &mut Kernel) -> Brep {
    let t = (4.0_f64 / 25.0).acos();
    let y = (625.0_f64 - 16.0).sqrt();
    let cy = 280.0 / 9.0;
    let curves = [
        arc((0.0, 0.0), 25.0, PI - t - 2.0 * PI, t),
        line((4.0, y), (4.0, cy)),
        arc((2.0, cy), 2.0, 0.0, PI / 2.0),
        line((2.0, cy + 2.0), (-2.0, cy + 2.0)),
        arc((-2.0, cy), 2.0, PI / 2.0, PI),
        line((-4.0, cy), (-4.0, y)),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 2.0, true).unwrap()
}

/// The corner arc's wedge, by Pappus (the file header).
fn pappus() -> f64 {
    let c = RADIUS * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    (1.0 - PI / 4.0) * RADIUS * RADIUS * (PI / 2.0) * (2.0 - c)
}

/// The fillet of the edges nearest `picks`: the volume it removes and the body's face count,
/// the body checked sound.
fn removed(wr706: bool, picks: &[[f64; 3]]) -> Result<(f64, usize), SolidError> {
    let mut k = kernel(wr706);
    let mut b = plate(&mut k);
    let before = volume(&k, &mut b);
    let es: Vec<EdgeId> = picks.iter().map(|p| pick(&k, &b, *p)).collect();
    fillet_edges(&mut k, &mut b, &es, RADIUS, 1, 1, 1.0)?;
    b.refresh_cached_topology();
    assert!(is_manifold_solid(&b).unwrap());
    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "{:?}", check.lines());
    Ok((before - volume(&k, &mut b), b.face_list().len()))
}

#[test]
fn the_pappus_wedge_is_the_closed_form() {
    assert!(
        (pappus() - 0.159_135_839_721_916).abs() <= 1e-15,
        "{}",
        pappus()
    );
}

/// The issue's case: both rounds built, the rim's volume plus the corner arc's wedge.
#[test]
fn rim_and_corner_arc_round_together() {
    let (both, faces) = removed(true, &[RIM, CORNER]).unwrap();
    let expected = RIM_REMOVED + pappus();
    assert!(
        (both - expected).abs() <= 1e-7,
        "removed {both}, rim + wedge {expected}"
    );
    // the plate's 8 faces, the rim's round, the corner arc's round and its two flat ends
    assert_eq!(faces, 12);
}

/// The two edges picked the other way round build the same body.
#[test]
fn corner_arc_and_rim_round_together() {
    let (both, faces) = removed(true, &[CORNER, RIM]).unwrap();
    assert!(
        (both - RIM_REMOVED - pappus()).abs() <= 1e-7,
        "removed {both}"
    );
    assert_eq!(faces, 12);
}

/// Each edge alone, through the trim, removes what it removes by the subtraction.
#[test]
fn each_edge_alone_is_unchanged() {
    let (rim, faces) = removed(true, &[RIM]).unwrap();
    assert!((rim - RIM_REMOVED).abs() <= 1e-9, "rim {rim}");
    assert_eq!(faces, 9);
    let (corner, faces) = removed(true, &[CORNER]).unwrap();
    assert!(
        (corner - pappus()).abs() <= 1e-7,
        "corner {corner}, Pappus {}",
        pappus()
    );
    // the plate's 8 faces, the round and its two flat ends
    assert_eq!(faces, 11);
}

/// Without WR-706 the subtraction refuses the round with a section end next to another
/// round (the issue's 1001), and still builds the corner arc alone.
#[test]
fn without_the_fix_the_subtraction_refuses_both() {
    let err = removed(false, &[RIM, CORNER]).unwrap_err();
    assert!(
        matches!(err, SolidError::Upstream { code: 1001, .. }),
        "{err:?}"
    );
    let (corner, faces) = removed(false, &[CORNER]).unwrap();
    assert!((corner - pappus()).abs() <= 1e-7, "corner {corner}");
    assert_eq!(faces, 11);
}
