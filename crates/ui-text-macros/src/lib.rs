//! SolidCraft's message ids checked when a crate compiles, and turned into their places.
//!
//! A message is looked up by its id (`app.tr("ui.file")`, `Locale::text`, `Locale::format`), and
//! an id the catalogs do not have compiles: the window then shows the id itself. [`msg!`] moves
//! that discovery to the compiler. It takes the id as a string literal, looks it up in the English
//! catalog and expands to the message's place among that catalog's sorted ids
//! (`solidcraft_ui_text::Msg`), so the id itself is not in the program:
//!
//! ```ignore
//! msg!("ui.file").tr()                                  // "File", in the current language
//! msg!("recovery.days_ago", count = &days).tr()         // "{count} days ago", filled in
//! ```
//!
//! - `msg!("id")`: the id is a message of the English catalog. A `Msg`.
//! - `msg!("id", name = value, ...)`: and the names are exactly the message's `{placeholders}`.
//!   A `Filled`, with its values (`&str`).
//!
//! Anything else stops the build at the literal, with the nearest id when there is one. Only the
//! English catalog is read: whether another language has the message stays with
//! `cargo xtask i18n` and the catalogs' tests, so a new English message is never held up by a
//! language its author cannot write.
//!
//! The English catalog is what the app builds it from (`crates/ui-egui/build.rs`):
//! `assets/locales/en.json`, `crates/ui-text/locales/en.json` and the `en` messages of
//! `assets/locales/pending/*.json`, found from the compiling crate's folder upwards. Cargo does
//! not know that a proc macro read a file, so a crate that uses `msg!` names the catalog folders
//! in its build script (`cargo:rerun-if-changed`), as `solidcraft-ui-egui` does, to be compiled
//! again when a message is added.
//!
//! With the place goes the fingerprint of the ids it was counted in
//! (`solidcraft_ui_text::numbered::fingerprint`, computed here by a copy of it: this crate
//! depends on no other of the workspace), so a table made for another list of ids refuses it.

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

/// `"id"`, then `name = value` for each placeholder, if any.
struct Input {
    id: LitStr,
    values: Option<Vec<(Ident, syn::Expr)>>,
}

impl Parse for Input {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let id: LitStr = input.parse()?;
        if input.is_empty() {
            return Ok(Input { id, values: None });
        }
        input.parse::<Token![,]>()?;
        let pair = |input: ParseStream<'_>| -> syn::Result<(Ident, syn::Expr)> {
            let name: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            Ok((name, input.parse()?))
        };
        let values = Punctuated::<(Ident, syn::Expr), Token![,]>::parse_terminated_with(input, pair)?;
        Ok(Input { id, values: Some(values.into_iter().collect()) })
    }
}

