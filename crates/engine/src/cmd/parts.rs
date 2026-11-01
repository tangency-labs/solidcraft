//! Standard parts: ISO metric fasteners, dowel pins and ball bearings, generated in code from
//! the dimensions in the public standards tables (no vendor geometry). Each part is inserted as
//! its own component, built by ordinary features (so it stays parametric and editable), with a
//! cosmetic thread where it has one; at a hole it gets a rigid joint that puts its axis on the
//! hole's and seats it on the face. Inserted at several holes it is still one component, placed
//! once per hole, each placement with its own joint.
//!
//! Part frame: the seating plane is z = 0 (the underside of a head, the bottom of a nut or
//! washer, the top of a set screw or pin); the shank runs down −z.

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use super::CommandSpec;
use crate::params::{bad, num, str_, vec3};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("parts.library", "Standard Parts", library).icon("library")
        .noundo()
        .params("family? → families (name, standard, sizes, lengths) and, with family, each size's dimensions (mm)"),
    CommandSpec::new("parts.fastener", "Insert Fastener", insert_atomic).at("SOLID", "INSERT").icon("fastener").params(
        "the Insert Part dialog's command (same as parts.insert): family, size, length?, at?|point?+direction?, name?",
    ),
    CommandSpec::new("parts.insert", "Insert Part", insert_atomic).icon("fastener").params(
        "family (socket_head_cap_screw | button_head_cap_screw | hex_bolt | hex_nut | washer | set_screw | dowel_pin | bearing); size (\"M6\", or \"608\" for bearings); \
         length? (mm; screws, bolts, set screws, pins; default a standard length near 2.5 d); \
         at?: [x,y,z] on a hole's rim or wall (the part seats on the hole's face with a rigid joint), or a list of them (one component, \
         placed and seated once per hole) | point?: [x,y,z] with direction?: [x,y,z] (no joint); name? → component, occurrence, joint?, \
         and with at: occurrences, joints (one per hole, in pick order)",
    ),
    CommandSpec::new("parts.resize", "Change Part Size", resize_atomic).icon("scale")
        .params("component: id|name of an inserted part; size?; length? — rebuilds it (joints keep it seated)"),
];

/// A family: (id, name, standard).
const FAMILIES: &[(&str, &str, &str)] = &[
    ("socket_head_cap_screw", "Socket head cap screw", "ISO 4762"),
    ("button_head_cap_screw", "Button head cap screw", "ISO 7380-1"),
    ("hex_bolt", "Hex head bolt / screw", "ISO 4014 / ISO 4017"),
    ("hex_nut", "Hex nut", "ISO 4032"),
    ("washer", "Plain washer", "ISO 7089"),
    ("set_screw", "Hex socket set screw, flat point", "ISO 4026"),
    ("dowel_pin", "Parallel pin (dowel)", "ISO 8734"),
    ("bearing", "Deep groove ball bearing (envelope)", "ISO 15 (6000/6200/600 series)"),
];

/// ISO metric coarse: (size, d, pitch).
const METRIC: [(&str, f64, f64); 14] = [
    ("M2", 2.0, 0.4),
    ("M2.5", 2.5, 0.45),
    ("M3", 3.0, 0.5),
    ("M4", 4.0, 0.7),
    ("M5", 5.0, 0.8),
    ("M6", 6.0, 1.0),
    ("M8", 8.0, 1.25),
    ("M10", 10.0, 1.5),
    ("M12", 12.0, 1.75),
    ("M14", 14.0, 2.0),
    ("M16", 16.0, 2.0),
    ("M20", 20.0, 2.5),
    ("M24", 24.0, 3.0),
    ("M30", 30.0, 3.5),
];

/// ISO 4762: (size, head diameter dk, head height k, socket s, socket depth t).
const SHCS: [(&str, f64, f64, f64, f64); 14] = [
    ("M2", 3.8, 2.0, 1.5, 1.0),
    ("M2.5", 4.5, 2.5, 2.0, 1.1),
    ("M3", 5.5, 3.0, 2.5, 1.3),
    ("M4", 7.0, 4.0, 3.0, 2.0),
    ("M5", 8.5, 5.0, 4.0, 2.5),
    ("M6", 10.0, 6.0, 5.0, 3.0),
    ("M8", 13.0, 8.0, 6.0, 4.0),
    ("M10", 16.0, 10.0, 8.0, 5.0),
    ("M12", 18.0, 12.0, 10.0, 6.0),
    ("M14", 21.0, 14.0, 12.0, 7.0),
    ("M16", 24.0, 16.0, 14.0, 8.0),
    ("M20", 30.0, 20.0, 17.0, 10.0),
    ("M24", 36.0, 24.0, 19.0, 12.0),
    ("M30", 45.0, 30.0, 22.0, 15.5),
];

/// ISO 7380-1: (size, head diameter dk, head height k, socket s, socket depth t, the last being
/// the key engagement depth, which is 0.59 to 0.63 k throughout). The standard runs M3 to M16:
/// the M2.5 and M14 some suppliers list are not in it.
const BHCS: [(&str, f64, f64, f64, f64); 8] = [
    ("M3", 5.7, 1.65, 2.0, 1.04),
    ("M4", 7.6, 2.2, 2.5, 1.3),
    ("M5", 9.5, 2.75, 3.0, 1.56),
    ("M6", 10.5, 3.3, 4.0, 2.08),
    ("M8", 14.0, 4.4, 5.0, 2.6),
    ("M10", 17.5, 5.5, 6.0, 3.12),
    ("M12", 21.0, 6.6, 8.0, 4.16),
    ("M16", 28.0, 8.8, 10.0, 5.2),
];

/// A button head's cylindrical flank, as a fraction of the head height k, and the flat top it
/// keeps around its socket, as a fraction of the socket's across-flats s. The flat clears the
/// socket's corners (0.577 s) at every size.
///
/// ISO 7380-1 draws neither, and a real button head has no flat at all: its crown curves on and
/// the socket's corners break through it. The flat is ours, so that the socket is a plain cut on
/// a plane. It costs us the standard's crown radius r1, which the head's own dk and k then fix:
/// the result is inside r1's tolerance at six of the eight sizes, 0.08 under it at M3 and 0.12
/// over it at M12. No one pair of fractions is inside it at all eight, and taking r1 as given
/// instead leaves no flat clear of the socket below M10, so dk, k, s and t are what we keep exact.
const BUTTON_FLANK: f64 = 0.15;
const BUTTON_FLAT: f64 = 0.7;

/// ISO 4014/4017 hex head: (size, across flats s, head height k).
const HEX_BOLT: [(&str, f64, f64); 14] = [
    ("M2", 4.0, 1.4),
    ("M2.5", 5.0, 1.7),
    ("M3", 5.5, 2.0),
    ("M4", 7.0, 2.8),
    ("M5", 8.0, 3.5),
    ("M6", 10.0, 4.0),
    ("M8", 13.0, 5.3),
    ("M10", 16.0, 6.4),
    ("M12", 18.0, 7.5),
    ("M14", 21.0, 8.8),
    ("M16", 24.0, 10.0),
    ("M20", 30.0, 12.5),
    ("M24", 36.0, 15.0),
    ("M30", 46.0, 18.7),
];

/// ISO 4032: (size, across flats s, height m).
const HEX_NUT: [(&str, f64, f64); 14] = [
    ("M2", 4.0, 1.6),
    ("M2.5", 5.0, 2.0),
    ("M3", 5.5, 2.4),
    ("M4", 7.0, 3.2),
    ("M5", 8.0, 4.7),
    ("M6", 10.0, 5.2),
    ("M8", 13.0, 6.8),
    ("M10", 16.0, 8.4),
    ("M12", 18.0, 10.8),
    ("M14", 21.0, 12.8),
    ("M16", 24.0, 14.8),
    ("M20", 30.0, 18.0),
    ("M24", 36.0, 21.5),
    ("M30", 46.0, 25.6),
];

