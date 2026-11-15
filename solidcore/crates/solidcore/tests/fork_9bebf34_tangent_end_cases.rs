//! The SolidCraft fork's tangent-end blends (tangency-labs/smlib@9bebf34, the first commit
//! of smlib#76; #2243), with the fork's marks at the end of smlib#76 (cd47efd).
//! Its a87c502 (WR-681: a rail's end on its side edge taken as the solver's rail/edge
//! crossing) was taken back out by 0cc48bd, and so was its flag here: at cd47efd
//! `CalcRailIntExtendedEdgeuse` is the pinned routine again.
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
//! [`TANGENT_END_CASES`] holds twelve of the fork's thirteen cases with its reason for each that fails
//! (`TANGENT_END_CASES` in capi `tests/ops_kernel.cpp`) at cd47efd, kept as they are. cd47efd's are 6d73e4f's: `concave_chamfer_d2`, `drafted_front_round_r2` and
//! `front_chamfer_d2` pass, and `chamfers_together_d2` fails at the 3x2 mixed corner where the
//! chamfers meet; 0cc48bd and b34d61b change no mark. Each case checks, as the fork's
//! `peg_blend`/`peg_result`/`done` do, the blend's success, SMLib's check at level 2, the box
//! (where the case has one) and the volume to 1e-6 relative of the fork's quadrature (where
//! it has one, at volume accuracy 1e-7). Where solidcore's outcome differs from the fork's
//! mark, the case says so ([`Mark::here`]).
//!
//! The marks run under [`solidcraft_kernel`], the fork's profile at cd47efd as far as solidcore
//! models it: every fix SolidCraft turns on but `geometric_g2_continuity` (as
//! `issue_2174_cap_rim_two_edges.rs`), with WR-710's refusal, not the fork's at cd47efd. The G^n
//! section and the generic blend's regularity and fit checks (WR-685, WR-696; the fork's
//! later 11ea2d9 and smlib#79) are on, as solidcore builds them always. At 9bebf34 the fork had neither the
//! tangent end of the trace (its later 6d73e4f/0cc48bd/b34d61b, WR-600) nor the blend corner
//! (its later smlib#79 step 4); solidcore builds both always (#2400), so 9bebf34's marks are
//! not run. At 5e8c9c1, with both, the fork's marks of the thirteen cases are cd47efd's, and
//! solidcore's are too.
//!
//! The thirteenth, `front_curvature_round_r2` (the front top edge rounded 2 by the G2 blend
//! curve at thirds, the fork's `SetBlendCurveAtThirds`), is not run: solidcore's
//! curvature-continuous round is the G^n section (WR-685; the blend curve and its at-thirds
//! option went with `fillet_gn_section_generator`, WR-685 addendum 2026-11-06), the fork's
//! later 11ea2d9. That round on the same edge is graded against its own quadrature in
//! `fork_cd47efd_tangent_end_rib_sqrt_s.rs`.
//!
//! Three of the cases have tests already: `fork_856b54e_tangent_end.rs` (front chamfer,
//! concave chamfer, front round, against closed forms). They stay in the table for the
//! fork's marks. The fork's twins on its `smlib-tangent-ends` branch,
//! c5ea180 and 169adea, have the same diffs as 9bebf34 and a87c502, and on its `replay/1.0.1`
//! 6d73e4f, 0cc48bd and b34d61b are 856b54e, 4ee7d3a and cd47efd (the same trees).
//!
//! Differences from the fork's capi, all in how the test drives the kernel: blends go
//! through [`FilletSession`] per edge (constant radius; `Linear` for the chamfer, without
//! the capi's bevelled corners), edges
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

/// Every fix SolidCraft turns on: all but `geometric_g2_continuity`. The fork's profile at
/// cd47efd, but for WR-710's refusal (not the fork's: it refuses the rounds together where
/// no Mixed corner takes the corner), which solidcore always makes.
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
        Kind::Chamfer => (FilletSurfaceGeneratorType::Linear, 1),
    };
    let xsects = vec![xsect; edges.len()];
    {
        let mut s = FilletSession::new(k, b).map_err(|e| e.to_string())?;
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
}

/// The fork's mark of a case at cd47efd: its reason the case fails there (`None`: it
/// passes), and how solidcore's outcome differs, if it does.
struct Mark {
    fails_today: Option<&'static str>,
    here: Option<Here>,
}

impl Mark {
    /// Whether the case fails here.
    fn fails_here(&self) -> bool {
        match self.here {
            Some(Here::Passes(_)) => false,
            None => self.fails_today.is_some(),
        }
    }
}

/// A tangent-end case: the fork's name and its mark at cd47efd.
struct TangentEndCase {
    name: &'static str,
    run: fn(&mut Kernel) -> Outcome,
    at_cd47efd: Mark,
}

/// A mark of a case that passes.
const PASSES: Mark = Mark {
    fails_today: None,
    here: None,
};

