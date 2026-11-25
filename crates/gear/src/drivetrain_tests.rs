//! Drivetrains read from 3D geometry: trains with clusters, long trains, bevels, bearings and
//! per-member errors; and the flat sketch of the user's build 6 report.

use crate::V2;
use crate::drivetrain::{Ask, Circle3, Item, Line3, Outcome, Role, Thing, read};
use crate::mesh2d::{Verdict, check_mesh};
use crate::plan::{Kind, Spec};
use crate::simple::Simple;
use crate::sketch_rules::{Circle, Drawn, Reading};

fn circle(c: [f64; 3], n: [f64; 3], r: f64) -> Item {
    Item { thing: Thing::Circle(Circle3 { c, n, r }), ask: Ask::default() }
}
fn line(a: [f64; 3], b: [f64; 3]) -> Item {
    Item { thing: Thing::Line(Line3 { a, b }), ask: Ask::default() }
}
fn point(p: [f64; 3]) -> Item {
    Item { thing: Thing::Point(p), ask: Ask::default() }
}
const Z: [f64; 3] = [0.0, 0.0, 1.0];

fn clean(p: &crate::Plan, count: usize) {
    let r = check_mesh(p);
    assert_eq!(r.len(), count);
    for m in &r {
        assert_eq!(m.verdict, Verdict::Ok, "{}", m.message);
    }
}

fn planned(o: &Outcome) -> crate::Plan {
    assert!(o.error.is_none(), "{:?}", o.error);
    crate::plan(&o.solved.as_ref().unwrap().spec).unwrap()
}

/// Shafts A, B, C along z; gear 1 on A meshes gear 2 on B; gear 3 on B (20 mm up) is joined to
/// gear 2 and meshes gear 4 on C; a bearing point on A, and a dot off every shaft.
fn compound() -> Vec<Item> {
    vec![
        line([0.0, 0.0, -10.0], [0.0, 0.0, 40.0]),
        line([50.0, 0.0, -10.0], [50.0, 0.0, 40.0]),
        line([85.0, 0.0, 15.0], [85.0, 0.0, 35.0]),
        circle([0.0, 0.0, 0.0], Z, 20.0),
        circle([50.0, 0.0, 0.0], Z, 30.0),
        circle([50.0, 0.0, 20.0], Z, 15.0),
        circle([85.0, 0.0, 20.0], Z, 20.0),
        point([0.0, 0.0, -5.0]),
        point([100.0, 100.0, 100.0]),
    ]
}

#[test]
fn a_3d_compound_two_circles_on_one_shaft_meshing_two_others() {
    let o = read(&compound(), None, &Simple { face_width: Some(8.0), ..Simple::default() }, &Spec::default());
    let p = planned(&o);
    // One body per member that turns as one: each shaft with its gears.
    assert_eq!(p.bodies.len(), 3, "{:?}", p.bodies.iter().map(|b| &b.name).collect::<Vec<_>>());
    assert_eq!(p.shafts.len(), 3);
    clean(&p, 2);
    // Members: the circles on their shafts; 5 and 6 one cluster; meshes 4–5 and 6–7.
    let m = &o.members;
    assert_eq!((m[3].shaft, m[4].shaft, m[5].shaft, m[6].shaft), (Some(0), Some(1), Some(1), Some(2)));
    assert!(m[4].cluster.is_some() && m[4].cluster == m[5].cluster && m[3].cluster.is_none());
    assert_eq!((m[3].meshes_with.clone(), m[5].meshes_with.clone()), (vec![4], vec![6]));
    assert_eq!((m[7].role, m[7].shaft, m[8].role), ("bearing", Some(0), "point"));
    assert_eq!(o.seats, vec![[0.0, 0.0, -5.0]]);
    assert!(m.iter().all(|m| m.error.is_none()));
    // The plan: one cluster of gears 2 and 3, one body with their shaft; gear 4's shaft from
    // its drawn line's end.
    assert_eq!(p.clusters, vec![vec![1, 2]]);
    assert_eq!((p.members[1].body, p.members[2].body), (Some(1), Some(1)));
    assert!((p.bodies[2].frame.origin[2] - 15.0).abs() < 1e-9 && (p.bodies[2].frame.origin[0] - 85.0).abs() < 1e-9, "{:?}", p.bodies[2].frame);
    assert!((p.shafts[2].length - 20.0).abs() < 1e-9);
    let (z1, z2, z3, z4) = (m[3].teeth.unwrap(), m[4].teeth.unwrap(), m[5].teeth.unwrap(), m[6].teeth.unwrap());
    let ratio = p.facts.iter().find(|f| f.label == "train ratio gear 1 → gear 4").unwrap().value;
    assert!((ratio - f64::from(z2) / f64::from(z1) * f64::from(z4) / f64::from(z3)).abs() < 1e-12);
}

