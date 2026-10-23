//! Differential runner for the part-5 `SmCurveClassification` operations.
//! Real lines, circles and stationary B-splines; complete interval and solution
//! state is compared. Topology-handle and part-6 gaps are explicit deferrals.
//!
//! Cases with a `topo` input run on real B-reps instead (module `topo`): an edge target
//! of `sm_InsertSolutions` and the vertices of `IntersectVertices`, including the edits
//! both make (vertex tolerances, the classification curve, edge-use UV trim curves).

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solidcore::curve::curve::p2::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::solution_array::{Solution, SolutionType};
use solidcore::intersect::curve_class::p5::{insert_solutions, ExtractOptions};
use solidcore::intersect::curve_class::{CurveClassification, CurveInterval};
use solidcore::intersect::point_class::{
    ClassGeometry, ClassObject, ClassObjectKind, PointClassType, PointClassification,
};
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::surface::volume::Volume;
use solidcore::{Result as ScResult, SolidError};
use solidcore_ffi::units::intersect_curve_class_p5 as ffi;

#[path = "intersect_curve_class_p5/topo.rs"]
mod topo;

/// Differential runner for curve classification part 5.
pub struct Runner;

/// Generated B-rep cases per run, whatever `--seeds` says (it can only raise this).
const MIN_TOPO_GENERATED: usize = 300;

