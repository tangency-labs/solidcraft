//! The SolidCraft fork's tangent-end blends (tangency-labs/smlib@9bebf34, the first commit
//! of smlib#76; #2243), and its a87c502 under
//! `fixes.fillet_extended_rail_end_at_side_edge_crossing` (WR-681): a rail's end on its side
//! edge is the solver's rail/edge crossing, not where the end curve leaves the fillet.
//!
//! The part is the fork's tangent peg (capi `tangent_peg`): a bar 40 x 16 x 10 whose far end
//! is a half disc R 8 about (40, 8), with a disc R 8 about the same centre standing 10 on its
//! top, joined. The bar's front face y = 0 runs tangent into the cylinder along x = 40, so
//! its front top edge (convex) and the concave arc where its top meets the cylinder are
//! tangent where they meet, at V = (40, 0, 10), and a blend of either ends on a face tangent
//! to the face it rolls on. V0 = 6400 + 960 pi. Every blend lies within the unblended body's
//! box (0, 0, 0) .. (48, 16, 20), to the blends' tolerance 4e-5. Drafted 5 degrees inward,
//! the front face runs tangent into a cone instead.
//!
//! [`TANGENT_END_CASES`] holds the fork's thirteen cases with its reason for each that fails
//! at a87c502 (`TANGENT_END_CASES` in capi `tests/ops_kernel.cpp`, after a87c502 dropped
//! `front_chamfer_d2`'s), kept as they are. Each case checks, as the fork's
//! `peg_blend`/`peg_result`/`done` do, the blend's success, SMLib's check at level 2, the box
//! (where the case has one) and the volume to 1e-6 relative of the fork's quadrature (where
//! it has one, at volume accuracy 1e-7). Where solidcore's outcome differs from the fork's
//! mark, the case says so ([`TangentEndCase::here`]).
//!
//! The kernel is the fork's at a87c502 as far as solidcore models it: every fix SolidCraft
//! turns on (all but `geometric_g2_continuity`, as `issue_2174_cap_rim_two_edges.rs`)
//! except WR-600's `fillet_nx1_tangent_side_edge_same_convexity`, whose tangent end of the
//! trace is the fork's later 6d73e4f/0cc48bd/b34d61b (under it a tangent end is a rail/edge
//! crossing of the trace and never reaches `CalcRailIntExtendedEdgeuse`).
//!
//! Four of the cases have tests already, under WR-600 on (the fork's later state):
//! `fork_856b54e_tangent_end.rs` (front chamfer, concave chamfer, front round) and
//! `fork_cd47efd_tangent_end_rib_sqrt_s.rs` (the curvature round). They stay in the table
//! because the marks are the fork's at a87c502, a different profile. The fork's twins on
//! its `smlib-tangent-ends` branch, c5ea180 and 169adea, have the same diffs as 9bebf34
//! and a87c502.
//!
//! Differences from the fork's capi, all in how the test drives the kernel: blends go
//! through [`FilletSession`] per edge (constant radius; `Linear` for the chamfer, without
//! the capi's bevelled corners; `BlendCurve` G2 at thirds for the curvature round), edges
//! are picked as the nearest within 0.1 of each point; the drafted peg is a taper extrude
//! with `LinearExtension` corners, because `CurveExtension` (the fork's
//! `SMLIB_DRAFT_CORNER_CURVATURE`) refuses the half disc's 180-degree wall here (`NotExact`
//! at (40, 0, 0)).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3, BIG_DOUBLE};
use solidcore::primitive::api_curves as curves;
use solidcore::query::api_queries::{brep_bounding_box, get_edges};
use solidcore::sweep::api_brep::{
    create_linear_sweep, create_taper_extrude_with_corner, OffsetCornerType, TaperExtrude,
};
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

const PI: f64 = core::f64::consts::PI;

/// The fork's profile at a87c502 (module doc), with or without its crossing.
fn kernel(crossing: bool) -> Kernel {
    let mut k = solidcraft_kernel();
    k.fixes.fillet_nx1_tangent_side_edge_same_convexity = false;
    k.fixes.fillet_extended_rail_end_at_side_edge_crossing = crossing;
    k
}

