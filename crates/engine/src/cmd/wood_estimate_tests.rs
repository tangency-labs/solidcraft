use super::*;
use crate::schema::ParamCheck;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {p}: {e}"))
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-5, "{a} != {b}");
}
fn stool() -> Session {
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    for (i, (x, y)) in [(0., 0.), (300., 0.), (0., 300.), (300., 300.)].into_iter().enumerate() {
        run(&mut s, "solid.box", json!({"length":35,"width":35,"height":400,"corner":[x,y,0],"body_name":format!("Leg{i}")}));
    }
    for (i, y) in [0., 300.].into_iter().enumerate() {
        let name = format!("ApronX{i}");
        run(&mut s, "solid.box", json!({"length":265,"width":18,"height":45,"corner":[35,y+8.5,300],"body_name":name}));
    }
    for (i, x) in [0., 300.].into_iter().enumerate() {
        let name = format!("ApronY{i}");
        run(&mut s, "solid.box", json!({"length":18,"width":265,"height":45,"corner":[x+8.5,35,250],"body_name":name}));
    }
    for (i, y) in [0., 167.5].into_iter().enumerate() {
        let name = format!("Top{i}");
        run(&mut s, "solid.box", json!({"length":335,"width":167.5,"height":20,"corner":[0,y,400],"body_name":name}));
        run(&mut s, "wood.edge_profile", json!({"body":name,"edges":[[167.5,if i==0 {0.} else {335.},420]],"profile":"roundover","radius":3}));
    }
    s
}
fn fixture(dir: &std::path::Path) -> Value {
    std::fs::create_dir_all(dir).unwrap();
    let library = dir.join("stock.json");
    std::fs::write(
        &library,
        serde_json::to_vec(&json!({"stock":[
        {"id":"leg","name":"Leg stock","kind":"board","length":1800,"width":35,"thickness":35,"price":20},
        {"id":"apron","name":"Apron stock","kind":"board","length":1500,"width":45,"thickness":18,"price":15},
        {"id":"top","name":"Top stock","kind":"panel","length":800,"width":400,"thickness":20,"price":30}
    ],"finishing":[{"name":"Oil, clear","quantity":0.25,"unit_price":12}]}))
        .unwrap(),
    )
    .unwrap();
    let mut rates: Rates = serde_json::from_str(DEFAULT_RATES).unwrap();
    rates.labour_per_hour = Some(30.);
    let rates_path = dir.join("rates.json");
    std::fs::write(&rates_path, serde_json::to_vec(&rates).unwrap()).unwrap();
    json!({"library":library,"rates":rates_path,"glue_ups":2,"kerf":3,"trim":10})
}
#[test]
fn wood_estimate_stool_matches_hand_cost_time_and_read_only_csv() {
    let mut s = stool();
    let dir = std::env::temp_dir().join(format!("wood-estimate-stool-{}", std::process::id()));
    let mut params = fixture(&dir);
    let plan: Layout = serde_json::from_value(run(&mut s, "cutlist.layout", json!({"library":params["library"],"kerf":3,"trim":10}))).unwrap();
    assert_eq!(plan.to_buy.iter().map(|p| (p.stock.as_str(), p.count)).collect::<Vec<_>>(), [("apron", 1), ("leg", 1), ("top", 1)]);
    // The plan records 4 leg crosscuts, 4 apron crosscuts and 3 top-sheet cuts.
    // Both tops share a 335 mm strip: one crosscut and two rip cuts. Trim is not a cut.
    assert_eq!(plan.sheets.iter().map(|s| s.cuts.len()).sum::<usize>(), 11);
    let before = s.doc.to_json();
    let (undo, revision) = (s.undo.len(), s.revision);
    params["path"] = json!(dir.join("estimate.csv"));
    let result: Estimate = serde_json::from_value(run(&mut s, ID, params)).unwrap();
    assert!(result.complete, "{:?}", result.warnings);
    close(result.stock_cost, 65.);
    close(result.material_cost, 68.);
    close(result.finishing_cost, 3.);
    // 11 saw cuts * 0.5 + two 335 mm roundovers (0.670 m) * 2 + 2 glue-ups * 15 = 36.84 minutes.
    close(result.work_minutes, 36.84);
    close(result.labour_cost.unwrap(), 18.42);
    close(result.total_cost, 86.42);
    assert_eq!(s.doc.to_json(), before);
    assert_eq!((s.undo.len(), s.revision), (undo, revision));
    let csv = std::fs::read_to_string(dir.join("estimate.csv")).unwrap();
    assert!(csv.contains("\"Oil, clear\""));
    let totals: Vec<_> = csv.lines().find(|line| line.starts_with("total,total,")).unwrap().split(',').collect();
    assert_eq!(totals.len(), 7);
    close(totals[4].parse().unwrap(), 36.84);
    close(totals[6].parse().unwrap(), 86.42);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn wood_estimate_changing_a_rate_only_changes_its_work_lines_and_labour() {
    let mut s = stool();
    let dir = std::env::temp_dir().join(format!("wood-estimate-rate-{}", std::process::id()));
    let params = fixture(&dir);
    let a: Estimate = serde_json::from_value(run(&mut s, ID, params.clone())).unwrap();
    let mut rates: Rates = serde_json::from_str(DEFAULT_RATES).unwrap();
    rates.labour_per_hour = Some(30.);
    rates.router_minutes_per_metre = 5.;
    std::fs::write(dir.join("rates.json"), serde_json::to_vec(&rates).unwrap()).unwrap();
    let b: Estimate = serde_json::from_value(run(&mut s, ID, params.clone())).unwrap();
    close(b.work_minutes - a.work_minutes, 0.670 * 3.);
    for (a, b) in a.lines.iter().zip(&b.lines) {
        if !matches!(a.category.as_str(), "router" | "labour") {
            assert_eq!(serde_json::to_value(a).unwrap(), serde_json::to_value(b).unwrap());
        }
    }
    rates.saw_cut_minutes = 1.;
    std::fs::write(dir.join("rates.json"), serde_json::to_vec(&rates).unwrap()).unwrap();
    let c: Estimate = serde_json::from_value(run(&mut s, ID, params)).unwrap();
    close(c.work_minutes - b.work_minutes, 11. * 0.5);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn wood_estimate_empty_hostile_missing_prices_and_suppression() {
    let mut s = Session::default();
    let empty = run(&mut s, ID, json!({}));
    for key in ["stock_cost", "material_cost", "finishing_cost", "total_cost", "work_minutes"] {
        assert_eq!(empty[key], 0.);
    }
    assert!(empty["labour_cost"].is_null());
    assert_eq!(empty["complete"], true);
    for p in [
        json!({"glue_ups":-1}),
        json!({"glue_ups":100001}),
        json!({"kerf":0}),
        json!({"trim":-1}),
        json!({"path":"x.json"}),
        json!({"finishing":[{"name":"Oil","quantity":-1,"unit_price":5}]}),
        json!({"unknown":true}),
    ] {
        assert!(s.execute(ID, &p).is_err(), "{p}");
    }
    run(&mut s, "solid.box", json!({"length":500,"width":200,"height":18,"body_name":"Board"}));
    run(&mut s, "wood.edge_profile", json!({"edges":[[250,0,18]],"profile":"roundover","radius":3,"name":"Round"}));
    let a: Estimate = serde_json::from_value(run(&mut s, ID, json!({}))).unwrap();
    assert!(!a.complete);
    assert!(a.warnings.iter().any(|w| w.contains("Missing stock price")));
    close(a.lines.iter().filter(|l| l.category == "router").map(|l| l.minutes).sum(), 1.0);
    run(&mut s, "timeline.suppress", json!({"feature":"Round"}));
    let b: Estimate = serde_json::from_value(run(&mut s, ID, json!({}))).unwrap();
    assert!(b.lines.iter().all(|l| l.category != "router"));
    let unplaced = run(&mut s, ID, json!({"stock":[]}));
    assert_eq!(unplaced["complete"], false);
    assert_eq!(unplaced["material_cost"], 0.);
}
#[test]
fn wood_estimate_rejects_bad_rates() {
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    let dir = std::env::temp_dir().join(format!("wood-estimate-bad-{}", std::process::id()));
    let params = fixture(&dir);
    for patch in [json!({"saw_cut_minutes":-1}), json!({"glue_up_minutes":2e6}), json!({"labour_per_hour":1e7}), json!({"extra":1})] {
        let mut rates: Value = serde_json::from_str(DEFAULT_RATES).unwrap();
        rates.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
        std::fs::write(dir.join("rates.json"), serde_json::to_vec(&rates).unwrap()).unwrap();
        assert!(s.execute(ID, &params).is_err());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn wood_estimate_counts_component_occurrences_and_excludes_bought_work() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"Rail"}));
    run(&mut s, "solid.box", json!({"length":100,"width":50,"height":18,"body_name":"Rail"}));
    run(&mut s, "wood.edge_profile", json!({"edges":[[50,0,18]],"profile":"roundover","radius":3}));
    run(&mut s, "component.activate", json!({"component":0}));
    run(&mut s, "occurrence.copy", json!({"component":"Rail","translate":[0,100,0]}));
    run(&mut s, "component.create", json!({"name":"Bought"}));
    run(&mut s, "component.metadata.set", json!({"component":"Bought","bom_type":"purchased"}));
    run(&mut s, "solid.box", json!({"length":100,"width":50,"height":18,"body_name":"Bought board"}));
    run(&mut s, "wood.edge_profile", json!({"body":"Bought board","edges":[[50,0,18]],"profile":"roundover","radius":3}));
    run(&mut s, "component.activate", json!({"component":0}));
    let estimate: Estimate = serde_json::from_value(run(&mut s, ID, json!({}))).unwrap();
    let router: Vec<_> = estimate.lines.iter().filter(|l| l.category == "router").collect();
    assert_eq!(router.len(), 1);
    close(router[0].quantity, 0.2);
    close(router[0].minutes, 0.4);
}

