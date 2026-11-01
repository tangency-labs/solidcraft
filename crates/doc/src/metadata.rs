//! Design and part properties (#1015): the optional `metadata` object of the design and of each
//! component.
//!
//! SolidCraft reads a few well-known, unprefixed text keys from it (project, part number,
//! revision, author, organization, description, licence), listed once in [`KEYS`] with where
//! each may be set (the design, a component or both), how its value is checked and its label.
//! It keeps every other key exactly as it was through open, edit and save. Other tools keep
//! their own data under one reverse-DNS key each (`com.example.tool`), holding an object.
//! A component's part properties have one more key: its bill of materials type (`bom_type`).
//!
//! Reading never validates: a well-known key holding something other than text is kept as it
//! is and reads as absent, so a file another tool wrote always opens. Writing through
//! [`Metadata::set`] (the `document.metadata.set` and `component.metadata.set` commands) checks
//! the key against where it is set, the value's kind and its size, so nothing SolidCraft writes
//! is malformed.
//!
//! The map is stored as is: typed fields would refuse a design another tool had written
//! `"author": 42` into. Everything under `metadata` is informational, never geometry, so keeping
//! unknown keys can't leave stale model data behind (unlike keeping unknown top-level fields).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{BomType, DocError, Result};

pub const PROJECT: &str = "project";
pub const PART_NUMBER: &str = "part_number";
pub const REVISION: &str = "revision";
pub const AUTHOR: &str = "author";
pub const ORGANIZATION: &str = "organization";
pub const DESCRIPTION: &str = "description";
pub const LICENSE: &str = "license";

/// Where a key may be set: on the design, on a component, or on both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Design,
    Component,
    Both,
}

impl Scope {
    /// Whether a key of this scope may be set in `owner`'s metadata.
    pub fn covers(self, owner: Owner) -> bool {
        matches!((self, owner), (Scope::Both, _) | (Scope::Design, Owner::Design) | (Scope::Component, Owner::Component))
    }
}

/// Whose metadata a change is to: the design's (`document.metadata.set`) or a component's
/// (`component.metadata.set`). [`Metadata::set`] is told which.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Design,
    Component,
}

impl Owner {
    /// What one of its properties, and several, are called in a refusal.
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Owner::Design => ("design property", "design properties"),
            Owner::Component => ("part property", "part properties"),
        }
    }
}

/// One of SolidCraft's own keys.
pub struct Key {
    pub name: &'static str,
    /// Where it may be set.
    pub scope: Scope,
    /// The message id of its label (the Design Properties dialog, the Bill of Materials window).
    pub label: &'static str,
    /// The value as it is saved (`None`: the key is removed), or why it can't be; given the key
    /// to name it in the refusal.
    check: fn(&str, Value) -> Result<Option<Value>>,
}

/// SolidCraft's own keys, in the order the Design Properties dialog shows them; a new key is one
/// row here. Every other unprefixed key is reserved for SolidCraft too.
pub static KEYS: &[Key] = &[
    Key { name: PROJECT, scope: Scope::Both, label: "ui.design_properties.project", check: text },
    Key { name: PART_NUMBER, scope: Scope::Both, label: "ui.design_properties.part_number", check: text },
    Key { name: REVISION, scope: Scope::Both, label: "ui.design_properties.revision", check: text },
    Key { name: AUTHOR, scope: Scope::Both, label: "ui.design_properties.author", check: text },
    Key { name: ORGANIZATION, scope: Scope::Both, label: "ui.design_properties.organization", check: text },
    Key { name: DESCRIPTION, scope: Scope::Both, label: "ui.design_properties.description", check: text },
    Key { name: LICENSE, scope: Scope::Both, label: "ui.design_properties.license", check: text },
];

/// The keys a bill of materials lists for each part, in the order of its columns:
/// `component.metadata` returns each, `component.metadata.set` takes each as a parameter of its
/// own, and the Bill of Materials window edits them.
pub static BOM_KEYS: &[&str] = &[PART_NUMBER, DESCRIPTION, REVISION];

