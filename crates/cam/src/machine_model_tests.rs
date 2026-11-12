//! Tests of machine models: a model read and its parts placed by their mounts above a tool, the
//! clearance check against boxes, mesh parts, and hostile files refused without a panic.

use solidcore_record::Vec3;

use crate::foundation_tests::*;
use crate::machine_model::*;
use crate::sim::Footprint;
use crate::*;

const MODEL: &str = r#"{
    "format": "solidcraft.machine_model", "version": 1, "name": "a small mill",
    "spindle": {"nose_gap": 5, "nose_max_above_table": 152, "rpm": [3000, 24000]},
    "table": {"size": [302, 175, 15], "corner_radius": 5, "cut_allowance": 0.1,
              "holes": [{"x": 275, "y": 152.5, "d": 13.5, "depth": 5.05, "label": "counterbore"}]},
    "parts": [
        {"name": "the spindle neck", "code": "spindle", "mount": "spindle", "shape": {"cylinder": {"diameter": 23.2, "z": [-5, 7]}}},
        {"name": "the carriage", "code": "x-carriage", "mount": "carriage", "shape": {"box": {"min": [-58, 81, 124.8], "max": [58, 89, 283.2]}}},
        {"name": "the gantry", "code": "gantry", "mount": "frame", "shape": {"box": {"min": [-227, 138, 240.5], "max": [227, 163.4, 265.9]}}}
    ]
}"#;

#[test]
fn parts_are_placed_by_their_mounts() {
    let m = MachineModel::from_json(MODEL).unwrap();
    // 25 mm of stickout and an ER-style nut 4 + 18 high: the nose 25 + 22 + 5 above the tip.
    let tool = Tool {
        stickout: Some(25.0),
        holder: vec![HolderSection { diameter: 34.0, length: 4.0 }, HolderSection { diameter: 34.0, length: 18.0 }],
        ..Tool::new(1, ToolKind::FlatEndmill, 6.0, 15.0)
    };
    close(nose_above_tip(&tool, Some(&m)), 52.0, 1e-12);
    let table = TablePlace { corner: Vec3::new(-100.0, -50.0, -30.0), size: [302.0, 175.0, 15.0] };
    let tip = Vec3::new(10.0, 20.0, 5.0);
    let z = zones(&tool, Some(&m), Some(&table), tip);
    // Cutter, shank, two holder sections, three parts.
    assert_eq!(
        z.iter().map(|z| z.of).collect::<Vec<_>>(),
        [ZoneOf::Cutter, ZoneOf::Shank, ZoneOf::Holder(0), ZoneOf::Holder(1), ZoneOf::Part(0), ZoneOf::Part(1), ZoneOf::Part(2)]
    );
    let neck = z[4];
    assert_eq!(neck.fp, Footprint::Circle { x: 10.0, y: 20.0, r: 11.6 });
    close(neck.z0, 5.0 + 52.0 - 5.0, 1e-12);
    // The carriage follows the tool in X and Y, from the table top.
    let Footprint::Rect { x0, y0, .. } = z[5].fp else { panic!() };
    close(x0, 10.0 - 58.0, 1e-12);
    close(y0, 20.0 + 81.0, 1e-12);
    close(z[5].z0, -30.0 + 124.8, 1e-12);
    // The frame follows in Y only, X from the table's centre.
    let Footprint::Rect { x0, .. } = z[6].fp else { panic!() };
    close(x0, -100.0 + 151.0 - 227.0, 1e-12);
    // Without the table's place only the spindle's parts are placed.
    assert_eq!(zones(&tool, Some(&m), None, tip).len(), 5);
}