#[test]
fn per_member_module_and_teeth_are_kept_or_contradictions_named() {
    let mut items = compound();
    // Gear 3 asks module 1.25 (24 teeth on Ø30): its group (gears 3 and 4) follows.
    items[5].ask.module = Some(1.25);
    let o = read(&items, None, &Simple { face_width: Some(8.0), ..Simple::default() }, &Spec::default());
    planned(&o);
    assert_eq!((o.members[5].teeth, o.members[6].teeth, o.members[6].module), (Some(24), Some(32), Some(1.25)));
    // Gear 4 asks another module: both named.
    items[6].ask.module = Some(2.0);
    let o = read(&items, None, &Simple::default(), &Spec::default());
    let e = o.members[6].error.clone().unwrap();
    assert!(e.contains("member 7 asks module 2") && e.contains("member 6"), "{e}");
    assert!(o.solved.is_none() && o.error.is_some());
    // Teeth and module both asked off the circle (Ø45 on Ø40): kept, the drawn centre distance
    // held by profile shift (decision 1).
    let mut items = compound();
    items[3].ask = Ask { module: Some(2.5), teeth: Some(18), ..Ask::default() };
    let o = read(&items, None, &Simple::default(), &Spec::default());
    assert!(o.members[3].error.is_none(), "{:?}", o.members[3].error);
    assert_eq!((o.members[3].teeth, o.members[3].module), (Some(18), Some(2.5)));
    assert!(o.notes.iter().any(|n| n.contains("Ø40 drawn, Ø45 built")), "{:?}", o.notes);
}

#[test]
fn a_ten_gear_train() {
    // Ten gears in a row along x, alternately Ø40 and Ø24, each touching the next.
    let radii: Vec<f64> = (0..10).map(|i| if i % 2 == 0 { 20.0 } else { 12.0 }).collect();
    let mut x = 0.0;
    let mut circles = Vec::new();
    for (i, r) in radii.iter().enumerate() {
        if i > 0 {
            x += radii[i - 1] + r;
        }
        circles.push(Circle { c: V2::new(x, 0.0), r: *r });
    }
    // Flat, through the sketch reader.
    let d = Drawn { points: vec![], circles: circles.clone(), lines: vec![] };
    let Ok(Reading::Train { circles: picked }) = crate::sketch_rules::read(Some(Kind::Spur), &d) else { panic!() };
    assert_eq!(picked.len(), 10);
    let (spec, _) = crate::sketch_rules::train_spec(&circles, Kind::Spur, &Simple::default(), &Spec::default()).unwrap();
    let p = crate::plan(&spec).unwrap();
    assert_eq!(p.bodies.len(), 10);
    clean(&p, 9);
    // Gear 10 turns -z1/z10 per turn of gear 1 (nine outside meshes).
    let t = spec.train.as_ref().unwrap();
    let want = -f64::from(t.gears[0].teeth) / f64::from(t.gears[9].teeth);
    assert!((p.preview[9].speed - want).abs() < 1e-12, "{} vs {want}", p.preview[9].speed);
    assert!((p.assembly.ratio(0, 9).unwrap() - want).abs() < 1e-12);
    // The same in 3D, on a plane tilted to the world.
    let tilt = [0.0, -0.6, 0.8];
    let x_dir = [1.0, 0.0, 0.0];
    let y_dir = [0.0, 0.8, 0.6];
    let items: Vec<Item> = circles
        .iter()
        .map(|c| circle([c.c.x * x_dir[0] + c.c.y * y_dir[0], c.c.x * x_dir[1] + c.c.y * y_dir[1], c.c.x * x_dir[2] + c.c.y * y_dir[2]], tilt, c.r))
        .collect();
    let o = read(&items, None, &Simple::default(), &Spec::default());
    let p = planned(&o);
    clean(&p, 9);
    assert!(o.members.iter().all(|m| m.error.is_none() && m.teeth.is_some()));
    assert_eq!(o.members[4].meshes_with, vec![3, 5]);
}