/// Every fix SolidCraft turns on: all but `geometric_g2_continuity`.
fn solidcraft_kernel() -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
}

/// Extruded 10 up, straight or drafted `draft_deg` inward.
fn extrude(k: &mut Kernel, cs: &[Curve], draft_deg: f64) -> Brep {
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    if draft_deg == 0.0 {
        return create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, true).unwrap();
    }
    let corner = OffsetCornerType::LinearExtension;
    match create_taper_extrude_with_corner(k, &refs, 10.0, draft_deg, 3, corner).unwrap() {
        TaperExtrude::Solid(b) => b,
        TaperExtrude::Refused(r) => panic!("draft refused: {r:?}"),
    }
}

/// The fork's `tangent_peg(draft)`.
fn tangent_peg(k: &mut Kernel, draft_deg: f64) -> Brep {
    let p = Point3::new;
    let d = 10.0 * draft_deg.to_radians().tan();
    let line = |a: Point3, b: Point3| Curve::from(curves::create_line_segment(a, b).unwrap());
    let bar = [
        line(p(0.0, 0.0, 0.0), p(40.0, 0.0, 0.0)),
        Curve::from(curves::create_arc(p(40.0, 8.0, 0.0), 8.0, -90.0, 90.0).unwrap()),
        line(p(40.0, 16.0, 0.0), p(0.0, 16.0, 0.0)),
        line(p(0.0, 16.0, 0.0), p(0.0, 0.0, 0.0)),
    ];
    let bar = extrude(k, &bar, draft_deg);
    let disc = [Curve::from(
        curves::create_circle(p(40.0, 8.0, 10.0), 8.0 - d).unwrap(),
    )];
    let disc = extrude(k, &disc, draft_deg);
    boolean_union(k, bar, disc).unwrap()
}

/// What a blend makes of each picked edge (the fork's `SMLIB_BLEND_*`).
#[derive(Clone, Copy)]
enum Kind {
    Round,
    Curvature,
    Chamfer,
}

/// The edge nearest `at`, within the fork's pick tolerance 0.1.
fn pick(k: &Kernel, b: &Brep, at: [f64; 3]) -> Option<EdgeId> {
    let p = Point3::new(at[0], at[1], at[2]);
    let mut best: Option<(EdgeId, f64)> = None;
    for e in get_edges(b).unwrap() {
        let Some(c) = edge::curve(b, e).unwrap() else {
            continue;
        };
        let ivl = edge::interval(b, e).unwrap();
        let d = c
            .drop_point(
                k,
                &ivl,
                p,
                None,
                BIG_DOUBLE,
                None,
                SolverOperation::Minimize,
            )
            .unwrap();
        if d.success && d.distance < 0.1 && best.is_none_or(|(_, bd)| d.distance < bd) {
            best = Some((e, d.distance));
        }
    }
    best.map(|(e, _)| e)
}

/// The fork's `peg_blend`: the edges at `points` blended as `kind`, each at its point's
/// size (an edge reached from several points takes the first point's), then stitched.
fn blend(
    k: &mut Kernel,
    b: &mut Brep,
    kind: Kind,
    points: &[[f64; 3]],
    sizes: &[f64],
) -> Result<(), String> {
    let mut edges = Vec::new();
    let mut radii = Vec::new();
    for (&at, &size) in points.iter().zip(sizes) {
        let e = pick(k, b, at).ok_or_else(|| format!("no edge at {at:?}"))?;
        if !edges.contains(&e) {
            edges.push(e);
            radii.push(size);
        }
    }
    let (xsect, continuity) = match kind {
        Kind::Round => (FilletSurfaceGeneratorType::Circular, 1),
        Kind::Curvature => (FilletSurfaceGeneratorType::BlendCurve, 2),
        Kind::Chamfer => (FilletSurfaceGeneratorType::Linear, 1),
    };
    let xsects = vec![xsect; edges.len()];
    {
        let mut s = FilletSession::new(k, b).map_err(|e| e.to_string())?;
        s.set_blend_curve_at_thirds(matches!(kind, Kind::Curvature));
        s.prepare_per_edge_constant_radius(k, &edges, &radii, &xsects, continuity, 1.0)
            .and_then(|()| s.do_fillet(k))
            .map_err(|e| format!("status: {e}"))?;
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).map_err(|e| e.to_string())
}

