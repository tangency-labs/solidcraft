//! Runner for the public `SmPrimitiveCreation` entry points of primitive p2 on a fresh
//! B-rep with only its infinite region, ported from upstream; checked against its
//! golden. Every record holds the status, the B-rep's editing flag, the context's doing-boolean flag,
//! the four entity-map counts and the start, side and end face counts, plus the
//! skin's ownership flags, the taper's normal and whether the planar path returned
//! a B-rep. Construction reaches unported topology and defers.
#![allow(clippy::indexing_slicing)] // the record layout is checked by `layout_ok` first

use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::composite_curve::OffsetCornerType;
use solidcore::curve::curve::Curve;
use solidcore::error::{Result, SolidError};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurve, NurbCircleParam};
use solidcore::primitive::primitive_creation::{
    model::PrimitiveModel, p1::PrimitiveCreation, SweepOptions,
};
use solidcore::sweep::topology_sweep::access::SweepAccess;
use solidcore::topology::{Brep, BrepId, Region, RegionId};

/// Differential runner.
pub struct Runner;

#[derive(Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default)]
    expected_rejection: bool,
    op: i32,
    data: Vec<f64>,
}

/// The status code of an operation error (its upstream `SmStatus`).
fn error(e: &SolidError) -> f64 {
    match e {
        e if crate::output::recorded_status(e).is_some() => {
            f64::from(crate::output::recorded_status(e).unwrap_or(-1))
        }
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.,
        _ => 1001.,
    }
}

/// p1's curve record: a B-spline line or a quadratic circular arc.
fn curve(data: &[f64]) -> Result<BSplineCurve> {
    let [kind, x, y, z, a, b, c] = data else {
        return Err(crate::output::routine_failure("fixture"));
    };
    if *kind == 0. {
        BSplineCurve::create_line_segment(3, Vec3::new(*x, *y, *z), Vec3::new(*a, *b, *c))
    } else {
        BSplineCurve::create_circle_segment(
            3,
            &Axis2Placement::new(Vec3::new(*x, *y, *z), Vec3::X, Vec3::Y),
            *a,
            *b,
            *c,
            NurbCircleParam::Quadratic,
        )
    }
}
fn curves(data: &[f64]) -> Result<Vec<Curve>> {
    data.as_chunks::<7>()
        .0
        .iter()
        .map(|record| curve(record).map(Curve::from))
        .collect()
}
fn frame(f: &[f64]) -> Axis2Placement {
    Axis2Placement::new(
        Vec3::new(f[0], f[1], f[2]),
        Vec3::new(f[3], f[4], f[5]),
        Vec3::new(f[6], f[7], f[8]),
    )
}
fn options(f: &[f64]) -> SweepOptions {
    SweepOptions {
        cap_ends: f[0] != 0.,
        translational_sweep: f[1] != 0.,
        move_profile_center_to_path: f[2] != 0.,
        orient_profile_perpendicular_to_path: f[3] != 0.,
        move_to_profile: f[4] != 0.,
        profile_at_path_start: false,
        up: None,
    }
}

// The native fixture is a fresh B-rep with only its infinite region. The entry points
// run on it through the production adapter (`PrimitiveModel`), which also makes the
// scratch B-reps (`new SmBrep()`) and copies faces between them; the calls it cannot
// resolve yet fail as `Unported` with their unit.
fn fixture_brep(editing: bool) -> Result<Brep> {
    let mut b = Brep::new();
    let region = b.add_region_record(Region {
        in_brep: true,
        ..Default::default()
    })?;
    b.set_infinite_region(Some(region));
    b.set_editing_enabled(editing);
    Ok(b)
}
fn unavailable<T>() -> Result<T> {
    Err(SolidError::invalid_input(
        "primitive_p2_fixture",
        "fixture has no topology/service for this call",
    ))
}

/// Whether `n` is a whole count in `0..=max`.
fn count(n: f64, max: usize) -> bool {
    n.fract() == 0.0 && (0.0..=max as f64).contains(&n)
}

