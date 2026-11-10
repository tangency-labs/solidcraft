//! What a theme arranges besides its colours: the optional `layout` object of a
//! `solidcraft-theme/1` file (`theme_files`), made of named sections. Each section is one row of
//! `sections!` below: a field, its type, and that type's [`Section::parse`], a strict reading of
//! hostile input. A part of the UI that becomes themeable adds a section (a type and a row); the
//! file format, import, export and storage need no change.
//!
//! Forward compatible: a section, or a setting inside one, that this version doesn't know is kept
//! in the file (exporting it loses nothing) and listed in [`Layout::unknown`], never refused, so a
//! theme written for a newer SolidCraft still imports into this one. A known setting with a bad
//! value is refused, as everywhere in theme files.
//!
//! The layout of the theme on now belongs to the UI thread ([`with`]); `prefs::apply` sets it with
//! the theme's colours. Today's one section:
//!
//! - `preferences`: values for the preferences in [`THEMEABLE`] (the workspace layout, where
//!   dialogs open, the navigation bar, ribbon labels), set when the theme is turned on; the user's
//!   own values come back when another theme is turned on (`prefs::apply_theme_preferences`).
//!
//! ```json
//! "layout": {
//!   "preferences": {"layout": "floating", "ribbon_labels": false, "dialog_place": "edge"}
//! }
//! ```

use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

use serde_json::{Map, Value};

/// Hostile-input caps: sections, and entries in one section.
const MAX_SECTIONS: usize = 32;
const MAX_ENTRIES: usize = 512;

/// One section of a theme's layout.
pub trait Section: Sized {
    /// Read the section, refusing bad values. A setting this version doesn't know goes onto
    /// `unknown` (its path inside the section) and is otherwise skipped.
    fn parse(value: &Value, unknown: &mut Vec<String>) -> Result<Self, String>;
}