#[test]
fn shafts_meeting_make_a_bevel_pair_and_the_circles_must_touch() {
    // Gear 1 on the z axis 30 below the apex, gear 2 on the x axis 20 out: they touch at
    // (20, 0, -30).
    let items = vec![
        line([0.0; 3], [0.0, 0.0, -60.0]),
        line([0.0; 3], [60.0, 0.0, 0.0]),
        circle([0.0, 0.0, -30.0], Z, 20.0),
        circle([20.0, 0.0, 0.0], [1.0, 0.0, 0.0], 30.0),
    ];
    let o = read(&items, None, &Simple::default(), &Spec::default());
    let p = planned(&o);
    let spec = &o.solved.as_ref().unwrap().spec;
    assert_eq!(spec.kind, Kind::Bevel);
    assert!((spec.shaft_angle - 90.0).abs() < 1e-9);
    assert!((f64::from(spec.teeth2) / f64::from(spec.teeth) - 1.5).abs() < 1e-12);
    assert_eq!(p.bodies.len(), 2);
    assert_eq!((o.members[2].meshes_with.clone(), o.members[2].shaft), (vec![3], Some(0)));
    // Helical teeth asked: spiral bevels.
    let o = read(&items, Some(Kind::Helical), &Simple::default(), &Spec::default());
    assert_eq!(o.solved.as_ref().unwrap().spec.kind, Kind::SpiralBevel);
    // Circles that don't meet.
    let mut apart = items.clone();
    apart[3] = circle([20.0, 0.0, 0.0], [1.0, 0.0, 0.0], 25.0);
    let o = read(&apart, None, &Simple::default(), &Spec::default());
    assert!(o.members[3].error.as_deref().unwrap_or_default().contains("don't touch"), "{:?}", o.members);
}

#[test]
fn per_member_errors_name_the_member() {
    // A circle moved off its place (touching nothing now); a bearing point off every line; a
    // style mix.
    let mut items = compound();
    items[6] = circle([85.0, 5.0, 20.0], Z, 20.0);
    items[8].ask.role = Some(Role::Bearing);
    let o = read(&items, None, &Simple::default(), &Spec::default());
    assert!(o.members[6].error.as_deref().unwrap_or_default().contains("member 7 touches no other gear"), "{:?}", o.members[6]);
    assert_eq!(o.members[6].shaft, None);
    assert!(o.members[8].error.as_deref().unwrap_or_default().contains("member 9: a bearing must lie on a shaft line"), "{:?}", o.members[8]);
    assert!(o.members[3].error.is_none() && o.solved.is_none());
    let e = o.error.unwrap();
    assert!(e.contains("member 7") && e.contains("member 9"), "{e}");
    let mut items = compound();
    items[4].ask.role = Some(Role::Gear(Kind::Helical));
    let o = read(&items, None, &Simple { face_width: Some(8.0), ..Simple::default() }, &Spec::default());
    assert!(o.members.iter().any(|m| m.error.as_deref().unwrap_or_default().contains("share one tooth style")), "{:?}", o.members);
}

#[test]
fn a_gear_on_a_selected_shaft_line_with_its_size() {
    // Two parallel shaft lines, one asked for a Ø40 gear, the other a circle Ø60 touching it.
    let mut items = vec![line([0.0; 3], [0.0, 0.0, 30.0]), circle([50.0, 0.0, 5.0], Z, 30.0), line([50.0, 0.0, 0.0], [50.0, 0.0, 30.0])];
    items[0].ask = Ask { pitch_diameter: Some(40.0), along: Some(5.0), ..Ask::default() };
    let o = read(&items, None, &Simple::default(), &Spec::default());
    let p = planned(&o);
    clean(&p, 1);
    assert_eq!((o.members[0].role, o.members[0].shaft, o.members[1].shaft), ("spur", Some(0), Some(2)));
    // The gear and its shaft are one body, from the shaft line's first end.
    assert_eq!(p.bodies.len(), 2);
    assert!(p.bodies[0].frame.origin[2].abs() < 1e-9 && (p.shafts[0].length - 30.0).abs() < 1e-9);
}

