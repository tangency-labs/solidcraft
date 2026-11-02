//! Repeatable Combine timings, excluding setup and result measurement.
//! Run with `cargo run --release -p solidcraft-engine --example combine_bench -- [REPEATS] [CASE]`.
//! Each JSON line includes command/kernel time and an analytic volume check.
use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_kernel::profile;
use std::{error::Error, io::Write, time::Instant};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn setup(case: &str) -> Result<(Session, Value, f64)> {
    let mut session = Session::default();
    let mut tools = Vec::new();
    let (operation, expected) = match case {
        "box_join" | "box_apart" | "box_cut" | "box_intersect" => {
            session.execute("solid.box", &json!({"length":20,"width":20,"height":20,"body_name":"Stock"}))?;
            let x = if case == "box_apart" { 30 } else { 10 };
            session.execute("solid.box", &json!({"corner":[x,0,0],"length":20,"width":20,"height":20,"body_name":"Tool"}))?;
            tools.push("Tool".to_string());
            match case {
                "box_cut" => ("cut", 4000.),
                "box_intersect" => ("intersect", 4000.),
                "box_apart" => ("join", 16000.),
                _ => ("join", 12000.),
            }
        }
        "posts_join" | "holes_cut" => {
            session.execute("solid.box", &json!({"length":80,"width":80,"height":5,"body_name":"Stock"}))?;
            for i in 0..16 {
                let name = format!("Tool{i}");
                let x = 10 + (i % 4) * 20;
                let y = 10 + (i / 4) * 20;
                session.execute("solid.cylinder", &json!({"base":[x,y,0],"radius":3,"height":10,"body_name":name}))?;
                tools.push(name);
            }
            let delta = 16. * std::f64::consts::PI * 9. * 5.;
            if case == "posts_join" { ("join", 32000. + delta) } else { ("cut", 32000. - delta) }
        }
        "sphere_join" | "sphere_cut" | "sphere_intersect" => {
            for (name, x) in [("Stock", 0), ("Tool", 10)] {
                session.execute("solid.sphere", &json!({"center":[x,0,0],"radius":10,"body_name":name}))?;
            }
            tools.push("Tool".to_string());
            let sphere = 4. * std::f64::consts::PI * 1000. / 3.;
            let overlap = std::f64::consts::PI * 50. * 100. / 12.;
            match case {
                "sphere_cut" => ("cut", sphere - overlap),
                "sphere_intersect" => ("intersect", overlap),
                _ => ("join", 2. * sphere - overlap),
            }
        }
        "ellipsoid_split" => {
            session.execute("solid.sphere", &json!({"radius":0.5,"body_name":"Stock"}))?;
            session.execute("solid.scale", &json!({"bodies":["Stock"],"factors":[200,300,220]}))?;
            return Ok((session, json!({"body":"Stock","plane":"YZ","name":"Split"}), 4. * std::f64::consts::PI * 100. * 150. * 110. / 3.));
        }
        _ => return Err(format!("unknown benchmark case: {case}").into()),
    };
    Ok((session, json!({"target":"Stock","tools":tools,"operation":operation}), expected))
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let repeats: usize = args.next().map(|v| v.parse()).transpose()?.unwrap_or(3);
    if !(1..=100).contains(&repeats) {
        return Err("repeats must be in 1..=100".into());
    }
    let filter = args.next();
    let cases = [
        "box_join",
        "box_apart",
        "box_cut",
        "box_intersect",
        "posts_join",
        "holes_cut",
        "sphere_join",
        "sphere_cut",
        "sphere_intersect",
        "ellipsoid_split",
    ];
    if filter.as_ref().is_some_and(|name| !cases.contains(&name.as_str())) {
        return Err("unknown case".into());
    }
    let mut out = std::io::stdout().lock();
    for case in cases.into_iter().filter(|case| filter.as_ref().is_none_or(|name| name == case)) {
        let split = case == "ellipsoid_split";
        let command = if split { "solid.split_body" } else { "solid.combine" };
        // Fresh evaluation caches per sample: scratch sessions share their cache, so cloning
        // one setup would benchmark a cached result after the warmup, not the CSG operation.
        for sample in 0..=repeats {
            let (mut session, params, expected) = setup(case)?;
            let before = session.model.recomputed;
            let start = Instant::now();
            let (result, times) = profile::capture(|| session.execute(command, &params));
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.;
            result?;
            let recomputed = session.model.recomputed - before;
            if recomputed == 0 {
                return Err(format!("{case}: measured a cached result instead of an operation").into());
            }
            if let Some(error) = session.model.results.iter().find_map(|r| r.error.as_ref()) {
                return Err(format!("{case}: {error}").into());
            }
            let state = session.model.state();
            let mut volume = 0.;
            let mut faces = 0;
            let mut shells = 0;
            for body in &state.bodies {
                volume += body.body.tessellate_with(0.01, true)?.measure().volume;
                faces += body.body.face_count();
                shells += body.body.shell_count();
                if !body.body.validity().is_empty() {
                    return Err(format!("{case}: invalid body {}", body.name).into());
                }
            }
            let relative_error = (volume - expected).abs() / expected;
            if relative_error > 0.005 || state.bodies.len() != if split { 2 } else { 1 } {
                return Err(format!("{case}: invalid result, volume {volume}, expected {expected}").into());
            }
            if sample > 0 {
                serde_json::to_writer(
                    &mut out,
                    &json!({"case":case,"command":command,"sample":sample,"elapsed_ms":elapsed_ms,
                    "boolean_ms":times.boolean_ms,"tessellate_ms":times.tessellate_ms,
                    "other_ms":(elapsed_ms-times.boolean_ms-times.tessellate_ms).max(0.),
                    "faces":faces,"shells":shells,"volume":volume,
                    "recomputed":recomputed,"restored":session.model.last_restored,
                    "expected_volume":expected,"relative_error":relative_error,
                    "fallbacks":session.model.results.iter().filter_map(|r| r.boolean_diagnostics.summary()).collect::<Vec<_>>()}),
                )?;
                writeln!(out)?;
                out.flush()?;
                if split {
                    for (command, params) in [("edit.undo", json!({})), ("edit.redo", json!({})), ("timeline.delete", json!({"feature":"Split"}))] {
                        let start = Instant::now();
                        let (result, times) = profile::capture(|| session.execute(command, &params));
                        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.;
                        result?;
                        serde_json::to_writer(
                            &mut out,
                            &json!({"case":case,"sample":sample,"command":command,"elapsed_ms":elapsed_ms,
                            "boolean_ms":times.boolean_ms,"tessellate_ms":times.tessellate_ms,
                            "recomputed":session.model.last_recomputed,"restored":session.model.last_restored}),
                        )?;
                        writeln!(out)?;
                        out.flush()?;
                    }
                }
            }
        }
    }
    Ok(())
}
