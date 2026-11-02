//! Provisional Drawing ribbon metadata, not a drawing-file or persistence API.
//!
//! These entries reserve discoverable homes while the owning Drawing contracts land.
//! JSON execution always refuses; the UI may show reviewed option layouts separately.
//! `drawing.export.planned` keeps the disabled UI action separate from `drawing.export`
//! (the sheet export provider). View previews likewise use `.planned` ids so the
//! real base-view and projected-view providers keep their executable contracts.

use super::CommandSpec;

/// Artwork awaiting owner review. Runtime assignments remain the intentional placeholder.
pub const PENDING_ICONS: &[(&str, &str)] = &[
    ("drawing.view.base.planned", "drawing_base_view"),
    ("drawing.view.projected.planned", "drawing_projected_view"),
    ("drawing.view.section", "drawing_section_view"),
    ("drawing.view.detail", "drawing_detail_view"),
    ("drawing.views.align", "drawing_align_views"),
    ("drawing.image", "drawing_image"),
    ("drawing.resources", "drawing_resources"),
    ("drawing.dimension", "drawing_dimension"),
    ("drawing.hole_callout", "drawing_hole_callout"),
    ("drawing.leader", "drawing_leader"),
    ("drawing.datum", "drawing_datum"),
    ("drawing.centre_mark", "drawing_centre_mark"),
    ("drawing.annotations.align", "drawing_align_annotations"),
    ("drawing.annotations.space", "drawing_space_annotations"),
    ("drawing.parts_list", "drawing_parts_list"),
    ("drawing.balloon", "drawing_balloon"),
    ("drawing.table.edit", "drawing_edit_table"),
    ("drawing.revision_table", "drawing_revision_table"),
    ("drawing.update", "drawing_update"),
    ("drawing.check", "drawing_check"),
    ("drawing.revision", "drawing_revision"),
    ("drawing.print", "drawing_print"),
];

pub fn pending_icon(id: &str) -> Option<&'static str> {
    PENDING_ICONS.iter().find(|(command, _)| *command == id).map(|(_, icon)| *icon)
}

pub static COMMANDS: &[CommandSpec] = &[
    planned!("drawing.view.base.planned", "Base View", "4816").at("DRAWING LAYOUT", "VIEWS").icon("placeholder"),
    planned!("drawing.view.projected.planned", "Projected View", "4816").at("DRAWING LAYOUT", "VIEWS").icon("placeholder"),
    planned!("drawing.view.section", "Section View", "4816").at("DRAWING LAYOUT", "VIEWS").icon("placeholder"),
    planned!("drawing.view.detail", "Detail View", "4816").at("DRAWING LAYOUT", "VIEWS").icon("placeholder"),
    planned!("drawing.views.align", "Align Views", "4894").at("DRAWING LAYOUT", "ARRANGE").icon("placeholder"),
    planned!("drawing.image", "Insert Image", "4894").at("DRAWING LAYOUT", "INSERT").icon("placeholder"),
    planned!("drawing.line", "Drawing Line", "4890").at("DRAWING LAYOUT", "INSERT").icon("line"),
    planned!("drawing.resources", "Browse Drawing Resources", "4894").at("DRAWING LAYOUT", "RESOURCES").icon("placeholder"),
    planned!("drawing.dimension", "Drawing Dimension", "4896").at("DRAWING ANNOTATE", "MEASURE").icon("placeholder"),
    planned!("drawing.hole_callout", "Hole Callout", "4896").at("DRAWING ANNOTATE", "MEASURE").icon("placeholder"),
    planned!("drawing.note", "Drawing Note", "4896").at("DRAWING ANNOTATE", "NOTES").icon("text"),
    planned!("drawing.leader", "Leader Note", "4896").at("DRAWING ANNOTATE", "NOTES").icon("placeholder"),
    planned!("drawing.datum", "Datum", "4896").at("DRAWING ANNOTATE", "SYMBOLS").icon("placeholder"),
    planned!("drawing.centre_mark", "Centre Mark", "4896").at("DRAWING ANNOTATE", "SYMBOLS").icon("placeholder"),
    planned!("drawing.annotations.align", "Align Annotations", "4896").at("DRAWING ANNOTATE", "ARRANGE").icon("placeholder"),
    planned!("drawing.annotations.space", "Space Annotations", "4896").at("DRAWING ANNOTATE", "ARRANGE").icon("placeholder"),
    planned!("drawing.parts_list", "Parts List", "3874").at("DRAWING PARTS", "ASSEMBLY").icon("placeholder"),
    planned!("drawing.balloon", "Balloon", "3874").at("DRAWING PARTS", "ASSEMBLY").icon("placeholder"),
    planned!("drawing.table.edit", "Edit Drawing Table", "4894").at("DRAWING PARTS", "TABLES").icon("placeholder"),
    planned!("drawing.revision_table", "Revision Table", "4894").at("DRAWING PARTS", "TABLES").icon("placeholder"),
    planned!("drawing.update", "Update Drawing", "4892").at("DRAWING REVIEW", "CHANGES").icon("placeholder"),
    planned!("drawing.check", "Check Drawing", "4894").at("DRAWING REVIEW", "CHANGES").icon("placeholder"),
    planned!("drawing.revision", "Add Drawing Revision", "4894").at("DRAWING REVIEW", "REVISION").icon("placeholder"),
    planned!("drawing.export.planned", "Export Drawing", "4890").at("DRAWING REVIEW", "OUTPUT").icon("export"),
    planned!("drawing.print", "Print Drawing", "4890").at("DRAWING REVIEW", "OUTPUT").icon("placeholder"),
];

#[cfg(test)]
mod tests {
    #[test]
    fn drawing_entries_are_metadata_only() {
        let mut session = crate::Session::new(Default::default());
        let before = serde_json::to_value(&session.doc).unwrap();
        for id in ["drawing.export", "drawing.view.base", "drawing.view.projected"] {
            assert!(crate::find_command(id).unwrap().planned.is_none(), "the executable provider {id} stays registered");
        }
        assert!(crate::find_command("drawing.export.planned").unwrap().planned.is_some(), "the scaffold export stays disabled");
        for command in super::COMMANDS {
            assert!(command.planned.is_some(), "{}", command.id);
            assert!(!command.info(&session).enabled, "{}", command.id);
            assert!((command.run)(&mut session, &serde_json::json!({})).is_err(), "{}", command.id);
        }
        assert_eq!(serde_json::to_value(&session.doc).unwrap(), before);
    }
}
