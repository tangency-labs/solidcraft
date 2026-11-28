//! Gears on selected shafts: what a selection of shafts (sketch lines, sketch points or circles,
//! model edges, cylindrical faces, axes), each with an optional size, makes.
//!
//! - one shaft: a single gear on it (or a set's first member, for a family that is a set);
//! - two shafts: the set their lie implies, by the axis-first solver ([`crate::axes::solve`],
//!   the one behind `gear.from_axes`): parallel a pair, meeting a bevel pair, skew crossed
//!   helicals or a worm; a family asked for instead where it fits;
//! - three or more shafts (all parallel): a gear train ([`crate::plan::train`]) with its centres
//!   where the shafts are; each gear's pitch diameter as given, the ones not given filling the
//!   gap to the gear before (or after) it, so the shafts in order are a chain.
//!
//! A tooth style (spur, helical, herringbone) asked for is the teeth of the set; a family
//! (pair, bevel, worm...) the set itself. Bearings are seats of the clearance body placed by
//! [`bearing_point`] (see [`crate::clearance`]).

use crate::axes::{self, Axis, Drive, Relation, Solved, dot, num};
use crate::geom::V2;
use crate::plan::{Fact, Frame, Kind, Spec, Unit};
use crate::sketch_rules::{Circle, fit_module, is_style, train_spec};

/// One selected shaft and the size asked for the gear on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shaft {
    pub axis: Axis,
    /// The gear's pitch diameter (mm).
    pub pitch_diameter: Option<f64>,
    pub teeth: Option<u32>,
}

/// What the selection's set is asked to be besides its shafts.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Ask {
    /// A tooth style or a family (`None`: spur teeth, the set the shafts imply).
    pub kind: Option<Kind>,
    pub module: Option<f64>,
    /// Gear 2's teeth over gear 1's, for two shafts without both sizes.
    pub ratio: Option<f64>,
    /// How far along shaft 1 (from its origin, along its direction) the gears sit (mm).
    pub along: f64,
    pub face_width: Option<f64>,
    /// Helix angle for helical and herringbone teeth (default 20°) and gear 1's hand.
    pub helix: Option<f64>,
    pub left_hand: bool,
}

/// The point `along` mm along `axis` from its origin: where a bearing on a selected shaft goes.
pub fn bearing_point(axis: &Axis, along: f64) -> [f64; 3] {
    [0, 1, 2].map(|i| axis.origin[i] + along * axis.dir[i])
}

/// The module and teeth of a gear of pitch diameter `d` (helix `helix_deg`): with both given
/// they must agree; teeth alone take the module that makes `d`; a module alone the nearest whole
/// teeth; neither the largest standard module (as drawn circles are sized).
fn sized(who: &str, d: f64, helix_deg: f64, module: Option<f64>, teeth: Option<u32>, notes: &mut Vec<String>) -> Result<(f64, u32), String> {
    if !(d.is_finite() && d > 0.0) {
        return Err(format!("{who}: the pitch diameter must be greater than 0"));
    }
    let cb = helix_deg.to_radians().cos();
    match (module, teeth) {
        (Some(m), Some(z)) => {
            let got = m * f64::from(z) / cb;
            if (got - d).abs() > 1e-6 * d.max(1.0) {
                // The pitch circle is the one drawn: say so, and what fits it.
                let fit = d * cb / m;
                let teeth_fit = if (fit - fit.round()).abs() < 1e-6 && fit.round() >= 3.0 {
                    format!("set {} teeth ({} × module {} = Ø{})", fit.round(), fit.round(), num(m), num(d))
                } else {
                    format!("set module {} for {z} teeth", num(d * cb / f64::from(z.max(1))))
                };
                return Err(format!(
                    "{who}: its pitch circle is Ø{} as drawn, but {z} teeth at module {} make Ø{} (gears that mesh share one module): {teeth_fit}, or redraw the circle Ø{}",
                    num(d),
                    num(m),
                    num(got),
                    num(got)
                ));
            }
            Ok((m, z))
        }
        (None, Some(z)) => {
            let m = d * cb / f64::from(z.max(1));
            notes.push(format!("{who}: module {} gives {z} teeth on Ø{}", num(m), num(d)));
            Ok((m, z))
        }
        (Some(m), None) => {
            let z = (d * cb / m).round().max(3.0) as u32;
            let got = m * f64::from(z) / cb;
            if (got - d).abs() > 1e-6 {
                notes.push(format!("{who}: Ø{} asked, Ø{} built ({z} teeth at module {})", num(d), num(got), num(m)));
            }
            Ok((m, z))
        }
        (None, None) => {
            let (m, z, standard) = fit_module(&[(d, 1.0 / cb)], 8);
            if !standard {
                notes.push(format!("{who}: no standard module gives Ø{}: module {} does", num(d), num(m)));
            }
            Ok((m, z[0]))
        }
    }
}