/// A component's [`BomType`] in the bill of materials (one of [`BomType::as_str`]'s words). It is
/// a part property of a component only, not a row of [`KEYS`], so the Design Properties dialog
/// doesn't list it and a design refuses it.
pub const BOM_TYPE: &str = "bom_type";

/// Longest key, in bytes.
pub const MAX_KEY_BYTES: usize = 128;
/// Longest well-known text value, in bytes.
pub const MAX_TEXT_BYTES: usize = 4096;
/// Largest namespaced value, as compact JSON.
pub const MAX_VALUE_BYTES: usize = 64 * 1024;
/// Deepest nesting of a namespaced value.
pub const MAX_VALUE_DEPTH: usize = 32;
/// Most keys, and largest object as compact JSON, that a change may grow the metadata to.
pub const MAX_KEYS: usize = 256;
pub const MAX_TOTAL_BYTES: usize = 256 * 1024;

/// A design's or a component's `metadata` object.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Metadata(pub Map<String, Value>);

/// SolidCraft's keys that may be set in `owner`'s metadata, in [`KEYS`]' order.
pub fn keys(owner: Owner) -> impl Iterator<Item = &'static Key> {
    KEYS.iter().filter(move |k| k.scope.covers(owner))
}

/// The names of [`keys`]`(owner)`, and a component's [`BOM_TYPE`], as refusals list them.
pub fn key_list(owner: Owner) -> String {
    let bom_type = (owner == Owner::Component).then_some(BOM_TYPE);
    keys(owner).map(|k| k.name).chain(bom_type).collect::<Vec<_>>().join(", ")
}

/// One of SolidCraft's keys, by name.
pub fn key(name: &str) -> Option<&'static Key> {
    KEYS.iter().find(|k| k.name == name)
}

/// A key SolidCraft reads as a design or part property.
pub fn is_well_known(name: &str) -> bool {
    key(name).is_some() || name == BOM_TYPE
}

/// A reverse-DNS key another tool keeps its data under: two or more dot-separated labels of
/// ASCII letters, digits, `-` and `_` (`com.example.tool`, `org.printfarm`).
pub fn is_namespaced(key: &str) -> bool {
    key.len() <= MAX_KEY_BYTES
        && key.split('.').count() >= 2
        && key.split('.').all(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
}

/// Text SolidCraft keeps as a property: at most [`MAX_TEXT_BYTES`], and no control characters
/// but line breaks and tabs. `what` names it in the refusal.
pub fn check_text(what: &str, text: &str) -> Result<()> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(invalid(format!("{what} is longer than {MAX_TEXT_BYTES} bytes")));
    }
    if text.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\t' | '\r')) {
        return Err(invalid(format!("{what} has a control character")));
    }
    Ok(())
}

/// A text key's check: text, which empty text (like `null`) removes.
fn text(key: &str, value: Value) -> Result<Option<Value>> {
    match value {
        Value::Null => Ok(None),
        Value::String(s) if s.is_empty() => Ok(None),
        Value::String(s) => {
            check_text(&format!("`{key}`"), &s)?;
            Ok(Some(Value::String(s)))
        }
        _ => Err(invalid(format!("`{key}` takes text"))),
    }
}

/// The bill of materials type's check: one of [`BomType`]'s words, which empty text (like
/// `null`) removes, so the component reads as normal again.
fn bom_type(value: Value) -> Result<Option<Value>> {
    let Some(word) = text(BOM_TYPE, value)? else { return Ok(None) };
    let s = word.as_str().unwrap_or_default();
    if BomType::parse(s).is_none() {
        let shown: String = s.chars().take(64).collect();
        let words: Vec<&str> = BomType::ALL.iter().map(|t| t.as_str()).collect();
        return Err(invalid(format!("`{BOM_TYPE}` is one of {}, not `{shown}`", words.join(", "))));
    }
    Ok(Some(word))
}