/// ISO 7089: (size, inner d1, outer d2, thickness h).
const WASHER: [(&str, f64, f64, f64); 14] = [
    ("M2", 2.2, 5.0, 0.3),
    ("M2.5", 2.7, 6.0, 0.5),
    ("M3", 3.2, 7.0, 0.5),
    ("M4", 4.3, 9.0, 0.8),
    ("M5", 5.3, 10.0, 1.0),
    ("M6", 6.4, 12.0, 1.6),
    ("M8", 8.4, 16.0, 1.6),
    ("M10", 10.5, 20.0, 2.0),
    ("M12", 13.0, 24.0, 2.5),
    ("M14", 15.0, 28.0, 2.5),
    ("M16", 17.0, 30.0, 3.0),
    ("M20", 21.0, 37.0, 3.0),
    ("M24", 25.0, 44.0, 4.0),
    ("M30", 31.0, 56.0, 4.0),
];

/// ISO 4026: (size, socket s, socket depth t).
const SET_SCREW: [(&str, f64, f64); 7] =
    [("M3", 1.5, 1.2), ("M4", 2.0, 1.5), ("M5", 2.5, 2.0), ("M6", 3.0, 2.0), ("M8", 4.0, 3.0), ("M10", 5.0, 4.0), ("M12", 6.0, 4.8)];

/// ISO 8734 diameters.
const DOWEL: [f64; 9] = [2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 16.0];

/// Bearings: (designation, bore, outside diameter, width).
const BEARINGS: [(&str, f64, f64, f64); 13] = [
    ("625", 5.0, 16.0, 5.0),
    ("626", 6.0, 19.0, 6.0),
    ("608", 8.0, 22.0, 7.0),
    ("6000", 10.0, 26.0, 8.0),
    ("6001", 12.0, 28.0, 8.0),
    ("6002", 15.0, 32.0, 9.0),
    ("6003", 17.0, 35.0, 10.0),
    ("6004", 20.0, 42.0, 12.0),
    ("6200", 10.0, 30.0, 9.0),
    ("6201", 12.0, 32.0, 10.0),
    ("6202", 15.0, 35.0, 11.0),
    ("6203", 17.0, 40.0, 12.0),
    ("6204", 20.0, 47.0, 14.0),
];

/// Standard lengths for screws, bolts and pins (mm).
const LENGTHS: [f64; 22] =
    [4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 16.0, 20.0, 25.0, 30.0, 35.0, 40.0, 45.0, 50.0, 55.0, 60.0, 65.0, 70.0, 80.0, 90.0, 100.0, 120.0];

fn metric(size: &str) -> Option<(f64, f64)> {
    METRIC.iter().find(|(s, _, _)| s.eq_ignore_ascii_case(size)).map(|(_, d, p)| (*d, *p))
}

fn default_length(d: f64) -> f64 {
    LENGTHS.iter().copied().find(|l| *l >= 2.5 * d).unwrap_or(100.0)
}

