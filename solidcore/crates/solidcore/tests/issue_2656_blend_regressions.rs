//! tangency-labs/solidcore#2656: the blend regressions SolidCraft's pin bump found
//! (tangency-labs/solidcraft#11875, 8ddce9016 to 5d2635d36), on
//! the blend corner (WR-697), with SolidCraft's fix set at that pin.
//!
//! **#2364 (smlib#124's End at a smooth vertex).** A blended edge that runs on tangentially
//! into an edge of its convexity that is not blended was capped there: the blend cut by the
//! plane through the vertex square to its edge, a face meeting the round and the part at 90
//! degrees. Where the continuing edge turns into the two edges' shared face away from the
//! blended edge continued (an earlier round's end arc, or a round between two faces of a
//! corner), the blend run on past the vertex runs out of the part, and the End on the end
//! face builds that, as before smlib#124: the closed forms below. Where it does not (a
//! rounded corner arc running on into a straight side, #2176; one face round the vertex,
//! #453), the cap stays (their tests: `issue_2176_round_of_arc_between_tangent_straights.rs`,
//! `issue_453_bore_rim_smooth_end.rs`). Without the fix the #2364 tests here fail: the cap
//! adds a face (ladder 012: 9 faces, 47770.945948; 1207: 9 faces, 7957.723438) or builds a
//! capped body where the run-on is refused (the curvature rounds).
//!
//! **#2369 (the setback corner).** The corner of SolidCraft's
//! report-fillet-curvature-three-edges-at-bar-corner-solver-residues-outside-setback builds
//! on main with SolidCraft's calls, and the cut outside its setback is the closed form.
//!
//! The cases are SolidCraft's (the kernel rig's entries in `tests/kernel_cases/`, its
//! ladder recipes and its tests), driven as SolidCraft's `blend_edges` and `fillet_variable`
//! drive the session. The values are closed forms (SolidCraft's `expected.json`) where the
//! case has one.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/solidcraft_fixes.rs"]
mod solidcraft_fixes;

use solidcore::blend::blend_corner::BlendCornerEvent;
use solidcore::blend::fillet_executive::p1::{FilletLaw, FilletLawKind};
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_section::section::GN_THUMBWEIGHT_EQUAL_AREA;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::p1::{
    BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::loopuse;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, Brep, EdgeId};
use solidcore::Kernel;
use std::f64::consts::FRAC_PI_4;

/// SolidCraft's whole preset (`support/solidcraft_fixes.rs`). That includes the 5d2635d36 bump's
/// other fixes (tangency-labs/solidcraft#11875: `closed_smooth_pole_tangent_limit`; its
/// `fillet_chamfer_corner` and `face_swap_uv_transposes_trims` are no longer flags), and
/// since the preset is `core::fixes()`, `brep_ray_classifies_current_trims`. Every value here is unchanged under it.
fn solidcraft_kernel() -> Kernel {
    Kernel {
        fixes: solidcraft_fixes::solidcraft_fixes(),
        ..Kernel::default()
    }
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The edge nearest `p` (SolidCraft's pick: within its tolerance).
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
    assert!(best.0 < 0.5, "no edge at {p:?}");
    best.1.unwrap()
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    Round,
    Curvature,
}

/// The refusal's error code and comment.
type Refused = (i32, String);

/// SolidCraft's `blend_edges` (one call, no setbacks): the edges at `at` blended `size`,
/// the corners made, filleted, stitched.
fn blend(
    k: &mut Kernel,
    b: &mut Brep,
    kind: Kind,
    at: &[[f64; 3]],
    size: f64,
) -> Result<(), Refused> {
    let es: Vec<EdgeId> = at.iter().map(|&p| pick(k, b, p)).collect();
    let sizes = vec![size; es.len()];
    let done = {
        let mut s = FilletSession::new(k, b).unwrap();
        let prepared = match kind {
            Kind::Round => s.prepare_per_edge_constant_radius(
                k,
                &es,
                &sizes,
                &[FilletSurfaceGeneratorType::Circular],
                1,
                1.0,
            ),
            Kind::Curvature => s.prepare_per_edge_constant_radius(
                k,
                &es,
                &sizes,
                &[FilletSurfaceGeneratorType::BlendCurve],
                2,
                GN_THUMBWEIGHT_EQUAL_AREA,
            ),
        };
        prepared
            .and_then(|()| s.create_corners(k, None))
            .and_then(|()| s.do_filleting(k, None))
            .map_err(|_| (s.error_info().code, s.error_info().comment.clone()))
    };
    done?;
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    b.refresh_cached_topology();
    Ok(())
}

/// SolidCraft's `fillet_variable` (its `law_along`): the edge at `at` rounded by the
/// linear law from `r0` at the end nearest `start` to `r1`, a degree-1 B-spline law.
fn variable(k: &mut Kernel, b: &mut Brep, at: [f64; 3], start: [f64; 3], r0: f64, r1: f64) {
    let e = pick(k, b, at);
    let c = edge::curve(b, e).unwrap().unwrap();
    let ivl = edge::interval(b, e).unwrap();
    let lo = c.evaluate_point(k, ivl.min()).unwrap();
    let hi = c.evaluate_point(k, ivl.max()).unwrap();
    let s = Point3::new(start[0], start[1], start[2]);
    let (a, z) = if (hi - s).length() < (lo - s).length() {
        (r1, r0)
    } else {
        (r0, r1)
    };
    let cps = [Vec3::new(a, 0.0, 0.0), Vec3::new(z, 1.0, 0.0)];
    let law_curve = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 1,
        control_points: &cps,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[2, 2],
        knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
    .unwrap();
    let law = FilletLaw::new(FilletLawKind::BSpline {
        law_curve,
        extended_law: None,
    });
    {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_variable_radius_edges(k, &[e], r0.max(r1), law, 1, 1)
            .unwrap();
        s.create_corners(k, None).unwrap();
        s.do_filleting(k, None).unwrap();
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    b.refresh_cached_topology();
}

/// `(faces, edges, vertices)` and the volume, the body checked a valid manifold solid.
fn measured(k: &mut Kernel, b: &mut Brep) -> ((usize, usize, usize), f64) {
    let v = volume(k, b);
    assert!(is_manifold_solid(b).unwrap());
    assert!(check_brep(k, b, CheckLevel::Level2).unwrap().is_valid());
    (
        (
            b.faces().ids().count(),
            b.edges().ids().count(),
            b.vertices().ids().count(),
        ),
        v,
    )
}

fn assert_rel(v: f64, want: f64, rel: f64, what: &str) {
    assert!(
        (v - want).abs() <= rel * want.abs(),
        "{what}: volume {v:.9}, want {want:.9} to {rel:e}"
    );
}

fn boxed(k: &mut Kernel, l: f64, w: f64, h: f64) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), l, w, h).unwrap();
    b.refresh_cached_topology();
    b
}

