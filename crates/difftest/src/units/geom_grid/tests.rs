//! Canonical-CI tests for explicit native cache coverage and observation sensitivity.
//! These are intentionally not a replacement for the unchanged 615-case corpus.

use super::*;
use crate::case::{repo_root, CaseSource};
use crate::compare::compare;

const NEW_FIXTURES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/geom.grid/native_cache.json"
));
const LEGACY_FIXTURES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/geom.grid/cases.json"
));

fn fixture(doc: &str, name: &str) -> Case {
    let v: Value = serde_json::from_str(doc).unwrap();
    let c = v["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap();
    Case {
        unit: "geom.grid".into(),
        name: name.into(),
        source: CaseSource::Inline,
        inputs: c["inputs"].clone(),
    }
}

fn output((ints, floats): (Vec<i64>, Vec<f64>)) -> Output {
    Output::new().ints("ints", ints).values("floats", floats)
}

#[test]
fn corpus_keeps_615_legacy_inputs_and_adds_six_explicit_cases() {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: 16,
    };
    let cases = Runner.cases(&ctx).unwrap();
    assert_eq!(cases.len(), 621);
    let old: Value = serde_json::from_str(LEGACY_FIXTURES).unwrap();
    let old = old["cases"].as_array().unwrap();
    assert_eq!(old.len(), 15);
    for c in old {
        let actual = cases
            .iter()
            .find(|x| x.name == c["name"].as_str().unwrap())
            .unwrap();
        assert_eq!(actual.inputs, c["inputs"]);
    }
    for n in 0..MIN_GENERATED {
        let seed = CaseContext::seed_for("geom.grid", n);
        let mut rng = Rng::new(seed);
        let expected = serde_json::to_value(gen_case(&mut rng)).unwrap();
        let name = format!("gen{n:04}");
        let actual = cases.iter().find(|c| c.name == name).unwrap();
        assert_eq!(actual.inputs, expected);
        assert!(!actual.inputs["ops"]
            .as_array()
            .unwrap()
            .iter()
            .any(|op| op["op"] == "define_surface"));
    }
}

/// The two legacy fixtures that used to wait for `smsurf_GetSurfaceCache` and
/// `SmBezierPatch::GetPseudoBox` now define a real surface: both sides build its
/// cache, expand it or add its nodes, and the observation compares the leaf records.
#[test]
fn the_formerly_deferred_fixtures_run_real_surface_caches() {
    for name in [
        "raytracer_surface_hierarchy_expand",
        "surface_leaf_element",
    ] {
        let case = fixture(LEGACY_FIXTURES, name);
        let i: Inputs = case.parse().unwrap();
        assert!(i
            .ops
            .iter()
            .any(|op| matches!(op, Op::DefineSurface { .. })));
        let cpp = Runner.run_cpp(&case).unwrap();
        let rust = output(run_rust(&i, &mut Stats::default()).unwrap());
        let r = compare(&cpp, &rust, &[]);
        assert!(r.pass(), "{name}: {r:?}");
        // at least one 51-value leaf record, and an element in the pool
        assert!(r.values_compared > 51, "{name}: {r:?}");
    }
}

/// An opaque surface label (`add_hier { surface: true }`) can be dumped but not
/// expanded, even next to an explicit surface of the same numeric label.
#[test]
fn explicit_definition_does_not_resolve_an_opaque_label() {
    let explicit: Inputs = fixture(NEW_FIXTURES, "native_affine_hierarchy_single")
        .parse()
        .unwrap();
    let mut i = explicit.clone();
    i.ops = vec![
        explicit.ops[0].clone(), // defines surface 7
        Op::AddHier {
            surface: true,
            obj: 7, // the same numeric label, the opaque input path
            tol: 0.0,
            bbox: [0.0, 0.0, 0.0, 1.0, 1.0, 0.1],
        },
        Op::Expand { h: 0 },
    ];
    assert!(matches!(
        run_rust(&i, &mut Stats::default()),
        Err(SolidError::InvalidInput { .. })
    ));
}

#[test]
fn explicit_guard_rejects_missing_owners_and_degenerate_geometry() {
    let good: Inputs = fixture(NEW_FIXTURES, "native_affine_hierarchy_single")
        .parse()
        .unwrap();
    ffi::validate(&prog(&good)).unwrap();
    let mut missing = good.clone();
    if let Op::AddCachedHier { surface, .. } = &mut missing.ops[1] {
        *surface = 999;
    }
    assert!(ffi::validate(&prog(&missing)).is_err());
    let mut parallel = good.clone();
    if let Op::DefineSurface { u, v, .. } = &mut parallel.ops[0] {
        *v = *u;
    }
    assert!(ffi::validate(&prog(&parallel)).is_err());
    let mut huge = good.clone();
    if let Op::DefineSurface { spans, .. } = &mut huge.ops[0] {
        *spans = [usize::MAX; 2];
    }
    assert!(ffi::validate(&prog(&huge)).is_err());
    let mut legacy_hier = good;
    legacy_hier.ops[1] = Op::AddHier {
        surface: true,
        obj: 7,
        tol: 0.0,
        bbox: [0.0; 6],
    };
    assert!(ffi::validate(&prog(&legacy_hier)).is_err()); // expand_cached cannot dereference it
}

#[test]
fn native_cache_node_selection_is_checked_before_add_element() {
    let mut i: Inputs = fixture(NEW_FIXTURES, "native_affine_cache_leaf_ordinal")
        .parse()
        .unwrap();
    if let Op::AddCachedLeaf { leaf, .. } = &mut i.ops[1] {
        *leaf = 255;
    }
    ffi::validate(&prog(&i)).unwrap(); // only the real built cache knows its leaf count
    assert!(ffi::run(&prog(&i)).is_err());
    assert!(run_rust(&i, &mut Stats::default()).is_err());
}

#[test]
fn native_observer_detects_skipped_expansion_wrong_identity_and_corrupt_normal() {
    let case = fixture(NEW_FIXTURES, "native_affine_identity_twins");
    let i: Inputs = case.parse().unwrap();
    let cpp = Runner.run_cpp(&case).unwrap();
    let rust = output(run_rust(&i, &mut Stats::default()).unwrap());
    let baseline = compare(&cpp, &rust, &[]);
    assert!(baseline.pass(), "{baseline:?}");
    assert!(baseline.values_compared > 2 * 51);
    assert!(baseline.exact_compared > 10);

    // Preserve successful op statuses, but actually omit the grid mutation.
    let skipped = output(run_rust_controlled(&i, &mut Stats::default(), false, |l| l).unwrap());
    assert!(!compare(&cpp, &skipped, &[]).pass());

    // Equal geometry means only the stable identity changes, not any float output.
    let wrong = output(
        run_rust_controlled(&i, &mut Stats::default(), true, |mut l| {
            l.surface = if l.surface == 7 { 19 } else { 7 };
            l
        })
        .unwrap(),
    );
    assert_eq!(rust.values, wrong.values);
    assert_ne!(rust.ints, wrong.ints);
    assert!(!compare(&cpp, &wrong, &[]).pass());

    // Topology, statuses and every other leaf field stay unchanged.
    let corrupt = output(
        run_rust_controlled(&i, &mut Stats::default(), true, |mut l| {
            l.mid_normal.x += 0.125;
            l
        })
        .unwrap(),
    );
    assert_eq!(rust.ints, corrupt.ints);
    assert!(!compare(&cpp, &corrupt, &[]).pass());
}
