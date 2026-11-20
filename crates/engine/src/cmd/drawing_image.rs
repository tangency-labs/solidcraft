//! `drawing.image.add` (W38e, #5426): a PNG or JPEG on a sheet, its bytes saved in the design, once
//! however many places it sits. One undo step, and no model is evaluated. A placed image goes with its
//! sheet, and its file goes with the last of them.
//!
//! `drawing.title_block.logo` (#3875): the same file in a title block's text cell, in place of its text.

use std::sync::Arc;

use serde_json::Value;
use solidcraft_doc::drawing::image::{ImageFile, MAX_IMAGE_BYTES, SheetImage};
use solidcraft_doc::ids;

use super::CommandSpec;
use crate::params::bad;
use crate::schema::drawing_image::{AddParams, AddResult, LogoParams, LogoResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const ADD: &str = "drawing.image.add";
const ADD_CONTRACT: TypedContract<AddParams> = TypedContract::new();
const LOGO: &str = "drawing.title_block.logo";
const LOGO_CONTRACT: TypedContract<LogoParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new(ADD, "Add Image", crate::checked!("drawing.image.add", add))
        .icon("new")
        .in_drawing()
        .doc_only()
        .contract(&ADD_CONTRACT)
        .output(output_schema_for::<AddResult>)
        .params(
            "sheet: id or name; file: a PNG or JPEG, at most 4 MiB and 64 million pixels, read from the file's header before anything decodes it; \
             rect_mm: [x0, y0, x1, y1] from the sheet's lower-left corner → {image, format, pixels}. Its bytes are saved in the design, once however many places it \
             sits, and its path isn't. image is the placement's id. At most 9 MiB of image files a drawing. One undo step; no model is evaluated.",
        ),
    CommandSpec::new(LOGO, "Title Block Logo", crate::checked!("drawing.title_block.logo", logo))
        .icon("new")
        .in_drawing()
        .doc_only()
        .contract(&LOGO_CONTRACT)
        .output(output_schema_for::<LogoResult>)
        .params(
            "cell: a text cell's name in a sheet's title block; file: a PNG or JPEG as drawing.image.add takes it, shown in place of the cell's text, or null for the text \
             again; sheets?: ids or names (every sheet whose block has the cell) → {sheets, image: the file's id or null}. One file however many sheets show it, and it \
             goes with the last of them. One undo step; no model is evaluated.",
        ),
];

/// The image file at the path `file`, as `id`, when its caps allow it.
fn read_file(cmd: &str, file: &str, id: u64) -> Result<ImageFile> {
    if file.trim().is_empty() || file.len() >= 4096 {
        return Err(bad(cmd, "`file` is the path of a PNG or JPEG"));
    }
    let bytes = solidcraft_io::vfs::read(file, MAX_IMAGE_BYTES).map_err(|e| EngineError::Other(format!("{file}: {e}")))?;
    ImageFile::from_file(id, &bytes).map_err(|e| bad(cmd, format!("{file}: {e}")))
}

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let request: AddParams = crate::schema::read(ADD, p)?;
    let record = s.drawing(ADD)?.clone();
    let sheet = crate::drawing_scene::sheet_id(&record, &request.sheet, ADD)?;
    let (id, new_file) = (record.allocate_id(&mut s.ids), record.allocate_id(&mut s.ids));
    let id = id.map_err(|e| bad(ADD, e.to_string()))?;
    let new_file = new_file.map_err(|e| bad(ADD, e.to_string()))?;
    let image = read_file(ADD, &request.file, new_file)?;
    let reply = AddResult { image: ids::encode(id), format: image.format.clone(), pixels: image.pixels, param_warnings: Vec::new() };
    let rect_mm = request.rect_mm;
    s.edit_drawing(ADD, |d| {
        let shown = d.keep_file(image);
        d.sheet_images.add([Arc::new(SheetImage { id, sheet, rect_mm, image: shown })]);
        // Every drawing saved opens again, so one that couldn't be saved is refused here, not at the save.
        d.to_json().map_err(|e| bad(ADD, e.to_string()))?;
        Ok(())
    })?;
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

fn logo(s: &mut Session, p: &Value) -> Result<Value> {
    let request: LogoParams = crate::schema::read(LOGO, p)?;
    let record = s.drawing(LOGO)?.clone();
    let named = request
        .sheets
        .as_ref()
        .map(|sheets| sheets.iter().map(|sheet| crate::drawing_scene::sheet_id(&record, sheet, LOGO)).collect::<Result<Vec<u64>>>());
    let named = named.transpose()?;
    let file = match &request.file {
        Some(file) => {
            let id = record.allocate_id(&mut s.ids).map_err(|e| bad(LOGO, e.to_string()))?;
            Some(read_file(LOGO, file, id)?)
        }
        None => None,
    };
    let (sheets, image) = s.edit_drawing(LOGO, |d| {
        let shown = d.set_logo(named.as_deref(), &request.cell, file).map_err(|e| bad(LOGO, e.to_string()))?;
        d.to_json().map_err(|e| bad(LOGO, e.to_string()))?;
        Ok(shown)
    })?;
    let reply = LogoResult { sheets: sheets.into_iter().map(ids::encode).collect(), image: image.map(ids::encode), param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

#[cfg(test)]
#[path = "drawing_image_tests.rs"]
mod tests;
