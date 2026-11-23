//! The Gears feature's tooth chamfer: one size for every gear of a set, cut off the tip circle's
//! edge on the end faces (side A, side B or both) after the set is planned.
//!
//! It goes on the gears with flat end faces: spur, helical and herringbone gears (a herringbone's
//! two outer ends), each layer of a compound (not an end that sits against a bigger layer), an
//! internal ring's inner tip edge, and the pinion of an internal or rack item. Bevel, hypoid,
//! worm, crossed helical, rack and planetary gears are left plain, each with a note naming it.
//! A size that would take the whole tooth tip (the addendum) or half the face is refused, naming
//! the gear. Off (size 0) it changes nothing.

use crate::axes::{dot, len, num, sub};
use crate::items::{GearItem, GearRef, ItemKind, face_extent, gear_geoms, who};
use crate::plan::system::SystemPlan;
use crate::plan::{Frame, Op, Step};

/// Which end faces a tooth chamfer goes on (side A: where the gear's axis points, decision 4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChamferFaces {
    #[default]
    Both,
    A,
    B,
}

impl ChamferFaces {
    /// As `gear.create` names it: "both", "a", "b".
    pub fn parse(s: &str) -> Option<ChamferFaces> {
        match s.trim().to_ascii_lowercase().as_str() {
            "both" | "" => Some(ChamferFaces::Both),
            "a" | "side_a" => Some(ChamferFaces::A),
            "b" | "side_b" => Some(ChamferFaces::B),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ChamferFaces::Both => "both",
            ChamferFaces::A => "a",
            ChamferFaces::B => "b",
        }
    }
}

/// The set's tooth chamfer: 45°, `size` mm from the tip circle (and as deep).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ToothChamfer {
    pub size: f64,
    pub faces: ChamferFaces,
}

/// Why a kind of item keeps its teeth plain ("worm"), or None when its gears take the chamfer.
fn left_plain(kind: ItemKind) -> Option<&'static str> {
    match kind {
        ItemKind::Spur | ItemKind::Helical | ItemKind::Herringbone | ItemKind::Compound | ItemKind::Internal | ItemKind::Rack | ItemKind::Shaft => {
            None
        }
        ItemKind::Planetary => Some("planetary set"),
        ItemKind::Bevel => Some("bevel"),
        ItemKind::Hypoid => Some("hypoid"),
        ItemKind::Worm => Some("worm"),
        ItemKind::CrossedHelical => Some("crossed helical"),
    }
}

/// One gear's end faces as planned: where it is (world), its axis (side A), how far it reaches
/// each side of its pitch circle's plane, its tip and pitch radii, its body.
struct Ends {
    gear: GearRef,
    centre: [f64; 3],
    axis: [f64; 3],
    a: f64,
    b: f64,
    tip: f64,
    pitch: f64,
    ring: bool,
    body: usize,
}

/// Cuts the set's tooth chamfer into `sp`'s bodies (see the module's notes). Errors name the
/// gear a size is too big for.
pub fn tooth_chamfer(items: &[GearItem], sp: &mut SystemPlan, c: ToothChamfer) -> Result<(), String> {
    if !(c.size > 0.0) {
        return Ok(());
    }
    if !c.size.is_finite() {
        return Err("the tooth chamfer's size must be a number of mm".into());
    }
    let mut notes = Vec::new();
    let mut ends = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let geoms = gear_geoms(it);
        if let Some(why) = left_plain(it.kind) {
            notes.push(format!("tooth chamfer not applied to {}: {why}", it.who()));
            continue;
        }
        for (g, geo) in geoms.iter().enumerate() {
            let r = GearRef::new(i, g);
            if geo.rack {
                notes.push(format!("tooth chamfer not applied to {}: rack", who(items, r)));
                continue;
            }
            let Some(m) = sp.member_of.iter().find(|(x, _)| *x == r).map(|(_, m)| *m) else { continue };
            let Some(body) = sp.plan.members.get(m).and_then(|x| x.body).filter(|b| *b < sp.plan.bodies.len()) else { continue };
            let want = geo.radius.map(|x| 2.0 * x);
            // Its shape: the member's (a compound's layers: the one of this pitch diameter).
            let Some(shape) = sp
                .plan
                .shapes
                .iter()
                .filter(|x| x.member == m)
                .min_by(|x, y| {
                    let d = |s: &crate::plan::MemberShape| want.map_or(0.0, |w| (s.pitch_diameter - w).abs());
                    d(x).total_cmp(&d(y))
                })
                .copied()
            else {
                continue;
            };
            let module = sp.gears.iter().find(|x| x.item == i && x.gear == g).map_or(1.0, |x| x.module);
            let (mut a, mut b) = face_extent(&it.settings(g), module);
            if shape.face > 0.0 && a + b > 0.0 {
                let k = shape.face / (a + b);
                a *= k;
                b *= k;
            }
            ends.push(Ends {
                gear: r,
                centre: geo.centre,
                axis: geo.axis,
                a,
                b,
                tip: shape.tip_diameter / 2.0,
                pitch: shape.pitch_diameter / 2.0,
                ring: geo.ring,
                body,
            });
        }
    }
    // Too big for a gear: refused, naming it.
    for e in &ends {
        let addendum = (e.tip - e.pitch).abs();
        let name = who(items, e.gear);
        if c.size >= addendum - 1e-9 {
            return Err(format!("Tooth chamfer {} mm is too big for {name} (addendum {} mm)", num(c.size), num(addendum)));
        }
        let face = e.a + e.b;
        if 2.0 * c.size >= face - 1e-9 {
            return Err(format!("Tooth chamfer {} mm is too big for {name} (face width {} mm)", num(c.size), num(face)));
        }
    }
    for (k, e) in ends.iter().enumerate() {
        let f = sp.plan.bodies[e.body].frame;
        let fy = f.y();
        let local = |p: [f64; 3]| [dot(p, f.x), dot(p, fy), dot(p, f.z)];
        let z = local(e.axis);
        let n = len(z);
        if !(n > 1e-9) {
            continue;
        }
        let z = z.map(|v| v / n);
        let pick = if z[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let d = dot(pick, z);
        let x = [pick[0] - z[0] * d, pick[1] - z[1] * d, pick[2] - z[2] * d];
        let xn = len(x);
        let x = x.map(|v| v / xn);
        let frame = Frame { origin: local(sub(e.centre, f.origin)), x, z };
        let sides = [(ChamferFaces::A, e.a, -1.0), (ChamferFaces::B, -e.b, 1.0)];
        for (side, zf, dz) in sides {
            if c.faces != ChamferFaces::Both && c.faces != side {
                continue;
            }
            // An end against another layer of its compound that reaches past this tip: buried.
            let end = [0, 1, 2].map(|i| e.centre[i] + e.axis[i] * zf);
            let against = ends.iter().enumerate().any(|(j, o)| {
                if j == k || o.gear.item != e.gear.item || o.body != e.body {
                    return false;
                }
                let t = dot(sub(end, o.centre), o.axis);
                let foot = [0, 1, 2].map(|i| o.centre[i] + o.axis[i] * t);
                let tol = 1e-6 * (1.0 + o.tip);
                len(sub(end, foot)) <= tol.max(1e-6) && t >= -o.b - 1e-6 && t <= o.a + 1e-6 && o.tip > e.tip - c.size - 1e-9
            });
            if against {
                continue;
            }
            let dr = if e.ring { 1.0 } else { -1.0 };
            let profile = crate::body::corner_cut(e.tip, zf, dr, dz, c.size, c.size);
            sp.plan.bodies[e.body].steps.push(Step::Placed { frame, step: Box::new(Step::Revolve { profile, op: Op::Cut }) });
        }
    }
    for n in notes {
        if !sp.notes.contains(&n) {
            sp.notes.push(n);
        }
    }
    Ok(())
}

