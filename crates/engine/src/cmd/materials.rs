//! Physical materials defined in a design (`material.create`, `material.edit`,
//! `material.delete`): a name, a density and optional visual and engineering records. They are assigned like the
//! built-in library's (`material.assign`), set masses (`component.properties`) and fill
//! configuration material cells. The design keeps them, so they are saved and undone like
//! features.

use serde_json::{Value, json};
use solidcraft_doc::appearance::{Look, library};
use solidcraft_doc::config::Column;
use solidcraft_doc::{DEFAULT_MATERIAL, MATERIALS, MAX_USER_MATERIALS, UserMaterial, valid_density};

use super::CommandSpec;
use super::inspect::{color_param, look_json};
use crate::params::{bad, str_};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("material.create", "New Physical Material", create).icon("material").params(
        "name (not a built-in material's); density_g_cm3: (0, 1000]; appearance?: library name (appearance.library) | color?: \"#rrggbb\" or [r, g, b] (0–255), opacity? (0–1), pbr?: {metallic:0..1, roughness:0.045..1, dielectric_ior:1..3} (opaque only, needs color/appearance): the default look. A named appearance keeps its preset response when pbr is omitted; pbr: null selects scalar shading. Optical edits do not change density. physical_card?: validated SI candidate record (null/omitted means unknown); card density never changes assigned density. Assign it with material.assign",
    ),
    CommandSpec::new("material.edit", "Edit Physical Material", edit).icon("material").enabled(has_user_materials).params(
        "material: a material defined in this design; name? (bodies and configuration cells follow); density_g_cm3?; appearance? | color?, opacity?, pbr? (as material.create; color: null clears the look). physical_card?: validated SI candidate record (omitted retains, null clears). Omitted appearance/color retains the entire current look",
    ),
    CommandSpec::new("material.inspect", "Inspect Physical Material", inspect).icon("material").noundo().doc_only().params(
        "material: built-in or design-owned name. Returns assigned density, default appearance, optional physical_card, validation and density comparison. Read-only; missing engineering data remains unknown and card density does not change mass",
    ),
    CommandSpec::new("material.delete", "Delete Physical Material", delete).icon("delete").enabled(has_user_materials).params(
        "material: a material defined in this design; replace?: material for the bodies and configuration cells that use it (without it, a material in use is refused)",
    ),
];

#[cfg(test)]
#[path = "material_card_persistence_tests.rs"]
mod persistence_tests;

fn has_user_materials(s: &Session) -> std::result::Result<(), String> {
    if s.doc.user_materials.is_empty() { Err("the design defines no physical materials (material.create)".into()) } else { Ok(()) }
}

/// A new material name: 1–128 characters, not a built-in material's or another design
/// material's (`except`: the one being renamed).
fn new_name(s: &Session, cmd: &str, name: &str, except: Option<&str>) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 128 {
        return Err(bad(cmd, "`name` must have 1–128 characters"));
    }
    if MATERIALS.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)) {
        return Err(bad(cmd, format!("`{name}` is a built-in material (material.list)")));
    }
    let taken = |m: &&UserMaterial| m.name.eq_ignore_ascii_case(name) && except.is_none_or(|e| !m.name.eq_ignore_ascii_case(e));
    if s.doc.user_materials.iter().any(|m| taken(&m)) {
        return Err(bad(cmd, format!("the design already has a material `{name}`")));
    }
    Ok(name.to_string())
}

fn density(cmd: &str, p: &Value) -> Result<f64> {
    p.get("density_g_cm3")
        .and_then(Value::as_f64)
        .filter(|d| valid_density(*d))
        .ok_or_else(|| bad(cmd, "`density_g_cm3` must be a number above 0 and at most 1000"))
}

