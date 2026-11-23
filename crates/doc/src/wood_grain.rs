//! Procedural wood grain on a look: which species, and how its grain runs through each body
//! (`solidcraft_viewport::wood`). Saved with the look; a look without it is a plain colour, as
//! before.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcraft_viewport::wood::{self, Axis, Frame, WoodShade};

/// A look's wood grain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "GrainWire", into = "GrainWire")]
pub struct Grain {
    /// The species (`walnut-dark`, or a starter id `starter.wood.walnut-dark`).
    pub species: String,
    pub axis: GrainAxis,
    /// The noise seed. `None`: each body gets its own, from its name, so the boards of a design
    /// differ and each keeps its grain.
    pub seed: Option<u32>,
    /// Scale of the whole pattern (1: real size).
    pub scale: f64,
    /// Ring spacing, mm (`None`: the species').
    pub ring_spacing_mm: Option<f64>,
    /// How much the rings wander, relative to the species (1: as the species).
    pub figure: f64,
    /// Where the log's centre (the pith) lies from the board's middle, mm: `[across its width,
    /// across its thickness]`. `[0, 0]`: rings centred on the board.
    pub pith_offset_mm: [f64; 2],
    /// How far the grain is slid along its length, mm.
    pub shift_mm: f64,
    /// Glued-up stock, mm: `[board width, board length]`. The body is shown as boards this
    /// wide glued edge to edge (and, with a length above 0, end to end), each with its own
    /// grain. `None`: one piece.
    pub stock_mm: Option<[f64; 2]>,
    /// A plywood's ply thickness, mm (`None`: the species'). The plies are fitted to each
    /// body: an odd number of them fills its thickness.
    pub ply_mm: Option<f64>,
    /// How strongly the rings show, relative to the species (1: as the species; 0: none).
    pub contrast: f64,
    /// Pore density, 0…1 (`None`: the species').
    pub pores: Option<f64>,
    /// How much dark streaking shows, 0…1 (`None`: the species').
    pub streaks: Option<f64>,
    /// The grain turned from where `axis` puts it, degrees: `[turn, tilt, roll]` (swung on the
    /// broad face, leaned out of it, the rings rolled about the grain).
    pub rotate_deg: [f64; 3],
    /// How uneven the growth is, 0…2 (0: even, round rings; 1: as a real log).
    pub variation: f64,
    /// How many knots, 0…1 (0: clear timber).
    pub knots: f64,
    /// Burl, 0…1 (0: none): swirling grain with eyes.
    pub burl: f64,
}

/// Which way the grain runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GrainAxis {
    /// Along each body's longest direction.
    Long,
    /// A fixed direction (unit vector, design coordinates).
    Fixed([f64; 3]),
}

impl Grain {
    /// The species' default grain.
    pub fn species(species: &str) -> Option<Grain> {
        wood::species_look(species)?;
        Some(Grain {
            species: species.strip_prefix("starter.wood.").unwrap_or(species).to_string(),
            axis: GrainAxis::Long,
            seed: None,
            scale: 1.0,
            ring_spacing_mm: None,
            figure: 1.0,
            pith_offset_mm: [0.0, 0.0],
            shift_mm: 0.0,
            stock_mm: None,
            ply_mm: None,
            contrast: 1.0,
            pores: None,
            streaks: None,
            rotate_deg: [0.0; 3],
            variation: 1.0,
            knots: 0.0,
            burl: 0.0,
        })
    }

