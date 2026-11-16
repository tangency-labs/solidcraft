//! The sheet cache (#3890): each drawing's views' last lines, kept in the design file so a drawing
//! opens showing them. One entry for each drawing, `cache/drawings/<id>.json`, deflated, its id as
//! [`ids::encode`] writes it. Like the model cache it is optional: written only when the file has
//! room, read back only by the build that wrote it (a view's lines come from the projection code as
//! well as the model), and anything wrong with it is a miss, never a failed open. Whether a view's
//! lines are current is not decided here: each view says what it showed, and the engine compares
//! that with the model the design opens with.
use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use solidcore_record::Vec2;
use solidcraft_doc::{ThreadInfo, ids};
use solidcraft_geom::Aabb2;
use solidcraft_geom::paper::{Group, MAX_SIDE_MM, Scene};

use super::*;

const FORMAT: &str = "solidcraft-sheet-cache/1";
const FOLDER: &str = "cache/drawings/";
/// The most one drawing's entry may hold, expanded.
const MAX_ENTRY: usize = 64 << 20;

/// One view's last lines, and what they were drawn from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetView {
    /// The view's id.
    pub view: u64,
    /// The view as it drew, as a number its build makes: lines of a view whose settings changed
    /// since are not its.
    pub resolved: u64,
    /// What it showed, body by body.
    pub shows: Vec<ShownBody>,
    /// Its lines, centred on the view.
    pub group: Arc<Group>,
    pub boundary: Option<Aabb2>,
    /// The middle of the model's box on the view's plane: with the view's own plane and scale, the
    /// frame its lines were drawn through.
    pub model_centre: Vec2,
    /// What it says beside its lines.
    pub why: Option<String>,
}

/// A body a view showed: the occurrences that place it, its definition's body name, whether it is
/// a reference, its geometry's key where it stands, and its cosmetic threads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShownBody {
    pub path: Vec<u64>,
    pub body: String,
    pub reference: bool,
    pub geometry: u64,
    pub threads: Vec<ThreadInfo>,
}

/// A drawing's lines to save: its views now, or the entry its file held, never read.
pub enum SheetLines {
    Views(Vec<SheetView>),
    Kept(Arc<[u8]>),
}

#[derive(Serialize)]
struct Written<'a> {
    build: String,
    format: &'a str,
    views: &'a [SheetView],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    build: String,
    format: String,
    views: Vec<SheetView>,
}

fn name(drawing: u64) -> String {
    format!("{FOLDER}{}.json", ids::encode(drawing))
}

/// Each drawing's entry, while the file has room for it within `budget`.
pub(super) fn write(writer: &mut PackageWriter, drawings: &[(u64, SheetLines)], budget: usize) -> Result<()> {
    for (drawing, lines) in drawings {
        let bytes = match lines {
            SheetLines::Views(views) if views.is_empty() => continue,
            SheetLines::Views(views) => {
                Cow::Owned(serde_json::to_vec(&Written { build: native_cache::build(), format: FORMAT, views }).map_err(invalid)?)
            }
            SheetLines::Kept(bytes) => Cow::Borrowed(&bytes[..]),
        };
        if bytes.len() <= MAX_ENTRY && writer.room(1, bytes.len(), budget) {
            writer.part(&name(*drawing), &bytes, true)?;
        }
    }
    Ok(())
}

/// Each drawing's entry the file holds, unread: a name that isn't a drawing's, or an entry too
/// large or damaged, is left out.
pub(super) fn read<R: Read + Seek>(reader: &mut PackageReader<R>) -> BTreeMap<u64, Arc<[u8]>> {
    let names = reader.part_names();
    let drawing = |name: &str| name.strip_prefix(FOLDER)?.strip_suffix(".json").and_then(ids::decode);
    names.iter().filter_map(|name| Some((drawing(name)?, Arc::from(reader.limited_part(name, MAX_ENTRY).ok()?)))).collect()
}

/// The views of a drawing's entry, when this build wrote it: each that passes a paper scene's checks.
/// None for an entry of another build, or one that doesn't read.
pub fn sheet_views(bytes: &[u8]) -> Vec<SheetView> {
    let Ok(entry) = serde_json::from_slice::<Stored>(bytes) else {
        return Vec::new();
    };
    let trusted = native_cache::source_trusted(env!("SOLIDCRAFT_SOURCE_COMMIT"), cfg!(any(test, feature = "trust-unknown-source")));
    if entry.format != FORMAT || entry.build != native_cache::build() || !trusted {
        return Vec::new();
    }
    entry.views.into_iter().filter(SheetView::checks).collect()
}

impl SheetView {
    /// Its lines pass the checks of a scene on the largest sheet, and its numbers are finite.
    fn checks(&self) -> bool {
        let finite = |p: Vec2| p.x.is_finite() && p.y.is_finite();
        let boundary = self.boundary.is_none_or(|b| finite(b.min) && finite(b.max));
        let side = Vec2::new(MAX_SIDE_MM, MAX_SIDE_MM);
        finite(self.model_centre) && boundary && Scene::new(side, vec![Group::clone(&self.group)]).validate().is_ok()
    }
}
