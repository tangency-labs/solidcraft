//! `cargo xtask doc-tests`: the doc tests (`cargo test --doc`) of the workspace libraries that
//! have any.
//!
//! `cargo test --doc` builds every library it is given before rustdoc reads a doc comment, so a
//! library without a doc test still costs its build. This reads the sources first and finds the
//! code blocks rustdoc would compile the way rustdoc finds them: an item's doc comments and
//! `#[doc = "..."]` attributes joined and unindented, parsed as Markdown by pulldown-cmark
//! (rustdoc's parser), each block's info string read by rustdoc's rules. Only the libraries with
//! such a block are built and tested.
//!
//! A doubt costs a build, never a skipped test. What cannot be read counts as a doc test: a doc
//! attribute that is not a string literal (`#[doc = include_str!(..)]`), a block doc comment, a
//! file that does not tokenise, a symbolic link among the sources. Docs written elsewhere are read
//! where they are written:
//! - files brought in from outside the package (`#[path]`, `include!("..")`, a `[lib] path`) are
//!   read like its own;
//! - a `macro_rules!` macro that writes a doc from an argument (`#[doc = $text]`), or hands its
//!   arguments on to one, has the strings it is called with read as docs, under any name it is
//!   imported as;
//! - code a build script writes (`include!(concat!(env!("OUT_DIR"), ..))`) is judged by what
//!   writes it, the build script and the workspace crates it builds with: a `///` line they write
//!   whole is read as a doc; one they leave open or start with a `{}` placeholder, doc text as an
//!   attribute (`#[doc` or `doc =` in a string, `#[doc ..]` in macro input such as `quote!`), or
//!   a build dependency from outside the workspace that writes Rust source (one that uses
//!   proc-macro2, quote, syn or prettyplease) counts as a doc test;
//! - a function-like macro given a path (`bindgen!({ path: "wit" })`) has the `///` comments of
//!   the files there read as docs.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use proc_macro2::{Delimiter, Group, TokenStream, TokenTree};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};

/// Crates for writing Rust source: a build dependency that uses one may write docs.
const CODE_WRITERS: [&str; 4] = ["prettyplease", "proc-macro2", "quote", "syn"];

/// Macros that read no file named in their arguments; those strings are not looked up.
const READ_NO_FILE: [&str; 24] = [
    "assert",
    "assert_eq",
    "assert_ne",
    "concat",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "env",
    "eprint",
    "eprintln",
    "format",
    "format_args",
    "include",
    "include_bytes",
    "include_str",
    "matches",
    "option_env",
    "panic",
    "print",
    "println",
    "todo",
    "unreachable",
    "write",
    "writeln",
];

/// `cargo xtask doc-tests [--list] [-p NAME]... [CARGO ARGS] [-- TEST ARGS]`; `--list` only names
/// the libraries it would test.
pub fn run(args: &[&str]) -> Result<(), String> {
    let (args, test_args) = args.iter().position(|a| *a == "--").map_or((args, &[][..]), |i| (&args[..i], &args[i + 1..]));
    let mut only = Vec::new();
    let mut cargo_args = Vec::new();
    let mut list = false;
    let mut rest = args.iter().copied();
    while let Some(arg) = rest.next() {
        if arg == "-p" || arg == "--package" {
            only.push(rest.next().ok_or_else(|| format!("{arg} needs a package name"))?);
        } else if let Some(name) = arg.strip_prefix("--package=") {
            only.push(name);
        } else if arg == "--list" {
            list = true;
        } else {
            cargo_args.push(arg);
        }
    }
    match command((!only.is_empty()).then_some(&only[..]), &cargo_args, test_args)? {
        Some((what, c)) if !list => crate::run(c, &what),
        _ => Ok(()),
    }
}

/// `cargo test --profile ci --doc` for the workspace libraries (of `only`, when given) with a code
/// block rustdoc would compile, and its label; `None`, after saying so, when none has one.
/// `--ignored` or `--include-ignored` in `test_args` count the blocks marked `ignore` too.
pub fn command(only: Option<&[&str]>, cargo_args: &[&str], test_args: &[&str]) -> Result<Option<(String, Command)>, String> {
    let include_ignored = test_args.iter().any(|a| *a == "--ignored" || *a == "--include-ignored");
    let libraries = libraries(&crate::metadata()?)?;
    // Every library is read, one thread each, even with `only`: a macro one defines may write docs
    // in another. A thread that fails leaves its library counted as having doc tests.
    let read: Vec<(&Library, Vec<FileDocs>)> = std::thread::scope(|scope| {
        let threads: Vec<_> = libraries.iter().map(|library| (library, scope.spawn(move || read_library(library, include_ignored)))).collect();
        threads
            .into_iter()
            .map(|(library, thread)| (library, thread.join().unwrap_or_else(|_| vec![FileDocs { test: true, ..FileDocs::default() }])))
            .collect()
    });
    let doc_macros = doc_macros(read.iter().flat_map(|(_, files)| files));
    let mut code_writers = None;
    let mut considered = 0;
    let mut chosen = Vec::new();
    for (library, files) in &read {
        if only.is_some_and(|only| !only.contains(&library.name.as_str())) {
            continue;
        }
        considered += 1;
        if has_doc_test(library, files, &doc_macros, &mut code_writers, include_ignored)? {
            chosen.push(library.name.as_str());
        }
    }
    if chosen.is_empty() {
        eprintln!("doc tests: none of {considered} libraries has a code block rustdoc would compile, so none is built");
        return Ok(None);
    }
    eprintln!("doc tests: {} of {considered} libraries have a code block rustdoc would compile: {}", chosen.len(), chosen.join(", "));
    let mut c = crate::cargo();
    c.args(["test", "--profile", "ci", "--doc"]);
    for name in &chosen {
        c.args(["-p", name]);
    }
    c.args(cargo_args).arg("--").args(test_args);
    Ok(Some((format!("doc tests ({} libraries)", chosen.len()), c)))
}

