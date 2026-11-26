//! MJCF export (#20786, docs/mjcf.md): bodies, joints written at the current values, limits, loops
//! and motion links closed by equality constraints. The `checker_*` tests read the file back and
//! move it as MuJoCo does, against values worked out by hand.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcraft_io::MjcfPackage;

use super::urdf_tests::{M4, fan, hinge, joint, link, m4_apply, m4_mul, m4_rot, m4_shift, occ, run, temp, tool_frame, two_cubes};
use crate::Session;

fn package(s: &Session) -> MjcfPackage {
    match solidcraft_io::mjcf(&s.doc, &s.doc.painted(&s.model.state()), "t") {
        Ok(p) => p,
        Err(e) => panic!("{e}"),
    }
}

/// An MJCF element as read back: its name, its attributes and the index of the one it is in.
struct Element {
    tag: String,
    attrs: BTreeMap<String, String>,
    parent: Option<usize>,
}

impl Element {
    fn get(&self, key: &str) -> &str {
        self.attrs.get(key).map_or("", String::as_str)
    }
    fn numbers(&self, key: &str) -> Vec<f64> {
        self.get(key).split_whitespace().map(|x| x.parse().unwrap_or_else(|e| panic!("{key} {x}: {e}"))).collect()
    }
    fn three(&self, key: &str) -> [f64; 3] {
        self.numbers(key).try_into().unwrap_or_else(|v| panic!("{key}: {v:?}"))
    }
    /// The frame `pos` and `xyaxes` give.
    fn pose(&self) -> M4 {
        let a = self.numbers("xyaxes");
        let (x, y) = ([a[0], a[1], a[2]], [a[3], a[4], a[5]]);
        let z = [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]];
        let p = self.three("pos");
        let mut m = m4_shift(p);
        for row in 0..3 {
            (m[row][0], m[row][1], m[row][2]) = (x[row], y[row], z[row]);
        }
        m
    }
}

/// An MJCF file as a reader sees it.
struct Model(Vec<Element>);

impl Model {
    fn read(xml: &str) -> Model {
        use quick_xml::events::Event;
        let mut reader = quick_xml::Reader::from_str(xml);
        let (mut all, mut open): (Vec<Element>, Vec<usize>) = (Vec::new(), Vec::new());
        loop {
            let (e, empty) = match reader.read_event() {
                Ok(Event::Start(e)) => (e, false),
                Ok(Event::Empty(e)) => (e, true),
                Ok(Event::End(_)) => {
                    open.pop();
                    continue;
                }
                Ok(Event::Eof) => break,
                Ok(_) => continue,
                Err(e) => panic!("the MJCF is not XML: {e}"),
            };
            let text = |b: &[u8]| String::from_utf8_lossy(b).to_string();
            let attrs = e.attributes().map(|a| a.unwrap_or_else(|e| panic!("{e}"))).map(|a| (text(a.key.as_ref()), text(&a.value))).collect();
            all.push(Element { tag: text(e.name().as_ref()), attrs, parent: open.last().copied() });
            if !empty {
                open.push(all.len() - 1);
            }
        }
        Model(all)
    }

    fn of(s: &Session) -> Model {
        Model::read(&package(s).xml)
    }

    fn all<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a Element> {
        self.0.iter().filter(move |e| e.tag == tag)
    }

    fn names<'a>(&'a self, tag: &'a str) -> Vec<&'a str> {
        self.all(tag).map(|e| e.get("name")).collect()
    }

