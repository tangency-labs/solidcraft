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

use solidcore::blend::blend_corner::end::RUNS_ON_ACROSS_SEAM;
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

/// SolidCraft's fixes at the 5d2635d36 pin: the corpus's `fixes.json` and the eight the bump
/// turns on (tangency-labs/solidcraft#11875).
fn solidcraft_kernel() -> Kernel {
    let names: Vec<String> = serde_json::from_str(include_str!("kernel_cases/fixes.json")).unwrap();
    let mut f = UpstreamFixes::default();
    for name in names.iter().map(String::as_str).chain([
        "offset_extends_ball_patches_as_themselves",
        "approximate_surface_checks_every_row",
        "topological_vertex_has_only_edge_uses",
        "planar_section_face_tolerance",
        "face_swap_uv_transposes_trims",
        "closed_smooth_pole_tangent_limit",
        "fillet_setback_beside_curved_faces",
        "fillet_chamfer_corner",
    ]) {
        *f.flag_mut(name).unwrap_or_else(|| panic!("flag {name}")) = true;
    }
    Kernel {
        fixes: f,
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
/// SolidCraft's golden (8ddce9016): 24 edges, 15 vertices, 47589.770467. Capped (#2364):
/// 25 edges, 16 vertices, 47591.482.
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
    assert_rel(v, 47_589.770_467, 1e-8, "021");
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
/// off). Capped (#2364): 10 and 15 faces.
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
    assert_rel(v, 10_198.718_249, 1e-9, "1080");
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

/// The rig's every-block-cylinder-tangent-concave-arc-r2-partial-chain input
/// (block 40 x 20 x 15 joined to the cylinder R10 x 30): the concave
/// arc where the block's top meets the cylinder is split at the cylinder's seam, and the
/// pick (the rig's point) takes one piece. The round of that piece would end at the seam
/// vertex, where the arc runs on between the same two faces: a round does not end at a seam
/// (#2864), so the blend refuses it instead of capping it there with #453's construction.
#[test]
fn block_cylinder_arc_piece_is_refused_at_the_seam() {
    let mut k = solidcraft_kernel();
    let bl = create_box(&mut k, Point3::new(0.0, -10.0, 0.0), 40.0, 20.0, 15.0).unwrap();
    let cy = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 30.0).unwrap();
    let mut b = boolean_union(&mut k, bl, cy).unwrap();
    b.refresh_cached_topology();
    let at_pick = [8.867375777689988, -4.60471376596052, 15.0];
    let piece = pick(&k, &b, at_pick);
    let ends: Vec<Point3> = edge::vertices(&b, piece, None)
        .unwrap()
        .into_iter()
        .map(|v| b.vertex(v.unwrap()).unwrap().point)
        .collect();
    let at = |p: Point3| ends.iter().any(|q| q.distance(p) < 1e-9);
    assert!(at(Point3::new(0.0, -10.0, 15.0)), "{ends:?}");
    assert!(
        !at(Point3::new(0.0, 10.0, 15.0)),
        "the piece ends at the seam: {ends:?}"
    );
    let r = blend(&mut k, &mut b, Kind::Round, &[at_pick], 2.0);
    assert_eq!(r.unwrap_err().1, RUNS_ON_ACROSS_SEAM, "not capped");
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
