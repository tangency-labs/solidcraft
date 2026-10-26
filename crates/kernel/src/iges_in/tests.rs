use super::*;
use crate::{Body, BoolOp, ExportBody, StepHeader, boolean, box_solid, cylinder, fillet, iges_export_bodies, measure, sphere, torus};
use solidcraft_geom::Vec3;

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-9)
}

/// Export a body to IGES and read it back: same volume and area, its name and colour.
fn round_trip(b: &Body) -> crate::StepImport {
    let body = ExportBody { name: "Bracket left 1".into(), body: b, color: Some([1.0, 0.5, 0.0]) };
    let text = iges_export_bodies(&[body], &StepHeader { file_name: "t.igs".into(), ..Default::default() }).unwrap();
    for l in text.lines() {
        assert_eq!(l.chars().count(), 80, "{l}");
    }
    let imp = iges_import(&text).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert_eq!(imp.bodies.len(), 1);
    let (m0, m1) = (measure(b).unwrap(), measure(&imp.bodies[0].body).unwrap());
    assert!(rel(m1.volume, m0.volume) < 1e-3, "volume {} vs {}", m1.volume, m0.volume);
    assert!(rel(m1.area, m0.area) < 1e-3, "area {} vs {}", m1.area, m0.area);
    assert!(imp.bodies[0].closed);
    assert_eq!(imp.bodies[0].name, "Bracket left 1");
    assert_eq!(imp.bodies[0].color, Some([1.0, 0.5, 0.0]));
    imp
}

#[test]
fn primitives_round_trip_through_iges() {
    round_trip(&box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap());
    round_trip(&cylinder(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 1.0, 1.0), 5.0, 12.0).unwrap());
    round_trip(&sphere(Vec3::new(1.0, 2.0, 3.0), 10.0).unwrap());
    round_trip(&torus(Vec3::ZERO, 20.0, 4.0).unwrap());
}

#[test]
fn features_round_trip_through_iges() {
    let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 10.0)).unwrap();
    let pin = cylinder(Vec3::new(20.0, 15.0, -1.0), Vec3::Z, 5.0, 12.0).unwrap();
    let holed = boolean(&plate, &pin, BoolOp::Cut).unwrap().unwrap();
    round_trip(&holed);
    round_trip(&fillet(&plate, &[Vec3::new(20.0, 0.0, 10.0)], 3.0).unwrap());
}

/// Hostile files fail cleanly: garbage, truncation, missing and circular references, huge
/// counts.
#[test]
fn hostile_iges_never_panics() {
    let good = iges_export_bodies(
        &[ExportBody { name: "B".into(), body: &box_solid(Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0)).unwrap(), color: None }],
        &StepHeader::default(),
    )
    .unwrap();
    let mut cases: Vec<String> = vec![String::new(), "garbage".into(), "\u{0}\u{ff}".into(), good[..good.len() / 2].to_string()];
    // Every parameter line with its numbers bumped, scrambled or blown up.
    for (k, f) in [|s: &str| s.replace('1', "9"), |s: &str| s.replace(',', ";"), |s: &str| s.replace("0,", "999999999,")].iter().enumerate() {
        let t: String = good
            .lines()
            .map(|l| if l.as_bytes().get(72) == Some(&b'P') && l.len() >= 64 { format!("{}{}", f(&l[..64]), &l[64..]) } else { l.to_string() })
            .collect::<Vec<_>>()
            .join("\n");
        let _ = k;
        cases.push(t);
    }
    // A transformation and a composite curve that refer to themselves.
    cases.push(
        [
            format!("{:<72}S{:>7}", "", 1),
            format!("{:<72}G{:>7}", "1H,,1H;;", 1),
            format!("{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}D{:>7}", 124, 1, 0, 0, 0, 0, 1, 0, "00000000", 1),
            format!("{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}D{:>7}", 124, 0, 0, 1, 0, 0, 0, "", 0, 2),
            format!("{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}D{:>7}", 102, 2, 0, 0, 0, 0, 1, 0, "00000000", 3),
            format!("{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}D{:>7}", 102, 0, 0, 1, 0, 0, 0, "", 0, 4),
            format!("{:<64}{:>8}P{:>7}", "124,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.;", 1, 1),
            format!("{:<64}{:>8}P{:>7}", "102,1,3;", 3, 2),
            format!("{:<72}T{:>7}", "S      1G      1D      4P      2", 1),
        ]
        .join("\n"),
    );
    for c in cases {
        let r = std::panic::catch_unwind(|| iges_import(&c).map(|i| i.bodies.len()));
        assert!(r.is_ok(), "panicked on a hostile file");
    }
}