    /// The elements `<tag>` in `<parent>`, which sets an equality's `<joint>` apart from a body's.
    fn within<'a>(&'a self, parent: &'a str, tag: &'a str) -> Vec<&'a Element> {
        self.all(tag).filter(|e| e.parent.is_some_and(|p| self.0[p].tag == parent)).collect()
    }

    fn joint(&self, name: &str) -> &Element {
        self.within("body", "joint").into_iter().find(|j| j.get("name") == name).unwrap_or_else(|| panic!("no joint {name}"))
    }

    /// A body's world frame (m) with the hinges and slides in `q` at those values and the others at
    /// their `ref`, as MuJoCo composes them: each joint moves the body in the frame the ones before
    /// it leave, by how far its value is from its `ref`.
    fn frame(&self, body: &str, q: &[(&str, f64)]) -> M4 {
        let at = self.0.iter().position(|e| e.tag == "body" && e.get("name") == body).unwrap_or_else(|| panic!("no body {body}"));
        let element = &self.0[at];
        let mut m = match element.parent.map(|p| &self.0[p]) {
            Some(p) if p.tag == "body" => m4_mul(&self.frame(p.get("name"), q), &element.pose()),
            _ => element.pose(),
        };
        for j in self.0.iter().filter(|e| e.tag == "joint" && e.parent == Some(at) && e.get("type") != "ball") {
            let Some((_, value)) = q.iter().find(|(n, _)| *n == j.get("name")) else { continue };
            let by = value - j.numbers("ref")[0];
            let (axis, pos) = (j.three("axis"), j.three("pos"));
            m = match j.get("type") {
                "slide" => m4_mul(&m, &m4_shift(axis.map(|x| x * by))),
                _ => m4_mul(&m4_mul(&m4_mul(&m, &m4_shift(pos)), &m4_rot(axis, by)), &m4_shift(pos.map(|x| -x))),
            };
        }
        m
    }

    /// A body's centre of mass in the world (m).
    fn com(&self, body: &str, q: &[(&str, f64)]) -> [f64; 3] {
        let at = self.0.iter().position(|e| e.tag == "body" && e.get("name") == body);
        let inertial = self.all("inertial").find(|e| e.parent == at).unwrap_or_else(|| panic!("{body} has no inertial"));
        m4_apply(&self.frame(body, q), inertial.three("pos"))
    }
}

fn assert_near(got: &[f64], want: &[f64], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: {got:?}");
    assert!(got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-9), "{what}: {got:?}, expected {want:?}");
}

#[test]
fn mjcf_of_a_hinge() {
    let mut s = hinge();
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 30}));
    let p = package(&s);
    let m = Model::read(&p.xml);
    assert_eq!(m.all("mujoco").map(|e| e.get("model")).collect::<Vec<_>>(), ["t"]);
    assert_eq!(m.names("mesh"), ["t-LeafA", "t-LeafB"]);
    assert_eq!(m.names("body"), ["LeafA", "LeafB"]);
    assert_eq!(m.within("body", "joint").len(), 1);
    // Written where the design is: the hinge's reference is its current value.
    assert_near(&m.joint("Hinge").numbers("ref"), &[30f64.to_radians()], "ref");
    let files: Vec<&str> = p.meshes.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(files, ["t-LeafA.stl", "t-LeafB.stl"]);
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
}

/// A joint with several values is one body with a joint per value, or one `ball`, with none of
/// the URDF chain's in-between links.
#[test]
fn mjcf_keeps_a_joint_with_several_values_in_one_body() {
    let cases = [
        ("planar", json!([2, 3, 40]), vec!["slide", "slide", "hinge"]),
        ("pin_slot", json!([30, 3]), vec!["slide", "hinge"]),
        ("cylindrical", json!([30, 4]), vec!["hinge", "slide"]),
        ("parallel", json!([1, 2, 3, 25]), vec!["slide", "slide", "slide", "hinge"]),
        ("ball", json!([20, 30, 40]), vec!["ball"]),
    ];
    for (kind, values, types) in cases {
        let mut s = two_cubes();
        joint(&mut s, kind, "J", Value::Null);
        run(&mut s, "joint.drive", json!({"joint": "J", "values": values}));
        let m = Model::of(&s);
        assert_eq!(m.names("body"), ["A", "B"], "{kind}");
        assert_eq!(m.within("body", "joint").iter().map(|j| j.get("type")).collect::<Vec<_>>(), types, "{kind}");
    }
}

