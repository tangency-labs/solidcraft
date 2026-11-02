use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use super::CommandSpec;
use crate::params::{bad, bool_, num, str_, vec3};
use crate::{Result, Session};

const MIN_TEETH: u32 = 8;
const MAX_TEETH: u32 = 300;
const MAX_HELICAL_TEETH: u32 = 24;
const MAX_MODULE: f64 = 100.0;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("gears.library", "Gear Library", library).icon("library").noundo().params("type? → supported gear types and parameters"),
    CommandSpec::new("gears.insert", "Insert Spur Gear", insert)
        .at("INSERT", "LIBRARY")
        .icon("gear")
        .params("module, teeth, height? (or face_width), bore?, pressure_angle?, backlash?, reversed_backlash?, clearance?, head_tolerance?, profile_shift?, num_points?, simple?, root_fillet_radius?, head_fillet_radius?, helix_angle? (90° is straight), double_helix?, axle_hole?, axle_hole_size?, offset_hole?, offset_hole_offset?, offset_hole_size?, point?, name?"),
    CommandSpec::new("gears.resize", "Edit Gear Parameters", resize_atomic)
        .icon("scale")
        .params("component: id; editable gear parameters as gears.insert"),
];

#[derive(Clone, Debug)]
struct Gear {
    module: f64,
    teeth: u32,
    face_width: f64,
    bore: f64,
    pressure_angle: f64,
    backlash: f64,
    root_fillet_radius: f64,
    head_fillet_radius: f64,
    helix_angle: f64,
    num_points: u32,
    simple: bool,
    double_helix: bool,
    axle_hole: bool,
    internal_gear: bool,
    rim_thickness: f64,
    offset_hole: bool,
    offset_hole_offset: f64,
    offset_hole_size: f64,
    profile_shift: f64,
    clearance: f64,
    head_tolerance: f64,
    reversed_backlash: bool,
}

impl Gear {
    fn new(
        module: f64,
        teeth: u32,
        face_width: f64,
        bore: f64,
        pressure_angle: f64,
        backlash: f64,
        root_fillet_radius: f64,
        helix_angle: f64,
    ) -> Self {
        Self {
            module,
            teeth,
            face_width,
            bore,
            pressure_angle,
            backlash,
            root_fillet_radius,
            head_fillet_radius: 0.0,
            helix_angle,
            num_points: 20,
            simple: false,
            double_helix: false,
            axle_hole: true,
            internal_gear: false,
            rim_thickness: 10.0,
            offset_hole: false,
            offset_hole_offset: 10.0,
            offset_hole_size: 10.0,
            profile_shift: 0.0,
            clearance: 0.25,
            head_tolerance: 0.0,
            reversed_backlash: false,
        }
    }

    fn pitch_radius(&self) -> f64 {
        self.module * self.teeth as f64 / 2.0
    }

    fn outside_radius(&self) -> f64 {
        if self.internal_gear {
            self.pitch_radius() + self.module * (1.0 + self.profile_shift) + self.head_tolerance
        } else {
            self.pitch_radius() + self.module * (1.0 + self.profile_shift) - self.head_tolerance
        }
    }

    fn root_radius(&self) -> f64 {
        if self.internal_gear {
            self.pitch_radius() - self.module * (1.0 - self.clearance + self.profile_shift)
        } else {
            self.pitch_radius() - self.module * (1.0 + self.clearance - self.profile_shift)
        }
    }

    fn base_radius(&self) -> f64 {
        self.pitch_radius() * self.pressure_angle.to_radians().cos()
    }

    fn max_root_fillet_radius(&self) -> f64 {
        let root = self.root_radius();
        let base = self.base_radius();
        if base <= root { 0.0 } else { ((base * base - root * root) / (2.0 * root)).min(self.module / 2.0) }
    }

    fn twist(&self) -> f64 {
        if self.helix_angle == 90.0 {
            return 0.0;
        }
        let pitch_radius = self.pitch_radius();
        if !(pitch_radius.is_finite() && pitch_radius > 0.0) {
            return f64::INFINITY;
        }
        self.face_width / pitch_radius * (90.0 - self.helix_angle).to_radians().tan()
    }

    fn effective_backlash(&self) -> f64 {
        let flip_for_internal_gear = if self.internal_gear { -1.00 } else { 1.00 };
        if self.reversed_backlash { self.backlash * flip_for_internal_gear } else { -self.backlash * flip_for_internal_gear }
    }

    fn involute_segments(&self) -> usize {
        let points = self.num_points as usize;
        if self.simple { (points / 4).max(4) } else { points }
    }

    fn arc_segments(&self) -> usize {
        (self.involute_segments() / 3).max(4)
    }