/// An IGES file from (type, form, parameters) entities, in order (pointers are 2k + 1).
fn iges_file(ents: &[(i64, i64, String)]) -> String {
    let placed: Vec<(i64, i64, String, i64)> = ents.iter().map(|(t, f, p)| (*t, *f, p.clone(), 0)).collect();
    iges_file_placed(&placed)
}

/// Like [`iges_file`], with each entity's transformation matrix pointer (0 for none).
fn iges_file_placed(ents: &[(i64, i64, String, i64)]) -> String {
    let mut out = vec![format!("{:<72}S{:>7}", "test", 1), format!("{:<72}G{:>7}", "1H,,1H;,,,,,,,,,,,1.,2,2HMM,,,,1.E-6;", 1)];
    let mut p_lines: Vec<(usize, String)> = Vec::new();
    let mut starts = Vec::new();
    for (k, (t, _, p, _)) in ents.iter().enumerate() {
        starts.push(p_lines.len() + 1);
        let text = format!("{t},{p};");
        let chars: Vec<char> = text.chars().collect();
        for c in chars.chunks(64) {
            p_lines.push((2 * k + 1, c.iter().collect()));
        }
    }
    for (k, (t, form, _, xf)) in ents.iter().enumerate() {
        let count = p_lines.iter().filter(|(d, _)| *d == 2 * k + 1).count();
        out.push(format!("{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}D{:>7}", t, starts[k], 0, 0, 0, 0, xf, 0, "00000000", 2 * k + 1));
        out.push(format!("{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}D{:>7}", t, 0, 0, count, form, 0, 0, "", 0, 2 * k + 2));
    }
    for (i, (d, l)) in p_lines.iter().enumerate() {
        out.push(format!("{l:<64}{d:>8}P{:>7}", i + 1));
    }
    out.push(format!("{:<72}T{:>7}", "", 1));
    out.join("\n")
}

/// A spherical cap written the way analytic IGES exports write one: a surface of revolution
/// (120) whose generatrix is a short circular arc (100) centred on the axis, turned into the
/// meridian plane by its transformation matrix (124), trimmed by a circle round the axis. The arc
/// is only the part of the circle the cap needs; read as the whole circle, the revolution was a
/// whole sphere twice over and the cap meshed as the full ball.
#[test]
fn an_arc_revolved_about_its_centre_is_a_cap_not_a_ball() {
    let (r, polar) = (50.0f64, 30f64.to_radians());
    let (rim_r, rim_z) = (r * polar.sin(), r * polar.cos());
    let ents: Vec<(i64, i64, String, i64)> = vec![
        // 1: the meridian plane: local x stays x, local y becomes z.
        (124, 0, "1.,0.,0.,0.,0.,0.,-1.,0.,0.,1.,0.,0.".into(), 0),
        // 3: the axis, 5: the arc from the rim (60° in its plane) to the pole (90°).
        (110, 0, "0.,0.,0.,0.,0.,1.".into(), 0),
        (100, 0, format!("0.,0.,0.,{:?},{:?},0.,{r:?}", rim_r, rim_z), 1),
        // 7: the surface, 9: the rim (a full circle at the cap's height), 11: on the surface.
        (120, 0, format!("3,5,0.,{:?}", std::f64::consts::TAU), 0),
        (100, 0, format!("{rim_z:?},0.,0.,{rim_r:?},0.,{rim_r:?},0."), 0),
        (142, 0, "0,7,0,9,2".into(), 0),
        // 13: the cap.
        (144, 0, "7,1,0,11".into(), 0),
    ];
    let imp = iges_import(&iges_file_placed(&ents)).unwrap();
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    let mesh = imp.bodies[0].body.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in &mesh.positions {
        for (k, x) in [p.x, p.y, p.z].into_iter().enumerate() {
            lo[k] = lo[k].min(x);
            hi[k] = hi[k].max(x);
        }
    }
    for k in 0..2 {
        assert!((lo[k] + rim_r).abs() < 0.1 && (hi[k] - rim_r).abs() < 0.1, "cap is {lo:?} .. {hi:?}");
    }
    assert!((lo[2] - rim_z).abs() < 0.1 && (hi[2] - r).abs() < 0.1, "cap is {lo:?} .. {hi:?}");
    // Area of a cap: 2π r h.
    let area = measure(&imp.bodies[0].body).unwrap().area;
    let cap = std::f64::consts::TAU * r * (r - rim_z);
    assert!(rel(area, cap) < 1e-3, "area {area} vs {cap}");
}

