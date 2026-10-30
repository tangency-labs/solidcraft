//! Cursor distance is minimized within the design's freedom; lengths stay mandatory.
#[path = "support/four_bar.rs"]
mod four_bar;

use serde_json::json;
use solidcraft_geom::Vec2;

#[test]
fn angular_drag_keeps_the_horizontal_construction_line() {
    for live in [false, true] {
        let mut s = solidcraft_engine::Session::default();
        for (id, params) in [
            ("sketch.create", json!({"plane":"XY"})),
            ("sketch.line", json!({"points":[[0,0],[20,0]],"ids":["reference"],"construction":true})),
            ("sketch.constraint.horizontal_vertical", json!({"line":"reference"})),
            ("sketch.line", json!({"points":["reference.start",[10,10]],"ids":["arm"],"infer":false})),
            ("sketch.dimension", json!({"entities":["reference","arm"],"value":45})),
            ("sketch.drag", json!({"entity":"arm.end","from":[10,10],"to":[10,10.5],"live":live})),
        ] {
            s.execute(id, &params).unwrap();
        }
        if live {
            s.execute("sketch.drag_end", &json!({})).unwrap();
        }
        let point = |id| four_bar::point(&s, id).unwrap();
        assert!(point("reference.start").dist(point("reference.end")) > 1.0, "the construction reference must remain visible");
        let end = point("arm.end");
        assert!((end.x - end.y).abs() < 1e-5, "the arm must retain its 45 degree angle: {end:?}");
    }
}

/// A line held at its midpoint, with an angle to a horizontal line from that point (#8944):
/// dragging one of its ends off that angle slides the end along the line as far as it can go,
/// and the horizontal line keeps its length. That line's far end is free only along it, which
/// can't bring the cursor closer, yet the cursor correction moved it a whole step at a time on
/// rounding: the line shrank, for the first target to nothing.
#[test]
fn a_midpoint_line_dragged_off_its_angle_leaves_the_horizontal_line_alone() {
    for live in [false, true] {
        for (end, to, reached) in
            [("l1.end", [14.0, 3.0], [11.617647, 6.970588]), ("l1.end", [5.0, 12.0], [8.970588, 5.382353]), ("l1.start", [-3.0, -12.0], [-7.5, -4.5])]
        {
            let mut s = solidcraft_engine::Session::default();
            for (id, params) in [
                ("sketch.create", json!({"plane":"XY"})),
                ("sketch.line", json!({"points":[[-10,-6],[10,6]],"infer":false})),
                ("sketch.constraint.midpoint", json!({"line":"l1","point":"origin"})),
                ("sketch.line", json!({"points":["origin",[15,0]],"infer":false})),
                ("sketch.constraint.horizontal_vertical", json!({"line":"l2"})),
                ("sketch.dimension", json!({"entities":["l1","l2"],"type":"angle"})),
            ] {
                s.execute(id, &params).unwrap();
            }
            let from = four_bar::point(&s, end).unwrap();
            s.execute("sketch.drag", &json!({"entity":end,"from":[from.x,from.y],"to":to,"live":live})).unwrap();
            if live {
                s.execute("sketch.drag_end", &json!({})).unwrap();
            }
            let point = |id| four_bar::point(&s, id).unwrap();
            let at = point(end);
            assert!(at.dist(Vec2::new(reached[0], reached[1])) < 1e-4, "{end} to {to:?} (live {live}) went to {at:?}");
            let far = point("l2.end");
            assert!(far.dist(Vec2::new(15.0, 0.0)) < 1e-6, "{end} to {to:?} (live {live}) moved l2's end to {far:?}");
        }
    }
}

fn drag(s: &mut solidcraft_engine::Session, target: Vec2) {
    let from = four_bar::point(s, "crank.end").unwrap();
    s.execute("sketch.drag", &json!({"entity":"crank.end","from":[from.x,from.y],"to":[target.x,target.y],"live":true})).unwrap();
}