    fn validate(&self) -> std::result::Result<(), String> {
        if !self.module.is_finite() || !(0.1..=MAX_MODULE).contains(&self.module) {
            return Err("module must be between 0.1 and 100 mm".into());
        }

        if !(MIN_TEETH..=MAX_TEETH).contains(&self.teeth) {
            return Err(format!("teeth must be between {MIN_TEETH} and {MAX_TEETH}"));
        }

        if !(self.root_radius().is_finite() && self.root_radius() > 0.0 && self.outside_radius() > self.root_radius()) {
            return Err("profile shift, clearance, and head tolerance produce an invalid gear profile".into());
        }

        if self.helix_angle < 90.0 && self.teeth > MAX_HELICAL_TEETH {
            return Err(format!("helical gears are limited to {MAX_HELICAL_TEETH} teeth to keep the twisted profile responsive"));
        }

        if !self.face_width.is_finite() || self.face_width <= 0.0 || self.face_width > 1000.0 {
            return Err("face_width must be > 0 and <= 1000 mm".into());
        }

        if !self.bore.is_finite() || self.bore < 0.0 || (self.axle_hole && self.bore >= 2.0 * self.root_radius()) {
            return Err("axle_hole_size must be >= 0 and smaller than the root diameter".into());
        }

        if !(4..=24).contains(&self.num_points) {
            return Err("num_points must be between 4 and 24".into());
        }

        if !self.profile_shift.is_finite() || !(-1.0..=1.0).contains(&self.profile_shift) {
            return Err("profile_shift must be between -1 and 1 modules".into());
        }

        if !self.clearance.is_finite() || !(0.0..=1.0).contains(&self.clearance) {
            return Err("clearance must be between 0 and 1 module".into());
        }

        if !self.head_tolerance.is_finite() || !(0.0..=self.module).contains(&self.head_tolerance) {
            return Err("head tolerance must be between 0 and one module".into());
        }

        if !self.head_fillet_radius.is_finite() || !(0.0..=self.module / 2.0).contains(&self.head_fillet_radius) {
            return Err("head_fillet_radius must be between 0 and half a module".into());
        }

        if !self.offset_hole_offset.is_finite() || !self.offset_hole_size.is_finite() || self.offset_hole_offset < 0.0 || self.offset_hole_size <= 0.0
        {
            return Err("offset hole offset and size must be finite; size must be positive".into());
        }

        if self.offset_hole {
            let radius = self.offset_hole_size / 2.0;
            if self.offset_hole_offset + radius >= self.root_radius() {
                return Err("the offset hole must fit inside the gear root diameter".into());
            }
            if self.axle_hole && self.offset_hole_offset < radius + self.bore / 2.0 {
                return Err("the offset hole must not overlap the axle hole".into());
            }
        }

        if !self.pressure_angle.is_finite() || !(14.0..=35.0).contains(&self.pressure_angle) {
            return Err("pressure_angle must be between 14 and 35 degrees".into());
        }

        if !self.backlash.is_finite() || self.backlash.abs() >= std::f64::consts::PI * self.module / 2.0 {
            return Err("absolute backlash must be less than half the circular pitch".into());
        }

        if !self.root_fillet_radius.is_finite() || self.root_fillet_radius < 0.0 || self.root_fillet_radius > self.max_root_fillet_radius() {
            return Err(format!("root_fillet_radius must be between 0 and {} mm for these gear parameters", self.max_root_fillet_radius()));
        }

        if !self.helix_angle.is_finite() || !(0.0 < self.helix_angle && self.helix_angle <= 90.0) {
            return Err("helix_angle must be greater than 0 and at most 90 degrees (90 degrees is a straight gear)".into());
        }

        if self.double_helix && self.helix_angle == 90.0 {
            return Err("double_helix requires a helical angle below 90 degrees".into());
        }

        if self.twist().abs() > solidcraft_kernel::SweepControl::MAX_TWIST {
            return Err("helix_angle and gear height produce more than four turns; increase the angle or reduce the height".into());
        }

        if !self.rim_thickness.is_finite() || self.rim_thickness < 0.0 || self.rim_thickness > 1000.0 {
            return Err("rim_thickness must be between 0 and 1000 mm".into());
        }

        if self.internal_gear {
            if self.rim_thickness <= 0.0 {
                return Err("internal gears require a positive rim thickness".into());
            }

            if self.axle_hole || self.offset_hole {
                return Err("internal gears cannot have axle or offset holes".into());
            }
        }
        Ok(())
    }

    fn encode(&self) -> String {
        format!(
            "m={};z={};b={};pa={};bl={};fr={};ha={};hf={};np={};sm={};dh={};ah={};oh={};oho={};ohs={};ps={};cl={};ht={};rb={};ig={};rh={}",
            self.module,
            self.teeth,
            self.bore,
            self.pressure_angle,
            self.backlash,
            self.root_fillet_radius,
            self.helix_angle,
            self.head_fillet_radius,
            self.num_points,
            self.simple,
            self.double_helix,
            self.axle_hole,
            self.offset_hole,
            self.offset_hole_offset,
            self.offset_hole_size,
            self.profile_shift,
            self.clearance,
            self.head_tolerance,
            self.reversed_backlash,
            self.internal_gear,
            self.rim_thickness,
        )
    }

    fn decode(s: &str, face_width: f64) -> Option<Self> {
        let mut module = None;
        let mut teeth = None;
        let mut bore = None;
        let mut pressure_angle = None;
        let mut backlash = None;
        let mut root_fillet_radius = None;
        let mut helix_angle = None;
        let mut extra = std::collections::BTreeMap::new();

        for field in s.split(';') {
            let (key, value) = field.split_once('=')?;
            match key {
                "m" => module = value.parse::<f64>().ok(),
                "z" => teeth = value.parse::<u32>().ok(),
                "b" => bore = value.parse::<f64>().ok(),
                "pa" => pressure_angle = value.parse::<f64>().ok(),
                "bl" => backlash = value.parse::<f64>().ok(),
                "fr" => root_fillet_radius = value.parse::<f64>().ok(),
                "ha" => helix_angle = value.parse::<f64>().ok(),
                "hf" | "np" | "sm" | "uc" | "dh" | "ah" | "oh" | "oho" | "ohs" | "ps" | "cl" | "ht" | "rb" | "ig" | "rh" => {
                    extra.insert(key, value);
                }
                _ => return None,
            }
        }

        let mut gear = Self::new(
            module?,
            teeth?,
            face_width,
            bore?,
            pressure_angle?,
            backlash.unwrap_or(0.0),
            root_fillet_radius.unwrap_or(0.0),
            helix_angle.unwrap_or(90.0),
        );
        gear.head_fillet_radius = extra.get("hf").and_then(|value| value.parse().ok()).unwrap_or(0.0);
        gear.num_points = extra.get("np").and_then(|value| value.parse().ok()).unwrap_or(20);
        gear.simple = extra.get("sm").is_some_and(|value| *value == "true");
        gear.double_helix = extra.get("dh").is_some_and(|value| *value == "true");
        gear.axle_hole = extra.get("ah").map_or(true, |value| *value == "true");
        gear.offset_hole = extra.get("oh").is_some_and(|value| *value == "true");
        gear.offset_hole_offset = extra.get("oho").and_then(|value| value.parse().ok()).unwrap_or(10.0);
        gear.offset_hole_size = extra.get("ohs").and_then(|value| value.parse().ok()).unwrap_or(10.0);
        gear.profile_shift = extra.get("ps").and_then(|value| value.parse().ok()).unwrap_or(0.0);
        gear.clearance = extra.get("cl").and_then(|value| value.parse().ok()).unwrap_or(0.25);
        gear.head_tolerance = extra.get("ht").and_then(|value| value.parse().ok()).unwrap_or(0.0);
        gear.reversed_backlash = extra.get("rb").is_some_and(|value| *value == "true");

        gear.internal_gear = extra.get("ig").is_some_and(|value| *value == "true");

        gear.rim_thickness = extra.get("rh").and_then(|value| value.parse::<f64>().ok()).unwrap_or(10.0);

        if gear.internal_gear {
            // Internal gears do not support axle or offset holes.
            gear.axle_hole = false;
            gear.offset_hole = false;
        }
        Some(gear)
    }
}