fn depth(v: &Value) -> usize {
    // Iterative, so a hostile value can't overflow the stack here.
    let mut deepest = 0;
    let mut stack = vec![(v, 1usize)];
    while let Some((v, d)) = stack.pop() {
        deepest = deepest.max(d);
        if d > MAX_VALUE_DEPTH {
            break;
        }
        match v {
            Value::Array(a) => stack.extend(a.iter().map(|x| (x, d + 1))),
            Value::Object(m) => stack.extend(m.values().map(|x| (x, d + 1))),
            _ => {}
        }
    }
    deepest
}

fn json_len(v: &impl Serialize) -> usize {
    serde_json::to_vec(v).map_or(usize::MAX, |b| b.len())
}

fn invalid(msg: String) -> DocError {
    DocError::Invalid(msg)
}

impl Metadata {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// A text value: `None` when the key is missing, empty or holds something other than text.
    pub fn text(&self, key: &str) -> Option<&str> {
        self.0.get(key)?.as_str().filter(|s| !s.is_empty())
    }

    pub fn project(&self) -> Option<&str> {
        self.text(PROJECT)
    }
    pub fn part_number(&self) -> Option<&str> {
        self.text(PART_NUMBER)
    }
    pub fn revision(&self) -> Option<&str> {
        self.text(REVISION)
    }
    pub fn author(&self) -> Option<&str> {
        self.text(AUTHOR)
    }
    pub fn organization(&self) -> Option<&str> {
        self.text(ORGANIZATION)
    }
    pub fn description(&self) -> Option<&str> {
        self.text(DESCRIPTION)
    }
    pub fn license(&self) -> Option<&str> {
        self.text(LICENSE)
    }
    /// The component's bill of materials type: normal when the key is missing, or holds a word
    /// this build doesn't know (a newer SolidCraft's type reads as normal and is kept as it is).
    pub fn bom_type(&self) -> BomType {
        self.text(BOM_TYPE).and_then(BomType::parse).unwrap_or_default()
    }

    /// The keys SolidCraft doesn't read (other tools' data, or keys a newer SolidCraft added),
    /// kept as they are.
    pub fn others(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.0.iter().filter(|(k, _)| !is_well_known(k))
    }

    /// Set `key` to `value` in `owner`'s metadata, after checking both; `null` (or empty text for
    /// a text key) removes it. Returns whether the metadata changed.
    pub fn set(&mut self, owner: Owner, key: &str, value: Value) -> Result<bool> {
        let value = check(owner, key, value)?;
        let Some(value) = value else { return Ok(self.remove(key)) };
        if self.0.get(key) == Some(&value) {
            return Ok(false);
        }
        let (keys, bytes) = (self.0.len(), json_len(&self.0));
        let old = self.0.insert(key.to_string(), value);
        // Growing past the limits is refused; a file already past them can still shrink.
        let (new_keys, new_bytes) = (self.0.len(), json_len(&self.0));
        if (new_keys > MAX_KEYS && new_keys > keys) || (new_bytes > MAX_TOTAL_BYTES && new_bytes > bytes) {
            match old {
                Some(old) => self.0.insert(key.to_string(), old),
                None => self.0.remove(key),
            };
            return Err(invalid(format!("{} are limited to {MAX_KEYS} keys and {} KiB", owner.names().1, MAX_TOTAL_BYTES / 1024)));
        }
        Ok(true)
    }

    /// Remove `key`, whatever it holds. Returns whether it was there.
    pub fn remove(&mut self, key: &str) -> bool {
        self.0.remove(key).is_some()
    }
}

