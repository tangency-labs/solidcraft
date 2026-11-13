//! Provisional Drawing ribbon metadata, not a drawing-file or persistence API.
//!
//! These entries reserve discoverable homes while the owning Drawing contracts land.
//! JSON execution always refuses; the UI may show reviewed option layouts separately.
//! `drawing.export.planned` keeps the disabled UI action separate from `drawing.export`
//! (the sheet export provider). View previews likewise use `.planned` ids so the
//! real base-view and projected-view providers keep their executable contracts.

use super::CommandSpec;

/// Original Drawing artwork, tracked for owner review.
pub const PENDING_ICONS: &[(&str, &str)] = &[
    ("drawing.annotations.align", "drawing_align_annotations"),
    ("drawing.annotations.space", "drawing_space_annotations"),
    ("drawing.balloon", "drawing_balloon"),
    ("drawing.balloon.auto", "drawing_auto_balloon"),
    ("drawing.centre_mark", "drawing_centre_mark"),
    ("drawing.check.planned", "drawing_check"),
    ("drawing.datum", "drawing_datum"),
    ("drawing.dimension", "drawing_dimension"),
    ("drawing.dimension.chain", "drawing_dimension_chain"),
    ("drawing.dimension.from_model.planned", "drawing_model_dimensions"),
    ("drawing.dimension.suggest", "drawing_dimension_suggest"),
    ("drawing.dimension.tidy", "drawing_dimension_tidy"),
    ("drawing.gdt", "drawing_gdt"),
    ("drawing.hatch", "drawing_hatch"),
    ("drawing.hole_callout", "drawing_hole_callout"),
    ("drawing.image", "drawing_image"),
    ("drawing.leader", "drawing_leader"),
    ("drawing.note", "drawing_note"),
    ("drawing.note.chamfer", "drawing_chamfer_note"),
    ("drawing.parts_list", "drawing_parts_list"),
    ("drawing.parts_list.renumber", "drawing_parts_renumber"),
    ("drawing.print", "drawing_print"),
    ("drawing.replace_model", "drawing_replace_model"),
    ("drawing.resources", "drawing_resources"),
    ("drawing.revise.planned", "drawing_issue_revision"),
    ("drawing.revision", "drawing_revision"),
    ("drawing.revision.tag.add.planned", "drawing_revision_tag"),
    ("drawing.revision_table", "drawing_revision_table"),
    ("drawing.sheet_sketch", "drawing_sheet_sketch"),
    ("drawing.surface_texture", "drawing_surface_texture"),
    ("drawing.table.edit.planned", "drawing_edit_table"),
    ("drawing.update.planned", "drawing_update"),
    ("drawing.view.auxiliary", "drawing_view_auxiliary"),
    ("drawing.view.base.planned", "drawing_base_view"),
    ("drawing.view.break", "drawing_view_break"),
    ("drawing.view.copy_to_sheet.planned", "drawing_view_copy_to_sheet"),
    ("drawing.view.detail", "drawing_detail_view"),
    ("drawing.view.draft", "drawing_view_draft"),
    ("drawing.view.flat_pattern", "drawing_view_flat_pattern"),
    ("drawing.view.move_to_sheet.planned", "drawing_view_move_to_sheet"),
    ("drawing.view.pictorial", "drawing_view_pictorial"),
    ("drawing.view.projected.planned", "drawing_projected_view"),
    ("drawing.view.section", "drawing_section_view"),
    ("drawing.views.align", "drawing_align_views"),
    ("drawing.weld", "drawing_weld"),
];

pub fn pending_icon(id: &str) -> Option<&'static str> {
    PENDING_ICONS.iter().find(|(command, _)| *command == id).map(|(_, icon)| *icon)
}

