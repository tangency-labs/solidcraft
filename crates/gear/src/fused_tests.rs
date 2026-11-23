//! The fused build of a body (crate::plan::fused) is the planned body's solid: point for point,
//! the teeth and the blank less what is cut after hold what the plan's steps in order hold.

use crate::hub::{Bore, BoreKind, Chamfer, Hardware, Shaft};
use crate::mesh3d::Solid;
use crate::plan::fused::{Fused, fused};
use crate::plan::{Body, Kind, Op, Spec, Step, plan};

fn body(steps: Vec<Step>, of: &Body) -> Body {
    Body { name: of.name.clone(), frame: of.frame, steps }
}

/// The fused parts as solids: in the teeth or the blank, and in no cut after.
fn holds(p: [f64; 3], parts: &(Solid, Option<Solid>, Vec<Solid>)) -> bool {
    let (teeth, blank, after) = parts;
    (teeth.contains(p) || blank.as_ref().is_some_and(|b| b.contains(p))) && !after.iter().any(|c| c.contains(p))
}

fn solids(f: &Fused, of: &Body) -> (Solid, Option<Solid>, Vec<Solid>) {
    let h = 0.05;
    let teeth = Solid::new(&body(f.teeth.clone(), of), h);
    let blank = (!f.blank.is_empty()).then(|| Solid::new(&body(f.blank.clone(), of), h));
    let after = f
        .after
        .iter()
        .map(|c| {
            let mut c = c.clone();
            *c.op_mut() = Op::New;
            Solid::new(&body(vec![c], of), h)
        })
        .collect();
    (teeth, blank, after)
}

/// The number of booleans a build takes on the toothed body: every step after the first, for
/// the plan; the fused build's teeth joins, its union with the blank and its cuts after.
fn booleans(steps: &[Step]) -> usize {
    steps.len().saturating_sub(1)
}

/// Checks every fused body of `s`'s plan against its planned steps at points through its box;
/// returns (bodies fused, booleans on the teeth before, after).
fn same_solids(s: &Spec) -> (usize, usize, usize) {
    same_bodies(&plan(s).unwrap_or_else(|e| panic!("{e}")))
}

fn same_bodies(p: &crate::plan::Plan) -> (usize, usize, usize) {
    let mut n = (0, 0, 0);
    for b in &p.bodies {
        let Some(f) = fused(b) else { continue };
        n.0 += 1;
        n.1 += booleans(&b.steps);
        n.2 += booleans(&f.teeth) + usize::from(!f.blank.is_empty()) + f.after.len();
        let whole = Solid::new(b, 0.05);
        let parts = solids(&f, b);
        let (lo, hi) = whole.bounds();
        let (lo, hi) = ([lo[0] - 1.0, lo[1] - 1.0, lo[2] - 1.0], [hi[0] + 1.0, hi[1] + 1.0, hi[2] + 1.0]);
        // A fixed scatter (a Halton sequence) through the box, and the box's axis.
        let halton = |mut i: u64, base: u64| {
            let (mut f, mut r) = (1.0, 0.0);
            while i > 0 {
                f /= base as f64;
                r += f * (i % base) as f64;
                i /= base;
            }
            r
        };
        let mut wrong = 0;
        let count = 40_000u64;
        for i in 1..=count {
            let q = [lo[0] + (hi[0] - lo[0]) * halton(i, 2), lo[1] + (hi[1] - lo[1]) * halton(i, 3), lo[2] + (hi[2] - lo[2]) * halton(i, 5)];
            if whole.contains(q) != holds(q, &parts) {
                wrong += 1;
            }
        }
        assert!(wrong == 0, "{}: {wrong} of {count} points differ\n{f:#?}", b.name);
    }
    n
}

fn spur() -> Spec {
    Spec { teeth: 30, module: 1.5, face_width: 8.0, ..Spec::defaults_for(Kind::Spur) }
}

#[test]
fn a_bore_and_lightening_holes_through_the_teeth_are_holes_in_their_region() {
    let s = Spec { bore: Bore { kind: BoreKind::Keyway, diameter: 8.0, ..Bore::default() }, holes: 5, ..spur() };
    let p = plan(&s).unwrap();
    let f = fused(&p.bodies[0]).expect("fused");
    let Step::Prism { region, .. } = &f.teeth[0] else { panic!("{f:?}") };
    assert_eq!(region.holes.len(), 6, "the keyed bore and five holes: {f:?}");
    assert!(f.blank.is_empty() && f.after.is_empty(), "no boolean left: {f:?}");
    assert_eq!(same_solids(&s), (1, 2, 0));
}

