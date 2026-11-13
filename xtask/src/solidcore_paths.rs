//! The path-level half of the solidcore seam (`cargo xtask layers`, issue #15031).
//!
//! The crate-level seam list (xtask/solidcore-seam.txt) lets a crate use the `solidcore` crate
//! at all; it says nothing about how much of the kernel's inside that crate names. This check
//! counts, per SolidCraft crate, every path into a solidcore crate that is not public
//! (xtask/solidcore-public.txt, or solidcore/public-crates.txt once solidcore owns it): each
//! `solidcore::a::b` in code, each leaf of a `use solidcore::a::{b, c::d}` tree, inside macro
//! calls too. Comments and strings are not code and do not count. solidcore's own tree
//! (solidcore/), vendor/ and build output are not scanned.
//!
//! The counts today are in [`BASELINE`], one crate a line, `<crate path> <count>`. A count may
//! only fall: the check fails when a crate names more paths than its line allows, and when it
//! names fewer, so the pull request that removes paths lowers the line to the new count (a crate
//! down to none drops its line), as the seam list and `tests/KNOWN_FAILING_TESTS` do.
//! `cargo xtask layers --paths [crate path]` lists the paths it counts.

use std::collections::BTreeMap;
use std::path::Path;

use proc_macro2::{Delimiter, TokenStream, TokenTree};

/// The committed per-crate counts.
pub const BASELINE: &str = "xtask/solidcore-paths.txt";

/// Top-level directories that are not SolidCraft's code: solidcore's own tree and third-party code.
const SKIPPED_TOP: &[&str] = &["solidcore", "vendor"];

/// One counted path: where it is and what it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub file: String,
    pub line: usize,
    pub path: String,
}

