//! tangency-labs/solidcore#2905: a tangent chain chamfered through a round's end arc, the
//! chain of a box's top edge, the arc and its vertical edge. On one offset it builds, its
//! volume in closed form. On unequal offsets the chamfers, each by its definition on its own
//! offsets, have no section that ends both at the smooth vertices: refused, naming why and
//! the offset that builds, rather than a bare face failure.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::blend_refusal::tangent_chain;
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_executive::NoRoomReason;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::arc::ArcDefinition;
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::curve::path::PathPiece;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::det;
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::procedural_blend::chamfer::ChamferSize;
use solidcore::surface::procedural_blend::convert::{rebuild, to_record};
use solidcore::surface::procedural_blend::record::RadiusRecord;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::is_outward_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2::surface;
use solidcore::topology::{Brep, EdgeId};
use solidcore::{Kernel, Vec3};

const R: f64 = 3.0;
const D: f64 = 0.473;

/// A refusal: its reason, and the size that builds and on which edge.
type Refused = (Option<NoRoomReason>, Option<f64>, Option<EdgeId>);

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

fn nearest(k: &Kernel, b: &Brep, p: Point3) -> EdgeId {
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        let d = edge_closest_point(k, b, e, p).unwrap().closest.distance;
        if d < best.0 {
            best = (d, Some(e));
        }
    }
    best.1.unwrap()
}

/// A kernel with the chamfer's definition on.
fn kernel() -> Kernel {
    Kernel::default()
}

/// A 40 x 30 x 20 box with its top front edge rounded `R`, and its chain round the side face
/// x = 40: the top edge, the round's end arc, the vertical edge.
fn rounded(k: &mut Kernel) -> (Brep, [EdgeId; 3]) {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let top = nearest(k, &b, Point3::new(20.0, 0.0, 20.0));
    fillet_edges(k, &mut b, &[top], R, 1, 1, 1.0).unwrap();
    let c = 0.5 * std::f64::consts::SQRT_2 * R;
    let chain = [
        nearest(k, &b, Point3::new(40.0, 15.0, 20.0)),
        nearest(k, &b, Point3::new(40.0, R - c, 20.0 - R + c)),
        nearest(k, &b, Point3::new(40.0, 0.0, 8.0)),
    ];
    (b, chain)
}

fn chamfer(
    k: &mut Kernel,
    b: &mut Brep,
    chain: &[EdgeId; 3],
    offsets: [f64; 3],
) -> Result<(), Refused> {
    blend(k, b, chain, offsets, FilletSurfaceGeneratorType::Linear)
}

fn blend(
    k: &mut Kernel,
    b: &mut Brep,
    chain: &[EdgeId; 3],
    offsets: [f64; 3],
    section: FilletSurfaceGeneratorType,
) -> Result<(), Refused> {
    let mut s = FilletSession::new(k, b).unwrap();
    s.prepare_per_edge_constant_radius_with_second(
        k,
        chain,
        &offsets,
        &offsets,
        &[section],
        1,
        1.0,
    )
    .unwrap();
    s.create_corners(k, None).unwrap();
    if section == FilletSurfaceGeneratorType::Linear {
        for id in s.fillet_corners().unwrap() {
            s.set_corner_bevel(id, true).unwrap();
        }
    }
    if s.do_filleting(k, None).is_ok() {
        return Ok(());
    }
    let fit = s.fit_size();
    Err((
        s.error_info().no_room_reason(),
        fit.as_ref().map(|f| f.size),
        fit.and_then(|f| f.edge),
    ))
}

/// One offset on the chain: two prisms of the section `D^2 / 2`, along 27 and 17, and the
/// section revolved a quarter turn, its centroid at `R - D / 3` from the arc's axis.
#[test]
fn a_chain_on_one_offset_builds_its_closed_form() {
    let mut k = kernel();
    let (mut b, chain) = rounded(&mut k);
    let before = volume(&k, &mut b);
    chamfer(&mut k, &mut b, &chain, [D; 3]).unwrap();
    let section = 0.5 * D * D;
    let removed = section * (27.0 + 17.0) + 0.5 * PI * (R - D / 3.0) * section;
    let got = before - volume(&k, &mut b);
    assert!((got - removed).abs() < 1e-5, "{got} vs {removed}");
}