fn scale() -> f64 {
    1.0
}
fn zone() -> f64 {
    1e-6
}
fn none_tol() -> f64 {
    -1.0
}
fn yes() -> bool {
    true
}
fn bounds() -> Vec<f64> {
    vec![0., 1.]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // Independent native flags are varied separately.
struct Input {
    op: usize,
    #[serde(default)]
    shape: usize,
    #[serde(default)]
    dim2: bool,
    #[serde(default)]
    ignore: bool,
    #[serde(default)]
    flag1: bool,
    #[serde(default)]
    flag2: bool,
    #[serde(default = "yes")]
    flag3: bool,
    #[serde(default)]
    target: usize,
    #[serde(default)]
    index: usize,
    #[serde(default = "scale")]
    scale: f64,
    #[serde(default = "zone")]
    tol: f64,
    #[serde(default)]
    angle: f64,
    #[serde(default = "bounds")]
    bounds: Vec<f64>,
    #[serde(default)]
    other_bounds: Option<Vec<f64>>,
    #[serde(default)]
    classes: Vec<i32>,
    #[serde(default)]
    other_classes: Vec<i32>,
    #[serde(default)]
    solutions: Vec<[f64; 7]>,
    #[serde(default = "none_tol")]
    xsect_tol: f64,
    /// A B-rep case (module `topo`); the fields above are then unused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    topo: Option<topo::TopoIn>,
}

impl Input {
    fn flat(&self) -> Vec<f64> {
        let n = self.bounds.len().saturating_sub(1);
        let other_bounds = self.other_bounds.as_ref().unwrap_or(&self.bounds);
        let other_n = other_bounds.len().saturating_sub(1);
        let mut out = vec![
            self.op as f64,
            self.shape as f64,
            if self.dim2 { 0. } else { 1. },
            f64::from(self.ignore),
            f64::from(self.flag1),
            f64::from(self.flag2),
            f64::from(self.flag3),
            self.target as f64,
            self.index as f64,
            self.scale,
            self.tol,
            self.angle,
            n as f64,
            self.solutions.len() as f64,
            self.xsect_tol,
            other_n as f64,
        ];
        out.extend(&self.bounds);
        out.extend(other_bounds);
        for (c, count) in [(&self.classes, n), (&self.other_classes, other_n)] {
            if c.is_empty() {
                out.extend(std::iter::repeat_n(100., 3 * count));
            } else {
                out.extend(c.iter().map(|x| f64::from(*x)));
            }
        }
        out.extend(self.solutions.iter().flatten());
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct H(usize, ClassObjectKind);
impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        self.1
    }
}
struct Pool {
    source: Curve,
    target: Curve,
    plane: Surface,
    vertex: Vec3,
    vertex_tol: f64,
    uv1: Curve,
    uv2: Curve,
}
impl ClassGeometry<H> for Pool {
    fn curve(&self, o: H) -> Option<&Curve> {
        if o.1 != ClassObjectKind::Curve {
            None
        } else if o.0 == 2 {
            Some(&self.uv1)
        } else if o.0 == 3 {
            Some(&self.uv2)
        } else if o.0 == 0 {
            Some(&self.source)
        } else {
            Some(&self.target)
        }
    }
    fn surface(&self, o: H) -> Option<&Surface> {
        if o.1 == ClassObjectKind::Surface {
            Some(&self.plane)
        } else {
            None
        }
    }
    fn volume(&self, _o: H) -> Option<&Volume> {
        None
    }
    fn point(&self, _o: H) -> Option<Vec3> {
        None
    }
    fn classification_vertex_point(&self, o: H) -> Option<Vec3> {
        if o.1 == ClassObjectKind::Vertex {
            Some(self.vertex)
        } else {
            None
        }
    }
    fn topology_zone_tol_3d(&self, o: H) -> Option<f64> {
        if o.1 == ClassObjectKind::Face {
            Some(0.)
        } else if o.1 == ClassObjectKind::Vertex {
            Some(self.vertex_tol)
        } else {
            None
        }
    }
}
fn target(i: &Input) -> H {
    H(
        1,
        match i.target {
            0 => ClassObjectKind::Curve,
            1 => ClassObjectKind::Surface,
            2 => ClassObjectKind::Vertex,
            _ => ClassObjectKind::Face,
        },
    )
}

fn setup(k: &Kernel, pool: &Pool, i: &Input, other: bool) -> ScResult<CurveClassification<H>> {
    let b = if other {
        i.other_bounds.as_ref().unwrap_or(&i.bounds)
    } else {
        &i.bounds
    };
    let classes = if other { &i.other_classes } else { &i.classes };
    let mut cc = CurveClassification::new(k);
    cc.set_curve(Some(H(0, ClassObjectKind::Curve)), false);
    cc.set_interval(&Extent1d::from_min_max(
        b.first()
            .copied()
            .ok_or_else(|| SolidError::sm_err("difftest::p5"))?,
        b.last()
            .copied()
            .ok_or_else(|| SolidError::sm_err("difftest::p5"))?,
    ))?;
    // Match the native fixture's literal SetZoneTol3d, including zero.
    cc.set_src_zone_tol_3d(i.tol);
    if i.op == 3 && ((!other && i.flag1) || (other && i.flag2)) {
        cc.set_uv_curve1(Some(H(if other { 3 } else { 2 }, ClassObjectKind::Curve)));
    }
    cc.set_ignore_tangency(i.ignore);
    cc.set_size(k, b.len() - 1)?;
    for (j, bounds) in b.windows(2).enumerate() {
        let mut ivl = CurveInterval::with_interval(
            k,
            &Extent1d::from_min_max(
                bounds
                    .first()
                    .copied()
                    .ok_or_else(|| SolidError::sm_err("difftest::p5"))?,
                bounds
                    .last()
                    .copied()
                    .ok_or_else(|| SolidError::sm_err("difftest::p5"))?,
            ),
            Some(&cc),
        );
        for (w, pc) in [&mut ivl.start, &mut ivl.mid, &mut ivl.end]
            .into_iter()
            .enumerate()
        {
            let class = PointClassType::from_code(classes.get(3 * j + w).copied().unwrap_or(100))
                .ok_or_else(|| {
                SolidError::invalid_input("difftest::p5", "bad classification")
            })?;
            if class != PointClassType::Unknown {
                pc.set_class_object(
                    class,
                    if class == PointClassType::Nothing {
                        None
                    } else {
                        Some(target(i))
                    },
                );
            }
        }
        *cc.at_mut(j)? = ivl;
    }
    let _ = pool;
    Ok(cc)
}

fn put_curve(k: &Kernel, out: &mut Vec<f64>, curve: &Curve) {
    let domain = curve.natural_interval();
    out.extend([
        curve.type_code() as f64,
        curve.dim() as f64,
        domain.min(),
        domain.max(),
    ]);
    for j in 0..=4 {
        match curve.evaluate_point(k, domain.evaluate(f64::from(j) / 4.0)) {
            Ok(p) => out.extend([0., p.x, p.y, p.z]),
            Err(e) => out.extend([status(&e), -f64::MAX, -f64::MAX, -f64::MAX]),
        }
    }
}
fn put_pc(out: &mut Vec<f64>, pc: &PointClassification<H>, target: H) {
    let uv = pc.uv_param();
    let uvw = pc.uvw_param();
    let sss = pc.sss_point();
    out.extend([
        f64::from(pc.point_class().code()),
        pc.object()
            .map_or(-1., |h| if h == target { 1. } else { -2. }),
        pc.deviation(),
        pc.t_param(),
        uv.x,
        uv.y,
        uvw.x,
        uvw.y,
        uvw.z,
        f64::from(pc.are_parameters_set()),
        f64::from(pc.sss_point_flag()),
        sss.x,
        sss.y,
        sss.z,
        pc.pre_snap_param(),
        pc.src_zone_tol_3d(),
    ]);
}
fn put_cc(out: &mut Vec<f64>, cc: &CurveClassification<H>, target: H) {
    out.extend([cc.size() as f64, cc.interval().min(), cc.interval().max()]);
    for ivl in cc.intervals() {
        out.extend([ivl.min(), ivl.max()]);
        for pc in [&ivl.start, &ivl.mid, &ivl.end] {
            put_pc(out, pc, target);
        }
    }
}
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        SolidError::InvalidInput { .. } => 1007.,
        _ => -1.,
    }
}