#[test]
fn clearance_finds_each_kind_once() {
    let m = MachineModel::from_json(MODEL).unwrap();
    let tool = Tool::new(1, ToolKind::FlatEndmill, 6.0, 15.0);
    let table = TablePlace { corner: Vec3::new(0.0, 0.0, 0.0), size: [302.0, 175.0, 15.0] };
    let jaw = Obstacle::boxed("the vise's fixed jaw", ObstacleKind::Vise, Vec3::new(50.0, 0.0, 0.0), Vec3::new(60.0, 100.0, 40.0));
    let floor = Obstacle::boxed("the table", ObstacleKind::Table, Vec3::new(0.0, 0.0, -15.0), Vec3::new(302.0, 175.0, 0.0));
    // Across the jaw at Z10, and down 0.05 into the table: the allowance.
    let hits = clearance(
        &tool,
        Some(&m),
        Some(&table),
        &[jaw.clone(), floor.clone()],
        &[Vec3::new(30.0, 50.0, 10.0), Vec3::new(80.0, 50.0, 10.0)],
        &[],
        0.5,
        0.01,
    );
    // The spindle's neck (Ø23.2, 15 to 27 above the tip) reaches the jaw first, then the cutter.
    assert_eq!(hits.iter().map(|h| h.code.as_str()).collect::<Vec<_>>(), ["spindle-hits-vise", "cutter-hits-vise"]);
    close(hits[0].at.x, 38.4, 0.5);
    close(hits[1].at.x, 47.0, 0.5);
    let hits = clearance(&tool, Some(&m), Some(&table), std::slice::from_ref(&floor), &[Vec3::new(30.0, 50.0, -0.05)], &[], 0.5, 0.01);
    assert!(hits.is_empty(), "{hits:?}");
    let hits = clearance(&tool, Some(&m), Some(&table), &[floor], &[Vec3::new(30.0, 50.0, -0.2)], &[], 0.5, 0.01);
    assert_eq!(hits.iter().map(|h| h.code.as_str()).collect::<Vec<_>>(), ["cutter-into-table"]);
    // The neck low enough to reach the jaw: a short tool beside it.
    let stub = Tool { stickout: Some(5.0), ..Tool::new(1, ToolKind::FlatEndmill, 2.0, 5.0) };
    let hits = clearance(&stub, Some(&m), Some(&table), &[jaw], &[Vec3::new(40.0, 50.0, 25.0)], &[], 0.5, 0.01);
    assert_eq!(hits.iter().map(|h| h.code.as_str()).collect::<Vec<_>>(), ["spindle-hits-vise"]);
    // The spindle's travel: the nose at most 152 above the table. Past it the program cannot run
    // as written: an error, as the job's check says.
    let high = clearance(&tool, Some(&m), Some(&table), &[], &[Vec3::new(0.0, 0.0, 140.0)], &[], 0.5, 0.01);
    assert_eq!(high.iter().map(|h| (h.code.as_str(), h.severity)).collect::<Vec<_>>(), [("z-travel", crate::findings::Severity::Error)]);
}