fn library(_s: &mut Session, p: &Value) -> Result<Value> {
    let kind = str_(p, "type").unwrap_or("spur");

    if !kind.eq_ignore_ascii_case("spur") {
        return Err(bad("gears.library", "only external spur gears are implemented"));
    }

    Ok(json!({
        "types": ["spur"],
        "units": "mm",
        "defaults": {
            "module": 2.0,
            "teeth": 24,
            "face_width": 10.0,
            "height": 10.0,
            "bore": 8.0,
            "pressure_angle": 20.0,
            "backlash": 0.0,
            "clearance": 0.25,
            "root_fillet_radius": 0.0,
            "head_fillet_radius": 0.0,
            "helix_angle": 90.0,
            "num_points": 20,
            "simple": false,
            "double_helix": false,
            "axle_hole": true,
            "axle_hole_size": 8.0,
            "rim_thickness":10.0,
            "internal_gear":false,
            "offset_hole": false,
            "offset_hole_offset": 10.0,
            "offset_hole_size": 10.0,
            "profile_shift": 0.0,
            "head_tolerance": 0.0,
            "reversed_backlash": false
        },
        "limits": {
            "module": [0.1, MAX_MODULE],
            "teeth": [MIN_TEETH, MAX_TEETH],
            "helical_teeth_max": MAX_HELICAL_TEETH,
            "face_width": [0.1, 1000.0],
            "height": [0.1, 1000.0],
            "pressure_angle": [14.0, 35.0],
            "backlash": ["greater than negative half the circular pitch", "less than half the circular pitch"],
            "root_fillet_radius": [0.0, "gear-dependent maximum"],
            "helix_angle": ["greater than 0 and at most 90 degrees", "90 degrees is straight"]
        },
        "geometry": {
            "pitch_diameter": "module * teeth",
            "outside_diameter": "2 * (pitch_radius + module * (1 + profile_shift) - head_tolerance)",
            "root_diameter": "2 * (pitch_radius - module * (1 + clearance - profile_shift))",
            "base_diameter": "pitch_diameter * cos(pressure_angle)",
            "transverse_module": "module / sin(helix_angle)",
            "transverse_pitch": "pi * transverse_module",
            "angular_backlash_degrees": "effective_backlash / pitch_radius, converted to degrees",
            "helix_convention": "90 degrees is straight; lower positive angles are helical",
            "axial_twist": "height / pitch_radius * tan(90 degrees - helix_angle)",
            "properties_from_tool": "not offered; reported properties are calculated from gear parameters"
        }
    }))
}

fn read_gear(p: &Value, previous: Option<&Gear>) -> Result<Gear> {
    if p.get("undercut").is_some() {
        return Err(bad("gears", "`undercut` is not implemented"));
    }
    let get = |key: &str, default: f64| num(p, key).unwrap_or(default);
    let get_alias = |key: &str, alias: &str, default: f64| num(p, key).or_else(|| num(p, alias)).unwrap_or(default);
    let flag = |key: &str, default: bool| -> Result<bool> {
        if p.get(key).is_some() { bool_(p, key).ok_or_else(|| bad("gears", format!("`{key}` must be a boolean"))) } else { Ok(default) }
    };

    let module = get("module", previous.map_or(2.0, |g| g.module));

    let teeth_value = num(p, "teeth").unwrap_or(previous.map_or(24.0, |g| g.teeth as f64));

    if !teeth_value.is_finite() || teeth_value.fract() != 0.0 || teeth_value < 0.0 || teeth_value > u32::MAX as f64 {
        return Err(bad("gears", "teeth must be a positive integer"));
    }

    let face_width = if p.get("height").is_some() {
        get("height", previous.map_or(10.0, |g| g.face_width))
    } else {
        get("face_width", previous.map_or(10.0, |g| g.face_width))
    };

    let bore = get_alias("axle_hole_size", "bore", previous.map_or(8.0, |g| g.bore));

    let pressure_angle = get("pressure_angle", previous.map_or(20.0, |g| g.pressure_angle));

    let backlash = get("backlash", previous.map_or(0.0, |g| g.backlash));

    let root_fillet_radius = get_alias("root_fillet_radius", "root_fillet", previous.map_or(0.0, |g| g.root_fillet_radius));
    let head_fillet_radius = get_alias("head_fillet_radius", "head_fillet", previous.map_or(0.0, |g| g.head_fillet_radius));

    let helix_angle = get("helix_angle", previous.map_or(90.0, |g| g.helix_angle));

    let mut gear = Gear::new(module, teeth_value as u32, face_width, bore, pressure_angle, backlash, root_fillet_radius, helix_angle);
    gear.head_fillet_radius = head_fillet_radius;
    let points = get("num_points", previous.map_or(20.0, |g| g.num_points as f64));
    if !points.is_finite() || points.fract() != 0.0 || points < 0.0 || points > u32::MAX as f64 {
        return Err(bad("gears", "`num_points` must be a positive integer"));
    }
    gear.num_points = points as u32;
    gear.simple = flag("simple", previous.is_some_and(|g| g.simple))?;
    gear.double_helix = flag("double_helix", previous.is_some_and(|g| g.double_helix))?;
    let default_axle_hole = previous.map_or(true, |g| g.axle_hole);
    gear.axle_hole = flag("axle_hole", if p.get("bore").is_some() && p.get("axle_hole_size").is_none() { bore > 0.0 } else { default_axle_hole })?;
    gear.offset_hole = flag("offset_hole", previous.is_some_and(|g| g.offset_hole))?;
    gear.offset_hole_offset = get("offset_hole_offset", previous.map_or(10.0, |g| g.offset_hole_offset));
    gear.offset_hole_size = get("offset_hole_size", previous.map_or(10.0, |g| g.offset_hole_size));
    gear.profile_shift = get_alias("profile_shift", "shift", previous.map_or(0.0, |g| g.profile_shift));
    gear.clearance = get("clearance", previous.map_or(0.25, |g| g.clearance));
    gear.head_tolerance = get_alias("head_tolerance", "head", previous.map_or(0.0, |g| g.head_tolerance));
    gear.reversed_backlash = flag("reversed_backlash", previous.is_some_and(|g| g.reversed_backlash))?;

    // Internal gear settings: preserve existing values when resizing.
    gear.internal_gear = flag("internal_gear", previous.is_some_and(|g| g.internal_gear))?;

    gear.rim_thickness = get("rim_thickness", previous.map_or(0.0, |g| g.rim_thickness));

    // Internal gears do not support axle or offset holes.
    if gear.internal_gear {
        gear.axle_hole = false;
        gear.offset_hole = false;
    }

    gear.validate().map_err(|e| bad("gears", e))?;
    Ok(gear)
}