/// An exported box inside a named group and nested subfigures, placed twice. The second
/// placement composes its translation and scale with a directory transformation.
fn assembly_file(form: i64, scale: f64) -> String {
    let body = box_solid(Vec3::ZERO, Vec3::new(2.0, 3.0, 4.0)).unwrap();
    let text = iges_export_bodies(&[ExportBody { name: "Body1".into(), body: &body, color: None }], &StepHeader::default()).unwrap();
    let f = parse(&text).unwrap();
    let mut ids: Vec<_> = f.des.keys().copied().collect();
    ids.sort_unstable();
    let solid = *ids.iter().find(|&&s| f.des[&s].typ == 186).unwrap();
    let mut transforms: HashMap<_, _> = f.des.iter().map(|(seq, d)| (*seq, d.xform)).collect();
    let mut ents: Vec<_> = ids
        .iter()
        .enumerate()
        .map(|(k, seq)| {
            assert_eq!(*seq, 2 * k + 1);
            let d = &f.des[seq];
            let p = f.params[seq]
                .iter()
                .skip(1)
                .map(|s| if s.is_empty() || num(s).is_some() { s.clone() } else { format!("{}H{s}", s.chars().count()) })
                .collect::<Vec<_>>()
                .join(",");
            (d.typ, d.form, p)
        })
        .collect();
    let definition = 2 * ents.len() + 1;
    ents.push((308, 0, format!("0,8HLead rod,1,{solid}")));
    let first = 2 * ents.len() + 1;
    ents.push((408, 0, format!("{definition},10.,0.,0.,1.")));
    let rotation = 2 * ents.len() + 1;
    ents.push((124, 0, "0.,-1.,0.,100.,1.,0.,0.,0.,0.,0.,1.,0.".into()));
    let second = 2 * ents.len() + 1;
    ents.push((408, 0, format!("{definition},0.,20.,0.,{scale}")));
    transforms.insert(second, rotation as i64);
    let frame = 2 * ents.len() + 1;
    ents.push((308, 0, format!("1,5HFrame,2,{first},{second}")));
    let property = 2 * ents.len() + 1;
    ents.push((406, 15, "1,6HMotion".into()));
    ents.push((402, form, format!("1,{frame},0,1,{property}")));
    iges_file(&ents)
        .lines()
        .map(|line| {
            if line.as_bytes()[72] != b'D' {
                return line.to_string();
            }
            let seq: usize = line[73..80].trim().parse().unwrap();
            match transforms.get(&seq) {
                Some(xf) => format!("{}{:>8}{}", &line[..48], xf, &line[56..]),
                None => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn groups_and_subfigure_instances_keep_names_structure_and_placements() {
    for (form, scale) in [(1, 1.0), (7, 2.0), (14, 1.0), (15, 2.0)] {
        let imp = iges_import(&assembly_file(form, scale)).unwrap();
        assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
        assert_eq!(imp.bodies.len(), 2);
        assert_eq!(imp.tree.len(), 1);
        let motion = &imp.tree[0].children[0];
        assert_eq!(motion.name, "Motion");
        let frame = &motion.children[0];
        assert_eq!(frame.name, "Frame");
        assert_eq!(frame.children.len(), 2);
        assert!(frame.children.iter().all(|n| n.name == "Lead rod"));
        assert!(imp.bodies.iter().all(|b| b.path == ["Import", "Motion", "Frame", "Lead rod"]));
        assert_eq!(imp.bodies[0].source_item, imp.bodies[1].source_item);
        let a = measure(&imp.bodies[0].body).unwrap();
        let b = measure(&imp.bodies[1].body).unwrap();
        assert!(rel(a.volume, 24.0) < 1e-6 && rel(b.volume, 24.0 * scale.powi(3)) < 1e-6);
        assert!(a.bbox.min.dist(Vec3::new(10.0, 0.0, 0.0)) < 1e-6 && a.bbox.max.dist(Vec3::new(12.0, 3.0, 4.0)) < 1e-6);
        assert!(b.bbox.min.dist(Vec3::new(80.0 - 3.0 * scale, 0.0, 0.0)) < 1e-6);
        assert!(b.bbox.max.dist(Vec3::new(80.0, 2.0 * scale, 4.0 * scale)) < 1e-6);
    }
}

#[test]
fn invalid_assembly_references_and_counts_fail_before_geometry() {
    for (params, message) in [
        ("0,4HLoop,1,1".into(), "cyclic"),
        ("0,4HPart,1,999".into(), "missing entity"),
        ("0,4HPart,-1".into(), "negative IGES group size"),
        (format!("0,4HPart,{}", MAX_LIST + 1), "work limit"),
    ] {
        let error = iges_import(&iges_file(&[(308, 0, params)])).unwrap_err();
        assert!(matches!(&error, crate::KernelError::Invalid(text) if text.contains(message)), "{error}");
    }
    let mut chain = Vec::new();
    for k in 0..=MAX_DEPTH {
        chain.push((308, 0, if k == MAX_DEPTH { "0,4HPart,0".into() } else { format!("0,4HPart,1,{}", 2 * k + 3) }));
    }
    let error = iges_import(&iges_file(&chain)).unwrap_err();
    assert!(matches!(&error, crate::KernelError::Invalid(text) if text.contains("nesting is too deep")), "{error}");
    let members = "1,".repeat(100_000);
    let many = [(308, 0, "0,4HPart,0".into()), (308, 0, format!("1,5HFrame,100001,{members}1"))];
    let error = iges_import(&iges_file(&many)).unwrap_err();
    assert!(matches!(&error, crate::KernelError::Invalid(text) if text.contains("too many IGES assembly placements")), "{error}");
}

/// Six trimmed bilinear patches with line boundaries, forming a 10 × 20 × 30 box.
fn trimmed_box(ents: &mut Vec<(i64, i64, String)>, x: f64) -> Vec<usize> {
    let c = |i: usize| -> [f64; 3] {
        [x + if i & 1 != 0 { 10.0 } else { 0.0 }, if i & 2 != 0 { 20.0 } else { 0.0 }, if i & 4 != 0 { 30.0 } else { 0.0 }]
    };
    // Faces as corner quads (counter-clockwise seen from outside).
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    let mut faces = Vec::new();
    let fmt = |p: [f64; 3]| format!("{:?},{:?},{:?}", p[0], p[1], p[2]);
    for q in quads {
        let (a, b, cc, d) = (c(q[0]), c(q[1]), c(q[2]), c(q[3]));
        // Patch: u from a to b, v from a to d (normal (b - a) × (d - a) outward).
        let surf = format!("1,1,1,1,0,0,1,0,0,0.,0.,1.,1.,0.,0.,1.,1.,1.,1.,1.,1.,{},{},{},{},0.,1.,0.,1.", fmt(a), fmt(b), fmt(d), fmt(cc));
        ents.push((128, 0, surf));
        let s_de = 2 * ents.len() - 1;
        let mut lines = Vec::new();
        for (p0, p1) in [(a, b), (b, cc), (cc, d), (d, a)] {
            ents.push((110, 0, format!("{},{}", fmt(p0), fmt(p1))));
            lines.push(2 * ents.len() - 1);
        }
        ents.push((102, 0, format!("4,{}", lines.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","))));
        let comp = 2 * ents.len() - 1;
        ents.push((142, 0, format!("0,{s_de},0,{comp},2")));
        let cos = 2 * ents.len() - 1;
        ents.push((144, 0, format!("{s_de},1,0,{cos}")));
        faces.push(2 * ents.len() - 1);
    }
    faces
}

#[test]
fn trimmed_surfaces_sew_into_a_solid() {
    let mut ents = Vec::new();
    trimmed_box(&mut ents, 0.0);
    let imp = iges_import(&iges_file(&ents)).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert!(imp.bodies[0].closed);
    let m = measure(&imp.bodies[0].body).unwrap();
    assert!(rel(m.volume, 6000.0) < 1e-6 && rel(m.area, 2200.0) < 1e-6, "{} {}", m.volume, m.area);
}

/// One part whose surfaces make two separate boxes: two solids, not one shell of two lumps
/// (closed edge by edge, but not connected, so it was read as an open body).
#[test]
fn separate_lumps_of_one_part_are_separate_solids() {
    let mut ents = Vec::new();
    let mut faces = trimmed_box(&mut ents, 0.0);
    faces.extend(trimmed_box(&mut ents, 100.0));
    let members = faces.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(",");
    ents.push((308, 0, format!("0,5HFrame,{},{members}", faces.len())));
    let imp = iges_import(&iges_file(&ents)).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert_eq!(imp.bodies.len(), 2);
    for (body, x) in imp.bodies.iter().zip([0.0, 100.0]) {
        let m = measure(&body.body).unwrap();
        assert!(body.closed && rel(m.volume, 6000.0) < 1e-6, "{} {}", body.name, m.volume);
        assert!(m.bbox.min.dist(Vec3::new(x, 0.0, 0.0)) < 1e-6, "{} at {:?}", body.name, m.bbox.min);
    }
}

/// Trim pieces whose ends miss each other by a hundredth of a millimetre, far past the file's
/// stated resolution: the loop is closed and the box sews into a solid. A gap of half a
/// millimetre is no corner written loosely, and stays open.
#[test]
fn trim_loops_with_small_gaps_still_close() {
    let with_gap = |gap: f64| {
        let mut ents = Vec::new();
        trimmed_box(&mut ents, 0.0);
        // The edge from (0, 0, 0) towards (0, 20, 0) stops `gap` short of that corner, in both
        // faces that run along it (the first face's first line, the fifth face's last).
        ents[1].2 = format!("0.,0.,0.,0.,{:?},0.", 20.0 - gap);
        ents[36].2 = format!("0.,{:?},0.,0.,0.,0.", 20.0 - gap);
        iges_import(&iges_file(&ents))
    };
    let imp = with_gap(0.01).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert!(imp.bodies[0].closed);
    let m = measure(&imp.bodies[0].body).unwrap();
    assert!(rel(m.volume, 6000.0) < 1e-3 && rel(m.area, 2200.0) < 1e-3, "{} {}", m.volume, m.area);
    if let Ok(imp) = with_gap(0.5) {
        assert!(imp.bodies.iter().all(|b| !b.closed), "a 0.5 mm gap was closed");
    }
}

#[test]
fn surfaces_in_separate_groups_remain_separate_parts() {
    let mut ents = Vec::new();
    for (name, x) in [("Left", 0.0), ("Right", 100.0)] {
        let faces = trimmed_box(&mut ents, x);
        let members = faces.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(",");
        ents.push((308, 0, format!("0,{}H{name},{},{members}", name.len(), faces.len())));
    }
    let imp = iges_import(&iges_file(&ents)).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert_eq!(imp.bodies.len(), 2);
    assert_eq!(imp.tree[0].children.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["Left", "Right"]);
    for (body, x) in imp.bodies.iter().zip([0.0, 100.0]) {
        let m = measure(&body.body).unwrap();
        assert!(body.closed && rel(m.volume, 6000.0) < 1e-6 && rel(m.area, 2200.0) < 1e-6);
        assert!(m.bbox.min.dist(Vec3::new(x, 0.0, 0.0)) < 1e-6 && m.bbox.max.dist(Vec3::new(x + 10.0, 20.0, 30.0)) < 1e-6);
    }
}

/// The box of [`trimmed_surfaces_sew_into_a_solid`] with its edge (0, 0, 0)–(0, 20, 0) stopping
/// 0.01 mm short of its corner in both faces that use it, its next pieces starting at the
/// corner. The pieces of a trim curve run on from one another, so the edge ends at the corner the
/// other three edges there agree on, exactly: a closed box SMLib finds valid. The edge was left
/// short with the gap on the vertex's tolerance, 2000 times the file's resolution (#3845).
#[test]
fn trim_pieces_that_miss_their_corner_are_closed_onto_it() {
    let c =
        |i: usize| -> [f64; 3] { [if i & 1 != 0 { 10.0 } else { 0.0 }, if i & 2 != 0 { 20.0 } else { 0.0 }, if i & 4 != 0 { 30.0 } else { 0.0 }] };
    let short = [0.0, 19.99, 0.0];
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    let mut ents: Vec<(i64, i64, String)> = Vec::new();
    let fmt = |p: [f64; 3]| format!("{:?},{:?},{:?}", p[0], p[1], p[2]);
    for q in quads {
        let (a, b, cc, d) = (c(q[0]), c(q[1]), c(q[2]), c(q[3]));
        let surf = format!("1,1,1,1,0,0,1,0,0,0.,0.,1.,1.,0.,0.,1.,1.,1.,1.,1.,1.,{},{},{},{},0.,1.,0.,1.", fmt(a), fmt(b), fmt(d), fmt(cc));
        ents.push((128, 0, surf));
        let s_de = 2 * ents.len() - 1;
        let mut lines = Vec::new();
        for (p0, p1) in [(a, b), (b, cc), (cc, d), (d, a)] {
            // The short edge: its end at (0, 20, 0) is 0.01 short.
            let (p0, p1) = match (p0, p1) {
                ([0.0, 0.0, 0.0], [0.0, 20.0, 0.0]) => (p0, short),
                ([0.0, 20.0, 0.0], [0.0, 0.0, 0.0]) => (short, p1),
                x => x,
            };
            ents.push((110, 0, format!("{},{}", fmt(p0), fmt(p1))));
            lines.push(2 * ents.len() - 1);
        }
        ents.push((102, 0, format!("4,{}", lines.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","))));
        let comp = 2 * ents.len() - 1;
        ents.push((142, 0, format!("0,{s_de},0,{comp},2")));
        let cos = 2 * ents.len() - 1;
        ents.push((144, 0, format!("{s_de},1,0,{cos}")));
    }
    let imp = iges_import(&iges_file(&ents)).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert!(imp.bodies[0].closed);
    let body = &imp.bodies[0].body;
    let check = body.smlib().unwrap().check(solidcraft_smlib::CheckLevel::All).unwrap();
    assert!(check.is_valid(), "{check}");
    for v in &body.record().vertices {
        assert!(v.tolerance <= body.record().tolerance, "a vertex {:?} kept a gap in its tolerance {}", v.point, v.tolerance);
    }
    let m = measure(body).unwrap();
    assert!(rel(m.volume, 6000.0) < 1e-6 && rel(m.area, 2200.0) < 1e-6, "{} {}", m.volume, m.area);
}

#[test]
fn a_drill_point_round_trips_through_iges() {
    // A blind hole with a cone tip: the tip's face closes at its apex (its seam runs out to the
    // apex and straight back).
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
    let plate = box_solid(Vec3::new(-20.0, -15.0, 0.0), Vec3::new(20.0, 15.0, 10.0)).unwrap();
    let profile =
        Region2 { outer: Loop2::polygon(&[Vec2::new(0.0, 2.0), Vec2::new(4.0, 5.0), Vec2::new(4.0, 11.0), Vec2::new(0.0, 11.0)]), holes: vec![] };
    let plane = Plane { y: Vec3::Z, ..Plane::XZ }; // Positive profile heights are world +Z.
    let hole = crate::revolve(&plane, &[profile], Vec2::ZERO, Vec2::new(0.0, 1.0), std::f64::consts::TAU).unwrap().pop().unwrap();
    let drilled = boolean(&plate, &hole, BoolOp::Cut).unwrap().unwrap();
    let v = 12000.0 - std::f64::consts::PI * 16.0 * (5.0 + 1.0);
    assert!(rel(measure(&drilled).unwrap().volume, v) < 1e-3);
    round_trip(&drilled);
}

/// A composite curve (102) names its children by pointer, and nothing stops a tree whose every
/// node names the same child twice. Twenty levels under the depth cap is a million leaf visits
/// and thirty is 2^30; only counting the work sees that, because the depth never grows past the
/// cap and a node that yields no segment trips no bound on the output.
#[test]
fn composite_curve_fan_out_is_bounded() {
    let quad = |z: f64| {
        let c = [[0.0, 0.0, z], [10.0, 0.0, z], [10.0, 20.0, z], [0.0, 20.0, z]];
        let fmt = |p: [f64; 3]| format!("{:?},{:?},{:?}", p[0], p[1], p[2]);
        format!("1,1,1,1,0,0,1,0,0,0.,0.,1.,1.,0.,0.,1.,1.,1.,1.,1.,1.,{},{},{},{},0.,1.,0.,1.", fmt(c[0]), fmt(c[1]), fmt(c[3]), fmt(c[2]))
    };
    let mut ents: Vec<(i64, i64, String)> = vec![(128, 0, quad(0.0)), (110, 0, "0.,0.,0.,10.,0.,0.".into())];
    let (surface, mut child) = (1, 3);
    // Twenty composites, each naming the one below it twice.
    for _ in 0..20 {
        ents.push((102, 0, format!("2,{child},{child}")));
        child = 2 * ents.len() - 1;
    }
    ents.push((142, 0, format!("0,{surface},0,{child},2")));
    let cos = 2 * ents.len() - 1;
    ents.push((144, 0, format!("{surface},1,0,{cos}")));

    let reported = match iges_to_step(&iges_file(&ents)) {
        Ok((_, warnings)) => warnings.join(" "),
        Err(error) => error,
    };
    // Check the work-budget rejection, not elapsed time: CI runs test binaries concurrently.
    assert!(reported.contains("too complex") || reported.contains("too many segments"), "{reported}");
}

/// Nothing stops directory entries from all naming the same parameter range, and each one
/// materialises it as its own string and token list. Ten thousand entries over a million
/// shared lines retained about a gigabyte each.
#[test]
fn directory_entries_cannot_share_their_way_past_the_file_size() {
    let mut out = vec![format!("{:<72}S{:>7}", "test", 1), format!("{:<72}G{:>7}", "1H,,1H;,,,,,,,,,,,1.,2,2HMM,,,,1.E-6;", 1)];
    // 1,000 entries, each claiming the same 2,000 parameter lines.
    for k in 0..1_000 {
        out.push(format!("{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}D{:>7}", 110, 1, 0, 0, 0, 0, 0, 0, "00000000", 2 * k + 1));
        out.push(format!("{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}D{:>7}", 110, 0, 0, 2_000, 0, 0, 0, "", 0, 2 * k + 2));
    }
    for i in 0..2_000 {
        out.push(format!("{:<64}{:>8}P{:>7}", "0.,0.,0.,1.,0.,0.;", 1, i + 1));
    }
    out.push(format!("{:<72}T{:>7}", "", 1));

    let t = std::time::Instant::now();
    let error = iges_to_step(&out.join("\n")).unwrap_err();
    assert!(error.contains("parameter data is too large"), "{error}");
    assert!(t.elapsed() < std::time::Duration::from_secs(10), "took {:?}", t.elapsed());
}

/// A curve on a surface (142) for a trimmed patch: model-space lines through `corners` with the
/// pieces `reversed` written end to start (as some exporters write composite trim curves), and
/// the same loop in the patch's parameter space through `uv`. Returns the 142's pointer.
fn trim_loop(ents: &mut Vec<(i64, i64, String)>, surface: usize, corners: &[[f64; 3]], uv: &[[f64; 2]], reversed: &[usize]) -> usize {
    let fmt = |p: [f64; 3]| format!("{:?},{:?},{:?}", p[0], p[1], p[2]);
    let mut model = Vec::new();
    let mut param = Vec::new();
    for k in 0..corners.len() {
        let (p0, p1) = (corners[k], corners[(k + 1) % corners.len()]);
        let (p0, p1) = if reversed.contains(&k) { (p1, p0) } else { (p0, p1) };
        ents.push((110, 0, format!("{},{}", fmt(p0), fmt(p1))));
        model.push(2 * ents.len() - 1);
        let (q0, q1) = (uv[k], uv[(k + 1) % uv.len()]);
        ents.push((110, 0, format!("{},{}", fmt([q0[0], q0[1], 0.0]), fmt([q1[0], q1[1], 0.0]))));
        param.push(2 * ents.len() - 1);
    }
    let list = |v: &[usize]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
    ents.push((102, 0, format!("{},{}", model.len(), list(&model))));
    let c = 2 * ents.len() - 1;
    ents.push((102, 0, format!("{},{}", param.len(), list(&param))));
    let b = 2 * ents.len() - 1;
    // Created by projection, preferring the parameter-space curve (PREF 1).
    ents.push((142, 0, format!("0,{surface},{b},{c},1")));
    2 * ents.len() - 1
}

/// A bilinear B-spline patch through corners `a` (u, v = 0), `b` (u = 1), `c` (u, v = 1) and
/// `d` (v = 1).
fn bilinear_patch(a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3]) -> (i64, i64, String) {
    let fmt = |p: [f64; 3]| format!("{:?},{:?},{:?}", p[0], p[1], p[2]);
    (128, 0, format!("1,1,1,1,0,0,1,0,0,0.,0.,1.,1.,0.,0.,1.,1.,1.,1.,1.,1.,{},{},{},{},0.,1.,0.,1.", fmt(a), fmt(b), fmt(d), fmt(c)))
}

/// The box of `trimmed_surfaces_sew_into_a_solid` with some pieces of every trim curve written
/// end to start (the first piece on half the faces): each face's loop still closes and the box
/// is a solid.
#[test]
fn trim_curves_with_reversed_pieces_still_close_their_faces() {
    let c =
        |i: usize| -> [f64; 3] { [if i & 1 != 0 { 10.0 } else { 0.0 }, if i & 2 != 0 { 20.0 } else { 0.0 }, if i & 4 != 0 { 30.0 } else { 0.0 }] };
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    let mut ents: Vec<(i64, i64, String)> = Vec::new();
    for (fi, q) in quads.iter().enumerate() {
        let corners = q.map(c);
        ents.push(bilinear_patch(corners[0], corners[1], corners[2], corners[3]));
        let surface = 2 * ents.len() - 1;
        let reversed: &[usize] = if fi % 2 == 0 { &[0, 2] } else { &[1, 3] };
        let cos = trim_loop(&mut ents, surface, &corners, &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], reversed);
        ents.push((144, 0, format!("{surface},1,0,{cos}")));
    }
    let imp = iges_import(&iges_file(&ents)).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert!(imp.bodies[0].closed);
    let m = measure(&imp.bodies[0].body).unwrap();
    assert!(rel(m.volume, 6000.0) < 1e-6 && rel(m.area, 2200.0) < 1e-6, "{} {}", m.volume, m.area);
}