#[test]
fn mesh_parts_are_read_for_display() {
    let text = r#"{"source": "test", "parts": [{"name": "spindle #c9ccd1", "mount": "spindle", "color": [0.79, 0.8, 0.82],
                   "tris": [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1]}]}"#;
    let parts = mesh_parts(text, 1000).unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!((parts[0].mount, parts[0].mesh.triangles.len()), (Mount::Spindle, 2));
    assert!(mesh_parts(text, 1).is_err(), "over the cap");
    assert!(mesh_parts(r#"{"parts": [{"mount": "spindle", "tris": [0, 0, 0, 1]}]}"#, 1000).is_err());
    assert!(mesh_parts(r#"{"parts": [{"mount": "nowhere", "tris": []}]}"#, 1000).is_err());
}

#[test]
fn hostile_machine_models_are_refused_without_panics() {
    let base: serde_json::Value = serde_json::from_str(MODEL).unwrap();
    let with = |path: &[&str], v: serde_json::Value| {
        let mut m = base.clone();
        let mut at = &mut m;
        for (k, p) in path.iter().enumerate() {
            if k + 1 == path.len() {
                at[*p] = v.clone();
            } else {
                at = &mut at[*p];
            }
        }
        m.to_string()
    };
    let many: Vec<serde_json::Value> = (0..1001)
        .map(|_| serde_json::json!({"name": "a", "code": "a", "mount": "spindle", "shape": {"cylinder": {"diameter": 1, "z": [0, 1]}}}))
        .collect();
    let bad = [
        with(&["format"], "something else".into()),
        with(&["version"], 2.into()),
        with(&["table", "size"], serde_json::json!([302, -175, 15])),
        with(&["table", "size"], serde_json::json!([302, 1e300, 15])),
        with(&["table", "cut_allowance"], (-1.0).into()),
        with(&["spindle", "nose_gap"], (-5.0).into()),
        with(&["spindle", "nose_max_above_table"], 0.into()),
        with(&["spindle", "rpm"], serde_json::json!([24000, 3000])),
        with(&["parts"], serde_json::Value::Array(many)),
        with(
            &["parts"],
            serde_json::json!([{"name": "x", "code": "Bad Code", "mount": "spindle", "shape": {"box": {"min": [0, 0, 0], "max": [1, 1, 1]}}}]),
        ),
        with(
            &["parts"],
            serde_json::json!([{"name": "x", "code": "x", "mount": "spindle", "shape": {"box": {"min": [0, 0, 0], "max": [1, -1, 1]}}}]),
        ),
        with(&["parts"], serde_json::json!([{"name": "x", "code": "x", "mount": "spindle", "shape": {"cylinder": {"diameter": 0, "z": [0, 1]}}}])),
        with(&["parts"], serde_json::json!([{"name": "x", "code": "x", "mount": "spindle", "shape": {"cylinder": {"diameter": 1, "z": [1, 0]}}}])),
        with(
            &["parts"],
            serde_json::json!([{"name": "x\u{7}", "code": "x", "mount": "spindle", "shape": {"cylinder": {"diameter": 1, "z": [0, 1]}}}]),
        ),
        with(&["unknown_key"], 1.into()),
        with(&["kinematics_path"], "a.json".into()).replace("\"name\"", "\"kinematics\": {\"name\": \"m\", \"rotaries\": []}, \"name\""),
        "{".to_string(),
        "[]".to_string(),
        r#"{"format": "solidcraft.machine_model", "version": 1, "spindle": {"nose_gap": 1e400}, "table": {"size": [1, 1, 1]}}"#.to_string(),
    ];
    for (k, text) in bad.iter().enumerate() {
        assert!(MachineModel::from_json(text).is_err(), "case {k} was taken");
    }
    assert!(MachineModel::from_json(MODEL).is_ok());
}

/// Work zero's machine X and Y by default, from where the work is on the table with the travel
/// centred on the table; the bottom of the spindle's travel from the soft limits' Z.
#[test]
fn travel_is_known_from_the_table_and_the_soft_limits() {
    let m = MachineModel::from_json(&MODEL.replace(r#""table": {"#, r#""soft_limits": {"min": [0, 0, -130], "max": [300, 170, 0]}, "table": {"#))
        .unwrap();
    // Work zero 100 in from the table's left edge and 50 from its front: the spindle there is at
    // machine X 150 - 151 + 100 and Y 85 - 87.5 + 50.
    let table = TablePlace { corner: Vec3::new(-100.0, -50.0, -30.0), size: [302.0, 175.0, 15.0] };
    let [x, y] = work_zero_machine_xy(&m, &table).unwrap();
    close(x, 99.0, 1e-12);
    close(y, 47.5, 1e-12);
    assert!(work_zero_machine_xy(&MachineModel::from_json(MODEL).unwrap(), &table).is_none());
    // The nose goes from 152 down to 22 above the table.
    close(nose_min_above_table(&m).unwrap(), 22.0, 1e-12);
    let tool = Tool {
        stickout: Some(25.0),
        holder: vec![HolderSection { diameter: 34.0, length: 4.0 }, HolderSection { diameter: 34.0, length: 18.0 }],
        ..Tool::new(1, ToolKind::FlatEndmill, 6.0, 15.0)
    };
    let flat = TablePlace { corner: Vec3::ZERO, size: [302.0, 175.0, 15.0] };
    // The nose 52 above the tip: at tip Z-40 it would be 12 above the table, under its travel.
    let low = clearance(&tool, Some(&m), Some(&flat), &[], &[Vec3::new(0.0, 0.0, -20.0), Vec3::new(0.0, 0.0, -40.0)], &[], 0.5, 0.01);
    assert_eq!(low.iter().map(|h| (h.code.as_str(), h.severity)).collect::<Vec<_>>(), [("z-travel-low", crate::findings::Severity::Error)]);
    close(low[0].at.z, -40.0, 1e-12);
    assert!(clearance(&tool, Some(&m), Some(&flat), &[], &[Vec3::new(0.0, 0.0, -30.0)], &[], 0.5, 0.01).is_empty());
    // Without soft limits the bottom of the travel is not known: not checked.
    let m0 = MachineModel::from_json(MODEL).unwrap();
    assert!(clearance(&tool, Some(&m0), Some(&flat), &[], &[Vec3::new(0.0, 0.0, -40.0)], &[], 0.5, 0.01).is_empty());
}

/// A zone meets a triangle exactly: inside it, crossing it with every corner outside, not when
/// it only touches a face or passes beside it.
#[test]
fn zones_meet_triangles_exactly() {
    let rect = Zone { of: ZoneOf::Part(0), fp: Footprint::Rect { x0: 0.0, x1: 10.0, y0: 0.0, y1: 10.0 }, z0: 0.0, z1: 10.0 };
    let v = Vec3::new;
    // Inside.
    assert!(zone_meets_triangle(&rect, &[v(1.0, 1.0, 1.0), v(2.0, 1.0, 1.0), v(1.0, 2.0, 1.0)]));
    // A large triangle through the middle, every corner outside the box.
    assert!(zone_meets_triangle(&rect, &[v(-50.0, -50.0, 5.0), v(60.0, -50.0, 5.0), v(5.0, 60.0, 5.0)]));
    // On the top face only, and beside the box across its corner's diagonal.
    assert!(!zone_meets_triangle(&rect, &[v(-50.0, -50.0, 10.0), v(60.0, -50.0, 10.0), v(5.0, 60.0, 10.0)]));
    assert!(!zone_meets_triangle(&rect, &[v(11.0, 0.0, 5.0), v(20.0, 0.0, 5.0), v(20.0, -9.5, 5.0)]));
    assert!(!zone_meets_triangle(&rect, &[v(20.0, 5.0, 0.0), v(5.0, 20.0, 0.0), v(5.0, 20.0, 10.0)]));
    let round = Zone { of: ZoneOf::Part(0), fp: Footprint::Circle { x: 0.0, y: 0.0, r: 5.0 }, z0: 0.0, z1: 10.0 };
    // A wall 4 from the axis meets it, one 6 away does not; above it, nothing.
    assert!(zone_meets_triangle(&round, &[v(4.0, -20.0, -5.0), v(4.0, 20.0, -5.0), v(4.0, 0.0, 20.0)]));
    assert!(!zone_meets_triangle(&round, &[v(6.0, -20.0, -5.0), v(6.0, 20.0, -5.0), v(6.0, 0.0, 20.0)]));
    assert!(!zone_meets_triangle(&round, &[v(-9.0, -9.0, 11.0), v(9.0, -9.0, 11.0), v(0.0, 9.0, 11.0)]));
}
