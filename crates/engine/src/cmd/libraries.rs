//! Local material libraries: inspect/export files and copy exact revisions into a design, and
//! the library places. The app keeps the places: like `settings.profile.*`, the place commands
//! check a change and echo it, and the app applies it (`library_places` in the UI crate).
use super::CommandSpec;
use crate::schema::libraries::{
    LibraryApplied, LibraryApplyParams, LibraryExportParams, LibraryExported, LibraryInspectParams, LibraryInspected, LibraryPlaceChange,
    LibraryPlacesAddParams, LibraryPlacesListParams, LibraryPlacesListed, LibraryPlacesRefreshParams, LibraryPlacesRefreshed,
    LibraryPlacesRemoveParams, LibraryPlacesSetParams, LibraryTargets, ListedLibrary, ListedPlace,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session, params::bad};
use serde_json::{Value, json};
use solidcraft_io::catalog::{Catalog, Definition};
use solidcraft_io::library::{self, Library, Place, PlaceEdit, PlaceRow, RowState, Scanned, Status};
use solidcraft_io::vfs::{self, Remote};

const INSPECT_CONTRACT: TypedContract<LibraryInspectParams> = TypedContract::new();
const EXPORT_CONTRACT: TypedContract<LibraryExportParams> = TypedContract::new();
const APPLY_CONTRACT: TypedContract<LibraryApplyParams> = TypedContract::new();
const PLACES_LIST_CONTRACT: TypedContract<LibraryPlacesListParams> = TypedContract::new();
const PLACES_ADD_CONTRACT: TypedContract<LibraryPlacesAddParams> = TypedContract::new();
const PLACES_REMOVE_CONTRACT: TypedContract<LibraryPlacesRemoveParams> = TypedContract::new();
const PLACES_SET_CONTRACT: TypedContract<LibraryPlacesSetParams> = TypedContract::new();
const PLACES_REFRESH_CONTRACT: TypedContract<LibraryPlacesRefreshParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("library.inspect", "Inspect Local Library", crate::checked!("library.inspect", inspect)).icon("inspect").noundo().doc_only()
        .contract(&INSPECT_CONTRACT)
        .output(output_schema_for::<LibraryInspected>)
        .params("path: v1/v2 library JSON; returns validated catalog and exact entry revisions, without changing the design"),
    CommandSpec::new("library.export", "Export Design Library", crate::checked!("library.export", export)).icon("export").noundo().doc_only()
        .contract(&EXPORT_CONTRACT)
        .output(output_schema_for::<LibraryExported>)
        .params("path: .json; id? (default user.local), name?, provenance?, overwrite? false, format? solidcraft-library/2 (default) | solidcraft-library/1 (refuses PBR/cards). Copies assigned looks and design-local physical definitions; never changes the design"),
    CommandSpec::new("library.apply", "Apply Local Library Entry", crate::checked!("library.apply", apply)).icon("import").enabled(super::not_in_sketch)
        .contract(&APPLY_CONTRACT)
        .output(output_schema_for::<LibraryApplied>)
        .params("path, entry: qualified ID, revision: exact content SHA-256; targets?: {bodies?, faces?: [{body,point}], components?}; name?: physical material name on import. Copies values in one undo step; material conflicts are explicit"),
    CommandSpec::new("library.places.list", "List Library Places", crate::checked!("library.places.list", places_list)).icon("list").noundo().doc_only()
        .contract(&PLACES_LIST_CONTRACT)
        .output(output_schema_for::<LibraryPlacesListed>)
        .params("→ places: [{kind, path, read_only, my_library, scan, why, libraries}], the built-in library first; reads no files"),
    CommandSpec::new("library.places.add", "Add Library Place", crate::checked!("library.places.add", places_add)).icon("add").noundo().doc_only()
        .contract(&PLACES_ADD_CONTRACT)
        .output(output_schema_for::<LibraryPlaceChange>)
        .params("path: absolute folder, kind: library | folder, read_only? (default: true for a network folder), at? (default: the end); the app adds it"),
    CommandSpec::new("library.places.remove", "Remove Library Place", crate::checked!("library.places.remove", places_remove)).icon("delete").noundo().doc_only()
        .contract(&PLACES_REMOVE_CONTRACT)
        .output(output_schema_for::<LibraryPlaceChange>)
        .params("path: a listed place; its files are left as they are; the app removes it"),
    CommandSpec::new("library.places.set", "Change Library Place", crate::checked!("library.places.set", places_set)).icon("settings").noundo().doc_only()
        .contract(&PLACES_SET_CONTRACT)
        .output(output_schema_for::<LibraryPlaceChange>)
        .params("path: a listed place, read_only?, at?: its new position; the app changes it"),
    CommandSpec::new("library.places.refresh", "Rescan Library Places", crate::checked!("library.places.refresh", places_refresh)).icon("refresh").noundo().doc_only()
        .contract(&PLACES_REFRESH_CONTRACT)
        .output(output_schema_for::<LibraryPlacesRefreshed>)
        .params("path?: a listed place (default: every place); the app scans it again"),
];