/// The set the selected `shafts` make (see the module docs), placed on them. `base` holds the
/// settings the selection doesn't give (pressure angle, bores, face width...).
pub fn set_on_shafts(shafts: &[Shaft], ask: &Ask, base: &Spec) -> Result<Solved, String> {
    if shafts.is_empty() {
        return Err("select a shaft for the gear: a sketch line or point, a circle, a model edge, a cylindrical face or an axis".into());
    }
    if !ask.along.is_finite() {
        return Err("`along` must be a finite distance".into());
    }
    let style = ask.kind.filter(|k| is_style(*k));
    let family = ask.kind.filter(|k| !is_style(*k));
    let helix = match style {
        Some(Kind::Helical | Kind::Herringbone) => ask.helix.map(f64::abs).filter(|h| *h > 0.0).unwrap_or(20.0),
        _ if family.is_some() => ask.helix.unwrap_or(base.helix),
        _ => 0.0,
    };
    let mut notes = Vec::new();
    match shafts {
        [one] => {
            let kind = ask.kind.unwrap_or(Kind::Spur);
            let mut spec = Spec { kind, helix: if is_style(kind) { helix } else { base.helix }, ..base.clone() };
            if let Some(f) = ask.face_width {
                spec.face_width = f;
            }
            let drive = Drive { along: ask.along, ..Drive::default() };
            match one.pitch_diameter {
                Some(d) => (spec.module, spec.teeth) = sized("gear 1", d, spec.helix, ask.module, one.teeth, &mut notes)?,
                None => {
                    if let Some(m) = ask.module {
                        spec.module = m;
                    }
                    if let Some(z) = one.teeth {
                        spec.teeth = z;
                    }
                }
            }
            let mut out = axes::solve(&one.axis, None, Some(kind), &spec, &drive)?;
            out.notes.splice(0..0, notes);
            Ok(out)
        }
        [s1, s2] => {
            let rel = axes::relate(&s1.axis, &s2.axis);
            let kind = match (family, &rel) {
                (Some(k), _) => k,
                (None, Relation::Coincident) => return Err("the two shafts are one line: select two different shafts".into()),
                (None, Relation::Parallel { .. }) => Kind::Pair,
                (None, Relation::Intersecting { .. }) if helix != 0.0 => Kind::SpiralBevel,
                (None, Relation::Intersecting { .. }) => Kind::Bevel,
                (None, rel) => axes::natural(Some(rel)),
            };
            // The base's settings (bores, shafts, tooth form...), with the family's own numbers
            // when the base was made for another type.
            let mut spec = Spec { kind, ..base.clone() };
            if base.kind != kind {
                let d = Spec::defaults_for(kind);
                (spec.helix, spec.spiral_angle, spec.starts, spec.offset) = (d.helix, d.spiral_angle, d.starts, d.offset);
            }
            if kind == Kind::Pair {
                spec.helix = helix;
                spec.herringbone = style == Some(Kind::Herringbone);
            }
            // Crossed helicals on skew shafts: sized with equal helices that add up to the
            // shafts' angle (the acute one: the set takes the hand that fits).
            if kind == Kind::CrossedHelical
                && let Relation::Skew { angle, .. } = rel
                && base.kind != kind
            {
                spec.helix = 0.5 * fold(angle);
            }
            if let Some(f) = ask.face_width {
                spec.face_width = f;
            }
            if let Some(m) = ask.module {
                spec.module = m;
            }
            let mut drive = Drive {
                along: ask.along,
                ratio: ask.ratio,
                module_free: ask.module.is_none(),
                teeth_free: s1.teeth.is_none(),
                teeth2_free: s2.teeth.is_none(),
                starts_free: true,
                ..Drive::default()
            };
            if let Some(z) = s1.teeth {
                spec.teeth = z;
            }
            if let Some(z) = s2.teeth {
                spec.teeth2 = z;
            }
            let parallel = matches!(rel, Relation::Parallel { .. });
            match (s1.pitch_diameter, s2.pitch_diameter, &rel) {
                // Both sizes on parallel shafts: their ratio; module and teeth to fit the spacing.
                (Some(d1), Some(d2), _) if parallel => {
                    drive.ratio = Some(d2 / d1);
                    if let Relation::Parallel { distance, .. } = rel
                        && ((d1 + d2) / 2.0 - distance).abs() > 0.01 * distance
                    {
                        notes.push(format!(
                            "Ø{} and Ø{} add up to {} mm between centres, but the shafts are {} mm apart: the gears are sized to the shafts",
                            num(d1),
                            num(d2),
                            num((d1 + d2) / 2.0),
                            num(distance)
                        ));
                    }
                }
                (d1, d2, _) => {
                    // A size fixes that gear's module and teeth; on parallel shafts the other
                    // gear fills the spacing.
                    if let Some(d) = d1 {
                        (spec.module, spec.teeth) = sized("gear 1", d, spec.helix, ask.module, s1.teeth, &mut notes)?;
                        (drive.module_free, drive.teeth_free) = (false, false);
                    }
                    if let Some(d) = d2 {
                        let m = if d1.is_some() { Some(spec.module) } else { ask.module };
                        (spec.module, spec.teeth2) = sized("gear 2", d, spec.helix, m, s2.teeth, &mut notes)?;
                        (drive.module_free, drive.teeth2_free) = (false, false);
                    }
                    if let Relation::Parallel { distance, .. } = rel
                        && drive.ratio.is_none()
                    {
                        let other = |d: f64| 2.0 * distance - d;
                        match (d1, d2) {
                            (Some(d), None) | (None, Some(d)) if other(d) <= 0.0 => {
                                return Err(format!(
                                    "a Ø{} gear reaches past the other shaft ({} mm away): make it smaller than Ø{}",
                                    num(d),
                                    num(distance),
                                    num(2.0 * distance)
                                ));
                            }
                            (Some(d), None) => drive.ratio = Some(other(d) / d),
                            (None, Some(d)) => drive.ratio = Some(d / other(d)),
                            _ => {}
                        }
                    }
                }
            }
            let mut out = axes::solve(&s1.axis, Some(&s2.axis), Some(kind), &spec, &drive)?;
            out.notes.splice(0..0, notes);
            Ok(out)
        }
        _ => train_on(shafts, ask, style, helix, base, notes),
    }
}