/// The names code uses for solidcore's crates (`solidcore`, `solidcore_ops`, ...): `solidcore`
/// and each `solidcore-*` package under solidcore/ (package names from its manifests).
pub fn solidcore_idents(manifests: &[(String, String)]) -> Vec<String> {
    let mut out = vec!["solidcore".to_owned()];
    for (_, text) in manifests {
        let Ok(doc) = text.parse::<toml::Table>() else { continue };
        let Some(name) = doc.get("package").and_then(|p| p.get("name")).and_then(toml::Value::as_str) else { continue };
        if name.starts_with("solidcore-") {
            out.push(name.replace('-', "_"));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Whether a full path (`solidcore::eval::plan`) is public. A public-list entry is a crate name
/// (`solidcore-ops`, matching every path into that crate) or a module path (`solidcore::foo`,
/// matching that module and everything under it).
fn is_public(path: &str, public: &[String]) -> bool {
    let krate = path.split("::").next().unwrap_or(path).replace('_', "-");
    public.iter().any(|p| if p.contains("::") { path == p || path.starts_with(&format!("{p}::")) } else { *p == krate })
}

/// Keywords a crate-root path (`::solidcore::x`) can follow; any other identifier before `::` is a
/// module, so `x::solidcore` is not the crate.
const PATH_KEYWORDS: &[&str] = &[
    "as", "box", "break", "const", "dyn", "else", "for", "if", "impl", "in", "let", "match", "move", "mut", "pub", "ref", "return", "static", "type",
    "unsafe", "use", "where", "while", "yield",
];

fn is_path_sep(tokens: &[TokenTree], i: usize) -> bool {
    matches!((tokens.get(i), tokens.get(i + 1)), (Some(TokenTree::Punct(a)), Some(TokenTree::Punct(b)))
        if a.as_char() == ':' && b.as_char() == ':' && a.spacing() == proc_macro2::Spacing::Joint)
}

/// The paths a `use` brace group expands to, each appended to `prefix`.
fn use_group_leaves(prefix: &str, group: TokenStream, out: &mut Vec<String>) {
    let tokens: Vec<TokenTree> = group.into_iter().collect();
    for item in tokens.split(|t| matches!(t, TokenTree::Punct(p) if p.as_char() == ',')) {
        if item.is_empty() {
            continue;
        }
        let mut path = prefix.to_owned();
        let mut i = 0;
        let mut nested = false;
        while i < item.len() {
            match &item[i] {
                TokenTree::Ident(id) if id == "as" => break,
                TokenTree::Ident(id) => path = format!("{path}::{id}"),
                TokenTree::Punct(p) if p.as_char() == '*' => path = format!("{path}::*"),
                TokenTree::Group(g) if g.delimiter() == Delimiter::Brace => {
                    use_group_leaves(&path, g.stream(), out);
                    nested = true;
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        if !nested {
            // `self` names the prefix module itself.
            out.push(path.strip_suffix("::self").map_or(path.clone(), str::to_owned));
        }
    }
}

/// Every path into a solidcore crate in a token stream, with its line: `solidcore::a::b`, and
/// each leaf of a `solidcore::a::{b, c}` tree. Recurses into groups (macro bodies included).
fn scan_tokens(stream: TokenStream, idents: &[String], out: &mut Vec<(usize, String)>) {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            TokenTree::Group(g) => scan_tokens(g.stream(), idents, out),
            TokenTree::Ident(id) if idents.iter().any(|n| id == n) => {
                // `x::solidcore::...` is some other module of that name, not the crate.
                // A leading `::solidcore` is still the crate.
                let after_sep = i >= 2 && is_path_sep(&tokens, i - 2);
                let rooted = !after_sep
                    || !(i >= 3
                        && match &tokens[i - 3] {
                            TokenTree::Ident(seg) => !PATH_KEYWORDS.iter().any(|k| seg == k),
                            TokenTree::Group(_) => true,
                            _ => false,
                        });
                let line = id.span().start().line;
                let mut path = id.to_string();
                let mut j = i + 1;
                let mut leaves = Vec::new();
                while is_path_sep(&tokens, j) {
                    match tokens.get(j + 2) {
                        Some(TokenTree::Ident(seg)) => {
                            path = format!("{path}::{seg}");
                            j += 3;
                        }
                        Some(TokenTree::Punct(p)) if p.as_char() == '*' => {
                            path = format!("{path}::*");
                            j += 3;
                            break;
                        }
                        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {
                            use_group_leaves(&path, g.stream(), &mut leaves);
                            j += 3;
                            break;
                        }
                        // `solidcore::<T>` and the like: the path ends here.
                        _ => break,
                    }
                }
                if rooted {
                    let is_use_of_crate =
                        j == i + 1 && matches!(tokens.get(i.wrapping_sub(1)), Some(TokenTree::Ident(u)) if u == "use" || u == "crate");
                    if !leaves.is_empty() {
                        out.extend(leaves.into_iter().map(|l| (line, l)));
                    } else if j > i + 1 || is_use_of_crate {
                        // A bare `solidcore` counts only as `use solidcore [as x]` / `extern crate solidcore`.
                        out.push((line, path));
                    }
                }
                i = j;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
}

/// The non-public solidcore paths one Rust source names, with their lines.
pub fn file_paths(source: &str, idents: &[String], public: &[String]) -> Result<Vec<(usize, String)>, String> {
    let stream: TokenStream = source.parse().map_err(|e| format!("cannot tokenize Rust source: {e:?}"))?;
    let mut all = Vec::new();
    scan_tokens(stream, idents, &mut all);
    all.retain(|(_, p)| !is_public(p, public));
    Ok(all)
}

/// The crate a file belongs to: its nearest ancestor directory with a Cargo.toml (relative to the
/// root, `.` for the root itself).
fn owning_crate(root: &Path, file: &Path) -> String {
    let mut dir = file.parent();
    while let Some(d) = dir {
        if d.join("Cargo.toml").is_file() {
            let rel = d.strip_prefix(root).unwrap_or(d).to_string_lossy().replace('\\', "/");
            return if rel.is_empty() { ".".to_owned() } else { rel };
        }
        if d == root {
            break;
        }
        dir = d.parent();
    }
    ".".to_owned()
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for e in entries {
        let e = e.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        let ft = e.file_type().map_err(|e| format!("{}: {e}", path.display()))?;
        if ft.is_dir() {
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            if dir == root && SKIPPED_TOP.contains(&name.as_str()) {
                continue;
            }
            walk(root, &path, out)?;
        } else if ft.is_file() && name.ends_with(".rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// Every non-public solidcore path SolidCraft's code names, by crate path.
pub fn scan(root: &Path, idents: &[String], public: &[String]) -> Result<BTreeMap<String, Vec<Site>>, String> {
    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    files.sort();
    let mut by_crate: BTreeMap<String, Vec<Site>> = BTreeMap::new();
    for f in files {
        let source = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        if !source.contains("solidcore") {
            continue;
        }
        let rel = f.strip_prefix(root).unwrap_or(&f).to_string_lossy().replace('\\', "/");
        let paths = file_paths(&source, idents, public).map_err(|e| format!("{rel}: {e}"))?;
        if paths.is_empty() {
            continue;
        }
        let sites = by_crate.entry(owning_crate(root, &f)).or_default();
        sites.extend(paths.into_iter().map(|(line, path)| Site { file: rel.clone(), line, path }));
    }
    Ok(by_crate)
}

/// The baseline's `crate count` lines.
fn parse_baseline(text: &str) -> (BTreeMap<String, usize>, Vec<String>) {
    let mut counts = BTreeMap::new();
    let mut errors = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        match (it.next(), it.next().map(str::parse::<usize>), it.next()) {
            (Some(c), Some(Ok(0)), None) => errors.push(format!("{BASELINE}:{}: `{c}` has a count of 0: remove its line", n + 1)),
            (Some(c), Some(Ok(k)), None) => {
                if counts.insert(c.to_owned(), k).is_some() {
                    errors.push(format!("{BASELINE}:{}: `{c}` is listed twice", n + 1));
                }
            }
            _ => errors.push(format!("{BASELINE}:{}: expected `<crate path> <count>`", n + 1)),
        }
    }
    (counts, errors)
}

/// The ratchet: each crate's count against its baseline line. Returns the messages, sorted.
pub fn check(counts: &BTreeMap<String, usize>, baseline: &str) -> Vec<String> {
    let (allowed, mut errors) = parse_baseline(baseline);
    let crates: std::collections::BTreeSet<&String> = counts.keys().chain(allowed.keys()).collect();
    for c in crates {
        let now = counts.get(c).copied().unwrap_or(0);
        let was = allowed.get(c).copied().unwrap_or(0);
        if now > was {
            errors.push(format!(
                "{c} names {now} paths inside solidcore, {} more than {BASELINE} allows ({was}): call solidcore-ops or solidcore-tess instead (solidcore docs/layers.md, #15031); `cargo xtask layers --paths {c}` lists them",
                now - was
            ));
        } else if now < was && now == 0 {
            errors.push(format!("{c} no longer names any path inside solidcore: remove its line from {BASELINE} in this pull request"));
        } else if now < was {
            errors.push(format!(
                "{c} now names {now} paths inside solidcore (was {was}): lower its line in {BASELINE} to `{c} {now}` in this pull request"
            ));
        }
    }
    errors.sort();
    errors
}

/// The counts of a scan.
pub fn counts(sites: &BTreeMap<String, Vec<Site>>) -> BTreeMap<String, usize> {
    sites.iter().map(|(c, s)| (c.clone(), s.len())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public() -> Vec<String> {
        ["solidcore-ops", "solidcore-tess", "solidcore-record", "solidcore-step"].map(str::to_owned).to_vec()
    }

    fn idents() -> Vec<String> {
        ["solidcore", "solidcore_convert", "solidcore_ops", "solidcore_tess"].map(str::to_owned).to_vec()
    }

    fn paths(src: &str) -> Vec<String> {
        file_paths(src, &idents(), &public()).unwrap().into_iter().map(|(_, p)| p).collect()
    }

    #[test]
    fn use_trees_count_each_leaf() {
        let src = "use solidcore::blend::fillet_executive::p3::{ErrorInfo, FitKind, NoRoomReason};\n\
                   use solidcore::eval::{plan::basis::{basis_derivatives, local_basis}, self};\n\
                   use solidcore::geom::*;";
        assert_eq!(
            paths(src),
            [
                "solidcore::blend::fillet_executive::p3::ErrorInfo",
                "solidcore::blend::fillet_executive::p3::FitKind",
                "solidcore::blend::fillet_executive::p3::NoRoomReason",
                "solidcore::eval::plan::basis::basis_derivatives",
                "solidcore::eval::plan::basis::local_basis",
                "solidcore::eval",
                "solidcore::geom::*",
            ]
        );
    }

    #[test]
    fn expression_type_and_macro_paths_count() {
        let src = "fn f(x: solidcore::Body) -> solidcore::topo::Face {\n\
                   let y = ::solidcore::eval::point(&x);\n\
                   assert!(solidcore::check::valid(&y));\n\
                   solidcore::topo::Face { id: 1 }\n}";
        assert_eq!(
            paths(src),
            ["solidcore::Body", "solidcore::topo::Face", "solidcore::eval::point", "solidcore::check::valid", "solidcore::topo::Face"]
        );
    }

    #[test]
    fn public_crates_comments_strings_and_other_modules_do_not_count() {
        let src = "use solidcore_ops::Order;\nuse solidcore_tess::{a, b};\n\
                   // solidcore::eval::plan in a comment\n/// docs: solidcore::x\n\
                   const S: &str = \"solidcore::eval\";\nuse crate::solidcore::local;\nlet solidcore = 1;\n\
                   solidcore_paths::scan(); use super::solidcore_extra::X;";
        assert!(paths(src).is_empty(), "{:?}", paths(src));
    }

    #[test]
    fn non_public_solidcore_crates_and_crate_aliases_count() {
        let src = "use solidcore_convert::Order;\nuse solidcore as kernel;\nextern crate solidcore;";
        assert_eq!(paths(src), ["solidcore_convert::Order", "solidcore", "solidcore"]);
        assert_eq!(paths("use ::solidcore::eval::Foo;\nfn f() -> impl ::solidcore::T { x }"), ["solidcore::eval::Foo", "solidcore::T"]);
    }

    #[test]
    fn public_list_may_name_a_module() {
        let mut p = public();
        p.push("solidcore::api".to_owned());
        let got = file_paths("use solidcore::api::{A, b::C}; use solidcore::apis::D;", &idents(), &p).unwrap();
        assert_eq!(got, [(1, "solidcore::apis::D".to_owned())]);
    }

    fn counts_of(pairs: &[(&str, usize)]) -> BTreeMap<String, usize> {
        pairs.iter().map(|(c, n)| ((*c).to_owned(), *n)).collect()
    }

    #[test]
    fn idents_come_from_solidcore_packages() {
        let m = |n: &str| (String::new(), format!("[package]\nname = \"{n}\"\n"));
        let got = solidcore_idents(&[m("solidcore"), m("solidcore-ops"), m("goldens"), (String::new(), "[workspace]\n".into())]);
        assert_eq!(got, ["solidcore", "solidcore_ops"]);
    }

    #[test]
    fn ratchet_passes_at_baseline() {
        assert!(check(&counts_of(&[("crates/smlib", 370), ("crates/geom", 6)]), "# x\ncrates/smlib 370\ncrates/geom 6\n").is_empty());
    }

    #[test]
    fn ratchet_fails_when_a_count_goes_up() {
        let e = check(&counts_of(&[("crates/smlib", 371), ("crates/geom", 6)]), "crates/smlib 370\ncrates/geom 6\n");
        assert_eq!(e.len(), 1);
        assert!(e[0].contains("crates/smlib names 371") && e[0].contains("1 more"), "{e:?}");
        // a crate with no line at all is allowed none
        let e = check(&counts_of(&[("crates/kernel", 1)]), "");
        assert_eq!(e.len(), 1, "{e:?}");
    }

    #[test]
    fn ratchet_fails_until_a_lower_count_is_written_down() {
        let e = check(&counts_of(&[("crates/smlib", 360)]), "crates/smlib 370\ncrates/geom 6\n");
        assert_eq!(e.len(), 2, "{e:?}");
        assert!(e.iter().any(|m| m.contains("`crates/smlib 360`")));
        assert!(e.iter().any(|m| m.contains("crates/geom no longer") && m.contains("remove its line")));
    }

    #[test]
    fn baseline_lines_are_checked() {
        let e = check(&BTreeMap::new(), "crates/smlib\ncrates/geom 1 2\ncrates/a 1\ncrates/a 1\n");
        // three malformed or doubled lines, and crates/a (1) no longer names any path
        assert_eq!(e.len(), 4, "{e:?}");
        let e = check(&BTreeMap::new(), "crates/smlib 0\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("count of 0"), "{e:?}");
    }
}