/// A workspace library rustdoc tests, and where its docs come from.
struct Library {
    name: String,
    /// Its package directory, whose .rs files are all read.
    dir: PathBuf,
    /// Its root file (`[lib] path`), whose directory is read too when it lies outside `dir`.
    root: PathBuf,
    /// What writes its build-time code: its build script, and the directories of the workspace
    /// crates it builds with.
    writers: Vec<PathBuf>,
    /// The crates from outside the workspace it builds with.
    external_writers: Vec<String>,
}

/// The workspace packages with a library rustdoc tests (`doctest` in cargo metadata).
fn libraries(meta: &serde_json::Value) -> Result<Vec<Library>, String> {
    let packages = meta["packages"].as_array().ok_or("cargo metadata: no packages array")?;
    let mut out = Vec::new();
    for p in packages {
        let (Some(name), Some(manifest)) = (p["name"].as_str(), p["manifest_path"].as_str()) else { continue };
        let Some(dir) = Path::new(manifest).parent() else { continue };
        let targets = p["targets"].as_array().map(Vec::as_slice).unwrap_or_default();
        let Some(root) = targets.iter().find(|t| t["doctest"] == true).and_then(|t| t["src_path"].as_str()) else { continue };
        let mut writers: Vec<PathBuf> = targets
            .iter()
            .filter(|t| t["kind"].as_array().is_some_and(|kinds| kinds.iter().any(|k| k == "custom-build")))
            .filter_map(|t| t["src_path"].as_str().map(PathBuf::from))
            .collect();
        let mut external_writers = Vec::new();
        for dependency in p["dependencies"].as_array().map(Vec::as_slice).unwrap_or_default() {
            if dependency["kind"] != "build" {
                continue;
            }
            match (dependency["path"].as_str(), dependency["name"].as_str()) {
                (Some(path), _) => writers.push(PathBuf::from(path)),
                (None, Some(name)) => external_writers.push(name.to_string()),
                (None, None) => {}
            }
        }
        out.push(Library { name: name.to_string(), dir: dir.to_path_buf(), root: PathBuf::from(root), writers, external_writers });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// What one source file's docs hold.
#[derive(Debug, Default)]
struct FileDocs {
    /// One of its docs has a code block rustdoc would compile, or cannot be read.
    test: bool,
    /// The `macro_rules!` macros it defines that write a doc from an argument (`#[doc = $text]`).
    doc_macros: Vec<String>,
    /// (macro, other): a `macro_rules!` macro it defines hands an argument on to another macro.
    forwards: Vec<(String, String)>,
    /// (name, other name): `name as other name`, as in an import.
    aliases: Vec<(String, String)>,
    /// The string literals it passes to macros that could hold a code block, with each macro's
    /// name: docs, if that macro writes them from its arguments.
    macro_literals: Vec<(String, String)>,
    /// The files its `#[path = "..."]` attributes and `include!("...")` calls name.
    paths: Vec<String>,
    /// The strings in its macro calls that may name a file or directory the macro reads.
    referenced: Vec<String>,
    /// It includes code its build writes (`include!(concat!(env!("OUT_DIR"), ..))`).
    generated: bool,
}

/// Whether a library has a doc test: in its files' docs (with what they bring in), in strings it
/// passes to a macro that writes docs from them, or in code its build writes.
fn has_doc_test(
    library: &Library,
    files: &[FileDocs],
    doc_macros: &BTreeSet<String>,
    code_writers: &mut Option<BTreeSet<String>>,
    include_ignored: bool,
) -> Result<bool, String> {
    if files.iter().any(|f| f.test)
        || files.iter().flat_map(|f| &f.macro_literals).any(|(name, text)| doc_macros.contains(name) && literal_has_doc_test(text, include_ignored))
    {
        return Ok(true);
    }
    if !files.iter().any(|f| f.generated) {
        return Ok(false);
    }
    if !library.external_writers.is_empty() && code_writers.is_none() {
        *code_writers = Some(code_writer_packages()?);
    }
    let external = code_writers.as_ref().is_some_and(|set| library.external_writers.iter().any(|name| set.contains(name)));
    Ok(external || writers_have_doc_test(library, include_ignored))
}

/// Whether a string literal a macro writes a doc from gives it a code block, either alone (as
/// rustdoc unindents it) or with its indentation kept (as beside `///` comments).
fn literal_has_doc_test(text: &str, include_ignored: bool) -> bool {
    compiles_code(&markdown(&[(text, false)]), include_ignored) || compiles_code(&format!("{text}\n"), include_ignored)
}

/// The macros that write docs from their arguments: those that do it themselves, those that hand
/// their arguments on to one, and the names they are imported as.
fn doc_macros<'a>(files: impl Iterator<Item = &'a FileDocs> + Clone) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = files.clone().flat_map(|f| f.doc_macros.iter().cloned()).collect();
    loop {
        let before = out.len();
        for f in files.clone() {
            let edges = f.forwards.iter().map(|(caller, callee)| (callee, caller)).chain(f.aliases.iter().map(|(name, alias)| (name, alias)));
            for (from, to) in edges {
                if out.contains(from) {
                    out.insert(to.clone());
                }
            }
        }
        if out.len() == before {
            return out;
        }
    }
}

