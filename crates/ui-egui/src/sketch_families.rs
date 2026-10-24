//! Sketch creation families and the active part's last interactive variant.

use std::collections::BTreeMap;

use solidcraft_engine::CommandSpec;

pub struct Family {
    pub label: &'static str,
    pub commands: &'static [&'static str],
}

pub const FAMILIES: &[Family] = &[
    Family { label: "Line", commands: &["sketch.line", "sketch.line.midpoint"] },
    Family {
        label: "Rectangle",
        commands: &["sketch.rectangle.two_point", "sketch.rectangle.three_point", "sketch.rectangle.center", "sketch.rectangle.rounded"],
    },
    Family {
        label: "Circle",
        commands: &[
            "sketch.circle.center",
            "sketch.circle.two_point",
            "sketch.circle.three_point",
            "sketch.circle.two_tangent",
            "sketch.circle.three_tangent",
        ],
    },
    Family { label: "Arc", commands: &["sketch.arc.three_point", "sketch.arc.center_point", "sketch.arc.tangent"] },
    Family {
        label: "Slot",
        commands: &[
            "sketch.slot.center_to_center",
            "sketch.slot.overall",
            "sketch.slot.center_point",
            "sketch.slot.arc_three_point",
            "sketch.slot.arc_center",
        ],
    },
    Family { label: "Polygon", commands: &["sketch.polygon.inscribed", "sketch.polygon.circumscribed", "sketch.polygon.edge"] },
    Family { label: "Spline", commands: &["sketch.spline.fit_point", "sketch.spline.control_point", "sketch.spline.control_point_5"] },
];

pub fn family(id: &str) -> Option<&'static Family> {
    FAMILIES.iter().find(|f| f.commands.contains(&id))
}

/// The variant last started in each family. A user preference, so it survives new and opened
/// designs and restarts; it never dirties the model or its undo history. Keys are family labels,
/// values command ids; anything that is not a variant of its family is ignored when read.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Choices(BTreeMap<String, String>);

impl Choices {
    pub fn remember(&mut self, id: &str) {
        if let Some(f) = family(id)
            && let Some(id) = f.commands.iter().find(|c| **c == id)
        {
            self.0.insert(f.label.to_string(), (*id).to_string());
        }
    }

    pub fn selected(&self, f: &Family) -> Option<&'static CommandSpec> {
        let saved = self.0.get(f.label).and_then(|s| f.commands.iter().find(|c| **c == s.as_str()));
        saved.or_else(|| f.commands.first()).copied().and_then(solidcraft_engine::find_command)
    }

    /// What a family's main button or shortcut starts: its remembered variant when `id` is the
    /// family's default command (the one carrying the shortcut), otherwise `id` itself.
    pub fn resolve(&self, id: &'static str) -> &'static str {
        match family(id) {
            Some(f) if f.commands.first().copied() == Some(id) => self.selected(f).map_or(id, |c| c.id),
            _ => id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_are_disjoint_registered_creation_commands() {
        let mut seen = std::collections::BTreeSet::new();
        for f in FAMILIES {
            assert!(!f.commands.is_empty());
            for id in f.commands {
                assert!(seen.insert(id), "duplicate variant {id}");
                let c = solidcraft_engine::find_command(id).unwrap();
                assert_eq!((c.tab, c.panel), ("SKETCH", "CREATE"));
            }
        }
    }

    #[test]
    fn choices_ignore_unknown_commands_and_keep_families_independent() {
        let mut choices = Choices::default();
        choices.remember("sketch.rectangle.center");
        choices.remember("sketch.circle.three_point");
        choices.remember("sketch.rectangle.unknown");
        for (id, expected) in [
            ("sketch.rectangle.two_point", "sketch.rectangle.center"),
            ("sketch.circle.center", "sketch.circle.three_point"),
            ("sketch.line", "sketch.line"),
        ] {
            assert_eq!(choices.selected(family(id).unwrap()).unwrap().id, expected);
        }
    }

    #[test]
    fn rectangle_comes_back_in_the_variant_last_used_and_it_survives_a_prefs_reload() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", serde_json::json!({"plane":"XY"})).unwrap();
        h.app.start("sketch.rectangle.three_point");
        h.settle();
        assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.rectangle.three_point"));
        h.app.tool = None;
        // The main button and the shortcut both start the family's default command.
        let rect = family("sketch.rectangle.two_point").unwrap();
        assert_eq!(h.app.preferences.sketch_variants.selected(rect).unwrap().id, "sketch.rectangle.three_point");
        assert_eq!(h.app.preferences.sketch_variants.resolve("sketch.rectangle.two_point"), "sketch.rectangle.three_point");
        h.app.start(h.app.preferences.sketch_variants.resolve("sketch.rectangle.two_point"));
        assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.rectangle.three_point"));
        // Another family is untouched, and a non-default variant is never rewritten.
        assert_eq!(h.app.preferences.sketch_variants.resolve("sketch.circle.center"), "sketch.circle.center");
        assert_eq!(h.app.preferences.sketch_variants.resolve("sketch.rectangle.center"), "sketch.rectangle.center");
        // A new design keeps it; so does a restart.
        let mut restored = crate::SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert_eq!(restored.preferences.sketch_variants.resolve("sketch.rectangle.two_point"), "sketch.rectangle.three_point");
    }

    #[test]
    fn pinned_sketch_variants_leave_the_family_choice_to_its_menu() {
        use serde_json::json;

        for (main, pinned, menu_choice) in [
            ("sketch.rectangle.two_point", "sketch.rectangle.center", "sketch.rectangle.three_point"),
            ("sketch.circle.center", "sketch.circle.two_point", "sketch.circle.three_point"),
        ] {
            let mut h = crate::scenario::Harness::new();
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            crate::toolbar_custom::pin(&mut h.app, "SKETCH", pinned).unwrap();
            h.settle();
            let f = family(main).unwrap();

            for expected in [main, menu_choice] {
                if expected == menu_choice {
                    h.step(&json!({"click":{"handle":format!("family:{}", f.label)}})).unwrap();
                    h.step(&json!({"click":{"handle":format!("dropdown:{menu_choice}")}})).unwrap();
                    assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some(menu_choice));
                }
                h.step(&json!({"click":{"handle":format!("button:{pinned}")}})).unwrap();
                assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some(pinned), "the pin starts its exact tool");
                assert_eq!(h.app.preferences.sketch_variants.resolve(main), expected, "the pin leaves the family choice alone");

                h.step(&json!({"click":{"handle":format!("button:{main}")}})).unwrap();
                assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some(expected), "the main button still starts the chosen variant");
            }
        }
    }

    #[test]
    fn old_or_damaged_prefs_fall_back_to_each_familys_first_variant() {
        let mut app = crate::SolveApp::new(Default::default(), Default::default());
        app.load_prefs(r#"{"preferences":{"save_mesh_format":"3mf"}}"#);
        assert_eq!(app.preferences.sketch_variants.resolve("sketch.rectangle.two_point"), "sketch.rectangle.two_point");
        app.load_prefs(r#"{"preferences":{"sketch_variants":{"Rectangle":"sketch.circle.center","Gone":"x"}}}"#);
        assert_eq!(app.preferences.sketch_variants.resolve("sketch.rectangle.two_point"), "sketch.rectangle.two_point");
        app.load_prefs(r#"{"preferences":{"sketch_variants":7}}"#);
        assert_eq!(app.preferences.sketch_variants.resolve("sketch.polygon.inscribed"), "sketch.polygon.inscribed");
    }
}