#[test]
fn four_bar_large_off_locus_drag_reaches_the_closest_feasible_point() {
    for target in [Vec2::new(-10.0, 8.0), Vec2::new(-10.0, -8.0), Vec2::new(20.0, 20.0), Vec2::new(20.0, 0.0), Vec2::new(1000.0, -1000.0)] {
        let mut s = four_bar::session().unwrap();
        let side = four_bar::branch(&s).unwrap();
        drag(&mut s, target);
        let actual = four_bar::point(&s, "crank.end").unwrap();
        let nearest = target * (10.0 / target.len());
        assert!(actual.dist(nearest) < 1e-4, "target {target:?}, actual {actual:?}, expected {nearest:?}");
        assert!(four_bar::length_error(&s).unwrap() < 1e-6);
        assert_eq!(four_bar::branch(&s).unwrap(), side);
        let state = s.model.state();
        let report = &state.sketch(s.active_sketch.unwrap()).unwrap().report;
        assert!(report.ok());
        assert_eq!(report.dof, 1);
        s.execute("sketch.drag_end", &json!({})).unwrap();
        assert!(four_bar::point(&s, "crank.end").unwrap().dist(nearest) < 1e-4);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(four_bar::point(&s, "crank.end").unwrap().dist(Vec2::new(-10.0, 0.0)) < 1e-7);
    }
}

#[test]
fn four_bar_reverse_motion_and_stationary_cursor_do_not_drift() {
    let mut s = four_bar::session().unwrap();
    let side = four_bar::branch(&s).unwrap();
    for target in [Vec2::new(-10.0, 16.0), Vec2::new(-10.0, -16.0), Vec2::new(0.0, 25.0), Vec2::new(0.0, -25.0)] {
        drag(&mut s, target);
        let at = four_bar::point(&s, "crank.end").unwrap();
        assert!(
            at.dist(target * (10.0 / target.len())) < 1e-4,
            "target {target:?}, actual {at:?}, length error {}",
            four_bar::length_error(&s).unwrap()
        );
        for _ in 0..8 {
            drag(&mut s, target);
            assert!(four_bar::point(&s, "crank.end").unwrap().dist(at) < 1e-4);
            assert!(four_bar::length_error(&s).unwrap() < 1e-6);
            assert_eq!(four_bar::branch(&s).unwrap(), side);
        }
    }
}

#[test]
fn four_bar_one_event_and_split_moves_agree_and_keep_the_branch() {
    let target = Vec2::new(-10.0, 16.0);
    let mut single = four_bar::session().unwrap();
    let mut split = four_bar::session().unwrap();
    let start = four_bar::point(&single, "crank.end").unwrap();
    let side = four_bar::branch(&single).unwrap();
    drag(&mut single, target);
    for i in 1..=16 {
        drag(&mut split, start + (target - start) * (i as f64 / 16.0));
        assert!(four_bar::length_error(&split).unwrap() < 1e-6);
        assert_eq!(four_bar::branch(&split).unwrap(), side);
    }
    assert!(four_bar::point(&single, "crank.end").unwrap().dist(four_bar::point(&split, "crank.end").unwrap()) < 1e-4);
}

#[test]
fn four_bar_live_cursor_motion_updates_driven_dimensions_before_release() {
    let mut s = four_bar::session().unwrap();
    s.execute("sketch.dimension", &json!({"entities": ["crank"], "type": "horizontal", "driven": true})).unwrap();
    let id = s.active_sketch.unwrap();
    let dimension = s.doc.sketch(id).unwrap().constraints.last().unwrap().id.clone();
    drag(&mut s, Vec2::new(-10.0, 8.0));
    let at = four_bar::point(&s, "crank.end").unwrap();
    assert!(at.dist(Vec2::new(-10.0, 8.0) * (10.0 / 164.0_f64.sqrt())) < 1e-4);
    let state = s.model.state();
    for sketch in [s.doc.sketch(id).unwrap(), &state.sketch(id).unwrap().sketch] {
        let c = sketch.constraints.iter().find(|c| c.id == dimension).unwrap();
        assert!(c.driven);
        assert!((c.kind.value().unwrap() - at.x.abs()).abs() < 1e-6, "reference dimension must measure the final live pose: {:?}", c.kind);
    }
}

#[test]
fn four_bar_other_joint_follows_the_rocker_arc_and_its_limits() {
    let base = Vec2::new(42.0, 0.0);
    let lo = -(-1264.0_f64 / 1680.0).acos();
    let hi = -(0.2_f64).acos();
    for angle in [-2.6_f64, -2.1, -1.5, -1.1] {
        let mut s = four_bar::session().unwrap();
        let side = four_bar::branch(&s).unwrap();
        let from = four_bar::point(&s, "coupler.end").unwrap();
        let target = base + Vec2::new(angle.cos(), angle.sin()) * 60.0;
        let nearest = base + Vec2::new(angle.clamp(lo, hi).cos(), angle.clamp(lo, hi).sin()) * 20.0;
        s.execute("sketch.drag", &json!({"entity":"coupler.end","from":[from.x,from.y],"to":[target.x,target.y],"live":true})).unwrap();
        assert!(four_bar::point(&s, "coupler.end").unwrap().dist(nearest) < 1e-4, "angle {angle}");
        assert!(four_bar::length_error(&s).unwrap() < 1e-6);
        assert_eq!(four_bar::branch(&s).unwrap(), side);
        s.execute("sketch.drag_end", &json!({})).unwrap();
        assert!(four_bar::point(&s, "coupler.end").unwrap().dist(nearest) < 1e-4);
    }
}

