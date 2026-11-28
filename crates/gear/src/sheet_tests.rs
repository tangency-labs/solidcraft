use std::f64::consts::TAU;

use super::*;
use crate::hub::Bore;
use crate::plan::plan;
use crate::strength::Load;

/// An entity read back from a DXF.
#[derive(Debug)]
enum Entity {
    Line(V2, V2),
    /// Centre, radius, start and end angle (degrees, counter-clockwise).
    Arc(V2, f64, f64, f64),
    Circle(V2, f64),
    Polyline(Vec<V2>),
}

/// (group code, value) pairs of a DXF's ENTITIES section, and its header's drawing extents.
fn read(dxf: &str) -> (Vec<(String, Entity)>, V2, V2) {
    let lines: Vec<&str> = dxf.lines().collect();
    assert!(lines.len().is_multiple_of(2), "group codes and values come in pairs");
    let pairs: Vec<(u32, &str)> = lines.chunks(2).map(|c| (c[0].trim().parse().unwrap_or_else(|_| panic!("group code {:?}", c[0])), c[1].trim())).collect();
    assert_eq!(pairs.last(), Some(&(0, "EOF")));
    assert!(pairs.windows(2).any(|w| w[0] == (9, "$ACADVER") && w[1] == (1, "AC1009")));
    let start = pairs.iter().position(|p| *p == (2, "ENTITIES")).expect("entities section") + 1;
    let header = &pairs[..start];
    let ext = |name: &str| {
        let i = header.iter().position(|p| *p == (9, name)).unwrap();
        V2::new(header[i + 1].1.parse().unwrap(), header[i + 2].1.parse().unwrap())
    };
    // Split into entities at each code 0.
    let mut groups: Vec<Vec<(u32, &str)>> = Vec::new();
    for p in &pairs[start..] {
        if p.0 == 0 {
            groups.push(vec![*p]);
        } else {
            groups.last_mut().unwrap().push(*p);
        }
    }
    let get = |g: &[(u32, &str)], code: u32| -> f64 { g.iter().find(|p| p.0 == code).unwrap_or_else(|| panic!("no group {code} in {g:?}")).1.parse().unwrap() };
    let layer = |g: &[(u32, &str)]| g.iter().find(|p| p.0 == 8).expect("a layer").1.to_string();
    let mut out = Vec::new();
    let mut open: Option<(String, Vec<V2>)> = None;
    for g in &groups {
        match g[0].1 {
            "LINE" => out.push((layer(g), Entity::Line(V2::new(get(g, 10), get(g, 20)), V2::new(get(g, 11), get(g, 21))))),
            "ARC" => out.push((layer(g), Entity::Arc(V2::new(get(g, 10), get(g, 20)), get(g, 40), get(g, 50), get(g, 51)))),
            "CIRCLE" => out.push((layer(g), Entity::Circle(V2::new(get(g, 10), get(g, 20)), get(g, 40)))),
            "POLYLINE" => {
                assert!(open.is_none() && get(g, 66) == 1.0 && get(g, 70) == 0.0);
                open = Some((layer(g), Vec::new()));
            }
            "VERTEX" => open.as_mut().expect("a vertex inside a polyline").1.push(V2::new(get(g, 10), get(g, 20))),
            "SEQEND" => {
                let (l, pts) = open.take().expect("a polyline to end");
                assert!(pts.len() >= 2);
                out.push((l, Entity::Polyline(pts)));
            }
            "ENDSEC" | "EOF" => {}
            other => panic!("unexpected entity {other}"),
        }
    }
    assert!(open.is_none());
    (out, ext("$EXTMIN"), ext("$EXTMAX"))
}