/// The middle edge on a wider offset: refused at the Join, the other offset naming the size
/// that builds on that edge. The refused chamfer's reason survives to the caller.
#[test]
fn a_chain_on_unequal_offsets_is_refused_by_name() {
    let mut k = kernel();
    let (mut b, chain) = rounded(&mut k);
    let wider = 0.5430636408071486;
    let (why, fit, edge) = chamfer(&mut k, &mut b, &chain, [D, wider, D]).unwrap_err();
    assert_eq!(why, Some(NoRoomReason::JoinOffsetsDiffer));
    assert_eq!(fit, Some(D));
    assert_eq!(edge, Some(chain[1]));
}

/// Rounds on unequal radii are the same refusal.
#[test]
fn a_round_chain_on_unequal_radii_is_refused_by_name() {
    let mut k = kernel();
    let (mut b, chain) = rounded(&mut k);
    let (why, fit, edge) = blend(
        &mut k,
        &mut b,
        &chain,
        [0.4, 0.6, 0.4],
        FilletSurfaceGeneratorType::Circular,
    )
    .unwrap_err();
    assert_eq!(why, Some(NoRoomReason::JoinOffsetsDiffer));
    assert_eq!(fit, Some(0.4));
    assert_eq!(edge, Some(chain[1]));
}

/// Offsets that differ by less than a loop closes are not this refusal's.
#[test]
fn offsets_within_the_loop_gap_are_not_refused_by_name() {
    let mut k = kernel();
    let (mut b, chain) = rounded(&mut k);
    let near = D + 0.5 * b.tolerance();
    if let Err((why, ..)) = chamfer(&mut k, &mut b, &chain, [D, near, D]) {
        assert_ne!(why, Some(NoRoomReason::JoinOffsetsDiffer));
    }
}

const SLANTED_END: f64 = (22.266243 - 17.733757) / 60.0;

fn bar_with_cut_end(k: &mut Kernel, slope: f64) -> Brep {
    let line = |a, b| Curve::from(Line::create_line_segment(3, a, b, None).unwrap());
    let arc = |x, z, quarter| {
        let step = 0.5 * PI;
        PathPiece::of_arc(
            k,
            &ArcDefinition {
                centre: Point3::new(x, 0.0, z),
                axis: -Vec3::Y,
                reference: Vec3::X,
                radius: 1.0,
                start: quarter * step,
                end: (quarter + 1.0) * step,
            },
        )
        .unwrap()
        .curve()
        .clone()
    };
    let p = |x, z| Point3::new(x, 0.0, z);
    let profile = [
        line(p(-19.0, 0.0), p(19.0, 0.0)),
        arc(19.0, 1.0, -1.0),
        line(p(20.0, 1.0), p(20.0, 5.0)),
        arc(19.0, 5.0, 0.0),
        line(p(19.0, 6.0), p(-19.0, 6.0)),
        arc(-19.0, 5.0, 1.0),
        line(p(-20.0, 5.0), p(-20.0, 1.0)),
        arc(-19.0, 1.0, 2.0),
    ];
    let bar = create_linear_sweep(k, &profile.each_ref().map(Some), Vec3::Y, 40.0, true).unwrap();

    let cut = [
        Point3::new(-30.0, -5.0, -5.0),
        Point3::new(30.0, -5.0, -5.0),
        Point3::new(30.0, 20.0 + 30.0 * slope, -5.0),
        Point3::new(-30.0, 20.0 - 30.0 * slope, -5.0),
    ];
    let profile = std::array::from_fn::<_, 4, _>(|i| line(cut[i], cut[(i + 1) % cut.len()]));
    let cutter =
        create_linear_sweep(k, &profile.each_ref().map(Some), Vec3::Z, 25.0, true).unwrap();
    boolean_difference(k, bar, cutter).unwrap()
}

fn end_rim(k: &Kernel, body: &mut Brep) -> Vec<EdgeId> {
    let start = nearest(k, body, Point3::new(0.0, 20.0, 6.0));
    let chain = tangent_chain(k, body, &[start], 0.5_f64.to_radians()).unwrap();
    assert_eq!(chain.len(), 8);
    chain
}

fn assert_on_rail(k: &Kernel, body: &Brep, point: Point3, tolerance: f64) {
    let rail = nearest(k, body, point);
    let distance = edge_closest_point(k, body, rail, point)
        .unwrap()
        .closest
        .distance;
    assert!(distance < tolerance, "rail at {point:?}: gap {distance}");
}

