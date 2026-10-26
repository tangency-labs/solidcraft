//! Item numbers (#3874). A view of an assembly is numbered once someone numbers it
//! (`assembly.bom.renumber`, `assembly.bom.item.set`): from then on its rows' numbers are saved in
//! the design ([`ItemNumbers`], `assembly.item_numbers`), each under its row's key, so a row keeps
//! its number through part-number edits. Until then a view's numbers are its rows' positions,
//! provisional, and listing it saves nothing.
//!
//! A row a numbered view has no number for (a part added since) shows the highest number saved at
//! its level plus one, provisional, and [`record`] saves it: the engine runs it after each change,
//! in the change's undo step. A row that goes keeps its number until a renumber, so a number on a
//! printed drawing never comes to mean another part, and a part put back gets its number again.
//! A renumber keeps locked numbers and skips them.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{Group, Member, Request, Row, RowKey, Tree, View, item_text, list, list_order};
use crate::{DocError, Document, MAX_PLACEMENTS, Result};

/// The highest item number.
pub const MAX_ITEM: u32 = 999_999;

/// The item numbers saved for one view of one assembly, with the list options it was numbered
/// with: the rows it numbers are listed by them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemNumbers {
    /// The assembly: a component, or 0 for the design.
    pub source: u64,
    #[serde(rename = "levels")]
    pub view: View,
    #[serde(default, rename = "group_by")]
    pub group: Group,
    #[serde(default, rename = "group_children", skip_serializing_if = "std::ops::Not::not")]
    pub fold: bool,
    #[serde(default, rename = "show_purchased_contents", skip_serializing_if = "std::ops::Not::not")]
    pub purchased_contents: bool,
    /// One for each row numbered, rows gone since included, by key.
    #[serde(default)]
    pub items: Vec<ItemNumber>,
}

/// One row's saved item number.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemNumber {
    #[serde(flatten)]
    pub(crate) key: RowKey,
    pub number: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
}

impl ItemNumbers {
    /// The list it numbers.
    pub fn request(&self) -> Request {
        Request { source: self.source, view: self.view, group: self.group, fold: self.fold, mass: false, purchased_contents: self.purchased_contents }
    }
}

/// How [`number`] numbers a view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Numbering {
    /// Each row without a saved number keeps the one it shows: the next at its level. Only a
    /// numbered view has any to save.
    NewRows,
    /// Every row from `start` by `step`, level by level, in `order`. Locked rows keep their
    /// numbers, which are skipped, and rows gone are forgotten.
    Renumber { start: u32, step: u32, order: Order },
    /// One row, by its item as the view shows it now (`[1, 2]` is 1.2): its number and its lock.
    /// Every other row keeps the number it shows.
    Set { row: Vec<u32>, item: Option<u32>, locked: Option<bool> },
}

/// The order a renumber goes in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Order {
    /// The list's own, by part number (`list_order`).
    #[default]
    List,
    /// The item numbers', closing the gaps.
    Item,
}

/// The numbering saved for a view of an assembly, if it has been numbered.
pub(super) fn saved(doc: &Document, source: u64, view: View) -> Option<&ItemNumbers> {
    doc.assembly.item_numbers.iter().find(|n| n.source == source && n.view == view)
}

/// `rows`, as their view lists them, numbered as `saved` says: each its saved number, else the next
/// at its level (provisional); then each level by item number, with duplicates marked. With no
/// numbering they stay as listed, every number provisional. Whether the view is numbered.
pub(super) fn apply(rows: &mut [Row], saved: Option<&ItemNumbers>) -> bool {
    let Some(saved) = saved else { return false };
    // A damaged file's number out of range is no number, and of a key saved twice the first holds.
    let mut numbers: HashMap<&RowKey, &ItemNumber> = HashMap::new();
    let mut highest: HashMap<Vec<Member>, u32> = HashMap::new();
    for item in saved.items.iter().filter(|i| (1..=MAX_ITEM).contains(&i.number)) {
        if numbers.contains_key(&item.key) {
            continue;
        }
        numbers.insert(&item.key, item);
        let h = highest.entry(item.key.parents.clone()).or_default();
        *h = (*h).max(item.number);
    }
    number_level(rows, &numbers, &mut highest);
    true
}

/// One level of [`apply`], and the levels under it. `highest` is the highest number at each level
/// (by its parents), saved or shown.
fn number_level(rows: &mut [Row], numbers: &HashMap<&RowKey, &ItemNumber>, highest: &mut HashMap<Vec<Member>, u32>) {
    for row in rows.iter_mut() {
        if let Some(saved) = numbers.get(&row.key) {
            row.item = saved.number;
            row.locked = saved.locked;
            row.provisional = false;
        } else {
            let h = highest.entry(row.key.parents.clone()).or_default();
            *h = h.saturating_add(1);
            row.item = *h;
        }
        number_level(&mut row.children, numbers, highest);
    }
    rows.sort_by_key(|r| r.item);
    let mut uses: HashMap<u32, usize> = HashMap::new();
    for row in rows.iter() {
        *uses.entry(row.item).or_default() += 1;
    }
    for row in rows.iter_mut() {
        row.duplicate = uses.get(&row.item).is_some_and(|&n| n > 1);
    }
}