macro_rules! sections {
    ($($(#[$doc:meta])* $field:ident: $ty:ty),* $(,)?) => {
        /// A theme's layout, read. A section the theme leaves out is `None`.
        #[derive(Clone, Debug, Default, PartialEq)]
        pub struct Layout {
            $($(#[$doc])* pub $field: Option<$ty>,)*
            /// Sections and settings this version doesn't know.
            pub unknown: Vec<String>,
        }

        impl Layout {
            /// Read a theme file's `layout` object.
            pub fn parse(layout: &BTreeMap<String, Value>) -> Result<Layout, String> {
                if layout.len() > MAX_SECTIONS {
                    return Err(format!("`layout` has more than {MAX_SECTIONS} sections"));
                }
                let mut out = Layout::default();
                for (key, value) in layout {
                    let mut unknown = Vec::new();
                    match key.as_str() {
                        $(stringify!($field) => {
                            out.$field = Some(<$ty>::parse(value, &mut unknown).map_err(|e| format!("`layout.{key}`: {e}"))?);
                            out.unknown.extend(unknown.into_iter().map(|path| format!("{key}.{path}")));
                        })*
                        _ => out.unknown.push(key.clone()),
                    }
                }
                Ok(out)
            }
        }
    };
}

sections! {
    /// Preference values set while the theme is on.
    preferences: Preferences,
}

fn object<'a>(value: &'a Value, what: &str) -> Result<&'a Map<String, Value>, String> {
    let object = value.as_object().ok_or_else(|| format!("{what} must be an object"))?;
    if object.len() > MAX_ENTRIES {
        return Err(format!("{what} has more than {MAX_ENTRIES} entries"));
    }
    Ok(object)
}

/// The preferences a theme may set: how the workspace is laid out (floating cards or docked),
/// where dialogs open, the navigation bar (its buttons, place, size and labels) and the ribbon's
/// labels. A preference that becomes themeable joins this list; its value is checked as the
/// preferences check it.
pub const THEMEABLE: &[&str] = &["dialog_place", "layout", "navigation_bar", "ribbon_labels"];

/// `preferences`: `{"<preference>": value}` for preferences in [`THEMEABLE`], in the form the
/// preferences file and `ui.prefs` use. A preference this version doesn't know, or doesn't let a
/// theme set, is skipped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preferences {
    pub values: BTreeMap<String, Value>,
}

impl Section for Preferences {
    fn parse(value: &Value, unknown: &mut Vec<String>) -> Result<Self, String> {
        let defaults = serde_json::to_value(crate::prefs::Prefs::default()).map_err(|e| e.to_string())?;
        let mut values = BTreeMap::new();
        for (key, value) in object(value, "the section")? {
            if !THEMEABLE.contains(&key.as_str()) {
                unknown.push(key.clone());
                continue;
            }
            if key == "navigation_bar" {
                crate::navigation_bar::validate(value)?;
            }
            // Read as the preferences file is: a value of the wrong kind doesn't read.
            let mut probe = defaults.clone();
            if let Some(fields) = probe.as_object_mut() {
                fields.insert(key.clone(), value.clone());
            }
            if serde_json::from_value::<crate::prefs::Prefs>(probe).is_err() {
                return Err(format!("`{key}` is not a value that preference takes"));
            }
            values.insert(key.clone(), value.clone());
        }
        Ok(Preferences { values })
    }
}

thread_local! {
    /// The layout of the theme on now and the `layout` object it was read from, per thread like
    /// the theme's colours. `None`: no theme layout.
    static CURRENT: Cell<Option<Rc<(BTreeMap<String, Value>, Layout)>>> = const { Cell::new(None) };
}

/// Run `read` with the layout of the theme on now (empty when it has none), without copying it.
pub fn with<R>(read: impl FnOnce(&Layout) -> R) -> R {
    let held = CURRENT.take();
    let out = match &held {
        Some(held) => read(&held.1),
        None => read(&Layout::default()),
    };
    CURRENT.set(held);
    out
}

/// Use a theme's `layout` object (`None`: the theme on has none). It is read again only when it
/// changes; one that doesn't read leaves no layout (it was checked when imported, so only a hand
/// edit of saved preferences gets here). Returns whether anything changed.
pub fn set_current(layout: Option<&BTreeMap<String, Value>>) -> bool {
    let held = CURRENT.take();
    let same = match (&held, layout) {
        (Some(held), Some(layout)) => held.0 == *layout,
        (None, None) => true,
        (None, Some(layout)) => layout.is_empty(),
        (Some(_), None) => false,
    };
    if same {
        CURRENT.set(held);
        return false;
    }
    let read = layout.filter(|layout| !layout.is_empty()).map(|layout| {
        let parsed = Layout::parse(layout).unwrap_or_else(|e| {
            log::warn!("theme layout: {e}; using none");
            Layout::default()
        });
        Rc::new((layout.clone(), parsed))
    });
    CURRENT.set(read);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn layout(value: Value) -> Result<Layout, String> {
        Layout::parse(&serde_json::from_value(value).unwrap())
    }

    #[test]
    fn a_theme_sets_only_themeable_preferences_with_values_they_take() {
        let read = layout(json!({"preferences": {"layout": "floating", "ribbon_labels": false, "theme": "dark"}})).unwrap();
        let values = read.preferences.unwrap().values;
        assert_eq!(values.keys().collect::<Vec<_>>(), ["layout", "ribbon_labels"]);
        assert_eq!(read.unknown, ["preferences.theme"], "a preference a theme may not set is kept, not used");
        for bad in [json!({"preferences": {"ribbon_labels": "yes"}}), json!({"preferences": {"layout": "sideways"}}), json!({"preferences": []})] {
            assert!(layout(bad.clone()).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_section_from_a_newer_version_is_kept_but_not_refused() {
        let read = layout(json!({"toolbars": {"anything": 1}, "preferences": {}})).unwrap();
        assert_eq!(read.unknown, ["toolbars"]);
        assert_eq!(read.preferences.map(|p| p.values.len()), Some(0));
    }

    #[test]
    fn the_current_layout_is_read_once_and_cleared_with_no_theme_layout() {
        let file: BTreeMap<String, Value> = serde_json::from_value(json!({"preferences": {"ribbon_labels": false}})).unwrap();
        assert!(set_current(Some(&file)));
        assert!(!set_current(Some(&file)), "the same layout is not read again");
        assert!(with(|l| l.preferences.as_ref().is_some_and(|p| p.values.contains_key("ribbon_labels"))));
        assert!(set_current(None));
        assert!(with(|l| l.preferences.is_none()));
    }
}