fn read_smb(k: &mut Kernel, bytes: &[u8]) -> Brep {
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &loopuse::notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes.to_vec()), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

/// Ladder 07-fillets/012-variable-into-rounded-corner: the 60 x 40 x
/// 20 box's upright (60, 40) rounded r5, then the top back edge by the linear law r1 (x = 0)
/// to r5 (x = 55), which at x = 55 runs on tangentially into the upright round's top arc and
/// on into its quarter column until it leaves the part. SolidCraft's closed form: V = 48000 -
/// 121.753137 - 107.300918 - 15.820019 = 47755.125926 (to 1e-6 there), 8 faces, 18 edges,
/// 12 vertices. Capped (#2364): 9 faces, 47770.945948.
#[test]
fn ladder_012_variable_round_runs_on_into_the_upright_round() {
    let mut k = solidcraft_kernel();
    let mut b = boxed(&mut k, 60.0, 40.0, 20.0);
    blend(&mut k, &mut b, Kind::Round, &[[60.0, 40.0, 10.0]], 5.0).unwrap();
    variable(
        &mut k,
        &mut b,
        [27.5, 40.0, 20.0],
        [0.0, 40.0, 20.0],
        1.0,
        5.0,
    );
    let (counts, v) = measured(&mut k, &mut b);
    assert_eq!(counts, (8, 18, 12));
    assert_rel(v, 47_755.125_926, 1e-8, "012");
}

/// Ladder 07-fillets/021-corner-three-sequential-r5-r3-r2-partial-chain:
/// the box's top back edge r5, the top right edge r3 alone (it runs
/// on at y = 35 into the r5 round's end arc), then the upright (60, 40) r2 with its tangent
/// chain, up the arc and along the seam. The case's invariants: 11 faces, one valid body;
/// SolidCraft's golden (8ddce9016): 24 edges, 15 vertices. Capped (#2364): 25 edges,
/// 16 vertices, 47591.482. The volume is the rolling-ball volume (#18166; each piece by
/// piecewise Gauss-Kronrod quadrature split at every boundary change, within 1e-11):
///
/// ```text
/// 48000 - (1 - pi/4)(25*60 + 9*35 + 4*15)
///       - 4.293342478629   r3 run-on
///       - 1.608436379598   end arc (Pappus)
///       - 1.158579441504   end arc under the r3 cut
///       - 0.790151961430   r3/r5 seam
///       = 47589.771046109
/// ```
///
/// It replaces SolidCraft's measured 47589.770467, which is 1.2e-8 off it, and the
/// 47589.770999 measured once a round is fitted as finely as its edge is known (#16735),
/// 1.0e-9 off it.
#[test]
fn ladder_021_round_runs_on_into_the_earlier_round() {
    let mut k = solidcraft_kernel();
    let mut b = boxed(&mut k, 60.0, 40.0, 20.0);
    blend(&mut k, &mut b, Kind::Round, &[[30.0, 40.0, 20.0]], 5.0).unwrap();
    blend(&mut k, &mut b, Kind::Round, &[[60.0, 20.0, 20.0]], 3.0).unwrap();
    let (counts, _) = measured(&mut k, &mut b);
    assert_eq!(counts, (8, 18, 12), "the r3 run on, uncapped");
    let chain = [
        [60.0, 40.0, 7.5],
        [60.0, 39.791287847477925, 16.0],
        [58.931891825700944, 37.559849485548405, 19.29401699439179],
    ];
    blend(&mut k, &mut b, Kind::Round, &chain, 2.0).unwrap();
    let (counts, v) = measured(&mut k, &mut b);
    assert_eq!(counts, (11, 24, 15));
    assert_rel(v, 47_589.771_046, 1e-8, "021");
}

/// fillets/1207-fillet-box-end-edge-into-unblended-round-partial-chain, also SolidCraft's
/// `smooth_edges_and_unfinished_tangent_chains_are_refused_saying_why`: the cube 20's top back
/// edge r3, then the end face's top edge r1 alone, which at y = 17 runs on into the r3 round's
/// end arc and through the round until it leaves the part at y = 17 + sqrt 5. SolidCraft's
/// closed form 8000 - 38.628316 - 3.648231 - 0.218236 = 7957.505202, 8 faces. Capped (#2364):
/// 9 faces, 7957.723438.
#[test]
fn rig_1207_round_runs_on_past_the_end_arc() {
    let mut k = solidcraft_kernel();
    let mut b = boxed(&mut k, 20.0, 20.0, 20.0);
    blend(&mut k, &mut b, Kind::Round, &[[10.0, 20.0, 20.0]], 3.0).unwrap();
    blend(&mut k, &mut b, Kind::Round, &[[0.0, 8.5, 20.0]], 1.0).unwrap();
    let (counts, v) = measured(&mut k, &mut b);
    assert_eq!(counts, (8, 18, 12));
    assert_rel(v, 7_957.505_202, 1e-8, "1207");
}

/// SolidCraft's `ops::tests::fillet_takes_exactly_the_picked_edges`:
/// the cube 20's upright (20, 20) rounded r5, then one straight edge of the top rim (now
/// line, arc, line) r1: one face more (8), the round run on into the upright round's top arc.
#[test]
fn rim_edge_runs_on_into_the_upright_round() {
    let mut k = solidcraft_kernel();
    let mut b = boxed(&mut k, 20.0, 20.0, 20.0);
    blend(&mut k, &mut b, Kind::Round, &[[20.0, 20.0, 10.0]], 5.0).unwrap();
    blend(&mut k, &mut b, Kind::Round, &[[7.5, 20.0, 20.0]], 1.0).unwrap();
    let ((faces, _, _), _) = measured(&mut k, &mut b);
    assert_eq!(faces, 8);
}

