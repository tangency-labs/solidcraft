//! Timing-belt pulleys (#20116): GT2 (2 mm pitch), HTD 3M and HTD 5M, set by the number of
//! grooves and the belt's width. Each pulley is inserted as its own component built by ordinary
//! features: the grooved outline sketched and extruded, a flange at each end (optional), and an
//! axle hole cut through (optional, off the axis with `bore_offset`).
//!
//! The pitch circle is the belt's pitch line: z·p/π across. The pulley's outside lies the
//! profile's pitch line differential (PLD) inside it, z·p/π − 2·PLD across (GT2 20 grooves:
//! 12.22 mm). Each groove is one circular arc of the profile's depth and radius: a close fit for
//! printing and machining, not the belt makers' own tooth geometry.
//!
//! Part frame: the axis is z, the pulley stands on z = 0 (its first flange, or its teeth without
//! flanges) and rises along +z.
//!
//! Parameter names follow `gear.insert` (#13431) where they mean the same: `teeth`,
//! `bore_diameter`, `bore_offset`, `name`.

use serde_json::{Value, json};

use super::CommandSpec;
use crate::params::{bad, bool_, num, str_};
use crate::schema::pulleys::{InsertedPulley, PulleyInsertParams};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const INSERT_CONTRACT: TypedContract<PulleyInsertParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("pulley.insert", "Insert Pulley", crate::checked!("pulley.insert", insert_atomic))
    .icon("gear")
    .contract(&INSERT_CONTRACT)
    .output(output_schema_for::<InsertedPulley>)
    .params(
        "profile (gt2: 2 mm pitch | htd3m | htd5m), teeth (10..200: the grooves), belt_width? (mm, 1..100, default 6 for gt2, 9 for htd3m, \
         15 for htd5m; the toothed part is 1 mm wider so the belt runs free), flanges? (bool, default true: a flange at each end), \
         bore_diameter? (mm: an axle hole; none without it), bore_offset? (mm: the hole's centre along +X), name? \
         → component, occurrence, name, pitch_diameter, outside_diameter, width (mm, flanges included)",
    )];

/// A belt profile: its id, pitch p, pitch line differential, groove depth and radius, flange
/// thickness and the belt width it gets by default (mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Profile {
    pub id: &'static str,
    pub pitch: f64,
    pub pld: f64,
    pub depth: f64,
    pub groove_radius: f64,
    pub flange: f64,
    pub belt_width: f64,
}

/// The profiles: pitch and pitch line differential are the belt standards' published values;
/// the groove's depth and radius approximate each tooth.
pub(crate) const PROFILES: [Profile; 3] = [
    Profile { id: "gt2", pitch: 2.0, pld: 0.254, depth: 0.75, groove_radius: 0.555, flange: 1.0, belt_width: 6.0 },
    Profile { id: "htd3m", pitch: 3.0, pld: 0.381, depth: 1.14, groove_radius: 0.86, flange: 1.0, belt_width: 9.0 },
    Profile { id: "htd5m", pitch: 5.0, pld: 0.5715, depth: 2.06, groove_radius: 1.49, flange: 1.5, belt_width: 15.0 },
];

/// How much wider the toothed part is than the belt (mm), so the belt runs free between the flanges.
pub(crate) const BELT_CLEARANCE: f64 = 1.0;

/// The most and fewest grooves a pulley may have.
const TEETH: (u32, u32) = (10, 200);
/// The widest belt (mm).
const MAX_BELT: f64 = 100.0;
/// The least wall left between the axle hole and the grooves' bottoms (mm).
const MIN_WALL: f64 = 0.5;

pub(crate) fn profile(id: &str) -> Option<Profile> {
    PROFILES.iter().copied().find(|p| p.id == id.trim().to_ascii_lowercase())
}

/// A pulley's sizes, all in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pulley {
    pub profile: Profile,
    pub teeth: u32,
    pub belt_width: f64,
    pub flanges: bool,
    /// The axle hole: diameter and its centre's offset along +x.
    pub bore: Option<(f64, f64)>,
}

impl Pulley {
    pub fn pitch_diameter(&self) -> f64 {
        f64::from(self.teeth) * self.profile.pitch / std::f64::consts::PI
    }

    pub fn outside_radius(&self) -> f64 {
        self.pitch_diameter() / 2.0 - self.profile.pld
    }

    /// The radius at the grooves' bottoms.
    pub fn root_radius(&self) -> f64 {
        self.outside_radius() - self.profile.depth
    }

    /// The toothed part's width along z.
    pub fn toothed_width(&self) -> f64 {
        self.belt_width + BELT_CLEARANCE
    }

    pub fn flange_thickness(&self) -> f64 {
        if self.flanges { self.profile.flange } else { 0.0 }
    }

    /// A flange's diameter: enough above the grooves to keep the belt on.
    pub fn flange_diameter(&self) -> f64 {
        2.0 * (self.outside_radius() + 1.5 * self.profile.depth + 0.5)
    }

