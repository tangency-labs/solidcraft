use super::*;
use solidcore::curve::curve::p2::{CurveKind, CIRCLE_TYPE, ELLIPSE_TYPE};
use solidcore_ffi::units::topology_vertexuse as vu_ffi;

fn trim_bits(b: &Brep) -> Vec<Vec<u64>> {
    b.edgeuses()
        .iter()
        .map(|(_, eu)| {
            let Some(c) = &eu.uv_trim_curve else {
                return vec![];
            };
            let CurveKind::BSpline(base) = c.kind() else {
                panic!("trim changed native kind");
            };
            let n = base.gw_nurb().unwrap();
            let mut bits = vec![base.dim(), n.degree() as u64];
            bits.extend(n.knots().iter().map(|v| v.to_bits()));
            for p in n.cpts() {
                bits.extend([p.x, p.y, p.z, p.w].map(f64::to_bits));
            }
            bits
        })
        .collect()
}

// The reference kernel has global state: keep the native checks in one test,
// and run the C++ runner test suite with --test-threads=1.
#[test]
fn ellipse_trims_survive_both_shared_graph_exports() {
    let ctx = crate::CaseContext {
        root: crate::case::repo_root(),
        seeds: 0,
    };
    let fixtures = ctx.fixture_cases(Runner.id()).unwrap();
    let mut configurations = 0;
    for case in fixtures
        .iter()
        .filter(|c| c.name.starts_with("ellipse682_") && c.name.ends_with("_eu0_sample0.0"))
    {
        configurations += 1;
        let input: Inputs = case.parse().unwrap();
        let BrepIn::EllipseFace {
            circle,
            angles,
            natural,
            inside_out,
            oob,
            radii,
            ..
        } = input.brep.as_ref().unwrap()
        else {
            unreachable!()
        };
        let ty = if *circle { CIRCLE_TYPE } else { ELLIPSE_TYPE };
        let exported = export(&input).unwrap();
        let (b, types) = import_raw_with_types(&exported).unwrap();
        assert!(types.edge_exact.iter().all(|v| *v));
        let (id, edge) = b
            .edges()
            .iter()
            .find(|(_, e)| e.curve.as_ref().is_some_and(|c| c.type_code() == ty))
            .unwrap();
        assert_eq!(types.edge_curve_types[id.index() as usize], ty);
        assert_eq!(types.inexact_curve(id.index() as usize), None);
        let CurveKind::Ellipse(ellipse) = edge.curve.as_ref().unwrap().kind() else {
            unreachable!()
        };
        assert_eq!(ellipse.is_circle_class(), *circle);
        assert_eq!(
            ellipse.step_interval(),
            Extent1d::from_raw(angles[0], angles[1])
        );
        assert_eq!(
            ellipse.base().natural_interval(),
            Extent1d::from_raw(natural[0], natural[1])
        );
        assert_eq!(ellipse.inside_out(), *inside_out);
        assert_eq!(ellipse.base().is_out_of_bounds_enabled(), *oob);
        let block = brep_block(&input).unwrap();
        let other_export = vu_ffi::eval(vu_ffi::Op::Export, "", &block).unwrap();
        let (mut other, _) = super::super::topology_vertexuse::import_raw(&other_export).unwrap();
        assert_eq!(trim_bits(&b), trim_bits(&other));
        assert_eq!(
            other.edge(id).unwrap().curve.as_ref().unwrap().type_code(),
            ty
        );
        assert!(trim_bits(&b).iter().any(|v| !v.is_empty()));
        for eu in 0..b.edgeuses().len() {
            for fraction in [0.0, 0.125, 0.37, 0.5, 0.875, 1.0] {
                let op = OpIn::TrimSample {
                    eu: eu as i64,
                    fraction,
                };
                let sample_input = Inputs {
                    op: op.clone(),
                    ..input.clone()
                };
                let cpp = run_cpp(&sample_input).unwrap();
                let rust = run_rust(&mut Kernel::default(), &sample_input).unwrap();
                let second = run_rust_brep(&Kernel::default(), &mut other, &op)
                    .ok()
                    .unwrap();
                for actual in [&rust, &second] {
                    assert!(
                        actual.error.is_none() && actual.deferred.is_none(),
                        "{}: {actual:?}",
                        case.name
                    );
                    let diff = crate::compare(&cpp, actual, Runner.abs_floors());
                    assert!(
                        diff.pass() && diff.values_compared >= 14,
                        "{}: {diff:?}",
                        case.name
                    );
                    let values = &actual.values["sample"];
                    for j in 0..3 {
                        assert!(
                            (values[6 + j] - values[11 + j]).abs() <= 1e-10 * radii[0],
                            "edge and lifted trim differ: {values:?}"
                        );
                    }
                }
            }
        }
        // Removing only the exact edge record reproduces the old lossy import.
        // Existing UV trims remain bit-identical, but the curve must stay inexact.
        let head = exported
            .windows(2)
            .position(|w| w == [-3.0, ty as f64])
            .unwrap();
        let end = head + 6 + exported[head + 5] as usize;
        let mut old = exported.clone();
        old.drain(head..end);
        let (old_brep, old_types) = import_raw_with_types(&old).unwrap();
        assert_eq!(trim_bits(&b), trim_bits(&old_brep));
        assert_eq!(old_types.inexact_curve(id.index() as usize), Some(ty));
        assert!(matches!(
            old_brep.edge(id).unwrap().curve.as_ref().unwrap().kind(),
            CurveKind::BSpline(_)
        ));
        let eu = edge.edgeuses[0];
        assert!(inexact_geometry(
            &old_brep,
            &old_types,
            &OpIn::TrimSample {
                eu: i64::from(eu.index()),
                fraction: 0.37
            }
        )
        .is_some());
        if configurations == 1 {
            // Reject damaged record envelopes, including a nested exact head.
            for (slot, value) in [
                (head + 1, 14010.0),
                (head + 3, 2.0),
                (head + 4, 0.5),
                (head + 5, 1e9),
                (head + 6, 256.0),
                (end, -3.0),
                (end + 6, exported[end + 6] + 1.0),
            ] {
                let mut bad = exported.clone();
                bad[slot] = value;
                assert!(
                    import_raw_with_types(&bad).is_err(),
                    "accepted damaged slot {slot}"
                );
            }
            assert!(import_raw_with_types(&exported[..end - 1]).is_err());
        }
    }
    assert_eq!(configurations, 48);
}