impl LibraryTargets {
    fn validate(&self) -> Result<()> {
        let cmd = "library.apply";
        if self.bodies.len().saturating_add(self.faces.len()).saturating_add(self.components.len()) > 1024 {
            return Err(bad(cmd, "at most 1024 library application targets"));
        }
        for name in self.bodies.iter().chain(&self.components).chain(self.faces.iter().map(|f| &f.body)) {
            if name.is_empty() || name.len() > 1024 || name.chars().any(char::is_control) {
                return Err(bad(cmd, "invalid library target name"));
            }
        }
        if self.faces.iter().any(|f| f.point.iter().any(|x| !x.is_finite() || x.abs() > 1e9)) {
            return Err(bad(cmd, "invalid library target point"));
        }
        Ok(())
    }
    fn empty(&self) -> bool {
        self.bodies.is_empty() && self.faces.is_empty() && self.components.is_empty()
    }
}
fn path(cmd: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(bad(cmd, "invalid library path"));
    }
    solidcraft_io::vfs::check(value).map_err(|e| bad(cmd, e.to_string()))
}
fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}
fn nested(s: &mut Session, id: &str, p: &Value) -> Result<Value> {
    crate::find_command(id).ok_or_else(|| bad("library.apply", "required assignment command is unavailable"))?.run_nested(s, p)
}
fn inspect(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "library.inspect";
    let request: LibraryInspectParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    path(cmd, &request.path)?;
    let catalog = Catalog::read(&request.path).map_err(|e| bad(cmd, e.to_string()))?;
    reply(cmd, LibraryInspected { catalog, param_warnings: Vec::new() })
}
fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "library.export";
    let request: LibraryExportParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    path(cmd, &request.path)?;
    if !request.path.to_ascii_lowercase().ends_with(".json") {
        return Err(bad(cmd, "library export path must end in .json"));
    }
    let catalog = Catalog::from_document(&s.doc, &request.id, &request.name, request.provenance)
        .and_then(|catalog| catalog.in_format(&request.format))
        .map_err(|e| bad(cmd, e.to_string()))?;
    let bytes = catalog.encode().map_err(|e| bad(cmd, e.to_string()))?;
    #[cfg(not(target_arch = "wasm32"))]
    let result = if request.overwrite {
        solidcraft_io::write_atomic(std::path::Path::new(&request.path), &bytes, false)
    } else {
        solidcraft_io::safe_write::create_atomic(std::path::Path::new(&request.path), &bytes)
    };
    #[cfg(target_arch = "wasm32")]
    let result = if !request.overwrite && solidcraft_io::vfs::exists(&request.path) {
        Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "library file already exists"))
    } else {
        solidcraft_io::vfs::write(&request.path, &bytes)
    };
    result.map_err(|e| bad(cmd, format!("could not export library: {e}")))?;
    let entries = catalog.entries.len();
    reply(cmd, LibraryExported { path: request.path, entries, bytes: bytes.len(), catalog, param_warnings: Vec::new() })
}
fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "library.apply";
    let request: LibraryApplyParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    path(cmd, &request.path)?;
    request.targets.validate()?;
    if request.entry.len() > 129 || request.revision.len() != 64 {
        return Err(bad(cmd, "invalid entry ID or revision"));
    }
    let catalog = Catalog::read(&request.path).map_err(|e| bad(cmd, e.to_string()))?;
    let entry = catalog.entries.iter().find(|entry| entry.id == request.entry).ok_or_else(|| bad(cmd, "library entry is missing"))?;
    if entry.revision != request.revision {
        return Err(bad(cmd, "library entry changed; reload and explicitly select its new revision"));
    }
    let result = match &entry.definition {
        Definition::Appearance(look) => {
            if request.name.is_some() || request.targets.empty() {
                return Err(bad(cmd, "appearance entries need targets and do not take a material name"));
            }
            let mut target = serde_json::to_value(&request.targets).map_err(|e| bad(cmd, e.to_string()))?;
            target["name"] = json!(look.name);
            target["color"] = json!(look.color);
            target["opacity"] = json!(look.opacity);
            // A catalog replaces the complete optical value. Null explicitly clears
            // inherited PBR. Its label is inert, even if it matches a starter ID.
            target["pbr"] = json!(look.pbr);
            nested(s, "appearance.assign", &target)?
        }
        Definition::Material(material) => {
            if !request.targets.faces.is_empty() || !request.targets.components.is_empty() {
                return Err(bad(cmd, "physical material assignment takes body targets only"));
            }
            if request.targets.bodies.iter().any(|body| s.model.state().body(body).is_none()) {
                return Err(bad(cmd, "physical material targets must name existing bodies"));
            }
            let wanted = request.name.as_deref().unwrap_or(&material.name).trim();
            if wanted.is_empty() || wanted.len() > 128 || wanted.chars().any(char::is_control) {
                return Err(bad(cmd, "invalid physical material name"));
            }
            let existing = s.doc.user_materials.iter().find(|m| m.name.eq_ignore_ascii_case(wanted)).cloned();
            let name = existing.as_ref().map_or(wanted, |m| m.name.as_str()).to_string();
            let copy = material.to_material(name.clone());
            let reused = if let Some(existing) = existing {
                if existing != copy {
                    return Err(bad(cmd, "physical material name conflicts; choose a new name explicitly"));
                }
                true
            } else {
                let mut params = json!({"name":name,"density_g_cm3":material.density_g_cm3});
                if let Some(look) = &material.appearance {
                    params["color"] = json!(look.color);
                    params["opacity"] = json!(look.opacity);
                    params["pbr"] = json!(look.pbr);
                }
                if let Some(card) = &material.physical_card {
                    params["physical_card"] = json!(card);
                }
                nested(s, "material.create", &params)?;
                // The existing colour API labels a custom look after its material.
                // Preserve the library's exact look label inside this same transaction.
                let imported = s
                    .doc_mut()
                    .user_materials
                    .iter_mut()
                    .find(|m| m.name == name)
                    .ok_or_else(|| bad(cmd, "imported physical material is missing"))?;
                imported.appearance = copy.appearance.clone();
                false
            };
            if !request.targets.bodies.is_empty() {
                nested(s, "material.assign", &json!({"bodies":request.targets.bodies,"material":name}))?;
            }
            json!({"material":name,"reused":reused,"bodies":request.targets.bodies})
        }
    };
    reply(cmd, LibraryApplied { entry: entry.id.clone(), revision: entry.revision.clone(), copied: true, result, param_warnings: Vec::new() })
}

