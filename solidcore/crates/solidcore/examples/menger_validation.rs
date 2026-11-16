//! Reproduce #2667's 27 sequential box cuts and separate validation from construction.
//!
//! Run with `cargo run -p solidcore --profile difftest --example menger_validation`.
//! Recipe: kernel-comparison-2/cases/menger-level2-cuts.json, SHA-256
//! 5f1547971e2f487323aa5aabd47e0dc9c2ced9b52871e32d3c691e0ae0448bd9.
//! The archived recipe order is X, Y, Z; each axis has its central cut followed
//! by the eight off-center grid cuts. Times are observations, not performance gates.
//! Add `--features profile` for phase counts; profiled timings include instrumentation.
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::assert_array::AssertArray;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p1::{edges, faces, vertices};
use solidcore::topology::brep::p3::brep_copy;
use solidcore::topology::brep::validation::{check_brep, contains_coincident_topology, CheckLevel};
use solidcore::{Kernel, Point3};
use std::time::Instant;

/// A 45-unit cube keeps 20/27 of its volume at each of two Menger levels.
const EXPECTED_VOLUME: f64 = 45.0 * 45.0 * 45.0 * 20.0 * 20.0 / (27.0 * 27.0);

/// Drain observational phase counts without treating profiled times as the baseline.
#[cfg(feature = "profile")]
fn coincidence_profile(kernel: &Kernel) -> serde_json::Value {
    let profile = kernel.take_profile();
    let rows: Vec<_> = profile
        .scopes
        .iter()
        .filter(|scope| scope.name.starts_with("validation::coincidence"))
        .map(|scope| {
            let counts: std::collections::BTreeMap<_, _> = solidcore::profile::Counter::ALL
                .iter()
                .zip(scope.counters)
                .map(|(counter, value)| (counter.name(), value))
                .collect();
            serde_json::json!({"phase": scope.name, "calls": scope.calls, "counts": counts})
        })
        .collect();
    serde_json::json!(rows)
}

/// Distinguish unmeasured work from a measured zero when profiling is disabled.
#[cfg(not(feature = "profile"))]
fn coincidence_profile(_kernel: &Kernel) -> serde_json::Value {
    serde_json::Value::Null
}

/// Execute the archived cuts and verify validity, topology and analytic volume.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut kernel = Kernel::default();
    let mut body = create_box(&mut kernel, Point3::new(0.0, 0.0, 0.0), 45.0, 45.0, 45.0)?;
    let mut cuts = 0;
    let mut all_valid = true;
    for axis in 0..3 {
        let positions = std::iter::once((22.5, 22.5, 15.0)).chain(
            [7.5, 22.5, 37.5].into_iter().flat_map(|u| {
                [7.5, 22.5, 37.5]
                    .into_iter()
                    .filter_map(move |v| (u != 22.5 || v != 22.5).then_some((u, v, 5.0)))
            }),
        );
        for (u, v, width) in positions {
            let (center, [dx, dy, dz]) = match axis {
                0 => (Point3::new(22.5, u, v), [47.0, width, width]),
                1 => (Point3::new(u, 22.5, v), [width, 47.0, width]),
                _ => (Point3::new(u, v, 22.5), [width, width, 47.0]),
            };
            let corner = Point3::new(
                center.x - dx / 2.0,
                center.y - dy / 2.0,
                center.z - dz / 2.0,
            );
            let tool = create_box(&mut kernel, corner, dx, dy, dz)?;
            kernel.work_counters.reset();
            let start = Instant::now();
            body = boolean_difference(&mut kernel, body, tool)?;
            let boolean_seconds = start.elapsed().as_secs_f64();
            let face_pairs = kernel.work_counters.face_pairs();
            let loops_classified = kernel.work_counters.loops_classified();

            // Independent geometry copies avoid sharing the surface cache slots
            // of clone_standing_for_self. Copy cost is outside the measured phase.
            let mut check_kernel = Kernel::default();
            let mut checked = brep_copy(&mut check_kernel, &body)?;
            let start = Instant::now();
            let check = check_brep(&mut check_kernel, &mut checked, CheckLevel::Level2)?;
            let validation_seconds = start.elapsed().as_secs_f64();
            let mut coincidence_kernel = Kernel::default();
            let mut coincident = brep_copy(&mut coincidence_kernel, &body)?;
            let identities = coincident.clone_standing_for_self()?;
            let keys = |entity| identities.owner_identity(entity);
            let mut reports = AssertArray::new();
            let start = Instant::now();
            let pairs = contains_coincident_topology(
                &coincidence_kernel,
                &mut coincident,
                -1.0,
                false,
                Some(&mut reports),
                0,
                &keys,
            )?
            .pairs
            .len();
            let coincidence_seconds = start.elapsed().as_secs_f64();
            all_valid &= check.is_valid() && pairs == 0;
            cuts += 1;
            println!(
                "{}",
                serde_json::json!({
                    "cut": cuts, "faces": faces(&body, None)?.len(),
                    "edges": edges(&body, None)?.len(), "vertices": vertices(&body, None)?.len(),
                    "boolean_face_pairs": face_pairs, "boolean_loops_classified": loops_classified,
                    "boolean_seconds": boolean_seconds, "validation_seconds": validation_seconds,
                    "coincidence_seconds": coincidence_seconds, "valid": check.is_valid(),
                    "validation_failures": check.lines(), "coincidence_pairs": pairs,
                    "validation_coincidence_profile": coincidence_profile(&check_kernel),
                    "standalone_coincidence_profile": coincidence_profile(&coincidence_kernel),
                })
            );
        }
    }
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let volume = brep_volume(&kernel, &mut body, 1e-7, &attrs)?;
    println!(
        "{}",
        serde_json::json!({"final_volume": volume, "cuts": cuts})
    );
    if !all_valid
        || cuts != 27
        || faces(&body, None)?.len() != 414
        || (volume - EXPECTED_VOLUME).abs() > 1e-9 * EXPECTED_VOLUME
    {
        return Err("Menger reproduction does not match expected topology/volume".into());
    }
    Ok(())
}
