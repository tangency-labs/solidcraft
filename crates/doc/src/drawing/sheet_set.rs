//! Sheet sets and output scopes (W39a, #9723): which of a drawing's sheets an output covers.
//!
//! A [`SheetSet`] is a named selection of a drawing's sheets, kept so the same few sheets (the
//! customer's, the workshop's) can be printed or exported again by name. This module holds the
//! rules only: the sets are not saved in the drawing yet, and no command reaches them.
//!
//! - **A set selects; it does not order.** Its sheets come out in the drawing's order, so
//!   reordering the drawing's sheets reorders every set's output, and nothing has to be kept in
//!   step. A set keeps its sheets' ids in id order, so the same set is always the same data.
//! - **Ids and names** follow the drawing's rules: each set has an id of [`crate::ids`], one no
//!   other set and no object of the drawing has; its name is 1 to [`super::MAX_NAME_BYTES`]
//!   bytes, not only spaces, without control characters. Two sets may share a name, as two sheets
//!   may (two people adding a "Workshop" set side by side never make a damaged drawing); a set is
//!   named by its id, else by a name only one set has ([`SheetSets::index`]), as a sheet is.
//! - **Sizes**: at most [`MAX_SHEET_SETS`] sets, each of 1 to [`super::MAX_SHEETS`] sheets, none
//!   twice, every one a sheet of the drawing. The sets are listed by id.
//! - **Deleting a sheet** reports the sets that hold it ([`SheetSets::holding`]) and then drops it
//!   from each ([`SheetSets::without_sheet`]); a set left with no sheets goes too.
//! - **An output's scope** ([`Scope`]) resolves to sheet positions in the drawing's order
//!   ([`SheetSets::resolve`]): every sheet not left out of printing, or the sheets named, alone,
//!   in a list or through a set. A sheet named outright comes out even when it is left out of
//!   printing every sheet: asking for it by name is the exception. A scope of no sheets is refused.

use std::collections::BTreeSet;

use super::{Drawing, check_name, require};
use crate::ids::{self, IdSource};
use crate::{DocError, Result};

/// The most sheet sets a drawing may have.
pub const MAX_SHEET_SETS: usize = 256;

/// A named selection of a drawing's sheets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetSet {
    pub id: u64,
    pub name: String,
    /// The sheets' ids, in id order, none twice.
    sheets: Vec<u64>,
}

impl SheetSet {
    /// The sheets it selects, by id, in id order (not the order they come out in: see
    /// [`SheetSets::resolve`]).
    pub fn sheets(&self) -> &[u64] {
        &self.sheets
    }
}

/// A drawing's sheet sets, by id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SheetSets {
    sets: Vec<SheetSet>,
}

