//! SolidCraft's message ids checked when a crate compiles.
//!
//! A message is looked up by its id (`app.tr("ui.file")`, `Locale::text`, `Locale::format`), and
//! an id the catalogs do not have compiles: the window then shows the id itself. [`msg!`] moves
//! that discovery to the compiler. It takes the id as a string literal, looks it up in the English
//! catalog and expands to the same literal:
//!
//! ```ignore
//! app.tr(msg!("ui.file"))
//! locale.format(msg!("recovery.days_ago", count), &[("count", &days)])
//! ```
//!
//! - `msg!("id")`: the id is a message of the English catalog.
//! - `msg!("id", name, ...)`: and the names are exactly the message's `{placeholders}`.
//!
//! ```
//! use solidcraft_ui_text_macros::msg;
//! assert_eq!(msg!("ui.file"), "ui.file");
//! assert_eq!(msg!("recovery.days_ago", count), "recovery.days_ago");
//! ```
//!
//! An id no catalog has does not compile, nor do names that are not the message's:
//!
//! ```compile_fail
//! let _ = solidcraft_ui_text_macros::msg!("ui.no_message_has_this_id");
//! ```
//!
//! ```compile_fail
//! let _ = solidcraft_ui_text_macros::msg!("recovery.days_ago", days);
//! ```
//!
//! ```compile_fail
//! // `{count}` is not named.
//! let _ = solidcraft_ui_text_macros::msg!("recovery.days_ago",);
//! ```
//!
//! Anything else stops the build at the literal, with the nearest id when there is one. Only the
//! English catalog is read: whether another language has the message stays with
//! `cargo xtask i18n` and the catalogs' tests, so a new English message is never held up by a
//! language its author cannot write.
//!
//! The English catalog is what the app builds it from (`crates/ui-egui/build.rs`):
//! `assets/locales/en.json`, `crates/ui-text/locales/en.json` and the `en` messages of
//! `assets/locales/pending/*.json`, found from the compiling crate's folder upwards. The macro
//! only checks, and embeds nothing. Cargo does not know that a proc macro read a file, so a crate
//! that uses `msg!` names the catalog folders in its build script (`cargo:rerun-if-changed`), as
//! `solidcraft-ui-egui` does, to be compiled again when a message is added.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, LitStr, Token};

/// The English catalog: message id to text.
type Catalog = BTreeMap<String, String>;

/// How far up from the compiling crate the workspace root is looked for.
const MAX_ANCESTORS: usize = 8;
/// Ids nearer than this many edits are offered as the one meant.
const MAX_EDITS: usize = 3;

/// `"id"`, then the placeholder names, if any.
struct Input {
    id: LitStr,
    names: Option<Vec<Ident>>,
}

impl Parse for Input {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let id: LitStr = input.parse()?;
        if input.is_empty() {
            return Ok(Input { id, names: None });
        }
        input.parse::<Token![,]>()?;
        let names = Punctuated::<Ident, Token![,]>::parse_terminated(input)?;
        Ok(Input { id, names: Some(names.into_iter().collect()) })
    }
}

/// A message id of the English catalog, as a string literal: see the crate's documentation.
#[proc_macro]
pub fn msg(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as Input);
    match check(&input) {
        Ok(()) => {
            let id = &input.id;
            quote!(#id).into()
        }
        Err(e) => e.to_compile_error().into(),
    }
}

fn check(input: &Input) -> syn::Result<()> {
    let catalog = catalog().as_ref().map_err(|why| syn::Error::new(input.id.span(), why))?;
    let id = input.id.value();
    let Some(text) = catalog.get(&id) else {
        let hint = nearest(catalog, &id).map(|near| format!("; did you mean `{near}`?")).unwrap_or_default();
        let message = format!(
            "`{id}` is not a message id of the English catalog (assets/locales/en.json, crates/ui-text/locales/en.json or a file of assets/locales/pending){hint}"
        );
        return Err(syn::Error::new(input.id.span(), message));
    };
    let Some(names) = &input.names else { return Ok(()) };
    let wanted = placeholders(text);
    let given: Vec<String> = names.iter().map(Ident::to_string).collect();
    if let Some(extra) = names.iter().find(|name| !wanted.contains(&name.to_string())) {
        return Err(syn::Error::new(extra.span(), format!("`{id}` has no placeholder `{{{extra}}}`: its text is {text:?}")));
    }
    if let Some(missing) = wanted.iter().find(|name| !given.contains(name)) {
        return Err(syn::Error::new(input.id.span(), format!("`{id}` also has the placeholder `{{{missing}}}`: its text is {text:?}")));
    }
    Ok(())
}

/// The names in `{braces}` in `text`, each once, in order. A name is letters, digits and `_`;
/// anything else between braces is text, not a placeholder.
fn placeholders(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('}') else { break };
        let name = &rest[..close];
        if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            if !names.iter().any(|n| n == name) {
                names.push(name.to_owned());
            }
            rest = &rest[close + 1..];
        }
    }
    names
}

