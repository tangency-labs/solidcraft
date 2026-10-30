//! Manual welding notes, independent of modelled beads, sheets and leaders (#7259).
//!
//! [`Weld`] stores manufacturing intent in millimetres. [`Convention`] is an explicit rendering
//! choice, never a reinterpretation of that intent. [`Weld::draw`] produces original paper
//! strokes and measured text in local x-right/y-up millimetres. Its origin is the reference-line
//! elbow, for a later leader; it does not draw an arrow or attach anything to a model.
//!
//! This is a bounded basic subset, not certification to the complete standards. See
//! `docs/weld-symbols.md` for clause references, supported combinations and layout limits.
//! ISO intermittent clear spacing is derived from stored centre pitch, never confused
//! with it; unsupported combinations return errors.

use serde::{Deserialize, Serialize};

use crate::{Loop2, Region2, Seg2, Vec2, paper::Item};

/// Engineering dimensions accepted by this record, in mm (not drawing scale).
pub const MIN_DIMENSION_MM: f64 = 1e-6;
pub const MAX_DIMENSION_MM: f64 = 100_000.0;
/// A single line of complementary information, in UTF-8 bytes.
pub const MAX_TAIL_BYTES: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("welding symbol: {0}")]
pub struct Invalid(pub &'static str);
pub type Result<T> = std::result::Result<T, Invalid>;

fn require(ok: bool, message: &'static str) -> Result<()> {
    if ok { Ok(()) } else { Err(Invalid(message)) }
}

/// Semantic side of the joint, independent of the line's page position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Arrow,
    Other,
}

/// The elementary shapes supported here. Preparation angles and root openings are later work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Fillet,
    SquareButt,
    SingleVButt,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Fillet => "fillet",
            Self::SquareButt => "square butt",
            Self::SingleVButt => "single-V butt",
        }
    }
}

/// A dimension's meaning is retained when changing the rendering convention.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "mm", rename_all = "snake_case", deny_unknown_fields)]
pub enum Size {
    Leg(f64),
    Throat(f64),
    /// ISO deep penetration needs both the total and nominal throat dimensions.
    DeepPenetrationThroat {
        total_mm: f64,
        nominal_mm: f64,
    },
    ButtPenetration(f64),
}