/// One part's shape, in its own frame.
#[derive(Clone, Debug, PartialEq)]
enum Shape {
    /// A head (round, domed or hex), a shank down −z, an optional hex socket in the head's top.
    Screw { d: f64, pitch: f64, len: f64, head: Head, socket: Option<(f64, f64)> },
    /// A hex prism with a through hole (nut), z 0…m.
    Nut { d: f64, pitch: f64, s: f64, m: f64 },
    /// A ring (washer, bearing envelope), z 0…h.
    Ring { d_in: f64, d_out: f64, h: f64 },
    /// A cylinder down −z with a socket in its top (set screw), or plain (pin).
    Rod { d: f64, pitch: Option<f64>, len: f64, socket: Option<(f64, f64)> },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Head {
    Round {
        dk: f64,
        k: f64,
    },
    /// A button head: a short cylindrical flank, a crown domed over it, and a flat top of
    /// radius `flat` around the socket, at z = k.
    Dome {
        dk: f64,
        k: f64,
        flat: f64,
    },
    Hex {
        s: f64,
        k: f64,
    },
}

fn shape(family: &str, size: &str, length: Option<f64>) -> std::result::Result<Shape, String> {
    let need = |what: &str| format!("no {what} size `{size}` (parts.library lists them)");
    let len = |d: f64| length.unwrap_or_else(|| default_length(d));
    Ok(match family {
        "socket_head_cap_screw" => {
            let (d, pitch) = metric(size).ok_or_else(|| need("metric"))?;
            let (_, dk, k, s, t) = SHCS.iter().find(|r| r.0.eq_ignore_ascii_case(size)).copied().ok_or_else(|| need("ISO 4762"))?;
            Shape::Screw { d, pitch, len: len(d), head: Head::Round { dk, k }, socket: Some((s, t)) }
        }
        "button_head_cap_screw" => {
            let (d, pitch) = metric(size).ok_or_else(|| need("metric"))?;
            let (_, dk, k, s, t) = BHCS.iter().find(|r| r.0.eq_ignore_ascii_case(size)).copied().ok_or_else(|| need("ISO 7380-1"))?;
            Shape::Screw { d, pitch, len: len(d), head: Head::Dome { dk, k, flat: BUTTON_FLAT * s }, socket: Some((s, t)) }
        }
        "hex_bolt" => {
            let (d, pitch) = metric(size).ok_or_else(|| need("metric"))?;
            let (_, s, k) = HEX_BOLT.iter().find(|r| r.0.eq_ignore_ascii_case(size)).copied().ok_or_else(|| need("ISO 4014"))?;
            Shape::Screw { d, pitch, len: len(d), head: Head::Hex { s, k }, socket: None }
        }
        "hex_nut" => {
            let (d, pitch) = metric(size).ok_or_else(|| need("metric"))?;
            let (_, s, m) = HEX_NUT.iter().find(|r| r.0.eq_ignore_ascii_case(size)).copied().ok_or_else(|| need("ISO 4032"))?;
            Shape::Nut { d, pitch, s, m }
        }
        "washer" => {
            let (_, d1, d2, h) = WASHER.iter().find(|r| r.0.eq_ignore_ascii_case(size)).copied().ok_or_else(|| need("ISO 7089"))?;
            Shape::Ring { d_in: d1, d_out: d2, h }
        }
        "set_screw" => {
            let (d, pitch) = metric(size).ok_or_else(|| need("metric"))?;
            let (_, s, t) = SET_SCREW.iter().find(|r| r.0.eq_ignore_ascii_case(size)).copied().ok_or_else(|| need("ISO 4026"))?;
            Shape::Rod {
                d,
                pitch: Some(pitch),
                len: length.unwrap_or_else(|| LENGTHS.iter().copied().find(|l| *l >= d).unwrap_or(d)),
                socket: Some((s, t)),
            }
        }
        "dowel_pin" => {
            let d: f64 = size.trim_start_matches(['d', 'D', 'Ø']).trim().parse().map_err(|_| need("dowel pin"))?;
            if !DOWEL.contains(&d) {
                return Err(need("ISO 8734"));
            }
            Shape::Rod { d, pitch: None, len: len(d * 1.6), socket: None }
        }
        "bearing" => {
            let (_, bore, od, w) = BEARINGS.iter().find(|r| r.0 == size.trim()).copied().ok_or_else(|| need("bearing"))?;
            Shape::Ring { d_in: bore, d_out: od, h: w }
        }
        o => return Err(format!("unknown part family `{o}` (parts.library lists them)")),
    })
}

fn run(s: &mut Session, id: &str, p: Value) -> Result<Value> {
    s.execute(id, &p)
}

/// A hex prism (across flats `af`) from z0 to z1, joined or cut.
fn hex(s: &mut Session, af: f64, z0: f64, z1: f64, op: &str) -> Result<()> {
    run(s, "sketch.create", json!({"plane": "XY", "offset": format!("{z0} mm")}))?;
    run(s, "sketch.polygon.circumscribed", json!({"center": [0, 0], "radius": af / 2.0, "sides": 6}))?;
    fix_all(s)?;
    run(s, "sketch.finish", json!({}))?;
    let (dist, dir) = if z1 >= z0 { (z1 - z0, "positive") } else { (z0 - z1, "negative") };
    run(s, "solid.extrude", json!({"distance": format!("{dist} mm"), "direction": dir, "operation": op}))?;
    Ok(())
}

fn cyl(s: &mut Session, d: f64, z0: f64, z1: f64, op: &str) -> Result<()> {
    run(
        s,
        "solid.cylinder",
        json!({"diameter": format!("{d} mm"), "height": format!("{} mm", z1 - z0), "base": [0, 0, z0], "axis": [0, 0, 1], "operation": op}),
    )?;
    Ok(())
}

/// A solid of revolution about z from a closed (r, z) outline, as one body or joined.
fn revolve(s: &mut Session, pts: &[(f64, f64)], op: &str) -> Result<()> {
    revolve_outline(s, pts, None, op)
}

/// The same, with the outline closed from its last point back to its first by an arc through
/// `through`: a button head's domed crown.
fn revolve_domed(s: &mut Session, pts: &[(f64, f64)], through: (f64, f64), op: &str) -> Result<()> {
    revolve_outline(s, pts, Some(through), op)
}

fn revolve_outline(s: &mut Session, pts: &[(f64, f64)], arc: Option<(f64, f64)>, op: &str) -> Result<()> {
    run(s, "sketch.create", json!({"plane": "XZ"}))?;
    let xz = |(r, z): &(f64, f64)| [*r, -*z];
    let points: Vec<[f64; 2]> = pts.iter().map(xz).collect();
    run(s, "sketch.line", json!({"points": points, "closed": arc.is_none()}))?;
    if let (Some(through), Some(first), Some(last)) = (arc, pts.first(), pts.last()) {
        run(s, "sketch.arc.three_point", json!({"start": xz(last), "through": xz(&through), "end": xz(first)}))?;
    }
    fix_all(s)?;
    run(s, "sketch.finish", json!({}))?;
    run(s, "solid.revolve", json!({"axis": "y", "operation": op}))?;
    Ok(())
}

/// A point halfway along a button head's crown: on the circle centred on the axis that runs
/// from the top of the flank (a, c) to the rim of the flat top (b, k).
fn crown(a: f64, c: f64, b: f64, k: f64) -> (f64, f64) {
    // The centre on the axis, equidistant from both, and the radius from it.
    let zc = (b * b + k * k - a * a - c * c) / (2.0 * (k - c));
    let r = (b * b + (k - zc) * (k - zc)).sqrt();
    // Halfway between the directions to the two ends.
    let (x, z) = ((a + b) / r, (c - zc + k - zc) / r);
    let n = x.hypot(z);
    if n.is_finite() && n > 0.0 { (r * x / n, zc + r * z / n) } else { ((a + b) / 2.0, (c + k) / 2.0) }
}

/// Fix every point and curve of the sketch being drawn: a standard part's outline is its size, so
/// its sketches are fully defined and a check of the design finds nothing left free in them.
fn fix_all(s: &mut Session) -> Result<()> {
    let sketch = super::sketch_store::active(s).and_then(|id| super::sketch_store::read(&s.doc, id).ok());
    let entities: Vec<String> =
        sketch.map(|sk| sk.points.iter().map(|p| p.id.clone()).chain(sk.curves.iter().map(|c| c.id.clone())).collect()).unwrap_or_default();
    if !entities.is_empty() {
        run(s, "sketch.constraint.fix", json!({"entities": entities, "fixed": true}))?;
    }
    Ok(())
}

fn thread(s: &mut Session, d: f64, pitch: f64, at: Vec3) {
    let _ = run(s, "solid.thread", json!({"face": at, "designation": format!("M{}x{}", d, pitch)}));
    let _ = d;
}

/// Build a part in the active component.
fn build(s: &mut Session, sh: &Shape) -> Result<()> {
    let _frame = crate::frames::local_geometry();
    match *sh {
        Shape::Screw { d, pitch, len, head, socket } => {
            let k = match head {
                // One revolved outline: shank and round head.
                Head::Round { dk, k } => {
                    revolve(s, &[(0.0, -len), (d / 2.0, -len), (d / 2.0, 0.0), (dk / 2.0, 0.0), (dk / 2.0, k), (0.0, k)], "new")?;
                    k
                }
                // The same outline up to the head's rim, then a short flank, and the arc of the
                // crown closing it back to the flat top.
                Head::Dome { dk, k, flat } => {
                    let (a, c) = (dk / 2.0, BUTTON_FLANK * k);
                    let outline = [(flat, k), (0.0, k), (0.0, -len), (d / 2.0, -len), (d / 2.0, 0.0), (a, 0.0), (a, c)];
                    revolve_domed(s, &outline, crown(a, c, flat, k), "new")?;
                    k
                }
                // A hex head joined over a shank that runs up into it.
                Head::Hex { s: af, k } => {
                    hex(s, af, 0.0, k, "new")?;
                    revolve(s, &[(0.0, -len), (d / 2.0, -len), (d / 2.0, k / 2.0), (0.0, k / 2.0)], "join")?;
                    k
                }
            };
            if let Some((af, t)) = socket {
                hex(s, af, k, k - t, "cut")?;
            }
            thread(s, d, pitch, Vec3::new(d / 2.0, 0.0, -len / 2.0));
        }
        Shape::Nut { d, pitch, s: af, m } => {
            hex(s, af, 0.0, m, "new")?;
            cyl(s, d, -1.0, m + 1.0, "cut")?;
            thread(s, d, pitch, Vec3::new(d / 2.0, 0.0, m / 2.0));
        }
        Shape::Ring { d_in, d_out, h } => {
            revolve(s, &[(d_in / 2.0, 0.0), (d_out / 2.0, 0.0), (d_out / 2.0, h), (d_in / 2.0, h)], "new")?;
        }
        Shape::Rod { d, pitch, len, socket } => {
            revolve(s, &[(0.0, -len), (d / 2.0, -len), (d / 2.0, 0.0), (0.0, 0.0)], "new")?;
            if let Some((af, t)) = socket {
                hex(s, af, 0.0, -t, "cut")?;
            }
            if let Some(p) = pitch {
                thread(s, d, p, Vec3::new(d / 2.0, 0.0, -len / 2.0));
            }
        }
    }
    Ok(())
}

fn library(_s: &mut Session, p: &Value) -> Result<Value> {
    let fam = |id: &str| -> Value {
        let mut sizes: Vec<Value> = match id {
            "socket_head_cap_screw" => SHCS.iter().map(|r| json!({"size": r.0, "d": metric(r.0).map(|m| m.0), "pitch": metric(r.0).map(|m| m.1), "head_diameter": r.1, "head_height": r.2, "socket": r.3, "socket_depth": r.4})).collect(),
            "button_head_cap_screw" => BHCS.iter().map(|r| json!({"size": r.0, "d": metric(r.0).map(|m| m.0), "pitch": metric(r.0).map(|m| m.1), "head_diameter": r.1, "head_height": r.2, "socket": r.3, "socket_depth": r.4})).collect(),
            "hex_bolt" => HEX_BOLT.iter().map(|r| json!({"size": r.0, "d": metric(r.0).map(|m| m.0), "pitch": metric(r.0).map(|m| m.1), "across_flats": r.1, "head_height": r.2})).collect(),
            "hex_nut" => HEX_NUT.iter().map(|r| json!({"size": r.0, "d": metric(r.0).map(|m| m.0), "pitch": metric(r.0).map(|m| m.1), "across_flats": r.1, "height": r.2})).collect(),
            "washer" => WASHER.iter().map(|r| json!({"size": r.0, "inner": r.1, "outer": r.2, "thickness": r.3})).collect(),
            "set_screw" => SET_SCREW.iter().map(|r| json!({"size": r.0, "d": metric(r.0).map(|m| m.0), "pitch": metric(r.0).map(|m| m.1), "socket": r.1, "socket_depth": r.2})).collect(),
            "dowel_pin" => DOWEL.iter().map(|d| json!({"size": format!("{d}"), "d": d})).collect(),
            "bearing" => BEARINGS.iter().map(|r| json!({"size": r.0, "bore": r.1, "outer": r.2, "width": r.3})).collect(),
            _ => Vec::new(),
        };
        let (_, name, standard) = FAMILIES.iter().find(|f| f.0 == id).copied().unwrap_or(("", "", ""));
        let lengths =
            matches!(id, "socket_head_cap_screw" | "button_head_cap_screw" | "hex_bolt" | "set_screw" | "dowel_pin").then_some(LENGTHS.to_vec());
        if lengths.is_some() {
            for row in &mut sizes {
                let Some(size) = row.get("size").and_then(Value::as_str).map(str::to_string) else { continue };
                let Some(l) = shape(id, &size, None).ok().as_ref().and_then(shape_len) else { continue };
                if let Value::Object(m) = row {
                    m.insert("default_length".into(), json!(l));
                }
            }
        }
        json!({"family": id, "name": name, "standard": standard, "sizes": sizes, "lengths": lengths})
    };
    match str_(p, "family") {
        Some(f) if FAMILIES.iter().any(|x| x.0 == f) => Ok(fam(f)),
        Some(f) => Err(bad("parts.library", format!("unknown part family `{f}`"))),
        None => Ok(json!({"families": FAMILIES.iter().map(|f| fam(f.0)).collect::<Vec<_>>()})),
    }
}

fn part_name(family: &str, size: &str, sh: &Shape) -> String {
    match sh {
        Shape::Screw { len, .. } | Shape::Rod { len, .. } => format!("{} {size}x{len}", label(family)),
        _ => format!("{} {size}", label(family)),
    }
}

/// A standard part's part number and description: its standard and size (`ISO 4762 M6x20`), or
/// a bearing's designation, and the family's name with the size.
fn identity(family: &str, size: &str, sh: &Shape) -> (String, String) {
    let size = match sh {
        Shape::Screw { len, .. } | Shape::Rod { len, .. } => format!("{size}x{len}"),
        _ => size.to_string(),
    };
    let standard = match family {
        "socket_head_cap_screw" => "ISO 4762",
        "button_head_cap_screw" => "ISO 7380-1",
        "hex_bolt" => "ISO 4017",
        "hex_nut" => "ISO 4032",
        "washer" => "ISO 7089",
        "set_screw" => "ISO 4026",
        "dowel_pin" => "ISO 8734",
        _ => "",
    };
    let name = FAMILIES.iter().find(|f| f.0 == family).map_or("Part", |f| f.1);
    let part_number = if standard.is_empty() { size.clone() } else { format!("{standard} {size}") };
    (part_number, format!("{name} {size}"))
}

/// Give a standard part its part number and description; a field someone changed since the last
/// size (`was`) is theirs and stays.
fn set_identity(s: &mut Session, comp: u64, now: (String, String), was: Option<(String, String)>) {
    use solidcraft_doc::metadata::{DESCRIPTION, Owner, PART_NUMBER};
    let Some(c) = s.doc.components.iter().find(|c| c.id == comp) else { return };
    let mut m = c.metadata.clone();
    for (k, new, old) in [(PART_NUMBER, now.0, was.as_ref().map(|w| w.0.as_str())), (DESCRIPTION, now.1, was.as_ref().map(|w| w.1.as_str()))] {
        if m.text(k).is_none_or(|cur| Some(cur) == old) {
            let _ = m.set(Owner::Component, k, Value::String(new));
        }
    }
    if m != c.metadata
        && let Some(c) = s.doc_mut().components.iter_mut().find(|c| c.id == comp)
    {
        c.metadata = m;
    }
}

fn label(family: &str) -> &'static str {
    match family {
        "socket_head_cap_screw" => "SHCS",
        "button_head_cap_screw" => "BHCS",
        "hex_bolt" => "Hex Bolt",
        "hex_nut" => "Hex Nut",
        "washer" => "Washer",
        "set_screw" => "Set Screw",
        "dowel_pin" => "Dowel Pin",
        _ => "Bearing",
    }
}