/// `value` as `key` may hold it in `owner`'s metadata (`None`: remove the key), or why it can't.
fn check(owner: Owner, key: &str, value: Value) -> Result<Option<Value>> {
    if key == BOM_TYPE && owner == Owner::Component {
        return bom_type(value);
    }
    if let Some(k) = self::key(key).filter(|k| k.scope.covers(owner)) {
        return (k.check)(key, value);
    }
    if is_namespaced(key) {
        return match value {
            Value::Null => Ok(None),
            Value::Object(m) => {
                let v = Value::Object(m);
                if depth(&v) > MAX_VALUE_DEPTH {
                    return Err(invalid(format!("`{key}` is nested more than {MAX_VALUE_DEPTH} levels deep")));
                }
                if json_len(&v) > MAX_VALUE_BYTES {
                    return Err(invalid(format!("`{key}` is larger than {} KiB", MAX_VALUE_BYTES / 1024)));
                }
                Ok(Some(v))
            }
            _ => Err(invalid(format!("`{key}` must hold an object (a tool keeps its own data as an object under its reverse-DNS key)"))),
        };
    }
    let shown: String = key.chars().take(64).collect();
    Err(invalid(format!(
        "`{shown}` is not a {}: SolidCraft's are {}; another tool's data goes under a reverse-DNS key such as `com.example.tool`",
        owner.names().0,
        key_list(owner)
    )))
}

#[cfg(test)]
mod tests {
    use super::Owner::{Component, Design};
    use super::*;
    use serde_json::json;

    #[test]
    fn well_known_keys_read_as_text_and_ignore_other_kinds() {
        let m: Metadata = serde_json::from_value(json!({"author": "Jane Doe", "revision": 3, "project": "", "license": null})).unwrap();
        assert_eq!(m.author(), Some("Jane Doe"));
        assert_eq!(m.revision(), None, "a number is kept but is not a revision");
        assert_eq!(m.project(), None, "empty text reads as absent");
        assert_eq!(m.license(), None);
        assert_eq!(m.get("revision"), Some(&json!(3)), "kept as it was");
    }

    #[test]
    fn set_checks_keys_kinds_and_sizes() {
        let mut m = Metadata::default();
        assert!(m.set(Design, "author", json!("Jane")).unwrap());
        assert!(!m.set(Design, "author", json!("Jane")).unwrap(), "the same value is no change");
        assert!(m.set(Design, "com.acme.tool", json!({"shop_id": 88123, "tags": ["a", null, 1.5]})).unwrap());
        assert!(m.set(Design, "author", json!(42)).is_err(), "well-known keys take text");
        assert!(m.set(Design, "author", json!("a\u{7}b")).is_err(), "no control characters");
        assert!(m.set(Design, "description", json!("two\nlines\tand a tab")).unwrap());
        assert!(m.set(Design, "com.acme.tool", json!("text")).is_err(), "a tool's key holds an object");
        assert!(m.set(Design, "colour", json!("red")).is_err(), "unprefixed keys are reserved");
        assert!(m.set(Design, "", json!({})).is_err());
        for bad in ["com.", ".com", "com..x", "com.ex ample", "com.é", "single"] {
            assert!(m.set(Design, bad, json!({})).is_err(), "{bad}");
        }
        assert!(m.set(Design, "author", json!("x".repeat(MAX_TEXT_BYTES + 1))).is_err());
        assert!(m.set(Design, "org.big", json!({"a": "x".repeat(MAX_VALUE_BYTES)})).is_err());
        let mut deep = json!({});
        for _ in 0..MAX_VALUE_DEPTH {
            deep = json!({ "a": deep });
        }
        assert!(m.set(Design, "org.deep", deep).is_err());
        assert_eq!(m.author(), Some("Jane"), "a refused value leaves the old one");
        // Null and empty text remove.
        assert!(m.set(Design, "author", json!("")).unwrap());
        assert!(m.set(Design, "com.acme.tool", Value::Null).unwrap());
        assert!(!m.set(Design, "com.acme.tool", Value::Null).unwrap());
        assert_eq!(m.0.keys().collect::<Vec<_>>(), ["description"]);
    }