/// The volume at the fork's `volume_of` accuracy, 1e-7.
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

// The fork's quadratures of the rig's expected answers (tanh-sinh, converged to 1e-13).
const PEG_V0: f64 = 6400.0 + 960.0 * PI;
const PEG_FRONT_ROUND_R2: f64 = 32.306_475_043_873_16;
const PEG_FRONT_CURVATURE_R2: f64 = 22.621_011_133_536_857;
const PEG_FRONT_CHAMFER_D2: f64 = 74.130_570_575_311_6;
const PEG_CONCAVE_CHAMFER_D2: f64 = 41.656_784_248_897_58;

const PEG_FRONT: [f64; 3] = [20.0, 0.0, 10.0];
const PEG_CONCAVE: [f64; 3] = [32.0, 8.0, 10.0];

/// After the front round `r`: a point on its end on the cylinder (the round's section at
/// 45 degrees), on the edge the concave blend's chain runs on into.
fn peg_front_end(r: f64) -> [f64; 3] {
    let c = 0.5_f64.sqrt();
    let s = r * (1.0 - c);
    [40.0 - (s * (16.0 - s)).sqrt(), s, 10.0 - r + r * c]
}

/// After the concave round `r`: a point on its end on the front face (the torus's section
/// at 45 degrees).
fn peg_concave_end(r: f64) -> [f64; 3] {
    let c = 0.5_f64.sqrt();
    let rho = 8.0 + r * (1.0 - c);
    [40.0 - (rho * rho - 64.0).sqrt(), 0.0, 10.0 + r - r * c]
}

/// A case's outcome: what failed (empty: it passes) and its volume where it has one.
#[derive(Debug, Default)]
struct Outcome {
    failed: Vec<String>,
    volume: Option<f64>,
}

/// The fork's `peg_result` and `done`: within the unblended box (`boxed`), of volume
/// `want` when it is known (> 0), and sound.
fn peg_result(k: &mut Kernel, b: &mut Brep, want: f64, boxed: bool, out: &mut Outcome) {
    if boxed {
        let bx = brep_bounding_box(k, b, true).unwrap();
        let (lo, hi) = (bx.min(), bx.max());
        let inside = [(lo.x, hi.x, 48.0), (lo.y, hi.y, 16.0), (lo.z, hi.z, 20.0)]
            .into_iter()
            .all(|(l, h, top)| l >= -4e-5 && h <= top + 4e-5);
        if !inside {
            out.failed.push(format!("box {lo:?} .. {hi:?}"));
        }
    }
    if want > 0.0 {
        let v = volume(k, b);
        out.volume = Some(v);
        if (v - want).abs() > 1e-6 * want.abs() {
            out.failed.push(format!("volume {v:.9}, want {want:.9}"));
        }
    }
    let lines = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
    if !lines.is_empty() {
        out.failed.push(format!("check: {lines:?}"));
    }
}

/// One blend of the peg (drafted `draft_deg`), checked.
fn one(
    k: &mut Kernel,
    draft_deg: f64,
    kind: Kind,
    points: &[[f64; 3]],
    sizes: &[f64],
    want: f64,
    boxed: bool,
) -> Outcome {
    let mut out = Outcome::default();
    let mut b = tangent_peg(k, draft_deg);
    match blend(k, &mut b, kind, points, sizes) {
        Ok(()) => peg_result(k, &mut b, want, boxed, &mut out),
        Err(e) => out.failed.push(format!("blend: {e}")),
    }
    out
}

/// The fork's `peg_rounds_in_turn`: one round, then the other on into its end.
fn rounds_in_turn(k: &mut Kernel, front: f64, concave: f64, front_first: bool) -> Outcome {
    let mut out = Outcome::default();
    let mut b = tangent_peg(k, 0.0);
    let r = if front_first {
        blend(k, &mut b, Kind::Round, &[PEG_FRONT], &[front]).and_then(|()| {
            let pts = [PEG_CONCAVE, peg_front_end(front)];
            blend(k, &mut b, Kind::Round, &pts, &[concave, concave])
        })
    } else {
        blend(k, &mut b, Kind::Round, &[PEG_CONCAVE], &[concave]).and_then(|()| {
            let pts = [PEG_FRONT, peg_concave_end(concave)];
            blend(k, &mut b, Kind::Round, &pts, &[front, front])
        })
    };
    match r {
        Ok(()) => peg_result(k, &mut b, 0.0, true, &mut out),
        Err(e) => out.failed.push(format!("blend: {e}")),
    }
    out
}

