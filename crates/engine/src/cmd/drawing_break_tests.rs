use serde_json::json;

#[test]
fn break_preview_is_read_only_and_reports_empty_gap_picks() {
    let mut s = crate::Session::new(Default::default());
    let before = serde_json::to_value(&s.doc).unwrap();
    let r = s
        .execute(
            "drawing.break.preview",
            &json!({
                "angle_rad": -std::f64::consts::FRAC_PI_2, "scale": "1:2",
                "breaks": [{"start":60,"end":140,"gap_mm":5}],
                "lines": [[[0,0],[200,0]]], "paper_points": [[32,0],[65,0]]
            }),
        )
        .unwrap();
    assert_eq!(r["lines"].as_array().unwrap().len(), 2);
    assert_eq!(r["shortening_mm"], 35.0);
    assert!(r["model_points"][0].is_null());
    assert!((r["model_points"][1][0].as_f64().unwrap() - 200.0).abs() < 1e-9);
    assert_eq!(serde_json::to_value(&s.doc).unwrap(), before);
    assert!(super::COMMANDS[0].contract.is_some());
    assert!(super::COMMANDS[0].output.is_some());
    assert!(!super::COMMANDS[0].undoable);
}

#[test]
fn bad_previews_refuse_atomically_including_record_and_output_caps() {
    let mut s = crate::Session::new(Default::default());
    let before = serde_json::to_value(&s.doc).unwrap();
    let cases = [
        json!({"angle_rad":0,"scale":"1:1","breaks":[{"start":0,"end":0,"gap_mm":1}]}),
        json!({"angle_rad":0,"scale":"1:0"}),
        json!({"angle_rad":0,"scale":"1:1","extra":true}),
        json!({"angle_rad":0,"scale":"1:1","lines":vec![[[0,0],[1,1]];2001]}),
        json!({"angle_rad":0,"scale":"1:1","breaks":vec![json!({"start":0,"end":10,"gap_mm":1});65]}),
    ];
    for mode in [crate::schema::ParamCheck::Off, crate::schema::ParamCheck::Warn, crate::schema::ParamCheck::Strict] {
        s.param_check = mode;
        for p in &cases {
            assert!(s.execute("drawing.break.preview", p).is_err());
        }
    }
    let bands: Vec<_> = (0..64).map(|i| json!({"start":i*20+5,"end":i*20+15,"gap_mm":1})).collect();
    assert!(s.execute("drawing.break.preview", &json!({"angle_rad":0,"scale":"1:1","breaks":bands,"lines":vec![[[0,0],[0,1300]];400]})).is_err());
    assert_eq!(serde_json::to_value(&s.doc).unwrap(), before);
}