fn length_param(p: &Value, cmd: &str) -> Result<Option<f64>> {
    match p.get("length") {
        None | Some(Value::Null) => Ok(None),
        Some(_) => {
            let l = num(p, "length").filter(|l| l.is_finite() && *l > 0.0 && *l <= 2000.0).ok_or_else(|| bad(cmd, "`length` must be 0…2000 mm"))?;
            Ok(Some(l))
        }
    }
}

/// Building the part and seating it runs many feature and joint commands: as in
/// [`resize_atomic`], they run on a scratch session and the insert is one undo step.
fn insert_atomic(s: &mut Session, p: &Value) -> Result<Value> {
    let mut computed = s.scratch();
    let result = insert(&mut computed, p)?;
    computed.undo.clear();
    computed.active_component = s.active_component;
    computed.active_occurrence_path = s.active_occurrence_path.clone();
    computed.active_sketch = s.active_sketch.filter(|id| computed.doc.feature(*id).is_some());
    s.accept_computed(computed);
    Ok(result)
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "parts.insert (Insert Fastener)";
    let family = str_(p, "family").ok_or_else(|| bad(cmd, "`family` is required (parts.library)"))?.to_string();
    let size = str_(p, "size").ok_or_else(|| bad(cmd, "`size` is required"))?.to_string();
    let sh = shape(&family, &size, length_param(p, cmd)?).map_err(|e| bad(cmd, e))?;
    // A pick on a hole's rim also touches its end face: step onto the wall just inside the rim.
    // Each hole's occurrence is found before the part exists, so a pick off every body fails the
    // insert before anything is made.
    let mut holes = Vec::new();
    let mut seats: Vec<(u64, Vec3)> = Vec::new();
    for a in hole_picks(p, cmd)? {
        let (a, seat) = hole_wall(s, a).unwrap_or((a, a));
        let host = super::joints::occurrence_at(s, a).ok_or_else(|| bad(cmd, "no body at `at` (pick a hole's rim or wall)"))?;
        // A rim is drawn as more than one edge, and one click on it picks them all: the picks
        // of one rim seat one part.
        if seats.iter().any(|(h, q)| *h == host && (*q - seat).len() < 1e-3) {
            continue;
        }
        seats.push((host, seat));
        holes.push((a, host));
    }
    let name = str_(p, "name").map(str::to_string).unwrap_or_else(|| part_name(&family, &size, &sh));
    let active = s.active_component;
    // Hardware is fitted to other parts: it starts free even as the design's first part (#2804).
    let c = run(s, "component.create", json!({"name": name, "grounded": false}))?;
    let (comp, occ) = (c["component"].as_u64().unwrap_or(0), c["occurrence"].as_u64().unwrap_or(0));
    build(s, &sh)?;
    s.doc_mut().parts.insert(comp, solidcraft_doc::StandardPart { family: family.clone(), size: size.clone(), length: shape_len(&sh) });
    set_identity(s, comp, identity(&family, &size, &sh), None);
    run(s, "component.activate", json!({"component": active}))?;
    let mut out = json!({"component": comp, "occurrence": occ, "name": name});
    if !holes.is_empty() {
        // The first hole has the placement the component came with; each further hole places the
        // same part again. All are made before any is seated, while each is still at the part's
        // own origin, where the joint below takes the part's seating point to be.
        let mut occurrences = vec![json!(occ)];
        for _ in 1..holes.len() {
            occurrences.push(run(s, "occurrence.copy", json!({"component": comp}))?["occurrence"].clone());
        }
        let mut joints = Vec::new();
        for (i, ((a, h), o)) in holes.into_iter().zip(&occurrences).enumerate() {
            // Seat it: its axis on the hole's, its seating plane on the hole's face, the shank
            // going into the hole (the joint mates the part's −z with the face's outward z).
            let seat = if i == 0 { format!("{name} seat") } else { format!("{name} seat {}", i + 1) };
            let j = run(
                s,
                "joint.create",
                json!({"type": "rigid", "a": {"occurrence": h, "circle": a}, "b": {"occurrence": o, "point": [0, 0, 0], "z": [0, 0, -1]}, "name": seat}),
            )?;
            joints.push(j["joint"].clone());
        }
        out["joint"] = joints.first().cloned().unwrap_or(Value::Null);
        out["occurrences"] = Value::Array(occurrences);
        out["joints"] = Value::Array(joints);
    } else if let Some(pt) = p.get("point").and_then(vec3) {
        // Placed at a point, its +z along `direction`.
        let dir = p.get("direction").and_then(vec3).and_then(|d| d.normalized()).unwrap_or(Vec3::Z);
        let m = solidcraft_doc::rigid(
            Vec3::ZERO,
            Vec3::ZERO,
            Vec3::Z.cross(dir).normalized().unwrap_or(Vec3::X),
            Vec3::Z.dot(dir).clamp(-1.0, 1.0).acos(),
        );
        let mut m = m;
        m[3] = [pt.x, pt.y, pt.z, 1.0];
        if !solidcraft_doc::is_placement(&m) {
            return Err(bad(cmd, "`point` must be within 1e9 mm and `direction` finite"));
        }
        if let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.id == occ) {
            o.transform = m;
        }
    }
    Ok(out)
}