/// The look `appearance` / `color` / `opacity` give: `None` when none is given, `Some(None)`
/// to clear it (`color: null`).
fn look(cmd: &str, p: &Value, material: &str) -> Result<Option<Option<Look>>> {
    if p.get("pbr").is_some() && str_(p, "appearance").is_none() && p.get("color").is_none_or(Value::is_null) {
        return Err(bad(cmd, "`pbr` requires an appearance or color"));
    }
    let opacity = match p.get("opacity") {
        Some(o) => Some(o.as_f64().filter(|x| x.is_finite() && (0.0..=1.0).contains(x)).ok_or_else(|| bad(cmd, "`opacity` must be 0…1"))?),
        None => None,
    };
    let mut l = match (str_(p, "appearance"), p.get("color")) {
        (Some(_), Some(c)) if !c.is_null() => return Err(bad(cmd, "give `appearance` or `color`, not both")),
        (Some(n), _) => library(n).ok_or_else(|| bad(cmd, format!("no appearance `{n}` in the library (appearance.library)")))?,
        (None, Some(Value::Null)) => return Ok(Some(None)),
        (None, Some(c)) => {
            let mut l = Look::custom(color_param(c).ok_or_else(|| bad(cmd, "`color` must be \"#rrggbb\" or [r, g, b] with values 0–255"))?, 1.0);
            l.name = material.to_string();
            l
        }
        (None, None) if opacity.is_some() => return Err(bad(cmd, "`opacity` needs `appearance` or `color`")),
        (None, None) => return Ok(None),
    };
    if let Some(o) = opacity {
        l.opacity = o;
    }
    super::inspect::attach_pbr(cmd, p, &mut l)?;
    Ok(Some(Some(l)))
}

pub(super) fn material_json(m: &UserMaterial) -> Result<Value> {
    let mut out = json!({"name": m.name, "density_g_cm3": m.density, "appearance": m.appearance.as_ref().map(look_json), "user": true});
    if let Some(record) = &m.physical_card {
        let card = record.card();
        out["physical_card"] = json!(record);
        out["physical_validation"] = json!(card.validate()?);
        let measured = card.properties.iter().find_map(|p| match p.data {
            solidcraft_doc::physical_material::PropertyData::Density { kg_m3 } => Some(kg_m3),
            _ => None,
        });
        if let Some(measured) = measured {
            let assigned = m.density * 1000.0;
            let relative_difference = (assigned - measured).abs() / assigned.max(measured);
            out["density_comparison"] = json!({"assigned_density_kg_m3":assigned,"card_density_kg_m3":measured,
                "relative_difference":relative_difference,"matches_within_relative_tolerance":relative_difference <= 1e-9,
                "relative_tolerance":1e-9});
        }
    }
    Ok(out)
}

fn physical_card(cmd: &str, p: &Value) -> Result<Option<Option<solidcraft_doc::physical_material::ValidatedCard>>> {
    match p.get("physical_card") {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(value) => {
            solidcraft_doc::physical_material::ValidatedCard::from_value(value).map(|card| Some(Some(card))).map_err(|e| bad(cmd, e.to_string()))
        }
    }
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "material.inspect";
    let name = str_(p, "material").ok_or_else(|| bad(cmd, "`material` must be a material name"))?;
    let canonical = s.doc.material_name(name).ok_or_else(|| bad(cmd, format!("unknown material `{name}` (see material.list)")))?;
    let mut out = if let Some(material) = s.doc.user_materials.iter().find(|m| m.name == canonical) {
        material_json(material)?
    } else {
        let density = MATERIALS
            .iter()
            .find(|(n, _)| *n == canonical.as_str())
            .map(|(_, density)| *density)
            .ok_or_else(|| bad(cmd, "material is unavailable"))?;
        json!({"name":canonical,"density_g_cm3":density,"user":false})
    };
    out["appearance"] = json!(s.doc.material_look(&canonical).as_ref().map(look_json));
    if out.get("physical_card").is_none() {
        out["physical_card"] = Value::Null;
    }
    Ok(out)
}

/// The index of the design material `material` names.
fn existing(s: &Session, cmd: &str, p: &Value) -> Result<usize> {
    let name = str_(p, "material").ok_or_else(|| bad(cmd, "`material` must be a material name"))?;
    s.doc
        .user_materials
        .iter()
        .position(|m| m.name.eq_ignore_ascii_case(name.trim()))
        .ok_or_else(|| bad(cmd, format!("the design defines no material `{name}` (built-in materials cannot be changed)")))
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "material.create";
    let name = new_name(s, cmd, str_(p, "name").ok_or_else(|| bad(cmd, "`name` is required"))?, None)?;
    if s.doc.user_materials.len() >= MAX_USER_MATERIALS {
        return Err(bad(cmd, format!("a design defines at most {MAX_USER_MATERIALS} materials")));
    }
    let material =
        UserMaterial { density: density(cmd, p)?, appearance: look(cmd, p, &name)?.flatten(), name, physical_card: physical_card(cmd, p)?.flatten() };
    let out = material_json(&material)?;
    s.doc_mut().user_materials.push(material);
    Ok(out)
}

