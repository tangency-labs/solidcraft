//! The units of a representation context (ISO 10303-41 measures and units, ISO 10303-43
//! representation contexts): GLOBAL_UNIT_ASSIGNED_CONTEXT's length and plane angle units and
//! GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT's distance uncertainty. Missing or malformed units read
//! as millimetres and radians; nothing here fails.

use crate::p21::{Exchange, Param};
use crate::read::Units;

/// What a representation context says about its numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Context {
    pub units: Units,
    /// The context's distance uncertainty in millimetres (the last UNCERTAINTY_MEASURE_WITH_UNIT
    /// it lists), if any.
    pub uncertainty: Option<f64>,
}

impl Default for Context {
    fn default() -> Self {
        Context { units: Units::default(), uncertainty: None }
    }
}

fn si_prefix(p: Option<&Param>) -> f64 {
    match p.and_then(Param::as_enum) {
        Some("EXA") => 1e18,
        Some("PETA") => 1e15,
        Some("TERA") => 1e12,
        Some("GIGA") => 1e9,
        Some("MEGA") => 1e6,
        Some("KILO") => 1e3,
        Some("HECTO") => 1e2,
        Some("DECA") => 1e1,
        Some("DECI") => 1e-1,
        Some("CENTI") => 1e-2,
        Some("MILLI") => 1e-3,
        Some("MICRO") => 1e-6,
        Some("NANO") => 1e-9,
        Some("PICO") => 1e-12,
        Some("FEMTO") => 1e-15,
        Some("ATTO") => 1e-18,
        _ => 1.0,
    }
}

/// What a unit measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitKind {
    Length,
    Angle,
    Other,
}

/// A unit's kind and size: millimetres for lengths, radians for plane angles (1 for anything
/// else, or a unit that cannot be read).
pub fn unit_factor(ex: &Exchange, id: u64) -> Option<(UnitKind, f64)> {
    factor(ex, id, 0)
}

fn factor(ex: &Exchange, id: u64, depth: usize) -> Option<(UnitKind, f64)> {
    if depth > 4 {
        return None;
    }
    let e = ex.get(id)?;
    let kind = if e.has("LENGTH_UNIT") {
        UnitKind::Length
    } else if e.has("PLANE_ANGLE_UNIT") {
        UnitKind::Angle
    } else {
        UnitKind::Other
    };
    if let Some(si) = e.record("SI_UNIT") {
        let base = match si.get(1).and_then(Param::as_enum) {
            Some("METRE") => 1000.0,
            Some("RADIAN") => 1.0,
            _ => return Some((kind, 1.0)),
        };
        return Some((kind, si_prefix(si.first()) * base));
    }
    if let Some(cb) = e.record("CONVERSION_BASED_UNIT") {
        let m = ex.get(cb.get(1)?.as_ref_id()?)?;
        let mp = m.params();
        let v = mp.first()?.as_f64()?;
        let (_, f) = factor(ex, mp.get(1)?.as_ref_id()?, depth + 1)?;
        let x = v * f;
        return (x.is_finite() && x > 0.0).then_some((kind, x));
    }
    Some((kind, 1.0))
}

/// The units and uncertainty of the representation context `id`.
pub fn context(ex: &Exchange, id: u64) -> Context {
    let mut c = Context::default();
    let Some(e) = ex.get(id) else { return c };
    if let Some(units) = e.record("GLOBAL_UNIT_ASSIGNED_CONTEXT").and_then(|r| r.first()).and_then(Param::as_list) {
        for uid in units.iter().filter_map(Param::as_ref_id) {
            match unit_factor(ex, uid) {
                Some((UnitKind::Length, f)) if f.is_finite() && f > 0.0 => c.units.len = f,
                Some((UnitKind::Angle, f)) if f.is_finite() && f > 0.0 => c.units.ang = f,
                _ => {}
            }
        }
    }
    if let Some(unc) = e.record("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT").and_then(|r| r.first()).and_then(Param::as_list) {
        for uid in unc.iter().filter_map(Param::as_ref_id) {
            if let Some(m) = ex.get(uid)
                && let Some(v) = m.params().first().and_then(Param::as_f64)
            {
                let f = m.params().get(1).and_then(Param::as_ref_id).and_then(|x| unit_factor(ex, x)).map(|x| x.1);
                c.uncertainty = Some(v * f.unwrap_or(c.units.len)).filter(|t| t.is_finite() && *t > 0.0);
            }
        }
    }
    c
}