    #[test]
    fn a_bom_type_takes_one_of_its_words_and_reads_as_normal_otherwise() {
        let mut m = Metadata::default();
        assert_eq!(m.bom_type(), BomType::Normal, "no key: normal");
        for t in BomType::ALL {
            assert!(m.set(Component, BOM_TYPE, json!(t.as_str())).is_ok(), "{t:?}");
            assert_eq!(m.bom_type(), t);
            assert_eq!(BomType::parse(t.as_str()), Some(t));
        }
        for bad in [json!("Phantom"), json!("kit"), json!(" phantom"), json!(1), json!({"type": "phantom"}), json!("a\u{7}")] {
            assert!(m.set(Component, BOM_TYPE, bad.clone()).is_err(), "{bad}");
        }
        assert_eq!(m.bom_type(), BomType::Inseparable, "a refused value leaves the old one");
        assert!(m.set(Component, BOM_TYPE, json!("")).unwrap(), "empty text removes it");
        assert_eq!(m.bom_type(), BomType::Normal);
        assert!(m.others().next().is_none(), "SolidCraft reads it, so it isn't another tool's key");
        // A word a newer build wrote is kept as it is and reads as normal here.
        let newer: Metadata = serde_json::from_value(json!({"bom_type": "kit"})).unwrap();
        assert_eq!(newer.bom_type(), BomType::Normal);
        assert_eq!(newer.get(BOM_TYPE), Some(&json!("kit")));
        assert!(!keys(Design).any(|k| k.name == BOM_TYPE), "the Design Properties dialog doesn't list it");
        assert!(Metadata::default().set(Design, BOM_TYPE, json!("phantom")).is_err(), "a design has no bill of materials type");
    }

    #[test]
    fn the_limits_refuse_growth_but_let_an_oversized_file_shrink() {
        let mut m = Metadata::default();
        for i in 0..MAX_KEYS {
            m.set(Design, &format!("org.k{i}"), json!({})).unwrap();
        }
        assert!(m.set(Design, "org.one_more", json!({})).is_err());
        assert_eq!(m.0.len(), MAX_KEYS);
        // Each owner's refusal names its own properties.
        let refusal = |owner| m.clone().set(owner, "org.one_more", json!({})).unwrap_err().to_string();
        assert_eq!(refusal(Design), "design properties are limited to 256 keys and 256 KiB");
        assert_eq!(refusal(Component), "part properties are limited to 256 keys and 256 KiB");
        // A file written past the limits by another tool still opens and can be cut down.
        let mut big = Metadata::default();
        for i in 0..MAX_KEYS + 10 {
            big.0.insert(format!("org.k{i}"), json!({}));
        }
        assert!(big.set(Design, "org.k0", Value::Null).unwrap());
        assert!(big.set(Design, "author", json!("Jane")).is_err(), "still over: no new keys");
        assert!(big.set(Design, "org.k1", json!({"replaced": true})).is_ok(), "the count doesn't grow");
    }

    #[test]
    fn each_key_is_one_row_in_the_order_the_dialog_shows() {
        let names: Vec<&str> = KEYS.iter().map(|k| k.name).collect();
        assert_eq!(names, [PROJECT, PART_NUMBER, REVISION, AUTHOR, ORGANIZATION, DESCRIPTION, LICENSE]);
        for (i, k) in KEYS.iter().enumerate() {
            assert!(!names[..i].contains(&k.name), "`{}` has two rows", k.name);
            assert!(!k.name.is_empty() && !is_namespaced(k.name), "`{}` is unprefixed", k.name);
            assert!(k.label.starts_with("ui.") && !KEYS[..i].iter().any(|o| o.label == k.label), "`{}`'s label", k.name);
            assert!(is_well_known(k.name) && key(k.name).is_some_and(|f| std::ptr::eq(f, k)));
        }
        for name in ["", "colour", "Author", "author ", "com.acme.tool"] {
            assert!(!is_well_known(name), "{name:?}");
        }
        // Every key SolidCraft took on the design and on a component before the table still is.
        assert_eq!(key_list(Design), "project, part_number, revision, author, organization, description, license");
        assert_eq!(key_list(Component), format!("{}, {BOM_TYPE}", key_list(Design)), "a component also takes its bill of materials type");
    }

    #[test]
    fn the_bill_of_materials_keys_are_rows_a_component_takes_in_column_order() {
        assert_eq!(BOM_KEYS, [PART_NUMBER, DESCRIPTION, REVISION], "the bill of materials' columns");
        for name in BOM_KEYS {
            assert!(key(name).is_some_and(|k| k.scope.covers(Component)), "`{name}` is a row of KEYS a component takes");
        }
    }