/// Which configuration columns hold materials.
fn material_columns(s: &Session) -> Vec<bool> {
    s.doc.configs.columns.iter().map(|c| matches!(c, Column::Material(_))).collect()
}

/// Where material `name` is used: bodies, and configuration rows with a cell naming it.
fn uses(s: &Session, name: &str) -> (Vec<String>, Vec<String>) {
    let bodies = s.doc.materials.iter().filter(|(_, m)| m.eq_ignore_ascii_case(name)).map(|(b, _)| b.clone()).collect();
    let columns = material_columns(s);
    let rows = s
        .doc
        .configs
        .rows
        .iter()
        .filter(|r| r.cells.iter().zip(&columns).any(|(cell, material)| *material && cell.trim().eq_ignore_ascii_case(name)))
        .map(|r| r.name.clone())
        .collect();
    (bodies, rows)
}

/// Point every use of material `from` (body assignments, configuration cells) at `to`
/// ([`DEFAULT_MATERIAL`] removes the body assignments).
fn move_uses(s: &mut Session, from: &str, to: &str) {
    let columns = material_columns(s);
    let d = s.doc_mut();
    let bodies: Vec<String> = d.materials.iter().filter(|(_, m)| m.eq_ignore_ascii_case(from)).map(|(b, _)| b.clone()).collect();
    for b in bodies {
        if to == DEFAULT_MATERIAL {
            d.materials.remove(&b);
        } else {
            d.materials.insert(b, to.to_string());
        }
    }
    for row in &mut d.configs.rows {
        for (cell, material) in row.cells.iter_mut().zip(&columns) {
            if *material && cell.trim().eq_ignore_ascii_case(from) {
                *cell = to.to_string();
            }
        }
    }
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "material.edit";
    let i = existing(s, cmd, p)?;
    let old = s.doc.user_materials.get(i).cloned().ok_or_else(|| bad(cmd, "no such material"))?;
    let name = match str_(p, "name") {
        Some(n) => new_name(s, cmd, n, Some(&old.name))?,
        None => old.name.clone(),
    };
    let density = if p.get("density_g_cm3").is_some() { density(cmd, p)? } else { old.density };
    let appearance = match look(cmd, p, &name)? {
        Some(l) => l,
        // A colour named after the material keeps following its name.
        None => old.appearance.clone().map(|mut l| {
            if l.name == old.name {
                l.name = name.clone();
            }
            l
        }),
    };
    let physical_card = physical_card(cmd, p)?.unwrap_or(old.physical_card);
    let material = UserMaterial { name: name.clone(), density, appearance, physical_card };
    let mut out = material_json(&material)?;
    if let Some(m) = s.doc_mut().user_materials.get_mut(i) {
        *m = material;
    }
    if name != old.name {
        move_uses(s, &old.name, &name);
    }
    out["bodies"] = json!(uses(s, &name).0);
    Ok(out)
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "material.delete";
    let i = existing(s, cmd, p)?;
    let name = s.doc.user_materials.get(i).map(|m| m.name.clone()).ok_or_else(|| bad(cmd, "no such material"))?;
    let (bodies, rows) = uses(s, &name);
    let replace = match str_(p, "replace") {
        Some(r) => {
            let r = s.doc.material_name(r).ok_or_else(|| bad(cmd, format!("unknown material `{r}` (see material.list)")))?;
            if r.eq_ignore_ascii_case(&name) {
                return Err(bad(cmd, "`replace` must be another material"));
            }
            Some(r)
        }
        None if bodies.is_empty() && rows.is_empty() => None,
        None => {
            let mut used: Vec<String> =
                bodies.iter().map(|b| format!("body `{b}`")).chain(rows.iter().map(|r| format!("configuration `{r}`"))).collect();
            if used.len() > 10 {
                let more = used.len() - 10;
                used.truncate(10);
                used.push(format!("{more} more"));
            }
            return Err(bad(cmd, format!("`{name}` is used by {}: give `replace` (a material for them)", used.join(", "))));
        }
    };
    if let Some(r) = &replace {
        move_uses(s, &name, r);
    }
    s.doc_mut().user_materials.remove(i);
    Ok(json!({"deleted": name, "replaced_by": replace, "bodies": bodies}))
}