/// The id of `catalog` nearest to `id`, when one is within [`MAX_EDITS`] edits.
fn nearest<'a>(catalog: &'a Catalog, id: &str) -> Option<&'a str> {
    catalog
        .keys()
        .filter(|known| known.len().abs_diff(id.len()) <= MAX_EDITS)
        .map(|known| (edits(known, id), known.as_str()))
        .filter(|(distance, _)| *distance <= MAX_EDITS)
        .min()
        .map(|(_, known)| known)
}

/// The edit distance between `a` and `b` (insertions, deletions and substitutions).
fn edits(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row.first().copied().unwrap_or(0);
        if let Some(first) = row.first_mut() {
            *first = i + 1;
        }
        for (j, cb) in b.iter().enumerate() {
            let above = row.get(j + 1).copied().unwrap_or(usize::MAX - 1);
            let left = row.get(j).copied().unwrap_or(usize::MAX - 1);
            let cell = (diagonal + usize::from(ca != *cb)).min(above + 1).min(left + 1);
            diagonal = above;
            if let Some(slot) = row.get_mut(j + 1) {
                *slot = cell;
            }
        }
    }
    row.last().copied().unwrap_or(0)
}

/// The English catalog, read once per compiled crate, or why it could not be read.
fn catalog() -> &'static Result<Catalog, String> {
    static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let from = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from).ok_or("msg!: CARGO_MANIFEST_DIR is not set")?;
        let root = from
            .ancestors()
            .take(MAX_ANCESTORS)
            .find(|dir| dir.join("assets/locales/en.json").is_file())
            .ok_or_else(|| format!("msg!: no assets/locales/en.json in {} or the {MAX_ANCESTORS} folders above it", from.display()))?;
        read_catalog(root)
    })
}

/// The English messages under the workspace `root`, as `crates/ui-egui/build.rs` merges them.
fn read_catalog(root: &Path) -> Result<Catalog, String> {
    let mut catalog = Catalog::new();
    let app = root.join("assets/locales/en.json");
    let shared = root.join("crates/ui-text/locales/en.json");
    add(&mut catalog, &app, object(&app)?)?;
    if shared.is_file() {
        add(&mut catalog, &shared, object(&shared)?)?;
    }
    let pending = root.join("assets/locales/pending");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&pending)
        .map(|entries| entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "json")).collect())
        .unwrap_or_default();
    files.sort();
    for file in files {
        if let Some(serde_json::Value::Object(english)) = object(&file)?.remove("en") {
            add(&mut catalog, &file, english)?;
        }
    }
    Ok(catalog)
}

/// The JSON object in `path`.
fn object(path: &Path) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("msg!: {}: {e}", path.display()))?;
    match serde_json::from_str(&text).map_err(|e| format!("msg!: {}: {e}", path.display()))? {
        serde_json::Value::Object(map) => Ok(map),
        _ => Err(format!("msg!: {}: a catalog must be a JSON object", path.display())),
    }
}

/// Add `messages` (id to text) of `path` to `catalog`.
fn add(catalog: &mut Catalog, path: &Path, messages: serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    for (id, text) in messages {
        let serde_json::Value::String(text) = text else {
            return Err(format!("msg!: {}: `{id}` is not a string", path.display()));
        };
        catalog.insert(id, text);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_the_names_in_braces_each_once() {
        assert_eq!(placeholders("Saved {name} to {folder}, {name} again"), ["name", "folder"]);
        assert!(placeholders("No names here").is_empty());
        // Braces around anything but a name are text.
        assert!(placeholders("a set {} and {1, 2} and an open { brace").is_empty());
        assert_eq!(placeholders("{a_1}{b}"), ["a_1", "b"]);
    }

    #[test]
    fn the_nearest_id_is_offered_only_when_it_is_close() {
        let catalog: Catalog = ["ui.file", "ui.files", "ui.preferences"].into_iter().map(|id| (id.to_owned(), String::new())).collect();
        assert_eq!(nearest(&catalog, "ui.fiel"), Some("ui.file"));
        assert_eq!(nearest(&catalog, "ui.preference"), Some("ui.preferences"));
        assert_eq!(nearest(&catalog, "render.something_else"), None);
        assert_eq!((edits("", "abc"), edits("abc", "abc"), edits("kitten", "sitting")), (3, 0, 3));
    }

    /// The repository's own English catalog reads, with the messages of the app, of the shared
    /// crate and of the pending files.
    #[test]
    fn the_repositorys_catalog_reads() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let catalog = read_catalog(&root).unwrap();
        assert!(catalog.contains_key("ui.file"));
        assert!(catalog.len() > 1000);
        assert!(read_catalog(&root.join("crates")).is_err(), "a folder without the catalog says so");
    }
}