fn chamfers_together_d2(k: &mut Kernel) -> Outcome {
    one(
        k,
        0.0,
        Kind::Chamfer,
        &[PEG_FRONT, PEG_CONCAVE],
        &[2.0, 2.0],
        0.0,
        true,
    )
}
fn concave_chamfer_d2(k: &mut Kernel) -> Outcome {
    let want = PEG_V0 + PEG_CONCAVE_CHAMFER_D2;
    one(k, 0.0, Kind::Chamfer, &[PEG_CONCAVE], &[2.0], want, true)
}
fn concave_round_r3_then_front_round_r1_5(k: &mut Kernel) -> Outcome {
    rounds_in_turn(k, 1.5, 3.0, false)
}
fn concave_round_then_front_round_r2(k: &mut Kernel) -> Outcome {
    rounds_in_turn(k, 2.0, 2.0, false)
}
fn drafted_front_round_r2(k: &mut Kernel) -> Outcome {
    let d = 10.0 * 5.0_f64.to_radians().tan();
    one(k, 5.0, Kind::Round, &[[20.0, d, 10.0]], &[2.0], 0.0, false)
}
fn drafted_rounds_together_r2(k: &mut Kernel) -> Outcome {
    let d = 10.0 * 5.0_f64.to_radians().tan();
    let pts = [[20.0, d, 10.0], [32.0 + d, 8.0, 10.0]];
    one(k, 5.0, Kind::Round, &pts, &[2.0, 2.0], 0.0, false)
}
fn front_chamfer_d2(k: &mut Kernel) -> Outcome {
    let want = PEG_V0 - PEG_FRONT_CHAMFER_D2;
    one(k, 0.0, Kind::Chamfer, &[PEG_FRONT], &[2.0], want, true)
}
fn front_curvature_round_r2(k: &mut Kernel) -> Outcome {
    let want = PEG_V0 - PEG_FRONT_CURVATURE_R2;
    one(k, 0.0, Kind::Curvature, &[PEG_FRONT], &[2.0], want, true)
}
fn front_round_r1_5_then_concave_round_r3(k: &mut Kernel) -> Outcome {
    rounds_in_turn(k, 1.5, 3.0, true)
}
fn front_round_r2(k: &mut Kernel) -> Outcome {
    let want = PEG_V0 - PEG_FRONT_ROUND_R2;
    one(k, 0.0, Kind::Round, &[PEG_FRONT], &[2.0], want, true)
}
fn front_round_then_concave_round_r2(k: &mut Kernel) -> Outcome {
    rounds_in_turn(k, 2.0, 2.0, true)
}
fn rounds_together_r2(k: &mut Kernel) -> Outcome {
    one(
        k,
        0.0,
        Kind::Round,
        &[PEG_FRONT, PEG_CONCAVE],
        &[2.0, 2.0],
        0.0,
        true,
    )
}
fn rounds_together_r5(k: &mut Kernel) -> Outcome {
    one(
        k,
        0.0,
        Kind::Round,
        &[PEG_FRONT, PEG_CONCAVE],
        &[5.0, 5.0],
        0.0,
        true,
    )
}