    /// Check the values (finite, in range, a known species).
    pub fn validate(&self) -> Result<(), String> {
        if wood::species_look(&self.species).is_none() {
            let ids: Vec<&str> = wood::SPECIES.iter().map(|s| s.id).collect();
            return Err(format!("unknown wood species `{}` (one of {})", self.species, ids.join(", ")));
        }
        if !(self.scale.is_finite() && (0.05..=20.0).contains(&self.scale)) {
            return Err("`grain.scale_mm`/`scale` must be 0.05…20".into());
        }
        if let Some(r) = self.ring_spacing_mm
            && !(r.is_finite() && (0.2..=50.0).contains(&r))
        {
            return Err("`grain.ring_spacing_mm` must be 0.2…50 mm".into());
        }
        if !(self.figure.is_finite() && (0.0..=5.0).contains(&self.figure)) {
            return Err("`grain.figure` must be 0…5".into());
        }
        if !self.pith_offset_mm.iter().all(|v| v.is_finite() && v.abs() <= MAX_PITH_MM) {
            return Err(format!("`grain.pith_offset_mm` must be within ±{MAX_PITH_MM} mm (past that the rings are straight lines)"));
        }
        if !(self.shift_mm.is_finite() && self.shift_mm.abs() <= MAX_PITH_MM) {
            return Err(format!("`grain.shift_mm` must be within ±{MAX_PITH_MM} mm"));
        }
        if let Some([w, l]) = self.stock_mm
            && !(w.is_finite() && (5.0..=5000.0).contains(&w) && l.is_finite() && (l == 0.0 || (50.0..=50_000.0).contains(&l)))
        {
            return Err("`grain.stock_mm` must be [board width 5…5000, board length 0 (full length) or 50…50000] in mm".into());
        }
        if let Some(p) = self.ply_mm
            && !(p.is_finite() && (0.2..=50.0).contains(&p))
        {
            return Err("`grain.ply_mm` must be 0.2…50 mm".into());
        }
        if !(self.contrast.is_finite() && (0.0..=3.0).contains(&self.contrast)) {
            return Err("`grain.contrast` must be 0…3".into());
        }
        if !self.rotate_deg.iter().all(|v| v.is_finite() && v.abs() <= 360.0) {
            return Err("`grain.rotate_deg` must be [turn, tilt, roll], each within ±360 degrees".into());
        }
        if !(self.variation.is_finite() && (0.0..=2.0).contains(&self.variation)) {
            return Err("`grain.variation` must be 0…2".into());
        }
        for (name, v) in [("pores", self.pores), ("streaks", self.streaks), ("knots", Some(self.knots)), ("burl", Some(self.burl))] {
            if let Some(v) = v
                && !(v.is_finite() && (0.0..=1.0).contains(&v))
            {
                return Err(format!("`grain.{name}` must be 0…1"));
            }
        }
        if let GrainAxis::Fixed(v) = self.axis {
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if !(l.is_finite() && l > 1e-9) {
                return Err("`grain.axis` must be a nonzero finite direction".into());
            }
        }
        Ok(())
    }

    /// The shade of a body named `body` whose display mesh is `positions`/`triangles` (world).
    pub fn shade(&self, body: &str, positions: &[solidcore_record::Vec3], triangles: &[[u32; 3]]) -> Option<WoodShade> {
        let mut look = wood::species_look(&self.species)?;
        look.seed = self.seed.unwrap_or_else(|| name_seed(body)) & 0x00ff_ffff;
        if let Some(r) = self.ring_spacing_mm {
            look.ring_mm = r as f32;
        }
        look.figure_mm *= self.figure as f32;
        look.contrast = (look.contrast * self.contrast as f32).clamp(0.0, 1.0);
        if let Some(v) = self.pores {
            look.pores = v as f32;
        }
        if let Some(v) = self.streaks {
            look.streaks = v as f32;
        }
        look.variation = self.variation as f32;
        look.knots = self.knots as f32;
        look.burl = self.burl as f32;
        // A plywood sheet's plies stay flat in it: its grain turns on the face only.
        let rotate_deg = if look.ply[0] > 0.0 { [self.rotate_deg[0], 0.0, 0.0] } else { self.rotate_deg };
        let axis = match self.axis {
            GrainAxis::Long => Axis::Long,
            GrainAxis::Fixed(v) => Axis::Fixed(v),
        };
        let to_grain = wood::frame_for_mesh(
            positions,
            triangles,
            Frame { axis, scale: self.scale, pith_mm: self.pith_offset_mm, shift_mm: self.shift_mm, rotate_deg },
        )?;
        if look.ply[0] > 0.0 {
            // Plywood is a sheet, not boards: its plies are fitted to the body's thickness.
            look.ply = wood::fit_plies(positions, &to_grain, self.ply_mm.unwrap_or(f64::from(look.ply[0])), self.scale);
        } else if let Some([w, l]) = self.stock_mm {
            // In grain mm: the pattern's scale stretches the grain, not the boards.
            let s = 1.0 / self.scale.clamp(0.01, 100.0);
            look.stock = [(w * s) as f32, (l * s) as f32, (self.pith_offset_mm[0] * s) as f32];
        }
        Some(WoodShade { look, to_grain })
    }