/// An angle between two lines, in degrees, as the acute one.
fn fold(deg: f64) -> f64 {
    if deg > 90.0 { 180.0 - deg } else { deg }
}

/// Three or more parallel shafts: a gear train (see the module docs).
fn train_on(shafts: &[Shaft], ask: &Ask, style: Option<Kind>, helix: f64, base: &Spec, mut notes: Vec<String>) -> Result<Solved, String> {
    if let Some(k) = ask.kind.filter(|k| !is_style(*k)) {
        return Err(format!(
            "three or more shafts make a gear train, whose type is its teeth (spur, helical or herringbone), not a {}",
            k.label().to_lowercase()
        ));
    }
    let a1 = shafts[0].axis;
    for (k, s) in shafts.iter().enumerate().skip(1) {
        let rel = axes::relate(&a1, &s.axis);
        if !matches!(rel, Relation::Parallel { .. } | Relation::Coincident) {
            return Err(format!(
                "shafts 1 and {} aren't parallel ({}): three or more shafts make a gear train, on parallel shafts",
                k + 1,
                rel.describe()
            ));
        }
    }
    // The train's plane: square to the shafts through shaft 1's origin, moved `along` it.
    let z = a1.dir;
    let x = axes::square_to(z);
    let origin = bearing_point(&a1, ask.along);
    let frame = Frame { origin, x, z };
    let y = frame.y();
    let centres: Vec<V2> = shafts
        .iter()
        .map(|s| {
            let q = [0, 1, 2].map(|i| s.axis.origin[i] - origin[i]);
            V2::new(dot(q, x), dot(q, y))
        })
        .collect();
    // Each gear's radius: as given (or teeth at the module), the rest filling the gap to the
    // gear before it, then (before the first given) to the gear after it.
    let cb = helix.to_radians().cos();
    let mut r: Vec<Option<f64>> =
        shafts.iter().map(|s| s.pitch_diameter.or_else(|| Some(ask.module? * f64::from(s.teeth?) / cb)).map(|d| d / 2.0)).collect();
    if r.iter().all(Option::is_none) {
        return Err(
            "give at least one shaft's pitch diameter (or its teeth with a module): the rest fill the spacing between the shafts in order".into()
        );
    }
    let n = r.len();
    for i in 1..n {
        if r[i].is_none()
            && let Some(prev) = r[i - 1]
        {
            r[i] = Some(centres[i].dist(centres[i - 1]) - prev);
        }
    }
    for i in (0..n - 1).rev() {
        if r[i].is_none()
            && let Some(next) = r[i + 1]
        {
            r[i] = Some(centres[i].dist(centres[i + 1]) - next);
        }
    }
    let mut circles = Vec::with_capacity(n);
    for (i, (c, r)) in centres.iter().zip(&r).enumerate() {
        let r = r.unwrap_or(0.0);
        if !(r > 0.0) {
            return Err(format!(
                "gear {} has no room: the gear before it reaches past its shaft; give its pitch diameter, or move the shafts apart",
                i + 1
            ));
        }
        if shafts[i].pitch_diameter.is_none() {
            notes.push(format!("gear {}: Ø{} fills the spacing to its neighbour", i + 1, num(2.0 * r)));
        }
        circles.push(Circle { c: *c, r });
    }
    let style = style.unwrap_or(Kind::Spur);
    let settings = crate::simple::Simple {
        module: ask.module,
        face_width: ask.face_width,
        helix: (helix != 0.0).then_some(helix),
        left_hand: Some(ask.left_hand),
        ..Default::default()
    };
    let (mut spec, more) = train_spec(&circles, style, &settings, base)?;
    notes.extend(more);
    spec.origin = [0.0; 3];
    spec.placement = Some(frame);
    let facts: Vec<Fact> = (1..n).map(|i| Fact { label: format!("shafts 1–{} distance", i + 1), value: centres[i].len(), unit: Unit::Mm }).collect();
    let gear1 = frame.compose(&Frame::at([centres[0].x, centres[0].y, 0.0]));
    Ok(Solved { spec, relation: None, frames: [gear1; 2], facts, notes, derived: Default::default() })
}