    /// The whole width along z, flanges included.
    pub fn width(&self) -> f64 {
        self.toothed_width() + 2.0 * self.flange_thickness()
    }

    /// The default name: `GT2 Pulley 20T 6mm`.
    pub fn name(&self) -> String {
        let profile = match self.profile.id {
            "gt2" => "GT2",
            "htd3m" => "HTD 3M",
            _ => "HTD 5M",
        };
        format!("{profile} Pulley {}T {}mm", self.teeth, self.belt_width)
    }
}

/// Read and check `pulley.insert`'s parameters.
pub(crate) fn pulley_from(p: &Value, cmd: &str) -> Result<Pulley> {
    let id = str_(p, "profile").ok_or_else(|| bad(cmd, "`profile` is required: gt2, htd3m or htd5m"))?;
    let profile = profile(id).ok_or_else(|| bad(cmd, format!("unknown `profile` `{id}`: gt2, htd3m or htd5m")))?;
    let teeth = p
        .get("teeth")
        .and_then(Value::as_u64)
        .and_then(|t| u32::try_from(t).ok())
        .filter(|t| (TEETH.0..=TEETH.1).contains(t))
        .ok_or_else(|| bad(cmd, format!("`teeth` must be a whole number {}..{}", TEETH.0, TEETH.1)))?;
    let belt_width = match p.get("belt_width") {
        None | Some(Value::Null) => profile.belt_width,
        Some(_) => {
            num(p, "belt_width").filter(|w| *w >= 1.0 && *w <= MAX_BELT).ok_or_else(|| bad(cmd, format!("`belt_width` must be 1..{MAX_BELT} mm")))?
        }
    };
    let flanges = match p.get("flanges") {
        None | Some(Value::Null) => true,
        Some(_) => bool_(p, "flanges").ok_or_else(|| bad(cmd, "`flanges` must be true or false"))?,
    };
    let pulley = Pulley { profile, teeth, belt_width, flanges, bore: None };
    let offset = match p.get("bore_offset") {
        None | Some(Value::Null) => 0.0,
        Some(_) => num(p, "bore_offset").ok_or_else(|| bad(cmd, "`bore_offset` must be a number (mm)"))?,
    };
    let bore = match p.get("bore_diameter") {
        None | Some(Value::Null) => None,
        Some(_) => {
            let d = num(p, "bore_diameter").filter(|d| *d > 0.0).ok_or_else(|| bad(cmd, "`bore_diameter` must be more than 0 mm"))?;
            // The hole stays inside the grooves' bottoms with a wall left round it.
            let room = pulley.root_radius() - MIN_WALL;
            if offset.abs() + d / 2.0 > room {
                return Err(bad(
                    cmd,
                    format!(
                        "the {d} mm hole{} reaches the grooves: it must stay within {:.2} mm of the axis",
                        if offset != 0.0 { " so far off the axis" } else { "" },
                        room
                    ),
                ));
            }
            Some((d, offset))
        }
    };
    outline(&pulley).map_err(|e| bad(cmd, e))?;
    Ok(Pulley { bore, ..pulley })
}

/// One piece of the outline: an arc about `c` from `a` to `b`, counter-clockwise.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Arc {
    pub c: [f64; 2],
    pub a: [f64; 2],
    pub b: [f64; 2],
}

fn polar(r: f64, a: f64) -> [f64; 2] {
    [r * a.cos(), r * a.sin()]
}

/// The pulley's outline: the outside circle with a circular groove centred on each tooth gap,
/// groove k at the angle 2πk/z, as closed loop of arcs (groove, land, groove, …).
pub(crate) fn outline(pulley: &Pulley) -> std::result::Result<Vec<Arc>, String> {
    let (z, rg) = (pulley.teeth, pulley.profile.groove_radius);
    let ro = pulley.outside_radius();
    let rc = ro - pulley.profile.depth + rg;
    if pulley.root_radius() <= MIN_WALL {
        return Err(format!("{z} grooves are too few for this profile"));
    }
    // Where the groove's circle crosses the outside circle, seen from the axis: ±beta about its centre.
    let cos_b = (ro * ro + rc * rc - rg * rg) / (2.0 * ro * rc);
    if !(cos_b > -1.0 && cos_b < 1.0) {
        return Err("the groove does not cut the outside diameter".into());
    }
    let beta = cos_b.acos();
    let step = std::f64::consts::TAU / f64::from(z);
    if 2.0 * beta >= step {
        return Err(format!("{z} grooves are too few for this profile"));
    }
    let edges: Vec<([f64; 2], [f64; 2])> = (0..z)
        .map(|k| {
            let t = step * f64::from(k);
            (polar(ro, t - beta), polar(ro, t + beta))
        })
        .collect();
    let mut out = Vec::with_capacity(2 * edges.len());
    for (k, &(a, b)) in edges.iter().enumerate() {
        let c = polar(rc, step * k as f64);
        // The groove is the part of its circle inside the outside one: from b back to a,
        // counter-clockwise about its centre, passing the side toward the axis.
        out.push(Arc { c, a: b, b: a });
        let next = edges.get((k + 1) % edges.len()).map_or(a, |e| e.0);
        out.push(Arc { c: [0.0, 0.0], a: b, b: next });
    }
    Ok(out)
}