/// The user's build 6 sketch: three circles on XZ, the first two touching, the third touching
/// nothing (its constraint missing); construction lines join the centres (the engine leaves
/// construction geometry out). Gear 3 is named, gears 1 and 2 are sized.
#[test]
fn build_6_sketch_names_the_circle_that_meshes_with_nothing() {
    let circles = vec![
        Circle { c: V2::new(0.0, 0.0), r: 15.0 },
        Circle { c: V2::new(-10.180755625393433, 20.064949911424925), r: 7.5 },
        Circle { c: V2::new(12.062125004376336, 21.214432234647948), r: 7.5 },
    ];
    let d = Drawn {
        points: vec![V2::new(-6.787170416928201, 13.376633274281524), V2::new(0.0, 21.214432234647948)],
        circles: circles.clone(),
        lines: vec![],
    };
    let Ok(Reading::Train { .. }) = crate::sketch_rules::read(Some(Kind::Spur), &d) else { panic!("not a train") };
    let e = crate::sketch_rules::train_spec(&circles, Kind::Spur, &Simple::default(), &Spec::default()).unwrap_err();
    assert!(e.starts_with("gear 3 touches no other gear") && !e.contains("gear 1:") && !e.contains("gear 2:"), "{e}");
    // Touching, the three make a train.
    let fixed = vec![circles[0], circles[1], Circle { c: V2::new(0.0, 22.5).rot(-0.5), r: 7.5 }];
    let (spec, _) = crate::sketch_rules::train_spec(&fixed, Kind::Spur, &Simple::default(), &Spec::default()).unwrap();
    clean(&crate::plan(&spec).unwrap(), 2);
}

/// A sketch's flat geometry in model space: (x, y) on the plane through `origin` with axes
/// `x` and `y` (its normal x × y).
fn on_plane(origin: [f64; 3], x: [f64; 3], y: [f64; 3], p: (f64, f64)) -> [f64; 3] {
    [0, 1, 2].map(|i| origin[i] + p.0 * x[i] + p.1 * y[i])
}

/// One gear system in three ordinary sketches: gears 1 and 2 in a sketch on z = 0, gears 3 and
/// 4 in a sketch on z = 20 (gear 3 on gear 2's axis), and the shared shaft line in a third
/// sketch on the XZ plane. Read together: a compound of gears 2 and 3 on that shaft.
#[test]
fn a_compound_from_two_sketches_on_parallel_planes_and_a_shaft_line_in_a_third() {
    let (x, y) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let a = |p| on_plane([0.0; 3], x, y, p);
    let b = |p| on_plane([0.0, 0.0, 20.0], x, y, p);
    // The third sketch is on XZ: its x along world X, its y along world -Z... any plane holding
    // the line; here (u, v) -> (u, 0, v).
    let c = |p: (f64, f64)| [p.0, 0.0, p.1];
    let items = vec![
        circle(a((0.0, 0.0)), Z, 20.0),
        circle(a((50.0, 0.0)), Z, 30.0),
        circle(b((50.0, 0.0)), Z, 15.0),
        circle(b((85.0, 0.0)), Z, 20.0),
        line(c((50.0, -10.0)), c((50.0, 40.0))),
        point(c((50.0, 30.0))),
    ];
    let o = read(&items, None, &Simple { face_width: Some(8.0), ..Simple::default() }, &Spec::default());
    let p = planned(&o);
    clean(&p, 2);
    let m = &o.members;
    assert!(m[1].cluster.is_some() && m[1].cluster == m[2].cluster && m[0].cluster.is_none() && m[3].cluster.is_none());
    assert_eq!((m[1].shaft, m[2].shaft, m[0].shaft), (Some(4), Some(4), None));
    assert_eq!((m[5].role, m[5].shaft), ("bearing", Some(4)));
    assert_eq!(p.clusters, vec![vec![1, 2]]);
    assert_eq!(p.members[1].body, p.members[2].body);
    // Without the shaft line, the shared axis alone makes the cluster.
    let o = read(&items[..4], None, &Simple { face_width: Some(8.0), ..Simple::default() }, &Spec::default());
    planned(&o);
    assert!(o.members[1].cluster.is_some() && o.members[1].cluster == o.members[2].cluster);
}

/// A synthetic copy of the user's RC gearbox (five sketches on offset planes, read in model
/// space): a Ø30 input driving two Ø15 countershaft gears (centres 35 apart, 22.5 from the
/// input), a 15 mm shaft line on each countershaft's axis to a second plane where each carries a
/// Ø25 (compound clusters), both Ø25s driving one Ø15 output (a power-split loop). The Ø36 round
/// the output and the Ø10 on the input's axis are the user's own checks, not picked.
fn gearbox() -> Vec<Item> {
    let h = 200f64.sqrt();
    let out_z = h + (400.0f64 - 17.5 * 17.5).sqrt();
    let (down, up) = ([0.0, -1.0, 0.0], [0.0, 1.0, 0.0]);
    vec![
        circle([0.0, 0.0, 0.0], down, 15.0),
        circle([17.5, 0.0, h], down, 7.5),
        circle([-17.5, 0.0, h], down, 7.5),
        circle([17.5, 15.0, h], up, 12.5),
        circle([-17.5, 15.0, h], up, 12.5),
        circle([0.0, 15.0, out_z], up, 7.5),
        line([17.5, 0.0, h], [17.5, 15.0, h]),
        line([-17.5, 0.0, h], [-17.5, 15.0, h]),
    ]
}