/// The most holes one insert seats a part in.
const MAX_HOLES: usize = 1000;

/// The hole picks in `at`: one point, or a list of them.
fn hole_picks(p: &Value, cmd: &str) -> Result<Vec<Vec3>> {
    let finite = |a: Vec3| a.x.is_finite() && a.y.is_finite() && a.z.is_finite();
    let one = |v: &Value| vec3(v).filter(|a| finite(*a)).ok_or_else(|| bad(cmd, "`at` must be [x,y,z] or a list of them"));
    match p.get("at") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(list)) if list.first().is_some_and(Value::is_array) || list.is_empty() => {
            if list.len() > MAX_HOLES {
                return Err(bad(cmd, format!("`at` takes at most {MAX_HOLES} holes")));
            }
            list.iter().map(one).collect()
        }
        Some(v) => Ok(vec![one(v)?]),
    }
}

/// A point on the cylindrical wall of the hole at `a`, a little in from the picked rim, and the
/// point on the hole's axis level with it (the same for every pick on one rim).
fn hole_wall(s: &Session, a: Vec3) -> Option<(Vec3, Vec3)> {
    // Picks are in world coordinates, including rims on already placed standard parts.
    let st = s.world_state().ok()?;
    let dirs = [Vec3::Z * -1.0, Vec3::Z, Vec3::X, Vec3::X * -1.0, Vec3::Y, Vec3::Y * -1.0];
    for step in [0.2, 0.5, 1.0, 2.0] {
        for b in &st.bodies {
            for d in dirs {
                let q = a + d * step;
                if let Some(c) = solidcraft_doc::kernel::cylinder_face_at(&b.body, q) {
                    let v = q - c.axis_point;
                    let along = v.dot(c.axis);
                    let on_wall = ((v - c.axis * along).len() - c.radius).abs() < 1e-3 && along > c.start && along < c.end;
                    if on_wall {
                        return Some((q, c.axis_point + c.axis * along));
                    }
                }
            }
        }
    }
    None
}

fn shape_len(sh: &Shape) -> Option<f64> {
    match sh {
        Shape::Screw { len, .. } | Shape::Rod { len, .. } => Some(*len),
        _ => None,
    }
}

/// Building the replacement runs several feature commands. Keep their intermediate history
/// and any failed build on a scratch session; the outer command records one complete undo.
fn resize_atomic(s: &mut Session, p: &Value) -> Result<Value> {
    let mut computed = s.scratch();
    let result = resize(&mut computed, p)?;
    computed.undo.clear();
    computed.active_component = s.active_component;
    computed.active_occurrence_path = s.active_occurrence_path.clone();
    computed.active_sketch = s.active_sketch.filter(|id| computed.doc.feature(*id).is_some());
    s.accept_computed(computed);
    Ok(result)
}