fn run(s: &mut Session, id: &str, p: Value) -> Result<Value> {
    s.execute(id, &p)
}

/// Fix every point and curve of the active sketch, so it is fully defined.
fn fix_all(s: &mut Session) -> Result<()> {
    let sketch = super::sketch_store::active(s).and_then(|id| super::sketch_store::read(&s.doc, id).ok());
    let entities: Vec<String> =
        sketch.map(|sk| sk.points.iter().map(|p| p.id.clone()).chain(sk.curves.iter().map(|c| c.id.clone())).collect()).unwrap_or_default();
    if !entities.is_empty() {
        run(s, "sketch.constraint.fix", json!({"entities": entities, "fixed": true}))?;
    }
    Ok(())
}

/// Sketch the outline on XY at height `z0`. The arcs go in as one edit of the sketch: drawn one
/// command at a time, each would solve the whole sketch again.
fn sketch_outline(s: &mut Session, arcs: &[Arc], z0: f64, cmd: &str) -> Result<()> {
    use solidcore_record::Vec2;
    use solidcraft_sketch::CurveKind;
    run(s, "sketch.create", json!({"plane": "XY", "offset": format!("{z0} mm")}))?;
    super::sketch::edit(s, &json!({}), cmd, false, |sk, _| {
        // Arcs that meet share their end point (each ends exactly where the next begins).
        let mut ends: std::collections::HashMap<(u64, u64), usize> = std::collections::HashMap::new();
        let mut end = |sk: &mut solidcraft_sketch::Sketch, p: [f64; 2]| -> Result<usize> {
            if let Some(i) = ends.get(&(p[0].to_bits(), p[1].to_bits())) {
                return Ok(*i);
            }
            let i = sk.add_point(Vec2::new(p[0], p[1]), None)?;
            ends.insert((p[0].to_bits(), p[1].to_bits()), i);
            Ok(i)
        };
        for g in arcs {
            let c = sk.add_point(Vec2::new(g.c[0], g.c[1]), None)?;
            let kind = CurveKind::Arc { c, a: end(sk, g.a)?, b: end(sk, g.b)? };
            sk.add_curve(kind, None)?;
        }
        Ok(())
    })?;
    s.refresh();
    fix_all(s)?;
    run(s, "sketch.finish", json!({}))?;
    Ok(())
}

/// A cylinder about the axis (moved `x` along +x) from z0 to z1, joined or cut.
fn cylinder(s: &mut Session, d: f64, x: f64, z0: f64, z1: f64, op: &str) -> Result<()> {
    run(
        s,
        "solid.cylinder",
        json!({"diameter": format!("{d} mm"), "height": format!("{} mm", z1 - z0), "base": [x, 0, z0], "axis": [0, 0, 1], "operation": op}),
    )?;
    Ok(())
}

/// Build the pulley in the active component.
pub(crate) fn build(s: &mut Session, pulley: &Pulley, cmd: &str) -> Result<()> {
    let _frame = crate::frames::local_geometry();
    let arcs = outline(pulley).map_err(EngineError::Other)?;
    let (f, t) = (pulley.flange_thickness(), pulley.toothed_width());
    sketch_outline(s, &arcs, f, cmd)?;
    run(s, "solid.extrude", json!({"distance": format!("{t} mm"), "direction": "positive", "operation": "new", "profiles": "all"}))?;
    if pulley.flanges {
        let df = pulley.flange_diameter();
        cylinder(s, df, 0.0, 0.0, f, "join")?;
        cylinder(s, df, 0.0, f + t, 2.0 * f + t, "join")?;
    }
    if let Some((d, x)) = pulley.bore {
        cylinder(s, d, x, -1.0, pulley.width() + 1.0, "cut")?;
    }
    Ok(())
}

/// `pulley.insert` as one step: built in a scratch session, kept only whole.
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
    let cmd = "pulley.insert (Insert Pulley)";
    let pulley = pulley_from(p, cmd)?;
    let name = str_(p, "name").map(str::trim).filter(|n| !n.is_empty()).map_or_else(|| pulley.name(), str::to_string);
    let active = s.active_component;
    // A pulley is fitted to a shaft: it starts free even as the design's first part.
    let c = run(s, "component.create", json!({"name": name, "grounded": false}))?;
    let component = c["component"].as_u64().ok_or_else(|| bad(cmd, "the component has no id"))?;
    let occurrence = c["occurrence"].as_u64().ok_or_else(|| bad(cmd, "the component has no occurrence"))?;
    build(s, &pulley, cmd)?;
    run(s, "component.activate", json!({"component": active}))?;
    let out = InsertedPulley {
        component,
        occurrence,
        name,
        pitch_diameter: pulley.pitch_diameter(),
        outside_diameter: 2.0 * pulley.outside_radius(),
        width: pulley.width(),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(out).map_err(|e| bad(cmd, e.to_string()))
}