    /// `shade` finished as its look says: tinted from the species' own colour (`species_colour`,
    /// its starter preset's) toward the look's `colour`, and as glossy as the look's
    /// `roughness` (the latewood a little rougher). A look left as the preset changes nothing
    /// but the gloss it names.
    pub fn finished(mut shade: WoodShade, species_colour: Option<[u8; 3]>, colour: [u8; 3], roughness: Option<f64>) -> WoodShade {
        if let Some(from) = species_colour.filter(|from| *from != colour) {
            let l = &mut shade.look;
            for k in 0..3 {
                let ratio = (f32::from(colour[k]) + 1.0) / (f32::from(from[k]) + 1.0);
                for c in [&mut l.early, &mut l.late, &mut l.streak, &mut l.light] {
                    c[k] = (c[k] * ratio).clamp(0.0, 1.0);
                }
            }
        }
        if let Some(r) = roughness {
            let r = r as f32;
            shade.look.roughness = [r.clamp(0.045, 1.0), (r + 0.08).clamp(0.045, 1.0)];
        }
        shade
    }

    /// Apply the fields of an `appearance.assign` `grain` object (or `true`: the defaults).
    pub fn apply_json(&mut self, v: &Value) -> Result<(), String> {
        let Some(o) = v.as_object() else {
            return if v.as_bool() == Some(true) { Ok(()) } else { Err("`grain` must be an object, true or null".into()) };
        };
        for (k, x) in o {
            match k.as_str() {
                "species" => self.species = x.as_str().ok_or("`grain.species` must be a string")?.trim_start_matches("starter.wood.").to_string(),
                "axis" => self.axis = parse_axis(x)?,
                "seed" => self.seed = Some(x.as_u64().filter(|s| *s < 1 << 24).ok_or("`grain.seed` must be an integer 0…16777215")? as u32),
                "scale" => self.scale = x.as_f64().ok_or("`grain.scale` must be a number")?,
                "ring_spacing_mm" => self.ring_spacing_mm = Some(x.as_f64().ok_or("`grain.ring_spacing_mm` must be a number")?),
                "figure" => self.figure = x.as_f64().ok_or("`grain.figure` must be a number")?,
                "pith_offset_mm" => self.pith_offset_mm = parse_pith(x)?,
                "shift_mm" => self.shift_mm = x.as_f64().ok_or("`grain.shift_mm` must be a number")?,
                "stock_mm" => self.stock_mm = parse_stock(x)?,
                "ply_mm" => self.ply_mm = Some(x.as_f64().ok_or("`grain.ply_mm` must be a number")?),
                "contrast" => self.contrast = x.as_f64().ok_or("`grain.contrast` must be a number")?,
                "pores" => self.pores = Some(x.as_f64().ok_or("`grain.pores` must be a number")?),
                "streaks" => self.streaks = Some(x.as_f64().ok_or("`grain.streaks` must be a number")?),
                "rotate_deg" => self.rotate_deg = parse_rotate(x)?,
                "variation" => self.variation = x.as_f64().ok_or("`grain.variation` must be a number")?,
                "knots" => self.knots = x.as_f64().ok_or("`grain.knots` must be a number")?,
                "burl" => self.burl = x.as_f64().ok_or("`grain.burl` must be a number")?,
                other => {
                    return Err(format!(
                        "unknown `grain` field `{other}` (species, axis, seed, scale, ring_spacing_mm, figure, pith_offset_mm, shift_mm, stock_mm, ply_mm, contrast, pores, streaks, rotate_deg, variation, knots, burl)"
                    ));
                }
            }
        }
        self.validate()
    }
}

/// The farthest the pith may lie from a board, mm (100 m: far past any log, where the rings
/// are straight lines; farther would cost the grain's precision).
pub const MAX_PITH_MM: f64 = 100_000.0;

/// `pith_offset_mm`: `[across the width, across the thickness]`, or one number (across the
/// thickness).
fn parse_pith(v: &Value) -> Result<[f64; 2], String> {
    let bad = || "`grain.pith_offset_mm` must be [across width, across thickness] in mm, or one number".to_string();
    match v {
        Value::Number(n) => Ok([0.0, n.as_f64().ok_or_else(bad)?]),
        Value::Array(a) if a.len() == 2 => Ok([a[0].as_f64().ok_or_else(bad)?, a[1].as_f64().ok_or_else(bad)?]),
        _ => Err(bad()),
    }
}

/// `rotate_deg`: `[turn, tilt, roll]` in degrees, or one number (the turn).
fn parse_rotate(v: &Value) -> Result<[f64; 3], String> {
    let bad = || "`grain.rotate_deg` must be [turn, tilt, roll] in degrees, or one number (the turn)".to_string();
    match v {
        Value::Number(n) => Ok([n.as_f64().ok_or_else(bad)?, 0.0, 0.0]),
        Value::Array(a) if a.len() == 3 => Ok([a[0].as_f64().ok_or_else(bad)?, a[1].as_f64().ok_or_else(bad)?, a[2].as_f64().ok_or_else(bad)?]),
        _ => Err(bad()),
    }
}