fn resize(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "parts.resize";
    let key = match p.get("component") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad(cmd, "`component` must be an id or a name")),
    };
    let comp = s.doc.find_component(&key).ok_or_else(|| bad(cmd, format!("no component `{key}`")))?;
    let part = s.doc.parts.get(&comp).cloned().ok_or_else(|| bad(cmd, "that component is not a standard part"))?;
    let size = str_(p, "size").map(str::to_string).unwrap_or(part.size.clone());
    let length = length_param(p, cmd)?.or(if size == part.size { part.length } else { None });
    let sh = shape(&part.family, &size, length).map_err(|e| bad(cmd, e))?;
    // Rebuild: the component's features go, the new shape is built in their place.
    let ids: Vec<u64> = s.doc.features.iter().filter(|f| f.component == comp).map(|f| f.id).collect();
    for id in ids.iter().rev() {
        let _ = s.doc_mut().delete_feature(*id);
    }
    let active = s.active_component;
    run(s, "component.activate", json!({"component": comp}))?;
    build(s, &sh)?;
    let name = part_name(&part.family, &size, &sh);
    let was = shape(&part.family, &part.size, part.length).ok().map(|old| identity(&part.family, &part.size, &old));
    set_identity(s, comp, identity(&part.family, &size, &sh), was);
    s.doc_mut().parts.insert(comp, solidcraft_doc::StandardPart { family: part.family.clone(), size: size.clone(), length: shape_len(&sh) });
    if let Some(c) = s.doc_mut().components.iter_mut().find(|c| c.id == comp) {
        c.name = name.clone();
    }
    run(s, "component.activate", json!({"component": active}))?;
    s.refresh();
    if let Some(e) = s.model.results.iter().find_map(|r| r.error.clone()) {
        return Err(EngineError::Other(e));
    }
    Ok(json!({"component": comp, "name": name, "size": size, "length": shape_len(&sh)}))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::Session;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        match s.execute(id, &p) {
            Ok(v) => v,
            Err(e) => panic!("{id} {p}: {e}"),
        }
    }

    fn bbox(s: &Session, prefix: &str) -> (solidcraft_geom::Vec3, solidcraft_geom::Vec3) {
        let st = s.world_state().unwrap();
        let comp = s.doc.components.iter().find(|c| c.name.starts_with(prefix)).map(|c| c.id).unwrap_or(0);
        let b = st
            .bodies
            .iter()
            .filter(|b| s.doc.model_body_component(b) == comp)
            .fold(solidcraft_geom::Aabb3::EMPTY, |a, b| a.union(&b.mesh().bounds()));
        (b.min, b.max)
    }

    #[test]
    fn dimensions_follow_the_standard_tables() {
        let l = run(&mut Session::default(), "parts.library", json!({"family": "socket_head_cap_screw"}));
        let m6 = l["sizes"].as_array().and_then(|a| a.iter().find(|x| x["size"] == "M6")).cloned().unwrap_or_default();
        assert_eq!((m6["head_diameter"].as_f64(), m6["head_height"].as_f64(), m6["socket"].as_f64()), (Some(10.0), Some(6.0), Some(5.0)));
        // An M6 x 20 cap screw: head Ø10 x 6, shank 20 long.
        let mut s = Session::default();
        run(&mut s, "parts.insert", json!({"family": "socket_head_cap_screw", "size": "M6", "length": 20}));
        let (lo, hi) = bbox(&s, "SHCS M6x20");
        assert!((hi.x - lo.x - 10.0).abs() < 1e-6 && (lo.z + 20.0).abs() < 1e-6 && (hi.z - 6.0).abs() < 1e-6, "{lo:?} {hi:?}");
        assert!(s.model.state().threads.iter().any(|t| t.designation.starts_with("M6")), "{:?}", s.model.state().threads);
        // An M8 nut: 13 across flats (14.43 across corners), 6.8 high.
        let mut n = Session::default();
        run(&mut n, "parts.insert", json!({"family": "hex_nut", "size": "M8"}));
        let (lo, hi) = bbox(&n, "Hex Nut M8");
        let (w, h) = ((hi.x - lo.x).max(hi.y - lo.y), hi.z - lo.z);
        assert!((w - 13.0 / (std::f64::consts::PI / 6.0).cos()).abs() < 1e-6 && (h - 6.8).abs() < 1e-6, "{w} {h}");
        // A 608 bearing: 8 x 22 x 7.
        let mut b = Session::default();
        run(&mut b, "parts.insert", json!({"family": "bearing", "size": "608"}));
        let (lo, hi) = bbox(&b, "Bearing 608");
        assert!((hi.x - lo.x - 22.0).abs() < 1e-6 && (hi.z - lo.z - 7.0).abs() < 1e-6);
        assert!(b.execute("parts.insert", &json!({"family": "hex_nut", "size": "M7"})).is_err());
    }

    /// The Length list of a dialog opens on the length an insert gets when none is asked for,
    /// so the library reports that length for every size of every family that has lengths. The
    /// rule differs by family, so the reported value is the one the part is actually built to.
    #[test]
    fn the_library_reports_the_default_length_of_every_size() {
        let all = run(&mut Session::default(), "parts.library", json!({}));
        let families = all["families"].as_array().cloned().unwrap_or_default();
        assert!(!families.is_empty(), "{all}");
        for fam in families {
            let id = fam["family"].as_str().unwrap_or_default().to_string();
            let standard: Vec<f64> = fam["lengths"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
            for row in fam["sizes"].as_array().cloned().unwrap_or_default() {
                let size = row["size"].as_str().unwrap_or_default().to_string();
                let reported = row["default_length"].as_f64();
                if standard.is_empty() {
                    assert!(reported.is_none(), "{id} {size} has no lengths of its own but reports a default");
                    continue;
                }
                // What an insert that asks for no length is actually built to.
                let mut s = Session::default();
                let out = run(&mut s, "parts.insert", json!({"family": id, "size": size}));
                let built = s.doc.parts.get(&out["component"].as_u64().unwrap_or(0)).and_then(|p| p.length);
                assert_eq!(reported, built, "{id} {size}: reported default is not what an insert builds");
                assert!(standard.iter().any(|l| Some(*l) == reported), "{id} {size}: {reported:?} is not one of the standard lengths");
            }
        }
    }

    /// #7692: a button head cap screw (ISO 7380-1) takes the same parameters as a socket head
    /// cap screw, with a domed crown in place of the cylindrical head.
    #[test]
    fn a_button_head_is_domed_over_the_shank() {
        let l = run(&mut Session::default(), "parts.library", json!({"family": "button_head_cap_screw"}));
        let m6 = l["sizes"].as_array().and_then(|a| a.iter().find(|x| x["size"] == "M6")).cloned().unwrap_or_default();
        assert_eq!((m6["head_diameter"].as_f64(), m6["head_height"].as_f64(), m6["socket"].as_f64()), (Some(10.5), Some(3.3), Some(4.0)));
        // ISO 7380-1 has no M2, M2.5, M14 or larger than M16; the lengths are a screw's.
        let sizes: Vec<&str> = l["sizes"].as_array().map(|a| a.iter().filter_map(|x| x["size"].as_str()).collect()).unwrap_or_default();
        assert_eq!(sizes, ["M3", "M4", "M5", "M6", "M8", "M10", "M12", "M16"]);
        assert!(l["lengths"].as_array().is_some_and(|a| !a.is_empty()), "{l}");
        // An M6 x 20 button head: head Ø10.5, 3.3 high over the seating plane, shank 20 long.
        let mut s = Session::default();
        run(&mut s, "parts.insert", json!({"family": "button_head_cap_screw", "size": "M6", "length": 20}));
        let (lo, hi) = bbox(&s, "BHCS M6x20");
        assert!((hi.x - lo.x - 10.5).abs() < 1e-3 && (lo.z + 20.0).abs() < 1e-6 && (hi.z - 3.3).abs() < 1e-3, "{lo:?} {hi:?}");
        assert!(s.model.state().threads.iter().any(|t| t.designation.starts_with("M6")), "{:?}", s.model.state().threads);
        // The profile: full width at the rim and up the flank (0.15 k = 0.50 high), drawing in
        // over the crown (r = 4.62 halfway up the head), and a flat top of 0.7 s = 2.8, which
        // leaves a ring of material around the socket's corners at r = 2.31.
        let st = s.world_state().unwrap();
        let widest = |above: f64| {
            st.bodies.iter().flat_map(|b| b.mesh().positions.clone()).filter(|p| p.z > above).map(|p| p.x.hypot(p.y)).fold(0.0, f64::max)
        };
        assert!((5.2..5.26).contains(&widest(-0.1)), "the rim: {}", widest(-0.1));
        assert!((5.2..5.26).contains(&widest(0.3)), "the flank: {}", widest(0.3));
        assert!((3.5..4.7).contains(&widest(1.65)), "the crown halfway up: {}", widest(1.65));
        assert!((2.6..2.9).contains(&widest(3.29)), "the flat top: {}", widest(3.29));
        // Its part number is the standard and size, as the other families' are.
        let comp = s.doc.components.iter().find(|c| c.name.starts_with("BHCS")).map(|c| c.id).unwrap_or(0);
        let number = s.doc.components.iter().find(|c| c.id == comp).and_then(|c| c.metadata.text(solidcraft_doc::metadata::PART_NUMBER));
        assert_eq!(number, Some("ISO 7380-1 M6x20"));
    }

    /// #2091: the second and larger ISO sizes, M2, M2.5, M14, M24 and M30, are in the screw, bolt,
    /// nut and washer tables (they stopped at M3 and M20).
    #[test]
    fn small_and_large_iso_sizes_insert() {
        for family in ["socket_head_cap_screw", "hex_bolt", "hex_nut", "washer"] {
            let l = run(&mut Session::default(), "parts.library", json!({"family": family}));
            let sizes: Vec<&str> = l["sizes"].as_array().unwrap().iter().filter_map(|x| x["size"].as_str()).collect();
            for size in ["M2", "M2.5", "M14", "M24", "M30"] {
                assert!(sizes.contains(&size), "{family} has {size}: {sizes:?}");
            }
        }
        // An M2.5 x 8 cap screw: head Ø4.5 x 2.5, shank 8 long.
        let mut s = Session::default();
        run(&mut s, "parts.insert", json!({"family": "socket_head_cap_screw", "size": "M2.5", "length": 8}));
        let (lo, hi) = bbox(&s, "SHCS M2.5x8");
        assert!((hi.x - lo.x - 4.5).abs() < 1e-6 && (lo.z + 8.0).abs() < 1e-6 && (hi.z - 2.5).abs() < 1e-6, "{lo:?} {hi:?}");
        // An M24 nut: 36 across flats, 21.5 high.
        let mut n = Session::default();
        run(&mut n, "parts.insert", json!({"family": "hex_nut", "size": "M24"}));
        let (lo, hi) = bbox(&n, "Hex Nut M24");
        let (w, h) = ((hi.x - lo.x).max(hi.y - lo.y), hi.z - lo.z);
        assert!((w - 36.0 / (std::f64::consts::PI / 6.0).cos()).abs() < 1e-6 && (h - 21.5).abs() < 1e-6, "{w} {h}");
        // An M30 washer: Ø56 x 4.
        let mut w = Session::default();
        run(&mut w, "parts.insert", json!({"family": "washer", "size": "M30"}));
        let (lo, hi) = bbox(&w, "Washer M30");
        assert!((hi.x - lo.x - 56.0).abs() < 1e-6 && (hi.z - lo.z - 4.0).abs() < 1e-6, "{lo:?} {hi:?}");
    }

    #[test]
    fn a_screw_seats_in_a_hole_and_resizes() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"}));
        run(&mut s, "solid.hole", json!({"position": [20, 15, 10], "diameter": 6.4}));
        // Pick the hole's rim at the top face.
        let r = run(&mut s, "parts.insert", json!({"family": "socket_head_cap_screw", "size": "M6", "length": 20, "at": [23.2, 15, 10]}));
        assert!(r["joint"].is_number(), "{r}");
        let (lo, hi) = bbox(&s, "SHCS M6x20");
        // The head sits flush on the top face, the shank down the hole's axis.
        assert!((hi.z - 16.0).abs() < 1e-6 && (lo.z + 10.0).abs() < 1e-6, "{lo:?} {hi:?}");
        assert!(((lo.x + hi.x) / 2.0 - 20.0).abs() < 1e-6 && ((lo.y + hi.y) / 2.0 - 15.0).abs() < 1e-6);
        // M8 x 25: rebuilt in place, still seated.
        let comp = r["component"].as_u64().unwrap_or(0);
        run(&mut s, "parts.resize", json!({"component": comp, "size": "M8", "length": 25}));
        let (lo, hi) = bbox(&s, "SHCS M8x25");
        assert!((hi.z - 18.0).abs() < 1e-6 && (lo.z + 15.0).abs() < 1e-6, "{lo:?} {hi:?}");
        assert!(s.model.state().threads.iter().any(|t| t.designation == "M8" && (t.pitch - 1.25).abs() < 1e-9));
    }

    #[test]
    fn a_screw_seats_on_a_translated_washer() {
        for z in [11.6, 11.2] {
            let mut s = Session::default();
            run(&mut s, "solid.box", json!({"length":60,"width":60,"height":10,"body_name":"Plate"}));
            run(&mut s, "solid.hole", json!({"position":[19.6,33.948,10],"diameter":8.4}));
            let washer = run(&mut s, "parts.insert", json!({"family":"washer","size":"M8","at":[23.8,33.948,10]}));
            let before = s.doc.clone();
            let undo = s.undo.len();
            // Both the top bore rim and the wall are world picks on the seated washer (#9648).
            let screw = run(&mut s, "parts.fastener", json!({"family":"socket_head_cap_screw","size":"M8","length":20,"at":[23.8,33.948,z]}));
            let joint = s.doc.assembly.joints.iter().find(|j| Some(j.id) == screw["joint"].as_u64()).unwrap();
            assert_eq!(Some(joint.a.occurrence), washer["occurrence"].as_u64());
            let (lo, hi) = bbox(&s, "SHCS M8x20");
            assert!((lo.z + 8.4).abs() < 1e-6 && (hi.z - 19.6).abs() < 1e-6, "{lo:?} {hi:?}");
            assert!(((lo.x + hi.x) / 2.0 - 19.6).abs() < 1e-6 && ((lo.y + hi.y) / 2.0 - 33.948).abs() < 1e-6, "{lo:?} {hi:?}");
            assert_eq!(s.undo.len(), undo + 1);
            run(&mut s, "edit.undo", json!({}));
            assert_eq!(s.doc, before);
        }
    }

    #[test]
    fn a_screw_seats_on_a_rotated_washer() {
        let mut s = Session::default();
        let washer = run(&mut s, "parts.insert", json!({"family":"washer","size":"M8","point":[40,50,60],"direction":[1,0,0]}));
        run(&mut s, "occurrence.ground", json!({"occurrence":washer["occurrence"],"grounded":true}));
        let screw = run(&mut s, "parts.insert", json!({"family":"socket_head_cap_screw","size":"M8","length":20,"at":[41.6,54.2,60]}));
        let joint = s.doc.assembly.joints.iter().find(|j| Some(j.id) == screw["joint"].as_u64()).unwrap();
        assert_eq!(Some(joint.a.occurrence), washer["occurrence"].as_u64());
        // The seating face is X = 41.6: the 20 mm shank runs along -X and the 8 mm head along +X.
        let (lo, hi) = bbox(&s, "SHCS M8x20");
        assert!(lo.dist(solidcraft_geom::Vec3::new(21.6, 43.5, 53.5)) < 1e-6, "{lo:?}");
        assert!(hi.dist(solidcraft_geom::Vec3::new(49.6, 56.5, 66.5)) < 1e-6, "{hi:?}");
    }

    /// Several holes in one insert: one component placed and seated once per hole, in one undo
    /// step (#9341); two picks of one rim are one hole; a pick off every body fails the whole
    /// insert.
    #[test]
    fn one_insert_seats_a_screw_in_each_hole() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"}));
        let centres = [(8.0, 8.0), (32.0, 8.0), (32.0, 22.0), (8.0, 22.0)];
        for (x, y) in centres {
            run(&mut s, "solid.hole", json!({"position": [x, y, 10], "diameter": 3.4}));
        }
        let (comps, occs, undo) = (s.doc.components.len(), s.doc.occurrences.len(), s.undo.len());
        // A hole off the plate fails the insert, and nothing is made.
        let off = json!({"family": "socket_head_cap_screw", "size": "M3", "length": 8, "at": [[9.7, 8, 10], [100, 100, 100]]});
        assert!(s.execute("parts.insert", &off).is_err());
        assert_eq!((s.doc.components.len(), s.doc.occurrences.len()), (comps, occs));
        // Each rim picked at the top face.
        // The first hole is picked twice, on both halves of its rim, as one click on a rim picks
        // every edge of it: it still gets one screw.
        let mut at: Vec<_> = centres.iter().map(|(x, y)| json!([x + 1.7, y, 10])).collect();
        at.insert(1, json!([centres[0].0 - 1.7, centres[0].1, 10]));
        let r = run(&mut s, "parts.fastener", json!({"family": "socket_head_cap_screw", "size": "M3", "length": 8, "at": at}));
        assert_eq!(s.doc.components.len(), comps + 1, "one part: {r}");
        let comp = r["component"].as_u64().unwrap_or(0);
        let placed: Vec<u64> = r["occurrences"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
        assert_eq!(placed.len(), 4, "{r}");
        assert!(placed.iter().all(|o| s.doc.occurrences.iter().any(|x| x.id == *o && x.component == comp)), "{r}");
        assert_eq!(r["joints"].as_array().map(|j| j.iter().filter(|j| j.is_number()).count()), Some(4), "{r}");
        assert_eq!(r["joint"], r["joints"][0]);
        // Each placement sits in its own hole: the head flush on the top face, the shank down
        // the hole's axis.
        let st = s.world_state().unwrap();
        let mut seated: Vec<(f64, f64)> = Vec::new();
        for b in st.bodies.iter().filter(|b| s.doc.model_body_component(b) == comp) {
            let (lo, hi) = (b.mesh().bounds().min, b.mesh().bounds().max);
            assert!((hi.z - 13.0).abs() < 1e-6 && (lo.z - 2.0).abs() < 1e-6, "{lo:?} {hi:?}");
            seated.push(((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0));
        }
        assert_eq!(seated.len(), 4, "{seated:?}");
        for (x, y) in centres {
            assert!(seated.iter().any(|c| (c.0 - x).abs() < 1e-6 && (c.1 - y).abs() < 1e-6), "no screw at ({x}, {y}): {seated:?}");
        }
        // One undo takes them all away.
        assert_eq!(s.undo.len(), undo + 1);
        run(&mut s, "edit.undo", json!({}));
        assert_eq!((s.doc.components.len(), s.doc.occurrences.len()), (comps, occs));
    }

    #[test]
    fn every_sketch_of_a_standard_part_is_fully_defined() {
        for (family, size, resized) in [
            ("socket_head_cap_screw", "M6", "M8"),
            ("button_head_cap_screw", "M6", "M8"),
            ("hex_bolt", "M6", "M8"),
            ("hex_nut", "M6", "M8"),
            ("washer", "M6", "M8"),
            ("set_screw", "M6", "M8"),
            ("dowel_pin", "6", "8"),
            ("bearing", "608", "6200"),
        ] {
            let mut s = Session::default();
            let part = run(&mut s, "parts.insert", json!({"family": family, "size": size}));
            for step in ["inserted", "resized"] {
                let st = s.model.state();
                let free: Vec<_> =
                    st.sketches.iter().filter(|k| k.report.dof != 0 || !k.report.fully_constrained()).map(|k| (&k.name, k.report.dof)).collect();
                assert!(!st.sketches.is_empty() && free.is_empty(), "{family} {step}: {free:?}");
                assert!(s.model.results.iter().all(|r| r.error.is_none()), "{family} {step}");
                run(&mut s, "parts.resize", json!({"component": part["component"], "size": resized}));
            }
        }
    }

    #[test]
    fn resizing_placed_parts_uses_local_geometry_and_millimetres_in_every_design_unit() {
        for (family, initial, resized, prefix) in [
            ("socket_head_cap_screw", "M6", "M8", "SHCS"),
            ("button_head_cap_screw", "M6", "M8", "BHCS"),
            ("hex_bolt", "M6", "M8", "Hex Bolt"),
            ("hex_nut", "M6", "M8", "Hex Nut"),
            ("washer", "M6", "M8", "Washer"),
            ("set_screw", "M6", "M8", "Set Screw"),
            ("dowel_pin", "6", "8", "Dowel Pin"),
            ("bearing", "608", "6200", "Bearing"),
        ] {
            let mut bounds = Vec::new();
            for units in ["mm", "in"] {
                let mut s = Session::default();
                run(&mut s, "document.units", json!({"units":units}));
                let part = run(&mut s, "parts.insert", json!({"family":family,"size":initial,"length":20,"point":[40,50,60],"direction":[1,0,0]}));
                run(&mut s, "parts.resize", json!({"component":part["component"],"size":resized,"length":30}));
                assert!(s.model.results.iter().all(|r| r.error.is_none()), "{family} {units}");
                let b = bbox(&s, prefix);
                assert!(b.0.x < b.1.x && b.0.y < b.1.y && b.0.z < b.1.z, "{family} {units}: {b:?}");
                if matches!(family, "socket_head_cap_screw" | "button_head_cap_screw" | "hex_bolt" | "hex_nut" | "set_screw") {
                    assert!(s.model.state().threads.iter().any(|t| t.designation == "M8" && (t.pitch - 1.25).abs() < 1e-9), "{family} {units}");
                }
                if family == "dowel_pin" {
                    assert!(b.0.dist(solidcraft_geom::Vec3::new(10., 46., 56.)) < 1e-6, "{b:?}");
                    assert!(b.1.dist(solidcraft_geom::Vec3::new(40., 54., 64.)) < 1e-6, "{b:?}");
                }
                bounds.push(b);
            }
            assert!(bounds[0].0.dist(bounds[1].0) < 1e-6 && bounds[0].1.dist(bounds[1].1) < 1e-6, "{family}: {bounds:?}");
        }
    }

    #[test]
    fn resizing_a_part_is_one_undo_and_keeps_the_active_occurrence() {
        let mut s = Session::default();
        let part = run(&mut s, "parts.insert", json!({"family":"socket_head_cap_screw","size":"M6","length":20}));
        let id = part["component"].as_u64().unwrap_or(0);
        let copied = run(&mut s, "occurrence.copy", json!({"component":id,"translate":[40,0,0]}));
        let path = vec![copied["occurrence"].as_u64().unwrap_or(0)];
        run(&mut s, "component.activate", json!({"component":id,"occurrence":copied["occurrence"],"path":path}));
        let before = s.doc.clone();
        let undo = s.undo.len();
        run(&mut s, "parts.resize", json!({"component":id,"size":"M8","length":25}));
        assert_eq!(s.undo.len(), undo + 1);
        assert_eq!(s.active_component, id);
        assert_eq!(s.active_occurrence_path, Some(path.clone()));
        let after = s.doc.clone();
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc, before);
        let redo = s.redo.len();
        assert!(s.execute("parts.resize", &json!({"component":id,"length":0})).is_err());
        assert_eq!(s.doc, before);
        assert_eq!(s.redo.len(), redo);
        assert_eq!(s.active_occurrence_path, Some(path));
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc, after);
    }

    #[test]
    fn resizing_a_part_at_the_history_limit_drops_only_one_old_step() {
        let mut s = Session::default();
        let part = run(&mut s, "parts.insert", json!({"family":"washer","size":"M6"}));
        let Some(snapshot) = s.undo.last().cloned() else { panic!("insert did not record history") };
        s.undo = (0..crate::MAX_UNDO)
            .map(|index| {
                let mut old = snapshot.clone();
                old.label = format!("Earlier step {index}");
                old
            })
            .collect();
        run(&mut s, "parts.resize", json!({"component":part["component"],"size":"M8"}));
        assert_eq!(s.undo.len(), crate::MAX_UNDO);
        assert_eq!(s.undo.first().map(|step| step.label.as_str()), Some("Earlier step 1"));
        assert_eq!(s.undo.last().map(|step| step.label.as_str()), Some("Change Part Size"));
    }
}