/// SolidCraft's `robustness::fillet_into_an_earlier_round`: the 60 x
/// 40 x 15 box's top front edge r2, then the front right upright r2 or r3 alone, which runs
/// on into the first round's end arc: each takes off about what it would alone, 36000 - (1 -
/// pi/4) (4 * 60 + r^2 * 15), within SolidCraft's 2, with one face more.
#[test]
fn upright_runs_on_into_the_earlier_round() {
    for r in [2.0, 3.0] {
        let mut k = solidcraft_kernel();
        let mut b = boxed(&mut k, 60.0, 40.0, 15.0);
        blend(&mut k, &mut b, Kind::Round, &[[30.0, 0.0, 15.0]], 2.0).unwrap();
        blend(&mut k, &mut b, Kind::Round, &[[60.0, 0.0, 7.0]], r).unwrap();
        let ((faces, _, _), v) = measured(&mut k, &mut b);
        assert_eq!(faces, 8, "r {r}");
        let want = 36000.0 - (1.0 - FRAC_PI_4) * (4.0 * 60.0 + r * r * 15.0);
        assert!((v - want).abs() < 2.0, "r {r}: {v} vs {want}");
    }
}

/// fillets/5158 and fillets/1080 in SolidCraft's two passes (its `overflow_first`: the rounds
/// that overflow first, then the others), with no faces handed over as the first pass's
/// (`add_earlier_pass_face`, which SolidCraft does not call). The
/// second rounds run on into the first ones' end arcs as before smlib#124: the plate 8
/// faces, SolidCraft's closed form 1681.903758348 (to 1e-5 there); the notched block 13
/// faces, 10198.718249 (the fork's two passes, 9822462; its closed form 10198.718039, 2.1e-8
/// off) until `brep_volume` closed tolerant edges' gaps (#16735): the 2.1e-8 was the
/// integrator's leak, and the block is now the closed form 10198.718038816 to 5.8e-12
/// (MEASURED). Capped (#2364): 10 and 15 faces.
#[test]
fn second_pass_runs_on_into_the_first_without_its_faces() {
    let mut k = solidcraft_kernel();
    let mut b = read_smb(
        &mut k,
        include_bytes!("data/issue_2364_solidcraft_5158_plate.smb"),
    );
    blend(&mut k, &mut b, Kind::Round, &[[-15.0, 0.0, 2.0]], 2.0).unwrap();
    blend(&mut k, &mut b, Kind::Round, &[[0.0, 15.5, 2.0]], 2.0).unwrap();
    let ((faces, _, _), v) = measured(&mut k, &mut b);
    assert_eq!(faces, 8, "5158");
    assert_rel(v, 1_681.903_758_348, 1e-8, "5158");

    let mut k = solidcraft_kernel();
    let mut b = read_smb(
        &mut k,
        include_bytes!("data/issue_2364_solidcraft_1080_notched_block.smb"),
    );
    let first = [[20.0, 0.0, 10.0], [40.0, 15.0, 10.0]];
    blend(&mut k, &mut b, Kind::Round, &first, 8.0).unwrap();
    let second = [[20.0, 30.0, 10.0], [0.0, 15.0, 10.0]];
    blend(&mut k, &mut b, Kind::Round, &second, 8.0).unwrap();
    let ((faces, _, _), v) = measured(&mut k, &mut b);
    assert_eq!(faces, 13, "1080");
    assert_rel(v, 10_198.718_038_816, 1e-10, "1080");
}

/// fillets/every-fillet-after-fillet-curvature-front-then-side-r3-partial-chain:
/// the 40 x 30 x 20 box's top front edge rounded curvature-continuous
/// 3, then the top side edge the same way alone, which at y = 3 runs on into the first
/// round's end curve (a curvature-continuous section: it leaves the top with almost no
/// curvature, then turns down). Run on it is refused, as before smlib#124, which the case
/// accepts ("solid or refused"); capped (#2364) it was a body with a 90 degree seam where G2
/// is wanted.
#[test]
fn curvature_side_round_running_on_into_the_front_round_is_not_capped() {
    let mut k = solidcraft_kernel();
    let mut b = boxed(&mut k, 40.0, 30.0, 20.0);
    blend(&mut k, &mut b, Kind::Curvature, &[[20.0, 0.0, 20.0]], 3.0).unwrap();
    let r = blend(&mut k, &mut b, Kind::Curvature, &[[40.0, 16.5, 20.0]], 3.0);
    assert!(r.is_err(), "the run on refused, not capped");
}

/// fillets/report-fillet-curvature-three-edges-at-bar-corner-solver-residues-outside-setback
/// (#2369) with SolidCraft's calls: the residues bar's three edges at
/// its top corner rounded curvature-continuous 2, the vertex set back
/// (`add_set_back_vertex`), its corner (the Setback) set back 3 (`set_corner_setback`, the
/// G2 corner's default 1.5 r) with its patches marked; then the cube [-1, 3]^2 x [7, 11]
/// about the corner cut away, which takes the whole patch (the rails end at sqrt(3^2 + 2^2)
/// from the corner). SolidCraft's closed form 9526.705048390, 13 faces.
#[test]
fn outside_setback_with_solidcraft_s_calls_is_the_closed_form() {
    let mut k = solidcraft_kernel();
    let mut b = read_smb(
        &mut k,
        include_bytes!("data/issue_2369_g2_bar_corner_residues.smb"),
    );
    let at = [
        [0.4892940775280783, 0.0579, 4.0],
        [21.53, 0.0579, 10.0],
        [0.4892940775280783, 8.058, 10.0],
    ];
    let es: Vec<EdgeId> = at.iter().map(|&p| pick(&k, &b, p)).collect();
    let corner = Point3::new(0.4892940775280783, 0.05792545052986289, 10.0);
    let v = b
        .vertices()
        .ids()
        .find(|&v| b.vertex(v).unwrap().point.distance(corner) < 1e-6)
        .unwrap();
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(
            &mut k,
            &es,
            &[2.0; 3],
            &[FilletSurfaceGeneratorType::BlendCurve],
            2,
            GN_THUMBWEIGHT_EQUAL_AREA,
        )
        .unwrap();
        s.add_set_back_vertex(v);
        s.create_corners(&k, None).unwrap();
        let mut set = 0;
        for c in s.fillet_corners().unwrap() {
            let fc = s.corner(c).unwrap();
            if fc.vertex == Some(v) {
                assert_eq!(fc.blend.map(|b| b.event), Some(BlendCornerEvent::Setback));
                s.set_corner_setback(c, 3.0).unwrap();
                set += 1;
            }
        }
        assert_eq!(set, 1);
        s.set_mark_corner_patches(true);
        s.do_filleting(&mut k, None).unwrap();
        assert_eq!(s.corner_patch_faces().unwrap().len(), 6);
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    b.refresh_cached_topology();
    let rail_ends = b
        .vertices()
        .ids()
        .filter(|&w| {
            let d = b.vertex(w).unwrap().point.distance(corner);
            (d - 13f64.sqrt()).abs() < 1e-9
        })
        .count();
    assert_eq!(rail_ends, 6, "the rounds end 3 along their edges");
    let cube = create_box(
        &mut k,
        Point3::new(-0.5107059224719217, -0.9420745494701371, 7.0),
        4.0,
        4.0,
        4.0,
    )
    .unwrap();
    let mut cut = boolean_difference(&mut k, b, cube).unwrap();
    cut.refresh_cached_topology();
    let ((faces, _, _), v) = measured(&mut k, &mut cut);
    assert_eq!(faces, 13);
    assert_rel(v, 9_526.705_048_390, 1e-9, "outside-setback");
}