#[test]
fn the_rc_gearbox_power_split_with_clusters() {
    let items = gearbox();
    let o = read(&items, None, &Simple::default(), &Spec::default());
    let p = planned(&o);
    // The loop closes (one countershaft turned on its shaft), every mesh clean.
    clean(&p, 4);
    let m = &o.members;
    assert!(m.iter().all(|m| m.error.is_none()), "{:?}", m);
    // The automatic module (smallest gear 17+ teeth): 0.8 for both stages; stage 1 38 and 19 teeth
    // (Ø30 and Ø15, 22.5 apart), stage 2 31 and 19 (Ø25 and Ø15, 20 apart), held by shift.
    assert_eq!((m[0].teeth, m[1].teeth, m[2].teeth), (Some(38), Some(19), Some(19)));
    assert_eq!((m[3].teeth, m[4].teeth, m[5].teeth), (Some(31), Some(31), Some(19)));
    assert_eq!((m[0].module, m[3].module), (Some(0.8), Some(0.8)));
    assert!(m[1].cluster.is_some() && m[1].cluster == m[3].cluster && m[2].cluster == m[4].cluster && m[1].cluster != m[2].cluster);
    assert_eq!((m[1].shaft, m[3].shaft, m[2].shaft, m[4].shaft), (Some(6), Some(6), Some(7), Some(7)));
    assert_eq!(m[5].meshes_with, vec![3, 4]);
    // Faces fit the 15 mm between the gears on each countershaft.
    assert!(m.iter().filter_map(|m| m.face_width).all(|f| f <= 13.5 + 1e-9), "{:?}", m.iter().map(|m| m.face_width).collect::<Vec<_>>());
    let ratio = p.facts.iter().find(|f| f.label == "train ratio gear 1 → gear 6").unwrap().value;
    // (19 ÷ 38) × (19 ÷ 31): the drawn 0.3, rounded to whole teeth.
    assert!((ratio - 19.0 / 38.0 * 19.0 / 31.0).abs() < 1e-12, "{ratio}");
    assert_eq!(p.clusters.len(), 2);
    // Auto: spur on every gear, worked out; four shafts (input, two countershafts, output), each
    // one body with its gears; a contact line per mesh.
    assert!(m[..6].iter().all(|m| m.role == "spur" && m.inferred), "{:?}", m.iter().map(|m| m.role).collect::<Vec<_>>());
    assert_eq!(p.shafts.len(), 4);
    assert_eq!(p.bodies.len(), 4);
    assert_eq!(p.contacts.len(), 4);
    // The countershafts run exactly along their drawn lines (15 mm).
    assert!(p.shafts.iter().filter(|s| s.gears.len() == 2).all(|s| (s.length - 15.0).abs() < 1e-9), "{:?}", p.shafts);
    // Picked as gears, a Ø36 round the output (one axis, one plane) and the output are named.
    let mut both = gearbox();
    let h = 200f64.sqrt();
    both.push(circle([0.0, 15.0, h + (400.0f64 - 17.5 * 17.5).sqrt()], up_dir(), 18.0));
    let o = read(&both, None, &Simple::default(), &Spec::default());
    assert!(o.members[8].error.as_deref().unwrap_or_default().contains("concentric in one plane"), "{:?}", o.members[8]);
    assert!(o.members[5].error.as_deref().unwrap_or_default().contains("concentric in one plane"));
}

fn up_dir() -> [f64; 3] {
    [0.0, 1.0, 0.0]
}

