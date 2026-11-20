use serde_json::{Value, json};

use super::*;

/// A 2 thick wall, 30 x 2 x 10, as the sketch `Private sketch` makes it in the body `Secret wall`.
fn wall() -> Session {
    let mut s = Session::default();
    for (c, p) in [
        ("sketch.create", json!({"plane":"XY", "name":"Private sketch"})),
        ("sketch.rectangle.two_point", json!({"p0":[0,0], "p1":[30,2]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance":"10 mm", "body_name":"Secret wall"})),
    ] {
        s.execute(c, &p).unwrap();
    }
    s
}

fn gunzip(bytes: &[u8]) -> Value {
    let mut text = String::new();
    std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(bytes), &mut text).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// #17926: a fillet the kernel refuses, rebuilt for its report, is a rig entry whose last call
/// is the refused blend with the code the command's error has, its operand given as a record in
/// the bodies beside it, and nothing of the design's names; rebuilding it leaves the session as
/// it was, and a fillet that builds has no case.
#[test]
fn a_refused_fillet_rebuilds_as_a_rig_entry_with_its_code() {
    let mut s = wall();
    let fillet = json!({"edges": [[15, 0, 10], [15, 2, 10]], "radius": "1.5 mm"});
    let code = s.execute("solid.fillet", &fillet).unwrap_err().error_code().cloned().unwrap();
    let features = s.doc.features.len();

    let case = rebuild(&s, "solid.fillet", &fillet, "abc123").unwrap();
    assert_eq!(s.doc.features.len(), features, "the session is untouched");
    assert_eq!(case.code.as_ref(), Some(&code));
    let entry: Value = serde_json::from_slice(&case.entry).unwrap();
    let calls = entry["calls"].as_array().unwrap();
    let last = calls.last().unwrap();
    assert!(last["error"].is_string(), "{last}");
    assert_eq!(last["code"], json!({"call": code.call, "kind": code.kind, "operation": code.operation}));
    assert_eq!(entry["records"], format!("{}.bodies.json.gz", case.name));
    assert_eq!((entry["solidcraft"].as_str(), entry["operation_expressible"].as_bool()), (Some("abc123"), Some(true)));
    // every body a call takes is a record given or an earlier call's output
    let records = gunzip(&case.bodies);
    let mut have: Vec<String> = records.as_object().unwrap().keys().cloned().collect();
    assert!(!have.is_empty());
    assert_eq!(entry["bodies"].as_object().unwrap().keys().cloned().collect::<Vec<_>>(), have);
    for c in calls {
        for i in c["inputs"].as_array().unwrap() {
            assert!(have.contains(&i.as_str().unwrap().to_owned()), "{i} in {c}");
        }
        have.extend(c["outputs"].as_array().into_iter().flatten().map(|o| o.as_str().unwrap().to_owned()));
    }
    let text = format!("{}{records}", String::from_utf8_lossy(&case.entry));
    assert!(!text.contains("Secret wall") && !text.contains("Private sketch"), "no names of the design");

    // `KERNEL_CASE_OUT=<dir>` leaves the case there, to replay it in solidcore's rig
    // (`KERNEL_CASES_ENTRY`)
    if let Some(dir) = std::env::var_os("KERNEL_CASE_OUT") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::write(dir.join(case.entry_name()), &case.entry).unwrap();
        std::fs::write(dir.join(case.bodies_name()), &case.bodies).unwrap();
    }

    let fits = json!({"edges": [[15, 0, 10], [15, 2, 10]], "radius": "0.5 mm"});
    assert!(rebuild(&s, "solid.fillet", &fits, "abc123").is_err(), "a fillet that builds has no case");
}