fn grab(s: &solidcraft_engine::Session, line: &str, fraction: f64) -> Vec2 {
    let a = four_bar::point(s, &format!("{line}.start")).unwrap();
    let b = four_bar::point(s, &format!("{line}.end")).unwrap();
    a + (b - a) * fraction
}

#[test]
fn a_free_line_grabbed_slightly_off_its_edge_still_translates_rigidly() {
    for from in [Vec2::new(15.0, 12.0), Vec2::new(32.0, 11.0)] {
        let mut s = solidcraft_engine::Session::default();
        s.execute("sketch.create", &json!({"plane":"XZ"})).unwrap();
        s.execute("sketch.line", &json!({"points":[[10,10],[30,10]],"ids":["free"],"infer":false})).unwrap();
        let to = from + Vec2::new(3.0, 5.0);
        let r = s.execute("sketch.drag", &json!({"entity":"free","from":[from.x,from.y],"to":[to.x,to.y],"live":true})).unwrap();
        assert!(four_bar::point(&s, "free.start").unwrap().dist(Vec2::new(13.0, 15.0)) < 1e-6);
        assert!(four_bar::point(&s, "free.end").unwrap().dist(Vec2::new(33.0, 15.0)) < 1e-6);
        let at = Vec2::new(r["at"][0].as_f64().unwrap(), r["at"][1].as_f64().unwrap());
        assert!(at.dist(to) < 1e-6);
    }
}

#[test]
fn an_offset_edge_grab_rotates_with_the_link_and_does_not_drift() {
    for local in [Vec2::new(0.7, 0.1), Vec2::new(0.7, -0.1)] {
        let mut s = four_bar::session().unwrap();
        let target = Vec2::new(-7.0, 11.2);
        let closest = target * (10.0 * local.len() / target.len());
        let attached = |s: &solidcraft_engine::Session| {
            let a = four_bar::point(s, "crank.start").unwrap();
            let d = four_bar::point(s, "crank.end").unwrap() - a;
            a + d * local.x + Vec2::new(-d.y, d.x) * local.y
        };
        let mut from = attached(&s);
        for _ in 0..8 {
            let r = s.execute("sketch.drag", &json!({"entity":"crank","from":[from.x,from.y],"to":[target.x,target.y],"live":true})).unwrap();
            from = Vec2::new(r["at"][0].as_f64().unwrap(), r["at"][1].as_f64().unwrap());
            assert!(from.dist(attached(&s)) < 1e-6);
            assert!(attached(&s).dist(closest) < 1e-4);
            assert!(four_bar::length_error(&s).unwrap() < 1e-6);
            assert_eq!(four_bar::branch(&s).unwrap(), -1.0);
        }
    }
}

#[test]
fn crank_edge_large_drag_reaches_the_nearest_material_point() {
    for fraction in [0.2, 0.5, 0.8] {
        let mut s = four_bar::session().unwrap();
        let side = four_bar::branch(&s).unwrap();
        let from = grab(&s, "crank", fraction);
        let target = Vec2::new(-10.0, 16.0) * fraction;
        let nearest = target * (10.0 * fraction / target.len());
        let result = s.execute("sketch.drag", &json!({"entity":"crank", "from":[from.x,from.y], "to":[target.x,target.y], "live":true})).unwrap();
        let actual = grab(&s, "crank", fraction);
        assert!(four_bar::length_error(&s).unwrap() < 1e-6);
        assert_eq!(four_bar::branch(&s).unwrap(), side);
        assert!(actual.dist(nearest) < 1e-4, "fraction {fraction}: {actual:?}, expected {nearest:?}");
        let at = Vec2::new(result["at"][0].as_f64().unwrap(), result["at"][1].as_f64().unwrap());
        assert!(at.dist(actual) < 1e-6, "reported grab {at:?}, actual {actual:?}");
        s.execute("sketch.drag_end", &json!({})).unwrap();
        assert!(grab(&s, "crank", fraction).dist(nearest) < 1e-4);
    }
}