fn involute_point(base_r: f64, t: f64) -> [f64; 2] {
    [base_r * (t.cos() + t * t.sin()), base_r * (t.sin() - t * t.cos())]
}

fn rotate_point(p: [f64; 2], a: f64) -> [f64; 2] {
    let (s, c) = a.sin_cos();
    [p[0] * c - p[1] * s, p[0] * s + p[1] * c]
}

fn involute_t(base_r: f64, r: f64) -> f64 {
    if r <= base_r { 0.0 } else { ((r / base_r).powi(2) - 1.0).sqrt() }
}
/// Return a positive angular difference in [0, 2*PI).
fn positive_angle_delta(from: f64, to: f64) -> f64 {
    use std::f64::consts::TAU;

    (to - from).rem_euclid(TAU)
}

fn run(s: &mut Session, id: &str, p: Value) -> Result<Value> {
    s.execute(id, &p)
}

use std::f64::consts::{PI, TAU};

#[derive(Clone, Debug)]
enum Seg {
    Line([f64; 2], [f64; 2]),
    Arc { start: [f64; 2], mid: [f64; 2], end: [f64; 2], center: [f64; 2] },
    Spline(Vec<[f64; 2]>), // fit points on the involute
}

impl Seg {
    fn start(&self) -> [f64; 2] {
        match self {
            Seg::Line(a, _) => *a,
            Seg::Arc { start, .. } => *start,
            Seg::Spline(p) => p[0],
        }
    }
    fn end(&self) -> [f64; 2] {
        match self {
            Seg::Line(_, b) => *b,
            Seg::Arc { end, .. } => *end,
            Seg::Spline(p) => p[p.len() - 1],
        }
    }
    fn mirrored_reversed(&self) -> Seg {
        let m = |p: [f64; 2]| [p[0], -p[1]];
        match self {
            Seg::Line(a, b) => Seg::Line(m(*b), m(*a)),
            Seg::Arc { start, mid, end, center } => Seg::Arc { start: m(*end), mid: m(*mid), end: m(*start), center: m(*center) },
            Seg::Spline(p) => Seg::Spline(p.iter().rev().map(|q| m(*q)).collect()),
        }
    }
    fn rotated(&self, a: f64) -> Seg {
        let r = |p: [f64; 2]| rotate_point(p, a);
        match self {
            Seg::Line(s, e) => Seg::Line(r(*s), r(*e)),
            Seg::Arc { start, mid, end, center } => Seg::Arc { start: r(*start), mid: r(*mid), end: r(*end), center: r(*center) },
            Seg::Spline(p) => Seg::Spline(p.iter().map(|q| r(*q)).collect()),
        }
    }
}

/// Fillet-sized arc (< 180°) between two points on a circle.
fn arc_between(c: [f64; 2], r: f64, from: [f64; 2], to: [f64; 2]) -> Seg {
    let a0 = (from[1] - c[1]).atan2(from[0] - c[0]);
    let a1 = (to[1] - c[1]).atan2(to[0] - c[0]);
    let sweep = (a1 - a0 + PI).rem_euclid(TAU) - PI;
    let a = a0 + sweep / 2.0;
    Seg::Arc { start: from, mid: [c[0] + r * a.cos(), c[1] + r * a.sin()], end: to, center: c }
}

/// CCW arc about the origin (tip circle, root circle).
fn arc_ccw_about_origin(r: f64, from: [f64; 2], to: [f64; 2]) -> Seg {
    let a0 = from[1].atan2(from[0]);
    let a1 = to[1].atan2(to[0]);
    let a = a0 + positive_angle_delta(a0, a1) / 2.0;
    Seg::Arc { start: from, mid: [r * a.cos(), r * a.sin()], end: to, center: [0.0, 0.0] }
}

/// One tooth centered on +X, CCW, starting on the root circle at the lower flank.
fn tooth_segments(g: &Gear) -> Result<Vec<Seg>> {
    let rb = g.base_radius();
    let ra = g.outside_radius();
    let rf = g.root_radius();
    let (rr, rh) = (g.root_fillet_radius, g.head_fillet_radius);
    let alpha = g.pressure_angle.to_radians();
    let phi = PI / (2.0 * g.teeth as f64) + alpha.tan() - alpha
        + (g.effective_backlash() / 2.0 + g.profile_shift * g.module * alpha.tan()) / g.pitch_radius();
    let ta = involute_t(rb, ra);

    let flank = |t: f64| rotate_point(involute_point(rb, t), -phi);
    // fillet center: d > 0 inside the material, d < 0 in the gap
    let center = |t: f64, d: f64| {
        let p = involute_point(rb, t);
        rotate_point([p[0] - d * t.sin(), p[1] + d * t.cos()], -phi)
    };

    let mut left: Vec<Seg> = Vec::new();
    let t_lo;

    if rf >= rb {
        if rr > 0.0 {
            let tc = (((rf + rr).powi(2) - rb * rb).sqrt() - rr) / rb;
            let c = center(tc, -rr);
            let k = rf / (rf + rr);
            left.push(arc_between(c, rr, [c[0] * k, c[1] * k], flank(tc)));
            t_lo = tc;
        } else {
            t_lo = involute_t(rb, rf);
        }
    } else {
        // below the base circle the flank is a radial line
        let (s, c) = (-phi).sin_cos();
        let foot = [rb * c, rb * s];
        if rr > 0.0 {
            let cd = rf + rr;
            let td = (cd * cd - rr * rr).sqrt();
            if td > rb {
                return Err(bad("gears", "root_fillet_radius is too large for this gear"));
            }
            let ctr = [td * c + rr * s, td * s - rr * c];
            let k = rf / cd;
            let tan_flank = [td * c, td * s];
            left.push(arc_between(ctr, rr, [ctr[0] * k, ctr[1] * k], tan_flank));
            left.push(Seg::Line(tan_flank, foot));
        } else {
            left.push(Seg::Line([rf * c, rf * s], foot));
        }
        t_lo = 0.0;
    }

    let mut head = None;
    let t_hi;
    if rh > 0.0 {
        let disc = (ra - rh).powi(2) - rb * rb;
        if disc < 0.0 {
            return Err(bad("gears", "head_fillet_radius is too large for this gear"));
        }
        let tc = (rh + disc.sqrt()) / rb;
        if tc <= t_lo + 1e-9 || tc >= ta {
            return Err(bad("gears", "head_fillet_radius is too large for the tooth flank"));
        }
        let c = center(tc, rh);
        let k = ra / (ra - rh);
        head = Some(arc_between(c, rh, flank(tc), [c[0] * k, c[1] * k]));
        t_hi = tc;
    } else {
        t_hi = ta;
    }

    let n = g.involute_segments().max(4);
    left.push(Seg::Spline((0..=n).map(|i| flank(t_lo + (t_hi - t_lo) * i as f64 / n as f64)).collect()));

    let left_tip = match head {
        Some(h) => {
            let e = h.end();
            left.push(h);
            e
        }
        None => flank(ta),
    };
    if left_tip[1] >= 0.0 {
        return Err(bad("gears", "tooth tip has no width; reduce fillets, profile_shift, or backlash"));
    }

    let mirrored: Vec<Seg> = left.iter().rev().map(Seg::mirrored_reversed).collect();
    left.push(arc_ccw_about_origin(ra, left_tip, [left_tip[0], -left_tip[1]]));
    left.extend(mirrored);
    Ok(left)
}