/// What an output covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope<'a> {
    /// Every sheet not left out of printing ([`super::Excluded::print`]).
    Printed,
    /// One sheet, by id or by a name only one sheet has.
    Sheet(&'a str),
    /// These sheets, each by id or by a name only one sheet has, none twice.
    Sheets(Vec<&'a str>),
    /// A set's sheets, the set by id or by a name only one set has.
    Set(&'a str),
}

/// What dropping a sheet from the sets changed ([`SheetSets::without_sheet`]), each list by set id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SheetsRemoved {
    /// Sets that held the sheet and still hold others.
    pub shrunk: Vec<u64>,
    /// Sets that held only the sheet, and went with it.
    pub emptied: Vec<u64>,
}

impl SheetSets {
    /// The sets, by id.
    pub fn sets(&self) -> &[SheetSet] {
        &self.sets
    }

    pub fn len(&self) -> usize {
        self.sets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }

    /// The position of the set `named` names: by id, else by a name only one set has.
    pub fn index(&self, named: &str) -> Result<usize> {
        if let Some(i) = ids::decode(named).and_then(|id| self.sets.iter().position(|s| s.id == id)) {
            return Ok(i);
        }
        let mut called = self.sets.iter().enumerate().filter(|(_, s)| s.name == named).map(|(i, _)| i);
        match (called.next(), called.next()) {
            (Some(i), None) => Ok(i),
            (Some(_), Some(_)) => Err(DocError::Invalid(format!("more than one sheet set is called `{named}`: give its id"))),
            (None, _) => Err(DocError::Invalid(format!("no sheet set `{named}`"))),
        }
    }

    /// The set `named` names ([`SheetSets::index`]).
    pub fn get(&self, named: &str) -> Result<&SheetSet> {
        let i = self.index(named)?;
        self.sets.get(i).ok_or_else(|| DocError::Invalid(format!("no sheet set `{named}`")))
    }

    /// Adds a set named `name` of the sheets `sheets` names (each by id or by a name only one sheet
    /// of `drawing` has), and answers its id: one from `id_source` that no set and no object of
    /// `drawing` has.
    pub fn add(&mut self, drawing: &Drawing, name: &str, sheets: &[&str], id_source: &mut IdSource) -> Result<u64> {
        require(self.sets.len() < MAX_SHEET_SETS, || format!("a drawing has at most {MAX_SHEET_SETS} sheet sets"))?;
        check_name(name, "a sheet set's")?;
        let sheets = sheet_ids(drawing, sheets)?;
        let taken: BTreeSet<u64> = drawing.ids().chain(self.sets.iter().map(|s| s.id)).collect();
        let id = id_source.next(&taken)?;
        let at = self.sets.partition_point(|s| s.id < id);
        self.sets.insert(at, SheetSet { id, name: name.into(), sheets });
        Ok(id)
    }

    /// Renames the set `named` names to `name`. False when it already had that name.
    pub fn rename(&mut self, named: &str, name: &str) -> Result<bool> {
        check_name(name, "a sheet set's")?;
        let set = self.get_mut(named)?;
        if set.name == name {
            return Ok(false);
        }
        set.name = name.into();
        Ok(true)
    }

    /// Makes the set `named` names select the sheets `sheets` names, as [`SheetSets::add`] takes
    /// them. False when it already selected those.
    pub fn set_sheets(&mut self, drawing: &Drawing, named: &str, sheets: &[&str]) -> Result<bool> {
        let sheets = sheet_ids(drawing, sheets)?;
        let set = self.get_mut(named)?;
        if set.sheets == sheets {
            return Ok(false);
        }
        set.sheets = sheets;
        Ok(true)
    }

    /// Takes out the set `named` names, and answers it.
    pub fn remove(&mut self, named: &str) -> Result<SheetSet> {
        let i = self.index(named)?;
        require(i < self.sets.len(), || format!("no sheet set `{named}`"))?;
        Ok(self.sets.remove(i))
    }

    /// The sets that hold sheet `sheet`, by id: what deleting the sheet changes, to report before it
    /// is deleted.
    pub fn holding(&self, sheet: u64) -> Vec<u64> {
        self.sets.iter().filter(|s| s.sheets.binary_search(&sheet).is_ok()).map(|s| s.id).collect()
    }

    /// Drops sheet `sheet` from every set, once the drawing no longer has it; a set left with no
    /// sheets goes too. Answers which sets changed and how.
    pub fn without_sheet(&mut self, sheet: u64) -> SheetsRemoved {
        let mut removed = SheetsRemoved::default();
        self.sets.retain_mut(|set| {
            let Ok(at) = set.sheets.binary_search(&sheet) else {
                return true;
            };
            set.sheets.remove(at);
            if set.sheets.is_empty() {
                removed.emptied.push(set.id);
                false
            } else {
                removed.shrunk.push(set.id);
                true
            }
        });
        removed
    }

    /// The positions in `drawing`'s sheets of the sheets `scope` covers, in the drawing's order.
    /// Refused when it covers none, or names a sheet or a set there isn't.
    pub fn resolve(&self, drawing: &Drawing, scope: &Scope) -> Result<Vec<usize>> {
        let positions: Vec<usize> = match scope {
            Scope::Printed => drawing.sheets.iter().enumerate().filter(|(_, s)| !s.excluded.print).map(|(i, _)| i).collect(),
            Scope::Sheet(named) => vec![drawing.sheet_index(named)?],
            Scope::Sheets(named) => in_order(drawing, &sheet_ids(drawing, named)?),
            Scope::Set(named) => {
                let set = self.get(named)?;
                let missing = set.sheets.iter().find(|id| !drawing.sheets.iter().any(|s| s.id == **id));
                if let Some(id) = missing {
                    return Err(DocError::Invalid(format!(
                        "sheet set `{}` holds sheet {}, which the drawing doesn't have",
                        set.name,
                        ids::encode(*id)
                    )));
                }
                in_order(drawing, &set.sheets)
            }
        };
        require(!positions.is_empty(), || match scope {
            Scope::Printed => "every sheet is left out of printing: name the sheets to print".into(),
            _ => "no sheets to print".into(),
        })?;
        Ok(positions)
    }

    /// The rules (the module's notes), against the drawing the sets are of: what reading them will
    /// check, and what every edit keeps.
    pub fn validate(&self, drawing: &Drawing) -> Result<()> {
        require(self.sets.len() <= MAX_SHEET_SETS, || format!("a drawing has at most {MAX_SHEET_SETS} sheet sets"))?;
        require(self.sets.is_sorted_by_key(|s| s.id), || "the sheet sets are not listed by id".into())?;
        let drawing_ids: BTreeSet<u64> = drawing.ids().collect();
        let sheets: BTreeSet<u64> = drawing.sheets.iter().map(|s| s.id).collect();
        let mut seen = BTreeSet::new();
        for set in &self.sets {
            let id = ids::encode(set.id);
            require(set.id != 0, || "a sheet set has the id 0, which is never one".into())?;
            require(seen.insert(set.id), || format!("two sheet sets have the id {id}"))?;
            require(!drawing_ids.contains(&set.id), || format!("sheet set {id} has the id of an object of the drawing"))?;
            check_name(&set.name, &format!("sheet set {id}'s"))?;
            require((1..=super::MAX_SHEETS).contains(&set.sheets.len()), || format!("sheet set {id} holds 1 to {} sheets", super::MAX_SHEETS))?;
            require(set.sheets.is_sorted_by(|a, b| a < b), || format!("sheet set {id}'s sheets are not listed by id, once each"))?;
            if let Some(gone) = set.sheets.iter().find(|s| !sheets.contains(s)) {
                return Err(DocError::Invalid(format!("sheet set {id} holds sheet {}, which the drawing doesn't have", ids::encode(*gone))));
            }
        }
        Ok(())
    }

    fn get_mut(&mut self, named: &str) -> Result<&mut SheetSet> {
        let i = self.index(named)?;
        self.sets.get_mut(i).ok_or_else(|| DocError::Invalid(format!("no sheet set `{named}`")))
    }
}

/// The ids of the sheets `named` names (each by id or by a name only one sheet has), in id order:
/// 1 to [`super::MAX_SHEETS`] of them, none twice.
fn sheet_ids(drawing: &Drawing, named: &[&str]) -> Result<Vec<u64>> {
    let max = super::MAX_SHEETS;
    require((1..=max).contains(&named.len()), || format!("name 1 to {max} sheets"))?;
    let mut ids = BTreeSet::new();
    for n in named {
        let i = drawing.sheet_index(n)?;
        let sheet = drawing.sheets.get(i).ok_or_else(|| DocError::Invalid(format!("no sheet `{n}`")))?;
        require(ids.insert(sheet.id), || format!("sheet `{n}` is named twice"))?;
    }
    Ok(ids.into_iter().collect())
}

/// The positions of the sheets `ids` (in id order) in `drawing`'s order; sheets it doesn't have are
/// left out.
fn in_order(drawing: &Drawing, ids: &[u64]) -> Vec<usize> {
    drawing.sheets.iter().enumerate().filter(|(_, s)| ids.binary_search(&s.id).is_ok()).map(|(i, _)| i).collect()
}

#[cfg(test)]
#[path = "sheet_set_tests.rs"]
mod tests;