    #[test]
    fn a_key_is_taken_only_where_its_scope_covers_and_a_refusal_names_the_owner() {
        let covered: Vec<bool> =
            [Scope::Design, Scope::Component, Scope::Both].into_iter().flat_map(|s| [s.covers(Design), s.covers(Component)]).collect();
        assert_eq!(covered, [true, false, false, true, true, true], "design, component and both, on the design and on a component");
        for owner in [Design, Component] {
            for k in KEYS {
                assert_eq!(Metadata::default().set(owner, k.name, json!("x")).is_ok(), k.scope.covers(owner), "`{}` on {owner:?}", k.name);
            }
            assert!(Metadata::default().set(owner, "com.acme.tool", json!({"id": 1})).unwrap(), "another tool's key, on {owner:?}");
        }
        let refusal = |owner, key: &str| Metadata::default().set(owner, key, json!("x")).unwrap_err().to_string();
        assert_eq!(
            refusal(Design, "colour"),
            "`colour` is not a design property: SolidCraft's are project, part_number, revision, author, organization, \
             description, license; another tool's data goes under a reverse-DNS key such as `com.example.tool`",
            "the design's wording is as it was"
        );
        assert_eq!(
            refusal(Component, "colour"),
            "`colour` is not a part property: SolidCraft's are project, part_number, revision, author, organization, \
             description, license, bom_type; another tool's data goes under a reverse-DNS key such as `com.example.tool`"
        );
        for owner in [Design, Component] {
            let word = owner.names().0;
            assert!(refusal(owner, "").starts_with(&format!("`` is not a {word}:")), "the empty key on {owner:?}");
            assert!(refusal(owner, "Part_Number").starts_with("`Part_Number` is not"), "keys are matched exactly");
            let long = "k".repeat(MAX_KEY_BYTES * 2);
            assert!(refusal(owner, &long).starts_with(&format!("`{}` is not a {word}", "k".repeat(64))), "shown cut to 64 characters");
        }
    }

    #[test]
    fn a_text_key_takes_up_to_its_limit_with_line_breaks_and_tabs_only() {
        for owner in [Design, Component] {
            let set = |value: Value| Metadata::default().set(owner, AUTHOR, value);
            assert!(set(json!("x".repeat(MAX_TEXT_BYTES))).unwrap(), "exactly the limit");
            assert!(set(json!("é".repeat(MAX_TEXT_BYTES / 2))).unwrap(), "bytes, not characters: 4096");
            let over = set(json!(format!("{}é", "x".repeat(MAX_TEXT_BYTES - 1)))).unwrap_err().to_string();
            assert_eq!(over, "`author` is longer than 4096 bytes", "one character, two bytes past the limit");
            assert!(set(json!("a\nb\r\nc\td")).unwrap(), "line breaks and tabs");
            assert!(set(json!(" ")).unwrap(), "a space is text, kept as it is");
            for c in ['\u{0}', '\u{7}', '\u{1b}', '\u{7f}', '\u{85}'] {
                let refused = set(json!(format!("a{c}b"))).unwrap_err().to_string();
                assert_eq!(refused, "`author` has a control character", "{:?}", c);
            }
            for kind in [json!(42), json!(true), json!(["x"]), json!({"text": "x"})] {
                assert_eq!(set(kind.clone()).unwrap_err().to_string(), "`author` takes text", "{kind}");
            }
            // Null and empty text remove the key, and removing a missing one is no change.
            let mut m = Metadata::default();
            assert!(m.set(owner, AUTHOR, json!("Jane")).unwrap());
            assert!(m.set(owner, AUTHOR, json!("")).unwrap() && m.is_empty());
            assert!(!m.set(owner, AUTHOR, Value::Null).unwrap());
        }
        assert_eq!(
            check_text("property `finish`", &"x".repeat(MAX_TEXT_BYTES + 1)).unwrap_err().to_string(),
            "property `finish` is longer than 4096 bytes"
        );
    }