/// The rig's every-block-cylinder-tangent-concave-arc-r2-partial-chain input: a block
/// x 0..40, y -10..10, z 0..15 joined to a cylinder R 10, z 0..30, about the z axis. The
/// block's sides y = +-10 run tangent into the cylinder's wall at x = 0, so its top meets the
/// wall in the concave half circle z = 15, rho = 10, x > 0, split in two at the cylinder's
/// seam. V = 12000 + 3000 pi - 750 pi (block and cylinder less their overlap, a half disc 15
/// high).
fn block_cylinder(k: &mut Kernel) -> Brep {
    let bl = create_box(k, Point3::new(0.0, -10.0, 0.0), 40.0, 20.0, 15.0).unwrap();
    let cy = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 10.0, 30.0).unwrap();
    let mut b = boolean_union(k, bl, cy).unwrap();
    b.refresh_cached_topology();
    b
}

/// Points on the half circle's two pieces: the rig's pick (y < 0) and its mirror, with the
/// end of the half circle each piece reaches, (0, -+10, 15).
const ARC_PIECES: [([f64; 3], [f64; 3]); 2] = [
    (
        [8.867375777689988, -4.60471376596052, 15.0],
        [0.0, -10.0, 15.0],
    ),
    (
        [8.867375777689988, 4.60471376596052, 15.0],
        [0.0, 10.0, 15.0],
    ),
];

/// The block and cylinder are the closed form, and the seam splits the half circle in two:
/// each piece reaches one end of it, not the other.
#[test]
fn block_cylinder_is_the_closed_form() {
    let mut k = solidcraft_kernel();
    let mut b = block_cylinder(&mut k);
    let (_, v) = measured(&mut k, &mut b);
    assert_rel(v, 12_000.0 + 2_250.0 * std::f64::consts::PI, 1e-9, "part");
    let pieces: Vec<EdgeId> = ARC_PIECES.iter().map(|&(p, _)| pick(&k, &b, p)).collect();
    assert_ne!(pieces[0], pieces[1]);
    for (&piece, &(_, end)) in pieces.iter().zip(&ARC_PIECES) {
        let ends: Vec<Point3> = edge::vertices(&b, piece, None)
            .unwrap()
            .into_iter()
            .map(|v| b.vertex(v.unwrap()).unwrap().point)
            .collect();
        let at = |p: [f64; 3]| {
            ends.iter()
                .any(|q| q.distance(Point3::new(p[0], p[1], p[2])) < 1e-9)
        };
        assert!(at(end), "{ends:?}");
        assert!(
            !at([0.0, -end[1], 15.0]),
            "the piece ends at the seam: {ends:?}"
        );
    }
}

/// One piece rounded alone (the rig's pick, then its mirror): the round reaches the seam
/// vertex, where the arc runs on between the same two faces. A seam is the cylinder's
/// parameterisation, not a feature, so the round runs on across it into the rest of the half
/// circle (#2864): the same torus as both pieces picked, V = 19091.962003483477. Without the
/// run-on it was refused there (and before that, capped with #453's construction).
#[test]
fn block_cylinder_arc_piece_runs_on_across_the_seam() {
    let mut k = solidcraft_kernel();
    for &(at_pick, _) in &ARC_PIECES {
        let mut b = block_cylinder(&mut k);
        blend(&mut k, &mut b, Kind::Round, &[at_pick], 2.0).unwrap();
        let (_, v) = measured(&mut k, &mut b);
        assert_rel(v, 19_091.962_003_483_477, 1e-9, "one piece");
    }
}

/// Both pieces rounded r 2: the torus about the z axis (centre circle rho 12, z 17) over the
/// block, cut by its side planes |y| = 10. At radius rho in [10, 12] it adds the spandrel
/// height 2 - sqrt(4 - (rho - 12)^2) over the arc of length 2 rho asin(10 / rho), so
/// V = 12000 + 2250 pi + 23.378532906443 = 19091.962003483477 (the rig case's `method`).
#[test]
fn block_cylinder_both_pieces_round_the_half_circle() {
    let mut k = solidcraft_kernel();
    let mut b = block_cylinder(&mut k);
    let picks = ARC_PIECES.map(|(p, _)| p);
    blend(&mut k, &mut b, Kind::Round, &picks, 2.0).unwrap();
    let (_, v) = measured(&mut k, &mut b);
    assert_rel(v, 19_091.962_003_483_477, 1e-9, "both pieces");
}

/// Both pieces rounded r 2 through the session's `do_fillet` (its tolerance scope and the
/// split at radius corners) with every fix, as the blend's other End tests: the same torus, valid at level
/// 2 and a manifold solid, its volume at 1e-12.
#[test]
fn block_cylinder_both_pieces_round_through_do_fillet() {
    let mut k = Kernel::default();
    for &name in UpstreamFixes::NAMES {
        *k.fixes.flag_mut(name).unwrap() = true;
    }
    let mut b = block_cylinder(&mut k);
    let pieces: Vec<EdgeId> = ARC_PIECES.iter().map(|&(p, _)| pick(&k, &b, p)).collect();
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(
            &mut k,
            &pieces,
            &[2.0; 2],
            &[FilletSurfaceGeneratorType::Circular; 2],
            1,
            1.0,
        )
        .unwrap();
        s.do_fillet(&mut k).unwrap();
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert!(is_manifold_solid(&b).unwrap());
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(&k, &mut b, 1e-12, &attrs).unwrap();
    assert_rel(v, 19_091.962_003_483_477, 1e-9, "do_fillet");
}

