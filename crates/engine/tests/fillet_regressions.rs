//! Fillet regressions.
use serde_json::Value;
use solidcraft_engine::Session;

fn volume(s: &Session) -> f64 {
    s.model.state().bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).map(|m| m.volume).unwrap_or(f64::NAN)).sum()
}

fn sound(s: &Session) {
    let errors: Vec<_> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    let st = s.model.state();
    assert_eq!(st.bodies.len(), 1, "one body");
    for b in &st.bodies {
        assert_eq!(b.body.shell_count(), 1, "one shell");
        assert!(b.body.validity().is_empty(), "{:?}", b.body.validity());
    }
}

#[test]
fn outer_top_fillet_after_the_inner_round_builds_one_closed_body() {
    let history: Value = serde_json::from_str(include_str!("fixtures/fillet_outer_top_after_inner_round_hist.json")).unwrap();
    let issue: Value = serde_json::from_str(include_str!("fixtures/fillet_outer_top_after_inner_round_issue.json")).unwrap();
    let mut s = Session::default();
    s.run_script(&history).unwrap();
    sound(&s);
    let cmd = &issue["commands"][0];
    let r = s.execute(cmd["command"].as_str().unwrap(), &cmd["params"]);
    assert!(r.is_ok(), "fillet refused: {}", r.err().map(|e| e.to_string()).unwrap_or_default());
    sound(&s);
    let (got, want) = (volume(&s), 323074.2236);
    assert!((got - want).abs() < 1e-6 * want, "volume {got}, want {want}");
}