#[test]
fn mjcf_writes_a_range_only_where_there_is_a_limit() {
    let mut s = two_cubes();
    joint(&mut s, "slider", "Rail", json!([[0, 10]]));
    assert_near(&Model::of(&s).joint("Rail").numbers("range"), &[0.0, 0.01], "range");

    let mut s = two_cubes();
    joint(&mut s, "slider", "Free", Value::Null);
    let p = package(&s);
    assert_eq!(Model::read(&p.xml).joint("Free").get("range"), "");
    assert!(p.warnings.is_empty(), "MJCF needs no limit: {:?}", p.warnings);
}

/// MuJoCo leaves a joint free where its range is one value, so a constraint holds it there: its
/// constant is the limit less the joint's `ref`.
#[test]
fn mjcf_holds_a_joint_whose_limits_are_one_value() {
    let mut s = two_cubes();
    joint(&mut s, "revolute", "Turn", json!([[-30, 45]]));
    let held = 20f64.to_radians();
    s.doc_mut().assembly.joints[0].limits = vec![Some((held, held))];
    let m = Model::of(&s);
    assert_eq!(m.joint("Turn").get("range"), "");
    let [c] = m.within("equality", "joint")[..] else { panic!("one constraint") };
    assert_eq!(c.get("joint1"), "Turn");
    let at = m.joint("Turn").numbers("ref")[0];
    assert_near(&c.numbers("polycoef"), &[held - at, 0.0, 0.0, 0.0, 0.0], "polycoef");
}

/// A sits on the ground, B turns on it (`AB`), C hangs on A a millimetre above B, and `name` closes
/// the loop on B's top face, on AB's axis.
fn loop_of(kind: &str, name: &str) -> Session {
    let mut s = two_cubes();
    joint(&mut s, "revolute", "AB", Value::Null);
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "C"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 21]}));
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "A", "b": "C", "name": "AC"}));
    run(&mut s, "joint.as_built", json!({"type": kind, "a": "B", "b": "C", "at": {"face": [5, 5, 20]}, "name": name}));
    s
}

/// The points (m, world) a loop's `connect` constraints hold.
fn held_points(m: &Model) -> Vec<[f64; 3]> {
    m.all("connect").map(|c| m4_apply(&m.frame(c.get("body1"), &[]), c.three("anchor"))).collect()
}

#[test]
fn checker_loops_are_closed_where_their_joints_are() {
    let m = Model::of(&loop_of("rigid", "BC"));
    let [weld] = m.all("weld").collect::<Vec<_>>()[..] else { panic!("one weld") };
    assert_eq!((weld.get("body1"), weld.get("body2")), ("B", "C"));

    // A hinge is held at two points of its axis: B's top face centre, and 10 mm along the axis,
    // which is AB's own (x 5, y 5 mm), so B keeps its turn.
    let m = Model::of(&loop_of("revolute", "BC"));
    let [near, far] = held_points(&m)[..] else { panic!("two points") };
    assert_near(&near, &[0.005, 0.005, 0.02], "near");
    assert_near(&far[..2], &[0.005, 0.005], "far, on the axis");
    assert_near(&[(far[2] - 0.02).abs()], &[0.01], "far, along the axis");

    let m = Model::of(&loop_of("ball", "BC"));
    let [centre] = held_points(&m)[..] else { panic!("one point") };
    assert_near(&centre, &[0.005, 0.005, 0.02], "centre");
}

#[test]
fn mjcf_leaves_out_a_loop_no_constraint_holds() {
    let p = package(&loop_of("slider", "BC"));
    assert!(!p.xml.contains("<equality>"), "{}", p.xml);
    assert!(p.warnings.iter().any(|w| w.contains("joint `BC` closes a loop as a Slider joint")), "{:?}", p.warnings);
}

/// MuJoCo refuses a file that names two constraints alike, and a hinge's loop takes two names.
#[test]
fn mjcf_names_every_constraint_apart() {
    let mut s = loop_of("revolute", "Loop");
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "B", "b": "C", "name": "Loop_1"}));
    let m = Model::of(&s);
    let mut names = [m.names("connect"), m.names("weld")].concat();
    assert_eq!(names.len(), 3, "{names:?}");
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 3, "{names:?}");
}