/// A plate with a square hole whose trim curve starts with a reversed piece: where the
/// parameter-space curve starts decides which way the hole runs, so it stays a hole.
#[test]
fn a_hole_whose_first_trim_piece_is_reversed_stays_a_hole() {
    let mut ents: Vec<(i64, i64, String)> = vec![bilinear_patch([0.0; 3], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0])];
    let outer = trim_loop(
        &mut ents,
        1,
        &[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0]],
        &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        &[],
    );
    // Clockwise about the patch's normal, as a hole runs.
    let hole = trim_loop(
        &mut ents,
        1,
        &[[3.0, 3.0, 0.0], [3.0, 7.0, 0.0], [7.0, 7.0, 0.0], [7.0, 3.0, 0.0]],
        &[[0.3, 0.3], [0.3, 0.7], [0.7, 0.7], [0.7, 0.3]],
        &[0],
    );
    ents.push((144, 0, format!("1,1,1,{outer},{hole}")));
    let imp = iges_import(&iges_file(&ents)).unwrap();
    assert_eq!(imp.bodies.len(), 1);
    assert!(!imp.warnings.iter().any(|w| w.contains("not read") || w.contains("face #")), "{:?}", imp.warnings);
    let m = measure(&imp.bodies[0].body).unwrap();
    assert!(rel(m.area, 84.0) < 1e-6, "area {}", m.area);
}