/// Every .rs file of a library's package (and of its root's directory, when that lies outside),
/// with what its `#[path]` attributes and `include!("...")` calls bring in, read; a path a macro is
/// given has its files' `///` comments read as docs. What cannot be read counts as a doc test.
fn read_library(library: &Library, include_ignored: bool) -> Vec<FileDocs> {
    let doubt = || FileDocs { test: true, ..FileDocs::default() };
    let Ok(dir) = std::fs::canonicalize(&library.dir) else { return vec![doubt()] };
    // Files are listed and joined by their plain paths (a canonical Windows path, `\\?\C:\..`,
    // does not resolve `..` joined to it); canonical paths are only compared.
    let mut roots = vec![library.dir.clone()];
    if let Some(parent) = library.root.parent()
        && std::fs::canonicalize(parent).map_or(true, |parent| !parent.starts_with(&dir))
    {
        roots.push(parent.to_path_buf());
    }
    let mut files = Vec::new();
    for root in &roots {
        if files_under(root, true, &mut files).is_err() {
            return vec![doubt()];
        }
    }
    // The .rs files under the roots are all listed; what else a file brings in is read once.
    let canonical_roots: Vec<PathBuf> = roots.iter().filter_map(|root| std::fs::canonicalize(root).ok()).collect();
    let listed = |path: &Path| path.extension().is_some_and(|e| e == "rs") && canonical_roots.iter().any(|root| path.starts_with(root));
    let mut brought_in = BTreeSet::new();
    let mut looked_up = BTreeSet::new();
    let mut out = Vec::new();
    while let Some(file) = files.pop() {
        let mut docs = std::fs::read_to_string(&file).map_or_else(|_| doubt(), |text| file_docs(&text, include_ignored));
        let base = file.parent().unwrap_or(&library.dir).to_path_buf();
        let own = base.join(file.file_stem().unwrap_or_default());
        for path in &docs.paths {
            // Relative to the file's directory, or to its own module directory for an attribute
            // in an inline module: either may be the one.
            for candidate in [base.join(path), own.join(path)] {
                if let Ok(canonical) = std::fs::canonicalize(&candidate)
                    && canonical.is_file()
                    && !listed(&canonical)
                    && brought_in.insert(canonical)
                {
                    files.push(candidate);
                }
            }
        }
        // A macro reads a path relative to the package (`CARGO_MANIFEST_DIR`) or to the file. The
        // package itself and the directories above it are not what a macro is pointed at.
        for path in &docs.referenced {
            for candidate in [library.dir.join(path), base.join(path)] {
                if looked_up.insert(candidate.clone())
                    && candidate.exists()
                    && let Ok(canonical) = std::fs::canonicalize(&candidate)
                    && !dir.starts_with(&canonical)
                    && comments_have_doc_test(&canonical, include_ignored)
                {
                    docs.test = true;
                }
            }
        }
        out.push(docs);
    }
    out
}

/// The files under `dir` (only .rs ones with `rust_only`), skipping build output; a symbolic link
/// is an error, since what it leads to is not read.
fn files_under(dir: &Path, rust_only: bool, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| format!("{}: {e}", path.display()))?;
        if kind.is_symlink() {
            return Err(format!("{}: a symbolic link", path.display()));
        } else if kind.is_dir() {
            if entry.file_name() != "target" {
                files_under(&path, rust_only, out)?;
            }
        } else if !rust_only || path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// Whether the `///` comments of a file, or of the files under a directory, hold a code block
/// rustdoc would compile, read as docs (what a macro given that path may copy into Rust). Rust
/// sources are read as such, and files that are not text are skipped.
fn comments_have_doc_test(path: &Path, include_ignored: bool) -> bool {
    let mut files = Vec::new();
    if path.is_dir() {
        if files_under(path, false, &mut files).is_err() {
            return true;
        }
    } else {
        files.push(path.to_path_buf());
    }
    files.iter().filter(|f| f.extension().is_none_or(|e| e != "rs")).any(|f| {
        std::fs::read_to_string(f).is_ok_and(|text| {
            comment_runs(&text).iter().any(|run| {
                let fragments: Vec<(&str, bool)> = run.iter().map(|line| (*line, true)).collect();
                compiles_code(&markdown(&fragments), include_ignored)
            })
        })
    })
}

/// The runs of consecutive `///` or `//!` comment lines in a text, without their markers.
fn comment_runs(text: &str) -> Vec<Vec<&str>> {
    let mut runs: Vec<Vec<&str>> = Vec::new();
    let mut open = false;
    for line in text.lines() {
        let line = line.trim_start();
        match line.strip_prefix("///").or_else(|| line.strip_prefix("//!")) {
            Some(doc) => {
                if !open {
                    runs.push(Vec::new());
                }
                open = true;
                if let Some(run) = runs.last_mut() {
                    run.push(doc);
                }
            }
            None => open = false,
        }
    }
    runs
}

/// The packages in the dependency graph that write Rust source: one of `CODE_WRITERS`, or a
/// package that depends on one.
fn code_writer_packages() -> Result<BTreeSet<String>, String> {
    let out = crate::cargo().args(["metadata", "--format-version", "1"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))?;
    let packages = meta["packages"].as_array().ok_or("cargo metadata: no packages array")?;
    let writes = |p: &serde_json::Value| {
        CODE_WRITERS.iter().any(|w| p["name"] == *w)
            || p["dependencies"]
                .as_array()
                .is_some_and(|deps| deps.iter().any(|d| d["kind"].is_null() && CODE_WRITERS.iter().any(|w| d["name"] == *w)))
    };
    Ok(packages.iter().filter(|p| writes(p)).filter_map(|p| p["name"].as_str().map(str::to_string)).collect())
}

