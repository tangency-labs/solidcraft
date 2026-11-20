//! Runner for unit `topology.brep_mirror` (`SmBrep::Mirror`). Ported from upstream
//! `SmBrep::Mirror` and `SmBrep::Transform`; checked against its golden.
//!
//! The fixtures are upstream `.smb` solids read by the Rust reader; each is mirrored about
//! an axis-aligned or a generated plane by `topology::brep::mirror::mirror`. The output is
//! the status, the whole B-rep graph (topology, vertex points, tolerances, intervals, UV
//! domains) and samples of every edge curve, every edge use's UV trim curve and every face
//! surface (type code, natural domain, evaluated points).
//!
//! A case with a `matrix` runs `SmBrep::Transform(const double[4][4], bScaleTol)`
//! (`topology::brep::mirror::transform_matrix`) instead of the mirror, recorded the same
//! way: the fixture file `transform_matrix.json` (identity, rotation, uniform and
//! non-uniform scale, shear, mirror, near-singular, singular, huge and tiny scale) and
//! [`MATRIX_GENERATED`] generated `mat####` cases, seeded apart from the `gen####`
//! mirror stream.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::topology::brep::mirror::{mirror, transform_matrix};
use solidcore::topology::brep::p3::notify_serial;
use solidcore::topology::Brep;
use solidcore::{Result as SolidResult, SolidError};

use super::boolean_merge_p1::graph_owned;
use super::topology_vertexuse::{path_of, BrepIn};
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};

// The case's transform certificate log (test builds only).
#[cfg(test)]
use super::topology_brep_p5::transform_certificate::Fired as CertificateLog;

/// The runner of `SmBrep::Mirror`.
pub struct Runner;