/// Whether `d` has the record layout of op `op` (its lengths and counts), so that
/// [`entry`] can index it.
fn layout_ok(op: i32, d: &[f64]) -> bool {
    match op {
        1 => d.len() == 15,
        2 => d.len() == 13,
        3 | 5 | 6 => d.len() == 14,
        4 => d.len() >= 5 && count(d[4], d.len() - 5) && d.len() >= 5 + d[4] as usize,
        7 => d.len() == 12,
        8 => d.len() >= 8,
        9 => {
            d.len() >= 6
                && count(d[4], d.len())
                && count(d[5], d.len())
                && d.len() == 6 + 7 * (d[4] as usize + d[5] as usize)
        }
        _ => false,
    }
}

/// Run one entry point on the fixture and build its record.
fn entry(i: &Inputs) -> Result<Vec<f64>> {
    let d = &i.data;
    if !layout_ok(i.op, d) {
        return Err(SolidError::invalid_input(
            "difftest::primitive_p2",
            "record layout",
        ));
    }
    let b = BrepId::from_index(0);
    let mut starts = Vec::new();
    let mut sides = Vec::new();
    let mut ends = Vec::new();
    let mut extra = Vec::new();
    if i.op == 9 {
        let mut k = crate::runner_kernel();
        let mut model = PrimitiveModel::new(&mut k, fixture_brep(false)?);
        let profile_count = d[4] as usize;
        let profiles = curves(&d[6..6 + 7 * profile_count])?;
        let paths = curves(&d[6 + 7 * profile_count..])?;
        let result = PrimitiveCreation::create_sweep_along_planar_path(
            &mut model,
            b,
            &profiles,
            &paths,
            d[0],
            d[1] != 0.,
            d[2] != 0.,
            d[3] != 0.,
        );
        if let Err(e) = &result {
            if e.is_unported() {
                return Err(e.clone());
            }
        }
        return Ok(vec![
            result.as_ref().err().map_or(0., error),
            0.,
            f64::from(model.kernel(b)?.context.doing_boolean()),
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            f64::from(result.is_ok()),
        ]);
    }
    let mut k = crate::runner_kernel();
    let mut model = PrimitiveModel::new(&mut k, fixture_brep(d[0] != 0.)?);
    let mut primitive = PrimitiveCreation::new(Some(RegionId::from_index(0)), 0, false);
    let result = match i.op {
        1 => primitive.create_partial_cone(
            &mut model,
            b,
            d[1],
            d[2],
            d[3],
            d[4],
            d[5],
            &frame(&d[6..15]),
        ),
        2 => primitive.create_partial_sphere(&mut model, b, d[1], d[2], d[3], &frame(&d[4..13])),
        3 => {
            primitive.create_partial_torus(&mut model, b, d[1], d[2], d[3], d[4], &frame(&d[5..14]))
        }
        4 => {
            let profile_count = d[4] as usize;
            let counts: Vec<usize> = d[5..5 + profile_count]
                .iter()
                .map(|c| *c as usize)
                .collect();
            let mut input: Vec<Option<Curve>> = curves(&d[5 + profile_count..])?
                .into_iter()
                .map(Some)
                .collect();
            let mut transferred = Vec::new();
            let result = primitive.create_skin_primitive(
                &mut model,
                b,
                &counts,
                &mut input,
                d[1],
                d[2] as usize,
                d[3] != 0.,
                &mut starts,
                &mut sides,
                &mut ends,
                &mut transferred,
            );
            // the native report has one flag per input curve
            extra.extend(
                (0..input.len()).map(|j| f64::from(transferred.get(j).copied().unwrap_or(false))),
            );
            result
        }
        5 | 6 => {
            let path = curve(&d[7..14])?;
            if i.op == 5 {
                primitive.create_curve_sweep_from_edges(
                    &mut model,
                    b,
                    &[],
                    &path,
                    None,
                    None,
                    d[1],
                    options(&d[2..7]),
                    &mut starts,
                    &mut sides,
                    &mut ends,
                )
            } else {
                primitive.create_curve_sweep_from_faces(
                    &mut model,
                    b,
                    &[],
                    &path,
                    None,
                    None,
                    d[1],
                    options(&d[2..7]),
                    &mut starts,
                    &mut sides,
                    &mut ends,
                )
            }
        }
        7 => {
            let path = curve(&d[5..12])?;
            primitive.create_pipe_sweep(
                &mut model,
                b,
                d[1],
                &path,
                d[2],
                d[3] != 0.,
                d[4] != 0.,
                &mut starts,
                &mut sides,
                &mut ends,
            )
        }
        8 => {
            let mut normal = Vec3::new(d[4], d[5], d[6]);
            let corner = match d[7] as i64 {
                0 => OffsetCornerType::LinearExtension,
                1 => OffsetCornerType::FilletCorner,
                _ => OffsetCornerType::LinearChamfer,
            };
            let result = primitive.create_taper_extrude(
                &mut model,
                b,
                curves(&d[8..])?,
                d[1],
                d[2],
                d[3] as i32,
                &mut normal,
                corner,
            );
            extra.extend(normal.to_array());
            result
        }
        _ => return unavailable(),
    };
    if let Err(e) = &result {
        if e.is_unported() {
            return Err(e.clone());
        }
    }
    let mut record = vec![
        result.as_ref().err().map_or(0., error),
        f64::from(model.brep(b)?.editing_enabled()),
        f64::from(model.kernel(b)?.context.doing_boolean()),
        primitive.maps.to_same.len() as f64,
        primitive.maps.to_higher.len() as f64,
        primitive.maps.from_same.len() as f64,
        primitive.maps.from_lower.len() as f64,
        starts.len() as f64,
        sides.len() as f64,
        ends.len() as f64,
    ];
    record.extend(extra);
    Ok(record)
}