/// `world` is MuJoCo's own body. Only the part that is called so is renamed, in its constraints too.
#[test]
fn mjcf_keeps_mujocos_world_apart() {
    let mut s = loop_of("rigid", "BC");
    run(&mut s, "component.rename", json!({"component": "B", "name": "world"}));
    let m = Model::of(&s);
    assert_eq!(m.names("body"), ["A", "world_2", "C"]);
    assert_eq!(m.all("weld").map(|w| (w.get("body1"), w.get("body2"))).collect::<Vec<_>>(), [("world_2", "C")]);
}

#[test]
fn mjcf_words_a_relation_for_mjcf() {
    let mut s = two_cubes();
    joint(&mut s, "perpendicular", "J", Value::Null);
    let p = package(&s);
    assert!(p.warnings.iter().any(|w| w.contains("which MJCF has no joint for")), "{:?}", p.warnings);
    assert!(!p.warnings.iter().any(|w| w.contains("URDF")), "{:?}", p.warnings);
}

/// B sits on A's top face and turns about the vertical axis through its centre (5, 5): its centre
/// of mass stays, and 30° to 120° turns it by 90°.
#[test]
fn checker_hinge_turns_b_about_its_axis() {
    let mut s = two_cubes();
    joint(&mut s, "revolute", "J", Value::Null);
    run(&mut s, "joint.drive", json!({"joint": "J", "value": 30}));
    let m = Model::of(&s);
    let q = [("J", 120f64.to_radians())];
    assert_near(&m.com("B", &[]), &[0.005, 0.005, 0.015], "centre");
    assert_near(&m.com("B", &q), &[0.005, 0.005, 0.015], "centre, turned");
    let (before, after) = (m.frame("B", &[]), m.frame("B", &q));
    // B's x axis goes to where a quarter turn about z takes it.
    let (bx, ax) = ([before[0][0], before[1][0], before[2][0]], [after[0][0], after[1][0], after[2][0]]);
    assert_near(&ax, &[-bx[1], bx[0], bx[2]], "x axis");
}

/// B slides up from A's top face: its centre is 15 mm up plus the joint's value.
#[test]
fn checker_slider_lifts_b_by_its_value() {
    let mut s = two_cubes();
    joint(&mut s, "slider", "J", Value::Null);
    run(&mut s, "joint.drive", json!({"joint": "J", "value": 60}));
    let m = Model::of(&s);
    assert_near(&m.com("B", &[]), &[0.005, 0.005, 0.075], "at 60 mm");
    assert_near(&m.com("B", &[("J", 0.008)]), &[0.005, 0.005, 0.023], "at 8 mm");
}

/// B slides 2 and 3 mm in A's top face and turns 40° in it: its centre is √(2² + 3²) mm off the
/// axis at 15 mm, and back on it at zero, whatever the turn.
#[test]
fn checker_planar_moves_b_in_the_face() {
    let mut s = two_cubes();
    joint(&mut s, "planar", "J", Value::Null);
    run(&mut s, "joint.drive", json!({"joint": "J", "values": [2, 3, 40]}));
    let m = Model::of(&s);
    let c = m.com("B", &[]);
    assert_near(&[(c[0] - 0.005).hypot(c[1] - 0.005), c[2]], &[13f64.sqrt() * 1e-3, 0.015], "centre");
    assert_near(&m.com("B", &[("J_dof1", 0.0), ("J_dof2", 0.0)]), &[0.005, 0.005, 0.015], "back on the axis");
}

/// Turned by z 20°, y 30°, z 40° about the centre of A's top face, B's centre, 5 mm along its own
/// z, rises 5 cos 30° mm above it. The ball's own value is the identity there.
#[test]
fn checker_ball_is_written_where_b_is() {
    let mut s = two_cubes();
    joint(&mut s, "ball", "J", Value::Null);
    run(&mut s, "joint.drive", json!({"joint": "J", "values": [20, 30, 40]}));
    let c = Model::of(&s).com("B", &[]);
    let d = [c[0] - 0.005, c[1] - 0.005, c[2] - 0.01];
    assert_near(&[d.iter().map(|x| x * x).sum::<f64>().sqrt(), d[2]], &[0.005, 0.005 * 30f64.to_radians().cos()], "centre");
}