/// Upstream solids (relative to `TestFiles/pt_TestFiles`) the generated cases mirror.
pub(super) const FILES: [&str; 12] = [
    "Solids/box.smb",
    "Solids/cylinder.smb",
    "Solids/sphere.smb",
    "Solids/pyramid.smb",
    "Solids/revolution.smb",
    "Solids/extrusion.smb",
    "Solids/l_shape_with_boss.smb",
    "Solids/nurb_fillet.smb",
    "cone.smb",
    "disk.smb",
    "box_with_large_fillet.smb",
    "plane_circ.smb",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Inputs {
    /// An upstream test file, relative to `upstream/TestFiles/pt_TestFiles`.
    file: String,
    /// Rebuild the B-rep's topology on reading (`ReadFromFile`'s flag).
    rebuild: bool,
    /// `m_bEditingEnabled` of the B-rep.
    editing: bool,
    /// The mirror placement's origin, X axis and Y axis (the plane is its XY plane).
    origin: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    /// When set, `SmBrep::Transform` by this 4x4 matrix (rows; the translation is row
    /// 3) instead of the mirror.
    matrix: Option<[[f64; 4]; 4]>,
    /// `bScaleTol` of the matrix transform (upstream's default `TRUE`).
    scale_tol: bool,
}

/// Generated matrix-transform cases per run.
pub const MATRIX_GENERATED: usize = 96;

impl Default for Inputs {
    fn default() -> Self {
        Self {
            file: "Solids/box.smb".into(),
            rebuild: false,
            editing: true,
            origin: [0.; 3],
            x: [1., 0., 0.],
            y: [0., 1., 0.],
            matrix: None,
            scale_tol: true,
        }
    }
}

pub(super) fn herr(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

fn v(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn path(i: &Inputs) -> Result<String, HarnessError> {
    path_of(&BrepIn::File {
        file: i.file.clone(),
        rebuild: i.rebuild,
    })
}

pub(super) fn status(r: SolidResult<()>) -> SolidResult<f64> {
    match r {
        Ok(()) => Ok(0.),
        Err(e) if e.is_unported() => Err(e),
        Err(up_err) if solidcore::error::upstream_status(&up_err).is_some() => Ok(f64::from(solidcore::error::upstream_status(&up_err).unwrap_or(-1))),
        // a failure in solidcore's terms (#2615): its C++ comparison status
        Err(e @ SolidError::Failed(_)) => Ok(f64::from(e.sm_status().unwrap_or(solidcore::error::SM_ERR))),
        Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => Ok(1007.),
        Err(e) => Err(e),
    }
}

// ---- geometry samples (the shim's `put_geometry`) ------------------------------------

/// A curve's record block: `-1`, or its type, natural interval and five samples at even
/// steps, each `0, x, y, z` or `1` (not evaluated).
pub(crate) fn curve_samples(
    k: &Kernel,
    out: &mut Vec<f64>,
    c: Option<&solidcore::curve::curve::Curve>,
) {
    let Some(c) = c else {
        out.push(-1.);
        return;
    };
    out.push(c.type_code() as f64);
    let d = c.natural_interval();
    out.extend([d.min(), d.max()]);
    for i in 0..5 {
        match c.evaluate_point(k, d.evaluate(f64::from(i) / 4.0)) {
            Ok(p) => out.extend([0., p.x, p.y, p.z]),
            Err(_) => out.push(1.),
        }
    }
}

fn surface_samples(
    k: &Kernel,
    out: &mut Vec<f64>,
    s: Option<&solidcore::surface::surface::Surface>,
) {
    let Some(s) = s else {
        out.push(-1.);
        return;
    };
    out.push(s.type_code() as f64);
    let d = s.natural_uv_domain_if_built();
    let (a, z) = (d.min(), d.max());
    out.extend([a.x, a.y, z.x, z.y]);
    for i in 0..3 {
        for j in 0..3 {
            let uv = solidcore::geom::vector2d::Point2::new(
                a.x + (z.x - a.x) * f64::from(i) / 2.0,
                a.y + (z.y - a.y) * f64::from(j) / 2.0,
            );
            match s.evaluate_point(k, uv) {
                Ok(p) => out.extend([0., p.x, p.y, p.z]),
                Err(_) => out.push(1.),
            }
        }
    }
}

/// The geometry record of the shim's `put_geometry`; `uv_trim` samples the edge uses' UV trim
/// curves too.
pub(super) fn geometry(k: &Kernel, b: &Brep, uv_trim: bool) -> SolidResult<Vec<f64>> {
    use solidcore::topology::brep::p1;
    #[cfg(test)]
    crate::certify::written(b);
    let mut out = Vec::new();
    let edges = p1::edges(b, None)?;
    out.push(edges.len() as f64);
    for e in edges {
        let edge = b.edge(e)?;
        curve_samples(k, &mut out, edge.curve.as_ref());
        out.push(edge.edgeuses.len() as f64);
        for &eu in &edge.edgeuses {
            if uv_trim {
                curve_samples(k, &mut out, b.edgeuse(eu)?.uv_trim_curve.as_ref());
            }
        }
    }
    let faces = p1::faces(b, None)?;
    out.push(faces.len() as f64);
    for f in faces {
        surface_samples(k, &mut out, b.face(f)?.surface.as_ref());
    }
    Ok(out)
}

/// Clears the faces' `loops` and the loops' `edges` lists: Rust-only logical records
/// the native export walk never has, so the import leaves them empty while the Rust
/// reader fills them (as `topology.brep_cutting` and `topology.brep_local_operation`).
fn clear_logical_lists(b: &mut Brep) -> SolidResult<()> {
    let faces: Vec<_> = b.faces().ids().collect();
    for id in faces {
        b.face_mut(id)?.loops.clear();
    }
    let loops: Vec<_> = b.loops().ids().collect();
    for id in loops {
        b.loop_mut(id)?.edges.clear();
    }
    Ok(())
}

impl Runner {
    /// [`UnitDiff::run_rust`], with the case's certificate log in test builds.
    fn run(
        case: &Case,
        #[cfg(test)] log: &mut CertificateLog,
    ) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let mut k = Kernel::default();
        // the Rust reader's B-rep of the same file, its geometry owners named by the
        // Rust reader (no C++ export or owner reaches the Rust side)
        let mut b = match crate::own_reader::read_file_rebuild(&mut k, &path(&i)?, i.rebuild)? {
            Ok(b) => b,
            Err(e) => {
                return Output::unported(&e).ok_or_else(|| herr(format!("rust read: {e}")));
            }
        };
        b.set_editing_enabled(i.editing);
        let plane = Axis2Placement::new(v(i.origin), v(i.x), v(i.y));
        let mut run = || -> SolidResult<Vec<f64>> {
            // (the input, for the certificate's step-by-step replay)
            #[cfg(test)]
            let input = b.clone_standing_for_self().expect("certificate input copy");
            let r = match &i.matrix {
                Some(m) => transform_matrix(&mut k, &mut b, m, i.scale_tol),
                None => mirror(&mut k, &mut b, &plane),
            };
            // (a mirror's own vertex tolerance updates are not `Transform`'s: only the
            // 4x4 transforms are checked, one `Transform` step at a time)
            #[cfg(test)]
            if let (Ok(()), Some(m)) = (&r, &i.matrix) {
                replay_certified(&mut k, input, &b, m, i.scale_tol, log)
                    .expect("certificate replay");
            }
            let st = status(r)?;
            let mut record = vec![st];
            record.extend(geometry(&k, &b, true)?);
            Ok(record)
        };
        match run() {
            Ok(record) => {
                clear_logical_lists(&mut b).map_err(herr)?;
                normalize_face_shells(&mut b).map_err(herr)?;
                // (the serial whose topology keys name the edit's new geometry)
                let serial = notify_serial(&mut k.context, &mut b).map_err(herr)?;
                Ok(graph_owned(&b, record, Some(serial)))
            }
            Err(e) => Output::unported(&e).ok_or_else(|| herr(e)),
        }
    }
}

#[cfg(test)]
impl super::topology_brep_p5::transform_certificate::Certified for Runner {
    fn run_certified(
        &self,
        case: &Case,
        log: &mut CertificateLog,
    ) -> Result<Output, HarnessError> {
        Self::run(case, log)
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_mirror"
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        Self::run(
            case,
            #[cfg(test)]
            &mut CertificateLog::default(),
        )
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let file = FILES.get(rng.below(FILES.len() as u64) as usize)?;
        let mut i = Inputs {
            file: (*file).into(),
            ..Inputs::default()
        };
        let scale = 10f64.powi(rng.range_i64(-1, 2) as i32);
        if rng.below(3) == 0 {
            // a plane perpendicular to a coordinate axis (Z of X x Y)
            {
                let at = rng.uniform(-10., 10.) * scale;
                match rng.below(3) {
                    0 => {
                        i.origin = [at, 0., 0.];
                        i.x = [0., 1., 0.];
                        i.y = [0., 0., 1.];
                    }
                    1 => {
                        i.origin = [0., at, 0.];
                        i.x = [0., 0., 1.];
                        i.y = [1., 0., 0.];
                    }
                    _ => i.origin = [0., 0., at],
                }
            }
        } else {
            // an arbitrary plane
            {
                let mut dir = || {
                    [
                        rng.uniform(-1., 1.),
                        rng.uniform(-1., 1.),
                        rng.uniform(-1., 1.),
                    ]
                };
                i.x = dir();
                i.y = dir();
                i.origin = [
                    rng.uniform(-10., 10.) * scale,
                    rng.uniform(-10., 10.) * scale,
                    rng.uniform(-10., 10.) * scale,
                ];
            }
        }
        i.editing = rng.chance(0.9);
        serde_json::to_value(i).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut ctx = ctx.clone();
        ctx.seeds = ctx.seeds.max(128);
        let mut cases = crate::runner::default_cases(self, &ctx)?;
        for n in 0..MATRIX_GENERATED {
            let seed = CaseContext::seed_for("topology.brep_mirror.transform_matrix", n);
            let mut rng = Rng::new(seed);
            let Some(inputs) = generate_matrix(&mut rng) else {
                break;
            };
            cases.push(Case {
                unit: self.id().to_string(),
                name: format!("mat{n:04}"),
                source: crate::case::CaseSource::Generated { seed },
                inputs,
            });
        }
        Ok(cases)
    }
}

/// A matrix-transform case: a generated 3x3 block (rotation times a scale of one of
/// the kinds below, sometimes sheared) and translation, on one of [`FILES`].
fn generate_matrix(rng: &mut Rng) -> Option<Value> {
    let file = FILES.get(rng.below(FILES.len() as u64) as usize)?;
    let (a, c) = (rng.uniform(-3.2, 3.2), rng.uniform(-3.2, 3.2));
    let (ca, sa, cc, sc) = (a.cos(), a.sin(), c.cos(), c.sin());
    // a rotation about Z then X
    let rot = [[ca, sa, 0.], [-sa * cc, ca * cc, sc], [sa * sc, -ca * sc, cc]];
    let d = match rng.below(9) {
        0 => [1., 1., 1.],
        1 => {
            let s = rng.uniform(0.2, 5.0);
            [s, s, s]
        }
        2 | 3 => [
            rng.uniform(0.2, 5.0),
            rng.uniform(0.2, 5.0),
            rng.uniform(0.2, 5.0),
        ],
        4 => [-1., 1., 1.],
        5 => [-rng.uniform(0.2, 5.0), rng.uniform(0.2, 5.0), 1.],
        6 => [1., 1., 10f64.powi(-(rng.range_i64(3, 10) as i32))],
        7 => {
            let s = 10f64.powi(rng.range_i64(-6, 6) as i32);
            [s, s, s]
        }
        _ => [1., 1., 0.],
    };
    let mut m = [[0.; 4]; 4];
    for ((out, row), dr) in m.iter_mut().zip(rot).zip(d) {
        for (o, x) in out.iter_mut().zip(row) {
            *o = x * dr;
        }
    }
    if rng.chance(0.4) {
        // a shear term
        let (r, k) = (rng.below(3) as usize, rng.below(3) as usize);
        let shear = rng.uniform(-1.0, 1.0);
        if let Some(x) = m.get_mut(r).and_then(|row| row.get_mut(k)) {
            *x += shear;
        }
    }
    let scale = 10f64.powi(rng.range_i64(-1, 2) as i32);
    let t = [
        rng.uniform(-10., 10.) * scale,
        rng.uniform(-10., 10.) * scale,
        rng.uniform(-10., 10.) * scale,
        1.,
    ];
    if let Some(row) = m.get_mut(3) {
        *row = t;
    }
    let i = Inputs {
        file: (*file).into(),
        matrix: Some(m),
        scale_tol: rng.chance(0.8),
        editing: rng.chance(0.9),
        ..Inputs::default()
    };
    serde_json::to_value(i).ok()
}

/// `Face::shell` and `Shell::faces` have no upstream counterpart: the import derives them
/// from the first face use in walk order, the port keeps them with the face's first
/// face use (`swap_reflection_shells`). Both are recomputed here from the face's own
/// first face use (`m_pFaceuse`, exported and compared), so the record the graph
/// compares is the same on both sides; the face uses' own shells are compared as they are.
pub(super) fn normalize_face_shells(b: &mut Brep) -> SolidResult<()> {
    // the faces in the graph's walk order (the import numbers them so), then the rest
    let mut faces: Vec<_> = super::boolean_merge_p1::Canon::reachable(b).faces.items;
    for f in b.faces().ids() {
        if !faces.contains(&f) {
            faces.push(f);
        }
    }
    let mut shell_of = Vec::new();
    for &f in &faces {
        let face = b.face(f)?;
        let shell = match face.faceuse {
            Some(fu) => b.faceuse(fu)?.shell.unwrap_or(face.shell),
            None => face.shell,
        };
        shell_of.push(shell);
    }
    for (&f, &shell) in faces.iter().zip(&shell_of) {
        b.face_mut(f)?.shell = shell;
    }
    let shells: Vec<_> = b.shells().ids().collect();
    for s in shells {
        b.shell_mut(s)?.faces = faces
            .iter()
            .zip(&shell_of)
            .filter(|(_, &sh)| sh == s)
            .map(|(&f, _)| f)
            .collect();
    }
    Ok(())
}

/// One step of `SmBrep::Transform(const double[4][4])`, as `transform_matrix` makes it.
#[cfg(test)]
enum Step {
    /// `SmBrep::Transform(placement, scale, bScaleTol)`.
    Transform(Axis2Placement, Option<Vec3>, bool),
    /// `SmBrep::Mirror(plane)`.
    Mirror(Axis2Placement),
}

/// The steps `transform_matrix` takes for the matrix `m`, built as it builds them:
/// one scaled transform when the rows are orthogonal and right-handed, else the SVD's
/// `V` transform, a mirror when `V` is left-handed, then the `U^T` transform scaled by
/// the singular values, and, when `U^T` is left-handed, a mirror and the translation.
/// `None` when the matrix does not decompose (the transform then fails).
#[cfg(test)]
fn matrix_steps(m: &[[f64; 4]; 4], scale_tol: bool) -> SolidResult<Option<Vec<Step>>> {
    use solidcore::geom::matrix::Matrix;
    use solidcore::math::{EFF_ZERO, EFF_ZERO_RAD, HALF_PI};
    let [m0, m1, m2, m3] = m;
    let row0 = Vec3::new(m0[0], m0[1], m0[2]);
    let row1 = Vec3::new(m1[0], m1[1], m1[2]);
    let row2 = Vec3::new(m2[0], m2[1], m2[2]);
    let disp = Vec3::new(m3[0], m3[1], m3[2]);
    // (a failed `AngleBetween` reports 0, as `transform_matrix` takes it by default)
    if row0.dot(row1).abs() < EFF_ZERO
        && row0.cross(row1).angle_between(row2).unwrap_or(0.0) <= EFF_ZERO_RAD
    {
        let (l0, l1, l2) = (row0.length(), row1.length(), row2.length());
        let placement = Axis2Placement::new(disp, row0 / l0, row1 / l1);
        return Ok(Some(vec![Step::Transform(
            placement,
            Some(Vec3::new(l0, l1, l2)),
            scale_tol,
        )]));
    }
    let mut sv = Matrix::from_row_major(
        3,
        3,
        vec![
            row0.x, row0.y, row0.z, row1.x, row1.y, row1.z, row2.x, row2.y, row2.z,
        ],
    )?;
    let Ok((d, mut ut)) = sv.sv_decompose() else {
        return Ok(None);
    };
    ut.transpose()?;
    let vec = |m: &Matrix, r: usize| match m.row(r) {
        Some([x, y, z]) => Vec3::new(*x, *y, *z),
        _ => Vec3::new(f64::NAN, f64::NAN, f64::NAN),
    };
    let dv = |i: usize| d.get(i).copied().unwrap_or(f64::NAN);
    let (vx, vy, vz) = (vec(&sv, 0), vec(&sv, 1), vec(&sv, 2));
    let (utx, uty, utz) = (vec(&ut, 0), vec(&ut, 1), vec(&ut, 2));
    let mut transform_v = Axis2Placement::IDENTITY;
    // (`transform_matrix` ignores `SetCanonical`'s status)
    let _ = transform_v.set_canonical(Vec3::ZERO, vx / vx.length(), vy / vy.length());
    let scale_v = Vec3::new(vx.length(), vy.length(), vz.length());
    let scale_ut = Vec3::new(
        utx.length() * dv(0),
        uty.length() * dv(1),
        utz.length() * dv(2),
    );
    let mirror_v = vx.cross(vy).angle_between(vz).unwrap_or(0.0) > HALF_PI;
    let mirror_ut = utx.cross(uty).angle_between(utz).unwrap_or(0.0) > HALF_PI;
    let mut steps = vec![Step::Transform(transform_v, Some(scale_v), scale_tol)];
    if mirror_v {
        steps.push(Step::Mirror(transform_v));
    }
    let mut transform_ut = Axis2Placement::IDENTITY;
    if mirror_ut {
        let _ = transform_ut.set_canonical(Vec3::ZERO, utx / utx.length(), uty / uty.length());
        steps.push(Step::Transform(transform_ut, Some(scale_ut), scale_tol));
        steps.push(Step::Mirror(transform_ut));
        let _ = transform_ut.set_canonical(disp, Vec3::X, Vec3::Y);
        steps.push(Step::Transform(transform_ut, None, true));
    } else {
        let _ = transform_ut.set_canonical(disp, utx / utx.length(), uty / uty.length());
        steps.push(Step::Transform(transform_ut, Some(scale_ut), scale_tol));
    }
    Ok(Some(steps))
}

/// Replays `transform_matrix`'s steps on a copy of its input, putting each `Transform`
/// step through the WR-507 / WR-508 tolerance certificate
/// (`topology_brep_p5::transform_certificate`), and panics unless the replay ends with
/// the transformed body's tolerances and model size estimate, bit for bit. The mirrors
/// between the steps are flag-independent and may change vertex tolerances themselves.
#[cfg(test)]
fn replay_certified(
    k: &mut Kernel,
    mut copy: Brep,
    result: &Brep,
    m: &[[f64; 4]; 4],
    scale_tol: bool,
    log: &mut CertificateLog,
) -> SolidResult<()> {
    use super::topology_brep_p5::transform_certificate::Snapshot;
    let Some(steps) = matrix_steps(m, scale_tol)? else {
        panic!("brep_mirror: a successful transform's matrix did not decompose");
    };
    for step in &steps {
        match step {
            Step::Transform(placement, scale, scale_tol) => {
                let before = Snapshot::before_transform(k, &copy, *scale)?;
                solidcore::topology::brep::p5::transform(k, &mut copy, placement, *scale, *scale_tol)?;
                before.check(k, &mut copy, *scale, *scale_tol, "brep_mirror step", log)?;
            }
            Step::Mirror(plane) => mirror(k, &mut copy, plane)?,
        }
    }
    let (replayed, got) = (Snapshot::of(k, &copy)?, Snapshot::of(k, result)?);
    assert!(
        replayed.unchanged(&got),
        "brep_mirror: the step replay does not reproduce the transform's tolerances"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};

    /// The rows WR-507 / WR-508 moved whose decomposition has a scaled `Transform` step.
    const SCALED: [&str; 22] = [
        "mat0009",
        "mat0010",
        "mat0014",
        "mat0017",
        "mat0019",
        "mat0026",
        "mat0027",
        "mat0032",
        "mat0033",
        "mat0042",
        "mat0043",
        "mat0044",
        "mat0048",
        "mat0058",
        "mat0068",
        "mat0069",
        "mat0074",
        "mat0077",
        "mat0078",
        "mat0080",
        "mat0083",
        "mat0090",
    ];
    /// The rows WR-507 / WR-508 moved whose steps all have a scale of 1: no tolerance
    /// changes and the model size estimate stays unset (upstream set it from the vertex box).
    const UNIT_SCALE: [&str; 3] = [
        "mat0041",
        "mat0059",
        "mat0091",
    ];

    /// Every case replays, and each 4x4 transform goes through the WR-507 / WR-508
    /// tolerance certificate (`topology_brep_p5::transform_certificate`): these are all
    /// of this unit's rows that moved when those flags were retired (the mirrors move
    /// none).
    #[test]
    fn every_transform_meets_the_wr507_wr508_certificate() {
        // Every case reads an upstream file: without the checkout, skip as
        // `successful_near_singular_transform_is_valid_or_known` does.
        let case = crate::case::regression_case(&Runner, SCALED[0]);
        let input: Inputs = case.parse().unwrap();
        if let Err(error) = path(&input) {
            assert!(
                std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
                "required transform fixture is unavailable: {error}"
            );
            return;
        }
        let fired = super::super::topology_brep_p5::transform_certificate::replay_matches_golden(
            &Runner,
        );
        for c in SCALED {
            let f = fired.get(c).unwrap_or_else(|| panic!("moved row {c} is not a case"));
            assert!(f.scaled > 0, "{c}: {f:?}");
        }
        for c in UNIT_SCALE {
            let f = fired.get(c).unwrap_or_else(|| panic!("moved row {c} is not a case"));
            assert!(f.unscaled > 0 && f.scaled == 0, "{c}: {f:?}");
        }
    }

    #[test]
    fn successful_near_singular_transform_is_valid_or_known() {
        let case = crate::case::regression_case(&Runner, "mat0076");
        let input: Inputs = case.parse().unwrap();
        let file = match path(&input) {
            Ok(file) => file,
            Err(error) => {
                assert!(
                    std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
                    "required transform fixture is unavailable: {error}"
                );
                return;
            }
        };
        let read = || {
            let mut kernel = Kernel::default();
            let body = crate::own_reader::read_file_rebuild(&mut kernel, &file, input.rebuild)
                .unwrap()
                .unwrap();
            (kernel, body)
        };
        // Check the input on an independent read, preserving the operation's evaluation order.
        let (mut input_kernel, mut original) = read();
        let input_check = check_brep(&mut input_kernel, &mut original, CheckLevel::Level2).unwrap();
        assert!(input_check.is_valid(), "the input must be valid: {input_check:?}");

        let (mut kernel, mut body) = read();
        body.set_editing_enabled(input.editing);
        let transformed = transform_matrix(
            &mut kernel,
            &mut body,
            input.matrix.as_ref().unwrap(),
            input.scale_tol,
        );
        let validity = if transformed.is_ok() {
            check_brep(&mut kernel, &mut body, CheckLevel::Level2).map(|check| check.is_valid())
        } else {
            Ok(true) // A refusal does not claim a valid transformed body.
        };
        let invalid_success = !matches!(validity, Ok(true));
        let known = crate::golden::known_failing(&crate::golden::default_dir(), Runner.id()).unwrap();
        assert_eq!(
            invalid_success,
            known.contains_key(&case.name),
            "a successful transform of a valid input must remain valid or be tracked as known wrong; \
             a repaired case must leave that classification: result={transformed:?}, validity={validity:?}"
        );
    }
}