/// tangency-labs/solidcore#2679 (the blend corner): fillets/1207 with the r3 round's end
/// arc on the end face (the edge the r1 round runs on into at y = 17) carrying a tangent
/// field (`SmTangentField`) as its curve. The field passes the smooth vertex's tests (its
/// derivative there runs on along the blended edge), but its values are cross tangents, not
/// points, and it gives no second or third derivative: the run-on cannot be decided from it,
/// and the End at the smooth vertex keeps its cap, as before #2657 (9 faces). On #2657 the
/// failed evaluation failed the round (`SM_ERR`). The body is not checked a solid: the arc's
/// curve is a field of vectors, not the arc, so nothing that reads its points can mate it.
/// (A composite curve never gets this far: the smooth vertex's own tangent test, which
/// evaluates the edge's curve, already refuses it.)
#[test]
fn a_tangent_field_edge_at_the_smooth_vertex_keeps_the_cap() {
    use solidcore::curve::curve::CurveKind;
    use solidcore::curve::tangent_field::{TangentDirType, TangentFieldCurve};
    use solidcore::surface::surface::SurfaceKind;
    use solidcore::topology::edgeuse::p1 as eu1;
    let mut k = solidcraft_kernel();
    let mut b = boxed(&mut k, 20.0, 20.0, 20.0);
    blend(&mut k, &mut b, Kind::Round, &[[10.0, 20.0, 20.0]], 3.0).unwrap();
    // the r3 round's end arc on x = 0, from (0, 17, 20) to (0, 20, 17)
    let v = Point3::new(0.0, 17.0, 20.0);
    let arc = *b
        .edge_list()
        .iter()
        .find(|&&e| {
            let ends: Vec<Point3> = edge::vertices(&b, e, None)
                .unwrap()
                .into_iter()
                .map(|w| b.vertex(w.unwrap()).unwrap().point)
                .collect();
            ends.iter().any(|p| p.distance(v) < 1e-9)
                && ends
                    .iter()
                    .any(|p| p.distance(Point3::new(0.0, 20.0, 17.0)) < 1e-9)
        })
        .unwrap();
    let plane = edge::edgeuses(&b, arc, None, None)
        .unwrap()
        .into_iter()
        .filter_map(|eu| eu1::face(&b, eu).unwrap())
        .find_map(|f| {
            let s = b.face(f).unwrap().surface.clone()?;
            matches!(s.kind(), SurfaceKind::Plane(_)).then_some(s)
        })
        .unwrap();
    let ivl = edge::interval(&b, arc).unwrap();
    let at_start = edge::start_vertex(&b, arc).unwrap()
        == b.vertices()
            .ids()
            .find(|&w| b.vertex(w).unwrap().point.distance(v) < 1e-9);
    // a UV line spanning the edge's interval with room past each end, so the edge's
    // ends are inside the field (not its stored end values)
    let (lo, hi) = (ivl.min() - 1.0, ivl.max() + 1.0);
    let uv = Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 2,
            degree: 1,
            control_points: &[Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)],
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[2, 2],
            knots: &[lo, hi],
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        })
        .unwrap(),
    );
    // the field's derivative is along end - start: +y (into the arc from V) at its start,
    // so the edge leaves V the way the arc does
    let y = Vec3::new(0.0, 1.0, 0.0);
    let (start, end) = if at_start { (-y, y) } else { (y, -y) };
    let field =
        TangentFieldCurve::new(&k, uv, plane, TangentDirType::Interpolate, start, end, None)
            .unwrap();
    b.edge_mut(arc).unwrap().curve = Some(Curve::new(CurveKind::TangentField(Box::new(field))));
    let r = blend(&mut k, &mut b, Kind::Round, &[[0.0, 8.5, 20.0]], 1.0);
    assert_eq!(r, Ok(()), "the round builds");
    assert_eq!(b.faces().ids().count(), 9, "capped at the smooth vertex");
}

/// tangency-labs/solidcore#2680's review (the blend corner): the run-on check passes a
/// cancel through, not as an undecided run-on. fillets/1207's vertex (0, 17, 20) after the r3
/// round, the end face's top edge blended there: uncancelled, the edge running on into the
/// r3 round's end arc is no smooth-vertex End (the run-on leaves the part); with the
/// kernel's token cancelled, the check returns the cancel it observes.
#[test]
fn a_cancel_during_the_run_on_check_comes_back() {
    use solidcore::blend::blend_corner::end::end_at_smooth_vertex;
    use solidcore::topology::vertex;
    use solidcore::{CancelToken, SolidError};
    let mut k = solidcraft_kernel();
    let mut b = boxed(&mut k, 20.0, 20.0, 20.0);
    blend(&mut k, &mut b, Kind::Round, &[[10.0, 20.0, 20.0]], 3.0).unwrap();
    let v = b
        .vertices()
        .ids()
        .find(|&w| {
            b.vertex(w)
                .unwrap()
                .point
                .distance(Point3::new(0.0, 17.0, 20.0))
                < 1e-9
        })
        .unwrap();
    let blended = pick(&k, &b, [0.0, 8.5, 20.0]);
    let fil_eu = edge::edgeuses(&b, blended, None, None).unwrap()[0];
    let all_eus: Vec<_> = vertex::edges(&b, v, None)
        .unwrap()
        .into_iter()
        .map(|e| edge::edgeuses(&b, e, None, None).unwrap()[0])
        .collect();
    let smooth = end_at_smooth_vertex(&k, &mut b, v, fil_eu, &all_eus, 0.01, &[]);
    assert!(matches!(smooth, Ok(false)), "{smooth:?}");
    let token = CancelToken::new();
    token.cancel();
    k.set_cancel_token(token);
    let cancelled = end_at_smooth_vertex(&k, &mut b, v, fil_eu, &all_eus, 0.01, &[]);
    assert!(
        matches!(
            cancelled,
            Err(SolidError::Cancelled {
                operation: "blend::blend_corner::end"
            })
        ),
        "{cancelled:?}"
    );
}