/// (driver, constant, ratio) of the coupling of `joint`, whichever way round it is written. Written
/// with a ratio below 1 it would be too soft to hold: MuJoCo sets its stiffness without the ratio.
fn coupling(m: &Model, joint: &str) -> Option<(String, f64, f64)> {
    let c = m.within("equality", "joint").into_iter().find(|c| c.get("name") == joint)?;
    let p = c.numbers("polycoef");
    assert!(p[1].abs() >= 1.0, "{joint}: {p:?}");
    let (y, x) = (c.get("joint1").to_string(), c.get("joint2").to_string());
    Some(if y == joint { (x, p[0], p[1]) } else { (y, -p[0] / p[1], 1.0 / p[1]) })
}

/// Radii 10 and 20: the second gear turns half as far, the other way. MuJoCo's constraint is
/// between how far each joint is from its `ref`, so the constant stays 0 when the pair is driven.
#[test]
fn checker_gears_turn_by_their_radii() {
    let mut s = fan(&["revolute", "revolute"]);
    run(&mut s, "joint.gear", json!({"a": "J0", "b": "J1", "ra": 10, "rb": 20}));
    for value in [0, 50] {
        run(&mut s, "joint.drive", json!({"joint": "J0", "value": value}));
        let m = Model::of(&s);
        assert_eq!(coupling(&m, "J0"), None);
        let (driver, constant, ratio) = coupling(&m, "J1").unwrap_or_else(|| panic!("no coupling"));
        assert_eq!(driver, "J0");
        assert_near(&[constant, ratio], &[0.0, -0.5], "coupling");
        assert_near(&m.joint("J1").numbers("ref"), &[-0.5 * f64::from(value).to_radians()], "ref");
    }
}

/// A pinion of radius 5 mm moves its rack 5 mm, 0.005 m, per radian.
#[test]
fn checker_rack_and_pinion_couples_metres_to_radians() {
    let mut s = fan(&["revolute", "slider"]);
    run(&mut s, "joint.rack_pinion", json!({"a": "J0", "b": "J1", "radius": 5}));
    let (_, constant, ratio) = coupling(&Model::of(&s), "J1").unwrap_or_else(|| panic!("no coupling"));
    assert_near(&[constant, ratio.abs()], &[0.0, 0.005], "coupling");
}

/// J1 = 2 J0 and J2 = 3 J1 + 0.1: J2 follows the first driver, at 2 × 3.
#[test]
fn checker_chained_links_follow_their_first_driver() {
    let mut s = fan(&["revolute", "revolute", "revolute"]);
    link(&mut s, "J0", "J1", 2.0, 0.0);
    link(&mut s, "J1", "J2", 3.0, 0.1);
    let m = Model::of(&s);
    let (driver, constant, ratio) = coupling(&m, "J2").unwrap_or_else(|| panic!("no coupling"));
    assert_eq!(driver, "J0");
    assert_near(&[constant, ratio], &[0.0, 6.0], "coupling");
    assert_near(&m.joint("J2").numbers("ref"), &[0.1], "ref");
}

#[test]
fn mjcf_leaves_out_a_link_to_a_ball_joint() {
    let mut s = fan(&["revolute", "ball"]);
    link(&mut s, "J0", "J1", 2.0, 0.0);
    let p = package(&s);
    assert!(!p.xml.contains("<equality>"), "{}", p.xml);
    assert!(p.warnings.iter().any(|w| w.contains("an MJCF ball joint has no values to couple")), "{:?}", p.warnings);
}

#[test]
fn mjcf_writes_saved_damping_and_friction() {
    let mut s = hinge();
    run(&mut s, "joint.dynamics", json!({"joint": "Hinge", "damping": 0.1, "friction": 0.02}));
    let m = Model::of(&s);
    let j = m.joint("Hinge");
    assert_eq!((j.get("damping"), j.get("frictionloss")), ("0.1", "0.02"));
}