fn gear_profile(g: &Gear) -> Result<Vec<Seg>> {
    let tooth = tooth_segments(g)?;
    let (s0, e0) = (tooth[0].start(), tooth[tooth.len() - 1].end());
    let pitch = TAU / g.teeth as f64;
    let rf = g.root_radius();

    let gap = positive_angle_delta(e0[1].atan2(e0[0]), s0[1].atan2(s0[0]) + pitch);
    if gap > pitch {
        return Err(bad("gears", "teeth overlap at the root; reduce backlash/profile_shift/fillets"));
    }

    let mut out = Vec::with_capacity(g.teeth as usize * (tooth.len() + 1));
    for k in 0..g.teeth {
        let rot = pitch * k as f64;
        out.extend(tooth.iter().map(|s| s.rotated(rot)));
        let a = rotate_point(e0, rot);
        // reuse the exact start point on the wrap-around so the loop closes bit-for-bit
        let b = if k + 1 == g.teeth { s0 } else { rotate_point(s0, pitch * (k + 1) as f64) };
        if (a[0] - b[0]).hypot(a[1] - b[1]) > 1e-9 {
            out.push(arc_ccw_about_origin(rf, a, b));
        }
    }
    Ok(out)
}
fn flatten(segs: &[Seg], arc_steps: usize) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for s in segs {
        match s {
            Seg::Line(a, _) => out.push(*a),
            Seg::Arc { start, mid, center, .. } => {
                let r = (start[0] - center[0]).hypot(start[1] - center[1]);
                let a0 = (start[1] - center[1]).atan2(start[0] - center[0]);
                let am = (mid[1] - center[1]).atan2(mid[0] - center[0]);
                let sweep = 2.0 * ((am - a0 + PI).rem_euclid(TAU) - PI);
                for i in 0..arc_steps {
                    let a = a0 + sweep * i as f64 / arc_steps as f64;
                    out.push([center[0] + r * a.cos(), center[1] + r * a.sin()]);
                }
            }
            Seg::Spline(p) => out.extend_from_slice(&p[..p.len() - 1]),
        }
    }
    out
}

fn gear_outline(g: &Gear) -> Result<Vec<[f64; 2]>> {
    Ok(flatten(&gear_profile(g)?, g.arc_segments()))
}

/// Adds the non-tooth loops (rim circle for internal gears; bore/offset holes for external ones).
fn add_profile_loops(s: &mut Session, g: &Gear, name: &str) -> Result<()> {
    run(s, "sketch.edit", json!({"sketch": name}))?;
    if g.internal_gear {
        run(
            s,
            "sketch.circle.center",
            json!({"center": [0.0, 0.0], "radius": g.outside_radius() + g.rim_thickness,
             "ids": ["internal_gear_profile"]
            }),
        )?;
    } else {
        if g.axle_hole && g.bore > 0.0 {
            run(s, "sketch.circle.center", json!({"center": [0.0, 0.0], "radius": g.bore / 2.0}))?;
        }
        if g.offset_hole {
            run(s, "sketch.circle.center", json!({"center": [g.offset_hole_offset, 0.0], "radius": g.offset_hole_size / 2.0}))?;
        }
    }
    run(s, "sketch.finish", json!({})).map(|_| ())
}

fn sector_sketch(s: &mut Session, g: &Gear, start: f64, start_rotation: f64, name: &str) -> Result<String> {
    let (sin, cos) = start_rotation.sin_cos();
    run(
        s,
        "sketch.create",
        json!({
            "plane": { "origin": [0, 0, start], "x_dir": [cos, sin, 0], "y_dir": [-sin, cos, 0] },
            "name": name
        }),
    )?;

    let outline = gear_outline(g)?;
    if outline.is_empty() {
        return Err(bad("gears", "the gear tooth profile is empty"));
    }
    let points: Vec<Value> = outline.iter().map(|p| json!([p[0], p[1]])).collect();
    run(s, "sketch.line", json!({"points": points, "closed": true, "ids": ["gear_profile"]}))?;

    add_profile_loops(s, g, name)?; // the only place loops are added
    Ok(name.to_string())
}
fn internal_rim_point(g: &Gear) -> [f64; 2] {
    [g.outside_radius() + g.rim_thickness / 2.0, 0.0]
}
fn helical_sweep(
    s: &mut Session,
    g: &Gear,
    start: f64,
    end: f64,
    start_rotation: f64,
    twist: f64,
    suffix: &str,
    operation: &str,
    body_name: &str,
) -> Result<()> {
    let profile_name = sector_sketch(s, g, start, start_rotation, &format!("Gear profile {suffix} {body_name}"))?;
    let path_name = format!("Gear path {suffix} {body_name}");
    let path_id = format!("gear_path_{suffix}");

    run(s, "sketch.create", json!({"plane": "XZ", "offset": "0 mm", "name": path_name}))?;
    run(s, "sketch.line", json!({"points": [[0, -start], [0, -end]], "closed": false, "ids": [path_id]}))?;
    run(s, "sketch.finish", json!({}))?;

    let mut params = json!({
        "sketch": profile_name,
        "path_sketch": path_name,
        "path": [path_id],
        "twist": format!("{twist} rad"),
        "operation": operation,
        "name": format!("Helical gear {suffix}"),
        "body_name": body_name,

    });
    if g.internal_gear {
        params["profiles"] = json!([{"point": internal_rim_point(g)}])
    }
    eprintln!("{}", params);
    run(s, "solid.sweep", params).map_err(|e| bad("gears", format!("helical gear sweep failed: {e}")))?;
    Ok(())
}
fn unique_gear_body_name(s: &Session, component: u64) -> Result<String> {
    let state = s.model.state();
    if state.body("Gear").is_none() {
        return Ok("Gear".to_string());
    }

    let base = format!("Gear ({component})");
    if state.body(&base).is_none() {
        return Ok(base);
    }

    let mut suffix = 2_u64;
    loop {
        let name = format!("{base} ({suffix})");
        if state.body(&name).is_none() {
            return Ok(name);
        }
        suffix = suffix.checked_add(1).ok_or_else(|| bad("gears", "could not find a unique gear body name"))?;
    }
}