/// The two chamfer cuts at `body.steps[i]` and `body.steps[i + 1]`, both ends of one outside
/// gear as [`tooth_chamfer`] adds them, as one step to intersect the body with: a revolved
/// profile that holds the whole body but the two chamfer triangles, each notched in from far
/// outside the tip. One boolean in place of two, and the same solid: the notches reach out only
/// where the body has no material (past its tip radius), and only the triangles' cones meet it.
/// For the builders (the plan keeps its two cuts). `None` when that can't be said: the steps
/// aren't such a pair (in one frame, side A's then side B's, an outside gear's), the chamfer's
/// axis isn't the body's, the body reaches past the tip, or the notches would overlap.
pub fn chamfers_as_one(body: &crate::plan::Body, i: usize) -> Option<Step> {
    use crate::geom::V2;
    let (Some(Step::Placed { frame, step: a }), Some(Step::Placed { frame: frame_b, step: b })) = (body.steps.get(i), body.steps.get(i + 1)) else {
        return None;
    };
    let (Step::Revolve { profile: pa, op: Op::Cut }, Step::Revolve { profile: pb, op: Op::Cut }) = (&**a, &**b) else { return None };
    // corner_cut's triangles: [a2, corner, b2], b2 on the tip cylinder (past the tip, for an
    // outside gear), a2 out past the end face: side A's corner above its b2, side B's below.
    let (&[a2a, _, b2a], &[a2b, _, b2b]) = (&pa[..], &pb[..]) else { return None };
    let outside = b2a.x > a2a.x && b2b.x > a2b.x && (b2a.x - b2b.x).abs() <= 1e-9 * (1.0 + b2a.x.abs());
    if frame != frame_b || !outside || !(a2b.y < b2b.y && b2b.y < b2a.y && b2a.y < a2a.y) {
        return None;
    }
    // The chamfer's axis is the body's own (its frame's z through its origin).
    let on_axis = len([frame.origin[0], frame.origin[1], 0.0]) <= 1e-9 && frame.z[2].abs() >= 1.0 - 1e-12;
    let solid = crate::mesh3d::Solid::new(body, 1.0);
    if !on_axis || !(solid.radius < b2a.x.min(b2b.x)) {
        return None;
    }
    let reach = solid.z.0.abs().max(solid.z.1.abs()) + frame.origin[2].abs();
    let (r, m) = (2.0 * (b2a.x + solid.radius) + 1.0, 2.0 * reach + 1.0);
    if !(r.is_finite() && m.is_finite()) {
        return None;
    }
    let profile = vec![
        V2::new(0.0, -m),
        V2::new(r, -m),
        V2::new(r, a2b.y),
        a2b,
        b2b,
        V2::new(r, b2b.y),
        V2::new(r, b2a.y),
        b2a,
        a2a,
        V2::new(r, a2a.y),
        V2::new(r, m),
        V2::new(0.0, m),
    ];
    Some(Step::Placed { frame: *frame, step: Box::new(Step::Revolve { profile, op: Op::Intersect }) })
}