/// How solidcore's outcome of a case differs from the fork's mark.
enum Here {
    /// The fork marks it failing; it passes here.
    Passes(&'static str),
    /// Both fail, for a different reason or by a different amount here.
    FailsOtherwise(&'static str),
}

/// A tangent-end case: the fork's name and reason it fails at a87c502 (`None`: it passes
/// there), and how solidcore's outcome differs from that, if it does.
struct TangentEndCase {
    name: &'static str,
    run: fn(&mut Kernel) -> Outcome,
    fails_today: Option<&'static str>,
    here: Option<Here>,
}

impl TangentEndCase {
    /// Whether the case fails here.
    fn fails_here(&self) -> bool {
        match self.here {
            Some(Here::Passes(_)) => false,
            Some(Here::FailsOtherwise(_)) => true,
            None => self.fails_today.is_some(),
        }
    }
}

const TANGENT_END_CASES: [TangentEndCase; 13] = [
    TangentEndCase {
        name: "chamfers_together_d2",
        run: chamfers_together_d2,
        fails_today: Some("status 1001: SmFilletNx1Corner, Corner Edge Computation Failure (the chamfers' tangent ends, as front_chamfer_d2 and concave_chamfer_d2)"),
        here: None,
    },
    TangentEndCase {
        name: "concave_chamfer_d2",
        run: concave_chamfer_d2,
        fails_today: Some("status 1001: SmFilletNx1Corner, Corner Edge Computation Failure, where the cone runs out over the front top edge and its end on the front face leaves its contact line tangentially"),
        here: None,
    },
    TangentEndCase {
        name: "concave_round_r3_then_front_round_r1_5",
        run: concave_round_r3_then_front_round_r1_5,
        fails_today: None,
        here: None,
    },
    TangentEndCase {
        name: "concave_round_then_front_round_r2",
        run: concave_round_then_front_round_r2,
        fails_today: None,
        here: None,
    },
    TangentEndCase {
        name: "drafted_front_round_r2",
        run: drafted_front_round_r2,
        fails_today: Some("SMLib's check refuses the round: Test #13 (a 2D curve with a radius of curvature of 8.6e-6) and Tests #7 and #8 (kinks at interior knots) on its end on the cone"),
        here: Some(Here::FailsOtherwise("on the LinearExtension draft, SMLib's check refuses the round with Tests #8, #5 and #11 (a kink and zero or slow speeds at interior knots)")),
    },
    TangentEndCase {
        name: "drafted_rounds_together_r2",
        run: drafted_rounds_together_r2,
        fails_today: Some("status 1001 before any geometry: the corner where the front top edge runs tangent into the concave arc, on the cone, is of no kind SMLib's blend corners take"),
        here: None,
    },
    TangentEndCase {
        name: "front_chamfer_d2",
        run: front_chamfer_d2,
        fails_today: None,
        here: None,
    },
    TangentEndCase {
        name: "front_curvature_round_r2",
        run: front_curvature_round_r2,
        fails_today: None,
        here: None,
    },
    TangentEndCase {
        name: "front_round_r1_5_then_concave_round_r3",
        run: front_round_r1_5_then_concave_round_r3,
        fails_today: None,
        here: None,
    },
    TangentEndCase {
        name: "front_round_r2",
        run: front_round_r2,
        fails_today: None,
        here: None,
    },
    TangentEndCase {
        name: "front_round_then_concave_round_r2",
        run: front_round_then_concave_round_r2,
        fails_today: None,
        here: None,
    },
    TangentEndCase {
        name: "rounds_together_r2",
        run: rounds_together_r2,
        fails_today: Some("the corner patch where the rounds meet stands 0.0374 out of the front face (box y min -0.0374099)"),
        here: Some(Here::Passes("the rounds together build within the box and check clean")),
    },
    TangentEndCase {
        name: "rounds_together_r5",
        run: rounds_together_r5,
        fails_today: Some("the corner patch where the rounds meet stands 0.128 out of the front face (box y min -0.1278205)"),
        here: Some(Here::FailsOtherwise("the corner patch stands 0.00156 out of the front face (box y min -0.0015609)")),
    },
];

/// Runs the cases that fail here (`known_failing`) or the others; the names of those whose
/// outcome is not the one listed.
fn tangent_end_cases(known_failing: bool) -> Vec<String> {
    let mut wrong = Vec::new();
    for c in TANGENT_END_CASES
        .iter()
        .filter(|c| c.fails_here() == known_failing)
    {
        let mut k = kernel(true);
        let out = (c.run)(&mut k);
        let failed = !out.failed.is_empty();
        if failed != known_failing {
            wrong.push(format!("{}: {:?}", c.name, out.failed));
        }
    }
    wrong
}

#[test]
fn tangent_end_cases_pass() {
    let wrong = tangent_end_cases(false);
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn tangent_end_cases_known_failing() {
    let wrong = tangent_end_cases(true);
    assert!(
        wrong.is_empty(),
        "passes, though listed as failing: {wrong:#?}"
    );
}

/// The fork's marks are kept as they are: solidcore's outcome is the fork's except where a
/// case says how it differs, and every passing case of the fork passes here.
#[test]
fn tangent_end_marks_are_the_fork_s() {
    for c in &TANGENT_END_CASES {
        let differs = c.fails_here() != c.fails_today.is_some();
        match &c.here {
            None => assert!(!differs, "{}", c.name),
            Some(Here::Passes(why)) => assert!(c.fails_today.is_some() && !why.is_empty()),
            Some(Here::FailsOtherwise(why)) => {
                assert!(c.fails_today.is_some() && !why.is_empty(), "{}", c.name);
            }
        }
    }
}

/// a87c502: the front chamfer's rail end on the cylinder is the rail/edge crossing. The
/// fork: "`front_chamfer_d2` passes, volume 9341.798354 against the quadrature's 9341.798377
/// (2.4e-9)"; at 9bebf34 it failed with status 1001.
#[test]
fn front_chamfer_ends_at_the_rail_edge_crossing() {
    let mut k = kernel(false);
    let before = front_chamfer_d2(&mut k);
    assert_eq!(before.failed.len(), 1, "{:?}", before.failed);
    assert!(before.failed[0].contains("1001"), "{:?}", before.failed);

    let mut k = kernel(true);
    let after = front_chamfer_d2(&mut k);
    assert!(after.failed.is_empty(), "{:?}", after.failed);
    let v = after.volume.unwrap();
    let quadrature = PEG_V0 - PEG_FRONT_CHAMFER_D2;
    // 9341.798353087 here, 1.0e-10 relative below the fork's 9341.798354 and 2.5e-9 below
    // the quadrature's 9341.798377
    assert!((v - 9341.798354).abs() < 2e-6, "V {v}");
    assert!(
        (v - quadrature).abs() < 3e-9 * quadrature,
        "V {v} vs {quadrature}"
    );
}

/// a87c502: "The round cases keep their results". A round's rail end at the tangent seam
/// is where the round touches the cylinder, so the crossing changes nothing there: the same
/// volumes to the last bit, and the same outcomes for the rounds in turn.
#[test]
fn round_cases_keep_their_results() {
    let volume_cases: [fn(&mut Kernel) -> Outcome; 2] = [front_round_r2, front_curvature_round_r2];
    for case in volume_cases {
        let (off, on) = (case(&mut kernel(false)), case(&mut kernel(true)));
        assert!(
            off.failed.is_empty() && on.failed.is_empty(),
            "{off:?} {on:?}"
        );
        assert_eq!(off.volume.unwrap().to_bits(), on.volume.unwrap().to_bits());
    }
    let in_turn: [fn(&mut Kernel) -> Outcome; 2] = [
        front_round_then_concave_round_r2,
        concave_round_then_front_round_r2,
    ];
    for case in in_turn {
        let (off, on) = (case(&mut kernel(false)), case(&mut kernel(true)));
        assert!(
            off.failed.is_empty() && on.failed.is_empty(),
            "{off:?} {on:?}"
        );
    }
}

/// With WR-600's tangent end of the trace on too (SolidCraft's profile, the fork's later
/// commits), a tangent end is a rail/edge crossing of the trace and never reaches
/// `CalcRailIntExtendedEdgeuse`: the crossing changes none of the front blends.
#[test]
fn with_the_tangent_end_of_the_trace_the_crossing_changes_nothing() {
    let cases: [fn(&mut Kernel) -> Outcome; 3] =
        [front_chamfer_d2, front_round_r2, front_curvature_round_r2];
    for case in cases {
        let mut off = solidcraft_kernel();
        off.fixes.fillet_extended_rail_end_at_side_edge_crossing = false;
        let (off, on) = (case(&mut off), case(&mut solidcraft_kernel()));
        assert!(
            off.failed.is_empty() && on.failed.is_empty(),
            "{off:?} {on:?}"
        );
        assert_eq!(off.volume.unwrap().to_bits(), on.volume.unwrap().to_bits());
    }
}
