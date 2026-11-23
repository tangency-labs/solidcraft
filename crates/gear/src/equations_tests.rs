//! The tooth formulas against the exact numeric teeth they stand for.

use crate::equations::{Edge, End, Form, Repeat, TeethSource, ToothCell, cell_for, cell_of, eval, render};
use std::f64::consts::TAU;

use crate::geom::{Outline, V2};
use crate::involute::{ToothFlank, rack_outline};
use crate::plan::{Kind, Spec, Step, plan};

fn source(s: &Spec, who: &str) -> TeethSource {
    let p = plan(s).unwrap();
    p.teeth.iter().find(|t| t.who == who).cloned().unwrap_or_else(|| panic!("no {who} in {:?}", p.teeth.iter().map(|t| &t.who).collect::<Vec<_>>()))
}

fn spec(kind: Kind, f: impl FnOnce(&mut Spec)) -> Spec {
    let mut s = Spec::defaults_for(kind);
    f(&mut s);
    s
}

/// The parameter where curve `i`'s radius is `r` (it grows along the curve).
fn at_radius(cell: &ToothCell, i: usize, r: f64) -> f64 {
    let (a, b) = cell.range(i).unwrap();
    let (mut lo, mut hi) = (a, b);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if cell.curve_at(i, mid).unwrap().len() < r {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Every fit point of the numeric flank and fillet lies on the formulas' curves, both sides,
/// within 1e-6 modules; and the curves' ends are the numeric tooth's.
fn check_external(src: &TeethSource) -> ToothCell {
    let Form::External { g, f } = &src.form else { panic!("not external") };
    let cell = cell_of(src, 0.0).unwrap();
    let tol = 1e-6 * g.module;
    let (fillet_lower, flank_lower, flank_upper, fillet_upper) = (0, 1, 2, 3);
    for (side, sign, flank, fillet) in [("upper", 1.0, flank_upper, fillet_upper), ("lower", -1.0, flank_lower, fillet_lower)] {
        for (r, h) in &f.flank {
            let q = cell.curve_at(flank, at_radius(&cell, flank, *r)).unwrap();
            let want = V2::polar(*r, sign * h);
            assert!(q.dist(want) < tol, "{side} flank at r {r}: {q:?} vs {want:?} ({})", q.dist(want));
        }
        for (r, h) in f.fillet.iter().take(f.fillet.len() - 1) {
            let q = cell.curve_at(fillet, at_radius(&cell, fillet, *r)).unwrap();
            let want = V2::polar(*r, sign * h);
            assert!(q.dist(want) < tol, "{side} fillet at r {r}: {q:?} vs {want:?} ({})", q.dist(want));
        }
        // Ends: the root, the handover and the tip.
        let root = cell.end(&End::Start(fillet)).unwrap();
        assert!(root.dist(V2::polar(f.rf, sign * f.root_half)) < tol, "{side} root {root:?}");
        let tip = cell.end(&End::End(flank)).unwrap();
        assert!(tip.dist(V2::polar(f.ra, sign * f.tip_half)) < tol, "{side} tip {tip:?}");
        let joint = cell.end(&End::End(fillet)).unwrap().dist(cell.end(&End::Start(flank)).unwrap());
        assert!(joint < 1e-9 * g.module.max(1.0), "{side}: the fillet and the flank meet ({joint})");
        let start = cell.end(&End::Start(flank)).unwrap();
        assert!((start.len() - f.flank[0].0).abs() < tol, "{side}: the flank starts at r {} not {}", start.len(), f.flank[0].0);
    }
    cell
}

#[test]
fn the_template_evaluator_reads_the_expression_language() {
    let vars = |k: &str| match k {
        "m" => Some(2.0),
        "t" => Some(0.5),
        _ => None,
    };
    let near = |a: f64, b: f64| assert!((a - b).abs() < 1e-12, "{a} vs {b}");
    near(eval("{m} * 3 + 1", &vars).unwrap(), 7.0);
    near(eval("-({m}) ^ 2", &vars).unwrap(), -4.0);
    near(eval("2 ^ 3 ^ 2", &vars).unwrap(), 512.0);
    near(eval("180 deg", &vars).unwrap(), std::f64::consts::PI);
    near(eval("t * 1 rad + atan(t)", &vars).unwrap(), 0.5 + 0.5f64.atan());
    near(eval("hypot(3 mm, 4 mm) + min(1, 2, -3) + max(0 mm, 2)", &vars).unwrap(), 4.0);
    near(eval("1e-3 + 2.5e2", &vars).unwrap(), 250.001);
    near(eval("pi / {m}", &vars).unwrap(), std::f64::consts::FRAC_PI_2);
    assert!(eval("{nope}", &vars).is_err());
    assert!(eval("2 +", &vars).is_err());
    assert_eq!(render("{a} + {b_c} * {a}", &|k| format!("P_{k}")), "P_a + P_b_c * P_a");
}

#[test]
fn a_spur_gears_formulas_are_its_generated_flank_and_fillet() {
    let src = source(&Spec::default(), "gear");
    let cell = check_external(&src);
    assert!(cell.exact, "a 20-tooth gear has no undercut: every range is a formula");
    assert_eq!(cell.count().unwrap(), 20);
    assert_eq!(cell.curves.len(), 4);
    assert_eq!(cell.joins.len(), 2);
    assert!(matches!(cell.repeat, Repeat::Circular { .. }));
    // The root half-arcs end half a pitch either side of the tooth: copies meet there.
    let Form::External { f, .. } = &src.form else { unreachable!() };
    let half = std::f64::consts::PI / 20.0;
    assert!(cell.point(1).unwrap().dist(V2::polar(f.rf, half)) < 1e-12);
    assert!(cell.point(0).unwrap().dist(V2::polar(f.rf, -half)) < 1e-12);
}

#[test]
fn helical_shifted_and_backlashed_gears_follow_their_flanks() {
    let cases: Vec<Spec> = vec![
        spec(Kind::Helical, |s| s.helix = 25.0),
        spec(Kind::Spur, |s| {
            s.shift = 0.4;
            s.backlash = 0.1;
        }),
        spec(Kind::Spur, |s| {
            s.teeth = 41;
            s.module = 1.5;
            s.pressure_angle = 25.0;
            s.shift = -0.2;
        }),
        spec(Kind::Spur, |s| s.rack = crate::involute::BasicRack::standard("print_fdm").unwrap()),
        spec(Kind::Herringbone, |s| s.teeth = 30),
    ];
    for s in cases {
        let src = source(&s, "gear");
        let cell = check_external(&src);
        assert!(cell.exact, "{:?} z{} x{}: exact", s.kind, s.teeth, s.shift);
        assert_eq!(cell.helix_key.is_some(), s.kind != Kind::Spur);
    }
}

#[test]
fn an_undercut_gear_gets_numeric_ranges_where_the_fillet_crosses_the_flank() {
    for z in [7, 9, 12] {
        let src = source(&spec(Kind::Spur, |s| s.teeth = z), "gear");
        let Form::External { f, .. } = &src.form else { unreachable!() };
        assert!(f.undercut, "z{z} is undercut");
        let cell = check_external(&src);
        assert!(!cell.exact, "z{z}: the crossing is numeric");
        assert!(cell.joins.is_empty());
    }
}

#[test]
fn a_ring_gears_formulas_are_its_tooth_spaces() {
    for s in [
        spec(Kind::Internal, |_| {}),
        spec(Kind::Internal, |s| {
            s.teeth = 45;
            s.helix = 15.0;
            s.shift = 0.3;
        }),
    ] {
        let src = source(&s, "ring");
        let Form::Internal { ring, f } = &src.form else { panic!("not a ring") };
        let cell = cell_of(&src, 0.0).unwrap();
        let tol = 1e-6 * ring.gear.module;
        for (sign, i) in [(-1.0, 0), (1.0, 1)] {
            for (r, h) in &f.flank {
                let q = cell.curve_at(i, at_radius(&cell, i, *r)).unwrap();
                assert!(q.dist(V2::polar(*r, sign * h)) < tol, "flank at r {r}: {q:?}");
            }
            let tip = cell.end(&End::End(i)).unwrap();
            assert!(tip.dist(V2::polar(f.ra, sign * f.tip_half)) < tol, "root end {tip:?}");
        }
        for (sign, q) in [(-1.0, cell.point(0).unwrap()), (1.0, cell.point(1).unwrap())] {
            assert!(q.dist(V2::polar(f.rf, sign * std::f64::consts::PI / f64::from(ring.gear.teeth))) < tol);
        }
        assert_eq!(cell.count().unwrap() as u32, ring.gear.teeth);
    }
}

#[test]
fn a_racks_points_are_its_outline() {
    for s in [
        spec(Kind::Rack, |_| {}),
        spec(Kind::Rack, |s| {
            s.shift = 0.3;
            s.backlash = 0.05;
            s.rack_teeth = 7;
        }),
    ] {
        let src = source(&s, "rack");
        let Form::Rack { g, count, below } = &src.form else { panic!("not a rack") };
        let o = rack_outline(g, *count, *below);
        let cell = cell_of(&src, 0.0).unwrap();
        let Repeat::Linear { spacing, .. } = &cell.repeat else { panic!("linear") };
        let pitch = cell.vars.eval(spacing, None).unwrap();
        // Every outline corner is a cell corner, shifted by whole pitches (or on the back).
        let mut corners: Vec<V2> = Vec::new();
        for k in 0..*count {
            for e in &cell.edges {
                let (Edge::Line(a, b) | Edge::Arc(a, b)) = e;
                for end in [a, b] {
                    corners.push(cell.end(end).unwrap() + V2::new(pitch * f64::from(k), 0.0));
                }
            }
        }
        for e in &cell.closing {
            let (Edge::Line(a, b) | Edge::Arc(a, b)) = e;
            corners.push(cell.end(a).unwrap());
            corners.push(cell.end(b).unwrap());
        }
        for seg in &o.segs {
            let q = seg.start();
            assert!(corners.iter().any(|c| c.dist(q) < 1e-9), "corner {q:?} is not drawn");
        }
        assert_eq!(cell.count().unwrap() as u32, *count);
    }
}

/// The tooth outlines of every prism of a plan.
fn prism_outlines(s: &Spec) -> Vec<Outline> {
    let p = plan(s).unwrap();
    let mut out = Vec::new();
    for b in &p.bodies {
        for st in &b.steps {
            if let Step::Prism { region, .. } = st {
                out.extend(region.outer.iter().chain(&region.holes).filter(|o| o.segs.len() > 12).cloned());
            }
        }
    }
    out
}

#[test]
fn every_tooth_outline_of_the_planar_sets_finds_its_formulas() {
    for (kind, members) in
        [(Kind::Spur, 1), (Kind::Helical, 1), (Kind::Herringbone, 2), (Kind::Internal, 1), (Kind::Pair, 2), (Kind::Planetary, 5), (Kind::Rack, 1)]
    {
        let s = Spec::defaults_for(kind);
        let p = plan(&s).unwrap();
        let outlines = prism_outlines(&s);
        let found: Vec<(&Outline, ToothCell)> = outlines.iter().filter_map(|o| cell_for(&p.teeth, o).map(|c| (o, c))).collect();
        assert!(found.len() >= members, "{kind:?}: {} of {} outlines found formulas", found.len(), outlines.len());
        for (o, c) in &found {
            // The cell's tooth 0 is where the outline's is: its curves' ends are outline points.
            let pts: Vec<V2> = o.segs.iter().map(|s| s.start()).collect();
            for i in 0..c.curves.len() {
                let q = c.end(&End::Start(i)).unwrap();
                assert!(pts.iter().any(|p| p.dist(q) < 1e-6 * c.module), "{kind:?} {}: curve {i} starts off the outline", c.who);
            }
        }
    }
}

#[test]
fn tuned_tips_have_no_formulas_and_are_left_to_the_fitted_splines() {
    let s = spec(Kind::Spur, |s| s.tip_round = 0.2);
    let p = plan(&s).unwrap();
    let outlines = prism_outlines(&s);
    assert!(!outlines.is_empty());
    assert!(outlines.iter().all(|o| cell_for(&p.teeth, o).is_none()), "tip rounding stays fitted");
}

#[test]
fn rendered_formulas_fit_an_equation_curve_and_evaluate_alike() {
    for s in [Spec::default(), spec(Kind::Helical, |s| s.helix = 20.0), spec(Kind::Spur, |s| s.teeth = 9), spec(Kind::Internal, |_| {})] {
        let p = plan(&s).unwrap();
        let src = &p.teeth[0];
        let cell = cell_of(src, 0.3).unwrap();
        for tpl in cell.templates() {
            // As design parameters with a long prefix, and as numbers: both short enough for an
            // equation curve (1024 characters), and the numbers give the same values.
            let named = render(tpl, &|k| format!("Planetary_set_{k}"));
            assert!(named.len() < 1024, "{} characters: {named}", named.len());
            let numeric = cell.vars.numeric(tpl);
            assert!(numeric.len() < 1024, "{} characters: {numeric}", numeric.len());
            assert!(!numeric.contains('{'), "{numeric}");
            for t in [0.0, 0.3, 1.1] {
                let a = cell.vars.eval(tpl, Some(t)).unwrap();
                let b = eval(&numeric, &|k| (k == "t").then_some(t)).unwrap();
                assert!((a - b).abs() <= 1e-9 * a.abs().max(1.0), "{tpl}: {a} vs {b}");
            }
        }
        // Derived values agree with their formulas over the inputs.
        for v in &cell.vars.list {
            if !v.input {
                let again = cell.vars.eval(&v.expr, None).unwrap();
                assert!((again - v.value).abs() <= 1e-12 * v.value.abs().max(1.0), "{}", v.key);
            }
        }
    }
}

#[test]
fn a_flank_without_a_land_is_refused() {
    let src = source(
        &spec(Kind::Spur, |s| {
            s.teeth = 10;
            s.shift = 1.2;
        }),
        "gear",
    );
    let Form::External { f, .. } = &src.form else { unreachable!() };
    let f: &ToothFlank = f;
    if f.pointed {
        assert!(cell_of(&src, 0.0).is_err());
    }
}

/// Writes a command script that draws a set's teeth the way the engine's equation builder does
/// (parametric), for trying on the kernel with the CLI: `GEAR_EQ_SCRIPT=out.json GEAR_EQ_KIND=internal
/// cargo test --release -p solidcraft-gear script_for_the_kernel -- --ignored`.
#[test]
#[ignore]
fn script_for_the_kernel() {
    use crate::equations::{Quantity, Repeat};
    use serde_json_lite::*;
    let path = std::env::var("GEAR_EQ_SCRIPT").unwrap();
    let kind = Kind::parse(&std::env::var("GEAR_EQ_KIND").unwrap_or("spur".into())).unwrap();
    let s = Spec::defaults_for(kind);
    let p = plan(&s).unwrap();
    let outlines = prism_outlines(&s);
    let cell = outlines.iter().find_map(|o| cell_for(&p.teeth, o)).unwrap();
    let pre = "G";
    let name = |k: &str| format!("{pre}_{k}");
    let text = |t: &str| render(t, &name);
    let mut cmds: Vec<String> = Vec::new();
    for v in &cell.vars.list {
        let e = if v.input { v.expr.clone() } else { render(&v.expr, &name) };
        let unit = match v.quantity {
            Quantity::Length => "mm",
            Quantity::Angle => "deg",
            Quantity::Number => "",
        };
        cmds.push(cmd("parameters.add", &[("name", s_(&name(&v.key))), ("expression", s_(&e)), ("unit", s_(unit))]));
    }
    cmds.push(cmd("sketch.create", &[("plane", s_("XY")), ("name", s_("S"))]));
    // Curve ids are s1, s2… in order; points p<n> are unknown here, so points get ids by name.
    let mut cid = Vec::new();
    for (i, c) in cell.curves.iter().enumerate() {
        cmds.push(cmd(
            "sketch.equation_curve",
            &[
                ("x", s_(&text(&c.x))),
                ("y", s_(&text(&c.y))),
                ("start", s_(&text(&c.start))),
                ("end", s_(&text(&c.end))),
                ("tolerance", n_(c.tolerance)),
            ],
        ));
        cid.push(format!("s{}", i + 1));
    }
    let mut made = std::collections::BTreeMap::new();
    let mut edges_ids = Vec::new();
    let mut k = 0;
    let mut end = |e: &End, cmds: &mut Vec<String>| -> String {
        match e {
            End::Start(i) => format!("\"{}.start\"", cid[*i]),
            End::End(i) => format!("\"{}.end\"", cid[*i]),
            End::Point(i) => {
                if !made.contains_key(i) {
                    let id = format!("q{i}");
                    let at = &cell.points[*i];
                    cmds.push(cmd("sketch.point", &[("id", s_(&id)), ("point", format!("[{},{}]", s_(&text(&at[0])), s_(&text(&at[1]))))]));
                    made.insert(*i, id);
                }
                s_(&made[i])
            }
        }
    };
    let mut edge = |e: &Edge, cmds: &mut Vec<String>, k: &mut usize| -> String {
        *k += 1;
        let id = format!("e{k}");
        match e {
            Edge::Arc(a, b) => {
                let (a, b) = (end(a, cmds), end(b, cmds));
                cmds.push(cmd("sketch.arc.center_point", &[("id", s_(&id)), ("center", s_("origin")), ("start", a), ("end", b)]));
            }
            Edge::Line(a, b) => {
                let (a, b) = (end(a, cmds), end(b, cmds));
                cmds.push(cmd("sketch.line", &[("ids", format!("[{}]", s_(&id))), ("points", format!("[{a},{b}]"))]));
            }
        }
        id
    };
    for e in &cell.edges {
        edges_ids.push(edge(e, &mut cmds, &mut k));
    }
    let seeds: Vec<String> = cid.iter().chain(&edges_ids).map(|x| s_(x)).collect();
    match &cell.repeat {
        Repeat::Circular { count } => cmds.push(cmd(
            "sketch.pattern.circular",
            &[("entities", format!("[{}]", seeds.join(","))), ("center", s_("origin")), ("count", s_(&text(count)))],
        )),
        Repeat::Linear { count, spacing } => cmds.push(cmd(
            "sketch.pattern.rectangular",
            &[("entities", format!("[{}]", seeds.join(","))), ("count", s_(&text(count))), ("spacing", s_(&text(spacing))), ("dir", "[1,0]".into())],
        )),
    }
    for e in &cell.closing {
        edge(e, &mut cmds, &mut k);
    }
    cmds.push(cmd("sketch.finish", &[]));
    let face = name(&cell.face_key);
    let mut ext = vec![("sketch", s_("S")), ("distance", s_(&face))];
    if matches!(kind, Kind::Internal) {
        // The ring: a disc round the teeth, its region picked by a point near its rim.
        let rr = p.bodies[0]
            .steps
            .iter()
            .find_map(|st| if let Step::Prism { region, .. } = st { region.outer.first().map(|o| o.max_radius()) } else { None })
            .unwrap();
        cmds.insert(cmds.len() - 1, cmd("sketch.circle.center", &[("center", s_("origin")), ("radius", n_(rr))]));
        ext.push(("profiles", format!("[{{\"point\":[{},0]}}]", rr - 0.5)));
    }
    cmds.push(cmd("solid.extrude", &ext));
    cmds.push(cmd("inspect.measure", &[]));
    let change = match kind {
        Kind::Rack => ("rack_teeth", "11"),
        Kind::Internal => ("teeth", "54"),
        _ => ("teeth", "24"),
    };
    cmds.push(cmd("parameters.change", &[("name", s_(&name(change.0))), ("expression", s_(change.1))]));
    cmds.push(cmd("inspect.measure", &[]));
    std::fs::write(path, format!("[\n{}\n]", cmds.join(",\n"))).unwrap();
    // Expected volumes: the numeric outline's area times the face width, before and after.
    let o = outlines.iter().find(|o| cell_for(&p.teeth, o).is_some()).unwrap();
    eprintln!("area {} face {}", o.area(), s.face_width);
}

/// Just enough JSON writing for the script above.
mod serde_json_lite {
    pub fn s_(t: &str) -> String {
        format!("\"{}\"", t.replace('\\', "\\\\").replace('"', "\\\""))
    }
    pub fn n_(v: f64) -> String {
        format!("{v}")
    }
    pub fn cmd(name: &str, kv: &[(&str, String)]) -> String {
        let body: Vec<String> = kv.iter().map(|(k, v)| format!("\"{k}\":{v}")).collect();
        format!("{{\"command\":\"{name}\",\"params\":{{{}}}}}", body.join(","))
    }
}

// ---------------------------------------------------------------------------------------------
// Parametric sets: where the members sit and how they turn, as formulas
// ---------------------------------------------------------------------------------------------

/// `vars` worked out again with some inputs changed (each derived value from its formula, in
/// order).
fn again(vars: &crate::equations::Vars, inputs: &[(&str, f64)]) -> crate::equations::Vars {
    let mut out = crate::equations::Vars::default();
    for v in &vars.list {
        let mut v = v.clone();
        if v.input {
            if let Some((_, x)) = inputs.iter().find(|(k, _)| *k == v.key) {
                v.value = *x;
            }
        } else {
            v.value = out.eval(&v.expr, None).unwrap_or_else(|e| panic!("{}: {e}", v.key));
        }
        out.list.push(v);
    }
    out
}

/// The cells of a plan's toothed prism outlines, by member name.
fn cells(s: &Spec) -> Vec<ToothCell> {
    let p = plan(s).unwrap();
    prism_outlines(s).iter().filter_map(|o| cell_for(&p.teeth, o)).collect()
}

fn same_angle(a: f64, b: f64, period: f64) -> bool {
    let d = (a - b) / period;
    (d - d.round()).abs() < 1e-7
}

#[test]
fn the_centre_distance_formula_is_the_working_centre_distance() {
    for (z1, z2, x1, x2, helix) in [(20, 30, 0.0, 0.0, 0.0), (13, 41, 0.4, 0.1, 0.0), (17, 23, -0.2, 0.5, 15.0), (9, 60, 0.6, -0.3, 30.0)] {
        let s = spec(Kind::Pair, |s| {
            s.teeth = z1;
            s.teeth2 = z2;
            s.shift = x1;
            s.shift2 = x2;
            s.helix = helix;
        });
        let p = plan(&s).unwrap();
        let a = p.facts.iter().find(|f| f.label == "centre distance").unwrap().value;
        let (index, x, y) = p.placing.centres.first().cloned().unwrap_or_else(|| panic!("{z1}/{z2}: no centre formula"));
        let (cx, cy) = p.placing.value(&x, &y).unwrap();
        assert!((cx - a).abs() < 1e-9 && cy.abs() < 1e-12, "{z1}/{z2} x {x1}/{x2} β {helix}: {cx} vs {a}");
        assert_eq!(p.bodies[index].frame.origin[0], a);
    }
}

/// A pair edited through its parameters: gear 2's centre and phase formulas, worked out again,
/// are the new set's.
#[test]
fn a_pairs_place_and_phase_follow_its_parameters() {
    let s = spec(Kind::Pair, |s| {
        s.module = 2.0;
        s.teeth = 20;
        s.teeth2 = 30;
    });
    let p = plan(&s).unwrap();
    let g2 = cells(&s).into_iter().find(|c| c.who == "gear 2").unwrap();
    assert!(g2.vars.get("gear2_phase").is_some(), "gear 2 is turned by a formula");
    for (z1, z2, x1) in [(24, 30, 0.0), (24, 33, 0.3), (13, 30, 0.0)] {
        let inputs = [("teeth", f64::from(z1)), ("teeth2", f64::from(z2)), ("shift", x1)];
        let placing = again(&p.placing.vars, &inputs);
        let cell = again(&g2.vars, &inputs);
        let t = spec(Kind::Pair, |t| {
            t.module = 2.0;
            t.teeth = z1;
            t.teeth2 = z2;
            t.shift = x1;
        });
        let q = plan(&t).unwrap();
        let (_, x, _) = &p.placing.centres[0];
        assert!((placing.eval(x, None).unwrap() - q.bodies[1].frame.origin[0]).abs() < 1e-9, "{z1}/{z2}");
        let new = cells(&t).into_iter().find(|c| c.who == "gear 2").unwrap();
        let (a, b) = (cell.value("gear2_phase").unwrap(), new.vars.value("gear2_phase").unwrap());
        assert!(same_angle(a, b, TAU / f64::from(z2)), "{z1}/{z2}: phase {a} vs {b}");
    }
}

/// A planetary set edited through its parameters: the planets' centres and phases and the
/// ring's phase, worked out again, are the new set's (evenly and unevenly spaced planets).
#[test]
fn a_planetary_sets_places_and_phases_follow_its_parameters() {
    let base = |zs: u32, zr: u32, m: f64| {
        spec(Kind::Planetary, |s| {
            s.module = m;
            s.teeth = zs;
            s.teeth2 = 24;
            s.teeth3 = zr;
            s.planets = 3;
        })
    };
    let s = base(12, 60, 1.0);
    let p = plan(&s).unwrap();
    assert_eq!(p.placing.centres.len(), 3);
    let made = cells(&s);
    for (zs, zr, m) in [(12, 60, 1.25), (15, 63, 1.0), (13, 61, 1.0), (14, 62, 1.5)] {
        let inputs = [("teeth", f64::from(zs)), ("teeth3", f64::from(zr)), ("module", m)];
        let placing = again(&p.placing.vars, &inputs);
        let t = base(zs, zr, m);
        let q = plan(&t).unwrap();
        let planets: Vec<&crate::plan::Body> = q.bodies.iter().filter(|b| b.name.starts_with("Planet")).collect();
        for (k, (_, x, y)) in p.placing.centres.iter().enumerate() {
            let (cx, cy) = (placing.eval(x, None).unwrap(), placing.eval(y, None).unwrap());
            let o = planets[k].frame.origin;
            assert!((cx - o[0]).abs() < 1e-9 && (cy - o[1]).abs() < 1e-9, "{zs}/{zr} m{m} planet {}: ({cx}, {cy}) vs {o:?}", k + 1);
        }
        let now = cells(&t);
        for cell in &made {
            let Some(key) = cell.vars.list.iter().find(|v| v.key.ends_with("_phase")).map(|v| v.key.clone()) else { continue };
            let (period, who) = if key == "ring_phase" { (TAU / f64::from(zr), "ring") } else { (TAU / 24.0, "planet") };
            let a = again(&cell.vars, &inputs).value(&key).unwrap();
            let new = now.iter().find(|c| c.who == who && c.vars.get(&key).is_some()).unwrap_or_else(|| panic!("{zs}/{zr}: no {key}"));
            let b = new.vars.value(&key).unwrap();
            assert!(same_angle(a, b, period), "{zs}/{zr} m{m} {key}: {a} vs {b}");
        }
    }
}

/// A parametric spur gear's clearance profile, its followed vertices worked out for other
/// parameters, has the new set's envelope (largest radius, ends along the axis).
#[test]
fn a_clearance_profile_follows_the_tip_and_the_face() {
    let base = |z: u32, face: f64| {
        spec(Kind::Spur, |s| {
            s.module = 2.0;
            s.teeth = z;
            s.face_width = face;
            s.clearance_body = Some(crate::clearance::Clearance::default());
        })
    };
    let s = base(20, 8.0);
    let p = plan(&s).unwrap();
    let cell = cells(&s).into_iter().next().unwrap();
    let envelope = |profile: &[V2]| {
        let r = profile.iter().map(|q| q.x).fold(0.0, f64::max);
        let (z0, z1) = profile.iter().fold((f64::MAX, f64::MIN), |a, q| (a.0.min(q.y), a.1.max(q.y)));
        (r, z0, z1)
    };
    let profile_of = |p: &crate::plan::Plan| match &p.clearance.as_ref().unwrap().steps[0] {
        Step::Placed { step, .. } => match step.as_ref() {
            Step::Revolve { profile, .. } => profile.clone(),
            _ => panic!("not a revolve"),
        },
        _ => panic!("not placed"),
    };
    let follow = &p.clearance_follow[0];
    let profile = profile_of(&p);
    assert_eq!(follow.len(), profile.len());
    assert!(follow.iter().any(|f| f.r.is_some()) && follow.iter().any(|f| f.z.is_some()), "{follow:?}");
    for (z, face) in [(30, 8.0), (13, 12.0), (40, 5.0)] {
        let vars = again(&cell.vars, &[("teeth", f64::from(z)), ("face_width", face)]);
        let moved: Vec<V2> = profile
            .iter()
            .zip(follow)
            .map(|(q, f)| {
                let x = f.r.as_ref().map_or(q.x, |(tpl, at)| q.x + vars.eval(tpl, None).unwrap() - at);
                let y = f.z.as_ref().map_or(q.y, |(k, key, at)| q.y + k * (vars.value(key).unwrap() - at));
                V2::new(x, y)
            })
            .collect();
        let want = envelope(&profile_of(&plan(&base(z, face)).unwrap()));
        let got = envelope(&moved);
        assert!(
            (got.0 - want.0).abs() < 1e-3 && (got.1 - want.1).abs() < 0.05 && (got.2 - want.2).abs() < 0.05,
            "z{z} face {face}: {got:?} vs {want:?}"
        );
    }
}