pub static COMMANDS: &[CommandSpec] = &[
    planned!("drawing.view.base.planned", "Base View", "4816").at("DRAWING LAYOUT", "VIEWS").icon("drawing_base_view"),
    planned!("drawing.view.projected.planned", "Projected View", "4816").icon("drawing_projected_view"),
    planned!("drawing.view.section", "Section View", "4816").at("DRAWING LAYOUT", "VIEWS").icon("drawing_section_view"),
    planned!("drawing.view.detail", "Detail View", "4816").at("DRAWING LAYOUT", "VIEWS").icon("drawing_detail_view"),
    planned!("drawing.views.align", "Align Views", "4894").at("DRAWING LAYOUT", "ARRANGE").icon("drawing_align_views"),
    planned!("drawing.image", "Insert Image", "4894").at("DRAWING LAYOUT", "INSERT").icon("drawing_image"),
    planned!("drawing.line", "Drawing Line", "4890").icon("line"),
    planned!("drawing.resources", "Browse Drawing Resources", "4894").icon("drawing_resources"),
    planned!("drawing.dimension", "Drawing Dimension", "4896").at("DRAWING ANNOTATE", "MEASURE").icon("drawing_dimension"),
    planned!("drawing.hole_callout", "Hole Callout", "4896").at("DRAWING ANNOTATE", "MEASURE").icon("drawing_hole_callout"),
    planned!("drawing.note", "Drawing Note", "4896").at("DRAWING ANNOTATE", "NOTES").icon("drawing_note"),
    planned!("drawing.leader", "Leader Note", "4896").at("DRAWING ANNOTATE", "NOTES").icon("drawing_leader"),
    planned!("drawing.datum", "Datum", "4896").icon("drawing_datum"),
    planned!("drawing.centre_mark", "Centre Mark", "4896").at("DRAWING ANNOTATE", "SYMBOLS").icon("drawing_centre_mark"),
    planned!("drawing.annotations.align", "Align Annotations", "4896").at("DRAWING ANNOTATE", "ARRANGE").icon("drawing_align_annotations"),
    planned!("drawing.annotations.space", "Space Annotations", "4896").at("DRAWING ANNOTATE", "ARRANGE").icon("drawing_space_annotations"),
    planned!("drawing.parts_list", "Parts List", "3874").at("DRAWING PARTS", "ASSEMBLY").icon("drawing_parts_list"),
    planned!("drawing.balloon", "Balloon", "3874").at("DRAWING PARTS", "ASSEMBLY").icon("drawing_balloon"),
    planned!("drawing.revision_table", "Revision Table", "4894").at("DRAWING PARTS", "TABLES").icon("drawing_revision_table"),
    planned!("drawing.update.planned", "Update Drawing", "4892").at("DRAWING REVIEW", "CHANGES").icon("drawing_update"),
    planned!("drawing.check.planned", "Check Drawing", "4894").at("DRAWING REVIEW", "CHANGES").icon("drawing_check"),
    planned!("drawing.revision", "Add Drawing Revision", "4894").at("DRAWING REVIEW", "REVISION").icon("drawing_revision"),
    planned!("drawing.export.planned", "Export Drawing", "4890").at("DRAWING REVIEW", "OUTPUT").icon("export"),
    planned!("drawing.print", "Print Drawing", "4890").at("DRAWING REVIEW", "OUTPUT").icon("drawing_print"),
    planned!("drawing.view.break", "Break View", "7578").at("DRAWING LAYOUT", "VIEWS").icon("drawing_view_break"),
    planned!("drawing.view.auxiliary", "Auxiliary View", "13044").at("DRAWING LAYOUT", "VIEWS").icon("drawing_view_auxiliary"),
    planned!("drawing.view.pictorial", "Pictorial View", "13044").at("DRAWING LAYOUT", "VIEWS").icon("drawing_view_pictorial"),
    planned!("drawing.view.draft", "Draft View", "4894").at("DRAWING LAYOUT", "VIEWS").icon("drawing_view_draft"),
    planned!("drawing.view.flat_pattern", "Flat Pattern View", "13030").at("DRAWING LAYOUT", "VIEWS").icon("drawing_view_flat_pattern"),
    planned!("drawing.view.move_to_sheet.planned", "Move to sheet", "4894").at("DRAWING LAYOUT", "ARRANGE").icon("drawing_view_move_to_sheet"),
    planned!("drawing.view.copy_to_sheet.planned", "Copy to sheet", "4894").at("DRAWING LAYOUT", "ARRANGE").icon("drawing_view_copy_to_sheet"),
    planned!("drawing.sheet_sketch", "Sheet Sketch", "5425").at("DRAWING LAYOUT", "INSERT").icon("drawing_sheet_sketch"),
    planned!("drawing.hatch", "Hatch", "7062").at("DRAWING LAYOUT", "INSERT").icon("drawing_hatch"),
    planned!("drawing.dimension.from_model.planned", "Model Dimensions", "13054").at("DRAWING ANNOTATE", "MEASURE").icon("drawing_model_dimensions"),
    planned!("drawing.dimension.chain", "Chain", "4896").at("DRAWING ANNOTATE", "MEASURE").icon("drawing_dimension_chain"),
    planned!("drawing.dimension.suggest", "Suggest", "4896").at("DRAWING ANNOTATE", "MEASURE").icon("drawing_dimension_suggest"),
    planned!("drawing.note.chamfer", "Chamfer Note", "13028").at("DRAWING ANNOTATE", "NOTES").icon("drawing_chamfer_note"),
    planned!("drawing.gdt", "GD&T", "7257").at("DRAWING ANNOTATE", "SYMBOLS").icon("drawing_gdt"),
    planned!("drawing.surface_texture", "Surface Texture", "7258").at("DRAWING ANNOTATE", "SYMBOLS").icon("drawing_surface_texture"),
    planned!("drawing.weld", "Weld", "7259").at("DRAWING ANNOTATE", "SYMBOLS").icon("drawing_weld"),
    planned!("drawing.dimension.tidy", "Tidy", "4896").at("DRAWING ANNOTATE", "ARRANGE").icon("drawing_dimension_tidy"),
    planned!("drawing.balloon.auto", "Auto Balloon", "13067").at("DRAWING PARTS", "ASSEMBLY").icon("drawing_auto_balloon"),
    planned!("drawing.parts_list.renumber", "Renumber", "3874").at("DRAWING PARTS", "ASSEMBLY").icon("drawing_parts_renumber"),
    planned!("drawing.table.edit.planned", "Edit Table", "3874").at("DRAWING PARTS", "TABLES").icon("drawing_edit_table"),
    planned!("drawing.replace_model", "Replace Model", "13029").at("DRAWING REVIEW", "CHANGES").icon("drawing_replace_model"),
    planned!("drawing.revise.planned", "Issue Revision", "3874").at("DRAWING REVIEW", "REVISION").icon("drawing_issue_revision"),
    planned!("drawing.revision.tag.add.planned", "Revision Tag", "3874").at("DRAWING REVIEW", "REVISION").icon("drawing_revision_tag"),
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
