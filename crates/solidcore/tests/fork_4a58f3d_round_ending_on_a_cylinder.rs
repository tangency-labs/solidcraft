//! The SolidCraft fork's capi case `round_ending_on_a_cylinder` (`ops_kernel.cpp`,
//! tangency-labs/smlib@4a58f3d), smlib#79 step 4d, tangency-labs/solidcore#2273: a round
//! ending on a cylindrical end face meets it along a certified rib (the blend corner's End,
//! brought over by #2307).
//!
//! A bar 30 x 10 x 10 (y within 5) less what lies outside the cylinder R 20 about z: its end
//! at x = sqrt(400 - y^2) is the cylinder. Its top front edge (y = -5, z = 10) rounded r 2
//! ends on that end face, and the round's end rib is where the round meets the cylinder, a
//! quartic. The round takes the section 2 - sqrt(4 - (y + 3)^2) high at y in [-5, -3], each
//! strip as long as sqrt(400 - y^2): with y = -3 - 2 sin t, the integral of
//! 4 (1 - cos t) cos t sqrt(400 - y^2) over t in [0, pi/2], 16.714055107294 (Simpson, 4000
//! intervals, as the capi sums it). It is graded as the fork grades it: the removed volume to
//! 1e-6 of that, one edge between the round and the cylinder, its largest distance from the
//! cylinder (`surface_max_distance_between` with no samples, the precise maximum) within the
//! fillet's tolerance (100 times the bar's; a rib on the plane through its ends and the vertex
//! would be 0.025 off), and SMLib's validity check at level 2. The fork prints 16.714056916
//! removed (1.1e-7) and the rib 4.22e-6 off; the port gives both to the digits printed.
//!
//! With `fillet_blend_corner` the round's end on the cylinder is the blend corner's End on its
//! end face; without it, the pinned Nx1 corner. The End builds the pinned construction (#2307
//! moved it unchanged), so the result is the same either way.
//!
//! How this test drives the kernel differs from the capi: the edge is picked as the nearest
//! within 0.1 of the capi's point (10, -5, 10), and the round is [`fillet_edges`] (the
//! circular section, then `StitchAndOrient`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::blend_corner::{BlendCornerEvent, BlendEnd};
use solidcore::blend::fillet_corner::p3::FilletCornerKind;
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::boolean_intersection;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::surface::p2::{CYLINDER_TYPE, PLANE_TYPE};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge;
use solidcore::topology::face::p2 as face;
use solidcore::topology::{vertex, EdgeId};
use solidcore::{Brep, Kernel};

/// Every fix but `geometric_g2_continuity`, with or without `fillet_blend_corner` (as
/// `wr697_end_and_join.rs`).
fn all_fixes(blend: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" && (blend || name != "fillet_blend_corner") {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
}

/// The edge nearest `p`, within 0.1.
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

/// The kind and blend event of the corner at `p` when the edge `e` is rounded r 2, as
/// `CreateFilletCorners` makes it.
fn corner_at(
    k: &mut Kernel,
    b: &Brep,
    e: EdgeId,
    p: Point3,
) -> (FilletCornerKind, Option<(BlendCornerEvent, BlendEnd)>) {
    let mut b = b.clone();
    let mut s = FilletSession::new(k, &mut b).unwrap();
    s.prepare_per_edge_constant_radius(
        k,
        &[e],
        &[2.0],
        &[FilletSurfaceGeneratorType::Circular],
        1,
        1.0,
    )
    .unwrap();
    s.create_corners(k, None).unwrap();
    for id in s.fillet_corners().unwrap() {
        let c = s.corner(id).unwrap();
        let v = c.filleted_vertex().unwrap();
        if vertex::point(s.target(), v).unwrap().distance(p) < 1e-6 {
            return (c.kind, c.blend.map(|bc| (bc.event, bc.end)));
        }
    }
    panic!("no corner at {p:?}");
}

