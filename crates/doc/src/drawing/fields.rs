//! A title block's fields (#3875): the keys each source has, and what they read. Stateless: the
//! drawing gives its own fields, its sheets' and its newest revision's, and the caller hands in the
//! component a sheet documents with what the model says of it (its material and mass, worked out
//! by the bill of materials). A key its source doesn't have is refused when a block is given or
//! read; a value that isn't there, or is empty, shows [`MISSING`]. Sheet tables' cells (B3) read
//! the same fields.
//!
//! - **component:** `name`, `part_number`, `description`, `revision` (its part properties),
//!   `material`, `mass`, and `properties.<name>` (its free-text properties, such as a finish);
//! - **drawing:** `name`, `number` (its `part_number` property: the drawing's identification
//!   number), `organization`, `author`, `project`, and `properties.<key>` (any of its properties);
//! - **sheet:** `name`, `number`, `count`: sheets left out of the count are neither counted nor
//!   numbered, and such a sheet shows neither;
//! - **revision:** the newest revision row's fields ([`Field`]);
//! - **prompt:** any key; answers come with templates, so a prompt shows [`MISSING`] until then.

use solidcraft_geom::cell::{Content, MISSING, Part, Source};
use solidcraft_geom::revision::Field;
use solidcraft_geom::title_block::{CellKind, TitleBlock};

use super::Drawing;
use crate::{DocError, Metadata, Result};

const COMPONENT: [&str; 6] = ["name", "part_number", "description", "revision", "material", "mass"];
const DRAWING: [&str; 5] = ["name", "number", "organization", "author", "project"];
const SHEET: [&str; 3] = ["name", "number", "count"];
/// Before a property's name: the field reads that property.
const PROPERTIES: &str = "properties.";

/// The component a sheet documents, as its fields read it.
#[derive(Clone, Copy, Debug)]
pub struct Component<'a> {
    pub name: &'a str,
    /// Its part properties.
    pub metadata: &'a Metadata,
    /// Its free-text properties (the design's own: none).
    pub properties: Option<&'a std::collections::BTreeMap<String, String>>,
    /// What it is made of, as its bill of materials row would say: none when nothing is assigned.
    pub material: Option<&'a str>,
    /// What one of it weighs, written for the sheet: none when it can't be weighed.
    pub mass: Option<&'a str>,
}

/// Whether `source` has the field `key`.
pub fn has_key(source: Source, key: &str) -> bool {
    let property = key.strip_prefix(PROPERTIES).is_some_and(|name| !name.is_empty());
    match source {
        Source::Component => COMPONENT.contains(&key) || property,
        Source::Drawing => DRAWING.contains(&key) || property,
        Source::Sheet => SHEET.contains(&key),
        Source::Revision => Field::ALL.iter().any(|f| f.key() == key),
        Source::Prompt => !key.is_empty(),
        Source::Row => false,
    }
}

/// Whether `block` has a text cell `name`: what a sheet's override may name.
pub fn overridable(block: Option<&TitleBlock>, name: &str) -> bool {
    block.is_some_and(|block| block.cells.iter().any(|cell| cell.name == name && matches!(cell.kind, CellKind::Text(_))))
}

/// Each field of `block` names a key its source has: what a sheet holds its block to.
pub fn check_block(block: &TitleBlock) -> Result<()> {
    for cell in &block.cells {
        let CellKind::Text(text) = &cell.kind else { continue };
        for part in &text.content.0 {
            if let Part::Field { source, key, .. } = part
                && !has_key(*source, key)
            {
                let source = serde_json::to_value(source).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                return Err(DocError::Invalid(format!("cell `{}`: the {source} has no field `{key}`", cell.name)));
            }
        }
    }
    Ok(())
}

/// What a sheet's fields read: its drawing, its place among the drawing's sheets, and the component
/// it documents.
#[derive(Clone, Copy, Debug)]
pub struct Values<'a> {
    pub drawing: &'a Drawing,
    pub sheet: usize,
    pub component: Option<Component<'a>>,
}

impl Values<'_> {
    /// `content` as its cell shows it: text as written, each field its value or [`MISSING`].
    pub fn text(&self, content: &Content) -> String {
        let part = |part: &Part| match part {
            Part::Text(text) => text.clone(),
            Part::Field { source, key, .. } => self.field(*source, key).filter(|v| !v.is_empty()).unwrap_or_else(|| MISSING.into()),
        };
        content.0.iter().map(part).collect()
    }

    /// The field `key` of `source`, when it has a value.
    fn field(&self, source: Source, key: &str) -> Option<String> {
        let d = self.drawing;
        let property = |metadata: &Metadata| key.strip_prefix(PROPERTIES).and_then(|name| metadata.text(name)).map(str::to_string);
        match source {
            Source::Drawing => match key {
                "name" => Some(d.name.clone()),
                "number" => d.properties.part_number().map(str::to_string),
                "organization" | "author" | "project" => d.properties.text(key).map(str::to_string),
                _ => property(&d.properties),
            },
            Source::Sheet => {
                let sheet = d.sheets.get(self.sheet)?;
                if sheet.excluded.count {
                    return (key == "name").then(|| sheet.name.clone());
                }
                let counted = |s: &&std::sync::Arc<super::Sheet>| !s.excluded.count;
                match key {
                    "name" => Some(sheet.name.clone()),
                    "number" => Some((d.sheets.iter().take(self.sheet).filter(counted).count() + 1).to_string()),
                    "count" => Some(d.sheets.iter().filter(counted).count().to_string()),
                    _ => None,
                }
            }
            Source::Revision => d.revisions.rows().last()?.field(key).map(str::to_string),
            Source::Component => {
                let c = self.component?;
                match key {
                    "name" => Some(c.name.to_string()),
                    "material" => c.material.map(str::to_string),
                    "mass" => c.mass.map(str::to_string),
                    "part_number" | "description" | "revision" => c.metadata.text(key).map(str::to_string),
                    _ => key.strip_prefix(PROPERTIES).and_then(|name| c.properties?.get(name)).cloned(),
                }
            }
            Source::Prompt | Source::Row => None,
        }
    }
}

#[cfg(test)]
#[path = "fields_tests.rs"]
mod tests;