#[test]
fn a_hub_and_shafts_are_a_blank_of_their_own_with_their_own_cuts() {
    let shaft = Shaft { diameter: 6.0, length: 10.0, chamfer: 0.5, ..Shaft::default() };
    let s = Spec {
        bore: Bore { kind: BoreKind::Round, diameter: 3.0, ..Bore::default() },
        hub_diameter: 16.0,
        hub_length: 6.0,
        hub_chamfer: 0.8,
        bore_chamfer: 0.3,
        holes: 4,
        shaft_back: shaft,
        tooth_chamfer_front: Chamfer { size: 0.4, angle: 45.0 },
        tooth_chamfer_back: Chamfer { size: 0.4, angle: 45.0 },
        hardware: Hardware { set_screw: 3.0, set_screws: 2, ..Hardware::default() },
        ..spur()
    };
    let (n, before, after) = same_solids(&s);
    assert_eq!(n, 1);
    let p = plan(&s).unwrap();
    let f = fused(&p.bodies[0]).unwrap();
    assert!(f.blank.len() >= 3, "hub, shaft and their cuts: {f:#?}");
    assert!(after < before, "{after} booleans on the teeth, {before} as planned");
}

#[test]
fn a_bearing_pocket_and_twisted_teeth_keep_their_booleans_and_their_solid() {
    let pocket = Spec {
        bore: Bore { kind: BoreKind::Round, diameter: 5.0, ..Bore::default() },
        hardware: Hardware { bearing_od: 13.0, bearing_width: 4.0, ..Hardware::default() },
        ..spur()
    };
    same_solids(&pocket);
    let helical = Spec {
        helix: 20.0,
        bore: Bore { kind: BoreKind::Round, diameter: 5.0, ..Bore::default() },
        hub_diameter: 14.0,
        hub_length: 5.0,
        holes: 4,
        ..Spec { kind: Kind::Helical, ..spur() }
    };
    same_solids(&helical);
    let foot = Spec { elephant_foot: 0.2, bore: Bore { kind: BoreKind::DFlat, diameter: 5.0, ..Bore::default() }, holes: 3, ..spur() };
    let (n, ..) = same_solids(&foot);
    assert_eq!(n, 1, "both layers of the foot take the bore");
    let pair = Spec {
        kind: Kind::Pair,
        teeth2: 45,
        bore: Bore { kind: BoreKind::Hex, diameter: 6.0, ..Bore::default() },
        bore2: Bore { kind: BoreKind::Round, diameter: 8.0, ..Bore::default() },
        holes: 6,
        ..spur()
    };
    let (n, ..) = same_solids(&pair);
    assert!(n >= 1);
}

#[test]
fn a_compounds_bores_are_holes_in_each_layer() {
    use crate::items::{GearItem, GearSettings, ItemKind, Thing};
    use crate::test_util::{g, mesh};
    const Z: [f64; 3] = [0.0, 0.0, 1.0];
    // A compound on a drawn shaft line (bored to it) meshing a spur.
    let mut l = GearItem::new("L", ItemKind::Compound).with(vec![
        Thing::line([0.0, 0.0, -6.0], [0.0, 0.0, 30.0]),
        Thing::circle([0.0; 3], Z, 15.0),
        Thing::circle([0.0, 0.0, 12.0], Z, 8.0),
    ]);
    l.shaft_diameter = Some(6.0);
    l.gears = vec![GearSettings::default(); 2];
    let mut a = GearItem::new("A", ItemKind::Spur).with(vec![Thing::circle([25.0, 0.0, 0.0], Z, 10.0)]);
    a.gears = vec![GearSettings::default()];
    let sp = crate::test_util::planned_system(&[l, a], &[mesh(g(1, 0), g(0, 0))]);
    let p = &sp.plan;
    let cluster = p.bodies.iter().find(|b| b.steps.len() == 5).expect("the compound's body: layer, bore, layer, bore, shaft");
    let f = fused(cluster).expect("fused");
    assert_eq!((f.teeth.len(), f.blank.len(), f.after.len()), (2, 1, 0), "both bores holes, the shaft the blank: {f:#?}");
    let (n, before, after) = same_bodies(p);
    assert!(n >= 1 && after < before, "{n} fused, {before} -> {after}");
}
