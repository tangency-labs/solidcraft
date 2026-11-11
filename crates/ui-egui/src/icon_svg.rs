//! SolidCraft's icon lookups over its set (`solidcraft_look::icons::SET`), drawn by the kit's
//! renderer ([`crate::ui_kit::icons`]).
//!
//! The masters, their colour roles, accents and aliases live in the look crate, which compiles
//! them and installs the set at startup and in the test contexts (`solidcraft_look::install`).
//! The lookups here ([`resolve`], [`doc_for`], …) read SolidCraft's set whether or not it is
//! installed; the look crate's tests hold that every master parses.

pub use crate::ui_kit::icons::{
    Doc, Look, PLACEHOLDER, Palette, Resolved, audit, render, render_tinted, set_theme_palette, theme_palette, theme_palette_state,
};
use solidcraft_look::icons::SET;

/// The master and accent for an app icon id: an alias (with its accent), a master of that name,
/// or the placeholder.
pub fn resolve(name: &str) -> Resolved {
    SET.resolve(name)
}

/// The master to draw at `px` pixels: the small-slot variant where one exists.
pub fn doc_for(master: &str, px: u32) -> Option<&'static Doc> {
    SET.doc_for(master, px)
}

/// [`IconSet::accent_index`](crate::ui_kit::icons::IconSet::accent_index) in SolidCraft's set.
pub fn accent_index(name: &str) -> Option<usize> {
    SET.accent_index(name)
}

/// Primary semantic stroke, also used by toolbar affordances. The palette owns its value.
pub fn primary(name: &str, dark: bool, vision: solidcraft_theme::ColorVision) -> egui::Color32 {
    SET.primary(name, dark, vision)
}