/// The view `request` asks for, numbered as `how` says: the numbering to save, or `None` when that
/// changes nothing.
pub fn number(tree: &mut Tree, request: &Request, how: Numbering) -> Result<Option<ItemNumbers>> {
    let request = Request { mass: false, ..*request };
    // Borrowed from the design, not the tree: listing needs the tree, and nothing is copied until
    // something changes.
    let doc = tree.doc;
    let old = saved(doc, request.source, request.view);
    let mut bom = list(tree, &request)?;
    let mut items: Vec<ItemNumber> = Vec::new();
    match how {
        Numbering::NewRows => {
            let Some(old) = old else { return Ok(None) };
            let room = MAX_PLACEMENTS.saturating_sub(old.items.len());
            let shown = bom.rows_deep();
            let new: Vec<ItemNumber> = shown
                .iter()
                .filter(|(_, r)| r.provisional && r.item <= MAX_ITEM)
                .take(room)
                .map(|(_, r)| ItemNumber { key: r.key.clone(), number: r.item, locked: false })
                .collect();
            if new.is_empty() {
                return Ok(None);
            }
            items = [old.items.as_slice(), &new].concat();
        }
        Numbering::Set { row, item, locked } => {
            if let Some(n) = item.filter(|n| !(1..=MAX_ITEM).contains(n)) {
                return Err(DocError::Invalid(format!("an item number is 1 to {MAX_ITEM}, not {n}")));
            }
            let shown = bom.rows_deep();
            let target = shown
                .iter()
                .find(|(i, _)| *i == row)
                .map(|(_, r)| r.key.clone())
                .ok_or_else(|| DocError::Unknown(format!("item {}", item_text(&row))))?;
            // Rows not listed now keep what was saved for them; listed ones the number they show.
            let listed: HashSet<&RowKey> = shown.iter().map(|(_, r)| &r.key).collect();
            items.extend(old.iter().flat_map(|o| &o.items).filter(|i| !listed.contains(&i.key)).cloned());
            items.extend(shown.iter().map(|(_, r)| ItemNumber { key: r.key.clone(), number: r.item, locked: r.locked }));
            if let Some(it) = items.iter_mut().find(|i| i.key == target) {
                it.number = item.unwrap_or(it.number);
                it.locked = locked.unwrap_or(it.locked);
            }
        }
        Numbering::Renumber { start, step, order } => {
            for (name, n) in [("start", start), ("step", step)] {
                if !(1..=MAX_ITEM).contains(&n) {
                    return Err(DocError::Invalid(format!("`{name}` is 1 to {MAX_ITEM}, not {n}")));
                }
            }
            renumber(&mut bom.rows, start, step, order, &mut items)?;
        }
    }
    items.sort_by(|a, b| a.key.cmp(&b.key));
    let new = ItemNumbers {
        source: request.source,
        view: request.view,
        group: request.group,
        fold: request.fold,
        purchased_contents: request.purchased_contents,
        items,
    };
    Ok((old != Some(&new)).then_some(new))
}

/// `rows` and the levels under them numbered from `start` by `step` in `order`, each level on its
/// own, into `out`. Locked rows keep their numbers and the others skip them, so no two rows get one
/// number.
fn renumber(rows: &mut [Row], start: u32, step: u32, order: Order, out: &mut Vec<ItemNumber>) -> Result<()> {
    if order == Order::List {
        rows.sort_by(list_order);
    }
    let locked: HashSet<u32> = rows.iter().filter(|r| r.locked).map(|r| r.item).collect();
    let mut next = start;
    for row in rows.iter_mut() {
        if !row.locked {
            while locked.contains(&next) {
                next = next.saturating_add(step);
            }
            if next > MAX_ITEM {
                return Err(DocError::Invalid(format!("numbering from {start} by {step} passes {MAX_ITEM}")));
            }
            row.item = next;
            next = next.saturating_add(step);
        }
        out.push(ItemNumber { key: row.key.clone(), number: row.item, locked: row.locked });
        renumber(&mut row.children, start, step, order, out)?;
    }
    Ok(())
}

/// Each numbered view of the design with its new rows' numbers saved ([`Numbering::NewRows`]), and
/// a component's views dropped once it is gone: the design's numberings to save, or `None` when
/// none changed. A view that can't be listed now (too large) is left as it is. A view numbered
/// twice (only a damaged file is) keeps its first numbering, the one listing reads.
pub fn record(tree: &mut Tree) -> Option<Vec<ItemNumbers>> {
    let doc = tree.doc;
    // Each numbering that changes: its new self, or none when its component is gone or its view
    // already has one, so no view is listed twice after a change.
    let mut changes: Vec<(usize, Option<ItemNumbers>)> = Vec::new();
    let mut views = HashSet::new();
    for (i, numbering) in doc.assembly.item_numbers.iter().enumerate() {
        let gone = numbering.source != 0 && !tree.components.contains_key(&numbering.source);
        if gone || !views.insert((numbering.source, numbering.view)) {
            changes.push((i, None));
        } else if let Ok(Some(new)) = number(tree, &numbering.request(), Numbering::NewRows) {
            changes.push((i, Some(new)));
        }
    }
    if changes.is_empty() {
        return None;
    }
    let mut out = doc.assembly.item_numbers.clone();
    for (i, change) in changes.into_iter().rev() {
        match change {
            Some(new) => {
                if let Some(numbering) = out.get_mut(i) {
                    *numbering = new;
                }
            }
            None => {
                out.remove(i);
            }
        }
    }
    Some(out)
}