fn stationary_curve(dim: u64, scale: f64, angle: f64) -> ScResult<Curve> {
    let points = [0.75, 0.25, 0.75]
        .map(|x| CPoint::new(x * scale * angle.cos(), x * scale * angle.sin(), 0., 1.));
    Ok(Curve::from(BSplineCurve::from_data(
        dim,
        2,
        5,
        2,
        &points,
        &[0., 0., 0., 1., 1., 1.],
    )?))
}

fn rust(i: &Input) -> ScResult<Output> {
    let k = Kernel::default();
    let source = if i.shape == 2 || i.shape == 4 {
        stationary_curve(if i.dim2 { 2 } else { 3 }, i.scale, 0.)?
    } else if i.shape != 1 {
        Curve::from(Line::bounded(
            Vec3::ZERO,
            Vec3::new(1., 0., 0.),
            &Extent1d::from_min_max(0., 1.),
            &BoundedLineOptions {
                scale: i.scale,
                dim: if i.dim2 { 2 } else { 3 },
                ..BoundedLineOptions::default()
            },
        ))
    } else {
        Curve::from(Ellipse::new_circle(
            &k,
            Vec3::ZERO,
            Vec3::new(1., 0., 0.),
            Vec3::new(0., 1., 0.),
            Extent1d::from_min_max(0., 360.),
            i.scale,
            &EllipseOptions {
                dim: if i.dim2 { 2 } else { 3 },
                ..EllipseOptions::default()
            },
        )?)
    };
    let pool = Pool {
        source,
        target: if i.shape == 3 || i.shape == 4 {
            stationary_curve(if i.dim2 { 2 } else { 3 }, i.scale, i.angle)?
        } else {
            Curve::from(Line::bounded(
                Vec3::ZERO,
                Vec3::new(i.angle.cos(), i.angle.sin(), 0.),
                &Extent1d::from_min_max(0., 1.),
                &BoundedLineOptions {
                    scale: i.scale,
                    dim: if i.dim2 { 2 } else { 3 },
                    ..BoundedLineOptions::default()
                },
            ))
        },
        plane: Surface::from(Plane::from_normal(Vec3::ZERO, Vec3::new(0., 0., 1.))),
        vertex: Vec3::new(i.angle * i.scale, 0., 0.),
        uv1: Curve::from(Line::bounded(
            Vec3::ZERO,
            Vec3::new(1., 0., 0.),
            &Extent1d::from_min_max(0., 1.),
            &BoundedLineOptions {
                scale: i.scale,
                dim: 2,
                ..BoundedLineOptions::default()
            },
        )),
        uv2: Curve::from(Line::bounded(
            Vec3::ZERO,
            Vec3::new(0., 1., 0.),
            &Extent1d::from_min_max(0., 1.),
            &BoundedLineOptions {
                scale: i.scale,
                dim: 2,
                ..BoundedLineOptions::default()
            },
        )),
        vertex_tol: i.tol,
    };
    let mut cc = setup(&k, &pool, i, false)?;
    let mut other = setup(&k, &pool, i, true)?;
    let mut solutions = Vec::new();
    for &[typ, t0, t1, t2, t3, gap0, gap1] in &i.solutions {
        let mut sol = Solution::new();
        sol.solution_type = SolutionType::from_code(typ as i32)
            .ok_or_else(|| SolidError::invalid_input("difftest::p5", "bad solution type"))?;
        sol.num_objects = 2;
        sol.num_variables = if i.target == 3 { 3 } else { 2 };
        if let Some(slot) = sol.objects.get_mut(0) {
            *slot = Some(H(0, ClassObjectKind::Curve));
        }
        if let Some(slot) = sol.objects.get_mut(1) {
            *slot = Some(target(i));
        }
        if i.op == 1 && i.index == 1 {
            sol.objects.swap(0, 1);
        }
        *sol.start.param_mut(0)? = t0;
        *sol.start.param_mut(1)? = t1;
        *sol.end.param_mut(0)? = t2;
        *sol.end.param_mut(1)? = t3;
        sol.start.solution_value = gap0;
        sol.end.solution_value = gap1;
        solutions.push(sol);
    }
    let mut out = Vec::new();
    let result = match i.op {
        0 => insert_solutions(
            &k,
            &pool,
            &mut cc,
            &mut solutions,
            target(i),
            if i.xsect_tol < 0. {
                None
            } else {
                Some(i.xsect_tol)
            },
        ),
        1 => cc.insert_topology_intersections(&k, &pool, &solutions, i.index),
        2 => cc.fixup_classification(&k, &pool),
        3 => cc
            .extract_geometry(
                &k,
                &pool,
                &mut other,
                ExtractOptions {
                    uv_curves1: i.flag1,
                    uv_curves2: i.flag2,
                    points_3d: i.flag3,
                },
            )
            .map(|g| {
                out.extend([
                    g.curves_3d.len() as f64,
                    g.uv_curves1.len() as f64,
                    g.uv_curves2.len() as f64,
                    g.points_3d.len() as f64,
                ]);
                for c in g.curves_3d.iter().chain(&g.uv_curves1).chain(&g.uv_curves2) {
                    put_curve(&k, &mut out, c);
                }
                for p in &g.points_3d {
                    out.extend([p.x, p.y, p.z]);
                }
            }),
        4 => cc
            .merge_classifications(&k, &pool, &mut other, i.flag1)
            .map(|(a, b)| {
                out.extend([
                    a.faces.len() as f64,
                    a.edges.len() as f64,
                    a.vertices.len() as f64,
                    b.faces.len() as f64,
                    b.edges.len() as f64,
                    b.vertices.len() as f64,
                ]);
            }),
        5 => cc.merge_intervals(&k, &pool, i.flag1).map(|a| {
            out.extend([
                a.faces.len() as f64,
                a.edges.len() as f64,
                a.vertices.len() as f64,
                0.,
                0.,
                0.,
            ]);
        }),
        6 => cc.intersect_vertices(
            &k,
            &pool,
            if i.flag1 {
                &[H(1, ClassObjectKind::Vertex)]
            } else {
                &[]
            },
            &pool.plane,
            i.flag2,
            i.flag3,
        ),
        _ => return Err(SolidError::invalid_input("difftest::p5", "bad operation")),
    };
    let st = match result {
        Ok(()) => 0.,
        Err(e) => {
            if let Some(deferred) = Output::unported(&e) {
                return Ok(deferred);
            }
            status(&e)
        }
    };
    out.insert(0, st);
    put_cc(&mut out, &cc, target(i));
    put_cc(&mut out, &other, target(i));
    for s in solutions {
        out.extend([
            f64::from(s.solution_type.code()),
            s.start.param(0)?,
            s.start.param(1)?,
            s.end.param(0)?,
            s.end.param(1)?,
            s.start.solution_value,
            s.end.solution_value,
        ]);
    }
    Ok(Output::new().values("out", out))
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.curve_class_p5"
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let op = [0, 0, 0, 0, 0, 1, 1, 1, 3, 3, 6, 6, 2, 2, 4, 5]
                .get(rng.below(16) as usize)
                .copied()
                .unwrap_or(0);
            let n = 1 + rng.below(5) as usize;
            let target = if op == 6 {
                2
            } else if op == 3 && rng.below(2) == 0 {
                3
            } else {
                rng.below(2) as usize
            };
            let scale = [1e-6, 1., 1e6]
                .get(rng.below(3) as usize)
                .copied()
                .unwrap_or(1.);
            let mut i: Input =
                serde_json::from_value(json!({"op":op,"target":target,"scale":scale})).ok()?;
            i.shape = rng.below(2) as usize;
            i.dim2 = rng.below(4) == 0;
            i.ignore = rng.below(2) == 0;
            i.angle = if rng.below(2) == 0 {
                0.
            } else {
                rng.unit() * std::f64::consts::PI
            };
            i.bounds = (0..=n).map(|j| j as f64 / n as f64).collect();
            i.tol = [0., 1e-9, 1e-6, 0.01]
                .get(rng.below(4) as usize)
                .copied()
                .unwrap_or(1e-6);
            i.flag1 = rng.below(2) == 0;
            i.flag2 = rng.below(2) == 0;
            i.flag3 = rng.below(2) == 0;
            i.index = rng.below(2) as usize;
            if op == 2 {
                i.classes = (0..3 * n)
                    .map(|j| {
                        if j % 3 == 1 {
                            if target == 0 {
                                106
                            } else {
                                107
                            }
                        } else {
                            100
                        }
                    })
                    .collect();
            }
            if op == 3 && target == 3 {
                i.classes = vec![102; 3 * n];
                i.other_classes.clone_from(&i.classes);
            }
            if op == 6 {
                i.flag1 = true;
                i.flag2 = false;
                i.flag3 = true;
                i.angle = if i.shape == 0 { rng.unit() } else { 1.0 };
            }
            if op == 0 || op == 1 {
                i.xsect_tol = if rng.below(2) == 0 {
                    -1.
                } else {
                    rng.unit() * 0.01
                };
                for _ in 0..=rng.below(4) {
                    let a = rng.unit();
                    let b = if rng.below(2) == 0 {
                        a + 1e-10
                    } else {
                        rng.unit()
                    };
                    let typ = if rng.below(3) == 0 { 2. } else { 1. };
                    i.solutions.push([
                        typ,
                        a,
                        rng.unit(),
                        b.min(1.),
                        rng.unit(),
                        rng.unit() * 1e-7,
                        rng.unit() * 1e-7,
                    ]);
                }
            }
            if ffi::run(&i.flat()).is_ok() {
                return serde_json::to_value(i).ok();
            }
        }
        None
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let context = CaseContext {
            seeds: ctx.seeds.max(800),
            ..ctx.clone()
        };
        let mut cases = default_cases(self, &context)?;
        // the B-rep cases: their own seed stream, after the flat ones
        let topo_context = CaseContext {
            seeds: ctx.seeds.max(MIN_TOPO_GENERATED),
            ..ctx.clone()
        };
        let stream = format!("{}/topo", self.id());
        for mut case in topo_context.generated_cases(&stream, |rng| {
            let t = topo::generate(rng)?;
            Some(json!({"op": 7, "topo": t}))
        }) {
            case.unit = self.id().to_string();
            case.name = case.name.replacen("gen", "topo", 1);
            cases.push(case);
        }
        Ok(cases)
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Input = case.parse()?;
        if let Some(t) = &i.topo {
            return topo::cpp(t);
        }
        ffi::run(&i.flat())
            .map(|v| Output::new().values("out", v))
            .map_err(|e| HarnessError::new(e.to_string()))
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Input = case.parse()?;
        if let Some(t) = &i.topo {
            return topo::rust(t);
        }
        ffi::validate(&i.flat()).map_err(|e| HarnessError::new(e.to_string()))?;
        rust(&i).map_err(|e| HarnessError::new(e.to_string()))
    }
}