const CHAMFERS_AT_CD47EFD: &str = "status 1001: SmFillet3x2MixedConvexityCorner, Vertex Computation Failure, where the two chamfers meet at the tangent seam (the corner, not the tangent ends: each chamfer alone passes)";
const DRAFTED_ROUNDS: &str = "status 1001 before any geometry: the corner where the front top edge runs tangent into the concave arc, on the cone, is of no kind SMLib's blend corners take";
/// The drafted peg here is the tangent rule's (`taper_tangent_corner_planes`, WR-521), whose
/// arc walls are their exact cones since the fork's 3ae7bfa (WR-521's addendum of
/// 2026-11-02), so the bar's and the disc's cone walls are one face.
const DRAFTED_HERE: &str = "on the tangent rule's cone walls the round builds within the box and checks clean (the rounds together remove 11.97)";
const ROUNDS_R2: &str = "the corner patch where the rounds meet stands 0.0374 out of the front face (box y min -0.0374099)";
const ROUNDS_R5: &str = "the corner patch where the rounds meet stands 0.128 out of the front face (box y min -0.1278205)";
const ROUNDS_R2_HERE: &str = "the rounds together build within the box and check clean, as the blend corner's Mixed corner (built in solidcore from smlib#82)";
const ROUNDS_R5_HERE: &str = "the blend corner's Mixed corner (built in solidcore from smlib#82) builds the rounds together within the box";
const CHAMFERS_HERE: &str = "the blend corner's Mixed corner (built in solidcore from smlib#82) builds the chamfers together at the tangent seam";

const TANGENT_END_CASES: [TangentEndCase; 12] = [
    TangentEndCase {
        name: "chamfers_together_d2",
        run: chamfers_together_d2,
        at_cd47efd: Mark {
            fails_today: Some(CHAMFERS_AT_CD47EFD),
            here: Some(Here::Passes(CHAMFERS_HERE)),
        },
    },
    TangentEndCase {
        name: "concave_chamfer_d2",
        run: concave_chamfer_d2,
        at_cd47efd: PASSES,
    },
    TangentEndCase {
        name: "concave_round_r3_then_front_round_r1_5",
        run: concave_round_r3_then_front_round_r1_5,
        at_cd47efd: PASSES,
    },
    TangentEndCase {
        name: "concave_round_then_front_round_r2",
        run: concave_round_then_front_round_r2,
        at_cd47efd: PASSES,
    },
    TangentEndCase {
        name: "drafted_front_round_r2",
        run: drafted_front_round_r2,
        at_cd47efd: PASSES,
    },
    TangentEndCase {
        name: "drafted_rounds_together_r2",
        run: drafted_rounds_together_r2,
        at_cd47efd: Mark {
            fails_today: Some(DRAFTED_ROUNDS),
            here: Some(Here::Passes(DRAFTED_HERE)),
        },
    },
    TangentEndCase {
        name: "front_chamfer_d2",
        run: front_chamfer_d2,
        at_cd47efd: PASSES,
    },
    TangentEndCase {
        name: "front_round_r1_5_then_concave_round_r3",
        run: front_round_r1_5_then_concave_round_r3,
        at_cd47efd: PASSES,
    },
    TangentEndCase {
        name: "front_round_r2",
        run: front_round_r2,
        at_cd47efd: PASSES,
    },
    TangentEndCase {
        name: "front_round_then_concave_round_r2",
        run: front_round_then_concave_round_r2,
        at_cd47efd: PASSES,
    },
    TangentEndCase {
        name: "rounds_together_r2",
        run: rounds_together_r2,
        at_cd47efd: Mark {
            fails_today: Some(ROUNDS_R2),
            here: Some(Here::Passes(ROUNDS_R2_HERE)),
        },
    },
    TangentEndCase {
        name: "rounds_together_r5",
        run: rounds_together_r5,
        at_cd47efd: Mark {
            fails_today: Some(ROUNDS_R5),
            here: Some(Here::Passes(ROUNDS_R5_HERE)),
        },
    },
];

/// Runs the cases that fail here (`known_failing`) or the others; the names of those whose
/// outcome is not the one listed.
fn tangent_end_cases(known_failing: bool) -> Vec<String> {
    let mut wrong = Vec::new();
    for c in TANGENT_END_CASES
        .iter()
        .filter(|c| c.at_cd47efd.fails_here() == known_failing)
    {
        let mut k = solidcraft_kernel();
        let out = (c.run)(&mut k);
        let failed = !out.failed.is_empty();
        if failed != known_failing {
            wrong.push(format!("{}: {:?}", c.name, out.failed));
        }
    }
    wrong
}

/// 6d73e4f's marks (cd47efd's: 0cc48bd and b34d61b change none) under SolidCraft's profile.
#[test]
fn tangent_end_cases_pass_at_cd47efd() {
    let wrong = tangent_end_cases(false);
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn tangent_end_cases_known_failing_at_cd47efd() {
    let wrong = tangent_end_cases(true);
    assert!(
        wrong.is_empty(),
        "passes, though listed as failing: {wrong:#?}"
    );
}

/// The fork's marks are kept as they are: solidcore's outcome is the fork's except where a
/// case says how it differs.
#[test]
fn tangent_end_marks_are_the_fork_s() {
    for c in &TANGENT_END_CASES {
        let m = &c.at_cd47efd;
        let differs = m.fails_here() != m.fails_today.is_some();
        match &m.here {
            None => assert!(!differs, "{}", c.name),
            Some(Here::Passes(why)) => {
                assert!(m.fails_today.is_some() && !why.is_empty(), "{}", c.name);
            }
        }
    }
}