/// `stock_mm`: `[board width, board length]`, one number (the width, full-length boards), or
/// null (one piece).
fn parse_stock(v: &Value) -> Result<Option<[f64; 2]>, String> {
    let bad = || "`grain.stock_mm` must be [board width, board length] in mm, one number (the width) or null".to_string();
    match v {
        Value::Null => Ok(None),
        Value::Number(n) => Ok(Some([n.as_f64().ok_or_else(bad)?, 0.0])),
        Value::Array(a) if a.len() == 2 => Ok(Some([a[0].as_f64().ok_or_else(bad)?, a[1].as_f64().ok_or_else(bad)?])),
        _ => Err(bad()),
    }
}

/// A body's own seed: FNV-1a of its name, 24 bits.
pub fn name_seed(name: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h & 0x00ff_ffff
}

fn parse_axis(v: &Value) -> Result<GrainAxis, String> {
    let bad = || "`grain.axis` must be \"long\", \"X\", \"Y\", \"Z\" or [x, y, z]".to_string();
    match v {
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "long" | "auto" => Ok(GrainAxis::Long),
            "x" => Ok(GrainAxis::Fixed([1.0, 0.0, 0.0])),
            "y" => Ok(GrainAxis::Fixed([0.0, 1.0, 0.0])),
            "z" => Ok(GrainAxis::Fixed([0.0, 0.0, 1.0])),
            _ => Err(bad()),
        },
        Value::Array(a) if a.len() == 3 => {
            let c: Vec<f64> = a.iter().filter_map(Value::as_f64).collect();
            let [x, y, z] = c[..] else { return Err(bad()) };
            let l = (x * x + y * y + z * z).sqrt();
            if !(l.is_finite() && l > 1e-9) {
                return Err(bad());
            }
            Ok(GrainAxis::Fixed([x / l, y / l, z / l]))
        }
        _ => Err(bad()),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrainWire {
    species: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    axis: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seed: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ring_spacing_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    figure: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pith_offset_mm: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shift_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stock_mm: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ply_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    contrast: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pores: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    streaks: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rotate_deg: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    variation: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    knots: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    burl: Option<f64>,
}

impl TryFrom<GrainWire> for Grain {
    type Error = String;
    fn try_from(w: GrainWire) -> Result<Self, Self::Error> {
        let g = Grain {
            species: w.species,
            axis: w.axis.as_ref().map(parse_axis).transpose()?.unwrap_or(GrainAxis::Long),
            seed: w.seed.map(|s| s & 0x00ff_ffff),
            scale: w.scale.unwrap_or(1.0),
            ring_spacing_mm: w.ring_spacing_mm,
            figure: w.figure.unwrap_or(1.0),
            pith_offset_mm: w.pith_offset_mm.unwrap_or([0.0, 0.0]),
            shift_mm: w.shift_mm.unwrap_or(0.0),
            stock_mm: w.stock_mm,
            ply_mm: w.ply_mm,
            contrast: w.contrast.unwrap_or(1.0),
            pores: w.pores,
            streaks: w.streaks,
            rotate_deg: w.rotate_deg.unwrap_or([0.0; 3]),
            variation: w.variation.unwrap_or(1.0),
            knots: w.knots.unwrap_or(0.0),
            burl: w.burl.unwrap_or(0.0),
        };
        g.validate()?;
        Ok(g)
    }
}

impl From<Grain> for GrainWire {
    fn from(g: Grain) -> Self {
        GrainWire {
            species: g.species,
            axis: match g.axis {
                GrainAxis::Long => None,
                GrainAxis::Fixed(v) => Some(Value::from(v.to_vec())),
            },
            seed: g.seed,
            scale: (g.scale != 1.0).then_some(g.scale),
            ring_spacing_mm: g.ring_spacing_mm,
            figure: (g.figure != 1.0).then_some(g.figure),
            pith_offset_mm: (g.pith_offset_mm != [0.0, 0.0]).then_some(g.pith_offset_mm),
            shift_mm: (g.shift_mm != 0.0).then_some(g.shift_mm),
            stock_mm: g.stock_mm,
            ply_mm: g.ply_mm,
            contrast: (g.contrast != 1.0).then_some(g.contrast),
            pores: g.pores,
            streaks: g.streaks,
            rotate_deg: (g.rotate_deg != [0.0; 3]).then_some(g.rotate_deg),
            variation: (g.variation != 1.0).then_some(g.variation),
            knots: (g.knots != 0.0).then_some(g.knots),
            burl: (g.burl != 0.0).then_some(g.burl),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn grain_settings_round_trip_and_refuse_bad_values() {
        let mut g = Grain::species("starter.wood.walnut-dark").unwrap();
        assert_eq!(serde_json::to_value(&g).unwrap(), json!({"species": "walnut-dark"}));
        g.apply_json(&json!({"axis": "X", "seed": 7, "scale": 1.5, "ring_spacing_mm": 5.0, "figure": 0.5, "pith_offset_mm": [10.0, -150.0], "shift_mm": 40.0, "stock_mm": [150.0, 1200.0], "ply_mm": 1.5, "contrast": 1.5, "pores": 0.2, "streaks": 0.9, "rotate_deg": [30.0, -5.0, 90.0], "variation": 1.5, "knots": 0.4, "burl": 0.2})).unwrap();
        let saved = serde_json::to_value(&g).unwrap();
        assert_eq!(serde_json::from_value::<Grain>(saved.clone()).unwrap(), g);
        assert_eq!(saved["axis"], json!([1.0, 0.0, 0.0]));
        for bad in [
            json!({"species": "balsa"}),
            json!({"axis": [0, 0, 0]}),
            json!({"scale": 0}),
            json!({"ring_spacing_mm": -1}),
            json!({"knots": true}),
            json!({"knots": 2}),
            json!({"burl": -1}),
            json!({"variation": 3}),
            json!({"rotate_deg": [0, 0]}),
            json!({"rotate_deg": [0, 0, 1e9]}),
            json!({"sapwood": 1}),
            json!({"pith_offset_mm": [0, 1e9]}),
            json!({"pith_offset_mm": "far"}),
            json!({"contrast": 9}),
            json!({"stock_mm": [1, 0]}),
            json!({"shift_mm": 1e9}),
            json!({"ply_mm": 0}),
            json!({"pores": 2}),
        ] {
            assert!(g.clone().apply_json(&bad).is_err(), "{bad}");
        }
        assert!(serde_json::from_value::<Grain>(json!({"species": "walnut-dark", "scale": 1e9})).is_err());
    }

    /// Plywood's plies fill each body's thickness: 13 in an 18 mm sheet, fewer and thicker
    /// when asked, whatever the pattern's scale.
    #[test]
    fn plywood_plies_are_fitted_to_the_body() {
        use solidcore_record::Vec3;
        let (x, y, z) = (600.0, 300.0, 18.0);
        let p: Vec<Vec3> = (0..8)
            .map(|i| Vec3::new(if i & 1 == 0 { 0.0 } else { x }, if i & 2 == 0 { 0.0 } else { y }, if i & 4 == 0 { 5.0 } else { 5.0 + z }))
            .collect();
        let t =
            vec![[0, 2, 1], [1, 2, 3], [4, 5, 6], [5, 7, 6], [0, 1, 4], [1, 5, 4], [2, 6, 3], [3, 6, 7], [0, 4, 2], [2, 4, 6], [1, 3, 5], [3, 7, 5]];
        let mut g = Grain::species("starter.wood.birch-plywood").unwrap();
        let shade = g.shade("Panel", &p, &t).unwrap();
        assert_eq!(shade.look.ply[2], 13.0);
        assert!((shade.look.ply[0] - 18.0 / 13.0).abs() < 1e-4);
        // The stack starts at the sheet's bottom face.
        let low = shade.grain_point(Vec3::new(10.0, 10.0, 5.0))[2].min(shade.grain_point(Vec3::new(10.0, 10.0, 23.0))[2]);
        assert!((shade.look.ply[1] - low).abs() < 1e-3);
        g.apply_json(&json!({"ply_mm": 3.0, "scale": 2.0, "stock_mm": 100.0})).unwrap();
        let shade = g.shade("Panel", &p, &t).unwrap();
        assert_eq!(shade.look.ply[2], 7.0, "a real 18 mm in plies of about 3 mm");
        assert_eq!(shade.look.stock, [0.0; 3], "a sheet is not glued from boards");
        assert_eq!(Grain::species("oak-honey").unwrap().shade("Panel", &p, &t).unwrap().look.ply, [0.0; 3]);
    }
}