#[test]
fn a_chamfer_on_a_cut_rounded_bar_keeps_its_setbacks() {
    for (slope, width) in [(0.0_f64, 0.2), (SLANTED_END, 0.2), (SLANTED_END, 0.0123)] {
        let mut k = kernel();
        let mut body = bar_with_cut_end(&mut k, slope);
        let before = volume(&k, &mut body);
        assert!((before - 20.0 * (236.0 + PI)).abs() < 1e-6);

        let chain = end_rim(&k, &mut body);
        let mut session = FilletSession::new(&mut k, &mut body).unwrap();
        session
            .prepare_chamfers_along(&k, &chain, &[ChamferSize::Distances([width; 2])], &[])
            .unwrap();
        if let Err(err) = session.do_fillet(&mut k) {
            panic!(
                "slope {slope}, width {width}: {err:?}, {:?}",
                session.error_info().no_room_reason()
            );
        }
        drop(session);

        let findings = check_brep(&mut k, &mut body, CheckLevel::Level2)
            .unwrap()
            .lines();
        assert!(findings.is_empty(), "{findings:?}");
        assert!(is_outward_solid(&mut k, &mut body).unwrap());
        assert_eq!(body.face_list().len(), 18);
        assert!(volume(&k, &mut body) < before);

        // In each plane, the rail is the requested width from the original straight rim.
        let normal_scale = det::hypot(1.0_f64, slope);
        let mut rails = vec![
            Point3::new(0.0, 20.0 + width * normal_scale, 6.0),
            Point3::new(0.0, 20.0, 6.0 - width),
        ];
        for side in [-1.0, 1.0] {
            let x = side * 20.0;
            let on_cap = x - side * width / normal_scale;
            rails.push(Point3::new(x, 20.0 + slope * x + width, 3.0));
            rails.push(Point3::new(on_cap, 20.0 + slope * on_cap, 3.0));
        }
        for point in rails {
            assert_on_rail(&k, &body, point, 1e-6);
        }

        // At each upper arc's 45-degree station, shift the two tangent planes by
        // the radius for the requested width in their wedge. The cylinder's radial normal has
        // zero Y component, so its contact lies at the original X and Z.
        let cap_inward = Vec3::new(-slope, 1.0, 0.0) / normal_scale;
        let across = std::f64::consts::FRAC_1_SQRT_2;
        for side in [-1.0, 1.0] {
            let lateral_inward = Vec3::new(-side * across, 0.0, -across);
            let wedge = det::acos(-lateral_inward.dot(cap_inward));
            let radius = width * det::tan(0.5 * wedge);
            let x = side * (19.0 + across);
            let y = 20.0 + slope * (x + radius * lateral_inward.x) + radius * normal_scale;
            let lateral = Point3::new(x, y, 5.0 + across);
            let cap = lateral + radius * (lateral_inward - cap_inward);
            assert_on_rail(&k, &body, lateral, 1e-5);
            assert_on_rail(&k, &body, cap, 1e-5);
        }

        if slope != 0.0 {
            let definitions: Vec<_> = body
                .face_list()
                .iter()
                .filter_map(|&id| match surface(&body, id).unwrap().map(Surface::kind) {
                    Some(SurfaceKind::ProceduralBlend(blend)) => {
                        Some(to_record(&k, blend).unwrap())
                    }
                    _ => None,
                })
                .filter(|record| matches!(record.definition.radius, RadiusRecord::EqualSetback))
                .collect();
            assert_eq!(definitions.len(), 4);
            let restored = rebuild(&k, &definitions[0]).unwrap();
            assert!(restored.recertify(&k).unwrap() <= restored.bound());
        }
    }
}

#[test]
fn a_cut_bar_chamfer_with_mismatched_setbacks_is_refused_at_the_join() {
    let mut k = kernel();
    let mut body = bar_with_cut_end(&mut k, SLANTED_END);
    let chain = end_rim(&k, &mut body);
    let mut sizes = vec![ChamferSize::Distances([0.2; 2]); chain.len()];
    sizes[0] = ChamferSize::Distances([0.4; 2]); // the top run
    let mut session = FilletSession::new(&mut k, &mut body).unwrap();
    session
        .prepare_chamfers_along(&k, &chain, &sizes, &[])
        .unwrap();
    session.do_fillet(&mut k).unwrap_err();
    assert_eq!(
        session.error_info().no_room_reason(),
        Some(NoRoomReason::JoinOffsetsDiffer)
    );
}
