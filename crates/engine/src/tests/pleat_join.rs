//! #1490: the owner's shelled battery case (`battery-tpu-case-v2.sc`, format 21, saved
//! at `wall = 2.5 mm`) with a pleat loft joined into a window in one end wall. The loft stands on
//! the case's open rim and its end strips lie in the wall's inner face. At walls of 2.4–2.6 mm
//! the join failed with "no result": every exact route was refused, and the last resort's
//! nudge failed both ways about its one screw axis. The other screws are now tried too.

use std::path::Path;

use serde_json::json;

use super::run;
use crate::Session;

fn open() -> Session {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures/battery-tpu-case-v2.sc");
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path.to_string_lossy()}));
    s
}

fn errors(s: &Session) -> Vec<(String, String)> {
    s.model.results.iter().filter_map(|r| Some((r.name.clone(), r.error.clone()?))).collect()
}

fn case_volume(s: &mut Session) -> f64 {
    let m = run(s, "inspect.measure", json!({"bodies": ["Case"]}));
    m["total"]["volume_mm3"].as_f64().unwrap()
}

/// The design rebuilds as saved, and at 2.4 and 2.6 mm, with both pleats and every round; the
/// case grows with the wall between them.
#[test]
fn the_pleats_join_at_every_wall_from_2_4_to_2_6_mm() {
    let mut s = open();
    assert_eq!(errors(&s), Vec::new(), "as saved, at 2.5 mm");
    let at = case_volume(&mut s);
    let mut volumes = Vec::new();
    for wall in ["2.4 mm", "2.6 mm"] {
        run(&mut s, "parameters.change", json!({"name": "wall", "expression": wall}));
        assert_eq!(errors(&s), Vec::new(), "at {wall}");
        volumes.push(case_volume(&mut s));
    }
    // About 14.8 cm³ of wall per tenth of a millimetre here.
    let (thin, thick) = (volumes[0], volumes[1]);
    assert!(thin < at && at < thick, "{thin} < {at} < {thick}");
    assert!(((at - thin) - (thick - at)).abs() < 0.01 * (thick - thin), "{thin} {at} {thick}");
}