/// The volume at the capi's accuracy (`volume_of`).
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// The section's integral: Simpson's rule on 4000 intervals of t in [0, pi/2].
fn removed_closed_form() -> f64 {
    let n = 4000;
    let h = 0.5 * PI / f64::from(n);
    let mut removed = 0.0;
    for i in 0..=n {
        let t = f64::from(i) * h;
        let y = -3.0 - 2.0 * t.sin();
        let f = 4.0 * (1.0 - t.cos()) * t.cos() * (400.0 - y * y).sqrt();
        let w = if i == 0 || i == n {
            1.0
        } else if i % 2 == 1 {
            4.0
        } else {
            2.0
        };
        removed += w * f;
    }
    removed * h / 3.0
}

/// The edges between the round (a face on neither a plane nor a cylinder) and the
/// cylinder's face, each with its largest distance from the cylinder.
fn ribs(k: &Kernel, b: &Brep) -> Vec<f64> {
    let mut out = Vec::new();
    for &e in b.edge_list() {
        let mut cylinder = None;
        let mut on_round = false;
        for f in edge::faces(b, e, None).unwrap() {
            let s = face::surface(b, f).unwrap().unwrap();
            let is_cylinder = s.is_kind_of(CYLINDER_TYPE).unwrap();
            if is_cylinder {
                cylinder = Some(f);
            }
            on_round = on_round || (!is_cylinder && !s.is_kind_of(PLANE_TYPE).unwrap());
        }
        let Some(cylinder) = cylinder else { continue };
        if !on_round {
            continue;
        }
        let c = edge::curve(b, e).unwrap().unwrap();
        let gap = c
            .surface_max_distance_between(
                k,
                &edge::interval(b, e).unwrap(),
                face::surface(b, cylinder).unwrap().unwrap(),
                &face::uv_domain(b, cylinder).unwrap(),
                0,
                None,
            )
            .unwrap();
        out.push(gap.max_distance);
    }
    out
}

/// 4a58f3d: the bar's top front edge rounded r 2 ends on the cylinder along one rib within
/// the fillet's tolerance of it, and removes the section's integral.
#[test]
fn a_round_ending_on_a_cylinder_meets_it_along_a_certified_rib() {
    let want = removed_closed_form();
    assert!((want - 16.714055107294).abs() <= 1e-12, "{want}");
    // the edge's end on the cylinder, (sqrt 375, -5, 10)
    let end = Point3::new(375.0_f64.sqrt(), -5.0, 10.0);
    for blend in [true, false] {
        let mut k = all_fixes(blend);
        let bx = create_box(&mut k, Point3::new(0.0, -5.0, 0.0), 30.0, 10.0, 10.0).unwrap();
        let cyl = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 10.0).unwrap();
        let mut b = boolean_intersection(&mut k, bx, cyl).unwrap();
        let v0 = volume(&k, &mut b);
        let tol = 100.0 * b.tolerance();
        let e = pick(&k, &b, [10.0, -5.0, 10.0]);
        let (kind, event) = corner_at(&mut k, &b, e, end);
        assert_eq!(kind, FilletCornerKind::NByOne);
        assert_eq!(
            event,
            blend.then_some((BlendCornerEvent::End, BlendEnd::OnEndFace))
        );
        fillet_edges(&mut k, &mut b, &[e], 2.0, 1, 1, 0.0).unwrap();
        let removed = v0 - volume(&k, &mut b);
        let off = ribs(&k, &b);
        eprintln!(
            "blend {blend}: removed {removed:.12} want {want:.12} (rel {:.3e}); ribs {off:?} \
             (allowed {tol:.3e})",
            (removed - want).abs() / want
        );
        let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
            .unwrap()
            .lines();
        assert!(lines.is_empty(), "{lines:?}");
        assert!((removed - want).abs() <= 1e-6 * want, "{removed} vs {want}");
        assert_eq!(off.len(), 1, "{off:?}");
        assert!(off[0] <= tol, "{off:?} vs {tol}");
        // the fork's printed values, to their last digit
        assert!((removed - 16.714056916).abs() <= 5e-10, "{removed}");
        assert!((off[0] - 4.22e-6).abs() <= 5e-9, "{off:?}");
    }
}