/// Whether what writes a library's build-time code (its build script and the workspace crates it
/// builds with) could write a doc test, by the strings and macro input in their sources.
fn writers_have_doc_test(library: &Library, include_ignored: bool) -> bool {
    let mut files = Vec::new();
    for writer in &library.writers {
        if writer.is_dir() {
            if files_under(writer, true, &mut files).is_err() {
                return true;
            }
        } else {
            files.push(writer.clone());
        }
    }
    files.iter().any(|file| std::fs::read_to_string(file).map_or(true, |text| source_writes_doc_test(&text, include_ignored)))
}

/// Whether Rust source that writes code could write a doc test (the module docs say how).
fn source_writes_doc_test(source: &str, include_ignored: bool) -> bool {
    let verdict = source.parse::<TokenStream>().map_or(true, |tokens| writes_doc_test(tokens, None, include_ignored));
    // Spans index a per-thread copy of every file tokenised; none outlives this one.
    proc_macro2::extra::invalidate_current_thread_spans();
    verdict
}

/// `writes_doc_test` over `tokens`, the arguments of the macro `in_macro` if any.
fn writes_doc_test(tokens: TokenStream, in_macro: Option<&str>, include_ignored: bool) -> bool {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut i = 0;
    while i < trees.len() {
        if let Some((attribute, _, next)) = attribute(&trees, i) {
            // In macro input (`quote!`) an attribute is code being written; elsewhere it is the
            // writer's own.
            if in_macro.is_some() && matches!(attribute.stream().into_iter().next(), Some(TokenTree::Ident(key)) if key == "doc") {
                return true;
            }
            i = next;
            continue;
        }
        match (&trees[i], trees.get(i + 1), trees.get(i + 2)) {
            (TokenTree::Literal(lit), _, _) => {
                if let Some(text) = crate::shared::string_literal(lit) {
                    // `writeln!` ends the line its format string leaves open.
                    let text = if matches!(in_macro, Some("writeln" | "println" | "eprintln")) { text + "\n" } else { text };
                    if text_writes_doc_test(&text, include_ignored) {
                        return true;
                    }
                }
            }
            (TokenTree::Ident(name), Some(TokenTree::Punct(bang)), Some(TokenTree::Group(args))) if bang.as_char() == '!' => {
                if writes_doc_test(args.stream(), Some(&name.to_string()), include_ignored) {
                    return true;
                }
                i += 3;
                continue;
            }
            (TokenTree::Group(group), _, _) if writes_doc_test(group.stream(), in_macro, include_ignored) => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

/// Whether a string a writer writes could write a doc test: doc text as an attribute (`#[doc`,
/// `doc =`), a `///` line it leaves open or starts with a `{}` placeholder, or whole `///` lines
/// holding a code block. (`//!` cannot reach `include!`d code, where inner attributes are refused.)
fn text_writes_doc_test(text: &str, include_ignored: bool) -> bool {
    if text.contains("#[doc") || text.contains("doc =") || text.contains("doc=") {
        return true;
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let mut doc = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(at) = line.find("///") else { continue };
        let rest = line.get(at + 3..).unwrap_or_default();
        if i + 1 == lines.len() || rest.trim_start().starts_with('{') {
            return true;
        }
        doc.push((rest, true));
    }
    compiles_code(&markdown(&doc), include_ignored)
}

/// The docs of one source file.
fn file_docs(source: &str, include_ignored: bool) -> FileDocs {
    let mut out = FileDocs::default();
    match source.parse::<TokenStream>() {
        Ok(tokens) => {
            let lines: Vec<&str> = source.lines().collect();
            walk(tokens, &lines, include_ignored, &mut out);
        }
        Err(_) => out.test = true,
    }
    // Spans index a per-thread copy of every file tokenised; none outlives this one.
    proc_macro2::extra::invalidate_current_thread_spans();
    out
}

/// Read the docs of every run of adjacent attributes in `tokens` (one item's), at every depth,
/// macro bodies and arguments included, with the macros, imports and paths around them.
fn walk(tokens: TokenStream, lines: &[&str], include_ignored: bool, out: &mut FileDocs) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut docs = Vec::new();
    let mut inner_run = None;
    let mut i = 0;
    while i < trees.len() {
        if let Some((attribute, inner, next)) = attribute(&trees, i) {
            // An item's outer docs and the inner docs of what follows are separate docs.
            if inner_run.is_some_and(|run| run != inner) {
                out.test |= docs_have_test(&docs, include_ignored);
                docs.clear();
            }
            inner_run = Some(inner);
            let parts: Vec<TokenTree> = attribute.stream().into_iter().collect();
            read_attribute(&parts, written(&trees[i], lines), &mut docs, &mut out.paths);
            i = next;
            continue;
        }
        inner_run = None;
        if !docs.is_empty() {
            out.test |= docs_have_test(&docs, include_ignored);
            docs.clear();
        }
        match (&trees[i], trees.get(i + 1), trees.get(i + 2), trees.get(i + 3)) {
            (TokenTree::Ident(name), Some(TokenTree::Punct(bang)), Some(TokenTree::Ident(defined)), Some(TokenTree::Group(body)))
                if bang.as_char() == '!' && *name == "macro_rules" =>
            {
                let defined = defined.to_string();
                if writes_doc_from_argument(body.stream()) {
                    out.doc_macros.push(defined.clone());
                }
                forwards(body.stream(), &defined, &mut out.forwards);
            }
            (TokenTree::Ident(name), Some(TokenTree::Punct(bang)), Some(TokenTree::Group(args)), _) if bang.as_char() == '!' => {
                let name = name.to_string();
                if name == "include" {
                    include(args.stream(), out);
                } else if !READ_NO_FILE.contains(&name.as_str()) {
                    referenced(args.stream(), &mut out.referenced);
                }
                collect_literals(args.stream(), &name, &mut out.macro_literals);
            }
            // `name as other`: an import under another name (or a cast, which does no harm).
            (TokenTree::Ident(name), Some(TokenTree::Ident(keyword)), Some(TokenTree::Ident(other)), _) if *keyword == "as" => {
                out.aliases.push((name.to_string(), other.to_string()));
            }
            _ => {}
        }
        if let TokenTree::Group(group) = &trees[i] {
            walk(group.stream(), lines, include_ignored, out);
        }
        i += 1;
    }
    if !docs.is_empty() {
        out.test |= docs_have_test(&docs, include_ignored);
    }
}

/// The bracketed group of the attribute (`#[...]` or `#![...]`) starting at `i`, whether it is an
/// inner one, and the index after it.
fn attribute(trees: &[TokenTree], i: usize) -> Option<(&Group, bool, usize)> {
    let TokenTree::Punct(hash) = trees.get(i)? else { return None };
    if hash.as_char() != '#' {
        return None;
    }
    let inner = matches!(trees.get(i + 1)?, TokenTree::Punct(bang) if bang.as_char() == '!');
    let at = i + 1 + usize::from(inner);
    match trees.get(at)? {
        TokenTree::Group(group) if group.delimiter() == Delimiter::Bracket => Some((group, inner, at + 1)),
        _ => None,
    }
}

/// How an attribute was written. proc-macro2 turns a doc comment into `#[doc = "..."]` spanning
/// the comment, so the source at its `#` tells.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Written {
    LineComment,
    BlockComment,
    Attribute,
}

fn written(hash: &TokenTree, lines: &[&str]) -> Written {
    let start = hash.span().start();
    let text: String =
        start.line.checked_sub(1).and_then(|l| lines.get(l)).map(|l| l.chars().skip(start.column).take(3).collect()).unwrap_or_default();
    match text.as_str() {
        "///" | "//!" => Written::LineComment,
        "/**" | "/*!" => Written::BlockComment,
        _ => Written::Attribute,
    }
}

/// One doc fragment's text.
enum Text {
    Known(String),
    /// Written from a macro argument (`#[doc = $text]`): read from the macro's calls instead.
    Argument,
    Unreadable,
}

struct Fragment {
    text: Text,
    /// From a `///` or `//!` comment, which rustdoc unindents a little differently.
    sugared: bool,
}

/// The doc fragments (`doc = ...`, also inside `cfg_attr`) and module paths (`path = "..."`) of one
/// attribute's contents.
fn read_attribute(parts: &[TokenTree], written: Written, docs: &mut Vec<Fragment>, paths: &mut Vec<String>) {
    match parts {
        [TokenTree::Ident(key), TokenTree::Punct(eq), value @ ..] if *key == "doc" && eq.as_char() == '=' => {
            let text = match value {
                _ if written == Written::BlockComment => Text::Unreadable,
                [TokenTree::Literal(lit)] => crate::shared::string_literal(lit).map_or(Text::Unreadable, Text::Known),
                [TokenTree::Punct(dollar), TokenTree::Ident(_)] if dollar.as_char() == '$' => Text::Argument,
                _ => Text::Unreadable,
            };
            docs.push(Fragment { text, sugared: written == Written::LineComment });
        }
        [TokenTree::Ident(key), TokenTree::Punct(eq), TokenTree::Literal(lit)] if *key == "path" && eq.as_char() == '=' => {
            paths.extend(crate::shared::string_literal(lit));
        }
        [TokenTree::Ident(key), TokenTree::Group(args)] if *key == "cfg_attr" => {
            // `cfg_attr(predicate, attribute, ...)`: each attribute after the predicate.
            for attribute in split_commas(args.stream()).iter().skip(1) {
                read_attribute(attribute, written, docs, paths);
            }
        }
        _ => {}
    }
}

/// `stream` split at its top-level commas.
fn split_commas(stream: TokenStream) -> Vec<Vec<TokenTree>> {
    let mut parts = vec![Vec::new()];
    for tree in stream {
        if matches!(&tree, TokenTree::Punct(p) if p.as_char() == ',') {
            parts.push(Vec::new());
        } else if let Some(part) = parts.last_mut() {
            part.push(tree);
        }
    }
    parts
}

/// An `include!` call: a file named by a literal is read like the package's own, one named from
/// `OUT_DIR` is code the build writes, and any other argument cannot be followed.
fn include(args: TokenStream, out: &mut FileDocs) {
    let trees: Vec<TokenTree> = args.into_iter().collect();
    match &trees[..] {
        [TokenTree::Literal(lit)] => match crate::shared::string_literal(lit) {
            Some(path) => out.paths.push(path),
            None => out.test = true,
        },
        _ if mentions(&trees, "OUT_DIR") => out.generated = true,
        _ => out.test = true,
    }
}

/// Whether `trees` hold the string literal `text`, at any depth.
fn mentions(trees: &[TokenTree], text: &str) -> bool {
    trees.iter().any(|tree| match tree {
        TokenTree::Literal(lit) => crate::shared::string_literal(lit).is_some_and(|s| s == text),
        TokenTree::Group(group) => mentions(&group.stream().into_iter().collect::<Vec<_>>(), text),
        _ => false,
    })
}

/// Whether a `macro_rules!` body writes a doc from one of its arguments (`doc = $text`).
fn writes_doc_from_argument(body: TokenStream) -> bool {
    let trees: Vec<TokenTree> = body.into_iter().collect();
    trees.iter().enumerate().any(|(i, tree)| match tree {
        TokenTree::Ident(id) if *id == "doc" => matches!(
            (trees.get(i + 1), trees.get(i + 2)),
            (Some(TokenTree::Punct(eq)), Some(TokenTree::Punct(dollar))) if eq.as_char() == '=' && dollar.as_char() == '$'
        ),
        TokenTree::Group(group) => writes_doc_from_argument(group.stream()),
        _ => false,
    })
}

/// The macros a `macro_rules!` body calls with one of its own arguments (`other!($text)`).
fn forwards(body: TokenStream, caller: &str, out: &mut Vec<(String, String)>) {
    let trees: Vec<TokenTree> = body.into_iter().collect();
    for (i, tree) in trees.iter().enumerate() {
        if let (TokenTree::Ident(callee), Some(TokenTree::Punct(bang)), Some(TokenTree::Group(args))) = (tree, trees.get(i + 1), trees.get(i + 2))
            && bang.as_char() == '!'
            && has_metavariable(args.stream())
        {
            out.push((caller.to_string(), callee.to_string()));
        }
        if let TokenTree::Group(group) = tree {
            forwards(group.stream(), caller, out);
        }
    }
}

/// Whether macro input refers to a metavariable (`$name`).
fn has_metavariable(stream: TokenStream) -> bool {
    stream.into_iter().any(|tree| match tree {
        TokenTree::Punct(p) => p.as_char() == '$',
        TokenTree::Group(group) => has_metavariable(group.stream()),
        _ => false,
    })
}

/// The string literals in a macro call's arguments that could hold a code block, with the macro's
/// name.
fn collect_literals(args: TokenStream, name: &str, out: &mut Vec<(String, String)>) {
    for tree in args {
        match tree {
            TokenTree::Literal(lit) => {
                if let Some(text) = crate::shared::string_literal(&lit).filter(|text| could_hold_code(text)) {
                    out.push((name.to_string(), text));
                }
            }
            TokenTree::Group(group) => collect_literals(group.stream(), name, out),
            _ => {}
        }
    }
}

/// The strings in a macro call's arguments that may name a file or directory.
fn referenced(args: TokenStream, out: &mut Vec<String>) {
    for tree in args {
        match tree {
            TokenTree::Literal(lit) => {
                if let Some(text) = crate::shared::string_literal(&lit).filter(|t| !t.is_empty() && t.len() < 260 && !t.contains(char::is_whitespace))
                {
                    out.push(text);
                }
            }
            TokenTree::Group(group) => referenced(group.stream(), out),
            _ => {}
        }
    }
}

/// Whether Markdown could hold a code block at all: one needs a fence or four columns of
/// indentation (in any container), so a tab or four spaces in a row.
fn could_hold_code(text: &str) -> bool {
    text.contains("```") || text.contains("~~~") || text.contains("    ") || text.contains('\t')
}

/// Whether one item's docs hold a code block rustdoc would compile, or cannot be read.
fn docs_have_test(docs: &[Fragment], include_ignored: bool) -> bool {
    let mut fragments = Vec::with_capacity(docs.len());
    for doc in docs {
        match &doc.text {
            Text::Known(text) => fragments.push((text.as_str(), doc.sugared)),
            // Read from the macro's calls (`has_doc_test`); here it only holds its place.
            Text::Argument => fragments.push(("", doc.sugared)),
            Text::Unreadable => return true,
        }
    }
    compiles_code(&markdown(&fragments), include_ignored)
}

/// An item's Markdown as rustdoc builds it from its doc fragments (text, from a `///` comment):
/// the fragments' lines, less the indentation all their non-blank lines share, where a `///`
/// comment's leading space counts once it is mixed with `#[doc]` attributes (rustdoc's
/// `unindent_doc_fragments`).
fn markdown(fragments: &[(&str, bool)]) -> String {
    let mixed = fragments.windows(2).any(|pair| pair[0].1 != pair[1].1) && fragments.iter().any(|&(_, sugared)| sugared);
    let add = usize::from(mixed);
    let blank = |line: &str| line.chars().all(char::is_whitespace);
    let indent = |line: &str| line.chars().take_while(|c| *c == ' ' || *c == '\t').count();
    let min = fragments
        .iter()
        .flat_map(|&(text, sugared)| text.lines().filter(|line| !blank(line)).map(move |line| indent(line) + if sugared { 0 } else { add }))
        .min()
        .unwrap_or(0);
    let mut out = String::new();
    for &(text, sugared) in fragments {
        if text.is_empty() {
            out.push('\n');
            continue;
        }
        let cut = if !sugared && min > 0 { min - add } else { min };
        for line in text.lines() {
            out.push_str(if blank(line) { line } else { line.get(cut..).unwrap_or(line) });
            out.push('\n');
        }
    }
    out
}

/// Whether rustdoc compiles a code block of `markdown`: an indented one, or a fenced one whose info
/// string says Rust. The parser options are rustdoc's.
fn compiles_code(markdown: &str, include_ignored: bool) -> bool {
    if !could_hold_code(markdown) {
        return false;
    }
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_SMART_PUNCTUATION;
    Parser::new_ext(markdown, options).any(|event| match event {
        Event::Start(Tag::CodeBlock(CodeBlockKind::Indented)) => true,
        Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => compiled(&info, include_ignored),
        _ => false,
    })
}

/// Whether rustdoc compiles a fenced block with this info string, read as rustdoc's `LangString`
/// reads it: Rust when it is empty, names Rust or holds one of rustdoc's own attributes
/// (`no_run`, `should_panic`, `edition2021`, ...); another language when it holds only other words
/// (`text`, `json`, `sh`). A block marked `ignore` is not compiled unless ignored tests are run.
/// Attribute syntax rustdoc may read in other ways (`{...}`, `=`, quotes) counts as Rust.
fn compiled(info: &str, include_ignored: bool) -> bool {
    if info.contains(['{', '}', '=', '"', '(', ')']) {
        return true;
    }
    let (mut rust, mut other, mut ignore_all, mut ignore_some) = (false, false, false, false);
    for word in info.split(|c: char| c == ',' || c.is_whitespace()).filter(|word| !word.is_empty()) {
        match word {
            "ignore" => ignore_all = true,
            "rust" | "should_panic" | "no_run" | "compile_fail" | "test_harness" | "standalone_crate" => rust = true,
            _ if word.starts_with("ignore-") => ignore_some = true,
            _ if word.starts_with("edition") || is_error_code(word) => rust = true,
            _ => other = true,
        }
    }
    let is_rust = rust || ignore_all || ignore_some || !other;
    is_rust && (include_ignored || !ignore_all || ignore_some)
}

/// A compiler error code, such as `E0277` (rustdoc reads these after `compile_fail`).
fn is_error_code(word: &str) -> bool {
    word.len() == 5 && word.starts_with('E') && word.bytes().skip(1).all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(source: &str) -> bool {
        file_docs(source, false).test
    }

    /// Whether a crate made of these files has a doc test in strings passed to doc-writing macros.
    fn found_in_calls(files: &[&str]) -> bool {
        let files: Vec<FileDocs> = files.iter().map(|source| file_docs(source, false)).collect();
        let macros = doc_macros(files.iter());
        files.iter().flat_map(|f| &f.macro_literals).any(|(name, text)| macros.contains(name) && literal_has_doc_test(text, false))
    }

    const DOCUMENTED: &str = "macro_rules! documented {\n    ($doc:literal) => {\n        #[doc = $doc]\n        pub struct S;\n    };\n}\n";

    #[test]
    fn rust_blocks_count_and_other_languages_do_not() {
        assert!(found("/// ```\n/// let x = 1;\n/// ```\nfn f() {}"));
        assert!(found("/// ```rust\n/// let x = 1;\n/// ```\nfn f() {}"));
        assert!(found("/// ```no_run\n/// let x = 1;\n/// ```\nfn f() {}"));
        assert!(found("//! ```should_panic\n//! let x = 1;\n//! ```\n"));
        assert!(found("/// ~~~\n/// let x = 1;\n/// ~~~\nfn f() {}"));
        assert!(!found("/// ```text\n/// output\n/// ```\nfn f() {}"));
        assert!(!found("/// ```json\n/// {}\n/// ```\nfn f() {}"));
        assert!(!found("/// Plain text, `code`, and a list:\n///\n/// - one\n/// - two\nfn f() {}"));
        // An unclosed fence runs to the end of the doc: still a block.
        assert!(found("/// ```\n/// let x = 1;\nfn f() {}"));
    }

    #[test]
    fn ignored_blocks_count_only_when_ignored_tests_run() {
        let source = "/// ```ignore\n/// let x = 1;\n/// ```\nfn f() {}";
        assert!(!file_docs(source, false).test);
        assert!(file_docs(source, true).test);
    }

    #[test]
    fn a_text_block_does_not_hide_a_rust_block_after_it() {
        assert!(found("/// ```text\n/// output\n/// ```\n///\n/// ```\n/// let x = 1;\n/// ```\nfn f() {}"));
        // Inline code at the start of a line is no fence, so the block after it is still Rust.
        assert!(found("/// ```text``` is inline code.\n///\n/// ```\n/// let x = 1;\n/// ```\nfn f() {}"));
        // Inside a longer fence, a shorter one is text.
        assert!(!found("/// ````text\n/// ```\n/// let x = 1;\n/// ```\n/// ````\nfn f() {}"));
    }

    #[test]
    fn indented_code_counts_once_the_shared_indent_is_gone() {
        assert!(found("/// Example:\n///\n///     let x = 1;\nfn f() {}"));
        assert!(found("/// - In a list:\n///\n///       let x = 1;\nfn f() {}"));
        // Every line indented alike: rustdoc removes it, so this is plain text.
        assert!(!found("///     one\n///     two\nfn f() {}"));
        // A continued paragraph is no code block.
        assert!(!found("/// A long line\n///     continued.\nfn f() {}"));
    }

    #[test]
    fn docs_are_read_per_item_from_comments_and_attributes() {
        assert!(found("#[doc = \"```\"]\n#[doc = \"let x = 1;\"]\n#[doc = \"```\"]\nfn f() {}"));
        assert!(found("#[cfg_attr(feature = \"x\", doc = \"```\\nlet x = 1;\\n```\")]\nfn f() {}"));
        assert!(found("mod m {\n    //! ```\n    //! let x = 1;\n    //! ```\n}\n"));
        assert!(!found("#[doc = \"Plain.\"]\n#[doc(hidden)]\n#[derive(Debug)]\nstruct S;"));
        // Comments and strings are not docs.
        assert!(!found("// ```\n// let x = 1;\n// ```\nconst S: &str = \"```\\nlet x = 1;\\n```\";\n"));
    }

    #[test]
    fn a_crates_inner_docs_and_the_next_items_docs_are_separate() {
        // Together, the open text fence would swallow the indented Rust block; rustdoc reads them
        // apart.
        assert!(found("//! ```text\n//! output\n/// Example:\n///\n///     let x = 1;\nfn f() {}"));
    }

    #[test]
    fn what_cannot_be_read_counts_as_a_doc_test() {
        assert!(found("#![doc = include_str!(\"../README.md\")]\n"));
        assert!(found("/** Plain. */\nfn f() {}"));
        assert!(found("fn f( {"));
        assert!(found("include!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/x.rs\"));\n"));
    }

    #[test]
    fn included_files_are_read_and_generated_code_is_noted() {
        let literal = file_docs("include!(\"part.rs\");\n", false);
        assert_eq!(literal.paths, ["part.rs"]);
        assert!(!literal.test && !literal.generated);
        let generated = file_docs("include!(concat!(env!(\"OUT_DIR\"), \"/api.rs\"));\n", false);
        assert!(generated.generated && !generated.test);
        assert_eq!(file_docs("#[path = \"../elsewhere.rs\"]\nmod elsewhere;\n", false).paths, ["../elsewhere.rs"]);
    }

    #[test]
    fn a_doc_written_from_a_macro_argument_is_read_from_the_calls() {
        let defined = file_docs(DOCUMENTED, false);
        assert_eq!(defined.doc_macros, ["documented"]);
        assert!(!defined.test);
        assert!(!found_in_calls(&[DOCUMENTED, "documented!(\"Plain    words.\");\n"]));
        assert!(found_in_calls(&[DOCUMENTED, "documented!(\"```\\nlet x = 1;\\n```\");\n"]));
        // A literal passed to a macro that writes no docs is no doc.
        assert!(!found_in_calls(&[DOCUMENTED, "println!(\"```\\nlet x = 1;\\n```\");\n"]));
    }

    #[test]
    fn a_macro_that_hands_its_argument_on_or_is_renamed_still_writes_docs() {
        let wrapper = "macro_rules! wrapper {\n    ($d:literal) => {\n        documented!($d);\n    };\n}\n";
        assert!(found_in_calls(&[DOCUMENTED, wrapper, "wrapper!(\"```\\nlet x = 1;\\n```\");\n"]));
        assert!(found_in_calls(&[DOCUMENTED, "use crate::documented as d;\nd!(\"```\\nlet x = 1;\\n```\");\n"]));
    }

    #[test]
    fn what_a_build_writes_is_judged_by_its_writer() {
        let no_docs = "fn main() { std::fs::write(\"x\", format!(\"pub const API: &str = {:?};\\n\", 1)).ok(); }";
        assert!(!source_writes_doc_test(no_docs, false));
        let whole_line = "fn main() { let s = String::from(\"/// The theme files (`{folder}`).\\npub const X: u8 = 1;\\n\"); }";
        assert!(!source_writes_doc_test(whole_line, false));
        let writeln = "fn main() { writeln!(out, \"/// Fixed text.\").ok(); }";
        assert!(!source_writes_doc_test(writeln, false));
        // A doc line from a value, one another string finishes, or an attribute: anything.
        assert!(source_writes_doc_test("fn main() { writeln!(out, \"/// {}\", doc).ok(); }", false));
        assert!(source_writes_doc_test("fn main() { out.push_str(\"/// \"); out.push_str(doc); }", false));
        assert!(source_writes_doc_test("fn main() { out.push_str(\"#[doc = \\\"x\\\"]\"); }", false));
        assert!(source_writes_doc_test("fn main() { let t = quote! { #[doc = #text] pub struct S; }; }", false));
        assert!(source_writes_doc_test("fn main() { out.push_str(\"/// ```\\n/// let x = 1;\\n/// ```\\n\"); }", false));
        // The writer's own docs and attributes are not what it writes.
        assert!(!source_writes_doc_test("/// ```\n/// let x = 1;\n/// ```\n#[doc = \"y\"]\nfn main() {}", false));
    }

    #[test]
    fn comment_lines_of_other_files_read_as_docs() {
        assert_eq!(comment_runs("/// one\n/// two\nx\n  //! three"), [vec![" one", " two"], vec![" three"]]);
    }

    #[test]
    fn info_strings_read_as_rustdoc_reads_them() {
        for (info, compiled_here) in [
            ("", true),
            ("rust", true),
            ("rust,no_run", true),
            ("should_panic", true),
            ("compile_fail,E0277", true),
            ("edition2021", true),
            ("ignore-wasm32", true),
            ("{.rust}", true),
            ("text", false),
            ("json", false),
            ("sh", false),
            ("ignore", false),
            ("rust,ignore", false),
        ] {
            assert_eq!(compiled(info, false), compiled_here, "{info:?}");
        }
        assert!(compiled("ignore", true));
    }

    #[test]
    fn unindenting_follows_rustdoc() {
        assert_eq!(markdown(&[(" one", true), ("     two", true)]), "one\n    two\n");
        // Mixed with a `#[doc]` attribute, a comment's leading space is the one taken off.
        assert_eq!(markdown(&[(" one", true), ("two", false)]), "one\ntwo\n");
        assert_eq!(markdown(&[(" one", true), ("", true), (" two", true)]), "one\n\ntwo\n");
    }
}
