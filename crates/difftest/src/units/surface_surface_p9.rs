//! Differential runner for `surface.surface_p9`. Reference: SmSurface.cpp:11644,
//! 13887, 14877. A quadratic Bezier patch is used on both sides. CSI fixtures use
//! one planar patch and one line span, so both subdivision trees have one leaf.
//! Cylinder and cone cases (`analytic_cone` 1 and 2) take the domain as fractions
//! of the surface's natural domain.
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::Curve;
use solidcore::curve::curve_cache::{CurveCache, CurveCacheOptions};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::global_solver::{GlobalSolver, SolverOperation};
use solidcore::geom::pseudo_box::PseudoBox;
use solidcore::geom::tree::{
    AuxDataType, NodeClass, NodeData, NodeId, SplitDir, SurfaceAux, Tree, TreeNode, TreeOwner,
};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::nurbs::crv_basic::Curve as NlCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::surface::p1::CsiGlobalSolver;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::{
    BezierAux2d, BezierPatch, SurfaceCache, SurfaceCacheOptions,
};
use solidcore_ffi::units::surface_surface_p9 as ffi;

use solidcore::geom::solution_array::{ObjectKind, SolutionObject};

/// A solution object: `0` the curve, `1` the surface, which carries the natural UV
/// domain of the real surface (`SmSurface::GetNaturalUVDomain`, as the shim's
/// surface pointer reaches it).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Handle(u8, Option<Extent2d>);
impl SolutionObject for Handle {
    fn kind(&self) -> ObjectKind {
        if self.0 == 0 {
            ObjectKind::Curve
        } else {
            ObjectKind::Surface
        }
    }
    fn natural_interval(&self) -> Option<Extent1d> {
        (self.0 == 0).then(|| Extent1d::from_min_max(0.0, 1.0))
    }
    fn natural_uv_domain(&self) -> Option<Extent2d> {
        self.1
    }
}
/// Differential runner for the three owned surface operations.
pub struct Runner;
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: i32,
    #[serde(default)]
    analytic_plane: bool,
    #[serde(default)]
    analytic_revolution: bool,
    /// 1: the corpus cylinder, 2: a canonical cone, 3: a canonical sphere, 4: a
    /// canonical torus (shim kinds 3 to 6).
    #[serde(default)]
    analytic_cone: u8,
    net: [[f64; 3]; 9],
    #[serde(default = "domain")]
    domain: [f64; 4],
    direction: i32,
    controls: [f64; 3],
    #[serde(default)]
    diagonals: bool,
    #[serde(default = "line")]
    line: [[f64; 3]; 2],
}
fn domain() -> [f64; 4] {
    [0.0, 0.0, 1.0, 1.0]
}
fn line() -> [[f64; 3]; 2] {
    [[0.5, 0.5, -5.0], [0.5, 0.5, 5.0]]
}
fn harness(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}
fn surface(k: &Kernel, i: &Inputs) -> Result<Surface, HarnessError> {
    if i.analytic_cone == 1 {
        let mut cylinder = solidcore::surface::cone::Cone::new_cylinder_bounded(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            1.0,
            0.0,
            360.0,
            2.0,
            false,
            false,
            false,
        );
        cylinder.make_nurb(k).map_err(harness)?;
        return Ok(Surface::from(cylinder));
    }
    if i.analytic_cone == 2 {
        let placement = solidcore::geom::axis2_placement::Axis2Placement::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        let cone = solidcore::surface::cone::Cone::create_canonical(
            k, &placement, 0.5, 1.5, 2.0, false, false,
        )
        .map_err(harness)?;
        return Ok(Surface::from(cone));
    }
    if i.analytic_cone == 3 {
        let placement = solidcore::geom::axis2_placement::Axis2Placement::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        let sphere = solidcore::surface::sphere::Sphere::create_canonical(k, &placement, 1.5)
            .map_err(harness)?;
        return Ok(Surface::from(sphere));
    }
    if i.analytic_cone == 4 {
        let placement = solidcore::geom::axis2_placement::Axis2Placement::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        let torus = solidcore::surface::torus::Torus::create_canonical(
            k,
            &placement,
            2.0,
            0.5,
            solidcore::surface::torus::TorusOptions::default(),
            None,
        )
        .map_err(harness)?;
        return Ok(Surface::from(torus));
    }
    if i.analytic_revolution {
        let generator = Curve::from(BSplineCurve::from_nurb(
            NlCurve::new(
                vec![
                    CPoint::new(1.0, 0.0, 0.0, NOW),
                    CPoint::new(1.0, 0.0, 1.0, NOW),
                ],
                1,
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .map_err(harness)?,
            3,
            false,
        ));
        let mut revolution = solidcore::surface::surf_of_revolution::SurfOfRevolution::new(
            k,
            generator,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Extent2d::from_bounds(0.0, 0.0, 360.0, 1.0),
            false,
        )
        .map_err(harness)?;
        revolution.make_nurb(k).map_err(harness)?;
        return Ok(Surface::from(revolution));
    }
    if i.analytic_plane {
        return Ok(Surface::from(solidcore::surface::plane::Plane::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            solidcore::geom::vector2d::Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        )));
    }
    let net = i
        .net
        .chunks(3)
        .map(|row| {
            row.iter()
                .map(|p| CPoint::new(p[0], p[1], p[2], NOW))
                .collect()
        })
        .collect();
    let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let nurb = NlSurface::new(net, 2, 2, knots.clone(), knots).map_err(harness)?;
    Ok(Surface::from(BSplineSurface::from_nurb(nurb, false)))
}
fn output(flat: Vec<f64>) -> Output {
    let mut values = flat.into_iter();
    let status = values.next().unwrap_or(-1.0);
    Output::new()
        .scalar("status", status)
        .values("out", values.collect::<Vec<_>>())
}
fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let surface = surface(&k, i)?;
    let direction =
        SurfParamType::from_code(i.direction as u64).ok_or_else(|| harness("direction"))?;
    let mut domain = Extent2d::from_bounds(i.domain[0], i.domain[1], i.domain[2], i.domain[3]);
    if i.analytic_cone != 0 {
        let natural = surface.natural_uv_domain();
        domain = Extent2d::from_points(
            natural.evaluate(i.domain[0], i.domain[1]),
            natural.evaluate(i.domain[2], i.domain[3]),
        );
    }
    let result = match i.op {
        0 => surface
            .find_tessellation_splits(
                &k,
                direction,
                &domain,
                i.controls[0],
                i.controls[1],
                i.controls[2],
                i.diagonals,
            )
            .map(|v| {
                let mut out = vec![0.0];
                out.extend(v);
                out
            }),
        1 => surface
            .create_iso_boundaries(&k, direction, i.controls[0])
            .and_then(|curves| {
                let mut out = vec![0.0, if curves.is_some() { 2.0 } else { 0.0 }];
                if let Some((a, b)) = curves {
                    for curve in [a, b] {
                        for n in 0..=4 {
                            // (virtual EvaluatePoint of the iso curve's own kind)
                            let p = curve.evaluate_point(
                                &k,
                                curve.natural_interval().evaluate(f64::from(n) / 4.0),
                            )?;
                            out.extend([p.x, p.y, p.z]);
                        }
                    }
                }
                Ok(out)
            }),
        2 => {
            let points = i
                .line
                .iter()
                .map(|p| CPoint::new(p[0], p[1], p[2], NOW))
                .collect();
            let curve = Curve::from(BSplineCurve::from_nurb(
                NlCurve::new(points, 1, vec![0.0, 0.0, 1.0, 1.0]).map_err(harness)?,
                3,
                false,
            ));
            let interval = Extent1d::from_min_max(0.0, 1.0);
            let mut cc = CurveCache::<Handle>::new(
                curve.clone(),
                interval,
                CurveCacheOptions {
                    make_full_tree: true,
                    chord_height_tolerance: 0.01,
                    ..CurveCacheOptions::default()
                },
            );
            cc.set_curve_handle(Some(Handle(0, None)));
            cc.tessellate(&k)
                .map_err(|e| harness(format!("CSI fixture cache setup: {e}")))?;
            {
                let surface_handle = Handle(1, Some(surface.natural_uv_domain()));
                let mut cache = SurfaceCache::<Handle>::new(
                    surface,
                    Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
                    SurfaceCacheOptions::default(),
                );
                cache.set_surface_handle(Some(surface_handle));
                let uv = cache.uv_domain();
                let mut bbox = Extent3d::new();
                for p in i.net {
                    bbox = bbox.union(&Extent3d::from_points(
                        Vec3::new(p[0], p[1], p[2]),
                        Vec3::new(p[0], p[1], p[2]),
                    ));
                }
                cache.patches_mut().push(BezierPatch {
                    aux: BezierAux2d {
                        uv_domain: uv,
                        owning_tree_node: Some(NodeId(0)),
                        ..BezierAux2d::default()
                    },
                    pseudo_box: PseudoBox::from_extent3d(&bbox),
                    ..BezierPatch::default()
                });
                let mut tree = Tree::new(Some(TreeOwner::SurfaceCache));
                tree.add_node(TreeNode {
                    bbox,
                    aux_type: AuxDataType::BezierSurface,
                    data: NodeData::Surface(SurfaceAux {
                        split_dir: SplitDir::Unknown,
                        is_marked: false,
                        node_class: NodeClass::Unknown,
                        face: None,
                        surface: Some(surface_handle),
                        slot: Some(0),
                    }),
                    ..TreeNode::new()
                });
                tree.set_top_node(Some(NodeId(0)));
                cache.set_tree(Some(tree));
                let curve_tree = cc.tree().ok_or_else(|| harness("curve tree"))?;
                let tree = cache.tree().ok_or_else(|| harness("surface tree"))?;
                let mut base = GlobalSolver::with_trees(curve_tree, tree, 3);
                base.operation = SolverOperation::Intersect;
                base.tolerance_3d = 1e-6;
                let mut solver = CsiGlobalSolver::new(
                    &k, curve_tree,
                    tree,
                    Some(cache.surface()),
                    false,
                    interval,
                    domain,
                );
                solver.base = base;
                solver.curve = Some(&curve);
                solver.surface_cache = Some(&cache);
                solver.kernel = &k;
                solver
                    .local_solve_branch(&k, &[Some(NodeId(0)), Some(NodeId(0)), None])
                    .map(|more| {
                        let mut out = vec![
                            0.0,
                            f64::from(u8::from(more)),
                            solver.base.solutions.len() as f64,
                        ];
                        for solution in solver.base.solutions.as_slice() {
                            out.extend([
                                f64::from(solution.solution_type.code()),
                                solution.num_variables as f64,
                                solution.num_objects as f64,
                                solution.start.solution_value,
                                solution.end.solution_value,
                            ]);
                            out.extend(solution.start.parameters.iter().take(3));
                            out.extend(solution.end.parameters.iter().take(3));
                            out.extend(
                                solution
                                    .nodes
                                    .iter()
                                    .take(2)
                                    .map(|v| v.map_or(-1.0, |n| n.0 as f64)),
                            );
                            out.extend(
                                solution
                                    .objects
                                    .iter()
                                    .take(2)
                                    .map(|v| v.map_or(-1.0, |n| f64::from(n.0))),
                            );
                        }
                        out
                    })
            }
        }
        _ => return Err(harness("invalid op")),
    };
    Ok(match result {
        Ok(v) => output(v),
        Err(e) => Output::unported(&e).unwrap_or_else(|| {
            let status = match e {
                solidcore::SolidError::Upstream { code, .. } => f64::from(code),
                _ => 1007.0,
            };
            output(vec![status])
        }),
    })
}
impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_p9"
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(600),
            ..ctx.clone()
        };
        crate::runner::default_cases(self, &ctx)
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        ffi::eval_surface_kind(
            i.op,
            if i.analytic_cone != 0 {
                2 + i32::from(i.analytic_cone)
            } else if i.analytic_revolution {
                2
            } else {
                i32::from(i.analytic_plane)
            },
            &i.net,
            i.domain,
            i.direction,
            i.controls,
            i.diagonals,
            i.line,
        )
        .map_err(harness)
        .and_then(|v| {
            if i.op == 2 && v.first().copied() != Some(0.0) {
                return Err(harness("CSI reference fixture must solve successfully"));
            }
            Ok(output(v))
        })
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse()?)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let mut net = [[0.0; 3]; 9];
        let scale = rng.uniform(0.5, 3.0);
        let height = rng.uniform(0.0, 1.5);
        for (index, p) in net.iter_mut().enumerate() {
            *p = [
                (index / 3) as f64 * scale,
                (index % 3) as f64 * scale,
                rng.uniform(-height, height),
            ];
        }
        if rng.chance(0.15) {
            for p in net.iter_mut().take(3) {
                *p = [0.0, 0.0, 0.0];
            }
        }
        if rng.chance(0.1) {
            for p in &mut net {
                for v in p {
                    *v *= 1e-7;
                }
            }
        }
        let domain = if rng.chance(0.25) {
            domain()
        } else {
            [
                rng.uniform(0.0, 0.2),
                rng.uniform(0.0, 0.2),
                rng.uniform(0.8, 1.0),
                rng.uniform(0.8, 1.0),
            ]
        };
        let direction = rng.below(2) as i32;
        let (op, controls, diagonals) = if rng.chance(0.25) {
            (1, [0.01, 0.0, 0.0], false)
        } else {
            let mode = rng.below(6);
            (
                0,
                [
                    if mode == 0 || mode == 5 {
                        0.0
                    } else {
                        rng.uniform(0.02, 0.2)
                    },
                    if mode < 2 {
                        0.0
                    } else {
                        rng.uniform(5.0, 30.0)
                    },
                    if mode == 3 {
                        rng.uniform(0.5, 2.0)
                    } else if mode == 4 || mode == 5 {
                        -rng.uniform(0.5, 2.0)
                    } else {
                        0.0
                    },
                ],
                rng.chance(0.5),
            )
        };
        let analytic_plane = rng.chance(0.1);
        // (drawn last: the earlier draws, and so the other cases, are unchanged)
        let analytic_cone = if rng.chance(0.3) {
            1 + rng.below(2) as u8
        } else {
            0
        };
        serde_json::to_value(Inputs {
            op,
            analytic_plane,
            analytic_revolution: false,
            analytic_cone,
            net,
            direction,
            domain,
            controls,
            // Preserve historical generated inputs; real cylinder/cone diagonals
            // are covered by the named analytic-diagonal regression fixtures.
            diagonals: diagonals && analytic_cone == 0,
            line: line(),
        })
        .ok()
    }
}