/// **tangency-labs/solidcraft#15437 (an End at a smooth vertex at a shallow crease).** A round
/// on one piece of an edge that ends at the vertex where one of its faces changes plane by a few
/// degrees: its edge runs on, nearly in line, into the other piece, which is not rounded. The
/// End caps the round on a plane through the vertex. Square to the edge, that plane holds the
/// face's end (the crease) only while the round's other face is square to it; beside a drafted
/// face the rail ended past the crease, on the face beyond, which the round does not touch, and
/// the trim leaked (status 1001). The round ends where its first rail reaches its face's end:
/// the cap is the plane through the vertex and that rail's crossing with the crease, the one
/// closest to square to the edge. The bodies are built with the kernel's own prism and
/// transform, as SolidCraft's extrude, box and move make them.
mod crease_end {
    use super::{assert_rel, blend, measured, solidcraft_kernel, volume, Kind};
    use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
    use solidcore::eval::Frame;
    use solidcore::math::{Point2, Point3, Vec3};
    use solidcore::primitive::api_primitives::create_box;
    use solidcore::prism::extrude::extrude_region;
    use solidcore::prism::profile::{Loop2, Region2, Seg2};
    use solidcore::topology::brep::mirror::transform_matrix;
    use solidcore::topology::Brep;
    use solidcore::Kernel;
    use std::f64::consts::FRAC_PI_4;

    /// The polygon `pts` (counter-clockwise in `frame`) extruded from level `lo` to `hi`
    /// along the frame's normal.
    fn prism(k: &mut Kernel, frame: &Frame, pts: &[[f64; 2]], lo: f64, hi: f64) -> Brep {
        let p = |q: [f64; 2]| Point2::new(q[0], q[1]);
        let outer = Loop2 {
            segs: pts
                .iter()
                .zip(pts.iter().cycle().skip(1))
                .map(|(&a, &b)| Seg2::Line { a: p(a), b: p(b) })
                .collect(),
        };
        let region = Region2 {
            outer,
            holes: Vec::new(),
        };
        let mut b = extrude_region(k, frame, &region, lo, hi).unwrap();
        b.refresh_cached_topology();
        b
    }

    /// A box at `corner` of `size`, turned `deg` about z (as a row vector, as SolidCraft's move
    /// passes it) and moved to `at`.
    fn turned_box(
        k: &mut Kernel,
        corner: [f64; 3],
        size: [f64; 3],
        deg: f64,
        at: [f64; 3],
    ) -> Brep {
        let mut b = create_box(
            k,
            Point3::new(corner[0], corner[1], corner[2]),
            size[0],
            size[1],
            size[2],
        )
        .unwrap();
        let (c, s) = (deg.to_radians().cos(), deg.to_radians().sin());
        let m = [
            [c, s, 0.0, 0.0],
            [-s, c, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [at[0], at[1], at[2], 1.0],
        ];
        transform_matrix(k, &mut b, &m, true).unwrap();
        b.refresh_cached_topology();
        b
    }

    // #15437's block: a section in y = 0 (x 0..LENGTH, z from the bottom to HEIGHT) extruded
    // DEPTH + 2 along y, the side y = DEPTH cut by a plane turned `draft` degrees about z through
    // the crease vertex (CREASE, DEPTH, 0). The bottom is flat to x = CREASE and rises TILT
    // degrees beyond: the crease is the line x = CREASE, z = 0.
    const LENGTH: f64 = 6.0;
    const CREASE: f64 = 0.8;
    const HEIGHT: f64 = 8.0;
    const DEPTH: f64 = 10.0;
    const TILT: f64 = 3.0;
    const RADIUS: f64 = 2.6;

    fn bottom(x: f64) -> f64 {
        if x <= CREASE {
            0.0
        } else {
            (x - CREASE) * TILT.to_radians().tan()
        }
    }

    fn side(x: f64, draft: f64) -> f64 {
        DEPTH - (x - CREASE) * draft.to_radians().tan()
    }

    fn block(k: &mut Kernel, draft: f64) -> Brep {
        // x along the frame's x, z along its y: its normal is -y, so level h is at y = -h
        let frame = Frame {
            origin: [0.0; 3],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 0.0, 1.0],
        };
        let section = [
            [0.0, 0.0],
            [CREASE, 0.0],
            [LENGTH, bottom(LENGTH)],
            [LENGTH, HEIGHT],
            [0.0, HEIGHT],
        ];
        let b = prism(k, &frame, &section, -(DEPTH + 2.0), 0.0);
        let cut = turned_box(
            k,
            [-20.0, 0.0, -20.0],
            [40.0, 20.0, 40.0],
            -draft,
            [CREASE, DEPTH, 0.0],
        );
        let mut b = boolean_difference(k, b, cut).unwrap();
        b.refresh_cached_topology();
        b
    }

    /// The block's volume: the section's height over x times its depth to the side plane,
    /// integrated (a polynomial in x on each side of the crease).
    fn block_volume(draft: f64) -> f64 {
        let (tt, td) = (TILT.to_radians().tan(), draft.to_radians().tan());
        let w = LENGTH - CREASE;
        let flat = HEIGHT * (DEPTH * CREASE + td * CREASE * CREASE / 2.0);
        let rising = HEIGHT * DEPTH * w - (HEIGHT * td + DEPTH * tt) * w * w / 2.0
            + tt * td * w * w * w / 3.0;
        flat + rising
    }