/// A 10 mm cube that collides as a box is drawn by its mesh and collides as its half sizes, 5 mm.
#[test]
fn mjcf_writes_collision_shapes() {
    let mut s = two_cubes();
    joint(&mut s, "revolute", "J", Value::Null);
    run(&mut s, "body.collision", json!({"component": "B", "shape": "box"}));
    let m = Model::of(&s);
    let at = m.0.iter().position(|e| e.tag == "body" && e.get("name") == "B");
    let geoms: Vec<&Element> = m.all("geom").filter(|g| g.parent == at).collect();
    let [drawn, shape] = geoms[..] else { panic!("{} geoms", geoms.len()) };
    assert_eq!((drawn.get("type"), drawn.get("contype"), drawn.get("conaffinity")), ("mesh", "0", "0"));
    assert_eq!(shape.get("type"), "box");
    assert_near(&shape.numbers("size"), &[0.005, 0.005, 0.005], "half sizes");
}

#[test]
fn mjcf_writes_a_frame_as_a_site() {
    let m = Model::of(&tool_frame());
    let [site] = m.all("site").collect::<Vec<_>>()[..] else { panic!("one site") };
    assert_eq!(site.get("name"), "tool0");
    assert!(!m.names("body").contains(&"tool0"));
}

#[test]
fn mjcf_command_writes_the_meshes_then_the_model() {
    let mut s = hinge();
    let d = temp("mjcf-command");
    let path = d.join("robot.xml");
    let r = run(&mut s, "file.export_mjcf", json!({"path": path.to_string_lossy()}));
    assert_eq!(r["format"], "Mjcf", "{r}");
    assert_eq!((r["bodies"].as_u64(), r["joints"].as_u64(), r["equalities"].as_u64()), (Some(2), Some(1), Some(0)), "{r}");
    let files: Vec<String> = r["files"].as_array().into_iter().flatten().filter_map(|f| f.as_str()).map(String::from).collect();
    let names: Vec<String> = files.iter().filter_map(|f| std::path::Path::new(f).file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["robot-LeafA.stl", "robot-LeafB.stl", "robot.xml"], "{r}");
    let xml = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(Model::read(&xml).all("mesh").map(|e| e.get("file")).collect::<Vec<_>>(), ["robot-LeafA.stl", "robot-LeafB.stl"]);
    let _ = std::fs::remove_dir_all(&d);
}

/// `file.export` writes MJCF when `format` names it, and refuses a body list. A `.xml` path alone
/// is any XML, left to an exporter plugin that claims it.
#[test]
fn file_export_writes_mjcf_when_named() {
    let mut s = hinge();
    let d = temp("mjcf-export");
    let path = d.join("a.xml");
    let r = run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "format": "mjcf"}));
    assert_eq!(r["format"], "Mjcf", "{r}");
    assert!(std::fs::read_to_string(&path).unwrap_or_default().starts_with("<?xml"), "{path:?}");
    assert!(s.execute("file.export", &json!({"path": d.join("b.xml").to_string_lossy()})).is_err());
    assert!(!d.join("b.xml").exists());
    let e = s.execute("file.export", &json!({"path": d.join("c.xml").to_string_lossy(), "format": "mjcf", "bodies": ["LeafA"]}));
    assert!(e.is_err_and(|e| e.to_string().contains("whole assembly")));
    let _ = std::fs::remove_dir_all(&d);
}

/// A part moved but not captured is exported where it is shown, as the URDF export does.
#[test]
fn mjcf_exports_uncaptured_moves() {
    let mut s = two_cubes();
    let b = occ(&s, "B");
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [0, 0, 30], "capture": false}));
    let d = temp("mjcf-pending");
    let path = d.join("m.xml");
    run(&mut s, "file.export", json!({"path": path.to_string_lossy(), "format": "mjcf"}));
    let m = Model::read(&std::fs::read_to_string(&path).unwrap_or_default());
    assert_near(&m.com("B", &[]), &[0.025, 0.005, 0.035], "B, 30 mm up");
    let _ = std::fs::remove_dir_all(&d);
}