/// A generated case near the unit's boundaries: each op, scale `1e-6..1e6`.
fn generated(rng: &mut Rng) -> (i32, Vec<f64>) {
    let scale = 10f64.powi(rng.range_i64(-6, 6) as i32);
    let edit = f64::from(rng.below(2) != 0);
    // the frame: origin, an X axis and a Y axis not parallel to it
    let mut f = vec![
        rng.uniform(-1., 1.) * scale,
        rng.uniform(-1., 1.) * scale,
        rng.uniform(-1., 1.) * scale,
        1.,
        rng.uniform(-0.5, 0.5),
        0.,
        0.,
        1.,
        rng.uniform(-0.5, 0.5),
    ];
    if rng.below(4) == 0 {
        f.splice(3..9, [0., 0., 1., 1., 0., 0.]);
    }
    // a turn near its boundaries: zero, the effective zero, a part turn, a full turn
    let start = [0., -45., 30.][rng.below(3) as usize];
    let turn = [
        0.,
        1e-13,
        2e-12,
        90.,
        360.,
        360. * (1. + 1e-13),
        360. * (1. + 1e-11),
        359.,
    ][rng.below(8) as usize];
    let size = |rng: &mut Rng| {
        [0., 1e-13, 5e-12, 1e-6, 1., 2.][rng.below(6) as usize]
            * if rng.below(2) == 0 { scale } else { 1. }
    };
    match rng.below(7) {
        0 => {
            let mut d = vec![edit, size(rng), size(rng), size(rng), start, start + turn];
            d.extend(&f);
            (1, d)
        }
        1 => {
            let mut d = vec![edit, size(rng), start, start + turn];
            d.extend(&f);
            (2, d)
        }
        2 => {
            let major = size(rng);
            let minor = if rng.below(4) == 0 { major } else { size(rng) };
            let mut d = vec![edit, major, minor, start, start + turn];
            d.extend(&f);
            (3, d)
        }
        3 => {
            // a skin of zero, one or two profiles of one circle each, at scale 1e-3 or
            // more (smaller skins take seconds to minutes natively, HANG-35)
            let side = scale.max(1e-3);
            let profiles = rng.below(3) as usize;
            let mut d = vec![
                edit,
                0.,
                1. + rng.below(3) as f64,
                f64::from(rng.below(2) != 0),
            ];
            d.push(profiles as f64);
            d.extend(std::iter::repeat_n(1., profiles));
            for p in 0..profiles {
                d.extend([1., 0., 0., p as f64 * side, side, 0., 360.]);
            }
            (4, d)
        }
        4 => {
            let mut d = vec![edit, scale * 1e-5];
            d.extend((0..5).map(|_| f64::from(rng.below(2) != 0)));
            d.extend([0., 0., 0., 0., 0., 0., scale]);
            (5 + rng.below(2) as i32, d)
        }
        5 => (
            7,
            vec![
                edit,
                size(rng),
                0.,
                f64::from(rng.below(2) != 0),
                f64::from(rng.below(2) != 0),
                0.,
                0.,
                0.,
                0.,
                0.,
                0.,
                scale,
            ],
        ),
        _ => {
            // a square taper (sides of at least 1e-3, UB-112) or a planar path
            if rng.below(2) == 0 {
                let side = scale.max(1e-3);
                let normal = [0., 1., -1.][rng.below(3) as usize];
                let mut d = vec![
                    edit,
                    scale,
                    rng.uniform(-30., 30.),
                    rng.below(4) as f64,
                    0.,
                    0.,
                    normal,
                    rng.below(3) as f64,
                ];
                let mut lines = vec![
                    [0., 0., 1., 0.],
                    [1., 0., 1., 1.],
                    [1., 1., 0., 1.],
                    [0., 1., 0., 0.],
                ];
                let pick = |rng: &mut Rng, [x0, y0, x1, y1]: [f64; 4]| {
                    if rng.below(2) == 0 {
                        [x0, y0, x1, y1]
                    } else {
                        [x1, y1, x0, y0]
                    }
                };
                // UB-50's chaining: a repeated side, either way (kept), a line with its
                // repeat alone (refused), a line back past a short first link (refused
                // for 3e-5, kept for 5e-5)
                match rng.below(4) {
                    0 => {
                        let side = lines[rng.below(4) as usize];
                        let repeat = pick(rng, side);
                        lines.push(repeat);
                    }
                    1 => lines = vec![[0., 0., 1., 0.], pick(rng, [0., 0., 1., 0.])],
                    2 => {
                        let short = [3e-5, 5e-5][rng.below(2) as usize] / side;
                        lines = vec![[0., 0., short, 0.], [short, 0., 1., 0.], [1., 0., 0., 0.]];
                    }
                    _ => {}
                }
                for [x0, y0, x1, y1] in lines {
                    d.extend([0., x0 * side, y0 * side, 0., x1 * side, y1 * side, 0.]);
                }
                (8, d)
            } else {
                // at scale 1e-3 or more: a 1e-4 path took over a minute natively
                let side = scale.max(1e-3);
                let mut d = vec![1e-5 * side, 1., 1., f64::from(rng.below(2) != 0), 1., 2.];
                d.extend([1., 0., 0., 0., 0.1 * side, 0., 360.]);
                d.extend([0., 0., 0., 0., 0., 0., side]);
                d.extend([0., 0., 0., side, side, 0., side]);
                (9, d)
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "primitive.primitive_creation_p2"
    }
    fn cases(
        &self,
        ctx: &crate::case::CaseContext,
    ) -> std::result::Result<Vec<Case>, HarnessError> {
        crate::runner::default_cases(
            self,
            &crate::case::CaseContext {
                seeds: ctx.seeds.max(256),
                ..ctx.clone()
            },
        )
    }
    fn run_rust(&self, case: &Case) -> std::result::Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        match entry(&i) {
            Ok(values) => Ok(Output::new().values("record", values)),
            Err(e) => {
                Output::unported(&e).map_or_else(|| Err(HarnessError::new(e.to_string())), Ok)
            }
        }
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let (op, data) = generated(rng);
        serde_json::to_value(Inputs {
            expected_rejection: false,
            op,
            data,
        })
        .ok()
    }
}

/// #14246 G1 (WR-662 retired): the rows whose success became a refusal. On main every one
/// of them except the bowtie built nothing (no face in the B-rep) and returned success;
/// the bowtie built one open face. Each row is refused for a closed-form reason:
/// - `Small`: a cone, sphere or pipe whose sizes are all within the B-rep's tolerance of
///   zero (radius, or radius and height), so every face it would have is degenerate;
/// - `NoArea`: a taper profile of zero signed area: its lines all lie on one line (a line
///   and its reverse, or a "triangle" whose corners are collinear), or it is the bowtie,
///   whose two lobes cancel and which crosses itself. Such a profile bounds no region, so
///   it has no caps and the taper no solid;
/// - `Open`: a taper profile that does not close, so it has no caps;
/// - `WasRefused`: main refused it too (status to status).
#[cfg(test)]
mod moved_rows {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use solidcore::error::sweep::SweepFailure;
    use solidcore::kernel::Kernel;

    /// Why no requested body exists for the row.
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Reason {
        /// Every size of the cone, sphere or pipe is within the B-rep's tolerance of zero.
        Small,
        /// The taper profile is closed and its signed area is exactly zero with no two
        /// lines crossing: its lines run back over each other, so it encloses nothing.
        NoArea,
        /// Two lines of the taper profile that do not share an end cross at a point
        /// inside both: the loop is not simple, so no cap face has it as its boundary,
        /// and its two lobes meet at that one point, so no closed capped solid exists.
        SelfCrossing,
        /// The taper profile does not close.
        Open,
        /// Main refused the row too; only the status differs.
        WasRefused,
    }

    /// The parameters along `a` and `b` (2D segments `[x0, y0, x1, y1]`) where they cross,
    /// if they are not parallel.
    fn crossing(a: [f64; 4], b: [f64; 4]) -> Option<(f64, f64)> {
        let (dx, dy) = (a[2] - a[0], a[3] - a[1]);
        let (ex, ey) = (b[2] - b[0], b[3] - b[1]);
        let det = dx * ey - dy * ex;
        if det == 0.0 {
            return None;
        }
        let (fx, fy) = (b[0] - a[0], b[1] - a[1]);
        Some(((fx * ey - fy * ex) / det, (fx * dy - fy * dx) / det))
    }

    /// The row's primitive call as [`entry`] makes it, with its typed result and the
    /// number of faces in the B-rep afterwards.
    fn run(i: &Inputs) -> (Result<()>, usize, f64) {
        let d = &i.data;
        let b = BrepId::from_index(0);
        let mut k = Kernel::default();
        let mut model = PrimitiveModel::new(&mut k, fixture_brep(d[0] != 0.).unwrap());
        let tolerance = model.brep(b).unwrap().tolerance();
        let mut primitive = PrimitiveCreation::new(Some(RegionId::from_index(0)), 0, false);
        let (mut starts, mut sides, mut ends) = (Vec::new(), Vec::new(), Vec::new());
        let result = match i.op {
            1 => primitive.create_partial_cone(
                &mut model,
                b,
                d[1],
                d[2],
                d[3],
                d[4],
                d[5],
                &frame(&d[6..15]),
            ),
            2 => primitive.create_partial_sphere(&mut model, b, d[1], d[2], d[3], &frame(&d[4..13])),
            7 => primitive.create_pipe_sweep(
                &mut model,
                b,
                d[1],
                &curve(&d[5..12]).unwrap(),
                d[2],
                d[3] != 0.,
                d[4] != 0.,
                &mut starts,
                &mut sides,
                &mut ends,
            ),
            8 => {
                let corner = match d[7] as i64 {
                    0 => OffsetCornerType::LinearExtension,
                    1 => OffsetCornerType::FilletCorner,
                    _ => OffsetCornerType::LinearChamfer,
                };
                primitive.create_taper_extrude(
                    &mut model,
                    b,
                    curves(&d[8..]).unwrap(),
                    d[1],
                    d[2],
                    d[3] as i32,
                    &mut Vec3::new(d[4], d[5], d[6]),
                    corner,
                )
            }
            op => panic!("{op}"),
        };
        let faces = solidcore::topology::brep::p1::faces(model.brep(b).unwrap(), None)
            .unwrap()
            .len();
        (result, faces, tolerance)
    }

    /// The taper profile's line ends in the XY plane, in order.
    fn ends(d: &[f64]) -> Vec<[f64; 4]> {
        d[8..]
            .as_chunks::<7>()
            .0
            .iter()
            .map(|c| {
                assert_eq!(c[0], 0.0, "lines only");
                assert_eq!((c[3], c[6]), (0.0, 0.0), "in the XY plane");
                [c[1], c[2], c[4], c[5]]
            })
            .collect()
    }

    fn reason(i: &Inputs, tolerance: f64) -> Reason {
        let d = &i.data;
        match i.op {
            1 => {
                assert!([d[1], d[2], d[3]].iter().all(|s| s.abs() <= tolerance));
                Reason::Small
            }
            2 => {
                assert!(d[1].abs() <= tolerance);
                Reason::Small
            }
            7 => {
                assert!(d[1].abs() <= tolerance, "radius");
                Reason::Small
            }
            8 => {
                let lines = ends(d);
                let closed = lines.windows(2).all(|w| w[0][2] == w[1][0] && w[0][3] == w[1][1])
                    && lines.last().unwrap()[2..] == lines.first().unwrap()[..2];
                if !closed {
                    return Reason::Open;
                }
                let n = lines.len();
                for (x, &a) in lines.iter().enumerate() {
                    for (y, &b) in lines.iter().enumerate().skip(x + 2) {
                        if x == 0 && y == n - 1 {
                            continue; // the closing pair shares the loop's start
                        }
                        if let Some((t, u)) = crossing(a, b) {
                            if t > 0.0 && t < 1.0 && u > 0.0 && u < 1.0 {
                                return Reason::SelfCrossing;
                            }
                        }
                    }
                }
                let area: f64 = lines.iter().map(|l| l[0] * l[3] - l[2] * l[1]).sum();
                assert_eq!(area, 0.0, "a profile with area");
                Reason::NoArea
            }
            op => panic!("{op}"),
        }
    }

    /// tangency-labs/solidcraft#14425 (WR-448's `revolve_analytic_profile_faces` retired)
    /// moved two cone rows from one refusal to another (1007 to 1006): their side face is
    /// now the analytic cylinder, which the sweep's `is_degenerate_curve` check no longer
    /// takes as a surface degenerating to a curve (the B-spline surface of revolution
    /// was refused there, 1007), so the call refuses one step later (tangency-labs/solidcraft#15350 tracks that test's
    /// dependence on the parametrisation). The cone's height is within the B-rep's
    /// tolerance of zero, so its base and top circles, and each vertex of one with the
    /// vertex above it, are one within tolerance: no solid of that height has a valid
    /// B-rep, and the call is refused either way (the API drops the B-rep it was
    /// building on a refusal, so no body reaches the caller). The refusing step follows:
    /// the swept profile edge's own curve is the side line from a base vertex to the
    /// vertex above it, of length the height, within tolerance of zero, so the face
    /// builder drops that line as degenerate and the sweep refuses with the typed
    /// [`SweepFailure::SweptEdgeCurveDropped`] (status 1006), which is asserted here.
    #[test]
    fn the_cones_below_tolerance_height_stay_refused() {
        let data: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/primitive.primitive_creation_p2/ub215_degenerate_faces.json"
        ))
        .unwrap();
        let mut seen = 0;
        for name in [
            "ub215_degenerate_face_side_line_5e-7",
            "ub215_degenerate_face_side_line_9.9e-7",
        ] {
            let case = data["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["name"] == name)
                .unwrap();
            let i: Inputs = serde_json::from_value(case["inputs"].clone()).unwrap();
            assert_eq!(i.op, 1, "{name}: a partial cone");
            let (result, faces, tolerance) = run(&i);
            assert!(i.data[1] > 0.0 && i.data[1] <= tolerance, "{name}: height {}", i.data[1]);
            let err = result.expect_err(name);
            // the refusal is the sweep's: `create_partial_rotational_sweep` has built the
            // planar profile face it turns, and no swept face follows (the API layer then
            // drops this B-rep)
            assert_eq!(faces, 1, "{name}: the profile face only");
            assert!(
                matches!(err.sweep_failure(), Some(SweepFailure::SweptEdgeCurveDropped)),
                "{name}: {err:?}"
            );
            assert_eq!(error(&err), 1006.0, "{name}");
            seen += 1;
        }
        assert_eq!(seen, 2);
    }

    /// (row, reason)
    const ROWS: [(&str, Reason); 21] = [
        ("gen0027", Reason::Small),
        ("gen0041", Reason::WasRefused),
        ("gen0046", Reason::Small),
        ("gen0082", Reason::NoArea),
        ("gen0105", Reason::Small),
        ("gen0112", Reason::NoArea),
        ("gen0119", Reason::NoArea),
        ("gen0123", Reason::Small),
        ("gen0134", Reason::NoArea),
        ("gen0135", Reason::Small),
        ("gen0158", Reason::WasRefused),
        ("gen0175", Reason::Small),
        ("gen0216", Reason::NoArea),
        ("gen0230", Reason::Small),
        ("gen0235", Reason::NoArea),
        ("line_back_2_5e-5_off_the_last_mid_start_cap", Reason::NoArea),
        ("review3_start_cap_bowtie", Reason::SelfCrossing),
        ("ub50_taper_end_cap_offsets_of_a_loop_enclosing_nothing", Reason::NoArea),
        ("ub50_taper_line_and_its_reverse", Reason::NoArea),
        ("ub50_taper_line_back_through_the_last_mid", Reason::NoArea),
        ("ub50_taper_two_identical_lines", Reason::Open),
    ];

    /// tangency-labs/solidcraft#18634: partial cones far from the origin whose refusal
    /// status moved when the degeneracy tests stopped scaling their zero with position.
    /// Main refused each of them too, so no answer is lost. Their status is now the one
    /// the same cone gets at the origin (the row with its frame's origin set to zero and
    /// its axes kept), so where the cone is no longer decides how it is refused.
    const FAR_CONES: [&str; 4] = [
        "ub215b_cone_random_refused_1",
        "ub215b_cone_random_refused_3",
        "ub215b_cone_top_1.0005e-6_at_1000",
        "ub215b_cone_top_2e-6_at_1e5",
    ];

    #[test]
    fn far_cones_are_refused_as_at_the_origin() {
        for name in FAR_CONES {
            let far: Inputs = crate::case::regression_case(&Runner, name).parse().unwrap();
            assert_eq!(far.op, 1, "{name}");
            assert!(far.data[6..9].iter().any(|c| c.abs() >= 1e3), "{name}: far");
            let mut near = far.clone();
            near.data[6..9].fill(0.0);
            let far_error = run(&far).0.expect_err(name);
            let near_error = run(&near).0.expect_err(name);
            assert_eq!(error(&far_error), error(&near_error), "{name}: {far_error:?} / {near_error:?}");
            assert_eq!(far_error.operation(), near_error.operation(), "{name}");
        }
    }

    #[test]
    fn each_moved_row_is_refused_for_its_reason() {
        for (name, expected) in ROWS {
            let i: Inputs = crate::case::regression_case(&Runner, name).parse().unwrap();
            let (result, faces, tolerance) = run(&i);
            result.expect_err(name);
            if expected != Reason::WasRefused {
                let found = reason(&i, tolerance);
                assert_eq!(found, expected, "{name}: reason");
            }
            // Main built nothing for every row but the bowtie. There it returned one face
            // (10 edges, level-2 clean, not a manifold solid): not the capped solid asked
            // for, which cannot exist on a self-crossing loop. The refusal leaves no face.
            assert_eq!(faces, 0, "{name}: the refusal leaves no face");
        }
    }
}
