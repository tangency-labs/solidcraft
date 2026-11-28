//! Midsurface on the built-in sample designs. The Sheet Metal Bracket's mid-surface is checked
//! against its volume over its thickness; the survey runs the whole chain (pairing, sheets,
//! surface body) on every body of every sample at two wall-thickness limits, where nothing may
//! panic and each stage either answers or fails with a message, and prints what each sample gives.

use std::panic::{AssertUnwindSafe, catch_unwind};

use solidcraft_doc::midsurface::{
    Analysis, Confidence, Options, Reason, analyze, completeness, confident, report, sheets, surface_body, thickness_clusters,
};

use crate::Session;

fn reasons(a: &Analysis) -> String {
    let (mut planar, mut range, mut none, mut rim) = (0, 0, 0, 0);
    for l in &a.leftovers {
        match l.reason {
            Reason::NotPlanar => planar += 1,
            Reason::OutOfRange { .. } | Reason::TooThin { .. } | Reason::NoOverlap { .. } => range += 1,
            Reason::NoFacingFace => none += 1,
            Reason::Rim { .. } => rim += 1,
        }
    }
    let clusters: Vec<String> = thickness_clusters(a)
        .iter()
        .map(|c| format!("t {:.2} x{} {:.0} mm2 -> limit {:.2}", c.thickness, c.pairs, c.area, c.suggested_limit))
        .collect();
    let c = completeness(a, &confident(a));
    format!(
        "left over: {rim} rims, {planar} not plane/cylinder, {range} facing but out of range or not overlapping, {none} nothing facing; completeness {} paired + {} rims + {} reported = {}/{}; clusters [{}]",
        c.paired.len(),
        c.rims.len(),
        c.reported.len(),
        c.paired.len() + c.rims.len() + c.reported.len(),
        c.faces,
        clusters.join("; ")
    )
}

/// One body at one limit: a line for the table, or the panic's message.
fn survey(body: &solidcraft_kernel::Body, max: f64) -> Result<String, String> {
    let opts = Options { min_thickness: 0.0, max_thickness: max, min_aspect: 5.0, tolerance: 0.0 };
    let run = catch_unwind(AssertUnwindSafe(|| -> String {
        let a = match analyze(body, &opts) {
            Ok(a) => a,
            Err(e) => return format!("analysis refused: {e}"),
        };
        let high = a.pairs.iter().filter(|p| p.confidence == Confidence::High).count();
        let mut line = format!("{} pairs ({high} high, {} review); {}", a.pairs.len(), a.pairs.len() - high, reasons(&a));
        let chosen = confident(&a);
        if chosen.is_empty() {
            return line + "; no confident pairs to build";
        }
        match sheets(body, &chosen) {
            Err(e) => line.push_str(&format!("; sheets refused: {e}")),
            Ok(s) => {
                line.push_str(&format!("; {} sheets", s.len()));
                match surface_body(&s) {
                    Err(e) => line.push_str(&format!("; surface refused: {e}")),
                    Ok(b) => {
                        match solidcraft_kernel::measure(&b) {
                            Ok(m) => line.push_str(&format!(
                                "; surface body {} faces, area {:.1} mm2, {} open edges",
                                m.faces,
                                m.area,
                                b.open_edge_count().map_or("?".into(), |n| n.to_string())
                            )),
                            Err(e) => line.push_str(&format!("; surface body not measurable: {e}")),
                        }
                        for note in report(&a, &s) {
                            line.push_str(&format!("\n      WARNING: {note}"));
                        }
                    }
                }
            }
        }
        line
    }));
    run.map_err(|p| p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_string())).unwrap_or_default())
}

/// The Sheet Metal Bracket is 2 mm steel throughout: flat walls and bends (inside radius 2,
/// outside 4), joined tangentially. A wall of constant thickness t made of flat and cylindrical
/// pieces holds volume = (its mid-surface's area) x t exactly (a cylindrical shell between radii
/// r - t/2 and r + t/2 over angle a and length L has volume a L r t), so when every face is paired or
/// a rim, the mid-surface's area is the solid's volume over 2.
#[test]
fn the_sheet_metal_brackets_midsurface_is_its_volume_over_its_thickness() {
    let mut s = Session::default();
    let bracket = crate::sample::samples().into_iter().find(|x| x.name == "Sheet Metal Bracket").expect("the bracket sample");
    s.run_script(&(bracket.script)()).expect("the bracket builds");
    let state = s.model.state();
    let body = &state.bodies.first().expect("a body").body;
    let a = analyze(body, &Options { min_thickness: 0.0, max_thickness: 3.0, min_aspect: 5.0, tolerance: 0.0 }).unwrap();
    assert!(a.pairs.iter().all(|p| p.confidence == Confidence::High && (p.thickness - 2.0).abs() < 1e-6), "{:?}", a.pairs);
    let c = completeness(&a, &confident(&a));
    assert!(c.reported.is_empty() && c.accounts_for_every_face(), "{c:?}");
    let surface = surface_body(&sheets(body, &confident(&a)).unwrap()).unwrap();
    let volume = solidcraft_kernel::measure(body).unwrap().volume;
    let area = solidcraft_kernel::measure(&surface).unwrap().area;
    let expected = volume / 2.0;
    assert!((area - expected).abs() < expected * 1e-4, "mid-surface {area} mm2 against volume / t = {expected} mm2");
}

#[test]
#[ignore = "survey of every sample, about ten minutes: run with --ignored --nocapture to see the table"]
fn every_built_in_sample_survives_the_midsurface_chain() {
    let mut panics = Vec::new();
    // A sample that does not build is not surveyed: the survey would pass without it.
    let mut unbuilt = Vec::new();
    let mut table = String::new();
    for sample in crate::sample::samples() {
        let mut s = Session::default();
        if let Err(e) = s.run_script(&(sample.script)()) {
            table.push_str(&format!("{}: the sample itself did not build: {e}\n", sample.name));
            unbuilt.push(sample.name.to_string());
            continue;
        }
        let state = s.model.state();
        for b in &state.bodies {
            let faces = b.body.face_count();
            for max in [3.0, 10.0] {
                match survey(&b.body, max) {
                    Ok(line) => table.push_str(&format!("MIDSURF {} / {} ({faces} faces) limit {max}: {line}\n", sample.name, b.name)),
                    Err(p) => {
                        table.push_str(&format!("MIDSURF {} / {} limit {max}: PANIC {p}\n", sample.name, b.name));
                        panics.push(format!("{} / {}", sample.name, b.name));
                    }
                }
            }
        }
    }
    println!("{table}");
    assert!(panics.is_empty(), "panics: {panics:?}");
    assert!(unbuilt.is_empty(), "samples that did not build, so were not surveyed: {unbuilt:?}");
}