#[test]
fn rocker_edge_keeps_its_material_point_and_motion_limits() {
    let base = Vec2::new(42.0, 0.0);
    let lo = -(-1264.0_f64 / 1680.0).acos();
    let hi = -(0.2_f64).acos();
    for fraction in [0.2, 0.5, 0.8] {
        for angle in [-2.6_f64, -2.1, -1.5, -1.1] {
            let mut s = four_bar::session().unwrap();
            let from = grab(&s, "rocker", fraction);
            let target = base + Vec2::new(angle.cos(), angle.sin()) * (60.0 * (1.0 - fraction));
            let closest = base + Vec2::new(angle.clamp(lo, hi).cos(), angle.clamp(lo, hi).sin()) * (20.0 * (1.0 - fraction));
            s.execute("sketch.drag", &json!({"entity":"rocker","from":[from.x,from.y],"to":[target.x,target.y],"live":true})).unwrap();
            assert!(
                grab(&s, "rocker", fraction).dist(closest) < 1e-4,
                "fraction {fraction}, angle {angle}, actual {:?}, expected {closest:?}",
                grab(&s, "rocker", fraction)
            );
            assert!(four_bar::length_error(&s).unwrap() < 1e-6);
            assert_eq!(four_bar::branch(&s).unwrap(), -1.0);
        }
    }
}

#[test]
fn an_off_centre_edge_grab_agrees_for_one_move_split_moves_and_holding_still() {
    for fraction in [0.2, 0.5, 0.8] {
        for frames in [1, 16] {
            let mut s = four_bar::session().unwrap();
            let start = grab(&s, "crank", fraction);
            let target = Vec2::new(-10.0, 16.0) * fraction;
            let mut from = start;
            for i in 1..=frames + 8 {
                let to = start + (target - start) * (i.min(frames) as f64 / frames as f64);
                let result = s.execute("sketch.drag", &json!({"entity":"crank","from":[from.x,from.y],"to":[to.x,to.y],"live":true})).unwrap();
                from = Vec2::new(result["at"][0].as_f64().unwrap(), result["at"][1].as_f64().unwrap());
                assert!(from.dist(grab(&s, "crank", fraction)) < 1e-6);
                assert!(four_bar::length_error(&s).unwrap() < 1e-6);
            }
            let closest = target * (10.0 * fraction / target.len());
            assert!(grab(&s, "crank", fraction).dist(closest) < 1e-4);
        }
    }
}

fn coupler_point(angle: f64, along: f64) -> Vec2 {
    let a = Vec2::new(angle.cos(), angle.sin()) * 10.0;
    let d = Vec2::new(42.0, 0.0) - a;
    let len = d.len();
    let u = d * (1.0 / len);
    let x = (40.0 * 40.0 - 20.0 * 20.0 + len * len) / (2.0 * len);
    let c = a + u * x + Vec2::new(u.y, -u.x) * (1600.0 - x * x).sqrt();
    a + (c - a) * along
}

fn closest_coupler_point(target: Vec2, along: f64) -> Vec2 {
    // Independent one-dimensional oracle: sample the full crank revolution, then refine
    // the best interval. Both circle intersections keep the reported linkage's branch.
    let step = std::f64::consts::TAU / 4096.0;
    let best = (0..4096)
        .min_by(|a, b| coupler_point(*a as f64 * step, along).dist(target).total_cmp(&coupler_point(*b as f64 * step, along).dist(target)))
        .unwrap();
    let (mut lo, mut hi) = ((best as f64 - 1.0) * step, (best as f64 + 1.0) * step);
    for _ in 0..80 {
        let a = (2.0 * lo + hi) / 3.0;
        let b = (lo + 2.0 * hi) / 3.0;
        if coupler_point(a, along).dist(target) < coupler_point(b, along).dist(target) {
            hi = b;
        } else {
            lo = a;
        }
    }
    coupler_point((lo + hi) * 0.5, along)
}