fn parse<T: serde::de::DeserializeOwned>(cmd: &str, p: &Value) -> Result<T> {
    serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))
}

/// The folder of a new place: a library path ([`path`]) that is a place's folder by io's rule.
fn place_path(cmd: &str, value: &str) -> Result<()> {
    path(cmd, value)?;
    library::place_refusal(value).map_or(Ok(()), |why| Err(bad(cmd, why)))
}

/// Checks `edit` against the session's places with the rule the app applies it by.
fn check_edit(s: &Session, cmd: &str, edit: &PlaceEdit) -> Result<()> {
    s.libraries.edit(edit).map(drop).map_err(|e| bad(cmd, e))
}

fn places_list(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "library.places.list";
    parse::<LibraryPlacesListParams>(cmd, p)?;
    let set = s.libraries.set();
    // The set holds the built-in scan, then one scan per scanned place in place order, and one
    // status per library in that same order.
    let mut scans = set.places().iter();
    let mut statuses = set.libraries().map(|(_, status)| status);
    let mut listed = |scan: &Scanned| -> Vec<ListedLibrary> {
        scan.libraries().iter().zip(statuses.by_ref()).map(|(l, status)| listed_library(l, status)).collect()
    };
    let mut places = Vec::new();
    if let Some(built_in) = scans.next() {
        let why = unavailable(built_in.status());
        places.push(ListedPlace {
            kind: "built_in".into(),
            path: None,
            read_only: true,
            my_library: false,
            scan: "scanned".into(),
            why,
            libraries: listed(built_in),
        });
    }
    let has_mine = s.libraries.my_library().is_some();
    for (i, (row, state)) in s.libraries.places().enumerate() {
        let (scan, why, libraries) = match state {
            RowState::Scanned => {
                let found = scans.next().ok_or_else(|| bad(cmd, "the session's library places are out of step with their scans"))?;
                ("scanned", unavailable(found.status()), listed(found))
            }
            RowState::Scanning => ("scanning", None, Vec::new()),
            RowState::NotScanned(why) => ("not_scanned", Some(why.clone()), Vec::new()),
        };
        let kind = match row.place {
            Place::BuiltIn => "built_in",
            Place::Library(_) => "library",
            Place::Folder(_) => "folder",
        };
        let path = row.place.path().map(str::to_string);
        places.push(ListedPlace {
            kind: kind.into(),
            path,
            read_only: row.read_only,
            my_library: has_mine && i == 0,
            scan: scan.into(),
            why,
            libraries,
        });
    }
    reply(cmd, LibraryPlacesListed { places, param_warnings: Vec::new() })
}

