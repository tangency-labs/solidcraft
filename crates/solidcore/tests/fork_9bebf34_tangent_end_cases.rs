//! The SolidCraft fork's tangent-end blends (tangency-labs/smlib@9bebf34, the first commit
//! of smlib#76; #2243), with the fork's marks at 9bebf34 and at the end of smlib#76 (cd47efd).
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
//! [`TANGENT_END_CASES`] holds the fork's thirteen cases with its reason for each that fails
//! (`TANGENT_END_CASES` in capi `tests/ops_kernel.cpp`) at 9bebf34 and at cd47efd, kept as
//! they are. cd47efd's are 6d73e4f's: `concave_chamfer_d2`, `drafted_front_round_r2` and
//! `front_chamfer_d2` pass, and `chamfers_together_d2` fails at the 3x2 mixed corner where the
//! chamfers meet; 0cc48bd and b34d61b change no mark. Each case checks, as the fork's
//! `peg_blend`/`peg_result`/`done` do, the blend's success, SMLib's check at level 2, the box
//! (where the case has one) and the volume to 1e-6 relative of the fork's quadrature (where
//! it has one, at volume accuracy 1e-7). Where solidcore's outcome differs from the fork's
//! mark, the case says so ([`Mark::here`]).
//!
//! Each revision's marks run under that revision's profile, the fork's as far as solidcore
//! models it. At cd47efd it is [`solidcraft_kernel`]: every fix SolidCraft turns on but
//! `geometric_g2_continuity` (as `issue_2174_cap_rim_two_edges.rs`) and the two that are not
//! the fork's at cd47efd, WR-685's G^n section (its 11ea2d9 is later) and WR-710's refusal. At
//! 9bebf34 it is [`kernel`]: the same without WR-600's
//! `fillet_nx1_tangent_side_edge_same_convexity`, whose tangent end of the trace is the fork's
//! later 6d73e4f/0cc48bd/b34d61b (under it a tangent end is a rail/edge crossing of the trace
//! and never reaches `CalcRailIntExtendedEdgeuse`), and without WR-697's `fillet_blend_corner`,
//! the fork's later smlib#79 step 4. In the fork that step always comes with the tangent
//! end of the trace: its End (3e26f76) takes a rail's end on the end face's surface, which a
//! tangent end reaching `CalcRailIntExtendedEdgeuse` without WR-600 does not meet (the
//! curvature round fails with 1005 there). At 5e8c9c1, with both, the fork's marks of the
//! thirteen cases are cd47efd's, and solidcore's under [`solidcraft_kernel`] are too.
//!
//! Four of the cases have tests already, under WR-600 on (cd47efd's profile):
//! `fork_856b54e_tangent_end.rs` (front chamfer, concave chamfer, front round, against closed
//! forms) and `fork_cd47efd_tangent_end_rib_sqrt_s.rs` (the curvature round's rib). They stay
//! in the table for the fork's marks. The fork's twins on its `smlib-tangent-ends` branch,
//! c5ea180 and 169adea, have the same diffs as 9bebf34 and a87c502, and on its `replay/1.0.1`
//! 6d73e4f, 0cc48bd and b34d61b are 856b54e, 4ee7d3a and cd47efd (the same trees).
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
use solidcore::blend::fillet_corner::p3::MIXED_CORNER_RAILS_CROSS;
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

/// The fork's profile at 9bebf34, before its tangent end of the trace (module doc):
/// [`solidcraft_kernel`] without WR-600's fix and without WR-697's blend corner.
fn kernel() -> Kernel {
    let mut k = solidcraft_kernel();
    k.fixes.fillet_nx1_tangent_side_edge_same_convexity = false;
    k.fixes.fillet_blend_corner = false;
    k
}

