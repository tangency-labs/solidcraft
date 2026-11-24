//! Scratch bench (not committed): run a command script through a Session as a window does,
//! timing each command until its result is shown.
//! usage: bgbench <script.json> <bg 0|1>
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let script: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let bg = a[2] == "1";
    let mut s = solidcraft_engine::Session::new(solidcraft_engine::doc::Document::new("bench"));
    s.set_background_checks(bg);
    let mut total = 0.0;
    let mut longest: f64 = 0.0;
    let t_all = Instant::now();
    for c in script["commands"].as_array().unwrap() {
        let t = Instant::now();
        s.execute(c["command"].as_str().unwrap(), &c["params"]).unwrap();
        let ms = t.elapsed().as_secs_f64() * 1e3;
        total += ms;
        if std::env::var("PCC_EACH").is_ok() {
            eprintln!("{} {ms:.1} pending={}", c["command"].as_str().unwrap(), s.model.checking_features());
        }
        longest = longest.max(ms);
        // The person looks at the result: the checks left running end meanwhile (not timed).
        s.settle_feature_checks(true);
    }
    let shown = t_all.elapsed().as_secs_f64() * 1e3;
    s.settle_feature_checks(true);
    let settled = t_all.elapsed().as_secs_f64() * 1e3;
    let errors = s.model.results.iter().filter(|r| r.error.is_some()).count();
    let vol: f64 = s.model.state().bodies.iter().map(|b| solidcraft_engine::kernel::measure(&b.body).map(|m| m.volume).unwrap_or(0.0)).sum();
    println!("bg={bg} commands_ms={total:.1} longest_ms={longest:.1} shown_ms={shown:.1} settled_ms={settled:.1} errors={errors} volume={vol:.6}");
}