/// Build a gear at its local origin, with the bottom face at z = 0.
fn build(s: &mut Session, g: &Gear, body_name: &str) -> Result<()> {
    let _frame = crate::frames::local_geometry();

    if g.helix_angle == 90.0 {
        // sector_sketch draws the tooth outline plus the rim/bore/offset loops and finishes the sketch.
        let profile_name = sector_sketch(s, g, 0.0, 0.0, &format!("Gear tooth profile {body_name}"))?;

        run(
            s,
            "solid.extrude",
            json!({
                "sketch": profile_name,
                "distance": format!("{} mm", g.face_width),
                "direction": "positive",
                "operation": "new",
                "name": "Gear",
                "body_name": body_name
            }),
        )?;
    } else if g.double_helix {
        let mid = g.face_width / 2.0;
        let half_twist = g.twist() / 2.0;
        helical_sweep(s, g, 0.0, mid, 0.0, half_twist, "first", "new", body_name)?;
        run(
            s,
            "solid.mirror",
            json!({
                "bodies": [body_name],
                "plane": {"origin": [0, 0, mid], "normal": [0, 0, 1]},
                "combine": true,
                "name": "Join double-helix halves"
            }),
        )
        .map_err(|error| bad("gears", format!("mirroring the double-helix half failed: {error}")))?;
    } else {
        helical_sweep(s, g, 0.0, g.face_width, 0.0, g.twist(), "single", "new", body_name)?;
    }

    Ok(())
}

fn gear_properties(g: &Gear) -> Value {
    let transverse_module = g.module / g.helix_angle.to_radians().sin();
    let transverse_pressure_angle = (g.pressure_angle.to_radians().tan() / g.helix_angle.to_radians().sin()).atan().to_degrees();
    json!({
        "type": "spur",
        "module": g.module,
        "teeth": g.teeth,
        "num_points": g.num_points,
        "simple": g.simple,
        "face_width": g.face_width,
        "height": g.face_width,
        "bore": g.bore,
        "axle_hole": g.axle_hole,
        "axle_hole_size": g.bore,
        "internal_gear": g.internal_gear,
        "rim_thickness": g.rim_thickness,
        "pressure_angle": g.pressure_angle,
        "backlash": g.backlash,
        "effective_backlash": g.effective_backlash(),
        "angular_backlash": (g.effective_backlash() / g.pitch_radius()).to_degrees(),
        "reversed_backlash": g.reversed_backlash,
        "root_fillet_radius": g.root_fillet_radius,
        "head_fillet_radius": g.head_fillet_radius,
        "helix_angle": g.helix_angle,
        "double_helix": g.double_helix,
        "offset_hole": g.offset_hole,
        "offset_hole_offset": g.offset_hole_offset,
        "offset_hole_size": g.offset_hole_size,
        "profile_shift": g.profile_shift,
        "clearance": g.clearance,
        "head_tolerance": g.head_tolerance,
        "clearance_mm": g.clearance * g.module,
        "twist": g.twist(),
        "pitch_diameter": g.pitch_radius() * 2.0,
        "outside_diameter": g.outside_radius() * 2.0,
        "addendum_diameter": g.outside_radius() * 2.0,
        "root_diameter": g.root_radius() * 2.0,
        "base_diameter": g.base_radius() * 2.0,
        "normal_module": g.module,
        "normal_pitch": std::f64::consts::PI * g.module,
        "transverse_module": transverse_module,
        "transverse_pitch": std::f64::consts::PI * transverse_module,
        "base_pitch": std::f64::consts::PI * g.module * g.pressure_angle.to_radians().cos(),
        "transverse_pressure_angle": transverse_pressure_angle
    })
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let g = read_gear(p, None)?;

    let active = s.active_component;

    let created = run(s, "component.create", json!({"name": format!("{} Gear", if g.internal_gear { "Internal" } else { "Spur" })}))?;
    let component = created["component"].as_u64().ok_or_else(|| bad("gears.insert", "component creation returned no component id"))?;
    let occurrence = created["occurrence"].as_u64().ok_or_else(|| bad("gears.insert", "component creation returned no occurrence id"))?;

    let name = str_(p, "name")
        .map(str::to_string)
        .unwrap_or_else(|| format!("{} Gear M{} Z{} #{}", if g.internal_gear { "Internal" } else { "Spur" }, g.module, g.teeth, component));

    run(s, "component.rename", json!({"component": component, "name": name}))?;

    let body_name = unique_gear_body_name(s, component)?;

    build(s, &g, &body_name)?;

    // Store the parameters so gears.resize can rebuild this gear.
    s.doc_mut()
        .parts
        .insert(component, solidcraft_doc::StandardPart { family: "spur_gear".to_string(), size: g.encode(), length: Some(g.face_width) });

    // Restore the caller's active component.
    run(s, "component.activate", json!({"component": active}))?;

    // Optional point placement. Directional placement and hole seating
    // can be added using the same transform/joint approach as parts.rs.
    if let Some(pt) = p.get("point").and_then(vec3)
        && let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.id == occurrence)
    {
        let mut m = solidcraft_doc::rigid(Vec3::ZERO, Vec3::ZERO, Vec3::X, 0.0);
        m[3] = [pt.x, pt.y, pt.z, 1.0];
        o.transform = m;
    }

    let mut properties = gear_properties(&g);
    properties["component"] = json!(component);
    properties["occurrence"] = json!(occurrence);
    properties["name"] = json!(name);
    Ok(properties)
}

