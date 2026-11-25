//! The stored preference fields, one file per Preferences page. Each file's macro adds its
//! fields and hands on to the next; `all_preferences!` runs them in page order into
//! `preference_settings!`, which defines `Prefs` and everything built from the table.

#[macro_use]
mod general;
#[macro_use]
mod appearance;
#[macro_use]
mod layout;
#[macro_use]
mod graphics;
#[macro_use]
mod design;
#[macro_use]
mod security;
#[macro_use]
mod sketch;
#[macro_use]
mod tools;
#[macro_use]
mod navigation;
#[macro_use]
mod mouse3d;
#[macro_use]
mod shortcuts;
#[macro_use]
mod assistant;
#[macro_use]
mod plugins;
#[macro_use]
mod unpaged;

/// The whole table: the given legacy block, then every page's fields, into `preference_settings!`.
macro_rules! all_preferences {
    ($($legacy:tt)*) => {
        general_preferences! { appearance_preferences, layout_preferences, graphics_preferences, design_preferences, security_preferences, sketch_preferences, tools_preferences, navigation_preferences, mouse3d_preferences, shortcuts_preferences, assistant_preferences, plugins_preferences, unpaged_preferences, preference_settings ; $($legacy)* }
    };
}

#[cfg(test)]
mod tests {
    use super::super::{FIELD_PAGES, Prefs};

    /// The stored text has exactly one key per field, whichever page file holds it, and reads
    /// back to the same preferences.
    #[test]
    fn every_field_is_stored_once_and_reads_back() {
        let prefs = Prefs::default();
        let saved = serde_json::to_value(&prefs).unwrap();
        let keys: std::collections::BTreeSet<&str> = saved.as_object().unwrap().keys().map(String::as_str).collect();
        let fields: std::collections::BTreeSet<&str> = FIELD_PAGES.iter().map(|(field, _)| *field).collect();
        assert_eq!(keys, fields);
        assert_eq!(fields.len(), FIELD_PAGES.len(), "a field is in two page files");
        let back: Prefs = serde_json::from_str(&saved.to_string()).unwrap();
        assert_eq!(back, prefs);
    }

    /// Each page file keeps its fields in alphabetical order, so new preferences spread out
    /// instead of all landing at the end of a file.
    #[test]
    fn page_files_keep_their_fields_sorted() {
        for (file, source) in [
            ("general.rs", include_str!("general.rs")),
            ("appearance.rs", include_str!("appearance.rs")),
            ("layout.rs", include_str!("layout.rs")),
            ("graphics.rs", include_str!("graphics.rs")),
            ("design.rs", include_str!("design.rs")),
            ("security.rs", include_str!("security.rs")),
            ("sketch.rs", include_str!("sketch.rs")),
            ("tools.rs", include_str!("tools.rs")),
            ("navigation.rs", include_str!("navigation.rs")),
            ("mouse3d.rs", include_str!("mouse3d.rs")),
            ("shortcuts.rs", include_str!("shortcuts.rs")),
            ("assistant.rs", include_str!("assistant.rs")),
            ("plugins.rs", include_str!("plugins.rs")),
            ("unpaged.rs", include_str!("unpaged.rs")),
        ] {
            let names: Vec<&str> = source
                .lines()
                .filter_map(|line| line.strip_prefix("            "))
                .filter(|line| !line.starts_with(char::is_whitespace) && !line.starts_with("//") && !line.starts_with('#'))
                .filter_map(|line| line.split_once(':').map(|(name, _)| name))
                .filter(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
                .collect();
            assert!(!names.is_empty(), "no fields found in {file}");
            let mut sorted = names.clone();
            sorted.sort_unstable();
            assert_eq!(names, sorted, "{file} keeps its fields in alphabetical order");
        }
    }
}