/// `msg!("id")` as tokens: the message's `Msg`, or with values its `Filled`.
fn message(input: &Input, unfilled: bool) -> syn::Result<proc_macro2::TokenStream> {
    let ((place, ids), values) = check(input, unfilled)?;
    let id = &input.id;
    let msg = quote!(::solidcraft_ui_text::Msg::checked(#place, #ids, #id));
    Ok(match values {
        None => msg,
        Some(values) => {
            let pairs = values.iter().map(|(name, value)| {
                let name = name.to_string();
                quote!((#name, #value))
            });
            quote!(::solidcraft_ui_text::Filled::checked(#msg, [#(#pairs),*]))
        }
    })
}

/// A message of the English catalog, by its place: a `Msg`, or with `name = value` for each
/// placeholder a `Filled`. See the crate's documentation. For the text itself, [`tr!`].
#[proc_macro]
pub fn msg(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as Input);
    message(&input, false).unwrap_or_else(|e| e.to_compile_error()).into()
}

/// A reader, if any, then what [`msg!`] takes.
struct Read {
    reader: Option<syn::Expr>,
    message: Input,
}

impl Parse for Read {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        if input.peek(LitStr) {
            return Ok(Read { reader: None, message: input.parse()? });
        }
        let reader: syn::Expr = input.parse()?;
        input.parse::<Token![,]>()?;
        Ok(Read { reader: Some(reader), message: input.parse()? })
    }
}

/// The text of a message of the English catalog, named by its id.
///
/// - `tr!("ui.file")`: in this thread's language, a `&'static str`.
/// - `tr!("recovery.days_ago", count = &days)`: with its placeholders filled, a `String`.
/// - `tr!(app, "ui.file")`, `tr!(locale, "recovery.days_ago", count = &days)`: in the language of
///   `app` or `locale`, which is anything with `tr(Msg)` and `fill(Filled)`.
///
/// The id is checked as [`msg!`] checks it and is not in the program. An id built at run time
/// cannot be given: that is `tr_by_id`.
#[proc_macro]
pub fn tr(input: TokenStream) -> TokenStream {
    let read = syn::parse_macro_input!(input as Read);
    let message = match message(&read.message, false) {
        Ok(message) => message,
        Err(e) => return e.to_compile_error().into(),
    };
    match (read.reader, read.message.values.is_some()) {
        (None, _) => quote!(#message.tr()),
        (Some(reader), false) => quote!((#reader).tr(#message)),
        (Some(reader), true) => quote!((#reader).fill(#message)),
    }
    .into()
}

/// The text of a message as the catalog has it, placeholders unfilled: for the few places that
/// want the template itself (a hint that shows `{design}-{body}` as written, a template handed
/// to a component that fills it). Checked and numbered as [`tr!`] does; `template!("id")` in this
/// thread's language, `template!(app, "id")` in the reader's. Anywhere else, `tr!` with values.
#[proc_macro]
pub fn template(input: TokenStream) -> TokenStream {
    let read = syn::parse_macro_input!(input as Read);
    if let Some((name, _)) = read.message.values.as_ref().and_then(|values| values.first()) {
        return syn::Error::new(name.span(), "`template!` gives the text unfilled: to fill it, use `tr!`").to_compile_error().into();
    }
    let message = match message(&read.message, true) {
        Ok(message) => message,
        Err(e) => return e.to_compile_error().into(),
    };
    match read.reader {
        None => quote!(#message.tr()),
        Some(reader) => quote!((#reader).tr(#message)),
    }
    .into()
}

/// `solidcraft_ui_text::numbered::fingerprint`: FNV-1a over the ids, each followed by a newline,
/// folded to 32 bits.
fn fingerprint<'a>(ids: impl IntoIterator<Item = &'a str>) -> u32 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in ids.into_iter().flat_map(|id| id.bytes().chain(std::iter::once(b'\n'))) {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash ^ (hash >> 32)) as u32
}

/// The message's place and its catalog's fingerprint, with the values given (`None` for a
/// message given without values), or why the message is refused.
#[allow(clippy::type_complexity)]
fn check(input: &Input, unfilled: bool) -> syn::Result<((u32, u32), Option<&[(Ident, syn::Expr)]>)> {
    let (catalog, ids) = catalog().as_ref().map_err(|why| syn::Error::new(input.id.span(), why))?;
    let ids = *ids;
    let id = input.id.value();
    let Some(text) = catalog.get(&id) else {
        let hint = nearest(catalog, &id).map(|near| format!("; did you mean `{near}`?")).unwrap_or_default();
        let message = format!(
            "`{id}` is not a message id of the English catalog (assets/locales/en.json, crates/ui-text/locales/en.json or a file of assets/locales/pending){hint}"
        );
        return Err(syn::Error::new(input.id.span(), message));
    };
    // A `BTreeMap`'s keys are in the order of their bytes, as the tables' ids are.
    let place = u32::try_from(catalog.range::<str, _>((std::ops::Bound::Unbounded, std::ops::Bound::Excluded(id.as_str()))).count())
        .map_err(|_| syn::Error::new(input.id.span(), "the English catalog has too many messages"))?;
    let wanted = placeholders(text);
    let Some(values) = &input.values else {
        return match wanted.first() {
            None => Ok(((place, ids), None)),
            Some(_) if unfilled => Ok(((place, ids), None)),
            Some(first) => Err(syn::Error::new(
                input.id.span(),
                format!("`{id}` has the placeholder `{{{first}}}`: give it a value, `{first} = ...`; its text is {text:?}"),
            )),
        };
    };
    if let Some((extra, _)) = values.iter().find(|(name, _)| !wanted.contains(&name.to_string())) {
        return Err(syn::Error::new(extra.span(), format!("`{id}` has no placeholder `{{{extra}}}`: its text is {text:?}")));
    }
    if let Some(missing) = wanted.iter().find(|name| !values.iter().any(|(given, _)| given == name)) {
        return Err(syn::Error::new(input.id.span(), format!("`{id}` also has the placeholder `{{{missing}}}`: its text is {text:?}")));
    }
    Ok(((place, ids), Some(values)))
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

/// The English catalog and the fingerprint of its ids, read once per compiled crate, or why it
/// could not be read.
fn catalog() -> &'static Result<(Catalog, u32), String> {
    static CATALOG: OnceLock<Result<(Catalog, u32), String>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let from = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from).ok_or("msg!: CARGO_MANIFEST_DIR is not set")?;
        let root = from
            .ancestors()
            .take(MAX_ANCESTORS)
            .find(|dir| dir.join("assets/locales/en.json").is_file())
            .ok_or_else(|| format!("msg!: no assets/locales/en.json in {} or the {MAX_ANCESTORS} folders above it", from.display()))?;
        let catalog = read_catalog(root)?;
        let ids = fingerprint(catalog.keys().map(String::as_str));
        Ok((catalog, ids))
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

    /// The same fingerprint as `solidcraft_ui_text::numbered::fingerprint` gives (its own test
    /// has this value).
    #[test]
    fn a_fingerprint_is_the_text_crates() {
        assert_eq!(fingerprint(["ui.cancel", "ui.file"]), 0x13f5_3633);
        assert_ne!(fingerprint(["ab", "c"]), fingerprint(["a", "bc"]));
    }

    fn refused(tokens: &str, unfilled: bool) -> String {
        let input: Input = syn::parse_str(tokens).unwrap();
        check(&input, unfilled).err().map(|e| e.to_string()).unwrap_or_default()
    }

    /// What `tr!`, `msg!` and `template!` refuse, and say about it. (The ids are the repository's
    /// own: `ui.file` has no placeholder, `recovery.days_ago` has `{count}`.)
    #[test]
    fn a_wrong_id_and_wrong_placeholders_are_refused_with_what_is_wrong() {
        assert_eq!(refused(r#""ui.file""#, false), "");
        assert_eq!(refused(r#""recovery.days_ago", count = x"#, false), "");
        // An id no catalog has, with the nearest one.
        let unknown = refused(r#""ui.fiel""#, false);
        assert!(unknown.contains("`ui.fiel` is not a message id") && unknown.contains("did you mean `ui.file`?"), "{unknown}");
        assert!(refused(r#""ui.no_message_has_this_id_at_all""#, false).contains("is not a message id"));
        // A placeholder left without a value, a name the message lacks, a name too many.
        assert!(refused(r#""recovery.days_ago""#, false).contains("has the placeholder `{count}`"));
        assert!(refused(r#""recovery.days_ago", days = x"#, false).contains("has no placeholder `{days}`"));
        assert!(refused(r#""recovery.restore_row", name = x"#, false).contains("also has the placeholder `{when}`"));
        assert!(refused(r#""ui.file", count = x"#, false).contains("has no placeholder `{count}`"));
        // `template!` takes a message with placeholders unfilled, and still wants a real id.
        assert_eq!(refused(r#""recovery.days_ago""#, true), "");
        assert!(refused(r#""recovery.days_agoo""#, true).contains("did you mean `recovery.days_ago`?"));
    }

    /// A message's place is its position among the catalog's ids in the order of their bytes,
    /// and every message of one catalog carries the same fingerprint.
    #[test]
    fn a_message_is_numbered_by_its_place_among_the_sorted_ids() {
        let (catalog, ids) = catalog().as_ref().unwrap();
        let place = |id: &str| {
            let input: Input = syn::parse_str(&format!("{id:?}")).unwrap();
            check(&input, true).unwrap().0
        };
        for (expected, id) in catalog.keys().enumerate().step_by(97) {
            assert_eq!(place(id), (expected as u32, *ids), "{id}");
        }
        assert_eq!(*ids, fingerprint(catalog.keys().map(String::as_str)));
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