/// Keep a failed multi-feature rebuild isolated from the real session.
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
    let component_key = match p.get("component") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => s.clone(),
        _ => {
            return Err(bad("gears.resize", "`component` must be an id"));
        }
    };

    let component = s.doc.find_component(&component_key).ok_or_else(|| bad("gears.resize", format!("no component `{component_key}`")))?;

    let part = s
        .doc
        .parts
        .get(&component)
        .cloned()
        .filter(|part| part.family == "spur_gear")
        .ok_or_else(|| bad("gears.resize", "component is not a spur gear"))?;

    let old_width = part.length.unwrap_or(10.0);
    let old = Gear::decode(&part.size, old_width).ok_or_else(|| bad("gears.resize", "stored gear parameters are invalid"))?;

    let g = read_gear(p, Some(&old))?;
    let body_name = s
        .model
        .state()
        .bodies
        .iter()
        .find(|body| s.doc.model_body_component(body) == component)
        .map(|body| body.name.clone())
        .map_or_else(|| unique_gear_body_name(s, component), Ok)?;

    // Remove this component's feature history in reverse order.
    let ids: Vec<u64> = s.doc.features.iter().filter(|f| f.component == component).map(|f| f.id).collect();

    for id in ids.iter().rev() {
        s.doc_mut().delete_feature(*id).map_err(|e| bad("gears.resize", format!("{e}")))?;
    }

    let active = s.active_component;

    run(s, "component.activate", json!({"component": component}))?;

    build(s, &g, &body_name)?;

    s.doc_mut()
        .parts
        .insert(component, solidcraft_doc::StandardPart { family: "spur_gear".to_string(), size: g.encode(), length: Some(g.face_width) });

    let count = s
        .model
        .state()
        .bodies
        .iter()
        .filter(|body| body.name.starts_with(&format!("{} Gear M{} Z{}", if g.internal_gear { "Internal" } else { "Spur" }, g.module, g.teeth)))
        .count();

    let name = str_(p, "name")
        .map(str::to_string)
        .unwrap_or_else(|| format!("{} Gear M{} Z{} - {}", if g.internal_gear { "Internal" } else { "Spur" }, g.module, g.teeth, count + 1));

    if let Some(c) = s.doc_mut().components.iter_mut().find(|c| c.id == component) {
        c.name = name.clone();
    }

    run(s, "component.activate", json!({"component": active}))?;

    s.refresh();

    if let Some(error) = s.model.results.iter().find_map(|r| r.error.clone()) {
        return Err(crate::EngineError::Other(error));
    }

    let mut properties = gear_properties(&g);
    properties["component"] = json!(component);
    properties["name"] = json!(name);
    Ok(properties)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn gear_height_alias_and_backlash_are_read_and_validated() {
        let gear = read_gear(&json!({"height": 8.0, "backlash": 0.2}), None).unwrap();
        assert_eq!(gear.face_width, 8.0);
        assert_eq!(gear.backlash, 0.2);

        let legacy = read_gear(&json!({"face_width": 12.0}), None).unwrap();
        assert_eq!(legacy.face_width, 12.0);
        let previously_stored = Gear::decode("m=2;z=24;b=8;pa=20;bl=0", 10.0).unwrap();
        assert_eq!(previously_stored.backlash, 0.0);
        assert_eq!(previously_stored.root_fillet_radius, 0.0);
        assert_eq!(previously_stored.helix_angle, 90.0);

        assert_eq!(read_gear(&json!({"backlash": -0.1}), None).unwrap().backlash, -0.1);
        assert!(read_gear(&json!({"module": 2.0, "backlash": -std::f64::consts::PI}), None).is_err());
        assert!(read_gear(&json!({"root_fillet_radius": -0.1}), None).is_err());
        assert!(read_gear(&json!({"root_fillet_radius": 2.0}), None).is_err());
        assert!(read_gear(&json!({"module": 2.0, "backlash": std::f64::consts::PI}), None).is_err());

        let straight = read_gear(&json!({"helix_angle": 90.0}), None).unwrap();
        assert!(straight.twist().abs() < 1e-12);
        let helical = read_gear(&json!({"height": 10.0, "helix_angle": 45.0}), None).unwrap();
        assert!((helical.twist() - 10.0 / helical.pitch_radius()).abs() < 1e-12);
        let angled = read_gear(&json!({"height": 10.0, "helix_angle": 60.0}), None).unwrap();
        let measured_angle = (angled.face_width / (angled.pitch_radius() * angled.twist())).atan().to_degrees();
        assert!((measured_angle - angled.helix_angle).abs() < 1e-12);
        assert!(read_gear(&json!({"helix_angle": 0.0}), None).is_err());
        assert!(read_gear(&json!({"helix_angle": 90.1}), None).is_err());
        assert!(read_gear(&json!({"height": 10.0, "helix_angle": 0.01}), None).is_err());
        assert!(read_gear(&json!({"teeth": MAX_HELICAL_TEETH + 1, "helix_angle": 45.0}), None).is_err());
    }

    #[test]
    fn inserted_gears_have_distinct_bodies_and_resize_independently() {
        let mut session = Session::default();
        let first = session.execute("gears.insert", &json!({"module":2,"teeth":8,"height":6,"simple":true})).unwrap();
        let second = session.execute("gears.insert", &json!({"module":3,"teeth":10,"height":9,"simple":true})).unwrap();
        let first_component = first["component"].as_u64().unwrap();
        let second_component = second["component"].as_u64().unwrap();
        let first_body_name = "Gear";
        let second_body_name = format!("Gear ({second_component})");

        assert_ne!(first_component, second_component);
        let state = session.world_state();
        let first_body = state.body(first_body_name).unwrap();
        let second_body = state.body(&second_body_name).unwrap();
        assert_eq!(session.doc.model_body_component(first_body), first_component);
        assert_eq!(session.doc.model_body_component(second_body), second_component);
        let second_height = solidcraft_kernel::measure(&second_body.body).unwrap().bbox.max.z;
        assert!((second_height - 9.0).abs() < 1e-6);

        session.execute("gears.resize", &json!({"component":second_component,"height":12})).unwrap();

        let state = session.world_state();
        let first_body = state.body(first_body_name).unwrap();
        let second_body = state.body(&second_body_name).unwrap();
        assert_eq!(session.doc.model_body_component(first_body), first_component);
        assert_eq!(session.doc.model_body_component(second_body), second_component);
        assert!((solidcraft_kernel::measure(&first_body.body).unwrap().bbox.max.z - 6.0).abs() < 1e-6);
        assert!((solidcraft_kernel::measure(&second_body.body).unwrap().bbox.max.z - 12.0).abs() < 1e-6);
    }

    #[test]
    fn helical_gear_build_completes_with_bounded_profile() {
        let mut session = Session::default();
        let result = session.execute("gears.insert", &json!({"module":2,"teeth":24,"height":10,"bore":8,"helix_angle":60,"num_points":24})).unwrap();
        let gear = read_gear(&json!({"module": 2, "teeth": 24, "helix_angle": 60.0, "num_points": 24}), None).unwrap();
        let outline = gear_outline(&gear).unwrap();
        assert!(
            outline.len() > 1_536 && outline.len() <= 2_048,
            "high accuracy should stay within the bounded smooth-sweep limit: {}",
            outline.len()
        );
        assert_eq!(result["helix_angle"], 60.0);
        let measured_angle = (10.0 / (result["pitch_diameter"].as_f64().unwrap() / 2.0 * result["twist"].as_f64().unwrap())).atan().to_degrees();
        assert!((measured_angle - 60.0).abs() < 1e-12);
        let state = session.world_state();
        let body = state.body("Gear").unwrap();
        let measure = solidcraft_kernel::measure(&body.body).unwrap();
        assert!(measure.volume > 0.0);
        assert!((measure.bbox.max.z - 10.0).abs() < 1e-6);
        assert!(session.doc.features.iter().any(|feature| feature.kind.type_name() == "SweepFeature"));
        assert!(
            session
                .doc
                .features
                .iter()
                .any(|feature| matches!(&feature.kind, solidcraft_doc::FeatureKind::Sketch { sketch, .. } if sketch.patterns.is_empty()))
        );
        assert!(!session.doc.features.iter().any(|feature| feature.kind.type_name() == "CircularPatternFeature"));
    }

    #[test]
    fn double_helix_mirrors_one_swept_half() {
        let mut session = Session::default();
        session
            .execute("gears.insert", &json!({"module":2,"teeth":8,"height":6,"helix_angle":60,"double_helix":true,"num_points":4,"simple":true}))
            .unwrap();

        let sweeps = session.doc.features.iter().filter(|feature| matches!(&feature.kind, solidcraft_doc::FeatureKind::Sweep { .. })).count();
        assert_eq!(sweeps, 1);
        assert!(session.doc.features.iter().any(|feature| matches!(&feature.kind, solidcraft_doc::FeatureKind::Mirror { combine: true, .. })));

        let state = session.world_state();
        let gear = state.body("Gear").unwrap();
        let measure = solidcraft_kernel::measure(&gear.body).unwrap();
        assert!(measure.volume > 0.0);
        assert!((measure.bbox.min.z).abs() < 1e-6);
        assert!((measure.bbox.max.z - 6.0).abs() < 1e-6);
    }

    #[test]
    fn gear_profile_computes_all_teeth_without_circular_pattern() {
        let mut session = Session::default();
        session
            .execute("gears.insert", &json!({"module": 2, "teeth": 8, "height": 6, "bore": 2, "root_fillet_radius": 0.2, "head_fillet_radius": 0.05}))
            .unwrap();
        assert!(session.doc.features.iter().any(|feature| feature.kind.type_name() == "ExtrudeFeature"));
        assert!(
            session
                .doc
                .features
                .iter()
                .any(|feature| matches!(&feature.kind, solidcraft_doc::FeatureKind::Sketch { sketch, .. } if sketch.patterns.is_empty()))
        );
        let state = session.world_state();
        assert_eq!(state.bodies.len(), 1);
        let gear = state.body("Gear").unwrap();
        let measure = solidcraft_kernel::measure(&gear.body).unwrap();
        assert!(measure.volume > 0.0);
        assert!((measure.bbox.max.z - 6.0).abs() < 1e-6);
    }

    #[test]
    fn extended_parameters_round_trip_and_report_derived_properties() {
        let gear = read_gear(
            &json!({
                "module": 2.0,
                "teeth": 20,
                "height": 8.0,
                "backlash": -0.1,
                "reversed_backlash": true,
                "profile_shift": 0.2,
                "clearance": 0.3,
                "head_tolerance": 0.05,
                "num_points": 16,
                "simple": true,
                "double_helix": true,
                "helix_angle": 45.0,
                "axle_hole": false,
                "offset_hole": true,
                "offset_hole_offset": 5.0,
                "offset_hole_size": 2.0,
                "root_fillet_radius": 0.2,
                "head_fillet_radius": 0.05
            }),
            None,
        )
        .unwrap();
        let decoded = Gear::decode(&gear.encode(), gear.face_width).unwrap();
        assert_eq!(decoded.num_points, 16);
        assert!(decoded.simple && decoded.double_helix);
        assert!(!decoded.axle_hole && decoded.offset_hole && decoded.reversed_backlash);
        assert_eq!(decoded.head_fillet_radius, 0.05);
        assert_eq!(decoded.profile_shift, 0.2);
        assert_eq!(decoded.clearance, 0.3);

        let properties = gear_properties(&gear);
        assert!((properties["pitch_diameter"].as_f64().unwrap() - 40.0).abs() < 1e-12);
        assert!((properties["outside_diameter"].as_f64().unwrap() - 44.7).abs() < 1e-12);
        assert!((properties["root_diameter"].as_f64().unwrap() - 35.6).abs() < 1e-12);
        assert!((properties["effective_backlash"].as_f64().unwrap() - 0.1).abs() < 1e-12);
        assert!((properties["transverse_module"].as_f64().unwrap() - 2.0 * std::f64::consts::SQRT_2).abs() < 1e-12);
        assert!(read_gear(&json!({"double_helix": true}), None).is_err());
    }

    #[test]
    fn detailed_tooth_profile_supports_head_and_root_fillets() {
        let gear = read_gear(
            &json!({
                "teeth": 24,
                "root_fillet_radius": 0.2,
                "head_fillet_radius": 0.05,
                "num_points": 24
            }),
            None,
        )
        .unwrap();
        let outline = gear_outline(&gear).unwrap();
        assert!(outline.len() > 1000);
        assert!(outline.iter().all(|point| point[0].is_finite() && point[1].is_finite()));
    }

    #[test]
    fn unimplemented_undercut_is_rejected_and_legacy_value_is_ignored() {
        assert!(read_gear(&json!({"undercut": true}), None).is_err());

        let legacy = Gear::decode("m=2;z=24;b=8;pa=20;uc=true", 10.0).unwrap();
        assert!(!legacy.encode().contains("uc="));
    }
}