#[test]
fn the_coupler_edge_follows_with_both_ends_free_to_rotate() {
    for along in [0.2, 0.5, 0.8] {
        for angle in [1.8, 2.3, 3.8] {
            let mut s = four_bar::session().unwrap();
            let from = grab(&s, "coupler", along);
            let target = coupler_point(angle, along) + Vec2::new(-3.0, 4.0);
            let closest = closest_coupler_point(target, along);
            let result = s.execute("sketch.drag", &json!({"entity":"coupler","from":[from.x,from.y],"to":[target.x,target.y],"live":true})).unwrap();
            let actual = grab(&s, "coupler", along);
            assert!(actual.dist(closest) < 1e-4, "along {along}, angle {angle}, actual {actual:?}, closest {closest:?}");
            let at = Vec2::new(result["at"][0].as_f64().unwrap(), result["at"][1].as_f64().unwrap());
            assert!(at.dist(actual) < 1e-6);
            assert!(four_bar::length_error(&s).unwrap() < 1e-6);
            assert_eq!(four_bar::branch(&s).unwrap(), -1.0);
        }
    }
}

/// Two parallelogram linkages, each held parallel and equal on both pairs of sides (two
/// redundant constraints each), the upper one hanging off the lower one's moving side and tied
/// to a fixed pivot by a 30 mm rod: one degree of freedom (#4585).
fn parallelograms() -> solidcraft_engine::Session {
    let mut s = solidcraft_engine::Session::default();
    let run = |s: &mut solidcraft_engine::Session, id: &str, p: serde_json::Value| {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    };
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    let at = |deg: f64, r: f64| Vec2::new(deg.to_radians().cos(), deg.to_radians().sin()) * r;
    let p1 = at(147.7, 80.0);
    let m = p1 + Vec2::new(0.0, 20.0);
    let r = m + at(20.0, 80.0);
    let (c, t) = (m + Vec2::new(0.0, 20.0), r + Vec2::new(0.0, 20.0));
    let g = r + at(-70.0, 30.0);
    let xy = |p: Vec2| [p.x, p.y];
    let (o, q) = (xy(Vec2::ZERO), xy(Vec2::new(0.0, 20.0)));
    run(&mut s, "sketch.line", json!({"points":[o, xy(p1), xy(m), q, o],"ids":["bot","left","top","right"],"infer":false}));
    run(&mut s, "sketch.line", json!({"points":["left.end", xy(r), xy(t), xy(c), "left.end"],"ids":["ubot","uright","utop","uleft"],"infer":false}));
    run(&mut s, "sketch.line", json!({"points":["ubot.end", xy(g)],"ids":["rod"],"infer":false}));
    run(&mut s, "sketch.constraint.fix", json!({"entities":["bot.start", "rod.end"]}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line":"right"}));
    for (line, value) in [("right", 20.0), ("bot", 80.0), ("ubot", 80.0), ("uleft", 20.0), ("rod", 30.0)] {
        run(&mut s, "sketch.dimension", json!({"entities":[line],"value":value}));
    }
    for (a, b) in [("top", "bot"), ("left", "right"), ("utop", "ubot"), ("uright", "uleft")] {
        run(&mut s, "sketch.constraint.parallel", json!({"a":a,"b":b}));
        run(&mut s, "sketch.constraint.equal", json!({"a":a,"b":b}));
    }
    run(&mut s, "sketch.constraint.parallel", json!({"a":"uleft","b":"left"}));
    s
}

/// The parallelograms' lengths, directions and fixed points, as the largest error.
fn parallelogram_error(s: &solidcraft_engine::Session) -> f64 {
    let p = |name: &str| four_bar::point(s, name).unwrap();
    let side = |line: &str| p(&format!("{line}.end")) - p(&format!("{line}.start"));
    let mut error = p("bot.start").len().max(p("right.start").dist(Vec2::new(0.0, 20.0)));
    for (line, value) in [("bot", 80.0), ("ubot", 80.0), ("uleft", 20.0), ("rod", 30.0)] {
        error = error.max((side(line).len() - value).abs());
    }
    for (a, b) in [("top", "bot"), ("left", "right"), ("utop", "ubot"), ("uright", "uleft")] {
        error = error.max((side(a).len() - side(b).len()).abs()).max(side(a).cross(side(b)).abs() / side(b).len());
    }
    error
}

#[test]
fn a_linkage_held_by_redundant_parallelograms_follows_the_cursor() {
    // Each held parallelogram's parallel and equal sides are dependent only at an exact
    // solution: a pose corrected to the solver's tolerance must still show the linkage's freedom.
    for live in [true, false] {
        let mut s = parallelograms();
        let report = s.execute("sketch.solve", &json!({})).unwrap();
        assert_eq!((&report["status"], &report["dof"]), (&json!("solved"), &json!(1)), "{report}");
        // The lower parallelogram's free corner, its moving sides and the upper one's side, each
        // dragged in turn from where the last drag left the linkage, follows most of the way
        // (the rest is off the path the linkage allows).
        for (entity, along) in [("bot.end", None), ("bot", Some(0.5)), ("left", Some(0.5)), ("ubot", Some(0.5)), ("bot.end", None)] {
            let from = along.map_or_else(|| four_bar::point(&s, entity).unwrap(), |f| grab(&s, entity, f));
            let to = from + Vec2::new(2.0, 2.0);
            let r = s.execute("sketch.drag", &json!({"entity":entity,"from":[from.x,from.y],"to":[to.x,to.y],"live":live})).unwrap();
            let reached = r["reached"].as_f64().unwrap();
            assert!(reached > 0.8, "live {live}: {entity} reached {reached}: {r}");
            assert!(parallelogram_error(&s) < 1e-6, "live {live}: {entity}: {}", parallelogram_error(&s));
            assert_eq!(r["sketch"]["solved"], json!(true));
        }
        // The lower parallelogram's free corner lands on its 80 mm circle, as close to the
        // cursor as the circle allows.
        let from = four_bar::point(&s, "bot.end").unwrap();
        let to = from + Vec2::new(1.0, 2.0);
        s.execute("sketch.drag", &json!({"entity":"bot.end","from":[from.x,from.y],"to":[to.x,to.y],"live":live})).unwrap();
        let at = four_bar::point(&s, "bot.end").unwrap();
        assert!(at.dist(to * (80.0 / to.len())) < 1e-4, "live {live}: {at:?}");
        assert!(parallelogram_error(&s) < 1e-6);
    }
}

#[test]
fn a_drag_that_would_fling_points_far_away_leaves_them_near_the_sketch() {
    let mut s = solidcraft_engine::Session::default();
    for (id, params) in [
        ("sketch.create", json!({"plane":"XY"})),
        ("sketch.line", json!({"points":[[2,-27],[-2,22],[-10,-10],[2,13]]})),
        ("sketch.rectangle.two_point", json!({"p0":[-12,1],"p1":[-18,28]})),
        ("sketch.line", json!({"points":[[6,-21],[-12,-27],[-25,-29],[16,5]]})),
        ("sketch.rectangle.two_point", json!({"p0":[-21,0],"p1":[-23,-3]})),
        ("sketch.line", json!({"points":[[24,23],[-7,7],[0,29],[-6,-7]]})),
        ("sketch.dimension", json!({"entities":["l16","l12"],"type":"angle","value":31})),
    ] {
        s.execute(id, &params).unwrap();
    }
    s.execute("sketch.drag", &json!({"entity":"p8","from":[-23.0,-0.534780330115472],"to":[23,-8]})).unwrap();
    for i in 0..30 {
        let Ok(p) = four_bar::point(&s, &format!("p{i}")) else { continue };
        assert!(p.len() < 1000.0, "p{i} was thrown to {p:?}");
    }
}

/// A line held at its midpoint, with an angle to a horizontal line from there: its end dragged
/// past the midpoint, where the line would have to turn over, pressed it to no length (the angle
/// then held no direction). The drag now stops just short of that, the line still pointing the
/// way the angle says.
#[test]
fn an_end_dragged_past_its_lines_midpoint_leaves_the_line_a_length() {
    for live in [false, true] {
        let mut s = solidcraft_engine::Session::default();
        for (id, params) in [
            ("sketch.create", json!({"plane":"XY"})),
            ("sketch.line", json!({"points":[[-10,-6],[10,6]],"infer":false})),
            ("sketch.constraint.midpoint", json!({"line":"l1","point":"origin"})),
            ("sketch.line", json!({"points":["origin",[15,0]],"infer":false})),
            ("sketch.constraint.horizontal_vertical", json!({"line":"l2"})),
            ("sketch.dimension", json!({"entities":["l1","l2"],"type":"angle"})),
            ("sketch.drag", json!({"entity":"l1.start","from":[-10,-6],"to":[12,4],"live":live})),
        ] {
            s.execute(id, &params).unwrap();
        }
        if live {
            s.execute("sketch.drag_end", &json!({})).unwrap();
        }
        let (a, b) = (four_bar::point(&s, "l1.start").unwrap(), four_bar::point(&s, "l1.end").unwrap());
        assert!(a.dist(b) > 1e-3, "l1 is left {} mm long (live {live})", a.dist(b));
        let d = b - a;
        assert!((d.y.atan2(d.x) - 6f64.atan2(10.0)).abs() < 1e-6, "l1 turned to {d:?} (live {live})");
    }
}