    /// The volume the round removes: a straight prism of the corner's spandrel (between the
    /// side face, the rising bottom and the ball's circle), cut by the end face x = LENGTH and
    /// by the cap, the plane through the crease vertex that holds the crease (along y) and is
    /// otherwise square to the edge. It is the spandrel's area times the length of its
    /// centroid's line between the two planes. For a corner of angle `phi`, with `d` from the
    /// corner to the tangent points and `a` half the circle's arc, the area is the kite's less
    /// the sector, `d R - a R^2`, and the centroid lies on the bisector at the kite's moment
    /// less the sector's over that area.
    fn capped_prism(draft: f64) -> f64 {
        let (r, tilt, draft) = (RADIUS, TILT.to_radians(), draft.to_radians());
        let vertex = Vec3::new(CREASE, DEPTH, 0.0);
        let into = |a: Vec3, away: Vec3| if a.dot(away) > 0.0 { -a } else { a };
        // the faces' outward normals, the edge, and each face's way into the material across it
        let side = Vec3::new(draft.tan(), 1.0, 0.0).unitize().unwrap();
        let base = Vec3::new(tilt.tan(), 0.0, -1.0).unitize().unwrap();
        let mut t = side.cross(base).unitize().unwrap();
        if t.x < 0.0 {
            t = -t;
        }
        let along_side = into(t.cross(side).unitize().unwrap(), base);
        let along_base = into(t.cross(base).unitize().unwrap(), side);
        let phi = along_side.angle_between(along_base).unwrap();
        let bisector = (along_side + along_base).unitize().unwrap();
        let d = r / (phi / 2.0).tan();
        let to_centre = r / (phi / 2.0).sin();
        let half_arc = (std::f64::consts::PI - phi) / 2.0;
        let area = d * r - half_arc * r * r;
        let kite = d * r * (d * (phi / 2.0).cos() + to_centre) / 3.0;
        let sector = half_arc * r * r * (to_centre - 2.0 * r * half_arc.sin() / (3.0 * half_arc));
        let c = vertex + bisector * ((kite - sector) / area);
        // the cap: through the vertex, its normal the edge's without its component along y
        let cap = Vec3::new(t.x, 0.0, t.z).unitize().unwrap();
        let at_cap = -(c - vertex).dot(cap) / t.dot(cap);
        let at_end = (LENGTH - c.x) / t.x;
        area * (at_end - at_cap)
    }

    /// The long piece of the side face's bottom edge rounded alone: the body's volume.
    fn rounded_block(draft: f64) -> f64 {
        let mut k = solidcraft_kernel();
        let mut b = block(&mut k, draft);
        assert_rel(volume(&k, &mut b), block_volume(draft), 1e-9, "the block");
        let at = [3.0, side(3.0, draft), bottom(3.0)];
        blend(&mut k, &mut b, Kind::Round, &[at], RADIUS)
            .unwrap_or_else(|e| panic!("draft {draft}: refused {e:?}"));
        measured(&mut k, &mut b).1
    }

    /// The side face square to the bottom: the cap square to the edge holds the crease.
    #[test]
    fn a_round_ending_at_a_shallow_crease_is_capped_through_it() {
        let want = block_volume(0.0) - capped_prism(0.0);
        assert_rel(rounded_block(0.0), want, 1e-9, "square side");
    }

    /// The side face drafted 3 degrees: the cap holds the crease, and the round builds.
    #[test]
    fn a_round_ending_at_a_crease_beside_a_drafted_side_is_capped_through_the_crease() {
        let want = block_volume(3.0) - capped_prism(3.0);
        assert_rel(rounded_block(3.0), want, 1e-9, "drafted side");
    }

    // The crease in the other face: a boss 22 long in x and 16 high, its wall y = 10 for
    // x >= PIECE and leaning WALL degrees out (about z) for x < PIECE, so the crease is the
    // upright x = PIECE. An arm 12 x 6 joined against the wall, its top drafted DRAFT degrees
    // (rising with x from z = 8 at x = 0). The arm top meets the wall in the long piece,
    // rounded, and the short piece across the crease, left sharp.
    const WALL: f64 = 4.0;
    const DRAFT: f64 = 2.5;
    const PIECE: f64 = 2.2;
    const ARM_RADIUS: f64 = 0.87;

    fn arm_top(x: f64) -> f64 {
        8.0 + x * DRAFT.to_radians().tan()
    }