    /// Every hostile key with every hostile value, on the design and on a component: no panic,
    /// and a refusal leaves the metadata exactly as it was.
    #[test]
    fn hostile_keys_and_values_are_refused_without_a_change() {
        let long_key = "k".repeat(MAX_KEY_BYTES + 1);
        let long_tool = format!("com.{}", "k".repeat(MAX_KEY_BYTES));
        let keys = [
            "",
            " ",
            ".",
            "a.",
            ".a",
            "a..b",
            "author",
            "AUTHOR",
            " author",
            "part_number\n",
            "é",
            "\u{0}",
            "com.acme.tool",
            long_key.as_str(),
            long_tool.as_str(),
        ];
        let mut deep = json!({});
        for _ in 0..MAX_VALUE_DEPTH {
            deep = json!({ "a": deep });
        }
        let values = [
            Value::Null,
            json!(""),
            json!(" "),
            json!(0),
            json!(-1.5e308),
            json!(true),
            json!([]),
            json!({}),
            json!("a\u{0}b"),
            json!("x".repeat(MAX_TEXT_BYTES)),
            json!("x".repeat(MAX_TEXT_BYTES + 1)),
            json!({"a": "x".repeat(MAX_VALUE_BYTES)}),
            deep,
        ];
        for owner in [Design, Component] {
            for key in keys {
                for value in &values {
                    let mut m: Metadata = serde_json::from_value(json!({"author": "Jane", "com.acme.tool": {"id": 7}})).unwrap();
                    let before = m.clone();
                    if m.set(owner, key, value.clone()).is_err() {
                        assert_eq!(m, before, "{owner:?} {key:?} = {value}");
                    }
                }
            }
        }
    }

    /// "Fields that change no geometry" covers each scope: the design's properties, a
    /// component's, and its free properties. Each alone, and its undo, is a part-property
    /// change (nothing to evaluate); anything else beside it is not.
    #[test]
    fn only_part_properties_changed_in_each_scope() {
        let mut before = crate::Document::default();
        for (id, name) in [(1, "Plate"), (2, "Pin")] {
            before.components.push(crate::Component { id, name: name.into(), ..Default::default() });
        }
        let mut design = before.clone();
        design.metadata.set(Design, AUTHOR, json!("Jane")).unwrap();
        let mut part = before.clone();
        part.components[1].metadata.set(Component, PART_NUMBER, json!("PN-040")).unwrap();
        let mut free = before.clone();
        free.components[0].properties.insert("finish".into(), "zinc".into());
        let mut all = design.clone();
        all.components.clone_from(&part.components);
        all.components[0].properties.clone_from(&free.components[0].properties);
        for (what, after) in [("design", &design), ("component", &part), ("free property", &free), ("all three", &all)] {
            assert!(after.differs_only_in_part_properties(&before), "{what}");
            assert!(before.differs_only_in_part_properties(after), "{what}, undone");
        }
        assert!(!before.differs_only_in_part_properties(&before.clone()), "no change is not a part-property change");

        let mut units = design.clone();
        units.units = "in".into();
        let mut renamed = part.clone();
        renamed.components[1].name = "Dowel".into();
        let mut added = free.clone();
        added.components.push(crate::Component { id: 3, name: "Nut".into(), ..Default::default() });
        let mut removed = part.clone();
        removed.components.remove(0);
        // A part property changed on components that swapped places: the components changed too.
        let mut swapped = before.clone();
        swapped.components.reverse();
        swapped.components[0].metadata.set(Component, PART_NUMBER, json!("PN-040")).unwrap();
        let mut next_id = design.clone();
        next_id.next_id += 1;
        for (what, after) in [
            ("units", &units),
            ("a component's name", &renamed),
            ("a new component", &added),
            ("a removed component", &removed),
            ("swapped components", &swapped),
            ("next id", &next_id),
        ] {
            assert!(!after.differs_only_in_part_properties(&before), "{what}");
            assert!(!before.differs_only_in_part_properties(after), "{what}, undone");
        }
    }
}