#[cfg(all(test, feature = "cpp"))]
mod tests {
    use super::*;

    const NAMES: [&str; 4] = [
        "analytic_diagonal_cylinder_u_disabled",
        "analytic_diagonal_cylinder_u_enabled",
        "analytic_diagonal_cone_u_disabled",
        "analytic_diagonal_cone_u_enabled",
    ];

    fn context() -> CaseContext {
        CaseContext {
            root: crate::case::repo_root(),
            seeds: 0,
        }
    }

    fn fixtures() -> Result<Vec<Case>, HarnessError> {
        let cases = context().fixture_cases(Runner.id())?;
        let selected: Vec<_> = cases
            .iter()
            .filter(|case| {
                matches!(&case.source, crate::case::CaseSource::Fixture { path, .. }
                    if path == "fixtures/surface.surface_p9/analytic-diagonals.json")
            })
            .collect();
        assert_eq!(selected.len(), NAMES.len());
        NAMES
            .iter()
            .map(|name| {
                let matching: Vec<_> = cases.iter().filter(|case| case.name == *name).collect();
                assert_eq!(matching.len(), 1, "{name}");
                let case = matching.first().ok_or_else(|| harness(name))?;
                assert!(selected.contains(case), "{name}");
                Ok((*case).clone())
            })
            .collect()
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= crate::REL_TOL * actual.abs().max(expected.abs()),
            "{actual} != {expected}"
        );
    }

    fn assert_splits(case: &Case, output: &Output) -> Result<(), HarnessError> {
        assert!(output.error.is_none(), "{}: {output:?}", case.name);
        assert!(output.deferred.is_none(), "{}: {output:?}", case.name);
        assert!(output.rejected.is_none(), "{}: {output:?}", case.name);
        assert_eq!(
            output.values.get("status"),
            Some(&vec![0.0]),
            "{}",
            case.name
        );
        let inputs: Inputs = case.parse()?;
        let splits = output.values.get("out").ok_or_else(|| harness("out"))?;
        assert_eq!(splits.len(), if inputs.diagonals { 9 } else { 2 });
        assert!(splits
            .iter()
            .all(|v| v.is_finite() && (0.4..=1.6).contains(v)));
        assert!(splits
            .windows(2)
            .all(|pair| matches!(pair, [a, b] if a < b)));
        assert_close(*splits.first().ok_or_else(|| harness("first split"))?, 0.4);
        assert_close(*splits.last().ok_or_else(|| harness("last split"))?, 1.6);
        // Shape expectations for these regressions, not a replacement native oracle.
        if inputs.diagonals {
            for (actual, expected) in splits
                .iter()
                .zip([0.4, 0.55, 0.7, 0.85, 1.0, 1.15, 1.3, 1.45, 1.6])
            {
                assert_close(*actual, expected);
            }
        }
        Ok(())
    }

    #[test]
    fn analytic_diagonal_fixtures_are_discovered_and_paired() {
        let cases = fixtures().unwrap();
        let discovered = Runner.cases(&context()).unwrap();
        for (index, case) in cases.iter().enumerate() {
            assert_eq!(discovered.iter().filter(|c| c.name == case.name).count(), 1);
            let inputs: Inputs = case.parse().unwrap();
            assert_eq!(inputs.op, 0);
            assert!(!inputs.analytic_plane && !inputs.analytic_revolution);
            assert_eq!(inputs.analytic_cone, if index < 2 { 1 } else { 2 });
            assert_eq!(inputs.direction, 0);
            assert_eq!(inputs.domain, [0.125, 0.2, 0.375, 0.8]);
            assert_eq!(inputs.controls, [0.05, 15.0, 0.0]);
            assert_eq!(inputs.diagonals, index % 2 == 1);
            assert_eq!(inputs.line, line());
            for (index, point) in inputs.net.iter().enumerate() {
                assert_eq!(
                    *point,
                    [(index / 3) as f64 * 0.5, (index % 3) as f64 * 0.5, 0.0]
                );
            }
        }
        for pair in cases.as_chunks::<2>().0 {
            let mut disabled = pair[0].inputs.clone();
            let mut enabled = pair[1].inputs.clone();
            disabled.as_object_mut().unwrap().remove("diagonals");
            enabled.as_object_mut().unwrap().remove("diagonals");
            assert_eq!(disabled, enabled);
        }
    }

    #[test]
    fn analytic_diagonal_controls_have_meaningful_splits() {
        for pair in fixtures().unwrap().as_chunks::<2>().0 {
            let disabled = Runner.run_rust(&pair[0]).unwrap();
            let enabled = Runner.run_rust(&pair[1]).unwrap();
            assert_splits(&pair[0], &disabled).unwrap();
            assert_splits(&pair[1], &enabled).unwrap();
            assert_ne!(disabled.values["out"], enabled.values["out"]);
        }
    }

    #[test]
    fn analytic_diagonal_native_outputs_match_complete_arrays() {
        assert!(Runner.abs_floors().is_empty());
        for pair in fixtures().unwrap().as_chunks::<2>().0 {
            let mut native = Vec::new();
            let mut rust = Vec::new();
            for case in pair {
                let cpp = Runner.run_cpp(case).unwrap();
                let rs = Runner.run_rust(case).unwrap();
                assert_splits(case, &cpp).unwrap();
                assert_splits(case, &rs).unwrap();
                let diff = crate::compare(&cpp, &rs, Runner.abs_floors());
                assert!(diff.pass(), "{}: {diff:?}", case.name);
                assert_eq!(diff.values_compared, cpp.values["out"].len() + 1);
                if case.parse::<Inputs>().unwrap().diagonals {
                    let mut perturbed = rs.clone();
                    perturbed.values.get_mut("out").unwrap()[1] *= 1.01;
                    let negative = crate::compare(&cpp, &perturbed, Runner.abs_floors());
                    assert!(!negative.pass(), "{}: interior change missed", case.name);
                    assert_eq!(negative.mismatch_count, 1);
                }
                native.push(cpp);
                rust.push(rs);
            }
            assert_ne!(native[0].values["out"], native[1].values["out"]);
            assert_ne!(rust[0].values["out"], rust[1].values["out"]);
        }
    }
}