#[test]
fn wood_estimate_design_supplies_round_trip_override_library_without_double_charging() {
    let mut s = Session::default();
    let dir = std::env::temp_dir().join(format!("wood-estimate-supplies-{}", std::process::id()));
    let mut params = fixture(&dir);
    params["glue_ups"] = json!(0);
    run(
        &mut s,
        "document.metadata.set",
        json!({"key":"io.solidcraft.wood.estimate","value":{"finishing":[{"name":"Wax","quantity":2,"unit_price":4}]}}),
    );
    let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    let mut cold = Session::new(doc);
    let a = run(&mut cold, ID, params.clone());
    assert_eq!(a["finishing_cost"], 8.);
    assert_eq!(a["labour_cost"], 0.);
    params["finishing"] = json!([]);
    assert_eq!(run(&mut cold, ID, params.clone())["finishing_cost"], 0.);
    params["finishing"] = json!([{"name":"Shellac","quantity":0.5,"unit_price":10}]);
    assert_eq!(run(&mut cold, ID, params.clone())["finishing_cost"], 5.);
    run(
        &mut cold,
        "document.metadata.set",
        json!({"key":"io.solidcraft.wood.estimate","value":{"finishing":[{"name":"Wax","quantity":-1,"unit_price":4}]}}),
    );
    params.as_object_mut().unwrap().remove("finishing");
    assert!(cold.execute(ID, &params).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}