/// [`IconSet::role_colour`](crate::ui_kit::icons::IconSet::role_colour) in SolidCraft's set.
pub fn role_colour(name: &str, look: Look) -> egui::Color32 {
    SET.role_colour(name, look)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_look::icons::{ACCENT_NAMES, ACCENT_VARIANTS};

    #[test]
    fn command_meanings_choose_the_same_accent_on_every_surface() {
        let accent = |name| resolve(name).accent.map(|i| ACCENT_NAMES[i]);
        for c in solidcraft_engine::command_specs() {
            let want = if c.id.starts_with("surface.") {
                Some("surface")
            } else if c.panel == "CONSTRAINTS" {
                Some("constraint")
            } else if matches!(c.id, "sketch.construction" | "sketch.centerline") || c.id.starts_with("sketch.construction_line") {
                Some("construction")
            } else if c.id.starts_with("sketch.") && matches!(c.panel, "CREATE" | "MODIFY" | "PATTERN" | "FINISH SKETCH") {
                Some("sketch")
            } else if c.id == "file.save_mesh" {
                Some("mesh")
            } else {
                continue;
            };
            assert_eq!(accent(c.icon), want, "{} ({})", c.id, c.icon);
        }
        for id in ["move", "scale", "copy", "delete", "export", "ground", "axis_two_planes"] {
            assert_eq!(accent(id), None, "shared utility / modelling icon {id}");
        }
        for (alias, master) in [("sketch_move", "move"), ("sketch_scale", "scale"), ("surface_fair", "spline_cv"), ("mesh_export", "export")] {
            assert_eq!(resolve(alias).master, resolve(master).master, "reuse accepted shapes");
        }
    }

    #[test]
    fn shipped_icon_palettes_preserve_alpha_shading_and_colour_vision_overrides() {
        crate::test_support::host_data();
        for file in crate::theme_files::builtin() {
            let palette = crate::theme_files::resolve_icon_palette(file, &Default::default()).expect("shipped icon palette");
            // Two colours can't make a pale/medium/strong ramp three distinct shades (1-bit, 1-bit Dark).
            for ramp in palette.accents.into_iter().flatten().filter(|_| !crate::test_support::is_two_colour(&file.id)) {
                // Ordered faces stay visibly distinct without changing accepted paths.
                assert_ne!(ramp[0], ramp[1], "{} pale/medium", file.id);
                assert_ne!(ramp[1], ramp[2], "{} medium/strong", file.id);
            }
            for vision in solidcraft_theme::ColorVision::ALL {
                for name in ["extrude", "surface_extrude", "mesh_export", "line", "c_parallel", "construction"] {
                    let resolved = resolve(name);
                    let plain = Look { dark: file.base.is_dark(), accent: resolved.accent, vision, palette: None };
                    let custom = Look { palette: Some(palette), ..plain };
                    let doc = doc_for(resolved.master, 32).unwrap();
                    let before = render(doc, plain, 32).unwrap();
                    let after = render(doc, custom, 32).unwrap();
                    assert!(before.pixels().iter().zip(after.pixels()).all(|(a, b)| a.alpha() == b.alpha()), "{} {name} alpha", file.id);
                    for (variant, accent, role, light, dark) in ACCENT_VARIANTS {
                        let selected = match vision {
                            solidcraft_theme::ColorVision::Off => "",
                            solidcraft_theme::ColorVision::Protan | solidcraft_theme::ColorVision::Deutan => "red-green",
                            solidcraft_theme::ColorVision::Tritan => "blue-yellow",
                        };
                        if *variant == selected && Some(*accent) == custom.accent {
                            assert_eq!(SET.role_rgb(custom, *role), if file.base.is_dark() { *dark } else { *light });
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn command_artwork_resolves_and_construct_methods_are_distinct() {
        let specs = solidcraft_engine::command_specs();
        for command in &specs {
            assert!(!resolve(command.icon).placeholder, "{}: {}", command.id, command.icon);
        }
        let construction: Vec<_> = specs.iter().filter(|c| c.panel == "CONSTRUCT").collect();
        let distinct: std::collections::HashSet<_> = construction.iter().map(|c| c.icon).collect();
        assert_eq!(construction.len(), 23);
        assert_eq!(construction.iter().find(|c| c.id == "construct.plane.axis_point").unwrap().icon, "plane_axis_point");
        assert_eq!(distinct.len(), construction.len(), "construction methods need distinct visual identities");
    }

    #[test]
    fn sketch_construction_modes_have_distinct_icons() {
        let families: &[&[&str]] = &[
            crate::sketch_families::family("sketch.construction_line").unwrap().commands,
            &["sketch.polygon.inscribed", "sketch.polygon.circumscribed", "sketch.polygon.edge"],
            &["sketch.slot.center_to_center", "sketch.slot.overall", "sketch.slot.center_point"],
            &["sketch.slot.arc_three_point", "sketch.slot.arc_center"],
            &["sketch.chamfer.equal_distance", "sketch.chamfer.distance_angle", "sketch.chamfer.two_distance"],
        ];
        for commands in families {
            let icons: std::collections::HashSet<_> = commands.iter().map(|id| solidcraft_engine::find_command(id).unwrap().icon).collect();
            assert_eq!(icons.len(), commands.len(), "Sketch modes share an icon: {commands:?}");
            for icon in icons {
                assert!(!resolve(icon).placeholder, "{icon}");
            }
        }
    }

    #[test]
    fn contact_actions_have_distinct_icons_from_interference_analysis() {
        let mut icons = std::collections::HashSet::new();
        let interference = solidcraft_engine::find_command("inspect.interference").unwrap().icon;
        for id in ["contact.enable_sets", "contact.enable_all", "contact.disable_all", "contact.create"] {
            let icon = solidcraft_engine::find_command(id).unwrap().icon;
            assert_ne!(icon, interference, "{id} is a contact control, not an interference result");
            assert!(icons.insert(icon), "contact controls share an icon: {id}");
            assert!(!resolve(icon).placeholder, "{id}: {icon}");
        }
    }
}