/// V (chevron) spiral bevels: each member two spiral halves of opposite hand joined at
/// mid-face, the mate mirrored; checked in 3D (no flank crossing, contact all round, and a
/// collision half a pitch out of phase) at 90° and at 60°.
#[test]
fn chevron_bevels_mesh_at_90_and_60_degrees() {
    for (label, s) in [
        ("chevron 90deg 17:34 35deg", Spec { herringbone: true, ..Spec::defaults_for(Kind::SpiralBevel) }),
        (
            "chevron 60deg 19:28 30deg",
            Spec { herringbone: true, shaft_angle: 60.0, teeth: 19, teeth2: 28, spiral_angle: 30.0, ..Spec::defaults_for(Kind::SpiralBevel) },
        ),
    ] {
        let p = crate::plan(&s).unwrap_or_else(|e| panic!("{label}: {e}"));
        // Each member's teeth are two groups (one per half, each a taper trimmed to its side of
        // the mid-face sphere), the second joined to the first.
        for b in &p.bodies {
            let halves = b
                .steps
                .iter()
                .filter(
                    |st| matches!(st, crate::plan::Step::Group { steps, .. } if steps.iter().any(|x| matches!(x, crate::plan::Step::Taper { .. }))),
                )
                .count();
            assert_eq!(halves, 2, "{label}: {} has {halves} V halves", b.name);
        }
        crate::mesh3d_tests::measure(label, &s).check();
    }
}

/// V teeth on a wheel near a crown gear (12:40 at 90°, the wheel's cone at 73°: the RC 4x4's
/// wheel bevels in a herringbone block). The halves are split on the sphere about the apex
/// through mid-face, not on flat sections (which left the wheel a groove most of its face wide
/// and each half a sliver): the wheel keeps its V teeth and about the plain spiral wheel's
/// volume, its two halves turned opposite ways either side of mid-face, and the pair meshes.
#[test]
fn chevron_teeth_on_a_near_crown_wheel_keep_their_volume() {
    use crate::mesh3d::{Solid, overlap};
    let s = Spec { module: 1.25, teeth: 12, teeth2: 40, face_width: 10.0, spiral_angle: 20.0, ..Spec::defaults_for(Kind::SpiralBevel) };
    let v = crate::plan(&Spec { herringbone: true, ..s.clone() }).unwrap();
    let plain = crate::plan(&s).unwrap();
    assert!(!v.warnings.iter().any(|w| w.contains("V teeth")), "{:?}", v.warnings);
    let vol = |b: &crate::plan::Body| {
        let sol = Solid::new(b, 0.03);
        overlap(&sol, &sol, 0.08)
    };
    for (a, b) in v.bodies.iter().zip(&plain.bodies) {
        let (va, vb) = (vol(a), vol(b));
        assert!((va / vb - 1.0).abs() < 0.03, "{}: V {va:.0} mm³, plain {vb:.0} mm³", a.name);
    }
    crate::mesh3d_tests::measure("chevron 90deg 12:40 20deg", &Spec { herringbone: true, ..s }).check();
}

/// A selected shaft given no size between two gears carries the gear filling the spacing to
/// them (here Ø60 between a Ø40 gear 50 mm away and a Ø30 one 45 mm away): the three are one
/// train. A shaft carrying a gear, or asked to be a shaft, stays as it is.
#[test]
fn an_unsized_shaft_between_two_gears_gets_the_gear_that_fills_the_spacing() {
    let mut items = vec![
        Item { ask: Ask { pitch_diameter: Some(40.0), ..Ask::default() }, ..line([0.0; 3], [0.0, 0.0, 1.0]) },
        line([50.0, 0.0, 0.0], [50.0, 0.0, 1.0]),
        circle([50.0, 45.0, 0.0], Z, 15.0),
        Item { ask: Ask { role: Some(Role::Shaft), ..Ask::default() }, ..line([-60.0, 0.0, 0.0], [-60.0, 0.0, 1.0]) },
    ];
    let notes = crate::drivetrain::fill_unsized_shafts(&mut items);
    assert_eq!(notes, vec!["member 2: Ø60 fills the spacing to member 3".to_string()]);
    assert_eq!(items[1].ask.pitch_diameter, Some(60.0));
    assert_eq!(items[3].ask.pitch_diameter, None);
    let o = read(&items[..3], None, &Simple::default(), &Spec::default());
    assert!(o.error.is_none(), "{:?}", o.error);
    assert_eq!(o.members[1].meshes_with.len(), 2, "{:?}", o.members[1]);
    // Room for no gear (the spacing taken up): nothing filled.
    let mut tight = vec![circle([0.0; 3], Z, 20.0), line([15.0, 0.0, 0.0], [15.0, 0.0, 1.0])];
    assert!(crate::drivetrain::fill_unsized_shafts(&mut tight).is_empty());
}