    fn boss_and_arm(k: &mut Kernel) -> Brep {
        let t = WALL.to_radians().tan();
        let plan = Frame {
            origin: [0.0; 3],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        };
        let boss = [
            [-10.0, 0.0],
            [12.0, 0.0],
            [12.0, 10.0],
            [PIECE, 10.0],
            [-10.0, 10.0 + (PIECE + 10.0) * t],
        ];
        let boss = prism(k, &plan, &boss, 0.0, 16.0);
        // the arm's section in y = 9 (inside the boss), extruded to y = 16: level h at y = 9 - h
        let side = Frame {
            origin: [0.0, 9.0, 0.0],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 0.0, 1.0],
        };
        let arm = [
            [0.0, 0.0],
            [12.0, 0.0],
            [12.0, arm_top(12.0)],
            [0.0, arm_top(0.0)],
        ];
        let arm = prism(k, &side, &arm, -7.0, 0.0);
        let mut b = boolean_union(k, boss, arm).unwrap();
        b.refresh_cached_topology();
        b
    }

    /// The cap is the plane x = PIECE through the crease. The round reaches the end face
    /// x = 12, and its faces meet square (the top is drafted about y, the wall is upright), so
    /// it adds the corner's spandrel `(1 - pi/4) R^2` times the length between the two planes
    /// along the edge, `(12 - PIECE) / cos(DRAFT)`.
    #[test]
    fn a_round_ending_at_a_wall_crease_under_a_drafted_top_is_capped_through_it() {
        let mut k = solidcraft_kernel();
        let mut b = boss_and_arm(&mut k);
        let before = volume(&k, &mut b);
        blend(
            &mut k,
            &mut b,
            Kind::Round,
            &[[6.0, 10.0, arm_top(6.0)]],
            ARM_RADIUS,
        )
        .unwrap_or_else(|e| panic!("refused {e:?}"));
        let added = measured(&mut k, &mut b).1 - before;
        let r = ARM_RADIUS;
        let want = (1.0 - FRAC_PI_4) * r * r * (12.0 - PIECE) / DRAFT.to_radians().cos();
        assert_rel(added, want, 1e-6, "the round added");
    }

    // A crease skewed in plan beside a tilted side: the round's edge is the x axis, where the
    // bottom z = 0 meets a side through the x axis turned `side` degrees about it (outward
    // normal (0, cos side, -sin side), the material on its -y side), so the faces meet at
    // 90 + `side` degrees. The bottom is flat on the round's side of the crease, the line through
    // the origin along (sin SKEW, -cos SKEW, 0), and rises TILT degrees about it beyond, so the
    // round on the edge's piece x > 0 ends at the origin, where its edge runs on, a few degrees
    // off, into the piece x < 0 left sharp. The block's far end is the plane square to the
    // crease 6 from it.
    //
    // The crease leans SKEW degrees over the round, so the cap holds it: the upright plane
    // through the crease, turned SKEW from square to the edge (the square cap ended the bottom's
    // rail on the rising piece beyond). The cap and the far end are parallel, so the round is
    // its spandrel's prism over the length between them along the edge, 6 / cos SKEW, whatever
    // the spandrel's centroid: `(d R - a R^2) 6 / cos SKEW` for the corner's angle phi,
    // d = R / tan(phi / 2) and a = (pi - phi) / 2. With the side leaning over the round's end
    // (side > 0) the cap's trace on the side runs the side's rail past the vertex, by
    // h tan SKEW sin(side) for the rails' distance h from the edge; the side runs on past the
    // vertex, so that rail still ends on its face.
    const SKEW: f64 = 30.0;
    const SKEW_RADIUS: f64 = 1.0;

    /// A box at `corner` of `size`, its points p moved to p `rows` + `at` (row vectors).
    fn placed_box(
        k: &mut Kernel,
        corner: [f64; 3],
        size: [f64; 3],
        rows: [[f64; 3]; 3],
        at: [f64; 3],
    ) -> Brep {
        let mut b = create_box(
            k,
            Point3::new(corner[0], corner[1], corner[2]),
            size[0],
            size[1],
            size[2],
        )
        .unwrap();
        let [r0, r1, r2] = rows;
        let m = [
            [r0[0], r0[1], r0[2], 0.0],
            [r1[0], r1[1], r1[2], 0.0],
            [r2[0], r2[1], r2[2], 0.0],
            [at[0], at[1], at[2], 1.0],
        ];
        transform_matrix(k, &mut b, &m, true).unwrap();
        b.refresh_cached_topology();
        b
    }

    fn skewed_crease(k: &mut Kernel, side: f64) -> Brep {
        let (sk, tilt) = (SKEW.to_radians(), TILT.to_radians());
        // the section square to the crease, x along (cos SKEW, sin SKEW, 0) (onto the round's
        // side), y along z: its normal is the crease's direction
        let frame = Frame {
            origin: [0.0; 3],
            x_axis: [sk.cos(), sk.sin(), 0.0],
            y_axis: [0.0, 0.0, 1.0],
        };
        let section = [
            [-6.0, 6.0 * tilt.tan()],
            [0.0, 0.0],
            [6.0, 0.0],
            [6.0, 8.0],
            [-6.0, 8.0],
        ];
        let b = prism(k, &frame, &section, -4.0, 12.0);
        // everything on the side's outward side: a box over y > 0 turned `side` about x
        let (c, s) = (side.to_radians().cos(), side.to_radians().sin());
        let cut = placed_box(
            k,
            [-20.0, 0.0, -20.0],
            [40.0, 20.0, 40.0],
            [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]],
            [0.0; 3],
        );
        let mut b = boolean_difference(k, b, cut).unwrap();
        b.refresh_cached_topology();
        b
    }

    /// The round on the skewed crease's block beside a side turned `side`: the volume removed
    /// against the spandrel's prism.
    fn skewed_round_is_the_prism(side: f64) {
        let mut k = solidcraft_kernel();
        let mut b = skewed_crease(&mut k, side);
        let before = volume(&k, &mut b);
        blend(&mut k, &mut b, Kind::Round, &[[3.0, 0.0, 0.0]], SKEW_RADIUS)
            .unwrap_or_else(|e| panic!("side {side}: refused {e:?}"));
        let removed = before - measured(&mut k, &mut b).1;
        let (r, phi) = (SKEW_RADIUS, (90.0 + side).to_radians());
        let d = r / (phi / 2.0).tan();
        let a = (std::f64::consts::PI - phi) / 2.0;
        let want = (d * r - a * r * r) * 6.0 / SKEW.to_radians().cos();
        assert_rel(removed, want, 1e-7, "the round removed");
    }

    /// The side leaning over the round's end: the cap through the crease, turned 30 degrees
    /// from square, runs the side's rail past the vertex, onto the side where it runs on; the
    /// round is the prism.
    #[test]
    fn a_cap_turned_through_a_skewed_crease_beside_a_leaning_side_is_the_prism() {
        skewed_round_is_the_prism(15.0);
    }

    /// The side leaning away: the side's rail ends short of the vertex; the round is the prism.
    #[test]
    fn a_cap_turned_through_a_skewed_crease_beside_a_side_leaning_away_is_the_prism() {
        skewed_round_is_the_prism(-15.0);
    }
}

/// Ladder 021's golden checked against its own convergence: the r2 chain built with its
/// edges stored at 4e-6, 2e-6 and 1e-6 (so the round is fitted at 4 times each) gives
/// volumes that never fall as the edges tighten and agree to 1e-5, and the golden (its edges
/// as the earlier rounds left them) lies within 1e-4 of the finest. The golden before a round was
/// fitted as finely as its edge is known, 47589.770467, lies 5.8e-4 below it.
#[test]
fn ladder_021_volume_converges_as_its_edges_tighten() {
    let chain = [
        [60.0, 40.0, 7.5],
        [60.0, 39.791287847477925, 16.0],
        [58.931891825700944, 37.559849485548405, 19.29401699439179],
    ];
    let v: Vec<f64> = [4e-6, 2e-6, 1e-6]
        .iter()
        .map(|&t| {
            let mut k = solidcraft_kernel();
            let mut b = boxed(&mut k, 60.0, 40.0, 20.0);
            blend(&mut k, &mut b, Kind::Round, &[[30.0, 40.0, 20.0]], 5.0).unwrap();
            blend(&mut k, &mut b, Kind::Round, &[[60.0, 20.0, 20.0]], 3.0).unwrap();
            for &p in &chain {
                let e = pick(&k, &b, p);
                edge::set_tolerance(&mut b, e, t, false, false).unwrap();
            }
            blend(&mut k, &mut b, Kind::Round, &chain, 2.0).unwrap();
            let (counts, v) = measured(&mut k, &mut b);
            assert_eq!(counts, (11, 24, 15), "edges at {t:e}");
            v
        })
        .collect();
    assert!(
        v[0] <= v[1] && v[1] <= v[2],
        "falls as the edges tighten: {v:?}"
    );
    assert!(v[2] - v[0] < 1e-5, "the tight builds disagree: {v:?}");
    let golden = 47_589.770_999;
    assert!(
        (v[2] - golden).abs() < 1e-4,
        "golden {golden} vs finest {}",
        v[2]
    );
    assert!(
        (v[2] - 47_589.770_467).abs() > 5e-4,
        "the old golden is as near: {v:?}"
    );
}