/// An entity as points along it (arcs finely), from its start to its end.
fn trace(e: &Entity) -> Vec<V2> {
    match e {
        Entity::Line(a, b) => vec![*a, *b],
        Entity::Polyline(p) => p.clone(),
        Entity::Arc(c, r, a0, a1) => {
            let sweep = (a1 - a0).rem_euclid(360.0).to_radians();
            (0..=256).map(|i| *c + V2::polar(*r, a0.to_radians() + sweep * f64::from(i) / 256.0)).collect()
        }
        Entity::Circle(c, r) => (0..=1024).map(|i| *c + V2::polar(*r, TAU * f64::from(i) / 1024.0)).collect(),
    }
}

fn length(p: &[V2]) -> f64 {
    p.windows(2).map(|w| w[0].dist(w[1])).sum()
}

fn dist_to(p: V2, path: &[V2]) -> f64 {
    path.windows(2)
        .map(|w| {
            let d = w[1] - w[0];
            let l2 = d.dot(d);
            let t = if l2 > 0.0 { ((p - w[0]).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
            p.dist(w[0] + d * t)
        })
        .fold(f64::INFINITY, f64::min)
}

/// Reads the DXF of `loops` back and holds it against the outlines: the same length and
/// bounds, every loop closed end to end, and no point of the true curve further from the DXF
/// than the tolerance. Returns the entities.
fn round_trip(dxf: &str, loops: &[&Outline], m: f64) -> Vec<(String, Entity)> {
    let (ents, lo, hi) = read(dxf);
    let traces: Vec<Vec<V2>> = ents.iter().map(|(_, e)| trace(e)).collect();
    // The outlines as the kernel runs them, very finely.
    let fine: Vec<Vec<V2>> = loops.iter().map(|o| polygon(o, 1e-6 * m)).collect();
    let (want, got) = (fine.iter().map(|p| length(p)).sum::<f64>(), traces.iter().map(|p| length(p)).sum::<f64>());
    // (Chords cut inside the curve: a few parts in ten thousand shorter, never longer.)
    assert!(got <= want * (1.0 + 1e-9) && got / want > 1.0 - 5e-4, "length {got} against {want}");
    let all: Vec<V2> = fine.iter().flatten().copied().collect();
    let (flo, fhi) = all.iter().fold((all[0], all[0]), |(lo, hi), q| (V2::new(lo.x.min(q.x), lo.y.min(q.y)), V2::new(hi.x.max(q.x), hi.y.max(q.y))));
    assert!(lo.dist(flo) < DXF_TOLERANCE * m && hi.dist(fhi) < DXF_TOLERANCE * m, "extents {lo:?} {hi:?} against {flo:?} {fhi:?}");
    let tall: Vec<V2> = traces.iter().flatten().copied().collect();
    let (tlo, thi) = tall.iter().fold((tall[0], tall[0]), |(lo, hi), q| (V2::new(lo.x.min(q.x), lo.y.min(q.y)), V2::new(hi.x.max(q.x), hi.y.max(q.y))));
    assert!(tlo.dist(flo) < DXF_TOLERANCE * m && thi.dist(fhi) < DXF_TOLERANCE * m);
    // Each loop's entities join end to end, whichever way an arc was written, and close.
    let mut k = 0;
    for o in loops {
        let n = o.segs.len();
        let mine = &traces[k..k + n];
        k += n;
        if n == 1 && matches!(o.segs[0], Seg::Circle(..)) {
            continue;
        }
        let ends = |t: &Vec<V2>| (t[0], t[t.len() - 1]);
        for i in 0..n {
            let ((a0, a1), (b0, b1)) = (ends(&mine[i]), ends(&mine[(i + 1) % n]));
            let gap = [a0.dist(b0), a0.dist(b1), a1.dist(b0), a1.dist(b1)].into_iter().fold(f64::INFINITY, f64::min);
            assert!(gap < 1e-6, "gap of {gap} after entity {i}");
        }
    }
    assert_eq!(k, ents.len());
    // Entity by entity: the true curve is nowhere further from the DXF than the tolerance, and
    // the DXF nowhere further from the curve.
    let pieces: Vec<&Seg> = loops.iter().flat_map(|o| o.segs.iter()).collect();
    for (seg, t) in pieces.iter().zip(&traces) {
        let f = polygon(&Outline { segs: vec![(*seg).clone()] }, 1e-6 * m);
        let off = f.iter().map(|p| dist_to(*p, t)).fold(0.0, f64::max).max(t.iter().map(|p| dist_to(*p, &f)).fold(0.0, f64::max));
        assert!(off <= DXF_TOLERANCE * m, "{off} mm off, {} allowed", DXF_TOLERANCE * m);
    }
    ents
}

#[test]
fn a_gear_profile_goes_to_dxf_and_back() {
    for (s, m) in [(Spec::default(), 2.0), (Spec { teeth: 9, module: 0.5, shift: 0.4, ..Spec::default() }, 0.5), (Spec { teeth: 57, module: 6.0, pressure_angle: 25.0, backlash: 0.2, ..Spec::default() }, 6.0)] {
        let p = plan(&s).unwrap();
        let o = &p.preview[0].region.outer[0];
        let dxf = profile_dxf(o, m);
        let ents = round_trip(&dxf, &[o], m);
        // One entity a piece: flanks and fillets as polylines, tip and root lands as arcs.
        let z = s.teeth as usize;
        assert_eq!(ents.iter().filter(|(_, e)| matches!(e, Entity::Polyline(_))).count(), 4 * z);
        assert_eq!(ents.iter().filter(|(_, e)| matches!(e, Entity::Arc(..))).count(), 2 * z);
        assert!(ents.iter().all(|(l, _)| l == "PROFILE"));
        // Dense, not absurd: tens of vertices a flank.
        let most = ents.iter().filter_map(|(_, e)| if let Entity::Polyline(p) = e { Some(p.len()) } else { None }).max().unwrap();
        assert!((8..400).contains(&most), "{most}");
        assert!(dxf.is_ascii() && !dxf.contains("NaN") && !dxf.contains("inf") && !dxf.contains('e'));
    }
}

#[test]
fn regions_put_holes_on_their_own_layer() {
    // A gear with a keyed bore.
    let s = Spec { bore: Bore { kind: BoreKind::Keyway, diameter: 8.0, ..Bore::default() }, ..Spec::default() };
    let p = plan(&s).unwrap();
    let r = &p.preview[0].region;
    let ents = round_trip(&region_dxf(r, s.module), &[&r.outer[0], &r.holes[0]], s.module);
    assert!(ents.iter().any(|(l, _)| l == "HOLES") && ents.iter().any(|(l, e)| l == "HOLES" && matches!(e, Entity::Line(..))));
    // A ring gear: the rim is a circle, the teeth a hole.
    let s = Spec::defaults_for(Kind::Internal);
    let p = plan(&s).unwrap();
    let r = &p.preview[0].region;
    let ents = round_trip(&region_dxf(r, s.module), &[&r.outer[0], &r.holes[0]], s.module);
    assert!(matches!(ents[0], (ref l, Entity::Circle(c, _)) if l == "PROFILE" && c == V2::ZERO));
    assert_eq!(ents.iter().filter(|(l, _)| l == "HOLES").count(), ents.len() - 1);
    // A plain round bore is a CIRCLE; a cycloidal wheel, a rack and a coupon piece (clockwise
    // arcs, straight sides) come through too.
    let s = Spec { bore: Bore { kind: BoreKind::Round, diameter: 5.0, ..Bore::default() }, ..Spec::defaults_for(Kind::Cycloidal) };
    let p = plan(&s).unwrap();
    let r = &p.preview[0].region;
    let ents = round_trip(&region_dxf(r, s.module), &[&r.outer[0], &r.holes[0]], s.module);
    assert!(matches!(ents.last().unwrap().1, Entity::Circle(_, r) if r == 2.5));
    let p = plan(&Spec::defaults_for(Kind::Rack)).unwrap();
    round_trip(&profile_dxf(&p.preview[0].region.outer[0], 2.0), &[&p.preview[0].region.outer[0]], 2.0);
    for s in [Spec { coupon: true, ..Spec::defaults_for(Kind::Pair) }, Spec { coupon: true, ..Spec::defaults_for(Kind::Planetary) }] {
        let p = plan(&s).unwrap();
        for g in &p.preview {
            round_trip(&profile_dxf(&g.region.outer[0], s.module), &[&g.region.outer[0]], s.module);
        }
    }
    let p = plan(&Spec::defaults_for(Kind::CycloidalDrive)).unwrap();
    round_trip(&profile_dxf(&p.preview[0].region.outer[0], 1.0), &[&p.preview[0].region.outer[0]], 1.0);
}

#[test]
fn the_design_sheet_lists_inputs_results_checks_and_parts() {
    let s = Spec { name: "Drive".into(), backlash: 0.1, bore: Bore { kind: BoreKind::Round, diameter: 6.0, ..Bore::default() }, load: Load { torque: 2.0, speed: 500.0, ..Load::default() }, ..Spec::defaults_for(Kind::Pair) };
    let p = plan(&s).unwrap();
    let md = design_sheet(&s, &p);
    assert!(md.starts_with("# Gear design sheet: Drive (gear pair)\n"));
    for part in ["## Inputs", "## Results", "## Mesh check", "## Warnings", "## Parts"] {
        assert_eq!(md.matches(part).count(), 1, "{part}");
    }
    for row in [
        "| Module | 2 mm |",
        "| Gear 1 teeth | 20 |",
        "| Gear 2 teeth | 32 |",
        "| Pressure angle | 20° |",
        "| Basic rack | iso53_a (addendum 1 × m, dedendum 1.25 × m, root radius 0.38 × m) |",
        "| Backlash | 0.1 mm |",
        "| Face width | 10 mm |",
        "| Bore | Ø6 mm |",
        "| ratio | 1.6 |  |",
        "| centre distance | 52 | mm |",
        "| working pressure angle | 20 | ° |",
        "- gear 1 and gear 2 mesh: no overlap, flanks 0.093 to 0.094 mm apart",
        "- Drive Gear z20",
        "- Drive Gear z32",
    ] {
        assert!(md.contains(row), "missing {row:?} in\n{md}");
    }
    assert!(md.contains("| Load | 2 N·m at 500 rpm (0.1047 kW) |") && md.contains("minimum bending safety SF"));
    // Every fact is a row; tables are well formed.
    assert_eq!(md.lines().filter(|l| l.starts_with("| ")).count(), p.facts.len() + 2 + md.lines().skip_while(|l| *l != "## Inputs").take_while(|l| *l != "## Results").filter(|l| l.starts_with("| ") && !l.starts_with("| Parameter")).count());
    assert!(md.lines().filter(|l| l.starts_with('|')).all(|l| l.ends_with('|')));
    assert!(md.contains("## Warnings\n\n- none\n"));
    // Warnings and sets without a 2D check say so.
    let s = Spec { teeth: 9, ..Spec::default() };
    let md = design_sheet(&s, &plan(&s).unwrap());
    assert!(md.contains("- gear: undercut") && md.contains("- a single member: nothing to check") && md.contains("| Teeth | 9 |"));
    let s = Spec::defaults_for(Kind::Worm);
    let md = design_sheet(&s, &plan(&s).unwrap());
    assert!(md.contains("- wheel and worm mesh: no overlap") && md.contains("| Worm starts | 1 |"));
    for kind in Kind::ALL.iter().copied() {
        let s = Spec::defaults_for(kind);
        let md = design_sheet(&s, &plan(&s).unwrap());
        assert!(md.contains(kind.label()) && md.len() > 300 && !md.contains("NaN"), "{kind:?}");
    }
    // A coupon's sheet names it.
    let s = Spec { coupon: true, ..Spec::defaults_for(Kind::Pair) };
    let md = design_sheet(&s, &plan(&s).unwrap());
    assert!(md.contains("| Test coupon | 4 teeth a piece |") && md.contains("- Coupon base") && md.contains("coupon: gear 1 centre to gear 2 centre"));
}