impl Size {
    fn value(self) -> f64 {
        match self {
            Self::Leg(v) | Self::Throat(v) | Self::ButtPenetration(v) => v,
            Self::DeepPenetrationThroat { total_mm, .. } => total_mm,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Leg(_) => "leg",
            Self::Throat(_) => "throat",
            Self::DeepPenetrationThroat { .. } => "deep penetration throat",
            Self::ButtPenetration(_) => "butt penetration",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Contour {
    /// Straight-face mark: ISO flush; AWS flat for fillets, flush for grooves.
    /// Does not choose a finishing operation.
    Flush,
    Convex,
    Concave,
}

/// Centre-to-centre pitch, never the clear gap between elements. Supports single-sided
/// intermittent fillets or matching two-sided chain fillets, never a staggered arrangement.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intermittent {
    /// Number of elements when specified. An omitted ISO count covers the joint length.
    /// AWS counted fillet callouts are unsupported. This count never repeats geometry here.
    pub count: Option<u16>,
    pub pitch_mm: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeldSide {
    pub kind: Kind,
    pub size: Option<Size>,
    /// Nominal length, or length of each element when intermittent.
    pub length_mm: Option<f64>,
    pub intermittent: Option<Intermittent>,
    pub contour: Option<Contour>,
}

impl WeldSide {
    pub fn validate(&self) -> Result<()> {
        if let Some(size) = self.size {
            dimension(size.value())?;
            if let Size::DeepPenetrationThroat { total_mm, nominal_mm } = size {
                dimension(nominal_mm)?;
                require(total_mm > nominal_mm, "deep penetration throat must exceed nominal throat")?;
            }
            require(
                matches!(
                    (self.kind, size),
                    (Kind::Fillet, Size::Leg(_) | Size::Throat(_) | Size::DeepPenetrationThroat { .. })
                        | (Kind::SquareButt | Kind::SingleVButt, Size::ButtPenetration(_))
                ),
                "size meaning does not match weld kind",
            )?;
        }
        if let Some(length) = self.length_mm {
            dimension(length)?;
        }
        if let Some(i) = self.intermittent {
            require(self.kind == Kind::Fillet, "only intermittent fillets are supported")?;
            require(i.count.is_none_or(|n| n >= 2), "intermittent count must be at least two")?;
            dimension(i.pitch_mm)?;
            let length = self.length_mm.ok_or(Invalid("pitch needs an element length"))?;
            require(i.pitch_mm >= length + MIN_DIMENSION_MM, "pitch must exceed element length by at least 1e-6 mm")?;
        }
        Ok(())
    }
}

/// No model, source path or sheet attachment is needed for an ordinary manual note.
/// With neither elementary symbol, a non-empty tail gives the details elsewhere.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Weld {
    pub arrow_side: Option<WeldSide>,
    pub other_side: Option<WeldSide>,
    pub all_round: bool,
    pub field: bool,
    pub tail: Option<String>,
}

fn dimension(value: f64) -> Result<()> {
    require(value.is_finite() && (MIN_DIMENSION_MM..=MAX_DIMENSION_MM).contains(&value), "dimension must be finite and between 1e-6 and 100000 mm")
}

impl Weld {
    /// Check after deserializing and before accepting an edit. Encoded-input boundaries must
    /// additionally cap bytes/nesting before parsing. Text and geometry also call this check.
    pub fn validate(&self) -> Result<()> {
        require(self.arrow_side.is_some() || self.other_side.is_some() || self.tail.is_some(), "a note needs a weld kind or a tail")?;
        for side in [&self.arrow_side, &self.other_side].into_iter().flatten() {
            side.validate()?;
            require(!(self.all_round && side.intermittent.is_some()), "all-round and intermittent are not combined in this subset")?;
        }
        if let Some(tail) = &self.tail {
            require(tail.len() <= MAX_TAIL_BYTES && !tail.trim().is_empty(), "tail must contain text and be at most 512 bytes")?;
            require(
                !tail.chars().any(|c| c.is_control() || matches!(c, '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')),
                "tail must be a single line without controls or bidi overrides",
            )?;
        }
        if let (Some(a), Some(b)) = (&self.arrow_side, &self.other_side)
            && (a.intermittent.is_some() || b.intermittent.is_some())
        {
            require(
                a.intermittent == b.intermittent && a.length_mm == b.length_mm,
                "two-sided intermittent fillets must have matching chain spacing",
            )?;
        }
        Ok(())
    }

    /// A lossless, human-readable semantic summary, not a standards callout or a parser format.
    /// Values are in mm and retain their full stored precision; tail text is quoted.
    pub fn text(&self) -> Result<String> {
        self.validate()?;
        let arrangement = if self.arrow_side.is_some() && self.other_side.is_some() { "chain" } else { "intermittent" };
        let mut parts = Vec::new();
        for (name, side) in [("arrow", &self.arrow_side), ("other", &self.other_side)] {
            if let Some(s) = side {
                let mut text = format!("{name}: {}", s.kind.name());
                if let Some(size) = s.size {
                    text.push_str(&format!(", {}={} mm", size.name(), size.value()));
                    if let Size::DeepPenetrationThroat { nominal_mm, .. } = size {
                        text.push_str(&format!(", nominal throat={nominal_mm} mm"));
                    }
                }
                if let Some(length) = s.length_mm {
                    text.push_str(&format!(", length={length} mm"));
                }
                if let Some(i) = s.intermittent {
                    if let Some(n) = i.count {
                        text.push_str(&format!(", {arrangement} count={n}"));
                    }
                    text.push_str(&format!(", {arrangement} pitch={} mm", i.pitch_mm));
                }
                if let Some(c) = s.contour {
                    text.push_str(match c {
                        Contour::Flush => ", flat/flush face",
                        Contour::Convex => ", convex",
                        Contour::Concave => ", concave",
                    });
                }
                parts.push(text);
            }
        }
        if self.all_round {
            parts.push("all round".into());
        }
        if self.field {
            parts.push("field".into());
        }
        if let Some(tail) = &self.tail {
            parts.push(format!("tail={tail:?}"));
        }
        Ok(parts.join("; "))
    }
}

/// Page position of the identification (dashed) line in ISO system A.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentificationLine {
    Above,
    Below,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Convention {
    Iso2553_2019A { identification: IdentificationLine },
    Iso2553_2019B,
    AwsA2_4_2020,
}

/// Lettering height and line width on paper, never multiplied by a model/view scale.
/// AWS geometry uses the SI reference profile scaled with this height; ISO layout choices and
/// the distinction between mandatory rules and informative guidance are in docs/weld-symbols.md.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub height_mm: f64,
    pub width_mm: f64,
}

impl Default for Style {
    fn default() -> Self {
        Self { height_mm: 3.5, width_mm: 0.35 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextRole {
    Size(Side),
    Length(Side),
    Tail,
}

/// Left-aligned baseline in local paper mm. Draw with the very same font/height used by the
/// measurement callback; no font glyphs or guessed character widths are supplied here.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub role: TextRole,
    pub text: String,
    pub baseline: Vec2,
    pub height_mm: f64,
    pub width_mm: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Drawing {
    pub convention: Convention,
    pub items: Vec<Item>,
    pub text: Vec<Text>,
    /// From the elbow (0,0) to the tail junction, along +x.
    pub reference_length_mm: f64,
}

impl Weld {
    /// Original local symbol geometry and measured dimension/tail labels. `measure(text, h)`
    /// supplies the actual font's ink/advance width in paper mm. Invalid metrics fail the whole
    /// operation. No text is silently omitted and no guessed font metrics enter the layout.
    pub fn draw(&self, convention: Convention, style: Style, mut measure: impl FnMut(&str, f64) -> f64) -> Result<Drawing> {
        self.validate()?;
        let Style { height_mm: h, width_mm: d } = style;
        require(h.is_finite() && (1.0..=20.0).contains(&h), "paper height must be 1 to 20 mm")?;
        require(d.is_finite() && d >= 0.01 && d <= h / 4.0, "line width must be 0.01 mm to one quarter of height")?;
        let aws = convention == Convention::AwsA2_4_2020;
        // AWS SI reference geometry is scaled together; lettering remains caller-measured.
        let g = if aws { h * (10.0 / 7.0) } else { h };
        let symmetric = self.arrow_side.is_some() && self.arrow_side == self.other_side;
        let dual = matches!(convention, Convention::Iso2553_2019A { .. }) && !symmetric && (self.arrow_side.is_some() || self.other_side.is_some());
        let mut labels = Vec::new();
        for (side, spec) in [(Side::Arrow, &self.arrow_side), (Side::Other, &self.other_side)] {
            if let Some(s) = spec {
                let (size, length) = callout(s, convention)?;
                for (role, text) in [(TextRole::Size(side), size), (TextRole::Length(side), length)] {
                    if !text.is_empty() {
                        let width = measure(&text, h);
                        require(width.is_finite() && width > 0.0 && width <= 1000.0, "text metric must be finite, positive and at most 1000 mm")?;
                        labels.push(Text { role, text, baseline: Vec2::ZERO, height_mm: h, width_mm: width });
                    }
                }
            }
        }
        let left = labels.iter().filter(|t| matches!(t.role, TextRole::Size(_))).map(|t| t.width_mm).fold(0.0, f64::max);
        let right = labels.iter().filter(|t| matches!(t.role, TextRole::Length(_))).map(|t| t.width_mm).fold(0.0, f64::max);
        let symbol_width = [&self.arrow_side, &self.other_side].into_iter().flatten().map(|s| elementary_size(s.kind, aws, g).0).fold(g, f64::max);
        let x = 3.0 * h + left;
        let end = x + symbol_width + right + 3.0 * h;
        let mut drawing = Drawing { convention, items: Vec::new(), text: Vec::new(), reference_length_mm: end };
        let round_radius = if !self.all_round {
            0.0
        } else if aws {
            0.45 * g
        } else {
            0.3 * g
        };
        stroke(&mut drawing.items, vec![line(round_radius, 0.0, end, 0.0)], false, d);
        let dashed_y = match convention {
            Convention::Iso2553_2019A { identification: IdentificationLine::Below } => -2.0 * h,
            _ => 2.0 * h,
        };
        if dual {
            // Explicit strokes: the paper scene has no line-pattern primitive yet. The bounded
            // reference length and minimum height cap this at fewer than 500 dashes. Both
            // reference lines have the same extent, with a dash at each end.
            let count = ((end - round_radius) / (4.5 * h)).ceil().max(2.0) as usize;
            let gap = 1.5 * h;
            let dash = (end - round_radius - gap * (count - 1) as f64) / count as f64;
            for i in 0..count {
                let start = round_radius + i as f64 * (dash + gap);
                let finish = if i + 1 == count { end } else { start + dash };
                stroke(&mut drawing.items, vec![line(start, dashed_y, finish, dashed_y)], false, d);
            }
        }
        let position = |side| -> (f64, f64) {
            if dual {
                let sign = dashed_y.signum();
                if side == Side::Arrow { (0.0, -sign) } else { (dashed_y, sign) }
            } else if side == Side::Arrow {
                (0.0, -1.0)
            } else {
                (0.0, 1.0)
            }
        };
        for label in &mut labels {
            let (side, left_label) = match label.role {
                TextRole::Size(s) => (s, true),
                TextRole::Length(s) => (s, false),
                TextRole::Tail => continue,
            };
            let (y, sign) = position(side);
            label.baseline =
                Vec2::new(if left_label { x - h - label.width_mm } else { x + symbol_width + h }, y + if sign > 0.0 { 0.25 * h } else { -1.25 * h });
        }
        drawing.text = labels;
        for (side, spec) in [(Side::Arrow, &self.arrow_side), (Side::Other, &self.other_side)] {
            if let Some(s) = spec {
                let (y, sign) = position(side);
                let (w, height) = elementary_size(s.kind, aws, g);
                elementary(&mut drawing.items, s.kind, x, y, w, sign * height, d);
                if let Some(contour) = s.contour {
                    let diagonal = 1.0 / 2.0_f64.sqrt();
                    let (face, tangent, outward) = if s.kind == Kind::Fillet {
                        (Vec2::new(x + g / 2.0, y + sign * g / 2.0), Vec2::new(diagonal, -sign * diagonal), Vec2::new(diagonal, sign * diagonal))
                    } else {
                        (Vec2::new(x + w / 2.0, y + sign * height), Vec2::new(1.0, 0.0), Vec2::new(0.0, sign))
                    };
                    let radius = 1.5 * g;
                    let offset = radius * 3.0_f64.sqrt() / 2.0;
                    let middle = face + outward * (0.5 * g + radius - offset);
                    let p0 = middle - tangent * (0.75 * g);
                    let p3 = middle + tangent * (0.75 * g);
                    let path = match contour {
                        Contour::Flush => Seg2::Line { a: p0, b: p3 },
                        Contour::Convex | Contour::Concave => {
                            let bulge = if contour == Contour::Convex { 1.0 } else { -1.0 };
                            let center = middle - outward * (bulge * offset);
                            let start = (p0.y - center.y).atan2(p0.x - center.x);
                            Seg2::Arc { center, radius, start, sweep: -sign * bulge * std::f64::consts::FRAC_PI_3 }
                        }
                    };
                    stroke(&mut drawing.items, vec![path], false, d);
                }
            }
        }
        if self.all_round {
            stroke(
                &mut drawing.items,
                vec![Seg2::Arc { center: Vec2::ZERO, radius: round_radius, start: 0.0, sweep: std::f64::consts::TAU }],
                true,
                d,
            );
        }
        if self.field {
            stroke(&mut drawing.items, vec![line(0.0, round_radius, 0.0, 1.2 * g)], false, d);
            drawing.items.push(Item::Fill {
                region: Region2 {
                    outer: Loop2 { segs: vec![line(0.0, 1.2 * g, 0.0, 0.6 * g), line(0.0, 0.6 * g, g, 0.9 * g), line(g, 0.9 * g, 0.0, 1.2 * g)] },
                    holes: Vec::new(),
                },
            });
        }
        if let Some(tail) = &self.tail {
            let width = measure(tail, h);
            require(width.is_finite() && width > 0.0 && width <= 1000.0, "tail metric must be finite, positive and at most 1000 mm")?;
            stroke(&mut drawing.items, vec![line(end + g, -g, end, 0.0), line(end, 0.0, end + g, g)], false, d);
            drawing.text.push(Text {
                role: TextRole::Tail,
                text: tail.clone(),
                baseline: Vec2::new(end + 1.5 * g, -h / 2.0),
                height_mm: h,
                width_mm: width,
            });
        }
        Ok(drawing)
    }
}

fn callout(side: &WeldSide, convention: Convention) -> Result<(String, String)> {
    let aws = convention == Convention::AwsA2_4_2020;
    let size = match side.size {
        None => String::new(),
        Some(Size::Leg(v)) => {
            if aws {
                v.to_string()
            } else {
                format!("z{v}")
            }
        }
        Some(Size::Throat(v)) if !aws => format!("a{v}"),
        Some(Size::DeepPenetrationThroat { total_mm, nominal_mm }) if !aws => format!("s{total_mm} a{nominal_mm}"),
        Some(Size::ButtPenetration(v)) if !aws => v.to_string(),
        Some(_) => return Err(Invalid("this size meaning has no supported AWS callout; no automatic conversion")),
    };
    let length = match (side.length_mm, side.intermittent) {
        (None, _) => String::new(),
        (Some(l), None) => l.to_string(),
        (Some(l), Some(i)) if aws && i.count.is_none() => format!("{l}-{}", i.pitch_mm),
        (Some(_), Some(_)) if aws => return Err(Invalid("counted AWS intermittent fillet callouts are not supported; use the semantic text")),
        (Some(l), Some(Intermittent { count: Some(n), pitch_mm })) => format!("{n}×{l} ({})", pitch_mm - l),
        (Some(l), Some(Intermittent { count: None, pitch_mm })) => format!("{l} ({})", pitch_mm - l),
    };
    Ok((size, length))
}

fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> Seg2 {
    Seg2::Line { a: Vec2::new(x1, y1), b: Vec2::new(x2, y2) }
}

fn stroke(items: &mut Vec<Item>, path: Vec<Seg2>, closed: bool, width_mm: f64) {
    items.push(Item::Stroke { path, closed, width_mm });
}

fn elementary_size(kind: Kind, aws: bool, g: f64) -> (f64, f64) {
    match (kind, aws) {
        (Kind::Fillet, _) => (g, g),
        (Kind::SquareButt, true) => (0.6 * g, 1.2 * g),
        (Kind::SquareButt, false) => (0.5 * g, g),
        (Kind::SingleVButt, true) => (2.0 * g, g),
        (Kind::SingleVButt, false) => (g, g * 3.0_f64.sqrt() / 2.0),
    }
}

fn elementary(items: &mut Vec<Item>, kind: Kind, x: f64, y: f64, w: f64, h: f64, d: f64) {
    match kind {
        // The base belongs to the symbol, including over a gap in ISO A's dashed line.
        Kind::Fillet => stroke(items, vec![line(x, y, x, y + h), line(x, y + h, x + w, y), line(x + w, y, x, y)], true, d),
        Kind::SquareButt => {
            stroke(items, vec![line(x, y, x, y + h)], false, d);
            stroke(items, vec![line(x + w, y, x + w, y + h)], false, d);
        }
        Kind::SingleVButt => {
            stroke(items, vec![line(x, y + h, x + w / 2.0, y), line(x + w / 2.0, y, x + w, y + h)], false, d);
        }
    }
}

#[cfg(test)]
#[path = "weld_tests.rs"]
mod tests;