/// Every fix SolidCraft turns on: all but `geometric_g2_continuity`, and but WR-685's
/// `fillet_gn_section_generator`, the fork's later 11ea2d9 (= 0bd3f4d), which smlib#76 does
/// not have (its section refuses the curvature round's thumbweight 1), and but WR-710's
/// `fillet_refuse_mixed_corner_crossing_rails`, not the fork's (it refuses the rounds
/// together: [`rounds_together_refused_by_wr710`]). The fork's profile at cd47efd.
fn solidcraft_kernel() -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity"
            && name != "fillet_gn_section_generator"
            && name != "fillet_refuse_mixed_corner_crossing_rails"
        {
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
    /// The fork marks it passing; it fails here.
    Fails(&'static str),
}

/// The fork's mark of a case at one revision: its reason the case fails there (`None`: it
/// passes), and how solidcore's outcome under that revision's profile differs, if it does.
struct Mark {
    fails_today: Option<&'static str>,
    here: Option<Here>,
}

impl Mark {
    /// Whether the case fails here.
    fn fails_here(&self) -> bool {
        match self.here {
            Some(Here::Passes(_)) => false,
            Some(Here::FailsOtherwise(_) | Here::Fails(_)) => true,
            None => self.fails_today.is_some(),
        }
    }
}

/// The fork revision whose marks a run checks, under that revision's profile (module doc).
#[derive(Clone, Copy, Debug)]
enum Revision {
    /// 9bebf34, under [`kernel`].
    At9bebf34,
    /// cd47efd, the end of smlib#76, under [`solidcraft_kernel`].
    AtCd47efd,
}

impl Revision {
    fn kernel(self) -> Kernel {
        match self {
            Self::At9bebf34 => kernel(),
            Self::AtCd47efd => solidcraft_kernel(),
        }
    }

    fn mark(self, c: &TangentEndCase) -> &Mark {
        match self {
            Self::At9bebf34 => &c.at_9bebf34,
            Self::AtCd47efd => &c.at_cd47efd,
        }
    }
}

/// A tangent-end case: the fork's name and its marks at 9bebf34 and at cd47efd.
struct TangentEndCase {
    name: &'static str,
    run: fn(&mut Kernel) -> Outcome,
    at_9bebf34: Mark,
    at_cd47efd: Mark,
}

/// A mark of a case that passes.
const PASSES: Mark = Mark {
    fails_today: None,
    here: None,
};

const CHAMFERS_AT_9BEBF34: &str = "status 1001: SmFilletNx1Corner, Corner Edge Computation Failure (the chamfers' tangent ends, as front_chamfer_d2 and concave_chamfer_d2)";
const CHAMFERS_AT_CD47EFD: &str = "status 1001: SmFillet3x2MixedConvexityCorner, Vertex Computation Failure, where the two chamfers meet at the tangent seam (the corner, not the tangent ends: each chamfer alone passes)";
const DRAFTED_ROUNDS: &str = "status 1001 before any geometry: the corner where the front top edge runs tangent into the concave arc, on the cone, is of no kind SMLib's blend corners take";
/// The drafted peg here is the tangent rule's (`taper_tangent_corner_planes`, WR-521), whose
/// arc walls are their exact cones since the fork's 3ae7bfa (WR-521's addendum of
/// 2026-11-02), so the bar's and the disc's cone walls are one face. Before, the bar's arc
/// wall was a ruled B-spline taken for a cone over its own domain, and SMLib's check refused
/// the front round (Tests #8, #5, #11, #13: a kink, a zero and a slow speed at interior
/// knots, and a 2D curve with a radius of curvature of 5.5e-5); the rounds together failed as
/// the fork's.
const DRAFTED_HERE: &str = "on the tangent rule's cone walls the round builds within the box and checks clean (front round r2 removes 25.861, the rounds together 12.05 at 9bebf34's profile and 11.97 at cd47efd's)";
const ROUNDS_R2: &str = "the corner patch where the rounds meet stands 0.0374 out of the front face (box y min -0.0374099)";
const ROUNDS_R5: &str = "the corner patch where the rounds meet stands 0.128 out of the front face (box y min -0.1278205)";
const ROUNDS_R2_HERE: &str = "the rounds together build within the box and check clean (with fillet_blend_corner, as the blend corner's Mixed corner, built in solidcore from smlib#82)";
const ROUNDS_R5_HERE: &str = "the blend corner's Mixed corner (fillet_blend_corner, built in solidcore from smlib#82) builds the rounds together within the box; without it the pinned corner patch stands 0.00156 out of the front face (box y min -0.0015609)";
const ROUNDS_R2_HERE_AT_9BEBF34: &str = "the pinned corner patch stands 0.00117 out of the front face (box y min -0.0011736; within the box with WR-641's through point, removed in #2500): this profile has no fillet_blend_corner, whose Mixed corner builds them at cd47efd";
const ROUNDS_R5_HERE_AT_9BEBF34: &str = "the pinned corner patch stands 0.0438 out of the front face (box y min -0.0437708; 0.00156 with WR-641's through point): this profile has no fillet_blend_corner, whose Mixed corner builds the rounds together within the box at cd47efd";
// Since #2500 this profile has no WR-468/WR-641 `fillet_through_extended_rail_end`, the
// fork's d33890f: on this old path (no `fillet_blend_corner`) the front round's rail on the
// front face ends on the extended edge at V, which gives the trace no point, so its trace
// stops at its last known section short of V and its surface reaches V as the trace's
// extension (WR-468's defect). The pinned corner, built on that extension, leaves the front
// face; the blend corner's trace runs on to the edge's end (`c2_trace_to_edge_ends`) and
// builds the same cases at cd47efd.
const CHAMFERS_HERE: &str = "the blend corner's Mixed corner (fillet_blend_corner, built in solidcore from smlib#82) builds the chamfers together at the tangent seam";

const TANGENT_END_CASES: [TangentEndCase; 13] = [
    TangentEndCase {
        name: "chamfers_together_d2",
        run: chamfers_together_d2,
        at_9bebf34: Mark {
            fails_today: Some(CHAMFERS_AT_9BEBF34),
            here: None,
        },
        at_cd47efd: Mark {
            fails_today: Some(CHAMFERS_AT_CD47EFD),
            here: Some(Here::Passes(CHAMFERS_HERE)),
        },
    },
    TangentEndCase {
        name: "concave_chamfer_d2",
        run: concave_chamfer_d2,
        at_9bebf34: Mark {
            fails_today: Some("status 1001: SmFilletNx1Corner, Corner Edge Computation Failure, where the cone runs out over the front top edge and its end on the front face leaves its contact line tangentially"),
            here: None,
        },
        at_cd47efd: PASSES,
    },
    TangentEndCase {
        name: "concave_round_r3_then_front_round_r1_5",
        run: concave_round_r3_then_front_round_r1_5,
        at_9bebf34: PASSES,
        at_cd47efd: PASSES,
    },
    TangentEndCase {
        name: "concave_round_then_front_round_r2",
        run: concave_round_then_front_round_r2,
        at_9bebf34: PASSES,
        at_cd47efd: PASSES,
    },
    TangentEndCase {
        name: "drafted_front_round_r2",
        run: drafted_front_round_r2,
        at_9bebf34: Mark {
            fails_today: Some("SMLib's check refuses the round: Test #13 (a 2D curve with a radius of curvature of 8.6e-6) and Tests #7 and #8 (kinks at interior knots) on its end on the cone"),
            here: Some(Here::Passes(DRAFTED_HERE)),
        },
        at_cd47efd: PASSES,
    },
    TangentEndCase {
        name: "drafted_rounds_together_r2",
        run: drafted_rounds_together_r2,
        at_9bebf34: Mark {
            fails_today: Some(DRAFTED_ROUNDS),
            here: Some(Here::Passes(DRAFTED_HERE)),
        },
        at_cd47efd: Mark {
            fails_today: Some(DRAFTED_ROUNDS),
            here: Some(Here::Passes(DRAFTED_HERE)),
        },
    },
    TangentEndCase {
        name: "front_chamfer_d2",
        run: front_chamfer_d2,
        at_9bebf34: Mark {
            fails_today: Some("status 1001: SmFilletNx1Corner, Corner Edge Computation Failure, where the chamfer's end on the cylinder leaves its contact line on the front face tangentially"),
            here: None,
        },
        at_cd47efd: PASSES,
    },
    TangentEndCase {
        name: "front_curvature_round_r2",
        run: front_curvature_round_r2,
        at_9bebf34: Mark {
            fails_today: None,
            here: Some(Here::Fails("the corner's trim on the cylinder gets a 2D cusp (check_brep #13, radius of curvature 7.1e-5 against 9.8e-5): the trace stops short of V without WR-641's through point (removed, #2500)")),
        },
        at_cd47efd: PASSES,
    },
    TangentEndCase {
        name: "front_round_r1_5_then_concave_round_r3",
        run: front_round_r1_5_then_concave_round_r3,
        at_9bebf34: Mark {
            fails_today: None,
            here: Some(Here::Fails("the corner patch stands 7.1e-5 out of the front face (box y min -7.13e-5): the trace stops short of V without WR-641's through point (removed, #2500)")),
        },
        at_cd47efd: PASSES,
    },
    TangentEndCase {
        name: "front_round_r2",
        run: front_round_r2,
        at_9bebf34: PASSES,
        at_cd47efd: PASSES,
    },
    TangentEndCase {
        name: "front_round_then_concave_round_r2",
        run: front_round_then_concave_round_r2,
        at_9bebf34: PASSES,
        at_cd47efd: PASSES,
    },
    TangentEndCase {
        name: "rounds_together_r2",
        run: rounds_together_r2,
        at_9bebf34: Mark {
            fails_today: Some(ROUNDS_R2),
            here: Some(Here::FailsOtherwise(ROUNDS_R2_HERE_AT_9BEBF34)),
        },
        at_cd47efd: Mark {
            fails_today: Some(ROUNDS_R2),
            here: Some(Here::Passes(ROUNDS_R2_HERE)),
        },
    },
    TangentEndCase {
        name: "rounds_together_r5",
        run: rounds_together_r5,
        at_9bebf34: Mark {
            fails_today: Some(ROUNDS_R5),
            here: Some(Here::FailsOtherwise(ROUNDS_R5_HERE_AT_9BEBF34)),
        },
        at_cd47efd: Mark {
            fails_today: Some(ROUNDS_R5),
            here: Some(Here::Passes(ROUNDS_R5_HERE)),
        },
    },
];

/// Runs the cases that fail here at `revision` (`known_failing`) or the others; the names of
/// those whose outcome is not the one listed.
fn tangent_end_cases(revision: Revision, known_failing: bool) -> Vec<String> {
    let mut wrong = Vec::new();
    for c in TANGENT_END_CASES
        .iter()
        .filter(|c| revision.mark(c).fails_here() == known_failing)
    {
        let mut k = revision.kernel();
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
    let wrong = tangent_end_cases(Revision::At9bebf34, false);
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn tangent_end_cases_known_failing() {
    let wrong = tangent_end_cases(Revision::At9bebf34, true);
    assert!(
        wrong.is_empty(),
        "passes, though listed as failing: {wrong:#?}"
    );
}

/// 6d73e4f's marks (cd47efd's: 0cc48bd and b34d61b change none) under SolidCraft's profile.
#[test]
fn tangent_end_cases_pass_at_cd47efd() {
    let wrong = tangent_end_cases(Revision::AtCd47efd, false);
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn tangent_end_cases_known_failing_at_cd47efd() {
    let wrong = tangent_end_cases(Revision::AtCd47efd, true);
    assert!(
        wrong.is_empty(),
        "passes, though listed as failing: {wrong:#?}"
    );
}

/// The rounds together at r 5: without the blend corner (9bebf34's profile) the pinned corner
/// patch stands 0.0438 out of the front face (0.00156 before #2500 removed WR-641's through
/// point, the front round's trace now stopping short of V on this old path); with it
/// (cd47efd's) the Mixed corner builds them within the box (`issue_2360_mixed_corner.rs`).
#[test]
fn rounds_together_r5_needs_the_mixed_corner() {
    let before = rounds_together_r5(&mut kernel());
    assert_eq!(before.failed.len(), 1, "{:?}", before.failed);
    assert!(
        before.failed[0].contains("y: -0.0437707967"),
        "{:?}",
        before.failed
    );

    let after = rounds_together_r5(&mut solidcraft_kernel());
    assert!(after.failed.is_empty(), "{:?}", after.failed);
}

/// WR-710 (`fillet_refuse_mixed_corner_crossing_rails`, #2256): the front top edge and the
/// concave arc leave V along one tangent, so the bar's top closes to a cusp there and the
/// rails on it cross before the corner. With the refusal on, the rounds together are refused
/// at r 2 and r 5, under either profile, instead of built with the corner patch the fork marks
/// out of the front face (here r 2 builds within the box, its seam at (32, 2, 10) G0 by
/// SolidCraft's continuity check, and r 5 stands 0.00156 out).
#[test]
fn rounds_together_refused_by_wr710() {
    for revision in [Revision::At9bebf34, Revision::AtCd47efd] {
        for run in [rounds_together_r2, rounds_together_r5] {
            let mut k = revision.kernel();
            // the blend corner's Mixed corner builds this corner (`issue_2360_mixed_corner.rs`)
            k.fixes.fillet_blend_corner = false;
            k.fixes.fillet_refuse_mixed_corner_crossing_rails = true;
            let out = run(&mut k);
            assert_eq!(out.failed.len(), 1, "{revision:?} {:?}", out.failed);
            assert!(
                out.failed[0].contains(MIXED_CORNER_RAILS_CROSS),
                "{revision:?} {:?}",
                out.failed
            );
        }
    }
}

/// The fork's marks are kept as they are: solidcore's outcome is the fork's except where a
/// case says how it differs, and every passing case of the fork passes here, but on 9bebf34's
/// old path where WR-641's through point, removed in #2500, built it.
#[test]
fn tangent_end_marks_are_the_fork_s() {
    for revision in [Revision::At9bebf34, Revision::AtCd47efd] {
        for c in &TANGENT_END_CASES {
            let m = revision.mark(c);
            let differs = m.fails_here() != m.fails_today.is_some();
            match &m.here {
                None => assert!(!differs, "{revision:?} {}", c.name),
                Some(Here::Passes(why) | Here::FailsOtherwise(why)) => {
                    assert!(
                        m.fails_today.is_some() && !why.is_empty(),
                        "{revision:?} {}",
                        c.name
                    );
                }
                Some(Here::Fails(why)) => {
                    assert!(
                        matches!(revision, Revision::At9bebf34)
                            && m.fails_today.is_none()
                            && why.contains("#2500"),
                        "{revision:?} {}",
                        c.name
                    );
                }
            }
        }
    }
}

/// 0cc48bd: without the tangent end of the trace the front chamfer fails with status 1001
/// again, as at 9bebf34 (a87c502's crossing is gone); with it (SolidCraft's profile, the fork at
/// cd47efd) it passes through the tangent end. The fork's 0cc48bd: "the tangent ends as on the
/// previous commit (`front_chamfer_d2` passes through the tangent end)".
#[test]
fn front_chamfer_passes_only_through_the_tangent_end() {
    let before = front_chamfer_d2(&mut kernel());
    assert_eq!(before.failed.len(), 1, "{:?}", before.failed);
    assert!(before.failed[0].contains("1001"), "{:?}", before.failed);

    let after = front_chamfer_d2(&mut solidcraft_kernel());
    assert!(after.failed.is_empty(), "{:?}", after.failed);
}