fn unavailable(status: &Status) -> Option<String> {
    match status {
        Status::Available => None,
        Status::Unavailable(reason) => Some(reason.to_string()),
    }
}

fn listed_library(library: &Library, status: &Status) -> ListedLibrary {
    ListedLibrary {
        id: library.id().map(str::to_string),
        name: library.name().map(str::to_string),
        folder: library.folder().map(str::to_string),
        available: *status == Status::Available,
        why: unavailable(status).or_else(|| library.read_only().map(ToString::to_string)),
        entries: library.entries().len(),
        refused: library.entries().iter().filter(|e| e.entry().is_err()).count(),
    }
}

fn places_add(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "library.places.add";
    let request: LibraryPlacesAddParams = parse(cmd, p)?;
    place_path(cmd, &request.path)?;
    let place = match request.kind.as_str() {
        "library" => Place::Library(request.path.clone()),
        "folder" => Place::Folder(request.path.clone()),
        other => return Err(bad(cmd, format!("`kind` is `library` or `folder`, not `{other}`"))),
    };
    // A network folder is read-only unless the user says otherwise.
    let read_only = request.read_only.unwrap_or(matches!(vfs::remote(&request.path), Some(Remote::Network(_))));
    check_edit(s, cmd, &PlaceEdit::Add { row: PlaceRow { place, read_only }, at: request.at })?;
    reply(
        cmd,
        LibraryPlaceChange { path: request.path, kind: Some(request.kind), read_only: Some(read_only), at: request.at, param_warnings: Vec::new() },
    )
}

fn places_remove(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "library.places.remove";
    let request: LibraryPlacesRemoveParams = parse(cmd, p)?;
    path(cmd, &request.path)?;
    check_edit(s, cmd, &PlaceEdit::Remove { path: request.path.clone() })?;
    reply(cmd, LibraryPlaceChange { path: request.path, kind: None, read_only: None, at: None, param_warnings: Vec::new() })
}

fn places_set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "library.places.set";
    let request: LibraryPlacesSetParams = parse(cmd, p)?;
    path(cmd, &request.path)?;
    check_edit(s, cmd, &PlaceEdit::Set { path: request.path.clone(), read_only: request.read_only, at: request.at })?;
    reply(cmd, LibraryPlaceChange { path: request.path, kind: None, read_only: request.read_only, at: request.at, param_warnings: Vec::new() })
}

fn places_refresh(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "library.places.refresh";
    let request: LibraryPlacesRefreshParams = parse(cmd, p)?;
    if let Some(path) = &request.path {
        // A refresh never adds a place: only a listed one, My Library included, is scanned again.
        if s.libraries.listed(path).is_none() {
            return Err(bad(cmd, format!("no place is listed at {path}")));
        }
    }
    reply(cmd, LibraryPlacesRefreshed { path: request.path, param_warnings: Vec::new() })
}

#[cfg(test)]
#[path = "libraries_tests.rs"]
mod tests;