/// The box of `trimmed_surfaces_sew_into_a_solid` with one edge written as a B-spline (126)
/// over a knot span of 1e-7 or less, as some exporters write short curves: the STEP side read
/// that span as a single value and refused the curve, and both faces it bounds were lost.
#[test]
fn a_curve_over_a_tiny_knot_span_still_bounds_its_faces() {
    let c =
        |i: usize| -> [f64; 3] { [if i & 1 != 0 { 10.0 } else { 0.0 }, if i & 2 != 0 { 20.0 } else { 0.0 }, if i & 4 != 0 { 30.0 } else { 0.0 }] };
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    let fmt = |p: [f64; 3]| format!("{:?},{:?},{:?}", p[0], p[1], p[2]);
    for span in [1e-7f64, 1e-9, 1e-13] {
        let mut ents: Vec<(i64, i64, String)> = Vec::new();
        for (fi, q) in quads.iter().enumerate() {
            let (a, b, cc, d) = (c(q[0]), c(q[1]), c(q[2]), c(q[3]));
            let surf = format!("1,1,1,1,0,0,1,0,0,0.,0.,1.,1.,0.,0.,1.,1.,1.,1.,1.,1.,{},{},{},{},0.,1.,0.,1.", fmt(a), fmt(b), fmt(d), fmt(cc));
            ents.push((128, 0, surf));
            let s_de = 2 * ents.len() - 1;
            let mut lines = Vec::new();
            for (k, (p0, p1)) in [(a, b), (b, cc), (cc, d), (d, a)].into_iter().enumerate() {
                if fi == 0 && k == 0 {
                    ents.push((126, 0, format!("1,1,0,0,1,0,0.,0.,{span:E},{span:E},1.,1.,{},{},0.,{span:E},0.,0.,1.", fmt(p0), fmt(p1))));
                } else {
                    ents.push((110, 0, format!("{},{}", fmt(p0), fmt(p1))));
                }
                lines.push(2 * ents.len() - 1);
            }
            ents.push((102, 0, format!("4,{}", lines.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","))));
            let comp = 2 * ents.len() - 1;
            ents.push((142, 0, format!("0,{s_de},0,{comp},2")));
            let cos = 2 * ents.len() - 1;
            ents.push((144, 0, format!("{s_de},1,0,{cos}")));
        }
        let imp = iges_import(&iges_file(&ents)).unwrap();
        assert!(imp.warnings.is_empty(), "span {span}: {:?}", imp.warnings);
        assert!(imp.bodies[0].closed, "span {span}");
        let m = measure(&imp.bodies[0].body).unwrap();
        assert!(rel(m.volume, 6000.0) < 1e-6 && rel(m.area, 2200.0) < 1e-6, "span {span}: {} {}", m.volume, m.area);
    }
}

#[test]
fn the_global_sections_native_system_becomes_the_originating_system() {
    let imp = round_trip(&box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap());
    assert_eq!(imp.system, "SolidCraft");
}
